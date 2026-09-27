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
    dimension_expression, insert_dimension_property, planned_feature_dimension_parameter_ids,
    transfer_feature_dimensions, HexToken,
};

#[test]
fn dimension_property_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut properties = BTreeMap::new();
    let error = insert_dimension_property(&ctx, &mut properties, "external_id", 7)
        .expect_err("one property needs one BTreeMap node");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo dimension property nodes"));
}

#[test]
fn dimension_property_refuses_before_key_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut properties = BTreeMap::new();
    let error = insert_dimension_property(&ctx, &mut properties, "external_id", 7)
        .expect_err("property key exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension property key"));
}

#[test]
fn dimension_property_refuses_before_value_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = "external_id".len() as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut properties = BTreeMap::new();
    let error = insert_dimension_property(&ctx, &mut properties, "external_id", 7)
        .expect_err("property value exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension property value"));
}

#[test]
fn dimension_expression_refuses_before_retained_text() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = dimension_expression(&ctx, Some(5.0))
        .expect_err("nonempty expression exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo dimension expression"));
}

#[test]
fn dimension_hex_token_keeps_lowercase_byte_order() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let mut properties = BTreeMap::new();
    insert_dimension_property(&ctx, &mut properties, "value_token", HexToken(&[0x00, 0xf1, 0x7f]))
        .expect("three token bytes fit service limits");
    assert_eq!(properties["value_token"], "00f17f");
    assert_eq!(dimension_expression(&ctx, Some(5.0)).expect("expression fits"), "5");
}

#[test]
fn dimension_transfer_rejects_duplicate_owner_feature_ids() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
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
        planned_feature_dimension_parameter_ids(&scan),
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

    let (transferred, _) = crate::decode::with_test_decode_ctx(|ctx| {
        transfer_feature_dimensions(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("valid test fixture");

    assert_eq!(transferred, 1);
    assert!(ir
        .model
        .features
        .iter()
        .all(|feature| feature.source_content.is_empty()));
}
