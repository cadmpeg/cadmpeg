// SPDX-License-Identifier: Apache-2.0
//! Resource admission for mesh orientation and edge-domain search.

use super::super::MeshQuotient;
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashSet;
use std::sync::Arc;

#[test]
fn mesh_orientation_options_refuse_collection_limit_before_declining() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    };
    let quotient = MeshQuotient::new(
        [vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    let run = |ctx: &DecodeContext<'_>| {
        quotient.assignment_options_limited(
            ctx,
            &assignment,
            &[vec![[0, 1]]],
            &HashSet::new(),
            1,
            None,
        )
    };
    catia_test_context!(service_ctx);
    assert!(run(&service_ctx)
        .expect("service resource budget")
        .is_empty());

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited_ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let Err(CodecError::ResourceLimit(error)) = run(&limited_ctx) else {
        panic!("orientation allocation must refuse the limit");
    };
    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
    assert_eq!(error.operation, "catia_orientation_direction_union");
}

#[test]
fn quotient_edge_domains_refuse_materialized_limit_before_declining() {
    let mut quotient = MeshQuotient::new(
        [vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    catia_test_context!(service_ctx);
    assert!(!quotient
        .edge_domains_viable(&service_ctx, &[vec![[2, 3]]])
        .expect("service resource budget"));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (limited_ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let Err(CodecError::ResourceLimit(error)) =
        quotient.edge_domains_viable(&limited_ctx, &[vec![[2, 3]]])
    else {
        panic!("edge domain allocation must refuse the limit");
    };
    assert_eq!(error.dimension, ResourceDimension::MaterializedBytes);
}

#[test]
fn fixed_mesh_directions_refuse_collection_limit_before_declining() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    };
    let quotient = MeshQuotient::new(
        [vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    let options = vec![vec![vec![false]]];
    catia_test_context!(service_ctx);
    assert!(quotient
        .assignment_options_for_directions(&service_ctx, &assignment, &options, 1, None)
        .expect("service resource budget")
        .is_empty());

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited_ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let Err(CodecError::ResourceLimit(error)) =
        quotient.assignment_options_for_directions(&limited_ctx, &assignment, &options, 1, None)
    else {
        panic!("fixed direction allocation must refuse the limit");
    };
    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
    assert_eq!(error.operation, "catia_quotient_clone_union");
}

#[test]
fn label_directions_refuse_collection_limit_before_declining() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    };
    let mut quotient = MeshQuotient::new(
        [vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    catia_test_context!(service_ctx);
    assert!(quotient
        .merge_label_directions_in_place(
            &service_ctx,
            &assignment,
            &[vec![false]],
            &[Some(false)],
            None,
        )
        .expect("service resource budget")
        .is_none());

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (limited_ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let Err(CodecError::ResourceLimit(error)) = quotient.merge_label_directions_in_place(
        &limited_ctx,
        &assignment,
        &[vec![false]],
        &[Some(false)],
        None,
    ) else {
        panic!("label direction allocation must refuse the limit");
    };
    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
    assert_eq!(error.operation, "catia_label_direction_rows");
}
