// SPDX-License-Identifier: Apache-2.0
//! The geometry item a design writes in place of its Base Feature
//! (cadir-v11.md §3.4, Geometry-true; f3d.md §3.1, "Records of a
//! sketch-driven Extrude"): which sketch-driven feature the writer writes
//! with its state and its names, and why the Base Feature holds it
//! otherwise. The rows every sketch-driven item shares are here; each family
//! adds its own.

use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::swept::Extrude;
use cadmpeg_ir::design::options::ProfileRef;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::BodyId;

use super::extrude::profile::{profile, ProfileLoop};
use super::extrude::{self, ExtrudePlan};
use super::parameters::{OwnedParameter, ParameterPlan};
use super::sketch::{PlannedSketch, SketchPlan};

/// The smallest linear tolerance the profile and the faces are checked
/// with, in mm: the decoder's linear resolution.
const MIN_LINEAR_TOLERANCE: f64 = 1e-5;
/// Millimetres per Fusion length unit (cm).
const MM_PER_CM: f64 = 10.0;

/// What the design writes for the scope's geometry feature.
#[derive(Debug)]
pub(super) enum Decision<'a> {
    /// The scope holds no typed sketch-driven feature among its geometry
    /// features.
    None,
    Item(Item<'a>),
    /// The feature stays in the Base Feature, for `reason`.
    BaseFeature {
        feature: &'a FeatureId,
        reason: String,
    },
}

impl<'a> Decision<'a> {
    /// Whether `feature` is written as the geometry item.
    pub(super) fn writes(&self, feature: &FeatureId) -> bool {
        self.item().is_some_and(|item| item.feature() == feature)
    }

    /// The reason the Base Feature holds `feature`, where the decided
    /// feature is it.
    pub(super) fn reason(&self, feature: &FeatureId) -> Option<&str> {
        match self {
            Self::BaseFeature {
                feature: decided,
                reason,
            } if *decided == feature => Some(reason),
            Self::None | Self::Item(_) | Self::BaseFeature { .. } => None,
        }
    }

    /// The written geometry item.
    pub(super) fn item(&self) -> Option<&Item<'a>> {
        match self {
            Self::Item(item) => Some(item),
            Self::None | Self::BaseFeature { .. } => None,
        }
    }
}

/// The sketch-driven feature the design writes as a timeline item in place
/// of the Base Feature.
#[derive(Debug)]
pub(super) enum Item<'a> {
    Extrude(Box<ExtrudePlan<'a>>),
}

impl<'a> Item<'a> {
    pub(super) fn feature(&self) -> &'a FeatureId {
        match self {
            Self::Extrude(plan) => plan.feature,
        }
    }

    /// The number of `Sketch` items before it in the timeline.
    pub(super) fn after(&self) -> usize {
        match self {
            Self::Extrude(plan) => plan.after,
        }
    }

    /// The parameters the item owns, in owner-local order.
    pub(super) fn owned(&self) -> Vec<&OwnedParameter> {
        match self {
            Self::Extrude(plan) => vec![&plan.along, &plan.taper],
        }
    }
}

/// The feature's typed operation the writer can write as an item.
enum Typed<'a> {
    Extrude(&'a Extrude),
}

fn typed<'a>(target: &'a CadIr, feature: &FeatureId) -> Option<Typed<'a>> {
    let record = target.design.features.iter().find(|f| &f.id == feature)?;
    match &record.definition.operation {
        FeatureOperation::Extrude(extrude) => Some(Typed::Extrude(extrude.as_ref())),
        _ => None,
    }
}

/// The decision for the scope's features in timeline order: the first
/// stateful feature of a family the writer writes as an item. `failure` is
/// the error of an earlier attempt to write the item. A written item whose
/// parameter binds a user parameter takes that parameter out of
/// `parameters`; its parameters take the source ordinals from `ordinal`.
pub(super) fn plan<'a>(
    target: &'a CadIr,
    scope: &'a DesignScope,
    features: &[&'a FeatureId],
    sketches: &SketchPlan<'a>,
    parameters: &mut ParameterPlan,
    failure: Option<&str>,
    ordinal: u32,
) -> Decision<'a> {
    let stateful = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| !target.is_stateless(feature, None).unwrap_or(false))
        .collect::<Vec<_>>();
    let Some((position, feature, operation)) = stateful.iter().find_map(|(position, feature)| {
        typed(target, feature).map(|operation| (*position, **feature, operation))
    }) else {
        return Decision::None;
    };
    let base = |reason: String| Decision::BaseFeature { feature, reason };
    if let Some(failure) = failure {
        return base(format!("the writer cannot write its records ({failure})"));
    }
    if stateful.len() > 1 {
        return base("the timeline holds other geometry features".to_owned());
    }
    let rows = Rows {
        target,
        scope,
        features,
        sketches,
        feature,
        position,
    };
    let planned = match operation {
        Typed::Extrude(operation) => extrude::plan(&rows, operation, parameters, ordinal)
            .map(|plan| Item::Extrude(Box::new(plan))),
    };
    match planned {
        Ok(item) => Decision::Item(item),
        Err(reason) => base(reason.to_owned()),
    }
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
}

/// What the shared rows establish of a sketch-driven feature.
pub(super) struct Profiled<'a, 'p> {
    pub(super) body: &'a BodyId,
    /// The position of its sketch in [`SketchPlan::items`].
    pub(super) sketch: usize,
    pub(super) planned: &'p PlannedSketch<'a>,
    /// The number of `Sketch` items before it in the timeline.
    pub(super) after: usize,
    /// The number of timeline items before it: the `Sketch` items and the
    /// `WorkPlane` items that precede them.
    pub(super) preceding: usize,
    /// The loops of the profile region in stored order: the exterior loop
    /// in ring order from the curve of side token `3`, then the holes.
    pub(super) loops: Vec<ProfileLoop>,
    /// The linear tolerance, in mm.
    pub(super) linear: f64,
}

impl<'a, 'p> Rows<'a, 'p> {
    /// The shared rows in order; the first that fails names the reason: one
    /// body, the timeline rolled to its end, the current state its result,
    /// and the profile `selection` one region of one written sketch before
    /// the feature, on an origin plane and not mirrored.
    pub(super) fn profiled(
        &self,
        selection: &ProfileRef,
    ) -> Result<Profiled<'a, 'p>, &'static str> {
        let target = self.target;
        let [body] = target.geometry.bodies.as_slice() else {
            return Err("the design holds more than one body");
        };
        self.rollback()?;
        self.current(&body.id)?;
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
            body: &body.id,
            sketch,
            planned,
            after,
            preceding: after
                + self
                    .sketches
                    .planes
                    .iter()
                    .filter(|plane| plane.first < after)
                    .count(),
            loops,
            linear,
        })
    }

    /// The feature's timeline is rolled to its end.
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
                rollback: Rollback::End,
                ..
            } => Ok(()),
            Container::Ordered { .. } | Container::Unordered { .. } => {
                Err("its timeline's rollback is not at the end")
            }
        }
    }

    /// The current state is the state after the feature or a later member,
    /// and holds the one body.
    fn current(&self, body: &BodyId) -> Result<(), &'static str> {
        const NOT_ITS_RESULT: &str = "the current state is not its result";
        let [state] = self.scope.current.as_slice() else {
            return Err(NOT_ITS_RESULT);
        };
        let Position::After(Member::Feature(at)) = &state.at else {
            return Err(NOT_ITS_RESULT);
        };
        let later = self
            .features
            .iter()
            .position(|feature| *feature == at)
            .is_some_and(|at| at >= self.position);
        if !later || !matches!(state.bodies.as_slice(), [only] if only == body) {
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
