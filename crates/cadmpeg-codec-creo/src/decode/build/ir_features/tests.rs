// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{admit_new_feature_id, ordered_row_feature_ids};

#[test]
fn row_feature_ids_preserve_first_source_order() {
    let rows = [
        crate::feature::rows::FeatureRow {
            feature_id: 40,
            root_schema_class: None,
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: 30,
            offset: 20,
        },
        crate::feature::rows::FeatureRow {
            feature_id: 12,
            root_schema_class: None,
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: 50,
            offset: 40,
        },
        crate::feature::rows::FeatureRow {
            feature_id: 40,
            root_schema_class: None,
            stream_offset: 0,
            body: vec![0; 2].try_into().expect("row body"),
            body_offset: 70,
            offset: 60,
        },
    ];

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| ordered_row_feature_ids(ctx, &rows))
            .expect("row IDs fit service limits"),
        vec![40, 12]
    );
}

fn one_feature_row() -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: None,
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 30,
        offset: 20,
    }
}

#[test]
fn feature_row_identity_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = ordered_row_feature_ids(&ctx, &[one_feature_row()])
        .expect_err("one distinct row needs one identity node");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature row identity nodes"));
}

#[test]
fn feature_row_id_refuses_before_vec_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = ordered_row_feature_ids(&ctx, &[one_feature_row()])
        .expect_err("one distinct row also needs one output slot");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature row IDs"));
}

#[test]
fn operation_feature_identity_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut ids = BTreeSet::new();
    let error = admit_new_feature_id(
        &ctx,
        &mut ids,
        40,
        "creo operation feature identity nodes",
    )
    .expect_err("one operation needs one identity node");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo operation feature identity nodes"));
    assert!(ids.is_empty());
}
