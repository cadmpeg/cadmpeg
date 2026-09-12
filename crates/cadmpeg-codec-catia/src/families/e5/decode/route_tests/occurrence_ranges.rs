// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn support_range_agreement_requires_matching_endpoints() {
    assert!(parameter_range_agreement_tolerance([0.0, 5.0], [0.0, 5.0]).is_some());
    assert!(parameter_range_agreement_tolerance([0.0, 5.0], [1.0, 6.0]).is_none());
}

#[test]
fn occurrence_intersection_maps_distinct_local_ranges_to_support_range() {
    let sides = vec![
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#left".to_string())
                .expect("identity grammar"),
            pcurve: PcurveGeometry::Line(
                cadmpeg_ir::geometry::LinePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            pcurve_range: [100.0, 200.0],
            curve: None,
        },
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#right".to_string())
                .expect("identity grammar"),
            pcurve: PcurveGeometry::Line(
                cadmpeg_ir::geometry::LinePcurve::try_new(
                    Point2::new(0.0, 1.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            pcurve_range: [-5.0, 5.0],
            curve: None,
        },
    ];
    let context = e5_support_occurrence_intersection_context([10.0, 20.0], [10.0, 20.0], &sides)
        .expect("support intersection context");
    assert_eq!(context.parameter_range(), [10.0, 20.0]);
    assert_eq!(
        context.sides()[0]
            .pcurve_parameter_range()
            .expect("left local range"),
        [100.0, 200.0]
    );
    assert_eq!(
        context.sides()[1]
            .pcurve_parameter_range()
            .expect("right local range"),
        [-5.0, 5.0]
    );
    assert_eq!(
        context.sides()[0]
            .pcurve_parameter([10.0, 20.0], 15.0)
            .expect("left mapped parameter"),
        150.0
    );
    assert_eq!(
        context.sides()[1]
            .pcurve_parameter([10.0, 20.0], 15.0)
            .expect("right mapped parameter"),
        0.0
    );
}

#[test]
fn occurrence_intersection_cache_requires_one_admitted_exact_carrier() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid LineCurve fixture"),
    ));
    let nurbs = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid linear NURBS"),
    ));
    let mut sides = vec![
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#left".to_string())
                .expect("identity grammar"),
            pcurve: PcurveGeometry::Line(
                cadmpeg_ir::geometry::LinePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            pcurve_range: [10.0, 20.0],
            curve: Some((line.clone(), [0.0, 1.0])),
        },
        E5OccurrenceIntersectionSide {
            surface: SurfaceId::mint("catia:test:surface#right".to_string())
                .expect("identity grammar"),
            pcurve: PcurveGeometry::Line(
                cadmpeg_ir::geometry::LinePcurve::try_new(
                    Point2::new(0.0, 1.0),
                    Point2::new(1.0, 0.0),
                )
                .expect("valid LinePcurve fixture"),
            ),
            pcurve_range: [-4.0, 6.0],
            curve: Some((nurbs, [100.0, 110.0])),
        },
    ];
    let (cache, range) = e5_occurrence_intersection_cache(&sides).expect("analytic cache");
    assert_eq!(cache, line);
    assert_eq!(range, [0.0, 1.0]);

    sides[1].curve = Some((
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("valid CircleCurve fixture"),
        )),
        [0.0, 1.0],
    ));
    assert!(e5_occurrence_intersection_cache(&sides).is_none());

    let left_circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    let right_circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            4.0,
        )
        .expect("valid CircleCurve fixture"),
    ));
    sides[0].curve = Some((left_circle.clone(), [0.0, std::f64::consts::PI]));
    sides[1].curve = Some((
        right_circle,
        [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
    ));
    let (cache, range) =
        e5_occurrence_intersection_cache(&sides).expect("frame-gauged circle cache");
    assert_eq!(cache, left_circle);
    assert_eq!(range, [0.0, std::f64::consts::PI]);
}

#[test]
fn quintic_jet_reproduces_endpoint_second_order_data() {
    let curve = quintic_jet_pcurve(
        5,
        &[0.0, 2.0],
        &[[0.0, 0.0], [2.0, 0.0]],
        &[[1.0, 0.0], [1.0, 0.0]],
        &[[0.0, 0.0], [0.0, 0.0]],
    )
    .expect("linear quintic segment");
    for parameter in [0.0, 0.5, 1.0, 2.0] {
        let point = pcurve_uv(&curve, parameter).expect("jet evaluation");
        assert!((point.u - parameter).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!(point.v.abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    }
}

#[test]
fn reversing_nurbs_preserves_tiny_knot_domain() {
    let tiny = 1e-200;
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![tiny, tiny, 2.0 * tiny, 2.0 * tiny],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid tiny-domain NURBS"),
    ));
    let (reversed, range) =
        crate::nurbs::reverse_curve_geometry(&curve, [tiny, 2.0 * tiny]).expect("reversed NURBS");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(reversed)) = reversed else {
        panic!("expected NURBS");
    };
    assert_eq!(range, [tiny, 2.0 * tiny]);
    assert_eq!(reversed.knots(), [tiny, tiny, 2.0 * tiny, 2.0 * tiny]);
    assert_eq!(
        reversed.control_points(),
        [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)]
    );
}
