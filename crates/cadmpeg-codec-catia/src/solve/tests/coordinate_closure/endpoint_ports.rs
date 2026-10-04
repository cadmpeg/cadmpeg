// SPDX-License-Identifier: Apache-2.0
//! endpoint ports tests.

use super::{
    bind_edge_port_candidates, propagate_edge_port_points, resolve_edge_faces_from_runs,
    same_unordered_pair, unique_coordinate_bijection, unique_duplicate_face_assignment,
    unique_mesh_edge_port_candidate_pairs, unique_mesh_edge_port_candidate_pairs_with_deferred,
    visit_duplicate_face_assignments, DuplicateFaceAssignmentVisit, HashSet, MeshEdgeRun,
};

#[test]
fn duplicate_face_assignment_visitor_reports_the_bound() {
    catia_test_context!(ctx);
    let serialized = [[0, 0], [0, 0]];
    let allowed = [vec![1, 2], vec![1, 2]];
    let mut visits = 0;

    let outcome = visit_duplicate_face_assignments(&ctx, &serialized, &allowed, 3, 3, |_| {
        visits += 1;
        Ok(true)
    })
    .expect("service resource budget");

    assert_eq!(outcome, Some(DuplicateFaceAssignmentVisit::Exhausted));
    assert_eq!(visits, 3);
}

#[test]
fn duplicate_face_visitor_refuses_before_assignment_and_choice_storage() {
    let serialized = [[0usize, 0]];
    let allowed = [vec![0usize, 1]];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        visit_duplicate_face_assignments(ctx, &serialized, &allowed, 2, 4, |_| Ok(true))
    };
    assert_eq!(
        crate::test_support::with_service_context(run).expect("service budget"),
        Some(DuplicateFaceAssignmentVisit::Complete)
    );
    let mut operations = std::collections::HashSet::new();
    for limit in 0..=32 {
        match crate::test_support::with_collection_limit(limit, run) {
            Err(cadmpeg_core::CodecError::ResourceLimit(error)) => {
                operations.insert(error.operation);
            }
            Ok(Some(DuplicateFaceAssignmentVisit::Complete)) => break,
            outcome => panic!("unexpected duplicate visit outcome: {outcome:?}"),
        }
    }
    for operation in [
        "catia_duplicate_visit_assignment",
        "catia_duplicate_visit_choices",
        "catia_duplicate_visit_branches",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn one_admitted_alternate_does_not_force_a_second_face() {
    const EDGE_COUNT: usize = 8;
    catia_test_context!(ctx);
    let serialized = vec![[0, 0]; EDGE_COUNT];
    let allowed = vec![vec![1, 1]; EDGE_COUNT];

    assert!(
        unique_duplicate_face_assignment(&ctx, &serialized, &allowed, 2, |_| Ok(true))
            .expect("service resource budget")
            .is_none()
    );
}

#[test]
fn exact_mesh_occurrences_complete_duplicate_face_slot() {
    catia_test_context!(ctx);
    let run = |edge, face| MeshEdgeRun {
        edge,
        face,
        cycle: 0,
        start: 0,
        segment_count: 1,
        reversed: false,
    };
    let faces = resolve_edge_faces_from_runs(
        &ctx,
        &[[1, 1], [2, 2], [3, 4]],
        &[run(0, 1), run(0, 5), run(1, 2), run(2, 3), run(2, 4)],
    )
    .expect("service resource budget")
    .expect("consistent exact face occurrences");

    assert_eq!(faces, vec![[1, 5], [2, 2], [3, 4]]);
}

#[test]
fn one_mesh_occurrence_keeps_duplicate_face_slot_unresolved() {
    catia_test_context!(ctx);
    let run = MeshEdgeRun {
        edge: 0,
        face: 1,
        cycle: 0,
        start: 0,
        segment_count: 1,
        reversed: false,
    };

    let faces = resolve_edge_faces_from_runs(&ctx, &[[1, 1]], &[run])
        .expect("service resource budget")
        .expect("a single occurrence does not conflict with the serialized wildcard");

    assert_eq!(faces, vec![[1, 1]]);
}

#[test]
fn ambiguous_mesh_occurrences_defer_duplicate_face_slot() {
    catia_test_context!(ctx);
    let run = |face| MeshEdgeRun {
        edge: 0,
        face,
        cycle: 0,
        start: 0,
        segment_count: 1,
        reversed: false,
    };

    let faces = resolve_edge_faces_from_runs(&ctx, &[[1, 1]], &[run(1), run(5), run(6)])
        .expect("service resource budget")
        .expect("ambiguous occurrences remain a deferred face domain");

    assert_eq!(faces, vec![[1, 1]]);
}

#[test]
fn endpoint_ports_reject_contradictory_pair_constraints() {
    let ports = [[10, 11], [11, 12], [12, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), Some([0, 3])];
    assert_eq!(
        crate::test_support::with_service_context(|ctx| propagate_edge_port_points(
            ctx, &ports, &pairs
        ))
        .expect("service resource budget"),
        None
    );
}

#[test]
fn native_edge_identities_bind_ambiguous_coordinate_pairs() {
    let ports = [[10, 11], [12, 13], [10, 12], [11, 13]];
    let candidates = [vec![[0, 1]], vec![[2, 3]], vec![[0, 2]], vec![[1, 3]]];
    assert_eq!(
        crate::test_support::with_service_context(|ctx| bind_edge_port_candidates(
            ctx,
            &ports,
            &candidates
        ))
        .expect("service resource budget"),
        Some(vec![[0, 1], [2, 3], [0, 2], [1, 3]])
    );
}

#[test]
fn mesh_edge_ports_allow_one_coordinate_row_at_multiple_ports() {
    let ports = [[10, 11], [12, 13]];
    let candidates = [vec![[0, 1]], vec![[0, 2]]];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| unique_mesh_edge_port_candidate_pairs(
            ctx,
            &ports,
            &candidates
        ))
        .expect("service resource budget"),
        Some(vec![[0, 1], [0, 2]])
    );
}

#[test]
fn mesh_edge_ports_reject_multiple_unordered_assignments() {
    let ports = [[10, 11]];
    let candidates = [vec![[0, 1], [0, 2]]];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| unique_mesh_edge_port_candidate_pairs(
            ctx,
            &ports,
            &candidates
        ))
        .expect("service resource budget"),
        None
    );
}

#[test]
fn mesh_edge_ports_resolve_shared_port_without_point_bijection() {
    let ports = [[10, 11], [10, 12], [11, 13]];
    let candidates = [vec![[0, 1]], vec![[0, 2]], vec![[1, 3]]];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| unique_mesh_edge_port_candidate_pairs(
            ctx,
            &ports,
            &candidates
        ))
        .expect("service resource budget"),
        Some(vec![[0, 1], [0, 2], [1, 3]])
    );
}

#[test]
fn deferred_mesh_edge_ports_do_not_constrain_settled_rows() {
    let ports = [[10, 11], [20, 21]];
    let candidates = [vec![[0, 1]], vec![[2, 3]]];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            unique_mesh_edge_port_candidate_pairs_with_deferred(
                ctx,
                &ports,
                &candidates,
                &[false, true],
            )
        })
        .expect("service resource budget"),
        Some(vec![Some([0, 1]), None])
    );
}

#[test]
fn deferred_mesh_edge_port_components_leave_all_connected_rows_unresolved() {
    let ports = [[10, 11], [11, 10], [12, 10], [13, 11], [10, 97], [11, 98]];
    let candidates = [
        vec![[0, 1]],
        vec![[0, 1]],
        vec![[0, 2]],
        vec![[0, 3], [1, 3]],
        vec![[0, 79]],
        vec![[0, 79]],
    ];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            unique_mesh_edge_port_candidate_pairs_with_deferred(
                ctx,
                &ports,
                &candidates,
                &[true, false, false, false, false, false],
            )
        })
        .expect("service resource budget"),
        Some(vec![None, None, None, None, None, None]),
    );
}

#[test]
fn native_edge_identities_reject_multiple_coordinate_bijections() {
    let ports = [[10, 11]];
    let candidates = [vec![[0, 1], [2, 3]]];
    assert_eq!(
        crate::test_support::with_service_context(|ctx| bind_edge_port_candidates(
            ctx,
            &ports,
            &candidates
        ))
        .expect("service resource budget"),
        None
    );
}

#[test]
fn native_edge_identities_preserve_endpoint_equality() {
    assert_eq!(
        crate::test_support::with_service_context(|ctx| bind_edge_port_candidates(
            ctx,
            &[[10, 11]],
            &[vec![[0, 0]]]
        ))
        .expect("service resource budget"),
        None
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| bind_edge_port_candidates(
            ctx,
            &[[10, 10]],
            &[vec![[0, 1]]]
        ))
        .expect("service resource budget"),
        None
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| bind_edge_port_candidates(
            ctx,
            &[[10, 10]],
            &[vec![[0, 0]]]
        ))
        .expect("service resource budget"),
        Some(vec![[0, 0]])
    );
}

#[test]
fn native_edge_identities_bind_independent_components_with_local_budgets() {
    const COMPONENT_COUNT: usize = 100;
    let ports = (0..COMPONENT_COUNT)
        .map(|component| {
            let port = u32::try_from(component * 2).expect("bounded port identity");
            [port, port + 1]
        })
        .collect::<Vec<_>>();
    let candidates = (0..COMPONENT_COUNT)
        .map(|component| vec![[component * 2, component * 2 + 1]])
        .collect::<Vec<_>>();

    let solution = crate::test_support::with_service_context(|ctx| {
        bind_edge_port_candidates(ctx, &ports, &candidates)
    })
    .expect("service resource budget")
    .expect("independent port components");

    assert_eq!(solution.len(), COMPONENT_COUNT);
    assert!(solution
        .iter()
        .zip(&candidates)
        .all(|(pair, candidates)| same_unordered_pair(*pair, candidates[0])));
}

#[test]
fn native_edge_identities_do_not_charge_forced_chain_depth() {
    const EDGE_COUNT: usize = 10_000;
    let ports = (0..EDGE_COUNT)
        .map(|edge| {
            let port = u32::try_from(edge).expect("bounded port identity");
            [port, port + 1]
        })
        .collect::<Vec<_>>();
    let candidates = (0..EDGE_COUNT)
        .map(|edge| vec![[edge, edge + 1]])
        .collect::<Vec<_>>();

    let solution = crate::test_support::with_service_context(|ctx| {
        bind_edge_port_candidates(ctx, &ports, &candidates)
    })
    .expect("service resource budget")
    .expect("forced connected port chain");

    assert_eq!(
        solution,
        candidates.into_iter().flatten().collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_coordinate_rows_have_one_geometric_bijection() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[0],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("matching fixture fits the service profile");
    let domains = [HashSet::from([0, 1]), HashSet::from([0, 1])];
    assert_eq!(
        unique_coordinate_bijection(&ctx, &domains, &[[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]])
            .expect("bijection fits the service profile"),
        Some(vec![0, 1])
    );
}

#[test]
fn forced_coordinate_bijection_has_no_recursive_depth_limit() {
    const POINT_COUNT: usize = 10_000;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[0],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("matching fixture fits the service profile");
    let domains = (0..POINT_COUNT)
        .map(|point| HashSet::from([point]))
        .collect::<Vec<_>>();
    let points = (0..POINT_COUNT)
        .map(|point| {
            [
                f64::from(u32::try_from(point).expect("bounded point index")),
                0.0,
                0.0,
            ]
        })
        .collect::<Vec<_>>();

    assert_eq!(
        unique_coordinate_bijection(&ctx, &domains, &points)
            .expect("bijection fits the service profile"),
        Some((0..POINT_COUNT).collect())
    );
}

#[test]
fn coordinate_bijection_respects_duplicate_class_capacity() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[0],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("matching fixture fits the service profile");
    let domains = [
        HashSet::from([0, 2]),
        HashSet::from([0, 1]),
        HashSet::from([0, 1]),
    ];
    let points = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];

    assert_eq!(
        unique_coordinate_bijection(&ctx, &domains, &points)
            .expect("bijection fits the service profile"),
        Some(vec![2, 0, 1])
    );
}

#[test]
fn distinct_coordinate_bijections_remain_ambiguous() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[0],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("matching fixture fits the service profile");
    let domains = [HashSet::from([0, 1]), HashSet::from([0, 1])];
    assert_eq!(
        unique_coordinate_bijection(&ctx, &domains, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
            .expect("bijection fits the service profile"),
        None
    );
}
