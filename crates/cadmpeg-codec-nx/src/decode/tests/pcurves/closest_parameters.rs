// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

use crate::decode::blend::{
    closest_nurbs_curve_parameter_with_budget, closest_pcurve_parameters,
    closest_periodic_analytic_curve_parameter, homogeneous_residual_distance,
    real_polynomial_roots,
};
use cadmpeg_ir::geometry::nurbs::{bezier::homogeneous_spans, NurbsCurve};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::math::{Point2, Point3};

const EPS_QUARTIC_ROOT_VALUE: f64 = 1.0e-10;
const EPS_FIXED_ANALYTIC_PARAMETER: f64 = 1.0e-8;

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
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                4,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                controls,
                Some(weights.to_vec()),
                false,
            )
            .expect("fixture pcurve construction admission")
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
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                4,
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                control_points,
                Some(weights.to_vec()),
                false,
            )
            .expect("fixture pcurve construction admission")
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
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            control_points,
            Some(weights.to_vec()),
            false,
        )
        .expect("fixture constructor admission")
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
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                knots.clone(),
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(0.0, 0.0),
                ],
                None,
                true,
            )
            .expect("fixture pcurve construction admission")
            .unwrap(),
        };
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            knots,
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            None,
            true,
        )
        .expect("fixture constructor admission")
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
    let roots = real_polynomial_roots(&[-1.0, 3.5, -3.0, -0.5, 1.0])
        .expect("finite quartic roots");

    assert_eq!(roots.as_slice().len(), 3);
    for (actual, expected) in roots.as_slice().iter().zip([-2.0, 0.5, 1.0]) {
        assert!((actual - expected).abs() < EPS_QUARTIC_ROOT_VALUE, "{actual}");
    }
}

#[test]
fn fixed_analytic_curve_search_uses_no_decode_resources() {
    use cadmpeg_ir::geometry::analytic::{CircleCurve, EllipseCurve};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

    let center = Point3::new(2.0, 3.0, 4.0);
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0);
    let reference = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let parameter = 1.2;
    let ellipse = SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(center, axis, reference, 12.0, 5.0).unwrap(),
    );
    let ellipse_geometry = CurveGeometry::Solved(ellipse.clone());
    let mut ellipse_point = cadmpeg_ir::eval::decode::curve_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &ellipse_geometry,
        parameter,
    )
    .unwrap()
    .get();
    ellipse_point.y += 3.0;

    let circle = SolvedCurveGeometry::Circle(
        CircleCurve::try_new(center, axis, reference, 12.0).unwrap(),
    );
    let circle_geometry = CurveGeometry::Solved(circle.clone());
    let mut circle_point = cadmpeg_ir::eval::decode::curve_point(
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &circle_geometry,
        parameter,
    )
    .unwrap()
    .get();
    circle_point.y += 3.0;

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let ellipse_parameter =
                closest_periodic_analytic_curve_parameter(&ellipse, ellipse_point, None)
                    .expect("ellipse closest parameter");
            let continued_ellipse_parameter = closest_periodic_analytic_curve_parameter(
                &ellipse,
                ellipse_point,
                Some(parameter + std::f64::consts::TAU),
            )
            .expect("continued ellipse parameter");
            let circle_parameter =
                closest_periodic_analytic_curve_parameter(&circle, circle_point, None)
                    .expect("circle closest parameter");
            let continued_circle_parameter = closest_periodic_analytic_curve_parameter(
                &circle,
                circle_point,
                Some(parameter + std::f64::consts::TAU),
            )
            .expect("continued circle parameter");
            let ellipse_center = closest_periodic_analytic_curve_parameter(
                &ellipse,
                center,
                Some(1.4),
            )
            .expect("upper ellipse center branch");
            let lower_ellipse_center = closest_periodic_analytic_curve_parameter(
                &ellipse,
                center,
                Some(4.8),
            )
            .expect("lower ellipse center branch");

            assert!(
                (ellipse_parameter - parameter).abs() < EPS_FIXED_ANALYTIC_PARAMETER
            );
            assert!(
                (continued_ellipse_parameter - parameter - std::f64::consts::TAU).abs()
                    < EPS_FIXED_ANALYTIC_PARAMETER
            );
            assert!(
                (circle_parameter - parameter).abs() < EPS_FIXED_ANALYTIC_PARAMETER
            );
            assert!(
                (continued_circle_parameter - parameter - std::f64::consts::TAU).abs()
                    < EPS_FIXED_ANALYTIC_PARAMETER
            );
            assert!(
                (ellipse_center - std::f64::consts::FRAC_PI_2).abs()
                    < EPS_FIXED_ANALYTIC_PARAMETER
            );
            assert!(
                (lower_ellipse_center - 3.0 * std::f64::consts::FRAC_PI_2).abs()
                    < EPS_FIXED_ANALYTIC_PARAMETER
            );
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn coincident_pcurve_interval_retains_seed_and_boundaries() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![Point2::new(2.0, -3.0); 3],
                None,
                false,
            )
            .expect("fixture pcurve construction admission")
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

#[test]
fn coincident_pcurve_geometry_probe_refuses_session_work_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::scalar::NonNegativeReal;

    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) =
        super::quadratic_paraboloid_surface()
    else {
        unreachable!("the fixture is a NURBS surface");
    };
    let surfaces = [
        SurfaceId::mint("nx:test:surface#quadratic-first").expect("identity grammar"),
        SurfaceId::mint("nx:test:surface#quadratic-second").expect("identity grammar"),
    ];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .extend(surfaces.iter().cloned().map(|id| Surface {
            id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone())),
            source_object: None,
        }));
    let pcurve = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
        )
        .expect("finite line pcurve"),
    );

    // Each separation probe and each probed interval is one unit of the
    // adaptive geometry budget, which draws on the session work allowance.
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "work_budget",
        |ctx| {
            crate::decode::pcurves::coincident_pcurve_pair(
                ctx,
                &ir,
                [&surfaces[0], &surfaces[1]],
                [&pcurve, &pcurve],
                [0.0, 1.0],
                NonNegativeReal::new(0.1).expect("nonnegative tolerance"),
            )
            .map(|_| ())
            .map_err(Into::into)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "work_budget"
    ));
}
