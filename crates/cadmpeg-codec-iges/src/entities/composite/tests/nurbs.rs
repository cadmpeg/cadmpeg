// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn degree_elevation_preserves_nonzero_declared_interval_endpoints() {
    let interval = [-29.063_334_917_342_4, 2.000_000_000_000_02];
    let mut curve = NurbsCurve {
        degree: 1,
        knots: vec![interval[0], interval[0], interval[1], interval[1]],
        control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        weights: None,
        periodic: false,
    };

    assert!(elevate_nurbs_to_degree(&mut curve, interval, 3, None));
    assert_eq!(curve.knots.first(), Some(&interval[0]));
    assert_eq!(curve.knots.last(), Some(&interval[1]));
}

#[test]
fn concatenation_accepts_analytic_arcs_with_ulp_endpoint_rounding() {
    let center = Point3::new(-55.9308, -12.896_865_742_92, 71.124_028_363_8);
    let first = circular_arc_nurbs(
        center,
        Vector3::new(1.0, 0.0, -0.0),
        Vector3::new(0.0, 0.999_999_999_999_995_7, 9.334_897_886_982_299e-8),
        10.185_400_000_000_001,
        [0.0, 3.141_592_560_240_814_3],
    )
    .unwrap();
    let second = circular_arc_nurbs(
        center,
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, -1.0, 0.0),
        10.185_400_000_000_001,
        [0.0, 3.141_592_746_938_772],
    )
    .unwrap();
    assert!(
        first
            .control_points
            .last()
            .unwrap()
            .distance(second.control_points[0])
            < 0.001
    );
    concatenate_nurbs(
        vec![
            (first, [0.0, 3.141_592_560_240_814_3]),
            (second, [0.0, 3.141_592_746_938_772]),
        ],
        Some(0.001),
    )
    .expect("analytic arcs with source-valid endpoints should concatenate");
}

#[test]
fn bounded_analytic_carrier_uses_admitted_source_endpoint_witnesses() {
    let curve_id = CurveId("circle".into());
    let start_id = PointId("start-point".into());
    let end_id = PointId("end-point".into());
    let start_vertex = VertexId("start-vertex".into());
    let end_vertex = VertexId("end-vertex".into());
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
        geometry: CurveGeometry::Circle {
            center,
            axis,
            ref_direction: reference,
            radius,
        },
        source_object: None,
    });
    ir.model.points.extend([
        Point {
            id: start_id.clone(),
            position: start,
            source_object: None,
        },
        Point {
            id: end_id.clone(),
            position: declared_end,
            source_object: None,
        },
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
        id: EdgeId("edge".into()),
        curve: Some(curve_id.clone()),
        start: start_vertex,
        end: end_vertex,
        param_range: Some(interval),
        tolerance: None,
    });

    let (carrier, _) =
        bounded_nurbs_for_curve_with_tolerance(&ir, &curve_id, Some(0.001), None, None)
            .expect("the source endpoint is inside the declared resolution");
    assert_eq!(carrier.control_points.first(), Some(&start));
    assert_eq!(carrier.control_points.last(), Some(&declared_end));
    assert!(
        bounded_nurbs_for_curve_with_tolerance(&ir, &curve_id, Some(0.0001), None, None,).is_none()
    );
}
