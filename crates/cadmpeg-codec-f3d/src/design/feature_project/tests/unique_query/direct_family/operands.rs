// SPDX-License-Identifier: Apache-2.0
//! Actual operand query phases retain native input.

use super::*;

#[test]
fn direct_unique_full_round_group_source_preserves_original_refusal() {
    let scope = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#77", DesignFeatureKind::Fillet, 77);
    let groups = [super::super::group(77, 0, 100, vec![101], DesignOperandRole::BODIES_A)];
    assert_refusal("visit F3D full-round group source", |ctx| super::super::super::super::project_full_round_fillet(ctx, &scope, &groups, &[], &[], &[], &[]));
}

#[test]
fn direct_unique_full_round_face_source_preserves_original_refusal() {
    let scope = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#77", DesignFeatureKind::Fillet, 77);
    let groups = [fixtures::full_round_group()];
    // An empty operand source still executes its actual end probe.
    assert_refusal("visit F3D full-round face operand source", |ctx| super::super::super::super::project_full_round_fillet(ctx, &scope, &groups, &[], &[], &[], &[]));
}

#[test]
fn direct_unique_surface_offset_group_source_preserves_original_refusal() {
    let scope = super::super::scope();
    let groups = [super::super::group(77, 0, 100, vec![101], DesignOperandRole::PROFILE)];
    let operation = crate::records::feature::surface_ops::DesignSurfaceOffsetOperation {
        distance: cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap(),
        distance_offset: 0,
        distance_record_index: 90,
        support: crate::records::feature::surface_ops::DesignSurfaceOffsetSupport::FaceGroups { group_record_indices: vec![100] },
    };
    assert_refusal("visit F3D surface offset group source", |ctx| super::super::super::super::project_surface_offset(ctx, &scope, &operation, &groups, &[]));
}

#[test]
fn direct_unique_draft_historical_operand_source_preserves_original_refusal() {
    let scope = fixtures::history_scope();
    let group = super::super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let selections = [super::super::selection(101, 0, 600)];
    assert_refusal("visit F3D Draft historical operand source", |ctx| super::super::super::super::selected_historical_face_selection(ctx, &scope, &group, &selections, &[]));
}

#[test]
fn direct_unique_split_face_path_operand_source_preserves_original_refusal() {
    let scope = fixtures::history_scope();
    let group = super::super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let selections = [super::super::selection(101, 0, 600)];
    assert_refusal("visit F3D SplitFace path operand source", |ctx| super::super::super::super::resolved_split_face_path(ctx, &scope, &group, &selections, &[]));
}

fn thicken_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#77", DesignFeatureKind::Thicken, 77);
    let DesignScopePayloadMut::Thicken(slot) = scope.payload_mut() else { panic!("Thicken payload"); };
    *slot = Some(crate::records::feature::direct_face::DesignThickenOperation {
        signed_thickness: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
        thickness_record_index: 90,
        thickness_offset: 0,
    });
    scope
}

#[test]
fn direct_unique_thicken_source_preserves_original_refusal() {
    let scope = thicken_scope();
    let groups = [super::super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X5)];
    assert_refusal("visit F3D Thicken group source", |ctx| super::super::super::super::project_thicken(ctx, &scope, &[], &groups));
}

#[test]
fn direct_unique_thicken_preserves_native_identity_without_group_storage() {
    for role in [DesignOperandRole::ROLE_0X5, DesignOperandRole::ROLE_0X12] {
        let scope = thicken_scope();
        let groups = [super::super::group(77, 0, 100, vec![101], role)];
        let original = groups.clone();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let Some(FeatureDefinition::Operation(FeatureOperation::Thicken { faces: FaceSelection::Native(native), thickness, side })) = super::super::super::super::project_thicken(ctx, &scope, &[], &groups).unwrap() else { panic!("native Thicken projection"); };
            assert_eq!(native, groups[0].id);
            assert_eq!(thickness.unwrap().get(), 5.0);
            assert_eq!(side, Some(ThickenSide::Forward));
            assert_eq!(ctx.resource_refusal(), None);
        });
        assert_eq!(groups, original);
    }
}

#[test]
fn direct_unique_variable_fillet_group_source_preserves_original_refusal() {
    let scope = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#77", DesignFeatureKind::Fillet, 77);
    let groups = [super::super::group(77, 0, 100, vec![101], DesignOperandRole::BODIES_A)];
    let inputs = super::super::super::super::ProjectInputs { construction_groups: &groups, ..Default::default() };
    assert_refusal("visit F3D variable Fillet group source", |ctx| super::super::super::super::project_variable_fillet(&scope, &[], &inputs, ctx));
}
