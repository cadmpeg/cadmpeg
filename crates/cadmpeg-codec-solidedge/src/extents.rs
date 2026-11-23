// SPDX-License-Identifier: Apache-2.0
//! Extrude and revolve extents (`docs/formats/solidedge.md` §6): the extent
//! kind, side, depth, axis and angle of a protrusion, a cutout and a revolved
//! protrusion, stated on the feature's design operation.
//!
//! A feature whose slots do not resolve, or hold a value the specification
//! does not state, stays unresolved and adds `feature.extent-unread`. Stored
//! lengths are metres; the design states millimetres.

use std::collections::BTreeMap;

use cadmpeg_core::decode::View;
use cadmpeg_ir::design::construction::{kinds, AxisValue, Construction, FrameValue};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::sketch::{EntityDefinition, PlanarCurve};
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin, Revolve};
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp, SolidOutput, SymmetricMeasure};
use cadmpeg_ir::design::selection::{
    Anchor, Members, OrderedSelection, PickTarget, Picked, Picks, Selection,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::SketchId;
use cadmpeg_ir::units::UnitVector3;

use crate::construction;
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::layout::{dimension, double_value, ref_axis};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};

/// The class of a feature stored as a part function.
const PART_CLASS: &str = "CLSID_JMPart";
/// The class of a stored number.
const DOUBLE_CLASS: &str = "Double Value Object";
/// The class of a stored dimension.
const DIMENSION_CLASS: &str = "CLSID_JDim";
/// The class of a stored revolve axis.
const AXIS_CLASS: &str = "CLSID_JMRefAxis";

/// Bytes from the end of a function name to each parameter slot (§6.1).
const KIND_AFTER_NAME: usize = 100;
const SIDE_AFTER_NAME: usize = 128;
const DEPTH_AFTER_NAME: usize = 184;
const AXIS_AFTER_NAME: usize = 184;
const ANGLE_AFTER_NAME: usize = 212;

/// The extent-kind values (§6.2).
const KIND_FINITE: f64 = 13.0;
const KIND_THROUGH_ALL: f64 = 16.0;
/// The side values (§6.2).
const SIDE_AGAINST: f64 = 1.0;
const SIDE_ALONG: f64 = 2.0;
const SIDE_BOTH: f64 = 3.0;

/// A stored enumeration value equals its code within this distance.
const EPS_CODE: f64 = 1e-9;
/// A stored axis direction has unit length within this distance.
const EPS_AXIS_UNIT: f64 = 1e-6;
/// A profile point lies on the axis when its sense value is within this
/// bound, in square millimetres.
const EPS_ON_AXIS: f64 = 1e-9;
/// A sweep angle exceeds a full turn by no more than this, in radians.
const EPS_FULL_TURN: f64 = 1e-9;
/// Millimetres per metre.
const MM_PER_M: f64 = 1000.0;
/// Bytes of one `f64`.
const REAL: usize = 8;

/// What a feature function extends.
#[derive(Clone, Copy)]
enum Shape {
    /// An extruded protrusion (`cut` false) or cutout (`cut` true).
    Extrude { cut: bool },
    /// A revolved protrusion.
    Revolve,
}

fn shape(function: &str) -> Option<Shape> {
    Some(match function {
        "JMProtrusionFunction" => Shape::Extrude { cut: false },
        "JMCutoutFunction" => Shape::Extrude { cut: true },
        "JMRevolvedProtrusionFunction" => Shape::Revolve,
        _ => return None,
    })
}

/// The sketch a feature owns: its identifier, frame and model-space sample
/// points of its curves in millimetres.
struct Profile {
    id: SketchId,
    frame: FrameValue,
    points: Vec<Vector3>,
}

fn is(value: f64, code: f64) -> bool {
    (value - code).abs() <= EPS_CODE
}

/// The `u32` identifier stored `after` bytes past the function name ending at
/// `end`.
fn slot(object: &Object<'_>, end: usize, after: usize) -> Result<u32, String> {
    end.checked_add(after)
        .and_then(|at| View::u32_le_at(object.bytes, at))
        .ok_or_else(|| format!("the feature block has no slot {after} bytes after its name"))
}

/// The object `id` names and its class.
fn named<'a>(
    cluster: &Cluster<'a>,
    types: &'a TypeTable,
    id: u32,
    what: &str,
) -> Result<(Object<'a>, &'a str), String> {
    if id == 0 {
        return Err(format!("the {what} slot names no object"));
    }
    let object = cluster
        .object(id)
        .ok_or_else(|| format!("the {what} object {id:#x} is not in the cluster"))?;
    let class = types
        .class_name(object.form, object.flags)
        .ok_or_else(|| format!("the {what} object {id:#x} has no class"))?;
    Ok((object, class))
}

fn real_at(object: &Object<'_>, at: usize, what: &str) -> Result<f64, String> {
    View::f64_le_at(object.bytes, at)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("the {what} object does not hold a finite value"))
}

/// The value of a `Double Value Object`.
fn double_slot(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    after: usize,
    what: &str,
) -> Result<f64, String> {
    let id = slot(object, end, after)?;
    let (found, class) = named(cluster, types, id, what)?;
    if class != DOUBLE_CLASS {
        return Err(format!("the {what} object is of class {class}"));
    }
    real_at(&found, double_value::VALUE, what)
}

/// The value of a `CLSID_JDim` or a `Double Value Object`.
fn measure_slot(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    after: usize,
    what: &str,
) -> Result<f64, String> {
    let id = slot(object, end, after)?;
    let (found, class) = named(cluster, types, id, what)?;
    match class {
        DOUBLE_CLASS => real_at(&found, double_value::VALUE, what),
        DIMENSION_CLASS => real_at(&found, dimension::VALUE, what),
        other => Err(format!("the {what} object is of class {other}")),
    }
}

/// A sweep boolean: a protrusion joins, a cutout cuts.
fn output(cut: bool) -> SolidOutput {
    SolidOutput {
        body: Some(BodyForm::Solid),
        boolean: Some(Boolean::Apply {
            op: if cut { BooleanOp::Cut } else { BooleanOp::Join },
            targets: None,
        }),
    }
}

fn profile_selection(sketch: &SketchId) -> Result<OrderedSelection, String> {
    let pick = Picked {
        target: PickTarget::Sketch(sketch.clone()),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![pick]).map_err(|_| "the profile selection is invalid".to_owned())?,
        ),
        native: None,
    })
}

fn length(metres: f64) -> Result<Dimension<Length>, String> {
    Length::new(metres * MM_PER_M)
        .map(Dimension::Literal)
        .ok_or_else(|| "a stored length is not finite".to_owned())
}

fn extrude(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    cut: bool,
    profile: &Profile,
) -> Result<FeatureOperation, String> {
    let kind = double_slot(cluster, types, object, end, KIND_AFTER_NAME, "extent kind")?;
    let side = double_slot(cluster, types, object, end, SIDE_AFTER_NAME, "side")?;
    let both = is(side, SIDE_BOTH);
    let against = is(side, SIDE_AGAINST);
    if !both && !against && !is(side, SIDE_ALONG) {
        return Err(format!("the side value {side} is not 1, 2 or 3"));
    }
    let span = if is(kind, KIND_FINITE) {
        let depth = measure_slot(cluster, types, object, end, DEPTH_AFTER_NAME, "depth")?;
        if depth <= 0.0 {
            return Err(format!("the depth {depth} is not positive"));
        }
        let amount = length(depth)?;
        if both {
            // The stored depth is the total extent of both sides.
            Span::Symmetric {
                amount,
                measure: Some(SymmetricMeasure::Total),
            }
        } else {
            Span::Between {
                start: Box::new(Limit::AtOrigin),
                end: Box::new(Limit::Amount(amount)),
            }
        }
    } else if is(kind, KIND_THROUGH_ALL) {
        Span::Between {
            start: Box::new(if both {
                Limit::ThroughAll
            } else {
                Limit::AtOrigin
            }),
            end: Box::new(Limit::ThroughAll),
        }
    } else {
        return Err(format!("the extent kind {kind} is not 13 or 16"));
    };
    Ok(FeatureOperation::Extrude(Box::new(Extrude {
        profile: profile_selection(&profile.id)?,
        loops: None,
        direction: Some(ExtrudeDirection::ProfileNormal {
            reversed: Some(Dimension::Literal(against)),
        }),
        origin: Some(ExtrudeOrigin::ProfilePlane),
        span: Some(span),
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: output(cut),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}

/// Three consecutive `f64` of an axis object, in metres.
fn vector(object: &Object<'_>, at: usize) -> Result<Vector3, String> {
    let at_index = |index: usize| {
        at.checked_add(index.checked_mul(REAL)?)
            .and_then(|at| View::f64_le_at(object.bytes, at))
            .filter(|value| value.is_finite())
    };
    match (at_index(0), at_index(1), at_index(2)) {
        (Some(x), Some(y), Some(z)) => Ok(Vector3::new(x, y, z)),
        _ => Err("the axis object does not hold finite values".to_owned()),
    }
}

fn axis(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
) -> Result<AxisValue, String> {
    let id = slot(object, end, AXIS_AFTER_NAME)?;
    let (found, class) = named(cluster, types, id, "axis")?;
    if class != AXIS_CLASS {
        return Err(format!("the axis object is of class {class}"));
    }
    if found.bytes.get(ref_axis::MARKER).copied() != Some(ref_axis::MARKER_VALUE) {
        return Err("the axis object does not carry the axis marker".to_owned());
    }
    let point = vector(&found, ref_axis::POINT)?;
    let direction = vector(&found, ref_axis::DIRECTION)?;
    if (direction.norm() - 1.0).abs() > EPS_AXIS_UNIT {
        return Err("the axis direction is not a unit vector".to_owned());
    }
    Ok(AxisValue {
        origin: FinitePoint3::new(Point3::new(
            point.x * MM_PER_M,
            point.y * MM_PER_M,
            point.z * MM_PER_M,
        ))
        .ok_or_else(|| "the axis point is not finite".to_owned())?,
        direction: UnitVector3::normalized(direction)
            .ok_or_else(|| "the axis direction is not a unit vector".to_owned())?,
    })
}

/// The sign of the rotation about `axis` that moves the profile towards its
/// normal `A`. A profile point `p` off the axis moves in direction
/// `D x (p - c)` under a positive rotation about `D` through `c`, so the
/// sign is that of `A . (D x (p - c))`. `Err` where the profile has no point
/// off the axis or points on both sides of it.
fn towards_normal(axis: &AxisValue, profile: &Profile) -> Result<f64, String> {
    let normal = Vector3::from(profile.frame.z_axis);
    let direction = Vector3::from(axis.direction);
    let centre = axis.origin.get();
    let (mut positive, mut negative) = (false, false);
    for point in &profile.points {
        let offset = Vector3::new(point.x - centre.x, point.y - centre.y, point.z - centre.z);
        let sense = normal.dot(direction.cross(offset));
        if sense > EPS_ON_AXIS {
            positive = true;
        } else if sense < -EPS_ON_AXIS {
            negative = true;
        }
    }
    match (positive, negative) {
        (true, false) => Ok(1.0),
        (false, true) => Ok(-1.0),
        (true, true) => Err("the profile lies on both sides of the axis".to_owned()),
        (false, false) => Err("the profile has no point off the axis".to_owned()),
    }
}

fn revolve(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    profile: &Profile,
) -> Result<FeatureOperation, String> {
    let kind = double_slot(cluster, types, object, end, KIND_AFTER_NAME, "extent kind")?;
    if !is(kind, KIND_FINITE) {
        return Err(format!("the extent kind {kind} of a revolve is not 13"));
    }
    let side = double_slot(cluster, types, object, end, SIDE_AFTER_NAME, "side")?;
    let axis = axis(cluster, types, object, end)?;
    let angle = measure_slot(cluster, types, object, end, ANGLE_AFTER_NAME, "angle")?;
    if angle <= 0.0 || angle > std::f64::consts::TAU + EPS_FULL_TURN {
        return Err(format!("the sweep angle {angle} is not in (0, 2*pi]"));
    }
    let span = if is(side, SIDE_BOTH) {
        // The stored angle is the total sweep of both sides.
        Span::Symmetric {
            amount: Dimension::Literal(
                Angle::new(angle).ok_or_else(|| "the sweep angle is not finite".to_owned())?,
            ),
            measure: Some(SymmetricMeasure::Total),
        }
    } else {
        let sign = if is(side, SIDE_ALONG) {
            towards_normal(&axis, profile)?
        } else if is(side, SIDE_AGAINST) {
            -towards_normal(&axis, profile)?
        } else {
            return Err(format!("the side value {side} is not 1, 2 or 3"));
        };
        Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(Limit::Amount(Dimension::Literal(
                Angle::new(sign * angle)
                    .ok_or_else(|| "the sweep angle is not finite".to_owned())?,
            ))),
        }
    };
    Ok(FeatureOperation::Revolve(Box::new(Revolve {
        profile: profile_selection(&profile.id)?,
        loops: None,
        axis: Some(Construction::<kinds::Axis> {
            resolved: Some(axis),
            ..construction::empty()
        }),
        span: Some(span),
        fit: None,
        output: output(false),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}

/// The model-space point of profile coordinates (`u`, `v`), in millimetres.
fn model_point(frame: &FrameValue, u: f64, v: f64) -> Vector3 {
    let origin = frame.origin.get();
    let x = Vector3::from(frame.x_axis);
    let y = Vector3::from(frame.y_axis);
    Vector3::new(
        origin.x + u * x.x + v * y.x,
        origin.y + u * x.y + v * y.y,
        origin.z + u * x.z + v * y.z,
    )
}

/// The sketch `owner` owns, with sample points of its curves.
fn profile_of(ir: &CadIr, owner: &cadmpeg_ir::features::FeatureId) -> Option<Profile> {
    let sketch = ir.design.sketches.iter().find(|s| &s.owner == owner)?;
    let cadmpeg_ir::design::ops::sketch::SketchPlane::Placed(plane) = &sketch.plane else {
        return None;
    };
    let frame = plane.resolved.clone()?;
    let mut points = Vec::new();
    for entity in ir
        .design
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
    {
        let EntityDefinition::Free(curve) = &entity.definition else {
            continue;
        };
        let mut at = |u: f64, v: f64| points.push(model_point(&frame, u, v));
        match curve {
            PlanarCurve::Line { start, end } => {
                at(start.get().u, start.get().v);
                at(end.get().u, end.get().v);
            }
            PlanarCurve::Circle { center, radius } => {
                let (c, r) = (center.get(), radius.get());
                at(c.u + r, c.v);
                at(c.u - r, c.v);
                at(c.u, c.v + r);
                at(c.u, c.v - r);
            }
            PlanarCurve::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let (c, r) = (center.get(), radius.get());
                for angle in [start.get(), end.get()] {
                    at(c.u + r * angle.cos(), c.v + r * angle.sin());
                }
            }
            _ => {}
        }
    }
    Some(Profile {
        id: sketch.id.clone(),
        frame,
        points,
    })
}

/// State the extent of each extrude and revolve feature whose profile is a
/// stated sketch. A feature that does not read keeps its unresolved operation
/// and adds `feature.extent-unread`.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for entry in listed {
        let Kind::Function(Some(function)) = &entry.kind else {
            continue;
        };
        let Some(shape) = shape(function) else {
            continue;
        };
        let Some(object) = cluster.object(entry.object) else {
            continue;
        };
        if types.class_name(object.form, object.flags) != Some(PART_CLASS) {
            continue;
        }
        let Some((_, end)) = function_span(&object) else {
            continue;
        };
        let id = feature_id(entry.object);
        let Some(profile) = profile_of(ir, &id) else {
            continue;
        };
        let read = match shape {
            Shape::Extrude { cut } => extrude(cluster, types, &object, end, cut, &profile),
            Shape::Revolve => revolve(cluster, types, &object, end, &profile),
        };
        match read {
            Ok(operation) => {
                if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                    feature.definition.operation = operation;
                }
            }
            Err(reason) => losses.push(SolidEdgeLossCode::FeatureExtentUnread.note(format!(
                "the extent of feature object {:#x} ({function}) does not read: {reason}; the \
                 feature stays unresolved",
                entry.object
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
