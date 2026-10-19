// SPDX-License-Identifier: Apache-2.0
//! Compare generated conic and spline helpers with their current parent geometry.

use super::{close, distance2, endpoints, sketch_locus_point, Geometries};
use crate::math::Point2;
use crate::sketches::{
    SketchEntityId, SketchGeometryDefinition as Geometry, SketchInternalAlignment as Alignment,
    SketchLocus,
};
use crate::CadIr;

fn placed(center: Point2, angle: f64, u: f64, v: f64) -> Option<Point2> {
    let (sine, cosine) = angle.sin_cos();
    let result = Point2::new(
        center.u + cosine * u - sine * v,
        center.v + sine * u + cosine * v,
    );
    (result.u.is_finite() && result.v.is_finite()).then_some(result)
}

fn point_agrees(
    ir: &CadIr,
    helper: &SketchEntityId,
    expected: Point2,
    geometry: &Geometries<'_>,
) -> Option<bool> {
    sketch_locus_point(&SketchLocus::Entity(helper.clone()), geometry)
        .map(|point| close(ir, distance2(point, expected), 0.0))
}

fn diameter_agrees(
    ir: &CadIr,
    helper: &SketchEntityId,
    first: Point2,
    second: Point2,
    geometry: &Geometries<'_>,
) -> Option<bool> {
    endpoints(helper, geometry).map(|(start, end)| {
        (close(ir, distance2(start, first), 0.0) && close(ir, distance2(end, second), 0.0))
            || (close(ir, distance2(start, second), 0.0) && close(ir, distance2(end, first), 0.0))
    })
}

fn axis_agrees(
    ir: &CadIr,
    helper: &SketchEntityId,
    origin: Point2,
    angle: f64,
    geometry: &Geometries<'_>,
) -> Option<bool> {
    let (start, end) = endpoints(helper, geometry)?;
    let (sine, cosine) = angle.sin_cos();
    let normal_distance =
        |point: Point2| ((point.u - origin.u) * sine - (point.v - origin.v) * cosine).abs();
    Some(
        normal_distance(start) <= ir.tolerances.linear.get()
            && normal_distance(end) <= ir.tolerances.linear.get(),
    )
}

pub(super) fn check(
    ir: &CadIr,
    helper: &SketchEntityId,
    parent: &SketchEntityId,
    alignment: Alignment,
    geometry: &Geometries<'_>,
) -> Option<bool> {
    match (alignment, geometry.get(parent.as_str())?.definition()) {
        (
            Alignment::EllipseMajorDiameter | Alignment::EllipseMinorDiameter,
            Geometry::Ellipse {
                center,
                major_angle,
                radii,
                ..
            },
        ) => {
            let (u, v) = if alignment == Alignment::EllipseMajorDiameter {
                (radii.major().get(), 0.0)
            } else {
                (0.0, radii.minor().get())
            };
            diameter_agrees(
                ir,
                helper,
                placed(center.get(), major_angle.get(), u, v)?,
                placed(center.get(), major_angle.get(), -u, -v)?,
                geometry,
            )
        }
        (
            Alignment::EllipseFocus1 | Alignment::EllipseFocus2,
            Geometry::Ellipse {
                center,
                major_angle,
                radii,
                ..
            },
        ) => {
            let ratio = radii.minor().get() / radii.major().get();
            let focus = radii.major().get() * (1.0 - ratio * ratio).sqrt();
            // Focus numbering is source-specific. Both labels must identify an
            // actual focus, without inventing a sign convention for the label.
            let first = placed(center.get(), major_angle.get(), focus, 0.0)?;
            let second = placed(center.get(), major_angle.get(), -focus, 0.0)?;
            Some(
                point_agrees(ir, helper, first, geometry)?
                    || point_agrees(ir, helper, second, geometry)?,
            )
        }
        (
            Alignment::HyperbolaMajor | Alignment::HyperbolaMinor,
            Geometry::Hyperbola {
                center,
                major_angle,
                ..
            },
        ) => axis_agrees(
            ir,
            helper,
            center.get(),
            major_angle.get()
                + if alignment == Alignment::HyperbolaMinor {
                    std::f64::consts::FRAC_PI_2
                } else {
                    0.0
                },
            geometry,
        ),
        (
            Alignment::HyperbolaFocus,
            Geometry::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                ..
            },
        ) => {
            let focus = major_radius.get().hypot(minor_radius.get());
            let first = placed(center.get(), major_angle.get(), focus, 0.0)?;
            let second = placed(center.get(), major_angle.get(), -focus, 0.0)?;
            Some(
                point_agrees(ir, helper, first, geometry)?
                    || point_agrees(ir, helper, second, geometry)?,
            )
        }
        (
            Alignment::ParabolaFocus,
            Geometry::Parabola {
                vertex,
                axis_angle,
                focal_length,
                ..
            },
        ) => point_agrees(
            ir,
            helper,
            placed(vertex.get(), axis_angle.get(), focal_length.get(), 0.0)?,
            geometry,
        ),
        (
            Alignment::ParabolaFocalAxis,
            Geometry::Parabola {
                vertex, axis_angle, ..
            },
        ) => axis_agrees(ir, helper, vertex.get(), axis_angle.get(), geometry),
        (Alignment::BsplineControlPoint(index), Geometry::Nurbs { curve }) => {
            let index = usize::try_from(index).ok()?;
            let Some(point) = curve.pole_rows().point_at(index) else {
                return Some(false);
            };
            point_agrees(ir, helper, point.get(), geometry)
        }
        // The index does not specify an expanded versus distinct knot lane.
        // No parameter value can be selected honestly from that fact alone.
        (Alignment::BsplineKnotPoint(_), Geometry::Nurbs { .. }) => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests;
