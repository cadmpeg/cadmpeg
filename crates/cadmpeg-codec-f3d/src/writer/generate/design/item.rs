// SPDX-License-Identifier: Apache-2.0
//! The geometry item a design writes in place of its Base Feature
//! (cadir-v11.md §3.4, Geometry-true; f3d.md §3.1, "Records of a
//! sketch-driven Extrude"): which sketch-driven feature the writer writes
//! with its state and its names, and why the Base Feature holds it
//! otherwise. The rows every sketch-driven item shares are here; each family
//! adds its own.

use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::{Extrude, Revolve};
use cadmpeg_ir::design::options::ProfileRef;
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp, SolidOutput};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::BodyId;

use super::extrude::profile::{profile, ProfileLoop};
use super::extrude::{self, ExtrudePlan};
use super::parameters::{OwnedParameter, ParameterPlan};
use super::revolve::{self, RevolvePlan};
use super::sketch::{PlannedSketch, SketchPlan};

/// The smallest linear tolerance the profile and the faces are checked
/// with, in mm: the decoder's linear resolution.
const MIN_LINEAR_TOLERANCE: f64 = 1e-5;
/// Millimetres per Fusion length unit (cm).
const MM_PER_CM: f64 = 10.0;

/// What the design writes for the scope's geometry features.
#[derive(Debug)]
pub(super) enum Decision<'a> {
    /// The scope holds no typed sketch-driven feature among its geometry
    /// features.
    None,
    /// The geometry features, each written as an item with its state, in
    /// timeline order. Each item after the first makes a new body.
    Items(Vec<Item<'a>>),
    /// The first typed feature stays in the Base Feature, with every
    /// geometry feature of the run. `reason` concerns `cause`, the feature
    /// the writer cannot write with its state, where that is a later
    /// feature, and `feature` otherwise.
    BaseFeature {
        feature: &'a FeatureId,
        cause: Option<&'a FeatureId>,
        reason: String,
    },
}

impl<'a> Decision<'a> {
    /// Whether `feature` is written as a geometry item.
    pub(super) fn writes(&self, feature: &FeatureId) -> bool {
        self.items().iter().any(|item| item.feature() == feature)
    }

    /// The reason the Base Feature holds `feature`, where the decision
    /// names it: the decided feature, or the later feature that causes the
    /// decision.
    pub(super) fn reason(&self, feature: &FeatureId) -> Option<String> {
        let Self::BaseFeature {
            feature: decided,
            cause,
            reason,
        } = self
        else {
            return None;
        };
        match cause {
            Some(cause) if *cause == feature => Some(reason.clone()),
            Some(cause) if *decided == feature => {
                Some(format!("the writer cannot write {cause} with its state"))
            }
            None if *decided == feature => Some(reason.clone()),
            Some(_) | None => None,
        }
    }

    /// The written geometry items, in timeline order.
    pub(super) fn items(&self) -> &[Item<'a>] {
        match self {
            Self::Items(items) => items,
            Self::None | Self::BaseFeature { .. } => &[],
        }
    }
}

/// The sketch-driven feature the design writes as a timeline item in place
/// of the Base Feature.
#[derive(Debug)]
pub(super) enum Item<'a> {
    Extrude(Box<ExtrudePlan<'a>>),
    Revolve(Box<RevolvePlan<'a>>),
}

impl<'a> Item<'a> {
    pub(super) fn feature(&self) -> &'a FeatureId {
        match self {
            Self::Extrude(plan) => plan.feature,
            Self::Revolve(plan) => plan.feature,
        }
    }

    /// The number of `Sketch` items before it in the timeline.
    pub(super) fn after(&self) -> usize {
        match self {
            Self::Extrude(plan) => plan.after,
            Self::Revolve(plan) => plan.after,
        }
    }

    /// The body of its state; `None` for a rolled-back item.
    pub(super) fn body(&self) -> Option<&'a BodyId> {
        match self {
            Self::Extrude(plan) => plan.result.as_ref().map(|result| result.body),
            Self::Revolve(plan) => plan.result.as_ref().map(|result| result.body),
        }
    }

    /// The parameters the item owns, in owner-local order.
    pub(super) fn owned(&self) -> Vec<&OwnedParameter> {
        match self {
            Self::Extrude(plan) => vec![&plan.along, &plan.taper],
            Self::Revolve(plan) => vec![&plan.angle],
        }
    }

    /// The source ordinal after the item's parameters, which take the
    /// ordinals from `ordinal`.
    pub(super) fn next_ordinal(&self, ordinal: u32) -> Option<u32> {
        ordinal.checked_add(u32::try_from(self.owned().len()).ok()?)
    }
}

/// The feature's typed operation the writer can write as an item.
#[derive(Clone, Copy)]
enum Typed<'a> {
    Extrude(&'a Extrude),
    Revolve(&'a Revolve),
}

impl Typed<'_> {
    /// Whether the operation states a new body: it leaves the bodies of
    /// the state before it unchanged.
    fn makes_new_body(self) -> bool {
        let output = match self {
            Self::Extrude(extrude) => &extrude.output,
            Self::Revolve(revolve) => &revolve.output,
        };
        matches!(output.boolean, Some(Boolean::NewBody))
    }
}

fn typed<'a>(target: &'a CadIr, feature: &FeatureId) -> Option<Typed<'a>> {
    let record = target.design.features.iter().find(|f| &f.id == feature)?;
    match &record.definition.operation {
        FeatureOperation::Extrude(extrude) => Some(Typed::Extrude(extrude.as_ref())),
        FeatureOperation::Revolve(revolve) => Some(Typed::Revolve(revolve.as_ref())),
        _ => None,
    }
}

/// Whether the writer can write `feature` with its state as a geometry
/// item of a run: a family it writes as an item that makes a new body, or
/// any such family where the feature is the run's `first` stateful feature.
/// `Err` names why it cannot.
pub(super) fn carries(
    target: &CadIr,
    feature: &FeatureId,
    first: bool,
) -> Result<(), &'static str> {
    let operation = typed(target, feature).ok_or(OTHER_FAMILY)?;
    if first || operation.makes_new_body() {
        Ok(())
    } else {
        Err(CHANGES_AN_EARLIER_BODY)
    }
}

/// Why a geometry feature is not written with its state where the writer
/// writes no item of its family.
const OTHER_FAMILY: &str = "the writer writes no item of its family with its state";
/// Why a geometry feature after the first is not written with its state
/// where it changes a body of the state before it.
const CHANGES_AN_EARLIER_BODY: &str = "it changes a body of the state before it, and f3d-open-items.md DR-1054 leaves open the record changes of such a state";

/// The features the geometry item is chosen from: the scope's features in
/// timeline order, or the run before the timeline marker of the
/// history-true form (cadir-v11.md §3.4).
#[derive(Clone, Copy)]
pub(super) struct Run<'a, 'p> {
    pub(super) features: &'p [&'a FeatureId],
    /// Whether features follow the run in the timeline, rolled back.
    pub(super) ends: bool,
}

/// The decision for the run's features in timeline order: every stateful
/// feature of the run, each written as an item with its state, where each
/// is of a family the writer writes as an item and each after the first
/// makes a new body (f3d.md §3.1, "History states"). `failure` is why an
/// earlier attempt could not write the items, with the feature it concerns
/// where it names one. A written item whose parameter
/// binds a user parameter takes that parameter out of `parameters`; the
/// items' parameters take the source ordinals from `ordinal`. The
/// `BaseFeature` decision names the first typed feature, and the later
/// feature its reason concerns.
pub(super) fn plan<'a>(
    target: &'a CadIr,
    scope: &'a DesignScope,
    run: Run<'a, '_>,
    sketches: &SketchPlan<'a>,
    parameters: &mut ParameterPlan,
    failure: Option<(String, Option<&FeatureId>)>,
    ordinal: u32,
) -> Decision<'a> {
    let features = run.features;
    let stateful = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| !target.is_stateless(feature, None).unwrap_or(false))
        .map(|(position, feature)| (position, *feature))
        .collect::<Vec<_>>();
    let Some(feature) = stateful
        .iter()
        .find_map(|(_, feature)| typed(target, feature).map(|_| *feature))
    else {
        return Decision::None;
    };
    let base = |cause: Option<&'a FeatureId>, reason: String| Decision::BaseFeature {
        feature,
        cause: cause.filter(|cause| *cause != feature),
        reason,
    };
    if let Some((failure, cause)) = failure {
        let cause = cause.and_then(|cause| features.iter().copied().find(|at| *at == cause));
        return base(cause, failure);
    }
    let mut chain = Vec::with_capacity(stateful.len());
    for &(position, feature) in &stateful {
        let Some(operation) = typed(target, feature) else {
            return base(
                None,
                "the timeline holds other geometry features".to_owned(),
            );
        };
        if !chain.is_empty() && !operation.makes_new_body() {
            return base(Some(feature), CHANGES_AN_EARLIER_BODY.to_owned());
        }
        chain.push((position, feature, operation));
    }
    let rows = |position: usize, feature: &'a FeatureId, body, items_before| Rows {
        target,
        scope,
        features,
        sketches,
        feature,
        position,
        ends: run.ends,
        rolled_back: false,
        body,
        items_before,
    };
    let Some(&(last_position, last, _)) = chain.last() else {
        return Decision::None;
    };
    let state = match rows(last_position, last, None, 0).state(chain.len()) {
        Ok(state) => state,
        Err(reason) => return base(None, reason.to_owned()),
    };
    let mut trial = parameters.clone();
    let mut next = Some(ordinal);
    let mut items = Vec::<Item<'a>>::with_capacity(chain.len());
    for &(position, feature, operation) in &chain {
        let items_before = items.len();
        let Some(item_ordinal) = next else {
            return base(
                Some(feature),
                "its parameters exceed the source ordinals".to_owned(),
            );
        };
        let mut first_error = None;
        let mut planned = None;
        for body in state
            .iter()
            .filter(|body| !items.iter().any(|item| item.body() == Some(*body)))
        {
            match family_plan(
                &rows(position, feature, Some(body), items_before),
                operation,
                &mut trial,
                item_ordinal,
            ) {
                Ok(item) => {
                    planned = Some(item);
                    break;
                }
                Err(reason) => {
                    first_error.get_or_insert(reason);
                }
            }
        }
        let Some(item) = planned else {
            return base(
                Some(feature),
                first_error
                    .unwrap_or("its faces do not match its profile")
                    .to_owned(),
            );
        };
        next = item.next_ordinal(item_ordinal);
        items.push(item);
    }
    *parameters = trial;
    Decision::Items(items)
}

/// The definition of the feature at `position` in `features`, the scope's
/// features in timeline order, as an item after the timeline marker of the
/// history-true form: rolled back, without its state (cadir-v11.md §3.4).
/// Its parameters take the source ordinals from `ordinal`. `Err` names why
/// the writer does not write it.
pub(super) fn rolled_back<'a>(
    target: &'a CadIr,
    scope: &'a DesignScope,
    features: &[&'a FeatureId],
    position: usize,
    sketches: &SketchPlan<'a>,
    parameters: &mut ParameterPlan,
    ordinal: u32,
) -> Result<Item<'a>, &'static str> {
    let feature = features[position];
    let operation =
        typed(target, feature).ok_or("the writer writes no rolled-back item of its family")?;
    let rows = Rows {
        target,
        scope,
        features,
        sketches,
        feature,
        position,
        ends: true,
        rolled_back: true,
        body: None,
        items_before: 0,
    };
    family_plan(&rows, operation, parameters, ordinal)
}

/// The rows of the feature's family. A failed plan leaves `parameters` as
/// it was.
fn family_plan<'a>(
    rows: &Rows<'a, '_>,
    operation: Typed<'a>,
    parameters: &mut ParameterPlan,
    ordinal: u32,
) -> Result<Item<'a>, &'static str> {
    let mut trial = parameters.clone();
    let planned = match operation {
        Typed::Extrude(operation) => extrude::plan(rows, operation, &mut trial, ordinal)
            .map(|plan| Item::Extrude(Box::new(plan))),
        Typed::Revolve(operation) => revolve::plan(rows, operation, &mut trial, ordinal)
            .map(|plan| Item::Revolve(Box::new(plan))),
    }?;
    *parameters = trial;
    Ok(planned)
}

/// What the shared rows read.
pub(super) struct Rows<'a, 'p> {
    pub(super) target: &'a CadIr,
    scope: &'a DesignScope,
    features: &'p [&'a FeatureId],
    pub(super) sketches: &'p SketchPlan<'a>,
    pub(super) feature: &'a FeatureId,
    /// The feature's position in `features`.
    position: usize,
    /// Whether rolled-back features follow `features` in the timeline.
    ends: bool,
    /// Whether the feature itself follows the timeline marker: the rows of
    /// its state do not apply.
    pub(super) rolled_back: bool,
    /// The body of its state, which [`Rows::state`] holds; `None` for a
    /// rolled-back feature.
    body: Option<&'a BodyId>,
    /// The number of geometry items before it in the timeline.
    items_before: usize,
}

/// What the shared rows establish of a sketch-driven feature.
pub(super) struct Profiled<'a, 'p> {
    /// The body of its state; `None` for a rolled-back feature.
    pub(super) body: Option<&'a BodyId>,
    /// The position of its sketch in [`SketchPlan::items`].
    pub(super) sketch: usize,
    pub(super) planned: &'p PlannedSketch<'a>,
    /// The number of `Sketch` items before it in the timeline.
    pub(super) after: usize,
    /// The persistent identities that the timeline items before it own:
    /// one for each `Sketch` item and each `WorkPlane` item before them, two
    /// for each geometry item.
    pub(super) preceding: usize,
    /// The loops of the profile region in stored order: the exterior loop
    /// in ring order from the curve of side token `3`, then the holes.
    pub(super) loops: Vec<ProfileLoop>,
    /// The linear tolerance, in mm.
    pub(super) linear: f64,
}

impl<'a, 'p> Rows<'a, 'p> {
    /// The shared rows of the profile in order; the first that fails names
    /// the reason: the profile `selection` is one region of one written
    /// sketch before the feature, on an origin plane and not mirrored.
    pub(super) fn profiled(
        &self,
        selection: &ProfileRef,
    ) -> Result<Profiled<'a, 'p>, &'static str> {
        let target = self.target;
        let body = if self.rolled_back {
            None
        } else {
            Some(self.body.ok_or("it has no body")?)
        };
        let (sketch, after) = self.sketch(selection)?;
        let planned = &self.sketches.items[sketch];
        if planned.origin_plane().is_none() {
            return Err("its sketch lies on a construction plane");
        }
        let linear = target.tolerances.linear.get().max(MIN_LINEAR_TOLERANCE);
        let loops = profile(planned, linear / MM_PER_CM)?;
        if planned.mirrored {
            return Err("its sketch is written mirrored on its origin plane");
        }
        Ok(Profiled {
            body,
            sketch,
            planned,
            after,
            preceding: after
                + self
                    .sketches
                    .planes
                    .iter()
                    .filter(|plane| plane.first < after)
                    .count()
                + 2 * self.items_before,
            loops,
            linear,
        })
    }

    /// The bodies of the state of the run's `items` geometry items, in the
    /// design's body order, where the feature is the last of them; the
    /// first row that fails names the reason: the design holds one body
    /// for each item, the timeline is rolled to its end, and the current
    /// state is the result of the feature. A current state after the
    /// timeline marker fails first.
    fn state(&self, items: usize) -> Result<Vec<&'a BodyId>, &'static str> {
        const AFTER_THE_MARKER: &str = "the current state stands after the timeline marker, and the writer writes no evaluation of the document as the state at the marker";
        if let [state] = self.scope.current.as_slice() {
            if self.ends && after_the_run(&state.at, self.features) {
                return Err(AFTER_THE_MARKER);
            }
        }
        let bodies = &self.target.geometry.bodies;
        if bodies.len() != items {
            return Err(if items == 1 {
                "the design holds more than one body"
            } else {
                "the design holds other bodies than its geometry items make"
            });
        }
        self.rollback()?;
        self.current(bodies)?;
        Ok(bodies.iter().map(|body| &body.id).collect())
    }

    /// The feature's timeline is rolled to its end, or, where rolled-back
    /// features follow the run, to a feature after the run. An unstated
    /// rollback is read as the end (cadir-v11.md §5.2), which the design's
    /// report names as an assumption.
    fn rollback(&self) -> Result<(), &'static str> {
        let member = Member::Feature(self.feature.clone());
        let container = self
            .target
            .design
            .containers
            .iter()
            .find(|container| match container {
                Container::Ordered { members, .. } => members.contains(&member),
                Container::Unordered { .. } => false,
            })
            .ok_or("it is in no ordered timeline")?;
        match container {
            Container::Ordered {
                rollback: None | Some(Rollback::End),
                ..
            } => Ok(()),
            Container::Ordered {
                rollback: Some(Rollback::Before(Member::Feature(before))),
                ..
            } if self.ends && !self.features.contains(&before) => Ok(()),
            Container::Ordered { .. } | Container::Unordered { .. } => {
                Err("its timeline's rollback is not at the end")
            }
        }
    }

    /// The current state is the state after the feature or a later member
    /// of the run, and holds the design's `bodies`.
    fn current(&self, bodies: &[cadmpeg_ir::topology::Body]) -> Result<(), &'static str> {
        const NOT_ITS_RESULT: &str = "the current state is not its result";
        let [state] = self.scope.current.as_slice() else {
            return Err(NOT_ITS_RESULT);
        };
        let later = match &state.at {
            Position::After(Member::Feature(at)) => self
                .features
                .iter()
                .position(|feature| *feature == at)
                .is_some_and(|at| at >= self.position),
            // The state at an unstated rollback, read as built through the
            // end of the timeline that holds the feature, where no feature
            // follows the run: §3.4 makes it no member's state.
            Position::After(Member::Container(timeline)) if !self.ends => {
                self.target.design.containers.iter().any(|container| {
                    matches!(
                        container,
                        Container::Ordered { id, members, rollback: None, .. }
                            if id == timeline
                                && members.contains(&Member::Feature(self.feature.clone()))
                    )
                })
            }
            Position::After(Member::Container(_)) | Position::Start(_) => false,
        };
        let holds = state.bodies.len() == bodies.len()
            && bodies.iter().all(|body| state.bodies.contains(&body.id));
        if !later || !holds {
            return Err(NOT_ITS_RESULT);
        }
        Ok(())
    }

    /// The profile is one written sketch before the feature. Returns its
    /// position in the plan and the number of written sketches before the
    /// feature.
    fn sketch(&self, selection: &ProfileRef) -> Result<(usize, usize), &'static str> {
        const NOT_WRITTEN: &str = "its sketch is not written";
        let Picks::Named(members) = &selection.picks else {
            return Err(NOT_WRITTEN);
        };
        let [picked] = members.as_slice() else {
            return Err(NOT_WRITTEN);
        };
        let PickTarget::Sketch(sketch) = &picked.target else {
            return Err(NOT_WRITTEN);
        };
        let owner = self
            .target
            .design
            .sketches
            .iter()
            .find(|record| &record.id == sketch)
            .map(|record| &record.owner)
            .ok_or(NOT_WRITTEN)?;
        let position_of = |feature: &FeatureId| {
            self.features
                .iter()
                .position(|candidate| *candidate == feature)
        };
        let index = self
            .sketches
            .items
            .iter()
            .position(|item| item.feature == owner)
            .ok_or(NOT_WRITTEN)?;
        if position_of(owner).is_none_or(|at| at >= self.position) {
            return Err(NOT_WRITTEN);
        }
        let after = self
            .sketches
            .items
            .iter()
            .filter(|item| position_of(item.feature).is_some_and(|at| at < self.position))
            .count();
        Ok((index, after))
    }
}

/// Whether the state at `at` stands after every feature of `run`: after a
/// feature outside the run, or after a timeline.
pub(super) fn after_the_run(at: &Position, run: &[&FeatureId]) -> bool {
    match at {
        Position::After(Member::Feature(at)) => !run.contains(&at),
        Position::After(Member::Container(_)) => true,
        Position::Start(_) => false,
    }
}

/// One new solid; the one-body row leaves no body for a join to act on.
pub(super) fn new_solid(output: &SolidOutput) -> Result<(), &'static str> {
    let new = matches!(
        output.boolean,
        Some(
            Boolean::NewBody
                | Boolean::Apply {
                    op: BooleanOp::Join,
                    ..
                }
        )
    );
    if new && matches!(output.body, None | Some(BodyForm::Solid)) {
        Ok(())
    } else {
        Err("it does not make one new solid")
    }
}
