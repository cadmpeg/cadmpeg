// SPDX-License-Identifier: Apache-2.0
//! Decode-loss and export-fidelity vocabulary.
//!
//! These types describe what a decode or export transferred and where it fell
//! short: [`Severity`] and [`LossCategory`] classify a [`LossNote`], [`LossCode`]
//! names a stable machine-readable loss kind with its [`StrictConsequence`], and
//! [`DecodeReport`]/[`ExportReport`] collect the losses and census counts for one
//! operation. The validation vocabulary lives in [`crate::validate`].

use std::collections::BTreeMap;
use std::fmt;

use crate::provenance::Provenance;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity of a loss note or validation finding.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
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
            Self::Other => "other",
        })
    }
}

/// Strict-mode handling for a loss code.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum StrictConsequence {
    /// Strict mode must refuse the operation.
    Reject,
    /// Strict mode may proceed.
    Tolerate,
}

/// Stable machine-readable loss kinds.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LossCode {
    /// Container-only decode was requested; entity decode was not attempted.
    ContainerOnly,
    /// No geometry stream was located in the container, so no B-rep could be
    /// transferred.
    MissingGeometryStream,
    /// The B-rep topology graph was not transferred, though carriers or a
    /// container were decoded.
    TopologyNotTransferred,
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
    /// A record was decoded but yielded no typed IR entity.
    RecordNotTyped,
    /// A decode-time diagnostic surfaced as a loss note; detail is in the
    /// message.
    DecodeDiagnostic,
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
}

impl LossCode {
    /// The stable `snake_case` identifier, matching the serialized form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ContainerOnly => "container_only",
            Self::MissingGeometryStream => "missing_geometry_stream",
            Self::TopologyNotTransferred => "topology_not_transferred",
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
            Self::RecordNotTyped => "record_not_typed",
            Self::DecodeDiagnostic => "decode_diagnostic",
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
        }
    }

    /// Returns the strict-mode consequence of this code.
    pub fn strict_consequence(self) -> StrictConsequence {
        match self {
            Self::MissingGeometryStream
            | Self::TopologyNotTransferred
            | Self::GeometryNotTransferred
            | Self::ReferenceGraphNotClosed
            | Self::CurvelessEdgeOmitted
            | Self::UnknownSurfaceFaceOmitted
            | Self::SubdOmitted
            | Self::NoExportableSolids => StrictConsequence::Reject,
            _ => StrictConsequence::Tolerate,
        }
    }
}

impl fmt::Display for LossCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One attributable instance of incomplete or approximate transfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LossNote {
    /// Stable machine-readable loss kind.
    pub code: LossCode,
    /// Affected subsystem.
    pub category: LossCategory,
    /// How serious the loss is.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
    /// Where in the source the loss occurred, when attributable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

/// Transfer status and loss details from a successful decode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DecodeReport {
    /// Source format id.
    pub format: String,
    /// Whether the decode stopped at the container layer (no entity decode).
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
}

impl DecodeReport {
    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|l| l.severity >= Severity::Error)
            .count()
    }
}

/// Entity census and fidelity details from a successful export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExportReport {
    /// Target format id.
    pub format: String,
    /// Exported entity counts keyed by target entity kind.
    pub entity_counts: BTreeMap<String, usize>,
    /// Total exported entities.
    pub total_entities: usize,
    /// Omitted, normalized, or reduced content.
    pub losses: Vec<LossNote>,
    /// Informational details about the export path.
    pub notes: Vec<String>,
}

impl ExportReport {
    /// Count loss notes at or above [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|loss| loss.severity >= Severity::Error)
            .count()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn loss_code_serializes_under_its_stable_identifier() {
        let note = LossNote {
            code: LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "topology graph not transferred".to_owned(),
            provenance: None,
        };
        let value: serde_json::Value = serde_json::to_value(&note).expect("required invariant");
        assert_eq!(value["code"], "topology_not_transferred");
        assert_eq!(value["code"], LossCode::TopologyNotTransferred.as_str());
    }

    #[test]
    fn loss_code_carries_reversibility_and_strict_consequence() {
        assert_eq!(
            LossCode::TopologyNotTransferred.strict_consequence(),
            StrictConsequence::Reject
        );
        assert_eq!(
            LossCode::PassthroughRecordOmitted.strict_consequence(),
            StrictConsequence::Tolerate
        );
    }
}
