// SPDX-License-Identifier: Apache-2.0
//! The chain a selection's sketch curves form, read from the solved curves
//! of one sketch whose solve is current, and the picked sketch curves that
//! are no line segments.

use crate::design::evaluation::EvaluationStatus;
use crate::design::ops::sketch::{
    EntityDefinition, EntityForm, Planar, PlanarCurve, SketchEntity, SpatialCurve,
};
use crate::design::selection::{MemberOrder, PickTarget, Selection};
use crate::features::geometry::FinitePoint3;
use crate::geometry::InlineCurve;
use crate::math::portable::Portable;
use crate::math::Point3;
use crate::units::FinitePoint2;

use super::super::super::index::DesignIndex;
use super::super::picks;
use super::{link, Shape};

/// The ends of a sketch curve.
enum Ends {
    /// A curve with two ends.
    Two(Point3, Point3),
    /// A curve that closes on itself.
    Closed,
}

fn at(point: FinitePoint2) -> Point3 {
    let point = point.get();
    Point3::new(point.u, point.v, 0.0)
}

/// The ends of `curve`. `None` where they are not read.
fn ends(curve: &PlanarCurve) -> Option<Ends> {
    match curve {
        PlanarCurve::Line { start, end } => Some(Ends::Two(at(*start), at(*end))),
        PlanarCurve::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let center = center.get();
            let on = |angle: f64| {
                Point3::new(
                    radius.get().mul_add(angle.portable_cos(), center.u),
                    radius.get().mul_add(angle.portable_sin(), center.v),
                    0.0,
                )
            };
            Some(Ends::Two(on(start.get()), on(end.get())))
        }
        PlanarCurve::Circle { .. } | PlanarCurve::Ellipse { range: None, .. } => Some(Ends::Closed),
        _ => None,
    }
}

/// The curve a sketch entity's solve gives it.
fn solved(entity: &SketchEntity<Planar>) -> Option<&PlanarCurve> {
    match &entity.definition {
        EntityDefinition::Free(curve) => Some(curve),
        EntityDefinition::Defined { form, solved } => match **form {
            EntityForm::Interpolated(_)
            | EntityForm::Conic { .. }
            | EntityForm::Derived(_)
            | EntityForm::Instanced { .. } => solved.as_ref(),
            _ => None,
        },
    }
}

/// The sketch entities `selection` picks, where every pick is an entity of
/// one sketch whose solve is current, with no context.
fn entities<'a, O: MemberOrder>(
    index: &DesignIndex<'a>,
    selection: &'a Selection<O>,
) -> Option<Vec<&'a SketchEntity<Planar>>> {
    let mut sketch = None;
    let mut found = Vec::new();
    for pick in picks(selection) {
        let (None, PickTarget::SketchEntity(id)) = (&pick.context, &pick.target) else {
            return None;
        };
        let entity = *index.sketch_entities.get(id.as_str())?;
        if *sketch.get_or_insert(entity.sketch.as_str()) != entity.sketch.as_str() {
            return None;
        }
        found.push(entity);
    }
    let sketch = index.sketches.get(sketch?)?;
    matches!(sketch.solve.status, EvaluationStatus::Current).then_some(found)
}

/// The shape the sketch curves of `selection` form: closed where every pick
/// is a sketch region. `None` where the selection picks other than the
/// curves of one current sketch or regions, or a curve's ends are not read.
pub(super) fn shape<'a, O: MemberOrder>(
    index: &DesignIndex<'a>,
    selection: &'a Selection<O>,
    tolerance: f64,
) -> Option<Shape> {
    let picked = picks(selection);
    let regions = picked.iter().all(|pick| {
        pick.context.is_none() && matches!(pick.target, PickTarget::SketchRegion { .. })
    });
    if !picked.is_empty() && regions {
        return Some(Shape::Closed);
    }
    let entities = entities(index, selection)?;
    let mut points = Vec::new();
    let mut curves = Vec::new();
    for entity in &entities {
        match ends(solved(entity)?)? {
            Ends::Closed if entities.len() == 1 => return Some(Shape::Closed),
            Ends::Closed => return None,
            Ends::Two(start, end) => {
                let first = points.len();
                points.extend([start, end]);
                curves.push([first, first + 1]);
            }
        }
    }
    Some(link(&points, &curves, tolerance).shape)
}

/// Whether points lie on one line, within `tolerance` of the line through
/// the first point and the point farthest from it. `None` where the points
/// lie within `tolerance` of one point.
fn collinear(points: &[Point3], tolerance: f64) -> Option<bool> {
    let (&first, rest) = points.split_first()?;
    let far = rest
        .iter()
        .copied()
        .max_by(|a, b| first.distance(*a).total_cmp(&first.distance(*b)))?;
    if first.distance(far) <= tolerance {
        return None;
    }
    let direction = far.vector_from(first).unit()?;
    Some(
        points
            .iter()
            .all(|point| point.vector_from(first).cross(direction).norm() <= tolerance),
    )
}

/// Whether a sketch curve is straight: a NURBS curve where its control
/// points are collinear within `tolerance`, and a spatial piece by its
/// carrier. `None` where it is not read.
fn straight(curve: &PlanarCurve, tolerance: f64) -> Option<bool> {
    match curve {
        PlanarCurve::Line { .. } | PlanarCurve::InfiniteLine { .. } => Some(true),
        PlanarCurve::Arc { .. }
        | PlanarCurve::Circle { .. }
        | PlanarCurve::Ellipse { .. }
        | PlanarCurve::Parabola { .. }
        | PlanarCurve::Hyperbola { .. } => Some(false),
        PlanarCurve::Nurbs(nurbs) => {
            let points: Vec<Point3> = nurbs.control_points().into_iter().map(at).collect();
            collinear(&points, tolerance)
        }
        PlanarCurve::Spatial(SpatialCurve::Bounded(piece)) => match &piece.carrier {
            InlineCurve::Line(_) => Some(true),
            InlineCurve::Circle(_)
            | InlineCurve::Ellipse(_)
            | InlineCurve::Parabola(_)
            | InlineCurve::Hyperbola(_) => Some(false),
            InlineCurve::Nurbs(nurbs) => {
                let points: Vec<Point3> = nurbs
                    .control_points()
                    .into_iter()
                    .map(FinitePoint3::get)
                    .collect();
                collinear(&points, tolerance)
            }
            InlineCurve::Polyline(_) | InlineCurve::Transformed { .. } => None,
        },
        PlanarCurve::Point(_) | PlanarCurve::Spatial(SpatialCurve::Point(_)) => None,
    }
}

/// The picked sketch curves of `selection` that are no line segments.
pub(super) fn curved<'a, O: MemberOrder>(
    index: &DesignIndex<'a>,
    selection: &'a Selection<O>,
    tolerance: f64,
) -> Vec<&'a str> {
    entities(index, selection)
        .into_iter()
        .flatten()
        .filter(|entity| solved(entity).and_then(|curve| straight(curve, tolerance)) == Some(false))
        .map(|entity| entity.id.as_str())
        .collect()
}
