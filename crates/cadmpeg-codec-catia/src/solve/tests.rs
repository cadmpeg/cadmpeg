// SPDX-License-Identifier: Apache-2.0
//! Trim-mesh port, coverage, and quotient tests over synthetic topology streams.

#![allow(clippy::unwrap_used)]

use crate::test_support::{
    compact_standard_triangle_topology_stream, standard_quad_topology_stream,
};

#[test]
fn compact_standard_ports_reuse_handles_within_the_table_scope() {
    let ports = crate::solve::missing_edge::standard_edge_port_identities(
        &compact_standard_triangle_topology_stream(),
    )
    .expect("compact standard ports");

    assert_eq!(ports, vec![[0, 1], [1, 2], [2, 0]]);
}

#[test]
fn standard_full_table_scopes_complete_rows_and_isolates_interior_rows() {
    let mut bytes = vec![0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2];
    bytes.extend_from_slice(&[0x01, 0x01, 0x03]);
    for handles in [&[10u16, 11][..], &[11, 12, 13][..], &[11, 12][..]] {
        bytes.extend_from_slice(&[0x02, handles.len() as u8]);
        for handle in handles {
            bytes.extend_from_slice(&handle.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&[
        0x10, 0x24, 0x04, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01, 0x06, 0x00,
    ]);

    let ports = crate::solve::missing_edge::standard_edge_port_identities(&bytes)
        .expect("standard full-table ports");
    assert_eq!(ports, vec![[0, 1], [2, 3], [1, 4]]);
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
fn unmatched_fbb_complete_row_arity_fixes_trim_span() {
    let mut bytes = crate::test_support::fbb_only_quad_topology_stream();
    let first_row = bytes
        .windows(5)
        .position(|window| window == [0x01, 0x01, 0x02, 0x02, 0x03])
        .expect("first FBB edge table");
    let second_row = first_row + 8;
    for (offset, handle) in (first_row + 5..first_row + 11)
        .chain(second_row + 5..second_row + 11)
        .zip([0u8, 20, 0, 21, 0, 22, 0, 30, 0, 31, 0, 32])
    {
        bytes[offset] = handle;
    }
    let coverage = crate::solve::missing_edge::standard_mesh_face_coverage(&bytes, &[[0, 0]; 4])
        .expect("unmatched FBB row coverage");
    assert_eq!(coverage[0].missing_edges, [0, 1]);
    assert_eq!(coverage[0].gaps[0].length, 4);

    let assignments = crate::solve::missing_edge::standard_mesh_missing_edge_assignments(
        &bytes,
        &[[0, 0]; 4],
        None,
        false,
    )
    .expect("complete FBB row spans");
    assert_eq!(assignments[0].len(), 2);
    assert!(assignments[0].iter().all(|assignment| {
        assignment
            .iter()
            .map(|placement| placement.segment_count)
            .collect::<Vec<_>>()
            == [2, 2]
    }));
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
fn standard_mesh_gap_assignment_uses_compact_endpoint_identity() {
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
    assert_eq!(assignments[0].len(), 280);
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
