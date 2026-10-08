// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn empty_feature_operation_routes_preserve_original_refusal_without_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(super::current_feature_operation(&ctx, &[], 40).expect("empty selection").is_none());
    assert!(super::current_feature_recipe(&ctx, &[], 40).expect("empty recipe").is_none());
    assert!(super::current_additive_feature_recipe(&ctx, &[], 40)
        .expect("empty additive recipe").is_none());
    assert!(super::current_feature_recipe_parent(&ctx, &[], 40)
        .expect("empty recipe parent").is_none());
    let original = ctx.charge_work_limit(1, "prior recipe refusal").expect_err("refusal");
    for error in [
        super::current_feature_operation(&ctx, &[], 40).map(|_| ()),
        super::current_feature_recipe(&ctx, &[], 40).map(|_| ()),
        super::current_additive_feature_recipe(&ctx, &[], 40).map(|_| ()),
        super::current_feature_recipe_parent(&ctx, &[], 40).map(|_| ()),
    ] {
        assert!(matches!(error, Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal == original));
    }
}

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


fn operation(feature_id: u32) -> crate::feature::operations::FeatureOperation {
    crate::feature::operations::FeatureOperation {
        feature_id,
        kind: crate::feature::operations::OperationKind::Extrude,
        name: crate::feature::operations::OperationName::Derived,
        recipe: crate::feature::operations::RecipeResolution::Resolved(
            crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
        ),
        display_state_conflict: false,
        depdb: Some(crate::feature::operations::DepdbPrefix {
            schema: crate::feature::schema::SchemaClass::Protrusion, parent: 7,
        }),
        offset: 0,
        state_offset: 0,
    }
}

#[test]
fn feature_operation_selector_visits_each_present_record_once() {
    let records = [operation(11), operation(40), operation(12)];
    for cap in [2, 3] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = super::current_feature_operation(&ctx, &records, 40);
        if cap == 3 {
            assert!(std::ptr::eq(result.expect("three visits").expect("unique"), &records[1]));
        } else {
            let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result else {
                panic!("third visit must refuse");
            };
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!(refusal.operation, "creo current feature operation rows");
            assert_eq!(refusal.used, 2);
            assert_eq!(refusal.additional, 1);
            assert!(matches!(super::current_feature_recipe(&ctx, &[], 40),
                Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == refusal));
        }
    }
    for count in 0..4 {
        let records = vec![operation(11); count];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(count).expect("fixed test count");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(super::current_feature_operation(&ctx, &records, 40)
            .expect("only present visits").is_none());
    }
}

#[test]
fn feature_operation_selector_stops_at_the_second_match() {
    let mut records = vec![operation(40), operation(40)];
    records.extend((0..64).map(|_| operation(11)));
    for cap in [1, 2] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = super::current_feature_operation(&ctx, &records, 40);
        if cap == 2 {
            assert!(result.expect("two matching visits").is_none());
        } else {
            assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == "creo current feature operation rows"
                    && refusal.used == 1 && refusal.additional == 1));
        }
    }
}

#[test]
fn sweep_conflict_discriminant_does_not_traverse_stored_kind_text() {
    use crate::feature::operations::OperationKind;
    for (kind, expected) in [
        (OperationKind::Native, true),
        (OperationKind::Extrude, false),
        (OperationKind::Revolve, false),
        (OperationKind::Stored("arbitrary stored family".repeat(256)), false),
    ] {
        let mut scan = crate::test_support::empty_container_scan();
        let mut selected = operation(40);
        selected.kind = kind;
        selected.recipe = crate::feature::operations::RecipeResolution::None;
        selected.display_state_conflict = true;
        scan.features.operations.push(selected);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(super::feature_section_sweep_semantics_conflict(&ctx, &scan, 40)
            .expect("one record visit and fixed variant tag"), expected);
    }
}
