// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition};

#[test]
fn nx_trim_body_rejects_mixed_store_and_target_alias_tools() {
    let body = (114, "nx:om-data-blocks-2:block#114".to_string());
    let operand = crate::native::features::FeatureOperationBodyOperand {
        id: "operand#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 114,
        body_reference_ordinal: 0,
        ordinal: 0,
        operand_object_index: 113,
        raw_operand_object_index: vec![113],
        operand_data_block: Some("nx:om-data-blocks-2:block#113".to_string()),
        segment_body_bindings: Vec::new(),
        source_offset: 0,
    };
    let expected_target = Some(FeatureDefinition::TrimBodies {
        targets: BodySelection::Local {
            bodies: vec![body.1.clone()],
            native: "nx:om-object-index#114".to_string(),
        },
        tools: BodySelection::Unresolved,
        keep: BodyTrimSide::Unresolved,
    });

    let mut mixed_store_operand = operand.clone();
    mixed_store_operand.operand_data_block = Some("nx:om-data-blocks-3:block#113".to_string());
    assert_eq!(
        super::offset_store_trim_body_feature_definition(
            std::slice::from_ref(&body),
            &[&mixed_store_operand],
        ),
        expected_target.clone()
    );

    let mut duplicate_block_operand = operand.clone();
    duplicate_block_operand.operand_object_index = 112;
    assert_eq!(
        super::offset_store_trim_body_feature_definition(
            std::slice::from_ref(&body),
            &[&operand, &duplicate_block_operand],
        ),
        expected_target
    );

    let mut target_alias_operand = operand;
    target_alias_operand.operand_object_index = 115;
    target_alias_operand.operand_data_block = Some(body.1.clone());
    assert_eq!(
        super::offset_store_trim_body_feature_definition(
            std::slice::from_ref(&body),
            &[&target_alias_operand],
        ),
        Some(FeatureDefinition::TrimBodies {
            targets: BodySelection::Local {
                bodies: vec![body.1],
                native: "nx:om-object-index#114".to_string(),
            },
            tools: BodySelection::Unresolved,
            keep: BodyTrimSide::Unresolved,
        })
    );
}
