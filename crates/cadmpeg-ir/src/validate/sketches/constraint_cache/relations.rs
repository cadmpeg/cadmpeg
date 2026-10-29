// SPDX-License-Identifier: Apache-2.0
//! Analytic relations whose current solved carriers supply a complete witness.

use super::{close, distance2, endpoints, mismatch, sketch_locus_point, Geometries};
use crate::math::Point2;
use crate::sketches::{
    SketchConstraintDefinitionInput as Constraint, SketchDistanceMeasurement, SketchEntityId,
    SketchGeometryDefinition as Geometry, SketchLocus,
};
use crate::CadIr;

fn center(id: &SketchEntityId, geometry: &Geometries<'_>) -> Option<Point2> {
    match geometry.get(id.as_str())?.definition() {
        Geometry::Circle { center, .. }
        | Geometry::Arc { center, .. }
        | Geometry::Ellipse { center, .. } => Some(center.get()),
        _ => None,
    }
}

fn circle(id: &SketchEntityId, geometry: &Geometries<'_>) -> Option<(Point2, f64)> {
    match geometry.get(id.as_str())?.definition() {
        Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => {
            Some((center.get(), radius.get()))
        }
        _ => None,
    }
}

fn parallel(ir: &CadIr, first: (Point2, Point2), second: (Point2, Point2)) -> bool {
    let a = (first.1.u - first.0.u, first.1.v - first.0.v);
    let b = (second.1.u - second.0.u, second.1.v - second.0.v);
    (a.0 * b.1 - a.1 * b.0).abs() <= ir.tolerances.angular.get() * a.0.hypot(a.1) * b.0.hypot(b.1)
}

fn line_distance(point: Point2, line: (Point2, Point2)) -> f64 {
    let direction = (line.1.u - line.0.u, line.1.v - line.0.v);
    ((point.u - line.0.u) * direction.1 - (point.v - line.0.v) * direction.0).abs()
        / direction.0.hypot(direction.1)
}

fn tangent(
    ir: &CadIr,
    first: &SketchEntityId,
    second: &SketchEntityId,
    geometry: &Geometries<'_>,
) -> Option<bool> {
    match (circle(first, geometry), circle(second, geometry)) {
        (Some((first_center, first_radius)), Some((second_center, second_radius))) => {
            let distance = distance2(first_center, second_center);
            return Some(
                close(ir, distance, first_radius + second_radius)
                    || close(ir, distance, (first_radius - second_radius).abs()),
            );
        }
        (Some((center, radius)), None) => {
            return endpoints(second, geometry)
                .map(|line| close(ir, line_distance(center, line), radius));
        }
        (None, Some((center, radius))) => {
            return endpoints(first, geometry)
                .map(|line| close(ir, line_distance(center, line), radius));
        }
        (None, None) => {}
    }
    endpoints(first, geometry)
        .zip(endpoints(second, geometry))
        .map(|(first, second)| {
            parallel(ir, first, second) && close(ir, line_distance(first.0, second), 0.0)
        })
}

fn tangent_direction(
    locus: &SketchLocus,
    point: Point2,
    geometry: &Geometries<'_>,
) -> Option<(f64, f64)> {
    let id = super::super::locus_entity(locus);
    match geometry.get(id.as_str())?.definition() {
        Geometry::Line { start, end } => Some((end.u - start.u, end.v - start.v)),
        Geometry::Circle { center, .. } | Geometry::Arc { center, .. } => {
            Some((center.v - point.v, point.u - center.u))
        }
        Geometry::Ellipse {
            center,
            major_angle,
            radii,
            ..
        } => {
            let cosine = major_angle.get().cos();
            let sine = major_angle.get().sin();
            let u = cosine * (point.u - center.u) + sine * (point.v - center.v);
            let v = -sine * (point.u - center.u) + cosine * (point.v - center.v);
            let normal_u = u / radii.major().get().powi(2);
            let normal_v = v / radii.minor().get().powi(2);
            Some((
                -sine * normal_u - cosine * normal_v,
                cosine * normal_u - sine * normal_v,
            ))
        }
        _ => None,
    }
}

pub(super) fn check(
    ir: &CadIr,
    definition: &Constraint,
    geometry: &Geometries<'_>,
) -> Option<&'static str> {
    match definition {
        Constraint::Coincident { entities } => {
            let points: Option<Vec<_>> = entities
                .iter()
                .map(|entity| sketch_locus_point(&SketchLocus::Entity(entity.clone()), geometry))
                .collect();
            mismatch(
                points.and_then(|points| {
                    points.first().map(|first| {
                        points
                            .iter()
                            .skip(1)
                            .all(|point| close(ir, distance2(*first, *point), 0.0))
                    })
                }),
                "solved points disagree with their coincidence constraint",
            )
        }
        Constraint::Concentric { first, second } | Constraint::Coradial { first, second } => {
            let concentric = center(first, geometry)
                .zip(center(second, geometry))
                .map(|(first, second)| close(ir, distance2(first, second), 0.0));
            let agrees = if matches!(definition, Constraint::Coradial { .. }) {
                concentric
                    .zip(circle(first, geometry).zip(circle(second, geometry)))
                    .map(|(concentric, ((_, first_radius), (_, second_radius)))| {
                        concentric && close(ir, first_radius, second_radius)
                    })
            } else {
                concentric
            };
            mismatch(
                agrees,
                "solved conics disagree with their concentric or coradial constraint",
            )
        }
        Constraint::Collinear { first, second } => mismatch(
            endpoints(first, geometry)
                .zip(endpoints(second, geometry))
                .map(|(first, second)| {
                    parallel(ir, first, second) && close(ir, line_distance(first.0, second), 0.0)
                }),
            "solved lines disagree with their collinearity constraint",
        ),
        Constraint::Symmetric {
            first,
            second,
            axis,
        } => mismatch(
            sketch_locus_point(first, geometry)
                .zip(sketch_locus_point(second, geometry))
                .zip(endpoints(axis, geometry))
                .map(|((first, second), axis)| {
                    let midpoint = Point2::new(
                        f64::midpoint(first.u, second.u),
                        f64::midpoint(first.v, second.v),
                    );
                    let direction = (axis.1.u - axis.0.u, axis.1.v - axis.0.v);
                    close(ir, line_distance(midpoint, axis), 0.0)
                        && ((second.u - first.u) * direction.0 + (second.v - first.v) * direction.1)
                            .abs()
                            <= ir.tolerances.linear.get() * direction.0.hypot(direction.1)
                }),
            "solved loci disagree with their line-symmetry constraint",
        ),
        Constraint::Tangent { first, second } => mismatch(
            tangent(ir, first, second, geometry),
            "solved analytic carriers disagree with their tangency constraint",
        ),
        Constraint::TangentLoci { first, second } => mismatch(
            sketch_locus_point(first, geometry)
                .zip(sketch_locus_point(second, geometry))
                .and_then(|(first_point, second_point)| {
                    let a = tangent_direction(first, first_point, geometry)?;
                    let b = tangent_direction(second, second_point, geometry)?;
                    Some(
                        close(ir, distance2(first_point, second_point), 0.0)
                            && (a.0 * b.1 - a.1 * b.0).abs()
                                <= ir.tolerances.angular.get() * a.0.hypot(a.1) * b.0.hypot(b.1),
                    )
                }),
            "solved contact loci disagree with their tangency constraint",
        ),
        Constraint::Curvature { first, second } => {
            let equal_curvature = match (circle(first, geometry), circle(second, geometry)) {
                (Some((_, first_radius)), Some((_, second_radius))) => {
                    Some(close(ir, first_radius, second_radius))
                }
                (None, None)
                    if endpoints(first, geometry).is_some()
                        && endpoints(second, geometry).is_some() =>
                {
                    Some(true)
                }
                (Some(_), None) if endpoints(second, geometry).is_some() => Some(false),
                (None, Some(_)) if endpoints(first, geometry).is_some() => Some(false),
                _ => None,
            };
            mismatch(
                equal_curvature
                    .zip(tangent(ir, first, second, geometry))
                    .map(|(curvature, tangent)| curvature && tangent),
                "solved analytic carriers disagree with their curvature-continuity constraint",
            )
        }
        Constraint::RepeatedDistance {
            measurements,
            parameter,
        } => mismatch(
            ir.dimension_value(parameter).and_then(|expected| {
                let agrees: Option<Vec<_>> = measurements
                    .iter()
                    .map(|measurement| {
                        let (first, second) = match measurement {
                            SketchDistanceMeasurement::Distance { first, second }
                            | SketchDistanceMeasurement::Horizontal { first, second }
                            | SketchDistanceMeasurement::Vertical { first, second } => {
                                (first, second)
                            }
                        };
                        let first = sketch_locus_point(first, geometry)?;
                        let second = sketch_locus_point(second, geometry)?;
                        let measured = match measurement {
                            SketchDistanceMeasurement::Distance { .. } => distance2(first, second),
                            SketchDistanceMeasurement::Horizontal { .. } => second.u - first.u,
                            SketchDistanceMeasurement::Vertical { .. } => second.v - first.v,
                        };
                        Some(close(ir, measured, expected.get()))
                    })
                    .collect();
                agrees.map(|values| values.into_iter().all(std::convert::identity))
            }),
            "solved repeated distances disagree with their canonical dimension",
        ),
        // These slots do not contain enough canonical geometry to invent a
        // second measurement: native/text/scalar-group records have no solved
        // relation, and Fixed stores no alternate location to compare.
        _ => None,
    }
}
