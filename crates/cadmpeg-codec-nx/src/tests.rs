// SPDX-License-Identifier: Apache-2.0
//! Tests over synthetic byte fixtures. No real CAD file exists in this repo and
//! none may be added, so every fixture is a hand-built `.prt` byte image whose
//! bytes exercise the real SPLMSSTR container parse, the Parasolid zlib
//! extraction/classification, and the analytic geometry decode, and fail if the
//! code regresses.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::Exactness;

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::NxCodec;

const MAGIC: &[u8; 8] = b"SPLMSSTR";

fn be_f64(v: f64) -> [u8; 8] {
    v.to_be_bytes()
}

/// Write three big-endian doubles into `rec` starting at `at`.
fn put_vec3(rec: &mut [u8], at: usize, xyz: [f64; 3]) {
    for (i, v) in xyz.iter().enumerate() {
        rec[at + 8 * i..at + 8 * i + 8].copy_from_slice(&be_f64(*v));
    }
}

fn put_f64(rec: &mut [u8], at: usize, v: f64) {
    rec[at..at + 8].copy_from_slice(&be_f64(v));
}

fn put_ref(rec: &mut [u8], at: usize, value: u16) {
    rec[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

/// One fixed-length analytic record: a `00 <tag>` header then zeroed payload the
/// caller fills at the documented offsets.
fn record(tag: u8, len: usize) -> Vec<u8> {
    let mut r = vec![0u8; len];
    r[0] = 0x00;
    r[1] = tag;
    r
}

fn indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 32];
    let base = 8usize;
    let class_name = b"UGS::ModlFeature";
    bytes[base] = class_name.len() as u8;
    bytes[base + 1..base + 1 + class_name.len()].copy_from_slice(class_name);
    bytes[base + 1 + class_name.len()] = 0x81;
    let root = b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables";
    let text = b"(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; ";
    let mut expression = vec![0x99, 0x04, (text.len() + 2) as u8];
    expression.extend_from_slice(text);
    expression.push(0);
    let records = [root.as_slice(), expression.as_slice()];
    let table = bytes.len() + 4 * 4;
    let table_end = table + 4 + 3 * 4;
    let first = table_end - base;
    let second = first + records[0].len();
    let end = second + records[1].len();
    for value in [0u32, first as u32, second as u32, end as u32] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for id in [0x100u32, 0x101, 0x102] {
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes.extend_from_slice(records[0]);
    bytes.extend_from_slice(records[1]);
    bytes
}

#[test]
fn om_index_pairs_object_ids_with_bounded_entity_records() {
    let bytes = indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 8);
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].object_id, 0x101);
    assert_eq!(
        sections[0].records[0].bytes,
        b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables"
    );
    assert_eq!(sections[0].records[1].object_id, 0x102);
    assert_eq!(
        sections[0].records[1].bytes,
        b"\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00"
    );
}

#[test]
fn om_registry_uses_length_framing_and_stays_outside_entity_payloads() {
    let mut bytes = indexed_om_section();
    bytes.extend_from_slice(b"\x10UGS::PayloadText");
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].types.len(), 1);
    assert_eq!(sections[0].types[0].name, "UGS::ModlFeature");
    assert_eq!(sections[0].types[0].class_id, 0x81);
    assert_eq!(sections[0].types[0].offset, 8);
}

#[test]
fn om_numeric_expression_retains_identity_name_unit_and_value() {
    let bytes = indexed_om_section();
    let section = crate::om::indexed_sections(&bytes).remove(0);
    let expressions = section.numeric_expressions();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, 0x102);
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].unit, crate::om::ExpressionUnit::Degree);
    assert_eq!(expressions[0].value, 120.0);
}

/// A synthetic Parasolid partition stream: the `PS 00 00` header, a prologue with
/// a `(partition)` subtype and a schema token, then one POINT, one PLANE, one
/// CYLINDER, and one LINE record laid out back-to-back at their fixed lengths.
fn partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(b"XX: TRANSMIT FILE (partition) created by modeller version 3400176\x00");
    s.extend_from_slice(b"SCH_TEST_1_9999\x00");

    // POINT (type 29): xyz at +16, metres.
    let mut pt = record(0x1d, 40);
    put_vec3(&mut pt, 16, [0.0625, 0.0, 0.0127]); // 62.5, 0, 12.7 mm
    s.extend_from_slice(&pt);

    // PLANE (type 50): origin +19, normal +43, x_axis +67.
    let mut pl = record(0x32, 91);
    pl[18] = b'+';
    put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]); // 76.2 mm
    put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&pl);

    // CYLINDER (type 51): origin +19, axis +43, radius +67, x_axis +75.
    let mut cy = record(0x33, 99);
    cy[18] = b'+';
    put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, 0.004_05); // 4.05 mm
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&cy);

    // LINE (type 30): point +19, direction +43.
    let mut ln = record(0x1e, 67);
    ln[18] = b'+';
    put_vec3(&mut ln, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut ln, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&ln);

    s
}

/// A complete one-face Parasolid topology. Every ownership and geometry link is
/// a small XMT reference, so this generated fixture exercises the codec's
/// connected-B-rep path without depending on an external CAD file.
fn topology_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(
        b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );

    let mut body = record(12, 24);
    put_ref(&mut body, 2, 2);
    s.extend_from_slice(&body);

    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    put_ref(&mut shell, 10, 2); // body
    put_ref(&mut shell, 14, 4); // first face
    s.extend_from_slice(&shell);

    let mut face = record(14, 39);
    put_ref(&mut face, 2, 4);
    put_f64(&mut face, 10, 0.000_2); // 0.2 mm
    put_ref(&mut face, 22, 5); // loop
    put_ref(&mut face, 24, 3); // shell
    put_ref(&mut face, 26, 6); // plane
    face[28] = b'+';
    s.extend_from_slice(&face);

    let mut loop_ = record(15, 16);
    put_ref(&mut loop_, 2, 5);
    put_ref(&mut loop_, 10, 7); // fin
    put_ref(&mut loop_, 12, 4); // face
    s.extend_from_slice(&loop_);

    let mut fin = record(17, 23);
    put_ref(&mut fin, 2, 7);
    put_ref(&mut fin, 6, 5); // loop
    put_ref(&mut fin, 8, 7); // next (one-fin ring)
    put_ref(&mut fin, 10, 7); // previous
    put_ref(&mut fin, 12, 10); // vertex
    put_ref(&mut fin, 16, 8); // edge
    put_ref(&mut fin, 18, 9); // curve
    fin[22] = b'+';
    s.extend_from_slice(&fin);

    let mut edge = record(16, 32);
    put_ref(&mut edge, 2, 8);
    put_f64(&mut edge, 10, 0.000_3); // 0.3 mm
    put_ref(&mut edge, 18, 7); // fin
    put_ref(&mut edge, 24, 9); // curve
    s.extend_from_slice(&edge);

    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 6);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&plane);

    let mut line = record(30, 67);
    put_ref(&mut line, 2, 9);
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut line, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&line);

    let mut vertex = record(18, 28);
    put_ref(&mut vertex, 2, 10);
    put_ref(&mut vertex, 16, 11); // point
    put_f64(&mut vertex, 18, 0.000_1); // 0.1 mm
    s.extend_from_slice(&vertex);

    let mut point = record(29, 40);
    put_ref(&mut point, 2, 11);
    put_vec3(&mut point, 16, [0.01, 0.02, 0.03]);
    s.extend_from_slice(&point);
    s
}

fn offset_surface_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    put_ref(&mut stream, face + 26, 12);

    let mut offset = record(60, 39);
    put_ref(&mut offset, 2, 12);
    offset[18] = b'+';
    offset[19] = b'V';
    offset[20] = 1;
    put_ref(&mut offset, 21, 6);
    put_f64(&mut offset, 23, 0.002_5);
    stream.extend(offset);
    stream
}

fn surface_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 12);
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 9);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_01);
    stream.extend(surface_curve);
    stream
}

fn blend_surface_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    put_ref(&mut stream, face + 26, 12);

    let mut blend = record(56, 66);
    put_ref(&mut blend, 2, 12);
    blend[18] = b'+';
    blend[19] = b'R';
    put_ref(&mut blend, 20, 6);
    put_ref(&mut blend, 22, 6);
    put_ref(&mut blend, 24, 1);
    put_f64(&mut blend, 26, -0.003);
    put_f64(&mut blend, 34, 0.003);
    put_f64(&mut blend, 42, 1.0);
    put_f64(&mut blend, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut blend, at, 1);
    }
    stream.extend(blend);
    stream
}

fn blend_surface_with_extended_support_reference() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    stream.splice(blend + 20..blend + 22, [0xff, 0xfa, 0x00, 0x00]);
    stream
}

fn intersection_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 12);
    intersection[18] = b'+';
    for (index, reference) in [6, 6, 1, 1, 1, 1].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);
    stream
}

fn charted_intersection_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }

    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 12);
    intersection[18] = b'+';
    for (index, reference) in [6, 1, 20, 21, 22, 23].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);

    let mut chart = record(40, 108);
    chart[2..6].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut chart, 6, 20);
    put_f64(&mut chart, 8, 0.0);
    put_f64(&mut chart, 16, 1.0);
    chart[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_f64(&mut chart, 28, 0.000_01);
    put_f64(&mut chart, 36, 0.001);
    put_f64(&mut chart, 44, -31_415_800_000_000.0);
    put_f64(&mut chart, 52, -31_415_800_000_000.0);
    put_vec3(&mut chart, 60, [0.0, 0.0, 0.0]);
    put_vec3(&mut chart, 84, [0.01, 0.0, 0.0]);
    stream.extend(chart);

    for (xmt, point) in [(21, [0.0, 0.0, 0.0]), (22, [0.01, 0.0, 0.0])] {
        let mut term = record(41, 34);
        term[2..6].copy_from_slice(&1u32.to_be_bytes());
        put_ref(&mut term, 6, xmt);
        term[8..10].copy_from_slice(b"L?");
        put_vec3(&mut term, 10, point);
        stream.extend(term);
    }

    let mut uv = record(204, 41);
    uv[2..6].copy_from_slice(&4u32.to_be_bytes());
    put_ref(&mut uv, 6, 23);
    uv[8] = 2;
    for (index, value) in [0.0, 0.0, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut uv, 9 + index * 8, value);
    }
    stream.extend(uv);
    stream
}

fn ext11_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    let mut entries = vec![0u8; 2 * 11 * 8];
    for (index, point) in [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0]].into_iter().enumerate() {
        let at = index * 88;
        put_vec3(&mut entries, at, point);
        put_vec3(&mut entries, at + 56, [1.0, 0.0, 0.0]);
        put_f64(&mut entries, at + 80, index as f64 * 0.01);
    }
    stream.splice(chart + 60..chart + 108, entries);
    stream
}

fn two_support_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);

    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 4, 0, 23])
        .expect("UV record");
    stream[uv + 2..uv + 6].copy_from_slice(&8u32.to_be_bytes());
    stream[uv + 8] = 4;
    let mut values = vec![0u8; 8 * 8];
    for (index, value) in [0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.01, 0.0]
        .into_iter()
        .enumerate()
    {
        put_f64(&mut values, index * 8, value);
    }
    stream.splice(uv + 9..uv + 41, values);

    let mut second_plane = record(50, 91);
    put_ref(&mut second_plane, 2, 13);
    second_plane[18] = b'+';
    put_vec3(&mut second_plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut second_plane, 43, [0.0, 1.0, 0.0]);
    put_vec3(&mut second_plane, 67, [1.0, 0.0, 0.0]);
    stream.extend(second_plane);
    stream
}

fn blend_bound_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 14);

    let mut bridge = record(59, 24);
    put_ref(&mut bridge, 2, 14);
    bridge[4..8].copy_from_slice(&9u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut bridge, at, 1);
    }
    bridge[18] = b'+';
    put_ref(&mut bridge, 19, 0);
    put_ref(&mut bridge, 21, 13);
    stream.extend(bridge);
    stream
}

fn inline_descriptor_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 4, 0, 23])
        .expect("UV record");
    let mut inline_uv = b"values\x00\x00\x00\x02\x01\x66\x01".to_vec();
    inline_uv.extend_from_slice(&4u32.to_be_bytes());
    inline_uv.extend_from_slice(&23u16.to_be_bytes());
    inline_uv.push(2);
    for value in [0.0_f64, 0.0, 0.01, 0.0] {
        inline_uv.extend_from_slice(&value.to_be_bytes());
    }
    stream.splice(uv..uv + 41, inline_uv);

    for (xmt, point) in [(22u16, [0.01_f64, 0.0, 0.0]), (21, [0.0, 0.0, 0.0])] {
        let marker = [0, 41, 0, 0, 0, 1, (xmt >> 8) as u8, xmt as u8];
        let term = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("term record");
        let mut inline = b"term_use\x00\x00\x00\x01\x01\x63\x43\x5a".to_vec();
        inline.extend_from_slice(&1u32.to_be_bytes());
        inline.extend_from_slice(&xmt.to_be_bytes());
        inline.extend_from_slice(b"L?");
        for coordinate in point {
            inline.extend_from_slice(&coordinate.to_be_bytes());
        }
        stream.splice(term..term + 34, inline);
    }
    stream
}

fn deltas_intersection_curve_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let subtype = stream
        .windows(b"(partition)".len())
        .position(|window| window == b"(partition)")
        .expect("partition subtype");
    stream.splice(
        subtype..subtype + b"(partition)".len(),
        b"(deltas)".iter().copied(),
    );
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    stream.extend_from_slice(b"intersection_data");
    stream.push(0x5a);
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream
}

fn status_framed_deltas_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    let mut face = Vec::new();
    face.extend_from_slice(&14u16.to_be_bytes());
    face.extend_from_slice(&100u16.to_be_bytes());
    face.extend_from_slice(&7u32.to_be_bytes());
    let push_ref = |record: &mut Vec<u8>, reference: u16| {
        record.extend_from_slice(&reference.to_be_bytes());
        record.push(1);
    };
    push_ref(&mut face, 1);
    face.extend_from_slice(&(-31_415_800_000_000.0f64).to_be_bytes());
    for reference in [1u16; 5] {
        push_ref(&mut face, reference);
    }
    face.push(b'+');
    for reference in [1u16; 5] {
        push_ref(&mut face, reference);
    }
    stream.extend(face);
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&50_000u16.to_be_bytes());
    stream.extend_from_slice(&[0, 1]);
    stream
}

fn variable_status_framed_deltas_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&(-100i16).to_be_bytes());
    stream.extend_from_slice(&0u16.to_be_bytes());
    stream.extend_from_slice(&8u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&101u16.to_be_bytes());
    stream.extend_from_slice(&9u32.to_be_bytes());
    for reference in [1u16, 2] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn status_framed_deltas_point_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&29u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&900u32.to_be_bytes());
    for reference in [1u16; 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for value in [0.0125f64, -0.002, 0.004] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn deltas_point_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend(status_framed_deltas_point_stream());
    stream
}

fn deltas_edge_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&8u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_9f64.to_be_bytes());
    for reference in [7u16, 1, 1, 9, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_face_vertex_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&14u16.to_be_bytes());
    stream.extend_from_slice(&4u16.to_be_bytes());
    stream.extend_from_slice(&902u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_8f64.to_be_bytes());
    for reference in [1u16, 1, 5, 3, 6] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }

    stream.extend_from_slice(&18u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&903u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 11] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&0.000_7f64.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream
}

fn deltas_loop_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&5u16.to_be_bytes());
    stream.extend_from_slice(&904u32.to_be_bytes());
    for reference in [1u16, 7, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_shell_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&3u16.to_be_bytes());
    stream.extend_from_slice(&905u32.to_be_bytes());
    for reference in [1u16, 2, 1, 4, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_fin_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&7u16.to_be_bytes());
    for reference in [1u16, 5, 7, 7, 10, 1, 8, 9, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    stream
}

fn deltas_line_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&30u16.to_be_bytes());
    stream.extend_from_slice(&9u16.to_be_bytes());
    stream.extend_from_slice(&906u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.004f64, 0.005, 0.006, 0.0, 1.0, 0.0] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn deltas_plane_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&6u16.to_be_bytes());
    stream.extend_from_slice(&907u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn circle_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (kind, xmt, field) in [(16u8, 8u8, 24usize), (17, 7, 18)] {
        let record = stream
            .windows(4)
            .position(|window| window == [0, kind, 0, xmt])
            .expect("topology record");
        put_ref(&mut stream, record + field, 12);
    }
    let mut circle = record(31, 99);
    put_ref(&mut circle, 2, 12);
    circle[18] = b'+';
    put_vec3(&mut circle, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut circle, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut circle, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut circle, 91, 0.01);
    stream.extend(circle);
    stream
}

fn deltas_circle_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&31u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&908u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.025] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn ellipse_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (kind, xmt, field) in [(16u8, 8u8, 24usize), (17, 7, 18)] {
        let record = stream
            .windows(4)
            .position(|window| window == [0, kind, 0, xmt])
            .expect("topology record");
        put_ref(&mut stream, record + field, 13);
    }
    let mut ellipse = record(32, 107);
    put_ref(&mut ellipse, 2, 13);
    ellipse[18] = b'+';
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.02);
    put_f64(&mut ellipse, 99, 0.01);
    stream.extend(ellipse);
    stream
}

fn deltas_ellipse_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&32u16.to_be_bytes());
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&909u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [
        0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.03, 0.012,
    ] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn cylinder_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face");
    put_ref(&mut stream, face + 26, 12);
    let mut cylinder = record(51, 99);
    put_ref(&mut cylinder, 2, 12);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, 0.01);
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);
    stream.extend(cylinder);
    stream
}

fn deltas_cylinder_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&51u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&910u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 0.025, 1.0, 0.0, 0.0] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn cone_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face");
    put_ref(&mut stream, face + 26, 12);
    let mut cone = record(52, 115);
    put_ref(&mut cone, 2, 12);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.01);
    put_f64(&mut cone, 75, 0.0);
    put_f64(&mut cone, 83, 1.0);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    stream.extend(cone);
    stream
}

fn deltas_cone_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&52u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&911u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [
        0.001f64,
        0.002,
        0.003,
        0.0,
        1.0,
        0.0,
        0.025,
        0.5,
        3.0f64.sqrt() / 2.0,
        1.0,
        0.0,
        0.0,
    ] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn sphere_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face");
    put_ref(&mut stream, face + 26, 12);
    let mut sphere = record(53, 99);
    put_ref(&mut sphere, 2, 12);
    sphere[18] = b'+';
    put_vec3(&mut sphere, 19, [0.0, 0.0, 0.0]);
    put_f64(&mut sphere, 43, 0.01);
    put_vec3(&mut sphere, 51, [0.0, 0.0, 1.0]);
    put_vec3(&mut sphere, 75, [1.0, 0.0, 0.0]);
    stream.extend(sphere);
    stream
}

fn deltas_sphere_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&53u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&912u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [0.001f64, 0.002, 0.003, 0.025, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn torus_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face");
    put_ref(&mut stream, face + 26, 12);
    let mut torus = record(54, 107);
    put_ref(&mut torus, 2, 12);
    torus[18] = b'+';
    put_vec3(&mut torus, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut torus, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut torus, 67, 0.03);
    put_f64(&mut torus, 75, 0.01);
    put_vec3(&mut torus, 83, [1.0, 0.0, 0.0]);
    stream.extend(torus);
    stream
}

fn deltas_torus_partition_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&54u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&913u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for value in [
        0.001f64, 0.002, 0.003, 0.0, 1.0, 0.0, 0.04, 0.015, 1.0, 0.0, 0.0,
    ] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn bspline_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00XX: TRANSMIT FILE (partition)\x00SCH_TEST_1_9999\x00");
    let mut surface = record(124, 23);
    put_ref(&mut surface, 2, 10);
    surface[18] = b'+';
    put_ref(&mut surface, 19, 20);
    put_ref(&mut surface, 21, 21);
    s.extend(surface);

    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 20);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 21);
    s.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 21);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    s.extend(data);

    for (tag, reference, values) in [(127, 30, vec![2u16, 2]), (127, 31, vec![2, 2])] {
        let mut array = record(tag, 8 + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        put_ref(&mut array, 6, reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 8 + index * 2, value);
        }
        s.extend(array);
    }
    for reference in [32, 33] {
        let mut array = record(128, 8 + 2 * 8);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        put_f64(&mut array, 8, 0.0);
        put_f64(&mut array, 16, 1.0);
        s.extend(array);
    }

    let mut curve = record(134, 23);
    put_ref(&mut curve, 2, 50);
    curve[18] = b'+';
    put_ref(&mut curve, 19, 40);
    put_ref(&mut curve, 21, 41);
    s.extend(curve);
    let mut curve_descriptor = record(136, 27);
    put_ref(&mut curve_descriptor, 2, 40);
    put_ref(&mut curve_descriptor, 4, 1);
    put_ref(&mut curve_descriptor, 8, 2);
    put_ref(&mut curve_descriptor, 10, 3);
    put_ref(&mut curve_descriptor, 14, 2);
    curve_descriptor[16] = 5;
    put_ref(&mut curve_descriptor, 23, 42);
    put_ref(&mut curve_descriptor, 25, 43);
    s.extend(curve_descriptor);
    let mut curve_data = record(135, 15 + 6 * 8);
    put_ref(&mut curve_data, 2, 41);
    curve_data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.0, 0.0].into_iter().enumerate() {
        put_f64(&mut curve_data, 15 + index * 8, value);
    }
    s.extend(curve_data);
    for (tag, reference) in [(127, 42), (128, 43)] {
        let mut array = record(tag, if tag == 127 { 12 } else { 24 });
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        if tag == 127 {
            put_ref(&mut array, 8, 2);
            put_ref(&mut array, 10, 2);
        } else {
            put_f64(&mut array, 8, 0.0);
            put_f64(&mut array, 16, 1.0);
        }
        s.extend(array);
    }
    s
}

fn bspline_surface_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 60);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 61);
    stream.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 61);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.03, 0.0, 0.015, 0.0, 0.0, 0.015, 0.03, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    stream.extend(data);
    stream
}

fn deltas_bspline_surface_wrapper_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&124u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&914u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [60u16, 61] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn bspline_curve_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(136, 27);
    put_ref(&mut descriptor, 2, 70);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 3);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    put_ref(&mut descriptor, 23, 42);
    put_ref(&mut descriptor, 25, 43);
    stream.extend(descriptor);

    let mut data = record(135, 15 + 6 * 8);
    put_ref(&mut data, 2, 71);
    data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut data, 15 + index * 8, value);
    }
    stream.extend(data);
    stream
}

fn deltas_bspline_curve_wrapper_stream() -> Vec<u8> {
    let mut stream =
        b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00".to_vec();
    stream.extend_from_slice(&134u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&915u32.to_be_bytes());
    for reference in [1u16; 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [70u16, 71] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 12);
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    trim[18] = b'+';
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.25);
    put_f64(&mut trim, 77, 0.75);
    // The closed edge's single vertex sits at the trim range's midpoint on the
    // basis line so both trimmed endpoints fall inside the edge's stored
    // 0.3 mm tolerance; the point record is the topology stream's last
    // 40 bytes, before the trim record is appended.
    let point_vec = stream.len() - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.0, 0.0]);
    stream.extend(trim);
    stream
}

fn topology_with_extended_edge_curve_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream[edge + 24..edge + 26].copy_from_slice(&(-9i16).to_be_bytes());
    stream.splice(edge + 26..edge + 26, [0, 0]);
    stream
}

fn topology_with_extended_face_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    stream.splice(face + 8..face + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

fn topology_with_extended_edge_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream.splice(edge + 8..edge + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

fn topology_with_extended_internal_topology_references() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(13, 3, 8), (15, 5, 8), (17, 7, 4), (18, 10, 8), (29, 11, 8)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        stream.splice(
            record + offset..record + offset + 2,
            [0xff, 0xff, 0x00, 0x00],
        );
    }
    stream
}

fn topology_with_fully_extended_geometry_headers() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt) in [(50, 6), (30, 9)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        for index in 0..5 {
            let at = record + 8 + index * 4;
            stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
        }
    }
    stream
}

fn offset_surface_with_fully_extended_common_header() -> Vec<u8> {
    let mut stream = offset_surface_topology_partition_stream();
    let record = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
    stream
}

fn fully_extend_common_header(stream: &mut Vec<u8>, marker: [u8; 4]) {
    let record = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("compact geometry record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
}

fn zlib_compress(raw: &[u8]) -> Vec<u8> {
    // Level 1 emits the `78 01` zlib header NX/Parasolid streams use.
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

fn zlib_compress_at_level(raw: &[u8], level: u32) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(level));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

/// Assemble a synthetic single-part `.prt`: the SPLMSSTR header, a HEADER
/// directory with one `/Root/UG_PART/UG_PART` file entry, and a zlib-compressed
/// Parasolid partition stream.
fn single_part_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06); // version tag
    f.extend_from_slice(&[0x11, 0x22, 0x33]); // u24 file tag
    f.extend_from_slice(&[0, 0, 0, 0]); // +0x0c constant
    f.push(0x00); // +0x10 constant
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // +0x11 footer offset (0 → no footer)
    f.extend_from_slice(&[0, 0]); // pad to 0x19
    assert_eq!(f.len(), 0x19);

    // HEADER directory: one file entry naming the canonical part stream.
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/UG_PART";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    // 16-byte payload: file_offset then size (both u64 LE) — point at the zlib blob.
    let blob = zlib_compress(&partition_stream());
    // The blob will be appended after the directory; compute its offset now.
    let dir_end = f.len() + 16; // after this entry's payload
    let blob_off = dir_end as u64;
    f.extend_from_slice(&blob_off.to_le_bytes());
    f.extend_from_slice(&(blob.len() as u64).to_le_bytes());
    f.extend_from_slice(&blob);
    f
}

fn prt_with_named_payloads(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let mut spans = Vec::new();
    for (name, _) in entries {
        file.extend_from_slice(&(name.len() as u32).to_le_bytes());
        file.extend_from_slice(name.as_bytes());
        spans.push(file.len());
        file.extend_from_slice(&[0; 16]);
    }
    for ((_, payload), span) in entries.iter().zip(spans) {
        let offset = file.len();
        file.extend_from_slice(payload);
        file[span..span + 8].copy_from_slice(&(offset as u64).to_le_bytes());
        file[span + 8..span + 16].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    }
    file
}

fn prt_with_arrangements() -> Vec<u8> {
    prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&partition_stream()),
        ),
        (
            "/Root/part/arrangements",
            br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="NO" Name="Exploded"/></Arrangements>"#.to_vec(),
        ),
    ])
}

fn topology_part_prt() -> Vec<u8> {
    prt_with_partition(&topology_partition_stream())
}

fn topology_with_missing_tolerances() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(14, 4, 10), (16, 8, 10), (18, 10, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_f64(&mut stream, record + offset, -31_415_800_000_000.0);
    }
    stream
}

fn prt_with_partition(stream: &[u8]) -> Vec<u8> {
    let mut f = single_part_prt();
    let compressed = zlib_compress(stream);
    let entry = container::scan_bytes(f.clone()).unwrap().entries.remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, f.len());
    f.truncate(offset as usize);
    f.extend_from_slice(&compressed);
    let size_at = offset as usize - 8;
    f[size_at..size_at + 8].copy_from_slice(&(compressed.len() as u64).to_le_bytes());
    f
}

fn prt_with_streams(streams: &[&[u8]]) -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let payload = streams
        .iter()
        .flat_map(|stream| zlib_compress(stream))
        .collect::<Vec<_>>();
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_indexed_om_section() -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let mut payload = indexed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

fn large_xmt_headers(stream: &[u8]) -> Vec<u8> {
    let marker = b"SCH_TEST_1_9999\x00";
    let start = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    let lengths = [24, 24, 39, 16, 23, 32, 91, 67, 28, 40];
    let mut out = stream[..start].to_vec();
    let mut pos = start;
    for len in lengths {
        let record = &stream[pos..pos + len];
        let xmt = u16::from_be_bytes([record[2], record[3]]);
        out.extend_from_slice(&record[..2]);
        out.extend_from_slice(&(-(i16::try_from(xmt).unwrap())).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&record[4..]);
        pos += len;
    }
    out
}

/// A synthetic assembly `.prt`: SPLMSSTR header, an `ExternalReferences` file
/// entry, and no embedded Parasolid stream.
fn assembly_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0, 0, 0]);
    f.extend_from_slice(&[0, 0, 0, 0]);
    f.push(0x00);
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    f.extend_from_slice(&[0u8; 16]); // opaque directory payload
    f
}

fn assembly_with_external_paths() -> Vec<u8> {
    let payload = b"EXTREFSTREAM\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    let offset = f.len() + 16;
    f.extend_from_slice(&(offset as u64).to_le_bytes());
    f.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    f.extend_from_slice(payload);
    f
}

fn rmfastload_prt() -> Vec<u8> {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1..=50u32 {
        payload.extend_from_slice(&id.to_le_bytes());
    }
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(6);
    f.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/FastLoad/RMFastLoad";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    let offset = f.len() + 16;
    f.extend_from_slice(&(offset as u64).to_le_bytes());
    f.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    f.extend(payload);
    f
}

fn many_face_partition_stream(node_id_start: u32) -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(
        b"PS\x00\x00XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );
    let mut body = record(12, 24);
    put_ref(&mut body, 2, 2);
    body[4..8].copy_from_slice(&(node_id_start + 100).to_be_bytes());
    stream.extend(body);
    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    shell[4..8].copy_from_slice(&(node_id_start + 101).to_be_bytes());
    put_ref(&mut shell, 10, 2);
    put_ref(&mut shell, 14, 300);
    stream.extend(shell);
    for index in 0..50u16 {
        let mut face = record(14, 39);
        put_ref(&mut face, 2, 300 + index);
        face[4..8].copy_from_slice(&(node_id_start + u32::from(index)).to_be_bytes());
        put_f64(&mut face, 10, 0.000_1);
        put_ref(&mut face, 22, 1);
        put_ref(&mut face, 24, 3);
        put_ref(&mut face, 26, 500 + index);
        face[28] = b'+';
        stream.extend(face);
    }
    for index in 0..50u16 {
        let mut plane = record(50, 91);
        put_ref(&mut plane, 2, 500 + index);
        plane[18] = b'+';
        put_vec3(&mut plane, 19, [f64::from(index) * 0.001, 0.0, 0.0]);
        put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
        put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
        stream.extend(plane);
    }
    stream
}

fn prt_with_two_bodies_and_rmfastload() -> Vec<u8> {
    let mut part_payload = zlib_compress(&many_face_partition_stream(1_000));
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    rm_payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1_000..1_050u32 {
        rm_payload.extend_from_slice(&id.to_le_bytes());
    }

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(6);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let part_name = b"/Root/UG_PART/UG_PART";
    file.extend_from_slice(&(part_name.len() as u32).to_le_bytes());
    file.extend_from_slice(part_name);
    let part_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let rm_name = b"/Root/FastLoad/RMFastLoad";
    file.extend_from_slice(&(rm_name.len() as u32).to_le_bytes());
    file.extend_from_slice(rm_name);
    let rm_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let part_offset = file.len();
    file.extend_from_slice(&part_payload);
    let rm_offset = file.len();
    file.extend_from_slice(&rm_payload);
    file[part_span..part_span + 8].copy_from_slice(&(part_offset as u64).to_le_bytes());
    file[part_span + 8..part_span + 16].copy_from_slice(&(part_payload.len() as u64).to_le_bytes());
    file[rm_span..rm_span + 8].copy_from_slice(&(rm_offset as u64).to_le_bytes());
    file[rm_span + 8..rm_span + 16].copy_from_slice(&(rm_payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_weak_rmfastload_overlap() -> Vec<u8> {
    let mut file = prt_with_two_bodies_and_rmfastload();
    let marker = b"UGS::Solid::Topol";
    let payload = file
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("RMFastLoad payload")
        + marker.len()
        + 4;
    for index in 0..50u32 {
        let id = if index < 5 {
            1_000 + index
        } else {
            10_000 + index
        };
        let at = payload + index as usize * 4;
        file[at..at + 4].copy_from_slice(&id.to_le_bytes());
    }
    file
}

#[test]
fn detect_high_on_magic() {
    assert_eq!(NxCodec.detect(MAGIC), Confidence::High);
    assert_eq!(NxCodec.detect(&single_part_prt()), Confidence::High);
    assert_eq!(NxCodec.detect(b"PK\x03\x04 not nx"), Confidence::No);
    // A Creo/Granite .prt shares the extension but not the magic.
    assert_eq!(NxCodec.detect(b"\xe0\x02\xff\xfeGRANITE"), Confidence::No);
}

#[test]
fn container_parses_header_and_directory() {
    let c = container::scan_bytes(single_part_prt()).unwrap();
    assert_eq!(c.version, 0x06);
    assert_eq!(c.file_tag, 0x33_22_11);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span.is_some()));
}

#[test]
fn inspect_reports_bounded_nx_object_model_entities() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let summary = NxCodec.inspect(&mut cur).unwrap();
    assert!(summary.notes.iter().any(|note| {
        note == "NX object model: 1 indexed section(s), 2 bounded entity record(s)"
    }));
}

#[test]
fn decode_retains_typed_nx_numeric_expression() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let expressions = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::Expression>("expressions")
        .unwrap();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, 0x102);
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].unit, crate::native::ExpressionUnit::Degree);
    assert_eq!(expressions[0].value, 120.0);
    assert_eq!(expressions[0].source_entry, "/Root/UG_PART/UG_PART");
}

#[test]
fn decode_retains_length_framed_nx_class_definition() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let classes = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ClassDefinition>("class_definitions")
        .unwrap();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "UGS::ModlFeature");
    assert_eq!(classes[0].class_id, 0x81);
    assert_eq!(classes[0].source_entry, "/Root/UG_PART/UG_PART");
}

#[test]
fn decode_retains_nx_arrangement_configurations() {
    let mut cur = Cursor::new(prt_with_arrangements());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let configurations = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::Configuration>("configurations")
        .unwrap();
    assert_eq!(configurations.len(), 2);
    assert_eq!(configurations[0].name, "Model");
    assert!(configurations[0].active);
    assert_eq!(configurations[1].name, "Exploded");
    assert!(!configurations[1].active);
}

#[test]
fn decode_rejects_ambiguous_nx_arrangement_table_atomically() {
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        (
            "/Root/part/arrangements",
            br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="YES" Name="Exploded"/></Arrangements>"#.to_vec(),
        ),
    ]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(result.ir.native.namespace("nx").is_none_or(|namespace| {
        namespace
            .arena_as::<crate::native::Configuration>("configurations")
            .unwrap()
            .is_empty()
    }));
}

#[test]
fn parasolid_extraction_classifies_partition_and_schema() {
    let f = single_part_prt();
    let streams = parasolid::extract_streams(&f);
    let part = streams
        .iter()
        .find(|s| s.kind == StreamKind::Partition)
        .expect("a partition stream");
    assert_eq!(part.schema.as_deref(), Some("SCH_TEST_1_9999"));
    assert!(part.inflated.starts_with(b"PS\x00\x00"));
}

#[test]
fn decode_transfers_point_plane_cylinder_line() {
    let mut cur = Cursor::new(single_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    // Point coordinate is scaled metres → millimetres, byte-exact.
    let p = &result.ir.model.points[0].position;
    assert!((p.x - 62.5).abs() < 1e-6 && (p.z - 12.7).abs() < 1e-6);

    // One plane, one cylinder decoded.
    let planes = result
        .ir
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let cyls: Vec<_> = result
        .ir
        .model
        .surfaces
        .iter()
        .filter_map(|s| match &s.geometry {
            SurfaceGeometry::Cylinder { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect();
    assert_eq!(planes, 1);
    assert_eq!(cyls.len(), 1);
    assert!((cyls[0] - 4.05).abs() < 1e-6);
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane {
            u_axis: axis,
            ..
        } if axis == Vector3::new(1.0, 0.0, 0.0)
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder {
            ref_direction: direction,
            ..
        } if direction == Vector3::new(1.0, 0.0, 0.0)
    )));

    // One line decoded, with a unit direction.
    let lines: Vec<_> = result
        .ir
        .model
        .curves
        .iter()
        .filter(|c| matches!(c.geometry, CurveGeometry::Line { .. }))
        .collect();
    assert_eq!(lines.len(), 1);

    // No topology graph is fabricated; the loss is reported as blocking.
    assert!(result.ir.model.faces.is_empty() && result.ir.model.edges.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.category == cadmpeg_ir::report::LossCategory::Topology
            && l.severity == cadmpeg_ir::report::Severity::Blocking));

    // The Parasolid stream is preserved verbatim.
    let unknowns = result.ir.native_unknowns("nx").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(result.source_fidelity.retained_records[0].sha256.len(), 64);
    assert_eq!(
        unknowns[0].links,
        ["nx:s0:surf#0", "nx:s0:surf#1", "nx:s0:crv#0",]
    );
    assert_eq!(
        result.source_fidelity.annotations.exactness[&unknowns[0].id.to_string()].fields["links"],
        Exactness::Derived
    );

    // The preserved stream owns partial-decode carriers without fabricating topology.
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_emits_connected_primitive_brep() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(
        result.ir.model.faces[0].loops,
        vec![result.ir.model.loops[0].id.clone()]
    );
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.coedges[0].radial_next,
        result.ir.model.coedges[0].id
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.category != cadmpeg_ir::report::LossCategory::Topology));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_emits_offset_surface_construction() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support,
        distance,
        u_sense,
        v_sense,
        extension_flags,
    } = &procedural.definition
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    assert_eq!(*u_sense, Some(0));
    assert_eq!(*v_sense, Some(0));
    assert!(extension_flags.is_empty());
    assert_ne!(procedural.surface, *support);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    assert!(matches!(
        result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .expect("offset carrier")
            .geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_fully_extended_offset_common_header() {
    let stream = offset_surface_with_fully_extended_common_header();
    assert_eq!(crate::topology::offset_surfaces(&stream).len(), 1);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    assert_ne!(procedural.surface, *support);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
}

#[test]
fn decode_tracks_fully_extended_compact_geometry_headers() {
    let mut blend = blend_surface_topology_partition_stream();
    fully_extend_common_header(&mut blend, [0, 56, 0, 12]);
    assert_eq!(crate::topology::blend_surfaces(&blend).len(), 1);

    let mut intersection = intersection_curve_topology_partition_stream();
    fully_extend_common_header(&mut intersection, [0, 38, 0, 12]);
    assert_eq!(crate::topology::composite_curves(&intersection).len(), 1);

    let mut surface_curve = surface_curve_topology_partition_stream();
    fully_extend_common_header(&mut surface_curve, [0, 137, 0, 12]);
    assert_eq!(crate::topology::surface_curves(&surface_curve), [(12, 9)]);

    let mut trimmed = trimmed_topology_partition_stream();
    fully_extend_common_header(&mut trimmed, [0, 133, 0, 12]);
    let trims = crate::topology::trimmed_curves(&trimmed);
    assert_eq!(trims.len(), 1);
    assert_eq!(trims[0].parameters, [0.25, 0.75]);

    let mut bspline = bspline_partition_stream();
    fully_extend_common_header(&mut bspline, [0, 124, 0, 10]);
    fully_extend_common_header(&mut bspline, [0, 134, 0, 50]);
    let mut cur = Cursor::new(prt_with_partition(&bspline));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_))));
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_))));
}

#[test]
fn decode_resolves_surface_curve_to_its_basis_curve() {
    let stream = surface_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_rolling_ball_blend_surface() {
    let stream = blend_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("blend surface");
    let ProceduralSurfaceDefinition::Blend {
        supports,
        radius,
        cross_section,
        spine,
        native,
    } = &procedural.definition
    else {
        panic!("blend definition");
    };
    assert_eq!(*cross_section, BlendCrossSection::Circular);
    assert_eq!(*radius, BlendRadiusLaw::Constant { signed_radius: 3.0 });
    assert_eq!(supports[0].as_ref().map(|side| side.reversed), Some(true));
    assert_eq!(supports[1].as_ref().map(|side| side.reversed), Some(false));
    assert!(spine.is_none());
    assert!(native.is_none());
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_blend_with_extended_support_reference() {
    let stream = blend_surface_with_extended_support_reference();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.procedural_surfaces[0].surface
    );
}

#[test]
fn decode_preserves_intersection_curve_as_connected_carrier() {
    let stream = intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let edge_curve = result.ir.model.edges[0].curve.as_ref().expect("edge curve");
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == edge_curve)
        .expect("intersection carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    assert_eq!(result.ir.model.procedural_curves[0].curve, curve.id);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_deltas_intersection_data_curve() {
    let stream = deltas_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.procedural_curves[0].curve)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_reports_status_framed_deltas_records_and_tombstones() {
    let stream = status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert_eq!(
        attributes.get("deltas.0.full.FACE").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes
            .get("deltas.0.tombstone.EDGE")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes.get("deltas.0.grammar").map(String::as_str),
        Some("status_byte_framed_topology")
    );
}

#[test]
fn decode_accepts_exact_loop_and_rejects_incomplete_fin_deltas() {
    let stream = variable_status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert!(!attributes.contains_key("deltas.0.full.FIN"));
    assert_eq!(
        attributes.get("deltas.0.full.LOOP").map(String::as_str),
        Some("1")
    );
}

#[test]
fn deltas_point_exposes_typed_position_in_model_units() {
    let points = crate::deltas::points(&status_framed_deltas_point_stream());
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].xmt, 50);
    assert_eq!(points[0].node_id, 900);
    assert_eq!(points[0].position.x, 12.5);
    assert_eq!(points[0].position.y, -2.0);
    assert_eq!(points[0].position.z, 4.0);
}

#[test]
fn deltas_point_normalizes_to_partition_record_framing() {
    let record = crate::deltas::walk(&status_framed_deltas_point_stream())
        .records
        .remove(0);
    let mut expected = crate::tests::record(29, 40);
    put_ref(&mut expected, 2, 50);
    expected[4..8].copy_from_slice(&900u32.to_be_bytes());
    for at in [8, 10, 12, 14] {
        put_ref(&mut expected, at, 1);
    }
    put_vec3(&mut expected, 16, [0.0125, -0.002, 0.004]);
    assert_eq!(record.canonical_bytes, expected);
}

#[test]
fn merged_deltas_full_record_replaces_partition_node() {
    let partition = topology_partition_stream();
    let mut deltas = status_framed_deltas_point_stream();
    deltas[2..4].copy_from_slice(&11u16.to_be_bytes());
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    let points = crate::geometry::points(&merged);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].position.x, 12.5);
    assert_eq!(points[0].position.y, -2.0);
    assert_eq!(points[0].position.z, 4.0);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
}

#[test]
fn merged_exact_key_tombstone_removes_partition_node() {
    let partition = topology_partition_stream();
    let mut tombstone = Vec::new();
    tombstone.extend_from_slice(&29u16.to_be_bytes());
    tombstone.extend_from_slice(&11u16.to_be_bytes());
    tombstone.extend_from_slice(&[0, 1]);
    let census = crate::deltas::walk(&tombstone);
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].kind, 29);
    assert_eq!(census.tombstones[0].xmt, 11);
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_none());
    assert!(crate::geometry::points(&merged).is_empty());
}

#[test]
fn merged_deltas_uses_last_full_or_tombstone_event() {
    let partition = topology_partition_stream();
    let tombstone = [0, 29, 0, 11, 0, 1];
    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&11u16.to_be_bytes());

    let mut delete_then_replace = tombstone.to_vec();
    delete_then_replace.extend_from_slice(&full);
    let merged = crate::deltas::merge_full_records(&partition, &delete_then_replace);
    assert_eq!(crate::geometry::points(&merged).len(), 1);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 12.5);

    let mut replace_then_delete = full;
    replace_then_delete.extend_from_slice(&tombstone);
    let merged = crate::deltas::merge_full_records(&partition, &replace_then_delete);
    assert!(crate::geometry::points(&merged).is_empty());
}

#[test]
fn decode_emits_point_added_by_deltas_stream() {
    let mut cur = Cursor::new(prt_with_partition(&deltas_point_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_replaces_partition_point_with_same_xmt_deltas_point() {
    let partition = topology_partition_stream();
    let mut deltas = deltas_point_partition_stream();
    let record = deltas
        .windows(2)
        .rposition(|window| window == 29u16.to_be_bytes())
        .expect("deltas POINT");
    deltas[record + 2..record + 4].copy_from_slice(&11u16.to_be_bytes());
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_replaces_partition_edge_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_edge_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.9));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_face_and_vertex_from_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_face_vertex_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.8));
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.7));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_loop_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_loop_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(15, 5)
            .and_then(|node| node.u32_at(4)),
        Some(904)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_shell_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_shell_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(13, 3)
            .and_then(|node| node.u32_at(4)),
        Some(905)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_fin_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_fin_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_line_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_line_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let CurveGeometry::Line { origin, direction } = result.ir.model.curves[0].geometry else {
        panic!("line");
    };
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0));
    assert_eq!(direction, Vector3::new(0.0, 1.0, 0.0));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_plane_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_plane_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { origin, normal, u_axis }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && normal == Vector3::new(0.0, 1.0, 0.0)
                && u_axis == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_circle_from_status_framed_deltas() {
    let partition = circle_topology_partition_stream();
    let deltas = deltas_circle_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_ellipse_from_status_framed_deltas() {
    let partition = ellipse_topology_partition_stream();
    let deltas = deltas_ellipse_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && major_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 30.0
            && minor_radius == 12.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cylinder_from_status_framed_deltas() {
    let partition = cylinder_topology_partition_stream();
    let deltas = deltas_cylinder_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { origin, axis, ref_direction, radius }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cone_from_status_framed_deltas() {
    let partition = cone_topology_partition_stream();
    let deltas = deltas_cone_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { origin, axis, ref_direction, radius, ratio, half_angle }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
                && ratio == 1.0
                && (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_sphere_from_status_framed_deltas() {
    let partition = sphere_topology_partition_stream();
    let deltas = deltas_sphere_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_torus_from_status_framed_deltas() {
    let partition = torus_topology_partition_stream();
    let deltas = deltas_torus_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 40.0
            && minor_radius == 15.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_charted_surface_intersection_construction() {
    let stream = charted_intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_curves
        .first()
        .expect("intersection construction");
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .expect("solved chart cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("charted NURBS cache");
    };
    assert_eq!(nurbs.degree, 1);
    assert_eq!(nurbs.control_points[0].x, 0.0);
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(procedural.cache_fit_tolerance, Some(0.01));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &procedural.definition
    else {
        panic!("typed surface intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_none());
    assert_eq!(context.parameter_range, [0.0, 0.01]);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_ext11_deltas_intersection_chart() {
    let stream = ext11_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let curve_id = &result.ir.model.procedural_curves[0].curve;
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("intersection cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("NURBS chart cache");
    };
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.01, 0.01]);
}

#[test]
fn decode_emits_both_intersection_support_pcurves() {
    let stream = two_support_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_intersection_second_support_through_blend_bound() {
    let stream = blend_bound_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let second = context.sides[1].surface.as_ref().expect("bridged support");
    assert_ne!(context.sides[0].surface.as_ref(), Some(second));
    assert!(context.sides[1].pcurve.is_some());
}

#[test]
fn decode_emits_inline_descriptor_intersection_witnesses() {
    let stream = inline_descriptor_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
    ));
    assert!(matches!(
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
            .expect("intersection curve")
            .geometry,
        CurveGeometry::Nurbs(_)
    ));
}

#[test]
fn decode_emits_topology_when_record_xmt_uses_extended_encoding() {
    let stream = large_xmt_headers(&topology_partition_stream());
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_maps_parasolid_tolerance_sentinel_to_none() {
    let stream = topology_with_missing_tolerances();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.vertices[0].tolerance, None);
    assert_eq!(result.ir.model.edges[0].tolerance, None);
    assert_eq!(result.ir.model.faces[0].tolerance, None);
}

#[test]
fn decode_dual_writes_inline_entity_metadata_to_annotations() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let ir = &result.ir;
    let annotations = &result.source_fidelity.annotations;

    macro_rules! assert_arena_annotations {
        ($arena:expr) => {
            for entity in $arena {
                let provenance = annotations
                    .provenance
                    .get(&entity.id.to_string())
                    .expect("annotation provenance");
                assert!(annotations.streams[provenance.stream as usize].starts_with("nx:"));
                assert!(provenance.tag.is_some());
            }
        };
    }

    assert_arena_annotations!(&ir.model.bodies);
    assert_arena_annotations!(&ir.model.regions);
    assert_arena_annotations!(&ir.model.shells);
    assert_arena_annotations!(&ir.model.faces);
    assert_arena_annotations!(&ir.model.loops);
    assert_arena_annotations!(&ir.model.coedges);
    assert_arena_annotations!(&ir.model.edges);
    assert_arena_annotations!(&ir.model.vertices);
    assert_arena_annotations!(&ir.model.points);
    assert_arena_annotations!(&ir.model.surfaces);
    assert_arena_annotations!(&ir.model.curves);
    let unknowns = ir.native_unknowns("nx").unwrap();
    assert_arena_annotations!(&unknowns);

    let point_note = &annotations.exactness[&ir.model.points[0].id.to_string()];
    assert_eq!(point_note.entity, Exactness::ByteExact);
    assert_eq!(point_note.fields["position"], Exactness::Derived);
    let surface_note = &annotations.exactness[&ir.model.surfaces[0].id.to_string()];
    assert_eq!(surface_note.fields["geometry"], Exactness::Derived);
    let curve_note = &annotations.exactness[&ir.model.curves[0].id.to_string()];
    assert_eq!(curve_note.fields["geometry"], Exactness::Derived);
    for id in [
        ir.model.vertices[0].id.to_string(),
        ir.model.edges[0].id.to_string(),
        ir.model.faces[0].id.to_string(),
    ] {
        assert_eq!(
            annotations.exactness[&id].fields["tolerance"],
            Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_bspline_surface_and_curve() {
    let stream = bspline_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(surface) => Some(surface),
            _ => None,
        })
        .expect("B-spline surface");
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert!((surface.control_points[1].y - 20.0).abs() < 1e-9);
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(curve) => Some(curve),
            _ => None,
        })
        .expect("B-spline curve");
    assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(curve.control_points.len(), 2);
    assert!((curve.control_points[1].x - 20.0).abs() < 1e-9);
}

#[test]
fn decode_replaces_partition_bspline_surface_wrapper_from_deltas() {
    let partition = bspline_surface_replacement_partition_stream();
    let deltas = deltas_bspline_surface_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 30.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_bspline_curve_wrapper_from_deltas() {
    let partition = bspline_curve_replacement_partition_stream();
    let deltas = deltas_bspline_curve_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 10.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_trimmed_edge_to_its_basis_curve_and_range() {
    let mut cur = Cursor::new(prt_with_partition(&trimmed_topology_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir.model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_extended_xmt_reference_inside_edge_record() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_curve_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_extended_face_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_face_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_extended_edge_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_all_extended_topology_reference_shifts() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_internal_topology_references(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.points[0].position.x, 10.0);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_fully_extended_geometry_header_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_fully_extended_geometry_headers(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
}

#[test]
fn cylinder_gate_rejects_denormal_radius() {
    // A coincidental byte alignment can present a unit axis and a model-scale
    // origin alongside a denormal (near-zero) double at the radius slot; the radius
    // floor must reject it rather than emit a fabricated zero-radius cylinder.
    let mut cy = record(0x33, 99);
    put_vec3(&mut cy, 19, [0.003_175, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, f64::from_bits(1)); // smallest positive subnormal
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    assert!(crate::geometry::surfaces(&cy).is_empty());
}

#[test]
fn decode_assembly_reports_external_dependency() {
    let mut cur = Cursor::new(assembly_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.message.contains("assembly")));
}

#[test]
fn assembly_metadata_lists_external_child_paths() {
    let mut cur = Cursor::new(assembly_with_external_paths());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attrs = &result.ir.source.expect("source").attributes;
    assert_eq!(
        attrs.get("external_reference.0").map(String::as_str),
        Some("child.prt")
    );
    assert_eq!(
        attrs.get("external_reference.1").map(String::as_str),
        Some("nested/b.prt")
    );
}

#[test]
fn container_reads_rmfastload_active_ids() {
    let container = container::scan_bytes(rmfastload_prt()).unwrap();
    assert_eq!(
        container.rmfastload_object_ids(),
        (1..=50).collect::<Vec<_>>()
    );
}

#[test]
fn decode_selects_dominant_rmfastload_body() {
    let mut cur = Cursor::new(prt_with_two_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(result.ir.model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir.model.faces.len(), 50);
    assert!(result
        .ir
        .model
        .faces
        .iter()
        .all(|face| face.id.0.starts_with("nx:s0:")));
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("active_body_selector"))
            .map(String::as_str),
        Some("rmfastload_object_id_membership")
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_keeps_bodies_when_rmfastload_overlap_is_weak() {
    let mut cur = Cursor::new(prt_with_weak_rmfastload_overlap());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert!(result
        .ir
        .source
        .as_ref()
        .is_none_or(|source| !source.attributes.contains_key("active_body_selector")));
}

#[test]
fn container_only_preserves_streams_without_geometry() {
    let mut cur = Cursor::new(single_part_prt());
    let opts = DecodeOptions {
        container_only: true,
    };
    let result = NxCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result.report.container_only);
    assert_eq!(result.ir.native_unknowns("nx").unwrap().len(), 1);
    assert!(result.ir.model.points.is_empty());
}

#[test]
fn inspect_enumerates_streams_and_names_schema() {
    let mut cur = Cursor::new(single_part_prt());
    let summary = NxCodec.inspect(&mut cur).unwrap();
    assert_eq!(summary.format, "nx");
    assert_eq!(summary.container_kind, "splmsstr");
    assert!(summary.entries.iter().any(|e| e.role == "parasolid-stream"));
    assert!(summary.notes.iter().any(|n| n.contains("partition")));
}

#[test]
fn extraction_uses_ug_part_bounds_and_all_standard_zlib_headers() {
    let part = zlib_compress_at_level(&partition_stream(), 6);
    assert_eq!(&part[..2], b"\x78\x9c");

    let mut decoy_stream = partition_stream();
    let schema = b"SCH_TEST_1_9999";
    let decoy = b"SCH_FAKE_1_9999";
    let pos = decoy_stream
        .windows(schema.len())
        .position(|w| w == schema)
        .unwrap();
    decoy_stream[pos..pos + schema.len()].copy_from_slice(decoy);
    let decoy = zlib_compress(&decoy_stream);

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let entries = [
        (b"/Root/UG_PART/UG_PART".as_slice(), part.len()),
        (b"/Root/FastLoad/JT".as_slice(), decoy.len()),
    ];
    let directory_len: usize = entries.iter().map(|(name, _)| 4 + name.len() + 16).sum();
    let mut next_offset = file.len() + directory_len;
    for (name, size) in &entries {
        file.extend_from_slice(&(name.len() as u32).to_le_bytes());
        file.extend_from_slice(name);
        file.extend_from_slice(&(next_offset as u64).to_le_bytes());
        file.extend_from_slice(&(*size as u64).to_le_bytes());
        next_offset += size;
    }
    file.extend_from_slice(&part);
    file.extend_from_slice(&decoy);

    let streams = parasolid::extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].schema.as_deref(), Some("SCH_TEST_1_9999"));
}
