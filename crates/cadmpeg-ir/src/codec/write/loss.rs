// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary charged by the sealed export wrapper.
//!
//! These codes belong to no codec: the wrapper charges some from a backend's
//! declared arena coverage, and a writer charges the others through the
//! helpers below where it meets the code's condition, so every encoder reports
//! them under one namespace with the spelling and severity of §3.2.

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
    /// The output holds a record of this kind as the source file stores it,
    /// without the document's edit (§3.4, Paths item 4).
    EditDropped(EntityKind),
    /// A writer dropped native records of its own namespace (§3.4, W1, W3).
    NativeRecordsOmitted,
    /// The output holds no geometry (§3.5).
    OutputEmpty,
    /// A partial face is not written (§3.4, Content the target cannot hold).
    FacePartial,
    /// A region that is marked or holds a marked shell or a partial face is
    /// not written as a solid (§3.4, Content the target cannot hold).
    SolidPartial,
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
    /// The output's own validation finds an Error that names no record of a
    /// model arena (§3.4, Refusals).
    RuleInvalid,
    /// A replayed or patched file holds the bytes of an `untyped` retained
    /// record (§3.4, Paths).
    SourceCarried,
    /// A patched file holds the bytes of an `untyped` retained record beside
    /// the edits it wrote (§3.4, Paths).
    SourceUnchecked,
    /// A patched file has no digest, so the writer cannot tell which records
    /// are edited (§3.4, Paths item 4).
    EditUnverified,
    /// A value is written at a coarser resolution than the document states
    /// (§3.2).
    ValueQuantized,
    /// A synthesized or empty file of a same-format source drops the bytes of
    /// an `untyped` retained record (§3.4, Paths).
    SourceDropped,
    /// A link of a patched file names the file the source names, not the
    /// caller's output name (§3.4, Paths item 4).
    LinkUnwritten,
    /// A definition no occurrence places is written placed, in a target that
    /// cannot hold an unplaced definition (§3.4, Encode sets).
    DefinitionPlaced,
    /// A fit's deviations are written as the dimension's own (§5.12).
    FitRestated,
    /// A fit is written without a tolerance (§5.12).
    FitDropped,
    /// The deviations a definition's general tolerance gives are written as
    /// each dimension's own (§8.1).
    GeneralToleranceRestated,
    /// A definition's general tolerance is written on no dimension (§8.1).
    GeneralToleranceDropped,
    /// A feature is written as an equivalent construction of the target's own
    /// features (§3.4, Lowering a typed operation).
    FeatureLowered,
    /// One edge of a dependency cycle is dropped (§3.4, Dependency cycles).
    DependencyCycleBroken,
}

impl ExportLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::ArenaOmitted(_) => "arena.omitted",
            Self::EditDropped(_) => "edit.dropped",
            Self::NativeRecordsOmitted => "native.records-omitted",
            Self::OutputEmpty => "output.empty",
            Self::FacePartial => "face.partial",
            Self::SolidPartial => "solid.partial",
            Self::EvaluationMissing => "evaluation.missing",
            Self::EvaluationStale => "evaluation.stale",
            Self::ParameterFlattened => "parameter.flattened",
            Self::ParameterRenamed => "parameter.renamed",
            Self::RecordResidualUnavailable => "record.residual-unavailable",
            Self::ValueUnwritable => "value.unwritable",
            Self::RecordInvalid(_) | Self::RuleInvalid => "record.invalid",
            Self::SourceCarried => "source.carried",
            Self::SourceUnchecked => "source.unchecked",
            Self::EditUnverified => "edit.unverified",
            Self::ValueQuantized => "value.quantized",
            Self::SourceDropped => "source.dropped",
            Self::LinkUnwritten => "link.unwritten",
            Self::DefinitionPlaced => "definition.placed",
            Self::FitRestated => "fit.restated",
            Self::FitDropped => "fit.dropped",
            Self::GeneralToleranceRestated => "general_tolerance.restated",
            Self::GeneralToleranceDropped => "general_tolerance.dropped",
            Self::FeatureLowered => "feature.lowered",
            Self::DependencyCycleBroken => "dependency.cycle-broken",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::ArenaOmitted(_)
            | Self::NativeRecordsOmitted
            | Self::OutputEmpty
            | Self::FacePartial
            | Self::SolidPartial
            | Self::EvaluationMissing
            | Self::EvaluationStale
            | Self::ParameterFlattened
            | Self::RecordResidualUnavailable
            | Self::ValueUnwritable
            | Self::SourceUnchecked
            | Self::EditUnverified
            | Self::SourceDropped
            | Self::DefinitionPlaced
            | Self::FitDropped
            | Self::GeneralToleranceDropped
            | Self::DependencyCycleBroken => Severity::Warning,
            Self::ParameterRenamed
            | Self::SourceCarried
            | Self::ValueQuantized
            | Self::FitRestated
            | Self::GeneralToleranceRestated
            | Self::FeatureLowered => Severity::Info,
            Self::EditDropped(_)
            | Self::RecordInvalid(_)
            | Self::RuleInvalid
            | Self::LinkUnwritten => Severity::Error,
        }
    }

    /// Shared category of the omitted arena's content.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        let kind = match self {
            Self::ArenaOmitted(kind) | Self::EditDropped(kind) | Self::RecordInvalid(kind) => kind,
            Self::NativeRecordsOmitted => return LossTaxonomy::PassthroughRecordOmitted,
            Self::SourceCarried | Self::SourceUnchecked | Self::SourceDropped => {
                return LossTaxonomy::RecordNotTyped;
            }
            Self::LinkUnwritten => return LossTaxonomy::AssemblyComponentsExternal,
            Self::DefinitionPlaced => return LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::OutputEmpty => return LossTaxonomy::NoExportableSolids,
            Self::FacePartial | Self::SolidPartial => {
                return LossTaxonomy::TopologyNotTransferred;
            }
            Self::EvaluationMissing
            | Self::EvaluationStale
            | Self::ParameterFlattened
            | Self::ParameterRenamed
            | Self::RecordResidualUnavailable
            | Self::ValueUnwritable
            | Self::RuleInvalid
            | Self::EditUnverified
            | Self::ValueQuantized
            | Self::FitRestated
            | Self::FitDropped
            | Self::GeneralToleranceRestated
            | Self::GeneralToleranceDropped
            | Self::FeatureLowered
            | Self::DependencyCycleBroken => return LossTaxonomy::ParametricRecordOmitted,
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

/// `export:value.quantized`: the writer writes these values at a coarser
/// resolution than the document states. Each entry names the record and the
/// value (§3.2).
#[must_use]
pub fn value_quantized<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::ValueQuantized, format, reason, ids)
}

/// `export:link.unwritten`: each of these links of a patched file names the
/// file the source names, not the caller's output name (§3.4, Paths item 4).
#[must_use]
pub fn link_unwritten<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::LinkUnwritten, format, reason, ids)
}

/// `export:definition.placed`: the target cannot hold a definition that no
/// occurrence places, and the writer writes these definitions placed (§3.4,
/// Encode sets).
#[must_use]
pub fn definition_placed<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::DefinitionPlaced, format, reason, ids)
}

/// `export:fit.restated`: the writer writes the deviations of these
/// parameters' fits as the dimensions' own (§5.12).
#[must_use]
pub fn fit_restated<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::FitRestated, format, reason, ids)
}

/// `export:fit.dropped`: the writer writes these parameters' fits as no
/// tolerance (§5.12).
#[must_use]
pub fn fit_dropped<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::FitDropped, format, reason, ids)
}

/// `export:general_tolerance.restated`: the writer writes the deviations
/// these definitions' general tolerances give as each dimension's own (§8.1).
#[must_use]
pub fn general_tolerance_restated<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(
        ExportLossCode::GeneralToleranceRestated,
        format,
        reason,
        ids,
    )
}

/// `export:general_tolerance.dropped`: the target holds no tolerance on the
/// dimensions to which these definitions' general tolerances apply, and the
/// writer writes none (§8.1).
#[must_use]
pub fn general_tolerance_dropped<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::GeneralToleranceDropped, format, reason, ids)
}

/// `export:feature.lowered`: the writer writes these features as equivalent
/// constructions of the target's own features (§3.4, Lowering a typed operation).
#[must_use]
pub fn feature_lowered<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::FeatureLowered, format, reason, ids)
}

/// `export:dependency.cycle-broken`: the target cannot hold a dependency
/// cycle, and the writer drops one edge of each cycle at these entries
/// (§3.4, Dependency cycles).
#[must_use]
pub fn dependency_cycle_broken<I, S>(format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::DependencyCycleBroken, format, reason, ids)
}

/// `export:edit.dropped`: the output holds these records of `kind` as the
/// source file stores them, without the document's edit (§3.4, Paths item 4).
/// Each entry names a record, or the file where the writer's digests do not
/// isolate the record.
#[must_use]
pub fn edit_dropped<I, S>(kind: EntityKind, format: &str, reason: &str, ids: I) -> Option<LossNote>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    listed(ExportLossCode::EditDropped(kind), format, reason, ids)
}

/// `export:edit.dropped` for one record, or the file, with a message that
/// names it and why its edit is not written (§3.4, Paths item 4).
#[must_use]
pub fn edit_dropped_note(kind: EntityKind, message: impl Into<String>) -> LossNote {
    ExportLossCode::EditDropped(kind).note(message)
}

/// `export:output.empty`: the written file holds no geometry (§3.5).
#[must_use]
pub fn output_empty(message: impl Into<String>) -> LossNote {
    ExportLossCode::OutputEmpty.note(message)
}

/// `export:face.partial`: the writer did not write the partial face `face`
/// (§3.4, Content the target cannot hold).
#[must_use]
pub fn face_partial(format: &str, face: &crate::ids::FaceId) -> LossNote {
    ExportLossCode::FacePartial.note(format!(
        "{format} writes no partial face: {face} does not state its boundary in full"
    ))
}

/// `export:solid.partial`: the writer did not write `region` as a solid,
/// because it is marked or holds a marked shell or a partial face (§3.4,
/// Content the target cannot hold). `form` names what it wrote instead.
#[must_use]
pub fn solid_partial(format: &str, region: &crate::ids::RegionId, form: &str) -> LossNote {
    ExportLossCode::SolidPartial.note(format!(
        "{format} writes no solid for {region}, which does not state every shell or face: {form}"
    ))
}

/// The losses of a writer that omits every partial face and writes no region
/// [`crate::document::Topology::complete_shells`] does not answer for as a
/// solid: `export:face.partial` naming each such face and
/// `export:solid.partial` naming each such region, with `form` saying what the
/// writer writes for it (§3.4, Content the target cannot hold).
#[must_use]
pub fn partial_topology_omitted(
    format: &str,
    geometry: &crate::document::Topology,
    form: &str,
) -> Vec<LossNote> {
    let faces = geometry
        .faces
        .iter()
        .filter(|face| face.loops.is_partial())
        .map(|face| face_partial(format, &face.id));
    let lists = geometry.child_lists();
    let regions = geometry
        .regions
        .iter()
        .filter(|region| lists.complete_shells(region).is_none())
        .map(|region| solid_partial(format, &region.id, form));
    faces.chain(regions).collect()
}

/// `export:record.invalid`: the writer wrote these records of `kind` from the
/// canonical side, and the output's own validation finds an Error in them
/// (§3.4, Refusals). The message names each record.
#[must_use]
pub fn record_invalid(kind: EntityKind, message: impl Into<String>) -> LossNote {
    ExportLossCode::RecordInvalid(kind).note(message)
}

/// `export:record.invalid` for Errors that name no record of a model arena:
/// the message names each rule (§3.4, Refusals).
#[must_use]
pub fn rule_invalid(message: impl Into<String>) -> LossNote {
    ExportLossCode::RuleInvalid.note(message)
}

/// `export:edit.unverified`: the writer patched a file of which it holds no
/// digest, so it cannot tell which records are edited. The message names the
/// file (§3.4, Paths item 4).
#[must_use]
pub fn edit_unverified(message: impl Into<String>) -> LossNote {
    ExportLossCode::EditUnverified.note(message)
}

/// The losses of a file that `path` wrote from the retained records of
/// `fidelity`: `export:source.carried` for a replay or a patch that holds the
/// bytes of an `untyped` record, and `export:source.unchecked` beside it for a
/// patch. Each note names the records and the classes they can hold (§3.4,
/// Paths).
#[must_use]
pub fn carried_source_losses(fidelity: Option<&SourceFidelity>, path: &WritePath) -> Vec<LossNote> {
    let patched = match path {
        WritePath::Synthesized { .. } => return Vec::new(),
        WritePath::VerbatimReplay => false,
        WritePath::Patched { .. } => true,
    };
    let records = untyped_records(fidelity);
    if records.is_empty() {
        return Vec::new();
    }
    let records = records.join("; ");
    let mut losses = vec![ExportLossCode::SourceCarried.note(format!(
        "the file holds the bytes of untyped retained records, content the document does not \
         state: {records}"
    ))];
    if patched {
        losses.push(ExportLossCode::SourceUnchecked.note(format!(
            "the patched file holds the bytes of untyped retained records beside the edits it \
             wrote, unchecked against them: {records}"
        )));
    }
    losses
}

/// `export:source.dropped` for a synthesized file of a same-format source:
/// the note names each `untyped` retained record of `fidelity` and its
/// classes (§3.4, Paths). `None` when `fidelity` holds no such record.
pub(super) fn dropped_source_loss(fidelity: Option<&SourceFidelity>) -> Option<LossNote> {
    let records = untyped_records(fidelity);
    if records.is_empty() {
        return None;
    }
    Some(ExportLossCode::SourceDropped.note(format!(
        "the synthesized file holds none of the bytes of the untyped retained records of its \
         source file: {}",
        records.join("; ")
    )))
}

/// Each `untyped` retained record of `fidelity`, with the classes it can hold.
fn untyped_records(fidelity: Option<&SourceFidelity>) -> Vec<String> {
    fidelity
        .into_iter()
        .flat_map(SourceFidelity::retained_records)
        .filter(|(_, record)| !record.untyped().is_empty())
        .map(|(id, record)| format!("{id} ({})", class_list(record.untyped())))
        .collect()
}

#[cfg(test)]
mod tests;
