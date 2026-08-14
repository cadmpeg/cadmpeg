// SPDX-License-Identifier: Apache-2.0
//! B5 dump tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn decode_reports_structurally_typed_unresolved_b5_faces() {
    let mut stream = b5_closed_triangle_stream();
    append_b5_record(
        &mut stream,
        0x5f,
        902,
        &[0x82, 0x18, 100, 0, 0x18, 0xe7, 0x03, 0x03],
    );
    append_b5_record(&mut stream, 0x5e, 903, &[]);
    let graph = crate::families::b5::graph::parse(&stream).expect("typed unresolved face graph");
    assert_eq!(graph.face_records.len(), 2);
    assert_eq!(graph.faces.len(), 1);
    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode typed unresolved face");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_FACE_COUNT),
        1
    );
}

#[test]
fn decode_reports_typed_distinct_surface_b5_faces() {
    let mut stream = b5_closed_triangle_stream();
    append_b5_record(&mut stream, 0x27, 101, &b5_plane_payload([0.0, 0.0, 1.0]));
    let mut face_payload = vec![0x83];
    face_payload.extend_from_slice(&b5_object_ref(100));
    face_payload.extend_from_slice(&b5_object_ref(101));
    face_payload.extend_from_slice(&b5_object_ref(400));
    face_payload.push(0x05);
    append_b5_record(&mut stream, 0x5f, 902, &face_payload);

    let graph = crate::families::b5::graph::parse(&stream).expect("typed multi-surface graph");
    assert_eq!(graph.face_records.len(), 2);
    assert_eq!(graph.faces.len(), 1);

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode typed multi-surface face");
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_MULTI_SURFACE_OBJECT_STREAM_FACE_COUNT),
        1
    );
}

#[test]
fn decode_reports_typed_b5_faces_without_a_resolved_topology_graph() {
    let mut stream = b2_sphere_stream();
    append_b5_record(&mut stream, 0x27, 100, &b5_plane_payload([0.0; 3]));
    append_b5_record(
        &mut stream,
        0x21,
        9,
        &b5_linear_pcurve_payload(100, [0.0, 0.0], [1.0, 0.0]),
    );
    append_b5_record(
        &mut stream,
        0x62,
        103,
        &[
            0x83, 0x89, 0x8a, 0xe4, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
            0x01,
        ],
    );
    append_b5_record(
        &mut stream,
        0x5f,
        101,
        &[0x82, 0x18, 100, 0, 0x18, 102, 0, 0x03],
    );
    append_b5_record(&mut stream, 0x5e, 102, &[]);
    append_b5_record(
        &mut stream,
        0x5e,
        104,
        &[0x85, 0x81, 0xe9, 0x83, 0x84, 0x85, 0x21],
    );
    append_b5_record(&mut stream, 0x5d, 105, &[0x81, 0x86, 0x04]);
    let mut incidence_payload = vec![0x81, 0x89, 0x81];
    incidence_payload.extend_from_slice(&le_f64(0.0));
    incidence_payload.push(0x81);
    append_b5_record(&mut stream, 0x06, 4, &incidence_payload);
    append_b5_record(&mut stream, 0x05, 6, &[0x81, 0x84]);
    assert!(crate::families::b5::graph::parse(&stream).is_none());
    assert_eq!(
        crate::families::b5::graph::typed_face_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_loop_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_edge_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_vertex_incidence_links(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_class_21_pcurves(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_parameter_incidences(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_vertex_incidence_rosters(&stream).len(),
        1
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode typed face without resolved topology");
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_FACE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_UNRESOLVED_OBJECT_STREAM_LOOP_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_21_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_04_COUNT
        ),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TYPED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_PARAMETER_INCIDENCE_MEMBER_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_ROSTER_MEMBER_COUNT
        ),
        1
    );
}
