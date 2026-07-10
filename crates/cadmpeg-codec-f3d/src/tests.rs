// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Tests over synthetic byte fixtures. No real CAD files exist in this repo and
//! none may be added, so every fixture is hand-built here to exercise a real
//! decode path that can fail if the code regresses.

use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
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

    // History boundary: 0x11 0x0d 0x0b "delta_state" ... (spec §4a).
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

/// Add a generated inline 2D `nubs` pcurve to the first coedge of the base
/// topology fixture. The new record is appended at `RecordTable` index 19.
fn synthetic_geometry_with_pcurve_smbh() -> Vec<u8> {
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
    pcurve.extend_from_slice(&generated_pcurve_block());
    t_end(&mut pcurve);
    bytes.splice(delta..delta, pcurve);
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

fn f3d_with_smbh_and_protein(smbh: &[u8]) -> Vec<u8> {
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    nested
        .start_file("AssetData/InstanceProperties.bin", stored)
        .unwrap();
    nested.write_all(&generated_instance_properties()).unwrap();
    nested
        .start_file("AssetData/DefinitionIteratorProperties.bin", stored)
        .unwrap();
    nested.write_all(&generated_definition_catalog()).unwrap();
    let protein = nested.finish().unwrap().into_inner();

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(b"synthetic-manifest").unwrap();
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.synthetic.protein",
        stored,
    )
    .unwrap();
    zip.write_all(&protein).unwrap();
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

fn generated_instance_properties() -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let mut logical = b"\x80\x00\x01\x00".to_vec();
    lp(&mut logical, "GenericSchema");
    lp(&mut logical, "11111111-2222-3333-4444-555555555555");
    lp(&mut logical, "Prism-001");
    lp(&mut logical, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    let value_block = logical.len();
    logical.resize(value_block + 112, 0);
    for value in [0.1f64, 0.2, 0.3, 1.0] {
        logical.extend_from_slice(&value.to_le_bytes());
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(0x88u32).to_le_bytes());
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let first = logical.len().min(132);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&logical[..first]);
    bytes.resize(16 + 136, 0);
    if first < logical.len() {
        let rest = &logical[first..];
        bytes.extend_from_slice(&[0xff; 4]);
        bytes.extend_from_slice(&(rest.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(rest);
        bytes.resize(16 + 272, 0);
    }
    bytes
}

fn generated_definition_catalog() -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    let mut out = b"\x80\x00\x01\x00".to_vec();
    for value in [
        "GenericSchema",
        "Prism-001",
        "Default",
        "Plastic/Thermoplastic",
    ] {
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
    assert_eq!(result.ir.unknowns.len(), 1);
    assert_eq!(result.ir.unknowns[0].sha256.len(), 64);
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
    // be selected as a fallback but flagged as non-authoritative (spec §3).
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
