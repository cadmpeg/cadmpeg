// SPDX-License-Identifier: Apache-2.0
//! Transfer-loss vocabulary and attributable loss notes.

use std::fmt;

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
            Self::MissingGeometryStream
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
            Self::CarrierSummary | Self::PassthroughRecordOmitted => Severity::Info,
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

/// Machine-readable loss code on the decode/export wire.
///
/// A `scope` tag selects the variant. A shared loss is
/// `{ "scope": "shared", "kind": "pcurve_omitted" }`; a codec-local one is
/// `{ "scope": "namespaced", "namespace": "rhino", "code": "brep.trim-pcurve-dropped",
/// "kind": "pcurve_omitted" }`, carrying `strict_floor` exactly when the code
/// pins one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum LossKind {
    /// Shared loss whose wire code is determined by its taxonomy.
    Shared {
        /// Shared taxonomy, which is also the local code.
        kind: LossTaxonomy,
    },
    /// Codec-local loss with an independently pinned strict-mode floor.
    Namespaced(NamespacedLossKind),
}

/// A loss namespace other than the reserved shared namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LossNamespace<'a>(&'a str);

/// The reserved shared namespace cannot identify a codec-local loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("LossKind.namespace cannot be shared for a namespaced loss")]
pub struct LossNamespaceError;

impl<'a> LossNamespace<'a> {
    /// Checks a codec-local namespace.
    pub const fn new(namespace: &'a str) -> Result<Self, LossNamespaceError> {
        if matches!(namespace.as_bytes(), b"shared") {
            Err(LossNamespaceError)
        } else {
            Ok(Self(namespace))
        }
    }

    /// Returns the namespace text.
    pub fn as_str(&self) -> &str {
        self.0
    }
}

/// An owned loss namespace other than the reserved shared namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "String", into = "String")]
struct LossNamespaceName(String);

impl LossNamespaceName {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for LossNamespaceName {
    type Error = LossNamespaceError;

    fn try_from(namespace: String) -> Result<Self, Self::Error> {
        LossNamespace::new(namespace.as_str())?;
        Ok(Self(namespace))
    }
}

impl From<LossNamespaceName> for String {
    fn from(namespace: LossNamespaceName) -> Self {
        namespace.0
    }
}

/// Codec-local loss identity and classification.
///
/// Fields are private so the reserved `shared` namespace can be constructed
/// only as [`LossKind::Shared`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NamespacedLossKind {
    namespace: LossNamespaceName,
    code: String,
    #[serde(rename = "kind")]
    taxonomy: LossTaxonomy,
    /// Strict-mode severity floor pinned by this code; absent when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    strict_floor: Option<Severity>,
}

impl LossKind {
    /// Shared-namespace code whose local id equals the taxonomy `snake_case` name.
    pub fn shared(taxonomy: LossTaxonomy) -> Self {
        Self::Shared { kind: taxonomy }
    }

    /// Constructs a codec-local loss with its taxonomy floor.
    pub fn namespaced(
        namespace: LossNamespace<'_>,
        code: impl Into<String>,
        taxonomy: LossTaxonomy,
    ) -> Self {
        NamespacedLossKind::new(namespace, code, taxonomy).into()
    }

    /// Codec or `shared` namespace.
    pub fn namespace(&self) -> &str {
        match self {
            Self::Shared { .. } => SHARED_LOSS_NAMESPACE,
            Self::Namespaced(kind) => kind.namespace.as_str(),
        }
    }

    /// Local code within the namespace.
    pub fn local_code(&self) -> &str {
        match self {
            Self::Shared { kind } => kind.as_str(),
            Self::Namespaced(kind) => &kind.code,
        }
    }

    /// Shared taxonomy used for category and default severity.
    pub const fn taxonomy(&self) -> LossTaxonomy {
        match self {
            Self::Shared { kind } => *kind,
            Self::Namespaced(kind) => kind.taxonomy,
        }
    }

    /// Returns the subsystem affected by this kind of loss.
    pub const fn category(&self) -> LossCategory {
        self.taxonomy().category()
    }

    /// Returns the default severity from the taxonomy.
    pub const fn default_severity(&self) -> Severity {
        self.taxonomy().default_severity()
    }

    /// Returns the pinned strict-mode severity floor.
    pub const fn strict_floor(&self) -> Option<Severity> {
        match self {
            Self::Shared { kind } => kind.strict_floor(),
            Self::Namespaced(kind) => kind.strict_floor,
        }
    }
}

impl NamespacedLossKind {
    /// Constructs a codec-local loss with its taxonomy floor.
    pub fn new(
        namespace: LossNamespace<'_>,
        code: impl Into<String>,
        taxonomy: LossTaxonomy,
    ) -> Self {
        Self {
            namespace: LossNamespaceName(namespace.as_str().to_owned()),
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
}

impl From<NamespacedLossKind> for LossKind {
    fn from(kind: NamespacedLossKind) -> Self {
        Self::Namespaced(kind)
    }
}

impl From<LossTaxonomy> for LossKind {
    fn from(taxonomy: LossTaxonomy) -> Self {
        Self::shared(taxonomy)
    }
}

impl fmt::Display for LossKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace(), self.local_code())
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
