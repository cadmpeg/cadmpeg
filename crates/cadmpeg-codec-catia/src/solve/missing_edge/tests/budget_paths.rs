//! Indexed occurrence, endpoint closure, and port propagation regressions.

use super::*;
use cadmpeg_core::CodecError;

#[test]
fn absent_deferred_ports_need_no_adjacency_storage() {
    let ports = [[0, 1]; 128];
    let mut deferred = [false; 128];
    crate::test_support::with_collection_limit(0, |ctx| {
        assert!(
            super::super::expand_deferred_edge_port_components(ctx, &ports, &mut deferred)
                .expect("no deferred seed means no adjacency consumer")
        );
    });
    assert_eq!(deferred, [false; 128]);
}

#[test]
fn indexed_occurrences_preserve_single_handles_pairs_and_precedence() {
    let rows = [
        EdgeRow::new(
            1,
            vec![9, 0, 9],
            EdgeBoundaryLayout::InteriorWithFlankingCorners,
        )
        .expect("single pattern"),
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun).expect("pair pattern"),
        EdgeRow::new(1, vec![0, 1, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("palindrome"),
    ];
    let cycles = [vec![vec![0, 1]]];
    // A two-handle cycle contains both the forward [0,1] at start zero
    // and reversed [1,0] at start one; that row has two occurrences.
    assert!(crate::test_support::with_service_context(|ctx| {
        super::super::mesh_edge_occurrences(ctx, &rows, &cycles)
    })
    .expect("duplicate-cycle admission")
    .is_none());
    let [single, pair, palindrome] = rows;
    let rows = [single, palindrome];
    let occurrences = crate::test_support::with_service_context(|ctx| {
        super::super::mesh_edge_occurrences(ctx, &rows, &cycles)
    })
    .expect("indexed single and palindrome")
    .expect("one occurrence per cycle");
    assert_eq!(
        occurrences
            .iter()
            .map(|runs| runs.len())
            .collect::<Vec<_>>(),
        [1, 1]
    );
    assert!(occurrences.iter().all(|runs| !runs[0].reversed));
    assert_eq!(occurrences[0][0].start, 1);
    assert_eq!(occurrences[1][0].start, 0);
    assert_eq!(occurrences[1][0].segment_count, 2);
    let pair_cycles = [vec![vec![0, 1, 2]]];
    let occurrences = crate::test_support::with_service_context(|ctx| {
        super::super::mesh_edge_occurrences(ctx, &[pair], &pair_cycles)
    })
    .expect("indexed pair")
    .expect("one directed pair occurrence");
    assert_eq!(occurrences[0].len(), 1);
    assert_eq!(occurrences[0][0].start, 0);
    assert_eq!(occurrences[0][0].segment_count, 1);
    assert!(!occurrences[0][0].reversed);
}

#[test]
fn pair_occurrence_index_matches_triangle_fan_in_source_order() {
    let count = 64_u32;
    let rows: Vec<_> = (1..=count)
        .map(|point| {
            EdgeRow::new(1, vec![0, point], EdgeBoundaryLayout::CompleteBoundaryRun).expect("pair")
        })
        .collect();
    let cycles: Vec<_> = (1..=count)
        .map(|point| vec![vec![0, point, point + 1]])
        .collect();
    let occurrences = crate::test_support::with_service_context(|ctx| {
        super::super::mesh_edge_occurrences(ctx, &rows, &cycles)
    })
    .expect("indexed fan")
    .expect("occurrences");
    for (edge, runs) in occurrences.iter().enumerate() {
        let expected_faces: Vec<_> = if edge == 0 {
            vec![0]
        } else {
            vec![edge - 1, edge]
        };
        assert_eq!(
            runs.iter().map(|run| run.face).collect::<Vec<_>>(),
            expected_faces
        );
        assert!(!runs.last().expect("forward run").reversed);
    }
}

#[test]
fn endpoint_closure_indexes_independent_forced_deficits() {
    let mut faces = Vec::new();
    let mut allowed = Vec::new();
    let mut pairs = Vec::new();
    let mut expected = Vec::new();
    for group in 0..16 {
        let face = group * 2;
        let point = group * 3;
        faces.extend([[face, face + 1], [face, face], [face, face + 1]]);
        allowed.extend([vec![], vec![face + 1], vec![]]);
        pairs.extend([
            [point, point + 1],
            [point + 1, point + 2],
            [point + 2, point],
        ]);
        expected.extend([[face, face + 1]; 3]);
    }
    let solutions = crate::test_support::with_service_context(|ctx| {
        repeated_face_endpoint_closures(ctx, &faces, &allowed, &pairs, 32)
    })
    .expect("indexed forced deficits")
    .expect("complete closure");
    assert_eq!(solutions, vec![expected]);
}

#[test]
fn forced_port_chain_revisits_only_bound_neighbors() {
    let count = 128_u32;
    let mut ports: Vec<_> = (0..count).map(|port| [port, port + 1]).collect();
    let mut candidates: Vec<_> = (0..usize::try_from(count).expect("small count"))
        .map(|point| vec![[point, point + 1]])
        .collect();
    ports.push([count, count]);
    let point = usize::try_from(count).expect("small count");
    candidates.push(vec![[point, point]]);
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::bind_edge_port_candidates(ctx, &ports, &candidates)
    };
    let expected: Vec<_> = candidates.iter().map(|pairs| pairs[0]).collect();
    assert_eq!(
        crate::test_support::with_service_context(run).expect("forced chain"),
        Some(expected)
    );
    let refused = crate::test_support::with_work_refusal("catia_port_search_queue", run);
    assert!(matches!(refused, Err(CodecError::ResourceLimit(_))));
}

#[test]
fn empty_endpoint_domains_are_rejected_before_byte_parsing() {
    crate::test_support::with_collection_limit(0, |ctx| {
        assert!(super::super::standard_mesh_prune_endpoint_candidates(
            ctx,
            &[],
            &[[0, 0]],
            &[Vec::new()]
        )
        .expect("empty domains need no parsing or all-pair expansion")
        .is_none());
    });
}
