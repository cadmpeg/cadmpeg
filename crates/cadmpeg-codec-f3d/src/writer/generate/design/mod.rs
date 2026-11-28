// SPDX-License-Identifier: Apache-2.0
//! The forms of a design scope (cadir-v11.md §3.4, Features without their
//! state). The geometry-true form writes the scope's parameters, and one
//! stored-geometry feature of the current bodies where the writer cannot
//! write every feature with its state. The history-true form writes the run
//! of features before the first feature it cannot write with its state,
//! then the timeline marker, then the later features rolled back without
//! their states; where the run holds no feature the writer writes with its
//! state, the marker stands before the first stateful feature and the
//! design holds no body.
//!
//! It runs where no native Design record is written and the document holds a
//! design scope for its root definition. It charges a loss for every design
//! scope, container, feature and parameter it does not write, for the root
//! scope's configuration, arrangements and occurrence states, for every
//! current state's `Stale` or `Failed` status and poses, and for every stored
//! evaluation. A failure here is a [`super::reduce::Stage::NeutralDesign`]
//! failure. A failure that names its records ([`Culprit`]) drops those
//! records and each record that needs them: a rolled-back item, a sketch, a
//! construction plane with the sketches on it, or a sketch dimension alone;
//! for the records or the history state of the item of a feature written
//! with its state, the history-true form ends the run at that feature,
//! where the items before it keep their states and result bodies
//! ([`held_state`]), and the geometry-true form writes the Base Feature; a
//! dropped sketch also ends the history-true run at the first item on it;
//! for the records of a [`Part`] that name no one record, that part. The
//! other fallbacks, in [`Part`] order, write the sketches without their
//! dimensions, the geometry item in the Base Feature, the design without its
//! construction planes and the sketches on them, the design without its
//! `Sketch` items, then no design ([`dropped_records`]).

mod base_feature;
pub(super) mod expression;
mod extrude;
pub(super) mod frames;
pub(super) mod history;
mod item;
pub(super) mod parameters;
mod revolve;
mod roster;
mod sketch;
#[cfg(test)]
mod tests;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_ir::codec::write::{
    evaluation_missing, evaluation_stale, ArenaDisposition, DesignArenaDispositions,
    MissingStateForm,
};
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::evaluation::{CurrentState, EvaluationStatus};
use cadmpeg_ir::design::ids::{ContainerId, EvaluationId};
use cadmpeg_ir::design::names::TopoName;
use cadmpeg_ir::design::parameter::ParameterOwner;
use cadmpeg_ir::design::refs::{Evaluated, Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::StepRef;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::SketchConstraintId;

use super::presentation::GeneratedDesignRegistry;
use super::records::ParameterRecords;
use super::reduce::named_loss;
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use base_feature::BaseFeaturePlan;
use frames::DesignRecords;
use item::{Decision, Item, Run};
use parameters::ParameterPlan;
use sketch::SketchPlan;

/// The smallest linear tolerance a dimension is checked with, in mm: the
/// decoder's linear resolution.
const MIN_LINEAR_TOLERANCE: f64 = 1e-5;
/// Millimetres per Fusion length unit (cm).
const MM_PER_CM: f64 = 10.0;

#[cfg(test)]
thread_local! {
    /// The number of next attempts that write the design scope on this
    /// thread and fail in the design module.
    static FAIL_NEXT_PLAN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Makes each plan on this thread fail where its fallback ends the
    /// history-true run at a feature.
    static FAIL_ENDED_PLANS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
    /// Makes the next attempt that writes a construction plane on this
    /// thread fail in the plane records.
    static FAIL_NEXT_PLANES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The failure [`FAIL_NEXT_PLANES`] requests, once.
#[cfg(test)]
fn requested_plane_failure() -> Option<cadmpeg_core::CodecError> {
    FAIL_NEXT_PLANES.with(|fail| fail.replace(false)).then(|| {
        cadmpeg_core::CodecError::NotImplemented("a test failure of the plane records".into())
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
    /// Makes the next attempt that writes the history of a geometry item on
    /// this thread find the last item's result body unwritten.
    static HIDE_NEXT_RESULT_BODY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
thread_local! {
    /// The identities whose records fail to write in every attempt on this
    /// thread: a feature, plane or constraint; `rolled {feature}` for the
    /// rolled-back item of a feature; `body of {feature}` and
    /// `rolled body of {feature}` for the bodies of its item in either form.
    static FAIL_RECORDS: std::cell::RefCell<BTreeSet<String>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

/// `written` without the result body of the last of `items` where
/// [`HIDE_NEXT_RESULT_BODY`] requests it, once.
#[cfg(test)]
fn hide_requested_body<'a>(written: Vec<&'a BodyId>, items: &[Item<'_>]) -> Vec<&'a BodyId> {
    let Some(hidden) = items.last().and_then(Item::body) else {
        return written;
    };
    if !HIDE_NEXT_RESULT_BODY.with(|hide| hide.replace(false)) {
        return written;
    }
    written.into_iter().filter(|body| *body != hidden).collect()
}

/// The failure [`FAIL_RECORDS`] requests for the records of `id`.
#[cfg(test)]
fn requested_record_failure(id: &impl std::fmt::Display) -> Option<cadmpeg_core::CodecError> {
    let id = id.to_string();
    FAIL_RECORDS
        .with(|fail| fail.borrow().contains(&id))
        .then(|| {
            cadmpeg_core::CodecError::NotImplemented(format!(
                "a test failure of the records of {id}"
            ))
        })
}

/// The failure [`FAIL_NEXT_PLAN`] requests, once per requested attempt.
#[cfg(test)]
pub(super) fn requested_test_failure() -> Option<cadmpeg_core::CodecError> {
    FAIL_NEXT_PLAN.with(|fail| {
        let left = fail.get().checked_sub(1)?;
        fail.set(left);
        Some(cadmpeg_core::CodecError::NotImplemented(
            "a test failure of the design module".into(),
        ))
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
    /// The geometry item: the Base Feature holds its feature.
    Item,
    /// The `WorkPlane` items and the `Sketch` items on them.
    Planes,
    /// The `Sketch` items.
    Sketches,
}

impl Part {
    const ORDER: [Self; 4] = [Self::Dimensions, Self::Item, Self::Planes, Self::Sketches];
}

/// The records a failure to write the design scope names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Culprit {
    /// The records of the geometry item of `feature`, rolled back after the
    /// timeline marker or written with its state.
    Item { feature: FeatureId, rolled: bool },
    /// The records of the `Sketch` item of the feature.
    Sketch(FeatureId),
    /// The records of a `WorkPlane` item, with the features of the sketches
    /// on it.
    Plane(Vec<FeatureId>),
    /// The records of the sketch dimension of the constraint.
    Dimension(SketchConstraintId),
    /// The records of a part, where the failure names no one record of it.
    Part(Part),
    /// The ASM history state of the item of the feature, or of every item
    /// where it names none.
    History(Option<FeatureId>),
}

/// A failure to write the records of the design scope, and the records it
/// names where the writer can drop them alone.
#[derive(Debug)]
pub(super) struct RecordFailure {
    pub(super) error: CodecError,
    pub(super) culprit: Option<Culprit>,
}

impl RecordFailure {
    /// The failure, naming `culprit` where it names no records yet.
    pub(super) fn or_named(self, culprit: impl FnOnce() -> Culprit) -> Self {
        Self {
            culprit: self.culprit.or_else(|| Some(culprit())),
            ..self
        }
    }
}

impl From<CodecError> for RecordFailure {
    fn from(error: CodecError) -> Self {
        Self {
            error,
            culprit: None,
        }
    }
}

/// The parts and records the writer has dropped, each with the failure that
/// dropped it.
#[derive(Clone, Debug, Default)]
pub(super) struct Fallback {
    dropped: Vec<(Part, String)>,
    /// The failure that dropped the geometry items written with their
    /// states, where it names what failed.
    fault: Option<ItemFault>,
    /// The features that end the run of the history-true form, each with
    /// why the writer cannot write it with its state.
    ends: BTreeMap<FeatureId, String>,
    /// The rolled-back items whose records the writer cannot write, each
    /// with the failure.
    rolled: BTreeMap<FeatureId, String>,
    /// The sketches the writer drops alone, each with why.
    sketches: BTreeMap<FeatureId, String>,
    /// The sketch dimensions whose records the writer cannot write, each
    /// with the failure.
    dimensions: BTreeMap<SketchConstraintId, String>,
}

/// What failed where the geometry items written with their states are
/// dropped. The rolled-back items commit no state and do not need them, so
/// the writer still writes those.
#[derive(Clone, Debug)]
enum ItemFault {
    /// The ASM history, with the feature whose state the failure concerns
    /// where it names one.
    History(Option<FeatureId>),
    /// The records of the item of the feature.
    Records(FeatureId),
}

impl Fallback {
    /// Why `part` is dropped.
    fn reason(&self, part: Part) -> Option<&str> {
        self.dropped
            .iter()
            .find(|(dropped, _)| *dropped == part)
            .map(|(_, reason)| reason.as_str())
    }

    /// Whether the geometry items written with their states are dropped.
    pub(super) fn drops_items(&self) -> bool {
        self.reason(Part::Item).is_some()
    }

    /// Why the geometry items are dropped, and the feature the failure
    /// concerns where it names one.
    fn item_failure(&self) -> Option<(String, Option<&FeatureId>)> {
        let reason = self.reason(Part::Item)?;
        Some(match &self.fault {
            Some(ItemFault::History(cause)) => (history_reason(reason), cause.as_ref()),
            Some(ItemFault::Records(feature)) => (records_reason(reason), Some(feature)),
            None => (records_reason(reason), None),
        })
    }

    /// Whether the writer writes no rolled-back item: the geometry items
    /// are dropped for a failure that names no records.
    fn drops_rolled_items(&self) -> bool {
        self.reason(Part::Item).is_some() && self.fault.is_none()
    }

    /// Why the writer writes no rolled-back item for `feature`, where its
    /// records failed.
    fn rolled_failure(&self, feature: &FeatureId) -> Option<String> {
        self.rolled
            .get(feature)
            .map(|reason| records_reason(reason))
    }

    /// Drop the records `culprit` names for `reason`. Returns whether the
    /// writer had not dropped them yet.
    pub(super) fn drop_records(
        &mut self,
        target: &CadIr,
        missing_states: MissingStateForm,
        culprit: &Culprit,
        reason: &str,
    ) -> bool {
        match culprit {
            Culprit::Item {
                feature,
                rolled: true,
            } => newly(&mut self.rolled, feature, reason.to_owned()),
            Culprit::Sketch(feature) => {
                let dropped = newly(
                    &mut self.sketches,
                    feature,
                    format!("is dropped because the writer cannot write its records ({reason})"),
                );
                if let Some(item) = dropped.then(|| first_item_on(target, feature)).flatten() {
                    self.end_run(
                        target,
                        missing_states,
                        item,
                        item::SKETCH_NOT_WRITTEN.to_owned(),
                    );
                }
                dropped
            }
            Culprit::Plane(features) => {
                let mut dropped = false;
                for feature in features {
                    dropped |= newly(
                        &mut self.sketches,
                        feature,
                        format!("is dropped because the writer cannot write the records of its construction plane ({reason})"),
                    );
                }
                dropped
            }
            Culprit::Dimension(constraint) => {
                newly(&mut self.dimensions, constraint, reason.to_owned())
            }
            Culprit::Part(part) => self.drop_part(target, missing_states, *part, reason),
            Culprit::Item {
                feature,
                rolled: false,
            } => {
                self.end_run(target, missing_states, feature, records_reason(reason))
                    || self.drop_items(
                        target,
                        missing_states,
                        ItemFault::Records(feature.clone()),
                        reason,
                    )
            }
            Culprit::History(Some(feature)) => {
                self.end_run(target, missing_states, feature, history_reason(reason))
                    || self.drop_items(
                        target,
                        missing_states,
                        ItemFault::History(Some(feature.clone())),
                        reason,
                    )
            }
            Culprit::History(None) => {
                self.drop_items(target, missing_states, ItemFault::History(None), reason)
            }
        }
    }

    /// End the run of the history-true form at `feature` for `reason`, where
    /// the feature stands before the run's end: the features before it keep
    /// their states, and the features from it on follow the timeline marker
    /// (cadir-v11.md §3.4, Features without their state). Returns whether
    /// the run's end moved.
    fn end_run(
        &mut self,
        target: &CadIr,
        missing_states: MissingStateForm,
        feature: &FeatureId,
        reason: String,
    ) -> bool {
        if missing_states != MissingStateForm::HistoryTrue {
            return false;
        }
        let Some(scope) = root_scope(target) else {
            return false;
        };
        let features = scope_features(target, scope);
        let Some(at) = features.iter().position(|candidate| *candidate == feature) else {
            return false;
        };
        let end =
            run_end(target, scope, &features, &self.ends).map_or(features.len(), |(end, _)| end);
        at < end && newly(&mut self.ends, feature, reason)
    }

    /// Drop the geometry items written with their states for `reason`, a
    /// failure of `fault`. Returns whether they were dropped.
    fn drop_items(
        &mut self,
        target: &CadIr,
        missing_states: MissingStateForm,
        fault: ItemFault,
        reason: &str,
    ) -> bool {
        let dropped = self.drop_part(target, missing_states, Part::Item, reason);
        if dropped {
            self.fault = Some(fault);
        }
        dropped
    }

    /// Drop the first part in [`Part::ORDER`] not yet dropped that the
    /// design of `target` in the form `missing_states` writes, for `reason`.
    /// Returns whether a part was dropped.
    pub(super) fn drop_next(
        &mut self,
        target: &CadIr,
        missing_states: MissingStateForm,
        reason: &str,
    ) -> bool {
        Part::ORDER
            .into_iter()
            .any(|part| self.drop_part(target, missing_states, part, reason))
    }

    /// Drop `part` for `reason` where it is not yet dropped and the design
    /// of `target` in the form `missing_states` writes it. Returns whether
    /// it was dropped.
    fn drop_part(
        &mut self,
        target: &CadIr,
        missing_states: MissingStateForm,
        part: Part,
        reason: &str,
    ) -> bool {
        if self.reason(part).is_some() {
            return false;
        }
        let writes = plan(target, missing_states, self)
            .ok()
            .flatten()
            .is_some_and(|design| match part {
                Part::Dimensions => design.writes_dimensions(),
                Part::Item => !design.item.items().is_empty() || !design.rolled.is_empty(),
                Part::Planes => !design.sketches.planes.is_empty(),
                Part::Sketches => design.writes_sketches(),
            });
        if writes {
            self.dropped.push((part, reason.to_owned()));
        }
        writes
    }
}

/// What the writer writes of the root definition's design scope.
pub(super) struct NeutralDesign<'a> {
    parameters: ParameterPlan,
    sketches: SketchPlan<'a>,
    /// The scope's geometry features, written as timeline items with their
    /// states in place of the Base Feature where the rows of [`item::plan`]
    /// allow it.
    item: Decision<'a>,
    /// The stateful features after the timeline marker, rolled back with
    /// their definitions, in timeline order.
    rolled: Vec<Item<'a>>,
    /// The position in `sketches.items` of the first `Sketch` item after the
    /// timeline marker; `sketches.items.len()` where the marker stands after
    /// the last item.
    marker: usize,
    base_feature: BaseFeaturePlan<'a>,
    /// Whether the design is the history-true form whose state at the
    /// timeline marker holds no body: it writes no `Base Feature` and no
    /// B-rep stream (f3d.md §3.1, "Records of a design whose timeline holds
    /// no body").
    body_less: bool,
    /// Whether the timeline marker stands before a feature of the scope:
    /// the history-true form then differs from the geometry-true form.
    rolls_back: bool,
    /// Whether the insertion history restates an evaluation of the scope.
    restates: bool,
    losses: Vec<LossNote>,
}

/// The plan for the root definition's design scope in the form
/// `missing_states`; `None` when the document holds none. Each part
/// `fallback` drops is written in its fallback form and charged with the
/// failure that dropped it.
pub(super) fn plan<'a>(
    target: &'a CadIr,
    missing_states: MissingStateForm,
    fallback: &Fallback,
) -> Result<Option<NeutralDesign<'a>>, CodecError> {
    #[cfg(test)]
    if FAIL_ENDED_PLANS.with(std::cell::Cell::get) && !fallback.ends.is_empty() {
        return Err(CodecError::NotImplemented(
            "a test failure of the design plan".into(),
        ));
    }
    let Some(scope) = root_scope(target) else {
        return Ok(None);
    };
    let mut parameters = parameters::plan(target, &scope.definition);
    let features = scope_features(target, scope);
    let mut sketches = sketch_plan(
        target,
        &features,
        fallback.reason(Part::Planes),
        &fallback.sketches,
    );
    let mut ordinal = 1;
    if let Some(reason) = fallback.reason(Part::Sketches) {
        sketches.drop_items(reason);
    } else {
        let every = fallback
            .reason(Part::Dimensions)
            .map(|reason| format!("the writer failed with the sketch's dimensions ({reason})"));
        sketches.drop_dimensions(|constraint| {
            every.clone().or_else(|| {
                fallback
                    .dimensions
                    .get(constraint)
                    .map(|reason| records_reason(reason))
            })
        });
        ordinal = sketches.claim_parameters(
            &mut parameters,
            sketch::dimensions::Tolerances {
                linear: target.tolerances.linear.get().max(MIN_LINEAR_TOLERANCE) / MM_PER_CM,
                angular: target.tolerances.angular.get(),
            },
        );
    }
    let mut rolled = features.len();
    let mut body_less = None;
    // The feature the timeline marker stands before where the writer cannot
    // write it with its state, and why.
    let mut cause = None;
    let item = if target.geometry.bodies.is_empty() {
        Decision::None
    } else {
        let marker = match missing_states {
            MissingStateForm::GeometryTrue => None,
            MissingStateForm::HistoryTrue => run_end(target, scope, &features, &fallback.ends),
        };
        let marker = marker.map(|(end, reason)| {
            cause = reason.map(|reason| (features[end], reason));
            end
        });
        let first_stateful = match missing_states {
            MissingStateForm::GeometryTrue => None,
            MissingStateForm::HistoryTrue => features
                .iter()
                .position(|feature| !target.is_stateless(feature, None).unwrap_or(false)),
        };
        let mut decide = |run: Run<'a, '_>| {
            item::plan(
                target,
                scope,
                run,
                &sketches,
                &mut parameters,
                fallback.item_failure(),
                ordinal,
            )
        };
        let whole = Run {
            features: &features,
            ends: false,
        };
        let run = match marker {
            Some(end) => decide(Run {
                features: &features[..end],
                ends: true,
            }),
            None => decide(whole),
        };
        match (run, first_stateful) {
            (run @ Decision::Items(_), _) => {
                rolled = marker.unwrap_or(features.len());
                run
            }
            (run, Some(first)) => {
                rolled = marker.map_or(first, |end| end.min(first));
                body_less = Some(run);
                Decision::None
            }
            (run, None) => run,
        }
    };
    let (rolled_items, mut rolled_reasons) = rolled_back_items(
        target,
        scope,
        &features,
        (rolled..features.len()).filter(|&at| fallback.rolled_failure(features[at]).is_none()),
        &sketches,
        &mut parameters,
        if fallback.drops_rolled_items() {
            Err(ITEM_DROPPED)
        } else {
            item.items()
                .iter()
                .try_fold(ordinal, |next, item| item.next_ordinal(next))
                .ok_or(ORDINALS_EXCEEDED)
        },
    );
    rolled_reasons.extend(
        features[rolled..]
            .iter()
            .filter_map(|feature| Some((*feature, fallback.rolled_failure(feature)?))),
    );
    let rolled_back = features[rolled..].iter().copied().collect::<BTreeSet<_>>();
    let marker = sketches
        .items
        .iter()
        .position(|planned| rolled_back.contains(planned.feature))
        .unwrap_or(sketches.items.len());
    let writes_rolled =
        |feature: &FeatureId| rolled_items.iter().any(|item| item.feature() == feature);
    let written_marker = features.get(rolled).copied().map(|before| Marker {
        before,
        first_item: writes_rolled(before)
            || sketches.items.get(marker).is_some_and(|planned| {
                planned.feature == before
                    && !sketches.planes.iter().any(|plane| plane.first == marker)
            }),
    });
    // Why the writer writes `feature` without its state: the reason the run
    // holds no body, or the reason the marker stands before it.
    let unstated = |feature: &FeatureId| {
        body_less
            .as_ref()
            .and_then(|run| run.reason(feature))
            .or_else(|| match &cause {
                Some((before, reason)) if *before == feature => Some(reason.clone()),
                _ => None,
            })
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
        .filter(|feature| {
            !sketches.writes(feature) && !item.writes(feature) && !writes_rolled(feature)
        })
        .map(|feature| {
            let mut notes = Vec::new();
            let rolled_reason = rolled_reasons.get(*feature);
            if rolled_back.contains(*feature) {
                notes.push(match rolled_reason {
                    Some(reason) => format!(
                        "it follows the timeline marker, and the writer writes no rolled-back item for it: {reason}"
                    ),
                    None => "it follows the timeline marker".to_owned(),
                });
            }
            if let Some(reason) = unstated(feature).filter(|reason| rolled_reason != Some(reason)) {
                notes.push(format!("the writer writes it without its state: {reason}"));
            }
            if let Some(reason) = sketches.unwritten.get(*feature) {
                notes.push(format!("its sketch {reason}"));
            }
            if let Some(reason) = item.reason(feature) {
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
    let reason = if !item.items().is_empty() || body_less.is_some() {
        "stores no timeline item for these features of the design"
    } else {
        "stores the current bodies as one Base Feature and no timeline item for these features of the design"
    };
    let mut losses = evaluation_missing("F3D", reason, unwritten)
        .into_iter()
        .collect::<Vec<_>>();
    losses.extend(evaluation_missing(
        "F3D",
        "writes these features after the timeline marker, rolled back with their definitions and without their states",
        features[rolled..]
            .iter()
            .filter(|feature| sketches.writes(feature) || writes_rolled(feature))
            .map(|feature| match unstated(feature) {
                Some(reason) => {
                    format!("{feature} (the writer writes it without its state: {reason})")
                }
                None => feature.to_string(),
            }),
    ));
    losses.extend(unwritten_records(target, scope, written_marker));
    let mut base_feature =
        base_feature::plan(target, sketches.items.len() + sketches.planes.len())?;
    if body_less.is_some() {
        base_feature = base_feature.without_bodies();
    }
    let restated = if base_feature.has_item() {
        restated(target, scope, item.items(), &sketches, &mut losses)
    } else {
        BTreeSet::new()
    };
    losses.extend(states(target, &restated));
    Ok(Some(NeutralDesign {
        parameters,
        sketches,
        item,
        rolled: rolled_items,
        marker,
        base_feature,
        body_less: body_less.is_some(),
        rolls_back: written_marker.is_some(),
        restates: !restated.is_empty(),
        losses,
    }))
}

/// The stateful features at `positions` of `features`, the scope's features
/// in timeline order, that the writer writes after the timeline marker with
/// their definitions, with the reason for each it does not write. Their
/// parameters take the source ordinals from `ordinal`, or `ordinal` names
/// why no item is written.
fn rolled_back_items<'a>(
    target: &'a CadIr,
    scope: &'a DesignScope,
    features: &[&'a FeatureId],
    positions: impl Iterator<Item = usize>,
    sketches: &SketchPlan<'a>,
    parameters: &mut ParameterPlan,
    ordinal: Result<u32, &'static str>,
) -> (Vec<Item<'a>>, BTreeMap<&'a FeatureId, String>) {
    let mut items = Vec::new();
    let mut reasons = BTreeMap::new();
    let mut ordinal = ordinal;
    for position in positions {
        let feature = &features[position];
        if target.is_stateless(feature, None).unwrap_or(false) {
            continue;
        }
        let planned = ordinal.and_then(|next| {
            item::rolled_back(
                target, scope, features, position, sketches, parameters, next,
            )
        });
        match planned {
            Ok(planned) => {
                ordinal =
                    ordinal.and_then(|next| planned.next_ordinal(next).ok_or(ORDINALS_EXCEEDED));
                items.push(planned);
            }
            Err(reason) => {
                reasons.insert(*feature, reason.to_owned());
            }
        }
    }
    (items, reasons)
}

/// Insert `value` at `key` where `map` holds no value there. Returns
/// whether it did.
fn newly<K: Ord + Clone>(map: &mut BTreeMap<K, String>, key: &K, value: String) -> bool {
    match map.entry(key.clone()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
            true
        }
        std::collections::btree_map::Entry::Occupied(_) => false,
    }
}

/// Why the writer writes no item whose records failed with `reason`.
fn records_reason(reason: &str) -> String {
    format!("the writer cannot write its records ({reason})")
}

/// Why the writer writes no state of an item whose ASM history failed with
/// `reason`.
fn history_reason(reason: &str) -> String {
    format!("the writer cannot write its history state ({reason})")
}

/// The first stateful feature of the root scope of `target` whose profile
/// lies on the sketch of `sketch`.
fn first_item_on<'a>(target: &'a CadIr, sketch: &FeatureId) -> Option<&'a FeatureId> {
    let scope = root_scope(target)?;
    scope_features(target, scope).into_iter().find(|feature| {
        !target.is_stateless(feature, None).unwrap_or(false)
            && item::profile_sketch(target, feature) == Some(sketch)
    })
}

/// Why a rolled-back item is not written where its parameters exceed the
/// source ordinals.
const ORDINALS_EXCEEDED: &str = "its parameters exceed the source ordinals";
/// Why a rolled-back item is not written after an attempt failed with the
/// records of a geometry item.
const ITEM_DROPPED: &str = "the writer cannot write the records of its item";

/// The timeline marker the history-true form writes.
#[derive(Clone, Copy)]
struct Marker<'a> {
    /// The first feature it rolls back.
    before: &'a FeatureId,
    /// Whether the item of `before` is the first item after the marker.
    first_item: bool,
}

/// The position in `features` of the first feature the history-true form
/// rolls back: the first feature that can change a state and that the
/// writer cannot write with its state as a geometry item of the run
/// ([`item::carries`]), or that an earlier attempt `ends` the run at, which
/// the form leaves without its state (cadir-v11.md §3.4, Features without
/// their state), with the reason the writer cannot, or the feature the
/// timeline's stated rollback stands before (§5.2) with no reason,
/// whichever comes first. `None` where the run is the whole timeline.
fn run_end(
    target: &CadIr,
    root: &DesignScope,
    features: &[&FeatureId],
    ends: &BTreeMap<FeatureId, String>,
) -> Option<(usize, Option<String>)> {
    let second = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| !target.is_stateless(feature, None).unwrap_or(false))
        .enumerate()
        .find_map(|(earlier, (at, feature))| {
            let reason = match ends.get(*feature) {
                Some(reason) => reason.clone(),
                None => item::carries(target, feature, earlier == 0)
                    .err()?
                    .to_owned(),
            };
            Some((at, Some(reason)))
        });
    let rollback = match written_timeline(target, root) {
        Some(Container::Ordered {
            rollback: Some(Rollback::Before(Member::Feature(before))),
            ..
        }) => features
            .iter()
            .position(|feature| *feature == before)
            .map(|at| (at, None)),
        _ => None,
    };
    match (second, rollback) {
        (Some(second), Some(rollback)) if rollback.0 <= second.0 => Some(rollback),
        (Some(end), _) | (None, Some(end)) => Some(end),
        (None, None) => None,
    }
}

/// The document the history-true form writes in place of `target`, where
/// the root scope's one current state stands after the timeline marker and
/// an evaluation holds the state at the marker (cadir-v11.md §3.4, Features
/// without their state; §5.4): `target` with that state as its current
/// state, and the loss of the records of the current state the document then
/// does not hold. `None` where the form writes `target` itself. The state
/// resolves under the caller's `limits`.
///
/// # Errors
///
/// Returns [`CodecError::ResourceLimit`], scoped to the session, where the
/// evaluation's base chain exceeds `limits` (§2.5).
pub(in crate::writer::generate) fn state_at_marker(
    target: &CadIr,
    limits: &ResourceLimits,
) -> Result<Option<(CadIr, Option<LossNote>)>, CodecError> {
    // A native Design names the records of the current state.
    if target.native.namespace("f3d").is_some() {
        return Ok(None);
    }
    let Some(scope) = root_scope(target) else {
        return Ok(None);
    };
    let features = scope_features(target, scope);
    let Some((end, _)) = run_end(target, scope, &features, &BTreeMap::new()) else {
        return Ok(None);
    };
    let run = &features[..end];
    let [current] = scope.current.as_slice() else {
        return Ok(None);
    };
    if !current.poses.is_empty() || !item::after_the_run(&current.at, run) {
        return Ok(None);
    }
    let Some(last) = run
        .iter()
        .rposition(|feature| !target.is_stateless(feature, None).unwrap_or(false))
    else {
        return Ok(None);
    };
    let Some(evaluation) = target.design.evaluations.iter().find(|evaluation| {
        evaluation.scope == scope.definition
            && evaluation.configuration == scope.active_configuration
            && evaluation.status == EvaluationStatus::Current
            && evaluation.content.is_some()
            && matches!(
                &evaluation.of,
                Evaluated::State(Position::After(Member::Feature(at))) if run[last..].contains(&at)
            )
    }) else {
        return Ok(None);
    };
    let Evaluated::State(at) = &evaluation.of else {
        return Ok(None);
    };
    // A chain past the caller's ceiling is the §2.5 refusal; another
    // resolution failure leaves the form writing `target` itself.
    let state = match target.resolved_evaluation_state(&evaluation.id, limits) {
        Ok(state) => state,
        Err(error) => {
            return error
                .resource_limit()
                .map_or(Ok(None), |limit| Err(CodecError::ResourceLimit(limit)));
        }
    };
    if state.records().bodies.is_empty() {
        return Ok(None);
    }
    let mut staged = target.clone();
    staged.geometry = state.records().clone();
    let Some(root) = staged
        .design
        .scopes
        .iter_mut()
        .find(|candidate| candidate.definition == scope.definition)
    else {
        return Ok(None);
    };
    root.current = vec![CurrentState {
        at: at.clone(),
        bodies: state
            .records()
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect(),
        poses: state.poses().clone(),
        status: EvaluationStatus::Current,
        native_ref: None,
    }];
    staged
        .design
        .evaluations
        .retain(|other| other.id != evaluation.id);
    staged.finalize();
    let unwritten = super::reduce::geometry_records_outside(&target.geometry, &staged.geometry);
    let loss = (!unwritten.is_empty()).then(|| {
        F3dLossCode::WriterGeometryOmitted.note(format!(
            "the history-true design holds the state at its timeline marker, which evaluation {} holds, in place of the current state {}, so these records of the current state were not written: {}",
            evaluation.id,
            position(&current.at),
            unwritten.join("; ")
        ))
    });
    Ok(Some((staged, loss)))
}

/// The state the history-true form writes at its timeline marker where a
/// failure ends the run at an item: the state after the items before it.
pub(super) struct HeldState {
    /// The state, as the scope's current state.
    pub(super) state: CurrentState,
    /// The feature the timeline marker stands before.
    pub(super) before: FeatureId,
}

/// The state the history-true form holds at its timeline marker where
/// `fallback` ends the run at a feature that `earlier` does not: the state
/// after the geometry items before that feature in the attempt `earlier`
/// plans, with the result bodies of those items, as
/// [`NeutralDesign::history_states`] commits it (f3d.md §3.1, "History
/// states"; cadir-v11.md §3.4, Features without their state). `None` where
/// that attempt writes no item before the feature. `scope` is the position
/// of the root scope ([`root_scope_at`]).
pub(super) fn held_state(
    target: &CadIr,
    scope: usize,
    missing_states: MissingStateForm,
    earlier: &Fallback,
    fallback: &Fallback,
) -> Option<HeldState> {
    let before = fallback
        .ends
        .keys()
        .find(|feature| !earlier.ends.contains_key(*feature))?;
    let scope = &target.design.scopes[scope];
    let [current] = scope.current.as_slice() else {
        return None;
    };
    let features = scope_features(target, scope);
    let run = &features[..features.iter().position(|feature| *feature == before)?];
    let design = plan(target, missing_states, earlier).ok().flatten()?;
    let kept = design
        .item
        .items()
        .iter()
        .filter(|item| run.contains(&item.feature()))
        .collect::<Vec<_>>();
    let after = kept.last()?.feature();
    Some(HeldState {
        state: CurrentState {
            at: Position::After(Member::Feature(after.clone())),
            bodies: kept
                .iter()
                .filter_map(|item| item.body().cloned())
                .collect(),
            poses: current.poses.clone(),
            status: current.status,
            native_ref: None,
        },
        before: before.clone(),
    })
}

impl NeutralDesign<'_> {
    /// Whether the design stores a body: its B-rep stream then carries the
    /// Base Feature's names and history.
    pub(super) fn has_bodies(&self) -> bool {
        self.base_feature.has_item()
    }

    /// Whether the timeline marker stands before a feature of the scope.
    pub(super) fn rolls_back(&self) -> bool {
        self.rolls_back
    }

    /// Whether the design writes the roster of a timeline: a design with a
    /// result body, or the history-true form whose state at the timeline
    /// marker holds no body.
    pub(super) fn writes_timeline(&self) -> bool {
        self.has_bodies() || self.body_less
    }

    /// Name the result bodies and their faces in `native`, whose records the
    /// B-rep stream is written from.
    pub(super) fn stamp(&mut self, native: &mut F3dNative) -> Result<(), CodecError> {
        let items = self.item.items();
        if items.is_empty() {
            let losses = self.base_feature.stamp(native)?;
            self.losses.extend(losses);
        }
        for item in items {
            match item {
                Item::Extrude(plan) => extrude::stamp(plan, &self.sketches.items, native)?,
                Item::Revolve(plan) => revolve::stamp(plan, native)?,
            }
        }
        Ok(())
    }

    /// The position in the design's body order of the result body of each
    /// state after the root state, in state order: one state that holds
    /// every body for a `Base Feature`, and the body of each geometry item
    /// (f3d.md §3.1, "History states").
    pub(super) fn history_states(&self, target: &CadIr) -> Result<Vec<usize>, MissingResult> {
        let written = self
            .base_feature
            .roster_bodies(target)
            .into_iter()
            .map(|body| body.id)
            .collect::<Vec<_>>();
        #[cfg(test)]
        let written = hide_requested_body(written, self.item.items());
        state_bodies(
            self.item
                .items()
                .iter()
                .filter_map(|item| Some((item.feature(), item.body()?))),
            &written,
        )
    }

    /// The parameter records and the design's records, with their types
    /// registered in `registry`.
    pub(super) fn records(
        &mut self,
        registry: &mut GeneratedDesignRegistry,
    ) -> Result<(ParameterRecords, DesignRecords), CodecError> {
        let (parameters, records, losses) = self.base_feature.encode(&self.parameters, registry)?;
        self.losses.extend(losses);
        Ok((parameters, records))
    }

    /// The members of the design folder of a design that writes a timeline:
    /// the Design, ACT and Browser segments, the history stream `smbh` and the
    /// snapshot stream `smb` of `streams`, and the members their records name
    /// (f3d.md §3.1, "Records of a Base Feature design"). A design with no
    /// body has no `streams` (f3d.md §3.1, "Records of a design whose
    /// timeline holds no body").
    pub(super) fn archive(
        &mut self,
        target: &CadIr,
        streams: Option<(&[u8], &[u8])>,
    ) -> Result<Vec<(String, Vec<u8>)>, RecordFailure> {
        let scope = root_scope(target)
            .ok_or_else(|| CodecError::Malformed("the design has no root scope".into()))?;
        let seed = match streams {
            Some((smbh, _)) => smbh.to_vec(),
            None => scope_features(target, scope)
                .into_iter()
                .flat_map(|feature| feature.as_str().bytes())
                .collect(),
        };
        let bodies = self.base_feature.roster_bodies(target);
        let items = self.item.items();
        let roster = roster::Roster::new(
            &bodies,
            self.parameters.written_count(),
            &self.sketches,
            items,
            &self.rolled,
        )?;
        let timeline_value = roster::ContextValue::of(
            self.parameters.names(),
            &self.sketches.items,
            &self.sketches.planes,
            items,
            &self.rolled,
        );
        let ctx = roster::Context {
            marker: self.marker,
            table: roster::type_table(&roster, bodies.iter().any(|body| body.sheet))?,
            roster,
            values: roster::values::Values::new(target, &scope.definition, &seed),
            bodies,
            sketches: &self.sketches.items,
            planes: &self.sketches.planes,
            timeline_value,
        };
        let mut records = DesignRecords::default();
        BaseFeaturePlan::roster_records(&ctx, &self.parameters, &mut records)?;
        let (members, losses) = roster::archive(&ctx, records, streams)?;
        self.losses.extend(losses);
        Ok(members)
    }

    /// Whether the insertion history writes evaluation geometry and reports its omitted values.
    pub(super) fn restates_evaluations(&self) -> bool {
        self.restates
    }

    /// Whether the design writes a feature of the scope as a timeline item:
    /// a `Sketch` item, a geometry item, or a rolled-back item.
    pub(super) fn writes_features(&self) -> bool {
        self.writes_sketches() || !self.item.items().is_empty() || !self.rolled.is_empty()
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

    /// The ids of the parameters a written record restates.
    pub(super) fn restated_parameters(&self) -> BTreeSet<ParameterId> {
        self.parameters.restated()
    }

    pub(super) fn into_losses(self) -> Vec<LossNote> {
        let mut losses = self.losses;
        losses.extend(self.parameters.into_losses());
        losses.extend(self.sketches.into_losses());
        losses
    }
}

/// The plan of the sketches among `features`, without the sketches off the
/// origin planes where `planes` names the failure that dropped them and
/// without the sketches `dropped` names. The
/// `Sketch` items precede the `Base Feature`, which a design with no body
/// does not write.
fn sketch_plan<'a>(
    target: &'a CadIr,
    features: &[&'a FeatureId],
    planes: Option<&str>,
    dropped: &BTreeMap<FeatureId, String>,
) -> SketchPlan<'a> {
    if target.geometry.bodies.is_empty() {
        SketchPlan::default()
    } else {
        sketch::plan(target, features, planes, dropped)
    }
}

/// The design scope of the root definition: the document's one scope, or
/// the scope whose definition no occurrence places.
pub(super) fn root_scope(target: &CadIr) -> Option<&DesignScope> {
    root_scope_at(target).map(|at| &target.design.scopes[at])
}

/// Whether the root scope states a timeline: an ordered container among its
/// roots.
pub(super) fn states_timeline(target: &CadIr) -> bool {
    root_scope(target).is_some_and(|scope| {
        target.design.containers.iter().any(|container| {
            matches!(container, Container::Ordered { .. }) && scope.roots.contains(container.id())
        })
    })
}

/// The position of [`root_scope`] among the document's scopes.
pub(super) fn root_scope_at(target: &CadIr) -> Option<usize> {
    match target.design.scopes.as_slice() {
        [] => None,
        [_] => Some(0),
        scopes => {
            let placed = target
                .model
                .occurrences
                .iter()
                .map(|occurrence| &occurrence.prototype)
                .collect::<BTreeSet<_>>();
            scopes
                .iter()
                .position(|scope| !placed.contains(&scope.definition))
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
    let mut losses = states(target, &BTreeSet::new());
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
/// status and each pose, which the written bodies do not carry, and each
/// evaluation outside `restated`, which no written history state holds.
fn states(target: &CadIr, restated: &BTreeSet<&EvaluationId>) -> Vec<LossNote> {
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
            EvaluationStatus::Current if restated.contains(&evaluation.id) => {}
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
        "the F3D writer writes no history state with the records of these states",
        evaluations,
    ));
    losses
}

/// The states whose geometry the insertion history writes. Report the
/// other values it cannot write before marking their evaluations covered.
fn restated<'a>(
    target: &'a CadIr,
    scope: &DesignScope,
    items: &[Item<'_>],
    sketches: &SketchPlan<'_>,
    losses: &mut Vec<LossNote>,
) -> BTreeSet<&'a EvaluationId> {
    let mut bodies = BTreeSet::new();
    let mut written = BTreeMap::new();
    for item in items {
        bodies.extend(item.body());
        written.insert(item.feature(), (item, bodies.clone()));
    }
    let mut restated = BTreeSet::new();
    for evaluation in &target.design.evaluations {
        if evaluation.scope != scope.definition
            || evaluation.configuration.is_some()
            || evaluation.status != EvaluationStatus::Current
        {
            continue;
        }
        let Evaluated::State(Position::After(Member::Feature(feature))) = &evaluation.of else {
            continue;
        };
        let Some((_, bodies)) = written.get(feature) else {
            continue;
        };
        let Ok(state) =
            target.resolved_evaluation_state(&evaluation.id, &ResourceLimits::default())
        else {
            continue;
        };
        if !state.poses().is_empty()
            || !state
                .records()
                .bodies
                .iter()
                .map(|body| &body.id)
                .eq(bodies.iter().copied())
            || !super::reduce::geometry_records_outside(state.records(), &target.geometry)
                .is_empty()
        {
            continue;
        }

        let id = &evaluation.id;
        let unwritten_names = state.names().iter().filter(|(entity, entry)| {
            if entry
                .native
                .as_ref()
                .is_some_and(|name| name.codec.as_str() == "f3d")
            {
                return true;
            }
            let Some(name @ TopoName::Generated { feature, .. }) = &entry.name else {
                return true;
            };
            !written
                .get(feature)
                .is_some_and(|(item, _)| item.writes_name(entity, name, sketches))
        });
        losses.extend(named_loss(
            F3dLossCode::WriterDesignRecordOmitted,
            "evaluation name",
            "the insertion history cannot stamp their names",
            unwritten_names.map(|(entity, _)| format!("{} in evaluation {id}", entity.id())),
        ));
        let definitions = &target.model.product_definitions;
        let different_owners = state.owners().iter().filter(|(body, owner)| {
            let current = definitions
                .iter()
                .find(|definition| definition.bodies.contains(body))
                .map_or(&scope.definition, |definition| &definition.id);
            *owner != current
        });
        losses.extend(named_loss(
            F3dLossCode::WriterDesignRecordOmitted,
            "evaluation body owner",
            "the insertion history uses the current body owners",
            different_owners
                .map(|(body, owner)| format!("{body} owned by {owner} in evaluation {id}")),
        ));
        let different_members = state.members().iter().filter(|(occurrence, owner)| {
            let current = cadmpeg_ir::products::owner(definitions, occurrence)
                .map_or(&scope.definition, |definition| &definition.id);
            *owner != current
        });
        losses.extend(named_loss(
            F3dLossCode::WriterDesignRecordOmitted,
            "evaluation occurrence membership",
            "the insertion history has no occurrence membership changes",
            different_members.map(|(occurrence, owner)| {
                format!("{occurrence} owned by {owner} in evaluation {id}")
            }),
        ));
        losses.extend(named_loss(
            F3dLossCode::WriterDesignRecordOmitted,
            "evaluation instance-body assignment",
            "the generated Design holds no occurrence state",
            state
                .instance_bodies()
                .keys()
                .map(|path| format!("{path:?} in evaluation {id}")),
        ));
        restated.insert(id);
    }
    restated
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
/// placement and rollback, unless the written timeline `marker` stands
/// where the rollback does.
fn unwritten_records(
    target: &CadIr,
    root: &DesignScope,
    marker: Option<Marker<'_>>,
) -> Vec<LossNote> {
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
        match (rollback, marker) {
            (
                Some(Rollback::Before(Member::Feature(before))),
                Some(Marker {
                    before: marker,
                    first_item: true,
                }),
            ) if before == marker => {}
            (Some(Rollback::Before(member)), _) => {
                values.push(format!("{id} rollback (before {})", member_id(member)));
            }
            (None, None) => losses.push(F3dLossCode::WriterRollbackAssumed.note(format!(
                "{id}: the document does not state the timeline's rollback; it is written built \
                 through its end, with the current geometry as the state after its last feature"
            ))),
            (None, Some(Marker { before, .. })) => {
                losses.push(F3dLossCode::WriterRollbackAssumed.note(format!(
                    "{id}: the document does not state the timeline's rollback; the timeline \
                     marker is written after the features before {before}, the first feature \
                     the history-true form rolls back"
                )));
            }
            (Some(Rollback::End), _) => {}
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

/// A history state whose result body the B-rep stream does not write.
#[derive(Debug)]
pub(super) struct MissingResult {
    /// The feature of the state.
    pub(super) feature: FeatureId,
    /// The failure, which names the feature and its body.
    pub(super) error: CodecError,
}

/// The position among `written` of the result body of each feature of
/// `results`, in order. A feature whose result body is not among `written`
/// fails the history with its name and the body's, so the design's
/// fallback charges it.
fn state_bodies<'a>(
    results: impl IntoIterator<Item = (&'a FeatureId, &'a BodyId)>,
    written: &[&BodyId],
) -> Result<Vec<usize>, MissingResult> {
    results
        .into_iter()
        .map(|(feature, body)| {
            written
                .iter()
                .position(|&at| at == body)
                .ok_or_else(|| MissingResult {
                    feature: feature.clone(),
                    error: CodecError::NotImplemented(format!(
                        "the history state of {feature} holds its result body {body}, which the B-rep stream does not write"
                    )),
                })
        })
        .collect()
}
