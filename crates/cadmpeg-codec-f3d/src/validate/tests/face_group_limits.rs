// SPDX-License-Identifier: Apache-2.0

fn selection_operand() -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    use crate::records::{
        identity::{DesignSecondaryIdentity, Located},
        mesh::DesignRelaxedGuidText,
        topology::entity_selection::{DesignEntitySelectionOperand, DesignEntitySelectionOperandDraft},
    };
    DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-entity-selection-operand#200".into(),
        scope_record_index: 10,
        group_record_index: 100,
        group_member_ordinal: 0,
        record_index: 200,
        byte_offset: 1_000,
        class_tag: "338".to_owned().try_into().unwrap(),
        asset_id: DesignRelaxedGuidText::try_from(
            "11111111-2222-4333-8444-555555555555".to_owned(),
        ).unwrap(),
        asset_id_offset: 1_034,
        context_id: DesignRelaxedGuidText::try_from(
            "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned(),
        ).unwrap(),
        context_id_offset: 1_100,
        identity_record_index: 203,
        identity_record_offset: 2_000,
        primary_identity: 949,
        primary_identity_offset: 2033,
        secondary: Some(DesignSecondaryIdentity {
            identity: Located { value: 249, offset: 2041 },
            curve_identity: None,
        }),
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 204,
        next_byte_offset: 2049,
    }).unwrap()
}

fn resolution_error(with_selection: bool, max_items: u64, max_retained: u64)
    -> cadmpeg_core::CodecError
{
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut native = crate::native::F3dNative::default();
    if with_selection {
        native.design_entity_selection_operands.push(selection_operand());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    super::super::validate_face_group_member_resolution(
        &ctx,
        &mut Vec::new(),
        [("f3d:Design/BulkStream.dat", 100, 201)].into_iter().collect(),
        &std::collections::HashSet::new(),
        &native.design_entity_selection_operands,
    ).unwrap_err()
}

#[test]
fn face_group_entity_selection_index_refuses_collection_limit() {
    let error = resolution_error(true, 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D face group entity selections"));
}

#[test]
fn face_group_unresolved_id_refuses_retained_limit() {
    let error = resolution_error(false, u64::MAX, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D face group member identity"));
}

#[test]
fn face_group_unresolved_finding_refuses_collection_limit() {
    let error = resolution_error(false, 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}
