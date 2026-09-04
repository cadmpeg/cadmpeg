// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `e5` family over synthetic byte fixtures.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SurfaceGeometry;

use crate::test_support::*;
use crate::variant::Variant;
use crate::CatiaCodec;

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

    let mut small = e5_circle_stream();
    small[86..94].copy_from_slice(&f64::from_bits(1).to_le_bytes());
    assert_eq!(crate::families::e5::records::e5_circles(&small).len(), 1);
    assert!(crate::families::e5::records::e5_surfaces(&small).is_empty());

    let mut zero = e5_circle_stream();
    zero[86..94].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::e5::records::e5_circles(&zero).is_empty());
    assert!(crate::families::e5::records::e5_surfaces(&zero).is_empty());
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
    for (id, start, end, bound) in [
        (100u8, 10u8, 20u8, [210u8, 0]),
        (101, 20, 30, [211, 0]),
        (102, 30, 10, [212, 0]),
    ] {
        let mut payload = vec![0x85, 0x08, 200, 0x08, start, 0x08, end];
        payload.extend_from_slice(&[0x18, bound[0], bound[1], 0x18, bound[0], bound[1]]);
        append_e5_record(&mut bytes, 0xff, u32::from(id), &payload);
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
    for (bound, pcurves) in [
        (210u32, [400u16, 410]),
        (211, [401, 411]),
        (212, [402, 412]),
    ] {
        let mut bound_payload = vec![0x82];
        for pcurve in pcurves {
            bound_payload.push(0x18);
            bound_payload.extend_from_slice(&pcurve.to_le_bytes());
        }
        bound_payload.push(0x82);
        bound_payload.extend_from_slice(&le_f64(0.5));
        bound_payload.extend_from_slice(&0_u32.to_le_bytes());
        bound_payload.extend_from_slice(&le_f64(0.5));
        bound_payload.extend_from_slice(&0_u32.to_le_bytes());
        append_e5_record(&mut bytes, 0x0e, bound, &bound_payload);
    }
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
    assert_eq!(
        topology.faces[0].loops[0]
            .members
            .iter()
            .map(|member| member.edge_use)
            .collect::<Vec<_>>(),
        vec![100, 101, 102]
    );
    assert_eq!(
        topology.faces[0].loops[0]
            .members
            .iter()
            .map(|member| member.reversed)
            .collect::<Vec<_>>(),
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
    assert_eq!(topology.curve_supports[&200].pcurves(), &[400, 410]);
    assert_eq!(topology.curve_supports[&200].range, [-10.0, 10.0]);
    assert!(matches!(
        topology.pcurves[&403],
        crate::families::e5::graph::E5Pcurve::Jet { ref sites, .. }
            if sites.iter().map(|site| site.knot).collect::<Vec<_>>() == [0.0, 1.0]
    ));

    let mut missing_support = bytes.clone();
    let support_start = missing_support
        .windows(4)
        .position(|window| window == [0xe5, 0x0d, 0x03, 0xc1])
        .expect("curve-support record");
    missing_support[support_start + 3] = 0x7f;
    assert!(crate::families::e5::graph::parse_topology(&missing_support).is_none());

    let mut missing_support_pcurve = bytes.clone();
    let support_start = missing_support_pcurve
        .windows(4)
        .position(|window| window == [0xe5, 0x0d, 0x03, 0xc1])
        .expect("curve-support record");
    missing_support_pcurve[support_start + 15..support_start + 17].copy_from_slice(&[0xff, 0x0f]);
    assert!(crate::families::e5::graph::parse_topology(&missing_support_pcurve).is_none());

    let mut missing_bounds = bytes.clone();
    let bounds_start = missing_bounds
        .windows(4)
        .position(|window| window == [0xe5, 0x0d, 0x03, 0x0e])
        .expect("parameter-bound record");
    missing_bounds[bounds_start + 3] = 0x7f;
    assert!(crate::families::e5::graph::parse_topology(&missing_bounds).is_none());

    let mut missing_pcurve_surface = bytes;
    let pcurve_start = missing_pcurve_surface
        .windows(13)
        .position(|window| {
            window.starts_with(&[0xe5, 0x0d, 0x03, 0x96])
                && u32::from_le_bytes(window[9..13].try_into().unwrap()) == 410
        })
        .expect("support pcurve record");
    missing_pcurve_surface[pcurve_start + 14..pcurve_start + 17]
        .copy_from_slice(&[0x18, 0x84, 0x03]);
    assert!(crate::families::e5::graph::parse_topology(&missing_pcurve_surface).is_none());
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

    let mut large = e5_torus_stream();
    large[110..118].copy_from_slice(&2_000_000.0_f64.to_le_bytes());
    large[118..126].copy_from_slice(&1_500_000.0_f64.to_le_bytes());
    assert!(matches!(
        crate::families::e5::records::e5_surfaces(&large)[0].geometry,
        SurfaceGeometry::Torus {
            major_radius: 2_000_000.0,
            minor_radius: 1_500_000.0,
            ..
        }
    ));

    let mut tiny = e5_torus_stream();
    tiny[110..118].copy_from_slice(&f64::from_bits(1).to_le_bytes());
    tiny[118..126].copy_from_slice(&f64::from_bits(1).to_le_bytes());
    assert!(crate::families::e5::records::e5_surfaces(&tiny).is_empty());
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

#[test]
fn decode_e5_stream_transfers_circle_carrier() {
    let scan = crate::container::scan_bytes(e5_catpart());
    assert_eq!(scan.variant, Variant::E5Stream);
    let mut cur = Cursor::new(e5_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.curves.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 2);
    assert!(result.ir().model.edges.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
    assert!(matches!(
        result.ir().model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle { .. }
    ));
    assert!(result.ir().native_unknowns("catia").unwrap()[0]
        .links
        .contains(&"catia:e5:surf#0".to_string()));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_e5_stream_transfers_standalone_d8_carrier() {
    const TEST_TOLERANCE: f64 = 1e-12;
    let mut stream = e5_d8_rolling_ball_stream();
    for id in 100..109 {
        append_e5_record(&mut stream, 0xfe, id, &[]);
    }
    let file = object_main_catpart(&stream);
    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("E5 D8 decode");

    assert_eq!(result.ir().model.surfaces.len(), 1);
    let [procedural] = result.ir().model.procedural_surfaces.as_slice() else {
        panic!("one standalone rolling-ball construction");
    };
    assert_eq!(
        result.ir().model.procedural_surface_owner(&procedural.id),
        Some(&result.ir().model.surfaces[0].id)
    );
    let surface = &result.ir().model.surfaces[0];
    assert!(matches!(
        &surface.geometry,
        SurfaceGeometry::Procedural { construction, .. } if construction == &procedural.id
    ));
    assert!(matches!(
        procedural.definition(),
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
            degree: 5,
            ref knots,
            ref multiplicities,
            ref sites,
        } if knots == &[2.0, 5.0] && multiplicities == &[6, 6] && sites.len() == 2
    ));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
    let point = cadmpeg_ir::eval::model_surface_point(result.ir(), &surface.geometry, 2.0, 0.5)
        .expect("D8 surface point");
    let expected = 2.0_f64.sqrt();
    assert!((point.x - expected).abs() < TEST_TOLERANCE);
    assert!((point.y - expected).abs() < TEST_TOLERANCE);
    assert!(point.z.abs() < TEST_TOLERANCE);

    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_e5_stream_transfers_reference_closed_torus_topology() {
    let stream = e5_torus_topology_stream();
    crate::families::e5::graph::parse_topology(&stream).expect("generated E5 topology");
    let file = object_main_catpart(&stream);
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::E5Stream
    );

    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(
        result.ir().model.loops[0].boundary_role_in(&result.ir().model.faces),
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert_eq!(result.ir().model.coedges.len(), 4);
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_eq!(result.ir().model.pcurves.len(), 4);
    assert_eq!(result.ir().model.curves.len(), 4);
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert!(matches!(
        result.ir().model.procedural_curves[0].definition(),
        cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
            family: cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric { .. },
        }
    ));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_some() && edge.param_range.is_some()));
    assert!(result.report().losses.iter().all(|loss| {
        loss.code.category() != cadmpeg_ir::report::LossCategory::Topology
            || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::report::LossCategory::Topology
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("two trailing orientation signs")
    }));

    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_e5_stream_binds_file_level_vertex_run() {
    let mut stream = e5_torus_topology_stream();
    let vertex_start = stream
        .windows(3)
        .position(|bytes| bytes == [0x05, 0x08, 0x01])
        .expect("E5 vertex run");
    let vertex_bytes = stream
        .drain(vertex_start..vertex_start + 4 * 15)
        .collect::<Vec<_>>();

    stream.extend_from_slice(b"FINJPL  ");
    stream.extend_from_slice(&0x0000_0080u32.to_be_bytes());
    stream.extend_from_slice(&vertex_bytes);
    let file = object_main_catpart(&stream);
    let vertex_file_start = file
        .windows(vertex_bytes.len())
        .position(|bytes| bytes == vertex_bytes)
        .expect("file-level E5 vertex run");

    let record_range = crate::container::e5_record_stream(&file).expect("coherent E5 walk");
    assert!(!record_range.contains(&vertex_file_start));
    assert!(crate::families::e5::records::e5_vertices(&file[record_range], 4).is_empty());
    assert_eq!(crate::families::e5::records::e5_vertices(&file, 4).len(), 4);
    let scan = crate::container::scan_bytes(file.clone());
    assert_eq!(scan.variant, Variant::E5Stream);

    let result = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("E5 decode");
    assert_eq!(result.ir().model.points.len(), 4);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 4);
}
