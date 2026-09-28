// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{
    admit_new_feature_id, merge_feature_dependencies, merge_feature_source_properties, ordered_row_feature_ids,
    refresh_feature_outputs, emit_model_features,
};

fn feature_for_output_refresh() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
        FeatureId, FeatureOperation,
    };

    Feature {
        id: FeatureId::mint("creo:model:feature#40").expect("identity grammar"),
        ordinal: 3,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        )),
        native_ref: None,
    }
}

#[test]
fn feature_output_refresh_refuses_before_update_rows() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(feature_for_output_refresh());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = refresh_feature_outputs(&ctx, &scan, &mut ir)
        .expect_err("one refresh needs one update row");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature output update rows"));
    assert_eq!(ir.model.features[0].ordinal, 3);
}

#[test]
fn feature_output_refresh_preserves_feature_order() {
    let scan = crate::container::scan_bytes_ok(Vec::new());
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(feature_for_output_refresh());
    crate::decode::with_test_decode_ctx(|ctx| refresh_feature_outputs(ctx, &scan, &mut ir))
        .expect("service profile admits the output update");
    assert_eq!(ir.model.features[0].ordinal, 3);
    assert!(ir.model.features[0].evaluation.outputs().is_empty());
}

fn property_key(value: &str) -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::text::NonBlankString::new(value).expect("fixture property key is nonblank")
}

#[test]
fn existing_feature_property_refuses_before_new_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut target = BTreeMap::new();
    let incoming = BTreeMap::from([(property_key("recipe"), "Extrude".to_string())]);
    let error = merge_feature_source_properties(&ctx, &mut target, incoming)
        .expect_err("existing Feature needs one destination node");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo IR Feature source property nodes"));
    assert!(target.is_empty());
}

#[test]
fn existing_feature_property_merge_keeps_order_and_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut target = BTreeMap::from([(property_key("recipe"), "Native".to_string())]);
    let incoming = BTreeMap::from([
        (property_key("featdefs_schema_state"), "absent".to_string()),
        (property_key("recipe"), "Extrude".to_string()),
    ]);
    merge_feature_source_properties(&ctx, &mut target, incoming)
        .expect("one new property and one replacement fit");
    assert_eq!(target["recipe"], "Extrude");
    assert_eq!(target["featdefs_schema_state"], "absent");
    assert_eq!(target.keys().next().map(cadmpeg_core::text::NonBlankString::as_str), Some("featdefs_schema_state"));
}

#[test]
fn existing_feature_dependency_refuses_before_member_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut target = cadmpeg_ir::features::DistinctMembers::default();
    let dependency = cadmpeg_ir::features::FeatureId::mint("creo:model:feature#12")
        .expect("dependency identity");
    let error = merge_feature_dependencies(&ctx, &mut target, vec![dependency])
        .expect_err("one new dependency needs one member slot");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo IR Feature dependency members"));
    assert!(target.is_empty());
}

#[test]
fn existing_feature_dependency_merge_preserves_first_order_and_uniqueness() {
    let first = cadmpeg_ir::features::FeatureId::mint("creo:model:feature#12")
        .expect("first dependency identity");
    let second = cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
        .expect("second dependency identity");
    let mut target = cadmpeg_ir::features::DistinctMembers::try_from_reserved_vec(vec![
        first.clone(),
    ])
    .expect("one member is distinct");
    crate::decode::with_test_decode_ctx(|ctx| {
        merge_feature_dependencies(ctx, &mut target, vec![first.clone(), second.clone()])
    })
    .expect("service profile admits one new member");
    assert_eq!(target.as_slice(), &[first, second]);
}

#[test]
fn native_operation_feature_refuses_kind_retained_limit() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.operations.push(crate::feature::operations::FeatureOperation {
        feature_id: 40,
        kind: crate::feature::operations::OperationKind::Native,
        name: crate::feature::operations::OperationName::Derived,
        recipe: crate::feature::operations::RecipeResolution::None,
        display_state_conflict: false,
        depdb: None,
        offset: 0,
        state_offset: 0,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let error = emit_model_features(
        &ctx,
        &scan,
        &mut ir,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("native kind needs retained text");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo native Feature kind"), "{error:?}");

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        emit_model_features(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("service-profile native Feature");
    let [feature] = ir.model.features.as_slice() else {
        panic!("one native Feature expected");
    };
    assert!(matches!(feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { kind, parameters }
        ) if kind.as_str() == "Native Feature" && parameters.is_empty()));
}

#[test]
fn native_row_feature_refuses_kind_retained_limit() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.rows.push(one_feature_row());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let error = emit_model_features(
        &ctx,
        &scan,
        &mut ir,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("row-native kind needs retained text");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo native row Feature kind"), "{error:?}");

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::decode::with_test_decode_ctx(|ctx| {
        emit_model_features(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("service-profile row-native Feature");
    let [feature] = ir.model.features.as_slice() else {
        panic!("one row-native Feature expected");
    };
    assert!(matches!(feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { kind, parameters }
        ) if kind.as_str() == "Native Feature" && parameters.is_empty()));
}

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
