// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `e5` family over synthetic byte fixtures.

#![allow(clippy::unwrap_used)]

use crate::tests::{
    append_e5_record, e5_circle_stream, e5_plane_stream, e5_plane_stream_with_transform_scalars,
    e5_torus_stream, e5_uv_line_payload, le_f32, le_f64,
};
use cadmpeg_ir::geometry::SurfaceGeometry;

#[test]
fn e5_circle_parser_reads_framed_carrier() {
    let stream = e5_circle_stream();
    let circles = crate::families::e5::records::e5_circles(&stream);
    assert_eq!(circles.len(), 1);
    match &circles[0].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        } => {
            assert_eq!(*center, cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0));
            assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(*radius, 2.5);
        }
        other => panic!("expected circle, got {other:?}"),
    }
    let surfaces = crate::families::e5::records::e5_surfaces(&stream);
    assert!(matches!(
        surfaces[0].geometry,
        SurfaceGeometry::Cylinder { radius: 2.5, .. }
    ));
}

#[test]
fn e5_edge_parser_reads_u24_reference_tokens() {
    let mut record = vec![0u8; 13];
    record[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
    record[3] = 0xff;
    let payload = [
        0x85, 0x38, 1, 2, 3, 0x38, 4, 5, 6, 0x38, 7, 8, 9, 0x80, 0x80, 0x80,
    ];
    record[5..7].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    record.extend_from_slice(&payload);

    let edges = crate::families::e5::records::e5_edges(&record);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].start_vertex_id, 0x06_0504);
    assert_eq!(edges[0].end_vertex_id, 0x09_0807);
}

#[test]
fn e5_topology_follows_face_loop_and_serialized_edge_members() {
    let mut bytes = Vec::new();
    for id in [10u32, 20, 30] {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }
    for (id, start, end) in [(100u8, 10u8, 20u8), (101, 20, 30), (102, 30, 10)] {
        append_e5_record(
            &mut bytes,
            0xff,
            u32::from(id),
            &[0x85, 0x08, 200, 0x08, start, 0x08, end, 0x80, 0x80],
        );
    }
    for (id, surface, offset) in [
        (400u32, 500u16, 0.0),
        (401, 500, 1.0),
        (402, 500, 2.0),
        (410, 501, 0.0),
        (411, 501, 1.0),
        (412, 501, 2.0),
    ] {
        append_e5_record(&mut bytes, 0x96, id, &e5_uv_line_payload(surface, offset));
    }
    let mut jet = vec![0x81, 0x18];
    jet.extend_from_slice(&500u16.to_le_bytes());
    for value in [5u32, 0, 0, 2, 0, 0, 0] {
        jet.extend_from_slice(&value.to_le_bytes());
    }
    jet.extend_from_slice(&le_f64(1.0));
    for value in [6u32, 6, 2] {
        jet.extend_from_slice(&value.to_le_bytes());
    }
    for values in [[1.0f64, 0.0], [0.0, 1.0], [0.0, -1.0], [1.0, 0.0]] {
        for value in values {
            jet.extend_from_slice(&le_f64(value));
        }
    }
    jet.extend_from_slice(&1u16.to_le_bytes());
    for values in [[-1.0f64, 0.0], [0.0, -1.0]] {
        for value in values {
            jet.extend_from_slice(&le_f64(value));
        }
    }
    jet.extend_from_slice(&le_f64(0.0));
    jet.extend_from_slice(&le_f64(1.0));
    append_e5_record(&mut bytes, 0xa0, 403, &jet);
    let mut support_payload = vec![0x82, 0x18, 144, 1, 0x18, 154, 1, 0x81, 0, 0];
    support_payload.extend_from_slice(&le_f64(-10.0));
    support_payload.extend_from_slice(&le_f64(10.0));
    append_e5_record(&mut bytes, 0xc1, 200, &support_payload);
    let mut bound_payload = vec![0x82, 0x18, 144, 1, 0x08, 200, 0x82];
    for (parameter, code) in [(0.25f64, 1u32), (0.75, 7)] {
        bound_payload.extend_from_slice(&le_f64(parameter));
        bound_payload.extend_from_slice(&code.to_le_bytes());
    }
    append_e5_record(&mut bytes, 0x0e, 900, &bound_payload);
    let mut loop_payload = vec![
        0x87, 0x18, 144, 1, 0x08, 100, 0x18, 145, 1, 0x08, 101, 0x18, 146, 1, 0x08, 102, 0x18, 244,
        1, 0x83,
    ];
    for _ in 0..13 {
        loop_payload.extend_from_slice(&1i16.to_le_bytes());
    }
    append_e5_record(&mut bytes, 0x09, 300, &loop_payload);
    let mut reverse_loop_payload = vec![
        0x87, 0x18, 154, 1, 0x08, 100, 0x18, 156, 1, 0x08, 102, 0x18, 155, 1, 0x08, 101, 0x18, 245,
        1, 0x83,
    ];
    for _ in 0..12 {
        reverse_loop_payload.extend_from_slice(&1i16.to_le_bytes());
    }
    reverse_loop_payload.extend_from_slice(&0i16.to_le_bytes());
    append_e5_record(&mut bytes, 0x09, 301, &reverse_loop_payload);
    append_e5_record(&mut bytes, 0xcc, 500, &[]);
    append_e5_record(&mut bytes, 0xcc, 501, &[]);
    append_e5_record(
        &mut bytes,
        0x00,
        600,
        &[0x82, 0x18, 244, 1, 0x18, 44, 1, 0x01, 0x00],
    );
    append_e5_record(
        &mut bytes,
        0x00,
        601,
        &[0x82, 0x18, 245, 1, 0x18, 45, 1, 0x01, 0x00],
    );
    append_e5_record(
        &mut bytes,
        0x08,
        700,
        &[0x82, 0x18, 88, 2, 0x18, 89, 2, 0x82, 1, 0, 1, 0, 1, 0, 1, 0],
    );
    append_e5_record(&mut bytes, 0x01, 800, &[0x81, 0x18, 188, 2]);

    let topology = crate::families::e5::graph::parse_topology(&bytes).expect("E5 graph");
    assert_eq!(topology.faces.len(), 2);
    assert_eq!(topology.faces[0].surface, 500);
    assert_eq!(topology.faces[0].loops[0].edge_uses, vec![100, 101, 102]);
    assert_eq!(
        topology.faces[0].loops[0].reversed,
        vec![false, false, false]
    );
    assert_eq!(topology.faces[0].loops[0].outer, Some(true));
    assert_eq!(topology.faces[0].loops[0].orientation_signs, vec![1; 13]);
    assert_eq!(
        topology.faces[0].loops[0]
            .resolved_members()
            .unwrap()
            .iter()
            .map(|member| (member.serialized_index, member.reversed))
            .collect::<Vec<_>>(),
        vec![(0, false), (1, false), (2, false)]
    );
    assert_eq!(
        topology.faces[1].loops[0]
            .resolved_members()
            .unwrap()
            .iter()
            .map(|member| (member.serialized_index, member.reversed))
            .collect::<Vec<_>>(),
        vec![(0, true), (1, true), (2, true)]
    );
    assert_eq!(
        topology.faces[1].loops[0].orientation_signs,
        [vec![1; 12], vec![0]].concat()
    );
    assert_eq!(topology.bodies[0].faces, vec![600, 601]);
    assert_eq!(topology.bodies[0].face_orientation_signs, vec![1, 1]);
    assert_eq!(topology.bodies[0].extra_orientation_signs, [1, 1]);
    assert_eq!(topology.pcurves.len(), 7);
    assert!(matches!(
        topology.pcurves[&400],
        crate::families::e5::graph::E5Pcurve::Line {
            direction: [1.0, 0.0],
            ..
        }
    ));
    assert_eq!(topology.bounds[&900].entries[0].parameter, 0.25);
    assert_eq!(topology.bounds[&900].entries[1].representation, 200);
    assert_eq!(topology.curve_supports[&200].pcurves, vec![400, 410]);
    assert_eq!(topology.curve_supports[&200].range, [-10.0, 10.0]);
    assert!(matches!(
        topology.pcurves[&403],
        crate::families::e5::graph::E5Pcurve::Jet { degree: 5, ref knots, .. } if knots == &[0.0, 1.0]
    ));
}

#[test]
fn e5_surface_parser_reads_framed_torus() {
    let surfaces = crate::families::e5::records::e5_surfaces(&e5_torus_stream());
    assert_eq!(surfaces.len(), 1);
    match &surfaces[0].geometry {
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            assert_eq!(*center, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
            assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(
                *ref_direction,
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            );
            assert_eq!((*major_radius, *minor_radius), (12.0, 2.0));
        }
        other => panic!("expected torus, got {other:?}"),
    }
}

#[test]
fn e5_plane_parser_preserves_origin_and_natural_bounds_without_fabricating_axes() {
    let planes = crate::families::e5::records::e5_planes(&e5_plane_stream());
    assert_eq!(planes.len(), 1);
    assert_eq!(planes[0].record_id, 42);
    assert_eq!(planes[0].origin, [1.0, 2.0, 3.0]);
    assert_eq!(planes[0].u_range, [-4.0, 7.0]);
    assert_eq!(planes[0].v_range, [-2.0, 9.0]);
}

#[test]
fn e5_plane_parser_reads_terminal_bounds_after_extended_transform_lane() {
    let planes =
        crate::families::e5::records::e5_planes(&e5_plane_stream_with_transform_scalars(5));
    assert_eq!(planes.len(), 1);
    assert_eq!(planes[0].origin, [1.0, 2.0, 3.0]);
    assert_eq!(planes[0].u_range, [-4.0, 7.0]);
    assert_eq!(planes[0].v_range, [-2.0, 9.0]);
}

#[test]
fn e5_vertices_exclude_marker_like_record_payload_bytes() {
    let mut false_vertex = vec![0x05, 0x08, 0x01];
    for value in [90.0f32, 91.0, 92.0] {
        false_vertex.extend_from_slice(&le_f32(value));
    }
    let mut stream = Vec::new();
    append_e5_record(&mut stream, 0xc0, 1, &false_vertex);
    stream.extend_from_slice(&[0x05, 0x08, 0x01]);
    for value in [1.0f32, 2.0, 3.0] {
        stream.extend_from_slice(&le_f32(value));
    }
    append_e5_record(&mut stream, 0xfe, 2, &[]);

    let vertices = crate::families::e5::records::e5_vertices(&stream, 1);
    assert_eq!(vertices.len(), 1);
    assert_eq!(vertices[0], cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
}

#[test]
fn e5_vertices_reject_multiple_matching_coordinate_runs() {
    let mut stream = Vec::new();
    for (record_id, coordinate) in [(1, 1.0f32), (2, 2.0)] {
        append_e5_record(&mut stream, 0xfe, record_id, &[]);
        stream.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [coordinate, 0.0, 0.0] {
            stream.extend_from_slice(&le_f32(value));
        }
    }
    append_e5_record(&mut stream, 0xfe, 3, &[]);

    assert!(crate::families::e5::records::e5_vertices(&stream, 1).is_empty());
}

#[test]
fn e5_vertices_concatenate_a_complete_split_roster() {
    let mut stream = Vec::new();
    for (record_id, coordinates) in [(1, [1.0f32, 2.0]), (2, [3.0, 4.0])] {
        append_e5_record(&mut stream, 0xfe, record_id, &[]);
        for coordinate in coordinates {
            stream.extend_from_slice(&[0x05, 0x08, 0x01]);
            for value in [coordinate, 0.0, 0.0] {
                stream.extend_from_slice(&le_f32(value));
            }
        }
    }
    append_e5_record(&mut stream, 0xfe, 3, &[]);

    let vertices = crate::families::e5::records::e5_vertices(&stream, 4);
    assert_eq!(
        vertices.iter().map(|point| point.x).collect::<Vec<_>>(),
        vec![1.0, 2.0, 3.0, 4.0]
    );
}
