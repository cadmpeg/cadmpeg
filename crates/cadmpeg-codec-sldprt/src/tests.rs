// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.sldprt` byte-fixture tests.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions, Encoder};
use cadmpeg_ir::decode::InspectOptions;
use cadmpeg_ir::LossCode;

use crate::container::{self, role, MARKER};
use crate::SldprtCodec;

fn sldprt_native(ir: &cadmpeg_ir::CadIr) -> crate::native::SldprtNative {
    crate::native::SldprtNative::load(
        ir.native
            .namespace("sldprt")
            .expect("SLDPRT native namespace"),
    )
    .unwrap()
}

fn update_sldprt_native<R>(
    ir: &mut cadmpeg_ir::CadIr,
    update: impl FnOnce(&mut crate::native::SldprtNative) -> R,
) -> R {
    let mut native = sldprt_native(ir);
    let result = update(&mut native);
    native.store(ir.native.namespace_mut("sldprt")).unwrap();
    result
}

#[test]
fn native_arenas_have_pinned_shape_and_typed_round_trip() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = decoded.ir.native.namespace("sldprt").unwrap();
    let typed = crate::native::SldprtNative::load(original).unwrap();
    let mut round_trip = cadmpeg_ir::NativeNamespace::default();
    typed.store(&mut round_trip).unwrap();
    assert_eq!(
        typed,
        crate::native::SldprtNative::load(&round_trip).unwrap()
    );
    assert_eq!(round_trip.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert_eq!(
        round_trip
            .arenas
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        crate::native::SLDPRT_ARENA_NAMES
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
fn native_version_one_migrates_the_body_selection_arena() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut legacy = decoded.ir.native.namespace("sldprt").unwrap().clone();
    legacy.version = 1;
    legacy.arenas.remove("feature_input_body_selections");

    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert_eq!(migrated.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(migrated
        .feature_input_lanes
        .iter()
        .all(|lane| lane.body_selections.is_empty()));
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).unwrap();
    assert_eq!(current.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(current.arenas.contains_key("feature_input_body_selections"));

    *decoded.ir.native.namespace_mut("sldprt") = legacy;
    assert!(crate::validate_native(&decoded.ir).is_empty());
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn native_version_two_migrates_the_edge_selection_arena() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut legacy = decoded.ir.native.namespace("sldprt").unwrap().clone();
    legacy.version = 2;
    legacy.arenas.remove("feature_input_edge_selections");

    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert_eq!(migrated.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(migrated
        .feature_input_lanes
        .iter()
        .all(|lane| lane.edge_selections.is_empty()));
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).unwrap();
    assert_eq!(current.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(current.arenas.contains_key("feature_input_edge_selections"));
}

#[test]
fn native_version_three_migrates_the_surface_selection_arena() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut legacy = decoded.ir.native.namespace("sldprt").unwrap().clone();
    legacy.version = 3;
    legacy.arenas.remove("feature_input_surface_selections");
    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert!(migrated
        .feature_input_lanes
        .iter()
        .all(|lane| lane.surface_selections.is_empty()));
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).unwrap();
    assert_eq!(current.version, crate::native::SLDPRT_NATIVE_VERSION);
    assert!(current
        .arenas
        .contains_key("feature_input_surface_selections"));
}

#[test]
fn native_version_four_migrates_sketch_marker_object_indices() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut legacy = decoded.ir.native.namespace("sldprt").unwrap().clone();
    legacy.version = 4;
    for record in legacy.arenas.get_mut("sketch_input_entities").unwrap() {
        record.fields.remove("object_index");
    }
    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert!(migrated.feature_input_lanes.iter().all(|lane| {
        lane.sketch_entities.iter().all(|entity| {
            usize::try_from(entity.offset).ok().and_then(|offset| {
                crate::resolved_features::marker_object_index(&lane.native_payload, offset)
            }) == entity.object_index
        })
    }));
    let mut current = cadmpeg_ir::NativeNamespace::default();
    migrated.store(&mut current).unwrap();
    assert_eq!(current.version, crate::native::SLDPRT_NATIVE_VERSION);

    let mut sentinel = decoded.ir.native.namespace("sldprt").unwrap().clone();
    sentinel.version = 6;
    sentinel.arenas.get_mut("sketch_input_entities").unwrap()[0]
        .fields
        .insert("object_index".into(), serde_json::json!(u32::MAX));
    let migrated = crate::native::SldprtNative::load(&sentinel).unwrap();
    assert_eq!(
        migrated.feature_input_lanes[0].sketch_entities[0].object_index,
        None
    );
}

#[test]
fn native_future_version_remains_rejected() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut future = decoded.ir.native.namespace("sldprt").unwrap().clone();
    future.version = crate::native::SLDPRT_NATIVE_VERSION + 1;
    let error = crate::native::SldprtNative::load(&future).unwrap_err();
    assert!(error
        .to_string()
        .contains("unsupported SLDPRT native namespace version"));
}

/// Nibble-swap a section name into its stored form (the swap is its own inverse,
/// so the decoder recovers the original).
fn swap_name(name: &str) -> Vec<u8> {
    name.bytes().map(|b| b.rotate_left(4)).collect()
}

fn raw_deflate(data: &[u8]) -> Vec<u8> {
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn crc32(data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(data);
    h.finalize()
}

/// Assemble one CRC-validated block frame carrying `payload`, named `section`.
fn make_block(type_id: u32, section: &str, payload: &[u8]) -> Vec<u8> {
    let comp = raw_deflate(payload);
    let preamble = swap_name(section);
    let mut b = Vec::new();
    b.extend_from_slice(&MARKER);
    b.extend_from_slice(&type_id.to_le_bytes());
    b.extend_from_slice(&crc32(payload).to_le_bytes());
    b.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    b.extend_from_slice(&(preamble.len() as u32).to_le_bytes());
    b.extend_from_slice(&preamble);
    b.extend_from_slice(&comp);
    b
}

/// A cache-cell grid entry: the marker, the `2L / L/2 / L` size triple, a name
/// length, and the nibble-swapped name.
fn make_cache_cell(logical_len: u32, name: &str) -> Vec<u8> {
    let swapped = swap_name(name);
    let mut b = Vec::new();
    b.extend_from_slice(&MARKER);
    b.extend_from_slice(&0u32.to_le_bytes()); // +6 type_id
    b.extend_from_slice(&(logical_len * 2).to_le_bytes()); // +10 2L
    b.extend_from_slice(&(logical_len / 2).to_le_bytes()); // +14 L/2
    b.extend_from_slice(&logical_len.to_le_bytes()); // +18 L
    b.extend_from_slice(&(swapped.len() as u32).to_le_bytes()); // +22 name_len
    b.extend_from_slice(&swapped);
    b
}

/// A tail section-directory entry naming an OPC part.
fn make_directory_entry(type_id: u32, size: u32, name: &str) -> Vec<u8> {
    let swapped = swap_name(name);
    let mut b = Vec::new();
    b.extend_from_slice(&MARKER);
    b.extend_from_slice(&type_id.to_le_bytes()); // +6
    b.extend_from_slice(&0u32.to_le_bytes()); // +10 zero
    b.extend_from_slice(&size.to_le_bytes()); // +14 size
    b.extend_from_slice(&0u32.to_le_bytes()); // +18 zero
    b.extend_from_slice(&(swapped.len() as u32).to_le_bytes()); // +22 name_len
    b.extend_from_slice(&[0u8; 14]); // +26 descriptor
    b.extend_from_slice(&swapped); // +40 name
    b.extend_from_slice(&[0xe5, 0x4b, 0x57, 0x5b, 0x00, 0x00]); // trailer
    b
}

/// A minimal Parasolid stream payload: `PS\0\0`, description, padding, a
/// length-prefixed schema token, then the class-definition record `body`.
fn parasolid_payload(description: &str, schema: &str) -> Vec<u8> {
    parasolid_with_body(description, schema, &[0u8; 8])
}

fn parasolid_with_body(description: &str, schema: &str, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&[b'P', b'S', 0x00, 0x00]);
    b.extend_from_slice(&(description.len() as u16).to_be_bytes());
    b.extend_from_slice(description.as_bytes());
    b.extend_from_slice(&[0x00, 0x00]); // padding
    b.push(schema.len() as u8);
    b.extend_from_slice(schema.as_bytes());
    b.extend_from_slice(body);
    b
}

const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

fn be16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_be_bytes());
}
fn be32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_be_bytes());
}
fn bef64(b: &mut Vec<u8>, v: f64) {
    b.extend_from_slice(&v.to_be_bytes());
}

/// A compact analytic plane carrier (tag `00 32`, 9 f64): origin, normal, refdir.
fn plane_carrier(attr: u16, origin: [f64; 3], normal: [f64; 3], refdir: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x32];
    be16(&mut b, attr);
    be32(&mut b, 0); // ordinal
    for _ in 0..5 {
        be16(&mut b, 0); // refs[5]
    }
    b.push(0x2b); // marker
    for v in origin.into_iter().chain(normal).chain(refdir) {
        bef64(&mut b, v);
    }
    b
}

/// A compact analytic line carrier (tag `00 1e`, 6 f64): point, direction.
fn line_carrier(attr: u16, point: [f64; 3], dir: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1e];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for v in point.into_iter().chain(dir) {
        bef64(&mut b, v);
    }
    b
}

fn prefixed_line_carrier(attr: u16, point: [f64; 3], dir: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1e];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for reference in [0u16, 0, 0, 0, 0] {
        be16(&mut b, reference);
        b.push(1);
    }
    b.push(0x2b);
    for value in point.into_iter().chain(dir) {
        bef64(&mut b, value);
    }
    b
}

fn cylinder_carrier(attr: u16, origin: [f64; 3], axis: [f64; 3], radius: f64) -> Vec<u8> {
    let mut b = vec![0x00, 0x33];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in origin
        .into_iter()
        .chain(axis)
        .chain([radius, 1.0, 0.0, 0.0])
    {
        bef64(&mut b, value);
    }
    b
}

fn cone_carrier(
    attr: u16,
    origin: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    half_angle: f64,
    reference: [f64; 3],
) -> Vec<u8> {
    let mut b = vec![0x00, 0x34];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in origin.into_iter().chain(axis).chain([
        radius,
        half_angle.sin(),
        half_angle.cos(),
        reference[0],
        reference[1],
        reference[2],
    ]) {
        bef64(&mut b, value);
    }
    b
}

fn torus_carrier(
    attr: u16,
    center: [f64; 3],
    axis: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
    reference: [f64; 3],
) -> Vec<u8> {
    let mut b = vec![0x00, 0x36];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in center.into_iter().chain(axis).chain([
        major_radius,
        minor_radius,
        reference[0],
        reference[1],
        reference[2],
    ]) {
        bef64(&mut b, value);
    }
    b
}

fn sphere_carrier(attr: u16, center: [f64; 3], radius: f64) -> Vec<u8> {
    let mut b = vec![0x00, 0x35];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in center
        .into_iter()
        .chain([radius, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0])
    {
        bef64(&mut b, value);
    }
    b
}

fn circle_carrier(attr: u16, center: [f64; 3], axis: [f64; 3], radius: f64) -> Vec<u8> {
    let mut b = vec![0x00, 0x1f];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    let reference = if axis[0].abs() > 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    for value in center
        .into_iter()
        .chain(axis)
        .chain(reference)
        .chain([radius])
    {
        bef64(&mut b, value);
    }
    b
}

fn ellipse_carrier(
    attr: u16,
    center: [f64; 3],
    axis: [f64; 3],
    major_direction: [f64; 3],
    major_radius: f64,
    minor_radius: f64,
) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x20];
    be16(&mut bytes, attr);
    be32(&mut bytes, 0);
    for _ in 0..5 {
        be16(&mut bytes, 0);
    }
    bytes.push(0x2b);
    for value in center
        .into_iter()
        .chain(axis)
        .chain(major_direction)
        .chain([major_radius, minor_radius])
    {
        bef64(&mut bytes, value);
    }
    bytes
}

fn closed_cylinder_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(cylinder_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(circle_carrier(70, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(circle_carrier(71, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(bridge(10, 20, 100));
    let mut first = loop_head(20, 30, 10);
    first[14..16].copy_from_slice(&21u16.to_be_bytes());
    b.extend(first);
    b.extend(loop_head(21, 31, 10));
    b.extend(coedge(30, 20, 30, 50, 0, 40, false));
    b.extend(coedge(31, 21, 31, 51, 0, 41, true));
    b.extend(edge_use(40, 70));
    b.extend(edge_use(41, 71));
    b.extend(vertex_use(50, 60));
    b.extend(vertex_use(51, 61));
    b.extend(world_point(60, [-1.0, 0.0, 0.0]));
    b.extend(world_point(61, [-1.0, 0.0, 1.0]));
    b
}

fn sphere_patch_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(sphere_carrier(100, [0.0, 0.0, 0.0], 1.0));
    b.extend(circle_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 1.0));
    b.extend(circle_carrier(71, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0));
    b.extend(circle_carrier(72, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(bridge(10, 20, 100));
    b.extend(loop_head(20, 30, 10));
    b.extend(coedge(30, 20, 31, 50, 0, 40, false));
    b.extend(coedge(31, 20, 32, 51, 0, 41, false));
    b.extend(coedge(32, 20, 30, 52, 0, 42, false));
    b.extend(edge_use(40, 70));
    b.extend(edge_use(41, 71));
    b.extend(edge_use(42, 72));
    b.extend(vertex_use(50, 60));
    b.extend(vertex_use(51, 61));
    b.extend(vertex_use(52, 62));
    b.extend(world_point(60, [1.0, 0.0, 0.0]));
    b.extend(world_point(61, [0.0, 1.0, 0.0]));
    b.extend(world_point(62, [0.0, 0.0, 1.0]));
    b
}

fn f64_array(tag: u8, attr: u16, values: &[f64]) -> Vec<u8> {
    let mut b = vec![0x00, tag, 0x2b];
    be32(&mut b, values.len() as u32);
    be16(&mut b, attr);
    for value in values {
        bef64(&mut b, *value);
    }
    b
}

fn u16_array(attr: u16, values: &[u16]) -> Vec<u8> {
    let mut b = vec![0x00, 0x7f, 0x2b];
    be32(&mut b, values.len() as u32);
    be16(&mut b, attr);
    for value in values {
        be16(&mut b, *value);
    }
    b
}

fn remove_array_type_markers(bytes: &mut Vec<u8>) {
    let mut offset = 0;
    while offset + 2 < bytes.len() {
        if bytes[offset] == 0
            && matches!(bytes[offset + 1], 0x2d | 0x7f | 0x80)
            && bytes[offset + 2] == 0x2b
        {
            bytes.remove(offset + 2);
        }
        offset += 1;
    }
}

fn nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let control_attr = descriptor_attr + 1;
    let mult_attr = descriptor_attr + 2;
    let knot_attr = descriptor_attr + 3;
    let mut b = vec![0x00, 0x86];
    be16(&mut b, wrapper_attr);
    be16(&mut b, descriptor_attr);
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&[0x00, 0x88]);
    be16(&mut b, descriptor_attr);
    be16(&mut b, 2);
    be32(&mut b, 3);
    be16(&mut b, 3);
    be32(&mut b, 2);
    b.push(0);
    be32(&mut b, 0);
    be16(&mut b, control_attr);
    be16(&mut b, mult_attr);
    be16(&mut b, knot_attr);
    b.extend(f64_array(
        0x2d,
        control_attr,
        &[0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 1.0, 0.0, 0.0],
    ));
    b.extend(u16_array(mult_attr, &[3, 3]));
    b.extend(f64_array(0x80, knot_attr, &[0.0, 1.0]));
    b
}

fn typed_nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let mut bytes = nurbs_curve_carrier(wrapper_attr, descriptor_attr);
    let descriptor = bytes.split_off(14);
    bytes.truncate(4);
    be32(&mut bytes, 0x1a);
    for reference in [
        descriptor_attr + 20,
        descriptor_attr + 21,
        descriptor_attr + 22,
    ] {
        be16(&mut bytes, reference);
    }
    be16(&mut bytes, 1);
    bytes.push(0x2b);
    be16(&mut bytes, descriptor_attr);
    be16(&mut bytes, descriptor_attr + 1);
    bytes.extend(descriptor);
    remove_array_type_markers(&mut bytes);
    bytes
}

fn rational_nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let control_attr = descriptor_attr + 1;
    let mult_attr = descriptor_attr + 2;
    let knot_attr = descriptor_attr + 3;
    let mut bytes = vec![0x00, 0x86];
    be16(&mut bytes, wrapper_attr);
    be16(&mut bytes, descriptor_attr);
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&[0x00, 0x88]);
    be16(&mut bytes, descriptor_attr);
    be16(&mut bytes, 2);
    be32(&mut bytes, 3);
    be16(&mut bytes, 4);
    be32(&mut bytes, 2);
    bytes.push(0);
    be32(&mut bytes, 0);
    be16(&mut bytes, control_attr);
    be16(&mut bytes, mult_attr);
    be16(&mut bytes, knot_attr);
    bytes.extend(f64_array(
        0x2d,
        control_attr,
        &[0.0, 0.0, 0.0, 1.0, 0.25, 0.5, 0.0, 0.5, 1.0, 0.0, 0.0, 1.0],
    ));
    bytes.extend(u16_array(mult_attr, &[3, 3]));
    bytes.extend(f64_array(0x80, knot_attr, &[0.0, 1.0]));
    bytes
}

fn linear_nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let control_attr = descriptor_attr + 1;
    let mult_attr = descriptor_attr + 2;
    let knot_attr = descriptor_attr + 3;
    let mut b = vec![0x00, 0x86];
    be16(&mut b, wrapper_attr);
    be16(&mut b, descriptor_attr);
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&[0x00, 0x88]);
    be16(&mut b, descriptor_attr);
    be16(&mut b, 1);
    be32(&mut b, 2);
    be16(&mut b, 3);
    be32(&mut b, 2);
    b.push(0);
    be32(&mut b, 0);
    be16(&mut b, control_attr);
    be16(&mut b, mult_attr);
    be16(&mut b, knot_attr);
    b.extend(f64_array(
        0x2d,
        control_attr,
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    ));
    b.extend(u16_array(mult_attr, &[2, 2]));
    b.extend(f64_array(0x80, knot_attr, &[0.0, 1.0]));
    b
}

fn rational_linear_nurbs_curve_carrier(wrapper_attr: u16, descriptor_attr: u16) -> Vec<u8> {
    let mut bytes = linear_nurbs_curve_carrier(wrapper_attr, descriptor_attr);
    let descriptor = bytes
        .windows(2)
        .position(|window| window == [0x00, 0x88])
        .unwrap();
    bytes[descriptor + 10..descriptor + 12].copy_from_slice(&4u16.to_be_bytes());
    let control = bytes
        .windows(3)
        .position(|window| window == [0x00, 0x2d, 0x2b])
        .unwrap();
    let old_end = control + 9 + 6 * 8;
    let mut replacement = f64_array(
        0x2d,
        descriptor_attr + 1,
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 0.5],
    );
    bytes.splice(control..old_end, replacement.drain(..));
    bytes
}

fn nurbs_surface_carrier(wrapper_attr: u16, descriptor_attr: u16, bridge_attr: u16) -> Vec<u8> {
    let control_attr = descriptor_attr + 1;
    let u_mult_attr = descriptor_attr + 2;
    let v_mult_attr = descriptor_attr + 3;
    let u_knot_attr = descriptor_attr + 4;
    let v_knot_attr = descriptor_attr + 5;
    let mut b = vec![0x00, 0x7c];
    be16(&mut b, wrapper_attr);
    be32(&mut b, 1);
    for reference in [0, bridge_attr, 0, 0, 0] {
        be16(&mut b, reference);
    }
    b.push(0x2b);
    be16(&mut b, descriptor_attr);
    be16(&mut b, 0);
    b.extend_from_slice(&[0x00, 0x7e]);
    be16(&mut b, descriptor_attr);
    b.extend_from_slice(&[0u8; 12]);
    for reference in [
        control_attr,
        u_mult_attr,
        v_mult_attr,
        u_knot_attr,
        v_knot_attr,
    ] {
        be16(&mut b, reference);
    }
    b.extend(f64_array(
        0x2d,
        control_attr,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5],
    ));
    b.extend(u16_array(u_mult_attr, &[2, 2]));
    b.extend(u16_array(v_mult_attr, &[2, 2]));
    b.extend(f64_array(0x80, u_knot_attr, &[0.0, 1.0]));
    b.extend(f64_array(0x80, v_knot_attr, &[0.0, 1.0]));
    b
}

fn rational_nurbs_surface_carrier(
    wrapper_attr: u16,
    descriptor_attr: u16,
    bridge_attr: u16,
) -> Vec<u8> {
    let mut bytes = nurbs_surface_carrier(wrapper_attr, descriptor_attr, bridge_attr);
    let control = bytes
        .windows(3)
        .position(|window| window == [0x00, 0x2d, 0x2b])
        .unwrap();
    let old_end = control + 9 + 12 * 8;
    let mut replacement = f64_array(
        0x2d,
        descriptor_attr + 1,
        &[
            0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 0.5, 1.0, 0.0, 0.0, 1.0, 0.5, 0.5, 0.25, 0.5,
        ],
    );
    bytes.splice(control..old_end, replacement.drain(..));
    bytes
}

fn markerless_nurbs_surface_carrier(
    wrapper_attr: u16,
    descriptor_attr: u16,
    bridge_attr: u16,
) -> Vec<u8> {
    let mut bytes = nurbs_surface_carrier(wrapper_attr, descriptor_attr, bridge_attr);
    remove_array_type_markers(&mut bytes);
    bytes
}

/// Bridge `00 0e`: `refs[2]` = loop head, `refs[4]` = surface carrier.
fn bridge(attr: u16, loop_attr: u16, surface_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x0e];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    be16(&mut b, 0); // p+6 ref0
    b.extend_from_slice(&MAGIC); // p+8..16
    let refs = [0u16, 0, loop_attr, 0, surface_attr];
    for r in refs {
        be16(&mut b, r); // p+16..26
    }
    b.push(0x2b); // p+26 marker
    b.extend_from_slice(&[0u8; 10]); // p+27..37 tail
    b
}

fn bridge_owned(attr: u16, loop_attr: u16, surface_attr: u16, owner: u16) -> Vec<u8> {
    let mut b = bridge(attr, loop_attr, surface_attr);
    b[8..10].copy_from_slice(&owner.to_be_bytes());
    b
}

fn entity51(flags: u32, attr: u16, disc: u16, slots: &[u16]) -> Vec<u8> {
    let mut b = vec![0x00, 0x51];
    be32(&mut b, flags);
    be16(&mut b, attr);
    be32(&mut b, 1);
    be16(&mut b, disc);
    for slot in slots {
        be16(&mut b, *slot);
    }
    b
}

fn count_entity51_family(payload: &[u8], flags: u32, disc: u16) -> usize {
    payload
        .windows(14)
        .filter(|window| {
            window[0..2] == [0x00, 0x51]
                && u32::from_be_bytes(window[2..6].try_into().unwrap()) == flags
                && u16::from_be_bytes(window[12..14].try_into().unwrap()) == disc
        })
        .count()
}

fn entity53_color(attr: u16, rgb: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x53];
    be32(&mut b, 3);
    be16(&mut b, attr);
    for value in rgb {
        bef64(&mut b, value);
    }
    b
}

/// Loop head `00 0f`: `refs[1]` = first coedge, `refs[2]` = owning bridge.
fn loop_head(attr: u16, first_coedge: u16, bridge_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x0f];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    let refs = [0u16, first_coedge, bridge_attr, 0];
    for r in refs {
        be16(&mut b, r); // p+6..14
    }
    b
}

/// Coedge `00 11`: `refs[1]` owner loop, `refs[3]` next, `refs[4]` start
/// vertex-use, `refs[5]` twin, `refs[6]` edge-use; marker is the local sense.
#[allow(clippy::too_many_arguments)]
fn coedge(
    attr: u16,
    owner_loop: u16,
    next: u16,
    start_vuse: u16,
    twin: u16,
    edge_use: u16,
    reversed: bool,
) -> Vec<u8> {
    let mut b = vec![0x00, 0x11];
    be16(&mut b, attr); // p+0
    let refs = [0u16, owner_loop, 0, next, start_vuse, twin, edge_use, 0, 0];
    for r in refs {
        be16(&mut b, r); // p+2..20
    }
    b.push(if reversed { 0x2d } else { 0x2b }); // p+20 marker
    b
}

fn tripled_coedge(
    attr: u16,
    owner_loop: u16,
    next: u16,
    start_vuse: u16,
    edge_use: u16,
) -> Vec<u8> {
    let mut b = vec![0x00, 0x11];
    be16(&mut b, attr);
    for reference in [0, owner_loop, 0, next, start_vuse, 0, edge_use, 0, 0] {
        be16(&mut b, reference);
        b.push(1);
    }
    b.push(0x2b);
    b
}

/// Edge-use `00 10`: `refs[3]` = support curve carrier (0 = none).
fn edge_use(attr: u16, curve_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x10];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    be16(&mut b, 0); // p+6 ref0
    b.extend_from_slice(&MAGIC); // p+8..16
    let refs = [0u16, 0, 0, curve_attr, 0, 0];
    for r in refs {
        be16(&mut b, r); // p+16..28
    }
    b
}

fn prefixed_edge_use(attr: u16, curve_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x10];
    be16(&mut b, attr);
    be32(&mut b, 0);
    be16(&mut b, 0);
    b.extend_from_slice(&[1, 0, 0]);
    b.extend_from_slice(&MAGIC);
    for reference in [0u16, 0, curve_attr] {
        b.push(1);
        be16(&mut b, reference);
    }
    b
}

/// Vertex-use `00 12`: `refs[4]` = world-point attr; magic at body+16.
fn vertex_use(attr: u16, point_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x12];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    let refs = [0u16, 0, 0, 0, point_attr];
    for r in refs {
        be16(&mut b, r); // p+6..16
    }
    b.extend_from_slice(&MAGIC); // p+16..24
    b
}

fn tripled_vertex_use(attr: u16, point_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x12];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for reference in [0u16, 0, 0, 0, point_attr] {
        be16(&mut b, reference);
        b.push(1);
    }
    b.extend_from_slice(&MAGIC);
    b
}

/// World point `00 1d`: xyz f64 BE (metres) at body+14.
fn world_point(attr: u16, xyz: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1d];
    be16(&mut b, attr); // p+0
    be32(&mut b, 0); // p+2 seq
    for _ in 0..4 {
        be16(&mut b, 0); // p+6..14 refs[4]
    }
    for v in xyz {
        bef64(&mut b, v); // p+14..38
    }
    b
}

fn tripled_world_point(attr: u16, xyz: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1d];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for reference in [0u16, 0, 0, 0] {
        be16(&mut b, reference);
        b.push(1);
    }
    for value in xyz {
        bef64(&mut b, value);
    }
    b
}

fn tripled_triangle_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
    ));
    b.extend(bridge(10, 20, 100));
    b.extend(loop_head(20, 30, 10));
    b.extend(tripled_coedge(30, 20, 31, 50, 40));
    b.extend(tripled_coedge(31, 20, 32, 51, 41));
    b.extend(tripled_coedge(32, 20, 30, 52, 42));
    b.extend(edge_use(40, 0));
    b.extend(edge_use(41, 0));
    b.extend(edge_use(42, 0));
    b.extend(tripled_vertex_use(50, 60));
    b.extend(tripled_vertex_use(51, 61));
    b.extend(tripled_vertex_use(52, 62));
    b.extend(tripled_world_point(60, [0.0, 0.0, 0.0]));
    b.extend(tripled_world_point(61, [1.0, 0.0, 0.0]));
    b.extend(tripled_world_point(62, [0.0, 1.0, 0.0]));
    b
}

fn prefixed_edge_triangle_body() -> Vec<u8> {
    let mut b = tripled_triangle_body();
    b.extend(prefixed_line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    b.extend(prefixed_edge_use(40, 70));
    b
}

/// One triangular planar face: a plane carrier, a bridge, a loop, three coedges
/// forming a closed ring, three edge-uses, three vertex-uses, and three points.
fn triangle_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    b.extend(bridge(10, 20, 100));
    b.extend(loop_head(20, 30, 10));
    b.extend(coedge(30, 20, 31, 50, 0, 40, false));
    b.extend(coedge(31, 20, 32, 51, 0, 41, false));
    b.extend(coedge(32, 20, 30, 52, 0, 42, false));
    b.extend(edge_use(40, 0));
    b.extend(edge_use(41, 0));
    b.extend(edge_use(42, 0));
    b.extend(vertex_use(50, 60));
    b.extend(vertex_use(51, 61));
    b.extend(vertex_use(52, 62));
    b.extend(world_point(60, [0.0, 0.0, 0.0]));
    b.extend(world_point(61, [1.0, 0.0, 0.0]));
    b.extend(world_point(62, [0.0, 1.0, 0.0]));
    b
}

fn triangle_body_with_overlapping_point() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    let mut face_bridge = bridge(10, 20, 100);
    face_bridge.splice(31..31, world_point(60, [0.0, 0.0, 0.0]));
    b.extend(face_bridge);
    b.extend(loop_head(20, 30, 10));
    b.extend(coedge(30, 20, 31, 50, 0, 40, false));
    b.extend(coedge(31, 20, 32, 51, 0, 41, false));
    b.extend(coedge(32, 20, 30, 52, 0, 42, false));
    b.extend(edge_use(40, 0));
    b.extend(edge_use(41, 0));
    b.extend(edge_use(42, 0));
    b.extend(vertex_use(50, 60));
    b.extend(vertex_use(51, 61));
    b.extend(vertex_use(52, 62));
    b.extend(world_point(61, [1.0, 0.0, 0.0]));
    b.extend(world_point(62, [0.0, 1.0, 0.0]));
    b
}

fn owned_triangle(base: u16, owner: u16, x: f64) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(plane_carrier(
        base + 100,
        [x, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    b.extend(bridge_owned(base + 10, base + 20, base + 100, owner));
    b.extend(loop_head(base + 20, base + 30, base + 10));
    b.extend(coedge(
        base + 30,
        base + 20,
        base + 31,
        base + 50,
        0,
        base + 40,
        false,
    ));
    b.extend(coedge(
        base + 31,
        base + 20,
        base + 32,
        base + 51,
        0,
        base + 41,
        false,
    ));
    b.extend(coedge(
        base + 32,
        base + 20,
        base + 30,
        base + 52,
        0,
        base + 42,
        false,
    ));
    b.extend(edge_use(base + 40, 0));
    b.extend(edge_use(base + 41, 0));
    b.extend(edge_use(base + 42, 0));
    b.extend(vertex_use(base + 50, base + 60));
    b.extend(vertex_use(base + 51, base + 61));
    b.extend(vertex_use(base + 52, base + 62));
    b.extend(world_point(base + 60, [x, 0.0, 0.0]));
    b.extend(world_point(base + 61, [x + 1.0, 0.0, 0.0]));
    b.extend(world_point(base + 62, [x, 1.0, 0.0]));
    b
}

/// A `.sldprt` whose partition block carries `triangle_body`.
fn sldprt_with_body(body: &[u8]) -> Vec<u8> {
    let mut f = outer_header();
    f.extend_from_slice(&make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", body),
    ));
    f
}

fn sldprt_with_body_and_material(body: &[u8], name: &str, rgb: [u8; 3]) -> Vec<u8> {
    let mut f = sldprt_with_body(body);
    f.extend(make_block(0x40, "SWObjects", &material_payload(name, rgb)));
    f
}

fn material_payload(name: &str, rgb: [u8; 3]) -> Vec<u8> {
    let mut material = b"moVisualProperties_c".to_vec();
    material.extend_from_slice(&u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 0]).to_le_bytes());
    material.extend_from_slice(&0u32.to_le_bytes());
    material.extend_from_slice(&0x00c0_c0c0u32.to_le_bytes());
    material.extend_from_slice(&[0xff, 0xfe, 0xff, 0x00]);
    material.extend_from_slice(&[0xff, 0xfe, 0xff, name.len() as u8]);
    for unit in name.encode_utf16() {
        material.extend_from_slice(&unit.to_le_bytes());
    }
    material
}

fn display_list_payload() -> Vec<u8> {
    fn descriptor(item_size: u32, kind: u32, count: u32, data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&item_size.to_le_bytes());
        b.extend_from_slice(&kind.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&count.to_le_bytes());
        b.extend_from_slice(data);
        b
    }
    let mut b = b"uoTempBodyTessData_c".to_vec();
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(b"uoTempFaceTessData_c");
    b.extend_from_slice(&[0u8; 8]);
    b.extend(descriptor(4, 8, 1, &3u32.to_le_bytes()));
    let mut positions = Vec::new();
    for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        positions.extend_from_slice(&value.to_le_bytes());
    }
    b.extend(descriptor(12, 100, 3, &positions));
    b.extend(descriptor(12, 100, 3, &[0u8; 36]));
    b.extend(descriptor(4, 8, 0, &[]));
    b.extend(descriptor(4, 8, 1, &4u32.to_le_bytes()));
    b.extend(descriptor(1, 8, 0, &[]));
    b
}

fn sldprt_with_body_and_display_list(body: &[u8]) -> Vec<u8> {
    let mut f = sldprt_with_body(body);
    f.extend(make_block(
        0x41,
        "Contents/DisplayLists",
        &display_list_payload(),
    ));
    f
}

fn sldprt_with_body_and_history(body: &[u8]) -> Vec<u8> {
    let mut f = sldprt_with_body(body);
    f.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords Name="Bracket"><Configuration Name="Default" Material="Steel" DisplayState="Shaded"/><Extrusion Name="Boss" Type="BossExtrude" id="7" Scope="Body1"><Dimension Name="Depth">12.5mm</Dimension><EquationDrivenCurve Name="Profile" id="8"/></Extrusion></Keywords>"#));
    f
}

fn resolved_features_payload(codes: &[u32]) -> Vec<u8> {
    resolved_features_payload_with_names(codes, &["Sketch1", "Boss-Extrude1", "D1"])
}

fn pmi_semantic_payload() -> Vec<u8> {
    pmi_semantic_payload_for("D1@Sketch1")
}

fn pmi_semantic_payload_for(cad_text: &str) -> Vec<u8> {
    fn string(bytes: &mut Vec<u8>, value: &str) {
        assert!(value.len() < 32);
        bytes.push(0xa0 | value.len() as u8);
        bytes.extend_from_slice(value.as_bytes());
    }
    let mut payload = b"unqlite".to_vec();
    payload.extend_from_slice(&[0; 57]);
    payload.extend_from_slice(b"01234567-89ab-cdef-0123-456789abcdef");
    payload.push(0x87);
    string(&mut payload, "annoType");
    payload.push(1);
    string(&mut payload, "cadText");
    string(&mut payload, cad_text);
    string(&mut payload, "dimItems");
    payload.push(0x91);
    payload.push(0x87);
    string(&mut payload, "class");
    string(&mut payload, "DimSemData");
    string(&mut payload, "dimSubType");
    string(&mut payload, "Linear");
    string(&mut payload, "isBasic");
    payload.push(0xc3);
    string(&mut payload, "isInspection");
    payload.push(0xc2);
    string(&mut payload, "isReferenceOnly");
    payload.push(0xc3);
    string(&mut payload, "valPrecision");
    payload.push(3);
    string(&mut payload, "value");
    payload.push(0xcb);
    payload.extend_from_slice(&0.025f64.to_be_bytes());
    string(&mut payload, "dimText");
    string(&mut payload, "25.000 mm");
    string(&mut payload, "dimType");
    payload.push(0);
    string(&mut payload, "iDString");
    string(&mut payload, "native-id");
    string(&mut payload, "reserved");
    payload.push(0xc0);
    payload
}

fn resolved_features_payload_with_names(codes: &[u32], names: &[&str]) -> Vec<u8> {
    resolved_features_payload_with_names_and_relation(codes, names, "sgPntPntDist")
}

fn resolved_feature_classes_with_ids(entries: &[(&str, &str, u32)]) -> Vec<u8> {
    let mut payload = Vec::new();
    for (class, name, object_id) in entries {
        payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
        payload.extend_from_slice(class.as_bytes());
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    payload
}

fn resolved_features_payload_with_names_and_relation(
    codes: &[u32],
    names: &[&str],
    relation_class: &str,
) -> Vec<u8> {
    resolved_features_payload_with_names_relation_and_scalar(codes, names, relation_class, 0.025)
}

fn resolved_features_payload_with_names_relation_and_scalar(
    codes: &[u32],
    names: &[&str],
    relation_class: &str,
    scalar_value: f64,
) -> Vec<u8> {
    let mut payload = Vec::new();
    for name in ["sgPointHandle", "sgLineHandle", "sgArcHandle"] {
        payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
        payload.extend_from_slice(name.as_bytes());
    }
    for name in names {
        if *name == "D1" {
            let class = relation_class;
            payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
            payload.extend_from_slice(&(class.len() as u16).to_le_bytes());
            payload.extend_from_slice(class.as_bytes());
        }
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        if name.starts_with('D')
            && name[1..]
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            payload.extend_from_slice(&[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00,
                0x00, 0x00, 0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
            ]);
            payload.extend_from_slice(&scalar_value.to_le_bytes());
            payload.extend_from_slice(&[
                0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02,
                0x00, 0x00,
            ]);
            payload.extend_from_slice(&[0; 5]);
            for index in [0u16, 2] {
                payload.extend_from_slice(&[0xd6, 0x80]);
                payload.extend_from_slice(&index.to_le_bytes());
                payload.extend_from_slice(&[0xff; 4]);
                payload.extend_from_slice(&[0; 4]);
            }
        }
    }
    for (ordinal, code) in codes.iter().enumerate() {
        payload.extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
        let mut record = [0u8; 87];
        // o+5..13: shared-geometry header (eight 0xff bytes).
        record[..8].fill(0xff);
        // o+13..17: -1.0f32 geometry sentinel.
        record[8..12].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        // o+17..21: native sketch-entity type code.
        record[12..16].copy_from_slice(&code.to_le_bytes());
        // o+21..27: profile-curve locus descriptor.
        record[16..22].copy_from_slice(&[0x00, 0x00, 0x04, 0x00, 0x02, 0x00]);
        // o+27..29: profile-curve role.
        record[22..24].copy_from_slice(&1u16.to_le_bytes());
        // o+31..39: -1.0f32 sentinel followed by the marker state descriptor.
        record[26..34].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        // o+48..56: state value.
        record[43..51].copy_from_slice(&(ordinal as f64 + 1.0).to_le_bytes());
        // o+70..80: local-link sentinel (zero selector padding, -1.0f64 marker).
        record[65..67].copy_from_slice(&[0, 0]);
        record[67..75].copy_from_slice(&(-1.0f64).to_le_bytes());
        // o+88..92: trailing local id.
        record[83..87].copy_from_slice(&((ordinal + 1) as u32).to_le_bytes());
        payload.extend_from_slice(&record);
    }
    payload
}

fn sldprt_with_body_and_resolved_features(body: &[u8], codes: &[u32]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(codes),
    ));
    file
}

fn sldprt_with_nested_sketch_profile(body: &[u8]) -> Vec<u8> {
    sldprt_with_nested_sketch_profiles(body, 1)
}

fn sldprt_with_nested_sketch_profiles(body: &[u8], count: usize) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[0, 1, 1, 1]);
    for _ in 0..count {
        payload.extend(parasolid_with_body(
            "feature input sketch",
            "SCH_SW_33103_11000",
            &triangle_body(),
        ));
    }
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

fn sldprt_with_compact_relation_pair(body: &[u8]) -> Vec<u8> {
    sldprt_with_tagged_compact_relation(body, "sgPntPntDist", [[0xd6, 0x80]; 2])
}

fn sldprt_with_tagged_compact_relation(
    body: &[u8],
    relation_class: &str,
    operand_tags: [[u8; 2]; 2],
) -> Vec<u8> {
    sldprt_with_tagged_compact_relation_names(
        body,
        relation_class,
        operand_tags,
        &["Sketch1", "D1", "D2"],
    )
}

fn sldprt_with_tagged_compact_relation_names(
    body: &[u8],
    relation_class: &str,
    operand_tags: [[u8; 2]; 2],
    names: &[&str],
) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload =
        resolved_features_payload_with_names_and_relation(&[0, 1, 1, 1], names, relation_class);
    let operand_offsets = payload
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == [0xd6, 0x80]).then_some(offset))
        .collect::<Vec<_>>();
    for (ordinal, offset) in operand_offsets.into_iter().enumerate() {
        payload[offset..offset + 2].copy_from_slice(&operand_tags[ordinal % 2]);
    }
    let d1_marker = [0x04, 0x80, 0xff, 0xfe, 0xff, 2, b'D', 0, b'1', 0];
    let d1_offset = payload
        .windows(d1_marker.len())
        .position(|window| window == d1_marker)
        .expect("D1 scalar name");
    payload[d1_offset + 69] = 1;
    payload.extend(parasolid_with_body(
        "feature input sketch",
        "SCH_SW_33103_11000",
        &triangle_body(),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

fn sldprt_with_tagged_compact_relation_scalar(
    body: &[u8],
    relation_class: &str,
    operand_tags: [[u8; 2]; 2],
    scalar_value: f64,
) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload_with_names_relation_and_scalar(
        &[0, 1, 1, 1],
        &["Sketch1", "D1", "D2"],
        relation_class,
        scalar_value,
    );
    let operand_offsets = payload
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == [0xd6, 0x80]).then_some(offset))
        .collect::<Vec<_>>();
    for (ordinal, offset) in operand_offsets.into_iter().enumerate() {
        payload[offset..offset + 2].copy_from_slice(&operand_tags[ordinal % 2]);
    }
    let d1_marker = [0x04, 0x80, 0xff, 0xfe, 0xff, 2, b'D', 0, b'1', 0];
    let d1_offset = payload
        .windows(d1_marker.len())
        .position(|window| window == d1_marker)
        .expect("D1 scalar name");
    payload[d1_offset + 69] = 1;
    payload.extend(parasolid_with_body(
        "feature input sketch",
        "SCH_SW_33103_11000",
        &triangle_body(),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

fn sldprt_with_compressed_nested_sketch_profile(body: &[u8]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[0, 1, 1, 1]);
    payload.extend_from_slice(&[
        0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef,
        0x99, 0, 0, 0, 0,
    ]);
    payload.extend(zlib(&parasolid_with_body(
        "feature input compressed sketch",
        "SCH_SW_33103_11000",
        &triangle_body(),
    )));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

fn circular_sketch_body() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(70, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 70));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [1.0, 0.0, 0.0]));
    body
}

fn sldprt_with_nested_circular_sketch(body: &[u8]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[2]);
    payload.extend(parasolid_with_body(
        "feature input circular sketch",
        "SCH_SW_33103_11000",
        &circular_sketch_body(),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

fn arc_sketch_body() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(70, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 70));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [1.0, 0.0, 0.0]));
    body.extend(world_point(61, [0.0, 1.0, 0.0]));
    body.extend(world_point(62, [0.0, 0.0, 0.0]));
    body
}

fn sldprt_with_nested_arc_sketch(body: &[u8]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[0, 2, 1, 1]);
    payload.extend(parasolid_with_body(
        "feature input arc sketch",
        "SCH_SW_33103_11000",
        &arc_sketch_body(),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

fn sldprt_with_nested_elliptical_sketch(body: &[u8]) -> Vec<u8> {
    let mut sketch = Vec::new();
    sketch.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    sketch.extend(ellipse_carrier(
        70,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
        2.0,
        1.0,
    ));
    sketch.extend(bridge(10, 20, 100));
    sketch.extend(loop_head(20, 30, 10));
    sketch.extend(coedge(30, 20, 30, 50, 0, 40, false));
    sketch.extend(edge_use(40, 70));
    sketch.extend(vertex_use(50, 60));
    sketch.extend(world_point(60, [0.0, 2.0, 0.0]));

    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[2]);
    payload.extend(parasolid_with_body(
        "feature input elliptical sketch",
        "SCH_SW_33103_11000",
        &sketch,
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

fn nurbs_sketch_body(rational: bool) -> Vec<u8> {
    let mut body = triangle_body();
    body.extend(if rational {
        rational_nurbs_curve_carrier(70, 80)
    } else {
        nurbs_curve_carrier(70, 80)
    });
    body.extend(edge_use(40, 70));
    body
}

fn sldprt_with_nested_nurbs_sketches(body: &[u8]) -> Vec<u8> {
    let mut file = sldprt_with_body(body);
    let mut payload = resolved_features_payload(&[1, 1]);
    payload.extend(parasolid_with_body(
        "feature input spline sketch",
        "SCH_SW_33103_11000",
        &nurbs_sketch_body(false),
    ));
    payload.extend(parasolid_with_body(
        "feature input rational spline sketch",
        "SCH_SW_33103_11000",
        &nurbs_sketch_body(true),
    ));
    file.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    file
}

fn sldprt_with_body_and_envelope(body: &[u8]) -> Vec<u8> {
    let mut f = sldprt_with_body(body);
    let mut payload = b"moBBoxCenterData_c".to_vec();
    payload.extend_from_slice(&1u32.to_le_bytes());
    for value in [0.01f64, 0.02, -0.03, 0.04] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(b"moDefaultRefPlnData_c");
    for value in [0.001f64, 0.002, 0.003, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(b"moTransRefPlaneData_c");
    payload.extend_from_slice(&[0xff; 8]);
    for value in [0.01f64, 0.02, 0.03, 0.1, 0.2, 1.0, 0.0, -1.0, 0.5] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(b"moPart_c");
    let mut part = [0u8; 13];
    part[0..4].copy_from_slice(&42u32.to_le_bytes());
    part[8..12].copy_from_slice(&2026u32.to_le_bytes());
    payload.extend_from_slice(&part);
    payload.extend_from_slice(b"moConfigurationMgr_c");
    let mut configuration = [0u8; 125];
    configuration[66..70].copy_from_slice(&17u32.to_le_bytes());
    configuration[107] = 3;
    configuration[117..125].copy_from_slice(&132_537_600_000_000_000u64.to_le_bytes());
    payload.extend_from_slice(&configuration);
    payload.extend_from_slice(b"moLengthUserUnits_c");
    payload.extend_from_slice(&[0xff, 0xfe, 0xff, 4, b'I', 0, b'N', 0]);
    f.extend(make_block(0x43, "SWObjects", &payload));
    f.extend(make_block(
        0x44,
        "Units",
        br#"<Metadata><Property Name="SW_UnitsLinear" Value="0"/></Metadata>"#,
    ));
    f
}

fn sldprt_with_partition_and_deltas(partition: &[u8], deltas: &[u8]) -> Vec<u8> {
    let mut f = outer_header();
    f.extend_from_slice(&make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", partition),
    ));
    f.extend_from_slice(&make_block(
        0x21,
        "Contents/Config-0-Deltas",
        &parasolid_with_body("deltas body", "SCH_SW_33103_11000", deltas),
    ));
    f
}

fn sldprt_with_colliding_sites() -> Vec<u8> {
    let mut f = outer_header();
    f.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 700, 0.0),
        ),
    ));
    f.extend(make_block(
        0x21,
        "Contents/Config-1-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 701, 10.0),
        ),
    ));
    f
}

/// The 8-byte outer header (`file_id`, then big-endian `version == 4`).
fn outer_header() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&0x0000_0001u32.to_le_bytes());
    b.extend_from_slice(&0x0000_0004u32.to_be_bytes());
    b
}

/// A synthetic `.sldprt`: header, a PNG-preview block, a Parasolid block, a
/// cache cell, and a tail-directory entry.
fn synthetic_sldprt() -> Vec<u8> {
    let mut f = outer_header();
    f.extend_from_slice(&make_block(
        0x10,
        "PreviewPNG",
        &[0x89, b'P', b'N', b'G', 1, 2, 3, 4],
    ));
    f.extend_from_slice(&make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_payload("partition body", "SCH_SW_33103_11000"),
    ));
    f.extend_from_slice(&make_cache_cell(90, "Contents/DisplayLists"));
    f.extend_from_slice(&make_directory_entry(0x30, 2, "[Content_Types].xml"));
    f
}

#[test]
fn detect_high_on_marker_after_header() {
    let f = synthetic_sldprt();
    assert_eq!(SldprtCodec.detect(&f), Confidence::High);
    assert_eq!(
        SldprtCodec.detect(b"\x00\x01\x02\x03 no marker here"),
        Confidence::No
    );
}

#[test]
fn compound_detection_distinguishes_solidworks_and_generic_signals() {
    let mut file = vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    file.resize(512, 0);
    for byte in b"ISolidWorksInformation" {
        file.extend_from_slice(&[*byte, 0]);
    }
    assert_eq!(SldprtCodec.detect(&file), Confidence::High);

    let generic_compound_document = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    assert_eq!(
        SldprtCodec.detect(&generic_compound_document),
        Confidence::Low
    );
}

#[test]
fn scan_classifies_blocks_cells_and_directory() {
    let f = synthetic_sldprt();
    let scan = container::scan_bytes(&f);
    assert_eq!(scan.version, 0x0000_0004);
    assert_eq!(scan.blocks.len(), 2);
    assert_eq!(scan.cache_cells.len(), 1);
    assert_eq!(scan.directory.len(), 1);

    let png = &scan.blocks[0];
    assert_eq!(png.section.as_deref(), Some("PreviewPNG"));
    assert_eq!(png.family, "png-preview");

    let ps = &scan.blocks[1];
    assert_eq!(ps.section.as_deref(), Some("Contents/Config-0-Partition"));
    assert_eq!(ps.family, "parasolid");

    assert_eq!(scan.cache_cells[0].name, "Contents/DisplayLists");
    assert_eq!(scan.cache_cells[0].logical_len, 90);
    assert_eq!(scan.directory[0].name, "[Content_Types].xml");
}

#[test]
fn decode_surfaces_preview_and_solidworks_xml_metadata() {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&640u32.to_be_bytes());
    png.extend_from_slice(&480u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 1]);
    png.extend_from_slice(&0u32.to_be_bytes());

    let mut bmp = vec![0; 28];
    bmp[4..8].copy_from_slice(&40u32.to_le_bytes());
    bmp[8..12].copy_from_slice(&320i32.to_le_bytes());
    bmp[12..16].copy_from_slice(&(-200i32).to_le_bytes());
    bmp[16..18].copy_from_slice(&1u16.to_le_bytes());
    bmp[18..20].copy_from_slice(&8u16.to_le_bytes());
    bmp[20..24].copy_from_slice(&1u32.to_le_bytes());
    bmp[24..28].copy_from_slice(&12_345u32.to_le_bytes());

    let xml = br#"<?xml version="1.0"?><swSolidWorks swVersion="34000" swCreationTime="1700000000" swPath="C:\part.SLDPRT"><swModel id="1" swName="Part" swConfigurationName="Default"/><swConfigurationList><swConfiguration swID="0" swName="Default" swMostRecentConfiguration="NO" swConfigurationNeedsUpdate="YES" swConfigurationFlags="384" swConfigurationAlternateName="Default derived"/></swConfigurationList></swSolidWorks>"#;
    let mut source = outer_header();
    source.extend(make_block(0x10, "PreviewPNG", &png));
    source.extend(make_block(0x11, "PreviewBMP", &bmp));
    source.extend(make_block(0x12, "SolidWorksMetadata", xml));
    source.extend(make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body(
            "partition body",
            "SCH_SW_33103_11000",
            &owned_triangle(0, 700, 0.0),
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode metadata fixture");
    let attributes = &decoded.ir.source.expect("source metadata").attributes;
    assert_eq!(attributes["png_preview_count"], "1");
    assert_eq!(attributes["png_preview_0_width"], "640");
    assert_eq!(attributes["png_preview_0_height"], "480");
    assert_eq!(attributes["png_preview_0_color_type"], "6");
    assert_eq!(attributes["bmp_thumbnail_count"], "1");
    assert_eq!(attributes["bmp_thumbnail_0_width"], "320");
    assert_eq!(attributes["bmp_thumbnail_0_height"], "-200");
    assert_eq!(attributes["bmp_thumbnail_0_compression"], "1");
    assert_eq!(attributes["sw_version"], "34000");
    assert_eq!(attributes["sw_creation_time_unix"], "1700000000");
    assert_eq!(attributes["sw_path"], r"C:\part.SLDPRT");
    assert_eq!(attributes["sw_name"], "Part");
    assert_eq!(attributes["sw_configuration_name"], "Default");
    assert_eq!(attributes["sw_configuration_0_needs_update"], "YES");
    assert_eq!(attributes["sw_configuration_0_most_recent"], "NO");
    assert_eq!(attributes["sw_configuration_0_flags"], "384");
    assert_eq!(
        attributes["sw_configuration_0_alternate_name"],
        "Default derived"
    );
}

#[test]
fn parasolid_stream_header_is_parsed() {
    let f = synthetic_sldprt();
    let scan = container::scan_bytes(&f);
    let (block, header) = container::select_active_parasolid(&scan).expect("active parasolid");
    assert_eq!(header.schema, "SCH_SW_33103_11000");
    assert!(header.description.contains("partition"));
    assert_eq!(block.family, "parasolid");
    assert!(crate::parasolid::is_body_stream(&header));
}

#[test]
fn parasolid_extracts_every_direct_stream_in_block() {
    let mut payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    payload.extend(parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &world_point(60, [2.0, 0.0, 0.0]),
    ));
    let streams = crate::parasolid::extract_streams(&payload);
    assert_eq!(streams.len(), 2);
    assert!(crate::parasolid::stream_header(&streams[0])
        .unwrap()
        .description
        .contains("partition"));
    assert!(crate::parasolid::stream_header(&streams[1])
        .unwrap()
        .description
        .contains("deltas"));
}

#[test]
fn parasolid_mesh_polyline_decodes_counted_xyz_array() {
    let description = b"boundary_polyline mesh";
    let schema = b"SCH_3201255_32001_13006";
    let mut stream = b"PS\0\0".to_vec();
    stream.extend((description.len() as u16).to_be_bytes());
    stream.extend(description);
    stream.push(schema.len() as u8);
    stream.extend(schema);
    stream.extend([0xff, 0xff, 0xff, 0xff, 0x00, 0x22]);
    stream.extend(6u32.to_be_bytes());
    stream.extend([0x00, 0x22]);
    for value in [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
        stream.extend(value.to_be_bytes());
    }
    assert_eq!(
        crate::parasolid::mesh_polyline(&stream),
        Some(vec![
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0),
        ])
    );
}

#[test]
fn helix_polyline_fit_recovers_axis_radius_and_rise() {
    let points = (0..=64)
        .map(|index| {
            let t = f64::from(index) / 64.0;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            cadmpeg_ir::math::Point3::new(
                10.0 + 3.5 * angle.cos(),
                20.0 - 3.2 * t,
                30.0 + 3.5 * angle.sin(),
            )
        })
        .collect::<Vec<_>>();
    let (origin, axis, radius, rise) =
        crate::resolved_features::fit_helix_polyline(&points, 0.25, false).unwrap();
    assert!((origin.x - 10.0).abs() < 1.0e-9);
    assert!((origin.y - 20.0).abs() < 1.0e-9);
    assert!((origin.z - 30.0).abs() < 1.0e-9);
    assert_eq!(axis, cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0));
    assert!((radius - 3.5).abs() < 1.0e-9);
    assert!((rise - 3.2).abs() < 1.0e-9);
}

#[test]
fn spatial_vertex_record_decodes_model_coordinates() {
    let mut payload = vec![0x55; 7];
    payload.extend([
        0xff, 0xfe, 0xff, 0x06, b'V', 0x00, b'e', 0x00, b'r', 0x00, b't', 0x00, b'e', 0x00, b'x',
        0x00,
    ]);
    payload.extend([0x00; 27]);
    payload.extend([0x0e, 0x00]);
    for value in [1.25f64, -2.5, 3.75] {
        payload.extend(value.to_le_bytes());
    }
    assert_eq!(
        crate::resolved_features::spatial_vertex_coordinates(&payload),
        vec![cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)]
    );
    payload[7 + 43] = 0x1e;
    assert!(crate::resolved_features::spatial_vertex_coordinates(&payload).is_empty());
}

#[test]
fn inspect_enumerates_every_structure() {
    let f = synthetic_sldprt();
    let mut cur = Cursor::new(f);
    let summary = SldprtCodec
        .inspect(&mut cur, &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format, "sldprt");
    assert_eq!(summary.container_kind, "sldprt-blocks");
    assert_eq!(
        summary
            .entries
            .iter()
            .filter(|e| e.role == role::BLOCK)
            .count(),
        2
    );
    assert!(summary.entries.iter().any(|e| e.role == role::CACHE_CELL));
    assert!(summary
        .entries
        .iter()
        .any(|e| e.role == role::DIRECTORY_ENTRY));
    assert!(summary
        .notes
        .iter()
        .any(|n| n.contains("active Parasolid B-rep candidate")));
}

#[test]
fn decode_without_geometry_falls_back_to_metadata() {
    let f = synthetic_sldprt();
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.report.geometry_transferred);
    assert_eq!(result.ir.native_unknowns("sldprt").unwrap().len(), 1);
    assert_eq!(result.source_fidelity.retained_records.len(), 2);
    assert!(result
        .source_fidelity
        .retained_record("sldprt:file:source-image#0")
        .is_some_and(|record| record.data.is_some()));
    assert!(result
        .source_fidelity
        .retained_records
        .iter()
        .any(|record| record.id != "sldprt:file:source-image#0" && record.sha256.len() == 64));
    let source = result.ir.source.as_ref().expect("source metadata");
    assert_eq!(source.format, "sldprt");
    assert_eq!(
        source
            .attributes
            .get("parasolid_schema")
            .map(String::as_str),
        Some("SCH_SW_33103_11000")
    );
}

#[test]
fn decode_explicit_empty_partition_and_deltas_as_an_empty_model() {
    let source = sldprt_with_partition_and_deltas(&[], &[]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report.geometry_transferred);
    assert!(decoded.ir.model.bodies.is_empty());
    assert!(!decoded.report.losses.iter().any(|loss| {
        loss.message.contains("geometry was not transferred")
            || loss.message.contains("topology graph")
    }));
}

#[test]
fn metadata_fallback_binds_resolved_feature_scalars() {
    let mut source = synthetic_sldprt();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round1" Type="Fillet"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Round1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.report.geometry_transferred);
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .expect("metadata fillet feature");
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("metadata D1 parameter");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter.native_ref.is_some());
    assert!(decoded.report.losses.iter().any(|loss| loss
        .message
        .contains("typed feature(s) retain native or unresolved required operation operands")));
}

#[test]
fn retained_source_image_round_trips_byte_exactly() {
    let source = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(source.clone());
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.source_fidelity.annotations.provenance.is_empty());
    for coedge in &result.ir.model.coedges {
        assert!(result
            .ir
            .model
            .coedges
            .iter()
            .any(|candidate| candidate.id == coedge.radial_next));
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&result.ir, &result.source_fidelity, &mut encoded)
        .unwrap();
    assert_eq!(encoded, source);
}

#[test]
fn encoder_writes_source_less_ir() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let scan = container::scan_bytes(&encoded);
    assert_eq!(scan.blocks.len(), 1);
    assert_eq!(scan.directory.len(), 1);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 6);
    assert_eq!(decoded.ir.model.edges.len(), 12);
    assert_eq!(decoded.ir.model.vertices.len(), 8);
}

#[test]
fn encoder_rejects_source_less_unresolved_extrusion_profile() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId, Length,
        ProfileRef, Termination,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#extrude".into()),
        ordinal: 0,
        name: Some("Extrude".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved("native:missing-owner".into()),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(10.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::Join,
            direction_source: None,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });

    let error = SldprtCodec.encode(&ir, &mut Vec::new()).unwrap_err();
    assert!(error
        .to_string()
        .contains("requires retained extrusion profile data"));
}

#[test]
fn encoder_writes_source_less_line_sketches() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, Feature, FeatureDefinition, FeatureId,
        Length, PathRef, ProfileRef, RevolveExtent, Termination,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let sketch_id = SketchId("synthetic:test:sketch#profile".into());
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(0.0, 10.0),
    ];
    let entity_ids = (0..3)
        .map(|index| SketchEntityId(format!("synthetic:test:sketch-entity#line-{index}")))
        .collect::<Vec<_>>();
    for index in 0..3 {
        ir.model.sketch_entities.push(SketchEntity {
            id: entity_ids[index].clone(),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: points[index],
                end: points[(index + 1) % 3],
            },
        });
    }
    for index in 0..3 {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId(format!("synthetic:test:constraint#coincident-{index}")),
            sketch: sketch_id.clone(),
            definition: SketchConstraintDefinition::CoincidentLoci {
                loci: vec![
                    SketchLocus::End(entity_ids[index].clone()),
                    SketchLocus::Start(entity_ids[(index + 1) % 3].clone()),
                ],
            },
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    }
    for (suffix, definition) in [
        (
            "fixed",
            SketchConstraintDefinition::Fixed {
                entity: entity_ids[1].clone(),
            },
        ),
        (
            "horizontal",
            SketchConstraintDefinition::Horizontal {
                entity: entity_ids[0].clone(),
            },
        ),
        (
            "vertical",
            SketchConstraintDefinition::Vertical {
                entity: entity_ids[2].clone(),
            },
        ),
    ] {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId(format!("synthetic:test:constraint#{suffix}")),
            sketch: sketch_id.clone(),
            definition,
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    }
    ir.model.sketch_entities.push(SketchEntity {
        id: SketchEntityId("synthetic:test:sketch-entity#point".into()),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Point {
            position: Point2::new(4.0, 5.0),
        },
    });
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![entity_ids
            .iter()
            .cloned()
            .map(|entity| SketchEntityUse {
                entity,
                reversed: false,
            })
            .collect()],
        native_ref: None,
    });
    let sketch_feature_id = FeatureId("synthetic:test:feature#profile".into());
    ir.model.features.push(Feature {
        id: sketch_feature_id.clone(),
        ordinal: 0,
        name: Some("Profile".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: None,
    });
    let profile = ProfileRef::Sketch(sketch_id.clone());
    let path = PathRef::Sketch(sketch_id.clone());
    let generated = [
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: Some(profile.clone()),
                axis: Some(cadmpeg_ir::features::RevolutionAxis {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    direction: Vector3::new(0.0, 1.0, 0.0),
                }),
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle: Angle(1.2) },
                }),
                axis_reference: None,
                solid: Some(true),
                face_maker_class: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            op: BooleanOp::NewBody,
        },
        FeatureDefinition::Sweep {
            profile: Some(profile.clone()),
            sections: Vec::new(),
            path: Some(path.clone()),
            mode: cadmpeg_ir::features::SweepMode::Solid {
                op: BooleanOp::Join,
            },
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: Some(Angle(0.3)),
            scale: Some(1.5),
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Loft {
            sections: vec![
                cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
                cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
            ],
            guides: vec![path],
            centerline: None,
            op: BooleanOp::NewBody,
            closed: false,
            solid: true,
            ruled: false,
            max_degree: None,
            check_compatibility: None,
            allow_multi_profile_faces: None,
        },
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(profile),
                direction: Some(Vector3::new(0.0, 0.0, 1.0)),
                thickness: Some(Length(2.5)),
                side: Some(cadmpeg_ir::features::RibSide::Centered),
                draft: cadmpeg_ir::features::RibDraft::Angle(Angle(0.1)),
            },
            op: BooleanOp::Join,
        },
    ];
    for (index, definition) in generated.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#profile-op-{index}")),
            ordinal: index as u64 + 2,
            name: Some(format!("Profile op {index}")),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#extrude".into()),
        ordinal: 1,
        name: Some("Boss".into()),
        suppressed: Some(false),
        parent: Some(sketch_feature_id),
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(sketch_id),
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3::new(
                0.0, 0.0, 1.0,
            )),
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(12.0),
                    },
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::Join,
            direction_source: None,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    });

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let scan = container::scan_bytes(&encoded);
    assert!(scan.blocks.iter().any(|block| {
        block
            .section
            .as_deref()
            .is_some_and(|section| section == "Contents/Config-0-ResolvedFeatures")
    }));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let marker_lane = &sldprt_native(&decoded.ir).feature_input_lanes[0];
    assert_eq!(
        marker_lane
            .sketch_entities
            .iter()
            .filter(|marker| marker.coordinates_m.is_some())
            .count(),
        7
    );
    let marker_relations = marker_lane
        .sketch_entities
        .iter()
        .filter(|marker| matches!(marker.kind, crate::records::SketchInputKind::Relation(_)))
        .collect::<Vec<_>>();
    assert_eq!(marker_relations.len(), 3);
    assert!(marker_relations
        .iter()
        .all(|marker| marker.links.len() == 2 && marker.link_selector == Some(0)));
    assert!(marker_relations
        .iter()
        .all(|marker| marker.links.iter().all(|link| marker_lane
            .sketch_entities
            .iter()
            .any(|candidate| candidate.id == link.entity_ref
                && candidate.local_id == Some(u32::from(link.local_id))))));
    assert_eq!(decoded.ir.model.sketches.len(), 1);
    assert_eq!(decoded.ir.model.sketches[0].profiles.len(), 1);
    assert_eq!(decoded.ir.model.sketches[0].profiles[0].len(), 3);
    assert_eq!(decoded.ir.model.sketch_entities.len(), 4);
    assert_eq!(
        decoded.ir.model.sketch_constraints.len(),
        6,
        "{:?}",
        decoded.ir.model.sketch_constraints
    );
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::Horizontal { .. }
            )
        }));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::Vertical { .. }
            )
        }));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::Fixed { .. }
            )
        }));
    assert_eq!(
        decoded
            .ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Line { .. }))
            .count(),
        3
    );
    assert!(decoded
        .ir
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(
            entity.geometry,
            SketchGeometry::Point { position }
                if (position.u - 4.0).abs() < 1.0e-12
                    && (position.v - 5.0).abs() < 1.0e-12
        )));
    assert!(decoded.ir.model.features.iter().any(|feature| matches!(
        feature.definition,
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(_),
            ..
        }
    )));
    assert!(decoded.ir.model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(12.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }
    )));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Revolve { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Sweep { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Loft { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Rib { .. })));
    let point = decoded
        .ir
        .model
        .sketch_entities
        .iter_mut()
        .find_map(|entity| match &mut entity.geometry {
            SketchGeometry::Point { position } => Some(position),
            _ => None,
        })
        .unwrap();
    point.u = 7.0;
    point.v = 8.0;
    let mut rewritten = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut rewritten)
        .unwrap();
    let rewritten = SldprtCodec
        .decode(&mut Cursor::new(rewritten), &DecodeOptions::default())
        .unwrap();
    assert!(rewritten
        .ir
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(
            entity.geometry,
            SketchGeometry::Point { position }
                if (position.u - 7.0).abs() < 1.0e-12
                    && (position.v - 8.0).abs() < 1.0e-12
        )));
}

#[test]
fn encoder_writes_source_less_spatial_point_and_line_sketches() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::sketches::{
        SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
        SpatialSketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let sketch_id = SpatialSketchId("synthetic:test:spatial-sketch#path".into());
    let entity_id = SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#line".into());
    let start = Point3::new(1.25, -2.5, 3.75);
    let end = Point3::new(4.5, 5.25, -6.0);
    let second_start = Point3::new(-7.0, 8.5, 9.25);
    let second_end = Point3::new(10.0, -11.5, 12.75);
    let point = Point3::new(-13.0, 14.25, 15.5);
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch_id.clone(),
        name: Some("Spatial path".into()),
        configuration: Some("0".into()),
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#a-point".into()),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Point { position: point },
    });
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: entity_id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line { start, end },
    });
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: SpatialSketchEntityId("synthetic:test:spatial-sketch-entity#second-line".into()),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Line {
            start: second_start,
            end: second_end,
        },
    });
    ir.model.features.push(Feature {
        id: FeatureId("synthetic:test:feature#spatial-path".into()),
        ordinal: 0,
        name: Some("Spatial path".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SpatialSketch {
            sketch: Some(sketch_id),
        },
        native_ref: None,
    });

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir.model.spatial_sketches.len(), 1);
    assert_eq!(regenerated.ir.model.spatial_sketch_entities.len(), 3);
    assert!(matches!(
        regenerated.ir.model.spatial_sketch_entities[0].geometry,
        SpatialSketchGeometry::Point { position }
            if (position.x - point.x).abs() < 1.0e-12
                && (position.y - point.y).abs() < 1.0e-12
                && (position.z - point.z).abs() < 1.0e-12
    ));
    assert!(matches!(
        regenerated.ir.model.spatial_sketch_entities[1].geometry,
        SpatialSketchGeometry::Line {
            start: regenerated_start,
            end: regenerated_end,
        } if regenerated_start == start && regenerated_end == end
    ));
    assert!(matches!(
        regenerated.ir.model.spatial_sketch_entities[2].geometry,
        SpatialSketchGeometry::Line {
            start: regenerated_start,
            end: regenerated_end,
        } if regenerated_start == second_start && regenerated_end == second_end
    ));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::SpatialSketch { sketch: Some(_) }
    ));

    let edited_start = Point3::new(13.0, 14.0, 15.0);
    let edited_end = Point3::new(-16.0, 17.0, 18.0);
    let edited_point = Point3::new(19.0, -20.0, 21.5);
    regenerated.ir.model.spatial_sketch_entities[0].geometry = SpatialSketchGeometry::Point {
        position: edited_point,
    };
    regenerated.ir.model.spatial_sketch_entities[2].geometry = SpatialSketchGeometry::Line {
        start: edited_start,
        end: edited_end,
    };
    let mut rewritten = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &regenerated.ir,
            &regenerated.source_fidelity,
            &mut rewritten,
        )
        .unwrap();
    let rewritten = SldprtCodec
        .decode(&mut Cursor::new(rewritten), &DecodeOptions::default())
        .unwrap();
    assert_eq!(rewritten.ir.model.spatial_sketch_entities.len(), 3);
    assert!(matches!(
        rewritten.ir.model.spatial_sketch_entities[0].geometry,
        SpatialSketchGeometry::Point { position }
            if (position.x - edited_point.x).abs() < 1.0e-12
                && (position.y - edited_point.y).abs() < 1.0e-12
                && (position.z - edited_point.z).abs() < 1.0e-12
    ));
    assert!(matches!(
        rewritten.ir.model.spatial_sketch_entities[2].geometry,
        SpatialSketchGeometry::Line { start, end }
            if start == edited_start && end == edited_end
    ));
}

#[test]
fn encoder_rejects_unrepresentable_source_less_sketch_constraints() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let sketch_id = SketchId("synthetic:test:sketch#profile".into());
    let entity_id = SketchEntityId("synthetic:test:sketch-entity#line".into());
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }]],
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity {
        id: entity_id.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    });
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:constraint#horizontal".into()),
        sketch: sketch_id,
        definition: SketchConstraintDefinition::Horizontal { entity: entity_id },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });

    let error = SldprtCodec.encode(&ir, &mut Vec::new()).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(_)
    ));
    assert!(error
        .to_string()
        .contains("requires an owning sketch feature"));
}

#[test]
fn encoder_writes_source_less_curved_sketches() {
    use cadmpeg_ir::features::{
        Angle, DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length,
        ParameterId, ParameterValue,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let sketch_id = SketchId("synthetic:test:sketch#curves".into());
    let geometries = vec![
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(8.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
        SketchGeometry::Arc {
            center: Point2::new(16.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(std::f64::consts::PI),
            end_angle: Angle(std::f64::consts::TAU),
        },
        SketchGeometry::Ellipse {
            center: Point2::new(0.0, 8.0),
            major_angle: Angle(0.4),
            major_radius: Length(3.0),
            minor_radius: Length(1.5),
            start_angle: None,
            end_angle: None,
        },
        SketchGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(6.0, 6.0),
                Point2::new(10.0, 10.0),
                Point2::new(6.0, 6.0),
            ],
            weights: Some(vec![1.0, 0.75, 1.0]),
            periodic: false,
        },
        SketchGeometry::Line {
            start: Point2::new(6.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
        SketchGeometry::Line {
            start: Point2::new(18.0, 0.0),
            end: Point2::new(14.0, 0.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(24.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(std::f64::consts::FRAC_PI_2),
            end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
        },
        SketchGeometry::Line {
            start: Point2::new(24.0, -2.0),
            end: Point2::new(24.0, 2.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(8.0, 0.0),
            radius: Length(3.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
        SketchGeometry::Line {
            start: Point2::new(5.0, 0.0),
            end: Point2::new(11.0, 0.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(40.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::FRAC_PI_2),
        },
        SketchGeometry::Line {
            start: Point2::new(40.0, 2.0),
            end: Point2::new(42.0, 0.0),
        },
        SketchGeometry::Point {
            position: Point2::new(30.0, 0.0),
        },
        SketchGeometry::Point {
            position: Point2::new(34.0, 0.0),
        },
        SketchGeometry::Point {
            position: Point2::new(30.0, 4.0),
        },
        SketchGeometry::Point {
            position: Point2::new(41.0, 1.0),
        },
        SketchGeometry::Circle {
            center: Point2::new(8.0, 2.0),
            radius: Length(2.0),
        },
        SketchGeometry::Line {
            start: Point2::new(50.0, 0.0),
            end: Point2::new(54.0, 0.0),
        },
        SketchGeometry::Line {
            start: Point2::new(54.0, 4.0),
            end: Point2::new(50.0, 4.0),
        },
        SketchGeometry::Arc {
            center: Point2::new(52.0, 0.0),
            radius: Length(2.0),
            start_angle: Angle(0.0),
            end_angle: Angle(std::f64::consts::PI),
        },
        SketchGeometry::Arc {
            center: Point2::new(52.0, 4.0),
            radius: Length(2.0),
            start_angle: Angle(std::f64::consts::PI),
            end_angle: Angle(std::f64::consts::TAU),
        },
        SketchGeometry::Circle {
            center: Point2::new(8.0, 0.0),
            radius: Length(2.0),
        },
        SketchGeometry::Ellipse {
            center: Point2::new(60.0, 0.0),
            major_angle: Angle(0.0),
            major_radius: Length(3.0),
            minor_radius: Length(1.5),
            start_angle: Some(Angle(0.0)),
            end_angle: Some(Angle(std::f64::consts::FRAC_PI_2)),
        },
        SketchGeometry::Line {
            start: Point2::new(60.0, 1.5),
            end: Point2::new(63.0, 0.0),
        },
    ];
    let entity_ids = geometries
        .into_iter()
        .enumerate()
        .map(|(index, geometry)| {
            let id = SketchEntityId(format!("synthetic:test:sketch-entity#curve-{index:02}"));
            ir.model.sketch_entities.push(SketchEntity {
                id: id.clone(),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry,
            });
            id
        })
        .collect::<Vec<_>>();
    let profile = |indices: &[usize]| {
        indices
            .iter()
            .map(|index| SketchEntityUse {
                entity: entity_ids[*index].clone(),
                reversed: false,
            })
            .collect()
    };
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Curves".into()),
        configuration: Some("Main".into()),
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![
            profile(&[0]),
            profile(&[1, 5]),
            profile(&[2, 6]),
            profile(&[7, 8]),
            profile(&[9, 10]),
            profile(&[11, 12]),
            profile(&[17]),
            profile(&[18, 20]),
            profile(&[19, 21]),
            profile(&[3]),
            profile(&[4]),
            profile(&[22]),
            profile(&[23, 24]),
        ],
        native_ref: None,
    });
    let feature_id = FeatureId("synthetic:test:feature#curves".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("Curves".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: None,
    });
    let distance_parameter = ParameterId("synthetic:test:parameter#00-distance".into());
    let point_line_parameter = ParameterId("synthetic:test:parameter#01-point-line".into());
    let line_line_parameter = ParameterId("synthetic:test:parameter#02-line-line".into());
    let horizontal_parameter = ParameterId("synthetic:test:parameter#03-horizontal".into());
    let vertical_parameter = ParameterId("synthetic:test:parameter#04-vertical".into());
    let angle_parameter = ParameterId("synthetic:test:parameter#05-angle".into());
    let radius_parameter = ParameterId("synthetic:test:parameter#06-radius".into());
    let diameter_parameter = ParameterId("synthetic:test:parameter#07-diameter".into());
    for (id, ordinal, name, expression, display, value) in [
        (
            distance_parameter.clone(),
            0,
            "D10",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            point_line_parameter.clone(),
            1,
            "D11",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            line_line_parameter.clone(),
            2,
            "D12",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            horizontal_parameter.clone(),
            3,
            "H1",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            vertical_parameter.clone(),
            4,
            "V1",
            "4mm",
            None,
            ParameterValue::Length(Length(4.0)),
        ),
        (
            angle_parameter.clone(),
            5,
            "A1",
            "90deg",
            None,
            ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2)),
        ),
        (
            radius_parameter.clone(),
            6,
            "R1",
            "R2mm",
            Some(DimensionDisplay::Radius),
            ParameterValue::Length(Length(2.0)),
        ),
        (
            diameter_parameter.clone(),
            7,
            "DIA1",
            "<MOD-DIAM>4mm",
            Some(DimensionDisplay::Diameter),
            ParameterValue::Length(Length(4.0)),
        ),
    ] {
        ir.model.parameters.push(DesignParameter {
            id,
            owner: Some(feature_id.clone()),
            ordinal,
            name: name.into(),
            expression: expression.into(),
            display,
            value: Some(value),
            dependencies: Vec::new(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    }
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:constraint#arc-angle".into()),
        sketch: sketch_id.clone(),
        definition: SketchConstraintDefinition::ArcAngle {
            entity: entity_ids[1].clone(),
            angle: Angle(std::f64::consts::PI),
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:constraint#arc-angle-ellipse".into()),
        sketch: sketch_id.clone(),
        definition: SketchConstraintDefinition::EllipseAngle {
            entity: entity_ids[23].clone(),
            angle: Angle(std::f64::consts::FRAC_PI_2),
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    for (suffix, definition) in [
        (
            "collinear",
            SketchConstraintDefinition::Collinear {
                first: entity_ids[5].clone(),
                second: entity_ids[6].clone(),
            },
        ),
        (
            "concentric",
            SketchConstraintDefinition::Concentric {
                first: entity_ids[1].clone(),
                second: entity_ids[9].clone(),
            },
        ),
        (
            "coradial",
            SketchConstraintDefinition::Coradial {
                first: entity_ids[1].clone(),
                second: entity_ids[22].clone(),
            },
        ),
        (
            "dimension-angle",
            SketchConstraintDefinition::Angle {
                first: entity_ids[5].clone(),
                second: entity_ids[8].clone(),
                parameter: angle_parameter,
            },
        ),
        (
            "dimension-diameter",
            SketchConstraintDefinition::Diameter {
                entity: entity_ids[17].clone(),
                parameter: diameter_parameter,
            },
        ),
        (
            "dimension-horizontal",
            SketchConstraintDefinition::HorizontalDistance {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[14].clone()),
                parameter: horizontal_parameter,
            },
        ),
        (
            "dimension-line-line",
            SketchConstraintDefinition::Distance {
                entities: vec![entity_ids[18].clone(), entity_ids[19].clone()],
                parameter: line_line_parameter,
            },
        ),
        (
            "dimension-point-line",
            SketchConstraintDefinition::DistanceLoci {
                first: SketchLocus::Entity(entity_ids[15].clone()),
                second: SketchLocus::Entity(entity_ids[5].clone()),
                parameter: point_line_parameter,
            },
        ),
        (
            "dimension-vertical",
            SketchConstraintDefinition::VerticalDistance {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[15].clone()),
                parameter: vertical_parameter,
            },
        ),
        (
            "distance",
            SketchConstraintDefinition::DistanceLoci {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[14].clone()),
                parameter: distance_parameter,
            },
        ),
        (
            "equal-arcs",
            SketchConstraintDefinition::Equal {
                first: entity_ids[1].clone(),
                second: entity_ids[2].clone(),
            },
        ),
        (
            "equal-lines",
            SketchConstraintDefinition::Equal {
                first: entity_ids[5].clone(),
                second: entity_ids[6].clone(),
            },
        ),
        (
            "horizontal-points",
            SketchConstraintDefinition::HorizontalPoints {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[14].clone()),
            },
        ),
        (
            "midpoint",
            SketchConstraintDefinition::Midpoint {
                point: SketchLocus::Entity(entity_ids[16].clone()),
                entity: entity_ids[12].clone(),
            },
        ),
        (
            "parallel",
            SketchConstraintDefinition::Parallel {
                first: entity_ids[5].clone(),
                second: entity_ids[6].clone(),
            },
        ),
        (
            "perpendicular",
            SketchConstraintDefinition::Perpendicular {
                first: entity_ids[5].clone(),
                second: entity_ids[8].clone(),
            },
        ),
        (
            "radius",
            SketchConstraintDefinition::Radius {
                entity: entity_ids[0].clone(),
                parameter: radius_parameter,
            },
        ),
        (
            "tangent",
            SketchConstraintDefinition::Tangent {
                first: entity_ids[5].clone(),
                second: entity_ids[17].clone(),
            },
        ),
        (
            "vertical-points",
            SketchConstraintDefinition::VerticalPoints {
                first: SketchLocus::Entity(entity_ids[13].clone()),
                second: SketchLocus::Entity(entity_ids[15].clone()),
            },
        ),
    ] {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId(format!("synthetic:test:constraint#{suffix}")),
            sketch: sketch_id.clone(),
            definition,
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.sketches.len(), 1);
    assert_eq!(decoded.ir.model.sketch_entities.len(), 29);
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Coradial { .. }
        )));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::EllipseAngle { .. }
        )));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::DistanceLoci { .. }
        )));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Radius { .. }
        )));
    assert!(decoded.ir.model.parameters.iter().any(|parameter| {
        parameter.name == "D10" && parameter.value == Some(ParameterValue::Length(Length(4.0)))
    }));
    assert!(decoded.ir.model.parameters.iter().any(|parameter| {
        parameter.name == "R1" && parameter.display == Some(DimensionDisplay::Radius)
    }));
    for name in ["D11", "D12", "H1", "V1", "A1", "DIA1"] {
        assert!(
            decoded
                .ir
                .model
                .parameters
                .iter()
                .any(|parameter| parameter.name == name),
            "missing regenerated {name} dimension parameter"
        );
    }
    for expected in ["line-line", "horizontal", "vertical", "angle", "diameter"] {
        assert!(
            decoded
                .ir
                .model
                .sketch_constraints
                .iter()
                .any(|constraint| matches!(
                    (expected, &constraint.definition),
                    ("line-line", SketchConstraintDefinition::Distance { .. })
                        | (
                            "horizontal",
                            SketchConstraintDefinition::HorizontalDistance { .. }
                        )
                        | (
                            "vertical",
                            SketchConstraintDefinition::VerticalDistance { .. }
                        )
                        | ("angle", SketchConstraintDefinition::Angle { .. })
                        | ("diameter", SketchConstraintDefinition::Diameter { .. })
                )),
            "missing regenerated {expected} dimension"
        );
    }
    let native = sldprt_native(&decoded.ir);
    let circle_relation = native.feature_input_lanes[0]
        .relation_instances
        .iter()
        .find(|relation| {
            relation.family == crate::records::FeatureInputRelationFamily::CircleDiameter
                && relation.parameter_scalar_ref.as_deref()
                    == decoded
                        .ir
                        .model
                        .parameters
                        .iter()
                        .find(|parameter| parameter.name == "DIA1")
                        .and_then(|parameter| parameter.native_ref.as_deref())
        })
        .expect("diameter relation instance");
    let [operand] = circle_relation.operands.as_slice() else {
        panic!("one diameter operand");
    };
    let marker = native.feature_input_lanes[0]
        .sketch_entities
        .iter()
        .find(|marker| Some(marker.id.as_str()) == operand.entity_ref.as_deref())
        .expect("resolved diameter marker");
    assert_eq!(marker.kind, crate::records::SketchInputKind::LineOrCircle);
    assert_ne!(marker.local_id, Some(u32::from(operand.entity_index)));
    assert!(native.feature_input_lanes[0]
        .relation_instances
        .iter()
        .flat_map(|relation| &relation.operands)
        .all(|operand| operand.entity_ref.is_some()));
    assert!(native.feature_input_lanes[0]
        .relation_instances
        .iter()
        .flat_map(|relation| &relation.operands)
        .filter(|operand| {
            matches!(
                operand.kind,
                crate::records::FeatureInputOperandKind::D6
                    | crate::records::FeatureInputOperandKind::E1
                    | crate::records::FeatureInputOperandKind::Native(0x8dcb | 0x8dda)
            )
        })
        .any(|operand| {
            native.feature_input_lanes[0]
                .sketch_entities
                .iter()
                .find(|marker| Some(marker.id.as_str()) == operand.entity_ref.as_deref())
                .is_some_and(|marker| marker.local_id != Some(u32::from(operand.entity_index)))
        }));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::ArcAngle {
                    angle: Angle(value),
                    ..
                } if (value - std::f64::consts::PI).abs() < 1.0e-12
            )
        }));
    for expected in [
        crate::records::SketchRelationKind::Parallel,
        crate::records::SketchRelationKind::Perpendicular,
        crate::records::SketchRelationKind::Equal,
        crate::records::SketchRelationKind::Collinear,
        crate::records::SketchRelationKind::Concentric,
        crate::records::SketchRelationKind::HorizontalPoints,
        crate::records::SketchRelationKind::VerticalPoints,
        crate::records::SketchRelationKind::Midpoint,
        crate::records::SketchRelationKind::Tangent,
    ] {
        assert!(sldprt_native(&decoded.ir)
            .feature_input_lanes
            .iter()
            .flat_map(|lane| &lane.sketch_entities)
            .any(|marker| marker.kind == crate::records::SketchInputKind::Relation(expected)));
    }
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Parallel { .. }
        )));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Perpendicular { .. }
        )));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Collinear { .. }
        )));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Concentric { .. }
        )));
    assert!(
        decoded
            .ir
            .model
            .sketch_constraints
            .iter()
            .filter(|constraint| matches!(
                constraint.definition,
                SketchConstraintDefinition::Equal { .. }
            ))
            .count()
            >= 2
    );
    for definition in [
        "horizontal_points",
        "vertical_points",
        "midpoint",
        "tangent",
    ] {
        assert!(decoded
            .ir
            .model
            .sketch_constraints
            .iter()
            .any(|constraint| {
                matches!(
                    (&constraint.definition, definition),
                    (
                        SketchConstraintDefinition::HorizontalPoints { .. },
                        "horizontal_points"
                    ) | (
                        SketchConstraintDefinition::VerticalPoints { .. },
                        "vertical_points"
                    ) | (SketchConstraintDefinition::Midpoint { .. }, "midpoint")
                        | (SketchConstraintDefinition::Tangent { .. }, "tangent")
                )
            }));
    }
    assert_eq!(
        decoded
            .ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Circle { .. }))
            .count(),
        3
    );
    assert_eq!(
        decoded
            .ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SketchGeometry::Arc { .. }))
            .count(),
        7
    );
    assert!(decoded
        .ir
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(entity.geometry, SketchGeometry::Ellipse { .. })));
    assert!(decoded
        .ir
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(entity.geometry, SketchGeometry::Nurbs { .. })));

    let parameter = ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "D10")
        .expect("source distance parameter");
    parameter.expression = "5mm".into();
    parameter.value = Some(ParameterValue::Length(Length(5.0)));
    let error = SldprtCodec.encode(&ir, &mut Vec::new()).unwrap_err();
    assert!(error
        .to_string()
        .contains("not satisfied by measured geometry"));
}

#[test]
fn encoder_binds_multiple_source_less_sketches_by_object_id() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    for (ordinal, name) in ["Profile", "Profile"].into_iter().enumerate() {
        let sketch_id = SketchId(format!("synthetic:test:sketch#named-{ordinal}"));
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: Some(name.into()),
            configuration: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, ordinal as f64),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: Vec::new(),
            native_ref: None,
        });
        ir.model.sketch_entities.push(SketchEntity {
            id: SketchEntityId(format!("synthetic:test:sketch-entity#named-{ordinal}")),
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(ordinal as f64, ordinal as f64 + 1.0),
            },
        });
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#named-{ordinal}")),
            ordinal: ordinal as u64,
            name: Some(name.into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch_id),
            },
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.sketches.len(), 2);
    assert_eq!(
        decoded
            .ir
            .model
            .sketches
            .iter()
            .filter_map(|sketch| sketch.name.as_deref())
            .collect::<Vec<_>>(),
        ["Profile", "Profile"]
    );
    let bound = decoded
        .ir
        .model
        .features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } => Some(sketch),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bound.len(), 2);
    assert_ne!(bound[0], bound[1]);
}

#[test]
fn encoder_writes_source_less_native_features() {
    use cadmpeg_ir::features::{
        Angle, BodySelection, BooleanOp, ChamferSpec, EdgeSelection, FaceMotion, FaceSelection,
        Feature, FeatureDefinition, FeatureId, HoleKind, Length, PatternKind, RadiusSpec,
        Termination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let seed_id = FeatureId("sldprt:model:feature#generated:0".into());
    ir.model.features.push(Feature {
        id: seed_id.clone(),
        ordinal: 0,
        name: Some("Boss".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "BossExtrude".into(),
            parameters: BTreeMap::from([("Depth".into(), "25mm".into())]),
            properties: BTreeMap::new(),
        },
        native_ref: None,
    });
    let definitions = vec![
        FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: EdgeSelection::Resolved {
                    edges: vec![ir.model.edges[0].id.clone()],
                    native: "edge-a,edge-b".into(),
                },
                radius: RadiusSpec::Constant {
                    radius: Length(3.0),
                },
                tangency_weight: None,
            }],
        },
        FeatureDefinition::Chamfer {
            groups: vec![cadmpeg_ir::features::ChamferGroup {
                edges: EdgeSelection::Native("edge-c".into()),
                spec: ChamferSpec::TwoDistances {
                    first: Length(1.0),
                    second: Length(2.0),
                },
            }],
            flip_direction: false,
        },
        FeatureDefinition::Shell {
            removed_faces: FaceSelection::Resolved {
                faces: vec![ir.model.faces[0].id.clone()],
                native: "face-a".into(),
            },
            thickness: Some(Length(1.5)),
            outward: Some(true),
            mode: None,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        },
        FeatureDefinition::Draft {
            faces: FaceSelection::Native("face-b".into()),
            neutral_plane: FaceSelection::Native("face-c".into()),
            pull_direction: Some(Vector3::new(0.0, 0.0, 1.0)),
            angle: Some(Angle(0.2)),
            outward: Some(false),
        },
        FeatureDefinition::Combine {
            target: BodySelection::Resolved {
                bodies: vec![ir.model.bodies[0].id.clone()],
                native: "body-a".into(),
            },
            tools: BodySelection::Native("body-b,body-c".into()),
            op: BooleanOp::Join,
        },
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Native("face-d".into()),
            heal: true,
        },
        FeatureDefinition::MoveFace {
            faces: FaceSelection::Native("face-e".into()),
            motion: FaceMotion::Rotate {
                axis_origin: Point3::new(1.0, 2.0, 3.0),
                axis_dir: Vector3::new(0.0, 1.0, 0.0),
                angle: Angle(0.4),
            },
        },
        FeatureDefinition::Dome {
            faces: FaceSelection::Native("face-f".into()),
            height: Some(Length(4.0)),
            elliptical: Some(true),
            reverse: Some(false),
        },
        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: Some(FaceSelection::Native("face-g".into())),
            position: None,
            direction: None,
            placements: vec![cadmpeg_ir::features::HolePlacement::Directed {
                position: Point3::new(3.0, 4.0, 5.0),
                direction: Vector3::new(0.0, 0.0, -1.0),
            }],
            kind: HoleKind::Countersink {
                diameter: Length(8.0),
                angle: Angle(1.4),
            },
            exit_kind: None,
            diameter: Some(Length(5.0)),
            extent: Some(Termination::Blind {
                length: Length(20.0),
            }),
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
    ];
    for (index, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#direct-{index}")),
            ordinal: index as u64 + 1,
            name: Some(format!("Direct {index}")),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    let patterns = [
        PatternKind::Linear {
            direction: Some(Vector3::new(1.0, 0.0, 0.0)),
            spacing: Length(10.0),
            count: 3,
            second: Some(cadmpeg_ir::features::LinearPatternDirection {
                direction: Vector3::new(0.0, 1.0, 0.0),
                spacing: Length(20.0),
                count: 4,
            }),
        },
        PatternKind::Circular {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_dir: Vector3::new(0.0, 0.0, 1.0),
            angle: Angle(std::f64::consts::TAU),
            count: 6,
        },
        PatternKind::Mirror {
            plane_origin: Point3::new(0.0, 0.0, 0.0),
            plane_normal: Vector3::new(1.0, 0.0, 0.0),
        },
    ];
    for (index, pattern) in patterns.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#pattern-{index}")),
            ordinal: index as u64 + 10,
            name: Some(format!("Pattern {index}")),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Pattern {
                seeds: vec![cadmpeg_ir::features::PatternSeed::Feature(seed_id.clone())],
                pattern,
            },
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let scan = container::scan_bytes(&encoded);
    assert!(scan.blocks.iter().any(|block| {
        block
            .section
            .as_deref()
            .is_some_and(|section| section.starts_with("Contents/Keywords-"))
    }));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(25.0),
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
    assert_eq!(
        sldprt_native(&decoded.ir).feature_histories[0].features[0].xml_tag,
        "Extrusion"
    );
    let native_features = &sldprt_native(&decoded.ir).feature_histories[0].features;
    let source_ids = native_features
        .iter()
        .map(|feature| {
            feature
                .source_id
                .as_deref()
                .expect("generated features have source ids")
                .parse::<u32>()
                .expect("generated feature source ids are numeric")
        })
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(source_ids.len(), native_features.len());
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Fillet { .. })));
    assert!(decoded.ir.model.features.iter().any(|feature| matches!(
        feature.definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Linear {
                second: Some(cadmpeg_ir::features::LinearPatternDirection {
                    direction: Vector3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0
                    },
                    spacing: Length(20.0),
                    count: 4,
                }),
                ..
            },
            ..
        }
    )));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Chamfer { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Shell { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Draft { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Combine { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::DeleteFace { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::MoveFace { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Dome { .. })));
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Hole { .. })));
    assert_eq!(
        decoded
            .ir
            .model
            .features
            .iter()
            .filter(|feature| matches!(feature.definition, FeatureDefinition::Pattern { .. }))
            .count(),
        3
    );
}

#[test]
fn semantic_writer_round_trips_flex_operations() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, FlexMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Flex Name="Bend" Type="Flex" id="44" Mode="Bending" Axis="0,1,0"><Dimension Name="Angle">30deg</Dimension></Flex></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let FeatureDefinition::Flex { axis, mode } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed flex feature");
    };
    assert_eq!(*axis, Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)));
    assert!(matches!(
        mode,
        FlexMode::Bending { angle }
            if (angle.0 - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
    *mode = FlexMode::Twisting { angle: Angle(0.75) };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].xml_tag,
        "Flex"
    );
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::Flex {
            axis,
            mode: FlexMode::Twisting { angle },
        } if *axis == Some(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0))
            && (angle.0 - 0.75).abs() < 1e-12
    ));
}

#[test]
fn semantic_writer_round_trips_all_flex_modes() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, FlexMode, Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Flex Name="Bend" Type="Flex" id="1" Mode="Bending" Axis="1,0,0"><Dimension Name="Angle">10deg</Dimension></Flex>
            <Flex Name="Twist" Type="Flex" id="2" Mode="Twisting" Axis="0,1,0"><Dimension Name="Angle">20deg</Dimension></Flex>
            <Flex Name="Taper" Type="Flex" id="3" Mode="Tapering" Axis="0,0,1"><Dimension Name="Factor">1.5</Dimension></Flex>
            <Flex Name="Stretch" Type="Flex" id="4" Mode="Stretching" Axis="1,1,0"><Dimension Name="Distance">8mm</Dimension></Flex>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    for feature in &mut decoded.ir.model.features {
        if let FeatureDefinition::Flex { mode, .. } = &mut feature.definition {
            *mode = match feature.name.as_deref().unwrap() {
                "Bend" => FlexMode::Bending { angle: Angle(0.1) },
                "Twist" => FlexMode::Twisting { angle: Angle(0.2) },
                "Taper" => FlexMode::Tapering { factor: 2.0 },
                "Stretch" => FlexMode::Stretching {
                    distance: Length(12.0),
                },
                name => panic!("unexpected flex {name}"),
            };
        } else {
            panic!("untyped flex feature");
        }
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let modes = regenerated
        .ir
        .model
        .features
        .iter()
        .map(|feature| &feature.definition)
        .collect::<Vec<_>>();
    assert!(
        matches!(modes[0], FeatureDefinition::Flex { mode: FlexMode::Bending { angle }, .. } if (angle.0 - 0.1).abs() < 1e-12)
    );
    assert!(
        matches!(modes[1], FeatureDefinition::Flex { mode: FlexMode::Twisting { angle }, .. } if (angle.0 - 0.2).abs() < 1e-12)
    );
    assert!(
        matches!(modes[2], FeatureDefinition::Flex { mode: FlexMode::Tapering { factor }, .. } if (*factor - 2.0).abs() < 1e-12)
    );
    assert!(
        matches!(modes[3], FeatureDefinition::Flex { mode: FlexMode::Stretching { distance }, .. } if (distance.0 - 12.0).abs() < 1e-12)
    );
}

#[test]
fn semantic_writer_retains_partial_native_flex_construction() {
    use cadmpeg_ir::features::{FeatureDefinition, FlexForm, FlexMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Flex Name="Axis" Type="Flex" id="1" Mode="Bending" Axis="0,0,0"><Dimension Name="Angle">10deg</Dimension></Flex>
            <Flex Name="Angle" Type="Flex" id="2" Mode="Twisting" Axis="0,1,0"><Dimension Name="Angle">NaNrad</Dimension></Flex>
            <Flex Name="Taper" Type="Flex" id="3" Mode="Tapering" Axis="0,0,1"><Dimension Name="Factor">0</Dimension></Flex>
            <Flex Name="Stretch" Type="Flex" id="4" Mode="Stretching" Axis="1,0,0"><Dimension Name="Distance">infmm</Dimension></Flex>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.features.len(), 4);
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Flex {
            axis: None,
            mode: FlexMode::Bending { .. },
        }
    ));
    for (index, form) in [FlexForm::Twisting, FlexForm::Tapering, FlexForm::Stretching]
        .into_iter()
        .enumerate()
    {
        assert!(matches!(
            decoded.ir.model.features[index + 1].definition,
            FeatureDefinition::Flex {
                axis: Some(_),
                mode: FlexMode::Unresolved {
                    form: Some(actual),
                    angle: None,
                    factor: None,
                    distance: None,
                },
            } if actual == form
        ));
    }

    for index in 0..4 {
        let mut detached = decoded.ir.clone();
        detached.model.features[index].native_ref = None;
        let error = SldprtCodec
            .write_preserved_with_source_fidelity(
                &detached,
                &decoded.source_fidelity,
                &mut Vec::new(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("unresolved flex construction"));
    }

    for (index, feature) in decoded.ir.model.features.iter_mut().enumerate() {
        feature.name = Some(format!("Renamed flex {}", index + 1));
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].properties["Axis"], "0,0,0");
    assert_eq!(native[1].parameters["Angle"], "NaNrad");
    assert_eq!(native[2].parameters["Factor"], "0");
    assert_eq!(native[3].parameters["Distance"], "infmm");
}

#[test]
fn decode_retains_nonfinite_feature_dimensions_as_native() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Extrude" Type="BossExtrude" id="1"><Dimension Name="Depth">NaNmm</Dimension></Extrusion>
            <Fillet Name="Fillet" Type="Fillet" id="2"><Dimension Name="Radius">infmm</Dimension></Fillet>
            <Shell Name="Shell" Type="Shell" id="3" Outward="false"><Dimension Name="Thickness">NaNmm</Dimension></Shell>
            <Dome Name="Dome" Type="Dome" id="4" Faces="face:1" Elliptical="false" Reverse="false"><Dimension Name="Height">infmm</Dimension></Dome>
            <Revolve Name="Revolve" Type="Revolve" id="5" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" Operation="Join"><Dimension Name="Angle">NaNrad</Dimension></Revolve>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.features.len(), 5);
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Native { .. }
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Constant),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir.model.features[2].definition,
        FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            thickness: None,
            outward: Some(false),
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[3].definition,
        FeatureDefinition::Dome {
            faces: cadmpeg_ir::features::FaceSelection::Native(_),
            height: None,
            elliptical: Some(false),
            reverse: Some(false),
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[4].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(_),
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
}

#[test]
fn decode_retains_nonpositive_feature_dimensions_as_native() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Extrude" Type="BossExtrude" id="1"><Dimension Name="Depth">0mm</Dimension></Extrusion>
            <Fillet Name="Fillet" Type="Fillet" id="2"><Dimension Name="Radius">-1mm</Dimension></Fillet>
            <Shell Name="Shell" Type="Shell" id="3" Outward="false"><Dimension Name="Thickness">0mm</Dimension></Shell>
            <Dome Name="Dome" Type="Dome" id="4" Faces="face:1" Elliptical="false" Reverse="false"><Dimension Name="Height">-2mm</Dimension></Dome>
            <Hole Name="Hole" Type="Hole" id="5"><Dimension Name="Diameter">0mm</Dimension><Dimension Name="Depth">5mm</Dimension></Hole>
            <Chamfer Name="Chamfer" Type="Chamfer" id="6"><Dimension Name="Distance">-3mm</Dimension></Chamfer>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.features.len(), 6);
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Native { .. }
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Constant),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir.model.features[2].definition,
        FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            thickness: None,
            outward: Some(false),
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[3].definition,
        FeatureDefinition::Dome {
            faces: cadmpeg_ir::features::FaceSelection::Native(_),
            height: None,
            elliptical: Some(false),
            reverse: Some(false),
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[4].definition,
        FeatureDefinition::Hole {
            kind: cadmpeg_ir::features::HoleKind::Simple,
            diameter: None,
            extent: Some(cadmpeg_ir::features::Termination::Blind {
                length: cadmpeg_ir::features::Length(5.0),
            }),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir.model.features[5].definition,
        FeatureDefinition::Chamfer {
            ref groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::ChamferForm::Distance),
            },
            ..
        }])
    ));
}

#[test]
fn decode_retains_invalid_feature_directions_and_angles_as_native() {
    use cadmpeg_ir::features::{FeatureDefinition, PatternForm, PatternKind};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="NativeSeed" id="1"/>
            <Pattern Name="Pattern" Type="LinearPattern" id="2" Seeds="1" Direction="0,0,0"><Dimension Name="Spacing">2mm</Dimension><Dimension Name="Count">2</Dimension></Pattern>
            <MoveFace Name="Move" Type="MoveFace" id="3" Faces="face:1" Mode="Translate" Direction="0,0,0"><Dimension Name="Distance">2mm</Dimension></MoveFace>
            <Chamfer Name="Chamfer" Type="Chamfer" id="4"><Dimension Name="Distance">2mm</Dimension><Dimension Name="Angle">180deg</Dimension></Chamfer>
            <Revolve Name="Revolve" Type="Revolve" id="5" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" Operation="Join"><Dimension Name="Angle">-1deg</Dimension></Revolve>
            <Sweep Name="Sweep" Type="Sweep" id="6" Profile="1" Path="1" Operation="Join"><Dimension Name="Scale">inf</Dimension></Sweep>
            <Rib Name="Rib" Type="Rib" id="7" Profile="1" Direction="0,0,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension></Rib>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.features.len(), 7);
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Linear),
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[4].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(_),
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[6].definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(_),
                direction: None,
                thickness: Some(cadmpeg_ir::features::Length(2.0)),
                side: Some(cadmpeg_ir::features::RibSide::OneSided),
                draft: cadmpeg_ir::features::RibDraft::None,
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
    assert!(matches!(
        &decoded.ir.model.features[3].definition,
        FeatureDefinition::Chamfer {
            ref groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::ChamferForm::DistanceAngle),
            },
            ..
        }])
    ));
    for index in [2, 5] {
        assert!(matches!(
            decoded.ir.model.features[index].definition,
            FeatureDefinition::Native { .. }
        ));
    }
}

#[test]
fn semantic_writer_preserves_native_feature_leaf_text() {
    use crate::records::FeatureContent;
    use cadmpeg_ir::features::FeatureSourceContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><MacroFeature Name="Custom" Type="Macro" id="70">prefix<Dimension Name="A">1</Dimension><Definition Name="Payload" Type="Definition" Language="expr">a &amp; b &lt; c</Definition>suffix<Dimension Name="B">2</Dimension></MacroFeature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    let definition = native.feature_histories[0]
        .features
        .iter()
        .find(|feature| feature.xml_tag == "Definition")
        .unwrap();
    assert_eq!(definition.text.as_deref(), Some("a & b < c"));
    assert_eq!(definition.properties["Language"], "expr");
    assert!(definition.tree_parent.is_some());
    let macro_feature = native.feature_histories[0]
        .features
        .iter()
        .find(|feature| feature.xml_tag == "MacroFeature")
        .unwrap();
    assert_eq!(
        macro_feature.content,
        [
            FeatureContent::Text("prefix".into()),
            FeatureContent::Dimension("A".into()),
            FeatureContent::Feature(definition.id.clone()),
            FeatureContent::Text("suffix".into()),
            FeatureContent::Dimension("B".into()),
        ]
    );
    let neutral_macro = decoded
        .ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.source_tag.as_deref() == Some("MacroFeature"))
        .unwrap();
    assert!(matches!(
        neutral_macro.source_content.as_slice(),
        [
            FeatureSourceContent::Text(prefix),
            FeatureSourceContent::Parameter(_),
            FeatureSourceContent::Feature(_),
            FeatureSourceContent::Text(suffix),
            FeatureSourceContent::Parameter(_),
        ] if prefix == "prefix" && suffix == "suffix"
    ));
    let FeatureSourceContent::Text(prefix) = &mut neutral_macro.source_content[0] else {
        unreachable!()
    };
    *prefix = "lead & more".into();
    let neutral_definition = decoded
        .ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.source_tag.as_deref() == Some("Definition"))
        .unwrap();
    assert_eq!(neutral_definition.source_text.as_deref(), Some("a & b < c"));
    neutral_definition.source_tag = Some("FormulaPayload".into());
    neutral_definition.source_text = Some("x > 1 & y < 2".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&regenerated.ir);
    let definition = native.feature_histories[0]
        .features
        .iter()
        .find(|feature| feature.xml_tag == "FormulaPayload")
        .unwrap();
    assert_eq!(definition.text.as_deref(), Some("x > 1 & y < 2"));
    assert_eq!(definition.properties["Language"], "expr");
    assert!(definition.tree_parent.is_some());
    let neutral_definition = regenerated
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("FormulaPayload"))
        .unwrap();
    assert_eq!(
        neutral_definition.source_text.as_deref(),
        Some("x > 1 & y < 2")
    );
    let macro_feature = native.feature_histories[0]
        .features
        .iter()
        .find(|feature| feature.xml_tag == "MacroFeature")
        .unwrap();
    assert_eq!(
        macro_feature.content,
        [
            FeatureContent::Text("lead & more".into()),
            FeatureContent::Dimension("A".into()),
            FeatureContent::Feature(definition.id.clone()),
            FeatureContent::Text("suffix".into()),
            FeatureContent::Dimension("B".into()),
        ]
    );
}

#[test]
fn semantic_writer_removes_deleted_history_records() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Keep"/><Configuration Name="Delete"/><Feature Name="Keep" Type="Custom" id="80"/><Feature Name="Delete" Type="Custom" id="81"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded
        .ir
        .model
        .features
        .retain(|feature| feature.name.as_deref() == Some("Keep"));
    decoded
        .ir
        .model
        .configurations
        .retain(|configuration| configuration.name == "Keep");

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir.model.features.len(), 1);
    assert_eq!(
        regenerated.ir.model.features[0].name.as_deref(),
        Some("Keep")
    );
    assert_eq!(regenerated.ir.model.configurations.len(), 1);
    assert_eq!(regenerated.ir.model.configurations[0].name, "Keep");
}

#[test]
fn semantic_writer_reorders_nested_history_records() {
    use crate::records::FeatureContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Folder Name="Parent" Type="Folder" id="90">prefix<Item Name="A" Type="Custom" id="91"/>middle<Item Name="B" Type="Custom" id="92"/></Folder></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    for feature in &mut decoded.ir.model.features {
        match feature.name.as_deref() {
            Some("A") => feature.ordinal = 2,
            Some("B") => feature.ordinal = 1,
            _ => {}
        }
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&regenerated.ir);
    let history = &native.feature_histories[0];
    let parent = history
        .features
        .iter()
        .find(|feature| feature.name == "Parent")
        .unwrap();
    let child_names = parent
        .content
        .iter()
        .filter_map(|item| match item {
            FeatureContent::Feature(id) => history
                .features
                .iter()
                .find(|feature| &feature.id == id)
                .map(|feature| feature.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(child_names, ["B", "A"]);
    assert!(matches!(
        parent.content[0],
        FeatureContent::Text(ref text) if text == "prefix"
    ));
    assert!(matches!(
        parent.content[2],
        FeatureContent::Text(ref text) if text == "middle"
    ));
}

#[test]
fn encoder_writes_source_less_datum_features() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let definitions = [
        FeatureDefinition::DatumPlane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        FeatureDefinition::DatumAxis {
            origin: Point3::new(4.0, 5.0, 6.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        },
        FeatureDefinition::DatumPoint {
            position: Point3::new(7.0, 8.0, 9.0),
        },
    ];
    for (ordinal, definition) in definitions.into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("synthetic:test:feature#datum-{ordinal}")),
            ordinal: ordinal as u64,
            name: Some(format!("Datum {ordinal}")),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::DatumPlane { .. }
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::DatumAxis { .. }
    ));
    assert!(matches!(
        decoded.ir.model.features[2].definition,
        FeatureDefinition::DatumPoint { .. }
    ));
}

#[test]
fn encoder_writes_source_less_neutral_configurations() {
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("sldprt:model:configuration#generated:z".into()),
        ordinal: 0,
        active: true,
        source_index: None,
        name: "Metric".into(),
        material: Some("Steel".into()),
        properties: BTreeMap::from([("Finish".into(), "Ground".into())]),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(vec![ir.model.bodies[0].id.clone()]),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("sldprt:model:configuration#generated:a".into()),
        ordinal: 1,
        active: false,
        source_index: None,
        name: "Empty".into(),
        material: None,
        properties: BTreeMap::new(),
        bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        suppressed_features: Vec::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    ir.finalize();

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let scan = container::scan_bytes(&encoded);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-0-Partition") }));
    assert!(!scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-1-Partition") }));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded
            .ir
            .model
            .configurations
            .iter()
            .map(|configuration| configuration.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Metric", "Empty"]
    );
    assert_eq!(
        sldprt_native(&decoded.ir).feature_histories[0]
            .configurations
            .iter()
            .map(|configuration| configuration.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    let configuration = &decoded.ir.model.configurations[0];
    assert_eq!(configuration.name, "Metric");
    assert_eq!(configuration.material.as_deref(), Some("Steel"));
    assert_eq!(configuration.properties["Finish"], "Ground");
    assert!(configuration.active);
    assert_eq!(
        configuration.bodies,
        decoded
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
    assert!(decoded.ir.model.configurations[1].bodies.is_empty());

    let mut inactive = decoded.ir;
    inactive
        .model
        .configurations
        .iter_mut()
        .for_each(|configuration| configuration.active = false);
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&inactive, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("requires exactly one active configuration"));
}

#[test]
fn semantic_writer_round_trips_active_configuration() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Manufacturing &amp; QA"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swName="Part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir.model.configurations[0].active);
    assert!(!decoded.ir.model.configurations[1].active);

    decoded.ir.model.configurations[0].active = false;
    decoded.ir.model.configurations[1].active = true;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(!regenerated.ir.model.configurations[0].active);
    assert!(regenerated.ir.model.configurations[1].active);
    assert_eq!(
        regenerated.ir.source.as_ref().unwrap().attributes["sw_configuration_name"],
        "Manufacturing & QA"
    );
}

#[test]
fn decode_preserves_unresolved_active_configuration() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Manufacturing"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swName="Part" swConfigurationName="Missing"/></swSolidWorks>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded
        .ir
        .model
        .configurations
        .iter()
        .all(|configuration| !configuration.active));
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.message
            == "active configuration identity is unresolved; 0 of 2 configuration records are active."
    }));
    assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn decode_reports_partition_inferred_configuration() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir.model.configurations.len(), 1);
    assert!(decoded.ir.model.configurations[0].native_ref.is_none());
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.message
            == "1 configuration state(s) are inferred from geometry partitions without native configuration definitions."
    }));
}

#[test]
fn encoder_partitions_source_less_bodies_by_configuration() {
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::tessellation::Tessellation;
    use cadmpeg_ir::transform::Transform;
    use std::collections::BTreeMap;

    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let mut ir = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap()
        .ir;
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.bodies.iter_mut().for_each(|body| body.name = None);
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    for (index, body) in ir.model.bodies.iter_mut().enumerate() {
        body.transform = Some(Transform {
            rows: [
                [1.0, 0.0, 0.0, (index as f64 + 1.0) * 10.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        });
    }
    ir.model.tessellations = body_ids
        .iter()
        .enumerate()
        .map(|(index, body)| Tessellation {
            id: format!("synthetic:test:tessellation#{index}"),
            body: Some(body.clone()),
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: None,
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            strip_lengths: vec![3],
            normals: vec![Vector3::new(0.0, 0.0, 1.0); 3],
            channels: Vec::new(),
        })
        .collect();
    ir.model.configurations = body_ids
        .iter()
        .enumerate()
        .map(|(index, body)| DesignConfiguration {
            id: ConfigurationId(format!("synthetic:test:configuration#config-{index}")),
            ordinal: index as u32,
            active: false,
            source_index: None,
            name: format!("Config {index}"),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(vec![body.clone()]),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        })
        .collect();
    ir.model.configurations[1].active = true;

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let scan = container::scan_bytes(&encoded);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-0-Partition") }));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-1-Partition") }));
    assert_eq!(container::active_configuration_index(&scan), Some(1));
    assert_eq!(
        container::select_active_parasolid(&scan)
            .unwrap()
            .0
            .section
            .as_deref(),
        Some("Contents/Config-1-Partition")
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.bodies.len(), 2);
    assert_eq!(decoded.ir.model.configurations[0].bodies.len(), 1);
    assert_eq!(decoded.ir.model.configurations[1].bodies.len(), 1);
    assert!(decoded.ir.model.configurations[1].active);
    assert_ne!(
        decoded.ir.model.configurations[0].bodies,
        decoded.ir.model.configurations[1].bodies
    );
    let mesh_x = decoded
        .ir
        .model
        .tessellations
        .iter()
        .flat_map(|mesh| mesh.vertices.iter().map(|point| point.x))
        .collect::<Vec<_>>();
    assert!(mesh_x.iter().any(|value| (*value - 10.0).abs() < 1.0e-6));
    assert!(mesh_x.iter().any(|value| (*value - 20.0).abs() < 1.0e-6));
}

#[test]
fn decode_assigns_selected_partition_bodies_to_configuration() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.configurations.len(), 1);
    assert!(decoded.ir.model.configurations[0].active);
    assert_eq!(
        decoded.ir.model.configurations[0].bodies,
        decoded
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir.model.configurations[0].bodies,
        round_trip
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn decode_synthesizes_sparse_partition_configuration() {
    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-3-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    assert_eq!(
        container::scan_bytes(&source).blocks[0].section.as_deref(),
        Some("Contents/Config-3-Partition")
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.configurations.len(), 1);
    let configuration = &decoded.ir.model.configurations[0];
    assert_eq!(configuration.ordinal, 0);
    assert_eq!(configuration.source_index, Some(3));
    assert!(configuration.active);
    assert_eq!(configuration.name, "Config-3");
    assert_eq!(
        configuration.bodies,
        decoded
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );

    let mut edited = decoded.ir;
    edited.model.points[0].position.x += 1.0;
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.as_deref() == Some("Contents/Config-3-Partition")));
    assert!(!scan
        .blocks
        .iter()
        .any(|block| block.section.as_deref() == Some("Contents/Config-0-Partition")));
}

#[test]
fn semantic_writer_remaps_partition_without_remapping_resolved_features() {
    let mut source = outer_header();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks><swModel swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    source.extend(make_block(
        0x20,
        "Contents/Config-3-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-3-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.configurations[0].source_index, Some(3));
    assert!(decoded.ir.model.configurations[0].active);

    decoded.ir.model.configurations[0].source_index = Some(5);
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-5-Partition") }));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-3-ResolvedFeatures") }));
    assert_eq!(container::active_configuration_index(&scan), Some(5));
    assert_eq!(
        container::select_active_parasolid(&scan)
            .unwrap()
            .0
            .section
            .as_deref(),
        Some("Contents/Config-5-Partition")
    );
    let stale = scan
        .blocks
        .iter()
        .filter_map(|block| block.section.as_deref())
        .filter(|section| {
            *section == "Contents/Config-3-Partition"
                || *section == "Contents/Config-5-ResolvedFeatures"
        })
        .collect::<Vec<_>>();
    assert!(stale.is_empty(), "stale sections: {stale:?}");
    assert!(!scan.blocks.iter().any(|block| {
        block.section.as_deref().is_some_and(|section| {
            section == "Contents/Config-3-Partition"
                || section == "Contents/Config-5-ResolvedFeatures"
        })
    }));
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir.model.configurations[0].source_index, Some(5));
}

#[test]
fn semantic_writer_allocates_partition_index_without_remapping_resolved_features() {
    let mut source = outer_header();
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks><swModel swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    source.extend(make_block(
        0x20,
        "Contents/Config-3-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-3-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded.ir.model.configurations[0].source_index = None;

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.as_deref() == Some("Contents/Config-0-Partition")));
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-3-ResolvedFeatures") }));
    assert!(!scan.blocks.iter().any(|block| {
        matches!(
            block.section.as_deref(),
            Some(
                "Contents/ResolvedFeatures"
                    | "Contents/Config-3-Partition"
                    | "Contents/Config-0-ResolvedFeatures"
            )
        )
    }));
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir.model.configurations[0].source_index, Some(0));
}

#[test]
fn semantic_writer_rejects_duplicate_configuration_source_indices() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut duplicate = decoded.ir.model.configurations[0].clone();
    duplicate.id.0.push_str("-duplicate");
    duplicate.ordinal += 1;
    duplicate.name.push_str(" Duplicate");
    duplicate.native_ref = None;
    decoded.ir.model.configurations.push(duplicate);

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("repeats configuration source index"),
        "{error}"
    );
}

#[test]
fn semantic_writer_rejects_empty_and_duplicate_configuration_names() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.configurations[0].name.clear();
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("empty name"), "{error}");

    decoded.ir.model.configurations[0].name = "Default".into();
    let mut duplicate = decoded.ir.model.configurations[0].clone();
    duplicate.id.0.push_str("-duplicate");
    duplicate.ordinal += 1;
    duplicate.source_index = None;
    duplicate.native_ref = None;
    duplicate.active = false;
    decoded.ir.model.configurations.push(duplicate);
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("repeats configuration name"),
        "{error}"
    );
}

#[test]
fn configuration_source_index_allocation_rejects_exhaustion() {
    let mut used = std::collections::HashSet::from([u32::MAX]);
    let mut next = u32::MAX;
    let error = crate::writer::reserve_configuration_index(&mut used, &mut next).unwrap_err();
    assert!(error.to_string().contains("index space is exhausted"));
}

#[test]
fn encoder_writes_source_less_neutral_parameters() {
    use cadmpeg_ir::features::{
        DesignParameter, Feature, FeatureDefinition, FeatureId, ParameterId,
    };
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let feature_id = FeatureId("sldprt:model:feature#generated:equation".into());
    ir.model.features.push(Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: Some("Equation".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: "EquationDriven".into(),
            parameters: BTreeMap::from([("Pitch".into(), "D1@Sketch1 * 2".into())]),
            properties: BTreeMap::from([("EquationSet".into(), "Global".into())]),
        },
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("sldprt:model:parameter#generated:equation:0".into()),
        owner: Some(feature_id),
        ordinal: 0,
        name: "Pitch".into(),
        expression: "D1@Sketch1 * 2".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.parameters.len(), 1);
    assert_eq!(decoded.ir.model.parameters[0].expression, "D1@Sketch1 * 2");
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Native { properties, .. }
            if properties.get("EquationSet").map(String::as_str) == Some("Global")
    ));
}

#[test]
fn encoder_bakes_rigid_body_transform() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::transform::Transform;

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let original_point = ir.model.points[0].position;
    let original_normal = ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match surface.geometry {
            SurfaceGeometry::Plane { normal, .. } if normal.x == 1.0 => Some(normal),
            _ => None,
        })
        .unwrap();
    ir.model.bodies[0].transform = Some(Transform {
        rows: [
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    let expected_point = Point3::new(
        -original_point.y + 10.0,
        original_point.x + 20.0,
        original_point.z + 30.0,
    );
    let expected_normal = Vector3::new(-original_normal.y, original_normal.x, original_normal.z);

    let mut encoded = Vec::new();
    SldprtCodec.encode(&ir, &mut encoded).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.ir.model.points.iter().any(|point| {
        (point.position.x - expected_point.x).abs() < 1e-9
            && (point.position.y - expected_point.y).abs() < 1e-9
            && (point.position.z - expected_point.z).abs() < 1e-9
    }));
    assert!(decoded.ir.model.surfaces.iter().any(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Plane { normal, .. } if normal == expected_normal)
    }));
    assert!(decoded
        .ir
        .model
        .bodies
        .iter()
        .all(|body| body.transform.is_none()));
}

#[test]
fn decode_encode_is_equivariant_under_rigid_motion() {
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::transform::Transform;

    let motions = [
        (
            [
                [0.0, -1.0, 0.0, 10.0],
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 0.0, 1.0, 30.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            (|p: Point3| Point3::new(-p.y + 10.0, p.x + 20.0, p.z + 30.0)) as fn(Point3) -> Point3,
        ),
        (
            [
                [1.0, 0.0, 0.0, -5.0],
                [0.0, 0.0, -1.0, 7.0],
                [0.0, 1.0, 0.0, 3.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            |p: Point3| Point3::new(p.x - 5.0, -p.z + 7.0, p.y + 3.0),
        ),
    ];

    // The `.sldprt` semantic writer refuses a body or face name without a
    // material, so strip the labels the round trip does not exercise here.
    let prepare = |ir: &mut cadmpeg_ir::document::CadIr| {
        ir.model.bodies[0].name = None;
        ir.model.faces.iter_mut().for_each(|face| face.name = None);
        ir.model
            .edges
            .iter_mut()
            .for_each(|edge| edge.param_range = None);
    };

    let mut base = cadmpeg_ir::examples::unit_cube();
    prepare(&mut base);
    base.model.bodies[0].transform = None;
    let mut base_bytes = Vec::new();
    SldprtCodec.encode(&base, &mut base_bytes).unwrap();
    let reference = SldprtCodec
        .decode(&mut Cursor::new(base_bytes), &DecodeOptions::default())
        .unwrap();
    let reference_points: Vec<Point3> = reference
        .ir
        .model
        .points
        .iter()
        .map(|point| point.position)
        .collect();

    for (rows, apply) in motions {
        let mut moved = cadmpeg_ir::examples::unit_cube();
        prepare(&mut moved);
        moved.model.bodies[0].transform = Some(Transform { rows });
        let mut bytes = Vec::new();
        SldprtCodec.encode(&moved, &mut bytes).unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();

        for reference_point in &reference_points {
            let expected = apply(*reference_point);
            assert!(
                decoded.ir.model.points.iter().any(|point| {
                    (point.position.x - expected.x).abs() < 1e-9
                        && (point.position.y - expected.y).abs() < 1e-9
                        && (point.position.z - expected.z).abs() < 1e-9
                }),
                "rigid motion not preserved for point {reference_point:?}"
            );
        }
        assert!(decoded
            .ir
            .model
            .bodies
            .iter()
            .all(|body| body.transform.is_none()));
    }
}

#[test]
fn decode_encode_decode_reaches_fixpoint() {
    let fixture = sldprt_with_body_and_history(&triangle_body());

    let first = SldprtCodec
        .decode(&mut Cursor::new(fixture), &DecodeOptions::default())
        .expect("first decode");
    assert!(first.report.geometry_transferred);

    let mut reencoded = Vec::new();
    SldprtCodec
        .encode_with_source_fidelity(&first.ir, Some(&first.source_fidelity), &mut reencoded)
        .expect("re-encode");

    let second = SldprtCodec
        .decode(&mut Cursor::new(reencoded), &DecodeOptions::default())
        .expect("second decode");

    assert_eq!(
        first.ir.model.points, second.ir.model.points,
        "points diverged at the fixpoint"
    );
    assert_eq!(
        first.ir.model.surfaces, second.ir.model.surfaces,
        "surfaces diverged at the fixpoint"
    );
    assert_eq!(
        first.ir.model.faces, second.ir.model.faces,
        "faces diverged at the fixpoint"
    );
    assert_eq!(
        first.ir.model.edges, second.ir.model.edges,
        "edges diverged at the fixpoint"
    );
    assert_eq!(
        first.ir.model.coedges, second.ir.model.coedges,
        "coedges diverged at the fixpoint"
    );
    assert_eq!(
        first.report.geometry_transferred, second.report.geometry_transferred,
        "geometry-transferred flag diverged at the fixpoint"
    );
}

#[test]
fn semantic_writer_regenerates_modified_planar_brep() {
    let source = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(source);
    let mut result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    result.ir.model.points[0].position.x += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&result.ir, &result.source_fidelity, &mut encoded)
        .unwrap();
    let mut regenerated = Cursor::new(encoded);
    let decoded = SldprtCodec
        .decode(&mut regenerated, &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir
        .model
        .points
        .iter()
        .any(|point| point.position.x == 1.0));
}

#[test]
fn semantic_writer_uses_schema_specific_face_families() {
    let mut solid = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    solid.ir.model.points[0].position.z += 1.0;
    let mut solid_bytes = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&solid.ir, &solid.source_fidelity, &mut solid_bytes)
        .unwrap();
    let solid_scan = container::scan_bytes(&solid_bytes);
    let solid_payload = &solid_scan.blocks[0].payload;
    assert!(count_entity51_family(solid_payload, 2, 0x0013) >= 1);
    assert!(count_entity51_family(solid_payload, 1, 0x0015) >= 1);

    let mut sheet_body = Vec::new();
    sheet_body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
    sheet_body.extend(entity51(1, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));
    sheet_body.extend(owned_triangle(0, 701, 0.0));
    let mut sheet = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&sheet_body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    sheet.ir.model.points[0].position.z += 1.0;
    let mut sheet_bytes = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&sheet.ir, &sheet.source_fidelity, &mut sheet_bytes)
        .unwrap();
    let sheet_scan = container::scan_bytes(&sheet_bytes);
    let sheet_payload = &sheet_scan.blocks[0].payload;
    assert!(count_entity51_family(sheet_payload, 2, 0x0015) >= 1);
    assert!(count_entity51_family(sheet_payload, 1, 0x001f) >= 1);
}

#[test]
fn semantic_writer_preserves_outer_header() {
    let mut source = sldprt_with_body(&triangle_body());
    source[..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    source[4..8].copy_from_slice(&7u32.to_be_bytes());
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();

    assert_eq!(
        u32::from_le_bytes(encoded[..4].try_into().unwrap()),
        0x1234_5678
    );
    assert_eq!(u32::from_be_bytes(encoded[4..8].try_into().unwrap()), 7);
}

/// Translate every model-space carrier along x so a forced modification stays
/// geometrically consistent: vertices remain on their edge curves and surfaces.
fn translate_model_x(ir: &mut cadmpeg_ir::document::CadIr, dx: f64) {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    fn translate_curve_x(curve: &mut CurveGeometry, dx: f64) {
        match curve {
            CurveGeometry::Line { origin, .. } => origin.x += dx,
            CurveGeometry::Circle { center, .. }
            | CurveGeometry::Ellipse { center, .. }
            | CurveGeometry::Hyperbola { center, .. } => center.x += dx,
            CurveGeometry::Parabola { vertex, .. } => vertex.x += dx,
            CurveGeometry::Degenerate { point } => point.x += dx,
            CurveGeometry::Nurbs(nurbs) => {
                for pole in &mut nurbs.control_points {
                    pole.x += dx;
                }
            }
            CurveGeometry::Polyline { points, .. } => {
                for point in points {
                    point.x += dx;
                }
            }
            CurveGeometry::Transformed { transform, .. } => transform.rows[0][3] += dx,
            CurveGeometry::Composite { .. } => {}
            CurveGeometry::Procedural { .. } => {}
            CurveGeometry::Unknown { .. } => {}
        }
    }
    for point in &mut ir.model.points {
        point.position.x += dx;
    }
    for curve in &mut ir.model.curves {
        translate_curve_x(&mut curve.geometry, dx);
    }
    for surface in &mut ir.model.surfaces {
        match &mut surface.geometry {
            SurfaceGeometry::Plane { origin, .. }
            | SurfaceGeometry::Cylinder { origin, .. }
            | SurfaceGeometry::Cone { origin, .. } => origin.x += dx,
            SurfaceGeometry::Sphere { center, .. } | SurfaceGeometry::Torus { center, .. } => {
                center.x += dx;
            }
            SurfaceGeometry::Nurbs(nurbs) => {
                for pole in &mut nurbs.control_points {
                    pole.x += dx;
                }
            }
            SurfaceGeometry::Polygonal { vertices, .. } => {
                for vertex in vertices {
                    vertex.x += dx;
                }
            }
            SurfaceGeometry::Transformed { transform, .. } => transform.rows[0][3] += dx,
            SurfaceGeometry::Procedural { .. } => {}
            SurfaceGeometry::Unknown { .. } => {}
        }
    }
}

#[test]
fn semantic_writer_regenerates_modified_analytic_breps() {
    for body in [closed_cylinder_body(), sphere_patch_body()] {
        let source = sldprt_with_body(&body);
        let mut cur = Cursor::new(source);
        let mut result = SldprtCodec
            .decode(&mut cur, &DecodeOptions::default())
            .unwrap();
        translate_model_x(&mut result.ir, 1.0);

        let mut encoded = Vec::new();
        SldprtCodec
            .write_preserved_with_source_fidelity(&result.ir, &result.source_fidelity, &mut encoded)
            .unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();

        assert_eq!(decoded.ir.model.faces.len(), result.ir.model.faces.len());
        assert_eq!(decoded.ir.model.curves.len(), result.ir.model.curves.len());
        assert_eq!(
            decoded
                .ir
                .model
                .surfaces
                .iter()
                .map(|surface| &surface.geometry)
                .collect::<Vec<_>>(),
            result
                .ir
                .model
                .surfaces
                .iter()
                .map(|surface| &surface.geometry)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn decode_builds_valid_topology_and_plane() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    let f = sldprt_with_body(&triangle_body());
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 3);
    assert_eq!(result.ir.model.edges.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.surfaces.len(), 1);

    match &result.ir.model.surfaces[0].geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            assert_eq!(*origin, Point3::new(0.0, 0.0, 0.0));
            assert_eq!(normal.z, 1.0);
            assert_eq!(u_axis.x, 1.0);
        }
        other => panic!("expected plane, got {other:?}"),
    }

    let xs: Vec<f64> = result
        .ir
        .model
        .points
        .iter()
        .map(|p| p.position.x)
        .collect();
    assert!(xs.contains(&1000.0));

    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
    assert_eq!(result.ir.model.loops[0].coedges.len(), 3);
    // Edges carry no analytic curve (their carriers were null), which is legal.
    assert!(result.ir.model.edges.iter().all(|e| e.curve.is_none()));
}

fn strict_options() -> DecodeOptions {
    use cadmpeg_ir::decode::{DecodeMode, DecodePolicy};
    DecodeOptions {
        container_only: false,
        policy: DecodePolicy {
            mode: DecodeMode::Strict,
            ..DecodePolicy::desktop()
        },
    }
}

#[test]
fn strict_accepts_operator_requested_container_only() {
    let fixture = synthetic_sldprt();
    let mut options = strict_options();
    options.container_only = true;
    SldprtCodec
        .decode(&mut Cursor::new(fixture), &options)
        .expect("strict container-only decode is accepted");
}

#[test]
fn strict_rejects_unrepresentable_geometry_while_salvage_records_loss_codes() {
    use cadmpeg_ir::report::{LossCode, StrictConsequence};

    let fixture = synthetic_sldprt();

    let salvaged = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("salvage decode keeps the partial result");
    assert!(!salvaged.report.geometry_transferred);
    assert!(salvaged
        .report
        .losses
        .iter()
        .any(|note| note.code == LossCode::GeometryNotTransferred));
    assert!(salvaged
        .report
        .losses
        .iter()
        .any(|note| note.code == LossCode::TopologyNotTransferred));
    assert!(salvaged
        .report
        .losses
        .iter()
        .any(|note| note.code.strict_consequence() == StrictConsequence::Reject));

    let strict = SldprtCodec.decode(&mut Cursor::new(fixture), &strict_options());
    match strict {
        Err(cadmpeg_ir::CodecError::Malformed(message)) => {
            assert!(
                message.contains("strict mode rejects geometry_not_transferred"),
                "unexpected message: {message}"
            );
        }
        other => panic!("strict decode must reject unrepresentable geometry, got {other:?}"),
    }
}

#[test]
fn strict_accepts_tolerable_gauge_substitution_geometry() {
    use cadmpeg_ir::report::{LossCode, StrictConsequence};

    let fixture = sldprt_with_body_and_history(&triangle_body());
    let strict = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect("strict decode accepts a tolerable-loss geometry result");
    assert!(strict.report.geometry_transferred);
    assert!(strict
        .report
        .losses
        .iter()
        .all(|note| note.code.strict_consequence() == StrictConsequence::Tolerate));
    assert!(strict
        .report
        .losses
        .iter()
        .any(|note| note.code == LossCode::TopologyGaugeSubstituted));
}

#[test]
fn decode_does_not_report_derived_pcurves_as_stored_geometry_loss() {
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("curve-on-surface")));
}

#[test]
fn decode_merges_partition_and_deltas_records() {
    let body = triangle_body();
    let split = body.len() / 2;
    let f = sldprt_with_partition_and_deltas(&body[..split], &body[split..]);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.points.len(), 3);
}

#[test]
fn decode_deduplicates_partition_and_deltas_face_bindings() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut partition = Vec::new();
    partition.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    partition.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    partition.extend(owned_triangle(0, 700, 0.0));
    let mut deltas = Vec::new();
    deltas.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    deltas.extend(entity53_color(900, [0.25, 0.5, 0.75]));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.appearances.len(), 1);
    assert_eq!(
        result
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses).is_ok());
}

#[test]
fn decode_merges_colliding_configuration_sites_with_disjoint_identities() {
    let mut cur = Cursor::new(sldprt_with_colliding_sites());
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.faces.len(), 2);
    assert!(result
        .ir
        .model
        .points
        .iter()
        .any(|point| point.position.x == 0.0));
    assert!(result
        .ir
        .model
        .points
        .iter()
        .any(|point| point.position.x == 10_000.0));
    let ids: std::collections::HashSet<_> = result
        .ir
        .model
        .points
        .iter()
        .map(|point| &point.id)
        .collect();
    assert_eq!(ids.len(), result.ir.model.points.len());
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn deltas_full_record_overrides_partition_record() {
    let partition = triangle_body();
    let deltas = world_point(60, [2.0, 0.0, 0.0]);
    let f = sldprt_with_partition_and_deltas(&partition, &deltas);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let point = result
        .ir
        .model
        .points
        .iter()
        .find(|point| point.id.0.ends_with("#60"))
        .expect("overridden point");

    assert_eq!(point.position.x, 2000.0);
}

#[test]
fn deltas_cannot_add_a_superseded_face_to_partition_membership() {
    let partition = triangle_body();
    let deltas = owned_triangle(200, 900, 10.0);
    let mut cur = Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas));

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(result
        .ir
        .model
        .points
        .iter()
        .all(|point| point.position.x != 10_000.0));
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn duplicate_face_uses_emit_one_face() {
    let mut body = triangle_body();
    let first_bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[first_bridge + 8..first_bridge + 10].copy_from_slice(&700u16.to_be_bytes());
    body.extend(bridge_owned(11, 20, 100, 700));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
}

#[test]
fn sheet_body_faces_are_retained_and_classified() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 700, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(result.ir.model.faces.len(), 2);
    assert_eq!(
        result
            .ir
            .model
            .bodies
            .iter()
            .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid)
            .count(),
        1
    );
    assert_eq!(
        result
            .ir
            .model
            .bodies
            .iter()
            .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Sheet)
            .count(),
        1
    );
}

#[test]
fn schema_33103_disc1d_flo2_is_not_a_sheet_region() {
    let mut body = Vec::new();
    body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
    body.extend(entity51(2, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 701, 0.0));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(
        decoded.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
}

#[test]
fn semantic_writer_preserves_sheet_body_classification() {
    let mut body = Vec::new();
    body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
    body.extend(entity51(1, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 701, 0.0));
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;
    let validation = cadmpeg_ir::validate::validate(&decoded.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir.model.bodies.len(), 1);
    assert_eq!(
        regenerated.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(regenerated.ir.model.faces.len(), 1);
    assert_eq!(
        regenerated
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("parasolid_schema"))
            .map(String::as_str),
        Some("SCH_SW_32001_11000")
    );
}

#[test]
fn semantic_writer_rejects_invalid_ir_without_panicking() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.faces[0].surface = cadmpeg_ir::ids::SurfaceId("missing".into());
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(error, cadmpeg_ir::codec::CodecError::Malformed(_)));
}

#[test]
fn semantic_writer_rejects_unrepresented_typed_fields() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.edges[0].param_range = Some([0.0, 1.0]);
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(_)
    ));
}

#[test]
fn semantic_writer_rejects_subds() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.subds.push(cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("test:sldprt:subd#0".into()),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    });

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(message)
            if message.contains("does not support SubD surfaces")
    ));
}

#[test]
fn semantic_writer_rejects_unsupported_conic_curves() {
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let major_direction = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    for geometry in [
        cadmpeg_ir::geometry::CurveGeometry::Parabola {
            vertex: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis,
            major_direction,
            focal_distance: 1.0,
        },
        cadmpeg_ir::geometry::CurveGeometry::Hyperbola {
            center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis,
            major_direction,
            major_radius: 2.0,
            minor_radius: 1.0,
        },
    ] {
        assert!(matches!(
            crate::writer::curve_values(&geometry, 0.001),
            Err(cadmpeg_ir::codec::CodecError::NotImplemented(_))
        ));
    }
}

#[test]
fn semantic_writer_rejects_noncanonical_ellipse_radius_order() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&closed_cylinder_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.curves[0].geometry = cadmpeg_ir::geometry::CurveGeometry::Ellipse {
        center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        major_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        major_radius: 1.0,
        minor_radius: 2.0,
    };

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::Malformed(message)
            if message.contains("ellipse major radius is smaller than its minor radius")
    ));
}

#[test]
fn semantic_writer_rejects_nonfinite_analytic_carriers() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&closed_cylinder_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Circle { center, .. } =
        &mut decoded.ir.model.curves[0].geometry
    else {
        panic!("closed cylinder edge must use a circle carrier");
    };
    center.x = f64::INFINITY;

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::Malformed(message)
            if message.contains("circle center is not finite")
    ));
}

#[test]
fn semantic_writer_rejects_unrepresentable_analytic_surface_parameterizations() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let origin = cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0);
    let axis = cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0);
    let reference = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    let cases = [
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction: reference,
                radius: 2.0,
                ratio: 0.5,
                half_angle: std::f64::consts::FRAC_PI_4,
            },
            "elliptical cone ratio 0.5",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction: reference,
                radius: 2.0,
                ratio: 1.0,
                half_angle: -std::f64::consts::FRAC_PI_4,
            },
            "cone half-angle -0.7853981633974483",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
                center: origin,
                axis,
                ref_direction: reference,
                radius: -2.0,
            },
            "signed sphere radius -2",
        ),
        (
            cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                center: origin,
                axis,
                ref_direction: reference,
                major_radius: 2.0,
                minor_radius: -0.5,
            },
            "torus radii (2, -0.5)",
        ),
    ];

    for (geometry, expected) in cases {
        let mut ir = decoded.ir.clone();
        let surface_id = ir.model.surfaces[0].id.0.clone();
        ir.model.surfaces[0].geometry = geometry;

        let error = SldprtCodec
            .write_preserved_with_source_fidelity(&ir, &decoded.source_fidelity, &mut Vec::new())
            .unwrap_err();

        assert!(matches!(
            error,
            cadmpeg_ir::codec::CodecError::NotImplemented(message)
                if message.contains(&surface_id) && message.contains(expected)
        ));
    }
}

#[test]
fn semantic_writer_converts_millimetres_to_native_metres() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.x = 50.8;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated
        .ir
        .model
        .points
        .iter()
        .any(|point| (point.position.x - 50.8).abs() < 1e-5));
}

#[test]
fn closed_cylinder_gets_derived_seam() {
    use cadmpeg_ir::geometry::CurveGeometry;
    let f = sldprt_with_body(&closed_cylinder_body());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.faces[0].loops.len(), 1);
    assert_eq!(result.ir.model.loops[0].coedges.len(), 4);
    assert_eq!(result.ir.model.pcurves.len(), 4);
    assert!(result
        .ir
        .model
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
    assert_eq!(result.ir.model.edges.len(), 3);
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Line { .. })));
}

#[test]
fn closed_cylinder_anchors_sentinel_vertices_to_the_surface_branch() {
    let mut body = closed_cylinder_body();
    for coedge_attr in [30u16, 31] {
        let offset = body
            .windows(4)
            .position(|window| {
                window[0..2] == [0x00, 0x11] && window[2..4] == coedge_attr.to_be_bytes()
            })
            .expect("coedge");
        body[offset + 12..offset + 14].copy_from_slice(&1u16.to_be_bytes());
    }

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let seam = decoded
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0.contains("#seam:"))
        .expect("derived seam");
    let positions = [&seam.start, &seam.end].map(|vertex_id| {
        let vertex = decoded
            .ir
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == *vertex_id)
            .unwrap();
        decoded
            .ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap()
            .position
    });
    assert_eq!(
        positions[0],
        cadmpeg_ir::math::Point3::new(-1000.0, 0.0, 0.0)
    );
    assert_eq!(
        positions[1],
        cadmpeg_ir::math::Point3::new(-1000.0, 0.0, 1000.0)
    );
}

#[test]
fn closed_circle_edge_gets_a_derived_seam_vertex() {
    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [1.0, 2.0, 0.0], [0.0, 0.0, 1.0], 0.5));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 1, 0, 40, false));
    body.extend(edge_use(40, 200));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert_eq!(decoded.ir.model.loops[0].coedges.len(), 1);
    let edge = &decoded.ir.model.edges[0];
    assert_eq!(edge.start, edge.end);
    let vertex = decoded
        .ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.start)
        .unwrap();
    let point = decoded
        .ir
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap();
    assert_eq!(
        [point.position.x, point.position.y, point.position.z],
        [1500.0, 2000.0, 0.0]
    );
    assert!(matches!(
        decoded.ir.model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Circle {
            center,
            radius: 500.0,
            y_axis: cadmpeg_ir::math::Point2 { u: 0.0, v: 1.0 },
            ..
        } if center == cadmpeg_ir::math::Point2::new(1000.0, 2000.0)
    ));
    assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn oblique_cylinder_section_gets_an_exact_polar_harmonic_pcurve() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut body = Vec::new();
    body.extend(cylinder_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    body.extend(ellipse_carrier(
        200,
        [0.0, 0.0, 0.0],
        [-s, 0.0, s],
        [s, 0.0, s],
        std::f64::consts::SQRT_2,
        1.0,
    ));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [1.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir.model.pcurves.len(), 1);
    assert!(matches!(
        decoded.ir.model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin: 0.0,
            axial_sin: 0.0,
            ..
        } if radial_center == cadmpeg_ir::math::Point2::new(0.0, 0.0)
            && (radial_cos.u - 1000.0).abs() < 1e-9
            && radial_cos.v.abs() < 1e-9
            && radial_sin.u.abs() < 1e-9
            && (radial_sin.v - 1000.0).abs() < 1e-9
    ));
    assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn coaxial_cone_circle_preserves_parameter_direction() {
    let mut body = Vec::new();
    body.extend(cone_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        1.0,
        std::f64::consts::FRAC_PI_4,
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], 2.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [2.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } =
        decoded.ir.model.pcurves[0].geometry
    else {
        panic!("expected line pcurve");
    };
    assert!(origin.u.abs() < 1e-12);
    assert!((origin.v - 1000.0).abs() < 1e-9);
    assert_eq!(direction, cadmpeg_ir::math::Point2::new(-1.0, 0.0));
    assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn coaxial_torus_circle_gets_constant_minor_angle_pcurve() {
    let mut body = Vec::new();
    body.extend(torus_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        2.0,
        1.0,
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 2.0));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 50, 0, 40, false));
    body.extend(edge_use(40, 200));
    body.extend(vertex_use(50, 60));
    body.extend(world_point(60, [2.0, 0.0, 1.0]));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction } =
        decoded.ir.model.pcurves[0].geometry
    else {
        panic!("expected line pcurve");
    };
    assert!(origin.u.abs() < 1e-12);
    assert!((origin.v - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(direction, cadmpeg_ir::math::Point2::new(1.0, 0.0));
    assert!(cadmpeg_ir::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn sphere_patch_gets_degenerate_meridian_seam() {
    let mut cur = Cursor::new(sldprt_with_body(&sphere_patch_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.edges.len(), 4);
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.loops[0].coedges.len(), 4);
    assert_eq!(result.ir.model.pcurves.len(), 4);
    let pole = result
        .ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.0.contains("sphere-seam"))
        .expect("sphere pole pcurve");
    assert!(matches!(
        pole.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point2::new(0.0, std::f64::consts::FRAC_PI_2)
                && direction == cadmpeg_ir::math::Point2::new(1.0, 0.0)
    ));
    assert_eq!(pole.parameter_range, Some([0.0, std::f64::consts::TAU]));
    let seam = result
        .ir
        .model
        .edges
        .iter()
        .find(|edge| {
            result
                .source_fidelity
                .annotations
                .provenance
                .get(&edge.id.0)
                .and_then(|note| note.tag.as_deref())
                == Some("derived_sphere_seam")
        })
        .expect("sphere seam");
    assert_eq!(seam.start, seam.end);
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| seam.curve.as_ref() == Some(&curve.id))
        .expect("sphere seam curve");
    assert!(matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Degenerate { point }
            if point == cadmpeg_ir::math::Point3::new(0.0, 0.0, 1000.0)
    ));
    let vertex = result
        .ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == seam.start)
        .unwrap();
    let point = result
        .ir
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap();
    assert_eq!(
        [point.position.x, point.position.y, point.position.z],
        [0.0, 0.0, 1000.0]
    );
}

#[test]
fn decode_recovers_overlapping_topology_records() {
    let f = sldprt_with_body(&triangle_body_with_overlapping_point());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
}

#[test]
fn decode_recovers_tripled_deltas_topology() {
    let mut cur = Cursor::new(sldprt_with_body(&tripled_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.coedges.len(), 3);
    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.faces.len(), 1);
}

#[test]
fn decode_resolves_prefixed_deltas_edge_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;
    let mut cur = Cursor::new(sldprt_with_body(&prefixed_edge_triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Line { .. })));
}

#[test]
fn decode_preserves_explicit_body_membership() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(result.ir.model.shells.len(), 2);
    assert_eq!(result.ir.model.faces.len(), 2);
    assert_eq!(result.ir.model.bodies[0].id.0, "sldprt:brep:body#500");
    assert_eq!(result.ir.model.bodies[1].id.0, "sldprt:brep:body#501");
}

#[test]
fn decode_preserves_multiple_regions_and_shells_per_body() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 511, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 511, 0x001b, &[521, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 521, 0x001f, &[531, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 530, 0x0021, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 531, 0x0021, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));

    let mut result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 2);
    assert_eq!(result.ir.model.shells.len(), 2);
    assert_eq!(result.ir.model.bodies[0].regions.len(), 2);
    assert!(result
        .ir
        .model
        .regions
        .iter()
        .all(|region| region.shells.len() == 1));
    assert!(result
        .ir
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);

    result.ir.model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&result.ir, &result.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir.model.bodies.len(), 1);
    assert_eq!(regenerated.ir.model.regions.len(), 2);
    assert_eq!(regenerated.ir.model.shells.len(), 2);
    assert!(regenerated
        .ir
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
}

#[test]
fn decode_follows_connector_region_lump_and_shell_chain() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[510, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x0019, &[0, 520, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001b, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 530, 0x001f, &[540, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 540, 0x0021, &[550, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 550, 0x0023, &[700, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.regions[0].id.0, "sldprt:brep:region#520");
    assert_eq!(decoded.ir.model.shells[0].id.0, "sldprt:brep:shell#550");
    assert_eq!(decoded.ir.model.shells[0].faces.len(), 1);
    assert_eq!(
        decoded.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
}

#[test]
fn decode_binds_schema_32001_face_intervals_through_bridge_ids() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[0, 510, 600, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 530, 0x0021, &[540, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 540, 0x0023, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 600, 0x0015, &[0, 500, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x001f, &[10, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 900, 0.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(decoded.report.geometry_transferred);
    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert_eq!(decoded.ir.model.shells[0].faces[0].0, "sldprt:brep:face#10");
}

#[test]
fn decode_partitions_interleaved_schema_33103_faces_by_adjacency() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[90, 510, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[91, 511, 0, 0, 0, 0]));
    body.extend(entity51(2, 510, 0x0019, &[90, 520, 0, 0, 0, 0]));
    body.extend(entity51(2, 511, 0x0019, &[91, 521, 0, 0, 0, 0]));
    for (region, lump, shell_link, shell) in [(520, 530, 540, 550), (521, 531, 541, 551)] {
        body.extend(entity51(1, region, 0x001b, &[lump, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, lump, 0x001f, &[shell_link, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, shell_link, 0x0021, &[shell, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, shell, 0x0023, &[0, 0, 0, 0, 0, 0]));
    }
    body.extend(entity51(2, 600, 0x0013, &[90, 500, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0015, &[701, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 601, 0x0013, &[91, 501, 0, 0, 0, 0]));
    body.extend(entity51(1, 800, 0x0015, &[801, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 701, 0x0015, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 801, 0x0015, &[800, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(owned_triangle(400, 800, 10.0));
    body.extend(owned_triangle(600, 801, 12.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir.model.shells.len(), 4);
    assert!(decoded
        .ir
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    for (native_shell, face_suffixes) in [(550, ["#10", "#210"]), (551, ["#410", "#610"])] {
        let prefix = format!("sldprt:brep:shell#{native_shell}");
        let faces = decoded
            .ir
            .model
            .shells
            .iter()
            .filter(|shell| shell.id.0.starts_with(&prefix))
            .flat_map(|shell| &shell.faces)
            .collect::<Vec<_>>();
        assert_eq!(faces.len(), 2);
        assert!(face_suffixes
            .iter()
            .all(|suffix| faces.iter().any(|face| face.0.ends_with(suffix))));
    }
}

#[test]
fn decode_partitions_disc14_faces_by_native_shell_rings() {
    let mut body = Vec::new();
    body.extend(entity51(1, 900, 0x001a, &[500, 501, 0, 0, 0, 0]));
    body.extend(entity51(1, 500, 0x0016, &[550, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 501, 0x0016, &[602, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 550, 0x0012, &[600, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 600, 0x0020, &[0, 0, 609, 601, 0, 0]));
    body.extend(entity51(1, 601, 0x0020, &[0, 0, 701, 600, 0, 0]));
    body.extend(entity51(1, 602, 0x0020, &[0, 0, 612, 603, 0, 0]));
    body.extend(entity51(1, 603, 0x0020, &[0, 0, 613, 602, 0, 0]));
    body.extend(entity51(1, 609, 0x001e, &[0, 0, 610, 0, 0, 0]));
    for (geometry, face) in [(610, 700), (611, 701), (612, 800), (613, 801)] {
        body.extend(entity51(1, geometry, 0x0018, &[0, 0, face, 0, 0, 0]));
        body.extend(entity51(1, face, 0x0014, &[0, 0, 0, 0, 0, 0]));
    }
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));
    body.extend(owned_triangle(400, 800, 10.0));
    body.extend(owned_triangle(600, 801, 12.0));

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.regions.len(), 1);
    assert_eq!(decoded.ir.model.shells.len(), 4);
    assert!(decoded
        .ir
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));

    decoded.ir.model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir.model.regions.len(), 1);
    assert_eq!(regenerated.ir.model.shells.len(), 4);
    assert!(regenerated
        .ir
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
}

#[test]
fn decode_partitions_disc20_faces_by_native_single_shell_lattice() {
    let mut body = Vec::new();
    body.extend(entity51(2, 900, 0x001a, &[500, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 500, 0x0016, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0020, &[0, 710, 0, 701, 701, 0]));
    body.extend(entity51(1, 701, 0x0020, &[0, 711, 0, 700, 700, 0]));
    body.extend(entity51(
        4,
        710,
        0x0024,
        &[0, 720, 700, 711, 711, 0, 0, 0, 0],
    ));
    body.extend(entity51(
        4,
        711,
        0x0024,
        &[0, 721, 701, 710, 710, 0, 0, 0, 0],
    ));
    body.extend(entity51(3, 720, 0x0026, &[0, 0, 710, 721, 721, 0]));
    body.extend(entity51(3, 721, 0x0026, &[0, 0, 711, 720, 720, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 2.0));

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(decoded.ir.model.bodies[0].id.0, "sldprt:brep:body#900");
    assert_eq!(decoded.ir.model.regions[0].id.0, "sldprt:brep:region#900");
    assert_eq!(decoded.ir.model.shells[0].id.0, "sldprt:brep:shell#500");
    assert_eq!(decoded.ir.model.shells.len(), 2);
    assert!(decoded
        .ir
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    assert_eq!(decoded.ir.model.regions[0].shells.len(), 2);
    assert!(!decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("No body record")));
}

#[test]
fn semantic_writer_preserves_multiple_body_ownership() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 0.0));
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir.model.bodies.len(), 2);
    assert_eq!(regenerated.ir.model.regions.len(), 2);
    assert_eq!(regenerated.ir.model.shells.len(), 2);
    assert!(regenerated
        .ir
        .model
        .shells
        .iter()
        .all(|shell| shell.faces.len() == 1));
    assert!(regenerated.ir.model.regions.iter().all(|region| {
        regenerated.source_fidelity.annotations.provenance[&region.id.0]
            .tag
            .as_deref()
            == Some("00_51_region")
    }));
    assert!(regenerated.ir.model.shells.iter().all(|shell| {
        regenerated.source_fidelity.annotations.provenance[&shell.id.0]
            .tag
            .as_deref()
            == Some("00_51_shell")
    }));
}

#[test]
fn edge_uses_decoded_line_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 70)); // curve = line carrier 70
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.curves.len(), 1);
    match &result.ir.model.curves[0].geometry {
        CurveGeometry::Line { direction, .. } => assert_eq!(direction.x, 1.0),
        other => panic!("expected line, got {other:?}"),
    }
    assert_eq!(
        result
            .ir
            .model
            .edges
            .iter()
            .filter(|e| e.curve.is_some())
            .count(),
        1
    );
    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert!(result
        .ir
        .model
        .coedges
        .iter()
        .any(|coedge| !coedge.pcurves.is_empty()));
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn edge_uses_decode_nurbs_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = triangle_body();
    body.extend(nurbs_curve_carrier(170, 171));
    let edge = body
        .windows(2)
        .position(|w| w == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let nurbs = result
        .ir
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS curve");
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 3);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
}

#[test]
fn edge_uses_decode_typed_reference_nurbs_curve() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = triangle_body();
    body.extend(typed_nurbs_curve_carrier(170, 171));
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let nurbs = result
        .ir
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS curve");
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 3);
}

#[test]
fn reused_carrier_attribute_resolves_by_geometry_kind() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&70u16.to_be_bytes());
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(plane_carrier(
        70,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
}

#[test]
fn false_later_loop_candidate_does_not_replace_owned_loop() {
    let mut body = triangle_body();
    body.extend(loop_head(20, 30, 999));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 3);
    assert_eq!(result.ir.model.loops[0].id.0, "sldprt:brep:loop#20");
}

#[test]
fn faces_decode_nurbs_surface() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(nurbs_surface_carrier(180, 181, 10));
    let bridge = body
        .windows(2)
        .position(|w| w == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let nurbs = result
        .ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS surface");
    assert_eq!((nurbs.u_degree, nurbs.v_degree), (1, 1));
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
    assert_eq!(nurbs.control_points.len(), 4);
}

#[test]
fn faces_decode_markerless_nurbs_surface_arrays() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = triangle_body();
    body.extend(markerless_nurbs_surface_carrier(180, 181, 10));
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let nurbs = result
        .ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS surface");
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
}

#[test]
fn semantic_writer_regenerates_modified_nurbs_carriers() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge_offset = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge_offset + 26..bridge_offset + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
    body.extend(nurbs_curve_carrier(170, 171));
    body.extend(nurbs_surface_carrier(180, 181, 10));
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let CurveGeometry::Nurbs(curve) = &mut decoded.ir.model.curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    curve.control_points[1].y += 250.0;
    let expected_curve = curve.clone();
    let SurfaceGeometry::Nurbs(surface) = &mut decoded.ir.model.surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    surface.control_points[3].z += 500.0;
    let expected_surface = surface.clone();

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated.ir.model.curves.iter().any(
        |curve| matches!(&curve.geometry, CurveGeometry::Nurbs(value) if value == &expected_curve)
    ));
    assert!(regenerated.ir.model.surfaces.iter().any(
        |surface| matches!(&surface.geometry, SurfaceGeometry::Nurbs(value) if value == &expected_surface)
    ));
}

#[test]
fn native_patch_edits_nurbs_carriers_beside_untyped_surfaces() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    let bridge_offset = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge_offset + 26..bridge_offset + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
    body.extend(nurbs_curve_carrier(170, 171));
    body.extend(nurbs_surface_carrier(180, 181, 10));
    body.extend(bridge(210, 220, 999));
    body.extend(loop_head(220, 230, 210));
    body.extend(coedge(230, 220, 231, 250, 0, 240, false));
    body.extend(coedge(231, 220, 232, 251, 0, 241, false));
    body.extend(coedge(232, 220, 230, 252, 0, 242, false));
    body.extend(edge_use(240, 0));
    body.extend(edge_use(241, 0));
    body.extend(edge_use(242, 0));
    body.extend(vertex_use(250, 260));
    body.extend(vertex_use(251, 261));
    body.extend(vertex_use(252, 262));
    body.extend(world_point(260, [10.0, 0.0, 0.0]));
    body.extend(world_point(261, [11.0, 0.0, 0.0]));
    body.extend(world_point(262, [10.0, 1.0, 0.0]));

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let curve = decoded
        .ir
        .model
        .curves
        .iter_mut()
        .find_map(|curve| match &mut curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .unwrap();
    curve.control_points[1].y = 1_500.0;
    curve.knots[3..].fill(2.0);
    let expected_curve = curve.clone();
    let surface = decoded
        .ir
        .model
        .surfaces
        .iter_mut()
        .find_map(|surface| match &mut surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
            _ => None,
        })
        .unwrap();
    surface.control_points[3].z = 750.0;
    surface.u_knots[2..].fill(2.0);
    surface.v_knots[2..].fill(3.0);
    let expected_surface = surface.clone();

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated.ir.model.curves.iter().any(
        |curve| matches!(&curve.geometry, CurveGeometry::Nurbs(value) if value == &expected_curve)
    ));
    assert!(regenerated.ir.model.surfaces.iter().any(
        |surface| matches!(&surface.geometry, SurfaceGeometry::Nurbs(value) if value == &expected_surface)
    ));
    assert!(regenerated
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. })));
}

#[test]
fn nurbs_boundary_curve_gets_isoparametric_pcurve() {
    let mut body = triangle_body();
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&190u16.to_be_bytes());
    body.extend(nurbs_surface_carrier(180, 181, 10));
    body.extend(linear_nurbs_curve_carrier(190, 191));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
    }));
}

#[test]
fn linear_nurbs_surface_boundary_gets_affine_line_pcurve() {
    let mut body = triangle_body();
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&190u16.to_be_bytes());
    body.extend(nurbs_surface_carrier(180, 181, 10));
    body.extend(line_carrier(190, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir.model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
            && matches!(
                pcurve.geometry,
                cadmpeg_ir::geometry::PcurveGeometry::Line { direction, .. }
                    if direction.v == 0.0 && direction.u != 0.0
            )
    }));
}

#[test]
fn rational_nurbs_surface_row_gets_isoparametric_pcurve() {
    let mut body = triangle_body();
    let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&190u16.to_be_bytes());
    body.extend(rational_nurbs_surface_carrier(180, 181, 10));
    body.extend(rational_linear_nurbs_curve_carrier(190, 191));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir.model.pcurves.iter().any(|pcurve| {
        result
            .source_fidelity
            .annotations
            .provenance
            .get(&pcurve.id.0)
            .and_then(|note| note.tag.as_deref())
            == Some("derived_nurbs_isoparametric_pcurve")
    }));
}

#[test]
fn decode_transfers_body_material_color() {
    let f = sldprt_with_body_and_material(&triangle_body(), "Steel", [32, 64, 128]);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let color = result.ir.model.bodies[0].color.expect("body color");
    assert!((color.r - 32.0 / 255.0).abs() < 1e-6);
    assert!((color.g - 64.0 / 255.0).abs() < 1e-6);
    assert!((color.b - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(result.ir.model.appearances.len(), 1);
    assert_eq!(result.ir.model.appearance_bindings.len(), 1);
    assert_eq!(
        result.ir.model.appearances[0].name.as_deref(),
        Some("Steel")
    );
}

#[test]
fn decode_preserves_ambiguous_materials_without_fabricating_ownership() {
    let mut source = sldprt_with_body(&triangle_body());
    let mut materials = material_payload("Steel", [32, 64, 128]);
    materials.extend(material_payload("Aluminum", [160, 170, 180]));
    source.extend(make_block(0x40, "SWObjects", &materials));

    let mut result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.appearances.len(), 2);
    assert!(result.ir.model.appearance_bindings.is_empty());
    assert!(result
        .ir
        .model
        .bodies
        .iter()
        .all(|body| body.color.is_none() && body.name.is_none()));

    result.ir.model.points[0].position.z += 1.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&result.ir, &result.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(regenerated.ir.model.appearances.len(), 2);
    assert_eq!(
        regenerated
            .ir
            .model
            .appearances
            .iter()
            .filter_map(|appearance| appearance.name.as_deref())
            .collect::<Vec<_>>(),
        vec!["Steel", "Aluminum"]
    );
    assert!(regenerated.ir.model.appearance_bindings.is_empty());
}

#[test]
fn semantic_writer_preserves_body_material() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_material(
                &triangle_body(),
                "Steel",
                [32, 64, 128],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;
    let validation = cadmpeg_ir::validate::validate(&decoded.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        regenerated.ir.model.bodies[0].name.as_deref(),
        Some("Steel")
    );
    let color = regenerated.ir.model.bodies[0].color.unwrap();
    assert!((color.r - 32.0 / 255.0).abs() < 1e-6);
    assert!((color.g - 64.0 / 255.0).abs() < 1e-6);
    assert!((color.b - 128.0 / 255.0).abs() < 1e-6);
    assert!(regenerated
        .ir
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.name.as_deref() == Some("Steel")));
}

#[test]
fn semantic_writer_rejects_overlong_material_names() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_material(
                &triangle_body(),
                "Steel",
                [32, 64, 128],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.appearances[0].name = Some("M".repeat(256));
    decoded.ir.model.bodies[0].name = Some("M".repeat(256));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("material name is too long"));
}

#[test]
fn decode_binds_entity53_color_to_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.report.losses.len(), 1, "{:#?}", result.report.losses);
    assert_eq!(
        result.report.losses[0].message,
        "1 configuration state(s) are inferred from geometry partitions without native configuration definitions."
    );
    let binding = result
        .ir
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let appearance = result
        .ir
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap();
    let color = appearance.base_color.unwrap();
    assert_eq!([color.r, color.g, color.b], [0.25, 0.5, 0.75]);
}

#[test]
fn decode_does_not_bind_color_to_an_unemitted_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut body = Vec::new();
    body.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    body.extend(entity51(1, 701, 0x0015, &[0, 0, 0, 0, 0, 901]));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(entity53_color(901, [0.75, 0.5, 0.25]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(plane_carrier(
        200,
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(bridge_owned(110, 120, 200, 701));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.appearances.len(), 2);
    assert_eq!(
        result
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses).is_ok());
}

#[test]
fn decode_removes_edges_and_vertices_from_a_rejected_loop() {
    let mut body = triangle_body();
    body.extend(plane_carrier(
        200,
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(bridge(110, 120, 200));
    body.extend(loop_head(120, 130, 110));
    body.extend(coedge(130, 120, 131, 150, 0, 140, false));
    body.extend(coedge(131, 120, 132, 151, 0, 141, false));
    body.extend(coedge(132, 120, 130, 152, 0, 142, false));
    body.extend(edge_use(140, 0));
    body.extend(edge_use(141, 0));
    body.extend(edge_use(142, 0));
    body.extend(vertex_use(150, 160));
    body.extend(vertex_use(151, 161));
    body.extend(vertex_use(152, 162));
    body.extend(world_point(160, [2.0, 0.0, 0.0]));
    body.extend(world_point(161, [3.0, 0.0, 0.0]));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.points.len(), 3);
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses).is_ok());
}

#[test]
fn partition_point_refs_do_not_select_deltas_framing() {
    let mut body = triangle_body();
    let point = body
        .windows(4)
        .position(|window| window == [0x00, 0x1d, 0x00, 0x3c])
        .expect("point 60");
    for (index, reference) in [1u16, 378, 379, 373].into_iter().enumerate() {
        let at = point + 8 + index * 2;
        body[at..at + 2].copy_from_slice(&reference.to_be_bytes());
    }

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.points.len(), 3);
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses).is_ok());
}

#[test]
fn deltas_point_index_does_not_replace_partition_coordinates() {
    let partition = triangle_body();
    let mut deltas = Vec::new();
    for attr in 60u16..80 {
        deltas.extend_from_slice(&[0x00, 0x1d]);
        deltas.extend_from_slice(&attr.to_be_bytes());
    }

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_partition_and_deltas(&partition, &deltas)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let point = result
        .ir
        .model
        .points
        .iter()
        .find(|point| point.id.0.ends_with("#60"))
        .unwrap();
    assert_eq!(point.position, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
}

#[test]
fn semantic_writer_preserves_face_appearance() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let binding = regenerated
        .ir
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let color = regenerated
        .ir
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .and_then(|appearance| appearance.base_color)
        .unwrap();
    assert_eq!([color.r, color.g, color.b], [0.25, 0.5, 0.75]);
}

#[test]
fn decode_binds_adjacent_entity53_color_to_disc14_face() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0014, &[0, 0, 0, 0, 0, 0]));
    body.extend(entity53_color(901, [1.0, 0.125, 0.0]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut cur = Cursor::new(sldprt_with_body(&body));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let binding = result
        .ir
        .model
        .appearance_bindings
        .iter()
        .find(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
        .expect("face binding");
    let color = result
        .ir
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .unwrap()
        .base_color
        .unwrap();
    assert_eq!([color.r, color.g, color.b], [1.0, 0.125, 0.0]);
}

#[test]
fn decode_reports_display_list_geometry() {
    let f = sldprt_with_body_and_display_list(&triangle_body());
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let source = result.ir.source.as_ref().expect("source metadata");

    assert_eq!(
        source
            .attributes
            .get("displaylist_vertices")
            .map(String::as_str),
        Some("3")
    );
    assert_eq!(
        source
            .attributes
            .get("displaylist_triangles")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(result.ir.model.tessellations.len(), 1);
    assert_eq!(result.ir.model.tessellations[0].vertices.len(), 3);
    assert_eq!(result.ir.model.tessellations[0].vertices[1].x, 1000.0);
    assert_eq!(result.ir.model.tessellations[0].triangles, vec![[0, 1, 2]]);
    assert_eq!(result.ir.model.tessellations[0].strip_lengths, vec![3]);
    assert_eq!(result.ir.model.tessellations[0].normals.len(), 3);
    assert_eq!(result.ir.model.tessellations[0].channels.len(), 6);
    assert!(result
        .ir
        .native_unknowns("sldprt")
        .unwrap()
        .iter()
        .any(|record| {
            result
                .source_fidelity
                .annotations
                .provenance
                .get(&record.id.0)
                .and_then(|note| note.tag.as_deref())
                == Some("displaylist_tessellation")
                && result
                    .source_fidelity
                    .retained_record(&record.id.0)
                    .is_some_and(|source| source.data.is_some())
        }));
}

#[test]
fn decode_rejects_inconsistent_display_list_table() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let at = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len()
        + 8
        + 16;
    payload[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.tessellations.is_empty());
    assert!(!result
        .ir
        .source
        .as_ref()
        .unwrap()
        .attributes
        .contains_key("displaylist_vertices"));
}

#[test]
fn decode_rejects_nonfinite_display_list_values() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let position_data = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len()
        + 8
        + 16
        + 4
        + 16;
    payload[position_data..position_data + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.tessellations.is_empty());
}

#[test]
fn decode_extracts_parametric_history() {
    let f = sldprt_with_body_and_history(&triangle_body());
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&result.ir);
    let history = &native.feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.configurations[0].material.as_deref(), Some("Steel"));
    assert_eq!(result.ir.model.configurations.len(), 1);
    assert_eq!(result.ir.model.configurations[0].name, "Default");
    assert_eq!(
        result.ir.model.configurations[0].material.as_deref(),
        Some("Steel")
    );
    assert_eq!(
        result.ir.model.configurations[0].native_ref.as_deref(),
        Some(history.configurations[0].id.as_str())
    );
    assert_eq!(history.features[0].kind, "BossExtrude");
    assert_eq!(history.features[0].xml_tag, "Extrusion");
    assert_eq!(history.features[0].parameters["Depth"], "12.5mm");
    assert_eq!(history.features[0].properties["Scope"], "Body1");
    assert_eq!(history.features[1].parent_source_id.as_deref(), Some("7"));
    assert_eq!(history.features[1].xml_tag, "EquationDrivenCurve");
    assert_eq!(result.ir.model.features.len(), 2);
    let neutral = &result.ir.model.features[0];
    assert_eq!(neutral.name.as_deref(), Some("Boss"));
    assert_eq!(
        neutral.native_ref.as_deref(),
        Some(history.features[0].id.as_str())
    );
    assert!(matches!(
        &neutral.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved(profile),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(12.5),
                    },
                    draft: None,
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        } if profile == &history.features[0].id
    ));
    assert_eq!(
        result.ir.model.features[1].parent.as_ref(),
        Some(&neutral.id)
    );
}

#[test]
fn decode_uses_plain_numeric_config_as_legacy_feature_input_lane() {
    let legacy = resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Config-7", &legacy));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let lanes = &sldprt_native(&decoded.ir).feature_input_lanes;
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].configuration.as_deref(), Some("7"));
    assert_eq!(lanes[0].native_payload, legacy);
}

#[test]
fn decode_prefers_explicit_feature_input_lanes_over_plain_config_streams() {
    let legacy = resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]);
    let explicit = resolved_feature_classes_with_ids(&[("Chamfer_c", "Bevel", 42)]);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Config-7", &legacy));
    source.extend(make_block(
        0x42,
        "Contents/Config-7-ResolvedFeatures",
        &explicit,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let lanes = &sldprt_native(&decoded.ir).feature_input_lanes;
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].configuration.as_deref(), Some("7"));
    assert_eq!(lanes[0].native_payload, explicit);
}

#[test]
fn decode_types_non_modeling_feature_tree_nodes() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureTreeNodeRole};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Annotations" Type="Annotations" id="101"/>
            <Feature Name="Ecuaciones" Type="Ecuaciones" id="102"/>
            <Feature Name="Bodies" Type="Solid Bodies" id="103"/>
            <Feature Name="Light" Type="Direccional" id="104"/>
            <Feature Name="Unknown" Type="CustomOperation" id="105"/>
            <Sketch Name="Origen" Type="Croquis localizado" id="106"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moDetailCabinet_c", "Annotations", 101),
            ("moEqnFolder_c", "Ecuaciones", 102),
            ("moSolidBodyFolder_c", "Bodies", 103),
            ("moOriginProfileFeature_c", "Origen", 106),
        ]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let definitions = decoded
        .ir
        .model
        .features
        .iter()
        .map(|feature| &feature.definition)
        .collect::<Vec<_>>();
    assert!(matches!(
        definitions[0],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
    assert!(matches!(
        definitions[1],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    assert!(matches!(
        definitions[2],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::SolidBodies,
            ..
        }
    ));
    assert!(matches!(definitions[3], FeatureDefinition::Native { .. }));
    assert!(matches!(definitions[4], FeatureDefinition::Native { .. }));
    assert!(matches!(
        definitions[5],
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::ModelOrigin,
            ..
        }
    ));
    assert!(!decoded
        .ir
        .model
        .features
        .iter()
        .any(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. })));
    decoded.ir.model.features[0].name = Some("Document annotations".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .encode_with_source_fidelity(&decoded.ir, Some(&decoded.source_fidelity), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
}

#[test]
fn decode_leaves_position_allocated_tree_nodes_untyped() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Luces y camaras" Type="Localized" id="6"/>
            <Feature Name="Ambiental" Type="Localized" id="12"/>
            <Feature Name="Direccional uno" Type="Localized" id="13"/>
            <Feature Name="Direccional dos" Type="Localized" id="14"/>
            <Feature Name="Direccional tres" Type="Localized" id="15"/>
            <Feature Name="Vistas" Type="Localized" id="19"/>
        </Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir
        .model
        .features
        .iter()
        .all(|feature| matches!(feature.definition, FeatureDefinition::Native { .. })));
}

#[test]
fn reserved_tree_node_ids_require_builtin_record_shape() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, FeatureDefinition, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Operation" Type="Localized" id="12"/>
            <Feature Name="Attributed" Type="Localized" id="19" State="custom"/>
        </Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::Native { .. }
    ));
}

#[test]
fn decode_binds_duplicate_feature_names_by_native_object_id() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureTreeNodeRole};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Folder" Type="Custom" id="41"/>
            <Feature Name="Folder" Type="Custom" id="42"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moEqnFolder_c", "Folder", 41),
            ("moSolidBodyFolder_c", "Folder", 42),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::SolidBodies,
            ..
        }
    ));
}

#[test]
fn decode_propagates_unique_object_class_by_serialized_type_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Redondeo1" Type="Redondeo" id="41"/>
            <Feature Name="Redondeo2" Type="Redondeo" id="42"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("Fillet_c", "Redondeo2", 42)]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    assert!(native.feature_histories[0]
        .features
        .iter()
        .all(|feature| feature.input_class.as_deref() == Some("Fillet_c")));
}

#[test]
fn decode_binds_repeated_instances_by_class_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="LocalizedFillet" id="41"/>
            <Feature Name="TokenSeed" Type="LocalizedFillet" id="42"/>
            <Feature Name="TokenOnly" Type="OpaqueType" id="43"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[("Fillet_c", "Seed", 41)]);
    for (name, object_id) in [("TokenSeed", 42u32), ("TokenOnly", 43)] {
        payload.extend_from_slice(&0x37a5u16.to_le_bytes());
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    assert!(native.feature_histories[0]
        .features
        .iter()
        .all(|feature| feature.input_class.as_deref() == Some("Fillet_c")));
}

#[test]
fn decode_does_not_propagate_ambiguous_object_class_by_type_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="First" Type="Custom" id="41"/>
            <Feature Name="Second" Type="Custom" id="42"/>
            <Feature Name="Third" Type="Custom" id="43"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("Fillet_c", "First", 41),
            ("moRefPlane_c", "Second", 42),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    assert_eq!(native.feature_histories[0].features[2].input_class, None);
}

#[test]
fn decode_does_not_bind_ambiguous_repeated_class_token() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="FilletSeed" Type="FilletType" id="41"/>
            <Feature Name="PlaneSeed" Type="PlaneType" id="42"/>
            <Feature Name="FilletToken" Type="FilletType" id="43"/>
            <Feature Name="PlaneToken" Type="PlaneType" id="44"/>
            <Feature Name="Unknown" Type="UnknownType" id="45"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[
        ("Fillet_c", "FilletSeed", 41),
        ("moRefPlane_c", "PlaneSeed", 42),
    ]);
    for (name, object_id) in [("FilletToken", 43u32), ("PlaneToken", 44), ("Unknown", 45)] {
        payload.extend_from_slice(&0x37a5u16.to_le_bytes());
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
    }
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    assert_eq!(native.feature_histories[0].features[4].input_class, None);
}

#[test]
fn decode_does_not_bind_object_class_by_display_name() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plane" Type="Custom" id="41"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plane", 42)]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Native { .. }
    ));
    assert_eq!(
        sldprt_native(&decoded.ir).feature_histories[0].features[0].input_class,
        None
    );
}

#[test]
fn keywords_root_id_does_not_create_feature_parentage() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords id="document"><Feature Name="Root" Type="Folder" id="1"><Feature Name="Nested" Type="Custom" id="2"/></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    let history = &native.feature_histories[0];
    assert_eq!(history.properties["id"], "document");
    assert_eq!(history.features[0].parent_source_id, None);
    assert_eq!(history.features[1].parent_source_id.as_deref(), Some("1"));
    assert!(crate::validate_native(&decoded.ir).is_empty());
}

#[test]
fn native_validation_rejects_duplicate_history_ordinals() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].features[1].ordinal = 0;
    });
    assert!(crate::validate_native(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains("repeats feature ordinal")));
}

#[test]
fn native_validation_rejects_broken_feature_graph() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].features[1].tree_parent = Some("missing-record".into());
    });
    assert!(crate::validate_native(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains("missing tree parent")));
}

#[test]
fn native_validation_rejects_broken_history_root_graph() {
    use crate::records::HistoryContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Feature Name="Root" Type="Custom" id="1"><Feature Name="Nested" Type="Custom" id="2"/></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        let history = &mut native.feature_histories[0];
        let nested = history
            .features
            .iter()
            .find(|feature| feature.name == "Nested")
            .unwrap()
            .id
            .clone();
        history.content = vec![
            HistoryContent::Feature(nested),
            HistoryContent::Configuration("missing-configuration".into()),
        ];
    });

    let messages = crate::validate_native(&decoded.ir)
        .into_iter()
        .map(|finding| finding.message)
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("references nested feature")));
    assert!(messages
        .iter()
        .any(|message| message.contains("references missing configuration")));
    assert!(messages
        .iter()
        .any(|message| message.contains("omits configuration")));
    assert!(messages
        .iter()
        .any(|message| message.contains("omits feature")));
}

#[test]
fn native_validation_rejects_orphan_history_records() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded
        .ir
        .native
        .namespace_mut("sldprt")
        .arenas
        .get_mut("features")
        .unwrap()[0]
        .fields
        .insert(
            "parent".into(),
            serde_json::Value::String("missing-history".into()),
        );
    assert!(crate::validate_native(&decoded.ir).iter().any(|finding| {
        finding.message.contains("invalid owner") && finding.message.contains("missing-history")
    }));
}

#[test]
fn native_store_rejects_mismatched_nested_owners_atomically() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    native.feature_histories[0].features[0].parent = "missing-history".into();
    let before = decoded.ir.native.namespace("sldprt").unwrap().clone();
    let error = native
        .store(decoded.ir.native.namespace_mut("sldprt"))
        .unwrap_err();
    assert!(error.to_string().contains("invalid owner"));
    assert_eq!(decoded.ir.native.namespace("sldprt").unwrap(), &before);
}

#[test]
fn native_validation_rejects_duplicate_sketch_marker_offsets() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        let offset = native.feature_input_lanes[0].sketch_entities[0].offset;
        native.feature_input_lanes[0].sketch_entities[1].offset = offset;
    });
    assert!(crate::validate_native(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains("repeats entity offset")));
}

#[test]
fn native_validation_rejects_edited_relation_binding() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].relation_bindings[0].family =
            crate::records::FeatureInputRelationFamily::LineLineDistance;
    });

    assert!(crate::validate_native(&decoded.ir).iter().any(|finding| {
        finding
            .message
            .contains("relation bindings do not match the native payload")
    }));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("edited relation bindings"));
}

#[test]
fn native_validation_rejects_edited_relation_instance() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].relation_instances[0].parameter_scalar_ref = None;
    });

    assert!(crate::validate_native(&decoded.ir).iter().any(|finding| {
        finding
            .message
            .contains("relation instances do not match the native payload")
    }));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("edited relation instances"));
}

#[test]
fn native_validation_requires_complete_ordered_sketch_markers() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1, 2],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].sketch_entities.remove(1);
        native.feature_input_lanes[0].sketch_entities[1].ordinal = 4;
    });
    let messages = crate::validate_native(&decoded.ir)
        .into_iter()
        .map(|finding| finding.message)
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("expects entity ordinal")));
    assert!(messages
        .iter()
        .any(|message| message.contains("omits marker at offset")));
}

#[test]
fn semantic_writer_rejects_incomplete_sketch_marker_lanes() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1, 2],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].sketch_entities.remove(1);
    });
    decoded.source_fidelity.annotations = cadmpeg_ir::Annotations::default();

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("has 3 markers but 2 native records"),
        "{error}"
    );
}

#[test]
fn semantic_writer_derives_resolved_feature_section_names() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.source_fidelity.annotations = cadmpeg_ir::Annotations::default();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].sketch_entities[0].kind =
            crate::records::SketchInputKind::Native(9);
    });

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| { block.section.as_deref() == Some("Contents/Config-0-ResolvedFeatures") }));

    let mut unscoped = decoded.ir;
    update_sldprt_native(&mut unscoped, |native| {
        native.feature_input_lanes[0].configuration = None;
    });
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&unscoped, &decoded.source_fidelity, &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    assert!(scan
        .blocks
        .iter()
        .any(|block| block.section.as_deref() == Some("Contents/ResolvedFeatures")));
}

#[test]
fn semantic_writer_preserves_idless_feature_tree_nodes() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Root" Type="Folder" id="1"><Folder Name="Group"><Sketch Name="Profile" Type="Sketch" id="2"/></Folder></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&decoded.ir).feature_histories[0].features;
    assert_eq!(
        native[1].tree_parent.as_deref(),
        Some(native[0].id.as_str())
    );
    assert_eq!(
        native[2].tree_parent.as_deref(),
        Some(native[1].id.as_str())
    );
    assert_eq!(
        decoded.ir.model.features[1].parent.as_ref(),
        Some(&decoded.ir.model.features[0].id)
    );
    assert_eq!(
        decoded.ir.model.features[2].parent.as_ref(),
        Some(&decoded.ir.model.features[1].id)
    );
    decoded.ir.model.features[2].name = Some("Edited Profile".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native.len(), 3);
    assert_eq!(native[0].xml_tag, "Feature");
    assert_eq!(native[1].xml_tag, "Folder");
    assert_eq!(native[2].xml_tag, "Sketch");
    assert_eq!(
        native[1].tree_parent.as_deref(),
        Some(native[0].id.as_str())
    );
    assert_eq!(
        native[2].tree_parent.as_deref(),
        Some(native[1].id.as_str())
    );
    assert_eq!(native[2].name, "Edited Profile");
}

#[test]
fn semantic_writer_applies_neutral_configuration_edits() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let configuration = &mut decoded.ir.model.configurations[0];
    configuration.name = "Machined".into();
    configuration.material = Some("Aluminum".into());
    configuration
        .properties
        .insert("Finish".into(), "Anodized".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&regenerated.ir);
    let configuration = &native.feature_histories[0].configurations[0];
    assert_eq!(configuration.name, "Machined");
    assert_eq!(configuration.material.as_deref(), Some("Aluminum"));
    assert_eq!(configuration.properties["Finish"], "Anodized");
    assert_eq!(regenerated.ir.model.configurations[0].name, "Machined");
}

#[test]
fn semantic_writer_rejects_conflicting_configuration_edits() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.configurations[0].name = "Neutral".into();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].configurations[0].name = "Native".into();
    });

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT configuration edits"));
}

#[test]
fn decode_projects_every_dimension_as_a_neutral_parameter() {
    use cadmpeg_ir::features::{Angle, DimensionDisplay, Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    let keywords = format!(
        r#"<Keywords><Feature Name="Inputs" Type="EquationDriven" id="16">
            <Dimension Name="Angle">90deg</Dimension>
            <Dimension Name="DisplayAngle">45.00{degree}</Dimension>
            <Dimension Name="Count">4</Dimension>
            <Dimension Name="Diameter">{diameter}2.5</Dimension>
            <Dimension Name="ModifiedDiameter">&lt;MOD-DIAM&gt;3.18</Dimension>
            <Dimension Name="Enabled">true</Dimension>
            <Dimension Name="Expression">D1@Sketch1 * 2</Dimension>
            <Dimension Name="Length">0.5in</Dimension>
            <Dimension Name="Radius">R0.5</Dimension>
            <Dimension Name="Ratio">1.25</Dimension>
        </Feature></Keywords>"#,
        degree = '\u{00b0}',
        diameter = '\u{2300}',
    );
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameters = &decoded.ir.model.parameters;
    assert_eq!(parameters.len(), 10);
    assert_eq!(
        parameters
            .iter()
            .map(|parameter| (parameter.ordinal, parameter.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, "Angle"),
            (1, "DisplayAngle"),
            (2, "Count"),
            (3, "Diameter"),
            (4, "ModifiedDiameter"),
            (5, "Enabled"),
            (6, "Expression"),
            (7, "Length"),
            (8, "Radius"),
            (9, "Ratio"),
        ]
    );
    let value = |name: &str| {
        parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .and_then(|parameter| parameter.value.as_ref())
    };
    assert!(matches!(
        value("Angle"),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
    assert!(matches!(
        value("DisplayAngle"),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12
    ));
    assert_eq!(value("Count"), Some(&ParameterValue::Integer(4)));
    assert_eq!(
        value("Diameter"),
        Some(&ParameterValue::Length(Length(2.5)))
    );
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "Diameter")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Diameter)
    );
    assert_eq!(
        value("ModifiedDiameter"),
        Some(&ParameterValue::Length(Length(3.18)))
    );
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "ModifiedDiameter")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Diameter)
    );
    assert_eq!(value("Enabled"), Some(&ParameterValue::Boolean(true)));
    assert_eq!(value("Expression"), None);
    assert_eq!(value("Length"), Some(&ParameterValue::Length(Length(12.7))));
    assert_eq!(value("Radius"), Some(&ParameterValue::Length(Length(0.5))));
    assert_eq!(
        parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Radius)
    );
    assert_eq!(value("Ratio"), Some(&ParameterValue::Real(1.25)));
    assert!(parameters
        .iter()
        .all(|parameter| parameter.owner.as_ref() == Some(&decoded.ir.model.features[0].id)));

    let radius = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Radius")
        .unwrap();
    radius.expression = "R2".into();
    radius.value = Some(ParameterValue::Length(Length(2.0)));
    let modified_diameter = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "ModifiedDiameter")
        .unwrap();
    modified_diameter.expression = "<MOD-DIAM>4".into();
    modified_diameter.value = Some(ParameterValue::Length(Length(4.0)));
    let display_angle = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "DisplayAngle")
        .unwrap();
    display_angle.expression = format!("30{}", '\u{00b0}');
    display_angle.value = Some(ParameterValue::Angle(Angle(30.0_f64.to_radians())));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native_parameters =
        &sldprt_native(&regenerated.ir).feature_histories[0].features[0].parameters;
    assert_eq!(native_parameters["Radius"], "R2");
    assert_eq!(native_parameters["ModifiedDiameter"], "<MOD-DIAM>4");
    assert_eq!(
        native_parameters["DisplayAngle"],
        format!("30{}", '\u{00b0}')
    );
    assert!(matches!(
        regenerated
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.value.as_ref()),
        Some(ParameterValue::Length(Length(2.0)))
    ));
    assert_eq!(
        regenerated
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Radius")
            .and_then(|parameter| parameter.display),
        Some(DimensionDisplay::Radius)
    );
    assert!(matches!(
        regenerated
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "DisplayAngle")
            .and_then(|parameter| parameter.value.as_ref()),
        Some(ParameterValue::Angle(Angle(angle)))
            if (*angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
    assert_eq!(
        regenerated
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "ModifiedDiameter")
            .map(|parameter| (parameter.display, parameter.value.as_ref())),
        Some((
            Some(DimensionDisplay::Diameter),
            Some(&ParameterValue::Length(Length(4.0)))
        ))
    );
}

#[test]
fn semantic_writer_applies_neutral_parameter_edits() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    parameter.expression = "20mm".into();
    parameter.value = Some(ParameterValue::Length(Length(20.0)));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].parameters["Depth"],
        "20mm"
    );
    assert_eq!(
        regenerated
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Depth")
            .unwrap()
            .expression,
        "20mm"
    );
}

#[test]
fn semantic_writer_preserves_dimension_attributes() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth" Driven="true" EquationId="D1@Boss">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = &mut decoded.ir.model.parameters[0];
    assert_eq!(parameter.properties["Driven"], "true");
    assert_eq!(parameter.properties["EquationId"], "D1@Boss");
    parameter.expression = "20mm".into();
    parameter.value = Some(ParameterValue::Length(Length(20.0)));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Depth"], "20mm");
    assert_eq!(feature.dimension_properties["Depth"]["Driven"], "true");
    assert_eq!(
        feature.dimension_properties["Depth"]["EquationId"],
        "D1@Boss"
    );
}

#[test]
fn semantic_writer_preserves_evaluated_equation_values() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth" Value="24mm" EquationId="D1@Boss">Width * 2</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = &mut decoded.ir.model.parameters[0];
    assert_eq!(parameter.expression, "Width * 2");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(24.0))));
    assert_eq!(parameter.properties["Value"], "24mm");
    parameter.expression = "Width * 3".into();
    parameter.value = Some(ParameterValue::Length(Length(36.0)));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = &regenerated.ir.model.parameters[0];
    assert_eq!(parameter.expression, "Width * 3");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(36.0))));
    assert_eq!(parameter.properties["Value"], "36mm");
    assert_eq!(parameter.properties["EquationId"], "D1@Boss");
}

#[test]
fn semantic_writer_projects_and_validates_parameter_dependencies() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Base" EquationId="D1@Equations">2mm</Dimension><Dimension Name="Wall Thickness">4mm</Dimension><Dimension Name="Datum &quot;A&quot;">1mm</Dimension><Dimension Name="Driven" EquationId="D2@Equations">&quot;Wall Thickness&quot; + &quot;Datum &quot;&quot;A&quot;&quot;&quot; + D1@Equations + &quot;Wall Thickness&quot;</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.parameters.len(), 4);
    assert_eq!(
        decoded.ir.model.parameters[3].dependencies,
        vec![
            decoded.ir.model.parameters[1].id.clone(),
            decoded.ir.model.parameters[2].id.clone(),
            decoded.ir.model.parameters[0].id.clone(),
        ]
    );

    decoded.ir.model.parameters[0]
        .properties
        .insert("EquationId".into(), "D1@Renamed".into());
    decoded.ir.model.parameters[1].name = "Wall Gauge".into();
    let mut renamed = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut renamed)
        .unwrap();
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded.ir.model.parameters[3].expression,
        "\"Wall Gauge\" + \"Datum \"\"A\"\"\" + D1@Renamed + \"Wall Gauge\""
    );
    assert_eq!(
        decoded.ir.model.parameters[3].dependencies,
        vec![
            decoded.ir.model.parameters[1].id.clone(),
            decoded.ir.model.parameters[2].id.clone(),
            decoded.ir.model.parameters[0].id.clone(),
        ]
    );

    decoded.ir.model.parameters[3].expression = "6mm".into();
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("dependencies are inconsistent with their expressions"));
}

#[test]
fn parameter_references_distinguish_reserved_expression_syntax() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="sin">1</Dimension><Dimension Name="pi">2</Dimension><Dimension Name="iif">3</Dimension><Dimension Name="Width">4mm</Dimension><Dimension Name="Driven">sin(30deg) + pi + iif(Width = 4mm, 1, 2) + &quot;sin&quot; + &quot;pi&quot; + &quot;iif&quot;</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter_id = |name: &str| {
        decoded
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
            .id
            .clone()
    };
    let expected_dependencies = vec![
        parameter_id("Width"),
        parameter_id("sin"),
        parameter_id("pi"),
        parameter_id("iif"),
    ];
    assert_eq!(
        decoded
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Driven")
            .unwrap()
            .dependencies,
        expected_dependencies
    );

    for (old_name, new_name) in [
        ("sin", "Sine input"),
        ("pi", "Pi input"),
        ("iif", "Choice input"),
    ] {
        decoded
            .ir
            .model
            .parameters
            .iter_mut()
            .find(|parameter| parameter.name == old_name)
            .unwrap()
            .name = new_name.into();
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let driven = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Driven")
        .unwrap();
    assert_eq!(
        driven.expression,
        "sin(30deg) + pi + iif(Width = 4mm, 1, 2) + \"Sine input\" + \"Pi input\" + \"Choice input\""
    );
    assert_eq!(driven.dependencies.len(), 4);
}

#[test]
fn decode_evaluates_parameter_dependency_expressions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Width">4mm</Dimension><Dimension Name="Copies">3</Dimension><Dimension Name="Double width">Width * 2</Dimension><Dimension Name="Per copy">&quot;Double width&quot; / Copies</Dimension><Dimension Name="Forward">Later + 1mm</Dimension><Dimension Name="Later">2mm</Dimension><Dimension Name="Scientific">1e-3 * Width</Dimension><Dimension Name="Mixed units">1ft + 1in + 1mil + 1uin + 1um + 1nm + 1&#197;</Dimension><Dimension Name="Power">2^3^2</Dimension><Dimension Name="Sine">sin(30deg)</Dimension><Dimension Name="Inverse sine">arcsin(0.5)</Dimension><Dimension Name="Absolute">abs(-2mm)</Dimension><Dimension Name="Root">sqr(9)</Dimension><Dimension Name="Sign negative">sgn(-2)</Dimension><Dimension Name="Sign zero">sgn(0)</Dimension><Dimension Name="Sign positive">sgn(2)</Dimension><Dimension Name="Pi">pi</Dimension><Dimension Name="Conditional">iif(Width >= 4mm, Width * 2, 1mm)</Dimension><Dimension Name="Leading equals">=iif(Copies&lt;&gt;3, 1, 2)</Dimension><Dimension Name="Comparison">Width = 4mm</Dimension><Dimension Name="Invalid">Width + Copies</Dimension><Dimension Name="Invalid area">Width^2</Dimension><Dimension Name="Invalid branches">iif(true, Width, Copies)</Dimension><Dimension Name="Invalid nested domain">sgn(arcsin(2))</Dimension></Feature></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let values = decoded
        .ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.value.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        values["Double width"],
        Some(ParameterValue::Length(Length(8.0)))
    );
    assert_eq!(
        values["Per copy"],
        Some(ParameterValue::Length(Length(8.0 / 3.0)))
    );
    assert_eq!(values["Forward"], Some(ParameterValue::Length(Length(3.0))));
    assert_eq!(
        values["Scientific"],
        Some(ParameterValue::Length(Length(0.004)))
    );
    assert_eq!(
        values["Mixed units"],
        Some(ParameterValue::Length(Length(
            304.8 + 25.4 + 0.0254 + 25.4e-6 + 1.0e-3 + 1.0e-6 + 1.0e-7
        )))
    );
    assert_eq!(values["Power"], Some(ParameterValue::Integer(512)));
    assert!(
        matches!(values["Sine"], Some(ParameterValue::Real(value)) if (value - 0.5).abs() < 1e-12)
    );
    assert!(matches!(
        values["Inverse sine"],
        Some(ParameterValue::Angle(cadmpeg_ir::features::Angle(value)))
            if (value - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
    assert_eq!(
        values["Absolute"],
        Some(ParameterValue::Length(Length(2.0)))
    );
    assert_eq!(values["Root"], Some(ParameterValue::Real(3.0)));
    assert_eq!(values["Sign negative"], Some(ParameterValue::Integer(-1)));
    assert_eq!(values["Sign zero"], Some(ParameterValue::Integer(0)));
    assert_eq!(values["Sign positive"], Some(ParameterValue::Integer(1)));
    assert_eq!(
        values["Pi"],
        Some(ParameterValue::Real(std::f64::consts::PI))
    );
    assert_eq!(
        values["Conditional"],
        Some(ParameterValue::Length(Length(8.0)))
    );
    assert_eq!(values["Leading equals"], Some(ParameterValue::Integer(2)));
    assert_eq!(values["Comparison"], Some(ParameterValue::Boolean(true)));
    assert_eq!(values["Invalid"], None);
    assert_eq!(values["Invalid area"], None);
    assert_eq!(values["Invalid branches"], None);
    assert_eq!(values["Invalid nested domain"], None);
    let ordinal = |name: &str| {
        decoded
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap()
            .ordinal
    };
    assert!(ordinal("Later") < ordinal("Forward"));
    assert!(!cadmpeg_ir::validate(&decoded.ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("parameter dependency")));
}

#[test]
fn semantic_writer_orders_forward_parameter_dependencies_before_consumers() {
    use crate::records::FeatureContent;
    use cadmpeg_ir::features::ParameterValue;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Result">Input + 1</Dimension><Dimension Name="Input">2</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut parameter_order = decoded
        .ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.ordinal, parameter.name.as_str()))
        .collect::<Vec<_>>();
    parameter_order.sort_unstable();
    assert_eq!(parameter_order, vec![(0, "Input"), (1, "Result")]);

    let result = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    result.expression = "Input + 2".into();
    result.value = Some(ParameterValue::Integer(4));
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(
        feature
            .content
            .iter()
            .filter_map(|content| match content {
                FeatureContent::Dimension(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["Input", "Result"]
    );
    let result = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "Input + 2");
    assert_eq!(result.value, Some(ParameterValue::Integer(4)));
    assert_eq!(result.dependencies.len(), 1);
}

#[test]
fn decode_projects_evaluated_equations_into_feature_semantics() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Equation boss" Type="BossExtrude" id="7" Operation="Join" EndCondition="Blind"><Dimension Name="Base">4mm</Dimension><Dimension Name="Depth">Base * 2</Dimension></Extrusion></Keywords>"#,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(8.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }
    ));
    let depth = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("depth parameter");
    assert_eq!(depth.expression, "Base * 2");
    assert_eq!(
        depth.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(Length(8.0)))
    );
    let native = &sldprt_native(&decoded.ir).feature_histories[0].features[0];
    assert_eq!(native.parameters["Depth"], "Base * 2");

    decoded.ir.model.features[0].name = Some("Renamed equation boss".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].parameters["Depth"],
        "Base * 2"
    );
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(8.0)
                    },
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_resolves_and_rewrites_owner_qualified_parameters() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="D1">2mm</Dimension></Feature><Feature Name="Sketch2" Type="Sketch" id="11"><Dimension Name="D1">3mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="12"><Dimension Name="Result">D1@Sketch1 * 2</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch1 = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap();
    let sketch1_parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&sketch1.id) && parameter.name == "D1")
        .unwrap()
        .id
        .clone();
    let result = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.dependencies, vec![sketch1_parameter.clone()]);

    decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == sketch1_parameter)
        .unwrap()
        .name = "Width".into();
    let mut renamed = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut renamed)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
        .unwrap();
    let result = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "Width@Sketch1 * 2");
    assert_eq!(result.dependencies.len(), 1);
}

#[test]
fn semantic_writer_rewrites_qualified_bare_equation_ids() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="Width" EquationId="D1">2mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="11"><Dimension Name="Result">D1@Sketch1 * 2</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let width = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Width")
        .unwrap();
    width.properties.insert("EquationId".into(), "D2".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let result = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "D2@Sketch1 * 2");
    assert_eq!(result.dependencies.len(), 1);
    assert_eq!(
        regenerated
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Width")
            .unwrap()
            .properties["EquationId"],
        "D2"
    );

    regenerated
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Width")
        .unwrap()
        .properties
        .insert("EquationId".into(), "D3@Sketch1".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &regenerated.ir,
            &regenerated.source_fidelity,
            &mut encoded,
        )
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        regenerated
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Result")
            .unwrap()
            .expression,
        "D3@Sketch1 * 2"
    );
}

#[test]
fn semantic_writer_rewrites_parameter_owners_when_features_are_renamed() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="Width" EquationId="D1@Sketch1">2mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="11"><Dimension Name="Result">D1@Sketch1 * 2</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch = decoded
        .ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap();
    sketch.name = Some("Profile".into());
    decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Width")
        .unwrap()
        .name = "Gauge".into();

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(regenerated
        .ir
        .model
        .features
        .iter()
        .any(|feature| feature.name.as_deref() == Some("Profile")));
    let gauge = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Gauge")
        .unwrap();
    assert_eq!(gauge.properties["EquationId"], "D1@Profile");
    let result = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "D1@Profile * 2");
    assert_eq!(result.dependencies, vec![gauge.id.clone()]);
}

#[test]
fn feature_rename_rewrites_only_its_qualified_parameter_references() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sketch1" Type="Sketch" id="10"><Dimension Name="D1">2mm</Dimension></Feature><Feature Name="Sketch2" Type="Sketch" id="11"><Dimension Name="D1">3mm</Dimension></Feature><Feature Name="Equations" Type="EquationDriven" id="12"><Dimension Name="Result">D1@Sketch1 + D1@Sketch2</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded
        .ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .unwrap()
        .name = Some("Profile".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let result = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Result")
        .unwrap();
    assert_eq!(result.expression, "D1@Profile + D1@Sketch2");
    assert_eq!(result.dependencies.len(), 2);
}

#[test]
fn semantic_writer_preserves_empty_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12mm</Dimension><Dimension Name="External" Driven="true"/></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let empty = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "External")
        .unwrap();
    assert_eq!(empty.expression, "");
    assert_eq!(empty.value, None);
    let depth = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    depth.expression = "20mm".into();
    depth.value = Some(ParameterValue::Length(Length(20.0)));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.parameters["External"], "");
    assert_eq!(feature.dimension_properties["External"]["Driven"], "true");
}

#[test]
fn semantic_writer_preserves_keywords_attributes() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords Name="Bracket" Schema="34000" Revision="12"><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = &mut decoded.ir.model.parameters[0];
    parameter.expression = "20mm".into();
    parameter.value = Some(ParameterValue::Length(Length(20.0)));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let history = &sldprt_native(&regenerated.ir).feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.properties["Schema"], "34000");
    assert_eq!(history.properties["Revision"], "12");
}

#[test]
fn semantic_writer_preserves_keywords_child_order() {
    use crate::records::HistoryContent;
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="First" Type="Custom" id="1"/>between<Configuration Name="Default"/><Extrusion Name="Boss" Type="BossExtrude" id="2"><Dimension Name="Depth">12mm</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let depth = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    depth.expression = "20mm".into();
    depth.value = Some(ParameterValue::Length(Length(20.0)));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let history = &sldprt_native(&regenerated.ir).feature_histories[0];
    assert!(matches!(
        history.content.as_slice(),
        [
            HistoryContent::Feature(_),
            HistoryContent::Text(text),
            HistoryContent::Configuration(_),
            HistoryContent::Feature(_),
        ] if text == "between"
    ));
}

#[test]
fn semantic_writer_applies_history_root_ordinals() {
    use crate::records::HistoryContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="First" Type="Custom" id="1"/><Configuration Name="A"/><Feature Name="Second" Type="Custom" id="2"/><Configuration Name="B"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    for feature in &mut decoded.ir.model.features {
        feature.ordinal = u64::from(feature.name.as_deref() == Some("First"));
    }
    for configuration in &mut decoded.ir.model.configurations {
        configuration.ordinal = u32::from(configuration.name == "A");
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let history = &sldprt_native(&regenerated.ir).feature_histories[0];
    let names = history
        .content
        .iter()
        .filter_map(|item| match item {
            HistoryContent::Feature(id) => history
                .features
                .iter()
                .find(|feature| feature.id == *id)
                .map(|feature| feature.name.as_str()),
            HistoryContent::Configuration(id) => history
                .configurations
                .iter()
                .find(|configuration| configuration.id == *id)
                .map(|configuration| configuration.name.as_str()),
            HistoryContent::Text(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["Second", "B", "First", "A"]);
}

#[test]
fn semantic_writer_applies_neutral_parameter_order() {
    use crate::records::FeatureContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Ordered" Type="EquationDriven" id="41"><Dimension Name="First">1</Dimension><Child Name="Nested" Type="Folder" id="42"/><Dimension Name="Second">2</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    for parameter in &mut decoded.ir.model.parameters {
        parameter.ordinal = match parameter.name.as_str() {
            "First" => 1,
            "Second" => 0,
            name => panic!("unexpected parameter {name}"),
        };
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        regenerated
            .ir
            .model
            .parameters
            .iter()
            .map(|parameter| (parameter.ordinal, parameter.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "Second"), (1, "First")]
    );
    let content = &sldprt_native(&regenerated.ir).feature_histories[0].features[0].content;
    assert_eq!(
        content
            .iter()
            .filter_map(|item| match item {
                FeatureContent::Dimension(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["Second", "First"]
    );
    assert!(content
        .iter()
        .any(|item| matches!(item, FeatureContent::Feature(_))));
}

#[test]
fn semantic_writer_rejects_conflicting_parameter_edits() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Depth")
        .unwrap();
    parameter.expression = "20mm".into();
    parameter.value = Some(ParameterValue::Length(Length(20.0)));
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert("Depth".into(), "30mm".into());
    });

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT parameter edits"));
}

#[test]
fn semantic_writer_rejects_conflicting_dimension_property_edits() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equation" Type="EquationDriven" id="41"><Dimension Name="Depth" Driven="false">12mm</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded.ir.model.parameters[0]
        .properties
        .insert("Driven".into(), "neutral".into());
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].features[0]
            .dimension_properties
            .get_mut("Depth")
            .unwrap()
            .insert("Driven".into(), "native".into());
    });

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT parameter edits"));
}

#[test]
fn decode_projects_cut_extrude_with_canonical_length() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Cut" Type="CutExtrude" id="9"><Dimension Name="Depth">0.5in</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(12.7),
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn decode_projects_compact_extrusion_with_unresolved_extent() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, ProfileRef, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Compact" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));

    decoded.ir.model.features[0].name = Some("Renamed compact extrusion".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_extrusion_termination() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, FeatureDefinition, Termination};

    fn compact_extrusion_payload(through_all: bool) -> Vec<u8> {
        let mut payload = resolved_feature_classes_with_ids(&[("moExtrusion_c", "Boss", 9)]);
        let offset = payload.len();
        payload.resize(offset + 104, 0);
        if through_all {
            payload[offset..offset + 2].copy_from_slice(&[0x0c, 0x8e]);
            payload[offset + 4] = 1;
            payload[offset + 18] = 1;
            payload[offset + 30..offset + 34].copy_from_slice(&[1, 0, 0, 1]);
            payload[offset + 92] = 1;
        }
        payload
    }

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Blind"/><Extrusion Name="Boss" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &compact_extrusion_payload(true),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-1-ResolvedFeatures",
        &compact_extrusion_payload(false),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        }
    ));
    let feature_id = feature.id.clone();
    assert!(matches!(
        decoded.ir.model.configurations[0]
            .feature_states
            .get(&feature_id)
            .map(|state| &state.definition),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ThroughAll,
                    ..
                }
            },
            ..
        })
    ));
    assert!(matches!(
        decoded.ir.model.configurations[1]
            .feature_states
            .get(&feature_id)
            .map(|state| &state.definition),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                }
            },
            ..
        })
    ));
    assert!(
        decoded
            .ir
            .model
            .configurations
            .iter()
            .all(|configuration| configuration.feature_states.len()
                == decoded.ir.model.features.len())
    );
    let round_trip =
        cadmpeg_ir::CadIr::from_json(&serde_json::to_string(&decoded.ir).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.configurations[0]
            .feature_states
            .get(&feature_id),
        decoded.ir.model.configurations[0]
            .feature_states
            .get(&feature_id)
    );

    let mut edited = decoded.ir.clone();
    let replacement = edited.model.configurations[0].feature_states[&feature_id].clone();
    edited.model.configurations[1]
        .feature_states
        .insert(feature_id, replacement);
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("configuration design-state edit has no complete native lane encoding"));
}

#[test]
fn decode_binds_adjacent_profile_feature_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="Sketch" id="8"/>
            <Extrusion Name="Boss" Type="Extrusion" id="9"/>
            <Sketch Name="Following" Type="Sketch" id="10"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile", 8),
            ("moExtrusion_c", "Boss", 9),
            ("moProfileFeature_c", "Following", 10),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile"))
        .unwrap();
    let extrusion = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &profile.id
    ));
    assert_eq!(extrusion.dependencies, vec![profile.id.clone()]);
}

#[test]
fn decode_does_not_globalize_configuration_local_adjacent_profile() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Sketch Name="Profile A" Type="Sketch" id="7"/><Sketch Name="Profile B" Type="Sketch" id="8"/><Extrusion Name="Boss" Type="Extrusion" id="9"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile A", 7),
            ("moExtrusion_c", "Boss", 9),
        ]),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Profile B", 8),
            ("moExtrusion_c", "Boss", 9),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let extrusion = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(owner),
            ..
        } if owner == extrusion.native_ref.as_deref().unwrap()
    ));
    let extrusion_id = extrusion.id.clone();
    let profile_a = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile A"))
        .unwrap();
    let profile_b = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile B"))
        .unwrap();
    let state_a = &decoded.ir.model.configurations[0].feature_states[&extrusion_id];
    let state_b = &decoded.ir.model.configurations[1].feature_states[&extrusion_id];
    assert!(matches!(
        &state_a.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile),
            ..
        } if profile == &profile_a.id
    ));
    assert!(matches!(
        &state_b.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile),
            ..
        } if profile == &profile_b.id
    ));
    assert_eq!(state_a.dependencies, vec![profile_a.id.clone()]);
    assert_eq!(state_b.dependencies, vec![profile_b.id.clone()]);
}

#[test]
fn decode_binds_following_profile_marked_as_dissected_child() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Previous" Type="Sketch" id="7"/>
            <Extrusion Name="Boss" Type="Extrusion" id="9"/>
            <Sketch Name="Profile&lt;3&gt;" Type="Sketch" id="8" Description="Profile&lt;3&gt;"/>
        </Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moProfileFeature_c", "Previous", 7),
            ("moICE_c", "Boss", 9),
            ("moProfileFeature_c", "Profile<3>", 8),
        ]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile<3>"))
        .unwrap();
    let extrusion = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &profile.id
    ));
    assert_eq!(extrusion.dependencies, vec![profile.id.clone()]);
}

#[test]
fn decode_binds_profile_to_inline_extrusion_with_ambiguous_class_token() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="Sketch" id="8"/>
            <Extrusion Name="Cut" Type="Localized" id="9"/>
        </Keywords>"#,
    ));
    let mut payload = resolved_feature_classes_with_ids(&[("moProfileFeature_c", "Profile", 8)]);
    payload.extend_from_slice(&0x84c5u16.to_le_bytes());
    payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, 3]);
    for unit in "Cut".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(&[0; 4]);
    payload.extend_from_slice(&[0xca, 1, 2, 0x40]);
    payload.extend_from_slice(&9u32.to_le_bytes());
    payload.extend_from_slice(&[0; 4]);
    payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Profile"))
        .unwrap();
    let extrusion = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Cut"))
        .unwrap();
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            op: BooleanOp::Cut,
            ..
        } if feature == &profile.id
    ));
}

#[test]
fn semantic_writer_round_trips_sparse_positional_extrusions() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ParameterValue,
        Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Boss-Extrude7" id="9"><Dimension Name="D1">200</Dimension></Extrusion>
            <Extrusion Name="Cortar-Extruir2" id="10"><Dimension Name="D1">3</Dimension></Extrusion>
            <Extrusion Name="Custom operation" id="11"><Dimension Name="D1">4</Dimension></Extrusion>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let first_definition = &decoded.ir.model.features[0].definition;
    assert!(
        matches!(
            first_definition,
            FeatureDefinition::Extrude {
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Blind {
                            length: Length(200.0)
                        },
                        ..
                    }
                },
                op: BooleanOp::Unresolved,
                ..
            }
        ),
        "{first_definition:?}"
    );
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(3.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[2].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(4.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert_eq!(
        decoded.ir.model.parameters[0].value,
        Some(ParameterValue::Length(Length(200.0)))
    );
    assert_eq!(
        decoded.ir.model.parameters[1].value,
        Some(ParameterValue::Length(Length(3.0)))
    );
    assert_eq!(
        decoded.ir.model.parameters[2].value,
        Some(ParameterValue::Length(Length(4.0)))
    );

    let FeatureDefinition::Extrude {
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: Termination::Blind { length },
                        ..
                    },
            },
        ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed positional boss extrusion");
    };
    *length = Length(250.0);
    let FeatureDefinition::Extrude {
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: Termination::Blind { length },
                        ..
                    },
            },
        ..
    } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed positional cut extrusion");
    };
    *length = Length(4.5);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].parameters["D1"], "250");
    assert_eq!(native[1].parameters["D1"], "4.5");
    for feature in &native[..2] {
        assert!(!feature.parameters.contains_key("Depth"));
        assert!(!feature.properties.contains_key("EndCondition"));
        assert!(!feature.properties.contains_key("Operation"));
        assert!(!feature.properties.contains_key("Profile"));
    }
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(250.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir.model.features[1].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(4.5)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir.model.features[2].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(4.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));

    regenerated.ir.model.parameters[0].expression = "225".into();
    regenerated.ir.model.parameters[0].value = Some(ParameterValue::Length(Length(225.0)));
    let mut parameter_encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &regenerated.ir,
            &regenerated.source_fidelity,
            &mut parameter_encoded,
        )
        .unwrap();
    let parameter_regenerated = SldprtCodec
        .decode(
            &mut Cursor::new(parameter_encoded),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        sldprt_native(&parameter_regenerated.ir).feature_histories[0].features[0].parameters["D1"],
        "225"
    );
    assert_eq!(
        parameter_regenerated.ir.model.parameters[0].value,
        Some(ParameterValue::Length(Length(225.0)))
    );
}

#[test]
fn decode_resolves_feature_topology_selections() {
    use cadmpeg_ir::features::{
        BodySelection, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition,
        PathRef, ProfileRef, Termination,
    };

    let body_bytes = triangle_body();
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body_bytes)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = &base.ir.model.bodies[0].id.0;
    let face = &base.ir.model.faces[0].id.0;
    let edge = &base.ir.model.edges[0].id.0;
    let keywords = format!(
        r#"<Keywords>
            <Fillet Name="Round" Type="Fillet" id="1" Edges="{edge}"><Dimension Name="Radius">1mm</Dimension></Fillet>
            <DeleteFace Name="Delete" Type="DeleteFace" id="2" Faces="{face}" Heal="true"/>
            <Combine Name="Union" Type="Combine" id="3" Target="{body}" Tools="{body}" Operation="Join"/>
            <Extrusion Name="UpTo" Type="BossExtrude" id="4" Profile="{face}" EndCondition="ToFace" Face="{face}" Operation="Join"/>
            <Hole Name="Drill" Type="Hole" id="5" Face="{face}" EndCondition="ThroughAll"><Dimension Name="Diameter">2mm</Dimension></Hole>
            <Sweep Name="Rail" Type="Sweep" id="6" Profile="{face}" Path="{edge}" Operation="NewBody"/>
        </Keywords>"#
    );
    let mut source = sldprt_with_body(&body_bytes);
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir.model.edges[0].id.clone();
    let face_id = decoded.ir.model.faces[0].id.clone();
    let body_id = decoded.ir.model.bodies[0].id.clone();

    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Resolved { edges, native }, ..
        }] if edges == &[base.ir.model.edges[0].id.clone()] && native == edge)
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Resolved { faces, native },
            ..
        } if faces == &[base.ir.model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir.model.features[2].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Resolved { bodies, native },
            tools: BodySelection::Resolved { .. },
            ..
        } if bodies == &[base.ir.model.bodies[0].id.clone()] && native == body
    ));
    assert!(matches!(
        &decoded.ir.model.features[3].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Faces(profile_faces),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToFace {
                        face: FaceSelection::Resolved { faces, native },
                        ..
                    },
                    ..
                }
            },
            ..
        } if profile_faces == &[base.ir.model.faces[0].id.clone()]
            && faces == &[base.ir.model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir.model.features[4].definition,
        FeatureDefinition::Hole {
            face: Some(FaceSelection::Resolved { faces, native }),
            ..
        } if faces == &[base.ir.model.faces[0].id.clone()] && native == face
    ));
    assert!(matches!(
        &decoded.ir.model.features[5].definition,
        FeatureDefinition::Sweep {
            profile: Some(ProfileRef::Faces(faces)),
            path: Some(PathRef::Edges(edges)),
            ..
        } if faces == std::slice::from_ref(&face_id) && edges == std::slice::from_ref(&edge_id)
    ));

    if let FeatureDefinition::Fillet { groups } = &mut decoded.ir.model.features[0].definition {
        groups[0].edges = EdgeSelection::Edges(vec![edge_id.clone()]);
    }
    if let FeatureDefinition::DeleteFace { faces, .. } =
        &mut decoded.ir.model.features[1].definition
    {
        *faces = FaceSelection::Faces(vec![face_id.clone()]);
    }
    if let FeatureDefinition::Combine { target, tools, .. } =
        &mut decoded.ir.model.features[2].definition
    {
        *target = BodySelection::Bodies(vec![body_id.clone()]);
        *tools = BodySelection::Bodies(vec![body_id.clone()]);
    }
    if let FeatureDefinition::Extrude {
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: Termination::ToFace { face, .. },
                        ..
                    },
            },
        ..
    } = &mut decoded.ir.model.features[3].definition
    {
        *face = FaceSelection::Faces(vec![face_id.clone()]);
    }
    if let FeatureDefinition::Hole { face, .. } = &mut decoded.ir.model.features[4].definition {
        *face = Some(FaceSelection::Faces(vec![face_id.clone()]));
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let records = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(records[0].properties["Edges"], edge_id.0);
    assert_eq!(records[1].properties["Faces"], face_id.0);
    assert_eq!(records[2].properties["Target"], body_id.0);
    assert_eq!(records[2].properties["Tools"], body_id.0);
    assert_eq!(records[3].properties["Face"], face_id.0);
    assert_eq!(records[3].properties["Profile"], face_id.0);
    assert_eq!(records[4].properties["Face"], face_id.0);
    assert_eq!(records[5].properties["Profile"], face_id.0);
    assert_eq!(records[5].properties["Path"], edge_id.0);
}

#[test]
fn semantic_writer_round_trips_feature_output_scope() {
    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));
    body.extend(owned_triangle(0, 700, 0.0));
    body.extend(owned_triangle(200, 701, 10.0));
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(base.ir.model.bodies.len(), 2);
    let scope = base.ir.model.bodies[0].id.0.clone();
    let mut source = sldprt_with_body(&body);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        format!(
            r#"<Keywords><Feature Name="Scoped" Type="Custom" id="1" Scope="{scope}"/></Keywords>"#
        )
        .as_bytes(),
    ));
    let source_partition = container::scan_bytes(&source)
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap()
        .payload
        .clone();
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded.ir.model.features[0].outputs,
        vec![decoded.ir.model.bodies[0].id.clone()]
    );
    decoded.ir.model.features[0].outputs = vec![decoded.ir.model.bodies[1].id.clone()];

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let written_partition = container::scan_bytes(&encoded)
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap()
        .payload
        .clone();
    assert_eq!(written_partition, source_partition);
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].properties["Scope"],
        regenerated.ir.model.bodies[1].id.0
    );
    assert_eq!(
        regenerated.ir.model.features[0].outputs,
        vec![regenerated.ir.model.bodies[1].id.clone()]
    );
}

#[test]
fn decode_reports_unresolved_feature_output_scope() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Scoped" Type="Custom" id="1" Scope="MissingBody"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.ir.model.features[0].outputs.is_empty());
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.message
            == "1 feature(s) retain non-empty native output scopes that do not resolve to model bodies."
    }));
}

#[test]
fn decode_projects_generic_extrusion_with_explicit_operation() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Generic" Type="Extrusion" id="10" Operation="NewBody"><Dimension Name="Depth">6mm</Dimension></Extrusion></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(6.0),
                    },
                    ..
                }
            },
            op: BooleanOp::NewBody,
            ..
        }
    ));
}

#[test]
fn decode_dispatches_typed_features_by_xml_family() {
    use cadmpeg_ir::features::{ChamferSpec, FeatureDefinition, HoleKind, Length, RadiusSpec};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Profile" Type="CustomSketch" id="51"/>
            <ReferencePoint Name="Origin" Type="CustomDatum" id="52" Position="1mm,2mm,3mm"/>
            <Fillet Name="Round" Type="CustomFillet" id="53" Dependencies="51,52,51" Algorithm="RollingBall"><Dimension Name="Radius">2mm</Dimension></Fillet>
            <Chamfer Name="Bevel" Type="CustomChamfer" id="54"><Dimension Name="Distance">3mm</Dimension></Chamfer>
            <Hole Name="Drill" Type="CustomHole" id="55"><Dimension Name="Diameter">4mm</Dimension><Dimension Name="Depth">5mm</Dimension></Hole>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Sketch { .. }
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::DatumPoint { .. }
    ));
    assert!(matches!(
        &decoded.ir.model.features[2].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: Length(2.0),
            },
            ..
        }])
    ));
    assert_eq!(
        decoded.ir.model.features[2].dependencies,
        vec![
            decoded.ir.model.features[0].id.clone(),
            decoded.ir.model.features[1].id.clone(),
        ]
    );
    assert_eq!(
        decoded.ir.model.features[2].source_properties["Algorithm"],
        "RollingBall"
    );
    assert!(matches!(
        &decoded.ir.model.features[3].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::Distance {
                distance: Length(3.0),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir.model.features[4].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Simple,
            diameter: Some(Length(4.0)),
            ..
        }
    ));

    let FeatureDefinition::Fillet { groups } = &mut decoded.ir.model.features[2].definition else {
        panic!("typed custom fillet");
    };
    let RadiusSpec::Constant { radius } = &mut groups[0].radius else {
        panic!("constant fillet");
    };
    *radius = Length(2.5);
    decoded.ir.model.features[2]
        .source_properties
        .insert("Algorithm".into(), "FaceBlend".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[2].kind, "CustomFillet");
    assert_eq!(native[2].parameters["Radius"], "2.5mm");
    assert_eq!(native[2].properties["Algorithm"], "FaceBlend");
    assert_eq!(
        regenerated.ir.model.features[2].source_properties["Algorithm"],
        "FaceBlend"
    );
    assert_eq!(
        regenerated.ir.model.features[2].dependencies,
        vec![
            regenerated.ir.model.features[0].id.clone(),
            regenerated.ir.model.features[1].id.clone(),
        ]
    );
    regenerated.ir.model.features[2].dependencies.pop();
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &regenerated.ir,
            &regenerated.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("dependencies are inconsistent with its operands"));
}

#[test]
fn semantic_writer_round_trips_all_extrusion_forms() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ProfileRef,
        Termination,
    };
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="30"/><Extrusion Name="Blind" Type="BossExtrude" id="31" Profile="30" EndCondition="Blind" Operation="Join"><Dimension Name="Depth">2mm</Dimension></Extrusion><Extrusion Name="Symmetric" Type="BossExtrude" id="32" Profile="30" EndCondition="Symmetric" Direction="0,0,1" Operation="NewBody"><Dimension Name="Depth">4mm</Dimension><Dimension Name="Draft">5deg</Dimension></Extrusion><Extrusion Name="Two" Type="CutExtrude" id="33" Profile="30" EndCondition="TwoSided" Operation="Cut"><Dimension Name="Depth">3mm</Dimension><Dimension Name="Depth2">7mm</Dimension></Extrusion><Extrusion Name="Through" Type="CutExtrude" id="34" Profile="30" EndCondition="ThroughAll" Direction="0,1,0" Operation="Cut"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile_feature = decoded.ir.model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile),
            direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind { length: Length(2.0) },
                    draft: None,
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        } if profile == &profile_feature
    ));
    assert!(matches!(
        decoded.ir.model.features[2].definition,
        FeatureDefinition::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3 { x: 0.0, y: 0.0, z: 1.0 }),
            extent: ExtrudeExtent::Symmetric {
                side: ExtrudeSide {
                    termination: Termination::Blind { length: Length(4.0) },
                    draft: Some(Angle(value)),
                    ..
                }
            },
            op: BooleanOp::NewBody,
            ..
        } if (value - 5f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        decoded.ir.model.features[3].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::TwoSided {
                first: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(3.0)
                    },
                    ..
                },
                second: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(7.0)
                    },
                    ..
                },
            },
            op: BooleanOp::Cut,
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[4].definition,
        FeatureDefinition::Extrude {
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ThroughAll,
                    ..
                }
            },
            ..
        }
    ));

    let FeatureDefinition::Extrude {
        direction,
        extent,
        op,
        ..
    } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed extrusion");
    };
    *direction = cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3::new(1.0, 0.0, 0.0));
    *extent = ExtrudeExtent::TwoSided {
        first: ExtrudeSide {
            termination: Termination::Blind {
                length: Length(8.0),
            },
            draft: Some(Angle(0.1)),
            offset: None,
        },
        second: ExtrudeSide {
            termination: Termination::Blind {
                length: Length(9.0),
            },
            draft: None,
            offset: None,
        },
    };
    *op = BooleanOp::Intersect;
    let FeatureDefinition::Extrude {
        direction, extent, ..
    } = &mut decoded.ir.model.features[3].definition
    else {
        panic!("typed extrusion");
    };
    *direction = cadmpeg_ir::features::ExtrudeDirection::ProfileNormal;
    *extent = ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: Termination::ThroughAll,
            draft: None,
            offset: None,
        },
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[1].properties["EndCondition"], "TwoSided");
    assert_eq!(native[1].properties["Direction"], "1,0,0");
    assert_eq!(native[1].properties["Operation"], "Intersect");
    assert_eq!(native[1].parameters["Depth"], "8mm");
    assert_eq!(native[1].parameters["Depth2"], "9mm");
    assert_eq!(native[1].parameters["Draft"], "0.1rad");
    assert_eq!(native[3].properties["EndCondition"], "ThroughAll");
    assert!(!native[3].parameters.contains_key("Depth"));
    assert!(!native[3].parameters.contains_key("Depth2"));
    assert!(!native[3].properties.contains_key("Direction"));
}

#[test]
fn semantic_writer_round_trips_extrusion_to_face() {
    use cadmpeg_ir::features::{
        ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="30"/><Extrusion Name="UpTo" Type="BossExtrude" id="31" Profile="30" EndCondition="ToFace" Face="face:12" Operation="Join"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let FeatureDefinition::Extrude { extent, .. } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed extrusion");
    };
    assert_eq!(
        extent,
        &ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::ToFace {
                    face: FaceSelection::Native("face:12".into()),
                    offset: None,
                },
                draft: None,
                offset: None,
            }
        }
    );
    *extent = ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: Termination::ToFace {
                face: FaceSelection::Native("face:13".into()),
                offset: None,
            },
            draft: None,
            offset: None,
        },
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[1];
    assert_eq!(native.properties["EndCondition"], "ToFace");
    assert_eq!(native.properties["Face"], "face:13");
    assert!(matches!(
        &regenerated.ir.model.features[1].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::ToFace {
                        face: FaceSelection::Native(face),
                        ..
                    },
                    ..
                }
            },
            ..
        } if face == "face:13"
    ));
}

#[test]
fn semantic_writer_retains_unresolved_native_edge_treatments() {
    use cadmpeg_ir::features::{
        ChamferForm, ChamferSpec, FeatureDefinition, RadiusForm, RadiusSpec,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Round" Type="Custom" id="10" Edges="edge:1"><Dimension Name="Radius">NaNmm</Dimension></Feature><Feature Name="Bevel" Type="Custom" id="11" Edges="edge:2"><Dimension Name="Distance">NaNmm</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("Fillet_c", "Round", 10),
            ("Chamfer_c", "Bevel", 11),
        ]),
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Unresolved {
                form: Some(RadiusForm::Constant),
            },
            ..
        }])
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::Unresolved {
                form: Some(ChamferForm::Distance),
            },
            ..
        }])
    ));

    let mut detached = decoded.ir.clone();
    detached.model.features[0].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("unresolved fillet radius law"));
    detached.model.features[0] = decoded.ir.model.features[0].clone();
    detached.model.features[1].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("unresolved chamfer dimensions"));

    decoded.ir.model.features[0].name = Some("Renamed round".into());
    decoded.ir.model.features[1].name = Some("Renamed bevel".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].parameters["Radius"], "NaNmm");
    assert_eq!(native[1].parameters["Distance"], "NaNmm");
    assert_eq!(native[0].properties["Edges"], "edge:1");
    assert_eq!(native[1].properties["Edges"], "edge:2");
}

#[test]
fn semantic_writer_round_trips_typed_fillet_radius() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round" Type="Fillet" id="10" Edges="edge:1,edge:2"><Dimension Name="Radius">2mm</Dimension></Fillet></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Native(selection),
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(2.0),
            },
            ..
        }] if selection == "edge:1,edge:2")
    ));

    let cadmpeg_ir::features::FeatureDefinition::Fillet { groups } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed fillet feature");
    };
    groups[0].radius = cadmpeg_ir::features::RadiusSpec::Constant {
        radius: cadmpeg_ir::features::Length(3.5),
    };
    groups[0].edges = cadmpeg_ir::features::EdgeSelection::Native("edge:3".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].parameters["Radius"],
        "3.5mm"
    );
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].properties["Edges"],
        "edge:3"
    );
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(3.5),
            },
            ..
        }])
    ));
}

#[test]
fn semantic_writer_round_trips_positional_fillet_and_localized_chamfer_dimensions() {
    use cadmpeg_ir::features::{
        Angle, ChamferSpec, EdgeSelection, FeatureDefinition, Length, ParameterValue, RadiusSpec,
    };

    let keywords = format!(
        r#"<Keywords>
            <Feature Name="Round" Type="Fillet" id="10"><Dimension Name="D1">R1</Dimension></Feature>
            <Feature Name="Bevel" Type="Chafl{acute}n" id="11"><Dimension Name="D1">0.3</Dimension><Dimension Name="D2">45.00{degree}</Dimension></Feature>
        </Keywords>"#,
        acute = '\u{00e1}',
        degree = '\u{00b0}',
    );
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", keywords.as_bytes()));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("Fillet_c", "Round", 10),
            ("Chamfer_c", "Bevel", 11),
        ]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Unresolved,
            radius: RadiusSpec::Constant {
                radius: Length(1.0)
            }, ..
        }])
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: EdgeSelection::Unresolved,
            spec: ChamferSpec::DistanceAngle {
                distance: Length(0.3),
                angle: Angle(angle),
            },
        }] if (angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12)
    ));
    assert_eq!(
        decoded.ir.model.parameters[1].value,
        Some(ParameterValue::Length(Length(0.3)))
    );

    let FeatureDefinition::Fillet { groups, .. } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed positional fillet");
    };
    groups[0].radius = RadiusSpec::Constant {
        radius: Length(2.5),
    };
    let FeatureDefinition::Chamfer { groups, .. } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed positional chamfer");
    };
    groups[0].spec = ChamferSpec::DistanceAngle {
        distance: Length(0.6),
        angle: Angle(30.0_f64.to_radians()),
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].parameters["D1"], "R2.5");
    assert!(!native[0].parameters.contains_key("Radius"));
    assert_eq!(native[1].kind, format!("Chafl{}n", '\u{00e1}'));
    assert_eq!(native[1].parameters["D1"], "0.6");
    assert_eq!(native[1].parameters["D2"], format!("30{}", '\u{00b0}'));
    assert!(!native[1].parameters.contains_key("Distance"));
    assert!(!native[1].parameters.contains_key("Angle"));
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: Length(2.5)
            },
            ..
        }])
    ));
    assert!(matches!(
        &regenerated.ir.model.features[1].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::DistanceAngle {
                distance: Length(0.6),
                angle: Angle(angle),
            },
            ..
        }] if (angle - 30.0_f64.to_radians()).abs() < 1e-12)
    ));
}

#[test]
fn semantic_writer_round_trips_variable_radius_fillet() {
    use cadmpeg_ir::features::{
        EdgeSelection, FeatureDefinition, Length, RadiusSpec, VariableRadius,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Blend" Type="Fillet" id="61"><Dimension Name="Position0">0</Dimension><Dimension Name="Radius0">2mm</Dimension><Dimension Name="Position1">0.5</Dimension><Dimension Name="Radius1">4mm</Dimension><Dimension Name="Position2">1</Dimension><Dimension Name="Radius2">3mm</Dimension></Fillet></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Fillet {
            groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            edges: EdgeSelection::Unresolved,
            radius: RadiusSpec::Variable { points }, ..
        }] if points == &vec![
            VariableRadius { parameter: 0.0, radius: Length(2.0) },
            VariableRadius { parameter: 0.5, radius: Length(4.0) },
            VariableRadius { parameter: 1.0, radius: Length(3.0) },
        ])
    ));
    let FeatureDefinition::Fillet { groups } = &mut decoded.ir.model.features[0].definition else {
        panic!("variable fillet");
    };
    let RadiusSpec::Variable { points } = &mut groups[0].radius else {
        panic!("variable fillet radius")
    };
    points[1].parameter = 0.4;
    points[1].radius = Length(5.0);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&regenerated.ir);
    assert_eq!(
        native.feature_histories[0].features[0].parameters["Position1"],
        "0.4"
    );
    assert_eq!(
        native.feature_histories[0].features[0].parameters["Radius1"],
        "5mm"
    );

    let FeatureDefinition::Fillet { groups, .. } = &mut regenerated.ir.model.features[0].definition
    else {
        panic!("variable fillet after regeneration");
    };
    groups[0].radius = RadiusSpec::Constant {
        radius: Length(6.0),
    };
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &regenerated.ir,
            &regenerated.source_fidelity,
            &mut encoded,
        )
        .unwrap();
    let final_ir = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameters = &sldprt_native(&final_ir.ir).feature_histories[0].features[0].parameters;
    assert_eq!(parameters["Radius"], "6mm");
    assert!(!parameters.keys().any(|name| name.starts_with("Position")));
    assert!(!parameters.keys().any(|name| name == "Radius0"));
}

#[test]
fn semantic_writer_round_trips_all_typed_chamfer_forms() {
    use cadmpeg_ir::features::{ChamferSpec, EdgeSelection, FeatureDefinition, Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Chamfer Name="Equal" Type="Chamfer" id="11" Edges="edge:1"><Dimension Name="Distance">2mm</Dimension></Chamfer>
            <Chamfer Name="Unequal" Type="Chamfer" id="12" Edges="edge:2"><Dimension Name="Distance1">3mm</Dimension><Dimension Name="Distance2">0.25in</Dimension></Chamfer>
            <Chamfer Name="Angled" Type="Chamfer" id="13" Edges="edge:3"><Dimension Name="Distance">4mm</Dimension><Dimension Name="Angle">45deg</Dimension></Chamfer>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: EdgeSelection::Native(edges),
            spec: ChamferSpec::Distance {
                distance: Length(2.0),
            },
            ..
        }] if edges == "edge:1")
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::TwoDistances {
                first: Length(3.0),
                second: Length(6.35),
            },
            ..
        }])
    ));
    assert!(matches!(
        &decoded.ir.model.features[2].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: ChamferSpec::DistanceAngle {
                distance: Length(4.0),
                angle,
            },
            ..
        }] if (angle.0 - std::f64::consts::FRAC_PI_4).abs() < 1e-12)
    ));

    let replacements = [
        ChamferSpec::Distance {
            distance: Length(2.5),
        },
        ChamferSpec::TwoDistances {
            first: Length(3.5),
            second: Length(7.0),
        },
        ChamferSpec::DistanceAngle {
            distance: Length(4.5),
            angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_6),
        },
    ];
    for (index, (feature, replacement)) in decoded
        .ir
        .model
        .features
        .iter_mut()
        .zip(replacements)
        .enumerate()
    {
        let FeatureDefinition::Chamfer { groups, .. } = &mut feature.definition else {
            panic!("typed chamfer feature");
        };
        groups[0].spec = replacement;
        groups[0].edges = EdgeSelection::Native(format!("edge:{}", index + 4));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(features[0].parameters["Distance"], "2.5mm");
    assert_eq!(features[0].properties["Edges"], "edge:4");
    assert_eq!(features[1].properties["Edges"], "edge:5");
    assert_eq!(features[2].properties["Edges"], "edge:6");
    assert_eq!(features[1].parameters["Distance1"], "3.5mm");
    assert_eq!(features[1].parameters["Distance2"], "7mm");
    assert_eq!(
        features[2].parameters["Angle"],
        format!("{}rad", std::f64::consts::FRAC_PI_6)
    );
}

#[test]
fn semantic_writer_retains_partial_native_wall_operations() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Shell Name="Unknown shell" Type="Shell" id="14" RemovedFaces="face:1"><Dimension Name="Thickness">NaNmm</Dimension></Shell><Thicken Name="Unknown thicken" Type="Thicken" id="15" Faces="face:2" BothSides="invalid"><Dimension Name="Thickness">NaNmm</Dimension></Thicken></Keywords>"#,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Shell {
            removed_faces: FaceSelection::Native(faces),
            thickness: None,
            outward: None,
            ..
        } if faces == "face:1"
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Native(faces),
            thickness: None,
            side: None,
        } if faces == "face:2"
    ));

    let mut detached = decoded.ir.clone();
    detached.model.features[0].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("unresolved shell construction"));
    detached.model.features[0] = decoded.ir.model.features[0].clone();
    detached.model.features[1].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("unresolved thicken construction"));

    decoded.ir.model.features[0].name = Some("Renamed shell".into());
    decoded.ir.model.features[1].name = Some("Renamed thicken".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].parameters["Thickness"], "NaNmm");
    assert!(!native[0].properties.contains_key("Outward"));
    assert_eq!(native[1].parameters["Thickness"], "NaNmm");
    assert_eq!(native[1].properties["BothSides"], "invalid");
}

#[test]
fn semantic_writer_round_trips_typed_shell() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Shell Name="Thin" Type="Shell" id="14" RemovedFaces="face:4" Outward="false"><Dimension Name="Thickness">0.08in</Dimension></Shell></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Shell {
            removed_faces: FaceSelection::Native(selection),
            thickness: Some(Length(value)),
            outward: Some(false),
            ..
        } if selection == "face:4" && (*value - 2.032).abs() < 1e-12
    ));

    let FeatureDefinition::Shell {
        removed_faces,
        thickness,
        outward,
        ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed shell feature");
    };
    *thickness = Some(Length(3.0));
    *outward = Some(true);
    *removed_faces = FaceSelection::Native("face:5,face:6".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Thickness"], "3mm");
    assert_eq!(feature.properties["RemovedFaces"], "face:5,face:6");
    assert_eq!(feature.properties["Outward"], "true");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::Shell {
            thickness: Some(Length(3.0)),
            outward: Some(true),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_typed_thicken() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Thicken Name="Wall" Type="Thicken" id="15" Faces="face:4" BothSides="false" Reverse="true"><Dimension Name="Thickness">0.08in</Dimension></Thicken></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Native(selection),
            thickness: Some(Length(value)),
            side: Some(ThickenSide::Reverse),
        } if selection == "face:4" && (*value - 2.032).abs() < 1e-12
    ));

    let FeatureDefinition::Thicken {
        faces,
        thickness,
        side,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed thicken feature");
    };
    *faces = FaceSelection::Native("face:5,face:6".into());
    *thickness = Some(Length(3.0));
    *side = Some(ThickenSide::Both);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Thickness"], "3mm");
    assert_eq!(feature.properties["Faces"], "face:5,face:6");
    assert_eq!(feature.properties["BothSides"], "true");
    assert_eq!(feature.properties["Reverse"], "false");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::Thicken {
            thickness: Some(Length(3.0)),
            side: Some(ThickenSide::Both),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_positional_thicken_dimension() {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, Length, ParameterValue, ThickenSide,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Wall" Type="Thicken" id="15"><Dimension Name="D1">6</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moThicken_c", "Wall", 15)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: Some(Length(6.0)),
            side: Some(ThickenSide::Forward),
        }
    ));
    assert_eq!(
        decoded.ir.model.parameters[0].value,
        Some(ParameterValue::Length(Length(6.0)))
    );

    let FeatureDefinition::Thicken { thickness, .. } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed positional thicken");
    };
    *thickness = Some(Length(8.5));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.parameters["D1"], "8.5");
    assert!(!native.parameters.contains_key("Thickness"));
    assert!(!native.properties.contains_key("BothSides"));
    assert!(!native.properties.contains_key("Reverse"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Thicken {
            thickness: Some(Length(8.5)),
            side: Some(ThickenSide::Forward),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_typed_scale() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, ScaleCenter, ScaleFactors};
    use cadmpeg_ir::math::Point3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Scale Name="Point" Type="Scale" id="16" Bodies="body:1" Center="1mm,2mm,3mm"><Dimension Name="Factor">2</Dimension></Scale>
            <Scale Name="Centroid" Type="Scale" id="17" Bodies="body:1" CenterType="Centroid"><Dimension Name="Factor">1.1</Dimension></Scale>
            <Scale Name="Origin" Type="Scale" id="18" Bodies="body:1" CenterType="Origin"><Dimension Name="Factor">1.2</Dimension></Scale>
            <Scale Name="Reference" Type="Scale" id="19" Bodies="body:1" CenterType="CoordinateSystem" CenterRef="csys:4"><Dimension Name="Factor">1.3</Dimension></Scale>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Scale {
            bodies: BodySelection::Native(selection),
            center: Some(ScaleCenter::Point(Point3 { x: 1.0, y: 2.0, z: 3.0 })),
            factors: ScaleFactors {
                uniform: Some(2.0),
                x: None,
                y: None,
                z: None,
            },
        } if selection == "body:1"
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Centroid),
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[2].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::ModelOrigin),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir.model.features[3].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Native(reference)),
            ..
        } if reference == "csys:4"
    ));

    let FeatureDefinition::Scale {
        bodies,
        center,
        factors,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed scale feature");
    };
    *bodies = BodySelection::Native("body:2,body:3".into());
    *center = Some(ScaleCenter::Point(Point3::new(4.0, 5.0, 6.0)));
    *factors = ScaleFactors {
        uniform: None,
        x: Some(1.5),
        y: Some(2.0),
        z: Some(2.5),
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.properties["Bodies"], "body:2,body:3");
    assert_eq!(feature.properties["Center"], "4mm,5mm,6mm");
    assert!(!feature.parameters.contains_key("Factor"));
    assert_eq!(feature.parameters["ScaleX"], "1.5");
    assert_eq!(feature.parameters["ScaleY"], "2");
    assert_eq!(feature.parameters["ScaleZ"], "2.5");
    let native_features = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native_features[1].properties["CenterType"], "Centroid");
    assert!(!native_features[1].properties.contains_key("Center"));
    assert_eq!(native_features[2].properties["CenterType"], "ModelOrigin");
    assert!(!native_features[2].properties.contains_key("Center"));
    assert_eq!(native_features[3].properties["CenterType"], "Reference");
    assert_eq!(native_features[3].properties["CenterRef"], "csys:4");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Point(Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            })),
            factors: ScaleFactors {
                uniform: None,
                x: Some(1.5),
                y: Some(2.0),
                z: Some(2.5),
            },
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir.model.features[1].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Centroid),
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir.model.features[2].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::ModelOrigin),
            ..
        }
    ));
    assert!(matches!(
        &regenerated.ir.model.features[3].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Native(reference)),
            ..
        } if reference == "csys:4"
    ));
}

#[test]
fn semantic_writer_retains_partial_native_scale_construction() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, ScaleCenter, ScaleFactors};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Scale Name="Unknown center" Type="Scale" id="71" Bodies="body:1" CenterType="Point" Center="invalid"><Dimension Name="Factor">2</Dimension><Dimension Name="ScaleX">3</Dimension></Scale>
            <Scale Name="Partial axes" Type="Scale" id="72" CenterType="Centroid"><Dimension Name="Factor">0</Dimension><Dimension Name="ScaleX">1.5</Dimension><Dimension Name="ScaleY">NaN</Dimension><Dimension Name="ScaleZ">2.5</Dimension></Scale>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Scale {
            bodies: BodySelection::Native(bodies),
            center: None,
            factors: ScaleFactors {
                uniform: Some(2.0),
                x: Some(3.0),
                y: None,
                z: None,
            },
        } if bodies == "body:1"
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::Scale {
            bodies: BodySelection::Unresolved,
            center: Some(ScaleCenter::Centroid),
            factors: ScaleFactors {
                uniform: None,
                x: Some(1.5),
                y: None,
                z: Some(2.5),
            },
        }
    ));

    for index in 0..2 {
        let mut detached = decoded.ir.clone();
        detached.model.features[index].native_ref = None;
        let error = SldprtCodec
            .write_preserved_with_source_fidelity(
                &detached,
                &decoded.source_fidelity,
                &mut Vec::new(),
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("unresolved scale construction"),
            "{error}"
        );
    }

    for (index, feature) in decoded.ir.model.features.iter_mut().enumerate() {
        feature.name = Some(format!("Renamed scale {}", index + 1));
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].properties["Center"], "invalid");
    assert_eq!(native[0].parameters["Factor"], "2");
    assert_eq!(native[0].parameters["ScaleX"], "3");
    assert_eq!(native[1].parameters["Factor"], "0");
    assert_eq!(native[1].parameters["ScaleY"], "NaN");
    assert_eq!(native[1].parameters["ScaleX"], "1.5");
    assert_eq!(native[1].parameters["ScaleZ"], "2.5");
}

#[test]
fn semantic_writer_round_trips_typed_draft() {
    use cadmpeg_ir::features::{Angle, FaceSelection, FeatureDefinition};
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Draft Name="Taper" Type="Draft" id="18" Faces="face:1,face:2" NeutralPlane="face:3" Direction="0,0,1" Outward="false"><Dimension Name="Angle">3deg</Dimension></Draft></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Draft {
            faces: FaceSelection::Native(faces),
            neutral_plane: FaceSelection::Native(neutral_plane),
            pull_direction: Some(Vector3 { x: 0.0, y: 0.0, z: 1.0 }),
            angle: Some(Angle(value)),
            outward: Some(false),
        } if faces == "face:1,face:2"
            && neutral_plane == "face:3"
            && (*value - 3f64.to_radians()).abs() < 1e-12
    ));

    let FeatureDefinition::Draft {
        faces,
        neutral_plane,
        pull_direction,
        angle,
        outward,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed draft");
    };
    *pull_direction = Some(Vector3::new(0.0, 1.0, 0.0));
    *angle = Some(Angle(7f64.to_radians()));
    *outward = Some(true);
    *faces = FaceSelection::Native("face:4".into());
    *neutral_plane = FaceSelection::Native("face:5".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:4");
    assert_eq!(feature.properties["NeutralPlane"], "face:5");
    assert_eq!(feature.properties["Direction"], "0,1,0");
    assert_eq!(feature.properties["Outward"], "true");
    assert_eq!(
        feature.parameters["Angle"],
        format!("{}rad", 7f64.to_radians())
    );
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Draft {
            pull_direction: Some(Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }),
            outward: Some(true),
            ..
        }
    ));
}

#[test]
fn semantic_writer_preserves_absent_feature_selections() {
    use cadmpeg_ir::features::{
        Angle, ChamferSpec, EdgeSelection, FaceSelection, FeatureDefinition, Length,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Chamfer Name="Bevel" Type="Chamfer" id="31"><Dimension Name="Distance">2mm</Dimension></Chamfer>
            <Shell Name="Thin" Type="Shell" id="32" Outward="false"><Dimension Name="Thickness">1mm</Dimension></Shell>
            <Draft Name="Taper" Type="Draft" id="33" Direction="0,0,1" Outward="false"><Dimension Name="Angle">3deg</Dimension></Draft>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: EdgeSelection::Unresolved, ..
        }])
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Shell {
            removed_faces: FaceSelection::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir.model.features[2].definition,
        FeatureDefinition::Draft {
            faces: FaceSelection::Unresolved,
            neutral_plane: FaceSelection::Unresolved,
            ..
        }
    ));

    let FeatureDefinition::Chamfer { groups, .. } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed chamfer");
    };
    groups[0].spec = ChamferSpec::Distance {
        distance: Length(2.5),
    };
    let FeatureDefinition::Shell { thickness, .. } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed shell");
    };
    *thickness = Some(Length(1.5));
    let FeatureDefinition::Draft { angle, .. } = &mut decoded.ir.model.features[2].definition
    else {
        panic!("typed draft");
    };
    *angle = Some(Angle(5f64.to_radians()));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(features[0].parameters["Distance"], "2.5mm");
    assert!(!features[0].properties.contains_key("Edges"));
    assert_eq!(features[1].parameters["Thickness"], "1.5mm");
    assert!(!features[1].properties.contains_key("RemovedFaces"));
    assert_eq!(
        features[2].parameters["Angle"],
        format!("{}rad", 5f64.to_radians())
    );
    assert!(!features[2].properties.contains_key("Faces"));
    assert!(!features[2].properties.contains_key("NeutralPlane"));
}

#[test]
fn semantic_writer_round_trips_typed_combine() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Combine Name="Union" Type="Combine" id="19" Target="body:1" Tools="body:2,body:3" Operation="Join"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            op: BooleanOp::Join,
        } if target == "body:1" && tools == "body:2,body:3"
    ));

    let FeatureDefinition::Combine { target, tools, op } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed combine");
    };
    *target = BodySelection::Native("body:4".into());
    *tools = BodySelection::Native("body:5,body:6".into());
    *op = BooleanOp::Intersect;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.properties["Target"], "body:4");
    assert_eq!(feature.properties["Tools"], "body:5,body:6");
    assert_eq!(feature.properties["Operation"], "Intersect");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            op: BooleanOp::Intersect,
        } if target == "body:4" && tools == "body:5,body:6"
    ));
}

#[test]
fn decode_projects_compact_combine_with_unresolved_semantics() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Compact" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moCombineBodies_c", "Compact", 119)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            op: BooleanOp::Unresolved,
        }
    ));

    decoded.ir.model.features[0].name = Some("Renamed compact combine".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            op: BooleanOp::Unresolved,
        }
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_combine_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

    fn append_body_path(payload: &mut Vec<u8>, local_id: u32) {
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&[0, 3, 0, 0]);
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[
            0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49,
            0xb2, 0x54,
        ]);
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&[0x32, 0x80, 0, 0]);
        payload.extend_from_slice(&[1; 12]);
        payload.extend_from_slice(&local_id.to_le_bytes());
    }

    fn combine_payload(has_selection: bool) -> Vec<u8> {
        let mut payload =
            resolved_feature_classes_with_ids(&[("moCombineBodies_c", "Combine", 119)]);
        if has_selection {
            append_body_path(&mut payload, 6);
            append_body_path(&mut payload, 7);
        }
        payload
    }

    let resolved_selection = combine_payload(true);
    assert_eq!(
        (12..resolved_selection.len())
            .filter(|offset| crate::resolved_features::compact_body_path_at(
                &resolved_selection,
                *offset
            )
            .is_some())
            .count(),
        2
    );

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Combine" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_selection,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &combine_payload(false),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));
    let feature_id = decoded.ir.model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        } if target.starts_with("sldprt:feature-input:body-path:")
            && tools.starts_with("sldprt:feature-input:body-path:")
    ));
    assert!(matches!(
        decoded.ir.model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));

    let mut source = outer_header();
    source.extend(make_block(
        0x20,
        "Contents/Config-1-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body()),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Combine" Type="Localized" id="119"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"><swModel swName="Part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &combine_payload(false),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_selection,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature_id = decoded.ir.model.features[0].id.clone();
    assert!(decoded.ir.model.configurations[0].active);
    assert_eq!(decoded.ir.model.configurations[0].source_index, Some(1));
    assert!(matches!(
        &decoded.ir.model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir.model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        } if target.starts_with("sldprt:feature-input:body-path:")
            && tools.starts_with("sldprt:feature-input:body-path:")
    ));
}

#[test]
fn semantic_writer_round_trips_delete_and_keep_body() {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <DeleteBody Name="Discard" Type="DeleteBody" id="20" Bodies="body:2,body:3"/>
            <KeepBody Name="Isolate" Type="KeepBody" id="21" Bodies="body:1"/>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Native(bodies),
            mode: BodyRetentionMode::DeleteSelected,
        } if bodies == "body:2,body:3"
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Native(bodies),
            mode: BodyRetentionMode::KeepSelected,
        } if bodies == "body:1"
    ));

    let FeatureDefinition::DeleteBody { bodies, .. } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed delete body");
    };
    *bodies = BodySelection::Native("body:4".into());
    let FeatureDefinition::DeleteBody { bodies, .. } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed keep body");
    };
    *bodies = BodySelection::Native("body:5,body:6".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(features[0].properties["Bodies"], "body:4");
    assert_eq!(features[0].properties["Mode"], "Delete");
    assert_eq!(features[1].properties["Bodies"], "body:5,body:6");
    assert_eq!(features[1].properties["Mode"], "Keep");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::DeleteSelected,
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir.model.features[1].definition,
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::KeepSelected,
            ..
        }
    ));
}

#[test]
fn semantic_writer_resolves_sparse_body_delete_keep_operation() {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Body-Delete/Keep 1" Type="Body-Delete/Keep " id="20"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        }
    ));

    decoded.ir.model.features[0].name = Some("Retained sparse operation".into());
    let mut sparse_encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut sparse_encoded,
        )
        .unwrap();
    let mut sparse = SldprtCodec
        .decode(&mut Cursor::new(sparse_encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&sparse.ir).feature_histories[0].features[0];
    assert_eq!(native.kind, "Body-Delete/Keep ");
    assert!(!native.properties.contains_key("Bodies"));
    assert!(!native.properties.contains_key("Mode"));
    assert!(matches!(
        sparse.ir.model.features[0].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        }
    ));

    let FeatureDefinition::DeleteBody { bodies, mode } =
        &mut sparse.ir.model.features[0].definition
    else {
        panic!("typed sparse body operation");
    };
    *bodies = BodySelection::Native("body:2,body:3".into());
    *mode = BodyRetentionMode::KeepSelected;
    let mut resolved_encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &sparse.ir,
            &sparse.source_fidelity,
            &mut resolved_encoded,
        )
        .unwrap();
    let resolved = SldprtCodec
        .decode(
            &mut Cursor::new(resolved_encoded),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = &sldprt_native(&resolved.ir).feature_histories[0].features[0];
    assert_eq!(native.kind, "Body-Delete/Keep ");
    assert_eq!(native.properties["Bodies"], "body:2,body:3");
    assert_eq!(native.properties["Mode"], "Keep");
    assert!(matches!(
        &resolved.ir.model.features[0].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Native(bodies),
            mode: BodyRetentionMode::KeepSelected,
        } if bodies == "body:2,body:3"
    ));
}

#[test]
fn semantic_writer_round_trips_typed_delete_face() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><DeleteFace Name="Remove Boss" Type="DeleteFace" id="20" Faces="face:4,face:5" Heal="true"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Native(faces),
            heal: true,
        } if faces == "face:4,face:5"
    ));

    let FeatureDefinition::DeleteFace { faces, heal } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed delete face");
    };
    *faces = FaceSelection::Native("face:7".into());
    *heal = false;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:7");
    assert_eq!(feature.properties["Heal"], "false");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Native(faces),
            heal: false,
        } if faces == "face:7"
    ));
}

#[test]
fn semantic_writer_round_trips_typed_replace_face() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><ReplaceFace Name="Patch" Type="ReplaceFace" id="21" Faces="face:4,face:5" ReplacementFaces="face:8"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::ReplaceFace {
            targets: FaceSelection::Native(targets),
            replacements: FaceSelection::Native(replacements),
        } if targets == "face:4,face:5" && replacements == "face:8"
    ));

    let FeatureDefinition::ReplaceFace {
        targets,
        replacements,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed replace face");
    };
    *targets = FaceSelection::Native("face:6".into());
    *replacements = FaceSelection::Native("face:9,face:10".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:6");
    assert_eq!(feature.properties["ReplacementFaces"], "face:9,face:10");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::ReplaceFace {
            targets: FaceSelection::Native(targets),
            replacements: FaceSelection::Native(replacements),
        } if targets == "face:6" && replacements == "face:9,face:10"
    ));
}

#[test]
fn semantic_writer_round_trips_all_move_face_forms() {
    use cadmpeg_ir::features::{Angle, FaceMotion, FaceSelection, FeatureDefinition, Length};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><MoveFace Name="Offset" Type="MoveFace" id="21" Faces="face:1" Mode="Offset"><Dimension Name="Distance">2mm</Dimension></MoveFace><MoveFace Name="Translate" Type="MoveFace" id="22" Faces="face:2" Mode="Translate" Direction="1,0,0"><Dimension Name="Distance">3mm</Dimension></MoveFace><MoveFace Name="Rotate" Type="MoveFace" id="23" Faces="face:3" Mode="Rotate" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1"><Dimension Name="Angle">15deg</Dimension></MoveFace></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::MoveFace {
            motion: FaceMotion::Offset {
                distance: Length(2.0)
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::MoveFace {
            motion: FaceMotion::Translate {
                direction: Vector3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0
                },
                distance: Length(3.0),
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[2].definition,
        FeatureDefinition::MoveFace {
            motion: FaceMotion::Rotate {
                axis_origin: Point3 { x: 1.0, y: 2.0, z: 3.0 },
                axis_dir: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
                angle: Angle(value),
            },
            ..
        } if (value - 15f64.to_radians()).abs() < 1e-12
    ));

    let FeatureDefinition::MoveFace { faces, motion } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed move face");
    };
    *faces = FaceSelection::Native("face:8".into());
    *motion = FaceMotion::Translate {
        direction: Vector3::new(0.0, 1.0, 0.0),
        distance: Length(4.0),
    };
    let FeatureDefinition::MoveFace { motion, .. } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed move face");
    };
    *motion = FaceMotion::Rotate {
        axis_origin: Point3::new(0.0, 0.0, 0.0),
        axis_dir: Vector3::new(1.0, 0.0, 0.0),
        angle: Angle(0.5),
    };
    let FeatureDefinition::MoveFace { motion, .. } = &mut decoded.ir.model.features[2].definition
    else {
        panic!("typed move face");
    };
    *motion = FaceMotion::Offset {
        distance: Length(-1.0),
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].properties["Mode"], "Translate");
    assert_eq!(native[0].properties["Faces"], "face:8");
    assert_eq!(native[0].properties["Direction"], "0,1,0");
    assert_eq!(native[0].parameters["Distance"], "4mm");
    assert_eq!(native[1].properties["Mode"], "Rotate");
    assert_eq!(native[1].properties["AxisOrigin"], "0mm,0mm,0mm");
    assert_eq!(native[1].properties["AxisDirection"], "1,0,0");
    assert_eq!(native[1].parameters["Angle"], "0.5rad");
    assert_eq!(native[2].properties["Mode"], "Offset");
    assert_eq!(native[2].parameters["Distance"], "-1mm");
    assert!(!native[2].parameters.contains_key("Angle"));
}

#[test]
fn semantic_writer_round_trips_typed_dome() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Dome Name="Crown" Type="Dome" id="24" Faces="face:9" Elliptical="false" Reverse="false"><Dimension Name="Height">0.25in</Dimension></Dome></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native(faces),
            height: Some(Length(value)),
            elliptical: Some(false),
            reverse: Some(false),
        } if faces == "face:9" && (*value - 6.35).abs() < 1e-12
    ));

    let FeatureDefinition::Dome {
        faces,
        height,
        elliptical,
        reverse,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed dome");
    };
    *faces = FaceSelection::Native("face:10,face:11".into());
    *height = Some(Length(8.0));
    *elliptical = Some(true);
    *reverse = Some(true);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:10,face:11");
    assert_eq!(feature.properties["Elliptical"], "true");
    assert_eq!(feature.properties["Reverse"], "true");
    assert_eq!(feature.parameters["Height"], "8mm");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native(faces),
            height: Some(Length(8.0)),
            elliptical: Some(true),
            reverse: Some(true),
        } if faces == "face:10,face:11"
    ));
}

#[test]
fn semantic_writer_retains_partial_native_dome_construction() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Dome Name="Partial dome" Type="Dome" id="25" Faces="face:12" Elliptical="true" Reverse="invalid"><Dimension Name="Height">NaNmm</Dimension></Dome></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native(faces),
            height: None,
            elliptical: Some(true),
            reverse: None,
        } if faces == "face:12"
    ));

    let mut detached = decoded.ir.clone();
    detached.model.features[0].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("unresolved dome construction"));

    decoded.ir.model.features[0].name = Some("Renamed dome".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.parameters["Height"], "NaNmm");
    assert_eq!(native.properties["Reverse"], "invalid");
    assert_eq!(native.properties["Elliptical"], "true");
}

#[test]
fn semantic_writer_round_trips_principal_reference_planes() {
    use cadmpeg_ir::features::{FeatureDefinition, PrincipalPlane};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Vorne" Type="Ebene" id="2"/><Feature Name="Oben" Type="Ebene" id="3"/><Feature Name="Rechts" Type="Ebene" id="4"/><Feature Name="Plane2" Type="Plane" id="39"/><Feature Name="Reserved-shaped custom record" Type="Ebene" id="2" NativeRole="custom"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moRefPlane_c", "Vorne", 2),
            ("moRefPlane_c", "Oben", 3),
            ("moRefPlane_c", "Rechts", 4),
        ]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    for (feature, plane) in decoded.ir.model.features[..3].iter().zip([
        PrincipalPlane::Front,
        PrincipalPlane::Top,
        PrincipalPlane::Right,
    ]) {
        assert_eq!(
            feature.definition,
            FeatureDefinition::DatumPrincipalPlane { plane }
        );
    }
    assert!(matches!(
        &decoded.ir.model.features[3].definition,
        FeatureDefinition::Native { kind, .. } if kind == "Plane"
    ));
    assert!(matches!(
        &decoded.ir.model.features[4].definition,
        FeatureDefinition::Native { kind, .. } if kind == "Ebene"
    ));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        regenerated.ir.model.features[..3]
            .iter()
            .map(|feature| feature.definition.clone())
            .collect::<Vec<_>>(),
        vec![
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Front,
            },
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Top,
            },
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Right,
            },
        ]
    );
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].kind,
        "Ebene"
    );

    decoded.ir.model.features[0].definition = FeatureDefinition::DatumPrincipalPlane {
        plane: PrincipalPlane::Right,
    };
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("principal-plane role"));
}

#[test]
fn semantic_writer_round_trips_legacy_principal_plane_triplet() {
    use cadmpeg_ir::features::{FeatureDefinition, PrincipalPlane};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="A" Type="LocalizedPlane" id="2"/><Feature Name="B" Type="LocalizedPlane" id="3"/><Feature Name="C" Type="LocalizedPlane" id="4"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    for (feature, plane) in decoded.ir.model.features.iter().zip([
        PrincipalPlane::Front,
        PrincipalPlane::Top,
        PrincipalPlane::Right,
    ]) {
        assert_eq!(
            feature.definition,
            FeatureDefinition::DatumPrincipalPlane { plane }
        );
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.features, regenerated.ir.model.features);
}

#[test]
fn semantic_writer_round_trips_typed_reference_plane() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><ReferencePlane Name="Datum A" Type="ReferencePlane" id="25" Origin="1mm,2mm,3mm" Normal="0,0,1" UAxis="1,0,0"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::DatumPlane {
            origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            },
            normal: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            u_axis: Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
        }
    ));

    let FeatureDefinition::DatumPlane {
        origin,
        normal,
        u_axis,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed reference plane");
    };
    *origin = Point3::new(25.4, 0.0, -2.0);
    *normal = Vector3::new(0.0, 1.0, 0.0);
    *u_axis = Vector3::new(0.0, 0.0, 1.0);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.properties["Origin"], "25.4mm,0mm,-2mm");
    assert_eq!(feature.properties["Normal"], "0,1,0");
    assert_eq!(feature.properties["UAxis"], "0,0,1");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::DatumPlane {
            origin: Point3 {
                x: 25.4,
                y: 0.0,
                z: -2.0
            },
            normal: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
            u_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));
}

#[test]
fn semantic_writer_round_trips_sparse_localized_offset_plane() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plano2" Type="Plano" id="549"><Dimension Name="D1">3</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plano2", 549)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(3.0),
        }
    ));
    assert_eq!(
        decoded.ir.model.parameters[0].value,
        Some(ParameterValue::Length(Length(3.0)))
    );

    let FeatureDefinition::DatumOffsetPlane { distance, .. } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("localized offset plane");
    };
    *distance = Length(-4.5);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.kind, "Plano");
    assert_eq!(native.parameters["D1"], "-4.5");
    assert!(!native.properties.contains_key("Reference"));
    assert!(!native.properties.contains_key("Plane"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(-4.5),
        }
    ));
}

#[test]
fn decode_projects_fixed_reference_plane_frame() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut resolved = resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plano", 42)]);
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    resolved.extend_from_slice(&("moFixedRefPlnData_c".len() as u16).to_le_bytes());
    resolved.extend_from_slice(b"moFixedRefPlnData_c");
    let mut frame = [0u8; 97];
    frame[0..8].copy_from_slice(&2.5f64.to_le_bytes());
    frame[8..16].copy_from_slice(&(-0.25f64).to_le_bytes());
    frame[16..24].copy_from_slice(&1.5f64.to_le_bytes());
    frame[24..32].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[32..40].copy_from_slice(&0.0f64.to_le_bytes());
    frame[40..48].copy_from_slice(&0.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..57].copy_from_slice(&0.0f64.to_le_bytes());
    frame[57..65].copy_from_slice(&0.0f64.to_le_bytes());
    frame[65..73].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[73..81].copy_from_slice(&0.0f64.to_le_bytes());
    frame[81..89].copy_from_slice(&(-1.0f64).to_le_bytes());
    frame[89..97].copy_from_slice(&0.0f64.to_le_bytes());
    resolved.extend_from_slice(&frame);

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plano" Type="Plano" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::DatumPlane {
            origin: Point3 {
                x: 2500.0,
                y: -250.0,
                z: 1500.0,
            },
            normal: Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0,
            },
            u_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            },
        }
    ));
}

#[test]
fn decode_rejects_nonorthogonal_fixed_reference_plane_frame() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut resolved = resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plane", 42)]);
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    resolved.extend_from_slice(&("moFixedRefPlnData_c".len() as u16).to_le_bytes());
    resolved.extend_from_slice(b"moFixedRefPlnData_c");
    let mut frame = [0u8; 97];
    frame[24..32].copy_from_slice(&1.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..57].copy_from_slice(&1.0f64.to_le_bytes());
    frame[73..81].copy_from_slice(&1.0f64.to_le_bytes());
    resolved.extend_from_slice(&frame);

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plane" Type="Plane" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Native { .. }
    ));
}

#[test]
fn semantic_writer_round_trips_reference_axis_and_point() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><ReferenceAxis Name="Axis A" Type="ReferenceAxis" id="26" Origin="1mm,2mm,3mm" Direction="0,0,1"/><ReferencePoint Name="Point A" Type="ReferencePoint" id="27" Position="4mm,5mm,6mm"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::DatumAxis {
            origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            },
            direction: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::DatumPoint {
            position: Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            },
        }
    ));

    let FeatureDefinition::DatumAxis { origin, direction } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed reference axis");
    };
    *origin = Point3::new(-1.0, 0.0, 2.0);
    *direction = Vector3::new(0.0, 1.0, 0.0);
    let FeatureDefinition::DatumPoint { position } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed reference point");
    };
    *position = Point3::new(7.0, 8.0, 9.0);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].properties["Origin"], "-1mm,0mm,2mm");
    assert_eq!(native[0].properties["Direction"], "0,1,0");
    assert_eq!(native[1].properties["Position"], "7mm,8mm,9mm");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::DatumAxis {
            origin: Point3 {
                x: -1.0,
                y: 0.0,
                z: 2.0
            },
            direction: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
        }
    ));
    assert!(matches!(
        regenerated.ir.model.features[1].definition,
        FeatureDefinition::DatumPoint {
            position: Point3 {
                x: 7.0,
                y: 8.0,
                z: 9.0
            },
        }
    ));
}

#[test]
fn semantic_writer_round_trips_reference_coordinate_system() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><CoordinateSystem Name="Fixture" Type="ReferenceCoordinateSystem" id="28" Origin="1mm,2mm,3mm" XAxis="1,0,0" YAxis="0,1,0" ZAxis="0,0,1"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::DatumCoordinateSystem {
            origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            },
            x_axis: Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
            y_axis: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
            z_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));

    let FeatureDefinition::DatumCoordinateSystem {
        origin,
        x_axis,
        y_axis,
        z_axis,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed reference coordinate system");
    };
    *origin = Point3::new(4.0, 5.0, 6.0);
    *x_axis = Vector3::new(0.0, 1.0, 0.0);
    *y_axis = Vector3::new(-1.0, 0.0, 0.0);
    *z_axis = Vector3::new(0.0, 0.0, 1.0);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.xml_tag, "CoordinateSystem");
    assert_eq!(feature.kind, "ReferenceCoordinateSystem");
    assert_eq!(feature.properties["Origin"], "4mm,5mm,6mm");
    assert_eq!(feature.properties["XAxis"], "0,1,0");
    assert_eq!(feature.properties["YAxis"], "-1,0,0");
    assert_eq!(feature.properties["ZAxis"], "0,0,1");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::DatumCoordinateSystem {
            origin: Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            },
            x_axis: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
            y_axis: Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            },
            z_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));
}

#[test]
fn semantic_writer_round_trips_equation_driven_curve() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><EquationDrivenCurve Name="Spiral" Type="EquationDrivenCurve" id="29" Parameter="t" XEquation="10*cos(t)" YEquation="10*sin(t)" ZEquation="t" Start="0" End="6.283185307179586" Closed="false"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::EquationCurve {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start,
            end,
        } if parameter == "t"
            && x_expression == "10*cos(t)"
            && y_expression == "10*sin(t)"
            && z_expression == "t"
            && *start == 0.0
            && (*end - std::f64::consts::TAU).abs() < 1.0e-12
    ));

    let FeatureDefinition::EquationCurve {
        parameter,
        x_expression,
        y_expression,
        z_expression,
        start,
        end,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed equation curve");
    };
    *parameter = "u".into();
    *x_expression = "u".into();
    *y_expression = "u^2".into();
    *z_expression = "u^3".into();
    *start = -2.0;
    *end = 3.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.xml_tag, "EquationDrivenCurve");
    assert_eq!(feature.kind, "EquationDrivenCurve");
    assert_eq!(feature.properties["Parameter"], "u");
    assert_eq!(feature.properties["XEquation"], "u");
    assert_eq!(feature.properties["YEquation"], "u^2");
    assert_eq!(feature.properties["ZEquation"], "u^3");
    assert_eq!(feature.properties["Start"], "-2");
    assert_eq!(feature.properties["End"], "3");
    assert_eq!(feature.properties["Closed"], "false");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::EquationCurve {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start: -2.0,
            end: 3.0,
        } if parameter == "u"
            && x_expression == "u"
            && y_expression == "u^2"
            && z_expression == "u^3"
    ));
}

#[test]
fn semantic_writer_round_trips_helix() {
    use cadmpeg_ir::features::{FeatureDefinition, Length};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Helix Name="Coil" Type="HelixSpiral" id="30" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1" Clockwise="true" Taper="none"><Dimension Name="Radius">4mm</Dimension><Dimension Name="Pitch">-2mm</Dimension><Dimension Name="Revolutions">3.5</Dimension></Helix></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Helix {
            axis_origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            },
            axis_direction: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            radius: Length(4.0),
            pitch: Length(-2.0),
            revolutions: 3.5,
            clockwise: true,
            ..
        }
    ));

    let FeatureDefinition::Helix {
        axis_origin,
        axis_direction,
        radius,
        pitch,
        revolutions,
        clockwise,
        ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed helix");
    };
    *axis_origin = Point3::new(4.0, 5.0, 6.0);
    *axis_direction = Vector3::new(0.0, 1.0, 0.0);
    *radius = Length(7.0);
    *pitch = Length(8.0);
    *revolutions = 9.25;
    *clockwise = false;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.xml_tag, "Helix");
    assert_eq!(feature.kind, "HelixSpiral");
    assert_eq!(feature.properties["AxisOrigin"], "4mm,5mm,6mm");
    assert_eq!(feature.properties["AxisDirection"], "0,1,0");
    assert_eq!(feature.properties["Clockwise"], "false");
    assert_eq!(feature.properties["Taper"], "none");
    assert_eq!(feature.parameters["Radius"], "7mm");
    assert_eq!(feature.parameters["Pitch"], "8mm");
    assert_eq!(feature.parameters["Revolutions"], "9.25");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Helix {
            axis_origin: Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            },
            axis_direction: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
            radius: Length(7.0),
            pitch: Length(8.0),
            revolutions: 9.25,
            clockwise: false,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_slash_named_helix() {
    use cadmpeg_ir::features::{FeatureDefinition, Length};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Coil" Type="Helix/Spiral" id="30" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1"><Dimension Name="Radius">4mm</Dimension><Dimension Name="Pitch">2mm</Dimension><Dimension Name="Revolutions">3.5</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moHelix_c", "Coil", 30)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Helix {
            radius: Length(4.0),
            pitch: Length(2.0),
            revolutions: 3.5,
            ..
        }
    ));

    let FeatureDefinition::Helix {
        axis_origin,
        axis_direction,
        radius,
        pitch,
        revolutions,
        clockwise,
        ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed helix");
    };
    *axis_origin = Point3::new(4.0, 5.0, 6.0);
    *axis_direction = Vector3::new(0.0, 1.0, 0.0);
    *radius = Length(7.0);
    *pitch = Length(8.0);
    *revolutions = 9.25;
    *clockwise = true;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.kind, "Helix/Spiral");
    assert_eq!(native.parameters["Radius"], "7mm");
    assert_eq!(native.parameters["Pitch"], "8mm");
    assert_eq!(native.parameters["Revolutions"], "9.25");
    assert_eq!(native.properties["AxisOrigin"], "4mm,5mm,6mm");
    assert_eq!(native.properties["AxisDirection"], "0,1,0");
    assert_eq!(native.properties["Clockwise"], "true");
}

#[test]
fn semantic_writer_round_trips_native_axis_helix() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        r#"<Keywords><Feature Name="Helix/Spiral1" Type="Helix/Spiral" id="30"><Dimension Name="D3">3200</Dimension><Dimension Name="D4">12800</Dimension><Dimension Name="D5">0.25</Dimension><Dimension Name="D7">0°</Dimension></Feature></Keywords>"#
            .as_bytes(),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moHelix_c", "Helix/Spiral1", 30)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = &decoded.ir.model.features[0];
    let native_ref = feature.native_ref.as_deref().unwrap();
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::HelixNativeAxis {
            axis_native_ref,
            axial_rise: Length(3200.0),
            pitch: Length(12800.0),
            revolutions: 0.25,
            start_angle: Angle(0.0),
            clockwise: false,
        } if axis_native_ref == native_ref
    ));
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.message
            == "1 typed feature(s) retain native or unresolved required operation operands."
    }));
    let findings = cadmpeg_ir::validate(&decoded.ir, Vec::new()).findings;
    assert!(findings.is_empty(), "{findings:#?}");

    let FeatureDefinition::HelixNativeAxis {
        axial_rise,
        pitch,
        revolutions,
        start_angle,
        clockwise,
        ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed native-axis helix");
    };
    *axial_rise = Length(4000.0);
    *pitch = Length(16000.0);
    *revolutions = 0.5;
    *start_angle = Angle(std::f64::consts::FRAC_PI_2);
    *clockwise = true;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.kind, "Helix/Spiral");
    assert_eq!(native.parameters["D3"], "4000");
    assert_eq!(native.parameters["D4"], "16000");
    assert_eq!(native.parameters["D5"], "0.5");
    assert_eq!(native.parameters["D7"], "90°");
    assert_eq!(native.properties["Clockwise"], "true");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::HelixNativeAxis {
            axial_rise: Length(4000.0),
            pitch: Length(16000.0),
            revolutions: 0.5,
            start_angle: Angle(value),
            clockwise: true,
            ..
        } if (value - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
}

#[test]
fn semantic_writer_round_trips_wrap() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ProfileRef, WrapMode};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><Wrap Name="Mark" Type="Wrap" id="31" Profile="{face}" Face="{face}" Mode="Emboss" Method="Spline"><Dimension Name="Depth">2mm</Dimension></Wrap></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Wrap {
            profile: ProfileRef::Faces(faces),
            face: FaceSelection::Resolved { faces: targets, native },
            mode: WrapMode::Emboss,
            depth: Some(Length(2.0)),
        } if faces == std::slice::from_ref(&face_id) && targets == std::slice::from_ref(&face_id) && native == &face
    ));

    let FeatureDefinition::Wrap {
        profile,
        face,
        mode,
        depth,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed wrap");
    };
    *profile = ProfileRef::Faces(vec![face_id.clone()]);
    *face = FaceSelection::Faces(vec![face_id.clone()]);
    *mode = WrapMode::Deboss;
    *depth = Some(Length(3.5));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Profile"], face_id.0);
    assert_eq!(native.properties["Face"], face_id.0);
    assert_eq!(native.properties["Mode"], "Deboss");
    assert_eq!(native.properties["Method"], "Spline");
    assert_eq!(native.parameters["Depth"], "3.5mm");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Wrap {
            mode: WrapMode::Deboss,
            depth: Some(Length(3.5)),
            ..
        }
    ));

    let mut scribed = regenerated;
    let FeatureDefinition::Wrap { mode, depth, .. } = &mut scribed.ir.model.features[0].definition
    else {
        panic!("typed wrap");
    };
    *mode = WrapMode::Scribe;
    *depth = None;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&scribed.ir, &scribed.source_fidelity, &mut encoded)
        .unwrap();
    let scribed = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&scribed.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Mode"], "Scribe");
    assert!(!native.parameters.contains_key("Depth"));
    assert!(matches!(
        scribed.ir.model.features[0].definition,
        FeatureDefinition::Wrap {
            mode: WrapMode::Scribe,
            depth: None,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_move_copy_body() {
    use cadmpeg_ir::features::{Angle, AxisAngle, BodySelection, FeatureDefinition};
    use cadmpeg_ir::math::{Point3, Vector3};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = base.ir.model.bodies[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><MoveBody Name="Copy" Type="MoveCopyBody" id="32" Bodies="{body}" Translation="1mm,2mm,3mm" RotationOrigin="4mm,5mm,6mm" RotationAxis="0,0,1" Copies="2" Frame="model"><Dimension Name="Rotation">90deg</Dimension></MoveBody></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let body_id = decoded.ir.model.bodies[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::MoveBody {
            bodies: BodySelection::Resolved { bodies, native },
            translation: Vector3 { x: 1.0, y: 2.0, z: 3.0 },
            rotation: Some(AxisAngle {
                origin: Point3 { x: 4.0, y: 5.0, z: 6.0 },
                direction: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
                angle: Angle(angle),
            }),
            copies: 2,
        } if bodies == std::slice::from_ref(&body_id) && native == &body
            && (*angle - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
    ));

    let FeatureDefinition::MoveBody {
        bodies,
        translation,
        rotation,
        copies,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed body motion");
    };
    *bodies = BodySelection::Bodies(vec![body_id.clone()]);
    *translation = Vector3::new(-7.0, 8.0, 9.0);
    *rotation = Some(AxisAngle {
        origin: Point3::new(10.0, 11.0, 12.0),
        direction: Vector3::new(0.0, 1.0, 0.0),
        angle: Angle(0.25),
    });
    *copies = 3;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Bodies"], body_id.0);
    assert_eq!(native.properties["Translation"], "-7mm,8mm,9mm");
    assert_eq!(native.properties["RotationOrigin"], "10mm,11mm,12mm");
    assert_eq!(native.properties["RotationAxis"], "0,1,0");
    assert_eq!(native.properties["Copies"], "3");
    assert_eq!(native.properties["Frame"], "model");
    assert_eq!(native.parameters["Rotation"], "0.25rad");

    let mut translated = regenerated;
    let FeatureDefinition::MoveBody {
        rotation, copies, ..
    } = &mut translated.ir.model.features[0].definition
    else {
        panic!("typed body motion");
    };
    *rotation = None;
    *copies = 0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &translated.ir,
            &translated.source_fidelity,
            &mut encoded,
        )
        .unwrap();
    let translated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&translated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Copies"], "0");
    assert!(!native.properties.contains_key("RotationOrigin"));
    assert!(!native.properties.contains_key("RotationAxis"));
    assert!(!native.parameters.contains_key("Rotation"));
    assert!(matches!(
        translated.ir.model.features[0].definition,
        FeatureDefinition::MoveBody {
            rotation: None,
            copies: 0,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_offset_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><OffsetSurface Name="Offset" Type="OffsetSurface" id="33" Faces="{face}" Knit="true"><Dimension Name="Distance">2mm</Dimension></OffsetSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Resolved { faces, native },
            distance: Some(Length(2.0)),
        } if faces == std::slice::from_ref(&face_id) && native == &face
    ));

    let FeatureDefinition::OffsetSurface { faces, distance } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed offset surface");
    };
    *faces = FaceSelection::Faces(vec![face_id.clone()]);
    *distance = Some(Length(-3.5));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.0);
    assert_eq!(native.properties["Knit"], "true");
    assert_eq!(native.parameters["Distance"], "-3.5mm");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::OffsetSurface {
            distance: Some(Length(-3.5)),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_knit_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><KnitSurface Name="Knit" Type="Knit" id="34" Faces="{face}" MergeEntities="false" CreateSolid="false" CheckGeometry="true"><Dimension Name="GapTolerance">0.01mm</Dimension></KnitSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Resolved { faces, native },
            merge_entities: Some(false),
            create_solid: Some(false),
            gap_tolerance: Some(Length(0.01)),
        } if faces == std::slice::from_ref(&face_id) && native == &face
    ));

    let FeatureDefinition::KnitSurface {
        faces,
        merge_entities,
        create_solid,
        gap_tolerance,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed knit surface");
    };
    *faces = FaceSelection::Faces(vec![face_id.clone()]);
    *merge_entities = Some(true);
    *create_solid = Some(true);
    *gap_tolerance = None;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.0);
    assert_eq!(native.properties["MergeEntities"], "true");
    assert_eq!(native.properties["CreateSolid"], "true");
    assert_eq!(native.properties["CheckGeometry"], "true");
    assert!(!native.parameters.contains_key("GapTolerance"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::KnitSurface {
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: None,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_cut_with_surface() {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = base.ir.model.bodies[0].id.0.clone();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><CutWithSurface Name="Cut" Type="SurfaceCut" id="35" Targets="{body}" Tools="{face}" Reverse="false" ConsumeTool="false"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let body_id = decoded.ir.model.bodies[0].id.clone();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::CutWithSurface {
            targets: BodySelection::Resolved { bodies, native: body_native },
            tools: FaceSelection::Resolved { faces, native: face_native },
            reverse: false,
        } if bodies == std::slice::from_ref(&body_id) && body_native == &body
            && faces == std::slice::from_ref(&face_id) && face_native == &face
    ));

    let FeatureDefinition::CutWithSurface {
        targets,
        tools,
        reverse,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed surface cut");
    };
    *targets = BodySelection::Bodies(vec![body_id.clone()]);
    *tools = FaceSelection::Faces(vec![face_id.clone()]);
    *reverse = true;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Targets"], body_id.0);
    assert_eq!(native.properties["Tools"], face_id.0);
    assert_eq!(native.properties["Reverse"], "true");
    assert_eq!(native.properties["ConsumeTool"], "false");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::CutWithSurface { reverse: true, .. }
    ));
}

#[test]
fn semantic_writer_round_trips_filled_surface() {
    use cadmpeg_ir::features::{
        EdgeSelection, FaceSelection, FeatureDefinition, SurfaceContinuity,
    };

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir.model.edges[0].id.0.clone();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><FilledSurface Name="Fill" Type="FillSurface" id="36" Boundary="{edge}" SupportFaces="{face}" Continuity="Tangent" MergeResult="false" Optimize="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir.model.edges[0].id.clone();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Resolved { edges, native: edge_native }),
            support_faces: FaceSelection::Resolved { faces, native: face_native },
            continuity: Some(SurfaceContinuity::Tangent),
            merge_result: Some(false),
        } if edges == std::slice::from_ref(&edge_id) && edge_native == &edge
            && faces == std::slice::from_ref(&face_id) && face_native == &face
    ));

    let FeatureDefinition::FilledSurface {
        boundary,
        support_faces,
        continuity,
        merge_result,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed filled surface");
    };
    *boundary =
        cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Edges(vec![edge_id.clone()]));
    *support_faces = FaceSelection::Faces(vec![face_id.clone()]);
    *continuity = Some(SurfaceContinuity::Curvature);
    *merge_result = Some(true);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Boundary"], edge_id.0);
    assert_eq!(native.properties["SupportFaces"], face_id.0);
    assert_eq!(native.properties["Continuity"], "Curvature");
    assert_eq!(native.properties["MergeResult"], "true");
    assert_eq!(native.properties["Optimize"], "true");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::FilledSurface {
            continuity: Some(SurfaceContinuity::Curvature),
            merge_result: Some(true),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_trim_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, PathRef, TrimRegion};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir.model.edges[0].id.0.clone();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><TrimSurface Name="Trim" Type="SurfaceTrim" id="37" Faces="{face}" Tool="{edge}" Keep="Inside" Split="false"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir.model.edges[0].id.clone();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::TrimSurface {
            faces: FaceSelection::Resolved { faces, native },
            tool: PathRef::Edges(edges),
            keep: TrimRegion::Inside,
        } if faces == std::slice::from_ref(&face_id) && native == &face && edges == std::slice::from_ref(&edge_id)
    ));

    let FeatureDefinition::TrimSurface { faces, tool, keep } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed trim surface");
    };
    *faces = FaceSelection::Faces(vec![face_id.clone()]);
    *tool = PathRef::Edges(vec![edge_id.clone()]);
    *keep = TrimRegion::Outside;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.0);
    assert_eq!(native.properties["Tool"], edge_id.0);
    assert_eq!(native.properties["Keep"], "Outside");
    assert_eq!(native.properties["Split"], "false");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::TrimSurface {
            keep: TrimRegion::Outside,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_extend_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, SurfaceExtension};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><ExtendSurface Name="Extend" Type="SurfaceExtend" id="38" Faces="{face}" Method="Natural" CornerMode="Merge"><Dimension Name="Distance">2mm</Dimension></ExtendSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Resolved { faces, native },
            distance: Some(Length(2.0)),
            method: SurfaceExtension::Natural,
        } if faces == std::slice::from_ref(&face_id) && native == &face
    ));

    let FeatureDefinition::ExtendSurface {
        faces,
        distance,
        method,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed extended surface");
    };
    *faces = FaceSelection::Faces(vec![face_id.clone()]);
    *distance = Some(Length(4.5));
    *method = SurfaceExtension::Linear;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.0);
    assert_eq!(native.properties["Method"], "Linear");
    assert_eq!(native.properties["CornerMode"], "Merge");
    assert_eq!(native.parameters["Distance"], "4.5mm");
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::ExtendSurface {
            distance: Some(Length(4.5)),
            method: SurfaceExtension::Linear,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_all_ruled_surface_modes() {
    use cadmpeg_ir::features::{
        EdgeSelection, FaceSelection, FeatureDefinition, Length, RuledSurfaceMode,
    };
    use cadmpeg_ir::math::Vector3;

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir.model.edges[0].id.0.clone();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><RuledSurface Name="Ruled" Type="SurfaceRuled" id="39" Edges="{edge}" SupportFaces="{face}" Mode="Direction" Direction="0,0,1" Trim="true"><Dimension Name="Distance">2mm</Dimension></RuledSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir.model.edges[0].id.clone();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::RuledSurface {
            edges: EdgeSelection::Resolved { edges, native: edge_native },
            support_faces: FaceSelection::Resolved { faces, native: face_native },
            mode: RuledSurfaceMode::Direction {
                direction: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
                distance: Length(2.0),
            },
        } if edges == std::slice::from_ref(&edge_id) && edge_native == &edge
            && faces == std::slice::from_ref(&face_id) && face_native == &face
    ));

    let FeatureDefinition::RuledSurface {
        edges,
        support_faces,
        mode,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed ruled surface");
    };
    *edges = EdgeSelection::Edges(vec![edge_id.clone()]);
    *support_faces = FaceSelection::Faces(vec![face_id.clone()]);
    *mode = RuledSurfaceMode::Normal {
        distance: Length(3.0),
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Mode"], "Normal");
    assert!(!native.properties.contains_key("Direction"));
    assert_eq!(native.properties["Trim"], "true");
    assert_eq!(native.parameters["Distance"], "3mm");

    let FeatureDefinition::RuledSurface { mode, .. } =
        &mut regenerated.ir.model.features[0].definition
    else {
        panic!("typed ruled surface");
    };
    *mode = RuledSurfaceMode::Tangent {
        distance: Length(4.0),
    };
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &regenerated.ir,
            &regenerated.source_fidelity,
            &mut encoded,
        )
        .unwrap();
    let tangent = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        tangent.ir.model.features[0].definition,
        FeatureDefinition::RuledSurface {
            mode: RuledSurfaceMode::Tangent {
                distance: Length(4.0)
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_projected_curve() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, PathRef};
    use cadmpeg_ir::math::Vector3;

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir.model.edges[0].id.0.clone();
    let face = base.ir.model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><ProjectedCurve Name="Projection" Type="ProjectionCurve" id="40" Source="{edge}" TargetFaces="{face}" Direction="0,0,1" Bidirectional="false" Simplify="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir.model.edges[0].id.clone();
    let face_id = decoded.ir.model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::ProjectedCurve {
            source: PathRef::Edges(edges),
            target_faces: FaceSelection::Resolved { faces, native },
            direction: cadmpeg_ir::features::CurveProjectionDirection::Vector(Vector3 { x: 0.0, y: 0.0, z: 1.0 }),
            bidirectional: Some(false),
        } if edges == std::slice::from_ref(&edge_id) && faces == std::slice::from_ref(&face_id) && native == &face
    ));

    let FeatureDefinition::ProjectedCurve {
        source,
        target_faces,
        direction,
        bidirectional,
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed projected curve");
    };
    *source = PathRef::Edges(vec![edge_id.clone()]);
    *target_faces = FaceSelection::Faces(vec![face_id.clone()]);
    *direction = cadmpeg_ir::features::CurveProjectionDirection::State(
        cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal,
    );
    *bidirectional = Some(true);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Source"], edge_id.0);
    assert_eq!(native.properties["TargetFaces"], face_id.0);
    assert_eq!(native.properties["Bidirectional"], "true");
    assert_eq!(native.properties["Simplify"], "true");
    assert!(!native.properties.contains_key("Direction"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::ProjectedCurve {
            direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal
            ),
            bidirectional: Some(true),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_ordered_composite_curve() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let first = base.ir.model.edges[0].id.0.clone();
    let second = base.ir.model.edges[1].id.0.clone();
    let xml = format!(
        r#"<Keywords><CompositeCurve Name="Chain" Type="CompositeCurve" id="41" Segments="{first};{second}" Closed="false" Simplify="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let first_id = decoded.ir.model.edges[0].id.clone();
    let second_id = decoded.ir.model.edges[1].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::CompositeCurve { segments, closed: false }
            if segments == &vec![
                PathRef::Edges(vec![first_id.clone()]),
                PathRef::Edges(vec![second_id.clone()]),
            ]
    ));

    let FeatureDefinition::CompositeCurve { segments, closed } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed composite curve");
    };
    *segments = vec![
        PathRef::Edges(vec![second_id.clone()]),
        PathRef::Edges(vec![first_id.clone()]),
    ];
    *closed = true;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(
        native.properties["Segments"],
        format!("{};{}", second_id.0, first_id.0)
    );
    assert_eq!(native.properties["Closed"], "true");
    assert_eq!(native.properties["Simplify"], "true");
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::CompositeCurve { segments, closed: true }
            if segments == &vec![
                PathRef::Edges(vec![second_id]),
                PathRef::Edges(vec![first_id]),
            ]
    ));
}

#[test]
fn semantic_writer_round_trips_typed_simple_blind_hole() {
    use cadmpeg_ir::features::{FeatureDefinition, HoleKind, Length, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Hole Name="Drill" Type="Hole" id="15"><Dimension Name="Diameter">0.25in</Dimension><Dimension Name="Depth">12mm</Dimension></Hole></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Hole {
            face: None,
            ref placements,
            kind: HoleKind::Simple,
            diameter: Some(Length(6.35)),
            extent: Some(Termination::Blind {
                length: Length(12.0),
            }),
            ..
        } if placements.is_empty()
    ));

    let FeatureDefinition::Hole {
        diameter, extent, ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed hole feature");
    };
    *diameter = Some(Length(8.0));
    *extent = Some(Termination::Blind {
        length: Length(16.0),
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Diameter"], "8mm");
    assert_eq!(feature.parameters["Depth"], "16mm");
}

#[test]
fn semantic_writer_retains_partial_native_hole_construction() {
    use cadmpeg_ir::features::{FeatureDefinition, HoleForm, HoleKind, Length, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Hole Name="Unknown diameter" Type="Hole" id="61" EndCondition="ThroughAll"><Dimension Name="Diameter">NaNmm</Dimension></Hole>
            <Hole Name="Partial counterbore" Type="Hole" id="62" EndCondition="ThroughAll"><Dimension Name="Diameter">6mm</Dimension><Dimension Name="CounterboreDiameter">10mm</Dimension><Dimension Name="CounterboreDepth">NaNmm</Dimension></Hole>
            <Hole Name="Conflicting entry" Type="Hole" id="63" EndCondition="Future" Position="invalid" Direction="0,0,0"><Dimension Name="Diameter">5mm</Dimension><Dimension Name="CounterboreDiameter">11mm</Dimension><Dimension Name="CounterboreDepth">3mm</Dimension><Dimension Name="CountersinkDiameter">9mm</Dimension><Dimension Name="CountersinkAngle">82deg</Dimension></Hole>
        </Keywords>"#,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Simple,
            diameter: None,
            extent: Some(Termination::ThroughAll),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Unresolved {
                form: Some(HoleForm::Counterbore),
                counterbore_diameter: Some(Length(10.0)),
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            },
            diameter: Some(Length(6.0)),
            extent: Some(Termination::ThroughAll),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir.model.features[2].definition,
        FeatureDefinition::Hole {
            ref placements,
            kind: HoleKind::Unresolved {
                form: None,
                counterbore_diameter: Some(Length(11.0)),
                counterbore_depth: Some(Length(3.0)),
                countersink_diameter: Some(Length(9.0)),
                countersink_angle: Some(_),
            },
            diameter: Some(Length(5.0)),
            extent: None,
            ..
        } if placements.is_empty()
    ));

    for (index, message) in [
        (0, "unresolved hole diameter"),
        (1, "unresolved hole entry construction"),
    ] {
        let mut detached = decoded.ir.clone();
        detached.model.features[index].native_ref = None;
        let error = SldprtCodec
            .write_preserved_with_source_fidelity(
                &detached,
                &decoded.source_fidelity,
                &mut Vec::new(),
            )
            .unwrap_err();
        assert!(error.to_string().contains(message));
    }
    let mut detached = decoded.ir.clone();
    detached.model.features[2].native_ref = None;
    let FeatureDefinition::Hole { kind, .. } = &mut detached.model.features[2].definition else {
        panic!("partial hole");
    };
    *kind = HoleKind::Simple;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("unresolved hole termination"));

    for (index, feature) in decoded.ir.model.features.iter_mut().enumerate() {
        feature.name = Some(format!("Renamed hole {}", index + 1));
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].parameters["Diameter"], "NaNmm");
    assert_eq!(native[1].parameters["CounterboreDepth"], "NaNmm");
    assert_eq!(native[2].properties["EndCondition"], "Future");
    assert_eq!(native[2].properties["Position"], "invalid");
    assert_eq!(native[2].properties["Direction"], "0,0,0");
    assert_eq!(native[2].parameters["CounterboreDiameter"], "11mm");
    assert_eq!(native[2].parameters["CountersinkDiameter"], "9mm");
}

#[test]
fn semantic_writer_round_trips_hole_placement() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, HolePlacement, Termination};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Hole Name="Placed" Type="Hole" id="28" Face="face:12" Position="1mm,2mm,3mm" Direction="0,0,-1" EndCondition="Blind"><Dimension Name="Diameter">6mm</Dimension><Dimension Name="Depth">10mm</Dimension></Hole></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let FeatureDefinition::Hole {
        face,
        placements,
        extent,
        ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed hole feature");
    };
    assert_eq!(face, &Some(FaceSelection::Native("face:12".into())));
    assert_eq!(
        placements,
        &[HolePlacement::Directed {
            position: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(0.0, 0.0, -1.0),
        }]
    );

    *face = Some(FaceSelection::Native("face:13".into()));
    *placements = vec![HolePlacement::Directed {
        position: Point3::new(4.0, 5.0, 6.0),
        direction: Vector3::new(0.0, 1.0, 0.0),
    }];
    *extent = Some(Termination::ThroughAll);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.properties["Face"], "face:13");
    assert_eq!(native.properties["Position"], "4mm,5mm,6mm");
    assert_eq!(native.properties["Direction"], "0,1,0");
    assert_eq!(native.properties["EndCondition"], "ThroughAll");
    assert!(!native.parameters.contains_key("Depth"));
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        FeatureDefinition::Hole {
            face: Some(FaceSelection::Native(face)),
            placements,
            extent: Some(Termination::ThroughAll),
            ..
        } if face == "face:13"
            && placements == &[HolePlacement::Directed {
                position: Point3::new(4.0, 5.0, 6.0),
                direction: Vector3::new(0.0, 1.0, 0.0),
            }]
    ));
}

#[test]
fn semantic_writer_round_trips_counterbore_and_countersink_holes() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, HoleKind, Length, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Hole Name="Counterbore" Type="Hole" id="51" EndCondition="Blind"><Dimension Name="Diameter">6mm</Dimension><Dimension Name="Depth">20mm</Dimension><Dimension Name="CounterboreDiameter">10mm</Dimension><Dimension Name="CounterboreDepth">4mm</Dimension></Hole>
            <Hole Name="Countersink" Type="Hole" id="52" EndCondition="ThroughAll"><Dimension Name="Diameter">5mm</Dimension><Dimension Name="CountersinkDiameter">9mm</Dimension><Dimension Name="CountersinkAngle">82deg</Dimension></Hole>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Counterbore {
                diameter: Length(10.0),
                depth: Length(4.0),
            },
            extent: Some(Termination::Blind {
                length: Length(20.0),
            }),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Countersink {
                diameter: Length(9.0),
                angle: Angle(value),
            },
            extent: Some(Termination::ThroughAll),
            ..
        } if (*value - 82f64.to_radians()).abs() < 1e-12
    ));

    let FeatureDefinition::Hole { kind, extent, .. } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("counterbore hole");
    };
    *kind = HoleKind::Counterbore {
        diameter: Length(12.0),
        depth: Length(5.0),
    };
    *extent = Some(Termination::ThroughAll);
    let FeatureDefinition::Hole { kind, extent, .. } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("countersink hole");
    };
    *kind = HoleKind::Countersink {
        diameter: Length(11.0),
        angle: Angle(90f64.to_radians()),
    };
    *extent = Some(Termination::Blind {
        length: Length(25.0),
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(features[0].properties["EndCondition"], "ThroughAll");
    assert!(!features[0].parameters.contains_key("Depth"));
    assert_eq!(features[0].parameters["CounterboreDiameter"], "12mm");
    assert_eq!(features[0].parameters["CounterboreDepth"], "5mm");
    assert_eq!(features[1].properties["EndCondition"], "Blind");
    assert_eq!(features[1].parameters["Depth"], "25mm");
    assert_eq!(features[1].parameters["CountersinkDiameter"], "11mm");
    assert_eq!(
        features[1].parameters["CountersinkAngle"],
        format!("{}rad", 90f64.to_radians())
    );
}

#[test]
fn decode_projects_generic_revolution_with_explicit_operation() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, RevolveExtent, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolution Name="Generic" Type="GenericRevolution" id="43" Operation="Cut" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1"><Dimension Name="Angle">180deg</Dimension></Revolution></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle },
                }),
                ..
            },
            op: BooleanOp::Cut,
        } if (angle.0 - std::f64::consts::PI).abs() < 1e-12
    ));
}

#[test]
fn semantic_writer_round_trips_typed_revolution() {
    use cadmpeg_ir::features::{Angle, BooleanOp, FeatureDefinition, RevolveExtent, Termination};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolve Name="Turn" Type="Revolve" id="17" AxisOrigin="10mm,20mm,30mm" AxisDirection="0,1,0" Operation="Join"><Dimension Name="Angle">180deg</Dimension></Revolve></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(cadmpeg_ir::features::RevolutionAxis {
                    origin: Point3 { x: 10.0, y: 20.0, z: 30.0 },
                    direction: Vector3 { x: 0.0, y: 1.0, z: 0.0 },
                }),
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle: Angle(value) },
                }),
                ..
            },
            op: BooleanOp::Join,
        } if (*value - std::f64::consts::PI).abs() < 1e-12
    ));

    let FeatureDefinition::Revolve { construction, op } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed revolution feature");
    };
    let Some(axis) = construction.axis.as_mut() else {
        panic!("resolved revolution axis");
    };
    axis.origin = Point3::new(1.0, 2.0, 3.0);
    axis.direction = Vector3::new(0.0, 0.0, 1.0);
    construction.extent = Some(RevolveExtent::OneSided {
        termination: Termination::Angle {
            angle: Angle(std::f64::consts::FRAC_PI_2),
        },
    });
    *op = BooleanOp::Cut;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(feature.properties["AxisOrigin"], "1mm,2mm,3mm");
    assert_eq!(feature.properties["AxisDirection"], "0,0,1");
    assert_eq!(feature.properties["Operation"], "Cut");
    assert_eq!(
        feature.parameters["Angle"],
        format!("{}rad", std::f64::consts::FRAC_PI_2)
    );
}

#[test]
fn semantic_writer_retains_partial_native_revolution_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolve Name="Unknown turn" Type="Revolve" id="17" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1"/></Keywords>"#,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(cadmpeg_ir::features::RevolutionAxis {
                    origin: Point3 {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0
                    },
                    direction: Vector3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0
                    },
                }),
                extent: None,
                ..
            },
            op: BooleanOp::Unresolved,
        }
    ));
    let mut detached = decoded.ir.clone();
    detached.model.features[0].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("unresolved revolution construction"));
    decoded.ir.model.features[0].name = Some("Renamed turn".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.name, "Renamed turn");
    assert_eq!(native.properties["AxisOrigin"], "1mm,2mm,3mm");
    assert_eq!(native.properties["AxisDirection"], "0,0,1");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(!native.parameters.contains_key("Angle"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(_),
                profile: None,
                extent: None,
                ..
            },
            op: BooleanOp::Unresolved,
        }
    ));
}

#[test]
fn semantic_writer_round_trips_all_revolution_extents() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, FeatureDefinition, ProfileRef, RevolveExtent, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="TurnProfile" Type="Sketch" id="40"/><Revolve Name="One" Type="Revolve" id="41" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" EndCondition="OneSided" Operation="Join"><Dimension Name="Angle">90deg</Dimension></Revolve><Revolve Name="Sym" Type="Revolve" id="42" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,1,0" EndCondition="Symmetric" Operation="NewBody"><Dimension Name="Angle">180deg</Dimension></Revolve><Revolve Name="Two" Type="Revolve" id="43" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="1,0,0" EndCondition="TwoSided" Operation="Cut"><Dimension Name="Angle">30deg</Dimension><Dimension Name="Angle2">60deg</Dimension></Revolve></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile_feature = decoded.ir.model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: Some(ProfileRef::Feature(profile)),
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle: Angle(value) },
                }),
                ..
            },
            op: BooleanOp::Join,
        } if profile == &profile_feature && (*value - 90f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        decoded.ir.model.features[2].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::Symmetric {
                    termination: Termination::Angle { angle: Angle(value) },
                }),
                ..
            },
            op: BooleanOp::NewBody,
        } if (value - std::f64::consts::PI).abs() < 1e-12
    ));
    assert!(matches!(
        decoded.ir.model.features[3].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::TwoSided {
                    first: Termination::Angle { angle: Angle(first) },
                    second: Termination::Angle { angle: Angle(second) },
                }),
                ..
            },
            op: BooleanOp::Cut,
        } if (first - 30f64.to_radians()).abs() < 1e-12
            && (second - 60f64.to_radians()).abs() < 1e-12
    ));

    let FeatureDefinition::Revolve { construction, op } =
        &mut decoded.ir.model.features[3].definition
    else {
        panic!("typed revolution");
    };
    construction.extent = Some(RevolveExtent::OneSided {
        termination: Termination::Angle { angle: Angle(0.75) },
    });
    *op = BooleanOp::Intersect;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[3].properties["EndCondition"], "OneSided");
    assert_eq!(native[3].properties["Operation"], "Intersect");
    assert_eq!(native[3].properties["Profile"], "40");
    assert_eq!(native[3].parameters["Angle"], "0.75rad");
    assert!(!native[3].parameters.contains_key("Angle2"));
}

#[test]
fn semantic_writer_round_trips_all_pattern_forms() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, Length, PatternKind};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="NativeSeed" id="7"/>
            <Pattern Name="Rows" Type="LinearPattern" id="18" Seeds="7" Direction="1,0,0"><Dimension Name="Count">3</Dimension><Dimension Name="Spacing">10mm</Dimension></Pattern>
            <Pattern Name="Ring" Type="CircularPattern" id="19" Seeds="7" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1"><Dimension Name="Count">4</Dimension><Dimension Name="Angle">360deg</Dimension></Pattern>
            <Mirror Name="Reflect" Type="Mirror" id="20" Seeds="7" PlaneOrigin="5mm,0mm,0mm" PlaneNormal="1,0,0"/>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let seed = decoded.ir.model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x: 1.0, y: 0.0, z: 0.0 }),
                spacing: Length(10.0),
                count: 3,
                second: None,
            },
        } if seeds == &[cadmpeg_ir::features::PatternSeed::Feature(seed.clone())]
    ));
    assert!(matches!(
        &decoded.ir.model.features[2].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Circular {
                axis_origin: Point3 { x: 0.0, y: 0.0, z: 0.0 },
                axis_dir: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
                angle: Angle(value),
                count: 4,
            },
            ..
        } if (*value - std::f64::consts::TAU).abs() < 1e-12
    ));
    assert!(matches!(
        &decoded.ir.model.features[3].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Mirror {
                plane_origin: Point3 {
                    x: 5.0,
                    y: 0.0,
                    z: 0.0
                },
                plane_normal: Vector3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0
                },
            },
            ..
        }
    ));

    let FeatureDefinition::Pattern {
        pattern:
            PatternKind::Linear {
                direction,
                spacing,
                count,
                second: _,
            },
        ..
    } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("linear pattern");
    };
    *direction = Some(Vector3::new(0.0, 1.0, 0.0));
    *spacing = Length(12.0);
    *count = 5;
    let FeatureDefinition::Pattern {
        pattern:
            PatternKind::Circular {
                axis_origin,
                angle,
                count,
                ..
            },
        ..
    } = &mut decoded.ir.model.features[2].definition
    else {
        panic!("circular pattern");
    };
    *axis_origin = Point3::new(1.0, 2.0, 3.0);
    *angle = Angle(std::f64::consts::PI);
    *count = 6;
    let FeatureDefinition::Pattern {
        pattern: PatternKind::Mirror {
            plane_origin,
            plane_normal,
        },
        ..
    } = &mut decoded.ir.model.features[3].definition
    else {
        panic!("mirror pattern");
    };
    *plane_origin = Point3::new(2.0, 0.0, 0.0);
    *plane_normal = Vector3::new(0.0, 1.0, 0.0);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(features[1].properties["Seeds"], "7");
    assert_eq!(features[1].properties["Direction"], "0,1,0");
    assert_eq!(features[1].parameters["Spacing"], "12mm");
    assert_eq!(features[1].parameters["Count"], "5");
    assert_eq!(features[2].properties["AxisOrigin"], "1mm,2mm,3mm");
    assert_eq!(features[2].parameters["Count"], "6");
    assert_eq!(features[3].properties["PlaneOrigin"], "2mm,0mm,0mm");
    assert_eq!(features[3].properties["PlaneNormal"], "0,1,0");
}

#[test]
fn semantic_writer_round_trips_sparse_curve_driven_pattern() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, PatternKind};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Curve Pattern1" Type="CrvPattern" id="169"><Dimension Name="D3">397.6</Dimension><Dimension Name="D1">16</Dimension></Feature></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::CurveDriven {
                path: None,
                spacing: Length(397.6),
                count: 16,
            },
        } if seeds.is_empty()
    ));
    assert_eq!(
        decoded.ir.model.parameters[0].value,
        Some(ParameterValue::Length(Length(397.6)))
    );
    assert_eq!(
        decoded.ir.model.parameters[1].value,
        Some(ParameterValue::Integer(16))
    );

    let FeatureDefinition::Pattern {
        pattern: PatternKind::CurveDriven { spacing, count, .. },
        ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("curve-driven pattern");
    };
    *spacing = Length(250.0);
    *count = 8;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.kind, "CrvPattern");
    assert_eq!(native.parameters["D3"], "250");
    assert_eq!(native.parameters["D1"], "8");
    assert!(!native.parameters.contains_key("Spacing"));
    assert!(!native.parameters.contains_key("Count"));
    assert!(!native.properties.contains_key("Seeds"));
    assert!(!native.properties.contains_key("Path"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven {
                path: None,
                spacing: Length(250.0),
                count: 8,
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_sparse_localized_linear_pattern() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, PatternKind};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="MatrizL1" Type="MatrizL" id="132"><Dimension Name="D1">15</Dimension><Dimension Name="D3">2.54</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moLPattern_c", "MatrizL1", 132)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::Linear {
                direction: None,
                spacing: Length(2.54),
                count: 15,
                second: None,
            },
        } if seeds.is_empty()
    ));
    assert_eq!(
        decoded.ir.model.parameters[0].value,
        Some(ParameterValue::Integer(15))
    );
    assert_eq!(
        decoded.ir.model.parameters[1].value,
        Some(ParameterValue::Length(Length(2.54)))
    );

    let FeatureDefinition::Pattern {
        pattern: PatternKind::Linear { spacing, count, .. },
        ..
    } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("localized linear pattern");
    };
    *spacing = Length(3.5);
    *count = 12;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.kind, "MatrizL");
    assert_eq!(native.input_class.as_deref(), Some("moLPattern_c"));
    assert_eq!(native.parameters["D1"], "12");
    assert_eq!(native.parameters["D3"], "3.5");
    assert!(!native.parameters.contains_key("Count"));
    assert!(!native.parameters.contains_key("Spacing"));
    assert!(!native.properties.contains_key("Seeds"));
    assert!(!native.properties.contains_key("Direction"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Linear {
                direction: None,
                spacing: Length(3.5),
                count: 12,
                second: None,
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_retains_unresolved_native_pattern_construction() {
    use cadmpeg_ir::features::{FeatureDefinition, PatternForm, PatternKind};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Unknown pattern" Type="Custom" id="132"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moLPattern_c", "Unknown pattern", 132)]),
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Pattern {
            seeds,
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Linear),
            },
        } if seeds.is_empty()
    ));
    decoded.ir.model.features[0].name = Some("Renamed pattern".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.name, "Renamed pattern");
    assert!(!native.properties.contains_key("Seeds"));
    assert!(!native.properties.contains_key("Direction"));
    assert!(!native.parameters.contains_key("Count"));
    assert!(!native.parameters.contains_key("Spacing"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Linear),
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_generic_pattern_type() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, PatternKind};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Seed" Type="NativeSeed" id="61"/><Pattern Name="Rows" Type="CustomPattern" id="62" PatternType="Linear" Seeds="61" Direction="1,0,0"><Dimension Name="Count">2</Dimension><Dimension Name="Spacing">4mm</Dimension></Pattern></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let FeatureDefinition::Pattern {
        pattern: PatternKind::Linear { spacing, count, .. },
        ..
    } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("generic linear pattern");
    };
    *spacing = Length(6.0);
    *count = 3;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[1];
    assert_eq!(feature.kind, "CustomPattern");
    assert_eq!(feature.properties["PatternType"], "Linear");
    assert_eq!(feature.parameters["Spacing"], "6mm");
    assert_eq!(feature.parameters["Count"], "3");
}

#[test]
fn semantic_writer_round_trips_typed_sweep() {
    use cadmpeg_ir::features::{Angle, BooleanOp, FeatureDefinition, PathRef, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="ProfileA" Type="Sketch" id="21"/>
            <Sketch Name="Path" Type="Sketch" id="22"/>
            <Sketch Name="ProfileB" Type="Sketch" id="23"/>
            <Sweep Name="Pipe" Type="Sweep" id="24" Profile="21" Path="22" Operation="NewBody"><Dimension Name="Scale">1.5</Dimension><Dimension Name="Twist">90deg</Dimension></Sweep>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile_a = decoded.ir.model.features[0].id.clone();
    let path = decoded.ir.model.features[1].native_ref.clone().unwrap();
    let profile_b = decoded.ir.model.features[2].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[3].definition,
        FeatureDefinition::Sweep {
            profile: Some(ProfileRef::Feature(profile)),
            path: Some(PathRef::Native(path_ref)),
            mode: cadmpeg_ir::features::SweepMode::Solid {
                op: BooleanOp::NewBody,
            },
            twist: Some(Angle(twist)),
            scale: Some(1.5),
            ..
        } if profile == &profile_a
            && path_ref == &path
            && (*twist - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));

    let FeatureDefinition::Sweep {
        profile,
        mode,
        twist,
        scale,
        ..
    } = &mut decoded.ir.model.features[3].definition
    else {
        panic!("typed sweep");
    };
    *profile = Some(ProfileRef::Feature(profile_b.clone()));
    *mode = cadmpeg_ir::features::SweepMode::Solid {
        op: BooleanOp::Join,
    };
    *twist = Some(Angle(std::f64::consts::PI));
    *scale = Some(2.0);
    decoded.ir.model.features[3]
        .dependencies
        .retain(|dependency| dependency != &profile_a);
    decoded.ir.model.features[3].dependencies.push(profile_b);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[3];
    assert_eq!(feature.properties["Profile"], "23");
    assert_eq!(feature.properties["Path"], "22");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.parameters["Scale"], "2");
    assert_eq!(
        feature.parameters["Twist"],
        format!("{}rad", std::f64::consts::PI)
    );
}

#[test]
fn semantic_writer_round_trips_sparse_surface_sweep() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moSweep_c", "Surface-Sweep1", 137)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Sweep {
            profile: None,
            path: None,
            mode: cadmpeg_ir::features::SweepMode::Surface,
            twist: None,
            scale: None,
            ..
        }
    ));

    let FeatureDefinition::Sweep { twist, .. } = &mut decoded.ir.model.features[0].definition
    else {
        panic!("surface sweep");
    };
    *twist = Some(Angle(0.5));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.kind, "Surface-Sweep");
    assert_eq!(native.parameters["Twist"], "0.5rad");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("Path"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Sweep {
            profile: None,
            path: None,
            mode: cadmpeg_ir::features::SweepMode::Surface,
            twist: Some(Angle(0.5)),
            scale: None,
            ..
        }
    ));
}

#[test]
fn semantic_writer_retains_native_solid_sweep_with_unresolved_operation() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Operacion1" Type="Personalizado" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moSweep_c", "Operacion1", 137)]),
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Sweep {
            profile: None,
            path: None,
            mode: SweepMode::Solid {
                op: BooleanOp::Unresolved
            },
            twist: None,
            scale: None,
            ..
        }
    ));
    decoded.ir.model.features[0].name = Some("Renamed sweep".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Solid {
                op: BooleanOp::Unresolved
            },
            ..
        }
    ));
}

#[test]
fn decode_projects_compact_solid_sweep_join_operation() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    let mut resolved = 15u32.to_le_bytes().to_vec();
    resolved.extend_from_slice(&[0; 8]);
    resolved.extend(resolved_feature_classes_with_ids(&[(
        "moSweep_c",
        "Sweep",
        137,
    )]));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Solid {
                op: BooleanOp::Join
            },
            ..
        }
    ));
}

#[test]
fn decode_projects_compact_solid_sweep_general_curve_path() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    let mut resolved = resolved_feature_classes_with_ids(&[("moSweep_c", "Sweep", 137)]);
    let path_offset = resolved.len();
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    let path_class = b"moGeneralCurveRef_w";
    resolved.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(path_class);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Sweep {
            path: Some(PathRef::Native(path)),
            ..
        } if path.ends_with(&format!(":{path_offset}"))
    ));
}

#[test]
fn decode_does_not_globalize_configuration_local_sweep_path() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    fn sweep_payload(has_path: bool) -> Vec<u8> {
        let mut payload = resolved_feature_classes_with_ids(&[("moSweep_c", "Sweep", 137)]);
        if has_path {
            payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
            let path_class = b"moGeneralCurveRef_w";
            payload.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
            payload.extend_from_slice(path_class);
        }
        payload
    }

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Alternate"/><Feature Name="Sweep" Type="Localized" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &sweep_payload(true),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-1-ResolvedFeatures",
        &sweep_payload(false),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Sweep { path: None, .. }
    ));
    let feature_id = decoded.ir.model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.configurations[0].feature_states[&feature_id].definition,
        FeatureDefinition::Sweep {
            path: Some(PathRef::Native(path)),
            ..
        } if path.starts_with("sldprt:feature-input:general-curve-ref:")
    ));
    assert!(matches!(
        decoded.ir.model.configurations[1].feature_states[&feature_id].definition,
        FeatureDefinition::Sweep { path: None, .. }
    ));
}

#[test]
fn decode_projects_native_surface_sweep_class_without_localized_type() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Operacion1" Type="Personalizado" id="137"/></Keywords>"#,
    ));
    let mut resolved =
        resolved_feature_classes_with_ids(&[("moSweepRefSurface_c", "Operacion1", 137)]);
    let path_offset = resolved.len();
    resolved.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
    let path_class = b"moGeneralCurveRef_w";
    resolved.extend_from_slice(&(path_class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(path_class);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Sweep {
            mode: SweepMode::Surface,
            path: Some(PathRef::Native(ref path)),
            ..
        }
        if path.ends_with(&format!(":{path_offset}"))
    ));
}

#[test]
fn decode_projects_surface_sweep_reference_curve_profile() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Helix1" Type="Helix/Spiral" id="119"/>
            <Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/>
        </Keywords>"#,
    ));
    let mut resolved = resolved_feature_classes_with_ids(&[
        ("moHelix_c", "Helix1", 119),
        ("moSweepRefSurface_c", "Surface-Sweep1", 137),
    ]);
    resolved.extend_from_slice(&[0xdd, 0x94, 0xff, 0xff, 1, 0]);
    let class = b"moCompReferenceCurve_c";
    resolved.extend_from_slice(&(class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(class);
    let prefix = resolved.len();
    resolved.resize(prefix + 133, 0);
    resolved[prefix..prefix + 10].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    resolved[prefix + 45..prefix + 61].fill(0xff);
    let reference = prefix + 81;
    resolved[reference..reference + 4].copy_from_slice(&119u32.to_le_bytes());
    resolved[reference + 4..reference + 8].copy_from_slice(&0x5edf_5674u32.to_le_bytes());
    resolved[reference + 16..reference + 20].copy_from_slice(&0x65u32.to_le_bytes());
    resolved[reference + 24..reference + 28].fill(0xff);
    for offset in [reference + 32, reference + 36, reference + 40] {
        resolved[offset..offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    resolved[reference + 48..reference + 52].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let helix = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Helix1"))
        .unwrap();
    let sweep = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap();
    assert!(matches!(
        &sweep.definition,
        FeatureDefinition::Sweep {
            profile: Some(ProfileRef::Feature(feature)),
            ..
        } if feature == &helix.id
    ));
    assert!(sweep.dependencies.contains(&helix.id));

    decoded
        .ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap()
        .name = Some("Renamed surface sweep".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Renamed surface sweep"))
            .map(|feature| &feature.definition),
        Some(FeatureDefinition::Sweep {
            profile: Some(ProfileRef::Feature(_)),
            ..
        })
    ));
}

#[test]
fn decode_projects_generated_surface_sweep_profile_path() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/>
            <Feature Name="Surface-Sweep2" Type="Surface-Sweep" id="211"/>
        </Keywords>"#,
    ));
    let mut resolved =
        resolved_feature_classes_with_ids(&[("moSweepRefSurface_c", "Surface-Sweep1", 137)]);
    resolved.extend_from_slice(&[0xdd, 0x94, 0xff, 0xff, 1, 0]);
    let class = b"moCompReferenceCurve_c";
    resolved.extend_from_slice(&(class.len() as u16).to_le_bytes());
    resolved.extend_from_slice(class);
    resolved.extend_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0, 0]);
    resolved.extend(resolved_feature_classes_with_ids(&[(
        "moSweepRefSurface_c",
        "Surface-Sweep2",
        211,
    )]));
    let wrapper = resolved.len();
    resolved.extend_from_slice(&[0xdd, 0x94, 0xa3, 0x92, 0x2b, 0x80, 0x02, 0, 0, 4, 0, 0]);
    let marker = resolved.len() + 12;
    resolved.resize(marker + 18, 0);
    resolved[marker - 12..marker - 8].copy_from_slice(&2u32.to_le_bytes());
    resolved[marker - 8..marker - 4].copy_from_slice(&[4, 2, 0, 0]);
    resolved[marker..marker + 16].copy_from_slice(&[
        0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2,
        0x54,
    ]);
    let entry = marker + 18;
    resolved.resize(entry + 32, 0);
    resolved[entry..entry + 2].copy_from_slice(&0x8c20u16.to_le_bytes());
    resolved[entry + 4..entry + 8].copy_from_slice(&[0x34, 0x80, 0x37, 0]);
    resolved[entry + 8..entry + 12].copy_from_slice(&137u32.to_le_bytes());
    resolved[entry + 12..entry + 16].copy_from_slice(&0x5edf_56e2u32.to_le_bytes());
    resolved[entry + 16..entry + 20].copy_from_slice(&7u32.to_le_bytes());
    resolved[entry + 28..entry + 32].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let first = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep1"))
        .unwrap();
    let second = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Surface-Sweep2"))
        .unwrap();
    assert!(matches!(
        &second.definition,
        FeatureDefinition::Sweep {
            profile: Some(ProfileRef::Generated { curves, native }),
            ..
        } if curves.len() == 1
            && curves[0].feature == first.id
            && curves[0].local_id == "7"
            && native.ends_with(&wrapper.to_string())
    ));
    assert!(second.dependencies.contains(&first.id));
}

#[test]
fn semantic_writer_round_trips_typed_loft() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, PathRef, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="SectionA" Type="Sketch" id="31"/>
            <Sketch Name="SectionB" Type="Sketch" id="32"/>
            <Sketch Name="SectionC" Type="Sketch" id="33"/>
            <Sketch Name="GuideA" Type="Sketch" id="34"/>
            <Sketch Name="GuideB" Type="Sketch" id="36"/>
            <Loft Name="Transition" Type="Loft" id="35" Profiles="31,32,33" Guides="34" Operation="NewBody" Closed="false"/>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native_refs = decoded.ir.model.features[..5]
        .iter()
        .map(|feature| feature.native_ref.clone().unwrap())
        .collect::<Vec<_>>();
    let feature_refs = decoded.ir.model.features[..5]
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    assert!(matches!(
        &decoded.ir.model.features[5].definition,
        FeatureDefinition::Loft {
            sections,
            guides,
            op: BooleanOp::NewBody,
            closed: false,
            ..
        } if sections == &vec![
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(feature_refs[0].clone())),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(feature_refs[1].clone())),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(feature_refs[2].clone())),
        ] && guides == &vec![PathRef::Native(native_refs[3].clone())]
    ));

    let FeatureDefinition::Loft {
        sections,
        guides,
        op,
        closed,
        ..
    } = &mut decoded.ir.model.features[5].definition
    else {
        panic!("typed loft");
    };
    sections.swap(0, 2);
    *guides = vec![PathRef::Native(native_refs[4].clone())];
    *op = BooleanOp::Join;
    *closed = true;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[5];
    assert_eq!(feature.properties["Profiles"], "33,32,31");
    assert_eq!(feature.properties["Guides"], "36");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.properties["Closed"], "true");
}

#[test]
fn semantic_writer_retains_unresolved_native_loft_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Loft Name="Unknown loft" Type="Custom" id="151"/></Keywords>"#,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Loft {
            ref sections,
            ref guides,
            op: BooleanOp::Unresolved,
            closed: false,
            ..
        } if sections.is_empty() && guides.is_empty()
    ));
    decoded.ir.model.features[0].name = Some("Renamed loft".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert!(!native.properties.contains_key("Profiles"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(!native.properties.contains_key("Closed"));
    assert!(matches!(
        regenerated.ir.model.features[0].definition,
        FeatureDefinition::Loft {
            op: BooleanOp::Unresolved,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_boundary_boss_as_loft() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="SectionA" Type="Sketch" id="41"/>
            <Sketch Name="SectionB" Type="Sketch" id="42"/>
            <Boundary Name="Blend" Type="BoundaryBoss" id="43" Profiles="41,42"/>
            <Boundary Name="Pocket" Type="BoundaryCut" id="44" Profiles="41,42"/>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let refs = decoded.ir.model.features[..2]
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    assert!(matches!(
        &decoded.ir.model.features[2].definition,
        FeatureDefinition::Loft {
            sections,
            guides,
            op: BooleanOp::Join,
            closed: false,
            ..
        } if sections == &vec![
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(refs[0].clone())),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Feature(refs[1].clone())),
        ] && guides.is_empty()
    ));
    assert!(matches!(
        &decoded.ir.model.features[3].definition,
        FeatureDefinition::Loft {
            op: BooleanOp::Cut,
            closed: false,
            ..
        }
    ));

    let FeatureDefinition::Loft {
        sections, closed, ..
    } = &mut decoded.ir.model.features[2].definition
    else {
        panic!("typed boundary loft");
    };
    sections.reverse();
    *closed = true;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[2];
    assert_eq!(feature.xml_tag, "Boundary");
    assert_eq!(feature.kind, "BoundaryBoss");
    assert_eq!(feature.properties["Profiles"], "42,41");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.properties["Closed"], "true");
    assert!(matches!(
        &regenerated.ir.model.features[3].definition,
        FeatureDefinition::Loft {
            op: BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn semantic_writer_retains_partial_native_rib_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, RibDraft};
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Rib Name="Unknown web" Type="Rib" id="42" Direction="0,1,0"><Dimension Name="Thickness">NaNmm</Dimension><Dimension Name="Draft">NaNrad</Dimension></Rib></Keywords>"#,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: None,
                direction: Some(Vector3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0
                }),
                thickness: None,
                side: None,
                draft: RibDraft::Unresolved,
            },
            op: BooleanOp::Unresolved,
        }
    ));
    let mut detached = decoded.ir.clone();
    detached.model.features[0].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, &decoded.source_fidelity, &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("unresolved rib construction"));

    decoded.ir.model.features[0].name = Some("Renamed web".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features[0];
    assert_eq!(native.name, "Renamed web");
    assert_eq!(native.properties["Direction"], "0,1,0");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("BothSides"));
    assert!(!native.properties.contains_key("Operation"));
    assert_eq!(native.parameters["Thickness"], "NaNmm");
    assert_eq!(native.parameters["Draft"], "NaNrad");
}

#[test]
fn semantic_writer_round_trips_typed_rib() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, FeatureDefinition, Length, ProfileRef, RibDraft, RibSide,
    };
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="RibProfile" Type="Sketch" id="41"/><Rib Name="Web" Type="Rib" id="42" Profile="41" Direction="0,1,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension><Dimension Name="Draft">5deg</Dimension></Rib></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile_ref = decoded.ir.model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir.model.features[1].definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(ProfileRef::Feature(profile)),
                direction: Some(Vector3 { x: 0.0, y: 1.0, z: 0.0 }),
                thickness: Some(Length(2.0)),
                side: Some(RibSide::OneSided),
                draft: RibDraft::Angle(Angle(value)),
            },
            op: BooleanOp::Join,
        } if profile == &profile_ref && (*value - 5f64.to_radians()).abs() < 1e-12
    ));

    let FeatureDefinition::Rib { construction, op } = &mut decoded.ir.model.features[1].definition
    else {
        panic!("typed rib");
    };
    construction.direction = Some(Vector3::new(1.0, 0.0, 0.0));
    construction.thickness = Some(Length(3.0));
    construction.side = Some(RibSide::Centered);
    construction.draft = RibDraft::None;
    *op = BooleanOp::NewBody;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(&regenerated.ir).feature_histories[0].features[1];
    assert_eq!(feature.properties["Profile"], "41");
    assert_eq!(feature.properties["Direction"], "1,0,0");
    assert_eq!(feature.properties["BothSides"], "true");
    assert_eq!(feature.properties["Operation"], "NewBody");
    assert_eq!(feature.parameters["Thickness"], "3mm");
    assert!(!feature.parameters.contains_key("Draft"));
}

#[test]
fn semantic_writer_preserves_parametric_history() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert("Depth".into(), "15mm".into());
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    let native = sldprt_native(&regenerated.ir);
    let history = &native.feature_histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Bracket"));
    assert_eq!(history.configurations[0].name, "Default");
    assert_eq!(history.configurations[0].material.as_deref(), Some("Steel"));
    assert_eq!(history.features.len(), 2);
    assert_eq!(history.features[0].kind, "BossExtrude");
    assert_eq!(history.features[0].parameters["Depth"], "15mm");
    assert_eq!(history.features[1].parent_source_id.as_deref(), Some("7"));
}

#[test]
fn semantic_writer_applies_neutral_feature_edits() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;
    let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed extrusion feature");
    };
    *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
        side: cadmpeg_ir::features::ExtrudeSide {
            termination: cadmpeg_ir::features::Termination::Blind {
                length: cadmpeg_ir::features::Length(18.0),
            },
            draft: None,
            offset: None,
        },
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].parameters["Depth"],
        "18mm"
    );
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(18.0),
                    },
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_rejects_conflicting_feature_edits() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed extrusion feature");
    };
    *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
        side: cadmpeg_ir::features::ExtrudeSide {
            termination: cadmpeg_ir::features::Termination::Blind {
                length: cadmpeg_ir::features::Length(18.0),
            },
            draft: None,
            offset: None,
        },
    };
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert("Depth".into(), "20mm".into());
    });

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("conflicting neutral and native"));
}

#[test]
fn semantic_writer_accepts_matching_resolved_feature_edits() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed extrusion feature");
    };
    *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
        side: cadmpeg_ir::features::ExtrudeSide {
            termination: cadmpeg_ir::features::Termination::Blind {
                length: cadmpeg_ir::features::Length(50.0),
            },
            draft: None,
            offset: None,
        },
    };
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].part_name = Some("Edited".into());
        let scalar = &mut native.feature_input_lanes[0].scalars[0];
        scalar.value = 0.05;
        let offset = usize::try_from(scalar.offset).unwrap();
        native.feature_input_lanes[0].native_payload[offset..offset + 8]
            .copy_from_slice(&0.05f64.to_le_bytes());
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(50.0),
                    },
                    ..
                }
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_patches_resolved_feature_sketch_types() {
    use crate::records::{FeatureInputClassRole, SketchInputKind};

    assert_eq!(
        serde_json::from_str::<SketchInputKind>(r#""curve""#).unwrap(),
        SketchInputKind::LineOrCircle
    );
    assert_eq!(
        serde_json::to_string(&SketchInputKind::LineOrCircle).unwrap(),
        r#""line_or_circle""#
    );

    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0, 1, 2, 3, 9]);
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    assert_eq!(native.feature_input_lanes.len(), 1);
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.configuration.as_deref(), Some("0"));
    assert_eq!(
        lane.classes
            .iter()
            .map(|class| class.name.as_str())
            .collect::<Vec<_>>(),
        [
            "sgPointHandle",
            "sgLineHandle",
            "sgArcHandle",
            "sgPntPntDist"
        ]
    );
    assert!(lane.classes[..3]
        .iter()
        .all(|class| class.role == FeatureInputClassRole::SketchEntity));
    assert_eq!(
        lane.classes[3].role,
        FeatureInputClassRole::SketchConstraint
    );
    assert_eq!(
        lane.names
            .iter()
            .map(|name| name.value.as_str())
            .collect::<Vec<_>>(),
        ["Sketch1", "Boss-Extrude1", "D1"]
    );
    assert_eq!(lane.scalars.len(), 1);
    assert_eq!(lane.scalars[0].name, lane.names[2].id);
    assert_eq!(lane.scalars[0].value, 0.025);
    assert_eq!(lane.scalars[0].object_id, 1);
    assert_eq!(lane.scalars[0].entity_indices, [0, 2]);
    assert_eq!(lane.references.len(), 2);
    assert_eq!(lane.references[0].object_index, 0);
    assert_eq!(lane.references[1].object_index, 2);
    assert!(lane
        .references
        .iter()
        .all(|reference| reference.kind == crate::records::FeatureInputOperandKind::D6));
    assert_eq!(lane.scalars[0].operands.len(), 2);
    assert_eq!(lane.scalars[0].operands[0].entity_index, 0);
    assert_eq!(lane.scalars[0].operands[1].entity_index, 2);
    assert_eq!(
        lane.scalars[0].operands[0].reference_ref,
        lane.references[0].id
    );
    assert_eq!(
        lane.scalars[0].operands[1].reference_ref,
        lane.references[1].id
    );
    assert!(lane.scalars[0]
        .operands
        .iter()
        .all(|operand| operand.kind == crate::records::FeatureInputOperandKind::D6));
    assert_eq!(lane.relation_bindings.len(), 1);
    assert_eq!(
        lane.relation_bindings[0].family,
        crate::records::FeatureInputRelationFamily::PointPointDistance
    );
    assert_eq!(lane.relation_bindings[0].class_ref, lane.classes[3].id);
    assert_eq!(lane.relation_bindings[0].scalar_ref, lane.scalars[0].id);
    assert_eq!(lane.relation_bindings[0].feature_ref, None);
    assert_eq!(
        lane.scalars[0].role,
        crate::records::FeatureInputScalarRole::Driving
    );
    assert!(lane
        .classes
        .iter()
        .enumerate()
        .all(|(ordinal, class)| class.ordinal == ordinal as u32));
    assert!(lane
        .sketch_entities
        .windows(2)
        .all(|entities| entities[0].offset < entities[1].offset));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.ordinal == ordinal as u32));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.local_id == Some(ordinal as u32 + 1)));
    assert!(lane
        .sketch_entities
        .iter()
        .enumerate()
        .all(|(ordinal, entity)| entity.state_value == Some(ordinal as f64 + 1.0)));
    let by_ordinal = |ordinal| {
        lane.sketch_entities
            .iter()
            .find(|entity| entity.ordinal == ordinal)
            .unwrap()
    };
    assert_eq!(by_ordinal(0).kind, SketchInputKind::Point);
    assert_eq!(
        by_ordinal(1).kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Distance)
    );
    assert_eq!(
        by_ordinal(2).kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Angle)
    );
    assert_eq!(
        by_ordinal(3).kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Radius)
    );
    assert_eq!(
        by_ordinal(4).kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Coincident)
    );
    update_sldprt_native(&mut decoded.ir, |native| {
        let entity = native.feature_input_lanes[0]
            .sketch_entities
            .iter_mut()
            .find(|entity| entity.ordinal == 1)
            .unwrap();
        entity.kind = SketchInputKind::Native(5);
        entity.state_value = Some(12.5);
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let scan = container::scan_bytes(&encoded);
    assert_eq!(
        scan.blocks
            .iter()
            .filter(|block| block.section.as_deref() == Some("Contents/Config-0-ResolvedFeatures"))
            .count(),
        1
    );
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let entity = &sldprt_native(&regenerated.ir).feature_input_lanes[0].sketch_entities[1];
    assert_eq!(
        entity.kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Vertical)
    );
    assert_eq!(entity.state_value, Some(12.5));
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_input_lanes[0]
            .sketch_entities
            .iter()
            .find(|entity| entity.ordinal == 1)
            .unwrap()
            .kind,
        SketchInputKind::Relation(crate::records::SketchRelationKind::Vertical)
    );
}

#[test]
fn decode_retains_e1_feature_input_operands() {
    let mut payload = resolved_features_payload(&[0, 1, 2]);
    let mut replacements = 0;
    for index in 0..payload.len().saturating_sub(1) {
        if payload[index..index + 2] == [0xd6, 0x80] {
            payload[index] = 0xe1;
            replacements += 1;
        }
    }
    assert_eq!(replacements, 2);
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    let scalar = &native.feature_input_lanes[0].scalars[0];
    assert!(native.feature_input_lanes[0]
        .references
        .iter()
        .all(|reference| reference.kind == crate::records::FeatureInputOperandKind::E1));
    assert!(scalar.entity_indices.is_empty());
    assert_eq!(
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .collect::<Vec<_>>(),
        [
            (crate::records::FeatureInputOperandKind::E1, 0),
            (crate::records::FeatureInputOperandKind::E1, 2),
        ]
    );
}

#[test]
fn decode_resolves_feature_input_operands_by_compatible_ordinal() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0, 0, 2], &["Sketch1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature_ref = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .and_then(|feature| feature.native_ref.as_deref())
        .expect("native sketch feature");
    let native = sldprt_native(&decoded.ir);
    let lane = &native.feature_input_lanes[0];
    let scalar = &lane.scalars[0];
    assert!(lane
        .references
        .iter()
        .all(|reference| reference.feature_ref.as_deref() == Some(feature_ref)));
    assert_eq!(scalar.operands[0].entity_index, 0);
    assert_eq!(
        scalar.operands[0].entity_ref.as_deref(),
        Some(lane.sketch_entities[0].id.as_str())
    );
    assert_eq!(scalar.operands[1].entity_index, 2);
    assert_eq!(scalar.operands[1].entity_ref, None);

    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_uses_operand_tag_to_disambiguate_marker_kind() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let mut payload = resolved_features_payload_with_names(&[0, 1, 2], &["Sketch1", "D1"]);
    for offset in 0..payload.len().saturating_sub(1) {
        if payload[offset..offset + 2] == [0xd6, 0x80] {
            payload[offset..offset + 2].copy_from_slice(&0x837bu16.to_le_bytes());
        }
    }
    let marker = [0xff, 0xff, 0x1f, 0x00, 0x03];
    let first = payload
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("first marker");
    payload[first + 88..first + 92].copy_from_slice(&2u32.to_le_bytes());
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let lane = &sldprt_native(&decoded.ir).feature_input_lanes[0];
    let operand = &lane.scalars[0].operands[1];
    assert_eq!(operand.entity_index, 2);
    assert_eq!(
        operand.entity_ref.as_deref(),
        Some(lane.sketch_entities[0].id.as_str())
    );
}

#[test]
fn decode_resolves_each_marker_link_by_trailing_local_id() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let mut payload = resolved_features_payload_with_names(&[4, 1, 2], &["Sketch1", "D1"]);
    let marker = [0xff, 0xff, 0x1f, 0x00, 0x03];
    let offset = payload
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("first sketch marker");
    payload[offset + 64..offset + 66].copy_from_slice(&2u16.to_le_bytes());
    payload[offset + 66..offset + 68].copy_from_slice(&99u16.to_le_bytes());
    payload[offset + 68..offset + 70].copy_from_slice(&1u16.to_le_bytes());
    payload[offset + 70..offset + 72].fill(0);
    payload[offset + 72..offset + 80].copy_from_slice(&(-1.0f64).to_le_bytes());
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    let lane = &native.feature_input_lanes[0];
    assert_eq!(
        lane.sketch_entities
            .iter()
            .map(|entity| entity.local_id)
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
    assert_eq!(lane.sketch_entities[0].link_selector, Some(1));
    assert_eq!(
        lane.sketch_entities[0]
            .links
            .iter()
            .map(|link| (link.local_id, link.entity_ref.as_str()))
            .collect::<Vec<_>>(),
        [(2, lane.sketch_entities[1].id.as_str())]
    );
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn semantic_writer_rejects_edited_feature_input_class_index() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].classes[0].name = "sgOtherHandle".into();
    });
    assert!(crate::validate_native(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains("class index does not match")));

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("has edited class declarations"));
}

#[test]
fn semantic_writer_rewrites_feature_input_name_values() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].names[1].value = "Depth".into();
    });
    assert!(crate::validate_native(&decoded.ir).is_empty());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_input_lanes[0].names[1].value,
        "Depth"
    );
}

#[test]
fn semantic_writer_rejects_edited_feature_input_scalar_index() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0]);
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].scalars[0].value = 0.050;
    });
    assert!(crate::validate_native(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains("scalar index does not match")));

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("has edited named scalars"));
}

#[test]
fn semantic_writer_rejects_edited_sketch_marker_local_id() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0, 1]);
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].sketch_entities[0].local_id = Some(7);
    });
    assert!(crate::validate_native(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains("local object id does not match")));

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("inconsistent marker order"));
}

#[test]
fn semantic_writer_rejects_edited_sketch_marker_object_index() {
    let source = sldprt_with_body_and_resolved_features(&triangle_body(), &[0, 1]);
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_input_lanes[0].sketch_entities[0].object_index = Some(77);
    });
    assert!(crate::validate_native(&decoded.ir)
        .iter()
        .any(|finding| finding.message.contains("object index does not match")));

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("inconsistent marker order"));
}

#[test]
fn decode_projects_unambiguous_resolved_feature_parameter() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss-Extrude1"))
        .expect("projected extrusion feature");
    let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } = &feature.definition
    else {
        panic!("typed extrusion feature");
    };
    assert_eq!(
        extent,
        &cadmpeg_ir::features::ExtrudeExtent::OneSided {
            side: cadmpeg_ir::features::ExtrudeSide {
                termination: cadmpeg_ir::features::Termination::Blind {
                    length: cadmpeg_ir::features::Length(25.0),
                },
                draft: None,
                offset: None,
            }
        }
    );
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected D1 parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:scalar#")));
    let native = sldprt_native(&decoded.ir);
    let scalar = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| Some(scalar.id.as_str()) == parameter.native_ref.as_deref())
        .expect("parameter scalar");
    assert_eq!(scalar.feature_ref.as_deref(), feature.native_ref.as_deref());
    assert_eq!(
        native.feature_input_lanes[0].relation_bindings[0].scalar_ref,
        scalar.id
    );
    assert_eq!(
        native.feature_input_lanes[0].relation_bindings[0]
            .feature_ref
            .as_deref(),
        feature.native_ref.as_deref()
    );
}

#[test]
fn decode_projects_hyphenated_extrusion_operations() {
    for (kind, expected) in [
        ("Boss-Extrude", cadmpeg_ir::features::BooleanOp::Join),
        ("Cut-Extrude", cadmpeg_ir::features::BooleanOp::Cut),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            format!(
                "<Keywords><Extrusion Name=\"Extrude1\" Type=\"{kind}\"><Dimension Name=\"D1\">25</Dimension></Extrusion></Keywords>"
            )
            .as_bytes(),
        ));

        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
        ));
    }
}

#[test]
fn decode_binds_generic_extrusion_to_its_dissectable_sketch_child() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8" DissectableChildren="3"><Dimension Name="D1">25</Dimension></Extrusion><Sketch Name="Sketch1" Type="Sketch" id="3"/></Keywords>"#,
    ));
    let original = source.clone();

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let extrusion = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrude1"))
        .expect("projected extrusion feature");
    let sketch = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert_eq!(extrusion.dependencies, vec![sketch.id.clone()]);
    assert!(sketch.ordinal < extrusion.ordinal);
    assert!(matches!(
        &extrusion.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Feature(profile),
            ..
        } if profile == &sketch.id
    ));
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    assert_eq!(encoded, original);
}

#[test]
fn decode_projects_feature_input_extrusion_operations() {
    fn operation_payload(
        code: u32,
        object_id: u32,
        name: &str,
        class_name: &str,
        direct_class: bool,
        padding: usize,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&code.to_le_bytes());
        payload.extend(std::iter::repeat_n(0, padding));
        if direct_class {
            payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
            payload.extend_from_slice(&(class_name.len() as u16).to_le_bytes());
            payload.extend_from_slice(class_name.as_bytes());
        } else {
            payload.extend_from_slice(&0x84d8u16.to_le_bytes());
        }
        payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff]);
        payload.push(name.encode_utf16().count() as u8);
        for unit in name.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload
    }

    fn inline_operation_payload(family: u8, operation: u8, object_id: u32) -> Vec<u8> {
        let class_name = if family == 0x40 {
            "moExtrusion_c"
        } else {
            "moICE_c"
        };
        let mut payload = operation_payload(14, object_id, "Extrude1", class_name, true, 8);
        payload.truncate(payload.len() - 12);
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[family, 1, operation, 0x40]);
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.extend_from_slice(&[0; 4]);
        payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
        payload
    }

    for (code, expected, class_name, layouts) in [
        (
            3,
            cadmpeg_ir::features::BooleanOp::Join,
            "moICE_c",
            &[(true, 8), (true, 4), (false, 8), (false, 4)][..],
        ),
        (
            11,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4), (false, 8), (false, 4)][..],
        ),
        (
            1,
            cadmpeg_ir::features::BooleanOp::Join,
            "moExtrusion_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            1,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            2,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
        (
            10,
            cadmpeg_ir::features::BooleanOp::Cut,
            "moICE_c",
            &[(true, 8), (true, 4)][..],
        ),
    ] {
        for &(direct_class, padding) in layouts {
            let mut source = sldprt_with_body(&triangle_body());
            source.extend(make_block(
                0x42,
                "Contents/Keywords",
                br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
            ));
            source.extend(make_block(
                0x45,
                "Contents/Config-0-ResolvedFeatures",
                &operation_payload(code, 8, "Extrude1", class_name, direct_class, padding),
            ));

            let decoded = SldprtCodec
                .decode(&mut Cursor::new(source), &DecodeOptions::default())
                .unwrap();
            let feature = decoded
                .ir
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some("Extrude1"))
                .expect("projected extrusion feature");
            assert!(
                matches!(
                    &feature.definition,
                    cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
                ),
                "code {code}, class {class_name}, direct {direct_class}, padding {padding}: {:?}",
                feature.definition
            );
        }
    }

    for code in [0, 4, 20] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
        ));
        source.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &operation_payload(code, 8, "Extrude1", "moICE_c", true, 8),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude {
                op: cadmpeg_ir::features::BooleanOp::Unresolved,
                ..
            }
        ));
    }

    for (family, operation, expected) in [
        (0x40, 0, cadmpeg_ir::features::BooleanOp::Join),
        (0xca, 2, cadmpeg_ir::features::BooleanOp::Cut),
    ] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Extrusion Name="Extrude1" Type="Extrusion" id="8"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
        ));
        source.extend(make_block(
            0x45,
            "Contents/Config-0-ResolvedFeatures",
            &inline_operation_payload(family, operation, 8),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Extrude1"))
            .expect("projected extrusion feature");
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Extrude { op, .. } if *op == expected
        ));
    }
}

#[test]
fn semantic_writer_updates_linked_resolved_feature_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "D1")
        .expect("projected D1 parameter");
    parameter.expression = "50mm".into();
    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::features::Length(50.0),
    ));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("regenerated D1 parameter");
    assert_eq!(parameter.expression, "50mm");
    let native_ref = parameter.native_ref.as_deref().expect("linked scalar");
    let native = sldprt_native(&regenerated.ir);
    let scalar = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.scalars)
        .find(|scalar| scalar.id == native_ref)
        .expect("regenerated scalar");
    assert_eq!(scalar.value, 0.05);
}

#[test]
fn semantic_writer_updates_resolved_scalar_from_feature_edit() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload(&[0]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let cadmpeg_ir::features::FeatureDefinition::Extrude { extent, .. } =
        &mut decoded.ir.model.features[0].definition
    else {
        panic!("typed extrusion feature");
    };
    *extent = cadmpeg_ir::features::ExtrudeExtent::OneSided {
        side: cadmpeg_ir::features::ExtrudeSide {
            termination: cadmpeg_ir::features::Termination::Blind {
                length: cadmpeg_ir::features::Length(50.0),
            },
            draft: None,
            offset: None,
        },
    };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &regenerated.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Blind {
                        length: cadmpeg_ir::features::Length(50.0),
                    },
                    ..
                }
            },
            ..
        }
    ));
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_input_lanes[0].scalars[0].value,
        0.05
    );
}

#[test]
fn semantic_writer_types_resolved_relation_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Sketch1", "D1"]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "D1")
        .expect("projected D1 parameter");
    parameter.expression = "0.5".into();
    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Real(0.5));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameter = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("regenerated D1 parameter");
    assert_eq!(parameter.expression, "500mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(500.0)
        ))
    );
    let native_ref = parameter.native_ref.as_deref().expect("linked scalar");
    let native = sldprt_native(&regenerated.ir);
    let scalar = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.scalars)
        .find(|scalar| scalar.id == native_ref)
        .expect("regenerated scalar");
    assert_eq!(scalar.value, 0.5);
}

#[test]
fn decode_does_not_project_ambiguous_resolved_feature_parameter() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude"/></Keywords>"#,
    ));
    let mut payload = resolved_features_payload(&[0]);
    payload.extend_from_slice(&[0x04, 0x80, 0xff, 0xfe, 0xff, 2]);
    payload.extend_from_slice(&[b'D', 0, b'1', 0]);
    payload.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
    ]);
    payload.extend_from_slice(&0.050f64.to_le_bytes());
    payload.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
    ]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .ir
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.name == "D1"));
}

#[test]
fn decode_projects_unambiguous_resolved_sketch_parameter() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Sketch1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch { .. }
    ));
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected sketch D1 parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:scalar#")));
}

#[test]
fn decode_projects_owned_native_sketch_relation() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    let cadmpeg_ir::features::FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch),
        ..
    } = &feature.definition
    else {
        panic!("bound sketch feature");
    };
    let native = sldprt_native(&decoded.ir);
    assert!(native.feature_input_lanes[0]
        .sketch_entities
        .iter()
        .all(|entity| entity.feature_ref.as_deref() == feature.native_ref.as_deref()));
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected relation parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let constraint = decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.is_some())
        .expect("projected native relation");
    assert_eq!(&constraint.sketch, sketch);
    assert!(constraint
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("sldprt:feature-input:relation-instance#")));
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            entities,
            parameter: Some(relation_parameter),
            operands,
            ..
        } if native_kind == "sgPntPntDist"
            && entities.is_empty()
            && relation_parameter == &parameter.id
            && operands.len() == 2
            && operands[0].native_kind == "d6"
            && operands[0].object_index == 0
            && operands[0].native_ref.is_some()
            && operands[1].native_kind == "d6"
            && operands[1].object_index == 2
            && operands[1].native_ref.is_none()
    ));
    let findings = cadmpeg_ir::validate(&decoded.ir, Vec::new()).findings;
    assert!(findings.is_empty(), "{findings:#?}");
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn native_store_rejects_missing_sketch_marker_feature_owner() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    native.feature_input_lanes[0]
        .sketch_entities
        .last_mut()
        .expect("sketch marker")
        .feature_ref = Some("sldprt:history:feature#missing".into());

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(error
        .to_string()
        .contains("inconsistent lane or feature ownership"));
}

#[test]
fn native_store_rejects_edited_history_feature_class() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Round" Type="Fillet" id="41"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 41)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    native.feature_histories[0].features[0].input_class = Some("moRefPlane_c".into());

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(error
        .to_string()
        .contains("feature classes do not match the feature-input index"));
}

#[test]
fn native_store_rejects_missing_sketch_marker_local_link() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    let entity = &mut native.feature_input_lanes[0].sketch_entities[0];
    entity.links = vec![crate::records::SketchInputLink {
        local_id: 7,
        entity_ref: "sldprt:feature-input:sketch-entity#missing".into(),
    }];
    entity.link_selector = Some(0);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(error.to_string().contains("missing local-link target"));
}

#[test]
fn native_store_preserves_midpoint_with_two_point_markers() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    let entities = &mut native.feature_input_lanes[0].sketch_entities;
    let owner = entities[0].feature_ref.clone();
    let point_id = entities[1].id.clone();
    let second_point_id = entities[2].id.clone();
    entities[1].feature_ref = owner.clone();
    entities[1].local_id = Some(7);
    entities[1].kind = crate::records::SketchInputKind::Point;
    entities[2].feature_ref = owner;
    entities[2].local_id = Some(8);
    entities[2].kind = crate::records::SketchInputKind::ConstrainedPoint;
    entities[0].kind =
        crate::records::SketchInputKind::Relation(crate::records::SketchRelationKind::Midpoint);
    entities[0].links = vec![
        crate::records::SketchInputLink {
            local_id: 7,
            entity_ref: point_id,
        },
        crate::records::SketchInputLink {
            local_id: 8,
            entity_ref: second_point_id,
        },
    ];
    entities[0].link_selector = Some(0);
    for scalar in &mut native.feature_input_lanes[0].scalars {
        for operand in &mut scalar.operands {
            operand.entity_ref = None;
        }
    }
    for relation in &mut native.feature_input_lanes[0].relation_instances {
        for operand in &mut relation.operands {
            operand.entity_ref = None;
        }
    }

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).unwrap();
    let stored = crate::native::SldprtNative::load(&namespace).unwrap();
    assert_eq!(
        stored.feature_input_lanes[0].sketch_entities[0].links.len(),
        2
    );
}

#[test]
fn decode_groups_compact_relation_scalar_pair() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
        panic!("one compact relation instance");
    };
    assert_eq!(relation.scalar_refs.len(), 2);
    let driving = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| scalar.role == crate::records::FeatureInputScalarRole::Driving)
        .expect("driving scalar");
    let display = native.feature_input_lanes[0]
        .scalars
        .iter()
        .find(|scalar| scalar.role == crate::records::FeatureInputScalarRole::Display)
        .expect("display scalar");
    assert_eq!(
        relation.parameter_scalar_ref.as_deref(),
        Some(driving.id.as_str())
    );
    assert_eq!(
        relation.display_scalar_ref.as_deref(),
        Some(display.id.as_str())
    );
    assert_eq!(relation.operands.len(), 2);
    assert_eq!(relation.operands[0].entity_index, 0);
    assert_eq!(relation.operands[1].entity_index, 2);

    let constraint = decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
        .expect("projected compact relation");
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            parameter: Some(parameter),
            ..
        } if native_kind == "sgPntPntDist"
            && decoded.ir.model.parameters.iter().any(|candidate| {
                &candidate.id == parameter
                    && candidate.native_ref.as_deref() == Some(driving.id.as_str())
            })
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_starts_another_relation_after_two_repeated_operand_scalars() {
    let mut source = sldprt_with_tagged_compact_relation_names(
        &triangle_body(),
        "sgPntPntDist",
        [[0xd6, 0x80]; 2],
        &["Sketch1", "D1", "D2", "D3"],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    assert_eq!(native.feature_input_lanes[0].relation_instances.len(), 2);
    assert_eq!(
        native.feature_input_lanes[0]
            .relation_instances
            .iter()
            .map(|relation| relation.scalar_refs.len())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
}

#[test]
fn decode_groups_native_tagged_point_line_relations() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgPntLineDist",
        [[0x7b, 0x83], [0x86, 0x83]],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving point-line parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let native = sldprt_native(&decoded.ir);
    let lane = &native.feature_input_lanes[0];
    assert_eq!(lane.references.len(), 4);
    assert!(lane
        .references
        .iter()
        .enumerate()
        .all(|(ordinal, reference)| {
            reference.kind
                == crate::records::FeatureInputOperandKind::Native(if ordinal % 2 == 0 {
                    0x837b
                } else {
                    0x8386
                })
        }));
    let [relation] = lane.relation_instances.as_slice() else {
        panic!("one point-line relation instance");
    };
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::PointLineDistance
    );
    let constraint = decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
        .expect("projected point-line relation");
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native {
            native_kind,
            operands,
            ..
        } if native_kind == "sgPntLineDist"
            && operands[0].native_kind == "7b83"
            && operands[1].native_kind == "8683"
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_uses_relation_units_for_bare_integer_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgPntPntVertDist",
        [[0xcb, 0x8d]; 2],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">25</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving vertical-distance parameter");
    assert_eq!(parameter.expression, "25");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(25.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_uses_relation_units_for_boolean_shaped_dimensions() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let mut source = sldprt_with_tagged_compact_relation_scalar(
        &triangle_body(),
        "sgPntPntDist",
        [[0xd6, 0x80]; 2],
        0.001,
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">1</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving distance parameter");
    assert_eq!(parameter.expression, "1");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(1.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_uses_relation_units_for_bare_integer_angles() {
    use cadmpeg_ir::features::{Angle, ParameterValue};

    let mut source =
        sldprt_with_tagged_compact_relation(&triangle_body(), "sgAnglDim", [[0xda, 0x8d]; 2]);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">25</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("driving angle parameter");
    assert_eq!(parameter.expression, "25");
    assert_eq!(parameter.value, Some(ParameterValue::Angle(Angle(0.025))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_groups_unary_circle_diameter_relations() {
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut source = sldprt_with_tagged_compact_relation(
        &triangle_body(),
        "sgCircleDim",
        [[0xfe, 0x83], [0, 0]],
    );
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"><Dimension Name="D2">&lt;MOD-DIAM&gt;25mm</Dimension></Sketch></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
        panic!("one circle-diameter relation instance");
    };
    assert_eq!(
        relation.family,
        crate::records::FeatureInputRelationFamily::CircleDiameter
    );
    assert_eq!(relation.operands.len(), 1);
    assert_eq!(
        relation.operands[0].kind,
        crate::records::FeatureInputOperandKind::Native(0x83fe)
    );
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .expect("diameter parameter");
    assert_eq!(
        relation.parameter_scalar_ref.as_deref(),
        parameter.native_ref.as_deref()
    );
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            constraint.native_ref.as_deref() == Some(relation.id.as_str())
                && matches!(
                    &constraint.definition,
                    SketchConstraintDefinition::Native {
                        native_kind,
                        parameter: Some(bound_parameter),
                        operands,
                        ..
                    } if native_kind == "sgCircleDim"
                        && bound_parameter == &parameter.id
                        && operands.len() == 1
                        && operands[0].native_kind == "fe83"
                )
        }));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();
}

#[test]
fn decode_groups_each_circle_dimension_operand_tag() {
    for tag in [
        [0xcc, 0x80],
        [0xfe, 0x83],
        [0xb6, 0x8a],
        [0x9d, 0x92],
        [0x69, 0xbd],
    ] {
        let mut source =
            sldprt_with_tagged_compact_relation(&triangle_body(), "sgCircleDim", [tag, [0, 0]]);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(&decoded.ir);
        let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
            panic!("one circle-diameter relation for tag {tag:02x?}");
        };
        assert_eq!(
            relation.family,
            crate::records::FeatureInputRelationFamily::CircleDiameter
        );
        let [operand] = relation.operands.as_slice() else {
            panic!("one circle-diameter operand for tag {tag:02x?}");
        };
        assert_eq!(
            operand.kind,
            crate::records::FeatureInputOperandKind::Native(u16::from_le_bytes(tag))
        );
        assert_eq!(operand.entity_index, 0);
    }
}

#[test]
fn decode_uses_declaration_to_disambiguate_native_relation_tags() {
    let cases = [
        (
            "sgPntPntDist",
            [0x7b, 0x83],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgLLDist",
            [0x86, 0x83],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgPntPntDist",
            [0x7c, 0xbc],
            crate::records::FeatureInputRelationFamily::PointPointDistance,
        ),
        (
            "sgLLDist",
            [0x87, 0xbc],
            crate::records::FeatureInputRelationFamily::LineLineDistance,
        ),
        (
            "sgPntPntHorDist",
            [0xcb, 0x8d],
            crate::records::FeatureInputRelationFamily::PointPointHorizontalDistance,
        ),
        (
            "sgPntPntVertDist",
            [0xcb, 0x8d],
            crate::records::FeatureInputRelationFamily::PointPointVerticalDistance,
        ),
        (
            "sgAnglDim",
            [0xda, 0x8d],
            crate::records::FeatureInputRelationFamily::Angle,
        ),
    ];
    for (class, tag, family) in cases {
        let mut source = sldprt_with_tagged_compact_relation(&triangle_body(), class, [tag; 2]);
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let parameter = decoded
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "D2")
            .expect("driving relation parameter");
        if family == crate::records::FeatureInputRelationFamily::Angle {
            assert_eq!(parameter.expression, "0.025rad");
            assert_eq!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Angle(
                    cadmpeg_ir::features::Angle(0.025)
                ))
            );
        } else {
            assert_eq!(parameter.expression, "25mm");
            assert_eq!(
                parameter.value,
                Some(cadmpeg_ir::features::ParameterValue::Length(
                    cadmpeg_ir::features::Length(25.0)
                ))
            );
        }
        let native = sldprt_native(&decoded.ir);
        let [relation] = native.feature_input_lanes[0].relation_instances.as_slice() else {
            panic!("one native-tagged relation instance for {class}");
        };
        assert_eq!(relation.family, family);
        assert!(relation.operands.iter().all(|operand| operand.kind
            == crate::records::FeatureInputOperandKind::Native(u16::from_le_bytes(tag))));
        assert!(decoded
            .ir
            .model
            .sketch_constraints
            .iter()
            .any(|constraint| {
                constraint.native_ref.as_deref() == Some(relation.id.as_str())
                    && matches!(
                        &constraint.definition,
                        cadmpeg_ir::sketches::SketchConstraintDefinition::Native {
                            native_kind,
                            ..
                        } if native_kind == class
                    )
            }));
    }
}

#[test]
fn native_store_rejects_relation_scalar_owner_disagreement() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    assert!(native.feature_input_lanes[0].relation_bindings[0]
        .feature_ref
        .is_some());
    native.feature_input_lanes[0].relation_bindings[0].feature_ref = None;

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(error
        .to_string()
        .contains("disagrees with its scalar owner"));
}

#[test]
fn native_store_rejects_nonlocal_relation_scalar_groups() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    let duplicate = native.feature_input_lanes[0].relation_instances[0].scalar_refs[0].clone();
    native.feature_input_lanes[0].relation_instances[0]
        .scalar_refs
        .push(duplicate);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("relation instance")
            && error.to_string().contains("inconsistent ownership")
    );
}

#[test]
fn native_load_rejects_nonadjacent_duplicate_relation_scalars() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut namespace = decoded
        .ir
        .native
        .namespace("sldprt")
        .expect("SLDPRT namespace")
        .clone();
    let mut relations: Vec<crate::records::FeatureInputRelationInstance> = namespace
        .arena_as("feature_input_relation_instances")
        .unwrap();
    let relation = relations.first_mut().expect("relation instance");
    assert_eq!(relation.scalar_refs.len(), 2);
    relation.scalar_refs.push(relation.scalar_refs[0].clone());
    namespace
        .set_arena("feature_input_relation_instances", &relations)
        .unwrap();

    let error = crate::native::SldprtNative::load(&namespace).unwrap_err();
    assert!(error.to_string().contains("relation instance"));
}

#[test]
fn native_store_rejects_relation_instance_operand_disagreement() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    native.feature_input_lanes[0].relation_instances[0].operands[0].entity_index += 1;

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("relation instance")
            && error.to_string().contains("inconsistent ownership")
    );
}

#[test]
fn native_store_rejects_inconsistent_scalar_marker_target() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0, 0, 2], &["Sketch1", "D1"]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    let wrong_target = native.feature_input_lanes[0].sketch_entities[0].id.clone();
    native.feature_input_lanes[0].scalars[0].operands[1].entity_ref = Some(wrong_target.clone());
    native.feature_input_lanes[0].relation_instances[0].operands[1].entity_ref = Some(wrong_target);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(error.to_string().contains("inconsistent sketch marker"));
}

#[test]
fn native_store_accepts_duplicate_local_ids_for_scalar_ordinals() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0, 0, 2], &["Sketch1", "D1"]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut native = sldprt_native(&decoded.ir);
    let lane = &mut native.feature_input_lanes[0];
    assert_eq!(lane.scalars[0].operands[0].entity_index, 0);
    assert!(lane.scalars[0].operands[0].entity_ref.is_some());
    lane.sketch_entities[1].local_id = lane.sketch_entities[0].local_id;

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).unwrap();
}

#[test]
fn decode_and_validate_compact_delete_body_selection() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Body-Delete/Keep 1" Type="Body-Delete/Keep " id="41"/></Keywords>"#,
    ));
    let mut payload =
        resolved_feature_classes_with_ids(&[("moDeleteBody_c", "Body-Delete/Keep 1", 41)]);
    payload.extend([0xff, 0xff, 0x01, 0x00]);
    payload.extend(18u16.to_le_bytes());
    payload.extend(b"moDeleteBodyData_c");
    payload.extend([0x08, 0x00]);
    let token = 0x89a4u16;
    let mut state = [0u8; 83];
    state[0..2].copy_from_slice(&token.to_le_bytes());
    state[2..11].copy_from_slice(&[0x2b, 0x80, 0x02, 0, 0, 0, 0, 0, 0]);
    state[11..15].copy_from_slice(&287u32.to_le_bytes());
    state[15..19].copy_from_slice(&287u32.to_le_bytes());
    state[47..63].fill(0xff);
    payload.extend(state);
    payload.extend([0x30, 0x80]);
    payload.extend(1u32.to_le_bytes());
    payload.extend([0; 4]);
    payload.extend(11000u32.to_le_bytes());
    payload.extend([0; 8]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(287u32.to_le_bytes());
    payload.extend(115u32.to_le_bytes());
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend([0; 12]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("body delete/keep feature(s)")));
    let mut native = sldprt_native(&decoded.ir);
    let [selection] = native.feature_input_lanes[0].body_selections.as_slice() else {
        panic!("one compact body selection");
    };
    assert_eq!(selection.local_body_ids, [287, 115]);
    assert_eq!(selection.body_state_ids, [287]);
    assert_eq!(
        selection.mode,
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected)
    );

    let mut legacy = decoded.ir.native.namespace("sldprt").unwrap().clone();
    legacy.version = 5;
    for record in legacy
        .arenas
        .get_mut("feature_input_body_selections")
        .unwrap()
    {
        record.fields.remove("mode");
    }
    let migrated = crate::native::SldprtNative::load(&legacy).unwrap();
    assert_eq!(
        migrated.feature_input_lanes[0].body_selections[0].mode,
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected)
    );
    assert!(selection.feature_ref.starts_with("sldprt:history:feature#"));
    let delete_feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body-Delete/Keep 1"))
        .expect("delete-body feature");
    assert!(matches!(
        &delete_feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, mode }
            if bodies == &cadmpeg_ir::features::BodySelection::Local {
                bodies: vec!["287".into(), "115".into()],
                native: "sldprt:feature-input:body-ids:287,115".into(),
            } && *mode == cadmpeg_ir::features::BodyRetentionMode::DeleteSelected
    ));
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();

    decoded
        .ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Body-Delete/Keep 1"))
        .expect("delete-body feature")
        .name = Some("Renamed Delete Body".into());
    let mut renamed = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut renamed)
        .unwrap();
    let renamed = SldprtCodec
        .decode(&mut Cursor::new(renamed), &DecodeOptions::default())
        .unwrap();
    let renamed_native = sldprt_native(&renamed.ir);
    assert!(!renamed_native.feature_histories[0].features[0]
        .properties
        .contains_key("Bodies"));
    assert_eq!(
        renamed_native.feature_input_lanes[0].body_selections[0].local_body_ids,
        [287, 115]
    );

    {
        let delete_feature = decoded
            .ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Renamed Delete Body"))
            .expect("delete-body feature");
        let cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, .. } =
            &mut delete_feature.definition
        else {
            panic!("typed delete-body feature");
        };
        *bodies =
            cadmpeg_ir::features::BodySelection::Native("sldprt:feature-input:body-ids:287".into());
    }
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("changes a compact body selection"));

    {
        let delete_feature = decoded
            .ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.name.as_deref() == Some("Renamed Delete Body"))
            .expect("delete-body feature");
        let cadmpeg_ir::features::FeatureDefinition::DeleteBody { bodies, mode } =
            &mut delete_feature.definition
        else {
            unreachable!("typed delete-body feature");
        };
        *bodies = cadmpeg_ir::features::BodySelection::Local {
            bodies: vec!["287".into(), "115".into()],
            native: "sldprt:feature-input:body-ids:287,115".into(),
        };
        *mode = cadmpeg_ir::features::BodyRetentionMode::KeepSelected;
    }
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("changes a compact body retention mode"));

    native.feature_input_lanes[0].body_selections[0]
        .body_state_ids
        .push(287);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
    native.feature_input_lanes[0].body_selections[0].body_state_ids = vec![287];

    native.feature_input_lanes[0].body_selections[0].mode =
        Some(cadmpeg_ir::features::BodyRetentionMode::KeepSelected);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
    native.feature_input_lanes[0].body_selections[0].mode =
        Some(cadmpeg_ir::features::BodyRetentionMode::DeleteSelected);

    native.feature_input_lanes[0].body_selections[0].local_body_ids[0] = 288;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    let error = native.store(&mut namespace).unwrap_err();
    assert!(
        error.to_string().contains("body selection")
            && error.to_string().contains("inconsistent ownership")
    );
}

#[test]
fn decode_extracts_pmi_semantic_dimension() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload(),
    ));

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&decoded.ir);
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one PMI dimension");
    };
    assert_eq!(dimension.guid, "01234567-89ab-cdef-0123-456789abcdef");
    assert_eq!(dimension.cad_text, "D1@Sketch1");
    assert_eq!(dimension.subtype, "Linear");
    assert_eq!(dimension.value, 0.025);
    assert_eq!(dimension.precision, 3);
    assert_eq!(dimension.display_text.as_deref(), Some("25.000 mm"));
    assert!(dimension.basic);
    assert!(!dimension.inspection);
    assert!(dimension.reference_only);
    assert_eq!(
        decoded.source_fidelity.annotations.provenance[&dimension.id].offset,
        dimension.offset
    );
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("PMI-backed parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    let semantic = parameter.pmi.as_ref().expect("PMI semantics");
    assert_eq!(
        semantic.subtype,
        cadmpeg_ir::features::PmiDimensionSubtype::Linear
    );
    assert_eq!(semantic.native_ref, dimension.id);
    SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap();

    let parameter = decoded
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "D1")
        .expect("editable PMI-backed parameter");
    parameter.expression = "50mm".into();
    parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::features::Length(50.0),
    ));
    let semantic = parameter.pmi.as_mut().expect("editable PMI semantics");
    semantic.precision = 4;
    semantic.display_text = Some("50.000 mm".into());
    semantic.basic = false;
    semantic.inspection = true;
    semantic.reference_only = false;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = sldprt_native(&regenerated.ir);
    let [dimension] = native.pmi_dimensions.as_slice() else {
        panic!("one regenerated PMI dimension");
    };
    assert_eq!(dimension.value, 0.05);
    assert_eq!(dimension.precision, 4);
    assert_eq!(dimension.display_text.as_deref(), Some("50.000 mm"));
    assert!(!dimension.basic);
    assert!(dimension.inspection);
    assert!(!dimension.reference_only);
}

#[test]
fn decode_reports_unbound_pmi_semantic_dimension() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_for("D1@MissingFeature"),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert!(decoded.report.losses.iter().any(|loss| {
        loss.message
            == "1 semantic dimension record(s) are not bound to parameters; 0 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn decode_uses_pmi_dimension_to_project_sparse_extrusion() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ProfileRef, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="Localized" id="42"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moExtrusion_c", "Boss", 42)]),
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload_for("D1@Boss"),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir.model.features[0].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(25.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Unresolved,
            ..
        }
    ));
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&decoded.ir.model.features[0].id))
        .expect("PMI extrusion parameter");
    assert_eq!(parameter.name, "D1");
    assert_eq!(parameter.expression, "25mm");
    assert!(parameter.pmi.is_some());
}

#[test]
fn decode_applies_owned_feature_units_to_resolved_scalar() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round1" Type="Fillet"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Round1", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .expect("projected fillet feature");
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected D1 parameter");
    assert_eq!(parameter.expression, "25mm");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(25.0)
        ))
    );
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_preserves_configuration_local_parameter_values() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue, RadiusSpec};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Large"/><Fillet Name="Round1" Type="Fillet"><Dimension Name="D1">30mm</Dimension><Dimension Name="D2">D1 * 2</Dimension></Fillet></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names_relation_and_scalar(
            &[0],
            &["Round1", "D1"],
            "sgPntPntDist",
            0.025,
        ),
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-1-ResolvedFeatures",
        &resolved_features_payload_with_names_relation_and_scalar(
            &[0],
            &["Round1", "D1"],
            "sgPntPntDist",
            0.050,
        ),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.report.losses.iter().any(|loss| loss
        .message
        .contains("parameter expression(s) cannot regenerate")));
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    let dependent = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D2")
        .unwrap();
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(30.0))));
    assert_eq!(parameter.native_ref, None);
    assert_eq!(
        decoded.ir.model.configurations[0]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(25.0)))
    );
    assert_eq!(
        decoded.ir.model.configurations[1]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );
    assert_eq!(
        decoded.ir.model.configurations[0]
            .parameter_values
            .get(&dependent.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );
    assert_eq!(
        decoded.ir.model.configurations[1]
            .parameter_values
            .get(&dependent.id),
        Some(&ParameterValue::Length(Length(100.0)))
    );
    let round_trip =
        cadmpeg_ir::CadIr::from_json(&serde_json::to_string(&decoded.ir).unwrap()).unwrap();
    assert_eq!(
        round_trip.model.configurations[1]
            .parameter_values
            .get(&parameter.id),
        Some(&ParameterValue::Length(Length(50.0)))
    );

    let parameter_id = parameter.id.clone();
    let dependent_id = dependent.id.clone();
    let feature_id = parameter.owner.clone();
    let mut incoherent = decoded.ir.clone();
    incoherent.model.configurations[1]
        .parameter_values
        .insert(parameter_id.clone(), ParameterValue::Length(Length(75.0)));
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &incoherent,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("configuration parameter values are inconsistent with their expressions"));

    let mut edited = decoded.ir.clone();
    edited.model.configurations[1]
        .parameter_values
        .insert(parameter_id.clone(), ParameterValue::Length(Length(75.0)));
    edited.model.configurations[1]
        .parameter_values
        .insert(dependent_id, ParameterValue::Length(Length(150.0)));
    let FeatureDefinition::Fillet { groups, .. } = &mut edited.model.configurations[1]
        .feature_states
        .get_mut(feature_id.as_ref().expect("feature-owned parameter"))
        .unwrap()
        .definition
    else {
        panic!("configuration fillet state");
    };
    groups[0].radius = RadiusSpec::Constant {
        radius: Length(75.0),
    };

    let mut conflicting = edited.clone();
    update_sldprt_native(&mut conflicting, |native| {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.configuration.as_deref() == Some("1"))
            .unwrap();
        let scalar = &mut lane.scalars[0];
        scalar.value = 0.060;
        let offset = usize::try_from(scalar.offset).unwrap();
        lane.native_payload[offset..offset + 8].copy_from_slice(&0.060f64.to_le_bytes());
    });
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &conflicting,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting neutral and native SLDPRT configuration design-state edits"));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&edited, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let regenerated_parameter = regenerated
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    let regenerated_feature = regenerated
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Round1"))
        .unwrap();
    assert_eq!(
        regenerated.ir.model.configurations[1]
            .parameter_values
            .get(&regenerated_parameter.id),
        Some(&ParameterValue::Length(Length(75.0)))
    );
    assert!(matches!(
        regenerated.ir.model.configurations[1].feature_states[&regenerated_feature.id].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: RadiusSpec::Constant {
                radius: Length(75.0)
            },
            ..
        }])
    ));
}

#[test]
fn decode_separates_document_expression_from_evaluated_feature_scalar() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Length, ParameterValue,
        Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="42"><Dimension Name="D1">2.5</Dimension></Extrusion></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_features_payload_with_names(&[0], &["Boss", "D1"]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Boss"))
        .expect("projected extrusion");
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(25.0)
                    },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }
    ));
    let parameter = decoded
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "D1")
        .expect("projected D1 parameter");
    assert_eq!(parameter.expression, "2.5");
    assert_eq!(parameter.value, Some(ParameterValue::Length(Length(25.0))));
    assert!(parameter.native_ref.is_some());
}

#[test]
fn decode_projects_nested_feature_input_profile_as_a_sketch() {
    use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchGeometry, SketchLocus};

    let source = sldprt_with_nested_sketch_profile(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(decoded.ir.model.sketches.len(), 1);
    assert_eq!(decoded.ir.model.sketch_entities.len(), 3);
    assert_eq!(decoded.ir.model.sketch_constraints.len(), 3);
    let sketch = &decoded.ir.model.sketches[0];
    assert_eq!(sketch.configuration.as_deref(), Some("0"));
    let (origin, normal, _) = sketch
        .resolved_placement()
        .expect("resolved sketch placement");
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(sketch.profiles.len(), 1);
    assert_eq!(sketch.profiles[0].len(), 3);
    assert!(decoded
        .ir
        .model
        .sketch_entities
        .iter()
        .all(|entity| matches!(entity.geometry, SketchGeometry::Line { .. })));
    assert!(decoded.ir.model.sketch_entities.iter().all(|entity| {
        entity
            .native_ref
            .as_deref()
            .is_some_and(|id| id.contains(":sldprt:brep:edge#"))
            && entity.endpoint_refs.len() == 2
            && entity
                .endpoint_refs
                .iter()
                .all(|id| id.contains(":sldprt:brep:point#"))
    }));
    assert!(decoded
        .ir
        .model
        .sketch_constraints
        .iter()
        .all(|constraint| {
            matches!(
                &constraint.definition,
                SketchConstraintDefinition::CoincidentLoci { loci }
                    if loci.len() == 2
                        && loci.iter().all(|locus| matches!(
                            locus,
                            SketchLocus::Start(_) | SketchLocus::End(_)
                        ))
            )
        }));
    assert!(sketch.native_ref.as_deref().is_some_and(|native_ref| {
        native_ref.starts_with("sldprt:feature-input:resolved-features#")
    }));
    let validation = cadmpeg_ir::validate(&decoded.ir, Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
}

#[test]
fn decode_binds_profile_stream_by_feature_object_interval() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch = decoded
        .ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.name.as_deref() == Some("Sketch1"))
        .expect("named feature-input sketch");
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sketch history feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(id),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_sweep() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sketch1" Type="Sweep"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [sketch] = decoded.ir.model.sketches.as_slice() else {
        panic!("one enclosed sweep profile stream");
    };
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sweep history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Sweep {
            profile: Some(ProfileRef::Sketch(id)),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_sweep() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sketch1" Type="Sweep"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sweep history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Sweep { profile: None, .. }
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Sketch1" Type="Boss-Extrude"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [sketch] = decoded.ir.model.sketches.as_slice() else {
        panic!("one enclosed extrusion profile stream");
    };
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("extrusion history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(id),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_binds_configuration_sketch_state_after_geometry_projection() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default" id="0"/><Sketch Name="Sketch1" Type="Sketch" id="0"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert!(matches!(
        &decoded.ir.model.configurations[0].feature_states[&feature.id].definition,
        FeatureDefinition::Sketch {
            sketch: Some(configuration_sketch),
            ..
        } if decoded.ir.model.sketches.iter().any(|sketch| &sketch.id == configuration_sketch)
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Sketch1" Type="Boss-Extrude"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("extrusion history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            ..
        }
    ));
}

#[test]
fn semantic_writer_rejects_retained_sketch_constraint_edits() {
    use cadmpeg_ir::sketches::{SketchConstraint, SketchConstraintDefinition, SketchConstraintId};

    let source = sldprt_with_nested_sketch_profile(&triangle_body());
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch = decoded.ir.model.sketches[0].id.clone();
    let entity = decoded.ir.model.sketch_entities[0].id.clone();
    decoded.ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("synthetic:test:constraint#horizontal".into()),
        sketch,
        definition: SketchConstraintDefinition::Horizontal { entity },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    assert_ne!(
        decoded.ir.source.as_ref().unwrap().attributes["semantic_sha256"],
        crate::decode::semantic_hash(&decoded.ir)
    );

    let error = SldprtCodec
        .encode_with_source_fidelity(&decoded.ir, Some(&decoded.source_fidelity), &mut Vec::new())
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::NotImplemented(_)
    ));
    assert!(error
        .to_string()
        .contains("SLDPRT native sketch relation editing is not implemented"));
}

#[test]
fn decode_binds_unique_sketch_history_to_profile_consumers() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="21"/><Rib Name="Web" Type="Rib" id="22" Profile="21" Direction="0,1,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension></Rib></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch_id = decoded.ir.model.sketches[0].id.clone();
    assert!(decoded.ir.model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(value), ..
        } if value == &sketch_id
    )));
    assert!(decoded.ir.model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(ProfileRef::Sketch(value)),
                ..
            },
            ..
        } if value == &sketch_id
    )));
    let validation = cadmpeg_ir::validate(&decoded.ir, Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(round_trip.ir.model.features.iter().any(|feature| matches!(
        feature.definition,
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(_),
            ..
        }
    )));
}

#[test]
fn matching_numbered_sketch_alias_binds_the_base_geometry() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureId, ProfileRef,
        Termination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchId};

    let sketch_id = SketchId("sketch".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
            entity: cadmpeg_ir::sketches::SketchEntityId("sketch:entity".into()),
            reversed: false,
        }]],
        native_ref: None,
    };
    let neutral =
        |id: &str, name: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
            id: FeatureId(id.into()),
            ordinal: 0,
            name: Some(name.into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Sketch".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: Some(native_ref.into()),
        };
    let mut features = vec![
        neutral(
            "base",
            "Profile",
            "native-base",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "alias",
            "Profile<3>",
            "native-alias",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "different",
            "Profile<4>",
            "native-different",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "consumer",
            "Boss",
            "native-consumer",
            FeatureDefinition::Extrude {
                profile: ProfileRef::Native("native-alias".into()),
                direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Unresolved,
                        draft: None,
                        offset: None,
                    },
                },
                op: BooleanOp::Join,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        ),
    ];
    let native = |id: &str, name: &str, depth: &str| crate::records::Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: name.into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("Depth".into(), depth.into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: vec![crate::records::FeatureContent::Dimension("Depth".into())],
    };
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native("native-base", "Profile", "2mm"),
            native("native-alias", "Profile<3>", "2mm"),
            native("native-different", "Profile<4>", "3mm"),
        ],
    };

    crate::history::bind_unique_sketch_feature(&mut features, &[sketch], &[history]);

    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert_eq!(features[1].dependencies, vec![FeatureId("base".into())]);
    assert!(matches!(
        &features[2].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert!(matches!(
        &features[3].definition,
        FeatureDefinition::Extrude { profile: ProfileRef::Sketch(id), .. } if id == &sketch_id
    ));
    assert_eq!(features[3].dependencies, vec![FeatureId("base".into())]);
}

#[test]
fn decode_binds_multiple_sketch_history_nodes_by_exact_name() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, ProfileRef};

    let mut source = sldprt_with_nested_nurbs_sketches(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="feature input spline sketch" Type="Sketch" id="21"/><Sketch Name="feature input rational spline sketch" Type="Sketch" id="22"/><Sweep Name="Pipe" Type="Sweep" id="23" Profile="21" Path="22" Operation="NewBody"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let bound = decoded
        .ir
        .model
        .features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } => Some(sketch.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bound.len(), 2);
    let sweep = decoded
        .ir
        .model
        .features
        .iter()
        .find_map(|feature| match &feature.definition {
            FeatureDefinition::Sweep {
                profile: Some(ProfileRef::Sketch(profile)),
                path: Some(PathRef::Sketch(path)),
                ..
            } => Some((profile, path)),
            _ => None,
        })
        .expect("bound sweep");
    assert_ne!(sweep.0, sweep.1);
    assert!(bound.contains(sweep.0) && bound.contains(sweep.1));
    let validation = cadmpeg_ir::validate(&decoded.ir, Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
}

#[test]
fn decode_does_not_bind_duplicate_sketch_names_by_order() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    let mut payload = resolved_features_payload(&[1, 1]);
    for _ in 0..2 {
        payload.extend(parasolid_with_body(
            "Duplicate",
            "SCH_SW_33103_11000",
            &nurbs_sketch_body(false),
        ));
    }
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Duplicate" Type="Sketch" id="21"/><Sketch Name="Duplicate" Type="Sketch" id="22"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir.model.sketches.len(), 2);
    assert!(decoded.ir.model.features.iter().all(|feature| matches!(
        feature.definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    )));
}

#[test]
fn semantic_writer_round_trips_planar_and_spatial_sketch_space() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="Spatial path" Type="3DSketch" id="40"/>
            <Sketch Name="Profile" Type="Sketch" id="41"/>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir.model.features[0].definition,
        FeatureDefinition::SpatialSketch { sketch: None }
    ));
    assert!(matches!(
        decoded.ir.model.features[1].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));

    decoded.ir.model.features[0].name = Some("Renamed spatial path".into());
    decoded.ir.model.features[1].definition = FeatureDefinition::SpatialSketch { sketch: None };

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(&regenerated.ir).feature_histories[0].features;
    assert_eq!(native[0].kind, "3DSketch");
    assert_eq!(native[0].name, "Renamed spatial path");
    assert_eq!(native[1].kind, "3DSketch");
    assert!(regenerated.ir.model.features.iter().all(|feature| matches!(
        feature.definition,
        FeatureDefinition::SpatialSketch { sketch: None }
    )));
}

#[test]
fn decode_distinguishes_full_circle_sketch_geometry() {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_circular_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir.model.sketches[0].profiles[0].len(), 1);
    assert!(matches!(
        decoded.ir.model.sketch_entities[0].geometry,
        SketchGeometry::Circle {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            radius: Length(1000.0),
        }
    ));
}

#[test]
fn decode_projects_full_ellipse_sketch_geometry() {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_elliptical_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        decoded.ir.model.sketch_entities[0].geometry,
        SketchGeometry::Ellipse {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            major_angle: Angle(value),
            major_radius: Length(2000.0),
            minor_radius: Length(1000.0),
            start_angle: None,
            end_angle: None,
        } if (value - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
}

#[test]
fn decode_projects_non_rational_and_rational_nurbs_sketch_geometry() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_nurbs_sketches(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let splines = decoded
        .ir
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| match &entity.geometry {
            SketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            } => Some((degree, knots, control_points, weights, periodic)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(splines.len(), 2);
    assert!(splines.iter().all(|(degree, knots, points, _, periodic)| {
        **degree == 2
            && knots.as_slice() == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
            && points.len() == 3
            && !**periodic
    }));
    assert!(splines
        .iter()
        .any(|(_, _, _, weights, _)| weights.is_none()));
    assert!(splines
        .iter()
        .any(|(_, _, _, weights, _)| { weights.as_deref() == Some(&[1.0, 0.5, 1.0]) }));
}

#[test]
fn semantic_writer_applies_line_sketch_edits() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_sketch_profile(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let point_ref = decoded.ir.model.sketch_entities[0].endpoint_refs[0].clone();
    for entity in &mut decoded.ir.model.sketch_entities {
        let SketchGeometry::Line { start, end } = &mut entity.geometry else {
            panic!("line sketch entity");
        };
        if entity.endpoint_refs[0] == point_ref {
            start.u += 1.0;
        }
        if entity.endpoint_refs[1] == point_ref {
            end.u += 1.0;
        }
    }

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let edited = regenerated
        .ir
        .model
        .sketch_entities
        .iter()
        .flat_map(|entity| match &entity.geometry {
            SketchGeometry::Line { start, end } => [start.u, end.u],
            _ => panic!("line sketch entity"),
        })
        .filter(|value| (*value - 1.0).abs() < 1.0e-12)
        .count();
    assert_eq!(edited, 2);
}

#[test]
fn semantic_writer_applies_compressed_line_sketch_edits() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_compressed_nested_sketch_profile(
                &triangle_body(),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let point_ref = decoded.ir.model.sketch_entities[0].endpoint_refs[0].clone();
    for entity in &mut decoded.ir.model.sketch_entities {
        let SketchGeometry::Line { start, end } = &mut entity.geometry else {
            panic!("line sketch entity");
        };
        if entity.endpoint_refs[0] == point_ref {
            start.v += 2.0;
        }
        if entity.endpoint_refs[1] == point_ref {
            end.v += 2.0;
        }
    }

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let scan = container::scan_bytes(&written);
    let lane = scan
        .blocks
        .iter()
        .find(|block| {
            block
                .section
                .as_deref()
                .is_some_and(|section| section.contains("ResolvedFeatures"))
        })
        .unwrap();
    assert!(lane
        .payload
        .windows(2)
        .any(|bytes| { bytes[0] == 0x78 && matches!(bytes[1], 0x01 | 0x9c | 0xda) }));
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let edited = regenerated
        .ir
        .model
        .sketch_entities
        .iter()
        .flat_map(|entity| match &entity.geometry {
            SketchGeometry::Line { start, end } => [start.v, end.v],
            _ => panic!("line sketch entity"),
        })
        .filter(|value| (*value - 2.0).abs() < 1.0e-12)
        .count();
    assert_eq!(edited, 2);
}

#[test]
fn semantic_writer_rejects_conflicting_shared_sketch_point_edits() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_sketch_profile(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let SketchGeometry::Line { start, .. } = &mut decoded.ir.model.sketch_entities[0].geometry
    else {
        panic!("line sketch entity");
    };
    start.u += 1.0;

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::Malformed(message)
            if message.contains("conflicting positions")
    ));
}

#[test]
fn semantic_writer_applies_circle_sketch_edits() {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_circular_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let SketchGeometry::Circle { center, radius } =
        &mut decoded.ir.model.sketch_entities[0].geometry
    else {
        panic!("circle sketch entity");
    };
    center.u = 250.0;
    *radius = Length(750.0);

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir.model.sketch_entities[0].geometry,
        SketchGeometry::Circle {
            center: cadmpeg_ir::math::Point2 { u: 250.0, v: 0.0 },
            radius: Length(750.0),
        }
    ));
}

#[test]
fn semantic_writer_applies_ellipse_sketch_edits() {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_elliptical_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let SketchGeometry::Ellipse {
        center,
        major_angle,
        major_radius,
        minor_radius,
        ..
    } = &mut decoded.ir.model.sketch_entities[0].geometry
    else {
        panic!("ellipse sketch entity");
    };
    center.v = 125.0;
    *major_angle = Angle(0.25);
    *major_radius = Length(1500.0);
    *minor_radius = Length(500.0);

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir.model.sketch_entities[0].geometry,
        SketchGeometry::Ellipse {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 125.0 },
            major_angle: Angle(angle),
            major_radius: Length(1500.0),
            minor_radius: Length(500.0),
            start_angle: None,
            end_angle: None,
        } if (angle - 0.25).abs() < 1.0e-12
    ));
}

#[test]
fn semantic_writer_applies_bounded_arc_sketch_edits() {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_arc_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let arc = decoded
        .ir
        .model
        .sketch_entities
        .iter_mut()
        .find(|entity| matches!(entity.geometry, SketchGeometry::Arc { .. }))
        .expect("arc sketch entity");
    let SketchGeometry::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = &mut arc.geometry
    else {
        unreachable!();
    };
    center.u = 100.0;
    *radius = Length(800.0);
    *start_angle = Angle(0.25);
    *end_angle = Angle(1.25);
    let endpoint_refs = arc.endpoint_refs.clone();
    let endpoints = [
        cadmpeg_ir::math::Point2::new(100.0 + 800.0 * 0.25f64.cos(), 800.0 * 0.25f64.sin()),
        cadmpeg_ir::math::Point2::new(100.0 + 800.0 * 1.25f64.cos(), 800.0 * 1.25f64.sin()),
    ];
    for entity in &mut decoded.ir.model.sketch_entities {
        let SketchGeometry::Line { start, end } = &mut entity.geometry else {
            continue;
        };
        for (reference, target) in endpoint_refs.iter().zip(endpoints) {
            if entity.endpoint_refs[0] == *reference {
                *start = target;
            }
            if entity.endpoint_refs[1] == *reference {
                *end = target;
            }
        }
    }

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(regenerated
        .ir
        .model
        .sketch_entities
        .iter()
        .any(|entity| matches!(
            entity.geometry,
            SketchGeometry::Arc {
                center: cadmpeg_ir::math::Point2 { u: 100.0, v: 0.0 },
                radius: Length(800.0),
                start_angle: Angle(start),
                end_angle: Angle(end),
            } if (start - 0.25).abs() < 1.0e-12 && (end - 1.25).abs() < 1.0e-12
        )));
}

#[test]
fn semantic_writer_applies_rational_and_non_rational_sketch_nurbs_edits() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_nurbs_sketches(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    for entity in &mut decoded.ir.model.sketch_entities {
        let SketchGeometry::Nurbs {
            control_points,
            weights,
            ..
        } = &mut entity.geometry
        else {
            continue;
        };
        control_points[1].v += 250.0;
        if let Some(weights) = weights {
            weights[1] = 0.75;
        }
    }

    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut written)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let splines = regenerated
        .ir
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| match &entity.geometry {
            SketchGeometry::Nurbs {
                control_points,
                weights,
                ..
            } => Some((control_points, weights)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(splines.len(), 2);
    assert!(splines
        .iter()
        .all(|(points, _)| (points[1].v - 1250.0).abs() < 1.0e-12));
    assert!(splines
        .iter()
        .any(|(_, weights)| weights.as_deref() == Some(&[1.0, 0.75, 1.0])));
}

#[test]
fn decode_extracts_document_envelope() {
    use cadmpeg_ir::attributes::AttributeValue;
    let mut cur = Cursor::new(sldprt_with_body_and_envelope(&triangle_body()));
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let envelope = result
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "bounding_envelope")
        .expect("envelope");
    let AttributeValue::Vector(values) = &envelope.values[0] else {
        panic!("vector")
    };
    assert_eq!(values, &[10.0, 20.0, -30.0, 40.0]);
    let plane = result
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "default_reference_plane")
        .expect("reference plane");
    let AttributeValue::Vector(origin) = &plane.values[0] else {
        panic!("origin")
    };
    let AttributeValue::Vector(frame) = &plane.values[1] else {
        panic!("frame")
    };
    assert_eq!(origin, &[1.0, 2.0, 3.0]);
    assert_eq!(frame[2], 1.0);
    let transformed = result
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "transformed_reference_plane")
        .expect("transformed reference plane");
    assert_eq!(
        transformed.values,
        vec![
            AttributeValue::Vector(vec![10.0, 20.0, 30.0]),
            AttributeValue::Vector(vec![100.0, 200.0]),
            AttributeValue::Vector(vec![1.0, 0.0, -1.0]),
            AttributeValue::Float(500.0),
        ]
    );
    let part = result
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "part_record")
        .unwrap();
    assert_eq!(
        part.values,
        vec![AttributeValue::Integer(42), AttributeValue::Integer(2026)]
    );
    let configuration = result
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "configuration_manager")
        .unwrap();
    assert_eq!(configuration.values[1], AttributeValue::Integer(3));
    let units = result
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "source_linear_unit_code")
        .unwrap();
    assert_eq!(units.values, vec![AttributeValue::Integer(0)]);
    let unit_name = result
        .ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "source_linear_unit_name")
        .unwrap();
    assert_eq!(unit_name.values, vec![AttributeValue::String("IN".into())]);
}

#[test]
fn semantic_writer_preserves_document_metadata() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_envelope(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;

    let expected = decoded
        .ir
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let actual = regenerated
        .ir
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(actual, expected);
}

#[test]
fn semantic_writer_preserves_opaque_auxiliary_blocks() {
    let payload = b"vendor-private\x00\x01\x02";
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x77, "Contents/CustomData", payload));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated
        .source_fidelity
        .retained_records
        .iter()
        .any(|record| {
            regenerated
                .source_fidelity
                .annotations
                .provenance
                .get(&record.id)
                .and_then(|note| {
                    regenerated
                        .source_fidelity
                        .annotations
                        .streams
                        .get(note.stream as usize)
                })
                .is_some_and(|stream| stream == "Contents/CustomData")
                && record.data.as_deref() == Some(payload.as_slice())
        }));
}

#[test]
fn semantic_writer_round_trips_all_supported_lanes_together() {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut body = Vec::new();
    body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 700, 0x0015, &[0, 0, 0, 0, 0, 900]));
    body.extend(entity53_color(900, [0.25, 0.5, 0.75]));
    body.extend(owned_triangle(0, 700, 0.0));
    let mut source = sldprt_with_body_and_material(&body, "Steel", [32, 64, 128]);
    source.extend(make_block(
        0x41,
        "Contents/DisplayLists",
        &display_list_payload(),
    ));
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords Name="Bracket"><Configuration Name="Default" Material="Steel"/><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12.5mm</Dimension></Extrusion></Keywords>"#));
    source.extend(make_block(0x77, "Contents/CustomData", b"opaque-state"));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded.ir.model.points[0].position.z += 2.0;
    decoded.ir.model.tessellations[0].vertices[0].z = 125.0;
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert("Depth".into(), "20mm".into());
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        regenerated.ir.model.bodies[0].name.as_deref(),
        Some("Steel")
    );
    assert!(regenerated
        .ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(binding.target, AppearanceTarget::Face(_))));
    assert_eq!(regenerated.ir.model.tessellations[0].vertices[0].z, 125.0);
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].parameters["Depth"],
        "20mm"
    );
    assert!(regenerated
        .source_fidelity
        .retained_records
        .iter()
        .any(|record| {
            regenerated
                .source_fidelity
                .annotations
                .provenance
                .get(&record.id)
                .and_then(|note| {
                    regenerated
                        .source_fidelity
                        .annotations
                        .streams
                        .get(note.stream as usize)
                })
                .is_some_and(|stream| stream == "Contents/CustomData")
                && record.data.as_deref() == Some(b"opaque-state".as_slice())
        }));

    let written = regenerated
        .source_fidelity
        .retained_record("sldprt:file:source-image#0")
        .and_then(|record| record.data.as_ref())
        .unwrap();
    let scan = container::scan_bytes(written);
    assert_eq!(scan.directory.len(), scan.blocks.len());
    for block in &scan.blocks {
        let section = block.section.as_deref().unwrap();
        if section == "Contents/CustomData" {
            assert_eq!(block.type_id, 0x77);
        }
        assert!(scan.directory.iter().any(|entry| {
            entry.name == section && entry.size == block.uncomp_sz && entry.type_id == block.type_id
        }));
    }
}

#[test]
fn face_on_untyped_surface_keeps_topology() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    let SurfaceGeometry::Unknown {
        record: Some(record),
    } = &result.ir.model.surfaces[0].geometry
    else {
        panic!("opaque surface has no replay record");
    };
    let unknowns = result.ir.native_unknowns("sldprt").unwrap();
    let retained = unknowns
        .iter()
        .find(|unknown| unknown.id == *record)
        .expect("opaque surface record");
    assert!(retained.links.contains(&result.ir.model.surfaces[0].id.0));
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.code == LossCode::GeometryNotTransferred));
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn strict_rejects_topology_decode_resting_on_untyped_surface() {
    use cadmpeg_ir::report::{LossCode, StrictConsequence};

    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));
    let fixture = sldprt_with_body(&body);

    let salvaged = SldprtCodec
        .decode(&mut Cursor::new(fixture.clone()), &DecodeOptions::default())
        .expect("salvage keeps the topology decode");
    assert_eq!(salvaged.ir.model.faces.len(), 1);
    let census = salvaged
        .report
        .losses
        .iter()
        .find(|l| l.code == LossCode::GeometryNotTransferred)
        .expect("untyped support surface raises a census note");
    assert_eq!(census.code.strict_consequence(), StrictConsequence::Reject);

    let error = SldprtCodec
        .decode(&mut Cursor::new(fixture), &strict_options())
        .expect_err("strict refuses the untyped-surface census");
    assert!(error
        .to_string()
        .contains("strict mode rejects geometry_not_transferred"));
}

#[test]
fn opaque_curve_is_retained_and_does_not_block_point_edits() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let mut body = triangle_body();
    body.extend(edge_use(40, 999));
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let curve_id = decoded.ir.model.edges[0]
        .curve
        .as_ref()
        .expect("opaque edge curve");
    let curve = decoded
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .expect("opaque curve carrier");
    let CurveGeometry::Unknown {
        record: Some(record),
    } = &curve.geometry
    else {
        panic!("opaque curve has no replay record");
    };
    let unknowns = decoded.ir.native_unknowns("sldprt").unwrap();
    let retained = unknowns
        .iter()
        .find(|unknown| unknown.id == *record)
        .expect("opaque curve record");
    assert!(retained.links.contains(&curve.id.0));

    decoded.ir.model.points[1].position.x = 1_500.0;
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir.model.points[1].position.x, 1_500.0);
    assert!(regenerated
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Unknown { .. })));
}

#[test]
fn native_patch_edits_points_without_dropping_untyped_surfaces() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    let deltas = parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &line_carrier(800, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    );
    let mut source = sldprt_with_body(&body);
    source.extend(make_block(0x21, "Contents/Config-0-Deltas", &deltas));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    decoded.ir.model.points[1].position.x = 1_250.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir.model.points[1].position.x, 1_250.0);
    assert!(matches!(
        regenerated.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    assert_eq!(regenerated.ir.model.faces.len(), 1);
    let written = regenerated
        .source_fidelity
        .retained_record("sldprt:file:source-image#0")
        .and_then(|record| record.data.as_deref())
        .unwrap();
    let scan = container::scan_bytes(written);
    assert!(scan.blocks.iter().any(|block| {
        block.section.as_deref() == Some("Contents/Config-0-Deltas") && block.payload == deltas
    }));
}

#[test]
fn native_patch_requires_point_provenance_annotation() {
    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let point_id = decoded.ir.model.points[1].id.0.clone();
    assert!(decoded
        .source_fidelity
        .annotations
        .provenance
        .contains_key(&point_id));
    decoded.ir.model.points[1].position.x = 1_250.0;
    decoded
        .source_fidelity
        .annotations
        .provenance
        .remove(&point_id);

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::codec::CodecError::Malformed(message)
            if message.contains("requires provenance annotation") && message.contains(&point_id)
    ));
}

#[test]
fn native_patch_edits_analytic_carriers_beside_untyped_surfaces() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let mut body = triangle_body();
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(edge_use(40, 70));
    body.extend(bridge(210, 220, 999));
    body.extend(loop_head(220, 230, 210));
    body.extend(coedge(230, 220, 231, 250, 0, 240, false));
    body.extend(coedge(231, 220, 232, 251, 0, 241, false));
    body.extend(coedge(232, 220, 230, 252, 0, 242, false));
    body.extend(edge_use(240, 0));
    body.extend(edge_use(241, 0));
    body.extend(edge_use(242, 0));
    body.extend(vertex_use(250, 260));
    body.extend(vertex_use(251, 261));
    body.extend(vertex_use(252, 262));
    body.extend(world_point(260, [10.0, 0.0, 0.0]));
    body.extend(world_point(261, [11.0, 0.0, 0.0]));
    body.extend(world_point(262, [10.0, 1.0, 0.0]));

    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plane = decoded
        .ir
        .model
        .surfaces
        .iter_mut()
        .find(|surface| matches!(surface.geometry, SurfaceGeometry::Plane { .. }))
        .unwrap();
    let SurfaceGeometry::Plane { origin, .. } = &mut plane.geometry else {
        unreachable!()
    };
    origin.x = 25.0;
    let line = decoded
        .ir
        .model
        .curves
        .iter_mut()
        .find(|curve| matches!(curve.geometry, CurveGeometry::Line { .. }))
        .unwrap();
    let CurveGeometry::Line { origin, .. } = &mut line.geometry else {
        unreachable!()
    };
    origin.y = 12.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert!(regenerated.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane { origin, .. } if origin.x == 25.0
    )));
    assert!(regenerated
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. })));
    assert!(regenerated.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Line { origin, .. } if origin.y == 12.0
    )));
}

#[test]
fn auxiliary_edit_retains_opaque_partition_payload() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let mut body = Vec::new();
    body.extend(bridge(10, 20, 999));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 0));
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));
    let mut source = sldprt_with_body_and_history(&body);
    source.extend(make_block(
        0x66,
        "Contents/Config-0-Deltas",
        b"opaque-deltas",
    ));
    source.extend(make_block(
        0x67,
        "Contents/Config-0-GhostPartition",
        b"opaque-ghost",
    ));
    source.extend(make_cache_cell(90, "Contents/Config-0-Partition"));
    source.extend(make_cache_cell(100, "Contents/Keywords"));
    let indexed = container::scan_bytes(&source);
    let partition = indexed
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap();
    let mut directory = make_directory_entry(
        partition.type_id,
        partition.uncomp_sz,
        "Contents/Config-0-Partition",
    );
    directory[26] = 0xab;
    let trailer = directory.len() - 6;
    directory[trailer..trailer + 4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    source.extend(directory);
    let source_scan = container::scan_bytes(&source);
    let source_partition = source_scan
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap()
        .payload
        .clone();
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let brep_hash = crate::decode::brep_semantic_hash(&decoded.ir);
    let semantic_hash = crate::decode::semantic_hash(&decoded.ir);
    update_sldprt_native(&mut decoded.ir, |native| {
        native.feature_histories[0].features[0]
            .parameters
            .insert("Depth".into(), "30mm".into());
    });
    decoded.source_fidelity.annotations.exactness.clear();
    assert_eq!(crate::decode::brep_semantic_hash(&decoded.ir), brep_hash);
    assert_ne!(crate::decode::semantic_hash(&decoded.ir), semantic_hash);

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let written_scan = container::scan_bytes(&encoded);
    let written_partition = written_scan
        .blocks
        .iter()
        .find(|block| block.section.as_deref() == Some("Contents/Config-0-Partition"))
        .unwrap();
    assert_eq!(written_partition.payload, source_partition);
    assert!(written_scan.blocks.iter().any(|block| {
        block.section.as_deref() == Some("Contents/Config-0-Deltas")
            && block.payload == b"opaque-deltas"
    }));
    assert_eq!(written_scan.cache_cells.len(), 1);
    assert_eq!(
        written_scan.cache_cells[0].name,
        "Contents/Config-0-Partition"
    );
    assert_eq!(written_scan.cache_cells[0].logical_len, 90);
    let partition_directory = written_scan
        .directory
        .iter()
        .find(|entry| entry.name == "Contents/Config-0-Partition")
        .unwrap();
    assert_eq!(encoded[partition_directory.offset + 26], 0xab);
    let trailer = partition_directory.offset + 40 + partition_directory.name.len();
    assert_eq!(&encoded[trailer..trailer + 4], &[0x11, 0x22, 0x33, 0x44]);
    assert!(written_scan.blocks.iter().any(|block| {
        block.section.as_deref() == Some("Contents/Config-0-GhostPartition")
            && block.payload == b"opaque-ghost"
    }));
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Unknown { .. }
    ));
    assert_eq!(
        sldprt_native(&regenerated.ir).feature_histories[0].features[0].parameters["Depth"],
        "30mm"
    );
}

#[test]
fn semantic_writer_preserves_display_list_geometry() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_display_list(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.points[0].position.z += 1.0;
    decoded.ir.model.tessellations[0].vertices[0].z = 250.0;

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&decoded.ir, &decoded.source_fidelity, &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();

    assert_eq!(regenerated.ir.model.tessellations.len(), 1);
    let mesh = &regenerated.ir.model.tessellations[0];
    assert_eq!(mesh.vertices[0].z, 250.0);
    assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
    assert_eq!(mesh.strip_lengths, vec![3]);
    assert_eq!(mesh.channels.len(), 6);
}

#[test]
fn semantic_writer_rejects_tessellation_f32_overflow() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_display_list(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir.model.tessellations[0].vertices[0].x = f64::MAX;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            &decoded.ir,
            &decoded.source_fidelity,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("tessellation position exceeds f32 range"));
}

#[test]
fn semantic_writer_expands_indexed_tessellation() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::tessellation::{Tessellation, TessellationChannel};

    let mesh = Tessellation {
        id: "synthetic:test:indexed-tessellation".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        strip_lengths: Vec::new(),
        normals: vec![Vector3::new(0.0, 0.0, 1.0); 4],
        channels: vec![TessellationChannel {
            item_size: 1,
            kind: 7,
            flags: 2,
            count: 4,
            data: vec![10, 11, 12, 13],
        }],
    };
    let expanded = crate::writer::sequential_tessellation(&mesh).unwrap();
    assert_eq!(expanded.strip_lengths, vec![3, 3]);
    assert_eq!(expanded.triangles, vec![[0, 1, 2], [3, 4, 5]]);
    assert_eq!(expanded.vertices.len(), 6);
    assert_eq!(expanded.normals.len(), 6);
    assert_eq!(expanded.channels[0].count, 6);
    assert_eq!(expanded.channels[0].data, vec![10, 11, 12, 10, 12, 13]);
}

#[test]
fn semantic_writer_rejects_out_of_range_tessellation_indices() {
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::tessellation::Tessellation;

    let mesh = Tessellation {
        id: "synthetic:test:invalid-tessellation".into(),
        body: None,
        faces: Vec::new(),
        chordal_deflection: None,
        source_object: None,
        vertices: vec![Point3::new(0.0, 0.0, 0.0); 3],
        triangles: vec![[0, 1, 3]],
        strip_lengths: Vec::new(),
        normals: Vec::new(),
        channels: Vec::new(),
    };
    let error = crate::writer::sequential_tessellation(&mesh).unwrap_err();
    assert!(error.to_string().contains("index is out of bounds"));
}

#[test]
fn compact_carrier_shapes_decode() {
    use crate::brep::{parse_carrier, CarrierGeometry};
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    // Cylinder (tag 00 33, 10 f64): origin, axis, radius, refdir.
    let mut cyl = vec![0x00, 0x33];
    be16(&mut cyl, 5);
    be32(&mut cyl, 0);
    for _ in 0..5 {
        be16(&mut cyl, 0);
    }
    cyl.push(0x2b);
    for v in [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.05, 1.0, 0.0, 0.0] {
        bef64(&mut cyl, v);
    }
    match parse_carrier(&cyl, 0).unwrap().geometry {
        CarrierGeometry::Surface(SurfaceGeometry::Cylinder { radius, axis, .. }) => {
            assert_eq!(radius, 50.0); // 0.05 m ×1000
            assert_eq!(axis.z, 1.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }

    // Circle (tag 00 1f, 10 f64): radius is the tenth value.
    let mut circ = vec![0x00, 0x1f];
    be16(&mut circ, 6);
    be32(&mut circ, 0);
    for _ in 0..5 {
        be16(&mut circ, 0);
    }
    circ.push(0x2d);
    for v in [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.003] {
        bef64(&mut circ, v);
    }
    match parse_carrier(&circ, 0).unwrap().geometry {
        CarrierGeometry::Curve(CurveGeometry::Circle { radius, .. }) => assert_eq!(radius, 3.0),
        other => panic!("expected circle, got {other:?}"),
    }

    // A bad marker (not 2b/2d) rejects the candidate.
    let mut bad = cyl.clone();
    bad[2 + 2 + 4 + 10] = 0x00;
    assert!(parse_carrier(&bad, 0).is_none());
}

#[test]
fn compact_carriers_reject_zero_direction_frames() {
    use crate::brep::parse_carrier;

    let line = line_carrier(5, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    assert!(parse_carrier(&line, 0).is_none());

    let cylinder = cylinder_carrier(6, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 1.0);
    assert!(parse_carrier(&cylinder, 0).is_none());
}

/// Translate every positional carrier in the model by `t`. Directions and
/// normals are invariant under translation, so a pure translation is a rigid
/// motion of the whole body.
fn translate_model(ir: &mut cadmpeg_ir::CadIr, t: [f64; 3]) {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;
    let shift = |p: &Point3| Point3::new(p.x + t[0], p.y + t[1], p.z + t[2]);
    for point in &mut ir.model.points {
        point.position = shift(&point.position);
    }
    for curve in &mut ir.model.curves {
        if let CurveGeometry::Line { origin, .. } = &mut curve.geometry {
            *origin = shift(origin);
        }
    }
    for surface in &mut ir.model.surfaces {
        if let SurfaceGeometry::Plane { origin, .. } = &mut surface.geometry {
            *origin = shift(origin);
        }
    }
}

fn source_less_cube() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    ir
}

fn encode_decode_result(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::codec::DecodeResult {
    let mut encoded = Vec::new();
    SldprtCodec.encode(ir, &mut encoded).unwrap();
    SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap()
}

fn encode_decode(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::CadIr {
    encode_decode_result(ir).ir
}

fn sorted_point_positions(ir: &cadmpeg_ir::CadIr) -> Vec<[f64; 3]> {
    let mut positions: Vec<[f64; 3]> = ir
        .model
        .points
        .iter()
        .map(|point| [point.position.x, point.position.y, point.position.z])
        .collect();
    positions.sort_by(|a, b| a.partial_cmp(b).unwrap());
    positions
}

/// Metamorphic property: a rigid translation of the input produces the same
/// rigid translation of the decoded output (equivariance), and topology is
/// invariant. Reader and writer cannot silently drop or reorient geometry
/// without breaking one of these relations.
#[test]
fn decode_is_equivariant_under_rigid_translation() {
    let base = source_less_cube();
    let t = [3.5, -7.25, 11.0];

    let mut moved = base.clone();
    translate_model(&mut moved, t);

    let base_out = encode_decode(&base);
    let moved_out = encode_decode(&moved);

    // Topology is invariant under a rigid motion.
    assert_eq!(base_out.model.faces.len(), moved_out.model.faces.len());
    assert_eq!(base_out.model.edges.len(), moved_out.model.edges.len());
    assert_eq!(
        base_out.model.vertices.len(),
        moved_out.model.vertices.len()
    );

    // Point positions of the moved decode equal the base decode shifted by t.
    let base_positions = sorted_point_positions(&base_out);
    let moved_positions = sorted_point_positions(&moved_out);
    assert_eq!(base_positions.len(), moved_positions.len());
    for (b, m) in base_positions.iter().zip(&moved_positions) {
        for axis in 0..3 {
            assert!(
                (b[axis] + t[axis] - m[axis]).abs() < 1e-6,
                "axis {axis}: {b:?} + {t:?} != {m:?}"
            );
        }
    }
}

/// Decode → encode → decode fixpoint: once through the writer, a source-less
/// model reaches a fixed point whose semantic hash and topology no longer
/// change. Paired with the value golden below so a shared reader/writer
/// misconception cannot hide behind a self-consistent round trip.
#[test]
fn source_less_cube_reaches_encode_decode_fixpoint() {
    let first = encode_decode_result(&source_less_cube());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(&first.ir, &first.source_fidelity, &mut encoded)
        .unwrap();
    let second = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap()
        .ir;

    let first_hash = crate::decode::semantic_hash(&first.ir);
    let second_hash = crate::decode::semantic_hash(&second);
    assert_eq!(first_hash, second_hash, "round trip is not a fixed point");

    // Value golden: the cube's record families and counts, asserted directly.
    assert_eq!(first.ir.model.bodies.len(), 1);
    assert_eq!(first.ir.model.faces.len(), 6);
    assert_eq!(first.ir.model.edges.len(), 12);
    assert_eq!(first.ir.model.vertices.len(), 8);
    assert_eq!(first.ir.model.coedges.len(), 24);
    assert_eq!(first.ir.model.loops.len(), 6);
    assert_eq!(
        sorted_point_positions(&first.ir),
        sorted_point_positions(&second)
    );
}
