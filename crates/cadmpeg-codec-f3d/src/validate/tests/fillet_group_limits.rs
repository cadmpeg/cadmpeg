// SPDX-License-Identifier: Apache-2.0

fn native() -> crate::native::F3dNative {
    use crate::records::{
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        topology::{construction::DesignConstructionOperandRole, extrude_selection::DesignOperandRole},
    };
    let mut native = super::construction_group_limits::native(true, false);
    native.design_parameter_scopes.clear();
    native.design_parameter_scopes.push(DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Fillet,
        10,
    ));
    native.design_construction_operand_groups[0].operand_role =
        DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_B);
    native
}

fn group_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let native = native();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    super::super::validate_fillet_operand_groups(
        &ctx, &mut Vec::new(), &std::collections::HashSet::new(),
    ).unwrap_err()
}

#[test]
fn fillet_missing_radius_finding_refuses_collection_limit() {
    let error = group_error(0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}

#[test]
fn fillet_missing_radius_entity_refuses_retained_limit() {
    let error = group_error(u64::MAX, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity"));
}

fn full_round_native() -> crate::native::F3dNative {
    use crate::records::{
        recipes::ConstructionRecipeKind,
        topology::{
            body_recipe::DesignOperandGroup,
            construction::DesignConstructionOperandRole,
            extrude_selection::DesignOperandRole,
            face::{DesignFaceOperand, DesignFaceOperandDraft},
        },
    };
    let mut native = native();
    native.design_construction_operand_groups[0].operand_role =
        DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    native.design_face_operands.push(DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-face-operand#101".into(),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        group: Some(DesignOperandGroup {
            group_record_index: 100,
            group_member_ordinal: 0,
        }),
        record_index: 101,
        byte_offset: 1_000,
        class_tag: "365".to_owned().try_into().unwrap(),
        paired_byte_offset: 1_016,
        paired_class_tag: "366".to_owned().try_into().unwrap(),
        recipe_record_index: 104,
        recipe_record_byte_offset: 1_032,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
        recipe_prefix_offset: 1_043,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 1_063,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 105,
        next_byte_offset: 1_071,
    }).unwrap());
    native
}

fn full_round_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let native = full_round_native();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    super::super::validate_fillet_operand_groups(
        &ctx, &mut Vec::new(), &std::collections::HashSet::new(),
    ).unwrap_err()
}

#[test]
fn fillet_full_round_finding_refuses_collection_limit() {
    let error = full_round_error(0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}

#[test]
fn fillet_full_round_entity_refuses_retained_limit() {
    let error = full_round_error(u64::MAX, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity"));
}
