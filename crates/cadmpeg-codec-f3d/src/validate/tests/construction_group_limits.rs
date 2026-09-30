// SPDX-License-Identifier: Apache-2.0

pub(super) fn native(valid: bool, duplicate: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::{Located, ReferenceRun},
        topology::construction::{
            DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandRole,
        },
    };
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        DesignFeatureKind::Hole,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![100, 101]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{stream}:design-construction-operand-group#100"),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1_000,
        class_tag: "277".to_owned().try_into().unwrap(),
        members: vec![Located {
            value: 101,
            offset: 1_026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1_021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1_058,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1_062,
                variant: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
        ),
        role_offset: 1_040,
        paired_class_tag: "258".to_owned().try_into().unwrap(),
        paired_byte_offset: 1_080,
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native
        .design_construction_operand_groups
        .push(group.clone());
    if duplicate {
        native.design_construction_operand_groups.push(group);
    }
    if valid {
        native.design_parameter_scopes.push(scope);
        for (index, offset) in [(100, 1_000), (101, 1_100)] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{index}"),
                record_index: index,
                class_tag: "277".to_owned().try_into().unwrap(),
                byte_offset: offset,
            });
        }
    }
    native
}

fn group_error(
    valid: bool,
    duplicate: bool,
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(valid, duplicate);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_construction_operand_groups(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn construction_group_member_index_refuses_collection_limit() {
    let error = group_error(true, false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D construction operand group members")
    );
}

#[test]
fn construction_group_slot_refuses_collection_limit() {
    let error = group_error(true, false, 1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D construction operand group slots")
    );
}

#[test]
fn construction_group_invalid_finding_refuses_collection_limit() {
    let error = group_error(false, false, 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn construction_group_invalid_entity_refuses_retained_limit() {
    let error = group_error(false, false, u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn construction_group_duplicate_slot_finding_refuses_collection_limit() {
    let error = group_error(true, true, 3, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}
