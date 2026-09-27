// SPDX-License-Identifier: Apache-2.0
//! Trim-mesh port, coverage, and quotient tests over synthetic topology streams.

#![allow(clippy::unwrap_used)]

use crate::test_support::test_topology::{
    compact_standard_triangle_topology_stream, standard_quad_topology_stream,
};

fn mesh_coverage_limit_operation(max_collection_items: u64) -> Option<&'static str> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let result = crate::solve::missing_edge::standard_mesh_face_coverage(
        &ctx,
        &standard_quad_topology_stream(),
        &[[0, 0]; 4],
    );
    match result {
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems =>
        {
            Some(limit.operation)
        }
        Ok(_) => None,
        other => panic!("expected collection resource limit, got {other:?}"),
    }
}

fn mesh_coverage_limit_operations() -> &'static std::collections::HashSet<&'static str> {
    static OPERATIONS: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let mut operations = std::collections::HashSet::new();
        for limit in 0..=1024 {
            if let Some(operation) = mesh_coverage_limit_operation(limit) {
                operations.insert(operation);
            }
        }
        operations
    })
}

#[test]
fn mesh_cycle_occurrences_propagate_collection_refusal() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_cycle_occurrences"));
}

#[test]
fn mesh_face_edges_propagate_collection_refusal() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_face_edges"));
}

#[test]
fn mesh_edges_by_face_propagate_collection_refusal() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_edges_by_face"));
}

#[test]
fn mesh_cycle_coverage_propagates_collection_refusal() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_cycle_coverage"));
}

#[test]
fn mesh_occurrence_face_rows_refuse_collection_limit() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_occurrence_faces"));
}

#[test]
fn mesh_cycle_occurrence_entries_refuse_collection_limit() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_cycle_occurrence_entries"));
}

#[test]
fn mesh_present_face_edges_refuse_collection_limit() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_present_face_edges"));
}

#[test]
fn mesh_face_edge_entries_refuse_collection_limit() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_face_edge_entries"));
}

#[test]
fn mesh_coverage_faces_refuse_collection_limit() {
    assert!(mesh_coverage_limit_operations().contains("catia_mesh_coverage_faces"));
}

fn one_gap_topology_stream() -> Vec<u8> {
    let mut bytes = standard_quad_topology_stream();
    let header = bytes
        .windows(3)
        .position(|window| window == [0x01, 0x01, 0x04])
        .expect("edge table header");
    let first_row = header + 3;
    bytes[first_row + 1] = 2;
    bytes.drain(first_row + 4..first_row + 6);
    bytes
}

fn mesh_gap_coverage_limit_operation(max_collection_items: u64) -> Option<&'static str> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    match crate::solve::missing_edge::standard_mesh_face_coverage(
        &ctx,
        &one_gap_topology_stream(),
        &[[0, 0]; 4],
    ) {
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems =>
        {
            Some(limit.operation)
        }
        Ok(_) => None,
        other => panic!("expected collection resource limit, got {other:?}"),
    }
}

fn mesh_gap_coverage_limit_operations() -> &'static std::collections::HashSet<&'static str> {
    static OPERATIONS: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let mut operations = std::collections::HashSet::new();
        for limit in 0..=1024 {
            if let Some(operation) = mesh_gap_coverage_limit_operation(limit) {
                operations.insert(operation);
            }
        }
        operations
    })
}

#[test]
fn mesh_coverage_gaps_refuse_collection_limit() {
    assert!(mesh_gap_coverage_limit_operations().contains("catia_mesh_coverage_gaps"));
}

#[test]
fn mesh_coverage_missing_edges_refuse_collection_limit() {
    assert!(mesh_gap_coverage_limit_operations().contains("catia_mesh_coverage_missing_edges"));
}

fn mesh_boundary_domain_limit_operation(max_collection_items: u64) -> Option<&'static str> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = standard_quad_topology_stream();
    let context = crate::test_support::with_service_context(|ctx| {
        crate::solve::missing_edge::StandardMeshBoundaryContext::parse(ctx, &bytes, &[[0, 0]; 4])
            .expect("service resource budget")
            .expect("quad boundary context")
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let result = crate::solve::missing_edge::standard_mesh_boundary_domains_from_context(
        &ctx, &context, None, false,
    );
    match result {
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems =>
        {
            Some(limit.operation)
        }
        Ok(_) => None,
        other => panic!("expected collection resource limit, got {other:?}"),
    }
}

fn mesh_boundary_domain_limit_operations() -> &'static std::collections::HashSet<&'static str> {
    static OPERATIONS: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let mut operations = std::collections::HashSet::new();
        for limit in 0..=1024 {
            if let Some(operation) = mesh_boundary_domain_limit_operation(limit) {
                operations.insert(operation);
            }
        }
        operations
    })
}

#[test]
fn mesh_ordered_boundaries_propagate_collection_refusal() {
    assert!(mesh_boundary_domain_limit_operations().contains("catia_mesh_ordered_boundaries"));
}

#[test]
fn mesh_boundary_coverage_propagates_collection_refusal() {
    assert!(mesh_boundary_domain_limit_operations().contains("catia_mesh_boundary_coverage"));
}

#[test]
fn mesh_ordered_boundary_entries_refuse_collection_limit() {
    assert!(mesh_boundary_domain_limit_operations().contains("catia_mesh_ordered_boundary_entries"));
}

#[test]
fn mesh_completed_boundary_entries_refuse_collection_limit() {
    assert!(
        mesh_boundary_domain_limit_operations().contains("catia_mesh_completed_boundary_entries")
    );
}

#[test]
fn mesh_completed_boundaries_refuse_collection_limit() {
    assert!(mesh_boundary_domain_limit_operations().contains("catia_mesh_completed_boundaries"));
}

#[test]
fn mesh_ordered_assignments_refuse_collection_limit() {
    assert!(mesh_boundary_domain_limit_operations().contains("catia_mesh_ordered_assignments"));
}

fn placement_endpoint_limit_operation(max_collection_items: u64) -> Option<&'static str> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let bytes = standard_quad_topology_stream();
    let result = crate::solve::missing_edge::standard_mesh_placement_endpoint_pairs(
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        &[None, Some([1, 2]), Some([2, 3]), Some([3, 0])],
    );
    match result {
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems =>
        {
            Some(limit.operation)
        }
        Ok(_) => None,
        other => panic!("expected collection resource limit, got {other:?}"),
    }
}

fn placement_endpoint_limit_operations() -> &'static std::collections::HashSet<&'static str> {
    static OPERATIONS: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let mut operations = std::collections::HashSet::new();
        for limit in 0..=1024 {
            if let Some(operation) = placement_endpoint_limit_operation(limit) {
                operations.insert(operation);
            }
        }
        operations
    })
}

fn boundary_support_limit_operation(max_collection_items: u64) -> &'static str {
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget,
    };
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let boundary = [crate::solve::missing_edge::MeshBoundaryEdgeCandidate {
        edge: 0,
        start: 0,
        end: 1,
        reversed: None,
    }];
    let error = crate::solve::missing_edge::boundary_endpoint_support(
        &ctx,
        &boundary,
        &[vec![[0, 1]]],
        &WorkBudget::new(100),
    )
    .expect_err("boundary support allocation exceeds the collection limit");
    match error {
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems =>
        {
            limit.operation
        }
        other => panic!("expected collection resource limit, got {other:?}"),
    }
}

#[test]
fn boundary_support_layer_marks_propagate_collection_refusal() {
    assert_eq!(
        boundary_support_limit_operation(0),
        "catia_boundary_layer_marks"
    );
}

#[test]
fn boundary_support_forward_marks_propagate_collection_refusal() {
    assert_eq!(
        boundary_support_limit_operation(2),
        "catia_boundary_forward_marks"
    );
}

#[test]
fn boundary_support_backward_marks_propagate_collection_refusal() {
    assert_eq!(
        boundary_support_limit_operation(4),
        "catia_boundary_backward_marks"
    );
}

#[test]
fn placement_endpoint_domains_propagate_collection_refusal() {
    assert!(placement_endpoint_limit_operations().contains("catia_placement_endpoint_domains"));
}

#[test]
fn placement_endpoint_counts_propagate_collection_refusal() {
    assert!(placement_endpoint_limit_operations().contains("catia_placement_counts"));
}

#[test]
fn placement_endpoint_bound_counts_propagate_collection_refusal() {
    assert!(placement_endpoint_limit_operations().contains("catia_placement_bound_counts"));
}

fn placement_missing_edge_limit_operation(max_collection_items: u64) -> Option<&'static str> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = one_gap_topology_stream();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    match crate::solve::missing_edge::standard_mesh_placement_endpoint_pairs(
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        &[None, Some([1, 2]), Some([2, 3]), Some([3, 0])],
    ) {
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems =>
        {
            Some(limit.operation)
        }
        Ok(_) => None,
        other => panic!("expected collection resource limit, got {other:?}"),
    }
}

fn placement_missing_edge_limit_operations() -> &'static std::collections::HashSet<&'static str> {
    static OPERATIONS: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let mut operations = std::collections::HashSet::new();
        for limit in 0..=1024 {
            if let Some(operation) = placement_missing_edge_limit_operation(limit) {
                operations.insert(operation);
            }
        }
        operations
    })
}

#[test]
fn placement_candidates_refuse_collection_limit() {
    assert!(placement_missing_edge_limit_operations().contains("catia_placement_candidates"));
}

#[test]
fn placement_endpoint_pairs_refuse_collection_limit() {
    assert!(placement_missing_edge_limit_operations().contains("catia_placement_endpoint_pairs"));
}

#[test]
fn compact_standard_ports_reuse_handles_in_the_global_trim_namespace() {
    let ports = crate::test_support::with_service_context(|ctx| {
        crate::solve::missing_edge::standard_global_edge_port_identities(
            ctx,
            &compact_standard_triangle_topology_stream(),
        )
    })
    .expect("service resource budget")
    .expect("compact standard ports");

    assert_eq!(ports, vec![[0, 1], [1, 2], [2, 0]]);
}

#[test]
fn standard_full_table_reuses_terminal_handles_for_every_row_layout() {
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

    let ports = crate::test_support::with_service_context(|ctx| {
        crate::solve::missing_edge::standard_global_edge_port_identities(ctx, &bytes)
    })
    .expect("service resource budget")
    .expect("standard full-table ports");
    assert_eq!(ports, vec![[0, 1], [1, 2], [1, 3]]);
}

#[test]
fn standard_mesh_ports_preserve_global_terminal_handle_identity() {
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

    let ports = crate::test_support::with_service_context(|ctx| {
        crate::solve::missing_edge::standard_mesh_edge_ports(ctx, &bytes)
    })
    .expect("service resource budget")
    .expect("mesh port collapse");
    let table_ports = crate::test_support::with_service_context(|ctx| {
        crate::solve::missing_edge::standard_global_edge_port_identities(ctx, &bytes)
    })
    .expect("service resource budget")
    .expect("global terminal-handle ports");
    assert_eq!(table_ports[1][1], table_ports[2][0]);
    assert_eq!(
        table_ports
            .iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    assert_eq!(ports[0][1], ports[1][0]);
    assert_eq!(ports[1][1], ports[2][0]);
    assert_eq!(ports[2][1], ports[3][0]);
    assert_eq!(ports[3][1], ports[0][0]);
    assert_eq!(
        ports
            .iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    assert_eq!(table_ports, ports);
}

#[test]
fn standard_mesh_resolver_derives_trim_components_from_local_ports() {
    catia_test_context!(ctx);
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
            &ctx,
            &bytes,
            &[[0, 0]; 4],
            &candidates,
        )
        .expect("service resource budget")
        .expect("trim occurrence endpoint quotient");

    assert_eq!(assignment, vec![0, 1, 2, 3]);
    assert_eq!(
        topology
            .edge_vertices(&ctx)
            .expect("service resource budget")
            .expect("resolved edge endpoints"),
        vec![[0, 1], [1, 2], [2, 3], [0, 3]]
    );
}

#[test]
fn standard_mesh_candidate_quotient_defers_occurrence_direction() {
    catia_test_context!(ctx);
    let candidates = vec![vec![[1, 2]], vec![[0, 3]], vec![[0, 1]]];
    let local_ports = [[0, 1], [2, 3], [4, 5]];
    let prematurely_oriented_ports = [[0, 1], [1, 2], [3, 1]];

    assert!(
        crate::solve::mesh_quotient::initial_mesh_quotient(&ctx, &candidates, 4, &local_ports)
            .expect("service resource budget")
            .is_some()
    );
    assert!(crate::solve::mesh_quotient::initial_mesh_quotient(
        &ctx,
        &candidates,
        4,
        &prematurely_oriented_ports,
    )
    .expect("service resource budget")
    .is_none());
}

#[test]
fn standard_mesh_ports_are_occurrence_components_not_coordinate_indices() {
    let ports = crate::test_support::with_service_context(|ctx| {
        crate::solve::missing_edge::standard_mesh_edge_ports(ctx, &standard_quad_topology_stream())
    })
    .expect("service resource budget")
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
    catia_test_context!(ctx);
    let coverage = crate::solve::missing_edge::standard_mesh_face_coverage(
        &ctx,
        &standard_quad_topology_stream(),
        &[[0, 0]; 4],
    )
    .expect("service resource budget")
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
    let coverage =
        crate::solve::missing_edge::standard_mesh_face_coverage(&ctx, &bytes, &[[0, 0]; 4])
            .expect("service resource budget")
            .expect("one gap");
    assert_eq!(coverage[0].missing_edges, [0]);
    assert_eq!(coverage[0].gaps.len(), 1);
    assert_eq!(coverage[0].gaps[0].length, 2);
    let placements = crate::solve::missing_edge::standard_mesh_missing_edge_placements(
        &ctx,
        &bytes,
        &[[0, 0]; 4],
    )
    .expect("service resource budget")
    .expect("complete missing-edge placement domain");
    assert_eq!(placements[0].len(), 1);
    assert_eq!(placements[0][0].edge, 0);
    assert_eq!(placements[0][0].segment_count, 2);
    let assignments = crate::solve::missing_edge::standard_mesh_missing_edge_assignments(
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        None,
        false,
    )
    .expect("service resource budget")
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
            &ctx,
            &local_ports,
            &[[0, 0]; 4],
            None,
            false
        )
        .expect("service resource budget")
        .is_some()
    );
    let boundaries = crate::solve::missing_edge::standard_mesh_boundary_assignments(
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        None,
    )
    .expect("service resource budget")
    .expect("complete ordered boundary assignments");
    let boundary_context =
        crate::solve::missing_edge::StandardMeshBoundaryContext::parse(&ctx, &bytes, &[[0, 0]; 4])
            .expect("service resource budget")
            .expect("parsed boundary context");
    assert_eq!(
        crate::solve::missing_edge::standard_mesh_boundary_assignments_from_context(
            &ctx,
            &boundary_context,
            None,
        )
        .expect("service resource budget")
        .expect("assignments from parsed context"),
        boundaries,
    );
    let singleton_endpoints = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[0, 3]]];
    assert_eq!(
        crate::solve::missing_edge::standard_mesh_boundary_assignments_from_context(
            &ctx,
            &boundary_context,
            Some(&singleton_endpoints),
        )
        .expect("service resource budget"),
        crate::solve::missing_edge::standard_mesh_boundary_assignments(
            &ctx,
            &bytes,
            &[[0, 0]; 4],
            Some(&singleton_endpoints),
        )
        .expect("service resource budget"),
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
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        &[0],
        &[vec![vec![false; 4]]],
    )
    .expect("service resource budget")
    .expect("selected mesh-corner quotient");
    assert_eq!(selected.logical_vertex_count(), 4);
    assert_eq!(
        selected
            .edge_vertices(&ctx)
            .expect("service resource budget")
            .expect("selected edge vertices"),
        [[0, 1], [1, 2], [2, 3], [3, 0]]
    );
    let (searched, point_assignment) =
        crate::solve::mesh_quotient::parse_standard_mesh_endpoint_candidates(
            &ctx,
            &bytes,
            &[[0, 0]; 4],
            &[Vec::new(), vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]],
        )
        .expect("service resource budget")
        .expect("abstract mesh quotient search");
    assert_eq!(searched.logical_vertex_count(), 4);
    assert_eq!(
        searched
            .edge_vertices(&ctx)
            .expect("service resource budget")
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
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        &[
            vec![[0, 1], [0, 2]],
            vec![[1, 2]],
            vec![[2, 3]],
            vec![[3, 0]],
        ],
    )
    .expect("service resource budget")
    .expect("ordered boundary endpoint domains");
    assert_eq!(cycle_domains[0], [[0, 1]]);
    let inferred_cycle_domains =
        crate::solve::missing_edge::standard_mesh_prune_endpoint_candidates(
            &ctx,
            &bytes,
            &[[0, 0]; 4],
            &[Vec::new(), vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]],
        )
        .expect("service resource budget")
        .expect("endpoint domain inferred from ordered neighbors");
    assert_eq!(inferred_cycle_domains[0], [[0, 1]]);
    let endpoint_domains = crate::solve::missing_edge::standard_mesh_placement_endpoint_pairs(
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        &[None, Some([1, 2]), Some([2, 3]), Some([3, 0])],
    )
    .expect("service resource budget")
    .expect("gap-corner endpoint domains");
    assert_eq!(endpoint_domains[0], [[0, 1]]);
    let endpoint_assignments =
        crate::solve::missing_edge::standard_mesh_missing_edge_endpoint_assignments(
            &ctx,
            &bytes,
            &[[0, 0]; 4],
            &[None, Some([1, 2]), Some([2, 3]), Some([3, 0])],
        )
        .expect("service resource budget")
        .expect("correlated gap-corner endpoint assignments");
    assert_eq!(endpoint_assignments[0].len(), 1);
    assert_eq!(endpoint_assignments[0][0].len(), 1);
    assert_eq!(
        endpoint_assignments[0][0][0].endpoint_pairs,
        Some(vec![[0, 1]])
    );
    let pruned =
        crate::solve::missing_edge::standard_mesh_pruned_missing_edge_endpoint_assignments(
            &ctx,
            &bytes,
            &[[0, 0]; 4],
            &[Some([1, 0]), Some([1, 2]), Some([2, 3]), Some([3, 0])],
        )
        .expect("service resource budget")
        .expect("endpoint-compatible face assignment");
    assert_eq!(pruned[0][0][0].endpoint_pairs, Some(vec![[0, 1]]));
    assert!(
        crate::solve::missing_edge::standard_mesh_pruned_missing_edge_endpoint_assignments(
            &ctx,
            &bytes,
            &[[0, 0]; 4],
            &[Some([0, 2]), Some([1, 2]), Some([2, 3]), Some([3, 0]),],
        )
        .expect("service resource budget")
        .is_none()
    );
}

#[test]
fn unmatched_standard_row_arity_does_not_fix_trim_span() {
    catia_test_context!(ctx);
    let mut bytes = standard_quad_topology_stream();
    let header = bytes
        .windows(3)
        .position(|window| window == [0x01, 0x01, 0x04])
        .expect("edge table header");
    let first_row = header + 3;
    bytes[first_row + 1] = 4;
    bytes.splice(first_row + 6..first_row + 6, 0x7ffe_u16.to_be_bytes());

    let coverage =
        crate::solve::missing_edge::standard_mesh_face_coverage(&ctx, &bytes, &[[0, 0]; 4])
            .expect("service resource budget")
            .expect("unmatched row coverage");
    assert_eq!(coverage[0].missing_edges, [0]);
    assert_eq!(coverage[0].gaps[0].length, 2);
    let assignments = crate::solve::missing_edge::standard_mesh_missing_edge_assignments(
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        None,
        false,
    )
    .expect("service resource budget")
    .expect("unmatched curve samples do not determine trim span");
    assert_eq!(assignments[0].len(), 1);
    assert_eq!(assignments[0][0][0].segment_count, 2);
}

#[test]
fn unmatched_fbb_complete_row_arity_fixes_trim_span() {
    catia_test_context!(ctx);
    let mut bytes = crate::test_support::test_topology::fbb_only_quad_topology_stream();
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
    let coverage =
        crate::solve::missing_edge::standard_mesh_face_coverage(&ctx, &bytes, &[[0, 0]; 4])
            .expect("service resource budget")
            .expect("unmatched FBB row coverage");
    assert_eq!(coverage[0].missing_edges, [0, 1]);
    assert_eq!(coverage[0].gaps[0].length, 4);

    let assignments = crate::solve::missing_edge::standard_mesh_missing_edge_assignments(
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        None,
        false,
    )
    .expect("service resource budget")
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
    let runs = crate::test_support::with_service_context(|ctx| {
        crate::solve::missing_edge::standard_mesh_edge_runs(ctx, &standard_quad_topology_stream())
    })
    .expect("service resource budget")
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
    catia_test_context!(ctx);
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
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        None,
        false,
    )
    .expect("service resource budget")
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
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        &[vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]],
    )
    .expect("service resource budget")
    .expect("endpoint-constrained full gap");
    assert_eq!(topology.logical_vertex_count(), 4);
    assert_eq!(points, [0, 1, 2, 3]);
}

#[test]
fn standard_mesh_endpoint_domains_ignore_row_local_endpoint_order() {
    catia_test_context!(ctx);
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
        &ctx,
        &bytes,
        &[[0, 0]; 4],
        &[vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]],
    )
    .expect("service resource budget")
    .expect("independent endpoint-port gauge");
    let coedges = &topology.faces()[0].boundaries[0].coedges;
    assert!(coedges.iter().all(|coedge| !coedge.reversed));
}
