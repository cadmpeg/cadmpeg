// SPDX-License-Identifier: Apache-2.0
//! PSB container framing and structural inspection.
//!
//! A `.prt` begins with an ASCII header block (`#UGC:2 …` through
//! `#-END_OF_UGC_HEADER`), an ASCII table of contents (`#UGC_TOC` …
//! `#END_OF_TOC_HEADER`), then a sequence of named binary sections. A real body
//! section header is `#\n#<name>\n`. The preceding `#` terminator and printable
//! name distinguish section boundaries from similar bytes in feature data.
//!
//! [`scan`] reads the stream and returns a [`ContainerScan`] containing section
//! metadata, the ND or DEPDB layout, namespace counts, typed structural rows,
//! native loops, units, feature identifiers, and datum planes. [`summarize`]
//! converts that scan into the codec-neutral container summary.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{ContainerEntry, ContainerSummary};
use cadmpeg_ir::decode::{DecodeContext, View};

use crate::curve::{
    self, BoundPrototypePcurve, CurveExpressionRecord, CurveExpressionValue, CurveParameterRecord,
    CurvePrototype, CurvePrototypeTopology, CurveTopologyRow, DepdbCurveRow,
    ExternalRelationSymbols, Fc05Circle, Fc05CylinderCapPair, FcCurveCoordinates, PcurveEndpoints,
    PrototypePcurveEndpoints,
};
use crate::datum::{self, DatumPlane};
use crate::feature::{
    self, FeatureAffectedIds, FeatureChoice, FeatureChoiceField, FeatureDefinition, FeatureEntity,
    FeatureEntityReference, FeatureEntityTable, FeatureGeometryTable, FeatureLoopRestoreDirection,
    FeatureOperation, FeatureRecipe, FeatureReferenceName, FeatureReplayAffectedIds,
    FeatureRevolutionExtent, FeatureRow,
};
use crate::placement::{self, FeatureSectionTransform};
use crate::primdata::{self, PrimitiveScalarArray, PrimitiveTriangleStrip};
use crate::psb;
use crate::reference::{self, ReferenceCircle, ReferenceConic, ReferenceEllipse, ReferenceLine};
use crate::surface::{
    self, OutlinePlane, PlaneEnvelopeRecord, PlaneLocalSystem, SurfaceParameterRecord,
    SurfacePrototype, SurfacePrototypeRecord, SurfaceRow, TabulatedCylinderCurveReplay,
};
use crate::topology::{
    self, FaceComponent, HalfEdge, HalfEdgeVertexIncidence, Loop, TopologicalVertex,
};

/// The PSB magic: every Creo `.prt` opens with this ASCII framing line.
pub const MAGIC: &[u8] = b"#UGC:2";

/// End of the UGC header block.
const UGC_HEADER_END: &[u8] = b"#-END_OF_UGC_HEADER";
/// Start of the ASCII table of contents.
const TOC_START: &[u8] = b"#UGC_TOC";
/// End of the ASCII table of contents.
const TOC_END: &[u8] = b"#END_OF_TOC_HEADER";
/// JPEG SOI magic, marking the `THMB_IMG_MAIN` preview payload (never geometry).
const JPEG_MAGIC: &[u8] = &[0xff, 0xd8, 0xff];

/// ASCII names that appear in the header/TOC framing and look like section
/// headers but are structural markers, not binary sections ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)).
const FRAMING_NAMES: &[&str] = &[
    "-END_OF_UGC_HEADER",
    "END_OF_UGC",
    "UGC_TOC",
    "END_OF_TOC_HEADER",
    "NEXT_TOC_ENTRY",
];

/// Codec-defined role labels for [`ContainerEntry::role`], grouping sections by
/// what they carry ([spec §2.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#12-section-map)).
pub mod role {
    /// Primary/invisible PSB geometry (`VisibGeom`, `NovisGeom`).
    pub const GEOMETRY: &str = "psb-geometry";
    /// Feature rows, definitions, history, datums, body counts.
    pub const MODEL_DATA: &str = "model-data";
    /// Materials, display, persistence, and other auxiliary metadata.
    pub const METADATA: &str = "metadata";
    /// The JPEG thumbnail preview (`THMB_IMG_MAIN`), excluded from geometry.
    pub const THUMBNAIL: &str = "thumbnail";
    /// A section name this codec does not classify.
    pub const OPAQUE: &str = "opaque";
}

/// The visible-geometry section name whose `srf_array`/`crv_array` counts drive
/// the inspect census.
const VISIBGEOM: &str = "VisibGeom";
/// Named active-unit selector. Unit-definition tables can contain inactive
/// systems, so this selector, rather than another unit-name string, is
/// authoritative.
const PRINCIPAL_UNIT_ID: &[u8] = b"_principal_sys_units_id\0";

/// The two layout families ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). Dispatched structurally, not per-file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Dense PSB rows in `VisibGeom` (~40+ sections; `ND:` name decoration).
    Nd,
    /// Sparse PSB views plus a persistence database (`DEPDB_DATA`, ~12 sections).
    Depdb,
    /// Neither signature was conclusive.
    Unknown,
}

impl Layout {
    /// A short, stable token for reports and source attributes.
    pub fn token(self) -> &'static str {
        match self {
            Layout::Nd => "ND",
            Layout::Depdb => "DEPDB",
            Layout::Unknown => "unknown",
        }
    }
}

/// One enumerated binary section.
#[derive(Debug, Clone)]
pub struct Section {
    /// Section name with any `ND:0:` prefix / `ModelView#N` suffix stripped.
    pub name: String,
    /// Raw name as it appeared in the header, when decorated.
    pub raw_name: String,
    /// Byte offset of the section header within the file.
    pub offset: usize,
    /// Payload length in bytes (header to the next section, or EOF).
    pub length: usize,
    /// Expanded payload length from the TOC, excluding the section header.
    pub expanded_length: Option<usize>,
    /// Role classification.
    pub role: &'static str,
}

/// A section payload decoded from Unix `compress` framing.
#[derive(Debug, Clone)]
pub struct ExpandedSection {
    /// Normalized owning section name.
    pub name: String,
    /// Offset of the compressed payload in the source file.
    pub source_offset: usize,
    /// Number of compressed source bytes.
    pub compressed_length: usize,
    /// Complete expanded PSB payload.
    pub data: Vec<u8>,
}

/// One counted model-level `double_xar` dictionary from an expanded section.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelDoubleXarTable {
    /// Normalized owning section name.
    pub section_name: String,
    /// Source-file offset of the compressed section payload.
    pub section_source_offset: usize,
    /// Offset of the table label in the expanded section.
    pub expanded_offset: usize,
    /// Stored array extent.
    pub count: u32,
    /// Entries in stored order.
    pub entries: Vec<crate::scalar::DoubleXarEntry>,
}

/// The byte-backed count headers read from the visible-geometry section.
#[derive(Debug, Clone, Default)]
pub struct GeomCensus {
    /// `srf_array\0 f8 <count>` surface-namespace count, when present.
    pub srf_array_count: Option<u32>,
    /// `crv_array\0 [f3] f8 <count>` curve-namespace count, when present.
    pub crv_array_count: Option<u32>,
}

/// Configuration family-table pointer carried by `FamilyInf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyTablePointer {
    /// Explicit `e1` null pointer.
    Null,
    /// Canonical `f7` entity reference to a driver table.
    Entity(u32),
}

/// Typed `drv_tbl_ptr` field from `FamilyInf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilyTableRecord {
    /// Null or referenced driver table.
    pub pointer: FamilyTablePointer,
    /// Byte offset of the pointer value.
    pub offset: usize,
}

/// Structural data read from one `.prt` file. The 76 decoded products are
/// grouped into per-domain sub-structs so each consumer names the domain it
/// reads. `ContainerScan` is never serialized; grouping and field naming are
/// internal and do not affect IR or JSON output.
pub struct ContainerScan {
    /// Container framing: raw bytes, header, TOC-enumerated sections, and
    /// model-level diagnostics.
    pub framing: FramingScan,
    /// Named scalar and triangle-strip products from expanded primitive data.
    pub primitives: PrimitiveScan,
    /// Model-space reference entities decoded from `MdlRefInfo`.
    pub references: ReferenceScan,
    /// Typed surface rows, parameter bodies, and prototypes across the model,
    /// non-visible, and cross-section namespaces.
    pub surfaces: SurfaceScan,
    /// Plane support frames, envelopes, placed planes, and datum planes.
    pub planes: PlaneScan,
    /// Curve prototypes, parameter bodies, pcurves, and native curve rows.
    pub curves: CurveScan,
    /// Native half-edge adjacency graph resolved from curve topology rows.
    pub topology: TopologyScan,
    /// Feature rows, definitions, operations, and the implicit entity graph.
    pub features: FeatureScan,
}

/// Container framing: raw bytes, header, sections, and model-level diagnostics.
pub struct FramingScan {
    /// Complete source bytes.
    pub data: Vec<u8>,
    /// The magic/version header line, ASCII, trimmed.
    pub version_line: String,
    /// Native model filename from the length-prefixed `CMNM` header record.
    pub model_name: Option<String>,
    /// Byte offset of the native model filename.
    pub model_name_offset: Option<usize>,
    /// Enumerated sections in file order.
    pub sections: Vec<Section>,
    /// Successfully expanded Unix-compress section payloads.
    pub expanded_sections: Vec<ExpandedSection>,
    /// Identified layout family.
    pub layout: Layout,
    /// Visible-geometry namespace census, when a `VisibGeom` section was found.
    pub census: GeomCensus,
    /// Active Creo principal coordinate unit system, when its selector is
    /// present. Both currently defined systems store model lengths in mm.
    pub principal_unit: Option<String>,
    /// Configuration driver-table pointer from `FamilyInf`.
    pub family_table: Option<FamilyTableRecord>,
    /// Declared `Geomlists.n_bodies` cardinality, when present.
    pub declared_body_count: Option<u32>,
    /// `Geomlists.first_quilt_ptr`: zero denotes the single-quilt form;
    /// nonzero is a multi-quilt discriminator rather than a body count.
    pub first_quilt_ptr: Option<u32>,
}

/// Named products from expanded primitive-data sections.
pub struct PrimitiveScan {
    /// Counted model-level scalar dictionaries from expanded sections.
    pub double_xar_tables: Vec<ModelDoubleXarTable>,
    /// Complete named model-space scalar arrays from expanded primitive data.
    pub scalar_arrays: Vec<PrimitiveScalarArray>,
    /// Complete named position-only triangle strips from expanded primitive data.
    pub triangle_strips: Vec<PrimitiveTriangleStrip>,
}

/// Model-space reference entities decoded from `MdlRefInfo`.
pub struct ReferenceScan {
    /// Complete model-space line entities from `MdlRefInfo`.
    pub lines: Vec<ReferenceLine>,
    /// Complete model-Z circular entities from `MdlRefInfo` rows.
    pub circles: Vec<ReferenceCircle>,
    /// Named conic entities from `MdlRefInfo` with complete defining fields.
    pub conics: Vec<ReferenceConic>,
    /// Conic records whose complete fields independently define an ellipse.
    pub ellipses: Vec<ReferenceEllipse>,
}

/// Typed surface rows, parameter bodies, and prototypes.
pub struct SurfaceScan {
    /// Typed fixed-prefix surface rows from the selected material model
    /// geometry namespace. Parameter bodies are decoded separately.
    pub rows: Vec<SurfaceRow>,
    /// Typed fixed-prefix rows from the separate invisible and construction
    /// surface namespace.
    pub nonvisible_rows: Vec<SurfaceRow>,
    /// Typed fixed-prefix surface rows from the DEPDB cross-section geometry
    /// namespace. These are kept separate from model-face surface rows.
    pub cross_section_rows: Vec<SurfaceRow>,
    /// Bounded scalar parameter bodies from positional surface rows.
    pub parameters: Vec<SurfaceParameterRecord>,
    /// Bounded scalar parameter bodies from the separate invisible and
    /// construction surface namespace.
    pub nonvisible_parameters: Vec<SurfaceParameterRecord>,
    /// Bounded scalar parameter bodies from DEPDB cross-section surface rows.
    pub cross_section_parameters: Vec<SurfaceParameterRecord>,
    /// Labeled surface prototypes with fully decoded scalar fields.
    pub prototypes: Vec<SurfacePrototype>,
    /// Bounded named `srf_prim_ptr(<kind>)` parameter records.
    pub prototype_records: Vec<SurfacePrototypeRecord>,
    /// Bounded named surface-prototype records from the separate invisible
    /// and construction geometry namespace.
    pub nonvisible_prototype_records: Vec<SurfacePrototypeRecord>,
}

/// Plane support frames, envelopes, placed planes, and datum planes.
pub struct PlaneScan {
    /// Inherited support frames following positional plane envelopes.
    pub local_systems: Vec<PlaneLocalSystem>,
    /// Plane support frames from the DEPDB cross-section namespace.
    pub cross_section_local_systems: Vec<PlaneLocalSystem>,
    /// Plane-specific standard and compact positional envelopes.
    pub envelopes: Vec<PlaneEnvelopeRecord>,
    /// Plane envelopes from the DEPDB cross-section namespace.
    pub cross_section_envelopes: Vec<PlaneEnvelopeRecord>,
    /// Axis-aligned placed planes derived from unambiguous outline corners.
    pub outlines: Vec<OutlinePlane>,
    /// Axis-aligned planes from marker-bound six-scalar positional frames.
    pub positional_frames: Vec<OutlinePlane>,
    /// Placed planes derived inside the DEPDB cross-section namespace.
    pub cross_section_outlines: Vec<OutlinePlane>,
    /// Model-space standard datum planes decoded from `ActDatums` outlines.
    pub datums: Vec<DatumPlane>,
}

/// Curve prototypes, parameter bodies, pcurves, and native curve rows.
pub struct CurveScan {
    /// Cubic curve replay records bound to following tabulated-cylinder rows.
    pub tabulated_cylinder_replays: Vec<TabulatedCylinderCurveReplay>,
    /// Labeled curve prototypes from geometry sections. The curve body and
    /// its analytic interpretation are decoded separately.
    pub prototypes: Vec<CurvePrototype>,
    /// Labeled curve prototypes from the separate invisible and construction
    /// geometry namespace.
    pub nonvisible_prototypes: Vec<CurvePrototype>,
    /// Labeled first curve rows from DEPDB cross-section namespaces.
    pub cross_section_prototypes: Vec<CurvePrototype>,
    /// Source programs from curve-from-equation entity records.
    pub expressions: Vec<CurveExpressionRecord>,
    /// Bounded analytic parameter bodies from positional curve rows.
    pub parameters: Vec<CurveParameterRecord>,
    /// Bounded curve parameter bodies from the separate invisible and
    /// construction geometry namespace.
    pub nonvisible_parameters: Vec<CurveParameterRecord>,
    /// Complete eight-slot pcurve endpoints in both adjacent face frames.
    pub pcurves: Vec<PcurveEndpoints>,
    /// Ordered world-coordinate lanes from FC-prefixed dense curve rows.
    pub fc_coordinates: Vec<FcCurveCoordinates>,
    /// FC05 records whose decoded points prove an exact circle.
    pub fc05_circles: Vec<Fc05Circle>,
    /// Cylinder cap groups joined through typed curve-face topology. Their
    /// model-space feature frame remains required before IR transfer.
    pub fc05_cylinder_cap_pairs: Vec<Fc05CylinderCapPair>,
    /// Complete pcurve UV endpoints from labeled curve prototypes.
    pub prototype_pcurves: Vec<PrototypePcurveEndpoints>,
    /// Labeled face and next-edge references from curve prototypes.
    pub prototype_topology: Vec<CurvePrototypeTopology>,
    /// Prototype pcurve endpoints bound to their adjacent face identifiers.
    pub bound_prototype_pcurves: Vec<BoundPrototypePcurve>,
    /// Curve rows with an unambiguous canonical four-reference topology
    /// suffix. These rows define the native half-edge adjacency graph.
    pub topology_rows: Vec<CurveTopologyRow>,
    /// Curve rows from the separate invisible and construction geometry
    /// namespace. These rows do not participate in model topology.
    pub nonvisible_topology_rows: Vec<CurveTopologyRow>,
    /// Complete one-sided curve rows from the DEPDB cross-section namespace.
    pub cross_section_rows: Vec<DepdbCurveRow>,
}

/// Native half-edge adjacency graph resolved from curve topology rows.
pub struct TopologyScan {
    /// Resolved native half-edges and closed loops built from curve rows.
    pub half_edges: Vec<HalfEdge>,
    /// Closed rings of half-edges, one per resolved face loop.
    pub loops: Vec<Loop>,
    /// Connected components of non-null face references in native curve
    /// topology. These are not emitted IR shells.
    pub face_components: Vec<FaceComponent>,
    /// Topological vertex identities derived from half-edge orbits.
    pub vertices: Vec<TopologicalVertex>,
    /// Start/end vertex binding for each decoded half-edge.
    pub half_edge_vertex_incidence: Vec<HalfEdgeVertexIncidence>,
}

/// Feature rows, definitions, operations, and the implicit entity graph.
pub struct FeatureScan {
    /// Feature IDs that own decoded geometry rows.
    pub ids: Vec<u32>,
    /// Byte-bounded `AllFeatur` rows for known geometry-owning features.
    pub rows: Vec<FeatureRow>,
    /// Section-bounded procedural recipe rows synthesized from `DEPDB_DATA`.
    pub depdb_recipe_rows: Vec<FeatureRow>,
    /// Labeled procedural-choice spans inside decoded feature rows.
    pub choices: Vec<FeatureChoice>,
    /// Named fields and typed wrappers inside procedural-choice spans.
    pub choice_fields: Vec<FeatureChoiceField>,
    /// Generated-geometry namespace headers owned by decoded features.
    pub geometry_tables: Vec<FeatureGeometryTable>,
    /// Complete named affected-ID arrays owned by decoded features.
    pub affected_ids: Vec<FeatureAffectedIds>,
    /// Affected-ID runs from unlabeled positional replay feature rows.
    pub replay_affected_ids: Vec<FeatureReplayAffectedIds>,
    /// Named compact direction values from loop-restoration records.
    pub loop_restore_directions: Vec<FeatureLoopRestoreDirection>,
    /// Resolved angular termination from rotational feature rows.
    pub revolution_extents: Vec<FeatureRevolutionExtent>,
    /// Byte-bounded `FeatDefs` records and definition-space parameter frames.
    pub definitions: Vec<FeatureDefinition>,
    /// Section-to-model frames resolved from perpendicular active datums.
    pub section_transforms: Vec<FeatureSectionTransform>,
    /// Every stored feature-operation state from `MdlStatus`, in byte order.
    pub operation_states: Vec<FeatureOperation>,
    /// Current feature-operation state for each feature identifier.
    pub operations: Vec<FeatureOperation>,
    /// Feature names joined to model feature identifiers by reference data.
    pub reference_names: Vec<FeatureReferenceName>,
    /// Named records in the implicit `AllFeatur` walker-order entity table.
    pub entities: Vec<FeatureEntity>,
    /// Canonical `f7` references between implicit `AllFeatur` entities.
    pub entity_references: Vec<FeatureEntityReference>,
    /// Mixed generated-entity tables from `AllFeatur`, with owner bindings
    /// retained only where their containing feature row is byte-bounded.
    pub entity_tables: Vec<FeatureEntityTable>,
}

/// Whether a byte prefix is a Creo PSB `.prt`: the `#UGC:2` ASCII magic is the
/// container signature ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). Detection is magic-based, never
/// extension-based, because `.prt` is shared with Siemens NX ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)).
pub fn looks_like_creo(prefix: &[u8]) -> bool {
    prefix.starts_with(MAGIC)
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn line_at(data: &[u8], start: usize) -> String {
    let end = find(data, b"\n", start).unwrap_or(data.len());
    String::from_utf8_lossy(&data[start..end])
        .trim()
        .to_string()
}

/// Normalize a decorated section name to its base ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)): strip a
/// `ModelView#N` suffix and an `ND:0:<Name>:N` decoration.
fn normalize_name(raw: &str) -> String {
    let base = raw.split('#').next().unwrap_or(raw);
    if let Some(rest) = base.strip_prefix("ND:") {
        // ND:0:Name:N  ->  parts = ["0", "Name", "N"]; the name is index 1.
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() >= 2 {
            return parts[1].to_string();
        }
    }
    base.to_string()
}

/// Classify a normalized section name by what it carries ([spec §2.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#12-section-map)).
fn classify(name: &str) -> &'static str {
    match name {
        "VisibGeom" | "NovisGeom" | "ActDatums" => role::GEOMETRY,
        "AllFeatur" | "FeatDefs" | "FeatDefsIndex" | "FeatDefsDtm" | "Geomlists" | "GeomDepen"
        | "Model_L05_PX" | "Model_L05P" | "BasicData" | "BasBasData" | "BasFullData"
        | "FullMData" => role::MODEL_DATA,
        "THMB_IMG_MAIN" => role::THUMBNAIL,
        "NeuPrtSld" | "NeuAsmSld" | "SolidPersistTable" | "SolidPrimdata" | "DEPDB_DATA"
        | "UnitSystemDef_L03" | "PDMTrail_L03" | "ActEntity" | "MdlStatus" | "MdlRefInfo"
        | "DispCntrl" | "ColorSchemeInfo" | "LargeText" | "BasicText" | "IdsGenInfoDb" => {
            role::METADATA
        }
        _ => role::OPAQUE,
    }
}

/// Enumerate binary sections from `body_start` to EOF by the `\n#<name>\n`
/// header rule ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). A candidate header is accepted only when its name is
/// a printable run and is not one of the header/TOC framing markers.
fn scan_sections(data: &[u8], body_start: usize) -> Vec<Section> {
    // Collect header hits as (offset_of_section_hash, raw_name).
    let mut hits: Vec<(usize, String)> = Vec::new();
    let search_start = body_start.saturating_sub(1);
    let mut i = search_start;
    while i + 1 < data.len() {
        let toc_delimited = data[i] == 0xf1 && data[i + 1] == b'#';
        if !toc_delimited && (data[i] != b'\n' || data[i + 1] != b'#') {
            i += 1;
            continue;
        }
        let hash_off = i + 1; // offset of the section-header '#'
        let name_start = i + 2;
        let Some(nl) = find(data, b"\n", name_start) else {
            break;
        };
        let name_bytes = &data[name_start..nl];
        i = nl; // continue scanning after this line regardless of acceptance
                // A real section name is a printable run with at least one alphanumeric
                // character; this rejects TOC/EOF padding lines made only of `#`.
        if !name_bytes.iter().all(|&b| is_name_byte(b))
            || name_bytes.len() < 2
            || !name_bytes.iter().any(u8::is_ascii_alphanumeric)
        {
            continue;
        }
        let raw = String::from_utf8_lossy(name_bytes).to_string();
        if FRAMING_NAMES.contains(&raw.as_str()) {
            continue;
        }
        if toc_delimited {
            let directory_end = hits
                .first()
                .map_or(body_start.min(data.len()), |(offset, _)| *offset);
            if !toc_lists_section(&data[..directory_end], name_bytes) {
                continue;
            }
        }
        hits.push((hash_off, raw));
    }

    let mut sections = Vec::with_capacity(hits.len());
    for (idx, (hdr_off, raw)) in hits.iter().enumerate() {
        let end = hits.get(idx + 1).map_or(data.len(), |(next, _)| *next);
        let name = normalize_name(raw);
        let role = classify(&name);
        sections.push(Section {
            name,
            raw_name: raw.clone(),
            offset: *hdr_off,
            length: end.saturating_sub(*hdr_off),
            expanded_length: None,
            role,
        });
    }
    sections
}

fn toc_sections(data: &[u8], header_base: usize) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut toc_from = 0;
    while let Some(toc_offset) = find(data, TOC_START, toc_from) {
        toc_from = toc_offset + TOC_START.len();
        let Some(line_end) = find(data, b"\n", toc_offset) else {
            continue;
        };
        let Ok(header) = std::str::from_utf8(&data[toc_offset..line_end]) else {
            continue;
        };
        let header = header.trim_end_matches('#');
        let fields = header.split_whitespace().collect::<Vec<_>>();
        let (Some(count), Some(row_width)) = (
            fields.get(2).and_then(|value| value.parse::<usize>().ok()),
            fields.get(3).and_then(|value| value.parse::<usize>().ok()),
        ) else {
            continue;
        };
        if row_width == 0 {
            continue;
        }
        let rows_start = line_end + 1;
        for index in 0..count {
            let start = rows_start.saturating_add(index.saturating_mul(row_width));
            let Some(row) = data.get(start..start.saturating_add(row_width)) else {
                break;
            };
            let Ok(row) = std::str::from_utf8(row) else {
                continue;
            };
            let fields = row
                .trim_end_matches(['#', '\n', '\r', ' '])
                .split_whitespace()
                .collect::<Vec<_>>();
            let Some(name) = fields.first().copied() else {
                continue;
            };
            if name == "NEXT_TOC_ENTRY" {
                continue;
            }
            let (raw_name, offset_field, length_field, expanded_field) = if name == "ModelView" {
                let (Some(id), Some(offset), Some(length), Some(expanded)) =
                    (fields.get(1), fields.get(2), fields.get(3), fields.get(4))
                else {
                    continue;
                };
                (format!("ModelView#{id}"), *offset, *length, *expanded)
            } else {
                let (Some(offset), Some(length), Some(expanded)) =
                    (fields.get(1), fields.get(2), fields.get(3))
                else {
                    continue;
                };
                (name.to_string(), *offset, *length, *expanded)
            };
            let (Ok(relative_offset), Ok(length), Ok(expanded_length)) = (
                usize::from_str_radix(offset_field, 16),
                usize::from_str_radix(length_field, 16),
                usize::from_str_radix(expanded_field, 16),
            ) else {
                continue;
            };
            let Some(offset) = header_base.checked_add(relative_offset) else {
                continue;
            };
            let marker = [b"#".as_slice(), raw_name.as_bytes(), b"\n"].concat();
            let Some(marker_end) = offset.checked_add(marker.len()) else {
                continue;
            };
            if length < marker.len()
                || data.get(offset..marker_end) != Some(marker.as_slice())
                || offset
                    .checked_add(length)
                    .is_none_or(|end| end > data.len())
            {
                continue;
            }
            let normalized = normalize_name(&raw_name);
            sections.push(Section {
                role: classify(&normalized),
                name: normalized,
                raw_name,
                offset,
                length,
                expanded_length: Some(expanded_length),
            });
        }
    }
    sections.sort_by_key(|section| section.offset);
    sections.dedup_by_key(|section| section.offset);
    sections
}

fn expanded_sections(data: &[u8], sections: &[Section]) -> Vec<ExpandedSection> {
    const MAX_EXPANDED_SECTION: usize = 256 * 1024 * 1024;
    sections
        .iter()
        .filter_map(|section| {
            let expected_length = section.expanded_length?;
            if expected_length > MAX_EXPANDED_SECTION {
                return None;
            }
            let header_length = section.raw_name.len().checked_add(2)?;
            let source_offset = section.offset.checked_add(header_length)?;
            let end = section.offset.checked_add(section.length)?;
            let payload = data.get(source_offset..end)?;
            if !payload.starts_with(&[0x1f, 0x9d]) {
                return None;
            }
            let expanded = crate::compress::decode(payload, expected_length)?;
            Some(ExpandedSection {
                name: section.name.clone(),
                source_offset,
                compressed_length: payload.len(),
                data: expanded,
            })
        })
        .collect()
}

fn toc_lists_section(toc: &[u8], name: &[u8]) -> bool {
    toc.windows(name.len() + 2).any(|window| {
        window[0] == b'\n' && &window[1..=name.len()] == name && window[1 + name.len()] == b' '
    })
}

/// Section-name bytes: printable ASCII minus space, plus the `ND:` decoration
/// punctuation and the `ModelView#N` separator.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-' | b'#')
}

/// Identify the layout family structurally ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). ND is signalled by `ND:`
/// name decoration or a large section count; DEPDB by a `DEPDB_DATA` section
/// with a sparse section list.
fn identify_layout(sections: &[Section]) -> Layout {
    let has_nd_decoration = sections.iter().any(|s| s.raw_name.starts_with("ND:"));
    let has_depdb = sections.iter().any(|s| s.name == "DEPDB_DATA");
    if has_nd_decoration {
        Layout::Nd
    } else if has_depdb && sections.len() <= 24 {
        Layout::Depdb
    } else if sections.len() >= 32 {
        Layout::Nd
    } else if has_depdb {
        Layout::Depdb
    } else {
        Layout::Unknown
    }
}

/// Sum every valid `<label>\0 [skip] f8 <count>` header in `region`.
/// After the label's NUL terminator, up to two optional non-`f8` framing bytes
/// (e.g. the `f3`/`f2` `crv_array` discriminators) are skipped before the
/// required `f8` opener, whose compact-integer count is then decoded ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array),
/// [§5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#5-topology-and-section-records)).
fn read_array_count(region: &[u8], label: &[u8]) -> Option<u32> {
    let mut from = 0;
    let mut total = 0u32;
    let mut found = false;
    while let Some(pos) = find(region, label, from) {
        let mut p = pos + label.len();
        // Require the NUL that terminates the namespace label.
        if region.get(p) == Some(&0) {
            p += 1;
            // Skip up to two framing bytes before the array opener.
            for _ in 0..3 {
                match region.get(p) {
                    Some(&psb::token::ARRAY_OPEN) => {
                        let (count, _) = psb::compact_int(region, p + 1);
                        total = total.saturating_add(count);
                        found = true;
                        break;
                    }
                    Some(_) => p += 1,
                    None => break,
                }
            }
        }
        from = pos + 1;
    }
    found.then_some(total)
}

/// Read the visible-geometry namespace census from the `VisibGeom` section body.
fn geom_census(data: &[u8], sections: &[Section]) -> GeomCensus {
    let Some(vg) = sections
        .iter()
        .find(|s| s.name == VISIBGEOM)
        .or_else(|| sections.iter().find(|s| s.name == "DEPDB_DATA"))
    else {
        return GeomCensus::default();
    };
    let region = &data[vg.offset..(vg.offset + vg.length).min(data.len())];
    GeomCensus {
        srf_array_count: read_array_count(region, b"srf_array"),
        crv_array_count: read_array_count(region, b"crv_array"),
    }
}

/// Decode the active unit-system selector. `51` is millimeter-Newton-Second
/// and `55` is millimeter-Kilogram-Second; both use millimeters for lengths.
fn principal_unit(data: &[u8]) -> Option<String> {
    let start = find(data, PRINCIPAL_UNIT_ID, 0)? + PRINCIPAL_UNIT_ID.len();
    match *data.get(start)? {
        51 => Some("mmNs".to_string()),
        55 => Some("mmKs".to_string()),
        value => Some(format!("unknown:{value}")),
    }
}

fn model_name(data: &[u8]) -> Option<(String, usize)> {
    const PREFIX: &[u8] = b"#- CMNM ";
    let marker = find(data, PREFIX, 0)?;
    let start = marker + PREFIX.len();
    find(data, PREFIX, start).is_none().then_some(())?;
    let length_bytes = data.get(start..start + 3)?;
    let length = usize::from_str_radix(std::str::from_utf8(length_bytes).ok()?, 16).ok()?;
    let name = data.get(start + 3..start + 3 + length)?;
    (!name.is_empty() && !name.iter().any(|byte| matches!(byte, 0 | b'\n' | b'\r')))
        .then_some(())?;
    Some((std::str::from_utf8(name).ok()?.to_string(), start + 3))
}

fn relation_model_name(filename: &str) -> Option<&str> {
    let filename = filename.trim_end_matches(' ');
    let suffix = filename.get(filename.len().checked_sub(4)?..)?;
    suffix.eq_ignore_ascii_case(".prt").then_some(())?;
    let name = filename.get(..filename.len() - 4)?;
    (!name.is_empty()).then_some(name)
}

fn family_table(data: &[u8], sections: &[Section]) -> Option<FamilyTableRecord> {
    let section = sections
        .iter()
        .find(|section| section.name == "FamilyInf")?;
    let end = (section.offset + section.length).min(data.len());
    let label = b"drv_tbl_ptr\0";
    let offset = find(data, label, section.offset)? + label.len();
    if offset >= end {
        return None;
    }
    let pointer = match data[offset] {
        0xe1 => FamilyTablePointer::Null,
        psb::token::ENTITY_REF => {
            let (id, after) = psb::reference_id(data, offset + 1).ok()?;
            (after <= end).then_some(FamilyTablePointer::Entity(id))?
        }
        _ => return None,
    };
    Some(FamilyTableRecord { pointer, offset })
}

fn model_geometry_sections(data: &[u8], sections: &[Section]) -> Vec<Section> {
    let visible_namespace_present = sections.iter().any(|candidate| {
        if candidate.name != VISIBGEOM {
            return false;
        }
        let end = (candidate.offset + candidate.length).min(data.len());
        let payload = &data[candidate.offset..end];
        find(payload, b"srf_array\0", 0).is_some() || find(payload, b"crv_array\0", 0).is_some()
    });
    sections
        .iter()
        .filter(|section| {
            if visible_namespace_present {
                section.name == VISIBGEOM
            } else if section.name == "DEPDB_DATA" {
                let end = section
                    .offset
                    .saturating_add(section.length)
                    .min(data.len());
                let payload = &data[section.offset..end];
                find(payload, b"srf_array\0", 0).is_some()
                    || find(payload, b"crv_array\0", 0).is_some()
            } else {
                false
            }
        })
        .cloned()
        .collect()
}

fn surface_rows(data: &[u8], sections: &[Section]) -> Vec<SurfaceRow> {
    let mut rows = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        rows.extend(
            surface::rows(&data[section.offset..end])
                .into_iter()
                .map(|mut row| {
                    row.offset += section.offset;
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn cross_section_surface_rows(data: &[u8], sections: &[Section]) -> Vec<SurfaceRow> {
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "Xsections")
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        rows.extend(
            surface::cross_section_rows(payload)
                .into_iter()
                .map(|mut row| {
                    row.offset += section.offset;
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn surface_prototypes(data: &[u8], sections: &[Section]) -> Vec<SurfacePrototype> {
    let mut prototypes = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        prototypes.extend(
            surface::prototypes(&data[section.offset..end])
                .into_iter()
                .map(|mut prototype| {
                    prototype.offset += section.offset;
                    prototype
                }),
        );
    }
    prototypes.sort_by_key(|prototype| prototype.offset);
    prototypes
}

fn surface_prototype_records(data: &[u8], sections: &[Section]) -> Vec<SurfacePrototypeRecord> {
    let mut records = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            surface::named_prototype_records(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    for parameter in &mut record.parameters {
                        parameter.offset += section.offset;
                        parameter.value_offset += section.offset;
                    }
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn surface_parameters(data: &[u8], sections: &[Section]) -> Vec<SurfaceParameterRecord> {
    let mut records = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            surface::parameter_records(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record.body_offset += section.offset;
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn cross_section_surface_parameters(
    data: &[u8],
    sections: &[Section],
) -> Vec<SurfaceParameterRecord> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "Xsections")
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        records.extend(
            surface::cross_section_parameter_records(payload)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record.body_offset += section.offset;
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn tabulated_cylinder_curve_replays(
    data: &[u8],
    sections: &[Section],
) -> Vec<TabulatedCylinderCurveReplay> {
    let mut records = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            surface::tabulated_cylinder_curve_replays(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record.surface_row_offset += section.offset;
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn plane_local_systems(data: &[u8], sections: &[Section]) -> Vec<PlaneLocalSystem> {
    let mut systems = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        systems.extend(
            surface::plane_local_systems(&data[section.offset..end])
                .into_iter()
                .map(|mut system| {
                    system.row_offset += section.offset;
                    system.offset += section.offset;
                    system
                }),
        );
    }
    systems.sort_by_key(|system| system.offset);
    systems
}

fn cross_section_plane_local_systems(data: &[u8], sections: &[Section]) -> Vec<PlaneLocalSystem> {
    let mut systems = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "Xsections")
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        systems.extend(
            surface::cross_section_plane_local_systems(payload)
                .into_iter()
                .map(|mut system| {
                    system.row_offset += section.offset;
                    system.offset += section.offset;
                    system
                }),
        );
    }
    systems.sort_by_key(|system| system.offset);
    systems
}

fn plane_envelopes(data: &[u8], sections: &[Section]) -> Vec<PlaneEnvelopeRecord> {
    let mut envelopes = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        envelopes.extend(
            surface::plane_envelopes(&data[section.offset..end])
                .into_iter()
                .map(|mut envelope| {
                    envelope.row_offset += section.offset;
                    envelope.offset += section.offset;
                    envelope
                }),
        );
    }
    envelopes.sort_by_key(|envelope| envelope.offset);
    envelopes
}

fn cross_section_plane_envelopes(data: &[u8], sections: &[Section]) -> Vec<PlaneEnvelopeRecord> {
    let mut envelopes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "Xsections")
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        envelopes.extend(
            surface::cross_section_plane_envelopes(payload)
                .into_iter()
                .map(|mut envelope| {
                    envelope.offset += section.offset;
                    envelope
                }),
        );
    }
    envelopes.sort_by_key(|envelope| envelope.offset);
    envelopes
}

fn curve_prototypes(data: &[u8], sections: &[Section]) -> Vec<CurvePrototype> {
    let mut prototypes = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        prototypes.extend(
            curve::prototypes(&data[section.offset..end])
                .into_iter()
                .map(|mut prototype| {
                    prototype.offset += section.offset;
                    prototype
                }),
        );
    }
    prototypes.sort_by_key(|prototype| prototype.offset);
    prototypes
}

fn curve_expressions(
    data: &[u8],
    sections: &[Section],
    model_name: Option<&str>,
) -> Vec<CurveExpressionRecord> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == VISIBGEOM || section.name == "DEPDB_DATA")
    {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            curve::expression_records_with_model_name(&data[section.offset..end], model_name)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record.expression_offset += section.offset;
                    for line in &mut record.lines {
                        line.offset += section.offset;
                    }
                    for assignment in &mut record.assignments {
                        assignment.offset += section.offset;
                    }
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn curve_parameters(data: &[u8], sections: &[Section]) -> Vec<CurveParameterRecord> {
    let mut records = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            curve::parameter_records(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record.body_offset += section.offset;
                    record.suffix_offset += section.offset;
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn prototype_pcurves(data: &[u8], sections: &[Section]) -> Vec<PrototypePcurveEndpoints> {
    let mut records = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            curve::prototype_pcurve_endpoints(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn curve_prototype_topology(data: &[u8], sections: &[Section]) -> Vec<CurvePrototypeTopology> {
    let mut records = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            curve::prototype_topology(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn curve_topology_rows(data: &[u8], sections: &[Section]) -> Vec<CurveTopologyRow> {
    let mut rows = Vec::new();
    for section in sections {
        let end = (section.offset + section.length).min(data.len());
        rows.extend(
            curve::topology_rows(&data[section.offset..end])
                .into_iter()
                .map(|mut row| {
                    row.offset += section.offset;
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn cross_section_curve_rows(data: &[u8], sections: &[Section]) -> Vec<DepdbCurveRow> {
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "Xsections")
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        rows.extend(
            curve::depdb_cross_section_rows(payload)
                .into_iter()
                .map(|mut row| {
                    row.offset += section.offset;
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn cross_section_curve_prototypes(data: &[u8], sections: &[Section]) -> Vec<CurvePrototype> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "Xsections")
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        records.extend(curve::prototypes(payload).into_iter().map(|mut record| {
            record.offset += section.offset;
            record
        }));
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn datum_planes(data: &[u8], sections: &[Section]) -> Vec<DatumPlane> {
    let mut planes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "ActDatums")
    {
        let end = (section.offset + section.length).min(data.len());
        planes.extend(
            datum::planes(&data[section.offset..end])
                .into_iter()
                .map(|mut plane| {
                    plane.offset_in_payload += section.offset;
                    plane
                }),
        );
        if let Some(mut plane) = datum::named_plane(&data[section.offset..end]) {
            plane.offset_in_payload += section.offset;
            planes.push(plane);
        }
    }
    planes.sort_by_key(|plane| plane.offset_in_payload);
    planes
}

fn feature_ids(data: &[u8], sections: &[Section], rows: &[SurfaceRow]) -> Vec<u32> {
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(rows.iter().map(|row| row.feature_id).filter(|id| *id != 0));
    for section in sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        let mut from = 0;
        while let Some(found) = find(payload, b"parent_feats\0", from) {
            let start = found + b"parent_feats\0".len();
            let Some(&psb::token::ARRAY_OPEN) = payload.get(start) else {
                from = start;
                continue;
            };
            let (count, mut cursor) = psb::compact_int(payload, start + 1);
            for _ in 0..count {
                let (id, next) = psb::compact_int(payload, cursor);
                if next == cursor {
                    break;
                }
                if id != 0 {
                    ids.insert(id);
                }
                cursor = next;
            }
            from = start;
        }
    }
    ids.into_iter().collect()
}

fn feature_entity_tables(
    data: &[u8],
    sections: &[Section],
    feature_ids: &[u32],
    rows: &[SurfaceRow],
) -> Vec<FeatureEntityTable> {
    let feature_ids = feature_ids.iter().copied().collect();
    let surface_ids = rows.iter().map(|row| row.id).collect();
    let mut tables = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "AllFeatur")
    {
        let end = (section.offset + section.length).min(data.len());
        tables.extend(
            feature::entity_tables(&data[section.offset..end], &feature_ids, &surface_ids)
                .into_iter()
                .map(|mut table| {
                    table.offset += section.offset;
                    for entry in &mut table.entries {
                        entry.offset += section.offset;
                        entry.end_offset += section.offset;
                    }
                    table
                }),
        );
    }
    tables.sort_by_key(|table| table.offset);
    tables
}

fn feature_rows(data: &[u8], sections: &[Section], feature_ids: &[u32]) -> Vec<FeatureRow> {
    let feature_ids = feature_ids.iter().copied().collect();
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "AllFeatur")
    {
        let end = (section.offset + section.length).min(data.len());
        rows.extend(
            feature::rows(&data[section.offset..end], &feature_ids)
                .into_iter()
                .map(|mut row| {
                    row.stream_offset = section.offset;
                    row.offset += section.offset;
                    row.body_offset += section.offset;
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn feature_entity_graph(
    data: &[u8],
    sections: &[Section],
) -> (Vec<FeatureEntity>, Vec<FeatureEntityReference>) {
    let Some(section) = sections.iter().find(|section| section.name == "AllFeatur") else {
        return (Vec::new(), Vec::new());
    };
    let end = (section.offset + section.length).min(data.len());
    let payload_start = find(data, b"\n", section.offset)
        .map_or(section.offset, |newline| newline + 1)
        .min(end);
    let (mut entities, mut references) = feature::entity_graph(&data[payload_start..end]);
    for entity in &mut entities {
        entity.offset += payload_start;
    }
    for reference in &mut references {
        reference.offset += payload_start;
    }
    (entities, references)
}

fn offset_feature_definition(definition: &mut FeatureDefinition, section_offset: usize) {
    definition.offset += section_offset;
    for frame in &mut definition.parameter_frames {
        frame.offset += section_offset;
    }
    for outline in &mut definition.outlines {
        outline.offset += section_offset;
    }
    if let Some(variables) = &mut definition.variables {
        variables.offset += section_offset;
        for row in &mut variables.rows {
            row.offset += section_offset;
        }
    }
    if let Some(segments) = &mut definition.segments {
        segments.offset += section_offset;
        for row in &mut segments.rows {
            row.offset += section_offset;
        }
        for row in &mut segments.opaque_rows {
            row.offset += section_offset;
        }
    }
    if let Some(entities) = &mut definition.trim_entities {
        entities.offset += section_offset;
        for row in &mut entities.rows {
            row.offset += section_offset;
        }
    }
    if let Some(vertices) = &mut definition.trim_vertices {
        vertices.offset += section_offset;
        for row in &mut vertices.rows {
            row.offset += section_offset;
        }
    }
    if let Some(order) = &mut definition.order_table {
        order.offset += section_offset;
        for row in &mut order.rows {
            row.offset += section_offset;
        }
    }
    if let Some(section_3d) = &mut definition.section_3d {
        section_3d.offset += section_offset;
    }
    if let Some(dimensions) = &mut definition.dimensions {
        dimensions.offset += section_offset;
        for row in &mut dimensions.rows {
            row.offset += section_offset;
        }
    }
    if let Some(relations) = &mut definition.relations {
        relations.offset += section_offset;
        for row in &mut relations.rows {
            row.offset += section_offset;
        }
    }
    if let Some(saved) = &mut definition.saved_section {
        saved.offset += section_offset;
        for entity in &mut saved.entities {
            match entity {
                feature::FeatureSavedEntity::Line(line) => line.offset += section_offset,
                feature::FeatureSavedEntity::Arc(arc) => arc.offset += section_offset,
                feature::FeatureSavedEntity::Circle(circle) => circle.offset += section_offset,
                feature::FeatureSavedEntity::Spline(spline) => spline.offset += section_offset,
                feature::FeatureSavedEntity::Dummy(dummy) => dummy.offset += section_offset,
            }
        }
    }
}

fn feature_definitions(data: &[u8], sections: &[Section]) -> Vec<FeatureDefinition> {
    let mut definitions = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "FeatDefs" || section.name == "DEPDB_DATA")
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        definitions.extend(
            (if section.name == "DEPDB_DATA" {
                feature::depdb_definitions(payload)
            } else {
                feature::definitions(payload)
            })
            .into_iter()
            .map(|mut definition| {
                offset_feature_definition(&mut definition, section.offset);
                definition
            }),
        );
        if section.name == "DEPDB_DATA" {
            let recipe_operations = feature::operations(payload)
                .into_iter()
                .filter(|operation| operation.recipe.is_some())
                .collect::<Vec<_>>();
            if let [operation] = recipe_operations.as_slice() {
                if let Some(mut definition) =
                    feature::depdb_section_definition(payload, operation.feature_id)
                {
                    offset_feature_definition(&mut definition, section.offset);
                    if let Some(existing) = definitions
                        .iter_mut()
                        .find(|existing| existing.offset == definition.offset)
                    {
                        *existing = definition;
                    } else {
                        definitions.push(definition);
                    }
                }
            }
        }
    }
    definitions.sort_by_key(|definition| definition.offset);
    definitions
}

fn feature_row_definitions(rows: &[FeatureRow]) -> Vec<FeatureDefinition> {
    let mut definitions = rows
        .iter()
        .filter_map(|row| {
            let mut definition = feature::depdb_section_definition(&row.body, row.feature_id)?;
            definition.owner_feature_id = None;
            offset_feature_definition(&mut definition, row.body_offset);
            Some(definition)
        })
        .collect::<Vec<_>>();
    definitions.sort_by_key(|definition| definition.offset);
    definitions
}

fn positional_replay_definitions(data: &[u8], sections: &[Section]) -> Vec<FeatureDefinition> {
    let mut definitions = Vec::new();
    for section in sections.iter().filter(|section| section.name == "FeatDefs") {
        let end = (section.offset + section.length).min(data.len());
        definitions.extend(
            feature::positional_replay_definitions(&data[section.offset..end])
                .into_iter()
                .map(|mut definition| {
                    offset_feature_definition(&mut definition, section.offset);
                    definition
                }),
        );
    }
    definitions.sort_by_key(|definition| definition.offset);
    definitions
}

fn feature_operations(data: &[u8], sections: &[Section]) -> Vec<FeatureOperation> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "MdlStatus" || section.name == "DEPDB_DATA")
    {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            feature::operations(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record.state_offset += section.offset;
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    let mut current = records
        .into_iter()
        .map(|record| (record.feature_id, record))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect::<Vec<_>>();
    current.sort_by_key(|record| record.offset);
    current
}

fn feature_reference_names(data: &[u8], sections: &[Section]) -> Vec<FeatureReferenceName> {
    sections
        .iter()
        .filter(|section| section.name == "MdlRefInfo")
        .flat_map(|section| {
            let end = section
                .offset
                .saturating_add(section.length)
                .min(data.len());
            feature::reference_names(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record
                })
        })
        .collect()
}

fn feature_operation_states(data: &[u8], sections: &[Section]) -> Vec<FeatureOperation> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "MdlStatus" || section.name == "DEPDB_DATA")
    {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            feature::operation_states(&data[section.offset..end])
                .into_iter()
                .map(|mut record| {
                    record.offset += section.offset;
                    record.state_offset += section.offset;
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn depdb_recipe_rows(data: &[u8], sections: &[Section]) -> Vec<FeatureRow> {
    fn recipe_end(payload: &[u8], search_start: usize, recipe: FeatureRecipe) -> Option<usize> {
        let name = match recipe {
            FeatureRecipe::ProtrudeExtrude => b"protextrude\0".as_slice(),
            FeatureRecipe::CutExtrude => b"cutextrude\0",
            FeatureRecipe::ProtrudeRevolve => b"protrevolve\0",
            FeatureRecipe::CutRevolve => b"cutrevolve\0",
        };
        find(payload, name, search_start)?.checked_add(name.len())
    }

    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "DEPDB_DATA")
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        let mut recipe_operations = feature::operation_states(payload)
            .into_iter()
            .filter(|operation| operation.recipe.is_some())
            .collect::<Vec<_>>();
        recipe_operations.sort_by_key(|operation| operation.offset);
        let mut body_start = 0;
        for operation in &recipe_operations {
            let Some(body_end) = recipe_end(
                payload,
                operation.offset,
                operation.recipe.expect("filtered recipe operation"),
            ) else {
                continue;
            };
            if body_start >= body_end {
                continue;
            }
            rows.push(FeatureRow {
                feature_id: operation.feature_id,
                header: [0; 2],
                root_schema_class: operation.root_schema_class,
                stream_offset: section.offset,
                body: payload[body_start..body_end].to_vec(),
                body_offset: section.offset + body_start,
                offset: section.offset + operation.offset,
            });
            body_start = body_end;
        }
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn geomlists_value(data: &[u8], sections: &[Section], label: &[u8]) -> Option<u32> {
    let section = sections
        .iter()
        .find(|section| section.name == "Geomlists")?;
    let end = (section.offset + section.length).min(data.len());
    let payload = &data[section.offset..end];
    let value_offset = find(payload, label, 0)? + label.len();
    let (count, after) = psb::compact_int(payload, value_offset);
    (after > value_offset).then_some(count)
}

/// Scan a `.prt` decode root into its container structure.
///
/// The decode/inspect entry point, matching the other container codecs'
/// `scan(ctx, root)` signature. `_ctx` is taken for parity; the scan is a pure
/// function of the source bytes and charges no decode budget. Copies the root
/// window into the owned buffer [`scan_bytes`] parses.
pub fn scan(_ctx: &DecodeContext<'_>, root: View<'_>) -> ContainerScan {
    scan_bytes(root.window().to_vec())
}

/// Parse a whole `.prt` byte image. The owned-buffer core reached by [`scan`]
/// for the decode/inspect paths, and called directly by the tests and the
/// container-scan fuzz target.
pub fn scan_bytes(data: Vec<u8>) -> ContainerScan {
    let version_line = line_at(&data, 0);
    let (model_name, model_name_offset) =
        model_name(&data).map_or((None, None), |(name, offset)| (Some(name), Some(offset)));

    // The binary body begins after the ASCII header and TOC. Prefer the TOC end
    // marker; fall back to the header end; fall back to the magic line.
    let header_end = find(&data, UGC_HEADER_END, 0)
        .and_then(|p| find(&data, b"\n", p))
        .map(|nl| nl + 1);
    let toc_end = find(&data, TOC_START, 0)
        .and_then(|toc| find(&data, TOC_END, toc))
        .and_then(|p| find(&data, b"\n", p))
        .map(|nl| nl + 1);
    let body_start = toc_end.or(header_end).unwrap_or(0);

    let sections = toc_sections(&data, header_end.unwrap_or(0));
    let sections = if sections.is_empty() {
        scan_sections(&data, body_start)
    } else {
        sections
    };
    let expanded_sections = expanded_sections(&data, &sections);
    let double_xar_tables = expanded_sections
        .iter()
        .flat_map(|section| {
            crate::scalar::double_xar_tables(&section.data)
                .into_iter()
                .map(|table| ModelDoubleXarTable {
                    section_name: section.name.clone(),
                    section_source_offset: section.source_offset,
                    expanded_offset: table.offset,
                    count: table.count,
                    entries: table.entries,
                })
        })
        .collect();
    let primitive_scalar_arrays = expanded_sections
        .iter()
        .filter(|section| section.name == "SolidPrimdata")
        .flat_map(|section| primdata::scalar_arrays(&section.data))
        .collect();
    let primitive_triangle_strips = expanded_sections
        .iter()
        .filter(|section| section.name == "SolidPrimdata")
        .flat_map(|section| primdata::triangle_strips(&section.data))
        .collect();
    let reference_lines = sections
        .iter()
        .filter(|section| section.name == "MdlRefInfo")
        .flat_map(|section| {
            let end = section
                .offset
                .saturating_add(section.length)
                .min(data.len());
            reference::lines(&data[section.offset..end])
                .into_iter()
                .chain(reference::line3d_lines(&data[section.offset..end]))
                .map(move |mut line| {
                    line.offset += section.offset;
                    line
                })
        })
        .collect();
    let reference_circles = sections
        .iter()
        .filter(|section| section.name == "MdlRefInfo")
        .flat_map(|section| {
            let end = section
                .offset
                .saturating_add(section.length)
                .min(data.len());
            reference::arc_z_circles(&data[section.offset..end])
                .into_iter()
                .map(move |mut circle| {
                    circle.offset += section.offset;
                    circle
                })
        })
        .collect();
    let reference_conics: Vec<ReferenceConic> = sections
        .iter()
        .filter(|section| section.name == "MdlRefInfo")
        .flat_map(|section| {
            let end = section
                .offset
                .saturating_add(section.length)
                .min(data.len());
            reference::named_conics(&data[section.offset..end])
                .into_iter()
                .chain(reference::positional_conics(&data[section.offset..end]))
                .map(move |mut conic| {
                    conic.offset += section.offset;
                    conic
                })
        })
        .collect();
    let reference_ellipses = reference::ellipse_carriers(&reference_conics);
    let layout = identify_layout(&sections);
    let model_geometry_sections = model_geometry_sections(&data, &sections);
    let census = geom_census(&data, &sections);
    let principal_unit = principal_unit(&data);
    let family_table = family_table(&data, &sections);
    let nonvisible_geometry_sections = sections
        .iter()
        .filter(|section| section.name == "NovisGeom")
        .cloned()
        .collect::<Vec<_>>();
    let nonvisible_surface_rows = surface_rows(&data, &nonvisible_geometry_sections);
    let surface_rows = surface_rows(&data, &model_geometry_sections);
    let cross_section_surface_rows = cross_section_surface_rows(&data, &sections);
    let nonvisible_surface_parameters = surface_parameters(&data, &nonvisible_geometry_sections);
    let surface_parameters = surface_parameters(&data, &model_geometry_sections);
    let cross_section_surface_parameters = cross_section_surface_parameters(&data, &sections);
    let tabulated_cylinder_curve_replays =
        tabulated_cylinder_curve_replays(&data, &model_geometry_sections);
    let plane_local_systems = plane_local_systems(&data, &model_geometry_sections);
    let cross_section_plane_local_systems = cross_section_plane_local_systems(&data, &sections);
    let plane_envelopes = plane_envelopes(&data, &model_geometry_sections);
    let cross_section_plane_envelopes = cross_section_plane_envelopes(&data, &sections);
    let outline_planes = surface::placed_outline_planes(&plane_envelopes, &plane_local_systems);
    let positional_frame_planes =
        surface::positional_frame_planes(&surface_parameters, &surface_rows);
    let mut placement_outline_planes = outline_planes.clone();
    placement_outline_planes.extend(
        positional_frame_planes
            .iter()
            .filter(|plane| {
                !outline_planes
                    .iter()
                    .any(|outline| outline.surface_id == plane.surface_id)
            })
            .cloned(),
    );
    let cross_section_outline_planes = surface::placed_outline_planes(
        &cross_section_plane_envelopes,
        &cross_section_plane_local_systems,
    );
    let surface_prototypes = surface_prototypes(&data, &model_geometry_sections);
    let nonvisible_surface_prototype_records =
        surface_prototype_records(&data, &nonvisible_geometry_sections);
    let surface_prototype_records = surface_prototype_records(&data, &model_geometry_sections);
    let nonvisible_curve_prototypes = curve_prototypes(&data, &nonvisible_geometry_sections);
    let curve_prototypes = curve_prototypes(&data, &model_geometry_sections);
    let cross_section_curve_prototypes = cross_section_curve_prototypes(&data, &sections);
    let mut curve_expressions = curve_expressions(
        &data,
        &sections,
        model_name.as_deref().and_then(relation_model_name),
    );
    let nonvisible_curve_parameters = curve_parameters(&data, &nonvisible_geometry_sections);
    let curve_parameters = curve_parameters(&data, &model_geometry_sections);
    let nonvisible_curve_topology_rows = curve_topology_rows(&data, &nonvisible_geometry_sections);
    let curve_topology_rows = curve_topology_rows(&data, &model_geometry_sections);
    let cross_section_curve_rows = cross_section_curve_rows(&data, &sections);
    let pcurves = curve::pcurve_endpoints(&curve_parameters, &curve_topology_rows);
    let fc_curve_coordinates = curve::fc_coordinates(&curve_parameters);
    let fc05_circles = curve::fc05_circles(&curve_parameters);
    let fc05_cylinder_cap_pairs =
        curve::fc05_cylinder_cap_pairs(&fc05_circles, &curve_topology_rows, &surface_rows);
    let prototype_pcurves = prototype_pcurves(&data, &model_geometry_sections);
    let curve_prototype_topology = curve_prototype_topology(&data, &model_geometry_sections);
    let bound_prototype_pcurves =
        curve::bind_prototype_pcurves(&prototype_pcurves, &curve_prototype_topology);
    let (half_edges, loops) = topology::build(&curve_topology_rows);
    let (topological_vertices, half_edge_vertex_incidence) = topology::vertex_orbits(&half_edges);
    let face_components = topology::face_components(&curve_topology_rows);
    let datum_planes = datum_planes(&data, &sections);
    let feature_ids = feature_ids(&data, &sections, &surface_rows);
    let feature_rows = feature_rows(&data, &sections, &feature_ids);
    let feature_choices = feature::choices(&feature_rows);
    let feature_choice_fields = feature::choice_fields(&feature_choices);
    let depdb_recipe_rows = depdb_recipe_rows(&data, &sections);
    let mut feature_geometry_tables = feature::geometry_tables(&feature_rows);
    feature_geometry_tables.extend(feature::geometry_tables(&depdb_recipe_rows));
    feature_geometry_tables.sort_by_key(|table| table.offset);
    let mut feature_affected_ids = feature::affected_ids(&feature_rows);
    feature_affected_ids.extend(feature::affected_ids(&depdb_recipe_rows));
    feature_affected_ids.sort_by_key(|record| record.offset);
    let feature_replay_affected_ids = feature::replay_affected_ids(&feature_rows);
    let feature_loop_restore_directions = feature::loop_restore_directions(&feature_rows);
    let feature_entity_tables =
        feature_entity_tables(&data, &sections, &feature_ids, &surface_rows);
    let feature_operation_states = feature_operation_states(&data, &sections);
    let feature_operations = feature_operations(&data, &sections);
    let feature_reference_names = feature_reference_names(&data, &sections);
    let mut feature_definitions = feature_definitions(&data, &sections);
    feature::bind_definition_owners(&mut feature_definitions, &feature_geometry_tables);
    feature::bind_trimmed_definition_owners(&mut feature_definitions, &feature_entity_tables);
    feature_definitions.extend(feature_row_definitions(&feature_rows));
    feature_definitions.sort_by_key(|definition| definition.offset);
    let claimed_definition_owners = feature_definitions
        .iter()
        .filter_map(|definition| definition.owner_feature_id)
        .collect();
    let mut replay_definitions = positional_replay_definitions(&data, &sections);
    feature::bind_replay_definition_owners(
        &mut replay_definitions,
        &feature_entity_tables,
        &claimed_definition_owners,
    );
    feature_definitions.extend(replay_definitions);
    feature_definitions.sort_by_key(|definition| definition.offset);
    let mut section_owner_ranges = sections
        .iter()
        .filter(|section| section.name == "DEPDB_DATA")
        .map(|section| {
            (
                section.offset,
                section.offset.saturating_add(section.length),
            )
        })
        .collect::<Vec<_>>();
    if sections.iter().any(|section| section.name == "DEPDB_DATA") {
        section_owner_ranges.extend(
            feature_rows
                .iter()
                .filter(|row| row.root_schema_class == Some(926))
                .map(|row| {
                    (
                        row.body_offset,
                        row.body_offset.saturating_add(row.body.len()),
                    )
                }),
        );
    }
    feature::bind_depdb_section_owners(
        &mut feature_definitions,
        &feature_operations,
        &section_owner_ranges,
    );
    let mut relation_dimension_symbols = ExternalRelationSymbols::default();
    for dimension in feature_definitions
        .iter()
        .filter_map(|definition| definition.dimensions.as_ref())
        .flat_map(|table| table.rows.iter())
    {
        let value = dimension.value.map(|value| match dimension.value_unit {
            feature::DimensionUnit::Radians => CurveExpressionValue::Angle(value.to_degrees()),
            feature::DimensionUnit::Millimeters => CurveExpressionValue::Length(value),
            feature::DimensionUnit::SchemaDefined => CurveExpressionValue::Number(value),
        });
        relation_dimension_symbols.observe(&format!("d{}", dimension.external_id), value);
    }
    curve::reevaluate_expression_records(
        &mut curve_expressions,
        model_name.as_deref().and_then(relation_model_name),
        &relation_dimension_symbols,
    );
    let mut feature_revolution_extents = feature::revolution_extents(&feature_rows);
    feature_revolution_extents.extend(feature::definition_revolution_extents(
        &feature_definitions,
        &feature_operations,
    ));
    feature_revolution_extents.sort_by_key(|record| record.offset);
    let feature_section_transforms = placement::resolve(
        &feature_definitions,
        &placement::PlacementSources {
            datums: &datum_planes,
            surface_rows: &surface_rows,
            model_planes: &plane_local_systems,
            outline_planes: &placement_outline_planes,
            plane_envelopes: &plane_envelopes,
            surface_parameters: &surface_parameters,
            geometry_tables: &feature_geometry_tables,
            affected_ids: &feature_affected_ids,
        },
        &feature_entity_tables,
    );
    let (feature_entities, feature_entity_references) = feature_entity_graph(&data, &sections);
    let declared_body_count = geomlists_value(&data, &sections, b"n_bodies\0");
    let first_quilt_ptr = geomlists_value(&data, &sections, b"first_quilt_ptr\0");

    ContainerScan {
        framing: FramingScan {
            data,
            version_line,
            model_name,
            model_name_offset,
            sections,
            expanded_sections,
            layout,
            census,
            principal_unit,
            family_table,
            declared_body_count,
            first_quilt_ptr,
        },
        primitives: PrimitiveScan {
            double_xar_tables,
            scalar_arrays: primitive_scalar_arrays,
            triangle_strips: primitive_triangle_strips,
        },
        references: ReferenceScan {
            lines: reference_lines,
            circles: reference_circles,
            conics: reference_conics,
            ellipses: reference_ellipses,
        },
        surfaces: SurfaceScan {
            rows: surface_rows,
            nonvisible_rows: nonvisible_surface_rows,
            cross_section_rows: cross_section_surface_rows,
            parameters: surface_parameters,
            nonvisible_parameters: nonvisible_surface_parameters,
            cross_section_parameters: cross_section_surface_parameters,
            prototypes: surface_prototypes,
            prototype_records: surface_prototype_records,
            nonvisible_prototype_records: nonvisible_surface_prototype_records,
        },
        planes: PlaneScan {
            local_systems: plane_local_systems,
            cross_section_local_systems: cross_section_plane_local_systems,
            envelopes: plane_envelopes,
            cross_section_envelopes: cross_section_plane_envelopes,
            outlines: outline_planes,
            positional_frames: positional_frame_planes,
            cross_section_outlines: cross_section_outline_planes,
            datums: datum_planes,
        },
        curves: CurveScan {
            tabulated_cylinder_replays: tabulated_cylinder_curve_replays,
            prototypes: curve_prototypes,
            nonvisible_prototypes: nonvisible_curve_prototypes,
            cross_section_prototypes: cross_section_curve_prototypes,
            expressions: curve_expressions,
            parameters: curve_parameters,
            nonvisible_parameters: nonvisible_curve_parameters,
            pcurves,
            fc_coordinates: fc_curve_coordinates,
            fc05_circles,
            fc05_cylinder_cap_pairs,
            prototype_pcurves,
            prototype_topology: curve_prototype_topology,
            bound_prototype_pcurves,
            topology_rows: curve_topology_rows,
            nonvisible_topology_rows: nonvisible_curve_topology_rows,
            cross_section_rows: cross_section_curve_rows,
        },
        topology: TopologyScan {
            half_edges,
            loops,
            face_components,
            vertices: topological_vertices,
            half_edge_vertex_incidence,
        },
        features: FeatureScan {
            ids: feature_ids,
            rows: feature_rows,
            depdb_recipe_rows,
            choices: feature_choices,
            choice_fields: feature_choice_fields,
            geometry_tables: feature_geometry_tables,
            affected_ids: feature_affected_ids,
            replay_affected_ids: feature_replay_affected_ids,
            loop_restore_directions: feature_loop_restore_directions,
            revolution_extents: feature_revolution_extents,
            definitions: feature_definitions,
            section_transforms: feature_section_transforms,
            operation_states: feature_operation_states,
            operations: feature_operations,
            reference_names: feature_reference_names,
            entities: feature_entities,
            entity_references: feature_entity_references,
            entity_tables: feature_entity_tables,
        },
    }
}

/// Return whether a thumbnail section contains a JPEG start marker.
pub fn has_thumbnail(scan: &ContainerScan) -> bool {
    scan.framing
        .sections
        .iter()
        .filter(|s| s.role == role::THUMBNAIL)
        .any(|s| {
            let region =
                &scan.framing.data[s.offset..(s.offset + s.length).min(scan.framing.data.len())];
            find(region, JPEG_MAGIC, 0).is_some()
        })
}

/// Build a codec-neutral summary of the sections, layout, and namespace census.
pub fn summarize(scan: &ContainerScan) -> ContainerSummary {
    let entries = scan
        .framing
        .sections
        .iter()
        .map(|s| {
            let mut attributes = BTreeMap::new();
            attributes.insert("offset".to_string(), s.offset.to_string());
            if s.raw_name != s.name {
                attributes.insert("raw_name".to_string(), s.raw_name.clone());
            }
            let expanded = scan.framing.expanded_sections.iter().find(|expanded| {
                expanded.name == s.name
                    && expanded.source_offset > s.offset
                    && expanded.source_offset < s.offset.saturating_add(s.length)
            });
            if let Some(expanded) = expanded {
                attributes.insert(
                    "expanded_payload_size".to_string(),
                    expanded.data.len().to_string(),
                );
            }
            ContainerEntry {
                name: s.name.clone(),
                role: s.role.to_string(),
                compression: expanded.map_or("none", |_| "unix-compress").to_string(),
                compressed_size: s.length as u64,
                uncompressed_size: expanded.map_or(s.length as u64, |expanded| {
                    (expanded.data.len() + s.raw_name.len() + 2) as u64
                }),
                attributes,
            }
        })
        .collect();

    let mut notes = vec![
        format!("PSB container: {}", scan.framing.version_line),
        format!(
            "layout: {}; {} section(s) enumerated",
            scan.framing.layout.token(),
            scan.framing.sections.len()
        ),
    ];
    if let Some(name) = &scan.framing.model_name {
        notes.push(format!("native model name: {name}"));
    }

    match (
        scan.framing.census.srf_array_count,
        scan.framing.census.crv_array_count,
    ) {
        (None, None) => {
            notes.push("no VisibGeom srf_array/crv_array count header was located".to_string());
        }
        (srf, crv) => {
            notes.push(format!(
                "VisibGeom namespace census: srf_array={}, crv_array={} (byte-backed count \
                 headers; per-instance row geometry is not decoded)",
                srf.map_or_else(|| "n/a".to_string(), |c| c.to_string()),
                crv.map_or_else(|| "n/a".to_string(), |c| c.to_string()),
            ));
        }
    }

    if has_thumbnail(scan) {
        notes.push("THMB_IMG_MAIN carries a JPEG preview (excluded from geometry)".to_string());
    }
    if !scan.framing.expanded_sections.is_empty() {
        notes.push(format!(
            "expanded {} Unix-compress section payload(s) with TOC-validated output lengths",
            scan.framing.expanded_sections.len()
        ));
    }

    notes.push(
        "container-level enumeration; `decode` preserves PSB geometry sections as unknown records \
         and transfers only carriers whose model-space placement is complete"
            .to_string(),
    );

    ContainerSummary {
        format: "creo".to_string(),
        container_kind: "psb".to_string(),
        entries,
        notes,
    }
}

#[cfg(test)]
mod feature_row_definition_tests {
    use super::*;

    #[test]
    fn zero_width_toc_has_no_rows() {
        let data = b"#UGC_TOC 2 18446744073709551615 0#\n";

        assert!(toc_sections(data, 0).is_empty());
    }

    #[test]
    fn embedded_section_definition_retains_separate_history_feature_owner() {
        let row = FeatureRow {
            feature_id: 42,
            header: [0xe3, 0xf6],
            root_schema_class: Some(917),
            stream_offset: 100,
            body: b"prefix gsec2d_ptr\0\xe0\x0aname\0S2D0002\0".to_vec(),
            body_offset: 120,
            offset: 118,
        };

        let definitions = feature_row_definitions(&[row]);

        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].id, 2);
        assert_eq!(definitions[0].owner_feature_id, None);
        assert_eq!(definitions[0].offset, 127);
    }
}
