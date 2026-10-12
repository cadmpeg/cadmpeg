use super::*;
use crate::families::b5::graph::B5PcurveParameterization;
use crate::test_support::test_b5;

fn line(start: f64, end: f64) -> B5Pcurve {
    B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: test_b5::finite_lane(&[10.0, 14.0]),
        multiplicities: vec![2, 2],
        control_points: vec![
            test_b5::finite_vector([0.25, start]),
            test_b5::finite_vector([0.25, end]),
        ],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    }
}

#[test]
fn isocurve_lift_binds_affine_and_reversed_reference_parameters() {
    crate::test_support::with_service_context(|ctx| {
        let surface = NurbsSurface::from_lanes(
            ctx,
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![2.0, 2.0, 6.0, 6.0], false),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![
                    vec![point3([0.0, 2.0, 0.0]), point3([0.0, 6.0, 0.0])],
                    vec![point3([2.0, 2.0, 0.0]), point3([2.0, 6.0, 2.0])],
                ],
                None,
            ),
            false,
        )
        .expect("budget")
        .expect("surface");
        let surface = B5Surface::Nurbs(surface);
        for (start, end) in [(2.0, 6.0), (6.0, 2.0)] {
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))) =
                parameterized_lift(ctx, &line(start, end), &surface).expect("budget")
            else {
                panic!("parameterized isocurve");
            };
            assert_eq!(curve.knots().as_slice(), &[10.0, 10.0, 14.0, 14.0]);
            let points = curve.pole_rows().raw_points();
            assert_eq!(points[0], point3([0.5, start, (start - 2.0) / 8.0]));
            assert_eq!(points[1], point3([0.5, end, (end - 2.0) / 8.0]));
        }
    });
}

#[test]
fn cylinder_lift_preserves_pcurve_origin_scale_and_reversal() {
    crate::test_support::with_service_context(|ctx| {
        let surface = B5Surface::Cylinder {
            origin: test_b5::point([0.0, 0.0, 3.0]),
            frame: test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            radius: test_b5::positive_length(2.0),
            u_range: test_b5::increasing([0.0, 1.0]),
            v_range: test_b5::increasing([-20.0, 20.0]),
            angular_scale: test_b5::finite(1.0),
            chart_origin: test_b5::finite(0.0),
        };
        for (start, end) in [(-5.0, 7.0), (7.0, -5.0)] {
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))) =
                parameterized_lift(ctx, &line(start, end), &surface).expect("budget")
            else {
                panic!("parameterized cylinder lift");
            };
            assert_eq!(curve.knots().as_slice(), &[10.0, 10.0, 14.0, 14.0]);
            let points = curve.pole_rows().raw_points();
            assert_eq!(
                points[0],
                point3([2.0 * 0.25_f64.cos(), 2.0 * 0.25_f64.sin(), 3.0 + start])
            );
            assert_eq!(
                points[1],
                point3([2.0 * 0.25_f64.cos(), 2.0 * 0.25_f64.sin(), 3.0 + end])
            );
        }
    });
}
