// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeMode, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

use super::*;

#[test]
fn composite_child_types_follow_the_declared_dialect() {
    assert!(composite_child_type_allowed(116, 0, Dialect::V4_0));
    assert!(composite_child_type_allowed(132, 0, Dialect::V4_0));
    assert!(!composite_child_type_allowed(106, 1, Dialect::V4_0));
    assert!(!composite_child_type_allowed(130, 0, Dialect::V4_0));
    assert!(composite_child_type_allowed(106, 1, Dialect::V5_0));
    assert!(composite_child_type_allowed(130, 0, Dialect::V5_0));
    assert!(composite_child_type_allowed(142, 0, Dialect::V5_3));
}

#[test]
fn zero_join_tolerance_requires_exact_endpoint_equality() {
    let left = Point3::new(1.0, 2.0, 3.0);
    let right = Point3::new(1.0, 2.0, 3.0 + f64::EPSILON * 4.0);

    assert!(close_with_tolerance(left, left, Some(0.0)));
    assert!(!close_with_tolerance(left, right, Some(0.0)));
}

#[test]
fn positive_join_tolerance_excludes_the_resolution_boundary() {
    let left = Point3::new(0.0, 0.0, 0.0);
    let inside = Point3::new(0.000_999, 0.0, 0.0);
    let boundary = Point3::new(0.001, 0.0, 0.0);

    assert!(close_with_tolerance(left, inside, Some(0.001)));
    assert!(!close_with_tolerance(left, boundary, Some(0.001)));
}

#[test]
fn bounded_line_carrier_excludes_an_endpoint_at_the_resolution_boundary() {
    let curve_id = CurveId("line".into());
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.points.extend([
        Point {
            id: PointId("start-point".into()),
            position: Point3::new(0.001, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("end-point".into()),
            position: Point3::new(1.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId("start".into()),
            point: PointId("start-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("end".into()),
            point: PointId("end-point".into()),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("edge".into()),
        curve: Some(curve_id.clone()),
        start: VertexId("start".into()),
        end: VertexId("end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: None,
    });

    assert!(
        bounded_nurbs_for_curve_with_tolerance(&ir, &curve_id, Some(0.001), None, None).is_none()
    );

    ir.model.points[0].position = Point3::new(0.000_999, 0.0, 0.0);
    assert!(
        bounded_nurbs_for_curve_with_tolerance(&ir, &curve_id, Some(0.001), None, None).is_some()
    );
}

#[test]
fn decode_refuses_a_composite_child_count_over_its_projection_limit() {
    let error = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 102,
                form: 0,
                label: "COMPOSIT".into(),
                status: "00000000",
                parameters: "102,100001;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("iges_composite_children")
                && limit.limit == 100_000
                && limit.used == 100_000
                && limit.additional == 1
    ));
}

#[test]
fn composite_flattening_over_its_depth_limit_fuses_the_decode_session() {
    let base_id = CurveId("base".into());
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(Curve {
        id: base_id.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });
    ir.model.points.extend([
        Point {
            id: PointId("base-start-point".into()),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("base-end-point".into()),
            position: Point3::new(1.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId("base-start".into()),
            point: PointId("base-start-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("base-end".into()),
            point: PointId("base-end-point".into()),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("base-edge".into()),
        curve: Some(base_id.clone()),
        start: VertexId("base-start".into()),
        end: VertexId("base-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: None,
    });

    let mut child_id = base_id;
    for level in 0..65 {
        let composite_id = CurveId(format!("composite-{level}"));
        ir.model.curves.push(Curve {
            id: composite_id.clone(),
            geometry: CurveGeometry::Composite {
                segments: vec![CompositeCurveSegment {
                    curve: child_id,
                    same_sense: true,
                    transition: CompositeCurveTransition::Continuous,
                }],
                self_intersect: None,
            },
            source_object: None,
        });
        child_id = composite_id;
    }

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default()).unwrap();
    assert!(bounded_nurbs_for_curve(&ir, &child_id, Some(&ctx), None).is_none());
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Codec("iges_composite_depth")
                && limit.limit == 64
                && limit.used == 64
                && limit.additional == 1
    ));
}

#[test]
fn bounded_line_carrier_selects_a_curve_valid_edge_occurrence() {
    let curve_id = CurveId("line".into());
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.points.extend([
        Point {
            id: PointId("wrong-start-point".into()),
            position: Point3::new(10.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("wrong-end-point".into()),
            position: Point3::new(11.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("matching-start-point".into()),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("matching-end-point".into()),
            position: Point3::new(2.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId("wrong-start".into()),
            point: PointId("wrong-start-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("wrong-end".into()),
            point: PointId("wrong-end-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("matching-start".into()),
            point: PointId("matching-start-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("matching-end".into()),
            point: PointId("matching-end-point".into()),
            tolerance: None,
        },
    ]);
    ir.model.edges.extend([
        Edge {
            id: EdgeId("wrong-occurrence".into()),
            curve: Some(curve_id.clone()),
            start: VertexId("wrong-start".into()),
            end: VertexId("wrong-end".into()),
            param_range: Some([5.0, 6.0]),
            tolerance: None,
        },
        Edge {
            id: EdgeId("matching-occurrence".into()),
            curve: Some(curve_id),
            start: VertexId("matching-start".into()),
            end: VertexId("matching-end".into()),
            param_range: Some([0.0, 2.0]),
            tolerance: None,
        },
    ]);

    let (carrier, range) = bounded_nurbs_for_curve(&ir, &CurveId("line".into()), None, None)
        .expect("the curve-valid edge occurrence");
    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(carrier.control_points[0], Point3::new(0.0, 0.0, 0.0));
    assert_eq!(carrier.control_points[1], Point3::new(2.0, 0.0, 0.0));
}

#[test]
fn bounded_line_carrier_rejects_conflicting_valid_edge_ranges() {
    let curve_id = CurveId("line".into());
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    for (index, end) in [(0, 1.0), (1, 2.0)] {
        let start_point = PointId(format!("start-point-{index}"));
        let end_point = PointId(format!("end-point-{index}"));
        let start_vertex = VertexId(format!("start-{index}"));
        let end_vertex = VertexId(format!("end-{index}"));
        ir.model.points.extend([
            Point {
                id: start_point.clone(),
                position: Point3::new(index as f64, 0.0, 0.0),
                source_object: None,
            },
            Point {
                id: end_point.clone(),
                position: Point3::new(end, 0.0, 0.0),
                source_object: None,
            },
        ]);
        ir.model.vertices.extend([
            Vertex {
                id: start_vertex.clone(),
                point: start_point,
                tolerance: None,
            },
            Vertex {
                id: end_vertex.clone(),
                point: end_point,
                tolerance: None,
            },
        ]);
        ir.model.edges.push(Edge {
            id: EdgeId(format!("edge-{index}")),
            curve: Some(curve_id.clone()),
            start: start_vertex,
            end: end_vertex,
            param_range: Some([index as f64, end]),
            tolerance: None,
        });
    }

    assert!(bounded_nurbs_for_curve(&ir, &curve_id, None, None).is_none());
}

#[test]
fn composite_index_lookups_match_the_unindexed_scan() {
    let bounded = CurveId("bounded".into());
    let edgeless = CurveId("edgeless".into());
    let absent = CurveId("absent".into());
    let mut ir = CadIr::empty(Units::default());
    for id in [bounded.clone(), edgeless.clone()] {
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    ir.model.points.extend([
        Point {
            id: PointId("start-point".into()),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("end-point".into()),
            position: Point3::new(2.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId("start".into()),
            point: PointId("start-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("end".into()),
            point: PointId("end-point".into()),
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("edge".into()),
        curve: Some(bounded.clone()),
        start: VertexId("start".into()),
        end: VertexId("end".into()),
        param_range: Some([0.0, 2.0]),
        tolerance: None,
    });

    let index = CompositeIndex::from_ir(&ir);
    for curve_id in [bounded, edgeless, absent] {
        let scanned = bounded_nurbs_for_curve(&ir, &curve_id, None, None);
        let indexed = bounded_nurbs_for_curve(&ir, &curve_id, None, Some(&index));
        assert_eq!(
            scanned.as_ref().map(|(carrier, range)| (
                carrier.degree,
                carrier.control_points.clone(),
                *range
            )),
            indexed.as_ref().map(|(carrier, range)| (
                carrier.degree,
                carrier.control_points.clone(),
                *range
            )),
        );
    }

    assert!(bounded_nurbs_for_curve(&ir, &CurveId("bounded".into()), None, Some(&index)).is_some());
    assert!(
        bounded_nurbs_for_curve(&ir, &CurveId("edgeless".into()), None, Some(&index)).is_none()
    );
    assert!(bounded_nurbs_for_curve(&ir, &CurveId("absent".into()), None, Some(&index)).is_none());
}

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
    assert!(bounded_nurbs_for_curve(&ir, &composite_id, None, None).is_none());
    let (carrier, range) =
        bounded_nurbs_for_curve_with_tolerance(&ir, &composite_id, Some(0.001), None, None)
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

#[test]
fn decode_concatenates_ordered_composite_curve_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected a concatenated NURBS cache");
    };
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(nurbs.control_points.len(), 3);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(1, &nurbs.knots, &nurbs.control_points, None, 1.5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn composite_join_uses_global_resolution_and_reports_degradation() {
    let within_resolution = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.000_999)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let within_curve = within_resolution
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .expect("Type 102 curve within the Global resolution");
    assert!(matches!(
        within_curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
    ));
    assert!(within_resolution.report().losses.is_empty());

    let outside_resolution = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_001)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let outside_curve = outside_resolution
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .expect("degraded Type 102 curve");
    let cadmpeg_ir::geometry::CurveGeometry::Composite { segments, .. } = &outside_curve.geometry
    else {
        panic!("expected retained native Type 102 carrier")
    };
    assert_eq!(
        segments[1].transition,
        cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous
    );
    assert!(outside_resolution.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::CompositeCarrierDegraded.kind()
            && loss.message.contains("Global minimum resolution")
    }));
    let validation = cadmpeg_ir::validate_neutral(
        outside_resolution.ir(),
        outside_resolution.report().losses.clone(),
    );
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let at_or_beyond_resolution = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_000_000_000_000_2)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let at_or_beyond_resolution_curve = at_or_beyond_resolution
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .expect("Type 102 curve at the Global resolution");
    assert!(matches!(
        at_or_beyond_resolution_curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Composite { .. }
    ));
    assert_eq!(
        at_or_beyond_resolution
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::CompositeCarrierDegraded.kind())
            .count(),
        1
    );
}

#[test]
fn strict_decode_refuses_a_degraded_composite_carrier_loss() {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;

    let error = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_001)),
            &options,
        )
        .unwrap_err();

    match error {
        CodecError::StrictRefusal { loss_code, .. } => {
            assert_eq!(
                loss_code,
                IgesLossCode::CompositeCarrierDegraded.kind().as_str()
            );
        }
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn decode_concatenates_exact_circular_arc_and_line_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(mixed_analytic_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected an exact quadratic composite cache");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 5);
    assert_eq!(
        nurbs.weights.as_ref().unwrap()[1],
        std::f64::consts::FRAC_1_SQRT_2
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_converts_heterogeneous_composite_curve_children_to_an_exact_carrier() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(heterogeneous_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected an exact heterogeneous composite carrier");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 5);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_mixed_degree_composite_pcurve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(mixed_degree_composite_pcurve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D7")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("expected an elevated cubic composite cache");
    };
    assert_eq!(nurbs.degree, 3);
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge
                .curve
                .as_ref()
                .is_some_and(|id| id.0 == "iges:model:curve#D7"))
            .and_then(|edge| edge.param_range),
        Some([0.0, 2.0])
    );
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D11")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(face.loops.len(), 1);
    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert!(matches!(
        result.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs { degree: 3, .. }
    ));
    assert_eq!(result.ir().model.pcurves[0].fit_tolerance, None);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_composite_curve_with_an_inconsistent_parametric_spline_child() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D3")
        .expect("composite curve should be projected after its spline child");
    assert!(matches!(
        composite.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
    ));
    assert_eq!(result.report().losses.len(), 2);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("terminal derivative block disagrees with the last polynomial")
    }));
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
            .count(),
        1
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::SplineHeaderNotTransferred.kind())
            .count(),
        1
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_large_composite_batch_without_repeated_curve_scans() {
    const COMPOSITE_COUNT: usize = 2_000;
    let mut entities = vec![
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "CHILD1".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "CHILD2".into(),
            status: "00010000",
            parameters: "110,1,0,0,2,0,0;".into(),
        },
    ];
    entities.extend((0..COMPOSITE_COUNT).map(|index| OwnedTestEntity {
        entity_type: 102,
        form: 0,
        label: format!("C{index:06}"),
        status: "00000000",
        parameters: "102,2,1,3;".into(),
    }));

    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_curves.len(), COMPOSITE_COUNT);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}
