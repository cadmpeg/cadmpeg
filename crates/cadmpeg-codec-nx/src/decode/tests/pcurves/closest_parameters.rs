// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

use crate::decode::blend::{
    closest_nurbs_curve_parameter_with_budget, closest_pcurve_parameters,
    homogeneous_residual_distance, real_polynomial_roots,
};
use cadmpeg_ir::geometry::nurbs::{bezier::homogeneous_spans, NurbsCurve};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::math::{Point2, Point3};

#[test]
fn rational_pcurve_incidence_isolates_close_branches() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
        let controls = [
            0.006_306_3,
            -0.029_213_45,
            0.095_295_133_333_333_34,
            -0.070_192_95,
            0.024_297_3,
        ]
        .into_iter()
        .zip(weights)
        .map(|(numerator, weight)| Point2::new(numerator / weight, 0.0))
        .collect::<Vec<_>>();
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
                4,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                controls,
                Some(weights.to_vec()),
                false,
            ).expect("fixture pcurve construction admission")
            .unwrap(),
        };
        let roots =
            closest_pcurve_parameters(geometry_ctx, &pcurve, Point2::new(0.0, 0.0), Some(0.11))
                .expect("evaluator allocation succeeds")
                .expect("complete homogeneous root isolation");

        assert_eq!(roots.len(), 4);
        for (actual, expected) in roots.iter().zip([0.1001, 0.1, 0.7, 0.9]) {
            assert!((actual - expected).abs() < 1.0e-8);
        }
    });
}

#[test]
fn rational_pcurve_closest_search_retains_close_global_branches() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
        let control_points = [
            0.006_306_3,
            -0.029_213_45,
            0.095_295_133_333_333_34,
            -0.070_192_95,
            0.024_297_3,
        ]
        .into_iter()
        .zip(weights)
        .map(|(numerator, weight)| Point2::new(numerator / weight, 0.0))
        .collect();
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
                4,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                control_points,
                Some(weights.to_vec()),
                false,
            ).expect("fixture pcurve construction admission")
            .unwrap(),
        };
        let parameters =
            closest_pcurve_parameters(geometry_ctx, &pcurve, Point2::new(0.0, 1.0e-4), Some(0.11))
                .expect("evaluator allocation succeeds")
                .expect("complete global closest-point search");

        assert_eq!(parameters.len(), 4, "{parameters:?}");
        for (actual, expected) in parameters.iter().zip([0.1001, 0.1, 0.7, 0.9]) {
            assert!((actual - expected).abs() < 1.0e-8);
        }
    });
}

#[test]
fn rational_spine_closest_search_resolves_close_global_branches() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let weights = [1.0, 1.1, 0.9, 1.2, 1.0];
        let control_points = [
            0.006_306_3,
            -0.029_213_45,
            0.095_295_133_333_333_34,
            -0.070_192_95,
            0.024_297_3,
        ]
        .into_iter()
        .zip(weights)
        .map(|(numerator, weight)| Point3::new(numerator / weight, 0.0, 0.0))
        .collect();
        let curve = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            control_points,
            Some(weights.to_vec()),
            false,
        ).expect("fixture constructor admission")
        .unwrap();
        let point = Point3::new(0.0, 1.0e-4, 0.0);

        let first = closest_nurbs_curve_parameter_with_budget(
            &curve,
            point,
            Some(0.099),
            &crate::decode::geometry_work::GeometryWorkBudget::from_context(
                geometry_ctx,
                cadmpeg_core::decode::u64_from_index(8_000_000),
            ),
        )
        .expect("evaluator allocation succeeds")
        .expect("first close branch");
        let second = closest_nurbs_curve_parameter_with_budget(
            &curve,
            point,
            Some(0.101),
            &crate::decode::geometry_work::GeometryWorkBudget::from_context(
                geometry_ctx,
                cadmpeg_core::decode::u64_from_index(8_000_000),
            ),
        )
        .expect("evaluator allocation succeeds")
        .expect("second close branch");
        let remote = closest_nurbs_curve_parameter_with_budget(
            &curve,
            point,
            Some(0.69),
            &crate::decode::geometry_work::GeometryWorkBudget::from_context(
                geometry_ctx,
                cadmpeg_core::decode::u64_from_index(8_000_000),
            ),
        )
        .expect("evaluator allocation succeeds")
        .expect("remote global branch");

        assert!((first - 0.1).abs() < 1.0e-8);
        assert!((second - 0.1001).abs() < 1.0e-8);
        assert!((remote - 0.7).abs() < 1.0e-8);
    });
}

#[test]
fn periodic_nurbs_inversion_lifts_the_continuation_phase() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let knots = vec![0.0, 0.0, 1.0, 2.0, 2.0];
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
                1,
                knots.clone(),
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(0.0, 0.0),
                ],
                None,
                true,
            ).expect("fixture pcurve construction admission")
            .unwrap(),
        };
        let curve = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            knots,
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            None,
            true,
        ).expect("fixture constructor admission")
        .unwrap();

        assert_eq!(
            closest_pcurve_parameters(geometry_ctx, &pcurve, Point2::new(0.0, 0.0), Some(4.1))
                .expect("evaluator allocation succeeds")
                .expect("periodic pcurve phase"),
            [4.0]
        );
        assert_eq!(
            closest_nurbs_curve_parameter_with_budget(
                &curve,
                Point3::new(0.0, 0.0, 0.0),
                Some(4.1),
                &crate::decode::geometry_work::GeometryWorkBudget::from_context(
                    geometry_ctx,
                    cadmpeg_core::decode::u64_from_index(8_000_000)
                )
            )
            .expect("evaluator allocation succeeds")
            .expect("periodic curve phase"),
            4.0
        );
    });
}

#[test]
fn polynomial_root_isolation_retains_repeated_real_roots() {
    let roots = crate::test_support::with_decode_context(|ctx| {
        real_polynomial_roots(ctx, &[-1.0, 3.5, -3.0, -0.5, 1.0])
    })
    .expect("roots are admitted")
    .expect("finite quartic roots");

    assert_eq!(roots.len(), 3);
    for (actual, expected) in roots.iter().zip([-2.0, 0.5, 1.0]) {
        assert!((actual - expected).abs() < 1.0e-10, "{actual}");
    }
}

#[test]
fn coincident_pcurve_interval_retains_seed_and_boundaries() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![Point2::new(2.0, -3.0); 3],
                None,
                false,
            ).expect("fixture pcurve construction admission")
            .unwrap(),
        };
        let roots =
            closest_pcurve_parameters(geometry_ctx, &pcurve, Point2::new(2.0, -3.0), Some(0.3))
                .expect("evaluator allocation succeeds")
                .expect("coincident interval");

        assert_eq!(roots, [0.3, 0.0, 1.0]);
    });
}

#[test]
fn pcurve_bezier_extraction_preserves_rational_knot_spans() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let knots = [0.0, 0.0, 0.0, 0.25, 0.75, 1.0, 1.0, 1.0];
        let points = [
            Point2::new(-1.0, 0.0),
            Point2::new(0.0, 2.0),
            Point2::new(1.0, -1.0),
            Point2::new(2.0, 3.0),
            Point2::new(4.0, 0.0),
        ];
        let weights = [1.0, 1.5, 0.75, 2.0, 1.25];
        let controls = points
            .iter()
            .zip(weights)
            .map(|(point, weight)| [point.u * weight, point.v * weight, weight])
            .collect::<Vec<_>>();
        let spans = homogeneous_spans(geometry_ctx, 2, &knots, &controls)
            .expect("resource allocation did not fail")
            .expect("valid Bézier extraction");

        assert_eq!(spans.len(), 3);
        for span in spans.iter() {
            for fraction in [0.0, 0.5, 1.0] {
                let parameter = span.domain[0] + fraction * (span.domain[1] - span.domain[0]);
                let expected = cadmpeg_ir::eval::nurbs_pcurve_uv(
                    geometry_ctx,
                    2,
                    &knots,
                    &points,
                    Some(&weights),
                    parameter,
                )
                .expect("source NURBS evaluation");
                let actual = homogeneous_residual_distance(
                    &span.controls,
                    parameter,
                    span.domain,
                    &crate::decode::geometry_work::GeometryWorkBudget::from_context(
                        geometry_ctx,
                        cadmpeg_core::decode::u64_from_index(100),
                    ),
                )
                .expect("test solver allocation succeeds");
                let expected = expected.as_raw();
                assert!((actual - expected.u.hypot(expected.v)).abs() < 1.0e-12);
            }
        }
    });
}
