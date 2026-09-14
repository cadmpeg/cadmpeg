// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::native::{
    CatiaDefinitionChainValue, CatiaDefinitionValue, CatiaEntityEvaluation,
    CatiaEntityEvaluationEncoding, CatiaEntitySchemaValue, CatiaEntitySuffixPayload,
    CatiaEntitySuffixSchemaValue,
};
use cadmpeg_ir::features::FeatureOperation;

#[test]
fn transfers_exact_definition_values_as_typed_feature_properties() {
    let mut operation = native_operation_object(
        "synthetic:test:object#operation-object",
        None,
        1,
        "operation-record",
        "Prism_ThickThin1",
        "operation-entry",
    );
    operation
        .definition_values
        .push("definition-entity".to_string());
    operation.fields.push("definition-record".to_string());
    let mut definition_entity = entity_record("definition-entity", "definition-record", 20, 1);
    definition_entity.value_production = Some(
        crate::native::entity_record::CatiaEntityValueProduction::DefinitionValue(
            CatiaDefinitionValue {
                definition: CatiaEntitySchemaValue {
                    offset: 4,
                    ordinal: 2,
                    entry: "definition-entry".to_string(),
                    value: "Mirror".to_string(),
                },
                payload: CatiaEntitySuffixPayload::Evaluation {
                    opcode_offset: 8,
                    evaluation: CatiaEntityEvaluation::Scalar {
                        bits: 12.5_f64.to_bits(),
                    },
                    encoding: CatiaEntityEvaluationEncoding::Direct,
                },
                schema_selection: None,
            },
        ),
    );
    let native = CatiaNative {
        design_objects: vec![operation],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![
                object_record(
                    "operation-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_ThickThin1"),
                    Some("operation-entry"),
                ),
                object_record(
                    "definition-record",
                    Some("synthetic:test:object#operation-object"),
                    Some(1),
                    Some(1),
                    None,
                    None,
                ),
            ],
        }],
        entity_records: vec![definition_entity],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();

    let transfer = transfer_design_features(
        &mut ir,
        &native,
        &crate::decode::ModelingGraphScope::Unscoped,
    )
    .unwrap();

    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Extrude
        })
    ));
    assert_eq!(
        &ir.model.features[0].source_properties,
        &BTreeMap::from([
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_definition_entry"),
                "definition-entry".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_definition_offset"),
                "4".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_definition_ordinal"),
                "2".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_definition_value"),
                "Mirror".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_entity"),
                "definition-entity".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_payload_encoding"),
                "direct".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_payload_evaluation"),
                "scalar".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_payload_evaluation_bits"),
                format!("{:016x}", 12.5_f64.to_bits()),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_payload_kind"),
                "evaluation".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_value_0_payload_opcode_offset"),
                "8".to_string(),
            ),
        ])
    );
    assert_eq!(transfer.native_operation_definition_value_count, 1);
    assert_eq!(
        transfer.native_operation_definition_value_records,
        HashSet::from(["definition-record".to_string()])
    );
    assert_eq!(
        transfer.consumed_records(),
        HashSet::from([
            "operation-record".to_string(),
            "definition-record".to_string()
        ])
    );
}

#[test]
fn transfers_exact_definition_chains_as_typed_feature_properties() {
    let mut operation = native_operation_object(
        "synthetic:test:object#operation-object",
        None,
        1,
        "operation-record",
        "Prism_ThickThin1",
        "operation-entry",
    );
    operation
        .definition_chain_values
        .push("definition-chain-entity".to_string());
    operation.fields.push("definition-chain-record".to_string());
    let mut chain_entity =
        entity_record("definition-chain-entity", "definition-chain-record", 20, 1);
    chain_entity.value_production = Some(
        crate::native::entity_record::CatiaEntityValueProduction::DefinitionChainValue(
            CatiaDefinitionChainValue {
                selector: CatiaEntitySchemaValue {
                    offset: 4,
                    ordinal: 2,
                    entry: "selector-entry".to_string(),
                    value: "Length".to_string(),
                },
                role: CatiaEntitySchemaValue {
                    offset: 8,
                    ordinal: 3,
                    entry: "role-entry".to_string(),
                    value: "UnsupportedRole".to_string(),
                },
                value: CatiaEntitySuffixSchemaValue::Evaluation {
                    opcode_offset: 12,
                    evaluation: CatiaEntityEvaluation::Scalar {
                        bits: 12.5_f64.to_bits(),
                    },
                },
            },
        ),
    );
    let native = CatiaNative {
        design_objects: vec![operation],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![
                object_record(
                    "operation-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_ThickThin1"),
                    Some("operation-entry"),
                ),
                object_record(
                    "definition-chain-record",
                    Some("synthetic:test:object#operation-object"),
                    Some(1),
                    Some(1),
                    None,
                    None,
                ),
            ],
        }],
        entity_records: vec![chain_entity],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();

    let transfer = transfer_design_features(
        &mut ir,
        &native,
        &crate::decode::ModelingGraphScope::Unscoped,
    )
    .unwrap();

    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Extrude
        })
    ));
    assert_eq!(
        &ir.model.features[0].source_properties,
        &BTreeMap::from([
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_entity"),
                "definition-chain-entity".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_role_entry"),
                "role-entry".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_role_offset"),
                "8".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_role_ordinal"),
                "3".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_role_value"),
                "UnsupportedRole".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_selector_entry"),
                "selector-entry".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_selector_offset"),
                "4".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_selector_ordinal"),
                "2".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_selector_value"),
                "Length".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_value_evaluation"),
                "scalar".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!(
                    "catia_definition_chain_value_0_value_evaluation_bits"
                ),
                format!("{:016x}", 12.5_f64.to_bits()),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_value_kind"),
                "evaluation".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!(
                    "catia_definition_chain_value_0_value_opcode_offset"
                ),
                "12".to_string(),
            ),
        ])
    );
    assert_eq!(transfer.native_operation_definition_chain_value_count, 1);
    assert_eq!(
        transfer.native_operation_definition_chain_value_records,
        HashSet::from(["definition-chain-record".to_string()])
    );
    assert_eq!(
        transfer.consumed_records(),
        HashSet::from([
            "operation-record".to_string(),
            "definition-chain-record".to_string()
        ])
    );
}

#[test]
fn transfers_definition_chains_from_exact_operation_owner_descendants() {
    let mut operation = native_operation_object(
        "synthetic:test:object#operation-object",
        None,
        1,
        "operation-record",
        "Prism_ThickThin1",
        "operation-entry",
    );
    operation.first_field_byte_offset = 10;
    let mut descendant = design_object(
        "synthetic:test:object#descendant-object",
        Some("synthetic:test:object#operation-object"),
    );
    descendant.first_field_byte_offset = 20;
    descendant
        .definition_chain_values
        .push("descendant-chain-entity".to_string());
    let mut descendant_entity =
        entity_record("descendant-chain-entity", "descendant-record", 30, 2);
    descendant_entity.value_production = Some(
        crate::native::entity_record::CatiaEntityValueProduction::DefinitionChainValue(
            CatiaDefinitionChainValue {
                selector: CatiaEntitySchemaValue {
                    offset: 2,
                    ordinal: 4,
                    entry: "selector-entry".to_string(),
                    value: "Length".to_string(),
                },
                role: CatiaEntitySchemaValue {
                    offset: 7,
                    ordinal: 5,
                    entry: "role-entry".to_string(),
                    value: "UnsupportedRole".to_string(),
                },
                value: CatiaEntitySuffixSchemaValue::Atom { value: 3 },
            },
        ),
    );
    let native = CatiaNative {
        design_objects: vec![operation, descendant],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![object_record(
                "operation-record",
                None,
                Some(1),
                None,
                Some("Prism_ThickThin1"),
                Some("operation-entry"),
            )],
        }],
        entity_records: vec![descendant_entity],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty();

    let transfer = transfer_design_features(
        &mut ir,
        &native,
        &crate::decode::ModelingGraphScope::Unscoped,
    )
    .unwrap();

    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Extrude
        })
    ));
    assert_eq!(
        &ir.model.features[0].source_properties,
        &BTreeMap::from([
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_entity"),
                "descendant-chain-entity".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_role_entry"),
                "role-entry".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_role_offset"),
                "7".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_role_ordinal"),
                "5".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_role_value"),
                "UnsupportedRole".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_selector_entry"),
                "selector-entry".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_selector_offset"),
                "2".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_selector_ordinal"),
                "4".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_selector_value"),
                "Length".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_value_kind"),
                "atom".to_string(),
            ),
            (
                cadmpeg_core::nonblank_literal!("catia_definition_chain_value_0_value_atom"),
                "3".to_string(),
            ),
        ])
    );
    assert_eq!(transfer.native_operation_definition_chain_value_count, 1);
}
