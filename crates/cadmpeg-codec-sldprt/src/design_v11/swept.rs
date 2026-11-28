// SPDX-License-Identifier: Apache-2.0
//! What the extrude and revolve projections share: the member walk that
//! finds each `Unresolved` feature of a family with the sketches and bodies
//! before it, and the profile check.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::sketch::{Planar, SketchEntity};
use cadmpeg_ir::design::selection::{OrderedSelection, PickTarget};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::sketches::{SketchEntityId, SketchId};

use super::parameters::Entries;
use super::profile_check::{self, Difference, Unchecked};
use super::sketch::{pick, selection};

/// What the projections read besides the feature.
pub(super) struct Context<'a> {
    pub(super) model: &'a cadmpeg_ir::document::Model,
    pub(super) histories: &'a [crate::records::FeatureHistory],
    pub(super) entries: &'a Entries<'a>,
    /// The sketches the design states.
    pub(super) sketches: &'a HashSet<SketchId>,
    /// The entities of those sketches.
    pub(super) entities: &'a [SketchEntity<Planar>],
    pub(super) outside_profile: &'a HashSet<SketchEntityId>,
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

pub(super) struct Profile {
    pub(super) picks: OrderedSelection,
    pub(super) differences: Result<Vec<Difference>, Unchecked>,
}

pub(super) fn profile(
    sketch: &SketchId,
    context: &Context<'_>,
    solid: bool,
) -> Result<Profile, &'static str> {
    let loops = context
        .model
        .sketches
        .iter()
        .find(|record| &record.id == sketch)
        .map_or(&[][..], |record| &record.profiles[..]);
    let inspection = (|| {
        let mut curves = profile_check::curves(sketch, context.entities)?;
        let differences = profile_check::check(loops, &curves)?;
        if !solid || differences.is_empty() || context.outside_profile.is_empty() {
            return Ok((differences, None));
        }
        let count = curves.len();
        curves.retain(|id, _| !context.outside_profile.contains(id));
        if curves.len() == count
            || !profile_check::check(loops, &curves).is_ok_and(|after| after.is_empty())
        {
            return Ok((differences, None));
        }
        Ok((differences, profile_check::stored_regions(loops, &curves)))
    })();
    let (picks, differences) = match inspection {
        Ok((_, Some(regions))) => (
            regions
                .into_iter()
                .map(|region| {
                    pick(PickTarget::SketchRegion {
                        sketch: sketch.clone(),
                        region,
                    })
                })
                .collect(),
            Ok(Vec::new()),
        ),
        other => (
            vec![pick(PickTarget::Sketch(sketch.clone()))],
            other.map(|(differences, _)| differences),
        ),
    };
    Ok(Profile {
        picks: selection(picks)?,
        differences,
    })
}
