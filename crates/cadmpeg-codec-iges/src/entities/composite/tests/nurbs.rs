// SPDX-License-Identifier: Apache-2.0

use crate::entities::composite::bounded_nurbs_for_curve_with_tolerance;
use crate::entities::composite::concatenate_nurbs;
use crate::entities::composite::elevate_nurbs_to_degree;
use crate::entities::curve_conversion::circular_arc_nurbs;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::ids::EdgeId;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::ids::VertexId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::topology::Vertex;
use cadmpeg_ir::CadIr;

#[test]
fn degree_elevation_preserves_nonzero_declared_interval_endpoints() {
    let interval = [-29.063_334_917_342_4, 2.000_000_000_000_02];
    let mut curve = NurbsCurve::from_lanes(
        1,
        vec![interval[0], interval[0], interval[1], interval[1]],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("valid line");

    elevate_nurbs_to_degree(&mut curve, interval, 3, None).expect("elevation lanes pair");
    assert_eq!(curve.knots().first(), Some(&interval[0]));
    assert_eq!(curve.knots().last(), Some(&interval[1]));
    assert_eq!(&curve.knots()[..4], &[interval[0]; 4]);
    assert_eq!(&curve.knots()[4..], &[interval[1]; 4]);
}

#[test]
fn concatenation_accepts_analytic_arcs_with_ulp_endpoint_rounding() {
    let center = Point3::new(-55.9308, -12.896_865_742_92, 71.124_028_363_8);
    let first = circular_arc_nurbs(
        center,
        Vector3::new(1.0, 0.0, -0.0),
        Vector3::new(0.0, 0.999_999_999_999_995_7, 9.334_897_886_982_299e-8),
        PositiveLength::new(10.185_400_000_000_001).expect("positive radius"),
        [0.0, 3.141_592_560_240_814_3],
    )
    .expect("carrier lanes pair")
    .unwrap();
    let second = circular_arc_nurbs(
        center,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, -1.0, 0.0),
        PositiveLength::new(10.185_400_000_000_001).expect("positive radius"),
        [0.0, 3.141_592_746_938_772],
    )
    .expect("carrier lanes pair")
    .unwrap();
    assert!(
        first
            .control_points()
            .last()
            .unwrap()
            .distance(second.control_points()[0].get())
            < 0.001
    );
    concatenate_nurbs(
        vec![
            (first, [0.0, 3.141_592_560_240_814_3], ()),
            (second, [0.0, 3.141_592_746_938_772], ()),
        ],
        Some(0.001),
    )
    .expect("carrier lanes pair")
    .expect("analytic arcs with source-valid endpoints should concatenate");
}

#[test]
fn bounded_analytic_carrier_uses_admitted_source_endpoint_witnesses() {
    let curve_id = CurveId::mint("test:model:curve#circle").expect("identity grammar");
    let start_id = PointId::mint("test:model:point#start-point").expect("identity grammar");
    let end_id = PointId::mint("test:model:point#end-point").expect("identity grammar");
    let start_vertex = VertexId::mint("test:model:vertex#start-vertex").expect("identity grammar");
    let end_vertex = VertexId::mint("test:model:vertex#end-vertex").expect("identity grammar");
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let radius = 10.0;
    let interval: [f64; 2] = [0.0, 1.25];
    let start = center.translated(reference, radius);
    let evaluated_end = center
        .translated(reference, radius * interval[1].cos())
        .translated(axis.cross(reference), radius * interval[1].sin());
    let declared_end = evaluated_end.translated(Vector3::new(0.0, 0.0005, 0.0), 1.0);
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(center, axis, reference, radius)
                .unwrap(),
        )),
        source_object: None,
    });
    ir.model.points.extend([
        Point::new(
            start_id.clone(),
            cadmpeg_ir::features::FinitePoint3::new(start).expect("a finite position is a point"),
            None,
        ),
        Point::new(
            end_id.clone(),
            cadmpeg_ir::features::FinitePoint3::new(declared_end)
                .expect("a finite position is a point"),
            None,
        ),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: start_vertex.clone(),
            point: start_id,
            tolerance: None,
        },
        Vertex {
            id: end_vertex.clone(),
            point: end_id,
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:edge#edge").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id.clone()), Some(interval))
            .unwrap(),
        start: start_vertex,
        end: end_vertex,
        tolerance: None,
    });

    let (carrier, _) =
        bounded_nurbs_for_curve_with_tolerance(&ir, &curve_id, Some(0.001), None, None)
            .expect("carrier lanes pair")
            .expect("the source endpoint is inside the declared resolution");
    assert_eq!(carrier.pole_rows().points().first(), Some(&start));
    assert_eq!(carrier.pole_rows().points().last(), Some(&declared_end));
    assert!(
        bounded_nurbs_for_curve_with_tolerance(&ir, &curve_id, Some(0.0001), None, None,)
            .expect("carrier lanes pair")
            .is_none()
    );
}

/// A child whose knot vector is not clamped to its declared interval states
/// that cause. The concatenation reports the boundary multiplicity, not an
/// endpoint join it never tested.
#[test]
fn a_child_that_does_not_elevate_states_its_own_cause() {
    // Child A: degree 1 with non-clamped knots.
    let first = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.5, 1.0, 1.5],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("valid child");
    // Child B: degree 2, clamped, starting at child A's last control point.
    let second = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("valid child");

    let error = concatenate_nurbs(
        vec![(first, [0.0, 1.5], ()), (second, [0.0, 1.0], ())],
        Some(0.001),
    )
    .expect_err("a child that does not raise to the composite degree states why");
    let text = error.to_string();
    assert!(
        text.contains("boundary knot") || text.contains("does not span its declared interval"),
        "the error names the elevation cause: {text}"
    );
    assert!(!text.contains("join"), "{text}");
}

#[test]
fn audit_regression_join_rescales_weights_without_overflowing_ratio() {
    let segment = |x, weight| {
        NurbsCurve::from_lanes(
            1,
            vec![0., 0., 1., 1.],
            vec![Point3::new(x, 0., 0.), Point3::new(x + 1., 0., 0.)],
            Some(vec![weight, weight]),
            false,
        )
        .unwrap()
    };
    let joined = concatenate_nurbs(
        vec![
            (segment(0., 1e200), [0., 1.], ()),
            (segment(1., 1e-200), [0., 1.], ()),
        ],
        Some(0.),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        joined.nurbs.control_points(),
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(1., 0., 0.),
            Point3::new(2., 0., 0.)
        ]
    );
}
