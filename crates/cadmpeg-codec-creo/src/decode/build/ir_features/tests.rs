// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{
    append_regeneration_edge, commit_regeneration_edges, compose_feature_id, emit_model_features,
    merge_feature_dependencies, merge_feature_source_properties, ordered_row_feature_ids,
    refresh_feature_outputs,
};

#[test]
fn model_feature_identity_refuses_before_materialization_and_retention() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = compose_feature_id(&ctx, 40).expect_err("temporary identity exceeds cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo model feature identity")
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (_, bytes) = compose_feature_id(&ctx, 40).expect("temporary identity admitted");
    let error = bytes.commit().expect_err("retained identity exceeds cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo model feature identity")
    );

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (id, bytes) = compose_feature_id(&ctx, 40).expect("service identity");
    bytes.commit().expect("service retention");
    assert_eq!(id.as_str(), "creo:model:feature#40");
}

#[test]
fn regeneration_edge_refuses_each_storage_boundary() {
    use cadmpeg_ir::features::FeatureId;
    let child = FeatureId::mint("creo:model:feature#41").expect("identity grammar");
    let parent = FeatureId::mint("creo:model:feature#40").expect("identity grammar");
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "creo regeneration edges",
        ),
        (
            ResourceDimension::RetainedBytes,
            "creo regeneration child identity",
        ),
        (
            ResourceDimension::RetainedBytes,
            "creo regeneration parent identity",
        ),
    ] {
        let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
            append_regeneration_edge(ctx, &mut Vec::new(), &child, &parent)
        });
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == dimension && resource.operation == operation)
        );
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut edges = Vec::new();
    append_regeneration_edge(&ctx, &mut edges, &child, &parent).expect("service edge");
    assert_eq!(edges, vec![(child, parent)]);
}

#[test]
fn regeneration_parent_node_refuses_before_tree_insertion() {
    use cadmpeg_ir::features::FeatureId;
    let parent_id = FeatureId::mint("creo:model:feature#40").expect("identity grammar");
    let child_id = FeatureId::mint("creo:model:feature#41").expect("identity grammar");
    let make_ir = || {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let mut parent = feature_for_output_refresh();
        parent.ordinal = 0;
        ir.model.features.push(parent);
        let mut child = feature_for_output_refresh();
        child.id = child_id.clone();
        child.ordinal = 1;
        ir.model.features.push(child);
        ir
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = commit_regeneration_edges(
        &ctx,
        &mut make_ir(),
        &[(child_id.clone(), parent_id.clone())],
    )
    .expect_err("tree node exceeds cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "install decoded feature regeneration parent")
    );

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut ir = make_ir();
    commit_regeneration_edges(&ctx, &mut ir, &[(child_id.clone(), parent_id.clone())])
        .expect("service node");
    assert_eq!(ir.model.feature_parent(&child_id), Some(&parent_id));
}

fn feature_for_output_refresh() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
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
    let scan = crate::test_support::empty_container_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(feature_for_output_refresh());
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo feature output update rows",
        |ctx| {
            let mut trial = ir.clone();
            let result = refresh_feature_outputs(ctx, &scan, &mut trial);
            assert_eq!(trial.model.features[0].ordinal, 3);
            result
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature output update rows")
    );
    assert_eq!(ir.model.features[0].ordinal, 3);
}

#[test]
fn feature_output_refresh_preserves_feature_order() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(feature_for_output_refresh());
    crate::decode::with_test_decode_ctx(|ctx| refresh_feature_outputs(ctx, &scan, &mut ir))
        .expect("service profile admits the output update");
    assert_eq!(ir.model.features[0].ordinal, 3);
    assert!(ir.model.features[0].evaluation.outputs().is_empty());
}

fn property_key(value: &str) -> cadmpeg_core::text::NonBlankString {
    cadmpeg_core::text::NonBlankString::try_from(value).expect("fixture property key is nonblank")
}

#[test]
fn existing_feature_property_refuses_before_new_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut target = BTreeMap::new();
    let incoming = BTreeMap::from([(property_key("recipe"), "Extrude".to_string())]);
    let error = merge_feature_source_properties(&ctx, &mut target, incoming)
        .expect_err("existing Feature needs one destination node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo IR Feature source property nodes")
    );
    assert!(target.is_empty());
}

#[test]
fn existing_feature_property_merge_keeps_order_and_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut target = BTreeMap::from([(property_key("recipe"), "Native".to_string())]);
    let incoming = BTreeMap::from([
        (property_key("featdefs_schema_state"), "absent".to_string()),
        (property_key("recipe"), "Extrude".to_string()),
    ]);
    merge_feature_source_properties(&ctx, &mut target, incoming)
        .expect("one new property and one replacement fit");
    assert_eq!(target["recipe"], "Extrude");
    assert_eq!(target["featdefs_schema_state"], "absent");
    assert_eq!(
        target
            .keys()
            .next()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("featdefs_schema_state")
    );
}

#[test]
fn existing_feature_property_staging_nodes_are_scoped_before_retained_destination() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo IR Feature source property nodes",
        |ctx| {
            let mut source_nodes = ctx.reserve_scoped(0, "creo feature source property nodes")?;
            let mut source = BTreeMap::new();
            crate::decode::feature_history::outputs::insert_feature_source_property(
                ctx,
                &mut source_nodes,
                &mut source,
                "recipe",
                "Extrude",
            )?;
            let mut incoming_nodes = ctx.reserve_scoped(0, "named entry map nodes")?;
            let incoming = incoming_nodes.with_storage(|| {
                cadmpeg_core::text::named_entries_for_decode(ctx, "feature", source)
                    .map_err(cadmpeg_core::CodecError::from)
            })?;
            drop(source_nodes);
            let mut target = BTreeMap::new();
            let result = merge_feature_source_properties(ctx, &mut target, incoming);
            if result.is_err() {
                assert!(target.is_empty());
            }
            drop(incoming_nodes);
            result
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes && resource.operation == "creo IR Feature source property nodes")
    );
}

#[test]
fn existing_feature_property_scoped_named_merge_keeps_order_and_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut source_nodes = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property lease");
    let mut source = BTreeMap::new();
    crate::decode::feature_history::outputs::insert_feature_source_property(
        &ctx,
        &mut source_nodes,
        &mut source,
        "featdefs_schema_state",
        "absent",
    )
    .expect("first source property fits");
    crate::decode::feature_history::outputs::insert_feature_source_property(
        &ctx,
        &mut source_nodes,
        &mut source,
        "recipe",
        "Extrude",
    )
    .expect("second source property fits");
    let mut incoming_nodes = ctx
        .reserve_scoped(0, "named entry map nodes")
        .expect("intermediate output lease");
    let incoming = incoming_nodes
        .with_storage(|| {
            cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", source)
                .map_err(cadmpeg_core::CodecError::from)
        })
        .expect("intermediate named map nodes are scoped");
    drop(source_nodes);
    let mut target = BTreeMap::from([(property_key("recipe"), "Native".to_string())]);
    merge_feature_source_properties(&ctx, &mut target, incoming)
        .expect("persistent destination inserts and replaces under service policy");
    drop(incoming_nodes);
    assert_eq!(target["recipe"], "Extrude");
    assert_eq!(target["featdefs_schema_state"], "absent");
    assert_eq!(
        target
            .keys()
            .next()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("featdefs_schema_state")
    );
}

#[test]
fn existing_feature_dependency_refuses_before_member_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut target = cadmpeg_ir::features::DistinctMembers::default();
    let dependency = cadmpeg_ir::features::FeatureId::mint("creo:model:feature#12")
        .expect("dependency identity");
    let error = merge_feature_dependencies(&ctx, &mut target, vec![dependency])
        .expect_err("one new dependency needs one member slot");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo IR Feature dependency members")
    );
    assert!(target.is_empty());
}

#[test]
fn existing_feature_dependency_merge_preserves_first_order_and_uniqueness() {
    let first = cadmpeg_ir::features::FeatureId::mint("creo:model:feature#12")
        .expect("first dependency identity");
    let second = cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
        .expect("second dependency identity");
    let mut target = crate::decode::with_test_decode_ctx(|ctx| {
        cadmpeg_ir::features::DistinctMembers::try_from(vec![first.clone()], ctx)
            .map_err(cadmpeg_core::CodecError::from)
    })
    .expect("one member is distinct");
    crate::decode::with_test_decode_ctx(|ctx| {
        merge_feature_dependencies(ctx, &mut target, vec![first.clone(), second.clone()])
    })
    .expect("service profile admits one new member");
    assert_eq!(target.as_slice(), &[first, second]);
}

#[test]
fn native_operation_feature_refuses_kind_retained_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
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
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo native Feature kind"),
        |cap| {
            let trial_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &trial_arena,
                &trial_policy,
            )
            .expect("root");
            let mut ir = cadmpeg_ir::document::CadIr::empty();
            emit_model_features(
                &trial_ctx,
                &scan,
                &mut ir,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let error = emit_model_features(
        &ctx,
        &scan,
        &mut ir,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("native kind needs retained text");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo native Feature kind"),
        "{error:?}"
    );

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
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(one_feature_row());
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo native row Feature kind",
        |ctx| {
            emit_model_features(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes && resource.operation == "creo native row Feature kind")
    );

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
fn native_row_feature_refuses_name_retained_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(one_feature_row());
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo row Feature name",
        |ctx| {
            emit_model_features(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes && resource.operation == "creo row Feature name")
    );

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
    .expect("service-profile row name");
    assert_eq!(
        ir.model.features[0].name.as_deref(),
        Some("Native Feature id 40")
    );
}

#[test]
fn stored_operation_feature_name_refuses_retained_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Native,
            name: crate::feature::operations::OperationName::Stored {
                bytes: b"~Native Feature id 40".to_vec(),
                keyword: crate::feature::operations::IdKeyword::Id,
                prefix: Some(b'~'),
            },
            recipe: crate::feature::operations::RecipeResolution::None,
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo stored Feature name"),
        |cap| {
            let trial_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &trial_arena,
                &trial_policy,
            )
            .expect("root");
            emit_model_features(
                &trial_ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = emit_model_features(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("the stripped stored name exceeds its retained boundary");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo stored Feature name"),
        "{error:?}"
    );

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
    .expect("service-profile stored name");
    assert_eq!(
        ir.model.features[0].name.as_deref(),
        Some("Native Feature id 40")
    );
}

#[test]
fn recipe_source_tag_refuses_retained_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Extrude,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::Resolved(
                crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
            ),
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo Feature source tag",
        |ctx| {
            emit_model_features(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes && resource.operation == "creo Feature source tag")
    );

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
    .expect("service-profile recipe tag");
    assert_eq!(
        ir.model.features[0].source_tag.as_deref(),
        Some("protextrude")
    );
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
            .expect("row IDs fit service limits")
            .order,
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
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = ordered_row_feature_ids(&ctx, &[one_feature_row()])
        .expect_err("one distinct row needs one identity node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature row identity nodes")
    );
}

#[test]
fn feature_row_id_refuses_before_vec_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo feature row IDs"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_collection_items = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            ordered_row_feature_ids(&trial_ctx, &[one_feature_row()])
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = ordered_row_feature_ids(&ctx, &[one_feature_row()])
        .expect_err("one distinct row also needs one output slot");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature row IDs")
    );
}

#[test]
fn operation_feature_identity_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut ids = BTreeSet::new();
    let error = ctx
        .insert_btree_set(&mut ids, 40, "creo operation feature identity nodes")
        .expect_err("one operation needs one identity node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo operation feature identity nodes")
    );
    assert!(ids.is_empty());
}

#[test]
fn model_feature_identity_preserves_formatted_namespace() {
    assert_eq!(
        crate::decode::with_test_decode_ctx(
            |ctx| compose_feature_id(ctx, 40).map(|(id, _reservation)| id)
        )
        .expect("service identity grammar")
        .as_str(),
        "creo:model:feature#40"
    );
}

#[test]
fn combined_feature_output_membership_refuses_work_and_preserves_service_outputs() {
    let output =
        cadmpeg_ir::ids::BodyId::mint("creo:feature:extrusion#40:body").expect("identity grammar");
    let mut initial = cadmpeg_ir::document::CadIr::empty();
    initial.model.bodies.push(cadmpeg_ir::topology::Body {
        id: output.clone(),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let mut existing = feature_for_output_refresh();
    existing.evaluation.set_outputs(
        crate::decode::with_test_decode_ctx(|ctx| {
            cadmpeg_ir::features::DistinctMembers::try_from(vec![output.clone()], ctx)
                .map_err(cadmpeg_core::CodecError::from)
        })
        .expect("one existing output body"),
    );
    initial.model.features.push(existing);

    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Native,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::None,
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });

    let (feature_count, outputs) = crate::test_support::assert_work_boundaries(
        &["creo combined feature output lookup"],
        |ctx| {
            let mut ir = initial.clone();
            let feature_count = emit_model_features(
                ctx,
                &scan,
                &mut ir,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )?;
            Ok((
                feature_count,
                ir.model.features[0].evaluation.outputs().clone(),
            ))
        },
    );
    assert_eq!(feature_count, 0);
    assert_eq!(outputs, vec![output]);
}

#[test]
fn feature_row_index_preserves_order_and_selects_the_lowest_source_offset() {
    let mut first = one_feature_row();
    first.offset = 60;
    let mut second = one_feature_row();
    second.feature_id = 12;
    let last = one_feature_row();
    let index = crate::decode::with_test_decode_ctx(|ctx| {
        ordered_row_feature_ids(ctx, &[first, second, last])
    })
    .expect("row index");
    assert_eq!(index.order, vec![40, 12]);
    assert_eq!(index.offsets[&40], 20);
}

#[test]
fn model_feature_index_accepts_only_the_composed_numeric_identity() {
    for (identity, number) in [
        ("creo:model:feature#40", Some(40)),
        ("creo:model:feature#0040", None),
        ("other:model:feature#40", None),
        ("creo:model:feature#4294967296", None),
    ] {
        assert_eq!(super::model_feature_number(identity), number);
    }
}

mod id_storage;
