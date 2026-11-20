// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn a_keep_tools_join_without_a_stored_predecessor_uses_the_transition() {
    use crate::records::feature::combine::{
        DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation, DesignCombineTools,
    };
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    use cadmpeg_ir::features::{
        BodySelection, BooleanKind, CombineOperands, DistinctMembers, Feature, FeatureDefinition,
        FeatureEvaluation, FeatureId, FeatureOperation,
    };

    let resolved_with = |form: Option<DesignCombineForm>,
                         mut histories: Vec<crate::records::AsmHistory>| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Combine,
            1,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(1);
                draft.previous_history_state_id = None;
                draft.layout_fixture_tail();
            })
            .unwrap();
        if let DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
            slot.operation = form.map(|form| DesignCombineOperation {
                form,
                operation: BooleanKind::Join,
                operation_offset: 0,
                keep_tools: true,
                keep_tools_offset: 0,
                target_record_index: 11,
                target_body_node: None,
                tools: DesignCombineTools {
                    first: DesignCombineBodySelection {
                        record_index: 12,
                        external_identity: None,
                        body_node: None,
                    },
                    additional: Vec::new(),
                },
                destination: Default::default(),
            });
        }
        let mut feature = Feature {
            id: FeatureId::mint("test:model:feature#combine").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: Some("Combine".into()),
            source_text: None,
            source_content: Default::default(),
            evaluation: FeatureEvaluation::new(
                FeatureDefinition::Operation(FeatureOperation::Combine {
                    operands: CombineOperands::new(
                        BodySelection::Unresolved,
                        BodySelection::Unresolved,
                    )
                    .unwrap(),
                    op: BooleanKind::Join,
                    keep_tools: true,
                }),
                DistinctMembers::try_from(vec![]).unwrap(),
            ),
            native_ref: Some(scope.id.clone()),
            product_definition: None,
        };
        let bound = super::super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut histories,
            &[],
            &Default::default(),
            None,
        )
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Combine { op, operands, .. }) =
            feature.evaluation.definition()
        else {
            panic!("expected Combine");
        };
        assert!(matches!(
            operands.clone().into_parts(),
            (BodySelection::Unresolved, BodySelection::Unresolved)
        ));
        let charged = bound.losses.iter().any(|note| {
            note.code == crate::loss::F3dLossCode::FeatureCombineKeptToolsCutAssumed.kind()
        });
        (*op, charged)
    };
    let history_with_before = |id: &str, bodies_before: &[i64]| {
        crate::test_support::history::history(
            id.into(),
            0,
            None,
            false,
            vec![
                state(1, 1, Some(0), solid_boxes_topology(&[1])),
                state(0, 0, None, solid_boxes_topology(bodies_before)),
            ],
        )
    };
    let resolved = |form: Option<DesignCombineForm>, bodies_before: &[i64]| {
        resolved_with(
            form,
            vec![history_with_before("f3d:asm-history#1", bodies_before)],
        )
    };

    assert_eq!(resolved(None, &[1]), (BooleanKind::Cut, true));
    assert_eq!(
        resolved(Some(DesignCombineForm::Standard), &[1]),
        (BooleanKind::Cut, true)
    );
    assert_eq!(
        resolved(Some(DesignCombineForm::Standard), &[1, 2]),
        (BooleanKind::Join, false)
    );
    assert_eq!(
        resolved(Some(DesignCombineForm::OperandLists), &[1]),
        (BooleanKind::Join, false)
    );

    // A state with no topology has no transition, so the stored Join stays unchanged.
    let no_topology_history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), None),
            state(0, 0, None, solid_boxes_topology(&[1])),
        ],
    );
    assert_eq!(
        resolved_with(Some(DesignCombineForm::Standard), vec![no_topology_history]),
        (BooleanKind::Join, false)
    );

    // State 1 present in two histories has a repeated predecessor, so the stored Join stays unchanged.
    assert_eq!(
        resolved_with(
            Some(DesignCombineForm::Standard),
            vec![
                history_with_before("f3d:asm-history#1", &[1]),
                history_with_before("f3d:asm-history#2", &[1]),
            ],
        ),
        (BooleanKind::Join, false)
    );
}
