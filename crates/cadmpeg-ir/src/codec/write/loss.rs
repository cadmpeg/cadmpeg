// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary charged by the sealed export wrapper.
//!
//! These codes belong to no codec: the wrapper charges them from a backend's
//! declared arena coverage and from the validation the caller gives it, so
//! every encoder reports them under one namespace.

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
    /// A record is written despite a validation Error, or the finding names
    /// no record.
    RecordInvalid,
}

impl ExportLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::ArenaOmitted(_) => "arena.omitted",
            Self::RecordInvalid => "record.invalid",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::ArenaOmitted(_) => Severity::Warning,
            Self::RecordInvalid => Severity::Error,
        }
    }

    /// Shared category of the omitted arena's content.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        let kind = match self {
            Self::ArenaOmitted(kind) => kind,
            Self::RecordInvalid => return LossTaxonomy::DecodeDiagnostic,
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
