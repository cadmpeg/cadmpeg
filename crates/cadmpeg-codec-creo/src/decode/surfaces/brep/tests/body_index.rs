// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{merge_body_components, used_brep_vertices, BrepBodyIndexes, NeutralShellSpec};

fn index_result(limit: u64) -> Result<BrepBodyIndexes, CodecError> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.layout = crate::container::Layout::Nd;
    let component = crate::decode::with_test_decode_ctx(|ctx| {
        crate::topology::FaceComponent::new_for_test(ctx, vec![5], vec![10])
    })
    .expect("component admission")
    .expect("valid component fixture");
    let admitted_components = [&component];
    let admitted_edge_curves = BTreeSet::from([10]);
    let eligible_faces = BTreeMap::from([(5, Vec::new())]);
    let eligible_face_ids = BTreeSet::from([5]);
    let curve_faces = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    BrepBodyIndexes::from_components(
        &ctx,
        &scan,
        &admitted_components,
        &admitted_edge_curves,
        &eligible_faces,
        &eligible_face_ids,
        &curve_faces,
    )
}

fn assert_refusal(error: &CodecError, operation: &'static str) {
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn brep_neutral_edge_curve_nodes_refuse_collection_limit() {
    assert_refusal(
        &index_result(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo B-rep neutral edge curve nodes"),
            index_result,
        ))
        .err()
        .expect("node refused"),
        "creo B-rep neutral edge curve nodes",
    );
}

#[test]
fn brep_component_face_ids_refuse_collection_limit() {
    assert_refusal(
        &index_result(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo B-rep component face IDs"),
            index_result,
        ))
        .err()
        .expect("face refused"),
        "creo B-rep component face IDs",
    );
}

#[test]
fn brep_component_wire_nodes_refuse_collection_limit() {
    assert_refusal(
        &index_result(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo B-rep component wire nodes"),
            index_result,
        ))
        .err()
        .expect("wire refused"),
        "creo B-rep component wire nodes",
    );
}

#[test]
fn brep_component_records_refuse_collection_limit() {
    assert_refusal(
        &index_result(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo B-rep component records"),
            index_result,
        ))
        .err()
        .expect("record refused"),
        "creo B-rep component records",
    );
}

#[test]
fn brep_used_vertex_nodes_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo B-rep used vertex nodes"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            used_brep_vertices(&ctx, &BTreeSet::from([10]), &BTreeMap::from([(10, [1, 2])]))
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = used_brep_vertices(&ctx, &BTreeSet::from([10]), &BTreeMap::from([(10, [1, 2])]))
        .expect_err("vertex refused");
    assert_refusal(&error, "creo B-rep used vertex nodes");
}

#[test]
fn brep_body_indexes_preserve_service_values() {
    let indexes = index_result(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        None,
        index_result,
    ))
    .expect("service body indexes admitted");
    assert_eq!(indexes.neutral_edge_curves, BTreeSet::from([10]));
    assert_eq!(
        indexes.body_components,
        vec![NeutralShellSpec {
            faces: vec![5],
            wire_curves: BTreeSet::from([10]),
        }]
    );
    let vertices = crate::decode::with_test_decode_ctx(|ctx| {
        used_brep_vertices(
            ctx,
            &indexes.neutral_edge_curves,
            &BTreeMap::from([(10, [1, 2])]),
        )
    })
    .expect("service vertices admitted");
    assert_eq!(vertices, BTreeSet::from([1, 2]));
}

fn merge_result(limit: u64) -> Result<Vec<NeutralShellSpec>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    merge_body_components(
        &ctx,
        vec![
            NeutralShellSpec {
                faces: vec![1],
                wire_curves: BTreeSet::from([10]),
            },
            NeutralShellSpec {
                faces: vec![2],
                wire_curves: BTreeSet::from([11]),
            },
        ],
    )
}

#[test]
fn brep_merged_component_faces_refuse_collection_limit() {
    assert_refusal(
        &merge_result(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo B-rep merged component faces"),
            merge_result,
        ))
        .expect_err("face refused"),
        "creo B-rep merged component faces",
    );
}

#[test]
fn brep_merged_component_wire_nodes_refuse_collection_limit() {
    assert_refusal(
        &merge_result(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo B-rep merged component wire nodes"),
            merge_result,
        ))
        .expect_err("wire refused"),
        "creo B-rep merged component wire nodes",
    );
}

#[test]
fn brep_merged_component_records_refuse_collection_limit() {
    assert_refusal(
        &merge_result(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo B-rep merged component records"),
            merge_result,
        ))
        .expect_err("record refused"),
        "creo B-rep merged component records",
    );
}
