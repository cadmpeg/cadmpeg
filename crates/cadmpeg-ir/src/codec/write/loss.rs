// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary shared by every writer.
//!
//! These codes belong to no codec. The wrapper charges [`ExportLossCode`] from
//! a backend's declared arena coverage; a writer charges [`WriterLossCode`]
//! and [`TargetLossCode`] where it meets the code's condition. Each code is
//! spelled and graded as CADIR v11 §3.2 states it, with no strict floor.

use crate::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};
use crate::schema::EntityKind;
use crate::source_fidelity::{class_list, SourceFidelity};

use super::WritePath;

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

/// A shared `export:` loss that a writer charges where it meets the condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum WriterLossCode {
    /// A replayed or patched file holds the bytes of an `untyped` retained
    /// record.
    SourceCarried,
    /// A patched file holds the bytes of an `untyped` retained record beside
    /// the edits it wrote.
    SourceUnchecked,
    /// A synthesized or empty file of a same-format source drops the bytes of
    /// an `untyped` retained record.
    SourceDropped,
    /// The output does not hold an edit.
    EditDropped,
    /// A patched file has no digest, so its edits are not known.
    EditUnverified,
    /// A link names another file than the caller's output name.
    LinkUnwritten,
    /// A record is written despite a validation Error, or the finding names
    /// no record.
    RecordInvalid,
    /// A non-finite value or a value wider than its field is degraded,
    /// dropped or defaulted.
    ValueUnwritable,
    /// An unplaced top definition is written under a placement the document
    /// does not state.
    DefinitionPlaced,
}

impl WriterLossCode {
    /// The stable code within the `export` namespace.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::SourceCarried => "source.carried",
            Self::SourceUnchecked => "source.unchecked",
            Self::SourceDropped => "source.dropped",
            Self::EditDropped => "edit.dropped",
            Self::EditUnverified => "edit.unverified",
            Self::LinkUnwritten => "link.unwritten",
            Self::RecordInvalid => "record.invalid",
            Self::ValueUnwritable => "value.unwritable",
            Self::DefinitionPlaced => "definition.placed",
        }
    }

    /// The severity §3.2 gives the code.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::SourceCarried => Severity::Info,
            Self::EditDropped | Self::LinkUnwritten | Self::RecordInvalid => Severity::Error,
            Self::SourceUnchecked
            | Self::SourceDropped
            | Self::EditUnverified
            | Self::ValueUnwritable
            | Self::DefinitionPlaced => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::SourceCarried | Self::SourceUnchecked => LossTaxonomy::RecordNotTyped,
            Self::SourceDropped => LossTaxonomy::PassthroughRecordOmitted,
            Self::EditDropped | Self::EditUnverified => LossTaxonomy::ParametricRecordOmitted,
            Self::LinkUnwritten => LossTaxonomy::AssemblyComponentsExternal,
            Self::RecordInvalid => LossTaxonomy::DecodeDiagnostic,
            Self::ValueUnwritable => LossTaxonomy::GeometryNotTransferred,
            Self::DefinitionPlaced => LossTaxonomy::AssemblyPlacementsNotTransferred,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        let kind = NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy())
            .with_strict_floor(None);
        LossNote::new(kind, message).with_severity(self.severity())
    }
}

/// A shared `target.` loss, charged in the writing codec's namespace beside
/// its `target.source-dialect-displaced`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TargetLossCode {
    /// An explicitly requested row is written as another.
    RequestDisplaced,
}

impl TargetLossCode {
    /// The stable code within the writing codec's namespace.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::RequestDisplaced => "target.request-displaced",
        }
    }

    /// The severity §3.2 gives the code.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::RequestDisplaced => Severity::Warning,
        }
    }

    /// Build a [`LossNote`] for this code in `codec`'s namespace.
    #[must_use]
    pub fn note(self, codec: LossNamespace<'_>, message: impl Into<String>) -> LossNote {
        let kind =
            NamespacedLossKind::new(codec, self.code(), LossTaxonomy::SourceDialectDisplaced)
                .with_strict_floor(None);
        LossNote::new(kind, message).with_severity(self.severity())
    }
}

/// The losses of a file that `path` wrote from the retained records of
/// `fidelity`: `export:source.carried` for a replay or a patch that holds the
/// bytes of an `untyped` record, and `export:source.unchecked` beside it for a
/// patch. Each note names the records and the classes they can hold.
#[must_use]
pub fn carried_source_losses(fidelity: Option<&SourceFidelity>, path: &WritePath) -> Vec<LossNote> {
    let codes: &[WriterLossCode] = match path {
        WritePath::Synthesized { .. } => return Vec::new(),
        WritePath::VerbatimReplay => &[WriterLossCode::SourceCarried],
        WritePath::Patched { .. } => &[
            WriterLossCode::SourceCarried,
            WriterLossCode::SourceUnchecked,
        ],
    };
    let records: Vec<String> = fidelity
        .into_iter()
        .flat_map(SourceFidelity::retained_records)
        .filter(|(_, record)| !record.untyped().is_empty())
        .map(|(id, record)| format!("{id} ({})", class_list(record.untyped())))
        .collect();
    if records.is_empty() {
        return Vec::new();
    }
    let records = records.join("; ");
    codes
        .iter()
        .map(|code| match code {
            WriterLossCode::SourceUnchecked => code.note(format!(
                "the patched file holds the bytes of untyped retained records beside the edits \
                 it wrote, unchecked against them: {records}"
            )),
            _ => code.note(format!(
                "the file holds the bytes of untyped retained records, content the document \
                 does not state: {records}"
            )),
        })
        .collect()
}

#[cfg(test)]
mod tests;
