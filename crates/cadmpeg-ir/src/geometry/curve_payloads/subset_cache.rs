// SPDX-License-Identifier: Apache-2.0
//! Solved caches of subset curves on the subset's local parameter.

use std::f64::consts::PI;

use super::SubsetCurveConstruction;
use crate::geometry::analytic::{CircleCurve, EllipseCurve, LineCurve};
use crate::geometry::{PlacedCurve, SolvedCurveGeometry};
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::units::OrthonormalFrame3;

impl SubsetCurveConstruction {
    /// The source geometry reparameterized onto this subset's local
    /// parameter: the point at `s` is the source point at `start + s` with
    /// a true sense and at `end - s` with a false sense. `None` when no
    /// carrier of the source's kind has that parameterization.
    #[must_use]
    pub fn local_cache(&self, source: &SolvedCurveGeometry) -> Option<SolvedCurveGeometry> {
        let [start, end] = self.parameter_range.endpoints();
        local_geometry(source, start, end, self.sense)
    }
}

fn local_geometry(
    source: &SolvedCurveGeometry,
    start: f64,
    end: f64,
    sense: bool,
) -> Option<SolvedCurveGeometry> {
    let origin = if sense { start } else { end };
    match source {
        SolvedCurveGeometry::Line(line) => {
            let direction = *line.direction().as_raw();
            let point = line.origin().get();
            let origin_point = Point3::new(
                point.x + origin * direction.x,
                point.y + origin * direction.y,
                point.z + origin * direction.z,
            );
            let direction = if sense {
                line.direction()
            } else {
                line.direction().negated()
            };
            LineCurve::try_new(origin_point, *direction.as_raw())
                .ok()
                .map(SolvedCurveGeometry::Line)
        }
        SolvedCurveGeometry::Circle(circle) => {
            let frame = rotated_frame(circle.frame(), origin, sense)?;
            Some(SolvedCurveGeometry::Circle(CircleCurve::new(
                circle.center(),
                frame,
                circle.radius(),
            )))
        }
        SolvedCurveGeometry::Ellipse(ellipse) => {
            let half_turns = (origin / PI).round();
            if half_turns * PI == origin {
                let frame = rotated_frame(ellipse.frame(), origin, sense)?;
                return EllipseCurve::try_from_parts(
                    ellipse.center(),
                    frame,
                    ellipse.major_radius(),
                    ellipse.minor_radius(),
                )
                .ok()
                .map(SolvedCurveGeometry::Ellipse);
            }
            phased_ellipse(ellipse, origin, sense)
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let mut nurbs = nurbs.clone();
            if sense {
                nurbs
                    .edit_knots(|knots| knots.iter_mut().for_each(|knot| *knot -= start))
                    .ok()?;
            } else {
                nurbs.reverse_parameterization_in_range(FiniteReal::ZERO, FiniteReal::new(end)?)?;
            }
            Some(SolvedCurveGeometry::Nurbs(nurbs))
        }
        SolvedCurveGeometry::Transformed(placed) => {
            let basis = local_geometry(placed.basis(), start, end, sense)?;
            PlacedCurve::try_new(Box::new(basis), *placed.transform())
                .ok()
                .map(SolvedCurveGeometry::Transformed)
        }
        _ => None,
    }
}

/// An ellipse whose angle 0 lies at `angle` is not in major-axis form: it is
/// `center + cos(s) * point(angle) + sin(s) * tangent(angle)`, the unit circle
/// placed by the affine map with those conjugate semi-diameters as columns.
fn phased_ellipse(ellipse: &EllipseCurve, angle: f64, sense: bool) -> Option<SolvedCurveGeometry> {
    let x = *ellipse.frame().reference().as_raw();
    let y = *ellipse.frame().binormal().as_raw();
    let axis = *ellipse.frame().axis().as_raw();
    let (major, minor) = (ellipse.major_radius().get(), ellipse.minor_radius().get());
    let (sin, cos) = angle.sin_cos();
    let turn = if sense { 1.0 } else { -1.0 };
    let combine =
        |a: f64, b: f64| Vector3::new(a * x.x + b * y.x, a * x.y + b * y.y, a * x.z + b * y.z);
    let first = combine(major * cos, minor * sin);
    let second = combine(-turn * major * sin, turn * minor * cos);
    let center = ellipse.center().get();
    let transform = crate::transform::Transform::affine([
        [first.x, second.x, axis.x, center.x],
        [first.y, second.y, axis.y, center.y],
        [first.z, second.z, axis.z, center.z],
    ])?;
    let unit = CircleCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
    )
    .ok()?;
    PlacedCurve::try_new(Box::new(SolvedCurveGeometry::Circle(unit)), transform)
        .ok()
        .map(SolvedCurveGeometry::Transformed)
}

/// The frame whose angle 0 lies at `angle` of `frame`, with the axis
/// reversed for a false sense so that the angle runs back.
fn rotated_frame(frame: &OrthonormalFrame3, angle: f64, sense: bool) -> Option<OrthonormalFrame3> {
    let x = frame.reference().as_raw();
    let y = frame.binormal();
    let y = y.as_raw();
    let (sin, cos) = angle.sin_cos();
    let reference = Vector3::new(
        cos * x.x + sin * y.x,
        cos * x.y + sin * y.y,
        cos * x.z + sin * y.z,
    );
    let axis = if sense {
        *frame.axis()
    } else {
        frame.axis().negated()
    };
    OrthonormalFrame3::new(*axis.as_raw(), reference)
}

#[cfg(test)]
mod tests;
