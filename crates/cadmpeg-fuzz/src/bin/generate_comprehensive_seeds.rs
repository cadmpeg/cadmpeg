// SPDX-License-Identifier: Apache-2.0
//! Writes deep topology, NURBS, and format-variant container seeds for
//! SolidWorks, CATIA, Creo, and NX. Existing F3D seeds remain unchanged.

use std::fs;
use std::io::Write;
use std::path::Path;

use flate2::write::DeflateEncoder;
use flate2::Compression;

fn main() {
    generate_f3d_seeds();
    generate_sldprt_seeds();
    generate_catia_seeds();
    generate_creo_seeds();
    generate_nx_seeds();
    println!("All comprehensive seeds generated.");
}

// ============================================================================
// F3D seeds
// ============================================================================

fn generate_f3d_seeds() {
    println!("f3d seeds already comprehensive, skipping regeneration");
}

// ============================================================================
// SLDPRT seeds
// ============================================================================

fn generate_sldprt_seeds() {
    let dir = Path::new("seeds/sldprt_container");
    fs::create_dir_all(dir).expect("required invariant");

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_header", sldprt::outer_header()),
        ("synthetic_sldprt", sldprt::synthetic_sldprt()),
        (
            "triangle_body",
            sldprt::sldprt_with_body(&sldprt::triangle_body()),
        ),
        (
            "triangle_overlapping_point",
            sldprt::sldprt_with_body(&sldprt::triangle_body_with_overlapping_point()),
        ),
        (
            "closed_cylinder",
            sldprt::sldprt_with_body(&sldprt::closed_cylinder_body()),
        ),
        (
            "with_material",
            sldprt::sldprt_with_body_and_material(&sldprt::triangle_body(), "Steel", [32, 64, 128]),
        ),
        (
            "with_display_list",
            sldprt::sldprt_with_body_and_display_list(&sldprt::triangle_body()),
        ),
        (
            "partition_and_deltas",
            sldprt::sldprt_with_partition_and_deltas(&sldprt::triangle_body()),
        ),
        (
            "sheet_body",
            sldprt::sldprt_with_body(&sldprt::sheet_body()),
        ),
        (
            "two_owned_triangles",
            sldprt::sldprt_with_body(&sldprt::two_owned_triangles()),
        ),
        (
            "with_nurbs_curve",
            sldprt::sldprt_with_body(&sldprt::triangle_with_nurbs_curve()),
        ),
        (
            "with_nurbs_surface",
            sldprt::sldprt_with_body(&sldprt::triangle_with_nurbs_surface()),
        ),
        (
            "face_on_untyped_surface",
            sldprt::sldprt_with_body(&sldprt::face_on_untyped_surface()),
        ),
        (
            "with_line_curve",
            sldprt::sldprt_with_body(&sldprt::triangle_with_line_curve()),
        ),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).expect("required invariant");
        println!("  sldprt/{} ({} bytes)", name, data.len());
    }
}

mod sldprt {
    use super::*;

    pub const MARKER: [u8; 4] = [0x9e, 0x14, 0x01, 0x00];
    const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

    fn swap_name(name: &str) -> Vec<u8> {
        name.bytes().map(|b| b.rotate_left(4)).collect()
    }
    fn raw_deflate(data: &[u8]) -> Vec<u8> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data).expect("required invariant");
        enc.finish().expect("required invariant")
    }
    fn crc32(data: &[u8]) -> u32 {
        let mut h = crc32fast::Hasher::new();
        h.update(data);
        h.finalize()
    }

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

    fn make_cache_cell(logical_len: u32, name: &str) -> Vec<u8> {
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

    fn make_directory_entry(type_id: u32, size: u32, name: &str) -> Vec<u8> {
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

    fn parasolid_payload(description: &str, schema: &str) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&[b'P', b'S', 0x00, 0x00]);
        b.extend_from_slice(&(description.len() as u16).to_be_bytes());
        b.extend_from_slice(description.as_bytes());
        b.extend_from_slice(&[0x00, 0x00]);
        b.push(schema.len() as u8);
        b.extend_from_slice(schema.as_bytes());
        b
    }

    fn parasolid_with_body(description: &str, schema: &str, body: &[u8]) -> Vec<u8> {
        let mut b = parasolid_payload(description, schema);
        b.extend_from_slice(body);
        b
    }

    pub fn outer_header() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&0x0000_0001u32.to_le_bytes());
        b.extend_from_slice(&0x0000_0004u32.to_be_bytes());
        b
    }

    pub fn synthetic_sldprt() -> Vec<u8> {
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

    pub fn sldprt_with_body(body: &[u8]) -> Vec<u8> {
        let mut f = outer_header();
        f.extend_from_slice(&make_block(
            0x20,
            "Contents/Config-0-Partition",
            &parasolid_with_body("partition body", "SCH_SW_33103_11000", body),
        ));
        f
    }

    pub fn sldprt_with_body_and_material(body: &[u8], name: &str, rgb: [u8; 3]) -> Vec<u8> {
        let mut f = sldprt_with_body(body);
        let mut material = b"moVisualProperties_c".to_vec();
        material.extend_from_slice(&u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 0]).to_le_bytes());
        material.extend_from_slice(&0u32.to_le_bytes());
        material.extend_from_slice(&0x00c0c0c0u32.to_le_bytes());
        material.extend_from_slice(&[0xff, 0xfe, 0xff, 0x00]);
        material.extend_from_slice(&[0xff, 0xfe, 0xff, name.len() as u8]);
        for unit in name.encode_utf16() {
            material.extend_from_slice(&unit.to_le_bytes());
        }
        f.extend(make_block(0x40, "SWObjects", &material));
        f
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

    pub fn sldprt_with_body_and_display_list(body: &[u8]) -> Vec<u8> {
        let mut f = sldprt_with_body(body);
        f.extend(make_block(
            0x41,
            "Contents/DisplayLists",
            &display_list_payload(),
        ));
        f
    }

    pub fn sldprt_with_partition_and_deltas(partition: &[u8]) -> Vec<u8> {
        let mut f = outer_header();
        f.extend_from_slice(&make_block(
            0x20,
            "Contents/Config-0-Partition",
            &parasolid_with_body("partition body", "SCH_SW_33103_11000", partition),
        ));
        f.extend_from_slice(&make_block(
            0x21,
            "Contents/Config-0-Deltas",
            &parasolid_with_body("deltas body", "SCH_SW_33103_11000", &[]),
        ));
        f
    }

    fn be16(b: &mut Vec<u8>, v: u16) {
        b.extend_from_slice(&v.to_be_bytes());
    }
    fn be32(b: &mut Vec<u8>, v: u32) {
        b.extend_from_slice(&v.to_be_bytes());
    }
    fn bef64(b: &mut Vec<u8>, v: f64) {
        b.extend_from_slice(&v.to_be_bytes());
    }

    fn plane_carrier(attr: u16, origin: [f64; 3], normal: [f64; 3], refdir: [f64; 3]) -> Vec<u8> {
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

    fn bridge(attr: u16, loop_attr: u16, surface_attr: u16) -> Vec<u8> {
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

    fn bridge_owned(attr: u16, loop_attr: u16, surface_attr: u16, owner: u16) -> Vec<u8> {
        let mut b = bridge(attr, loop_attr, surface_attr);
        b[8..10].copy_from_slice(&owner.to_be_bytes());
        b
    }

    fn loop_head(attr: u16, first_coedge: u16, bridge_attr: u16) -> Vec<u8> {
        let mut b = vec![0x00, 0x0f];
        be16(&mut b, attr);
        be32(&mut b, 0);
        for r in [0u16, first_coedge, bridge_attr, 0] {
            be16(&mut b, r);
        }
        b
    }

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
        be16(&mut b, attr);
        for r in [0u16, owner_loop, 0, next, start_vuse, twin, edge_use, 0, 0] {
            be16(&mut b, r);
        }
        b.push(if reversed { 0x2d } else { 0x2b });
        b
    }

    fn edge_use(attr: u16, curve_attr: u16) -> Vec<u8> {
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

    fn vertex_use(attr: u16, point_attr: u16) -> Vec<u8> {
        let mut b = vec![0x00, 0x12];
        be16(&mut b, attr);
        be32(&mut b, 0);
        for r in [0u16, 0, 0, 0, point_attr] {
            be16(&mut b, r);
        }
        b.extend_from_slice(&MAGIC);
        b
    }

    fn world_point(attr: u16, xyz: [f64; 3]) -> Vec<u8> {
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

    pub fn triangle_body_with_overlapping_point() -> Vec<u8> {
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

    pub fn sheet_body() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(entity51(2, 500, 0x0017, &[510, 700, 0, 0, 0, 0]));
        body.extend(entity51(2, 501, 0x0017, &[511, 701, 0, 0, 0, 0]));
        body.extend(entity51(1, 510, 0x001b, &[700, 0, 0, 0, 0, 0]));
        body.extend(entity51(1, 511, 0x001d, &[701, 0, 0, 0, 0, 0]));

        let mut tri1 = Vec::new();
        tri1.extend(plane_carrier(
            100,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        tri1.extend(bridge_owned(10, 20, 100, 700));
        tri1.extend(loop_head(20, 30, 10));
        tri1.extend(coedge(30, 20, 31, 50, 0, 40, false));
        tri1.extend(coedge(31, 20, 32, 51, 0, 41, false));
        tri1.extend(coedge(32, 20, 30, 52, 0, 42, false));
        tri1.extend(edge_use(40, 0));
        tri1.extend(edge_use(41, 0));
        tri1.extend(edge_use(42, 0));
        tri1.extend(vertex_use(50, 60));
        tri1.extend(vertex_use(51, 61));
        tri1.extend(vertex_use(52, 62));
        tri1.extend(world_point(60, [0.0, 0.0, 0.0]));
        tri1.extend(world_point(61, [1.0, 0.0, 0.0]));
        tri1.extend(world_point(62, [0.0, 1.0, 0.0]));
        body.extend(tri1);

        let mut tri2 = Vec::new();
        tri2.extend(plane_carrier(
            200,
            [10.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        tri2.extend(bridge_owned(210, 220, 200, 701));
        tri2.extend(loop_head(220, 230, 210));
        tri2.extend(coedge(230, 220, 231, 250, 0, 240, false));
        tri2.extend(coedge(231, 220, 232, 251, 0, 241, false));
        tri2.extend(coedge(232, 220, 230, 252, 0, 242, false));
        tri2.extend(edge_use(240, 0));
        tri2.extend(edge_use(241, 0));
        tri2.extend(edge_use(242, 0));
        tri2.extend(vertex_use(250, 260));
        tri2.extend(vertex_use(251, 261));
        tri2.extend(vertex_use(252, 262));
        tri2.extend(world_point(260, [10.0, 0.0, 0.0]));
        tri2.extend(world_point(261, [11.0, 0.0, 0.0]));
        tri2.extend(world_point(262, [10.0, 1.0, 0.0]));
        body.extend(tri2);

        body
    }

    pub fn two_owned_triangles() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(entity51(2, 500, 0x0017, &[700, 0, 0, 0, 0, 0]));
        body.extend(entity51(2, 501, 0x0017, &[701, 0, 0, 0, 0, 0]));

        let mut tri1 = Vec::new();
        tri1.extend(plane_carrier(
            100,
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        tri1.extend(bridge_owned(10, 20, 100, 700));
        tri1.extend(loop_head(20, 30, 10));
        tri1.extend(coedge(30, 20, 31, 50, 0, 40, false));
        tri1.extend(coedge(31, 20, 32, 51, 0, 41, false));
        tri1.extend(coedge(32, 20, 30, 52, 0, 42, false));
        tri1.extend(edge_use(40, 0));
        tri1.extend(edge_use(41, 0));
        tri1.extend(edge_use(42, 0));
        tri1.extend(vertex_use(50, 60));
        tri1.extend(vertex_use(51, 61));
        tri1.extend(vertex_use(52, 62));
        tri1.extend(world_point(60, [0.0, 0.0, 0.0]));
        tri1.extend(world_point(61, [1.0, 0.0, 0.0]));
        tri1.extend(world_point(62, [0.0, 1.0, 0.0]));
        body.extend(tri1);

        let mut tri2 = Vec::new();
        tri2.extend(plane_carrier(
            300,
            [10.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ));
        tri2.extend(bridge_owned(310, 320, 300, 701));
        tri2.extend(loop_head(320, 330, 310));
        tri2.extend(coedge(330, 320, 331, 350, 0, 340, false));
        tri2.extend(coedge(331, 320, 332, 351, 0, 341, false));
        tri2.extend(coedge(332, 320, 330, 352, 0, 342, false));
        tri2.extend(edge_use(340, 0));
        tri2.extend(edge_use(341, 0));
        tri2.extend(edge_use(342, 0));
        tri2.extend(vertex_use(350, 360));
        tri2.extend(vertex_use(351, 361));
        tri2.extend(vertex_use(352, 362));
        tri2.extend(world_point(360, [10.0, 0.0, 0.0]));
        tri2.extend(world_point(361, [11.0, 0.0, 0.0]));
        tri2.extend(world_point(362, [10.0, 1.0, 0.0]));
        body.extend(tri2);

        body
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

    pub fn triangle_with_nurbs_curve() -> Vec<u8> {
        let mut body = triangle_body();

        let wrapper_attr = 170u16;
        let descriptor_attr = 171u16;
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
        body.extend(b);

        let edge = body.windows(2).position(|w| w == [0x00, 0x10]).expect("required invariant");
        body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());

        body
    }

    pub fn triangle_with_nurbs_surface() -> Vec<u8> {
        let mut body = triangle_body();

        let wrapper_attr = 180u16;
        let descriptor_attr = 181u16;
        let bridge_attr = 10u16;
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
        body.extend(b);

        let bridge = body.windows(2).position(|w| w == [0x00, 0x0e]).expect("required invariant");
        body[bridge + 26..bridge + 28].copy_from_slice(&180u16.to_be_bytes());

        body
    }

    pub fn face_on_untyped_surface() -> Vec<u8> {
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
        body
    }

    pub fn triangle_with_line_curve() -> Vec<u8> {
        let mut body = triangle_body();
        body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        let edge = body.windows(2).position(|w| w == [0x00, 0x10]).expect("required invariant");
        body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
        body
    }
}

// ============================================================================
// CATIA seeds - comprehensive
// ============================================================================

fn generate_catia_seeds() {
    let dir = Path::new("seeds/catia_container");
    fs::create_dir_all(dir).expect("required invariant");

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", catia::outer_magic()),
        ("zero_entity", catia::zero_entity_catpart()),
        (
            "zero_entity_cylinder",
            catia::zero_entity_cylinder_catpart(),
        ),
        ("zero_entity_nurbs", catia::zero_entity_nurbs_catpart()),
        ("standard_nested", catia::standard_catpart()),
        ("e5_circle", catia::e5_catpart()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).expect("required invariant");
        println!("  catia/{} ({} bytes)", name, data.len());
    }
}

mod catia {
    const OUTER_MAGIC: &[u8; 8] = b"V5_CFV2\0";
    const DIR_MAGIC: &[u8; 16] = b"CATIA_V5 CB0001\0";

    pub fn outer_magic() -> Vec<u8> {
        OUTER_MAGIC.to_vec()
    }

    fn be32(v: u32) -> [u8; 4] {
        v.to_be_bytes()
    }
    fn le_f32(v: f32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn le_f64(v: f64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn be_f32(v: f32) -> [u8; 4] {
        v.to_be_bytes()
    }

    fn main_stream() -> Vec<u8> {
        let mut b = Vec::new();
        for _ in 0..2 {
            b.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
        }
        b.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
        for xyz in [[0.0f32, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]] {
            b.extend_from_slice(&[0x05, 0x08, 0x01]);
            for v in xyz {
                b.extend_from_slice(&le_f32(v));
            }
        }
        b
    }

    fn surf_stream() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
        b.push(0x00);
        b.push(0x1a);
        b.extend_from_slice(&[0x00, 0x33, 0x33]);
        for v in [0.0f32, 0.0, 0.0, 0.0, 0.0, 5.0] {
            b.extend_from_slice(&be_f32(v));
        }
        b
    }

    fn descriptor(name: &str, phys_off: u32, phys_len: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x54];
        b[0x0c..0x10].copy_from_slice(&be32(phys_len));
        let mut np = 0x10;
        for ch in name.chars() {
            b[np] = ch as u8;
            b[np + 1] = 0x00;
            np += 2;
        }
        b[0x50..0x54].copy_from_slice(&be32(1));
        b.extend_from_slice(&be32(phys_off));
        b.extend_from_slice(&be32(phys_len));
        b.extend_from_slice(&be32(phys_len));
        b.extend_from_slice(&be32(0));
        b.extend_from_slice(&be32(0));
        b
    }

    pub fn standard_catpart() -> Vec<u8> {
        let main = main_stream();
        let surf = surf_stream();
        let main_off = 16u32;
        let surf_off = main_off + main.len() as u32;
        let dir_rel = surf_off + surf.len() as u32;

        let mut dir = Vec::new();
        dir.extend_from_slice(DIR_MAGIC);
        dir.extend_from_slice(&descriptor("MainDataStream", main_off, main.len() as u32));
        dir.extend_from_slice(&descriptor("SurfacicReps", surf_off, surf.len() as u32));
        dir.extend_from_slice(b"CB__END");
        let b_len = dir.len() as u32;

        let mut inner = Vec::new();
        inner.extend_from_slice(OUTER_MAGIC);
        inner.extend_from_slice(&be32(dir_rel));
        inner.extend_from_slice(&be32(b_len));
        inner.extend_from_slice(&main);
        inner.extend_from_slice(&surf);
        inner.extend_from_slice(&dir);

        let mut f = Vec::new();
        f.extend_from_slice(OUTER_MAGIC);
        let outer_dir_off = 16u32 + inner.len() as u32;
        f.extend_from_slice(&be32(outer_dir_off));
        f.extend_from_slice(&be32(0));
        f.extend_from_slice(&inner);
        f
    }

    pub fn zero_entity_catpart() -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(OUTER_MAGIC);
        f.extend_from_slice(&be32(0));
        f.extend_from_slice(&be32(0));
        for _ in 0..5 {
            f.extend_from_slice(&[0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        f
    }

    pub fn zero_entity_cylinder_catpart() -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(OUTER_MAGIC);
        f.extend_from_slice(&be32(0));
        f.extend_from_slice(&be32(0));
        f.extend_from_slice(&[0xa9, 0x03, 0x28, 0x8a]);
        let mut payload = vec![0u8; 146];
        let write = |payload: &mut [u8], at: usize, value: f64| {
            payload[at..at + 8].copy_from_slice(&le_f64(value));
        };
        for (at, value) in [
            (8, 1.0),
            (16, 2.0),
            (24, 3.0),
            (33, 1.0),
            (65, 1.0),
            (81, 4.0),
        ] {
            write(&mut payload, at, value);
        }
        f.extend_from_slice(&payload);
        f
    }

    pub fn zero_entity_nurbs_catpart() -> Vec<u8> {
        let mut f = vec![0u8; 16];
        f[..8].copy_from_slice(OUTER_MAGIC);
        let record = f.len();
        f.extend_from_slice(&[0xa9, 0x03, 0x34, 0xc8]);
        f.resize(record + 4 + 300, 0);
        let write_f64 = |f: &mut [u8], at: usize, value: f64| {
            f[record + at..record + at + 8].copy_from_slice(&le_f64(value));
        };
        let write_token = |f: &mut [u8], at: usize, value: u32| {
            f[record + at] = 0x10;
            f[record + at + 1..record + at + 5].copy_from_slice(&value.to_le_bytes());
        };
        write_f64(&mut f, 23, 0.0);
        write_f64(&mut f, 31, 1.0);
        write_token(&mut f, 39, 3);
        write_token(&mut f, 44, 3);
        write_f64(&mut f, 50, 0.0);
        write_f64(&mut f, 58, 1.0);
        write_token(&mut f, 66, 3);
        write_token(&mut f, 71, 3);
        for i in 0..9 {
            let at = 79 + i * 24;
            write_f64(&mut f, at, i as f64);
            write_f64(&mut f, at + 8, (i / 3) as f64);
            write_f64(&mut f, at + 16, (i % 3) as f64);
        }
        f
    }

    fn e5_circle_stream() -> Vec<u8> {
        let mut record = vec![0u8; 113];
        record[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
        record[3] = 0xc9;
        record[5..7].copy_from_slice(&100u16.to_le_bytes());
        let write = |record: &mut [u8], at: usize, value: f64| {
            record[at..at + 8].copy_from_slice(&le_f64(value));
        };
        for (at, value) in [
            (14, 10.0),
            (22, 20.0),
            (30, 30.0),
            (38, 1.0),
            (70, 1.0),
            (86, 2.5),
        ] {
            write(&mut record, at, value);
        }
        record
    }

    pub fn e5_catpart() -> Vec<u8> {
        let main = e5_circle_stream();
        let surf = vec![0u8];
        let main_off = 16u32;
        let surf_off = main_off + main.len() as u32;
        let dir_rel = surf_off + surf.len() as u32;
        let mut dir = Vec::new();
        dir.extend_from_slice(DIR_MAGIC);
        dir.extend_from_slice(&descriptor("MainDataStream", main_off, main.len() as u32));
        dir.extend_from_slice(&descriptor("SurfacicReps", surf_off, surf.len() as u32));
        dir.extend_from_slice(b"CB__END");
        let mut inner = Vec::new();
        inner.extend_from_slice(OUTER_MAGIC);
        inner.extend_from_slice(&be32(dir_rel));
        inner.extend_from_slice(&be32(dir.len() as u32));
        inner.extend_from_slice(&main);
        inner.extend_from_slice(&surf);
        inner.extend_from_slice(&dir);
        let mut file = Vec::new();
        file.extend_from_slice(OUTER_MAGIC);
        file.extend_from_slice(&be32(16 + inner.len() as u32));
        file.extend_from_slice(&be32(0));
        file.extend_from_slice(&inner);
        file
    }
}

// ============================================================================
// CREO seeds - comprehensive
// ============================================================================

fn generate_creo_seeds() {
    let dir = Path::new("seeds/creo_container");
    fs::create_dir_all(dir).expect("required invariant");

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", creo::just_magic()),
        ("minimal_prt", creo::minimal_prt()),
        ("with_visibgeom", creo::with_visibgeom()),
        ("nd_layout", creo::nd_layout()),
        ("depdb_layout", creo::depdb_layout()),
        ("with_surface_rows", creo::with_surface_rows()),
        ("with_curve_prototypes", creo::with_curve_prototypes()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).expect("required invariant");
        println!("  creo/{} ({} bytes)", name, data.len());
    }
}

mod creo {
    pub fn just_magic() -> Vec<u8> {
        b"#UGC:2 P test\n".to_vec()
    }

    fn build_prt(version: &str, sections: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(format!("#UGC:2 P {version}\n").as_bytes());
        out.extend_from_slice(b"#-END_OF_UGC_HEADER\n");
        out.extend_from_slice(b"#UGC_TOC\n");
        out.extend_from_slice(b"toc entry line\n");
        out.extend_from_slice(b"#END_OF_TOC_HEADER\n");
        for (name, payload) in sections {
            out.push(b'#');
            out.push(b'\n');
            out.push(b'#');
            out.extend_from_slice(name.as_bytes());
            out.push(b'\n');
            out.extend_from_slice(payload);
        }
        out
    }

    fn visibgeom_payload(srf: u8, crv: u8) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(b"srf_array\0");
        p.extend_from_slice(&[0xf8, srf]);
        p.extend_from_slice(&[0xe0, 0x22, b'p', 0]);
        p.extend_from_slice(b"crv_array\0");
        p.extend_from_slice(&[0xf3, 0xf8, crv]);
        p
    }

    pub fn minimal_prt() -> Vec<u8> {
        build_prt("c", &[("VisibGeom", vec![0x00])])
    }
    pub fn with_visibgeom() -> Vec<u8> {
        build_prt("c", &[("VisibGeom", visibgeom_payload(5, 12))])
    }
    pub fn nd_layout() -> Vec<u8> {
        build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(3, 4))])
    }
    pub fn depdb_layout() -> Vec<u8> {
        build_prt(
            "c",
            &[("VisibGeom", vec![0x00]), ("DEPDB_DATA", vec![0x00, 0x01])],
        )
    }

    pub fn with_surface_rows() -> Vec<u8> {
        let mut payload = visibgeom_payload(2, 0);
        payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 8]);
        payload.extend_from_slice(&[8, 0x24, 4, 0xf6, 0x01, 0]);
        build_prt("c", &[("VisibGeom", payload)])
    }

    pub fn with_curve_prototypes() -> Vec<u8> {
        let mut payload = visibgeom_payload(0, 1);
        payload.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04");
        build_prt("c", &[("VisibGeom", payload)])
    }
}

// ============================================================================
// NX seeds - comprehensive
// ============================================================================

fn generate_nx_seeds() {
    let dir = Path::new("seeds/nx_container");
    fs::create_dir_all(dir).expect("required invariant");

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", nx::just_magic()),
        ("single_part", nx::single_part_prt()),
        ("assembly", nx::assembly_prt()),
        ("topology_part", nx::topology_part_prt()),
        ("bspline_part", nx::bspline_part_prt()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).expect("required invariant");
        println!("  nx/{} ({} bytes)", name, data.len());
    }
}

mod nx {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    const MAGIC: &[u8; 8] = b"SPLMSSTR";

    fn be_f64(v: f64) -> [u8; 8] {
        v.to_be_bytes()
    }

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

    fn record(tag: u8, len: usize) -> Vec<u8> {
        let mut r = vec![0u8; len];
        r[0] = 0x00;
        r[1] = tag;
        r
    }

    fn partition_stream() -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(b"PS\x00\x00");
        s.extend_from_slice(
            b"XX: TRANSMIT FILE (partition) created by modeller version 3400176\x00",
        );
        s.extend_from_slice(b"SCH_TEST_1_9999\x00");

        let mut pt = record(0x1d, 40);
        put_vec3(&mut pt, 16, [0.0625, 0.0, 0.0127]);
        s.extend_from_slice(&pt);

        let mut pl = record(0x32, 91);
        put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]);
        put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
        put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
        s.extend_from_slice(&pl);

        let mut cy = record(0x33, 99);
        put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
        put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
        put_f64(&mut cy, 67, 0.004_05);
        s.extend_from_slice(&cy);

        let mut ln = record(0x1e, 67);
        put_vec3(&mut ln, 19, [0.01, 0.02, 0.03]);
        put_vec3(&mut ln, 43, [1.0, 0.0, 0.0]);
        s.extend_from_slice(&ln);

        s
    }

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
        put_ref(&mut shell, 10, 2);
        put_ref(&mut shell, 14, 4);
        s.extend_from_slice(&shell);

        let mut face = record(14, 39);
        put_ref(&mut face, 2, 4);
        put_ref(&mut face, 22, 5);
        put_ref(&mut face, 24, 3);
        put_ref(&mut face, 26, 6);
        face[28] = b'+';
        s.extend_from_slice(&face);

        let mut loop_ = record(15, 16);
        put_ref(&mut loop_, 2, 5);
        put_ref(&mut loop_, 10, 7);
        put_ref(&mut loop_, 12, 4);
        s.extend_from_slice(&loop_);

        let mut fin = record(17, 23);
        put_ref(&mut fin, 2, 7);
        put_ref(&mut fin, 6, 5);
        put_ref(&mut fin, 8, 7);
        put_ref(&mut fin, 10, 7);
        put_ref(&mut fin, 12, 10);
        put_ref(&mut fin, 16, 8);
        put_ref(&mut fin, 18, 9);
        fin[22] = b'+';
        s.extend_from_slice(&fin);

        let mut edge = record(16, 32);
        put_ref(&mut edge, 2, 8);
        put_ref(&mut edge, 18, 7);
        put_ref(&mut edge, 24, 9);
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
        put_ref(&mut vertex, 16, 11);
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

    fn zlib_compress(raw: &[u8]) -> Vec<u8> {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
        e.write_all(raw).expect("required invariant");
        e.finish().expect("required invariant")
    }

    pub fn just_magic() -> Vec<u8> {
        MAGIC.to_vec()
    }

    pub fn single_part_prt() -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(MAGIC);
        f.push(0x06);
        f.extend_from_slice(&[0x11, 0x22, 0x33]);
        f.extend_from_slice(&[0, 0, 0, 0]);
        f.push(0x00);
        f.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        f.extend_from_slice(&[0, 0]);

        f.extend_from_slice(b"HEADER");
        let name = b"/Root/UG_PART/UG_PART";
        f.extend_from_slice(&(name.len() as u32).to_le_bytes());
        f.extend_from_slice(name);

        let blob = zlib_compress(&partition_stream());
        let dir_end = f.len() + 16;
        let blob_off = dir_end as u64;
        f.extend_from_slice(&blob_off.to_le_bytes());
        f.extend_from_slice(&(blob.len() as u64).to_le_bytes());
        f.extend_from_slice(&blob);
        f
    }

    pub fn topology_part_prt() -> Vec<u8> {
        prt_with_partition(&topology_partition_stream())
    }
    pub fn bspline_part_prt() -> Vec<u8> {
        prt_with_partition(&bspline_partition_stream())
    }

    fn prt_with_partition(stream: &[u8]) -> Vec<u8> {
        let mut f = single_part_prt();
        let compressed = zlib_compress(stream);
        let len = f.len();
        f.truncate(len - compressed.len());
        let blob_off = f.len() as u64;
        let off_idx = f.len() - 16;
        let size_idx = f.len() - 8;
        f[off_idx..size_idx].copy_from_slice(&blob_off.to_le_bytes());
        f[size_idx..].copy_from_slice(&(compressed.len() as u64).to_le_bytes());
        f.extend_from_slice(&compressed);
        f
    }

    pub fn assembly_prt() -> Vec<u8> {
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
        f.extend_from_slice(&[0u8; 16]);
        f
    }
}
