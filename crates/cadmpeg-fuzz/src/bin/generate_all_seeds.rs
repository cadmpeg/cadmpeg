// SPDX-License-Identifier: Apache-2.0
//! Writes structural container and IR seeds, then derives deterministic
//! truncation, byte-flip, and oversized-length mutants.

use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use flate2::write::DeflateEncoder;
use flate2::Compression;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

fn main() {
    generate_f3d_seeds();
    generate_sldprt_seeds();
    generate_catia_seeds();
    generate_creo_seeds();
    generate_nx_seeds();
    generate_ir_seeds();
    generate_mutated_seeds();
    println!("All seeds generated.");
}

// ============================================================================
// F3D seeds
// ============================================================================

fn generate_f3d_seeds() {
    let dir = Path::new("seeds/f3d_container");
    fs::create_dir_all(dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty_zip", f3d::empty_zip()),
        ("bare_zip_with_txt", f3d::bare_zip_with_txt()),
        (
            "synthetic_smbh_header_only",
            f3d::f3d_with_smbh(&f3d::synthetic_smbh()),
        ),
        (
            "synthetic_geometry",
            f3d::f3d_with_smbh(&f3d::synthetic_geometry_smbh()),
        ),
        (
            "synthetic_mixed",
            f3d::f3d_with_smbh(&f3d::synthetic_mixed_smbh()),
        ),
        ("full_f3d_with_smbh", f3d::synthetic_f3d(true)),
        ("full_f3d_smb_only", f3d::synthetic_f3d(false)),
        ("corrupt_zip_magic", f3d::corrupt_zip_magic()),
        ("truncated_smbh", f3d::truncated_smbh()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
        println!("  f3d/{} ({} bytes)", name, data.len());
    }
}

mod f3d {
    use super::*;

    pub fn empty_zip() -> Vec<u8> {
        zip::ZipWriter::new(Cursor::new(Vec::new()))
            .finish()
            .unwrap()
            .into_inner()
    }

    pub fn bare_zip_with_txt() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("readme.txt", stored).unwrap();
        zip.write_all(b"hello").unwrap();
        zip.finish().unwrap().into_inner()
    }

    pub fn corrupt_zip_magic() -> Vec<u8> {
        let mut data = empty_zip();
        data[0] = 0xFF;
        data[1] = 0xFF;
        data
    }

    pub fn truncated_smbh() -> Vec<u8> {
        let mut smbh = synthetic_smbh();
        smbh.truncate(60);
        f3d_with_smbh(&smbh)
    }

    fn push_u8_string(b: &mut Vec<u8>, s: &str) {
        b.push(0x07);
        b.push(s.len() as u8);
        b.extend_from_slice(s.as_bytes());
    }

    fn push_tagged_f64(b: &mut Vec<u8>, v: f64) {
        b.push(0x06);
        b.extend_from_slice(&v.to_le_bytes());
    }

    fn smbh_header_prefix() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"ASM BinaryFile8<");
        b.extend_from_slice(&[0u8; 8]);
        b.extend_from_slice(&7u64.to_be_bytes());
        b.extend_from_slice(&3u64.to_be_bytes());
        b.extend_from_slice(&[0u8; 7]);
        push_u8_string(&mut b, "Autodesk Neutron");
        push_u8_string(&mut b, "ASM 231.6.3.65535 OSX");
        push_u8_string(&mut b, "Tue Mar 31 16:16:19 2026");
        push_tagged_f64(&mut b, 60.0);
        push_tagged_f64(&mut b, 1e-6);
        push_tagged_f64(&mut b, 1e-10);
        b
    }

    pub fn synthetic_smbh() -> Vec<u8> {
        let mut b = smbh_header_prefix();
        b.extend_from_slice(&[0x0d, 0x04, b'b', b'o', b'd', b'y', 0x11]);
        let active_len = b.len();
        b.extend_from_slice(&[0x11, 0x0d, 0x0b]);
        b.extend_from_slice(b"delta_state");
        b.extend_from_slice(&[0u8; 16]);
        assert_eq!(&b[active_len + 3..active_len + 3 + 11], b"delta_state");
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

    pub fn synthetic_geometry_smbh() -> Vec<u8> {
        let mut r = Vec::new();
        t_ident(&mut r, "asmheader");
        push_u8_string(&mut r, "231.6.3.65535");
        t_end(&mut r);

        t_ident(&mut r, "body");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 2);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_end(&mut r);

        t_ident(&mut r, "region");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 3);
        t_ref(&mut r, 1);
        t_end(&mut r);

        t_ident(&mut r, "shell");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 4);
        t_ref(&mut r, -1);
        t_ref(&mut r, 2);
        t_end(&mut r);

        t_ident(&mut r, "face");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 5);
        t_ref(&mut r, 3);
        t_ref(&mut r, -1);
        t_ref(&mut r, 6);
        r.push(0x0b);
        r.push(0x0b);
        t_end(&mut r);

        t_ident(&mut r, "loop");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 7);
        t_ref(&mut r, 4);
        t_end(&mut r);

        t_subident(&mut r, "plane");
        t_ident(&mut r, "surface");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, [0.0, 0.0, 0.0]);
        t_vec(&mut r, [0.0, 0.0, 1.0]);
        t_pos(&mut r, [1.0, 0.0, 0.0]);
        r.push(0x0b);
        t_end(&mut r);

        let coedges = [(7i64, 8, 9, 10), (8, 9, 7, 11), (9, 7, 8, 12)];
        for (_id, next, prev, edge) in coedges {
            t_ident(&mut r, "coedge");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, next);
            t_ref(&mut r, prev);
            t_ref(&mut r, -1);
            t_ref(&mut r, edge);
            r.push(0x0b);
            t_ref(&mut r, 5);
            t_long(&mut r, 0);
            t_ref(&mut r, -1);
            t_end(&mut r);
        }

        let edges = [(10i64, 13, 14), (11, 14, 15), (12, 15, 13)];
        for (_id, start, end) in edges {
            t_ident(&mut r, "edge");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, start);
            t_dbl(&mut r, 0.0);
            t_ref(&mut r, end);
            t_dbl(&mut r, 1.0);
            t_ref(&mut r, -1);
            t_ref(&mut r, -1);
            r.push(0x0b);
            push_u8_string(&mut r, "unknown");
            t_end(&mut r);
        }

        let verts = [(13i64, 10, 16), (14, 11, 17), (15, 12, 18)];
        for (_id, edge, point) in verts {
            t_ident(&mut r, "vertex");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_ref(&mut r, edge);
            t_long(&mut r, 0);
            t_ref(&mut r, point);
            t_end(&mut r);
        }

        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            t_ident(&mut r, "point");
            t_ref(&mut r, -1);
            t_long(&mut r, -1);
            t_ref(&mut r, -1);
            t_pos(&mut r, p);
            t_long(&mut r, 1);
            t_end(&mut r);
        }

        t_ident(&mut r, "delta_state");
        let mut out = smbh_header_prefix();
        out.extend_from_slice(&r);
        out
    }

    pub fn synthetic_mixed_smbh() -> Vec<u8> {
        let mut r = Vec::new();
        t_ident(&mut r, "asmheader");
        push_u8_string(&mut r, "231.6.3.65535");
        t_end(&mut r);

        t_ident(&mut r, "body");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 2);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_end(&mut r);

        t_ident(&mut r, "region");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 3);
        t_ref(&mut r, 1);
        t_end(&mut r);

        t_ident(&mut r, "shell");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, -1);
        t_ref(&mut r, 4);
        t_ref(&mut r, -1);
        t_ref(&mut r, 2);
        t_end(&mut r);

        let face = |r: &mut Vec<u8>, next: i64, first_loop: i64, surface: i64| {
            t_ident(r, "face");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, next);
            t_ref(r, first_loop);
            t_ref(r, 3);
            t_ref(r, -1);
            t_ref(r, surface);
            r.push(0x0b);
            r.push(0x0b);
            t_end(r);
        };
        face(&mut r, 5, 6, 8);
        face(&mut r, -1, 7, 9);

        let lp = |r: &mut Vec<u8>, first_coedge: i64, owner_face: i64| {
            t_ident(r, "loop");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, -1);
            t_ref(r, first_coedge);
            t_ref(r, owner_face);
            t_end(r);
        };
        lp(&mut r, 10, 4);
        lp(&mut r, 13, 5);

        t_subident(&mut r, "plane");
        t_ident(&mut r, "surface");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, [0.0, 0.0, 0.0]);
        t_vec(&mut r, [0.0, 0.0, 1.0]);
        t_pos(&mut r, [1.0, 0.0, 0.0]);
        r.push(0x0b);
        t_end(&mut r);

        t_subident(&mut r, "spline");
        t_ident(&mut r, "surface");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_dbl(&mut r, 0.0);
        r.push(0x0b);
        t_end(&mut r);

        let ce = |r: &mut Vec<u8>,
                  next: i64,
                  prev: i64,
                  partner: i64,
                  edge: i64,
                  rev: bool,
                  owner: i64| {
            t_ident(r, "coedge");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, next);
            t_ref(r, prev);
            t_ref(r, partner);
            t_ref(r, edge);
            r.push(if rev { 0x0a } else { 0x0b });
            t_ref(r, owner);
            t_long(r, 0);
            t_ref(r, -1);
            t_end(r);
        };
        ce(&mut r, 11, 12, 13, 16, false, 6);
        ce(&mut r, 12, 10, -1, 17, false, 6);
        ce(&mut r, 10, 11, -1, 18, false, 6);
        ce(&mut r, 14, 15, 10, 16, true, 7);
        ce(&mut r, 15, 13, -1, 19, false, 7);
        ce(&mut r, 13, 14, -1, 20, false, 7);

        let edge = |r: &mut Vec<u8>, start: i64, end: i64| {
            t_ident(r, "edge");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, start);
            t_dbl(r, 0.0);
            t_ref(r, end);
            t_dbl(r, 1.0);
            t_ref(r, -1);
            t_ref(r, -1);
            r.push(0x0b);
            push_u8_string(r, "unknown");
            t_end(r);
        };
        edge(&mut r, 21, 22);
        edge(&mut r, 22, 23);
        edge(&mut r, 23, 21);
        edge(&mut r, 21, 24);
        edge(&mut r, 24, 22);

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
        vert(&mut r, 16, 25);
        vert(&mut r, 16, 26);
        vert(&mut r, 17, 27);
        vert(&mut r, 19, 28);

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

        t_ident(&mut r, "delta_state");
        let mut out = smbh_header_prefix();
        out.extend_from_slice(&r);
        out
    }

    pub fn f3d_with_smbh(smbh: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("Manifest.dat", stored).unwrap();
        zip.write_all(b"synthetic-manifest").unwrap();
        zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
            .unwrap();
        zip.write_all(smbh).unwrap();
        zip.finish().unwrap().into_inner()
    }

    pub fn synthetic_f3d(include_smbh: bool) -> Vec<u8> {
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

        let mut smb = synthetic_smbh();
        smb.truncate(60);
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

        zip.finish().unwrap().into_inner()
    }
}

// ============================================================================
// SLDPRT seeds
// ============================================================================

fn generate_sldprt_seeds() {
    let dir = Path::new("seeds/sldprt_container");
    fs::create_dir_all(dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_header", sldprt::outer_header()),
        ("synthetic_sldprt", sldprt::synthetic_sldprt()),
        (
            "with_triangle_body",
            sldprt::sldprt_with_body(&sldprt::triangle_body()),
        ),
        (
            "with_cylinder",
            sldprt::sldprt_with_body(&sldprt::closed_cylinder_body()),
        ),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
        println!("  sldprt/{} ({} bytes)", name, data.len());
    }
}

mod sldprt {
    use super::*;

    pub const MARKER: [u8; 4] = [0x9e, 0x14, 0x01, 0x00];

    fn swap_name(name: &str) -> Vec<u8> {
        name.bytes().map(|b| b.rotate_left(4)).collect()
    }

    fn raw_deflate(data: &[u8]) -> Vec<u8> {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
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
        f.extend_from_slice(&make_block(0x20, "Contents/Config-0-Partition", &{
            let mut p = parasolid_payload("partition body", "SCH_SW_33103_11000");
            p.extend_from_slice(body);
            p
        }));
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

    const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

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
}

// ============================================================================
// CATIA seeds
// ============================================================================

fn generate_catia_seeds() {
    let dir = Path::new("seeds/catia_container");
    fs::create_dir_all(dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", catia::outer_magic()),
        ("zero_entity", catia::zero_entity_catpart()),
        ("standard_nested", catia::standard_catpart()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
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
}

// ============================================================================
// CREO seeds
// ============================================================================

fn generate_creo_seeds() {
    let dir = Path::new("seeds/creo_container");
    fs::create_dir_all(dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", creo::just_magic()),
        ("minimal_prt", creo::minimal_prt()),
        ("with_visibgeom", creo::with_visibgeom()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
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
}

// ============================================================================
// NX seeds
// ============================================================================

fn generate_nx_seeds() {
    let dir = Path::new("seeds/nx_container");
    fs::create_dir_all(dir).unwrap();

    let seeds: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("just_magic", nx::just_magic()),
        ("single_part", nx::single_part_prt()),
        ("assembly", nx::assembly_prt()),
    ];

    for (name, data) in seeds {
        fs::write(dir.join(name), &data).unwrap();
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

    fn zlib_compress(raw: &[u8]) -> Vec<u8> {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
        e.write_all(raw).unwrap();
        e.finish().unwrap()
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

// ============================================================================
// IR seeds
// ============================================================================

fn generate_ir_seeds() {
    let minimal = cadmpeg_ir::CadIr::empty(Default::default())
        .to_canonical_json()
        .unwrap();
    let cube = cadmpeg_ir::examples::unit_cube()
        .to_canonical_json()
        .unwrap();
    let directed_subd_sum = cadmpeg_ir::examples::directed_subd_sum()
        .to_canonical_json()
        .unwrap();
    let canonical = [
        ("minimal_v13.json", minimal.as_bytes()),
        ("unit_cube_v13.json", cube.as_bytes()),
        ("directed_subd_sum_v13.json", directed_subd_sum.as_bytes()),
    ];
    let valid_v0 = minimal.replacen(r#""ir_version": "54""#, r#""ir_version": "0""#, 1);
    let current_version_field = format!(r#""ir_version": "{}""#, cadmpeg_ir::IR_VERSION);
    let valid_v0 = minimal.replacen(&current_version_field, r#""ir_version": "0""#, 1);
    assert_ne!(valid_v0, minimal, "current ir_version field must match");

    let from_json = Path::new("seeds/ir_from_json");
    replace_seed_directory(from_json);
    for (name, data) in &canonical {
        fs::write(from_json.join(name), data).unwrap();
        println!("  ir/{name} ({} bytes)", data.len());
    }
    fs::write(from_json.join("valid_v0_rejected.json"), valid_v0).unwrap();

    let migrate = Path::new("seeds/ir_migrate_json");
    replace_seed_directory(migrate);
    for (name, data) in &canonical {
        let legacy = std::str::from_utf8(data).unwrap().replacen(
            r#""ir_version": "54""#,
            r#""ir_version": "53""#,
        let current = std::str::from_utf8(data).unwrap();
        let legacy = current.replacen(
            &current_version_field,
            &format!(r#""ir_version": "{}""#, cadmpeg_ir::PREVIOUS_IR_VERSION),
            1,
        );
        assert_ne!(legacy, current, "current ir_version field must match");
        fs::write(migrate.join(name.replace("_v13.json", "_v12.json")), legacy).unwrap();
    }

    for target in ["ir_validate", "ir_canonical_roundtrip", "step_writer"] {
        let dir = Path::new("seeds").join(target);
        replace_seed_directory(&dir);
        for (name, data) in &canonical {
            fs::write(dir.join(name), data).unwrap();
        }
    }

    let mutated = Path::new("seeds/ir_validate_mutated");
    replace_seed_directory(mutated);
    for (index, (name, data)) in canonical.iter().enumerate() {
        let mut input = vec![index as u8];
        input.extend_from_slice(data);
        fs::write(mutated.join(name), input).unwrap();
    }

    let custom = Path::new("seeds/step_writer_custom");
    replace_seed_directory(custom);
    for (index, (name, data)) in canonical.iter().enumerate() {
        let mut input = vec![index as u8; 8];
        input.extend_from_slice(data);
        fs::write(custom.join(name), input).unwrap();
    }

    let diff = Path::new("seeds/ir_diff");
    replace_seed_directory(diff);
    for (name, selector, left, right) in [
        (
            "minimal_vs_minimal",
            0_u8,
            minimal.as_bytes(),
            minimal.as_bytes(),
        ),
        ("minimal_vs_cube", 1_u8, minimal.as_bytes(), cube.as_bytes()),
        ("cube_vs_minimal", 2_u8, cube.as_bytes(), minimal.as_bytes()),
    ] {
        let mut input = vec![selector];
        input.extend_from_slice(left);
        input.push(0);
        input.extend_from_slice(right);
        fs::write(diff.join(name), input).unwrap();
    }
}

fn replace_seed_directory(directory: &Path) {
    fs::create_dir_all(directory).unwrap();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
}

// ============================================================================
// Mutated seeds: deterministic corruptions of every structurally valid seed
// ============================================================================

const MUTANT_SUFFIXES: [&str; 3] = [".mut_trunc", ".mut_flip", ".mut_lenmax"];

/// For each seed emitted above, write three deterministic corruptions:
/// - `.mut_trunc`: cut at 50% length (mid-record truncation)
/// - `.mut_flip`: invert the byte at 50% offset (payload corruption past the header)
/// - `.mut_lenmax`: saturate 4 bytes at 25% offset to 0xFF (oversized count/length fields)
///
/// Mutants are derived only from files this run just wrote, never from other
/// mutants, so regeneration is idempotent.
fn generate_mutated_seeds() {
    let container_dirs = [
        "seeds/f3d_container",
        "seeds/sldprt_container",
        "seeds/catia_container",
        "seeds/creo_container",
        "seeds/nx_container",
    ];
    for dir in container_dirs {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if MUTANT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
                fs::remove_file(path).unwrap();
            }
        }
    }

    for dir in ["seeds/ir_from_json"] {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if MUTANT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
                fs::remove_file(path).unwrap();
            }
        }
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                let name = p.file_name().unwrap().to_str().unwrap();
                p.is_file() && !MUTANT_SUFFIXES.iter().any(|s| name.ends_with(s))
            })
            .collect();
        entries.sort();
        for path in entries {
            let data = fs::read(&path).unwrap();
            // Too small to have structure past the magic; corruptions would
            // duplicate the existing bad-magic/truncation seeds.
            if data.len() < 32 {
                continue;
            }
            let name = path.file_name().unwrap().to_str().unwrap().to_string();

            let mut trunc = data.clone();
            trunc.truncate(data.len() / 2);

            let mut flip = data.clone();
            let mid = data.len() / 2;
            flip[mid] = !flip[mid];

            let mut lenmax = data.clone();
            let off = data.len() / 4;
            for b in &mut lenmax[off..(off + 4).min(data.len())] {
                *b = 0xFF;
            }

            for (suffix, mutant) in MUTANT_SUFFIXES.iter().zip([trunc, flip, lenmax]) {
                let out = path.with_file_name(format!("{name}{suffix}"));
                fs::write(&out, &mutant).unwrap();
                println!("  {} ({} bytes)", out.display(), mutant.len());
            }
        }
    }
}
