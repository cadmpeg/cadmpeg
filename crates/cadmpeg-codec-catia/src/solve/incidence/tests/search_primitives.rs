use crate::solve::missing_edge::MeshFaceBoundaryAssignment;

use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;

use crate::solve::incidence::incidence_choice_components;

use crate::solve::mesh_quotient::MeshQuotient;

use crate::solve::tests::repeated_domain;

use crate::solve::missing_edge::MeshFaceBoundaryDomain;

use crate::solve::incidence::join_incidence_components_by_coupling;

#[test]
fn setting_a_mask_bit_allocates_the_mask_for_a_new_key() {
    use std::collections::HashMap;
    let mut masks = HashMap::<usize, Vec<u64>>::new();
    crate::test_support::with_service_context(|ctx| {
        super::super::set_mask_bit(
            ctx,
            &mut masks,
            3,
            0,
            1,
            1,
            "catia_face_configuration_mask_words",
        )
    })
    .expect("service resource budget");
    assert_eq!(masks.get(&3), Some(&vec![1]));
}

#[test]
fn face_configuration_support_accepts_agreeing_shared_edge_pairs() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
    let fixture = || {
        vec![
            vec![vec![(0, [0, 1])], vec![(0, [0, 1])]],
            vec![vec![(0, [0, 1])]],
        ]
    };
    let run = |ctx: &DecodeContext<'_>| {
        let mut domains = fixture();
        super::super::prune_face_configuration_support(ctx, &mut domains, &WorkBudget::new(1_000))
    };
    assert!(crate::test_support::with_service_context(run).expect("service resource budget"));
}

#[test]
fn face_singleton_support_accepts_agreeing_shared_edge_pairs() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
    let fixture = || {
        vec![
            vec![vec![(0, [0, 1])], vec![(0, [0, 1])]],
            vec![vec![(0, [0, 1])]],
        ]
    };
    let run = |ctx: &DecodeContext<'_>| {
        let mut domains = fixture();
        super::super::prune_face_configuration_singleton_support(
            ctx,
            &mut domains,
            &WorkBudget::new(1_000),
        )
    };
    assert!(crate::test_support::with_service_context(run).expect("service resource budget"));
}

#[test]
fn assigning_an_edge_pair_raises_both_point_degrees_on_both_faces() {
    use std::collections::BTreeMap;
    let mut degrees = vec![BTreeMap::new(), BTreeMap::new()];
    let undo = super::super::adjust_incidence_degrees(&mut degrees, &[[0, 1]], 0, [2, 3]);
    assert_eq!(undo.entries.len(), 4);
    assert_eq!(degrees[0].len(), 2);
    assert_eq!(degrees[1].len(), 2);
}

#[test]
fn deferred_boundary_cycle_matches_its_exact_edge_uses() {
    use std::collections::HashSet;
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 2,
        reversed: None,
    };
    let mesh = crate::solve::missing_edge::MeshDeferredBoundaryCycle {
        length: 2,
        exact_uses: vec![(use_(0, 0), 1), (use_(1, 1), 1)],
    };
    let incidence = [(0, false), (1, false)];
    let missing = HashSet::new();
    let run = || super::super::deferred_boundary_cycle_matches(&mesh, &incidence, &missing);
    assert!(run());
}

#[test]
fn partial_assignment_on_a_face_triangle_is_orientable() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
    let edge_faces = [[0, 1], [1, 2], [2, 0]];
    let face_edges = vec![vec![0, 2], vec![0, 1], vec![1, 2]];
    let partial = [Some([0, 1]), Some([1, 2]), None];
    let run = |ctx: &DecodeContext<'_>| {
        super::super::partial_face_orientability_viable(
            ctx,
            &partial,
            &edge_faces,
            &face_edges,
            &WorkBudget::new(10_000),
        )
    };
    assert!(crate::test_support::with_service_context(run).expect("service resource budget"));
}

#[test]
fn incidence_components_group_edges_whose_choices_share_points() {
    use std::collections::HashSet;
    use std::sync::Arc;
    let choices = [
        vec![[0, 1], [0, 2]],
        vec![[1, 3], [2, 3]],
        vec![[4, 5], [4, 6]],
    ];
    let edge_faces = [[0, 0]; 3];
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let quotient = MeshQuotient::new(
        [
            HashSet::from([0, 1]),
            HashSet::from([0, 2]),
            HashSet::from([1, 3]),
            HashSet::from([3, 4]),
            HashSet::from([6, 7]),
            HashSet::from([6, 8]),
        ]
        .map(Arc::new)
        .to_vec(),
    );

    assert_eq!(
        incidence_choice_components(&choices, &edge_faces, Some(&domains), Some(&quotient))
            .expect("service resource budget"),
        vec![vec![0, 1], vec![2]],
    );
}

#[test]
fn coupled_edges_join_their_incidence_components() {
    let components = vec![vec![0, 2], vec![1], vec![3, 5], vec![4]];
    let active = [true, false, false, true, false, false];
    let expected = vec![vec![0, 2, 3, 5], vec![1], vec![4]];

    assert_eq!(
        join_incidence_components_by_coupling(components.clone(), &active)
            .expect("service resource budget"),
        expected,
    );
}

#[test]
fn common_boundary_propagation_succeeds_for_a_single_ordered_face() {
    use cadmpeg_core::decode::DecodeContext;
    use std::collections::HashSet;
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                },
            ]],
        },
    ])];
    let candidates = vec![Vec::new(), Vec::new()];
    let run = |ctx: &DecodeContext<'_>| {
        let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), 4));
        crate::solve::mesh_quotient::propagate_common_boundary_components(
            ctx,
            &domains,
            &candidates,
            &mut quotient,
        )
    };
    assert_eq!(
        crate::test_support::with_service_context(run).expect("service resource budget"),
        Some(())
    );
}

#[test]
fn unordered_cycle_search_closes_a_triangle() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
    use std::collections::HashSet;
    use std::sync::Arc;
    let quotient = MeshQuotient::new(
        [0, 1, 1, 2, 2, 0]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
    );
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(10_000);
        crate::solve::mesh_quotient::bounded_unordered_cycle_assignments(
            ctx,
            &[0, 1, 2],
            &quotient,
            16,
            &budget,
        )
    };
    assert!(!crate::test_support::with_service_context(run)
        .expect("service resource budget")
        .expect("closed cycle assignments")
        .is_empty());
}
