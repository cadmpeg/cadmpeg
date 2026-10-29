// SPDX-License-Identifier: Apache-2.0
//! Shared export loss vocabulary for writers and the sealed wrapper.
//!
//! Fixed factories keep the namespace, code and severity consistent. The
//! wrapper charges arena omissions from each backend's declared coverage.

use crate::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};
use crate::schema::EntityKind;

const NAMESPACE: LossNamespace<'static> = crate::loss_namespace!("export");

/// A stable, machine-readable identifier for one shared export loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportLossCode {
    /// The write path does not represent a non-empty model arena.
    ArenaOmitted(EntityKind),
    /// Typed native records were omitted by this writer.
    NativeRecordsOmitted,
    /// A record residual is absent, so a target field uses its specified default.
    RecordResidualUnavailable,
    /// A target field stores a value at reduced precision.
    ValueQuantized,
    /// The output contains no exportable content.
    OutputEmpty,
    /// Stored feature evaluation is stale and the output has no recompute flag.
    EvaluationStale,
}

impl ExportLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::ArenaOmitted(_) => "arena.omitted",
            Self::NativeRecordsOmitted => "native.records-omitted",
            Self::EvaluationStale => "evaluation.stale",
            Self::RecordResidualUnavailable => "record.residual-unavailable",
            Self::ValueQuantized => "value.quantized",
            Self::OutputEmpty => "output.empty",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::ArenaOmitted(_) => Severity::Warning,
            Self::NativeRecordsOmitted | Self::ValueQuantized => Severity::Info,
            Self::RecordResidualUnavailable | Self::OutputEmpty => Severity::Warning,
            Self::EvaluationStale => Severity::Warning,
        }
    }

    /// Shared category of the omitted arena's content.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        let kind = match self {
            Self::ArenaOmitted(kind) => kind,
            Self::NativeRecordsOmitted => return LossTaxonomy::RecordNotTyped,
            Self::EvaluationStale => return LossTaxonomy::FeatureHistoryRetained,
            Self::RecordResidualUnavailable => return LossTaxonomy::PreservedSourceUnavailable,
            Self::ValueQuantized => return LossTaxonomy::ValueQuantized,
            Self::OutputEmpty => return LossTaxonomy::NoExportableSolids,
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
            | EntityKind::DesignConfiguration
            | EntityKind::DesignParameter
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
            | EntityKind::ViewPresentation => LossTaxonomy::MetadataNotTransferred,
            EntityKind::SemanticAnnotation | EntityKind::PmiAnnotation => LossTaxonomy::PmiOmitted,
            EntityKind::Tessellation => LossTaxonomy::TessellationOmitted,
            EntityKind::Appearance | EntityKind::AppearanceBinding => {
                LossTaxonomy::MaterialNotTransferred
            }
            EntityKind::SourceAttribute | EntityKind::PresentationLayer => {
                LossTaxonomy::AttributesNotTransferred
            }
        }
    }

    fn kind(self) -> LossKind {
        NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy()).into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests;
