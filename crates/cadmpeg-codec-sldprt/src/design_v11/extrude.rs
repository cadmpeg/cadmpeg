// SPDX-License-Identifier: Apache-2.0
//! Extrudes (§6.3), from the decoder's typed extrusions.
//!
//! An extrude whose profile is a whole sketch the design states, named by the
//! sketch or by its feature, and whose limits are blind distances or
//! through-all, is typed: the profile picks each region of the sketch's
//! profile loops the decoder resolved (`region`), else the whole sketch, a
//! blind limit binds the dimension that holds its distance, and a merge into
//! a part that holds no body yet is a new body (§6). An extrude whose start,
//! limits, draft, or profile the decoder reads only in part stays
//! `Unresolved`, and so does one whose sketch the timeline lists after it,
//! which its `Input` profile pick cannot see. An extrude whose sketch stays `Unresolved` still binds its
//! blind depths, each a length.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin};
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, BooleanOp, NativeOptions, SolidOutput, SymmetricMeasure,
};
use cadmpeg_ir::design::selection::{PickTarget, Picked};
use cadmpeg_ir::design::units::Quantity;
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::extents::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, ParameterId};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{SketchEntityId, SketchId};

use super::parameters::Entries;
use super::region::regions;
use super::sketch::{pick, selection};

/// The extrudes typed, and the quantity of each parameter a limit binds.
#[derive(Default)]
pub(super) struct Extrudes {
    pub(super) bound: HashMap<ParameterId, Quantity>,
    /// The extrude features left `Unresolved`, with the reason.
    pub(super) unprojected: Vec<(FeatureId, &'static str)>,
    /// The typed extrudes whose blind depth is the record's only dimension,
    /// named neither `Depth` nor `D1`, with that dimension.
    pub(super) assumed: Vec<(FeatureId, ParameterId)>,
    /// The typed extrudes that pick their whole sketch, with the reason they
    /// pick no region.
    pub(super) whole_sketch: Vec<(FeatureId, &'static str)>,
}

/// What the projection reads besides the feature.
pub(super) struct Context<'a> {
    pub(super) model: &'a cadmpeg_ir::document::Model,
    pub(super) histories: &'a [crate::records::FeatureHistory],
    pub(super) entries: &'a Entries<'a>,
    /// The sketches the design states.
    pub(super) sketches: &'a HashSet<SketchId>,
    /// The curve of each entity of those sketches that is no construction.
    pub(super) curves: &'a HashMap<&'a SketchEntityId, &'a PlanarCurve>,
}

/// What a typed extrude adds to the notes.
#[derive(Default)]
struct Notes {
    /// The dimension its blind depth binds as its record's only one.
    assumed: Option<ParameterId>,
    /// Why it picks its whole sketch.
    whole_sketch: Option<&'static str>,
}

/// Type each `Unresolved` extrude of `features`, in member order.
pub(super) fn project(features: &mut [Feature], context: &Context<'_>) -> Extrudes {
    let mut out = Extrudes::default();
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
    let mut body_before = false;
    let mut preceding = HashSet::new();
    for feature in features.iter_mut() {
        if let FeatureOperation::Sketch(sketch) = &feature.definition.operation {
            preceding.insert(sketch.clone());
        }
        let family = match &feature.definition.operation {
            FeatureOperation::Unresolved { family } => Some(*family),
            FeatureOperation::Datum(_) | FeatureOperation::Sketch(_) => None,
            _ => {
                body_before |= !suppressed(feature);
                continue;
            }
        };
        match family {
            Some(OperationFamily::Extrude) => {
                let mut bound = Vec::new();
                let mut notes = Notes::default();
                let typed = extrude(
                    feature,
                    context,
                    &sketch_of,
                    &preceding,
                    body_before,
                    &mut bound,
                    &mut notes,
                );
                out.bound.extend(bound);
                match typed {
                    Ok(extrude) => {
                        feature.definition.operation = FeatureOperation::Extrude(Box::new(extrude));
                        out.assumed.extend(
                            notes
                                .assumed
                                .map(|parameter| (feature.id.clone(), parameter)),
                        );
                        out.whole_sketch.extend(
                            notes
                                .whole_sketch
                                .map(|reason| (feature.id.clone(), reason)),
                        );
                    }
                    Err(reason) => out.unprojected.push((feature.id.clone(), reason)),
                }
            }
            Some(
                OperationFamily::Datum | OperationFamily::Sketch | OperationFamily::SpatialSketch,
            )
            | None => continue,
            Some(_) => {}
        }
        body_before |= !suppressed(feature);
    }
    out
}

fn suppressed(feature: &Feature) -> bool {
    matches!(feature.suppressed, Some(Dimension::Literal(true)))
}

type Bound = Vec<(ParameterId, Quantity)>;

fn extrude(
    feature: &Feature,
    context: &Context<'_>,
    sketch_of: &HashMap<FeatureId, Option<SketchId>>,
    preceding: &HashSet<SketchId>,
    body_before: bool,
    bound: &mut Bound,
    notes: &mut Notes,
) -> Result<Extrude, &'static str> {
    let source = context
        .model
        .features
        .iter()
        .find(|candidate| candidate.id == feature.id)
        .ok_or("the extrude has no decoded record")?;
    let FeatureDefinition::Operation(v10::FeatureOperation::Extrude {
        profile,
        direction,
        start,
        extent,
        op,
        solid,
        face_maker,
        inner_wire_taper,
        length_along_profile_normal,
        allow_multi_profile_faces,
    }) = source.evaluation.definition()
    else {
        return Err("the extrude's decoded record is no extrusion");
    };
    if face_maker.is_some()
        || inner_wire_taper.is_some()
        || length_along_profile_normal.is_some()
        || allow_multi_profile_faces.is_some()
    {
        return Err("the extrude states an option of another format");
    }
    let sketch = match profile {
        ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)) => Some(sketch),
        ProfileRef::Planar(PlanarProfileRef::Feature(owner)) => sketch_of
            .get(owner)
            .ok_or("the extrude's profile feature states no sketch")?
            .as_ref(),
        _ => return Err("the extrude's profile is no whole sketch"),
    };
    let direction = match direction {
        cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {} => {
            ExtrudeDirection::ProfileNormal { reversed: None }
        }
        _ => return Err("the extrude's direction is not the profile normal"),
    };
    if !matches!(
        start,
        cadmpeg_ir::features::extents::ExtrudeStart::ProfilePlane {}
    ) {
        return Err("the extrude starts off the profile plane");
    }
    let depths = Depths::new(feature, context);
    let span = span(extent, &depths, bound)?;
    if let ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } = extent {
        if matches!(side.termination, LinearTermination::Blind { .. }) && depths.first.is_none() {
            notes.assumed.clone_from(&depths.sole);
        }
    }
    let sketch = sketch
        .filter(|sketch| context.sketches.contains(*sketch))
        .ok_or("the extrude's sketch is not stated")?;
    if !preceding.contains(sketch) {
        return Err("the extrude's sketch follows it in the timeline");
    }
    let boolean = match op {
        cadmpeg_ir::features::operation_settings::BooleanOp::Join if !body_before => {
            Some(Boolean::NewBody)
        }
        cadmpeg_ir::features::operation_settings::BooleanOp::Join => Some(apply(BooleanOp::Join)),
        cadmpeg_ir::features::operation_settings::BooleanOp::Cut => Some(apply(BooleanOp::Cut)),
        cadmpeg_ir::features::operation_settings::BooleanOp::Intersect => {
            Some(apply(BooleanOp::Intersect))
        }
        cadmpeg_ir::features::operation_settings::BooleanOp::NewBody => Some(Boolean::NewBody),
        cadmpeg_ir::features::operation_settings::BooleanOp::Unresolved => None,
    };
    let picks = region_picks(sketch, context).unwrap_or_else(|reason| {
        notes.whole_sketch = Some(reason);
        vec![pick(PickTarget::Sketch(sketch.clone()))]
    });
    Ok(Extrude {
        profile: selection(picks)?,
        loops: None,
        direction: Some(direction),
        origin: Some(ExtrudeOrigin::ProfilePlane),
        span: Some(span),
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: SolidOutput {
            body: solid.map(|solid| {
                if solid {
                    BodyForm::Solid
                } else {
                    BodyForm::Sheet
                }
            }),
            boolean,
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })
}

/// One `SketchRegion` pick for each region of `sketch`'s profile loops, or
/// the reason it has none.
fn region_picks(sketch: &SketchId, context: &Context<'_>) -> Result<Vec<Picked>, &'static str> {
    let record = context
        .model
        .sketches
        .iter()
        .find(|record| &record.id == sketch)
        .ok_or("its sketch record is missing")?;
    if record.profiles.is_empty() {
        return Err("the decoder resolved no profile loop of its sketch");
    }
    Ok(regions(&record.profiles, |entity| {
        context.curves.get(entity).copied()
    })?
    .into_iter()
    .map(|region| {
        pick(PickTarget::SketchRegion {
            sketch: sketch.clone(),
            region,
        })
    })
    .collect())
}

fn apply(op: BooleanOp) -> Boolean {
    Boolean::Apply { op, targets: None }
}

/// The span of `extent`, its blind distances bound to the dimensions that
/// hold them. A mid-plane depth is the total (§6).
fn span(
    extent: &ExtrudeExtent,
    depths: &Depths,
    bound: &mut Bound,
) -> Result<Span<Length>, &'static str> {
    let undrafted = |side: &ExtrudeSide| {
        side.draft
            .is_none()
            .then_some(())
            .ok_or("the extrude is drafted")
    };
    Ok(match extent {
        ExtrudeExtent::OneSided { side } => {
            undrafted(side)?;
            Span::Between {
                start: Box::new(Limit::AtOrigin),
                end: Box::new(limit(&side.termination, depths.first_or_sole(), bound)?),
            }
        }
        ExtrudeExtent::TwoSided { first, second } => {
            undrafted(first)?;
            undrafted(second)?;
            Span::Between {
                start: Box::new(limit(&second.termination, depths.second.as_ref(), bound)?),
                end: Box::new(limit(&first.termination, depths.first.as_ref(), bound)?),
            }
        }
        ExtrudeExtent::Symmetric { side } => {
            undrafted(side)?;
            match limit(&side.termination, depths.first_or_sole(), bound)? {
                Limit::Amount(amount) => Span::Symmetric {
                    amount,
                    measure: Some(SymmetricMeasure::Total),
                },
                Limit::ThroughAll => Span::Between {
                    start: Box::new(Limit::ThroughAll),
                    end: Box::new(Limit::ThroughAll),
                },
                _ => return Err("the symmetric extent has no symmetric limit"),
            }
        }
    })
}

/// The limit of `termination`, a blind distance bound to `depth` where the
/// feature lists that dimension.
fn limit(
    termination: &LinearTermination,
    depth: Option<&ParameterId>,
    bound: &mut Bound,
) -> Result<Limit<Length>, &'static str> {
    Ok(match termination {
        LinearTermination::Blind { length } => match depth {
            Some(parameter) => {
                bound.push((parameter.clone(), Quantity::LENGTH));
                Limit::Amount(Dimension::Parameter(parameter.clone()))
            }
            None => Limit::Amount(Dimension::Literal(
                Length::new(length.get()).ok_or("the extrude's depth is not finite")?,
            )),
        },
        LinearTermination::ThroughAll {} => Limit::ThroughAll,
        _ => return Err("the extrude's limit is neither blind nor through all"),
    })
}

/// The dimensions that hold an extrude's two depths: `Depth` and `Depth2`,
/// else `D1` and `D2`, the names the decoder reads the distances from. A
/// one-sided or mid-plane extrude without `Depth` or `D1` takes the
/// feature's one dimension for its depth; a two-sided extrude's first depth
/// without them is the literal the decoder read.
struct Depths {
    first: Option<ParameterId>,
    second: Option<ParameterId>,
    sole: Option<ParameterId>,
}

impl Depths {
    fn new(feature: &Feature, context: &Context<'_>) -> Self {
        let record = feature.native_ref.as_deref().and_then(|native| {
            context
                .histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|record| record.id == native)
        });
        let owned = |names: [&str; 2]| {
            names.iter().find_map(|name| {
                record
                    .is_some_and(|record| record.parameters.contains_key(*name))
                    .then(|| context.entries.owned(&feature.id, name))
                    .flatten()
                    .cloned()
            })
        };
        let sole = || {
            let record = record?;
            let mut names = record.parameters.keys();
            let name = names.next().filter(|_| names.next().is_none())?;
            context.entries.owned(&feature.id, name.as_str()).cloned()
        };
        Self {
            first: owned(["Depth", "D1"]),
            second: owned(["Depth2", "D2"]),
            sole: sole(),
        }
    }

    /// The dimension of a single depth: the first, else the sole one.
    fn first_or_sole(&self) -> Option<&ParameterId> {
        self.first.as_ref().or(self.sole.as_ref())
    }
}
