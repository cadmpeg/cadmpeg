// SPDX-License-Identifier: Apache-2.0
//! Caller admission for stored carrier evaluation scratch.

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::CurveGeometry;

pub(super) fn curve_point(
    ctx: &DecodeContext<'_>,
    curve: &CurveGeometry,
    parameter: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
            ctx, curve, parameter,
        ))?,
    )?)
}

/// A pcurve carrier's surface parameters at `parameter`, absent where the
/// carrier states none or the evaluation leaves the finite range.
pub(super) fn pcurve_uv(
    ctx: &DecodeContext<'_>,
    pcurve: &cadmpeg_ir::geometry::pcurve::PcurveGeometry,
    parameter: f64,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::pcurve_uv(
            ctx, pcurve, parameter,
        ))?,
    )?)
}

pub(super) fn nurbs_curve_point(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    parameter: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            ctx, curve, parameter,
        ))?,
    )?)
}

pub(crate) fn nurbs_surface_point(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    u: f64,
    v: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::nurbs_surface_point(
            ctx, surface, u, v,
        ))?,
    )?)
}

pub(crate) fn nurbs_surface_parameter_near_point(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    point: cadmpeg_ir::math::Point3,
    seed: Option<cadmpeg_ir::math::Point2>,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    Ok(cadmpeg_ir::eval::nurbs_surface_parameter_near_point(
        ctx, surface, point, seed,
    )?)
}

pub(crate) fn nurbs_surface_partials(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    u: f64,
    v: f64,
) -> Result<
    Option<cadmpeg_ir::eval::SurfacePartials<FinitePoint3, cadmpeg_ir::features::FiniteVector3>>,
    CodecError,
> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::nurbs_surface_partials(ctx, surface, u, v),
    )?)
}

/// The point and first partials of a surface carrier at `(u, v)`, or `None`
/// where they have no finite value.
///
/// # Errors
///
/// Propagates the evaluator's resource refusal.
pub(super) fn surface_partials(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<
    Option<cadmpeg_ir::eval::SurfacePartials<FinitePoint3, cadmpeg_ir::features::FiniteVector3>>,
    cadmpeg_core::decode::ResourceLimit,
> {
    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::surface_partials(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
        surface,
        u,
        v,
    ))
}

/// The point and first partials of an analytic surface in closed form, or
/// `None` for any other surface or a non-finite value.
fn analytic_first_partials(
    surface: &cadmpeg_ir::geometry::SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<cadmpeg_ir::eval::SurfacePartials<FinitePoint3, cadmpeg_ir::features::FiniteVector3>> {
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry as S;
    use cadmpeg_ir::math::{Point3, Vector3};
    let combine = |terms: &[(f64, Vector3)]| {
        terms
            .iter()
            .fold(Vector3::new(0.0, 0.0, 0.0), |sum, (factor, vector)| {
                Vector3::new(
                    factor.mul_add(vector.x, sum.x),
                    factor.mul_add(vector.y, sum.y),
                    factor.mul_add(vector.z, sum.z),
                )
            })
    };
    let (base, point, du, dv): (Point3, Vector3, Vector3, Vector3) = match surface {
        S::Plane(plane) => {
            let normal = *plane.frame().axis().as_raw();
            let u_axis = *plane.frame().reference().as_raw();
            let v_axis = normal.cross(u_axis);
            (
                plane.origin().get(),
                combine(&[(u, u_axis), (v, v_axis)]),
                u_axis,
                v_axis,
            )
        }
        S::Cylinder(cylinder) => {
            let axis = *cylinder.frame().axis().as_raw();
            let reference = *cylinder.frame().reference().as_raw();
            let transverse = axis.cross(reference);
            let radius = cylinder.radius().get();
            let (sine, cosine) = u.sin_cos();
            (
                cylinder.origin().get(),
                combine(&[
                    (radius * cosine, reference),
                    (radius * sine, transverse),
                    (v, axis),
                ]),
                combine(&[(-radius * sine, reference), (radius * cosine, transverse)]),
                axis,
            )
        }
        S::Cone(cone) => {
            let axis = *cone.frame().axis().as_raw();
            let reference = *cone.frame().reference().as_raw();
            let transverse = axis.cross(reference);
            let ratio = cone.ratio().get();
            let slope = cone.half_angle().get().tan();
            let local = slope.mul_add(v, cone.radius().get());
            let (sine, cosine) = u.sin_cos();
            (
                cone.origin().get(),
                combine(&[
                    (local * cosine, reference),
                    (local * ratio * sine, transverse),
                    (v, axis),
                ]),
                combine(&[
                    (-local * sine, reference),
                    (local * ratio * cosine, transverse),
                ]),
                combine(&[
                    (slope * cosine, reference),
                    (slope * ratio * sine, transverse),
                    (1.0, axis),
                ]),
            )
        }
        S::Sphere(sphere) => {
            let axis = *sphere.frame().axis().as_raw();
            let reference = *sphere.frame().reference().as_raw();
            let transverse = axis.cross(reference);
            let radius = sphere.radius().get();
            let (u_sine, u_cosine) = u.sin_cos();
            let (v_sine, v_cosine) = v.sin_cos();
            (
                sphere.center().get(),
                combine(&[
                    (radius * v_cosine * u_cosine, reference),
                    (radius * v_cosine * u_sine, transverse),
                    (radius * v_sine, axis),
                ]),
                combine(&[
                    (-radius * v_cosine * u_sine, reference),
                    (radius * v_cosine * u_cosine, transverse),
                ]),
                combine(&[
                    (-radius * v_sine * u_cosine, reference),
                    (-radius * v_sine * u_sine, transverse),
                    (radius * v_cosine, axis),
                ]),
            )
        }
        S::Torus(torus) => {
            let axis = *torus.frame().axis().as_raw();
            let reference = *torus.frame().reference().as_raw();
            let transverse = axis.cross(reference);
            let minor = torus.minor_radius().get();
            let (u_sine, u_cosine) = u.sin_cos();
            let (v_sine, v_cosine) = v.sin_cos();
            let ring = minor.mul_add(v_cosine, torus.major_radius().get());
            (
                torus.center().get(),
                combine(&[
                    (ring * u_cosine, reference),
                    (ring * u_sine, transverse),
                    (minor * v_sine, axis),
                ]),
                combine(&[(-ring * u_sine, reference), (ring * u_cosine, transverse)]),
                combine(&[
                    (-minor * v_sine * u_cosine, reference),
                    (-minor * v_sine * u_sine, transverse),
                    (minor * v_cosine, axis),
                ]),
            )
        }
        _ => return None,
    };
    let point = Point3::new(base.x + point.x, base.y + point.y, base.z + point.z);
    Some(cadmpeg_ir::eval::SurfacePartials {
        point: FinitePoint3::new(point)?,
        du: cadmpeg_ir::features::FiniteVector3::new(du)?,
        dv: cadmpeg_ir::features::FiniteVector3::new(dv)?,
    })
}

/// Steps of the nearest-point solve on an analytic surface.
const MAX_FOOT_STEPS: usize = 16;
/// Model-space step at which the nearest-point solve has converged.
const FOOT_STEP_TOLERANCE_MM: f64 = 1.0e-12;
/// Step, relative to the point's largest coordinate, at which a plain
/// floating-point solve has reached rounding.
pub(super) const ROUNDING_RELATIVE: f64 = 16.0 * f64::EPSILON;

/// The point of an analytic surface nearest `point` and the unit natural
/// normal there: the surface's own parameter inverse refined by Gauss-Newton
/// steps on the squared distance, or `None` where the surface has no
/// inverse or the steps do not converge.
pub(super) fn analytic_surface_foot(
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    point: cadmpeg_ir::math::Point3,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(solved) = surface else {
        return None;
    };
    let parameters = cadmpeg_ir::eval::analytic_surface_parameters(surface, point)?;
    let tolerance = FOOT_STEP_TOLERANCE_MM.max(rounding_scale(point));
    let [mut u, mut v] = [parameters.get().u, parameters.get().v];
    for _ in 0..MAX_FOOT_STEPS {
        let partials = analytic_first_partials(solved, u, v)?;
        let (at, du, dv) = (partials.point.get(), partials.du.get(), partials.dv.get());
        let residual = point.vector_from(at);
        let (uu, uv, vv) = (du.dot(du), du.dot(dv), dv.dot(dv));
        let determinant = uu * vv - uv * uv;
        let normal = du.cross(dv);
        let length = normal.norm();
        if !(determinant > 0.0 && determinant.is_finite() && length > 0.0 && length.is_finite()) {
            return None;
        }
        let (ru, rv) = (du.dot(residual), dv.dot(residual));
        let step_u = (ru * vv - rv * uv) / determinant;
        let step_v = (rv * uu - ru * uv) / determinant;
        u += step_u;
        v += step_v;
        let moved = (du.x * step_u + dv.x * step_v)
            .hypot(du.y * step_u + dv.y * step_v)
            .hypot(du.z * step_u + dv.z * step_v);
        if moved <= tolerance {
            return Some((
                at,
                cadmpeg_ir::math::Vector3::new(
                    normal.x / length,
                    normal.y / length,
                    normal.z / length,
                ),
            ));
        }
    }
    None
}

/// The rounding of a plain floating-point step near `point`.
pub(super) fn rounding_scale(point: cadmpeg_ir::math::Point3) -> f64 {
    ROUNDING_RELATIVE * point.x.abs().max(point.y.abs()).max(point.z.abs())
}

pub(crate) fn surface_point(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<Option<FinitePoint3>, CodecError> {
    Ok(cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::surface_point(
            ctx, surface, u, v,
        ))?,
    )?)
}

/// Work one NURBS surface solve may spend before its pcurve is skipped.
pub(crate) const SURFACE_SOLVER_LOCAL_WORK: usize = 1_000_000;

/// Run one budgeted solve. An exhausted budget sets `capped` and answers
/// `None`; the caller names the skipped entity.
fn with_surface_solver_work<T>(
    work: usize,
    capped: &mut bool,
    run: impl FnOnce(&WorkBudget) -> Result<Option<T>, CodecError>,
) -> Result<Option<T>, CodecError> {
    let budget = WorkBudget::new(work);
    let result = run(&budget);
    if budget.exhausted() {
        *capped = true;
        return Ok(None);
    }
    result
}

pub(super) fn nurbs_surface_parameter_within_tolerance(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    point: cadmpeg_ir::math::Point3,
    seed: Option<cadmpeg_ir::math::Point2>,
    tolerance: f64,
    capped: &mut bool,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    with_surface_solver_work(SURFACE_SOLVER_LOCAL_WORK, capped, |budget| {
        Ok(
            cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget(
                ctx, surface, point, seed, tolerance, budget,
            )?,
        )
    })
}

pub(super) fn nurbs_surface_parameter_segment_chord_bound(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    parameters: [cadmpeg_ir::math::Point2; 2],
    chord: [cadmpeg_ir::math::Point3; 2],
    capped: &mut bool,
) -> Result<Option<f64>, CodecError> {
    with_surface_solver_work(SURFACE_SOLVER_LOCAL_WORK, capped, |budget| {
        cadmpeg_ir::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
            ctx, surface, parameters, chord, budget,
        )
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::Point3;

    /// A dense wavy cubic patch grid whose global inversion of an off-surface
    /// point needs more than the local solver slice.
    fn dense_wavy_surface() -> NurbsSurface {
        const COUNT: u32 = 80;
        let mut knots = vec![0.0; 4];
        knots.extend((1..COUNT - 3).map(f64::from));
        knots.extend([f64::from(COUNT - 3); 4]);
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(3, knots.clone(), false),
            NurbsSurfaceAxis::new(3, knots, false),
            NurbsSurfaceLanes::new(
                (0..COUNT)
                    .map(|u| {
                        (0..COUNT)
                            .map(|v| {
                                let (u, v) = (f64::from(u), f64::from(v));
                                Point3::new(u, v, (u * 0.7).sin() * (v * 0.9).cos())
                            })
                            .collect()
                    })
                    .collect(),
                None,
            ),
            false,
        )
        .expect("dense cubic surface")
    }

    #[test]
    fn a_surface_foot_converges_far_from_the_origin() {
        use cadmpeg_ir::geometry::analytic::CylinderSurface;
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
        use cadmpeg_ir::math::Vector3;
        // A cylinder of radius 2 about the z axis through (5000, 5000, 5000);
        // the point 3 mm from the axis along x has its foot 2 mm from it.
        let origin = Point3::new(5000.0, 5000.0, 5000.0);
        let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                origin,
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        ));
        let point = Point3::new(5003.0, 5000.0, 5001.0);
        let (foot, normal) =
            super::analytic_surface_foot(&cylinder, point).expect("the foot converges");
        assert!(foot.distance(Point3::new(5002.0, 5000.0, 5001.0)) < 1e-9);
        assert!((normal.x - 1.0).abs() < 1e-12);
    }

    #[test]
    fn analytic_first_partials_agree_with_the_ir_evaluator() {
        use cadmpeg_ir::geometry::analytic::{
            ConeSurface, CylinderSurface, PlaneSurface, SphereSurface, TorusSurface,
        };
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry as S, SurfaceGeometry};
        use cadmpeg_ir::math::Vector3;
        let origin = Point3::new(1.5, -2.25, 0.75);
        let axis = Vector3::new(0.0, 0.6, 0.8);
        let reference = Vector3::new(1.0, 0.0, 0.0);
        let surfaces = [
            S::Plane(PlaneSurface::try_new(origin, axis, reference).unwrap()),
            S::Cylinder(CylinderSurface::try_new(origin, axis, reference, 2.5).unwrap()),
            S::Cone(ConeSurface::try_new(origin, axis, reference, 2.5, 0.75, 0.4).unwrap()),
            S::Sphere(SphereSurface::try_new(origin, axis, reference, 2.5).unwrap()),
            S::Torus(TorusSurface::try_new(origin, axis, reference, 7.0, 1.25).unwrap()),
        ];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for surface in surfaces {
            for (u, v) in [(0.0, 0.0), (0.7, -0.3), (2.9, 1.1), (-4.0, 0.45)] {
                let fast = super::analytic_first_partials(&surface, u, v)
                    .unwrap()
                    .into_raw();
                let exact = cadmpeg_ir::eval::surface_partials(
                    cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(&ctx),
                    &SurfaceGeometry::Solved(surface.clone()),
                    u,
                    v,
                )
                .unwrap()
                .into_raw();
                let gap = |left: Vector3, right: Vector3| (left - right).norm();
                assert!(fast.point.distance(exact.point) < 1e-12);
                assert!(gap(fast.du, exact.du) < 1e-12);
                assert!(gap(fast.dv, exact.dv) < 1e-12);
            }
        }
    }

    #[test]
    fn surface_inversion_skips_when_the_local_solver_slice_is_exhausted() {
        let surface = dense_wavy_surface();
        let point = Point3::new(40.5, 40.5, 50.0);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let budget = WorkBudget::new(super::SURFACE_SOLVER_LOCAL_WORK);
        let solved = cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget(
            &ctx, &surface, point, None, 1.0e-3, &budget,
        );
        assert!(
            solved.is_ok(),
            "the session admits the local slice: {solved:?}"
        );
        assert!(
            budget.exhausted(),
            "the fixture must exhaust the local slice"
        );

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut capped = false;
        assert!(super::nurbs_surface_parameter_within_tolerance(
            &ctx,
            &surface,
            point,
            None,
            1.0e-3,
            &mut capped
        )
        .unwrap()
        .is_none());
        assert!(capped);
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn surface_chord_bound_skips_when_the_local_solver_slice_is_exhausted() {
        let surface = dense_wavy_surface();
        let parameters = [
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(76.0, 76.0),
        ];
        let chord = [
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(76.0, 76.0, 50.0),
        ];
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut capped = false;
        assert!(super::with_surface_solver_work(0, &mut capped, |budget| {
            cadmpeg_ir::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
                &ctx, &surface, parameters, chord, budget,
            )
        })
        .unwrap()
        .is_none());
        assert!(capped);
        assert!(ctx.finish_session().is_ok());
    }
}
