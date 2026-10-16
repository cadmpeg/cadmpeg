use super::{
    refine_repeated_edge_face_candidates, repeated_edge_face_handle_candidates_from_sets,
    repeated_face_endpoint_closures, unique_duplicate_face_assignment,
    visualization_endpoint_pairs, FaceOptions, StandardMeshBoundaryContext,
    INDEXED_VISUALIZATION_POINT_HEADER_LEN, INDEXED_VISUALIZATION_POINT_MARKER,
};
use crate::families::standard::topology::{EdgeBoundaryLayout, EdgeRow};
use crate::solve::missing_edge::resolve_edge_faces_from_runs;
use crate::solve::missing_edge::MeshEdgeRun;
use std::collections::HashSet;
use std::sync::Arc;

fn row(handles: &[u32]) -> EdgeRow {
    EdgeRow::new(2, handles.to_vec(), EdgeBoundaryLayout::CompleteBoundaryRun)
        .expect("admitted edge row")
}

fn handles(values: &[u32]) -> HashSet<u32> {
    values.iter().copied().collect()
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
    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let bytes = raw_visualization_table(1, &[points[0], points[1], points[0]]);
    let rows = [row(&[0, 40, 1]), row(&[1, 41, 2])];

    assert_eq!(
        visualization_endpoint_pairs(&bytes, &rows, &points),
        Some(vec![[0, 1], [1, 0]])
    );
}

#[test]
fn compressed_visualization_points_reuse_coordinate_prefixes() {
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
        visualization_endpoint_pairs(&bytes, &rows, &points),
        Some(vec![[0, 1], [1, 2]])
    );
}

#[test]
fn compressed_visualization_points_require_initial_xyz_and_exact_scalar_count() {
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
        visualization_endpoint_pairs(&missing_scalar, &rows, &points),
        None
    );

    bytes[19] |= 1;
    assert_eq!(visualization_endpoint_pairs(&bytes, &rows, &points), None);

    let mut missing_delimiter = bytes;
    missing_delimiter[20] = 0;
    assert_eq!(
        visualization_endpoint_pairs(&missing_delimiter, &rows, &points),
        None
    );
}

#[test]
fn visualization_points_abstain_for_other_modes_or_incomplete_coverage() {
    let points = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
    let rows = [row(&[0, 1])];

    assert_eq!(
        visualization_endpoint_pairs(&raw_visualization_table(2, &points), &rows, &points),
        None
    );
    assert_eq!(
        visualization_endpoint_pairs(
            &raw_visualization_table(1, &[points[0], points[0]]),
            &rows,
            &points
        ),
        None
    );

    let mut missing_secondary_lead = raw_visualization_table(1, &points);
    missing_secondary_lead[10] = 0;
    assert_eq!(
        visualization_endpoint_pairs(&missing_secondary_lead, &rows, &points),
        None
    );
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
        .expect("compute the repeated edge face handle candidates from sets")
        .expect("complete owning-face containment");

    assert_eq!(candidates, vec![vec![1]]);
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
        .expect("compute the repeated edge face handle candidates from sets")
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
        .expect("compute the repeated edge face handle candidates from sets")
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
            .expect("compute the repeated edge face handle candidates from sets")
            .is_none()
    );
}

#[test]
fn handle_face_candidates_do_not_reopen_resolved_incidence() {
    let edge_faces = [[0, 2], [1, 1], [3, 3]];
    let mut allowed = [vec![2], vec![2, 4], Vec::new()];
    let handles = [vec![2], vec![4, 5], vec![5]];

    refine_repeated_edge_face_candidates(&edge_faces, &mut allowed, &handles)
        .expect("aligned face domains");

    assert!(allowed[0].is_empty());
    assert_eq!(allowed[1], vec![4]);
    assert_eq!(allowed[2], vec![5]);
}

#[test]
fn endpoint_degree_closure_selects_optional_second_face() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 1], [0, 0], [0, 1]];
    let allowed = [Vec::new(), vec![1], Vec::new()];
    let endpoint_pairs = [[0, 1], [1, 2], [2, 0]];

    let completed =
        repeated_face_endpoint_closures(&ctx, &edge_faces, &allowed, &endpoint_pairs, 2)
            .expect("compute the repeated face endpoint closures")
            .expect("bounded endpoint closure");

    assert_eq!(completed, vec![vec![[0, 1], [0, 1], [0, 1]]]);
}

#[test]
fn endpoint_degree_closure_retains_one_face_incidences() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 1], [0, 0], [1, 1], [0, 1]];
    let allowed = [Vec::new(), vec![1], vec![0], Vec::new()];
    let endpoint_pairs = [[0, 1], [1, 2], [1, 2], [2, 0]];

    let completed =
        repeated_face_endpoint_closures(&ctx, &edge_faces, &allowed, &endpoint_pairs, 2)
            .expect("compute the repeated face endpoint closures")
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
            .expect("compute the repeated face endpoint closures")
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
        .expect("parse the standard mesh boundary context")
        .expect("quad boundary context");
    let mut candidates = Vec::with_capacity(CANDIDATES);
    assert_eq!(Arc::strong_count(&base.analysis), 1);
    for _ in 0..CANDIDATES {
        candidates.push(
            base.with_edge_faces(&ctx, &faces)
                .expect("bind the edge faces")
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
    for (retained, others) in [
        (3usize, vec![]),
        (3, vec![5, 9]),
        (7, vec![1, 4]),
        (5, vec![1, 9]),
        (0, vec![0usize; 0]),
    ] {
        let options = FaceOptions::from_admitted(retained, others.clone());
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
    // Unsorted, with a repeat, and holding `retained` itself. The type does
    // the filter, the sort and the dedup, so the caller states none of them.
    let options = FaceOptions::from_admitted(5, [9usize, 1, 5, 9, 1, 3]);

    assert_eq!(options.iter().collect::<Vec<_>>(), vec![1, 3, 5, 9]);
    assert_eq!(options.count(), 4);
    assert_eq!(options.first, 1);

    // `retained` smaller than every admitted face, still unsorted and repeated.
    let options = FaceOptions::from_admitted(0, [4usize, 2, 4]);
    assert_eq!(options.iter().collect::<Vec<_>>(), vec![0, 2, 4]);

    // Nothing admitted beyond the retained face.
    let options = FaceOptions::from_admitted(7, [7usize, 7]);
    assert_eq!(options.iter().collect::<Vec<_>>(), vec![7]);
    assert_eq!(options.count(), 1);
}

#[test]
fn a_repeated_slot_with_one_admitted_face_takes_it_without_a_search() {
    let serialized = [[0usize, 0]];
    let allowed = vec![vec![0usize]];
    let solved = unique_duplicate_face_assignment(&serialized, &allowed, 1, |_| Ok(true))
        .expect("find the unique duplicate face assignment");
    assert_eq!(solved, Some(vec![[0, 0]]));
}

#[test]
fn a_repeated_slot_with_two_admitted_faces_resolves_to_the_one_valid_assignment() {
    let serialized = [[0usize, 0]];
    let allowed = vec![vec![0usize, 1]];
    let solved = unique_duplicate_face_assignment(&serialized, &allowed, 2, |assignment| {
        Ok(assignment[0][1] == 1)
    })
    .expect("find the unique duplicate face assignment");
    assert_eq!(solved, Some(vec![[0, 1]]));
}

mod ports_and_coverage;

#[test]
fn repeated_handle_index_preserves_face_order() {
    let rows = vec![row(&[10, 10, 11, 11])];
    let mut faces = vec![handles(&[10, 11]), handles(&[10, 11]), handles(&[10, 11])];
    for handle in 100..1124 {
        faces.push(handles(&[handle]));
    }
    let expected = vec![vec![1, 2]];
    let actual = crate::test_support::with_service_context(|ctx| {
        repeated_edge_face_handle_candidates_from_sets(ctx, &rows, &faces, &[[0, 0]])
    })
    .expect("find the repeated edge face handle candidates");
    assert_eq!(actual, Some(expected));
}

#[test]
fn standard_and_fbb_quads_count_four_edge_ports() {
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
        let ports = if fbb {
            super::fbb_edge_port_identities(&bytes, true)
        } else {
            crate::test_support::with_service_context(|ctx| {
                super::standard_edge_port_identities(ctx, &bytes, true)
            })
            .expect("compute the standard edge port identities")
        }
        .expect("counted edge ports");
        assert_eq!(ports.len(), 4);
    }
}

#[test]
fn edge_runs_resolve_a_consistent_edge_face_pair() {
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
        resolve_edge_faces_from_runs(&service_ctx, &[[1, 1]], &[run])
            .expect("resolve edge faces from runs"),
        Some(vec![[1, 1]])
    );
}

#[test]
fn edge_port_propagation_keeps_a_seeded_point_pair() {
    let ports = [[10, 11]];
    let pairs = [Some([0, 1])];
    catia_test_context!(service_ctx);
    assert_eq!(
        super::propagate_edge_port_points(&service_ctx, &ports, &pairs)
            .expect("propagate edge port points"),
        Some(vec![Some([0, 1])])
    );
}

#[test]
fn ordered_seed_matching_the_pair_propagates_unchanged() {
    let ports = [[10, 11]];
    let pairs = [Some([0, 1])];
    let ordered = [Some([0, 1])];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::propagate_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &ordered)
    };
    assert_eq!(
        crate::test_support::with_service_context(run)
            .expect("propagate edge port points with ordered seeds"),
        Some(vec![Some([0, 1])])
    );
}

#[test]
fn partial_port_propagation_returns_the_given_pairs() {
    let ports = [Some([10, 11]), None];
    let pairs = [Some([0, 1]), Some([2, 3])];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::propagate_partial_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &[])
    };
    assert_eq!(
        crate::test_support::with_service_context(run)
            .expect("propagate partial edge port points with ordered seeds"),
        Some(pairs.to_vec())
    );
}

#[test]
fn single_port_candidate_binds_to_its_edge() {
    let ports = [[10, 11]];
    let candidates = [vec![[0, 1]]];
    catia_test_context!(service_ctx);
    assert_eq!(
        super::bind_edge_port_candidates(&service_ctx, &ports, &candidates)
            .expect("bind the edge port candidates"),
        Some(vec![[0, 1]])
    );
}

#[test]
fn raw_and_compressed_visualization_tables_bind_the_same_endpoints() {
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
    for bytes in [&raw, &compressed] {
        let run = || visualization_endpoint_pairs(bytes, &rows, &points);
        assert_eq!(run(), Some(vec![[0, 1]]));
    }
}

#[test]
fn repeated_handle_row_selects_the_second_sharing_face() {
    let rows = vec![row(&[10, 11])];
    let faces = vec![handles(&[10, 11]), handles(&[10, 11])];
    catia_test_context!(service_ctx);
    assert_eq!(
        repeated_edge_face_handle_candidates_from_sets(&service_ctx, &rows, &faces, &[[0, 0]],)
            .expect("compute the repeated edge face handle candidates from sets"),
        Some(vec![vec![1]])
    );
}

#[test]
fn repeated_face_refinement_accepts_any_single_allowed_face() {
    for allowed_face in [vec![2usize], vec![3usize]] {
        let run = || {
            let mut allowed = [allowed_face.clone()];
            refine_repeated_edge_face_candidates(&[[0, 0]], &mut allowed, &[vec![2]])
        };
        assert_eq!(run(), Some(()));
    }
}

#[test]
fn endpoint_degree_closure_closes_a_three_edge_cycle() {
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
    .expect("compute the repeated face endpoint closures")
    .is_some());
}

#[test]
fn duplicate_face_assignment_picks_the_one_passing_face() {
    let serialized = [[0usize, 0]];
    let allowed = [vec![0usize, 1]];
    let run =
        || unique_duplicate_face_assignment(&serialized, &allowed, 2, |faces| Ok(faces[0][1] == 1));
    assert_eq!(
        run().expect("find the unique duplicate face assignment"),
        Some(vec![[0, 1]])
    );
}

#[test]
fn standard_quad_materializes_four_mesh_edge_runs() {
    let bytes = crate::test_support::test_topology::standard_quad_topology_stream();
    let run =
        |ctx: &cadmpeg_core::decode::DecodeContext<'_>| super::standard_mesh_edge_runs(ctx, &bytes);
    assert_eq!(
        crate::test_support::with_service_context(run)
            .expect("parse the standard mesh edge runs")
            .expect("runs")
            .len(),
        4
    );
}
