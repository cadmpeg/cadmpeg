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
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Vector3;

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
    put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]); // 76.2 mm
    put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&pl);

    // CYLINDER (type 51): origin +19, axis +43, radius +67, x_axis +75.
    let mut cy = record(0x33, 99);
    put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, 0.004_05); // 4.05 mm
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&cy);

    // LINE (type 30): point +19, direction +43.
    let mut ln = record(0x1e, 67);
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
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&plane);

    let mut line = record(30, 67);
    put_ref(&mut line, 2, 9);
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

fn bspline_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00XX: TRANSMIT FILE (partition)\x00SCH_TEST_1_9999\x00");
    let mut surface = record(124, 23);
    put_ref(&mut surface, 2, 10);
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

fn trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 12);
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.25);
    put_f64(&mut trim, 77, 0.75);
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

fn topology_part_prt() -> Vec<u8> {
    prt_with_partition(&topology_partition_stream())
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
    assert_eq!(result.ir.points.len(), 1);
    assert_eq!(result.ir.vertices.len(), 1);
    // Point coordinate is scaled metres → millimetres, byte-exact.
    let p = &result.ir.points[0].position;
    assert!((p.x - 62.5).abs() < 1e-6 && (p.z - 12.7).abs() < 1e-6);

    // One plane, one cylinder decoded.
    let planes = result
        .ir
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let cyls: Vec<_> = result
        .ir
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
    assert!(result.ir.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane {
            u_axis: Some(axis),
            ..
        } if axis == Vector3::new(1.0, 0.0, 0.0)
    )));
    assert!(result.ir.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder {
            ref_direction: Some(direction),
            ..
        } if direction == Vector3::new(1.0, 0.0, 0.0)
    )));

    // One line decoded, with a unit direction.
    let lines: Vec<_> = result
        .ir
        .curves
        .iter()
        .filter(|c| matches!(c.geometry, CurveGeometry::Line { .. }))
        .collect();
    assert_eq!(lines.len(), 1);

    // No topology graph is fabricated; the loss is reported as blocking.
    assert!(result.ir.faces.is_empty() && result.ir.edges.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.category == cadmpeg_ir::report::LossCategory::Topology
            && l.severity == cadmpeg_ir::report::Severity::Blocking));

    // The Parasolid stream is preserved verbatim.
    assert_eq!(result.ir.unknowns.len(), 1);
    assert_eq!(result.ir.unknowns[0].sha256.len(), 64);

    // The produced IR validates (free carriers, no dangling references).
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_emits_connected_primitive_brep() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.bodies.len(), 1);
    assert_eq!(result.ir.lumps.len(), 1);
    assert_eq!(result.ir.shells.len(), 1);
    assert_eq!(result.ir.faces.len(), 1);
    assert_eq!(result.ir.loops.len(), 1);
    assert_eq!(result.ir.coedges.len(), 1);
    assert_eq!(result.ir.edges.len(), 1);
    assert_eq!(result.ir.vertices.len(), 1);
    assert_eq!(
        result.ir.faces[0].loops,
        vec![result.ir.loops[0].id.clone()]
    );
    assert_eq!(
        result.ir.edges[0].curve.as_ref(),
        Some(&result.ir.curves[0].id)
    );
    assert_eq!(result.ir.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.edges[0].tolerance, Some(0.3));
    assert_eq!(result.ir.faces[0].tolerance, Some(0.2));
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.category != cadmpeg_ir::report::LossCategory::Topology));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_topology_when_record_xmt_uses_extended_encoding() {
    let stream = large_xmt_headers(&topology_partition_stream());
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.faces.len(), 1);
    assert_eq!(result.ir.edges.len(), 1);
    assert_eq!(result.ir.vertices.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_transfers_bspline_surface_and_curve() {
    let stream = bspline_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir
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
fn decode_resolves_trimmed_edge_to_its_basis_curve_and_range() {
    let mut cur = Cursor::new(prt_with_partition(&trimmed_topology_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_extended_xmt_reference_inside_edge_record() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_curve_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.edges.len(), 1);
    assert_eq!(
        result.ir.edges[0].curve.as_ref(),
        Some(&result.ir.curves[0].id)
    );
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
fn container_only_preserves_streams_without_geometry() {
    let mut cur = Cursor::new(single_part_prt());
    let opts = DecodeOptions {
        container_only: true,
    };
    let result = NxCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result.report.container_only);
    assert_eq!(result.ir.unknowns.len(), 1);
    assert!(result.ir.points.is_empty());
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
