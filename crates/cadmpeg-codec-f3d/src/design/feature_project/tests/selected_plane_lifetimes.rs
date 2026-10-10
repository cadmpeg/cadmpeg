// SPDX-License-Identifier: Apache-2.0
//! Selected plane rows own only live temporary and returned backing.

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
};
use crate::records::topology::entity_selection::DesignEntitySelectionOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::{u64_from_index, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SCRATCH_ALLOWANCE: u64 = 4096;

fn group(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    members: Vec<u32>,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("262".to_owned())
                .unwrap(),

            members: members
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn selection(record: u32, ordinal: u32, primary: u64) -> DesignEntitySelectionOperand {
    let selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: format!("f3d:Design/BulkStream.dat:entity-selection#{record}"),
                scope_record_index: 77,
                group_record_index: 100,
                group_member_ordinal: ordinal,
                record_index: record,
                byte_offset: 0,
                class_tag: "372".to_owned().try_into().unwrap(),
                asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                asset_id_offset: 0,
                context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                context_id_offset: 0,
                identity_record_index: record + 3,
                identity_record_offset: 0,
                primary_identity: primary,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: record + 4,
                next_byte_offset: 29,
            },
        )
        .unwrap();
    selection
}

fn plane(record: u32) -> DesignParameterScope {
    let mut plane = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:scope#{record}"),
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        record,
    );
    plane.with_work_plane_transform(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );
    plane
}

fn scope() -> DesignParameterScope {
    DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        DesignFeatureKind::SplitFace,
        77,
    )
}

fn row_bytes() -> u64 {
    // The genuine core vector starts with four slots for a non-ZST pointer.
    4 * u64::try_from(std::mem::size_of::<&DesignParameterScope>()).unwrap()
}

fn assert_discarded_prefix(second_primary: u64, include_second: bool) {
    let scope = scope();
    let group = group(77, 0, 100, vec![101, 102], DesignOperandRole::ROLE_0X21);
    let mut selections = vec![selection(101, 0, 600)];
    if include_second {
        selections.push(selection(102, 1, second_primary));
    }
    let planes = [plane(601)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_ALLOWANCE;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            assert!(
                super::super::selected_work_planes(ctx, &scope, &group, &selections, &planes)
                    .unwrap()
                    .is_none()
            );
            drop(
                ctx.reserve_scoped(SCRATCH_ALLOWANCE, "reuse discarded selected plane rows")
                    .unwrap(),
            );
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn selected_plane_lifetime_missing_later_operand_refunds_prefix() {
    assert_discarded_prefix(600, false);
}

#[test]
fn selected_plane_lifetime_duplicate_later_target_refunds_prefix() {
    assert_discarded_prefix(600, true);
}

#[test]
fn selected_plane_lifetime_missing_later_plane_refunds_prefix() {
    assert_discarded_prefix(601, true);
}

#[test]
fn selected_plane_lifetime_survivor_retains_only_rows_in_source_order() {
    let scope = scope();
    let group = group(77, 0, 100, vec![101, 102], DesignOperandRole::ROLE_0X21);
    let selections = [selection(101, 0, 601), selection(102, 1, 600)];
    let planes = [plane(601), plane(602)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_ALLOWANCE;
    policy.limits.max_retained_bytes = row_bytes();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            let mut storage = ctx
                .provisional_retained("test actual selected plane output lifetime")
                .unwrap();
            let selected = storage
                .with_storage(|| {
                    super::super::selected_work_planes(ctx, &scope, &group, &selections, &planes)
                })
                .unwrap()
                .unwrap();
            assert_eq!(selected.len(), 2);
            assert!(std::ptr::eq(selected[0], &raw const planes[1]));
            assert!(std::ptr::eq(selected[1], &raw const planes[0]));
            drop(
                ctx.reserve_scoped(SCRATCH_ALLOWANCE, "reuse selected plane index scratch")
                    .unwrap(),
            );
            assert_eq!(selected[0].id, planes[1].id);
            assert_eq!(selected[1].id, planes[0].id);
            drop(selected);
            drop(storage);
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn selected_plane_lifetime_parent_scratch_keeps_rows_until_data_drop() {
    let scope = scope();
    let group = group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let selections = [selection(101, 0, 600)];
    let planes = [plane(601)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_ALLOWANCE;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            let mut storage = ctx
                .reserve_scoped(0, "test actual selected plane temporary caller")
                .unwrap();
            let selected = storage
                .with_storage(|| {
                    super::super::selected_work_planes(ctx, &scope, &group, &selections, &planes)
                })
                .unwrap()
                .unwrap();
            drop(
                ctx.reserve_scoped(
                    SCRATCH_ALLOWANCE - row_bytes(),
                    "reuse remaining plane scratch while rows live",
                )
                .unwrap(),
            );
            assert!(std::ptr::eq(selected[0], &raw const planes[0]));
            drop(selected);
            drop(storage);
            drop(
                ctx.reserve_scoped(SCRATCH_ALLOWANCE, "reuse all plane scratch after data drop")
                    .unwrap(),
            );
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn selected_plane_lifetime_retained_transfer_refuses_one_below_real_rows() {
    let scope = scope();
    let group = group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let selections = [selection(101, 0, 600)];
    let planes = [plane(601)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_ALLOWANCE;
    policy.limits.max_retained_bytes = row_bytes() - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            super::super::selected_work_planes(ctx, &scope, &group, &selections, &planes)
        else {
            panic!("actual returned row backing must refuse transfer");
        };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.operation, "hold F3D selected work plane rows");
        assert_eq!((original.used, original.additional), (0, row_bytes()));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(super::super::selected_work_planes(ctx, &scope, &group, &[], &planes), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}
