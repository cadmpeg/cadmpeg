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

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};

use crate::curve::{
    self, BoundPrototypePcurve, CurveExpressionRecord, CurveParameterRecord, CurvePrototype,
    CurvePrototypeTopology, CurveTopologyRow, Fc05Circle, Fc05CylinderCapPair,
    FcCurveControlPoints, PcurveEndpoints, PrototypePcurveEndpoints,
};
use crate::datum::{self, DatumPlane};
use crate::feature::{
    self, FeatureAffectedIds, FeatureChoice, FeatureChoiceField, FeatureDefinition,
    FeatureDirectionByte, FeatureEntity, FeatureEntityReference, FeatureEntityTable,
    FeatureGeometryTable, FeatureOperation, FeatureReplayAffectedIds, FeatureRow,
};
use crate::placement::{self, FeatureSectionTransform};
use crate::psb;
use crate::surface::{
    self, OutlinePlane, PlaneEnvelopeRecord, PlaneLocalSystem, SurfaceParameterRecord,
    SurfacePrototype, SurfacePrototypeRecord, SurfaceRow,
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
    /// Role classification.
    pub role: &'static str,
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

/// Structural data read from one `.prt` file.
pub struct ContainerScan {
    /// Complete source bytes.
    pub data: Vec<u8>,
    /// The magic/version header line, ASCII, trimmed.
    pub version_line: String,
    /// Enumerated sections in file order.
    pub sections: Vec<Section>,
    /// Identified layout family.
    pub layout: Layout,
    /// Visible-geometry namespace census, when a `VisibGeom` section was found.
    pub census: GeomCensus,
    /// Active Creo principal coordinate unit system, when its selector is
    /// present. Both currently defined systems store model lengths in mm.
    pub principal_unit: Option<String>,
    /// Configuration driver-table pointer from `FamilyInf`.
    pub family_table: Option<FamilyTableRecord>,
    /// Typed fixed-prefix surface rows from visible and invisible geometry
    /// sections. Parameter bodies are decoded separately.
    pub surface_rows: Vec<SurfaceRow>,
    /// Bounded scalar parameter bodies from positional surface rows.
    pub surface_parameters: Vec<SurfaceParameterRecord>,
    /// Inherited support frames following positional plane envelopes.
    pub plane_local_systems: Vec<PlaneLocalSystem>,
    /// Plane-specific standard and compact positional envelopes.
    pub plane_envelopes: Vec<PlaneEnvelopeRecord>,
    /// Axis-aligned placed planes derived from unambiguous outline corners.
    pub outline_planes: Vec<OutlinePlane>,
    /// Labeled surface prototypes with fully decoded scalar fields.
    pub surface_prototypes: Vec<SurfacePrototype>,
    /// Bounded named `srf_prim_ptr(<kind>)` parameter records.
    pub surface_prototype_records: Vec<SurfacePrototypeRecord>,
    /// Labeled curve prototypes from geometry sections. The curve body and
    /// its analytic interpretation are decoded separately.
    pub curve_prototypes: Vec<CurvePrototype>,
    /// Source programs from curve-from-equation entity records.
    pub curve_expressions: Vec<CurveExpressionRecord>,
    /// Bounded analytic parameter bodies from positional curve rows.
    pub curve_parameters: Vec<CurveParameterRecord>,
    /// Complete eight-slot pcurve endpoints in both adjacent face frames.
    pub pcurves: Vec<PcurveEndpoints>,
    /// Ordered world-coordinate lanes from FC-prefixed dense curve rows.
    pub fc_curve_control_points: Vec<FcCurveControlPoints>,
    /// FC05 records whose decoded points prove an exact circle.
    pub fc05_circles: Vec<Fc05Circle>,
    /// Cylinder cap groups joined through typed curve-face topology. Their
    /// model-space feature frame remains required before IR transfer.
    pub fc05_cylinder_cap_pairs: Vec<Fc05CylinderCapPair>,
    /// Complete pcurve UV endpoints from labeled curve prototypes.
    pub prototype_pcurves: Vec<PrototypePcurveEndpoints>,
    /// Labeled face and next-edge references from curve prototypes.
    pub curve_prototype_topology: Vec<CurvePrototypeTopology>,
    /// Prototype pcurve endpoints bound to their adjacent face identifiers.
    pub bound_prototype_pcurves: Vec<BoundPrototypePcurve>,
    /// Curve rows with an unambiguous canonical four-reference topology
    /// suffix. These rows define the native half-edge adjacency graph.
    pub curve_topology_rows: Vec<CurveTopologyRow>,
    /// Resolved native half-edges and closed loops built from curve rows.
    pub half_edges: Vec<HalfEdge>,
    /// Closed rings of half-edges, one per resolved face loop.
    pub loops: Vec<Loop>,
    /// Connected components of non-null face references in native curve
    /// topology. These are not emitted IR shells.
    pub face_components: Vec<FaceComponent>,
    /// Topological vertex identities derived from half-edge orbits.
    pub topological_vertices: Vec<TopologicalVertex>,
    /// Start/end vertex binding for each decoded half-edge.
    pub half_edge_vertex_incidence: Vec<HalfEdgeVertexIncidence>,
    /// Model-space standard datum planes decoded from `ActDatums` outlines.
    pub datum_planes: Vec<DatumPlane>,
    /// Feature IDs that own decoded geometry rows.
    pub feature_ids: Vec<u32>,
    /// Byte-bounded `AllFeatur` rows for known geometry-owning features.
    pub feature_rows: Vec<FeatureRow>,
    /// Labeled procedural-choice spans inside decoded feature rows.
    pub feature_choices: Vec<FeatureChoice>,
    /// Named fields and typed wrappers inside procedural-choice spans.
    pub feature_choice_fields: Vec<FeatureChoiceField>,
    /// Generated-geometry namespace headers owned by decoded features.
    pub feature_geometry_tables: Vec<FeatureGeometryTable>,
    /// Complete named affected-ID arrays owned by decoded features.
    pub feature_affected_ids: Vec<FeatureAffectedIds>,
    /// Affected-ID runs from unlabeled positional replay feature rows.
    pub feature_replay_affected_ids: Vec<FeatureReplayAffectedIds>,
    /// Named direction bytes from feature recipes.
    pub feature_direction_bytes: Vec<FeatureDirectionByte>,
    /// Byte-bounded `FeatDefs` records and definition-space parameter frames.
    pub feature_definitions: Vec<FeatureDefinition>,
    /// Section-to-model frames resolved from perpendicular active datums.
    pub feature_section_transforms: Vec<FeatureSectionTransform>,
    /// Ordered feature-operation names from `MdlStatus`.
    pub feature_operations: Vec<FeatureOperation>,
    /// Named records in the implicit `AllFeatur` walker-order entity table.
    pub feature_entities: Vec<FeatureEntity>,
    /// Canonical `f7` references between implicit `AllFeatur` entities.
    pub feature_entity_references: Vec<FeatureEntityReference>,
    /// Mixed generated-entity tables from `AllFeatur`, with owner bindings
    /// retained only where their containing feature row is byte-bounded.
    pub feature_entity_tables: Vec<FeatureEntityTable>,
    /// Declared `Geomlists.n_bodies` cardinality, when present.
    pub declared_body_count: Option<u32>,
    /// `Geomlists.first_quilt_ptr`: zero denotes the single-quilt form;
    /// nonzero is a multi-quilt discriminator rather than a body count.
    pub first_quilt_ptr: Option<u32>,
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
        if data[i] != b'\n' || data[i + 1] != b'#' {
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
            role,
        });
    }
    sections
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

fn is_geometry_namespace(section: &Section) -> bool {
    section.role == role::GEOMETRY || section.name == "DEPDB_DATA"
}

fn surface_rows(data: &[u8], sections: &[Section]) -> Vec<SurfaceRow> {
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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

fn surface_prototypes(data: &[u8], sections: &[Section]) -> Vec<SurfacePrototype> {
    let mut prototypes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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

fn plane_local_systems(data: &[u8], sections: &[Section]) -> Vec<PlaneLocalSystem> {
    let mut systems = Vec::new();
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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

fn plane_envelopes(data: &[u8], sections: &[Section]) -> Vec<PlaneEnvelopeRecord> {
    let mut envelopes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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

fn curve_prototypes(data: &[u8], sections: &[Section]) -> Vec<CurvePrototype> {
    let mut prototypes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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

fn curve_expressions(data: &[u8], sections: &[Section]) -> Vec<CurveExpressionRecord> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
        let end = (section.offset + section.length).min(data.len());
        records.extend(
            curve::expression_records(&data[section.offset..end])
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
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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
    for section in sections
        .iter()
        .filter(|section| is_geometry_namespace(section))
    {
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
        if let Some(mut plane) = datum::named_zero_plane(&data[section.offset..end]) {
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
        definitions.extend(
            feature::definitions(&data[section.offset..end])
                .into_iter()
                .map(|mut definition| {
                    offset_feature_definition(&mut definition, section.offset);
                    definition
                }),
        );
        if section.name == "DEPDB_DATA" {
            let payload = &data[section.offset..end];
            let recipe_operations = feature::operations(payload)
                .into_iter()
                .filter(|operation| operation.recipe.is_some())
                .collect::<Vec<_>>();
            if let [operation] = recipe_operations.as_slice() {
                if let Some(mut definition) =
                    feature::depdb_section_definition(payload, operation.feature_id)
                {
                    offset_feature_definition(&mut definition, section.offset);
                    definitions.push(definition);
                }
            }
        }
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

fn depdb_recipe_rows(data: &[u8], sections: &[Section]) -> Vec<FeatureRow> {
    sections
        .iter()
        .filter(|section| section.name == "DEPDB_DATA")
        .filter_map(|section| {
            let end = (section.offset + section.length).min(data.len());
            let payload = &data[section.offset..end];
            let recipe_operations = feature::operations(payload)
                .into_iter()
                .filter(|operation| operation.recipe.is_some())
                .collect::<Vec<_>>();
            let [operation] = recipe_operations.as_slice() else {
                return None;
            };
            Some(FeatureRow {
                feature_id: operation.feature_id,
                header: [0; 2],
                root_schema_class: operation.root_schema_class,
                body: payload.to_vec(),
                body_offset: section.offset,
                offset: section.offset + operation.offset,
            })
        })
        .collect()
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

/// Read the whole file and parse its container framing.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<ContainerScan, CodecError> {
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(CodecError::Io)?;
    let mut data = Vec::new();
    reader.read_to_end(&mut data).map_err(CodecError::Io)?;
    Ok(scan_bytes(data))
}

/// Parse a whole `.prt` byte image. Split out so tests drive it from a synthetic
/// buffer without a reader.
pub fn scan_bytes(data: Vec<u8>) -> ContainerScan {
    let version_line = line_at(&data, 0);

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

    let sections = scan_sections(&data, body_start);
    let layout = identify_layout(&sections);
    let census = geom_census(&data, &sections);
    let principal_unit = principal_unit(&data);
    let family_table = family_table(&data, &sections);
    let surface_rows = surface_rows(&data, &sections);
    let surface_parameters = surface_parameters(&data, &sections);
    let plane_local_systems = plane_local_systems(&data, &sections);
    let plane_envelopes = plane_envelopes(&data, &sections);
    let outline_planes = surface::outline_planes(&plane_envelopes);
    let surface_prototypes = surface_prototypes(&data, &sections);
    let surface_prototype_records = surface_prototype_records(&data, &sections);
    let curve_prototypes = curve_prototypes(&data, &sections);
    let curve_expressions = curve_expressions(&data, &sections);
    let curve_parameters = curve_parameters(&data, &sections);
    let curve_topology_rows = curve_topology_rows(&data, &sections);
    let pcurves = curve::pcurve_endpoints(&curve_parameters, &curve_topology_rows);
    let fc_curve_control_points = curve::fc_control_points(&curve_parameters);
    let fc05_circles = curve::fc05_circles(&curve_parameters);
    let fc05_cylinder_cap_pairs =
        curve::fc05_cylinder_cap_pairs(&fc05_circles, &curve_topology_rows, &surface_rows);
    let prototype_pcurves = prototype_pcurves(&data, &sections);
    let curve_prototype_topology = curve_prototype_topology(&data, &sections);
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
    let feature_direction_bytes = feature::direction_bytes(&feature_rows);
    let mut feature_definitions = feature_definitions(&data, &sections);
    feature::bind_definition_owners(&mut feature_definitions, &feature_geometry_tables);
    let feature_entity_tables =
        feature_entity_tables(&data, &sections, &feature_ids, &surface_rows);
    let feature_section_transforms = placement::resolve(
        &feature_definitions,
        &datum_planes,
        &plane_local_systems,
        &outline_planes,
        &feature_geometry_tables,
        &feature_entity_tables,
        &feature_affected_ids,
    );
    let feature_operations = feature_operations(&data, &sections);
    let (feature_entities, feature_entity_references) = feature_entity_graph(&data, &sections);
    let declared_body_count = geomlists_value(&data, &sections, b"n_bodies\0");
    let first_quilt_ptr = geomlists_value(&data, &sections, b"first_quilt_ptr\0");

    ContainerScan {
        data,
        version_line,
        sections,
        layout,
        census,
        principal_unit,
        family_table,
        surface_rows,
        surface_parameters,
        plane_local_systems,
        plane_envelopes,
        outline_planes,
        surface_prototypes,
        surface_prototype_records,
        curve_prototypes,
        curve_expressions,
        curve_parameters,
        pcurves,
        fc_curve_control_points,
        fc05_circles,
        fc05_cylinder_cap_pairs,
        prototype_pcurves,
        curve_prototype_topology,
        bound_prototype_pcurves,
        curve_topology_rows,
        half_edges,
        loops,
        face_components,
        topological_vertices,
        half_edge_vertex_incidence,
        datum_planes,
        feature_ids,
        feature_rows,
        feature_choices,
        feature_choice_fields,
        feature_geometry_tables,
        feature_affected_ids,
        feature_replay_affected_ids,
        feature_direction_bytes,
        feature_definitions,
        feature_section_transforms,
        feature_operations,
        feature_entities,
        feature_entity_references,
        feature_entity_tables,
        declared_body_count,
        first_quilt_ptr,
    }
}

/// Return whether a thumbnail section contains a JPEG start marker.
pub fn has_thumbnail(scan: &ContainerScan) -> bool {
    scan.sections
        .iter()
        .filter(|s| s.role == role::THUMBNAIL)
        .any(|s| {
            let region = &scan.data[s.offset..(s.offset + s.length).min(scan.data.len())];
            find(region, JPEG_MAGIC, 0).is_some()
        })
}

/// Build a codec-neutral summary of the sections, layout, and namespace census.
pub fn summarize(scan: &ContainerScan) -> ContainerSummary {
    let entries = scan
        .sections
        .iter()
        .map(|s| {
            let mut attributes = BTreeMap::new();
            attributes.insert("offset".to_string(), s.offset.to_string());
            if s.raw_name != s.name {
                attributes.insert("raw_name".to_string(), s.raw_name.clone());
            }
            ContainerEntry {
                name: s.name.clone(),
                role: s.role.to_string(),
                compression: "none".to_string(),
                compressed_size: s.length as u64,
                uncompressed_size: s.length as u64,
                attributes,
            }
        })
        .collect();

    let mut notes = vec![
        format!("PSB container: {}", scan.version_line),
        format!(
            "layout: {}; {} section(s) enumerated",
            scan.layout.token(),
            scan.sections.len()
        ),
    ];

    match (scan.census.srf_array_count, scan.census.crv_array_count) {
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
