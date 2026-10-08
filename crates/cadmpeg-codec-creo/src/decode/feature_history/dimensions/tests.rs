// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition as IrFeatureDefinition, FeatureOperation as IrFeatureOperation,
    ParameterId,
};
use cadmpeg_ir::AnnotationBuilder;

use super::super::dimensions::{
    dimension_expression, feature_dimension_parameter_layout,
    feature_dimension_parameter_row_id_admitted, insert_dimension_property,
    planned_feature_dimension_parameter_ids, push_feature_source_parameter,
    transfer_feature_dimensions, HexToken,
};

fn layout_key() -> cadmpeg_ir::sketches::SketchId {
    cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917".to_string())
        .expect("valid test identity")
}

#[test]
fn dimension_row_identity_refuses_before_formatting() {
    let sketch = layout_key();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo dimension parameter identity"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    feature_dimension_parameter_row_id_admitted(&ctx, &sketch, 3, Some(1)).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_dimension_parameter_row_id_admitted(&ctx, &sketch, 3, Some(1))
        .expect_err("dimension row ID exceeds retained cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension parameter identity")
    );
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
    assert_eq!(
        feature_dimension_parameter_row_id_admitted(&ctx, &sketch, 3, Some(1))
            .expect("service ID")
            .expect("valid ID")
            .as_str(),
        "creo:featdefs:parameter#917:3:2"
    );
}

fn one_dimension_transfer() -> (crate::container::ContainerScan<'static>, CadIr) {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(crate::feature::definitions::FeatureDimensionTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::definitions::FeatureDimension {
                    dimension_type: 2,
                    value: crate::feature::definitions::DimensionValue::Resolved(5.0),
                    value_body: Vec::new(),
                    direction_byte: 0,
                    auxiliary_value: None,
                    auxiliary_body: Vec::new(),
                    external_id: 3,
                    references: None,
                    offset: 10,
                }],
                offset: 9,
            }),
            relations: None,
            saved_section: None,
            offset: 8,
        });
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:sketch_feature#917")
            .expect("feature ID"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            IrFeatureDefinition::Operation(IrFeatureOperation::Native {
                kind: "test".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: None,
    });
    (scan, ir)
}

#[test]
fn dimension_transfer_refuses_staging_and_tree_nodes() {
    let (scan, ir) = one_dimension_transfer();
    let arena = DecodeArena::new();
    let operations = [
        "creo dimension owner feature ID nodes",
        "creo dimension candidates",
        "creo dimension layout keys",
        "creo dimension layout count nodes",
        "creo dimension parameter layout",
        "creo dimension layout ordinal nodes",
        "creo unique dimension external ID nodes",
        "creo relation parameter nodes",
    ];
    crate::test_support::assert_refusal_order(ResourceDimension::CollectionItems, &operations, |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        transfer_feature_dimensions(
            &ctx, &scan, &mut ir.clone(), &mut AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    });
    let mut ir = ir;
    let (transferred, parameters) = crate::decode::with_test_decode_ctx(|ctx| {
        transfer_feature_dimensions(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("service dimension transfer");
    assert_eq!(transferred, 1);
    assert_eq!(
        parameters.get("d3").map(ParameterId::as_str),
        Some("creo:featdefs:parameter#917:3")
    );
    assert_eq!(ir.model.parameters.len(), 1);
}

#[test]
fn feature_source_parameter_refuses_before_content_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo feature source content"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let id = ParameterId::mint("creo:featdefs:parameter#917:3".to_string())
        .expect("valid test identity");
    let mut content = cadmpeg_ir::features::FeatureContent::default();
    push_feature_source_parameter(&ctx, &mut content, id).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let id = ParameterId::mint("creo:featdefs:parameter#917:3".to_string())
        .expect("valid test identity");
    let mut content = cadmpeg_ir::features::FeatureContent::default();
    let error = push_feature_source_parameter(&ctx, &mut content, id)
        .expect_err("one reference needs one content row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature source content")
    );
    assert!(content.is_empty());
}

#[test]
fn feature_source_parameter_keeps_order_and_duplicate_refusal() {
    let id = ParameterId::mint("creo:featdefs:parameter#917:3".to_string())
        .expect("valid test identity");
    let mut content = cadmpeg_ir::features::FeatureContent::default();
    crate::decode::with_test_decode_ctx(|ctx| {
        push_feature_source_parameter(ctx, &mut content, id.clone())
    })
    .expect("one reference fits service limits");
    assert_eq!(content.len(), 1);
    assert!(
        matches!(&content[0], cadmpeg_ir::features::FeatureSourceContent::Parameter(value)
        if value == &id)
    );
    let error = crate::decode::with_test_decode_ctx(|ctx| {
        push_feature_source_parameter(ctx, &mut content, id)
    })
    .expect_err("duplicate parameter remains invalid");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn dimension_layout_refuses_before_count_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo dimension layout count nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    feature_dimension_parameter_layout(&ctx, &[(layout_key(), 3)]).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_dimension_parameter_layout(&ctx, &[(layout_key(), 3)])
        .expect_err("one count needs one BTreeMap node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo dimension layout count nodes")
    );
}

#[test]
fn dimension_layout_refuses_before_output_vector() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo dimension parameter layout"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    feature_dimension_parameter_layout(&ctx, &[(layout_key(), 3)]).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_dimension_parameter_layout(&ctx, &[(layout_key(), 3)])
        .expect_err("one layout row needs one vector slot");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo dimension parameter layout")
    );
}

#[test]
fn dimension_layout_refuses_before_ordinal_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo dimension layout ordinal nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    feature_dimension_parameter_layout(&ctx, &[(layout_key(), 3)]).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_dimension_parameter_layout(&ctx, &[(layout_key(), 3)])
        .expect_err("one sketch needs one ordinal BTreeMap node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo dimension layout ordinal nodes")
    );
}

#[test]
fn dimension_layout_refuses_before_occurrence_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo dimension layout occurrence nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let key = layout_key();
    feature_dimension_parameter_layout(&ctx, &[(key.clone(), 3), (key, 3)]).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let key = layout_key();
    let error = feature_dimension_parameter_layout(&ctx, &[(key.clone(), 3), (key, 3)])
        .expect_err("duplicate key needs an occurrence BTreeMap node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo dimension layout occurrence nodes")
    );
}

#[test]
fn dimension_layout_refuses_before_retained_name() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo dimension parameter name",
        |ctx| feature_dimension_parameter_layout(ctx, &[(layout_key(), 3)]).map(|layout| layout.map(|layout| layout.rows)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension parameter name")
    );
}

#[test]
fn dimension_property_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo dimension property nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    let error =
        insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7)
            .expect_err("one property needs one BTreeMap node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo dimension property nodes")
    );
}

#[test]
fn dimension_property_refuses_before_key_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo dimension property key"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    let error =
        insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7)
            .expect_err("property key exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension property key")
    );
}

#[test]
fn dimension_property_refuses_before_value_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo dimension property value"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    let error =
        insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7)
            .expect_err("property value exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension property value")
    );
}

#[test]
fn dimension_property_staging_node_refuses_materialized_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo dimension property nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    let error =
        insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7)
            .expect_err("one staging node exceeds materialized storage");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo dimension property nodes")
    );
}

#[test]
fn dimension_property_named_output_node_remains_retained() {
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
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7)
        ?;
    cadmpeg_core::text::named_entries_for_decode(&ctx, "dimension", properties).map(|_| ()).map_err(|error| match error {
        cadmpeg_core::text::NamedEntryError::ResourceRefusal(resource) => cadmpeg_core::CodecError::ResourceLimit(resource),
        error => panic!("unexpected named entry refusal: {error:?}"),
    })
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    insert_dimension_property(&ctx, &mut node_storage, &mut properties, "external_id", 7)
        .expect("staging node uses scoped storage");
    let error = cadmpeg_core::text::named_entries_for_decode(&ctx, "dimension", properties)
        .expect_err("the decoded output node needs retained storage");
    assert!(
        matches!(error, cadmpeg_core::text::NamedEntryError::ResourceRefusal(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "named entry map nodes")
    );
}

#[test]
fn dimension_property_named_entry_service_preserves_order() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    insert_dimension_property(&ctx, &mut node_storage, &mut properties, "z", 1)
        .expect("first property fits");
    insert_dimension_property(&ctx, &mut node_storage, &mut properties, "a", 2)
        .expect("second property fits");
    let entries = cadmpeg_core::text::named_entries_for_decode(&ctx, "dimension", properties)
        .expect("valid property keys are admitted");
    drop(node_storage);
    assert_eq!(
        entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        vec![("a", "2"), ("z", "1")]
    );
}

#[test]
fn dimension_expression_refuses_before_retained_text() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo dimension expression"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    dimension_expression(&ctx, Some(5.0)).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = dimension_expression(&ctx, Some(5.0))
        .expect_err("nonempty expression exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension expression")
    );
}

#[test]
fn dimension_hex_token_keeps_lowercase_byte_order() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo dimension property nodes")
        .expect("dimension property lease");
    let mut properties = BTreeMap::new();
    insert_dimension_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "value_token",
        HexToken(&[0x00, 0xf1, 0x7f]),
    )
    .expect("three token bytes fit service limits");
    assert_eq!(properties["value_token"], "00f17f");
    assert_eq!(
        dimension_expression(&ctx, Some(5.0)).expect("expression fits"),
        "5"
    );
}

#[test]
fn planned_dimension_ids_refuse_before_tree_node_and_identity_copy() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: Some(40),
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(crate::feature::definitions::FeatureDimensionTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::definitions::FeatureDimension {
                    dimension_type: 2,
                    value: crate::feature::definitions::DimensionValue::Resolved(5.0),
                    value_body: Vec::new(),
                    direction_byte: 0,
                    auxiliary_value: None,
                    auxiliary_body: Vec::new(),
                    external_id: 3,
                    references: None,
                    offset: 10,
                }],
                offset: 9,
            }),
            relations: None,
            saved_section: None,
            offset: 8,
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some("creo planned dimension parameter identity"), |cap| {
        let arena = DecodeArena::new();
        let mut policy = policy;
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        planned_feature_dimension_parameter_ids(&ctx, &scan)
    });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = planned_feature_dimension_parameter_ids(&ctx, &scan)
        .expect_err("parameter identity exceeds remaining retained cap");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo planned dimension parameter identity")
    );
    policy.limits.max_retained_bytes = DecodePolicy::service().limits.max_retained_bytes;
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo planned dimension parameter ID nodes"), |cap| {
        let arena = DecodeArena::new();
        let mut policy = policy;
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        planned_feature_dimension_parameter_ids(&ctx, &scan)
    });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = planned_feature_dimension_parameter_ids(&ctx, &scan)
        .expect_err("one parameter needs one tree node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo planned dimension parameter ID nodes")
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            planned_feature_dimension_parameter_ids(ctx, &scan)
        })
        .expect("service IDs admitted")
        .len(),
        1
    );
}

#[test]
fn dimension_transfer_rejects_duplicate_owner_feature_ids() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Section),
        stream_offset: 0,
        body: vec![0; 20].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(917),
                owner_feature_id: Some(40),
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(crate::feature::definitions::FeatureDimensionTable {
                declared_count: 1,
                entity_ref: None,
                rows: vec![crate::feature::definitions::FeatureDimension {
                    dimension_type: 2,
                    value: crate::feature::definitions::DimensionValue::Resolved(5.0),
                    value_body: Vec::new(),
                    direction_byte: 0,
                    auxiliary_value: None,
                    auxiliary_body: Vec::new(),
                    external_id: 3,
                    references: None,
                    offset: 10,
                }],
                offset: 9,
            }),
            relations: None,
            saved_section: None,
            offset: 8,
        });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| planned_feature_dimension_parameter_ids(
            ctx, &scan
        ))
        .expect("planned IDs admitted"),
        BTreeSet::from([
            ParameterId::mint("creo:featdefs:parameter#917:3".to_string())
                .expect("identity grammar")
        ])
    );

    let mut ir = CadIr::empty();
    for ordinal in 0..2 {
        ir.model.features.push(Feature {
            id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
                .expect("identity grammar"),
            ordinal,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                IrFeatureDefinition::Operation(IrFeatureOperation::Native {
                    kind: "test".into(),
                    parameters: BTreeMap::new(),
                }),
            ),
            native_ref: None,
        });
    }

    let (transferred, ir) = crate::test_support::assert_work_boundaries(
        &[
            "creo dimension owner feature ID lookup",
            "creo dimension owner lookup",
        ],
        |ctx| {
            let mut ir = ir.clone();
            let (transferred, _) = transfer_feature_dimensions(
                ctx,
                &scan,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
            )?;
            Ok::<_, cadmpeg_core::CodecError>((transferred, ir))
        },
    );

    assert_eq!(transferred, 1);
    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| feature.source_content.is_empty()));
}

#[test]
fn relation_coverage_refuses_rows_above_declared_empty_table() {
    let table = crate::feature::definitions::FeatureRelationTable {
        declared_count: 1,
        entity_ref: None,
        rows: vec![crate::feature::definitions::FeatureRelation {
            relation_id: 1,
            used: 1,
            operands: Vec::new(),
            operand_vectors: None,
            sign: 0,
            dimension_id: 0,
            relation_type: 0,
            body: Vec::new(),
            offset: 0,
        }],
        skamps: None,
        triples: None,
        offset: 0,
    };
    assert!(matches!(
        super::feature_relation_table_missing_rows(&table),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}
