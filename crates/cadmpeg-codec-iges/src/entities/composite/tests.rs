// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn rational_linear_degree_elevation_preserves_the_curve() {
    let mut curve = NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        weights: Some(vec![1.0, 3.0]),
        periodic: false,
    };
    let before = cadmpeg_ir::eval::nurbs_curve_point(
        curve.degree,
        &curve.knots,
        &curve.control_points,
        curve.weights.as_deref(),
        0.25,
    )
    .expect("valid rational linear NURBS evaluates before degree elevation");
    assert!(elevate_linear_bezier_to_degree(&mut curve, [0.0, 1.0], 2));
    let after = cadmpeg_ir::eval::nurbs_curve_point(
        curve.degree,
        &curve.knots,
        &curve.control_points,
        curve.weights.as_deref(),
        0.25,
    )
    .expect("valid rational quadratic NURBS evaluates after degree elevation");
    assert!(before.distance(after) <= 1.0e-12);
    assert_eq!(curve.control_points[1], Point3::new(1.5, 0.0, 0.0));
    assert_eq!(curve.weights, Some(vec![1.0, 2.0, 3.0]));
}

#[test]
fn multi_span_linear_degree_elevation_preserves_a_degenerate_curve() {
    let mut curve = NurbsCurve {
        degree: 1,
        knots: vec![0.5, 0.5, 1.5, 2.5, 2.5],
        control_points: vec![
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(1.0, 2.0, 3.0),
        ],
        weights: None,
        periodic: false,
    };
    let before = cadmpeg_ir::eval::nurbs_curve_point(
        curve.degree,
        &curve.knots,
        &curve.control_points,
        curve.weights.as_deref(),
        2.0,
    )
    .expect("valid multi-span linear NURBS evaluates before degree elevation");
    assert!(elevate_linear_nurbs_to_degree(
        &mut curve,
        [0.5, 2.5],
        3,
        None
    ));
    let after = cadmpeg_ir::eval::nurbs_curve_point(
        curve.degree,
        &curve.knots,
        &curve.control_points,
        curve.weights.as_deref(),
        2.0,
    )
    .expect("valid multi-span linear NURBS evaluates after degree elevation");
    assert_eq!(curve.degree, 3);
    assert!(before.distance(after) <= 1.0e-12);
}

#[test]
fn mixed_degree_composition_accepts_a_multi_span_linear_child() {
    let point = |x, y| Point3::new(x, y, 0.0);
    let line = |start, end| NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![start, end],
        weights: None,
        periodic: false,
    };
    let constant = |position| NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        control_points: vec![position; 3],
        weights: None,
        periodic: false,
    };
    let cubic = NurbsCurve {
        degree: 3,
        knots: vec![0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0],
        control_points: vec![
            point(1.0, 1.0),
            point(1.666_666_666_666_666_7, 0.666_666_666_666_666_6),
            point(2.333_333_333_333_333_5, 0.333_333_333_333_333_3),
            point(3.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let mut children = vec![
        (line(point(3.0, 0.0), point(2.0, 0.0)), [0.0, 1.0]),
        (constant(point(2.0, 0.0)), [0.0, 2.0]),
        (line(point(2.0, 0.0), point(1.0, 0.0)), [0.0, 1.0]),
        (line(point(1.0, 0.0), point(1.0, 1.0)), [0.0, 1.0]),
        (cubic, [0.0, 2.0]),
        (line(point(3.0, 0.0), point(3.0, 0.0)), [0.0, 1.0]),
    ];
    for (index, (curve, interval)) in children.iter_mut().enumerate() {
        if curve.degree < 3 {
            assert!(
                elevate_linear_nurbs_to_degree(curve, *interval, 3, None),
                "child {index} should elevate"
            );
        }
    }
    let concatenated = concatenate_nurbs(children, None)
        .expect("mixed-degree composite should have an exact NURBS carrier");
    assert_eq!(concatenated.nurbs.degree, 3);
    assert_eq!(
        concatenated.boundaries,
        vec![0.0, 1.0, 3.0, 4.0, 5.0, 7.0, 8.0]
    );
}

#[test]
fn concatenated_range_is_exactly_the_canonical_knot_domain() {
    let line = |start: f64, end: f64, x: f64| {
        (
            NurbsCurve {
                degree: 1,
                knots: vec![start, start, end, end],
                control_points: vec![Point3::new(x, 0.0, 0.0), Point3::new(x + 1.0, 0.0, 0.0)],
                weights: None,
                periodic: false,
            },
            [start, end],
        )
    };
    let first = line(0.0, 0.3, 0.0);
    let second = line(1.0e9, 1.0e9 + 0.1, 1.0);

    let concatenated =
        concatenate_nurbs(vec![first, second], None).expect("joined lines should concatenate");

    assert_eq!(
        concatenated.boundaries.last(),
        concatenated.nurbs.knots.last()
    );
}

#[test]
fn tolerance_allows_a_bounded_carrier_join_within_resolution() {
    let first_id = CurveId("first".into());
    let second_id = CurveId("second".into());
    let composite_id = CurveId("composite".into());
    let first_end = Point3::new(1.0, 0.0, 0.0);
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.curves.extend([
        Curve {
            id: first_id.clone(),
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point3::new(0.0, 0.0, 0.0), first_end],
                weights: None,
                periodic: false,
            }),
            source_object: None,
        },
        Curve {
            id: second_id.clone(),
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point3::new(1.0005, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                weights: None,
                periodic: false,
            }),
            source_object: None,
        },
        Curve {
            id: composite_id.clone(),
            geometry: CurveGeometry::Composite {
                segments: vec![
                    CompositeCurveSegment {
                        curve: first_id.clone(),
                        same_sense: true,
                        transition: CompositeCurveTransition::Continuous,
                    },
                    CompositeCurveSegment {
                        curve: second_id.clone(),
                        same_sense: true,
                        transition: CompositeCurveTransition::Continuous,
                    },
                ],
                self_intersect: None,
            },
            source_object: None,
        },
    ]);
    for (index, curve) in [first_id, second_id].into_iter().enumerate() {
        ir.model.edges.push(Edge {
            id: EdgeId(format!("edge-{index}")),
            curve: Some(curve),
            start: VertexId(format!("start-{index}")),
            end: VertexId(format!("end-{index}")),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        });
    }
    assert!(bounded_nurbs_for_curve(&ir, &composite_id, None).is_none());
    let (carrier, range) =
        bounded_nurbs_for_curve_with_tolerance(&ir, &composite_id, Some(0.001), None)
            .expect("carrier join within the global resolution should project");
    assert_eq!(range, [0.0, 2.0]);
    assert_eq!(carrier.control_points[0], Point3::new(0.0, 0.0, 0.0));
}

#[test]
fn reversing_a_subrange_reflects_the_active_nurbs_domain() {
    let curve = NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 10.0, 10.0],
        control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
        weights: None,
        periodic: false,
    };
    let (reversed, range) = reverse_nurbs(curve, [2.0, 5.0])
        .expect("a bounded subrange should have an exact reversed carrier");
    assert_eq!(range, [5.0, 8.0]);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            reversed.degree,
            &reversed.knots,
            &reversed.control_points,
            reversed.weights.as_deref(),
            range[0],
        ),
        Some(Point3::new(5.0, 0.0, 0.0))
    );
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            reversed.degree,
            &reversed.knots,
            &reversed.control_points,
            reversed.weights.as_deref(),
            range[1],
        ),
        Some(Point3::new(2.0, 0.0, 0.0))
    );
}

#[test]
fn reversing_a_range_outside_the_active_nurbs_domain_is_rejected() {
    let curve = NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 10.0, 10.0],
        control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
        weights: None,
        periodic: false,
    };
    assert!(reverse_nurbs(curve, [-1.0, 5.0]).is_none());
}
