// SPDX-License-Identifier: Apache-2.0
//! Outer-container wrapping and variant CATPart images.

#![allow(clippy::unwrap_used)]
use crate::container::{DIR_MAGIC, OUTER_MAGIC};

use super::test_topology::{
    fbb_only_quad_surface_stream, fbb_only_quad_topology_stream,
    fbb_only_quad_unmatched_edge_topology_stream,
};
use super::{be32, be_f32, le_f32, le_f64};

pub(crate) fn summary_preview_segment() -> Vec<u8> {
    let mut bytes = b"FINJPL  \x01\x01\x00\x03\x00\x00\x00\x15\x00CATSummaryInformation".to_vec();
    bytes.extend_from_slice(b"LastSaveVersion\0<Version>5/<Version><Release>27/<Release><ServicePack>2/<ServicePack><BuildDate>03-10-2017.22.00/<BuildDate><HotFix>0/<HotFix>\0");
    bytes.extend_from_slice(&[
        0xff, 0xd8, // SOI
        0xff, 0xc0, 0x00, 0x0b, 8, 0x01, 0x20, 0x02, 0x80, 1, 1, 0x11, 0, 0xff, 0xda, 0x00, 0x08,
        1, 1, 0, 0, 0x3f, 0, 0x11, 0x22, 0xff, 0x00, 0x33, 0xff, 0xd9, // EOI
    ]);
    bytes.extend_from_slice(b"summary-tail");
    bytes
}

pub(crate) fn external_reference_segment(target: &str) -> Vec<u8> {
    let mut bytes = b"FINJPL  \x01\x01\x00\x02\x00\x00\x00\x0a\x00CATPreview".to_vec();
    for value in ["CATStorageProperty", "CATUnicodeString"] {
        bytes.push(0x34);
        bytes.push(u8::try_from(value.len()).unwrap());
        bytes.extend_from_slice(value.as_bytes());
        let suffix: &[u8] = if value == "CATStorageProperty" {
            &[
                0x80, 0x01, 0, 0, 0, 0, 0x22, 0x0c, 0, 0, 0, 0x34, 0x01, 0x01, 0x00,
            ]
        } else {
            &[0xa0, 0x02, 0, 0, 0, 0]
        };
        bytes.extend_from_slice(suffix);
    }
    bytes.extend_from_slice(&[0x34, 5]);
    bytes.extend_from_slice(b"CATIA");
    bytes.extend_from_slice(&[0x9f, 0xa0, 0x02, 0, 0, 0, 0, 0x34]);
    bytes.push(u8::try_from(target.len()).unwrap());
    bytes.extend_from_slice(target.as_bytes());
    bytes.push(0x9f);
    bytes
}

pub(crate) fn outer_body_catpart(body: &[u8]) -> Vec<u8> {
    let directory_length = DIR_MAGIC.len();
    let directory_offset = 16usize.checked_add(body.len()).expect("bounded outer body");
    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(
        u32::try_from(directory_offset).expect("bounded directory offset"),
    ));
    file.extend_from_slice(&be32(
        u32::try_from(directory_length).expect("bounded directory length"),
    ));
    file.extend_from_slice(body);
    file.extend_from_slice(DIR_MAGIC);
    file
}

pub(crate) fn fbb_only_quad_catpart() -> Vec<u8> {
    standard_catpart_from_streams(
        &fbb_only_quad_topology_stream(),
        &fbb_only_quad_surface_stream(),
    )
}

pub(crate) fn fbb_only_quad_unmatched_edge_catpart() -> Vec<u8> {
    standard_catpart_from_streams(
        &fbb_only_quad_unmatched_edge_topology_stream(),
        &fbb_only_quad_surface_stream(),
    )
}

/// A `MainDataStream` physical payload: two FBB spine rows, two empty standard
/// edge tables, and a counted table of three `05 08 01` vertex records.
pub(crate) fn main_stream() -> Vec<u8> {
    let mut b = Vec::new();
    // Non-planar positional packet for the first, cylindrical face.
    b.extend_from_slice(&[0x01, 0x41, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00]);
    b.extend_from_slice(&[0, 0, 0, 1, 0, 2]);
    // Planar packet for the second face, with a byte-stored +Z normal.
    b.extend_from_slice(&[0x01, 0x49, 0x01, 0xff, 0x03, 0x00, 0x00, 0x00]);
    for value in [0.0f32, 0.0, 1.0] {
        b.extend_from_slice(&le_f32(value));
    }
    b.extend_from_slice(&[0, 0, 0, 1, 0, 2]);
    // Two stride-8 FBB rows (`30 04 04 ff` + 4 constant bytes).
    for _ in 0..2 {
        b.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    }
    for kind in [1, 2] {
        b.extend_from_slice(&[0x01, kind, 0]);
        b.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    }
    // Counted vertex table: three records (3×f32 LE, millimetres).
    b.extend_from_slice(&[0x01, 0x06, 3]);
    for xyz in [[0.0f32, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]] {
        b.extend_from_slice(&[0x05, 0x08, 0x01]);
        for v in xyz {
            b.extend_from_slice(&le_f32(v));
        }
    }
    b
}

/// A `SurfacicReps` physical payload carrying one inline cylinder record under
/// the strict 5-byte prefix template.
pub(crate) fn surf_stream() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&[0xAA, 0xBB, 0xCC]); // target u24
    b.push(0x00); // sentinel
    b.push(0x1a); // cylinder/cone prebyte
    b.extend_from_slice(&[0x00, 0x33, 0x33]); // `00 33 KIND` (cylinder)
                                              // BE f32: px py pz ax ay radius
    for v in [0.0f32, 0.0, 0.0, 0.0, 0.0, 5.0] {
        b.extend_from_slice(&be_f32(v));
    }
    b.resize(73, 0);
    b[72] = 0x01; // cylinder face sense
                  // Tag-bridged plane: the plane marker and bounds record share the same
                  // u24le tag. The paired trim packet stores the normal.
    b.extend_from_slice(&[0x11, 0x22, 0x33]);
    b.push(0x00);
    b.push(0x02);
    b.extend_from_slice(&[0x00, 0x33, 0x32]);
    b.resize(122, 0);
    b[121] = 0xff; // plane face sense
    b.extend_from_slice(&[0xff, 0x11, 0x22, 0x33]);
    b.extend_from_slice(&[0x00, 0x02, 0x00, 0x33, 0x32]);
    for v in [1.0f32, 2.0, 3.0, 0.0, 4.0, 0.0, 1.0, 2.0, 3.0, 4.0] {
        b.extend_from_slice(&le_f32(v));
    }
    b.extend_from_slice(&[0x60, 0x44, 0x55, 0x66]);
    b.extend_from_slice(&[0x00, 0x12, 0x00, 0x33, 0x37]);
    for v in [0.0f32, 0.0, 0.0, 5.0] {
        b.extend_from_slice(&be_f32(v));
    }
    b.extend_from_slice(&[0, 1]); // adjacent face ordinals
    b
}

/// One descriptor block: a `0x54`-byte header (logical length at `+0x0c`, the
/// UTF-16LE name at `+0x10`, the extent count at `+0x50`) followed by one 20-byte
/// extent. `phys_off` is measured from the inner magic.
pub(crate) fn descriptor(name: &str, phys_off: u32, phys_len: u32) -> Vec<u8> {
    let mut b = vec![0u8; 0x54];
    b[0x0c..0x10].copy_from_slice(&be32(phys_len)); // logical_length == cum
    let mut np = 0x10;
    for ch in name.chars() {
        b[np] = ch as u8;
        b[np + 1] = 0x00;
        np += 2;
    }
    b[0x50..0x54].copy_from_slice(&be32(1)); // extent count k = 1
    b.extend_from_slice(&be32(phys_off)); // phys_off
    b.extend_from_slice(&be32(phys_len)); // phys_len
    b.extend_from_slice(&be32(phys_len)); // log_len
    b.extend_from_slice(&be32(0)); // log_off
    b.extend_from_slice(&be32(0)); // flags
    b
}

/// Assemble a standard-nested `.CATPart`: a minimal outer header, then a nested
/// `V5_CFV2` whose `CATIA_V5 CB0001` directory catalogues a `MainDataStream` and
/// a `SurfacicReps`, with their physical bytes placed right after the inner
/// header and the directory placed after them.
pub(crate) fn standard_catpart() -> Vec<u8> {
    standard_catpart_from_streams(&main_stream(), &surf_stream())
}

pub(crate) fn standard_catpart_from_streams(main: &[u8], surf: &[u8]) -> Vec<u8> {
    // Physical stream layout, relative to the inner magic:
    //   [0..16]  inner header (magic, A, B)
    //   [16..]   MainDataStream, then SurfacicReps
    //   [A..A+B] directory
    let main_off = 16u32;
    let surf_off = main_off + main.len() as u32;
    let dir_rel = surf_off + surf.len() as u32; // == A

    let mut dir = Vec::new();
    dir.extend_from_slice(DIR_MAGIC);
    dir.extend_from_slice(&descriptor("MainDataStream", main_off, main.len() as u32));
    dir.extend_from_slice(&descriptor("SurfacicReps", surf_off, surf.len() as u32));
    dir.extend_from_slice(b"CB__END");
    let b_len = dir.len() as u32;

    let mut inner = Vec::new();
    inner.extend_from_slice(OUTER_MAGIC);
    inner.extend_from_slice(&be32(dir_rel)); // A
    inner.extend_from_slice(&be32(b_len)); // B
    inner.extend_from_slice(main);
    inner.extend_from_slice(surf);
    inner.extend_from_slice(&dir);

    // Outer header: magic + a big-endian directory offset/length pair whose sum
    // is the file size (the directory here is the inner container's tail).
    let mut f = Vec::new();
    f.extend_from_slice(OUTER_MAGIC);
    let outer_dir_off = 16u32 + inner.len() as u32; // placed at EOF (zero-length)
    f.extend_from_slice(&be32(outer_dir_off));
    f.extend_from_slice(&be32(0));
    f.extend_from_slice(&inner);
    f
}

pub(crate) fn outer_directory_catpart() -> Vec<u8> {
    let payload = b"outer logical stream";
    let mut dir = Vec::new();
    dir.extend_from_slice(DIR_MAGIC);
    dir.extend_from_slice(&descriptor("RootStorage", 16, payload.len() as u32));
    dir.extend_from_slice(b"CB__END");

    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(16 + payload.len() as u32));
    file.extend_from_slice(&be32(dir.len() as u32));
    file.extend_from_slice(payload);
    file.extend_from_slice(&dir);
    file
}

pub(crate) fn outer_container_catpart(stream: &[u8]) -> (Vec<u8>, u64) {
    let mut declaration = vec![0; 40];
    declaration[8..12].copy_from_slice(b"\x01\x00\x03\x00");
    declaration[12..16].copy_from_slice(&2u32.to_le_bytes());
    declaration[16..24].copy_from_slice(b"\x01\x00\x6c\x00\x02\x00\x00\x00");
    declaration[32..36].copy_from_slice(b"\x02\x00\x81\x20");
    declaration.extend_from_slice(b"CATPrtCont\0CATProdCont\0\0");
    declaration.extend_from_slice(b"\x03\x00\xf7\x00\x03\x00\x00\x00");
    declaration.extend_from_slice(&0x4bbc_295cu32.to_be_bytes());
    declaration.extend_from_slice(&0x0000_1048u32.to_be_bytes());
    declaration.extend_from_slice(&0x62eb_7b6fu32.to_be_bytes());
    declaration.extend_from_slice(&0x0000_1825u32.to_be_bytes());

    let data_offset = 16u32;
    let graph_offset = data_offset + declaration.len() as u32;
    let mut dir = Vec::new();
    dir.extend_from_slice(DIR_MAGIC);
    dir.extend_from_slice(&descriptor("Data", data_offset, declaration.len() as u32));
    dir.extend_from_slice(&descriptor(
        "1048_62eb7b6f_1825",
        graph_offset,
        stream.len() as u32,
    ));
    dir.extend_from_slice(b"CB__END");

    let directory_offset = graph_offset + stream.len() as u32;
    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(directory_offset));
    file.extend_from_slice(&be32(dir.len() as u32));
    file.extend(declaration);
    file.extend(stream);
    file.extend(dir);
    (file, u64::from(graph_offset))
}

pub(crate) fn tetrahedron_topology_catpart() -> Vec<u8> {
    let mut main = Vec::new();
    let boundaries: [[u16; 9]; 4] = [
        [30, 10, 20, 31, 11, 21, 32, 12, 22],
        [40, 13, 23, 41, 24, 14, 42, 20, 10],
        [50, 14, 24, 51, 25, 15, 52, 21, 11],
        [60, 15, 25, 61, 23, 13, 62, 22, 12],
    ];
    for (face, boundary) in boundaries.into_iter().enumerate() {
        main.extend_from_slice(&[0x01, 0x44, 0x01, 0xff, 11, 0, 0, 0, 11]);
        main.extend_from_slice(&(500u16 + face as u16).to_be_bytes());
        for handle in boundary {
            main.extend_from_slice(&handle.to_be_bytes());
        }
        main.extend_from_slice(&boundary[0].to_be_bytes());
    }
    for _ in 0..4 {
        main.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    }
    main.extend_from_slice(&[0x01, 0x01, 6]);
    for row in [
        [100u16, 10, 20, 101],
        [101, 11, 21, 102],
        [102, 12, 22, 100],
        [100, 13, 23, 103],
        [101, 14, 24, 103],
        [102, 15, 25, 103],
    ] {
        main.extend_from_slice(&[0x02, 4]);
        for handle in row {
            main.extend_from_slice(&handle.to_be_bytes());
        }
    }
    main.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    main.extend_from_slice(&[0x01, 0x06, 4]);
    let points = [
        [1.0f32, 1.0, 1.0],
        [1.0, -1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
    ];
    for point in points {
        main.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            main.extend_from_slice(&le_f32(value));
        }
    }
    for (edge, faces) in [[0u8, 1u8], [0, 2], [0, 3], [1, 3], [1, 2], [2, 3]]
        .into_iter()
        .enumerate()
    {
        main.push(0x60);
        main.extend_from_slice(&[(edge + 1) as u8, 0, 0]);
        main.extend_from_slice(&[0x00, 0x02, 0x00, 0x33, 0x36, faces[0], faces[1]]);
    }

    let face_vertices = [[0usize, 1, 2], [0, 3, 1], [1, 3, 2], [2, 3, 0]];
    let mut surf = Vec::new();
    for (face, indices) in face_vertices.into_iter().enumerate() {
        let mut center = [0.0f32; 3];
        for index in indices {
            for axis in 0..3 {
                center[axis] += points[index][axis] / 3.0;
            }
        }
        let radius = ((points[indices[0]][0] - center[0]).powi(2)
            + (points[indices[0]][1] - center[1]).powi(2)
            + (points[indices[0]][2] - center[2]).powi(2))
        .sqrt();
        let start = surf.len();
        surf.extend_from_slice(&[(face + 1) as u8, 0, 0, 0, 0x12, 0, 0x33, 0x35]);
        for value in [center[0], center[1], center[2], radius] {
            surf.extend_from_slice(&be_f32(value));
        }
        surf.resize(start + 65, 0);
        surf[start + 64] = 1;
    }
    standard_catpart_from_streams(&main, &surf)
}

pub(crate) fn fbb_only_catpart() -> Vec<u8> {
    let mut file = standard_catpart();
    let delimiter = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
    let positions = file
        .windows(delimiter.len())
        .enumerate()
        .filter_map(|(position, bytes)| (bytes == delimiter).then_some(position))
        .collect::<Vec<_>>();
    assert_eq!(positions.len(), 2);
    for position in positions {
        file[position] = 0x11;
    }
    file
}

/// A zero-entity `.CATPart`: the outer magic, no nested `V5_CFV2`, and a handful
/// of `a9 03` record-family markers in the preamble.
pub(crate) fn zero_entity_catpart() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(OUTER_MAGIC);
    f.extend_from_slice(&be32(0)); // outer dir offset (unused here)
    f.extend_from_slice(&be32(0));
    for _ in 0..5 {
        f.extend_from_slice(&[0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
    f
}

/// A zero-entity cylinder carrier with the native `a9 03 28 8a` frame.  The
/// record length is `0x8a + 12`, so this also exercises framed-stream walking.
pub(crate) fn zero_entity_cylinder_catpart() -> Vec<u8> {
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
    f.extend_from_slice(&[0x05, 0x08, 0x01]);
    for value in [1.0f32, 2.0, 3.0] {
        f.extend_from_slice(&le_f32(value));
    }
    f
}

pub(crate) fn zero_entity_cylinder_parametric_support_catpart() -> Vec<u8> {
    let mut file = zero_entity_cylinder_catpart();
    file.truncate(16 + 4 + 146);

    let mut support = vec![0u8; 0x91 + 12];
    support[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x91]);
    support[12] = 0x10;
    support[13..17].copy_from_slice(&1u32.to_le_bytes());
    support[67..75].copy_from_slice(&0.0f64.to_le_bytes());
    support[75..83].copy_from_slice(&1.0f64.to_le_bytes());
    for offset in [83, 88] {
        support[offset] = 0x10;
        support[offset + 1..offset + 5].copy_from_slice(&4u32.to_le_bytes());
    }
    for (index, [u, v]) in [[0.0f64, 0.0], [0.25, 0.2], [0.75, 0.8], [1.0, 1.0]]
        .into_iter()
        .enumerate()
    {
        let offset = 93 + index * 16;
        support[offset..offset + 8].copy_from_slice(&u.to_le_bytes());
        support[offset + 8..offset + 16].copy_from_slice(&v.to_le_bytes());
    }
    file.extend(support);
    file
}

pub(crate) fn zero_entity_nurbs_catpart() -> Vec<u8> {
    let mut f = vec![0u8; 16];
    f[..8].copy_from_slice(OUTER_MAGIC);
    let record = f.len();
    f.extend_from_slice(&[0xa9, 0x03, 0x34, 0xc8]);
    // The nominal record is 212 bytes, but the fixed 7×7 pole grid extends
    // past it and starts at logical offset +167.
    f.resize(record + 167 + 49 * 24, 0);
    let write_f64 = |f: &mut [u8], at: usize, value: f64| {
        f[record + at..record + at + 8].copy_from_slice(&le_f64(value));
    };
    let write_token = |f: &mut [u8], at: usize, value: u32| {
        f[record + at] = 0x10;
        f[record + at + 1..record + at + 5].copy_from_slice(&value.to_le_bytes());
    };
    for (index, value) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
        write_f64(&mut f, 23 + index * 8, value);
        write_f64(&mut f, 99 + index * 8, value);
    }
    for (index, value) in [4, 1, 1, 1, 4].into_iter().enumerate() {
        write_token(&mut f, 63 + index * 5, value);
        write_token(&mut f, 139 + index * 5, value);
    }
    write_token(&mut f, 88, 1);
    write_token(&mut f, 93, 1);
    f[record + 98] = 0x04;
    f[record + 164..record + 167].copy_from_slice(&[0x08, 0x00, 0x00]);
    for i in 0..49 {
        let at = 167 + i * 24;
        write_f64(&mut f, at, i as f64);
        write_f64(&mut f, at + 8, (i / 7) as f64);
        write_f64(&mut f, at + 16, (i % 7) as f64);
    }
    f
}

pub(crate) fn surface_alias_stream() -> Vec<u8> {
    let mut bytes = 1u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0x01, 0x00, 0x04, 0x00]);
    bytes.extend_from_slice(&0xab12_3456u32.to_le_bytes());
    bytes.extend_from_slice(&[0xff, 2, 3, 7]);
    bytes.extend_from_slice(&0x1122_3344u32.to_le_bytes());
    bytes.extend_from_slice(&0x5566_7788u32.to_le_bytes());
    bytes
}

pub(crate) fn marker_7cd9_stream() -> Vec<u8> {
    vec![0xaa, 0x7c, 0xd9, 1, 2, 3, 0x7c, 0xd9, 4, 5]
}

pub(crate) fn finjpl_stream() -> Vec<u8> {
    let mut bytes = vec![0xaa, 0xbb];
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0000_008eu32.to_be_bytes());
    bytes.extend_from_slice(&[1, 2, 3]);
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0101_0001u32.to_be_bytes());
    bytes.extend_from_slice(&[4, 5]);
    bytes
}

pub(crate) fn object_main_catpart(main: &[u8]) -> Vec<u8> {
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
    inner.extend_from_slice(main);
    inner.extend_from_slice(&surf);
    inner.extend_from_slice(&dir);
    let mut file = Vec::new();
    file.extend_from_slice(OUTER_MAGIC);
    file.extend_from_slice(&be32(16 + inner.len() as u32));
    file.extend_from_slice(&be32(0));
    file.extend_from_slice(&inner);
    file
}
