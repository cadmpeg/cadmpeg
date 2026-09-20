// SPDX-License-Identifier: Apache-2.0

use crate::nurbs::*;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};

const RELATIVE_ROUNDOFF: f64 = 32.0 * f64::EPSILON;

#[test]
fn reversal_preserves_large_parameter_offsets_and_endpoint_values() {
    for range in [[1e16, 1e16 + 2.0], [1e308, 1.1e308], [-1.1e308, -1e308]] {
        let [lower, upper] = range;
        let knots = vec![lower, lower, upper, upper];
        let source = NurbsCurve::from_lanes(
            1,
            knots.clone(),
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("linear model curve");
        let reversed = reverse_nurbs_curve(&source, range).expect("finite reflected knots");
        assert_eq!(reversed.knots(), knots);
        assert_eq!(
            reverse_nurbs_curve(&reversed, range).expect("double reversal"),
            source
        );
        let model = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(source));
        let mut refusal = LaneRefusals::new();
        let (reversed, reversed_range) =
            reverse_curve_geometry(&model, range, &mut refusal, "offset curve")
                .expect("reversed curve");
        assert_eq!(reversed_range, range);
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                knots,
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .expect("linear pcurve"),
        };
        let reversed_pcurve =
            reverse_pcurve_geometry(&pcurve, range, &mut refusal, "offset pcurve")
                .expect("reversed pcurve");
        for (parameter, expected_x) in [(lower, 1.0), (upper, 0.0)] {
            assert_eq!(
                cadmpeg_ir::eval::curve_point(&reversed, parameter),
                Some(Point3::new(expected_x, 0.0, 0.0))
            );
            assert_eq!(
                cadmpeg_ir::eval::pcurve_uv(&reversed_pcurve, parameter),
                Some(Point2::new(expected_x, 0.0))
            );
        }
        assert!(refusal.take_notes().is_empty());
    }
}

#[test]
fn reflected_knots_include_exterior_knots_and_canonical_zero() {
    assert_eq!(
        reverse_knots(
            &[1e16 - 2.0, 1e16, 1e16 + 2.0, 1e16 + 4.0],
            [1e16, 1e16 + 2.0]
        ),
        vec![1e16 - 2.0, 1e16, 1e16 + 2.0, 1e16 + 4.0]
    );
    assert_eq!(
        reverse_knots(&[-0.0, 1.0], [0.0, 1.0])[0].to_bits(),
        0.0_f64.to_bits()
    );
}

#[test]
fn line_pcurve_reversal_does_not_need_a_finite_endpoint_sum() {
    let source = PcurveGeometry::Line(
        cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(0.25, 0.0),
        )
        .expect("line pcurve"),
    );
    let range = [1e308, 1.1e308];
    let reversed = reverse_pcurve_geometry(&source, range, &mut LaneRefusals::new(), "large line")
        .expect("finite reflected origin");
    for (from, to) in [(range[0], range[1]), (range[1], range[0])] {
        let original = cadmpeg_ir::eval::pcurve_uv(&source, from).expect("source point");
        let reverse = cadmpeg_ir::eval::pcurve_uv(&reversed, to).expect("reversed point");
        assert!((reverse.u / original.u - 1.0).abs() < RELATIVE_ROUNDOFF);
        assert_eq!(reverse.v, 0.0);
    }
}

fn bilinear_surface(points: Vec<Vec<Point3>>, weights: Vec<Vec<f64>>) -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(points, Some(weights)),
        false,
    )
    .expect("bilinear surface")
}

#[test]
fn isocurve_is_invariant_under_common_weight_scale() {
    for scale in [1.0, 1e308, 1e-200, f64::from_bits(1), -1e308] {
        let surface = bilinear_surface(
            vec![
                vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 1.0, 0.0)],
                vec![Point3::new(4.0, 0.0, 0.0), Point3::new(4.0, 1.0, 0.0)],
            ],
            vec![vec![scale; 2]; 2],
        );
        for (fix_u, expected) in [
            (
                true,
                [Point3::new(3.0, 0.0, 0.0), Point3::new(3.0, 1.0, 0.0)],
            ),
            (
                false,
                [Point3::new(2.0, 0.5, 0.0), Point3::new(4.0, 0.5, 0.0)],
            ),
        ] {
            let curve = cadmpeg_ir::eval::nurbs_surface_isocurve(&surface, if fix_u { cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U } else { cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::V }, 0.5)
            .expect("finite isocurve at any common weight scale");
            assert_eq!(curve.control_points(), expected);
            let weights = curve.weights().expect("rational isocurve");
            assert_eq!(weights[0] / weights[1], 1.0);
        }
    }
}

#[test]
fn isocurve_preserves_weight_ratios_between_output_poles() {
    for scale in [1.0, 1e307, f64::from_bits(1)] {
        let surface = bilinear_surface(
            vec![
                vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 1.0, 0.0)],
                vec![Point3::new(4.0, 0.0, 0.0), Point3::new(4.0, 1.0, 0.0)],
            ],
            vec![vec![scale, 2.0 * scale], vec![2.0 * scale, 4.0 * scale]],
        );
        let curve = cadmpeg_ir::eval::nurbs_surface_isocurve(&surface, cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U, 0.5)
        .expect("finite isocurve");
        let weights = curve.weights().expect("rational isocurve");
        assert!((weights[1] / weights[0] - 2.0).abs() < RELATIVE_ROUNDOFF);
        for (pole, y) in curve.control_points().iter().zip([0.0, 1.0]) {
            assert!((pole.x - 10.0 / 3.0).abs() < RELATIVE_ROUNDOFF);
            assert_eq!(pole.y, y);
        }
    }
}

#[test]
fn isocurve_keeps_finite_maximum_coordinates() {
    let surface = bilinear_surface(
        vec![vec![Point3::new(f64::MAX, 0.0, 0.0); 2]; 2],
        vec![vec![1e200; 2]; 2],
    );
    let curve = cadmpeg_ir::eval::nurbs_surface_isocurve(&surface, cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis::U, 0.5)
    .expect("constant finite surface");
    assert_eq!(curve.control_points(), [Point3::new(f64::MAX, 0.0, 0.0); 2]);
}

#[test]
fn quintic_jet_handles_spans_whose_square_overflows_or_underflows() {
    let h = 1e160;
    let (_, controls) = quintic_jet_bspline(
        5,
        &[0.0, h],
        &[[0.0; 3], [1.0, 0.0, 0.0]],
        &[[1.0 / h, 0.0, 0.0]; 2],
        &[[0.0; 3]; 2],
    )
    .expect("finite linear controls");
    for (control, expected_x) in controls.iter().zip([0.0, 0.2, 0.4, 0.6, 0.8, 1.0]) {
        assert!((control[0] - expected_x).abs() < RELATIVE_ROUNDOFF);
        assert_eq!(&control[1..], &[0.0, 0.0]);
    }
    for (h, acceleration, expected) in [
        (
            2.0_f64.powi(600),
            2.0_f64.powi(-1000),
            2.0_f64.powi(200) / 20.0,
        ),
        (
            2.0_f64.powi(-600),
            2.0_f64.powi(1000),
            2.0_f64.powi(-200) / 20.0,
        ),
        (2.0, f64::MAX, f64::MAX / 5.0),
    ] {
        let (_, controls) = quintic_jet_bspline(
            5,
            &[0.0, h],
            &[[0.0; 2]; 2],
            &[[0.0; 2]; 2],
            &[[acceleration, 0.0]; 2],
        )
        .expect("finite curvature offsets");
        for control in &controls[2..4] {
            assert!((control[0] / expected - 1.0).abs() < RELATIVE_ROUNDOFF);
            assert_eq!(control[1], 0.0);
        }
    }
}

#[test]
fn helix_cache_computes_finite_sagitta_without_doubling_radius() {
    for (radius, sweep, tolerance) in [
        (f64::MAX, 1.0, f64::MAX),
        (1e308, 1e-155, 1e-4),
        (1e308, 1e-165, 1e-4),
    ] {
        let definition = ProceduralCurveDefinition::Helix(
            cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
                [0.0, sweep],
                cadmpeg_ir::geometry::HelixFrame {
                    center: Point3::new(0.0, 0.0, 0.0),
                    major: Vector3::new(radius, 0.0, 0.0),
                    minor: Vector3::new(0.0, radius, 0.0),
                    pitch: Vector3::new(0.0, 0.0, 1.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                },
                0.0,
                None,
            )
            .expect("circular helix"),
        );
        let cache = circular_helix_cache(
            &definition,
            tolerance,
            &mut LaneRefusals::new(),
            "large helix",
        )
        .expect("finite cache and sagitta");
        assert!(cache.fit_tolerance > 0.0 && cache.fit_tolerance <= tolerance);
        let step = sweep / (cache.curve.control_points().len() - 1) as f64;
        // For a tiny angle, sagitta/r is step^2/8 to relative roundoff.
        // Divide before comparing so the reference does not square the angle.
        let expected = if step < 1e-100 {
            (radius * step) * (step / 8.0)
        } else {
            radius * (1.0 - (step / 2.0).cos())
        };
        assert!((cache.fit_tolerance / expected - 1.0).abs() < RELATIVE_ROUNDOFF);
    }
}
