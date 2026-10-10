// SPDX-License-Identifier: Apache-2.0
//! Draft scans use bounded role slots and preserve native anchor choices.

use crate::records::feature::direct_face::DesignDraftOperation;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::ReferenceRun;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DraftAnchor, FaceSelection, FeatureDefinition, FeatureOperation};

fn scope(references: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        DesignFeatureKind::Draft,
        77,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let DesignScopePayloadMut::Draft(slot) = scope.payload_mut() else {
        panic!("actual Draft payload");
    };
    *slot = Some(DesignDraftOperation {
        angle: cadmpeg_ir::scalar::Angle::new(0.25).unwrap(),
        angle_record_index: 90,
        angle_offset: 0,
        opposite_angle_record_index: 91,
        opposite_angle_offset: 0,
    });
    scope
}

fn groups() -> [DesignConstructionOperandGroup; 2] {
    [
        super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X10),
        super::group(77, 2, 102, vec![103], DesignOperandRole::ROLE_0X21),
    ]
}

fn project(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Result<Option<FeatureDefinition>, CodecError> {
    super::super::super::project_draft(ctx, scope, &[], groups, &[], &[], &[])
}

#[test]
fn draft_scope_queries_preserve_native_faces_anchor_and_empty_streams_without_role_storage() {
    for empty_stream in [false, true] {
        let mut scope = scope(vec![100, 101, 102, 103]);
        let mut groups = groups();
        if empty_stream {
            scope.id = ":scope#77".into();
            groups[0].id = ":group#100".into();
            groups[1].id = ":group#102".into();
        }
        let original = groups.clone();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let Some(FeatureDefinition::Operation(FeatureOperation::Draft {
                faces: FaceSelection::Native(faces),
                anchor:
                    DraftAnchor::NeutralPlane {
                        plane: FaceSelection::Native(plane),
                        pull,
                    },
                angle,
                outward,
            })) = project(ctx, &scope, &groups).unwrap()
            else {
                panic!("native Draft selections must remain exact");
            };
            assert_eq!(faces, groups[0].id);
            assert_eq!(plane, groups[1].id);
            assert!(pull.is_none());
            assert_eq!(angle.unwrap().get(), 0.25);
            assert_eq!(outward, Some(false));
            assert_eq!(ctx.resource_refusal(), None);
        });
        assert_eq!(groups, original);
    }
}

#[test]
fn draft_scope_queries_missing_member_keeps_native_input_and_uses_no_storage() {
    let scope = scope(vec![100, 102, 103]);
    let groups = [
        super::group(
            77,
            0,
            100,
            vec![101, 999, 999],
            DesignOperandRole::ROLE_0X10,
        ),
        super::group(77, 2, 102, vec![103], DesignOperandRole::ROLE_0X21),
    ];
    let original = groups.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(project(ctx, &scope, &groups).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(groups, original);
}

#[test]
fn draft_scope_queries_no_role_group_skips_unused_reference_run() {
    let scope = scope(vec![100; 4096]);
    let groups = [super::group(
        77,
        0,
        100,
        vec![101],
        DesignOperandRole::ROLE_0X10,
    )];
    let stream = crate::ids::native_stream(&scope.id).unwrap();
    let group_stream = crate::ids::native_stream(&groups[0].id).unwrap();
    // Single-face query and role query each parse the expected suffix once,
    // visit one raw group, parse and compare its stream, then probe the end.
    let reached = u64::try_from(
        2 * (scope.id.len() - stream.len() + 1 + groups[0].id.len() - group_stream.len()
            + stream.len()
            + 1),
    )
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(project(ctx, &scope, &groups).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(refusal)) =
            ctx.charge_work(1, "check Draft reached work")
        else {
            panic!("the actual two queries must consume exactly their reached work");
        };
        assert_eq!((refusal.used, refusal.additional), (reached, 1));
    });
    assert_eq!(scope.reference_members().len(), 4096);
}

#[test]
fn draft_scope_queries_third_role_refuses_shape_without_owned_role_storage() {
    let scope = scope(vec![100, 101, 102, 103, 104, 105, 106, 107]);
    let groups = [
        super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X10),
        super::group(77, 2, 102, vec![103], DesignOperandRole::ROLE_0X21),
        super::group(77, 4, 104, vec![105], DesignOperandRole::ROLE_0X21),
        super::group(77, 6, 106, vec![107], DesignOperandRole::ROLE_0X21),
    ];
    let original = groups.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(project(ctx, &scope, &groups).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(groups, original);
}

fn assert_refusal(operation: &'static str) {
    let scope = scope(vec![100, 101, 102, 103]);
    let groups = groups();
    let original = groups.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
        let Err(CodecError::ResourceLimit(refusal)) = project(ctx, &scope, &groups) else {
            panic!("actual Draft query phase must refuse before output construction");
        };
        assert_eq!(refusal.operation, operation);
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        
    });
    assert_eq!(groups, original);
}

#[test]
fn draft_scope_queries_scope_stream_preserves_original_refusal() {
    assert_refusal("resolve F3D Draft role scope stream");
}

#[test]
fn draft_scope_queries_role_source_preserves_original_refusal() {
    assert_refusal("visit F3D Draft role group");
}

#[test]
fn draft_scope_queries_role_stream_preserves_original_refusal() {
    assert_refusal("match F3D Draft role group stream");
}

#[test]
fn draft_scope_queries_group_member_preserves_original_refusal() {
    assert_refusal("visit F3D Draft group member");
}

#[test]
fn draft_scope_queries_scope_member_preserves_original_refusal() {
    assert_refusal("visit F3D Draft scope member");
}

#[test]
fn draft_scope_queries_face_reference_preserves_original_refusal() {
    assert_refusal("visit F3D Draft face group reference");
}
