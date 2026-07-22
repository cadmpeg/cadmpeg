// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Tests over synthetic byte fixtures. No real CAD files exist in this repo and
//! none may be added, so every fixture is hand-built here to exercise a real
//! decode path that can fail if the code regresses.

use std::io::{Cursor, Read, Write};

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions, Encoder};
use cadmpeg_ir::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::asm_header;
use crate::bytes::lp_utf16_bytes;
use crate::container::{self, role};
use crate::F3dCodec;

fn with_scan<T>(bytes: &[u8], f: impl FnOnce(&container::ContainerScan<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    f(&scan)
}

/// Build a synthetic ASM `BinaryFile8` BREP stream: a spec-shaped header
/// followed by a couple of filler records and a `delta_state` history marker.
fn synthetic_smbh() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile8"); // 0..15 magic
    b.extend_from_slice(&23100u32.to_le_bytes()); // 15..19 release word
    b.extend_from_slice(&[0u8; 12]); // 19..31 zero
    b.extend_from_slice(&7u64.to_le_bytes()); // 31..39 entity-count word
    b.extend_from_slice(&3u64.to_le_bytes()); // 39..47 flags: history partition
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
    b.extend_from_slice(b"ASM BinaryFile8");
    b.extend_from_slice(&23100u32.to_le_bytes()); // release word
    b.extend_from_slice(&[0u8; 12]); // zero region
    b.extend_from_slice(&5u64.to_le_bytes()); // entity-count word
    b.extend_from_slice(&3u64.to_le_bytes()); // flags: history partition
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
fn t_u16_string(b: &mut Vec<u8>, value: &str) {
    b.push(0x08);
    b.extend_from_slice(&u16::try_from(value.len()).unwrap().to_le_bytes());
    b.extend_from_slice(value.as_bytes());
}

fn renamed_generated_subtype(mut bytes: Vec<u8>, old: &str, new: &str) -> Vec<u8> {
    let old = old.as_bytes();
    let position = bytes
        .windows(old.len())
        .position(|window| window == old)
        .expect("generated subtype name");
    assert!(matches!(
        bytes.get(position.wrapping_sub(2)),
        Some(0x0d | 0x0e)
    ));
    bytes[position - 1] = u8::try_from(new.len()).expect("short subtype name");
    bytes.splice(position..position + old.len(), new.bytes());
    bytes
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
    let native = ir.native.namespace("f3d").expect("F3D native namespace");
    assert_eq!(native.version, crate::native::F3D_NATIVE_VERSION);
}

fn f3d_native(ir: &cadmpeg_ir::document::CadIr) -> crate::native::F3dNative {
    crate::native::F3dNative::load(ir.native.namespace("f3d").expect("F3D native namespace"))
        .unwrap()
}

struct F3dNativeMut<'a> {
    ir: &'a mut cadmpeg_ir::document::CadIr,
    native: crate::native::F3dNative,
}

impl std::ops::Deref for F3dNativeMut<'_> {
    type Target = crate::native::F3dNative;

    fn deref(&self) -> &Self::Target {
        &self.native
    }
}

impl std::ops::DerefMut for F3dNativeMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.native
    }
}

impl Drop for F3dNativeMut<'_> {
    fn drop(&mut self) {
        self.native
            .store(self.ir.native.namespace_mut("f3d"))
            .unwrap();
    }
}

fn f3d_native_mut(ir: &mut cadmpeg_ir::document::CadIr) -> F3dNativeMut<'_> {
    let native = ir
        .native
        .namespace("f3d")
        .map(crate::native::F3dNative::load)
        .transpose()
        .unwrap()
        .unwrap_or_default();
    F3dNativeMut { ir, native }
}

#[test]
fn native_arenas_have_pinned_shape_and_typed_round_trip() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = decoded.ir.native.namespace("f3d").unwrap();
    let typed = crate::native::F3dNative::load(original).unwrap();
    let mut round_trip = cadmpeg_ir::NativeNamespace::default();
    typed.store(&mut round_trip).unwrap();
    assert_eq!(typed, crate::native::F3dNative::load(&round_trip).unwrap());
    assert_eq!(round_trip.version, crate::native::F3D_NATIVE_VERSION);
    assert_eq!(
        round_trip
            .arenas
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        crate::native::F3D_ARENA_NAMES
    );
    for records in round_trip.arenas.values() {
        for record in records {
            let json = serde_json::to_value(record).unwrap();
            assert_eq!(json["id"], record.id);
            assert!(json.as_object().unwrap().len() > 1);
        }
    }
}

#[test]
fn diff_reports_design_material_assignment_changes() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut edited = decoded.ir.clone();
    edited
        .native
        .namespace_mut("f3d")
        .arenas
        .get_mut("design_material_assignments")
        .unwrap()[0]
        .fields
        .insert("entity_suffix".into(), serde_json::json!(123456));
    let report = cadmpeg_ir::diff(&decoded.ir, &edited);
    let arena = report
        .per_arena
        .iter()
        .find(|arena| arena.kind == "native.f3d.design_material_assignments")
        .unwrap();
    assert_eq!(arena.modified.len(), 1);
}

fn update_f3d_native<R>(
    ir: &mut cadmpeg_ir::document::CadIr,
    update: impl FnOnce(&mut crate::native::F3dNative) -> R,
) -> R {
    let mut native = f3d_native_mut(ir);
    update(&mut native)
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
    let verts = [(13i64, 10, 0, 16), (14, 10, 1, 17), (15, 12, 0, 18)];
    for (_id, edge, index_flag, point) in verts {
        t_ident(&mut r, "vertex");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, edge); // 3 owning_edge
        t_long(&mut r, index_flag); // 4 index_flag
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
        t_end(&mut r);
    }

    // History boundary: previous record's 0x11 + 0x0d 0x0b 'delta_state'.
    t_ident(&mut r, "delta_state"); // 0x0d 0x0b 'delta_state'

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

fn replace_generated_record_head(bytes: &mut Vec<u8>, from: &str, to: &str) {
    let mut needle = vec![0x0d, from.len() as u8];
    needle.extend_from_slice(from.as_bytes());
    let mut replacement = vec![0x0d, to.len() as u8];
    replacement.extend_from_slice(to.as_bytes());
    let offsets = bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
        .collect::<Vec<_>>();
    for offset in offsets.into_iter().rev() {
        bytes.splice(offset..offset + needle.len(), replacement.iter().copied());
    }
}

fn append_generated_record_tail(bytes: &mut Vec<u8>, head: &str, tail: &[u8]) {
    let record_start = bytes
        .windows(b"\x0d\x09asmheader".len())
        .position(|window| window == b"\x0d\x09asmheader")
        .expect("generated ASM record table");
    let offsets = crate::sab::frame(bytes, record_start, bytes.len(), 8)
        .expect("generated ASM records must frame")
        .into_iter()
        .filter(|record| record.head == head)
        .map(|record| record.offset + record.len - 1)
        .collect::<Vec<_>>();
    for offset in offsets.into_iter().rev() {
        bytes.splice(offset..offset, tail.iter().copied());
    }
}

#[test]
fn decode_transfers_generated_tolerant_coedge_parameters_and_topology() {
    let mut smbh = synthetic_geometry_smbh();
    let mut parameter_tail = Vec::new();
    t_dbl(&mut parameter_tail, 0.25);
    t_dbl(&mut parameter_tail, 0.75);
    t_ref(&mut parameter_tail, -1);
    t_long(&mut parameter_tail, 0);
    t_long(&mut parameter_tail, 0);
    append_generated_record_tail(&mut smbh, "coedge", &parameter_tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");
    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("generated tolerant coedges must decode");

    assert_eq!(decoded.ir.model.coedges.len(), 3);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    assert_eq!(decoded.ir.model.shells[0].faces.len(), 1);
    assert_eq!(
        f3d_native(&decoded.ir)
            .tolerant_coedge_parameters
            .iter()
            .map(|parameters| parameters.parameter_range)
            .collect::<Vec<_>>(),
        vec![[0.25, 0.75]; 3]
    );
    assert!(f3d_native(&decoded.ir)
        .tolerant_coedge_parameters
        .iter()
        .all(|parameters| matches!(
            parameters.extension,
            crate::records::TolerantCoedgeExtension::Empty { target: None }
        )));

    decoded.ir.model.coedges[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    update_f3d_native(&mut decoded.ir, |native| {
        native.tolerant_coedge_parameters[0].parameter_range = [-1.5, 2.25];
    });
    let mut edited = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut edited)
        .expect("tolerant coedge sense edit");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(edited), &DecodeOptions::default())
        .expect("edited tolerant coedge round trip");
    assert_eq!(
        round_trip.ir.model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        f3d_native(&round_trip.ir).tolerant_coedge_parameters[0].parameter_range,
        [-1.5, 2.25]
    );
}

#[test]
fn decode_selects_tolerant_coedge_extension_from_asm_release() {
    for (release, fixed_tail, expected) in [
        (
            23000u32,
            {
                let mut bytes = Vec::new();
                t_ref(&mut bytes, -1);
                t_long(&mut bytes, 1);
                bytes.extend_from_slice(&[0x0a, 0x0f]);
                t_long(&mut bytes, 22800);
                bytes.extend_from_slice(&[0x10, 0x0a]);
                t_dbl(&mut bytes, -2.0);
                bytes.push(0x0a);
                t_dbl(&mut bytes, 3.0);
                t_long(&mut bytes, 0);
                bytes
            },
            crate::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: true,
                payload_token_count: 1,
                parameter_range: Some([-2.0, 3.0]),
            },
        ),
        (
            21900u32,
            {
                let mut bytes = Vec::new();
                t_ref(&mut bytes, 17);
                bytes
            },
            crate::records::TolerantCoedgeExtension::Reference { target: Some(17) },
        ),
        (
            21400u32,
            Vec::new(),
            crate::records::TolerantCoedgeExtension::None,
        ),
    ] {
        let mut smbh = synthetic_geometry_smbh();
        smbh[15..19].copy_from_slice(&release.to_le_bytes());
        let mut tail = Vec::new();
        t_dbl(&mut tail, -0.5);
        t_dbl(&mut tail, 1.5);
        tail.extend_from_slice(&fixed_tail);
        append_generated_record_tail(&mut smbh, "coedge", &tail);
        replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("release-selected tolerant coedges must decode");
        assert_eq!(
            f3d_native(&decoded.ir)
                .tolerant_coedge_parameters
                .iter()
                .map(|parameters| parameters.extension.clone())
                .collect::<Vec<_>>(),
            vec![expected; 3]
        );
    }
}

#[test]
fn decode_transfers_embedded_tolerant_coedge_use_curves() {
    let mut smbh = synthetic_geometry_smbh();
    let mut tail = Vec::new();
    t_dbl(&mut tail, 0.0);
    t_dbl(&mut tail, 1.0);
    t_ref(&mut tail, -1);
    t_long(&mut tail, 1);
    tail.extend_from_slice(&[0x0a, 0x0f]);
    tail.extend_from_slice(&generated_curve_block());
    tail.extend_from_slice(&[0x10, 0x0a]);
    t_dbl(&mut tail, -2.0);
    tail.push(0x0a);
    t_dbl(&mut tail, 3.0);
    t_long(&mut tail, 0);
    append_generated_record_tail(&mut smbh, "coedge", &tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("embedded tolerant-coedge curves must decode");
    assert_eq!(
        decoded
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count(),
        3
    );
    assert!(decoded.ir.model.coedges.iter().all(|coedge| {
        coedge.use_curve_parameter_range == Some([-2.0, 3.0])
            && coedge.use_curve.as_ref().is_some_and(|id| {
                decoded.ir.model.curves.iter().any(|curve| {
                    curve.id == *id
                        && matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref nurbs) if nurbs.degree == 2)
                })
            })
    }));
    let first_use_curve = decoded.ir.model.coedges[0]
        .use_curve
        .as_ref()
        .and_then(|id| decoded.ir.model.curves.iter().find(|curve| curve.id == *id))
        .expect("first embedded use curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(first_use_curve) = &first_use_curve.geometry
    else {
        panic!("embedded use curve must be NURBS")
    };
    assert_eq!(
        first_use_curve.control_points[0],
        cadmpeg_ir::math::Point3::new(20.0, 0.0, 0.0)
    );
    assert_eq!(
        first_use_curve.control_points[2],
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(first_use_curve.knots, [-1.0, -1.0, -1.0, -0.0, -0.0, -0.0]);

    let mut edited = decoded.ir.clone();
    let use_curve = edited.model.coedges[0]
        .use_curve
        .clone()
        .expect("first coedge use curve");
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == use_curve)
        .expect("embedded use-curve carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("embedded use curve must be NURBS")
    };
    nurbs.control_points[0].x += 1.0;
    let expected = nurbs.clone();
    let mut preserved = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut preserved)
        .expect("embedded use-curve edit");
    let preserved = F3dCodec
        .decode(&mut Cursor::new(preserved), &DecodeOptions::default())
        .expect("embedded use-curve edit round trip");
    assert!(preserved.ir.model.curves.iter().any(|curve| {
        curve.id == use_curve
            && matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref curve) if *curve == expected)
    }));

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let generated_curve_id = cadmpeg_ir::ids::CurveId("generated:tolerant-use-curve#0".into());
    source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: generated_curve_id.clone(),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(expected.clone()),
        source_object: None,
    });
    let tolerant_coedge = source_less.model.coedges[0].id.clone();
    source_less.model.coedges[0].use_curve = Some(generated_curve_id);
    source_less.model.coedges[0].use_curve_parameter_range = Some([-2.0, 3.0]);
    f3d_native_mut(&mut source_less).tolerant_coedge_parameters =
        vec![crate::records::TolerantCoedgeParameters {
            id: "generated:tolerant-coedge-parameters#0".into(),
            coedge: tolerant_coedge,
            record_index: 0,
            parameter_range: [0.0, 1.0],
            extension: crate::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: false,
                payload_token_count: 0,
                parameter_range: Some([-2.0, 3.0]),
            },
        }];
    let mut generated = Vec::new();
    F3dCodec
        .encode(&source_less, &mut generated)
        .expect("source-less embedded use curves");
    let generated = F3dCodec
        .decode(&mut Cursor::new(generated), &DecodeOptions::default())
        .expect("source-less embedded use-curve round trip");
    assert_eq!(
        generated
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count(),
        1
    );
    assert!(generated.ir.model.curves.iter().any(|curve| {
        matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref curve) if *curve == expected)
    }));
}

#[test]
fn decode_frames_history_less_stream_whose_final_record_ends_at_eof() {
    // A history-less `.smb` stream has no `delta_state` boundary and its final
    // `End-of-ASM-data` record ends at EOF without the `0x11` terminator.
    let mut smbh = synthetic_geometry_smbh();
    let marker = smbh
        .windows(b"\x0d\x0bdelta_state".len())
        .position(|window| window == b"\x0d\x0bdelta_state")
        .expect("generated history boundary");
    smbh.truncate(marker);
    for name in ["End", "of", "ASM"] {
        t_subident(&mut smbh, name);
    }
    t_ident(&mut smbh, "data"); // no trailing 0x11
    assert!(crate::asm_header::first_delta_state_offset(&smbh).is_none());

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("history-less stream must decode");
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    assert_eq!(decoded.ir.model.vertices.len(), 3);
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
    for reference in [-1, 1, 0, -1, 0] {
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
    t_ref(&mut tail, 1830);
    t_ref(&mut tail, -1);
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
    transform.extend_from_slice(&[0x0b, 0x0b, 0x0b]);
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

fn synthetic_geometry_with_mesh_surface_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let limit = crate::asm_header::first_delta_state_offset(&bytes).expect("history boundary");
    let start = crate::asm_header::record_stream_start(&bytes).expect("record stream");
    let records = crate::sab::frame(&bytes, start, limit, 8).expect("generated SAB");
    let plane = records
        .iter()
        .find(|record| record.head == "plane")
        .expect("generated plane surface");
    let mut sentinel = Vec::new();
    t_ident(&mut sentinel, "mesh_surface");
    t_end(&mut sentinel);
    bytes.splice(plane.offset..plane.offset + plane.len, sentinel);
    bytes
}

/// Add a generated inline 2D `nubs` pcurve to the first coedge of the base
/// topology fixture. The new record is appended at `RecordTable` index 19.
fn synthetic_geometry_with_pcurve_smbh() -> Vec<u8> {
    synthetic_geometry_with_pcurve_block_smbh(generated_pcurve_block())
}

fn synthetic_geometry_with_wrapped_ref_pcurve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let opener = bytes
        .windows(b"\x0f\x0d\x0bexp_par_cur".len())
        .position(|window| window == b"\x0f\x0d\x0bexp_par_cur")
        .expect("generated wrapped pcurve subtype");
    let close = bytes[opener..]
        .windows([0x10, 0x0a, 0x0b, 0x0a, 0x0b].len())
        .position(|window| window == [0x10, 0x0a, 0x0b, 0x0a, 0x0b])
        .map(|offset| opener + offset)
        .expect("generated wrapped pcurve subtype close");
    let mut reference = vec![0x0f];
    t_ident(&mut reference, "ref");
    t_long(&mut reference, 0);
    reference.push(0x10);
    bytes.splice(opener..=close, reference);

    let delta = bytes
        .windows(b"delta_state".len())
        .position(|window| window == b"delta_state")
        .unwrap()
        - 2;
    let mut target = Vec::new();
    t_subident(&mut target, "intcurve");
    t_ident(&mut target, "curve");
    t_ref(&mut target, -1);
    t_long(&mut target, -1);
    t_ref(&mut target, -1);
    target.push(0x0f);
    t_ident(&mut target, "int_int_cur");
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    t_end(&mut target);
    bytes.splice(delta..delta, target);
    bytes
}

fn synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh() -> Vec<u8> {
    replace_generated_face_with_nurbs_surface(synthetic_geometry_with_pcurve_smbh())
}

fn replace_generated_face_with_nurbs_surface(mut bytes: Vec<u8>) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let old = &records[6];
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.extend_from_slice(&generated_surface_block());
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh() -> Vec<u8> {
    replace_generated_face_with_nurbs_surface(synthetic_geometry_with_ref_pcurve_smbh())
}

fn synthetic_geometry_with_short_pcurve_tail_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let marker = [0x10, 0x0a, 0x0b, 0x0a, 0x0b, 0x06];
    let tail = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated inline pcurve tail");
    bytes.remove(tail + 1);
    bytes
}

fn synthetic_geometry_with_out_of_scope_pcurve_cache_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh();
    let subtype = bytes
        .windows(b"exp_par_cur".len())
        .position(|window| window == b"exp_par_cur")
        .expect("generated inline pcurve subtype");
    let cache = bytes[subtype..]
        .windows(b"nubs".len())
        .position(|window| window == b"nubs")
        .map(|offset| subtype + offset)
        .expect("generated inline pcurve cache");
    bytes[cache] = b'x';
    bytes
}

fn synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_pcurve_smbh();
    let subtype = bytes
        .windows(b"exp_par_cur".len())
        .position(|window| window == b"exp_par_cur")
        .expect("generated inline pcurve subtype");
    let tail = bytes[subtype..]
        .windows([0x10, 0x0a, 0x0b, 0x0a, 0x0b].len())
        .position(|window| window == [0x10, 0x0a, 0x0b, 0x0a, 0x0b])
        .map(|offset| subtype + offset)
        .expect("generated inline pcurve subtype close");
    bytes.splice(tail + 1..tail + 1, generated_pcurve_block());
    bytes
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

    // Move the coedge's edge endpoints onto the pcurve's surface image so the
    // fixture stays geometrically consistent: the plane maps `(u, v)` to
    // `(u, v, 0)` mm, and the block runs `(0.25, 0.5) -> (0.75, 1.5)`.
    for (index, position_cm) in [(16usize, [0.025, 0.05, 0.0]), (17, [0.075, 0.15, 0.0])] {
        let point = &records[index];
        let record = &mut bytes[point.offset..point.offset + point.len];
        let tag = record.iter().position(|b| *b == 0x13).unwrap();
        for (slot, value) in position_cm.iter().copied().enumerate() {
            record[tag + 1 + slot * 8..tag + 9 + slot * 8]
                .copy_from_slice(&f64::to_le_bytes(value));
        }
    }

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
    pcurve.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b]);
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

fn with_pcurve_discriminator(mut bytes: Vec<u8>, discriminator: i64) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head == "pcurve")
        .expect("generated pcurve record");
    let offsets = crate::sab::payload_token_offsets(&bytes, pcurve, 8, 0x04)
        .expect("generated pcurve integer offsets");
    bytes[offsets[1] + 1..offsets[1] + 9].copy_from_slice(&discriminator.to_le_bytes());
    bytes
}

fn with_inline_pcurve_non_boolean_wrapper(mut bytes: Vec<u8>) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head == "pcurve")
        .expect("generated pcurve record");
    let integers = crate::sab::payload_token_offsets(&bytes, pcurve, 8, 0x04)
        .expect("generated pcurve integer offsets");
    let wrapper = integers[1] + 9;
    assert_eq!(bytes[wrapper], 0x0b, "generated inline wrapper boolean");
    bytes.splice(wrapper..=wrapper, [0x02, 0x00]);
    bytes
}

fn with_ref_pcurve_companion_name(mut bytes: Vec<u8>, name: &[u8; 8]) -> Vec<u8> {
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let pcurve = records
        .iter()
        .find(|record| record.head == "pcurve")
        .expect("generated pcurve record");
    let companion_index = pcurve.ref_at(4).expect("generated ref-form companion");
    let companion = &records[usize::try_from(companion_index).unwrap()];
    let head = bytes[companion.offset..companion.offset + companion.len]
        .windows(b"intcurve".len())
        .position(|window| window == b"intcurve")
        .map(|offset| companion.offset + offset)
        .expect("generated intcurve companion name");
    bytes[head..head + name.len()].copy_from_slice(name);
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

fn synthetic_geometry_with_cacheless_helix_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_helix_curve_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let helix = records.iter().find(|record| record.index == 19).unwrap();
    let block = generated_curve_block();
    let relative = bytes[helix.offset..helix.offset + helix.len]
        .windows(block.len())
        .position(|window| window == block)
        .unwrap();
    let cache = helix.offset + relative;
    bytes.drain(cache..cache + block.len() + 9);
    bytes
}

fn synthetic_geometry_with_law_curve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let edge = &records[10];
    let offsets = crate::sab::payload_token_offsets(&bytes, edge, 8, 0x0c).unwrap();
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
    t_ident(&mut curve, "law_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0005);
    for origin in [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        t_ident(&mut curve, "plane");
        t_pos(&mut curve, origin);
        t_vec(&mut curve, [0.0, 0.0, 1.0]);
        t_vec(&mut curve, [1.0, 0.0, 0.0]);
        curve.push(0x0b);
    }
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    for values in [&[0.25][..], &[][..], &[][..]] {
        append_generated_float_array(&mut curve, values);
    }
    t_long(&mut curve, 0);
    push_u8_string(&mut curve, "primary_law");
    t_long(&mut curve, 1);
    push_u8_string(&mut curve, "EDGE");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, -0.5);
    t_dbl(&mut curve, 1.5);
    t_long(&mut curve, 2);
    push_u8_string(&mut curve, "null_law");
    push_u8_string(&mut curve, "null_law");
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

fn synthetic_geometry_with_exact_curve_smbh() -> Vec<u8> {
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
    t_ident(&mut curve, "exact_int_cur");
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0004);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_decoy_curve_sense_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_exact_curve_smbh();
    let marker = b"\x0f\x0d\x0dexact_int_cur";
    let subtype = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated exact intcurve subtype");
    bytes.splice(subtype..subtype, [0x0a, 0x0b]);
    bytes
}

fn with_legacy_subtype(mut bytes: Vec<u8>, modern: &str, legacy: &str) -> Vec<u8> {
    let position = bytes
        .windows(modern.len())
        .position(|window| window == modern.as_bytes())
        .expect("generated modern subtype");
    bytes[position - 1] = legacy.len() as u8;
    bytes.splice(
        position..position + modern.len(),
        legacy.as_bytes().iter().copied(),
    );
    bytes
}

fn synthetic_geometry_with_compound_curve_smbh() -> Vec<u8> {
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
    t_ident(&mut curve, "comp_int_cur");
    t_long(&mut curve, 3);
    for value in [0.0, 0.5, 1.0] {
        t_dbl(&mut curve, value);
    }
    t_long(&mut curve, 2);
    t_dbl(&mut curve, -2.0);
    t_dbl(&mut curve, 4.0);
    curve.push(0x0b);
    curve.extend_from_slice(&generated_curve_block());
    curve.extend_from_slice(&generated_curve_block());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0003);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_two_sided_offset_curve_smbh() -> Vec<u8> {
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
    t_ident(&mut curve, "off_int_cur");
    for name in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        t_ident(&mut curve, name);
    }
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 2);
    t_dbl(&mut curve, 0.25);
    t_dbl(&mut curve, 0.75);
    t_long(&mut curve, 0);
    t_long(&mut curve, 1);
    t_dbl(&mut curve, 0.5);
    curve.push(0x0a);
    t_dbl(&mut curve, -0.2);
    t_dbl(&mut curve, 0.4);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0002);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_embedded_offset_supports_smbh() -> Vec<u8> {
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
    t_ident(&mut curve, "off_int_cur");
    for _ in 0..2 {
        t_ident(&mut curve, "spline");
        curve.extend_from_slice(&generated_surface_block());
    }
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_rational_pcurve_block());
    t_dbl(&mut curve, 0.0);
    t_dbl(&mut curve, 1.0);
    for _ in 0..3 {
        t_long(&mut curve, 0);
    }
    curve.push(0x0b);
    t_dbl(&mut curve, -0.1);
    t_dbl(&mut curve, 0.3);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0001);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_analytic_offset_supports_smbh() -> Vec<u8> {
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
    t_ident(&mut curve, "off_int_cur");
    t_ident(&mut curve, "cone");
    t_pos(&mut curve, [1.0, 2.0, 3.0]);
    t_vec(&mut curve, [0.0, 0.0, 1.0]);
    t_vec(&mut curve, [1.0, 0.0, 0.0]);
    t_dbl(&mut curve, 0.4);
    curve.extend_from_slice(&[0x0b; 2]);
    t_dbl(&mut curve, -0.5);
    t_dbl(&mut curve, 3.0_f64.sqrt() / 2.0);
    t_dbl(&mut curve, 1.25);
    curve.extend_from_slice(&[0x0b; 5]);
    t_ident(&mut curve, "torus");
    t_pos(&mut curve, [-1.0, 0.5, 2.0]);
    t_vec(&mut curve, [0.0, 1.0, 0.0]);
    t_dbl(&mut curve, 2.5);
    t_dbl(&mut curve, -0.75);
    t_vec(&mut curve, [1.0, 0.0, 0.0]);
    curve.extend_from_slice(&[0x0b; 5]);
    curve.extend_from_slice(&generated_pcurve_block());
    curve.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut curve, 0.0);
    t_dbl(&mut curve, 1.0);
    for _ in 0..3 {
        t_long(&mut curve, 0);
    }
    curve.push(0x0b);
    t_dbl(&mut curve, -0.15);
    t_dbl(&mut curve, 0.25);
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0001);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

fn synthetic_geometry_with_surface_intersection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype..subtype + b"int_int_cur".len()].copy_from_slice(b"int_int_cur");
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes[solved - 19] = 0x0a;
    bytes.drain(solved - 18..solved);
    bytes
}

fn synthetic_geometry_with_projection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype - 1] = b"proj_int_cur".len() as u8;
    bytes.splice(
        subtype..subtype + b"off_int_cur".len(),
        b"proj_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes[solved - 19] = 0x0a;
    let mut tail = generated_curve_block();
    tail.push(0x0a);
    t_dbl(&mut tail, -2.0);
    t_dbl(&mut tail, 3.0);
    push_u8_string(&mut tail, "surf2");
    bytes.splice(solved - 18..solved, tail);
    bytes
}

fn synthetic_geometry_with_early_close_projection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_projection_smbh();
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let source = bytes[..solved]
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated projection source curve");
    let source_end = source + generated_curve_block().len();
    bytes.splice(source_end..solved, [0x0a, 0x10]);
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("shifted solved curve cache");
    let fit_end = solved + generated_curve_block().len() + 9;
    assert_eq!(bytes[fit_end], 0x10);
    bytes.remove(fit_end);
    bytes
}

fn synthetic_geometry_with_three_surface_intersection_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype..subtype + b"sss_int_cur".len()].copy_from_slice(b"sss_int_cur");
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut third = Vec::new();
    t_long(&mut third, 7);
    t_ident(&mut third, "sphere");
    t_pos(&mut third, [0.5, 1.0, -2.0]);
    t_dbl(&mut third, -1.25);
    t_vec(&mut third, [1.0, 0.0, 0.0]);
    t_vec(&mut third, [0.0, 0.0, 1.0]);
    third.extend_from_slice(&[0x0b; 5]);
    third.extend_from_slice(&generated_rational_pcurve_block());
    bytes.splice(solved - 19..solved, third);
    bytes
}

fn synthetic_geometry_with_surface_curve_smbh(name: &str) -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = name.len() as u8;
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        name.as_bytes().iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    bytes.remove(solved - 1);
    bytes
}

fn synthetic_geometry_with_silhouette_smbh(name: &str, draft_factor: Option<f64>) -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = name.len() as u8;
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        name.as_bytes().iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut tail = Vec::new();
    t_ident(&mut tail, "sphere");
    t_pos(&mut tail, [0.0, 0.0, 0.0]);
    t_dbl(&mut tail, 1.5);
    t_vec(&mut tail, [1.0, 0.0, 0.0]);
    t_vec(&mut tail, [0.0, 0.0, 1.0]);
    tail.extend_from_slice(&[0x0b; 5]);
    t_vec(&mut tail, [0.0, -2.0, 0.0]);
    if let Some(draft_factor) = draft_factor {
        t_dbl(&mut tail, draft_factor);
    }
    bytes.splice(solved - 1..solved, tail);
    bytes
}

fn synthetic_geometry_with_surface_offset_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_analytic_offset_supports_smbh();
    let subtype = bytes
        .windows(b"off_int_cur".len())
        .position(|window| window == b"off_int_cur")
        .expect("generated offset subtype");
    bytes[subtype - 1] = b"off_surf_int_cur".len() as u8;
    bytes.splice(
        subtype..subtype + b"off_int_cur".len(),
        b"off_surf_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut tail = vec![0x0a];
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut tail, value);
    }
    tail.extend_from_slice(&generated_curve_block());
    t_dbl(&mut tail, -0.5);
    t_dbl(&mut tail, 1.5);
    t_dbl(&mut tail, -0.25);
    t_dbl(&mut tail, 0.75);
    t_dbl(&mut tail, 1.25);
    bytes.splice(solved - 19..solved, tail);
    bytes
}

fn synthetic_geometry_with_spring_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_surface_intersection_smbh();
    let subtype = bytes
        .windows(b"int_int_cur".len())
        .position(|window| window == b"int_int_cur")
        .expect("generated intersection subtype");
    bytes[subtype - 1] = b"spring_int_cur".len() as u8;
    bytes.splice(
        subtype..subtype + b"int_int_cur".len(),
        b"spring_int_cur".iter().copied(),
    );
    let solved = bytes
        .windows(b"\x0d\x04nubs".len())
        .rposition(|window| window == b"\x0d\x04nubs")
        .expect("generated solved curve cache");
    let mut direction = Vec::new();
    direction.push(0x15);
    direction.extend_from_slice(&(-3i64).to_le_bytes());
    bytes.splice(solved..solved, direction);
    bytes
}

fn synthetic_geometry_with_null_support_spring_smbh() -> Vec<u8> {
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
    t_ident(&mut curve, "spring_int_cur");
    t_ident(&mut curve, "null_surface");
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut curve, value);
    }
    t_ident(&mut curve, "null_surface");
    for value in [-6.0, 7.0, -8.0, 9.0] {
        t_dbl(&mut curve, value);
    }
    t_ident(&mut curve, "nullbs");
    t_dbl(&mut curve, -10.0);
    t_dbl(&mut curve, 11.0);
    t_ident(&mut curve, "nullbs");
    t_dbl(&mut curve, -1.0);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 1);
    t_dbl(&mut curve, 0.25);
    t_long(&mut curve, 0);
    t_long(&mut curve, 2);
    t_dbl(&mut curve, 0.5);
    t_dbl(&mut curve, 0.75);
    curve.push(0x0a);
    curve.push(0x15);
    curve.extend_from_slice(&4i64.to_le_bytes());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0004);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

/// Splice one cache-first intcurve record built by `tail` into the synthetic
/// geometry stream and point edge 10 at it.
fn synthetic_geometry_with_cache_first_curve_smbh(
    subtype: &str,
    tail: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
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
    t_ident(&mut curve, subtype);
    t_long(&mut curve, 23100);
    curve.push(0x15);
    curve.extend_from_slice(&0i64.to_le_bytes());
    curve.extend_from_slice(&generated_curve_block());
    t_dbl(&mut curve, 0.0004);
    t_ident(&mut curve, "null_surface");
    t_ident(&mut curve, "null_surface");
    t_ident(&mut curve, "nullbs");
    t_ident(&mut curve, "nullbs");
    curve.push(0x0a);
    t_dbl(&mut curve, -1.0);
    curve.push(0x0a);
    t_dbl(&mut curve, 2.0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 0);
    t_long(&mut curve, 7);
    tail(&mut curve);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(delta..delta, curve);
    bytes
}

#[test]
fn generated_cache_first_spring_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh("spring_int_cur", |curve| {
                    curve.push(0x15);
                    curve.extend_from_slice(&4i64.to_le_bytes());
                }),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first spring decode");
    let ProceduralCurveDefinition::Spring {
        context,
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        discontinuity_flag,
        cache_first,
        direction,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected spring construction")
    };
    let form = cache_first.as_ref().expect("cache-first spring form");
    assert_eq!(form.revision, 23100);
    assert_eq!(form.solved_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(form.extension, 7);
    assert_eq!(*direction, 4);
    assert!(!discontinuity_flag);
    assert_eq!(*surface_parameter_ranges, [None, None]);
    assert_eq!(*first_pcurve_parameter_range, None);
    assert_eq!(context.parameter_range, [-1.0, 2.0]);

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less cache-first spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first spring round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_cache_first_parametric_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceCurveFamily};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh("par_int_cur", |curve| {
                    curve.push(0x0a);
                    curve.push(0x0b);
                }),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first parametric decode");
    let ProceduralCurveDefinition::SurfaceCurve {
        family,
        context,
        tail,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected surface-curve construction")
    };
    assert_eq!(*family, SurfaceCurveFamily::Parametric);
    let tail = tail.as_ref().expect("cache-first parametric tail");
    assert_eq!(tail.revision, 23100);
    assert_eq!(tail.extension, 7);
    assert!(tail.flag);
    assert_eq!(tail.second_flag, Some(false));
    assert_eq!(tail.solved_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(context.parameter_range, [-1.0, 2.0]);

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less cache-first parametric encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first parametric round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_cache_first_surface_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh("off_surf_int_cur", |curve| {
                    for value in [-1.0, 2.0, -3.0, 4.0] {
                        curve.push(0x0a);
                        t_dbl(curve, value);
                    }
                    curve.extend_from_slice(&generated_curve_block());
                    curve.push(0x0b);
                    curve.push(0x0b);
                    curve.push(0x0a);
                    t_dbl(curve, -0.5);
                    curve.push(0x0a);
                    t_dbl(curve, 1.5);
                    t_dbl(curve, -0.25);
                    t_dbl(curve, 0.75);
                    t_dbl(curve, 1.25);
                }),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first surface-offset decode");
    let ProceduralCurveDefinition::SurfaceOffset {
        cache_first,
        base_u_range,
        base_v_range,
        base_endpoints,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected surface-offset construction")
    };
    let form = cache_first
        .as_ref()
        .expect("cache-first surface-offset form");
    assert_eq!(form.revision, 23100);
    assert_eq!(form.extension, 7);
    assert_eq!(*base_u_range, [-1.0, 2.0]);
    assert_eq!(*base_v_range, [-3.0, 4.0]);
    assert_eq!(*base_endpoints, [None, None]);
    assert_eq!(*base_range, [-0.5, 1.5]);
    assert_eq!(*distance, -2.5);
    assert_eq!(*shift, 0.75);
    assert_eq!(*scale, 1.25);

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less cache-first surface-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first surface-offset round trip");
    let mut expected = source_less.model.procedural_curves[0].definition.clone();
    let mut actual = round_trip.ir.model.procedural_curves[0].definition.clone();
    let (
        ProceduralCurveDefinition::SurfaceOffset {
            base: expected_base,
            ..
        },
        ProceduralCurveDefinition::SurfaceOffset {
            base: actual_base, ..
        },
    ) = (&mut expected, &mut actual)
    else {
        panic!("expected surface-offset round trip")
    };
    let round_trip_base = actual_base.clone();
    *actual_base = expected_base.clone();
    assert_eq!(actual, expected);
    assert!(round_trip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == round_trip_base));
}

fn t_str(b: &mut Vec<u8>, s: &str) {
    b.push(0x07);
    b.push(u8::try_from(s.len()).expect("short string"));
    b.extend_from_slice(s.as_bytes());
}

fn push_revision_surface_tail(surface: &mut Vec<u8>) {
    surface.push(0x15);
    surface.extend_from_slice(&0i64.to_le_bytes());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(surface, 0.002);
    for _ in 0..6 {
        t_long(surface, 0);
    }
    surface.push(0x0b);
}

/// Replace record 9 of the mixed stream with a revision-gated spline-surface
/// record whose subtype body is built by `body`.
fn synthetic_revision_surface_smbh(subtype: &str, body: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
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
    t_ident(&mut surface, subtype);
    t_long(&mut surface, 23100);
    body(&mut surface);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn scrubbed_definition(definition: &cadmpeg_ir::geometry::ProceduralSurfaceDefinition) -> String {
    let text = serde_json::to_string(definition).expect("definition JSON");
    let mut out = String::with_capacity(text.len());
    let mut in_index = false;
    for c in text.chars() {
        if in_index && c.is_ascii_digit() {
            continue;
        }
        in_index = c == '#';
        out.push(c);
    }
    out
}

fn assert_revision_surface_round_trip(smbh: Vec<u8>, expected_kind: &str) {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision surface decode");
    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("revision surface construction");
    let expected = scrubbed_definition(&procedural.definition);
    let kind = serde_json::to_value(&procedural.definition).expect("kind")["kind"]
        .as_str()
        .expect("kind string")
        .to_string();
    assert_eq!(kind, expected_kind);
    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less revision surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less revision surface round trip");
    let actual = scrubbed_definition(
        &round_trip
            .ir
            .model
            .procedural_surfaces
            .first()
            .expect("round-trip construction")
            .definition,
    );
    assert_eq!(actual, expected);
}

#[test]
fn generated_revision_offset_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.push(0x0b);
        t_dbl(surface, 0.3);
        for flag in [false, true, false, false] {
            surface.push(if flag { 0x0a } else { 0x0b });
        }
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "offset");
}

#[test]
fn generated_revision_orthogonal_taper_round_trips() {
    let smbh = synthetic_revision_surface_smbh("ortho_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.extend_from_slice(&generated_pcurve_block());
        t_dbl(surface, 0.5);
        push_revision_surface_tail(surface);
        surface.push(0x0a);
    });
    assert_revision_surface_round_trip(smbh, "taper");
}

#[test]
fn generated_revision_sweep_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("sweep_sur", |surface| {
        surface.push(0x0b);
        t_long(surface, -1);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        t_pos(surface, [1.0, 2.0, 3.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        t_vec(surface, [1.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 1.0, 0.0]);
        t_long(surface, 1);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 0.5);
        t_dbl(surface, 0.0);
        surface.push(0x0b);
        t_str(surface, "MTRAIL(EDGE1)");
        t_long(surface, 1);
        t_str(surface, "EDGE");
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        t_dbl(surface, 0.0);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "sweep");
}

#[test]
fn generated_revision_loft_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        t_long(surface, 1);
        t_dbl(surface, 0.0);
        t_long(surface, 1);
        t_long(surface, 1);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        t_ident(surface, "null_surface");
        t_ident(surface, "nullbs");
        surface.push(0x0b);
        t_long(surface, -1);
        t_long(surface, 213);
        t_long(surface, 1);
        t_long(surface, 1);
        for value in [0.0, 1.0, 0.25, 0.75, 0.5, 1.5] {
            t_dbl(surface, value);
        }
        surface.push(0x0b);
        t_ident(surface, "null_curve");
        t_long(surface, 0);
        t_long(surface, -1);
        t_long(surface, 0);
        for value in [0.0, 1.0, 0.0, 1.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 0);
        t_long(surface, 0);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "loft");
}

fn synthetic_geometry_with_deformable_curve_smbh(mode: i64) -> Vec<u8> {
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
    t_ident(&mut curve, "defm_int_cur");
    t_long(&mut curve, 0);
    curve.extend_from_slice(&generated_curve_block());
    t_long(&mut curve, mode);
    match mode {
        8 => {
            for vector in [
                [1.0, 2.0, 3.0],
                [4.0, 5.0, 6.0],
                [7.0, 8.0, 9.0],
                [10.0, 11.0, 12.0],
            ] {
                t_vec(&mut curve, vector);
            }
            t_long(&mut curve, 2);
            for value in [-1.0, 0.25, 2.0, 3.5] {
                t_dbl(&mut curve, value);
            }
        }
        5 => {
            t_ident(&mut curve, "plane");
            t_pos(&mut curve, [1.0, 2.0, 3.0]);
            t_vec(&mut curve, [0.0, 0.0, 1.0]);
            t_vec(&mut curve, [1.0, 0.0, 0.0]);
            curve.push(0x0b);
        }
        _ => unreachable!(),
    }
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
    t_ref(&mut attribute, 20);
    push_u8_string(&mut attribute, "generic_tag_attrib_def");
    for value in [3, 3, -1] {
        t_long(&mut attribute, value);
    }
    push_u8_string(&mut attribute, "generic_tag_attrib_def ");
    t_long(&mut attribute, 3);
    for (kind, id, reference) in [(3, "311", 6), (4, "900", 42), (3, "322", 7)] {
        t_long(&mut attribute, kind);
        push_u8_string(&mut attribute, id);
        for value in [reference, 0, 0] {
            t_long(&mut attribute, value);
        }
    }
    t_end(&mut attribute);
    t_subident(&mut attribute, "ATTRIB_CUSTOM");
    t_ident(&mut attribute, "attrib");
    t_ref(&mut attribute, -1);
    push_u8_string(&mut attribute, "Timestamp_attrib_def");
    t_long(&mut attribute, 1);
    t_dbl(&mut attribute, 1_579_392_000_000_007.0);
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

fn synthetic_free_vertex_body_smbh() -> Vec<u8> {
    let mut records = Vec::new();
    t_ident(&mut records, "asmheader");
    push_u8_string(&mut records, "231.6.3.65535");
    t_end(&mut records);

    t_ident(&mut records, "body");
    t_ref(&mut records, -1);
    t_long(&mut records, 1);
    for reference in [-1, 2, 4, -1] {
        t_ref(&mut records, reference);
    }
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
    for reference in [-1, -1, -1, 3, 5] {
        t_ref(&mut records, reference);
    }
    records.push(0x0b);
    t_end(&mut records);

    t_ident(&mut records, "vertex");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_ref(&mut records, 4);
    t_long(&mut records, -1);
    t_ref(&mut records, 6);
    t_end(&mut records);

    t_ident(&mut records, "point");
    t_ref(&mut records, -1);
    t_long(&mut records, -1);
    t_ref(&mut records, -1);
    t_pos(&mut records, [1.0, 2.0, 3.0]);
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
    let vertex = &records[14];
    let owner = crate::sab::payload_token_offsets(&bytes, vertex, 8, 0x0c)
        .expect("generated vertex reference offsets")[2];
    bytes[owner + 1..owner + 9].copy_from_slice(&11i64.to_le_bytes());
    let endpoint = crate::sab::payload_token_offsets(&bytes, vertex, 8, 0x04)
        .expect("generated vertex integer offsets")[1];
    bytes[endpoint + 1..endpoint + 9].copy_from_slice(&0i64.to_le_bytes());

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
    synthetic_cyl_spl_sur_with_cache_smbh(true)
}

fn synthetic_versioned_cyl_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;

    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    surface.push(0x0f);
    t_ident(&mut surface, "cyl_spl_sur");
    t_long(&mut surface, 23100);
    t_ident(&mut surface, "intcurve");
    surface.push(0x0a);
    surface.push(0x0f);
    t_ident(&mut surface, "exact_int_cur");
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x10);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.25);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.75);
    t_vec(&mut surface, [0.0, 0.0, 2.0]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.002);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    bytes
}

fn synthetic_cacheless_cyl_spl_sur_smbh() -> Vec<u8> {
    synthetic_cyl_spl_sur_with_cache_smbh(false)
}

fn synthetic_cyl_spl_sur_with_cache_smbh(include_cache: bool) -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;

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
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    surface.extend_from_slice(&generated_curve_block());
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.002);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    bytes
}

fn synthetic_exact_spl_sur_smbh(name: &str) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0015);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_exact_spl_sur_with_decoy_sense_smbh() -> Vec<u8> {
    let mut bytes = synthetic_exact_spl_sur_smbh("exact_spl_sur");
    let marker = b"\x0f\x0d\x0dexact_spl_sur";
    let subtype = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("generated exact spline-surface subtype");
    bytes.splice(subtype..subtype, [0x0a, 0x0b]);
    bytes
}

fn synthetic_ruled_spl_sur_smbh(name: &str, include_cache: bool) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.0025);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_sum_spl_sur_smbh(name: &str, include_cache: bool) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    if include_cache {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.0035);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_rot_spl_sur_smbh(name: &str) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    surface.extend_from_slice(&generated_curve_block());
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0045);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_off_spl_sur_smbh(name: &str) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_dbl(&mut surface, -1.25);
    surface.push(0x15);
    surface.extend_from_slice(&3i64.to_le_bytes());
    surface.push(0x15);
    surface.extend_from_slice(&(-4i64).to_le_bytes());
    if name == "off_spl_sur" {
        surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0055);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_comp_spl_sur_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "comp_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0065);
    t_long(&mut surface, 2);
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_ident(&mut surface, "spline");
    surface.extend_from_slice(&generated_rational_surface_block());
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_taper_spl_sur_smbh(name: &str) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, -2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_pcurve_block());
    t_dbl(&mut surface, 0.35);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0075);
    match name {
        "ortho_spl_sur" | "orthosur" => surface.push(0x0a),
        "edge_tpr_spl_sur" => t_vec(&mut surface, [1.0, 2.0, 3.0]),
        "shadow_tpr_spl_sur" | "shadowtapersur" | "swept_tpr_spl_sur" | "swepttapersur" => {
            t_vec(&mut surface, [1.0, 2.0, 3.0]);
            t_dbl(&mut surface, 0.6);
            t_dbl(&mut surface, 0.8);
        }
        "ruled_tpr_spl_sur" | "ruledtapersur" => {
            t_vec(&mut surface, [1.0, 2.0, 3.0]);
            t_dbl(&mut surface, 0.6);
            t_dbl(&mut surface, 0.8);
            t_dbl(&mut surface, 1.25);
        }
        "taper_spl_sur" => {}
        _ => unreachable!(),
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_generated_loft_section(bytes: &mut Vec<u8>, parameter: f64, direction: bool) {
    t_long(bytes, 1);
    t_dbl(bytes, parameter);
    t_long(bytes, 1);
    t_long(bytes, 9);
    bytes.extend_from_slice(&generated_curve_block());
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_pcurve_block());
    bytes.push(0x0b);
    t_long(bytes, -1);
    t_long(bytes, 211);
    t_long(bytes, 4);
    t_long(bytes, 0);
    t_dbl(bytes, -0.25);
    t_dbl(bytes, 0.75);
    bytes.push(if direction { 0x0a } else { 0x0b });
    if direction {
        t_vec(bytes, [0.0, 1.0, 0.0]);
    }
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 1);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 6);
}

fn synthetic_loft_spl_sur_smbh(name: &str) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    append_generated_loft_section(&mut surface, 0.0, true);
    append_generated_loft_section(&mut surface, 1.0, false);
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut surface, value);
    }
    for value in [1i64, 2, 3, 4] {
        surface.push(0x15);
        surface.extend_from_slice(&value.to_le_bytes());
    }
    t_long(&mut surface, 2);
    surface.push(0x0a);
    t_long(&mut surface, 17);
    t_dbl(&mut surface, 0.125);
    push_u8_string(&mut surface, "bridge");
    surface.push(0x15);
    surface.extend_from_slice(&(-7i64).to_le_bytes());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0085);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_net_spl_sur_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "net_spl_sur");
    append_generated_loft_section(&mut surface, 0.0, true);
    append_generated_loft_section(&mut surface, 1.0, false);
    for value in 0..12 {
        t_dbl(&mut surface, f64::from(value) / 10.0);
    }
    t_long(&mut surface, 17);
    for direction in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, direction);
    }
    for _ in 0..4 {
        push_u8_string(&mut surface, "null_law");
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_profile_first_sweep_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&3i64.to_le_bytes());
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x15);
    surface.extend_from_slice(&4i64.to_le_bytes());
    for direction in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
    ] {
        t_vec(&mut surface, direction);
    }
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    for value in [0.1, 0.2, 0.3, 0.4] {
        t_dbl(&mut surface, value);
    }
    for _ in 0..3 {
        push_u8_string(&mut surface, "null_law");
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_t_spl_sur_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "t_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_subtrans_object");
    t_u16_string(
        &mut surface,
        "degree 3\nunits mm\nv 1 0 0 0\nv 2 1 0 0\ne 1 1 2\n",
    );
    surface.push(0x0b);
    t_u16_string(&mut surface, "100verts 1 2\n");
    surface.push(0x10);
    t_long(&mut surface, 9);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_helix_surface_smbh(circular: bool) -> Vec<u8> {
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
    t_ident(
        &mut surface,
        if circular {
            "helix_spl_circ"
        } else {
            "helix_spl_line"
        },
    );
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 0.5);
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    if circular {
        t_dbl(&mut surface, 1.25);
    }
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, std::f64::consts::TAU);
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_pos(&mut surface, [2.0, 0.0, 0.0]);
    t_pos(&mut surface, [0.0, 2.0, 0.0]);
    t_pos(&mut surface, [0.0, 0.0, 4.0]);
    t_dbl(&mut surface, 0.25);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        t_ident(&mut surface, sentinel);
    }
    if circular {
        t_dbl(&mut surface, 0.75);
    } else {
        t_pos(&mut surface, [5.0, 6.0, 7.0]);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_minimal_deformable_surface_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 8);
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, vector);
    }
    t_long(&mut surface, 0);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_framed_deformable_surface_smbh(mode: i64) -> Vec<u8> {
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
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, mode);
    for vector in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, vector);
    }
    t_dbl(&mut surface, 0.5);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    for vector in [[1.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 0.0, 1.0]] {
        t_vec(&mut surface, vector);
    }
    t_dbl(&mut surface, 0.75);
    surface.extend_from_slice(&[0x0b, 0x0a]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a, 0x0b, 0x0a]);
    if mode == 1 {
        t_long(&mut surface, 2);
        for value in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6] {
            t_dbl(&mut surface, value);
        }
    } else {
        t_long(&mut surface, 1);
        t_dbl(&mut surface, 0.9);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_surface_curve_deformable_smbh() -> Vec<u8> {
    let mut bytes = synthetic_minimal_deformable_surface_smbh();
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
    t_ident(&mut surface, "defm_spl_sur");
    for z in [0.0, 1.0] {
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [0.0, 0.0, z]);
        t_vec(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        if z == 0.0 {
            t_long(&mut surface, 5);
        }
    }
    t_long(&mut surface, 42);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.2);
    t_long(&mut surface, 3);
    t_dbl(&mut surface, 0.4);
    surface.extend_from_slice(&generated_curve_block());
    for v in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, v);
    }
    t_dbl(&mut surface, 0.6);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    t_long(&mut surface, 1);
    for v in [0.1, 0.2, 0.3] {
        t_dbl(&mut surface, v);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_full_deformable_surface_smbh(version_value: Option<i64>) -> Vec<u8> {
    let mut bytes = synthetic_minimal_deformable_surface_smbh();
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
    t_ident(&mut surface, "defm_spl_sur");
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 6);
    for v in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ] {
        t_vec(&mut surface, v);
    }
    t_dbl(&mut surface, 0.1);
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    t_long(&mut surface, 7);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    t_long(&mut surface, 42);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.2);
    if let Some(version_value) = version_value {
        t_long(&mut surface, version_value);
    }
    t_dbl(&mut surface, 0.3);
    surface.extend_from_slice(&generated_curve_block());
    for frame in 0..2 {
        for v in [
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
            [-1.0, 1.0, 0.0],
        ] {
            t_vec(&mut surface, v);
        }
        t_dbl(&mut surface, 0.4 + f64::from(frame) * 0.1);
        surface.extend_from_slice(&[0x0b, 0x0a, 0x0b]);
    }
    t_long(&mut surface, 99);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_referenced_t_spl_sur_smbh() -> Vec<u8> {
    let mut bytes = synthetic_mixed_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 8).unwrap();
    let old_offset = records[9].offset;
    let old_len = records[9].len;
    let mut surface = Vec::new();
    t_subident(&mut surface, "spline");
    t_ident(&mut surface, "surface");
    t_ref(&mut surface, -1);
    t_long(&mut surface, -1);
    t_ref(&mut surface, -1);
    let shared_offset = surface.len();
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_subtrans_object");
    t_u16_string(&mut surface, "degree 3\nv 1 0 0 0\n");
    t_u16_string(&mut surface, "100verts 1\n");
    surface.push(0x10);
    surface.push(0x0f);
    t_ident(&mut surface, "t_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    for value in [-2.0, 3.0, -4.0, 5.0] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 7);
    surface.push(0x0f);
    t_ident(&mut surface, "ref");
    let reference_value_offset = surface.len() + 1;
    t_long(&mut surface, 0);
    surface.push(0x10);
    t_long(&mut surface, 9);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old_offset..old_offset + old_len, surface);
    let records = crate::sab::frame(
        &bytes,
        asm_header::record_stream_start(&bytes).unwrap(),
        asm_header::first_delta_state_offset(&bytes).unwrap(),
        8,
    )
    .unwrap();
    let tables = crate::nurbs::subtypes::SubtypeTables::from_records(&records, &bytes);
    let index = tables
        .index_of_offset(8, old_offset + shared_offset)
        .expect("shared T-spline subtype index");
    bytes[old_offset + reference_value_offset..old_offset + reference_value_offset + 8]
        .copy_from_slice(&i64::try_from(index).unwrap().to_le_bytes());
    bytes
}

fn synthetic_explicit_formula_sweep_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 7);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    surface.push(0x0a);
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 1);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.75);
    surface.push(0x0b);
    push_u8_string(&mut surface, "null_law");
    surface.push(0x0a);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_explicit_guide_sweep_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 8);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.25);
    t_dbl(&mut surface, 1.25);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 2);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.5);
    surface.extend_from_slice(&[0x0a, 0x0b]);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    t_long(&mut surface, 11);
    t_long(&mut surface, 12);
    for value in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6] {
        t_dbl(&mut surface, value);
    }
    surface.extend_from_slice(&[0x0a, 0x0b, 0x0a]);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_explicit_surface_sweep_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&2i64.to_le_bytes());
    t_long(&mut surface, 9);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_long(&mut surface, 3);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.25);
    surface.push(0x15);
    surface.extend_from_slice(&1i64.to_le_bytes());
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [1.0, 2.0, 3.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x0a);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_law_driven_sweep_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "sweep_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&5i64.to_le_bytes());
    t_long(&mut surface, 10);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, 0.0);
    t_dbl(&mut surface, 1.0);
    surface.push(0x0b);
    t_pos(&mut surface, [4.0, 5.0, 6.0]);
    for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        t_vec(&mut surface, direction);
    }
    t_dbl(&mut surface, 2.5);
    t_long(&mut surface, 21);
    t_dbl(&mut surface, -1.0);
    t_dbl(&mut surface, 1.0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_long(&mut surface, 22);
    surface.push(0x0a);
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -2.0);
    t_dbl(&mut surface, 3.0);
    t_dbl(&mut surface, 0.75);
    surface.push(0x0b);
    t_vec(&mut surface, [1.0, 2.0, 3.0]);
    t_long(&mut surface, 23);
    push_u8_string(&mut surface, "null_law");
    surface.push(0x0a);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.005);
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_generated_compound_loft_scale(bytes: &mut Vec<u8>) {
    t_long(bytes, 1);
    t_long(bytes, 9);
    bytes.extend_from_slice(&generated_curve_block());
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_pcurve_block());
    bytes.push(0x0b);
    t_long(bytes, -1);
    t_long(bytes, 211);
    t_long(bytes, 4);
    t_long(bytes, 0);
    t_dbl(bytes, -0.25);
    t_dbl(bytes, 0.75);
    bytes.push(0x0a);
    t_vec(bytes, [0.0, 1.0, 0.0]);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 1);
    bytes.extend_from_slice(&generated_curve_block());
    t_long(bytes, 2);
    t_long(bytes, 3);
}

fn synthetic_compound_loft_smbh() -> Vec<u8> {
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
    t_ident(&mut surface, "cl_loft_spl_sur");
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    append_generated_compound_loft_scale(&mut surface);
    surface.push(0x0a);
    surface.push(0x0b);
    t_long(&mut surface, 0);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.push(0x0a);
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_generated_float_array(bytes: &mut Vec<u8>, values: &[f64]) {
    t_long(bytes, i64::try_from(values.len()).unwrap());
    for value in values {
        t_dbl(bytes, *value);
    }
}

fn synthetic_scaled_compound_loft_smbh(full: bool) -> Vec<u8> {
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
    t_ident(&mut surface, "scaled_cloft_spl_sur");
    surface.push(0x15);
    surface.extend_from_slice(&11i64.to_le_bytes());
    if full {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.004);
    } else {
        for value in [-1.0, 2.0, -3.0, 4.0] {
            t_dbl(&mut surface, value);
        }
        append_generated_float_array(&mut surface, &[0.25]);
        append_generated_float_array(&mut surface, &[0.5, 0.75]);
    }
    for values in [&[0.25][..], &[][..], &[][..], &[][..], &[][..], &[][..]] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    append_generated_compound_loft_scale(&mut surface);
    surface.push(0x0a);
    surface.push(0x0b);
    t_long(&mut surface, 0);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    surface.push(0x0b);
    surface.push(0x0a);
    t_long(&mut surface, 2);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 1.0, 0.0]);
    surface.push(0x15);
    surface.extend_from_slice(&12i64.to_le_bytes());
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_skin_spl_sur_smbh(law_case: u8, expanded: bool) -> Vec<u8> {
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
    t_ident(&mut surface, "skin_spl_sur");
    for value in [1i64, 2, 3] {
        surface.push(0x15);
        surface.extend_from_slice(&value.to_le_bytes());
    }
    t_long(&mut surface, 4);
    t_dbl(&mut surface, 0.25);
    t_long(&mut surface, 1);
    if expanded {
        t_long(&mut surface, 9);
        surface.extend_from_slice(&generated_curve_block());
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [1.0, -2.0, 3.0]);
        t_vec(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_pcurve_block());
        surface.push(0x0b);
        t_long(&mut surface, -1);
        t_long(&mut surface, 211);
        t_long(&mut surface, 4);
        t_long(&mut surface, 0);
        t_dbl(&mut surface, -0.5);
        t_dbl(&mut surface, 1.5);
        surface.push(0x0a);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, -1);
        t_long(&mut surface, 7);
    } else {
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, 211);
        t_long(&mut surface, 4);
        t_long(&mut surface, 0);
        t_dbl(&mut surface, -0.5);
        t_dbl(&mut surface, 1.5);
        t_long(&mut surface, -1);
        surface.extend_from_slice(&generated_curve_block());
        t_long(&mut surface, 7);
    }
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_dbl(&mut surface, 0.75);
    if law_case == 1 {
        push_u8_string(&mut surface, "structural-law");
        t_long(&mut surface, 3);
        push_u8_string(&mut surface, "null_law");
        push_u8_string(&mut surface, "TRANS");
        for value in 0..13 {
            t_dbl(&mut surface, f64::from(value) / 10.0);
        }
        for value in [4i64, 5, 6] {
            surface.push(0x15);
            surface.extend_from_slice(&value.to_le_bytes());
        }
        push_u8_string(&mut surface, "EDGE");
        surface.extend_from_slice(&generated_curve_block());
        t_dbl(&mut surface, -0.25);
        t_dbl(&mut surface, 1.25);
    } else if law_case == 2 {
        push_u8_string(&mut surface, "algebraic-law");
        t_long(&mut surface, 2);
        push_u8_string(&mut surface, "SIN");
        push_u8_string(&mut surface, "ABS");
        t_dbl(&mut surface, -2.5);
        push_u8_string(&mut surface, "DOT");
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
    } else {
        push_u8_string(&mut surface, "skin-law");
        t_long(&mut surface, 1);
        push_u8_string(&mut surface, "SPLINE_LAW");
        t_long(&mut surface, 5);
        append_generated_float_array(&mut surface, &[0.0, 0.5, 1.0]);
        append_generated_float_array(&mut surface, &[1.0, 2.0, 3.0]);
        t_pos(&mut surface, [1.0, 2.0, 3.0]);
    }
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.006);
    for values in [
        &[0.1][..],
        &[0.2, 0.3][..],
        &[][..],
        &[][..],
        &[][..],
        &[][..],
    ] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x0a);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_law_spl_sur_smbh(name: &str, legacy_ranges: bool, tail_selector: i64) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    if legacy_ranges {
        for value in [-1.0, 2.0, -3.0, 4.0] {
            t_dbl(&mut surface, value);
        }
    }
    push_u8_string(&mut surface, "primary-law");
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "SET");
    t_dbl(&mut surface, -2.5);
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "aux-law");
    t_long(&mut surface, 1);
    push_u8_string(&mut surface, "TERM");
    t_vec(&mut surface, [1.0, 2.0, 3.0]);
    t_long(&mut surface, 1);
    if !legacy_ranges {
        surface.push(0x15);
        surface.extend_from_slice(&tail_selector.to_le_bytes());
    } else {
        assert_eq!(tail_selector, 0);
    }
    match tail_selector {
        0 => {
            surface.extend_from_slice(&generated_surface_block());
            t_dbl(&mut surface, 0.007);
        }
        1 => {
            append_generated_float_array(&mut surface, &[0.0, 0.5, 1.0]);
            append_generated_float_array(&mut surface, &[-1.0, 1.0]);
            t_dbl(&mut surface, 0.008);
            for value in [0i64, 2, 1, 3] {
                surface.push(0x15);
                surface.extend_from_slice(&value.to_le_bytes());
            }
        }
        2 => {
            for value in [-0.5, 1.5, -2.0, 2.0] {
                t_dbl(&mut surface, value);
            }
            for value in [1i64, 2, 0, 4] {
                surface.push(0x15);
                surface.extend_from_slice(&value.to_le_bytes());
            }
        }
        3 | 4 => {}
        _ => panic!("invalid law tail selector"),
    }
    for values in [
        &[0.1][..],
        &[0.2, 0.3][..],
        &[][..],
        &[][..],
        &[][..],
        &[][..],
    ] {
        append_generated_float_array(&mut surface, values);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn synthetic_sub_spl_sur_smbh(name: &str) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    for value in [-1.0, 2.0, -3.0, 4.0] {
        t_dbl(&mut surface, value);
    }
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.1, -0.2, 0.3]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_generated_g2_side(bytes: &mut Vec<u8>, label: &str) {
    push_u8_string(bytes, label);
    t_ident(bytes, "plane");
    t_pos(bytes, [1.0, -2.0, 3.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&generated_pcurve_block());
    t_vec(bytes, [0.0, 1.0, 0.0]);
    bytes.extend_from_slice(&generated_pcurve_block());
}

fn synthetic_g2_blend_spl_sur_smbh(name: &str, full: bool) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    append_generated_g2_side(&mut surface, "first");
    surface.push(0x15);
    surface.extend_from_slice(&(if full { 11i64 } else { 12i64 }).to_le_bytes());
    if full {
        surface.extend_from_slice(&generated_surface_block());
        t_dbl(&mut surface, 0.002);
    } else {
        for value in 1..=9 {
            t_dbl(&mut surface, f64::from(value));
        }
        t_dbl(&mut surface, 0.003);
        t_long(&mut surface, 44);
        surface.extend_from_slice(&generated_pcurve_block());
    }
    append_generated_g2_side(&mut surface, "second");
    surface.extend_from_slice(&generated_surface_block());
    surface.extend_from_slice(&generated_curve_block());
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    t_long(&mut surface, 8);
    for value in [-1.0, 2.0, -3.0, 4.0, 0.1, 0.2, 0.3, 0.4] {
        t_dbl(&mut surface, value);
    }
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.0095);
    t_long(&mut surface, 1);
    t_dbl(&mut surface, 0.25);
    t_long(&mut surface, 0);
    t_long(&mut surface, 2);
    t_dbl(&mut surface, 0.5);
    t_dbl(&mut surface, 0.75);
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

fn append_generated_rolling_ball_side(bytes: &mut Vec<u8>, label: &str, x: f64) {
    push_u8_string(
        bytes,
        if label == "left" {
            "blend_support_surface"
        } else {
            "blend_support_curve"
        },
    );
    t_ident(bytes, "plane");
    t_pos(bytes, [x, 0.0, 0.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&[0x0b; 4]);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    bytes.extend_from_slice(&generated_pcurve_block());
    t_pos(bytes, [x, 2.0, 3.0]);
    t_ident(bytes, "nullbs");
    t_long(bytes, if label == "left" { 3 } else { 4 });
    t_ident(bytes, "nullbs");
}

fn synthetic_full_rolling_ball_smbh(name: &str) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    t_long(&mut surface, 22507);
    append_generated_rolling_ball_side(&mut surface, "left", 1.0);
    append_generated_rolling_ball_side(&mut surface, "right", 4.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    for value in [-0.3, -0.6] {
        t_dbl(&mut surface, value);
    }
    surface.push(0x15);
    surface.extend_from_slice(&(-1i64).to_le_bytes());
    for value in [-1.0, 2.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    surface.push(0x0b);
    surface.push(0x0b);
    t_long(&mut surface, 1);
    for value in [0.1, 0.2] {
        t_dbl(&mut surface, value);
    }
    t_long(&mut surface, 17);
    push_tagged_i64(&mut surface, 0x15, 0);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [&[0.25][..], &[][..], &[0.5, 0.75][..]] {
        t_long(&mut surface, i64::try_from(values.len()).unwrap());
        for value in values {
            t_dbl(&mut surface, *value);
        }
    }
    if matches!(name, "sss_blend_spl_sur" | "sssblndsur") {
        push_u8_string(&mut surface, "third");
        t_ident(&mut surface, "plane");
        t_pos(&mut surface, [0.0, 0.0, 1.0]);
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        t_vec(&mut surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_curve_block());
        t_ident(&mut surface, "nullbs");
        t_vec(&mut surface, [0.0, 1.0, 0.0]);
        surface.extend_from_slice(&generated_pcurve_block());
        t_long(&mut surface, 23);
        t_ident(&mut surface, "nullbs");
        surface.push(0x0b);
    }
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_generated_variable_blend_side(bytes: &mut Vec<u8>, label: &str, x: f64) {
    push_u8_string(
        bytes,
        if label == "left" {
            "blend_support_surface"
        } else {
            "blendsupcur"
        },
    );
    t_ident(bytes, "plane");
    t_pos(bytes, [x, 0.0, 0.0]);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.extend_from_slice(&[0x0b; 4]);
    bytes.extend_from_slice(&generated_curve_block());
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    bytes.extend_from_slice(&generated_pcurve_block());
    t_pos(bytes, [x, 2.0, 3.0]);
    t_ident(bytes, "nullbs");
    t_long(bytes, if label == "left" { 0 } else { 5 });
    t_ident(bytes, "nullbs");
}

fn append_generated_variable_blend_value(
    bytes: &mut Vec<u8>,
    parameters: [f64; 2],
    radii: [f64; 2],
) {
    push_u8_string(bytes, "two_ends");
    t_long(bytes, 7);
    bytes.push(0x15);
    bytes.extend_from_slice(&3i64.to_le_bytes());
    bytes.push(0x0a);
    for value in parameters.into_iter().chain(radii) {
        t_dbl(bytes, value);
    }
}

fn synthetic_variable_blend_smbh(name: &str) -> Vec<u8> {
    synthetic_variable_blend_smbh_with_selector(name, false, None)
}

fn synthetic_variable_blend_smbh_with_branch(name: &str, rounded_chamfer: bool) -> Vec<u8> {
    synthetic_variable_blend_smbh_with_selector(name, rounded_chamfer, rounded_chamfer.then_some(3))
}

fn synthetic_variable_blend_smbh_with_selector(
    name: &str,
    two_radii: bool,
    chamfer_selector: Option<i64>,
) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    t_long(&mut surface, 23100);
    append_generated_variable_blend_side(&mut surface, "left", 1.0);
    append_generated_variable_blend_side(&mut surface, "right", 4.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    t_dbl(&mut surface, -0.2);
    t_dbl(&mut surface, 0.4);
    surface.push(0x15);
    surface.extend_from_slice(&i64::from(two_radii).to_le_bytes());
    append_generated_variable_blend_value(&mut surface, [0.25, 0.75], [1.5, 2.5]);
    if !two_radii {
        if let Some(selector) = chamfer_selector {
            surface.push(0x15);
            surface.extend_from_slice(&selector.to_le_bytes());
        }
    }
    if two_radii {
        append_generated_variable_blend_value(&mut surface, [0.1, 0.9], [3.5, 4.5]);
        if let Some(selector) = chamfer_selector {
            surface.push(0x15);
            surface.extend_from_slice(&selector.to_le_bytes());
            if selector == 3 {
                surface.push(0x15);
                surface.extend_from_slice(&2i64.to_le_bytes());
                append_generated_variable_blend_value(&mut surface, [0.0, 1.0], [5.5, 6.5]);
            }
        }
    }
    for value in [-1.0, 2.0] {
        surface.push(0x0a);
        t_dbl(&mut surface, value);
    }
    surface.push(0x0b);
    surface.push(0x0b);
    t_long(&mut surface, 11);
    t_dbl(&mut surface, 0.125);
    t_dbl(&mut surface, 0.6);
    t_long(&mut surface, 12);
    push_tagged_i64(&mut surface, 0x15, 0);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
    for values in [
        &[0.125][..],
        &[][..],
        &[0.25, 0.375][..],
        &[][..],
        &[0.5][..],
        &[][..],
    ] {
        t_long(&mut surface, i64::try_from(values.len()).unwrap());
        for value in values {
            t_dbl(&mut surface, *value);
        }
    }
    surface.push(0x0a);
    for value in [31, 32, 33] {
        t_long(&mut surface, value);
    }
    surface.extend_from_slice(&generated_curve_block());
    surface.extend_from_slice(&[0x0b, 0x0b]);
    surface.push(0x0a);
    surface.push(0x0b);
    surface.push(0x0a);
    t_dbl(&mut surface, 0.0);
    surface.push(0x0a);
    t_dbl(&mut surface, 1.0);
    surface.extend_from_slice(&generated_curve_block());
    t_ident(&mut surface, "nullbs");
    surface.push(0x10);
    t_end(&mut surface);
    bytes.splice(old.offset..old.offset + old.len, surface);
    bytes
}

fn append_vertex_boundary_common(bytes: &mut Vec<u8>, kind: &str, x: f64) {
    push_u8_string(bytes, kind);
    bytes.push(0x0a);
    t_pos(bytes, [x, 0.0, 0.0]);
    bytes.push(0x0b);
    bytes.push(0x0a);
    t_dbl(bytes, x + 0.25);
}

fn synthetic_vertex_blend_smbh(name: &str) -> Vec<u8> {
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
    t_ident(&mut surface, name);
    t_long(&mut surface, 4);

    append_vertex_boundary_common(&mut surface, "circle", 1.0);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x15);
    surface.extend_from_slice(&1i64.to_le_bytes());
    t_pos(&mut surface, [2.0, 3.0, 4.0]);
    t_dbl(&mut surface, 0.1);
    t_dbl(&mut surface, 0.9);
    surface.push(0x0b);

    append_vertex_boundary_common(&mut surface, "deg", 2.0);
    t_pos(&mut surface, [5.0, 6.0, 7.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 1.0, 0.0]);

    append_vertex_boundary_common(&mut surface, "pcurve", 3.0);
    t_ident(&mut surface, "plane");
    t_pos(&mut surface, [0.0, 0.0, 0.0]);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_vec(&mut surface, [1.0, 0.0, 0.0]);
    surface.push(0x0b);
    surface.extend_from_slice(&generated_pcurve_block());
    surface.push(0x0a);
    t_dbl(&mut surface, 0.002);

    append_vertex_boundary_common(&mut surface, "plane", 4.0);
    t_vec(&mut surface, [0.0, 0.0, 1.0]);
    t_dbl(&mut surface, -0.5);
    t_dbl(&mut surface, 1.5);
    surface.extend_from_slice(&generated_curve_block());

    t_long(&mut surface, 17);
    t_dbl(&mut surface, 0.003);
    surface.extend_from_slice(&generated_surface_block());
    t_dbl(&mut surface, 0.004);
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
    let vert = |r: &mut Vec<u8>, owning_edge: i64, index_flag: i64, point: i64| {
        t_ident(r, "vertex");
        t_ref(r, -1);
        t_long(r, -1);
        t_ref(r, -1);
        t_ref(r, owning_edge);
        t_long(r, index_flag);
        t_ref(r, point);
        t_end(r);
    };
    vert(&mut r, 16, 0, 25); // 21 A
    vert(&mut r, 16, 1, 26); // 22 B
    vert(&mut r, 17, 1, 27); // 23 C
    vert(&mut r, 19, 1, 28); // 24 D

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

fn set_zip_entry_uncompressed_size(archive: &mut [u8], target: &[u8], size: u32) {
    let central = archive
        .windows(4)
        .enumerate()
        .find_map(|(offset, signature)| {
            if signature != b"PK\x01\x02" || offset + 46 > archive.len() {
                return None;
            }
            let name_length = u16::from_le_bytes(
                archive[offset + 28..offset + 30]
                    .try_into()
                    .expect("central name-length field"),
            ) as usize;
            (archive.get(offset + 46..offset + 46 + name_length) == Some(target)).then_some(offset)
        })
        .expect("generated ZIP central-directory entry");
    archive[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
}

#[test]
fn oversized_zip_entry_declaration_is_rejected_before_allocation() {
    let mut archive = f3d_with_smbh(&synthetic_geometry_smbh());
    let target = b"FusionAssetName[Active]/Breps.BlobParts/Body1.smbh";
    set_zip_entry_uncompressed_size(&mut archive, target, u32::MAX);

    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect_err("oversized inflated entry must be rejected");
    assert!(error.to_string().contains("inflated bytes"));
}

#[test]
fn oversized_nested_protein_entry_is_rejected_before_allocation() {
    let target = b"AssetData/InstanceProperties.bin";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file(std::str::from_utf8(target).unwrap(), stored)
        .unwrap();
    zip.write_all(b"properties").unwrap();
    let mut protein = zip.finish().unwrap().into_inner();
    set_zip_entry_uncompressed_size(&mut protein, target, u32::MAX);

    let error =
        crate::materials::patch_protein_appearances(&protein, &std::collections::BTreeMap::new())
            .expect_err("oversized nested Protein entry must be rejected");
    assert!(error.to_string().contains("inflated bytes"));
}

fn f3d_with_configuration(smbh: &[u8], name: &str, payload: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(b"synthetic-manifest").unwrap();
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.start_file(name, stored).unwrap();
    zip.write_all(payload).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn generated_design_configuration_json_decodes_and_writes_source_less() {
    let name = "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg";
    let payload = br#"{"configurations":{"wide":{"parameters":{"width":"25 mm"},"suppressed":["slot"]}},"active":"wide","extension":{"future":7}}"#;
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                name,
                payload,
            )),
            &DecodeOptions::default(),
        )
        .expect("generated configuration decode");
    let native = f3d_native(&decoded.ir);
    assert_eq!(native.design_configurations.len(), 1);
    assert_eq!(native.design_configurations[0].entry_name, name);
    assert_eq!(
        native.design_configurations[0].id,
        format!("f3d:configuration:entry#{name}")
    );
    assert_eq!(
        native.design_configurations[0].kind,
        crate::records::DesignConfigurationKind::Table
    );
    assert_eq!(native.design_configurations[0].payload["active"], "wide");
    assert_eq!(
        native.design_configurations[0].payload["extension"]["future"],
        7
    );
    assert_eq!(decoded.ir.model.configurations.len(), 1);
    let wide = &decoded.ir.model.configurations[0];
    assert_eq!(wide.name, "wide");
    assert!(wide.active);
    assert_eq!(wide.properties["parameter:width"], "25 mm");
    assert_eq!(wide.properties["suppressed:slot"], "true");
    assert_eq!(
        wide.native_ref.as_deref(),
        Some(native.design_configurations[0].id.as_str())
    );

    let mut retained = decoded.ir.clone();
    update_f3d_native(&mut retained, |native| {
        native.design_configurations[0].payload["active"] = "narrow".into();
        native.design_configurations[0].payload["configurations"]["narrow"] =
            serde_json::json!({"parameters":{"width":"12 mm"},"suppressed":[]});
    });
    retained.model.configurations = crate::design::configurations::project_configurations(
        &f3d_native(&retained).design_configurations,
    );
    let expected_retained = f3d_native(&retained).design_configurations;
    let mut retained_bytes = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(
            &retained,
            &decoded.source_fidelity,
            &mut retained_bytes,
        )
        .expect("retained configuration edit");
    let retained_round_trip = F3dCodec
        .decode(&mut Cursor::new(retained_bytes), &DecodeOptions::default())
        .expect("retained configuration round trip");
    assert_eq!(
        f3d_native(&retained_round_trip.ir).design_configurations,
        expected_retained
    );

    let expected_projected = decoded.ir.model.configurations.clone();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less configuration encode");
    let mut inconsistent = source_less.clone();
    inconsistent.model.configurations[0].active = false;
    let error = F3dCodec
        .encode(&inconsistent, &mut Vec::new())
        .expect_err("neutral/native configuration divergence must be rejected");
    assert!(error
        .to_string()
        .contains("must equal the projection of native configuration tables"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less configuration round trip");
    assert_eq!(
        f3d_native(&round_trip.ir).design_configurations,
        native.design_configurations
    );
    assert_eq!(round_trip.ir.model.configurations, expected_projected);

    let rule_name = "FusionAssetName[Active]/DesignConfigurationRule.456.dsgcfgrule";
    let rule_result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                rule_name,
                br#"{"when":"width > 20 mm","activate":"wide"}"#,
            )),
            &DecodeOptions::default(),
        )
        .expect("generated configuration-rule decode");
    assert!(rule_result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains(
            "configuration rule(s) were retained without an unambiguous neutral activation target"
        )));
    let rule = f3d_native(&rule_result.ir).design_configurations.remove(0);
    assert_eq!(rule.kind, crate::records::DesignConfigurationKind::Rule);
    assert_eq!(rule.payload["activate"], "wide");

    let invalid = F3dCodec.decode(
        &mut Cursor::new(f3d_with_configuration(
            &synthetic_geometry_smbh(),
            name,
            b"[]",
        )),
        &DecodeOptions::default(),
    );
    assert!(matches!(
        invalid,
        Err(cadmpeg_ir::codec::CodecError::Malformed(message))
            if message.contains("configuration JSON must be an object")
    ));

    for (payload, expected) in [
        (
            br#"{"configurations":{"wide":{}},"active":"missing"}"#.as_slice(),
            "is not a named variant",
        ),
        (
            br#"{"configurations":{"wide":{"parameters":[]}}}"#.as_slice(),
            "parameters must be an object",
        ),
        (
            br#"{"configurations":{"wide":{"suppressed":[7]}}}"#.as_slice(),
            "suppressed list must contain strings",
        ),
        (
            br#"{"configurations":{"wide":{"material":7}}}"#.as_slice(),
            "material must be a string",
        ),
    ] {
        let invalid = F3dCodec.decode(
            &mut Cursor::new(f3d_with_configuration(
                &synthetic_geometry_smbh(),
                name,
                payload,
            )),
            &DecodeOptions::default(),
        );
        assert!(matches!(
            invalid,
            Err(cadmpeg_ir::codec::CodecError::Malformed(message))
                if message.contains(expected)
        ));
    }

    let invalid_rule = F3dCodec.decode(
        &mut Cursor::new(f3d_with_configuration(
            &synthetic_geometry_smbh(),
            rule_name,
            br#"{"when":"width > 20 mm"}"#,
        )),
        &DecodeOptions::default(),
    );
    assert!(matches!(
        invalid_rule,
        Err(cadmpeg_ir::codec::CodecError::Malformed(message))
            if message.contains("`when` and `activate` must be paired strings")
    ));
}

#[test]
fn generated_f3d_replays_byte_exactly_and_rejects_semantic_edits() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();

    let mut replayed = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut replayed)
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
        .write_preserved_with_source_fidelity(
            &point_edited,
            &decoded.source_fidelity,
            &mut regenerated,
        )
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
        .write_preserved_with_source_fidelity(&modified, &decoded.source_fidelity, &mut Vec::new())
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.bodies[0].visible = Some(false);
    source_less.model.vertices[0].tolerance = Some(0.025);
    source_less.model.edges[0].tolerance = Some(0.035);
    let tangent_edge = source_less.model.edges[0].id.clone();
    let visible_body = source_less.model.bodies[0].id.clone();
    let tolerant_vertex = source_less.model.vertices[0].id.clone();
    let tolerant_edge = source_less.model.edges[0].id.clone();
    let owner_coedge = source_less.model.coedges[0].id.clone();
    let tolerant_coedge = source_less.model.coedges[1].id.clone();
    {
        let mut native = f3d_native_mut(&mut source_less);
        let metadata = native
            .edge_continuities
            .iter_mut()
            .find(|metadata| metadata.edge == tangent_edge)
            .expect("generated edge continuity");
        metadata.continuity = "tangent".into();
        metadata.sense = cadmpeg_ir::topology::Sense::Reversed;
        native.face_sidedness[0].containment = Some(crate::records::FaceContainment::In);
        native.edge_ownerships[0].owner_coedge = Some(owner_coedge);
        native.tolerant_vertex_tails = vec![crate::records::TolerantVertexTail {
            id: "f3d:asm:tolerant-vertex-tail#generated".into(),
            vertex: tolerant_vertex,
            record_index: 0,
            leading_tolerances: [1.25, -2.5],
        }];
        native.tolerant_edge_tails = vec![crate::records::TolerantEdgeTail {
            id: "f3d:asm:tolerant-edge-tail#generated".into(),
            edge: tolerant_edge,
            record_index: 0,
            trailing_integers: [22800, 0],
        }];
        native.tolerant_coedge_parameters = vec![crate::records::TolerantCoedgeParameters {
            id: "f3d:asm:tolerant-coedge-parameters#generated".into(),
            coedge: tolerant_coedge,
            record_index: 0,
            parameter_range: [0.25, 0.75],
            extension: crate::records::TolerantCoedgeExtension::None,
        }];
        native.body_visibilities = vec![crate::records::BodyVisibility {
            id: "f3d:design:body-visibility#generated".into(),
            body: visible_body,
            stream: "FusionAssetName[Active]/Design1/BulkStream.dat".into(),
            byte_offset: 0,
            asm_body_key_offset: 0,
            asm_body_key: 42,
            entity_suffix: 42,
            visible: false,
        }];
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less F3D encode");
    let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
    let mut properties = Vec::new();
    archive
        .by_name("Properties.dat")
        .expect("generated Properties.dat")
        .read_to_end(&mut properties)
        .expect("generated properties bytes");
    assert_eq!(properties, 0u32.to_le_bytes());
    let mut smbh = Vec::new();
    archive
        .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
        .expect("generated BREP stream")
        .read_to_end(&mut smbh)
        .expect("generated BREP bytes");
    let record_start = smbh
        .windows(b"\x0d\x09asmheader".len())
        .position(|window| window == b"\x0d\x09asmheader")
        .expect("generated ASM record table");
    let records = crate::sab::frame(&smbh, record_start, smbh.len(), 8)
        .expect("generated ASM records must frame");
    let point_records = records
        .iter()
        .filter(|record| record.head == "point")
        .collect::<Vec<_>>();
    assert_eq!(point_records.len(), 3);
    assert!(point_records
        .iter()
        .all(|record| record.len == 60 && record.tokens.len() == 4));
    assert_eq!(
        records
            .iter()
            .filter(|record| record.head == "tcoedge")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.head == "tedge")
            .count(),
        1
    );
    drop(archive);
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less F3D round trip");

    {
        let mut invalid = source_less.clone();
        f3d_native_mut(&mut invalid).face_sidedness[0].normalized_sense =
            match source_less.model.faces[0].sense {
                cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
                cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
            };
        let error = F3dCodec
            .encode(&invalid, &mut Vec::new())
            .expect_err("stale normalized face sense must not be rewritten");
        assert!(error
            .to_string()
            .contains("normalized sense conflicts with face"));
    }
    {
        let mut invalid = source_less.clone();
        f3d_native_mut(&mut invalid).body_visibilities[0].asm_body_key = 43;
        let error = F3dCodec
            .encode(&invalid, &mut Vec::new())
            .expect_err("visibility must rejoin the emitted ASM body");
        assert!(error
            .to_string()
            .contains("uses an ASM key different from body"));
    }

    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(
        f3d_native(&round_trip.ir).body_native_keys[0].asm_body_key,
        Some(42)
    );
    assert_eq!(round_trip.ir.model.bodies[0].visible, Some(false));
    assert_eq!(f3d_native(&round_trip.ir).body_visibilities.len(), 1);
    assert!(!f3d_native(&round_trip.ir).body_visibilities[0].visible);
    assert_eq!(
        f3d_native(&round_trip.ir).body_visibilities[0].id,
        "f3d:FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh:body-visibility#42"
    );
    assert_eq!(
        round_trip.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(round_trip.ir.model.faces.len(), 1);
    assert_eq!(round_trip.ir.model.loops.len(), 1);
    assert_eq!(round_trip.ir.model.coedges.len(), 3);
    assert_eq!(round_trip.ir.model.edges.len(), 3);
    assert_eq!(round_trip.ir.model.vertices.len(), 3);
    assert_eq!(round_trip.ir.model.vertices[0].tolerance, Some(0.025));
    assert_eq!(round_trip.ir.model.edges[0].tolerance, Some(0.035));
    assert_eq!(
        f3d_native(&round_trip.ir).tolerant_edge_tails[0].trailing_integers,
        [22800, 0]
    );
    assert_eq!(
        f3d_native(&round_trip.ir).tolerant_vertex_tails[0].leading_tolerances,
        [1.25, -2.5]
    );
    assert_eq!(
        f3d_native(&round_trip.ir).tolerant_coedge_parameters[0].parameter_range,
        [0.25, 0.75]
    );
    let ownerships = f3d_native(&round_trip.ir).vertex_ownerships;
    assert_eq!(ownerships.len(), 3);
    assert_eq!(
        ownerships
            .iter()
            .map(|metadata| metadata.endpoint_index)
            .collect::<Vec<_>>(),
        [0, 1, 0]
    );
    let continuities = f3d_native(&round_trip.ir).edge_continuities;
    assert_eq!(continuities.len(), 3);
    assert_eq!(continuities[0].continuity, "tangent");
    assert_eq!(continuities[0].sense, cadmpeg_ir::topology::Sense::Reversed);
    assert_eq!(
        f3d_native(&round_trip.ir).edge_ownerships[0].owner_coedge,
        Some(round_trip.ir.model.coedges[0].id.clone())
    );
    assert!(continuities[1..]
        .iter()
        .all(|metadata| metadata.continuity == "unknown"));
    assert_eq!(
        f3d_native(&round_trip.ir).face_sidedness[0].containment,
        Some(crate::records::FaceContainment::In)
    );
    assert_eq!(round_trip.ir.model.points, source_less.model.points);
    assert_eq!(round_trip.ir.model.surfaces, source_less.model.surfaces);

    let mut edited = round_trip.ir;
    edited.model.bodies[0].visible = Some(true);
    edited.model.vertices[0].tolerance = Some(0.05);
    edited.model.edges[0].tolerance = Some(0.06);
    {
        let mut native = f3d_native_mut(&mut edited);
        native.body_native_keys[0].asm_body_key = Some(84);
        native.face_sidedness[0].containment = Some(crate::records::FaceContainment::Out);
        native.tolerant_vertex_tails[0].leading_tolerances = [3.5, -4.5];
    }
    let mut retained = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &round_trip.source_fidelity, &mut retained)
        .expect("retained double-sided containment edit");
    let retained = F3dCodec
        .decode(&mut Cursor::new(retained), &DecodeOptions::default())
        .expect("retained double-sided containment round trip");
    assert_eq!(
        f3d_native(&retained.ir).face_sidedness[0].containment,
        Some(crate::records::FaceContainment::Out)
    );
    assert_eq!(retained.ir.model.vertices[0].tolerance, Some(0.05));
    assert_eq!(retained.ir.model.edges[0].tolerance, Some(0.06));
    assert_eq!(
        f3d_native(&retained.ir).tolerant_edge_tails[0].trailing_integers,
        [22800, 0]
    );
    assert_eq!(retained.ir.model.bodies[0].visible, Some(true));
    assert_eq!(
        f3d_native(&retained.ir).body_native_keys[0].asm_body_key,
        Some(84)
    );
    assert_eq!(
        f3d_native(&retained.ir).body_visibilities[0].asm_body_key,
        84
    );
    assert!(f3d_native(&retained.ir).body_visibilities[0].visible);
    assert_eq!(
        f3d_native(&retained.ir).tolerant_vertex_tails[0].leading_tolerances,
        [3.5, -4.5]
    );
}

#[test]
fn generated_source_less_f3d_rejects_subds() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.subds.push(cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("test:f3d:subd#0".into()),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    });

    let error = F3dCodec.encode(&source_less, &mut Vec::new()).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(message)
            if message.contains("does not support SubD surfaces")
    ));
}

#[test]
fn generated_source_less_f3d_rejects_unbacked_design_parameters() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less
        .model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: cadmpeg_ir::features::ParameterId("test:f3d:parameter#0".into()),
            owner: None,
            ordinal: 0,
            name: "Width".into(),
            expression: "60 mm".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::features::Length(60.0),
            )),
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });

    let error = F3dCodec.encode(&source_less, &mut Vec::new()).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::Malformed(message)
            if message.contains("must equal the projection")
    ));
}

#[test]
fn generated_source_less_f3d_writes_document_design_parameters() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let native_id = format!("f3d:{stream}:design-parameter#0");
    f3d_native_mut(&mut source_less)
        .design_parameters
        .push(crate::records::DesignParameter {
            id: native_id.clone(),
            byte_offset: 0,
            class_tag: "305".into(),
            record_index: 700,
            prefix_value: 0,
            prefix_value_offset: 22,
            source_ordinal: 0,
            owner_record_index: None,
            expression: "Width / 2".into(),
            expression_offset: 36,
            source_kind: "User Parameter".into(),
            source_kind_offset: 70,
            kind: crate::records::DesignParameterKind::User,
            unit: Some("mm".into()),
            unit_offset: Some(110),
            name: "HalfWidth".into(),
            name_offset: 120,
            evaluated_value: 3.0,
            evaluated_value_offset: 150,
        });
    f3d_native_mut(&mut source_less)
        .design_parameters
        .push(crate::records::DesignParameter {
            id: format!("f3d:{stream}:design-parameter#1"),
            byte_offset: 0,
            class_tag: "305".into(),
            record_index: 701,
            prefix_value: 0,
            prefix_value_offset: 22,
            source_ordinal: 1,
            owner_record_index: None,
            expression: "60 mm".into(),
            expression_offset: 36,
            source_kind: "User Parameter".into(),
            source_kind_offset: 70,
            kind: crate::records::DesignParameterKind::User,
            unit: Some("mm".into()),
            unit_offset: Some(110),
            name: "Width".into(),
            name_offset: 120,
            evaluated_value: 6.0,
            evaluated_value_offset: 150,
        });
    let (_, parameters) = crate::design::feature_project::project_parameter_design(
        &f3d_native(&source_less).design_parameters,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    source_less.model.parameters = parameters;

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less document parameter encode");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less document parameter round trip");
    let mut round_trip_parameters = decoded.ir.model.parameters.clone();
    let mut expected_parameters = source_less.model.parameters.clone();
    for parameter in &mut round_trip_parameters {
        parameter.native_ref = None;
    }
    for parameter in &mut expected_parameters {
        parameter.native_ref = None;
    }
    assert_eq!(round_trip_parameters, expected_parameters);
    assert_eq!(f3d_native(&decoded.ir).design_parameters.len(), 2);
    assert_eq!(
        decoded.ir.model.parameters[0].dependencies,
        [cadmpeg_ir::features::ParameterId(format!(
            "f3d:model:parameter#{}:f3d:{stream}1",
            format!("f3d:{stream}").len(),
        ))]
    );
    assert_eq!(
        f3d_native(&decoded.ir).design_parameters[0].evaluated_value,
        3.0
    );
}

#[test]
fn generated_source_less_writes_document_tolerance_contract() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
fn generated_source_less_preserves_supported_topology_tolerances_or_refuses_loss() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    source_less.model.faces[0].tolerance = Some(0.02);
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("face tolerance must not disappear");
    assert!(
        error.to_string().contains("cannot serialize face")
            && error.to_string().contains("tolerance losslessly")
    );

    source_less.model.faces[0].tolerance = None;
    source_less.model.edges[0].tolerance = Some(0.03);
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("supported tolerant edge encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("supported tolerant edge round trip");
    assert_eq!(round_trip.ir.model.edges[0].tolerance, Some(0.03));

    source_less.model.edges[0].tolerance = None;
    source_less.model.vertices[0].tolerance = Some(0.04);
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("supported tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("supported tolerant vertex round trip");
    assert_eq!(round_trip.ir.model.vertices[0].tolerance, Some(0.04));
}

#[test]
fn generated_source_less_refuses_auxiliary_geometry_and_source_identity_loss() {
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::tessellation::Tessellation;
    use cadmpeg_ir::SourceObjectAssociation;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let association = SourceObjectAssociation {
        format: "generated".into(),
        object_id: "object-1".into(),
        name: Some("exact carrier".into()),
        color: None,
        visible: Some(true),
        layer: None,
        instance_path: Vec::new(),
    };

    source_less.model.surfaces[0].source_object = Some(association.clone());
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("surface source identity must not disappear");
    assert!(error
        .to_string()
        .contains("source-object association on surface"));

    source_less.model.surfaces[0].source_object = None;
    source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: "generated:associated-curve#0".into(),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: Some(association),
    });
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("curve source identity must not disappear");
    assert!(error
        .to_string()
        .contains("source-object association on curve"));

    source_less.model.curves.pop();
    source_less.model.tessellations.push(Tessellation {
        id: "generated:tessellation#0".into(),
        source_object: None,
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        channels: Vec::new(),
    });
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("neutral tessellation must not disappear");
    assert!(error
        .to_string()
        .contains("cannot serialize neutral tessellation"));
}

#[test]
fn generated_source_less_rejects_body_kind_that_conflicts_with_incidence() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    let point_id = PointId("generated:point#3".into());
    source_less.model.points.push(cadmpeg_ir::topology::Point {
        id: point_id.clone(),
        position: cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
        source_object: None,
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
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();

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
            source_object: None,
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    source_less.model.edges[0].param_range = Some([0.25, 1.75]);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less circle-carrier encode");
    let mut round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less circle-carrier round trip");
    assert_eq!(round_trip.ir.model.curves[0].geometry, expected);
    assert_eq!(round_trip.ir.model.edges[0].param_range, Some([0.25, 1.75]));
    assert!(round_trip.ir.model.edges[0].curve.is_some());
    assert!(!cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == cadmpeg_ir::Check::Annotations));
    round_trip.ir.model.curves[0].geometry = CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };
    let error = F3dCodec
        .write_preserved_with_source_fidelity(
            &round_trip.ir,
            &round_trip.source_fidelity,
            &mut Vec::new(),
        )
        .expect_err("native ellipse record cannot silently retain a line edit");
    assert!(error
        .to_string()
        .contains("does not support edits to curve"));
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
        source_object: None,
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
    assert!(!cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == cadmpeg_ir::Check::Annotations));
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
fn generated_source_less_closed_cylinder_band_keeps_compact_periodic_topology() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };

    let mut source_less = CadIr::empty(Default::default());
    let body = BodyId("synthetic:cylinder-band:body#0".into());
    let region = RegionId("synthetic:cylinder-band:region#0".into());
    let shell = ShellId("synthetic:cylinder-band:shell#0".into());
    let face = FaceId("synthetic:cylinder-band:face#0".into());
    let surface = SurfaceId("synthetic:cylinder-band:surface#0".into());
    let loops = [
        LoopId("synthetic:cylinder-band:loop#bottom".into()),
        LoopId("synthetic:cylinder-band:loop#top".into()),
    ];
    let coedges = [
        CoedgeId("synthetic:cylinder-band:coedge#bottom".into()),
        CoedgeId("synthetic:cylinder-band:coedge#top".into()),
    ];
    let edges = [
        EdgeId("synthetic:cylinder-band:edge#bottom".into()),
        EdgeId("synthetic:cylinder-band:edge#top".into()),
    ];
    let curves = [
        CurveId("synthetic:cylinder-band:curve#bottom".into()),
        CurveId("synthetic:cylinder-band:curve#top".into()),
    ];
    let vertices = [
        VertexId("synthetic:cylinder-band:vertex#bottom".into()),
        VertexId("synthetic:cylinder-band:vertex#top".into()),
    ];
    let points = [
        PointId("synthetic:cylinder-band:point#bottom".into()),
        PointId("synthetic:cylinder-band:point#top".into()),
    ];

    source_less.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region.clone()],
        transform: None,
        name: Some("closed cylinder band".into()),
        color: None,
        visible: None,
    });
    source_less.model.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    source_less.model.shells.push(Shell {
        id: shell.clone(),
        region,
        faces: vec![face.clone()],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    source_less.model.faces.push(Face {
        id: face.clone(),
        shell,
        surface: surface.clone(),
        sense: Sense::Forward,
        loops: loops.to_vec(),
        name: None,
        color: None,
        tolerance: None,
    });
    source_less.model.surfaces.push(Surface {
        id: surface,
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    });
    for index in 0..2 {
        let z = index as f64 * 10.0;
        source_less.model.loops.push(Loop {
            id: loops[index].clone(),
            face: face.clone(),
            coedges: vec![coedges[index].clone()],
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            vertex_uses: Vec::new(),
        });
        source_less.model.coedges.push(Coedge {
            id: coedges[index].clone(),
            owner_loop: loops[index].clone(),
            edge: edges[index].clone(),
            next: coedges[index].clone(),
            previous: coedges[index].clone(),
            radial_next: coedges[index].clone(),
            sense: if index == 0 {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
        });
        source_less.model.edges.push(Edge {
            id: edges[index].clone(),
            curve: Some(curves[index].clone()),
            start: vertices[index].clone(),
            end: vertices[index].clone(),
            param_range: Some([-std::f64::consts::PI, std::f64::consts::PI]),
            tolerance: None,
        });
        source_less.model.curves.push(Curve {
            id: curves[index].clone(),
            geometry: CurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, z),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 5.0,
            },
            source_object: None,
        });
        source_less.model.vertices.push(Vertex {
            id: vertices[index].clone(),
            point: points[index].clone(),
            tolerance: None,
        });
        source_less.model.points.push(Point {
            id: points[index].clone(),
            position: Point3::new(-5.0, 0.0, z),
            source_object: None,
        });
    }
    source_less.finalize();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less closed cylinder band encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less closed cylinder band round trip");

    assert_eq!(round_trip.ir.model.faces.len(), 1);
    assert_eq!(round_trip.ir.model.loops.len(), 2);
    assert_eq!(round_trip.ir.model.coedges.len(), 2);
    assert_eq!(round_trip.ir.model.edges.len(), 2);
    assert!(
        round_trip.ir.model.edges.iter().all(|edge| {
            edge.start == edge.end
                && edge.param_range.is_some_and(|range| {
                    (range[0] + std::f64::consts::PI).abs() < 1.0e-12
                        && (range[1] - std::f64::consts::PI).abs() < 1.0e-12
                })
        }),
        "{:?}",
        round_trip.ir.model.edges
    );
    assert!(round_trip.ir.model.loops.iter().all(|loop_| {
        loop_.coedges.len() == 1
            && round_trip
                .ir
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == loop_.coedges[0])
                .is_some_and(|coedge| {
                    coedge.next == coedge.id
                        && coedge.previous == coedge.id
                        && coedge.radial_next == coedge.id
                })
    }));
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = SurfaceGeometry::Cone {
        origin: cadmpeg_ir::math::Point3::new(1.0, 3.0, -5.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 9.0,
        ratio: 1.0,
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
fn generated_f3d_rewrites_cone_ratio_and_half_angle() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated planar triangle decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.surfaces[0].geometry = SurfaceGeometry::Cone {
        origin: cadmpeg_ir::math::Point3::new(1.0, 3.0, -5.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 9.0,
        ratio: 0.6,
        half_angle: 0.5,
    };

    let mut initial = Vec::new();
    F3dCodec
        .encode(&source_less, &mut initial)
        .expect("source-less cone encode");
    let retained_decode = F3dCodec
        .decode(&mut Cursor::new(initial), &DecodeOptions::default())
        .expect("generated cone decode");
    let mut retained = retained_decode.ir;
    let SurfaceGeometry::Cone {
        ratio, half_angle, ..
    } = &mut retained.model.surfaces[0].geometry
    else {
        panic!("expected cone")
    };
    *ratio = 0.4;
    *half_angle = 0.35;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(
            &retained,
            &retained_decode.source_fidelity,
            &mut regenerated,
        )
        .expect("cone ratio regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated cone decode");
    assert!(matches!(
        round_trip.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Cone {
            ratio: 0.4,
            half_angle,
            ..
        } if (half_angle - 0.35).abs() < 1.0e-12
    ));
}

#[test]
fn generated_f3d_rewrites_plane_frame() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated planar triangle decode");
    let mut edited = decoded.ir.clone();
    let expected = SurfaceGeometry::Plane {
        origin: cadmpeg_ir::math::Point3::new(10.0, -20.0, 30.0),
        normal: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };
    edited.model.surfaces[0].geometry = expected.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("plane frame regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated plane decode");
    assert_eq!(round_trip.ir.model.surfaces[0].geometry, expected);
}

#[test]
fn generated_f3d_rejects_analytic_surface_family_changes() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated planar triangle decode");
    let mut edited = decoded.ir.clone();
    edited.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut Vec::new())
        .expect_err("native plane record cannot silently retain a sphere edit");
    assert!(error
        .to_string()
        .contains("does not support edits to surface"));
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
        source_object: None,
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
        round_trip.ir.model.pcurves[0].native_tail_flags,
        expected.native_tail_flags
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
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    let pcurve_coedge = round_trip
        .ir
        .model
        .coedges
        .iter()
        .find(|coedge| !coedge.pcurves.is_empty())
        .expect("generated coedge with pcurve");
    assert!(pcurve_coedge
        .pcurves
        .first()
        .is_some_and(|use_| use_.parameter_range.is_some()));
    assert!(crate::validate::validate_native(&round_trip.ir).is_empty());
}

#[test]
fn generated_source_less_face_lowers_line_pcurve_exactly() {
    use cadmpeg_ir::geometry::PcurveGeometry;
    use cadmpeg_ir::math::Point2;

    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated inline pcurve decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let pcurve = &mut source_less.model.pcurves[0];
    pcurve.geometry = PcurveGeometry::Line {
        origin: Point2::new(2.0, -1.0),
        direction: Point2::new(0.5, 2.0),
    };
    pcurve.parameter_range = Some([-2.0, 3.0]);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less line pcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less line pcurve round trip");
    assert_eq!(
        round_trip.ir.model.pcurves[0].parameter_range,
        Some([-2.0, 3.0])
    );
    assert_eq!(
        round_trip.ir.model.pcurves[0].geometry,
        PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![-2.0, -2.0, 3.0, 3.0],
            control_points: vec![Point2::new(1.0, -5.0), Point2::new(3.5, 5.0)],
            weights: None,
            periodic: false,
        }
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    assert_eq!(actual.native_tail_flags, expected.native_tail_flags);
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
        source_object: None,
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    let loop_id = LoopId("generated:loop#1".into());
    let mut coedge_ids = Vec::new();
    let coordinates = [[2.0, 2.0, 0.0], [4.0, 2.0, 0.0], [2.0, 4.0, 0.0]];
    for (index, [x, y, z]) in coordinates.into_iter().enumerate() {
        let point_id = PointId(format!("generated:inner_point#{index}"));
        source_less.model.points.push(cadmpeg_ir::topology::Point {
            id: point_id.clone(),
            position: cadmpeg_ir::math::Point3::new(x, y, z),
            source_object: None,
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
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
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
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
        coedges: coedge_ids,
        vertex_uses: Vec::new(),
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();

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
        source_object: None,
    });
    source_less.model.edges[0].curve = Some(curve_id);
    let pcurve_id = PcurveId("generated:pcurve#0".into());
    let mut pcurve = pcurve;
    pcurve.id = pcurve_id.clone();
    let expected_pcurve = pcurve.geometry.clone();
    source_less.model.pcurves.push(pcurve);
    source_less.model.coedges[0].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: pcurve_id,
        isoparametric: None,
        parameter_range: None,
    }];

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
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
fn generated_source_less_unit_cube_writes_closed_shared_edge_shell() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let tolerant_coedge = source_less.model.coedges[7].id.clone();
    f3d_native_mut(&mut source_less).tolerant_coedge_parameters =
        vec![crate::records::TolerantCoedgeParameters {
            id: "f3d:asm:tolerant-coedge-parameters#cube".into(),
            coedge: tolerant_coedge,
            record_index: 0,
            parameter_range: [-1.5, 2.25],
            extension: crate::records::TolerantCoedgeExtension::None,
        }];
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less unit cube encode");
    {
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).unwrap();
        let mut stream = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
            .unwrap()
            .read_to_end(&mut stream)
            .unwrap();
        let records = crate::sab::frame(&stream, 47, stream.len(), 8).unwrap();
        let tolerant = records
            .iter()
            .find(|record| record.head == "tcoedge")
            .expect("canonical tolerant coedge record");
        assert!(matches!(
            tolerant.chunk(13),
            Some(crate::sab::Token::Ref(-1))
        ));
        assert!(matches!(
            tolerant.chunk(14),
            Some(crate::sab::Token::Long(0))
        ));
        assert!(matches!(
            tolerant.chunk(15),
            Some(crate::sab::Token::Long(0))
        ));
    }
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less unit cube round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir.model.bodies[0].name.as_deref(),
        source_less.model.bodies[0].name.as_deref()
    );
    assert_eq!(
        round_trip.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
    assert_eq!(round_trip.ir.model.regions.len(), 1);
    assert_eq!(round_trip.ir.model.shells.len(), 1);
    assert_eq!(round_trip.ir.model.faces.len(), 6);
    assert_eq!(
        round_trip
            .ir
            .model
            .faces
            .iter()
            .map(|face| face.name.as_deref())
            .collect::<Vec<_>>(),
        source_less
            .model
            .faces
            .iter()
            .map(|face| face.name.as_deref())
            .collect::<Vec<_>>()
    );
    assert_eq!(round_trip.ir.model.loops.len(), 6);
    assert_eq!(round_trip.ir.model.coedges.len(), 24);
    assert_eq!(round_trip.ir.model.edges.len(), 12);
    assert_eq!(round_trip.ir.model.vertices.len(), 8);
    assert_eq!(round_trip.ir.model.points.len(), 8);
    assert_eq!(
        f3d_native(&round_trip.ir).tolerant_coedge_parameters[0].parameter_range,
        [-1.5, 2.25]
    );
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
        source_object: None,
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 8.0,
        ratio: 1.0,
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
        source_object: None,
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = source_less.model.procedural_surfaces[0].clone();
    let directrix_id = match &expected.definition {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { directrix, .. } => {
            directrix.clone()
        }
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == directrix_id)
        .expect("extrusion directrix")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(5.0, 10.0, -5.0),
        direction: cadmpeg_ir::math::Vector3::new(2.0, -4.0, 1.0),
    };

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
        parameter_interval,
        native_position,
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
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [0.25, 0.25, 0.75, 0.75]
                && curve.control_points == [
                    cadmpeg_ir::math::Point3::new(5.5, 9.0, -4.75),
                    cadmpeg_ir::math::Point3::new(6.5, 7.0, -4.25),
                ]
    ));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
}

#[test]
fn generated_cacheless_translational_extrusion_retains_exact_construction() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cacheless_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated cache-less extrusion decode");

    assert_eq!(decoded.ir.model.procedural_surfaces.len(), 1);
    let procedural = &decoded.ir.model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance, None);
    let ProceduralSurfaceDefinition::Extrusion {
        directrix,
        direction,
        parameter_interval,
        native_position,
    } = &procedural.definition
    else {
        panic!("expected extrusion definition")
    };
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
    let directrix_geometry = decoded
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .map(|curve| &curve.geometry);
    assert!(
        matches!(directrix_geometry, Some(CurveGeometry::Nurbs(_))),
        "unexpected extrusion directrix: {directrix_geometry:?}"
    );
    let u = 0.5;
    let v = 0.25;
    let directrix_point =
        cadmpeg_ir::eval::curve_point(directrix_geometry.expect("typed extrusion directrix"), u)
            .expect("directrix evaluation");
    let surface_geometry = decoded
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .map(|surface| &surface.geometry)
        .expect("extrusion surface carrier");
    let surface_point = cadmpeg_ir::eval::model_surface_point(&decoded.ir, surface_geometry, u, v)
        .expect("procedural extrusion evaluation");
    assert_eq!(surface_point.x, directrix_point.x + v * direction.x);
    assert_eq!(surface_point.y, directrix_point.y + v * direction.y);
    assert_eq!(surface_point.z, directrix_point.z + v * direction.z);
    assert!(matches!(
        decoded
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Procedural { construction }) if *construction == procedural.id
    ));

    let expected_definition = procedural.definition.clone();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less cache-less extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-less extrusion round trip");
    assert_eq!(round_trip.ir.model.procedural_surfaces.len(), 1);
    assert_eq!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        expected_definition
    );
    assert_eq!(
        round_trip.ir.model.procedural_surfaces[0].cache_fit_tolerance,
        None
    );
    assert!(matches!(
        round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == round_trip.ir.model.procedural_surfaces[0].surface)
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Procedural { construction })
            if *construction == round_trip.ir.model.procedural_surfaces[0].id
    ));

    source_less.model.procedural_surfaces[0].cache_fit_tolerance = Some(0.01);
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("cache-less extrusion tolerance must be rejected");
    assert!(error
        .to_string()
        .contains("cache-less F3D extrusion cannot carry a cache-fit tolerance"));
}

#[test]
fn generated_cacheless_circle_extrusion_decodes_as_analytic_cylinder() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cacheless_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated cache-less extrusion decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Extrusion {
        directrix,
        parameter_interval,
        direction,
        ..
    } = &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!("expected extrusion definition")
    };
    *parameter_interval = Some([0.0, std::f64::consts::TAU]);
    *direction = Vector3::new(0.0, 0.0, -20.0);
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == *directrix)
        .expect("extrusion directrix")
        .geometry = CurveGeometry::Circle {
        center: Point3::new(2.0, 3.0, 4.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less circle extrusion encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less circle extrusion round trip");
    let surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == round_trip.ir.model.procedural_surfaces[0].surface)
        .expect("extrusion carrier");
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    } = surface.geometry
    else {
        panic!("unexpected extrusion carrier: {:?}", surface.geometry)
    };
    assert!((origin.x - 2.0).abs() < 1.0e-12);
    assert!((origin.y - 3.0).abs() < 1.0e-12);
    assert!((origin.z - 4.0).abs() < 1.0e-12);
    assert_eq!(axis, Vector3::new(0.0, 0.0, -1.0));
    assert!((ref_direction.x - 1.0).abs() < 1.0e-12);
    assert!(ref_direction.y.abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
    assert!((radius - 5.0).abs() < 1.0e-12);
}

#[test]
fn generated_source_less_writes_rolling_ball_blend_definition() {
    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let supports = match &source_less.model.procedural_surfaces[0].definition {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend { supports, .. } => {
            supports.each_ref().map(|support| {
                support
                    .as_ref()
                    .expect("rolling-ball support")
                    .surface
                    .clone()
            })
        }
        _ => panic!("expected rolling-ball definition"),
    };
    let spine = match &source_less.model.procedural_surfaces[0].definition {
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend { spine, .. } => {
            spine.clone().expect("rolling-ball spine")
        }
        _ => unreachable!(),
    };
    let support_geometries = [
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        },
        cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
            center: cadmpeg_ir::math::Point3::new(10.0, -5.0, 2.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius: 7.5,
        },
    ];
    for (support, geometry) in supports.iter().zip(&support_geometries) {
        source_less
            .model
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == *support)
            .expect("rolling-ball support carrier")
            .geometry = geometry.clone();
    }
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .expect("rolling-ball spine carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(-2.0, 4.0, 1.0),
        direction: cadmpeg_ir::math::Vector3::new(3.0, -1.0, 2.0),
    };
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
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend {
        supports, spine, ..
    } = &actual.definition
    else {
        unreachable!()
    };
    for (support, expected) in supports.iter().zip(support_geometries) {
        let support = support.as_ref().expect("round-trip rolling-ball support");
        let actual = round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == support.surface)
            .expect("round-trip rolling-ball support carrier");
        assert_eq!(actual.geometry, expected);
    }
    let spine = spine.as_ref().expect("round-trip rolling-ball spine");
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *spine)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [0.0, 0.0, 1.0, 1.0]
                && curve.control_points == [
                    cadmpeg_ir::math::Point3::new(-2.0, 4.0, 1.0),
                    cadmpeg_ir::math::Point3::new(1.0, 3.0, 3.0),
                ]
    ));
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
    let hints = &f3d_native(&round_trip.ir).transform_hints[0];
    assert!(hints.rotation);
    assert!(!hints.reflection);
    assert!(!hints.shear);
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
    use crate::records::{
        CreationTimestamp, PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
    };
    use cadmpeg_ir::attributes::AttributeTarget;
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
    let vertex_id = source_less.model.vertices[0].id.clone();
    let mut native = f3d_native_mut(&mut source_less);
    native.persistent_design_links = vec![
        PersistentDesignLink {
            id: "generated:persistent-design-link#0".into(),
            target: AttributeTarget::Body(body_id.clone()),
            design_id: "311".into(),
            entity_kind: 3,
            design_reference: 7,
            ordinal: 0,
            is_current: false,
        },
        PersistentDesignLink {
            id: "generated:persistent-design-link#1".into(),
            target: AttributeTarget::Body(body_id.clone()),
            design_id: "322".into(),
            entity_kind: 3,
            design_reference: 8,
            ordinal: 1,
            is_current: true,
        },
    ];
    native.persistent_subentity_tags = vec![
        PersistentSubentityTag {
            id: "generated:persistent-subentity-tag#0".into(),
            target: AttributeTarget::Face(face_id.clone()),
            selector: 1,
            token: "8".into(),
            design_references: vec![301, -314, 411],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "generated:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Edge(edge_id.clone()),
            selector: 2,
            token: "-1".into(),
            design_references: vec![511],
            ordinal: 0,
        },
    ];
    native.sketch_curve_links = vec![SketchCurveLink {
        id: "generated:sketch-curve-link#0".into(),
        coedge: coedge_id.clone(),
        sketch_curve_id: 113,
        signed_reference: Some(1),
        role: 2,
        closure: 3,
    }];
    native.creation_timestamps = [
        (AttributeTarget::Body(body_id), 1_579_392_000_000_001.0),
        (AttributeTarget::Face(face_id), 1_579_392_000_000_002.0),
        (AttributeTarget::Edge(edge_id), 1_579_392_000_000_003.0),
        (AttributeTarget::Coedge(coedge_id), 1_579_392_000_000_004.0),
        (AttributeTarget::Vertex(vertex_id), 1_579_392_000_000_005.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, (target, unix_microseconds))| CreationTimestamp {
        id: format!("generated:creation-timestamp#{ordinal}"),
        target,
        record_index: 0,
        unix_microseconds,
    })
    .collect();

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less provenance attribute encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less provenance attribute round trip");
    let native = f3d_native(&round_trip.ir);
    assert_eq!(native.persistent_design_links.len(), 2);
    assert_eq!(native.persistent_design_links[0].design_id, "311");
    assert_eq!(native.persistent_design_links[0].entity_kind, 3);
    assert_eq!(native.persistent_design_links[0].design_reference, 7);
    assert_eq!(native.persistent_design_links[1].design_id, "322");
    assert_eq!(native.persistent_design_links[1].design_reference, 8);
    assert!(native.persistent_design_links[1].is_current);
    assert_eq!(native.persistent_subentity_tags.len(), 2);
    assert!(native.persistent_subentity_tags.iter().any(|tag| {
        tag.design_references == [301, -314, 411] && matches!(tag.target, AttributeTarget::Face(_))
    }));
    assert!(crate::validate::validate_native(&round_trip.ir).is_empty());
    assert!(native.persistent_subentity_tags.iter().any(|tag| {
        tag.token == "-1"
            && tag.design_references == [511]
            && matches!(tag.target, AttributeTarget::Edge(_))
    }));
    assert_eq!(native.sketch_curve_links.len(), 1);
    assert_eq!(native.sketch_curve_links[0].sketch_curve_id, 113);
    assert_eq!(native.sketch_curve_links[0].signed_reference, Some(1));
    assert_eq!(native.sketch_curve_links[0].role, 2);
    assert_eq!(native.sketch_curve_links[0].closure, 3);
    assert_eq!(native.creation_timestamps.len(), 5);
    assert!(native.creation_timestamps.iter().any(|timestamp| {
        matches!(timestamp.target, AttributeTarget::Vertex(_))
            && timestamp.unix_microseconds == 1_579_392_000_000_005.0
    }));
    assert_eq!(
        round_trip.ir.model.bodies[0].color,
        source_less.model.bodies[0].color
    );
    assert_eq!(
        round_trip.ir.model.faces[0].color,
        source_less.model.faces[0].color
    );

    let duplicate = f3d_native(&source_less).creation_timestamps[0].clone();
    f3d_native_mut(&mut source_less)
        .creation_timestamps
        .push(duplicate);
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("duplicate generated timestamp target must be rejected");
    assert!(error
        .to_string()
        .contains("multiple F3D creation timestamps target the same entity"));
}

#[test]
fn generated_source_less_rejects_lossy_design_link_metadata() {
    use crate::records::{PersistentDesignLink, SketchCurveLink};
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let body = source_less.model.bodies[0].id.clone();
    let coedge = source_less.model.coedges[0].id.clone();
    let mut native = f3d_native_mut(&mut source_less);
    native.persistent_design_links = vec![PersistentDesignLink {
        id: "generated:persistent-design-link#0".into(),
        target: AttributeTarget::Body(body),
        design_id: "311".into(),
        entity_kind: 3,
        design_reference: 7,
        ordinal: 1,
        is_current: false,
    }];
    native.sketch_curve_links = [0, 1]
        .map(|ordinal| SketchCurveLink {
            id: format!("generated:sketch-curve-link#{ordinal}"),
            coedge: coedge.clone(),
            sketch_curve_id: 113 + ordinal,
            signed_reference: Some(1),
            role: 2,
            closure: 3,
        })
        .into();
    drop(native);

    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("duplicate sketch links must not be collapsed");
    assert!(error
        .to_string()
        .contains("one sketch-curve link per coedge"));

    f3d_native_mut(&mut source_less).sketch_curve_links.pop();
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("noncanonical persistent link order must not be rewritten");
    assert!(error
        .to_string()
        .contains("contiguous ordinals and only the final link current"));
}

#[test]
fn generated_source_less_rejects_collapsed_native_topology_metadata() {
    use crate::records::{EdgeContinuity, TolerantVertexTail};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let edge = source_less.model.edges[0].id.clone();
    let vertex = source_less.model.vertices[0].id.clone();
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.edge_continuities = [0, 1]
            .map(|ordinal| EdgeContinuity {
                id: format!("f3d:asm:edge-continuity#generated-{ordinal}"),
                edge: edge.clone(),
                record_index: ordinal,
                sense: cadmpeg_ir::topology::Sense::Forward,
                continuity: "unknown".into(),
            })
            .into();
    }
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("duplicate edge metadata must not collapse");
    assert!(error
        .to_string()
        .contains("multiple F3D edge-continuity records"));

    {
        let mut native = f3d_native_mut(&mut source_less);
        native.edge_continuities.truncate(1);
        native.tolerant_vertex_tails = vec![TolerantVertexTail {
            id: "f3d:asm:tolerant-vertex-tail#generated".into(),
            vertex,
            record_index: 0,
            leading_tolerances: [1.0, 2.0],
        }];
    }
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("tolerant metadata on an ordinary vertex must not be dropped");
    assert!(error
        .to_string()
        .contains("requires finite fields and a tolerant vertex"));
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = f3d_native(&source_less).asm_histories[0].clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less history encode");
    let mut preambleless = source_less.clone();
    {
        let mut native = f3d_native_mut(&mut preambleless);
        native.asm_histories[0].stream_size = None;
        native.asm_histories[0].history_entry_count = None;
    }
    let mut preambleless_bytes = Vec::new();
    F3dCodec
        .encode(&preambleless, &mut preambleless_bytes)
        .expect("source-less preambleless history encode");
    let preambleless_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(preambleless_bytes),
            &DecodeOptions::default(),
        )
        .expect("source-less preambleless history round trip");
    assert_eq!(
        f3d_native(&preambleless_round_trip.ir).asm_histories[0].stream_size,
        None
    );
    assert_eq!(
        f3d_native(&preambleless_round_trip.ir).asm_histories[0].history_entry_count,
        None
    );
    f3d_native_mut(&mut source_less).asm_histories[0].states[0].bulletin_boards[0].changes[0]
        .kind = crate::history_records::AsmEntityChangeKind::Delete;
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("inconsistent generated history change kind must be rejected");
    assert!(error
        .to_string()
        .contains("kind inconsistent with its references"));
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.asm_histories[0].states[0].bulletin_boards[0].changes[0].kind =
            crate::history_records::AsmEntityChangeKind::Update;
        native.asm_histories[0].stream_size = Some(3);
    }
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("incoherent generated history preamble must be rejected");
    assert!(error
        .to_string()
        .contains("head state_id == stream_size and nonnegative history_entry_count"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less history round trip");
    let actual = &f3d_native(&round_trip.ir).asm_histories[0];
    assert_eq!(actual.stream_size, expected.stream_size);
    assert_eq!(actual.history_entry_count, expected.history_entry_count);
    assert_eq!(actual.states.len(), expected.states.len());
    assert_eq!(actual.states[0].state_id, expected.states[0].state_id);
    assert_eq!(actual.states[0].bulletin_boards.len(), 1);
    assert_eq!(actual.states[0].bulletin_boards[0].changes.len(), 2);
    assert_eq!(actual.states[0].records.len(), 1);
    assert_eq!(actual.states[0].records[0].name, "history_payload");
}

#[test]
fn generated_source_less_rejects_lossy_asm_history_graphs() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated history decode");
    let mut orphaned = decoded.ir.clone();
    orphaned.source = None;
    orphaned.set_native_unknowns("f3d", &[]).unwrap();
    orphaned
        .native
        .namespace_mut("f3d")
        .arenas
        .get_mut("asm_history_records")
        .expect("history-record arena")[0]
        .fields
        .insert("parent".into(), serde_json::json!("missing-state"));
    let error = F3dCodec
        .encode(&orphaned, &mut Vec::new())
        .expect_err("orphan history records must not be discarded");
    assert!(error
        .to_string()
        .contains("orphaned or ambiguously parented records"));

    let mut duplicate = decoded.ir.clone();
    duplicate.source = None;
    duplicate.set_native_unknowns("f3d", &[]).unwrap();
    let states = duplicate
        .native
        .namespace_mut("f3d")
        .arenas
        .get_mut("asm_delta_states")
        .expect("delta-state arena");
    states.push(states[0].clone());
    let error = F3dCodec
        .encode(&duplicate, &mut Vec::new())
        .expect_err("duplicate history identities must not multiply children");
    assert!(error
        .to_string()
        .contains("asm_delta_states contains duplicate record ids"));

    let mut broken_chain = decoded.ir;
    broken_chain.source = None;
    broken_chain.set_native_unknowns("f3d", &[]).unwrap();
    f3d_native_mut(&mut broken_chain).asm_histories[0].states[0].next_ref = Some(99);
    let error = F3dCodec
        .encode(&broken_chain, &mut Vec::new())
        .expect_err("unresolved history links must be rejected");
    assert!(error
        .to_string()
        .contains("not a coherent doubly linked state chain"));
}

#[test]
fn generated_source_less_writes_design_object_metastream() {
    use crate::records::{DesignObject, DesignObjectKind};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.design_objects = vec![
        DesignObject {
            id: "generated:design-object#0".into(),
            byte_offset: 0,
            kind: DesignObjectKind::Fusion,
            entity_ids: vec![1, 2],
            entity_id_offsets: Vec::new(),
            self_guid: "11111111-2222-3333-4444-555555555555".into(),
            self_guid_offset: 0,
            zero_run_length: 16,
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
            zero_run_length: 4,
            parent_guid: Some("11111111-2222-3333-4444-555555555555".into()),
            parent_guid_offset: None,
            revision: 9,
            revision_offset: 0,
        },
        DesignObject {
            id: "generated:design-object#2".into(),
            byte_offset: 0,
            kind: DesignObjectKind::Other("FutureFeature".into()),
            entity_ids: vec![999],
            entity_id_offsets: Vec::new(),
            self_guid: "33333333-4444-5555-6666-777777777777".into(),
            self_guid_offset: 0,
            zero_run_length: 0,
            parent_guid: Some("11111111-2222-3333-4444-555555555555".into()),
            parent_guid_offset: None,
            revision: 11,
            revision_offset: 0,
        },
    ];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less Design MetaStream encode");
    for invalid in ["", "11111111-2222-3333-4444-555555555555"] {
        let mut invalid_kind = source_less.clone();
        f3d_native_mut(&mut invalid_kind).design_objects[2].kind =
            DesignObjectKind::Other(invalid.into());
        let error = F3dCodec
            .encode(&invalid_kind, &mut Vec::new())
            .expect_err("invalid Design object class must not be emitted");
        assert!(error
            .to_string()
            .contains("Design object class is empty or GUID-shaped"));
    }
    f3d_native_mut(&mut source_less).design_objects[0].parent_guid =
        Some("22222222-3333-4444-5555-666666666666".into());
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("cyclic Design ownership must not be emitted");
    assert!(error
        .to_string()
        .contains("Design object hierarchy contains a cycle"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design MetaStream round trip");
    let objects = &f3d_native(&round_trip.ir).design_objects;
    assert_eq!(objects.len(), 3);
    let fusion = objects
        .iter()
        .find(|object| object.kind == DesignObjectKind::Fusion)
        .expect("Fusion object");
    assert_eq!(fusion.entity_ids, [1, 2]);
    assert_eq!(fusion.revision, 7);
    assert_eq!(fusion.zero_run_length, 16);
    let sketch = objects
        .iter()
        .find(|object| object.kind == DesignObjectKind::Sketch)
        .expect("Sketch object");
    assert_eq!(sketch.entity_ids, [277]);
    assert_eq!(
        sketch.parent_guid.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(sketch.revision, 9);
    assert_eq!(sketch.zero_run_length, 4);
    let future = objects
        .iter()
        .find(|object| object.kind == DesignObjectKind::Other("FutureFeature".into()))
        .expect("forward-compatible object");
    assert_eq!(future.entity_ids, [999]);
    assert_eq!(future.revision, 11);
}

#[test]
fn generated_source_less_writes_design_recipes_and_persistent_references() {
    use crate::records::{
        ConstructionRecipe, ConstructionRecipeKind, LostEdgeReference, PersistentReference,
        PersistentReferenceKind,
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
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
            value: 900,
        },
        PersistentReference {
            id: "generated:persistent-reference#1".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::CurvePrimary,
            value: 100,
        },
        PersistentReference {
            id: "generated:persistent-reference#2".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::CurveSecondary,
            value: 500,
        },
    ];
    native.lost_edge_references = vec![LostEdgeReference {
        id: "generated:lost-edge-reference#0".into(),
        record_byte_offset: 0,
        class_tag_offset: 0,
        class_tag: "419".into(),
        record_index: 4645,
        record_index_offset: 0,
        byte_offset: 0,
        next_byte_offset: 0,
        next_class_tag: "419".into(),
        next_record_index: 4646,
    }];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less Design BulkStream encode");
    f3d_native_mut(&mut source_less).construction_recipes[0].recipe_index = 1;
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("recipe group indices must not be renumbered");
    assert!(error
        .to_string()
        .contains("has noncontiguous group index 1"));
    let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
    let mut bulkstream = Vec::new();
    archive
        .by_name("FusionAssetName[Active]/Design1/BulkStream.dat")
        .expect("generated Design BulkStream")
        .read_to_end(&mut bulkstream)
        .expect("read generated Design BulkStream");
    for name in [
        b"body_recipe_data".as_slice(),
        b"face_recipe_data".as_slice(),
        b"bounded_face_recipe_data".as_slice(),
        b"edge_recipe_data".as_slice(),
        b"vertex_recipe_data".as_slice(),
    ] {
        let offset = bulkstream
            .windows(name.len())
            .position(|window| window == name)
            .expect("generated recipe name");
        assert_eq!(
            u32::from_le_bytes(bulkstream[offset - 4..offset].try_into().unwrap()),
            u32::try_from(name.len()).unwrap()
        );
        let payload = offset + name.len();
        assert_eq!(
            i64::from_le_bytes(bulkstream[payload..payload + 8].try_into().unwrap()),
            -1
        );
        assert_eq!(
            (0..5)
                .map(|ordinal| {
                    let at = payload + 8 + ordinal * 4;
                    i32::from_le_bytes(bulkstream[at..at + 4].try_into().unwrap())
                })
                .collect::<Vec<_>>(),
            [2, 0, -1, 1, -1]
        );
    }
    for name in [
        b"pt_tag".as_slice(),
        b"crv_primary_id".as_slice(),
        b"crv_secondary_id".as_slice(),
    ] {
        let offset = bulkstream
            .windows(name.len())
            .position(|window| window == name)
            .expect("generated persistent-reference name");
        let payload = offset + name.len();
        assert_eq!(
            &bulkstream[payload..payload + 8],
            &[2, 0, 0, 0, 14, 0, 0, 0]
        );
        assert_eq!(&bulkstream[payload + 8..payload + 22], &[0; 14]);
        assert_eq!(
            u32::from_le_bytes(bulkstream[payload + 22..payload + 26].try_into().unwrap()),
            23
        );
        assert_eq!(
            &bulkstream[payload + 26..payload + 49],
            b"IntrinsicMetaTypeuint64"
        );
    }
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
    let bounded = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace)
        .expect("bounded-face recipe");
    assert_eq!(bounded.design_id.as_deref(), Some("322"));
    assert_eq!(bounded.record_index, 102);
    assert_eq!(native.persistent_references.len(), 3);
    assert_eq!(
        native
            .persistent_references
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [900, 100, 500]
    );
    assert_eq!(
        native.persistent_references[1].kind,
        PersistentReferenceKind::CurvePrimary
    );
    assert_eq!(native.lost_edge_references.len(), 1);
    assert_eq!(native.lost_edge_references[0].class_tag, "419");
    assert_eq!(native.lost_edge_references[0].record_index, 4645);
    assert_eq!(native.lost_edge_references[0].next_class_tag, "419");
    assert_eq!(native.lost_edge_references[0].next_record_index, 4646);
}

#[test]
fn generated_source_less_writes_design_ownership_and_record_headers() {
    use crate::records::{
        DesignBodyMember, DesignEntityHeader, DesignObject, DesignObjectKind, DesignRecordHeader,
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.design_objects = vec![DesignObject {
        id: "generated:design-object#0".into(),
        byte_offset: 0,
        kind: DesignObjectKind::Sketch,
        entity_ids: vec![277],
        entity_id_offsets: Vec::new(),
        self_guid: "22222222-3333-4444-5555-666666666666".into(),
        self_guid_offset: 0,
        zero_run_length: 0,
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
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
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

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less Design ownership encode");
    f3d_native_mut(&mut source_less).design_entity_headers[0].declared_reference_count = Some(3);
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("mismatched sketch reference counts must not be normalized");
    assert!(error
        .to_string()
        .contains("has an inconsistent reference list"));
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.design_entity_headers[0].declared_reference_count = Some(2);
        native.design_entity_headers[0].object_kind = Some(DesignObjectKind::Body);
    }
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("cross-stream object kinds must not diverge");
    assert!(error
        .to_string()
        .contains("object kind conflicts with MetaStream ownership"));
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
    use crate::records::{
        DesignEntityHeader, DesignObject, DesignObjectKind, SketchConstraintKind,
        SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchRelation,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.design_objects = vec![DesignObject {
        id: "generated:sketch-object#0".into(),
        byte_offset: 0,
        kind: DesignObjectKind::Sketch,
        entity_ids: vec![277],
        entity_id_offsets: Vec::new(),
        self_guid: "22222222-3333-4444-5555-666666666666".into(),
        self_guid_offset: 0,
        zero_run_length: 0,
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
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    }];
    native.sketch_points = vec![SketchPoint {
        id: "generated:sketch-point#0".into(),
        record_index: 100,
        owner_reference: None,
        class_tag: "360".into(),
        byte_offset: 0,
        coordinate_offset: 89,
        entity_genesis: Some(900),
        persistent_id: 500,
        paired_reference: 101,
        coordinates: Point2::new(12.5, -25.0),
        raw_bytes: Vec::new(),
    }];
    native.sketch_curve_identities = vec![
        SketchCurveIdentity {
            id: "generated:sketch-curve#0".into(),
            record_index: 600,
            owner_reference: None,
            class_tag: "361".into(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: Some(901),
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
            owner_reference: None,
            class_tag: "362".into(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: None,
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
            owner_reference: None,
            class_tag: "363".into(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: None,
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
        owner_entity_id: String::new(),
        owner_reference_offset: 0,
        auxiliary_references: vec![900],
        auxiliary_reference_offsets: Vec::new(),
        members: vec![100, 600],
        resolved_members: Vec::new(),
        member_offsets: Vec::new(),
        state: 0x11,
        constraint_kinds: vec![
            SketchConstraintKind::Coincident,
            SketchConstraintKind::Parallel,
        ],
        unknown_constraint_bits: 0,
        member_roles: Vec::new(),
        entity_genesis: None,
        pattern: None,
        return_members: vec![600, 100],
        resolved_return_members: Vec::new(),
        return_member_offsets: Vec::new(),
        raw_bytes: Vec::new(),
    }];

    let expected_geometries = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.geometry.clone().unwrap())
        .collect::<Vec<_>>();
    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less sketch BulkStream encode");
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation.members = vec![100, 600, 100, 600, 100, 600, 100, 600];
        relation.return_members = relation.members.iter().rev().copied().collect();
    }
    let mut variable_relation = Vec::new();
    F3dCodec
        .encode(&source_less, &mut variable_relation)
        .expect("source-less variable-width sketch relation encode");
    let variable_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(variable_relation),
            &DecodeOptions::default(),
        )
        .expect("source-less variable-width sketch relation round trip");
    assert_eq!(
        f3d_native(&variable_round_trip.ir).sketch_relations[0].members,
        [100, 600, 100, 600, 100, 600, 100, 600]
    );
    assert!(
        f3d_native(&variable_round_trip.ir).sketch_relations[0]
            .raw_bytes
            .len()
            > 101
    );
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation.members = vec![100, 600];
        relation.return_members = vec![600, 100];
    }
    f3d_native_mut(&mut source_less).sketch_relations[0].owner_reference = 999;
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("relations with missing sketch owners must not disappear");
    assert!(error
        .to_string()
        .contains("references missing sketch owner"));
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.sketch_relations[0].owner_reference = 277;
        native.sketch_points[0].record_index = 600;
    }
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("duplicate typed sketch indices must not be deduplicated");
    assert!(error.to_string().contains("share record index 600"));
    f3d_native_mut(&mut source_less).sketch_points[0].record_index = 100;
    f3d_native_mut(&mut source_less).sketch_relations[0].constraint_kinds =
        vec![SketchConstraintKind::Horizontal];
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("inconsistent generated sketch constraint mask must be rejected");
    assert!(error
        .to_string()
        .contains("mask inconsistent with its typed constraint kinds"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sketch BulkStream round trip");
    let native = f3d_native(&round_trip.ir);
    assert_eq!(native.sketch_points.len(), 1);
    assert_eq!(native.sketch_points[0].persistent_id, 500);
    assert_eq!(native.sketch_points[0].entity_genesis, Some(900));
    assert_eq!(native.sketch_points[0].coordinate_offset, 141);
    assert_eq!(native.sketch_points[0].owner_reference, Some(277));
    assert_eq!(
        native.sketch_points[0].coordinates,
        Point2::new(12.5, -25.0)
    );
    assert_eq!(native.sketch_curve_identities.len(), 3);
    let genesis_curve = native
        .sketch_curve_identities
        .iter()
        .find(|curve| curve.primary_id == 700)
        .expect("genesis curve");
    assert_eq!(genesis_curve.entity_genesis, Some(901));
    assert_eq!(genesis_curve.geometry_offset, 185);
    assert_eq!(genesis_curve.owner_reference, Some(277));
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
    assert_eq!(native.sketch_relations[0].owner_entity_id, "0_277");
    assert_eq!(native.sketch_relations[0].state, 0x11);
    assert_eq!(native.sketch_relations[0].return_members, [600, 100]);
    assert_eq!(
        native.sketch_relations[0].resolved_members,
        [
            crate::records::SketchRelationOperand::Point {
                record_index: 100,
                persistent_id: 500,
            },
            crate::records::SketchRelationOperand::Curve {
                record_index: 600,
                primary_id: 700,
                secondary_id: 701,
            },
        ]
    );
    assert_eq!(
        native.sketch_relations[0].resolved_return_members,
        [
            crate::records::SketchRelationOperand::Curve {
                record_index: 600,
                primary_id: 700,
                secondary_id: 701,
            },
            crate::records::SketchRelationOperand::Point {
                record_index: 100,
                persistent_id: 500,
            },
        ]
    );
    assert!(crate::validate::validate_native(&round_trip.ir).is_empty());

    let mut inconsistent = round_trip.ir.clone();
    f3d_native_mut(&mut inconsistent).sketch_relations[0]
        .resolved_members
        .swap(0, 1);
    assert!(crate::validate::validate_native(&inconsistent)
        .iter()
        .any(|finding| {
            finding.check == cadmpeg_ir::Check::NativeLinks
                && finding.message.contains("typed operands disagree")
        }));

    let mut points = native.sketch_points.clone();
    let mut curves = native.sketch_curve_identities.clone();
    let mut relations = native.sketch_relations.clone();
    let mut conflicting_relation = relations[0].clone();
    let relation_scope = relations[0]
        .id
        .rsplit_once(':')
        .expect("generated relation identity has a stream")
        .0;
    conflicting_relation.id = format!("{relation_scope}:sketch-relation-conflict#1");
    conflicting_relation.owner_reference = 278;
    relations.push(conflicting_relation);
    let mut entities = native.design_entity_headers.clone();
    let mut second_owner = entities[0].clone();
    let entity_scope = entities[0]
        .id
        .rsplit_once(':')
        .expect("generated entity identity has a stream")
        .0;
    second_owner.id = format!("{entity_scope}:sketch-header-conflict#1");
    second_owner.entity_suffix = 278;
    second_owner.entity_id = "0_278".into();
    entities.push(second_owner);
    let error = crate::design::decode::sketch::bind_sketch_graph(
        &entities,
        &mut points,
        &mut curves,
        &mut [],
        &mut relations,
    )
    .expect_err("typed sketch geometry cannot belong to two sketches");
    assert!(error.to_string().contains("belongs to multiple sketches"));
}

#[test]
fn generated_source_less_writes_act_table_channels_and_root_component() {
    use std::collections::BTreeMap;

    use crate::records::{ActEntity, ActGuid, ActRootComponent};

    let appearance_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let physical_guid = "cccccccc-1111-2222-3333-dddddddddddd";
    let standalone_guid = "eeeeeeee-1111-2222-3333-ffffffffffff";
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
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

    drop(native);
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
fn generated_source_less_rejects_lossy_act_layouts() {
    use std::collections::BTreeMap;

    use crate::records::{ActEntity, ActGuid};

    let channel_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let standalone_guid = "eeeeeeee-1111-2222-3333-ffffffffffff";
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    {
        let mut native = f3d_native_mut(&mut source_less);
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
            channels: BTreeMap::from([("Appearance".into(), channel_guid.into())]),
            channel_guid_offsets: BTreeMap::new(),
        }];
        native.act_guids = [channel_guid, standalone_guid]
            .into_iter()
            .enumerate()
            .map(|(ordinal, guid)| ActGuid {
                id: format!("generated:act-guid#{ordinal}"),
                byte_offset: 0,
                guid_offset: 0,
                ordinal: ordinal as u32,
                guid: guid.into(),
            })
            .collect();
    }
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("ACT GUID order must not be normalized");
    assert!(error
        .to_string()
        .contains("cannot preserve this ACT GUID pool ordering"));

    {
        let mut native = f3d_native_mut(&mut source_less);
        native.act_guids.clear();
        native.act_entities[0].in_table = false;
        native.act_entities[0].channels.clear();
        native.act_entities[0].channel_class_tag = None;
    }
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("unemitted ACT entities must not disappear");
    assert!(error
        .to_string()
        .contains("has neither a table row nor channels"));
}

#[test]
fn generated_source_less_writes_protein_appearance_and_body_binding() {
    use std::collections::BTreeMap;

    use crate::records::{DesignMaterialAssignment, DesignObject, DesignObjectKind};
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
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
        textures: Vec::new(),
    }];
    source_less.model.appearance_bindings = vec![AppearanceBinding {
        id: "generated:appearance-binding#0".into(),
        target: AppearanceTarget::Body(source_less.model.bodies[0].id.clone()),
        appearance: appearance_id,
        source_entity_id: Some("0_985".into()),
        object_type: Some("Body".into()),
        channels: BTreeMap::new(),
    }];
    let mut native = f3d_native_mut(&mut source_less);
    native.design_objects = vec![DesignObject {
        id: "generated:body-object#0".into(),
        byte_offset: 0,
        kind: DesignObjectKind::Body,
        entity_ids: vec![985],
        entity_id_offsets: Vec::new(),
        self_guid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
        self_guid_offset: 0,
        zero_run_length: 0,
        parent_guid: None,
        parent_guid_offset: None,
        revision: 1,
        revision_offset: 0,
    }];
    native.design_material_assignments = vec![DesignMaterialAssignment {
        id: "generated:material-assignment#0".into(),
        asm_body_key: 42,
        asm_body_key_offset: 0,
        entity_suffix: 985,
        entity_suffix_offset: 0,
        entity_id: "0_985".into(),
        entity_id_offset: 0,
        visual_guid: visual_guid.into(),
        visual_guid_offset: 0,
        physical_token: Some("PrismMaterial-Generated".into()),
        physical_token_offset: None,
        visual_preset: None,
        visual_preset_offset: None,
    }];

    drop(native);
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
    assert_eq!(round_trip.ir.model.bodies[0].color, appearance.base_color);
    assert_eq!(
        f3d_native(&round_trip.ir).design_material_assignments[0].asm_body_key,
        42
    );
    assert_eq!(
        f3d_native(&round_trip.ir).design_material_assignments[0].visual_guid,
        visual_guid
    );
    assert_eq!(
        f3d_native(&round_trip.ir).design_material_assignments[0].visual_preset,
        None
    );
}

#[test]
fn generated_source_less_rejects_collapsed_design_body_bindings() {
    use crate::records::DesignMaterialAssignment;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    f3d_native_mut(&mut source_less).design_material_assignments = [("0_985", 985), ("0_986", 986)]
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (entity_id, entity_suffix))| DesignMaterialAssignment {
                id: format!("generated:material-assignment#{ordinal}"),
                asm_body_key: 42,
                asm_body_key_offset: 0,
                entity_suffix,
                entity_suffix_offset: 0,
                entity_id: entity_id.into(),
                entity_id_offset: 0,
                visual_guid: "11111111-2222-3333-4444-555555555555".into(),
                visual_guid_offset: 0,
                physical_token: Some("PrismMaterial-Generated".into()),
                physical_token_offset: None,
                visual_preset: None,
                visual_preset_offset: None,
            },
        )
        .collect();

    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("conflicting body-map rows must not collapse");
    assert!(error
        .to_string()
        .contains("conflicts with the body-map key/suffix bijection"));
}

#[test]
fn generated_f3d_rewrites_native_sketch_point_coordinates() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let expected = update_f3d_native(&mut edited, |native| {
        let point = &mut native.sketch_points[0];
        point.coordinates.u += 12.5;
        point.coordinates.v -= 7.5;
        point.coordinates
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("native sketch-point regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(&round_trip.ir).sketch_points[0].coordinates,
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
    let expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[0];
        let Some(crate::records::SketchCurveGeometry::Arc {
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
        curve.geometry.clone()
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("native sketch-arc regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(&round_trip.ir).sketch_curve_identities[0].geometry,
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
    let expected_references = update_f3d_native(&mut edited, |native| {
        let relation = &mut native.sketch_relations[0];
        relation.state = 0x40;
        relation.constraint_kinds = vec![crate::records::SketchConstraintKind::Horizontal];
        relation.unknown_constraint_bits = 0;
        relation.members.reverse();
        for reference in &mut relation.auxiliary_references {
            *reference = reference.saturating_add(1);
        }
        relation.return_members.reverse();
        (
            relation.members.clone(),
            relation.auxiliary_references.clone(),
            relation.owner_reference,
            relation.return_members.clone(),
        )
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("native sketch-constraint regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let native = f3d_native(&round_trip.ir);
    let relation = &native.sketch_relations[0];
    assert_eq!(relation.state, 0x40);
    assert_eq!(
        relation.constraint_kinds,
        [crate::records::SketchConstraintKind::Horizontal]
    );
    assert_eq!(relation.unknown_constraint_bits, 0);
    assert_eq!(relation.members, expected_references.0);
    assert_eq!(relation.auxiliary_references, expected_references.1);
    assert_eq!(relation.owner_reference, expected_references.2);
    assert_eq!(relation.return_members, expected_references.3);
}

#[test]
fn validation_rejects_wrong_sketch_constraint_kind_with_equal_cardinality() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut ir = decoded.ir;
    let relation_id = {
        let relation = &mut f3d_native_mut(&mut ir).sketch_relations[0];
        assert_eq!(relation.constraint_kinds.len(), 1);
        relation.constraint_kinds = vec![crate::records::SketchConstraintKind::Horizontal];
        relation.id.clone()
    };

    let findings = crate::validate::validate_native(&ir);
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::ReferentialIntegrity
            && finding.entity.as_deref() == Some(relation_id.as_str())
    }));
}

#[test]
fn validation_rejects_duplicate_sketch_geometry_persistent_identities() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut ir = decoded.ir;
    let (point_id, curve_id) = {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        native.sketch_points[1].persistent_id = native.sketch_points[0].persistent_id;
        native.sketch_points[1].owner_reference = native.sketch_points[0].owner_reference;
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[1].owner_reference =
            native.sketch_curve_identities[0].owner_reference;
        (
            native.sketch_points[1].id.clone(),
            native.sketch_curve_identities[1].id.clone(),
        )
    };

    let findings = crate::validate::validate_native(&ir);
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(point_id.as_str())
            && finding.message.contains("persistent identity")
    }));
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(curve_id.as_str())
            && finding.message.contains("persistent identity")
    }));
}

#[test]
fn validation_accepts_sketch_geometry_persistent_identities_reused_by_another_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut ir = decoded.ir;
    let (point_id, curve_id) = {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        native.sketch_points[1].persistent_id = native.sketch_points[0].persistent_id;
        native.sketch_points[0].owner_reference = Some(100);
        native.sketch_points[1].owner_reference = Some(101);
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = Some(100);
        native.sketch_curve_identities[1].owner_reference = Some(101);
        (
            native.sketch_points[1].id.clone(),
            native.sketch_curve_identities[1].id.clone(),
        )
    };

    assert!(
        !crate::validate::validate_native(&ir).iter().any(|finding| {
            finding.check == cadmpeg_ir::Check::NativeLinks
                && (finding.entity.as_deref() == Some(point_id.as_str())
                    || finding.entity.as_deref() == Some(curve_id.as_str()))
                && finding.message.contains("persistent identity")
        })
    );
}

#[test]
fn validation_rejects_aliased_sketch_geometry_records() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut ir = decoded.ir;
    let curve_id = {
        let mut native = f3d_native_mut(&mut ir);
        let point_record_index = native.sketch_points[0].record_index;
        native.sketch_curve_identities[0].record_index = point_record_index;
        native.sketch_curve_identities[0].id.clone()
    };

    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(curve_id.as_str())
            && finding
                .message
                .contains("aliases another typed indexed record")
    }));
}

#[test]
fn validation_rejects_duplicate_design_entity_suffixes() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut ir = decoded.ir;
    let duplicate_id = {
        let mut native = f3d_native_mut(&mut ir);
        let mut duplicate = native
            .design_entity_headers
            .first()
            .expect("generated Design entity header")
            .clone();
        duplicate.id.push_str("-duplicate");
        duplicate.entity_id.push_str(":duplicate");
        let id = duplicate.entity_id.clone();
        native.design_entity_headers.push(duplicate);
        id
    };

    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some(duplicate_id.as_str())
            && finding.message.contains("entity suffix is duplicated")
    }));
}

#[test]
fn validation_rejects_invalid_design_parameter_family_and_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut ir = decoded.ir;
    let parameter = crate::records::DesignParameter {
        id: "generated:design-parameter#0".into(),
        byte_offset: 100,
        class_tag: "305".into(),
        record_index: 900,
        prefix_value: 0,
        prefix_value_offset: 122,
        source_ordinal: 0,
        owner_record_index: None,
        expression: "60 mm".into(),
        expression_offset: 136,
        source_kind: "User Parameter".into(),
        source_kind_offset: 166,
        kind: crate::records::DesignParameterKind::User,
        unit: Some("mm".into()),
        unit_offset: Some(210),
        name: "Width".into(),
        name_offset: 220,
        evaluated_value: 6.0,
        evaluated_value_offset: 234,
    };
    f3d_native_mut(&mut ir).design_parameters.push(parameter);
    assert!(crate::validate::validate_native(&ir).is_empty());

    f3d_native_mut(&mut ir).design_parameters[0].prefix_value = 7;
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some("generated:design-parameter#0")
            && finding.message.contains("family discriminator")
    }));
    f3d_native_mut(&mut ir).design_parameters[0].prefix_value = 0;

    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameters[0].kind = crate::records::DesignParameterKind::Feature;
        native.design_parameters[0].owner_record_index = Some(1234);
    }
    assert!(crate::validate::validate_native(&ir).iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::NativeLinks
            && finding.entity.as_deref() == Some("generated:design-parameter#0")
    }));
}

#[test]
fn validation_requires_one_exact_extrude_profile_group() {
    use crate::records::{
        DesignConstructionOperandGroup, DesignExtrudeExtent, DesignExtrudeOperandRole,
        DesignExtrudeOperation, DesignExtrudeStart, DesignParameterScope,
        DesignSketchProfileOperand,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let profile = DesignSketchProfileOperand {
        scope_reference_ordinal: 0,
        record_index: 20,
        byte_offset: 200,
        class_tag: "300".into(),
        asset_id: "asset".into(),
        asset_id_offset: 230,
        entity_id: "0_10".into(),
        entity_suffix: 10,
        entity_reference_offset: 250,
        paired_class_tag: "260".into(),
        paired_byte_offset: 300,
    };
    let scope = DesignParameterScope {
        id: "f3d:test:scope#10".into(),
        byte_offset: 100,
        class_tag: "301".into(),
        record_index: 10,
        frame_length: 200,
        kind: "Extrude".into(),
        kind_offset: 210,
        extrude_operation: Some(DesignExtrudeOperation::NewBody),
        extrude_operation_offset: Some(128),
        extrude_extent: Some(DesignExtrudeExtent::OneSidedDistance),
        extrude_extent_offsets: Some([132, 136]),
        extrude_direction_reversed: Some(false),
        extrude_direction_reversed_offset: Some(140),
        extrude_start: Some(DesignExtrudeStart::ProfilePlane),
        extrude_start_offset: Some(141),
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 220,
        history_state_id: None,
        history_state_id_offset: 224,
        previous_history_state_id: None,
        previous_history_state_id_offset: 228,
        reference_count_offset: 180,
        reference_members: vec![20, 30],
        reference_member_offsets: vec![184, 195],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: Some(profile),
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 300,
    };
    let group = DesignConstructionOperandGroup {
        id: "f3d:test:operand-group#30".into(),
        scope_record_index: 10,
        scope_reference_ordinal: 1,
        record_index: 30,
        byte_offset: 400,
        class_tag: "302".into(),
        member_count_offset: 420,
        members: vec![20],
        lost_edge_references: Vec::new(),
        member_offsets: vec![424],
        identity_record_index: 31,
        identity_record_offset: 440,
        role: 0x0000_0041_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Profile),
        extrude_face_role: None,
        role_offset: 450,
        opaque_index: 1,
        opaque_index_offset: 460,
        opaque_scalar: 0.5,
        opaque_scalar_offset: 464,
        variant: false,
        paired_class_tag: "262".into(),
        paired_byte_offset: 500,
    };
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native
            .design_construction_operand_groups
            .push(group.clone());
    }
    let profile_message = |finding: &cadmpeg_ir::Finding| {
        finding.message == "Fusion Design Extrude profile conflicts with its profile operand group"
    };
    let findings = crate::validate::validate_native(&ir);
    assert!(!findings.iter().any(profile_message));
    assert!(!findings
        .iter()
        .any(|finding| finding.message.contains("no counted selection group")));

    f3d_native_mut(&mut ir)
        .design_construction_operand_groups
        .push(group);
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));

    f3d_native_mut(&mut ir)
        .design_construction_operand_groups
        .clear();
    assert!(crate::validate::validate_native(&ir)
        .iter()
        .any(profile_message));
}

#[test]
fn sketch_constraint_mask_decodes_equal_length_bit() {
    let (kinds, unknown) = crate::design::decode::sketch::decode_constraint_kinds(0x0000_0008);
    assert_eq!(kinds, [crate::records::SketchConstraintKind::EqualLength]);
    assert_eq!(unknown, 0);
}

#[test]
fn zero_sketch_constraint_state_decodes_as_coincident() {
    let (kinds, unknown) = crate::design::decode::sketch::decode_constraint_kinds(0);
    assert_eq!(kinds, [crate::records::SketchConstraintKind::Coincident]);
    assert_eq!(unknown, 0);
}

#[test]
fn generated_f3d_rewrites_native_sketch_nurbs_values() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[1];
        let Some(crate::records::SketchCurveGeometry::Nurbs {
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
        curve.geometry.clone()
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("native sketch-NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(&round_trip.ir).sketch_curve_identities[1].geometry,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_body_transform() {
    let source = f3d_with_smbh(&synthetic_geometry_with_transform_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    assert_eq!(f3d_native(&decoded.ir).transform_hints.len(), 1);
    assert!(!f3d_native(&decoded.ir).transform_hints[0].rotation);
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
    f3d_native_mut(&mut edited).transform_hints[0].reflection = true;
    f3d_native_mut(&mut edited).body_native_keys[0].asm_body_key = Some(84);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("body-transform regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir.model.bodies[0].transform, Some(expected));
    assert!(!f3d_native(&round_trip.ir).transform_hints[0].rotation);
    assert!(f3d_native(&round_trip.ir).transform_hints[0].reflection);
    assert_eq!(
        f3d_native(&round_trip.ir).body_native_keys[0].asm_body_key,
        Some(84)
    );
}

#[test]
fn generated_f3d_rewrites_design_recipe_and_persistent_reference() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Design decode");
    let mut edited = decoded.ir;
    let mut native = f3d_native(&edited);
    let reference = native
        .persistent_references
        .iter_mut()
        .find(|reference| reference.value == 439)
        .expect("generated persistent reference");
    assert!(reference.byte_offset > 0);
    assert!(reference.value_offset > 0);
    reference.value = 9_001;
    let recipe = &mut native.construction_recipes[0];
    assert!(recipe.byte_offset > 0);
    assert!(recipe.record_index_offset.is_some());
    assert!(recipe.design_id_offset.is_some());
    recipe.record_index = 777;
    recipe.design_id = Some("333".into());
    let member = native
        .design_body_members
        .iter_mut()
        .find(|member| member.entity_suffix == 985)
        .expect("generated body member");
    assert!(member.byte_offset > 0);
    member.entity_suffix = 12_345;
    member.flags = 7;
    let header = native
        .design_entity_headers
        .iter_mut()
        .find(|header| header.object_kind == Some(crate::records::DesignObjectKind::Sketch))
        .expect("generated sketch entity header");
    assert!(header.byte_offset > 0);
    assert!(header.record_reference_offset.is_some());
    assert_eq!(header.reference_offsets.len(), 2);
    header.record_reference = Some(585);
    header.reference_indices.swap(0, 1);
    let object = native
        .design_objects
        .iter_mut()
        .find(|object| object.kind == crate::records::DesignObjectKind::Body)
        .expect("generated body design object");
    assert!(object.byte_offset < object.revision_offset);
    assert_eq!(object.entity_id_offsets.len(), 1);
    object.entity_ids[0] = 986;
    object.self_guid = "91111111-2222-3333-4444-555555555555".into();
    object.parent_guid = Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef".into());
    object.revision = 9;
    let act_guid = native
        .act_guids
        .iter_mut()
        .find(|guid| guid.guid == "eeeeeeee-1111-2222-3333-ffffffffffff")
        .expect("generated standalone ACT GUID");
    assert!(act_guid.guid_offset > act_guid.byte_offset);
    act_guid.guid = "ffffffff-1111-2222-3333-444444444444".into();
    let act_root = &mut native.act_root_components[0];
    act_root.record_index = 70;
    act_root.instance_root_record = 71;
    act_root.components_root_record = 72;
    act_root.registry_flag = 0;
    act_root.entity_id = "0_4".into();
    act_root.display_name = "(Renamed)".into();
    let act_entity = &mut native.act_entities[0];
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
    let lost_edge = &mut native.lost_edge_references[0];
    assert!(lost_edge.class_tag_offset > lost_edge.record_byte_offset);
    assert!(lost_edge.class_tag_offset < lost_edge.byte_offset);
    lost_edge.class_tag = "420".into();
    lost_edge.record_index = 4_700;
    let assignment = &mut native.design_material_assignments[0];
    assert!(assignment.entity_id_offset > 0);
    assert!(assignment.asm_body_key_offset > 0);
    assignment.entity_id = "0_986".into();
    assignment.entity_suffix = 986;
    assignment.physical_token = Some("PrismMaterial-019".into());
    assignment.visual_preset = Some("Prism-002".into());
    native.body_native_keys[0].asm_body_key = Some(84);
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
    native.act_entities[0].entity_id = "0_986".into();
    assert_eq!(
        native.act_entities[0].entity_id,
        native.design_material_assignments[0].entity_id
    );
    native.store(edited.native.namespace_mut("f3d")).unwrap();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("persistent-reference regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Design decode");
    assert_eq!(
        f3d_native(&round_trip.ir).design_material_assignments[0].asm_body_key,
        84
    );
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
        .find(|header| header.object_kind == Some(crate::records::DesignObjectKind::Sketch))
        .cloned()
        .expect("round-trip sketch entity header");
    assert_eq!(header.record_reference, Some(585));
    assert_eq!(header.reference_indices, [44, 33]);
    let object = f3d_native(&round_trip.ir)
        .design_objects
        .iter()
        .find(|object| object.kind == crate::records::DesignObjectKind::Body)
        .cloned()
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
    update_f3d_native(&mut edited, |native| {
        native.act_entities[0].channels.insert(
            "Appearance".into(),
            "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
        );
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut Vec::new())
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
    update_f3d_native(&mut edited, |native| {
        native.design_material_assignments[0].physical_token = Some("PrismMaterial-019".into());
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut Vec::new())
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
        .write_preserved_with_source_fidelity(&invalid, &decoded.source_fidelity, &mut Vec::new())
        .expect_err("out-of-range refraction must be refused");
    assert!(
        matches!(error, cadmpeg_ir::codec::CodecError::Malformed(message) if message.contains("refraction_index"))
    );

    let mut structural = decoded.ir;
    structural.model.appearances[0]
        .properties
        .insert("unserialized_property".into(), 0.5);
    let error = F3dCodec
        .write_preserved_with_source_fidelity(
            &structural,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("edge-range regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir.model.edges[0].param_range, Some([-2.5, 4.75]));
}

#[test]
fn generated_f3d_rewrites_edge_native_metadata() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let owner = edited.model.coedges[0].id.clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        native.edge_continuities[0].continuity = "tangent".into();
        native.edge_continuities[0].sense = cadmpeg_ir::topology::Sense::Reversed;
        native.edge_ownerships[0].owner_coedge = Some(owner.clone());
    }

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("edge-continuity regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(&round_trip.ir).edge_continuities[0].continuity,
        "tangent"
    );
    assert_eq!(
        f3d_native(&round_trip.ir).edge_continuities[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        f3d_native(&round_trip.ir).edge_ownerships[0].owner_coedge,
        Some(owner)
    );
}

#[test]
fn generated_f3d_rewrites_vertex_ownership() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let mut edited = decoded.ir;
    let replacement = edited.model.edges[1].id.clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        native.vertex_ownerships[1].owning_edge = replacement.clone();
        native.vertex_ownerships[1].endpoint_index = 0;
    }

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("vertex-ownership regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let ownership = &f3d_native(&round_trip.ir).vertex_ownerships[1];
    assert_eq!(ownership.owning_edge, replacement);
    assert_eq!(ownership.endpoint_index, 0);
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        point[89..97].copy_from_slice(&coordinates[0].to_le_bytes());
        point[97..105].copy_from_slice(&coordinates[1].to_le_bytes());
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
    alternate_point[141..149].copy_from_slice(&(-4.0f64).to_le_bytes());
    alternate_point[149..157].copy_from_slice(&5.0f64.to_le_bytes());
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
    recipe_prefix[23..27].copy_from_slice(&16u32.to_le_bytes());
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
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"419");
    out.extend_from_slice(&4645u32.to_le_bytes());
    out.extend_from_slice(&[0; 14]);
    out.extend_from_slice(&19u32.to_le_bytes());
    out.extend_from_slice(b"EDGE_REFERENCE_LOST");
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"419");
    out.extend_from_slice(&4646u32.to_le_bytes());
    out.extend_from_slice(b"body_recipe_data");
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
    assert_eq!(h.release, Some(23100));
    assert_eq!(h.entity_count, Some(7));
    assert_eq!(h.flags, Some(3));
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
    synthetic_geometry_bf4_smbh_with_arc_sense(0x0b)
}

fn synthetic_geometry_bf4_nurbs_smbh() -> Vec<u8> {
    fn tagged_i32(bytes: &mut Vec<u8>, tag: u8, value: i32) {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let mut bytes = synthetic_geometry_bf4_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::first_delta_state_offset(&bytes).unwrap();
    let records = crate::sab::frame(&bytes, start, limit, 4).unwrap();
    let ellipse_range = records[19].offset..records[19].offset + records[19].len;

    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    tagged_i32(&mut curve, 0x0c, -1);
    tagged_i32(&mut curve, 0x04, -1);
    tagged_i32(&mut curve, 0x0c, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "surf_surf_int_cur");
    curve.extend_from_slice(b"\x0d\x04nubs");
    tagged_i32(&mut curve, 0x04, 2);
    tagged_i32(&mut curve, 0x15, 0);
    tagged_i32(&mut curve, 0x04, 2);
    for (knot, multiplicity) in [(0.0, 2), (1.0, 2)] {
        push_tagged_f64(&mut curve, knot);
        tagged_i32(&mut curve, 0x04, multiplicity);
    }
    for point in [[0.0, 0.0, 0.0], [0.5, 0.5, 0.0], [1.0, 0.0, 0.0]] {
        for coordinate in point {
            push_tagged_f64(&mut curve, coordinate);
        }
    }
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(ellipse_range, curve);
    bytes
}

/// `synthetic_geometry_bf4_smbh` with the arc edge's sense byte set to
/// `arc_edge_sense` (`0x0b` forward, `0x0a` reversed).
fn synthetic_geometry_bf4_smbh_with_arc_sense(arc_edge_sense: u8) -> Vec<u8> {
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
        r.push(if curve >= 0 { arc_edge_sense } else { 0x0b }); // 9 sense
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

    // The circle arc's stored [-π, -π/2] range is wrapped into the canonical
    // [0, τ] domain with its sweep preserved.
    let arc = result
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.is_some())
        .expect("edge on the ellipse carrier");
    let [start, end] = arc.param_range.expect("arc range");
    assert!((start - std::f64::consts::PI).abs() < 1e-9);
    assert!((end - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}

#[test]
fn generated_f3d_rewrites_binaryfile4_geometry() {
    let source = f3d_with_smbh(&synthetic_geometry_bf4_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated BinaryFile4 decode");
    let mut edited = decoded.ir;
    edited.model.points[0].position.x += 2.5;
    let expected = edited.model.points[0].position;
    let edge = edited
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.curve.is_some())
        .expect("generated BinaryFile4 arc edge");
    let range = edge.param_range.as_mut().expect("generated arc range");
    range[0] += 0.125;
    range[1] -= 0.125;
    let expected_range = *range;
    edited.model.faces[0].sense = match edited.model.faces[0].sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    };
    let expected_face_sense = edited.model.faces[0].sense;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("generated BinaryFile4 regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated BinaryFile4 decode");
    assert_eq!(round_trip.ir.model.points[0].position, expected);
    assert_eq!(
        round_trip
            .ir
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.is_some())
            .and_then(|edge| edge.param_range),
        Some(expected_range)
    );
    assert_eq!(round_trip.ir.model.faces[0].sense, expected_face_sense);
}

#[test]
fn generated_f3d_rewrites_binaryfile4_nurbs_integer_fields() {
    let source = f3d_with_smbh(&synthetic_geometry_bf4_nurbs_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated BinaryFile4 NURBS decode");
    let mut edited = decoded.ir;
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| {
            matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
            )
        })
        .expect("generated BinaryFile4 NURBS curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        unreachable!()
    };
    nurbs.degree = 1;
    nurbs.periodic = true;
    nurbs.knots = vec![-1.0, -1.0, 2.0, 2.0, 2.0];
    nurbs.control_points[1].z = 4.5;
    let expected = nurbs.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("generated BinaryFile4 NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated BinaryFile4 NURBS decode");
    assert!(round_trip.ir.model.curves.iter().any(|curve| {
        matches!(&curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) if nurbs == &expected)
    }));
}

#[test]
fn reversed_edge_sense_reverses_its_conic_carrier() {
    let f3d = f3d_with_smbh(&synthetic_geometry_bf4_smbh_with_arc_sense(0x0a));
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    // A reversed edge runs `E(t) = C(-t)`; the IR keeps edges forward on
    // their curve, so the conic carrier is emitted with a negated plane
    // normal. The stored parameters already live on the reversed
    // parameterization and transform exactly like a forward edge's.
    let arc = result
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.is_some())
        .expect("edge on the ellipse carrier");
    let [start, end] = arc.param_range.expect("arc range");
    assert!((start - std::f64::consts::PI).abs() < 1e-9);
    assert!((end - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);

    let curve_id = arc.curve.as_ref().expect("curve link");
    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("conic carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Circle { axis, .. } = &carrier.geometry else {
        panic!("expected the ratio-1 ellipse to decode as a circle");
    };
    assert!((axis.z - -1.0).abs() < 1e-12, "axis must be negated");
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
    assert_eq!(history.history_entry_count, Some(99));
    assert_eq!(history.states.len(), 2);
    assert_eq!(history.states[0].state_id, 2);
    assert_eq!(history.states[0].next_ref, Some(1));
    assert_eq!(history.states[0].bulletin_boards.len(), 1);
    assert_eq!(history.states[0].bulletin_boards[0].changes.len(), 2);
    assert_eq!(history.states[0].records.len(), 1);
    assert_eq!(history.states[0].records[0].name, "history_payload");
    assert_eq!(history.states[0].records[0].revision_id, Some(1830));
    assert_eq!(history.states[0].records[0].entity_references, [1830, -1]);
    assert!(!history.states[0].records[0].raw_bytes.is_empty());
    assert_eq!(
        history.states[0].bulletin_boards[0].changes[1].kind,
        crate::history_records::AsmEntityChangeKind::Insert
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
    update_f3d_native(&mut edited, |native| {
        let history = &mut native.asm_histories[0];
        assert!(history.byte_offset > 0);
        assert!(history.states[0].byte_offset > 0);
        history.stream_size = Some(8);
        history.history_entry_count = Some(120);
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
        board.changes[0].kind = crate::history_records::AsmEntityChangeKind::Delete;
        board.changes[0].old_ref = Some(26);
        board.changes[0].new_ref = None;
        board.changes[1].new_ref = Some(28);
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        f3d_native(&round_trip.ir).asm_histories[0].history_entry_count,
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
        crate::history_records::AsmEntityChangeKind::Delete
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
    let summary = codec.inspect(&mut cur, &InspectOptions::default()).unwrap();

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

    // The active history-stream selection prefers the .smbh.
    assert!(summary
        .notes
        .iter()
        .any(|n| n.contains(".smbh history stream")));
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
    let unknowns = result.ir.native_unknowns("f3d").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(result.source_fidelity.retained_records.len(), 2);
    assert!(result
        .source_fidelity
        .retained_records
        .iter()
        .all(|record| record.sha256.len() == 64));
    assert!(result
        .source_fidelity
        .retained_record("f3d:file:source-image#0")
        .is_some());
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
        .source_fidelity
        .annotations
        .provenance
        .contains_key(&unknowns[0].id.0));
}

#[test]
fn smb_only_is_reported_as_construction_snapshot() {
    // With no .smbh present, only the .smb construction snapshot remains; it must
    // be selected as a fallback but flagged as non-authoritative ([spec §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#3-asm-binary-header)).
    let f3d = synthetic_f3d(false);
    with_scan(&f3d, |scan| {
        let active = container::select_active_brep(scan).unwrap();
        assert!(!active.is_smbh);
        let summary = container::summarize(scan);
        assert!(summary
            .notes
            .iter()
            .any(|n| n.contains("construction snapshot")));
    });
}

#[test]
fn smbh_header_string_region_starts_at_byte_47() {
    // Regression: the three product strings begin at byte 47, not 48 — the
    // schema word `7` at offset 40 puts its low byte 0x07 at offset 47, which
    // doubles as the first string's TAG_UTF8_U8 tag. A parser that starts the
    // string walk at 48 reads a length byte as a tag and desyncs the whole
    // header, so record_stream_start lands mid-header and framing fails.
    let prefix = smbh_header_prefix();
    assert_eq!(prefix[47], 0x07, "first string tag at offset 47");
    // The header parses all three strings and both tolerances despite the
    // overlap, and the record stream begins immediately after the last double.
    let h = asm_header::parse(&prefix).expect("magic present");
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.flags, Some(3));
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
    let ownerships = f3d_native(&result.ir).vertex_ownerships;
    assert_eq!(ownerships.len(), 3);
    assert_eq!(
        ownerships
            .iter()
            .map(|metadata| metadata.endpoint_index)
            .collect::<Vec<_>>(),
        [0, 1, 0]
    );
    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert_eq!(f3d_native(&result.ir).face_sidedness.len(), 1);
    assert_eq!(f3d_native(&result.ir).face_sidedness[0].containment, None);
    let continuities = f3d_native(&result.ir).edge_continuities;
    assert_eq!(continuities.len(), 3);
    assert!(continuities
        .iter()
        .all(|metadata| metadata.continuity == "unknown"));
    assert!(continuities
        .iter()
        .all(|metadata| metadata.sense == cadmpeg_ir::topology::Sense::Forward));
    assert_f3d_native_parity(&result.ir);
    assert!(result
        .source_fidelity
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
    let mut result = F3dCodec
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
    assert_eq!(f3d_native(&result.ir).wire_topologies.len(), 1);
    assert_eq!(
        f3d_native(&result.ir).wire_topologies[0].side,
        crate::records::WireSide::Out
    );
    assert_eq!(
        result.ir.model.shells[0].wire_edges[0],
        result.ir.model.edges[0].id
    );
    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("wire=")));
    update_f3d_native(&mut result.ir, |native| {
        native.wire_topologies[0].side = crate::records::WireSide::In;
    });
    let mut edited = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&result.ir, &result.source_fidelity, &mut edited)
        .expect("wire-side retained edit");
    let edited = F3dCodec
        .decode(&mut Cursor::new(edited), &DecodeOptions::default())
        .expect("wire-side retained round trip");
    assert_eq!(
        f3d_native(&edited.ir).wire_topologies[0].side,
        crate::records::WireSide::In
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_transfers_isolated_vertex_wire_topology() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert!(result.ir.model.shells[0].wire_edges.is_empty());
    assert_eq!(result.ir.model.shells[0].free_vertices.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
    );
    assert!(f3d_native(&result.ir).vertex_ownerships.is_empty());
    let wire = &f3d_native(&result.ir).wire_topologies[0];
    assert!(wire.edges.is_empty());
    assert_eq!(
        wire.free_vertex,
        Some(result.ir.model.vertices[0].id.clone())
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "free-vertex findings: {:?}",
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();

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
fn generated_source_less_writes_general_face_and_point_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    let free = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let renamed = free
        .ir
        .to_canonical_json()
        .expect("canonical free-vertex JSON")
        .replace("f3d:brep:", "generated:general_point_wire:");
    let mut free =
        cadmpeg_ir::document::CadIr::from_json(&renamed).expect("renamed free-vertex IR");
    source_less.model.shells[0]
        .free_vertices
        .push(free.model.vertices[0].id.clone());
    source_less.model.vertices.append(&mut free.model.vertices);
    source_less.model.points.append(&mut free.model.points);

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less face-and-point-wire body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less face-and-point-wire body round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(round_trip.ir.model.faces.len(), 1);
    assert_eq!(round_trip.ir.model.shells[0].wire_edges.len(), 1);
    assert_eq!(round_trip.ir.model.shells[0].free_vertices.len(), 1);
    assert_eq!(f3d_native(&round_trip.ir).wire_topologies.len(), 2);
    assert!(f3d_native(&round_trip.ir)
        .wire_topologies
        .iter()
        .any(|wire| wire.edges.is_empty() && wire.free_vertex.is_some()));
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "face-and-point-wire findings: {:?}",
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    update_f3d_native(&mut source_less, |native| {
        native.wire_topologies[0].side = crate::records::WireSide::In;
    });
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
    assert_eq!(
        f3d_native(&round_trip.ir).wire_topologies[0].side,
        crate::records::WireSide::In
    );
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
fn generated_source_less_writes_isolated_vertex_wire() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    update_f3d_native(&mut source_less, |native| {
        native.wire_topologies[0].side = crate::records::WireSide::In;
    });

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less free-vertex wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less free-vertex wire round trip");
    assert_eq!(round_trip.ir.model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert!(round_trip.ir.model.shells[0].wire_edges.is_empty());
    assert_eq!(round_trip.ir.model.shells[0].free_vertices.len(), 1);
    assert!(round_trip.ir.model.edges.is_empty());
    assert_eq!(round_trip.ir.model.vertices.len(), 1);
    assert_eq!(
        round_trip.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
    );
    assert!(f3d_native(&round_trip.ir).vertex_ownerships.is_empty());
    let wire = &f3d_native(&round_trip.ir).wire_topologies[0];
    assert!(wire.edges.is_empty());
    assert_eq!(
        wire.free_vertex,
        Some(round_trip.ir.model.vertices[0].id.clone())
    );
    assert_eq!(wire.side, crate::records::WireSide::In);
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "free-vertex findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_edge_and_point_wires_on_one_shell() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let free = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let free_json = free
        .ir
        .to_canonical_json()
        .expect("canonical free-vertex JSON");
    for namespace in ["generated:point_wire_one:", "generated:point_wire_two:"] {
        let renamed = free_json.replace("f3d:brep:", namespace);
        let mut free =
            cadmpeg_ir::document::CadIr::from_json(&renamed).expect("renamed free-vertex IR");
        source_less.model.shells[0]
            .free_vertices
            .push(free.model.vertices[0].id.clone());
        source_less.model.vertices.append(&mut free.model.vertices);
        source_less.model.points.append(&mut free.model.points);
    }

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less mixed-wire shell encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less mixed-wire shell round trip");
    assert_eq!(round_trip.ir.model.shells[0].wire_edges.len(), 1);
    assert_eq!(round_trip.ir.model.shells[0].free_vertices.len(), 2);
    assert_eq!(f3d_native(&round_trip.ir).wire_topologies.len(), 3);
    assert!(f3d_native(&round_trip.ir)
        .wire_topologies
        .iter()
        .any(|wire| wire.edges.len() == 1 && wire.free_vertex.is_none()));
    assert!(f3d_native(&round_trip.ir)
        .wire_topologies
        .iter()
        .any(|wire| wire.edges.is_empty() && wire.free_vertex.is_some()));
    assert_eq!(
        f3d_native(&round_trip.ir)
            .wire_topologies
            .iter()
            .filter(|wire| wire.edges.is_empty() && wire.free_vertex.is_some())
            .count(),
        2
    );
    assert_eq!(round_trip.ir.model.vertices.len(), 4);
    assert_eq!(round_trip.ir.model.points.len(), 4);
    let validation = cadmpeg_ir::validate::validate(&round_trip.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "mixed-wire findings: {:?}",
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
    use crate::brep::geometry::{decode_curve, decode_surface};
    use crate::sab::{Record, Token};
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    fn rec(head: &str, tokens: Vec<Token>) -> Record {
        Record {
            index: 0,
            name: head.to_string(),
            head: head.to_string(),
            tokens: tokens.into(),
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

    let mut elliptical_cylinder = base();
    elliptical_cylinder.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([2.0, 0.0, 0.0]),
        Token::Double(0.4),
        Token::Double(0.0),
        Token::Double(1.0),
        Token::Double(2.0),
    ]);
    assert!(matches!(
        decode_surface(&rec("cone", elliptical_cylinder)).unwrap().0,
        SurfaceGeometry::Cone {
            radius: 20.0,
            ratio: 0.4,
            half_angle: 0.0,
            ..
        }
    ));

    // cone with nonzero sine keeps the acute half-angle asin(|sine|). A
    // both-negative sine/cosine pair has a positive slope (the radius still
    // grows along `+axis`, so the axis is kept), and the negative cosine
    // marks the inward native normal for the face-sense fold.
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
    let (geo, inward) = decode_surface(&rec("cone", cone)).unwrap();
    assert!(inward, "negative cosine points the native normal inward");
    match geo {
        SurfaceGeometry::Cone {
            half_angle,
            axis,
            ref_direction,
            ..
        } => {
            assert!((half_angle - 0.5f64.asin()).abs() < 1e-12);
            assert_eq!(axis.z, 1.0, "positive slope keeps the axis");
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected cone, got {other:?}"),
    }

    // A negative sine with positive cosine shrinks the radius along the
    // native axis; the IR cone grows along `+axis`, so the axis flips. The
    // radius comes from the major-axis vector, not the trailing u-parameter
    // scale double, which diverges on offset-derived surfaces.
    let mut shrinking = base();
    shrinking.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([4.655, 0.0, 0.0]), // |major| = 4.655 cm
        Token::Double(1.0),
        Token::Double(-0.5), // sine
        Token::Double(0.866_025_4),
        Token::Double(5.055), // u-parameter scale, not the radius
    ]);
    let (geo, inward) = decode_surface(&rec("cone", shrinking)).unwrap();
    assert!(!inward, "positive cosine keeps the outward normal");
    match geo {
        SurfaceGeometry::Cone {
            half_angle,
            axis,
            radius,
            ..
        } => {
            assert!((half_angle - 0.5f64.asin()).abs() < 1e-12);
            assert_eq!(axis.z, -1.0, "negative slope flips the axis");
            assert!((radius - 46.55).abs() < 1e-12);
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
        result
            .ir
            .native_unknowns("f3d")
            .unwrap()
            .iter()
            .any(|u| u.id == *link),
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
    assert!(note.message.contains("Native kinds: splne=1."));

    // The decoded document still validates.
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn cached_unmodeled_spline_families_retain_exact_shape_and_opaque_construction() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    for family in [
        "crv_crv_v_bl_spl_sur",
        "crv_srf_v_bl_spl_sur",
        "sfcv_free_bl_spl_sur",
        "VBL_OFFSURF",
        "offsetvbsur",
        "skin_spl_sur2",
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_exact_spl_sur_smbh(family))),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} cached decode: {error}"));
        let surface = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_)))
            .unwrap_or_else(|| panic!("{family} must retain its solved NURBS carrier"));
        let procedural = result
            .ir
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.surface == surface.id)
            .unwrap_or_else(|| panic!("{family} must retain its construction identity"));
        let ProceduralSurfaceDefinition::Unknown {
            record: Some(record),
        } = &procedural.definition
        else {
            panic!("{family} must retain its opaque construction")
        };
        assert!(result
            .ir
            .native_unknowns("f3d")
            .unwrap()
            .iter()
            .any(|unknown| unknown.id == *record));
        assert!(!result
            .report
            .losses
            .iter()
            .any(|loss| loss.message.contains("unknown-geometry surface")));
    }
}

#[test]
fn decode_reports_faces_with_missing_surface_references() {
    for (surface, condition) in [(-1i64, "null-reference=1"), (999, "dangling-reference=1")] {
        let mut smbh = synthetic_mixed_smbh();
        let start = asm_header::record_stream_start(&smbh).unwrap();
        let limit = asm_header::first_delta_state_offset(&smbh).unwrap();
        let records = crate::sab::frame(&smbh, start, limit, 8).unwrap();
        let face = records
            .iter()
            .filter(|record| record.head == "face")
            .nth(1)
            .expect("second generated face");
        let record = &mut smbh[face.offset..face.offset + face.len];
        let surface_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
        record[surface_ref + 1..surface_ref + 9].copy_from_slice(&surface.to_le_bytes());

        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("missing face surface remains an explicitly lossy decode");
        assert_eq!(result.ir.model.faces.len(), 1);
        let note = result
            .report
            .losses
            .iter()
            .find(|loss| loss.message.contains("required surface reference"))
            .unwrap_or_else(|| {
                panic!("missing face-surface loss note: {:?}", result.report.losses)
            });
        assert!(note.message.contains(condition), "{}", note.message);
    }
}

#[test]
fn decode_reports_undecoded_edge_curve_kinds() {
    let mut smbh = synthetic_geometry_with_procedural_curve_smbh();
    let needle = b"nubs";
    let position = smbh
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("procedural NURBS cache present");
    smbh[position] = b'x';

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("undecoded edge-curve carrier remains a successful topology decode");

    let note = result
        .report
        .losses
        .iter()
        .find(|loss| loss.message.contains("no decodable inline B-spline cache"))
        .expect("undecoded edge-curve loss note");
    assert!(
        note.message.contains("Native kinds: intcurve=1."),
        "{}",
        note.message
    );
}

#[test]
fn decode_reports_dangling_edge_curve_references() {
    let mut smbh = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&smbh).unwrap();
    let limit = asm_header::first_delta_state_offset(&smbh).unwrap();
    let records = crate::sab::frame(&smbh, start, limit, 8).unwrap();
    let edge = &records[10];
    let record = &mut smbh[edge.offset..edge.offset + edge.len];
    let curve_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[curve_ref + 1..curve_ref + 9].copy_from_slice(&999i64.to_le_bytes());

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("dangling curve reference remains a successful topology decode");
    let note = result
        .report
        .losses
        .iter()
        .find(|loss| loss.message.contains("no decodable inline B-spline cache"))
        .expect("dangling edge-curve loss note");
    assert!(note.message.contains("Native kinds: dangling-reference=1."));
}

#[test]
fn zero_payload_mesh_surface_is_typed_as_a_native_sentinel() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let source = f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("mesh-surface decode");

    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    let native = f3d_native(&result.ir);
    assert_eq!(native.mesh_surface_sentinels.len(), 1);
    assert_eq!(
        native.mesh_surface_sentinels[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(result.report.losses.iter().any(|loss| {
        loss.severity == cadmpeg_ir::report::Severity::Info
            && loss.message.contains("zero-payload mesh_surface")
    }));
    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("spline/procedural surfaces")));

    let mut replay = Vec::new();
    F3dCodec
        .encode_with_source_fidelity(&result.ir, Some(&result.source_fidelity), &mut replay)
        .expect("mesh-surface native replay");
    assert_eq!(replay, source);

    let mut edited = result.ir.clone();
    f3d_native_mut(&mut edited).mesh_surface_sentinels[0].id =
        "f3d:asm:mesh-surface-sentinel#edited".into();
    let error = F3dCodec
        .encode_with_source_fidelity(&edited, Some(&result.source_fidelity), &mut Vec::new())
        .expect_err("mesh-surface structural metadata is immutable");
    assert!(error.to_string().contains("edits beyond supported"));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.model.surfaces[0].geometry = SurfaceGeometry::Unknown { record: None };
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("mesh-surface sentinel requires retained ASM bytes");
    assert!(error
        .to_string()
        .contains("cannot serialize mesh-surface sentinel"));
}

#[test]
fn nurbs_surface_block_decodes_to_carrier() {
    use crate::nurbs::core::decode_surface_cache;

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
fn generated_exact_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SplineSurfaceParameters};

    for name in ["exact_spl_sur", "exactsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_exact_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("exact spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        assert_eq!(procedural.cache_fit_tolerance, Some(0.015));
        assert_eq!(
            procedural.definition,
            ProceduralSurfaceDefinition::Exact {
                parameters: SplineSurfaceParameters::OrderedRanges {
                    ranges: [[-2.0, 3.0], [-4.0, 5.0]],
                },
                extension: 7,
                revision_form: None,
            }
        );

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less exact spline surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less exact spline surface round trip");
        assert_eq!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Exact {
                parameters: SplineSurfaceParameters::OrderedRanges {
                    ranges: [[-2.0, 3.0], [-4.0, 5.0]],
                },
                extension: 7,
                revision_form: None,
            }
        );
    }
}

#[test]
fn generated_ruled_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["rule_sur", "rulesur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_ruled_spl_sur_smbh(name, true))),
                &DecodeOptions::default(),
            )
            .expect("ruled spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        assert_eq!(procedural.cache_fit_tolerance, Some(0.025));
        let ProceduralSurfaceDefinition::Ruled { first, second } = &procedural.definition else {
            panic!("expected ruled surface construction")
        };
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *first));
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *second));
        let profiles = [first.clone(), second.clone()];

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, profile) in profiles.into_iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == profile)
                .expect("ruled profile")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, 1.0, -2.0),
            };
        }
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less ruled surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less ruled surface round trip");
        let ProceduralSurfaceDefinition::Ruled { first, second } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip ruled surface")
        };
        for profile in [first, second] {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == *profile)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
    }
}

#[test]
fn generated_sum_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["sum_spl_sur", "sumsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_sum_spl_sur_smbh(name, true))),
                &DecodeOptions::default(),
            )
            .expect("sum spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Sum {
            first,
            second,
            basepoint,
            revision_form: None,
        } = &procedural.definition
        else {
            panic!("expected sum surface construction")
        };
        assert_eq!(
            *basepoint,
            cadmpeg_ir::math::Vector3::new(10.0, -20.0, 30.0)
        );
        let source_curves = [first.clone(), second.clone()];
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *first));
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *second));

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, source) in source_curves.into_iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == source)
                .expect("sum source curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(1.0, ordinal as f64, -1.0),
                direction: cadmpeg_ir::math::Vector3::new(2.0, 3.0, 4.0),
            };
        }
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less sum surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less sum surface round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Sum {
                basepoint: cadmpeg_ir::math::Vector3 {
                    x: 10.0,
                    y: -20.0,
                    z: 30.0
                },
                ..
            }
        ));
    }
}

#[test]
fn generated_cacheless_ruled_and_sum_surfaces_are_exact_carriers() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    for bytes in [
        synthetic_ruled_spl_sur_smbh("rule_sur", false),
        synthetic_sum_spl_sur_smbh("sum_spl_sur", false),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("cacheless exact surface decode");
        let procedural = result
            .ir
            .model
            .procedural_surfaces
            .first()
            .expect("cacheless procedural surface");
        assert!(procedural.cache_fit_tolerance.is_none());
        assert!(matches!(
            procedural.definition,
            ProceduralSurfaceDefinition::Ruled { .. } | ProceduralSurfaceDefinition::Sum { .. }
        ));
        assert!(matches!(
            result
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == procedural.surface)
                .map(|surface| &surface.geometry),
            Some(SurfaceGeometry::Procedural { construction })
                if construction == &procedural.id
        ));

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("cacheless exact surface source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("cacheless exact surface source-less round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Ruled { .. } | ProceduralSurfaceDefinition::Sum { .. }
        ));
    }
}

#[test]
fn generated_revolution_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["rot_spl_sur", "rotsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_rot_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("revolution spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            transposed,
            revision_form: None,
        } = &procedural.definition
        else {
            panic!("expected revolution surface construction")
        };
        assert_eq!(
            *axis_origin,
            cadmpeg_ir::math::Point3::new(10.0, -20.0, 30.0)
        );
        assert_eq!(
            *axis_direction,
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        );
        assert_eq!(*angular_interval, [0.0, 1.0]);
        assert_eq!(*parameter_interval, Some([0.0, 1.0]));
        assert!(!transposed);
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *directrix));
        let directrix = directrix.clone();

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == directrix)
            .expect("revolution directrix")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, 3.0, 4.0),
            direction: cadmpeg_ir::math::Vector3::new(5.0, -2.0, 1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less revolution surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less revolution surface round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Revolution {
                transposed: false,
                ..
            }
        ));
        let ProceduralSurfaceDefinition::Revolution { directrix, .. } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            unreachable!()
        };
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *directrix)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [0.0, 0.0, 1.0, 1.0]
                    && curve.control_points == [
                        cadmpeg_ir::math::Point3::new(2.0, 3.0, 4.0),
                        cadmpeg_ir::math::Point3::new(7.0, 1.0, 5.0),
                    ]
        ));
    }
}

#[test]
fn generated_offset_spline_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for (name, expected_flags) in [("off_spl_sur", vec![true, false, true]), ("offsur", vec![])] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_off_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("offset spline surface decode");
        let procedural = result.ir.model.procedural_surfaces.first().unwrap();
        let ProceduralSurfaceDefinition::Offset {
            support,
            revision_form: _,
            distance,
            u_sense,
            v_sense,
            extension_flags,
        } = &procedural.definition
        else {
            panic!("expected offset surface construction")
        };
        assert_eq!(*distance, -12.5);
        assert_eq!((*u_sense, *v_sense), (Some(3), Some(-4)));
        assert_eq!(*extension_flags, expected_flags);
        assert!(result
            .ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *support));

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less offset surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less offset surface round trip");
        let ProceduralSurfaceDefinition::Offset {
            distance,
            u_sense,
            v_sense,
            extension_flags,
            ..
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip offset surface")
        };
        assert_eq!((*distance, *u_sense, *v_sense), (-12.5, Some(3), Some(-4)));
        assert_eq!(*extension_flags, expected_flags);
    }
}

#[test]
fn generated_compound_spline_surface_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_comp_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("compound spline surface decode");
    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Compound {
        parameters,
        components,
    } = &procedural.definition
    else {
        panic!("expected compound surface construction")
    };
    assert_eq!(parameters, &[-0.5, 1.5]);
    assert_eq!(components.len(), 2);
    let solved = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .expect("compound solved surface");
    let SurfaceGeometry::Nurbs(solved) = &solved.geometry else {
        panic!("expected solved NURBS surface")
    };
    assert!(solved.weights.is_none());
    let rational_component = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == components[1])
        .expect("compound rational component");
    assert!(matches!(
        rational_component.geometry,
        SurfaceGeometry::Nurbs(ref surface) if surface.weights.is_some()
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less compound surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound surface round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Compound { ref parameters, ref components }
            if parameters == &[-0.5, 1.5] && components.len() == 2
    ));
}

#[test]
fn generated_taper_surface_family_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, TaperSurfaceKind};

    let cases = [
        ("taper_spl_sur", 0),
        ("ortho_spl_sur", 1),
        ("orthosur", 1),
        ("edge_tpr_spl_sur", 2),
        ("shadow_tpr_spl_sur", 3),
        ("shadowtapersur", 3),
        ("ruled_tpr_spl_sur", 4),
        ("ruledtapersur", 4),
        ("swept_tpr_spl_sur", 5),
        ("swepttapersur", 5),
    ];
    for (name, expected_kind) in cases {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_taper_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("taper surface decode");
        let ProceduralSurfaceDefinition::Taper {
            support,
            revision_form: _,
            reference,
            pcurve,
            parameter,
            taper,
        } = &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected taper surface")
        };
        assert_eq!(*parameter, 0.35);
        assert!(pcurve.is_some());
        assert!(result
            .ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *support));
        assert!(result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *reference));
        let actual_kind = match taper {
            TaperSurfaceKind::Standard => 0,
            TaperSurfaceKind::Orthogonal { sense: true } => 1,
            TaperSurfaceKind::Edge { .. } => 2,
            TaperSurfaceKind::Shadow { sine, cosine, .. } if (*sine, *cosine) == (0.6, 0.8) => 3,
            TaperSurfaceKind::Ruled { factor, .. } if *factor == 1.25 => 4,
            TaperSurfaceKind::Swept { sine, cosine, .. } if (*sine, *cosine) == (0.6, 0.8) => 5,
            _ => panic!("unexpected taper tail"),
        };
        assert_eq!(actual_kind, expected_kind);
        let reference = reference.clone();

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == reference)
            .expect("taper reference curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, -1.0, 2.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less taper encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less taper round trip");
        let ProceduralSurfaceDefinition::Taper { reference, .. } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip taper")
        };
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *reference)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [0.0, 0.0, 1.0, 1.0]
                    && curve.control_points == [
                        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                        cadmpeg_ir::math::Point3::new(5.0, 1.0, 5.0),
                    ]
        ));
    }
}

#[test]
fn generated_loft_surface_decodes_full_nested_graph() {
    use cadmpeg_ir::geometry::{
        LoftBridgeToken, ProceduralSurfaceDefinition, SplineSurfaceParameters,
    };

    for name in ["loft_spl_sur", "loftsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_loft_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("loft surface decode");
        let ProceduralSurfaceDefinition::Loft {
            sections,
            revision_form: _,
            parameters,
            closures,
            singularities,
            mode,
            bridge,
        } = &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected loft surface")
        };
        assert_eq!(
            parameters,
            &SplineSurfaceParameters::OrderedRanges {
                ranges: [[-1.0, 2.0], [-3.0, 4.0]],
            }
        );
        assert_eq!(*closures, [1, 2]);
        assert_eq!(*singularities, [3, 4]);
        assert_eq!(*mode, 2);
        assert_eq!(
            bridge,
            &[
                LoftBridgeToken::Boolean(true),
                LoftBridgeToken::Integer(17),
                LoftBridgeToken::Double(0.125),
                LoftBridgeToken::Text("bridge".into()),
                LoftBridgeToken::Enum(-7),
            ]
        );
        assert!(sections.iter().all(|section| section.entries.len() == 1));
        assert_eq!(
            sections[0].entries[0].profile[0].data.subdata.type_code,
            211
        );
        assert_eq!(
            sections[0].entries[0].profile[0].data.direction,
            Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
        );
        assert!(sections[1].entries[0].profile[0].data.direction.is_none());
        assert!(sections
            .iter()
            .flat_map(|section| &section.entries)
            .all(|entry| entry.path.auxiliaries.len() == 1));
        let line_profile = sections[0].entries[0].profile[0].curve.clone();

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == line_profile)
            .expect("loft line profile")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(4.0, -1.0, 2.0),
            direction: cadmpeg_ir::math::Vector3::new(2.0, 3.0, -1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less loft encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less loft round trip");
        let ProceduralSurfaceDefinition::Loft {
            sections,
            revision_form: _,
            parameters,
            closures,
            singularities,
            mode,
            bridge,
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip loft surface")
        };
        assert_eq!(
            parameters,
            &SplineSurfaceParameters::OrderedRanges {
                ranges: [[-1.0, 2.0], [-3.0, 4.0]],
            }
        );
        assert_eq!((*closures, *singularities, *mode), ([1, 2], [3, 4], 2));
        assert_eq!(bridge.len(), 5);
        assert!(sections.iter().all(|section| {
            section.entries.len() == 1
                && section.entries[0].profile.len() == 1
                && section.entries[0].path.auxiliaries.len() == 1
        }));
        assert_eq!(
            sections[0].entries[0].profile[0].data.direction,
            Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
        );
        assert!(sections[1].entries[0].profile[0].data.direction.is_none());
        let profile = &sections[0].entries[0].profile[0].curve;
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *profile)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [-1.0, -1.0, 2.0, 2.0]
                    && curve.control_points == [
                        cadmpeg_ir::math::Point3::new(2.0, -4.0, 3.0),
                        cadmpeg_ir::math::Point3::new(8.0, 5.0, 0.0),
                    ]
        ));
    }
}

#[test]
fn generated_net_surface_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_net_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("net surface decode");
    let ProceduralSurfaceDefinition::Net { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected net surface")
    };
    assert!(construction
        .sections
        .iter()
        .all(|section| section.entries.len() == 1));
    assert_eq!(construction.frame_parameters[11], 1.1);
    assert_eq!(construction.flag, 17);
    assert_eq!(construction.directions[2].z, 1.0);
    assert!(construction
        .formulas
        .iter()
        .all(|formula| formula.name == "null_law"));
    assert_eq!(construction.discontinuities[0], [0.25]);

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less net surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less net surface round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Net { .. }
    ));
}

#[test]
fn generated_profile_first_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_profile_first_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("profile-first sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    assert_eq!(native.primary_kind, 3);
    let SweepSurfaceLayout::ProfileFirst {
        secondary_kind,
        directions,
        origin,
        parameters,
        formulas,
    } = &native.layout
    else {
        panic!("expected profile-first sweep")
    };
    assert_eq!(*secondary_kind, 4);
    assert_eq!(directions[2].z, 1.0);
    assert_eq!(origin.z, 30.0);
    assert_eq!(*parameters, [0.1, 0.2, 0.3, 0.4]);
    assert!(formulas.iter().all(|formula| formula.name == "null_law"));
    assert_eq!(native.discontinuities[0], [0.25]);

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less profile-first sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less profile-first sweep round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Sweep {
            native: Some(_),
            ..
        }
    ));
}

#[test]
fn generated_t_spline_surface_decodes_and_writes_inline_subtransform() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, TSplineSubtransform, TSplineSurfaceConstruction,
    };

    fn construction(definition: &ProceduralSurfaceDefinition) -> &TSplineSurfaceConstruction {
        let ProceduralSurfaceDefinition::TSpline { construction } = definition else {
            panic!("expected T-spline surface")
        };
        construction
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_t_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("T-spline surface decode");
    let native = construction(&decoded.ir.model.procedural_surfaces[0].definition).clone();
    assert_eq!(native.parameter_ranges, [[-20.0, 30.0], [-40.0, 50.0]]);
    assert_eq!((native.type_code, native.trailing_value), (7, 9));
    let TSplineSubtransform::Inline {
        program,
        separator,
        values,
    } = &native.subtransform
    else {
        panic!("expected inline T-spline subtransform")
    };
    assert!(program.contains("v 1 0 0 0"));
    assert_eq!(*separator, Some(false));
    assert_eq!(values, "100verts 1 2\n");
    let graph = native
        .program_graph
        .as_ref()
        .expect("parsed T-spline graph");
    assert_eq!(graph.headers.len(), 2);
    assert_eq!(graph.records.len(), 3);
    assert_eq!(graph.records[0].kind, "v");
    assert!(graph.unparsed_lines.is_empty());
    assert_eq!(
        native.values_graph.as_ref().unwrap().records[0].kind,
        "100verts"
    );

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less T-spline encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less T-spline round trip");
    assert_eq!(
        construction(&round_trip.ir.model.procedural_surfaces[0].definition),
        &native
    );
}

#[test]
fn generated_helix_surfaces_decode_and_write_exact_constructions() {
    use cadmpeg_ir::geometry::{HelixSurfaceProfile, ProceduralSurfaceDefinition, SurfaceGeometry};

    for circular in [true, false] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_helix_surface_smbh(circular))),
                &DecodeOptions::default(),
            )
            .expect("helix surface decode");
        let ProceduralSurfaceDefinition::Helix { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected helix surface")
        };
        assert_eq!(construction.angle_range, [-0.5, 0.5]);
        assert_eq!(construction.path.center.z, 30.0);
        assert_eq!(construction.path.pitch.z, 40.0);
        assert_eq!(
            circular,
            matches!(construction.profile, HelixSurfaceProfile::Circle { .. })
        );

        let surface_id = decoded.ir.model.procedural_surfaces[0].surface.clone();
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let surface = source_less
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == surface_id)
            .unwrap();
        assert!(
            matches!(
                &surface.geometry,
                SurfaceGeometry::Procedural { construction }
                    if *construction == source_less.model.procedural_surfaces[0].id
            ),
            "unexpected helix carrier: {:?}",
            surface.geometry
        );
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less helix surface encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less helix surface round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Helix { .. }
        ));
    }
}

#[test]
fn generated_source_less_rejects_duplicate_procedural_surface_owners() {
    for (smbh, label) in [
        (synthetic_cyl_spl_sur_smbh(), "cached"),
        (synthetic_helix_surface_smbh(true), "cacheless"),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("generated {label} surface decode: {error}"));
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut duplicate = source_less.model.procedural_surfaces[0].clone();
        duplicate.id = format!("generated:duplicate-{label}").into();
        source_less.model.procedural_surfaces.push(duplicate);

        let error = F3dCodec.encode(&source_less, &mut Vec::new()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("multiple procedural constructions"),
            "unexpected {label} duplicate-owner error: {error}"
        );
    }
}

#[test]
fn generated_source_less_refuses_procedural_construction_loss_on_analytic_carriers() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated procedural surface decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let surface_id = source_less.model.procedural_surfaces[0].surface.clone();
    source_less
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap()
        .geometry = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("analytic carrier must not discard its procedural surface");
    assert!(error
        .to_string()
        .contains("cannot retain its construction on analytic carrier"));

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated procedural curve decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let curve_id = source_less.model.procedural_curves[0].curve.clone();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("analytic carrier must not discard its procedural curve");
    assert!(error
        .to_string()
        .contains("cannot retain its construction on carrier"));
}

#[test]
fn generated_minimal_deformable_surface_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_minimal_deformable_surface_smbh())),
            &DecodeOptions::default(),
        )
        .expect("deformable surface decode");
    let ProceduralSurfaceDefinition::Deformable { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected deformable surface")
    };
    let DeformableSurfaceData::Minimal { vectors, selector } = &construction.data else {
        panic!("expected minimal deformable surface")
    };
    assert_eq!(vectors[2].z, 1.0);
    assert_eq!(*selector, 0);
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec.encode(&source_less, &mut encoded).unwrap();
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Deformable { .. }
    ));
}

#[test]
fn generated_framed_deformable_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    for mode in [1, 3] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_framed_deformable_surface_smbh(
                    mode,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected deformable surface")
        };
        match &construction.data {
            DeformableSurfaceData::Plain {
                frame,
                parameter_triples,
            } => {
                assert_eq!(mode, 1);
                assert_eq!(frame.point.z, 60.0);
                assert_eq!(parameter_triples.len(), 2);
            }
            DeformableSurfaceData::Guided {
                frame,
                guide_parameter,
                ..
            } => {
                assert_eq!(mode, 3);
                assert_eq!(frame.point.z, 60.0);
                assert_eq!(*guide_parameter, 0.9);
            }
            DeformableSurfaceData::Minimal { .. }
            | DeformableSurfaceData::SurfaceCurve { .. }
            | DeformableSurfaceData::Full { .. } => {
                panic!("wrong mode")
            }
        }
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec.encode(&source_less, &mut encoded).unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Deformable { .. }
        ));
    }
}

#[test]
fn generated_surface_curve_deformable_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_surface_curve_deformable_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ProceduralSurfaceDefinition::Deformable { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    let DeformableSurfaceData::SurfaceCurve {
        native_id,
        first_parameter,
        selector,
        second_parameter,
        curve,
        parameter_triples,
        ..
    } = &construction.data
    else {
        panic!()
    };
    assert_eq!((*native_id, *selector), (42, 3));
    assert_eq!(parameter_triples, &[[0.1, 0.2, 0.3]]);
    let curve = curve.clone();
    let range = [*first_parameter, *second_parameter];
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|candidate| candidate.id == curve)
        .expect("surface-curve deformable curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(1.0, -2.0, 3.0),
        direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -1.0),
    };
    let mut encoded = Vec::new();
    F3dCodec.encode(&source_less, &mut encoded).unwrap();
    let round = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        round.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Deformable { .. }
    ));
    assert!(round.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve)
            if curve.degree == 1
                && curve.knots == [range[0], range[0], range[1], range[1]]
    )));
}

#[test]
fn generated_full_deformable_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    for expected_version_value in [None, Some(226)] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_full_deformable_surface_smbh(
                    expected_version_value,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!()
        };
        let DeformableSurfaceData::Full {
            selector,
            native_id,
            first_parameter,
            version_value,
            second_parameter,
            curve,
            frames,
            trailing_value,
            ..
        } = &construction.data
        else {
            panic!()
        };
        assert_eq!((*selector, *native_id), (7, 42));
        assert_eq!(*version_value, expected_version_value);
        assert_eq!(frames[0].parameter, 0.4);
        assert_eq!(frames[1].parameter, 0.5);
        assert_eq!(*trailing_value, 99);
        let curve = curve.clone();
        let range = [*first_parameter, *second_parameter];
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|candidate| candidate.id == curve)
            .expect("full deformable curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, -4.0, 2.0),
        };
        let mut encoded = Vec::new();
        F3dCodec.encode(&source_less, &mut encoded).unwrap();
        let round = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable { construction } =
            &round.ir.model.procedural_surfaces[0].definition
        else {
            panic!()
        };
        assert!(matches!(
            construction.data,
            DeformableSurfaceData::Full { version_value, .. }
                if version_value == expected_version_value
        ));
        assert!(round.ir.model.curves.iter().any(|curve| matches!(
            &curve.geometry,
            cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve)
                if curve.degree == 1
                    && curve.knots == [range[0], range[0], range[1], range[1]]
        )));
    }
}

#[test]
fn generated_t_spline_surface_resolves_shared_subtransform_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, TSplineSubtransform};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_referenced_t_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("referenced T-spline decode");
    let ProceduralSurfaceDefinition::TSpline { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected T-spline surface")
    };
    let TSplineSubtransform::Reference {
        index,
        resolved: Some(resolved),
    } = &construction.subtransform
    else {
        panic!("expected resolved T-spline reference")
    };
    assert!(*index >= 0);
    assert!(matches!(
        resolved.as_ref(),
        TSplineSubtransform::Inline { program, .. } if program.contains("v 1 0 0 0")
    ));
    assert_eq!(
        construction.program_graph.as_ref().unwrap().records.len(),
        1
    );
    assert_eq!(
        construction.values_graph.as_ref().unwrap().records[0].kind,
        "100verts"
    );

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less referenced T-spline encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less referenced T-spline round trip");
    let ProceduralSurfaceDefinition::TSpline { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip T-spline surface")
    };
    assert!(matches!(
        construction.subtransform,
        TSplineSubtransform::Inline { .. }
    ));
}

#[test]
fn generated_explicit_formula_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_formula_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit formula sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    let SweepSurfaceLayout::ExplicitFormula {
        mode,
        profile_range,
        profile_frame,
        origin,
        path_range,
        formula,
        ..
    } = &native.layout
    else {
        panic!("expected explicit formula sweep")
    };
    assert_eq!(*mode, 7);
    assert_eq!(*profile_range, [-0.5, 1.5]);
    assert_eq!(profile_frame.as_ref().unwrap().0.z, 30.0);
    assert_eq!(origin.z, 60.0);
    assert_eq!(*path_range, [-20.0, 30.0]);
    assert_eq!(formula.name, "null_law");
    let profile = profile.clone();
    let spine = spine.clone();

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, curve_id) in [&profile, &spine].into_iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, -1.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, -2.0, 4.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less explicit formula sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit formula sweep round trip");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip explicit formula sweep")
    };
    assert!(matches!(
        native.layout,
        SweepSurfaceLayout::ExplicitFormula { .. }
    ));
    for (curve_id, knots) in [
        (profile, [-0.5, -0.5, 1.5, 1.5]),
        (spine, [-2.0, -2.0, 3.0, 3.0]),
    ] {
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == knots
        ));
    }
}

#[test]
fn generated_source_less_sweep_refuses_missing_native_graph() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_formula_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated native sweep decode")
        .ir;
    decoded.source = None;
    decoded.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Sweep { native, .. } =
        &mut decoded.model.procedural_surfaces[0].definition
    else {
        panic!("expected generated sweep")
    };
    *native = None;

    let error = F3dCodec
        .encode(&decoded, &mut Vec::new())
        .expect_err("a sweep without its native graph must not be guessed");
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(message)
            if message.contains("lacks its native construction graph")
    ));
}

#[test]
fn generated_explicit_guide_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_guide_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit guide sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    let SweepSurfaceLayout::ExplicitGuide {
        mode,
        profile_range,
        profile_frame,
        path_range,
        guide_curve,
        guide_range,
        guide_modes,
        guide_parameters,
        trailing_flags,
        ..
    } = &native.layout
    else {
        panic!("expected explicit guide sweep")
    };
    assert_eq!(*mode, 8);
    assert!(profile_frame.is_none());
    assert_eq!(*guide_range, [0.0, 1.0]);
    assert_eq!(*guide_modes, [11, 12]);
    assert_eq!(guide_parameters[5], 0.6);
    assert_eq!(*trailing_flags, [true, false, true]);
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), [path_range[0] / 10.0, path_range[1] / 10.0]),
        (guide_curve.clone(), *guide_range),
    ];

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit guide sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, -2.0, 1.0),
            direction: cadmpeg_ir::math::Vector3::new(2.0, 4.0, -3.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less explicit guide sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit guide sweep round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Sweep {
            native: Some(native),
            ..
        } if matches!(native.layout, SweepSurfaceLayout::ExplicitGuide { .. })
    ));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_explicit_surface_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_surface_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit surface sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    let SweepSurfaceLayout::ExplicitSurface {
        mode,
        profile_range,
        path_range,
        singularity,
        auxiliary_curve,
        support_flag,
        legacy_flag,
        ..
    } = &native.layout
    else {
        panic!("expected explicit surface sweep")
    };
    assert_eq!((*mode, *singularity), (9, 1));
    assert!(auxiliary_curve.is_some());
    assert!(*support_flag);
    assert_eq!(*legacy_flag, Some(false));
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), [path_range[0] / 10.0, path_range[1] / 10.0]),
    ];

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit surface sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 1.0, -2.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less explicit surface sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit surface sweep round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Sweep {
            native: Some(native),
            ..
        } if matches!(native.layout, SweepSurfaceLayout::ExplicitSurface { .. })
    ));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_law_driven_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_law_driven_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("law-driven sweep decode");
    let ProceduralSurfaceDefinition::Sweep {
        profile,
        spine,
        native: Some(native),
        ..
    } = &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected native sweep")
    };
    let SweepSurfaceLayout::LawDriven {
        mode,
        profile_range,
        first_law,
        first_mode,
        second_law,
        formula_mode,
        formula,
        path_range,
        ..
    } = &native.layout
    else {
        panic!("expected law-driven sweep")
    };
    assert_eq!((*mode, *first_mode, *formula_mode), (10, 21, 23));
    assert!(matches!(first_law.as_ref(), LawExpression::Double { value } if *value == 2.5));
    assert!(matches!(second_law.as_ref(), LawExpression::Vector { value } if value.z == 3.0));
    assert_eq!(formula.name, "null_law");
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), *path_range),
    ];

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("law-driven sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, -1.0, 2.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, 4.0, -2.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less law-driven sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less law-driven sweep round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Sweep {
            native: Some(native),
            ..
        } if matches!(native.layout, SweepSurfaceLayout::LawDriven { .. })
    ));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_legacy_surface_names_select_modern_layouts() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let cases = [
        (
            renamed_generated_subtype(
                synthetic_skin_spl_sur_smbh(0, false),
                "skin_spl_sur",
                "skinsur",
            ),
            "skin",
        ),
        (
            renamed_generated_subtype(synthetic_net_spl_sur_smbh(), "net_spl_sur", "netsur"),
            "net",
        ),
        (
            renamed_generated_subtype(
                synthetic_profile_first_sweep_smbh(),
                "sweep_spl_sur",
                "sweepsur",
            ),
            "sweep",
        ),
        (
            renamed_generated_subtype(
                synthetic_scaled_compound_loft_smbh(true),
                "scaled_cloft_spl_sur",
                "sclclftsur",
            ),
            "scaled_compound_loft",
        ),
        (
            renamed_generated_subtype(synthetic_cyl_spl_sur_smbh(), "cyl_spl_sur", "cylsur"),
            "extrusion",
        ),
    ];
    for (smbh, expected) in cases {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{expected} legacy decode: {error}"));
        let definition = &decoded.ir.model.procedural_surfaces[0].definition;
        assert!(
            matches!(
                (expected, definition),
                ("skin", ProceduralSurfaceDefinition::Skin { .. })
                    | ("net", ProceduralSurfaceDefinition::Net { .. })
                    | ("sweep", ProceduralSurfaceDefinition::Sweep { .. })
                    | (
                        "scaled_compound_loft",
                        ProceduralSurfaceDefinition::ScaledCompoundLoft { .. }
                    )
                    | ("extrusion", ProceduralSurfaceDefinition::Extrusion { .. })
            ),
            "wrong definition for {expected}: {definition:?}"
        );
    }
}

#[test]
fn generated_procedural_surface_tolerance_presence_matches_native_grammar() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let required = [
        (
            synthetic_minimal_deformable_surface_smbh(),
            "deformable surface",
        ),
        (synthetic_t_spl_sur_smbh(), "T-spline surface"),
        (
            synthetic_exact_spl_sur_smbh("exact_spl_sur"),
            "exact spline surface",
        ),
        (
            synthetic_variable_blend_smbh("var_blend_spl_sur"),
            "variable blend",
        ),
        (
            synthetic_full_rolling_ball_smbh("rb_blend_spl_sur"),
            "rolling-ball blend",
        ),
        (synthetic_skin_spl_sur_smbh(0, false), "skin surface"),
        (synthetic_net_spl_sur_smbh(), "net surface"),
        (synthetic_profile_first_sweep_smbh(), "sweep surface"),
    ];
    for (smbh, family) in required {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} decode: {error}"));
        assert!(decoded.ir.model.procedural_surfaces[0]
            .cache_fit_tolerance
            .is_some());
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.procedural_surfaces[0].cache_fit_tolerance = None;
        let error = F3dCodec.encode(&source_less, &mut Vec::new()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("{family} requires a native cache-fit tolerance")),
            "unexpected {family} error: {error}"
        );
    }

    let optional = [
        (synthetic_comp_spl_sur_smbh(), "compound"),
        (synthetic_taper_spl_sur_smbh("taper_spl_sur"), "taper"),
        (synthetic_ruled_spl_sur_smbh("rule_sur", true), "ruled"),
        (synthetic_sum_spl_sur_smbh("sum_spl_sur", true), "sum"),
        (synthetic_rot_spl_sur_smbh("rot_spl_sur"), "revolution"),
        (synthetic_off_spl_sur_smbh("off_spl_sur"), "offset"),
        (synthetic_cyl_spl_sur_smbh(), "extrusion"),
        (
            synthetic_g2_blend_spl_sur_smbh("g2_blend_spl_sur", false),
            "G2 blend",
        ),
    ];
    for (smbh, family) in optional {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("optional-tolerance surface decode");
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.procedural_surfaces[0].cache_fit_tolerance = None;
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less surface without optional tolerance");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{family} round trip: {error}"));
        assert_eq!(
            round_trip.ir.model.procedural_surfaces.len(),
            1,
            "{family} procedural surface was not reconstructed"
        );
        assert_eq!(
            round_trip.ir.model.procedural_surfaces[0].cache_fit_tolerance,
            None
        );
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_loft_spl_sur_smbh("loft_spl_sur"))),
            &DecodeOptions::default(),
        )
        .expect("loft decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.procedural_surfaces[0].cache_fit_tolerance = None;
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less loft without optional tolerance");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less loft round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::Loft { .. }
    ));
    assert_eq!(
        round_trip.ir.model.procedural_surfaces[0].cache_fit_tolerance,
        None
    );
}

#[test]
fn generated_procedural_curve_optional_tolerance_absence_round_trips() {
    let cases = [
        (synthetic_geometry_with_exact_curve_smbh(), "exact"),
        (synthetic_geometry_with_law_curve_smbh(), "law"),
        (
            synthetic_geometry_with_deformable_curve_smbh(8),
            "deformable",
        ),
        (synthetic_geometry_with_projection_smbh(), "projection"),
        (
            synthetic_geometry_with_early_close_projection_smbh(),
            "early-close projection",
        ),
        (synthetic_geometry_with_compound_curve_smbh(), "compound"),
        (
            synthetic_geometry_with_surface_curve_smbh("surf_int_cur"),
            "surface curve",
        ),
        (
            synthetic_geometry_with_silhouette_smbh("para_silh_int_cur", None),
            "silhouette",
        ),
        (
            synthetic_geometry_with_surface_offset_smbh(),
            "surface offset",
        ),
        (synthetic_geometry_with_spring_smbh(), "spring"),
        (
            synthetic_geometry_with_three_surface_intersection_smbh(),
            "three-surface intersection",
        ),
        (
            synthetic_geometry_with_two_sided_offset_curve_smbh(),
            "two-sided offset",
        ),
        (
            synthetic_geometry_with_vector_offset_curve_smbh(),
            "vector offset",
        ),
        (synthetic_geometry_with_subset_curve_smbh(), "subset"),
        (synthetic_geometry_with_helix_curve_smbh(), "helix"),
    ];
    for (smbh, family) in cases {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} decode: {error}"));
        assert_eq!(
            decoded.ir.model.procedural_curves.len(),
            1,
            "{family} fixture must decode one procedural curve"
        );
        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.procedural_curves[0].cache_fit_tolerance = None;
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .unwrap_or_else(|error| panic!("{family} source-less encode: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{family} round trip: {error}"));
        assert_eq!(
            round_trip.ir.model.procedural_curves.len(),
            1,
            "{family} procedural curve was not reconstructed"
        );
        assert_eq!(
            round_trip.ir.model.procedural_curves[0].cache_fit_tolerance, None,
            "{family} invented a cache-fit tolerance"
        );
    }
}

#[test]
fn generated_compound_loft_decodes_scale_and_zero_tail() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, CompoundLoftTail, ProceduralSurfaceDefinition,
    };

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_compound_loft_smbh())),
            &DecodeOptions::default(),
        )
        .expect("compound-loft decode");
    let ProceduralSurfaceDefinition::CompoundLoft { construction } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected compound loft")
    };
    let scale = construction.scales[0].as_ref().expect("first scale");
    assert!(construction.scales[1..].iter().all(Option::is_none));
    assert_eq!(scale.members.len(), 1);
    assert!(scale.members[0].data.pcurve.is_some());
    assert_eq!(scale.auxiliaries.len(), 1);
    assert_eq!(scale.tail, [2, 3]);
    assert_eq!(construction.flags, [true, false]);
    let CompoundLoftTail::Zero {
        flags,
        selector,
        direction,
        trailing_flags,
    } = &construction.tail
    else {
        panic!("expected zero tail")
    };
    assert_eq!(*flags, [false, true]);
    assert_eq!(*selector, 0);
    assert!(matches!(direction, CompoundLoftDirection::Vector { .. }));
    assert_eq!(*trailing_flags, [true, false]);
    let member_curve = scale.members[0].curve.clone();

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut missing_tolerance = source_less.clone();
    missing_tolerance.model.procedural_surfaces[0].cache_fit_tolerance = None;
    let error = F3dCodec
        .encode(&missing_tolerance, &mut Vec::new())
        .expect_err("compound loft without its required tolerance must be rejected");
    assert!(
        error
            .to_string()
            .contains("compound-loft surface requires a native cache-fit tolerance"),
        "unexpected error: {error}"
    );
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == member_curve)
        .expect("compound-loft member curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
        direction: cadmpeg_ir::math::Vector3::new(4.0, -3.0, 2.0),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less compound-loft encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound-loft round trip");
    let ProceduralSurfaceDefinition::CompoundLoft { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip compound loft")
    };
    assert!(construction.scales[0].is_some());
    assert!(construction.scales[1..].iter().all(Option::is_none));
    assert_eq!(construction.flags, [true, false]);
    assert!(matches!(
        construction.tail,
        CompoundLoftTail::Zero {
            selector: 0,
            direction: CompoundLoftDirection::Vector { .. },
            ..
        }
    ));
    let member_curve = &construction.scales[0]
        .as_ref()
        .expect("round-trip scale")
        .members[0]
        .curve;
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *member_curve)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
    ));
}

#[test]
fn generated_compound_loft_writes_every_tail_shape_source_less() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, CompoundLoftTail, ProceduralSurfaceDefinition,
    };
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_compound_loft_smbh())),
            &DecodeOptions::default(),
        )
        .expect("compound-loft decode");
    let ProceduralSurfaceDefinition::CompoundLoft { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected compound loft")
    };
    let scale = construction.scales[0].clone().expect("generated scale");
    let curve = scale.path.clone();
    let line_curve = cadmpeg_ir::ids::CurveId("generated:compound_loft_tail_line#0".into());
    let tails = [
        CompoundLoftTail::Six {
            flags: [true, false],
            scale: Box::new(scale.clone()),
            selector: 31,
            direction: Vector3::new(0.0, 1.0, 0.0),
            parameter_range: [-0.5, 1.5],
            curve: line_curve.clone(),
        },
        CompoundLoftTail::Seven {
            first_flag: true,
            first_scale: Some(Box::new(scale.clone())),
            second_flag: false,
            second_scale: Box::new(scale.clone()),
            selector: -7,
            direction: Vector3::new(1.0, 0.0, 0.0),
            trailing_flags: [false, true],
        },
        CompoundLoftTail::Zero {
            flags: [false, true],
            selector: 4,
            direction: CompoundLoftDirection::Curve { curve },
            trailing_flags: [true, true],
        },
    ];

    for (tail_index, expected) in tails.into_iter().enumerate() {
        let mut source_less = decoded.ir.clone();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: line_curve.clone(),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, -2.0, 1.0),
            },
            source_object: None,
        });
        let ProceduralSurfaceDefinition::CompoundLoft { construction } =
            &mut source_less.model.procedural_surfaces[0].definition
        else {
            unreachable!()
        };
        construction.tail = expected.clone();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less compound-loft encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less compound-loft round trip");
        assert_eq!(
            round_trip.ir.model.procedural_surfaces.len(),
            1,
            "tail {tail_index} did not decode"
        );
        let ProceduralSurfaceDefinition::CompoundLoft { construction } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip compound loft")
        };
        match (&expected, &construction.tail) {
            (
                CompoundLoftTail::Six { .. },
                CompoundLoftTail::Six {
                    parameter_range,
                    curve,
                    ..
                },
            ) => {
                assert!(matches!(
                    round_trip
                        .ir
                        .model
                        .curves
                        .iter()
                        .find(|candidate| candidate.id == *curve)
                        .map(|curve| &curve.geometry),
                    Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                        if curve.degree == 1
                            && curve.knots
                                == [
                                    parameter_range[0],
                                    parameter_range[0],
                                    parameter_range[1],
                                    parameter_range[1],
                                ]
                ));
            }
            (CompoundLoftTail::Seven { .. }, CompoundLoftTail::Seven { first_scale, .. }) => {
                assert!(first_scale.is_some());
            }
            (
                CompoundLoftTail::Zero { .. },
                CompoundLoftTail::Zero {
                    selector: 4,
                    direction: CompoundLoftDirection::Curve { .. },
                    ..
                },
            ) => {}
            _ => panic!("compound-loft tail shape changed"),
        }
    }
}

#[test]
fn generated_scaled_compound_loft_decodes_full_direct_branch() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ProceduralSurfaceDefinition, ScaledCompoundLoftBranch,
        ScaledCompoundLoftShape,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(true))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected scaled compound loft")
    };
    assert!(matches!(construction.shape, ScaledCompoundLoftShape::Full));
    assert_eq!(construction.singularity, 11);
    assert_eq!(construction.discontinuities[0], [0.25]);
    assert!(construction.discontinuities[1..].iter().all(Vec::is_empty));
    assert!(construction.discontinuity_flag);
    assert!(construction.scales[0].is_some());
    assert!(construction.scales[1..].iter().all(Option::is_none));
    assert_eq!(construction.flags, [true, false]);
    assert_eq!(construction.selector, 0);
    assert!(matches!(
        construction.branch,
        ScaledCompoundLoftBranch::Direct {
            flag: true,
            selector: 0,
            direction: CompoundLoftDirection::Vector { .. },
        }
    ));
    assert_eq!(construction.trailing_flags, [false, true]);
    assert_eq!(construction.tail_kind, 2);
    assert_eq!(construction.tail_singularity, 12);

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut missing_tolerance = source_less.clone();
    missing_tolerance.model.procedural_surfaces[0].cache_fit_tolerance = None;
    assert!(F3dCodec
        .encode(&missing_tolerance, &mut Vec::new())
        .expect_err("full scaled compound loft without tolerance must fail")
        .to_string()
        .contains("full shape requires a native cache-fit tolerance"));
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less scaled compound-loft encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less scaled compound-loft round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::ScaledCompoundLoft { .. }
    ));
}

#[test]
fn generated_scaled_compound_loft_writes_all_middle_branches_source_less() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ProceduralSurfaceDefinition, ScaledCompoundLoftBranch,
        ScaledCompoundLoftShape,
    };
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(true))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected scaled compound loft")
    };
    let scale = construction.scales[0].clone().expect("generated scale");
    let curve = scale.path.clone();
    let cases = [
        (
            ScaledCompoundLoftShape::Full,
            ScaledCompoundLoftBranch::ExtendedVector {
                first_scale: None,
                second_scale: Box::new(scale.clone()),
                selector: 9,
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
        ),
        (
            ScaledCompoundLoftShape::Full,
            ScaledCompoundLoftBranch::ExtendedCurve {
                scale: None,
                flag: true,
                singularity: 13,
                curve: curve.clone(),
            },
        ),
        (
            ScaledCompoundLoftShape::Full,
            ScaledCompoundLoftBranch::Direct {
                flag: false,
                selector: 4,
                direction: CompoundLoftDirection::Curve {
                    curve: curve.clone(),
                },
            },
        ),
    ];

    for (case_index, (shape, branch)) in cases.into_iter().enumerate() {
        let mut source_less = decoded.ir.clone();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
            &mut source_less.model.procedural_surfaces[0].definition
        else {
            unreachable!()
        };
        construction.shape = shape;
        construction.branch = branch;
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less scaled compound-loft encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less scaled compound-loft round trip");
        assert_eq!(
            round_trip.ir.model.procedural_surfaces.len(),
            1,
            "scaled compound-loft case {case_index} did not decode"
        );
        let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip scaled compound loft")
        };
        assert!(matches!(
            (&construction.shape, &construction.branch),
            (
                ScaledCompoundLoftShape::Full,
                ScaledCompoundLoftBranch::ExtendedVector { .. }
            ) | (
                ScaledCompoundLoftShape::Full,
                ScaledCompoundLoftBranch::ExtendedCurve { .. }
            ) | (
                ScaledCompoundLoftShape::Full,
                ScaledCompoundLoftBranch::Direct {
                    direction: CompoundLoftDirection::Curve { .. },
                    ..
                }
            )
        ));
    }
}

#[test]
fn generated_scaled_compound_loft_none_shape_round_trips_as_procedural_face() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, ScaledCompoundLoftShape, SurfaceGeometry,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(false))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft none-shape decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected scaled compound loft")
    };
    assert!(matches!(
        construction.shape,
        ScaledCompoundLoftShape::None {
            parameter_ranges: [[-1.0, 2.0], [-3.0, 4.0]],
            ..
        }
    ));
    let owner = decoded.ir.model.procedural_surfaces[0].surface.clone();
    assert!(matches!(
        decoded
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == owner)
            .expect("procedural owner")
            .geometry,
        SurfaceGeometry::Procedural { ref construction }
            if *construction == decoded.ir.model.procedural_surfaces[0].id
    ));
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut unexpected_tolerance = source_less.clone();
    unexpected_tolerance.model.procedural_surfaces[0].cache_fit_tolerance = Some(0.04);
    assert!(F3dCodec
        .encode(&unexpected_tolerance, &mut Vec::new())
        .expect_err("none-shape scaled compound loft with tolerance must fail")
        .to_string()
        .contains("none shape cannot carry a cache-fit tolerance"));
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less scaled compound-loft none-shape encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less scaled compound-loft none-shape round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::ScaledCompoundLoft { .. }
    ));
}

#[test]
fn generated_skin_surface_decodes_recursive_spline_law() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SkinSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, false))),
            &DecodeOptions::default(),
        )
        .expect("skin surface decode");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected skin surface")
    };
    assert_eq!(construction.surface_boolean, 1);
    assert_eq!(construction.surface_normal, 2);
    assert_eq!(construction.surface_direction, 3);
    assert_eq!(construction.count, 4);
    assert_eq!(construction.parameter, 0.25);
    assert!(matches!(
        construction.layout,
        SkinSurfaceLayout::Compact { .. }
    ));
    assert_eq!(construction.direction.z, 1.0);
    assert_eq!(construction.trailing_parameter, 0.75);
    assert_eq!(construction.formula.name, "skin-law");
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [LawExpression::Spline {
            native_id: 5,
            knots,
            controls,
            ..
        }] if knots == &[0.0, 0.5, 1.0] && controls == &[1.0, 2.0, 3.0]
    ));
    assert_eq!(construction.discontinuities[0], [0.1]);
    assert_eq!(construction.discontinuities[1], [0.2, 0.3]);
    assert!(construction.discontinuity_flag);

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less skin surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less skin surface round trip");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip skin surface")
    };
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [LawExpression::Spline { native_id: 5, .. }]
    ));
}

#[test]
fn generated_law_surfaces_decode_and_round_trip_modern_and_legacy_layouts() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    for (name, legacy_ranges) in [("law_spl_sur", false), ("lawsur", true)] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_law_spl_sur_smbh(
                    name,
                    legacy_ranges,
                    0,
                ))),
                &DecodeOptions::default(),
            )
            .expect("law surface decode");
        let ProceduralSurfaceDefinition::Law { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected law surface")
        };
        assert_eq!(
            construction.parameter_ranges,
            legacy_ranges.then_some([[-1.0, 2.0], [-3.0, 4.0]])
        );
        assert_eq!(construction.primary.name, "primary-law");
        assert!(matches!(
            construction.primary.variables.as_slice(),
            [LawExpression::Algebraic { operator, operands }]
                if operator == "SET" && operands.len() == 1
        ));
        assert_eq!(construction.additional.len(), 1);
        assert_eq!(construction.additional[0].name, "aux-law");
        assert!(matches!(
            construction.additional[0].variables.as_slice(),
            [LawExpression::Algebraic { operator, operands }]
                if operator == "TERM" && operands.len() == 2
        ));
        assert_eq!(construction.discontinuities[0], [0.1]);
        assert_eq!(construction.discontinuities[1], [0.2, 0.3]);
        assert_eq!(
            decoded.ir.model.procedural_surfaces[0].cache_fit_tolerance,
            Some(0.07)
        );

        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec.encode(&source_less, &mut encoded).unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let ProceduralSurfaceDefinition::Law { construction } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip law surface")
        };
        assert_eq!(
            construction.parameter_ranges,
            legacy_ranges.then_some([[-1.0, 2.0], [-3.0, 4.0]])
        );
        assert_eq!(construction.additional.len(), 1);
    }
}

#[test]
fn generated_sub_surfaces_decode_and_write_exact_support_graphs() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    for name in ["sub_spl_sur", "subsur"] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_sub_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let procedural = &decoded.ir.model.procedural_surfaces[0];
        let ProceduralSurfaceDefinition::SubSurface {
            support,
            parameter_ranges,
        } = &procedural.definition
        else {
            panic!("expected sub-surface")
        };
        assert_eq!(*parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
        assert!(matches!(
            decoded
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support)
                .map(|surface| &surface.geometry),
            Some(SurfaceGeometry::Plane { origin, .. })
                if *origin == cadmpeg_ir::math::Point3::new(1.0, -2.0, 3.0)
        ));
        assert!(matches!(
            decoded
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == procedural.surface)
                .map(|surface| &surface.geometry),
            Some(SurfaceGeometry::Procedural { .. })
        ));

        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec.encode(&source_less, &mut encoded).unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::SubSurface {
                parameter_ranges: [[-1.0, 2.0], [-3.0, 4.0]],
                ..
            }
        ));
    }
}

#[test]
fn generated_law_surfaces_round_trip_every_standard_tail_mode() {
    use cadmpeg_ir::geometry::{LawSurfaceTail, ProceduralSurfaceDefinition};

    for selector in 1..=4 {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_law_spl_sur_smbh(
                    "law_spl_sur",
                    false,
                    selector,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Law { construction } =
            &decoded.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected law surface")
        };
        assert!(match (&construction.tail, selector) {
            (
                LawSurfaceTail::Summary {
                    parameters,
                    fit_tolerance,
                    closures: [0, 2],
                    singularities: [1, 3],
                },
                1,
            ) => parameters[0] == [0.0, 0.5, 1.0] && *fit_tolerance == 0.08,
            (
                LawSurfaceTail::None {
                    parameter_ranges: [[-0.5, 1.5], [-2.0, 2.0]],
                    closures: [1, 2],
                    singularities: [0, 4],
                },
                2,
            ) => true,
            (LawSurfaceTail::Historical, 3) | (LawSurfaceTail::Optimal, 4) => true,
            _ => false,
        });
        assert_eq!(
            decoded.ir.model.procedural_surfaces[0].cache_fit_tolerance,
            None
        );
        assert!(matches!(
            decoded
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == decoded.ir.model.procedural_surfaces[0].surface)
                .map(|surface| &surface.geometry),
            Some(cadmpeg_ir::geometry::SurfaceGeometry::Procedural { .. })
        ));
        let expected_tail = construction.tail.clone();

        let mut source_less = decoded.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec.encode(&source_less, &mut encoded).unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let ProceduralSurfaceDefinition::Law { construction } =
            &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip law surface")
        };
        assert_eq!(construction.tail, expected_tail);
    }
}

#[test]
fn generated_skin_surface_round_trips_structural_law_nodes() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(1, false))),
            &DecodeOptions::default(),
        )
        .expect("skin structural-law decode");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected skin surface")
    };
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [
            LawExpression::Null,
            LawExpression::Transform {
                enums: [4, 5, 6],
                ..
            },
            LawExpression::Edge {
                parameters: [-0.25, 1.25],
                ..
            }
        ]
    ));
    let LawExpression::Edge { curve, .. } = &construction.formula.variables[2] else {
        unreachable!()
    };
    let law_edge = curve.clone();

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == law_edge)
        .expect("law edge curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(1.0, -2.0, 3.0),
        direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -1.0),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less structural-law encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less structural-law round trip");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip skin surface")
    };
    assert_eq!(construction.formula.variables.len(), 3);
    let LawExpression::Edge { curve, .. } = &construction.formula.variables[2] else {
        panic!("expected round-trip edge law")
    };
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|candidate| candidate.id == *curve)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [-0.25, -0.25, 1.25, 1.25]
    ));
}

#[test]
fn generated_skin_surface_round_trips_expanded_profiles() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SkinSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, true))),
            &DecodeOptions::default(),
        )
        .expect("expanded skin decode");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected skin surface")
    };
    let SkinSurfaceLayout::Profiles { profiles, tail, .. } = &construction.layout else {
        panic!("expected expanded skin profiles")
    };
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].type_code, 9);
    assert_eq!(profiles[0].data.asm_extension, -1);
    assert!(profiles[0].data.pcurve.is_some());
    assert!(profiles[0].data.direction.is_some());
    assert_eq!(*tail, [-1, 7]);
    let profile_curve = profiles[0].curve.clone();

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == profile_curve)
        .expect("skin profile curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(2.0, -1.0, 3.0),
        direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less expanded skin encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less expanded skin round trip");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip skin surface")
    };
    assert!(matches!(
        &construction.layout,
        SkinSurfaceLayout::Profiles { profiles, .. }
            if profiles.len() == 1 && profiles[0].data.direction.is_some()
    ));
    let SkinSurfaceLayout::Profiles { profiles, .. } = &construction.layout else {
        unreachable!()
    };
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == profiles[0].curve)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
    ));
}

#[test]
fn generated_skin_surface_round_trips_fixed_arity_algebraic_laws() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .expect("algebraic skin law decode");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected skin surface")
    };
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [
            LawExpression::Algebraic {
                operator,
                operands,
            },
            LawExpression::Algebraic {
                operator: dot,
                operands: vectors,
            }
        ] if operator == "SIN"
            && matches!(operands.as_slice(), [LawExpression::Algebraic { operator, operands }]
                if operator == "ABS"
                    && matches!(operands.as_slice(), [LawExpression::Double { value }] if *value == -2.5))
            && dot == "DOT"
            && matches!(vectors.as_slice(), [LawExpression::Vector { .. }, LawExpression::Vector { .. }])
    ));

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less algebraic skin encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less algebraic skin round trip");
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip skin surface")
    };
    assert_eq!(construction.formula.variables.len(), 2);
}

#[test]
fn source_less_writer_rejects_invalid_and_unframed_law_arities() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Skin { construction } =
        &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    construction.formula.variables[0] = LawExpression::Algebraic {
        operator: "SIN".into(),
        operands: Vec::new(),
    };
    let error = F3dCodec.encode(&source_less, &mut Vec::new()).unwrap_err();
    assert!(error.to_string().contains("requires 1 operands, got 0"));

    let ProceduralSurfaceDefinition::Skin { construction } =
        &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    construction.formula.variables[0] = LawExpression::Algebraic {
        operator: "MIN".into(),
        operands: vec![LawExpression::Double { value: 1.0 }],
    };
    let error = F3dCodec.encode(&source_less, &mut Vec::new()).unwrap_err();
    assert!(error.to_string().contains("unresolved variable arity"));
}

#[test]
fn generated_skin_surface_round_trips_set_rotate_and_term_laws() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Skin { construction } =
        &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    construction.formula.variables = vec![
        LawExpression::Algebraic {
            operator: "SET".into(),
            operands: vec![LawExpression::Double { value: -2.0 }],
        },
        LawExpression::Algebraic {
            operator: "ROTATE".into(),
            operands: vec![
                LawExpression::Vector {
                    value: Vector3::new(1.0, 2.0, 3.0),
                },
                LawExpression::Transform {
                    scalars: [0.0; 13],
                    enums: [0, 0, 0],
                },
            ],
        },
        LawExpression::Algebraic {
            operator: "TERM".into(),
            operands: vec![
                LawExpression::Vector {
                    value: Vector3::new(4.0, 5.0, 6.0),
                },
                LawExpression::Integer { value: 1 },
            ],
        },
    ];

    let mut encoded = Vec::new();
    F3dCodec.encode(&source_less, &mut encoded).unwrap();
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let ProceduralSurfaceDefinition::Skin { construction } =
        &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!()
    };
    assert!(matches!(
        construction.formula.variables.as_slice(),
        [
            LawExpression::Algebraic { operator: set, operands: set_operands },
            LawExpression::Algebraic { operator: rotate, operands: rotate_operands },
            LawExpression::Algebraic { operator: term, operands: term_operands },
        ] if set == "SET" && set_operands.len() == 1
            && rotate == "ROTATE" && rotate_operands.len() == 2
            && term == "TERM" && term_operands.len() == 2
    ));
}

#[test]
fn generated_g2_blend_surfaces_decode_both_singularity_branches() {
    use cadmpeg_ir::geometry::{G2BlendFirstShape, LoftBridgeToken, ProceduralSurfaceDefinition};

    for name in ["g2_blend_spl_sur", "g2blnsur"] {
        for full in [true, false] {
            let result = F3dCodec
                .decode(
                    &mut Cursor::new(f3d_with_smbh(&synthetic_g2_blend_spl_sur_smbh(name, full))),
                    &DecodeOptions::default(),
                )
                .expect("G2 blend decode");
            let ProceduralSurfaceDefinition::G2Blend { construction } =
                &result.ir.model.procedural_surfaces[0].definition
            else {
                panic!("expected G2 blend")
            };
            assert_eq!(construction.first.label, "first");
            assert_eq!(construction.second.label, "second");
            assert_eq!(construction.singularity, if full { 11 } else { 12 });
            assert_eq!(construction.center_parameters, [-0.5, 1.5]);
            assert_eq!(construction.parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
            assert_eq!(construction.trailing_parameters, [0.1, 0.2, 0.3, 0.4]);
            assert_eq!(
                construction.discontinuities,
                [vec![0.25], vec![], vec![0.5, 0.75]]
            );
            match &construction.first_shape {
                G2BlendFirstShape::Full { surface, tolerance } if full => {
                    assert!(surface.is_some());
                    assert_eq!(*tolerance, Some(0.02));
                }
                G2BlendFirstShape::None {
                    coefficients,
                    tolerance,
                    extension,
                    pcurve,
                } if !full => {
                    assert_eq!(*coefficients, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
                    assert_eq!(*tolerance, 0.03);
                    assert_eq!(*extension, Some(LoftBridgeToken::Integer(44)));
                    assert!(pcurve.is_some());
                }
                _ => panic!("wrong G2 singularity payload"),
            }
            let side_curves = [
                construction.first.curve.clone(),
                construction.second.curve.clone(),
            ];
            let center_curve = construction.center_curve.clone();

            let mut source_less = result.ir;
            source_less.source = None;
            source_less.set_native_unknowns("f3d", &[]).unwrap();
            for (ordinal, side) in side_curves.into_iter().enumerate() {
                source_less
                    .model
                    .curves
                    .iter_mut()
                    .find(|curve| curve.id == side)
                    .expect("G2 side curve")
                    .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                    origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, -1.0),
                    direction: cadmpeg_ir::math::Vector3::new(3.0, -2.0, 4.0),
                };
            }
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == center_curve)
                .expect("G2 center curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(-2.0, 1.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, -3.0, 2.0),
            };
            let mut encoded = Vec::new();
            F3dCodec
                .encode(&source_less, &mut encoded)
                .expect("source-less G2 encode");
            let round_trip = F3dCodec
                .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
                .expect("source-less G2 round trip");
            let ProceduralSurfaceDefinition::G2Blend { construction } =
                &round_trip.ir.model.procedural_surfaces[0].definition
            else {
                panic!("expected round-trip G2 blend")
            };
            assert_eq!(construction.singularity, if full { 11 } else { 12 });
            assert_eq!(construction.center_parameters, [-0.5, 1.5]);
            assert_eq!(construction.parameter_ranges, [[-1.0, 2.0], [-3.0, 4.0]]);
            assert_eq!(construction.discontinuities[2], [0.5, 0.75]);
            assert_eq!(
                matches!(construction.first_shape, G2BlendFirstShape::Full { .. }),
                full
            );
            for side in [&construction.first, &construction.second] {
                assert!(matches!(
                    round_trip
                        .ir
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == side.curve)
                        .map(|curve| &curve.geometry),
                    Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                        if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
                ));
            }
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == construction.center_curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1
                        && curve.knots == [-0.5, -0.5, 1.5, 1.5]
            ));
        }
    }
}

#[test]
fn generated_rolling_ball_and_sss_blends_decode_full_native_graphs() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, RollingBallRadiusSelector};

    for name in [
        "rb_blend_spl_sur",
        "rbblnsur",
        "pipe_spl_sur",
        "pipesur",
        "sss_blend_spl_sur",
        "sssblndsur",
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_full_rolling_ball_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("rolling-ball decode");
        let ProceduralSurfaceDefinition::Blend {
            native: Some(native),
            ..
        } = &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected complete rolling-ball graph")
        };
        assert_eq!(native.definition_index, 22507);
        assert_eq!(
            native.sides[0].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Surface
        );
        assert_eq!(
            native.sides[1].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Curve
        );
        assert_eq!(
            native.sides[0].location,
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
        );
        assert!(native.sides.iter().all(|side| side.surface.is_some()));
        assert!(native.sides.iter().all(|side| side.pcurve.is_some()));
        assert_eq!(native.sides[0].extension, Some(3));
        assert_eq!(native.sides[1].extension, Some(4));
        assert_eq!(native.offsets, [-3.0, -6.0]);
        assert_eq!(native.radius_selector, RollingBallRadiusSelector::None);
        assert_eq!(native.u_range, [Some(-1.0), Some(2.0)]);
        assert_eq!(native.v_range, [None, None]);
        assert_eq!(native.shape_prefix, 1);
        assert_eq!(native.parameters, [0.1, 0.2]);
        assert_eq!(native.tail, 17);
        assert_eq!(native.cache_selector, 0);
        assert_eq!(
            native.discontinuities,
            [vec![0.25], vec![], vec![0.5, 0.75]]
        );
        assert_eq!(native.third.is_some(), name.starts_with("sss"));
        if let Some(third) = &native.third {
            assert_eq!(third.label, "third");
            assert_eq!(third.extension, 23);
            assert!(third.secondary_pcurve.is_some());
            assert!(!third.flag);
        }

        let expected = native.clone();
        let side_curves = native
            .sides
            .iter()
            .map(|side| side.curve.clone())
            .collect::<Vec<_>>();
        let third_curve = native.third.as_ref().map(|third| third.curve.clone());
        let slice_curve = native.slice.clone();
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, side) in side_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| Some(&curve.id) == side.as_ref())
                .expect("rolling-ball side curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 3.0, -2.0),
                direction: cadmpeg_ir::math::Vector3::new(4.0, -1.0, 2.0),
            };
        }
        if let Some(third) = &third_curve {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == *third)
                .expect("rolling-ball third-side curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(3.0, 4.0, -2.0),
            };
        }
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == slice_curve)
            .expect("rolling-ball slice curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, -3.0, 1.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less rolling-ball encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less rolling-ball round trip");
        let ProceduralSurfaceDefinition::Blend {
            native: Some(actual),
            ..
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected complete round-trip rolling-ball graph")
        };
        assert_eq!(actual.as_ref(), expected.as_ref());
        for side in actual.sides.iter() {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| Some(&curve.id) == side.curve.as_ref())
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        if let Some(third) = &actual.third {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == third.curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == actual.slice)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [-1.0, -1.0, 2.0, 2.0]
        ));
    }
}

#[test]
fn generated_variable_blends_decode_complete_single_radius_graphs() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendValuePayload};

    for name in [
        "var_blend_spl_sur",
        "varblendsplsur",
        "srf_srf_v_bl_spl_sur",
        "srfsrfblndsur",
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("variable-blend decode");
        let ProceduralSurfaceDefinition::VariableBlend { construction } =
            &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected variable blend")
        };
        assert_eq!(construction.revision, 23100);
        assert_eq!(
            construction.sides[0].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Surface
        );
        assert_eq!(
            construction.sides[1].support_kind,
            cadmpeg_ir::geometry::VariableBlendSupportKind::Curve
        );
        assert_eq!(construction.sides[0].extension, Some(0));
        assert_eq!(construction.sides[1].extension, Some(5));
        assert_eq!(
            construction.sides[0].location,
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
        );
        assert_eq!(construction.offsets, [-2.0, 4.0]);
        assert_eq!(
            construction.radius_kind,
            cadmpeg_ir::geometry::VariableBlendRadiusKind::SingleRadius
        );
        let VariableBlendValuePayload::TwoEnds { parameters, radii } =
            &construction.first_value.payload
        else {
            panic!("expected two-ends radius law")
        };
        assert!(construction.first_value.modern_flag);
        assert_eq!(construction.first_value.discriminator, 7);
        assert_eq!(construction.first_value.calibrated, 3);
        assert_eq!(*parameters, [0.25, 0.75]);
        assert_eq!(*radii, [15.0, 25.0]);
        assert_eq!(construction.slice_range, [None, None]);
        assert_eq!(construction.u_range, [Some(-1.0), Some(2.0)]);
        assert_eq!(construction.v_range, [None, None]);
        assert_eq!(construction.shape_prefix, 11);
        assert_eq!(construction.shape_length, 6.0);
        assert_eq!(construction.cache_selector, 0);
        assert_eq!(
            construction.discontinuities,
            [
                vec![0.125],
                vec![],
                vec![0.25, 0.375],
                vec![],
                vec![0.5],
                vec![]
            ]
        );
        assert!(construction.tail_flag);
        assert_eq!(construction.tail_extensions, [31, 32, 33]);
        assert!(construction.secondary_curve.is_some());
        assert_eq!(construction.secondary_range, [None, None]);
        assert_eq!(
            construction.convexity,
            cadmpeg_ir::geometry::VariableBlendConvexity::Convex
        );
        assert_eq!(
            construction.render_mode,
            cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallSnapshot
        );
        assert_eq!(construction.post_range, [Some(0.0), Some(1.0)]);
        assert!(construction.post_curve.is_some());
        assert!(construction.post_pcurve.is_none());
        assert!(construction.sides.iter().all(|side| side.pcurve.is_some()));

        let expected = construction.clone();
        let post_curve = construction.post_curve.clone().expect("post curve");
        let slice_curve = construction.slice.clone();
        let side_curves = construction
            .sides
            .iter()
            .map(|side| side.curve.clone().expect("side curve"))
            .collect::<Vec<_>>();
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == post_curve)
            .expect("variable-blend post curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(-2.0, 1.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(3.0, -4.0, 2.0),
        };
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == slice_curve)
            .expect("variable-blend slice curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(3.0, -2.0, 1.0),
            direction: cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0),
        };
        for (ordinal, side) in side_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == *side)
                .expect("variable-blend side curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, -1.0, 2.0),
                direction: cadmpeg_ir::math::Vector3::new(2.0, 3.0, -4.0),
            };
        }
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less variable-blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less variable-blend round trip");
        let ProceduralSurfaceDefinition::VariableBlend {
            construction: actual,
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip variable blend")
        };
        assert_eq!(actual.as_ref(), expected.as_ref());
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| Some(&curve.id) == actual.post_curve.as_ref())
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
        ));
        for side in actual.sides.iter() {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|curve| Some(&curve.id) == side.curve.as_ref())
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1 && curve.knots == [0.0, 0.0, 1.0, 1.0]
            ));
        }
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == actual.slice)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1 && curve.knots == [-1.0, -1.0, 2.0, 2.0]
        ));
    }
}

#[test]
fn generated_variable_blend_rejects_cross_branch_radius_payloads() {
    use cadmpeg_ir::geometry::{
        LoftBridgeToken, ProceduralSurfaceDefinition, VariableBlendRadiusKind,
        VariableBlendSingleRadiusTail,
    };

    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh(
                "var_blend_spl_sur",
            ))),
            &DecodeOptions::default(),
        )
        .expect("variable-blend decode")
        .ir;
    decoded.source = None;
    decoded.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &mut decoded.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    construction.radius_kind = VariableBlendRadiusKind::TwoRadii;
    construction.second_value = Some(construction.first_value.clone());
    construction.single_radius_tail = Some(VariableBlendSingleRadiusTail {
        selector: LoftBridgeToken::Integer(1),
        parameters: [0.25, 0.75],
    });

    assert!(cadmpeg_ir::validate(&decoded, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "variable blend construction payload is invalid"));
    let error = F3dCodec.encode(&decoded, &mut Vec::new()).unwrap_err();
    assert!(error
        .to_string()
        .contains("two-radii variable blend carries a single-radius tail"));
}

#[test]
fn generated_two_radii_variable_blend_round_trips_rounded_chamfer() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendRadiusKind, VariableBlendValuePayload,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_branch(
                "var_blend_spl_sur",
                true,
            ))),
            &DecodeOptions::default(),
        )
        .expect("two-radii variable-blend decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.radius_kind, VariableBlendRadiusKind::TwoRadii);
    assert!(matches!(
        construction
            .second_value
            .as_ref()
            .map(|value| &value.payload),
        Some(VariableBlendValuePayload::TwoEnds {
            parameters: [0.1, 0.9],
            radii: [35.0, 45.0]
        })
    ));
    let chamfer = construction.chamfer.as_ref().expect("rounded chamfer");
    assert_eq!(
        chamfer.kind,
        cadmpeg_ir::geometry::VariableBlendChamferKind::Rounded
    );
    assert_eq!(chamfer.chamfer_type, 2);
    assert!(matches!(
        &chamfer.value.payload,
        VariableBlendValuePayload::TwoEnds {
            parameters: [0.0, 1.0],
            radii: [55.0, 65.0]
        }
    ));
    assert!(construction.single_radius_tail.is_none());

    let expected = construction.clone();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("two-radii variable-blend source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("two-radii variable-blend round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction == &expected
    ));
}

#[test]
fn generated_two_radii_variable_blend_consumes_zero_chamfer_selector() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendRadiusKind};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                true,
                Some(0),
            ))),
            &DecodeOptions::default(),
        )
        .expect("two-radii selector-zero decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.radius_kind, VariableBlendRadiusKind::TwoRadii);
    assert_eq!(construction.chamfer_selector, Some(0));
    assert!(construction.chamfer.is_none());
    let expected = construction.clone();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("selector-zero source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("selector-zero round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction == &expected
    ));
}

fn push_optional_value_quartet(surface: &mut Vec<u8>) {
    for value in [1.0, 0.0, 1.0, 0.0] {
        surface.push(0x0a);
        t_dbl(surface, value);
    }
}

#[test]
fn generated_revision_exact_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
    });
    assert_revision_surface_round_trip(smbh, "exact");
}

#[test]
fn generated_revision_sum_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("sum_spl_sur", |surface| {
        for (lower, upper) in [(0.0, 1.0), (-2.0, 2.0)] {
            surface.extend_from_slice(&generated_curve_block());
            surface.push(0x0a);
            t_dbl(surface, lower);
            surface.push(0x0a);
            t_dbl(surface, upper);
        }
        t_pos(surface, [1.0, 2.0, 3.0]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "sum");
}

#[test]
fn generated_revision_rot_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("rot_spl_sur", |surface| {
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        t_pos(surface, [0.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "revolution");
}

#[test]
fn generated_revision_t_spline_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("t_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
        surface.push(0x0f);
        t_ident(surface, "t_spl_subtrans_object");
        t_u16_string(
            surface,
            "degree 3\nunits mm\nv 1 0 0 0\nv 2 1 0 0\ne 1 1 2\n",
        );
        surface.push(0x0b);
        t_u16_string(surface, "100verts 1 2\n");
        surface.push(0x10);
        t_long(surface, 2);
    });
    assert_revision_surface_round_trip(smbh, "t_spline");
}

#[test]
fn generated_revision_g2_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("g2_blend_spl_sur", |surface| {
        t_dbl(surface, 1.0);
        t_dbl(surface, 1.0);
        append_generated_variable_blend_side(surface, "left", 1.0);
        append_generated_variable_blend_side(surface, "right", 4.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.5);
        surface.push(0x0a);
        t_dbl(surface, 2.5);
        t_dbl(surface, 0.125);
        t_dbl(surface, 0.125);
        push_tagged_i64(surface, 0x15, -1);
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 1);
        t_dbl(surface, 0.001);
        t_dbl(surface, 0.0001);
        t_long(surface, 1);
        push_revision_surface_tail(surface);
        for value in [0, 0, 0] {
            t_long(surface, value);
        }
    });
    assert_revision_surface_round_trip(smbh, "revision_g2_blend");
}

#[test]
fn generated_revision_vertex_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("VBL_SURF", |surface| {
        t_long(surface, 2);

        t_ident(surface, "circle");
        surface.push(0x0a);
        t_vec(surface, [0.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.1);
        surface.push(0x0a);
        t_dbl(surface, 0.9);
        push_tagged_i64(surface, 0x15, 3);
        t_vec(surface, [0.0, 0.0, 0.5]);
        t_vec(surface, [0.5, 0.0, 0.0]);
        t_dbl(surface, 0.1);
        t_dbl(surface, 0.9);
        surface.push(0x0b);

        t_ident(surface, "pcurve");
        surface.push(0x0b);
        t_vec(surface, [0.0, 0.0, 0.0]);
        surface.push(0x0a);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        t_ident(surface, "plane");
        t_pos(surface, [0.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        t_vec(surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_pcurve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.002);

        t_long(surface, 9);
        t_dbl(surface, 0.003);
    });
    assert_revision_surface_round_trip(smbh, "vertex_blend");
}

#[test]
fn generated_revision_offset_with_inline_untyped_support_decodes() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.push(0x0b);
        surface.push(0x0f);
        t_ident(surface, "mystery_spl_sur");
        t_long(surface, 23100);
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x10);
        surface.extend_from_slice(&[0x0b; 4]);
        t_dbl(surface, 0.3);
        surface.extend_from_slice(&[0x0b; 4]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "offset");
}

#[test]
fn generated_single_radius_variable_blend_consumes_zero_selector() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                false,
                Some(0),
            ))),
            &DecodeOptions::default(),
        )
        .expect("single-radius selector-zero decode");
    let ProceduralSurfaceDefinition::VariableBlend { construction } =
        &decoded.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected variable blend")
    };
    assert_eq!(construction.single_radius_selector, Some(0));
    assert!(construction.single_radius_tail.is_none());
    let expected = construction.clone();
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("selector-zero source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("selector-zero round trip");
    assert!(matches!(
        &round_trip.ir.model.procedural_surfaces[0].definition,
        ProceduralSurfaceDefinition::VariableBlend { construction }
            if construction == &expected
    ));
}

fn push_revision_cl_scale(surface: &mut Vec<u8>, with_path: bool) {
    // One member: type, curve, endpoints, support, pcurve, flags, subdata.
    t_long(surface, 1);
    t_long(surface, 1);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x0a);
    t_dbl(surface, 0.0);
    surface.push(0x0a);
    t_dbl(surface, 1.0);
    t_ident(surface, "null_surface");
    t_ident(surface, "nullbs");
    surface.push(0x0b);
    t_long(surface, -1);
    // Subdata type 213 with one row and one column: leading pair plus
    // `column_count + 1` trailing pairs in the revision encoding.
    t_long(surface, 213);
    t_long(surface, 1);
    t_long(surface, 1);
    for value in [0.0, 1.0, -0.5, 0.25, 0.75, 0.75] {
        t_dbl(surface, value);
    }
    surface.push(0x0b);
    if with_path {
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
    } else {
        t_ident(surface, "null_curve");
    }
    t_long(surface, 0);
    t_long(surface, -1);
}

#[test]
fn generated_revision_compound_loft_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, true);
        t_long(surface, 2);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 0.0);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0b);
        surface.push(0x0b);
    });
    assert_revision_surface_round_trip(smbh, "revision_compound_loft");
}

#[test]
fn generated_revision_compound_loft_trailing_curve_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, false);
        t_long(surface, 1);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.extend_from_slice(&generated_curve_block());
    });
    assert_revision_surface_round_trip(smbh, "revision_compound_loft");
}

#[test]
fn record_level_surface_bounds_round_trip() {
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("exact revision decode");
    let mut source_less = decoded.ir;
    assert_eq!(source_less.model.procedural_surfaces[0].record_bounds, None);
    source_less.model.procedural_surfaces[0].record_bounds =
        Some([Some(0.1), None, Some(0.2), None]);
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("record-bounds encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("record-bounds round trip");
    assert_eq!(
        round_trip.ir.model.procedural_surfaces[0].record_bounds,
        Some([Some(0.1), None, Some(0.2), None])
    );
}

#[test]
fn generated_vertex_blends_decode_all_boundary_variants() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, SurfaceGeometry, VertexBlendBoundaryGeometry,
    };

    for name in ["VBL_SURF", "vertexblendsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_vertex_blend_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("vertex-blend decode");
        let ProceduralSurfaceDefinition::VertexBlend { construction } =
            &result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected vertex blend")
        };
        let owner = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == result.ir.model.procedural_surfaces[0].surface)
            .expect("vertex-blend owner");
        assert!(
            matches!(
                owner.geometry,
                SurfaceGeometry::Procedural { ref construction }
                    if *construction == result.ir.model.procedural_surfaces[0].id
            ),
            "unexpected vertex-blend carrier: {:?}",
            owner.geometry
        );
        assert_eq!(construction.boundaries.len(), 4);
        assert_eq!(construction.grid_size, 17);
        assert_eq!(construction.fit_tolerance, 0.03);
        let VertexBlendBoundaryGeometry::Circle {
            form,
            twists,
            parameters,
            sense,
            ..
        } = &construction.boundaries[0].geometry
        else {
            panic!("expected circle boundary")
        };
        assert_eq!(*form, 1);
        assert_eq!(twists, &[cadmpeg_ir::math::Point3::new(20.0, 30.0, 40.0)]);
        assert_eq!(*parameters, [0.1, 0.9]);
        assert_eq!(*sense, 0);
        assert!(matches!(
            construction.boundaries[1].geometry,
            VertexBlendBoundaryGeometry::Degenerate { .. }
        ));
        assert!(matches!(
            construction.boundaries[2].geometry,
            VertexBlendBoundaryGeometry::Pcurve {
                pcurve: Some(_),
                ..
            }
        ));
        assert!(matches!(
            construction.boundaries[3].geometry,
            VertexBlendBoundaryGeometry::Plane { .. }
        ));
        let bounded_curves =
            [0usize, 3].map(|ordinal| match &construction.boundaries[ordinal].geometry {
                VertexBlendBoundaryGeometry::Circle {
                    curve, parameters, ..
                }
                | VertexBlendBoundaryGeometry::Plane {
                    curve, parameters, ..
                } => (curve.clone(), *parameters),
                _ => unreachable!(),
            });

        let expected = construction.clone();
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, (curve, _)) in bounded_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|candidate| candidate.id == *curve)
                .expect("vertex-blend boundary curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, -3.0),
                direction: cadmpeg_ir::math::Vector3::new(2.0, -1.0, 4.0),
            };
        }
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less vertex-blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less vertex-blend round trip");
        let ProceduralSurfaceDefinition::VertexBlend {
            construction: actual,
        } = &round_trip.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected round-trip vertex blend")
        };
        assert_eq!(actual.as_ref(), expected.as_ref());
        for (curve, range) in bounded_curves {
            assert!(matches!(
                round_trip
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|candidate| candidate.id == curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree == 1
                        && curve.knots == [range[0], range[0], range[1], range[1]]
            ));
        }
    }
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
        parameter_interval,
        native_position,
    } = &procedural.definition
    else {
        panic!("expected extrusion")
    };
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
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
fn decode_retains_versioned_nested_translational_extrusion() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_versioned_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("versioned extrusion decode");
    let procedural = result.ir.model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance, Some(0.02));
    let ProceduralSurfaceDefinition::Extrusion {
        direction,
        parameter_interval,
        native_position,
        ..
    } = &procedural.definition
    else {
        panic!("expected versioned extrusion")
    };
    assert_eq!(*parameter_interval, Some([0.25, 0.75]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
}

#[test]
fn generated_f3d_rewrites_translational_extrusion_header() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let mut edited = decoded.ir;
    let ProceduralSurfaceDefinition::Extrusion {
        parameter_interval,
        direction,
        native_position,
        ..
    } = &mut edited.model.procedural_surfaces[0].definition
    else {
        panic!("expected extrusion")
    };
    *parameter_interval = Some([-0.5, 1.25]);
    *direction = cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0);
    *native_position = Some(cadmpeg_ir::math::Point3::new(-20.0, 70.0, 15.0));

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("extrusion-direction regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let ProceduralSurfaceDefinition::Extrusion {
        parameter_interval,
        direction,
        native_position,
        ..
    } = &round_trip.ir.model.procedural_surfaces[0].definition
    else {
        panic!("expected round-trip extrusion")
    };
    assert_eq!(*parameter_interval, Some([-0.5, 1.25]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0));
    assert_eq!(
        *native_position,
        Some(cadmpeg_ir::math::Point3::new(-20.0, 70.0, 15.0))
    );
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        ..
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
fn generated_solved_plane_plane_blend_decodes_as_analytic_cylinder() {
    use cadmpeg_ir::geometry::{
        BlendRadiusLaw, CurveGeometry, NurbsCurve, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated rolling-ball decode");
    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralSurfaceDefinition::Blend {
        supports,
        spine: Some(spine),
        radius,
        ..
    } = &mut source_less.model.procedural_surfaces[0].definition
    else {
        panic!("expected rolling-ball definition")
    };
    let support_ids = [
        supports[0].as_ref().expect("first support").surface.clone(),
        supports[1]
            .as_ref()
            .expect("second support")
            .surface
            .clone(),
    ];
    let spine_id = spine.clone();
    *radius = BlendRadiusLaw::Constant {
        signed_radius: -2.0,
    };
    let support_geometry = [
        SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
            u_axis: Vector3::new(0.0, 1.0, 0.0),
        },
        SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
    ];
    for (id, geometry) in support_ids.into_iter().zip(support_geometry) {
        source_less
            .model
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == id)
            .expect("rolling-ball support")
            .geometry = geometry;
    }
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine_id)
        .expect("rolling-ball spine")
        .geometry = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(2.0, 2.0, -4.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(2.0, 2.0, 7.0),
        ],
        weights: None,
        periodic: false,
    });

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less rolling-ball encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rolling-ball round trip");
    let carrier_id = &round_trip.ir.model.procedural_surfaces[0].surface;
    assert!(matches!(
        round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| &surface.id == carrier_id)
            .expect("rolling-ball carrier")
            .geometry,
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } if origin == Point3::new(2.0, 2.0, -4.0)
            && axis == Vector3::new(0.0, 0.0, 1.0)
            && radius == 2.0
    ));
}

#[test]
fn generated_rolling_ball_surface_aliases_decode_and_write_canonically() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["rbblnsur", "pipe_spl_sur", "pipesur"] {
        let bytes =
            with_legacy_subtype(synthetic_rb_blend_spl_sur_smbh(), "rb_blend_spl_sur", name);
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("rolling-ball alias decode");
        assert!(matches!(
            result.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Blend { .. }
        ));
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("canonical rolling-ball encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("canonical rolling-ball round trip");
        assert!(matches!(
            round_trip.ir.model.procedural_surfaces[0].definition,
            ProceduralSurfaceDefinition::Blend { .. }
        ));
    }
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
    use crate::nurbs::core::decode_surface_cache_resolving_refs;

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
    let decoded = decode_surface_cache_resolving_refs(
        &source,
        &active,
        &crate::nurbs::subtypes::SubtypeTables::from_stream(&active),
    )
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
    let color = crate::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
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
    let color = crate::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r, color.g, color.b, color.a),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 128.0 / 255.0)
    );
}

#[test]
fn bt_text_color_attribute_chain_decodes_rgb() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "entatt_color");
    t_subident(&mut bytes, "bt");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    push_u8_string(&mut bytes, "4227264"); // 0x4080c0
    t_end(&mut bytes);

    let records = crate::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color = crate::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r, color.g, color.b, color.a),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
    );
}

#[test]
fn bt_text_color_rejects_non_decimal_and_overwide_values() {
    use std::collections::HashMap;

    for value in ["0x4080c0", "16777216"] {
        let mut bytes = Vec::new();
        t_ident(&mut bytes, "face");
        t_ref(&mut bytes, 1);
        t_end(&mut bytes);
        t_subident(&mut bytes, "entatt_color");
        t_subident(&mut bytes, "bt");
        t_ident(&mut bytes, "attrib");
        t_ref(&mut bytes, -1);
        push_u8_string(&mut bytes, value);
        t_end(&mut bytes);

        let records = crate::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
        let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
        assert!(crate::brep::attributes::attribute_chain_color(&records[0], &by_index).is_none());
    }
}

#[test]
fn invalid_color_attribute_does_not_hide_later_chain_color() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "entatt_color");
    t_subident(&mut bytes, "bt");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, 2);
    push_u8_string(&mut bytes, "not-a-color");
    t_end(&mut bytes);
    t_subident(&mut bytes, "rgb_color");
    t_subident(&mut bytes, "st");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    t_dbl(&mut bytes, 0.1);
    t_dbl(&mut bytes, 0.2);
    t_dbl(&mut bytes, 0.3);
    t_end(&mut bytes);

    let records = crate::sab::frame(&bytes, 0, bytes.len(), 8).unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color = crate::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!((color.r, color.g, color.b, color.a), (0.1, 0.2, 0.3, 1.0));
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
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let transform = crate::brep::attributes::decode_transform(&record, 60.0).unwrap();
    assert_eq!(transform.rows[0], [1.0, 0.0, 0.0, 600.0]);
    assert_eq!(transform.rows[1], [0.0, 1.0, 0.0, 1200.0]);
    assert_eq!(transform.rows[2], [0.0, 0.0, 1.0, 1800.0]);
    assert_eq!(transform.rows[3], [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn nurbs_curve_block_decodes_to_carrier() {
    use crate::nurbs::core::decode_curve_cache;

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
        &procedural.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown {
            native_kind: Some(native_kind),
            record: None,
        } if native_kind == "surf_surf_int_cur"
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
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
fn cacheless_helix_construction_is_the_exact_edge_carrier() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cacheless_helix_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("cacheless helix decode");
    let procedural = result
        .ir
        .model
        .procedural_curves
        .first()
        .expect("helix construction");
    assert!(matches!(
        procedural.definition,
        ProceduralCurveDefinition::Helix { .. }
    ));
    assert_eq!(procedural.cache_fit_tolerance, None);
    assert!(matches!(
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == procedural.curve)
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Procedural { construction }) if *construction == procedural.id
    ));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("procedural intcurve")));

    let expected = procedural.definition.clone();
    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("cacheless helix source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("cacheless helix source-less round trip");
    assert!(matches!(
        round_trip.ir.model.curves[0].geometry,
        CurveGeometry::Procedural { .. }
    ));
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        expected
    );
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        None
    );
}

#[test]
fn generated_law_intcurve_decodes_and_writes_recursive_formulas() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralCurveDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_law_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("law intcurve decode");
    let procedural = decoded
        .ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition, ProceduralCurveDefinition::Law { .. }))
        .expect("law intcurve construction");
    let ProceduralCurveDefinition::Law {
        context,
        extension,
        primary,
        additional,
    } = &procedural.definition
    else {
        unreachable!()
    };
    assert_eq!(context.parameter_range, [-1.0, 2.0]);
    assert_eq!(*extension, 0);
    assert_eq!(primary.name, "primary_law");
    assert!(matches!(
        primary.variables[0],
        LawExpression::Edge { parameters, .. } if parameters == [-0.5, 1.5]
    ));
    assert_eq!(additional.len(), 2);

    let mut source_less = decoded.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less law intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less law intcurve round trip");
    assert!(round_trip.ir.model.procedural_curves.iter().any(|curve| {
        matches!(
            &curve.definition,
            ProceduralCurveDefinition::Law { primary, .. }
                if matches!(primary.variables[0], LawExpression::Edge { .. })
        )
    }));
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
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let source_id = match &source_less.model.procedural_curves[0].definition {
        ProceduralCurveDefinition::VectorOffset { source, .. } => source.clone(),
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == source_id)
        .expect("vector-offset source carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(-5.0, 4.0, 2.0),
        direction: cadmpeg_ir::math::Vector3::new(2.0, 1.0, -0.5),
    };
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
    assert!(matches!(
        round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
            if curve.degree == 1
                && curve.knots == [-2.0, -2.0, 5.0, 5.0]
                && curve.control_points == [
                    cadmpeg_ir::math::Point3::new(-9.0, 2.0, 3.0),
                    cadmpeg_ir::math::Point3::new(5.0, 9.0, -0.5),
                ]
    ));
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
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let source_id = match &source_less.model.procedural_curves[0].definition {
        ProceduralCurveDefinition::Subset { source, .. } => source.clone(),
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == source_id)
        .expect("subset source carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
        origin: cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0),
        direction: cadmpeg_ir::math::Vector3::new(1.0, -2.0, 0.5),
    };
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
    let source_curve = round_trip
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *source)
        .expect("round-trip subset source");
    assert_eq!(
        source_curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![-1.5, -1.5, 3.5, 3.5],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(8.5, 23.0, 29.25),
                cadmpeg_ir::math::Point3::new(13.5, 13.0, 31.75),
            ],
            weights: None,
            periodic: false,
        })
    );
}

#[test]
fn generated_exact_intcurve_preserves_native_construction_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_exact_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated exact intcurve decode");
    assert_eq!(
        result.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Exact
    );
    assert_eq!(
        result.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.004)
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less exact intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less exact intcurve round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Exact
    );
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].cache_fit_tolerance,
        Some(0.004)
    );
}

#[test]
fn generated_spline_carriers_write_explicit_forward_sense() {
    for (smbh, head) in [
        (synthetic_geometry_with_exact_curve_smbh(), "intcurve"),
        (synthetic_exact_spl_sur_smbh("exact_spl_sur"), "spline"),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated spline carrier decode");
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();

        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less spline carrier encode");
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
        let mut generated_smbh = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
            .expect("generated BREP stream")
            .read_to_end(&mut generated_smbh)
            .expect("generated BREP bytes");
        let record_start = generated_smbh
            .windows(b"\x0d\x09asmheader".len())
            .position(|window| window == b"\x0d\x09asmheader")
            .expect("generated ASM record table");
        let records = crate::sab::frame(&generated_smbh, record_start, generated_smbh.len(), 8)
            .expect("generated ASM records must frame");
        let record = records
            .iter()
            .find(|record| record.head == head)
            .expect("generated spline carrier record");
        let subtype = record
            .tokens
            .iter()
            .position(|token| matches!(token, crate::sab::Token::SubtypeOpen))
            .expect("spline carrier subtype scope");
        assert!(subtype > 0);
        assert_eq!(record.tokens[subtype - 1], crate::sab::Token::False);
    }
}

#[test]
fn generated_intcurve_sense_uses_token_adjacent_to_subtype() {
    let decode_curve = |smbh: Vec<u8>| {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated exact intcurve decode");
        let curve_id = &result.ir.model.procedural_curves[0].curve;
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *curve_id)
            .expect("exact intcurve carrier")
            .geometry
            .clone()
    };

    assert_eq!(
        decode_curve(synthetic_geometry_with_decoy_curve_sense_smbh()),
        decode_curve(synthetic_geometry_with_exact_curve_smbh())
    );
}

#[test]
fn generated_spline_surface_sense_uses_token_adjacent_to_subtype() {
    let decode_surface = |smbh: Vec<u8>| {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated exact spline-surface decode");
        let surface_id = &result.ir.model.procedural_surfaces[0].surface;
        let geometry = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .expect("exact spline-surface carrier")
            .geometry
            .clone();
        let face_sense = result
            .ir
            .model
            .faces
            .iter()
            .find(|face| face.surface == *surface_id)
            .expect("spline-surface face")
            .sense;
        (geometry, face_sense)
    };

    assert_eq!(
        decode_surface(synthetic_exact_spl_sur_with_decoy_sense_smbh()),
        decode_surface(synthetic_exact_spl_sur_smbh("exact_spl_sur"))
    );
}

#[test]
fn generated_legacy_intcurve_aliases_decode_and_write_canonically() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let cases = [
        with_legacy_subtype(
            synthetic_geometry_with_exact_curve_smbh(),
            "exact_int_cur",
            "exactcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_vector_offset_curve_smbh(),
            "offset_int_cur",
            "offsetintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_subset_curve_smbh(),
            "subset_int_cur",
            "subsetintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_analytic_offset_supports_smbh(),
            "off_int_cur",
            "offintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_offset_smbh(),
            "off_surf_int_cur",
            "offsurfintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_projection_smbh(),
            "proj_int_cur",
            "projcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_intersection_smbh(),
            "int_int_cur",
            "surfintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_spring_smbh(),
            "spring_int_cur",
            "blndsprngcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("blend_int_cur"),
            "blend_int_cur",
            "bldcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("surf_int_cur"),
            "surf_int_cur",
            "surfcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("par_int_cur"),
            "par_int_cur",
            "parcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("skin_int_cur"),
            "skin_int_cur",
            "d5c2_cur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_silhouette_smbh("para_silh_int_cur", None),
            "para_silh_int_cur",
            "parasil",
        ),
    ];

    for bytes in cases {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("legacy intcurve alias decode");
        assert!(!matches!(
            result.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::Unknown { .. }
        ));
        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("canonical source-less intcurve encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("canonical intcurve round trip");
        assert!(!matches!(
            round_trip.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::Unknown { .. }
        ));
    }
}

#[test]
fn generated_compound_intcurve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_compound_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated compound intcurve decode");
    let ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        components,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected compound construction")
    };
    assert_eq!(parameters, &[0.0, 0.5, 1.0]);
    assert_eq!(component_parameters, &[-2.0, 4.0]);
    assert_eq!(components.len(), 2);
    assert!(components.iter().all(|component| result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *component)));
    assert!(
        (result.ir.model.procedural_curves[0]
            .cache_fit_tolerance
            .expect("compound fit tolerance")
            - 0.003)
            .abs()
            < 1e-12
    );
    let component_ids = components.clone();

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    *parameters = vec![-0.25, 0.75, 1.25];
    *component_parameters = vec![-3.0, 5.0];
    let expected_edit = edited.model.procedural_curves[0].definition.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("compound intcurve regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated compound intcurve decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        expected_edit
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    for (ordinal, component) in component_ids.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *component)
            .expect("compound component curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(ordinal as f64, -1.0, 2.0),
            direction: cadmpeg_ir::math::Vector3::new(2.0, 3.0, -4.0),
        };
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less compound intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound intcurve round trip");
    let ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        components,
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip compound construction")
    };
    assert_eq!(parameters, &[0.0, 0.5, 1.0]);
    assert_eq!(component_parameters, &[-2.0, 4.0]);
    assert_eq!(components.len(), 2);
    for (ordinal, component) in components.iter().enumerate() {
        let curve = round_trip
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *component)
            .expect("round-trip compound component");
        let cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve) = &curve.geometry else {
            panic!("compound line component was not lowered to NURBS")
        };
        assert_eq!(curve.degree, 1);
        let range = [ordinal as f64 * 0.5, (ordinal + 1) as f64 * 0.5];
        assert_eq!(curve.knots, [range[0], range[0], range[1], range[1]]);
    }
}

#[test]
fn generated_two_sided_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_two_sided_offset_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated two-sided offset decode");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected two-sided offset construction")
    };
    assert_eq!(context.parameter_range, [-1.0, 2.0]);
    assert!(*discontinuity_flag);
    assert_eq!(
        context.discontinuities,
        [vec![0.25, 0.75], vec![], vec![0.5]]
    );
    assert!(context
        .sides
        .iter()
        .all(|side| side.surface.is_none() && side.pcurve.is_none()));
    assert_eq!(*offsets, [-2.0, 4.0]);

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-2.0, 3.0];
    context.discontinuities = [vec![0.2, 0.8], vec![], vec![0.6]];
    *discontinuity_flag = false;
    *offsets = [-3.0, 5.0];
    let expected_edit = edited.model.procedural_curves[0].definition.clone();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("two-sided offset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated two-sided offset decode");
    assert_eq!(
        regenerated.ir.model.procedural_curves[0].definition,
        expected_edit
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less two-sided offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-sided offset round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_embedded_offset_supports_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{PcurveGeometry, ProceduralCurveDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_embedded_offset_supports_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("embedded offset-support decode");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context, offsets, ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected embedded two-sided offset")
    };
    assert_eq!(*offsets, [-1.0, 3.0]);
    for side in &context.sides {
        let surface_id = side.surface.as_ref().expect("embedded support surface");
        assert!(result.ir.model.surfaces.iter().any(|surface| {
            surface.id == *surface_id && matches!(surface.geometry, SurfaceGeometry::Nurbs(_))
        }));
        assert!(matches!(side.pcurve, Some(PcurveGeometry::Nurbs { .. })));
    }
    assert!(matches!(
        context.sides[1].pcurve,
        Some(PcurveGeometry::Nurbs {
            weights: Some(_),
            ..
        })
    ));

    let mut retained = result.ir.clone();
    let ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = &mut retained.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-2.0, 5.0];
    for (side, discontinuities) in context.discontinuities.iter_mut().enumerate() {
        for (ordinal, value) in discontinuities.iter_mut().enumerate() {
            *value = 0.125 * (side + ordinal + 1) as f64;
        }
    }
    *discontinuity_flag = false;
    *offsets = [-2.5, 4.5];
    let expected_retained = retained.model.procedural_curves[0].definition.clone();
    let mut retained_bytes = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(
            &retained,
            &result.source_fidelity,
            &mut retained_bytes,
        )
        .expect("retained embedded offset-support edit");
    let retained_round_trip = F3dCodec
        .decode(&mut Cursor::new(retained_bytes), &DecodeOptions::default())
        .expect("retained embedded offset-support round trip");
    assert_eq!(
        retained_round_trip.ir.model.procedural_curves[0].definition,
        expected_retained
    );

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut expected = source_less.model.procedural_curves[0].definition.clone();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less embedded offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less embedded offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context: expected_context,
        ..
    } = &mut expected
    else {
        unreachable!()
    };
    let ProceduralCurveDefinition::TwoSidedOffset {
        context: actual_context,
        ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip embedded offset supports")
    };
    for side in 0..2 {
        let expected_surface = source_less
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == expected_context.sides[side].surface.as_ref())
            .expect("source support surface");
        let actual_surface = round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == actual_context.sides[side].surface.as_ref())
            .expect("round-trip support surface");
        assert_eq!(actual_surface.geometry, expected_surface.geometry);
        expected_context.sides[side].surface = actual_context.sides[side].surface.clone();
    }
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        expected
    );
}

#[test]
fn generated_mixed_offset_supports_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_embedded_offset_supports_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated embedded offset-support decode");
    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let ProceduralCurveDefinition::TwoSidedOffset { context, .. } =
        &mut source_less.model.procedural_curves[0].definition
    else {
        panic!("expected two-sided offset construction")
    };
    context.sides[1].surface = None;
    context.sides[1].pcurve = None;
    context.sides[0].pcurve = Some(cadmpeg_ir::geometry::PcurveGeometry::Line {
        origin: cadmpeg_ir::math::Point2::new(1.0, 2.0),
        direction: cadmpeg_ir::math::Point2::new(3.0, -1.0),
    });
    let first_support = context.sides[0]
        .surface
        .clone()
        .expect("retained first support id");
    let expected_surface = source_less
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == first_support)
        .expect("retained first support")
        .geometry
        .clone();

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less mixed offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less mixed offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset { context, .. } =
        &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip two-sided offset construction")
    };
    assert!(context.sides[1].surface.is_none() && context.sides[1].pcurve.is_none());
    assert_eq!(
        context.sides[0].pcurve,
        Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point2::new(1.0, 2.0),
                cadmpeg_ir::math::Point2::new(4.0, 1.0),
            ],
            weights: None,
            periodic: false,
        })
    );
    let actual_surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == context.sides[0].surface.as_ref())
        .expect("round-trip first support");
    assert_eq!(actual_surface.geometry, expected_surface);
}

#[test]
fn generated_analytic_offset_supports_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_analytic_offset_supports_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("analytic offset-support decode");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context, offsets, ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected analytic two-sided offset")
    };
    assert_eq!(*offsets, [-1.5, 2.5]);
    let supports = context.sides.each_ref().map(|side| {
        result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("analytic support surface")
            .geometry
            .clone()
    });
    assert!(matches!(
        supports[0],
        SurfaceGeometry::Cone {
            radius: 10.0,
            ratio: 0.4,
            half_angle,
            axis,
            ..
        } if (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12
            && axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
    ));
    assert!(matches!(
        supports[1],
        SurfaceGeometry::Torus {
            minor_radius: -7.5,
            ..
        }
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected_geometries = supports;
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less analytic offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less analytic offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset {
        context, offsets, ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip analytic offset supports")
    };
    assert_eq!(*offsets, [-1.5, 2.5]);
    for (side, expected) in context.sides.iter().zip(expected_geometries) {
        let actual = round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("round-trip analytic support surface");
        assert_eq!(actual.geometry, expected);
    }
}

#[test]
fn generated_surface_intersection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_surface_intersection_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("surface intersection decode");
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected surface intersection")
    };
    assert!(*discontinuity_flag);
    let expected_geometries = context.sides.each_ref().map(|side| {
        result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("intersection support surface")
            .geometry
            .clone()
    });
    assert!(matches!(
        expected_geometries[0],
        SurfaceGeometry::Cone { half_angle, .. }
            if (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1.0e-12
    ));
    assert!(matches!(
        expected_geometries[1],
        SurfaceGeometry::Torus { .. }
    ));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-1.0, 2.0];
    *discontinuity_flag = false;
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("intersection context regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated intersection decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Intersection {
            ref context,
            discontinuity_flag: false,
        } if context.parameter_range == [-1.0, 2.0]
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less surface intersection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less surface intersection round trip");
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip surface intersection")
    };
    assert!(*discontinuity_flag);
    for (side, expected) in context.sides.iter().zip(expected_geometries) {
        let actual = round_trip
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("round-trip intersection support");
        assert_eq!(actual.geometry, expected);
    }
}

#[test]
fn generated_projection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, ProjectionTail};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_projection_smbh())),
            &DecodeOptions::default(),
        )
        .expect("projection decode");
    let ProceduralCurveDefinition::Projection {
        context,
        discontinuity_flag,
        source,
        tail,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected projection")
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    assert!(*discontinuity_flag);
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        tail,
        &ProjectionTail::Ranged {
            flag: true,
            parameter_range: [-2.0, 3.0],
            role: "surf2".into(),
        }
    );

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Projection {
        context,
        discontinuity_flag,
        tail,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-1.0, 2.0];
    *discontinuity_flag = false;
    let ProjectionTail::Ranged {
        flag,
        parameter_range,
        role,
    } = tail
    else {
        unreachable!()
    };
    *flag = false;
    *parameter_range = [-4.0, 5.0];
    *role = "surf1".into();
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("projection context regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated projection decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Projection {
            ref context,
            discontinuity_flag: false,
            tail: ProjectionTail::Ranged {
                flag: false,
                parameter_range: [-4.0, 5.0],
                ref role,
            },
            ..
        } if context.parameter_range == [-1.0, 2.0] && role == "surf1"
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less projection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less projection round trip");
    let ProceduralCurveDefinition::Projection {
        discontinuity_flag,
        tail,
        ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip projection")
    };
    assert!(*discontinuity_flag);
    assert_eq!(
        tail,
        &ProjectionTail::Ranged {
            flag: true,
            parameter_range: [-2.0, 3.0],
            role: "surf2".into(),
        }
    );
}

#[test]
fn generated_early_close_projection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, ProjectionTail};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_early_close_projection_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("early-close projection decode");
    assert!(matches!(
        result.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Projection {
            discontinuity_flag: true,
            tail: ProjectionTail::EarlyClose { flag: true },
            ..
        }
    ));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Projection {
        tail: ProjectionTail::EarlyClose { flag },
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    *flag = false;
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("early-close projection regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated early-close projection decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Projection {
            tail: ProjectionTail::EarlyClose { flag: false },
            ..
        }
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less early-close projection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less early-close projection round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Projection {
            discontinuity_flag: true,
            tail: ProjectionTail::EarlyClose { flag: true },
            ..
        }
    ));
}

#[test]
fn generated_three_surface_intersection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_three_surface_intersection_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("three-surface intersection decode");
    let ProceduralCurveDefinition::ThreeSurfaceIntersection {
        context,
        selector,
        third,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected three-surface intersection")
    };
    assert_eq!(*selector, 7);
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    let third_surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == third.surface.as_ref())
        .expect("third support surface");
    assert!(matches!(
        third_surface.geometry,
        SurfaceGeometry::Sphere { radius: -12.5, .. }
    ));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::ThreeSurfaceIntersection {
        context, selector, ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-1.0, 2.0];
    *selector = -4;
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("three-surface intersection regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated three-surface intersection decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::ThreeSurfaceIntersection {
            ref context,
            selector: -4,
            ..
        } if context.parameter_range == [-1.0, 2.0]
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less three-surface intersection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less three-surface intersection round trip");
    let ProceduralCurveDefinition::ThreeSurfaceIntersection {
        selector, third, ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip three-surface intersection")
    };
    assert_eq!(*selector, 7);
    let third_surface = round_trip
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == third.surface.as_ref())
        .expect("round-trip third support surface");
    assert!(matches!(
        third_surface.geometry,
        SurfaceGeometry::Sphere { radius: -12.5, .. }
    ));
}

#[test]
fn generated_prefix_only_surface_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceCurveFamily};

    for (name, expected_family) in [
        ("blend_int_cur", SurfaceCurveFamily::Blend),
        ("surf_int_cur", SurfaceCurveFamily::SurfaceConstrained),
        ("par_int_cur", SurfaceCurveFamily::Parametric),
        ("skin_int_cur", SurfaceCurveFamily::Skin),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_surface_curve_smbh(
                    name,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name} decode failed: {error}"));
        let ProceduralCurveDefinition::SurfaceCurve {
            family, context, ..
        } = &result.ir.model.procedural_curves[0].definition
        else {
            panic!("expected {name} surface curve")
        };
        assert_eq!(family, &expected_family);
        assert!(context.sides.iter().all(|side| side.surface.is_some()));

        let mut edited = result.ir.clone();
        let ProceduralCurveDefinition::SurfaceCurve { context, .. } =
            &mut edited.model.procedural_curves[0].definition
        else {
            unreachable!()
        };
        context.parameter_range = [-1.0, 2.0];
        let mut regenerated = Vec::new();
        F3dCodec
            .write_preserved_with_source_fidelity(
                &edited,
                &result.source_fidelity,
                &mut regenerated,
            )
            .unwrap_or_else(|error| panic!("{name} context regeneration failed: {error}"));
        let regenerated = F3dCodec
            .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("regenerated {name} decode failed: {error}"));
        assert!(matches!(
            regenerated.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::SurfaceCurve { ref context, .. }
                if context.parameter_range == [-1.0, 2.0]
        ));

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .unwrap_or_else(|error| panic!("{name} source-less encode failed: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name} round trip failed: {error}"));
        assert!(matches!(
            &round_trip.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::SurfaceCurve { family, .. } if family == &expected_family
        ));
    }
}

#[test]
fn generated_silhouette_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SilhouetteKind};

    for (name, draft_factor) in [
        ("silh_int_cur", None),
        ("para_silh_int_cur", None),
        ("taper_silh_int_cur", Some(0.35)),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_silhouette_smbh(
                    name,
                    draft_factor,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name} decode failed: {error}"));
        let ProceduralCurveDefinition::Silhouette {
            silhouette,
            cast_surface,
            light_direction,
            ..
        } = &result.ir.model.procedural_curves[0].definition
        else {
            panic!("expected {name} silhouette")
        };
        assert!(result
            .ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *cast_surface));
        assert_eq!(
            *light_direction,
            cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0)
        );
        match (silhouette, draft_factor) {
            (SilhouetteKind::Standard, None) if name == "silh_int_cur" => {}
            (SilhouetteKind::Parametric, None) if name == "para_silh_int_cur" => {}
            (
                SilhouetteKind::Taper {
                    draft_factor: actual,
                },
                Some(expected),
            ) => {
                assert_eq!(*actual, expected);
            }
            _ => panic!("wrong silhouette family for {name}"),
        }

        let mut edited = result.ir.clone();
        let ProceduralCurveDefinition::Silhouette {
            silhouette,
            light_direction,
            ..
        } = &mut edited.model.procedural_curves[0].definition
        else {
            unreachable!()
        };
        *light_direction = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
        if let SilhouetteKind::Taper { draft_factor } = silhouette {
            *draft_factor = -0.2;
        }
        let mut regenerated = Vec::new();
        F3dCodec
            .write_preserved_with_source_fidelity(
                &edited,
                &result.source_fidelity,
                &mut regenerated,
            )
            .unwrap_or_else(|error| panic!("{name} regeneration failed: {error}"));
        let regenerated = F3dCodec
            .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("regenerated {name} decode failed: {error}"));
        assert!(matches!(
            regenerated.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::Silhouette {
                ref silhouette,
                light_direction,
                ..
            } if light_direction == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
                && match silhouette {
                    SilhouetteKind::Taper { draft_factor } => *draft_factor == -0.2,
                    _ => true,
                }
        ));

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .unwrap_or_else(|error| panic!("{name} source-less encode failed: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name} round trip failed: {error}"));
        assert!(matches!(
            round_trip.ir.model.procedural_curves[0].definition,
            ProceduralCurveDefinition::Silhouette { .. }
        ));
    }
}

#[test]
fn generated_surface_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_surface_offset_smbh())),
            &DecodeOptions::default(),
        )
        .expect("surface-offset decode");
    let ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected surface-offset construction")
    };
    assert_eq!(*base_u_range, [-1.0, 2.0]);
    assert_eq!(context.parameter_range, [0.0, 1.0]);
    assert!(*discontinuity_flag);
    assert_eq!(*base_v_range, [-3.0, 4.0]);
    assert_eq!(*base_range, [-0.5, 1.5]);
    assert_eq!((*distance, *shift, *scale), (-2.5, 0.75, 1.25));
    assert!(result.ir.model.curves.iter().any(|curve| curve.id == *base));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-1.5, 2.5];
    *discontinuity_flag = false;
    *base_u_range = [-2.0, 5.0];
    *base_v_range = [-6.0, 7.0];
    *base_range = [-0.75, 1.75];
    (*distance, *shift, *scale) = (3.5, -0.25, 0.8);
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("surface-offset scalar regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated surface-offset decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::SurfaceOffset {
            ref context,
            discontinuity_flag: false,
            base_u_range: [-2.0, 5.0],
            base_v_range: [-6.0, 7.0],
            base_range: [-0.75, 1.75],
            distance: 3.5,
            shift: -0.25,
            scale: 0.8,
            ..
        } if context.parameter_range == [-1.5, 2.5]
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less surface-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less surface-offset round trip");
    let ProceduralCurveDefinition::SurfaceOffset {
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &round_trip.ir.model.procedural_curves[0].definition
    else {
        panic!("expected round-trip surface offset")
    };
    assert_eq!(*base_u_range, [-1.0, 2.0]);
    assert!(*discontinuity_flag);
    assert_eq!(*base_v_range, [-3.0, 4.0]);
    assert_eq!(*base_range, [-0.5, 1.5]);
    assert_eq!((*distance, *shift, *scale), (-2.5, 0.75, 1.25));
}

#[test]
fn generated_spring_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_spring_smbh())),
            &DecodeOptions::default(),
        )
        .expect("spring decode");
    let ProceduralCurveDefinition::Spring {
        context, direction, ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected spring construction")
    };
    assert_eq!(*direction, -3);
    assert!(context
        .sides
        .iter()
        .all(|side| side.surface.is_some() && side.pcurve.is_some()));

    let mut edited = result.ir.clone();
    let ProceduralCurveDefinition::Spring {
        context,
        discontinuity_flag,
        direction,
        ..
    } = &mut edited.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    context.parameter_range = [-2.0, 3.0];
    let expected_flag = !*discontinuity_flag;
    *discontinuity_flag = expected_flag;
    *direction = 4;
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &result.source_fidelity, &mut regenerated)
        .expect("spring tail regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated spring decode");
    assert!(matches!(
        regenerated.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Spring {
            ref context,
            discontinuity_flag,
            direction: 4,
            ..
        } if discontinuity_flag == expected_flag && context.parameter_range == [-2.0, 3.0]
    ));

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less spring round trip");
    assert!(matches!(
        round_trip.ir.model.procedural_curves[0].definition,
        ProceduralCurveDefinition::Spring { direction: -3, .. }
    ));
}

#[test]
fn generated_null_support_spring_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_null_support_spring_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("null-support spring decode");
    let ProceduralCurveDefinition::Spring {
        context,
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        discontinuity_flag,
        cache_first,
        direction,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected spring construction")
    };
    assert_eq!(*cache_first, None);
    assert_eq!(*direction, 4);
    assert!(*discontinuity_flag);
    assert!(context
        .sides
        .iter()
        .all(|side| side.surface.is_none() && side.pcurve.is_none()));
    assert_eq!(
        surface_parameter_ranges[0],
        Some([[-2.0, 3.0], [-4.0, 5.0]])
    );
    assert_eq!(
        surface_parameter_ranges[1],
        Some([[-6.0, 7.0], [-8.0, 9.0]])
    );
    assert_eq!(*first_pcurve_parameter_range, Some([-10.0, 11.0]));
    assert_eq!(context.parameter_range, [-1.0, 2.0]);

    let mut source_less = result.ir;
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less null-support spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less null-support spring round trip");
    assert_eq!(
        round_trip.ir.model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_deformable_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{DeformableCurveData, ProceduralCurveDefinition};

    for mode in [8, 5] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_deformable_curve_smbh(mode),
                )),
                &DecodeOptions::default(),
            )
            .expect("deformable decode");
        let ProceduralCurveDefinition::Deformable {
            extension,
            bend,
            data,
        } = &result.ir.model.procedural_curves[0].definition
        else {
            panic!("expected deformable construction")
        };
        assert_eq!(*extension, 0);
        assert!(result.ir.model.curves.iter().any(|curve| curve.id == *bend));
        match (mode, data) {
            (
                8,
                DeformableCurveData::VectorField {
                    vectors,
                    parameter_pairs,
                },
            ) => {
                assert_eq!(vectors[3], cadmpeg_ir::math::Vector3::new(10.0, 11.0, 12.0));
                assert_eq!(parameter_pairs, &[[-1.0, 0.25], [2.0, 3.5]]);
            }
            (5, DeformableCurveData::Surface { surface }) => {
                assert!(result
                    .ir
                    .model
                    .surfaces
                    .iter()
                    .any(|item| item.id == *surface));
            }
            _ => panic!("wrong deformable discriminator payload"),
        }
        let expected_data = data.clone();
        let bend = bend.clone();

        let mut source_less = result.ir;
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == bend)
            .expect("deformable bend carrier")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(3.0, -2.0, 5.0),
            direction: cadmpeg_ir::math::Vector3::new(2.0, 4.0, -1.0),
        };
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("source-less deformable encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less deformable round trip");
        let ProceduralCurveDefinition::Deformable {
            extension: round_extension,
            bend: round_bend,
            data: round_data,
        } = &round_trip.ir.model.procedural_curves[0].definition
        else {
            panic!("expected round-trip deformable construction")
        };
        assert_eq!(*round_extension, 0);
        match (&expected_data, round_data) {
            (DeformableCurveData::VectorField { .. }, DeformableCurveData::VectorField { .. }) => {
                assert_eq!(round_data, &expected_data)
            }
            (DeformableCurveData::Surface { .. }, DeformableCurveData::Surface { surface }) => {
                assert!(round_trip
                    .ir
                    .model
                    .surfaces
                    .iter()
                    .any(|item| item.id == *surface))
            }
            _ => panic!("round-trip deformable discriminator changed"),
        }
        assert!(round_trip
            .ir
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *round_bend));
        assert!(matches!(
            round_trip
                .ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *round_bend)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                if curve.degree == 1
                    && curve.knots == [0.0, 0.0, 1.0, 1.0]
                    && curve.control_points == [
                        cadmpeg_ir::math::Point3::new(3.0, -2.0, 5.0),
                        cadmpeg_ir::math::Point3::new(5.0, 2.0, 4.0),
                    ]
        ));
    }
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    source_less.model.procedural_curves[0].definition = ProceduralCurveDefinition::BlendSpine {
        blend_surface: None,
    };
    let mut encoded = Vec::new();
    let error = F3dCodec
        .encode(&source_less, &mut encoded)
        .expect_err("typed intersection must not degrade to a cache-only curve");
    assert!(error
        .to_string()
        .contains("lacks its native blend construction"));

    source_less.model.procedural_curves[0].definition = ProceduralCurveDefinition::Unknown {
        native_kind: None,
        record: None,
    };
    let error = F3dCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("unknown construction must not degrade to a cache-only curve");
    assert!(error
        .to_string()
        .contains("cannot be regenerated losslessly"));
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
    source_less.set_native_unknowns("f3d", &[]).unwrap();
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
    use crate::nurbs::pcurve::decode_pcurve_cache;

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
fn ref_pcurve_collects_intcurve_uv_candidates() {
    let mut intcurve = generated_curve_block();
    intcurve.extend_from_slice(&generated_pcurve_block());

    let candidates = crate::nurbs::pcurve::decode_pcurve_cache_candidates_resolving_refs(
        &intcurve,
        &intcurve,
        &crate::nurbs::subtypes::SubtypeTables::from_stream(&intcurve),
    );
    let pcurve = candidates
        .first()
        .expect("intcurve UV cache is a candidate");
    assert!(pcurve.unambiguous_2d);
    assert_eq!(pcurve.curve.control_points[0].u, 0.25);
    assert_eq!(pcurve.curve.control_points[1].v, 1.5);
}

#[test]
fn ref_pcurve_resolves_intcurve_subtype_candidates() {
    let mut target = b"\x0f\x0d\x0bint_int_cur".to_vec();
    target.extend_from_slice(&generated_curve_block());
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    let mut source = b"\x0f\x0d\x03ref\x04".to_vec();
    source.extend_from_slice(&0i64.to_le_bytes());
    source.push(0x10);
    let mut active = target;
    active.extend_from_slice(&source);

    let candidates = crate::nurbs::pcurve::decode_pcurve_cache_candidates_resolving_refs(
        &source,
        &active,
        &crate::nurbs::subtypes::SubtypeTables::from_stream(&active),
    );
    let pcurve = candidates
        .first()
        .expect("intcurve subtype carries a UV candidate");
    assert!(pcurve.unambiguous_2d);
    assert_eq!(pcurve.curve.control_points[1].v, 1.5);
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
            .filter(|c| !c.pcurves.is_empty())
            .count(),
        1
    );
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn inline_pcurve_scope_is_its_exact_carrier_identity() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("structurally unique inline pcurve decode");

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
}

#[test]
fn wrapped_ref_pcurve_resolves_its_subtype_carrier() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_wrapped_ref_pcurve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("wrapped ref pcurve decode");

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
}

#[test]
fn unique_bs2_intcurve_role_is_its_ref_pcurve_carrier() {
    for discriminator in [2, -2] {
        let smbh = with_pcurve_discriminator(
            synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh(),
            discriminator,
        );
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("structurally unique ref pcurve decode");

        assert_eq!(result.ir.model.pcurves.len(), 1);
        assert_eq!(
            result
                .ir
                .model
                .coedges
                .iter()
                .filter(|coedge| !coedge.pcurves.is_empty())
                .count(),
            1
        );
        assert!(result
            .report
            .losses
            .iter()
            .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
    }
}

#[test]
fn generated_inline_pcurve_tail_requires_four_adjacent_booleans() {
    let decode = |smbh: Vec<u8>| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated inline pcurve decode")
            .ir
            .model
            .pcurves
            .into_iter()
            .next()
            .expect("generated inline pcurve")
    };

    let complete = decode(synthetic_geometry_with_pcurve_smbh());
    assert_eq!(complete.native_tail_flags, Some([true, false, true, false]));
    assert_eq!(complete.parameter_range, Some([-1.0, 2.0]));

    let short = decode(synthetic_geometry_with_short_pcurve_tail_smbh());
    assert_eq!(short.native_tail_flags, None);
    assert_eq!(short.parameter_range, Some([-1.0, 2.0]));
}

#[test]
fn generated_inline_pcurve_fit_tolerance_is_scoped() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated inline pcurve decode");
    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, Some(0.001));
}

#[test]
fn generated_pcurve_geometry_dispatch_follows_discriminator() {
    for smbh in [
        with_pcurve_discriminator(synthetic_geometry_with_pcurve_smbh(), 2),
        with_inline_pcurve_non_boolean_wrapper(synthetic_geometry_with_pcurve_smbh()),
        renamed_generated_subtype(
            synthetic_geometry_with_pcurve_smbh(),
            "exp_par_cur",
            "bad_par_cur",
        ),
        synthetic_geometry_with_out_of_scope_pcurve_cache_smbh(),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), 0),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), 7),
        with_ref_pcurve_companion_name(synthetic_geometry_with_ref_pcurve_smbh(), b"badcurve"),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated mismatched pcurve decode");
        assert!(result.ir.model.pcurves.is_empty());
        assert!(result
            .ir
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.is_empty()));
        let note = result
            .report
            .losses
            .iter()
            .find(|loss| loss.message.contains("explicit UV pcurve reference"))
            .expect("undecoded pcurve loss note");
        assert!(note.message.contains("Native kinds: pcurve=1."));
    }
}

#[test]
fn generated_pcurve_reports_dangling_carrier_reference() {
    let mut smbh = synthetic_geometry_with_pcurve_smbh();
    let start = asm_header::record_stream_start(&smbh).unwrap();
    let limit = asm_header::first_delta_state_offset(&smbh).unwrap();
    let records = crate::sab::frame(&smbh, start, limit, 8).unwrap();
    let coedge = &records[7];
    let record = &mut smbh[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[pcurve_ref + 1..pcurve_ref + 9].copy_from_slice(&999i64.to_le_bytes());

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("dangling pcurve reference remains a successful topology decode");
    let note = result
        .report
        .losses
        .iter()
        .find(|loss| loss.message.contains("explicit UV pcurve reference"))
        .expect("dangling pcurve loss note");
    assert!(note.message.contains("Native kinds: dangling-reference=1."));
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
    assert_eq!(pcurve.native_tail_flags, Some([true, false, true, false]));
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
    pcurve.native_tail_flags = Some([false, true, false, true]);
    pcurve.parameter_range = Some([-2.0, 3.0]);
    pcurve.fit_tolerance = Some(0.0025);
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected.clone()]);
}

#[test]
fn generated_f3d_scopes_inline_pcurve_edits() {
    let source =
        f3d_with_smbh(&synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated scoped pcurve decode");
    let mut edited = decoded.ir;
    let pcurve = &mut edited.model.pcurves[0];
    let cadmpeg_ir::geometry::PcurveGeometry::Nurbs { control_points, .. } = &mut pcurve.geometry
    else {
        panic!("expected NURBS pcurve")
    };
    control_points[0].u = -0.75;
    pcurve.fit_tolerance = Some(0.0025);
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("scoped pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated scoped pcurve decode");
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
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
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("ref-form pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated ref-form pcurve decode");
    assert_eq!(round_trip.ir.model.pcurves, [expected.clone()]);

    edited.source = None;
    edited.set_native_unknowns("f3d", &[]).unwrap();
    let mut source_less = Vec::new();
    F3dCodec
        .encode(&edited, &mut source_less)
        .expect("source-less ref-form pcurve encode");
    let source_less_round_trip = F3dCodec
        .decode(&mut Cursor::new(source_less), &DecodeOptions::default())
        .expect("source-less ref-form pcurve round trip");
    let actual = &source_less_round_trip.ir.model.pcurves[0];
    assert_eq!(actual.geometry, expected.geometry);
    assert_eq!(actual.wrapper_reversed, expected.wrapper_reversed);
    assert_eq!(actual.native_tail_flags, expected.native_tail_flags);
    assert_eq!(actual.parameter_range, expected.parameter_range);
    assert_eq!(actual.fit_tolerance, expected.fit_tolerance);
    assert!(source_less_round_trip
        .ir
        .model
        .coedges
        .iter()
        .any(|coedge| coedge.pcurves.iter().any(|use_| use_.pcurve == actual.id)));

    let mut mixed = edited;
    let mut inline = mixed.model.pcurves[0].clone();
    inline.id = cadmpeg_ir::ids::PcurveId("generated:mixed-inline-pcurve#0".into());
    inline.wrapper_reversed = Some(false);
    inline.native_tail_flags = Some([true, false, true, false]);
    inline.fit_tolerance = Some(0.002);
    mixed.model.coedges[1].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: inline.id.clone(),
        isoparametric: None,
        parameter_range: None,
    }];
    mixed.model.pcurves.push(inline);
    let mut mixed_bytes = Vec::new();
    F3dCodec
        .encode(&mixed, &mut mixed_bytes)
        .expect("mixed inline/ref-form pcurve encode");
    let mixed_round_trip = F3dCodec
        .decode(&mut Cursor::new(mixed_bytes), &DecodeOptions::default())
        .expect("mixed inline/ref-form pcurve round trip");
    assert_eq!(mixed_round_trip.ir.model.pcurves.len(), 2);
    assert!(mixed_round_trip
        .ir
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.wrapper_reversed.is_none()));
    assert!(mixed_round_trip
        .ir
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.wrapper_reversed == Some(false)));
    assert!(mixed_round_trip
        .ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| &use_.pcurve))
        .all(|pcurve_id| mixed_round_trip
            .ir
            .model
            .pcurves
            .iter()
            .any(|pcurve| pcurve.id == *pcurve_id)));
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
        crate::records::ConstructionRecipeKind::Body
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
                && reference.kind == crate::records::PersistentReferenceKind::CurvePrimary
        }));
    assert_eq!(f3d_native(&result.ir).lost_edge_references.len(), 1);
    assert_eq!(
        f3d_native(&result.ir).lost_edge_references[0].class_tag,
        "419"
    );
    assert_eq!(
        f3d_native(&result.ir).lost_edge_references[0].record_index,
        4645
    );
    assert_eq!(
        f3d_native(&result.ir).lost_edge_references[0].next_record_index,
        4646
    );
    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("source parametric edge reference(s) were marked")));
    assert_eq!(f3d_native(&result.ir).design_objects.len(), 3);
    let sketch = f3d_native(&result.ir)
        .design_objects
        .iter()
        .find(|object| object.kind == crate::records::DesignObjectKind::Sketch)
        .cloned()
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
        Some(crate::records::DesignObjectKind::Sketch)
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
        .cloned()
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
        [crate::records::SketchConstraintKind::Parallel]
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
        .cloned()
        .expect("point 500");
    assert_eq!(point_500.coordinates.u, 12.5);
    assert_eq!(point_500.coordinates.v, -25.0);
    let point_600 = f3d_native(&result.ir)
        .sketch_points
        .iter()
        .find(|point| point.persistent_id == 600)
        .cloned()
        .expect("point 600");
    assert_eq!(point_600.coordinates.u, -40.0);
    assert_eq!(point_600.entity_genesis, Some(9));
    assert_eq!(f3d_native(&result.ir).sketch_curve_identities.len(), 2);
    assert_eq!(
        f3d_native(&result.ir).sketch_curve_identities[0].primary_id,
        440
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_curve_identities[0].secondary_id,
        0
    );
    assert_eq!(
        f3d_native(&result.ir).sketch_curve_identities[1].entity_genesis,
        Some(10)
    );
    assert!(matches!(
        f3d_native(&result.ir).sketch_curve_identities[0].geometry,
        Some(crate::records::SketchCurveGeometry::Arc { radius: 30.0, .. })
    ));
    assert!(matches!(
        &f3d_native(&result.ir).sketch_curve_identities[1].geometry,
        Some(crate::records::SketchCurveGeometry::Nurbs {
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
fn decode_binds_revision_suffixed_protein_visual_guid() {
    let visual = "11111111-2222-3333-4444-555555555555_Post2015_Post2015";
    let f3d = f3d_with_smbh_and_protein_guids(&synthetic_geometry_smbh(), &[visual]);
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .expect("revision-suffixed Protein decode");

    assert_eq!(result.ir.model.appearances.len(), 1);
    assert_eq!(
        result.ir.model.appearances[0].visual_guid.as_deref(),
        Some(visual)
    );
    assert_eq!(result.ir.model.appearance_bindings.len(), 1);
    assert_eq!(
        result.ir.model.appearance_bindings[0].appearance,
        result.ir.model.appearances[0].id
    );
}

#[test]
fn decode_transfers_generated_custom_attribute() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_attribute_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.attributes.len(), 2);
    let attribute = result
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.values.iter().any(|value| {
                matches!(
                    value,
                    cadmpeg_ir::attributes::AttributeValue::String(text)
                        if text == "generic_tag_attrib_def"
                )
            })
        })
        .expect("generic tag attribute");
    assert_eq!(attribute.name, "ATTRIB_CUSTOM-attrib");
    assert!(matches!(
        &attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Body(body) if body == &result.ir.model.bodies[0].id
    ));
    assert!(attribute.values.iter().any(|value| matches!(
        value,
        cadmpeg_ir::attributes::AttributeValue::String(text) if text == "322"
    )));
    assert_eq!(f3d_native(&result.ir).persistent_design_links.len(), 2);
    assert_eq!(
        f3d_native(&result.ir).persistent_design_links[1].design_id,
        "322"
    );
    assert_eq!(
        f3d_native(&result.ir).persistent_design_links[1].design_reference,
        7
    );
    assert!(!f3d_native(&result.ir).persistent_design_links[0].is_current);
    assert!(f3d_native(&result.ir).persistent_design_links[1].is_current);
    assert!(attribute.values.iter().any(|value| matches!(
        value,
        cadmpeg_ir::attributes::AttributeValue::String(text) if text == "900"
    )));
    assert_eq!(f3d_native(&result.ir).creation_timestamps.len(), 1);
    assert_eq!(
        f3d_native(&result.ir).creation_timestamps[0].unix_microseconds,
        1_579_392_000_000_007.0
    );
}

#[test]
fn source_less_tolerant_vertex_retains_custom_attribute_ownership() {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut source = cadmpeg_ir::examples::unit_cube();
    source.source = None;
    source.set_native_unknowns("f3d", &[]).unwrap();
    let vertex = source.model.vertices[0].id.clone();
    source.model.vertices[0].tolerance = Some(0.025);
    f3d_native_mut(&mut source).creation_timestamps = vec![crate::records::CreationTimestamp {
        id: "f3d:asm:creation-timestamp#generated".into(),
        target: AttributeTarget::Vertex(vertex),
        record_index: 0,
        unix_microseconds: 1_579_392_000_000_037.0,
    }];

    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source, &mut encoded)
        .expect("source-less tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less tolerant vertex decode");

    let tolerant_vertex = round_trip
        .ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.tolerance == Some(0.025))
        .expect("tolerant vertex");
    let attribute = round_trip
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.name == "ATTRIB_CUSTOM-attrib"
                && attribute.target == AttributeTarget::Vertex(tolerant_vertex.id.clone())
        })
        .expect("tolerant vertex attribute");
    assert_eq!(
        attribute.target,
        AttributeTarget::Vertex(tolerant_vertex.id.clone())
    );
    assert_eq!(
        f3d_native(&round_trip.ir).creation_timestamps[0].unix_microseconds,
        1_579_392_000_000_037.0
    );
}

#[test]
fn generated_f3d_rewrites_creation_timestamp() {
    let source = f3d_with_smbh(&synthetic_geometry_with_attribute_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated timestamp decode");
    let mut edited = decoded.ir;
    let expected = 1_704_067_200_000_009.0;
    update_f3d_native(&mut edited, |native| {
        assert_eq!(native.creation_timestamps[0].record_index, 20);
        native.creation_timestamps[0].unix_microseconds = expected;
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut regenerated)
        .expect("timestamp regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated timestamp decode");
    assert_eq!(
        f3d_native(&round_trip.ir).creation_timestamps[0].unix_microseconds,
        expected
    );
}

#[test]
fn decode_transfers_generated_sketch_curve_link() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let link = f3d_native(&result.ir)
        .sketch_curve_links
        .first()
        .cloned()
        .unwrap();
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

#[test]
fn body_visibility_maps_asm_keys_through_member_nodes() {
    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units: Vec<u16> = value.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    let mut bulk = Vec::new();
    // Body-binding record: pair count, (ASM key, member) pairs, the 12-byte
    // tail, then the blob name.
    bulk.extend_from_slice(&2u32.to_le_bytes());
    for (key, member) in [(3u64, 269u64), (6, 533)] {
        bulk.extend_from_slice(&key.to_le_bytes());
        bulk.extend_from_slice(&member.to_le_bytes());
    }
    bulk.extend_from_slice(&1793u64.to_le_bytes());
    bulk.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut bulk, "BREP.synthetic.smbh");
    // Browser-node records: GUID, hidden flag, `01 01` marker, member id.
    for (guid, hidden, member) in [
        ("b412e170-dc0c-4932-b699-43fc72cc8b13", 0u8, 269u64),
        ("d4b1078c-43bf-4f6d-a50a-963f94273901", 1, 533),
    ] {
        lp_utf16(&mut bulk, guid);
        bulk.push(hidden);
        bulk.extend_from_slice(&[0x01, 0x01]);
        bulk.extend_from_slice(&member.to_le_bytes());
    }

    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(b"synthetic-manifest").unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    let bytes = zip.finish().unwrap().into_inner();

    with_scan(&bytes, |scan| {
        let visibility = crate::design::decode::body::decode_all_body_visibility(scan).unwrap();
        assert_eq!(
            visibility
                .get(&("BREP.synthetic.smbh".into(), 3))
                .map(|item| item.visible),
            Some(true),
            "flag 0 decodes visible"
        );
        assert_eq!(
            visibility
                .get(&("BREP.synthetic.smbh".into(), 6))
                .map(|item| item.visible),
            Some(false),
            "flag 1 decodes hidden"
        );

        assert!(!visibility.contains_key(&("BREP.other.smbh".into(), 3)));
    });
}

fn browser_body_record(entity: u64, name: Option<&str>, visual: &str) -> Vec<u8> {
    let mut bytes = vec![0u8; 8];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"299");
    bytes.extend_from_slice(&entity.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0u8, 40));
    bytes.extend(lp_utf16_bytes("D87FBE62-3B12-4CA8-9014-BAD31ABDB101"));
    bytes.extend(lp_utf16_bytes("C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C"));
    bytes.extend([0u8; 4]);
    bytes.extend(lp_utf16_bytes("PrismMaterial-018"));
    bytes.push(0x01);
    bytes.extend_from_slice(&(entity - 100).to_le_bytes());
    bytes.extend([0u8; 3]);
    bytes.extend(lp_utf16_bytes("67a722bb-f14e-43d6-94b1-d0539bb8060c"));
    bytes.push(0x01);
    bytes.extend_from_slice(&(entity + 1).to_le_bytes());
    bytes.extend([0u8; 2]);
    if let Some(name) = name {
        bytes.extend(lp_utf16_bytes(name));
    }
    bytes.extend([0u8; 12]);
    bytes.extend_from_slice(&1f32.to_le_bytes());
    bytes.extend([0x01, 0x01]);
    bytes.extend([0u8; 10]);
    bytes.extend(lp_utf16_bytes(visual));
    bytes
}

#[test]
fn browser_body_appearance_decodes_named_and_nameless_records() {
    let visual = "7DD7765D-CA8C-4A38-B156-B3B4916E0C17_Post2015_Post2015";
    let mut bytes = browser_body_record(200598, Some("Hexagon 1"), visual);
    bytes.extend(browser_body_record(454966, None, visual));
    let out = crate::materials::browser_body_appearances(&bytes);
    assert_eq!(
        out,
        vec![
            (200598, "7DD7765D-CA8C-4A38-B156-B3B4916E0C17".to_string()),
            (454966, "7DD7765D-CA8C-4A38-B156-B3B4916E0C17".to_string()),
        ]
    );
}

#[test]
fn protein_revision_suffix_does_not_change_visual_guid_identity() {
    assert!(crate::materials::visual_guid_matches(
        "7DD7765D-CA8C-4A38-B156-B3B4916E0C17_Post2015_Post2015",
        "7dd7765d-ca8c-4a38-b156-b3b4916e0c17",
    ));
    assert!(!crate::materials::visual_guid_matches(
        "7DD7765D-CA8C-4A38-B156-B3B4916E0C17_Post2015",
        "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F",
    ));
    assert!(!crate::materials::visual_guid_matches(
        "not-a-guid_Post2015",
        "not-a-guid",
    ));
}

#[test]
fn browser_body_appearance_requires_head_and_node_entity_agreement() {
    let visual = "7DD7765D-CA8C-4A38-B156-B3B4916E0C17_Post2015";
    let mut bytes = browser_body_record(200598, Some("Hexagon 1"), visual);
    // Corrupt the node entity so it no longer equals the head entity plus one.
    let node = (200599u64).to_le_bytes();
    let at = bytes
        .windows(8)
        .position(|window| window == node)
        .expect("node entity bytes are present");
    bytes[at..at + 8].copy_from_slice(&(999u64).to_le_bytes());
    assert!(crate::materials::browser_body_appearances(&bytes).is_empty());
}

#[test]
fn face_appearance_assignment_joins_face_guid_to_visual_guid() {
    let mut bytes = vec![0u8; 8];
    bytes.extend(lp_utf16_bytes("cd92d0f6-5b31-4bbf-84ae-4611f435537e"));
    bytes.extend([0u8; 20]);
    bytes.extend(lp_utf16_bytes(
        "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015_Post2015",
    ));
    bytes.extend([0u8; 6]);
    bytes.extend(lp_utf16_bytes("BA5EE55E-9982-449B-9D66-9F036540E140"));
    let out = crate::materials::face_appearance_assignments(&bytes);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].face_guid, "cd92d0f6-5b31-4bbf-84ae-4611f435537e");
    assert_eq!(out[0].visual_guid, "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F");
}

#[test]
fn face_appearance_assignment_rejects_entity_id_and_uppercase_targets() {
    // A body-style assignment has an entity id, not a face GUID, before the
    // visual GUID; an uppercase GUID is a marker constant, not a face GUID.
    for target in ["0_985", "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C"] {
        let mut bytes = vec![0u8; 8];
        bytes.extend(lp_utf16_bytes(target));
        bytes.extend(lp_utf16_bytes(
            "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015",
        ));
        bytes.extend(lp_utf16_bytes("BA5EE55E-9982-449B-9D66-9F036540E140"));
        assert!(crate::materials::face_appearance_assignments(&bytes).is_empty());
    }
}

/// A `RedirectionsStream.dat` body with one self design entry plus one design
/// and one XREF reference per `(relative_path, role)` pair.
fn redirections_json(own_name: &str, targets: &[(&str, &str)]) -> String {
    let mut designs = vec![format!(
        r#"{{"file-version":1,"targetFileName":"{own_name}","displayName":"root","lineageUrn":"urn:adsk.wipprod:dm.lineage:RootKey","versionUrn":"urn:adsk.wipprod:fs.file:vf.RootKey?version=1"}}"#
    )];
    let mut references = Vec::new();
    for (ordinal, (path, role)) in targets.iter().enumerate() {
        designs.push(format!(
            r#"{{"file-version":1,"targetFileName":"{path}","displayName":"component{ordinal}","lineageUrn":"urn:adsk.wipprod:dm.lineage:Key{ordinal}","versionUrn":"urn:adsk.wipprod:fs.file:vf.Key{ordinal}?version=1"}}"#
        ));
        references.push(format!(
            r#"{{"from":"{own_name}","relativePath":"{path}","type":"XREF","properties":[{{"neutronRole":{{"value":"{role}","dataType":"STRING"}}}},{{"neutronData":{{"value":"{role}","dataType":"STRING"}}}}]}}"#
        ));
    }
    format!(
        r#"{{"name":"RedirectionsStream","schema-version":0,"designs":[{}],"references":[{}]}}"#,
        designs.join(","),
        references.join(",")
    )
}

/// A BREP-less `.f3d` with a docstruct `Properties.dat` and a redirections
/// table referencing `targets`.
fn f3d_without_brep(doc_type: &str, own_name: &str, targets: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(b"synthetic-manifest").unwrap();
    zip.start_file("Properties.dat", stored).unwrap();
    let properties = format!(
        r#"{{"docstruct":{{"version":"1.0.0","type":"{doc_type}","subtype":"synthetic","attributes":{{}}}}}}"#
    );
    zip.write_all(&u32::try_from(properties.len()).unwrap().to_le_bytes())
        .unwrap();
    zip.write_all(properties.as_bytes()).unwrap();
    zip.start_file("ComponentReferenceData.json", stored)
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json(own_name, targets).as_bytes())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

/// Wrap members into a `.f3z` archive with `Manifest.json` naming the root.
fn f3z_archive(root_name: &str, members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in members {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.start_file("Manifest.json", stored).unwrap();
    zip.write_all(format!(r#"{{"root":"{root_name}"}}"#).as_bytes())
        .unwrap();
    zip.start_file("DesignDescription.json", stored).unwrap();
    zip.write_all(br#"{"name":"Autodesk Design Description","version":"0.1","designDescription":{"id":"0","designGraphs":[]}}"#)
        .unwrap();
    zip.finish().unwrap().into_inner()
}

const XREF_ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";

#[test]
fn assembly_root_without_brep_is_not_a_blocking_loss() {
    let archive = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(
        decoded
            .report
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "assembly document must not report blocking/error losses: {:?}",
        decoded.report.losses
    );
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("assembly document")));
    assert!(decoded
        .report
        .notes
        .iter()
        .any(|note| note.contains("comp.f3d") && note.contains(XREF_ROLE)));
    let native =
        crate::native::F3dNative::load(decoded.ir.native.namespace("f3d").unwrap()).unwrap();
    assert_eq!(native.xref_designs.len(), 2);
    assert_eq!(native.xref_references.len(), 1);
    assert_eq!(native.xref_references[0].relative_path, "comp.f3d");
    assert_eq!(native.xref_references[0].neutron_role, XREF_ROLE);
    let source = decoded.ir.source.unwrap();
    assert_eq!(
        source.attributes.get("docstruct_type").map(String::as_str),
        Some("assembly-design")
    );
}

#[test]
fn part_without_brep_keeps_blocking_losses() {
    // A leaf redirections table (no outgoing references) does not make a
    // BREP-less part a valid assembly.
    let archive = f3d_without_brep("part-design", "part.f3d", &[]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Blocking));
}

#[test]
fn redirections_leaf_form_parses_empty_object_references() {
    let table = crate::xref::parse(
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"part.f3d","displayName":"part","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":{}}"#,
    )
    .unwrap();
    assert_eq!(table.designs.len(), 1);
    assert_eq!(table.designs[0].target_file_name, "part.f3d");
    assert!(table.references.is_empty());
}

#[test]
fn f3z_archive_merges_identity_occurrences() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let component_alone = F3dCodec
        .decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.report.geometry_transferred);
    assert!(
        decoded
            .report
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "{:?}",
        decoded.report.losses
    );
    assert!(decoded
        .report
        .notes
        .iter()
        .any(|note| note.contains("merged 1 external occurrence")));
    assert_eq!(
        decoded.ir.model.bodies.len(),
        component_alone.ir.model.bodies.len()
    );
    assert_eq!(
        decoded.ir.model.faces.len(),
        component_alone.ir.model.faces.len()
    );
    assert_eq!(
        decoded.ir.model.points.len(),
        component_alone.ir.model.points.len()
    );
    let prefix = format!("f3d:xref/{XREF_ROLE}/");
    let body = &decoded.ir.model.bodies[0];
    assert!(body.id.0.starts_with(&prefix), "{}", body.id.0);
    for shell_owner in &decoded.ir.model.shells {
        assert!(
            shell_owner.id.0.starts_with(&prefix),
            "occurrence graph must stay internally consistent: {}",
            shell_owner.id.0
        );
    }
}

#[test]
fn f3z_archive_recursively_merges_nested_occurrences() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let component_alone = F3dCodec
        .decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let middle = f3d_without_brep(
        "assembly-design",
        "middle.f3d",
        &[("component.f3d", CHILD_ROLE)],
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        decoded.ir.model.bodies.len(),
        component_alone.ir.model.bodies.len()
    );
    assert!(decoded
        .report
        .notes
        .iter()
        .any(|note| note.contains("merged 2 external occurrence")));
    let body_id = &decoded.ir.model.bodies[0].id.0;
    assert!(body_id.contains(&format!(
        "xref/{XREF_ROLE}/occurrence-0/xref/{CHILD_ROLE}/occurrence-0/"
    )));
}

#[test]
fn f3z_archive_reports_reference_cycles_without_recursing() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let middle = f3d_without_brep("assembly-design", "middle.f3d", &[("root.f3d", CHILD_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
        ],
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report.losses.iter().any(|loss| {
        loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("reference cycle through root.f3d")
    }));
}

#[test]
fn f3z_prefix_detects_as_f3d() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );
    assert_eq!(
        F3dCodec.detect(&archive[..512.min(archive.len())]),
        Confidence::High
    );
}
