use super::*;
use cadmpeg_core::decode::ResourceDimension;

#[test]
fn singleton_relation_pair_needs_no_signature_slot_storage() {
    const ROWS: usize = 128;
    let rows = (0..ROWS)
        .map(|_| EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun).expect("row"))
        .collect::<Vec<_>>();
    let faces = vec![[0, 1]; ROWS];
    let geometry = vec![MeshEdgeGeometry::Line; ROWS];
    let candidates = (0..ROWS)
        .map(|edge| vec![[2 * edge.saturating_sub(1), 2 * edge.saturating_sub(1) + 1]])
        .collect::<Vec<_>>();
    let evidence = vec![false; ROWS];
    let gauge = MeshCandidateGauge {
        edge_rows: &rows,
        edge_faces: &faces,
        edge_geometry: &geometry,
        edge_candidates: &candidates,
        edge_identity_evidence: &evidence,
        coordinate_gauge: None,
    };
    let permutation = (0..2 * ROWS).collect::<Vec<_>>();
    let state = |pairs| {
        (
            vec![None; ROWS],
            vec![vec![MeshEndpointRelationSelection::Enumerated {
                assignments: vec![],
                edge_pairs: pairs,
            }]],
        )
    };
    let without = state(Vec::new());
    let cap = crate::test_support::with_service_context(|ctx| {
        assert_eq!(
            relation_row_gauge_mapping(ctx, &without, gauge, &permutation).expect("mapping"),
            Some((0..ROWS).collect())
        );
        let CodecError::ResourceLimit(limit) = ctx
            .alloc_filled(1_000_000_000, 0u8, "measure collection items")
            .expect_err("collection refusal")
        else {
            panic!("refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        limit.used
    });
    // A singleton-only reference adds no collection item to the empty pair lane.
    assert_eq!(
        crate::test_support::with_collection_limit(cap, |ctx| {
            relation_row_gauge_mapping(ctx, &state(vec![(2, [4, 5])]), gauge, &permutation)
        })
        .expect("singleton reference uses the existing row mapping"),
        Some((0..ROWS).collect())
    );
}
