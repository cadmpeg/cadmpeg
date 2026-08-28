// SPDX-License-Identifier: Apache-2.0
//! Decode loss and validation findings.

use std::collections::BTreeMap;
use std::fmt;

use cadmpeg_core::dialect::{DialectId, DialectMatch};

use crate::provenance::SourceProvenance;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity of a loss note or validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational; no action needed.
    Info,
    /// Non-fatal approximation or normalization.
    Warning,
    /// A correctness problem in the produced IR or export.
    Error,
    /// A hard stop: the requested operation cannot be completed faithfully.
    Blocking,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Blocking => "blocking",
        })
    }
}

/// What subsystem a loss pertains to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LossCategory {
    /// Geometry (surfaces/curves/points) not transferred or approximated.
    Geometry,
    /// Topology (graph structure) not transferred.
    Topology,
    /// Materials/appearances not transferred.
    Material,
    /// Document metadata not transferred.
    Metadata,
    /// Units/tolerances issues.
    Units,
    /// Attributes (names, colors, custom attribs) not transferred.
    Attribute,
    /// Features, sketches, parameters, configurations, or design history not transferred.
    DesignIntent,
    /// Product structure, component occurrences, placements, or external dependencies.
    Product,
    /// Anything else.
    Other,
}

impl fmt::Display for LossCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Geometry => "geometry",
            Self::Topology => "topology",
            Self::Material => "material",
            Self::Metadata => "metadata",
            Self::Units => "units",
            Self::Attribute => "attribute",
            Self::DesignIntent => "design_intent",
            Self::Product => "product",
            Self::Other => "other",
        })
    }
}

/// Strict-mode handling for a loss code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StrictConsequence {
    /// Strict mode must refuse the operation.
    Reject,
    /// Strict mode may proceed.
    Tolerate,
}

/// Shared cross-codec loss taxonomy.
///
/// Category and default severity live here. Codec-local loss enums map into a
/// taxonomy variant for subsystem reporting; strict-mode floors on a
/// [`LossKind`] are pinned at construction so a later local→taxonomy remap in
/// source does not silently change rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LossTaxonomy {
    /// Container-only decode was requested; entity decode was not attempted.
    ContainerOnly,
    /// No geometry stream was located in the container, so no B-rep could be
    /// transferred.
    MissingGeometryStream,
    /// The B-rep topology graph was not transferred, though carriers or a
    /// container were decoded.
    TopologyNotTransferred,
    /// The source topology violates a neutral topology invariant; the decoder
    /// reported the defect and retained only valid neutral topology.
    SourceTopologyInvalid,
    /// B-rep geometry was not transferred, though carriers or a container were
    /// decoded.
    GeometryNotTransferred,
    /// A reference graph decoded but did not close into a consistent
    /// surface/pcurve/edge/vertex binding.
    ReferenceGraphNotClosed,
    /// Face sense, body kind, or a body/region/shell hierarchy was supplied by
    /// a deterministic gauge because the source fields were unresolved.
    TopologyGaugeSubstituted,
    /// A carrier axis, plane, or orientation was inferred from adjacent
    /// carriers rather than read from a source field.
    CarrierAxisInferred,
    /// Informational carrier or record census; no content was lost.
    CarrierSummary,
    /// Materials or appearances were not transferred.
    MaterialNotTransferred,
    /// Document, feature, or part metadata was not transferred.
    MetadataNotTransferred,
    /// Attributes (names, colors, custom attributes) were not transferred.
    AttributesNotTransferred,
    /// Named feature operations and their dependency tables were retained as
    /// native passthrough rather than replayed.
    FeatureHistoryRetained,
    /// The part is an assembly; component geometry lives in external referenced
    /// files, not inline.
    AssemblyComponentsExternal,
    /// Assembly component occurrence placements were not transferred.
    AssemblyPlacementsNotTransferred,
    /// A record was decoded but yielded no typed IR entity.
    RecordNotTyped,
    /// A decode-time diagnostic surfaced as a loss note; detail is in the
    /// message.
    DecodeDiagnostic,
    /// Stored integrity data does not match the bytes it protects.
    IntegrityFailure,
    /// The source uses a recoverable but noncanonical serialization.
    NoncanonicalSourceSyntax,
    /// The source declares a dialect or specification version whose semantics
    /// the decoder has not verified for that declaration.
    SourceDialectUnverified,
    /// The writer emitted a different dialect from the source dialect, so the
    /// source dialect identity was not preserved.
    SourceDialectDisplaced,
    /// Standalone mesh vertices were stored at reduced (f32) precision by the
    /// source archive.
    MeshVertexPrecision,
    /// Some source object records were not transferred to typed IR.
    ObjectRecordsUntransferred,
    /// An object family or class is not supported and was not transferred.
    UnsupportedObjectFamily,
    /// A named source asset (geometry, material, or other) was not transferred.
    AssetNotTransferred,
    /// The IR contained no exportable solids, so the target representation is
    /// empty.
    NoExportableSolids,
    /// Hidden bodies were omitted from the exported output.
    HiddenBodyOmitted,
    /// A body's non-identity transform was not applied; coordinates are written
    /// in body-local space.
    BodyTransformNotApplied,
    /// Signed or self-intersecting analytic surfaces were normalized to the
    /// target's positive-radius convention.
    AnalyticSurfaceNormalized,
    /// Elliptical cones were reduced to circular conical carriers.
    EllipticalConeReduced,
    /// Edges without a typed 3D curve were omitted from their edge loops.
    CurvelessEdgeOmitted,
    /// Faces resting on an unknown surface were omitted from the exported shell.
    UnknownSurfaceFaceOmitted,
    /// Parameter-space pcurves were not written; consumers recompute trims.
    PcurveOmitted,
    /// Subdivision surfaces were omitted because the writer does not encode
    /// control cages.
    SubdOmitted,
    /// Tessellations were omitted because the writer emits exact geometry only.
    TessellationOmitted,
    /// Product-manufacturing-information annotations were not represented in the target.
    PmiOmitted,
    /// Source-object associations were not represented in the target.
    SourceAssociationOmitted,
    /// Uninterpreted passthrough records were not represented in the target.
    PassthroughRecordOmitted,
    /// Procedural surface or curve definitions were reduced to their solved
    /// carriers.
    ProceduralReduced,
    /// Parametric design or history records were not represented in the target.
    ParametricRecordOmitted,
    /// Appearance assets were reduced to base colors; schemas, textures, and
    /// shader properties were dropped.
    AppearanceReduced,
    /// Preserved source bytes required for a byte-exact write were unavailable.
    PreservedSourceUnavailable,
}

impl LossTaxonomy {
    /// The stable `snake_case` identifier for this taxonomy variant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContainerOnly => "container_only",
            Self::MissingGeometryStream => "missing_geometry_stream",
            Self::TopologyNotTransferred => "topology_not_transferred",
            Self::SourceTopologyInvalid => "source_topology_invalid",
            Self::GeometryNotTransferred => "geometry_not_transferred",
            Self::ReferenceGraphNotClosed => "reference_graph_not_closed",
            Self::TopologyGaugeSubstituted => "topology_gauge_substituted",
            Self::CarrierAxisInferred => "carrier_axis_inferred",
            Self::CarrierSummary => "carrier_summary",
            Self::MaterialNotTransferred => "material_not_transferred",
            Self::MetadataNotTransferred => "metadata_not_transferred",
            Self::AttributesNotTransferred => "attributes_not_transferred",
            Self::FeatureHistoryRetained => "feature_history_retained",
            Self::AssemblyComponentsExternal => "assembly_components_external",
            Self::AssemblyPlacementsNotTransferred => "assembly_placements_not_transferred",
            Self::RecordNotTyped => "record_not_typed",
            Self::DecodeDiagnostic => "decode_diagnostic",
            Self::IntegrityFailure => "integrity_failure",
            Self::NoncanonicalSourceSyntax => "noncanonical_source_syntax",
            Self::SourceDialectUnverified => "source_dialect_unverified",
            Self::SourceDialectDisplaced => "source_dialect_displaced",
            Self::MeshVertexPrecision => "mesh_vertex_precision",
            Self::ObjectRecordsUntransferred => "object_records_untransferred",
            Self::UnsupportedObjectFamily => "unsupported_object_family",
            Self::AssetNotTransferred => "asset_not_transferred",
            Self::NoExportableSolids => "no_exportable_solids",
            Self::HiddenBodyOmitted => "hidden_body_omitted",
            Self::BodyTransformNotApplied => "body_transform_not_applied",
            Self::AnalyticSurfaceNormalized => "analytic_surface_normalized",
            Self::EllipticalConeReduced => "elliptical_cone_reduced",
            Self::CurvelessEdgeOmitted => "curveless_edge_omitted",
            Self::UnknownSurfaceFaceOmitted => "unknown_surface_face_omitted",
            Self::PcurveOmitted => "pcurve_omitted",
            Self::SubdOmitted => "subd_omitted",
            Self::TessellationOmitted => "tessellation_omitted",
            Self::PmiOmitted => "pmi_omitted",
            Self::SourceAssociationOmitted => "source_association_omitted",
            Self::PassthroughRecordOmitted => "passthrough_record_omitted",
            Self::ProceduralReduced => "procedural_reduced",
            Self::ParametricRecordOmitted => "parametric_record_omitted",
            Self::AppearanceReduced => "appearance_reduced",
            Self::PreservedSourceUnavailable => "preserved_source_unavailable",
        }
    }

    /// Parse a v1 bare `snake_case` taxonomy identifier.
    pub fn from_v1_str(text: &str) -> Option<Self> {
        Some(match text {
            "container_only" => Self::ContainerOnly,
            "missing_geometry_stream" => Self::MissingGeometryStream,
            "topology_not_transferred" => Self::TopologyNotTransferred,
            "source_topology_invalid" => Self::SourceTopologyInvalid,
            "geometry_not_transferred" => Self::GeometryNotTransferred,
            "reference_graph_not_closed" => Self::ReferenceGraphNotClosed,
            "topology_gauge_substituted" => Self::TopologyGaugeSubstituted,
            "carrier_axis_inferred" => Self::CarrierAxisInferred,
            "carrier_summary" => Self::CarrierSummary,
            "material_not_transferred" => Self::MaterialNotTransferred,
            "metadata_not_transferred" => Self::MetadataNotTransferred,
            "attributes_not_transferred" => Self::AttributesNotTransferred,
            "feature_history_retained" => Self::FeatureHistoryRetained,
            "assembly_components_external" => Self::AssemblyComponentsExternal,
            "assembly_placements_not_transferred" => Self::AssemblyPlacementsNotTransferred,
            "record_not_typed" => Self::RecordNotTyped,
            "decode_diagnostic" => Self::DecodeDiagnostic,
            "integrity_failure" => Self::IntegrityFailure,
            "noncanonical_source_syntax" => Self::NoncanonicalSourceSyntax,
            "source_dialect_unverified" => Self::SourceDialectUnverified,
            "source_dialect_displaced" => Self::SourceDialectDisplaced,
            "mesh_vertex_precision" => Self::MeshVertexPrecision,
            "object_records_untransferred" => Self::ObjectRecordsUntransferred,
            "unsupported_object_family" => Self::UnsupportedObjectFamily,
            "asset_not_transferred" => Self::AssetNotTransferred,
            "no_exportable_solids" => Self::NoExportableSolids,
            "hidden_body_omitted" => Self::HiddenBodyOmitted,
            "body_transform_not_applied" => Self::BodyTransformNotApplied,
            "analytic_surface_normalized" => Self::AnalyticSurfaceNormalized,
            "elliptical_cone_reduced" => Self::EllipticalConeReduced,
            "curveless_edge_omitted" => Self::CurvelessEdgeOmitted,
            "unknown_surface_face_omitted" => Self::UnknownSurfaceFaceOmitted,
            "pcurve_omitted" => Self::PcurveOmitted,
            "subd_omitted" => Self::SubdOmitted,
            "tessellation_omitted" => Self::TessellationOmitted,
            "pmi_omitted" => Self::PmiOmitted,
            "source_association_omitted" => Self::SourceAssociationOmitted,
            "passthrough_record_omitted" => Self::PassthroughRecordOmitted,
            "procedural_reduced" => Self::ProceduralReduced,
            "parametric_record_omitted" => Self::ParametricRecordOmitted,
            "appearance_reduced" => Self::AppearanceReduced,
            "preserved_source_unavailable" => Self::PreservedSourceUnavailable,
            _ => return None,
        })
    }

    /// Returns the subsystem affected by this kind of loss.
    pub const fn category(self) -> LossCategory {
        match self {
            Self::TopologyNotTransferred
            | Self::SourceTopologyInvalid
            | Self::ReferenceGraphNotClosed
            | Self::TopologyGaugeSubstituted
            | Self::NoExportableSolids
            | Self::HiddenBodyOmitted => LossCategory::Topology,
            Self::MaterialNotTransferred | Self::AppearanceReduced => LossCategory::Material,
            Self::MetadataNotTransferred | Self::SourceAssociationOmitted => LossCategory::Metadata,
            Self::AttributesNotTransferred | Self::PmiOmitted => LossCategory::Attribute,
            Self::FeatureHistoryRetained | Self::ParametricRecordOmitted => {
                LossCategory::DesignIntent
            }
            Self::AssemblyComponentsExternal | Self::AssemblyPlacementsNotTransferred => {
                LossCategory::Product
            }
            Self::RecordNotTyped
            | Self::DecodeDiagnostic
            | Self::IntegrityFailure
            | Self::NoncanonicalSourceSyntax
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::AssetNotTransferred
            | Self::PassthroughRecordOmitted
            | Self::PreservedSourceUnavailable => LossCategory::Other,
            Self::ContainerOnly
            | Self::MissingGeometryStream
            | Self::GeometryNotTransferred
            | Self::CarrierAxisInferred
            | Self::CarrierSummary
            | Self::MeshVertexPrecision
            | Self::ObjectRecordsUntransferred
            | Self::UnsupportedObjectFamily
            | Self::BodyTransformNotApplied
            | Self::AnalyticSurfaceNormalized
            | Self::EllipticalConeReduced
            | Self::CurvelessEdgeOmitted
            | Self::UnknownSurfaceFaceOmitted
            | Self::PcurveOmitted
            | Self::SubdOmitted
            | Self::TessellationOmitted
            | Self::ProceduralReduced => LossCategory::Geometry,
        }
    }

    /// Returns the default severity for this kind of loss.
    pub const fn default_severity(self) -> Severity {
        match self {
            Self::ContainerOnly | Self::CarrierSummary | Self::PassthroughRecordOmitted => {
                Severity::Info
            }
            Self::MissingGeometryStream | Self::NoExportableSolids | Self::IntegrityFailure => {
                Severity::Error
            }
            _ => Severity::Warning,
        }
    }

    /// Returns the minimum severity that makes strict mode reject this loss.
    pub const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::MissingGeometryStream
            | Self::TopologyNotTransferred
            | Self::GeometryNotTransferred
            | Self::ReferenceGraphNotClosed
            | Self::CurvelessEdgeOmitted
            | Self::UnknownSurfaceFaceOmitted
            | Self::SubdOmitted
            | Self::NoncanonicalSourceSyntax
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::IntegrityFailure
            | Self::NoExportableSolids => Some(Severity::Warning),
            _ => None,
        }
    }
}

impl fmt::Display for LossTaxonomy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Namespace for shared (non-codec-local) loss codes.
pub const SHARED_LOSS_NAMESPACE: &str = "shared";

/// Namespaced machine-readable loss code on the decode/export wire.
///
/// Wire form (sidecar v2 / report payloads):
/// `{ "namespace": "rhino", "code": "brep.trim-pcurve-dropped", "kind": "pcurve_omitted" }`.
/// The optional `strict_floor` field is omitted when it matches [`LossTaxonomy::strict_floor`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename_all = "snake_case"))]
pub struct LossKind {
    /// Codec namespace, or [`SHARED_LOSS_NAMESPACE`].
    pub namespace: String,
    /// Local code within the namespace.
    pub code: String,
    /// Shared taxonomy for category and default severity.
    pub taxonomy: LossTaxonomy,
    /// Pinned strict-mode floor; omitted on the wire when equal to the taxonomy floor.
    #[cfg_attr(feature = "schema", schemars(skip))]
    strict_floor: Option<Severity>,
}

#[derive(Serialize, Deserialize)]
struct LossKindWire {
    namespace: String,
    code: String,
    kind: LossTaxonomy,
    /// Present when the floor differs from [`LossTaxonomy::strict_floor`]; may be JSON null.
    #[serde(default, deserialize_with = "deserialize_optional_strict_floor")]
    strict_floor: StrictFloorWire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum StrictFloorWire {
    /// Field absent: use the taxonomy default.
    #[default]
    Absent,
    /// Field present, including explicit JSON null.
    Explicit(Option<Severity>),
}

impl Serialize for StrictFloorWire {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Absent => serializer.serialize_none(),
            Self::Explicit(floor) => floor.serialize(serializer),
        }
    }
}

fn deserialize_optional_strict_floor<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<StrictFloorWire, D::Error> {
    Ok(StrictFloorWire::Explicit(Option::<Severity>::deserialize(
        deserializer,
    )?))
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde skip_serializing_if requires &T
fn strict_floor_wire_is_absent(value: &StrictFloorWire) -> bool {
    matches!(value, StrictFloorWire::Absent)
}

#[derive(Serialize)]
struct LossKindSerializeWire<'a> {
    namespace: &'a str,
    code: &'a str,
    kind: LossTaxonomy,
    #[serde(skip_serializing_if = "strict_floor_wire_is_absent")]
    strict_floor: StrictFloorWire,
}

impl Serialize for LossKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let taxonomy_floor = self.taxonomy.strict_floor();
        let strict_floor = if self.strict_floor == taxonomy_floor {
            StrictFloorWire::Absent
        } else {
            StrictFloorWire::Explicit(self.strict_floor)
        };
        LossKindSerializeWire {
            namespace: &self.namespace,
            code: &self.code,
            kind: self.taxonomy,
            strict_floor,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LossKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = LossKindWire::deserialize(deserializer)?;
        let strict_floor = match wire.strict_floor {
            StrictFloorWire::Absent => wire.kind.strict_floor(),
            StrictFloorWire::Explicit(floor) => floor,
        };
        Ok(Self {
            namespace: wire.namespace,
            code: wire.code,
            taxonomy: wire.kind,
            strict_floor,
        })
    }
}

impl LossKind {
    /// Shared-namespace code whose local id equals the taxonomy `snake_case` name.
    pub fn shared(taxonomy: LossTaxonomy) -> Self {
        Self {
            namespace: SHARED_LOSS_NAMESPACE.into(),
            code: taxonomy.as_str().into(),
            taxonomy,
            strict_floor: taxonomy.strict_floor(),
        }
    }

    /// Codec-local code under `namespace`, classified by `taxonomy` for category.
    ///
    /// Strict floor defaults to the taxonomy floor; override with
    /// [`LossKind::with_strict_floor`] so a local→taxonomy remap cannot change
    /// rejection without an explicit local-floor change.
    pub fn namespaced(
        namespace: impl Into<String>,
        code: impl Into<String>,
        taxonomy: LossTaxonomy,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            code: code.into(),
            taxonomy,
            strict_floor: taxonomy.strict_floor(),
        }
    }

    /// Pins the strict-mode severity floor independently of taxonomy.
    #[must_use]
    pub fn with_strict_floor(mut self, floor: Option<Severity>) -> Self {
        self.strict_floor = floor;
        self
    }

    /// Codec or `shared` namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Local code within the namespace.
    pub fn local_code(&self) -> &str {
        &self.code
    }

    /// Shared taxonomy used for category and default severity.
    pub const fn taxonomy(&self) -> LossTaxonomy {
        self.taxonomy
    }

    /// Stable display form `namespace/code`.
    pub fn as_str(&self) -> String {
        format!("{}/{}", self.namespace, self.code)
    }

    /// Returns the subsystem affected by this kind of loss.
    pub const fn category(&self) -> LossCategory {
        self.taxonomy.category()
    }

    /// Returns the default severity from the taxonomy.
    pub const fn default_severity(&self) -> Severity {
        self.taxonomy.default_severity()
    }

    /// Returns the pinned strict-mode severity floor.
    pub const fn strict_floor(&self) -> Option<Severity> {
        self.strict_floor
    }

    /// Reconstruct from a v1 bare taxonomy string (`"geometry_not_transferred"`).
    pub fn from_v1_str(text: &str) -> Option<Self> {
        LossTaxonomy::from_v1_str(text).map(Self::shared)
    }
}

impl From<LossTaxonomy> for LossKind {
    fn from(taxonomy: LossTaxonomy) -> Self {
        Self::shared(taxonomy)
    }
}

impl fmt::Display for LossKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace, self.code)
    }
}

/// One attributable instance of incomplete or approximate transfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LossNote {
    /// Stable machine-readable loss kind.
    pub code: LossKind,
    /// How serious the loss is.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// Where in the source the loss occurred, when attributable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SourceProvenance>,
}

impl LossNote {
    /// Creates a loss note with the kind's default severity and no provenance.
    pub fn new(code: impl Into<LossKind>, message: impl Into<String>) -> Self {
        let code = code.into();
        Self {
            severity: code.default_severity(),
            code,
            message: message.into(),
            provenance: None,
        }
    }

    /// Overrides this note's severity.
    #[must_use]
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Attaches source provenance to this note.
    #[must_use]
    pub fn with_provenance(mut self, provenance: SourceProvenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// Returns strict-mode handling after applying the kind's severity floor.
    pub fn strict_consequence(&self) -> StrictConsequence {
        match self.code.strict_floor() {
            Some(floor) if self.severity >= floor => StrictConsequence::Reject,
            _ => StrictConsequence::Tolerate,
        }
    }
}

/// Transfer status and loss details from a successful decode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DecodeReport {
    /// Source format id.
    pub format: String,
    /// Whether the decode stopped at the container layer (no entity decode).
    /// The shared codec wrapper stamps this from the decode request.
    pub container_only: bool,
    /// Whether the decoder transferred B-rep geometry into the IR.
    pub geometry_transferred: bool,
    /// Decode coverage counts keyed by measure name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub coverage: BTreeMap<String, usize>,
    /// Explicit loss notes.
    pub losses: Vec<LossNote>,
    /// Free-form informational notes (e.g. container findings).
    pub notes: Vec<String>,
    /// Per-source disposition ledger for decoded records and entities.
    #[serde(default, skip_serializing_if = "TransferLedger::is_empty")]
    pub transfer_ledger: TransferLedger,
    /// Dialect identification, one entry per format layer the decode read.
    ///
    /// Empty only when the decode identified no layer at all. Once populated,
    /// exactly one entry's `format` equals [`Self::format`]: that entry is the
    /// primary layer, and it is the one mirrored into
    /// [`crate::document::SourceMeta::dialect`]. [`crate::codec::DecodeResult::new`]
    /// enforces the invariant at the decode boundary.
    ///
    /// Always serialized. Reports written before the field existed omit the key
    /// and read back empty.
    #[serde(default)]
    pub dialects: Vec<DialectMatch>,
}

/// Final disposition of one source record or semantic object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TransferDisposition {
    /// Transferred as an exact neutral or native entity.
    Emitted,
    /// Preserved in a native retained-record arena.
    Retained,
    /// Transferred with an explicit approximation.
    Approximated,
    /// Deliberately not transferred.
    Omitted,
}

/// One source object's transfer disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TransferRecord {
    /// Stable source identity or source-local record key.
    pub source: String,
    /// Resulting neutral or native identity, when one was produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Final transfer disposition.
    pub disposition: TransferDisposition,
    /// Concise reason for approximation or omission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Complete source-to-result accounting for a decode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TransferLedger {
    /// Entries in deterministic source traversal order.
    pub entries: Vec<TransferRecord>,
}

impl TransferLedger {
    /// Returns whether the ledger contains no transfer entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records one source disposition.
    pub fn record(
        &mut self,
        source: impl Into<String>,
        target: Option<String>,
        disposition: TransferDisposition,
        note: Option<String>,
    ) {
        self.entries.push(TransferRecord {
            source: source.into(),
            target,
            disposition,
            note,
        });
    }

    /// Verifies every produced target against a finalized model index.
    pub fn verify(&self, index: &crate::index::ModelIndex<'_>) -> Result<(), String> {
        for entry in &self.entries {
            let produces_target = matches!(
                entry.disposition,
                TransferDisposition::Emitted
                    | TransferDisposition::Retained
                    | TransferDisposition::Approximated
            );
            match (&entry.target, produces_target) {
                (Some(target), true) if !index.contains(target) => {
                    return Err(format!(
                        "transfer source {:?} targets unresolved identity {:?}",
                        entry.source, target
                    ));
                }
                (None, true) => {
                    return Err(format!(
                        "transfer source {:?} has {:?} disposition without a target",
                        entry.source, entry.disposition
                    ));
                }
                (Some(_), false) => {
                    return Err(format!(
                        "omitted transfer source {:?} unexpectedly has a target",
                        entry.source
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// A statically declared decode-coverage measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverageKey(pub &'static str);

impl DecodeReport {
    /// Records a coverage measure count for a statically declared key.
    ///
    /// Producers pass the observed count (not an implied +1). Repeated calls
    /// for the same key replace the prior value.
    pub fn record_coverage(&mut self, key: CoverageKey, count: usize) {
        self.coverage.insert(key.0.to_owned(), count);
    }

    /// Returns a coverage measure, treating an unobserved measure as zero.
    pub fn coverage_count(&self, key: CoverageKey) -> usize {
        self.coverage.get(key.0).copied().unwrap_or(0)
    }

    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|l| l.severity >= Severity::Error)
            .count()
    }
}

/// Entity census and fidelity details from a successful export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ExportReport {
    /// Target format id.
    pub format: String,
    /// Entity counts and the semantic basis on which they were measured.
    pub census: EntityCensus,
    /// How decode-time source fidelity was handled.
    pub fidelity: FidelityResolution,
    /// Which write path produced the exported bytes.
    pub write_path: WritePath,
    /// Omitted, normalized, or reduced content.
    pub losses: Vec<LossNote>,
    /// Informational details about the export path.
    pub notes: Vec<String>,
    /// The concrete dialect written, including on replay and patch paths, where
    /// the encoder states what the preserved dialect was.
    ///
    /// `None` on exactly one write path, and it stays `Option` for that one:
    /// [`crate::codec::CadirEncoder`] writes the neutral document itself, whose
    /// version is data about cadmpeg and never a dialect, so there is no id to
    /// name. Every native encoder names one on every path, replay and patch
    /// included.
    ///
    /// Always serialized, as `null` when absent. Reports written before the
    /// field existed omit the key and read back `None`.
    #[serde(default)]
    target: Option<DialectId>,
}

/// Which of an encoder's write paths produced the exported bytes.
///
/// An encoder that retains its source bytes has two ways to answer "write this
/// document": copy the retained bytes out, or run the writer. The two are
/// indistinguishable from the output alone whenever the writer happens to
/// reproduce the input, so a round-trip test that only compares bytes cannot say
/// which one it exercised — and a test over an unedited document takes the copy
/// path, proving nothing about the writer. This value is set at the branch the
/// encoder actually took, never derived from the output afterwards, so the
/// distinction is a fact the caller can assert on.
///
/// The variants are ordered by how much of the output the encoder authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WritePath {
    /// Retained source bytes were copied to the output unchanged. No writer code
    /// ran, so the output says nothing about the writer.
    VerbatimReplay,
    /// The writer ran and consumed retained source content, rewriting part of a
    /// container it did not author in full.
    Patched,
    /// The writer ran over neutral IR content alone, authoring every output byte.
    Synthesized,
}

impl fmt::Display for WritePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::VerbatimReplay => "verbatim_replay",
            Self::Patched => "patched",
            Self::Synthesized => "synthesized",
        })
    }
}

/// How an encoder resolved optional source fidelity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum FidelityResolution {
    /// The input had no decode-time fidelity state.
    NotProvided,
    /// Preserved source content was consumed successfully.
    Replayed,
    /// The encoder does not consume source fidelity.
    NotConsumed,
    /// Fidelity was available but could not be consumed.
    Degraded {
        /// Explanation of the degradation.
        reason: String,
    },
}

/// The model against which export entity counts were measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CensusBasis {
    /// Counts describe records emitted in the target format.
    TargetRecords,
    /// Counts describe input IR arenas.
    IrArenas,
}

/// Explicitly based entity counts for one export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct EntityCensus {
    /// Semantic basis of `counts`.
    pub basis: CensusBasis,
    /// Counts keyed by arena or target-record kind.
    pub counts: BTreeMap<String, usize>,
}

impl EntityCensus {
    /// Total count across every census row.
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }
}

impl ExportReport {
    /// The concrete native dialect written, or `None` for neutral CADIR.
    #[must_use]
    pub fn target(&self) -> Option<&DialectId> {
        self.target.as_ref()
    }

    /// Constructs a report for the neutral CADIR document, which has no native
    /// dialect target.
    #[must_use]
    pub fn cadir(
        format: String,
        census: EntityCensus,
        fidelity: FidelityResolution,
        write_path: WritePath,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            format,
            census,
            fidelity,
            write_path,
            losses,
            notes,
            target: None,
        }
    }

    /// Constructs a native-format report with its required dialect target.
    #[must_use]
    pub fn native(
        target: DialectId,
        format: String,
        census: EntityCensus,
        fidelity: FidelityResolution,
        write_path: WritePath,
        losses: Vec<LossNote>,
        notes: Vec<String>,
    ) -> Self {
        Self {
            format,
            census,
            fidelity,
            write_path,
            losses,
            notes,
            target: Some(target),
        }
    }

    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|loss| loss.severity >= Severity::Error)
            .count()
    }
}

/// Which invariant a validation finding concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// The document schema version is not the version accepted by this build.
    Version,
    /// Entity identifiers are empty, duplicated, or not globally unique.
    Identity,
    /// Product occurrence ownership, references, or acyclicity.
    ProductStructure,
    /// PMI targets and annotation-to-annotation references.
    Pmi,
    /// Presentation-layer membership and references.
    Presentation,
    /// An arena is not sorted lexicographically by entity id.
    ArenaOrder,
    /// A referenced id does not resolve in its arena.
    ReferentialIntegrity,
    /// A face loop's coedge ring does not close.
    LoopClosure,
    /// An edge's two coedges do not pair consistently.
    CoedgePairing,
    /// Wire edges, free vertices, or wire bodies violate topology ownership rules.
    WireTopology,
    /// A face-bearing shell is disconnected through shared edges or vertices.
    ShellTopology,
    /// A geometry carrier cannot be reached from topology or retained construction data.
    CarrierReachability,
    /// An annotation key, stream index, or field path is invalid.
    Annotations,
    /// A source-native namespace record has an unresolved link.
    NativeLinks,
    /// An edge parameter range violates the carrier's canonical domain.
    ParameterDomain,
    /// A document-wide or per-entity tolerance is not sane.
    Tolerances,
    /// A preserved byte payload does not match its declared digest or length.
    PayloadIntegrity,
    /// A tessellation payload is malformed.
    Tessellation,
    /// The document's units are missing or non-canonical, or a tolerance is
    /// invalid.
    Units,
    /// A geometric quantity is out of sane range (e.g. negative radius).
    Bounds,
    /// Evaluated carrier geometry disagrees with the topology it supports:
    /// an edge's curve endpoints or a pcurve's surface image miss the edge's
    /// vertex positions.
    GeometricConsistency,
    /// Arena counts / cross-references are internally inconsistent.
    Counts,
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Version => "version",
            Self::Identity => "identity",
            Self::ProductStructure => "product_structure",
            Self::Pmi => "pmi",
            Self::Presentation => "presentation",
            Self::ArenaOrder => "arena_order",
            Self::ReferentialIntegrity => "referential_integrity",
            Self::LoopClosure => "loop_closure",
            Self::CoedgePairing => "coedge_pairing",
            Self::WireTopology => "wire_topology",
            Self::ShellTopology => "shell_topology",
            Self::CarrierReachability => "carrier_reachability",
            Self::Annotations => "annotations",
            Self::NativeLinks => "native_links",
            Self::ParameterDomain => "parameter_domain",
            Self::Tolerances => "tolerances",
            Self::PayloadIntegrity => "payload_integrity",
            Self::Tessellation => "tessellation",
            Self::Units => "units",
            Self::Bounds => "bounds",
            Self::GeometricConsistency => "geometric_consistency",
            Self::Counts => "counts",
        })
    }
}

/// A single validation finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Finding {
    /// Which check produced this finding.
    pub check: Check,
    /// Severity.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// The entity id the finding is about, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
}

/// Entity counts, findings, and propagated decode losses for one document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ValidationReport {
    /// Count of entities per arena, keyed by entity kind (sorted).
    pub entity_counts: BTreeMap<String, usize>,
    /// Findings, in discovery order.
    pub findings: Vec<Finding>,
    /// Loss notes supplied to validation.
    #[serde(default)]
    pub losses: Vec<LossNote>,
}

impl ValidationReport {
    /// Number of findings at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity >= Severity::Error)
            .count()
    }

    /// Number of findings at exactly [`Severity::Warning`].
    pub fn warning_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count()
    }

    /// True when there are no [`Severity::Error`]/[`Severity::Blocking`] findings.
    pub fn is_ok(&self) -> bool {
        self.error_count() == 0
    }
}

#[cfg(test)]
mod tests;
