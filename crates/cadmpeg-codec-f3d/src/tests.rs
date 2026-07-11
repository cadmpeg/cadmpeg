// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Tests over synthetic byte fixtures. No real CAD files exist in this repo and
//! none may be added, so every fixture is hand-built here to exercise a real
//! decode path that can fail if the code regresses.

use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions, Encoder};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::asm_header;
use crate::container::{self, role};
use crate::F3dCodec;

/// Build a synthetic ASM `BinaryFile8` BREP stream: a spec-shaped header
/// followed by a couple of filler records and a `delta_state` history marker.
fn synthetic_smbh() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile8<"); // 0..16 magic
    b.extend_from_slice(&[0u8; 8]); // 16..24 zero
    b.extend_from_slice(&7u64.to_be_bytes()); // 24..32 version word
    b.extend_from_slice(&3u64.to_be_bytes()); // 32..40 format version
                                              // Schema word `7` occupies 40..48, but its low byte at offset 47 (0x07) is
                                              // reused as the first product string's tag: write only the seven high zero
                                              // bytes here, then let `push_u8_string`'s 0x07 tag land at offset 47.
    b.extend_from_slice(&[0u8; 7]); // 40..47 schema word high bytes
    push_u8_string(&mut b, "Autodesk Neutron"); // 0x07 tag at offset 47
    push_u8_string(&mut b, "ASM 231.6.3.65535 OSX");
    push_u8_string(&mut b, "Tue Mar 31 16:16:19 2026");
    push_tagged_f64(&mut b, 60.0); // scale
    push_tagged_f64(&mut b, 1e-6); // resabs
    push_tagged_f64(&mut b, 1e-10); // resnor

    // Some active-model filler (no delta_state here).
    b.extend_from_slice(&[0x0d, 0x04, b'b', b'o', b'd', b'y', 0x11]);
    let active_len = b.len();

    // History boundary: 0x11 0x0d 0x0b "delta_state" ... ([spec §4a](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#2-b-rep-streams-and-history-partition)).
    b.extend_from_slice(&[0x11, 0x0d, 0x0b]);
    b.extend_from_slice(b"delta_state");
    b.extend_from_slice(&[0u8; 16]);

    // Sanity: the delta_state string starts at active_len + 3.
    assert_eq!(&b[active_len + 3..active_len + 3 + 11], b"delta_state");
    b
}

fn push_u8_string(b: &mut Vec<u8>, s: &str) {
    b.push(0x07);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}

// ---- SAB record-stream fixtures ---------------------------------------------
//
// The helpers below assemble a minimal but genuine active model slice: an
// `asmheader` at RecordTable index 0 followed by a single planar face bounded by
// a closed three-coedge loop, with its edges, vertices, and points. Entity
// references are RecordTable indices; `-1` is null. This exercises the framer,
// topology graph builder, and analytic surface decode end to end.

/// The three `0x07`-tagged strings + three `0x06`-tagged doubles of a
/// `BinaryFile8` header, i.e. the bytes up to the start of the record stream.
fn smbh_header_prefix() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile8<");
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&7u64.to_be_bytes());
    b.extend_from_slice(&3u64.to_be_bytes());
    // Schema word `7`'s low byte at offset 47 doubles as the first product
    // string's 0x07 tag; write the seven high zero bytes and let the string tag
    // supply byte 47 (mirrors the real .smbh wrapper layout).
    b.extend_from_slice(&[0u8; 7]);
    push_u8_string(&mut b, "Autodesk Neutron");
    push_u8_string(&mut b, "ASM 231.6.3.65535 OSX");
    push_u8_string(&mut b, "Tue Mar 31 16:16:19 2026");
    push_tagged_f64(&mut b, 60.0);
    push_tagged_f64(&mut b, 1e-6);
    push_tagged_f64(&mut b, 1e-10);
    b
}

fn t_ref(b: &mut Vec<u8>, v: i64) {
    b.push(0x0c);
    b.extend_from_slice(&v.to_le_bytes());
}
fn t_long(b: &mut Vec<u8>, v: i64) {
    b.push(0x04);
    b.extend_from_slice(&v.to_le_bytes());
}
fn t_dbl(b: &mut Vec<u8>, v: f64) {
    b.push(0x06);
    b.extend_from_slice(&v.to_le_bytes());
}
fn t_pos(b: &mut Vec<u8>, p: [f64; 3]) {
    b.push(0x13);
    for c in p {
        b.extend_from_slice(&c.to_le_bytes());
    }
}
fn t_vec(b: &mut Vec<u8>, p: [f64; 3]) {
    b.push(0x14);
    for c in p {
        b.extend_from_slice(&c.to_le_bytes());
    }
}
fn t_ident(b: &mut Vec<u8>, s: &str) {
    b.push(0x0d);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}
fn t_subident(b: &mut Vec<u8>, s: &str) {
    b.push(0x0e);
    b.push(s.len() as u8);
    b.extend_from_slice(s.as_bytes());
}
fn t_end(b: &mut Vec<u8>) {
    b.push(0x11);
}

fn assert_f3d_native_parity(ir: &cadmpeg_ir::document::CadIr) {
    let native = ir.native.f3d.as_ref().expect("F3D native namespace");
    assert_eq!(native.version, cadmpeg_ir::native::F3D_NATIVE_VERSION);
}

fn f3d_native(ir: &cadmpeg_ir::document::CadIr) -> &cadmpeg_ir::native::F3dNative {
    ir.native.f3d.as_ref().expect("F3D native namespace")
}

/// Assemble the active slice: header prefix + records + `delta_state` boundary.
/// `RecordTable` indices are the order below, starting at 0 (`asmheader`).
fn synthetic_geometry_smbh() -> Vec<u8> {
    // Indices: 0 asmheader, 1 body, 2 region, 3 shell, 4 face, 5 loop,
    // 6 plane, 7/8/9 coedges, 10/11/12 edges, 13/14/15 vertices,
    // 16/17/18 points.
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);

    // 1: body  (chunk3 = first_region)
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, 42); // 1 native ASM body key
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_region
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: region  (chunk4 = first_shell, chunk5 = owner_body)
    t_ident(&mut r, "region");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, 3); // 4 first_shell
    t_ref(&mut r, 1); // 5 owner_body
    t_end(&mut r);

    // 3: shell  (chunk5 = first_face, chunk7 = owner_region)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, -1); // 4 null
    t_ref(&mut r, 4); // 5 first_face
    t_ref(&mut r, -1); // 6 wire
    t_ref(&mut r, 2); // 7 owner_region
    t_end(&mut r);

    // 4: face  (chunk4 first_loop, chunk5 owner_shell, chunk7 surface, chunk8 sense)
    t_ident(&mut r, "face");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_face
    t_ref(&mut r, 5); // 4 first_loop
    t_ref(&mut r, 3); // 5 owner_shell
    t_ref(&mut r, -1); // 6 null
    t_ref(&mut r, 6); // 7 surface
    r.push(0x0b); // 8 sense = forward
    r.push(0x0b); // 9 sides = single
    t_end(&mut r);

    // 5: loop  (chunk4 first_coedge, chunk5 owner_face)
    t_ident(&mut r, "loop");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_loop
    t_ref(&mut r, 7); // 4 first_coedge
    t_ref(&mut r, 4); // 5 owner_face
    t_end(&mut r);

    // 6: plane-surface  (origin, normal, uv-origin)
    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1); // attrib
    t_long(&mut r, -1); // history
    t_ref(&mut r, -1); // null
    t_pos(&mut r, [0.0, 0.0, 0.0]); // root
    t_vec(&mut r, [0.0, 0.0, 1.0]); // normal
    t_vec(&mut r, [1.0, 0.0, 0.0]); // UV reference direction
    r.push(0x0b); // sense
    t_end(&mut r);

    // 7/8/9: coedges forming the ring 7 -> 8 -> 9 -> 7
    let coedges = [(7i64, 8, 9, 10), (8, 9, 7, 11), (9, 7, 8, 12)];
    for (_id, next, prev, edge) in coedges {
        t_ident(&mut r, "coedge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, next); // 3 next
        t_ref(&mut r, prev); // 4 prev
        t_ref(&mut r, -1); // 5 partner (open loop, none)
        t_ref(&mut r, edge); // 6 edge
        r.push(0x0b); // 7 sense = forward
        t_ref(&mut r, 5); // 8 owner_loop
        t_long(&mut r, 0); // 9 reserved
        t_ref(&mut r, -1); // 10 pcurve
        t_end(&mut r);
    }

    // 10/11/12: edges  (start, end vertices), curve = null
    let edges = [(10i64, 13, 14), (11, 14, 15), (12, 15, 13)];
    for (_id, start, end) in edges {
        t_ident(&mut r, "edge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, start); // 3 start_vertex
        t_dbl(&mut r, 0.0); // 4 t_start
        t_ref(&mut r, end); // 5 end_vertex
        t_dbl(&mut r, 1.0); // 6 t_end
        t_ref(&mut r, -1); // 7 owner_coedge
        t_ref(&mut r, -1); // 8 curve (degenerate: none)
        r.push(0x0b); // 9 sense
        push_u8_string(&mut r, "unknown"); // 10 continuity text
        t_end(&mut r);
    }

    // 13/14/15: vertices (owning_edge, index_flag, point)
    let verts = [(13i64, 10, 16), (14, 11, 17), (15, 12, 18)];
    for (_id, edge, point) in verts {
        t_ident(&mut r, "vertex");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, edge); // 3 owning_edge
        t_long(&mut r, 0); // 4 index_flag
        t_ref(&mut r, point); // 5 point
        t_end(&mut r);
    }

    // 16/17/18: points  (coordinates in cm; ×10 = mm)
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for p in points {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1); // attrib
        t_long(&mut r, -1); // history
        t_ref(&mut r, -1); // null
        t_pos(&mut r, p);
        t_long(&mut r, 1); // reference count
        t_end(&mut r);
    }

    // History boundary: previous record's 0x11 + 0x0d 0x0b 'delta_state'.
    t_ident(&mut r, "delta_state"); // 0x0d 0x0b 'delta_state'

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

fn synthetic_geometry_with_history_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let name_tag = bytes
        .windows(b"\x0d\x0bdelta_state".len())
        .position(|window| window == b"\x0d\x0bdelta_state")
        .unwrap();
    let mut preamble = Vec::new();
    for name in ["Begin", "of", "ASM", "History"] {
        t_subident(&mut preamble, name);
    }
    t_ident(&mut preamble, "Data");
    t_ident(&mut preamble, "history_stream");
    for value in [2, 2, 0, 99] {
        t_long(&mut preamble, value);
    }
    for reference in [-1, 0, 1, -1] {
        t_ref(&mut preamble, reference);
    }
    t_end(&mut preamble);
    bytes.splice(name_tag..name_tag, preamble);

    let first_name_end = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        + b"delta_state".len();
    let mut tail = Vec::new();
    for value in [2, 1, 0] {
        t_long(&mut tail, value);
    }
    for reference in [-1, 2, 0, -1, 0] {
        t_ref(&mut tail, reference);
    }
    tail.push(0x0b);
    t_long(&mut tail, 1); // board present
    t_ref(&mut tail, 0); // board owner
    t_long(&mut tail, 2); // board number
    t_long(&mut tail, 1); // change present
    t_ref(&mut tail, 1830); // old
    t_ref(&mut tail, 1); // new: update
    t_long(&mut tail, 1); // change present
    t_ref(&mut tail, -1); // old null
    t_ref(&mut tail, 8); // new: insert
    t_long(&mut tail, 0); // end changes
    t_long(&mut tail, 0); // end boards
    t_end(&mut tail);
    t_ident(&mut tail, "history_payload");
    t_long(&mut tail, 37);
    t_end(&mut tail);
    t_ident(&mut tail, "delta_state");
    for value in [3, 1, 0] {
        t_long(&mut tail, value);
    }
    for reference in [0, -1, 1, -1, 0] {
        t_ref(&mut tail, reference);
    }
    tail.push(0x0b);
    t_end(&mut tail);
    bytes.splice(first_name_end.., tail);
    bytes
}

fn synthetic_geometry_with_transform_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = crate::asm_header::first_delta_state_offset(&bytes).expect("history boundary");
    let start = crate::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = crate::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let body = &records[1];
    let transform_ref =
        crate::sab::payload_token_offsets(&bytes, body, 8, 0x0c).expect("body reference tokens")[4];
    bytes[transform_ref + 1..transform_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut transform = Vec::new();
    t_ident(&mut transform, "transform");
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 2.0, 3.0],
    ] {
        t_vec(&mut transform, vector);
    }
    t_dbl(&mut transform, 1.0);
    t_end(&mut transform);
    bytes.splice(limit..limit, transform);
    bytes
}

fn synthetic_geometry_with_body_color_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = crate::asm_header::first_delta_state_offset(&bytes).expect("history boundary");
    let start = crate::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = crate::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let body = &records[1];
    let attribute_ref =
        crate::sab::payload_token_offsets(&bytes, body, 8, 0x0c).expect("body reference tokens")[0];
    bytes[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut attribute = Vec::new();
    t_subident(&mut attribute, "rgb_color");
    t_subident(&mut attribute, "st");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    t_dbl(&mut attribute, 0.1);
    t_dbl(&mut attribute, 0.2);
    t_dbl(&mut attribute, 0.3);
    t_end(&mut attribute);
    bytes.splice(limit..limit, attribute);
    bytes
}

fn synthetic_geometry_with_face_color_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = crate::asm_header::first_delta_state_offset(&bytes).expect("history boundary");
    let start = crate::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = crate::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let face = &records[4];
    let attribute_ref =
        crate::sab::payload_token_offsets(&bytes, face, 8, 0x0c).expect("face reference tokens")[0];
    bytes[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let mut attribute = Vec::new();
    t_subident(&mut attribute, "rgb_color");
    t_subident(&mut attribute, "st");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    t_dbl(&mut attribute, 0.15);
    t_dbl(&mut attribute, 0.25);
    t_dbl(&mut attribute, 0.35);
    t_end(&mut attribute);
    bytes.splice(limit..limit, attribute);
    bytes
}

/// Add a generated inline 2D `nubs` pcurve to the first coedge of the base
/// topology fixture. The new record is appended at `RecordTable` index 19.
fn synthetic_geometry_with_pcurve_smbh() -> Vec<u8> {
    synthetic_geometry_with_pcurve_block_smbh(generated_pcurve_block())
}

fn synthetic_geometry_with_rational_pcurve_smbh() -> Vec<u8> {
    synthetic_geometry_with_pcurve_block_smbh(generated_rational_pcurve_block())
}

fn synthetic_geometry_with_pcurve_block_smbh(block: Vec<u8>) -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref_tag = record.iter().rposition(|b| *b == 0x0c).unwrap();
    record[pcurve_ref_tag + 1..pcurve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes[..]
        .windows(b"delta_state".len())
        .position(|w| w == b"delta_state")
        .unwrap()
        - 2;
    let mut pcurve = Vec::new();
    t_ident(&mut pcurve, "pcurve");
    t_ref(&mut pcurve, -1);
    t_long(&mut pcurve, -1);
    t_ref(&mut pcurve, -1);
    t_long(&mut pcurve, 0);
    pcurve.push(0x0b);
    pcurve.push(0x0f);
    t_ident(&mut pcurve, "exp_par_cur");
    pcurve.extend_from_slice(&block);
    t_dbl(&mut pcurve, 0.001);
    pcurve.push(0x10);
    pcurve.extend_from_slice(&[0x0b; 4]);
    t_dbl(&mut pcurve, -1.0);
    t_dbl(&mut pcurve, 2.0);
    t_end(&mut pcurve);
    bytes.splice(delta..delta, pcurve);
    bytes
}

fn synthetic_geometry_with_ref_pcurve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref_tag = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[pcurve_ref_tag + 1..pcurve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut records = Vec::new();
    t_ident(&mut records, "pcurve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_long(&mut records, 2);
    t_ref(&mut records, 20);
    t_dbl(&mut records, -2.0);
    t_dbl(&mut records, 4.0);
    t_end(&mut records);
    t_subident(&mut records, "intcurve");
    t_ident(&mut records, "curve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    records.extend_from_slice(&generated_curve_block());
    records.extend_from_slice(&generated_pcurve_block());
    t_end(&mut records);
    bytes.splice(delta..delta, records);
    bytes
}

fn synthetic_geometry_with_procedural_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let record = &mut bytes[edge.offset..edge.offset + edge.len];
    let curve_ref_tag = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[curve_ref_tag + 1..curve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "surf_surf_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_helix_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = crate::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "helix_int_cur");
    curve.push(0x0a);
    t_dbl(&mut curve, 0.0);
    curve.push(0x0a);
    t_dbl(&mut curve, std::f64::consts::TAU);
    t_pos(&mut curve, [1.0, 2.0, 3.0]);
    t_pos(&mut curve, [2.0, 0.0, 0.0]);
    t_pos(&mut curve, [0.0, 2.0, 0.0]);
    t_pos(&mut curve, [0.0, 0.0, 4.0]);
    t_dbl(&mut curve, 0.25);
    t_vec(&mut curve, [0.0, 0.0, 1.0]);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_vector_offset_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = crate::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "offset_int_cur");
    curve.push(0x0b);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -2.0);
    t_dbl(&mut curve, 5.0);
    t_vec(&mut curve, [0.5, -1.0, 2.0]);
    push_u8_string(&mut curve, "source");
    t_long(&mut curve, 7);
    push_u8_string(&mut curve, "offset");
    t_long(&mut curve, 9);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0008);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_subset_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = crate::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());
    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "subset_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -1.5);
    t_dbl(&mut curve, 3.5);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0006);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_attribute_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let body = &records[1];
    let record = &mut bytes[body.offset..body.offset + body.len];
    let attribute_ref = record.iter().position(|byte| *byte == 0x0c).unwrap();
    record[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut attribute = Vec::new();
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    push_u8_string(&mut attribute, "generic_tag_attrib_def");
    for value in [3, 3, -1] {
        t_long(&mut attribute, value);
    }
    push_u8_string(&mut attribute, "generic_tag_attrib_def ");
    t_long(&mut attribute, 1);
    t_long(&mut attribute, 3);
    push_u8_string(&mut attribute, "322");
    for value in [7, 0, 0] {
        t_long(&mut attribute, value);
    }
    t_end(&mut attribute);
    bytes.splice(delta..delta, attribute);
    bytes
}

fn synthetic_geometry_with_sketch_link_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let coedge = &records[7];
    let record = &mut bytes[coedge.offset..coedge.offset + coedge.len];
    let attribute_ref = record.iter().position(|byte| *byte == 0x0c).unwrap();
    record[attribute_ref + 1..attribute_ref + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut attribute = Vec::new();
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    push_u8_string(&mut attribute, "sketch_attrib_def");
    for value in [1, 1, 3] {
        t_long(&mut attribute, value);
    }
    push_u8_string(&mut attribute, "113 0 1 0 2 3");
    t_end(&mut attribute);
    bytes.splice(delta..delta, attribute);
    bytes
}

fn synthetic_wire_body_smbh() -> Vec<u8> {
    let mut records = Vec::new();
    t_ident(&mut records, "asmheader");
    push_u8_string(&mut records, "231.6.3.65535");
    t_end(&mut records);

    t_ident(&mut records, "body");
    t_ref(&mut records, -1);
    t_long(&mut records, 1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 2);
    t_ref(&mut records, -1);
    t_ref(&mut records, -1);
    t_end(&mut records);

    t_ident(&mut records, "region");
    for reference in [-1, -1, -1, -1, 3, 1] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "shell");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, -1, -1, 4, 2] {
        t_ref(&mut records, reference);
    }
    t_end(&mut records);

    t_ident(&mut records, "wire");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, -1, 5, 3, -1] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_end(&mut records);

    t_ident(&mut records, "coedge");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    for reference in [-1, 5, 5, -1, 6] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_ref(&mut records, 4);
    t_long(&mut records, 0);
    t_ref(&mut records, -1);
    t_end(&mut records);

    t_ident(&mut records, "edge");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 7);
    t_dbl(&mut records, 0.0);
    t_ref(&mut records, 8);
    t_dbl(&mut records, 2.0);
    t_ref(&mut records, 5);
    t_ref(&mut records, 11);
    records.push(0x0b);
    push_u8_string(&mut records, "unknown");
    t_end(&mut records);

    for (point, index_flag) in [(9, 0), (10, 1)] {
        t_ident(&mut records, "vertex");
        t_ref(&mut records, -1);
        t_long(&mut records, -1);
        t_ref(&mut records, -1);
        t_ref(&mut records, 6);
        t_long(&mut records, index_flag);
        t_ref(&mut records, point);
        t_end(&mut records);
    }
    for position in [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]] {
        t_ident(&mut records, "point");
        t_ref(&mut records, -1);
        t_long(&mut records, -1);
        t_ref(&mut records, -1);
        t_pos(&mut records, position);
        t_long(&mut records, 1);
        t_end(&mut records);
    }
    t_subident(&mut records, "straight");
    t_ident(&mut records, "curve");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_pos(&mut records, [0.0, 0.0, 0.0]);
    t_vec(&mut records, [1.0, 0.0, 0.0]);
    t_end(&mut records);
    t_ident(&mut records, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&records);
    out
}

fn synthetic_mixed_face_wire_body_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    for (record_index, reference_ordinal) in [(1usize, 3usize), (3, 5)] {
        let record = &records[record_index];
        let offsets = crate::sab::payload_token_offsets(&bytes, record, 8, 0x0c)
            .expect("generated reference offsets");
        let offset = offsets[reference_ordinal];
        bytes[offset + 1..offset + 9].copy_from_slice(&19i64.to_le_bytes());
    }
    let updated = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    assert_eq!(updated[1].ref_at(4), Some(19));
    assert_eq!(updated[3].ref_at(6), Some(19));

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut appended = Vec::new();
    t_ident(&mut appended, "wire");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    for reference in [-1, -1, 20, 3, -1] {
        t_ref(&mut appended, reference);
    }
    appended.push(0x0b);
    t_end(&mut appended);

    t_ident(&mut appended, "coedge");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    for reference in [-1, 20, 20, -1, 21] {
        t_ref(&mut appended, reference);
    }
    appended.push(0x0b);
    t_ref(&mut appended, 19);
    t_long(&mut appended, 0);
    t_ref(&mut appended, -1);
    t_end(&mut appended);

    t_ident(&mut appended, "edge");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    t_ref(&mut appended, -1);
    t_ref(&mut appended, 22);
    t_dbl(&mut appended, 0.0);
    t_ref(&mut appended, 23);
    t_dbl(&mut appended, 2.0);
    t_ref(&mut appended, 20);
    t_ref(&mut appended, 26);
    appended.push(0x0b);
    push_u8_string(&mut appended, "unknown");
    t_end(&mut appended);

    for (point, index_flag) in [(24, 0), (25, 1)] {
        t_ident(&mut appended, "vertex");
        t_ref(&mut appended, -1);
        t_long(&mut appended, -1);
        t_ref(&mut appended, -1);
        t_ref(&mut appended, 21);
        t_long(&mut appended, index_flag);
        t_ref(&mut appended, point);
        t_end(&mut appended);
    }
    for position in [[0.0, 0.0, 1.0], [2.0, 0.0, 1.0]] {
        t_ident(&mut appended, "point");
        t_ref(&mut appended, -1);
        t_long(&mut appended, -1);
        t_ref(&mut appended, -1);
        t_pos(&mut appended, position);
        t_long(&mut appended, 1);
        t_end(&mut appended);
    }
    t_subident(&mut appended, "straight");
    t_ident(&mut appended, "curve");
    t_ref(&mut appended, -1);
    t_long(&mut appended, -1);
    t_ref(&mut appended, -1);
    t_pos(&mut appended, [0.0, 0.0, 1.0]);
    t_vec(&mut appended, [1.0, 0.0, 0.0]);
    t_end(&mut appended);
    bytes.splice(delta..delta, appended);
    bytes
}

fn synthetic_geometry_with_degenerate_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = crate::sab::payload_token_offsets(&bytes, edge, 8, 0x0c)
        .expect("generated edge reference offsets");
    bytes[offsets[3] + 1..offsets[3] + 9].copy_from_slice(&13i64.to_le_bytes());
    bytes[offsets[5] + 1..offsets[5] + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut curve = Vec::new();
    t_subident(&mut curve, "degenerate_curve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    t_pos(&mut curve, [0.0, 0.0, 0.0]);
    curve.extend_from_slice(&[0x0b, 0x0b]);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn generated_pcurve_block() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for [u, v] in [[0.25, 0.5], [0.75, 1.5]] {
        push_tagged_f64(&mut b, u);
        push_tagged_f64(&mut b, v);
    }
    b
}

fn generated_rational_pcurve_block() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x05nurbs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for ([u, v], weight) in [([0.25, 0.5], 1.0), ([0.75, 1.5], 0.5)] {
        push_tagged_f64(&mut b, u);
        push_tagged_f64(&mut b, v);
        push_tagged_f64(&mut b, weight);
    }
    b
}

fn generated_curve_block() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 2);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 2i64), (1.0, 2)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for point in [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]] {
        for coordinate in point {
            push_tagged_f64(&mut b, coordinate);
        }
    }
    b
}

fn generated_surface_block() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x04, 1);
    for _ in 0..4 {
        push_tagged_i64(&mut b, 0x15, 0);
    }
    push_tagged_i64(&mut b, 0x04, 2);
    push_tagged_i64(&mut b, 0x04, 2);
    for _ in 0..2 {
        for (k, m) in [(0.0, 1i64), (1.0, 1)] {
            push_tagged_f64(&mut b, k);
            push_tagged_i64(&mut b, 0x04, m);
        }
    }
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ] {
        for c in p {
            push_tagged_f64(&mut b, c);
        }
    }
    b
}

fn generated_rational_surface_block() -> Vec<u8> {
    let mut block = generated_surface_block();
    block.splice(0..6, b"\x0d\x05nurbs".iter().copied());
    let non_rational = generated_surface_block();
    let control_start = non_rational.len() - 4 * 3 * 9;
    let rational_control_start = control_start + 1;
    for pole in (0..4).rev() {
        let at = rational_control_start + pole * 3 * 9 + 3 * 9;
        let weight = [1.0f64, 0.8, 1.2, 1.0][pole];
        let mut tagged = vec![0x06];
        tagged.extend_from_slice(&weight.to_le_bytes());
        block.splice(at..at, tagged);
    }
    block
}

fn synthetic_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "cyl_spl_sur");
    t_dbl(&mut surface, 0.25);
    t_dbl(&mut surface, 0.75);
    t_vec(&mut surface, [0.0, 0.0, 2.0]);
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.002);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_rational_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_cyl_spl_sur_smbh();
    let old = generated_surface_block();
    let start = bytes
        .windows(old.len())
        .rposition(|window| window == old)
        .expect("generated solved surface cache");
    bytes.splice(start..start + old.len(), generated_rational_surface_block());
    bytes
}

fn synthetic_ref_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_cyl_spl_sur_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let asmheader = &records[0];
    let surface = &records[9];
    let marker = b"\x0f\x0d\x0bcyl_spl_sur";
    let relative = bytes[surface.offset..surface.offset + surface.len]
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    let target_start = surface.offset + relative;
    let target_end = surface.offset + surface.len - 1;
    let target = bytes[target_start..target_end].to_vec();

    let mut reference = Vec::new();
    reference.extend_from_slice(b"\x0f\x0d\x03ref\x04");
    reference.extend_from_slice(&0i64.to_le_bytes());
    reference.push(0x10);
    bytes.splice(target_start..target_end, reference);
    let asmheader_end = asmheader.offset + asmheader.len - 1;
    bytes.splice(asmheader_end..asmheader_end, target);
    bytes
}

fn synthetic_rb_blend_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[9];

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "rb_blend_spl_sur");
    push_u8_string(&mut surface, "blend_support_surface");
    t_subident(&mut surface, "plane");
    surface.extend_from_slice(&generated_surface_block());
    push_u8_string(&mut surface, "blend_support_surface");
    t_subident(&mut surface, "sphere");
    surface.extend_from_slice(&generated_surface_block());
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.3);
    t_dbl(&mut surface, -0.3);
    push_tagged_i64(&mut surface, 0x15, -1);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.001);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_partial_rb_blend_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_rb_blend_spl_sur_smbh();
    let marker = b"\x0e\x06sphere";
    let start = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    bytes.drain(start..start + marker.len());
    bytes
}

/// Two triangular faces sharing one edge: face 4 rests on a plane (analytic),
/// face 5 on a `spline-surface` (undecoded → unknown-geometry carrier). The
/// shared edge 16 is used by coedge 10 (face 4, forward) and coedge 13 (face 5,
/// reversed), which must decode as mutually-referencing partners.
fn synthetic_mixed_smbh() -> Vec<u8> {
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "231.6.3.65535");
    t_end(&mut r);

    // 1: body
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_region
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: region
    t_ident(&mut r, "region");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 3); // first_shell
    t_ref(&mut r, 1); // owner_body
    t_end(&mut r);

    // 3: shell (first_face = 4)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, 4); // first_face
    t_ref(&mut r, -1);
    t_ref(&mut r, 2); // owner_region
    t_end(&mut r);

    // Face builder: next_face, first_loop, surface.
    let face = |r: &mut Vec<u8>, next: i64, first_loop: i64, surface: i64| {
        t_ident(r, "face");
        t_ref(r, -1); // 0 attrib
        t_long(r, -1); // 1 history
        t_ref(r, -1); // 2 null
        t_ref(r, next); // 3 next_face
        t_ref(r, first_loop); // 4 first_loop
        t_ref(r, 3); // 5 owner_shell
        t_ref(r, -1); // 6 null
        t_ref(r, surface); // 7 surface
        r.push(0x0b); // 8 sense forward
        r.push(0x0b); // 9 sides single
        t_end(r);
    };
    face(&mut r, 5, 6, 8); // 4: plane face
    face(&mut r, -1, 7, 9); // 5: spline face

    // Loop builder: first_coedge, owner_face.
    let lp = |r: &mut Vec<u8>, first_coedge: i64, owner_face: i64| {
        t_ident(r, "loop");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, -1); // next_loop
        t_ref(r, first_coedge);
        t_ref(r, owner_face);
        t_end(r);
    };
    lp(&mut r, 10, 4); // 6: loop of face 4
    lp(&mut r, 13, 5); // 7: loop of face 5

    // 8: plane-surface
    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_pos(&mut r, [0.0, 0.0, 0.0]);
    t_vec(&mut r, [0.0, 0.0, 1.0]);
    t_vec(&mut r, [1.0, 0.0, 0.0]);
    r.push(0x0b);
    t_end(&mut r);

    // 9: spline-surface (undecoded carrier; only needs to frame cleanly)
    t_subident(&mut r, "spline");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_dbl(&mut r, 0.0);
    r.push(0x0b);
    t_end(&mut r);

    // Coedge builder: next, prev, partner, edge, sense_reversed, owner_loop.
    let ce =
        |r: &mut Vec<u8>, next: i64, prev: i64, partner: i64, edge: i64, rev: bool, owner: i64| {
            t_ident(r, "coedge");
            t_ref(r, -1); // 0 attrib
            t_long(r, -1); // 1 history
            t_ref(r, -1); // 2 null
            t_ref(r, next); // 3 next
            t_ref(r, prev); // 4 prev
            t_ref(r, partner); // 5 partner
            t_ref(r, edge); // 6 edge
            r.push(if rev { 0x0a } else { 0x0b }); // 7 sense
            t_ref(r, owner); // 8 owner_loop
            t_long(r, 0); // 9 reserved
            t_ref(r, -1); // 10 pcurve
            t_end(r);
        };
    // Loop of face 4: 10 -> 11 -> 12 -> 10; coedge 10 partners coedge 13.
    ce(&mut r, 11, 12, 13, 16, false, 6); // 10 (shared edge, forward)
    ce(&mut r, 12, 10, -1, 17, false, 6); // 11
    ce(&mut r, 10, 11, -1, 18, false, 6); // 12
                                          // Loop of face 5: 13 -> 14 -> 15 -> 13; coedge 13 partners coedge 10.
    ce(&mut r, 14, 15, 10, 16, true, 7); // 13 (shared edge, reversed)
    ce(&mut r, 15, 13, -1, 19, false, 7); // 14
    ce(&mut r, 13, 14, -1, 20, false, 7); // 15

    // Edge builder: start_vertex, end_vertex.
    let edge = |r: &mut Vec<u8>, start: i64, end: i64| {
        t_ident(r, "edge");
        t_ref(r, -1); // 0 attrib
        t_long(r, -1); // 1 history
        t_ref(r, -1); // 2 null
        t_ref(r, start); // 3 start_vertex
        t_dbl(r, 0.0); // 4 t_start
        t_ref(r, end); // 5 end_vertex
        t_dbl(r, 1.0); // 6 t_end
        t_ref(r, -1); // 7 owner_coedge
        t_ref(r, -1); // 8 curve (none)
        r.push(0x0b); // 9 sense
        push_u8_string(r, "unknown"); // 10 continuity
        t_end(r);
    };
    edge(&mut r, 21, 22); // 16 A->B (shared)
    edge(&mut r, 22, 23); // 17 B->C
    edge(&mut r, 23, 21); // 18 C->A
    edge(&mut r, 21, 24); // 19 A->D
    edge(&mut r, 24, 22); // 20 D->B

    // Vertex builder: owning_edge, point.
    let vert = |r: &mut Vec<u8>, owning_edge: i64, point: i64| {
        t_ident(r, "vertex");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, owning_edge);
        t_long(r, 0);
        t_ref(r, point);
        t_end(r);
    };
    vert(&mut r, 16, 25); // 21 A
    vert(&mut r, 16, 26); // 22 B
    vert(&mut r, 17, 27); // 23 C
    vert(&mut r, 19, 28); // 24 D

    // Points.
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
    ] {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, p);
        t_long(&mut r, 1);
        t_end(&mut r);
    }

    // History boundary.
    t_ident(&mut r, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

/// Wrap an active-slice byte blob into a `.f3d` ZIP as the authoritative `.smbh`.
fn f3d_with_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(b"synthetic-manifest").unwrap();
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn generated_f3d_replays_byte_exactly_and_rejects_semantic_edits() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();

    let mut replayed = Vec::new();
    F3dCodec
        .write_preserved(&decoded.ir, &mut replayed)
        .unwrap();
    assert_eq!(replayed, source);

    let mut point_edited = decoded.ir.clone();
    point_edited.model.points[0].position.x += 12.5;
    let cadmpeg_ir::geometry::SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &mut point_edited.model.surfaces[0].geometry
    else {
        panic!("generated carrier must be a plane")
    };
    origin.z += 25.0;
    *normal = cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0);
    *u_axis = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&point_edited, &mut regenerated)
        .unwrap();
    assert_ne!(regenerated, source);
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir.model.points[0].position,
        point_edited.model.points[0].position
    );
    assert_eq!(
        round_trip.ir.model.surfaces[0].geometry,
        point_edited.model.surfaces[0].geometry
    );

    let mut modified = decoded.ir;
    modified.model.bodies[0].name = Some("edited".into());
    let error = F3dCodec
        .write_preserved(&modified, &mut Vec::new())
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(_)
    ));
}

#[test]
fn generated_source_less_planar_triangle_writes_native_f3d() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less F3D encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less F3D round trip");

    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(round_trip.ir.model.faces.len(), 1);
    assert_eq!(round_trip.ir.model.loops.len(), 1);
    assert_eq!(round_trip.ir.model.coedges.len(), 3);
    assert_eq!(round_trip.ir.model.edges.len(), 3);
    assert_eq!(round_trip.ir.model.vertices.len(), 3);
    assert_eq!(round_trip.ir.model.points, source_less.model.points);
    assert_eq!(round_trip.ir.model.surfaces, source_less.model.surfaces);
}

#[test]
fn generated_source_less_writes_document_tolerance_contract() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    source_less.tolerances.linear = 2.5e-7;
    source_less.tolerances.angular = 4.0e-11;

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less tolerance encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less tolerance round trip");
    assert_eq!(round_trip.ir.tolerances, source_less.tolerances);
}

#[test]
fn generated_source_less_rejects_body_kind_that_conflicts_with_incidence() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    assert_eq!(
        source_less.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    source_less.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;

    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("open face cannot be emitted as a solid body");
    assert!(matches!(error, cadmpeg_ir::codec::CodecError::Malformed(_)));
}

#[test]
fn generated_source_less_planar_polygon_plans_dynamic_record_indices() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, PointId, VertexId};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();

    let point_id = PointId("generated:point#3".into());
    source_less.model.points.push(cadmpeg_ir::topology::Point {
        id: point_id.clone(),
        position: cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
    });
    let vertex_id = VertexId("generated:vertex#3".into());
    source_less
        .model
        .vertices
        .push(cadmpeg_ir::topology::Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
        });
    let first_vertex = source_less.model.edges[0].start.clone();
    source_less.model.edges[2].end = vertex_id.clone();
    let edge_id = EdgeId("generated:edge#3".into());
    source_less.model.edges.push(cadmpeg_ir::topology::Edge {
        id: edge_id.clone(),
        curve: None,
        start: vertex_id,
        end: first_vertex,
        param_range: Some([0.0, 1.0]),
        tolerance: None,
    });
    let coedge_id = CoedgeId("generated:coedge#3".into());
    let loop_id = source_less.model.loops[0].id.clone();
    source_less
        .model
        .coedges
        .push(cadmpeg_ir::topology::Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id,
            edge: edge_id,
            next: coedge_id.clone(),
            previous: coedge_id.clone(),
            radial_next: coedge_id.clone(),
            sense: cadmpeg_ir::topology::Sense::Forward,
            pcurve: None,
        });
    source_less.model.loops[0].coedges.push(coedge_id);
    let ring = source_less.model.loops[0].coedges.clone();
    for (index, id) in ring.iter().enumerate() {
        let coedge = source_less
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == *id)
            .unwrap();
        coedge.next = ring[(index + 1) % ring.len()].clone();
        coedge.previous = ring[(index + ring.len() - 1) % ring.len()].clone();
    }

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less polygon encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less polygon round trip");

    assert_eq!(round_trip.ir.model.coedges.len(), 4);
    assert_eq!(round_trip.ir.model.edges.len(), 4);
    assert_eq!(round_trip.ir.model.vertices.len(), 4);
    assert_eq!(round_trip.ir.model.points.len(), 4);
    assert_eq!(
        round_trip
            .ir
            .model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>(),
        source_less
            .model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generated_source_less_planar_face_writes_straight_edge_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();

    for index in 0..source_less.model.edges.len() {
        let edge = &source_less.model.edges[index];
        let start = source_less
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.start)
            .and_then(|vertex| {
                source_less
                    .model
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
            })
            .unwrap()
            .position;
        let end = source_less
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.end)
            .and_then(|vertex| {
                source_less
                    .model
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
            })
            .unwrap()
            .position;
        let delta =
            cadmpeg_ir::math::Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let length = delta.norm();
        let direction =
            cadmpeg_ir::math::Vector3::new(delta.x / length, delta.y / length, delta.z / length);
        let id = CurveId(format!("generated:curve#{index}"));
        source_less.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Line {
                origin: start,
                direction,
            },
        });
        source_less.model.edges[index].curve = Some(id);
        source_less.model.edges[index].param_range = Some([0.0, length]);
    }

    let expected = source_less
        .model
        .curves
        .iter()
        .map(|curve| curve.geometry.clone())
        .collect::<Vec<_>>();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less line-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less line-carrier round trip");
    assert_eq!(round_trip.ir.model.curves.len(), expected.len());
    for (actual, expected) in round_trip.ir.model.curves.iter().zip(expected) {
        let (
            CurveGeometry::Line {
                origin: actual_origin,
                direction: actual_direction,
            },
            CurveGeometry::Line {
                origin: expected_origin,
                direction: expected_direction,
            },
        ) = (&actual.geometry, expected)
        else {
            panic!("expected line carriers")
        };
        assert_eq!(*actual_origin, expected_origin);
        assert!((actual_direction.x - expected_direction.x).abs() < 1e-14);
        assert!((actual_direction.y - expected_direction.y).abs() < 1e-14);
        assert!((actual_direction.z - expected_direction.z).abs() < 1e-14);
    }
    assert!(round_trip
        .ir
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_some()));
}

#[test]
fn generated_source_less_planar_face_writes_circle_edge_carrier() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let curve_id = CurveId("generated:circle#0".into());
    let expected = CurveGeometry::Circle {
        center: cadmpeg_ir::math::Point3::new(4.0, -2.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        radius: 6.5,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected.clone(),
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([0.25, 1.75]);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less circle-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less circle-carrier round trip");
    assert_eq!(round_trip.ir.model.curves[0].geometry, expected);
    assert_eq!(round_trip.ir.model.edges[0].param_range, Some([0.25, 1.75]));
    assert!(round_trip.ir.model.edges[0].curve.is_some());
}

#[test]
fn generated_source_less_planar_face_writes_ellipse_edge_carrier() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let curve_id = CurveId("generated:ellipse#0".into());
    let expected = CurveGeometry::Ellipse {
        center: cadmpeg_ir::math::Point3::new(-3.0, 5.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        major_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 8.0,
        minor_radius: 2.0,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected.clone(),
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([0.5, 2.0]);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less ellipse-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less ellipse-carrier round trip");
    assert_eq!(round_trip.ir.model.curves[0].geometry, expected);
    assert_eq!(round_trip.ir.model.edges[0].param_range, Some([0.5, 2.0]));
}

#[test]
fn generated_source_less_face_writes_cylinder_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = SurfaceGeometry::Cylinder {
        origin: cadmpeg_ir::math::Point3::new(2.0, -4.0, 6.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 7.5,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less cylinder encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cylinder round trip");
    assert_eq!(round_trip.ir.model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_signed_sphere_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = SurfaceGeometry::Sphere {
        center: cadmpeg_ir::math::Point3::new(-2.0, 4.0, 8.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: -3.5,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less sphere encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sphere round trip");
    assert_eq!(round_trip.ir.model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_cone_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = SurfaceGeometry::Cone {
        origin: cadmpeg_ir::math::Point3::new(1.0, 3.0, -5.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 9.0,
        half_angle: 0.5,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less cone encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cone round trip");
    assert_eq!(round_trip.ir.model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_signed_torus_surface_carrier() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = SurfaceGeometry::Torus {
        center: cadmpeg_ir::math::Point3::new(3.0, -6.0, 9.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.5,
        minor_radius: -6.0,
    };
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less torus encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less torus round trip");
    assert_eq!(round_trip.ir.model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_nurbs_surface_carrier() {
    use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![-1.0, -1.0, 2.0, 2.0],
        v_knots: vec![-2.0, -2.0, 3.0, 3.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 2.0),
            cadmpeg_ir::math::Point3::new(20.0, 0.0, 3.0),
            cadmpeg_ir::math::Point3::new(20.0, 10.0, 4.0),
        ],
        weights: None,
        u_periodic: true,
        v_periodic: false,
    });
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less NURBS surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less NURBS surface round trip");
    assert_eq!(round_trip.ir.model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_surface_carrier() {
    use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 8.0, 1.0),
            cadmpeg_ir::math::Point3::new(12.0, 0.0, 2.0),
            cadmpeg_ir::math::Point3::new(12.0, 8.0, 3.0),
        ],
        weights: Some(vec![1.0, 0.75, 1.25, 1.0]),
        u_periodic: false,
        v_periodic: true,
    });
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less rational NURBS surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational NURBS surface round trip");
    assert_eq!(round_trip.ir.model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_edge_curve() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let curve_id = CurveId("generated:nurbs_curve#0".into());
    let expected = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(5.0, 8.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 2.0),
        ],
        weights: Some(vec![1.0, 0.6, 1.0]),
        periodic: true,
    });
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected.clone(),
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([-1.0, 2.0]);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less rational NURBS curve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational NURBS curve round trip");
    assert_eq!(round_trip.ir.model.curves[0].geometry, expected);
    assert_eq!(round_trip.ir.model.edges[0].param_range, Some([-1.0, 2.0]));
}

#[test]
fn generated_source_less_face_writes_inline_nurbs_pcurve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated inline pcurve decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = source_less.model.pcurves[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less inline pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less inline pcurve round trip");
    assert_eq!(round_trip.ir.model.pcurves.len(), 1);
    assert_eq!(round_trip.ir.model.pcurves[0].geometry, expected.geometry);
    assert_eq!(
        round_trip.ir.model.pcurves[0].wrapper_reversed,
        expected.wrapper_reversed
    );
    assert_eq!(
        round_trip.ir.model.pcurves[0].parameter_range,
        expected.parameter_range
    );
    assert_eq!(
        round_trip.ir.model.pcurves[0].fit_tolerance,
        expected.fit_tolerance
    );
    assert_eq!(
        round_trip
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.pcurve.is_some())
            .count(),
        1
    );
}

#[test]
fn generated_source_less_face_writes_rational_nurbs_pcurve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_rational_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated rational pcurve decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = source_less.model.pcurves[0].clone();
    assert!(matches!(
        &expected.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            weights: Some(weights),
            ..
        } if weights == &vec![1.0, 0.5]
    ));

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less rational pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rational pcurve round trip");
    assert_eq!(round_trip.ir.model.pcurves.len(), 1);
    let actual = &round_trip.ir.model.pcurves[0];
    assert_eq!(actual.geometry, expected.geometry);
    assert_eq!(actual.wrapper_reversed, expected.wrapper_reversed);
    assert_eq!(actual.parameter_range, expected.parameter_range);
    assert_eq!(actual.fit_tolerance, expected.fit_tolerance);
}

#[test]
fn generated_source_less_two_faces_preserve_shared_radial_edge() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected_surface = SurfaceGeometry::Cylinder {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_line#0".into());
    let expected_curve = CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
    });
    source_less.model.edges[0].curve = Some(curve_id);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less shared-edge encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less shared-edge round trip");
    assert_eq!(round_trip.ir.model.faces.len(), 2);
    assert_eq!(round_trip.ir.model.loops.len(), 2);
    assert_eq!(round_trip.ir.model.coedges.len(), 6);
    assert_eq!(round_trip.ir.model.edges.len(), 5);
    assert_eq!(round_trip.ir.model.vertices.len(), 4);
    assert_eq!(round_trip.ir.model.surfaces.len(), 2);
    assert_eq!(round_trip.ir.model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir.model.curves[0].geometry, expected_curve);
    assert!(round_trip.ir.model.edges[0].curve.is_some());
    let shared = round_trip
        .ir
        .model
        .edges
        .iter()
        .find(|edge| {
            round_trip
                .ir
                .model
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == edge.id)
                .count()
                == 2
        })
        .expect("shared radial edge");
    let radial = round_trip
        .ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == shared.id)
        .collect::<Vec<_>>();
    assert_eq!(radial.len(), 2);
    assert_eq!(radial[0].radial_next, radial[1].id);
    assert_eq!(radial[1].radial_next, radial[0].id);
}

#[test]
fn generated_source_less_face_preserves_multiple_loop_chain() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId, PointId, VertexId};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();

    let loop_id = LoopId("generated:loop#1".into());
    let mut coedge_ids = Vec::new();
    let coordinates = [[2.0, 2.0, 0.0], [4.0, 2.0, 0.0], [2.0, 4.0, 0.0]];
    for (index, [x, y, z]) in coordinates.into_iter().enumerate() {
        let point_id = PointId(format!("generated:inner_point#{index}"));
        source_less.model.points.push(cadmpeg_ir::topology::Point {
            id: point_id.clone(),
            position: cadmpeg_ir::math::Point3::new(x, y, z),
        });
        let vertex_id = VertexId(format!("generated:inner_vertex#{index}"));
        source_less
            .model
            .vertices
            .push(cadmpeg_ir::topology::Vertex {
                id: vertex_id,
                point: point_id,
                tolerance: None,
            });
    }
    let inner_vertices = source_less.model.vertices[3..]
        .iter()
        .map(|vertex| vertex.id.clone())
        .collect::<Vec<_>>();
    for index in 0..3 {
        let edge_id = EdgeId(format!("generated:inner_edge#{index}"));
        source_less.model.edges.push(cadmpeg_ir::topology::Edge {
            id: edge_id.clone(),
            curve: None,
            start: inner_vertices[index].clone(),
            end: inner_vertices[(index + 1) % 3].clone(),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        });
        let coedge_id = CoedgeId(format!("generated:inner_coedge#{index}"));
        coedge_ids.push(coedge_id.clone());
        source_less
            .model
            .coedges
            .push(cadmpeg_ir::topology::Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id,
                sense: cadmpeg_ir::topology::Sense::Reversed,
                pcurve: None,
            });
    }
    for index in 0..3 {
        let coedge = source_less
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_ids[index])
            .unwrap();
        coedge.next = coedge_ids[(index + 1) % 3].clone();
        coedge.previous = coedge_ids[(index + 2) % 3].clone();
    }
    let face_id = source_less.model.faces[0].id.clone();
    source_less.model.loops.push(cadmpeg_ir::topology::Loop {
        id: loop_id.clone(),
        face: face_id,
        coedges: coedge_ids,
    });
    source_less.model.faces[0].loops.push(loop_id);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less multiple-loop encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multiple-loop round trip");
    assert_eq!(round_trip.ir.model.faces.len(), 1);
    assert_eq!(round_trip.ir.model.loops.len(), 2);
    assert_eq!(round_trip.ir.model.faces[0].loops.len(), 2);
    assert_eq!(round_trip.ir.model.coedges.len(), 6);
    assert_eq!(round_trip.ir.model.edges.len(), 6);
}

#[test]
fn generated_source_less_multi_face_writes_nurbs_carriers_and_pcurve() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
    use cadmpeg_ir::ids::{CurveId, PcurveId};

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let pcurve_source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let pcurve = F3dCodec
        .decode(&mut Cursor::new(pcurve_source), &DecodeOptions::default())
        .expect("generated pcurve decode")
        .ir
        .model
        .pcurves
        .into_iter()
        .next()
        .expect("generated pcurve");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();

    let expected_surface = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 2.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 3.0),
        ],
        weights: Some(vec![1.0, 0.8, 1.2, 1.0]),
        u_periodic: false,
        v_periodic: true,
    });
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_nurbs#0".into());
    let expected_curve = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(5.0, 3.0, 1.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        weights: Some(vec![1.0, 0.7, 1.0]),
        periodic: false,
    });
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
    });
    source_less.model.edges[0].curve = Some(curve_id);
    let pcurve_id = PcurveId("generated:pcurve#0".into());
    let mut pcurve = pcurve;
    pcurve.id = pcurve_id.clone();
    let expected_pcurve = pcurve.geometry.clone();
    source_less.model.pcurves.push(pcurve);
    source_less.model.coedges[0].pcurve = Some(pcurve_id);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less multi-face NURBS encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face NURBS round trip");
    assert_eq!(round_trip.ir.model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir.model.curves[0].geometry, expected_curve);
    assert_eq!(round_trip.ir.model.pcurves[0].geometry, expected_pcurve);
    assert_eq!(
        round_trip
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.pcurve.is_some())
            .count(),
        1
    );
}

#[test]
fn generated_source_less_unit_cube_writes_closed_shared_edge_shell() {
    let source_less = cadmpeg_ir::examples::unit_cube();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less unit cube encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less unit cube round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
    assert_eq!(round_trip.ir.model.regions.len(), 1);
    assert_eq!(round_trip.ir.model.shells.len(), 1);
    assert_eq!(round_trip.ir.model.faces.len(), 6);
    assert_eq!(round_trip.ir.model.loops.len(), 6);
    assert_eq!(round_trip.ir.model.coedges.len(), 24);
    assert_eq!(round_trip.ir.model.edges.len(), 12);
    assert_eq!(round_trip.ir.model.vertices.len(), 8);
    assert_eq!(round_trip.ir.model.points.len(), 8);
    assert!(round_trip.ir.model.edges.iter().all(|edge| {
        round_trip
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .count()
            == 2
    }));
    let report = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn generated_source_less_multi_face_writes_torus_and_circle_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected_surface = SurfaceGeometry::Torus {
        center: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 8.0,
        minor_radius: -3.0,
    };
    source_less.model.surfaces[1].geometry = expected_surface.clone();
    let curve_id = CurveId("generated:shared_circle#0".into());
    let expected_curve = CurveGeometry::Circle {
        center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: expected_curve.clone(),
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([0.25, 1.5]);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less multi-face torus encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face torus round trip");
    assert_eq!(round_trip.ir.model.surfaces[1].geometry, expected_surface);
    assert_eq!(round_trip.ir.model.curves[0].geometry, expected_curve);
    assert_eq!(round_trip.ir.model.edges[0].param_range, Some([0.25, 1.5]));
}

#[test]
fn generated_source_less_multi_face_writes_cone_sphere_and_ellipse_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let source = f3d_with_smbh(&synthetic_mixed_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated shared-edge decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 8.0,
        half_angle: 0.35,
    };
    let sphere = SurfaceGeometry::Sphere {
        center: Point3::new(-1.0, 4.0, 2.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -12.0,
    };
    source_less.model.surfaces[0].geometry = cone.clone();
    source_less.model.surfaces[1].geometry = sphere.clone();
    let curve_id = CurveId("generated:shared_ellipse#0".into());
    let ellipse = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 9.0,
        minor_radius: 4.0,
    };
    source_less.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: ellipse.clone(),
    });
    source_less.model.edges[0].curve = Some(curve_id);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less multi-face analytic encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-face analytic round trip");
    assert_eq!(round_trip.ir.model.surfaces[0].geometry, cone);
    assert_eq!(round_trip.ir.model.surfaces[1].geometry, sphere);
    assert_eq!(round_trip.ir.model.curves[0].geometry, ellipse);
}

#[test]
fn generated_source_less_writes_translational_extrusion_definition() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = source_less.model.procedural_surfaces[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less extrusion round trip");
    assert_eq!(round_trip.ir.model.procedural_surfaces.len(), 1);
    let actual = &round_trip.ir.model.procedural_surfaces[0];
    assert_eq!(actual.definition, expected.definition);
    assert_eq!(actual.cache_fit_tolerance, expected.cache_fit_tolerance);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
        directrix,
        direction,
    } = &actual.definition
    else {
        panic!("expected extrusion definition")
    };
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *directrix));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
}

#[test]
fn generated_source_less_writes_rolling_ball_blend_definition() {
    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = source_less.model.procedural_surfaces[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less rolling-ball encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rolling-ball round trip");
    assert_eq!(round_trip.ir.model.procedural_surfaces.len(), 1);
    let actual = &round_trip.ir.model.procedural_surfaces[0];
    assert_eq!(actual.definition, expected.definition);
    assert_eq!(actual.cache_fit_tolerance, expected.cache_fit_tolerance);
}

#[test]
fn generated_source_less_unit_cube_writes_body_transform() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let expected = cadmpeg_ir::transform::Transform {
        rows: [
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, -30.0],
            [0.0, 0.0, 1.0, 40.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    source_less.model.bodies[0].transform = Some(expected);
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less transformed cube encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less transformed cube round trip");
    assert_eq!(round_trip.ir.model.bodies[0].transform, Some(expected));
}

#[test]
fn generated_source_less_unit_cube_writes_body_and_face_colors() {
    use cadmpeg_ir::topology::Color;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let body_color = Color {
        r: 0.1,
        g: 0.2,
        b: 0.3,
        a: 1.0,
    };
    let face_color = Color {
        r: 0.65,
        g: 0.45,
        b: 0.25,
        a: 1.0,
    };
    source_less.model.bodies[0].color = Some(body_color);
    source_less.model.faces[2].color = Some(face_color);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less colored cube encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less colored cube round trip");
    assert_eq!(round_trip.ir.model.bodies[0].color, Some(body_color));
    assert_eq!(round_trip.ir.model.faces[2].color, Some(face_color));
    assert!(round_trip
        .ir
        .model
        .faces
        .iter()
        .enumerate()
        .all(|(ordinal, face)| ordinal == 2 || face.color.is_none()));
}

#[test]
fn generated_source_less_writes_persistent_body_and_sketch_provenance_attributes() {
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::design::{PersistentDesignLink, SketchCurveLink};
    use cadmpeg_ir::topology::Color;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.bodies[0].color = Some(Color {
        r: 0.2,
        g: 0.4,
        b: 0.6,
        a: 1.0,
    });
    source_less.model.faces[0].color = Some(Color {
        r: 0.7,
        g: 0.3,
        b: 0.1,
        a: 1.0,
    });
    let body_id = source_less.model.bodies[0].id.clone();
    let face_id = source_less.model.faces[0].id.clone();
    let edge_id = source_less.model.edges[0].id.clone();
    let coedge_id = source_less.model.coedges[0].id.clone();
    let native = source_less
        .native
        .f3d
        .get_or_insert_with(cadmpeg_ir::native::F3dNative::default);
    native.persistent_design_links = vec![
        PersistentDesignLink {
            id: "generated:persistent-design-link#0".into(),
            target: AttributeTarget::Body(body_id.clone()),
            design_id: "311".into(),
            ordinal: 0,
            is_current: false,
        },
        PersistentDesignLink {
            id: "generated:persistent-design-link#1".into(),
            target: AttributeTarget::Body(body_id),
            design_id: "322".into(),
            ordinal: 1,
            is_current: true,
        },
        PersistentDesignLink {
            id: "generated:persistent-design-link#2".into(),
            target: AttributeTarget::Face(face_id),
            design_id: "411".into(),
            ordinal: 0,
            is_current: true,
        },
        PersistentDesignLink {
            id: "generated:persistent-design-link#3".into(),
            target: AttributeTarget::Edge(edge_id),
            design_id: "511".into(),
            ordinal: 0,
            is_current: true,
        },
    ];
    native.sketch_curve_links = vec![SketchCurveLink {
        id: "generated:sketch-curve-link#0".into(),
        coedge: coedge_id,
        sketch_curve_id: 113,
        signed_reference: Some(1),
        role: 2,
        closure: 3,
    }];

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less provenance attribute encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less provenance attribute round trip");
    let native = f3d_native(&round_trip.ir);
    assert_eq!(native.persistent_design_links.len(), 4);
    assert_eq!(native.persistent_design_links[0].design_id, "311");
    assert_eq!(native.persistent_design_links[1].design_id, "322");
    assert!(native.persistent_design_links[1].is_current);
    assert!(native.persistent_design_links.iter().any(|link| {
        link.design_id == "411" && matches!(link.target, AttributeTarget::Face(_))
    }));
    assert!(native.persistent_design_links.iter().any(|link| {
        link.design_id == "511" && matches!(link.target, AttributeTarget::Edge(_))
    }));
    assert_eq!(native.sketch_curve_links.len(), 1);
    assert_eq!(native.sketch_curve_links[0].sketch_curve_id, 113);
    assert_eq!(native.sketch_curve_links[0].signed_reference, Some(1));
    assert_eq!(native.sketch_curve_links[0].role, 2);
    assert_eq!(native.sketch_curve_links[0].closure, 3);
    assert_eq!(
        round_trip.ir.model.bodies[0].color,
        source_less.model.bodies[0].color
    );
    assert_eq!(
        round_trip.ir.model.faces[0].color,
        source_less.model.faces[0].color
    );
}

#[test]
fn generated_source_less_writes_two_independent_cube_bodies() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical cube JSON")
        .replace("synthetic:cube:", "synthetic:cube_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second cube IR");
    second.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform {
        rows: [
            [1.0, 0.0, 0.0, 30.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    source_less.model.bodies.append(&mut second.model.bodies);
    source_less.model.regions.append(&mut second.model.regions);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.faces.append(&mut second.model.faces);
    source_less.model.loops.append(&mut second.model.loops);
    source_less.model.coedges.append(&mut second.model.coedges);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less
        .model
        .surfaces
        .append(&mut second.model.surfaces);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less two-body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-body round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 2);
    assert_eq!(round_trip.ir.model.regions.len(), 2);
    assert_eq!(round_trip.ir.model.shells.len(), 2);
    assert_eq!(round_trip.ir.model.faces.len(), 12);
    assert_eq!(round_trip.ir.model.edges.len(), 24);
    assert_eq!(round_trip.ir.model.points.len(), 16);
    assert_eq!(
        round_trip.ir.model.bodies[1]
            .transform
            .expect("second body transform")
            .rows[0][3],
        30.0
    );
    let report = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn generated_source_less_writes_typed_asm_history_graph() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated history decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = f3d_native(&source_less).asm_histories[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less history encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less history round trip");
    let actual = &f3d_native(&round_trip.ir).asm_histories[0];
    assert_eq!(actual.stream_size, expected.stream_size);
    assert_eq!(actual.high_water_mark, expected.high_water_mark);
    assert_eq!(actual.states.len(), expected.states.len());
    assert_eq!(actual.states[0].state_id, expected.states[0].state_id);
    assert_eq!(actual.states[0].bulletin_boards.len(), 1);
    assert_eq!(actual.states[0].bulletin_boards[0].changes.len(), 2);
    assert_eq!(actual.states[0].records.len(), 1);
    assert_eq!(actual.states[0].records[0].name, "history_payload");
}

#[test]
fn generated_source_less_writes_design_object_metastream() {
    use cadmpeg_ir::design::{DesignObject, DesignObjectKind};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let native = source_less
        .native
        .f3d
        .get_or_insert_with(cadmpeg_ir::native::F3dNative::default);
    native.design_objects = vec![
        DesignObject {
            id: "generated:design-object#0".into(),
            byte_offset: 0,
            kind: DesignObjectKind::Fusion,
            entity_ids: vec![1, 2],
            entity_id_offsets: Vec::new(),
            self_guid: "11111111-2222-3333-4444-555555555555".into(),
            self_guid_offset: 0,
            parent_guid: None,
            parent_guid_offset: None,
            revision: 7,
            revision_offset: 0,
        },
        DesignObject {
            id: "generated:design-object#1".into(),
            byte_offset: 0,
            kind: DesignObjectKind::Sketch,
            entity_ids: vec![277],
            entity_id_offsets: Vec::new(),
            self_guid: "22222222-3333-4444-5555-666666666666".into(),
            self_guid_offset: 0,
            parent_guid: Some("11111111-2222-3333-4444-555555555555".into()),
            parent_guid_offset: None,
            revision: 9,
            revision_offset: 0,
        },
    ];

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less Design MetaStream encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design MetaStream round trip");
    let objects = &f3d_native(&round_trip.ir).design_objects;
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0].kind, DesignObjectKind::Fusion);
    assert_eq!(objects[0].entity_ids, [1, 2]);
    assert_eq!(objects[0].revision, 7);
    assert_eq!(objects[1].kind, DesignObjectKind::Sketch);
    assert_eq!(objects[1].entity_ids, [277]);
    assert_eq!(
        objects[1].parent_guid.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(objects[1].revision, 9);
}

#[test]
fn generated_source_less_writes_design_recipes_and_persistent_references() {
    use cadmpeg_ir::design::{
        ConstructionRecipe, ConstructionRecipeKind, LostEdgeReference, PersistentReference,
        PersistentReferenceKind,
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let native = source_less
        .native
        .f3d
        .get_or_insert_with(cadmpeg_ir::native::F3dNative::default);
    native.construction_recipes = [
        ConstructionRecipeKind::Body,
        ConstructionRecipeKind::Face,
        ConstructionRecipeKind::BoundedFace,
        ConstructionRecipeKind::Edge,
        ConstructionRecipeKind::Vertex,
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, kind)| ConstructionRecipe {
        id: format!("generated:recipe#{ordinal}"),
        byte_offset: 0,
        record_index_offset: None,
        kind,
        design_id: Some(format!("{}", 320 + ordinal)),
        design_id_offset: None,
        design_id_binary_u32: false,
        recipe_index: 0,
        record_index: 100 + i32::try_from(ordinal).unwrap(),
    })
    .collect();
    native.persistent_references = vec![
        PersistentReference {
            id: "generated:persistent-reference#0".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::Point,
            value: 700,
        },
        PersistentReference {
            id: "generated:persistent-reference#1".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::CurvePrimary,
            value: 701,
        },
        PersistentReference {
            id: "generated:persistent-reference#2".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::CurveSecondary,
            value: 702,
        },
    ];
    native.lost_edge_references = vec![LostEdgeReference {
        id: "generated:lost-edge-reference#0".into(),
        byte_offset: 0,
        class_tag_offset: 0,
        class_tag: "419".into(),
        record_index: 4646,
        record_index_offset: 0,
    }];

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less Design BulkStream encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design BulkStream round trip");
    let native = f3d_native(&round_trip.ir);
    assert_eq!(native.construction_recipes.len(), 5);
    let body_recipe = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::Body)
        .expect("body recipe");
    assert_eq!(body_recipe.record_index, 100);
    assert_eq!(body_recipe.design_id.as_deref(), Some("320"));
    assert!(native
        .construction_recipes
        .iter()
        .any(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace));
    assert_eq!(native.persistent_references.len(), 3);
    assert_eq!(native.persistent_references[0].value, 700);
    assert_eq!(
        native.persistent_references[1].kind,
        PersistentReferenceKind::CurvePrimary
    );
    assert_eq!(native.lost_edge_references.len(), 1);
    assert_eq!(native.lost_edge_references[0].class_tag, "419");
    assert_eq!(native.lost_edge_references[0].record_index, 4646);
}

#[test]
fn generated_source_less_writes_design_ownership_and_record_headers() {
    use cadmpeg_ir::design::{
        DesignBodyMember, DesignEntityHeader, DesignObject, DesignObjectKind, DesignRecordHeader,
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let native = source_less
        .native
        .f3d
        .get_or_insert_with(cadmpeg_ir::native::F3dNative::default);
    native.design_objects = vec![DesignObject {
        id: "generated:design-object#0".into(),
        byte_offset: 0,
        kind: DesignObjectKind::Sketch,
        entity_ids: vec![277],
        entity_id_offsets: Vec::new(),
        self_guid: "22222222-3333-4444-5555-666666666666".into(),
        self_guid_offset: 0,
        parent_guid: None,
        parent_guid_offset: None,
        revision: 4,
        revision_offset: 0,
    }];
    native.design_body_members = vec![
        DesignBodyMember {
            id: "generated:body-member#0".into(),
            byte_offset: 0,
            entity_suffix: 985,
            flags: 0,
        },
        DesignBodyMember {
            id: "generated:body-member#1".into(),
            byte_offset: 0,
            entity_suffix: 8422,
            flags: 3,
        },
    ];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "generated:entity-header#0".into(),
        byte_offset: 0,
        entity_suffix: 277,
        entity_id: "0_277".into(),
        class_tag: "269".into(),
        optional_slot_present: true,
        object_kind: Some(DesignObjectKind::Sketch),
        record_reference: Some(584),
        record_reference_offset: None,
        declared_reference_count: Some(2),
        reference_indices: vec![33, 44],
        reference_offsets: Vec::new(),
    }];
    native.design_record_headers = vec![
        DesignRecordHeader {
            id: "generated:record-header#0".into(),
            record_index: 33,
            class_tag: "350".into(),
            byte_offset: 0,
        },
        DesignRecordHeader {
            id: "generated:record-header#1".into(),
            record_index: 44,
            class_tag: "351".into(),
            byte_offset: 0,
        },
    ];

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less Design ownership encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design ownership round trip");
    let native = f3d_native(&round_trip.ir);
    assert_eq!(native.design_body_members.len(), 2);
    assert_eq!(native.design_body_members[0].entity_suffix, 985);
    assert_eq!(native.design_body_members[1].flags, 3);
    assert_eq!(native.design_entity_headers.len(), 1);
    assert_eq!(native.design_entity_headers[0].entity_id, "0_277");
    assert_eq!(native.design_entity_headers[0].record_reference, Some(584));
    assert_eq!(native.design_entity_headers[0].reference_indices, [33, 44]);
    assert_eq!(native.design_record_headers.len(), 2);
    assert_eq!(native.design_record_headers[0].record_index, 33);
    assert_eq!(native.design_record_headers[1].class_tag, "351");
}

#[test]
fn generated_source_less_writes_sketch_points_curves_and_constraints() {
    use cadmpeg_ir::design::{
        DesignEntityHeader, DesignObject, DesignObjectKind, SketchConstraintKind,
        SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchRelation,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let native = source_less
        .native
        .f3d
        .get_or_insert_with(cadmpeg_ir::native::F3dNative::default);
    native.design_objects = vec![DesignObject {
        id: "generated:sketch-object#0".into(),
        byte_offset: 0,
        kind: DesignObjectKind::Sketch,
        entity_ids: vec![277],
        entity_id_offsets: Vec::new(),
        self_guid: "22222222-3333-4444-5555-666666666666".into(),
        self_guid_offset: 0,
        parent_guid: None,
        parent_guid_offset: None,
        revision: 1,
        revision_offset: 0,
    }];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "generated:sketch-header#0".into(),
        byte_offset: 0,
        entity_suffix: 277,
        entity_id: "0_277".into(),
        class_tag: "269".into(),
        optional_slot_present: true,
        object_kind: Some(DesignObjectKind::Sketch),
        record_reference: Some(584),
        record_reference_offset: None,
        declared_reference_count: Some(1),
        reference_indices: vec![33],
        reference_offsets: Vec::new(),
    }];
    native.sketch_points = vec![SketchPoint {
        id: "generated:sketch-point#0".into(),
        record_index: 100,
        class_tag: "360".into(),
        byte_offset: 0,
        coordinate_offset: 96,
        persistent_id: 500,
        paired_reference: 101,
        coordinates: Point2::new(12.5, -25.0),
        raw_bytes: Vec::new(),
    }];
    native.sketch_curve_identities = vec![
        SketchCurveIdentity {
            id: "generated:sketch-curve#0".into(),
            record_index: 600,
            class_tag: "361".into(),
            byte_offset: 0,
            geometry_offset: 133,
            primary_id: 700,
            secondary_id: 701,
            geometry: Some(SketchCurveGeometry::Line {
                start: Point3::new(10.0, 20.0, 0.0),
                end: Point3::new(40.0, 20.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            }),
        },
        SketchCurveIdentity {
            id: "generated:sketch-curve#1".into(),
            record_index: 601,
            class_tag: "362".into(),
            byte_offset: 0,
            geometry_offset: 133,
            primary_id: 702,
            secondary_id: 703,
            geometry: Some(SketchCurveGeometry::Arc {
                center: Point3::new(5.0, 6.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                reference_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 30.0,
                start_angle: 0.25,
                end_angle: 2.5,
            }),
        },
        SketchCurveIdentity {
            id: "generated:sketch-curve#2".into(),
            record_index: 602,
            class_tag: "363".into(),
            byte_offset: 0,
            geometry_offset: 133,
            primary_id: 704,
            secondary_id: 705,
            geometry: Some(SketchCurveGeometry::Nurbs {
                carrier_reference: None,
                subtype_class_tag: "365".into(),
                subtype_record_index: 602,
                degree: 2,
                fit_tolerance: 1.0e-8,
                scalar_width: 8,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                weights: vec![1.0, 0.8, 1.0],
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(10.0, 20.0, 0.0),
                    Point3::new(30.0, 10.0, 0.0),
                ],
            }),
        },
    ];
    native.sketch_relations = vec![SketchRelation {
        id: "generated:sketch-relation#0".into(),
        record_index: 33,
        class_tag: "350".into(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 277,
        auxiliary_references: vec![900],
        members: vec![100, 600],
        state: 0x11,
        constraint_kinds: vec![
            SketchConstraintKind::Coincident,
            SketchConstraintKind::Parallel,
        ],
        unknown_constraint_bits: 0,
        return_members: vec![600, 100],
        raw_bytes: Vec::new(),
    }];

    let expected_geometries = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.geometry.clone().unwrap())
        .collect::<Vec<_>>();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less sketch BulkStream encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sketch BulkStream round trip");
    let native = f3d_native(&round_trip.ir);
    assert_eq!(native.sketch_points.len(), 1);
    assert_eq!(native.sketch_points[0].persistent_id, 500);
    assert_eq!(
        native.sketch_points[0].coordinates,
        Point2::new(12.5, -25.0)
    );
    assert_eq!(native.sketch_curve_identities.len(), 3);
    for expected in expected_geometries {
        assert!(native
            .sketch_curve_identities
            .iter()
            .any(|curve| curve.geometry.as_ref() == Some(&expected)));
    }
    assert_eq!(native.sketch_relations.len(), 1);
    assert_eq!(native.sketch_relations[0].members, [100, 600]);
    assert_eq!(native.sketch_relations[0].auxiliary_references, [900]);
    assert_eq!(native.sketch_relations[0].owner_reference, 277);
    assert_eq!(native.sketch_relations[0].state, 0x11);
    assert_eq!(native.sketch_relations[0].return_members, [600, 100]);
}

#[test]
fn generated_source_less_writes_act_table_channels_and_root_component() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::design::{ActEntity, ActGuid, ActRootComponent};

    let appearance_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let physical_guid = "cccccccc-1111-2222-3333-dddddddddddd";
    let standalone_guid = "eeeeeeee-1111-2222-3333-ffffffffffff";
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let native = source_less
        .native
        .f3d
        .get_or_insert_with(cadmpeg_ir::native::F3dNative::default);
    native.act_entities = vec![ActEntity {
        id: "generated:act-entity#0".into(),
        record_index: 7,
        table_record_index_offset: None,
        channel_record_index_offset: None,
        entity_id: "0_985".into(),
        table_entity_id_offset: None,
        channel_entity_id_offset: None,
        in_table: true,
        channel_class_tag: Some("261".into()),
        channels: BTreeMap::from([
            ("Appearance".into(), appearance_guid.into()),
            ("PhysicalMaterial".into(), physical_guid.into()),
        ]),
        channel_guid_offsets: BTreeMap::new(),
    }];
    native.act_guids = [standalone_guid, appearance_guid, physical_guid]
        .into_iter()
        .enumerate()
        .map(|(ordinal, guid)| ActGuid {
            id: format!("generated:act-guid#{ordinal}"),
            byte_offset: 0,
            guid_offset: 0,
            ordinal: u32::try_from(ordinal).unwrap(),
            guid: guid.into(),
        })
        .collect();
    native.act_root_components = vec![ActRootComponent {
        id: "generated:act-root#0".into(),
        byte_offset: 0,
        record_index: 9,
        record_index_offset: 0,
        class_tag: "267".into(),
        instance_root_record: 12,
        instance_root_record_offset: 0,
        components_root_record: 7,
        components_root_record_offset: 0,
        registry_flag: 1,
        registry_flag_offset: 0,
        entity_id: "0_3".into(),
        entity_id_offset: 0,
        display_name: "Generated Design".into(),
        display_name_offset: 0,
    }];

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less ACT encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less ACT round trip");
    let native = f3d_native(&round_trip.ir);
    assert_eq!(native.act_entities.len(), 1);
    assert!(native.act_entities[0].in_table);
    assert_eq!(native.act_entities[0].record_index, 7);
    assert_eq!(native.act_entities[0].entity_id, "0_985");
    assert_eq!(
        native.act_entities[0]
            .channels
            .get("Appearance")
            .map(String::as_str),
        Some(appearance_guid)
    );
    assert_eq!(native.act_guids.len(), 3);
    assert!(native
        .act_guids
        .iter()
        .any(|guid| guid.guid == standalone_guid));
    assert_eq!(native.act_root_components.len(), 1);
    assert_eq!(native.act_root_components[0].instance_root_record, 12);
    assert_eq!(native.act_root_components[0].components_root_record, 7);
    assert_eq!(
        native.act_root_components[0].display_name,
        "Generated Design"
    );
}

#[test]
fn generated_source_less_writes_protein_appearance_and_body_binding() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::design::{DesignMaterialAssignment, DesignObject, DesignObjectKind};
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let visual_guid = "11111111-2222-3333-4444-555555555555";
    let appearance_id = AppearanceId("generated:appearance#0".into());
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.appearances = vec![Appearance {
        id: appearance_id.clone(),
        name: Some("Prism-Generated".into()),
        asset_guid: Some(visual_guid.into()),
        visual_guid: Some(visual_guid.into()),
        physical_token: Some("PrismMaterial-Generated".into()),
        schema: Some("GenericSchema".into()),
        category: Some("Plastic/Generated".into()),
        base_color: Some(Color {
            r: 0.15,
            g: 0.35,
            b: 0.75,
            a: 1.0,
        }),
        properties: BTreeMap::from([
            ("reflectivity_at_0deg".into(), 0.25),
            ("refraction_index".into(), 1.5),
        ]),
    }];
    source_less.model.appearance_bindings = vec![AppearanceBinding {
        id: "generated:appearance-binding#0".into(),
        target: AppearanceTarget::Body(source_less.model.bodies[0].id.clone()),
        appearance: appearance_id,
        source_entity_id: Some("0_985".into()),
        object_type: Some("Body".into()),
        channels: BTreeMap::new(),
    }];
    let native = source_less
        .native
        .f3d
        .get_or_insert_with(cadmpeg_ir::native::F3dNative::default);
    native.design_objects = vec![DesignObject {
        id: "generated:body-object#0".into(),
        byte_offset: 0,
        kind: DesignObjectKind::Body,
        entity_ids: vec![985],
        entity_id_offsets: Vec::new(),
        self_guid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
        self_guid_offset: 0,
        parent_guid: None,
        parent_guid_offset: None,
        revision: 1,
        revision_offset: 0,
    }];
    native.design_material_assignments = vec![DesignMaterialAssignment {
        id: "generated:material-assignment#0".into(),
        asm_body_key: 42,
        entity_suffix: 985,
        entity_suffix_offset: 0,
        entity_id: "0_985".into(),
        entity_id_offset: 0,
        visual_guid: visual_guid.into(),
        visual_guid_offset: 0,
        physical_token: Some("PrismMaterial-Generated".into()),
        physical_token_offset: None,
        visual_preset: Some("Prism-Generated".into()),
        visual_preset_offset: None,
    }];

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less Protein appearance encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Protein appearance round trip");
    assert_eq!(round_trip.ir.model.appearances.len(), 1);
    let appearance = &round_trip.ir.model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("Prism-Generated"));
    assert_eq!(appearance.visual_guid.as_deref(), Some(visual_guid));
    assert_eq!(appearance.schema.as_deref(), Some("GenericSchema"));
    assert_eq!(appearance.category.as_deref(), Some("Plastic/Generated"));
    assert_eq!(
        appearance.base_color,
        Some(Color {
            r: 0.15,
            g: 0.35,
            b: 0.75,
            a: 1.0,
        })
    );
    assert_eq!(
        appearance.properties.get("reflectivity_at_0deg"),
        Some(&0.25)
    );
    assert_eq!(appearance.properties.get("refraction_index"), Some(&1.5));
    assert_eq!(round_trip.ir.model.appearance_bindings.len(), 1);
    assert!(matches!(
        &round_trip.ir.model.appearance_bindings[0].target,
        AppearanceTarget::Body(body) if body == &round_trip.ir.model.bodies[0].id
    ));
    assert_eq!(
        f3d_native(&round_trip.ir).design_material_assignments[0].asm_body_key,
        42
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_point_coordinates() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let point = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .sketch_points[0];
    point.coordinates.u += 12.5;
    point.coordinates.v -= 7.5;
    let expected = point.coordinates;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("native sketch-point regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip
            .ir
            .native
            .f3d
            .as_ref()
            .expect("F3D native namespace")
            .sketch_points[0]
            .coordinates,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_arc_geometry() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let curve = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .sketch_curve_identities[0];
    let Some(cadmpeg_ir::design::SketchCurveGeometry::Arc {
        center,
        radius,
        start_angle,
        end_angle,
        ..
    }) = &mut curve.geometry
    else {
        panic!("generated sketch curve must be an arc")
    };
    center.x += 20.0;
    *radius = 35.0;
    *start_angle = 0.25;
    *end_angle = 2.75;
    let expected = curve.geometry.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("native sketch-arc regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip
            .ir
            .native
            .f3d
            .as_ref()
            .expect("F3D native namespace")
            .sketch_curve_identities[0]
            .geometry,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_constraint_mask() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let relation = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .sketch_relations[0];
    relation.state = 0x40;
    relation.constraint_kinds = vec![cadmpeg_ir::design::SketchConstraintKind::Horizontal];
    relation.unknown_constraint_bits = 0;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("native sketch-constraint regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let relation = &round_trip
        .ir
        .native
        .f3d
        .as_ref()
        .expect("F3D native namespace")
        .sketch_relations[0];
    assert_eq!(relation.state, 0x40);
    assert_eq!(
        relation.constraint_kinds,
        [cadmpeg_ir::design::SketchConstraintKind::Horizontal]
    );
    assert_eq!(relation.unknown_constraint_bits, 0);
}

#[test]
fn generated_f3d_rewrites_native_sketch_nurbs_values() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let curve = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .sketch_curve_identities[1];
    let Some(cadmpeg_ir::design::SketchCurveGeometry::Nurbs {
        fit_tolerance,
        control_points,
        ..
    }) = &mut curve.geometry
    else {
        panic!("generated sketch curve must be NURBS")
    };
    *fit_tolerance = 0.125;
    control_points[1].x += 15.0;
    control_points[1].y -= 5.0;
    let expected = curve.geometry.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("native sketch-NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip
            .ir
            .native
            .f3d
            .as_ref()
            .expect("F3D native namespace")
            .sketch_curve_identities[1]
            .geometry,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_body_transform() {
    let source = f3d_with_smbh(&synthetic_geometry_with_transform_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let transform = edited.model.bodies[0]
        .transform
        .as_mut()
        .expect("generated body transform");
    transform.rows[0][3] = 125.0;
    transform.rows[1][3] = -75.0;
    transform.rows[2][3] = 50.0;
    transform.rows[3][3] = 2.0;
    let expected = *transform;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("body-transform regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir.model.bodies[0].transform, Some(expected));
}

#[test]
fn generated_f3d_rewrites_design_recipe_and_persistent_reference() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Design decode");
    let mut edited = decoded.ir;
    let reference = edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .persistent_references
        .iter_mut()
        .find(|reference| reference.value == 439)
        .expect("generated persistent reference");
    assert!(reference.byte_offset > 0);
    assert!(reference.value_offset > 0);
    reference.value = 9_001;
    let recipe = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .construction_recipes[0];
    assert!(recipe.byte_offset > 0);
    assert!(recipe.record_index_offset.is_some());
    assert!(recipe.design_id_offset.is_some());
    recipe.record_index = 777;
    recipe.design_id = Some("333".into());
    let member = edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .design_body_members
        .iter_mut()
        .find(|member| member.entity_suffix == 985)
        .expect("generated body member");
    assert!(member.byte_offset > 0);
    member.entity_suffix = 12_345;
    member.flags = 7;
    let header = edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .design_entity_headers
        .iter_mut()
        .find(|header| header.object_kind == Some(cadmpeg_ir::design::DesignObjectKind::Sketch))
        .expect("generated sketch entity header");
    assert!(header.byte_offset > 0);
    assert!(header.record_reference_offset.is_some());
    assert_eq!(header.reference_offsets.len(), 2);
    header.record_reference = Some(585);
    header.reference_indices.swap(0, 1);
    let object = edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .design_objects
        .iter_mut()
        .find(|object| object.kind == cadmpeg_ir::design::DesignObjectKind::Body)
        .expect("generated body design object");
    assert!(object.byte_offset < object.revision_offset);
    assert_eq!(object.entity_id_offsets.len(), 1);
    object.entity_ids[0] = 986;
    object.self_guid = "91111111-2222-3333-4444-555555555555".into();
    object.parent_guid = Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef".into());
    object.revision = 9;
    let act_guid = edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .act_guids
        .iter_mut()
        .find(|guid| guid.guid == "eeeeeeee-1111-2222-3333-ffffffffffff")
        .expect("generated standalone ACT GUID");
    assert!(act_guid.guid_offset > act_guid.byte_offset);
    act_guid.guid = "ffffffff-1111-2222-3333-444444444444".into();
    let act_root = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .act_root_components[0];
    act_root.record_index = 70;
    act_root.instance_root_record = 71;
    act_root.components_root_record = 72;
    act_root.registry_flag = 0;
    act_root.entity_id = "0_4".into();
    act_root.display_name = "(Renamed)".into();
    let act_entity = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .act_entities[0];
    assert!(act_entity.table_entity_id_offset.is_some());
    assert!(act_entity.channel_entity_id_offset.is_some());
    act_entity.channels.insert(
        "Appearance".into(),
        "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
    );
    let binding = &mut edited.model.appearance_bindings[0];
    binding.id = binding.id.replace("0_985", "0_986");
    binding.source_entity_id = Some("0_986".into());
    binding.channels.insert(
        "Appearance".into(),
        "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
    );
    let lost_edge = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .lost_edge_references[0];
    assert!(lost_edge.class_tag_offset > lost_edge.byte_offset);
    lost_edge.class_tag = "420".into();
    lost_edge.record_index = 4_700;
    let assignment = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .design_material_assignments[0];
    assert!(assignment.entity_id_offset > 0);
    assignment.entity_id = "0_986".into();
    assignment.entity_suffix = 986;
    assignment.physical_token = Some("PrismMaterial-019".into());
    assignment.visual_preset = Some("Prism-002".into());
    edited.model.appearances[0].physical_token = Some("PrismMaterial-019".into());
    edited.model.appearances[0].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.8,
        g: 0.6,
        b: 0.4,
        a: 1.0,
    });
    edited.model.appearances[0]
        .properties
        .insert("reflectivity_at_0deg".into(), 0.7);
    edited.model.appearances[0]
        .properties
        .insert("refraction_index".into(), 1.8);
    edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .act_entities[0]
        .entity_id = "0_986".into();
    assert_eq!(
        edited.native.f3d.as_ref().unwrap().act_entities[0].entity_id,
        edited
            .native
            .f3d
            .as_ref()
            .unwrap()
            .design_material_assignments[0]
            .entity_id
    );

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("persistent-reference regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Design decode");
    assert!(f3d_native(&round_trip.ir)
        .persistent_references
        .iter()
        .any(|reference| reference.value == 9_001));
    assert_eq!(
        f3d_native(&round_trip.ir).construction_recipes[0].record_index,
        777
    );
    assert_eq!(
        f3d_native(&round_trip.ir).construction_recipes[0]
            .design_id
            .as_deref(),
        Some("333")
    );
    assert!(f3d_native(&round_trip.ir)
        .design_body_members
        .iter()
        .any(|member| member.entity_suffix == 12_345 && member.flags == 7));
    let header = f3d_native(&round_trip.ir)
        .design_entity_headers
        .iter()
        .find(|header| header.object_kind == Some(cadmpeg_ir::design::DesignObjectKind::Sketch))
        .expect("round-trip sketch entity header");
    assert_eq!(header.record_reference, Some(585));
    assert_eq!(header.reference_indices, [44, 33]);
    let object = f3d_native(&round_trip.ir)
        .design_objects
        .iter()
        .find(|object| object.kind == cadmpeg_ir::design::DesignObjectKind::Body)
        .expect("round-trip body design object");
    assert_eq!(object.entity_ids, [986]);
    assert_eq!(object.self_guid, "91111111-2222-3333-4444-555555555555");
    assert_eq!(
        object.parent_guid.as_deref(),
        Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef")
    );
    assert_eq!(object.revision, 9);
    assert!(f3d_native(&round_trip.ir)
        .act_guids
        .iter()
        .any(|guid| guid.guid == "ffffffff-1111-2222-3333-444444444444"));
    let act_root = &f3d_native(&round_trip.ir).act_root_components[0];
    assert_eq!(act_root.record_index, 70);
    assert_eq!(act_root.instance_root_record, 71);
    assert_eq!(act_root.components_root_record, 72);
    assert_eq!(act_root.registry_flag, 0);
    assert_eq!(act_root.entity_id, "0_4");
    assert_eq!(act_root.display_name, "(Renamed)");
    let act_entity = &f3d_native(&round_trip.ir).act_entities[0];
    assert_eq!(act_entity.entity_id, "0_986");
    assert_eq!(
        act_entity.channels.get("Appearance").map(String::as_str),
        Some("dddddddd-1111-2222-3333-eeeeeeeeeeee")
    );
    let binding = &round_trip.ir.model.appearance_bindings[0];
    assert_eq!(binding.source_entity_id.as_deref(), Some("0_986"));
    assert_eq!(
        binding.channels.get("Appearance").map(String::as_str),
        Some("dddddddd-1111-2222-3333-eeeeeeeeeeee")
    );
    let lost_edge = &f3d_native(&round_trip.ir).lost_edge_references[0];
    assert_eq!(lost_edge.class_tag, "420");
    assert_eq!(lost_edge.record_index, 4_700);
    assert_eq!(
        f3d_native(&round_trip.ir).design_material_assignments[0].entity_id,
        "0_986"
    );
    assert_eq!(
        f3d_native(&round_trip.ir).design_material_assignments[0]
            .visual_preset
            .as_deref(),
        Some("Prism-002")
    );
    assert_eq!(
        round_trip.ir.model.appearances[0].physical_token.as_deref(),
        Some("PrismMaterial-019")
    );
    assert_eq!(
        round_trip.ir.model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.8,
            g: 0.6,
            b: 0.4,
            a: 1.0,
        })
    );
    assert_eq!(
        round_trip.ir.model.appearances[0]
            .properties
            .get("reflectivity_at_0deg"),
        Some(&0.7)
    );
    assert_eq!(
        round_trip.ir.model.appearances[0]
            .properties
            .get("refraction_index"),
        Some(&1.8)
    );
}

#[test]
fn generated_f3d_rejects_act_binding_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let mut edited = decoded.ir;
    edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .act_entities[0]
        .channels
        .insert(
            "Appearance".into(),
            "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
        );

    let error = F3dCodec
        .write_preserved(&edited, &mut Vec::new())
        .expect_err("divergent ACT and appearance binding must fail");
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(_)
    ));
}

#[test]
fn generated_f3d_rejects_material_assignment_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let mut edited = decoded.ir;
    edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .design_material_assignments[0]
        .physical_token = Some("PrismMaterial-019".into());

    let error = F3dCodec
        .write_preserved(&edited, &mut Vec::new())
        .expect_err("divergent assignment and appearance must fail");
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(_)
    ));
}

#[test]
fn generated_f3d_rejects_invalid_or_structural_protein_property_edits() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Protein decode");

    let mut invalid = decoded.ir.clone();
    invalid.model.appearances[0]
        .properties
        .insert("refraction_index".into(), 0.5);
    let error = F3dCodec
        .write_preserved(&invalid, &mut Vec::new())
        .expect_err("out-of-range refraction must be refused");
    assert!(
        matches!(error, cadmpeg_ir::codec::CodecError::Malformed(message) if message.contains("refraction_index"))
    );

    let mut structural = decoded.ir;
    structural.model.appearances[0]
        .properties
        .insert("unserialized_property".into(), 0.5);
    let error = F3dCodec
        .write_preserved(&structural, &mut Vec::new())
        .expect_err("new Protein property must be refused");
    assert!(
        matches!(error, cadmpeg_ir::codec::CodecError::NotImplemented(message) if message.contains("unchanged property set"))
    );
}

#[test]
fn generated_f3d_routes_appearance_edits_across_multiple_protein_assets() {
    let source = f3d_with_smbh_and_protein_guids(
        &synthetic_geometry_smbh(),
        &[
            "11111111-2222-3333-4444-555555555555",
            "99999999-2222-3333-4444-555555555555",
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated multi-Protein decode");
    assert_eq!(decoded.ir.model.appearances.len(), 2);
    let mut edited = decoded.ir;
    edited.model.appearances[0].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.2,
        g: 0.3,
        b: 0.4,
        a: 1.0,
    });
    edited.model.appearances[1].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.6,
        g: 0.7,
        b: 0.8,
        a: 1.0,
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("multi-Protein appearance regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated multi-Protein decode");
    assert_eq!(round_trip.ir.model.appearances, edited.model.appearances);
}

#[test]
fn generated_f3d_rewrites_prism_scalar_properties() {
    let source = f3d_with_smbh_and_instance_properties(
        &synthetic_geometry_smbh(),
        &[
            generated_prism_instance_properties(
                "PrismOpaqueSchema",
                "11111111-2222-3333-4444-555555555555",
            ),
            generated_prism_instance_properties(
                "PrismTransparentSchema",
                "99999999-2222-3333-4444-555555555555",
            ),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Prism decode");
    let mut edited = decoded.ir;
    let opaque = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismOpaqueSchema"))
        .expect("opaque appearance");
    opaque.properties.insert("surface_roughness".into(), 0.75);
    let transparent = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismTransparentSchema"))
        .expect("transparent appearance");
    transparent
        .properties
        .insert("refraction_index".into(), 2.25);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("Prism scalar regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Prism decode");
    assert!(round_trip.ir.model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismOpaqueSchema")
            && appearance.properties.get("surface_roughness") == Some(&0.75)
    }));
    assert!(round_trip.ir.model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismTransparentSchema")
            && appearance.properties.get("refraction_index") == Some(&2.25)
    }));
}

#[test]
fn generated_f3d_rewrites_body_rgb_color() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let expected = cadmpeg_ir::topology::Color {
        r: 0.7,
        g: 0.4,
        b: 0.2,
        a: 1.0,
    };
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("body-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir.model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rewrites_face_rgb_color_and_sense() {
    let source = f3d_with_smbh(&synthetic_geometry_with_face_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let expected = cadmpeg_ir::topology::Color {
        r: 0.6,
        g: 0.3,
        b: 0.9,
        a: 1.0,
    };
    edited.model.faces[0].color = Some(expected);
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("face-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir.model.faces[0].color, Some(expected));
    assert_eq!(
        round_trip.ir.model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
}

#[test]
fn generated_f3d_rewrites_edge_parameter_range() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    edited.model.edges[0].param_range = Some([-2.5, 4.75]);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("edge-range regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir.model.edges[0].param_range, Some([-2.5, 4.75]));
}

#[test]
fn generated_f3d_rewrites_face_and_coedge_sense() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    edited.model.coedges[0].sense = cadmpeg_ir::topology::Sense::Reversed;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("orientation regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip.ir.model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        round_trip.ir.model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
}

fn f3d_with_smbh_and_protein(smbh: &[u8]) -> Vec<u8> {
    f3d_with_smbh_and_protein_guids(smbh, &["11111111-2222-3333-4444-555555555555"])
}

fn f3d_with_smbh_and_protein_guids(smbh: &[u8], guids: &[&str]) -> Vec<u8> {
    let properties = guids
        .iter()
        .map(|guid| generated_instance_properties_for(guid))
        .collect::<Vec<_>>();
    f3d_with_smbh_and_instance_properties(smbh, &properties)
}

fn f3d_with_smbh_and_instance_properties(smbh: &[u8], properties: &[Vec<u8>]) -> Vec<u8> {
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let proteins = properties
        .iter()
        .map(|properties| {
            let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
            nested
                .start_file("AssetData/InstanceProperties.bin", stored)
                .unwrap();
            nested.write_all(properties).unwrap();
            nested
                .start_file("AssetData/DefinitionIteratorProperties.bin", stored)
                .unwrap();
            nested
                .write_all(&generated_definition_catalog_for(
                    generated_schema_from_paged(properties),
                ))
                .unwrap();
            nested.finish().unwrap().into_inner()
        })
        .collect::<Vec<_>>();

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(b"synthetic-manifest").unwrap();
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    for (ordinal, protein) in proteins.iter().enumerate() {
        zip.start_file(
            format!(
                "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"
            ),
            stored,
        )
        .unwrap();
        zip.write_all(protein).unwrap();
    }
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&generated_design_bulkstream()).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&generated_design_metastream()).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/FusionACTSegmentType1/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(&generated_act_bulkstream()).unwrap();
    zip.finish().unwrap().into_inner()
}

fn generated_design_metastream() -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    fn record(
        out: &mut Vec<u8>,
        kind: &str,
        ids: &[u64],
        self_guid: &str,
        parent_guid: &str,
        revision: u32,
    ) {
        lp(out, kind);
        out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
        for id in ids {
            out.extend_from_slice(&id.to_le_bytes());
        }
        lp(out, self_guid);
        lp(out, parent_guid);
        out.extend_from_slice(&revision.to_le_bytes());
    }
    let mut out = Vec::new();
    let parent = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    record(
        &mut out,
        "Body",
        &[985],
        "11111111-2222-3333-4444-555555555555",
        parent,
        3,
    );
    record(
        &mut out,
        "MSketch",
        &[277],
        "22222222-3333-4444-5555-666666666666",
        parent,
        4,
    );
    record(
        &mut out,
        "Dimension",
        &[270, 271],
        "33333333-4444-5555-6666-777777777777",
        parent,
        5,
    );
    out
}

fn generated_act_bulkstream() -> Vec<u8> {
    fn lp_ascii(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units: Vec<u16> = value.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
    let mut out = Vec::new();
    lp_ascii(&mut out, "268");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    lp_ascii(&mut out, "ACTTable");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_utf16(&mut out, "0_985");
    lp_utf16(&mut out, "eeeeeeee-1111-2222-3333-ffffffffffff");
    lp_ascii(&mut out, "267");
    out.extend_from_slice(&9u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 10]);
    out.push(1);
    out.extend_from_slice(&12u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_utf16(&mut out, "0_3");
    out.push(1);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 5]);
    out.push(1);
    out.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut out, "(Unsaved)");
    out.push(0);
    out.push(1);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_ascii(&mut out, "261");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 10]);
    out.extend_from_slice(&2u32.to_le_bytes());
    for (name, guid) in [
        ("Appearance", "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb"),
        ("PhysicalMaterial", "cccccccc-1111-2222-3333-dddddddddddd"),
    ] {
        lp_ascii(&mut out, name);
        lp_utf16(&mut out, guid);
    }
    lp_utf16(&mut out, "0_985");
    out
}

fn generated_design_bulkstream() -> Vec<u8> {
    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units: Vec<u16> = value.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&42u64.to_le_bytes());
    out.extend_from_slice(&985u64.to_le_bytes());
    out.extend_from_slice(&1793u64.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut out, "BREP.synthetic.smbh");
    for value in [
        "0_985",
        "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
        "PrismMaterial-018",
        "Body",
        "11111111-2222-3333-4444-555555555555",
        "BA5EE55E-9982-449B-9D66-9F036540E140",
        "Prism-001",
    ] {
        lp_utf16(&mut out, value);
    }
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"269");
    out.extend_from_slice(&277u64.to_le_bytes());
    out.extend_from_slice(&[0u8; 5]);
    out.push(1);
    out.extend_from_slice(&[0u8; 4]);
    lp_utf16(&mut out, "0_277");
    out.extend_from_slice(&584u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&2u32.to_le_bytes());
    for reference in [33u32, 44] {
        out.push(1);
        out.extend_from_slice(&reference.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
    }
    for (class_tag, record_index, members) in
        [("350", 33u32, [100u32, 200u32]), ("351", 44, [300, 400])]
    {
        let mut relation = vec![0u8; 101];
        relation[0..4].copy_from_slice(&3u32.to_le_bytes());
        relation[4..7].copy_from_slice(class_tag.as_bytes());
        relation[7..11].copy_from_slice(&record_index.to_le_bytes());
        relation[19] = 1;
        relation[20..24].copy_from_slice(&2u32.to_le_bytes());
        relation[24] = 1;
        relation[25..29].copy_from_slice(&members[0].to_le_bytes());
        relation[39] = 1;
        relation[40..44].copy_from_slice(&members[1].to_le_bytes());
        relation[55] = 1;
        relation[56..60].copy_from_slice(&277u32.to_le_bytes());
        relation[66] = 1;
        let state = if record_index == 33 { 0x10u32 } else { 0x04 };
        relation[67..71].copy_from_slice(&state.to_le_bytes());
        relation[74..78].copy_from_slice(&2u32.to_le_bytes());
        relation[78] = 1;
        relation[79..83].copy_from_slice(&members[1].to_le_bytes());
        relation[89] = 1;
        relation[90..94].copy_from_slice(&members[0].to_le_bytes());
        if record_index == 44 {
            relation[55..101].fill(0);
            relation[55] = 1;
            relation[60] = 1;
            relation[61..65].copy_from_slice(&277u32.to_le_bytes());
            relation[71] = 1;
            relation[72..76].copy_from_slice(&0x04u32.to_le_bytes());
            relation[79..83].copy_from_slice(&2u32.to_le_bytes());
            relation[83] = 1;
            relation[84..88].copy_from_slice(&members[1].to_le_bytes());
            relation[94] = 1;
            relation[95..99].copy_from_slice(&members[0].to_le_bytes());
        }
        out.extend_from_slice(&relation);
    }
    for (record_index, persistent_id, coordinates) in [
        (100u32, 500u64, [1.25f64, -2.5f64]),
        (200, 501, [3.0, 4.0]),
        (300, 502, [-1.0, 0.5]),
        (400, 503, [2.0, 1.0]),
    ] {
        let mut point = vec![0u8; 112];
        point[0..4].copy_from_slice(&3u32.to_le_bytes());
        point[4..7].copy_from_slice(b"360");
        point[7..11].copy_from_slice(&record_index.to_le_bytes());
        point[20] = 1;
        point[21..25].copy_from_slice(&1u32.to_le_bytes());
        point[25..29].copy_from_slice(&6u32.to_le_bytes());
        point[29..35].copy_from_slice(b"pt_tag");
        point[35..39].copy_from_slice(&23u32.to_le_bytes());
        point[39..62].copy_from_slice(b"IntrinsicMetaTypeuint64");
        point[62..70].copy_from_slice(&persistent_id.to_le_bytes());
        point[70] = 1;
        point[71..75].copy_from_slice(&(record_index + 1).to_le_bytes());
        point[96..104].copy_from_slice(&coordinates[0].to_le_bytes());
        point[104..112].copy_from_slice(&coordinates[1].to_le_bytes());
        out.extend_from_slice(&point);
    }
    let mut curve = vec![0u8; 229];
    curve[0..4].copy_from_slice(&3u32.to_le_bytes());
    curve[4..7].copy_from_slice(b"361");
    curve[7..11].copy_from_slice(&600u32.to_le_bytes());
    curve[20] = 1;
    curve[21..25].copy_from_slice(&2u32.to_le_bytes());
    curve[25..29].copy_from_slice(&14u32.to_le_bytes());
    curve[29..43].copy_from_slice(b"crv_primary_id");
    curve[43..47].copy_from_slice(&23u32.to_le_bytes());
    curve[47..70].copy_from_slice(b"IntrinsicMetaTypeuint64");
    curve[70..78].copy_from_slice(&440u64.to_le_bytes());
    curve[78..82].copy_from_slice(&16u32.to_le_bytes());
    curve[82..98].copy_from_slice(b"crv_secondary_id");
    curve[98..102].copy_from_slice(&23u32.to_le_bytes());
    curve[102..125].copy_from_slice(b"IntrinsicMetaTypeuint64");
    curve[125..133].copy_from_slice(&0u64.to_le_bytes());
    for (ordinal, value) in [
        1.0f64,
        2.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        3.0,
        0.0,
        std::f64::consts::PI,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 133 + ordinal * 8;
        curve[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&curve);
    let mut alternate_point = vec![0u8; 164];
    alternate_point[0..4].copy_from_slice(&3u32.to_le_bytes());
    alternate_point[4..7].copy_from_slice(b"362");
    alternate_point[7..11].copy_from_slice(&700u32.to_le_bytes());
    alternate_point[20] = 1;
    alternate_point[21..25].copy_from_slice(&2u32.to_le_bytes());
    alternate_point[25..29].copy_from_slice(&13u32.to_le_bytes());
    alternate_point[29..42].copy_from_slice(b"EntityGenesis");
    alternate_point[42..46].copy_from_slice(&23u32.to_le_bytes());
    alternate_point[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_point[69..77].copy_from_slice(&9u64.to_le_bytes());
    alternate_point[77..81].copy_from_slice(&6u32.to_le_bytes());
    alternate_point[81..87].copy_from_slice(b"pt_tag");
    alternate_point[87..91].copy_from_slice(&23u32.to_le_bytes());
    alternate_point[91..114].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_point[114..122].copy_from_slice(&600u64.to_le_bytes());
    alternate_point[122] = 1;
    alternate_point[123..127].copy_from_slice(&701u32.to_le_bytes());
    alternate_point[148..156].copy_from_slice(&(-4.0f64).to_le_bytes());
    alternate_point[156..164].copy_from_slice(&5.0f64.to_le_bytes());
    out.extend_from_slice(&alternate_point);

    let mut alternate_curve = vec![0u8; 443];
    alternate_curve[0..4].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[4..7].copy_from_slice(b"363");
    alternate_curve[7..11].copy_from_slice(&800u32.to_le_bytes());
    alternate_curve[20] = 1;
    alternate_curve[21..25].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[25..29].copy_from_slice(&13u32.to_le_bytes());
    alternate_curve[29..42].copy_from_slice(b"EntityGenesis");
    alternate_curve[42..46].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[69..77].copy_from_slice(&10u64.to_le_bytes());
    alternate_curve[77..81].copy_from_slice(&14u32.to_le_bytes());
    alternate_curve[81..95].copy_from_slice(b"crv_primary_id");
    alternate_curve[95..99].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[99..122].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[122..130].copy_from_slice(&700u64.to_le_bytes());
    alternate_curve[130..134].copy_from_slice(&16u32.to_le_bytes());
    alternate_curve[134..150].copy_from_slice(b"crv_secondary_id");
    alternate_curve[150..154].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[154..177].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[177..185].copy_from_slice(&0u64.to_le_bytes());
    alternate_curve[185..193].copy_from_slice(&42u64.to_le_bytes());
    alternate_curve[193..197].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[197..200].copy_from_slice(b"365");
    alternate_curve[200..204].copy_from_slice(&800u32.to_le_bytes());
    alternate_curve[273] = 1;
    alternate_curve[275..279].copy_from_slice(&2u32.to_le_bytes());
    alternate_curve[279..287].copy_from_slice(&1.0e-9f64.to_le_bytes());
    alternate_curve[287..291].copy_from_slice(&6u32.to_le_bytes());
    alternate_curve[291..295].copy_from_slice(&6u32.to_le_bytes());
    alternate_curve[295..299].copy_from_slice(&8u32.to_le_bytes());
    for (ordinal, knot) in [0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0].into_iter().enumerate() {
        let offset = 299 + ordinal * 8;
        alternate_curve[offset..offset + 8].copy_from_slice(&knot.to_le_bytes());
    }
    alternate_curve[347..351].copy_from_slice(&0u32.to_le_bytes());
    alternate_curve[351..355].copy_from_slice(&0u32.to_le_bytes());
    alternate_curve[355..359].copy_from_slice(&8u32.to_le_bytes());
    alternate_curve[359..363].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[363..367].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[367..371].copy_from_slice(&8u32.to_le_bytes());
    for (ordinal, coordinate) in [0.0f64, 0.0, 0.0, 1.0, 2.0, 0.0, 3.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = 371 + ordinal * 8;
        alternate_curve[offset..offset + 8].copy_from_slice(&coordinate.to_le_bytes());
    }
    out.extend_from_slice(&alternate_curve);
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(b"BodiesRoot");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(b"BodiesRoot");
    out.extend_from_slice(&2u32.to_le_bytes());
    for entity_suffix in [985u64, 8422] {
        out.push(1);
        out.extend_from_slice(&entity_suffix.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.push(0);
    let mut recipe_prefix = vec![0u8; 27];
    recipe_prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
    recipe_prefix[4..7].copy_from_slice(b"322");
    recipe_prefix[11..15].copy_from_slice(&123i32.to_le_bytes());
    out.extend_from_slice(&recipe_prefix);
    out.extend_from_slice(b"body_recipe_data");
    out.extend_from_slice(&(-1i64).to_le_bytes());
    for value in [2i32, 0, -1, 1, -1] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(b"pt_tag");
    out.extend_from_slice(&23u32.to_le_bytes());
    out.extend_from_slice(b"IntrinsicMetaTypeuint64");
    out.extend_from_slice(&439u64.to_le_bytes());
    out.extend_from_slice(b"EDGE_REFERENCE_LOST");
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"419");
    out.extend_from_slice(&4646u32.to_le_bytes());
    out
}

fn generated_instance_properties_for(guid: &str) -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let mut logical = b"\x80\x00\x01\x00".to_vec();
    lp(&mut logical, "GenericSchema");
    lp(&mut logical, guid);
    lp(&mut logical, "Prism-001");
    lp(&mut logical, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    let value_block = logical.len();
    logical.resize(value_block + 209, 0);
    for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
        let offset = value_block + 112 + ordinal * 8;
        logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    logical[value_block + 171..value_block + 175].copy_from_slice(b"\x0c\x00\x00\x00");
    logical[value_block + 175..value_block + 183].copy_from_slice(&0.25f64.to_le_bytes());
    logical[value_block + 197..value_block + 201].copy_from_slice(b"\x0c\x00\x00\x00");
    logical[value_block + 201..value_block + 209].copy_from_slice(&1.5f64.to_le_bytes());

    paged_instance_properties(&logical)
}

fn generated_prism_instance_properties(schema: &str, guid: &str) -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let mut logical = b"\x80\x00\x01\x00".to_vec();
    lp(&mut logical, schema);
    lp(&mut logical, guid);
    lp(&mut logical, "Prism-001");
    lp(&mut logical, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    let position = logical.len();
    match schema {
        "PrismOpaqueSchema" => {
            logical.resize(position + 96, 0);
            for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
                let offset = position + 8 + ordinal * 8;
                logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            logical[position + 64..position + 68].copy_from_slice(b"\x0e\x20\x00\x00");
            logical[position + 68..position + 76].copy_from_slice(&0.25f64.to_le_bytes());
        }
        "PrismTransparentSchema" => {
            logical.resize(position + 177, 0);
            for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
                let offset = position + 121 + ordinal * 8;
                logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            logical[position + 169..position + 177].copy_from_slice(&1.5f64.to_le_bytes());
        }
        _ => panic!("unsupported generated Prism schema"),
    }
    paged_instance_properties(&logical)
}

fn paged_instance_properties(logical: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(0x88u32).to_le_bytes());
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let first = logical.len().min(132);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&logical[..first]);
    bytes.resize(16 + 136, 0);
    let mut rest = &logical[first..];
    while rest.len() > 128 {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"\x80\x00\x00\x00");
        bytes.extend_from_slice(&rest[..128]);
        rest = &rest[128..];
    }
    if !rest.is_empty() {
        bytes.extend_from_slice(&[0xff; 4]);
        bytes.extend_from_slice(&(rest.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(rest);
        let page_end = 16 + (bytes.len() - 16).next_multiple_of(136);
        bytes.resize(page_end, 0);
    }
    bytes
}

fn generated_schema_from_paged(properties: &[u8]) -> &str {
    let length = u32::from_le_bytes(properties[24..28].try_into().unwrap()) as usize;
    std::str::from_utf8(&properties[28..28 + length]).unwrap()
}

fn generated_definition_catalog_for(schema: &str) -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    let mut out = b"\x80\x00\x01\x00".to_vec();
    for value in [schema, "Prism-001", "Default", "Plastic/Thermoplastic"] {
        lp(&mut out, value);
    }
    out
}

fn push_tagged_f64(b: &mut Vec<u8>, v: f64) {
    b.push(0x06);
    b.extend_from_slice(&v.to_le_bytes());
}

/// Push a `tag`-prefixed little-endian i64 (used for `0x04` longs and `0x15`
/// enum values in B-spline block fixtures).
fn push_tagged_i64(b: &mut Vec<u8>, tag: u8, v: i64) {
    b.push(tag);
    b.extend_from_slice(&v.to_le_bytes());
}

/// Assemble a synthetic `.f3d` ZIP with a manifest, a BREP `.smbh`, a `.smb`
/// snapshot, and a few side entries, mirroring the spec's naming families.
fn synthetic_f3d(include_smbh: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let folder = "FusionAssetName[Active]";
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(b"synthetic-manifest").unwrap();

    if include_smbh {
        zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smbh"), deflated)
            .unwrap();
        zip.write_all(&synthetic_smbh()).unwrap();
    }

    // A construction-snapshot .smb (header only, no delta_state).
    let mut smb = synthetic_smbh();
    smb.truncate(60); // header prefix only, no delta_state marker
    zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smb"), stored)
        .unwrap();
    zip.write_all(&smb).unwrap();

    zip.start_file(
        format!("{folder}/FusionDesignSegmentType1/BulkStream.dat"),
        stored,
    )
    .unwrap();
    zip.write_all(b"design-bulk").unwrap();

    zip.start_file(format!("{folder}/Previews/thumbnail.png"), stored)
        .unwrap();
    zip.write_all(b"\x89PNG").unwrap();

    let cursor = zip.finish().unwrap();
    cursor.into_inner()
}

#[test]
fn asm_header_parses_documented_fields() {
    let bytes = synthetic_smbh();
    let h = asm_header::parse(&bytes).expect("magic present");
    assert_eq!(h.width, 8);
    assert_eq!(h.version_word, Some(7));
    assert_eq!(h.format_version, Some(3));
    assert_eq!(h.schema_version, Some(7));
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.product_version.as_deref(), Some("ASM 231.6.3.65535 OSX"));
    assert_eq!(h.save_date.as_deref(), Some("Tue Mar 31 16:16:19 2026"));
    assert_eq!(h.scale, Some(60.0));
    assert_eq!(h.linear, Some(1e-6));
    assert_eq!(h.angular, Some(1e-10));
}

#[test]
fn asm_header_absent_on_non_asm_bytes() {
    assert!(asm_header::parse(b"not an asm stream at all").is_none());
    assert!(!asm_header::has_asm_magic(b"PK\x03\x04"));
}

/// The `BinaryFile4` fixed header ([spec §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#3-asm-binary-header)): 15-byte magic, four little-endian
/// u32 words (release, record count, entity count, flags), then the same
/// tagged string/double sequence as `BinaryFile8`.
fn bf4_header_prefix(flags: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile4");
    b.extend_from_slice(&22700u32.to_le_bytes()); // ASM release word
    b.extend_from_slice(&0u32.to_le_bytes()); // record count (unwritten)
    b.extend_from_slice(&2u32.to_le_bytes()); // entity count
    b.extend_from_slice(&flags.to_le_bytes());
    push_u8_string(&mut b, "Autodesk Neutron");
    push_u8_string(&mut b, "ASM 227.5.0.65535 NT");
    push_u8_string(&mut b, "Mon Aug  8 02:39:24 2022");
    push_tagged_f64(&mut b, 50.0); // scale
    push_tagged_f64(&mut b, 1e-6); // resabs
    push_tagged_f64(&mut b, 1e-10); // resnor
    b
}

/// The minimal `BinaryFile4` active model slice: the planar-face graph of
/// `synthetic_geometry_smbh` with 4-byte integer/ref fields, the ASM-227
/// `lump` head for the body-subdivision record, and one edge resting on an
/// ellipse arc whose stored range is negative.
fn synthetic_geometry_bf4_smbh() -> Vec<u8> {
    // Width-4 writers; the remaining tag writers are width-independent.
    fn t_ref(b: &mut Vec<u8>, v: i32) {
        b.push(0x0c);
        b.extend_from_slice(&v.to_le_bytes());
    }
    fn t_long(b: &mut Vec<u8>, v: i32) {
        b.push(0x04);
        b.extend_from_slice(&v.to_le_bytes());
    }

    // Indices: 0 asmheader, 1 body, 2 lump, 3 shell, 4 face, 5 loop,
    // 6 plane, 7/8/9 coedges, 10/11/12 edges, 13/14/15 vertices,
    // 16/17/18 points, 19 ellipse.
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "227.5.0.65535");
    t_end(&mut r);

    // 1: body  (chunk3 = first_lump)
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, 42); // 1 native ASM body key
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_lump
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: lump  (chunk4 = first_shell, chunk5 = owner_body)
    t_ident(&mut r, "lump");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, 3); // 4 first_shell
    t_ref(&mut r, 1); // 5 owner_body
    t_end(&mut r);

    // 3: shell  (chunk5 = first_face, chunk7 = owner_lump)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, -1); // 4 null
    t_ref(&mut r, 4); // 5 first_face
    t_ref(&mut r, -1); // 6 wire
    t_ref(&mut r, 2); // 7 owner_lump
    t_end(&mut r);

    // 4: face
    t_ident(&mut r, "face");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_face
    t_ref(&mut r, 5); // 4 first_loop
    t_ref(&mut r, 3); // 5 owner_shell
    t_ref(&mut r, -1); // 6 null
    t_ref(&mut r, 6); // 7 surface
    r.push(0x0b); // 8 sense = forward
    r.push(0x0b); // 9 sides = single
    t_end(&mut r);

    // 5: loop
    t_ident(&mut r, "loop");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_loop
    t_ref(&mut r, 7); // 4 first_coedge
    t_ref(&mut r, 4); // 5 owner_face
    t_end(&mut r);

    // 6: plane-surface
    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_pos(&mut r, [0.0, 0.0, 0.0]);
    t_vec(&mut r, [0.0, 0.0, 1.0]);
    t_vec(&mut r, [1.0, 0.0, 0.0]);
    r.push(0x0b);
    t_end(&mut r);

    // 7/8/9: coedges forming the ring 7 -> 8 -> 9 -> 7
    let coedges = [(8i32, 9, 10), (9, 7, 11), (7, 8, 12)];
    for (next, prev, edge) in coedges {
        t_ident(&mut r, "coedge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, next); // 3 next
        t_ref(&mut r, prev); // 4 prev
        t_ref(&mut r, -1); // 5 partner
        t_ref(&mut r, edge); // 6 edge
        r.push(0x0b); // 7 sense = forward
        t_ref(&mut r, 5); // 8 owner_loop
        t_long(&mut r, 0); // 9 reserved
        t_ref(&mut r, -1); // 10 pcurve
        t_end(&mut r);
    }

    // 10/11/12: edges. Edge 10 rests on the ellipse arc (19) with the stored
    // ASM range [-π, -π/2]; edges 11/12 carry no curve.
    let edges = [(13i32, 14, 19), (14, 15, -1), (15, 13, -1)];
    for (start, end, curve) in edges {
        t_ident(&mut r, "edge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, start); // 3 start_vertex
        t_dbl(&mut r, -std::f64::consts::PI); // 4 t_start
        t_ref(&mut r, end); // 5 end_vertex
        t_dbl(&mut r, -std::f64::consts::FRAC_PI_2); // 6 t_end
        t_ref(&mut r, -1); // 7 owner_coedge
        t_ref(&mut r, curve); // 8 curve
        r.push(0x0b); // 9 sense
        push_u8_string(&mut r, "unknown"); // 10 continuity text
        t_end(&mut r);
    }

    // 13/14/15: vertices
    let verts = [(10i32, 16), (11, 17), (12, 18)];
    for (edge, point) in verts {
        t_ident(&mut r, "vertex");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, edge); // 3 owning_edge
        t_long(&mut r, 0); // 4 index_flag
        t_ref(&mut r, point); // 5 point
        t_end(&mut r);
    }

    // 16/17/18: points
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for p in points {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, p);
        t_long(&mut r, 1);
        t_end(&mut r);
    }

    // 19: ellipse-curve (circle: ratio 1) carrying edge 10's arc.
    t_subident(&mut r, "ellipse");
    t_ident(&mut r, "curve");
    t_ref(&mut r, -1); // attrib
    t_long(&mut r, -1); // history
    t_ref(&mut r, -1); // null
    t_pos(&mut r, [0.5, 0.0, 0.0]); // center
    t_vec(&mut r, [0.0, 0.0, 1.0]); // normal
    t_vec(&mut r, [0.5, 0.0, 0.0]); // major axis (radius 0.5 cm)
    t_dbl(&mut r, 1.0); // ratio
    t_end(&mut r);

    // History boundary.
    t_ident(&mut r, "delta_state");

    let mut out = bf4_header_prefix(5);
    out.extend_from_slice(&r);
    out
}

#[test]
fn asm_header_parses_binaryfile4_fields() {
    let bytes = bf4_header_prefix(5);
    assert!(asm_header::has_asm_magic(&bytes));
    let h = asm_header::parse(&bytes).expect("magic present");
    assert_eq!(h.width, 4);
    assert_eq!(h.release, Some(22700));
    assert_eq!(h.record_count, Some(0));
    assert_eq!(h.entity_count, Some(2));
    assert_eq!(h.flags, Some(5));
    assert_eq!(h.version_word, None);
    assert_eq!(h.format_version, None);
    assert_eq!(h.schema_version, None);
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.product_version.as_deref(), Some("ASM 227.5.0.65535 NT"));
    assert_eq!(h.save_date.as_deref(), Some("Mon Aug  8 02:39:24 2022"));
    assert_eq!(h.scale, Some(50.0));
    assert_eq!(h.linear, Some(1e-6));
    assert_eq!(h.angular, Some(1e-10));
    // The record stream begins directly after the tolerance doubles.
    assert_eq!(asm_header::record_stream_start(&bytes), Some(bytes.len()));
}

#[test]
fn decodes_binaryfile4_geometry_with_lump_topology() {
    let f3d = f3d_with_smbh(&synthetic_geometry_bf4_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.bodies.len(), 1);
    // The ASM-227 `lump` head is emitted as the region record.
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 3);
    assert_eq!(result.ir.model.points.len(), 3);

    // The circle arc's stored [-π, -π/2] range is shifted by the ratio-sign
    // phase convention (+π/2) and wrapped into the canonical [0, τ] domain.
    let arc = result
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.is_some())
        .expect("edge on the ellipse carrier");
    let [start, end] = arc.param_range.expect("arc range");
    assert!((start - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert!((end - std::f64::consts::TAU).abs() < 1e-9);
}

#[test]
fn delta_state_boundary_is_located() {
    let bytes = synthetic_smbh();
    let off = asm_header::first_delta_state_offset(&bytes).expect("has a delta_state");
    assert_eq!(&bytes[off..off + 11], b"delta_state");
    // The .smb truncation removes the marker.
    let mut smb = bytes.clone();
    smb.truncate(60);
    assert!(asm_header::first_delta_state_offset(&smb).is_none());
}

#[test]
fn decode_retains_generated_asm_history_graph() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(f3d_native(&result.ir).asm_histories.len(), 1);
    let history = &f3d_native(&result.ir).asm_histories[0];
    assert_eq!(history.stream_size, Some(2));
    assert_eq!(history.high_water_mark, Some(99));
    assert_eq!(history.states.len(), 2);
    assert_eq!(history.states[0].state_id, 2);
    assert_eq!(history.states[0].next_ref, Some(2));
    assert_eq!(history.states[0].bulletin_boards.len(), 1);
    assert_eq!(history.states[0].bulletin_boards[0].changes.len(), 2);
    assert_eq!(history.states[0].records.len(), 1);
    assert_eq!(history.states[0].records[0].name, "history_payload");
    assert!(!history.states[0].records[0].raw_bytes.is_empty());
    assert_eq!(
        history.states[0].bulletin_boards[0].changes[1].kind,
        cadmpeg_ir::history::AsmEntityChangeKind::Insert
    );
    assert_eq!(history.states[1].previous_ref, Some(0));
    assert_eq!(history.states[1].next_ref, None);
    assert!(result.report.geometry_transferred);
}

#[test]
fn generated_f3d_rewrites_fixed_delta_state_header() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated history decode");
    let mut edited = decoded.ir;
    let history = &mut edited
        .native
        .f3d
        .as_mut()
        .expect("F3D native namespace")
        .asm_histories[0];
    assert!(history.byte_offset > 0);
    assert!(history.states[0].byte_offset > 0);
    history.stream_size = Some(8);
    history.high_water_mark = Some(120);
    history.states[0].state_id = 8;
    history.states[0].version_flag = 4;
    history.states[0].state_flag = 6;
    history.states[0].previous_ref = Some(12);
    history.states[0].next_ref = Some(14);
    history.states[0].node_index = 16;
    history.states[0].partner_ref = Some(18);
    history.states[0].owner_ref = 20;
    let board = &mut history.states[0].bulletin_boards[0];
    assert!(board.byte_offset > 0);
    board.owner_ref = 22;
    board.number = 24;
    assert!(board.changes[0].byte_offset > 0);
    board.changes[0].kind = cadmpeg_ir::history::AsmEntityChangeKind::Delete;
    board.changes[0].old_ref = Some(26);
    board.changes[0].new_ref = None;
    board.changes[1].new_ref = Some(28);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("delta-state owner regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated history decode");
    let state = &f3d_native(&round_trip.ir).asm_histories[0].states[0];
    assert_eq!(
        f3d_native(&round_trip.ir).asm_histories[0].stream_size,
        Some(8)
    );
    assert_eq!(
        f3d_native(&round_trip.ir).asm_histories[0].high_water_mark,
        Some(120)
    );
    assert_eq!(state.state_id, 8);
    assert_eq!(state.version_flag, 4);
    assert_eq!(state.state_flag, 6);
    assert_eq!(state.previous_ref, Some(12));
    assert_eq!(state.next_ref, Some(14));
    assert_eq!(state.node_index, 16);
    assert_eq!(state.partner_ref, Some(18));
    assert_eq!(state.owner_ref, 20);
    let board = &state.bulletin_boards[0];
    assert_eq!(board.owner_ref, 22);
    assert_eq!(board.number, 24);
    assert_eq!(
        board.changes[0].kind,
        cadmpeg_ir::history::AsmEntityChangeKind::Delete
    );
    assert_eq!(board.changes[0].old_ref, Some(26));
    assert_eq!(board.changes[0].new_ref, None);
    assert_eq!(board.changes[1].new_ref, Some(28));
}

#[test]
fn classify_matches_spec_families() {
    assert_eq!(classify("a/Breps.BlobParts/x.smbh"), role::BREP_SMBH);
    assert_eq!(classify("a/Breps.BlobParts/x.smb"), role::BREP_SMB);
    assert_eq!(
        classify("a/ProteinAssets.BlobParts/y.protein"),
        role::PROTEIN
    );
    assert_eq!(classify("a/Design1/BulkStream.dat"), role::BULKSTREAM);
    assert_eq!(classify("a/Design1/MetaStream.dat"), role::METASTREAM);
    assert_eq!(classify("Manifest.dat"), role::MANIFEST);
    assert_eq!(classify("a/Previews/thumb.png"), role::PREVIEW);
    assert_eq!(classify("a/x.paramesh"), role::PARAMESH);
    assert_eq!(classify("a/b/"), role::DIRECTORY);
}

use crate::container::classify;

#[test]
fn detect_high_on_f3d_zip_low_on_bare_zip() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    assert_eq!(codec.detect(&f3d), Confidence::High);

    // A ZIP whose visible prefix has no f3d markers.
    let mut bare = zip::ZipWriter::new(Cursor::new(Vec::new()));
    bare.start_file("readme.txt", SimpleFileOptions::default())
        .unwrap();
    bare.write_all(b"hello").unwrap();
    let bare = bare.finish().unwrap().into_inner();
    assert_eq!(codec.detect(&bare), Confidence::Low);

    assert_eq!(codec.detect(b"\x00\x01\x02\x03 not a zip"), Confidence::No);
}

#[test]
fn inspect_enumerates_and_reads_headers() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    let mut cur = Cursor::new(f3d);
    let summary = codec.inspect(&mut cur).unwrap();

    assert_eq!(summary.format, "f3d");
    assert_eq!(summary.container_kind, "zip");

    let smbh = summary
        .entries
        .iter()
        .find(|e| e.role == role::BREP_SMBH)
        .expect("smbh entry present");
    assert_eq!(smbh.compression, "deflate");
    assert_eq!(
        smbh.attributes.get("product_family").map(String::as_str),
        Some("Autodesk Neutron")
    );
    assert_eq!(smbh.attributes.get("scale").map(String::as_str), Some("60"));
    assert!(smbh.attributes.contains_key("delta_state_first_offset"));
    assert!(smbh.attributes.contains_key("sha256"));

    // The active-BREP selection note prefers the .smbh.
    assert!(summary
        .notes
        .iter()
        .any(|n| n.contains("authoritative .smbh")));
}

#[test]
fn decode_yields_metadata_and_honest_report() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    let mut cur = Cursor::new(f3d);
    let result = codec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    // No geometry was produced, and the report says so.
    assert!(!result.report.geometry_transferred);
    assert!(result.ir.model.faces.is_empty());
    assert!(result.report.error_count() >= 1);
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| matches!(l.category, cadmpeg_ir::report::LossCategory::Geometry)));

    // But the active BREP is preserved as an unknown passthrough with a hash,
    // and source metadata was captured.
    assert_eq!(result.ir.unknowns.len(), 2);
    assert!(result
        .ir
        .unknowns
        .iter()
        .all(|record| record.sha256.len() == 64));
    assert!(result
        .ir
        .unknowns
        .iter()
        .any(|record| record.id.0 == "f3d:file:source-image#0"));
    let source = result.ir.source.as_ref().expect("source metadata");
    assert_eq!(source.format, "f3d");
    assert_eq!(
        source.attributes.get("product_family").map(String::as_str),
        Some("Autodesk Neutron")
    );
    // resabs/resnor were carried into tolerances.
    assert_eq!(result.ir.tolerances.linear, 1e-6);
    assert_f3d_native_parity(&result.ir);
    assert!(result
        .ir
        .annotations
        .provenance
        .contains_key(&result.ir.unknowns[0].id.0));
}

#[test]
fn smb_only_is_reported_as_construction_snapshot() {
    // With no .smbh present, only the .smb construction snapshot remains; it must
    // be selected as a fallback but flagged as non-authoritative ([spec §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#3-asm-binary-header)).
    let f3d = synthetic_f3d(false);
    let mut cur = Cursor::new(f3d);
    let scan = container::scan(&mut cur).unwrap();
    let active = container::select_active_brep(&scan).unwrap();
    assert!(!active.is_smbh);
    let summary = container::summarize(&scan);
    assert!(summary
        .notes
        .iter()
        .any(|n| n.contains("construction snapshot")));
}

#[test]
fn smbh_header_string_region_starts_at_byte_47() {
    // Regression: the three product strings begin at byte 47, not 48 — the
    // schema word `7` at offset 40 puts its low byte 0x07 at offset 47, which
    // doubles as the first string's TAG_UTF8_U8 tag. A parser that starts the
    // string walk at 48 reads a length byte as a tag and desyncs the whole
    // header, so record_stream_start lands mid-header and framing fails.
    let prefix = smbh_header_prefix();
    assert_eq!(prefix[47], 0x07, "schema-word low byte / first string tag");
    // The header parses all three strings and both tolerances despite the
    // overlap, and the record stream begins immediately after the last double.
    let h = asm_header::parse(&prefix).expect("magic present");
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.schema_version, Some(7));
    assert_eq!(h.angular, Some(1e-10));
    assert_eq!(
        asm_header::record_stream_start(&prefix),
        Some(prefix.len()),
        "record stream starts right after the header"
    );
}

#[test]
fn sab_framer_indexes_records_from_asmheader() {
    let bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).expect("record stream start");
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap_or(bytes.len());
    let records = crate::sab::frame(&bytes, start, limit, 8).expect("framing succeeds");

    // asmheader occupies index 0; the topology records follow in order.
    assert_eq!(records[0].index, 0);
    assert_eq!(records[0].head, "asmheader");
    assert_eq!(records[1].head, "body");
    assert_eq!(records[4].head, "face");
    assert_eq!(records[4].name, "face");
    assert_eq!(records[6].name, "plane-surface");
    // The face's surface reference (chunk[7]) resolves to the plane at index 6.
    assert_eq!(records[4].ref_at(7), Some(6));
    // The delta_state boundary record is not part of the active slice.
    assert!(records.iter().all(|r| r.head != "delta_state"));
}

#[test]
fn decode_builds_valid_topology_and_geometry() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    let f3d = f3d_with_smbh(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report.geometry_transferred);
    assert!(result
        .report
        .notes
        .iter()
        .all(|note| !note.starts_with("container-level inspection only")));
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 3);
    assert_eq!(result.ir.model.edges.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert_f3d_native_parity(&result.ir);
    assert!(result
        .ir
        .annotations
        .provenance
        .contains_key(&result.ir.model.bodies[0].id.0));

    // The plane decoded with its stored origin and complete parameter frame.
    match &result.ir.model.surfaces[0].geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            assert_eq!(*origin, Point3::new(0.0, 0.0, 0.0));
            assert_eq!(normal.z, 1.0);
            assert_eq!(*u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected plane, got {other:?}"),
    }
    // Point coordinates converted centimetre → millimetre (×10).
    let xs: Vec<f64> = result
        .ir
        .model
        .points
        .iter()
        .map(|p| p.position.x)
        .collect();
    assert!(xs.contains(&10.0));

    // The decoded document is internally valid: refs resolve, the loop ring
    // closes, no bounds violations.
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);

    // Edges carry no analytic curve (their carriers were null), which is legal.
    assert!(result.ir.model.edges.iter().all(|e| e.curve.is_none()));
    // The loop's coedge ring is the three coedges in order.
    assert_eq!(result.ir.model.loops[0].coedges.len(), 3);
}

#[test]
fn decode_transfers_generated_wire_body_topology() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(result.ir.model.shells.len(), 1);
    assert!(result.ir.model.shells[0].faces.is_empty());
    assert_eq!(result.ir.model.shells[0].wire_edges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 2);
    assert_eq!(result.ir.model.points.len(), 2);
    assert_eq!(result.ir.model.curves.len(), 1);
    assert_eq!(
        result.ir.model.shells[0].wire_edges[0],
        result.ir.model.edges[0].id
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_classifies_generated_mixed_face_wire_body_as_general() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    assert_eq!(
        result.ir.model.bodies.len(),
        1,
        "mixed decode report: {:?}",
        result.report
    );
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.shells[0].wire_edges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 4);
    assert_eq!(result.ir.model.curves.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "mixed-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_degenerate_curve_decodes_regenerates_and_writes_source_less() {
    use cadmpeg_ir::{geometry::CurveGeometry, math::Point3};

    let source = f3d_with_smbh(&synthetic_geometry_with_degenerate_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated degenerate curve decode");
    let curve = decoded
        .ir
        .model
        .curves
        .iter()
        .find(|curve| matches!(curve.geometry, CurveGeometry::Degenerate { .. }))
        .expect("degenerate curve carrier");
    assert_eq!(
        curve.geometry,
        CurveGeometry::Degenerate {
            point: Point3::new(0.0, 0.0, 0.0)
        }
    );
    let curve_id = curve.id.clone();

    let mut edited = decoded.ir.clone();
    let edited_curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .expect("editable degenerate curve");
    edited_curve.geometry = CurveGeometry::Degenerate {
        point: Point3::new(2.0, 3.0, 4.0),
    };
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("degenerate curve regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated degenerate curve decode");
    assert!(regenerated.ir.model.curves.iter().any(|curve| {
        curve.geometry
            == CurveGeometry::Degenerate {
                point: Point3::new(2.0, 3.0, 4.0),
            }
    }));

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = CurveGeometry::Degenerate {
        point: Point3::new(0.0, 0.0, 0.0),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less degenerate curve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less degenerate curve round trip");
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == expected));
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "degenerate-curve findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_general_face_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less general body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less general body round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(round_trip.ir.model.faces.len(), 1);
    assert_eq!(round_trip.ir.model.shells[0].wire_edges.len(), 1);
    assert_eq!(round_trip.ir.model.edges.len(), 4);
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "mixed-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_solid_and_wire_bodies_together() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let decoded_wire = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let wire_json = decoded_wire
        .ir
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:combined_wire:");
    let mut wire =
        cadmpeg_ir::document::CadIr::from_json(&wire_json).expect("renamed combined wire IR");
    source_less.model.bodies.append(&mut wire.model.bodies);
    source_less.model.regions.append(&mut wire.model.regions);
    source_less.model.shells.append(&mut wire.model.shells);
    source_less.model.edges.append(&mut wire.model.edges);
    source_less.model.vertices.append(&mut wire.model.vertices);
    source_less.model.points.append(&mut wire.model.points);
    source_less.model.curves.append(&mut wire.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less solid-plus-wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less solid-plus-wire round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 2);
    assert_eq!(
        round_trip
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.kind)
            .collect::<Vec<_>>(),
        [
            cadmpeg_ir::topology::BodyKind::Solid,
            cadmpeg_ir::topology::BodyKind::Wire,
        ]
    );
    assert_eq!(round_trip.ir.model.faces.len(), 6);
    assert_eq!(round_trip.ir.model.shells[1].wire_edges.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "combined-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_wire_body_topology() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected_curve = source_less.model.curves[0].geometry.clone();
    let expected_points = source_less
        .model
        .points
        .iter()
        .map(|point| point.position)
        .collect::<Vec<_>>();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less wire body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less wire body round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(round_trip.ir.model.shells[0].wire_edges.len(), 1);
    assert_eq!(round_trip.ir.model.edges.len(), 1);
    assert_eq!(
        round_trip
            .ir
            .model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>(),
        expected_points
    );
    assert_eq!(round_trip.ir.model.curves[0].geometry, expected_curve);
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_two_independent_wire_bodies() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire IR");
    second.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform {
        rows: [
            [1.0, 0.0, 0.0, 25.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    source_less.model.bodies.append(&mut second.model.bodies);
    source_less.model.regions.append(&mut second.model.regions);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less two-wire-body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-wire-body round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 2);
    assert!(round_trip
        .ir
        .model
        .bodies
        .iter()
        .all(|body| body.kind == cadmpeg_ir::topology::BodyKind::Wire));
    assert_eq!(round_trip.ir.model.regions.len(), 2);
    assert_eq!(round_trip.ir.model.shells.len(), 2);
    assert_eq!(round_trip.ir.model.edges.len(), 2);
    assert_eq!(round_trip.ir.model.curves.len(), 2);
    assert_eq!(
        round_trip.ir.model.bodies[1]
            .transform
            .expect("second wire transform")
            .rows[0][3],
        25.0
    );
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_edge_wire_ring() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_edge_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire edge IR");
    let second_edge = second.model.edges[0].id.clone();
    source_less.model.shells[0].wire_edges.push(second_edge);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less multi-edge wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-edge wire round trip");
    assert_eq!(round_trip.ir.model.shells[0].wire_edges.len(), 2);
    assert_eq!(round_trip.ir.model.edges.len(), 2);
    assert_eq!(round_trip.ir.model.curves.len(), 2);
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_region_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_region_two:");
    let mut second = cadmpeg_ir::document::CadIr::from_json(&second_json)
        .expect("renamed second wire region IR");
    let body_id = source_less.model.bodies[0].id.clone();
    let region_id = second.model.regions[0].id.clone();
    second.model.regions[0].body = body_id;
    source_less.model.bodies[0].regions.push(region_id);
    source_less.model.regions.append(&mut second.model.regions);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less multi-region wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-region wire round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(round_trip.ir.model.bodies[0].regions.len(), 2);
    assert_eq!(round_trip.ir.model.regions.len(), 2);
    assert_eq!(round_trip.ir.model.shells.len(), 2);
    assert!(round_trip
        .ir
        .model
        .regions
        .iter()
        .all(|region| region.body == round_trip.ir.model.bodies[0].id));
    assert_eq!(round_trip.ir.model.edges.len(), 2);
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_shell_wire_region() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_shell_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire shell IR");
    let region_id = source_less.model.regions[0].id.clone();
    let shell_id = second.model.shells[0].id.clone();
    second.model.shells[0].region = region_id;
    source_less.model.regions[0].shells.push(shell_id);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less multi-shell wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-shell wire round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(round_trip.ir.model.regions.len(), 1);
    assert_eq!(round_trip.ir.model.regions[0].shells.len(), 2);
    assert_eq!(round_trip.ir.model.shells.len(), 2);
    assert!(round_trip
        .ir
        .model
        .shells
        .iter()
        .all(|shell| shell.region == round_trip.ir.model.regions[0].id));
    assert_eq!(round_trip.ir.model.edges.len(), 2);
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn analytic_carrier_decode_covers_each_shape() {
    use crate::brep::{decode_curve, decode_surface};
    use crate::sab::{Record, Token};
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    fn rec(head: &str, tokens: Vec<Token>) -> Record {
        Record {
            index: 0,
            name: head.to_string(),
            head: head.to_string(),
            tokens,
            offset: 0,
            len: 0,
        }
    }
    let refn = || Token::Ref(-1);
    let base = || vec![refn(), Token::Long(-1), refn()];

    // cone with sine==0 decodes to a cylinder; |major| (cm) ×10 = radius (mm).
    let mut cyl = base();
    cyl.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]), // axis
        Token::Vector3([2.0, 0.0, 0.0]), // ref × r_major, |.|=2 cm
        Token::Double(1.0),              // ratio
        Token::Double(0.0),              // sine → cylinder
        Token::Double(1.0),              // cosine
        Token::Double(2.0),              // r1 = 2 cm
    ]);
    match decode_surface(&rec("cone", cyl)).unwrap().0 {
        SurfaceGeometry::Cylinder {
            radius,
            axis,
            ref_direction,
            ..
        } => {
            assert_eq!(radius, 20.0);
            assert_eq!(axis.z, 1.0);
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected cylinder, got {other:?}"),
    }

    // cone with nonzero sine keeps the acute half-angle asin(|sine|).
    let mut cone = base();
    cone.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([2.0, 0.0, 0.0]),
        Token::Double(1.0),
        Token::Double(-0.5), // sine (both-negative branch)
        Token::Double(-0.866_025_4),
        Token::Double(2.0),
    ]);
    match decode_surface(&rec("cone", cone)).unwrap().0 {
        SurfaceGeometry::Cone {
            half_angle,
            ref_direction,
            ..
        } => {
            assert!((half_angle - 0.5f64.asin()).abs() < 1e-12);
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected cone, got {other:?}"),
    }

    // sphere: the signed radius identifies a concave carrier and is preserved.
    let mut sph = base();
    sph.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Double(-1.0), // concave
        Token::Vector3([1.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
    ]);
    let (geo, signed) = decode_surface(&rec("sphere", sph)).unwrap();
    assert!(!signed);
    match geo {
        SurfaceGeometry::Sphere {
            radius,
            axis,
            ref_direction,
            ..
        } => {
            assert_eq!(radius, -10.0);
            assert_eq!(axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected sphere, got {other:?}"),
    }

    // torus: major/minor ×10; signed minor radius is preserved.
    let mut tor = base();
    tor.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Double(1.0),  // major
        Token::Double(-2.0), // signed minor radius, with |minor| > major
        Token::Vector3([1.0, 0.0, 0.0]),
    ]);
    let (geo, inside_out) = decode_surface(&rec("torus", tor)).unwrap();
    assert!(!inside_out);
    match geo {
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ref_direction,
            ..
        } => {
            assert_eq!(major_radius, 10.0);
            assert_eq!(minor_radius, -20.0);
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected torus, got {other:?}"),
    }

    // ellipse with ratio 1 → circle; radius = |ref| (cm) ×10.
    let mut circ = base();
    circ.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([3.0, 0.0, 0.0]),
        Token::Double(1.0),
    ]);
    match decode_curve(&rec("ellipse", circ)).unwrap() {
        CurveGeometry::Circle { radius, .. } => assert_eq!(radius, 30.0),
        other => panic!("expected circle, got {other:?}"),
    }

    // ellipse with ratio != 1 → ellipse; minor = major·|ratio|.
    let mut ell = base();
    ell.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([4.0, 0.0, 0.0]),
        Token::Double(0.5),
    ]);
    match decode_curve(&rec("ellipse", ell)).unwrap() {
        CurveGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => {
            assert_eq!(major_radius, 40.0);
            assert_eq!(minor_radius, 20.0);
        }
        other => panic!("expected ellipse, got {other:?}"),
    }

    // straight line: origin ×10, unit direction.
    let mut line = vec![refn(), refn(), refn()];
    line.extend([
        Token::Position([1.0, 0.0, 0.0]),
        Token::Vector3([0.0, 1.0, 0.0]),
    ]);
    match decode_curve(&rec("straight", line)).unwrap() {
        CurveGeometry::Line { origin, direction } => {
            assert_eq!(origin.x, 10.0);
            assert_eq!(direction.y, 1.0);
        }
        other => panic!("expected line, got {other:?}"),
    }
}

#[test]
fn decode_succeeds_when_geometry_present() {
    let f3d = f3d_with_smbh(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.surfaces.len(), 1);
}

#[test]
fn decode_keeps_face_on_unknown_surface() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    // Rename the plane record so the face rests on a carrier this codec does not
    // decode. The face must now be KEPT — topology intact — with an
    // unknown-geometry surface linking to the preserved record bytes.
    let mut smbh = synthetic_geometry_smbh();
    let needle = b"\x0e\x05plane";
    let pos = smbh
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("plane subident present");
    smbh[pos + 2..pos + 7].copy_from_slice(b"splne");

    let f3d = f3d_with_smbh(&smbh);
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    // Topology is transferred: the face, its loop, coedges, and vertices survive.
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.surfaces.len(), 1);

    // The one surface is unknown-geometry and links to a preserved record.
    let SurfaceGeometry::Unknown { record } = &result.ir.model.surfaces[0].geometry else {
        panic!("expected unknown surface geometry");
    };
    let link = record.as_ref().expect("unknown surface links to a record");
    assert!(
        result.ir.unknowns.iter().any(|u| u.id == *link),
        "the linked unknown record is present in the arena"
    );

    // The loss note is a Warning now (topology transferred), not an Error.
    let note = result
        .report
        .losses
        .iter()
        .find(|l| l.message.contains("unknown-geometry surface"))
        .expect("unknown-surface loss note present");
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Warning);

    // The decoded document still validates.
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn nurbs_surface_block_decodes_to_carrier() {
    use crate::nurbs::decode_surface_cache;

    // A degree-1 × degree-1 nubs surface with a 2×2 control grid. Endpoint
    // multiplicities are stored as `degree` (=1); the clamped knot vector adds
    // one at each end, giving [0,0,1,1] in each direction.
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 1); // degree_u
    push_tagged_i64(&mut b, 0x04, 1); // degree_v
    for _ in 0..4 {
        push_tagged_i64(&mut b, 0x15, 0); // periodic/singularity enums = open
    }
    push_tagged_i64(&mut b, 0x04, 2); // n_unique_knots_u
    push_tagged_i64(&mut b, 0x04, 2); // n_unique_knots_v
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    // Control grid stored v-major (v outer, u inner); coordinates in cm.
    let grid = [
        [0.0, 0.0, 0.0], // (u0,v0)
        [1.0, 0.0, 0.0], // (u1,v0)
        [0.0, 1.0, 0.0], // (u0,v1)
        [1.0, 1.0, 0.0], // (u1,v1)
    ];
    for p in grid {
        for c in p {
            push_tagged_f64(&mut b, c);
        }
    }

    let s = decode_surface_cache(&b).expect("surface block decodes");
    assert_eq!((s.u_degree, s.v_degree), (1, 1));
    assert_eq!((s.u_count, s.v_count), (2, 2));
    assert_eq!(s.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(s.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(s.control_points.len(), 4);
    assert!(s.weights.is_none());
    // Transposed to u-major: index u*v_count+v. Pole (u1,v0) sits at index 2,
    // and coordinates are cm→mm scaled (×10).
    assert_eq!(s.control_points[2].x, 10.0);
    assert_eq!(s.control_points[2].y, 0.0);
}

#[test]
fn decode_retains_generated_translational_extrusion_and_fit_contract() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let f3d = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance, Some(0.02));
    let ProceduralSurfaceDefinition::Extrusion {
        direction,
        directrix,
    } = &procedural.definition
    else {
        panic!("expected extrusion")
    };
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    let directrix = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .expect("extrusion directrix carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(directrix) = &directrix.geometry else {
        panic!("expected NURBS directrix")
    };
    assert_eq!(directrix.control_points.len(), 3);
}

#[test]
fn generated_f3d_rewrites_translational_extrusion_direction() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Extrusion { direction, .. } =
        &mut edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected extrusion")
    };
    *direction = cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("extrusion-direction regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let ProceduralSurfaceDefinition::Extrusion { direction, .. } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip extrusion")
    };
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0));
}

#[test]
fn generated_f3d_rewrites_procedural_surface_fit_tolerance() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated procedural-surface decode");
    let mut edited = decoded.ir;
    edited.model.procedural_surfaces[0].cache_fit_tolerance = Some(0.075);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("procedural-surface fit regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated procedural-surface decode");
    assert_eq!(
        round_trip.ir.model.procedural_surfaces[0].cache_fit_tolerance,
        Some(0.075)
    );
}

#[test]
fn generated_f3d_rewrites_nurbs_surface_control_grid() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated NURBS surface decode");
    let mut edited = decoded.ir;
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(_)
            )
        })
        .expect("generated NURBS surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &mut surface.geometry else {
        unreachable!()
    };
    nurbs.control_points[2].x = 17.5;
    nurbs.control_points[2].z = -3.25;
    nurbs.u_degree = 2;
    nurbs.v_degree = 2;
    nurbs.u_knots = vec![-1.0, -1.0, -1.0, 2.0, 2.0];
    nurbs.v_knots = vec![-0.5, -0.5, -0.5, 1.5, 1.5];
    nurbs.u_periodic = true;
    let expected = nurbs.clone();
    let surface_id = surface.id.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("NURBS surface regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated NURBS surface decode");
    let surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .expect("round-trip NURBS surface");
    assert_eq!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(expected)
    );
}

#[test]
fn generated_f3d_rewrites_rational_nurbs_surface_weights() {
    let source = f3d_with_smbh(&synthetic_rational_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rational surface decode");
    let mut edited = decoded.ir;
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                &surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs)
                    if nurbs.weights.is_some()
            )
        })
        .expect("generated rational surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &mut surface.geometry else {
        unreachable!()
    };
    nurbs.weights.as_mut().expect("rational weights")[1] = 0.65;
    let expected = nurbs.clone();
    let surface_id = surface.id.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("rational-weight regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rational surface decode");
    let surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .expect("round-trip rational surface");
    assert_eq!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(expected)
    );
}

#[test]
fn generated_f3d_rewrites_extrusion_directrix_control_points() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Extrusion { directrix, .. } =
        &edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected extrusion")
    };
    let directrix_id = directrix.clone();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == directrix_id)
        .expect("extrusion directrix");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("expected NURBS directrix")
    };
    nurbs.control_points[1].y = 12.5;
    nurbs.control_points[1].z = -2.0;
    nurbs.degree = 1;
    nurbs.knots = vec![-2.0, -2.0, 3.0, 3.0, 3.0];
    nurbs.periodic = true;
    let expected = nurbs.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("extrusion-directrix regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let curve = round_trip
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == directrix_id)
        .expect("round-trip directrix");
    assert_eq!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(expected)
    );
}

#[test]
fn decode_resolves_generated_ref_translational_extrusion() {
    let f3d = f3d_with_smbh(&synthetic_ref_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir.model.procedural_surfaces[0].cache_fit_tolerance,
        Some(0.02)
    );
}

#[test]
fn decode_retains_generated_rolling_ball_definition() {
    use cadmpeg_ir::geometry::{BlendCrossSection, BlendRadiusLaw, ProceduralSurfaceDefinition};

    let f3d = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance, Some(0.01));
    let ProceduralSurfaceDefinition::Blend {
        supports,
        spine,
        radius,
        cross_section,
    } = &procedural.definition
    else {
        panic!("expected rolling-ball blend")
    };
    assert!(supports.iter().all(Option::is_some));
    assert!(supports.iter().flatten().all(|support| result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id == support.surface)));
    let spine = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == spine.as_ref())
        .expect("blend spine carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(spine) = &spine.geometry else {
        panic!("expected NURBS blend spine")
    };
    assert_eq!(spine.control_points.len(), 3);
    assert_eq!(cross_section, &BlendCrossSection::Circular);
    assert_eq!(
        radius,
        &BlendRadiusLaw::Constant {
            signed_radius: -3.0
        }
    );
}

#[test]
fn generated_f3d_rewrites_rolling_ball_radius_law() {
    use cadmpeg_ir::geometry::{BlendRadiusLaw, ProceduralSurfaceDefinition};

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &mut edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected rolling-ball blend")
    };
    *radius = BlendRadiusLaw::Linear {
        start: -2.0,
        end: -4.0,
    };

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("rolling-ball radius regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip rolling-ball blend")
    };
    assert_eq!(
        radius,
        &BlendRadiusLaw::Linear {
            start: -2.0,
            end: -4.0,
        }
    );
}

#[test]
fn generated_f3d_rewrites_rolling_ball_spine_cache() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Blend {
        spine: Some(spine), ..
    } = &edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected rolling-ball spine")
    };
    let spine_id = spine.clone();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine_id)
        .expect("blend spine curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("expected NURBS blend spine")
    };
    nurbs.control_points[1].x = 8.0;
    nurbs.control_points[1].y = -6.0;
    nurbs.degree = 1;
    nurbs.knots = vec![-1.0, -1.0, 2.0, 2.0, 2.0];
    let expected = curve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("blend-spine regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve == &expected));
}

#[test]
fn generated_f3d_rewrites_rolling_ball_support_cache() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected rolling-ball blend")
    };
    let support_id = supports[0]
        .as_ref()
        .expect("first blend support")
        .surface
        .clone();
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == support_id)
        .expect("blend support surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &mut surface.geometry else {
        panic!("expected NURBS blend support")
    };
    nurbs.control_points[1].x = 6.0;
    nurbs.control_points[1].z = 4.0;
    nurbs.u_degree = 2;
    nurbs.u_knots = vec![-1.0, -1.0, -1.0, 2.0, 2.0];
    let expected = surface.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("blend-support regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    assert!(round_trip
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface == &expected));
}

#[test]
fn decode_reports_generated_partial_rolling_ball_supports() {
    let f3d = f3d_with_smbh(&synthetic_partial_rb_blend_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("only one of two native supports resolved")));
}

#[test]
fn subtype_reference_resolves_surface_cache() {
    use crate::nurbs::decode_surface_cache_resolving_refs;

    let mut target = Vec::new();
    target.extend_from_slice(b"\x0f\x0d\x07surface");
    // A payload byte equal to SUBTYPE_CLOSE must not terminate the span.
    target.push(0x06);
    target.extend_from_slice(&[0x10, 0, 0, 0, 0, 0, 0, 0]);
    target.extend_from_slice(&generated_surface_block());
    target.push(0x10);

    let mut source = Vec::new();
    source.extend_from_slice(b"\x0f\x0d\x03ref\x04");
    source.extend_from_slice(&0i64.to_le_bytes());
    source.push(0x10);

    let mut active = target;
    active.extend_from_slice(&source);
    let decoded = decode_surface_cache_resolving_refs(&source, &active)
        .expect("subtype-table reference resolves to its surface cache");
    assert_eq!((decoded.u_count, decoded.v_count), (2, 2));
}

#[test]
fn rgb_attribute_chain_decodes_body_color() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "body");
    t_ref(&mut bytes, 1); // attrib-chain head
    t_end(&mut bytes);
    t_subident(&mut bytes, "rgb_color");
    t_subident(&mut bytes, "st");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1); // next attrib
    t_dbl(&mut bytes, 0.1);
    t_dbl(&mut bytes, 0.2);
    t_dbl(&mut bytes, 0.3);
    t_end(&mut bytes);

    let records = crate::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color = crate::brep::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!((color.r, color.g, color.b, color.a), (0.1, 0.2, 0.3, 1.0));
}

#[test]
fn truecolor_attribute_chain_decodes_argb() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "truecolor");
    t_subident(&mut bytes, "adesk");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    bytes.push(0x17);
    bytes.extend_from_slice(&(0x8040_80c0i64).to_le_bytes());
    t_end(&mut bytes);

    let records = crate::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color = crate::brep::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r, color.g, color.b, color.a),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 128.0 / 255.0)
    );
}

#[test]
fn transform_decodes_column_major_basis_and_scaled_translation() {
    use crate::sab::{Record, Token};

    let record = Record {
        index: 0,
        name: "transform".into(),
        head: "transform".into(),
        tokens: vec![
            Token::Vector3([1.0, 0.0, 0.0]),
            Token::Vector3([0.0, 1.0, 0.0]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Position([1.0, 2.0, 3.0]),
            Token::Double(1.0),
        ],
        offset: 0,
        len: 0,
    };
    let transform = crate::brep::decode_transform(&record, 60.0).unwrap();
    assert_eq!(transform.rows[0], [1.0, 0.0, 0.0, 600.0]);
    assert_eq!(transform.rows[1], [0.0, 1.0, 0.0, 1200.0]);
    assert_eq!(transform.rows[2], [0.0, 0.0, 1.0, 1800.0]);
    assert_eq!(transform.rows[3], [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn nurbs_curve_block_decodes_to_carrier() {
    use crate::nurbs::decode_curve_cache;

    // A degree-2 nubs curve with two unique knots at stored multiplicity 2:
    // sum(mults) 4, n_poles = 4 - (degree - 1) = 3.
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 2); // degree
    push_tagged_i64(&mut b, 0x15, 0); // closure = open
    push_tagged_i64(&mut b, 0x04, 2); // n_unique_knots
    for (k, m) in [(0.0, 2i64), (1.0, 2)] {
        push_tagged_f64(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for p in [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]] {
        for c in p {
            push_tagged_f64(&mut b, c);
        }
    }

    let c = decode_curve_cache(&b).expect("curve block decodes");
    assert_eq!(c.degree, 2);
    assert_eq!(c.control_points.len(), 3);
    // Clamped knots: [0,0,0,1,1,1] (endpoint mult 2 + 1 = 3 each).
    assert_eq!(c.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(c.control_points[1].x, 10.0);
    assert_eq!(c.control_points[1].y, 20.0);
    assert!(c.weights.is_none());
}

#[test]
fn decode_retains_generated_procedural_curve_fit_contract() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir.model.procedural_curves.first().unwrap();
    assert!(matches!(
        procedural.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown { .. }
    ));
    assert_eq!(procedural.cache_fit_tolerance, Some(0.005));
    assert_eq!(result.ir.model.curves.len(), 1);
}

#[test]
fn decode_retains_generated_helix_construction() {
    use cadmpeg_ir::{geometry::ProceduralCurveDefinition, math::Point3};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated helix decode");
    let procedural = result
        .ir
        .model
        .procedural_curves
        .first()
        .expect("helix construction");
    let ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = procedural.definition
    else {
        panic!("expected helix construction")
    };
    assert_eq!(angle_range, [0.0, std::f64::consts::TAU]);
    assert_eq!(center, Point3::new(10.0, 20.0, 30.0));
    assert_eq!(major, cadmpeg_ir::math::Vector3::new(20.0, 0.0, 0.0));
    assert_eq!(minor, cadmpeg_ir::math::Vector3::new(0.0, 20.0, 0.0));
    assert_eq!(pitch, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 40.0));
    assert_eq!(apex_factor, 0.25);
    assert_eq!(axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(procedural.cache_fit_tolerance, Some(0.005));

    let mut edited = result.ir.clone();
    edited.model.procedural_curves[0].definition = ProceduralCurveDefinition::Helix {
        angle_range: [-1.0, 7.0],
        center: Point3::new(12.0, 23.0, 34.0),
        major: cadmpeg_ir::math::Vector3::new(30.0, 0.0, 0.0),
        minor: cadmpeg_ir::math::Vector3::new(0.0, -30.0, 0.0),
        pitch: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 55.0),
        apex_factor: 0.5,
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
    };
    edited.model.procedural_curves[0].cache_fit_tolerance = Some(0.012);
    let solved_curve_id = edited.model.procedural_curves[0].curve.clone();
    let solved_curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == solved_curve_id)
        .expect("helix solved curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(solved_cache) = &mut solved_curve.geometry
    else {
        panic!("expected helix NURBS cache")
    };
    solved_cache.control_points[1].x = 17.0;
    solved_cache.control_points[1].z = -2.0;
    let edited_definition = edited.model.procedural_curves[0].definition.clone();
    let edited_cache = solved_curve.geometry.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("helix definition regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated helix decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        edited_definition
    );
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.012)
    );
    assert!(regenerated
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == edited_cache));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let expected = source_less.model.procedural_curves[0].definition.clone();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less helix encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less helix round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        expected
    );
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.005)
    );
}

#[test]
fn generated_vector_offset_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_vector_offset_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated vector-offset decode");
    let procedural = &result.ir.model.procedural_curves[0];
    let ProceduralCurveDefinition::VectorOffset {
        source,
        parameter_range,
        offset,
        labels,
        codes,
    } = &procedural.definition
    else {
        panic!("expected vector offset construction")
    };
    assert_eq!(*parameter_range, [-2.0, 5.0]);
    assert_eq!(*offset, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 20.0));
    assert_eq!(labels, &["source".to_string(), "offset".to_string()]);
    assert_eq!(*codes, [7, 9]);
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(procedural.cache_fit_tolerance, Some(0.008));
    let expected_range = *parameter_range;
    let expected_offset = *offset;
    let expected_labels = labels.clone();
    let expected_codes = *codes;

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::VectorOffset {
        parameter_range,
        offset,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        panic!("expected editable vector offset")
    };
    *parameter_range = [-3.0, 6.0];
    *offset = cadmpeg_ir::math::Vector3::new(8.0, -12.0, 25.0);
    edited.model.procedural_curves[0].cache_fit_tolerance = Some(0.015);
    let edited_definition = edited.model.procedural_curves[0].definition.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("vector-offset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated vector-offset decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        edited_definition
    );
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.015)
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less vector-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less vector-offset round trip");
    let ProceduralCurveDefinition::VectorOffset {
        source,
        parameter_range,
        offset,
        labels,
        codes,
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip vector offset")
    };
    assert_eq!(*parameter_range, expected_range);
    assert_eq!(*offset, expected_offset);
    assert_eq!(*labels, expected_labels);
    assert_eq!(*codes, expected_codes);
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.008)
    );
}

#[test]
fn generated_subset_curve_decodes_edits_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_subset_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated subset decode");
    let ProceduralCurveDefinition::Subset {
        source,
        parameter_range,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected subset construction")
    };
    assert_eq!(*parameter_range, [-1.5, 3.5]);
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert!(
        (result.ir.model.procedural_curves[0]
            .cache_fit_tolerance
            .expect("subset fit tolerance")
            - 0.006)
            .abs()
            < 1e-12
    );

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Subset {
        parameter_range, ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    *parameter_range = [-2.0, 4.0];
    let expected_edit = edited.model.procedural_curves[0].definition.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("subset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated subset decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        expected_edit
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less subset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less subset round trip");
    let ProceduralCurveDefinition::Subset {
        source,
        parameter_range,
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip subset")
    };
    assert_eq!(*parameter_range, [-1.5, 3.5]);
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
}

#[test]
fn generated_f3d_rewrites_procedural_curve_fit_tolerance() {
    let source = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated procedural-curve decode");
    let mut edited = decoded.ir;
    edited.model.procedural_curves[0].cache_fit_tolerance = Some(0.025);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("procedural-curve fit regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated procedural-curve decode");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.025)
    );
}

#[test]
fn generated_source_less_refuses_lossy_procedural_curve_fallbacks() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_procedural_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated procedural curve decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    source_less.model.procedural_curves[0].definition = ProceduralCurveDefinition::Intersection {
        supports: [None, None],
    };
    let mut encoded = Vec::new();
    let error = F3dCodec
        .encode(&source_less, &mut encoded)
        .expect_err("typed intersection must not degrade to a cache-only curve");
    assert!(error
        .to_string()
        .contains("does not support procedural curve definition"));
}

#[test]
fn generated_source_less_rejects_duplicate_procedural_curve_owners() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated helix decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.unknowns.clear();
    let mut duplicate = source_less.model.procedural_curves[0].clone();
    duplicate.id = "generated:duplicate-helix".into();
    source_less.model.procedural_curves.push(duplicate);
    let mut encoded = Vec::new();
    let error = F3dCodec
        .encode(&source_less, &mut encoded)
        .expect_err("duplicate procedural construction must be rejected");
    assert!(error
        .to_string()
        .contains("multiple procedural constructions"));
}

#[test]
fn generated_f3d_rewrites_topology_bound_nurbs_curve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated intcurve decode");
    let mut edited = decoded.ir;
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id.as_str() == "f3d:brep:entity#19")
        .expect("topology-bound intcurve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("expected NURBS edge carrier")
    };
    nurbs.control_points[1].x = 14.0;
    nurbs.control_points[1].z = -3.0;
    nurbs.degree = 1;
    nurbs.knots = vec![-1.0, -1.0, 2.0, 2.0, 2.0];
    let expected = curve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("topology-bound NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated intcurve decode");
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve == &expected));
}

#[test]
fn nurbs_pcurve_block_decodes_without_length_scaling() {
    use crate::nurbs::decode_pcurve_cache;

    // A degree-1 2D pcurve. Unlike model-space NURBS control points, these
    // are UV parameters and therefore must not be converted from cm to mm.
    let b = generated_pcurve_block();

    let pcurve = decode_pcurve_cache(&b).expect("2D pcurve block decodes");
    assert_eq!(pcurve.degree, 1);
    assert_eq!(pcurve.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(pcurve.control_points[0].u, 0.25);
    assert_eq!(pcurve.control_points[1].v, 1.5);
}

#[test]
fn ref_pcurve_uses_second_intcurve_cache() {
    let mut intcurve = generated_curve_block();
    intcurve.extend_from_slice(&generated_pcurve_block());

    let pcurve = crate::nurbs::decode_intcurve_pcurve_cache(&intcurve)
        .expect("second cache is the UV pcurve");
    assert_eq!(pcurve.control_points[0].u, 0.25);
    assert_eq!(pcurve.control_points[1].v, 1.5);
}

#[test]
fn ref_pcurve_resolves_intcurve_subtype_cache() {
    let mut target = b"\x0f\x0d\x0bint_int_cur".to_vec();
    target.extend_from_slice(&generated_curve_block());
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    let mut source = b"\x0f\x0d\x03ref\x04".to_vec();
    source.extend_from_slice(&0i64.to_le_bytes());
    source.push(0x10);
    let mut active = target;
    active.extend_from_slice(&source);

    let pcurve = crate::nurbs::decode_intcurve_pcurve_cache_resolving_refs(&source, &active)
        .expect("intcurve subtype carries UV cache");
    assert_eq!(pcurve.control_points[1].v, 1.5);
}

#[test]
fn decode_attaches_generated_pcurve_to_its_coedge() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir
            .model
            .coedges
            .iter()
            .filter(|c| c.pcurve.is_some())
            .count(),
        1
    );
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn generated_f3d_rewrites_nurbs_pcurve_control_points() {
    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated pcurve decode");
    let mut edited = decoded.ir;
    let pcurve = &mut edited.model.pcurves[0];
    assert_eq!(pcurve.wrapper_reversed, Some(false));
    assert_eq!(pcurve.parameter_range, Some([-1.0, 2.0]));
    assert_eq!(pcurve.fit_tolerance, Some(0.001));
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        periodic,
        ..
    } = &mut pcurve.geometry
    else {
        panic!("expected NURBS pcurve")
    };
    control_points[0].u = -0.5;
    control_points[1].v = 2.25;
    *degree = 2;
    *knots = vec![-1.0, -1.0, -1.0, 2.0, 2.0];
    *periodic = true;
    pcurve.wrapper_reversed = Some(true);
    pcurve.parameter_range = Some([-2.0, 3.0]);
    pcurve.fit_tolerance = Some(0.0025);
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected]);
}

#[test]
fn generated_f3d_rewrites_rational_pcurve_weights() {
    let source = f3d_with_smbh(&synthetic_geometry_with_rational_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rational pcurve decode");
    let mut edited = decoded.ir;
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        control_points,
        weights: Some(weights),
        ..
    } = &mut edited.model.pcurves[0].geometry
    else {
        panic!("expected rational pcurve")
    };
    control_points[0].u = -0.25;
    weights[1] = 0.75;
    let expected = edited.model.pcurves[0].clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("rational pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rational pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected]);
}

#[test]
fn generated_f3d_rewrites_ref_form_pcurve_geometry_and_range() {
    let source = f3d_with_smbh(&synthetic_geometry_with_ref_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ref-form pcurve decode");
    let mut edited = decoded.ir;
    let pcurve = &mut edited.model.pcurves[0];
    assert_eq!(pcurve.wrapper_reversed, None);
    assert_eq!(pcurve.fit_tolerance, None);
    assert_eq!(pcurve.parameter_range, Some([-2.0, 4.0]));
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        control_points,
        knots,
        ..
    } = &mut pcurve.geometry
    else {
        panic!("expected ref-form NURBS pcurve")
    };
    control_points[0].u = -0.75;
    control_points[1].v = 3.5;
    *knots = vec![-1.0, -1.0, 2.0, 2.0];
    pcurve.parameter_range = Some([-3.0, 5.0]);
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved(&edited, &mut regenerated)
        .expect("ref-form pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated ref-form pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected]);
}

#[test]
fn decode_transfers_generated_protein_appearance() {
    let f3d = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.appearances.len(), 1);
    let appearance = &result.ir.model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("Prism-001"));
    assert_eq!(
        appearance.visual_guid.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    let color = appearance.base_color.expect("decoded diffuse color");
    assert_eq!((color.r, color.g, color.b), (0.1, 0.2, 0.3));
    assert_eq!(
        appearance.physical_token.as_deref(),
        Some("PrismMaterial-018")
    );
    assert_eq!(appearance.schema.as_deref(), Some("GenericSchema"));
    assert_eq!(
        appearance.category.as_deref(),
        Some("Plastic/Thermoplastic")
    );
    assert_eq!(result.ir.model.appearance_bindings.len(), 1);
    assert_eq!(f3d_native(&result.ir).act_entities.len(), 1);
    assert_eq!(f3d_native(&result.ir).act_entities[0].record_index, 7);
    assert_eq!(f3d_native(&result.ir).act_entities[0].entity_id, "0_985");
    assert!(f3d_native(&result.ir)
        .act_guids
        .iter()
        .any(|record| record.guid == "eeeeeeee-1111-2222-3333-ffffffffffff"));
    assert!(f3d_native(&result.ir).act_entities[0].in_table);
    assert_eq!(f3d_native(&result.ir).act_root_components.len(), 1);
    assert_eq!(
        f3d_native(&result.ir).act_root_components[0].entity_id,
        "0_3"
    );
    assert_eq!(
        f3d_native(&result.ir).act_root_components[0].display_name,
        "(Unsaved)"
    );
    assert_eq!(
        f3d_native(&result.ir).act_root_components[0].instance_root_record,
        12
    );
    assert_eq!(
        f3d_native(&result.ir).act_root_components[0].components_root_record,
        7
    );
    assert_eq!(
        f3d_native(&result.ir).act_root_components[0].registry_flag,
        1
    );
    assert_eq!(
        f3d_native(&result.ir).act_entities[0]
            .channel_class_tag
            .as_deref(),
        Some("261")
    );
    assert_eq!(
        result.ir.model.appearance_bindings[0].appearance,
        appearance.id
    );
    assert!(matches!(
        &result.ir.model.appearance_bindings[0].target,
        cadmpeg_ir::appearance::AppearanceTarget::Body(body) if body == &result.ir.model.bodies[0].id
    ));
    assert_eq!(
        result.ir.model.appearance_bindings[0]
            .channels
            .get("Appearance")
            .map(String::as_str),
        Some("aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb")
    );
    assert_eq!(
        result.ir.model.appearance_bindings[0]
            .source_entity_id
            .as_deref(),
        Some("0_985")
    );
    assert_eq!(
        result.ir.model.appearance_bindings[0]
            .object_type
            .as_deref(),
        Some("Body")
    );
    assert_eq!(f3d_native(&result.ir).construction_recipes.len(), 1);
    assert_eq!(
        f3d_native(&result.ir).construction_recipes[0].kind,
        cadmpeg_ir::design::ConstructionRecipeKind::Body
    );
    assert_eq!(
        f3d_native(&result.ir).construction_recipes[0]
            .design_id
            .as_deref(),
        Some("322")
    );
    assert_eq!(
        f3d_native(&result.ir).construction_recipes[0].record_index,
        123
    );
    assert_eq!(f3d_native(&result.ir).persistent_references.len(), 10);
    assert!(f3d_native(&result.ir)
        .persistent_references
        .iter()
        .any(|reference| reference.value == 439));
    assert!(f3d_native(&result.ir)
        .persistent_references
        .iter()
        .any(|reference| {
            reference.value == 440
                && reference.kind == cadmpeg_ir::design::PersistentReferenceKind::CurvePrimary
        }));
    assert_eq!(f3d_native(&result.ir).lost_edge_references.len(), 1);
    assert_eq!(
        f3d_native(&result.ir).lost_edge_references[0].class_tag,
        "419"
    );
    assert_eq!(
        f3d_native(&result.ir).lost_edge_references[0].record_index,
        4646
    );
    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("source parametric edge reference(s) were marked")));
    assert_eq!(f3d_native(&result.ir).design_objects.len(), 3);
    let sketch = f3d_native(&result.ir)
        .design_objects
        .iter()
        .find(|object| object.kind == cadmpeg_ir::design::DesignObjectKind::Sketch)
        .unwrap();
    assert_eq!(sketch.entity_ids, vec![277]);
    assert_eq!(sketch.revision, 4);
    assert_eq!(f3d_native(&result.ir).design_entity_headers.len(), 1);
    assert_eq!(
        f3d_native(&result.ir).design_entity_headers[0].entity_id,
        "0_277"
    );
    assert_eq!(
        f3d_native(&result.ir).design_entity_headers[0].class_tag,
        "269"
    );
    assert!(f3d_native(&result.ir).design_entity_headers[0].optional_slot_present);
    assert_eq!(
        f3d_native(&result.ir).design_entity_headers[0].object_kind,
        Some(cadmpeg_ir::design::DesignObjectKind::Sketch)
    );
    assert_eq!(
        f3d_native(&result.ir).design_entity_headers[0].record_reference,
        Some(584)
    );
    assert_eq!(
        f3d_native(&result.ir).design_entity_headers[0].declared_reference_count,
        Some(2)
    );
    assert_eq!(
        f3d_native(&result.ir).design_entity_headers[0].reference_indices,
        [33, 44]
    );
    assert_eq!(f3d_native(&result.ir).design_record_headers.len(), 6);
    let record_33 = f3d_native(&result.ir)
        .design_record_headers
        .iter()
        .find(|record| record.record_index == 33)
        .expect("record 33");
    assert_eq!(record_33.class_tag, "350");
    assert_eq!(f3d_native(&result.ir).sketch_relations.len(), 2);
    assert_eq!(
        f3d_native(&result.ir).sketch_relations[0].members,
        [100, 200]
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_relations[0].return_members,
        [200, 100]
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_relations[0].owner_reference,
        277
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_relations[0].constraint_kinds,
        [cadmpeg_ir::design::SketchConstraintKind::Parallel]
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_relations[0].unknown_constraint_bits,
        0
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_relations[1].auxiliary_references,
        [0]
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_relations[0].raw_bytes.len(),
        101
    );
    assert_eq!(f3d_native(&result.ir).sketch_points.len(), 5);
    let point_500 = f3d_native(&result.ir)
        .sketch_points
        .iter()
        .find(|point| point.persistent_id == 500)
        .expect("point 500");
    assert_eq!(point_500.coordinates.u, 12.5);
    assert_eq!(point_500.coordinates.v, -25.0);
    let point_600 = f3d_native(&result.ir)
        .sketch_points
        .iter()
        .find(|point| point.persistent_id == 600)
        .expect("point 600");
    assert_eq!(point_600.coordinates.u, -40.0);
    assert_eq!(f3d_native(&result.ir).sketch_curve_identities.len(), 2);
    assert_eq!(
        f3d_native(&result.ir).sketch_curve_identities[0].primary_id,
        440
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_curve_identities[0].secondary_id,
        0
    );
    assert!(matches!(
        f3d_native(&result.ir).sketch_curve_identities[0].geometry,
        Some(cadmpeg_ir::design::SketchCurveGeometry::Arc { radius: 30.0, .. })
    ));
    assert!(matches!(
        &f3d_native(&result.ir).sketch_curve_identities[1].geometry,
        Some(cadmpeg_ir::design::SketchCurveGeometry::Nurbs {
            carrier_reference: Some(42),
            degree: 2,
            weights,
            control_points,
            ..
        }) if weights.is_empty() && control_points.len() == 3
    ));
    assert_eq!(f3d_native(&result.ir).design_body_members.len(), 2);
    assert_eq!(
        f3d_native(&result.ir).design_body_members[0].entity_suffix,
        985
    );
    assert_eq!(
        f3d_native(&result.ir).design_body_members[1].entity_suffix,
        8422
    );
    assert!(f3d_native(&result.ir)
        .design_body_members
        .iter()
        .all(|member| member.flags == 0));
}

#[test]
fn decode_transfers_generated_custom_attribute() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_attribute_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.attributes.len(), 1);
    let attribute = &result.ir.model.attributes[0];
    assert_eq!(attribute.name, "ATTRIB_CUSTOM-attrib");
    assert!(matches!(
        &attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Body(body) if body == &result.ir.model.bodies[0].id
    ));
    assert!(attribute.values.iter().any(|value| matches!(
        value,
        cadmpeg_ir::attributes::AttributeValue::String(text) if text == "322"
    )));
    assert_eq!(f3d_native(&result.ir).persistent_design_links.len(), 1);
    assert_eq!(
        f3d_native(&result.ir).persistent_design_links[0].design_id,
        "322"
    );
    assert!(f3d_native(&result.ir).persistent_design_links[0].is_current);
}

#[test]
fn decode_transfers_generated_sketch_curve_link() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let link = f3d_native(&result.ir).sketch_curve_links.first().unwrap();
    assert_eq!(link.coedge.0, "f3d:brep:entity#7");
    assert_eq!(link.sketch_curve_id, 113);
    assert_eq!(link.signed_reference, Some(1));
    assert_eq!((link.role, link.closure), (2, 3));
}

#[test]
fn decode_mixed_analytic_and_unknown_faces_sharing_an_edge() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let f3d = f3d_with_smbh(&synthetic_mixed_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report.geometry_transferred);
    // Two faces (one plane, one spline), sharing one edge; five edges total.
    assert_eq!(result.ir.model.faces.len(), 2);
    assert_eq!(result.ir.model.edges.len(), 5);
    assert_eq!(result.ir.model.vertices.len(), 4);
    assert_eq!(result.ir.model.coedges.len(), 6);

    // Exactly one analytic (plane) and one unknown surface.
    let planes = result
        .ir
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let unknowns = result
        .ir
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Unknown { .. }))
        .count();
    assert_eq!((planes, unknowns), (1, 1));

    // The shared edge is used by two mutually-referencing coedges of opposite
    // sense (the manifold invariant), which coedge-pairing validation enforces.
    let paired = result
        .ir
        .model
        .coedges
        .iter()
        .filter(|c| c.radial_next != c.id)
        .count();
    assert_eq!(paired, 2);

    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
    // Both the analytic face and the unknown-surface face are present and each
    // references a surface that exists in the arena.
    assert_eq!(result.ir.model.surfaces.len(), 2);
}
