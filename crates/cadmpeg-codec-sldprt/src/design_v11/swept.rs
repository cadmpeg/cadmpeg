// SPDX-License-Identifier: Apache-2.0
//! What the extrude and revolve projections share: the member walk that
//! finds each `Unresolved` feature of a family with the sketches and bodies
//! before it, the profile sketch, the boolean, and the profile check.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::sketch::{Planar, SketchEntity};
use cadmpeg_ir::design::options::{Boolean, BooleanOp};
use cadmpeg_ir::features::operation_settings::BooleanOp as DecodedOp;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::sketches::SketchId;

use super::parameters::Entries;
use super::profile_check::{self, Difference, Unchecked};

/// What the projections read besides the feature.
pub(super) struct Context<'a> {
    pub(super) model: &'a cadmpeg_ir::document::Model,
    pub(super) histories: &'a [crate::records::FeatureHistory],
    pub(super) entries: &'a Entries<'a>,
    /// The sketches the design states.
    pub(super) sketches: &'a HashSet<SketchId>,
    /// The entities of those sketches.
    pub(super) entities: &'a [SketchEntity<Planar>],
}

/// What the timeline holds before a feature.
pub(super) struct Before<'a> {
    /// The sketch of each sketch feature, `None` where the sketch stays
    /// `Unresolved`.
    pub(super) sketch_of: &'a HashMap<FeatureId, Option<SketchId>>,
    /// The sketches of the earlier members.
    pub(super) preceding: &'a HashSet<SketchId>,
    /// Whether an earlier unsuppressed member makes or changes a body.
    pub(super) body: bool,
}

/// Visit each `Unresolved` feature of `family` in `features`, in member
/// order, with what the timeline holds before it.
pub(super) fn walk(
    features: &mut [Feature],
    family: OperationFamily,
    mut visit: impl FnMut(&mut Feature, &Before<'_>),
) {
    let sketch_of = features
        .iter()
        .filter_map(|feature| match &feature.definition.operation {
            FeatureOperation::Sketch(sketch) => Some((feature.id.clone(), Some(sketch.clone()))),
            FeatureOperation::Unresolved {
                family: OperationFamily::Sketch,
            } => Some((feature.id.clone(), None)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut body = false;
    let mut preceding = HashSet::new();
    for feature in features.iter_mut() {
        if let FeatureOperation::Sketch(sketch) = &feature.definition.operation {
            preceding.insert(sketch.clone());
        }
        let unresolved = match &feature.definition.operation {
            FeatureOperation::Unresolved { family } => Some(*family),
            FeatureOperation::Datum(_) | FeatureOperation::Sketch(_) => None,
            _ => {
                body |= !suppressed(feature);
                continue;
            }
        };
        match unresolved {
            Some(found) if found == family => visit(
                feature,
                &Before {
                    sketch_of: &sketch_of,
                    preceding: &preceding,
                    body,
                },
            ),
            Some(
                OperationFamily::Datum | OperationFamily::Sketch | OperationFamily::SpatialSketch,
            )
            | None => continue,
            Some(_) => {}
        }
        body |= !suppressed(feature);
    }
}

fn suppressed(feature: &Feature) -> bool {
    matches!(feature.suppressed, Some(Dimension::Literal(true)))
}

/// The boolean of a decoded operation: a join into a part that holds no
/// body yet is a new body (§6).
pub(super) fn boolean(op: DecodedOp, body_before: bool) -> Option<Boolean> {
    let apply = |op| Boolean::Apply { op, targets: None };
    match op {
        DecodedOp::Join if !body_before => Some(Boolean::NewBody),
        DecodedOp::Join => Some(apply(BooleanOp::Join)),
        DecodedOp::Cut => Some(apply(BooleanOp::Cut)),
        DecodedOp::Intersect => Some(apply(BooleanOp::Intersect)),
        DecodedOp::NewBody => Some(Boolean::NewBody),
        DecodedOp::Unresolved => None,
    }
}

/// How the closed regions of `sketch` differ from the profile loops the
/// decoder resolved for it.
pub(super) fn differences(
    sketch: &SketchId,
    context: &Context<'_>,
) -> Result<Vec<Difference>, Unchecked> {
    let loops = context
        .model
        .sketches
        .iter()
        .find(|record| &record.id == sketch)
        .map_or(&[][..], |record| &record.profiles[..]);
    profile_check::check(loops, &profile_check::curves(sketch, context.entities)?)
}
