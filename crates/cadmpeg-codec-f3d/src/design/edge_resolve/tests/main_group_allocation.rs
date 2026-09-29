// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_main_group_refusal(operation: &'static str, surface_patch: bool) {
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
        policy.limits.max_collection_items = limit;
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
fn edge_group_member_identity_refuses_collection_limit() {
    assert_main_group_refusal("f3d edge group member identity", false);
}

#[test]
fn matched_edge_group_operand_refuses_collection_limit() {
    assert_main_group_refusal("f3d matched edge group operand", false);
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
