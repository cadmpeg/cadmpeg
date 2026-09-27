use crate::solve::incidence::{incidence_choice_components, join_incidence_components_by_coupling};
use crate::solve::mesh_quotient::MeshQuotient;
use crate::solve::missing_edge::{
    MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashSet;
use std::sync::Arc;

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
