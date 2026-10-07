// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

/// Storage of a hash table grown from empty to three entries: four buckets of
/// `element` bytes, 15 bytes of group padding (alignment 16), four control
/// bytes and a 16-byte trailer. Admitting the growth holds exactly this as a
/// transient reservation, and the retained growth that follows charges the
/// same bytes, the old table being empty.
fn first_table(element: usize) -> u64 {
    u64::try_from(4 * element + 15 + 4 + 16).expect("table size fits")
}

fn assignment_refusal(materialized: u64) -> cadmpeg_core::decode::ResourceLimit {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = materialized;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    match super::super::bipartite_assignment(&[vec![17]], None, &ctx) {
        Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => limit,
        other => panic!("expected a resource refusal, got {other:?}"),
    }
}

#[test]
fn edge_assignment_visits_refuse_materialized_limit() {
    // The visited set of the one candidate edge is a three-entry `i64` table.
    let visited = first_table(std::mem::size_of::<i64>());
    let limit = assignment_refusal(visited - 1);
    assert_eq!(
        (
            limit.dimension,
            limit.operation,
            limit.used,
            limit.additional
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d edge assignment visits",
            0,
            visited
        )
    );
}

#[test]
fn edge_assignment_members_refuse_materialized_limit() {
    // The visited set stays live as scoped storage for the whole search, so the
    // member table's growth is admitted behind it.
    let visited = first_table(std::mem::size_of::<i64>());
    let members = first_table(std::mem::size_of::<(i64, usize)>());

    // One byte under the visited table: its growth is the first refusal.
    let limit = assignment_refusal(visited - 1);
    assert_eq!(
        (
            limit.dimension,
            limit.operation,
            limit.used,
            limit.additional
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d edge assignment visits",
            0,
            visited
        )
    );
    // One byte under visited plus member table: the member growth is refused
    // behind the live visited table.
    let limit = assignment_refusal(visited + members - 1);
    assert_eq!(
        (
            limit.dimension,
            limit.operation,
            limit.used,
            limit.additional
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d edge assignment members",
            visited,
            members
        )
    );
    // Exactly both tables is the whole need: the assignment completes.
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = visited + members;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        super::super::bipartite_assignment(&[vec![17]], None, &ctx).expect("both tables fit"),
        Some(vec![17])
    );
}

#[test]
fn edge_assignment_search_refuses_recursion_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bipartite_assignment(&[vec![17]], None, &ctx)
        .expect_err("one search frame needs one recursion level");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RecursionDepth
                && limit.operation == "f3d edge assignment search"
    ));
}

#[test]
fn edge_assignment_candidate_refuses_work_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bipartite_assignment(&[vec![17]], None, &ctx)
        .expect_err("one candidate needs one work unit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "f3d edge assignment candidate"
    ));
}
