use super::{
    refine_repeated_edge_face_candidates, repeated_edge_face_handle_candidates_from_sets,
    repeated_face_endpoint_closures, resolve_edge_faces_from_runs,
    unique_duplicate_face_assignment, visualization_endpoint_pairs, FaceOptions, MeshEdgeRun,
    StandardMeshBoundaryContext, INDEXED_VISUALIZATION_POINT_HEADER_LEN,
    INDEXED_VISUALIZATION_POINT_MARKER,
};
use crate::families::standard::topology::{EdgeBoundaryLayout, EdgeRow};
use std::collections::HashSet;
use std::sync::Arc;

fn row(handles: &[u32]) -> EdgeRow {
    EdgeRow {
        kind: 2,
        handles: handles.to_vec(),
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }
}

fn handles(values: &[u32]) -> HashSet<u32> {
    values.iter().copied().collect()
}

#[test]
fn counted_port_identity_maps_and_pairs_refuse_before_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let fixtures = [
        (
            crate::test_support::test_topology::standard_quad_topology_stream(),
            false,
        ),
        (
            crate::test_support::test_topology::fbb_only_quad_topology_stream(),
            true,
        ),
    ];
    for (bytes, fbb) in fixtures {
        let run = |ctx: &DecodeContext<'_>| {
            if fbb {
                super::fbb_global_edge_port_identities(ctx, &bytes)
            } else {
                super::standard_global_edge_port_identities(ctx, &bytes)
            }
        };
        let ports = crate::test_support::with_service_context(run)
            .expect("service resource budget")
            .expect("counted edge ports");
        assert_eq!(ports.len(), 4);
        let mut refused = HashSet::new();
        for cap in 0..256 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("fixture fits the input limit");
            match run(&ctx) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    refused.insert(limit.operation);
                }
                Ok(Some(_)) => break,
                _ => panic!("unexpected port identity result"),
            }
        }
        let operations = if fbb {
            ["catia_fbb_port_handle_ids", "catia_fbb_port_pairs"]
        } else {
            [
                "catia_standard_port_handle_ids",
                "catia_standard_port_pairs",
            ]
        };
        for operation in operations {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
    }
}

#[test]
fn edge_run_face_collection_refuses_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let run = MeshEdgeRun {
        edge: 0,
        face: 1,
        cycle: 0,
        start: 0,
        segment_count: 1,
        reversed: false,
    };
    catia_test_context!(service_ctx);
    assert_eq!(
        resolve_edge_faces_from_runs(&service_ctx, &[[1, 1]], &[run]).expect("service budget"),
        Some(vec![[1, 1]])
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = resolve_edge_faces_from_runs(&ctx, &[[1, 1]], &[run])
        .expect_err("edge run face collection exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_edge_run_faces"));
}

#[test]
fn edge_port_queue_propagates_collection_refusal() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let ports = [[10, 11]];
    let pairs = [Some([0, 1])];
    catia_test_context!(service_ctx);
    assert_eq!(
        super::propagate_edge_port_points(&service_ctx, &ports, &pairs)
            .expect("service resource budget"),
        Some(vec![Some([0, 1])])
    );

    let mut operations = HashSet::new();
    for limit in 0..=128 {
        match crate::test_support::with_collection_limit(limit, |ctx| {
            super::propagate_edge_port_points(ctx, &ports, &pairs)
        }) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(Some(resolved)) => {
                assert_eq!(resolved, vec![Some([0, 1])]);
                break;
            }
            outcome => panic!("unexpected edge port propagation outcome: {outcome:?}"),
        }
    }
    for operation in [
        "catia_port_resolved_pairs",
        "catia_port_edge_entries",
        "catia_port_incident_edges",
        "catia_port_pair_points",
        "catia_edge_port_initial_queue",
        "catia_edge_port_queue",
        "catia_port_resolved_port_rows",
        "catia_port_resolved_candidate_pair",
        "catia_port_resolved_candidate_rows",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn ordered_port_seed_refuses_before_binding_point_map() {
    let ports = [[10, 11]];
    let pairs = [Some([0, 1])];
    let ordered = [Some([0, 1])];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::propagate_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &ordered)
    };
    assert_eq!(crate::test_support::with_service_context(run).expect("service budget"), Some(vec![Some([0, 1])]));
    let mut operations = HashSet::new();
    for limit in 0..=128 {
        match crate::test_support::with_collection_limit(limit, run) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                operations.insert(error.operation);
            }
            Ok(Some(_)) => break,
            outcome => panic!("unexpected ordered port outcome: {outcome:?}"),
        }
    }
    assert!(operations.contains("catia_port_bound_points"));
}

#[test]
fn partial_port_projection_refuses_before_known_rows() {
    let ports = [Some([10, 11]), None];
    let pairs = [Some([0, 1]), Some([2, 3])];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::propagate_partial_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &[])
    };
    assert_eq!(crate::test_support::with_service_context(run).expect("service budget"), Some(pairs.to_vec()));
    let mut operations = HashSet::new();
    for limit in 0..=128 {
        match crate::test_support::with_collection_limit(limit, run) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                operations.insert(error.operation);
            }
            Ok(Some(_)) => break,
            outcome => panic!("unexpected partial port outcome: {outcome:?}"),
        }
    }
    for operation in [
        "catia_partial_port_resolved_pairs",
        "catia_partial_known_port_rows",
        "catia_partial_port_rows",
        "catia_partial_pair_rows",
        "catia_partial_ordered_rows",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn edge_port_solution_propagates_collection_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let ports = [[10, 11]];
    let candidates = [vec![[0, 1]]];
    catia_test_context!(service_ctx);
    assert_eq!(
        super::bind_edge_port_candidates(&service_ctx, &ports, &candidates)
            .expect("service resource budget"),
        Some(vec![[0, 1]])
    );

    let mut reached = false;
    for cap in 0..=64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        if let Err(CodecError::ResourceLimit(limit)) =
            super::bind_edge_port_candidates(&ctx, &ports, &candidates)
        {
            if limit.operation == "catia_edge_port_solution" {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                reached = true;
                break;
            }
        }
    }
    assert!(
        reached,
        "the solution must refuse at its collection boundary"
    );
}

#[test]
fn edge_port_component_pairs_propagate_collection_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let ports = [[10, 11]];
    let candidates = [vec![[0, 1]]];
    catia_test_context!(service_ctx);
    assert_eq!(
        super::bind_edge_port_candidates(&service_ctx, &ports, &candidates)
            .expect("service resource budget"),
        Some(vec![[0, 1]])
    );

    let mut reached = false;
    for cap in 0..=64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        if let Err(CodecError::ResourceLimit(limit)) =
            super::bind_edge_port_candidates(&ctx, &ports, &candidates)
        {
            if limit.operation == "catia_edge_port_pairs" {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                reached = true;
                break;
            }
        }
    }
    assert!(
        reached,
        "component pairs must refuse at their collection boundary"
    );
}

fn raw_visualization_table(mode: u8, triples: &[[f32; 3]]) -> Vec<u8> {
    let mut bytes = INDEXED_VISUALIZATION_POINT_MARKER.to_vec();
    let count = u32::try_from(triples.len()).expect("synthetic table count");
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.push(0xff);
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, mode]);
    for triple in triples {
        for coordinate in triple {
            bytes.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn raw_visualization_points_bind_terminal_handles_by_direct_index() {
    catia_test_context!(ctx);
    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let bytes = raw_visualization_table(1, &[points[0], points[1], points[0]]);
    let rows = [row(&[0, 40, 1]), row(&[1, 41, 2])];

    assert_eq!(
        visualization_endpoint_pairs(&ctx, &bytes, &rows, &points).expect("service budget"),
        Some(vec![[0, 1], [1, 0]])
    );
}

#[test]
fn compressed_visualization_points_reuse_coordinate_prefixes() {
    catia_test_context!(ctx);
    let points = [[1.0, 2.0, 3.0], [1.0, 2.0, 4.0], [1.0, 5.0, 6.0]];
    let mut bytes = INDEXED_VISUALIZATION_POINT_MARKER.to_vec();
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.push(0xff);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xe4, 0xff]);
    bytes.extend_from_slice(&6u32.to_le_bytes());
    for scalar in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0] {
        bytes.extend_from_slice(&scalar.to_le_bytes());
    }
    let rows = [row(&[0, 20, 2]), row(&[2, 21, 3])];

    assert_eq!(
        visualization_endpoint_pairs(&ctx, &bytes, &rows, &points).expect("service budget"),
        Some(vec![[0, 1], [1, 2]])
    );
}

#[test]
fn compressed_visualization_points_require_initial_xyz_and_exact_scalar_count() {
    catia_test_context!(ctx);
    let points = [[1.0, 2.0, 3.0], [1.0, 2.0, 4.0]];
    let rows = [row(&[0, 1])];
    let mut bytes = INDEXED_VISUALIZATION_POINT_MARKER.to_vec();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.push(0xff);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x08, 0xff]);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    for scalar in [1.0f32, 2.0, 3.0, 4.0] {
        bytes.extend_from_slice(&scalar.to_le_bytes());
    }

    let scalar_count_at = INDEXED_VISUALIZATION_POINT_HEADER_LEN + 2;
    let mut missing_scalar = bytes.clone();
    missing_scalar[scalar_count_at..scalar_count_at + 4].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(
        visualization_endpoint_pairs(&ctx, &missing_scalar, &rows, &points).expect("service budget"),
        None
    );

    bytes[19] |= 1;
    assert_eq!(visualization_endpoint_pairs(&ctx, &bytes, &rows, &points).expect("service budget"), None);

    let mut missing_delimiter = bytes;
    missing_delimiter[20] = 0;
    assert_eq!(
        visualization_endpoint_pairs(&ctx, &missing_delimiter, &rows, &points).expect("service budget"),
        None
    );
}

#[test]
fn visualization_points_abstain_for_other_modes_or_incomplete_coverage() {
    catia_test_context!(ctx);
    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let rows = [row(&[0, 1])];

    assert_eq!(
        visualization_endpoint_pairs(&ctx, &raw_visualization_table(2, &points), &rows, &points,).expect("service budget"),
        None
    );
    assert_eq!(
        visualization_endpoint_pairs(&ctx,
            &raw_visualization_table(1, &[points[0], points[0]]),
            &rows,
            &points,
        ).expect("service budget"),
        None
    );

    let mut missing_secondary_lead = raw_visualization_table(1, &points);
    missing_secondary_lead[10] = 0;
    assert_eq!(
        visualization_endpoint_pairs(&ctx, &missing_secondary_lead, &rows, &points).expect("service budget"),
        None
    );
}

#[test]
fn visualization_endpoint_bindings_refuse_before_each_collection() {
    use cadmpeg_core::CodecError;

    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let rows = [row(&[0, 1])];
    let raw = raw_visualization_table(1, &points);
    let mut compressed = INDEXED_VISUALIZATION_POINT_MARKER.to_vec();
    compressed.extend_from_slice(&2u32.to_le_bytes());
    compressed.push(0xff);
    compressed.extend_from_slice(&2u32.to_le_bytes());
    compressed.extend_from_slice(&[0, 0, 0, 0]);
    compressed.extend_from_slice(&[0x00, 0xff]);
    compressed.extend_from_slice(&6u32.to_le_bytes());
    for scalar in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0] {
        compressed.extend_from_slice(&scalar.to_le_bytes());
    }
    for (bytes, binding_operation) in [
        (&raw, "catia_raw_visualization_bindings"),
        (&compressed, "catia_compressed_visualization_bindings"),
    ] {
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            visualization_endpoint_pairs(ctx, bytes, &rows, &points)
        };
        assert_eq!(crate::test_support::with_service_context(run).expect("service budget"), Some(vec![[0, 1]]));
        let mut operations = HashSet::new();
        for limit in 0..=32 {
            match crate::test_support::with_collection_limit(limit, run) {
                Err(CodecError::ResourceLimit(error)) => { operations.insert(error.operation); }
                Ok(Some(pairs)) if pairs == [[0, 1]] => break,
                outcome => panic!("unexpected visualization result: {outcome:?}"),
            }
        }
        for operation in [
            "catia_visualization_point_bits",
            "catia_visualization_terminal_handles",
            binding_operation,
            "catia_visualization_matched_points",
            "catia_visualization_endpoint_pairs",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }
}

#[test]
fn repeated_long_row_selects_one_majority_sharing_face() {
    catia_test_context!(ctx);
    let rows = vec![row(&[10, 11, 12, 13, 14])];
    let faces = vec![
        handles(&[10, 11, 12, 13, 14, 90]),
        handles(&[10, 11, 12, 80]),
        handles(&[10, 14, 70]),
    ];

    let candidates = repeated_edge_face_handle_candidates_from_sets(&ctx, &rows, &faces, &[[0, 0]])
        .expect("service resource budget")
        .expect("complete owning-face containment");

    assert_eq!(candidates, vec![vec![1]]);
}

#[test]
fn repeated_handle_candidates_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let rows = vec![row(&[10, 11])];
    let faces = vec![handles(&[10, 11]), handles(&[10, 11])];
    catia_test_context!(service_ctx);
    assert_eq!(
        repeated_edge_face_handle_candidates_from_sets(&service_ctx, &rows, &faces, &[[0, 0]],)
            .expect("service budget"),
        Some(vec![vec![1]])
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = repeated_edge_face_handle_candidates_from_sets(&ctx, &rows, &faces, &[[0, 0]])
        .expect_err("candidate collection exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_repeated_edge_handle_face_candidates"));
}

#[test]
fn repeated_long_row_abstains_when_majority_sharing_is_not_unique() {
    catia_test_context!(ctx);
    let rows = vec![row(&[10, 11, 12, 13])];
    let faces = vec![
        handles(&[10, 11, 12, 13]),
        handles(&[10, 11, 12]),
        handles(&[11, 12, 13]),
    ];

    let candidates = repeated_edge_face_handle_candidates_from_sets(&ctx, &rows, &faces, &[[0, 0]])
        .expect("service resource budget")
        .expect("complete owning-face containment");

    assert_eq!(candidates, vec![Vec::<usize>::new()]);
}

#[test]
fn repeated_short_row_retains_every_complete_handle_sharing_face() {
    catia_test_context!(ctx);
    let rows = vec![row(&[10, 11])];
    let faces = vec![
        handles(&[10, 11, 90]),
        handles(&[10, 11, 80]),
        handles(&[10, 11, 70]),
        handles(&[10, 60]),
    ];

    let candidates = repeated_edge_face_handle_candidates_from_sets(&ctx, &rows, &faces, &[[0, 0]])
        .expect("service resource budget")
        .expect("complete owning-face containment");

    assert_eq!(candidates, vec![vec![1, 2]]);
}

#[test]
fn repeated_handle_selector_requires_file_wide_owning_face_containment() {
    catia_test_context!(ctx);
    let rows = vec![row(&[10, 11]), row(&[20, 21])];
    let faces = vec![handles(&[10, 11, 20]), handles(&[20, 21])];

    assert!(
        repeated_edge_face_handle_candidates_from_sets(&ctx, &rows, &faces, &[[0, 0], [0, 1]],)
            .expect("service resource budget")
            .is_none()
    );
}

#[test]
fn handle_face_candidates_do_not_reopen_resolved_incidence() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 2], [1, 1], [3, 3]];
    let mut allowed = [vec![2], vec![2, 4], Vec::new()];
    let handles = [vec![2], vec![4, 5], vec![5]];

    refine_repeated_edge_face_candidates(&ctx, &edge_faces, &mut allowed, &handles)
        .expect("service budget")
        .expect("aligned face domains");

    assert!(allowed[0].is_empty());
    assert_eq!(allowed[1], vec![4]);
    assert_eq!(allowed[2], vec![5]);
}

#[test]
fn repeated_face_refinement_refuses_before_intersection_and_copy() {
    use cadmpeg_core::CodecError;
    for (allowed_face, expected_operation) in [
        (vec![2usize], "catia_repeated_face_intersection"),
        (vec![3usize], "catia_repeated_face_handle_copy"),
    ] {
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let mut allowed = [allowed_face.clone()];
            refine_repeated_edge_face_candidates(ctx, &[[0, 0]], &mut allowed, &[vec![2]])
        };
        assert_eq!(crate::test_support::with_service_context(run).expect("service budget"), Some(()));
        assert!(matches!(
            crate::test_support::with_collection_limit(0, run),
            Err(CodecError::ResourceLimit(limit)) if limit.operation == expected_operation
        ));
    }
}

#[test]
fn endpoint_degree_closure_selects_optional_second_face() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 1], [0, 0], [0, 1]];
    let allowed = [Vec::new(), vec![1], Vec::new()];
    let endpoint_pairs = [[0, 1], [1, 2], [2, 0]];

    let completed =
        repeated_face_endpoint_closures(&ctx, &edge_faces, &allowed, &endpoint_pairs, 2)
            .expect("service resource budget")
            .expect("bounded endpoint closure");

    assert_eq!(completed, vec![vec![[0, 1], [0, 1], [0, 1]]]);
}

#[test]
fn endpoint_degree_closure_refuses_face_degree_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let edge_faces = [[0, 1], [0, 0], [0, 1]];
    let allowed = [Vec::new(), vec![1], Vec::new()];
    let endpoint_pairs = [[0, 1], [1, 2], [2, 0]];
    catia_test_context!(service_ctx);
    assert!(repeated_face_endpoint_closures(
        &service_ctx,
        &edge_faces,
        &allowed,
        &endpoint_pairs,
        2,
    )
    .expect("service budget")
    .is_some());

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = repeated_face_endpoint_closures(&ctx, &edge_faces, &allowed, &endpoint_pairs, 2)
        .expect_err("face degree collection exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia missing-edge face degrees"));
}

#[test]
fn endpoint_degree_closure_charges_branch_search_arrays() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let edge_faces = [[0, 1], [0, 0], [0, 1]];
    let allowed = [Vec::new(), vec![1], Vec::new()];
    let endpoint_pairs = [[0, 1], [1, 2], [2, 0]];
    let mut operations = BTreeSet::new();
    let mut completed = false;
    for limit in 0..=64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match repeated_face_endpoint_closures(&ctx, &edge_faces, &allowed, &endpoint_pairs, 2) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(Some(_)) => {
                completed = true;
                break;
            }
            Ok(None) => panic!("endpoint closure fixture must remain viable"),
            Err(error) => panic!("unexpected endpoint closure refusal: {error}"),
        }
    }
    assert!(
        completed,
        "collection limit 64 must admit the closure fixture"
    );
    assert!(operations.contains("catia missing-edge branch assignment"));
    assert!(operations.contains("catia missing-edge used branches"));
    for operation in [
        "catia missing-edge branch choices",
        "catia missing-edge branches",
        "catia missing-edge branch owners",
        "catia missing-edge search choices",
        "catia missing-edge solution assignment",
        "catia missing-edge solution list",
        "catia missing-edge completed solutions",
        "catia missing-edge completed faces",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn endpoint_degree_closure_refuses_closed_face_copy() {
    let faces = [[0usize, 0], [0, 0], [0, 0]];
    let allowed = [Vec::new(), Vec::new(), Vec::new()];
    let pairs = [[0, 1], [1, 2], [2, 0]];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        repeated_face_endpoint_closures(ctx, &faces, &allowed, &pairs, 1)
    };
    assert_eq!(crate::test_support::with_service_context(run).expect("service budget"), Some(vec![faces.to_vec()]));
    let mut operations = HashSet::new();
    for limit in 0..=32 {
        match crate::test_support::with_collection_limit(limit, run) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => { operations.insert(error.operation); }
            Ok(Some(_)) => break,
            outcome => panic!("unexpected closed-face result: {outcome:?}"),
        }
    }
    assert!(operations.contains("catia missing-edge closed faces"));
    assert!(operations.contains("catia missing-edge closed solutions"));
}

#[test]
fn endpoint_degree_closure_retains_one_face_incidences() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 1], [0, 0], [1, 1], [0, 1]];
    let allowed = [Vec::new(), vec![1], vec![0], Vec::new()];
    let endpoint_pairs = [[0, 1], [1, 2], [1, 2], [2, 0]];

    let completed =
        repeated_face_endpoint_closures(&ctx, &edge_faces, &allowed, &endpoint_pairs, 2)
            .expect("service resource budget")
            .expect("bounded endpoint closure");

    assert_eq!(completed, vec![edge_faces.to_vec()]);
}

#[test]
fn endpoint_degree_closure_retains_symmetric_face_swaps() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 1], [2, 3], [2, 2], [3, 3]];
    let allowed = [Vec::new(), Vec::new(), vec![0, 1], vec![0, 1]];
    let endpoint_pairs = [[0, 1]; 4];

    let mut completed =
        repeated_face_endpoint_closures(&ctx, &edge_faces, &allowed, &endpoint_pairs, 4)
            .expect("service resource budget")
            .expect("bounded endpoint closure");
    completed.sort();

    assert_eq!(
        completed,
        vec![
            vec![[0, 1], [2, 3], [2, 0], [3, 1]],
            vec![[0, 1], [2, 3], [2, 1], [3, 0]],
        ]
    );
}

#[test]
fn candidate_contexts_share_edge_row_storage() {
    const CANDIDATES: usize = 1024;
    catia_test_context!(ctx);
    let bytes = crate::test_support::test_topology::standard_quad_topology_stream();
    let faces = [[0, 0]; 4];
    let base = StandardMeshBoundaryContext::parse(&ctx, &bytes, &faces)
        .expect("service resource budget")
        .expect("quad boundary context");
    let mut candidates = Vec::with_capacity(CANDIDATES);
    assert_eq!(Arc::strong_count(&base.analysis), 1);
    for _ in 0..CANDIDATES {
        candidates.push(
            base.with_edge_faces(&ctx, &faces)
                .expect("service resource budget")
                .expect("quad candidate context"),
        );
        assert_eq!(Arc::strong_count(&base.analysis), candidates.len() + 1);
    }
    for candidate in &candidates {
        let StandardMeshBoundaryContext {
            analysis,
            coverage,
            edge_ports,
            edge_runs,
            cycle_lengths,
        } = candidate;
        assert!(Arc::ptr_eq(analysis, &base.analysis));
        assert_eq!(coverage, &base.coverage);
        assert_eq!(edge_ports, &base.edge_ports);
        assert_eq!(edge_runs, &base.edge_runs);
        assert_eq!(cycle_lengths, &base.cycle_lengths);
    }
    drop(candidates);
    assert_eq!(Arc::strong_count(&base.analysis), 1);
}

#[test]
fn face_options_hold_the_retained_face_in_ascending_order() {
    catia_test_context!(ctx);
    for (retained, others) in [
        (3usize, vec![]),
        (3, vec![5, 9]),
        (7, vec![1, 4]),
        (5, vec![1, 9]),
        (0, vec![0usize; 0]),
    ] {
        let options = FaceOptions::from_admitted(&ctx, retained, others.clone()).expect("service budget");
        let mut expected = others;
        expected.push(retained);
        expected.sort_unstable();
        assert_eq!(options.iter().collect::<Vec<_>>(), expected);
        assert_eq!(options.count(), expected.len());
        assert_eq!(options.first, expected[0]);
    }
}

#[test]
fn face_options_order_and_deduplicate_the_admitted_faces_they_are_given() {
    catia_test_context!(ctx);
    // Unsorted, with a repeat, and holding `retained` itself. The type does
    // the filter, the sort and the dedup, so the caller states none of them.
    let options = FaceOptions::from_admitted(&ctx, 5, [9usize, 1, 5, 9, 1, 3]).expect("service budget");

    assert_eq!(options.iter().collect::<Vec<_>>(), vec![1, 3, 5, 9]);
    assert_eq!(options.count(), 4);
    assert_eq!(options.first, 1);

    // `retained` smaller than every admitted face, still unsorted and repeated.
    let options = FaceOptions::from_admitted(&ctx, 0, [4usize, 2, 4]).expect("service budget");
    assert_eq!(options.iter().collect::<Vec<_>>(), vec![0, 2, 4]);

    // Nothing admitted beyond the retained face.
    let options = FaceOptions::from_admitted(&ctx, 7, [7usize, 7]).expect("service budget");
    assert_eq!(options.iter().collect::<Vec<_>>(), vec![7]);
    assert_eq!(options.count(), 1);
}

#[test]
fn a_repeated_slot_with_one_admitted_face_takes_it_without_a_search() {
    catia_test_context!(ctx);
    let serialized = [[0usize, 0]];
    let allowed = vec![vec![0usize]];
    let solved = unique_duplicate_face_assignment(&ctx, &serialized, &allowed, 1, |_| Ok(true))
        .expect("service resource budget");
    assert_eq!(solved, Some(vec![[0, 0]]));
}

#[test]
fn a_repeated_slot_with_two_admitted_faces_resolves_to_the_one_valid_assignment() {
    catia_test_context!(ctx);
    let serialized = [[0usize, 0]];
    let allowed = vec![vec![0usize, 1]];
    let solved = unique_duplicate_face_assignment(&ctx, &serialized, &allowed, 2, |assignment| {
        Ok(assignment[0][1] == 1)
    })
    .expect("service resource budget");
    assert_eq!(solved, Some(vec![[0, 1]]));
}

#[test]
fn duplicate_face_assignment_refuses_before_unresolved_slot_storage() {
    let serialized = [[0usize, 0]];
    let allowed = [vec![0usize, 1]];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        unique_duplicate_face_assignment(ctx, &serialized, &allowed, 2, |faces| {
            Ok(faces[0][1] == 1)
        })
    };
    assert_eq!(crate::test_support::with_service_context(run).expect("service budget"), Some(vec![[0, 1]]));
    assert!(matches!(
        crate::test_support::with_collection_limit(0, run),
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_duplicate_face_unresolved"
    ));
}

#[test]
fn mesh_edge_run_materialization_refuses_before_occurrence_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = crate::test_support::test_topology::standard_quad_topology_stream();
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::standard_mesh_edge_runs(ctx, &bytes)
    };
    assert_eq!(crate::test_support::with_service_context(run).expect("service budget").expect("runs").len(), 4);
    let mut limit = 0;
    let mut refused = HashSet::new();
    for _ in 0..256 {
        match crate::test_support::with_collection_limit(limit, run) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation);
                limit = error.used + error.additional;
            }
            Ok(Some(runs)) => {
                assert_eq!(runs.len(), 4);
                break;
            }
            outcome => panic!("unexpected mesh runs outcome: {outcome:?}"),
        }
    }
    assert!(refused.contains("catia_mesh_edge_run_rows"));
}

mod ports_and_coverage;
