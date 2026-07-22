// SPDX-License-Identifier: Apache-2.0
//! Writes structural seeds for the F3D container and replay fuzz targets.

use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

fn main() {
    let seeds = [
        ("empty_zip", empty_zip()),
        ("bare_zip_with_txt", bare_zip_with_txt()),
        (
            "synthetic_smbh_header_only",
            f3d_with_smbh(&synthetic_smbh()),
        ),
        (
            "synthetic_geometry",
            f3d_with_smbh(&synthetic_geometry_smbh()),
        ),
        ("synthetic_mixed", f3d_with_smbh(&synthetic_mixed_smbh())),
        (
            "synthetic_with_pcurve",
            f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh()),
        ),
        ("full_f3d_with_smbh", synthetic_f3d(true)),
        ("full_f3d_smb_only", synthetic_f3d(false)),
        ("corrupt_zip_magic", corrupt_zip_magic()),
        ("truncated_smbh", truncated_smbh()),
        (
            "binary_file4_width",
            f3d_with_smbh(&synthetic_binary_file4()),
        ),
    ];

    for directory in ["seeds/f3d_container", "seeds/f3d_roundtrip"] {
        let seeds_dir = Path::new(directory);
        fs::create_dir_all(seeds_dir).expect("create seeds dir");
        for (name, data) in &seeds {
            let path = seeds_dir.join(name);
            fs::write(&path, data).expect("write seed");
            println!("wrote {} ({} bytes)", path.display(), data.len());
        }
    }
}

fn empty_zip() -> Vec<u8> {
    let zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.finish().expect("required invariant").into_inner()
}

fn bare_zip_with_txt() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("readme.txt", stored).expect("required invariant");
    zip.write_all(b"hello").expect("required invariant");
    zip.finish().expect("required invariant").into_inner()
}

fn corrupt_zip_magic() -> Vec<u8> {
    let mut data = empty_zip();
    data[0] = 0xFF;
    data[1] = 0xFF;
    data
}

fn truncated_smbh() -> Vec<u8> {
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

fn push_tagged_i64(b: &mut Vec<u8>, tag: u8, v: i64) {
    b.push(tag);
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

fn synthetic_smbh() -> Vec<u8> {
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

    b.extend_from_slice(&[0x0d, 0x04, b'b', b'o', b'd', b'y', 0x11]);
    let active_len = b.len();

    b.extend_from_slice(&[0x11, 0x0d, 0x0b]);
    b.extend_from_slice(b"delta_state");
    b.extend_from_slice(&[0u8; 16]);

    assert_eq!(&b[active_len + 3..active_len + 3 + 11], b"delta_state");
    b
}

fn synthetic_binary_file4() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile4<");
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

    b.extend_from_slice(&[0x11, 0x0d, 0x0b]);
    b.extend_from_slice(b"delta_state");
    b.extend_from_slice(&[0u8; 16]);
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

fn synthetic_geometry_smbh() -> Vec<u8> {
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

    t_ident(&mut r, "delta_state");

    let mut out = smbh_header_prefix();
    out.extend_from_slice(&r);
    out
}

fn synthetic_geometry_with_pcurve_smbh() -> Vec<u8> {
    let mut bytes = synthetic_geometry_smbh();
    let start = find_record_stream_start(&bytes).expect("required invariant");
    let limit = find_delta_state_offset(&bytes).expect("required invariant");
    let records = frame_records(&bytes, start, limit);
    let coedge = &records[7];
    let record = &mut bytes[coedge.0..coedge.0 + coedge.1];
    let pcurve_ref_tag = record.iter().rposition(|b| *b == 0x0c).expect("required invariant");
    record[pcurve_ref_tag + 1..pcurve_ref_tag + 9].copy_from_slice(&19i64.to_le_bytes());

    let delta = bytes[..]
        .windows(b"delta_state".len())
        .position(|w| w == b"delta_state")
        .expect("required invariant")
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

fn synthetic_mixed_smbh() -> Vec<u8> {
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

    let ce =
        |r: &mut Vec<u8>, next: i64, prev: i64, partner: i64, edge: i64, rev: bool, owner: i64| {
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

fn f3d_with_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("Manifest.dat", stored).expect("required invariant");
    zip.write_all(b"synthetic-manifest").expect("required invariant");
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .expect("required invariant");
    zip.write_all(smbh).expect("required invariant");
    zip.finish().expect("required invariant").into_inner()
}

fn synthetic_f3d(include_smbh: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let folder = "FusionAssetName[Active]";
    zip.start_file("Manifest.dat", stored).expect("required invariant");
    zip.write_all(b"synthetic-manifest").expect("required invariant");

    if include_smbh {
        zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smbh"), deflated)
            .expect("required invariant");
        zip.write_all(&synthetic_smbh()).expect("required invariant");
    }

    let mut smb = synthetic_smbh();
    smb.truncate(60);
    zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smb"), stored)
        .expect("required invariant");
    zip.write_all(&smb).expect("required invariant");

    zip.start_file(
        format!("{folder}/FusionDesignSegmentType1/BulkStream.dat"),
        stored,
    )
    .expect("required invariant");
    zip.write_all(b"design-bulk").expect("required invariant");

    zip.start_file(format!("{folder}/Previews/thumbnail.png"), stored)
        .expect("required invariant");
    zip.write_all(b"\x89PNG").expect("required invariant");

    let cursor = zip.finish().expect("required invariant");
    cursor.into_inner()
}

fn find_record_stream_start(bytes: &[u8]) -> Option<usize> {
    let magic = b"ASM BinaryFile";
    let pos = bytes.windows(magic.len()).position(|w| w == magic)?;
    let after_magic = pos + magic.len();
    if bytes.get(after_magic..after_magic + 1)?[0] == b'8' {
        Some(after_magic + 1 + 8 + 8 + 8 + 8 + 7)
    } else {
        None
    }
}

fn find_delta_state_offset(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(b"delta_state".len())
        .position(|w| w == b"delta_state")
}

fn frame_records(bytes: &[u8], start: usize, limit: usize) -> Vec<(usize, usize)> {
    let mut records = Vec::new();
    let mut pos = start;
    while pos < limit {
        let rec_start = pos;
        if bytes.get(pos) == Some(&0x0d) || bytes.get(pos) == Some(&0x0e) {
            let len = bytes.get(pos + 1).copied().unwrap_or(0) as usize;
            pos += 2 + len;
        }
        while pos < limit {
            match bytes.get(pos) {
                Some(0x11) => {
                    pos += 1;
                    break;
                }
                Some(0x0c) => pos += 9,
                Some(0x04) => pos += 9,
                Some(0x06) => pos += 9,
                Some(0x13) => pos += 25,
                Some(0x14) => pos += 25,
                Some(0x07) => {
                    let len = bytes.get(pos + 1).copied().unwrap_or(0) as usize;
                    pos += 2 + len;
                }
                Some(0x0b) | Some(0x0a) => pos += 1,
                Some(0x0d) => {
                    let len = bytes.get(pos + 1).copied().unwrap_or(0) as usize;
                    pos += 2 + len;
                }
                Some(0x0e) => {
                    let len = bytes.get(pos + 1).copied().unwrap_or(0) as usize;
                    pos += 2 + len;
                }
                _ => pos += 1,
            }
        }
        records.push((rec_start, pos - rec_start));
    }
    records
}
