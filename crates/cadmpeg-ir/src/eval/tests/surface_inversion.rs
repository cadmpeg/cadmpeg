// SPDX-License-Identifier: Apache-2.0
//! Surface inversion regressions.

use super::bilinear_surface;
use super::nurbs_surface_parameter_near_point;
use super::nurbs_surface_parameter_within_tolerance;
use super::nurbs_surface_parameter_within_tolerance_with_budget;
use super::NurbsSurface;
use super::NurbsSurfaceAxis;
use super::NurbsSurfaceLanes;
use super::Point2;
use super::Point3;
use super::WorkBudget;

const EPS_INVERSE_CONTRACT_MARGIN: f64 = 1.0e-12;
const EPS_SURFACE_INVERSE_FIT: f64 = 1.0e-10;

#[test]
fn surface_inverse_returns_attached_refusal_before_optional_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    for trigger in 0..4 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let budget = ctx.work_budget(10_000);
        let result = match trigger {
            0 => crate::eval::nurbs_surface_closest_parameter_with_budget(
                &ctx, &surface, point, None, &budget,
            ),
            1 => nurbs_surface_parameter_within_tolerance(
                &ctx,
                &surface,
                point,
                None,
                EPS_SURFACE_INVERSE_FIT,
            ),
            2 => nurbs_surface_parameter_within_tolerance_with_budget(
                &ctx,
                &surface,
                point,
                None,
                EPS_SURFACE_INVERSE_FIT,
                &budget,
            ),
            3 => crate::eval::nurbs_surface_parameter_within_nonnegative_tolerance_with_budget(
                &ctx,
                &surface,
                point,
                None,
                crate::scalar::NonNegativeReal::new(EPS_SURFACE_INVERSE_FIT).expect("nonnegative"),
                &budget,
            ),
            _ => unreachable!("four public entry points"),
        };
        let limit = result.expect_err("an attached work refusal is not optional absence");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.limit, 0);
        assert_eq!(limit.operation, "work_budget");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}

#[test]
fn nurbs_surface_inverse_distinguishes_closest_and_tolerance_contracts() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let closest = crate::eval::nurbs_surface_closest_parameter_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        &cadmpeg_core::decode::WorkBudget::new(crate::eval::DEFAULT_NURBS_SURFACE_INVERSION_WORK),
    )
    .expect("resource allocation did not fail")
    .expect("closest surface parameter");
    assert!((closest.u - 0.3).abs() < 1.0e-12);
    assert!((closest.v - 0.7).abs() < 1.0e-12);
    assert!(nurbs_surface_parameter_within_tolerance(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        0.19
    )
    .expect("resource allocation did not fail")
    .is_none());
    assert!(nurbs_surface_parameter_within_tolerance(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        0.2 + EPS_INVERSE_CONTRACT_MARGIN
    )
    .expect("resource allocation did not fail")
    .is_some());
}

#[test]
fn nurbs_surface_inverse_admits_its_tolerance_before_the_search() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let budget = || WorkBudget::new(crate::eval::DEFAULT_NURBS_SURFACE_INVERSION_WORK);
    for tolerance in [-1.0e-12, f64::NAN, f64::INFINITY] {
        assert!(nurbs_surface_parameter_within_tolerance_with_budget(
            &cadmpeg_test_support::service_decode_context(),
            &surface,
            point,
            None,
            tolerance,
            &budget(),
        )
        .expect("resource allocation did not fail")
        .is_none());
    }
    let tolerance = crate::scalar::NonNegativeReal::new(0.2 + 1.0e-12).unwrap();
    let parameters = crate::eval::nurbs_surface_parameter_within_nonnegative_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        tolerance,
        &budget(),
    )
    .expect("resource allocation did not fail");
    assert!(parameters.is_some());
    assert_eq!(
        parameters,
        nurbs_surface_parameter_within_tolerance_with_budget(
            &cadmpeg_test_support::service_decode_context(),
            &surface,
            point,
            None,
            tolerance.get(),
            &budget(),
        )
        .expect("resource allocation did not fail")
    );
}

#[test]
fn budgeted_nurbs_surface_inverse_stops_before_unbounded_patch_work() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let budget = WorkBudget::new(0);

    assert!(nurbs_surface_parameter_within_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        EPS_SURFACE_INVERSE_FIT,
        &budget,
    )
    .expect("resource allocation did not fail")
    .is_none());
    assert!(budget.exhausted());

    let budget = WorkBudget::new(10_000);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        None,
        EPS_SURFACE_INVERSE_FIT,
        &budget,
    )
    .expect("resource allocation did not fail")
    .expect("a valid surface fits within a larger caller-owned budget");
    assert!((parameters.u - 0.3).abs() < 1.0e-12);
    assert!((parameters.v - 0.7).abs() < 1.0e-12);
    assert!(budget.consumed() > 0);
}

#[test]
fn budgeted_nurbs_surface_inverse_accepts_a_fit_qualified_seed_first() {
    const FIT_TOLERANCE: f64 = 1.0e-12;

    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let refused_ctx = cadmpeg_test_support::service_decode_context();
    let refused_budget = WorkBudget::new(12);
    let refusal = nurbs_surface_parameter_within_tolerance_with_budget(
        &refused_ctx,
        &surface,
        point,
        Some(Point2::new(0.3, 0.7)),
        FIT_TOLERANCE,
        &refused_budget,
    )
    .expect_err("the original cap cannot admit all pole traversals");
    assert_eq!(refusal.operation, "geometry evaluation work slice");
    assert!(matches!(refused_ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == refusal));
    let budget = WorkBudget::new(20);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        Some(Point2::new(0.3, 0.7)),
        FIT_TOLERANCE,
        &budget,
    )
    .expect("resource allocation did not fail")
    .expect("a fit-qualified continuation seed does not need global search");

    assert_eq!(parameters, Point2::new(0.3, 0.7));
    // Four poles enter each of four coordinate sums and the constant scan.
    assert_eq!(budget.consumed(), 4 * 5);
}

#[test]
fn budgeted_nurbs_surface_inverse_refines_an_approximate_seed_before_global_search() {
    const FIT_TOLERANCE: f64 = 1.0e-10;
    const PARAMETER_TOLERANCE: f64 = 1.0e-12;

    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let refused_ctx = cadmpeg_test_support::service_decode_context();
    let refused_budget = WorkBudget::new(256);
    let refusal = nurbs_surface_parameter_within_tolerance_with_budget(
        &refused_ctx,
        &surface,
        point,
        Some(Point2::new(0.29, 0.69)),
        FIT_TOLERANCE,
        &refused_budget,
    )
    .expect_err("the original cap cannot admit all pole traversals");
    assert_eq!(refusal.operation, "geometry evaluation work slice");
    assert!(matches!(refused_ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == refusal));
    let budget = WorkBudget::new(32768);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point,
        Some(Point2::new(0.29, 0.69)),
        FIT_TOLERANCE,
        &budget,
    )
    .expect("resource allocation did not fail")
    .expect("a nearby seed should be refined before global patch search");

    assert!((parameters.u - 0.3).abs() <= PARAMETER_TOLERANCE);
    assert!((parameters.v - 0.7).abs() <= PARAMETER_TOLERANCE);
    assert!(budget.consumed() > 0);
}

#[test]
fn nurbs_surface_local_inverse_returns_a_forward_checked_candidate() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let parameters = nurbs_surface_parameter_near_point(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        point,
        None,
    )
    .expect("resource allocation did not fail")
    .expect("bounded local surface candidate");
    let mapped = crate::eval::decode::nurbs_surface_point(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        parameters.u,
        parameters.v,
    )
    .expect("surface point");
    assert!(mapped.distance(point) <= 0.2 + f64::EPSILON * 1024.0);
    assert!((parameters.u - 0.3).abs() < f64::EPSILON * 1024.0);
    assert!((parameters.v - 0.7).abs() < f64::EPSILON * 1024.0);
}

#[test]
fn nurbs_surface_inverse_handles_rational_internal_spans() {
    let surface = NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(0.5, 0.0, 0.2), Point3::new(0.5, 1.0, 0.2)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
            Some(vec![1.0, 1.0, 0.7, 0.7, 1.0, 1.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let point = crate::eval::decode::nurbs_surface_point(
        crate::eval::admission::EvaluationAdmission::Standard,
        &surface,
        0.75,
        0.4,
    )
    .expect("surface point");
    let parameters = nurbs_surface_parameter_within_tolerance(
        &cadmpeg_test_support::service_decode_context(),
        &surface,
        point.get(),
        None,
        EPS_SURFACE_INVERSE_FIT,
    )
    .expect("resource allocation did not fail")
    .expect("rational multi-span inverse");
    assert!((parameters.u - 0.75).abs() < 1.0e-9);
    assert!((parameters.v - 0.4).abs() < 1.0e-9);
}

#[test]
fn local_surface_inverse_preserves_each_work_refusal() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    let surface = bilinear_surface();
    for seed in [None, Some(Point2::new(0.0, 0.0))] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "IR homogeneous pole traversal",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::eval::test_support::with_policy(policy, |ctx| {
                    let result = nurbs_surface_parameter_near_point(
                        ctx,
                        &surface,
                        Point3::new(0.3, 0.7, 0.0),
                        seed,
                    );
                    let original = result.unwrap_err();
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(original.operation, "IR homogeneous pole traversal");
                    assert_eq!(ctx.resource_refusal(), Some(original));
                    assert_eq!(
                        nurbs_surface_parameter_near_point(
                            ctx,
                            &surface,
                            Point3::new(f64::NAN, 0.0, 0.0),
                            None,
                        ),
                        Err(original)
                    );
                    result.map_err(cadmpeg_core::CodecError::from)
                })
            },
        );
    }
}

#[test]
fn surface_partials_preserve_scratch_refusal_and_temporary_lifetime() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
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
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!("three scratch dimensions"),
        }
        crate::eval::test_support::with_policy(policy, |ctx| {
            let Err(crate::eval::EvaluationFailure::ResourceLimit(original)) =
                crate::eval::nurbs_surface_partials(ctx, &surface, 0.5, 0.5)
            else {
                panic!("partial scratch must refuse")
            };
            assert_eq!(original.dimension, dimension);
            assert_eq!((original.limit, original.used), (0, 0));
            assert!(original.additional > 0);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert_eq!(
                crate::eval::nurbs_surface_second_partials(ctx, &surface, f64::NAN, 0.5),
                Err(crate::eval::EvaluationFailure::ResourceLimit(original))
            );
        });
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::eval::test_support::with_policy(policy, |ctx| {
        let partials = crate::eval::nurbs_surface_partials(ctx, &surface, 0.5, 0.5).unwrap();
        assert_eq!(partials.point, Point3::new(0.5, 0.5, 0.0));
        assert_eq!(partials.du, crate::math::Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(partials.dv, crate::math::Vector3::new(0.0, 1.0, 0.0));
        let second = crate::eval::nurbs_surface_second_partials(ctx, &surface, 0.5, 0.5).unwrap();
        assert_eq!(second.duu, crate::math::Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(second.duv, crate::math::Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(second.dvv, crate::math::Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(ctx.resource_refusal(), None);
    });
}
