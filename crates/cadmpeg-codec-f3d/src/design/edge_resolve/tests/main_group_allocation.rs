// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_main_group_refusal_with_limit(
    operation: &'static str,
    surface_patch: bool,
    retained: bool,
) {
    let mut group = group(2, 10);
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.resolved_edge_slot = Some(17);
    let mut operands = vec![first];
    if surface_patch {
        group.try_set_members(vec![
            crate::records::identity::Located { value: 10, offset: 0 },
            crate::records::identity::Located { value: 11, offset: 11 },
        ]).unwrap();
        operands[0].surface_patch_recipe_structure = Some(
            crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure {
                clauses: std::array::from_fn(|_| {
                    crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                        fields: Vec::new(),
                        face_reference_ordinals: [0, 0],
                        edge_reference_ordinals: [0, 0],
                        entries: Vec::new(),
                    }
                }),
            },
        );
        let mut second = recipe_edge_operand(11, &[], &[]);
        second.resolved_edge_slot = Some(18);
        second.surface_patch_recipe_structure = operands[0].surface_patch_recipe_structure.clone();
        operands.push(second);
    }
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#main-edge-group")
        .unwrap();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        if retained {
            policy.limits.max_retained_bytes = limit;
        } else {
            policy.limits.max_collection_items = limit;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match resolved_edge_group(&group, std::slice::from_ref(&group), &operands,
            &[], Some(7), &feature_id, Some(&ctx)) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {},
            other => panic!("expected {operation} refusal: {other:?}"),
        }
    }
    panic!("no {operation} refusal");
}

fn assert_main_group_refusal(operation: &'static str, surface_patch: bool) {
    assert_main_group_refusal_with_limit(operation, surface_patch, false);
}

#[test]
fn generic_surface_patch_member_index_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch member index", true);
}

#[test]
fn generic_surface_patch_matched_operand_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch matched operand", true);
}

#[test]
fn generic_surface_patch_resolved_slot_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch resolved slot", true);
}

#[test]
fn generic_surface_patch_distinct_slot_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch distinct slot", true);
}

#[test]
fn generic_surface_patch_historical_edge_refuses_collection_limit() {
    assert_main_group_refusal("f3d generic surface patch historical edge", true);
}

#[test]
fn generic_surface_patch_historical_group_id_refuses_retained_limit() {
    assert_main_group_refusal_with_limit(
        "f3d generic surface patch historical group id", true, true);
}

#[test]
fn edge_group_member_identity_refuses_collection_limit() {
    assert_main_group_refusal("f3d edge group member identity", false);
}

#[test]
fn matched_edge_group_operand_refuses_collection_limit() {
    assert_main_group_refusal("f3d matched edge group operand", false);
}

#[test]
fn exact_edge_group_slot_refuses_collection_limit() {
    assert_main_group_refusal("f3d exact edge group slot", false);
}

#[test]
fn edge_group_matched_identity_refuses_collection_limit() {
    let group = group(2, 10);
    let operand = identity(10, &[]);
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#identity-index")
        .unwrap();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match resolved_edge_group(&group, std::slice::from_ref(&group), &[],
            std::slice::from_ref(&operand), Some(7), &feature_id, Some(&ctx)) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d edge group matched identity" => return,
            Err(CodecError::ResourceLimit(_)) => {},
            other => panic!("expected matched identity refusal: {other:?}"),
        }
    }
    panic!("no matched identity refusal");
}

fn assert_identity_transition_refusal(operation: &'static str) {
    let group = group(2, 10);
    let operand = identity(10, &[(17, 3.0), (18, 5.0)]);
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#transition-index")
        .unwrap();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match resolved_edge_treatment_group_with_corners(&group,
            std::slice::from_ref(&group), &[], std::slice::from_ref(&operand), &[], &[],
            Some(7), &feature_id, None, Some(&ctx)) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {},
            other => panic!("expected {operation} refusal: {other:?}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn single_identity_transition_slot_refuses_collection_limit() {
    assert_identity_transition_refusal("f3d single identity transition slot");
}

#[test]
fn group_identity_transition_slot_refuses_collection_limit() {
    assert_identity_transition_refusal("f3d group identity transition slot");
}

#[test]
fn compared_identity_transition_slot_refuses_collection_limit() {
    assert_identity_transition_refusal("f3d compared identity transition slot");
}

fn assert_combined_edge_refusal(operation: &'static str) {
    let group = group(2, 10);
    let recipe = recipe_edge_operand(10, &[], &[]);
    let identity = identity(10, &[]);
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#combined-edge")
        .unwrap();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match resolved_edge_group(&group, std::slice::from_ref(&group),
            std::slice::from_ref(&recipe), std::slice::from_ref(&identity),
            Some(7), &feature_id, Some(&ctx)) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {},
            other => panic!("expected {operation} refusal: {other:?}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn combined_edge_group_slot_refuses_collection_limit() {
    assert_combined_edge_refusal("f3d combined edge group slot");
}

#[test]
fn partial_edge_group_member_refuses_collection_limit() {
    assert_combined_edge_refusal("f3d partial edge group member");
}
