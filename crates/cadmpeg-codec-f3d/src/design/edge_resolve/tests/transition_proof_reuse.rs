// SPDX-License-Identifier: Apache-2.0
//! One recipe proof serves the identical member and group transition chains.

use super::*;
use crate::design::edge_resolve::EdgeTreatmentInputs;
use crate::ids;
use cadmpeg_core::CodecError;

fn proof_inputs() -> (
    DesignConstructionOperandGroup,
    DesignEdgeOperand,
    DesignEdgeIdentityOperand,
    cadmpeg_ir::features::FeatureId,
) {
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
fn edge_transition_recipe_member_refuses_before_storage() {
    let operation = "f3d transition recipe member";
    let error = crate::test_support::resource_refusal_at(ResourceDimension::CollectionItems, operation, 0, selection);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
}

#[test]
fn edge_transition_historical_output_refuses_before_storage() {
    let operation = "f3d group identity historical edge";
    let error = crate::test_support::resource_refusal_at(ResourceDimension::CollectionItems, operation, 0, selection);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
}

#[test]
fn edge_transition_proof_reuse_preserves_native_historical_identity_and_order() {
    crate::test_support::with_decode_context(|ctx| {
        let cadmpeg_ir::features::EdgeSelection::Historical { edges, native, .. } =
            selection(ctx).unwrap()
        else {
            panic!("complete transition proof");
        };
        assert_eq!(native.as_str(), "f3d:test:group-2");
        let prefix = ids::history_input_prefix(&cadmpeg_ir::identity_key!("single-proof"), 7);
        assert_eq!(
            edges.as_slice(),
            [
                ids::history_input_edge_id(&prefix, 17),
                ids::history_input_edge_id(&prefix, 18)
            ]
        );
    });
}
