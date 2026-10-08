// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{
    feature_row_schema_classes, feature_schema_class, row_feature_schema_classes,
    unique_feature_revolution_extent,
};

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
    let error =
        row_feature_schema_classes(&ctx, &rows, 40).expect_err("one class needs one BTreeSet node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo row schema class nodes")
    );
}

#[test]
fn feature_schema_classes_refuse_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .depdb_recipe_rows
        .push(row(crate::feature::schema::SchemaClass::Round));
    let error = feature_row_schema_classes(&ctx, &scan, 40)
        .expect_err("depdb class needs one BTreeSet node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature schema class nodes")
    );
}

#[test]
fn feature_schema_classes_keep_distinct_sorted_values() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .rows
        .push(row(crate::feature::schema::SchemaClass::Round));
    scan.features
        .depdb_recipe_rows
        .push(row(crate::feature::schema::SchemaClass::Round));
    scan.features
        .depdb_recipe_rows
        .push(row(crate::feature::schema::SchemaClass::Chamfer));
    let classes =
        crate::decode::with_test_decode_ctx(|ctx| feature_row_schema_classes(ctx, &scan, 40))
            .expect("three rows fit service limits");
    assert_eq!(classes.len(), 2);
    assert!(classes.contains(&crate::feature::schema::SchemaClass::Round));
    assert!(classes.contains(&crate::feature::schema::SchemaClass::Chamfer));
}

#[test]
fn feature_schema_class_refuses_before_row_selection() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .rows
        .push(row(crate::feature::schema::SchemaClass::Round));
    let error = feature_schema_class(&ctx, &scan, 40).expect_err("row scan exceeds work limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo feature schema rows")
    );
}

#[test]
fn revolution_extent_lookup_refuses_before_search() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let records = [crate::feature::rows::FeatureRevolutionExtent {
        feature_id: 40,
        offset: 0,
    }];
    let error = unique_feature_revolution_extent(&ctx, &records, 40)
        .expect_err("extent search exceeds work limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo feature revolution extent rows")
    );
}

#[test]
fn schema_conflict_does_not_admit_unused_rows_or_depdb_tail() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows = vec![
        row(crate::feature::schema::SchemaClass::Round),
        row(crate::feature::schema::SchemaClass::Chamfer),
    ];
    let run = |scan: &crate::container::ContainerScan<'_>, cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        feature_schema_class(&ctx, scan, 40)
    };
    let cap = crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| {
        run(&scan, cap)
    });
    assert_eq!(run(&scan, cap).expect("short conflict"), None);
    scan.features
        .rows
        .extend((0..4096).map(|_| row(crate::feature::schema::SchemaClass::Round)));
    scan.features
        .depdb_recipe_rows
        .extend((0..4096).map(|_| row(crate::feature::schema::SchemaClass::Round)));
    assert_eq!(
        run(&scan, cap).expect("conflict stops before both tails"),
        None
    );
}
