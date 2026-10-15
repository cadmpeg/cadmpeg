// SPDX-License-Identifier: Apache-2.0
//! Caller admission for stored carrier evaluation scratch.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
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

const SURFACE_SOLVER_LOCAL_WORK: u64 = 1_000_000;

fn surface_solver_budget<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    operation: &'static str,
) -> Result<cadmpeg_core::decode::WorkBudget<'ctx>, CodecError> {
    // One normalized evaluator unit also pays for fixed arithmetic and a
    // complete scan of both knot vectors. The local ceiling bounds solver work.
    let scale = 64_u64
        .checked_add(u64_from_index(surface.u_knots().len()))
        .and_then(|scale| scale.checked_add(u64_from_index(surface.v_knots().len())))
        .and_then(std::num::NonZeroU64::new)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    Ok(ctx
        .work_budget(SURFACE_SOLVER_LOCAL_WORK)
        .with_session_work_scale(scale))
}

pub(super) fn nurbs_surface_parameter_within_tolerance(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    point: cadmpeg_ir::math::Point3,
    seed: Option<cadmpeg_ir::math::Point2>,
    tolerance: f64,
) -> Result<Option<cadmpeg_ir::units::FinitePoint2>, CodecError> {
    const OPERATION: &str = "invert SLDPRT NURBS surface globally";
    let budget = surface_solver_budget(ctx, surface, OPERATION)?;
    let result = cadmpeg_ir::eval::nurbs_surface_parameter_within_tolerance_with_budget(
        ctx, surface, point, seed, tolerance, &budget,
    );
    if budget.exhausted() {
        return Err(ctx.refuse_codec_limit(
            OPERATION,
            SURFACE_SOLVER_LOCAL_WORK,
            SURFACE_SOLVER_LOCAL_WORK + 1,
        ));
    }
    Ok(result?)
}

pub(super) fn nurbs_surface_parameter_segment_chord_bound(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    parameters: [cadmpeg_ir::math::Point2; 2],
    chord: [cadmpeg_ir::math::Point3; 2],
) -> Result<Option<f64>, CodecError> {
    const OPERATION: &str = "certify SLDPRT NURBS surface chord";
    let budget = surface_solver_budget(ctx, surface, OPERATION)?;
    let result = cadmpeg_ir::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
        ctx, surface, parameters, chord, &budget,
    );
    if budget.exhausted() {
        return Err(ctx.refuse_codec_limit(
            OPERATION,
            SURFACE_SOLVER_LOCAL_WORK,
            SURFACE_SOLVER_LOCAL_WORK + 1,
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::Point3;

    const EPS_PLANAR_CHORD: f64 = 1e-10;

    #[test]
    fn chord_wrapper_charges_only_the_ir_work() {
        use cadmpeg_ir::math::Point2;
        let surface = NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                ],
                None,
            ),
            false,
        )
        .unwrap()
        .unwrap();
        let parameters = [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)];
        let chord = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)];
        let operation = "test completed chord certificate";
        let boundary = |wrapper| {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                operation,
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    policy.limits.max_retained_bytes = 0;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let result = if wrapper {
                        super::nurbs_surface_parameter_segment_chord_bound(
                            &ctx, &surface, parameters, chord,
                        )?
                    } else {
                        let budget = super::surface_solver_budget(
                            &ctx,
                            &surface,
                            "certify SLDPRT NURBS surface chord",
                        )?;
                        cadmpeg_ir::eval::nurbs_surface_parameter_segment_chord_bound_with_budget(
                            &ctx, &surface, parameters, chord, &budget,
                        )?
                    };
                    assert!(result.is_some_and(|bound| bound.abs() <= EPS_PLANAR_CHORD));
                    ctx.charge_work(1, operation)
                },
            );
            let CodecError::ResourceLimit(limit) = error else {
                panic!("completed chord boundary")
            };
            limit.used
        };
        assert_eq!(boundary(true), boundary(false));
        crate::test_support::work_refusal_at("IR surface segment patch search", |ctx| {
            super::nurbs_surface_parameter_segment_chord_bound(ctx, &surface, parameters, chord)
        });
    }

    #[test]
    fn stored_surface_helpers_preserve_actual_admission_refusal() {
        let surface = NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                (0..3)
                    .map(|u| {
                        (0..3)
                            .map(|v| Point3::new(f64::from(u) * 0.5, f64::from(v) * 0.5, 0.0))
                            .collect()
                    })
                    .collect(),
                None,
            ),
            false,
        )
        .expect("fixture admission")
        .expect("quadratic plane");
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!("three scratch dimensions"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let CodecError::ResourceLimit(original) =
                super::nurbs_surface_partials(&ctx, &surface, 0.5, 0.5).unwrap_err()
            else {
                panic!("the partial helper must preserve its scratch refusal")
            };
            assert_eq!(original.dimension, dimension);
            assert_eq!((original.limit, original.used), (0, 0));
            assert!(original.additional > 0);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert_eq!(
                super::nurbs_surface_parameter_near_point(
                    &ctx,
                    &surface,
                    Point3::new(f64::NAN, 0.0, 0.0),
                    None
                )
                .unwrap(),
                None
            );
            // Rejecting a non-finite point needs no input-sized work. The
            // session still returns the original scratch refusal.
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(limit)) if limit == original));
        }
    }
}
