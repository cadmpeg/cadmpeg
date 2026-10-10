// SPDX-License-Identifier: Apache-2.0
//! Preserve a callback refusal when the edited Combine pair also overlaps.

use crate::history::{bind_feature_body_selections, FeatureBodySelectionInputs};
use crate::history_records::AsmHistory;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, CombineOperands, Feature, FeatureDefinition, FeatureEvaluation,
    FeatureOperation,
};

fn inputs(overlap: bool) -> (Feature, DesignParameterScope, AsmHistory) {
    let (mut feature, mut scope, mut history, _) =
        crate::history::test_support::output_binding_inputs();
    scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Combine
                .try_into()
                .expect("Combine payload");
            draft.layout_fixture_tail();
        })
        .expect("Combine scope");
    let crate::records::feature::scope::DesignScopePayloadMut::Combine(operation) =
        scope.payload_mut()
    else {
        panic!("Combine payload")
    };
    *operation = Some(crate::records::feature::combine::DesignCombineOperation {
        form: crate::records::feature::combine::DesignCombineForm::ExtendedReference,
        operation: BooleanKind::Join,
        operation_offset: 0,
        keep_tools: false,
        keep_tools_offset: 0,
        target_record_index: 11,
        tools: crate::records::feature::combine::DesignCombineTools {
            first: crate::records::feature::combine::DesignCombineBodySelection {
                record_index: 12,
                external_identity: None,
            },
            additional: Vec::new(),
        },
    });
    let transition = history.states[0]
        .transition
        .as_mut()
        .expect("current transition");
    transition.topology.bodies.inserted.clear();
    transition.topology.bodies.updated.push(1);
    let crate::history_records::AsmTopologyCache::Complete(previous) =
        &mut history.states[1].topology_cache
    else {
        panic!("previous topology")
    };
    previous.bodies.push(1);
    let setup = cadmpeg_test_support::service_decode_context();
    let tools = if overlap {
        BodySelection::historical(
            crate::ids::history_input_state_id_charged(&setup, &feature.id, 0)
                .expect("input state"),
            vec![
                crate::ids::history_input_body_id_charged(&setup, &feature.id, 0, 1)
                    .expect("input body"),
            ],
            "native:tool".into(),
            &setup,
        )
        .expect("historical storage")
        .expect("historical tool")
    } else {
        BodySelection::Unresolved
    };
    let operands =
        CombineOperands::new(BodySelection::Native("native:target".into()), tools, &setup)
            .expect("initial pair admission")
            .expect("initial pair is disjoint");
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Combine {
            operands,
            op: BooleanKind::Join,
            keep_tools: false,
        },
    ));
    (feature, scope, history)
}

fn bind(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), CodecError> {
    bind_feature_body_selections(
        ctx,
        features,
        &FeatureBodySelectionInputs {
            scopes,
            groups: &[],
            body_recipe_operands: &[],
            construction_recipes: &[],
            persistent_design_links: &[],
            histories,
            bodies: &[],
            regions: &[],
            shells: &[],
        },
    )
}

#[test]
fn combine_callback_refusal_wins_over_post_edit_overlap_rejection() {
    let (mut feature, scope, history) = inputs(true);
    let original_feature = feature.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_collection_items = u64::MAX;
    policy.limits.max_retained_bytes = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        // The target is first replaced by the revised historical body. This
        // subsequent tool query refuses, leaving the draft target and tools
        // on the same historical body. The pair's Standard overlap rejection
        // must not replace the earlier authentic callback refusal.
        let operation = "index F3D scope-reference body recipe operands";
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        let Err(CodecError::ResourceLimit(original)) = bind(
            ctx,
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            std::slice::from_ref(&history),
        ) else {
            panic!("original tool query refusal")
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, operation);
        assert!(original.additional > 0);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert_eq!(feature, original_feature);
        let Err(CodecError::ResourceLimit(sticky)) = bind(ctx, &mut [], &[], &[]) else {
            panic!("original refusal before the next empty pass")
        };
        assert_eq!(sticky, original);
        assert_eq!(feature, original_feature);
    });
}

#[test]
fn combine_post_edit_overlap_keeps_ordinary_grammar_rejection_and_native_pair() {
    let (mut feature, scope, history) = inputs(true);
    let original_feature = feature.clone();
    crate::test_support::with_decode_context(|ctx| {
        let Err(CodecError::Malformed(message)) = bind(
            ctx,
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            std::slice::from_ref(&history),
        ) else {
            panic!("overlapping edited pair rejects")
        };
        assert_eq!(
            message,
            "target and tools must have valid arity and disjoint membership"
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(feature, original_feature);
}

#[test]
fn combine_nonoverlapping_target_retains_exact_historical_and_native_identity() {
    let (mut feature, scope, history) = inputs(false);
    let expected_feature_id = feature.id.clone();
    let expected_native_ref = feature.native_ref.clone();
    let setup = cadmpeg_test_support::service_decode_context();
    let expected_state = crate::ids::history_input_state_id_charged(&setup, &feature.id, 0)
        .expect("expected input state");
    let expected_body = crate::ids::history_input_body_id_charged(&setup, &feature.id, 0, 1)
        .expect("expected input body");
    crate::test_support::with_decode_context(|ctx| {
        bind(
            ctx,
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            std::slice::from_ref(&history),
        )
        .expect("nonoverlapping edited pair");
        assert_eq!(ctx.resource_refusal(), None);
    });
    let FeatureDefinition::Operation(FeatureOperation::Combine {
        operands,
        op,
        keep_tools,
    }) = feature.evaluation.definition()
    else {
        panic!("Combine definition preserved")
    };
    let BodySelection::Historical {
        state,
        bodies,
        native,
    } = operands.target()
    else {
        panic!("revised historical target")
    };
    assert_eq!(state, &expected_state);
    assert_eq!(bodies.as_slice(), &[expected_body]);
    assert_eq!(native, "native:target");
    assert_eq!(operands.tools(), &BodySelection::Unresolved);
    assert_eq!(*op, BooleanKind::Join);
    assert!(!keep_tools);
    assert_eq!(feature.id, expected_feature_id);
    assert_eq!(feature.native_ref, expected_native_ref);
}
