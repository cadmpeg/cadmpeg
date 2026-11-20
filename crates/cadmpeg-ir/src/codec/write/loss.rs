// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary charged by the sealed export wrapper.
//!
//! These codes belong to no codec: the wrapper charges them from a backend's
//! declared arena coverage, so every encoder reports them under one namespace.

use crate::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};
use crate::schema::EntityKind;

const NAMESPACE: LossNamespace<'static> = crate::loss_namespace!("export");

/// A stable, machine-readable identifier for one wrapper-charged export loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportLossCode {
    /// The write path does not represent a non-empty model arena.
    ArenaOmitted(EntityKind),
    /// A writer dropped native records of its own namespace (§3.4, W1, W3).
    NativeRecordsOmitted,
    /// The output holds no geometry (§3.5).
    OutputEmpty,
    /// A feature the target needs a state or names for is not written
    /// (§3.4, Features without their state).
    EvaluationMissing,
    /// A state whose status is `Stale` or `Failed` is written to a target
    /// that has no stale mark (§3.4, Features without their state).
    EvaluationStale,
    /// A parameter's expression is written as its cached value (§5.13).
    ParameterFlattened,
    /// A parameter is written under another name (§5.13).
    ParameterRenamed,
    /// A record's byte positions that no decoder reads have no stated value
    /// (§3.4, Residual bytes).
    RecordResidualUnavailable,
    /// A value is written without a binding the target cannot hold (§3.2).
    ValueUnwritable,
    /// A record of this kind is written with an Error the output's own
    /// validation finds (§3.4, Refusals).
    RecordInvalid(EntityKind),
}

impl ExportLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::ArenaOmitted(_) => "arena.omitted",
            Self::NativeRecordsOmitted => "native.records-omitted",
            Self::OutputEmpty => "output.empty",
            Self::EvaluationMissing => "evaluation.missing",
            Self::EvaluationStale => "evaluation.stale",
            Self::ParameterFlattened => "parameter.flattened",
            Self::ParameterRenamed => "parameter.renamed",
            Self::RecordResidualUnavailable => "record.residual-unavailable",
            Self::ValueUnwritable => "value.unwritable",
            Self::RecordInvalid(_) => "record.invalid",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::ArenaOmitted(_)
            | Self::NativeRecordsOmitted
            | Self::OutputEmpty
            | Self::EvaluationMissing
            | Self::EvaluationStale
            | Self::ParameterFlattened
            | Self::RecordResidualUnavailable
            | Self::ValueUnwritable => Severity::Warning,
            Self::ParameterRenamed => Severity::Info,
            Self::RecordInvalid(_) => Severity::Error,
        }
    }

    /// Shared category of the omitted arena's content.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        let kind = match self {
            Self::ArenaOmitted(kind) | Self::RecordInvalid(kind) => kind,
            Self::NativeRecordsOmitted => return LossTaxonomy::PassthroughRecordOmitted,
            Self::OutputEmpty => return LossTaxonomy::NoExportableSolids,
            Self::EvaluationMissing
            | Self::EvaluationStale
            | Self::ParameterFlattened
            | Self::ParameterRenamed
            | Self::RecordResidualUnavailable
            | Self::ValueUnwritable => return LossTaxonomy::ParametricRecordOmitted,
        };
        match kind {
            EntityKind::Body
            | EntityKind::Region
            | EntityKind::Shell
            | EntityKind::Face
            | EntityKind::Loop
            | EntityKind::Coedge
            | EntityKind::Edge
            | EntityKind::Vertex => LossTaxonomy::TopologyNotTransferred,
            EntityKind::Point | EntityKind::Surface | EntityKind::Curve => {
                LossTaxonomy::GeometryNotTransferred
            }
            EntityKind::SubdSurface => LossTaxonomy::SubdOmitted,
            EntityKind::Pcurve => LossTaxonomy::PcurveOmitted,
            EntityKind::ProceduralSurface | EntityKind::ProceduralCurve => {
                LossTaxonomy::ProceduralReduced
            }
            EntityKind::Asset => LossTaxonomy::AssetNotTransferred,
            EntityKind::Feature
            | EntityKind::FeatureInputTopology
            | EntityKind::FeatureResultTopology
            | EntityKind::Configuration
            | EntityKind::Parameter
            | EntityKind::Sketch
            | EntityKind::SketchEntity
            | EntityKind::SketchConstraint
            | EntityKind::SpatialSketch
            | EntityKind::SpatialSketchEntity
            | EntityKind::SpatialSketchConstraint
            | EntityKind::Spreadsheet => LossTaxonomy::ParametricRecordOmitted,
            EntityKind::ProductDefinition | EntityKind::Occurrence | EntityKind::AssemblyJoint => {
                LossTaxonomy::AssemblyPlacementsNotTransferred
            }
            EntityKind::Drawing
            | EntityKind::PresentationDocument
            | EntityKind::ViewPresentation
            | EntityKind::DeclaredMeasure => LossTaxonomy::MetadataNotTransferred,
            EntityKind::SemanticAnnotation | EntityKind::PmiAnnotation => LossTaxonomy::PmiOmitted,
            EntityKind::Tessellation => LossTaxonomy::TessellationOmitted,
            EntityKind::PointCloud => LossTaxonomy::PointCloudOmitted,
            EntityKind::HistoryBody
            | EntityKind::DesignScope
            | EntityKind::DesignContainer
            | EntityKind::DesignFeature
            | EntityKind::DesignSketch
            | EntityKind::DesignSketchEntity
            | EntityKind::DesignSketchConstraint
            | EntityKind::DesignSpatialSketch
            | EntityKind::DesignSpatialSketchEntity
            | EntityKind::DesignSpatialSketchConstraint
            | EntityKind::DesignParameter
            | EntityKind::DesignSpreadsheet
            | EntityKind::DesignSheetMetalRule
            | EntityKind::DesignProgram
            | EntityKind::DesignConfiguration
            | EntityKind::DesignArrangement
            | EntityKind::DesignPublication
            | EntityKind::DesignEvaluation => LossTaxonomy::ParametricRecordOmitted,
            EntityKind::Appearance | EntityKind::AppearanceBinding => {
                LossTaxonomy::MaterialNotTransferred
            }
            EntityKind::SourceAttribute | EntityKind::PresentationLayer => {
                LossTaxonomy::AttributesNotTransferred
            }
            EntityKind::PhysicalMaterial
            | EntityKind::MassProperties
            | EntityKind::RigidBody
            | EntityKind::Collider
            | EntityKind::Articulation
            | EntityKind::PhysicsScene
            | EntityKind::SolverSettings => LossTaxonomy::PhysicsNotTransferred,
            EntityKind::ProcessSetup
            | EntityKind::ProcessOperation
            | EntityKind::ProcessFeature
            | EntityKind::ProcessGroup
            | EntityKind::ProcessTool
            | EntityKind::ProcessPreset
            | EntityKind::ProcessMachine
            | EntityKind::ProcessProgram
            | EntityKind::ProcessParameter
            | EntityKind::ProcessToolpath
            | EntityKind::ProcessState => LossTaxonomy::ProcessRecordOmitted,
        }
    }

    fn kind(self) -> LossKind {
        NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy()).into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

/// `export:native.records-omitted`: the writer dropped these records of its
/// own native namespace. The message gives the count and then every id,
/// sorted (§3.2). `None` when `ids` is empty.
#[must_use]
pub fn native_records_omitted<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut ids = ids.into_iter().map(Into::into).collect::<Vec<String>>();
    if ids.is_empty() {
        return None;
    }
    ids.sort_unstable();
    ids.dedup();
    Some(ExportLossCode::NativeRecordsOmitted.note(format!(
        "the {format} write omits {} native record(s) {reason}: {}",
        ids.len(),
        ids.join(", ")
    )))
}

/// `export:evaluation.missing`: the writer writes no item for these
/// features. The message gives the count and then every id, sorted (§3.2).
/// `None` when `ids` is empty.
#[must_use]
pub fn evaluation_missing<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::EvaluationMissing, format, reason, ids)
}

/// `export:evaluation.stale`: the target has no stale mark, and the writer
/// writes these states, whose status is `Stale` or `Failed`, without one.
/// The message gives the count and then every entry, sorted (§3.2). `None`
/// when `ids` is empty.
#[must_use]
pub fn evaluation_stale<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::EvaluationStale, format, reason, ids)
}

/// `export:parameter.flattened`: the writer writes these parameters as their
/// cached values, or not at all where they have none (§5.13).
#[must_use]
pub fn parameter_flattened<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::ParameterFlattened, format, reason, ids)
}

/// `export:parameter.renamed`: the writer writes these parameters under
/// other names. Each entry names the parameter and both names (§5.13).
#[must_use]
pub fn parameter_renamed<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::ParameterRenamed, format, reason, ids)
}

/// `export:record.residual-unavailable`: the format specification states no
/// value for byte positions of these records (§3.4, Residual bytes).
#[must_use]
pub fn record_residual_unavailable<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(
        ExportLossCode::RecordResidualUnavailable,
        format,
        reason,
        ids,
    )
}

/// `export:value.unwritable`: the writer writes these values without a
/// binding the target cannot hold. Each entry names the record and the
/// binding (§3.2).
#[must_use]
pub fn value_unwritable<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::ValueUnwritable, format, reason, ids)
}

/// One note of `code` that lists every entry of `ids`, sorted, after their
/// count.
fn listed<I, S>(code: ExportLossCode, format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut ids = ids.into_iter().map(Into::into).collect::<Vec<String>>();
    if ids.is_empty() {
        return None;
    }
    ids.sort_unstable();
    ids.dedup();
    Some(code.note(format!(
        "the {format} write {reason} ({}): {}",
        ids.len(),
        ids.join(", ")
    )))
}

/// `export:output.empty`: the written file holds no geometry (§3.5).
#[must_use]
pub fn output_empty(message: impl Into<String>) -> LossNote {
    ExportLossCode::OutputEmpty.note(message)
}

/// `export:record.invalid`: the writer wrote these records of `kind` from the
/// canonical side, and the output's own validation finds an Error in them
/// (§3.4, Refusals). The message names each record.
#[must_use]
pub fn record_invalid(kind: EntityKind, message: impl Into<String>) -> LossNote {
    ExportLossCode::RecordInvalid(kind).note(message)
}
