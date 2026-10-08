// SPDX-License-Identifier: Apache-2.0

use super::{insert_feature_parameter, insert_feature_source_property, replace_feature_parameter, BTreeMap, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn feature_parameter_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo feature parameter nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    let error = insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect_err("one parameter needs one BTreeMap node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature parameter nodes")
    );
}

#[test]
fn feature_parameter_staging_node_refuses_materialized_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo feature parameter nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    let error = insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect_err("one source node exceeds materialized storage");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo feature parameter nodes"
            && resource.additional > 0)
    );
}

#[test]
fn feature_parameter_refuses_before_staging_value() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo feature parameter value"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    let error = insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect_err("staging value exceeds materialized allowance");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo feature parameter value")
    );
}

#[test]
fn feature_parameter_refuses_before_scoped_key_candidate() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo feature parameter key candidate"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    let error = insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect_err("candidate exceeds materialized allowance");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo feature parameter key candidate")
    );
}

#[test]
fn feature_parameter_native_text_transfer_refuses_retained_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo feature parameter text"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect("staging text fits materialized storage");
    text_storage
        .commit().map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect("staging text fits materialized storage");
    let error = text_storage
        .commit()
        .expect_err("Native parameter text exceeds retained allowance");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature parameter text")
    );
}

#[test]
fn feature_parameter_keeps_duplicate_suffix_and_direct_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "a",
    )
    .expect("first fits");
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "b",
    )
    .expect("second fits");
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "c",
    )
    .expect("third fits");
    replace_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "z",
    )
    .expect("replacement fits");
    assert_eq!(
        parameters.into_iter().collect::<Vec<_>>(),
        vec![
            ("choice.value".into(), "z".into()),
            ("choice.value#2".into(), "b".into()),
            ("choice.value#3".into(), "c".into()),
        ]
    );
}

#[test]
fn feature_parameter_named_entry_service_preserves_suffix_order() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    for value in ["a", "b", "c"] {
        insert_feature_parameter(
            &ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "choice.value",
            value,
        )
        .expect("parameter collision suffix is admitted");
    }
    text_storage
        .commit()
        .expect("Native output retains staged parameter text");
    let entries = cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", parameters)
        .expect("parameter names are nonblank and unique");
    drop(node_storage);
    assert_eq!(
        entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("choice.value", "a"),
            ("choice.value#2", "b"),
            ("choice.value#3", "c"),
        ]
    );
}

#[test]
fn feature_source_property_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo feature source property nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    ).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    let error = insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect_err("one property needs one map node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature source property nodes")
    );
    assert!(properties.is_empty());
}

#[test]
fn feature_source_property_staging_node_refuses_materialized_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo feature source property nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    ).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    let error = insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect_err("one staging node exceeds materialized storage");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo feature source property nodes")
    );
}

#[test]
fn feature_source_property_refuses_before_key_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo feature source property key"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    ).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    let error = insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect_err("key bytes exceed the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature source property key")
    );
}

#[test]
fn feature_source_property_refuses_before_value_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo feature source property value"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    ).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    let error = insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect_err("value bytes exceed the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature source property value")
    );
}

#[test]
fn feature_source_property_keeps_key_order_and_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "z", 1)
        .expect("first property fits");
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "a", 2)
        .expect("second property fits");
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "z", 3)
        .expect("replacement fits");
    assert_eq!(
        properties.into_iter().collect::<Vec<_>>(),
        vec![("a".into(), "2".into()), ("z".into(), "3".into())]
    );
}

#[test]
fn feature_source_property_named_output_node_remains_retained() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("named entry map nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    ?;
    cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", properties).map(|_| ()).map_err(|error| match error {
        cadmpeg_core::text::NamedEntryError::ResourceRefusal(resource) => cadmpeg_core::CodecError::ResourceLimit(resource),
        error => panic!("unexpected named entry refusal: {error:?}"),
    })
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect("staging node uses scoped storage");
    let error = cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", properties)
        .expect_err("the decoded output node needs retained storage");
    assert!(
        matches!(error, cadmpeg_core::text::NamedEntryError::ResourceRefusal(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "named entry map nodes")
    );
}

#[test]
fn feature_source_property_named_entry_service_preserves_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "z", 1)
        .expect("first property fits");
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "a", 2)
        .expect("second property fits");
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "z", 3)
        .expect("replacement fits");
    let entries = cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", properties)
        .expect("property names are nonblank and unique");
    drop(node_storage);
    assert_eq!(
        entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        vec![("a", "2"), ("z", "3")]
    );
}

#[test]
fn feature_parameter_collision_boundary_keeps_first_unused_suffix() {
    let parameters = crate::test_support::assert_work_boundaries(
        &["creo feature parameter key lookup"],
        |ctx| {
            let mut text_storage = ctx.reserve_scoped(0, "creo feature parameter text")?;
            let mut node_storage = ctx.reserve_scoped(0, "creo feature parameter nodes")?;
            let mut parameters = BTreeMap::from([
                ("choice.value".to_owned(), "a".to_owned()),
                ("choice.value#2".to_owned(), "b".to_owned()),
            ]);
            insert_feature_parameter(
                ctx,
                &mut text_storage,
                &mut node_storage,
                &mut parameters,
                "choice.value",
                "c",
            )?;
            Ok::<_, cadmpeg_core::CodecError>(parameters)
        },
    );
    assert_eq!(
        parameters.into_iter().collect::<Vec<_>>(),
        vec![
            ("choice.value".to_owned(), "a".to_owned()),
            ("choice.value#2".to_owned(), "b".to_owned()),
            ("choice.value#3".to_owned(), "c".to_owned()),
        ],
    );
}
