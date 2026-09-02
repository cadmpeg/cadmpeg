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
    pub fn as_str(self) -> String {
        let Ok(serde_json::Value::String(name)) = serde_json::to_value(self) else {
            unreachable!("a fieldless loss taxonomy always serializes as a string")
        };
        name
    }

    /// Parse a v1 bare `snake_case` taxonomy identifier.
    pub fn from_v1_str(text: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
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
        f.write_str(&self.as_str())
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
            code: taxonomy.as_str(),
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
        self.to_string()
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
