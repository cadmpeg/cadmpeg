// SPDX-License-Identifier: Apache-2.0
//! Tests over synthetic byte fixtures. No real CAD file exists in this repo and
//! none may be added, so every fixture is a hand-built `.CATPart` byte image
//! whose bytes exercise the real container, variant-detection, and geometry
//! decode paths and fail if the code regresses.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::Annotations;

use crate::variant::Variant;
use crate::CatiaCodec;

fn assert_every_entity_has_v1_annotation(ir: &CadIr, annotations: &Annotations) {
    let mut entity_count = 0;
    macro_rules! check {
        ($entities:expr) => {
            for entity in $entities {
                entity_count += 1;
                let provenance = &annotations.provenance[&entity.id.0];
                assert!(annotations.streams[provenance.stream as usize].starts_with("catia:"));
            }
        };
    }

    check!(&ir.model.bodies);
    check!(&ir.model.regions);
    check!(&ir.model.shells);
    check!(&ir.model.faces);
    check!(&ir.model.loops);
    check!(&ir.model.coedges);
    check!(&ir.model.edges);
    check!(&ir.model.vertices);
    check!(&ir.model.points);
    check!(&ir.model.surfaces);
    check!(&ir.model.curves);
    let unknowns = ir.native_unknowns("catia").unwrap();
    check!(&unknowns);
    assert_eq!(annotations.provenance.len(), entity_count);
}

fn standard_quad_topology_stream() -> Vec<u8> {
    let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 10, 0, 0, 0, 10];
    for handle in [1u16, 10, 11, 12, 13, 14, 15, 16, 17, 10] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }

    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    bytes.extend_from_slice(&[0x01, 0x01, 0x04]);
    for row in [
        [100u16, 11, 101],
        [101, 13, 102],
        [102, 15, 103],
        [103, 17, 100],
    ] {
        bytes.extend_from_slice(&[0x02, 0x03]);
        for handle in row {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x01, 0x06, 0x04]);
    for xyz in [
        [0.0f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}

#[test]
fn standard_topology_recovers_a_quad_boundary_and_port_vertices() {
    let topology = crate::topology::parse_standard(&standard_quad_topology_stream())
        .expect("valid standard topology");

    assert_eq!(topology.face_count(), 1);
    assert_eq!(topology.edge_rows().len(), 4);
    assert_eq!(topology.vertex_points().len(), 4);
    let boundary = &topology.faces()[0].boundaries[0];
    assert_eq!(boundary.coedges.len(), 4);
    assert_eq!(
        boundary
            .coedges
            .iter()
            .map(|use_| use_.edge_row)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert!(boundary.coedges.iter().all(|use_| !use_.reversed));
    assert_eq!(topology.logical_vertex_count(), 4);
}

#[test]
fn standard_topology_accepts_delimiters_between_counted_edge_tables() {
    let mut bytes = standard_quad_topology_stream();
    let header = bytes
        .windows(3)
        .position(|window| window == [0x01, 0x01, 0x04])
        .expect("edge table header");
    bytes[header + 2] = 2;
    let second_table = header + 3 + 2 * 8;
    bytes.splice(
        second_table..second_table,
        [
            0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02,
        ],
    );

    let topology = crate::topology::parse_standard(&bytes).expect("two edge tables");
    assert_eq!(
        topology
            .edge_rows()
            .iter()
            .map(|row| row.kind)
            .collect::<Vec<_>>(),
        vec![1, 1, 2, 2]
    );
}

#[test]
fn fbb_topology_reads_u24_mesh_and_edge_handles() {
    let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 10, 0, 0, 0, 10];
    for handle in [
        1u32, 0x01_0010, 0x01_0011, 0x01_0012, 0x01_0013, 0x01_0014, 0x01_0015, 0x01_0016,
        0x01_0017, 0x01_0010,
    ] {
        bytes.extend_from_slice(&handle.to_be_bytes()[1..]);
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    for (kind, rows) in [
        (
            1,
            [
                [0x02_0000u32, 0x01_0011, 0x02_0001],
                [0x02_0001, 0x01_0013, 0x02_0002],
            ],
        ),
        (
            2,
            [
                [0x02_0002u32, 0x01_0015, 0x02_0003],
                [0x02_0003, 0x01_0017, 0x02_0000],
            ],
        ),
    ] {
        bytes.extend_from_slice(&[0x01, kind, 2]);
        for row in rows {
            bytes.extend_from_slice(&[0x02, 3]);
            for handle in row {
                bytes.extend_from_slice(&handle.to_be_bytes()[1..]);
            }
        }
        bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0]);

    let topology = crate::topology::parse_fbb(&bytes).expect("valid FBB topology");
    assert_eq!(
        topology.edge_rows()[0].handles,
        vec![0x02_0000, 0x01_0011, 0x02_0001]
    );
    assert_eq!(topology.faces()[0].boundaries[0].coedges.len(), 4);
    assert_eq!(topology.logical_vertex_count(), 4);
    assert!(topology.vertex_points().is_empty());
}

#[test]
fn standard_topology_matches_edge_interiors_and_collapses_endpoint_ports() {
    let mut bytes = vec![0x01, 0x44, 0x01, 0xff, 11, 0, 0, 0, 11];
    for handle in [1u16, 10, 11, 12, 13, 14, 15, 16, 17, 18, 10] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    bytes.extend_from_slice(&[0x01, 0x01, 3]);
    for row in [
        [101u16, 12, 11, 100],
        [101, 14, 15, 102],
        [102, 17, 18, 100],
    ] {
        bytes.extend_from_slice(&[0x02, 4]);
        for handle in row {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x01, 0x06, 3]);
    for index in 0..3 {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [index as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let topology = crate::topology::parse_standard(&bytes).expect("interior-run topology");
    let coedges = &topology.faces()[0].boundaries[0].coedges;
    assert_eq!(
        coedges.iter().map(|use_| use_.edge_row).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(coedges[0].reversed);
    assert_eq!(topology.logical_vertex_count(), 3);
}

#[test]
fn standard_legacy_two_strip_packet_recovers_two_face_boundaries() {
    let mut bytes = vec![0x01, 0x42, 0x02, 0xff, 12, 0, 0, 0, 6, 6];
    for handle in [10u16, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 25] {
        bytes.extend_from_slice(&handle.to_be_bytes());
    }
    bytes.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    bytes.extend_from_slice(&[0x01, 0x01, 6]);
    for row in [
        [100u16, 11, 101],
        [101, 15, 102],
        [102, 12, 100],
        [200, 21, 201],
        [201, 25, 202],
        [202, 22, 200],
    ] {
        bytes.extend_from_slice(&[0x02, 3]);
        for handle in row {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    bytes.extend_from_slice(&[0x01, 0x06, 6]);
    for index in 0..6 {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [index as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let topology = crate::topology::parse_standard(&bytes).expect("legacy B=2 packet");
    assert_eq!(topology.faces()[0].boundaries.len(), 2);
    assert!(topology.faces()[0]
        .boundaries
        .iter()
        .all(|boundary| boundary.coedges.len() == 3));
    assert_eq!(topology.logical_vertex_count(), 6);
}

#[test]
fn standard_curve_support_table_recovers_leading_spline_and_widened_faces() {
    let mut bytes = vec![0x60, 1, 2, 3, 0, 0, 0, 0xff];
    bytes.extend_from_slice(&260u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0x60, 4, 5, 6, 0, 2, 0, 0x33, 0x36, 0xff]);
    bytes.extend_from_slice(&260u32.to_le_bytes());
    bytes.push(2);

    let rows = crate::geometry::standard_curve_supports(&bytes, 300);
    assert_eq!(rows.len(), 2);
    assert!(matches!(
        rows[0].geometry,
        crate::geometry::StandardCurveGeometry::Bspline
    ));
    assert!(matches!(
        rows[1].geometry,
        crate::geometry::StandardCurveGeometry::Line
    ));
    assert_eq!(rows[0].faces, [260, 1]);
    assert_eq!(rows[1].faces, [260, 2]);
}

#[test]
fn topology_binds_logical_vertices_from_exact_edge_endpoint_pairs() {
    let topology =
        crate::topology::parse_standard(&standard_quad_topology_stream()).expect("quad topology");
    let assignment = topology
        .bind_vertex_points(&[[0, 1], [1, 2], [2, 3], [3, 0]])
        .expect("unique point assignment");

    assert_eq!(assignment, vec![0, 1, 2, 3]);
}

const OUTER_MAGIC: &[u8; 8] = b"V5_CFV2\0";
const DIR_MAGIC: &[u8; 16] = b"CATIA_V5 CB0001\0";

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}
fn le_f32(v: f32) -> [u8; 4] {
    v.to_le_bytes()
}
fn be_f32(v: f32) -> [u8; 4] {
    v.to_be_bytes()
}
fn le_f64(v: f64) -> [u8; 8] {
    v.to_le_bytes()
}

/// A `MainDataStream` physical payload: two FBB spine rows, the standard
/// edge-table delimiter, and three `05 08 01` vertex records.
fn main_stream() -> Vec<u8> {
    let mut b = Vec::new();
    // Two stride-8 FBB rows (`30 04 04 ff` + 4 constant bytes).
    for _ in 0..2 {
        b.extend_from_slice(&[0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    }
    // Standard edge-table delimiter.
    b.extend_from_slice(&[0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00]);
    // Three vertex records (3×f32 LE, millimetres).
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
fn surf_stream() -> Vec<u8> {
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
                  // Tag-bridged plane: the plane marker and parameter record share the same
                  // u24le tag.  The normal is perpendicular to the stored yz diagonal.
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
fn descriptor(name: &str, phys_off: u32, phys_len: u32) -> Vec<u8> {
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
fn standard_catpart() -> Vec<u8> {
    standard_catpart_from_streams(&main_stream(), &surf_stream())
}

fn standard_catpart_from_streams(main: &[u8], surf: &[u8]) -> Vec<u8> {
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

fn tetrahedron_topology_catpart() -> Vec<u8> {
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

fn fbb_only_catpart() -> Vec<u8> {
    let mut file = standard_catpart();
    let delimiter = [0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00];
    let pos = file
        .windows(delimiter.len())
        .position(|bytes| bytes == delimiter)
        .expect("standard fixture delimiter");
    file[pos] = 0x11;
    file
}

/// A zero-entity `.CATPart`: the outer magic, no nested `V5_CFV2`, and a handful
/// of `a9 03` record-family markers in the preamble.
fn zero_entity_catpart() -> Vec<u8> {
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
fn zero_entity_cylinder_catpart() -> Vec<u8> {
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

fn zero_entity_nurbs_catpart() -> Vec<u8> {
    let mut f = vec![0u8; 16];
    f[..8].copy_from_slice(OUTER_MAGIC);
    let record = f.len();
    f.extend_from_slice(&[0xa9, 0x03, 0x34, 0xc8]);
    // The nominal record is 212 bytes, but the inline pole grid extends past it.
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
    let mut edge = vec![0u8; 19];
    edge[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
    edge[3] = 0xff;
    edge[5..7].copy_from_slice(&6u16.to_le_bytes());
    edge[13..19].copy_from_slice(&[0x85, 0x80, 0x81, 0x82, 0x80, 0x80]);
    record.extend_from_slice(&edge);
    for xyz in [[12.5f32, 20.0, 30.0], [7.5, 20.0, 30.0]] {
        record.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            record.extend_from_slice(&le_f32(value));
        }
    }
    record
}

fn e5_torus_stream() -> Vec<u8> {
    let mut record = vec![0u8; 143];
    record[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
    record[3] = 0xcc;
    record[5..7].copy_from_slice(&130u16.to_le_bytes());
    let write = |record: &mut [u8], at: usize, value: f64| {
        record[at..at + 8].copy_from_slice(&le_f64(value));
    };
    for (at, value) in [
        (14, 1.0),
        (22, 2.0),
        (30, 3.0),
        (38, 1.0),
        (102, 1.0),
        (110, 12.0),
        (118, 2.0),
    ] {
        write(&mut record, at, value);
    }
    record
}

fn e5_plane_stream() -> Vec<u8> {
    let mut payload = vec![0u8; 90];
    for (index, value) in [1.0f64, 2.0, 3.0].into_iter().enumerate() {
        payload[1 + 8 * index..9 + 8 * index].copy_from_slice(&le_f64(value));
    }
    payload[25] = 0x33;
    for index in 0..4 {
        payload[26 + 8 * index..34 + 8 * index].copy_from_slice(&le_f64(1.0));
    }
    for (index, value) in [-4.0f64, 7.0, -2.0, 9.0].into_iter().enumerate() {
        payload[58 + 8 * index..66 + 8 * index].copy_from_slice(&le_f64(value));
    }
    let mut bytes = Vec::new();
    append_e5_record(&mut bytes, 0xc8, 42, &payload);
    bytes
}

fn a8_surface_stream() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(0); // lead
    payload.extend_from_slice(&[9, 0, 0, 9, 1]); // degree, flags, K, marker
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.extend_from_slice(&[13, 13]); // multiplicities [3, 3]
    payload.extend_from_slice(&[9, 0, 0, 9, 1]);
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.extend_from_slice(&[13, 13, 1]); // multiplicities and plain mode
    for i in 0..9 {
        for value in [i as f64, (i / 3) as f64, (i % 3) as f64] {
            payload.extend_from_slice(&le_f64(value));
        }
    }
    let mut record = Vec::new();
    record.extend_from_slice(&[0xa8, 0x03, 0x34]);
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&0xdeca_fbad_u32.to_le_bytes());
    record.extend_from_slice(&payload);
    record
}

fn a8_rational_surface_stream() -> Vec<u8> {
    let mut record = a8_surface_stream();
    // Header is 11 bytes; the common-form mode follows the two degree/knot
    // sections at record offset 58 for this 2×2 distinct-knot fixture.
    record[58] = 0x05;
    for _ in 0..9 {
        record.extend_from_slice(&le_f64(2.0));
    }
    let payload_len = (record.len() - 11) as u32;
    record[3..7].copy_from_slice(&payload_len.to_le_bytes());
    record
}

fn a8_pcurve_stream() -> Vec<u8> {
    let mut payload = vec![0, 0x18, 0x34, 0x12, 21, 0, 0, 9, 0x0c];
    for value in [0.0f64, 1.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload.extend_from_slice(&[25, 25, 9, 1]);
    for values in [[0.0f64, 1.0], [0.0, 1.0], [1.0, 1.0], [0.0, 0.0]] {
        for value in values {
            payload.extend_from_slice(&le_f64(value));
        }
    }
    payload.push(0x05);
    for _ in 0..4 {
        payload.extend_from_slice(&le_f64(0.0));
    }
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.push(0x07);
    let mut record = vec![0xa8, 0x03, 0x20];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&0x5678u32.to_le_bytes());
    record.extend_from_slice(&payload);
    record
}

fn a5_pcurve_stream() -> Vec<u8> {
    a5_pcurve_stream_with_uv([0.0, 1.0], [0.0, 1.0])
}

fn a6_pcurve_stream() -> Vec<u8> {
    let narrow = a5_pcurve_stream();
    let mut wide = vec![0xa6, 0x03, 0x20];
    wide.extend_from_slice(&narrow[3..7]);
    wide.extend_from_slice(&[0x05, 0x00]);
    wide.extend_from_slice(&narrow[8..]);
    wide
}

fn b2_pcurve_stream() -> Vec<u8> {
    let narrow = a5_pcurve_stream();
    let payload = &narrow[8..];
    let mut record = vec![0xb2, 0x03, 0x20, u8::try_from(payload.len()).unwrap(), 0x05];
    record.extend_from_slice(payload);
    record
}

fn b2_parameter_point_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    for values in [
        vec![2.0f64, 3.0],
        vec![11.0, 4.0, 5.0],
        vec![1.0, 2.0, 3.0, 4.0, 5.0],
    ] {
        let length = 2 + 8 * values.len();
        bytes.extend_from_slice(&[0xb2, 0x03, 0x18, u8::try_from(length).unwrap(), 0x05, 0x05]);
        bytes.push(0x12);
        for value in values {
            bytes.extend_from_slice(&le_f64(value));
        }
    }
    bytes
}

fn b2_reference_list_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x37, 0x22, 0x05];
    for value in 0u8..26 {
        record.push(4 * value + 1);
    }
    record.extend_from_slice(&le_f64(1.0));
    record
}

fn b2_cone_face_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x3b, 0x20, 0x05];
    for value in 0u8..16 {
        record.push(4 * value + 1);
    }
    record.extend_from_slice(&le_f64(1.5));
    record.extend_from_slice(&le_f64(std::f64::consts::FRAC_PI_4));
    record
}

fn b2_topology_metadata_stream() -> Vec<u8> {
    let mut bytes = vec![
        0xb2, 0x03, 0x5e, 0x07, 0x05, 0x0a, 0x34, 0x12, 0x0a, 0x78, 0x56, 0,
    ];
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 1, 2, 3, 0x88]);
    bytes
}

fn b2_revolution_stream() -> Vec<u8> {
    let scale = 2.0;
    let angular_lo = scale * 0.5;
    let angular_hi = angular_lo + scale * std::f64::consts::TAU;
    let mean = scale * (std::f64::consts::PI + 0.5);
    let mut record = vec![0xb2, 0x03, 0x2d, 0xae, 0x05];
    let mut payload = vec![0u8; 0xae];
    payload[0] = 0x0a;
    payload[1..3].copy_from_slice(&0x1234u16.to_le_bytes());
    let frame = [
        1.0f64, 2.0, 3.0, // origin
        1.0, 0.0, 0.0, // first basis
        0.0, 1.0, 0.0, // second basis
        0.0, 0.0, 1.0, // axis
    ];
    for (index, value) in frame.into_iter().enumerate() {
        payload[3 + 8 * index..11 + 8 * index].copy_from_slice(&le_f64(value));
    }
    for (index, value) in [angular_lo, angular_hi, -4.0, 9.0].into_iter().enumerate() {
        payload[99 + 8 * index..107 + 8 * index].copy_from_slice(&le_f64(value));
    }
    payload[131..133].copy_from_slice(&[0x05, 0x05]);
    payload[133..141].copy_from_slice(&le_f64(scale));
    payload[141..149].copy_from_slice(&le_f64(1.0));
    payload[149..157].copy_from_slice(&le_f64(1.0));
    payload[157..165].copy_from_slice(&le_f64(0.0));
    payload[165] = 0x01;
    payload[166..174].copy_from_slice(&le_f64(mean));
    record.extend_from_slice(&payload);
    record
}

fn b2_group_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x65, 0x04, 0x05, 0x81, 0x03, 0x05, 0x0d, 0xb2, 0x03, 0x60, 0x02, 0x05, 0x81,
        0x0d,
    ]
}

fn a5_pcurve_stream_with_uv(u: [f64; 2], v: [f64; 2]) -> Vec<u8> {
    let mut payload = vec![0x08, 0x34, 0x12, 21, 9, 0x08, 9];
    for value in [0.0f64, 1.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload.extend_from_slice(&[9, 2]);
    for values in [u, v, [1.0, 1.0], [0.0, 0.0]] {
        for value in values {
            payload.extend_from_slice(&le_f64(value));
        }
    }
    payload.push(0x05);
    for _ in 0..4 {
        payload.extend_from_slice(&le_f64(0.0));
    }
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    let mut record = vec![0xa5, 0x03, 0x20];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

fn a5_circle_bound_edge_stream() -> Vec<u8> {
    let radius = 3.0;
    let arc = [0.0, 2.0 * std::f64::consts::PI * radius];
    let mut bytes = a5_pcurve_stream_with_uv(arc, [2.0, 2.0]);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(arc, [2.0, 2.0]));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&b2_circle_stream());
    bytes
}

fn a5_cone_bound_edge_stream() -> Vec<u8> {
    let u = [0.0f64, 1.0];
    let v = [2.0f64, 3.0];
    let mut bytes = a5_pcurve_stream_with_uv(u, v);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(u, v));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&b2_cone_stream());
    for (u, v) in u.into_iter().zip(v) {
        let phi = u / 3.0;
        let point = [
            1.0 + v * 0.25f64.sin() * phi.cos(),
            2.0 + v * 0.25f64.sin() * phi.sin(),
            3.0 + v * 0.25f64.cos(),
        ];
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&(value as f32).to_le_bytes());
        }
    }
    bytes
}

fn b2_offset_support_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x31, 0x2b, 0x05, 0x08, 0x34, 0x12];
    for value in [2.5f64, 0.0, -1.0, 4.0, 3.0] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

fn b3_offset_support_stream() -> Vec<u8> {
    let narrow = b2_offset_support_stream();
    let mut wide = vec![0xb3, 0x03, 0x31, narrow[3], 0x05, 0x00];
    wide.extend_from_slice(&narrow[5..]);
    wide
}

fn b2_edge_parameter_stream() -> Vec<u8> {
    b2_edge_parameter_stream_for(2.0, 7.0)
}

fn b2_edge_parameter_stream_for(lo: f64, hi: f64) -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x23, 0x4e, 0];
    record.extend_from_slice(&[0; 6]);
    for value in [lo, hi, 1e-6, lo, hi, 1.0, lo, hi, 1e-6] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

fn a5_edge_block_stream() -> Vec<u8> {
    let mut bytes = a5_pcurve_stream();
    bytes.extend_from_slice(&a5_pcurve_stream());
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes
}

fn a5_cylinder_bound_edge_stream() -> Vec<u8> {
    let mut bytes = a5_edge_block_stream();
    bytes.extend_from_slice(&b2_cylinder_stream());
    let endpoints = [
        [1.0f32, 4.0, 3.0],
        [2.0, (2.0 + 2.0 * 0.5f32.cos()), (3.0 + 2.0 * 0.5f32.sin())],
    ];
    for point in endpoints {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

fn a5_nurbs_bound_edge_stream(offset: f64) -> Vec<u8> {
    let cylinder_uv = ([0.0f64, 1.0], [0.0f64, 1.0]);
    let surface_uv = ([0.0f64, 1.0], [0.0f64, 0.0]);
    let p0 = [1.0, 4.0, 3.0];
    let p1 = [2.0, 2.0 + 2.0 * 0.5f64.cos(), 3.0 + 2.0 * 0.5f64.sin()];
    let normal = {
        let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let v = [0.0f64, 0.0, 1.0];
        let cross = [u[1] * v[2] - u[2] * v[1], -u[0] * v[2], 0.0];
        let length = cross[0].hypot(cross[1]);
        [cross[0] / length, cross[1] / length, 0.0]
    };
    let shifted = |point: [f64; 3]| {
        [
            point[0] - offset * normal[0],
            point[1] - offset * normal[1],
            point[2],
        ]
    };
    let s0 = shifted(p0);
    let s1 = shifted(p1);
    let mut bytes = a5_pcurve_stream_with_uv(cylinder_uv.0, cylinder_uv.1);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(surface_uv.0, surface_uv.1));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&b2_cylinder_stream());
    bytes.extend_from_slice(&a5_surface_stream_with_poles([
        s0,
        [s0[0], s0[1], s0[2] + 1.0],
        s1,
        [s1[0], s1[1], s1[2] + 1.0],
    ]));
    for point in [p0, p1] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&(value as f32).to_le_bytes());
        }
    }
    bytes
}

fn b2_circle_stream() -> Vec<u8> {
    let radius = 3.0;
    let mut record = vec![0xb2, 0x03, 0x19, 0x33, 0x05, 0x08, 0x34, 0x12];
    for value in [
        4.0f64,
        -2.0,
        radius,
        0.0,
        2.0 * std::f64::consts::PI * radius,
    ] {
        record.extend_from_slice(&le_f64(value));
    }
    record.extend_from_slice(&[0; 8]);
    record
}

fn b2_cylinder_stream() -> Vec<u8> {
    let radius = 2.0;
    let mut record = vec![0xb2, 0x03, 0x28, 0x5a, 0x05];
    record.resize(95, 0);
    let p = 5;
    for (index, value) in [1.0f64, 2.0, 3.0].into_iter().enumerate() {
        record[p + 8 * index..p + 8 * index + 8].copy_from_slice(&le_f64(value));
    }
    record[p + 24] = 0x19;
    record[p + 25..p + 33].copy_from_slice(&le_f64(1.0));
    record[p + 33..p + 41].copy_from_slice(&le_f64(0.0));
    record[p + 41..p + 49].copy_from_slice(&le_f64(1.0));
    record[p + 49..p + 57].copy_from_slice(&le_f64(radius));
    record[p + 57..p + 65].copy_from_slice(&le_f64(0.0));
    record[p + 65..p + 73].copy_from_slice(&le_f64(2.0 * std::f64::consts::PI * radius));
    record[p + 73..p + 81].copy_from_slice(&le_f64(-4.0));
    record[p + 81..p + 89].copy_from_slice(&le_f64(5.0));
    record[p + 89] = 0x07;
    record
}

fn b3_cylinder_stream() -> Vec<u8> {
    let narrow = b2_cylinder_stream();
    let mut wide = vec![0xb3, 0x03, 0x28, 0x5a, 0x05, 0x00];
    wide.extend_from_slice(&narrow[5..]);
    wide
}

fn b2_implicit_axis_cylinder_stream() -> Vec<u8> {
    let radius = 2.0;
    let mut record = vec![0xb2, 0x03, 0x28, 0x52, 0x05];
    record.resize(87, 0);
    let p = 5;
    record[p + 24] = 0x1d;
    record[p + 25..p + 33].copy_from_slice(&le_f64(1.0));
    record[p + 33..p + 41].copy_from_slice(&le_f64(1.0));
    record[p + 41..p + 49].copy_from_slice(&le_f64(radius));
    record[p + 49..p + 57].copy_from_slice(&le_f64(0.0));
    record[p + 57..p + 65].copy_from_slice(&le_f64(2.0 * std::f64::consts::PI * radius));
    record[p + 65..p + 73].copy_from_slice(&le_f64(-1.0));
    record[p + 73..p + 81].copy_from_slice(&le_f64(3.0));
    record[p + 81] = 0x07;
    record
}

fn b2_phase_tailed_cylinder_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x28, 0x62, 0x05];
    record.resize(103, 0);
    let p = 5;
    record[p + 24] = 0x0e;
    record[p + 25..p + 33].copy_from_slice(&le_f64(0.0));
    record[p + 33..p + 41].copy_from_slice(&le_f64(1.0));
    record[p + 49..p + 57].copy_from_slice(&le_f64(4.0));
    record[p + 57..p + 65].copy_from_slice(&le_f64(0.0));
    record[p + 65..p + 73].copy_from_slice(&le_f64(8.0));
    record[p + 73..p + 81].copy_from_slice(&le_f64(-2.0));
    record[p + 81..p + 89].copy_from_slice(&le_f64(2.0));
    record[p + 89] = 0x03;
    record[p + 90..p + 98].copy_from_slice(&le_f64(0.75));
    record
}

fn b2_cone_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x29, 0xb8, 0x05];
    record.resize(189, 0);
    for (start, values) in [
        (5, [1.0f64, 2.0, 3.0]),
        (29, [1.0, 0.0, 0.0]),
        (53, [0.0, 1.0, 0.0]),
        (77, [0.0, 0.0, 1.0]),
    ] {
        for (index, value) in values.into_iter().enumerate() {
            record[start + 8 * index..start + 8 * index + 8].copy_from_slice(&le_f64(value));
        }
    }
    record[101..109].copy_from_slice(&le_f64(0.25));
    record[125..133].copy_from_slice(&le_f64(0.5));
    record[133..141].copy_from_slice(&le_f64(2.0));
    record[141..149].copy_from_slice(&le_f64(8.0));
    record[149..157].copy_from_slice(&le_f64(3.0));
    record
}

fn b2_construction_use_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x30, 0x2d, 0x05, 0x05, 0x08, 0x34, 0x12];
    record.extend_from_slice(&le_f64(-2.0));
    record.push(0x01);
    for value in [0.0f64, 4.0, -1.0, 3.0] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

fn b2_embedded_cylinder_stream() -> Vec<u8> {
    let standalone = b2_cylinder_stream();
    let mut record = vec![
        0xb2, 0x03, 0x60, 0x02, 0x05, 0x81, 0x0d, 0xb4, 0x03, 0x28, 0x5a,
    ];
    record.extend_from_slice(&[0x08, 0x78, 0x56]);
    record.extend_from_slice(&standalone[5..]);
    record
}

fn object_graph_record(head: &[u8], payload: &[u8]) -> Vec<u8> {
    let child_len = 6 + payload.len();
    let total_len = 6 + head.len() + child_len;
    let mut bytes = vec![0x7c, 0x09];
    bytes.extend_from_slice(&(total_len as u32).to_le_bytes());
    bytes.extend_from_slice(head);
    bytes.extend_from_slice(&[0x7c, 0x0a]);
    bytes.extend_from_slice(&(child_len as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn object_graph_from_records(records: &[Vec<u8>]) -> Vec<u8> {
    let total_len = 6 + records.iter().map(Vec::len).sum::<usize>();
    let mut bytes = vec![0x7c, 0x08];
    bytes.extend_from_slice(&(total_len as u32).to_le_bytes());
    for record in records {
        bytes.extend_from_slice(record);
    }
    bytes
}

fn object_graph_stream() -> Vec<u8> {
    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x82, 0x83, 0x84],
            &[0x81, 0x85, 0x3a, 0x87, 0xfe],
        ),
        object_graph_record(
            &[0x14, 0x01, 0x82, 0x84],
            &[0xe5, 0x02, 0, 0, 0, 0xaa, 0xbb, 0xfe],
        ),
    ];
    object_graph_from_records(&records)
}

fn object_graph_vm_stream() -> Vec<u8> {
    object_graph_from_records(&[
        object_graph_record(
            &[0x1c, 0x01, 0x82, 0x80, 0xff, 0xff, 0xff, 0xff, 0x83],
            &[
                0x3b, 0x83, 0x81, 0x85, 0x80, 0x86, 0xd1, 0x09, 0x3c, 0x82, 1, 0, 0, 0, 0x0d, 0xfe,
            ],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x83], &[0xfe]),
    ])
}

fn catalog_stream(entries: &[&str]) -> Vec<u8> {
    let mut bytes = vec![0x7c, 0x02, 0, 0, 0, 0];
    bytes.push(0x80 + u8::try_from(entries.len() + 1).unwrap());
    for entry in entries {
        bytes.push(u8::try_from(entry.len() + 1).unwrap());
        bytes.extend_from_slice(entry.as_bytes());
    }
    let total_len = u32::try_from(bytes.len()).unwrap();
    bytes[2..6].copy_from_slice(&total_len.to_le_bytes());
    bytes
}

fn value_block_stream(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x7c, 0x0b, 0, 0, 0, 0];
    bytes.extend_from_slice(payload);
    let declared_len = u32::try_from(bytes.len()).expect("generated 7C0B length");
    bytes[2..6].copy_from_slice(&declared_len.to_le_bytes());
    bytes.push(0xfe);
    bytes
}

fn standard_catpart_with_object_graph() -> Vec<u8> {
    let graph = object_graph_stream();
    let mut file = standard_catpart();
    file.splice(16..16, graph);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_catalog() -> Vec<u8> {
    let catalog = catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
        "Pad",
    ]);
    let mut file = standard_catpart();
    file.splice(16..16, catalog);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_value_block() -> Vec<u8> {
    let mut stream = value_block_stream(&[0x81, 0x83, 0x32, 0xea, 0, 0, 0, 0x83, 0x82]);
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn surface_alias_stream() -> Vec<u8> {
    let mut bytes = 1u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0x01, 0x00, 0x04, 0x00]);
    bytes.extend_from_slice(&0xab12_3456u32.to_le_bytes());
    bytes.extend_from_slice(&[0xff, 2, 3, 7]);
    bytes.extend_from_slice(&0x1122_3344u32.to_le_bytes());
    bytes.extend_from_slice(&0x5566_7788u32.to_le_bytes());
    bytes
}

fn marker_7cd9_stream() -> Vec<u8> {
    vec![0xaa, 0x7c, 0xd9, 1, 2, 3, 0x7c, 0xd9, 4, 5]
}

fn finjpl_stream() -> Vec<u8> {
    let mut bytes = vec![0xaa, 0xbb];
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0000_008eu32.to_be_bytes());
    bytes.extend_from_slice(&[1, 2, 3]);
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0101_0001u32.to_be_bytes());
    bytes.extend_from_slice(&[4, 5]);
    bytes
}

fn a5_surface_stream() -> Vec<u8> {
    a5_surface_stream_with_poles([
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [2.0, 1.0, 0.0],
        [3.0, 1.0, 1.0],
    ])
}

fn a6_surface_stream() -> Vec<u8> {
    let narrow = a5_surface_stream();
    let mut wide = vec![0xa6, 0x03, 0x34];
    wide.extend_from_slice(&narrow[3..7]);
    wide.extend_from_slice(&[0x05, 0x00]);
    wide.extend_from_slice(&narrow[8..]);
    wide
}

fn a5_surface_stream_with_poles(poles: [[f64; 3]; 4]) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&[0xa5, 0x03, 0x34]);
    record.extend_from_slice(&0u32.to_le_bytes());
    record.push(0); // unclassified byte before the compact header
    record.extend_from_slice(&[5, 9, 0x0c]); // degree 1, two U knots
    record.extend_from_slice(&le_f64(0.0));
    record.extend_from_slice(&le_f64(1.0));
    record.extend_from_slice(&[5, 9, 0x0c]); // degree 1, two V knots
    record.extend_from_slice(&le_f64(0.0));
    record.extend_from_slice(&le_f64(1.0));
    record.push(0x01); // non-rational
    for pole in poles {
        for value in pole {
            record.extend_from_slice(&le_f64(value));
        }
    }
    record.extend_from_slice(&[0x05, 0x01, 0x05, 0x01]);
    record.extend(std::iter::repeat_n(0u8, 64));
    let payload_len = u32::try_from(record.len() - 8).unwrap();
    record[3..7].copy_from_slice(&payload_len.to_le_bytes());
    record
}

fn a5_rational_surface_stream() -> Vec<u8> {
    let mut record = a5_surface_stream();
    record[46] = 0x05;
    let tail = record.split_off(143);
    record.extend_from_slice(&[0x01, 0x07, 0x00]);
    record.extend_from_slice(&le_f64(2.0)); // mirrored seed row -> [2, 2]
    record.push(0x02); // copy the row for the second u row
    record.extend_from_slice(&tail);
    let payload_len = u32::try_from(record.len() - 8).unwrap();
    record[3..7].copy_from_slice(&payload_len.to_le_bytes());
    record
}

fn a5_freeform_curve_stream() -> Vec<u8> {
    let mut payload = vec![9, 21, 9, 0x0c];
    for value in [0.0f64, 1.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    let sites = [
        [
            1.0f64,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ],
        [
            2.0,
            0.0,
            0.0,
            0.0,
            2.0,
            0.0,
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ],
    ];
    for block in 0..3 {
        for site in sites {
            for value in if block == 0 { site } else { [0.0; 10] } {
                payload.extend_from_slice(&le_f64(value));
            }
        }
    }
    let mut record = vec![0xa5, 0x03, 0x32];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

fn a6_freeform_curve_stream() -> Vec<u8> {
    let narrow = a5_freeform_curve_stream();
    let mut wide = vec![0xa6, 0x03, 0x32];
    wide.extend_from_slice(&narrow[3..7]);
    wide.extend_from_slice(&[0x05, 0x00]);
    wide.extend_from_slice(&narrow[8..]);
    wide
}

fn a5_guide_curve_stream() -> Vec<u8> {
    let mut payload = vec![9, 21, 9, 0x0c];
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    let positions = [
        [0.0f64, 0.0, 0.0, 1.0, 0.0, 0.0],
        [2.0, 3.0, 4.0, 2.0, 4.0, 4.0],
    ];
    for block in 0..3 {
        for site in positions {
            for value in if block == 0 { site } else { [0.0; 6] } {
                payload.extend_from_slice(&le_f64(value));
            }
        }
    }
    payload.extend_from_slice(&[0; 48]);
    let mut record = vec![0xa5, 0x03, 0x39];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

fn a8_freeform_curve_stream() -> Vec<u8> {
    let mut payload = vec![0, 9, 21, 0, 0, 9, 0x0c];
    for value in [0.0f64, 1.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload.extend_from_slice(&[25, 25]);
    let sites = [
        [
            1.0f64,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ],
        [
            2.0,
            0.0,
            0.0,
            0.0,
            2.0,
            0.0,
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ],
    ];
    for block in 0..3 {
        for site in sites {
            for value in if block == 0 { site } else { [0.0; 10] } {
                payload.extend_from_slice(&le_f64(value));
            }
        }
    }
    let mut record = vec![0xa8, 0x03, 0x32];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&0x1234_5678u32.to_le_bytes());
    record.extend_from_slice(&payload);
    record
}

fn e5_catpart() -> Vec<u8> {
    let mut main = e5_circle_stream();
    for id in 2..=10 {
        append_e5_record(&mut main, 0xfe, id, &[]);
    }
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

fn a8_catpart() -> Vec<u8> {
    object_main_catpart(&a8_surface_stream())
}

fn object_main_catpart(main: &[u8]) -> Vec<u8> {
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

fn inner_no_directory_a8_catpart() -> Vec<u8> {
    let mut file = a8_catpart();
    let name = b"M\x00a\x00i\x00n\x00D\x00a\x00t\x00a\x00S\x00t\x00r\x00e\x00a\x00m\x00";
    let pos = file
        .windows(name.len())
        .position(|bytes| bytes == name)
        .expect("main stream name");
    file[pos] = b'X';
    file
}

fn inner_no_directory_b2_catpart() -> Vec<u8> {
    let mut file = object_main_catpart(&b2_cylinder_stream());
    let name = b"M\x00a\x00i\x00n\x00D\x00a\x00t\x00a\x00S\x00t\x00r\x00e\x00a\x00m\x00";
    let pos = file
        .windows(name.len())
        .position(|bytes| bytes == name)
        .expect("main stream name");
    file[pos] = b'X';
    file
}

#[test]
fn detect_high_on_outer_magic() {
    assert_eq!(CatiaCodec.detect(OUTER_MAGIC), Confidence::High);
    assert_eq!(CatiaCodec.detect(&standard_catpart()), Confidence::High);
    assert_eq!(CatiaCodec.detect(b"PK\x03\x04 not catia"), Confidence::No);
}

#[test]
fn scan_parses_directory_and_identifies_standard() {
    let f = standard_catpart();
    let scan = crate::container::scan_bytes(f);
    assert_eq!(scan.variant, Variant::StandardNested);
    let dir = scan.inner.expect("inner directory");
    assert!(dir.descriptors.iter().any(|d| d.name == "MainDataStream"));
    assert!(dir.descriptors.iter().any(|d| d.name == "SurfacicReps"));
    let brep = scan.brep.expect("reconstructed brep stream");
    // The BREP stream is MainDataStream followed by SurfacicReps.
    assert!(brep.windows(3).any(|w| w == [0x05, 0x08, 0x01]));
    assert!(brep.windows(3).any(|w| w == [0x00, 0x33, 0x33]));
    assert!(scan.census.fbb_runs >= 2);
    assert!(scan.census.edge_delimiters >= 1);
    assert_eq!(scan.census.vertex_markers, 3);
}

#[test]
fn inspect_enumerates_streams_and_names_variant() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let summary = CatiaCodec.inspect(&mut cur).unwrap();
    assert_eq!(summary.format, "catia");
    assert_eq!(summary.container_kind, "v5-cfv2");
    assert!(summary.entries.iter().any(|e| e.name == "MainDataStream"));
    assert!(summary.entries.iter().any(|e| e.name == "SurfacicReps"));
    assert!(summary.notes.iter().any(|n| n.contains("standard nested")));
}

#[test]
fn decode_standard_transfers_vertices_and_cylinder() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report.geometry_transferred);
    // Three vertex records → three points and three vertices.
    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
    // A vertex coordinate is transferred verbatim in millimetres (no scaling).
    assert!(result
        .ir
        .model
        .points
        .iter()
        .any(|p| (p.position.x - 10.0).abs() < 1e-6));

    // Cylinder and tag-bridged plane carriers are decoded from their stored
    // parameters.
    assert_eq!(result.ir.model.surfaces.len(), 2);
    assert_eq!(result.ir.model.curves.len(), 1);
    let unknowns = result.ir.native_unknowns("catia").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].id.0, "catia:payload:unknown#brep-stream");
    assert!(unknowns[0]
        .links
        .contains(&"catia:standard:circle#0".to_string()));
    match &result.ir.model.surfaces[0].geometry {
        SurfaceGeometry::Cylinder { radius, axis, .. } => {
            assert!((radius - 5.0).abs() < 1e-6);
            assert!((axis.z - 1.0).abs() < 1e-6);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        }
            if (origin.x - 1.0).abs() < 1e-6
                && (origin.y - 2.0).abs() < 1e-6
                && (origin.z - 3.0).abs() < 1e-6
                && normal.x.abs() < 1e-6
                && normal.y.abs() < 1e-6
                && (normal.z.abs() - 1.0).abs() < 1e-6
                && u_axis.x.abs() < 1e-6
                && (u_axis.y - 1.0).abs() < 1e-6
                && u_axis.z.abs() < 1e-6
    )));

    // Complete FBB face records with stored carrier senses bind the analytic
    // carrier order to a body/shell/face hierarchy. Boundary topology remains
    // unavailable until the trim/edge graph is decoded.
    assert_eq!(result.ir.model.faces.len(), 2);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(matches!(
        result.ir.model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    ));
    assert!(matches!(
        result.ir.model.faces[1].sense,
        cadmpeg_ir::topology::Sense::Reversed
    ));
    assert!(result.ir.model.edges.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.category == cadmpeg_ir::report::LossCategory::Topology));

    // The produced IR validates (free carriers, no dangling references).
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_standard_builds_surface_bound_topology_graph() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(tetrahedron_topology_catpart()),
            &DecodeOptions::default(),
        )
        .expect("decode generated topology part");

    assert_eq!(decoded.ir.model.faces.len(), 4);
    assert_eq!(decoded.ir.model.loops.len(), 4);
    assert_eq!(decoded.ir.model.edges.len(), 6);
    assert_eq!(decoded.ir.model.coedges.len(), 12);
    assert!(decoded
        .ir
        .model
        .faces
        .iter()
        .all(|face| face.loops.len() == 1));
    assert!(decoded
        .ir
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.radial_next != coedge.id));
    assert!(decoded
        .ir
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_some()));
    assert!(!decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Topology
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
}

#[test]
fn decode_fbb_only_transfers_shared_vertices_and_carriers() {
    assert_eq!(
        crate::container::scan_bytes(fbb_only_catpart()).variant,
        Variant::FbbOnly
    );
    let mut cur = Cursor::new(fbb_only_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(result.ir.model.surfaces.len(), 2);
}

#[test]
fn decode_zero_entity_falls_back_to_metadata() {
    let f = zero_entity_catpart();
    let scan = crate::container::scan_bytes(f.clone());
    assert_eq!(scan.variant, Variant::ZeroEntity);
    assert!(scan.inner.is_none());

    let mut cur = Cursor::new(f);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(!result.report.geometry_transferred);
    let source = result.ir.source.expect("source metadata");
    assert_eq!(
        source.attributes.get("variant").map(String::as_str),
        Some("zero_entity")
    );
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.message.contains("zero_entity")));
}

#[test]
fn decode_zero_entity_transfers_framed_cylinder() {
    let mut cur = Cursor::new(zero_entity_cylinder_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    match &result.ir.model.surfaces[0].geometry {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
            assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(
                *ref_direction,
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            );
            assert_eq!(*radius, 4.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }
}

#[test]
fn decode_zero_entity_transfers_inline_nurbs_surface() {
    let mut cur = Cursor::new(zero_entity_nurbs_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.surfaces.len(), 1);
    match &result.ir.model.surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree, surface.v_degree), (2, 2));
            assert_eq!((surface.u_count, surface.v_count), (3, 3));
            assert_eq!(surface.u_knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            assert_eq!(surface.control_points.len(), 9);
            assert_eq!(surface.control_points[8].x, 8.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn e5_circle_parser_reads_framed_carrier() {
    let stream = e5_circle_stream();
    let circles = crate::geometry::e5_circles(&stream);
    assert_eq!(circles.len(), 1);
    match &circles[0].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        } => {
            assert_eq!(*center, cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0));
            assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(*radius, 2.5);
        }
        other => panic!("expected circle, got {other:?}"),
    }
    let surfaces = crate::geometry::e5_surfaces(&stream);
    assert!(matches!(
        surfaces[0].geometry,
        SurfaceGeometry::Cylinder { radius: 2.5, .. }
    ));
}

#[test]
fn e5_edge_parser_reads_u24_reference_tokens() {
    let mut record = vec![0u8; 13];
    record[..3].copy_from_slice(&[0xe5, 0x0d, 0x03]);
    record[3] = 0xff;
    let payload = [
        0x85, 0x38, 1, 2, 3, 0x38, 4, 5, 6, 0x38, 7, 8, 9, 0x80, 0x80, 0x80,
    ];
    record[5..7].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    record.extend_from_slice(&payload);

    let edges = crate::geometry::e5_edges(&record);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].support_id, 0x03_0201);
    assert_eq!(edges[0].start_vertex_id, 0x06_0504);
    assert_eq!(edges[0].end_vertex_id, 0x09_0807);
}

fn append_e5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(&[0xe5, 0x0d, 0x03, class, 0]);
    bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

fn e5_uv_line_payload(surface: u16, offset: f64) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    for value in [offset, 0.0, 1.0, 0.0, -1.0, 1.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload
}

fn e5_torus_topology_stream() -> Vec<u8> {
    let mut bytes = Vec::new();

    let mut torus = vec![0; 130];
    for (offset, value) in [
        (1, 0.0),
        (9, 0.0),
        (17, 0.0),
        (25, 1.0),
        (33, 0.0),
        (41, 0.0),
        (73, 0.0),
        (81, 0.0),
        (89, 1.0),
        (97, 10.0),
        (105, 2.0),
    ] {
        torus[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    append_e5_record(&mut bytes, 0xcc, 50, &torus);

    for id in [10u32, 20, 30, 40] {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }

    let raw_corners = [
        [0.0, 0.0],
        [5.0 * std::f64::consts::PI, 0.0],
        [5.0 * std::f64::consts::PI, std::f64::consts::PI],
        [0.0, std::f64::consts::PI],
    ];
    for index in 0..4 {
        let start = raw_corners[index];
        let end = raw_corners[(index + 1) % 4];
        let mut payload = vec![0x81, 0xb2];
        for value in [
            start[0],
            start[1],
            end[0] - start[0],
            end[1] - start[1],
            0.0,
            1.0,
        ] {
            payload.extend_from_slice(&le_f64(value));
        }
        append_e5_record(&mut bytes, 0x96, 60 + index as u32, &payload);

        let mut support = vec![0x81, 0xbc + index as u8, 0x81, 0, 0];
        support.extend_from_slice(&le_f64(0.0));
        support.extend_from_slice(&le_f64(1.0));
        append_e5_record(&mut bytes, 0xc0, 70 + index as u32, &support);
    }

    for (index, (start, end)) in [(10u8, 20u8), (20, 30), (30, 40), (40, 10)]
        .into_iter()
        .enumerate()
    {
        append_e5_record(
            &mut bytes,
            0xff,
            80 + index as u32,
            &[
                0x85,
                0xc6 + index as u8,
                0x80 + start,
                0x80 + end,
                0x80,
                0x80,
                0x80,
            ],
        );
    }

    let mut loop_payload = vec![0x89];
    for index in 0..4 {
        loop_payload.extend_from_slice(&[0xbc + index, 0xd0 + index]);
    }
    loop_payload.push(0xb2);
    append_e5_record(&mut bytes, 0x09, 90, &loop_payload);
    append_e5_record(&mut bytes, 0x00, 91, &[0x82, 0xb2, 0xda, 1, 0]);
    append_e5_record(&mut bytes, 0x08, 92, &[0x81, 0xdb, 0x81, 1, 0, 1, 0, 1, 0]);
    append_e5_record(&mut bytes, 0x01, 93, &[0x81, 0xdc]);

    for xyz in [
        [12.0f32, 0.0, 0.0],
        [0.0, 12.0, 0.0],
        [0.0, 10.0, 2.0],
        [10.0, 0.0, 2.0],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}

#[test]
fn e5_topology_follows_face_loop_and_serialized_edge_members() {
    let mut bytes = Vec::new();
    for id in [10u32, 20, 30] {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }
    for (id, start, end) in [(100u8, 10u8, 20u8), (101, 20, 30), (102, 30, 10)] {
        append_e5_record(
            &mut bytes,
            0xff,
            u32::from(id),
            &[0x85, 0x08, 200, 0x08, start, 0x08, end, 0x80, 0x80, 0x80],
        );
    }
    for (id, surface, offset) in [
        (400u32, 500u16, 0.0),
        (401, 500, 1.0),
        (402, 500, 2.0),
        (410, 501, 0.0),
        (411, 501, 1.0),
        (412, 501, 2.0),
    ] {
        append_e5_record(&mut bytes, 0x96, id, &e5_uv_line_payload(surface, offset));
    }
    let mut jet = vec![0x81, 0x18];
    jet.extend_from_slice(&500u16.to_le_bytes());
    for value in [5u32, 0, 0, 2, 0, 0, 0] {
        jet.extend_from_slice(&value.to_le_bytes());
    }
    jet.extend_from_slice(&le_f64(1.0));
    for value in [6u32, 6, 2] {
        jet.extend_from_slice(&value.to_le_bytes());
    }
    for values in [[1.0f64, 0.0], [0.0, 1.0], [0.0, -1.0], [1.0, 0.0]] {
        for value in values {
            jet.extend_from_slice(&le_f64(value));
        }
    }
    jet.extend_from_slice(&1u16.to_le_bytes());
    for values in [[-1.0f64, 0.0], [0.0, -1.0]] {
        for value in values {
            jet.extend_from_slice(&le_f64(value));
        }
    }
    jet.extend_from_slice(&le_f64(0.0));
    jet.extend_from_slice(&le_f64(1.0));
    append_e5_record(&mut bytes, 0xa0, 403, &jet);
    let mut support_payload = vec![0x82, 0x18, 144, 1, 0x18, 154, 1, 0x81, 0, 0];
    support_payload.extend_from_slice(&le_f64(-10.0));
    support_payload.extend_from_slice(&le_f64(10.0));
    append_e5_record(&mut bytes, 0xc1, 200, &support_payload);
    let mut bound_payload = vec![0x82, 0x18, 144, 1, 0x08, 200, 0x82];
    for (parameter, code) in [(0.25f64, 1u32), (0.75, 7)] {
        bound_payload.extend_from_slice(&le_f64(parameter));
        bound_payload.extend_from_slice(&code.to_le_bytes());
    }
    append_e5_record(&mut bytes, 0x0e, 900, &bound_payload);
    let mut loop_payload = vec![
        0x87, 0x18, 144, 1, 0x08, 100, 0x18, 145, 1, 0x08, 101, 0x18, 146, 1, 0x08, 102, 0x18, 244,
        1, 0x83,
    ];
    for _ in 0..13 {
        loop_payload.extend_from_slice(&1i16.to_le_bytes());
    }
    append_e5_record(&mut bytes, 0x09, 300, &loop_payload);
    let mut reverse_loop_payload = vec![
        0x87, 0x18, 154, 1, 0x08, 100, 0x18, 156, 1, 0x08, 102, 0x18, 155, 1, 0x08, 101, 0x18, 245,
        1, 0x83,
    ];
    for _ in 0..13 {
        reverse_loop_payload.extend_from_slice(&1i16.to_le_bytes());
    }
    append_e5_record(&mut bytes, 0x09, 301, &reverse_loop_payload);
    append_e5_record(&mut bytes, 0xcc, 500, &[]);
    append_e5_record(&mut bytes, 0xcc, 501, &[]);
    append_e5_record(
        &mut bytes,
        0x00,
        600,
        &[0x82, 0x18, 244, 1, 0x18, 44, 1, 0x01, 0x00],
    );
    append_e5_record(
        &mut bytes,
        0x00,
        601,
        &[0x82, 0x18, 245, 1, 0x18, 45, 1, 0x01, 0x00],
    );
    append_e5_record(
        &mut bytes,
        0x08,
        700,
        &[0x82, 0x18, 88, 2, 0x18, 89, 2, 0x82, 1, 0, 1, 0, 1, 0, 1, 0],
    );
    append_e5_record(&mut bytes, 0x01, 800, &[0x81, 0x18, 188, 2]);

    let topology = crate::e5::parse_topology(&bytes).expect("E5 graph");
    assert_eq!(topology.faces.len(), 2);
    assert_eq!(topology.faces[0].surface, 500);
    assert_eq!(topology.faces[0].loops[0].edge_uses, vec![100, 101, 102]);
    assert_eq!(
        topology.faces[0].loops[0].reversed,
        vec![false, false, false]
    );
    assert_eq!(topology.faces[0].loops[0].outer, Some(true));
    assert_eq!(
        topology.faces[0].loops[0].absolute_reversed,
        Some(vec![false, false, false])
    );
    assert_eq!(
        topology.faces[1].loops[0].absolute_reversed,
        Some(vec![true, true, true])
    );
    assert_eq!(topology.bodies[0].faces, vec![600, 601]);
    assert_eq!(topology.pcurves.len(), 7);
    assert!(matches!(
        topology.pcurves[&400],
        crate::e5::E5Pcurve::Line {
            direction: [1.0, 0.0],
            ..
        }
    ));
    assert_eq!(topology.bounds[&900].entries[0].parameter, 0.25);
    assert_eq!(topology.bounds[&900].entries[1].representation, 200);
    assert_eq!(topology.curve_supports[&200].pcurves, vec![400, 410]);
    assert_eq!(topology.curve_supports[&200].range, [-10.0, 10.0]);
    assert!(matches!(
        topology.pcurves[&403],
        crate::e5::E5Pcurve::Jet { degree: 5, ref knots, .. } if knots == &[0.0, 1.0]
    ));
}

#[test]
fn standard_circle_parser_rejects_non_support_marker() {
    let mut bytes = vec![0x61, 0, 0, 0, 0, 0x12, 0, 0x33, 0x37];
    bytes.extend_from_slice(&[0; 18]);
    assert!(crate::geometry::standard_circles(&bytes, 1).is_empty());
}

fn zero_entity_record(kind: u8, mut tail: Vec<u8>) -> Vec<u8> {
    let length = 12 + tail.len();
    let mut record = vec![
        0xa9,
        0x03,
        kind,
        u8::try_from(length - 12).expect("length code"),
    ];
    record.resize(12, 0);
    record.append(&mut tail);
    record
}

#[test]
fn zero_entity_parser_decodes_face_loop_lanes_and_packed_senses() {
    let mut carrier = zero_entity_record(0x27, vec![0; 106]);
    for (offset, value) in [
        (14usize, 10.0f64),
        (22, 0.0),
        (30, 0.0),
        (38, 1.0),
        (46, 0.0),
        (54, 0.0),
        (62, 0.0),
        (70, 1.0),
        (78, 0.0),
    ] {
        carrier[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    let mut support_tail = vec![0; 113];
    support_tail[0] = 0x10;
    support_tail[1..5].copy_from_slice(&2u32.to_le_bytes());
    for (offset, value) in [(81usize, 1.0f64), (89, 2.0), (97, 3.0), (105, 4.0)] {
        support_tail[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    let support = zero_entity_record(0x21, support_tail);
    let mut face_tail = vec![0x82, 0x10];
    face_tail.extend_from_slice(&1000u32.to_le_bytes());
    face_tail.push(0x10);
    face_tail.extend_from_slice(&900u32.to_le_bytes());
    face_tail.push(0);
    let face = zero_entity_record(0x5f, face_tail);

    let mut loop_tail = vec![0x85];
    for reference in [98u32, 500, 97, 501, 100] {
        loop_tail.push(0x10);
        loop_tail.extend_from_slice(&reference.to_le_bytes());
    }
    loop_tail.extend_from_slice(&[0x82, 0x41, 0b01_0111, 0x01]);
    let loop_record = zero_entity_record(0x62, loop_tail);
    let mut physical_edge = zero_entity_record(0x5e, vec![0; 26]);
    for (offset, reference) in [
        (7usize, 10u32),
        (12, 20),
        (17, 30),
        (22, 40),
        (27, 50),
        (32, 60),
    ] {
        physical_edge[offset] = 0x10;
        physical_edge[offset + 1..offset + 5].copy_from_slice(&reference.to_le_bytes());
    }
    let mut side_pair_tail = vec![0x82, 0x10];
    side_pair_tail.extend_from_slice(&1000u32.to_le_bytes());
    side_pair_tail.push(0x10);
    side_pair_tail.extend_from_slice(&2000u32.to_le_bytes());
    side_pair_tail.resize(105, 0);
    let side_pair = zero_entity_record(0x25, side_pair_tail);
    let coedge_twin = |side: u8| {
        let mut record = zero_entity_record(0x06, vec![0; 56]);
        record[7] = 0x10;
        record[8..12].copy_from_slice(&1u32.to_le_bytes());
        record[12..15].copy_from_slice(&[0x83, 0x10, side]);
        for (offset, reference) in [
            (15usize, 1000u32 + u32::from(side)),
            (20, 2000u32 + u32::from(side)),
        ] {
            record[offset] = 0x10;
            record[offset + 1..offset + 5].copy_from_slice(&reference.to_le_bytes());
        }
        record
    };
    let mut incidence_tail = vec![0x83];
    for item in [700u32, 701, 702] {
        incidence_tail.push(0x10);
        incidence_tail.extend_from_slice(&item.to_le_bytes());
    }
    let incidence = zero_entity_record(0x05, incidence_tail);
    let vertex_marker = zero_entity_record(0x5d, vec![0; 6]);
    let mut bytes = carrier;
    bytes.extend_from_slice(&support);
    bytes.extend_from_slice(&face);
    bytes.extend_from_slice(&loop_record);
    bytes.extend_from_slice(&physical_edge);
    bytes.extend_from_slice(&side_pair);
    bytes.extend_from_slice(&coedge_twin(1));
    bytes.extend_from_slice(&coedge_twin(2));
    bytes.extend_from_slice(&incidence);
    bytes.extend_from_slice(&vertex_marker);

    let topology = crate::zero_entity::parse(&bytes).expect("zero-entity topology records");
    assert_eq!(topology.faces[0].loop_terminals, vec![100]);
    assert_eq!(topology.loops[0].member_ids, vec![98, 97]);
    assert_eq!(topology.loops[0].secondary_refs, vec![500, 501]);
    assert_eq!(topology.loops[0].terminal_id, 100);
    assert_eq!(topology.loops[0].reversed, vec![false, true]);
    assert!(!topology.loops[0].inner);
    assert_eq!(topology.carrier_runs[0].support_ordinals, vec![1]);
    assert_eq!(topology.supports[0].slot, 2);
    assert_eq!(
        topology.supports[0].uv_endpoints,
        Some([[1.0, 2.0], [3.0, 4.0]])
    );
    assert_eq!(topology.faces[0].loop_indices, vec![0]);
    assert_eq!(topology.loops[0].support_indices, vec![Some(0), None]);
    assert_eq!(
        topology.supports[0].lifted_endpoints,
        Some([[11.0, 2.0, 0.0], [13.0, 4.0, 0.0]])
    );
    assert_eq!(
        topology.physical_edges[0].references,
        [10, 20, 30, 40, 50, 60]
    );
    assert_eq!(topology.side_pairs[0].bases, [1000, 2000]);
    assert_eq!(
        topology.side_pairs[0].composite_keys,
        [[1001, 2001], [1002, 2002]]
    );
    assert_eq!(topology.coedge_twins[1].side, 2);
    assert_eq!(topology.vertices[0].incidence_items, vec![700, 701, 702]);
}

fn append_b5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

fn b5_linear_pcurve_payload(surface: u16, start: [f64; 2], end: [f64; 2]) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.extend_from_slice(&[0x01, 5, 0, 0, 9, 0x08, 1]);
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.extend_from_slice(&[9, 9]);
    for uv in [start, end] {
        payload.extend_from_slice(&le_f64(uv[0]));
        payload.extend_from_slice(&le_f64(uv[1]));
    }
    payload.extend_from_slice(&[0x05, 0x05, 0x07]);
    payload
}

fn b5_closed_triangle_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut plane = vec![0; 73];
    for (offset, value) in [
        (1usize, 0.0f64),
        (9, 0.0),
        (17, 0.0),
        (25, 1.0),
        (33, 0.0),
        (41, 0.0),
        (49, 0.0),
        (57, 1.0),
        (65, 0.0),
    ] {
        plane[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    append_b5_record(&mut bytes, 0x27, 100, &plane);
    for (id, start, end) in [
        (200u32, [0.0, 0.0], [1.0, 0.0]),
        (201, [1.0, 0.0], [0.0, 1.0]),
        (202, [0.0, 1.0], [0.0, 0.0]),
    ] {
        append_b5_record(
            &mut bytes,
            0x21,
            id,
            &b5_linear_pcurve_payload(100, start, end),
        );
    }
    for id in [300u32, 301, 302] {
        append_b5_record(&mut bytes, 0x5e, id, &[]);
    }
    append_b5_record(
        &mut bytes,
        0x62,
        400,
        &[
            0x87, 0x18, 200, 0, 0x18, 44, 1, 0x18, 201, 0, 0x18, 45, 1, 0x18, 202, 0, 0x18, 46, 1,
            0x18, 100, 0,
        ],
    );
    append_b5_record(&mut bytes, 0x5f, 500, &[0x18, 100, 0, 0x18, 144, 1]);
    for point in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}

#[test]
fn b5_object_graph_resolves_face_loop_pcurve_and_edge_members() {
    let mut bytes = a8_surface_stream();
    bytes[7..11].copy_from_slice(&0x1234u32.to_le_bytes());
    let mut plane = vec![0; 73];
    for (offset, value) in [
        (1usize, 10.0f64),
        (9, 0.0),
        (17, 0.0),
        (25, 1.0),
        (33, 0.0),
        (41, 0.0),
        (49, 0.0),
        (57, 1.0),
        (65, 0.0),
    ] {
        plane[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    append_b5_record(&mut bytes, 0x27, 100, &plane);
    for (id, offset) in [(200u32, 0.0f64), (201, 1.0), (202, 2.0)] {
        let payload = b5_linear_pcurve_payload(100, [offset, 0.0], [offset + 1.0, 0.0]);
        append_b5_record(&mut bytes, 0x21, id, &payload);
    }
    let mut profile = vec![0; 49];
    for (offset, value) in [
        (1usize, 1.0f64),
        (9, 0.0),
        (17, 0.0),
        (25, 0.0),
        (33, 0.0),
        (41, 1.0),
    ] {
        profile[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    append_b5_record(&mut bytes, 0x0e, 110, &profile);
    let mut revolution = vec![0; 143];
    revolution[1] = 0x38;
    revolution[2..5].copy_from_slice(&[110, 0, 0]);
    revolution[77 + 16..77 + 24].copy_from_slice(&le_f64(1.0));
    revolution[135..143].copy_from_slice(&le_f64(1.0));
    append_b5_record(&mut bytes, 0x2d, 120, &revolution);
    append_b5_record(
        &mut bytes,
        0x21,
        210,
        &b5_linear_pcurve_payload(120, [1.0, 0.0], [1.0, std::f64::consts::PI]),
    );
    append_b5_record(
        &mut bytes,
        0x21,
        211,
        &b5_linear_pcurve_payload(0x1234, [0.0, 0.0], [1.0, 1.0]),
    );
    append_b5_record(&mut bytes, 0x5e, 300, &[]);
    append_b5_record(&mut bytes, 0x5e, 301, &[]);
    append_b5_record(&mut bytes, 0x5e, 0x01_0100, &[]);
    let loop_payload = [
        0x87, 0x18, 200, 0, 0x18, 44, 1, 0x18, 201, 0, 0x18, 45, 1, 0x18, 202, 0, 0x30, 1, 1, 0x18,
        100, 0,
    ];
    append_b5_record(&mut bytes, 0x62, 400, &loop_payload);
    append_b5_record(&mut bytes, 0x5f, 500, &[0x18, 100, 0, 0x18, 144, 1]);
    for point in [
        [10.0f32, 0.0, 0.0],
        [11.0, 0.0, 0.0],
        [12.0, 0.0, 0.0],
        [13.0, 0.0, 0.0],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let graph = crate::b5::parse(&bytes).expect("B5 object topology");
    assert_eq!(graph.faces[0].surface, 100);
    assert_eq!(graph.faces[0].loops, vec![400]);
    assert_eq!(graph.loops[&400].pcurves, vec![200, 201, 202]);
    assert_eq!(graph.loops[&400].edges, vec![300, 301, 0x01_0100]);
    assert_eq!(graph.pcurves[&200].degree, 1);
    assert_eq!(
        graph.pcurves[&200].control_points,
        vec![[0.0, 0.0], [1.0, 0.0]]
    );
    assert_eq!(
        graph.pcurves[&200].lifted_endpoints,
        Some([[10.0, 0.0, 0.0], [11.0, 0.0, 0.0]])
    );
    assert_eq!(graph.edge_vertices[&300], [0, 1]);
    assert_eq!(graph.edge_vertices[&0x01_0100], [2, 3]);
    let revolution_endpoints = graph.pcurves[&210]
        .lifted_endpoints
        .expect("revolution lift");
    assert!((revolution_endpoints[0][0] - 1.0).abs() < 1e-12);
    assert!((revolution_endpoints[1][0] + 1.0).abs() < 1e-12);
    assert!((revolution_endpoints[1][2] - 1.0).abs() < 1e-12);
    assert_eq!(
        graph.pcurves[&211].lifted_endpoints,
        Some([[0.0, 0.0, 0.0], [8.0, 2.0, 2.0]])
    );
}

#[test]
fn standard_line_parser_reads_face_incidence() {
    let bytes = [0x60, 1, 2, 3, 0, 2, 0, 0x33, 0x36, 0, 1];
    let lines = crate::geometry::standard_lines(&bytes, 2);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].faces, [0, 1]);
}

#[test]
fn e5_surface_parser_reads_framed_torus() {
    let surfaces = crate::geometry::e5_surfaces(&e5_torus_stream());
    assert_eq!(surfaces.len(), 1);
    match &surfaces[0].geometry {
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            assert_eq!(*center, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
            assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(
                *ref_direction,
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            );
            assert_eq!((*major_radius, *minor_radius), (12.0, 2.0));
        }
        other => panic!("expected torus, got {other:?}"),
    }
}

#[test]
fn e5_plane_parser_preserves_origin_and_natural_bounds_without_fabricating_axes() {
    let planes = crate::geometry::e5_planes(&e5_plane_stream());
    assert_eq!(planes.len(), 1);
    assert_eq!(planes[0].record_id, 42);
    assert_eq!(planes[0].origin, [1.0, 2.0, 3.0]);
    assert_eq!(planes[0].u_range, [-4.0, 7.0]);
    assert_eq!(planes[0].v_range, [-2.0, 9.0]);
}

#[test]
fn a8_surface_parser_reads_common_form_nurbs() {
    let surfaces = crate::geometry::a8_surfaces(&a8_surface_stream());
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].object_id, 0xdeca_fbad);
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree, surface.v_degree), (2, 2));
            assert_eq!((surface.u_count, surface.v_count), (3, 3));
            assert_eq!(surface.control_points[8].x, 8.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a8_pcurve_parser_reads_degree5_uv_jet() {
    let pcurves = crate::geometry::a8_pcurves(&a8_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(
        (pcurves[0].object_id, pcurves[0].support_id),
        (0x5678, 0x1234)
    );
    assert_eq!(pcurves[0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(pcurves[0].range, [0.0, 1.0]);
}

#[test]
fn a5_pcurve_parser_reads_compact_support_and_uv_jet() {
    let pcurves = crate::geometry::a5_pcurves(&a5_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x1234);
    assert_eq!(pcurves[0].extrapolation_sites, 2);
    assert_eq!(pcurves[0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(pcurves[0].range, [0.0, 1.0]);
}

#[test]
fn consolidated_pcurve_parser_reads_width2_frame() {
    let pcurves = crate::geometry::a5_pcurves(&a6_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x1234);
    assert_eq!(pcurves[0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
}

#[test]
fn b_family_pcurve_parser_reads_six_channel_uv_jet() {
    let pcurves = crate::geometry::b2_pcurves(&b2_pcurve_stream());
    assert_eq!(pcurves.len(), 1);
    assert_eq!(pcurves[0].support_id, 0x1234);
    assert_eq!(pcurves[0].degree, 5);
    assert_eq!(pcurves[0].second_derivatives, vec![[0.0, 0.0]; 2]);
}

#[test]
fn b2_parameter_point_parser_reads_uv_station_and_unsplit_layouts() {
    use crate::geometry::B2ParameterPoint;

    let points = crate::geometry::b2_parameter_points(&b2_parameter_point_stream());
    assert_eq!(points.len(), 3);
    assert!(matches!(
        points[0],
        B2ParameterPoint::Uv { uv: [2.0, 3.0], .. }
    ));
    assert!(matches!(
        points[1],
        B2ParameterPoint::StationUv {
            station: 11.0,
            uv: [4.0, 5.0],
            ..
        }
    ));
    assert!(matches!(points[2], B2ParameterPoint::FiveScalars { .. }));
}

#[test]
fn b2_reference_list_parser_reads_compact_refs_and_unit_tail() {
    let records = crate::geometry::b2_reference_lists(&b2_reference_list_stream());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].references, (0u32..26).collect::<Vec<_>>());
}

#[test]
fn b2_cone_face_parser_reads_refs_scale_and_half_angle() {
    let records = crate::geometry::b2_cone_faces(&b2_cone_face_stream());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].references.len(), 16);
    assert_eq!(records[0].angular_scale, 1.5);
    assert_eq!(records[0].half_angle, std::f64::consts::FRAC_PI_4);
}

#[test]
fn b2_topology_metadata_parser_preserves_refs_and_sense_code() {
    use crate::geometry::B2UseSense;

    let bytes = b2_topology_metadata_stream();
    let edges = crate::geometry::b2_edge_metadata(&bytes);
    let uses = crate::geometry::b2_use_metadata(&bytes);
    assert_eq!(edges[0].references, vec![0x1234, 0x5678]);
    assert_eq!(edges[0].payload, [0x0a, 0x34, 0x12, 0x0a, 0x78, 0x56, 0]);
    assert_eq!(uses[0].sense, Some(B2UseSense::Sense88));
    assert_eq!(uses[0].payload, [1, 2, 3, 0x88]);
}

#[test]
fn b2_revolution_parser_reads_axis_profile_bounds_and_exact_scale_relations() {
    let records = crate::geometry::b2_revolutions(&b2_revolution_stream());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].profile_curve_id, 0x1234);
    assert_eq!(records[0].origin, [1.0, 2.0, 3.0]);
    assert_eq!(records[0].axis, [0.0, 0.0, 1.0]);
    assert_eq!(records[0].profile_range, [-4.0, 9.0]);
}

#[test]
fn b2_group_parser_reads_separator_and_typed_opener() {
    let bytes = b2_group_stream();
    let separators = crate::geometry::b2_group_separators(&bytes);
    let groups = crate::geometry::b2_groups(&bytes);
    assert_eq!(separators.len(), 1);
    assert_eq!(separators[0].token, 0x05);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group_id, 32);
    assert_eq!(groups[0].group_type, 3);
}

#[test]
fn b2_offset_support_parser_reads_carrier_distance_and_domain() {
    let offsets = crate::geometry::b2_offset_supports(&b2_offset_support_stream());
    assert_eq!(offsets.len(), 1);
    assert_eq!(offsets[0].support_id, 0x1234);
    assert_eq!(offsets[0].distance, 2.5);
    assert_eq!(offsets[0].domain, [0.0, -1.0, 4.0, 3.0]);
}

#[test]
fn consolidated_offset_support_parser_reads_width2_frame() {
    let offsets = crate::geometry::b2_offset_supports(&b3_offset_support_stream());
    assert_eq!(offsets.len(), 1);
    assert_eq!(offsets[0].support_id, 0x1234);
    assert_eq!(offsets[0].distance, 2.5);
}

#[test]
fn b2_edge_parameter_parser_validates_repeated_range_packet() {
    let packets = crate::geometry::b2_edge_parameters(&b2_edge_parameter_stream());
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].range, [2.0, 7.0]);
    assert_eq!(packets[0].tolerance, 1e-6);
}

#[test]
fn a5_edge_block_parser_groups_two_coparametric_pcurves_and_packet() {
    let blocks = crate::geometry::a5_edge_blocks(&a5_edge_block_stream());
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].co_parametric);
    assert_eq!(blocks[0].pcurves[0].support_id, 0x1234);
    assert_eq!(blocks[0].pcurves[1].range, [0.0, 1.0]);
    assert_eq!(blocks[0].parameters.range, [0.0, 1.0]);
}

#[test]
fn a5_edge_binding_resolves_cylinder_by_endpoint_lifts() {
    use crate::geometry::A5SupportBinding;

    let blocks = crate::geometry::resolve_a5_edge_blocks(&a5_cylinder_bound_edge_stream());
    assert_eq!(blocks.len(), 1);
    assert!(matches!(
        blocks[0].supports[0],
        Some(A5SupportBinding::Cylinder { .. })
    ));
    assert!(matches!(
        blocks[0].supports[1],
        Some(A5SupportBinding::Cylinder { .. })
    ));
}

#[test]
fn a5_edge_binding_resolves_partner_nurbs_carrier() {
    use crate::geometry::A5SupportBinding;

    let blocks = crate::geometry::resolve_a5_edge_blocks(&a5_nurbs_bound_edge_stream(0.0));
    assert!(matches!(
        blocks[0].supports[0],
        Some(A5SupportBinding::Cylinder { .. })
    ));
    assert!(matches!(
        blocks[0].supports[1],
        Some(A5SupportBinding::NurbsCarrier { offset, .. }) if offset == 0.0
    ));
}

#[test]
fn a5_edge_binding_resolves_constant_normal_offset_carrier() {
    use crate::geometry::A5SupportBinding;

    let blocks = crate::geometry::resolve_a5_edge_blocks(&a5_nurbs_bound_edge_stream(2.0));
    assert!(matches!(
        blocks[0].supports[1],
        Some(A5SupportBinding::NurbsCarrier { offset, .. }) if (offset.abs() - 2.0).abs() < 1e-6
    ));
}

#[test]
fn a5_edge_binding_resolves_circle_by_constant_v_and_arc_range() {
    use crate::geometry::A5SupportBinding;

    let blocks = crate::geometry::resolve_a5_edge_blocks(&a5_circle_bound_edge_stream());
    assert!(matches!(
        blocks[0].supports[0],
        Some(A5SupportBinding::Circle { .. })
    ));
}

#[test]
fn a5_edge_binding_resolves_cone_by_endpoint_lifts() {
    use crate::geometry::A5SupportBinding;

    let blocks = crate::geometry::resolve_a5_edge_blocks(&a5_cone_bound_edge_stream());
    assert!(matches!(
        blocks[0].supports[0],
        Some(A5SupportBinding::Cone { .. })
    ));
}

#[test]
fn b2_circle_parser_reads_arc_length_parameterization() {
    let circles = crate::geometry::b2_circles(&b2_circle_stream());
    assert_eq!(circles.len(), 1);
    assert_eq!(circles[0].record_id, 0x1234);
    assert_eq!(circles[0].center_pair, [4.0, -2.0]);
    assert_eq!(circles[0].radius, 3.0);
    assert!(circles[0].full_circle);
}

#[test]
fn b2_cylinder_parser_reads_arc_length_carrier() {
    let cylinders = crate::geometry::b2_cylinders(&b2_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].u_range, [0.0, 4.0 * std::f64::consts::PI]);
    assert_eq!(cylinders[0].v_range, [-4.0, 5.0]);
    match &cylinders[0].geometry {
        Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        }) => {
            assert_eq!([origin.x, origin.y, origin.z], [1.0, 2.0, 3.0]);
            assert_eq!([axis.x, axis.y, axis.z], [1.0, 0.0, 0.0]);
            assert_eq!(*radius, 2.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }
}

#[test]
fn consolidated_cylinder_parser_reads_width2_frame() {
    let cylinders = crate::geometry::b2_cylinders(&b3_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].layout, 0x5a);
    assert!(cylinders[0].geometry.is_some());
}

#[test]
fn consolidated_frame_width_and_flag_are_independent() {
    let mut width1_flag13 = b2_cylinder_stream();
    width1_flag13[1] = 0x13;
    let mut width2_flag83 = b3_cylinder_stream();
    width2_flag83[1] = 0x83;
    assert_eq!(crate::geometry::b2_cylinders(&width1_flag13).len(), 1);
    assert_eq!(crate::geometry::b2_cylinders(&width2_flag83).len(), 1);
}

#[test]
fn consolidated_record_walk_inventory_preserves_width_flag_and_boundaries() {
    use crate::geometry::ConsolidatedFamily;

    let first = a6_pcurve_stream();
    let second = b3_cylinder_stream();
    let mut bytes = first.clone();
    bytes.extend_from_slice(&second);
    let records = crate::geometry::consolidated_records(&bytes);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].family, ConsolidatedFamily::A);
    assert_eq!(
        (records[0].width, records[0].flag, records[0].class),
        (2, 0x03, 0x20)
    );
    assert_eq!(records[0].range, 0..first.len());
    assert_eq!(records[1].family, ConsolidatedFamily::B);
    assert_eq!(records[1].range, first.len()..first.len() + second.len());
}

#[test]
fn b2_cylinder_parser_reads_implicit_axis_layout() {
    let cylinders = crate::geometry::b2_cylinders(&b2_implicit_axis_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].layout, 0x52);
    assert!(matches!(
        cylinders[0].geometry,
        Some(SurfaceGeometry::Cylinder { axis, .. }) if [axis.x, axis.y, axis.z] == [1.0, 0.0, 0.0]
    ));
}

#[test]
fn b2_cylinder_parser_preserves_phase_tailed_layout_raw() {
    let cylinders = crate::geometry::b2_cylinders(&b2_phase_tailed_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].layout, 0x62);
    assert!(cylinders[0].geometry.is_none());
    assert_eq!(cylinders[0].stored_vector, Some([0.0, 1.0]));
    assert_eq!(cylinders[0].phase, Some(0.75));
}

#[test]
fn b2_cone_parser_reads_orthonormal_slant_chart() {
    let cones = crate::geometry::b2_cones(&b2_cone_stream());
    assert_eq!(cones.len(), 1);
    assert_eq!(cones[0].apex, [1.0, 2.0, 3.0]);
    assert_eq!(cones[0].axis, [0.0, 0.0, 1.0]);
    assert_eq!(cones[0].half_angle, 0.25);
    assert_eq!(cones[0].slant_range, [2.0, 8.0]);
    assert_eq!(cones[0].angular_scale, 3.0);
}

#[test]
fn b2_construction_use_parser_reorders_offset_domain() {
    let uses = crate::geometry::b2_construction_uses(&b2_construction_use_stream());
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].support_id, 0x1234);
    assert_eq!(uses[0].distance, -2.0);
    assert_eq!(uses[0].kind, 0x01);
    assert_eq!(uses[0].domain, Some([0.0, -1.0, 4.0, 3.0]));
}

#[test]
fn b2_composite_parser_reads_embedded_cylinder_frame() {
    let cylinders = crate::geometry::b2_embedded_cylinders(&b2_embedded_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].object_id, 0x5678);
    assert_eq!(cylinders[0].wrapper_pos, 0);
    assert_eq!(
        cylinders[0].cylinder.u_range,
        [0.0, 4.0 * std::f64::consts::PI]
    );
}

#[test]
fn outer_object_graph_parser_reads_nested_heads_and_payload_fields() {
    use crate::object_graph::{PayloadField, PayloadSubtype};

    let graph = crate::object_graph::parse(&object_graph_stream()).unwrap();
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].owner_ref, Some(2));
    assert_eq!(graph.records[0].class_ref, Some(3));
    assert_eq!(graph.records[0].storage_ref, Some(4));
    assert_eq!(graph.records[0].subtype, PayloadSubtype::Empty);
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Reference { value: 5, .. },
            PayloadField::Scalar {
                tag: 0x3a,
                value: 7,
                ..
            },
            PayloadField::Terminator
        ]
    ));
    assert_eq!(graph.records[1].subtype, PayloadSubtype::Blob);
}

#[test]
fn catalog_parser_reads_exact_inclusive_length_dictionary() {
    let entries = [
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
        "Pad",
    ];
    let catalogs = crate::catalog::parse(&catalog_stream(&entries));

    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].declared_count, 7);
    assert_eq!(catalogs[0].entries.len(), entries.len());
    assert_eq!(catalogs[0].entries[4].ordinal, 4);
    assert_eq!(catalogs[0].entries[4].value, "Sketch");
    assert_eq!(catalogs[0].entries[5].value, "Pad");
}

#[test]
fn value_block_parser_reads_length_to_terminator_boundary() {
    let payload = [0x81, 0x83, 0x32, 0xea, 0, 0, 0, 0x83, 0x82];
    let mut bytes = value_block_stream(&payload);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let blocks = crate::value_block::parse(&bytes);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].pos, 0);
    assert_eq!(blocks[0].declared_len, 15);
    assert_eq!(blocks[0].total_len, 16);
    assert_eq!(blocks[0].payload, payload);
}

#[test]
fn outer_object_graph_vm_reads_lists_paged_atoms_bulk_and_null_handles() {
    use crate::object_graph::{HeadToken, ListItem, PayloadField, PayloadSubtype};

    let graph = crate::object_graph::parse(&object_graph_vm_stream()).unwrap();
    assert!(graph.records[0].head.contains(&HeadToken::NullHandle));
    assert_eq!(graph.records[0].subtype, PayloadSubtype::BulkTable);
    assert!(matches!(
        &graph.records[0].payload.fields[0],
        PayloadField::List { items, .. }
            if items == &vec![ListItem::Reference(5), ListItem::Atom(6), ListItem::Atom(10)]
    ));
    assert!(matches!(
        graph.records[0].payload.fields[1],
        PayloadField::BulkTable {
            count: 2,
            table_count: 1,
            ..
        }
    ));
}

#[test]
fn decode_retains_outer_object_graph_order_and_dependencies() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_object_graph()),
            &DecodeOptions::default(),
        )
        .expect("decode generated object graph part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.object_graphs.len(), 1);
    let graph = &native.object_graphs[0];
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].ordinal, 0);
    assert_eq!(graph.records[0].owner_ref, Some(2));
    assert_eq!(graph.records[0].class_ref, Some(3));
    assert_eq!(graph.records[0].storage_ref, Some(4));
    assert_eq!(graph.records[1].ordinal, 1);
    assert_eq!(graph.records[1].owner_ref, Some(2));
    assert_eq!(graph.records[1].class_ref, Some(4));
}

#[test]
fn decode_retains_catalog_schema_names_without_promoting_features() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_catalog()),
            &DecodeOptions::default(),
        )
        .expect("decode generated catalog part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.catalogs[0].entries[4].value, "Sketch");
    assert_eq!(native.catalogs[0].entries[5].value, "Pad");
    assert!(decoded.ir.model.features.is_empty());
}

#[test]
fn decode_retains_value_blocks_at_their_schema_boundary() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_value_block()),
            &DecodeOptions::default(),
        )
        .expect("decode generated value block part");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(native.value_blocks[0].byte_offset, 16);
    assert_eq!(native.value_blocks[0].byte_len, 16);
    assert_eq!(
        native.value_blocks[0].payload,
        [0x81, 0x83, 0x32, 0xea, 0, 0, 0, 0x83, 0x82]
    );
}

#[test]
fn outer_surface_alias_parser_reads_fixed_core() {
    use crate::object_graph::AliasLead;

    let rows = crate::object_graph::surface_aliases(&surface_alias_stream());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].lead, AliasLead::SurfaceSupportStorage);
    assert_eq!(rows[0].tag, 0x0012_3456);
    assert_eq!(rows[0].tag_raw, 0xab12_3456);
    assert_eq!(rows[0].entity_record_ordinal, 7);
    assert_eq!((rows[0].f2, rows[0].f3), (0x1122_3344, 0x5566_7788));
}

#[test]
fn unresolved_7cd9_scanner_preserves_bounded_context_and_spacing() {
    let markers = crate::object_graph::markers_7cd9(&marker_7cd9_stream(), 5);
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].pos, 1);
    assert_eq!(markers[0].context, [0x7c, 0xd9, 1, 2, 3]);
    assert_eq!(markers[0].next_delta, Some(5));
    assert_eq!(markers[1].next_delta, None);
}

#[test]
fn finjpl_parser_splits_segments_and_classifies_type_words() {
    use crate::container::FinjplKind;

    let bytes = finjpl_stream();
    let segments = crate::container::finjpl_segments(&bytes, 0, bytes.len());
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].kind, FinjplKind::Storage);
    assert_eq!(segments[0].type_word, 0x0000_008e);
    assert_eq!(segments[0].range, 2..17);
    assert_eq!(segments[1].kind, FinjplKind::ProjectFlags);
}

#[test]
fn e5_stream_selection_prefers_coherent_storage_segment_over_stray_preamble_marker() {
    let mut bytes = vec![0u8; 32];
    bytes[..8].copy_from_slice(OUTER_MAGIC);
    bytes[8..12].copy_from_slice(&512u32.to_be_bytes());
    bytes[12..16].copy_from_slice(&32u32.to_be_bytes());
    append_e5_record(&mut bytes, 0xfe, 1, &[]);
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0000_0080u32.to_be_bytes());
    for id in 10..21 {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0000_008eu32.to_be_bytes());
    let expected_start = bytes.len() - 12;
    for id in 30..41 {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }
    bytes.resize(544, 0);

    let range = crate::container::e5_record_stream(&bytes).expect("coherent E5 stream");
    assert_eq!(range.start, expected_start);
    assert_eq!(&bytes[range.start..range.start + 8], b"FINJPL  ");
}

#[test]
fn a8_surface_parser_reads_rational_weight_grid() {
    let surfaces = crate::geometry::a8_surfaces(&a8_rational_surface_stream());
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => assert_eq!(surface.weights, Some(vec![2.0; 9])),
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_surface_parser_reads_consolidated_nurbs() {
    let surfaces = crate::geometry::a5_surfaces(&a5_surface_stream());
    assert_eq!(surfaces.len(), 1);
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => {
            assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
            assert_eq!((surface.u_count, surface.v_count), (2, 2));
            assert_eq!(surface.control_points[3].x, 3.0);
        }
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn consolidated_surface_parser_reads_width2_frame() {
    let surfaces = crate::geometry::a5_surfaces(&a6_surface_stream());
    assert_eq!(surfaces.len(), 1);
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => assert_eq!((surface.u_count, surface.v_count), (2, 2)),
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_surface_parser_reads_rational_weight_program() {
    let surfaces = crate::geometry::a5_surfaces(&a5_rational_surface_stream());
    match &surfaces[0].geometry {
        SurfaceGeometry::Nurbs(surface) => assert_eq!(surface.weights, Some(vec![2.0; 4])),
        other => panic!("expected NURBS surface, got {other:?}"),
    }
}

#[test]
fn a5_curve_parser_reads_degree5_rolling_ball_jet() {
    let curves = crate::geometry::a5_freeform_curves(&a5_freeform_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].knots, vec![0.0, 1.0]);
    assert_eq!(curves[0].sites[1].radius, 2.0);
    assert!(!curves[0].radius_constant);
}

#[test]
fn consolidated_curve_parser_reads_width2_frame() {
    let curves = crate::geometry::a5_freeform_curves(&a6_freeform_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].sites[1].radius, 2.0);
}

#[test]
fn guide_curve_parser_reads_position_and_unit_direction_jet() {
    let curves = crate::geometry::a5_guide_curves(&a5_guide_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].sites[0].point, [0.0, 0.0, 0.0]);
    assert_eq!(curves[0].sites[0].direction, [1.0, 0.0, 0.0]);
    assert_eq!(curves[0].sites[1].direction, [0.0, 1.0, 0.0]);
}

#[test]
fn a8_curve_parser_reads_common_form_rolling_ball_jet() {
    let curves = crate::geometry::a8_freeform_curves(&a8_freeform_curve_stream());
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].object_id, 0x1234_5678);
    assert_eq!(curves[0].degree, 5);
    assert_eq!(curves[0].multiplicities, vec![6, 6]);
    assert_eq!(curves[0].sites[1].radius, 2.0);
}

#[test]
fn decode_float_packed_stream_transfers_a8_nurbs() {
    assert_eq!(
        crate::container::scan_bytes(a8_catpart()).variant,
        Variant::FloatPackedInnerNoFbb
    );
    let mut cur = Cursor::new(a8_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Nurbs(_)
    ));
}

#[test]
fn decode_float_packed_stream_transfers_reference_closed_b5_topology() {
    let stream = b5_closed_triangle_stream();
    crate::b5::parse(&stream).expect("generated B5 topology");
    let file = object_main_catpart(&stream);
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::FloatPackedInnerNoFbb
    );

    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 3);
    assert_eq!(result.ir.model.edges.len(), 3);
    assert_eq!(result.ir.model.curves.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.pcurves.len(), 3);
    assert!(result.report.losses.iter().all(|loss| {
        loss.category != cadmpeg_ir::report::LossCategory::Topology
            || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_inner_no_directory_transfers_a8_nurbs() {
    assert_eq!(
        crate::container::scan_bytes(inner_no_directory_a8_catpart()).variant,
        Variant::InnerNoDirectory
    );
    let mut cur = Cursor::new(inner_no_directory_a8_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Nurbs(_)
    ));
}

#[test]
fn decode_inner_no_directory_transfers_b2_cylinder() {
    assert_eq!(
        crate::container::scan_bytes(inner_no_directory_b2_catpart()).variant,
        Variant::InnerNoDirectory
    );
    let mut cur = Cursor::new(inner_no_directory_b2_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Cylinder { radius: 2.0, .. }
    ));
}

#[test]
fn decode_e5_stream_transfers_circle_carrier() {
    let scan = crate::container::scan_bytes(e5_catpart());
    assert_eq!(scan.variant, Variant::E5Stream);
    let mut cur = Cursor::new(e5_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.curves.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 2);
    assert!(result.ir.model.edges.is_empty());
    assert!(result.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Topology
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle { .. }
    ));
    assert!(result.ir.native_unknowns("catia").unwrap()[0]
        .links
        .contains(&"catia:e5:surf#0".to_string()));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_e5_stream_transfers_reference_closed_torus_topology() {
    let stream = e5_torus_topology_stream();
    crate::e5::parse_topology(&stream).expect("generated E5 topology");
    let file = object_main_catpart(&stream);
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::E5Stream
    );

    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 4);
    assert_eq!(result.ir.model.edges.len(), 4);
    assert_eq!(result.ir.model.vertices.len(), 4);
    assert_eq!(result.ir.model.pcurves.len(), 4);
    assert!(result.report.losses.iter().all(|loss| {
        loss.category != cadmpeg_ir::report::LossCategory::Topology
            || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));

    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn container_only_stops_before_geometry() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let opts = DecodeOptions {
        container_only: true,
    };
    let result = CatiaCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result.report.container_only);
    // The reconstructed BREP stream is preserved as an unknown passthrough.
    let unknowns = result.ir.native_unknowns("catia").unwrap();
    assert_eq!(unknowns.len(), 1);
    let retained = &result.source_fidelity.retained_records[0];
    assert_eq!(retained.sha256.len(), 64);
    assert!(retained.data.is_some());
}

#[test]
fn every_decode_path_populates_v1_annotations() {
    let fixtures = [
        standard_catpart(),
        fbb_only_catpart(),
        zero_entity_catpart(),
        zero_entity_cylinder_catpart(),
        e5_catpart(),
        a8_catpart(),
        inner_no_directory_a8_catpart(),
    ];
    for fixture in fixtures {
        let decoded = CatiaCodec
            .decode(&mut Cursor::new(fixture), &DecodeOptions::default())
            .unwrap();
        assert_every_entity_has_v1_annotation(&decoded.ir, &decoded.source_fidelity.annotations);
    }

    let container_only = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart()),
            &DecodeOptions {
                container_only: true,
            },
        )
        .unwrap();
    assert_every_entity_has_v1_annotation(
        &container_only.ir,
        &container_only.source_fidelity.annotations,
    );
}
