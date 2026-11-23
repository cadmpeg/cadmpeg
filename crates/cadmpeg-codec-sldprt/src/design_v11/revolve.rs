// SPDX-License-Identifier: Apache-2.0
//! Revolves (§6.3), from the decoder's typed revolutions.
//!
//! A revolution whose profile is a whole sketch the design states, whose
//! axis lies in that sketch's plane, and whose angle is one full turn on one
//! side or a mid-plane angle is typed. The profile is a `Sketch` pick checked
//! against the stored profile loops as an extrude's is, and the angle binds
//! the dimension the decoder read it from.
//!
//! The axis states the decoder's axis as its value and picks the line of the
//! profile sketch that lies on it, a construction line before a profile line,
//! the input `reversed` where that line runs against the axis direction
//! (§5.9 Sense). Where the revolution's record stores the axis, the pick is
//! inferred. Where no record stores it, the decoder derived it from the
//! sketch's records or the solid's surfaces: the revolve is typed only where
//! a line of its sketch lies on that axis, with a warning naming the line.
//!
//! A cut cuts, and a join joins, into a part that holds no body yet as a new
//! body (§6). A revolution of the adding class whose operation the decoder
//! did not read is a new body where nothing precedes it that can hold a body,
//! and states no boolean after one.
//!
//! A partial or two-sided turn, whose sense the decoder does not read, stays
//! `Unresolved`, and so does a revolution whose axis lies outside its
//! sketch's plane (open item DI-85) or whose sketch holds no profile curve.

use std::collections::HashMap;

use cadmpeg_ir::design::construction::{
    kinds, AxisValue, Construction, ConstructionInput, InputReference, InputRole, Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::{EntityDefinition, Planar, PlanarCurve, SketchEntity};
use cadmpeg_ir::design::ops::swept::Revolve;
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, BooleanOp, NativeOptions, SolidOutput, SymmetricMeasure,
};
use cadmpeg_ir::design::selection::PickTarget;
use cadmpeg_ir::design::units::Quantity;
use cadmpeg_ir::features as v10;
use cadmpeg_ir::features::extents::AngularTermination;
use cadmpeg_ir::features::operation_settings::BooleanOp as DecodedOp;
use cadmpeg_ir::features::parameters::ParameterValue;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::features::revolve::{
    PartialRevolveConstruction, RevolutionAxis, RevolveConstruction, RevolveExtent,
};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, ParameterId};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::sketches::{SketchEntityId, SketchId};
use cadmpeg_ir::units::UnitVector3;

use super::profile_check::{Difference, Unchecked};
use super::sketch::{pick, selection};
use super::swept::{self, Before, Context};
use crate::brep::graph::LINEAR_RESOLUTION_MM;
use crate::resolved_features::axes::axis_in_plane;

/// Two angles within this many radians are one angle.
const EPS_REVOLVE_ANGLE_RAD: f64 = 1e-9;

/// The revolves typed, and the quantity of each parameter an angle binds.
#[derive(Default)]
pub(super) struct Revolves {
    pub(super) bound: HashMap<ParameterId, Quantity>,
    /// The revolve features left `Unresolved`, with the reason.
    pub(super) unprojected: Vec<(FeatureId, &'static str)>,
    /// The typed revolves whose stored axis picks a line of their profile
    /// sketch.
    pub(super) inferred: Vec<AxisPick>,
    /// The typed revolves whose axis no record stores, which the decoder
    /// derived and which lies on a line of their profile sketch.
    pub(super) derived: Vec<AxisPick>,
    /// The typed revolves whose boolean the design leaves unstated, with
    /// their name and why.
    pub(super) boolean_unstated: Vec<(FeatureId, Option<String>, &'static str)>,
    /// The typed revolves whose sketch's closed regions differ from the
    /// profile loops the decoder resolved, with their name and how they
    /// differ.
    pub(super) differs: Vec<(FeatureId, Option<String>, Vec<Difference>)>,
    /// The typed revolves whose sketch's closed regions the check did not
    /// compare with the stored profile, with their name and why.
    pub(super) unchecked: Vec<(FeatureId, Option<String>, Unchecked)>,
}

/// The line a revolve's axis picks, and how many of its sketch's lines lie
/// on the axis.
type AxisLine = (SketchEntityId, usize);

/// A typed revolve, its name, the line its axis picks, and how many of its
/// sketch's lines lie on the axis.
pub(super) type AxisPick = (FeatureId, Option<String>, SketchEntityId, usize);

/// What a typed revolve adds to the notes.
#[derive(Default)]
struct Notes {
    line: Option<AxisLine>,
    /// Whether a record stores the axis.
    stored: bool,
    /// Why the design states no boolean.
    boolean_unstated: Option<&'static str>,
    /// How its sketch's closed regions differ from the stored profile, or
    /// why the check did not compare them.
    profile: Option<Result<Vec<Difference>, Unchecked>>,
}

/// Type each `Unresolved` revolve of `features`, in member order.
pub(super) fn project(features: &mut [Feature], context: &Context<'_>) -> Revolves {
    let mut out = Revolves::default();
    swept::walk(features, OperationFamily::Revolve, |feature, before| {
        let mut notes = Notes::default();
        match revolve(feature, context, before, &mut out.bound, &mut notes) {
            Ok(revolve) => {
                feature.definition.operation = FeatureOperation::Revolve(Box::new(revolve));
                let (id, name) = (feature.id.clone(), feature.name.clone());
                if let Some(why) = notes.boolean_unstated {
                    out.boolean_unstated.push((id.clone(), name.clone(), why));
                }
                if let Some((line, among)) = notes.line {
                    let picks = if notes.stored {
                        &mut out.inferred
                    } else {
                        &mut out.derived
                    };
                    picks.push((id.clone(), name.clone(), line, among));
                }
                match notes.profile {
                    Some(Ok(differs)) if !differs.is_empty() => {
                        out.differs.push((id, name, differs));
                    }
                    Some(Err(why)) => out.unchecked.push((id, name, why)),
                    _ => {}
                }
            }
            Err(reason) => out.unprojected.push((feature.id.clone(), reason)),
        }
    });
    out
}

fn revolve(
    feature: &Feature,
    context: &Context<'_>,
    before: &Before<'_>,
    bound: &mut HashMap<ParameterId, Quantity>,
    notes: &mut Notes,
) -> Result<Revolve, &'static str> {
    let source = context
        .model
        .features
        .iter()
        .find(|candidate| candidate.id == feature.id)
        .ok_or("the revolve has no decoded record")?;
    let FeatureDefinition::Operation(v10::FeatureOperation::Revolve { construction, op }) =
        source.evaluation.definition()
    else {
        return Err("the revolve's decoded record is no revolution");
    };
    let (profile, axis, extent, solid) = match construction {
        RevolveConstruction::Resolved {
            profile,
            axis,
            extent,
            solid,
            face_maker,
            fuse_order,
            allow_multi_profile_faces,
        } => {
            if face_maker.is_some() || fuse_order.is_some() || allow_multi_profile_faces.is_some() {
                return Err("the revolve states an option of another format");
            }
            (profile, axis, extent, *solid)
        }
        RevolveConstruction::Unresolved(PartialRevolveConstruction::Profile { .. }) => {
            return Err("the revolve's profile is not read");
        }
        RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis { .. }) => {
            return Err("the revolve's axis is not read");
        }
        RevolveConstruction::Unresolved(PartialRevolveConstruction::Extent { .. }) => {
            return Err("the revolve's angle is not read");
        }
    };
    let sketch = match profile {
        PlanarProfileRef::Sketch(sketch) => Some(sketch),
        PlanarProfileRef::Feature(owner) => before
            .sketch_of
            .get(owner)
            .ok_or("the revolve's profile feature states no sketch")?
            .as_ref(),
        _ => return Err("the revolve's profile is no whole sketch"),
    };
    let span = span(
        extent,
        |angle| angle_dimension(&feature.id, context, angle),
        bound,
    )?;
    let sketch = sketch
        .filter(|sketch| context.sketches.contains(*sketch))
        .ok_or("the revolve's sketch is not stated")?;
    if !before.preceding.contains(sketch) {
        return Err("the revolve's sketch follows it in the timeline");
    }
    let profile = swept::differences(sketch, context);
    if matches!(profile, Err(Unchecked::NoProfileCurve)) {
        return Err("the revolve's sketch holds no profile curve");
    }
    let record = feature.native_ref.as_deref().and_then(|native| {
        context
            .histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|record| record.id == native)
    });
    let stored = record.is_some_and(|record| {
        record.properties.contains_key("AxisOrigin")
            && record.properties.contains_key("AxisDirection")
    });
    let (axis, line) = axis_construction(axis, sketch, context)?;
    if !stored && line.is_none() {
        return Err(
            "no record stores the revolve's axis, and no line of its sketch lies on the axis \
             the decoder derived",
        );
    }
    let adds = record.and_then(|record| record.input_class.as_deref()) == Some("moRevolution_c");
    let boolean = boolean(*op, adds, before.body);
    notes.line = line;
    notes.stored = stored;
    notes.boolean_unstated = boolean.is_none().then_some(if adds {
        "it adds material after an earlier member, and the decoder reads no stored field that \
         tells a new body from a join"
    } else {
        "the decoder reads no Boolean operation"
    });
    notes.profile = Some(profile);
    Ok(Revolve {
        profile: selection(vec![pick(PickTarget::Sketch(sketch.clone()))])?,
        loops: None,
        axis: Some(axis),
        span: Some(span),
        fit: None,
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

/// The boolean of a revolution whose decoded operation is `op`: a cut cuts
/// and a join joins, into a part that holds no body yet as a new body (§6).
/// A revolution of a class that adds material (`adds`) whose operation the
/// decoder did not read is a new body where no member precedes it that can
/// hold a body; after one, no read field decides, and the boolean is `None`.
fn boolean(op: DecodedOp, adds: bool, body_before: bool) -> Option<Boolean> {
    let apply = |op| Some(Boolean::Apply { op, targets: None });
    match op {
        DecodedOp::Cut => apply(BooleanOp::Cut),
        DecodedOp::Intersect => apply(BooleanOp::Intersect),
        DecodedOp::Join if body_before => apply(BooleanOp::Join),
        DecodedOp::Join | DecodedOp::NewBody => Some(Boolean::NewBody),
        DecodedOp::Unresolved if adds && !body_before => Some(Boolean::NewBody),
        DecodedOp::Unresolved => None,
    }
}

/// The span of `extent`, each angle bound to the dimension `dimension`
/// gives for it. A full turn on one side and a mid-plane angle are the same
/// solid in either sense of the axis; the others are not.
fn span(
    extent: &RevolveExtent,
    dimension: impl Fn(f64) -> Option<ParameterId>,
    bound: &mut HashMap<ParameterId, Quantity>,
) -> Result<Span<Angle>, &'static str> {
    let mut amount = |termination: &AngularTermination| {
        let AngularTermination::Angle { angle } = termination else {
            return Err("the revolve's limit is no angle");
        };
        let angle = angle.get();
        let amount = match dimension(angle) {
            Some(parameter) => {
                bound.insert(parameter.clone(), Quantity::ANGLE);
                Dimension::Parameter(parameter)
            }
            None => {
                Dimension::Literal(Angle::new(angle).ok_or("the revolve's angle is not finite")?)
            }
        };
        Ok((angle, amount))
    };
    match extent {
        RevolveExtent::OneSided { termination } => {
            let (angle, amount) = amount(termination)?;
            if (angle - std::f64::consts::TAU).abs() > EPS_REVOLVE_ANGLE_RAD {
                return Err("the revolve turns less than a full turn on one side, and its sense is not read");
            }
            Ok(Span::Between {
                start: Box::new(Limit::AtOrigin),
                end: Box::new(Limit::Amount(amount)),
            })
        }
        RevolveExtent::Symmetric { termination } => Ok(Span::Symmetric {
            amount: amount(termination)?.1,
            measure: Some(SymmetricMeasure::Total),
        }),
        RevolveExtent::TwoSided { .. } => {
            Err("the revolve is two-sided, and its sense is not read")
        }
    }
}

/// The dimension the decoder read a revolve's first angle from (sldprt.md
/// §2): `Angle`, else `D1`, else the feature's first dimension whose value is
/// a positive angle; `None` where its value is not `angle`.
fn angle_dimension(feature: &FeatureId, context: &Context<'_>, angle: f64) -> Option<ParameterId> {
    let value = |parameter: &v10::parameters::DesignParameter| match &parameter.value {
        Some(ParameterValue::Angle(value)) => Some(value.get()),
        _ => None,
    };
    let named = ["Angle", "D1"]
        .iter()
        .find_map(|name| context.entries.owned(feature, name));
    let first = || {
        context
            .model
            .parameters
            .iter()
            .filter(|parameter| {
                parameter.owner.as_ref() == Some(feature)
                    && value(parameter).is_some_and(|value| value > 0.0)
                    && context.entries.contains(&parameter.id)
            })
            .min_by_key(|parameter| parameter.ordinal)
            .map(|parameter| &parameter.id)
    };
    let parameter = named.or_else(first)?;
    context
        .model
        .parameters
        .iter()
        .find(|candidate| &candidate.id == parameter)
        .and_then(value)
        .is_some_and(|value| (value - angle).abs() <= EPS_REVOLVE_ANGLE_RAD)
        .then(|| parameter.clone())
}

/// The axis of a revolve about `axis` whose profile is `sketch`: the
/// decoder's axis as its value, and a `Coincident` pick of the sketch's line
/// that lies on it, with that line and the number of lines on the axis. An
/// axis outside the sketch's plane fails.
fn axis_construction(
    axis: &RevolutionAxis,
    sketch: &SketchId,
    context: &Context<'_>,
) -> Result<(Construction<kinds::Axis>, Option<AxisLine>), &'static str> {
    let direction = UnitVector3::normalized_nonzero(axis.direction.into())
        .ok_or("the revolve's axis is degenerate")?;
    let mut construction = Construction {
        resolved: Some(AxisValue {
            origin: axis.origin,
            direction,
        }),
        ..Construction::default()
    };
    let Some((origin, normal, u_axis)) = context
        .model
        .sketches
        .iter()
        .find(|record| &record.id == sketch)
        .and_then(cadmpeg_ir::sketches::Sketch::resolved_placement)
    else {
        return Ok((construction, None));
    };
    match axis_in_plane(axis, origin, normal) {
        None => return Ok((construction, None)),
        Some(false) => {
            return Err("the revolve's axis lies outside its profile sketch's plane (DI-85)")
        }
        Some(true) => {}
    }
    let frame = Frame {
        origin: Vector3::new(origin.x, origin.y, origin.z),
        u: u_axis.get().unit(),
        normal: normal.get().unit(),
    };
    let on_axis = Line {
        origin: Vector3::new(axis.origin.x, axis.origin.y, axis.origin.z),
        direction: *direction.as_raw(),
    };
    let mut lines = context
        .entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .filter_map(|entity| Some((entity, frame.along(entity, &on_axis)?)))
        .collect::<Vec<_>>();
    lines.sort_by_key(|(entity, _)| entity.construction != Some(true));
    let Some((line, along)) = lines.first() else {
        return Ok((construction, None));
    };
    construction.inputs.push(ConstructionInput {
        role: Some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Coincident,
        }),
        reference: Some(InputReference::Picks(selection(vec![pick(
            PickTarget::SketchEntity(line.id.clone()),
        )])?)),
        value: None,
        reversed: (!along).then_some(true),
    });
    Ok((construction, Some((line.id.clone(), lines.len()))))
}

/// A sketch's plane in model space.
struct Frame {
    origin: Vector3,
    u: Option<Vector3>,
    normal: Option<Vector3>,
}

/// An infinite line: a point and a unit direction.
struct Line {
    origin: Vector3,
    direction: Vector3,
}

impl Line {
    fn distance(&self, point: Vector3) -> f64 {
        let offset = point - self.origin;
        (offset - self.direction.scale(offset.dot(self.direction))).norm()
    }
}

impl Frame {
    /// Whether the sketch line `entity` lies on `line`, both its ends within
    /// the linear resolution of it, and runs along its direction; `None`
    /// where it is no line or does not lie on it.
    fn along(&self, entity: &SketchEntity<Planar>, line: &Line) -> Option<bool> {
        let curve = match &entity.definition {
            EntityDefinition::Free(curve) => curve,
            EntityDefinition::Defined { solved, .. } => solved.as_ref()?,
        };
        let PlanarCurve::Line { start, end } = curve else {
            return None;
        };
        let (u, normal) = (self.u?, self.normal?);
        let v = normal.cross(u);
        let place = |point: cadmpeg_ir::units::FinitePoint2| {
            let point = point.get();
            self.origin + u.scale(point.u) + v.scale(point.v)
        };
        let (start, end) = (place(*start), place(*end));
        let run = end - start;
        (run.norm() > LINEAR_RESOLUTION_MM
            && line.distance(start) <= LINEAR_RESOLUTION_MM
            && line.distance(end) <= LINEAR_RESOLUTION_MM)
            .then(|| run.dot(line.direction) > 0.0)
    }
}
