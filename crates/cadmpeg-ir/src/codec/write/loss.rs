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
}

impl ExportLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::ArenaOmitted(_) => "arena.omitted",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::ArenaOmitted(_) => Severity::Warning,
        }
    }

    /// Shared category of the omitted arena's content.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        let Self::ArenaOmitted(kind) = self;
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
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}
