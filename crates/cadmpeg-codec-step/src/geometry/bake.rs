// SPDX-License-Identifier: Apache-2.0
//! Places a carrier under a rigid placement chain directly in its own
//! coordinates, so the writer emits the placed carrier instead of a
//! `CURVE_REPLICA` or `SURFACE_REPLICA` over its basis.
//!
//! The placed carrier has the parameterization of the replica it replaces: an
//! affine map keeps that of a NURBS carrier, a line pcurve, a polyline and a
//! point, and a proper rigid motion keeps that of every analytic carrier. Any
//! other placement is not made here, and the writer keeps the replica.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    analytic::{
        CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, HyperbolaCurve,
        LineCurve, ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
    },
    pcurve::PcurveGeometry,
    pcurve::{CirclePcurve, EllipsePcurve, HyperbolaPcurve, LinePcurve, ParabolaPcurve},
    SolvedCurveGeometry, SolvedSurfaceGeometry,
};
use cadmpeg_ir::transform::{Transform, Transform2};
use cadmpeg_ir::units::{FinitePoint2, NonzeroPoint2, OrthonormalFrame3, UnitVector3};

/// Tolerance of the two-dimensional proper rigid test, the one
/// [`Transform::is_proper_rigid`] uses in three dimensions.
const EPS_PROPER_RIGID_2D: f64 = 1.0e-9;

/// The composition of `placements`, outermost first.
pub(super) fn chain(placements: &[&Transform]) -> Option<Transform> {
    let mut composed = Transform::identity();
    for transform in placements {
        composed = composed.compose(**transform).ok()?;
    }
    Some(composed)
}

fn placed_point(transform: Transform, point: FinitePoint3) -> Option<FinitePoint3> {
    transform.apply_point(point.get())
}

fn placed_frame(transform: Transform, frame: OrthonormalFrame3) -> Option<OrthonormalFrame3> {
    let axis = transform.apply_vector(*frame.axis().as_raw())?.get();
    let reference = transform.apply_vector(*frame.reference().as_raw())?.get();
    OrthonormalFrame3::new(axis, reference)
}

/// `basis` placed by `transform`.
///
/// An affine map keeps the parameterization of a NURBS curve, a polyline and a
/// point, so each is placed under any finite `transform`. An analytic carrier
/// is written from a unit frame and its radii, so it is placed only under a
/// proper rigid motion.
pub(super) fn placed_curve(
    transform: Transform,
    basis: &SolvedCurveGeometry,
) -> Option<SolvedCurveGeometry> {
    let analytic = matches!(
        basis,
        SolvedCurveGeometry::Line(_)
            | SolvedCurveGeometry::Circle(_)
            | SolvedCurveGeometry::Ellipse(_)
            | SolvedCurveGeometry::Parabola(_)
            | SolvedCurveGeometry::Hyperbola(_)
    );
    if analytic && !transform.is_proper_rigid() {
        return None;
    }
    Some(match basis {
        SolvedCurveGeometry::Line(line) => SolvedCurveGeometry::Line(LineCurve::new(
            placed_point(transform, line.origin())?,
            UnitVector3::new(transform.apply_vector(*line.direction().as_raw())?.get())?,
        )),
        SolvedCurveGeometry::Circle(circle) => SolvedCurveGeometry::Circle(CircleCurve::new(
            placed_point(transform, circle.center())?,
            placed_frame(transform, *circle.frame())?,
            circle.radius(),
        )),
        SolvedCurveGeometry::Ellipse(ellipse) => SolvedCurveGeometry::Ellipse(
            EllipseCurve::try_from_parts(
                placed_point(transform, ellipse.center())?,
                placed_frame(transform, *ellipse.frame())?,
                ellipse.major_radius(),
                ellipse.minor_radius(),
            )
            .ok()?,
        ),
        SolvedCurveGeometry::Parabola(parabola) => {
            SolvedCurveGeometry::Parabola(ParabolaCurve::new(
                placed_point(transform, parabola.vertex())?,
                placed_frame(transform, *parabola.frame())?,
                parabola.focal_distance(),
            ))
        }
        SolvedCurveGeometry::Hyperbola(hyperbola) => {
            SolvedCurveGeometry::Hyperbola(HyperbolaCurve::new(
                placed_point(transform, hyperbola.center())?,
                placed_frame(transform, *hyperbola.frame())?,
                hyperbola.major_radius(),
                hyperbola.minor_radius(),
            ))
        }
        SolvedCurveGeometry::Degenerate(degenerate) => SolvedCurveGeometry::Degenerate(
            DegenerateCurve::new(placed_point(transform, degenerate.point())?),
        ),
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let mut nurbs = nurbs.clone();
            nurbs
                .try_map_control_points(|_, point| transform.apply_point(point.get()).ok_or(()))
                .ok()?;
            SolvedCurveGeometry::Nurbs(nurbs)
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let mut polyline = polyline.clone();
            polyline
                .edit_samples(|samples| {
                    samples.edit_points(|point| {
                        *point = transform
                            .apply_point(*point)
                            .ok_or_else(|| {
                                cadmpeg_ir::geometry::sampled::GeometryLayoutError::EditRefused(
                                    "a placed polyline point is not finite".into(),
                                )
                            })?
                            .get();
                        Ok(())
                    })
                })
                .ok()?;
            SolvedCurveGeometry::Polyline(polyline)
        }
        SolvedCurveGeometry::Transformed(_)
        | SolvedCurveGeometry::Composite { .. }
        | SolvedCurveGeometry::Unknown { .. } => return None,
    })
}

/// `basis` placed by `transform`.
///
/// A NURBS surface is placed under any finite `transform`; an analytic surface
/// only under a proper rigid motion, as [`placed_curve`] places curves.
pub(super) fn placed_surface(
    transform: Transform,
    basis: &SolvedSurfaceGeometry,
) -> Option<SolvedSurfaceGeometry> {
    if !matches!(basis, SolvedSurfaceGeometry::Nurbs(_)) && !transform.is_proper_rigid() {
        return None;
    }
    Some(match basis {
        SolvedSurfaceGeometry::Plane(plane) => SolvedSurfaceGeometry::Plane(PlaneSurface::new(
            placed_point(transform, plane.origin())?,
            placed_frame(transform, *plane.frame())?,
        )),
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            SolvedSurfaceGeometry::Cylinder(CylinderSurface::new(
                placed_point(transform, cylinder.origin())?,
                placed_frame(transform, *cylinder.frame())?,
                cylinder.radius(),
            ))
        }
        SolvedSurfaceGeometry::Cone(cone) => SolvedSurfaceGeometry::Cone(ConeSurface::new(
            placed_point(transform, cone.origin())?,
            placed_frame(transform, *cone.frame())?,
            cone.radius(),
            cone.ratio(),
            cone.half_angle(),
        )),
        SolvedSurfaceGeometry::Sphere(sphere) => SolvedSurfaceGeometry::Sphere(SphereSurface::new(
            placed_point(transform, sphere.center())?,
            placed_frame(transform, *sphere.frame())?,
            sphere.radius(),
        )),
        SolvedSurfaceGeometry::Torus(torus) => SolvedSurfaceGeometry::Torus(TorusSurface::new(
            placed_point(transform, torus.center())?,
            placed_frame(transform, *torus.frame())?,
            torus.major_radius(),
            torus.minor_radius(),
        )),
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let mut nurbs = nurbs.clone();
            nurbs
                .try_map_control_points(|_, point| transform.apply_point(point.get()).ok_or(()))
                .ok()?;
            SolvedSurfaceGeometry::Nurbs(nurbs)
        }
        SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Unknown { .. } => return None,
    })
}

/// Whether `transform` is a finite, right-handed rigid motion of the plane.
fn is_proper_rigid_2d(transform: Transform2) -> bool {
    let [[a, b, _], [c, d, _]] = transform.affine_rows();
    (a * a + c * c - 1.0).abs() <= EPS_PROPER_RIGID_2D
        && (b * b + d * d - 1.0).abs() <= EPS_PROPER_RIGID_2D
        && (a * b + c * d).abs() <= EPS_PROPER_RIGID_2D
        && (a * d - b * c - 1.0).abs() <= EPS_PROPER_RIGID_2D
}

/// `outer * inner` for column-vector application.
fn compose_2d(outer: Transform2, inner: Transform2) -> Option<Transform2> {
    let left = outer.rows();
    let right = inner.rows();
    let entry = |row: usize, column: usize| {
        (0..3)
            .map(|inner| left[row][inner] * right[inner][column])
            .sum::<f64>()
    };
    Transform2::affine([
        [entry(0, 0), entry(0, 1), entry(0, 2)],
        [entry(1, 0), entry(1, 1), entry(1, 2)],
    ])
}

fn placed_point_2d(transform: Transform2, point: FinitePoint2) -> Option<FinitePoint2> {
    FinitePoint2::new(transform.apply_point(point.get()))
}

fn placed_axis_2d(transform: Transform2, axis: FinitePoint2) -> Option<FinitePoint2> {
    FinitePoint2::new(transform.apply_vector(axis.get()))
}

/// `geometry` placed by `transform`, with nested placements composed into it.
///
/// An affine map keeps the parameterization of a line and of a NURBS curve, so
/// either is placed under any finite `transform`. A conic is written from its
/// center, first axis and radii, so it is placed only under a proper rigid
/// motion.
pub(super) fn placed_pcurve(
    transform: Transform2,
    geometry: &PcurveGeometry,
) -> Option<PcurveGeometry> {
    let center = |point: &FinitePoint2| placed_point_2d(transform, *point);
    let axis = |point: &FinitePoint2| placed_axis_2d(transform, *point);
    let conic = is_proper_rigid_2d(transform);
    Some(match geometry {
        PcurveGeometry::Transformed(placed) => {
            return placed_pcurve(compose_2d(transform, *placed.transform())?, placed.basis());
        }
        PcurveGeometry::Line(line) => PcurveGeometry::Line(LinePcurve::new(
            center(line.origin())?,
            NonzeroPoint2::new(transform.apply_vector(*line.direction().as_raw()))?,
        )),
        PcurveGeometry::Nurbs { nurbs } => {
            let mut nurbs = nurbs.clone();
            nurbs
                .try_map_control_points(|_, point| placed_point_2d(transform, point).ok_or(()))
                .ok()?;
            PcurveGeometry::Nurbs { nurbs }
        }
        PcurveGeometry::Circle(circle) if conic => {
            PcurveGeometry::Circle(CirclePcurve::from_parts(
                center(circle.center())?,
                axis(circle.x_axis())?,
                axis(circle.y_axis())?,
                circle.radius(),
            )?)
        }
        PcurveGeometry::Ellipse(ellipse) if conic => {
            PcurveGeometry::Ellipse(EllipsePcurve::from_parts(
                center(ellipse.center())?,
                axis(ellipse.x_axis())?,
                axis(ellipse.y_axis())?,
                ellipse.major_radius(),
                ellipse.minor_radius(),
            )?)
        }
        PcurveGeometry::Parabola(parabola) if conic => {
            PcurveGeometry::Parabola(ParabolaPcurve::from_parts(
                center(parabola.vertex())?,
                axis(parabola.x_axis())?,
                axis(parabola.y_axis())?,
                parabola.focal_distance(),
            )?)
        }
        PcurveGeometry::Hyperbola(hyperbola) if conic => {
            PcurveGeometry::Hyperbola(HyperbolaPcurve::from_parts(
                center(hyperbola.center())?,
                axis(hyperbola.x_axis())?,
                axis(hyperbola.y_axis())?,
                hyperbola.major_radius(),
                hyperbola.minor_radius(),
            )?)
        }
        _ => return None,
    })
}

/// The half-turn of the parameter plane, `(u, v)` to `(-u, -v)`.
pub(super) fn half_turn() -> Option<Transform2> {
    Transform2::affine([[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]])
}
