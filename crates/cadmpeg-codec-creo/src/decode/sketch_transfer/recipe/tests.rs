// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{feature_row_schema_classes, row_feature_schema_classes};

fn row(schema_class: crate::feature::schema::SchemaClass) -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(schema_class),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    }
}

#[test]
fn row_schema_classes_refuse_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let rows = [row(crate::feature::schema::SchemaClass::Round)];
    let error = row_feature_schema_classes(&ctx, &rows, 40)
        .expect_err("one class needs one BTreeSet node");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo row schema class nodes"));
}

#[test]
fn feature_schema_classes_refuse_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.depdb_recipe_rows.push(row(crate::feature::schema::SchemaClass::Round));
    let error = feature_row_schema_classes(&ctx, &scan, 40)
        .expect_err("depdb class needs one BTreeSet node");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature schema class nodes"));
}

#[test]
fn feature_schema_classes_keep_distinct_sorted_values() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.rows.push(row(crate::feature::schema::SchemaClass::Round));
    scan.features.depdb_recipe_rows.push(row(crate::feature::schema::SchemaClass::Round));
    scan.features.depdb_recipe_rows.push(row(crate::feature::schema::SchemaClass::Chamfer));
    let classes = crate::decode::with_test_decode_ctx(|ctx| feature_row_schema_classes(ctx, &scan, 40))
        .expect("three rows fit service limits");
    assert_eq!(classes.len(), 2);
    assert!(classes.contains(&crate::feature::schema::SchemaClass::Round));
    assert!(classes.contains(&crate::feature::schema::SchemaClass::Chamfer));
}
