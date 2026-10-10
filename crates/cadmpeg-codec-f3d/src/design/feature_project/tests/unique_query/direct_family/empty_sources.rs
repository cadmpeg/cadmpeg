// SPDX-License-Identifier: Apache-2.0
//! Constant absence skips scope queries and preserves the caller's original refusal.

use cadmpeg_core::decode::{DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FaceSelection;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut};
use crate::records::topology::extrude_selection::DesignOperandRole;

fn no_query_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy
}

fn assert_no_query(query: impl FnOnce(&DecodeContext<'_>) -> Result<bool, CodecError>) {
    crate::test_support::with_decode_policy(&no_query_policy(), |ctx| {
        assert!(query(ctx).unwrap());
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "observe F3D no-query work") else { panic!("zero executed work"); };
        assert_eq!((original.used, original.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

fn assert_original_refusal(query: impl FnOnce(&DecodeContext<'_>) -> Result<bool, CodecError>) {
    crate::test_support::with_decode_policy(&no_query_policy(), |ctx| {
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original F3D no-query refusal") else { panic!("original refusal"); };
        assert!(matches!(query(ctx), Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

fn long_scope() -> DesignParameterScope {
    let mut scope = super::super::scope();
    scope.id = format!("f3d:Design/BulkStream.dat:{}", "x".repeat(4096));
    scope
}

fn thicken_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(&long_scope().id, DesignFeatureKind::Thicken, 77);
    let DesignScopePayloadMut::Thicken(slot) = scope.payload_mut() else { panic!("Thicken payload"); };
    *slot = Some(crate::records::feature::direct_face::DesignThickenOperation {
        signed_thickness: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
        thickness_record_index: 90,
        thickness_offset: 0,
    });
    scope
}

fn base_flange_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(&long_scope().id, DesignFeatureKind::BaseFlange, 77);
    let DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() else { panic!("BaseFlange payload"); };
    *slot = Some(crate::records::feature::scope::DesignBaseFlangeScope {
        base_flange_operation: Some(crate::records::feature::sheet_metal::DesignBaseFlangeOperation {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(0.25).unwrap(),
            thickness_offset: 0,
            profile_group_record_index: 100,
            profile_record_index: 101,
            thickness_record_index: 90,
            settings_record_index: 91,
        }),
        base_flange_profile: None,
    });
    scope
}

#[test]
fn thread_empty_references_skip_unused_scope_and_all_storage() {
    let scope = long_scope();
    let original = scope.clone();
    assert_no_query(|ctx| Ok(super::super::super::super::project_thread_face_selection(ctx, &scope, &[], &[], &[])? == FaceSelection::Unresolved));
    assert_eq!(scope, original);
}

#[test]
fn thread_empty_references_preserve_original_stored_refusal() {
    let scope = long_scope();
    assert_original_refusal(|ctx| Ok(super::super::super::super::project_thread_face_selection(ctx, &scope, &[], &[], &[])? == FaceSelection::Unresolved));
}

#[test]
fn thread_empty_groups_skip_unused_scope_with_nonempty_references() {
    let scope = long_scope();
    let original = scope.clone();
    assert_no_query(|ctx| Ok(super::super::super::super::project_thread_face_selection(ctx, &scope, &[100], &[], &[])? == FaceSelection::Unresolved));
    assert_eq!(scope, original);
}

#[test]
fn thread_empty_groups_preserve_original_stored_refusal() {
    let scope = long_scope();
    assert_original_refusal(|ctx| Ok(super::super::super::super::project_thread_face_selection(ctx, &scope, &[100], &[], &[])? == FaceSelection::Unresolved));
}

#[test]
fn selected_plane_empty_members_skip_unused_scope_and_all_storage() {
    let scope = long_scope();
    let group = super::super::group(77, 0, 100, vec![], DesignOperandRole::ROLE_0X21);
    let original = (scope.clone(), group.clone());
    assert_no_query(|ctx| Ok(super::super::super::super::selected_work_planes(ctx, &scope, &group, &[], &[])?.is_none()));
    assert_eq!((scope, group), original);
}

#[test]
fn selected_plane_empty_members_preserve_original_stored_refusal() {
    let scope = long_scope();
    let group = super::super::group(77, 0, 100, vec![], DesignOperandRole::ROLE_0X21);
    assert_original_refusal(|ctx| Ok(super::super::super::super::selected_work_planes(ctx, &scope, &group, &[], &[])?.is_none()));
}

#[test]
fn thicken_empty_fallback_groups_skip_unused_scope_and_all_storage() {
    let scope = thicken_scope();
    let original = scope.clone();
    assert_no_query(|ctx| Ok(super::super::super::super::project_thicken(ctx, &scope, &[], &[])?.is_none()));
    assert_eq!(scope, original);
}

#[test]
fn thicken_empty_fallback_groups_preserve_original_stored_refusal() {
    let scope = thicken_scope();
    assert_original_refusal(|ctx| Ok(super::super::super::super::project_thicken(ctx, &scope, &[], &[])?.is_none()));
}

#[test]
fn base_flange_empty_groups_skip_unused_scope_and_all_storage() {
    let scope = base_flange_scope();
    let original = scope.clone();
    assert_no_query(|ctx| Ok(super::super::super::super::project_base_flange(ctx, &scope, &[], &[])?.is_none()));
    assert_eq!(scope, original);
}

#[test]
fn base_flange_empty_groups_preserve_original_stored_refusal() {
    let scope = base_flange_scope();
    assert_original_refusal(|ctx| Ok(super::super::super::super::project_base_flange(ctx, &scope, &[], &[])?.is_none()));
}

#[test]
fn thicken_missing_payload_skips_unused_scope_and_all_storage() {
    let scope = DesignParameterScope::empty(&long_scope().id, DesignFeatureKind::Thicken, 77);
    let original = scope.clone();
    assert_no_query(|ctx| Ok(super::super::super::super::project_thicken(ctx, &scope, &[], &[])?.is_none()));
    assert_eq!(scope, original);
}

#[test]
fn thicken_missing_payload_preserves_original_stored_refusal() {
    let scope = DesignParameterScope::empty(&long_scope().id, DesignFeatureKind::Thicken, 77);
    assert_original_refusal(|ctx| Ok(super::super::super::super::project_thicken(ctx, &scope, &[], &[])?.is_none()));
}

#[test]
fn base_flange_missing_operation_skips_unused_scope_and_all_storage() {
    let scope = DesignParameterScope::empty(&long_scope().id, DesignFeatureKind::BaseFlange, 77);
    let original = scope.clone();
    assert_no_query(|ctx| Ok(super::super::super::super::project_base_flange(ctx, &scope, &[], &[])?.is_none()));
    assert_eq!(scope, original);
}

#[test]
fn base_flange_missing_operation_preserves_original_stored_refusal() {
    let scope = DesignParameterScope::empty(&long_scope().id, DesignFeatureKind::BaseFlange, 77);
    assert_original_refusal(|ctx| Ok(super::super::super::super::project_base_flange(ctx, &scope, &[], &[])?.is_none()));
}
