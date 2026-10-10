// SPDX-License-Identifier: Apache-2.0
//! Unique queries admit raw source visits and preserve repeated-match refusal.

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::{DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame};
use crate::records::topology::entity_selection::DesignEntitySelectionOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension, u64_from_index};
use cadmpeg_core::CodecError;

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
    DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#77", DesignFeatureKind::SplitFace, 77)
}


#[test]
fn unique_query_duplicate_stops_before_the_unvisited_source_suffix() {
    let values = [5, 5, 99, 99, 99];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visited = Vec::new();
        assert!(super::super::unique_feature_match_for_decode(ctx, &values, |value| {
            visited.push(*value);
            Ok(*value == 5)
        }, "visit actual unique query fixture").unwrap().is_none());
        assert_eq!(visited, [5, 5]);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn unique_query_single_match_returns_the_actual_source_reference() {
    let values = [1, 5, 3];
    let mut policy = DecodePolicy::service();
    // Three visited scalar slots and the actual second search's end probe.
    policy.limits.max_work_units = 4;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let selected = super::super::unique_feature_match_for_decode(ctx, &values, |value| Ok(*value == 5), "visit actual unique query fixture").unwrap().unwrap();
        assert!(std::ptr::eq(selected, &values[1]));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn unique_query_original_visit_refusal_does_not_execute_the_predicate() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visits = 0;
        let Err(CodecError::ResourceLimit(original)) = super::super::unique_feature_match_for_decode(ctx, &[5], |_| { visits += 1; Ok(true) }, "visit actual unique query fixture") else { panic!("source visit must refuse"); };
        assert_eq!(visits, 0);
        assert_eq!(original.operation, "visit actual unique query fixture");
        assert_eq!((original.used, original.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(super::super::unique_feature_match_for_decode(ctx, &[] as &[i32], |_| { visits += 1; Ok(true) }, "later unique query"), Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(visits, 0);
    });
}

#[test]
fn unique_query_selected_plane_duplicate_skips_the_large_operand_tail() {
    let scope = scope();
    let group = group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let first = selection(101, 0, 600);
    let mut tail = selection(102, 1, 601);
    tail.id = format!("f3d:Other/BulkStream.dat:{}", "x".repeat(4096));
    let selections = [first.clone(), first, tail];
    let scope_stream = crate::ids::native_stream(&scope.id).unwrap();
    let operand_stream = crate::ids::native_stream(&selections[0].id).unwrap();
    // The scope reverse scan, then two raw operand slots, reverse scans and
    // equal stream comparisons. The duplicate ends the query without a probe.
    let reached = u64::try_from(scope.id.len() - scope_stream.len()
        + 2 * (1 + selections[0].id.len() - operand_stream.len() + operand_stream.len())).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(super::super::selected_work_planes(ctx, &scope, &group, &selections, &[]).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = super::super::selected_work_planes(ctx, &scope, &group, &selections, &[]) else { panic!("one below actual last compared byte must refuse"); };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "compare F3D selected work plane operand stream");
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(super::super::selected_work_planes(ctx, &scope, &group, &[], &[]), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

fn assert_selected_plane_refusal(operation: &'static str) {
    let scope = scope();
    let group = group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let selections = [selection(101, 0, 600)];
    let planes = [plane(601)];
    let mut policy = DecodePolicy::service();
    // RefusalProbe searches only a dimension explicitly marked unlimited.
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let Err(CodecError::ResourceLimit(original)) = super::super::selected_work_planes(ctx, &scope, &group, &selections, &planes) else { panic!("actual query phase must refuse"); };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, operation);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(super::super::selected_work_planes(ctx, &scope, &group, &[], &[]), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn unique_query_selected_plane_scope_stream_preserves_original_refusal() {
    assert_selected_plane_refusal("resolve F3D selected work plane stream");
}

#[test]
fn unique_query_selected_plane_source_visit_preserves_original_refusal() {
    assert_selected_plane_refusal("visit F3D selected work plane operand");
}

#[test]
fn unique_query_selected_plane_operand_stream_preserves_original_refusal() {
    assert_selected_plane_refusal("resolve F3D selected work plane operand stream");
}

#[test]
fn unique_query_selected_plane_stream_comparison_preserves_original_refusal() {
    assert_selected_plane_refusal("compare F3D selected work plane operand stream");
}

mod remaining;

mod operand_groups;

mod callback_refusal;

mod entity_membership;
