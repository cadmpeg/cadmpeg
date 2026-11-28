// SPDX-License-Identifier: Apache-2.0
//! Extrudes (§6.3), from the decoder's typed extrusions.
//!
//! An extrude whose profile is a whole sketch the design states, named by the
//! sketch or by its feature, and whose limits are blind distances or
//! through-all, is typed. Its profile uses the stated sketch and the decoded
//! boundaries. A blind limit binds
//! the dimension that holds its distance, and a merge into a part that holds
//! no body yet is a new body (§6). An extrude whose start, limits, draft, or
//! profile the decoder reads only in part stays `Unresolved`, and so does one
//! whose sketch the timeline lists after it, which its `Input` profile pick
//! cannot see. An extrude whose sketch stays `Unresolved` still binds its
//! blind depths, each a length.

use std::collections::HashMap;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin};
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, BooleanOp, NativeOptions, SolidOutput, SymmetricMeasure,
};
use cadmpeg_ir::design::units::Quantity;
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::extents::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::features::operation_settings::BooleanOp as DecodedOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, ParameterId};
use cadmpeg_ir::scalar::Length;

use super::profile_check::{Difference, Unchecked};
use super::swept::{self, Before, Context};

/// The extrudes typed, and the quantity of each parameter a limit binds.
#[derive(Default)]
pub(super) struct Extrudes {
    pub(super) bound: HashMap<ParameterId, Quantity>,
    /// The extrude features left `Unresolved`, with the reason.
    pub(super) unprojected: Vec<(FeatureId, &'static str)>,
    /// The typed extrudes whose blind depth is the record's only dimension,
    /// named neither `Depth` nor `D1`, with that dimension.
    pub(super) assumed: Vec<(FeatureId, ParameterId)>,
    /// The typed extrudes whose sketch's closed regions differ from the
    /// profile loops the decoder resolved, with their name and how they
    /// differ.
    pub(super) differs: Vec<(FeatureId, Option<String>, Vec<Difference>)>,
    /// The typed extrudes whose sketch's closed regions the check did not
    /// compare with the stored profile, with their name and why.
    pub(super) unchecked: Vec<(FeatureId, Option<String>, Unchecked)>,
}

/// What a typed extrude adds to the notes.
#[derive(Default)]
struct Notes {
    /// The dimension its blind depth binds as its record's only one.
    assumed: Option<ParameterId>,
    /// How its sketch's closed regions differ from the stored profile, or
    /// why the check did not compare them.
    profile: Option<Result<Vec<Difference>, Unchecked>>,
}

/// Type each `Unresolved` extrude of `features`, in member order.
pub(super) fn project(features: &mut [Feature], context: &Context<'_>) -> Extrudes {
    let mut out = Extrudes::default();
    swept::walk(features, OperationFamily::Extrude, |feature, before| {
        let mut bound = Vec::new();
        let mut notes = Notes::default();
        let typed = extrude(feature, context, before, &mut bound, &mut notes);
        out.bound.extend(bound);
        match typed {
            Ok(extrude) => {
                feature.definition.operation = FeatureOperation::Extrude(Box::new(extrude));
                out.assumed.extend(
                    notes
                        .assumed
                        .map(|parameter| (feature.id.clone(), parameter)),
                );
                let named = (feature.id.clone(), feature.name.clone());
                match notes.profile {
                    Some(Ok(differs)) if !differs.is_empty() => {
                        out.differs.push((named.0, named.1, differs));
                    }
                    Some(Err(why)) => out.unchecked.push((named.0, named.1, why)),
                    _ => {}
                }
            }
            Err(reason) => out.unprojected.push((feature.id.clone(), reason)),
        }
    });
    out
}

type Bound = Vec<(ParameterId, Quantity)>;

fn extrude(
    feature: &Feature,
    context: &Context<'_>,
    before: &Before<'_>,
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
        ProfileRef::Planar(PlanarProfileRef::Feature(owner)) => before
            .sketch_of
            .get(owner)
            .ok_or("the extrude's profile feature states no sketch")?
            .as_ref(),
        _ => return Err("the extrude's profile is no whole sketch"),
    };
    let direction = match direction {
        cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {} => {
            ExtrudeDirection::ProfileNormal { reversed: None }
        }
        cadmpeg_ir::features::extents::ExtrudeDirection::ReversedProfileNormal {} => {
            ExtrudeDirection::ProfileNormal {
                reversed: Some(Dimension::Literal(true)),
            }
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
    if !before.preceding.contains(sketch) {
        return Err("the extrude's sketch follows it in the timeline");
    }
    let boolean = boolean(*op, before.body);
    let profile = swept::profile(sketch, context, *solid == Some(true))?;
    notes.profile = Some(profile.differences);
    Ok(Extrude {
        profile: profile.picks,
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

/// The boolean of a decoded operation: a join into a part that holds no
/// body yet is a new body (§6).
fn boolean(op: DecodedOp, body_before: bool) -> Option<Boolean> {
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
