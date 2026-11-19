// SPDX-License-Identifier: Apache-2.0
//! Extrudes (§6.3), from the decoder's typed extrusions.
//!
//! An extrude whose profile is a whole sketch the design states, and whose
//! limits are blind distances or through-all, is typed: the profile is a
//! `Sketch` pick, a blind limit binds the dimension that holds its distance,
//! and a merge into a part that holds no body yet is a new body (§6). An
//! extrude whose start, limits, draft, or profile the decoder reads only in
//! part stays `Unresolved`.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin};
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, BooleanOp, NativeOptions, SolidOutput, SymmetricMeasure,
};
use cadmpeg_ir::design::selection::PickTarget;
use cadmpeg_ir::design::units::Quantity;
use cadmpeg_ir::features::{
    self as v10, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureId, LinearTermination,
    ParameterId, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::SketchId;

use super::parameters::Entries;
use super::sketch::{pick, selection};

/// The extrudes typed, and the quantity of each parameter a limit binds.
#[derive(Default)]
pub(super) struct Extrudes {
    pub(super) bound: HashMap<ParameterId, Quantity>,
    /// The extrude features left `Unresolved`, with the reason.
    pub(super) unprojected: Vec<(FeatureId, &'static str)>,
}

/// What the projection reads besides the feature.
pub(super) struct Context<'a> {
    pub(super) model: &'a cadmpeg_ir::document::Model,
    pub(super) histories: &'a [crate::records::FeatureHistory],
    pub(super) entries: &'a Entries<'a>,
    /// The sketches the design states.
    pub(super) sketches: &'a HashSet<SketchId>,
}

/// Type each `Unresolved` extrude of `features`, in member order.
pub(super) fn project(features: &mut [Feature], context: &Context<'_>) -> Extrudes {
    let mut out = Extrudes::default();
    let mut body_before = false;
    for feature in features.iter_mut() {
        let family = match &feature.definition.operation {
            FeatureOperation::Unresolved { family } => Some(*family),
            FeatureOperation::Datum(_) | FeatureOperation::Sketch(_) => None,
            _ => {
                body_before |= !suppressed(feature);
                continue;
            }
        };
        match family {
            Some(OperationFamily::Extrude) => match extrude(feature, context, body_before) {
                Ok((extrude, bound)) => {
                    feature.definition.operation = FeatureOperation::Extrude(Box::new(extrude));
                    out.bound.extend(bound);
                }
                Err(reason) => out.unprojected.push((feature.id.clone(), reason)),
            },
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
    body_before: bool,
) -> Result<(Extrude, Bound), &'static str> {
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
    let ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)) = profile else {
        return Err("the extrude's profile is no whole sketch");
    };
    if !context.sketches.contains(sketch) {
        return Err("the extrude's sketch is not stated");
    }
    let direction = match direction {
        v10::ExtrudeDirection::ProfileNormal {} => {
            ExtrudeDirection::ProfileNormal { reversed: None }
        }
        _ => return Err("the extrude's direction is not the profile normal"),
    };
    if !matches!(start, v10::ExtrudeStart::ProfilePlane {}) {
        return Err("the extrude starts off the profile plane");
    }
    let depths = Depths::new(feature, context);
    let mut bound = Vec::new();
    let span = span(extent, &depths, &mut bound)?;
    let boolean = match op {
        v10::BooleanOp::Join if !body_before => Some(Boolean::NewBody),
        v10::BooleanOp::Join => Some(apply(BooleanOp::Join)),
        v10::BooleanOp::Cut => Some(apply(BooleanOp::Cut)),
        v10::BooleanOp::Intersect => Some(apply(BooleanOp::Intersect)),
        v10::BooleanOp::NewBody => Some(Boolean::NewBody),
        v10::BooleanOp::Unresolved => None,
    };
    Ok((
        Extrude {
            profile: selection(vec![pick(PickTarget::Sketch(sketch.clone()))])?,
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
        },
        bound,
    ))
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
                end: Box::new(limit(&side.termination, depths.first.as_ref(), bound)?),
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
            match limit(&side.termination, depths.first.as_ref(), bound)? {
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
/// else `D1` and `D2`, the names the decoder reads the distances from, else
/// the feature's one dimension for its first depth.
struct Depths {
    first: Option<ParameterId>,
    second: Option<ParameterId>,
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
            first: owned(["Depth", "D1"]).or_else(sole),
            second: owned(["Depth2", "D2"]),
        }
    }
}
