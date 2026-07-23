// SPDX-License-Identifier: Apache-2.0
//! Tests over synthetic byte fixtures. No real CAD file exists in this repo and
//! none may be added, so every fixture is a hand-built `.CATPart` byte image
//! whose bytes exercise the real container, variant-detection, and geometry
//! decode paths and fail if the code regresses.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};

use cadmpeg_ir::document::CadIr;

use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SurfaceGeometry};

use cadmpeg_ir::math::{Point3, Vector3};

use cadmpeg_ir::Annotations;

use crate::variant::Variant;

use crate::CatiaCodec;

fn summary_preview_segment() -> Vec<u8> {
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

fn external_reference_segment(target: &str) -> Vec<u8> {
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

pub(crate) fn standard_quad_topology_stream() -> Vec<u8> {
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
fn standard_mesh_ports_bridge_table_local_endpoint_names() {
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
            0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x01, 0x02,
        ],
    );

    let ports =
        crate::solve::missing_edge::standard_mesh_edge_ports(&bytes).expect("mesh port collapse");
    let table_ports = crate::solve::missing_edge::standard_edge_port_identities(&bytes)
        .expect("table-local ports");
    assert_ne!(table_ports[1][1], table_ports[2][0]);
    assert_eq!(
        table_ports
            .iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        8
    );
    assert_eq!(ports[0][1], ports[1][0]);
    assert_eq!(ports[1][1], ports[2][0]);
    assert_eq!(ports[2][1], ports[3][0]);
    assert_eq!(ports[3][1], ports[0][0]);
    assert_eq!(
        ports
            .into_iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
}

#[test]
fn standard_mesh_ports_are_occurrence_components_not_coordinate_indices() {
    let ports =
        crate::solve::missing_edge::standard_mesh_edge_ports(&standard_quad_topology_stream())
            .expect("mesh endpoint components");
    assert_eq!(ports.len(), 4);
    assert_eq!(
        ports
            .into_iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
}

#[test]
fn standard_mesh_coverage_reports_exact_matched_partition() {
    let coverage = crate::solve::missing_edge::standard_mesh_face_coverage(
        &standard_quad_topology_stream(),
        &[[0, 0]; 4],
    )
    .expect("mesh coverage");
    assert_eq!(coverage.len(), 1);
    assert_eq!(coverage[0].face, 0);
    assert!(coverage[0].gaps.is_empty());
    assert!(coverage[0].missing_edges.is_empty());

    let mut bytes = standard_quad_topology_stream();
    let header = bytes
        .windows(3)
        .position(|window| window == [0x01, 0x01, 0x04])
        .expect("edge table header");
    let first_row = header + 3;
    bytes[first_row + 1] = 2;
    bytes.drain(first_row + 4..first_row + 6);
    let coverage = crate::solve::missing_edge::standard_mesh_face_coverage(&bytes, &[[0, 0]; 4])
        .expect("one gap");
    assert_eq!(coverage[0].missing_edges, [0]);
    assert_eq!(coverage[0].gaps.len(), 1);
    assert_eq!(coverage[0].gaps[0].length, 2);
    let placements =
        crate::solve::missing_edge::standard_mesh_missing_edge_placements(&bytes, &[[0, 0]; 4])
            .expect("complete missing-edge placement domain");
    assert_eq!(placements[0].len(), 1);
    assert_eq!(placements[0][0].edge, 0);
    assert_eq!(placements[0][0].segment_count, 2);
    let assignments = crate::solve::missing_edge::standard_mesh_missing_edge_assignments(
        &bytes,
        &[[0, 0]; 4],
        None,
        false,
    )
    .expect("complete missing-edge assignments");
    assert_eq!(assignments[0], [placements[0].clone()]);
    let mut local_ports = bytes.clone();
    let first_row = local_ports
        .windows(2)
        .position(|window| window == [0x02, 0x02])
        .expect("short edge row");
    local_ports[first_row + 2..first_row + 4].copy_from_slice(&200u16.to_be_bytes());
    local_ports[first_row + 4..first_row + 6].copy_from_slice(&201u16.to_be_bytes());
    assert!(
        crate::solve::missing_edge::standard_mesh_missing_edge_assignments(
            &local_ports,
            &[[0, 0]; 4],
            None,
            false
        )
        .is_some()
    );
    let boundaries =
        crate::solve::missing_edge::standard_mesh_boundary_assignments(&bytes, &[[0, 0]; 4], None)
            .expect("complete ordered boundary assignments");
    assert_eq!(boundaries[0].len(), 1);
    assert_eq!(boundaries[0][0].boundaries.len(), 1);
    assert_eq!(
        boundaries[0][0].boundaries[0]
            .iter()
            .map(|use_| (use_.edge, use_.reversed))
            .collect::<Vec<_>>(),
        [
            (0, None),
            (1, Some(false)),
            (2, Some(false)),
            (3, Some(false))
        ]
    );
    let selected = crate::solve::missing_edge::parse_standard_mesh_selection(
        &bytes,
        &[[0, 0]; 4],
        &[0],
        &[vec![vec![false; 4]]],
    )
    .expect("selected mesh-corner quotient");
    assert_eq!(selected.logical_vertex_count(), 4);
    assert_eq!(
        selected.edge_vertices().expect("selected edge vertices"),
        [[0, 1], [1, 2], [2, 3], [3, 0]]
    );
    let (searched, point_assignment) =
        crate::solve::mesh_quotient::parse_standard_mesh_endpoint_candidates(
            &bytes,
            &[[0, 0]; 4],
            &[Vec::new(), vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]],
        )
        .expect("abstract mesh quotient search");
    assert_eq!(searched.logical_vertex_count(), 4);
    assert_eq!(
        searched
            .edge_vertices()
            .expect("searched edge vertices")
            .into_iter()
            .map(|vertices| {
                let mut points = vertices.map(|vertex| point_assignment[vertex]);
                points.sort_unstable();
                points
            })
            .collect::<Vec<_>>(),
        [[0, 1], [1, 2], [2, 3], [0, 3]]
    );
    let cycle_domains = crate::solve::missing_edge::standard_mesh_prune_endpoint_candidates(
        &bytes,
        &[[0, 0]; 4],
        &[
            vec![[0, 1], [0, 2]],
            vec![[1, 2]],
            vec![[2, 3]],
            vec![[3, 0]],
        ],
    )
    .expect("ordered boundary endpoint domains");
    assert_eq!(cycle_domains[0], [[0, 1]]);
    let inferred_cycle_domains =
        crate::solve::missing_edge::standard_mesh_prune_endpoint_candidates(
            &bytes,
            &[[0, 0]; 4],
            &[Vec::new(), vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]],
        )
        .expect("endpoint domain inferred from ordered neighbors");
    assert_eq!(inferred_cycle_domains[0], [[0, 1]]);
    let endpoint_domains = crate::solve::missing_edge::standard_mesh_placement_endpoint_pairs(
        &bytes,
        &[[0, 0]; 4],
        &[None, Some([1, 2]), Some([2, 3]), Some([3, 0])],
    )
    .expect("gap-corner endpoint domains");
    assert_eq!(endpoint_domains[0], [[0, 1]]);
    let endpoint_assignments =
        crate::solve::missing_edge::standard_mesh_missing_edge_endpoint_assignments(
            &bytes,
            &[[0, 0]; 4],
            &[None, Some([1, 2]), Some([2, 3]), Some([3, 0])],
        )
        .expect("correlated gap-corner endpoint assignments");
    assert_eq!(endpoint_assignments[0].len(), 1);
    assert_eq!(endpoint_assignments[0][0].len(), 1);
    assert_eq!(
        endpoint_assignments[0][0][0].endpoint_pairs,
        Some(vec![[0, 1]])
    );
    let pruned =
        crate::solve::missing_edge::standard_mesh_pruned_missing_edge_endpoint_assignments(
            &bytes,
            &[[0, 0]; 4],
            &[Some([1, 0]), Some([1, 2]), Some([2, 3]), Some([3, 0])],
        )
        .expect("endpoint-compatible face assignment");
    assert_eq!(pruned[0][0][0].endpoint_pairs, Some(vec![[0, 1]]));
    assert!(
        crate::solve::missing_edge::standard_mesh_pruned_missing_edge_endpoint_assignments(
            &bytes,
            &[[0, 0]; 4],
            &[Some([0, 2]), Some([1, 2]), Some([2, 3]), Some([3, 0]),],
        )
        .is_none()
    );
}

#[test]
fn unmatched_standard_row_arity_does_not_fix_trim_span() {
    let mut bytes = standard_quad_topology_stream();
    let header = bytes
        .windows(3)
        .position(|window| window == [0x01, 0x01, 0x04])
        .expect("edge table header");
    let first_row = header + 3;
    bytes[first_row + 1] = 4;
    bytes.splice(first_row + 6..first_row + 6, 0x7ffe_u16.to_be_bytes());

    let coverage = crate::solve::missing_edge::standard_mesh_face_coverage(&bytes, &[[0, 0]; 4])
        .expect("unmatched row coverage");
    assert_eq!(coverage[0].missing_edges, [0]);
    assert_eq!(coverage[0].gaps[0].length, 2);
    let assignments = crate::solve::missing_edge::standard_mesh_missing_edge_assignments(
        &bytes,
        &[[0, 0]; 4],
        None,
        false,
    )
    .expect("unmatched curve samples do not determine trim span");
    assert_eq!(assignments[0].len(), 1);
    assert_eq!(assignments[0][0][0].segment_count, 2);
}

#[test]
fn standard_mesh_runs_include_flanking_segments() {
    let runs =
        crate::solve::missing_edge::standard_mesh_edge_runs(&standard_quad_topology_stream())
            .expect("mesh edge runs");
    assert_eq!(runs.len(), 4);
    assert_eq!(
        runs.iter()
            .map(|run| (run.edge, run.start, run.segment_count))
            .collect::<Vec<_>>(),
        vec![(0, 0, 2), (1, 2, 2), (2, 4, 2), (3, 6, 2)]
    );
}

#[test]
fn standard_mesh_gap_assignment_does_not_merge_row_local_endpoint_names() {
    let mut bytes = standard_quad_topology_stream();
    for _ in 0..4 {
        let row = bytes
            .windows(2)
            .position(|window| window == [0x02, 0x03])
            .expect("unmodified edge row");
        bytes[row + 1] = 2;
        bytes.drain(row + 4..row + 6);
    }

    let assignments = crate::solve::missing_edge::standard_mesh_missing_edge_assignments(
        &bytes,
        &[[0, 0]; 4],
        None,
        false,
    )
    .expect("native port-ordered full gap");
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].len(), 840);
    assert!(assignments[0].iter().all(|assignment| {
        assignment
            .iter()
            .map(|placement| placement.edge)
            .collect::<std::collections::HashSet<_>>()
            .len()
            == 4
    }));
    let (topology, points) = crate::solve::mesh_quotient::parse_standard_mesh_endpoint_candidates(
        &bytes,
        &[[0, 0]; 4],
        &[vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]],
    )
    .expect("endpoint-constrained full gap");
    assert_eq!(topology.logical_vertex_count(), 4);
    assert_eq!(points, [0, 1, 2, 3]);
}

#[test]
fn standard_mesh_endpoint_domains_ignore_row_local_endpoint_order() {
    let mut bytes = standard_quad_topology_stream();
    let header = bytes
        .windows(3)
        .position(|window| window == [0x01, 0x01, 0x04])
        .expect("edge table header");
    let first_row = header + 3;
    let start = bytes[first_row + 2..first_row + 4].to_vec();
    let end = bytes[first_row + 6..first_row + 8].to_vec();
    bytes[first_row + 2..first_row + 4].copy_from_slice(&end);
    bytes[first_row + 6..first_row + 8].copy_from_slice(&start);

    let (topology, _) = crate::solve::mesh_quotient::parse_standard_mesh_endpoint_candidates(
        &bytes,
        &[[0, 0]; 4],
        &[vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]],
    )
    .expect("independent endpoint-port gauge");
    let coedges = &topology.faces()[0].boundaries[0].coedges;
    assert!(coedges.iter().all(|coedge| !coedge.reversed));
}

pub(crate) const OUTER_MAGIC: &[u8; 8] = b"V5_CFV2\0";

const DIR_MAGIC: &[u8; 16] = b"CATIA_V5 CB0001\0";

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

pub(crate) fn le_f32(v: f32) -> [u8; 4] {
    v.to_le_bytes()
}

fn be_f32(v: f32) -> [u8; 4] {
    v.to_be_bytes()
}

pub(crate) fn le_f64(v: f64) -> [u8; 8] {
    v.to_le_bytes()
}

/// A `MainDataStream` physical payload: two FBB spine rows, two empty standard
/// edge tables, and a counted table of three `05 08 01` vertex records.
fn main_stream() -> Vec<u8> {
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

fn outer_directory_catpart() -> Vec<u8> {
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

pub(crate) fn e5_circle_stream() -> Vec<u8> {
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

pub(crate) fn e5_torus_stream() -> Vec<u8> {
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

pub(crate) fn e5_plane_stream() -> Vec<u8> {
    e5_plane_stream_with_transform_scalars(4)
}

pub(crate) fn e5_plane_stream_with_transform_scalars(scalar_count: usize) -> Vec<u8> {
    let mut payload = vec![0u8; 58 + 8 * scalar_count];
    for (index, value) in [1.0f64, 2.0, 3.0].into_iter().enumerate() {
        payload[1 + 8 * index..9 + 8 * index].copy_from_slice(&le_f64(value));
    }
    payload[25] = 0x33;
    for index in 0..scalar_count {
        payload[26 + 8 * index..34 + 8 * index].copy_from_slice(&le_f64(1.0));
    }
    for (index, value) in [-4.0f64, 7.0, -2.0, 9.0].into_iter().enumerate() {
        let at = 26 + 8 * scalar_count + 8 * index;
        payload[at..at + 8].copy_from_slice(&le_f64(value));
    }
    let mut bytes = Vec::new();
    append_e5_record(&mut bytes, 0xc8, 42, &payload);
    bytes
}

pub(crate) fn a8_surface_stream() -> Vec<u8> {
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

pub(crate) fn a8_elided_surface_stream() -> Vec<u8> {
    let mut bytes = a8_surface_stream();
    bytes.truncate(59);
    let mut tail = vec![0; 141];
    tail[..4].copy_from_slice(&[0x05, 0x21, 0x05, 0x05]);
    bytes.extend_from_slice(&tail);
    let payload_len = u32::try_from(bytes.len() - 11).unwrap();
    bytes[3..7].copy_from_slice(&payload_len.to_le_bytes());

    let mut pcurve_payload = vec![0; 58];
    pcurve_payload[0] = 0x81;
    pcurve_payload[57] = 0x07;
    bytes.extend_from_slice(&[0xb5, 0x03, 0x21, 58, 1, 0, 0, 0]);
    bytes.extend_from_slice(&pcurve_payload);
    for point in 0..9 {
        for coordinate in [f64::from(point), f64::from(point % 3), 2.0] {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0, 2, 0, 0, 0]);
    bytes
}

pub(crate) fn a8_rational_surface_stream() -> Vec<u8> {
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

pub(crate) fn a8_pcurve_stream() -> Vec<u8> {
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

pub(crate) fn a5_pcurve_stream() -> Vec<u8> {
    a5_pcurve_stream_with_uv([0.0, 1.0], [0.0, 1.0])
}

pub(crate) fn a6_pcurve_stream() -> Vec<u8> {
    let narrow = a5_pcurve_stream();
    let mut wide = vec![0xa6, 0x03, 0x20];
    wide.extend_from_slice(&narrow[3..7]);
    wide.extend_from_slice(&[0x05, 0x00]);
    wide.extend_from_slice(&narrow[8..]);
    wide
}

pub(crate) fn b2_pcurve_stream() -> Vec<u8> {
    let narrow = a5_pcurve_stream();
    let payload = &narrow[8..];
    let mut record = vec![0xb2, 0x03, 0x20, u8::try_from(payload.len()).unwrap(), 0x05];
    record.extend_from_slice(payload);
    record
}

pub(crate) fn b2_parameter_point_stream() -> Vec<u8> {
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

pub(crate) fn b2_reference_list_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x37, 0x22, 0x05];
    for value in 0u8..26 {
        record.push(4 * value + 1);
    }
    record.extend_from_slice(&le_f64(1.0));
    record
}

pub(crate) fn b2_owner_packet_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x62, 0x52, 0x05, 0x89];
    for (index, value) in [1000u16, 1, 1001, 2, 1002, 3, 1003, 4, 1004]
        .into_iter()
        .enumerate()
    {
        if index % 2 == 0 {
            record.push(0x0a);
            record.extend_from_slice(&value.to_le_bytes());
        } else {
            record.push(4 * u8::try_from(value).unwrap() + 1);
        }
    }
    record.extend(0u8..62);
    record
}

pub(crate) fn b2_width_coded_owner_packet_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x62, 0x50, 0x05, 0x89];
    for (index, value) in [216u16, 3, 540, 7, 223, 19, 545, 31, 606]
        .into_iter()
        .enumerate()
    {
        if index % 2 == 0 {
            if u8::try_from(value).is_ok() {
                record.extend_from_slice(&[0x04, u8::try_from(value).unwrap()]);
            } else {
                record.push(0x08);
                record.extend_from_slice(&value.to_le_bytes());
            }
        } else {
            record.push(u8::try_from(value).unwrap());
        }
    }
    record.extend(0u8..62);
    record
}

pub(crate) fn b2_counted_61_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x61, 0x0c, 0x05, 0x84, 0x08, 0x14, 0x05, 0x08, 0x0e, 0x05, 0x79, 0x04, 0x4a,
        0x41, 0x03,
    ]
}

pub(crate) fn b2_long_61_stream() -> Vec<u8> {
    let mut payload = vec![0xb5, 0x03, 0x2b, 0x47, 0x8f, 0xb3, 0xd7, 0xfb, 0x06];
    for member in [0x064a_u16, 0x0650, 0x0656] {
        payload.extend_from_slice(&member.to_le_bytes());
    }
    payload.push(0xfe);
    for reference in [0x0100_u16, 0x0103, 0x0106, 0x0109, 0x010c] {
        payload.push(0x0a);
        payload.extend_from_slice(&reference.to_le_bytes());
    }
    payload.extend_from_slice(&le_f64(42.5));
    payload.push(0x03);
    let mut record = vec![0xb2, 0x03, 0x61, u8::try_from(payload.len()).unwrap(), 0x05];
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn b2_link_5f_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x08, 0x5d, 0x02, 0x03, 0x05,
    ]
}

pub(crate) fn b2_linked_owner_stream() -> Vec<u8> {
    let mut bytes = vec![
        0xb2, 0x03, 0x5f, 0x06, 0x05, 0x82, 0x08, 0xeb, 0x03, 0x03, 0x05,
    ];
    bytes.extend_from_slice(&b2_owner_packet_stream());
    bytes
}

pub(crate) fn b2_linked_counted_owner_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x5f, 0x06, 0x11, 0x82, 0x08, 0x94, 0x03, 0x03, 0x05, 0xb2, 0x03, 0x62, 0x19,
        0x05, 0x87, 0x08, 0x8f, 0x03, 0x1d, 0x08, 0x07, 0x01, 0x08, 0x02, 0x01, 0x08, 0x19, 0x01,
        0x08, 0x14, 0x01, 0x08, 0x95, 0x03, 0x83, 0x41, 0x92, 0x00, 0x01,
    ]
}

pub(crate) fn b2_cone_face_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x3b, 0x20, 0x05];
    for value in 0u8..16 {
        record.push(4 * value + 1);
    }
    record.extend_from_slice(&le_f64(1.5));
    record.extend_from_slice(&le_f64(std::f64::consts::FRAC_PI_4));
    record
}

pub(crate) fn b2_topology_metadata_stream() -> Vec<u8> {
    let mut bytes = vec![
        0xb2, 0x03, 0x5e, 0x07, 0x05, 0x0a, 0x34, 0x12, 0x0a, 0x78, 0x56, 0,
    ];
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 1, 2, 3, 0x88]);
    bytes
}

pub(crate) fn b2_edge_node_stream() -> Vec<u8> {
    vec![
        0xb2, 0x03, 0x5e, 0x0d, 0x05, 0x04, 0xd8, 0x08, 0x79, 0x03, 0x08, 0x7f, 0x03, 0x04, 0xd7,
        0x04, 0xd6, 0x21,
    ]
}

pub(crate) fn b2_revolution_stream() -> Vec<u8> {
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

pub(crate) fn b2_group_stream() -> Vec<u8> {
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
    payload.push(0x07);
    let mut record = vec![0xa5, 0x03, 0x20];
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.push(0x05);
    record.extend_from_slice(&payload);
    record
}

pub(crate) fn a5_circle_bound_edge_stream() -> Vec<u8> {
    let radius = 3.0;
    let arc = [0.0, 2.0 * std::f64::consts::PI * radius];
    let mut bytes = a5_pcurve_stream_with_uv(arc, [2.0, 2.0]);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(arc, [2.0, 2.0]));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&b2_circle_stream());
    bytes
}

pub(crate) fn a5_cone_bound_edge_stream() -> Vec<u8> {
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

pub(crate) fn b2_offset_support_stream() -> Vec<u8> {
    b2_offset_support_stream_for([0.0, -1.0, 4.0, 3.0])
}

fn b2_offset_support_stream_for(domain: [f64; 4]) -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x31, 0x2b, 0x05, 0x08, 0x34, 0x12];
    for value in [2.5f64, domain[0], domain[1], domain[2], domain[3]] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b3_offset_support_stream() -> Vec<u8> {
    let narrow = b2_offset_support_stream();
    let mut wide = vec![0xb3, 0x03, 0x31, narrow[3], 0x05, 0x00];
    wide.extend_from_slice(&narrow[5..]);
    wide
}

pub(crate) fn b2_edge_parameter_stream() -> Vec<u8> {
    b2_edge_parameter_stream_for(2.0, 7.0)
}

pub(crate) fn b2_edge_parameter_stream_for(lo: f64, hi: f64) -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x23, 0x4e, 0];
    record.extend_from_slice(&[0; 6]);
    for value in [lo, hi, 1e-6, lo, hi, 1.0, lo, hi, 1e-6] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn a5_edge_block_stream() -> Vec<u8> {
    let mut bytes = a5_pcurve_stream();
    bytes.extend_from_slice(&a5_pcurve_stream());
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes
}

pub(crate) fn b2_edge_block_stream() -> Vec<u8> {
    fn b_family_pcurve() -> Vec<u8> {
        let a_family = a5_pcurve_stream();
        let payload = &a_family[8..];
        let mut record = vec![
            0xb2,
            0x03,
            0x20,
            u8::try_from(payload.len()).unwrap(),
            a_family[7],
        ];
        record.extend_from_slice(payload);
        record
    }

    let mut bytes = b_family_pcurve();
    bytes.extend_from_slice(&b_family_pcurve());
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes
}

fn a5_topology_edge_run_stream() -> Vec<u8> {
    let mut bytes = a5_edge_block_stream();
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 5, 9, 0x84]);
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 9, 13, 0x88]);
    bytes.extend_from_slice(&b2_edge_node_stream());
    bytes
}

pub(crate) fn b2_topology_edge_run_stream() -> Vec<u8> {
    let mut bytes = b2_edge_block_stream();
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 5, 9, 0x84]);
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 9, 13, 0x88]);
    bytes.extend_from_slice(&b2_edge_node_stream());
    bytes
}

pub(crate) fn a5_native_edge_run_stream(curve: u8, start: u8, end: u8) -> Vec<u8> {
    assert!(curve >= 3);
    let mut bytes = a5_edge_block_stream();
    bytes.extend_from_slice(&a5_native_edge_identity_stream(curve, start, end));
    bytes
}

fn a5_native_edge_identity_stream(curve: u8, start: u8, end: u8) -> Vec<u8> {
    assert!(curve >= 3);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[
        0xb2,
        0x03,
        0x06,
        0x04,
        0x05,
        0x82,
        4 * (curve - 2) + 1,
        4 * (curve - 1) + 1,
        0x88,
    ]);
    bytes.extend_from_slice(&[
        0xb2,
        0x03,
        0x06,
        0x04,
        0x05,
        0x82,
        4 * (curve - 1) + 1,
        4 * curve + 1,
        0x84,
    ]);
    let mut payload = vec![4 * curve + 1, 0x06, start, 0x06, end, 9, 5, 0x21];
    bytes.extend_from_slice(&[0xb2, 0x03, 0x5e, u8::try_from(payload.len()).unwrap(), 0x05]);
    bytes.append(&mut payload);
    bytes
}

pub(crate) fn a5_cylinder_bound_edge_stream() -> Vec<u8> {
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

pub(crate) fn a5_nurbs_bound_edge_stream(offset: f64) -> Vec<u8> {
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
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
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

pub(crate) fn b2_circle_stream() -> Vec<u8> {
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

pub(crate) fn b2_cylinder_stream() -> Vec<u8> {
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

pub(crate) fn b3_cylinder_stream() -> Vec<u8> {
    let narrow = b2_cylinder_stream();
    let mut wide = vec![0xb3, 0x03, 0x28, 0x5a, 0x05, 0x00];
    wide.extend_from_slice(&narrow[5..]);
    wide
}

pub(crate) fn b2_implicit_axis_cylinder_stream() -> Vec<u8> {
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

pub(crate) fn b2_phase_tailed_cylinder_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x28, 0x62, 0x05];
    record.resize(103, 0);
    let p = 5;
    record[p + 24] = 0x0e;
    record[p + 25..p + 33].copy_from_slice(&le_f64(0.0));
    record[p + 33..p + 41].copy_from_slice(&le_f64(1.0));
    record[p + 41..p + 49].copy_from_slice(&le_f64(1.0));
    record[p + 49..p + 57].copy_from_slice(&le_f64(4.0));
    record[p + 57..p + 65].copy_from_slice(&le_f64(0.0));
    record[p + 65..p + 73].copy_from_slice(&le_f64(8.0));
    record[p + 73..p + 81].copy_from_slice(&le_f64(-2.0));
    record[p + 81..p + 89].copy_from_slice(&le_f64(2.0));
    record[p + 89] = 0x03;
    record[p + 90..p + 98].copy_from_slice(&le_f64(0.75));
    record
}

pub(crate) fn b2_cone_stream() -> Vec<u8> {
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

pub(crate) fn b2_construction_use_stream() -> Vec<u8> {
    b2_construction_use_stream_for([0.0, -1.0, 4.0, 3.0])
}

fn b2_construction_use_stream_for(domain: [f64; 4]) -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x30, 0x2d, 0x05, 0x05, 0x08, 0x34, 0x12];
    record.extend_from_slice(&le_f64(-2.0));
    record.push(0x01);
    for value in [domain[0], domain[2], domain[1], domain[3]] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b2_embedded_cylinder_stream() -> Vec<u8> {
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

fn standard_catpart_with_nested_design_objects() -> Vec<u8> {
    let graph = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x83, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x83, 0x84], &[0xfe]),
    ]);
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
        "GSMLoft",
        "GSMPointBetweenValues",
        "GSMPlaneAngle",
    ]);
    let mut file = standard_catpart();
    file.splice(16..16, catalog);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_value_block() -> Vec<u8> {
    let mut stream = object_graph_stream();
    stream.extend(value_block_stream(&[
        0x81, 0x83, 0x32, 4, 0, 0, 0, 0x83, 0x82,
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "VPGlobal",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_visualization_values_only() -> Vec<u8> {
    let mut stream = value_block_stream(&[0x32, 4, 0, 0, 0, 0x83]);
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "VPGlobal",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_design_class(class: &str) -> Vec<u8> {
    let mut stream = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    stream.extend(value_block_stream(&[0x81]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "CurrentFeature",
        class,
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

pub(crate) fn a5_surface_stream() -> Vec<u8> {
    a5_surface_stream_with_poles([
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [2.0, 1.0, 0.0],
        [3.0, 1.0, 1.0],
    ])
}

pub(crate) fn a6_surface_stream() -> Vec<u8> {
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

pub(crate) fn a5_rational_surface_stream() -> Vec<u8> {
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

pub(crate) fn a5_freeform_curve_stream() -> Vec<u8> {
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

pub(crate) fn a6_freeform_curve_stream() -> Vec<u8> {
    let narrow = a5_freeform_curve_stream();
    let mut wide = vec![0xa6, 0x03, 0x32];
    wide.extend_from_slice(&narrow[3..7]);
    wide.extend_from_slice(&[0x05, 0x00]);
    wide.extend_from_slice(&narrow[8..]);
    wide
}

pub(crate) fn a5_guide_curve_stream() -> Vec<u8> {
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

pub(crate) fn a8_freeform_curve_stream() -> Vec<u8> {
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
    payload.extend_from_slice(&[0; 59]);
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
fn summary_preview_parser_extracts_exact_jpeg_and_dimensions() {
    let bytes = summary_preview_segment();
    let segments = crate::container::finjpl_segments(&bytes, 0, bytes.len());
    assert_eq!(segments[0].name.as_deref(), Some("CATSummaryInformation"));
    let previews = crate::container::preview_images(&bytes);
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].width, 640);
    assert_eq!(previews[0].height, 288);
    assert_eq!(previews[0].components, 1);
    assert_eq!(&bytes[previews[0].range.clone()][..2], [0xff, 0xd8]);
    assert_eq!(
        &bytes[previews[0].range.clone()][previews[0].range.len() - 2..],
        [0xff, 0xd9]
    );
    let summary = crate::container::summarize(&crate::container::scan_bytes(bytes.clone()));
    assert!(summary.entries.iter().any(|entry| {
        entry.role == crate::container::role::FINJPL_SEGMENT
            && entry.name == "CATSummaryInformation"
    }));

    let mut truncated = bytes;
    let eoi = truncated
        .windows(2)
        .position(|value| value == [0xff, 0xd9])
        .unwrap();
    truncated.truncate(eoi + 1);
    assert!(crate::container::preview_images(&truncated).is_empty());
}

#[test]
fn summary_version_parser_requires_one_consistent_tuple() {
    let bytes = summary_preview_segment();
    let version = crate::container::last_save_version(&bytes).unwrap();
    assert_eq!(version.version, 5);
    assert_eq!(version.release, 27);
    assert_eq!(version.service_pack, 2);
    assert_eq!(version.hot_fix, 0);
    assert_eq!(version.build_date, "03-10-2017.22.00");

    let mut conflicting = bytes;
    let mut other = summary_preview_segment();
    let release = other
        .windows(11)
        .position(|value| value == b"<Release>27")
        .unwrap();
    other[release + 9] = b'2';
    other[release + 10] = b'8';
    conflicting.extend_from_slice(&other);
    assert!(crate::container::last_save_version(&conflicting).is_none());

    let mut non_summary = summary_preview_segment();
    non_summary[8..12].copy_from_slice(&0x0101_0002u32.to_be_bytes());
    assert!(crate::container::last_save_version(&non_summary).is_none());
    assert!(crate::container::preview_images(&non_summary).is_empty());
    let native = crate::native::CatiaNative::decode(&non_summary);
    assert!(native.preview_images.is_empty());
}

#[test]
fn storage_property_parser_enumerates_external_catia_documents() {
    let mut bytes = external_reference_segment("Support.CATPart");
    bytes.extend_from_slice(&external_reference_segment("Assembly.CATProduct"));
    bytes.extend_from_slice(&external_reference_segment("notes.txt"));
    let references = crate::container::external_references(&bytes);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].target, "Support.CATPart");
    assert_eq!(references[1].target, "Assembly.CATProduct");

    let scan = crate::container::scan_bytes(bytes.clone());
    let summary = crate::container::summarize(&scan);
    assert_eq!(
        summary
            .entries
            .iter()
            .filter(|entry| entry.role == crate::container::role::EXTERNAL_REFERENCE)
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["Support.CATPart", "Assembly.CATProduct"]
    );

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.version, crate::native::CATIA_NATIVE_VERSION);
    assert_eq!(native.external_references.len(), 2);
    assert_eq!(native.external_references[0].target, "Support.CATPart");
    assert_eq!(
        native.external_references[0].segment,
        native.finjpl_segments[0].id
    );
    assert_eq!(
        native.external_references[1].segment,
        native.finjpl_segments[1].id
    );
    for reference in &native.external_references {
        let segment = native
            .finjpl_segments
            .iter()
            .find(|segment| segment.id == reference.segment)
            .expect("external-reference segment");
        assert!(reference.byte_offset >= segment.byte_offset);
        assert!(reference.byte_offset < segment.byte_offset + segment.byte_len);
    }
}

#[test]
fn decode_persists_external_references_in_native_namespace() {
    let mut file = standard_catpart();
    file.extend_from_slice(&external_reference_segment("Support.CATPart"));
    let file_len = u32::try_from(file.len()).expect("external-reference fixture length");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode external-reference fixture");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load CATIA native namespace");
    let [reference] = native.external_references.as_slice() else {
        panic!("one external reference");
    };
    assert_eq!(reference.target, "Support.CATPart");
    assert!(native
        .finjpl_segments
        .iter()
        .any(|segment| segment.id == reference.segment));
}

#[test]
fn native_namespace_retains_summary_preview_bytes() {
    let bytes = summary_preview_segment();
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.preview_images.len(), 1);
    let preview = &native.preview_images[0];
    assert_eq!(
        (preview.width, preview.height, preview.components),
        (640, 288, 1)
    );
    assert_eq!(preview.data.len() as u64, preview.byte_len);
    assert_eq!(&preview.data[..2], [0xff, 0xd8]);
    assert_eq!(&preview.data[preview.data.len() - 2..], [0xff, 0xd9]);
    assert_eq!(native.finjpl_segments.len(), 1);
    assert_eq!(
        native.finjpl_segments[0].name.as_deref(),
        Some("CATSummaryInformation")
    );
    assert_eq!(native.finjpl_segments[0].family, "project-flags");
    assert_eq!(native.finjpl_segments[0].data, bytes);
}

#[test]
fn summary_preview_requires_a_coherent_frame_header() {
    let valid = summary_preview_segment();
    let frame = valid
        .windows(2)
        .position(|bytes| bytes == [0xff, 0xc0])
        .expect("fixture SOF marker");

    let mut zero_height = valid.clone();
    zero_height[frame + 5..frame + 7].copy_from_slice(&0u16.to_be_bytes());
    assert!(crate::container::preview_images(&zero_height).is_empty());

    let mut inconsistent_components = valid;
    inconsistent_components[frame + 9] = 2;
    assert!(crate::container::preview_images(&inconsistent_components).is_empty());
    assert!(crate::native::CatiaNative::decode(&inconsistent_components)
        .preview_images
        .is_empty());
}

#[test]
fn summary_preview_requires_one_complete_jpeg_candidate() {
    let valid = summary_preview_segment();
    let image_start = valid
        .windows(3)
        .position(|bytes| bytes == [0xff, 0xd8, 0xff])
        .expect("fixture JPEG SOI");

    let mut malformed_prefix = valid.clone();
    malformed_prefix.splice(image_start..image_start, [0xff, 0xd8, 0xff, 0xd9]);
    let previews = crate::container::preview_images(&malformed_prefix);
    let [preview] = previews.as_slice() else {
        panic!("one complete preview after malformed SOI")
    };
    assert_eq!(&malformed_prefix[preview.range.clone()][..2], [0xff, 0xd8]);

    let image_end = valid
        .windows(2)
        .enumerate()
        .skip(image_start)
        .find_map(|(at, bytes)| (bytes == [0xff, 0xd9]).then_some(at + 2))
        .expect("fixture JPEG EOI");
    let image = valid[image_start..image_end].to_vec();
    let mut duplicate = valid;
    duplicate.extend(image);
    assert!(crate::container::preview_images(&duplicate).is_empty());
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
fn standard_decode_retains_native_surface_carrier_tags() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart()),
            &DecodeOptions::default(),
        )
        .expect("standard decode");
    let identities = decoded
        .ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            surface
                .source_object
                .as_ref()
                .map(|source| (source.format.as_str(), source.object_id.as_str()))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        identities,
        [
            ("catia", "cgm-carrier:ccbbaa"),
            ("catia", "cgm-carrier:332211"),
        ]
    );
}

#[test]
fn standard_decode_retains_vertex_allocation_tags() {
    let mut surf = surf_stream();
    for identity in [0x01_0203u32, 0x01_0206, 0x01_0209] {
        surf.push(0x54);
        surf.extend_from_slice(&identity.to_le_bytes()[..3]);
        surf.extend_from_slice(&[0, 0, 0]);
    }
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_from_streams(&main_stream(), &surf)),
            &DecodeOptions::default(),
        )
        .expect("standard decode");
    let identities = decoded
        .ir
        .model
        .points
        .iter()
        .map(|point| {
            point
                .source_object
                .as_ref()
                .map(|source| (source.format.as_str(), source.object_id.as_str()))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        identities,
        [
            Some(("catia", "cgm-vertex:010203")),
            Some(("catia", "cgm-vertex:010206")),
            Some(("catia", "cgm-vertex:010209")),
        ]
    );
}

#[test]
fn scan_parses_outer_directory_with_absolute_extents() {
    let bytes = outer_directory_catpart();
    let scan = crate::container::scan_bytes(bytes.clone());
    let outer = scan.outer.as_ref().expect("outer directory");
    assert_eq!(outer.inner, 0);
    assert_eq!(outer.descriptors.len(), 1);
    let descriptor = &outer.descriptors[0];
    assert_eq!(descriptor.name, "RootStorage");
    assert_eq!(
        crate::container::reconstruct_logical_stream(&bytes, descriptor, outer.inner),
        b"outer logical stream"
    );

    let summary = crate::container::summarize(&scan);
    let entry = summary
        .entries
        .iter()
        .find(|entry| entry.name == "RootStorage")
        .expect("outer stream summary");
    assert_eq!(entry.attributes["directory"], "outer");
}

#[test]
fn inspect_enumerates_streams_and_names_variant() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let summary = CatiaCodec
        .inspect(&mut cur, &cadmpeg_ir::decode::InspectOptions::default())
        .unwrap();
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
                && (u_axis.x * u_axis.x + u_axis.y * u_axis.y + u_axis.z * u_axis.z - 1.0).abs() < 1e-6
                && (u_axis.x * normal.x + u_axis.y * normal.y + u_axis.z * normal.z).abs() < 1e-6
    )));

    // Stored face/carrier rows do not establish a B-rep without a complete
    // trim and edge graph. Carriers remain free and vertices receive only the
    // neutral ownership required for a disconnected point set.
    assert!(result.ir.model.faces.is_empty());
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(result.ir.model.shells[0].free_vertices.len(), 3);
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
fn decode_standard_retains_unresolved_roster_carrier_without_fabricating_a_face() {
    let mut surf = surf_stream();
    let bridge = [0xff, 0x11, 0x22, 0x33, 0x00, 0x02, 0x00, 0x33, 0x32];
    let bridge_start = surf
        .windows(bridge.len())
        .position(|bytes| bytes == bridge)
        .expect("plane parameter bridge");
    surf.drain(bridge_start..bridge_start + bridge.len() + 40);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_from_streams(&main_stream(), &surf)),
            &DecodeOptions::default(),
        )
        .expect("decode unresolved roster carrier");

    assert_eq!(decoded.ir.model.surfaces.len(), 2);
    assert!(decoded.ir.model.faces.is_empty());
    assert!(matches!(
        decoded.ir.model.surfaces[1].geometry,
        SurfaceGeometry::Unknown { record: Some(_) }
    ));
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("1 unresolved surface carriers")
    }));
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
    assert_eq!(
        decoded
            .ir
            .model
            .curves
            .iter()
            .map(|curve| curve
                .source_object
                .as_ref()
                .map(|source| source.object_id.as_str()))
            .collect::<Vec<_>>(),
        (1..=6)
            .map(|tag| format!("cgm-edge-support:{tag:06x}"))
            .collect::<Vec<_>>()
            .iter()
            .map(|object_id| Some(object_id.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(!decoded.report.losses.iter().any(|loss| {
        matches!(
            loss.category,
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
}

#[test]
fn decode_fbb_only_without_parseable_counted_table_transfers_only_carriers() {
    assert_eq!(
        crate::container::scan_bytes(fbb_only_catpart()).variant,
        Variant::FbbOnly
    );
    let mut cur = Cursor::new(fbb_only_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.points.is_empty());
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
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(
        result.ir.model.shells[0].free_vertices,
        [result.ir.model.vertices[0].id.clone()]
    );
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
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
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

pub(crate) fn append_e5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(&[0xe5, 0x0d, 0x03, class, 0]);
    bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

pub(crate) fn e5_uv_line_payload(surface: u16, offset: f64) -> Vec<u8> {
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
        [5.0 * std::f64::consts::PI, std::f64::consts::FRAC_PI_2],
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
        [
            0.0,
            10.0 + std::f32::consts::SQRT_2,
            std::f32::consts::SQRT_2,
        ],
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

pub(crate) fn zero_entity_record(kind: u8, mut tail: Vec<u8>) -> Vec<u8> {
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

pub(crate) fn append_b5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

pub(crate) fn b5_linear_pcurve_payload(surface: u16, start: [f64; 2], end: [f64; 2]) -> Vec<u8> {
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

pub(crate) fn b5_analytic_line_pcurve_payload(
    surface: u16,
    origin: [f64; 2],
    direction: [f64; 2],
    interval: [f64; 2],
) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.push(0x01);
    for value in [
        origin[0],
        origin[1],
        direction[0],
        direction[1],
        interval[0],
        interval[1],
    ] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload
}

pub(crate) fn b5_isoparametric_line_pcurve_payload(
    surface: u16,
    constant_u: f64,
    interval_v: [f64; 2],
) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.push(0x05);
    for value in [constant_u, interval_v[0], interval_v[1]] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload
}

pub(crate) fn b5_transverse_isoparametric_line_pcurve_payload(
    surface: u16,
    constant_v: f64,
    interval_u: [f64; 2],
) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.push(0x09);
    for value in [constant_v, interval_u[0], interval_u[1]] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload
}

pub(crate) fn b5_closed_triangle_stream() -> Vec<u8> {
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
            0x18, 100, 0, 0x83, 0x05, 0x05,
        ],
    );
    append_b5_record(
        &mut bytes,
        0x5f,
        500,
        &[0x82, 0x18, 100, 0, 0x18, 144, 1, 0x05],
    );
    for point in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&le_f32(value));
        }
    }
    bytes
}

#[test]
fn decode_geometry_fallback_transfers_an_external_a8_pole_grid() {
    let file = object_main_catpart(&a8_elided_surface_stream());
    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let SurfaceGeometry::Nurbs(surface) = &result.ir.model.surfaces[0].geometry else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.control_points.len(), 9);
    assert_eq!(surface.control_points[8], Point3::new(8.0, 2.0, 2.0));
}

#[test]
fn decode_object_stream_does_not_promote_unbound_a8_pcurve() {
    let file = object_main_catpart(&a8_pcurve_stream());
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode unbound object-stream pcurve");
    assert!(decoded.ir.model.pcurves.is_empty());
    assert!(!decoded.ir.native_unknowns("catia").unwrap().is_empty());
}

#[test]
fn decode_standard_does_not_promote_unbound_consolidated_pcurve() {
    let mut file = standard_catpart();
    file.splice(16..16, a5_pcurve_stream());
    let file_len = u32::try_from(file.len()).expect("pcurve fixture length");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode consolidated pcurve");
    assert!(decoded.ir.model.pcurves.is_empty());
    assert!(!decoded.ir.native_unknowns("catia").unwrap().is_empty());
}

#[test]
fn native_namespace_retains_unbound_consolidated_pcurve_jets() {
    let mut bytes = Vec::new();
    for _ in 0..6 {
        bytes.extend(a5_pcurve_stream());
        bytes.extend(b2_pcurve_stream());
    }
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_pcurves.len(), 12);
    assert_eq!(
        native.consolidated_pcurves[0].family,
        crate::native::CatiaConsolidatedFamily::A
    );
    assert_eq!(
        native.consolidated_pcurves[1].family,
        crate::native::CatiaConsolidatedFamily::B
    );
    assert_eq!(native.consolidated_pcurves[0].support_id, 0x1234);
    assert_eq!(
        native.consolidated_pcurves[0].points,
        vec![[0.0, 0.0], [1.0, 1.0]]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA pcurves");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA pcurves"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_pcurves[0].degree = 4;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA pcurve for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_historical_edge_runs() {
    let bytes = a5_native_edge_run_stream(6, 139, 142);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_pcurves.len(), 2);
    assert_eq!(native.consolidated_edge_runs.len(), 1);
    let run = &native.consolidated_edge_runs[0];
    assert_eq!(
        run.pcurves,
        ["catia:consolidated:pcurve#0", "catia:consolidated:pcurve#1"]
    );
    assert_eq!(run.node, "catia:consolidated:edge-node#0");
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one consolidated edge node");
    };
    assert_eq!(node.vertex_refs, [139, 142]);
    assert_eq!(
        node.vertices,
        [
            "catia:consolidated:vertex-identity#0",
            "catia:consolidated:vertex-identity#1"
        ]
    );
    assert_eq!(node.parameter_selectors, [2, 1]);
    let uses = node.uses.as_ref().expect("edge-owned oriented uses");
    assert_eq!(uses.references, [[4, 5], [5, 6]]);
    assert_eq!(uses.senses, [0x88, 0x84]);
    let definition = node.definition.as_ref().expect("edge-owned definition");
    assert_eq!(definition.class, 0x23);
    assert!(definition.byte_offset < node.byte_offset);
    assert_eq!(native.consolidated_vertex_identities.len(), 2);
    assert_eq!(native.consolidated_vertex_identities[0].identity, 139);
    assert_eq!(
        native.consolidated_vertex_identities[0].incident_edge_nodes,
        ["catia:consolidated:edge-node#0"]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA edge run"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_edge_runs[0].pcurves[1] = "missing".to_string();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA edge run for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_edge_nodes[0]
        .definition
        .as_mut()
        .expect("edge definition")
        .class = 0x26;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA edge definition");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_edge_nodes[0].uses = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store orphaned CATIA edge definition");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = crate::native::CatiaNative::decode(&bytes);
    invalid.consolidated_vertex_identities[0]
        .incident_edge_nodes
        .clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA vertex incidence for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_merges_shared_consolidated_vertex_identity() {
    let mut bytes = a5_native_edge_run_stream(6, 139, 142);
    bytes.extend_from_slice(&a5_native_edge_run_stream(9, 142, 151));
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.consolidated_edge_runs.len(), 2);
    assert_eq!(native.consolidated_vertex_identities.len(), 3);
    let shared = native
        .consolidated_vertex_identities
        .iter()
        .find(|vertex| vertex.identity == 142)
        .expect("shared consolidated vertex identity");
    assert_eq!(
        shared.incident_edge_nodes,
        [
            "catia:consolidated:edge-node#0",
            "catia:consolidated:edge-node#1"
        ]
    );
    assert_eq!(
        native.consolidated_edge_nodes[0].vertices[1],
        native.consolidated_edge_nodes[1].vertices[0]
    );
}

#[test]
fn native_namespace_retains_standalone_consolidated_edge_nodes() {
    let bytes = b2_edge_node_stream();
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.consolidated_edge_runs.is_empty());
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one standalone consolidated edge node");
    };
    assert_eq!(node.width, 1);
    assert_eq!(node.flag, 0x03);
    assert_eq!(node.header_token, 5);
    assert_eq!(node.vertex_refs, [889, 895]);
    assert!(node.uses.is_none());
    assert_eq!(native.consolidated_vertex_identities.len(), 2);
    assert_eq!(
        native.consolidated_vertex_identities[0].incident_edge_nodes,
        ["catia:consolidated:edge-node#0"]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store standalone consolidated edge node");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load standalone consolidated edge node"),
        native
    );
}

#[test]
fn native_namespace_attaches_oriented_uses_without_pcurves() {
    let bytes = a5_native_edge_identity_stream(6, 139, 142);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.consolidated_edge_runs.is_empty());
    let [node] = native.consolidated_edge_nodes.as_slice() else {
        panic!("one consolidated edge node");
    };
    let uses = node.uses.as_ref().expect("standalone edge-owned uses");
    assert_eq!(uses.references, [[4, 5], [5, 6]]);
    assert_eq!(uses.senses, [0x88, 0x84]);
}

#[test]
fn native_namespace_retains_resolved_consolidated_edge_supports_and_loci() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));

    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.consolidated_edge_runs.as_slice() else {
        panic!("one consolidated edge run");
    };
    assert!(run.support_bindings.iter().all(|binding| matches!(
        binding,
        Some(CatiaConsolidatedSupportBinding::Cylinder { .. })
    )));
    assert_eq!(run.shared_loci.as_ref().map(Vec::len), Some(2));
    assert_eq!(
        run.endpoint_loci,
        run.shared_loci
            .as_ref()
            .map(|loci| [loci[0], loci[loci.len() - 1]])
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store resolved CATIA edge run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load resolved CATIA edge run"),
        native
    );
}

#[test]
fn standard_decode_transfers_resolved_consolidated_cylinder_surface_curve() {
    let mut records = b2_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        records.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            records.extend_from_slice(&value.to_le_bytes());
        }
    }
    records.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));
    let mut file = standard_catpart();
    file.splice(16..16, records);
    let file_len = u32::try_from(file.len()).expect("consolidated fixture length");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode resolved consolidated edge");
    let procedural = decoded
        .ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| curve.id.0.starts_with("catia:consolidated:construction#"))
        .expect("resolved consolidated construction");
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        panic!("two resolved support sides form an intersection");
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    let pcurve = context.sides[0].pcurve.as_ref().expect("cylinder pcurve");
    let start = cadmpeg_ir::eval::pcurve_uv(pcurve, 0.0).expect("pcurve start");
    let end = cadmpeg_ir::eval::pcurve_uv(pcurve, 1.0).expect("pcurve end");
    assert_eq!([start.u, start.v], [0.0, 0.0]);
    assert_eq!([end.u, end.v], [0.5, 1.0]);
}

#[test]
fn standard_decode_transfers_resolved_consolidated_cone_surface_curve() {
    let u = [0.0f64, 1.0];
    let v = [2.0f64, 3.0];
    let mut records = a5_pcurve_stream_with_uv(u, v);
    records.extend_from_slice(&a5_pcurve_stream_with_uv(u, v));
    records.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    records.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
    records.extend_from_slice(&b2_cone_stream());
    for (u, v) in u.into_iter().zip(v) {
        let phi = u / 3.0;
        let point = [
            1.0 + v * 0.25f64.sin() * phi.cos(),
            2.0 + v * 0.25f64.sin() * phi.sin(),
            3.0 + v * 0.25f64.cos(),
        ];
        records.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            records.extend_from_slice(&(value as f32).to_le_bytes());
        }
    }
    let mut file = standard_catpart();
    file.splice(16..16, records);
    let file_len = u32::try_from(file.len()).expect("consolidated fixture length");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode resolved consolidated cone edge");
    let procedural = decoded
        .ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| curve.id.0.starts_with("catia:consolidated:construction#"))
        .expect("resolved consolidated construction");
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        panic!("two resolved support sides form an intersection");
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    let pcurve = context.sides[0].pcurve.as_ref().expect("cone pcurve");
    let start = cadmpeg_ir::eval::pcurve_uv(pcurve, 0.0).expect("pcurve start");
    let end = cadmpeg_ir::eval::pcurve_uv(pcurve, 1.0).expect("pcurve end");
    assert_eq!([start.u, start.v], [0.0, 0.0]);
    assert_eq!([end.u, end.v], [1.0 / 3.0, 0.25f64.cos()]);
}

#[test]
fn standard_decode_transfers_resolved_consolidated_nurbs_surface_curves() {
    for offset in [0.0, 1.25] {
        let mut file = standard_catpart();
        file.splice(16..16, a5_nurbs_bound_edge_stream(offset));
        let file_len = u32::try_from(file.len()).expect("consolidated fixture length");
        file[8..12].copy_from_slice(&be32(file_len));

        let decoded = CatiaCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("decode resolved consolidated NURBS edge");
        let procedural = decoded
            .ir
            .model
            .procedural_curves
            .iter()
            .find(|curve| curve.id.0.starts_with("catia:consolidated:construction#"))
            .expect("resolved consolidated construction");
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            panic!("two resolved support sides form an intersection");
        };
        let surface_id = context.sides[1]
            .surface
            .as_ref()
            .expect("resolved NURBS support");
        let pcurve = context.sides[1].pcurve.as_ref().expect("NURBS pcurve");
        let start = cadmpeg_ir::eval::pcurve_uv(pcurve, 0.0).expect("pcurve start");
        let end = cadmpeg_ir::eval::pcurve_uv(pcurve, 1.0).expect("pcurve end");
        assert_eq!([start.u, start.v], [0.0, 0.0]);
        assert_eq!([end.u, end.v], [1.0, 0.0]);

        if offset == 0.0 {
            let surface = decoded
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| &surface.id == surface_id)
                .expect("direct NURBS carrier");
            assert!(matches!(surface.geometry, SurfaceGeometry::Nurbs(_)));
        } else {
            let construction = decoded
                .ir
                .model
                .procedural_surfaces
                .iter()
                .find(|surface| &surface.surface == surface_id)
                .expect("offset NURBS construction");
            let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset {
                support, distance, ..
            } = &construction.definition
            else {
                panic!("resolved normal offset is retained as an offset construction");
            };
            assert!((*distance - offset).abs() < 1e-12);
            assert!(decoded.ir.model.surfaces.iter().any(|surface| {
                surface.id == *support && matches!(surface.geometry, SurfaceGeometry::Nurbs(_))
            }));
        }
    }
}

#[test]
fn offset_support_binds_by_native_domain_knot_limits() {
    let mut carriers = crate::families::a5a8::records::a5_surfaces(&a5_surface_stream());
    let mut decoy = carriers[0].clone();
    let SurfaceGeometry::Nurbs(surface) = &mut decoy.geometry else {
        panic!("NURBS fixture");
    };
    for knot in &mut surface.v_knots {
        *knot += 10.0;
    }
    carriers.push(decoy);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let offset = crate::families::b2::records::B2OffsetSupport {
        pos: 0,
        support_id: 7,
        distance: 2.0,
        domain: [
            surface.u_knots[0],
            surface.v_knots[0],
            *surface.u_knots.last().unwrap(),
            *surface.v_knots.last().unwrap(),
        ],
    };

    assert_eq!(
        crate::families::b2::records::offset_support_carriers(&[offset], &carriers),
        [Some(0)]
    );
}

#[test]
fn decode_standard_transfers_exact_offset_construction() {
    let surface_bytes = a5_surface_stream();
    let carriers = crate::families::a5a8::records::a5_surfaces(&surface_bytes);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let domain = [
        surface.u_knots[0],
        surface.v_knots[0],
        *surface.u_knots.last().unwrap(),
        *surface.v_knots.last().unwrap(),
    ];
    let mut payload = surface_bytes;
    payload.extend_from_slice(&b2_offset_support_stream_for(domain));
    let mut file = standard_catpart();
    file.splice(16..16, payload);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("standard decode");
    let [procedural] = decoded.ir.model.procedural_surfaces.as_slice() else {
        panic!("one offset construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset {
        support,
        distance,
        u_sense,
        v_sense,
        extension_flags,
        ..
    } = &procedural.definition
    else {
        panic!("offset construction");
    };
    assert!(decoded
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id == *support));
    assert_eq!(*distance, 2.5);
    assert_eq!([*u_sense, *v_sense], [Some(1), Some(1)]);
    assert!(extension_flags.is_empty());
}

#[test]
fn decode_standard_transfers_construction_use_offset() {
    let surface_bytes = a5_surface_stream();
    let carriers = crate::families::a5a8::records::a5_surfaces(&surface_bytes);
    let SurfaceGeometry::Nurbs(surface) = &carriers[0].geometry else {
        panic!("NURBS fixture");
    };
    let domain = [
        surface.u_knots[0],
        surface.v_knots[0],
        *surface.u_knots.last().unwrap(),
        *surface.v_knots.last().unwrap(),
    ];
    let mut payload = surface_bytes;
    payload.extend_from_slice(&b2_construction_use_stream_for(domain));
    let mut file = standard_catpart();
    file.splice(16..16, payload);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("standard decode");
    let [procedural] = decoded.ir.model.procedural_surfaces.as_slice() else {
        panic!("one offset construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset { distance, .. } =
        &procedural.definition
    else {
        panic!("offset construction");
    };
    assert_eq!(*distance, -2.0);
}

#[test]
fn decode_standard_transfers_exact_rolling_ball_jet() {
    let mut file = standard_catpart();
    file.splice(16..16, a5_freeform_curve_stream());
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("standard decode");
    let [procedural] = decoded.ir.model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
        degree,
        knots,
        multiplicities,
        sites,
    } = &procedural.definition
    else {
        panic!("rolling-ball jet");
    };
    assert_eq!(*degree, 5);
    assert_eq!(knots, &[0.0, 1.0]);
    assert_eq!(multiplicities, &[6, 6]);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].first_limit, Point3::new(1.0, 0.0, 0.0));
    assert_eq!(sites[1].second_limit, Point3::new(0.0, 2.0, 0.0));
    assert_eq!(sites[0].angle, std::f64::consts::FRAC_PI_2);
    assert_eq!(
        sites[0].first_derivative.center,
        Vector3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn consolidated_edge_use_run_is_independent_of_pcurve_availability() {
    use crate::families::b2::records::B2UseSense;

    let runs = crate::families::consolidated::records::consolidated_edge_use_runs(
        &a5_native_edge_identity_stream(6, 139, 142),
    );
    let [run] = runs.as_slice() else {
        panic!("one standalone edge-use run");
    };
    assert!(run.identity_chain_consistent);
    assert_eq!(run.uses[0].sense, Some(B2UseSense::Sense88));
    assert_eq!(run.uses[1].sense, Some(B2UseSense::Sense84));
    assert_eq!(run.node.start_vertex_ref, 139);
    assert_eq!(run.node.end_vertex_ref, 142);
}

#[test]
fn consolidated_edge_use_run_owns_adjacent_compact_definition() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let mut bytes = vec![0xb2, 0x03, 0x24, 0x04, 0x05, 0x81, 0x05, 0x0f, 0x87];
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));

    let runs = crate::families::consolidated::records::consolidated_edge_use_runs(&bytes);
    let [run] = runs.as_slice() else {
        panic!("one edge-use run");
    };
    let definition = run.definition.as_ref().expect("adjacent definition");
    assert_eq!(definition.class, 0x24);
    assert_eq!(definition.header_token, 5);
    assert_eq!(definition.payload, [0x81, 0x05, 0x0f, 0x87]);
    assert_eq!(
        definition.data,
        Some(ConsolidatedEdgeDefinitionData::Compact24 { operand: 1 })
    );

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .expect("native definition")
            .class,
        0x24
    );
    assert!(matches!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .and_then(|definition| definition.data.as_ref()),
        Some(
            crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Compact24 {
                operand: 1
            }
        )
    ));
}

#[test]
fn consolidated_edge_definition_decodes_class25_scalar_layouts() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let operands = [0x82, 0x05, 0xe7, 0x0a, 0x87, 0x0d];
    let mut plain = operands.to_vec();
    for value in [1.0_f64, 2.0, 1e-6, 3.0, 4.0, 1.0, 5.0, 1e-6] {
        plain.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &plain),
        Some(ConsolidatedEdgeDefinitionData::Scalar25 {
            operands: [1, 0xe7, 3463],
            persistent_lead: Some(0x0a),
            values: vec![1.0, 2.0, 1e-6, 3.0, 4.0, 1.0, 5.0, 1e-6],
        })
    );

    let mut segmented = operands.to_vec();
    for value in [1.0_f64, 2.0, 1e-6, 3.0, 4.0] {
        segmented.extend_from_slice(&value.to_le_bytes());
    }
    segmented.push(0x82);
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 1e-6] {
        segmented.extend_from_slice(&value.to_le_bytes());
    }
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &segmented),
        Some(ConsolidatedEdgeDefinitionData::SegmentedScalar25 {
            operands: [1, 0xe7, 3463],
            persistent_lead: Some(0x0a),
            marker: 0x82,
            ref trailing,
            ..
        }) if trailing.len() == 6
    ));
    segmented[46] = 0x84;
    assert!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &segmented)
            .is_none()
    );

    let mut odd_lead = plain.clone();
    odd_lead[3] = 0x0b;
    odd_lead.drain(odd_lead.len() - 8..);
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &odd_lead),
        Some(ConsolidatedEdgeDefinitionData::Scalar25 {
            persistent_lead: Some(0x0b),
            ref values,
            ..
        }) if values.len() == 7
    ));

    let mut long_segment = operands.to_vec();
    for value in [1.0_f64, 2.0, 1e-6, 3.0, 4.0] {
        long_segment.extend_from_slice(&value.to_le_bytes());
    }
    long_segment.push(0x89);
    for value in 0..20 {
        long_segment.extend_from_slice(&f64::from(value).to_le_bytes());
    }
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &long_segment),
        Some(ConsolidatedEdgeDefinitionData::SegmentedScalar25 {
            marker: 0x89,
            ref trailing,
            ..
        }) if trailing.len() == 20
    ));

    let mut bytes = vec![0xb2, 0x03, 0x25, plain.len() as u8, 0x05];
    bytes.extend_from_slice(&plain);
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(matches!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .and_then(|definition| definition.data.as_ref()),
        Some(
            crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Scalar25 {
                operands: [1, 0xe7, 3463],
                persistent_lead: Some(0x0a),
                ..
            }
        )
    ));

    let mut descriptor_payload = vec![0x08, 0x34, 0x12, 0x02];
    descriptor_payload.extend_from_slice(&3.0_f64.to_le_bytes());
    descriptor_payload.extend_from_slice(&7.0_f64.to_le_bytes());
    let mut described = vec![0xb2, 0x03, 0x18, descriptor_payload.len() as u8, 0x05];
    described.extend_from_slice(&descriptor_payload);
    described.extend_from_slice(&bytes);
    let runs = crate::families::consolidated::records::consolidated_class25_edge_runs(&described);
    let [run] = runs.as_slice() else {
        panic!("one described class-25 edge run");
    };
    assert_eq!(run.descriptor.record_id, 0x1234);
    assert_eq!(run.descriptor.values, [3.0, 7.0]);
    assert!(run.identity_chain_consistent);
    let native = crate::native::CatiaNative::decode(&described);
    assert_eq!(
        native.consolidated_edge_nodes[0]
            .class25_descriptor
            .as_ref()
            .expect("native class-25 descriptor")
            .control,
        0x02
    );
}

#[test]
fn consolidated_analytic_circle_run_binds_adjacent_carrier() {
    fn record(class: u8, token: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xb2, 0x03, class, payload.len() as u8, token];
        bytes.extend_from_slice(payload);
        bytes
    }

    let mut parameter = vec![0x05, 0x00];
    parameter.extend_from_slice(&12.0_f64.to_le_bytes());
    parameter.extend_from_slice(&34.0_f64.to_le_bytes());
    let mut circle = vec![0x05];
    for value in [12.0_f64, 34.0, 5.0, 0.0, 10.0] {
        circle.extend_from_slice(&value.to_le_bytes());
    }
    circle.extend_from_slice(&[0; 9]);
    let mut definition = vec![0x82, 0x05, 0x09, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 10.0, 1e-6, 4.0, 9.0, 1.0, -2.0, 1e-6] {
        definition.extend_from_slice(&value.to_le_bytes());
    }
    let mut bytes = record(0x18, 0x15, &parameter);
    bytes.extend_from_slice(&record(0x19, 0x05, &circle));
    bytes.extend_from_slice(&record(0x23, 0x05, &definition));
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));

    let runs =
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs(&bytes);
    let [run] = runs.as_slice() else {
        panic!("one analytic-circle edge run");
    };
    assert_eq!(run.circle.center_pair, [12.0, 34.0]);
    assert_eq!(run.circle.radius, 5.0);
    assert_eq!(run.descriptor.header_token, 0x15);
    assert_eq!(run.definition.pos, parameter.len() + circle.len() + 10);
    assert!(run.identity_chain_consistent);

    let native = crate::native::CatiaNative::decode(&bytes);
    let carrier = native.consolidated_edge_nodes[0]
        .analytic_circle
        .as_ref()
        .expect("native analytic circle");
    assert_eq!(carrier.center_pair, [12.0, 34.0]);
    assert_eq!(carrier.range, [0.0, 10.0]);

    let circle_end = parameter.len() + circle.len() + 10;
    let mut broken = bytes[..circle_end].to_vec();
    broken.extend_from_slice(&record(0x05, 0x05, &[0x00]));
    broken.extend_from_slice(&bytes[circle_end..]);
    assert!(
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs(&broken)
            .is_empty()
    );
}

#[test]
fn a5_topology_edge_run_preserves_uses_and_native_endpoint_identities() {
    use crate::families::b2::records::B2UseSense;

    let runs = crate::families::consolidated::records::consolidated_topology_edge_runs(
        &a5_topology_edge_run_stream(),
    );
    assert_eq!(runs.len(), 1);
    assert!(runs[0].edge.co_parametric);
    assert_eq!(runs[0].uses[0].sense, Some(B2UseSense::Sense84));
    assert_eq!(runs[0].uses[1].sense, Some(B2UseSense::Sense88));
    assert_eq!(runs[0].uses[0].references.as_deref(), Some(&[1, 2][..]));
    assert_eq!(runs[0].uses[1].references.as_deref(), Some(&[2, 3][..]));
    assert!(!runs[0].identity_chain_consistent);
    assert_eq!(runs[0].node.start_vertex_ref, 889);
    assert_eq!(runs[0].node.end_vertex_ref, 895);
}

#[test]
fn outer_object_graph_parser_reads_nested_heads_and_payload_fields() {
    use crate::object_graph::{PayloadField, PayloadSubtype};

    let graph = crate::object_graph::parse(&object_graph_stream()).unwrap();
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].owner_ref, Some(2));
    assert_eq!(graph.records[0].class_ref, Some(3));
    assert_eq!(graph.records[0].storage_ref, Some(4));
    assert_eq!(graph.records[0].subtype, PayloadSubtype::Mixed);
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
    assert_eq!(graph.record(1).map(|record| record.index), Some(0));
    assert_eq!(graph.record(2).map(|record| record.index), Some(1));
    assert!(graph.record(0).is_none());
    assert!(graph.record(3).is_none());
    assert_eq!(
        graph
            .children(2)
            .map(|record| record.index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn outer_object_graph_uses_the_unique_length_closing_child_frame() {
    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x7c, 0x0a, 0xff, 0xff, 0xff, 0xff, 0x82, 0x83],
            &[0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x84], &[0xfe]),
    ];
    let graph = crate::object_graph::parse(&object_graph_from_records(&records))
        .expect("length-closing object payload");
    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(
        &graph.records[0].head[graph.records[0].head.len() - 2..],
        [
            crate::object_graph::HeadToken::Reference(2),
            crate::object_graph::HeadToken::Reference(3),
        ]
    );
}

#[test]
fn outer_object_graph_rejects_ambiguous_length_closing_child_frames() {
    let mut first = object_graph_record(&[0x04, 0x01, 0x82, 0x83], &[0xfe]);
    let fake = 8;
    first.splice(fake..fake, [0x7c, 0x0a, 0, 0, 0, 0]);
    let closing_len = u32::try_from(first.len() - fake).expect("fixture child length");
    first[fake + 2..fake + 6].copy_from_slice(&closing_len.to_le_bytes());
    let record_len = u32::try_from(first.len()).expect("fixture record length");
    first[2..6].copy_from_slice(&record_len.to_le_bytes());

    let second = object_graph_record(&[0x04, 0x01, 0x82, 0x84], &[0xfe]);
    assert!(crate::object_graph::parse(&object_graph_from_records(&[first, second])).is_none());
}

#[test]
fn outer_object_graph_requires_records_to_cover_the_root_extent() {
    let mut bytes = object_graph_stream();
    bytes.extend_from_slice(&[0xaa, 0xbb]);
    let declared_len = u32::try_from(bytes.len()).expect("fixture graph length");
    bytes[2..6].copy_from_slice(&declared_len.to_le_bytes());

    assert!(crate::object_graph::parse(&bytes).is_none());
}

#[test]
fn outer_object_graph_requires_a_final_payload_terminator() {
    for payload in [&[0xfe, 0xaa][..], &[0xe5, 1, 0, 0, 0, 0xfe][..]] {
        let bytes =
            object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], payload)]);
        assert!(crate::object_graph::parse(&bytes).is_none());
    }
}

#[test]
fn outer_object_graph_requires_a_stored_head_lead() {
    let bytes = object_graph_from_records(&[object_graph_record(&[], &[0xfe])]);
    assert!(crate::object_graph::parse(&bytes).is_none());
}

#[test]
fn outer_object_graph_accepts_one_length_closed_record() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("one-record object graph");

    assert_eq!(graph.records.len(), 1);
    assert_eq!(graph.records[0].owner_ref, Some(1));
    assert_eq!(graph.records[0].class_ref, Some(1));
    assert_eq!(
        graph.records[0].subtype,
        crate::object_graph::PayloadSubtype::Empty
    );
}

#[test]
fn outer_object_graph_keeps_adjacent_compact_head_references_separate() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83, 0x84],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("compact object head");
    let record = &graph.records[0];

    assert_eq!(record.owner_ref, Some(1));
    assert_eq!(record.class_ref, Some(3));
    assert_eq!(record.storage_ref, Some(4));
    assert_eq!(
        &record.head[2..],
        [
            crate::object_graph::HeadToken::Reference(1),
            crate::object_graph::HeadToken::Reference(3),
            crate::object_graph::HeadToken::Reference(4),
        ]
    );
}

#[test]
fn outer_object_graph_does_not_slide_head_roles_across_null_handles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x82, 0xff, 0xff, 0xff, 0xff, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("null-interrupted object head");
    let record = &graph.records[0];

    assert_eq!(record.owner_ref, Some(2));
    assert_eq!(record.class_ref, None);
    assert_eq!(record.storage_ref, None);
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_does_not_promote_unassigned_head_bytes() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0xe5, 0xff, 0xff, 0xff, 0xe4],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("literal head bytes");

    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[0].storage_ref, None);
    assert_eq!(
        &graph.records[0].head[2..],
        [
            crate::object_graph::HeadToken::Literal(0xe5),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xff),
            crate::object_graph::HeadToken::Literal(0xe4),
        ]
    );
}

#[test]
fn outer_object_graph_requires_the_head_separator_for_relations() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x82, 0x83, 0x84], &[0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("retained malformed head");

    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[0].storage_ref, None);
    assert!(graph.records[0]
        .head
        .iter()
        .any(|token| matches!(token, crate::object_graph::HeadToken::Reference(2))));
}

#[test]
fn outer_object_graph_reads_compact_owner_and_field_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x02, 0x82], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x82, 0x83, 0x84], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("compact heads");

    assert_eq!(graph.records[0].owner_ref, Some(2));
    assert_eq!(graph.records[0].class_ref, None);
    assert_eq!(graph.records[1].owner_ref, Some(2));
    assert_eq!(graph.records[1].class_ref, Some(3));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[2].owner_ref, Some(2));
    assert_eq!(graph.records[2].class_ref, Some(3));
    assert_eq!(graph.records[2].storage_ref, Some(4));
}

#[test]
fn object_graph_payload_reads_fixed_width_escaped_values() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x83],
            &[
                0x80, 0x78, 0x56, 0x34, 0x12, 0x32, 2, 0, 0, 0, 0x32, 0xef, 0xcd, 0xab, 0x89, 0xfe,
            ],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("fixed-width object payload");
    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 0x1234_5678,
                offset: 0,
            },
            PayloadField::Reference {
                value: 2,
                offset: 5,
            },
            PayloadField::Reference {
                value: 0x89ab_cdef,
                offset: 10,
            },
            PayloadField::Terminator,
        ]
    );
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.object_graphs[0].records[0].references,
        [
            crate::native::CatiaObjectRecordReference {
                ordinal: 2,
                target: Some(native.object_graphs[0].records[1].id.clone()),
                design_object: native.object_graphs[0].records[1].design_object.clone(),
            },
            crate::native::CatiaObjectRecordReference {
                ordinal: 0x89ab_cdef,
                target: None,
                design_object: None,
            },
        ]
    );
}

#[test]
fn incomplete_object_payload_tags_do_not_consume_the_terminator() {
    for tag in [0x81, 0x3a, 0x39, 0x7a] {
        let bytes = object_graph_from_records(&[object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[tag, 0xfe],
        )]);
        let graph = crate::object_graph::parse(&bytes).expect("terminated tagged payload");
        let record = &graph.records[0];

        assert_eq!(
            record.payload.fields,
            [
                crate::object_graph::PayloadField::Atom {
                    value: u32::from(tag),
                    offset: 0,
                },
                crate::object_graph::PayloadField::Terminator,
            ]
        );
        assert!(
            crate::native::CatiaNative::decode(&bytes).object_graphs[0].records[0]
                .references
                .is_empty()
        );
    }
}

#[test]
fn native_design_objects_preserve_payload_references_to_target_owners() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0x81, 0x81, 0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_ordinal, 1);
    assert_eq!(native.design_objects[0].ordinal, 0);
    assert_eq!(
        native.design_objects[0].first_field_byte_offset,
        native.object_graphs[0].records[0].byte_offset
    );
    assert_eq!(native.design_objects[0].fields.len(), 2);
    assert!(native.design_objects[0].field_classes.is_empty());
    let graph = &native.object_graphs[0];
    assert_eq!(
        graph.records[0].design_object.as_deref(),
        Some(native.design_objects[0].id.as_str())
    );
    assert_eq!(
        graph.records[0].references,
        [crate::native::CatiaObjectRecordReference {
            ordinal: 3,
            target: Some(graph.records[2].id.clone()),
            design_object: graph.records[2].design_object.clone(),
        }]
    );
    assert_eq!(
        native.design_objects[0].object_references,
        vec![native.design_objects[1].id.clone()]
    );
    assert_eq!(native.design_objects[1].owner_ordinal, 3);
    assert_eq!(native.design_objects[1].ordinal, 1);
    assert_eq!(
        native.design_objects[1].first_field_byte_offset,
        native.object_graphs[0].records[2].byte_offset
    );
    assert_eq!(
        native.design_objects[1].object_references,
        vec![native.design_objects[0].id.clone()]
    );
}

#[test]
fn compact_design_objects_use_field_vocabulary_not_anchor_class() {
    let mut bytes = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "BaseFeature",
        "Groove",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.design_objects.len(), 1);
    let object = &native.design_objects[0];
    assert_eq!(object.owner_ordinal, 2);
    assert!(object.owner_record.is_some());
    assert_eq!(object.owner_class, None);
    assert_eq!(object.owner_storage_ref, None);
    assert_eq!(
        object.field_classes,
        [
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[4].id.clone(),
                name: "BaseFeature".to_string(),
            },
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[5].id.clone(),
                name: "Groove".to_string(),
            },
        ]
    );
}

#[test]
fn design_field_vocabulary_distinguishes_equal_names_from_distinct_entries() {
    let mut bytes = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Feature",
        "Feature",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let classes = &native.design_objects[0].field_classes;

    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0].name, classes[1].name);
    assert_ne!(classes[0].entry, classes[1].entry);
}

#[test]
fn native_design_objects_preserve_unresolved_owner_identities() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x80, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x84, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = &native.object_graphs[0];

    assert_eq!(graph.records[0].owner_ref, Some(0));
    assert_eq!(graph.records[1].owner_ref, Some(4));
    assert!(graph
        .records
        .iter()
        .all(|record| record.design_object.is_some()));
    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_ordinal, 0);
    assert_eq!(native.design_objects[1].owner_ordinal, 4);
    assert!(native
        .design_objects
        .iter()
        .all(|object| object.owner_record.is_none()));
}

#[test]
fn native_design_objects_follow_first_field_order() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| object.owner_ordinal)
            .collect::<Vec<_>>(),
        [3, 1]
    );
    assert_eq!(native.design_objects[0].fields.len(), 2);
    assert_eq!(native.design_objects[1].fields.len(), 1);
    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| (object.ordinal, object.first_field_byte_offset))
            .collect::<Vec<_>>(),
        [
            (0, native.object_graphs[0].records[0].byte_offset),
            (1, native.object_graphs[0].records[1].byte_offset),
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store source-ordered design objects");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load source-ordered design objects");
    assert_eq!(
        loaded
            .design_objects
            .iter()
            .map(|object| object.owner_ordinal)
            .collect::<Vec<_>>(),
        [3, 1]
    );
}

#[test]
fn incomplete_object_lists_do_not_assert_reference_links() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0x3b, 0x83, 0x81, 0x82, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(native.object_graphs[0].records[0].references.is_empty());
    assert!(native.design_objects[0].object_references.is_empty());
    assert!(matches!(
        &native.object_graphs[0].records[0].payload.fields[0],
        crate::object_graph::PayloadField::List {
            declared_count: 3,
            items,
            ..
        } if items == &[crate::object_graph::ListItem::Reference(2)]
    ));
}

#[test]
fn incomplete_object_list_tags_do_not_consume_the_payload_terminator() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[0x3b, 0x82, 0x81, 0x82, 0x81, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let record = &native.object_graphs[0].records[0];

    assert!(record.references.is_empty());
    assert!(native.design_objects[0].object_references.is_empty());
    assert!(matches!(
        record.payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 2,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Reference(2)]
    ));
}

#[test]
fn incomplete_object_list_headers_do_not_consume_the_payload_terminator() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x81],
        &[0x3b, 0xfe],
    )]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let record = &native.object_graphs[0].records[0];

    assert_eq!(
        record.payload.fields,
        [
            crate::object_graph::PayloadField::Atom {
                value: 0x3b,
                offset: 0,
            },
            crate::object_graph::PayloadField::Terminator,
        ]
    );
    assert!(record.references.is_empty());
    assert!(native.design_objects[0].object_references.is_empty());
}

#[test]
fn outer_object_graph_resolves_class_names_from_following_schema() {
    let mut bytes = object_graph_stream();
    let graph_len = bytes.len();
    bytes.extend(value_block_stream(&[0x81]));
    let catalog_pos = bytes.len();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let graph = crate::object_graph::parse(&bytes).expect("object graph with schema");
    assert_eq!(graph.total_len, graph_len);
    assert_eq!(graph.catalog_pos, Some(catalog_pos));
    assert_eq!(graph.records[0].class_name.as_deref(), Some(""));
    assert_eq!(graph.records[1].class_name.as_deref(), Some("Sketch"));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.object_graphs[0].catalog,
        Some(native.catalogs[0].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[0].class_entry,
        Some(native.catalogs[0].entries[3].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[1].class_entry,
        Some(native.catalogs[0].entries[4].id.clone())
    );
    assert_eq!(
        native.design_objects[0].field_classes,
        [
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[3].id.clone(),
                name: String::new(),
            },
            crate::native::CatiaDesignClass {
                entry: native.catalogs[0].entries[4].id.clone(),
                name: "Sketch".to_string(),
            },
        ]
    );
    assert_eq!(
        native.design_objects[0].owner_class,
        Some(crate::native::CatiaDesignClass {
            entry: native.catalogs[0].entries[4].id.clone(),
            name: "Sketch".to_string(),
        })
    );
    assert_eq!(native.design_objects[0].owner_storage_ref, None);
}

#[test]
fn outer_object_graph_parser_preserves_every_root() {
    let first = object_graph_stream();
    let mut bytes = first.clone();
    bytes.extend(object_graph_vm_stream());
    let graphs = crate::object_graph::parse_all(&bytes);
    assert_eq!(graphs.len(), 2);
    assert_eq!(graphs[0].pos, 0);
    assert_eq!(graphs[1].pos, first.len());
}

#[test]
fn outer_object_graph_suppresses_roots_inside_framed_payloads() {
    let nested =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut payload = vec![0xe5];
    payload.extend_from_slice(
        &u32::try_from(nested.len())
            .expect("fixture nested graph length")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&nested);
    payload.push(0xfe);
    let outer =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)]);

    let graphs = crate::object_graph::parse_all(&outer);
    assert_eq!(graphs.len(), 1);
    assert_eq!(graphs[0].pos, 0);
}

#[test]
fn outer_object_graph_resolves_paged_class_ordinals() {
    let records = [
        object_graph_record(&[0x14, 0x01, 0x82, 0xd1, 0x88], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x82], &[0xfe]),
    ];
    let mut bytes = object_graph_from_records(&records);
    let mut names = vec!["field"; 138];
    names[0] = "CATCatalogManager";
    names[1] = "catalogManager";
    names[2] = "catalogLinks";
    names[3] = "";
    names[137] = "Pad";
    let mut schema = vec![0x7c, 0x02, 0, 0, 0, 0, 0xd1, 0x8a];
    for name in names {
        schema.push(u8::try_from(name.len() + 1).expect("fixture schema name length"));
        schema.extend_from_slice(name.as_bytes());
    }
    let schema_len = u32::try_from(schema.len()).expect("fixture schema length");
    schema[2..6].copy_from_slice(&schema_len.to_le_bytes());
    bytes.extend(schema);
    let graph = crate::object_graph::parse(&bytes).expect("paged class graph");
    assert_eq!(graph.records[0].class_ref, Some(137));
    assert_eq!(graph.records[0].class_name.as_deref(), Some("Pad"));
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
    let payload = [0x81, 0x83, 0x32, 4, 0, 0, 0, 0x83, 0x82];
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
fn native_value_blocks_require_a_complete_adjacent_catalog() {
    let mut bytes = value_block_stream(&[0x81]);
    bytes.extend_from_slice(&[0x7c, 0x02]);

    assert_eq!(crate::value_block::parse(&bytes).len(), 1);
    assert!(crate::native::CatiaNative::decode(&bytes)
        .value_blocks
        .is_empty());
}

#[test]
fn native_value_blocks_distinguish_the_terminal_schema_sentinel() {
    let mut bytes = value_block_stream(&[0x32, 4, 0, 0, 0, 0x83, 0x32, 5, 0, 0, 0, 0x82]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let block = &native.value_blocks[0];
    assert_eq!(block.schema_selections.len(), 1);
    assert_eq!(block.schema_selections[0].ordinal, 4);
    assert_eq!(block.schema_selections[0].entry, None);
    assert_eq!(block.schema_selections[0].name, None);
    assert!(block.schema_selections[0].encoded_value.is_empty());
    assert!(block.fields.iter().any(|field| matches!(
        field,
        crate::value_block::ValueField::SchemaSelector { ordinal: 5, .. }
    )));
}

#[test]
fn native_value_blocks_frame_values_between_catalog_valid_selectors() {
    let mut bytes = value_block_stream(&[
        0x32, 3, 0, 0, 0, 0x83, 0x32, 5, 0, 0, 0, 0x84, 0x32, 2, 0, 0, 0, 0x32, 1, 0, 0, 0, 0x82,
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    let selections = &native.value_blocks[0].schema_selections;
    assert_eq!(selections.len(), 3);
    assert_eq!(selections[0].parent, native.value_blocks[0].id);
    assert_eq!(
        selections[0].id,
        format!(
            "catia:outer:value-selection#{:010}",
            native.value_blocks[0].byte_offset + 6 + selections[0].offset
        )
    );
    assert_eq!(selections[0].ordinal, 3);
    assert!(matches!(
        selections[0].encoded_value.as_slice(),
        [
            crate::value_block::ValueField::Atom { value: 3, .. },
            crate::value_block::ValueField::SchemaSelector { ordinal: 5, .. },
            crate::value_block::ValueField::Atom { value: 4, .. },
        ]
    ));
    assert_eq!(selections[1].ordinal, 2);
    assert!(selections[1].encoded_value.is_empty());
    assert_eq!(selections[2].ordinal, 1);
    assert!(matches!(
        selections[2].encoded_value.as_slice(),
        [crate::value_block::ValueField::Atom { value: 2, .. }]
    ));
}

#[test]
fn native_design_inventory_excludes_records_inside_object_payloads() {
    let mut nested = value_block_stream(&[0x81]);
    nested.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));
    nested.extend(surface_alias_stream());
    let mut payload = vec![0xe5];
    payload.extend_from_slice(
        &u32::try_from(nested.len())
            .expect("fixture nested design length")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&nested);
    payload.push(0xfe);
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)]);

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.object_graphs.len(), 1);
    assert!(native.alias_rows.is_empty());
    assert!(native.catalogs.is_empty());
    assert!(native.value_blocks.is_empty());
}

#[test]
fn native_design_inventory_excludes_records_inside_value_payloads() {
    let mut nested = value_block_stream(&[0x81]);
    nested.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));
    nested.extend(surface_alias_stream());
    let mut bytes = value_block_stream(&nested);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    assert_eq!(crate::value_block::parse(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(native.alias_rows.is_empty());
    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
}

#[test]
fn native_design_inventory_excludes_object_graphs_inside_value_payloads() {
    let nested =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut bytes = value_block_stream(&nested);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
    ]));

    assert_eq!(crate::object_graph::parse_all(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(native.object_graphs.is_empty());
    assert!(native.design_objects.is_empty());
    assert_eq!(native.value_blocks.len(), 1);
    assert_eq!(native.catalogs.len(), 1);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
}

#[test]
fn native_design_inventory_excludes_alias_rows_inside_catalog_entries() {
    let mut alias = 1u32.to_le_bytes().to_vec();
    alias.extend_from_slice(&[0x01, 0x00, 0x04, 0x00]);
    alias.extend_from_slice(&0x0012_3456u32.to_le_bytes());
    alias.extend_from_slice(&[1, 2, 3, 4]);
    alias.extend_from_slice(&0x1122_3344u32.to_le_bytes());
    alias.extend_from_slice(&0x5566_7744u32.to_le_bytes());
    let entry = String::from_utf8(alias).expect("alias-shaped UTF-8 entry bytes");
    let bytes = catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        &entry,
    ]);

    assert_eq!(crate::object_graph::surface_aliases(&bytes).len(), 1);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.catalogs.len(), 1);
    assert!(native.alias_rows.is_empty());
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
fn decode_retains_outer_object_graph_order_and_references() {
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
    assert_eq!(native.design_objects.len(), 1);
    let object = &native.design_objects[0];
    assert_eq!(object.parent, graph.id);
    assert_eq!(object.owner_ordinal, 2);
    assert_eq!(
        object.owner_record.as_deref(),
        Some(graph.records[1].id.as_str())
    );
    assert_eq!(
        object.fields,
        graph
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(decoded.report.coverage["decoded_object_graph_count"], 1);
    assert_eq!(decoded.report.coverage["decoded_object_record_count"], 2);
    assert_eq!(decoded.report.coverage["decoded_design_object_count"], 1);
    assert_eq!(decoded.report.coverage["decoded_design_field_count"], 2);
    assert_eq!(
        decoded.report.coverage["decoded_design_object_reference_count"],
        0
    );
    assert_eq!(decoded.report.coverage["classified_design_object_count"], 0);
    assert_eq!(decoded.report.coverage["unresolved_design_owner_count"], 0);
    assert_eq!(decoded.report.coverage["transferred_feature_count"], 0);
    assert_eq!(decoded.report.coverage["transferred_parameter_count"], 0);
    assert_eq!(decoded.report.coverage["transferred_sketch_count"], 0);
    assert_eq!(
        decoded.report.coverage["transferred_sketch_constraint_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["transferred_configuration_count"],
        0
    );
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("1 design object(s)")
            && loss.message.contains("2 object-graph field record(s)")
    }));
    let validation = cadmpeg_ir::validate::validate(&decoded.ir, Vec::new());
    assert!(validation
        .findings
        .iter()
        .all(|finding| finding.check != cadmpeg_ir::report::Check::Identity));
}

#[test]
fn decode_links_design_objects_through_their_owner_record_group() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_nested_design_objects()),
            &DecodeOptions::default(),
        )
        .expect("decode nested design objects");
    let native = crate::native::CatiaNative::load(
        decoded
            .ir
            .native
            .namespace("catia")
            .expect("CATIA namespace"),
    )
    .expect("load CATIA native records");

    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_ordinal, 2);
    assert_eq!(native.design_objects[1].owner_ordinal, 3);
    assert_eq!(
        native.design_objects[0].owner_design_object.as_deref(),
        Some(native.design_objects[1].id.as_str())
    );
    assert_eq!(native.design_objects[1].owner_design_object, None);
    assert_eq!(
        decoded.report.coverage["decoded_design_object_owner_link_count"],
        1
    );
}

#[test]
fn native_load_rejects_orphaned_and_ambiguously_owned_design_records() {
    let mut bytes = object_graph_stream();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA native namespace");

    for arena_name in ["catalogs", "object_graphs"] {
        let mut malformed = namespace.clone();
        malformed
            .arenas
            .get_mut(arena_name)
            .expect("owner arena")
            .clear();
        assert!(matches!(
            crate::native::CatiaNative::load(&malformed),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }

    for arena_name in ["catalogs", "object_graphs"] {
        let mut malformed = namespace.clone();
        let arena = malformed.arenas.get_mut(arena_name).expect("owner arena");
        arena.push(arena.first().expect("owner record").clone());
        assert!(matches!(
            crate::native::CatiaNative::load(&malformed),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }

    let mut stale_design_objects = namespace.clone();
    stale_design_objects
        .arenas
        .get_mut("design_objects")
        .expect("derived design-object arena")
        .clear();
    assert!(matches!(
        crate::native::CatiaNative::load(&stale_design_objects),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_dangling_cross_arena_links() {
    let mut value_native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    value_native.value_blocks[0].catalog = "catia:missing-catalog".to_string();
    let mut value_namespace = cadmpeg_ir::NativeNamespace::default();
    value_native
        .store(&mut value_namespace)
        .expect("store malformed value link");
    assert!(matches!(
        crate::native::CatiaNative::load(&value_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut omitted_value_graph =
        crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    omitted_value_graph.value_blocks[0].object_graph = None;
    let mut omitted_value_namespace = cadmpeg_ir::NativeNamespace::default();
    omitted_value_graph
        .store(&mut omitted_value_namespace)
        .expect("store omitted value-block graph link");
    assert!(matches!(
        crate::native::CatiaNative::load(&omitted_value_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut external_native =
        crate::native::CatiaNative::decode(&external_reference_segment("Support.CATPart"));
    external_native.external_references[0].segment = "catia:missing-segment".to_string();
    let mut external_namespace = cadmpeg_ir::NativeNamespace::default();
    external_native
        .store(&mut external_namespace)
        .expect("store malformed external-reference link");
    assert!(matches!(
        crate::native::CatiaNative::load(&external_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut alias_native = crate::native::CatiaNative::decode(&surface_alias_stream());
    alias_native.alias_rows[0].object_graph = Some("catia:missing-graph".to_string());
    alias_native.alias_rows[0].object_record = Some("catia:missing-record".to_string());
    let mut alias_namespace = cadmpeg_ir::NativeNamespace::default();
    alias_native
        .store(&mut alias_namespace)
        .expect("store malformed alias link");
    assert!(matches!(
        crate::native::CatiaNative::load(&alias_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let mut linked_alias = surface_alias_stream();
    linked_alias[15] = 1;
    let mut linked_bytes = graph;
    linked_bytes.extend(linked_alias);
    let mut omitted_alias_links = crate::native::CatiaNative::decode(&linked_bytes);
    assert!(omitted_alias_links.alias_rows[0].object_graph.is_some());
    omitted_alias_links.alias_rows[0].object_graph = None;
    omitted_alias_links.alias_rows[0].object_record = None;
    let mut omitted_alias_namespace = cadmpeg_ir::NativeNamespace::default();
    omitted_alias_links
        .store(&mut omitted_alias_namespace)
        .expect("store omitted alias links");
    assert!(matches!(
        crate::native::CatiaNative::load(&omitted_alias_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_noncanonical_catalog_and_record_views() {
    let mut bytes = object_graph_stream();
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);

    let mut invalid_count = native.clone();
    invalid_count.catalogs[0].declared_count += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_count
        .store(&mut namespace)
        .expect("store invalid catalog count");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_entry_ordinal = native.clone();
    invalid_entry_ordinal.catalogs[0].entries[0].ordinal = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_entry_ordinal
        .store(&mut namespace)
        .expect("store invalid catalog ordinal");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_record_ordinal = native.clone();
    invalid_record_ordinal.object_graphs[0].records[0].ordinal = 9;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_record_ordinal
        .store(&mut namespace)
        .expect("store invalid record ordinal");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_design_link = native.clone();
    invalid_design_link.object_graphs[0].records[0].design_object = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_design_link
        .store(&mut namespace)
        .expect("store invalid design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid_references = native;
    invalid_references.object_graphs[0].records[0]
        .references
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_references
        .store(&mut namespace)
        .expect("store invalid payload-reference links");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_load_rejects_noncanonical_value_block_views() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    let mut canonical_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut canonical_namespace)
        .expect("store canonical value selections");
    assert!(canonical_namespace
        .arenas
        .get("value_blocks")
        .is_some_and(|blocks| blocks
            .iter()
            .all(|block| !block.fields.contains_key("schema_selections"))));
    assert_eq!(
        canonical_namespace
            .arenas
            .get("value_schema_selections")
            .map(Vec::len),
        Some(native.value_blocks[0].schema_selections.len())
    );
    let mut orphaned_selections: Vec<crate::native::CatiaValueSchemaSelection> =
        canonical_namespace
            .arena_as("value_schema_selections")
            .expect("load stored value selections");
    orphaned_selections[0].parent = "catia:missing-value-block".to_string();
    canonical_namespace
        .set_arena("value_schema_selections", &orphaned_selections)
        .expect("store orphaned value selection");
    assert!(matches!(
        crate::native::CatiaNative::load(&canonical_namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed value-block view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut invalid_length = native.clone();
    invalid_length.value_blocks[0].declared_len += 1;
    assert_rejected(invalid_length);

    let mut invalid_payload = native.clone();
    invalid_payload.value_blocks[0].payload.push(0x80);
    assert_rejected(invalid_payload);

    let mut invalid_fields = native.clone();
    invalid_fields.value_blocks[0].fields.clear();
    assert_rejected(invalid_fields);

    let mut invalid_selections = native;
    assert!(!invalid_selections.value_blocks[0]
        .schema_selections
        .is_empty());
    invalid_selections.value_blocks[0].schema_selections.clear();
    assert_rejected(invalid_selections);
}

#[test]
fn native_load_rejects_noncanonical_graph_catalog_views() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    assert!(native.object_graphs[0].catalog_byte_offset.is_some());
    assert!(native.object_graphs[0].catalog.is_some());
    assert!(native.object_graphs[0].records[0].class_name.is_some());
    assert!(native.object_graphs[0].records[0].class_entry.is_some());
    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed graph-catalog view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut missing_catalog_link = native.clone();
    missing_catalog_link.object_graphs[0].catalog_byte_offset = None;
    assert_rejected(missing_catalog_link);

    let mut missing_catalog_identity = native.clone();
    missing_catalog_identity.object_graphs[0].catalog = None;
    assert_rejected(missing_catalog_identity);

    let mut invalid_class = native.clone();
    invalid_class.object_graphs[0].records[0].class_name = Some("WrongClass".to_string());
    assert_rejected(invalid_class);

    let mut invalid_class_entry = native;
    invalid_class_entry.object_graphs[0].records[0].class_entry = None;
    assert_rejected(invalid_class_entry);
}

#[test]
fn native_load_rejects_invalid_source_identities_and_extents() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_value_block());
    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed source identity");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut invalid_catalog_extent = native.clone();
    invalid_catalog_extent.catalogs[0].byte_len += 1;
    assert_rejected(invalid_catalog_extent);

    let mut invalid_entry_offset = native.clone();
    invalid_entry_offset.catalogs[0].entries[0].byte_offset += 1;
    assert_rejected(invalid_entry_offset);

    let mut invalid_record_offset = native.clone();
    invalid_record_offset.object_graphs[0].records[0].byte_offset += 1;
    assert_rejected(invalid_record_offset);

    let mut invalid_value_id = native;
    invalid_value_id.value_blocks[0].id = "catia:outer:value-block#wrong".to_string();
    assert_rejected(invalid_value_id);

    let mut invalid_alias_id = crate::native::CatiaNative::decode(&surface_alias_stream());
    invalid_alias_id.alias_rows[0].id = "catia:outer:alias-row#wrong".to_string();
    assert_rejected(invalid_alias_id);
}

#[test]
fn native_store_paths_write_the_current_schema_version() {
    let borrowed = crate::native::CatiaNative {
        version: 1,
        ..crate::native::CatiaNative::default()
    };
    let mut borrowed_namespace = cadmpeg_ir::NativeNamespace::default();
    borrowed
        .store(&mut borrowed_namespace)
        .expect("store borrowed CATIA namespace");
    assert_eq!(
        borrowed_namespace.version,
        crate::native::CATIA_NATIVE_VERSION
    );

    let owned = crate::native::CatiaNative {
        version: 1,
        ..crate::native::CatiaNative::default()
    };
    let mut owned_namespace = cadmpeg_ir::NativeNamespace::default();
    owned
        .store_owned(&mut owned_namespace)
        .expect("store owned CATIA namespace");
    assert_eq!(owned_namespace.version, crate::native::CATIA_NATIVE_VERSION);
}

#[test]
fn native_load_restores_segment_source_order_and_validates_retained_views() {
    let mut bytes = Vec::new();
    for index in 0..12 {
        bytes.extend(external_reference_segment(&format!(
            "Support{index}.CATPart"
        )));
    }
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store indexed FINJPL segments");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load indexed FINJPL segments");
    assert_eq!(
        loaded
            .finjpl_segments
            .iter()
            .map(|segment| segment.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:finjpl#{index}"))
            .collect::<Vec<_>>()
    );
    assert!(loaded
        .finjpl_segments
        .windows(2)
        .all(|pair| pair[0].byte_offset < pair[1].byte_offset));
    assert_eq!(
        loaded
            .external_references
            .iter()
            .map(|reference| reference.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:external-reference#{index}"))
            .collect::<Vec<_>>()
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed FINJPL view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };
    let mut invalid_length = native.clone();
    invalid_length.finjpl_segments[0].byte_len += 1;
    assert_rejected(invalid_length);
    let mut invalid_family = native.clone();
    invalid_family.finjpl_segments[0].family = "other".to_string();
    assert_rejected(invalid_family);
    let mut missing_reference = native.clone();
    missing_reference.external_references.pop();
    assert_rejected(missing_reference);
    let mut invalid_target = native.clone();
    invalid_target.external_references[0].target = "Wrong.CATPart".to_string();
    assert_rejected(invalid_target);
    let mut invalid_reference_offset = native.clone();
    invalid_reference_offset.external_references[0].byte_offset += 1;
    assert_rejected(invalid_reference_offset);
    let mut invalid_type = native;
    invalid_type.finjpl_segments[0].type_word ^= 1;
    assert_rejected(invalid_type);

    let mut invalid_offset = crate::native::CatiaNative::decode(&bytes);
    invalid_offset.finjpl_segments[1].byte_offset += 1;
    assert_rejected(invalid_offset);
}

#[test]
fn native_load_derives_complete_source_ordered_preview_views() {
    let mut bytes = Vec::new();
    for _ in 0..12 {
        bytes.extend(summary_preview_segment());
    }
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.preview_images.len(), 12);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store indexed preview views");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load indexed preview views");
    assert_eq!(
        loaded
            .preview_images
            .iter()
            .map(|preview| preview.id.clone())
            .collect::<Vec<_>>(),
        (0..12)
            .map(|index| format!("catia:outer:preview#{index}"))
            .collect::<Vec<_>>()
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed preview view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };
    let mut missing = native.clone();
    missing.preview_images.pop();
    assert_rejected(missing);
    let mut invalid_width = native.clone();
    invalid_width.preview_images[0].width += 1;
    assert_rejected(invalid_width);
    let mut invalid_data = native;
    invalid_data.preview_images[0].data[0] = 0;
    assert_rejected(invalid_data);
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
    assert_eq!(native.catalogs[0].entries[6].value, "GSMLoft");
    assert_eq!(native.catalogs[0].entries[7].value, "GSMPointBetweenValues");
    assert_eq!(native.catalogs[0].entries[8].value, "GSMPlaneAngle");
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
    assert_eq!(
        native.value_blocks[0].byte_offset,
        u64::try_from(16 + object_graph_stream().len()).unwrap()
    );
    assert_eq!(native.value_blocks[0].byte_len, 16);
    assert_eq!(native.value_blocks[0].catalog, native.catalogs[0].id);
    assert_eq!(
        native.value_blocks[0].object_graph.as_deref(),
        Some(native.object_graphs[0].id.as_str())
    );
    assert_eq!(
        native.value_blocks[0].payload,
        [0x81, 0x83, 0x32, 4, 0, 0, 0, 0x83, 0x82]
    );
    assert_eq!(native.value_blocks[0].schema_selections.len(), 1);
    assert_eq!(native.value_blocks[0].schema_selections[0].ordinal, 4);
    assert_eq!(
        native.value_blocks[0].schema_selections[0].entry.as_deref(),
        Some(native.catalogs[0].entries[4].id.as_str())
    );
    assert_eq!(
        native.value_blocks[0].schema_selections[0].name.as_deref(),
        Some("VPGlobal")
    );
    assert_eq!(
        native.value_blocks[0].schema_selections[0].encoded_value,
        [
            crate::value_block::ValueField::Atom {
                value: 3,
                width: 1,
                offset: 7,
            },
            crate::value_block::ValueField::Atom {
                value: 2,
                width: 1,
                offset: 8,
            },
        ]
    );
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Attribute
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("1 visualization value block(s)")
            && loss
                .message
                .contains("1 schema-selected presentation value(s)")
    }));
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("neutral features")
            && !loss.message.contains("value block")
    }));
}

#[test]
fn visualization_values_do_not_assert_missing_design_intent() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_visualization_values_only()),
            &DecodeOptions::default(),
        )
        .expect("decode visualization-only values");

    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Attribute
            && loss.message.contains("schema-selected presentation value")
    }));
    assert!(decoded
        .report
        .losses
        .iter()
        .all(|loss| loss.category != cadmpeg_ir::report::LossCategory::DesignIntent));
}

#[test]
fn decode_does_not_promote_field_class_names_to_features() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_design_class("Groove")),
            &DecodeOptions::default(),
        )
        .expect("decode field-class vocabulary");

    assert!(decoded.ir.model.features.is_empty());
    let native = crate::native::CatiaNative::load(
        decoded
            .ir
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load retained field-class vocabulary");
    assert_eq!(
        native.design_objects[0]
            .field_classes
            .iter()
            .map(|class| class.name.as_str())
            .collect::<Vec<_>>(),
        ["CurrentFeature", "Groove"]
    );
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.message.contains("neutral features")
    }));
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
fn outer_alias_parser_classifies_both_ordinal_linked_storage_leads() {
    use crate::object_graph::AliasLead;

    for (lead, expected) in [
        (0x8eu32, AliasLead::E5LinkedSurfaceStorage),
        (0x8fu32, AliasLead::OrdinalLinkedStorage8f),
    ] {
        let mut bytes = surface_alias_stream();
        bytes[..4].copy_from_slice(&lead.to_le_bytes());
        let [row] = crate::object_graph::surface_aliases(&bytes)
            .try_into()
            .expect("one ordinal-linked alias row");
        assert_eq!(row.lead, expected);
        assert_eq!(row.entity_record_ordinal, 7);
    }
}

#[test]
fn outer_surface_alias_parser_retains_zero_low_tag_bits() {
    let mut bytes = surface_alias_stream();
    bytes[8..12].copy_from_slice(&0xab00_0000u32.to_le_bytes());

    let rows = crate::object_graph::surface_aliases(&bytes);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tag, 0);
    assert_eq!(rows[0].tag_raw, 0xab00_0000);
    assert_eq!(rows[0].entity_record_ordinal, 7);
    assert_eq!((rows[0].f2, rows[0].f3), (0x1122_3344, 0x5566_7788));
}

#[test]
fn outer_surface_alias_parser_requires_the_lead_word() {
    let bytes = surface_alias_stream();
    assert!(crate::object_graph::surface_aliases(&bytes[4..]).is_empty());
}

#[test]
fn native_namespace_retains_surface_alias_core() {
    let native = crate::native::CatiaNative::decode(&surface_alias_stream());
    let [row] = native.alias_rows.as_slice() else {
        panic!("one alias row")
    };
    assert_eq!(row.byte_offset, 4);
    assert_eq!(row.tag, 0x0012_3456);
    assert_eq!(row.tag_raw, 0xab12_3456);
    assert_eq!(row.entity_record_ordinal, 7);
    assert!(row.design_object.is_none());
    assert_eq!((row.f2, row.f3), (0x1122_3344, 0x5566_7788));

    let mut invalid = native;
    invalid.alias_rows[0].design_object = Some("catia:missing-design-object".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store unresolved alias with a design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_alias_f1_resolves_primary_object_record() {
    let graph = object_graph_stream();
    let mut alias = surface_alias_stream();
    alias[13..16].copy_from_slice(&[3, 0, 2]);
    let mut bytes = graph;
    bytes.extend(alias);

    let native = crate::native::CatiaNative::decode(&bytes);
    let [row] = native.alias_rows.as_slice() else {
        panic!("one alias row")
    };
    assert_eq!(
        row.object_graph.as_deref(),
        Some("catia:outer:object-graph#0000000000")
    );
    assert_eq!(
        row.object_record.as_deref(),
        Some("catia:outer:object-record#0000000028")
    );
    let record = &native.object_graphs[0].records[1];
    assert_eq!(row.design_object, record.design_object);

    let mut invalid = native;
    invalid.alias_rows[0].design_object = Some("catia:missing-design-object".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid alias design-object link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
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
fn standard_decode_transfers_consolidated_guide_curve() {
    let mut bytes = standard_catpart();
    bytes.splice(16..16, a5_guide_curve_stream());
    let file_len = u32::try_from(bytes.len()).expect("guide fixture length");
    bytes[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode guide fixture");
    let guide = decoded
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0.starts_with("catia:guide:curve#"))
        .expect("typed guide curve");
    let CurveGeometry::Nurbs(nurbs) = &guide.geometry else {
        panic!("guide curve must be NURBS");
    };
    assert_eq!(nurbs.degree, 5);
    assert_eq!(nurbs.control_points.first().unwrap().x, 0.0);
    assert_eq!(nurbs.control_points.last().unwrap().z, 4.0);
}

#[test]
fn decode_object_stream_transfers_a8_rolling_ball_jet() {
    let file = object_main_catpart(&a8_freeform_curve_stream());
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::FloatPackedInnerNoFbb
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode rolling-ball object stream");
    let [procedural] = decoded.ir.model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet {
        degree,
        knots,
        multiplicities,
        sites,
    } = &procedural.definition
    else {
        panic!("rolling-ball jet");
    };
    assert_eq!(*degree, 5);
    assert_eq!(knots, &[0.0, 1.0]);
    assert_eq!(multiplicities, &[6, 6]);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[1].first_limit, Point3::new(2.0, 0.0, 0.0));
    assert_eq!(sites[1].angle, std::f64::consts::FRAC_PI_2);
    let provenance = &decoded.source_fidelity.annotations.provenance[&procedural.id.0];
    assert_eq!(
        decoded.source_fidelity.annotations.streams[provenance.stream as usize],
        "catia:object_stream_a8_03_32"
    );
    let tag = provenance
        .tag
        .as_deref()
        .expect("rolling-ball provenance tag");
    assert!(tag.contains("object_id:12345678"));
    assert!(tag.contains("multiplicities:[6, 6]"));
    assert_eq!(
        decoded.ir.model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:12345678"))
    );
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
    assert_eq!(
        result.ir.model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
}

#[test]
fn decode_float_packed_stream_transfers_reference_closed_b5_topology() {
    let stream = b5_closed_triangle_stream();
    crate::families::b5::graph::parse(&stream).expect("generated B5 topology");
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
    assert!(result.ir.model.surfaces.iter().all(|surface| {
        surface.source_object.as_ref().is_some_and(|source| {
            source.format == "catia" && source.object_id.starts_with("cgm-surface:")
        })
    }));
    assert!(result.ir.model.curves.iter().all(|curve| {
        curve.source_object.as_ref().is_some_and(|source| {
            source.format == "catia" && source.object_id.starts_with("cgm-edge:")
        })
    }));
    assert_eq!(result.ir.model.procedural_curves.len(), 3);
    assert!(result.ir.model.procedural_curves.iter().all(|curve| {
        matches!(
            curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                ref context,
                ..
            } if context.sides[0].surface.is_some()
                && context.sides[0].pcurve.is_some()
                && context.sides[1].surface.is_none()
        )
    }));
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.pcurves.len(), 3);
    assert!(result
        .ir
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.parameter_range == Some([0.0, 1.0])));
    assert!(result.report.losses.iter().all(|loss| {
        !matches!(
            loss.category,
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
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
    assert_eq!(
        result.ir.model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
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
    crate::families::e5::graph::parse_topology(&stream).expect("generated E5 topology");
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
    assert_eq!(result.ir.model.curves.len(), 4);
    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    assert!(matches!(
        result.ir.model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
            family: cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric,
            ..
        }
    ));
    assert!(result
        .ir
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_some() && edge.param_range.is_some()));
    assert!(result.report.losses.iter().all(|loss| {
        loss.category != cadmpeg_ir::report::LossCategory::Topology
            || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    assert!(result.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Topology
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("two trailing orientation signs")
    }));

    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_e5_stream_binds_file_level_vertex_run() {
    let mut stream = e5_torus_topology_stream();
    let vertex_start = stream
        .windows(3)
        .position(|bytes| bytes == [0x05, 0x08, 0x01])
        .expect("E5 vertex run");
    let vertex_bytes = stream
        .drain(vertex_start..vertex_start + 4 * 15)
        .collect::<Vec<_>>();

    stream.extend_from_slice(b"FINJPL  ");
    stream.extend_from_slice(&0x0000_0080u32.to_be_bytes());
    stream.extend_from_slice(&vertex_bytes);
    let file = object_main_catpart(&stream);
    let vertex_file_start = file
        .windows(vertex_bytes.len())
        .position(|bytes| bytes == vertex_bytes)
        .expect("file-level E5 vertex run");

    let record_range = crate::container::e5_record_stream(&file).expect("coherent E5 walk");
    assert!(!record_range.contains(&vertex_file_start));
    assert!(crate::families::e5::records::e5_vertices(&file[record_range], 4).is_empty());
    assert_eq!(crate::families::e5::records::e5_vertices(&file, 4).len(), 4);
    let scan = crate::container::scan_bytes(file.clone());
    assert_eq!(scan.variant, Variant::E5Stream);

    let result = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("E5 decode");
    assert_eq!(result.ir.model.points.len(), 4);
    assert_eq!(result.ir.model.vertices.len(), 4);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 4);
}

#[test]
fn container_only_stops_before_geometry() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let opts = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
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
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_every_entity_has_v1_annotation(
        &container_only.ir,
        &container_only.source_fidelity.annotations,
    );
}
