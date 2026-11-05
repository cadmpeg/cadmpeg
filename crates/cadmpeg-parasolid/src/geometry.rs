// SPDX-License-Identifier: Apache-2.0
//! Neutral geometry of Parasolid analytic carrier nodes.
//!
//! [`analytic`] maps the value fields of one LINE, CIRCLE, ELLIPSE, PLANE,
//! CYLINDER, CONE, SPHERE, or TORUS node, in base-schema order after
//! `sense` and in metres, to IR geometry in millimetres. The stored axis and
//! reference vectors are kept as written. The `sense` byte stays with the
//! host, which assigns its meaning.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, LineCurve, PlaneSurface,
    SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{
    Angle, FiniteReal, Magnification, NonNegativeLength, NonZeroLength, PositiveLength,
    PositiveReal,
};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

use crate::records::node_type::{CIRCLE, CONE, CYLINDER, ELLIPSE, LINE, PLANE, SPHERE, TORUS};

/// Largest departure of a cone's stored `sin² + cos²` from one.
const EPS_CONE_HALF_ANGLE_IDENTITY: f64 = 1.0e-9;

const MILLIMETRES_PER_METRE: f64 = Magnification::MILLIMETERS_PER_METER.get();

/// The neutral geometry of one analytic node.
#[derive(Debug, Clone, PartialEq)]
pub enum Analytic {
    /// A LINE, CIRCLE, or ELLIPSE.
    Curve(CurveGeometry),
    /// A PLANE, CYLINDER, CONE, SPHERE, or TORUS.
    Surface {
        /// The surface, with a torus at the magnitude of its major radius.
        geometry: SurfaceGeometry,
        /// Whether a torus stores a negative major radius, which reverses
        /// its parameter frame.
        negative_major: bool,
    },
}

/// The number of value fields of an analytic node type after `sense`.
#[must_use]
pub fn value_count(kind: u16) -> Option<usize> {
    match kind {
        LINE => Some(6),
        PLANE => Some(9),
        CIRCLE | CYLINDER | SPHERE => Some(10),
        ELLIPSE | TORUS => Some(11),
        CONE => Some(12),
        _ => None,
    }
}

/// Why [`analytic`] refuses the value fields of an analytic node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The fields break a rule of their node type other than the cone
    /// half-angle rules.
    Fields,
    /// A cone stores a negative sine or cosine of its half-angle.
    ConeHalfAngleSign,
    /// The squared sum of a cone's stored sine and cosine differs from one
    /// by more than `1e-9`.
    ConeHalfAngleIdentity,
}

/// The half-angle of a cone from its stored sine and cosine. Each value is
/// in the open interval (0, 1), and `sin² + cos²` is within `1e-9` of one.
///
/// # Errors
///
/// Returns the [`Refusal`] that names the broken rule.
pub fn cone_half_angle(sine: f64, cosine: f64) -> Result<Angle, Refusal> {
    let unit_magnitude = |value: f64| value != 0.0 && value.abs() < 1.0;
    (unit_magnitude(sine) && unit_magnitude(cosine))
        .then_some(())
        .ok_or(Refusal::Fields)?;
    ((sine * sine + cosine * cosine - 1.0).abs() <= EPS_CONE_HALF_ANGLE_IDENTITY)
        .then_some(())
        .ok_or(Refusal::ConeHalfAngleIdentity)?;
    (sine > 0.0 && cosine > 0.0)
        .then_some(())
        .ok_or(Refusal::ConeHalfAngleSign)?;
    let half_angle = FiniteReal::new(sine)
        .zip(FiniteReal::new(cosine))
        .map(|(sine, cosine)| sine.atan2(cosine))
        .ok_or(Refusal::Fields)?;
    Ok(Angle::from_assigned_real(half_angle))
}

/// The neutral geometry of an analytic node of type `kind` from its value
/// fields `v`, in base-schema order after `sense` and in metres. Every
/// value is finite, every axis and reference vector is a unit vector, each
/// axis is perpendicular to its reference vector, each radius has the sign
/// its type requires, and a cone half-angle obeys [`cone_half_angle`].
///
/// # Errors
///
/// Returns the [`Refusal`] that names the broken rule. A cone reports a
/// half-angle refusal only when its other fields are valid.
pub fn analytic(kind: u16, v: &[f64]) -> Result<Analytic, Refusal> {
    (Some(v.len()) == value_count(kind) && v.iter().all(|value| value.is_finite()))
        .then_some(())
        .ok_or(Refusal::Fields)?;
    let half_angle = if kind == CONE {
        cone_half_angle(v[7], v[8])
    } else {
        Ok(Angle::ZERO)
    };
    let geometry = analytic_fields(kind, v, half_angle.unwrap_or(Angle::ZERO));
    let geometry = geometry.ok_or(Refusal::Fields)?;
    half_angle.map(|_| geometry)
}

fn analytic_fields(kind: u16, v: &[f64], half_angle: Angle) -> Option<Analytic> {
    let point = || {
        FinitePoint3::new(Point3::new(
            v[0] * MILLIMETRES_PER_METRE,
            v[1] * MILLIMETRES_PER_METRE,
            v[2] * MILLIMETRES_PER_METRE,
        ))
    };
    let length = |value: f64| value * MILLIMETRES_PER_METRE;
    let curve = |geometry| Some(Analytic::Curve(CurveGeometry::Solved(geometry)));
    let surface = |geometry, negative_major| {
        Some(Analytic::Surface {
            geometry: SurfaceGeometry::Solved(geometry),
            negative_major,
        })
    };
    match kind {
        LINE => curve(SolvedCurveGeometry::Line(LineCurve::new(
            point()?,
            UnitVector3::new(vector(&v[3..6]))?,
        ))),
        CIRCLE => curve(SolvedCurveGeometry::Circle(CircleCurve::new(
            point()?,
            frame(&v[3..6], &v[6..9])?,
            PositiveLength::new(length(v[9]))?,
        ))),
        ELLIPSE => curve(SolvedCurveGeometry::Ellipse(
            EllipseCurve::from_source_radii(
                point()?,
                frame(&v[3..6], &v[6..9])?,
                [v[9], v[10]],
                PositiveReal::new(MILLIMETRES_PER_METRE)?,
            )?,
        )),
        PLANE => surface(
            SolvedSurfaceGeometry::Plane(PlaneSurface::new(point()?, frame(&v[3..6], &v[6..9])?)),
            false,
        ),
        CYLINDER => surface(
            SolvedSurfaceGeometry::Cylinder(CylinderSurface::new(
                point()?,
                frame(&v[3..6], &v[7..10])?,
                PositiveLength::new(length(v[6]))?,
            )),
            false,
        ),
        CONE => surface(
            SolvedSurfaceGeometry::Cone(ConeSurface::new(
                point()?,
                frame(&v[3..6], &v[9..12])?,
                NonNegativeLength::new(length(v[6]))?,
                PositiveReal::ONE,
                half_angle,
            )),
            false,
        ),
        SPHERE => surface(
            SolvedSurfaceGeometry::Sphere(SphereSurface::new(
                point()?,
                frame(&v[4..7], &v[7..10])?,
                NonZeroLength::from(PositiveLength::new(length(v[3]))?),
            )),
            false,
        ),
        TORUS => surface(
            SolvedSurfaceGeometry::Torus(TorusSurface::new(
                point()?,
                frame(&v[3..6], &v[8..11])?,
                PositiveLength::new(length(v[6].abs()))?,
                NonZeroLength::from(PositiveLength::new(length(v[7]))?),
            )),
            v[6].is_sign_negative(),
        ),
        _ => None,
    }
}

fn vector(v: &[f64]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

/// The frame of a stored axis and reference vector, kept as written.
fn frame(axis: &[f64], reference: &[f64]) -> Option<OrthonormalFrame3> {
    OrthonormalFrame3::new(vector(axis), vector(reference))
}

#[cfg(test)]
mod tests;
