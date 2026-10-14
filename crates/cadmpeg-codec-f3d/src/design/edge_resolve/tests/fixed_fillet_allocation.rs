// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn project_fixture_fillet(
    policy: &DecodePolicy,
    variable: bool,
    complete: bool,
) -> Result<(), CodecError> {
    let mut scope_value = serde_json::to_value(fixed_scope()).unwrap();
    if variable {
        scope_value["fixed_fillet_parameters"]["groups"][0]["radii"] =
            serde_json::json!([1.0, 2.0, 3.0]);
        scope_value["fixed_fillet_parameters"]["groups"][0]["radius_record_indexes"] =
            serde_json::json!([5, 6, 7]);
        scope_value["fixed_fillet_parameters"]["groups"][0]["radius_offsets"] =
            serde_json::json!([0, 8, 16]);
        scope_value["fixed_fillet_parameters"]["groups"][0]["intermediate_parameters"] =
            serde_json::json!([0.5]);
        scope_value["fixed_fillet_parameters"]["groups"][0]
            ["intermediate_parameter_record_indexes"] = serde_json::json!([8]);
        scope_value["fixed_fillet_parameters"]["groups"][0]["intermediate_parameter_offsets"] =
            serde_json::json!([24]);
    }
    let scope: DesignParameterScope = serde_json::from_value(scope_value).unwrap();
    let selection_group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_state_id = Some(7);
    operand.resolved_edge_slot = Some(17);
    let identity = identity(10, &[(17, 3.0), (18, 5.0), (19, 3.0)]);
    let operands = if complete {
        std::slice::from_ref(&operand)
    } else {
        &[]
    };
    let identities = if complete {
        &[][..]
    } else {
        std::slice::from_ref(&identity)
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).unwrap();
    project_fixed_fillet(
        &scope,
        std::slice::from_ref(&selection_group),
        operands,
        identities,
        &ctx,
    )
    .map(|_| ())
}

fn assert_fixed_fillet_refusal(operation: &'static str, variable: bool, complete: bool) {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        operation,
        |cap| {
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = cap;
            project_fixture_fillet(&policy, variable, complete)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(_)));
}

#[test]
fn fixed_fillet_scope_group_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet scope group", false, false);
}

#[test]
fn fixed_fillet_complete_group_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet complete group", false, true);
}

#[test]
fn fixed_fillet_identity_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet identity", false, false);
}

#[test]
fn fixed_fillet_output_group_charges_collection_items() {
    // Released identity scratch peaks above the output group charge.
    cadmpeg_test_support::refusal::assert_charged(
        ResourceDimension::CollectionItems,
        "f3d fixed fillet output group",
        || project_fixture_fillet(&DecodePolicy::service(), false, false),
    );
}

#[test]
fn fixed_fillet_radius_point_refuses_collection_limit() {
    assert_fixed_fillet_refusal("f3d fixed fillet radius point", true, true);
}
