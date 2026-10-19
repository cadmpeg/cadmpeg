// SPDX-License-Identifier: Apache-2.0
//! Measurable spatial constraint agreement against current solved carriers.

use crate::index::identities::BorrowedIdentities;
use crate::math::{Point3, Vector3};
use crate::sketches::{
    SpatialSketchConstraintDefinitionInput as Constraint, SpatialSketchEntityId,
    SpatialSketchGeometry, SpatialSketchGeometryDefinition as Geometry, SpatialSketchId,
};
use crate::CadIr;

type Geometries<'a> = BorrowedIdentities<'a, (&'a SpatialSketchId, &'a SpatialSketchGeometry)>;

fn delta(first: Point3, second: Point3) -> Vector3 {
    Vector3::new(second.x - first.x, second.y - first.y, second.z - first.z)
}

fn close(ir: &CadIr, first: Point3, second: Point3) -> bool {
    delta(first, second).norm() <= ir.tolerances.linear.get()
}

fn point(id: &SpatialSketchEntityId, geometry: &Geometries<'_>) -> Option<Point3> {
    match geometry.get(id.as_str())?.1.definition() {
        Geometry::Point { position } => Some(position.get()),
        _ => None,
    }
}

fn line(id: &SpatialSketchEntityId, geometry: &Geometries<'_>) -> Option<(Point3, Vector3)> {
    match geometry.get(id.as_str())?.1.definition() {
        Geometry::Line { start, end } => {
            let direction = crate::features::FiniteVector3::new(delta(start.get(), end.get()))?
                .unit_nonzero()?;
            Some((start.get(), direction))
        }
        _ => None,
    }
}

fn circle(id: &SpatialSketchEntityId, geometry: &Geometries<'_>) -> Option<(Point3, Vector3, f64)> {
    match geometry.get(id.as_str())?.1.definition() {
        Geometry::Circle {
            center,
            normal,
            radius,
            ..
        }
        | Geometry::Arc {
            center,
            normal,
            radius,
            ..
        } => Some((center.get(), *normal.as_raw(), radius.get())),
        _ => None,
    }
}

fn line_circle(ir: &CadIr, line: (Point3, Vector3), circle: (Point3, Vector3, f64)) -> bool {
    let displacement = delta(line.0, circle.0);
    line.1.dot(circle.1).abs() <= ir.tolerances.angular.get()
        && displacement.dot(circle.1).abs() <= ir.tolerances.linear.get()
        && (displacement.cross(line.1).norm() - circle.2).abs() <= ir.tolerances.linear.get()
}

fn tangent(
    ir: &CadIr,
    first: &SpatialSketchEntityId,
    second: &SpatialSketchEntityId,
    geometry: &Geometries<'_>,
) -> Option<bool> {
    match (line(first, geometry), line(second, geometry)) {
        (Some(first), Some(second)) => {
            return Some(
                first.1.cross(second.1).norm() <= ir.tolerances.angular.get()
                    && delta(first.0, second.0).cross(first.1).norm() <= ir.tolerances.linear.get(),
            )
        }
        (Some(first), None) => {
            return circle(second, geometry).map(|second| line_circle(ir, first, second))
        }
        (None, Some(second)) => {
            return circle(first, geometry).map(|first| line_circle(ir, second, first))
        }
        _ => {}
    }
    let (first, second) = circle(first, geometry).zip(circle(second, geometry))?;
    if first.1.cross(second.1).norm() > ir.tolerances.angular.get() {
        // Non-coplanar circles can touch. Their tangency needs a different
        // complete witness, rather than the planar center-distance relation.
        return None;
    }
    let displacement = delta(first.0, second.0);
    let distance = displacement.norm();
    Some(
        displacement.dot(first.1).abs() <= ir.tolerances.linear.get()
            && ((distance - first.2 - second.2).abs() <= ir.tolerances.linear.get()
                || (distance - (first.2 - second.2).abs()).abs() <= ir.tolerances.linear.get()),
    )
}

pub(super) fn check<U, L>(
    ir: &CadIr,
    constraint: &Constraint<U, L>,
    geometry: &Geometries<'_>,
) -> Option<&'static str> {
    let (agrees, message) = match constraint {
        Constraint::Coincident { first, second } => (
            point(first, geometry)
                .zip(point(second, geometry))
                .map(|(first, second)| close(ir, first, second)),
            "solved spatial points disagree with their coincidence constraint",
        ),
        Constraint::Midpoint {
            point: point_id,
            entity,
        } => (
            point(point_id, geometry).and_then(|point| {
                match geometry.get(entity.as_str())?.1.definition() {
                    Geometry::Line { start, end } => Some(close(
                        ir,
                        point,
                        Point3::new(
                            f64::midpoint(start.x, end.x),
                            f64::midpoint(start.y, end.y),
                            f64::midpoint(start.z, end.z),
                        ),
                    )),
                    _ => None,
                }
            }),
            "solved spatial point disagrees with its line midpoint constraint",
        ),
        Constraint::Tangent { first, second } => (
            tangent(ir, first, second, geometry),
            "solved spatial analytic carriers disagree with their tangency constraint",
        ),
        _ => return None,
    };
    (agrees == Some(false)).then_some(message)
}

/// A singleton line pair has an unambiguous offset distance even though general
/// source/result sets do not state a positional correspondence between members.
pub(super) fn offset(
    ir: &CadIr,
    sources: &[SpatialSketchEntityId],
    results: &[SpatialSketchEntityId],
    normal: &crate::units::UnitVector3,
    distance: &crate::features::Dimension<crate::scalar::NonZeroLength>,
    geometry: &Geometries<'_>,
) -> Option<&'static str> {
    let ([source], [result]) = (sources, results) else {
        return None;
    };
    let source = line(source, geometry)?;
    let result = line(result, geometry)?;
    let expected = ir.dimension_value(distance)?.get().abs();
    let displacement = delta(source.0, result.0);
    let normal = *normal.as_raw();
    let agrees = source.1.cross(result.1).norm() <= ir.tolerances.angular.get()
        && source.1.dot(normal).abs() <= ir.tolerances.angular.get()
        && displacement.dot(normal).abs() <= ir.tolerances.linear.get()
        && (displacement.cross(source.1).norm() - expected).abs() <= ir.tolerances.linear.get();
    (!agrees).then_some("spatial offset geometry disagrees with its canonical distance")
}

#[cfg(test)]
mod tests;
