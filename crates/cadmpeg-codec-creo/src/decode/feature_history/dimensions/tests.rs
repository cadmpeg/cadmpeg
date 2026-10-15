// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition as IrFeatureDefinition, FeatureOperation as IrFeatureOperation,
    ParameterId,
};
use cadmpeg_ir::AnnotationBuilder;

use super::super::dimensions::{
    dimension_expression, feature_dimension_parameter_row_id_admitted, insert_dimension_property,
    planned_feature_dimension_parameter_ids, push_feature_source_parameter,
    transfer_feature_dimensions, HexToken,
};

fn layout_key() -> cadmpeg_ir::sketches::SketchId {
    cadmpeg_ir::sketches::SketchId::mint("creo:model:sketch#917".to_string())
        .expect("valid test identity")
}

#[test]
fn dimension_row_identity_appends_occurrence_suffix() {
    let sketch = layout_key();
    assert_eq!(
        feature_dimension_parameter_row_id_admitted(&sketch, 3, Some(1))
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
fn dimension_transfer_records_parameter_for_external_id() {
    let (scan, mut ir) = one_dimension_transfer();
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
fn dimension_hex_token_keeps_lowercase_byte_order() {
    let mut properties = BTreeMap::new();
    insert_dimension_property(
        &mut properties,
        "value_token",
        HexToken(&[0x00, 0xf1, 0x7f]),
    );
    assert_eq!(properties["value_token"], "00f17f");
    assert_eq!(
        dimension_expression(Some(5.0)).expect("expression fits"),
        "5"
    );
}

#[test]
fn planned_dimension_ids_cover_owned_dimension_row() {
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
    assert_eq!(planned_feature_dimension_parameter_ids(&scan).len(), 1);
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
