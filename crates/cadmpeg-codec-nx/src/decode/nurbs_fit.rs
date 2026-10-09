// SPDX-License-Identifier: Apache-2.0
//! Certified local NURBS fits before complete surface inversion.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, WorkBudget};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::{finite_or_refusal, EvaluationFailure};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::units::FinitePoint2;

/// Fit a stored support or contact point. A local branch needs only a forward
/// residual certificate; a failed local fit retains the complete search.
pub(super) fn fit_nurbs_surface_parameter(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
    budget: &WorkBudget<'_>,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    let local = fit_nurbs_surface_parameter_locally(ctx, surface, point, seed, tolerance, budget)?;
    if local.is_some() {
        return Ok(local);
    }
    cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget(
        ctx, surface, point, seed, tolerance, budget,
    )
}

/// Return a forward-certified local candidate without starting a complete
/// search. Callers with several stored seeds can try all local branches first.
pub(super) fn fit_nurbs_surface_parameter_locally(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Option<Point2>,
    tolerance: f64,
    budget: &WorkBudget<'_>,
) -> Result<Option<FinitePoint2>, ResourceLimit> {
    ctx.charge_work_limit(0, "NX NURBS fit boundary")?;
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Ok(None);
    }
    finite_or_refusal(
        EvaluationAdmission::Decode(ctx).within_work_slice(budget, |admission| {
            // Keep the stored-seed fast path. Newton derivatives are not
            // needed when the seed already supplies a certified point.
            if let Some(seed) = seed.and_then(FinitePoint2::new) {
                let u_degree =
                    usize::try_from(surface.u_degree()).map_err(|_| EvaluationFailure::NoValue)?;
                let v_degree =
                    usize::try_from(surface.v_degree()).map_err(|_| EvaluationFailure::NoValue)?;
                let u_start = *surface
                    .u_knots()
                    .get(u_degree)
                    .ok_or(EvaluationFailure::NoValue)?;
                let u_end = *surface
                    .u_knots()
                    .get(surface.u_count())
                    .ok_or(EvaluationFailure::NoValue)?;
                let v_start = *surface
                    .v_knots()
                    .get(v_degree)
                    .ok_or(EvaluationFailure::NoValue)?;
                let v_end = *surface
                    .v_knots()
                    .get(surface.v_count())
                    .ok_or(EvaluationFailure::NoValue)?;
                if ![u_start, u_end, v_start, v_end]
                    .into_iter()
                    .all(f64::is_finite)
                    || u_start >= u_end
                    || v_start >= v_end
                {
                    return Err(EvaluationFailure::NoValue);
                }
                let seed = seed.get();
                let parameters = FinitePoint2::new(Point2::new(
                    seed.u.clamp(u_start, u_end),
                    seed.v.clamp(v_start, v_end),
                ))
                .ok_or(EvaluationFailure::NoValue)?;
                let image = cadmpeg_ir::eval::decode::nurbs_surface_point(
                    admission,
                    surface,
                    parameters.u,
                    parameters.v,
                )?;
                if Point3::distance(image.get(), point) <= tolerance {
                    return Ok(parameters);
                }
            }
            let parameters = cadmpeg_ir::eval::nurbs_surface_parameter_near_point(
                admission, surface, point, seed,
            )
            .map_err(EvaluationFailure::ResourceLimit)?
            .ok_or(EvaluationFailure::NoValue)?;
            let image = cadmpeg_ir::eval::decode::nurbs_surface_point(
                admission,
                surface,
                parameters.u,
                parameters.v,
            )?;
            if Point3::distance(image.get(), point) <= tolerance {
                Ok(parameters)
            } else {
                Err(EvaluationFailure::NoValue)
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{fit_nurbs_surface_parameter, fit_nurbs_surface_parameter_locally};
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::{Point2, Point3};

    const EPS_FIT: f64 = 1.0e-10;

    fn surface(u_knots: Vec<f64>, x_coordinates: &[f64]) -> NurbsSurface {
        NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, u_knots, false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                x_coordinates
                    .iter()
                    .map(|&x| vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)])
                    .collect(),
                None,
            ),
            false,
        )
        .expect("fixture storage")
        .expect("valid surface")
    }

    #[test]
    fn local_fit_certifies_an_unseeded_point_in_a_small_work_slice() {
        let surface = surface(vec![0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]);
        crate::test_support::with_decode_context(|ctx| {
            let budget = ctx.work_budget(10_000);
            let parameters = fit_nurbs_surface_parameter(
                ctx,
                &surface,
                Point3::new(0.3, 0.7, 0.0),
                None,
                EPS_FIT,
                &budget,
            )
            .expect("bounded search")
            .expect("forward-certified point");
            assert!((parameters.u - 0.3).abs() <= EPS_FIT);
            assert!((parameters.v - 0.7).abs() <= EPS_FIT);
            assert!(budget.consumed() > 0);
            assert!(budget.remaining() > 0);
        });
    }

    #[test]
    fn stored_fit_seed_does_not_require_newton_derivatives() {
        let surface = surface(vec![0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]);
        crate::test_support::with_decode_context(|ctx| {
            let budget = ctx.work_budget(128);
            let parameters = fit_nurbs_surface_parameter(
                ctx,
                &surface,
                Point3::new(0.3, 0.7, 0.0),
                Some(Point2::new(0.3, 0.7)),
                EPS_FIT,
                &budget,
            )
            .expect("point evaluation fits the slice")
            .expect("stored seed fits");
            assert_eq!(parameters.get(), Point2::new(0.3, 0.7));
        });
    }

    #[test]
    fn failed_local_branch_keeps_the_complete_search() {
        let surface = surface(vec![0.0, 0.0, 0.5, 1.0, 1.0], &[0.0, 0.0, 1.0]);
        crate::test_support::with_decode_context(|ctx| {
            let budget = ctx.work_budget(2_000_000);
            let parameters = fit_nurbs_surface_parameter(
                ctx,
                &surface,
                Point3::new(0.8, 0.3, 0.0),
                Some(Point2::new(0.1, 0.3)),
                EPS_FIT,
                &budget,
            )
            .expect("bounded search")
            .expect("complete search reaches the nondegenerate span");
            assert!((parameters.u - 0.9).abs() <= EPS_FIT);
            assert!((parameters.v - 0.3).abs() <= EPS_FIT);
        });
    }

    #[test]
    fn failed_stored_seed_leaves_work_for_other_local_branches() {
        let surface = surface(vec![0.0, 0.0, 0.5, 1.0, 1.0], &[0.0, 0.0, 1.0]);
        crate::test_support::with_decode_context(|ctx| {
            let budget = ctx.work_budget(10_000);
            assert!(fit_nurbs_surface_parameter_locally(
                ctx,
                &surface,
                Point3::new(0.8, 0.3, 0.0),
                Some(Point2::new(0.1, 0.3)),
                EPS_FIT,
                &budget,
            )
            .expect("local branch stays bounded")
            .is_none());
            assert!(fit_nurbs_surface_parameter_locally(
                ctx,
                &surface,
                Point3::new(0.8, 0.3, 0.0),
                Some(Point2::new(0.9, 0.3)),
                EPS_FIT,
                &budget,
            )
            .expect("later stored seed can still be tried")
            .is_some());
        });
    }

    #[test]
    fn local_inverse_without_a_fit_certificate_is_rejected() {
        let surface = surface(vec![0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]);
        crate::test_support::with_decode_context(|ctx| {
            let budget = ctx.work_budget(2_000_000);
            assert!(fit_nurbs_surface_parameter(
                ctx,
                &surface,
                Point3::new(0.3, 0.7, 0.2),
                Some(Point2::new(0.3, 0.7)),
                0.1,
                &budget,
            )
            .expect("bounded search")
            .is_none());
        });
    }

    #[test]
    fn local_fit_propagates_the_original_session_refusal() {
        let surface = surface(vec![0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]);
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let budget = ctx.work_budget(10_000);
                let refusal = fit_nurbs_surface_parameter(
                    ctx,
                    &surface,
                    Point3::new(0.3, 0.7, 0.0),
                    None,
                    EPS_FIT,
                    &budget,
                )
                .expect_err("work remains mandatory");
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(ctx.resource_refusal(), Some(refusal));
            },
        );
    }

    #[test]
    fn invalid_fit_tolerance_keeps_an_existing_session_refusal() {
        let surface = surface(vec![0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]);
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let original = ctx
                    .charge_work_limit(1, "synthetic preceding operation")
                    .expect_err("establish the session refusal");
                let budget = ctx.work_budget(10_000);
                for tolerance in [-1.0, f64::NAN] {
                    assert_eq!(
                        fit_nurbs_surface_parameter(
                            ctx,
                            &surface,
                            Point3::new(0.3, 0.7, 0.0),
                            None,
                            tolerance,
                            &budget,
                        )
                        .expect_err("invalid tolerance does not absorb the refusal"),
                        original,
                    );
                }
            },
        );
    }
}
