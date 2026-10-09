// SPDX-License-Identifier: Apache-2.0
//! One recipe proof serves the identical member and group transition chains.

use super::*;
use crate::design::edge_resolve::EdgeTreatmentInputs;
use crate::ids;
use cadmpeg_core::CodecError;

fn proof_inputs() -> (DesignConstructionOperandGroup, DesignEdgeOperand, DesignEdgeIdentityOperand, cadmpeg_ir::features::FeatureId) {
    (
        group(2, 10),
        recipe_edge_operand(10, &[17, 18], &[]),
        identity(10, &[(17, 3.0), (18, 5.0)]),
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#single-proof").unwrap(),
    )
}

fn selection(ctx: &DecodeContext<'_>) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    let (group, recipe, identity, feature) = proof_inputs();
    resolved_edge_treatment_group_with_corners(
        &group,
        EdgeTreatmentInputs {
            groups: std::slice::from_ref(&group),
            operands: std::slice::from_ref(&recipe),
            identity_operands: std::slice::from_ref(&identity),
            vertex_operands: &[],
            histories: &[],
            previous_state_id: Some(7),
            feature_id: &feature,
            treatment_radius: None,
        },
        ctx,
    )
}

#[test]
fn edge_transition_proof_reuse_preserves_the_first_actual_recipe_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One matched identity + two slots for the singleton, group and first
    // comparison = seven items before the actual recipe member is kept.
    policy.limits.max_collection_items = 1 + 2 + 2 + 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = selection(&ctx).unwrap_err() else {
        panic!("first useful recipe proof must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "f3d transition recipe member");
    assert_eq!(limit.used, 7);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.charge_work(0, "later transition proof"), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn edge_transition_proof_reuse_skips_the_second_identical_recipe_query() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The seven identity items, one recipe member and two changed boundaries
    // complete the one useful proof. The next item belongs to actual output.
    policy.limits.max_collection_items = 1 + 2 + 2 + 2 + 1 + 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = selection(&ctx).unwrap_err() else {
        panic!("historical output needs its own item");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "f3d group identity historical edge");
    assert_eq!(limit.used, 10);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(selection(&ctx), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn edge_transition_proof_reuse_preserves_native_historical_identity_and_order() {
    crate::test_support::with_decode_context(|ctx| {
        let cadmpeg_ir::features::EdgeSelection::Historical { edges, native, .. } = selection(ctx).unwrap() else {
            panic!("complete transition proof");
        };
        assert_eq!(native.as_str(), "f3d:test:group-2");
        let prefix = ids::history_input_prefix(&cadmpeg_ir::identity_key!("single-proof"), 7);
        assert_eq!(edges.as_slice(), [ids::history_input_edge_id(&prefix, 17), ids::history_input_edge_id(&prefix, 18)]);
    });
}
