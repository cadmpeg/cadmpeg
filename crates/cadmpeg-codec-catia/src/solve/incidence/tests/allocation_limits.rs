use crate::solve::incidence::{incidence_choice_components, join_incidence_components_by_coupling};
use crate::solve::mesh_quotient::MeshQuotient;
use crate::solve::missing_edge::{
    MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
};
use crate::solve::tests::repeated_domain;
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

#[test]
fn face_configuration_mask_refuses_before_word_and_key_growth() {
    for (cap, operation) in [
        (0, "catia_face_configuration_mask_words"),
        (1, "catia face configuration mask keys"),
    ] {
        let mut masks = HashMap::<usize, Vec<u64>>::new();
        assert!(matches!(
            crate::test_support::with_collection_limit(cap, |ctx| {
                super::super::set_mask_bit(ctx, &mut masks, 3, 0, 1, 1, "catia_face_configuration_mask_words")
            }),
            Err(CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
        assert!(masks.is_empty());
    }
    let mut masks = HashMap::<usize, Vec<u64>>::new();
    crate::test_support::with_service_context(|ctx| {
        super::super::set_mask_bit(ctx, &mut masks, 3, 0, 1, 1, "catia_face_configuration_mask_words")
    }).expect("service resource budget");
    assert_eq!(masks.get(&3), Some(&vec![1]));
}

#[test]
fn face_configuration_support_refuses_collection_growth() {
    let fixture = || vec![
        vec![vec![(0, [0, 1])], vec![(0, [0, 1])]],
        vec![vec![(0, [0, 1])]],
    ];
    let run = |ctx: &DecodeContext<'_>| {
        let mut domains = fixture();
        super::super::prune_face_configuration_support(ctx, &mut domains, &WorkBudget::new(1_000))
    };
    assert!(crate::test_support::with_service_context(run).expect("service resource budget"));
    let mut refused = HashSet::new();
    for cap in 0..128 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => { refused.insert(limit.operation); }
            Ok(true) => break,
            other => panic!("unexpected face support result: {other:?}"),
        }
    }
    for operation in [
        "catia face configuration edge sets",
        "catia face configuration edges",
        "catia face configuration keep marks",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn face_configuration_singleton_refuses_trial_copies() {
    let fixture = || vec![
        vec![vec![(0, [0, 1])], vec![(0, [0, 1])]],
        vec![vec![(0, [0, 1])]],
    ];
    let run = |ctx: &DecodeContext<'_>| {
        let mut domains = fixture();
        super::super::prune_face_configuration_singleton_support(ctx, &mut domains, &WorkBudget::new(1_000))
    };
    assert!(crate::test_support::with_service_context(run).expect("service resource budget"));
    let mut refused = HashSet::new();
    for cap in 0..256 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => { refused.insert(limit.operation); }
            Ok(true) => break,
            other => panic!("unexpected singleton support result: {other:?}"),
        }
    }
    for operation in [
        "catia face configuration singleton order",
        "catia face configuration trial rows",
        "catia face configuration trial masks",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn incidence_degree_adjustment_refuses_before_undo_and_point_growth() {
    for (cap, operation) in [
        (0, "catia incidence degree undo entries"),
        (4, "catia incidence degree points"),
    ] {
        let mut degrees = vec![BTreeMap::new(), BTreeMap::new()];
        assert!(matches!(
            crate::test_support::with_collection_limit(cap, |ctx| {
                super::super::adjust_incidence_degrees(ctx, &mut degrees, &[[0, 1]], 0, [2, 3])
            }),
            Err(CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
        assert!(degrees.iter().all(BTreeMap::is_empty));
    }
    let mut degrees = vec![BTreeMap::new(), BTreeMap::new()];
    let undo = crate::test_support::with_service_context(|ctx| {
        super::super::adjust_incidence_degrees(ctx, &mut degrees, &[[0, 1]], 0, [2, 3])
    }).expect("service resource budget");
    assert_eq!(undo.entries.len(), 4);
    assert_eq!(degrees[0].len(), 2);
    assert_eq!(degrees[1].len(), 2);
}

#[test]
fn deferred_cycle_assignment_refuses_each_inner_collection_limit() {
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
    let run = |ctx: &DecodeContext<'_>| {
        super::super::deferred_boundary_cycle_matches(ctx, &mesh, &incidence, &missing)
    };
    assert!(crate::test_support::with_service_context(run).expect("service resource budget"));
    let mut refused = HashSet::new();
    for cap in 0..32 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => { refused.insert(limit.operation); }
            Ok(true) => break,
            other => panic!("unexpected deferred cycle result: {other:?}"),
        }
    }
    for operation in [
        "catia deferred cycle expected edges",
        "catia deferred cycle actual edges",
        "catia deferred cycle positions",
        "catia deferred cycle boundary uses",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn partial_orientability_refuses_nested_point_and_trail_growth() {
    let edge_faces = [[0, 1], [1, 2], [2, 0]];
    let face_edges = vec![vec![0, 2], vec![0, 1], vec![1, 2]];
    let partial = [Some([0, 1]), Some([1, 2]), None];
    let run = |ctx: &DecodeContext<'_>| {
        super::super::partial_face_orientability_viable(
            ctx, &partial, &edge_faces, &face_edges, &WorkBudget::new(10_000),
        )
    };
    assert!(crate::test_support::with_service_context(run).expect("service resource budget"));
    let mut refused = HashSet::new();
    for cap in 0..256 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => { refused.insert(limit.operation); }
            Ok(true) => break,
            other => panic!("unexpected orientability result: {other:?}"),
        }
    }
    for operation in [
        "catia orientability edge points",
        "catia orientability selected edges",
        "catia orientability point indexes",
        "catia orientability indexed edges",
        "catia orientability traversal stack",
        "catia orientability open trail",
        "catia orientability edge uses",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn incidence_component_graph_refuses_each_collection_limit() {
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
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    assert_eq!(
        incidence_choice_components(&ctx, &choices, &edge_faces, Some(&domains), Some(&quotient))
            .expect("service resource budget"),
        vec![vec![0, 1], vec![2]],
    );

    let mut operations = HashSet::new();
    for cap in 0..=128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits input limit");
        match incidence_choice_components(
            &ctx,
            &choices,
            &edge_faces,
            Some(&domains),
            Some(&quotient),
        ) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                operations.insert(limit.operation);
            }
            Ok(components) => assert_eq!(components, vec![vec![0, 1], vec![2]]),
            Err(error) => panic!("unexpected component refusal: {error}"),
        }
    }
    for operation in [
        "catia_incidence_choice_union",
        "catia_incidence_point_nodes",
        "catia_incidence_fixed_union",
        "catia_incidence_ambiguous_edges",
        "catia_incidence_choice_owner",
        "catia_incidence_boundary_edges",
        "catia_incidence_quotient_owner",
        "catia_incidence_component_edges",
        "catia_incidence_component_roots",
        "catia_incidence_components",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn incidence_component_coupling_refuses_each_collection_limit() {
    let components = vec![vec![0, 2], vec![1], vec![3, 5], vec![4]];
    let active = [true, false, false, true, false, false];
    let expected = vec![vec![0, 2, 3, 5], vec![1], vec![4]];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    assert_eq!(
        join_incidence_components_by_coupling(&ctx, components.clone(), &active)
            .expect("service resource budget"),
        expected,
    );

    let mut operations = HashSet::new();
    for cap in 0..=64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits input limit");
        match join_incidence_components_by_coupling(&ctx, components.clone(), &active) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                operations.insert(limit.operation);
            }
            Ok(joined) => assert_eq!(joined, expected),
            Err(error) => panic!("unexpected coupling refusal: {error}"),
        }
    }
    for operation in [
        "catia_incidence_component_index",
        "catia_incidence_coupling_union",
        "catia_incidence_joined_roots",
        "catia_incidence_joined_edges",
        "catia_incidence_joined_groups",
        "catia_incidence_joined_components",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn boundary_component_graph_refuses_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

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
    catia_test_context!(service_ctx);
    assert_eq!(
        run(&service_ctx).expect("service resource budget"),
        Some(())
    );

    let mut operations = HashSet::new();
    for cap in 0..=256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                operations.insert(limit.operation);
            }
            Ok(Some(())) => {}
            Ok(None) => panic!("closed boundary component must admit a quotient"),
            Err(error) => panic!("unexpected component refusal: {error}"),
        }
    }
    for operation in [
        "catia_boundary_domain_edges",
        "catia_component_active_faces",
        "catia_component_domain_rows",
        "catia_component_active_index",
        "catia_component_union",
        "catia_component_edge_owner",
        "catia_component_face_members",
        "catia_component_face_groups",
        "catia_component_groups",
        "catia_component_selected_edges",
        "catia_component_ordered_faces",
        "catia_quotient_clone_union",
        "catia_quotient_clone_domains",
        "catia_quotient_clone_member_rows",
        "catia_quotient_clone_member_nodes",
        "catia_component_states",
        "catia_component_candidates",
        "catia_component_oriented_edges",
        "catia_component_oriented_signature",
        "catia_quotient_signature_members",
        "catia_quotient_signature_domain",
        "catia_quotient_signature_components",
        "catia_component_signatures",
        "catia_component_alternatives",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn unordered_cycle_search_refuses_each_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

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
    catia_test_context!(service_ctx);
    assert!(!run(&service_ctx)
        .expect("service resource budget")
        .expect("closed cycle assignments")
        .is_empty());

    let mut operations = HashSet::new();
    for cap in 0..=256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                operations.insert(limit.operation);
            }
            Ok(Some(assignments)) => assert!(!assignments.is_empty()),
            Ok(None) => panic!("closed cycle must yield assignments"),
            Err(error) => panic!("unexpected cycle refusal: {error}"),
        }
    }
    for operation in [
        "catia_unordered_sorted_edges",
        "catia_quotient_clone_union",
        "catia_quotient_clone_domains",
        "catia_quotient_clone_member_rows",
        "catia_quotient_clone_member_nodes",
        "catia_unordered_nodes",
        "catia_unordered_compatible",
        "catia_unordered_search_boundary",
        "catia_unordered_completed_boundary",
        "catia_unordered_boundary_rows",
        "catia_unordered_assignments",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}
