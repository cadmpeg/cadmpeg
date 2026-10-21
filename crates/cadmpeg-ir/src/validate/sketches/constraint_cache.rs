// SPDX-License-Identifier: Apache-2.0
//! Checks measurable canonical constraints against their solved planar carriers.

use super::{distance2, sketch_locus_point};
use crate::features::{Dimension, DimensionValue};
use crate::index::identities::BorrowedIdentities;
use crate::math::Point2;
use crate::sketches::{
    SketchAxis, SketchConstraintDefinitionInput as Constraint, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition as Geometry, SketchLocus,
};
use crate::CadIr;

type Geometries<'a> = BorrowedIdentities<'a, &'a SketchGeometry>;

fn close(ir: &CadIr, measured: f64, expected: f64) -> bool {
    (measured - expected).abs() <= ir.tolerances.linear.get()
}
fn angle_close(ir: &CadIr, measured: f64, expected: f64) -> bool {
    let difference = (measured - expected).rem_euclid(std::f64::consts::TAU);
    difference.min(std::f64::consts::TAU - difference) <= ir.tolerances.angular.get()
}
fn scalar<Q: DimensionValue>(
    ir: &CadIr,
    dimension: &Dimension<Q>,
    numeric: impl FnOnce(Q) -> f64,
) -> Option<f64> {
    ir.dimension_value(dimension).map(numeric)
}
fn endpoints(id: &SketchEntityId, geometry: &Geometries<'_>) -> Option<(Point2, Point2)> {
    match geometry.get(id.as_str())?.definition() {
        Geometry::Line { start, end } => Some((start.get(), end.get())),
        _ => None,
    }
}
fn direction(id: &SketchEntityId, geometry: &Geometries<'_>) -> Option<f64> {
    let (start, end) = endpoints(id, geometry)?;
    Some((end.v - start.v).atan2(end.u - start.u))
}
fn radius(id: &SketchEntityId, geometry: &Geometries<'_>) -> Option<f64> {
    match geometry.get(id.as_str())?.definition() {
        Geometry::Circle { radius, .. } | Geometry::Arc { radius, .. } => Some(radius.get()),
        _ => None,
    }
}
fn size(id: &SketchEntityId, geometry: &Geometries<'_>) -> Option<f64> {
    if let Some(radius) = radius(id, geometry) {
        return Some(radius);
    }
    let (start, end) = endpoints(id, geometry)?;
    Some(distance2(start, end))
}
fn point_on(
    point: Point2,
    id: &SketchEntityId,
    geometry: &Geometries<'_>,
    ir: &CadIr,
) -> Option<bool> {
    match geometry.get(id.as_str())?.definition() {
        Geometry::Line { start, end } => {
            let length = distance2(start.get(), end.get());
            Some(
                ((point.u - start.u) * (end.v - start.v) - (point.v - start.v) * (end.u - start.u))
                    .abs()
                    <= ir.tolerances.linear.get() * length,
            )
        }
        Geometry::Circle { center, radius } | Geometry::Arc { center, radius, .. } => {
            Some(close(ir, distance2(point, center.get()), radius.get()))
        }
        _ => None,
    }
}
fn mismatch(result: Option<bool>, message: &'static str) -> Option<&'static str> {
    (result == Some(false)).then_some(message)
}

pub(super) fn check(
    ir: &CadIr,
    definition: &Constraint,
    geometry: &Geometries<'_>,
) -> Option<&'static str> {
    match definition {
        Constraint::DistanceLoci {
            first,
            second,
            parameter,
        }
        | Constraint::HorizontalDistance {
            first,
            second,
            parameter,
        }
        | Constraint::VerticalDistance {
            first,
            second,
            parameter,
        } => {
            let points =
                sketch_locus_point(first, geometry).zip(sketch_locus_point(second, geometry));
            let expected = scalar(ir, parameter, crate::scalar::Length::get);
            mismatch(
                points.zip(expected).map(|((first, second), expected)| {
                    let measured = match definition {
                        Constraint::HorizontalDistance { .. } => second.u - first.u,
                        Constraint::VerticalDistance { .. } => second.v - first.v,
                        _ => distance2(first, second),
                    };
                    close(ir, measured, expected)
                }),
                "solved locus distance disagrees with its canonical dimension",
            )
        }
        Constraint::Distance {
            entities,
            parameter,
        } => {
            let measured = match entities.as_slice() {
                [entity] => endpoints(entity, geometry).map(|(start, end)| distance2(start, end)),
                [first, second] => {
                    sketch_locus_point(&SketchLocus::Entity(first.clone()), geometry)
                        .zip(sketch_locus_point(
                            &SketchLocus::Entity(second.clone()),
                            geometry,
                        ))
                        .map(|(first, second)| distance2(first, second))
                }
                _ => None,
            };
            mismatch(
                measured
                    .zip(scalar(ir, parameter, crate::scalar::Length::get))
                    .map(|(measured, expected)| close(ir, measured, expected)),
                "solved distance disagrees with its canonical dimension",
            )
        }
        Constraint::RepeatedLength {
            entities,
            parameter,
        } => {
            let expected = scalar(ir, parameter, crate::scalar::Length::get);
            mismatch(
                expected.map(|expected| {
                    entities.iter().all(|entity| {
                        endpoints(entity, geometry)
                            .is_none_or(|(start, end)| close(ir, distance2(start, end), expected))
                    })
                }),
                "solved repeated line length disagrees with its canonical dimension",
            )
        }
        Constraint::Radius { entity, parameter } | Constraint::Diameter { entity, parameter } => {
            let expected = scalar(ir, parameter, crate::scalar::PositiveLength::get);
            mismatch(
                radius(entity, geometry)
                    .zip(expected)
                    .map(|(radius, expected)| {
                        close(
                            ir,
                            if matches!(definition, Constraint::Diameter { .. }) {
                                2.0 * radius
                            } else {
                                radius
                            },
                            expected,
                        )
                    }),
                "solved circular size disagrees with its canonical dimension",
            )
        }
        Constraint::RepeatedRadius {
            entities,
            parameter,
        }
        | Constraint::RepeatedDiameter {
            entities,
            parameter,
        } => {
            let factor = if matches!(definition, Constraint::RepeatedDiameter { .. }) {
                2.0
            } else {
                1.0
            };
            mismatch(
                scalar(ir, parameter, crate::scalar::PositiveLength::get).map(|expected| {
                    entities.iter().all(|entity| {
                        radius(entity, geometry)
                            .is_none_or(|radius| close(ir, factor * radius, expected))
                    })
                }),
                "solved repeated circular size disagrees with its canonical dimension",
            )
        }
        Constraint::Angle {
            first,
            second,
            parameter,
        } => mismatch(
            direction(first, geometry)
                .zip(direction(second, geometry))
                .zip(scalar(ir, parameter, crate::scalar::Angle::get))
                .map(|((first, second), expected)| angle_close(ir, second - first, expected)),
            "solved line angle disagrees with its canonical dimension",
        ),
        Constraint::AngleToAxis {
            entity,
            axis,
            parameter,
        } => {
            let axis = match axis {
                SketchAxis::Horizontal => 0.0,
                SketchAxis::Vertical => std::f64::consts::FRAC_PI_2,
            };
            mismatch(
                direction(entity, geometry)
                    .zip(scalar(ir, parameter, crate::scalar::Angle::get))
                    .map(|(angle, expected)| angle_close(ir, angle - axis, expected)),
                "solved line-to-axis angle disagrees with its canonical dimension",
            )
        }
        Constraint::ArcAngle { entity, angle } | Constraint::EllipseAngle { entity, angle } => {
            let measured =
                geometry
                    .get(entity.as_str())
                    .and_then(|geometry| match geometry.definition() {
                        Geometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        } => Some(
                            (end_angle.get() - start_angle.get()).rem_euclid(std::f64::consts::TAU),
                        ),
                        Geometry::Ellipse {
                            bounds: Some(bounds),
                            ..
                        } => Some(
                            (bounds[1].get() - bounds[0].get()).rem_euclid(std::f64::consts::TAU),
                        ),
                        _ => None,
                    });
            mismatch(
                measured
                    .zip(scalar(ir, angle, crate::scalar::PositiveAngle::get))
                    .map(|(measured, expected)| angle_close(ir, measured, expected)),
                "solved curve sweep disagrees with its canonical angular dimension",
            )
        }
        Constraint::Horizontal { entity } | Constraint::Vertical { entity } => mismatch(
            endpoints(entity, geometry).map(|(start, end)| {
                if matches!(definition, Constraint::Horizontal { .. }) {
                    close(ir, start.v, end.v)
                } else {
                    close(ir, start.u, end.u)
                }
            }),
            "solved line disagrees with its horizontal or vertical constraint",
        ),
        Constraint::Parallel { first, second } | Constraint::Perpendicular { first, second } => {
            mismatch(
                direction(first, geometry)
                    .zip(direction(second, geometry))
                    .map(|(first, second)| {
                        let difference = second - first;
                        if matches!(definition, Constraint::Parallel { .. }) {
                            difference.sin().abs() <= ir.tolerances.angular.get()
                        } else {
                            difference.cos().abs() <= ir.tolerances.angular.get()
                        }
                    }),
                "solved lines disagree with their parallel or perpendicular constraint",
            )
        }
        Constraint::Equal { first, second } => mismatch(
            size(first, geometry)
                .zip(size(second, geometry))
                .map(|(first, second)| close(ir, first, second)),
            "solved entities disagree with their equal-size constraint",
        ),
        Constraint::CoincidentLoci { loci } => {
            let first = loci
                .first()
                .and_then(|locus| sketch_locus_point(locus, geometry));
            mismatch(
                first.map(|first| {
                    loci.iter().skip(1).all(|locus| {
                        sketch_locus_point(locus, geometry)
                            .is_none_or(|point| close(ir, distance2(first, point), 0.0))
                    })
                }),
                "solved loci disagree with their coincidence constraint",
            )
        }
        Constraint::SameCoordinate { relation } => mismatch(
            sketch_locus_point(relation.first(), geometry)
                .zip(sketch_locus_point(relation.second(), geometry))
                .map(|(first, second)| match relation.axis() {
                    crate::sketches::SketchCoordinateAxis::U => close(ir, first.u, second.u),
                    crate::sketches::SketchCoordinateAxis::V => close(ir, first.v, second.v),
                }),
            "solved loci disagree with their shared-coordinate constraint",
        ),
        Constraint::PointOnObject { point, entity } => mismatch(
            sketch_locus_point(point, geometry)
                .and_then(|point| point_on(point, entity, geometry, ir)),
            "solved point does not lie on its canonical supporting entity",
        ),
        Constraint::AtIntersection {
            point,
            first,
            second,
        } => mismatch(
            sketch_locus_point(point, geometry).and_then(|point| {
                Some(
                    point_on(point, first, geometry, ir)? && point_on(point, second, geometry, ir)?,
                )
            }),
            "solved point does not lie at its canonical entity intersection",
        ),
        Constraint::Midpoint { point, entity } => mismatch(
            sketch_locus_point(point, geometry)
                .zip(endpoints(entity, geometry))
                .map(|(point, (start, end))| {
                    close(ir, point.u, f64::midpoint(start.u, end.u))
                        && close(ir, point.v, f64::midpoint(start.v, end.v))
                }),
            "solved point disagrees with its canonical midpoint constraint",
        ),
        Constraint::PointSymmetric {
            first,
            second,
            center,
        } => mismatch(
            sketch_locus_point(first, geometry)
                .zip(sketch_locus_point(second, geometry))
                .zip(sketch_locus_point(center, geometry))
                .map(|((first, second), center)| {
                    close(ir, f64::midpoint(first.u, second.u), center.u)
                        && close(ir, f64::midpoint(first.v, second.v), center.v)
                }),
            "solved points disagree with their central-symmetry constraint",
        ),
        Constraint::RectangularPattern { pattern } => patterns::rectangular(ir, pattern, geometry),
        Constraint::CircularPattern { pattern } => patterns::circular(ir, pattern, geometry),
        Constraint::InternalAlignment {
            helper,
            parent,
            alignment,
        } => mismatch(
            alignment::check(ir, helper, parent, *alignment, geometry),
            "solved internal-alignment helper disagrees with its canonical conic or spline",
        ),
        _ => relations::check(ir, definition, geometry),
    }
}

mod alignment;
mod patterns;
mod relations;

#[cfg(test)]
mod tests;
