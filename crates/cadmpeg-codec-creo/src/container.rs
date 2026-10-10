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

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::DecodeContext;
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
use crate::loop_array::{self, LoopArrayScan};
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
/// The extent is a fact of the type. [`Section::scan`] is the only constructor
/// and returns a section/payload pair only when `offset..end` is a region of the file the
/// scan read, so no reader re-derives the sum and none of them can overflow.
///
/// `offset` and `length` are private, so a struct literal outside this module
/// and its descendants does not compile and there is no spelling of a section
/// whose extent the file does not hold.
#[derive(Debug, Clone)]
pub(crate) struct Section {
    /// Raw name as it appeared in the header, when decorated.
    raw_name: String,
    /// Byte range of the normalized name within `raw_name`.
    name: std::ops::Range<usize>,
    /// Payload role of the normalized name.
    role: SectionRole,
    /// Byte offset of the section header within the file.
    offset: usize,
    /// Payload length in bytes (header to the next section, or EOF).
    length: usize,
    /// Expanded payload length from the TOC, excluding the section header.
    expanded_length: Option<usize>,
}

impl DecodeCost for Section {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (
            &self.raw_name,
            (self.name.start, self.name.end),
            self.offset,
            self.length,
            self.expanded_length,
        )
            .decode_cost(ctx, operation)
    }
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

impl cadmpeg_core::decode::cost::DecodeCost for ScannedSection<'_> {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.section, &self.region),
            ctx,
            operation,
        )
    }
}

impl ScannedSection<'_> {
    fn copy_retained(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            section: Section {
                raw_name: ctx
                    .copy_retained_text(&self.section.raw_name, "creo copied section names")?,
                name: self.section.name.clone(),
                role: self.section.role,
                offset: self.section.offset,
                length: self.section.length,
                expanded_length: self.section.expanded_length,
            },
            region: self.region,
        })
    }
}

impl Section {
    /// The section whose payload is `data[offset..end]`, with those bytes, or
    /// `None` when that is not a region of `data`: an end before the offset, or
    /// past the last byte of the file. The name is normalized and classified
    /// here, once, so the accessors do no work.
    pub(crate) fn scan<'a>(
        ctx: &DecodeContext<'_>,
        raw_name: String,
        offset: usize,
        end: usize,
        expanded_length: Option<usize>,
        data: &'a [u8],
    ) -> Result<Option<ScannedSection<'a>>, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let Some(region) = data.get(offset..end) else {
            return Ok(None);
        };
        let name = normalized_name_range(ctx, &raw_name)?;
        let role = classify(&raw_name[name.clone()]);
        Ok(Some(ScannedSection {
            section: Self {
                raw_name,
                name,
                role,
                offset,
                length: region.len(),
                expanded_length,
            },
            region,
        }))
    }

    /// [`Section::scan`] under an unlimited test session.
    #[cfg(test)]
    pub(crate) fn scan_for_test(
        raw_name: String,
        offset: usize,
        end: usize,
        expanded_length: Option<usize>,
        data: &[u8],
    ) -> Option<ScannedSection<'_>> {
        crate::decode::with_test_decode_ctx(|ctx| {
            Self::scan(ctx, raw_name, offset, end, expanded_length, data)
        })
        .expect("test section name is admitted")
    }

    /// Raw name as it appeared in the header, when decorated.
    pub(crate) fn raw_name(&self) -> &str {
        &self.raw_name
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
    /// Plain `+`: [`Section::scan`] admitted the sum, so it is a byte offset of
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
        &self.raw_name[self.name.clone()]
    }

    /// Payload role derived from the section name.
    pub(crate) fn role(&self) -> SectionRole {
        self.role
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
    pub(crate) rows: surface::SurfaceRows,
    /// Typed fixed-prefix rows from the separate invisible and construction
    /// surface namespace.
    pub(crate) nonvisible_rows: surface::SurfaceRows,
    /// Typed fixed-prefix surface rows from the DEPDB cross-section geometry
    /// namespace. These are kept separate from model-face surface rows.
    pub(crate) cross_section_rows: surface::SurfaceRows,
    /// Bounded scalar parameter bodies from positional surface rows.
    pub(crate) parameters: surface::SurfaceParameters,
    /// Bounded scalar parameter bodies from the separate invisible and
    /// construction surface namespace.
    pub(crate) nonvisible_parameters: surface::SurfaceParameters,
    /// Bounded scalar parameter bodies from DEPDB cross-section surface rows.
    pub(crate) cross_section_parameters: surface::SurfaceParameters,
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

fn line_at(ctx: &DecodeContext<'_>, data: &[u8], start: usize) -> Result<String, CodecError> {
    let end = ctx
        .find_bytes_from(data, b"\n", start, "creo version line scan")?
        .unwrap_or(data.len());
    let bytes = &data[start..end];
    if let Ok(line) = ctx.validate_utf8(bytes, "creo version UTF-8 validation")? {
        let trimmed = ctx.trim_text(line, "creo version line trim")?;
        return ctx.copy_retained_text(trimmed, "creo version line");
    }
    let mut text_storage = ctx.reserve_scoped(0, "creo version text storage")?;
    let line = text_storage
        .with_storage(|| ctx.copy_retained_lossy_utf8(bytes, "creo version lossy scratch"))?;
    let trimmed = ctx.trim_text(&line, "creo version line trim")?;
    ctx.copy_retained_text(trimmed, "creo version line")
}

/// Normalize a decorated section name to its base ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)): strip a
/// `ModelView#N` suffix and an `ND:0:<Name>:N` decoration.
///
/// Return the byte range of the normalized name within `raw`.
fn normalized_name_range(
    ctx: &DecodeContext<'_>,
    raw: &str,
) -> Result<std::ops::Range<usize>, CodecError> {
    let base_end = ctx
        .find_text(raw, "#", "creo section name normalization")?
        .unwrap_or(raw.len());
    let base = &raw[..base_end];
    let Some(rest) = base.strip_prefix("ND:") else {
        return Ok(0..base_end);
    };
    let Some(first_colon) = ctx.find_text(rest, ":", "creo section name normalization")? else {
        return Ok(0..base_end);
    };
    let start = "ND:".len() + first_colon + 1;
    let end = ctx
        .find_text(&base[start..], ":", "creo section name normalization")?
        .map_or(base_end, |length| start + length);
    Ok(start..end)
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
fn scan_sections<'a>(
    ctx: &DecodeContext<'_>,
    backing: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    data: &'a [u8],
    body_start: usize,
) -> Result<Vec<ScannedSection<'a>>, CodecError> {
    // Collect header hits as (offset_of_section_hash, raw_name).
    let mut hit_storage = ctx.reserve_scoped(0, "creo section header hit storage")?;
    let mut hits: Vec<(usize, String)> = Vec::new();
    let mut i = body_start;
    if let Some(preceding_byte) = body_start.checked_sub(1) {
        i = preceding_byte;
    }
    let pair_end = data.len() - data.len().min(1);
    let mut positions = i..pair_end;
    while !positions.is_empty() {
        let Some(i) = ctx.next_charged(&mut positions, "creo section framing scan")? else {
            break;
        };
        let toc_delimited = data[i] == 0xf1 && data[i + 1] == b'#';
        if !toc_delimited && (data[i] != b'\n' || data[i + 1] != b'#') {
            continue;
        }
        let hash_off = i + 1; // offset of the section-header '#'
        let name_start = i + 2;
        let Some(nl) =
            ctx.find_bytes_from(data, b"\n", name_start, "creo section name boundary scan")?
        else {
            break;
        };
        let name_bytes = &data[name_start..nl];
        positions = nl..pair_end;
        // A name contains only defined ASCII name bytes and has an alphanumeric byte.
        if name_bytes.len() < 2
            || !ctx.all_by(
                name_bytes,
                |byte| Ok(is_name_byte(*byte)),
                "creo section name validation",
            )?
            || !ctx.any_by(
                name_bytes,
                |byte| Ok(byte.is_ascii_alphanumeric()),
                "creo section name validation",
            )?
        {
            continue;
        }
        let name = ctx
            .validate_utf8(name_bytes, "creo UTF-8 validation")?
            .map_err(|_| CodecError::malformed("non-ASCII Creo section name"))?;
        if FRAMING_NAMES.contains(&name) {
            continue;
        }
        if toc_delimited {
            let directory_end = hits.first().map_or(body_start, |(offset, _)| *offset);
            let Some(directory) = data.get(..directory_end) else {
                return Err(CodecError::malformed(ctx.format_retained(
                    format_args!(
                        "creo section `{name}` is TOC-delimited and its directory window ends at \
                         {directory_end}, past the file length {}",
                        data.len(),
                    ),
                    "creo section directory bounds error",
                )?));
            };
            if !toc_lists_section(ctx, directory, name_bytes)? {
                continue;
            }
        }
        let raw = ctx.copy_retained_text(name, "creo section header names")?;
        hit_storage.with_storage(|| ctx.reserve_vec(&mut hits, 1, "creo section header hits"))?;
        hits.push((hash_off, raw));
    }

    let mut sections = Vec::new();
    ctx.reserve_scoped_vec(backing, &mut sections, hits.len(), "creo scanned sections")?;
    let mut headers = hits.into_iter().peekable();
    while headers.len() != 0 {
        let Some((offset, name)) =
            ctx.next_charged(&mut headers, "creo section header traversal")?
        else {
            break;
        };
        let end = headers.peek().map_or(data.len(), |(next, _)| *next);
        sections.extend(Section::scan(ctx, name, offset, end, None, data)?);
    }
    Ok(sections)
}

fn toc_sections<'a>(
    ctx: &DecodeContext<'_>,
    backing: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    data: &'a [u8],
    header_base: usize,
) -> Result<Vec<ScannedSection<'a>>, CodecError> {
    let mut sections = Vec::new();
    let mut offset_storage = ctx.reserve_scoped(0, "creo TOC section offsets")?;
    let mut offsets = std::collections::HashSet::new();
    let mut toc_from = 0;
    while let Some(toc_offset) =
        ctx.find_bytes_from(data, TOC_START, toc_from, "creo TOC discovery scan")?
    {
        toc_from = toc_offset + TOC_START.len();
        let Some(line_end) =
            ctx.find_bytes_from(data, b"\n", toc_offset, "creo TOC header scan")?
        else {
            continue;
        };
        let Ok(header) = ctx.validate_utf8(&data[toc_offset..line_end], "creo UTF-8 validation")?
        else {
            continue;
        };
        let mut fields =
            ctx.trim_end_matches(header, |c| Ok(c == '#'), "creo TOC header padding")?;
        legacy::text_field(ctx, &mut fields, false)?;
        legacy::text_field(ctx, &mut fields, false)?;
        let count = match legacy::text_field(ctx, &mut fields, false)? {
            Some(value) => ctx
                .parse_text::<usize>(value, "creo scalar text parsing")?
                .ok(),
            None => None,
        };
        let row_width = match legacy::text_field(ctx, &mut fields, false)? {
            Some(value) => ctx
                .parse_text::<usize>(value, "creo scalar text parsing")?
                .ok(),
            None => None,
        };
        let (Some(count), Some(row_width)) = (count, row_width) else {
            continue;
        };
        if row_width == 0 {
            continue;
        }
        let rows_start = line_end + 1;
        let mut indices = 0..count;
        while !indices.is_empty() {
            let index = indices.start;
            let Some(start) = index
                .checked_mul(row_width)
                .and_then(|relative| rows_start.checked_add(relative))
            else {
                break;
            };
            let Some(end) = start.checked_add(row_width) else {
                break;
            };
            let Some(row) = data.get(start..end) else {
                break;
            };
            let Some(_) = ctx.next_charged(&mut indices, "creo TOC row traversal")? else {
                break;
            };
            let Ok(row) = ctx.validate_utf8(row, "creo TOC row UTF-8")? else {
                continue;
            };
            let mut fields = ctx.trim_end_matches(
                row,
                |c| Ok(matches!(c, '#' | '\n' | '\r' | ' ')),
                "creo TOC row padding",
            )?;
            let Some(name) = legacy::text_field(ctx, &mut fields, false)? else {
                continue;
            };
            if name == "NEXT_TOC_ENTRY" {
                continue;
            }
            let (view_id, offset_field, length_field, expanded_field) = if name == "ModelView" {
                let (Some(id), Some(offset), Some(length), Some(expanded)) = (
                    legacy::text_field(ctx, &mut fields, false)?,
                    legacy::text_field(ctx, &mut fields, false)?,
                    legacy::text_field(ctx, &mut fields, false)?,
                    legacy::text_field(ctx, &mut fields, false)?,
                ) else {
                    continue;
                };
                (Some(id), offset, length, expanded)
            } else {
                let (Some(offset), Some(length), Some(expanded)) = (
                    legacy::text_field(ctx, &mut fields, false)?,
                    legacy::text_field(ctx, &mut fields, false)?,
                    legacy::text_field(ctx, &mut fields, false)?,
                ) else {
                    continue;
                };
                (None, offset, length, expanded)
            };
            let (Ok(relative_offset), Ok(length), Ok(expanded_length)) = (
                ctx.parse_radix::<usize>(offset_field, 16, "creo TOC offset hexadecimal parsing")?,
                ctx.parse_radix::<usize>(length_field, 16, "creo TOC length hexadecimal parsing")?,
                ctx.parse_radix::<usize>(
                    expanded_field,
                    16,
                    "creo TOC expanded length hexadecimal parsing",
                )?,
            ) else {
                continue;
            };
            let Some(offset) = header_base.checked_add(relative_offset) else {
                continue;
            };
            const VIEW_PREFIX: &[u8] = b"ModelView#";
            let raw_name_len = match view_id {
                Some(id) => VIEW_PREFIX.len().checked_add(id.len()),
                None => Some(name.len()),
            };
            let Some(marker_len) = raw_name_len.and_then(|length| length.checked_add(2)) else {
                continue;
            };
            let Some(marker_end) = offset.checked_add(marker_len) else {
                continue;
            };
            let Some(end) = offset.checked_add(length) else {
                continue;
            };
            if data.get(offset..end).is_none() {
                continue;
            }
            let Some(marker) = data.get(offset..marker_end) else {
                continue;
            };
            if length < marker_len || marker.first() != Some(&b'#') || marker.last() != Some(&b'\n') {
                continue;
            }
            let marker_name = &marker[1..marker.len() - 1];
            let name_matches = match view_id {
                Some(id) => marker_name.starts_with(VIEW_PREFIX)
                    && ctx.equal(
                        &marker_name[VIEW_PREFIX.len()..],
                        id.as_bytes(),
                        "creo TOC marker name equality",
                    )?,
                None => ctx.equal(marker_name, name.as_bytes(), "creo TOC marker name equality")?,
            };
            if !name_matches {
                continue;
            }
            if !offset_storage.with_storage(|| ctx.insert_hash_set(&mut offsets, offset,
                "creo TOC section offsets"))? {
                continue;
            }
            let raw_name = match view_id {
                Some(id) => ctx.format_retained(format_args!("ModelView#{id}"), "creo TOC section names")?,
                None => ctx.copy_retained_text(name, "creo TOC section names")?,
            };
            ctx.reserve_scoped_vec(backing, &mut sections, 1, "creo TOC sections")?;
            sections.extend(Section::scan(
                ctx,
                raw_name,
                offset,
                end,
                Some(expanded_length),
                data,
            )?);
        }
    }
    if sections.len() > 1 {
        ctx.stable_sort_by_key(
            sections.as_mut_slice(),
            |value| value.section.offset(),
            Ord::cmp,
            "creo toc sections sections ordering",
        )?;
    }
    Ok(sections)
}

fn legacy_toc_sections<'a>(
    ctx: &DecodeContext<'_>,
    backing: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    data: &'a [u8],
    banner_offset: usize,
) -> Result<Vec<ScannedSection<'a>>, CodecError> {
    let Some(toc_offset) = ctx
        .find_bytes_from(
            data,
            b"\n@Toc ",
            banner_offset,
            "find Creo container marker",
        )?
        .map(|offset| offset + 1)
    else {
        return Ok(Vec::new());
    };
    let Some((toc_declaration, after_toc_declaration)) = legacy::line(ctx, data, toc_offset)?
    else {
        return Ok(Vec::new());
    };
    let Some((toc_id, _, _)) =
        legacy::parse_declaration(ctx, toc_declaration)?.filter(|(_, name, type_code)| {
            *name == "Toc" && matches!(type_code, LegacyTypeCode::Object)
        })
    else {
        return Ok(Vec::new());
    };
    let Some((toc_value, after_toc_value)) = legacy::line(ctx, data, after_toc_declaration)? else {
        return Ok(Vec::new());
    };
    let Ok(toc_value) = ctx.validate_utf8(toc_value, "creo UTF-8 validation")? else {
        return Ok(Vec::new());
    };
    let mut toc_fields = toc_value;
    if legacy::text_field(ctx, &mut toc_fields, true)? != Some("0")
        || match legacy::text_field(ctx, &mut toc_fields, true)? {
            Some(id) => ctx.parse_text::<u32>(id, "creo scalar text parsing")?.ok(),
            None => None,
        } != Some(toc_id)
        || legacy::text_field(ctx, &mut toc_fields, true)? != Some("->")
        || legacy::text_field(ctx, &mut toc_fields, true)?.is_some()
    {
        return Ok(Vec::new());
    }

    let Some((entry_declaration, after_entry_declaration)) =
        legacy::line(ctx, data, after_toc_value)?
    else {
        return Ok(Vec::new());
    };
    let Some((entry_id, _, _)) =
        legacy::parse_declaration(ctx, entry_declaration)?.filter(|(_, name, type_code)| {
            *name == "entry" && matches!(type_code, LegacyTypeCode::String)
        })
    else {
        return Ok(Vec::new());
    };
    let Some((entry_array, mut next)) = legacy::line(ctx, data, after_entry_declaration)? else {
        return Ok(Vec::new());
    };
    let Ok(entry_array) = ctx.validate_utf8(entry_array, "creo UTF-8 validation")? else {
        return Ok(Vec::new());
    };
    let mut array_fields = entry_array;
    if legacy::text_field(ctx, &mut array_fields, true)? != Some("1")
        || match legacy::text_field(ctx, &mut array_fields, true)? {
            Some(id) => ctx.parse_text::<u32>(id, "creo scalar text parsing")?.ok(),
            None => None,
        } != Some(entry_id)
    {
        return Ok(Vec::new());
    }
    let Some(count_field) = legacy::text_field(ctx, &mut array_fields, true)? else {
        return Ok(Vec::new());
    };
    let Some(count) = count_field
        .strip_prefix('[')
        .and_then(|count| count.strip_suffix(']'))
    else {
        return Ok(Vec::new());
    };
    let Ok(count) = ctx.parse_text::<usize>(count, "creo scalar text parsing")? else {
        return Ok(Vec::new());
    };
    if legacy::text_field(ctx, &mut array_fields, true)?.is_some() {
        return Ok(Vec::new());
    }

    let Some(remaining) = data.len().checked_sub(next) else {
        return Ok(Vec::new());
    };
    if cadmpeg_core::decode::bounded_len(cadmpeg_core::decode::u64_from_index(count), 1, remaining)
        .is_none()
    {
        return Ok(Vec::new());
    }
    let mut sections = Vec::new();
    let mut offset_storage = ctx.reserve_scoped(0, "creo legacy TOC section offsets")?;
    let mut offsets = std::collections::HashSet::new();
    let mut entries = 0..count;
    while !entries.is_empty() && next < data.len() {
        let Some(_) = ctx.next_charged(&mut entries, "creo legacy TOC entries")? else {
            break;
        };
        let Some((entry, after_entry)) = legacy::line(ctx, data, next)? else {
            break;
        };
        next = after_entry;
        let Ok(entry) = ctx.validate_utf8(entry, "creo legacy TOC entry UTF-8")? else {
            continue;
        };
        let entry =
            ctx.trim_end_matches(entry, |c| Ok(c == '#'), "creo legacy TOC entry padding")?;
        let mut fields = ctx.trim_end_text(entry, "creo legacy TOC entry whitespace")?;
        let (
            Some(kind),
            Some(id),
            Some(raw_name),
            Some(offset_field),
            Some(length_field),
            Some(zero),
            Some(revision),
            None,
        ) = (
            legacy::text_field(ctx, &mut fields, true)?,
            legacy::text_field(ctx, &mut fields, true)?,
            legacy::text_field(ctx, &mut fields, true)?,
            legacy::text_field(ctx, &mut fields, true)?,
            legacy::text_field(ctx, &mut fields, true)?,
            legacy::text_field(ctx, &mut fields, true)?,
            legacy::text_field(ctx, &mut fields, true)?,
            legacy::text_field(ctx, &mut fields, true)?,
        )
        else {
            continue;
        };
        if kind != "2"
            || ctx.parse_text::<u32>(id, "creo legacy TOC entry ID")?.ok() != Some(entry_id)
            || zero != "0"
            || ctx
                .parse_text::<u32>(revision, "creo legacy TOC entry revision")?
                .is_err()
        {
            continue;
        }
        if raw_name.len() < 2
            || !ctx.all_by(
                raw_name.bytes(),
                |byte| Ok(is_name_byte(byte)),
                "creo legacy TOC name validation",
            )?
            || !ctx.any_by(
                raw_name.bytes(),
                |byte| Ok(byte.is_ascii_alphanumeric()),
                "creo legacy TOC name validation",
            )?
        {
            continue;
        }
        let (Ok(relative_offset), Ok(length)) = (
            ctx.parse_radix::<usize>(offset_field, 16, "creo legacy TOC offset parsing")?,
            ctx.parse_radix::<usize>(length_field, 16, "creo legacy TOC length parsing")?,
        ) else {
            continue;
        };
        let Some(offset) = banner_offset.checked_add(relative_offset) else {
            continue;
        };
        let Some(marker_len) = raw_name.len().checked_add(2) else {
            continue;
        };
        let Some(marker_end) = offset.checked_add(marker_len) else {
            continue;
        };
        let Some(end) = offset.checked_add(length) else {
            continue;
        };
        let Some(marker) = data.get(offset..marker_end) else {
            continue;
        };
        if length < marker_len
            || marker.first() != Some(&b'#')
            || !ctx.equal(
                &marker[1..=raw_name.len()],
                raw_name.as_bytes(),
                "creo TOC marker name equality",
            )?
            || marker.last() != Some(&b'\n')
        {
            continue;
        }
        if data.get(offset..end).is_none() {
            continue;
        }
        if !offset_storage.with_storage(|| ctx.insert_hash_set(&mut offsets, offset,
            "creo legacy TOC section offsets"))? {
            continue;
        }
        let raw_name = ctx.copy_retained_text(raw_name, "creo legacy TOC section names")?;
        ctx.reserve_scoped_vec(backing, &mut sections, 1, "creo legacy TOC sections")?;
        sections.extend(Section::scan(ctx, raw_name, offset, end, None, data)?);
    }
    if sections.len() > 1 {
        ctx.stable_sort_by_key(
            sections.as_mut_slice(),
            |value| value.section.offset(),
            Ord::cmp,
            "creo legacy toc sections sections ordering",
        )?;
    }
    Ok(sections)
}

fn expanded_sections(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    sections: &[ScannedSection<'_>],
) -> Result<Vec<ExpandedSection>, CodecError> {
    const MAX_EXPANDED_SECTION: usize = 256 * 1024 * 1024;
    let mut expanded_sections = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        let Some(expected_length) = section.section.expanded_length else {
            continue;
        };
        if expected_length > MAX_EXPANDED_SECTION {
            return Err(ctx.refuse_codec_limit(
                "creo expanded section ceiling",
                cadmpeg_core::decode::u64_from_index(MAX_EXPANDED_SECTION),
                cadmpeg_core::decode::u64_from_index(expected_length),
            ));
        }
        let Some(header_length) = section.section.raw_name.len().checked_add(2) else {
            continue;
        };
        let Some(source_offset) = section.section.offset().checked_add(header_length) else {
            continue;
        };
        let Some(payload) = data.get(source_offset..section.section.end()) else {
            continue;
        };
        if !payload.starts_with(UNIX_COMPRESS_MAGIC) {
            continue;
        }
        let Some(expanded) = crate::compress::decode(ctx, payload, expected_length)? else {
            continue;
        };
        let name = ctx.copy_retained_text(section.section.name(), "creo expanded section names")?;
        ctx.reserve_vec(&mut expanded_sections, 1, "creo expanded sections")?;
        expanded_sections.push(ExpandedSection {
            name,
            source_offset,
            compressed_length: payload.len(),
            data: expanded,
        });
    }
    Ok(expanded_sections)
}

/// Find the expanded payload owned by one section.
pub(crate) fn expanded_section_for<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    section: &Section,
) -> Result<Option<&'a ExpandedSection>, CodecError> {
    ctx.find_by(
        &scan.framing.expanded_sections,
        |expanded| {
            // An expanded payload begins after its section's `#<name>\n` header, so
            // its source offset is inside the section but never its first byte.
            Ok(section.contains(expanded.source_offset)
                && expanded.source_offset != section.offset()
                && ctx.equal(
                    expanded.name.as_str(),
                    section.name(),
                    "creo expanded section name comparison",
                )?)
        },
        "creo expanded section selection",
    )
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

fn toc_lists_section(ctx: &DecodeContext<'_>, toc: &[u8], name: &[u8]) -> Result<bool, CodecError> {
    ctx.any_by(
        toc.windows(name.len() + 2),
        |window| {
            Ok(window[0] == b'\n'
                && window[1 + name.len()] == b' '
                && ctx.equal(&window[1..=name.len()], name, "creo TOC name equality")?)
        },
        "creo TOC name lookup",
    )
}

/// Section-name bytes: printable ASCII minus space, plus the `ND:` decoration
/// punctuation and the `ModelView#N` separator.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-' | b'#')
}

const DEPDB_ROOT_RECORD: &[u8] = b"\xe0\x00p_dep_db\0\xe3";

fn legacy_product_release(
    ctx: &DecodeContext<'_>,
    banner: &[u8],
) -> Result<Option<String>, CodecError> {
    let mut pending = banner;
    let mut separate_release = false;
    loop {
        let Some(start) = ctx.position_by(
            pending,
            |byte| Ok(!byte.is_ascii_whitespace()),
            "creo legacy banner word start",
        )?
        else {
            return Ok(None);
        };
        let rest = &pending[start..];
        let boundary = ctx
            .position_by(
                &rest[1..],
                |byte| Ok(byte.is_ascii_whitespace()),
                "creo legacy banner word end",
            )?
            .map(|offset| offset + 1);
        let end = boundary.unwrap_or(rest.len());
        let word = &rest[..end];
        pending = boundary.map_or(&[][..], |offset| &rest[offset + 1..]);
        let release = if separate_release {
            if !ctx.all_by(
                word,
                |byte| Ok(byte.is_ascii_graphic()),
                "creo legacy product release validation",
            )? {
                return Ok(None);
            }
            word
        } else if word == b"Version" || word == b"Release" {
            separate_release = true;
            continue;
        } else if let Some(release) = word.strip_prefix(b"Release") {
            if release.is_empty()
                || !ctx.all_by(
                    release,
                    |byte| Ok(byte.is_ascii_graphic()),
                    "creo legacy product release validation",
                )?
            {
                continue;
            }
            release
        } else {
            continue;
        };
        let release = ctx
            .validate_utf8(release, "creo UTF-8 validation")?
            .map_err(|_| CodecError::malformed("non-ASCII Creo release"))?;
        return ctx
            .copy_retained_text(release, "creo legacy product release")
            .map(Some);
    }
}

fn legacy_ascii_framing(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<LegacyAsciiFraming>, CodecError> {
    let Some(header_end) = ctx
        .find_bytes_from(data, UGC_HEADER_END, 0, "find Creo container marker")?
        .and_then(|offset| offset.checked_add(UGC_HEADER_END.len()))
    else {
        return Ok(None);
    };
    let Some(body) = data
        .get(header_end..)
        .and_then(|tail| tail.strip_prefix(b"\n"))
    else {
        return Ok(None);
    };
    if !body.starts_with(LEGACY_OBJECT_START) {
        return Ok(None);
    }
    let Some(object_header_end) = ctx.find_bytes_from(
        body,
        b"\n",
        LEGACY_OBJECT_START.len(),
        "find Creo container marker",
    )?
    else {
        return Ok(None);
    };
    let schema = &body[LEGACY_OBJECT_START.len()..object_header_end];
    if schema.is_empty()
        || !ctx.all_by(
            schema,
            |byte| Ok(byte.is_ascii_digit()),
            "creo legacy schema digit validation",
        )?
    {
        return Ok(None);
    }
    let schema = ctx
        .validate_utf8(schema, "creo UTF-8 validation")?
        .map_err(|_| CodecError::malformed("non-ASCII Creo schema"))?;
    let schema = ctx.copy_retained_text(schema, "creo legacy schema")?;
    let mut from = object_header_end + 1;
    while let Some(object_end) =
        ctx.find_bytes_from(body, LEGACY_OBJECT_END, from, "find Creo container marker")?
    {
        if let Some(banner) = object_end
            .checked_add(LEGACY_OBJECT_END.len())
            .and_then(|banner| body.get(banner..))
            .and_then(|tail| tail.strip_prefix(b"\n"))
            .filter(|tail| tail.starts_with(LEGACY_BANNER_START))
        {
            let banner_end = ctx
                .find_bytes_from(banner, b"\n", 0, "find Creo container marker")?
                .unwrap_or(banner.len());
            let banner_offset = data.len() - banner.len();
            return Ok(Some(LegacyAsciiFraming {
                schema,
                product_release: legacy_product_release(ctx, &banner[..banner_end])?,
                banner_offset,
                object_offset: header_end + 1,
                persistence: legacy::Persistence::default(),
            }));
        }
        from = object_end + 1;
    }
    Ok(None)
}

fn legacy_scope_ranges(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    framing: &LegacyAsciiFraming,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<std::ops::Range<usize>>, CodecError> {
    let count = sections
        .len()
        .checked_add(1)
        .ok_or_else(|| CodecError::malformed("legacy persistence scope count exceeds usize"))?;
    let mut scopes = Vec::new();
    ctx.reserve_vec(&mut scopes, count, "creo legacy persistence scopes")?;
    let initial_end = sections
        .first()
        .map_or(data.len(), |section| section.section.offset());
    scopes.push(framing.object_offset..initial_end);
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        let region = section.region;
        let Some(payload_start) = section
            .section
            .offset()
            .checked_add(section.section.raw_name.len())
            .and_then(|start| start.checked_add(2))
        else {
            continue;
        };
        if legacy::starts_with_declaration(ctx, data, payload_start)? {
            scopes.push(section.section.offset()..section.section.offset() + region.len());
        }
    }
    Ok(scopes)
}

/// Identify the layout family structurally ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#1-container)). The
/// `DEPDB_DATA` root record is authoritative because a persistence payload can
/// contain embedded names with the `ND:` decoration. An undecorated file with
/// neither a valid root record, an outer `ND:` name, nor a complete legacy
/// ASCII object remains unknown.
fn identify_layout(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    legacy_ascii: Option<LegacyAsciiFraming>,
) -> Result<Layout, CodecError> {
    let mut has_depdb_root = false;
    let mut has_depdb_section = false;
    let mut has_nd_decoration = false;
    for section in ctx.admit_iter(sections, "creo layout section traversal")? {
        has_nd_decoration |= section.section.raw_name.starts_with("ND:");
        if section.section.name() == "DEPDB_DATA" {
            has_depdb_section = true;
            let header_length = section.section.raw_name.len() + 2;
            has_depdb_root |= section
                .region
                .get(header_length..)
                .is_some_and(|payload| payload.starts_with(DEPDB_ROOT_RECORD));
        }
    }
    Ok(if has_depdb_section {
        if has_depdb_root {
            Layout::Depdb
        } else {
            Layout::Unknown(UnknownLayout::DepdbRootMissing)
        }
    } else if has_nd_decoration {
        Layout::Nd
    } else if let Some(framing) = legacy_ascii {
        ctx.charge_collection_items(1, "creo legacy framing box")?;
        Layout::LegacyAscii(Box::new(framing))
    } else {
        Layout::Unknown(UnknownLayout::NoDiscriminant)
    })
}

/// Sum every valid `<label>\0 [skip] f8 <count>` header in `region`.
/// After the label's NUL terminator, up to two optional non-`f8` framing bytes
/// (e.g. the `f3`/`f2` `crv_array` discriminators) are skipped before the
/// required `f8` opener, whose compact-integer count is then decoded ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array),
/// [§5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#5-topology-and-section-records)).
fn read_array_count(
    ctx: &DecodeContext<'_>,
    region: &[u8],
    label: &[u8],
) -> Result<Option<u32>, CodecError> {
    let mut from = 0;
    let mut total = 0u32;
    let mut found = false;
    while let Some(pos) = ctx.find_bytes_from(region, label, from, "creo geometry census search")? {
        let mut p = pos + label.len();
        // Require the NUL that terminates the namespace label.
        if region.get(p) == Some(&0) {
            p += 1;
            // Skip up to two framing bytes before the array opener.
            for _ in 0..3 {
                match region.get(p) {
                    Some(&psb::token::ARRAY_OPEN) => {
                        let (count, _) =
                            psb::complete_compact_int(region, p + 1).ok_or_else(|| {
                                CodecError::malformed("incomplete geometry census count")
                            })?;
                        let Some(sum) = total.checked_add(count) else {
                            return Err(CodecError::malformed(ctx.format_retained(
                                format_args!(
                                    "creo `{}` namespace array at offset {pos} declares {count} \
                                     entries, which added to the {total} already declared exceeds \
                                     the 32-bit census",
                                    String::from_utf8_lossy(label),
                                ),
                                "creo geometry array census error",
                            )?));
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
fn geom_census(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<GeomCensus, CodecError> {
    let Some(vg) = (match ctx.find_by(
        sections,
        |section| Ok(section.section.name() == VISIBGEOM),
        "creo geometry section selection",
    )? {
        Some(section) => Some(section),
        None => ctx.find_by(
            sections,
            |section| Ok(section.section.name() == "DEPDB_DATA"),
            "creo geometry section fallback",
        )?,
    }) else {
        return Ok(GeomCensus::default());
    };
    let region = vg.region;
    Ok(GeomCensus {
        srf_array_count: read_array_count(ctx, region, b"srf_array")?,
        crv_array_count: read_array_count(ctx, region, b"crv_array")?,
    })
}

/// Agreement of all binary unit declarations in the image.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BinaryUnitSelection {
    Absent,
    Selected(legacy::PrincipalUnitSystem),
    Unsupported,
    Conflicting,
}

fn binary_principal_unit(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<BinaryUnitSelection, CodecError> {
    let mut selector = None;
    let mut conflicting = false;
    let mut from = 0;
    while let Some(found) = ctx.find_bytes_from(
        data,
        PRINCIPAL_UNIT_ID,
        from,
        "creo binary unit declaration scan",
    )? {
        let start = found + PRINCIPAL_UNIT_ID.len();
        let Some(&value) = data.get(start) else {
            return Ok(BinaryUnitSelection::Unsupported);
        };
        if let Some(previous) = selector {
            conflicting |= previous != value;
        } else {
            selector = Some(value);
        }
        from = start;
    }
    Ok(if conflicting {
        BinaryUnitSelection::Conflicting
    } else {
        match selector {
            None => BinaryUnitSelection::Absent,
            Some(51) => {
                BinaryUnitSelection::Selected(legacy::PrincipalUnitSystem::MillimeterNewtonSecond)
            }
            Some(54) => {
                BinaryUnitSelection::Selected(legacy::PrincipalUnitSystem::InchPoundMassSecond)
            }
            Some(55) => {
                BinaryUnitSelection::Selected(legacy::PrincipalUnitSystem::MillimeterKilogramSecond)
            }
            Some(_) => BinaryUnitSelection::Unsupported,
        }
    })
}

fn cmnm_model_name(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Option<(String, usize)>, CodecError> {
    const PREFIX: &[u8] = &cmnm::PREFIX_VALUE;
    let Some(marker) = ctx.find_bytes_from(data, PREFIX, 0, "creo container model-name scan")?
    else {
        return Ok(None);
    };
    let start = marker + cmnm::NAME_LENGTH_HEX;
    if ctx
        .find_bytes_from(data, PREFIX, start, "creo container model-name scan")?
        .is_some()
    {
        return Ok(None);
    }

    let Some(length_bytes) = data.get(start..marker + cmnm::LEN) else {
        return Ok(None);
    };
    let Ok(length_text) = std::str::from_utf8(length_bytes) else {
        return Ok(None);
    };
    let Ok(length) = usize::from_str_radix(length_text, 16) else {
        return Ok(None);
    };
    let Some(name) = data.get(marker + cmnm::LEN..marker + cmnm::LEN + length) else {
        return Ok(None);
    };
    if name.is_empty()
        || ctx.any_by(
            name,
            |byte| Ok(matches!(byte, 0 | b'\n' | b'\r')),
            "creo CMNM forbidden name byte traversal",
        )?
    {
        return Ok(None);
    }
    let Ok(name) = ctx.validate_utf8(name, "creo UTF-8 validation")? else {
        return Ok(None);
    };
    Ok(Some((
        ctx.copy_retained_text(name, "creo CMNM model name")?,
        marker + cmnm::LEN,
    )))
}

/// Find the root model name stored by binary sections that do not carry a
/// `CMNM` header record.
fn native_model_name(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Option<(String, usize)>, CodecError> {
    const FIELD: &[u8] = b"model_name\0";

    ctx.find_map(
        sections,
        |section| {
            if section.section.role() == SectionRole::Thumbnail {
                return Ok(None);
            }
            let region = section.region;
            let mut from = 0;
            while let Some(field) =
                ctx.find_bytes_from(region, FIELD, from, "find Creo native model-name field")?
            {
                let value_start = field + FIELD.len();
                if region.get(value_start) == Some(&0xe1) {
                    from = value_start + 1;
                    continue;
                }
                let Some(value_end) = ctx.find_bytes_from(
                    region,
                    b"\0",
                    value_start,
                    "find Creo native model-name end",
                )?
                else {
                    break;
                };
                let mut name_start = value_start;
                if region.get(name_start) == Some(&0xf1) {
                    name_start += 1;
                }
                let value = &region[name_start..value_end];
                if let Ok(name) = ctx.validate_utf8(value, "creo UTF-8 validation")? {
                    if !name.is_empty()
                        && ctx.all_by(
                            name.chars(),
                            |character| Ok(!character.is_control()),
                            "creo native name control validation",
                        )?
                    {
                        return Ok(Some((
                            ctx.copy_retained_text(name, "creo native model name")?,
                            section.section.offset() + name_start,
                        )));
                    }
                }
                from = value_end + 1;
            }
            Ok(None)
        },
        "creo native model-name section selection",
    )
}

fn relation_model_name<'a>(
    ctx: &DecodeContext<'_>,
    filename: &'a str,
) -> Result<Option<&'a str>, CodecError> {
    let filename = ctx.trim_end_matches(
        filename,
        |character| Ok(character == ' '),
        "creo relation model name padding",
    )?;
    let part_suffix = if filename.len() >= 4 {
        match filename.get(filename.len() - 4..) {
            Some(suffix) => suffix.eq_ignore_ascii_case(".prt"),
            None => false,
        }
    } else {
        false
    };
    let name = if part_suffix {
        &filename[..filename.len() - 4]
    } else if ctx
        .find_text(filename, ".", "creo relation model extension search")?
        .is_none()
    {
        filename
    } else {
        return Ok(None);
    };
    Ok((!name.is_empty()).then_some(name))
}

fn family_table(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Option<FamilyTableRecord>, CodecError> {
    let Some(section) = ctx.find_by(
        sections,
        |section| Ok(section.section.name() == "FamilyInf"),
        "creo named section selection",
    )?
    else {
        return Ok(None);
    };
    let payload = section.region;
    let label = b"drv_tbl_ptr\0";
    let Some(label_offset) =
        ctx.find_bytes_from(payload, label, 0, "find Creo family table")?
    else {
        return Ok(None);
    };
    let offset = label_offset + label.len();
    let pointer = match payload.get(offset) {
        Some(&0xe1) => FamilyTablePointer::Null,
        Some(&psb::token::ENTITY_REF) => {
            let Ok((id, _)) = psb::reference_id(payload, offset + 1) else {
                return Ok(None);
            };
            FamilyTablePointer::Entity(id)
        }
        _ => return Ok(None),
    };
    Ok(Some(FamilyTableRecord {
        pointer,
        offset: section.section.offset() + offset,
    }))
}

fn model_geometry_sections<'a>(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'a>],
) -> Result<Vec<ScannedSection<'a>>, CodecError> {
    let visible_namespace_present = ctx.any_by(
        sections,
        |candidate| {
            if candidate.section.name() != VISIBGEOM {
                return Ok(false);
            }
            let payload = candidate.region;
            Ok(ctx
                .find_bytes_from(payload, b"srf_array\0", 0, "find Creo container marker")?
                .is_some()
                || ctx
                    .find_bytes_from(payload, b"crv_array\0", 0, "find Creo container marker")?
                    .is_some())
        },
        "creo visible geometry section selection",
    )?;
    let mut selected = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        let keep = if visible_namespace_present {
            section.section.name() == VISIBGEOM
        } else if section.section.name() == "DEPDB_DATA" {
            let payload = section.region;
            ctx.find_bytes_from(payload, b"srf_array\0", 0, "find Creo container marker")?
                .is_some()
                || ctx
                    .find_bytes_from(payload, b"crv_array\0", 0, "find Creo container marker")?
                    .is_some()
        } else {
            false
        };
        if keep {
            ctx.reserve_vec(&mut selected, 1, "creo model geometry sections")?;
            selected.push(section.copy_retained(ctx)?);
        }
    }
    Ok(selected)
}

fn nonvisible_geometry_sections<'a>(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'a>],
) -> Result<Vec<ScannedSection<'a>>, CodecError> {
    let mut selected = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        if section.section.name() != "NovisGeom" {
            continue;
        }
        ctx.reserve_vec(&mut selected, 1, "creo nonvisible geometry sections")?;
        selected.push(section.copy_retained(ctx)?);
    }
    Ok(selected)
}

fn loop_array_sections<'a>(
    ctx: &DecodeContext<'_>,
    model: &[ScannedSection<'a>],
    nonvisible: &[ScannedSection<'a>],
    sections: &[ScannedSection<'a>],
) -> Result<Vec<ScannedSection<'a>>, CodecError> {
    let mut selected = Vec::new();
    let mut offset_storage = ctx.reserve_scoped(0, "creo loop section offset storage")?;
    let mut offsets = std::collections::HashSet::new();
    for section in ctx
        .admit_iter(model, "creo loop model section traversal")?
        .chain(ctx.admit_iter(nonvisible, "creo loop nonvisible section traversal")?)
    {
        if !offset_storage.with_storage(|| ctx.insert_hash_set(
            &mut offsets, section.section.offset(), "creo loop section offsets"))? {
            continue;
        }
        ctx.reserve_vec(&mut selected, 1, "creo loop array sections")?;
        selected.push(section.copy_retained(ctx)?);
    }
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        if section.section.name() != "Xsections" {
            continue;
        }
        if ctx
            .find_bytes_from(
                section.region,
                b"Sld_Xsections\0",
                0,
                "find Creo container marker",
            )?
            .is_some()
        {
            if !offset_storage.with_storage(|| ctx.insert_hash_set(
            &mut offsets, section.section.offset(), "creo loop section offsets"))? {
            continue;
        }
        ctx.reserve_vec(&mut selected, 1, "creo loop array sections")?;
            selected.push(section.copy_retained(ctx)?);
        }
    }
    if selected.len() > 1 {
        ctx.stable_sort_by_key(
            selected.as_mut_slice(),
            |value| value.section.offset(),
            Ord::cmp,
            "creo loop array sections selected ordering",
        )?;
    }
    Ok(selected)
}

fn surface_rows(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<SurfaceRow>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| surface::rows(ctx, bytes),
        |row, base| {
            row.offset += base;
            Ok(())
        },
        |row| row.offset,
    )
}

fn cross_section_surface_rows(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<SurfaceRow>, CodecError> {
    collect_section_records_result(
        ctx,
        cross_sections(ctx, sections)?,
        |bytes| surface::cross_section_rows(ctx, bytes),
        |record, base| {
            record.offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn surface_prototype_count(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<usize, CodecError> {
    let mut total = 0usize;
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        let section_bytes = section.region;
        total += surface::prototype_count(ctx, section_bytes)?;
    }
    Ok(total)
}

fn surface_prototype_records(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    refusals: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Vec<SurfacePrototypeRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| surface::named_prototype_records(ctx, bytes, refusals),
        |record, base| {
            record.offset += base;
            for parameter in ctx.admit_iter(
                &mut record.parameters,
                "creo record child relocation traversal",
            )? {
                parameter.offset += base;
                parameter.value_offset += base;
            }
            Ok(())
        },
        |record| record.offset,
    )
}

fn surface_parameters(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<SurfaceParameterRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| surface::parameter_records(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.body_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn cross_section_surface_parameters(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<SurfaceParameterRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        cross_sections(ctx, sections)?,
        |bytes| surface::cross_section_parameter_records(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.body_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn surface_contours(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<SurfaceContourRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| surface::contour_records(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.envelope_offset += base;
            record.surface_row_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn cross_section_surface_contours(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<SurfaceContourRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        cross_sections(ctx, sections)?,
        |bytes| surface::cross_section_contour_records(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.envelope_offset += base;
            record.surface_row_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn loop_array_scan(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<LoopArrayScan, CodecError> {
    let mut frames = Vec::new();
    let mut records = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        let payload = section.region;
        let scan = loop_array::scan(ctx, payload)?;
        ctx.reserve_vec(
            &mut frames,
            scan.frames.len(),
            "creo loop array aggregate frames",
        )?;
        frames.extend(
            ctx.admit_iter(scan.frames, "creo loop frame relocation traversal")?
                .map(|mut frame| {
                    frame.offset += section.section.offset();
                    frame.prototype_end += section.section.offset();
                    frame.end += section.section.offset();
                    frame
                }),
        );
        ctx.reserve_vec(
            &mut records,
            scan.records.len(),
            "creo loop array aggregate records",
        )?;
        records.extend(
            ctx.admit_iter(scan.records, "creo loop record relocation traversal")?
                .map(|mut record| {
                    record.frame_offset += section.section.offset();
                    record.offset += section.section.offset();
                    record.body_offset += section.section.offset();
                    record
                }),
        );
    }
    if frames.len() > 1 {
        ctx.stable_sort_by(
            frames.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo loop array scan frames ordering",
        )?;
    }
    if records.len() > 1 {
        ctx.stable_sort_by(
            records.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo loop array scan records ordering",
        )?;
    }
    Ok(LoopArrayScan { frames, records })
}

fn tabulated_cylinder_curve_replays(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<TabulatedCylinderCurveReplay>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| surface::tabulated_cylinder_curve_replays(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.surface_row_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn plane_local_systems(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<PlaneLocalSystem>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| surface::plane_local_systems(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.row_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn cross_section_plane_local_systems(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<PlaneLocalSystem>, CodecError> {
    collect_section_records_result(
        ctx,
        cross_sections(ctx, sections)?,
        |bytes| surface::cross_section_plane_local_systems(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.row_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn plane_envelopes(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<PlaneEnvelopeRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| surface::plane_envelopes(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.row_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn cross_section_plane_envelopes(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<PlaneEnvelopeRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        cross_sections(ctx, sections)?,
        |bytes| surface::cross_section_plane_envelopes(ctx, bytes),
        |record, base| {
            record.offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn curve_prototypes(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<CurvePrototype>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| curve::prototypes(ctx, bytes),
        |prototype, base| {
            prototype.offset += base;
            Ok(())
        },
        |prototype| prototype.offset,
    )
}

fn curve_expressions(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    model_name: Option<&str>,
) -> Result<Vec<CurveExpressionRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| curve::expression_records_with_model_name(ctx, bytes, model_name),
        |record, base| {
            record.offset += base;
            record.expression_offset += base;
            for line in
                ctx.admit_iter(&mut record.lines, "creo record child relocation traversal")?
            {
                line.offset += base;
            }
            for assignment in ctx.admit_iter(
                &mut record.assignments,
                "creo record child relocation traversal",
            )? {
                assignment.offset += base;
            }
            for block in ctx.admit_iter(
                &mut record.solve_blocks,
                "creo record child relocation traversal",
            )? {
                block.offset += base;
                block.for_offset += base;
                for equation in ctx.admit_iter(
                    &mut block.equations,
                    "creo record child relocation traversal",
                )? {
                    equation.offset += base;
                }
                for assignment in ctx.admit_iter(
                    &mut block.assignments,
                    "creo record child relocation traversal",
                )? {
                    assignment.offset += base;
                }
            }
            Ok(())
        },
        |record| record.offset,
    )
}

fn curve_parameters(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    face_ids: &BTreeSet<u32>,
) -> Result<Vec<CurveParameterRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| curve::parameter_records_with_face_ids(ctx, bytes, Some(face_ids)),
        |record, base| {
            record.offset += base;
            record.body_offset += base;
            record.suffix_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn two_chart_pcurves(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    face_ids: &BTreeSet<u32>,
) -> Result<Vec<TwoChartPcurveSamples>, CodecError> {
    let mut records = collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| curve::two_chart_pcurve_samples(ctx, bytes, Some(face_ids)),
        |record, base| {
            record.offset += base;
            Ok(())
        },
        |record| record.offset,
    )?;
    let mut count_storage = ctx.reserve_scoped(0, "creo two-chart pcurve count storage")?;
    let mut counts = std::collections::HashMap::new();
    for record in ctx.admit_iter(&records, "creo two-chart pcurve count traversal")? {
        let count = count_storage
            .with_storage(|| {
                ctx.entry_hash_map(&mut counts, record.curve_id, "creo two-chart pcurve counts")
            })?
            .or_insert(0usize);
        *count += 1;
    }
    ctx.retain_vec(
        &mut records,
        |record| Ok(counts.get(&record.curve_id) == Some(&1)),
        "creo aggregate pcurve retain",
    )?;
    Ok(records)
}

fn prototype_pcurves(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<PrototypePcurveEndpoints>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| curve::prototype_pcurve_endpoints(ctx, bytes),
        |record, base| {
            record.offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn curve_prototype_topology(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<CurvePrototypeTopology>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| curve::prototype_topology(ctx, bytes),
        |record, base| {
            record.offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn curve_topology_rows(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    face_ids: &BTreeSet<u32>,
) -> Result<Vec<CurveTopologyRow>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?.map(Ok),
        |bytes| curve::topology_rows_with_face_ids(ctx, bytes, Some(face_ids)),
        |row, base| {
            row.offset += base;
            Ok(())
        },
        |row| row.offset,
    )
}

fn cross_section_curve_rows(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<DepdbCurveRow>, CodecError> {
    collect_section_records_result(
        ctx,
        cross_sections(ctx, sections)?,
        |bytes| curve::depdb_cross_section_rows(ctx, bytes),
        |record, base| {
            record.offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn cross_section_curve_prototypes(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<CurvePrototype>, CodecError> {
    collect_section_records_result(
        ctx,
        cross_sections(ctx, sections)?,
        |bytes| curve::prototypes(ctx, bytes),
        |record, base| {
            record.offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn datum_planes(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<DatumPlaneRecord>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?
            .filter(|section| section.section.name() == "ActDatums")
            .map(Ok),
        |bytes| {
            let mut planes = datum::planes(ctx, bytes)?;
            if let Some(plane) = datum::named_plane(ctx, bytes)? {
                ctx.reserve_vec(&mut planes, 1, "creo named datum plane aggregation")?;
                planes.push(plane);
            }
            Ok(planes)
        },
        |plane, base| {
            plane.offset_in_payload += base;
            Ok(())
        },
        |plane| plane.offset_in_payload,
    )
}

fn datum_cylinders(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<DatumCylinder>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?
            .filter(|section| section.section.name() == "ActDatums")
            .map(Ok),
        |bytes| datum::cylinders(ctx, bytes),
        |cylinder, base| {
            cylinder.offset_in_payload += base;
            Ok(())
        },
        |cylinder| cylinder.offset_in_payload,
    )
}

fn structural_feature_ids(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    surface_rows: &[SurfaceRow],
    curve_rows: &[CurveTopologyRow],
) -> Result<std::collections::BTreeSet<u32>, CodecError> {
    let mut ids = std::collections::BTreeSet::new();
    for id in ctx
        .admit_iter(surface_rows, "creo structural surface traversal")?
        .map(|row| row.feature_id)
        .chain(
            ctx.admit_iter(curve_rows, "creo structural curve traversal")?
                .map(|row| row.feature_id),
        )
        .filter(|id| *id != 0)
    {
        ctx.insert_btree_set(&mut ids, id, "creo structural feature ids")?;
    }
    for section in ctx.admit_iter(sections, "creo structural feature sections")? {
        if !(section.section.role() == SectionRole::PsbGeometry) {
            continue;
        }
        let payload = section.region;
        let mut from = 0;
        while let Some(found) = ctx.find_bytes_from(
            payload,
            b"parent_feats\0",
            from,
            "creo parent-feature search",
        )? {
            let start = found + b"parent_feats\0".len();
            let Some(&psb::token::ARRAY_OPEN) = payload.get(start) else {
                from = start;
                continue;
            };
            let (count, mut cursor) = psb::complete_compact_int(payload, start + 1)
                .ok_or_else(|| CodecError::malformed("incomplete parent-feature count"))?;
            let mut entries = 0..count;
            while !entries.is_empty() {
                if cursor >= payload.len() {
                    return Err(CodecError::malformed("incomplete parent-feature entry"));
                }
                let Some(_) =
                    ctx.next_charged(&mut entries, "creo parent-feature entries")?
                else {
                    break;
                };
                let (id, next) = psb::complete_compact_int(payload, cursor)
                    .ok_or_else(|| CodecError::malformed("incomplete parent-feature entry"))?;
                if id != 0 {
                    ctx.insert_btree_set(&mut ids, id, "creo structural feature ids")?;
                }
                cursor = next;
            }
            from = start;
        }
    }
    Ok(ids)
}

/// Consume source IDs admitted by the caller before its adapters.
fn topology_face_ids(
    ctx: &DecodeContext<'_>,
    ids: impl IntoIterator<Item = u32>,
) -> Result<BTreeSet<u32>, CodecError> {
    let mut faces = BTreeSet::new();
    for id in ids {
        ctx.insert_btree_set(&mut faces, id, "creo topology face ids")?;
    }
    Ok(faces)
}

/// Copy structural IDs and consume additions admitted by the caller.
fn candidate_feature_ids(
    ctx: &DecodeContext<'_>,
    structural: &BTreeSet<u32>,
    additions: impl IntoIterator<Item = u32>,
) -> Result<BTreeSet<u32>, CodecError> {
    let mut ids = BTreeSet::new();
    for id in ctx.admit_iter(structural, "creo structural feature ID traversal")? {
        ctx.insert_btree_set(&mut ids, *id, "creo candidate structural feature ids")?;
    }
    for id in additions {
        ctx.insert_btree_set(&mut ids, id, "creo candidate feature ids")?;
    }
    Ok(ids)
}

/// Consume admitted additions and retain the ordered output IDs.
fn complete_feature_ids(
    ctx: &DecodeContext<'_>,
    structural: BTreeSet<u32>,
    additions: impl IntoIterator<Item = u32>,
) -> Result<Vec<u32>, CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "creo complete feature index storage")?;
    let mut structural = structural;
    for id in additions {
        index_storage.with_storage(|| {
            ctx.insert_btree_set(&mut structural, id, "creo complete feature ids")
        })?;
    }
    let mut ordered = Vec::new();
    ctx.reserve_vec(&mut ordered, structural.len(), "creo ordered feature ids")?;
    ordered.extend(ctx.admit_iter(structural, "creo ordered feature ID traversal")?);
    Ok(ordered)
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

struct FeatureIdentityIndex<'ctx> {
    operation_classes: std::collections::HashSet<(u32, Option<u32>)>,
    known_operation_ids: std::collections::HashSet<u32>,
    reference_kinds: std::collections::HashMap<u32, u8>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> FeatureIdentityIndex<'ctx> {
    const SECTION: u8 = 1;
    const DATUM: u8 = 2;
    const COORDINATE_SYSTEM: u8 = 4;

    fn new(
        ctx: &'ctx DecodeContext<'_>,
        rows: &[FeatureRow],
        structural: &BTreeSet<u32>,
        operations: &[FeatureOperation],
        reference_names: &[FeatureReferenceName],
    ) -> Result<Self, CodecError> {
        let storage = ctx.reserve_scoped(0, "creo feature identity index storage")?;
        let mut index = Self {
            operation_classes: std::collections::HashSet::new(),
            known_operation_ids: std::collections::HashSet::new(),
            reference_kinds: std::collections::HashMap::new(),
            storage,
        };
        if rows.is_empty() {
            return Ok(index);
        }
        let mut owner_storage =
            ctx.reserve_scoped(0, "creo operation identity owner storage")?;
        let mut owners = std::collections::HashSet::new();
        owner_storage.with_storage(|| {
            for row in ctx.admit_iter(rows, "creo operation identity row selection")? {
                if !ctx.contains_btree_set(
                    structural,
                    &row.feature_id,
                    "creo structural model identity lookup",
                )? {
                    ctx.insert_hash_set(
                        &mut owners,
                        row.feature_id,
                        "creo operation identity owner selection",
                    )?;
                }
            }
            Ok::<(), CodecError>(())
        })?;
        if owners.is_empty() {
            return Ok(index);
        }
        index.storage.with_storage(|| {
            for operation in ctx.admit_iter(operations, "creo operation identity indexing")? {
                if !owners.contains(&operation.feature_id) {
                    continue;
                }
                let class = stored_operation_schema_class(operation)
                    .map(feature::schema::SchemaClass::code);
                ctx.insert_hash_set(
                    &mut index.operation_classes,
                    (operation.feature_id, class),
                    "creo operation identity classes",
                )?;
                if class.is_some() {
                    ctx.insert_hash_set(
                        &mut index.known_operation_ids,
                        operation.feature_id,
                        "creo known operation identities",
                    )?;
                }
            }
            Ok::<(), CodecError>(())
        })?;
        let mut needed_storage =
            ctx.reserve_scoped(0, "creo reference identity selection storage")?;
        let mut needed = std::collections::HashMap::<u32, u8>::new();
        needed_storage.with_storage(|| {
            for row in ctx.admit_iter(rows, "creo reference identity row selection")? {
                if owners.contains(&row.feature_id)
                    && !index.has_operation(row)
                    && matches!(
                        row.root_schema_class,
                        Some(
                            crate::feature::schema::SchemaClass::Section
                                | crate::feature::schema::SchemaClass::DatumPlane
                                | crate::feature::schema::SchemaClass::CoordinateSystem
                        )
                    )
                {
                    let kind = match row.root_schema_class {
                        Some(crate::feature::schema::SchemaClass::Section) => Self::SECTION,
                        Some(crate::feature::schema::SchemaClass::DatumPlane) => Self::DATUM,
                        Some(crate::feature::schema::SchemaClass::CoordinateSystem) => {
                            Self::COORDINATE_SYSTEM
                        }
                        _ => continue,
                    };
                    let mask = ctx
                        .entry_hash_map(
                            &mut needed,
                            row.feature_id,
                            "creo reference identity owner selection",
                        )?
                        .or_default();
                    *mask |= kind;
                }
            }
            Ok::<(), CodecError>(())
        })?;
        drop((owners, owner_storage));
        if needed.is_empty() {
            return Ok(index);
        }
        index.storage.with_storage(|| {
            for reference in
                ctx.admit_iter(reference_names, "creo feature identity reference scan")?
            {
                let Some(&requested) = needed.get(&reference.feature_id) else {
                    continue;
                };
                let Ok(name) = ctx.validate_utf8(&reference.name_bytes, "creo UTF-8 validation")?
                else {
                    continue;
                };
                let numbered_family = |family: &str| -> Result<bool, CodecError> {
                    let Some(suffix) = name.strip_prefix(family) else {
                        return Ok(false);
                    };
                    let Some(digits) = suffix
                        .strip_prefix(" id ")
                        .or_else(|| suffix.strip_prefix(" ID "))
                    else {
                        return Ok(false);
                    };
                    Ok(ctx
                        .parse_text::<u32>(digits, "creo scalar text parsing")?
                        .ok()
                        == Some(reference.feature_id))
                };
                let datum = requested & Self::DATUM != 0
                    && (matches!(name, "Datum Plane" | "Bezugsebene")
                        || numbered_family("Datum Plane")?
                        || numbered_family("Bezugsebene")?
                        || match name.strip_prefix("DTM") {
                            Some(ordinal) => {
                                !ordinal.is_empty()
                                    && ctx.all_by(
                                        ordinal.bytes(),
                                        |byte| Ok(byte.is_ascii_digit()),
                                        "creo datum ordinal validation",
                                    )?
                            }
                            None => false,
                        });
                let kinds = requested & Self::SECTION
                    | if datum { Self::DATUM } else { 0 }
                    | if requested & Self::COORDINATE_SYSTEM != 0 && name == "PRT_CSYS_DEF" {
                        Self::COORDINATE_SYSTEM
                    } else {
                        0
                    };
                if kinds == 0 {
                    continue;
                }
                let previous = ctx
                    .entry_hash_map(
                        &mut index.reference_kinds,
                        reference.feature_id,
                        "creo reference identity kinds",
                    )?
                    .or_default();
                *previous |= kinds;
            }
            Ok::<(), CodecError>(())
        })?;
        Ok(index)
    }

    fn has_operation(&self, row: &FeatureRow) -> bool {
        self.operation_classes.contains(&(
            row.feature_id,
            row.root_schema_class
                .map(feature::schema::SchemaClass::code),
        )) || (row
            .root_schema_class
            .is_some_and(|class| !registered_feature_schema_class(class))
            && self.known_operation_ids.contains(&row.feature_id))
    }

    fn contains(
        &self,
        ctx: &DecodeContext<'_>,
        row: &FeatureRow,
        structural: &BTreeSet<u32>,
    ) -> Result<bool, CodecError> {
        use crate::feature::schema::SchemaClass;
        if ctx.contains_btree_set(
            structural,
            &row.feature_id,
            "creo structural model identity lookup",
        )? || self.has_operation(row)
        {
            return Ok(true);
        }
        let kind = match row.root_schema_class {
            Some(SchemaClass::Section) => Self::SECTION,
            Some(SchemaClass::DatumPlane) => Self::DATUM,
            Some(SchemaClass::CoordinateSystem) => Self::COORDINATE_SYSTEM,
            _ => return Ok(false),
        };
        Ok(self
            .reference_kinds
            .get(&row.feature_id)
            .is_some_and(|kinds| kinds & kind != 0))
    }
}

#[cfg(test)]
fn feature_row_has_model_identity(
    ctx: &DecodeContext<'_>,
    row: &FeatureRow,
    structural: &BTreeSet<u32>,
    operations: &[FeatureOperation],
    reference_names: &[FeatureReferenceName],
) -> Result<bool, CodecError> {
    FeatureIdentityIndex::new(
        ctx,
        std::slice::from_ref(row),
        structural,
        operations,
        reference_names,
    )?
    .contains(ctx, row, structural)
}

fn feature_entity_tables(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    feature_ids: &[u32],
    rows: &[SurfaceRow],
) -> Result<Vec<FeatureEntityTable>, CodecError> {
    let mut identity_storage =
        ctx.reserve_scoped(0, "creo feature entity admission index storage")?;
    let mut feature_ids_set = BTreeSet::new();
    let mut surface_ids = BTreeSet::new();
    identity_storage.with_storage(|| {
        for &feature_id in ctx.admit_iter(feature_ids, "creo feature ID traversal")? {
            ctx.insert_btree_set(
                &mut feature_ids_set,
                feature_id,
                "creo feature entity owner ids",
            )?;
        }
        for row in ctx.admit_iter(rows, "creo container row traversal")? {
            ctx.insert_btree_set(&mut surface_ids, row.id, "creo feature entity surface ids")?;
        }
        Ok::<(), CodecError>(())
    })?;
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?
            .filter(|section| section.section.name() == "AllFeatur")
            .map(Ok),
        |bytes| feature::entity::entity_tables(ctx, bytes, &feature_ids_set, &surface_ids),
        |table, base| {
            table.offset += base;
            table.entries.relocate_offsets(ctx, base)
        },
        |table| table.offset,
    )
}

fn feature_rows(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    feature_ids: &BTreeSet<u32>,
) -> Result<Vec<FeatureRow>, CodecError> {
    let mut rows = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        if section.section.name() != "AllFeatur" {
            continue;
        }
        let section_bytes = section.region;
        let decoded =
            feature::rows::rows(ctx, section_bytes, feature_ids, section.section.offset())?;
        ctx.reserve_vec(&mut rows, decoded.len(), "creo feature row aggregation")?;
        rows.extend(ctx.admit_iter(decoded, "creo feature row aggregation traversal")?);
    }
    if rows.len() > 1 {
        ctx.stable_sort_by(
            rows.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo feature rows rows ordering",
        )?;
    }
    Ok(rows)
}

fn feature_entity_graph(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<(Vec<FeatureEntity>, Vec<FeatureEntityReference>), CodecError> {
    let Some(section) = ctx.find_by(
        sections,
        |section| Ok(section.section.name() == "AllFeatur"),
        "creo named section selection",
    )?
    else {
        return Ok((Vec::new(), Vec::new()));
    };
    let section_bytes = section.region;
    // The payload follows the `#<name>\n` section header. A section without
    // that newline carries no header, so the whole region is the payload.
    let header_length = ctx
        .find_bytes_from(section_bytes, b"\n", 0, "find Creo container marker")?
        .map_or(0, |newline| newline + 1);
    let payload_start = section.section.offset() + header_length;
    let (mut entities, mut references) =
        feature::entity::entity_graph(ctx, &section_bytes[header_length..])?;
    for entity in ctx.admit_iter(&mut entities, "creo entity graph relocation traversal")? {
        entity.offset += payload_start;
    }
    for reference in ctx.admit_iter(&mut references, "creo reference graph relocation traversal")? {
        reference.offset += payload_start;
    }
    Ok((entities, references))
}

fn offset_feature_definition(
    ctx: &DecodeContext<'_>,
    definition: &mut FeatureDefinition,
    section_offset: usize,
) -> Result<(), CodecError> {
    definition.offset += section_offset;
    for frame in ctx.admit_iter(
        &mut definition.parameter_frames,
        "creo feature definition relocation traversal",
    )? {
        frame.offset += section_offset;
    }
    for outline in ctx.admit_iter(
        &mut definition.outlines,
        "creo feature definition relocation traversal",
    )? {
        outline.offset += section_offset;
    }
    if let Some(variables) = &mut definition.variables {
        variables.offset += section_offset;
        for row in ctx.admit_iter(
            &mut variables.rows,
            "creo feature definition relocation traversal",
        )? {
            row.offset += section_offset;
        }
    }
    if let Some(segments) = &mut definition.segments {
        segments.offset += section_offset;
        segments.rows.add_offset(ctx, section_offset)?;
    }
    if let Some(entities) = &mut definition.trim_entities {
        entities.offset += section_offset;
        for row in ctx.admit_iter(
            &mut entities.rows,
            "creo feature definition relocation traversal",
        )? {
            row.offset += section_offset;
        }
    }
    if let Some(vertices) = &mut definition.trim_vertices {
        vertices.offset += section_offset;
        for row in ctx.admit_iter(
            &mut vertices.rows,
            "creo feature definition relocation traversal",
        )? {
            row.offset += section_offset;
        }
    }
    if let Some(order) = &mut definition.order_table {
        order.offset += section_offset;
        order.rows.add_offset(ctx, section_offset)?;
    }
    if let Some(section_3d) = &mut definition.section_3d {
        section_3d.offset += section_offset;
    }
    if let Some(dimensions) = &mut definition.dimensions {
        dimensions.offset += section_offset;
        for row in ctx.admit_iter(
            &mut dimensions.rows,
            "creo feature definition relocation traversal",
        )? {
            row.offset += section_offset;
            if let Some(references) = &mut row.references {
                references.offset += section_offset;
                for reference in ctx.admit_iter(
                    &mut references.rows,
                    "creo feature definition relocation traversal",
                )? {
                    reference.offset += section_offset;
                }
            }
        }
    }
    if let Some(relations) = &mut definition.relations {
        relations.offset += section_offset;
        for row in ctx.admit_iter(
            &mut relations.rows,
            "creo feature definition relocation traversal",
        )? {
            row.offset += section_offset;
        }
        if let Some(table) = &mut relations.skamps {
            table.shift_offsets(ctx, section_offset)?;
        }
        if let Some(table) = &mut relations.triples {
            table.shift_offsets(ctx, section_offset)?;
        }
    }
    if let Some(saved) = &mut definition.saved_section {
        saved.offset += section_offset;
        for entity in ctx.admit_iter(
            &mut saved.entities,
            "creo feature definition relocation traversal",
        )? {
            match entity {
                feature::definitions::FeatureSavedEntity::Line(line) => {
                    line.offset += section_offset;
                }
                feature::definitions::FeatureSavedEntity::Arc(arc) => arc.offset += section_offset,
                feature::definitions::FeatureSavedEntity::Circle(circle) => {
                    circle.offset += section_offset;
                }
                feature::definitions::FeatureSavedEntity::Conic(conic) => {
                    conic.offset += section_offset;
                }
                feature::definitions::FeatureSavedEntity::Spline(spline) => {
                    spline.offset += section_offset;
                }
                feature::definitions::FeatureSavedEntity::Dummy(dummy) => {
                    dummy.offset += section_offset;
                }
            }
        }
    }
    Ok(())
}

fn feature_definitions(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut definitions = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        if !(section.section.name() == "FeatDefs" || section.section.name() == "DEPDB_DATA") {
            continue;
        }
        let payload = section.region;
        let decoded = if section.section.name() == "DEPDB_DATA" {
            feature::definitions::depdb_definitions(ctx, payload)?
        } else {
            feature::definitions::definitions(ctx, payload)?
        };
        ctx.reserve_vec(&mut definitions, decoded.len(), "creo feature definitions")?;
        for mut definition in
            ctx.admit_iter(decoded, "creo definition aggregate relocation traversal")?
        {
            offset_feature_definition(ctx, &mut definition, section.section.offset())?;
            definitions.push(definition);
        }
        if section.section.name() == "DEPDB_DATA" {
            let owner_feature_id = feature::operations::unique_recipe_owner(ctx, payload)?;
            if let Some(owner_feature_id) = owner_feature_id {
                if let Some(mut definition) = feature::definitions::depdb_section_definition(
                    ctx,
                    payload,
                    Some(owner_feature_id),
                )? {
                    offset_feature_definition(ctx, &mut definition, section.section.offset())?;
                    if let Some(position) = ctx.position_by(
                        &definitions,
                        |existing| Ok(existing.offset == definition.offset),
                        "creo definition aggregate replacement search",
                    )? {
                        definitions[position] = definition;
                    } else {
                        ctx.reserve_vec(&mut definitions, 1, "creo feature definitions")?;
                        definitions.push(definition);
                    }
                }
            }
        }
    }
    if definitions.len() > 1 {
        ctx.stable_sort_by(
            definitions.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo feature definitions definitions ordering",
        )?;
    }
    Ok(definitions)
}

fn feature_row_definitions(
    ctx: &DecodeContext<'_>,
    rows: &[FeatureRow],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut definitions = Vec::new();
    for row in ctx.admit_iter(rows, "creo container row traversal")? {
        let Some(mut definition) =
            feature::definitions::depdb_section_definition(ctx, &row.body, None)?
        else {
            continue;
        };
        offset_feature_definition(ctx, &mut definition, row.body_offset)?;
        ctx.reserve_vec(&mut definitions, 1, "creo feature row definitions")?;
        definitions.push(definition);
    }
    if definitions.len() > 1 {
        ctx.stable_sort_by(
            definitions.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo feature row definitions definitions ordering",
        )?;
    }
    Ok(definitions)
}

fn claimed_definition_owners(
    ctx: &DecodeContext<'_>,
    definitions: &[FeatureDefinition],
) -> Result<BTreeSet<u32>, CodecError> {
    let mut owners = BTreeSet::new();
    for id in ctx
        .admit_iter(definitions, "creo claimed definition traversal")?
        .filter_map(|definition| definition.identity.owner_feature_id())
    {
        ctx.insert_btree_set(&mut owners, id, "creo claimed definition owners")?;
    }
    Ok(owners)
}

fn feature_geometry_tables(
    ctx: &DecodeContext<'_>,
    rows: &[FeatureRow],
    depdb_rows: &[FeatureRow],
) -> Result<Vec<FeatureGeometryTable>, CodecError> {
    let mut tables = feature::rows::geometry_tables(ctx, rows)?;
    let depdb_tables = feature::rows::geometry_tables(ctx, depdb_rows)?;
    ctx.reserve_vec(
        &mut tables,
        depdb_tables.len(),
        "creo feature geometry table aggregation",
    )?;
    tables.extend(ctx.admit_iter(
        depdb_tables,
        "creo feature geometry table aggregation traversal",
    )?);
    if tables.len() > 1 {
        ctx.stable_sort_by(
            tables.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo feature geometry tables tables ordering",
        )?;
    }
    Ok(tables)
}

fn feature_affected_ids(
    ctx: &DecodeContext<'_>,
    rows: &[FeatureRow],
    depdb_rows: &[FeatureRow],
) -> Result<Vec<FeatureAffectedIds>, CodecError> {
    let mut records = feature::rows::affected_ids(ctx, rows)?;
    let depdb_records = feature::rows::affected_ids(ctx, depdb_rows)?;
    ctx.reserve_vec(
        &mut records,
        depdb_records.len(),
        "creo affected-id aggregation",
    )?;
    records.extend(ctx.admit_iter(depdb_records, "creo affected-id aggregation traversal")?);
    if records.len() > 1 {
        ctx.stable_sort_by(
            records.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo feature affected ids records ordering",
        )?;
    }
    Ok(records)
}

fn feature_revolution_extents(
    ctx: &DecodeContext<'_>,
    rows: &[FeatureRow],
    definitions: &[FeatureDefinition],
    operations: &[FeatureOperation],
) -> Result<Vec<FeatureRevolutionExtent>, CodecError> {
    let mut extents = feature::rows::revolution_extents(ctx, rows)?;
    let definition_extents =
        feature::definitions::definition_revolution_extents(ctx, definitions, operations)?;
    ctx.reserve_vec(
        &mut extents,
        definition_extents.len(),
        "creo revolution extent aggregation",
    )?;
    extents.extend(ctx.admit_iter(
        definition_extents,
        "creo revolution extent aggregation traversal",
    )?);
    if extents.len() > 1 {
        ctx.stable_sort_by(
            extents.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo feature revolution extents extents ordering",
        )?;
    }
    Ok(extents)
}

fn section_owner_ranges(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    feature_rows: &[FeatureRow],
) -> Result<Vec<(usize, usize)>, CodecError> {
    let mut ranges = Vec::new();
    for section in ctx.admit_iter(sections, "creo section owner traversal")? {
        if section.section.name() == "DEPDB_DATA" {
            ctx.reserve_vec(&mut ranges, 1, "creo section owner ranges")?;
            ranges.push((section.section.offset(), section.section.end()));
        }
    }
    ctx.reserve_vec(&mut ranges, feature_rows.len(), "creo section owner ranges")?;
    for row in ctx.admit_iter(feature_rows, "creo feature owner range traversal")? {
        let end = row
            .body_offset
            .checked_add(row.body.len())
            .ok_or_else(|| CodecError::malformed("feature owner range end exceeds usize"))?;
        ranges.push((row.body_offset, end));
    }
    Ok(ranges)
}

fn positional_replay_definitions(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?
            .filter(|section| section.section.name() == "FeatDefs")
            .map(Ok),
        |bytes| feature::definitions::positional_replay_definitions(ctx, bytes),
        |definition, base| offset_feature_definition(ctx, definition, base),
        |definition| definition.offset,
    )
}

fn feature_operations(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<FeatureOperation>, CodecError> {
    let records = collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?
            .filter(|section| matches!(section.section.name(), "MdlStatus" | "DEPDB_DATA"))
            .map(Ok),
        |bytes| feature::operations::operations(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.state_offset += base;
            Ok(())
        },
        |record| record.offset,
    )?;
    let mut node_storage = ctx.reserve_scoped(0, "creo current operation index storage")?;
    let mut by_feature = BTreeMap::new();
    for record in ctx.admit_iter(records, "creo operation aggregate traversal")? {
        node_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut by_feature,
                record.feature_id,
                record,
                "creo current feature operation nodes",
            )
        })?;
    }
    let mut current = Vec::new();
    ctx.reserve_vec(
        &mut current,
        by_feature.len(),
        "creo current feature operation order",
    )?;
    current.extend(
        ctx.admit_iter(by_feature, "creo current operation traversal")?
            .map(|(_, record)| record),
    );
    drop(node_storage);
    if current.len() > 1 {
        ctx.stable_sort_by(
            current.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo feature operations current ordering",
        )?;
    }
    Ok(current)
}

fn feature_reference_names(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<FeatureReferenceName>, CodecError> {
    let mut records = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        if section.section.name() != "MdlRefInfo" {
            continue;
        }
        let section_bytes = section.region;
        let decoded = feature::operations::reference_names(ctx, section_bytes)?;
        ctx.reserve_vec(&mut records, decoded.len(), "creo feature reference names")?;
        records.extend(
            ctx.admit_iter(decoded, "creo reference name relocation traversal")?
                .map(|mut record| {
                    record.offset += section.section.offset();
                    record
                }),
        );
    }
    Ok(records)
}

fn feature_operation_states(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<FeatureOperationState>, CodecError> {
    collect_section_records_result(
        ctx,
        ctx.admit_iter(sections, "creo section traversal")?
            .filter(|section| matches!(section.section.name(), "MdlStatus" | "DEPDB_DATA"))
            .map(Ok),
        |bytes| feature::operations::operation_states(ctx, bytes),
        |record, base| {
            record.offset += base;
            record.state_offset += base;
            Ok(())
        },
        |record| record.offset,
    )
}

fn depdb_recipe_rows(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<Vec<FeatureRow>, CodecError> {
    let mut rows = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        if section.section.name() != "DEPDB_DATA" {
            continue;
        }
        let payload = section.region;
        let mut body_start = 0;
        feature::operations::for_each_recipe_state(
            ctx,
            payload,
            |feature_id, root_schema_class, recipe, operation_offset| {
                let name = match recipe {
                    FeatureRecipe::ProtrudeExtrude => b"protextrude\0".as_slice(),
                    FeatureRecipe::CutExtrude => b"cutextrude\0",
                    FeatureRecipe::ProtrudeRevolve => b"protrevolve\0",
                    FeatureRecipe::CutRevolve => b"cutrevolve\0",
                };
                let Some(body_end) = ctx
                    .find_bytes_from(payload, name, operation_offset, "find Creo recipe end")?
                    .and_then(|offset| offset.checked_add(name.len()))
                else {
                    return Ok(());
                };
                let Some(body_bytes) = payload
                    .get(body_start..body_end)
                    .filter(|bytes| bytes.len() >= 2)
                else {
                    return Ok(());
                };
                let body = ctx
                    .copy_retained(body_bytes, "creo DEPDB recipe row body")?
                    .try_into()
                    .map_err(CodecError::malformed)?;
                ctx.reserve_vec(&mut rows, 1, "creo DEPDB recipe rows")?;
                rows.push(FeatureRow {
                    feature_id,
                    root_schema_class,
                    stream_offset: section.section.offset(),
                    body,
                    body_offset: section.section.offset() + body_start,
                    offset: section.section.offset() + operation_offset,
                });
                body_start = body_end;
                Ok(())
            },
        )?;
    }
    if rows.len() > 1 {
        ctx.stable_sort_by(
            rows.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo depdb recipe rows rows ordering",
        )?;
    }
    Ok(rows)
}

fn geomlists_value(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
    label: &[u8],
) -> Result<Option<u32>, CodecError> {
    let Some(section) = ctx.find_by(
        sections,
        |section| Ok(section.section.name() == "Geomlists"),
        "creo named section selection",
    )?
    else {
        return Ok(None);
    };
    let payload = section.region;
    let Some(offset) = ctx.find_bytes_from(payload, label, 0, "find Creo geometry-list value")?
    else {
        return Ok(None);
    };
    let value_offset = offset + label.len();
    let (count, after) = psb::compact_int(payload, value_offset);
    Ok((after > value_offset).then_some(count))
}

/// Read one scalar integer owned by the unique legacy `Sld_GeomDepend` root.
///
/// Legacy ASCII stores this metadata in the persistence object tree rather
/// than in a binary `Geomlists` section. Distinct complete values remain
/// unresolved; equal duplicate records are one value witness.
fn legacy_first_quilt_ptr(
    ctx: &DecodeContext<'_>,
    persistence: &legacy::Persistence,
) -> Result<Option<u32>, CodecError> {
    let mut parent_storage = ctx.reserve_scoped(0, "creo legacy geometry parent index storage")?;
    let mut parents = std::collections::HashMap::<usize, bool>::new();
    let mut parents_indexed = false;
    let mut selected = None;
    let mut records = persistence.integer_values.rows.iter();
    while records.len() != 0 {
        let Some(record) =
            ctx.next_charged(&mut records, "creo legacy geometry value traversal")?
        else {
            break;
        };
        if record.name != "first_quilt_ptr" {
            continue;
        }
        let Some(parent_id) = record.parent else {
            continue;
        };
        if !parents_indexed {
            parent_storage.with_storage(|| {
                for object in
                    ctx.admit_iter(&persistence.objects, "creo legacy geometry parent indexing")?
                {
                    ctx.entry_hash_map(
                        &mut parents,
                        object.offset,
                        "creo legacy geometry parent nodes",
                    )?
                    .or_insert(object.name == "Sld_GeomDepend");
                }
                Ok::<(), CodecError>(())
            })?;
            parents_indexed = true;
        }
        if parents.get(&parent_id) != Some(&true) {
            continue;
        }
        let legacy::NumericPayload::Scalar { value } = record.payload else {
            continue;
        };
        let Ok(value) = u32::try_from(value) else {
            continue;
        };
        match selected {
            Some(previous) if previous != value => return Ok(None),
            Some(_) => {}
            None => selected = Some(value),
        }
    }
    Ok(selected)
}

fn reference_scan(
    ctx: &DecodeContext<'_>,
    sections: &[ScannedSection<'_>],
) -> Result<ReferenceScan, CodecError> {
    let mut lines = Vec::new();
    let mut circles = Vec::new();
    let mut conics = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        if section.section.name() != "MdlRefInfo" {
            continue;
        }
        let payload = section.region;
        for mut line in ctx
            .admit_iter(
                reference::lines(ctx, payload)?,
                "creo reference line merge traversal",
            )?
            .chain(ctx.admit_iter(
                reference::line3d_lines(ctx, payload)?,
                "creo reference line3d merge traversal",
            )?)
        {
            ctx.reserve_vec(&mut lines, 1, "creo reference line aggregation")?;
            line.offset += section.section.offset();
            lines.push(line);
        }
        for mut circle in ctx.admit_iter(
            reference::arc_z_circles(ctx, payload)?,
            "creo reference circle merge traversal",
        )? {
            ctx.reserve_vec(&mut circles, 1, "creo reference circle aggregation")?;
            circle.offset += section.section.offset();
            circles.push(circle);
        }
        for mut conic in ctx
            .admit_iter(
                reference::named_conics(ctx, payload)?,
                "creo named conic merge traversal",
            )?
            .chain(ctx.admit_iter(
                reference::positional_conics(ctx, payload)?,
                "creo positional conic merge traversal",
            )?)
        {
            ctx.reserve_vec(&mut conics, 1, "creo reference conic aggregation")?;
            conic.offset += section.section.offset();
            conics.push(conic);
        }
    }
    let ellipses = reference::ellipse_carriers(ctx, &conics)?;
    Ok(ReferenceScan {
        lines,
        circles,
        conics,
        ellipses,
    })
}

/// Copy outline planes and append positional planes without a matching outline.
fn placement_outline_planes(
    ctx: &DecodeContext<'_>,
    outline_planes: &[surface::OutlinePlane],
    positional_frame_planes: &[surface::OutlinePlane],
) -> Result<Vec<surface::OutlinePlane>, CodecError> {
    let mut identity_storage = ctx.reserve_scoped(0, "creo outline plane identity storage")?;
    let mut ids = std::collections::HashSet::new();
    let mut result = Vec::new();
    ctx.reserve_vec(
        &mut result,
        outline_planes.len(),
        "creo placement outline plane copies",
    )?;
    for plane in ctx.admit_iter(outline_planes, "creo outline plane source traversal")? {
        identity_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut ids,
                plane.surface_id,
                "creo outline plane identity nodes",
            )
        })?;
        result.push(plane.clone());
    }
    for plane in ctx.admit_iter(
        positional_frame_planes,
        "creo positional plane source traversal",
    )? {
        if !ids.contains(&plane.surface_id) {
            ctx.reserve_vec(&mut result, 1, "creo positional placement plane copies")?;
            result.push(plane.clone());
        }
    }
    Ok(result)
}

/// Append a source admitted by the caller before its adapters.
fn append_topology_rows(
    ctx: &DecodeContext<'_>,
    rows: &mut Vec<curve::CurveTopologyRow>,
    additional: impl ExactSizeIterator<Item = curve::CurveTopologyRow>,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.reserve_vec(rows, additional.len(), operation)?;
    rows.extend(additional);
    if rows.len() > 1 {
        ctx.stable_sort_by(
            rows.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo append topology rows rows ordering",
        )?;
        ctx.dedup_by_key(
            rows,
            |row| Ok(row.offset),
            "creo append topology rows rows deduplication",
        )?;
    }
    Ok(())
}

fn append_legacy_curve_witnesses(
    ctx: &DecodeContext<'_>,
    topology_rows: &mut Vec<curve::CurveTopologyRow>,
    pcurves: &mut Vec<curve::PcurveEndpoints>,
    legacy_topology_rows: &[curve::CurveTopologyRow],
    legacy_pcurves: &[curve::PcurveEndpoints],
) -> Result<(), CodecError> {
    append_topology_rows(
        ctx,
        topology_rows,
        ctx.admit_iter(legacy_topology_rows, "creo topology row append traversal")?
            .cloned(),
        "creo legacy topology row aggregation",
    )?;
    ctx.reserve_vec(
        pcurves,
        legacy_pcurves.len(),
        "creo legacy pcurve aggregation",
    )?;
    pcurves.extend(
        ctx.admit_iter(legacy_pcurves, "creo legacy pcurve append traversal")?
            .cloned(),
    );
    if pcurves.len() > 1 {
        ctx.stable_sort_by(
            pcurves.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo append legacy curve witnesses pcurves ordering",
        )?;
        ctx.dedup_by_key(
            pcurves,
            |pcurve| Ok(pcurve.offset),
            "creo append legacy curve witnesses pcurves deduplication",
        )?;
    }
    Ok(())
}

/// Parse a whole `.prt` byte image.
pub(crate) fn scan_bytes<'a>(
    ctx: &DecodeContext<'_>,
    data: impl Into<Cow<'a, [u8]>>,
) -> Result<ContainerScan<'a>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let data = data.into();
    if !looks_like_creo(&data) {
        return Err(CodecError::WrongFormat(
            "missing Creo #UGC:2 signature".into(),
        ));
    }

    let version_line = line_at(ctx, &data, 0)?;
    let mut model_name =
        cmnm_model_name(ctx, &data)?.map(|(name, offset)| ModelName { name, offset });

    // The binary body begins after the ASCII header and TOC. Prefer the TOC end
    // marker; fall back to the header end; fall back to the magic line.
    let header_end =
        match ctx.find_bytes_from(&data, UGC_HEADER_END, 0, "creo container header scans")? {
            Some(offset) => ctx
                .find_bytes_from(&data, b"\n", offset, "creo container header scans")?
                .map(|newline| newline + 1),
            None => None,
        };
    let toc_end = match ctx.find_bytes_from(&data, TOC_START, 0, "creo container TOC scans")? {
        Some(offset) => {
            match ctx.find_bytes_from(&data, TOC_END, offset, "creo container TOC scans")? {
                Some(end) => ctx
                    .find_bytes_from(&data, b"\n", end, "creo container TOC scans")?
                    .map(|newline| newline + 1),
                None => None,
            }
        }
        None => None,
    };
    let body_start = toc_end.or(header_end).unwrap_or(0);

    let mut legacy_ascii = legacy_ascii_framing(ctx, &data)?;
    let mut section_storage = ctx.reserve_scoped(0, "creo scanned section roster storage")?;
    let sections = if let Some(legacy) = legacy_ascii.as_ref() {
        legacy_toc_sections(ctx, &mut section_storage, &data, legacy.banner_offset)?
    } else {
        toc_sections(ctx, &mut section_storage, &data, header_end.unwrap_or(0))?
    };
    let sections = if sections.is_empty() {
        drop(sections);
        drop(section_storage);
        section_storage = ctx.reserve_scoped(0, "creo scanned section roster storage")?;
        scan_sections(ctx, &mut section_storage, &data, body_start)?
    } else {
        sections
    };
    if let Some(framing) = &mut legacy_ascii {
        let scopes_owned_storage = ctx
            .with_scoped_storage("creo legacy scope range storage", || {
                legacy_scope_ranges(ctx, &data, framing, &sections)
            })?;
        let _scope_storage = scopes_owned_storage.1;
        let scopes = scopes_owned_storage.0;
        framing.persistence = legacy::scan(ctx, &data, scopes.iter().cloned())?;
    }
    if model_name.is_none() {
        if let Some((name, offset)) = legacy_ascii
            .as_ref()
            .map(|framing| framing.persistence.model_name(ctx))
            .transpose()?
            .flatten()
        {
            model_name = Some(ModelName { name, offset });
        }
    }
    if model_name.is_none() {
        if let Some((name, offset)) = legacy_ascii
            .as_ref()
            .map(|framing| framing.persistence.first_source_model_name(ctx))
            .transpose()?
            .flatten()
        {
            model_name = Some(ModelName { name, offset });
        }
    }
    let expanded_sections = expanded_sections(ctx, &data, &sections)?;
    let primitives = scan_primitives(ctx, &expanded_sections)?;
    let references = reference_scan(ctx, &sections)?;
    let layout = identify_layout(ctx, &sections, legacy_ascii)?;
    if model_name.is_none() && !matches!(layout, Layout::LegacyAscii(_)) {
        if let Some((name, offset)) = native_model_name(ctx, &sections)? {
            model_name = Some(ModelName { name, offset });
        }
    }
    let legacy_ascii = layout.legacy_ascii();
    let legacy_geometry = legacy_ascii
        .map(|framing| crate::legacy_geometry::scan(ctx, &framing.persistence))
        .transpose()?
        .unwrap_or_default();
    let legacy_rounds = legacy_ascii
        .map(|framing| {
            crate::legacy_feature::scan(ctx, &framing.persistence, &legacy_geometry.topology_rows)
        })
        .transpose()?
        .unwrap_or_default();
    let mut selection_storage = ctx.reserve_scoped(0, "Creo model section selection storage")?;
    let model_geometry_sections =
        selection_storage.with_storage(|| model_geometry_sections(ctx, &sections))?;
    let census = geom_census(ctx, &sections)?;
    let principal_unit = match binary_principal_unit(ctx, &data)? {
        BinaryUnitSelection::Selected(unit) => Some(unit),
        BinaryUnitSelection::Absent => legacy_ascii
            .map(|framing| framing.persistence.principal_unit_system(ctx))
            .transpose()?
            .flatten(),
        BinaryUnitSelection::Unsupported | BinaryUnitSelection::Conflicting => None,
    };
    let family_table = family_table(ctx, &sections)?;
    let legacy_family_table = legacy_ascii
        .map(|framing| crate::legacy_family::parse(ctx, &framing.persistence))
        .transpose()?
        .flatten();
    let nonvisible_geometry_sections =
        selection_storage.with_storage(|| nonvisible_geometry_sections(ctx, &sections))?;
    let mut loop_selection_storage =
        ctx.reserve_scoped(0, "creo loop array section selection storage")?;
    let loop_array_sections = loop_selection_storage.with_storage(|| {
        loop_array_sections(
            ctx,
            &model_geometry_sections,
            &nonvisible_geometry_sections,
            &sections,
        )
    })?;
    let loop_arrays = loop_array_scan(ctx, &loop_array_sections)?;
    drop(loop_array_sections);
    drop(loop_selection_storage);
    let mut nonvisible_surface_rows = surface_rows(ctx, &nonvisible_geometry_sections)?;
    ctx.reserve_vec(
        &mut nonvisible_surface_rows,
        legacy_geometry.nonvisible_rows.len(),
        "creo legacy nonvisible surface row aggregation",
    )?;
    nonvisible_surface_rows.extend(ctx.admit_iter(
        legacy_geometry.nonvisible_rows,
        "creo legacy nonvisible surface append traversal",
    )?);
    if nonvisible_surface_rows.len() > 1 {
        ctx.stable_sort_by(
            nonvisible_surface_rows.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo scan bytes nonvisible surface rows ordering",
        )?;
    }
    let mut surface_rows = surface_rows(ctx, &model_geometry_sections)?;
    ctx.reserve_vec(
        &mut surface_rows,
        legacy_geometry.rows.len(),
        "creo legacy surface row aggregation",
    )?;
    surface_rows
        .extend(ctx.admit_iter(legacy_geometry.rows, "creo legacy surface append traversal")?);
    if surface_rows.len() > 1 {
        ctx.stable_sort_by(
            surface_rows.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo scan bytes surface rows ordering",
        )?;
    }
    let cross_section_surface_rows = cross_section_surface_rows(ctx, &sections)?;
    let nonvisible_surface_parameters = surface_parameters(ctx, &nonvisible_geometry_sections)?;
    let surface_parameters = surface_parameters(ctx, &model_geometry_sections)?;
    let cross_section_surface_parameters = cross_section_surface_parameters(ctx, &sections)?;
    let nonvisible_surface_rows = surface::SurfaceRows::new(
        ctx,
        nonvisible_surface_rows,
        "creo nonvisible surface row index",
    )?;
    let surface_rows = surface::SurfaceRows::new(ctx, surface_rows, "creo surface row index")?;
    let cross_section_surface_rows = surface::SurfaceRows::new(
        ctx,
        cross_section_surface_rows,
        "creo cross-section surface row index",
    )?;
    let nonvisible_surface_parameters = surface::SurfaceParameters::new(
        ctx,
        nonvisible_surface_parameters,
        "creo nonvisible surface parameter index",
    )?;
    let surface_parameters =
        surface::SurfaceParameters::new(ctx, surface_parameters, "creo surface parameter index")?;
    let cross_section_surface_parameters = surface::SurfaceParameters::new(
        ctx,
        cross_section_surface_parameters,
        "creo cross-section surface parameter index",
    )?;
    let nonvisible_surface_contours = surface_contours(ctx, &nonvisible_geometry_sections)?;
    let surface_contours = surface_contours(ctx, &model_geometry_sections)?;
    let cross_section_surface_contours = cross_section_surface_contours(ctx, &sections)?;
    let tabulated_cylinder_curve_replays =
        tabulated_cylinder_curve_replays(ctx, &model_geometry_sections)?;
    let plane_local_systems = plane_local_systems(ctx, &model_geometry_sections)?;
    let cross_section_plane_local_systems = cross_section_plane_local_systems(ctx, &sections)?;
    let plane_envelopes = plane_envelopes(ctx, &model_geometry_sections)?;
    let cross_section_plane_envelopes = cross_section_plane_envelopes(ctx, &sections)?;
    let outline_planes =
        surface::placed_outline_planes(ctx, &plane_envelopes, &plane_local_systems)?;
    let positional_frame_planes =
        surface::positional_frame_planes(ctx, &surface_parameters, &surface_rows)?;
    let placement_outline_planes_owned_storage = ctx
        .with_scoped_storage("creo placement outline plane storage", || {
            placement_outline_planes(ctx, &outline_planes, &positional_frame_planes)
        })?;
    let placement_outline_storage = placement_outline_planes_owned_storage.1;
    let placement_outline_planes = placement_outline_planes_owned_storage.0;
    let cross_section_outline_planes = surface::placed_outline_planes(
        ctx,
        &cross_section_plane_envelopes,
        &cross_section_plane_local_systems,
    )?;
    let surface_prototype_count = surface_prototype_count(ctx, &model_geometry_sections)?;
    let mut nonvisible_prototype_refusals = crate::lane_refusal::LaneRefusals::new();
    let nonvisible_surface_prototype_records = surface_prototype_records(
        ctx,
        &nonvisible_geometry_sections,
        &mut nonvisible_prototype_refusals,
    )?;
    let mut prototype_refusals = crate::lane_refusal::LaneRefusals::new();
    let surface_prototype_records =
        surface_prototype_records(ctx, &model_geometry_sections, &mut prototype_refusals)?;
    let nonvisible_curve_prototypes = curve_prototypes(ctx, &nonvisible_geometry_sections)?;
    let curve_prototypes = curve_prototypes(ctx, &model_geometry_sections)?;
    let cross_section_curve_prototypes = cross_section_curve_prototypes(ctx, &sections)?;
    let mut curve_expressions = curve_expressions(
        ctx,
        &sections,
        match &model_name {
            Some(model) => relation_model_name(ctx, &model.name)?,
            None => None,
        },
    )?;
    let topology_face_ids_owned_storage =
        ctx.with_scoped_storage("creo topology face index storage", || {
            topology_face_ids(
                ctx,
                ctx.admit_iter(
                    &nonvisible_surface_rows[..],
                    "creo nonvisible topology face traversal",
                )?
                .chain(ctx.admit_iter(&surface_rows[..], "creo visible topology face traversal")?)
                .map(|row| row.id),
            )
        })?;
    let topology_face_storage = topology_face_ids_owned_storage.1;
    let topology_face_ids = topology_face_ids_owned_storage.0;
    let nonvisible_curve_parameters =
        curve_parameters(ctx, &nonvisible_geometry_sections, &topology_face_ids)?;
    let curve_parameters = curve_parameters(ctx, &model_geometry_sections, &topology_face_ids)?;
    let nonvisible_curve_topology_rows =
        curve_topology_rows(ctx, &nonvisible_geometry_sections, &topology_face_ids)?;
    let mut curve_topology_rows =
        curve_topology_rows(ctx, &model_geometry_sections, &topology_face_ids)?;
    let curve_prototype_topology = curve_prototype_topology(ctx, &model_geometry_sections)?;
    let prototype_topology_rows = curve::prototype_topology_rows(
        ctx,
        &curve_prototypes,
        &curve_prototype_topology,
        &curve_topology_rows,
        &topology_face_ids,
    )?;
    append_topology_rows(
        ctx,
        &mut curve_topology_rows,
        ctx.admit_iter(
            prototype_topology_rows,
            "creo topology row append traversal",
        )?,
        "creo prototype topology row aggregation",
    )?;
    let cross_section_curve_rows = cross_section_curve_rows(ctx, &sections)?;
    let mut pcurves = curve::pcurve_endpoints(ctx, &curve_parameters, &curve_topology_rows)?;
    let two_chart_pcurves = two_chart_pcurves(ctx, &model_geometry_sections, &topology_face_ids)?;
    drop((topology_face_ids, topology_face_storage));
    if matches!(layout, Layout::LegacyAscii(_)) {
        append_legacy_curve_witnesses(
            ctx,
            &mut curve_topology_rows,
            &mut pcurves,
            &legacy_geometry.topology_rows,
            &legacy_geometry.pcurves,
        )?;
    }
    let fc_curve_coordinates = curve::fc_coordinates(ctx, &curve_parameters)?;
    let fc05_circles = curve::fc05_circles(ctx, &curve_parameters)?;
    let fc05_cylinder_cap_pairs =
        curve::fc05_cylinder_cap_pairs(ctx, &fc05_circles, &curve_topology_rows, &surface_rows)?;
    let prototype_pcurves = prototype_pcurves(ctx, &model_geometry_sections)?;
    drop(model_geometry_sections);
    drop(nonvisible_geometry_sections);
    drop(selection_storage);
    let bound_prototype_pcurves =
        curve::bind_prototype_pcurves(ctx, &prototype_pcurves, &curve_prototype_topology)?;
    let (half_edges, loops) = topology::build(ctx, &curve_topology_rows)?;
    let vertex_orbits = topology::vertex_orbits(ctx, &half_edges)?;
    let face_components = topology::face_components(ctx, &curve_topology_rows)?;
    let datum_planes = datum_planes(ctx, &sections)?;
    let datum_cylinders = datum_cylinders(ctx, &sections)?;
    let feature_operation_states = feature_operation_states(ctx, &sections)?;
    let feature_operations = feature_operations(ctx, &sections)?;
    let feature_reference_names = feature_reference_names(ctx, &sections)?;
    let structural_feature_ids_owned_storage = ctx
        .with_scoped_storage("creo structural feature index storage", || {
            structural_feature_ids(ctx, &sections, &surface_rows, &curve_topology_rows)
        })?;
    let structural_feature_storage = structural_feature_ids_owned_storage.1;
    let structural_feature_ids = structural_feature_ids_owned_storage.0;
    let candidate_feature_ids_owned_storage =
        ctx.with_scoped_storage("creo candidate feature index storage", || {
            candidate_feature_ids(
                ctx,
                &structural_feature_ids,
                ctx.admit_iter(
                    &feature_operations,
                    "creo operation identity source traversal",
                )?
                .map(|operation| operation.feature_id)
                .chain(
                    ctx.admit_iter(
                        &feature_reference_names,
                        "creo reference identity source traversal",
                    )?
                    .map(|reference| reference.feature_id),
                ),
            )
        })?;
    let candidate_feature_storage = candidate_feature_ids_owned_storage.1;
    let candidate_feature_ids = candidate_feature_ids_owned_storage.0;
    let mut feature_rows = feature_rows(ctx, &sections, &candidate_feature_ids)?;
    drop((candidate_feature_ids, candidate_feature_storage));
    let feature_identity_index = FeatureIdentityIndex::new(
        ctx,
        &feature_rows,
        &structural_feature_ids,
        &feature_operations,
        &feature_reference_names,
    )?;
    ctx.retain_vec(
        &mut feature_rows,
        |row| feature_identity_index.contains(ctx, row, &structural_feature_ids),
        "creo feature identity row retention",
    )?;
    drop(feature_identity_index);
    let feature_ids = complete_feature_ids(
        ctx,
        structural_feature_ids,
        ctx.admit_iter(
            &feature_rows,
            "creo final feature identity source traversal",
        )?
        .map(|row| row.feature_id),
    )?;
    drop(structural_feature_storage);
    let feature_round_replay_scalars = feature::rows::round_replay_scalars(ctx, &feature_rows)?;
    let feature_choices = feature::rows::choices(ctx, &feature_rows)?;
    let feature_choice_fields = feature::rows::choice_fields(ctx, &feature_choices)?;
    let depdb_recipe_rows = depdb_recipe_rows(ctx, &sections)?;
    let feature_geometry_tables = feature_geometry_tables(ctx, &feature_rows, &depdb_recipe_rows)?;
    let feature_loop_history_entries =
        feature::rows::loop_history_entries(ctx, &feature_rows, &feature_geometry_tables)?;
    let feature_affected_ids = feature_affected_ids(ctx, &feature_rows, &depdb_recipe_rows)?;
    let feature_replay_affected_ids = feature::rows::replay_affected_ids(ctx, &feature_rows)?;
    let surface_merge_replay_affected_ids = feature::rows::surface_merge_replay_affected_ids(
        ctx,
        &feature_rows,
        &feature_affected_ids,
    )?;
    let feature_loop_restore_directions =
        feature::rows::loop_restore_directions(ctx, &feature_rows)?;
    let feature_entity_tables = feature_entity_tables(ctx, &sections, &feature_ids, &surface_rows)?;
    let feature_definitions = feature_definitions(ctx, &sections)?;
    let feature_definitions = feature::definitions::bind_definition_owners(
        ctx,
        feature_definitions,
        &feature_geometry_tables,
    )?;
    let mut feature_definitions = feature::definitions::bind_trimmed_definition_owners(
        ctx,
        feature_definitions,
        &feature_entity_tables,
    )?;
    ctx.extend_vec(
        &mut feature_definitions,
        feature_row_definitions(ctx, &feature_rows)?,
        "creo feature row definition aggregation",
    )?;
    if feature_definitions.len() > 1 {
        ctx.stable_sort_by(
            feature_definitions.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo scan bytes feature definitions ordering",
        )?;
    }
    let claimed_definition_owners_owned_storage = ctx
        .with_scoped_storage("creo claimed definition owner storage", || {
            claimed_definition_owners(ctx, &feature_definitions)
        })?;
    let claimed_owner_storage = claimed_definition_owners_owned_storage.1;
    let claimed_definition_owners = claimed_definition_owners_owned_storage.0;
    let replay_definitions = feature::definitions::bind_replay_definition_owners(
        ctx,
        positional_replay_definitions(ctx, &sections)?,
        &feature_entity_tables,
        &claimed_definition_owners,
    )?;
    drop((claimed_definition_owners, claimed_owner_storage));
    ctx.extend_vec(
        &mut feature_definitions,
        replay_definitions,
        "creo replay definition aggregation",
    )?;
    if feature_definitions.len() > 1 {
        ctx.stable_sort_by(
            feature_definitions.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo scan bytes feature definitions ordering",
        )?;
    }
    let section_owner_ranges_owned_storage = ctx
        .with_scoped_storage("creo section owner range storage", || {
            section_owner_ranges(ctx, &sections, &feature_rows)
        })?;
    let section_owner_storage = section_owner_ranges_owned_storage.1;
    let section_owner_ranges = section_owner_ranges_owned_storage.0;
    let feature_definitions = feature::definitions::bind_section_owners(
        ctx,
        feature_definitions,
        &feature_operations,
        &section_owner_ranges,
    )?;
    drop((section_owner_ranges, section_owner_storage));
    let mut relation_dimension_symbols = ExternalRelationSymbols::default();
    for definition in ctx.admit_iter(
        &feature_definitions,
        "creo relation dimension definition traversal",
    )? {
        let Some(table) = &definition.dimensions else {
            continue;
        };
        for dimension in ctx.admit_iter(&table.rows, "creo relation dimension row traversal")? {
            let value = dimension
                .value
                .resolved()
                .and_then(|value| match dimension.unit() {
                    feature::definitions::DimensionUnit::Radians => {
                        cadmpeg_ir::scalar::FiniteReal::new(value.to_degrees())
                            .map(CurveExpressionValue::Angle)
                    }
                    feature::definitions::DimensionUnit::Millimeters => {
                        cadmpeg_ir::scalar::FiniteReal::new(value).map(CurveExpressionValue::Length)
                    }
                    feature::definitions::DimensionUnit::SchemaDefined => {
                        cadmpeg_ir::scalar::FiniteReal::new(value).map(CurveExpressionValue::Number)
                    }
                });
            let (name, _reservation) = ctx.format_scoped(
                format_args!("d{}", dimension.external_id),
                "creo relation dimension symbol formatting",
            )?;
            relation_dimension_symbols.observe(ctx, name, value)?;
        }
    }
    curve::reevaluate_expression_records(
        ctx,
        &mut curve_expressions,
        match &model_name {
            Some(model) => relation_model_name(ctx, &model.name)?,
            None => None,
        },
        &relation_dimension_symbols,
    )?;
    let feature_revolution_extents = feature_revolution_extents(
        ctx,
        &feature_rows,
        &feature_definitions,
        &feature_operations,
    )?;
    let feature_section_transforms = placement::resolve(
        ctx,
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
    )?;
    drop((placement_outline_planes, placement_outline_storage));
    let (feature_entities, feature_entity_references) = feature_entity_graph(ctx, &sections)?;
    let declared_body_count = geomlists_value(ctx, &sections, b"n_bodies\0")?;
    let first_quilt_ptr = match geomlists_value(ctx, &sections, b"first_quilt_ptr\0")? {
        Some(value) => Some(value),
        None => legacy_ascii
            .map(|framing| legacy_first_quilt_ptr(ctx, &framing.persistence))
            .transpose()?
            .flatten(),
    };

    // The `Cow` takes the bytes here, so the scan's borrowed regions end and
    // the framing keeps the owned sections.
    let mut retained_sections = Vec::new();
    for section in ctx.admit_iter(sections, "creo section traversal")? {
        ctx.reserve_vec(&mut retained_sections, 1, "creo retained scan sections")?;
        retained_sections.push(section.section);
    }
    drop(section_storage);

    Ok(ContainerScan {
        framing: FramingScan {
            data,
            version_line,
            model_name,
            sections: retained_sections,
            expanded_sections,
            layout,
            census,
            principal_unit,
            family_table,
            legacy_family_table,
            declared_body_count,
            first_quilt_ptr,
        },
        primitives,
        references,
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
            prototype_field_refusals: prototype_refusals.take_records_checked()?,
            nonvisible_prototype_field_refusals: nonvisible_prototype_refusals
                .take_records_checked()?,
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

fn scan_primitives(
    ctx: &DecodeContext<'_>,
    expanded_sections: &[ExpandedSection],
) -> Result<PrimitiveScan, CodecError> {
    let mut double_xar_tables = Vec::new();
    let mut primitive_scalar_arrays = Vec::new();
    let mut primitive_triangle_strips = Vec::new();
    let mut conflicting_triangle_strip_representation_count = 0usize;
    let mut primitive_namespace_seen = false;
    for section in ctx.admit_iter(expanded_sections, "creo primitive section traversal")? {
        let tables = crate::scalar::double_xar_tables(ctx, &section.data)?;
        ctx.reserve_vec(
            &mut double_xar_tables,
            tables.len(),
            "creo model double_xar tables",
        )?;
        for table in ctx.admit_iter(tables, "creo primitive table aggregate traversal")? {
            double_xar_tables.push(ModelDoubleXarTable {
                section_name: ctx
                    .copy_retained_text(&section.name, "creo model double_xar section names")?,
                section_source_offset: section.source_offset,
                expanded_offset: table.offset,
                entries: table.entries,
            });
        }
        if section.name == "SolidPrimdata" {
            if primitive_namespace_seen {
                return Err(CodecError::malformed(
                    "duplicate SolidPrimdata identity namespace",
                ));
            }
            primitive_namespace_seen = true;
            let arrays = primdata::scalar_arrays(ctx, &section.data)?;
            ctx.reserve_vec(
                &mut primitive_scalar_arrays,
                arrays.len(),
                "creo model primitive scalar arrays",
            )?;
            primitive_scalar_arrays
                .extend(ctx.admit_iter(arrays, "creo primitive scalar append traversal")?);
            let scan = primdata::triangle_strips(ctx, &section.data)?;
            ctx.reserve_vec(
                &mut primitive_triangle_strips,
                scan.strips.len(),
                "creo model triangle strips",
            )?;
            primitive_triangle_strips
                .extend(ctx.admit_iter(scan.strips, "creo primitive strip append traversal")?);
            conflicting_triangle_strip_representation_count +=
                scan.conflicting_representation_count;
        }
    }
    Ok(PrimitiveScan {
        double_xar_tables,
        scalar_arrays: primitive_scalar_arrays,
        triangle_strips: primitive_triangle_strips,
        conflicting_triangle_strip_representation_count,
    })
}

fn cross_sections<'a, 'data, 'ctx>(
    ctx: &'a DecodeContext<'ctx>,
    sections: &'a [ScannedSection<'data>],
) -> Result<
    impl Iterator<Item = Result<&'a ScannedSection<'data>, CodecError>> + use<'a, 'data, 'ctx>,
    CodecError,
> {
    Ok(ctx
        .admit_iter(sections, "creo cross-section traversal")?
        .filter_map(move |section| {
            if section.section.name() != "Xsections" {
                return None;
            }
            match ctx.find_bytes_from(
                section.region,
                b"Sld_Xsections\0",
                0,
                "find Creo cross-section namespace",
            ) {
                Ok(Some(_)) => Some(Ok(section)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            }
        }))
}

fn collect_section_records_result<'a, 'data: 'a, T>(
    ctx: &DecodeContext<'_>,
    sections: impl Iterator<Item = Result<&'a ScannedSection<'data>, CodecError>>,
    mut decode: impl FnMut(&[u8]) -> Result<Vec<T>, CodecError>,
    relocate: impl Fn(&mut T, usize) -> Result<(), CodecError>,
    offset: impl Fn(&T) -> usize,
) -> Result<Vec<T>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut records = Vec::new();
    for section in sections {
        let section = section?;
        let decoded = decode(section.region)?;
        ctx.reserve_vec(
            &mut records,
            decoded.len(),
            "creo section record aggregation",
        )?;
        for mut record in ctx.admit_iter(decoded, "creo section record relocation traversal")? {
            relocate(&mut record, section.section.offset())?;
            records.push(record);
        }
    }
    if records.len() > 1 {
        ctx.stable_sort_by_key(
            records.as_mut_slice(),
            offset,
            Ord::cmp,
            "creo collect section records result records ordering",
        )?;
    }
    Ok(records)
}

/// The container scan of an in-tree fixture that states its own extents.
///
/// Only tests use it. A refusal is a defect in the fixture, so it fails the
/// test rather than returning a shortened scan.
#[cfg(test)]
pub(crate) fn scan_bytes_ok<'a>(data: impl Into<Cow<'a, [u8]>>) -> ContainerScan<'a> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let data = data.into();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data.as_ref(), &arena, &policy)
            .expect("the fixture fits the test decode context");
    match scan_bytes(&ctx, data.clone()) {
        Ok(scan) => scan,
        Err(refusal) => panic!("the fixture states a section past its own end: {refusal}"),
    }
}

/// Return whether a thumbnail section contains a JPEG start marker.
pub(crate) fn has_thumbnail(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<bool, CodecError> {
    ctx.any_by(
        &scan.framing.sections,
        |section| {
            if section.role() != SectionRole::Thumbnail {
                return Ok(false);
            }
            let Some(raw) = section_region(&scan.framing.data, section) else {
                return Ok(false);
            };
            // The `#<name>\n` header precedes the payload.
            let payload_start = section.raw_name.len() + 2;
            let raw_is_compressed = raw
                .get(payload_start..)
                .is_some_and(|payload| payload.starts_with(UNIX_COMPRESS_MAGIC));
            if !raw_is_compressed
                && ctx.find_bytes_from(raw, JPEG_MAGIC, 0, "find Creo thumbnail")?
                    .is_some()
            {
                return Ok(true);
            }
            match expanded_section_for(ctx, scan, section)? {
                Some(expanded) => Ok(ctx
                    .find_bytes_from(
                        &expanded.data,
                        JPEG_MAGIC,
                        0,
                        "find Creo expanded thumbnail",
                    )?
                    .is_some()),
                None => Ok(false),
            }
        },
        "creo thumbnail section selection",
    )
}

/// Build a codec-neutral summary of the sections, layout, and namespace census.
pub(crate) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: crate::dialect::DialectClassification,
) -> Result<ContainerSummary, CodecError> {
    let mut entries = Vec::new();
    for s in ctx.admit_iter(&scan.framing.sections, "creo summary section traversal")? {
        let mut attributes = BTreeMap::new();
        ctx.insert_btree_map(
            &mut attributes,
            ctx.copy_retained_text("offset", "creo summary attribute key")?,
            ctx.format_retained(format_args!("{}", s.offset()), "creo summary offset")?,
            "creo summary attribute nodes",
        )?;
        if s.name.start != 0 || s.name.end != s.raw_name.len() {
            ctx.insert_btree_map(
                &mut attributes,
                ctx.copy_retained_text("raw_name", "creo summary attribute key")?,
                ctx.copy_retained_text(&s.raw_name, "creo summary raw name")?,
                "creo summary attribute nodes",
            )?;
        }
        let expanded = expanded_section_for(ctx, scan, s)?;
        if let Some(expanded) = expanded {
            ctx.insert_btree_map(
                &mut attributes,
                ctx.copy_retained_text("expanded_payload_size", "creo summary attribute key")?,
                ctx.format_retained(
                    format_args!("{}", expanded.data.len()),
                    "creo summary expanded size",
                )?,
                "creo summary attribute nodes",
            )?;
        }
        let name = ctx.copy_retained_text(s.name(), "creo summary entry name")?;
        ctx.reserve_vec(&mut entries, 1, "creo summary entries")?;
        entries.push(ContainerEntry {
            name,
            role: s.role().into(),
            storage: expanded.map_or_else(
                || {
                    EntryStorage::verbatim(
                        VerbatimLabel::None,
                        cadmpeg_core::decode::u64_from_index(s.length()),
                    )
                },
                |expanded| EntryStorage::Compressed {
                    method: CompressionMethod::UnixCompress,
                    stored: Some(cadmpeg_core::decode::u64_from_index(s.length())),
                    expanded: Some(cadmpeg_core::decode::u64_from_index(
                        expanded.data.len() + s.raw_name.len() + 2,
                    )),
                },
            ),
            attributes,
        });
    }

    let notes = notes(ctx, scan)?;
    let mut losses = Vec::new();
    if let Some(loss) = classification.loss(ctx)? {
        ctx.reserve_vec(&mut losses, 1, "creo summary losses")?;
        losses.push(loss);
    }

    Ok(ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(classification.into_matched()),
        cadmpeg_ir::ContainerKind::Psb,
        entries,
        losses,
        notes,
    ))
}

/// Build the diagnostic notes shared by inspection and decode reports.
pub(crate) fn notes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<String>, CodecError> {
    fn push_note(
        ctx: &DecodeContext<'_>,
        notes: &mut Vec<String>,
        value: impl std::fmt::Display,
    ) -> Result<(), CodecError> {
        let value = ctx.format_retained(format_args!("{value}"), "creo container note text")?;
        ctx.reserve_vec(notes, 1, "creo container notes")?;
        notes.push(value);
        Ok(())
    }
    struct OptionalCount<T>(Option<T>);
    impl<T: std::fmt::Display> std::fmt::Display for OptionalCount<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match &self.0 {
                Some(value) => write!(f, "{value}"),
                None => f.write_str("n/a"),
            }
        }
    }
    let mut notes = Vec::new();
    push_note(
        ctx,
        &mut notes,
        format_args!("PSB container: {}", scan.framing.version_line),
    )?;
    push_note(
        ctx,
        &mut notes,
        format_args!(
            "layout: {}; {} section(s) enumerated",
            scan.framing.layout.token(),
            scan.framing.sections.len()
        ),
    )?;
    if let Some(name) = &scan.framing.model_name {
        push_note(
            ctx,
            &mut notes,
            format_args!("native model name: {}", name.name),
        )?;
    }
    if let Some(legacy) = scan.framing.layout.legacy_ascii() {
        let counts = &legacy.persistence.counts;
        let release = legacy.product_release.as_deref().unwrap_or("unspecified");
        let continuation_count = counts.continuations;
        push_note(
            ctx,
            &mut notes,
            format_args!(
                "legacy ASCII persistence: schema {}; product release {release}; {} attribute \
             declarations, {} resolved values, {continuation_count} continuation rows in {} scopes",
                legacy.schema, counts.declarations, counts.values, counts.scopes,
            ),
        )?;
        if counts.unresolved_values != 0 || counts.conflicting_declarations != 0 {
            push_note(
                ctx,
                &mut notes,
                format_args!(
                "legacy ASCII structural gaps: {} unresolved values, {} conflicting declarations",
                counts.unresolved_values,
                counts.conflicting_declarations,
            ),
            )?;
        }
    }

    match (
        scan.framing.census.srf_array_count,
        scan.framing.census.crv_array_count,
    ) {
        (None, None) => {
            push_note(
                ctx,
                &mut notes,
                "no VisibGeom srf_array/crv_array count header was located",
            )?;
        }
        (srf, crv) => {
            push_note(
                ctx,
                &mut notes,
                format_args!(
                    "VisibGeom namespace census: srf_array={}, crv_array={} (byte-backed count \
                 headers; per-instance row geometry is not decoded)",
                    OptionalCount(srf),
                    OptionalCount(crv),
                ),
            )?;
        }
    }

    if has_thumbnail(ctx, scan)? {
        push_note(
            ctx,
            &mut notes,
            "THMB_IMG_MAIN carries a JPEG preview (excluded from geometry)",
        )?;
    }
    if !scan.framing.expanded_sections.is_empty() {
        push_note(
            ctx,
            &mut notes,
            format_args!(
                "expanded {} Unix-compress section payload(s) with TOC-validated output lengths",
                scan.framing.expanded_sections.len()
            ),
        )?;
    }

    push_note(
        ctx,
        &mut notes,
        "container-level enumeration; `decode` preserves PSB geometry sections as unknown records \
         and transfers only carriers whose model-space placement is complete",
    )?;

    Ok(notes)
}

#[cfg(test)]
mod feature_row_definition_tests {
    use super::{feature_row_has_model_identity, structural_feature_ids, toc_sections};
    use crate::curve::CurveTopologyRow;
    use crate::feature;
    use crate::feature::operations::{FeatureOperation, FeatureRecipe, FeatureReferenceName};
    use crate::feature::rows::FeatureRow;
    use crate::surface::SurfaceRow;

    fn feature_row_definitions(rows: &[FeatureRow]) -> Vec<super::FeatureDefinition> {
        crate::decode::with_test_decode_ctx(|ctx| super::feature_row_definitions(ctx, rows))
            .expect("feature row definitions admitted")
    }

    fn section_owner_ranges(
        sections: &[super::ScannedSection<'_>],
        rows: &[FeatureRow],
    ) -> Vec<(usize, usize)> {
        crate::decode::with_test_decode_ctx(|ctx| super::section_owner_ranges(ctx, sections, rows))
            .expect("section owner ranges admitted")
    }

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
            crate::decode::with_test_decode_ctx(|ctx| {
                structural_feature_ids(ctx, &[], &[surface], &[curve])
            })
            .expect("structural feature ids admitted"),
            std::collections::BTreeSet::from([40, 41])
        );
    }

    #[test]
    fn toc_offset_radix_parse_refuses_work() {
        use cadmpeg_core::decode::ResourceDimension;
        let row = "VisibGeom 0 0 0\n";
        let data = format!("#UGC_TOC 2 1 {}#\n{row}", row.len());
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo TOC offset hexadecimal parsing",
            |ctx| toc_sections(ctx, &mut ctx.reserve_scoped(0, "test section roster storage").expect("empty storage"), data.as_bytes(), 0),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo TOC offset hexadecimal parsing")
        );
    }

    #[test]
    fn toc_length_radix_parse_refuses_work() {
        use cadmpeg_core::decode::ResourceDimension;
        let row = "VisibGeom 0 0 0\n";
        let data = format!("#UGC_TOC 2 1 {}#\n{row}", row.len());
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo TOC length hexadecimal parsing",
            |ctx| toc_sections(ctx, &mut ctx.reserve_scoped(0, "test section roster storage").expect("empty storage"), data.as_bytes(), 0),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo TOC length hexadecimal parsing")
        );
    }

    #[test]
    fn toc_expanded_length_radix_parse_refuses_work() {
        use cadmpeg_core::decode::ResourceDimension;
        let row = "VisibGeom 0 0 0\n";
        let data = format!("#UGC_TOC 2 1 {}#\n{row}", row.len());
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo TOC expanded length hexadecimal parsing",
            |ctx| toc_sections(ctx, &mut ctx.reserve_scoped(0, "test section roster storage").expect("empty storage"), data.as_bytes(), 0),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits
                && resource.operation == "creo TOC expanded length hexadecimal parsing")
        );
    }

    #[test]
    fn zero_width_toc_has_no_rows() {
        let data = b"#UGC_TOC 2 18446744073709551615 0#\n";

        assert!(
            crate::decode::with_test_decode_ctx(|ctx| toc_sections(ctx, &mut ctx.reserve_scoped(0, "test section roster storage").expect("empty storage"), data, 0))
                .expect("empty TOC admitted")
                .is_empty()
        );
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

        assert!(
            crate::decode::with_test_decode_ctx(|ctx| feature_row_has_model_identity(
                ctx,
                &row(42, 913),
                &structural,
                std::slice::from_ref(&operation),
                std::slice::from_ref(&reference),
            ))
            .expect("service profile admits scalar parsing")
        );
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| feature_row_has_model_identity(
                ctx,
                &row(42, 923),
                &structural,
                std::slice::from_ref(&operation),
                std::slice::from_ref(&reference),
            ))
            .expect("service profile admits scalar parsing")
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| feature_row_has_model_identity(
                ctx,
                &row(73, 926),
                &structural,
                &[operation],
                &[reference],
            ))
            .expect("service profile admits scalar parsing")
        );
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| feature_row_has_model_identity(
                ctx,
                &row(87, 923),
                &structural,
                &[],
                std::slice::from_ref(&datum_reference),
            ))
            .expect("service profile admits scalar parsing")
        );
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| feature_row_has_model_identity(
                ctx,
                &row(87, 911),
                &structural,
                &[],
                &[datum_reference],
            ))
            .expect("service profile admits scalar parsing")
        );
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

        let definitions = crate::decode::with_test_decode_ctx(|ctx| {
            feature::definitions::bind_section_owners(
                ctx,
                definitions,
                &[
                    operation(247, Some(FeatureRecipe::ProtrudeRevolve), 10),
                    operation(248, None, 20),
                ],
                &section_owner_ranges(&[], &[row]),
            )
        })
        .expect("service section owner binding");

        assert_eq!(definitions[0].identity.id(), 2);
        assert_eq!(definitions[0].identity.owner_feature_id(), Some(247));
    }
}

#[cfg(test)]
mod tests;
