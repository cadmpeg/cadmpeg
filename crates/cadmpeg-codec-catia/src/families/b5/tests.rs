// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `b5` family over synthetic byte fixtures.

#![allow(clippy::unwrap_used)]

use crate::test_support::{
    a8_surface_stream, append_b5_record, b5_analytic_line_pcurve_payload,
    b5_closed_triangle_stream, b5_isoparametric_line_pcurve_payload, b5_linear_pcurve_payload,
    b5_plane_payload, b5_transverse_isoparametric_line_pcurve_payload, le_f32, le_f64,
};

#[test]
fn b5_frame_walk_ignores_markers_inside_payloads() {
    let mut bytes = Vec::new();
    let mut payload = vec![0xb5, 0x03, 0x5f, 0x08, 0, 0, 0, 0, 0x05, 0x08, 0x01];
    for value in [90.0f32, 91.0, 92.0] {
        payload.extend_from_slice(&le_f32(value));
    }
    append_b5_record(&mut bytes, 0x06, 1, &payload);
    bytes.extend_from_slice(&b5_closed_triangle_stream());
    let graph = crate::families::b5::graph::parse(&bytes).expect("length-closed B5 graph");
    assert_eq!(graph.faces.len(), 1);
    assert_eq!(graph.loops.len(), 1);
    assert_eq!(graph.vertex_points.len(), 3);
    assert_eq!(graph.edge_vertices.len(), 3);
}

#[test]
fn b5_analytic_line_pcurve_resolves_to_clamped_linear_form() {
    let mut bytes = b5_closed_triangle_stream();
    append_b5_record(
        &mut bytes,
        0x18,
        600,
        &b5_analytic_line_pcurve_payload(100, [2.0, 3.0], [4.0, -2.0], [-0.5, 1.5]),
    );
    append_b5_record(
        &mut bytes,
        0x18,
        601,
        &b5_isoparametric_line_pcurve_payload(100, 2.0, [-3.0, 5.0]),
    );
    append_b5_record(
        &mut bytes,
        0x18,
        602,
        &b5_transverse_isoparametric_line_pcurve_payload(100, -4.0, [1.0, 7.0]),
    );
    append_b5_record(&mut bytes, 0x5e, 603, &[]);
    let graph = crate::families::b5::graph::parse(&bytes).expect("length-closed B5 graph");
    let pcurve = graph.pcurves.get(&600).expect("analytic line pcurve");
    assert_eq!(pcurve.degree, 1);
    assert_eq!(pcurve.distinct_knots, vec![-0.5, 1.5]);
    assert_eq!(pcurve.multiplicities, vec![2, 2]);
    assert_eq!(pcurve.control_points, vec![[0.0, 4.0], [8.0, 0.0]]);
    assert_eq!(
        pcurve.lifted_endpoints,
        Some([[0.0, 4.0, 0.0], [8.0, 0.0, 0.0]])
    );
    let isoparametric = graph.pcurves.get(&601).expect("isoparametric line pcurve");
    assert_eq!(isoparametric.degree, 1);
    assert_eq!(isoparametric.distinct_knots, vec![-3.0, 5.0]);
    assert_eq!(isoparametric.multiplicities, vec![2, 2]);
    assert_eq!(isoparametric.control_points, vec![[2.0, -3.0], [2.0, 5.0]]);
    assert_eq!(
        isoparametric.lifted_endpoints,
        Some([[2.0, -3.0, 0.0], [2.0, 5.0, 0.0]])
    );
    let transverse = graph.pcurves.get(&602).expect("transverse line pcurve");
    assert_eq!(transverse.degree, 1);
    assert_eq!(transverse.distinct_knots, vec![1.0, 7.0]);
    assert_eq!(transverse.multiplicities, vec![2, 2]);
    assert_eq!(transverse.control_points, vec![[1.0, -4.0], [7.0, -4.0]]);
    assert_eq!(
        transverse.lifted_endpoints,
        Some([[1.0, -4.0, 0.0], [7.0, -4.0, 0.0]])
    );
}

#[test]
fn b5_circle_pcurve_rejects_nonfinite_derived_poles() {
    let mut bytes = b5_closed_triangle_stream();
    let mut payload = vec![0x81, 0x18, 0, 0];
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [f64::MAX, 0.0, 1.0, 1.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    append_b5_record(&mut bytes, 0x19, 600, &payload);

    let graph = crate::families::b5::graph::parse(&bytes).expect("length-closed B5 graph");
    assert!(!graph.pcurves.contains_key(&600));
}

#[test]
fn b5_line_pcurve_rejects_nonfinite_derived_poles() {
    let mut bytes = b5_closed_triangle_stream();
    append_b5_record(
        &mut bytes,
        0x18,
        600,
        &b5_analytic_line_pcurve_payload(0, [f64::MAX, 0.0], [f64::MAX, 0.0], [1.0, 2.0]),
    );

    let graph = crate::families::b5::graph::parse(&bytes).expect("length-closed B5 graph");
    assert!(!graph.pcurves.contains_key(&600));
}

#[test]
fn b5_pcurve_lift_rejects_nonfinite_world_endpoints() {
    let mut bytes = b5_closed_triangle_stream();
    bytes[9..17].copy_from_slice(&f64::MAX.to_le_bytes());
    bytes[8 + 89..8 + 97].copy_from_slice(&0.0f64.to_le_bytes());
    bytes[8 + 97..8 + 105].copy_from_slice(&f64::MAX.to_le_bytes());
    append_b5_record(
        &mut bytes,
        0x18,
        600,
        &b5_isoparametric_line_pcurve_payload(100, f64::MAX, [0.0, 1.0]),
    );

    let graph = crate::families::b5::graph::parse(&bytes).expect("length-closed B5 graph");
    assert!(graph.pcurves[&600].lifted_endpoints.is_none());
}

#[test]
fn b5_object_graph_resolves_face_loop_pcurve_and_edge_members() {
    let mut bytes = a8_surface_stream();
    bytes[7..11].copy_from_slice(&0x1234u32.to_le_bytes());
    let plane = b5_plane_payload([10.0, 0.0, 0.0]);
    append_b5_record(&mut bytes, 0x27, 100, &plane);
    for (id, offset) in [(200u32, 0.0f64), (201, 1.0), (202, 2.0)] {
        let payload = b5_linear_pcurve_payload(100, [offset, 0.0], [offset + 1.0, 0.0]);
        append_b5_record(&mut bytes, 0x21, id, &payload);
    }
    let mut profile = vec![0; 73];
    profile[0] = 0x80;
    for (offset, value) in [
        (1usize, 1.0f64),
        (9, 0.0),
        (17, 0.0),
        (25, 0.0),
        (33, 0.0),
        (41, 1.0),
    ] {
        profile[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    profile[49..57].copy_from_slice(&le_f64(1.0));
    profile[57..65].copy_from_slice(&le_f64(-1.0));
    profile[65..73].copy_from_slice(&le_f64(2.0));
    append_b5_record(&mut bytes, 0x0e, 110, &profile);
    let mut revolution = vec![0; 176];
    revolution[0] = 0x81;
    revolution[1] = 0x38;
    revolution[2..5].copy_from_slice(&[110, 0, 0]);
    revolution[29..37].copy_from_slice(&le_f64(1.0));
    revolution[61..69].copy_from_slice(&le_f64(1.0));
    revolution[93..101].copy_from_slice(&le_f64(1.0));
    for (offset, value) in [
        (101usize, 0.0f64),
        (109, std::f64::consts::PI),
        (117, -1.0),
        (125, 2.0),
        (135, 1.0),
        (143, 1.0),
        (151, 1.0),
        (159, 0.0),
        (168, std::f64::consts::PI),
    ] {
        revolution[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    revolution[133..135].copy_from_slice(&[0x05, 0x05]);
    revolution[167] = 0x01;
    append_b5_record(&mut bytes, 0x2d, 120, &revolution);
    append_b5_record(
        &mut bytes,
        0x21,
        210,
        &b5_linear_pcurve_payload(120, [1.0, 0.0], [1.0, std::f64::consts::PI]),
    );
    append_b5_record(
        &mut bytes,
        0x21,
        211,
        &b5_linear_pcurve_payload(0x1234, [0.0, 0.0], [1.0, 1.0]),
    );
    append_b5_record(&mut bytes, 0x5e, 300, &[]);
    append_b5_record(&mut bytes, 0x5e, 301, &[]);
    append_b5_record(&mut bytes, 0x5e, 0x01_0100, &[]);
    let loop_payload = [
        0x87, 0x18, 200, 0, 0x18, 44, 1, 0x18, 201, 0, 0x18, 45, 1, 0x18, 202, 0, 0x30, 1, 1, 0x18,
        100, 0, 0x83, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0x01, 0x00, 0xff, 0xff,
        0x01, 0x00, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0x01,
    ];
    append_b5_record(&mut bytes, 0x62, 400, &loop_payload);
    append_b5_record(
        &mut bytes,
        0x5f,
        500,
        &[0x82, 0x18, 100, 0, 0x18, 144, 1, 0x05],
    );
    for point in [
        [10.0f32, 0.0, 0.0],
        [11.0, 0.0, 0.0],
        [12.0, 0.0, 0.0],
        [13.0, 0.0, 0.0],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let graph = crate::families::b5::graph::parse(&bytes).expect("B5 object topology");
    assert_eq!(graph.faces[0].surface, 100);
    assert_eq!(graph.faces[0].loops, vec![400]);
    assert_eq!(graph.faces[0].terminal_control, Some(0x05));
    assert_eq!(graph.loops[&400].pcurves, vec![200, 201, 202]);
    assert_eq!(graph.loops[&400].edges, vec![300, 301, 0x01_0100]);
    assert_eq!(graph.pcurves[&200].degree, 1);
    assert_eq!(
        graph.pcurves[&200].control_points,
        vec![[0.0, 0.0], [1.0, 0.0]]
    );
    assert_eq!(
        graph.pcurves[&200].lifted_endpoints,
        Some([[10.0, 0.0, 0.0], [11.0, 0.0, 0.0]])
    );
    assert_eq!(graph.edge_vertices[&300], [0, 1]);
    assert_eq!(graph.edge_vertices[&0x01_0100], [2, 3]);
    let revolution_endpoints = graph.pcurves[&210]
        .lifted_endpoints
        .expect("revolution lift");
    assert!((revolution_endpoints[0][0] - 1.0).abs() < 1.0e-12);
    assert!((revolution_endpoints[1][0] + 1.0).abs() < 1.0e-12);
    assert!((revolution_endpoints[1][2] - 1.0).abs() < 1.0e-12);
    assert_eq!(
        graph.pcurves[&211].lifted_endpoints,
        Some([[0.0, 0.0, 0.0], [8.0, 2.0, 2.0]])
    );
}
