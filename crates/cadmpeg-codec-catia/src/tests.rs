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
fn standard_mesh_resolver_derives_trim_components_from_local_ports() {
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
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[0, 3]]];

    let (topology, assignment) =
        crate::solve::mesh_quotient::parse_standard_mesh_endpoint_candidates(
            &bytes,
            &[[0, 0]; 4],
            &candidates,
        )
        .expect("trim occurrence endpoint quotient");

    assert_eq!(assignment, vec![0, 1, 2, 3]);
    assert_eq!(
        topology.edge_vertices().expect("resolved edge endpoints"),
        vec![[0, 1], [1, 2], [2, 3], [3, 0]]
    );
}

#[test]
fn standard_mesh_candidate_quotient_defers_occurrence_direction() {
    let candidates = vec![vec![[1, 2]], vec![[0, 3]], vec![[0, 1]]];
    let local_ports = [[0, 1], [2, 3], [4, 5]];
    let prematurely_oriented_ports = [[0, 1], [1, 2], [3, 1]];

    assert!(
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 4, &local_ports).is_some()
    );
    assert!(crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        4,
        &prematurely_oriented_ports,
    )
    .is_none());
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
    let boundary_context =
        crate::solve::missing_edge::StandardMeshBoundaryContext::parse(&bytes, &[[0, 0]; 4])
            .expect("parsed boundary context");
    assert_eq!(
        crate::solve::missing_edge::standard_mesh_boundary_assignments_from_context(
            &boundary_context,
            None,
        )
        .expect("assignments from parsed context"),
        boundaries,
    );
    let singleton_endpoints = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[0, 3]]];
    assert_eq!(
        crate::solve::missing_edge::standard_mesh_boundary_assignments_from_context(
            &boundary_context,
            Some(&singleton_endpoints),
        ),
        crate::solve::missing_edge::standard_mesh_boundary_assignments(
            &bytes,
            &[[0, 0]; 4],
            Some(&singleton_endpoints),
        ),
    );
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

fn outer_container_object_graph_catpart() -> (Vec<u8>, u64) {
    outer_container_catpart(&object_graph_stream())
}

fn outer_container_catpart(stream: &[u8]) -> (Vec<u8>, u64) {
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

fn zero_entity_cylinder_parametric_support_catpart() -> Vec<u8> {
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
    for (prefix, values) in [
        (0x05, vec![2.0f64, 3.0]),
        (0x09, vec![11.0, 4.0, 5.0]),
        (0x0d, vec![1.0, 2.0, 3.0, 4.0, 5.0]),
        (0x11, vec![12.0, 6.0, 7.0]),
    ] {
        let length = 2 + 8 * values.len();
        bytes.extend_from_slice(&[
            0xb2,
            0x03,
            0x18,
            u8::try_from(length).unwrap(),
            0x05,
            prefix,
        ]);
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
    record.extend_from_slice(&owner_numeric_tail());
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
    record.extend_from_slice(&owner_numeric_tail());
    record
}

fn owner_numeric_tail() -> Vec<u8> {
    let mut tail = vec![0x84, 0x41, 0xbb, 0x05, 0x0d];
    for value in [-0.0f64, 4.5, 12.25, 7.0] {
        tail.extend_from_slice(&value.to_le_bytes());
    }
    tail.push(0x01);
    for value in [-2.0f32, 1.0, 3.5, 4.0, 5.25, 6.0] {
        tail.extend_from_slice(&value.to_le_bytes());
    }
    tail
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
    record.extend_from_slice(&[
        0x85, 0x05, 0x08, 0x7f, 0x05, 0x08, 0x14, 0x03, 0xe5, 0xdd, 0x05, 0x01, 0x01, 0x05, 0x03,
        0x11,
    ]);
    record.extend_from_slice(&le_f64(1.5));
    record.extend_from_slice(&le_f64(std::f64::consts::FRAC_PI_4));
    record
}

pub(crate) fn b2_cone_face_parameter_point_stream() -> Vec<u8> {
    let mut bytes = b2_cone_face_stream();
    bytes.extend_from_slice(&b2_parameter_point_stream());
    bytes
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

pub(crate) fn b2_line_profile_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x0e, 0x48, 0x05];
    for value in [1.0f64, 2.0, 3.0, 0.0, 0.6, 0.8, 1.0, -4.0, 9.0] {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn zero_entity_support_stream() -> Vec<u8> {
    let mut plane = vec![0u8; 0x6a + 12];
    plane[..4].copy_from_slice(&[0xa9, 0x03, 0x27, 0x6a]);
    for (offset, value) in [
        (14, 1.0f64),
        (22, 2.0),
        (30, 3.0),
        (38, 1.0),
        (46, 0.0),
        (54, 0.0),
        (62, 0.0),
        (70, 1.0),
        (78, 0.0),
    ] {
        plane[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut support = vec![0u8; 0x71 + 12];
    support[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x71]);
    support[12] = 0x10;
    support[13..17].copy_from_slice(&42u32.to_le_bytes());
    support[67..75].copy_from_slice(&0.0f64.to_le_bytes());
    support[75..83].copy_from_slice(&1.0f64.to_le_bytes());
    support[83] = 0x10;
    support[84..88].copy_from_slice(&2u32.to_le_bytes());
    support[88] = 0x10;
    support[89..93].copy_from_slice(&2u32.to_le_bytes());
    for (offset, value) in [(93, -2.0f64), (101, 4.0), (109, 6.0), (117, 8.0)] {
        support[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    plane.extend(support);
    plane
}

pub(crate) fn zero_entity_face_support_stream() -> Vec<u8> {
    let mut stream = zero_entity_support_stream();
    let mut face = vec![0u8; 0x0c + 12];
    face[..4].copy_from_slice(&[0xa9, 0x03, 0x5f, 0x0c]);
    face[7] = 0x10;
    face[8..12].copy_from_slice(&1u32.to_le_bytes());
    face[12] = 0x82;
    face[13] = 0x10;
    face[14..18].copy_from_slice(&10u32.to_le_bytes());
    face[18] = 0x10;
    face[19..23].copy_from_slice(&3u32.to_le_bytes());
    face[23] = 0x05;
    stream.extend(face);
    stream
}

pub(crate) fn zero_entity_face_loop_support_stream() -> Vec<u8> {
    let mut stream = zero_entity_face_support_stream();
    let mut loop_record = vec![0u8; 0x14 + 12];
    loop_record[..4].copy_from_slice(&[0xa9, 0x03, 0x62, 0x14]);
    loop_record[12] = 0x83;
    for (index, value) in [6u32, 1, 7].into_iter().enumerate() {
        let offset = 13 + index * 5;
        loop_record[offset] = 0x10;
        loop_record[offset + 1..offset + 5].copy_from_slice(&value.to_le_bytes());
    }
    loop_record[28..].copy_from_slice(&[0x81, 0x41, 0x07, 0x01]);
    stream.extend(loop_record);
    stream
}

pub(crate) fn zero_entity_topology_stream() -> Vec<u8> {
    let write_tagged_u32 = |record: &mut [u8], at: usize, value: u32| {
        record[at] = 0x10;
        record[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
    };
    let mut edge_stride = vec![0u8; 38];
    edge_stride[..4].copy_from_slice(&[0xa9, 0x03, 0x5e, 0x1a]);
    for (index, value) in [1, 5, 7, 8, 4, 3].into_iter().enumerate() {
        write_tagged_u32(&mut edge_stride, 7 + index * 5, value);
    }
    edge_stride[37] = 0x21;

    let mut header = vec![0u8; 0x69 + 12];
    header[..4].copy_from_slice(&[0xa9, 0x03, 0x25, 0x69]);
    write_tagged_u32(&mut header, 7, 1);
    header[12] = 0x82;
    write_tagged_u32(&mut header, 13, 100);
    write_tagged_u32(&mut header, 18, 200);

    let make_use = |side, allocations: [u32; 2]| {
        let mut record = vec![0u8; 0x38 + 12];
        record[..4].copy_from_slice(&[0xa9, 0x03, 0x06, 0x38]);
        write_tagged_u32(&mut record, 7, 1);
        record[12] = 0x83;
        write_tagged_u32(&mut record, 13, side);
        write_tagged_u32(&mut record, 18, allocations[0]);
        write_tagged_u32(&mut record, 23, allocations[1]);
        record
    };

    let mut incidence = vec![0u8; 0x10 + 12];
    incidence[..4].copy_from_slice(&[0xa9, 0x03, 0x05, 0x10]);
    write_tagged_u32(&mut incidence, 7, 1);
    incidence[12] = 0x83;
    for (index, value) in [1, 2, 5].into_iter().enumerate() {
        write_tagged_u32(&mut incidence, 13 + index * 5, value);
    }

    let mut vertex = vec![0u8; 0x06 + 12];
    vertex[..4].copy_from_slice(&[0xa9, 0x03, 0x5d, 0x06]);
    write_tagged_u32(&mut vertex, 7, 1);
    write_tagged_u32(&mut vertex, 12, 1);
    let mut support0 = vec![0u8; 0x18 + 12];
    support0[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x18]);
    let mut support1 = vec![0u8; 0x18 + 12];
    support1[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x18]);

    edge_stride
        .into_iter()
        .chain(header)
        .chain(make_use(1, [101, 201]))
        .chain(make_use(2, [102, 202]))
        .chain(incidence)
        .chain(vertex)
        .chain(support0)
        .chain(support1)
        .collect()
}

pub(crate) fn zero_entity_ownership_stream(face_count: u8) -> Vec<u8> {
    assert!(face_count != 0 && face_count < 0x80);
    let write_tagged_u32 = |record: &mut Vec<u8>, value: u32| {
        record.push(0x10);
        record.extend_from_slice(&value.to_le_bytes());
    };
    let mut face_roster = vec![0xa9, 0x03, 0x61, 0x42, 0, 0, 0];
    write_tagged_u32(&mut face_roster, 1);
    face_roster.push(0x80 + face_count);
    for slot in (1..=u32::from(face_count)).rev() {
        write_tagged_u32(&mut face_roster, slot);
    }
    face_roster.extend_from_slice(&[0x00, 0x01, 0xc0, 0xff, 0xff, 0x3f, 0, 0, 0, 0, 0x03]);

    let mut shell = vec![0xa9, 0x03, 0x60, 0x06, 0, 0, 0];
    write_tagged_u32(&mut shell, 1);
    shell.push(0x81);
    write_tagged_u32(&mut shell, 1);

    let mut body = vec![0xa9, 0x03, 0x65, 0x08, 0, 0, 0];
    write_tagged_u32(&mut body, 1);
    body.push(0x81);
    write_tagged_u32(&mut body, 1);
    body.extend_from_slice(&[0x05, 0x0d]);

    face_roster.into_iter().chain(shell).chain(body).collect()
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

pub(crate) fn b2_resolved_revolution_stream() -> Vec<u8> {
    let mut circle = b2_circle_stream();
    circle[32..40].copy_from_slice(&le_f64(-4.0));
    circle[40..48].copy_from_slice(&le_f64(9.0));
    circle.extend_from_slice(&b2_revolution_stream());
    circle
}

pub(crate) fn b2_torus_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x2b, 200, 0x05];
    let mut values = [0.0f64; 25];
    values[0..3].copy_from_slice(&[1.0, 2.0, 3.0]);
    values[3..6].copy_from_slice(&[1.0, 0.0, 0.0]);
    values[6..9].copy_from_slice(&[0.0, 1.0, 0.0]);
    values[9..12].copy_from_slice(&[0.0, 0.0, 1.0]);
    values[12] = 7.0;
    values[13] = 2.0;
    values[14..18].copy_from_slice(&[
        std::f64::consts::FRAC_PI_2,
        3.0 * std::f64::consts::FRAC_PI_2,
        0.0,
        std::f64::consts::TAU,
    ]);
    values[18..22].copy_from_slice(&[
        0.0,
        std::f64::consts::PI,
        -std::f64::consts::FRAC_PI_2,
        3.0 * std::f64::consts::FRAC_PI_2,
    ]);
    values[22] = 14.0;
    values[23] = 4.0;
    for value in values {
        record.extend_from_slice(&le_f64(value));
    }
    record
}

pub(crate) fn b2_sphere_stream() -> Vec<u8> {
    let mut record = vec![0xb2, 0x03, 0x2a, 152, 0x05];
    let mut values = [0.0f64; 19];
    values[0..3].copy_from_slice(&[1.0, 2.0, 3.0]);
    values[3..6].copy_from_slice(&[5.0, 0.0, 0.0]);
    values[6..9].copy_from_slice(&[0.0, 5.0, 0.0]);
    values[9..12].copy_from_slice(&[0.0, 0.0, 5.0]);
    values[12] = 5.0;
    values[13..17].copy_from_slice(&[-2.0, 4.0, -1.0, std::f64::consts::FRAC_PI_2]);
    values[17] = values[12];
    values[18] = values[12] * ((values[13] + values[14]) * 0.5 - std::f64::consts::PI);
    for value in values {
        record.extend_from_slice(&le_f64(value));
    }
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

pub(crate) fn a5_nurbs_pair_bound_edge_stream(duplicate_first_surface: bool) -> Vec<u8> {
    let p0 = [1.0, 2.0, 3.0];
    let p1 = [4.0, 5.0, 6.0];
    let mut bytes = a5_pcurve_stream_with_uv([0.0, 1.0], [0.0, 0.0]);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv([0.0, 0.0], [0.0, 1.0]));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    let first_surface = a5_surface_stream_with_poles([
        p0,
        [p0[0], p0[1], p0[2] + 1.0],
        p1,
        [p1[0], p1[1], p1[2] + 1.0],
    ]);
    bytes.extend_from_slice(&first_surface);
    bytes.extend_from_slice(&a5_surface_stream_with_poles([
        p0,
        p1,
        [p0[0], p0[1] + 1.0, p0[2]],
        [p1[0], p1[1] + 1.0, p1[2]],
    ]));
    if duplicate_first_surface {
        bytes.extend_from_slice(&first_surface);
    }
    bytes
}

pub(crate) fn b2_circle_stream() -> Vec<u8> {
    let radius = 3.0;
    let mut record = vec![0xb2, 0x03, 0x19, 0x34, 0x05, 0x08, 0x34, 0x12];
    for value in [
        4.0f64,
        -2.0,
        radius,
        0.0,
        2.0 * std::f64::consts::PI * radius,
    ] {
        record.extend_from_slice(&le_f64(value));
    }
    record.push(0x01);
    record.extend_from_slice(&le_f64(0.0));
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

pub(crate) fn b2_range_origin_cylinder_stream() -> Vec<u8> {
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
    let range_origin = (0.0 + 8.0) * 0.5 - std::f64::consts::PI * 4.0;
    record[p + 90..p + 98].copy_from_slice(&le_f64(range_origin));
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
    record[109..117].copy_from_slice(&le_f64(4.0));
    record[117..125].copy_from_slice(&le_f64(0.5));
    record[125..133].copy_from_slice(&le_f64(0.5 + std::f64::consts::PI));
    record[133..141].copy_from_slice(&le_f64(2.0));
    record[141..149].copy_from_slice(&le_f64(8.0));
    record[149..157].copy_from_slice(&le_f64(3.0));
    record[157..165].copy_from_slice(&le_f64(1.0));
    record[173..181].copy_from_slice(&le_f64(0.5 - std::f64::consts::FRAC_PI_2));
    record[181..189].copy_from_slice(&le_f64(0.5 + 3.0 * std::f64::consts::FRAC_PI_2));
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

fn inline_object_graph_record(body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x7c, 0x09];
    bytes.extend_from_slice(&(6_u32 + body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(body);
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

fn entity_table_record(entity_id: u32) -> Vec<u8> {
    entity_table_record_with_value(entity_id, &[])
}

fn entity_table_record_with_value(entity_id: u32, value: &[u8]) -> Vec<u8> {
    entity_table_record_with_definition_and_value(entity_id, &[0x01], value)
}

fn entity_table_record_with_definition_and_value(
    entity_id: u32,
    definition_prefix: &[u8],
    value: &[u8],
) -> Vec<u8> {
    let mut bytes = vec![0x7c, 0x05, 0, 0, 0, 0, 0x00, 0x7c, 0x06];
    bytes.extend_from_slice(
        &u32::try_from(definition_prefix.len() + 11)
            .expect("generated 7C06 length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(definition_prefix);
    bytes.push(0xea);
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&[0x7c, 0x07]);
    bytes.extend_from_slice(
        &u32::try_from(value.len() + 6)
            .expect("generated 7C07 length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value);
    let total_len = u32::try_from(bytes.len()).expect("generated 7C05 length");
    bytes[2..6].copy_from_slice(&total_len.to_le_bytes());
    bytes
}

fn entity_backed_object_graph(records: &[Vec<u8>], entity_ids: &[u32]) -> Vec<u8> {
    assert_eq!(records.len(), entity_ids.len());
    let mut bytes = entity_ids
        .iter()
        .flat_map(|entity_id| entity_table_record(*entity_id))
        .collect::<Vec<_>>();
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(records));
    bytes
}

fn sequential_entity_backed_object_graph(records: &[Vec<u8>]) -> Vec<u8> {
    let entity_ids = (1..=u32::try_from(records.len()).expect("bounded generated entity count"))
        .collect::<Vec<_>>();
    entity_backed_object_graph(records, &entity_ids)
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
    let graph = entity_backed_object_graph(&records, &[1, 2]);
    let mut file = standard_catpart();
    file.splice(16..16, graph);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_nested_design_objects() -> Vec<u8> {
    let records = [
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x83, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x83, 0x84], &[0xfe]),
    ];
    let graph = entity_backed_object_graph(&records, &[1, 2, 3]);
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

fn standard_catpart_with_repeated_reference_schema_selection() -> Vec<u8> {
    let mut payload = vec![0xac, 0xe5];
    payload.extend_from_slice(&59_u32.to_le_bytes());
    payload.extend_from_slice(&[0; 59]);
    payload.extend_from_slice(&[
        0x85, 0xae, 0x84, 0xb0, 0x82, 0x81, 0x81, 0x81, 0x82, 0x82, 0x81, 0x81, 0xd1, 0x80, 0xfe,
    ]);
    let mut stream =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x84], &payload)]);
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "TargetSchema",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_entity_value_schema_selection() -> Vec<u8> {
    let mut value = vec![0x32, 4, 0, 0, 0, 0x87, 0xe6];
    value.extend_from_slice(&12.5_f64.to_bits().to_le_bytes());
    value.extend_from_slice(&[0xe8, 0xe0, 0x0a, 0x37, 0xfe, 0xfe]);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_value(1, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "TargetValue",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_relation_expression(parameter_role: &str) -> Vec<u8> {
    standard_catpart_with_relation_expression_signature(
        parameter_role,
        "#1_ ",
        "(#1_ : #In LENGTH) : LENGTH",
    )
}

fn standard_catpart_with_relation_expression_signature(
    parameter_role: &str,
    placeholder: &str,
    signature: &str,
) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut value = Vec::new();
    for ordinal in 5u32..=10 {
        value.push(0x32);
        value.extend_from_slice(&ordinal.to_le_bytes());
    }
    value.push(0xfe);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_definition_and_value(1, &definition, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        placeholder,
        "#1_ /2-2mm",
        parameter_role,
        signature,
        "opened",
        "RelationExpFct",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_parser_version_relation_expression(
    prefix_role: &str,
    parser_version_role: &str,
) -> Vec<u8> {
    standard_catpart_with_parser_version_relation_expression_roles(
        prefix_role,
        parser_version_role,
        None,
    )
}

fn standard_catpart_with_opened_parser_version_relation_expression(
    prefix_role: &str,
    parser_version_role: &str,
    state_role: &str,
) -> Vec<u8> {
    standard_catpart_with_parser_version_relation_expression_roles(
        prefix_role,
        parser_version_role,
        Some(state_role),
    )
}

fn standard_catpart_with_parser_version_relation_expression_roles(
    prefix_role: &str,
    parser_version_role: &str,
    state_role: Option<&str>,
) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut value = Vec::new();
    for ordinal in 5u32..=10 + u32::from(state_role.is_some()) {
        value.push(0x32);
        value.extend_from_slice(&ordinal.to_le_bytes());
    }
    value.push(0xfe);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_definition_and_value(1, &definition, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    let mut entries = vec![
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        prefix_role,
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2",
        parser_version_role,
        "param",
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : Real\n",
    ];
    entries.extend(state_role);
    entries.push("RelationExpFct");
    stream.extend(catalog_stream(&entries));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_unprefixed_parser_version_relation_expression(
    parser_version_role: &str,
) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut value = Vec::new();
    for ordinal in 5u32..=9 {
        value.push(0x32);
        value.extend_from_slice(&ordinal.to_le_bytes());
    }
    value.push(0xfe);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_definition_and_value(1, &definition, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        "360.0*1 deg/#1_",
        parser_version_role,
        "param",
        "(#1_ : #In Integer) : ANGLE\n",
        "RelationExpFct",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_relation_program_instance(
    program_entity_id: u32,
    repeated_reference_entity_id: u32,
    context_entity_id: u32,
    stored_self_entity_id: u32,
) -> Vec<u8> {
    let mut instance_payload = Vec::new();
    let reference = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x32);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    let atom = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x80);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    reference(&mut instance_payload, 20);
    atom(&mut instance_payload, 3);
    reference(&mut instance_payload, 21);
    atom(&mut instance_payload, 22);
    atom(&mut instance_payload, 0x3d7d_031f);
    atom(&mut instance_payload, 5);
    atom(&mut instance_payload, 89);
    atom(&mut instance_payload, 1_127_154_762);
    reference(&mut instance_payload, 23);
    atom(&mut instance_payload, repeated_reference_entity_id);
    reference(&mut instance_payload, 25);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, repeated_reference_entity_id);
    reference(&mut instance_payload, context_entity_id);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, 21);
    atom(&mut instance_payload, program_entity_id);
    reference(&mut instance_payload, 27);
    atom(&mut instance_payload, stored_self_entity_id);
    instance_payload.push(0xfe);

    standard_catpart_with_relation_program_payload(&[0x12, 0x8a, 0x80], &instance_payload)
}

fn standard_catpart_with_lead54_relation_program_instance(
    program_entity_id: u32,
    repeated_reference_entity_id: u32,
    trailing_entity_id: u32,
    stored_self_entity_id: u32,
) -> Vec<u8> {
    let mut instance_payload = Vec::new();
    let reference = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x32);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    let atom = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x80);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    atom(&mut instance_payload, 244);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, 5);
    atom(&mut instance_payload, program_entity_id);
    atom(&mut instance_payload, 2_142_008_808);
    atom(&mut instance_payload, 247);
    atom(&mut instance_payload, repeated_reference_entity_id);
    reference(&mut instance_payload, 20);
    atom(&mut instance_payload, stored_self_entity_id);
    atom(&mut instance_payload, 249);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, repeated_reference_entity_id);
    reference(&mut instance_payload, 21);
    atom(&mut instance_payload, 2);
    reference(&mut instance_payload, 5);
    atom(&mut instance_payload, trailing_entity_id);
    atom(&mut instance_payload, 129);
    instance_payload.push(0xfe);

    standard_catpart_with_relation_program_payload(
        &[0x54, 0x01, 0x82, 0x80, 0x81],
        &instance_payload,
    )
}

fn standard_catpart_with_relation_program_payload(head: &[u8], instance_payload: &[u8]) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut expression_value = Vec::new();
    for ordinal in 5_u32..=10 {
        expression_value.push(0x32);
        expression_value.extend_from_slice(&ordinal.to_le_bytes());
    }
    expression_value.push(0xfe);

    let mut stream =
        entity_table_record_with_definition_and_value(1, &definition, &expression_value);
    stream.extend(entity_table_record(2));
    stream.push(0xde);
    stream.extend(object_graph_from_records(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
        object_graph_record(head, instance_payload),
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        "Boolean",
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2",
        "ParserVersion",
        "param",
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : Real\n",
        "RelationExpFct",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_configuration_incidences(
    configuration_schema_ordinal: u32,
    second_configuration_entity_id: u32,
    row_successor_entity_id: u32,
) -> Vec<u8> {
    let reference = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x32);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    let atom = |payload: &mut Vec<u8>, value: u32| {
        payload.push(0x80);
        payload.extend_from_slice(&value.to_le_bytes());
    };
    let mut configuration_payload = Vec::new();
    reference(&mut configuration_payload, configuration_schema_ordinal);
    atom(&mut configuration_payload, 2);
    reference(&mut configuration_payload, second_configuration_entity_id);
    atom(&mut configuration_payload, 129);
    configuration_payload.push(0xfe);
    let mut row_payload = Vec::new();
    atom(&mut row_payload, 250);
    atom(&mut row_payload, row_successor_entity_id);
    row_payload.push(0xfe);

    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let mut value = Vec::new();
    for ordinal in 8_u32..=13 {
        reference(&mut value, ordinal);
    }
    value.push(0xfe);
    let mut stream = entity_table_record_with_definition_and_value(5, &definition, &value);
    stream.extend(entity_table_record(6));
    stream.extend(entity_table_record(7));
    stream.push(0xde);
    stream.extend(object_graph_from_records(&[
        object_graph_record(&[0x12, 0x87, 0x85], &configuration_payload),
        object_graph_record(&[0x12, 0x87, 0x86], &row_payload),
        object_graph_record(&[0x12, 0x87, 0x87], &[0xfe]),
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        "Configuration",
        "configrow",
        "body",
        "Boolean",
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2",
        "ParserVersion",
        "param",
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : Real\n",
        "RelationExpFct",
        "opened",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_configuration_row_chain() -> Vec<u8> {
    let row_payload = |successor: u32| {
        let mut payload = vec![0x80];
        payload.extend_from_slice(&250_u32.to_le_bytes());
        payload.push(0x80);
        payload.extend_from_slice(&successor.to_le_bytes());
        payload.push(0xfe);
        payload
    };
    let mut stream = entity_table_record(5);
    stream.extend(entity_table_record(6));
    stream.extend(entity_table_record(7));
    stream.extend(entity_table_record(8));
    stream.push(0xde);
    stream.extend(object_graph_from_records(&[
        object_graph_record(&[0x12, 0x89, 0x85], &row_payload(6)),
        object_graph_record(&[0x12, 0x89, 0x85], &row_payload(7)),
        object_graph_record(&[0x12, 0x89, 0x85], &row_payload(8)),
        object_graph_record(&[0x12, 0x89, 0x86], &[0xfe]),
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "body",
        "configrow",
        "body",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_parameter_value(suffix: &[u8]) -> Vec<u8> {
    standard_catpart_with_two_selector_value("Thickness", "#1_ /2", suffix)
}

fn standard_catpart_with_two_selector_value(first: &str, second: &str, suffix: &[u8]) -> Vec<u8> {
    let value = [0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0, 0xfe];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut entity = entity_table_record_with_definition_and_value(1, &[0x01], &value);
    entity[6] = 2;
    entity.extend_from_slice(suffix);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        first,
        second,
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_definition_value(
    definition: &[u8],
    value: &[u8],
    suffix: &[u8],
) -> Vec<u8> {
    let records = [object_graph_record(&[0x16, 0x84, 0x81, 0x81], &[0xfe])];
    let mut entity = entity_table_record_with_definition_and_value(1, definition, value);
    entity[6] = 2;
    entity.extend_from_slice(suffix);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Thickness",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_definition_chain_value(suffix: &[u8]) -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0];
    let records = [object_graph_record(&[0x16, 0x84, 0x81, 0x81], &[0xfe])];
    let mut entity = entity_table_record_with_definition_and_value(1, &definition, &[0xfe]);
    entity[6] = 2;
    entity.extend_from_slice(suffix);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "FeatureFEDGE",
        "Real",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_unassigned_definition_chain_value() -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0];
    let records = [object_graph_record(
        &[0x16, 0x84, 0x80, 66, 23, 0, 0, 0x80, 0x81, 25, 0, 0],
        &[0xfe],
    )];
    let mut entity = entity_table_record_with_definition_and_value(1, &definition, &[0xfe]);
    entity[6] = 2;
    entity.extend_from_slice(&[0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe7]);
    let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
    entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
    let mut stream = entity;
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "FeatureFEDGE",
        "Real",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_two_definition_chain_values() -> Vec<u8> {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0];
    let suffix = |value: u8| [0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x80 + value];
    let mut stream = Vec::new();
    for (entity_id, value) in [(1_u32, 1_u8), (2_u32, 2_u8)] {
        let mut entity =
            entity_table_record_with_definition_and_value(entity_id, &definition, &[0xfe]);
        entity[6] = 2;
        entity.extend_from_slice(&suffix(value));
        let entity_len = u32::try_from(entity.len()).expect("bounded entity record");
        entity[2..6].copy_from_slice(&entity_len.to_le_bytes());
        stream.extend(entity);
    }
    stream.push(0xde);
    stream.extend(object_graph_from_records(&[
        object_graph_record(&[0x16, 0x84, 0x81, 0x81], &[0xfe]),
        object_graph_record(&[0x16, 0x84, 0x82, 0x81], &[0xfe]),
    ]));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "FeatureFEDGE",
        "Real",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_formula_relation(
    parameter_entity_id: u8,
    duplicate_binding: bool,
) -> Vec<u8> {
    standard_catpart_with_typed_formula_relation(
        parameter_entity_id,
        duplicate_binding,
        "LENGTH",
        "LENGTH",
        35.0,
        33.0,
        "#1_ /2-2mm",
    )
}

fn standard_catpart_with_typed_formula_relation(
    parameter_entity_id: u8,
    duplicate_binding: bool,
    input_type: &str,
    result_type: &str,
    input_value: f64,
    output_value: f64,
    source_expression: &str,
) -> Vec<u8> {
    standard_catpart_with_typed_formula_inputs(
        parameter_entity_id,
        duplicate_binding,
        &[("#1_", input_type, "Thickness", "#1_ /2", input_value)],
        result_type,
        Some(output_value),
        source_expression,
    )
}

fn standard_catpart_with_typed_formula_inputs(
    parameter_entity_id: u8,
    duplicate_binding: bool,
    inputs: &[(&str, &str, &str, &str, f64)],
    result_type: &str,
    output_value: Option<f64>,
    source_expression: &str,
) -> Vec<u8> {
    standard_catpart_with_typed_formula_inputs_and_object_payload(
        parameter_entity_id,
        duplicate_binding,
        inputs,
        result_type,
        output_value,
        source_expression,
        (&[0xfe], None),
    )
}

fn standard_catpart_with_typed_formula_inputs_and_object_payload(
    parameter_entity_id: u8,
    duplicate_binding: bool,
    inputs: &[(&str, &str, &str, &str, f64)],
    result_type: &str,
    output_value: Option<f64>,
    source_expression: &str,
    input_options: (&[u8], Option<usize>),
) -> Vec<u8> {
    let (input_object_payload, unset_input_index) = input_options;
    assert!(!duplicate_binding || !inputs.is_empty());
    let formula_definition = [0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0];
    let expression_definition = [0x00, 0x08, 0x32, 5, 0, 0, 0, 0x32, 5, 0, 0, 0];
    let mut expression_value = Vec::new();
    for ordinal in 6u32..=11 {
        expression_value.push(0x32);
        expression_value.extend_from_slice(&ordinal.to_le_bytes());
    }
    expression_value.push(0xfe);

    let mut stream = entity_table_record_with_definition_and_value(1, &formula_definition, &[0xfe]);
    stream.extend(entity_table_record_with_definition_and_value(
        2,
        &expression_definition,
        &expression_value,
    ));
    let parameter = |entity_id, name_ordinal: u32, binding_ordinal: u32, value: Option<f64>| {
        let mut parameter_value = vec![0x32];
        parameter_value.extend_from_slice(&name_ordinal.to_le_bytes());
        parameter_value.push(0x32);
        parameter_value.extend_from_slice(&binding_ordinal.to_le_bytes());
        parameter_value.push(0xfe);
        let mut parameter =
            entity_table_record_with_definition_and_value(entity_id, &[0x01], &parameter_value);
        parameter[6] = 2;
        parameter.extend_from_slice(&[0x85, 0x96, 0x82, 0x6a]);
        match value {
            Some(value) => {
                parameter.push(0xe6);
                parameter.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            None => parameter.push(0xe7),
        }
        parameter.extend_from_slice(&[0x81, 0x52]);
        let parameter_len = u32::try_from(parameter.len()).expect("bounded parameter entity");
        parameter[2..6].copy_from_slice(&parameter_len.to_le_bytes());
        parameter
    };
    for (index, (_, _, _, _, value)) in inputs.iter().enumerate() {
        let entity_id = 3 + u8::try_from(index).expect("bounded input count");
        let name_ordinal = 12 + 2 * u32::try_from(index).expect("bounded input count");
        stream.extend(parameter(
            entity_id.into(),
            name_ordinal,
            name_ordinal + 1,
            (unset_input_index != Some(index)).then_some(*value),
        ));
    }
    if duplicate_binding {
        let entity_id = 3 + u8::try_from(inputs.len()).expect("bounded input count");
        stream.extend(parameter(
            entity_id.into(),
            12_u32,
            13_u32,
            Some(inputs[0].4),
        ));
    }
    let output_entity_id =
        3 + u8::try_from(inputs.len()).expect("bounded input count") + u8::from(duplicate_binding);
    let output_name_ordinal = 12 + 2 * u32::try_from(inputs.len()).expect("bounded input count");
    stream.extend(parameter(
        output_entity_id.into(),
        output_name_ordinal,
        output_name_ordinal + 1,
        output_value,
    ));
    stream.push(0xde);
    let mut records = vec![
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x84],
            &[
                0xf9,
                0x84,
                0x81,
                0x81,
                0x81,
                0x82,
                0x81,
                parameter_entity_id,
                0xd1,
                0x80,
                0xfe,
            ],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
    ];
    records.extend(
        inputs
            .iter()
            .map(|_| object_graph_record(&[0x04, 0x01, 0x81, 0x84], input_object_payload)),
    );
    if duplicate_binding {
        records.push(object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]));
    }
    records.push(object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]));
    stream.extend(object_graph_from_records(&records));
    let input_signature = inputs
        .iter()
        .map(|(symbol, input_type, _, _, _)| format!("{symbol} : #In {input_type}"))
        .collect::<Vec<_>>()
        .join(",");
    let type_signature = format!("({input_signature}) : {result_type}");
    let mut catalog = vec![
        "CATCatalogManager".to_string(),
        "catalogManager".to_string(),
        "catalogLinks".to_string(),
        String::new(),
        "Formula".to_string(),
        "body".to_string(),
        inputs
            .first()
            .map_or_else(String::new, |input| format!("{} ", input.0)),
        source_expression.to_string(),
        "param".to_string(),
        type_signature,
        "opened".to_string(),
        "RelationExpFct".to_string(),
    ];
    for (_, _, name, binding, _) in inputs {
        catalog.push((*name).to_string());
        catalog.push((*binding).to_string());
    }
    catalog.push("Result".to_string());
    catalog.push("#result_ /1".to_string());
    stream.extend(catalog_stream(
        &catalog.iter().map(String::as_str).collect::<Vec<_>>(),
    ));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

#[derive(Clone, Copy)]
enum FormulaChainCase {
    Linear,
    Cyclic,
    DuplicateTerminal,
    DuplicateIntermediate,
    IncompatibleDownstream,
    AmbiguousIntermediateWithIncompatibleDownstream,
}

fn standard_catpart_with_formula_chain(case: FormulaChainCase) -> Vec<u8> {
    let cyclic = matches!(case, FormulaChainCase::Cyclic);
    let duplicate_terminal = matches!(case, FormulaChainCase::DuplicateTerminal);
    let duplicate_intermediate = matches!(
        case,
        FormulaChainCase::DuplicateIntermediate
            | FormulaChainCase::AmbiguousIntermediateWithIncompatibleDownstream
    );
    let incompatible_downstream = matches!(
        case,
        FormulaChainCase::IncompatibleDownstream
            | FormulaChainCase::AmbiguousIntermediateWithIncompatibleDownstream
    );
    let definition = |ordinal: u32| {
        let mut bytes = vec![0x00, 0x08, 0x32];
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        bytes.push(0x32);
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        bytes
    };
    let expression_value = |ordinals: [u32; 6]| {
        let mut bytes = Vec::new();
        for ordinal in ordinals {
            bytes.push(0x32);
            bytes.extend_from_slice(&ordinal.to_le_bytes());
        }
        bytes.push(0xfe);
        bytes
    };
    let parameter = |entity_id: u32, name: u32, binding: u32, value: f64| {
        let mut payload = vec![0x32];
        payload.extend_from_slice(&name.to_le_bytes());
        payload.push(0x32);
        payload.extend_from_slice(&binding.to_le_bytes());
        payload.push(0xfe);
        let mut entity =
            entity_table_record_with_definition_and_value(entity_id, &[0x01], &payload);
        entity[6] = 2;
        entity.extend_from_slice(&[0x85, 0x96, 0x82, 0x6a, 0xe6]);
        entity.extend_from_slice(&value.to_bits().to_le_bytes());
        entity.extend_from_slice(&[0x81, 0x52]);
        let len = u32::try_from(entity.len()).expect("bounded parameter entity");
        entity[2..6].copy_from_slice(&len.to_le_bytes());
        entity
    };
    let formula_object = |owner: u8, expression: u8, output: u8| {
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x84],
            &[
                0xf9,
                0x84,
                0x81,
                0x80 + owner,
                0x81,
                0x80 + expression,
                0x81,
                output,
                0xd1,
                0x80,
                0xfe,
            ],
        )
    };
    let empty_object = || object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]);

    let mut stream = entity_table_record_with_definition_and_value(1, &definition(4), &[0xfe]);
    stream.extend(entity_table_record_with_definition_and_value(
        2,
        &definition(5),
        &expression_value([6, 7, 8, 9, 10, 11]),
    ));
    stream.extend(parameter(3, 12, 13, 1.0));
    stream.extend(parameter(4, 14, 15, if cyclic { 3.0 } else { 2.0 }));
    stream.extend(entity_table_record_with_definition_and_value(
        5,
        &definition(4),
        &[0xfe],
    ));
    stream.extend(entity_table_record_with_definition_and_value(
        6,
        &definition(5),
        &expression_value(if duplicate_terminal {
            [6, 7, 8, 9, 10, 11]
        } else {
            [16, 17, 8, 18, 10, 11]
        }),
    ));
    stream.extend(parameter(7, 19, 20, 3.0));
    if duplicate_intermediate {
        stream.extend(entity_table_record_with_definition_and_value(
            8,
            &definition(4),
            &[0xfe],
        ));
        stream.extend(entity_table_record_with_definition_and_value(
            9,
            &definition(5),
            &expression_value([6, 7, 8, 9, 10, 11]),
        ));
    }
    stream.push(0xde);
    let mut objects = vec![
        formula_object(1, 2, 4),
        empty_object(),
        empty_object(),
        empty_object(),
        formula_object(5, 6, if duplicate_terminal { 4 } else { 7 }),
        empty_object(),
        empty_object(),
    ];
    if duplicate_intermediate {
        objects.extend([formula_object(8, 9, 4), empty_object()]);
    }
    stream.extend(object_graph_from_records(&objects));
    let first_expression = if cyclic { "#3_ /4" } else { "#1_ /2+1mm" };
    let first_placeholder = if cyclic { "#3_ " } else { "#1_ " };
    let first_signature = if cyclic {
        "(#3_ : #In LENGTH) : LENGTH"
    } else {
        "(#1_ : #In LENGTH) : LENGTH"
    };
    let catalog = vec![
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Formula",
        "body",
        first_placeholder,
        first_expression,
        "param",
        first_signature,
        "opened",
        "RelationExpFct",
        "Input",
        "#1_ /2",
        "Intermediate",
        "#2_ /3",
        "#2_ ",
        if cyclic {
            "#2_ /3"
        } else if incompatible_downstream {
            "#2_ /3+1"
        } else {
            "#2_ /3+1mm"
        },
        if incompatible_downstream {
            "(#2_ : #In Real) : Real"
        } else {
            "(#2_ : #In LENGTH) : LENGTH"
        },
        "Final",
        "#3_ /4",
    ];
    stream.extend(catalog_stream(&catalog));

    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_crossing_entity_value_packet() -> Vec<u8> {
    let value = [
        0x32, 4, 0, 0, 0, 0x81, 0x82, 0xe8, 0xf4, 0x1a, 0x37, 0x83, 0x84, 0xe6, 0x32, 4, 0, 0, 0,
        0, 0, 0, 0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe])];
    let mut stream = entity_table_record_with_value(1, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "TargetValue",
    ]));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    file
}

fn standard_catpart_with_numeric_entity_value_tuple() -> Vec<u8> {
    let value = [
        0x91, 0x84, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6, 0, 0, 0, 0, 0, 0, 0x12, 0x40, 0xfe,
        0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut stream = entity_table_record_with_value(1, &value);
    stream.push(0xde);
    stream.extend(object_graph_from_records(&records));
    let mut file = standard_catpart();
    file.splice(16..16, stream);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
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
fn standard_decode_distinguishes_consolidated_surface_frames() {
    let mut payload = a5_surface_stream();
    payload.extend_from_slice(&a5_surface_stream());
    let mut file = standard_catpart();
    file.splice(16..16, payload);
    let file_len = u32::try_from(file.len()).unwrap();
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("standard decode");
    let identities = decoded
        .ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| surface.source_object.as_ref())
        .map(|source| source.object_id.as_str())
        .collect::<Vec<_>>();
    let frame_identities = identities
        .iter()
        .copied()
        .filter(|identity| identity.starts_with("cgm-a5-surface-frame:"))
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(frame_identities.len(), 2);
    assert!(!identities.contains(&"cgm-surface:000000"));
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
    let directory_offset =
        usize::try_from(u32::from_be_bytes(bytes[8..12].try_into().unwrap())).unwrap();
    assert_eq!(
        crate::container::outer_stream_directory_range(&bytes),
        Some(directory_offset..bytes.len())
    );
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
    assert_eq!(
        result.report.coverage["attempted_standard_topology_count"],
        1
    );
    assert_eq!(
        result.report.coverage["attached_standard_topology_count"],
        0
    );
    assert_eq!(
        result
            .report
            .coverage
            .iter()
            .filter(|(key, _)| key.starts_with("standard_topology_failure_"))
            .map(|(_, count)| count)
            .sum::<usize>(),
        1
    );
    assert_eq!(
        [
            "standard_topology_mesh_ambiguity_coordinate_root_closure_count",
            "standard_topology_mesh_ambiguity_endpoint_resolution_count",
            "standard_topology_mesh_ambiguity_distinct_topology_solutions_count",
        ]
        .into_iter()
        .map(|key| result.report.coverage[key])
        .sum::<usize>(),
        result.report.coverage["standard_topology_failure_ambiguous_solution_count"]
    );
    assert_eq!(
        result.report.coverage["standard_topology_empty_endpoint_domain_count"]
            + result.report.coverage["standard_topology_singleton_endpoint_domain_count"]
            + result.report.coverage["standard_topology_multiple_endpoint_domain_count"],
        result.report.coverage["standard_topology_curve_support_count"]
    );
    assert!(
        result
            .report
            .coverage
            .iter()
            .filter(|(key, _)| {
                key.starts_with("standard_topology_mesh_rejection_")
                    && !key.starts_with("standard_topology_mesh_rejection_incidence_")
                    && (!key.contains("endpoint_incidence_")
                        || key.ends_with("endpoint_incidence_count"))
            })
            .map(|(_, count)| count)
            .sum::<usize>()
            <= 1
    );
    assert_eq!(
        result.report.coverage["standard_topology_mesh_rejection_endpoint_incidence_count"],
        result.report.coverage
            ["standard_topology_mesh_rejection_endpoint_incidence_no_assignment_count"]
            + result.report.coverage
                ["standard_topology_mesh_rejection_endpoint_incidence_boundary_reconstruction_count"]
    );
    assert_eq!(
        result.report.coverage
            ["standard_topology_mesh_rejection_endpoint_incidence_no_assignment_count"],
        result.report.coverage["standard_topology_mesh_rejection_incidence_input_shape_count"]
            + result.report.coverage
                ["standard_topology_mesh_rejection_incidence_choice_pruning_count"]
            + result.report.coverage
                ["standard_topology_mesh_rejection_incidence_fixed_assignment_count"]
            + result.report.coverage
                ["standard_topology_mesh_rejection_incidence_component_domain_count"]
            + result.report.coverage
                ["standard_topology_mesh_rejection_incidence_component_composition_count"]
    );

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
    assert_eq!(
        decoded.report.coverage["attempted_standard_topology_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["attached_standard_topology_count"],
        1
    );
    assert_eq!(
        decoded
            .report
            .coverage
            .iter()
            .filter(|(key, _)| key.starts_with("standard_topology_failure_"))
            .map(|(_, count)| count)
            .sum::<usize>(),
        0
    );
    assert_eq!(
        [
            "standard_topology_mesh_ambiguity_coordinate_root_closure_count",
            "standard_topology_mesh_ambiguity_endpoint_resolution_count",
            "standard_topology_mesh_ambiguity_distinct_topology_solutions_count",
        ]
        .into_iter()
        .map(|key| decoded.report.coverage[key])
        .sum::<usize>(),
        decoded.report.coverage["standard_topology_failure_ambiguous_solution_count"]
    );
    assert_eq!(
        decoded.report.coverage["standard_topology_empty_endpoint_domain_count"]
            + decoded.report.coverage["standard_topology_singleton_endpoint_domain_count"]
            + decoded.report.coverage["standard_topology_multiple_endpoint_domain_count"],
        decoded.report.coverage["standard_topology_curve_support_count"]
    );
    assert_eq!(
        decoded
            .report
            .coverage
            .iter()
            .filter(|(key, _)| {
                key.starts_with("standard_topology_mesh_rejection_")
                    && !key.starts_with("standard_topology_mesh_rejection_incidence_")
                    && (!key.contains("endpoint_incidence_")
                        || key.ends_with("endpoint_incidence_count"))
            })
            .map(|(_, count)| count)
            .sum::<usize>(),
        0
    );
    assert_eq!(
        decoded.report.coverage["standard_topology_mesh_rejection_endpoint_incidence_count"],
        decoded.report.coverage
            ["standard_topology_mesh_rejection_endpoint_incidence_no_assignment_count"]
            + decoded.report.coverage
                ["standard_topology_mesh_rejection_endpoint_incidence_boundary_reconstruction_count"]
    );
    assert_eq!(
        decoded.report.coverage
            ["standard_topology_mesh_rejection_endpoint_incidence_no_assignment_count"],
        decoded.report.coverage["standard_topology_mesh_rejection_incidence_input_shape_count"]
            + decoded.report.coverage
                ["standard_topology_mesh_rejection_incidence_choice_pruning_count"]
            + decoded.report.coverage
                ["standard_topology_mesh_rejection_incidence_fixed_assignment_count"]
            + decoded.report.coverage
                ["standard_topology_mesh_rejection_incidence_component_domain_count"]
            + decoded.report.coverage
                ["standard_topology_mesh_rejection_incidence_component_composition_count"]
    );
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
fn decode_accounts_for_unresolved_legacy_entity_runs() {
    let mut bytes = zero_entity_catpart();
    for (entity_id, lead) in [(1_u32, 0x81), (3, 0xe5), (8, 0xfd)] {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([lead, 0xfd, 0x8c]);
    }
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode legacy identity run");
    assert_eq!(
        decoded.report.coverage["decoded_legacy_entity_run_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_entity_identity_count"],
        3
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_identity_lead_81_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_identity_lead_82_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_identity_lead_e5_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_identity_lead_fd_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_role_selector_count"],
        0
    );
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.message.contains("legacy design run")
    }));
}

#[test]
fn decode_retains_compound_legacy_text_fields_and_relation_roles() {
    fn compound_field(bytes: &mut Vec<u8>, value: &str, role: &str, selector_low: u8) {
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(value.len() + 1).expect("short value"));
        bytes.extend(value.as_bytes());
        bytes.push(u8::try_from(role.len() + 1).expect("short role"));
        bytes.extend(role.as_bytes());
        bytes.extend([0xe3, selector_low]);
    }

    fn selected_compound_field(
        bytes: &mut Vec<u8>,
        value: &str,
        role_selector: u8,
        selector_low: u8,
    ) {
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.push(u8::try_from(value.len() + 1).expect("short value"));
        bytes.extend(value.as_bytes());
        bytes.extend([role_selector, 0xe3, selector_low]);
    }

    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0xe5);
    compound_field(&mut bytes, "", "body", 0x53);
    compound_field(&mut bytes, "2 * #1_", "param", 0x52);
    compound_field(&mut bytes, "(#1_ : #In LENGTH) : LENGTH\n", "opened", 0x51);
    bytes.push(0xea);
    bytes.extend(2_u32.to_le_bytes());
    bytes.push(0xfd);
    bytes.extend([0xa2, 0xe3, 0xa0]);
    selected_compound_field(&mut bytes, "", 0xcf, 0x9f);
    selected_compound_field(&mut bytes, "#1_ + #2_", 0xd1, 0x9e);
    selected_compound_field(
        &mut bytes,
        "(#1_ : #In LENGTH,#2_ : #In LENGTH) : LENGTH\n",
        0xd3,
        0x9d,
    );
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode compound legacy fields");
    assert_eq!(
        decoded.report.coverage["decoded_legacy_text_field_count"],
        6
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_e3_role_tail_text_field_count"],
        6
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_role_text_field_count"],
        5
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_selected_role_count"],
        4
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_role_field_binding_count"],
        5
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_schema_field_count"],
        5
    );
    assert_eq!(decoded.report.coverage["decoded_legacy_relation_count"], 2);

    let native = crate::native::CatiaNative::load(
        decoded
            .ir
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load compound legacy fields");
    assert!(native.legacy_entity_runs[0]
        .text_fields
        .iter()
        .all(|field| {
            field.encoding == crate::native::CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
        }));
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].expression,
        "2 * #1_"
    );
    assert_eq!(
        native.legacy_entity_runs[0].relations[1].expression,
        "#1_ + #2_"
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[3]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&crate::native::CatiaLegacyRoleName::Selector(0xa2), 4769))
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[4]
            .role
            .as_ref()
            .map(|role| (&role.name, role.selector)),
        Some((&crate::native::CatiaLegacyRoleName::Selector(0xcf), 4768))
    );

    let mut invalid_relation_pair = native.clone();
    let prelude = invalid_relation_pair.legacy_entity_runs[0].text_fields[3].clone();
    invalid_relation_pair.legacy_entity_runs[0].relations[1].expression_offset =
        prelude.byte_offset;
    invalid_relation_pair.legacy_entity_runs[0].relations[1].expression = prelude.value;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_relation_pair
        .store(&mut namespace)
        .expect("store invalid selected relation pair");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].role_selectors[3].name =
        crate::native::CatiaLegacyRoleName::Selector(0);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid selected role");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn decode_retains_legacy_relation_synchronous_states() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([0x81, 0xfd, 0x8c]);
    for (selector, state) in [(15108_u32, 0x81), (15109, 0x82)] {
        bytes.extend([
            10, b's', b'y', b'n', b'c', b'h', b'r', b'o', b'n', b'e', 0x80,
        ]);
        bytes.extend(selector.to_le_bytes());
        bytes.extend([0xe8, 0x00, 0x1c, 0x01, state, 0xfe]);
    }
    bytes.extend([0xa3, 0xe3, 0x3c, 0xe8, 0x00, 0x1c, 0x01, 0x82]);
    bytes.extend([0xa4, 0xe3, 0x3d, 0xe8, 0x34, 0x17, 0x01, 0xfe]);
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode legacy relation update states");

    assert_eq!(
        decoded.report.coverage["decoded_legacy_synchronous_state_count"],
        3
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_synchronous_relation_count"],
        2
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_asynchronous_relation_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_schema_field_count"],
        3
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_role_field_binding_count"],
        4
    );
    let native = crate::native::CatiaNative::load(
        decoded
            .ir
            .native
            .namespace("catia")
            .expect("CATIA native namespace"),
    )
    .expect("load retained update states");
    assert_eq!(
        native.legacy_entity_runs[0]
            .synchronous_states
            .iter()
            .map(|state| (state.selector, state.synchronous))
            .collect::<Vec<_>>(),
        [(15108, false), (15109, true), (4669, true)]
    );
    assert_eq!(
        native.legacy_entity_runs[0]
            .schema_fields
            .iter()
            .map(|field| (field.field_code, field.payload.as_slice()))
            .collect::<Vec<_>>(),
        [
            (0x1c00, &[0x81, 0xfe][..]),
            (0x1c00, &[0x82, 0xfe][..]),
            (0x1c00, &[0x82][..]),
        ]
    );

    let mut missing_selected_successor = native.clone();
    missing_selected_successor.legacy_entity_runs[0]
        .role_selectors
        .pop();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    missing_selected_successor
        .store(&mut namespace)
        .expect("store selected state without successor role");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_field_boundary = native.clone();
    invalid_field_boundary.legacy_entity_runs[0].schema_fields[0].boundary_role_byte_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_field_boundary
        .store(&mut namespace)
        .expect("store invalid schema-field boundary");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut missing_bound_field_code = native.clone();
    missing_bound_field_code.legacy_entity_runs[0].role_selectors[0].field_code = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    missing_bound_field_code
        .store(&mut namespace)
        .expect("store schema field without its role binding");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].synchronous_states[0].selector += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid relation update state");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_parameter() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    bytes.extend(12.5_f64.to_bits().to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy parameter");

    let [parameter] = decoded.ir.model.parameters.as_slice() else {
        panic!("one transferred legacy parameter")
    };
    assert_eq!(parameter.name, "Width");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.5)
        ))
    );
    assert_eq!(parameter.expression, "12.5 mm");
    assert!(parameter
        .native_ref
        .as_deref()
        .is_some_and(|id| id.starts_with("catia:legacy:entity-run#")));
    assert_eq!(
        decoded.report.coverage["transferred_legacy_parameter_count"],
        1
    );
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_string() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([
        12, b'R', b'e', b's', b'p', b'o', b'n', b's', b'i', b'b', b'l', b'e', 0xfe,
    ]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'S', b't', b'r', b'i', b'n', b'g', 0x83]);
    bytes.extend(b"\xfe\x85\x93\x82\xfe");
    bytes.extend([
        12, b'C', b'i', b'l', b'a', b's', b' ', b'E', b'v', b'a', b'n', b's',
    ]);
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy string");

    let [parameter] = decoded.ir.model.parameters.as_slice() else {
        panic!("one transferred legacy string")
    };
    assert_eq!(parameter.name, "Responsible");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::String(
            "Cilas Evans".to_string()
        ))
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_string_value_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_named_string_value_count"],
        1
    );
}

#[test]
fn decode_transfers_a_uniquely_named_literal_typed_legacy_integer() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'C', b'o', b'u', b'n', b't', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([8, b'I', b'n', b't', b'e', b'g', b'e', b'r', 0x83]);
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed legacy integer");

    let [parameter] = decoded.ir.model.parameters.as_slice() else {
        panic!("one transferred legacy integer")
    };
    assert_eq!(parameter.name, "Count");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(11))
    );
    assert_eq!(parameter.expression, "11");
    assert_eq!(
        decoded.report.coverage["decoded_legacy_integer_value_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_legacy_named_integer_value_count"],
        1
    );
}

#[test]
fn decode_transfers_an_unset_typed_legacy_parameter() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    bytes.extend(b"\xfe\x84\x92\x82");
    bytes.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe7");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode unset typed legacy parameter");

    let [parameter] = decoded.ir.model.parameters.as_slice() else {
        panic!("one transferred legacy parameter")
    };
    assert_eq!(parameter.name, "Width");
    assert_eq!(parameter.value, None);
    assert!(parameter.expression.is_empty());
    assert_eq!(parameter.properties["value_type"], "LENGTH");
    assert_eq!(
        decoded.report.coverage["transferred_legacy_parameter_count"],
        1
    );
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn decode_transfers_unset_non_numeric_legacy_parameters() {
    for parameter_type in ["Boolean", "String"] {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([6, b'V', b'a', b'l', b'u', b'e', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(parameter_type.len() + 1).expect("short parameter type"));
        bytes.extend(parameter_type.as_bytes());
        bytes.push(0x83);
        bytes.extend(b"\xfe\x84\x88\x82\xfe\xe7");
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

        let decoded = CatiaCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode unset non-numeric legacy parameter");
        let [parameter] = decoded.ir.model.parameters.as_slice() else {
            panic!("one transferred legacy parameter")
        };

        assert_eq!(parameter.name, "Value");
        assert_eq!(parameter.value, None);
        assert!(parameter.expression.is_empty());
        assert_eq!(parameter.properties["value_type"], parameter_type);
        assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new()).is_ok());
    }
}

#[test]
fn decode_transfers_intrinsically_typed_evaluated_string_and_integer_parameters() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([0x58, 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01\x09Revision\xfe");
    bytes.extend([0x5f, 0xd1, 9]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x93\x82\xfe\x0bRevision-1");
    bytes.push(0xea);
    bytes.extend(2_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([0x58, 0xd1, 10]);
    bytes.extend(b"\xe8\x00\x12\x01\x07Search\xfe");
    bytes.extend([6, b'V', b'a', b'l', b'b', b'y', 0xd1, 11]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x80");
    bytes.extend((-7_i32).to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode intrinsically typed evaluated parameters");

    let [string, integer] = decoded.ir.model.parameters.as_slice() else {
        panic!("two transferred evaluated parameters")
    };
    assert_eq!(string.name, "Revision");
    assert_eq!(
        string.value,
        Some(cadmpeg_ir::features::ParameterValue::String(
            "Revision-1".to_string()
        ))
    );
    assert_eq!(string.properties["value_type"], "String");
    assert_eq!(integer.name, "Search");
    assert_eq!(
        integer.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(-7))
    );
    assert_eq!(integer.properties["value_type"], "Integer");
    assert_eq!(
        decoded.report.coverage["transferred_legacy_parameter_count"],
        2
    );
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn decode_does_not_override_a_string_value_type_descriptor() {
    for descriptor in [
        b"\xfe\x84\x92\x82\x08Integer\x83".as_slice(),
        b"\xfe\x84\x92\x82\x82\x83".as_slice(),
    ] {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([0x58, 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01\x06Value\xfe");
        bytes.extend([0x5f, 0xd1, 9]);
        bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
        bytes.extend(descriptor);
        bytes.extend(b"\xfe\x85\x93\x82\xfe\x05Text");
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

        let decoded = CatiaCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode string with an incompatible or unresolved descriptor");

        assert!(decoded.ir.model.parameters.is_empty());
        assert_eq!(
            decoded.report.coverage["transferred_legacy_parameter_count"],
            0
        );
    }
}

#[test]
fn decode_rejects_a_legacy_parameter_with_multiple_type_descriptors() {
    let mut bytes = zero_entity_catpart();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0xfd, 0x8c]);
    bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01");
    bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    for value_type in [b"LENGTH".as_slice(), b"Real".as_slice()] {
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(value_type.len() + 1).expect("short type"));
        bytes.extend(value_type);
        bytes.push(0x83);
    }
    bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    bytes.extend(12.5_f64.to_bits().to_le_bytes());
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode ambiguous legacy parameter");

    assert!(decoded.ir.model.parameters.is_empty());
    assert_eq!(
        decoded.report.coverage["transferred_legacy_parameter_count"],
        0
    );
}

#[test]
fn decode_resolves_only_an_acyclic_unique_legacy_type_selector_chain() {
    fn selected_type(terminal: Option<&str>) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82\x84\x83");
        bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
        bytes.extend(8.0_f64.to_bits().to_le_bytes());
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend(b"\xfe\x84\x92\x82");
        if let Some(value_type) = terminal {
            bytes.push(u8::try_from(value_type.len() + 1).expect("short type"));
            bytes.extend(value_type.as_bytes());
            bytes.push(0x83);
        } else {
            bytes.extend([0x81, 0x83]);
        }
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(selected_type(Some("LENGTH"))),
            &DecodeOptions::default(),
        )
        .expect("decode selected legacy type");
    assert_eq!(
        decoded.ir.model.parameters[0].value,
        Some(cadmpeg_ir::ParameterValue::Length(
            cadmpeg_ir::features::Length(8.0)
        ))
    );
    assert_eq!(
        decoded.report.coverage["transferred_legacy_selector_parameter_count"],
        1
    );

    let cyclic = CatiaCodec
        .decode(
            &mut Cursor::new(selected_type(None)),
            &DecodeOptions::default(),
        )
        .expect("decode cyclic legacy type");
    assert!(cyclic.ir.model.parameters.is_empty());
    assert_eq!(
        cyclic.report.coverage["transferred_legacy_selector_parameter_count"],
        0
    );
}

#[test]
fn decode_transfers_only_an_agreeing_closed_legacy_formula() {
    fn legacy_constant(
        expression: &str,
        stored: Option<f64>,
        parameter_type: &str,
        relation_type: &str,
    ) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        let signature = format!("() : {relation_type}\n");
        for (role, selector, value) in [
            ("body", 1_u32, expression),
            ("param", 4_u32, signature.as_str()),
        ] {
            bytes.push(u8::try_from(role.len() + 1).expect("short role"));
            bytes.extend(role.as_bytes());
            bytes.push(0x80);
            bytes.extend(selector.to_le_bytes());
            bytes.extend(b"\xe8\x00\x12\x01");
            bytes.push(u8::try_from(value.len() + 1).expect("short text"));
            bytes.extend(value.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01");
        bytes.extend([7, b'R', b'e', b's', b'u', b'l', b't', 0xfe]);
        bytes.extend(b"\xfe\x84\x92\x82");
        bytes.push(u8::try_from(parameter_type.len() + 1).expect("short type"));
        bytes.extend(parameter_type.as_bytes());
        bytes.push(0x83);
        bytes.extend(b"\xfe\x84\x88\x82\xfe");
        if let Some(stored) = stored {
            bytes.push(0xe6);
            bytes.extend(stored.to_bits().to_le_bytes());
        } else {
            bytes.push(0xe7);
        }
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", Some(5.0), "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy formula");
    let [parameter] = decoded.ir.model.parameters.as_slice() else {
        panic!("one legacy formula parameter")
    };
    assert_eq!(parameter.expression, "2+3");
    assert_eq!(
        decoded.report.coverage["transferred_legacy_formula_count"],
        1
    );
    let validation = cadmpeg_ir::validate::validate(&decoded.ir, Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);

    let mismatched = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", Some(6.0), "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched legacy formula");
    assert_eq!(mismatched.ir.model.parameters[0].expression, "6");
    assert_eq!(
        mismatched.report.coverage["transferred_legacy_formula_count"],
        0
    );

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", None, "Real", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode unset closed legacy formula");
    let [parameter] = unset.ir.model.parameters.as_slice() else {
        panic!("one unset legacy formula parameter")
    };
    assert_eq!(parameter.expression, "2+3");
    assert_eq!(parameter.value, None);
    assert_eq!(unset.report.coverage["transferred_legacy_formula_count"], 1);

    let mismatched_unset = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("2+3", None, "LENGTH", "Real")),
            &DecodeOptions::default(),
        )
        .expect("decode type-mismatched unset legacy formula");
    let [parameter] = mismatched_unset.ir.model.parameters.as_slice() else {
        panic!("one unset legacy parameter")
    };
    assert!(parameter.expression.is_empty());
    assert_eq!(parameter.value, None);
    assert_eq!(
        mismatched_unset.report.coverage["transferred_legacy_formula_count"],
        0
    );

    let boolean = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant("not false", None, "Boolean", "Boolean")),
            &DecodeOptions::default(),
        )
        .expect("decode Boolean negation formula");
    let [parameter] = boolean.ir.model.parameters.as_slice() else {
        panic!("one Boolean formula parameter")
    };
    assert_eq!(parameter.expression, "not false");
    assert_eq!(parameter.value, None);
    assert_eq!(
        parameter.properties.get("value_type").map(String::as_str),
        Some("Boolean")
    );
    assert_eq!(
        boolean.report.coverage["transferred_legacy_formula_count"],
        1
    );

    let conditional = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_constant(
                "true ? 5 ; 1 / 0",
                Some(5.0),
                "Real",
                "Real",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode lazy conditional formula");
    let [parameter] = conditional.ir.model.parameters.as_slice() else {
        panic!("one conditional formula parameter")
    };
    assert_eq!(parameter.expression, "true ? 5 ; 1 / 0");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(5.0))
    );
    assert_eq!(
        conditional.report.coverage["transferred_legacy_formula_count"],
        1
    );
}

#[test]
fn decode_transfers_an_agreeing_closed_legacy_string_formula() {
    fn legacy_string_constant(expression: &str, stored: &str) -> Vec<u8> {
        let mut bytes = zero_entity_catpart();
        bytes.push(0xea);
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        for (role, selector, value) in [
            ("body", 1_u32, expression),
            ("param", 4_u32, "() : String\n"),
        ] {
            bytes.push(u8::try_from(role.len() + 1).expect("short role"));
            bytes.extend(role.as_bytes());
            bytes.push(0x80);
            bytes.extend(selector.to_le_bytes());
            bytes.extend(b"\xe8\x00\x12\x01");
            bytes.push(u8::try_from(value.len() + 1).expect("short text"));
            bytes.extend(value.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xea);
        bytes.extend(4_u32.to_le_bytes());
        bytes.push(0x81);
        bytes.extend([0xfd, 0x8c]);
        bytes.extend([0x58, 0xd1, 8]);
        bytes.extend(b"\xe8\x00\x12\x01\x0fNewResponsible\xfe");
        bytes.extend([0x5f, 0xd1, 9]);
        bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
        bytes.extend(b"\xfe\x85\x93\x82\xfe");
        bytes.push(u8::try_from(stored.len() + 1).expect("short stored string"));
        bytes.extend(stored.as_bytes());
        bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
        bytes
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")",
                "Easy Evans",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy string formula");
    let [parameter] = decoded.ir.model.parameters.as_slice() else {
        panic!("one legacy string formula parameter")
    };
    assert_eq!(
        parameter.expression,
        "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")"
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::String("Easy Evans".to_string()))
    );
    assert_eq!(
        decoded.report.coverage["transferred_legacy_formula_count"],
        1
    );
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new()).is_ok());

    let mismatched = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ReplaceSubText(\"Cilas Evans\",\"Cilas\",\"Easy\")",
                "Cilas Evans",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched legacy string formula");
    assert!(mismatched.ir.model.parameters[0].expression.is_empty());
    assert_eq!(
        mismatched.report.coverage["transferred_legacy_formula_count"],
        0
    );

    let methods = CatiaCodec
        .decode(
            &mut Cursor::new(legacy_string_constant(
                "ToLower(\"MIXED\").Extract(1,4) - \"x\"",
                "ied",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode closed legacy string-method formula");
    let [parameter] = methods.ir.model.parameters.as_slice() else {
        panic!("one legacy string-method formula parameter")
    };
    assert_eq!(
        parameter.expression,
        "ToLower(\"MIXED\").Extract(1,4) - \"x\""
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::ParameterValue::String("ied".to_string()))
    );
    assert_eq!(
        methods.report.coverage["transferred_legacy_formula_count"],
        1
    );
}

#[test]
fn decode_zero_entity_transfers_framed_cylinder() {
    let mut cur = Cursor::new(zero_entity_cylinder_catpart());
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert!(result.ir.model.points.is_empty());
    assert!(result.ir.model.vertices.is_empty());
    assert!(result.ir.model.bodies.is_empty());
    assert!(result.ir.model.shells.is_empty());
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
fn decode_zero_entity_transfers_parametric_surface_curve_without_a_cache() {
    let result = CatiaCodec
        .decode(
            &mut Cursor::new(zero_entity_cylinder_parametric_support_catpart()),
            &DecodeOptions::default(),
        )
        .expect("decode zero-entity parametric support");

    assert_eq!(
        result.report.coverage["transferred_zero_entity_support_curve_count"],
        1
    );
    assert_eq!(
        result.report.coverage["transferred_zero_entity_parametric_surface_curve_count"],
        1
    );
    let [curve] = result.ir.model.curves.as_slice() else {
        panic!("one transferred support curve")
    };
    let [construction] = result.ir.model.procedural_curves.as_slice() else {
        panic!("one cacheless support construction")
    };
    assert!(matches!(
        &curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Procedural {
            construction: id
        } if id == &construction.id
    ));
    assert_eq!(construction.curve, curve.id);
    assert_eq!(construction.cache_fit_tolerance, None);
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
        family,
        context,
        tail: None,
    } = &construction.definition
    else {
        panic!("parametric surface-curve construction")
    };
    assert_eq!(
        *family,
        cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric
    );
    assert_eq!(context.parameter_range, [0.0, 1.0]);
    assert_eq!(
        context.sides[0].surface.as_ref(),
        Some(&result.ir.model.surfaces[0].id)
    );
    assert!(context.sides[0].pcurve.is_some());
    assert_eq!(context.sides[1].surface, None);
    assert_eq!(context.sides[1].pcurve, None);

    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_zero_entity_transfers_exact_model_curve_directly() {
    let mut file = vec![0u8; 16];
    file[..8].copy_from_slice(OUTER_MAGIC);
    file.extend(zero_entity_support_stream());
    let result = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity exact support");

    assert_eq!(
        result.report.coverage["transferred_zero_entity_support_curve_count"],
        1
    );
    assert_eq!(
        result.report.coverage["transferred_zero_entity_parametric_surface_curve_count"],
        0
    );
    assert!(matches!(
        result.ir.model.curves.as_slice(),
        [cadmpeg_ir::geometry::Curve {
            geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(_),
            ..
        }]
    ));
    assert!(result.ir.model.procedural_curves.is_empty());

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

pub(crate) fn append_b5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

pub(crate) fn b5_linear_pcurve_payload(surface: u16, start: [f64; 2], end: [f64; 2]) -> Vec<u8> {
    let mut payload = vec![0x81, 0x18];
    payload.extend_from_slice(&surface.to_le_bytes());
    payload.extend_from_slice(&[0x01, 5, 1, 1, 9, 1]);
    payload.extend_from_slice(&le_f64(0.0));
    payload.extend_from_slice(&le_f64(1.0));
    payload.extend_from_slice(&[9, 9]);
    for uv in [start, end] {
        payload.extend_from_slice(&le_f64(uv[0]));
        payload.extend_from_slice(&le_f64(uv[1]));
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [0.0, 1.0, 1.0, 0.0] {
        payload.extend_from_slice(&le_f64(value));
    }
    payload.extend_from_slice(&[0x00, 0x07]);
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

pub(crate) fn b5_plane_payload(origin: [f64; 3]) -> Vec<u8> {
    let mut plane = vec![0; 121];
    plane[0] = 0x80;
    for (offset, value) in [
        (1usize, origin[0]),
        (9, origin[1]),
        (17, origin[2]),
        (25, 1.0),
        (33, 0.0),
        (41, 0.0),
        (49, 0.0),
        (57, 1.0),
        (65, 0.0),
        (73, 1.0),
        (81, 1.0),
        (89, -10_000_000.0),
        (97, 10_000_000.0),
        (105, -10_000_000.0),
        (113, 10_000_000.0),
    ] {
        plane[offset..offset + 8].copy_from_slice(&le_f64(value));
    }
    plane
}

pub(crate) fn b5_closed_triangle_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    let plane = b5_plane_payload([0.0; 3]);
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
            0x18, 100, 0, 0x83, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0x01, 0x00,
            0xff, 0xff, 0x01, 0x00, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0x01,
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
fn native_namespace_retains_typed_consolidated_groups() {
    let native = crate::native::CatiaNative::decode(&b2_group_stream());
    let [group] = native.consolidated_groups.as_slice() else {
        panic!("one consolidated group")
    };
    assert_eq!(group.byte_offset, 9);
    assert_eq!(group.group_type, 3);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA consolidated groups");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA consolidated groups"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_groups[0].id.push_str("-changed");
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA consolidated group for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_class61_records() {
    let mut stream = b2_counted_61_stream();
    stream.extend_from_slice(&b2_long_61_stream());
    let native = crate::native::CatiaNative::decode(&stream);
    let [counted, long] = native.consolidated_class61_records.as_slice() else {
        panic!("two consolidated class-0x61 records")
    };
    let crate::native::CatiaConsolidatedClass61Payload::Counted { references, tail } =
        &counted.payload
    else {
        panic!("counted class-0x61 record")
    };
    assert_eq!(references, &[1300, 1294, 30, 74]);
    assert_eq!(tail, &[0x41, 0x03]);
    let crate::native::CatiaConsolidatedClass61Payload::Long {
        prefix,
        members,
        references,
        scalar,
    } = &long.payload
    else {
        panic!("long class-0x61 record")
    };
    assert_eq!(prefix, &[0xb5, 0x03, 0x2b, 0x47, 0x8f, 0xb3, 0xd7, 0xfb]);
    assert_eq!(members, &[0x064a, 0x0650, 0x0656]);
    assert_eq!(references, &[0x0100, 0x0103, 0x0106, 0x0109, 0x010c]);
    assert_eq!(*scalar, 42.5);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA class-0x61 records");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA class-0x61 records"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaConsolidatedClass61Payload::Long { members, .. } =
        &mut invalid.consolidated_class61_records[1].payload
    else {
        panic!("long class-0x61 record")
    };
    members.swap(0, 1);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA class-0x61 record for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_all_consolidated_parameter_point_layouts() {
    let native = crate::native::CatiaNative::decode(&b2_parameter_point_stream());
    let [uv, station_uv, five_scalars, station_uv_last] =
        native.consolidated_parameter_points.as_slice()
    else {
        panic!("four consolidated parameter points")
    };
    assert_eq!(
        [
            uv.prefix,
            station_uv.prefix,
            five_scalars.prefix,
            station_uv_last.prefix
        ],
        [0x05, 0x09, 0x0d, 0x11]
    );
    assert_eq!(uv.layout, 0x12);
    assert_eq!(uv.control, 0x12);
    assert!(matches!(
        &uv.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::Uv { uv: [2.0, 3.0] }
    ));
    assert_eq!(station_uv.layout, 0x1a);
    assert!(matches!(
        &station_uv.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::StationUv {
            station: 11.0,
            uv: [4.0, 5.0],
        }
    ));
    assert_eq!(five_scalars.layout, 0x2a);
    assert!(matches!(
        &five_scalars.payload,
        crate::native::CatiaConsolidatedParameterPointPayload::FiveScalars {
            values: [1.0, 2.0, 3.0, 4.0, 5.0],
        }
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA parameter points");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA parameter points"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_parameter_points[0].layout = 0x1a;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA parameter point");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_reference_lists() {
    let native = crate::native::CatiaNative::decode(&b2_reference_list_stream());
    let [list] = native.consolidated_reference_lists.as_slice() else {
        panic!("one consolidated reference list")
    };
    assert_eq!(list.references, (0u32..26).collect::<Vec<_>>());

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA reference list");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA reference list"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_reference_lists[0].references.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA reference list");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_standalone_consolidated_circle_supports() {
    let native = crate::native::CatiaNative::decode(&b2_circle_stream());
    let [circle] = native.consolidated_circles.as_slice() else {
        panic!("one consolidated circle")
    };
    assert_eq!(circle.layout, 0x34);
    assert_eq!(circle.record_id, 0x1234);
    assert_eq!(circle.frame_token, 0x05);
    assert_eq!(circle.center_pair, [4.0, -2.0]);
    assert_eq!(circle.radius, 3.0);
    assert_eq!(circle.range, [0.0, std::f64::consts::TAU * circle.radius]);
    assert!(circle.full_circle);
    assert_eq!(circle.chart_shift, 0.0);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA circle");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA circle"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_circles[0].full_circle = false;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA circle for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_all_consolidated_cylinder_layouts() {
    let mut stream = b2_cylinder_stream();
    stream.extend_from_slice(&b2_implicit_axis_cylinder_stream());
    stream.extend_from_slice(&b2_range_origin_cylinder_stream());
    let native = crate::native::CatiaNative::decode(&stream);
    let [explicit, implicit, range_origin] = native.consolidated_cylinders.as_slice() else {
        panic!("three consolidated cylinders")
    };
    assert_eq!(explicit.layout, 0x5a);
    assert_eq!(explicit.origin, [1.0, 2.0, 3.0]);
    assert_eq!(explicit.radius, 2.0);
    assert!(matches!(
        explicit.payload,
        crate::native::CatiaConsolidatedCylinderPayload::Resolved {
            frame_token: 0x19,
            axis: [1.0, 0.0, 0.0],
            reference_direction: [0.0, 1.0, 0.0],
        }
    ));
    assert_eq!(implicit.layout, 0x52);
    assert!(matches!(
        implicit.payload,
        crate::native::CatiaConsolidatedCylinderPayload::Resolved { .. }
    ));
    assert_eq!(range_origin.layout, 0x62);
    assert_eq!(range_origin.radius, 4.0);
    assert!(matches!(
        range_origin.payload,
        crate::native::CatiaConsolidatedCylinderPayload::RangeOrigin {
            stored_vector: [0.0, 1.0],
            axis: [0.0, 1.0, 0.0],
            reference_direction: [0.0, 0.0, 1.0],
            range_origin,
        } if range_origin.to_bits()
            == ((0.0 + 8.0) * 0.5 - std::f64::consts::PI * 4.0).to_bits()
    ));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA cylinders");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cylinders"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaConsolidatedCylinderPayload::RangeOrigin { range_origin, .. } =
        &mut invalid.consolidated_cylinders[2].payload
    else {
        panic!("range-origin cylinder")
    };
    *range_origin += 1.0;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cylinder for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_exact_consolidated_cone_charts() {
    let native = crate::native::CatiaNative::decode(&b2_cone_stream());
    let [cone] = native.consolidated_cones.as_slice() else {
        panic!("one consolidated cone")
    };
    assert_eq!(cone.apex, [1.0, 2.0, 3.0]);
    assert_eq!(cone.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(cone.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(cone.axis, [0.0, 0.0, 1.0]);
    assert_eq!(cone.half_angle, 0.25);
    assert_eq!(cone.pre_angular_range_scalar, 4.0);
    assert_eq!(cone.angular_range, [0.5, 0.5 + std::f64::consts::PI]);
    assert_eq!(cone.slant_range, [2.0, 8.0]);
    assert_eq!(cone.angular_scale, 3.0);
    assert_eq!(
        cone.angular_domain,
        [
            0.5 - std::f64::consts::FRAC_PI_2,
            0.5 + 3.0 * std::f64::consts::FRAC_PI_2
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA cone");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cone"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_cones[0].angular_domain[0] += 0.25;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_cone_face_charts() {
    let native = crate::native::CatiaNative::decode(&b2_cone_face_parameter_point_stream());
    let [face] = native.consolidated_cone_faces.as_slice() else {
        panic!("one consolidated cone-face chart")
    };
    assert_eq!(face.program.len(), 16);
    assert_eq!(face.angular_scale, 1.5);
    assert_eq!(face.half_angle, std::f64::consts::FRAC_PI_4);
    assert_eq!(
        face.parameter_points,
        [
            "catia:consolidated:parameter-point#0",
            "catia:consolidated:parameter-point#1",
            "catia:consolidated:parameter-point#2",
            "catia:consolidated:parameter-point#3",
        ]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA cone-face chart");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA cone-face chart"),
        native
    );

    let mut invalid = native.clone();
    invalid.consolidated_cone_faces[0].program.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone-face chart");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native.clone();
    invalid.consolidated_cone_faces[0]
        .parameter_points
        .swap(0, 1);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA cone-face parameter run");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut mixed = b2_cone_face_parameter_point_stream();
    mixed.extend_from_slice(&[0xb2, 0x03, 0x18, 0x02, 0x05, 0x99, 0x99]);
    let mixed = crate::native::CatiaNative::decode(&mixed);
    assert!(mixed.consolidated_cone_faces[0].parameter_points.is_empty());

    let mut file = standard_catpart();
    file.splice(16..16, b2_cone_face_parameter_point_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode CATIA cone-face chart");
    assert_eq!(
        decoded.report.coverage["decoded_consolidated_cone_face_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_consolidated_cone_face_parameter_point_count"],
        4
    );
}

#[test]
fn native_namespace_retains_resolved_consolidated_revolution_carriers() {
    let native = crate::native::CatiaNative::decode(&b2_resolved_revolution_stream());
    let [revolution] = native.consolidated_revolutions.as_slice() else {
        panic!("one consolidated revolution carrier")
    };
    assert_eq!(revolution.reference_token, 0x0a);
    assert_eq!(revolution.profile_allocation_id, 0x1234);
    assert_eq!(revolution.origin, [1.0, 2.0, 3.0]);
    assert_eq!(revolution.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(revolution.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(revolution.axis, [0.0, 0.0, 1.0]);
    assert_eq!(revolution.profile_range, [-4.0, 9.0]);
    assert_eq!(
        revolution.profile_circle.as_deref(),
        Some("catia:consolidated:circle#0")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA revolution");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA revolution"),
        native
    );

    let mut invalid = native.clone();
    invalid.consolidated_revolutions[0].profile_circle = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA revolution profile binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native;
    invalid.consolidated_revolutions[0].axis = [0.0, 0.0, -1.0];
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA revolution for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut file = standard_catpart();
    file.splice(16..16, b2_resolved_revolution_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode resolved CATIA revolution");
    let directrix = decoded
        .ir
        .model
        .curves
        .iter()
        .find(|curve| {
            curve
                .id
                .0
                .starts_with("catia:standard:revolution-directrix#")
        })
        .expect("transferred revolution directrix");
    assert!(matches!(
        directrix.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius: 3.0,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 4.0, -2.0)
            && axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            && ref_direction == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    ));
    let revolution = decoded
        .ir
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.id.0.starts_with("catia:standard:revolution#"))
        .expect("transferred revolution construction");
    assert!(decoded.ir.model.surfaces.iter().any(|surface| {
        surface.id == revolution.surface
            && matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                    center,
                    axis,
                    ref_direction,
                    major_radius: 2.0,
                    minor_radius: 3.0,
                } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, -2.0)
                    && axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                    && ref_direction == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
            )
    }));
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new()).is_ok());
    assert!(matches!(
        &revolution.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            angular_interval,
            parameter_interval: Some([-4.0, 9.0]),
            ..
        } if *angular_interval == [0.5, 0.5 + std::f64::consts::TAU]
    ));
    assert_eq!(
        decoded.report.coverage["transferred_consolidated_revolution_count"],
        1
    );
    assert!(!decoded.report.losses.iter().any(|loss| loss
        .message
        .contains("consolidated surface-of-revolution record")));
}

#[test]
fn native_namespace_retains_exact_consolidated_line_profiles() {
    let native = crate::native::CatiaNative::decode(&b2_line_profile_stream());
    let [line] = native.consolidated_line_profiles.as_slice() else {
        panic!("one consolidated line profile")
    };
    assert_eq!(line.origin, [1.0, 2.0, 3.0]);
    assert_eq!(line.direction, [0.0, 0.6, 0.8]);
    assert_eq!(line.range, [-4.0, 9.0]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA line profile");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA line profile"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_line_profiles[0].direction = [0.0, 0.0, 2.0];
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA line profile for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn decode_transfers_exact_consolidated_line_profiles() {
    let mut file = standard_catpart();
    file.splice(16..16, b2_line_profile_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode consolidated line profile");
    assert_eq!(
        decoded.report.coverage["decoded_consolidated_line_profile_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["transferred_consolidated_line_profile_count"],
        1
    );
    assert!(decoded.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && direction == cadmpeg_ir::math::Vector3::new(0.0, 0.6, 0.8)
    )));
    assert!(!decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("consolidated line-profile record(s)")));
}

#[test]
fn transferred_line_profile_identities_retain_their_native_ordinals() {
    let mut file = standard_catpart();
    file.splice(
        16..16,
        [b2_line_profile_stream(), b2_line_profile_stream()].concat(),
    );
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode mixed-metric line profiles");
    let line_ids = decoded
        .ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            curve
                .id
                .0
                .starts_with("catia:consolidated:line-profile-curve#")
        })
        .map(|curve| curve.id.0.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        line_ids,
        [
            "catia:consolidated:line-profile-curve#0",
            "catia:consolidated:line-profile-curve#1",
        ]
    );
}

#[test]
fn native_namespace_retains_zero_entity_surface_support_runs() {
    let mut stream = zero_entity_face_loop_support_stream();
    let support_slot = 0x6a + 12 + 13;
    stream[support_slot..support_slot + 4].copy_from_slice(&1u32.to_le_bytes());
    let native = crate::native::CatiaNative::decode(&stream);
    assert!(native.zero_entity_endpoint_pair_candidates.is_empty());
    let [run] = native.zero_entity_support_runs.as_slice() else {
        panic!("one zero-entity support run")
    };
    assert_eq!(run.carrier_byte_offset, 0);
    assert_eq!(run.carrier_record_ordinal, 1);
    let face = run.face.as_ref().expect("positionally aligned face");
    assert_eq!(face.record_ordinal, 3);
    assert_eq!(face.allocations, [10, 3]);
    assert_eq!(face.loop_terminals, [7]);
    let [loop_record] = face.loops.as_slice() else {
        panic!("one loop")
    };
    assert_eq!(loop_record.member_ids, [6]);
    assert_eq!(loop_record.typed_references, [1]);
    assert_eq!(
        loop_record.typed_records,
        ["catia:zero-entity:record#1".to_string()]
    );
    assert_eq!(loop_record.terminal_id, 7);
    assert_eq!(loop_record.loop_class, 0x41);
    assert_eq!(loop_record.forward_senses, [true]);
    assert_eq!(loop_record.support_record_ordinals, [2]);
    assert!(loop_record.oriented_model_endpoints.is_empty());
    let [support] = run.supports.as_slice() else {
        panic!("one zero-entity support occurrence")
    };
    assert_eq!(support.tag, [0x21, 0x71]);
    assert_eq!(support.record_ordinal, 2);
    assert_eq!(support.face_local_slot, 1);
    assert_eq!(support.uv_endpoints, Some([[-2.0, 4.0], [6.0, 8.0]]));
    assert!(matches!(
        support.pcurve,
        Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            ref control_points,
            weights: None,
            periodic: false,
            ..
        }) if control_points.len() == 2
    ));
    assert!(matches!(
        support.model_curve,
        Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve {
                degree: 1,
                ref control_points,
                weights: None,
                periodic: false,
                ..
            }
        )) if control_points.len() == 2
    ));
    assert!(support.model_curve_construction.is_none());
    assert_eq!(support.model_parameters, Some([0.0, 1.0]));
    assert_eq!(
        support.model_midpoint,
        Some(cadmpeg_ir::math::Point3::new(3.0, 8.0, 3.0))
    );
    assert_eq!(
        support.model_endpoints,
        Some([
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
            cadmpeg_ir::math::Point3::new(7.0, 10.0, 3.0),
        ])
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity support run");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA zero-entity support run"),
        native
    );

    let mut invalid_face = native.clone();
    invalid_face.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loop_terminals[0] = 8;
    let mut invalid_face_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face
        .store(&mut invalid_face_namespace)
        .expect("store invalid CATIA zero-entity face");
    assert!(crate::native::CatiaNative::load(&invalid_face_namespace).is_err());

    let mut zero_face_terminal = native.clone();
    zero_face_terminal.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loop_terminals[0] = 0;
    let mut zero_face_terminal_namespace = cadmpeg_ir::NativeNamespace::default();
    zero_face_terminal
        .store(&mut zero_face_terminal_namespace)
        .expect("store zero CATIA zero-entity face loop terminal");
    assert!(crate::native::CatiaNative::load(&zero_face_terminal_namespace).is_err());

    let mut invalid_loop_roster = native.clone();
    invalid_loop_roster.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .loop_class = 0x50;
    let mut invalid_loop_roster_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop_roster
        .store(&mut invalid_loop_roster_namespace)
        .expect("store invalid CATIA zero-entity loop roster");
    assert!(crate::native::CatiaNative::load(&invalid_loop_roster_namespace).is_err());

    let mut invalid_face_allocation = native.clone();
    invalid_face_allocation.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .allocations[0] = 0;
    let mut invalid_face_allocation_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face_allocation
        .store(&mut invalid_face_allocation_namespace)
        .expect("store invalid CATIA zero-entity face allocation");
    assert!(crate::native::CatiaNative::load(&invalid_face_allocation_namespace).is_err());

    let mut invalid_face_control = native.clone();
    invalid_face_control.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .terminal_control = 0x04;
    let mut invalid_face_control_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_face_control
        .store(&mut invalid_face_control_namespace)
        .expect("store invalid CATIA zero-entity face control");
    assert!(crate::native::CatiaNative::load(&invalid_face_control_namespace).is_err());

    let mut invalid_loop_gap = native.clone();
    invalid_loop_gap.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .gap = 0;
    let mut invalid_loop_gap_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop_gap
        .store(&mut invalid_loop_gap_namespace)
        .expect("store invalid CATIA zero-entity loop gap");
    assert!(crate::native::CatiaNative::load(&invalid_loop_gap_namespace).is_err());

    let mut invalid_support_slot = native.clone();
    invalid_support_slot.zero_entity_support_runs[0].supports[0].face_local_slot = 0;
    let mut invalid_support_slot_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_support_slot
        .store(&mut invalid_support_slot_namespace)
        .expect("store invalid CATIA zero-entity support slot");
    assert!(crate::native::CatiaNative::load(&invalid_support_slot_namespace).is_err());

    let mut invalid_loop = native.clone();
    invalid_loop.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .forward_senses
        .clear();
    let mut invalid_loop_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_loop
        .store(&mut invalid_loop_namespace)
        .expect("store invalid CATIA zero-entity loop");
    assert!(crate::native::CatiaNative::load(&invalid_loop_namespace).is_err());

    let mut invalid_typed_record = native.clone();
    invalid_typed_record.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .typed_records[0] = "catia:zero-entity:record#2".to_string();
    let mut invalid_typed_record_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_typed_record
        .store(&mut invalid_typed_record_namespace)
        .expect("store invalid CATIA zero-entity typed loop reference");
    assert!(crate::native::CatiaNative::load(&invalid_typed_record_namespace).is_err());

    let mut invalid_binding = native.clone();
    invalid_binding.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .support_record_ordinals[0] = 1;
    let mut invalid_binding_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_binding
        .store(&mut invalid_binding_namespace)
        .expect("store invalid CATIA zero-entity loop support binding");
    assert!(crate::native::CatiaNative::load(&invalid_binding_namespace).is_err());

    let mut invalid_pcurve = native.clone();
    let Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs { degree, .. }) =
        invalid_pcurve.zero_entity_support_runs[0].supports[0]
            .pcurve
            .as_mut()
    else {
        panic!("NURBS support pcurve")
    };
    *degree = 2;
    let mut invalid_pcurve_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_pcurve
        .store(&mut invalid_pcurve_namespace)
        .expect("store invalid CATIA zero-entity support pcurve");
    assert!(crate::native::CatiaNative::load(&invalid_pcurve_namespace).is_err());

    let mut invalid_model_curve = native.clone();
    let Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(model_curve)) =
        invalid_model_curve.zero_entity_support_runs[0].supports[0]
            .model_curve
            .as_mut()
    else {
        panic!("NURBS support model curve")
    };
    model_curve.periodic = true;
    let mut invalid_model_curve_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_curve
        .store(&mut invalid_model_curve_namespace)
        .expect("store invalid CATIA zero-entity support model curve");
    assert!(crate::native::CatiaNative::load(&invalid_model_curve_namespace).is_err());

    let mut invalid_model_parameters = native.clone();
    invalid_model_parameters.zero_entity_support_runs[0].supports[0].model_parameters =
        Some([1.0, 1.0]);
    let mut invalid_model_parameters_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_parameters
        .store(&mut invalid_model_parameters_namespace)
        .expect("store invalid CATIA zero-entity support model parameters");
    assert!(crate::native::CatiaNative::load(&invalid_model_parameters_namespace).is_err());

    let mut missing_model_midpoint = native.clone();
    missing_model_midpoint.zero_entity_support_runs[0].supports[0].model_midpoint = None;
    let mut missing_model_midpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    missing_model_midpoint
        .store(&mut missing_model_midpoint_namespace)
        .expect("store CATIA zero-entity support without its model midpoint");
    assert!(crate::native::CatiaNative::load(&missing_model_midpoint_namespace).is_err());

    let mut invalid_model_construction = native.clone();
    invalid_model_construction.zero_entity_support_runs[0].supports[0].model_curve_construction =
        Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
            angle_range: [0.0, 1.0],
            center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            major: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            minor: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            pitch: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 1.0,
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        });
    let mut invalid_model_construction_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_construction
        .store(&mut invalid_model_construction_namespace)
        .expect("store invalid CATIA zero-entity support model construction");
    assert!(crate::native::CatiaNative::load(&invalid_model_construction_namespace).is_err());

    let mut invalid_oriented_endpoints = native.clone();
    invalid_oriented_endpoints.zero_entity_support_runs[0]
        .face
        .as_mut()
        .expect("face")
        .loops[0]
        .oriented_model_endpoints
        .push([
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        ]);
    let mut invalid_oriented_endpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_oriented_endpoints
        .store(&mut invalid_oriented_endpoint_namespace)
        .expect("store invalid CATIA zero-entity oriented endpoints");
    assert!(crate::native::CatiaNative::load(&invalid_oriented_endpoint_namespace).is_err());

    let mut invalid_endpoint_pair = native.clone();
    invalid_endpoint_pair
        .zero_entity_endpoint_pair_candidates
        .push(crate::native::CatiaZeroEntityEndpointPairCandidate {
            id: "catia:zero-entity:endpoint-pair-candidate#0".to_string(),
            face_records: [
                "catia:zero-entity:record#3".to_string(),
                "catia:zero-entity:record#3".to_string(),
            ],
            support_records: [
                "catia:zero-entity:record#2".to_string(),
                "catia:zero-entity:record#2".to_string(),
            ],
            model_endpoints: [
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            ],
            model_midpoint: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        });
    let mut invalid_endpoint_pair_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_endpoint_pair
        .store(&mut invalid_endpoint_pair_namespace)
        .expect("store invalid CATIA zero-entity endpoint pair");
    assert!(crate::native::CatiaNative::load(&invalid_endpoint_pair_namespace).is_err());

    let mut invalid_endpoint_locus = native.clone();
    invalid_endpoint_locus
        .zero_entity_endpoint_locus_candidates
        .push(crate::native::CatiaZeroEntityEndpointLocusCandidate {
            id: "catia:zero-entity:endpoint-locus-candidate#0".to_string(),
            incident_endpoint_pair_endpoints: vec![
                crate::native::CatiaZeroEntityEndpointPairEndpoint {
                    endpoint_pair: "catia:zero-entity:endpoint-pair-candidate#0".to_string(),
                    endpoint_index: 0,
                },
            ],
            representative_point: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            maximum_deviation: 0.0,
        });
    let mut invalid_endpoint_locus_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_endpoint_locus
        .store(&mut invalid_endpoint_locus_namespace)
        .expect("store invalid CATIA zero-entity endpoint-locus candidate");
    assert!(crate::native::CatiaNative::load(&invalid_endpoint_locus_namespace).is_err());

    let mut invalid_model_endpoint = native.clone();
    invalid_model_endpoint.zero_entity_support_runs[0].supports[0]
        .model_endpoints
        .as_mut()
        .expect("model endpoints")[0]
        .x = f64::NAN;
    let mut invalid_model_endpoint_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_model_endpoint
        .store(&mut invalid_model_endpoint_namespace)
        .expect("store invalid CATIA zero-entity model endpoint");
    assert!(crate::native::CatiaNative::load(&invalid_model_endpoint_namespace).is_err());

    let mut invalid = native;
    invalid.zero_entity_support_runs[0].supports[0].uv_endpoints = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity support run");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_closed_zero_entity_endpoint_tapes() {
    let mut stream = zero_entity_face_loop_support_stream();
    let support = 0x6a + 12;
    stream[support + 13..support + 17].copy_from_slice(&1u32.to_le_bytes());
    let first_endpoint: [u8; 16] = stream[support + 93..support + 109]
        .try_into()
        .expect("endpoint pair");
    stream[support + 109..support + 125].copy_from_slice(&first_endpoint);

    let native = crate::native::CatiaNative::decode(&stream);
    let loop_record = &native.zero_entity_support_runs[0]
        .face
        .as_ref()
        .expect("face")
        .loops[0];
    assert_eq!(
        loop_record.oriented_model_endpoints,
        [[
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
            cadmpeg_ir::math::Point3::new(-1.0, 6.0, 3.0),
        ]]
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity endpoint tape");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA zero-entity endpoint tape"),
        native
    );
}

#[test]
fn native_namespace_retains_zero_entity_ownership_root() {
    let mut stream = zero_entity_face_support_stream();
    stream.extend(zero_entity_ownership_stream(1));
    let native = crate::native::CatiaNative::decode(&stream);
    let [root] = native.zero_entity_ownership_roots.as_slice() else {
        panic!("one zero-entity ownership root")
    };
    assert_eq!(root.face_slots, [1]);
    assert_eq!(root.face_roster_record_ordinal, 4);
    assert_eq!(root.shell_record_ordinal, 5);
    assert_eq!(root.body_record_ordinal, 6);
    assert_eq!(
        native.zero_entity_records[3].logical_end,
        root.shell_byte_offset
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity ownership root");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load CATIA zero-entity ownership root"),
        native
    );

    let mut invalid = native;
    invalid.zero_entity_ownership_roots[0].face_slots.clear();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity ownership root");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_separate_zero_entity_topology_registries() {
    let native = crate::native::CatiaNative::decode(&zero_entity_topology_stream());
    assert_eq!(native.zero_entity_records.len(), 8);
    assert_eq!(native.zero_entity_records[0].record_ordinal, 1);
    assert_eq!(native.zero_entity_records[0].tag, [0x5e, 0x1a]);
    let [edge_stride] = native.zero_entity_edge_strides.as_slice() else {
        panic!("one zero-entity edge stride")
    };
    assert_eq!(edge_stride.record_ordinal, 1);
    assert_eq!(edge_stride.allocations, [5, 7, 8, 4, 3]);

    let [pair] = native.zero_entity_oriented_use_pairs.as_slice() else {
        panic!("one zero-entity oriented-use pair")
    };
    assert_eq!(pair.header_record_ordinal, 2);
    assert_eq!(pair.base_columns, [100, 200]);

    let [incidence] = native.zero_entity_vertex_incidences.as_slice() else {
        panic!("one zero-entity vertex incidence")
    };
    assert_eq!(incidence.record_ordinal, 5);
    assert_eq!(incidence.allocations, [1, 2, 5]);
    assert_eq!(
        incidence.vertex_record.as_deref(),
        Some("catia:zero-entity:record#6")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA zero-entity topology registries");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace)
            .expect("load CATIA zero-entity topology registries"),
        native
    );

    let mut invalid = native.clone();
    invalid.zero_entity_edge_strides[0].allocations[0] = 0;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity edge allocation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native.clone();
    invalid.zero_entity_vertex_incidences[0].vertex_record = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity vertex owner");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut invalid = native;
    invalid.zero_entity_oriented_use_pairs[0].uses[1].allocations[0] += 1;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA zero-entity topology registries");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn zero_entity_vertex_binding_declines_atomically_when_structure_changes() {
    let bytes = zero_entity_topology_stream();
    let native = crate::native::CatiaNative::decode(&bytes);
    let vertex_offset =
        usize::try_from(native.zero_entity_records[5].byte_offset).expect("fixture byte offset");

    let mut missing_vertex = bytes;
    missing_vertex[vertex_offset + 2] = 0x60;
    let missing_vertex = crate::native::CatiaNative::decode(&missing_vertex);
    assert!(missing_vertex.zero_entity_vertex_incidences.is_empty());

    let mut separated_vertex = zero_entity_topology_stream();
    separated_vertex.insert(vertex_offset, 0xff);
    let separated_vertex = crate::native::CatiaNative::decode(&separated_vertex);
    assert!(separated_vertex.zero_entity_vertex_incidences.is_empty());
}

#[test]
fn decode_reports_zero_entity_surface_support_runs() {
    let mut file = standard_catpart();
    file.splice(16..16, zero_entity_face_loop_support_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity support run");
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_face_bound_support_run_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_face_terminal_control_03_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_face_terminal_control_05_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_loop_terminal_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_loop_record_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_loop_class_41_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_loop_class_50_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_loop_class_c1_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_forward_loop_member_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_reversed_loop_member_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_oriented_loop_member_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_support_run_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_support_occurrence_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_support_pcurve_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_support_model_curve_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_support_model_construction_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_uv_endpoint_pair_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_model_midpoint_count"],
        1
    );
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Topology
            && loss
                .message
                .contains("1 zero-entity surface-support run(s)")
            && loss
                .message
                .contains("1 run(s) bind the complete face roster")
            && loss.message.contains("1 stored member sense(s)")
            && loss.message.contains("oriented-use")
    }));
}

#[test]
fn decode_reports_separate_zero_entity_topology_registries() {
    let mut file = standard_catpart();
    file.splice(16..16, zero_entity_topology_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode zero-entity topology registries");
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_record_count"],
        8
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_edge_stride_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_edge_stride_allocation_count"],
        5
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_oriented_use_pair_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_oriented_use_count"],
        2
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_oriented_use_allocation_count"],
        4
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_vertex_incidence_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_vertex_incidence_allocation_count"],
        3
    );
    assert_eq!(
        decoded.report.coverage["decoded_zero_entity_vertex_owner_binding_count"],
        1
    );
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Topology
            && loss.message.contains("1 edge-stride allocation tuple(s)")
            && loss.message.contains("1 oriented-use pair(s)")
            && loss.message.contains("1 vertex-incidence record(s)")
            && loss.message.contains("remain separate")
            && loss.message.contains("bind their adjacent vertex owner")
            && loss.message.contains("loop-to-use")
    }));
}

#[test]
fn standard_decode_refines_a_unique_quantized_analytic_carrier() {
    let exact_x = 1.000_000_01_f64;
    let mut surf = surf_stream();
    for (index, value) in [exact_x as f32, 2.0_f32, 3.0_f32, 1.0_f32, 0.0_f32, 2.0_f32]
        .into_iter()
        .enumerate()
    {
        surf[8 + 4 * index..12 + 4 * index].copy_from_slice(&be_f32(value));
    }
    let mut consolidated = b2_cylinder_stream();
    consolidated[5..13].copy_from_slice(&le_f64(exact_x));
    let mut file = standard_catpart_from_streams(&main_stream(), &surf);
    file.splice(16..16, consolidated);
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode exact consolidated analytic refinement");
    let surface = decoded
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "catia:standard:surf#0")
        .expect("refined standard cylinder");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Cylinder { origin, axis, .. }
            if origin.x == exact_x
                && axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(
        decoded.report.coverage["refined_consolidated_analytic_surface_count"],
        1
    );
}

#[test]
fn native_namespace_retains_exact_consolidated_torus_charts() {
    let native = crate::native::CatiaNative::decode(&b2_torus_stream());
    let [torus] = native.consolidated_tori.as_slice() else {
        panic!("one consolidated torus")
    };
    assert_eq!(torus.center, [1.0, 2.0, 3.0]);
    assert_eq!(torus.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(torus.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(torus.axis, [0.0, 0.0, 1.0]);
    assert_eq!(torus.major_radius, 7.0);
    assert_eq!(torus.minor_radius, 2.0);
    assert_eq!(
        torus.major_angular_range,
        [
            std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(torus.major_angular_domain, [0.0, std::f64::consts::TAU]);
    assert_eq!(torus.minor_angular_range, [0.0, std::f64::consts::PI]);
    assert_eq!(
        torus.minor_angular_domain,
        [
            -std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2
        ]
    );
    assert_eq!(torus.major_scale, 14.0);
    assert_eq!(torus.minor_scale, 4.0);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA torus");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA torus"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_tori[0].major_angular_domain[0] += 0.25;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA torus for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_exact_consolidated_sphere_charts() {
    let native = crate::native::CatiaNative::decode(&b2_sphere_stream());
    let [sphere] = native.consolidated_spheres.as_slice() else {
        panic!("one consolidated sphere")
    };
    assert_eq!(sphere.center, [1.0, 2.0, 3.0]);
    assert_eq!(sphere.direction_x, [1.0, 0.0, 0.0]);
    assert_eq!(sphere.direction_y, [0.0, 1.0, 0.0]);
    assert_eq!(sphere.axis, [0.0, 0.0, 1.0]);
    assert_eq!(sphere.radius, 5.0);
    assert_eq!(sphere.azimuth_range, [-2.0, 4.0]);
    assert_eq!(sphere.latitude_range, [-1.0, std::f64::consts::FRAC_PI_2]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store CATIA sphere");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA sphere"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_spheres[0].latitude_range.reverse();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid CATIA sphere for load validation");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_retains_consolidated_owner_packet_and_allocation_link() {
    let native = crate::native::CatiaNative::decode(&b2_linked_owner_stream());
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    let crate::native::CatiaOwnerPacketPayload::FixedNine {
        references,
        numeric_tail,
        ..
    } = &packet.payload
    else {
        panic!("fixed-nine owner payload")
    };
    assert_eq!(*references, [1000, 1, 1001, 2, 1002, 3, 1003, 4, 1004]);
    assert_eq!(numeric_tail.header, [0x84, 0x41, 0xbb, 0x05, 0x0d]);
    assert_eq!(numeric_tail.lower, [-0.0, 4.5]);
    assert_eq!(numeric_tail.upper, [12.25, 7.0]);
    assert_eq!(numeric_tail.bounds, [[-2.0, 1.0], [3.5, 4.0], [5.25, 6.0]]);
    let link = packet.allocation_link.expect("allocation-successor link");
    assert_eq!(link.byte_len, 11);
    assert_eq!(link.target, 1003);
    assert_eq!(link.target + 1, references[8]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store CATIA owner packet");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load CATIA owner packet"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_owner_packets[0]
        .allocation_link
        .as_mut()
        .expect("allocation-successor link")
        .target -= 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid CATIA owner packet");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_count_framed_owner_packet_and_allocation_link() {
    let native = crate::native::CatiaNative::decode(&b2_linked_counted_owner_stream());
    let [packet] = native.consolidated_owner_packets.as_slice() else {
        panic!("one consolidated owner packet")
    };
    let crate::native::CatiaOwnerPacketPayload::Counted { references, tail } = &packet.payload
    else {
        panic!("count-framed owner payload")
    };
    assert_eq!(references, &[911, 7, 263, 258, 281, 276, 917]);
    assert_eq!(tail, &[0x83, 0x41, 0x92, 0x00, 0x01]);
    let link = packet.allocation_link.expect("allocation-successor link");
    assert_eq!(link.target, 916);
    assert_eq!(
        link.target + 1,
        *references.last().expect("final owner reference")
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store count-framed CATIA owner packet");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load count-framed CATIA owner packet"),
        native
    );

    let mut invalid = native;
    let crate::native::CatiaOwnerPacketPayload::Counted { tail, .. } =
        &mut invalid.consolidated_owner_packets[0].payload
    else {
        panic!("count-framed owner payload")
    };
    tail.clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid count-framed CATIA owner packet");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
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

    let mut file = standard_catpart();
    file.splice(16..16, bytes.clone());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode consolidated edge-run coverage");
    assert_eq!(
        decoded.report.coverage["decoded_consolidated_edge_run_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_consolidated_edge_run_support_binding_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unresolved_consolidated_edge_run_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["partially_resolved_consolidated_edge_run_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["fully_resolved_consolidated_edge_run_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_consolidated_edge_run_shared_locus_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_consolidated_edge_run_endpoint_locus_count"],
        0
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
fn consolidated_edge_nodes_require_canonical_headers_and_terminal_controls() {
    let bytes = b2_edge_node_stream();
    assert_eq!(crate::families::b2::records::b2_edge_nodes(&bytes).len(), 1);

    let mut noncanonical_header = bytes.clone();
    noncanonical_header[0] = 0xb3;
    noncanonical_header[4] = 0x04;
    noncanonical_header.insert(5, 1);
    assert!(crate::families::b2::records::b2_edge_nodes(&noncanonical_header).is_empty());

    let mut wide_header = bytes.clone();
    wide_header[0] = 0xb3;
    wide_header[4] = 0x04;
    wide_header.insert(5, 0x40);
    let wide_nodes = crate::families::b2::records::b2_edge_nodes(&wide_header);
    let [wide_node] = wide_nodes.as_slice() else {
        panic!("canonical wide-header edge node")
    };
    assert_eq!(wide_node.header_token, 0x4004);

    let mut invalid_terminal = bytes;
    *invalid_terminal.last_mut().expect("edge terminal") = 0x03;
    assert!(crate::families::b2::records::b2_edge_nodes(&invalid_terminal).is_empty());
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

    namespace
        .set_arena(
            "consolidated_cylinders",
            &Vec::<crate::native::CatiaConsolidatedCylinder>::new(),
        )
        .expect("remove retained cylinders");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
}

#[test]
fn native_namespace_retains_embedded_cylinders_with_their_owning_group() {
    let native = crate::native::CatiaNative::decode(&b2_embedded_cylinder_stream());
    assert!(native.consolidated_cylinders.is_empty());
    let [group] = native.consolidated_groups.as_slice() else {
        panic!("one consolidated group");
    };
    let [cylinder] = native.consolidated_embedded_cylinders.as_slice() else {
        panic!("one embedded consolidated cylinder");
    };
    assert_eq!(group.group_type, 3);
    assert_eq!(cylinder.group, group.id);
    assert_eq!(cylinder.object_id, 0x5678);
    assert_eq!(cylinder.u_range, [0.0, 4.0 * std::f64::consts::PI]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store embedded CATIA cylinder");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load embedded CATIA cylinder"),
        native
    );

    namespace
        .set_arena(
            "consolidated_groups",
            &Vec::<crate::native::CatiaConsolidatedGroup>::new(),
        )
        .expect("remove owning consolidated group");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut two_groups = b2_embedded_cylinder_stream();
    two_groups.extend_from_slice(&b2_embedded_cylinder_stream());
    let mut invalid = crate::native::CatiaNative::decode(&two_groups);
    assert_eq!(invalid.consolidated_groups.len(), 2);
    assert_eq!(invalid.consolidated_embedded_cylinders.len(), 2);
    invalid.consolidated_embedded_cylinders[1]
        .group
        .clone_from(&invalid.consolidated_groups[0].id);
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store cross-group embedded cylinder");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());
}

#[test]
fn native_namespace_binds_edges_to_retained_embedded_cylinders() {
    use crate::native::CatiaConsolidatedSupportBinding;

    let mut bytes = b2_embedded_cylinder_stream();
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
        Some(CatiaConsolidatedSupportBinding::EmbeddedCylinder { .. })
    )));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store embedded-cylinder edge binding");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load embedded-cylinder edge binding"),
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
    circle.push(0x01);
    circle.extend_from_slice(&0.0_f64.to_le_bytes());
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
    let binding = native.consolidated_edge_nodes[0]
        .analytic_circle
        .as_ref()
        .expect("native analytic circle");
    assert_eq!(binding.circle, "catia:consolidated:circle#0");
    assert_eq!(native.consolidated_circles[0].center_pair, [12.0, 34.0]);
    assert_eq!(native.consolidated_circles[0].range, [0.0, 10.0]);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store analytic circle binding");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load analytic circle binding"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_edge_nodes[0]
        .analytic_circle
        .as_mut()
        .expect("analytic circle binding")
        .circle = "missing".to_string();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid analytic circle binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

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
fn object_graph_payload_assigns_blobs_only_inside_the_terminator_boundary() {
    use crate::object_graph::PayloadField;

    let valid = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[0xe5, 1, 0, 0, 0, 0xaa, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&valid).expect("bounded blob");
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Blob {
                declared_len: 1,
                bytes,
                ..
            },
            PayloadField::Terminator
        ] if bytes.as_slice() == [0xaa]
    ));

    let unbounded = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[0xe5, 0xfd, 0xd8, 0xc1, 0x74, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&unbounded).expect("literal E5 atom");
    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            PayloadField::Atom {
                value: 0xe5,
                offset: 0
            },
            ..,
            PayloadField::Terminator
        ]
    ));
}

#[test]
fn object_graph_payload_preserves_the_complete_terminator_run() {
    let bytes =
        object_graph_from_records(&[object_graph_record(&[0x04], &[0x83, 0xfe, 0xfe, 0xfe])]);
    let graph = crate::object_graph::parse(&bytes).expect("multi-terminator payload");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::Atom { value: 3, .. },
            crate::object_graph::PayloadField::Terminator,
            crate::object_graph::PayloadField::Terminator,
            crate::object_graph::PayloadField::Terminator,
        ]
    ));
}

#[test]
fn object_graph_payload_reads_tagged_fixed_width_references() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04],
        &[
            0x81, 0x80, 0xfe, 0x1e, 0, 0, 0x81, 0x32, 0xeb, 0, 0, 0, 0xfe,
        ],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("tagged fixed-width references");

    assert!(matches!(
        graph.records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::Reference { value: 7934, .. },
            crate::object_graph::PayloadField::Reference { value: 235, .. },
            crate::object_graph::PayloadField::Terminator,
        ]
    ));
}

#[test]
fn object_graph_lists_retain_direct_fixed_width_references() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x81],
            &[0x3b, 0x81, 0x32, 2, 0, 0, 0, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x82, 0x81], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);

    assert!(matches!(
        native.object_graphs[0].records[0].payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 1,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
    ));
    assert_eq!(
        native.object_graphs[0].records[0].references[0].entity_id,
        2
    );
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
fn outer_object_graph_preserves_inline_records() {
    let nested = object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe]);
    let inline = inline_object_graph_record(&[
        0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
    ]);
    let graph = crate::object_graph::parse(&object_graph_from_records(&[nested, inline]))
        .expect("inline control record");

    assert_eq!(graph.records.len(), 2);
    assert_eq!(graph.records[1].lead, 0x10);
    assert!(graph.records[1].head.is_empty());
    assert_eq!(
        graph.records[1].inline_body.as_deref(),
        Some(&[0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,][..])
    );
    assert!(graph.records[1].payload.fields.is_empty());
}

#[test]
fn outer_object_graph_accepts_each_inline_layout() {
    let bodies = [
        vec![
            0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x82, 0xd3, 0x79, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd4, 0x33, 0x82, 0x32, 0xe6, 0x00, 0x00, 0x00, 0x32, 0xe4, 0x00, 0x00,
            0x00, 0x82, 0xb1, 0x81, 0x06,
        ],
        vec![
            0x10, 0xfe, 0xd4, 0x32, 0x82, 0x32, 0xe6, 0x00, 0x00, 0x00, 0x32, 0xe4, 0x00, 0x00,
            0x00, 0x82, 0xd1, 0xfd, 0x82, 0xd4, 0x34, 0x06,
        ],
    ];

    for body in bodies {
        let graph =
            crate::object_graph::parse(&object_graph_from_records(&[inline_object_graph_record(
                &body,
            )]))
            .expect("assigned inline control layout");
        assert_eq!(
            graph.records[0].inline_body.as_deref(),
            Some(body.as_slice())
        );
    }
}

#[test]
fn outer_object_graph_rejects_unassigned_childless_records() {
    let valid = [
        0x10, 0xfe, 0xd3, 0x77, 0x82, 0xf2, 0xf0, 0x82, 0xd3, 0x5f, 0x81, 0x06,
    ];
    for index in [0, 1, 4, 10, 11] {
        let mut body = valid;
        body[index] ^= 1;
        assert!(crate::object_graph::parse(&object_graph_from_records(&[
            inline_object_graph_record(&body)
        ]))
        .is_none());
    }
    assert!(
        crate::object_graph::parse(&object_graph_from_records(&[inline_object_graph_record(
            &[0x10, 0xfe, 0x81, 0x06]
        )]))
        .is_none()
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
fn outer_object_graph_reads_extended_compact_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x12, 0x82, 0x80, 0x83, 0, 0], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x80, 0xe8, 0x16, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.owner_ref, Some(2));
        assert_eq!(record.class_ref, None);
        assert_eq!(record.storage_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_incomplete_extended_compact_owner_framing() {
    for head in [
        &[0x12, 0x82, 0x80, 0x83, 0][..],
        &[0x12, 0x82, 0x80, 0x83, 0, 1][..],
        &[0x12, 0x80, 0x80, 0x83, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x16, 0x94, 0x80, 0x95, 22, 0, 0, 0x80, 0x96, 0, 0],
            &[0xfe],
        ),
        object_graph_record(&[0x16, 0x94, 0x80, 0x95, 0, 0, 0x80, 17, 28, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
        assert_eq!(record.owner_ref, Some(21));
    }
}

#[test]
fn outer_object_graph_reads_short_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0, 0], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x95, 0x80, 17, 21, 0, 0], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("short extended compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(21));
        assert_eq!(record.owner_ref, Some(0));
    }
}

#[test]
fn outer_object_graph_reads_reference_terminated_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x96, 0x80, 0x97, 0, 0], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x80, 0x96, 23, 0, 0, 0xd2, 0x2b], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 0x80, 123, 21, 0, 0, 0xd2, 0x2b], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("reference-terminated compact heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
    }
    assert_eq!(graph.records[0].storage_ref, Some(22));
    assert_eq!(graph.records[0].owner_ref, Some(0));
    for record in &graph.records[1..] {
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[1].owner_ref, Some(22));
    assert_eq!(graph.records[2].owner_ref, None);
    for record in &graph.records[1..] {
        assert!(matches!(
            record.head.last(),
            Some(crate::object_graph::HeadToken::Reference(300))
        ));
    }
}

#[test]
fn outer_object_graph_rejects_partial_reference_terminated_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x96, 23, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x96, 22, 0, 0, 0x97][..],
        &[0x16, 0x94, 0x80, 0x96, 23, 0, 0, 97][..],
        &[0x16, 0x94, 0x80, 0x80, 0x97, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_partial_short_extended_class_storage_owner_roles() {
    for head in [
        &[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0][..],
        &[0x16, 0x94, 0x95, 0x80, 0x96, 20, 0, 1][..],
        &[0x16, 0x94, 0x80, 0x80, 0x96, 20, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_two_block_extended_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[0x16, 0x94, 0x80, 95, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[0x16, 0x94, 0x80, 1, 23, 0, 0, 0x80, 0x96, 25, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("two-block extended compact heads");

    assert_eq!(graph.records[0].owner_ref, Some(21));
    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[1].owner_ref, None);
    assert_eq!(graph.records[2].owner_ref, None);
}

#[test]
fn outer_object_graph_retains_roles_before_a_literal_short_extended_owner() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x16, 0x94, 0x80, 66, 23, 0, 0, 0x80, 0x97, 0, 0],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("literal-owner extended head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(0));
    assert_eq!(record.owner_ref, None);
    assert_eq!(record.owner_literal, Some(66));
}

#[test]
fn outer_object_graph_rejects_partial_two_block_extended_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 25, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 24, 0, 0, 0x80, 0x96, 25, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 23, 0, 0, 0x80, 0x96, 26, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_partial_extended_class_storage_owner_roles() {
    for head in [
        &[0x16, 0x94, 0x80, 0x95, 22, 0, 1, 0x80, 0x96, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x95, 0, 0, 0x80, 17, 29, 0, 0][..],
        &[0x16, 0x94, 0x80, 0x80, 22, 0, 0, 0x80, 0x96, 0, 0][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_class_storage_owner_compact_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x16, 0x92, 0xd2, 0x2b, 0xd2, 0x39],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("class-storage-owner compact head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(18));
    assert_eq!(record.storage_ref, Some(300));
    assert_eq!(record.owner_ref, Some(314));
}

#[test]
fn outer_object_graph_retains_class_first_roles_before_an_unassigned_slot() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x16, 0x94, 0x95, 95], &[0xfe]),
        object_graph_record(&[0x16, 0x94, 95, 0x96], &[0xfe]),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("class-first compact heads");

    assert_eq!(graph.records[0].class_ref, Some(20));
    assert_eq!(graph.records[0].storage_ref, Some(21));
    assert_eq!(graph.records[0].owner_ref, None);
    assert_eq!(graph.records[1].class_ref, Some(20));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[1].owner_ref, None);
}

#[test]
fn outer_object_graph_reads_null_lane_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b],
            &[0xfe],
        ),
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 0, 0],
            &[0xfe],
        ),
        object_graph_record(
            &[
                0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 23, 0, 0,
            ],
            &[0xfe],
        ),
        object_graph_record(
            &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 95, 23, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("null-lane compact head");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[0].owner_ref, Some(300));
    for record in &graph.records[1..] {
        assert_eq!(record.owner_ref, Some(0));
    }
}

#[test]
fn outer_object_graph_reads_terminal_null_lane_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminal null-lane head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(0));
    assert_eq!(record.owner_ref, Some(300));
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_rejects_incomplete_terminal_null_lane_roles() {
    for head in [
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b][..],
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0xd2, 0x2b, 0x84][..],
        &[0x5a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x83][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained terminal null-lane head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_terminal_lane_class_storage_owner_roles() {
    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x56, 0x94, 0x95, 0x96, 0x83],
        &[0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminal-lane compact head");
    let record = &graph.records[0];

    assert_eq!(record.class_ref, Some(20));
    assert_eq!(record.storage_ref, Some(21));
    assert_eq!(record.owner_ref, Some(22));
    assert!(matches!(
        record.head.last(),
        Some(crate::object_graph::HeadToken::Reference(3))
    ));
}

#[test]
fn outer_object_graph_reads_extended_terminal_lane_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(
            &[0x56, 0x94, 0x80, 0x96, 22, 0, 0, 0x80, 0x97, 0, 0, 0x83],
            &[0xfe],
        ),
        object_graph_record(
            &[0x56, 0x94, 0x80, 96, 23, 0, 0, 0x80, 97, 25, 0, 0, 0x83],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended terminal-lane heads");

    for record in &graph.records {
        assert_eq!(record.class_ref, Some(20));
        assert_eq!(record.storage_ref, Some(0));
    }
    assert_eq!(graph.records[0].owner_ref, Some(22));
    assert_eq!(graph.records[1].owner_ref, None);
}

#[test]
fn outer_object_graph_rejects_incomplete_terminal_lane_roles() {
    for head in [
        &[0x56, 0x94, 0x95, 0x96][..],
        &[0x56, 0x94, 0x95, 0x96, 0x84][..],
        &[0x56, 0x94, 0x95, 0x80, 0x83][..],
        &[0x56, 0x94, 0x80, 0x96, 22, 0, 0, 0x80, 0x97, 0, 0, 0x84][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
        assert_eq!(graph.records[0].owner_ref, None);
    }
}

#[test]
fn outer_object_graph_rejects_incomplete_null_lane_roles() {
    for head in [
        &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff][..],
        &[0x1a, 0x94, 0x80, 0, 0, 0, 0, 0xd2, 0x2b][..],
        &[0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80][..],
        &[
            0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 24, 0, 0,
        ][..],
        &[
            0x1a, 0x94, 0x80, 0xff, 0xff, 0xff, 0xff, 0x80, 0x95, 23, 0, 1,
        ][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn outer_object_graph_reads_extended_owner_class_storage_roles() {
    let bytes = object_graph_from_records(&[
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 22, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 0, 0, 0x83], &[0xfe]),
        object_graph_record(&[0x52, 0x92, 0x80, 0x95, 0, 0, 0x80, 0x95, 0, 0], &[0xfe]),
        object_graph_record(
            &[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x80, 95, 22, 0, 0],
            &[0xfe],
        ),
    ]);
    let graph = crate::object_graph::parse(&bytes).expect("extended compact heads");

    for record in &graph.records {
        assert_eq!(record.owner_ref, Some(18));
        assert_eq!(record.class_ref, Some(0));
    }
    assert_eq!(graph.records[0].storage_ref, Some(21));
    assert_eq!(graph.records[1].storage_ref, None);
    assert_eq!(graph.records[2].storage_ref, Some(21));
    assert_eq!(graph.records[3].storage_ref, Some(21));
    assert_eq!(graph.records[4].storage_ref, None);
}

#[test]
fn outer_object_graph_rejects_incomplete_extended_owner_class_storage_roles() {
    for head in [
        &[0x52, 0x92, 0x80, 0x95, 22, 0, 0, 0x84][..],
        &[0x52, 0x92, 0x80, 0x95, 0, 0, 0x80, 0x96, 0, 0][..],
        &[0x52, 0x92, 0x80, 95, 22, 0, 0, 0x80, 95, 23, 0, 0][..],
        &[0x52, 0x80, 0x80, 0x95, 22, 0, 0, 0x83][..],
    ] {
        let bytes = object_graph_from_records(&[object_graph_record(head, &[0xfe])]);
        let graph = crate::object_graph::parse(&bytes).expect("retained compact head");

        assert_eq!(graph.records[0].owner_ref, None);
        assert_eq!(graph.records[0].class_ref, None);
        assert_eq!(graph.records[0].storage_ref, None);
    }
}

#[test]
fn object_graph_payload_reads_fixed_width_escaped_values() {
    use crate::object_graph::PayloadField;

    let records = [
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x83],
            &[
                0x80, 0x78, 0x56, 0x34, 0x12, 0x32, 2, 0, 0, 0, 0x32, 0xef, 0xcd, 0xab, 0x89, 0xfe,
            ],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
    ];
    let bytes = object_graph_from_records(&records);
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
    let native =
        crate::native::CatiaNative::decode(&sequential_entity_backed_object_graph(&records));
    assert_eq!(
        native.object_graphs[0].records[0].references,
        [
            crate::native::CatiaObjectRecordReference {
                entity_id: 2,
                payload_offset: 5,
                source: crate::native::CatiaObjectRecordReferenceSource::Field,
                is_null: false,
                target: Some(native.object_graphs[0].records[1].id.clone()),
                design_object: native.object_graphs[0].records[1].design_object.clone(),
            },
            crate::native::CatiaObjectRecordReference {
                entity_id: 0x89ab_cdef,
                payload_offset: 10,
                source: crate::native::CatiaObjectRecordReferenceSource::Field,
                is_null: false,
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
    let bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(
            &[0x04, 0x01, 0x81, 0x83],
            &[0x3b, 0x82, 0x81, 0x83, 0x81, 0x83, 0xfe],
        ),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x81, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0x81, 0x81, 0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_entity_id, 1);
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
        [
            crate::native::CatiaObjectRecordReference {
                entity_id: 3,
                payload_offset: 2,
                source: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                    list_payload_offset: 0,
                    item_ordinal: 0,
                },
                is_null: false,
                target: Some(graph.records[2].id.clone()),
                design_object: graph.records[2].design_object.clone(),
            },
            crate::native::CatiaObjectRecordReference {
                entity_id: 3,
                payload_offset: 4,
                source: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                    list_payload_offset: 0,
                    item_ordinal: 1,
                },
                is_null: false,
                target: Some(graph.records[2].id.clone()),
                design_object: graph.records[2].design_object.clone(),
            },
        ]
    );
    assert_eq!(
        native.design_objects[0].relations,
        [
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 2,
                    container: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                        list_payload_offset: 0,
                        item_ordinal: 0,
                    },
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 4,
                    container: crate::native::CatiaObjectRecordReferenceSource::ListItem {
                        list_payload_offset: 0,
                        item_ordinal: 1,
                    },
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[1].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 0,
                    container: crate::native::CatiaObjectRecordReferenceSource::Field,
                },
                target_entity_id: 1,
                target_field: graph.records[0].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[0].id.clone()),
            },
        ]
    );
    assert_eq!(
        graph.records[1].references,
        [crate::native::CatiaObjectRecordReference {
            entity_id: 1,
            payload_offset: 0,
            source: crate::native::CatiaObjectRecordReferenceSource::Field,
            is_null: false,
            target: Some(graph.records[0].id.clone()),
            design_object: graph.records[0].design_object.clone(),
        }]
    );
    assert_eq!(native.design_objects[1].owner_entity_id, 3);
    assert_eq!(native.design_objects[1].ordinal, 1);
    assert_eq!(
        native.design_objects[1].first_field_byte_offset,
        native.object_graphs[0].records[2].byte_offset
    );
    assert_eq!(
        native.design_objects[1].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: graph.records[2].id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 1,
            target_field: graph.records[0].id.clone(),
            target_class: None,
            target_design_object: Some(native.design_objects[0].id.clone()),
        }]
    );
}

#[test]
fn native_design_objects_preserve_storage_relations_before_payload_relations() {
    let bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x84, 0x83], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x86], &[0xfe]),
    ]);
    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = &native.object_graphs[0];
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode storage and payload relations");

    assert_eq!(
        native.design_objects[0].relations,
        [
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Storage,
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
            crate::native::CatiaDesignObjectRelation {
                source_field: graph.records[0].id.clone(),
                source_class: None,
                source: crate::native::CatiaDesignObjectRelationSource::Payload {
                    payload_offset: 0,
                    container: crate::native::CatiaObjectRecordReferenceSource::Field,
                },
                target_entity_id: 3,
                target_field: graph.records[2].id.clone(),
                target_class: None,
                target_design_object: Some(native.design_objects[1].id.clone()),
            },
        ]
    );
    assert_eq!(
        decoded.report.coverage["decoded_design_object_relation_count"],
        2
    );

    let mut malformed = native.clone();
    malformed.design_objects[0].relations.swap(0, 1);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store reordered design relations");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_design_objects_preserve_relations_to_unowned_fields() {
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81], &[0x81, 0x82, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0xe5, 0xff, 0xff, 0xff, 0xe4], &[0xfe]),
    ];
    let bytes = sequential_entity_backed_object_graph(&records);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode relation to unowned field");
    let native =
        crate::native::CatiaNative::load(decoded.ir.native.namespace("catia").expect("namespace"))
            .expect("load relation to unowned field");
    let graph = &native.object_graphs[0];

    assert_eq!(native.design_objects.len(), 1);
    assert_eq!(
        native.design_objects[0].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: graph.records[0].id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 2,
            target_field: graph.records[1].id.clone(),
            target_class: None,
            target_design_object: None,
        }]
    );
    assert_eq!(
        decoded.report.coverage["decoded_design_unowned_field_relation_count"],
        1
    );
}

#[test]
fn native_design_objects_preserve_reflexive_field_relations() {
    let records = [object_graph_record(
        &[0x04, 0x01, 0x81],
        &[0x81, 0x81, 0xfe],
    )];
    let bytes = sequential_entity_backed_object_graph(&records);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode reflexive field relation");
    let native =
        crate::native::CatiaNative::load(decoded.ir.native.namespace("catia").expect("namespace"))
            .expect("load reflexive field relation");
    let field = &native.object_graphs[0].records[0];

    assert_eq!(
        native.design_objects[0].relations,
        [crate::native::CatiaDesignObjectRelation {
            source_field: field.id.clone(),
            source_class: None,
            source: crate::native::CatiaDesignObjectRelationSource::Payload {
                payload_offset: 0,
                container: crate::native::CatiaObjectRecordReferenceSource::Field,
            },
            target_entity_id: 1,
            target_field: field.id.clone(),
            target_class: None,
            target_design_object: Some(native.design_objects[0].id.clone()),
        }]
    );
    assert_eq!(
        decoded.report.coverage["decoded_design_same_object_relation_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_design_reflexive_field_relation_count"],
        1
    );
}

#[test]
fn native_object_references_select_sparse_entity_identities() {
    let records = [
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x87, 0x86], &[0xfe]),
    ];
    let native =
        crate::native::CatiaNative::decode(&entity_backed_object_graph(&records, &[1, 3, 7]));
    let graph = &native.object_graphs[0];

    assert_eq!(
        native
            .entity_records
            .iter()
            .map(|record| record.entity_id)
            .collect::<Vec<_>>(),
        [1, 3, 7]
    );
    assert_eq!(
        graph
            .records
            .iter()
            .map(|record| record.entity_id)
            .collect::<Vec<_>>(),
        [Some(1), Some(3), Some(7)]
    );
    assert_eq!(
        graph.records[0].references[0].target.as_deref(),
        Some(graph.records[1].id.as_str())
    );
    assert_ne!(
        graph.records[0].references[0].target.as_deref(),
        Some(graph.records[2].id.as_str())
    );
    assert_eq!(
        native
            .design_objects
            .iter()
            .map(|object| object.owner_entity_id)
            .collect::<Vec<_>>(),
        [1, 3, 7]
    );
}

#[test]
fn native_design_relations_preserve_both_endpoint_schema_classes() {
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0x81, 0x83, 0xfe]),
        object_graph_record(&[0x04, 0x01, 0x81, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x86], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Limit",
        "Pad",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let relation = &native.design_objects[0].relations[0];
    assert_eq!(
        relation
            .source_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Profile")
    );
    assert_eq!(
        relation
            .target_class
            .as_ref()
            .map(|class| class.name.as_str()),
        Some("Pad")
    );
    assert_eq!(
        relation
            .source_class
            .as_ref()
            .map(|class| class.entry.as_str()),
        native.object_graphs[0].records[0].class_entry.as_deref()
    );
    assert_eq!(
        relation
            .target_class
            .as_ref()
            .map(|class| class.entry.as_str()),
        native.object_graphs[0].records[2].class_entry.as_deref()
    );
}

#[test]
fn compact_design_objects_use_field_vocabulary_not_anchor_class() {
    let mut bytes = sequential_entity_backed_object_graph(&[
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
    assert_eq!(object.owner_entity_id, 2);
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
fn null_storage_roles_are_not_unresolved_storage_links() {
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x16, 0x84, 0x80, 0x82], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ]);
    bytes.extend(value_block_stream(&[0x81]));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "BaseFeature",
    ]));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode null storage role");

    assert_eq!(
        decoded.report.coverage["unresolved_storage_record_count"],
        0
    );
}

#[test]
fn pattern_schema_definition_does_not_create_a_feature_instance() {
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0];
    let mut native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &definition,
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    native.entity_records[0]
        .definition_value
        .as_mut()
        .expect("definition value")
        .definition
        .value = "CircPattern".to_string();
    native.object_graphs[0].records[0].class_name = Some("Element1".to_string());

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);
    assert!(ir.model.features.is_empty());
    assert!(transfer.consumed_records().is_empty());
}

#[test]
fn prt_sketch_schema_field_does_not_create_a_feature_instance() {
    let records = [
        object_graph_record(&[0x12, 0x82, 0x83], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ];
    let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "PRTSketch",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.object_graphs[0].records[1].class_name.as_deref(),
        Some("PRTSketch")
    );

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

    assert!(ir.model.features.is_empty());
    assert!(ir.model.sketches.is_empty());
    assert!(transfer.consumed_records().is_empty());
}

#[test]
fn complete_standalone_principal_plane_declarations_transfer_one_history_node() {
    use cadmpeg_ir::features::{FeatureDefinition, PrincipalPlane};

    for (class, plane) in [
        ("xy-plane", PrincipalPlane::Top),
        ("yz-plane", PrincipalPlane::Right),
        ("zx-plane", PrincipalPlane::Front),
    ] {
        let records = [
            object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
            object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        ];
        let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
        bytes.extend(catalog_stream(&[
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            class,
        ]));
        let native = crate::native::CatiaNative::decode(&bytes);
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());

        let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

        assert!(ir.model.sketches.is_empty());
        assert_eq!(ir.model.features.len(), 1);
        assert_eq!(
            ir.model.features[0].definition,
            FeatureDefinition::DatumPrincipalPlane { plane }
        );
        assert_eq!(ir.model.features[0].source_tag.as_deref(), Some(class));
        assert_eq!(
            transfer.principal_plane_records,
            native.design_objects[0].fields.iter().cloned().collect()
        );

        let mut excluded_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let excluded = crate::design_feature::transfer_design_features(
            &mut excluded_ir,
            &native,
            Some(&std::collections::HashSet::new()),
        );
        assert!(excluded_ir.model.features.is_empty());
        assert!(excluded.consumed_records().is_empty());
    }
}

#[test]
fn mixed_or_payload_bearing_principal_plane_fields_do_not_transfer() {
    for (records, catalog) in [
        (
            vec![
                object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
                object_graph_record(&[0x12, 0x82, 0x85], &[0xfe]),
            ],
            vec![
                "CATCatalogManager",
                "catalogManager",
                "catalogLinks",
                "",
                "xy-plane",
                "yz-plane",
            ],
        ),
        (
            vec![
                object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
                object_graph_record(&[0x12, 0x82, 0x84], &[0x80, 0xfe]),
            ],
            vec![
                "CATCatalogManager",
                "catalogManager",
                "catalogLinks",
                "",
                "xy-plane",
            ],
        ),
    ] {
        let mut bytes = entity_backed_object_graph(&records, &[2, 3]);
        bytes.extend(catalog_stream(&catalog));
        let native = crate::native::CatiaNative::decode(&bytes);
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());

        let transfer = crate::design_feature::transfer_design_features(&mut ir, &native, None);

        assert!(ir.model.features.is_empty());
        assert!(transfer.principal_plane_records.is_empty());
    }
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

    assert_eq!(graph.records[0].owner_entity_id(), Some(0));
    assert_eq!(graph.records[1].owner_entity_id(), Some(4));
    assert!(graph
        .records
        .iter()
        .all(|record| record.design_object.is_some()));
    assert_eq!(native.design_objects.len(), 2);
    assert_eq!(native.design_objects[0].owner_entity_id, 0);
    assert_eq!(native.design_objects[1].owner_entity_id, 4);
    assert!(native
        .design_objects
        .iter()
        .all(|object| object.owner_record.is_none()));
}

#[test]
fn native_design_objects_retain_and_validate_parallel_reference_tables() {
    let list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x84, 0x85, 0xfe];
    let list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &list_b),
        object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Limit",
        "Profile",
        "Limit",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let table = native.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table");
    assert_eq!(table.columns, native.design_objects[0].fields);
    assert_eq!(table.column_classes.len(), table.columns.len());
    assert!(table.column_classes.iter().all(Option::is_some));
    assert_eq!(table.rows.len(), 2);
    assert_eq!(
        table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.entity_id)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![3, 4], vec![4, 3]]
    );
    assert!(table.rows.iter().flat_map(|row| &row.cells).all(|cell| {
        cell.field.is_some() && cell.field_class.is_some() && cell.design_object.is_some()
    }));
    assert_eq!(
        table.rows[0].matching_design_object,
        table.rows[0].cells[0].design_object
    );
    assert!(table.rows[0].matching_design_object.is_some());
    assert!(table.rows[1].matching_design_object.is_none());

    let expected = table.clone();
    let mut malformed = native.clone();
    malformed.design_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows[0]
        .cells[0]
        .entity_id += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed parallel reference table");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store current parallel reference table");
    let mut previous_objects: Vec<crate::native::CatiaDesignObject> = previous_namespace
        .arena_as("design_objects")
        .expect("load stored design objects");
    previous_objects[0].parallel_reference_table = None;
    previous_namespace
        .set_arena("design_objects", &previous_objects)
        .expect("store previous design objects");
    previous_namespace.version = 200;
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous parallel reference table");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_203_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_203_namespace)
        .expect("store current parallel reference row matches");
    let mut version_203_objects: Vec<crate::native::CatiaDesignObject> = version_203_namespace
        .arena_as("design_objects")
        .expect("load version 203 design objects");
    for row in &mut version_203_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
    {
        row.matching_design_object = None;
    }
    version_203_namespace
        .set_arena("design_objects", &version_203_objects)
        .expect("store version 203 design objects");
    version_203_namespace.version = 203;
    let migrated = crate::native::CatiaNative::load(&version_203_namespace)
        .expect("migrate version 203 parallel reference row matches");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_202_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_202_namespace)
        .expect("store current classified parallel reference columns");
    let mut version_202_objects: Vec<crate::native::CatiaDesignObject> = version_202_namespace
        .arena_as("design_objects")
        .expect("load version 202 design objects");
    version_202_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .column_classes
        .clear();
    version_202_namespace
        .set_arena("design_objects", &version_202_objects)
        .expect("store version 202 design objects");
    version_202_namespace.version = 202;
    let migrated = crate::native::CatiaNative::load(&version_202_namespace)
        .expect("migrate version 202 source field classes");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected.clone())
    );

    let mut version_201_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_201_namespace)
        .expect("store current classified parallel reference table");
    let mut version_201_objects: Vec<crate::native::CatiaDesignObject> = version_201_namespace
        .arena_as("design_objects")
        .expect("load version 201 design objects");
    for cell in version_201_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
    {
        cell.field_class = None;
    }
    version_201_namespace
        .set_arena("design_objects", &version_201_objects)
        .expect("store version 201 design objects");
    version_201_namespace.version = 201;
    let migrated = crate::native::CatiaNative::load(&version_201_namespace)
        .expect("migrate version 201 target field classes");
    assert_eq!(
        migrated.design_objects[0].parallel_reference_table,
        Some(expected)
    );

    let null_list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x85, 0x85, 0xfe];
    let null_list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x85, 0x86, 0xfe];
    let terminal_null =
        crate::native::CatiaNative::decode(&sequential_entity_backed_object_graph(&[
            object_graph_record(&[0x04, 0x01, 0x81, 0x83], &null_list_a),
            object_graph_record(&[0x04, 0x01, 0x81, 0x84], &null_list_b),
            object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
            object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
        ]));
    let null_table = terminal_null.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table with terminal null row");
    assert!(null_table.rows[1].cells.iter().all(|cell| {
        cell.entity_id == 5 && cell.is_null && cell.field.is_none() && cell.design_object.is_none()
    }));

    let mut version_210_namespace = cadmpeg_ir::NativeNamespace::default();
    terminal_null
        .store(&mut version_210_namespace)
        .expect("store terminal null parallel reference cells");
    let mut version_210_records: Vec<crate::native::CatiaObjectRecord> = version_210_namespace
        .arena_as("object_graph_records")
        .expect("load version 210 object records");
    for reference in version_210_records
        .iter_mut()
        .flat_map(|record| &mut record.references)
    {
        reference.is_null = false;
    }
    version_210_namespace
        .set_arena("object_graph_records", &version_210_records)
        .expect("store version 210 object records");
    let mut version_210_objects: Vec<crate::native::CatiaDesignObject> = version_210_namespace
        .arena_as("design_objects")
        .expect("load version 210 design objects");
    for cell in version_210_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows
        .iter_mut()
        .flat_map(|row| &mut row.cells)
    {
        cell.is_null = false;
    }
    version_210_namespace
        .set_arena("design_objects", &version_210_objects)
        .expect("store version 210 design objects");
    version_210_namespace.version = 210;
    let migrated = crate::native::CatiaNative::load(&version_210_namespace)
        .expect("migrate terminal null parallel reference cells");
    assert!(migrated.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("migrated parallel reference table")
        .rows[1]
        .cells
        .iter()
        .all(|cell| cell.is_null));

    let three_references = [0x3b, 0x83, 0x81, 0x83, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mismatched = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &three_references),
        object_graph_record(&[0x04, 0x01, 0x83, 0x85], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x84, 0x86], &[0xfe]),
    ]);
    assert!(
        crate::native::CatiaNative::decode(&mismatched).design_objects[0]
            .parallel_reference_table
            .is_none()
    );
}

#[test]
fn parallel_reference_row_match_requires_distinct_target_fields() {
    let list_a = [0x3b, 0x82, 0x81, 0x83, 0x81, 0x83, 0x85, 0xfe];
    let list_b = [0x3b, 0x82, 0x81, 0x84, 0x81, 0x83, 0x86, 0xfe];
    let mut bytes = sequential_entity_backed_object_graph(&[
        object_graph_record(&[0x04, 0x01, 0x81, 0x83], &list_a),
        object_graph_record(&[0x04, 0x01, 0x81, 0x84], &list_b),
        object_graph_record(&[0x04, 0x01, 0x83, 0x83], &[0xfe]),
        object_graph_record(&[0x04, 0x01, 0x83, 0x84], &[0xfe]),
    ]);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Profile",
        "Profile",
        "Profile",
        "Profile",
    ]));
    let native = crate::native::CatiaNative::decode(&bytes);
    let table = native.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("parallel reference table");

    assert!(table.rows[0].matching_design_object.is_some());
    assert!(table.rows[1].matching_design_object.is_none());
    assert_eq!(table.rows[1].cells[0].field, table.rows[1].cells[1].field);

    let mut version_204_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_204_namespace)
        .expect("store current parallel reference row matches");
    let mut version_204_objects: Vec<crate::native::CatiaDesignObject> = version_204_namespace
        .arena_as("design_objects")
        .expect("load version 204 design objects");
    version_204_objects[0]
        .parallel_reference_table
        .as_mut()
        .expect("parallel reference table")
        .rows[1]
        .matching_design_object = table.rows[1].cells[0].design_object.clone();
    version_204_namespace
        .set_arena("design_objects", &version_204_objects)
        .expect("store version 204 design objects");
    version_204_namespace.version = 204;

    let migrated = crate::native::CatiaNative::load(&version_204_namespace)
        .expect("migrate version 204 parallel reference row matches");
    assert!(migrated.design_objects[0]
        .parallel_reference_table
        .as_ref()
        .expect("migrated parallel reference table")
        .rows[1]
        .matching_design_object
        .is_none());
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
            .map(|object| object.owner_entity_id)
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
            .map(|object| object.owner_entity_id)
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
    assert!(native.design_objects[0].relations.is_empty());
    assert!(matches!(
        &native.object_graphs[0].records[0].payload.fields[0],
        crate::object_graph::PayloadField::List {
            declared_count: 3,
            items,
            ..
        } if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
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
    assert!(native.design_objects[0].relations.is_empty());
    assert!(matches!(
        record.payload.fields.as_slice(),
        [
            crate::object_graph::PayloadField::List {
                declared_count: 2,
                items,
                ..
            },
            crate::object_graph::PayloadField::Terminator,
        ] if items == &[crate::object_graph::ListItem::Reference {
            value: 2,
            offset: 2,
        }]
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
    assert!(native.design_objects[0].relations.is_empty());
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
    let mut native_bytes = entity_table_record(1);
    native_bytes.extend(entity_table_record(2));
    native_bytes.push(0xde);
    native_bytes.extend_from_slice(&bytes);
    let native = crate::native::CatiaNative::decode(&native_bytes);
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
fn object_graph_payload_does_not_consume_terminator_as_fixed_width_atom_data() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83],
        &[0x8d, 0x80, 0x8f, 0x81, 0x8b, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminator-bounded object payload");

    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 13,
                offset: 0,
            },
            PayloadField::Atom {
                value: 0,
                offset: 1,
            },
            PayloadField::Atom {
                value: 15,
                offset: 2,
            },
            PayloadField::Reference {
                value: 11,
                offset: 3,
            },
            PayloadField::Terminator,
        ]
    );
}

#[test]
fn object_graph_payload_does_not_consume_terminator_as_paged_atom_data() {
    use crate::object_graph::PayloadField;

    let bytes = object_graph_from_records(&[object_graph_record(
        &[0x04, 0x01, 0x81, 0x83],
        &[0x8d, 0xd2, 0xfe],
    )]);
    let graph = crate::object_graph::parse(&bytes).expect("terminator-bounded paged atom");

    assert_eq!(
        graph.records[0].payload.fields,
        [
            PayloadField::Atom {
                value: 13,
                offset: 0,
            },
            PayloadField::Atom {
                value: 0xd2,
                offset: 1,
            },
            PayloadField::Terminator,
        ]
    );
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
            if items == &vec![
                ListItem::Reference {
                    value: 5,
                    offset: 2,
                },
                ListItem::Atom {
                    value: 6,
                    offset: 4,
                },
                ListItem::Atom {
                    value: 10,
                    offset: 6,
                },
            ]
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
    assert_eq!(graph.records[0].owner_entity_id(), Some(2));
    assert_eq!(graph.records[0].class_ref, Some(3));
    assert_eq!(graph.records[0].storage_ref, Some(4));
    assert_eq!(graph.records[1].ordinal, 1);
    assert_eq!(graph.records[1].owner_entity_id(), Some(2));
    assert_eq!(graph.records[1].class_ref, Some(4));
    assert_eq!(native.design_objects.len(), 1);
    let object = &native.design_objects[0];
    assert_eq!(object.parent, graph.id);
    assert_eq!(object.owner_entity_id, 2);
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
        decoded.report.coverage["decoded_design_object_relation_count"],
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
fn unresolved_modeling_scope_accounts_for_every_retained_object_record() {
    let (mut bytes, _) = outer_container_object_graph_catpart();
    let class_offset = bytes
        .windows(b"CATPrtCont".len())
        .position(|window| window == b"CATPrtCont")
        .expect("part-container declaration");
    bytes[class_offset..class_offset + b"CATPrtCont".len()].copy_from_slice(b"CATFooCont");

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode object graph without a declared part container");

    assert_eq!(decoded.report.coverage["decoded_object_graph_count"], 1);
    assert_eq!(decoded.report.coverage["decoded_object_record_count"], 2);
    assert_eq!(decoded.report.coverage["modeling_object_graph_count"], 0);
    assert_eq!(decoded.report.coverage["modeling_object_record_count"], 0);
    assert_eq!(
        decoded.report.coverage["retained_unscoped_object_graph_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["retained_unscoped_object_record_count"],
        2
    );
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("1 retained object graph(s)")
            && loss.message.contains("2 field record(s)")
    }));
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
    assert_eq!(native.design_objects[0].owner_entity_id, 2);
    assert_eq!(native.design_objects[1].owner_entity_id, 3);
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
fn native_load_rejects_noncanonical_entity_frame_lengths() {
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let native =
        crate::native::CatiaNative::decode(&sequential_entity_backed_object_graph(&records));

    for mutate in [
        |record: &mut crate::native::CatiaEntityRecord| record.definition_len += 1,
        |record: &mut crate::native::CatiaEntityRecord| record.value_len += 1,
        |record: &mut crate::native::CatiaEntityRecord| record.byte_len += 1,
    ] as [fn(&mut crate::native::CatiaEntityRecord); 3]
    {
        let mut malformed = native.clone();
        mutate(&mut malformed.entity_records[0]);
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed entity frame");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    }
}

#[test]
fn native_namespace_retains_and_validates_definition_schema_selections() {
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes =
        entity_table_record_with_definition_and_value(1, &[0, 0, 0x32, 4, 0, 0, 0], &[]);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.entity_records[0].definition_schema_selections,
        [crate::native::CatiaDefinitionSchemaSelection {
            offset: 2,
            ordinal: 4,
            entry: Some(native.catalogs[0].entries[4].id.clone()),
            name: Some("Sketch".to_string()),
        }]
    );

    let mut malformed = native;
    malformed.entity_records[0].definition_schema_selections[0].name = Some("Pad".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed definition-schema view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_retains_and_validates_repeated_reference_suffixes() {
    let payload = [
        0xb0, 0x83, 0x81, 0xbc, 0x81, 0xbe, 0x81, 0xb1, 0x83, 0x81, 0xbc, 0x81, 0xbe, 0xd1, 0x80,
        0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &payload)];
    let native = crate::native::CatiaNative::decode(&entity_backed_object_graph(&records, &[1]));
    let suffix = native.object_graphs[0].records[0]
        .repeated_reference_suffix
        .as_ref()
        .expect("repeated reference suffix");
    assert_eq!(suffix.schema_preamble, None);
    assert_eq!(suffix.repeated_references, [60, 62]);
    assert_eq!(suffix.terminal_reference, 49);

    let mut malformed = native;
    malformed.object_graphs[0].records[0]
        .repeated_reference_suffix
        .as_mut()
        .expect("repeated reference suffix")
        .terminal_reference += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed repeated-reference-suffix view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_resolves_and_validates_repeated_reference_schema_selections() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_repeated_reference_schema_selection(),
    );
    let selection = native.object_graphs[0].records[0]
        .repeated_reference_schema_selection
        .as_ref()
        .expect("reference schema selection");
    assert_eq!(
        selection.order,
        crate::native::CatiaRepeatedReferenceSchemaOrder::BlobThenSchema
    );
    assert_eq!(selection.ordinal, 4);
    assert_eq!(selection.offset, 67);
    assert_eq!(selection.name.as_deref(), Some("TargetSchema"));
    assert!(selection.entry.is_some());

    let mut malformed = native;
    malformed.object_graphs[0].records[0]
        .repeated_reference_schema_selection
        .as_mut()
        .expect("reference schema selection")
        .name = Some("WrongSchema".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-schema view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_retains_and_validates_complete_entity_numeric_tuples() {
    let value = [
        0x91, 0x84, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6, 0, 0, 0, 0, 0, 0, 0x12, 0x40, 0xfe,
        0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    let tuple = native.entity_records[0]
        .numeric_tuple
        .as_ref()
        .expect("complete numeric tuple");
    assert!(tuple.items.iter().any(|item| {
        matches!(
            item,
            crate::entity_table::NumericTupleItem::Binary64 { bits, .. }
                if *bits == 4.5_f64.to_bits()
        )
    }));

    let mut malformed = native;
    malformed.entity_records[0]
        .numeric_tuple
        .as_mut()
        .expect("complete numeric tuple")
        .value_atom += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed numeric-tuple view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn decode_reports_complete_numeric_entity_value_tuples_separately_from_packets() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_numeric_entity_value_tuple()),
            &DecodeOptions::default(),
        )
        .expect("decode complete numeric entity-value tuple");

    assert_eq!(
        decoded.report.coverage["decoded_numeric_entity_value_tuple_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_numeric_entity_value_packet_count"],
        0
    );
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.message
            .contains("1 complete numeric entity-value tuple(s)")
            && loss
                .message
                .contains("0 embedded numeric entity-value packet(s)")
    }));
}

#[test]
fn native_namespace_retains_and_validates_complete_entity_reference_signatures() {
    let value = [
        0x32, 0xcf, 0, 0, 0, 0x82, 0xe8, 0xe0, 0x0a, 0x37, 0x8c, 0x81, b'(', b'E', b')', 0xfe,
        0x32, 0xd0, 0, 0, 0, 0x83, 0xe9, 0xe0, 0x17, 0x08, 0x37, 0xfe, 0xfe, 0xfe,
    ];
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    let signature = native.entity_records[0]
        .reference_signature
        .as_ref()
        .expect("complete reference signature");
    assert_eq!(signature.first_reference, 207);
    assert_eq!(signature.second_reference, 208);
    assert_eq!(signature.signature, "(E)");

    let mut malformed = native;
    malformed.entity_records[0]
        .reference_signature
        .as_mut()
        .expect("complete reference signature")
        .second_reference += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed reference-signature view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_tokenizes_and_validates_complete_entity_values() {
    let mut value = vec![0x32, 4, 0, 0, 0, 0x87, 0xe6];
    value.extend_from_slice(&12.5_f64.to_bits().to_le_bytes());
    value.extend_from_slice(&[0x87, 0xe8, 0xfe]);
    let records = [object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])];
    let mut bytes = entity_table_record_with_value(1, &value);
    bytes.push(0xde);
    bytes.extend(object_graph_from_records(&records));

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.entity_records[0].value_fields,
        [
            crate::value_block::ValueField::SchemaSelector {
                ordinal: 4,
                offset: 0,
            },
            crate::value_block::ValueField::Binary64 {
                bits: 12.5_f64.to_bits(),
                offset: 5,
            },
            crate::value_block::ValueField::Marker {
                code: 0xe8,
                offset: 15,
            },
            crate::value_block::ValueField::Terminator { offset: 17 },
        ]
    );

    let mut malformed = native;
    malformed.entity_records[0].value_fields.pop();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed entity-value view");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_resolves_and_validates_entity_value_schema_selections() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_entity_value_schema_selection());
    let selection = &native.entity_records[0].value_schema_selections[0];
    assert_eq!(selection.ordinal, 4);
    assert_eq!(selection.offset, 0);
    assert_eq!(selection.name, "TargetValue");
    assert!(!selection.entry.is_empty());
    assert_eq!(
        selection.encoded_value,
        [
            crate::value_block::ValueField::Binary64 {
                bits: 12.5_f64.to_bits(),
                offset: 5,
            },
            crate::value_block::ValueField::Opcode {
                code: 0xe8,
                offset: 15,
            },
            crate::value_block::ValueField::Atom {
                value: 3851,
                width: 2,
                offset: 16,
            },
            crate::value_block::ValueField::Separator { offset: 18 },
            crate::value_block::ValueField::Terminator { offset: 19 },
            crate::value_block::ValueField::Terminator { offset: 20 },
        ]
    );
    assert_eq!(
        selection.packets,
        [crate::entity_table::EntityValuePacket::Compact {
            offset: 15,
            value_selector: 0x0ae0,
        }]
    );

    let assert_rejected = |malformed: crate::native::CatiaNative| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        malformed
            .store(&mut namespace)
            .expect("store malformed entity-value schema view");
        assert!(matches!(
            crate::native::CatiaNative::load(&namespace),
            Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
        ));
    };

    let mut wrong_name = native.clone();
    wrong_name.entity_records[0].value_schema_selections[0].name = "WrongValue".to_string();
    assert_rejected(wrong_name);

    let mut wrong_packet = native;
    let crate::entity_table::EntityValuePacket::Compact { value_selector, .. } =
        &mut wrong_packet.entity_records[0].value_schema_selections[0].packets[0]
    else {
        panic!("compact value packet");
    };
    *value_selector += 1;
    assert_rejected(wrong_packet);
}

#[test]
fn native_namespace_types_and_validates_complete_relation_expressions() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression("param"));
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("complete relation expression");
    let crate::native::CatiaRelationExpressionFraming::PlaceholderState {
        placeholder,
        state_role,
    } = &expression.framing
    else {
        panic!("placeholder-state framing")
    };
    assert_eq!(placeholder.value, "#1_ ");
    assert_eq!(state_role.value, "opened");
    assert_eq!(expression.expression.value, "#1_ /2-2mm");
    assert_eq!(expression.parameter_role.value, "param");
    assert_eq!(
        expression.type_signature.value,
        "(#1_ : #In LENGTH) : LENGTH"
    );
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature.inputs,
        [crate::native::CatiaRelationTypeInput {
            parameter: "#1_".to_string(),
            input_type: "LENGTH".to_string(),
        }]
    );
    assert_eq!(signature.result_type, "LENGTH");
    assert_eq!(expression.function_role.value, "RelationExpFct");

    let mut malformed = native;
    malformed.entity_records[0]
        .relation_expression
        .as_mut()
        .expect("complete relation expression")
        .expression
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed relation expression");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_parser_version_relation_expression("Boolean", "ParserVersion"),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::BooleanParserVersion {
        prefix_role,
        parser_version_role,
    } = &expression.framing
    else {
        panic!("parser-version framing")
    };
    assert_eq!(prefix_role.value, "Boolean");
    assert_eq!(
        expression.expression.value,
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2"
    );
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(expression.parameter_role.value, "param");
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature
            .inputs
            .iter()
            .map(|input| (input.parameter.as_str(), input.input_type.as_str()))
            .collect::<Vec<_>>(),
        [("#1_", "LENGTH"), ("#2_", "LENGTH")]
    );
    assert_eq!(signature.result_type, "Real");
}

#[test]
fn opened_parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_opened_parser_version_relation_expression(
            "Boolean",
            "ParserVersion",
            "opened",
        ),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("opened parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::OpenedBooleanParserVersion {
        prefix_role,
        parser_version_role,
        state_role,
    } = &expression.framing
    else {
        panic!("opened parser-version framing")
    };
    assert_eq!(prefix_role.value, "Boolean");
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(state_role.value, "opened");
    assert_eq!(
        expression.expression.value,
        "log(min(100,max(20*#1_,#2_)/#2_))/log(100)/2"
    );
    assert_eq!(expression.parameter_role.value, "param");
    assert!(expression.signature.is_some());
}

#[test]
fn opened_parser_version_relation_expression_requires_every_exact_role() {
    for (prefix_role, parser_version_role, state_role) in [
        ("Real", "ParserVersion", "opened"),
        ("Boolean", "ParserRevision", "opened"),
        ("Boolean", "ParserVersion", "closed"),
    ] {
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_opened_parser_version_relation_expression(
                prefix_role,
                parser_version_role,
                state_role,
            ),
        );

        assert!(native.entity_records[0].relation_expression.is_none());
    }
}

#[test]
fn decode_retains_an_opened_parser_version_expression_without_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_opened_parser_version_relation_expression(
                    "Boolean",
                    "ParserVersion",
                    "opened",
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode opened parser-version expression");

    assert!(decoded.ir.model.parameters.is_empty());
    assert_eq!(
        decoded.report.coverage["decoded_relation_expression_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_opened_boolean_parser_version_relation_expression_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_typed_relation_expression_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_referenced_relation_expression_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unresolved_unreferenced_relation_expression_count"],
        1
    );
    assert_eq!(decoded.report.coverage["decoded_formula_relation_count"], 0);
    assert_eq!(decoded.report.coverage["transferred_parameter_count"], 0);
}

#[test]
fn unprefixed_parser_version_relation_expression_retains_its_distinct_framing() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_unprefixed_parser_version_relation_expression("ParserVersion"),
    );
    let expression = native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("unprefixed parser-version relation expression");

    let crate::native::CatiaRelationExpressionFraming::ParserVersion {
        parser_version_role,
    } = &expression.framing
    else {
        panic!("unprefixed parser-version framing")
    };
    assert_eq!(expression.expression.value, "360.0*1 deg/#1_");
    assert_eq!(parser_version_role.value, "ParserVersion");
    assert_eq!(expression.parameter_role.value, "param");
    let signature = expression.signature.as_ref().expect("typed signature");
    assert_eq!(
        signature.inputs,
        [crate::native::CatiaRelationTypeInput {
            parameter: "#1_".to_string(),
            input_type: "Integer".to_string(),
        }]
    );
    assert_eq!(signature.result_type, "ANGLE");
}

#[test]
fn unprefixed_parser_version_relation_expression_requires_the_exact_version_role() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_unprefixed_parser_version_relation_expression("ParserRevision"),
    );

    assert!(native.entity_records[0].relation_expression.is_none());
}

#[test]
fn decode_retains_an_unprefixed_parser_version_expression_without_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_unprefixed_parser_version_relation_expression(
                    "ParserVersion",
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode unprefixed parser-version expression");

    assert!(decoded.ir.model.parameters.is_empty());
    assert_eq!(
        decoded.report.coverage["decoded_relation_expression_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_parser_version_relation_expression_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_typed_relation_expression_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_referenced_relation_expression_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unresolved_unreferenced_relation_expression_count"],
        1
    );
    assert_eq!(decoded.report.coverage["decoded_formula_relation_count"], 0);
    assert_eq!(decoded.report.coverage["transferred_parameter_count"], 0);
}

#[test]
fn relation_program_instance_requires_the_complete_identity_frame() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(1, 1, 1, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("complete instance frame");
    assert_eq!(
        instance.framing,
        crate::native::CatiaRelationProgramInstanceFraming::Lead12
    );
    assert_eq!(instance.program_entity.entity_id, 1);
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(instance.program_entity.class_name.as_deref(), Some("body"));
    assert_eq!(instance.repeated_entity.entity_id, 1);
    assert_eq!(
        instance.repeated_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(instance.repeated_entity.class_name.as_deref(), Some("body"));
    assert_eq!(
        instance.relation_expression.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    let context = instance
        .lead12_context_entity
        .as_ref()
        .expect("lead-12 context entity");
    assert_eq!(context.entity_id, 1);
    assert_eq!(
        context.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(context.class_name.as_deref(), Some("body"));

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(2, 1, 3, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("resolved non-expression program");
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert!(instance.relation_expression.is_none());
    let context = instance
        .lead12_context_entity
        .as_ref()
        .expect("lead-12 context entity");
    assert_eq!(context.entity_id, 3);
    assert!(context.entity.is_none());

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(3, 3, 1, 2),
    );
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("unresolved program identity");
    assert!(instance.program_entity.entity.is_none());
    assert_eq!(instance.repeated_entity.entity_id, 3);
    assert!(instance.repeated_entity.entity.is_none());
    assert!(instance.relation_expression.is_none());

    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_program_instance(1, 1, 1, 3),
    );
    assert!(native
        .entity_records
        .iter()
        .all(|entity| entity.relation_program_instance.is_none()));
}

#[test]
fn lead54_relation_program_instance_requires_its_complete_identity_frame() {
    let file = standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 2);
    let native = crate::native::CatiaNative::decode(&file);
    let instance = native.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("complete lead-54 instance frame");
    assert_eq!(
        instance.framing,
        crate::native::CatiaRelationProgramInstanceFraming::Lead54
    );
    assert!(instance.lead12_context_entity.is_none());
    let trailing = instance
        .lead54_trailing_entity
        .as_ref()
        .expect("lead-54 trailing entity");
    assert_eq!(trailing.entity_id, 1);
    assert_eq!(
        trailing.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(trailing.class_name.as_deref(), Some("body"));
    assert_eq!(
        instance.program_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance.repeated_entity.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        instance.relation_expression.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode lead-54 relation-program instance");
    assert_eq!(
        decoded.report.coverage["decoded_relation_program_instance_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_lead12_relation_program_instance_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_lead54_relation_program_instance_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_resolved_lead12_relation_program_context_entity_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unresolved_lead12_relation_program_context_entity_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_lead12_relation_program_paramout_context_entity_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_other_lead12_relation_program_context_class_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unclassified_lead12_relation_program_context_entity_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_resolved_lead54_relation_program_trailing_entity_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_lead54_relation_program_trailing_entity_count"],
        0
    );

    let unresolved = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance(1, 3, 3, 2),
    );
    let instance = unresolved.entity_records[1]
        .relation_program_instance
        .as_ref()
        .expect("unresolved repeated identity");
    assert_eq!(instance.repeated_entity.entity_id, 3);
    assert!(instance.repeated_entity.entity.is_none());
    let trailing = instance
        .lead54_trailing_entity
        .as_ref()
        .expect("lead-54 trailing entity");
    assert_eq!(trailing.entity_id, 3);
    assert!(trailing.entity.is_none());

    let malformed = crate::native::CatiaNative::decode(
        &standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 3),
    );
    assert!(malformed
        .entity_records
        .iter()
        .all(|entity| entity.relation_program_instance.is_none()));
}

#[test]
fn decode_reports_exact_relation_program_instances() {
    for (
        program_entity_id,
        repeated_reference_entity_id,
        resolved,
        expression,
        other,
        unresolved,
        resolved_repeated,
    ) in [
        (1, 1, 1, 1, 0, 0, 1),
        (2, 1, 1, 0, 1, 0, 1),
        (3, 1, 0, 0, 0, 1, 1),
        (1, 3, 1, 1, 0, 0, 0),
    ] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_relation_program_instance(
                    program_entity_id,
                    repeated_reference_entity_id,
                    1,
                    2,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode relation-program instance");
        assert_eq!(
            decoded.report.coverage["decoded_relation_program_instance_count"],
            1
        );
        assert_eq!(
            decoded.report.coverage["decoded_lead12_relation_program_instance_count"],
            1
        );
        assert_eq!(
            decoded.report.coverage["decoded_lead54_relation_program_instance_count"],
            0
        );
        assert_eq!(
            decoded.report.coverage
                ["decoded_resolved_lead12_relation_program_context_entity_count"],
            1
        );
        assert_eq!(
            decoded.report.coverage["unresolved_lead12_relation_program_context_entity_count"],
            0
        );
        assert_eq!(
            decoded.report.coverage
                ["decoded_lead12_relation_program_paramout_context_entity_count"],
            0
        );
        assert_eq!(
            decoded.report.coverage["decoded_other_lead12_relation_program_context_class_count"],
            1
        );
        assert_eq!(
            decoded.report.coverage["unclassified_lead12_relation_program_context_entity_count"],
            0
        );
        assert_eq!(
            decoded.report.coverage["decoded_resolved_relation_program_instance_count"],
            resolved
        );
        assert_eq!(
            decoded.report.coverage["decoded_relation_expression_program_instance_count"],
            expression
        );
        assert_eq!(
            decoded.report.coverage["decoded_other_relation_program_instance_count"],
            other
        );
        assert_eq!(
            decoded.report.coverage["unresolved_relation_program_instance_count"],
            unresolved
        );
        assert_eq!(
            decoded.report.coverage["decoded_resolved_relation_program_repeated_reference_count"],
            resolved_repeated
        );
        assert_eq!(
            decoded.report.coverage["unresolved_relation_program_repeated_reference_count"],
            1 - resolved_repeated
        );
        let classified_program = usize::from(program_entity_id <= 2);
        assert_eq!(
            decoded.report.coverage["decoded_classified_relation_program_entity_count"],
            classified_program
        );
        assert_eq!(
            decoded.report.coverage["unclassified_relation_program_entity_count"],
            1 - classified_program
        );
        let classified_repeated = usize::from(repeated_reference_entity_id == 1);
        assert_eq!(
            decoded.report.coverage["decoded_classified_relation_program_repeated_entity_count"],
            classified_repeated
        );
        assert_eq!(
            decoded.report.coverage["unclassified_relation_program_repeated_entity_count"],
            1 - classified_repeated
        );
        assert_eq!(
            decoded.report.coverage["decoded_instanced_relation_expression_count"],
            expression
        );
        assert_eq!(decoded.report.coverage["decoded_formula_relation_count"], 0);
        assert!(decoded.ir.model.parameters.is_empty());
    }
}

#[test]
fn native_load_derives_relation_program_instances_from_older_namespaces() {
    for native in [
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_program_instance(
            1, 1, 1, 2,
        )),
        crate::native::CatiaNative::decode(
            &standard_catpart_with_lead54_relation_program_instance(1, 1, 1, 2),
        ),
    ] {
        let expected = native.entity_records[1]
            .relation_program_instance
            .clone()
            .expect("decoded relation-program instance");
        let mut stored = cadmpeg_ir::NativeNamespace::default();
        native
            .store(&mut stored)
            .expect("store older relation-program namespace");
        for (version, remove_context, remove_trailing, remove_framing) in [
            (
                crate::native::CATIA_RELATION_TYPED_REFERENCE_VERSION - 1,
                false,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_CONTEXT_VERSION - 1,
                true,
                false,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_CONTEXT_VERSION - 2,
                true,
                true,
                false,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_INSTANCE_VERSION,
                true,
                true,
                true,
            ),
            (
                crate::native::CATIA_RELATION_PROGRAM_INSTANCE_VERSION - 1,
                true,
                true,
                true,
            ),
        ] {
            let mut namespace = stored.clone();
            namespace.version = version;
            let stored_instance = namespace
                .arenas
                .get_mut("entity_records")
                .expect("stored entity records")[1]
                .fields
                .get_mut("relation_program_instance")
                .expect("stored relation-program field")
                .as_object_mut()
                .expect("stored relation-program instance");
            if remove_context {
                stored_instance.remove("lead12_context_entity");
            }
            if remove_trailing {
                stored_instance.remove("lead54_trailing_entity");
            }
            if remove_framing {
                stored_instance.remove("framing");
            }
            stored_instance.remove("program_entity");
            stored_instance.remove("repeated_entity");
            for field in ["lead12_context_entity", "lead54_trailing_entity"] {
                if let Some(reference) = stored_instance
                    .get_mut(field)
                    .and_then(|value| value.as_object_mut())
                {
                    reference.remove("class_name");
                }
            }

            let migrated = crate::native::CatiaNative::load(&namespace)
                .expect("migrate relation-program instance");
            assert_eq!(
                migrated.entity_records[1]
                    .relation_program_instance
                    .as_ref(),
                Some(&expected)
            );
        }
    }
}

#[test]
fn configuration_productions_retain_exact_same_graph_incidence() {
    let file = standard_catpart_with_configuration_incidences(8, 5, 7);
    let native = crate::native::CatiaNative::decode(&file);
    let configuration = native.entity_records[0]
        .configuration_record
        .as_ref()
        .expect("complete Configuration production");
    assert_eq!(configuration.schema_ordinal, 8);
    assert_eq!(configuration.schema_name, "Boolean");
    assert_eq!(configuration.entity_reference.entity_id, 5);
    assert_eq!(
        configuration.entity_reference.entity.as_deref(),
        Some(native.entity_records[0].id.as_str())
    );
    assert_eq!(
        configuration.entity_reference.class_name.as_deref(),
        Some("Configuration")
    );
    let row = native.entity_records[1]
        .configuration_row_link
        .as_ref()
        .expect("complete configrow production");
    assert_eq!(row.class_reference.entity_id, 6);
    assert_eq!(
        row.class_reference.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(row.class_reference.class_name.as_deref(), Some("configrow"));
    assert_eq!(row.successor.entity_id, 7);
    assert_eq!(
        row.successor.entity.as_deref(),
        Some(native.entity_records[2].id.as_str())
    );
    assert_eq!(row.successor.class_name.as_deref(), Some("body"));
    assert_eq!(native.configuration_row_chains.len(), 1);
    let chain = &native.configuration_row_chains[0];
    assert_eq!(chain.object_graph, native.entity_records[1].object_graph);
    assert_eq!(chain.class_reference, row.class_reference);
    assert_eq!(
        chain
            .rows
            .iter()
            .map(|reference| reference.entity_id)
            .collect::<Vec<_>>(),
        [6]
    );
    assert_eq!(
        chain.rows[0].entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(chain.terminal, row.successor);
    assert!(native.entity_records[2].configuration_record.is_none());
    assert!(native.entity_records[2].configuration_row_link.is_none());

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode configuration incidences");
    assert_eq!(
        decoded.report.coverage["decoded_configuration_record_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_configuration_schema_reference_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_resolved_configuration_entity_reference_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_configuration_entity_reference_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_classified_configuration_entity_reference_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unclassified_configuration_entity_reference_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_configuration_row_link_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_resolved_configuration_row_class_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_resolved_configuration_row_successor_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_complete_configuration_row_chain_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_ordered_configuration_row_link_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_resolved_configuration_row_chain_terminal_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_classified_configuration_row_chain_terminal_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_configuration_row_order_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["transferred_configuration_count"],
        0
    );
    assert!(decoded.ir.model.configurations.is_empty());
}

#[test]
fn configuration_row_chain_retains_complete_source_order() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_configuration_row_chain());
    assert_eq!(native.configuration_row_chains.len(), 1);
    let chain = &native.configuration_row_chains[0];
    assert_eq!(chain.class_reference.entity_id, 5);
    assert_eq!(
        chain
            .rows
            .iter()
            .map(|reference| reference.entity_id)
            .collect::<Vec<_>>(),
        [5, 6, 7]
    );
    assert!(chain
        .rows
        .iter()
        .all(|reference| reference.class_name.as_deref() == Some("configrow")));
    assert_eq!(chain.terminal.entity_id, 8);
    assert_eq!(chain.terminal.class_name.as_deref(), Some("body"));
}

#[test]
fn configuration_productions_preserve_unresolved_identities() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 15, 16),
    );
    let configuration = native.entity_records[0]
        .configuration_record
        .as_ref()
        .expect("complete Configuration production");
    assert_eq!(configuration.schema_name, "Boolean");
    assert!(configuration.entity_reference.entity.is_none());
    let row = native.entity_records[1]
        .configuration_row_link
        .as_ref()
        .expect("complete configrow production");
    assert!(row.successor.entity.is_none());

    let mismatched_schema = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(14, 15, 16),
    );
    assert!(mismatched_schema.entity_records[0]
        .configuration_record
        .is_none());

    let mut malformed = standard_catpart_with_configuration_incidences(8, 15, 16);
    let marker = [0x80, 250, 0, 0, 0];
    let offset = malformed
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("configrow marker");
    malformed[offset + 1] = 249;
    let malformed = crate::native::CatiaNative::decode(&malformed);
    assert!(malformed
        .entity_records
        .iter()
        .all(|entity| entity.configuration_row_link.is_none()));

    let cyclic_file = standard_catpart_with_configuration_incidences(8, 15, 6);
    let cyclic_native = crate::native::CatiaNative::decode(&cyclic_file);
    assert!(cyclic_native.configuration_row_chains.is_empty());
    let cyclic = CatiaCodec
        .decode(&mut Cursor::new(cyclic_file), &DecodeOptions::default())
        .expect("decode cyclic configuration row");
    assert_eq!(
        cyclic.report.coverage["decoded_complete_configuration_row_chain_count"],
        0
    );
    assert_eq!(
        cyclic.report.coverage["decoded_ordered_configuration_row_link_count"],
        0
    );
    assert_eq!(
        cyclic.report.coverage["unresolved_configuration_row_order_count"],
        1
    );
}

#[test]
fn native_load_migrates_and_validates_configuration_incidences() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_configuration_incidences(8, 5, 7),
    );
    let mut older = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut older)
        .expect("store configuration namespace");
    older.version = crate::native::CATIA_CONFIGURATION_SCHEMA_REFERENCE_VERSION - 1;
    for entity in older
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")
    {
        entity.fields.remove("configuration_record");
        entity.fields.remove("configuration_row_link");
    }
    let migrated =
        crate::native::CatiaNative::load(&older).expect("migrate configuration incidences");
    assert_eq!(
        migrated.entity_records[0].configuration_record,
        native.entity_records[0].configuration_record
    );
    assert_eq!(
        migrated.entity_records[1].configuration_row_link,
        native.entity_records[1].configuration_row_link
    );
    assert_eq!(
        migrated.configuration_row_chains,
        native.configuration_row_chains
    );

    let mut older = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut older)
        .expect("store pre-chain configuration namespace");
    older.version = crate::native::CATIA_CONFIGURATION_ROW_CHAIN_VERSION - 1;
    older.arenas.remove("configuration_row_chains");
    let migrated =
        crate::native::CatiaNative::load(&older).expect("migrate configuration-row chains");
    assert_eq!(
        migrated.configuration_row_chains,
        native.configuration_row_chains
    );

    let mut malformed_chain = native.clone();
    malformed_chain.configuration_row_chains[0]
        .terminal
        .entity_id = 6;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed_chain
        .store(&mut current)
        .expect("store malformed configuration chain");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[1]
        .configuration_row_link
        .as_mut()
        .expect("decoded configrow link")
        .successor
        .entity_id = 6;
    let mut current = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut current)
        .expect("store malformed current namespace");
    assert!(matches!(
        crate::native::CatiaNative::load(&current),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn parser_version_relation_expression_requires_both_exact_framing_roles() {
    for (prefix_role, parser_version_role) in
        [("Real", "ParserVersion"), ("Boolean", "ParserRevision")]
    {
        let native = crate::native::CatiaNative::decode(
            &standard_catpart_with_parser_version_relation_expression(
                prefix_role,
                parser_version_role,
            ),
        );

        assert!(native.entity_records[0].relation_expression.is_none());
    }
}

#[test]
fn decode_retains_a_parser_version_expression_without_fabricating_formula_incidence() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parser_version_relation_expression(
                "Boolean",
                "ParserVersion",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode parser-version expression");

    assert!(decoded.ir.model.parameters.is_empty());
    assert_eq!(
        decoded.report.coverage["decoded_relation_expression_count"],
        1
    );
    assert_eq!(decoded.report.coverage["decoded_formula_relation_count"], 0);
    assert_eq!(decoded.report.coverage["transferred_parameter_count"], 0);
}

#[test]
fn relation_expression_signature_preserves_ordered_typed_inputs() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression_signature(
            "param",
            "#1_ ",
            "(#1_ :  #In LENGTH,#2_ :  #In ANGLE) : Real",
        ));
    let signature = native.entity_records[0]
        .relation_expression
        .as_ref()
        .and_then(|expression| expression.signature.as_ref())
        .expect("multi-input signature");

    assert_eq!(
        signature.inputs,
        [
            crate::native::CatiaRelationTypeInput {
                parameter: "#1_".to_string(),
                input_type: "LENGTH".to_string(),
            },
            crate::native::CatiaRelationTypeInput {
                parameter: "#2_".to_string(),
                input_type: "ANGLE".to_string(),
            },
        ]
    );
    assert_eq!(signature.result_type, "Real");
}

#[test]
fn relation_expression_signature_accepts_an_empty_input_list_with_an_empty_placeholder() {
    let native = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_expression_signature("param", "", "() : LENGTH"),
    );
    let signature = native.entity_records[0]
        .relation_expression
        .as_ref()
        .and_then(|expression| expression.signature.as_ref())
        .expect("zero-input signature");

    assert!(signature.inputs.is_empty());
    assert_eq!(signature.result_type, "LENGTH");

    let nonempty_placeholder = crate::native::CatiaNative::decode(
        &standard_catpart_with_relation_expression_signature("param", "#1_ ", "() : LENGTH"),
    );
    assert!(nonempty_placeholder.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("relation expression")
        .signature
        .is_none());
}

#[test]
fn relation_expression_signature_rejects_duplicate_inputs() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression_signature(
            "param",
            "#1_ ",
            "(#1_ : #In LENGTH,#1_ : #In ANGLE) : Real",
        ));

    assert!(native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("relation expression")
        .signature
        .is_none());
}

#[test]
fn relation_expression_requires_every_exact_role() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_relation_expression("parameter"));

    assert!(native.entity_records[0].relation_expression.is_none());
}

#[test]
fn relation_expression_signature_requires_the_selected_placeholder() {
    let mut file = standard_catpart_with_relation_expression("param");
    let signature = file
        .windows("(#1_ : #In LENGTH) : LENGTH".len())
        .position(|bytes| bytes == b"(#1_ : #In LENGTH) : LENGTH")
        .expect("relation type signature");
    file[signature + 2] = b'2';

    let native = crate::native::CatiaNative::decode(&file);
    assert!(native.entity_records[0]
        .relation_expression
        .as_ref()
        .expect("complete relation expression")
        .signature
        .is_none());
}

#[test]
fn native_namespace_types_and_validates_named_parameter_values() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer, CatiaEntitySuffixValue,
    };

    let scalar = 35.0_f64.to_bits();
    let mut scalar_suffix = vec![0x85, 0x96, 0x82, 0x6a, 0xe6];
    scalar_suffix.extend_from_slice(&scalar.to_le_bytes());
    scalar_suffix.extend_from_slice(&[0x81, 0x52]);
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&scalar_suffix));
    let parameter = native.entity_records[0]
        .parameter_value
        .as_ref()
        .expect("complete named parameter value");
    assert_eq!(parameter.name.value, "Thickness");
    assert_eq!(parameter.binding.value, "#1_ /2");
    assert_eq!(
        parameter.evaluation,
        CatiaEntityEvaluation::Scalar { bits: scalar }
    );
    assert_eq!(
        native.entity_records[0].suffix_value,
        Some(CatiaEntitySuffixValue {
            prefix_atoms: [5, 22, 2],
            prefix_atom_widths: [1, 1, 1],
            prefix_code: 0x6a,
            payload: CatiaEntitySuffixPayload::Evaluation {
                evaluation: CatiaEntityEvaluation::Scalar { bits: scalar },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            trailer: CatiaEntitySuffixTrailer::Token8152,
        })
    );

    let unset = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x85, 0x96, 0x82, 0x6a, 0xe7, 0x81, 0x52,
    ]));
    assert_eq!(
        unset.entity_records[0]
            .parameter_value
            .as_ref()
            .expect("complete unset parameter")
            .evaluation,
        CatiaEntityEvaluation::Unset
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .parameter_value
        .as_mut()
        .expect("complete named parameter value")
        .name
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed parameter value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_types_dimension_constraint_ranges() {
    use crate::native::{CatiaConstraintRangeFraming, CatiaEntityEvaluation};

    let scalar = 128.0_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xc1, 0xe6];
    suffix.extend_from_slice(&scalar.to_le_bytes());
    let file = standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix);
    let native = crate::native::CatiaNative::decode(&file);
    let range = native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete dimension constraint range");
    assert!(!range.range.entry.is_empty());
    assert_eq!(range.range.value, "Range");
    assert!(!range.constraint.entry.is_empty());
    assert_eq!(range.constraint.value, "CstAttr_Dimension");
    assert_eq!(range.framing, CatiaConstraintRangeFraming::DimensionC1);
    assert_eq!(
        range.evaluation,
        CatiaEntityEvaluation::Scalar { bits: scalar }
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode constraint range");
    assert_eq!(decoded.report.coverage["decoded_constraint_range_count"], 1);
    assert_eq!(
        decoded.report.coverage["decoded_dimension_constraint_range_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_complex_constraint_range_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_evaluated_constraint_range_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_unset_constraint_range_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_constraint_range_incoming_reference_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unreferenced_constraint_range_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["uniquely_referenced_constraint_range_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["multiply_referenced_constraint_range_count"],
        0
    );
    assert!(!decoded
        .report
        .coverage
        .contains_key("decoded_structurally_owned_constraint_range_count"));
    assert!(!decoded
        .report
        .coverage
        .contains_key("unresolved_constraint_range_owner_count"));

    let referenced_file = |reference_count: usize| {
        let value = [0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0, 0xfe];
        let mut range_entity = entity_table_record_with_definition_and_value(1, &[0x01], &value);
        range_entity[6] = 2;
        range_entity.extend_from_slice(&suffix);
        let range_len = u32::try_from(range_entity.len()).expect("bounded range entity");
        range_entity[2..6].copy_from_slice(&range_len.to_le_bytes());
        let mut stream = range_entity;
        stream.extend(entity_table_record_with_definition_and_value(
            2,
            &[0x01],
            &[0xfe],
        ));
        let mut reference_payload = [0x81, 0x81].repeat(reference_count);
        reference_payload.push(0xfe);
        stream.push(0xde);
        stream.extend(object_graph_from_records(&[
            object_graph_record(&[0x04, 0x01, 0x81, 0x84], &[0xfe]),
            object_graph_record(&[0x04, 0x01, 0x82, 0x84], &reference_payload),
        ]));
        stream.extend(catalog_stream(&[
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            "Range",
            "CstAttr_Dimension",
        ]));
        let mut file = standard_catpart();
        file.splice(16..16, stream);
        let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
        file[8..12].copy_from_slice(&be32(file_len));
        file
    };
    let unique_file = referenced_file(1);
    let unique_native = crate::native::CatiaNative::decode(&unique_file);
    let incoming = &unique_native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete referenced constraint range")
        .incoming_references;
    assert_eq!(incoming.len(), 1);
    assert_eq!(
        incoming[0].object_record,
        unique_native.object_graphs[0].records[1].id
    );
    let source_entity = incoming[0]
        .source_entity
        .as_ref()
        .expect("source record has a paired entity");
    assert_eq!(source_entity.entity_id, 2);
    assert_eq!(
        source_entity.entity.as_deref(),
        Some(unique_native.entity_records[1].id.as_str())
    );
    assert_eq!(
        source_entity.class_name,
        unique_native.object_graphs[0].records[1].class_name
    );
    assert_eq!(
        incoming[0].payload_offset,
        unique_native.object_graphs[0].records[1].references[0].payload_offset
    );
    assert_eq!(
        incoming[0].source,
        unique_native.object_graphs[0].records[1].references[0].source
    );

    let uniquely_referenced = CatiaCodec
        .decode(&mut Cursor::new(unique_file), &DecodeOptions::default())
        .expect("decode uniquely referenced constraint range");
    assert_eq!(
        uniquely_referenced.report.coverage["decoded_constraint_range_incoming_reference_count"],
        1
    );
    assert_eq!(
        uniquely_referenced.report.coverage
            ["decoded_classified_constraint_range_source_entity_count"],
        usize::from(source_entity.class_name.is_some())
    );
    assert_eq!(
        uniquely_referenced.report.coverage["unclassified_constraint_range_source_entity_count"],
        usize::from(source_entity.class_name.is_none())
    );
    assert_eq!(
        uniquely_referenced.report.coverage["unreferenced_constraint_range_count"],
        0
    );
    assert_eq!(
        uniquely_referenced.report.coverage["uniquely_referenced_constraint_range_count"],
        1
    );
    assert_eq!(
        uniquely_referenced.report.coverage["multiply_referenced_constraint_range_count"],
        0
    );

    let multiple_file = referenced_file(2);
    let multiple_native = crate::native::CatiaNative::decode(&multiple_file);
    let incoming = &multiple_native.entity_records[0]
        .constraint_range
        .as_ref()
        .expect("complete multiply referenced constraint range")
        .incoming_references;
    assert_eq!(incoming.len(), 2);
    assert_eq!(
        incoming
            .iter()
            .map(|reference| reference.payload_offset)
            .collect::<Vec<_>>(),
        multiple_native.object_graphs[0].records[1]
            .references
            .iter()
            .map(|reference| reference.payload_offset)
            .collect::<Vec<_>>()
    );

    let multiply_referenced = CatiaCodec
        .decode(&mut Cursor::new(multiple_file), &DecodeOptions::default())
        .expect("decode multiply referenced constraint range");
    assert_eq!(
        multiply_referenced.report.coverage["decoded_constraint_range_incoming_reference_count"],
        2
    );
    assert_eq!(
        multiply_referenced.report.coverage["unreferenced_constraint_range_count"],
        0
    );
    assert_eq!(
        multiply_referenced.report.coverage["uniquely_referenced_constraint_range_count"],
        0
    );
    assert_eq!(
        multiply_referenced.report.coverage["multiply_referenced_constraint_range_count"],
        1
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete dimension constraint range")
        .framing = CatiaConstraintRangeFraming::DimensionB8;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint range");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = crate::native::CatiaNative::decode(
        &standard_catpart_with_two_selector_value("Range", "CstAttr_Dimension", &suffix),
    );
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete dimension constraint range")
        .constraint
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint role");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = unique_native.clone();
    malformed.entity_records[0]
        .constraint_range
        .as_mut()
        .expect("complete referenced constraint range")
        .incoming_references[0]
        .payload_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed constraint-range incidence");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    unique_native
        .store(&mut stored)
        .expect("store older constraint-range namespace");
    stored.version = crate::native::CATIA_CONSTRAINT_RANGE_INCIDENCE_VERSION - 1;
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields
        .get_mut("constraint_range")
        .expect("stored constraint range")
        .as_object_mut()
        .expect("stored constraint-range object")
        .remove("incoming_references");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate constraint-range incidence");
    assert_eq!(
        migrated.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("migrated constraint range")
            .incoming_references
            .len(),
        1
    );

    let mut stored = cadmpeg_ir::NativeNamespace::default();
    unique_native
        .store(&mut stored)
        .expect("store older constraint-range source namespace");
    stored.version = crate::native::CATIA_CONSTRAINT_RANGE_SOURCE_ENTITY_VERSION - 1;
    stored
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields
        .get_mut("constraint_range")
        .expect("stored constraint range")
        .as_object_mut()
        .expect("stored constraint-range object")
        .get_mut("incoming_references")
        .expect("stored incoming references")
        .as_array_mut()
        .expect("stored incoming-reference array")[0]
        .as_object_mut()
        .expect("stored incoming-reference object")
        .remove("source_entity");
    let migrated =
        crate::native::CatiaNative::load(&stored).expect("migrate constraint-range source entity");
    assert_eq!(
        migrated.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("migrated constraint range")
            .incoming_references[0]
            .source_entity,
        unique_native.entity_records[0]
            .constraint_range
            .as_ref()
            .expect("source constraint range")
            .incoming_references[0]
            .source_entity
    );
}

#[test]
fn constraint_range_requires_an_exact_role_and_framing_pair() {
    use crate::native::CatiaConstraintRangeFraming;

    for (constraint, code, expected) in [
        (
            "CstAttr_Dimension",
            0xb8,
            CatiaConstraintRangeFraming::DimensionB8,
        ),
        ("ComplexCst", 0xc9, CatiaConstraintRangeFraming::ComplexC9),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            "Range",
            constraint,
            &[0x84, 0x96, 0x82, code, 0xe7],
        ));
        assert_eq!(
            native.entity_records[0]
                .constraint_range
                .as_ref()
                .expect("complete constraint range")
                .framing,
            expected
        );
    }

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_two_selector_value(
                "Range",
                "ComplexCst",
                &[0x84, 0x96, 0x82, 0xc9, 0xe7],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode unset complex constraint range");
    assert_eq!(decoded.report.coverage["decoded_constraint_range_count"], 1);
    assert_eq!(
        decoded.report.coverage["decoded_dimension_constraint_range_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_complex_constraint_range_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_evaluated_constraint_range_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_unset_constraint_range_count"],
        1
    );

    for (range, constraint, code) in [
        ("Tolerance", "CstAttr_Dimension", 0xc1),
        ("Range", "ComplexCst", 0xc1),
        ("Range", "CstAttr_Dimension", 0xc9),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
            range,
            constraint,
            &[0x84, 0x96, 0x82, code, 0xe7],
        ));
        assert!(native.entity_records[0].constraint_range.is_none());
    }
}

#[test]
fn native_namespace_types_and_validates_generic_entity_suffix_values() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer, CatiaEntitySuffixValue,
    };

    let bits = 0.1_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xad, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x49]);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode generic entity suffix");
    assert_eq!(
        decoded.report.coverage["decoded_scalar_entity_suffix_value_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_unset_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_control_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_control_e8_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_control_e9_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_separator_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_atom_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_schema_selected_atom_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_schema_selected_evaluation_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_schema_selected_control_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_schema_selected_separator_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_schema_selected_schema_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_schema_selected_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_wide_prefix_entity_suffix_value_count"],
        0
    );
    let native =
        crate::native::CatiaNative::load(decoded.ir.native.namespace("catia").expect("namespace"))
            .expect("load generic entity suffix");

    assert_eq!(native.entity_records[0].parameter_value, None);
    assert_eq!(
        native.entity_records[0].suffix_value,
        Some(CatiaEntitySuffixValue {
            prefix_atoms: [4, 22, 2],
            prefix_atom_widths: [1, 1, 1],
            prefix_code: 0xad,
            payload: CatiaEntitySuffixPayload::Evaluation {
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            trailer: CatiaEntitySuffixTrailer::Token8149,
        })
    );

    let wide_scalar_bits = 0.001_f64.to_bits();
    let mut wide_scalar_suffix = vec![0xd1, 0x53, 0x96, 0x82, 0xa6, 0xe6];
    wide_scalar_suffix.extend_from_slice(&wide_scalar_bits.to_le_bytes());
    let wide_scalar = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&wide_scalar_suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode wide-prefix scalar suffix");
    assert_eq!(
        wide_scalar.report.coverage["decoded_wide_prefix_entity_suffix_value_count"],
        1
    );
    let wide_scalar = crate::native::CatiaNative::load(
        wide_scalar.ir.native.namespace("catia").expect("namespace"),
    )
    .expect("load wide-prefix scalar suffix");
    assert_eq!(
        wide_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix scalar")
            .prefix_atoms,
        [84, 22, 2]
    );
    assert_eq!(
        wide_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix scalar")
            .prefix_atom_widths,
        [2, 1, 1]
    );
    let mut malformed_wide_scalar = wide_scalar;
    malformed_wide_scalar.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete wide-prefix scalar")
        .prefix_atom_widths[0] = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_wide_scalar
        .store(&mut namespace)
        .expect("store malformed wide-prefix scalar");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let wide_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49,
        ]));
    assert!(matches!(
        wide_control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix control"),
        CatiaEntitySuffixValue {
            prefix_atoms: [104, 8, 1],
            prefix_atom_widths: [2, 1, 1],
            payload: CatiaEntitySuffixPayload::ControlE8,
            ..
        }
    ));

    let truncated_wide_prefix =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0xd1, 0x53, 0xd1,
        ]));
    assert_eq!(truncated_wide_prefix.entity_records[0].suffix_value, None);

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic unset entity suffix");
    assert_eq!(
        unset.report.coverage["decoded_scalar_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        unset.report.coverage["decoded_unset_entity_suffix_value_count"],
        1
    );

    let incomplete = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49, 0x00,
    ]));
    assert_eq!(incomplete.entity_records[0].suffix_value, None);

    let unknown_trailer =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x50,
        ]));
    assert_eq!(unknown_trailer.entity_records[0].suffix_value, None);

    let invalid_prefix =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x7f, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49,
        ]));
    assert_eq!(invalid_prefix.entity_records[0].suffix_value, None);

    let control = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x96, 0x81, 0xa6, 0xe8,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic control entity suffix");
    assert_eq!(
        control.report.coverage["decoded_control_entity_suffix_value_count"],
        1
    );
    assert_eq!(
        control.report.coverage["decoded_control_e8_entity_suffix_value_count"],
        1
    );
    assert_eq!(
        control.report.coverage["decoded_control_e9_entity_suffix_value_count"],
        0
    );
    let control =
        crate::native::CatiaNative::load(control.ir.native.namespace("catia").expect("namespace"))
            .expect("load generic control suffix");
    assert!(matches!(
        control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete control suffix")
            .payload,
        CatiaEntitySuffixPayload::ControlE8
    ));

    let control_e9 = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x88, 0x82, 0xf0, 0xe9, 0x81, 0x4a,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode E9 control entity suffix");
    assert_eq!(
        control_e9.report.coverage["decoded_control_entity_suffix_value_count"],
        1
    );
    assert_eq!(
        control_e9.report.coverage["decoded_control_e8_entity_suffix_value_count"],
        0
    );
    assert_eq!(
        control_e9.report.coverage["decoded_control_e9_entity_suffix_value_count"],
        1
    );
    let control_e9 = crate::native::CatiaNative::load(
        control_e9.ir.native.namespace("catia").expect("namespace"),
    )
    .expect("load E9 control suffix");
    assert!(matches!(
        control_e9.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete E9 control suffix")
            .payload,
        CatiaEntitySuffixPayload::ControlE9
    ));
    let mut malformed_control_e9 = control_e9.clone();
    malformed_control_e9.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete E9 control suffix")
        .payload = CatiaEntitySuffixPayload::ControlE8;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_control_e9
        .store(&mut namespace)
        .expect("store malformed E9 control suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
    let malformed_control_e9 =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x82, 0xf0, 0xe9, 0x81, 0x4a, 0x00,
        ]));
    assert_eq!(malformed_control_e9.entity_records[0].suffix_value, None);

    let malformed_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x96, 0x81, 0xa6, 0xe8, 0x81,
        ]));
    assert_eq!(malformed_control.entity_records[0].suffix_value, None);

    let separator = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x93, 0x81, 0xa1, 0x37, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic separator entity suffix");
    assert_eq!(
        separator.report.coverage["decoded_separator_entity_suffix_value_count"],
        1
    );
    let separator = crate::native::CatiaNative::load(
        separator.ir.native.namespace("catia").expect("namespace"),
    )
    .expect("load generic separator suffix");
    assert!(matches!(
        separator.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete separator suffix")
            .payload,
        CatiaEntitySuffixPayload::Separator37
    ));

    let malformed_separator =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x93, 0x81, 0xa1, 0x37, 0x81, 0x49, 0,
        ]));
    assert_eq!(malformed_separator.entity_records[0].suffix_value, None);

    let atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x81, 0x92, 0x81, 0xb3, 0x83, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic atom entity suffix");
    assert_eq!(
        atom.report.coverage["decoded_atom_entity_suffix_value_count"],
        1
    );
    let atom =
        crate::native::CatiaNative::load(atom.ir.native.namespace("catia").expect("namespace"))
            .expect("load generic atom suffix");
    assert!(matches!(
        atom.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete atom suffix")
            .payload,
        CatiaEntitySuffixPayload::Atom { value: 3 }
    ));
    let mut malformed_atom = atom;
    malformed_atom.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete atom suffix")
        .payload = CatiaEntitySuffixPayload::Atom { value: 4 };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_atom
        .store(&mut namespace)
        .expect("store malformed atom suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let truncated_compact_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x81, 0xb3, 0xd1,
        ]));
    assert_eq!(truncated_compact_atom.entity_records[0].suffix_value, None);

    let schema_selected_atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x81, 0x92, 0x82, 0x32, 4, 0, 0, 0, 0x81, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected atom entity suffix");
    assert_eq!(
        schema_selected_atom.report.coverage
            ["decoded_schema_selected_atom_entity_suffix_value_count"],
        1
    );
    assert_eq!(
        schema_selected_atom.report.coverage["decoded_schema_selected_entity_suffix_value_count"],
        1
    );
    let schema_selected_atom = crate::native::CatiaNative::load(
        schema_selected_atom
            .ir
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected atom suffix");
    assert!(matches!(
        schema_selected_atom.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete schema-selected atom suffix")
            .payload,
        CatiaEntitySuffixPayload::SchemaSelected {
            selector: 4,
            value: crate::native::CatiaEntitySuffixSelectedValue::Atom { value: 1 }
        }
    ));
    assert_eq!(
        schema_selected_atom.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved suffix selector"),
        &crate::native::CatiaEntitySuffixSchemaSelection {
            ordinal: 4,
            entry: schema_selected_atom.catalogs[0].entries[4].id.clone(),
            name: "Thickness".to_string(),
            value: crate::native::CatiaEntitySuffixSchemaValue::Atom { value: 1 },
        }
    );
    let mut malformed_schema_selected_atom = schema_selected_atom.clone();
    malformed_schema_selected_atom.entity_records[0]
        .suffix_schema_selection
        .as_mut()
        .expect("resolved suffix selector")
        .name = "Width".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_schema_selected_atom
        .store(&mut namespace)
        .expect("store malformed schema-selected atom suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let out_of_range_schema_selected_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x82, 0x32, 0xcf, 0, 0, 0, 0x81, 0x81, 0x49,
        ]));
    assert!(out_of_range_schema_selected_atom.entity_records[0]
        .suffix_schema_selection
        .is_none());

    let selected_scalar_bits = 17.25_f64.to_bits();
    let mut selected_scalar_suffix = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    selected_scalar_suffix.extend_from_slice(&selected_scalar_bits.to_le_bytes());
    selected_scalar_suffix.extend_from_slice(&[0x81, 0x4a]);
    let selected_scalar = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(
                &selected_scalar_suffix,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected scalar suffix");
    assert_eq!(
        selected_scalar.report.coverage
            ["decoded_schema_selected_evaluation_entity_suffix_value_count"],
        1
    );
    let selected_scalar = crate::native::CatiaNative::load(
        selected_scalar
            .ir
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected scalar suffix");
    assert!(matches!(
        selected_scalar.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected scalar"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            ordinal: 4,
            name,
            value: crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
                evaluation: CatiaEntityEvaluation::Scalar { bits }
            },
            ..
        } if name == "Thickness" && *bits == selected_scalar_bits
    ));

    let selected_unset =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe7,
        ]));
    assert!(matches!(
        selected_unset.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected unset"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            value: crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
                evaluation: CatiaEntityEvaluation::Unset
            },
            ..
        }
    ));

    let selected_control = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x88, 0x81, 0x32, 4, 0, 0, 0, 0xe8, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected control suffix");
    assert_eq!(
        selected_control.report.coverage
            ["decoded_schema_selected_control_entity_suffix_value_count"],
        1
    );
    assert_eq!(
        selected_control.report.coverage["decoded_schema_selected_entity_suffix_value_count"],
        1
    );
    let selected_control = crate::native::CatiaNative::load(
        selected_control
            .ir
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected control suffix");
    assert!(matches!(
        selected_control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete schema-selected control suffix")
            .payload,
        CatiaEntitySuffixPayload::SchemaSelected {
            selector: 4,
            value: crate::native::CatiaEntitySuffixSelectedValue::ControlE8,
        }
    ));
    assert!(matches!(
        selected_control.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected control suffix"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            ordinal: 4,
            name,
            value: crate::native::CatiaEntitySuffixSchemaValue::ControlE8,
            ..
        } if name == "Thickness"
    ));
    let mut malformed_selected_control = selected_control.clone();
    malformed_selected_control.entity_records[0]
        .suffix_schema_selection
        .as_mut()
        .expect("resolved schema-selected control suffix")
        .value = crate::native::CatiaEntitySuffixSchemaValue::Separator37;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_selected_control
        .store(&mut namespace)
        .expect("store malformed schema-selected control suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
    let malformed_selected_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x81, 0x32, 4, 0, 0, 0, 0xe8, 0x81, 0x49, 0x00,
        ]));
    assert_eq!(
        malformed_selected_control.entity_records[0].suffix_value,
        None
    );

    let selected_separator =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x82, 0x93, 0x81, 0x32, 4, 0, 0, 0, 0x37, 0x81, 0x52,
        ]));
    assert!(matches!(
        &selected_separator.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected separator")
            .value,
        crate::native::CatiaEntitySuffixSchemaValue::Separator37
    ));

    let selected_schema =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x93, 0x82, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0, 0x81, 0x49,
        ]));
    assert!(matches!(
        &selected_schema.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved nested suffix selector")
            .value,
        crate::native::CatiaEntitySuffixSchemaValue::SchemaSelector {
            ordinal: 5,
            ref name,
            ..
        } if name.as_deref() == Some("#1_ /2")
    ));

    let mut nonfinite_selected_scalar = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    nonfinite_selected_scalar.extend_from_slice(&f64::NAN.to_bits().to_le_bytes());
    let nonfinite_selected_scalar = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&nonfinite_selected_scalar),
    );
    assert_eq!(
        nonfinite_selected_scalar.entity_records[0].suffix_value,
        None
    );

    let malformed_schema_selected_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x82, 0x32, 0xcf, 0, 0, 0, 0x81, 0, 0, 0,
        ]));
    assert_eq!(
        malformed_schema_selected_atom.entity_records[0].suffix_value,
        None
    );

    let mut bare_scalar = vec![0x84, 0x96, 0x82, 0xb1, 0xe6];
    bare_scalar.extend_from_slice(&6.75_f64.to_bits().to_le_bytes());
    let bare_scalar =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&bare_scalar));
    assert_eq!(
        bare_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete bare scalar suffix")
            .trailer,
        CatiaEntitySuffixTrailer::Empty
    );

    let bare_unset = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x84, 0x96, 0x82, 0xb1, 0xe7,
    ]));
    assert!(matches!(
        bare_unset.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete bare unset suffix")
            .payload,
        CatiaEntitySuffixPayload::Evaluation {
            evaluation: CatiaEntityEvaluation::Unset,
            ..
        }
    ));

    let nested_bits = 11.725_f64.to_bits();
    let mut nested_scalar = vec![0x84, 0x88, 0x82, 0x32, 0xe6, 0, 0, 0, 0xe6];
    nested_scalar.extend_from_slice(&nested_bits.to_le_bytes());
    let nested_scalar =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&nested_scalar));
    let nested_value = nested_scalar.entity_records[0]
        .suffix_value
        .as_ref()
        .expect("complete zero-padded scalar suffix");
    assert_eq!(
        nested_value.payload,
        CatiaEntitySuffixPayload::Evaluation {
            evaluation: CatiaEntityEvaluation::Scalar { bits: nested_bits },
            encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
        }
    );
    assert_eq!(nested_value.trailer, CatiaEntitySuffixTrailer::Empty);

    let mut nonfinite_nested = vec![0x84, 0x88, 0x82, 0x32, 0xe6, 0, 0, 0, 0xe6];
    nonfinite_nested.extend_from_slice(&f64::INFINITY.to_bits().to_le_bytes());
    let nonfinite_nested = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&nonfinite_nested),
    );
    assert_eq!(nonfinite_nested.entity_records[0].suffix_value, None);

    let mut zero_frame_scalar = vec![0x84, 0x96, 0x82, 0x55, 0xe6];
    zero_frame_scalar.extend_from_slice(&(-26.703_618_806_753_155_f64).to_bits().to_le_bytes());
    zero_frame_scalar.extend_from_slice(&[0xfe, 0xf6]);
    zero_frame_scalar.extend_from_slice(&[0; 16]);
    let zero_frame_scalar = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&zero_frame_scalar),
    );
    assert_eq!(
        zero_frame_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete zero-frame scalar suffix")
            .trailer,
        CatiaEntitySuffixTrailer::FixedZeroFrame
    );

    let mut malformed_zero_frame = vec![0x84, 0x96, 0x82, 0x55, 0xe6];
    malformed_zero_frame.extend_from_slice(&1.0_f64.to_bits().to_le_bytes());
    malformed_zero_frame.extend_from_slice(&[0xfe, 0xf6]);
    malformed_zero_frame.extend_from_slice(&[0; 15]);
    malformed_zero_frame.push(1);
    let malformed_zero_frame = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&malformed_zero_frame),
    );
    assert_eq!(malformed_zero_frame.entity_records[0].suffix_value, None);

    let mut malformed_encoding = native.clone();
    malformed_encoding.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete suffix value")
        .payload = CatiaEntitySuffixPayload::Evaluation {
        evaluation: CatiaEntityEvaluation::Scalar { bits },
        encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_encoding
        .store(&mut namespace)
        .expect("store malformed suffix encoding");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete suffix value")
        .trailer = CatiaEntitySuffixTrailer::Token814A;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed suffix value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn native_namespace_binds_two_definition_value_chains() {
    use crate::native::{
        CatiaDefinitionChainValue, CatiaEntityEvaluation, CatiaEntitySchemaValue,
        CatiaEntitySuffixSchemaValue,
    };

    let bits = 12.5_f64.to_bits();
    let mut suffix = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x49]);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode definition-chain evaluation");
    assert_eq!(
        decoded.report.coverage["decoded_definition_chain_value_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_definition_chain_evaluation_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_structurally_owned_definition_chain_value_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_definition_chain_value_owner_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_evaluated_definition_chain_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_unset_definition_chain_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_structurally_owned_definition_chain_evaluation_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_definition_chain_evaluation_owner_count"],
        0
    );
    let mut native =
        crate::native::CatiaNative::load(decoded.ir.native.namespace("catia").expect("namespace"))
            .expect("load definition-chain evaluation");
    assert_eq!(
        native.entity_records[0].definition_chain_value,
        Some(CatiaDefinitionChainValue {
            selector: CatiaEntitySchemaValue {
                entry: native.catalogs[0].entries[4].id.clone(),
                value: "FeatureFEDGE".to_string(),
            },
            role: CatiaEntitySchemaValue {
                entry: native.catalogs[0].entries[5].id.clone(),
                value: "Real".to_string(),
            },
            value: CatiaEntitySuffixSchemaValue::Evaluation {
                evaluation: CatiaEntityEvaluation::Scalar { bits },
            },
        })
    );
    assert_eq!(
        native.design_objects[0].definition_chain_values,
        [native.entity_records[0].id.clone()]
    );

    let mut malformed_ownership = native.clone();
    malformed_ownership.design_objects[0]
        .definition_chain_values
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_ownership
        .store(&mut namespace)
        .expect("store malformed definition-chain ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    native.entity_records[0]
        .definition_chain_value
        .as_mut()
        .expect("definition-chain evaluation")
        .role
        .value = "changed".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store malformed definition-chain evaluation");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let wrong_selector =
        crate::native::CatiaNative::decode(&standard_catpart_with_definition_chain_value(&[
            0x84, 0x88, 0x82, 0x32, 5, 0, 0, 0, 0xe7,
        ]));
    assert!(wrong_selector.entity_records[0]
        .definition_chain_value
        .is_none());

    let atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x87,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode definition-chain atom");
    assert_eq!(
        atom.report.coverage["decoded_definition_chain_value_count"],
        1
    );
    assert_eq!(
        atom.report.coverage["decoded_definition_chain_atom_count"],
        1
    );
    assert_eq!(
        atom.report.coverage["decoded_definition_chain_evaluation_count"],
        0
    );
    let atom_native =
        crate::native::CatiaNative::load(atom.ir.native.namespace("catia").expect("namespace"))
            .expect("load definition-chain atom");
    assert_eq!(
        atom_native.entity_records[0]
            .definition_chain_value
            .as_ref()
            .map(|value| &value.value),
        Some(&CatiaEntitySuffixSchemaValue::Atom { value: 7 })
    );

    for (payload, coverage) in [
        (0xe8, "decoded_definition_chain_control_count"),
        (0x37, "decoded_definition_chain_separator_count"),
    ] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                    0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, payload,
                ])),
                &DecodeOptions::default(),
            )
            .expect("decode definition-chain state");
        assert_eq!(decoded.report.coverage[coverage], 1);
    }

    let nested = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_chain_value(&[
                0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode nested definition-chain selector");
    assert_eq!(
        nested.report.coverage["decoded_definition_chain_schema_selector_count"],
        1
    );
    let nested_native =
        crate::native::CatiaNative::load(nested.ir.native.namespace("catia").expect("namespace"))
            .expect("load nested definition-chain selector");
    assert_eq!(
        nested_native.entity_records[0]
            .definition_chain_value
            .as_ref()
            .map(|value| &value.value),
        Some(&CatiaEntitySuffixSchemaValue::SchemaSelector {
            ordinal: 5,
            entry: Some(nested_native.catalogs[0].entries[5].id.clone()),
            name: Some("Real".to_string()),
        })
    );
}

#[test]
fn design_objects_retain_definition_chain_values_in_field_order() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_two_definition_chain_values());
    assert_eq!(native.design_objects.len(), 1);
    assert_eq!(
        native.design_objects[0].definition_chain_values,
        native
            .entity_records
            .iter()
            .map(|entity| entity.id.clone())
            .collect::<Vec<_>>()
    );

    let mut reversed = native;
    reversed.design_objects[0].definition_chain_values.reverse();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    reversed
        .store(&mut namespace)
        .expect("store misordered definition-chain ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_two_definition_chain_values());
    let expected = native.design_objects[0].definition_chain_values.clone();
    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store current definition-chain ownership");
    let mut previous_design_objects: Vec<crate::native::CatiaDesignObject> = previous_namespace
        .arena_as("design_objects")
        .expect("load stored design objects");
    for object in &mut previous_design_objects {
        object.definition_chain_values.clear();
    }
    previous_namespace
        .set_arena("design_objects", &previous_design_objects)
        .expect("store previous design objects");
    previous_namespace.version = 195;
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous definition-chain ownership");
    assert_eq!(migrated.design_objects[0].definition_chain_values, expected);
}

#[test]
fn literal_owner_slots_remain_unassigned_and_migrate_from_previous_namespaces() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_unassigned_definition_chain_value()),
            &DecodeOptions::default(),
        )
        .expect("decode literal owner slot");
    assert_eq!(
        decoded.report.coverage["decoded_definition_chain_value_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_definition_chain_value_owner_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_unassigned_definition_chain_value_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_unassigned_definition_chain_evaluation_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_unassigned_object_owner_slot_count"],
        1
    );

    let native =
        crate::native::CatiaNative::load(decoded.ir.native.namespace("catia").expect("namespace"))
            .expect("load literal owner slot");
    let record = &native.object_graphs[0].records[0];
    assert_eq!(
        record.owner,
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(66))
    );
    assert!(record.design_object.is_none());
    assert!(native.design_objects.is_empty());

    let mut malformed = native.clone();
    malformed.object_graphs[0].records[0].owner =
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(67));
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed literal owner slot");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut previous_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut previous_namespace)
        .expect("store current literal owner slot");
    let mut previous_records: Vec<crate::native::CatiaObjectRecord> = previous_namespace
        .arena_as("object_graph_records")
        .expect("load stored object records");
    previous_records[0].owner = None;
    previous_namespace
        .set_arena("object_graph_records", &previous_records)
        .expect("store previous object records");
    previous_namespace.version = 197;
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate previous literal owner slot");
    assert_eq!(
        migrated.object_graphs[0].records[0].owner,
        Some(crate::native::CatiaObjectOwner::UnassignedLiteral(66))
    );
}

#[test]
fn native_namespace_binds_and_validates_definition_values() {
    use crate::native::{
        CatiaDefinitionValue, CatiaEntityEvaluation, CatiaEntityEvaluationEncoding,
        CatiaEntitySchemaValue, CatiaEntitySuffixPayload,
    };

    let bits = 12.5_f64.to_bits();
    let mut suffix = vec![0xd1, 0x53, 0x96, 0x82, 0xa6, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    let definition = [0x00, 0x08, 0x32, 4, 0, 0, 0];
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_definition_value(
                &definition,
                &[0xfe],
                &suffix,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode definition-bound value");
    assert_eq!(decoded.report.coverage["decoded_definition_value_count"], 1);
    assert_eq!(
        decoded.report.coverage["decoded_owned_definition_value_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_definition_value_owner_count"],
        0
    );
    let mut native =
        crate::native::CatiaNative::load(decoded.ir.native.namespace("catia").expect("namespace"))
            .expect("load definition-bound value");
    assert_eq!(
        native.entity_records[0].definition_value,
        Some(CatiaDefinitionValue {
            definition: CatiaEntitySchemaValue {
                entry: native.catalogs[0].entries[4].id.clone(),
                value: "Thickness".to_string(),
            },
            payload: CatiaEntitySuffixPayload::Evaluation {
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            schema_selection: None,
        })
    );
    assert_eq!(
        native.design_objects[0].definition_values,
        [native.entity_records[0].id.clone()]
    );
    assert_eq!(
        native.object_graphs[0].records[0].storage_record,
        Some(native.object_graphs[0].records[0].id.clone())
    );
    assert_eq!(
        native.object_graphs[0].records[0].storage_design_object,
        Some(native.design_objects[0].id.clone())
    );

    let mut malformed_storage = native.clone();
    malformed_storage.object_graphs[0].records[0].storage_record = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_storage
        .store(&mut namespace)
        .expect("store malformed storage link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed_ownership = native.clone();
    malformed_ownership.design_objects[0]
        .definition_values
        .clear();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed_ownership
        .store(&mut namespace)
        .expect("store malformed definition-value ownership");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let definition_value = native.entity_records[0]
        .definition_value
        .as_mut()
        .expect("definition-bound value");
    definition_value.payload = CatiaEntitySuffixPayload::Evaluation {
        evaluation: CatiaEntityEvaluation::Unset,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store malformed definition value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let control = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
        &definition,
        &[0xfe],
        &[0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49],
    ));
    assert!(matches!(
        control.entity_records[0]
            .definition_value
            .as_ref()
            .expect("definition-bound control")
            .payload,
        CatiaEntitySuffixPayload::ControlE8
    ));

    let schema_selected =
        crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
            &definition,
            &[0xfe],
            &[0x84, 0x96, 0x82, 0x32, 4, 0, 0, 0, 0xe7, 0x81, 0x49],
        ));
    let definition_value = schema_selected.entity_records[0]
        .definition_value
        .as_ref()
        .expect("definition-bound schema-selected value");
    assert!(matches!(
        definition_value.payload,
        CatiaEntitySuffixPayload::SchemaSelected { selector: 4, .. }
    ));
    assert_eq!(
        definition_value
            .schema_selection
            .as_ref()
            .expect("resolved suffix schema")
            .name,
        "Thickness"
    );

    for (definition, value) in [
        (
            vec![0x00, 0x08, 0x32, 4, 0, 0, 0, 0x32, 4, 0, 0, 0],
            vec![0xfe],
        ),
        (definition.to_vec(), vec![0x80, 0xfe]),
    ] {
        let native = crate::native::CatiaNative::decode(&standard_catpart_with_definition_value(
            &definition,
            &value,
            &suffix,
        ));
        assert_eq!(native.entity_records[0].definition_value, None);
    }
}

#[test]
fn named_parameter_value_requires_the_complete_finite_suffix() {
    let nonfinite = f64::NAN.to_bits();
    let mut suffix = vec![0x85, 0x96, 0x82, 0x6a, 0xe6];
    suffix.extend_from_slice(&nonfinite.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x52]);

    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&suffix));
    assert!(native.entity_records[0].suffix_value.is_none());
    assert!(native.entity_records[0].parameter_value.is_none());

    let control = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x85, 0x96, 0x82, 0x6a, 0xe8, 0x81, 0x52,
    ]));
    assert!(matches!(
        control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete control suffix")
            .payload,
        crate::native::CatiaEntitySuffixPayload::ControlE8
    ));
    assert!(control.entity_records[0].parameter_value.is_none());
}

#[test]
fn native_namespace_types_and_validates_formula_relations() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, false));
    let formula = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation");
    assert_eq!(formula.expression_entity.entity_id, 2);
    assert_eq!(
        formula.expression_entity.entity.as_deref(),
        Some(native.entity_records[1].id.as_str())
    );
    assert_eq!(
        formula.expression_entity.class_name,
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .find(|record| record.entity_id == Some(2))
            .and_then(|record| record.class_name.clone())
    );
    assert_eq!(formula.output_entity.entity_id, 99);
    assert_eq!(formula.output_entity.entity, None);
    let parameter_entity = &native.entity_records[2];
    assert_eq!(
        formula.parameter_dependencies,
        [crate::native::CatiaFormulaParameterDependency {
            symbol: "#1_ /2".to_string(),
            candidates: vec![crate::native::CatiaEntityReference {
                entity_id: parameter_entity.entity_id,
                is_null: false,
                entity: Some(parameter_entity.id.clone()),
                class_name: native
                    .object_graphs
                    .iter()
                    .flat_map(|graph| &graph.records)
                    .find(|record| record.entity_id == Some(parameter_entity.entity_id))
                    .and_then(|record| record.class_name.clone()),
            }],
        }]
    );
    let expected_formula = formula.clone();

    let mut version_235_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_235_namespace)
        .expect("store current formula output reference");
    let formula_fields = version_235_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object");
    let expression = formula_fields
        .remove("expression_entity")
        .expect("stored expression entity");
    let expression = expression
        .as_object()
        .expect("stored expression-entity object");
    formula_fields.insert("expression".to_string(), expression["entity"].clone());
    let output = formula_fields
        .remove("output_entity")
        .expect("stored output entity");
    let output = output.as_object().expect("stored output-entity object");
    formula_fields.insert(
        "parameter_entity_id".to_string(),
        output["entity_id"].clone(),
    );
    formula_fields.insert(
        "parameter_is_null".to_string(),
        output.get("is_null").cloned().unwrap_or_default(),
    );
    formula_fields.insert(
        "parameter".to_string(),
        output.get("entity").cloned().unwrap_or_default(),
    );
    version_235_namespace.version = crate::native::CATIA_FORMULA_OUTPUT_REFERENCE_VERSION - 1;
    let migrated = crate::native::CatiaNative::load(&version_235_namespace)
        .expect("migrate formula output reference");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_236_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_236_namespace)
        .expect("store current formula expression reference");
    let formula_fields = version_236_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object");
    let expression = formula_fields
        .remove("expression_entity")
        .expect("stored expression entity");
    formula_fields.insert(
        "expression".to_string(),
        expression
            .as_object()
            .expect("stored expression-entity object")["entity"]
            .clone(),
    );
    version_236_namespace.version = crate::native::CATIA_FORMULA_EXPRESSION_REFERENCE_VERSION - 1;
    let migrated = crate::native::CatiaNative::load(&version_236_namespace)
        .expect("migrate formula expression reference");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_237_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_237_namespace)
        .expect("store current formula dependency references");
    let candidates = version_237_namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields
        .get_mut("formula_relation")
        .expect("stored formula relation")
        .as_object_mut()
        .expect("stored formula-relation object")
        .get_mut("parameter_dependencies")
        .expect("stored parameter dependencies")
        .as_array_mut()
        .expect("stored parameter-dependency array")[0]
        .as_object_mut()
        .expect("stored parameter dependency")
        .get_mut("candidates")
        .expect("stored dependency candidates")
        .as_array_mut()
        .expect("stored candidate array");
    for candidate in candidates {
        *candidate = candidate.as_object().expect("stored candidate reference")["entity"].clone();
    }
    version_237_namespace.version = crate::native::CATIA_FORMULA_DEPENDENCY_REFERENCE_VERSION - 1;
    let migrated = crate::native::CatiaNative::load(&version_237_namespace)
        .expect("migrate formula dependency references");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula.clone())
    );

    let mut version_205_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_205_namespace)
        .expect("store current formula dependency candidates");
    let mut version_205_entities: Vec<crate::native::CatiaEntityRecord> = version_205_namespace
        .arena_as("entity_records")
        .expect("load version 205 entity records");
    version_205_entities[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .parameter_dependencies[0]
        .candidates
        .clear();
    version_205_namespace
        .set_arena("entity_records", &version_205_entities)
        .expect("store version 205 entity records");
    version_205_namespace.version = 205;
    let migrated = crate::native::CatiaNative::load(&version_205_namespace)
        .expect("migrate version 205 formula dependency candidates");
    assert_eq!(
        migrated.entity_records[0].formula_relation,
        Some(expected_formula)
    );

    let mut malformed = native;
    malformed.entity_records[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .output_entity
        .entity_id = 98;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    malformed
        .store(&mut namespace)
        .expect("store malformed formula relation");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn formula_relation_requires_a_complete_relation_expression_target() {
    let mut file = standard_catpart_with_formula_relation(0x63, false);
    let role = file
        .windows("param".len())
        .position(|bytes| bytes == b"param")
        .expect("formula parameter role");
    file[role..role + "param".len()].copy_from_slice(b"other");

    let native = crate::native::CatiaNative::decode(&file);
    assert!(native.entity_records[0].formula_relation.is_none());
}

#[test]
fn formula_parameter_dependency_requires_a_unique_binding() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_formula_relation(0x63, true));
    let dependency = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0];

    assert_eq!(dependency.symbol, "#1_ /2");
    assert_eq!(dependency.candidates.len(), 2);
}

#[test]
fn formula_parameter_dependency_retains_an_unmatched_symbol() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#2_ /2", 35.0)],
        "LENGTH",
        Some(33.0),
        "#1_ /2-2mm",
    ));
    let dependency = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0];

    assert_eq!(dependency.symbol, "#1_ /2");
    assert!(dependency.candidates.is_empty());
}

#[test]
fn formula_relation_resolves_bare_expression_symbols() {
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#1_", 35.0)],
        "LENGTH",
        Some(33.0),
        "#1_-2mm",
    ));

    assert_eq!(
        native.entity_records[0]
            .formula_relation
            .as_ref()
            .expect("complete formula relation")
            .parameter_dependencies,
        [crate::native::CatiaFormulaParameterDependency {
            symbol: "#1_".to_string(),
            candidates: vec![crate::native::CatiaEntityReference {
                entity_id: native.entity_records[2].entity_id,
                is_null: false,
                entity: Some(native.entity_records[2].id.clone()),
                class_name: native.object_graphs[0]
                    .records
                    .iter()
                    .find(|record| record.entity_id == Some(native.entity_records[2].entity_id))
                    .and_then(|record| record.class_name.clone()),
            }],
        }]
    );
}

#[test]
fn decode_transfers_a_complete_typed_input_when_the_formula_output_is_unresolved() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_relation(0x63, false)),
            &DecodeOptions::default(),
        )
        .expect("decode formula with unresolved output");
    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("independently typed formula input")
    };

    assert_eq!(input.name, "Thickness");
    assert_eq!(input.ordinal, 0);
    assert_eq!(input.expression, "35 mm");
    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert!(input.dependencies.is_empty());
    assert_eq!(decoded.report.coverage["transferred_parameter_count"], 1);
    assert_eq!(
        decoded.report.coverage["decoded_resolved_formula_output_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unresolved_formula_output_count"],
        1
    );
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new())
        .findings
        .is_empty());
}

#[test]
fn terminal_entity_identity_is_a_null_formula_output() {
    let bytes = standard_catpart_with_formula_relation(5, false);
    let native = crate::native::CatiaNative::decode(&bytes);
    let formula = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation");
    assert_eq!(formula.output_entity.entity_id, 5);
    assert!(formula.output_entity.is_null);
    assert_eq!(formula.output_entity.entity, None);
    let formula_record = native.object_graphs[0]
        .records
        .iter()
        .find(|record| record.id == native.entity_records[0].object_record)
        .expect("formula object record");
    assert!(formula_record.references[2].is_null);
    assert_eq!(formula_record.references[2].target, None);

    let mut version_210_namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut version_210_namespace)
        .expect("store terminal null references");
    let mut version_210_records: Vec<crate::native::CatiaObjectRecord> = version_210_namespace
        .arena_as("object_graph_records")
        .expect("load version 210 object records");
    for record in &mut version_210_records {
        for reference in &mut record.references {
            reference.is_null = false;
        }
    }
    version_210_namespace
        .set_arena("object_graph_records", &version_210_records)
        .expect("store version 210 object records");
    let mut version_210_entities: Vec<crate::native::CatiaEntityRecord> = version_210_namespace
        .arena_as("entity_records")
        .expect("load version 210 entity records");
    version_210_entities[0]
        .formula_relation
        .as_mut()
        .expect("complete formula relation")
        .output_entity
        .is_null = false;
    version_210_namespace
        .set_arena("entity_records", &version_210_entities)
        .expect("store version 210 entity records");
    version_210_namespace.version = 210;
    let migrated = crate::native::CatiaNative::load(&version_210_namespace)
        .expect("migrate terminal null references");
    assert!(migrated.object_graphs[0].records[0].references[2].is_null);
    assert!(
        migrated.entity_records[0]
            .formula_relation
            .as_ref()
            .expect("migrated formula relation")
            .output_entity
            .is_null
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode formula with null output");
    assert_eq!(
        decoded.report.coverage["decoded_null_formula_output_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_classified_formula_output_entity_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unclassified_formula_output_entity_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_formula_output_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["decoded_null_object_record_reference_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_object_record_reference_count"],
        0
    );
}

#[test]
fn formula_input_with_additional_object_payload_remains_unresolved() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_typed_formula_inputs_and_object_payload(
                    0x63,
                    false,
                    &[("#1_", "LENGTH", "Thickness", "#1_ /2", 35.0)],
                    "LENGTH",
                    Some(33.0),
                    "#1_ /2-2mm",
                    (&[0x81, 0xfe], None),
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode formula input with additional object payload");

    assert_eq!(decoded.ir.model.parameters.len(), 1);
    assert_eq!(
        decoded.report.coverage["transferred_formula_design_record_count"],
        0
    );
    assert_eq!(decoded.report.coverage["unresolved_design_record_count"], 4);
}

#[test]
fn decode_transfers_a_closed_length_formula_and_its_input() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let bytes = standard_catpart_with_formula_relation(4, false);
    let native = crate::native::CatiaNative::decode(&bytes);
    let output_entity = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .output_entity;
    assert_eq!(output_entity.entity_id, 4);
    assert!(output_entity.entity.is_some());
    assert_eq!(
        output_entity.class_name,
        native
            .object_graphs
            .iter()
            .flat_map(|graph| &graph.records)
            .find(|record| record.entity_id == Some(4))
            .and_then(|record| record.class_name.clone())
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode closed length formula");
    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("closed formula parameters")
    };

    assert_eq!(input.name, "Thickness");
    assert_eq!(input.expression, "35 mm");
    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(input.properties["value_type"], "LENGTH");
    assert!(input.dependencies.is_empty());
    assert_eq!(output.name, "Result");
    assert_eq!(output.ordinal, 1);
    assert_eq!(output.expression, "#1_ /2-2mm");
    assert_eq!(output.value, Some(ParameterValue::Length(Length(33.0))));
    assert_eq!(output.properties["value_type"], "LENGTH");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(decoded.report.coverage["transferred_parameter_count"], 2);
    assert_eq!(
        decoded.report.coverage["transferred_formula_design_record_count"],
        4
    );
    assert_eq!(
        decoded.report.coverage["decoded_resolved_formula_output_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_classified_formula_output_entity_count"],
        usize::from(output_entity.class_name.is_some())
    );
    assert_eq!(
        decoded.report.coverage["unclassified_formula_output_entity_count"],
        usize::from(output_entity.class_name.is_none())
    );
    let expression_classified = native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .expression_entity
        .class_name
        .is_some();
    assert_eq!(
        decoded.report.coverage["decoded_classified_formula_expression_entity_count"],
        usize::from(expression_classified)
    );
    assert_eq!(
        decoded.report.coverage["unclassified_formula_expression_entity_count"],
        usize::from(!expression_classified)
    );
    let dependency_candidate = &native.entity_records[0]
        .formula_relation
        .as_ref()
        .expect("complete formula relation")
        .parameter_dependencies[0]
        .candidates[0];
    assert_eq!(
        decoded.report.coverage["decoded_formula_parameter_dependency_candidate_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_classified_formula_parameter_dependency_candidate_count"],
        usize::from(dependency_candidate.class_name.is_some())
    );
    assert_eq!(
        decoded.report.coverage["unclassified_formula_parameter_dependency_candidate_count"],
        usize::from(dependency_candidate.class_name.is_none())
    );
    assert_eq!(
        decoded.report.coverage["decoded_referenced_relation_expression_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["unresolved_unreferenced_relation_expression_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unresolved_formula_output_count"],
        0
    );
    assert_eq!(decoded.report.coverage["unresolved_design_record_count"], 0);
    assert!(decoded.report.losses.iter().all(|loss| {
        loss.category != cadmpeg_ir::report::LossCategory::DesignIntent
            || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    assert_eq!(
        decoded.source_fidelity.annotations.exactness[&input.id.0].fields["expression"],
        cadmpeg_ir::Exactness::Derived
    );
    assert_eq!(
        decoded.source_fidelity.annotations.exactness[&input.id.0].fields["properties"],
        cadmpeg_ir::Exactness::Derived
    );
    assert_eq!(
        decoded.source_fidelity.annotations.exactness[&output.id.0].fields["properties"],
        cadmpeg_ir::Exactness::Derived
    );
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new())
        .findings
        .is_empty());
}

#[test]
fn decode_keeps_a_mismatched_formula_result_unresolved() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 35.0)],
                "LENGTH",
                Some(34.0),
                "#1_ /2-2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula with mismatched stored result");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
    assert!(input.dependencies.is_empty());
    assert_eq!(
        decoded.report.coverage["transferred_formula_design_record_count"],
        1
    );
    assert_eq!(decoded.report.coverage["unresolved_design_record_count"], 3);
}

#[test]
fn decode_evaluates_formula_precedence_and_parentheses() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                Some(30.0),
                "(#1_ /2+3mm)*2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode parenthesized formula");

    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("validated formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(30.0)
        ))
    );
}

#[test]
fn decode_transfers_a_closed_constant_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(12.0),
                "10mm+2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode constant formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("constant formula output")
    };
    assert_eq!(output.name, "Result");
    assert_eq!(output.expression, "10mm+2mm");
    assert!(output.dependencies.is_empty());
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.0)
        ))
    );
    assert!(decoded
        .source_fidelity
        .annotations
        .exactness
        .get(&output.id.0)
        .is_none_or(|annotation| !annotation.fields.contains_key("expression")));
}

#[test]
fn decode_rejects_a_constant_formula_that_disagrees_with_its_stored_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(13.0),
                "10mm+2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode mismatched constant formula");

    assert!(decoded.ir.model.parameters.is_empty());
    assert_eq!(
        decoded.report.coverage["transferred_formula_design_record_count"],
        0
    );
}

#[test]
fn decode_converts_degree_literals_to_radians() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "Integer", "Count", "#1_ /2", 4.0)],
                "ANGLE",
                Some(std::f64::consts::FRAC_PI_2),
                "360.0*1 deg/#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode degree formula");

    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("degree formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2)
        ))
    );
}

#[test]
fn decode_evaluates_the_dimensionless_pi_constant_in_an_angle_expression() {
    let output_value = std::f64::consts::PI - 1.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "ANGLE", "Angle", "#1_ /2", 1.0)],
                "ANGLE",
                Some(output_value),
                "PI*1rad-#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula with PI");

    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("PI formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(output_value)
        ))
    );
}

#[test]
fn decode_evaluates_dimensionless_trigonometric_arguments_as_radians() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(0.0),
                "sin(0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode scalar-radian trigonometric formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("scalar-radian trigonometric formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(0.0))
    );
}

#[test]
fn decode_evaluates_dimension_checked_trigonometric_calls() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[(
                    "#1_",
                    "ANGLE",
                    "Sweep",
                    "#1_ /2",
                    std::f64::consts::FRAC_PI_2,
                )],
                "Real",
                Some(1.0),
                "sin(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode trigonometric formula");

    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("trigonometric formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(1.0))
    );
}

#[test]
fn decode_evaluates_nested_logarithm_and_extrema_calls() {
    let output_value = -(4.0_f64.log10()) / 100.0_f64.log10() / 2.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "Gain", "#1_ /2", 2.0),
                    ("#2_", "Real", "Reference", "#2_ /3", 10.0),
                ],
                "Real",
                Some(output_value),
                "-log(min(100,max(20*#1_ /2,#2_ /3)/#2_ /3))/log(100)/2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode logarithmic formula");

    let [first, second, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("logarithmic formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(output_value))
    );
}

#[test]
fn decode_distinguishes_common_and_natural_logarithms() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(3.0),
                "log(100)+ln(E)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode logarithm formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("logarithm formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(3.0))
    );
}

#[test]
fn decode_normalizes_every_admitted_formula_length_unit_to_millimetres() {
    let expected = 0.001 + 1_609_344.0 + 914.4 + 1.0 + 10.0 + 1_000_000.0 + 304.8 + 25.4 + 1_000.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(expected),
                "1micron+1mile+1yard+1mm+1cm+1km+1ft+1in+1m",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode complete length-unit formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("length-unit formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(expected)
        ))
    );
}

#[test]
fn decode_normalizes_every_admitted_formula_angle_unit_to_radians() {
    let expected = 1.0 + std::f64::consts::PI / 200.0 + std::f64::consts::PI / 180.0;
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(expected),
                "1rad+1grad+1deg",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode complete angle-unit formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("angle-unit formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(expected)
        ))
    );
}

#[test]
fn decode_evaluates_exponential_and_hyperbolic_functions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(2.0),
                "exp(0)+sinh(0)+cosh(0)+tanh(0)+asinh(0)+acosh(1)+atanh(0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponential and hyperbolic formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("exponential and hyperbolic formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(2.0))
    );
}

#[test]
fn decode_evaluates_scalar_rounding_functions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(8.0),
                "ceil(1.2)+floor(1.8)+int(-1.8)+round(2.5)+round(3.5)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode scalar rounding formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("scalar rounding formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(8.0))
    );
}

#[test]
fn decode_evaluates_dimensioned_rounding_in_the_selected_unit() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(1_230.0),
                "round(1234mm,\"cm\",0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned rounding formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("dimensioned rounding formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1_230.0)
        ))
    );
}

#[test]
fn decode_evaluates_integer_part_as_an_integer_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Integer",
                Some(-1.0),
                "int(-1.8)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode integer-part formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("integer-part formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Integer(-1))
    );
}

#[test]
fn decode_evaluates_variadic_extrema_and_integer_part_remainder() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(9.0),
                "min(8,5,7,3)+max(1,4,2)+mod(7.8,3)+max(1)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode variadic extrema and remainder formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("variadic extrema and remainder formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(9.0))
    );
}

#[test]
fn decode_evaluates_remainder_of_a_negative_real_dividend_integer_part() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(-1.0),
                "mod(-7.5,3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode negative real remainder formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("negative real remainder formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(-1.0))
    );
}

#[test]
fn decode_evaluates_a_square_root_of_a_dimensioned_product() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Width", "#1_ /2", 3.0),
                    ("#2_", "LENGTH", "Height", "#2_ /3", 4.0),
                ],
                "LENGTH",
                Some(5.0),
                "sqrt(#1_ /2*#1_ /2+#2_ /3*#2_ /3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned square root");

    let [first, second, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("square-root formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(5.0)
        ))
    );
}

#[test]
fn decode_evaluates_right_associative_exponentiation_above_unary_signs() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(-512.0),
                "-2**3**2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponent formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("exponent formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(-512.0))
    );
}

#[test]
fn decode_evaluates_an_integral_power_of_a_dimensioned_value() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 3.0)],
                "LENGTH",
                Some(3.0),
                "sqrt((#1_ /2)**2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensioned exponent formula");

    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("dimensioned exponent formula parameters")
    };
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(3.0)
        ))
    );
}

#[test]
fn decode_evaluates_inverse_trigonometric_calls_as_angles() {
    let output_value = 0.5_f64.asin() + 0.5_f64.acos() + 1.0_f64.atan();
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(output_value),
                "asin(0.5)+acos(0.5)+atan(1)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode inverse trigonometric formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("inverse trigonometric formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(output_value)
        ))
    );
}

#[test]
fn decode_evaluates_dimension_safe_absolute_and_tangent_calls() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Offset", "#1_ /2", -2.0),
                    ("#2_", "ANGLE", "Slope", "#2_ /3", 0.0),
                ],
                "LENGTH",
                Some(2.0),
                "abs(#1_ /2)*(1+tan(#2_ /3))",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode absolute and tangent formula");

    let [first, second, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("absolute and tangent formula parameters")
    };
    assert_eq!(output.dependencies, [first.id.clone(), second.id.clone()]);
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(2.0)
        ))
    );
}

#[test]
fn decode_rejects_a_square_root_with_an_odd_dimension_exponent() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "AreaLike", "#1_ /2", 4.0)],
                "LENGTH",
                Some(2.0),
                "sqrt(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid square root");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "AreaLike");
}

#[test]
fn decode_rejects_a_fractional_power_of_a_dimensioned_value() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 4.0)],
                "LENGTH",
                Some(2.0),
                "(#1_ /2)**0.5",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid exponent formula");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_dimension_exponent_overflow() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "LENGTH",
                Some(1.0),
                "((#1_ /2)**2147483647)**2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode exponent-overflow formula");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_inverse_trigonometry_outside_its_scalar_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "ANGLE",
                Some(std::f64::consts::FRAC_PI_4),
                "atan(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid inverse trigonometric formula");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_inverse_trigonometry_outside_its_numeric_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "ANGLE",
                Some(0.0),
                "asin(2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode out-of-domain inverse trigonometric formula");

    assert!(decoded.ir.model.parameters.is_empty());
}

#[test]
fn decode_rejects_scalar_functions_with_dimensioned_arguments() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 1.0)],
                "Real",
                Some(1.0),
                "exp(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid exponential formula");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
}

#[test]
fn decode_rejects_invalid_inverse_hyperbolic_domains() {
    for expression in ["acosh(0.5)", "atanh(1)"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    3,
                    false,
                    &[],
                    "Real",
                    Some(0.0),
                    expression,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode out-of-domain inverse hyperbolic formula");

        assert!(decoded.ir.model.parameters.is_empty(), "{expression}");
    }
}

#[test]
fn decode_rejects_nonfinite_exponential_results() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "Real",
                Some(0.0),
                "exp(1000)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode overflowing exponential formula");

    assert!(decoded.ir.model.parameters.is_empty());
}

#[test]
fn decode_rejects_invalid_remainder_divisors() {
    for expression in ["mod(7,0)", "mod(7,2.5)", "mod(7,1mm)"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    3,
                    false,
                    &[],
                    "Real",
                    Some(0.0),
                    expression,
                )),
                &DecodeOptions::default(),
            )
            .expect("decode invalid remainder formula");

        assert!(decoded.ir.model.parameters.is_empty(), "{expression}");
    }
}

#[test]
fn decode_rejects_a_logarithm_outside_its_dimensionless_positive_domain() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "Real", "Ratio", "#1_ /2", 0.0)],
                "Real",
                Some(0.0),
                "log(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode out-of-domain logarithm");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Ratio");
}

#[test]
fn decode_transfers_linear_interpolation_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(4.0),
                "LinearInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode linear interpolation formula");

    let [start, end, fraction, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("linear interpolation parameters")
    };
    assert_eq!(start.value, Some(cadmpeg_ir::ParameterValue::Real(2.0)));
    assert_eq!(end.value, Some(cadmpeg_ir::ParameterValue::Real(10.0)));
    assert_eq!(fraction.value, Some(cadmpeg_ir::ParameterValue::Real(0.25)));
    assert_eq!(output.value, Some(cadmpeg_ir::ParameterValue::Real(4.0)));
    assert_eq!(
        output.dependencies,
        vec![start.id.clone(), end.id.clone(), fraction.id.clone()]
    );
}

#[test]
fn decode_transfers_cubic_interpolation_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(3.25),
                "CubicInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode cubic interpolation formula");

    let [start, end, fraction, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("cubic interpolation parameters")
    };
    assert_eq!(start.value, Some(cadmpeg_ir::ParameterValue::Real(2.0)));
    assert_eq!(end.value, Some(cadmpeg_ir::ParameterValue::Real(10.0)));
    assert_eq!(fraction.value, Some(cadmpeg_ir::ParameterValue::Real(0.25)));
    assert_eq!(output.value, Some(cadmpeg_ir::ParameterValue::Real(3.25)));
    assert_eq!(
        output.dependencies,
        vec![start.id.clone(), end.id.clone(), fraction.id.clone()]
    );
}

#[test]
fn decode_rejects_dimensioned_cubic_interpolation_arguments() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "Real", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "LENGTH", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(3.25),
                "CubicInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid cubic interpolation");

    assert_eq!(decoded.ir.model.parameters.len(), 3);
}

#[test]
fn decode_converts_metric_length_literals_to_millimetres() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                3,
                false,
                &[],
                "LENGTH",
                Some(1_023.0),
                "1m+2cm+3mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode metric length formula");

    let [output] = decoded.ir.model.parameters.as_slice() else {
        panic!("metric length formula output")
    };
    assert_eq!(
        output.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1_023.0)
        ))
    );
}

#[test]
fn decode_rejects_dimensioned_linear_interpolation_arguments() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "LENGTH", "Start", "#1_ /2", 2.0),
                    ("#2_", "Real", "End", "#2_ /3", 10.0),
                    ("#3_", "Real", "Fraction", "#3_ /4", 0.25),
                ],
                "Real",
                Some(4.0),
                "LinearInterpolation(#1_ /2,#2_ /3,#3_ /4)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid linear interpolation");

    assert_eq!(decoded.ir.model.parameters.len(), 3);
}

#[test]
fn decode_rejects_extrema_between_different_dimensions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "LENGTH", "Length", "#1_ /2", 2.0),
                    ("#2_", "ANGLE", "Angle", "#2_ /3", 1.0),
                ],
                "LENGTH",
                Some(2.0),
                "max(#1_ /2,#2_ /3)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid maximum");

    let [first, second] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed inputs")
    };
    assert_eq!(first.name, "Length");
    assert_eq!(second.name, "Angle");
}

#[test]
fn decode_rejects_trigonometric_calls_with_length_arguments() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Offset", "#1_ /2", 0.0)],
                "Real",
                Some(0.0),
                "sin(#1_ /2)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid trigonometric formula");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Offset");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_rejects_dimensionally_invalid_formula_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                Some(12.0),
                "#1_ /2+1rad",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode dimensionally invalid formula");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Width");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_transfers_typed_integer_to_angle_formula() {
    use cadmpeg_ir::features::{Angle, ParameterValue};

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "Integer",
                "ANGLE",
                2.0,
                0.5,
                "#1_ /2*0.25rad",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode typed formula");
    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("typed formula parameters")
    };

    assert_eq!(input.expression, "2");
    assert_eq!(input.value, Some(ParameterValue::Integer(2)));
    assert_eq!(output.value, Some(ParameterValue::Angle(Angle(0.5))));
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new())
        .findings
        .is_empty());
}

#[test]
fn decode_transfers_dimensionless_real_formula() {
    use cadmpeg_ir::features::ParameterValue;

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4, false, "Real", "R", 2.5, 1.25, "#1_ /2/2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode real formula");
    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("real formula parameters")
    };

    assert_eq!(input.expression, "2.5");
    assert_eq!(input.value, Some(ParameterValue::Real(2.5)));
    assert_eq!(input.properties["value_type"], "Real");
    assert_eq!(output.value, Some(ParameterValue::Real(1.25)));
    assert_eq!(output.properties["value_type"], "Real");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    for parameter in [input, output] {
        assert_eq!(
            decoded.source_fidelity.annotations.exactness[&parameter.id.0].fields["properties"],
            cadmpeg_ir::Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_an_unset_typed_formula_result() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                4,
                false,
                &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                "LENGTH",
                None,
                "#1_ /2+1mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode unset formula result");
    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("unset formula parameters")
    };

    assert_eq!(output.value, None);
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.expression, "#1_ /2+1mm");
    assert_eq!(output.properties["value_type"], "LENGTH");
}

#[test]
fn decode_transfers_a_typed_boolean_predicate_formula() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "X", "#1_ /2", 5.0),
                    ("#2_", "Real", "Y", "#2_ /2", 3.0),
                ],
                "Boolean",
                None,
                "(#1_ /2>#2_ /2) and (#1_ /2>=0)",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode Boolean predicate formula");
    let [x, y, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("predicate formula parameters")
    };

    assert_eq!(output.value, None);
    assert_eq!(output.properties["value_type"], "Boolean");
    assert_eq!(output.expression, "(#1_ /2>#2_ /2) and (#1_ /2>=0)");
    assert_eq!(output.dependencies, [x.id.clone(), y.id.clone()]);
    assert_eq!(
        decoded.report.coverage["transferred_formula_design_record_count"],
        5
    );
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new()).is_ok());
}

#[test]
fn decode_transfers_an_unset_typed_formula_input_without_deriving_the_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(
                standard_catpart_with_typed_formula_inputs_and_object_payload(
                    4,
                    false,
                    &[("#1_", "LENGTH", "Width", "#1_ /2", 12.0)],
                    "LENGTH",
                    Some(13.0),
                    "#1_ /2+1mm",
                    (&[0xfe], Some(0)),
                ),
            ),
            &DecodeOptions::default(),
        )
        .expect("decode unset formula input");
    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed unset input")
    };

    assert_eq!(input.name, "Width");
    assert_eq!(input.value, None);
    assert!(input.expression.is_empty());
    assert!(input.dependencies.is_empty());
    assert_eq!(input.properties["value_type"], "LENGTH");
}

#[test]
fn decode_transfers_unset_non_numeric_formula_inputs_without_deriving_the_output() {
    for parameter_type in ["Boolean", "String"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(
                    standard_catpart_with_typed_formula_inputs_and_object_payload(
                        4,
                        false,
                        &[("#1_", parameter_type, "Value", "#1_ /2", 1.0)],
                        "Real",
                        Some(1.0),
                        "#1_ /2",
                        (&[0xfe], Some(0)),
                    ),
                ),
                &DecodeOptions::default(),
            )
            .expect("decode unset non-numeric formula input");
        let [input] = decoded.ir.model.parameters.as_slice() else {
            panic!("only the independently typed unset input")
        };

        assert_eq!(input.name, "Value");
        assert_eq!(input.value, None);
        assert!(input.expression.is_empty());
        assert!(input.dependencies.is_empty());
        assert_eq!(input.properties["value_type"], parameter_type);
        assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new()).is_ok());
    }
}

#[test]
fn decode_does_not_treat_numeric_packets_as_non_numeric_formula_values() {
    for parameter_type in ["Boolean", "String"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                    4,
                    false,
                    &[("#1_", parameter_type, "Value", "#1_ /2", 1.0)],
                    "Real",
                    Some(1.0),
                    "#1_ /2",
                )),
                &DecodeOptions::default(),
            )
            .expect("decode non-numeric formula input with numeric packet");

        assert!(decoded.ir.model.parameters.is_empty());
    }
}

#[test]
fn decode_rejects_nonintegral_integer_formula_input() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "Integer",
                "I",
                3.5,
                4.0,
                "#1_ /2-2mm",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode invalid integer formula");

    assert!(decoded.ir.model.parameters.is_empty());
}

#[test]
fn decode_deduplicates_repeated_single_input_formula_symbols() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "ANGLE",
                "ANGLE",
                0.25,
                0.5,
                "#1_ /2+#1_ /2",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode repeated formula input");
    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("repeated formula input parameters")
    };

    assert_eq!(input.expression, "0.25 rad");
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));
}

#[test]
fn decode_transfers_ordered_multi_input_formula_dependencies() {
    use cadmpeg_ir::features::ParameterValue;

    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                5,
                false,
                &[
                    ("#1_", "Real", "Width", "#1_ /2", 12.0),
                    ("#2_", "Integer", "Count", "#2_ /3", 3.0),
                ],
                "Real",
                Some(15.0),
                "#1_ /2+#2_ /3",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode multi-input formula");
    let [width, count, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("multi-input formula parameters")
    };

    assert_eq!(width.value, Some(ParameterValue::Real(12.0)));
    assert_eq!([width.ordinal, count.ordinal, output.ordinal], [0, 1, 2]);
    assert_eq!(count.value, Some(ParameterValue::Integer(3)));
    assert_eq!(
        output.dependencies,
        [width.id.clone(), count.id.clone()].as_slice()
    );
    assert_eq!(output.value, Some(ParameterValue::Real(15.0)));
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new())
        .findings
        .is_empty());
}

#[test]
fn decode_transfers_a_closed_formula_with_bare_symbols() {
    use cadmpeg_ir::features::{Length, ParameterValue};

    let bytes = standard_catpart_with_typed_formula_inputs(
        4,
        false,
        &[("#1_", "LENGTH", "Thickness", "#1_", 35.0)],
        "LENGTH",
        Some(33.0),
        "#1_-2mm",
    );
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("decode bare-symbol formula");
    let [input, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("closed bare-symbol formula parameters")
    };

    assert_eq!(input.value, Some(ParameterValue::Length(Length(35.0))));
    assert_eq!(output.expression, "#1_-2mm");
    assert_eq!(output.value, Some(ParameterValue::Length(Length(33.0))));
    assert_eq!(output.dependencies, std::slice::from_ref(&input.id));

    let native = crate::native::CatiaNative::decode(&bytes);
    let mut excluded_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::Annotations::default();
    let excluded = crate::formula::transfer_parameters(
        &mut excluded_ir,
        &native,
        &mut annotations,
        Some(&std::collections::HashSet::new()),
    );
    assert!(excluded_ir.model.parameters.is_empty());
    assert!(excluded.consumed_object_records.is_empty());
}

#[test]
fn decode_transfers_each_supported_formula_input_independently() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_inputs(
                6,
                false,
                &[
                    ("#1_", "LENGTH", "Width", "#1_ /2", 12.0),
                    ("#2_", "String", "Label", "#2_ /3", 0.25),
                    ("#3_", "Real", "Depth", "#3_ /4", 6.5),
                ],
                "Real",
                Some(3.0),
                "#1_ /2+#2_ /3+#3_ /4",
            )),
            &DecodeOptions::default(),
        )
        .expect("decode incomplete multi-input formula");

    let [width, depth] = decoded.ir.model.parameters.as_slice() else {
        panic!("independently bound formula inputs")
    };
    assert_eq!(width.name, "Width");
    assert_eq!(
        width.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(12.0)
        ))
    );
    assert!(width.dependencies.is_empty());
    assert_eq!(depth.name, "Depth");
    assert_eq!(
        depth.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(6.5))
    );
    assert!(depth.dependencies.is_empty());
    assert_eq!(
        decoded.report.coverage["transferred_formula_design_record_count"],
        2
    );
    assert_eq!(decoded.report.coverage["unresolved_design_record_count"], 4);
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
            && loss.message.contains("4 modeling-scope field record(s)")
    }));
}

#[test]
fn decode_transfers_a_chained_formula_definition_once() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::Linear,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula chain");
    let [input, intermediate, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("formula chain parameters")
    };

    assert_eq!(intermediate.expression, "#1_ /2+1mm");
    assert_eq!(intermediate.dependencies, std::slice::from_ref(&input.id));
    assert_eq!(output.expression, "#2_ /3+1mm");
    assert_eq!(output.dependencies, std::slice::from_ref(&intermediate.id));
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new())
        .findings
        .is_empty());
}

#[test]
fn decode_rejects_multiple_formula_definitions_for_one_output() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::DuplicateTerminal,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode duplicate formula output");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed formula input")
    };
    assert_eq!(input.name, "Input");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_retains_a_typed_input_with_ambiguous_formula_definitions() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::DuplicateIntermediate,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode ambiguous intermediate formula output");
    let [input, intermediate, output] = decoded.ir.model.parameters.as_slice() else {
        panic!("scalar fallback and downstream formula parameters")
    };

    assert_eq!(input.name, "Input");
    assert_eq!(intermediate.name, "Intermediate");
    assert_eq!(intermediate.expression, "2 mm");
    assert_eq!(
        intermediate.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(2.0)
        ))
    );
    assert!(intermediate.dependencies.is_empty());
    assert_eq!(output.expression, "#2_ /3+1mm");
    assert_eq!(output.dependencies, std::slice::from_ref(&intermediate.id));
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new())
        .findings
        .is_empty());
}

#[test]
fn decode_rejects_an_incompatible_downstream_formula_without_erasing_its_input() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::IncompatibleDownstream,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode incompatible downstream formula");
    let [input, intermediate] = decoded.ir.model.parameters.as_slice() else {
        panic!("upstream formula parameters")
    };

    assert_eq!(input.name, "Input");
    assert_eq!(intermediate.name, "Intermediate");
    assert_eq!(intermediate.expression, "#1_ /2+1mm");
    assert_eq!(intermediate.dependencies, std::slice::from_ref(&input.id));
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new())
        .findings
        .is_empty());
}

#[test]
fn decode_does_not_infer_a_fallback_from_conflicting_formula_input_types() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::AmbiguousIntermediateWithIncompatibleDownstream,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode conflicting formula input types");
    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the unambiguous scalar root")
    };

    assert_eq!(input.name, "Input");
    assert!(input.dependencies.is_empty());
    assert!(cadmpeg_ir::validate::validate(&decoded.ir, Vec::new())
        .findings
        .is_empty());
}

#[test]
fn decode_rejects_a_cyclic_formula_component() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_chain(
                FormulaChainCase::Cyclic,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode cyclic formula component");

    assert!(decoded.ir.model.parameters.is_empty());
}

#[test]
fn decode_rejects_a_formula_exceeding_the_expression_depth_limit() {
    let boundary_expression = format!("{}#1_ /2", "+".repeat(128));
    let boundary = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "LENGTH",
                "LENGTH",
                12.0,
                12.0,
                &boundary_expression,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode formula at depth limit");
    assert_eq!(boundary.ir.model.parameters.len(), 2);

    let expression = format!("{}#1_ /2", "+".repeat(129));
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_typed_formula_relation(
                4,
                false,
                "LENGTH",
                "LENGTH",
                12.0,
                12.0,
                &expression,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode depth-limited formula");

    let [input] = decoded.ir.model.parameters.as_slice() else {
        panic!("only the independently typed input")
    };
    assert_eq!(input.name, "Thickness");
    assert!(input.dependencies.is_empty());
}

#[test]
fn decode_rejects_a_formula_with_ambiguous_input_binding() {
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_formula_relation(5, true)),
            &DecodeOptions::default(),
        )
        .expect("decode ambiguous formula");

    assert!(decoded.ir.model.parameters.is_empty());
    assert_eq!(
        decoded.report.coverage["decoded_formula_parameter_dependency_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["decoded_resolved_formula_parameter_dependency_count"],
        0
    );
    assert_eq!(
        decoded.report.coverage["unresolved_formula_parameter_dependency_count"],
        1
    );
    assert_eq!(
        decoded.report.coverage["ambiguous_formula_parameter_dependency_count"],
        1
    );
}

#[test]
fn entity_value_schema_selection_excludes_a_packet_crossing_its_boundary() {
    let native =
        crate::native::CatiaNative::decode(&standard_catpart_with_crossing_entity_value_packet());
    assert_eq!(native.entity_records[0].value_packets.len(), 1);
    assert_eq!(native.entity_records[0].value_schema_selections.len(), 2);
    assert!(native.entity_records[0]
        .value_schema_selections
        .iter()
        .all(|selection| selection.packets.is_empty()));

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store crossing packet fixture");
    crate::native::CatiaNative::load(&namespace).expect("validate canonical packet ownership");
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
fn native_migrates_and_validates_evaluated_value_names() {
    let mut bytes = Vec::new();
    bytes.push(0xea);
    bytes.extend(1_u32.to_le_bytes());
    bytes.push(0x81);
    bytes.extend([0x58, 0xd1, 8]);
    bytes.extend(b"\xe8\x00\x12\x01\x06Count\xfe");
    bytes.extend([0x5f, 0xd1, 9]);
    bytes.extend(b"\xe8\xc4\x17\x01\xfe\xfe");
    bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");

    let native = crate::native::CatiaNative::decode(&bytes);
    let value = &native.legacy_entity_runs[0].integer_values[0];
    assert_eq!(value.name.as_deref(), Some("Count"));

    let mut invalid = native.clone();
    invalid.legacy_entity_runs[0].integer_values[0].name = None;
    invalid.legacy_entity_runs[0].integer_values[0].name_field = None;
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut invalid_namespace)
        .expect("store noncanonical evaluated value name");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let mut previous_namespace = invalid_namespace;
    previous_namespace.version = 223;
    let migrated = crate::native::CatiaNative::load(&previous_namespace)
        .expect("migrate evaluated value name");
    assert_eq!(
        migrated.legacy_entity_runs[0].integer_values[0]
            .name
            .as_deref(),
        Some("Count")
    );
}

#[test]
fn native_round_trips_legacy_entity_identity_runs() {
    let mut bytes = Vec::new();
    for entity_id in [1_u32, 4, 9, 12, 13] {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([0x81, 0xfd, 0x8c]);
        if entity_id == 4 {
            for (role, selector, value) in [
                ("body", vec![0x80, 4, 0, 0, 0], "#1_ + 2"),
                ("param", vec![0xd1, 8], "(#1_ : #In Real) : Real\n"),
            ] {
                bytes.push(u8::try_from(role.len() + 1).expect("short role"));
                bytes.extend(role.as_bytes());
                bytes.extend(selector);
                bytes.extend(b"\xe8\x00\x12\x01");
                bytes.push(u8::try_from(value.len() + 1).expect("short text"));
                bytes.extend(value.as_bytes());
                bytes.push(0xfe);
            }
        } else if entity_id == 9 {
            bytes.extend([8, b'p', b'a', b'r', b'a', b'm', b'i', b'n', 0x80]);
            bytes.extend(4134_u32.to_le_bytes());
            bytes.extend([0xe8, 0xe4, 0x0b, 0x01]);
            bytes.extend(b"\xfe\x84\x92\x82\x08Boolean\x83");
            bytes.extend(b"\xfe\x84\x92\x82\x96\x83");
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 9]);
            bytes.extend(b"\xe8\x00\x12\x01\x07Result\xfe");
            bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
            bytes.extend(3.5_f64.to_bits().to_le_bytes());
        } else if entity_id == 12 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 11]);
            bytes.extend(b"\xe8\x00\x12\x01\x0cResponsible\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x07String\x83");
            bytes.extend(b"\xfe\x85\x93\x82\xfe\x0cCilas Evans");
        } else if entity_id == 13 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 12]);
            bytes.extend(b"\xe8\x00\x12\x01\x06Count\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x08Integer\x83");
            bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
        }
    }
    let catalog_offset = bytes.len();
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    bytes.extend(b"\xfe\xfe\xfe");
    let schema_program_offset = bytes.len();
    bytes.extend([0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);
    let schema_footer_offset = bytes.len();
    bytes.extend(b"\x4e\x11\x00\x00\x00DASSAULT-SYSTEMES\x05\x00\x00\x00CATIA");

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.legacy_entity_runs.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0]
            .identities
            .iter()
            .map(|identity| identity.entity_id)
            .collect::<Vec<_>>(),
        [1, 4, 9, 12, 13]
    );
    assert!(native.legacy_entity_runs[0]
        .identities
        .iter()
        .all(|identity| identity.lead == 0x81));
    assert_eq!(
        native.legacy_entity_runs[0].catalog_offset,
        catalog_offset as u64
    );
    let schema_program = native.legacy_entity_runs[0]
        .schema_program
        .as_ref()
        .expect("complete compact schema program");
    assert_eq!(schema_program.byte_offset, schema_program_offset as u64);
    assert_eq!(
        schema_program.boundary_byte_offset,
        schema_footer_offset as u64
    );
    assert_eq!(
        schema_program.boundary,
        crate::native::CatiaLegacySchemaProgramBoundary::VendorFooter
    );
    assert_eq!(
        schema_program.data,
        [0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]
    );
    assert_eq!(schema_program.identifiers.len(), 1);
    assert_eq!(
        schema_program.identifiers[0].byte_offset,
        schema_program_offset as u64 + 1
    );
    assert_eq!(schema_program.identifiers[0].value, "Foo");
    assert_eq!(native.legacy_entity_runs[0].text_fields.len(), 5);
    assert_eq!(
        native.legacy_entity_runs[0]
            .role_selectors
            .iter()
            .map(|role| {
                (
                    role.entity_id,
                    role.name.literal().expect("literal role"),
                    role.encoding,
                    role.selector,
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                4,
                "body",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::FixedU32),
                4,
            ),
            (
                4,
                "param",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                9,
            ),
            (
                9,
                "paramin",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::FixedU32),
                4134,
            ),
            (
                9,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                10,
            ),
            (
                12,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                12,
            ),
            (
                13,
                "name",
                Some(crate::native::CatiaLegacyRoleSelectorEncoding::Paged),
                13,
            ),
        ]
    );
    assert_eq!(native.legacy_entity_runs[0].text_fields[0].entity_id, 4);
    assert_eq!(native.legacy_entity_runs[0].text_fields[0].value, "#1_ + 2");
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[0]
            .role
            .as_ref()
            .map(|role| { (role.name.literal().expect("literal role"), role.selector,) }),
        Some(("body", 4))
    );
    assert_eq!(
        native.legacy_entity_runs[0].text_fields[1]
            .role
            .as_ref()
            .map(|role| { (role.name.literal().expect("literal role"), role.selector,) }),
        Some(("param", 9))
    );
    assert_eq!(native.legacy_entity_runs[0].relations.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].parameter_entity_id,
        Some(9)
    );
    assert_eq!(
        native.legacy_entity_runs[0].relations[0].inputs[0].parameter,
        "#1_"
    );
    assert_eq!(native.legacy_entity_runs[0].type_descriptors.len(), 4);
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[0].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "Boolean".to_string()
        }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[1].value,
        crate::native::CatiaLegacyTypeValue::Selector { value: 22 }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[2].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "String".to_string()
        }
    );
    assert_eq!(
        native.legacy_entity_runs[0].type_descriptors[3].value,
        crate::native::CatiaLegacyTypeValue::Name {
            value: "Integer".to_string()
        }
    );
    assert_eq!(native.legacy_entity_runs[0].scalar_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].scalar_values[0]
            .name
            .as_deref(),
        Some("Result")
    );
    assert_eq!(
        native.legacy_entity_runs[0].scalar_values[0].encoding,
        crate::native::CatiaLegacyScalarEncoding::Named84
    );
    assert!(native.legacy_entity_runs[0].scalar_values[0]
        .id
        .starts_with("catia:legacy:scalar#00000000-"));
    assert!(matches!(
        native.legacy_entity_runs[0].scalar_values[0].evaluation,
        crate::native::CatiaLegacyScalarEvaluation::Value { bits }
            if bits == 3.5_f64.to_bits()
    ));
    assert_eq!(native.legacy_entity_runs[0].string_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].string_values[0]
            .name
            .as_deref(),
        Some("Responsible")
    );
    assert_eq!(
        native.legacy_entity_runs[0].string_values[0].value,
        "Cilas Evans"
    );
    assert_eq!(native.legacy_entity_runs[0].integer_values.len(), 1);
    assert_eq!(
        native.legacy_entity_runs[0].integer_values[0]
            .name
            .as_deref(),
        Some("Count")
    );
    assert_eq!(native.legacy_entity_runs[0].integer_values[0].value, 11);

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store legacy entity run");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load legacy entity run");
    assert_eq!(loaded.legacy_entity_runs, native.legacy_entity_runs);

    let mut previous_schema_namespace = namespace.clone();
    let mut previous_schema_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_schema_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous schema-program runs");
    previous_schema_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .identifiers
        .clear();
    previous_schema_namespace
        .set_arena("legacy_entity_runs", &previous_schema_runs)
        .expect("store previous schema-program runs");
    previous_schema_namespace.version = 221;
    let migrated_schema = crate::native::CatiaNative::load(&previous_schema_namespace)
        .expect("migrate schema identifiers");
    assert_eq!(
        migrated_schema.legacy_entity_runs[0]
            .schema_program
            .as_ref()
            .expect("migrated schema program")
            .identifiers,
        schema_program.identifiers
    );

    let mut previous_boundary_namespace = namespace.clone();
    let mut previous_boundary_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_boundary_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous schema-program boundary");
    previous_boundary_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .boundary = crate::native::CatiaLegacySchemaProgramBoundary::StreamDirectory;
    previous_boundary_namespace
        .set_arena("legacy_entity_runs", &previous_boundary_runs)
        .expect("store previous schema-program boundary");
    previous_boundary_namespace.version = 222;
    let migrated_boundary = crate::native::CatiaNative::load(&previous_boundary_namespace)
        .expect("migrate schema-program boundary");
    assert_eq!(
        migrated_boundary.legacy_entity_runs[0]
            .schema_program
            .as_ref()
            .expect("migrated schema program")
            .boundary,
        crate::native::CatiaLegacySchemaProgramBoundary::VendorFooter
    );

    let mut invalid_schema_program = native.clone();
    invalid_schema_program.legacy_entity_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .data
        .pop();
    let mut invalid_schema_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_schema_program
        .store(&mut invalid_schema_namespace)
        .expect("store invalid schema program");
    assert!(crate::native::CatiaNative::load(&invalid_schema_namespace).is_err());

    let mut invalid_schema_identifier = native.clone();
    invalid_schema_identifier.legacy_entity_runs[0]
        .schema_program
        .as_mut()
        .expect("schema program")
        .identifiers[0]
        .value = "Bar".to_string();
    let mut invalid_identifier_namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_schema_identifier
        .store(&mut invalid_identifier_namespace)
        .expect("store invalid schema identifier");
    assert!(crate::native::CatiaNative::load(&invalid_identifier_namespace).is_err());

    let mut previous_field_namespace = namespace.clone();
    let mut previous_field_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_field_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous field-binding runs");
    for run in &mut previous_field_runs {
        for role in &mut run.role_selectors {
            role.field_code = None;
        }
        for role in run
            .text_fields
            .iter_mut()
            .filter_map(|field| field.role.as_mut())
        {
            role.field_code = None;
        }
    }
    previous_field_namespace
        .set_arena("legacy_entity_runs", &previous_field_runs)
        .expect("store previous field-binding runs");
    previous_field_namespace.version = 219;
    let migrated_field_bindings = crate::native::CatiaNative::load(&previous_field_namespace)
        .expect("load previous field bindings");
    assert!(migrated_field_bindings.legacy_entity_runs[0]
        .role_selectors
        .iter()
        .all(|role| role.field_code.is_none()));

    let mut previous_identity_namespace = namespace.clone();
    let mut previous_identity_runs: Vec<crate::native::CatiaLegacyEntityRun> =
        previous_identity_namespace
            .arena_as("legacy_entity_runs")
            .expect("load previous identity runs");
    for identity in previous_identity_runs
        .iter_mut()
        .flat_map(|run| &mut run.identities)
    {
        identity.lead = 0;
    }
    previous_identity_namespace
        .set_arena("legacy_entity_runs", &previous_identity_runs)
        .expect("store previous identity runs");
    previous_identity_namespace.version = 215;
    let migrated_identity = crate::native::CatiaNative::load(&previous_identity_namespace)
        .expect("migrate legacy identity leads");
    assert!(migrated_identity.legacy_entity_runs[0]
        .identities
        .iter()
        .all(|identity| identity.lead == 0x81));

    let mut previous_namespace = namespace.clone();
    let mut previous_runs: Vec<crate::native::CatiaLegacyEntityRun> = previous_namespace
        .arena_as("legacy_entity_runs")
        .expect("load legacy entity runs");
    previous_runs[0].role_selectors.clear();
    previous_runs[0].schema_fields.clear();
    for field in &mut previous_runs[0].text_fields {
        if let Some(role) = &mut field.role {
            role.entity_id = 0;
        }
    }
    previous_namespace
        .set_arena("legacy_entity_runs", &previous_runs)
        .expect("store previous legacy entity runs");
    previous_namespace.version = 211;
    let migrated =
        crate::native::CatiaNative::load(&previous_namespace).expect("migrate legacy text roles");
    assert_eq!(migrated.legacy_entity_runs[0].role_selectors.len(), 5);
    assert!(migrated.legacy_entity_runs[0]
        .role_selectors
        .iter()
        .all(|role| role.entity_id != 0));

    let mut invalid_type_name = native.clone();
    invalid_type_name.legacy_entity_runs[0].type_descriptors[0].value =
        crate::native::CatiaLegacyTypeValue::Name {
            value: "1Boolean".to_string(),
        };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_type_name
        .store(&mut namespace)
        .expect("store invalid legacy type name");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_lead = native.clone();
    invalid_lead.legacy_entity_runs[0].identities[0].lead = 0xe6;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_lead
        .store(&mut namespace)
        .expect("store invalid legacy identity lead");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_name = native.clone();
    invalid_name.legacy_entity_runs[0].scalar_values[0].name = Some("Other".to_string());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_name
        .store(&mut namespace)
        .expect("store invalid legacy scalar name");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_scalar_id = native.clone();
    invalid_scalar_id.legacy_entity_runs[0].scalar_values[0].id =
        "catia:legacy:scalar#00000000-0".to_string();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_scalar_id
        .store(&mut namespace)
        .expect("store invalid legacy scalar identity");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_integer = native.clone();
    invalid_integer.legacy_entity_runs[0].integer_values[0].value = -1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_integer
        .store(&mut namespace)
        .expect("store invalid inline legacy integer");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid_parameter = native.clone();
    invalid_parameter.legacy_entity_runs[0].relations[0].parameter_entity_id = Some(4);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid_parameter
        .store(&mut namespace)
        .expect("store invalid legacy relation parameter");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());

    let mut invalid = native;
    invalid.legacy_entity_runs[0].identities[1].entity_id = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid legacy entity run");
    assert!(crate::native::CatiaNative::load(&namespace).is_err());
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
fn object_graphs_retain_exact_finjpl_containment() {
    let preamble_graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x81, 0x81], &[0xfe])]);
    let segment_graph =
        object_graph_from_records(&[object_graph_record(&[0x04, 0x01, 0x82, 0x82], &[0xfe])]);
    let mut bytes = preamble_graph;
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0101_0001u32.to_be_bytes());
    bytes.extend_from_slice(&segment_graph);

    let native = crate::native::CatiaNative::decode(&bytes);

    assert_eq!(native.object_graphs.len(), 2);
    assert_eq!(native.object_graphs[0].finjpl_segment, None);
    assert_eq!(
        native.object_graphs[1].finjpl_segment.as_deref(),
        Some(native.finjpl_segments[0].id.as_str())
    );

    let mut invalid = native;
    invalid.object_graphs[1].finjpl_segment = None;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store malformed graph segment link");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}

#[test]
fn object_graphs_retain_exact_outer_container_declarations() {
    let (bytes, graph_offset) = outer_container_object_graph_catpart();

    let native = crate::native::CatiaNative::decode(&bytes);
    let graph = native
        .object_graphs
        .iter()
        .find(|graph| graph.byte_offset == graph_offset)
        .expect("declared-stream object graph");
    let container = graph
        .outer_container
        .as_ref()
        .expect("outer container binding");
    assert_eq!(container.data_offset, 0);
    assert_eq!(container.ordinal, 2);
    assert_eq!(container.class_name, "CATPrtCont");
    assert_eq!(container.base_class, "CATProdCont");
    assert_eq!(container.stream_name, "1048_62eb7b6f_1825");
    let expected = container.clone();

    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store outer container binding");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load outer container binding");
    assert_eq!(
        loaded
            .object_graphs
            .iter()
            .find(|graph| graph.byte_offset == graph_offset)
            .and_then(|graph| graph.outer_container.as_ref()),
        Some(&expected)
    );
}

#[test]
fn legacy_parameters_retain_and_require_the_part_container_binding() {
    let graph = object_graph_stream();
    let legacy_offset = graph.len();
    let mut stream = graph;
    stream.push(0xea);
    stream.extend(1_u32.to_le_bytes());
    stream.push(0x81);
    stream.extend([0xfd, 0x8c]);
    stream.extend([5, b'n', b'a', b'm', b'e', 0xd1, 8]);
    stream.extend(b"\xe8\x00\x12\x01");
    stream.extend([6, b'W', b'i', b'd', b't', b'h', 0xfe]);
    stream.extend(b"\xfe\x84\x92\x82");
    stream.extend([7, b'L', b'E', b'N', b'G', b'T', b'H', 0x83]);
    stream.extend(b"\xfe\x84\x88\x82\xfe\xe6");
    stream.extend(12.5_f64.to_bits().to_le_bytes());
    stream.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    let (bytes, stream_offset) = outer_container_catpart(&stream);

    let native = crate::native::CatiaNative::decode(&bytes);
    let run = native
        .legacy_entity_runs
        .iter()
        .find(|run| run.byte_offset == stream_offset + legacy_offset as u64)
        .expect("declared-stream legacy run");
    assert_eq!(
        run.outer_container.as_ref(),
        native.object_graphs[0].outer_container.as_ref()
    );
    let expected_binding = run.outer_container.clone();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store container-bound legacy run");
    let loaded =
        crate::native::CatiaNative::load(&namespace).expect("load container-bound legacy run");
    assert_eq!(
        loaded.legacy_entity_runs[0].outer_container,
        expected_binding
    );

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode container-bound legacy parameter");
    assert_eq!(
        decoded.report.coverage["transferred_legacy_parameter_count"],
        1
    );
    assert_eq!(decoded.ir.model.parameters.len(), 1);
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
fn decode_does_not_promote_operation_field_class_names_to_features() {
    for class in ["Groove", "GSMHelix"] {
        let decoded = CatiaCodec
            .decode(
                &mut Cursor::new(standard_catpart_with_design_class(class)),
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
            ["CurrentFeature", class]
        );
        assert!(decoded.report.losses.iter().any(|loss| {
            loss.category == cadmpeg_ir::report::LossCategory::DesignIntent
                && loss.message.contains("neutral features")
        }));
    }
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
fn outer_alias_parser_closes_group_header_and_overlapping_target_slot() {
    let mut bytes = vec![0x02, 0x00];
    bytes.extend_from_slice(&0xafu32.to_le_bytes());
    bytes.extend_from_slice(&0x148u32.to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
    let mut alias = surface_alias_stream();
    alias[15..19].copy_from_slice(&0x0000_017bu32.to_le_bytes());
    bytes.extend(alias);

    let [row] = crate::object_graph::surface_aliases(&bytes)
        .try_into()
        .expect("one grouped alias row");
    let group = row.group.expect("exact group header");
    assert_eq!(group.prototype, 0xaf);
    assert_eq!(group.group_id, 0x148);
    assert_eq!(group.target_slot, 0x17b);
    assert_eq!(group.storage_prefix, [0x01, 0x00, 0x00, 0x00]);
    assert_eq!(row.entity_record_ordinal, 0x7b);

    bytes[10] = 1;
    let [row] = crate::object_graph::surface_aliases(&bytes)
        .try_into()
        .expect("one ungrouped alias row");
    assert!(row.group.is_none());
}

#[test]
fn outer_alias_group_parser_accepts_each_bounded_storage_prefix() {
    for storage in [
        &[0x00, 0x00, 0x00][..],
        &[0x01, 0x00, 0x00, 0x00],
        &[0x01, 0x01, 0x00, 0x7c, 0x02, 0x00, 0x00],
        &[0x01, 0x00, 0x01, 0x00, 0x7c, 0x02, 0x00, 0x00],
    ] {
        let mut bytes = vec![0x02, 0x00];
        bytes.extend_from_slice(&0xafu32.to_le_bytes());
        bytes.extend_from_slice(&0x147u32.to_le_bytes());
        bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
        bytes.extend_from_slice(storage);
        let mut alias = surface_alias_stream();
        alias.drain(..4);
        alias[11..15].copy_from_slice(&0x0000_017du32.to_le_bytes());
        bytes.extend(alias);

        let [row] = crate::object_graph::surface_aliases(&bytes)
            .try_into()
            .expect("one grouped alias row");
        let group = row.group.expect("bounded group storage");
        assert_eq!(group.storage_prefix, storage);
        assert_eq!(group.target_slot, 0x17d);
    }
}

#[test]
fn native_namespace_retains_and_validates_alias_group_membership() {
    let mut bytes = vec![0x02, 0x00];
    bytes.extend_from_slice(&0xafu32.to_le_bytes());
    bytes.extend_from_slice(&0x148u32.to_le_bytes());
    bytes.extend_from_slice(&[0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x30, 0x00, 0x00]);
    let mut alias = surface_alias_stream();
    alias[15..19].copy_from_slice(&0x0000_017bu32.to_le_bytes());
    bytes.extend(alias);

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.alias_rows[0]
            .group
            .as_ref()
            .expect("group membership")
            .target_slot,
        0x17b
    );
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(&mut namespace)
        .expect("store grouped alias row");
    let loaded = crate::native::CatiaNative::load(&namespace).expect("load grouped alias row");
    assert_eq!(loaded, native);

    let mut invalid = native;
    invalid.alias_rows[0]
        .group
        .as_mut()
        .expect("group membership")
        .target_slot += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid grouped alias row");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut invalid = loaded;
    invalid.alias_rows[0]
        .group
        .as_mut()
        .expect("group membership")
        .storage_prefix = vec![2, 0, 0];
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    invalid
        .store(&mut namespace)
        .expect("store invalid group storage");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
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
    assert!(row.group.is_none());

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
    let mut stream = b5_closed_triangle_stream();
    append_b5_record(
        &mut stream,
        0x5e,
        900,
        &[
            0x85, 0x81, 0x18, 0x85, 0x03, 0x18, 0x85, 0x03, 0x81, 0x81, 0x2a,
        ],
    );
    append_b5_record(&mut stream, 0x5d, 901, &[0x81, 0x81, 0x04]);
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
    assert_eq!(
        result.report.coverage["resolved_object_stream_face_terminal_control_03_count"],
        0
    );
    assert_eq!(
        result.report.coverage["resolved_object_stream_face_terminal_control_05_count"],
        1
    );
    assert_eq!(
        result.report.coverage["resolved_object_stream_uncounted_face_count"],
        0
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_edge_terminal_control_2a_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_vertex_incidence_terminal_control_04_count"],
        1
    );
    assert_eq!(
        result.report.coverage["resolved_object_stream_loop_framing_controls_05_05_count"],
        1
    );
    assert_eq!(
        result.report.coverage["resolved_object_stream_extended_loop_metadata_count"],
        0
    );
    assert_eq!(
        result.report.coverage["resolved_object_stream_class_21_pcurve_suffix_scalar_count"],
        3
    );
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
fn decode_reports_structurally_typed_unresolved_b5_faces() {
    let mut stream = b5_closed_triangle_stream();
    append_b5_record(
        &mut stream,
        0x5f,
        902,
        &[0x82, 0x18, 100, 0, 0x18, 0xe7, 0x03, 0x03],
    );
    append_b5_record(&mut stream, 0x5e, 903, &[]);
    let graph = crate::families::b5::graph::parse(&stream).expect("typed unresolved face graph");
    assert_eq!(graph.face_records.len(), 2);
    assert_eq!(graph.faces.len(), 1);
    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode typed unresolved face");

    assert_eq!(
        result.report.coverage["typed_object_stream_face_terminal_control_03_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_face_terminal_control_05_count"],
        1
    );
    assert_eq!(
        result.report.coverage["resolved_object_stream_face_terminal_control_03_count"],
        0
    );
    assert_eq!(
        result.report.coverage["typed_unresolved_object_stream_face_count"],
        1
    );
}

#[test]
fn decode_reports_typed_b5_faces_without_a_resolved_topology_graph() {
    let mut stream = b2_sphere_stream();
    append_b5_record(&mut stream, 0x27, 100, &b5_plane_payload([0.0; 3]));
    append_b5_record(
        &mut stream,
        0x21,
        9,
        &b5_linear_pcurve_payload(100, [0.0, 0.0], [1.0, 0.0]),
    );
    append_b5_record(
        &mut stream,
        0x62,
        103,
        &[
            0x83, 0x89, 0x8a, 0xe4, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
            0x01,
        ],
    );
    append_b5_record(
        &mut stream,
        0x5f,
        101,
        &[0x82, 0x18, 100, 0, 0x18, 102, 0, 0x03],
    );
    append_b5_record(&mut stream, 0x5e, 102, &[]);
    append_b5_record(
        &mut stream,
        0x5e,
        104,
        &[0x85, 0x81, 0xe9, 0x83, 0x84, 0x85, 0x21],
    );
    append_b5_record(&mut stream, 0x5d, 105, &[0x81, 0x86, 0x04]);
    let mut incidence_payload = vec![0x81, 0x89, 0x81];
    incidence_payload.extend_from_slice(&le_f64(0.0));
    incidence_payload.push(0x81);
    append_b5_record(&mut stream, 0x06, 4, &incidence_payload);
    append_b5_record(&mut stream, 0x05, 6, &[0x81, 0x84]);
    assert!(crate::families::b5::graph::parse(&stream).is_none());
    assert_eq!(
        crate::families::b5::graph::typed_face_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_loop_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_edge_records(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_vertex_incidence_links(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_class_21_pcurves(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_parameter_incidences(&stream).len(),
        1
    );
    assert_eq!(
        crate::families::b5::graph::typed_vertex_incidence_rosters(&stream).len(),
        1
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode typed face without resolved topology");
    assert_eq!(
        result.report.coverage["typed_object_stream_face_terminal_control_03_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_unresolved_object_stream_face_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_loop_framing_controls_05_05_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_unresolved_object_stream_loop_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_edge_terminal_control_21_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_vertex_incidence_terminal_control_04_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_class_21_pcurve_suffix_scalar_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_parameter_incidence_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_parameter_incidence_member_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_vertex_incidence_roster_count"],
        1
    );
    assert_eq!(
        result.report.coverage["typed_object_stream_vertex_incidence_roster_member_count"],
        1
    );
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
    assert_eq!(
        result.ir.model.loops[0].boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
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
