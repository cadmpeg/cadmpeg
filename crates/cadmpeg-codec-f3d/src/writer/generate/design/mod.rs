// SPDX-License-Identifier: Apache-2.0
//! The geometry-true form of a design scope (cadir-v11.md §3.4, Features
//! without their state): the scope's parameters, and one stored-geometry
//! feature of the current bodies.
//!
//! It runs where no native Design record is written and the document holds a
//! design scope for its root definition. It charges a loss for every design
//! scope, container, feature and parameter it does not write, for the root
//! scope's configuration, arrangements and occurrence states, for every
//! current state's `Stale` or `Failed` status and poses, and for every stored
//! evaluation. A failure here is a [`super::reduce::Stage::NeutralDesign`]
//! failure. Its fallbacks, in [`Part`] order, write the sketches without
//! their dimensions, the extrude in the Base Feature, the design without its
//! `Sketch` items, then no design ([`dropped_records`]): they never drop a
//! body.

mod base_feature;
mod expression;
mod extrude;
pub(super) mod frames;
pub(super) mod history;
pub(super) mod parameters;
mod roster;
mod sketch;
#[cfg(test)]
mod tests;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

use cadmpeg_ir::codec::write::{
    evaluation_missing, evaluation_stale, ArenaDisposition, DesignArenaDispositions,
};
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::parameter::ParameterOwner;
use cadmpeg_ir::design::refs::{Evaluated, Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::StepRef;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::report::loss::LossNote;

use super::presentation::GeneratedDesignRegistry;
use super::reduce::named_loss;
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use base_feature::BaseFeaturePlan;
use extrude::Decision;
use frames::DesignRecords;
use parameters::ParameterPlan;
use sketch::SketchPlan;

/// The smallest linear tolerance a dimension is checked with, in mm: the
/// decoder's linear resolution.
const MIN_LINEAR_TOLERANCE: f64 = 1e-5;
/// Millimetres per Fusion length unit (cm).
const MM_PER_CM: f64 = 10.0;

#[cfg(test)]
thread_local! {
    /// Makes the next attempt that writes the design scope on this thread
    /// fail in the design module.
    static FAIL_NEXT_PLAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
thread_local! {
    /// Makes the next attempt that writes a `Sketch` item on this thread
    /// fail in the sketch records.
    static FAIL_NEXT_SKETCHES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The failure [`FAIL_NEXT_SKETCHES`] requests, once.
#[cfg(test)]
fn requested_sketch_failure() -> Option<cadmpeg_core::CodecError> {
    FAIL_NEXT_SKETCHES
        .with(|fail| fail.replace(false))
        .then(|| {
            cadmpeg_core::CodecError::NotImplemented("a test failure of the sketch records".into())
        })
}

#[cfg(test)]
thread_local! {
    /// Makes the next attempt that writes a sketch dimension on this thread
    /// fail in the dimension records.
    static FAIL_NEXT_DIMENSIONS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The failure [`FAIL_NEXT_DIMENSIONS`] requests, once.
#[cfg(test)]
fn requested_dimension_failure() -> Option<cadmpeg_core::CodecError> {
    FAIL_NEXT_DIMENSIONS
        .with(|fail| fail.replace(false))
        .then(|| {
            cadmpeg_core::CodecError::NotImplemented(
                "a test failure of the dimension records".into(),
            )
        })
}

#[cfg(test)]
thread_local! {
    /// Makes the next attempt that writes an `Extrude` item on this thread
    /// fail in the `Extrude` records.
    static FAIL_NEXT_EXTRUDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The failure [`FAIL_NEXT_EXTRUDE`] requests, once.
#[cfg(test)]
fn requested_extrude_failure() -> Option<cadmpeg_core::CodecError> {
    FAIL_NEXT_EXTRUDE.with(|fail| fail.replace(false)).then(|| {
        cadmpeg_core::CodecError::NotImplemented("a test failure of the Extrude records".into())
    })
}

/// The failure [`FAIL_NEXT_PLAN`] requests, once.
#[cfg(test)]
pub(super) fn requested_test_failure() -> Option<cadmpeg_core::CodecError> {
    FAIL_NEXT_PLAN.with(|fail| fail.replace(false)).then(|| {
        cadmpeg_core::CodecError::NotImplemented("a test failure of the design module".into())
    })
}

/// The design arenas where the design is dropped: [`dropped_records`]
/// charges each of their records.
pub(super) const DROPPED: DesignArenaDispositions = DesignArenaDispositions {
    scopes: ArenaDisposition::Dropped,
    containers: ArenaDisposition::Dropped,
    features: ArenaDisposition::Dropped,
    parameters: ArenaDisposition::Dropped,
    evaluations: ArenaDisposition::Dropped,
    ..DesignArenaDispositions::uniform(ArenaDisposition::Omitted)
};

/// A part of the design scope's form the writer drops after a failure with
/// it, in the order it drops them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Part {
    /// The sketch dimensions: their parameters stay user parameters.
    Dimensions,
    /// The `Extrude` item: the Base Feature holds the extrude.
    Extrude,
    /// The `Sketch` items.
    Sketches,
}

impl Part {
    const ORDER: [Self; 3] = [Self::Dimensions, Self::Extrude, Self::Sketches];
}

/// The parts the writer has dropped, each with the failure that dropped it.
#[derive(Clone, Debug, Default)]
pub(super) struct Fallback {
    dropped: Vec<(Part, String)>,
}

impl Fallback {
    /// Why `part` is dropped.
    fn reason(&self, part: Part) -> Option<&str> {
        self.dropped
            .iter()
            .find(|(dropped, _)| *dropped == part)
            .map(|(_, reason)| reason.as_str())
    }

    /// Drop the first part after the parts already dropped that the design
    /// of `target` writes, for `reason`. Returns whether a part was dropped.
    pub(super) fn drop_next(&mut self, target: &CadIr, reason: &str) -> bool {
        let after = self.dropped.last().map(|(part, _)| *part);
        for part in Part::ORDER
            .into_iter()
            .filter(|part| after.is_none_or(|after| *part > after))
        {
            let writes = plan(target, self)
                .ok()
                .flatten()
                .is_some_and(|design| match part {
                    Part::Dimensions => design.writes_dimensions(),
                    Part::Extrude => matches!(design.extrude, Decision::Extrude(_)),
                    Part::Sketches => design.writes_sketches(),
                });
            if writes {
                self.dropped.push((part, reason.to_owned()));
                return true;
            }
        }
        false
    }
}

/// What the writer writes of the root definition's design scope.
pub(super) struct NeutralDesign<'a> {
    parameters: ParameterPlan,
    sketches: SketchPlan<'a>,
    /// The scope's extrude, written as an `Extrude` item in place of the
    /// Base Feature where the rule of [`extrude::plan`] allows it.
    extrude: Decision<'a>,
    base_feature: BaseFeaturePlan<'a>,
    losses: Vec<LossNote>,
}

/// The plan for the root definition's design scope; `None` when the
/// document holds none. Each part `fallback` drops is written in its
/// fallback form and charged with the failure that dropped it.
pub(super) fn plan<'a>(
    target: &'a CadIr,
    fallback: &Fallback,
) -> Result<Option<NeutralDesign<'a>>, CodecError> {
    let Some(scope) = root_scope(target) else {
        return Ok(None);
    };
    let mut parameters = parameters::plan(target, &scope.definition);
    let features = scope_features(target, scope);
    let mut sketches = sketch_plan(target, &features);
    let mut ordinal = 1;
    if let Some(reason) = fallback.reason(Part::Sketches) {
        sketches.drop_items(reason);
    } else if let Some(reason) = fallback.reason(Part::Dimensions) {
        sketches.drop_dimensions(reason);
    } else {
        ordinal = sketches.claim_dimensions(
            &mut parameters,
            sketch::dimensions::Tolerances {
                linear: target.tolerances.linear.get().max(MIN_LINEAR_TOLERANCE) / MM_PER_CM,
                angular: target.tolerances.angular.get(),
            },
        );
    }
    let extrude = if target.geometry.bodies.is_empty() {
        Decision::None
    } else {
        extrude::plan(
            target,
            scope,
            &features,
            &sketches,
            &mut parameters,
            fallback.reason(Part::Extrude),
            ordinal,
        )
    };
    let written = parameters.written().collect::<BTreeSet<_>>();
    let mut owned = BTreeMap::<&FeatureId, Vec<String>>::new();
    for parameter in &target.design.parameters {
        if let Some(ParameterOwner::Feature(feature)) = &parameter.owner {
            if written.contains(&parameter.id) {
                owned
                    .entry(feature)
                    .or_default()
                    .push(parameter.id.to_string());
            }
        }
    }
    let unwritten = features
        .iter()
        .filter(|feature| !sketches.writes(feature) && !extrude.writes(feature))
        .map(|feature| {
            let mut notes = Vec::new();
            if let Some(reason) = sketches.unwritten.get(*feature) {
                notes.push(format!("its sketch {reason}"));
            }
            if let Some(reason) = extrude.reason(feature) {
                notes.push(format!("the Base Feature holds it: {reason}"));
            }
            if let Some(parameters) = owned.get(*feature) {
                notes.push(format!(
                    "its parameters are written as user parameters: {}",
                    parameters.join(" ")
                ));
            }
            if notes.is_empty() {
                feature.to_string()
            } else {
                format!("{feature} ({})", notes.join("; "))
            }
        });
    let reason = if matches!(extrude, Decision::Extrude(_)) {
        "stores no timeline item for these features of the design"
    } else {
        "stores the current bodies as one Base Feature and no timeline item for these features of the design"
    };
    let mut losses = evaluation_missing("F3D", reason, unwritten)
        .into_iter()
        .collect::<Vec<_>>();
    losses.extend(unwritten_records(target, scope));
    losses.extend(states(target));
    let base_feature = base_feature::plan(target, sketches.items.len())?;
    Ok(Some(NeutralDesign {
        parameters,
        sketches,
        extrude,
        base_feature,
        losses,
    }))
}

impl NeutralDesign<'_> {
    /// Whether the design stores a body: its B-rep stream then carries the
    /// Base Feature's names and history.
    pub(super) fn has_bodies(&self) -> bool {
        self.base_feature.has_item()
    }

    /// Name the result bodies and their faces in `native`, whose records the
    /// B-rep stream is written from.
    pub(super) fn stamp(&mut self, native: &mut F3dNative) -> Result<(), CodecError> {
        match &self.extrude {
            Decision::Extrude(plan) => extrude::stamp(plan, &self.sketches.items, native),
            Decision::None | Decision::BaseFeature { .. } => {
                let losses = self.base_feature.stamp(native)?;
                self.losses.extend(losses);
                Ok(())
            }
        }
    }

    /// The parameter records and the design's records, with their types
    /// registered in `registry`.
    pub(super) fn records(
        &mut self,
        registry: &mut GeneratedDesignRegistry,
    ) -> Result<(Vec<u8>, DesignRecords), CodecError> {
        let (parameter_bytes, records, losses) =
            self.base_feature.encode(&self.parameters, registry)?;
        self.losses.extend(losses);
        Ok((parameter_bytes, records))
    }

    /// The members of the design folder of a design with result bodies:
    /// the Design, ACT and Browser segments, the history stream `smbh`, the
    /// snapshot stream `smb` and the members their records name (f3d.md §3.1, "Records of a Base
    /// Feature design").
    pub(super) fn archive(
        &mut self,
        target: &CadIr,
        smbh: &[u8],
        smb: &[u8],
    ) -> Result<Vec<(String, Vec<u8>)>, CodecError> {
        let scope = root_scope(target)
            .ok_or_else(|| CodecError::Malformed("the design has no root scope".into()))?;
        let bodies = self.base_feature.roster_bodies(target);
        let extrude = match &self.extrude {
            Decision::Extrude(plan) => Some(plan.as_ref()),
            Decision::None | Decision::BaseFeature { .. } => None,
        };
        let roster = roster::Roster::new(
            bodies.len(),
            self.parameters.written_count(),
            &self.sketches.items,
            extrude,
        )?;
        let ctx = roster::Context {
            table: roster::type_table(&roster, bodies.iter().any(|body| body.sheet))?,
            roster,
            values: roster::values::Values::new(target, &scope.definition, smbh),
            bodies,
            sketches: &self.sketches.items,
            extrude,
        };
        let mut records = DesignRecords::default();
        BaseFeaturePlan::roster_records(&ctx, &self.parameters, &mut records)?;
        let (members, losses) = roster::archive(&ctx, records, smbh, smb)?;
        self.losses.extend(losses);
        Ok(members)
    }

    /// Whether the design writes a `Sketch` item.
    pub(super) fn writes_sketches(&self) -> bool {
        !self.sketches.items.is_empty()
    }

    /// Whether the design writes a sketch dimension.
    fn writes_dimensions(&self) -> bool {
        self.sketches
            .items
            .iter()
            .any(|item| !item.dimensions.is_empty())
    }

    pub(super) fn into_losses(self) -> Vec<LossNote> {
        let mut losses = self.losses;
        losses.extend(self.parameters.into_losses());
        losses.extend(self.sketches.into_losses());
        losses
    }
}

/// The plan of the sketches among `features`. The `Sketch` items precede
/// the `Base Feature`, which a design with no body does not write.
fn sketch_plan<'a>(target: &'a CadIr, features: &[&'a FeatureId]) -> SketchPlan<'a> {
    if target.geometry.bodies.is_empty() {
        SketchPlan::default()
    } else {
        sketch::plan(target, features)
    }
}

/// The design scope of the root definition: the document's one scope, or
/// the scope whose definition no occurrence places.
pub(super) fn root_scope(target: &CadIr) -> Option<&DesignScope> {
    match target.design.scopes.as_slice() {
        [] => None,
        [scope] => Some(scope),
        scopes => {
            let placed = target
                .model
                .occurrences
                .iter()
                .map(|occurrence| &occurrence.prototype)
                .collect::<BTreeSet<_>>();
            scopes
                .iter()
                .find(|scope| !placed.contains(&scope.definition))
        }
    }
}

/// The ids of the root scope's design parameters: the neutral parameters
/// they restate are written or charged with them.
pub(super) fn restated_parameters(target: &CadIr) -> BTreeSet<&str> {
    root_scope(target).map_or_else(BTreeSet::new, |scope| {
        target
            .design
            .parameters
            .iter()
            .filter(|parameter| parameter.scope == scope.definition)
            .map(|parameter| parameter.id.as_str())
            .collect()
    })
}

/// The losses of a design the writer drops: every design scope, container,
/// feature, parameter and evaluation of the document, and the current
/// states' values.
pub(super) fn dropped_records(target: &CadIr, reason: &str) -> Vec<LossNote> {
    let design = &target.design;
    let unwritten = format!("the F3D writer writes no design ({reason})");
    let mut losses = states(target);
    losses.extend(evaluation_missing(
        "F3D",
        &format!("stores no design for these features ({reason})"),
        design.features.iter().map(|feature| feature.id.to_string()),
    ));
    losses.extend(named_loss(
        F3dLossCode::WriterParameterOmitted,
        "parameter",
        &unwritten,
        design
            .parameters
            .iter()
            .map(|parameter| parameter.id.to_string()),
    ));
    losses.extend(named_loss(
        F3dLossCode::WriterDesignRecordOmitted,
        "design scope",
        &unwritten,
        design
            .scopes
            .iter()
            .map(|scope| scope.definition.to_string()),
    ));
    losses.extend(named_loss(
        F3dLossCode::WriterDesignRecordOmitted,
        "design container",
        &unwritten,
        design
            .containers
            .iter()
            .map(|container| container.id().to_string()),
    ));
    losses
}

/// The losses of the scopes' states: a current state's `Stale` or `Failed`
/// status and each pose, which the written bodies do not carry, and every
/// evaluation, which no written record holds.
fn states(target: &CadIr) -> Vec<LossNote> {
    let mut stale = Vec::new();
    let mut poses = Vec::new();
    let mut evaluations = Vec::new();
    for scope in &target.design.scopes {
        let definition = &scope.definition;
        for state in &scope.current {
            let at = position(&state.at);
            match state.status {
                EvaluationStatus::Current => {}
                EvaluationStatus::Stale => {
                    stale.push(format!("{definition} current state {at} (stale)"));
                }
                EvaluationStatus::Failed => {
                    stale.push(format!("{definition} current state {at} (failed)"));
                }
            }
            poses.extend(state.poses.keys().map(|path| {
                let steps = path
                    .0
                    .iter()
                    .map(|step| match &step.occurrence {
                        StepRef::Occurrence(id) => id.to_string(),
                        StepRef::Native(name) => format!("{:?}", name.value),
                    })
                    .collect::<Vec<_>>()
                    .join("/");
                format!("{definition} pose of `{steps}` in the current state {at}")
            }));
        }
    }
    for evaluation in &target.design.evaluations {
        let of = match &evaluation.of {
            Evaluated::State(at) => format!("state {}", position(at)),
            Evaluated::Tool(feature) => format!("tool body of {feature}"),
        };
        let configuration = evaluation
            .configuration
            .as_ref()
            .map_or_else(String::new, |id| format!(" in configuration {id}"));
        let entry = format!(
            "{}: {} {of}{configuration}",
            evaluation.id, evaluation.scope
        );
        match evaluation.status {
            EvaluationStatus::Current => evaluations.push(entry),
            EvaluationStatus::Stale => stale.push(format!("{entry} (stale)")),
            EvaluationStatus::Failed => stale.push(format!("{entry} (failed)")),
        }
    }
    let mut losses = Vec::new();
    losses.extend(evaluation_stale(
        "F3D",
        "stores no stale or failure mark: it writes these current states as current bodies and writes no evaluation",
        stale,
    ));
    losses.extend(named_loss(
        F3dLossCode::WriterDesignRecordOmitted,
        "current state value",
        "the F3D writer writes no occurrence pose of a current state",
        poses,
    ));
    losses.extend(named_loss(
        F3dLossCode::WriterDesignRecordOmitted,
        "evaluation",
        "the F3D design holds its current bodies alone, with no stored evaluation",
        evaluations,
    ));
    losses
}

fn position(at: &Position) -> String {
    match at {
        Position::Start(container) => format!("at the start of {container}"),
        Position::After(member) => format!("after {}", member_id(member)),
    }
}

/// The losses of the design records outside the root scope's written form:
/// the root scope's configuration, arrangements and occurrence states, the
/// other scopes with their features and parameters, every container but the
/// one the written timeline stands for, and that container's folders,
/// rollback and placement.
fn unwritten_records(target: &CadIr, root: &DesignScope) -> Vec<LossNote> {
    let others = move || {
        target
            .design
            .scopes
            .iter()
            .filter(move |scope| !std::ptr::eq(*scope, root))
    };
    let charged = scope_features(target, root)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut placed = BTreeMap::new();
    for scope in others() {
        for feature in scope_features(target, scope) {
            placed.entry(feature).or_insert(&scope.definition);
        }
    }
    let mut losses = Vec::new();
    losses.extend(named_loss(
        F3dLossCode::WriterDesignRecordOmitted,
        "design scope",
        &format!(
            "the F3D writer writes the design of the root definition {} alone",
            root.definition
        ),
        others().map(|scope| scope.definition.to_string()),
    ));
    let definition = &root.definition;
    let configuration = root
        .active_configuration
        .iter()
        .map(|id| format!("{definition} active configuration {id}"));
    let arrangements = root
        .active_arrangements
        .iter()
        .map(|id| format!("{definition} active arrangement {id}"));
    let occurrences = root
        .occurrences
        .keys()
        .map(|id| format!("{definition} design state of occurrence {id}"));
    losses.extend(named_loss(
        F3dLossCode::WriterDesignRecordOmitted,
        "design scope value",
        "the F3D design holds its current bodies alone, with no configuration, arrangement or occurrence state",
        configuration.chain(arrangements).chain(occurrences),
    ));
    losses.extend(evaluation_missing(
        "F3D",
        &format!(
            "writes the design of the root definition {} alone, and no timeline item for these features",
            root.definition
        ),
        target
            .design
            .features
            .iter()
            .map(|feature| &feature.id)
            .filter(|feature| !charged.contains(feature))
            .map(|feature| match placed.get(feature) {
                Some(definition) => format!("{feature} (design scope of {definition})"),
                None => format!("{feature} (in no design scope)"),
            }),
    ));
    losses.extend(named_loss(
        F3dLossCode::WriterParameterOmitted,
        "parameter",
        &format!(
            "the F3D writer writes the parameters of the root definition {} alone",
            root.definition
        ),
        target
            .design
            .parameters
            .iter()
            .filter(|parameter| parameter.scope != root.definition)
            .map(|parameter| format!("{} (definition {})", parameter.id, parameter.scope)),
    ));
    let timeline = written_timeline(target, root);
    losses.extend(named_loss(
        F3dLossCode::WriterDesignRecordOmitted,
        "design container",
        "the F3D design writes one timeline and no other container",
        target
            .design
            .containers
            .iter()
            .filter(|container| {
                !timeline.is_some_and(|timeline| std::ptr::eq(*container, timeline))
            })
            .map(|container| container.id().to_string()),
    ));
    if let Some(Container::Ordered {
        id,
        folders,
        rollback,
        placement,
        ..
    }) = timeline
    {
        let mut values = folders
            .iter()
            .map(|folder| match &folder.name {
                Some(name) => format!(
                    "{id} folder `{name}` (members {}..={})",
                    folder.first, folder.last
                ),
                None => format!("{id} folder (members {}..={})", folder.first, folder.last),
            })
            .collect::<Vec<_>>();
        if let Rollback::Before(member) = rollback {
            values.push(format!("{id} rollback (before {})", member_id(member)));
        }
        if placement.is_some() {
            values.push(format!("{id} placement"));
        }
        losses.extend(named_loss(
            F3dLossCode::WriterDesignRecordOmitted,
            "timeline value",
            "the F3D timeline holds its written items in one run, so no folder, rollback or placement of the source timeline has its members",
            values,
        ));
    }
    losses
}

/// The container the written Fusion timeline stands for: the root scope's
/// one root, where it is ordered.
fn written_timeline<'a>(target: &'a CadIr, root: &DesignScope) -> Option<&'a Container> {
    let [id] = root.roots.as_slice() else {
        return None;
    };
    target
        .design
        .containers
        .iter()
        .find(|container| container.id() == id)
        .filter(|container| matches!(container, Container::Ordered { .. }))
}

fn member_id(member: &Member) -> &str {
    match member {
        Member::Feature(feature) => feature.as_str(),
        Member::Container(container) => container.as_str(),
    }
}

/// Every feature of the scope's containers, in container order.
fn scope_features<'a>(target: &'a CadIr, scope: &'a DesignScope) -> Vec<&'a FeatureId> {
    let containers = target
        .design
        .containers
        .iter()
        .map(|container| (container.id(), container))
        .collect::<BTreeMap<_, _>>();
    let mut features = Vec::new();
    let mut visited = BTreeSet::new();
    for root in &scope.roots {
        walk(root, &containers, &mut visited, &mut features);
    }
    features
}

fn walk<'a>(
    id: &'a ContainerId,
    containers: &BTreeMap<&'a ContainerId, &'a Container>,
    visited: &mut BTreeSet<&'a ContainerId>,
    features: &mut Vec<&'a FeatureId>,
) {
    if !visited.insert(id) {
        return;
    }
    let Some(container) = containers.get(id) else {
        return;
    };
    let members: Vec<&Member> = match container {
        Container::Ordered { members, .. } => members.iter().collect(),
        Container::Unordered { members, .. } => members.iter().collect(),
    };
    for member in members {
        match member {
            Member::Feature(feature) => features.push(feature),
            Member::Container(nested) => walk(nested, containers, visited, features),
        }
    }
}
