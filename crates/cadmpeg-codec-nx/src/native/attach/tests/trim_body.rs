// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition, FeatureOperation};

#[test]
fn nx_trim_body_rejects_mixed_store_and_target_alias_tools() {
    let body = (114, "nx:om-data-blocks-2:block#114".to_string());
    let operand = crate::native::features::FeatureOperationBodyOperand {
        id: "operand#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 114,
        body_reference_ordinal: 0,
        ordinal: 0,
        operand: crate::om::compact::LocatedCompactIndex {
            atom: crate::om::compact::CompactIndexAtom::from_wire(113, &[113]).unwrap(),
            offset: 0,
        },
        operand_data_block: Some("nx:om-data-blocks-2:block#113".to_string()),
        segment_body_bindings: Vec::new(),
    };
    let expected_target = Some(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
        operands: cadmpeg_ir::features::TrimBodyOperands::new(
            BodySelection::local(vec![body.1.clone()], "nx:om-object-index#114".to_string())
                .unwrap(),
            BodySelection::Unresolved,
        )
        .unwrap(),

        keep: BodyTrimSide::Unresolved,
    }));

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
    duplicate_block_operand.operand.atom =
        crate::om::compact::CompactIndexAtom::read(&[112]).unwrap();
    assert_eq!(
        super::offset_store_trim_body_feature_definition(
            std::slice::from_ref(&body),
            &[&operand, &duplicate_block_operand],
        ),
        expected_target
    );

    let mut target_alias_operand = operand;
    target_alias_operand.operand.atom = crate::om::compact::CompactIndexAtom::read(&[115]).unwrap();
    target_alias_operand.operand_data_block = Some(body.1.clone());
    assert_eq!(
        super::offset_store_trim_body_feature_definition(
            std::slice::from_ref(&body),
            &[&target_alias_operand],
        ),
        Some(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::local(vec![body.1], "nx:om-object-index#114".to_string()).unwrap(),
                BodySelection::Unresolved
            )
            .unwrap(),

            keep: BodyTrimSide::Unresolved,
        }))
    );
}
