// SPDX-License-Identifier: Apache-2.0
//! PSB container framing and structural inspection.
//!
//! A `.prt` begins with an ASCII header block (`#UGC:2 …` through
//! `#-END_OF_UGC_HEADER`), an ASCII table of contents (`#UGC_TOC` …
//! `#END_OF_TOC_HEADER`), then a sequence of named binary sections. Earlier
//! files instead begin an ASCII `P_OBJECT` persistence record immediately after
//! the UGC header. A real body section header is `#\n#<name>\n`. The preceding
//! `#` terminator and printable name distinguish section boundaries from
//! similar bytes in feature data.
//!
//! [`scan_bytes`] reads the stream and returns a [`ContainerScan`] containing section
//! metadata, the persistence layout, namespace counts, typed structural rows,
//! native loops, units, feature identifiers, and datum planes. [`summarize`]
//! converts that scan into the codec-neutral container summary.

use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::bytes::find_from as find;
use cadmpeg_core::CodecError;
use cadmpeg_core::ContainerEntry;
use cadmpeg_ir::ContainerSummary;

use crate::curve::{
    self, BoundPrototypePcurve, CurveExpressionRecord, CurveExpressionValue, CurveParameterRecord,
    CurvePrototype, CurvePrototypeTopology, CurveTopologyRow, DepdbCurveRow,
    ExternalRelationSymbols, Fc05Circle, Fc05CylinderCapPair, FcCurveCoordinates, PcurveEndpoints,
    PrototypePcurveEndpoints, TwoChartPcurveSamples,
};
use crate::datum::{self, DatumCylinder, DatumPlaneRecord};
use crate::feature;
use crate::feature::definitions::FeatureDefinition;
use crate::feature::entity::{FeatureEntity, FeatureEntityReference, FeatureEntityTable};
use crate::feature::operations::{
    FeatureOperation, FeatureOperationState, FeatureRecipe, FeatureReferenceName,
};
use crate::feature::rows::{
    FeatureAffectedIds, FeatureChoice, FeatureChoiceField, FeatureGeometryTable,
    FeatureLoopHistoryEntry, FeatureLoopRestoreDirection, FeatureReplayAffectedIds,
    FeatureRevolutionExtent, FeatureRow,
};
use crate::layout::cmnm_model_name_record as cmnm;
use crate::legacy;
use crate::legacy::type_code::LegacyTypeCode;
use crate::loop_array::{self, LoopArrayFrame, LoopArrayRecord, LoopArrayScan};
use crate::placement::{self, FeatureSectionTransform};
use crate::primdata::{self, PrimitiveScalarArray, PrimitiveTriangleStrip};
use crate::psb;
use crate::reference::{self, ReferenceCircle, ReferenceConic, ReferenceEllipse, ReferenceLine};
use crate::surface::{
    self, OutlinePlane, PlaneEnvelopeRecord, PlaneLocalSystem, SurfaceContourRecord,
    SurfaceParameterRecord, SurfacePrototypeRecord, SurfaceRow, TabulatedCylinderCurveReplay,
};
use crate::topology::{
    self, FaceComponent, HalfEdge, HalfEdgeVertexIncidence, Loop, TopologicalVertex,
};

/// The PSB magic: every Creo `.prt` opens with this ASCII framing line.
const MAGIC: &[u8] = b"#UGC:2";

/// End of the UGC header block.
const UGC_HEADER_END: &[u8] = b"#-END_OF_UGC_HEADER";
/// Start of the ASCII table of contents.
const TOC_START: &[u8] = b"#UGC_TOC";
/// End of the ASCII table of contents.
const TOC_END: &[u8] = b"#END_OF_TOC_HEADER";
/// Start of the legacy ASCII persistence object.
const LEGACY_OBJECT_START: &[u8] = b"#P_OBJECT ";
/// End of the legacy ASCII persistence object.
const LEGACY_OBJECT_END: &[u8] = b"#END_OF_P_OBJECT";
/// Banner following a complete legacy ASCII persistence object.
const LEGACY_BANNER_START: &[u8] = b"#Pro/ENGINEER";
/// JPEG SOI magic, marking the `THMB_IMG_MAIN` preview payload (never geometry).
pub(crate) const JPEG_MAGIC: &[u8] = &[0xff, 0xd8, 0xff];
/// Unix `compress` payload prefix.
pub(crate) const UNIX_COMPRESS_MAGIC: &[u8] = &crate::layout::unix_compress_header::MAGIC_VALUE;

/// ASCII names that appear in the header/TOC framing and look like section
/// headers but are structural markers, not body sections ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)).
const FRAMING_NAMES: &[&str] = &[
    "-END_OF_UGC_HEADER",
    "END_OF_P_OBJECT",
    "END_OF_UGC",
    "UGC_TOC",
    "END_OF_TOC_HEADER",
    "NEXT_TOC_ENTRY",
];

/// The visible-geometry section name whose `srf_array`/`crv_array` counts drive
/// the inspect census.
const VISIBGEOM: &str = "VisibGeom";
/// Named active-unit selector. Unit-definition tables can contain inactive
/// systems, so this selector, rather than another unit-name string, is
/// authoritative.
const PRINCIPAL_UNIT_ID: &[u8] = b"_principal_sys_units_id\0";

/// The persistence layout families ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). Dispatched structurally, not per-file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Dense PSB rows in `VisibGeom` (~40+ sections; `ND:` name decoration).
    Nd,
    /// Sparse PSB views plus a persistence database (`DEPDB_DATA`, ~12 sections).
    Depdb,
    /// ASCII `P_OBJECT` persistence used before the ND and DEPDB byte grammars.
    LegacyAscii(Box<LegacyAsciiFraming>),
    /// No verified layout matched, with the failed discriminant retained.
    Unknown(UnknownLayout),
}

/// Why no verified Creo persistence layout matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnknownLayout {
    /// `DEPDB_DATA` was present but did not start with its required root record.
    DepdbRootMissing,
    /// No DEPDB root, ND decoration, or complete legacy object was present.
    NoDiscriminant,
}

/// Header metadata from a complete legacy ASCII `P_OBJECT` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyAsciiFraming {
    /// Decimal persistence-schema token following `#P_OBJECT`.
    pub(crate) schema: String,
    /// Product release token in a `Version` or `Release` banner form.
    pub(crate) product_release: Option<String>,
    /// Byte offset of the `#Pro/ENGINEER` banner and legacy TOC offset base.
    banner_offset: usize,
    /// Byte offset of the header-adjacent `#P_OBJECT` line.
    object_offset: usize,
    /// Structurally resolved attribute declarations and value rows.
    pub(crate) persistence: legacy::Persistence,
}

impl Layout {
    pub(crate) fn legacy_ascii(&self) -> Option<&LegacyAsciiFraming> {
        match self {
            Self::LegacyAscii(framing) => Some(framing),
            Self::Nd | Self::Depdb | Self::Unknown(_) => None,
        }
    }

    /// A short, stable token for human reports.
    pub(crate) fn token(&self) -> &'static str {
        match self {
            Layout::Nd => "ND",
            Layout::Depdb => "DEPDB",
            Layout::LegacyAscii(_) => "LEGACY_ASCII",
            Layout::Unknown(_) => "unknown",
        }
    }
}

/// One enumerated binary section.
///
/// The extent is a fact of the type. [`Section::new`] is the only constructor
/// and it admits a section only when `offset..end` is a region of the file the
/// scan read, so no reader re-derives the sum and none of them can overflow.
///
/// `offset` and `length` are private, so a struct literal outside this module
/// and its descendants does not compile and there is no spelling of a section
/// whose extent the file does not hold.
#[derive(Debug, Clone)]
pub(crate) struct Section {
    /// Raw name as it appeared in the header, when decorated.
    pub(crate) raw_name: String,
    /// Byte offset of the section header within the file.
    offset: usize,
    /// Payload length in bytes (header to the next section, or EOF).
    length: usize,
    /// Expanded payload length from the TOC, excluding the section header.
    expanded_length: Option<usize>,
}

/// One declared section together with the bytes it was admitted against.
///
/// [`Section::scan`] is the only constructor. It admits `offset..end` against
/// the file the scan is reading and keeps that region, so every reader inside
/// the scan reads the section's bytes with no second bound and no `Option`.
/// The scan's readers hold this type; [`FramingScan`] stores the owned
/// [`Section`] once the `Cow` takes the bytes.
#[derive(Debug, Clone)]
pub(crate) struct ScannedSection<'a> {
    /// The declared section.
    pub(crate) section: Section,
    /// The section's payload bytes in the file the scan read it from.
    region: &'a [u8],
}

impl Section {
    /// The section whose payload is `data[offset..end]`, with those bytes, or
    /// `None` when that is not a region of `data`: an end before the offset, or
    /// past the last byte of the file.
    pub(crate) fn scan(
        raw_name: String,
        offset: usize,
        end: usize,
        expanded_length: Option<usize>,
        data: &[u8],
    ) -> Option<ScannedSection<'_>> {
        let region = data.get(offset..end)?;
        Some(ScannedSection {
            section: Self {
                raw_name,
                offset,
                length: region.len(),
                expanded_length,
            },
            region,
        })
    }

    /// Byte offset of the section header within the file.
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    /// Payload length in bytes.
    pub(crate) fn length(&self) -> usize {
        self.length
    }

    /// Byte offset one past the section's last byte.
    ///
    /// Plain `+`: [`Section::new`] admitted the sum, so it is a byte offset of
    /// the file the scan read.
    pub(crate) fn end(&self) -> usize {
        self.offset + self.length
    }

    /// Whether `offset` is a byte of the section: at or after its offset and
    /// before its end. This is the one range predicate over a section.
    pub(crate) fn contains(&self, offset: usize) -> bool {
        (self.offset()..self.end()).contains(&offset)
    }

    /// Normalized section name.
    pub(crate) fn name(&self) -> &str {
        normalize_name(&self.raw_name)
    }

    /// Payload role derived from the section name.
    pub(crate) fn role(&self) -> SectionRole {
        classify(self.name())
    }
}

/// The five payload roles admitted by a Creo section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SectionRole {
    PsbGeometry,
    ModelData,
    Thumbnail,
    Metadata,
    Opaque,
}

impl From<SectionRole> for ContainerRole {
    fn from(role: SectionRole) -> Self {
        match role {
            SectionRole::PsbGeometry => Self::PsbGeometry,
            SectionRole::ModelData => Self::ModelData,
            SectionRole::Thumbnail => Self::Thumbnail,
            SectionRole::Metadata => Self::Metadata,
            SectionRole::Opaque => Self::Opaque,
        }
    }
}

/// A section payload decoded from Unix `compress` framing.
#[derive(Debug, Clone)]
pub(crate) struct ExpandedSection {
    /// Normalized owning section name.
    pub(crate) name: String,
    /// Offset of the compressed payload in the source file.
    pub(crate) source_offset: usize,
    /// Number of compressed source bytes.
    pub(crate) compressed_length: usize,
    /// Complete expanded PSB payload.
    pub(crate) data: Vec<u8>,
}

/// One counted model-level `double_xar` dictionary from an expanded section.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelDoubleXarTable {
    /// Normalized owning section name.
    pub(crate) section_name: String,
    /// Source-file offset of the compressed section payload.
    pub(crate) section_source_offset: usize,
    /// Offset of the table label in the expanded section.
    pub(crate) expanded_offset: usize,
    /// Entries in stored order.
    pub(crate) entries: Vec<crate::scalar::DoubleXarSlot>,
}

/// The byte-backed count headers read from the visible-geometry section.
#[derive(Debug, Clone, Default)]
pub(crate) struct GeomCensus {
    /// `srf_array\0 f8 <count>` surface-namespace count, when present.
    pub(crate) srf_array_count: Option<u32>,
    /// `crv_array\0 [f3] f8 <count>` curve-namespace count, when present.
    pub(crate) crv_array_count: Option<u32>,
}

/// Configuration family-table pointer carried by `FamilyInf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FamilyTablePointer {
    /// Explicit `e1` null pointer.
    Null,
    /// Canonical `f7` entity reference to a driver table.
    Entity(u32),
}

/// Typed `drv_tbl_ptr` field from `FamilyInf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FamilyTableRecord {
    /// Null or referenced driver table.
    pub(crate) pointer: FamilyTablePointer,
    /// Byte offset of the pointer value.
    pub(crate) offset: usize,
}

/// Structural data read from one `.prt` file. Decoded products are grouped
/// into per-domain sub-structs so each consumer names the domain it reads.
/// `ContainerScan` is never serialized; grouping and field naming are internal
/// and do not affect IR or JSON output.
pub(crate) struct ContainerScan<'a> {
    /// Container framing: raw bytes, header, TOC-enumerated sections, and
    /// model-level diagnostics.
    pub(crate) framing: FramingScan<'a>,
    /// Named scalar and triangle-strip products from expanded primitive data.
    pub(crate) primitives: PrimitiveScan,
    /// Model-space reference entities decoded from `MdlRefInfo`.
    pub(crate) references: ReferenceScan,
    /// Typed surface rows, parameter bodies, and prototypes across the model,
    /// non-visible, and cross-section namespaces.
    pub(crate) surfaces: SurfaceScan,
    /// Plane support frames, envelopes, placed planes, and datum planes.
    pub(crate) planes: PlaneScan,
    /// Curve prototypes, parameter bodies, pcurves, and native curve rows.
    pub(crate) curves: CurveScan,
    /// Native half-edge adjacency graph resolved from curve topology rows.
    pub(crate) topology: TopologyScan,
    /// Native `lo_array` frame headers and complete positional roster rows.
    pub(crate) loop_arrays: LoopArrayScan,
    /// Feature rows, definitions, operations, and the implicit entity graph.
    pub(crate) features: FeatureScan,
}

/// Native model name and its source position.
pub(crate) struct ModelName {
    pub(crate) name: String,
    pub(crate) offset: usize,
}

/// Container framing: raw bytes, header, sections, and model-level diagnostics.
pub(crate) struct FramingScan<'a> {
    /// Complete source bytes.
    pub(crate) data: Cow<'a, [u8]>,
    /// The magic/version header line, ASCII, trimmed.
    pub(crate) version_line: String,
    /// Native root model filename or name from `CMNM` or a binary
    /// `model_name` field.
    pub(crate) model_name: Option<ModelName>,
    /// Enumerated sections in file order.
    pub(crate) sections: Vec<Section>,
    /// Successfully expanded Unix-compress section payloads.
    pub(crate) expanded_sections: Vec<ExpandedSection>,
    /// Identified layout family.
    pub(crate) layout: Layout,
    /// Visible-geometry namespace census, when a `VisibGeom` section was found.
    pub(crate) census: GeomCensus,
    /// Active Creo principal coordinate unit system, when its selector is
    /// present and unambiguous.
    pub(crate) principal_unit: Option<legacy::PrincipalUnitSystem>,
    /// Configuration driver-table pointer from `FamilyInf`.
    pub(crate) family_table: Option<FamilyTableRecord>,
    /// Complete legacy ASCII family-table root and ordered rows, when joined.
    pub(crate) legacy_family_table: Option<crate::legacy_family::FamilyTable>,
    /// Declared `Geomlists.n_bodies` cardinality, when present.
    pub(crate) declared_body_count: Option<u32>,
    /// `Geomlists.first_quilt_ptr`: zero denotes the single-quilt form;
    /// nonzero is a multi-quilt discriminator rather than a body count.
    pub(crate) first_quilt_ptr: Option<u32>,
}

/// Named products from expanded primitive-data sections.
pub(crate) struct PrimitiveScan {
    /// Counted model-level scalar dictionaries from expanded sections.
    pub(crate) double_xar_tables: Vec<ModelDoubleXarTable>,
    /// Complete named model-space scalar arrays from expanded primitive data.
    pub(crate) scalar_arrays: Vec<PrimitiveScalarArray>,
    /// Complete named position-only triangle strips from expanded primitive data.
    pub(crate) triangle_strips: Vec<PrimitiveTriangleStrip>,
    /// Triangle-strip records whose complete position or normal representations disagree.
    pub(crate) conflicting_triangle_strip_representation_count: usize,
}

/// Model-space reference entities decoded from `MdlRefInfo`.
pub(crate) struct ReferenceScan {
    /// Complete model-space line entities from `MdlRefInfo`.
    pub(crate) lines: Vec<ReferenceLine>,
    /// Complete model-Z circular entities from `MdlRefInfo` rows.
    pub(crate) circles: Vec<ReferenceCircle>,
    /// Named conic entities from `MdlRefInfo` with complete defining fields.
    pub(crate) conics: Vec<ReferenceConic>,
    /// Conic records whose complete fields independently define an ellipse.
    pub(crate) ellipses: Vec<ReferenceEllipse>,
}

/// Typed surface rows, parameter bodies, and prototypes.
pub(crate) struct SurfaceScan {
    /// Typed fixed-prefix surface rows from the selected material model
    /// geometry namespace. Parameter bodies are decoded separately.
    pub(crate) rows: Vec<SurfaceRow>,
    /// Typed fixed-prefix rows from the separate invisible and construction
    /// surface namespace.
    pub(crate) nonvisible_rows: Vec<SurfaceRow>,
    /// Typed fixed-prefix surface rows from the DEPDB cross-section geometry
    /// namespace. These are kept separate from model-face surface rows.
    pub(crate) cross_section_rows: Vec<SurfaceRow>,
    /// Bounded scalar parameter bodies from positional surface rows.
    pub(crate) parameters: Vec<SurfaceParameterRecord>,
    /// Bounded scalar parameter bodies from the separate invisible and
    /// construction surface namespace.
    pub(crate) nonvisible_parameters: Vec<SurfaceParameterRecord>,
    /// Bounded scalar parameter bodies from DEPDB cross-section surface rows.
    pub(crate) cross_section_parameters: Vec<SurfaceParameterRecord>,
    /// Complete positional contour-chain entries from the selected material
    /// model geometry namespace.
    pub(crate) contours: Vec<SurfaceContourRecord>,
    /// Complete positional contour-chain entries from the separate invisible
    /// and construction surface namespace.
    pub(crate) nonvisible_contours: Vec<SurfaceContourRecord>,
    /// Complete positional contour-chain entries from DEPDB cross-section
    /// geometry.
    pub(crate) cross_section_contours: Vec<SurfaceContourRecord>,
    /// Count of labeled known-family prototypes plus unlabeled `geom_type` records.
    pub(crate) prototype_count: usize,
    /// Bounded named `srf_prim_ptr(<kind>)` parameter records.
    pub(crate) prototype_records: Vec<SurfacePrototypeRecord>,
    /// Bounded named surface-prototype records from the separate invisible
    /// and construction geometry namespace.
    pub(crate) nonvisible_prototype_records: Vec<SurfacePrototypeRecord>,
    /// Named prototype fields whose bounded scalar body the decoder refused,
    /// each stating its record, field, declared slot count and the slot and
    /// byte it refused at. The field bytes are retained opaque.
    pub(crate) prototype_field_refusals: Vec<String>,
    /// The same, for the separate invisible and construction geometry
    /// namespace.
    pub(crate) nonvisible_prototype_field_refusals: Vec<String>,
    /// Complete analytic carriers from legacy visible surface prototypes.
    pub(crate) legacy_carriers: Vec<crate::legacy_geometry::LegacySurfaceCarrier>,
}

/// Plane support frames, envelopes, placed planes, and datum planes.
pub(crate) struct PlaneScan {
    /// Inherited support frames following positional plane envelopes.
    pub(crate) local_systems: Vec<PlaneLocalSystem>,
    /// Plane support frames from the DEPDB cross-section namespace.
    pub(crate) cross_section_local_systems: Vec<PlaneLocalSystem>,
    /// Plane-specific standard and compact positional envelopes.
    pub(crate) envelopes: Vec<PlaneEnvelopeRecord>,
    /// Plane envelopes from the DEPDB cross-section namespace.
    pub(crate) cross_section_envelopes: Vec<PlaneEnvelopeRecord>,
    /// Axis-aligned placed planes derived from unambiguous outline corners.
    pub(crate) outlines: Vec<OutlinePlane>,
    /// Axis-aligned planes from marker-bound six-scalar positional frames.
    pub(crate) positional_frames: Vec<OutlinePlane>,
    /// Placed planes derived inside the DEPDB cross-section namespace.
    pub(crate) cross_section_outlines: Vec<OutlinePlane>,
    /// Model-space standard datum planes decoded from `ActDatums` outlines.
    pub(crate) datums: Vec<DatumPlaneRecord>,
    /// Complete model-space cylinder carriers decoded from active-datum
    /// surface rows.
    pub(crate) datum_cylinders: Vec<DatumCylinder>,
}

/// Curve prototypes, parameter bodies, pcurves, and native curve rows.
pub(crate) struct CurveScan {
    /// Cubic curve replay records bound to following tabulated-cylinder rows.
    pub(crate) tabulated_cylinder_replays: Vec<TabulatedCylinderCurveReplay>,
    /// Labeled curve prototypes from geometry sections. The curve body and
    /// its analytic interpretation are decoded separately.
    pub(crate) prototypes: Vec<CurvePrototype>,
    /// Labeled curve prototypes from the separate invisible and construction
    /// geometry namespace.
    pub(crate) nonvisible_prototypes: Vec<CurvePrototype>,
    /// Labeled first curve rows from DEPDB cross-section namespaces.
    pub(crate) cross_section_prototypes: Vec<CurvePrototype>,
    /// Source programs from curve-from-equation entity records.
    pub(crate) expressions: Vec<CurveExpressionRecord>,
    /// Bounded analytic parameter bodies from positional curve rows.
    pub(crate) parameters: Vec<CurveParameterRecord>,
    /// Bounded curve parameter bodies from the separate invisible and
    /// construction geometry namespace.
    pub(crate) nonvisible_parameters: Vec<CurveParameterRecord>,
    /// Complete eight-slot pcurve endpoints in both adjacent face frames.
    pub(crate) pcurves: Vec<PcurveEndpoints>,
    /// Ordered, pointwise-corresponding samples in both incident-face charts.
    pub(crate) two_chart_pcurves: Vec<TwoChartPcurveSamples>,
    /// Ordered world-coordinate lanes from FC-prefixed dense curve rows.
    pub(crate) fc_coordinates: Vec<FcCurveCoordinates>,
    /// FC05 records whose decoded points prove an exact circle.
    pub(crate) fc05_circles: Vec<Fc05Circle>,
    /// Cylinder cap groups joined through typed curve-face topology. Their
    /// model-space feature frame remains required before IR transfer.
    pub(crate) fc05_cylinder_cap_pairs: Vec<Fc05CylinderCapPair>,
    /// Complete pcurve UV endpoints from labeled curve prototypes.
    pub(crate) prototype_pcurves: Vec<PrototypePcurveEndpoints>,
    /// Labeled face and next-edge references from curve prototypes.
    pub(crate) prototype_topology: Vec<CurvePrototypeTopology>,
    /// Prototype pcurve endpoints bound to their adjacent face identifiers.
    pub(crate) bound_prototype_pcurves: Vec<BoundPrototypePcurve>,
    /// Curve rows with an unambiguous canonical four-reference topology
    /// suffix. These rows define the native half-edge adjacency graph.
    pub(crate) topology_rows: Vec<CurveTopologyRow>,
    /// Curve rows from the separate invisible and construction geometry
    /// namespace. These rows do not participate in model topology.
    pub(crate) nonvisible_topology_rows: Vec<CurveTopologyRow>,
    /// Complete one-sided curve rows from the DEPDB cross-section namespace.
    pub(crate) cross_section_rows: Vec<DepdbCurveRow>,
}

/// Native half-edge adjacency graph resolved from curve topology rows.
pub(crate) struct TopologyScan {
    /// Resolved native half-edges and closed loops built from curve rows.
    pub(crate) half_edges: Vec<HalfEdge>,
    /// Closed rings of half-edges, one per resolved face loop.
    pub(crate) loops: Vec<Loop>,
    /// Connected components of non-null face references in native curve
    /// topology. These are not emitted IR shells.
    pub(crate) face_components: Vec<FaceComponent>,
    /// Topological vertex identities derived from half-edge orbits.
    pub(crate) vertices: Vec<TopologicalVertex>,
    /// Start/end vertex binding for each decoded half-edge.
    pub(crate) half_edge_vertex_incidence: Vec<HalfEdgeVertexIncidence>,
    /// The seed half-edge of every orbit past the one-based `u32` vertex
    /// identifier space. Each names an orbit that states no topological
    /// vertex, so its half-edges carry no incidence.
    pub(crate) unstatable_vertex_orbits: Vec<crate::topology::HalfEdgeId>,
}

/// Feature rows, definitions, operations, and the implicit entity graph.
pub(crate) struct FeatureScan {
    /// Feature IDs that own decoded geometry rows.
    pub(crate) ids: Vec<u32>,
    /// Byte-bounded `AllFeatur` rows for known geometry-owning features.
    pub(crate) rows: Vec<FeatureRow>,
    /// Short-form radius candidates from bounded class-913 round replays.
    pub(crate) round_replay_scalars: Vec<crate::feature::rows::FeatureRoundReplayScalar>,
    /// Section-bounded procedural recipe rows synthesized from `DEPDB_DATA`.
    pub(crate) depdb_recipe_rows: Vec<FeatureRow>,
    /// Labeled procedural-choice spans inside decoded feature rows.
    pub(crate) choices: Vec<FeatureChoice>,
    /// Named fields and typed wrappers inside procedural-choice spans.
    pub(crate) choice_fields: Vec<FeatureChoiceField>,
    /// Generated-geometry namespace headers owned by decoded features.
    pub(crate) geometry_tables: Vec<FeatureGeometryTable>,
    /// Ordered feature-local loop identities from complete `lo_hist` rosters.
    pub(crate) loop_history_entries: Vec<FeatureLoopHistoryEntry>,
    /// Complete named affected-ID arrays owned by decoded features.
    pub(crate) affected_ids: Vec<FeatureAffectedIds>,
    /// Affected-ID runs from unlabeled positional replay feature rows.
    pub(crate) replay_affected_ids: Vec<FeatureReplayAffectedIds>,
    /// Affected geometry, edge, and quilt arrays from class-946 replay rows.
    pub(crate) surface_merge_replay_affected_ids:
        Vec<crate::feature::rows::FeatureSurfaceMergeAffectedIds>,
    /// Named compact direction values from loop-restoration records.
    pub(crate) loop_restore_directions: Vec<FeatureLoopRestoreDirection>,
    /// Resolved angular termination from rotational feature rows.
    pub(crate) revolution_extents: Vec<FeatureRevolutionExtent>,
    /// Byte-bounded `FeatDefs` records and definition-space parameter frames.
    pub(crate) definitions: Vec<FeatureDefinition>,
    /// Section-to-model frames resolved from perpendicular active datums.
    pub(crate) section_transforms: Vec<FeatureSectionTransform>,
    /// Every stored feature-operation state from `MdlStatus`, in byte order.
    pub(crate) operation_states: Vec<FeatureOperationState>,
    /// Unambiguous or consensus feature-operation projection for each identifier.
    pub(crate) operations: Vec<FeatureOperation>,
    /// Feature names joined to model feature identifiers by reference data.
    pub(crate) reference_names: Vec<FeatureReferenceName>,
    /// Named records in the implicit `AllFeatur` walker-order entity table.
    pub(crate) entities: Vec<FeatureEntity>,
    /// Canonical `f7` references between implicit `AllFeatur` entities.
    pub(crate) entity_references: Vec<FeatureEntityReference>,
    /// Mixed generated-entity tables from `AllFeatur`, with owner bindings
    /// retained only where their containing feature row is byte-bounded.
    pub(crate) entity_tables: Vec<FeatureEntityTable>,
    /// Legacy ASCII round features joined to their dimensions and result edges.
    pub(crate) legacy_rounds: Vec<crate::legacy_feature::LegacyRoundFeature>,
}

/// Whether a byte prefix is a Creo PSB `.prt`: the `#UGC:2` ASCII magic is the
/// container signature ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). Detection is magic-based, never
/// extension-based, because `.prt` is shared with Siemens NX ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)).
pub(crate) fn looks_like_creo(prefix: &[u8]) -> bool {
    prefix.starts_with(MAGIC)
}

fn line_at(data: &[u8], start: usize) -> String {
    let end = find(data, b"\n", start).unwrap_or(data.len());
    String::from_utf8_lossy(&data[start..end])
        .trim()
        .to_string()
}

/// Normalize a decorated section name to its base ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)): strip a
/// `ModelView#N` suffix and an `ND:0:<Name>:N` decoration.
fn normalize_name(raw: &str) -> &str {
    let base = raw.split('#').next().unwrap_or(raw);
    base.strip_prefix("ND:")
        .and_then(|rest| rest.split(':').nth(1))
        .unwrap_or(base)
}

/// Classify a normalized section name by what it carries ([spec §2.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#12-section-map)).
fn classify(name: &str) -> SectionRole {
    match name {
        "VisibGeom" | "NovisGeom" | "ActDatums" => SectionRole::PsbGeometry,
        "AllFeatur" | "FeatDefs" | "FeatDefsIndex" | "FeatDefsDtm" | "Geomlists" | "GeomDepen"
        | "Model_L05_PX" | "Model_L05P" | "BasicData" | "BasBasData" | "BasFullData"
        | "FullMData" => SectionRole::ModelData,
        "THMB_IMG_MAIN" => SectionRole::Thumbnail,
        "NeuPrtSld" | "NeuAsmSld" | "SolidPersistTable" | "SolidPrimdata" | "DEPDB_DATA"
        | "UnitSystemDef_L03" | "PDMTrail_L03" | "ActEntity" | "MdlStatus" | "MdlRefInfo"
        | "DispCntrl" | "ColorSchemeInfo" | "LargeText" | "BasicText" | "IdsGenInfoDb" => {
            SectionRole::Metadata
        }
        _ => SectionRole::Opaque,
    }
}

/// Enumerate binary sections from `body_start` to EOF by the `\n#<name>\n`
/// header rule ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). A candidate header is accepted only when its name is
/// a printable run and is not one of the header/TOC framing markers.
fn scan_sections(data: &[u8], body_start: usize) -> Result<Vec<ScannedSection<'_>>, CodecError> {
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
            let directory_end = hits.first().map_or(body_start, |(offset, _)| *offset);
            let Some(directory) = data.get(..directory_end) else {
                return Err(CodecError::malformed(format!(
                    "creo section `{raw}` is TOC-delimited and its directory window ends at \
                     {directory_end}, past the file length {}",
                    data.len(),
                )));
            };
            if !toc_lists_section(directory, name_bytes) {
                continue;
            }
        }
        hits.push((hash_off, raw));
    }

    let mut sections = Vec::with_capacity(hits.len());
    for (idx, (hdr_off, raw)) in hits.iter().enumerate() {
        let end = hits.get(idx + 1).map_or(data.len(), |(next, _)| *next);
        sections.extend(Section::scan(raw.clone(), *hdr_off, end, None, data));
    }
    Ok(sections)
}

fn toc_sections(data: &[u8], header_base: usize) -> Vec<ScannedSection<'_>> {
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
            let Some(end) = offset.checked_add(length) else {
                continue;
            };
            if length < marker.len() || data.get(offset..marker_end) != Some(marker.as_slice()) {
                continue;
            }
            sections.extend(Section::scan(
                raw_name,
                offset,
                end,
                Some(expanded_length),
                data,
            ));
        }
    }
    sections.sort_by_key(|section| section.section.offset());
    sections.dedup_by_key(|section| section.section.offset());
    sections
}

fn legacy_toc_sections(data: &[u8], banner_offset: usize) -> Vec<ScannedSection<'_>> {
    const MAX_LEGACY_TOC_ENTRIES: usize = 4096;

    let Some(toc_offset) = find(data, b"\n@Toc ", banner_offset).map(|offset| offset + 1) else {
        return Vec::new();
    };
    let Some((toc_declaration, after_toc_declaration)) = legacy::line(data, toc_offset) else {
        return Vec::new();
    };
    let Some(toc_declaration) =
        legacy::parse_declaration(toc_declaration, toc_offset).filter(|declaration| {
            declaration.name == "Toc" && matches!(declaration.type_code, LegacyTypeCode::Object)
        })
    else {
        return Vec::new();
    };
    let toc_id = toc_declaration.id;
    let Some((toc_value, after_toc_value)) = legacy::line(data, after_toc_declaration) else {
        return Vec::new();
    };
    let Ok(toc_value) = std::str::from_utf8(toc_value) else {
        return Vec::new();
    };
    let mut toc_fields = toc_value.split_ascii_whitespace();
    if toc_fields.next() != Some("0")
        || toc_fields.next().and_then(|id| id.parse::<u32>().ok()) != Some(toc_id)
        || toc_fields.next() != Some("->")
        || toc_fields.next().is_some()
    {
        return Vec::new();
    }

    let Some((entry_declaration, after_entry_declaration)) = legacy::line(data, after_toc_value)
    else {
        return Vec::new();
    };
    let Some(entry_declaration) = legacy::parse_declaration(entry_declaration, after_toc_value)
        .filter(|declaration| {
            declaration.name == "entry" && matches!(declaration.type_code, LegacyTypeCode::String)
        })
    else {
        return Vec::new();
    };
    let entry_id = entry_declaration.id;
    let Some((entry_array, mut next)) = legacy::line(data, after_entry_declaration) else {
        return Vec::new();
    };
    let Ok(entry_array) = std::str::from_utf8(entry_array) else {
        return Vec::new();
    };
    let mut array_fields = entry_array.split_ascii_whitespace();
    if array_fields.next() != Some("1")
        || array_fields.next().and_then(|id| id.parse::<u32>().ok()) != Some(entry_id)
    {
        return Vec::new();
    }
    let Some(count) = array_fields
        .next()
        .and_then(|count| count.strip_prefix('['))
        .and_then(|count| count.strip_suffix(']'))
        .and_then(|count| count.parse::<usize>().ok())
        .filter(|count| *count <= MAX_LEGACY_TOC_ENTRIES)
    else {
        return Vec::new();
    };
    if array_fields.next().is_some() {
        return Vec::new();
    }

    let mut sections = Vec::new();
    for _ in 0..count {
        let Some((entry, after_entry)) = legacy::line(data, next) else {
            break;
        };
        next = after_entry;
        let Ok(entry) = std::str::from_utf8(entry) else {
            continue;
        };
        let entry = entry.trim_end_matches('#').trim_end();
        let fields = entry.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() == 2 {
            continue;
        }
        if fields.len() != 7
            || fields[0] != "2"
            || fields[1].parse::<u32>().ok() != Some(entry_id)
            || fields[5] != "0"
            || fields[6].parse::<u32>().is_err()
        {
            continue;
        }
        let raw_name = fields[2];
        if raw_name.len() < 2
            || !raw_name.bytes().all(is_name_byte)
            || !raw_name.bytes().any(|byte| byte.is_ascii_alphanumeric())
        {
            continue;
        }
        let (Ok(relative_offset), Ok(length)) = (
            usize::from_str_radix(fields[3], 16),
            usize::from_str_radix(fields[4], 16),
        ) else {
            continue;
        };
        let Some(offset) = banner_offset.checked_add(relative_offset) else {
            continue;
        };
        let marker = [b"#".as_slice(), raw_name.as_bytes(), b"\n"].concat();
        let Some(marker_end) = offset.checked_add(marker.len()) else {
            continue;
        };
        let Some(end) = offset.checked_add(length) else {
            continue;
        };
        if length < marker.len() || data.get(offset..marker_end) != Some(marker.as_slice()) {
            continue;
        }
        sections.extend(Section::scan(raw_name.to_string(), offset, end, None, data));
    }
    sections.sort_by_key(|section| section.section.offset());
    sections.dedup_by_key(|section| section.section.offset());
    sections
}

fn expanded_sections(data: &[u8], sections: &[ScannedSection<'_>]) -> Vec<ExpandedSection> {
    const MAX_EXPANDED_SECTION: usize = 256 * 1024 * 1024;
    sections
        .iter()
        .filter_map(|section| {
            let expected_length = section.section.expanded_length?;
            if expected_length > MAX_EXPANDED_SECTION {
                return None;
            }
            let header_length = section.section.raw_name.len().checked_add(2)?;
            let source_offset = section.section.offset().checked_add(header_length)?;
            let payload = data.get(source_offset..section.section.end())?;
            if !payload.starts_with(UNIX_COMPRESS_MAGIC) {
                return None;
            }
            let expanded = crate::compress::decode(payload, expected_length)?;
            Some(ExpandedSection {
                name: section.section.name().to_string(),
                source_offset,
                compressed_length: payload.len(),
                data: expanded,
            })
        })
        .collect()
}

/// Find the expanded payload owned by one section.
pub(crate) fn expanded_section_for<'a>(
    scan: &'a ContainerScan<'_>,
    section: &Section,
) -> Option<&'a ExpandedSection> {
    scan.framing.expanded_sections.iter().find(|expanded| {
        // An expanded payload begins after its section's `#<name>\n` header, so
        // its source offset is inside the section but never its first byte.
        expanded.name == section.name()
            && section.contains(expanded.source_offset)
            && expanded.source_offset != section.offset()
    })
}

/// The payload bytes of one declared section, in the file the scan read it
/// from.
///
/// The `None` is `data`'s own bound. `data` is a free parameter, so the type
/// cannot state that it is the file the section was scanned from. Inside the
/// scan no caller states that bound: [`ScannedSection`] carries the region
/// [`Section::scan`] admitted. The callers left are the four that hold an owned
/// [`Section`] after the scan and pass `scan.framing.data` beside it
/// ([`has_thumbnail`], `decode::build::passthrough`, and the two in
/// `decode::surfaces::prototypes`); for them the pairing is the free
/// parameter's own bound and this `None` states it.
pub(crate) fn section_region<'a>(data: &'a [u8], section: &Section) -> Option<&'a [u8]> {
    data.get(section.offset()..section.end())
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

const DEPDB_ROOT_RECORD: &[u8] = b"\xe0\x00p_dep_db\0\xe3";

fn legacy_product_release(banner: &[u8]) -> Option<String> {
    let mut words = banner
        .split(u8::is_ascii_whitespace)
        .filter(|word| !word.is_empty());
    while let Some(word) = words.next() {
        if word == b"Version" || word == b"Release" {
            let release = words.next()?;
            if release.iter().all(u8::is_ascii_graphic) {
                return String::from_utf8(release.to_vec()).ok();
            }
            return None;
        }
        if let Some(release) = word.strip_prefix(b"Release") {
            if !release.is_empty() && release.iter().all(u8::is_ascii_graphic) {
                return String::from_utf8(release.to_vec()).ok();
            }
        }
    }
    None
}

fn legacy_ascii_framing(data: &[u8]) -> Option<LegacyAsciiFraming> {
    let header_end = find(data, UGC_HEADER_END, 0)
        .and_then(|offset| offset.checked_add(UGC_HEADER_END.len()))?;
    let body = data
        .get(header_end..)
        .and_then(|tail| tail.strip_prefix(b"\n"))?;
    if !body.starts_with(LEGACY_OBJECT_START) {
        return None;
    }
    let object_header_end = find(body, b"\n", LEGACY_OBJECT_START.len())?;
    let schema = &body[LEGACY_OBJECT_START.len()..object_header_end];
    if schema.is_empty() || !schema.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let schema = String::from_utf8(schema.to_vec()).ok()?;
    let mut from = object_header_end + 1;
    while let Some(object_end) = find(body, LEGACY_OBJECT_END, from) {
        if let Some(banner) = object_end
            .checked_add(LEGACY_OBJECT_END.len())
            .and_then(|banner| body.get(banner..))
            .and_then(|tail| tail.strip_prefix(b"\n"))
            .filter(|tail| tail.starts_with(LEGACY_BANNER_START))
        {
            let banner_end = find(banner, b"\n", 0).unwrap_or(banner.len());
            let banner_offset = data.len() - banner.len();
            return Some(LegacyAsciiFraming {
                schema,
                product_release: legacy_product_release(&banner[..banner_end]),
                banner_offset,
                object_offset: header_end + 1,
                persistence: legacy::Persistence::default(),
            });
        }
        from = object_end + 1;
    }
    None
}

/// Identify the layout family structurally ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). The
/// `DEPDB_DATA` root record is authoritative because a persistence payload can
/// contain embedded names with the `ND:` decoration. An undecorated file with
/// neither a valid root record, an outer `ND:` name, nor a complete legacy
/// ASCII object remains unknown.
fn identify_layout(
    data: &[u8],
    sections: &[ScannedSection<'_>],
    legacy_ascii: Option<LegacyAsciiFraming>,
) -> Layout {
    let has_depdb_root = sections.iter().any(|section| {
        if section.section.name() != "DEPDB_DATA" {
            return false;
        }
        let Some(header_end) = section
            .section
            .offset()
            .checked_add(section.section.raw_name.len() + 2)
        else {
            return false;
        };
        data.get(header_end..section.section.end())
            .is_some_and(|payload| payload.starts_with(DEPDB_ROOT_RECORD))
    });
    let has_depdb_section = sections
        .iter()
        .any(|section| section.section.name() == "DEPDB_DATA");
    let has_nd_decoration = sections
        .iter()
        .any(|s| s.section.raw_name.starts_with("ND:"));
    if has_depdb_section {
        if has_depdb_root {
            Layout::Depdb
        } else {
            Layout::Unknown(UnknownLayout::DepdbRootMissing)
        }
    } else if has_nd_decoration {
        Layout::Nd
    } else if let Some(framing) = legacy_ascii {
        Layout::LegacyAscii(Box::new(framing))
    } else {
        Layout::Unknown(UnknownLayout::NoDiscriminant)
    }
}

/// Sum every valid `<label>\0 [skip] f8 <count>` header in `region`.
/// After the label's NUL terminator, up to two optional non-`f8` framing bytes
/// (e.g. the `f3`/`f2` `crv_array` discriminators) are skipped before the
/// required `f8` opener, whose compact-integer count is then decoded ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array),
/// [§5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#5-topology-and-section-records)).
fn read_array_count(region: &[u8], label: &[u8]) -> Result<Option<u32>, CodecError> {
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
                        let Some(sum) = total.checked_add(count) else {
                            return Err(CodecError::malformed(format!(
                                "creo `{}` namespace array at offset {pos} declares {count} \
                                 entries, which added to the {total} already declared exceeds \
                                 the 32-bit census",
                                String::from_utf8_lossy(label),
                            )));
                        };
                        total = sum;
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
    Ok(found.then_some(total))
}

/// Read the visible-geometry namespace census from the `VisibGeom` section body.
fn geom_census(sections: &[ScannedSection<'_>]) -> Result<GeomCensus, CodecError> {
    let Some(vg) = sections
        .iter()
        .find(|s| s.section.name() == VISIBGEOM)
        .or_else(|| sections.iter().find(|s| s.section.name() == "DEPDB_DATA"))
    else {
        return Ok(GeomCensus::default());
    };
    let region = vg.region;
    Ok(GeomCensus {
        srf_array_count: read_array_count(region, b"srf_array")?,
        crv_array_count: read_array_count(region, b"crv_array")?,
    })
}

/// Decode the active unit-system selector. `51` is millimeter-Newton-Second,
/// `54` is the Creo default inch-pound-mass-second system, and `55` is
/// millimeter-Kilogram-Second.
fn binary_principal_unit(data: &[u8]) -> Option<legacy::PrincipalUnitSystem> {
    let start = find(data, PRINCIPAL_UNIT_ID, 0)? + PRINCIPAL_UNIT_ID.len();
    match *data.get(start)? {
        51 => Some(legacy::PrincipalUnitSystem::MillimeterNewtonSecond),
        54 => Some(legacy::PrincipalUnitSystem::InchPoundMassSecond),
        55 => Some(legacy::PrincipalUnitSystem::MillimeterKilogramSecond),
        value => Some(legacy::PrincipalUnitSystem::UnknownBinarySelector(value)),
    }
}

fn cmnm_model_name(data: &[u8]) -> Option<(String, usize)> {
    const PREFIX: &[u8] = &cmnm::PREFIX_VALUE;
    let marker = find(data, PREFIX, 0)?;
    let start = marker + cmnm::NAME_LENGTH_HEX;
    find(data, PREFIX, start).is_none().then_some(())?;
    let length_bytes = data.get(start..marker + cmnm::LEN)?;
    let length = usize::from_str_radix(std::str::from_utf8(length_bytes).ok()?, 16).ok()?;
    let name = data.get(marker + cmnm::LEN..marker + cmnm::LEN + length)?;
    (!name.is_empty() && !name.iter().any(|byte| matches!(byte, 0 | b'\n' | b'\r')))
        .then_some(())?;
    Some((
        std::str::from_utf8(name).ok()?.to_string(),
        marker + cmnm::LEN,
    ))
}

/// Find the root model name stored by binary sections that do not carry a
/// `CMNM` header record.
fn native_model_name(sections: &[ScannedSection<'_>]) -> Option<(String, usize)> {
    const FIELD: &[u8] = b"model_name\0";

    for section in sections {
        if section.section.role() == SectionRole::Thumbnail {
            continue;
        }
        let region = section.region;
        let mut from = 0;
        while let Some(field) = find(region, FIELD, from) {
            let value_start = field + FIELD.len();
            if region.get(value_start) == Some(&0xe1) {
                from = value_start + 1;
                continue;
            }
            let Some(value_end) = find(region, b"\0", value_start) else {
                break;
            };
            let mut name_start = value_start;
            if region.get(name_start) == Some(&0xf1) {
                name_start += 1;
            }
            let value = &region[name_start..value_end];
            if let Ok(name) = std::str::from_utf8(value) {
                if !name.is_empty() && name.chars().all(|character| !character.is_control()) {
                    return Some((name.to_owned(), section.section.offset() + name_start));
                }
            }
            from = value_end + 1;
        }
    }
    None
}

fn relation_model_name(filename: &str) -> Option<&str> {
    let filename = filename.trim_end_matches(' ');
    let name = if filename.len() >= 4
        && filename.as_bytes()[filename.len() - 4..].eq_ignore_ascii_case(b".prt")
    {
        &filename[..filename.len() - 4]
    } else if !filename.contains('.') {
        filename
    } else {
        return None;
    };
    (!name.is_empty()).then_some(name)
}

fn family_table(data: &[u8], sections: &[ScannedSection<'_>]) -> Option<FamilyTableRecord> {
    let section = sections
        .iter()
        .find(|section| section.section.name() == "FamilyInf")?;
    let end = section.section.end();
    let label = b"drv_tbl_ptr\0";
    let offset = find(data, label, section.section.offset())? + label.len();
    if offset >= end {
        return None;
    }
    let pointer = match data[offset] {
        0xe1 => FamilyTablePointer::Null,
        psb::token::ENTITY_REF => {
            let Ok((id, after)) = psb::reference_id(data, offset + 1) else {
                return None;
            };
            if after > end {
                return None;
            }
            FamilyTablePointer::Entity(id)
        }
        _ => return None,
    };
    Some(FamilyTableRecord { pointer, offset })
}

fn model_geometry_sections<'a>(sections: &[ScannedSection<'a>]) -> Vec<ScannedSection<'a>> {
    let mut visible_namespace_present = false;
    for candidate in sections
        .iter()
        .filter(|candidate| candidate.section.name() == VISIBGEOM)
    {
        let payload = candidate.region;
        if find(payload, b"srf_array\0", 0).is_some() || find(payload, b"crv_array\0", 0).is_some()
        {
            visible_namespace_present = true;
            break;
        }
    }
    let mut selected = Vec::new();
    for section in sections {
        let keep = if visible_namespace_present {
            section.section.name() == VISIBGEOM
        } else if section.section.name() == "DEPDB_DATA" {
            let payload = section.region;
            find(payload, b"srf_array\0", 0).is_some() || find(payload, b"crv_array\0", 0).is_some()
        } else {
            false
        };
        if keep {
            selected.push(section.clone());
        }
    }
    selected
}

fn surface_rows(sections: &[ScannedSection<'_>]) -> Vec<SurfaceRow> {
    let mut rows = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        rows.extend(surface::rows(section_bytes).into_iter().map(|mut row| {
            row.offset += section.section.offset();
            row
        }));
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn cross_section_surface_rows(sections: &[ScannedSection<'_>]) -> Vec<SurfaceRow> {
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "Xsections")
    {
        let payload = section.region;
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        rows.extend(
            surface::cross_section_rows(payload)
                .into_iter()
                .map(|mut row| {
                    row.offset += section.section.offset();
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn surface_prototype_count(sections: &[ScannedSection<'_>]) -> usize {
    let mut total = 0usize;
    for section in sections {
        let section_bytes = section.region;
        total += surface::prototype_count(section_bytes);
    }
    total
}

fn surface_prototype_records(
    sections: &[ScannedSection<'_>],
    refusals: &mut crate::lane_refusal::LaneRefusals,
) -> Vec<SurfacePrototypeRecord> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            surface::named_prototype_records(section_bytes, refusals)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    for parameter in &mut record.parameters {
                        parameter.offset += section.section.offset();
                        parameter.value_offset += section.section.offset();
                    }
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn surface_parameters(sections: &[ScannedSection<'_>]) -> Vec<SurfaceParameterRecord> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            surface::parameter_records(section_bytes)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.body_offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn cross_section_surface_parameters(
    sections: &[ScannedSection<'_>],
) -> Vec<SurfaceParameterRecord> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "Xsections")
    {
        let payload = section.region;
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        records.extend(
            surface::cross_section_parameter_records(payload)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.body_offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn surface_contours(sections: &[ScannedSection<'_>]) -> Vec<SurfaceContourRecord> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            surface::contour_records(section_bytes)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.envelope_offset += section.section.offset();
                    record.surface_row_offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn cross_section_surface_contours(sections: &[ScannedSection<'_>]) -> Vec<SurfaceContourRecord> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "Xsections")
    {
        let payload = section.region;
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        records.extend(
            surface::cross_section_contour_records(payload)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.envelope_offset += section.section.offset();
                    record.surface_row_offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn loop_array_scan(sections: &[ScannedSection<'_>]) -> LoopArrayScan {
    let mut frames = Vec::new();
    let mut records = Vec::new();
    for section in sections {
        let payload = section.region;
        let scan = loop_array::scan(payload);
        frames.extend(scan.frames.into_iter().map(|mut frame| {
            frame.offset += section.section.offset();
            frame.prototype_end += section.section.offset();
            frame.end += section.section.offset();
            frame
        }));
        records.extend(scan.records.into_iter().map(|mut record| {
            record.frame_offset += section.section.offset();
            record.offset += section.section.offset();
            record.body_offset += section.section.offset();
            record
        }));
    }
    frames.sort_by_key(|frame: &LoopArrayFrame| frame.offset);
    records.sort_by_key(|record: &LoopArrayRecord| record.offset);
    LoopArrayScan { frames, records }
}

fn tabulated_cylinder_curve_replays(
    sections: &[ScannedSection<'_>],
) -> Vec<TabulatedCylinderCurveReplay> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            surface::tabulated_cylinder_curve_replays(section_bytes)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.surface_row_offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn plane_local_systems(sections: &[ScannedSection<'_>]) -> Vec<PlaneLocalSystem> {
    let mut systems = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        systems.extend(surface::plane_local_systems(section_bytes).into_iter().map(
            |mut system| {
                system.row_offset += section.section.offset();
                system.offset += section.section.offset();
                system
            },
        ));
    }
    systems.sort_by_key(|system| system.offset);
    systems
}

fn cross_section_plane_local_systems(sections: &[ScannedSection<'_>]) -> Vec<PlaneLocalSystem> {
    let mut systems = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "Xsections")
    {
        let payload = section.region;
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        systems.extend(
            surface::cross_section_plane_local_systems(payload)
                .into_iter()
                .map(|mut system| {
                    system.row_offset += section.section.offset();
                    system.offset += section.section.offset();
                    system
                }),
        );
    }
    systems.sort_by_key(|system| system.offset);
    systems
}

fn plane_envelopes(sections: &[ScannedSection<'_>]) -> Vec<PlaneEnvelopeRecord> {
    let mut envelopes = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        envelopes.extend(surface::plane_envelopes(section_bytes).into_iter().map(
            |mut envelope| {
                envelope.row_offset += section.section.offset();
                envelope.offset += section.section.offset();
                envelope
            },
        ));
    }
    envelopes.sort_by_key(|envelope| envelope.offset);
    envelopes
}

fn cross_section_plane_envelopes(sections: &[ScannedSection<'_>]) -> Vec<PlaneEnvelopeRecord> {
    let mut envelopes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "Xsections")
    {
        let payload = section.region;
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        envelopes.extend(
            surface::cross_section_plane_envelopes(payload)
                .into_iter()
                .map(|mut envelope| {
                    envelope.offset += section.section.offset();
                    envelope
                }),
        );
    }
    envelopes.sort_by_key(|envelope| envelope.offset);
    envelopes
}

fn curve_prototypes(sections: &[ScannedSection<'_>]) -> Vec<CurvePrototype> {
    let mut prototypes = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        prototypes.extend(
            curve::prototypes(section_bytes)
                .into_iter()
                .map(|mut prototype| {
                    prototype.offset += section.section.offset();
                    prototype
                }),
        );
    }
    prototypes.sort_by_key(|prototype| prototype.offset);
    prototypes
}

fn curve_expressions(
    sections: &[ScannedSection<'_>],
    model_name: Option<&str>,
) -> Vec<CurveExpressionRecord> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            curve::expression_records_with_model_name(section_bytes, model_name)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.expression_offset += section.section.offset();
                    for line in &mut record.lines {
                        line.offset += section.section.offset();
                    }
                    for assignment in &mut record.assignments {
                        assignment.offset += section.section.offset();
                    }
                    for block in &mut record.solve_blocks {
                        block.offset += section.section.offset();
                        block.for_offset += section.section.offset();
                        for equation in &mut block.equations {
                            equation.offset += section.section.offset();
                        }
                        for assignment in &mut block.assignments {
                            assignment.offset += section.section.offset();
                        }
                    }
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn curve_parameters(
    sections: &[ScannedSection<'_>],
    face_ids: &BTreeSet<u32>,
) -> Vec<CurveParameterRecord> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            curve::parameter_records_with_face_ids(section_bytes, Some(face_ids))
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.body_offset += section.section.offset();
                    record.suffix_offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn two_chart_pcurves(
    sections: &[ScannedSection<'_>],
    face_ids: &BTreeSet<u32>,
) -> Vec<TwoChartPcurveSamples> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            curve::two_chart_pcurve_samples(section_bytes, Some(face_ids))
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    let mut counts = BTreeMap::new();
    for record in &records {
        *counts.entry(record.curve_id).or_insert(0usize) += 1;
    }
    records.retain(|record| counts.get(&record.curve_id) == Some(&1));
    records
}

fn prototype_pcurves(sections: &[ScannedSection<'_>]) -> Vec<PrototypePcurveEndpoints> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            curve::prototype_pcurve_endpoints(section_bytes)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn curve_prototype_topology(sections: &[ScannedSection<'_>]) -> Vec<CurvePrototypeTopology> {
    let mut records = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        records.extend(
            curve::prototype_topology(section_bytes)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn curve_topology_rows(
    sections: &[ScannedSection<'_>],
    face_ids: &BTreeSet<u32>,
) -> Vec<CurveTopologyRow> {
    let mut rows = Vec::new();
    for section in sections {
        let section_bytes = section.region;
        rows.extend(
            curve::topology_rows_with_face_ids(section_bytes, Some(face_ids))
                .into_iter()
                .map(|mut row| {
                    row.offset += section.section.offset();
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn cross_section_curve_rows(sections: &[ScannedSection<'_>]) -> Vec<DepdbCurveRow> {
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "Xsections")
    {
        let payload = section.region;
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        rows.extend(
            curve::depdb_cross_section_rows(payload)
                .into_iter()
                .map(|mut row| {
                    row.offset += section.section.offset();
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn cross_section_curve_prototypes(sections: &[ScannedSection<'_>]) -> Vec<CurvePrototype> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "Xsections")
    {
        let payload = section.region;
        if find(payload, b"Sld_Xsections\0", 0).is_none() {
            continue;
        }
        records.extend(curve::prototypes(payload).into_iter().map(|mut record| {
            record.offset += section.section.offset();
            record
        }));
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn datum_planes(sections: &[ScannedSection<'_>]) -> Vec<DatumPlaneRecord> {
    let mut planes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "ActDatums")
    {
        let section_bytes = section.region;
        planes.extend(datum::planes(section_bytes).into_iter().map(|mut plane| {
            plane.offset_in_payload += section.section.offset();
            plane
        }));
        if let Some(mut plane) = datum::named_plane(section_bytes) {
            plane.offset_in_payload += section.section.offset();
            planes.push(plane);
        }
    }
    planes.sort_by_key(|plane| plane.offset_in_payload);
    planes
}

fn datum_cylinders(sections: &[ScannedSection<'_>]) -> Vec<DatumCylinder> {
    let mut cylinders = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "ActDatums")
    {
        let section_bytes = section.region;
        cylinders.extend(
            datum::cylinders(section_bytes)
                .into_iter()
                .map(|mut cylinder| {
                    cylinder.offset_in_payload += section.section.offset();
                    cylinder
                }),
        );
    }
    cylinders.sort_by_key(|cylinder| cylinder.offset_in_payload);
    cylinders
}

fn structural_feature_ids(
    sections: &[ScannedSection<'_>],
    surface_rows: &[SurfaceRow],
    curve_rows: &[CurveTopologyRow],
) -> std::collections::BTreeSet<u32> {
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(
        surface_rows
            .iter()
            .map(|row| row.feature_id)
            .chain(curve_rows.iter().map(|row| row.feature_id))
            .filter(|id| *id != 0),
    );
    for section in sections
        .iter()
        .filter(|section| section.section.role() == SectionRole::PsbGeometry)
    {
        let payload = section.region;
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
    ids
}

fn stored_operation_schema_class(
    operation: &FeatureOperation,
) -> Option<crate::feature::schema::SchemaClass> {
    use crate::feature::schema::SchemaClass;
    operation
        .root_schema_class()
        .or_else(|| match operation.kind.as_str() {
            "Hole" => Some(SchemaClass::Hole),
            "Round" | "Rundung" => Some(SchemaClass::Round),
            "Chamfer" => Some(SchemaClass::Chamfer),
            "Cut" => Some(SchemaClass::Cut),
            "Protrusion" => Some(SchemaClass::Protrusion),
            "Datum Plane" | "Bezugsebene" => Some(SchemaClass::DatumPlane),
            "Section" => Some(SchemaClass::Section),
            "Draft" | "Schräge" => Some(SchemaClass::Draft),
            "Surface Merge" => Some(SchemaClass::SurfaceMerge),
            _ => operation
                .recipe
                .resolved()
                .map(|recipe| match recipe.effect() {
                    feature::operations::FeatureRecipeEffect::Cut => SchemaClass::Cut,
                    feature::operations::FeatureRecipeEffect::Protrude => SchemaClass::Protrusion,
                }),
        })
}

fn registered_feature_schema_class(schema_class: crate::feature::schema::SchemaClass) -> bool {
    use crate::feature::schema::SchemaClass;
    matches!(
        schema_class,
        SchemaClass::Hole
            | SchemaClass::Round
            | SchemaClass::Chamfer
            | SchemaClass::Cut
            | SchemaClass::Protrusion
            | SchemaClass::DatumPlane
            | SchemaClass::Section
            | SchemaClass::Draft
            | SchemaClass::SurfaceMerge
            | SchemaClass::CoordinateSystem
    )
}

fn feature_row_has_model_identity(
    row: &FeatureRow,
    structural_ids: &std::collections::BTreeSet<u32>,
    operations: &[FeatureOperation],
    reference_names: &[FeatureReferenceName],
) -> bool {
    use crate::feature::schema::SchemaClass;
    structural_ids.contains(&row.feature_id)
        || operations.iter().any(|operation| {
            operation.feature_id == row.feature_id
                && (stored_operation_schema_class(operation) == row.root_schema_class
                    || (stored_operation_schema_class(operation).is_some()
                        && row.root_schema_class.is_some_and(|schema_class| {
                            !registered_feature_schema_class(schema_class)
                        })))
        })
        || reference_names.iter().any(|reference| {
            let name = reference.name();
            let numbered_family = |family: &str| {
                [" id ", " ID "].into_iter().any(|separator| {
                    name.strip_prefix(family)
                        .and_then(|suffix| suffix.strip_prefix(separator))
                        .and_then(|ordinal| ordinal.parse::<u32>().ok())
                        == Some(reference.feature_id)
                })
            };
            let named_datum = matches!(name.as_ref(), "Datum Plane" | "Bezugsebene")
                || numbered_family("Datum Plane")
                || numbered_family("Bezugsebene")
                || name.strip_prefix("DTM").is_some_and(|ordinal| {
                    !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
                });
            reference.feature_id == row.feature_id
                && (row.root_schema_class == Some(SchemaClass::Section)
                    || (row.root_schema_class == Some(SchemaClass::DatumPlane) && named_datum)
                    || (row.root_schema_class == Some(SchemaClass::CoordinateSystem)
                        && name == "PRT_CSYS_DEF"))
        })
}

fn feature_entity_tables(
    sections: &[ScannedSection<'_>],
    feature_ids: &[u32],
    rows: &[SurfaceRow],
) -> Vec<FeatureEntityTable> {
    let feature_ids = feature_ids.iter().copied().collect();
    let surface_ids = rows.iter().map(|row| row.id).collect();
    let mut tables = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "AllFeatur")
    {
        let section_bytes = section.region;
        tables.extend(
            feature::entity::entity_tables(section_bytes, &feature_ids, &surface_ids)
                .into_iter()
                .map(|mut table| {
                    table.offset += section.section.offset();
                    for entry in &mut table.entries {
                        entry.offset += section.section.offset();
                        entry.end_offset += section.section.offset();
                    }
                    table
                }),
        );
    }
    tables.sort_by_key(|table| table.offset);
    tables
}

fn feature_rows(sections: &[ScannedSection<'_>], feature_ids: &[u32]) -> Vec<FeatureRow> {
    let feature_ids = feature_ids.iter().copied().collect();
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "AllFeatur")
    {
        let section_bytes = section.region;
        rows.extend(feature::rows::rows(
            section_bytes,
            &feature_ids,
            section.section.offset(),
        ));
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn feature_entity_graph(
    sections: &[ScannedSection<'_>],
) -> (Vec<FeatureEntity>, Vec<FeatureEntityReference>) {
    let Some(section) = sections
        .iter()
        .find(|section| section.section.name() == "AllFeatur")
    else {
        return (Vec::new(), Vec::new());
    };
    let section_bytes = section.region;
    // The payload follows the `#<name>\n` section header. A section without
    // that newline carries no header, so the whole region is the payload.
    let header_length = find(section_bytes, b"\n", 0).map_or(0, |newline| newline + 1);
    let payload_start = section.section.offset() + header_length;
    let (mut entities, mut references) =
        feature::entity::entity_graph(&section_bytes[header_length..]);
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
        segments.rows.add_offset(section_offset);
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
            if let Some(references) = &mut row.references {
                references.offset += section_offset;
                for reference in &mut references.rows {
                    reference.offset += section_offset;
                }
            }
        }
    }
    if let Some(relations) = &mut definition.relations {
        relations.offset += section_offset;
        for row in &mut relations.rows {
            row.offset += section_offset;
        }
        if let Some(table) = &mut relations.skamps {
            table.shift_offsets(section_offset);
        }
        if let Some(table) = &mut relations.triples {
            table.shift_offsets(section_offset);
        }
    }
    if let Some(saved) = &mut definition.saved_section {
        saved.offset += section_offset;
        for entity in &mut saved.entities {
            match entity {
                feature::definitions::FeatureSavedEntity::Line(line) => {
                    line.offset += section_offset
                }
                feature::definitions::FeatureSavedEntity::Arc(arc) => arc.offset += section_offset,
                feature::definitions::FeatureSavedEntity::Circle(circle) => {
                    circle.offset += section_offset
                }
                feature::definitions::FeatureSavedEntity::Conic(conic) => {
                    conic.offset += section_offset
                }
                feature::definitions::FeatureSavedEntity::Spline(spline) => {
                    spline.offset += section_offset
                }
                feature::definitions::FeatureSavedEntity::Dummy(dummy) => {
                    dummy.offset += section_offset
                }
            }
        }
    }
}

fn feature_definitions(sections: &[ScannedSection<'_>]) -> Vec<FeatureDefinition> {
    let mut definitions = Vec::new();
    for section in sections.iter().filter(|section| {
        section.section.name() == "FeatDefs" || section.section.name() == "DEPDB_DATA"
    }) {
        let payload = section.region;
        definitions.extend(
            (if section.section.name() == "DEPDB_DATA" {
                feature::definitions::depdb_definitions(payload)
            } else {
                feature::definitions::definitions(payload)
            })
            .into_iter()
            .map(|mut definition| {
                offset_feature_definition(&mut definition, section.section.offset());
                definition
            }),
        );
        if section.section.name() == "DEPDB_DATA" {
            let recipe_operations = feature::operations::operations(payload)
                .into_iter()
                .filter(|operation| operation.recipe.resolved().is_some())
                .collect::<Vec<_>>();
            if let [operation] = recipe_operations.as_slice() {
                if let Some(mut definition) = feature::definitions::depdb_section_definition(
                    payload,
                    Some(operation.feature_id),
                ) {
                    offset_feature_definition(&mut definition, section.section.offset());
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
            let mut definition = feature::definitions::depdb_section_definition(&row.body, None)?;
            offset_feature_definition(&mut definition, row.body_offset);
            Some(definition)
        })
        .collect::<Vec<_>>();
    definitions.sort_by_key(|definition| definition.offset);
    definitions
}

fn section_owner_ranges(
    sections: &[ScannedSection<'_>],
    feature_rows: &[FeatureRow],
) -> Vec<(usize, usize)> {
    let mut ranges = sections
        .iter()
        .filter(|section| section.section.name() == "DEPDB_DATA")
        .map(|section| (section.section.offset(), section.section.end()))
        .collect::<Vec<_>>();
    ranges.extend(feature_rows.iter().map(|row| {
        (
            row.body_offset,
            row.body_offset.saturating_add(row.body.len()),
        )
    }));
    ranges
}

fn positional_replay_definitions(sections: &[ScannedSection<'_>]) -> Vec<FeatureDefinition> {
    let mut definitions = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "FeatDefs")
    {
        let section_bytes = section.region;
        definitions.extend(
            feature::definitions::positional_replay_definitions(section_bytes)
                .into_iter()
                .map(|mut definition| {
                    offset_feature_definition(&mut definition, section.section.offset());
                    definition
                }),
        );
    }
    definitions.sort_by_key(|definition| definition.offset);
    definitions
}

fn feature_operations(sections: &[ScannedSection<'_>]) -> Vec<FeatureOperation> {
    let mut records = Vec::new();
    for section in sections.iter().filter(|section| {
        section.section.name() == "MdlStatus" || section.section.name() == "DEPDB_DATA"
    }) {
        let section_bytes = section.region;
        records.extend(
            feature::operations::operations(section_bytes)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.state_offset += section.section.offset();
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

fn feature_reference_names(sections: &[ScannedSection<'_>]) -> Vec<FeatureReferenceName> {
    let mut records = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "MdlRefInfo")
    {
        let section_bytes = section.region;
        records.extend(
            feature::operations::reference_names(section_bytes)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record
                }),
        );
    }
    records
}

fn feature_operation_states(sections: &[ScannedSection<'_>]) -> Vec<FeatureOperationState> {
    let mut records = Vec::new();
    for section in sections.iter().filter(|section| {
        section.section.name() == "MdlStatus" || section.section.name() == "DEPDB_DATA"
    }) {
        let section_bytes = section.region;
        records.extend(
            feature::operations::operation_states(section_bytes)
                .into_iter()
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record.state_offset += section.section.offset();
                    record
                }),
        );
    }
    records.sort_by_key(|record| record.offset);
    records
}

fn depdb_recipe_rows(sections: &[ScannedSection<'_>]) -> Vec<FeatureRow> {
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
        .filter(|section| section.section.name() == "DEPDB_DATA")
    {
        let payload = section.region;
        let mut recipe_operations = feature::operations::operation_states(payload)
            .into_iter()
            .filter_map(|operation| {
                operation
                    .recipe
                    .candidate()
                    .map(|recipe| (operation, recipe))
            })
            .collect::<Vec<_>>();
        recipe_operations.sort_by_key(|(operation, _)| operation.offset);
        let mut body_start = 0;
        for (operation, recipe) in &recipe_operations {
            let Some(body_end) = recipe_end(payload, operation.offset, *recipe) else {
                continue;
            };
            let Some(body) = payload
                .get(body_start..body_end)
                .and_then(|bytes| bytes.to_vec().try_into().ok())
            else {
                continue;
            };
            rows.push(FeatureRow {
                feature_id: operation.feature_id,
                root_schema_class: operation.root_schema_class(),
                stream_offset: section.section.offset(),
                body,
                body_offset: section.section.offset() + body_start,
                offset: section.section.offset() + operation.offset,
            });
            body_start = body_end;
        }
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn geomlists_value(sections: &[ScannedSection<'_>], label: &[u8]) -> Option<u32> {
    let section = sections
        .iter()
        .find(|section| section.section.name() == "Geomlists")?;
    let payload = section.region;
    let value_offset = find(payload, label, 0)? + label.len();
    let (count, after) = psb::compact_int(payload, value_offset);
    (after > value_offset).then_some(count)
}

/// Read one scalar integer owned by the unique legacy `Sld_GeomDepend` root.
///
/// Legacy ASCII stores this metadata in the persistence object tree rather
/// than in a binary `Geomlists` section. Distinct complete values remain
/// unresolved; equal duplicate records are one value witness.
fn legacy_geom_depend_value(persistence: &legacy::Persistence, field_name: &str) -> Option<u32> {
    let mut values = persistence
        .integer_values
        .rows
        .iter()
        .filter(|record| record.name == field_name)
        .filter_map(|record| {
            let parent_id = record.parent?;
            let parent = persistence
                .objects
                .iter()
                .find(|object| object.offset == parent_id)?;
            (parent.name == "Sld_GeomDepend").then_some(())?;
            let legacy::NumericPayload::Scalar { value } = &record.payload else {
                return None;
            };
            u32::try_from(*value).ok()
        });
    let value = values.next()?;
    values.all(|other| other == value).then_some(value)
}

/// Parse a whole `.prt` byte image.
pub(crate) fn scan_bytes<'a>(
    data: impl Into<Cow<'a, [u8]>>,
) -> Result<ContainerScan<'a>, CodecError> {
    let data = data.into();
    let version_line = line_at(&data, 0);
    let mut model_name = cmnm_model_name(&data).map(|(name, offset)| ModelName { name, offset });

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

    let mut legacy_ascii = legacy_ascii_framing(&data);
    let sections = legacy_ascii.as_ref().map_or_else(
        || toc_sections(&data, header_end.unwrap_or(0)),
        |legacy| legacy_toc_sections(&data, legacy.banner_offset),
    );
    let sections = if sections.is_empty() {
        scan_sections(&data, body_start)?
    } else {
        sections
    };
    if let Some(framing) = &mut legacy_ascii {
        let initial_end = sections
            .first()
            .map_or(data.len(), |section| section.section.offset());
        let mut scopes = Vec::with_capacity(sections.len() + 1);
        scopes.push(framing.object_offset..initial_end);
        for section in &sections {
            let region = section.region;
            let Some(payload_start) = section
                .section
                .offset()
                .checked_add(section.section.raw_name.len())
                .and_then(|start| start.checked_add(2))
            else {
                continue;
            };
            if legacy::starts_with_declaration(&data, payload_start) {
                scopes.push(section.section.offset()..section.section.offset() + region.len());
            }
        }
        framing.persistence = legacy::scan(&data, scopes)?;
    }
    if model_name.is_none() {
        if let Some((name, offset)) = legacy_ascii
            .as_ref()
            .and_then(|framing| framing.persistence.model_name())
        {
            model_name = Some(ModelName { name, offset });
        }
    }
    if model_name.is_none() {
        if let Some((name, offset)) = legacy_ascii
            .as_ref()
            .and_then(|framing| framing.persistence.first_source_model_name())
        {
            model_name = Some(ModelName { name, offset });
        }
    }
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
                    entries: table.entries,
                })
        })
        .collect();
    let primitive_scalar_arrays = expanded_sections
        .iter()
        .filter(|section| section.name == "SolidPrimdata")
        .flat_map(|section| primdata::scalar_arrays(&section.data))
        .collect();
    let (primitive_triangle_strips, conflicting_triangle_strip_representation_count) =
        expanded_sections
            .iter()
            .filter(|section| section.name == "SolidPrimdata")
            .map(|section| primdata::triangle_strips(&section.data))
            .fold((Vec::new(), 0usize), |(mut strips, conflicts), scan| {
                strips.extend(scan.strips);
                (
                    strips,
                    conflicts.saturating_add(scan.conflicting_representation_count),
                )
            });
    let mut reference_lines = Vec::new();
    let mut reference_circles = Vec::new();
    let mut reference_conics: Vec<ReferenceConic> = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "MdlRefInfo")
    {
        let payload = section.region;
        reference_lines.extend(
            reference::lines(payload)
                .into_iter()
                .chain(reference::line3d_lines(payload))
                .map(|mut line| {
                    line.offset += section.section.offset();
                    line
                }),
        );
        reference_circles.extend(reference::arc_z_circles(payload).into_iter().map(
            |mut circle| {
                circle.offset += section.section.offset();
                circle
            },
        ));
        reference_conics.extend(
            reference::named_conics(payload)
                .into_iter()
                .chain(reference::positional_conics(payload))
                .map(|mut conic| {
                    conic.offset += section.section.offset();
                    conic
                }),
        );
    }
    let reference_ellipses = reference::ellipse_carriers(&reference_conics);
    let layout = identify_layout(&data, &sections, legacy_ascii);
    if model_name.is_none() && !matches!(layout, Layout::LegacyAscii(_)) {
        if let Some((name, offset)) = native_model_name(&sections) {
            model_name = Some(ModelName { name, offset });
        }
    }
    let legacy_ascii = layout.legacy_ascii();
    let legacy_geometry = legacy_ascii
        .map(|framing| crate::legacy_geometry::scan(&framing.persistence))
        .unwrap_or_default();
    let legacy_rounds = legacy_ascii
        .map(|framing| {
            crate::legacy_feature::scan(&framing.persistence, &legacy_geometry.topology_rows)
        })
        .unwrap_or_default();
    let model_geometry_sections = model_geometry_sections(&sections);
    let census = geom_census(&sections)?;
    let principal_unit =
        binary_principal_unit(&data).or_else(|| legacy_ascii?.persistence.principal_unit_system());
    let family_table = family_table(&data, &sections);
    let legacy_family_table =
        legacy_ascii.and_then(|framing| crate::legacy_family::parse(&framing.persistence));
    let nonvisible_geometry_sections = sections
        .iter()
        .filter(|section| section.section.name() == "NovisGeom")
        .cloned()
        .collect::<Vec<_>>();
    let mut loop_array_sections = model_geometry_sections.clone();
    loop_array_sections.extend(nonvisible_geometry_sections.iter().cloned());
    for section in sections
        .iter()
        .filter(|section| section.section.name() == "Xsections")
    {
        let payload = section.region;
        if find(payload, b"Sld_Xsections\0", 0).is_some() {
            loop_array_sections.push(section.clone());
        }
    }
    loop_array_sections.sort_by_key(|section| section.section.offset());
    loop_array_sections.dedup_by_key(|section| section.section.offset());
    let loop_arrays = loop_array_scan(&loop_array_sections);
    let mut nonvisible_surface_rows = surface_rows(&nonvisible_geometry_sections);
    nonvisible_surface_rows.extend(legacy_geometry.nonvisible_rows);
    nonvisible_surface_rows.sort_by_key(|row| row.offset);
    let mut surface_rows = surface_rows(&model_geometry_sections);
    surface_rows.extend(legacy_geometry.rows);
    surface_rows.sort_by_key(|row| row.offset);
    let cross_section_surface_rows = cross_section_surface_rows(&sections);
    let nonvisible_surface_parameters = surface_parameters(&nonvisible_geometry_sections);
    let surface_parameters = surface_parameters(&model_geometry_sections);
    let cross_section_surface_parameters = cross_section_surface_parameters(&sections);
    let nonvisible_surface_contours = surface_contours(&nonvisible_geometry_sections);
    let surface_contours = surface_contours(&model_geometry_sections);
    let cross_section_surface_contours = cross_section_surface_contours(&sections);
    let tabulated_cylinder_curve_replays =
        tabulated_cylinder_curve_replays(&model_geometry_sections);
    let plane_local_systems = plane_local_systems(&model_geometry_sections);
    let cross_section_plane_local_systems = cross_section_plane_local_systems(&sections);
    let plane_envelopes = plane_envelopes(&model_geometry_sections);
    let cross_section_plane_envelopes = cross_section_plane_envelopes(&sections);
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
    let surface_prototype_count = surface_prototype_count(&model_geometry_sections);
    let mut nonvisible_prototype_refusals = crate::lane_refusal::LaneRefusals::new();
    let nonvisible_surface_prototype_records = surface_prototype_records(
        &nonvisible_geometry_sections,
        &mut nonvisible_prototype_refusals,
    );
    let mut prototype_refusals = crate::lane_refusal::LaneRefusals::new();
    let surface_prototype_records =
        surface_prototype_records(&model_geometry_sections, &mut prototype_refusals);
    let nonvisible_curve_prototypes = curve_prototypes(&nonvisible_geometry_sections);
    let curve_prototypes = curve_prototypes(&model_geometry_sections);
    let cross_section_curve_prototypes = cross_section_curve_prototypes(&sections);
    let mut curve_expressions = curve_expressions(
        &sections,
        model_name
            .as_ref()
            .and_then(|model| relation_model_name(&model.name)),
    );
    let topology_face_ids = nonvisible_surface_rows
        .iter()
        .chain(surface_rows.iter())
        .map(|row| row.id)
        .collect::<BTreeSet<_>>();
    let nonvisible_curve_parameters =
        curve_parameters(&nonvisible_geometry_sections, &topology_face_ids);
    let curve_parameters = curve_parameters(&model_geometry_sections, &topology_face_ids);
    let nonvisible_curve_topology_rows =
        curve_topology_rows(&nonvisible_geometry_sections, &topology_face_ids);
    let mut curve_topology_rows = curve_topology_rows(&model_geometry_sections, &topology_face_ids);
    let curve_prototype_topology = curve_prototype_topology(&model_geometry_sections);
    let prototype_topology_rows = curve::prototype_topology_rows(
        &curve_prototypes,
        &curve_prototype_topology,
        &curve_topology_rows,
        &topology_face_ids,
    );
    curve_topology_rows.extend(prototype_topology_rows);
    curve_topology_rows.sort_by_key(|row| row.offset);
    curve_topology_rows.dedup_by_key(|row| row.offset);
    let cross_section_curve_rows = cross_section_curve_rows(&sections);
    let mut pcurves = curve::pcurve_endpoints(&curve_parameters, &curve_topology_rows);
    let two_chart_pcurves = two_chart_pcurves(&model_geometry_sections, &topology_face_ids);
    if matches!(layout, Layout::LegacyAscii(_)) {
        curve_topology_rows.extend(legacy_geometry.topology_rows.iter().cloned());
        pcurves.extend(legacy_geometry.pcurves.iter().cloned());
        curve_topology_rows.sort_by_key(|row| row.offset);
        curve_topology_rows.dedup_by_key(|row| row.offset);
        pcurves.sort_by_key(|pcurve| pcurve.offset);
        pcurves.dedup_by_key(|pcurve| pcurve.offset);
    }
    let fc_curve_coordinates = curve::fc_coordinates(&curve_parameters);
    let fc05_circles = curve::fc05_circles(&curve_parameters);
    let fc05_cylinder_cap_pairs =
        curve::fc05_cylinder_cap_pairs(&fc05_circles, &curve_topology_rows, &surface_rows);
    let prototype_pcurves = prototype_pcurves(&model_geometry_sections);
    let bound_prototype_pcurves =
        curve::bind_prototype_pcurves(&prototype_pcurves, &curve_prototype_topology);
    let (half_edges, loops) = topology::build(&curve_topology_rows);
    let vertex_orbits = topology::vertex_orbits(&half_edges);
    let face_components = topology::face_components(&curve_topology_rows);
    let datum_planes = datum_planes(&sections);
    let datum_cylinders = datum_cylinders(&sections);
    let feature_operation_states = feature_operation_states(&sections);
    let feature_operations = feature_operations(&sections);
    let feature_reference_names = feature_reference_names(&sections);
    let structural_feature_ids =
        structural_feature_ids(&sections, &surface_rows, &curve_topology_rows);
    let mut candidate_feature_ids = structural_feature_ids.clone();
    candidate_feature_ids.extend(
        feature_operations
            .iter()
            .map(|operation| operation.feature_id),
    );
    candidate_feature_ids.extend(
        feature_reference_names
            .iter()
            .map(|reference| reference.feature_id),
    );
    let mut feature_rows = feature_rows(
        &sections,
        &candidate_feature_ids.iter().copied().collect::<Vec<_>>(),
    );
    feature_rows.retain(|row| {
        feature_row_has_model_identity(
            row,
            &structural_feature_ids,
            &feature_operations,
            &feature_reference_names,
        )
    });
    let mut feature_ids = structural_feature_ids;
    feature_ids.extend(feature_rows.iter().map(|row| row.feature_id));
    let feature_ids = feature_ids.into_iter().collect::<Vec<_>>();
    let feature_round_replay_scalars = feature::rows::round_replay_scalars(&feature_rows);
    let feature_choices = feature::rows::choices(&feature_rows);
    let feature_choice_fields = feature::rows::choice_fields(&feature_choices);
    let depdb_recipe_rows = depdb_recipe_rows(&sections);
    let mut feature_geometry_tables = feature::rows::geometry_tables(&feature_rows);
    feature_geometry_tables.extend(feature::rows::geometry_tables(&depdb_recipe_rows));
    feature_geometry_tables.sort_by_key(|table| table.offset);
    let feature_loop_history_entries =
        feature::rows::loop_history_entries(&feature_rows, &feature_geometry_tables);
    let mut feature_affected_ids = feature::rows::affected_ids(&feature_rows);
    feature_affected_ids.extend(feature::rows::affected_ids(&depdb_recipe_rows));
    feature_affected_ids.sort_by_key(|record| record.offset);
    let feature_replay_affected_ids = feature::rows::replay_affected_ids(&feature_rows);
    let surface_merge_replay_affected_ids =
        feature::rows::surface_merge_replay_affected_ids(&feature_rows, &feature_affected_ids);
    let feature_loop_restore_directions = feature::rows::loop_restore_directions(&feature_rows);
    let feature_entity_tables = feature_entity_tables(&sections, &feature_ids, &surface_rows);
    let feature_definitions = feature_definitions(&sections);
    let feature_definitions =
        feature::definitions::bind_definition_owners(feature_definitions, &feature_geometry_tables);
    let mut feature_definitions = feature::definitions::bind_trimmed_definition_owners(
        feature_definitions,
        &feature_entity_tables,
    );
    feature_definitions.extend(feature_row_definitions(&feature_rows));
    feature_definitions.sort_by_key(|definition| definition.offset);
    let claimed_definition_owners = feature_definitions
        .iter()
        .filter_map(|definition| definition.identity.owner_feature_id())
        .collect();
    let replay_definitions = feature::definitions::bind_replay_definition_owners(
        positional_replay_definitions(&sections),
        &feature_entity_tables,
        &claimed_definition_owners,
    );
    feature_definitions.extend(replay_definitions);
    feature_definitions.sort_by_key(|definition| definition.offset);
    let section_owner_ranges = section_owner_ranges(&sections, &feature_rows);
    let feature_definitions = feature::definitions::bind_section_owners(
        feature_definitions,
        &feature_operations,
        &section_owner_ranges,
    );
    let mut relation_dimension_symbols = ExternalRelationSymbols::default();
    for dimension in feature_definitions
        .iter()
        .filter_map(|definition| definition.dimensions.as_ref())
        .flat_map(|table| table.rows.iter())
    {
        let value = dimension
            .value
            .resolved()
            .map(|value| match dimension.unit() {
                feature::definitions::DimensionUnit::Radians => {
                    CurveExpressionValue::Angle(value.to_degrees())
                }
                feature::definitions::DimensionUnit::Millimeters => {
                    CurveExpressionValue::Length(value)
                }
                feature::definitions::DimensionUnit::SchemaDefined => {
                    CurveExpressionValue::Number(value)
                }
            });
        relation_dimension_symbols.observe(&format!("d{}", dimension.external_id), value);
    }
    curve::reevaluate_expression_records(
        &mut curve_expressions,
        model_name
            .as_ref()
            .and_then(|model| relation_model_name(&model.name)),
        &relation_dimension_symbols,
    );
    let mut feature_revolution_extents = feature::rows::revolution_extents(&feature_rows);
    feature_revolution_extents.extend(feature::definitions::definition_revolution_extents(
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
    let (feature_entities, feature_entity_references) = feature_entity_graph(&sections);
    let declared_body_count = geomlists_value(&sections, b"n_bodies\0");
    let first_quilt_ptr = geomlists_value(&sections, b"first_quilt_ptr\0").or_else(|| {
        legacy_ascii
            .and_then(|framing| legacy_geom_depend_value(&framing.persistence, "first_quilt_ptr"))
    });

    // The `Cow` takes the bytes here, so the scan's borrowed regions end and
    // the framing keeps the owned sections.
    let sections = sections
        .into_iter()
        .map(|section| section.section)
        .collect::<Vec<_>>();

    Ok(ContainerScan {
        framing: FramingScan {
            data,
            version_line,
            model_name,
            sections,
            expanded_sections,
            layout,
            census,
            principal_unit,
            family_table,
            legacy_family_table,
            declared_body_count,
            first_quilt_ptr,
        },
        primitives: PrimitiveScan {
            double_xar_tables,
            scalar_arrays: primitive_scalar_arrays,
            triangle_strips: primitive_triangle_strips,
            conflicting_triangle_strip_representation_count,
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
            contours: surface_contours,
            nonvisible_contours: nonvisible_surface_contours,
            cross_section_contours: cross_section_surface_contours,
            prototype_count: surface_prototype_count,
            prototype_records: surface_prototype_records,
            nonvisible_prototype_records: nonvisible_surface_prototype_records,
            prototype_field_refusals: prototype_refusals.take_records(),
            nonvisible_prototype_field_refusals: nonvisible_prototype_refusals.take_records(),
            legacy_carriers: legacy_geometry.carriers,
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
            datum_cylinders,
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
            two_chart_pcurves,
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
            vertices: vertex_orbits.vertices,
            half_edge_vertex_incidence: vertex_orbits.incidence,
            unstatable_vertex_orbits: vertex_orbits.unstatable_orbits,
        },
        loop_arrays,
        features: FeatureScan {
            ids: feature_ids,
            rows: feature_rows,
            round_replay_scalars: feature_round_replay_scalars,
            depdb_recipe_rows,
            choices: feature_choices,
            choice_fields: feature_choice_fields,
            geometry_tables: feature_geometry_tables,
            loop_history_entries: feature_loop_history_entries,
            affected_ids: feature_affected_ids,
            replay_affected_ids: feature_replay_affected_ids,
            surface_merge_replay_affected_ids,
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
            legacy_rounds: legacy_rounds.rounds,
        },
    })
}

/// The container scan of an in-tree fixture that states its own extents.
///
/// Only tests use it. A refusal is a defect in the fixture, so it fails the
/// test rather than returning a shortened scan.
#[cfg(test)]
pub(crate) fn scan_bytes_ok<'a>(data: impl Into<Cow<'a, [u8]>>) -> ContainerScan<'a> {
    match scan_bytes(data) {
        Ok(scan) => scan,
        Err(refusal) => panic!("the fixture states a section past its own end: {refusal}"),
    }
}

/// Return whether a thumbnail section contains a JPEG start marker.
pub(crate) fn has_thumbnail(scan: &ContainerScan) -> bool {
    for section in scan
        .framing
        .sections
        .iter()
        .filter(|s| s.role() == SectionRole::Thumbnail)
    {
        let Some(raw) = section_region(&scan.framing.data, section) else {
            continue;
        };
        // The `#<name>\n` header precedes the payload.
        let payload_start = section.raw_name.len() + 2;
        let raw_is_compressed = raw
            .get(payload_start..)
            .is_some_and(|payload| payload.starts_with(UNIX_COMPRESS_MAGIC));
        let expanded_carries_jpeg = expanded_section_for(scan, section)
            .is_some_and(|expanded| find(&expanded.data, JPEG_MAGIC, 0).is_some());
        let carries_jpeg = if raw_is_compressed {
            expanded_carries_jpeg
        } else {
            find(raw, JPEG_MAGIC, 0).is_some() || expanded_carries_jpeg
        };
        if carries_jpeg {
            return true;
        }
    }
    false
}

/// Build a codec-neutral summary of the sections, layout, and namespace census.
pub(crate) fn summarize(
    scan: &ContainerScan,
    classification: &crate::dialect::DialectClassification,
) -> ContainerSummary {
    let entries = scan
        .framing
        .sections
        .iter()
        .map(|s| {
            let mut attributes = BTreeMap::new();
            attributes.insert("offset".to_string(), s.offset().to_string());
            if s.raw_name != s.name() {
                attributes.insert("raw_name".to_string(), s.raw_name.clone());
            }
            let expanded = expanded_section_for(scan, s);
            if let Some(expanded) = expanded {
                attributes.insert(
                    "expanded_payload_size".to_string(),
                    expanded.data.len().to_string(),
                );
            }
            ContainerEntry {
                name: s.name().to_string(),
                role: s.role().into(),
                storage: expanded.map_or_else(
                    || EntryStorage::verbatim(VerbatimLabel::None, s.length() as u64),
                    |expanded| EntryStorage::Compressed {
                        method: CompressionMethod::UnixCompress,
                        stored: Some(s.length() as u64),
                        expanded: Some((expanded.data.len() + s.raw_name.len() + 2) as u64),
                    },
                ),
                attributes,
            }
        })
        .collect();

    let notes = notes(scan);

    ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(classification.matched().clone()),
        cadmpeg_ir::ContainerKind::Psb,
        entries,
        classification.loss().into_iter().collect(),
        notes,
    )
}

/// Build the diagnostic notes shared by inspection and decode reports.
pub(crate) fn notes(scan: &ContainerScan) -> Vec<String> {
    let mut notes = vec![
        format!("PSB container: {}", scan.framing.version_line),
        format!(
            "layout: {}; {} section(s) enumerated",
            scan.framing.layout.token(),
            scan.framing.sections.len()
        ),
    ];
    if let Some(name) = &scan.framing.model_name {
        notes.push(format!("native model name: {}", name.name));
    }
    if let Some(legacy) = scan.framing.layout.legacy_ascii() {
        let release = legacy.product_release.as_deref().unwrap_or("unspecified");
        let continuation_count = legacy.persistence.continuation_count();
        notes.push(format!(
            "legacy ASCII persistence: schema {}; product release {release}; {} attribute \
             declarations, {} resolved values, {continuation_count} continuation rows in {} scopes",
            legacy.schema,
            legacy.persistence.declaration_count(),
            legacy.persistence.value_count(),
            legacy.persistence.scopes.len(),
        ));
        if legacy.persistence.unresolved_value_count() != 0
            || legacy.persistence.conflicting_declaration_count() != 0
        {
            notes.push(format!(
                "legacy ASCII structural gaps: {} unresolved values, {} conflicting declarations",
                legacy.persistence.unresolved_value_count(),
                legacy.persistence.conflicting_declaration_count(),
            ));
        }
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

    notes
}

#[cfg(test)]
mod feature_row_definition_tests {
    use super::{
        feature_row_definitions, feature_row_has_model_identity, section_owner_ranges,
        structural_feature_ids, toc_sections,
    };
    use crate::curve::CurveTopologyRow;
    use crate::feature;
    use crate::feature::operations::{FeatureOperation, FeatureRecipe, FeatureReferenceName};
    use crate::feature::rows::FeatureRow;
    use crate::surface::SurfaceRow;

    #[test]
    fn surface_and_curve_generators_are_structural_feature_identities() {
        let surface = SurfaceRow {
            id: 12,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 40,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        };
        let curve = CurveTopologyRow {
            id: 45,
            type_byte: 8,
            feature_id: 41,
            directions: [1, 0xf6],
            faces: [std::num::NonZeroU32::new(12), std::num::NonZeroU32::new(13)],
            next_edges: [45, 45],
            offset: 0,
        };

        assert_eq!(
            structural_feature_ids(&[], &[surface], &[curve]),
            std::collections::BTreeSet::from([40, 41])
        );
    }

    #[test]
    fn zero_width_toc_has_no_rows() {
        let data = b"#UGC_TOC 2 18446744073709551615 0#\n";

        assert!(toc_sections(data, 0).is_empty());
    }

    #[test]
    fn stored_feature_identities_require_compatible_allfeatur_rows() {
        let operation = FeatureOperation {
            feature_id: 42,
            kind: crate::feature::operations::OperationKind::Stored("Round".to_string()),
            name: crate::feature::operations::OperationName::Stored {
                bytes: b"Round id 42".to_vec(),
                keyword: crate::feature::operations::IdKeyword::Id,
                prefix: None,
            },
            recipe: crate::feature::operations::RecipeResolution::None,
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        };
        let reference = FeatureReferenceName {
            feature_id: 73,
            name_bytes: b"SKETCH_1".to_vec(),
            own_reference_id: 9,
            reference_type: 1,
            offset: 0,
        };
        let datum_reference = FeatureReferenceName {
            feature_id: 87,
            name_bytes: b"Datum Plane id 87".to_vec(),
            own_reference_id: 10,
            reference_type: 1,
            offset: 0,
        };

        let row = |feature_id, root_schema_class| FeatureRow {
            feature_id,
            root_schema_class: Some(crate::feature::schema::SchemaClass::from(root_schema_class)),
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: 0,
            offset: 0,
        };
        let structural = std::collections::BTreeSet::new();

        assert!(feature_row_has_model_identity(
            &row(42, 913),
            &structural,
            std::slice::from_ref(&operation),
            std::slice::from_ref(&reference),
        ));
        assert!(!feature_row_has_model_identity(
            &row(42, 923),
            &structural,
            std::slice::from_ref(&operation),
            std::slice::from_ref(&reference),
        ));
        assert!(feature_row_has_model_identity(
            &row(73, 926),
            &structural,
            &[operation],
            &[reference],
        ));
        assert!(feature_row_has_model_identity(
            &row(87, 923),
            &structural,
            &[],
            std::slice::from_ref(&datum_reference),
        ));
        assert!(!feature_row_has_model_identity(
            &row(87, 911),
            &structural,
            &[],
            &[datum_reference],
        ));
    }

    #[test]
    fn embedded_section_definition_retains_separate_history_feature_owner() {
        let row = FeatureRow {
            feature_id: 42,
            root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
            stream_offset: 100,
            body: b"prefix gsec2d_ptr\0\xe0\x0aname\0S2D0002\0"
                .to_vec()
                .try_into()
                .expect("row body"),
            body_offset: 120,
            offset: 118,
        };

        let definitions = feature_row_definitions(&[row]);

        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].identity.id(), 2);
        assert_eq!(definitions[0].identity.owner_feature_id(), None);
        assert_eq!(definitions[0].offset, 127);
    }

    #[test]
    fn embedded_section_definition_uses_the_bounded_feature_row_for_chain_binding() {
        let row = FeatureRow {
            feature_id: 247,
            root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
            stream_offset: 100,
            body: b"prefix gsec2d_ptr\0\xe0\x0aname\0S2D0002\0\
                    \xe0\x00gsec3d_ptr\0\xf1\xe3\
                    \xe0\x01plane_id\0\x80\xf9\
                    \xe0\x00p_saved_result\0"
                .to_vec()
                .try_into()
                .expect("row body"),
            body_offset: 120,
            offset: 118,
        };
        let definitions = feature_row_definitions(std::slice::from_ref(&row));
        let operation = |feature_id, recipe, offset| FeatureOperation {
            feature_id,
            kind: crate::feature::operations::OperationKind::Stored(String::new()),
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::from(recipe),
            display_state_conflict: false,
            depdb: None,
            offset,
            state_offset: offset,
        };

        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].identity.owner_feature_id(), None);
        assert_eq!(
            definitions[0]
                .section_3d
                .as_ref()
                .and_then(|section| section.sketch_plane_entity_id),
            Some(249)
        );

        let definitions = feature::definitions::bind_section_owners(
            definitions,
            &[
                operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
                operation(248, None, 20),
            ],
            &section_owner_ranges(&[], &[row]),
        );

        assert_eq!(definitions[0].identity.id(), 2);
        assert_eq!(definitions[0].identity.owner_feature_id(), Some(247));
    }
}

#[cfg(test)]
mod tests;
