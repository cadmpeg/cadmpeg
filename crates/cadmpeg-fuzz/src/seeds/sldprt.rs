// SPDX-License-Identifier: Apache-2.0
//! SolidWorks SLDPRT container and Parasolid body seed builders.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::Compression;

pub const MARKER: [u8; 4] = [0x9e, 0x14, 0x01, 0x00];
const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

pub fn swap_name(name: &str) -> Vec<u8> {
    name.bytes().map(|b| b.rotate_left(4)).collect()
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(data);
    h.finalize()
}

pub fn make_cache_cell(logical_len: u32, name: &str) -> Vec<u8> {
    let swapped = swap_name(name);
    let mut b = Vec::new();
    b.extend_from_slice(&MARKER);
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&(logical_len * 2).to_le_bytes());
    b.extend_from_slice(&(logical_len / 2).to_le_bytes());
    b.extend_from_slice(&logical_len.to_le_bytes());
    b.extend_from_slice(&(swapped.len() as u32).to_le_bytes());
    b.extend_from_slice(&swapped);
    b
}

pub fn make_directory_entry(type_id: u32, size: u32, name: &str) -> Vec<u8> {
    let swapped = swap_name(name);
    let mut b = Vec::new();
    b.extend_from_slice(&MARKER);
    b.extend_from_slice(&type_id.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&size.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&(swapped.len() as u32).to_le_bytes());
    b.extend_from_slice(&[0u8; 14]);
    b.extend_from_slice(&swapped);
    b.extend_from_slice(&[0xe5, 0x4b, 0x57, 0x5b, 0x00, 0x00]);
    b
}

pub fn parasolid_payload(description: &str, schema: &str) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&[b'P', b'S', 0x00, 0x00]);
    b.extend_from_slice(&(description.len() as u16).to_be_bytes());
    b.extend_from_slice(description.as_bytes());
    b.extend_from_slice(&[0x00, 0x00]);
    b.push(schema.len() as u8);
    b.extend_from_slice(schema.as_bytes());
    b
}

pub fn outer_header() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&0x0000_0001u32.to_le_bytes());
    b.extend_from_slice(&0x0000_0004u32.to_be_bytes());
    b
}

pub fn be16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_be_bytes());
}

pub fn be32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_be_bytes());
}

pub fn bef64(b: &mut Vec<u8>, v: f64) {
    b.extend_from_slice(&v.to_be_bytes());
}

pub fn plane_carrier(attr: u16, origin: [f64; 3], normal: [f64; 3], refdir: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x32];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for v in origin.into_iter().chain(normal).chain(refdir) {
        bef64(&mut b, v);
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

fn circle_carrier(attr: u16, center: [f64; 3], axis: [f64; 3], radius: f64) -> Vec<u8> {
    let mut b = vec![0x00, 0x1f];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..5 {
        be16(&mut b, 0);
    }
    b.push(0x2b);
    for value in center
        .into_iter()
        .chain(axis)
        .chain([1.0, 0.0, 0.0, radius])
    {
        bef64(&mut b, value);
    }
    b
}

pub fn bridge(attr: u16, loop_attr: u16, surface_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x0e];
    be16(&mut b, attr);
    be32(&mut b, 0);
    be16(&mut b, 0);
    b.extend_from_slice(&MAGIC);
    for r in [0u16, 0, loop_attr, 0, surface_attr] {
        be16(&mut b, r);
    }
    b.push(0x2b);
    b.extend_from_slice(&[0u8; 10]);
    b
}

pub fn loop_head(attr: u16, first_coedge: u16, bridge_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x0f];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for r in [0u16, first_coedge, bridge_attr, 0] {
        be16(&mut b, r);
    }
    b
}

pub fn coedge(
    attr: u16,
    owner_loop: u16,
    next: u16,
    start_vuse: u16,
    twin: u16,
    edge_use: u16,
    reversed: bool,
) -> Vec<u8> {
    let mut b = vec![0x00, 0x11];
    be16(&mut b, attr);
    for r in [0u16, owner_loop, 0, next, start_vuse, twin, edge_use, 0, 0] {
        be16(&mut b, r);
    }
    b.push(if reversed { 0x2d } else { 0x2b });
    b
}

pub fn edge_use(attr: u16, curve_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x10];
    be16(&mut b, attr);
    be32(&mut b, 0);
    be16(&mut b, 0);
    b.extend_from_slice(&MAGIC);
    for r in [0u16, 0, 0, curve_attr, 0, 0] {
        be16(&mut b, r);
    }
    b
}

pub fn vertex_use(attr: u16, point_attr: u16) -> Vec<u8> {
    let mut b = vec![0x00, 0x12];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for r in [0u16, 0, 0, 0, point_attr] {
        be16(&mut b, r);
    }
    b.extend_from_slice(&MAGIC);
    b
}

pub fn world_point(attr: u16, xyz: [f64; 3]) -> Vec<u8> {
    let mut b = vec![0x00, 0x1d];
    be16(&mut b, attr);
    be32(&mut b, 0);
    for _ in 0..4 {
        be16(&mut b, 0);
    }
    for v in xyz {
        bef64(&mut b, v);
    }
    b
}

pub fn triangle_body() -> Vec<u8> {
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

pub fn closed_cylinder_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(cylinder_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(circle_carrier(70, [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0));
    b.extend(circle_carrier(71, [-1.0, 0.0, 1.0], [0.0, 0.0, 1.0], 1.0));
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

fn raw_deflate(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)?;
    enc.finish()
}

pub fn make_block(type_id: u32, section: &str, payload: &[u8]) -> std::io::Result<Vec<u8>> {
    let comp = raw_deflate(payload)?;
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
    Ok(b)
}

pub fn parasolid_with_body(description: &str, schema: &str, body: &[u8]) -> Vec<u8> {
    let mut b = parasolid_payload(description, schema);
    b.extend_from_slice(body);
    b
}

pub fn synthetic_sldprt() -> std::io::Result<Vec<u8>> {
    let mut f = outer_header();
    f.extend_from_slice(&make_block(
        0x10,
        "PreviewPNG",
        &[0x89, b'P', b'N', b'G', 1, 2, 3, 4],
    )?);
    f.extend_from_slice(&make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_payload("partition body", "SCH_SW_33103_11000"),
    )?);
    f.extend_from_slice(&make_cache_cell(90, "Contents/DisplayLists"));
    f.extend_from_slice(&make_directory_entry(0x30, 2, "[Content_Types].xml"));
    Ok(f)
}

pub fn sldprt_with_body(body: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut f = outer_header();
    f.extend_from_slice(&make_block(
        0x20,
        "Contents/Config-0-Partition",
        &parasolid_with_body("partition body", "SCH_SW_33103_11000", body),
    )?);
    Ok(f)
}
