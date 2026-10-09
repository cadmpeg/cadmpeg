// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::SurfaceContinuity;

use crate::design::feature_project::{project_surface_patch, surface_patch_boundary_continuities};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut};
use crate::records::feature::surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary};
use crate::records::topology::construction::DesignConstructionOperandGroup;

fn continuity_scope(codes: &[DesignPatchContinuity]) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:test:scope#1", DesignFeatureKind::SurfacePatch, 1);
    let DesignScopePayloadMut::SurfacePatch(boundaries) = scope.payload_mut() else {
        panic!("the fixture must own SurfacePatch boundaries");
    };
    *boundaries = codes.iter().enumerate().map(|(ordinal, &continuity)| DesignSurfacePatchBoundary {
        scope_reference_ordinal: u32::try_from(ordinal * 3 + 3).unwrap(),
        record_index: u32::try_from(ordinal * 10 + 102).unwrap(),
        is_seed_selection: ordinal == 1,
        continuity,
        flip: 2,
        scale: crate::test_support::real(-1.0),
        model_reference: u32::try_from(ordinal * 10 + 100).unwrap(),
    }).collect();
    scope
}

fn continuity_backing() -> usize {
    let width = std::mem::size_of::<SurfaceContinuity>();
    let minimum = match width { 1 => 8, 2..=1024 => 4, _ => 1 };
    minimum.max(3) * width
}

#[test]
fn unknown_patch_continuity_releases_actual_prefix_backing() {
    for rejected in 0..3 {
        let mut codes = [DesignPatchContinuity::Connected; 3];
        codes[rejected] = DesignPatchContinuity::Unknown(9);
        let scope = continuity_scope(&codes);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = u64_from_index(continuity_backing());
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(surface_patch_boundary_continuities(&ctx, &scope).unwrap().is_empty());
        assert!(surface_patch_boundary_continuities(&ctx, &scope).unwrap().is_empty());
        let storage;
        let (held, held_storage) = ctx.scoped_vector_storage::<u8>(continuity_backing(), "after unknown patch continuity").unwrap();
        storage = held_storage;
        assert_eq!(held.capacity(), continuity_backing());
        let retained = ctx.copy_retained_text("a", "after unknown patch continuity").unwrap();
        assert_eq!(retained, "a");
        drop(held);
        drop(storage);
    }
}

#[test]
fn patch_continuity_visits_only_the_reached_prefix() {
    for rejected in 0..3 {
        let mut codes = [DesignPatchContinuity::Connected; 1024];
        codes[rejected] = DesignPatchContinuity::Unknown(9);
        let scope = continuity_scope(&codes);
        let visits = u64_from_index(rejected + 1);
        for ceiling in [visits, visits - 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_work_units = ceiling;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = surface_patch_boundary_continuities(&ctx, &scope);
            if ceiling == visits {
                assert!(result.unwrap().is_empty());
                let CodecError::ResourceLimit(limit) = ctx.copy_retained_text("a", "after reached continuity prefix").unwrap_err() else {
                    panic!("each reached boundary must consume one visit");
                };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, "after reached continuity prefix");
                assert_eq!(limit.used, visits);
                assert_eq!(limit.additional, 1);
                assert_eq!(limit.limit, ceiling);
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("the next reached boundary must refuse before its visit");
                };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, "scan F3D surface-patch continuity");
                assert_eq!(limit.used, ceiling);
                assert_eq!(limit.additional, 1);
                assert_eq!(limit.limit, ceiling);
                let CodecError::ResourceLimit(later) = ctx.charge_work(0, "after continuity visit refusal").unwrap_err() else {
                    panic!("the original refusal must remain fused");
                };
                assert_eq!(later, limit);
            }
        }
    }
    for codes in [&[][..], &[DesignPatchContinuity::Connected, DesignPatchContinuity::Tangent, DesignPatchContinuity::Curvature][..]] {
        let scope = continuity_scope(codes);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = u64_from_index(codes.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let output = surface_patch_boundary_continuities(&ctx, &scope).unwrap();
        assert_eq!(output.len(), codes.len());
    }
}

#[test]
fn patch_continuity_transfers_only_actual_surviving_conditions() {
    let scope = continuity_scope(&[DesignPatchContinuity::Connected, DesignPatchContinuity::Tangent, DesignPatchContinuity::Curvature]);
    let backing = u64_from_index(continuity_backing());
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::RetainedBytes] {
        for ceiling in [backing, backing - 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = ceiling,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = ceiling,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = surface_patch_boundary_continuities(&ctx, &scope);
            if ceiling == backing {
                let output = result.unwrap();
                assert_eq!(output, [SurfaceContinuity::Contact, SurfaceContinuity::Tangent, SurfaceContinuity::Curvature]);
                assert_eq!(output.capacity() * std::mem::size_of::<SurfaceContinuity>(), continuity_backing());
                if dimension == ResourceDimension::MaterializedBytes {
                    let storage;
                    let (held, held_storage) = ctx.scoped_vector_storage::<u8>(continuity_backing(), "after patch continuity transfer").unwrap();
                    storage = held_storage;
                    assert_eq!(held.capacity(), continuity_backing());
                    assert_eq!(output[2], SurfaceContinuity::Curvature);
                    drop(held);
                    drop(storage);
                } else {
                    let CodecError::ResourceLimit(limit) = ctx.copy_retained_text("a", "after patch continuity transfer").unwrap_err() else {
                        panic!("the surviving conditions must retain their backing");
                    };
                    assert_eq!(limit.dimension, dimension);
                    assert_eq!(limit.operation, "after patch continuity transfer");
                    assert_eq!(limit.used, backing);
                    assert_eq!(limit.additional, 1);
                    assert_eq!(limit.limit, ceiling);
                    assert_eq!(output[2], SurfaceContinuity::Curvature);
                }
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("the actual continuity backing must refuse");
                };
                assert_eq!(limit.dimension, dimension);
                assert_eq!(limit.operation, "f3d surface-patch continuity");
                assert_eq!(limit.used, 0);
                assert_eq!(limit.additional, backing);
                assert_eq!(limit.limit, ceiling);
                let CodecError::ResourceLimit(later) = ctx.charge_work(0, "after continuity storage refusal").unwrap_err() else {
                    panic!("the original storage refusal must remain fused");
                };
                assert_eq!(later, limit);
            }
        }
    }
}

fn occupancy_fixture(rejected: usize) -> (DesignParameterScope, [DesignConstructionOperandGroup; 3]) {
    use crate::records::topology::construction::{DesignConstructionOperandGroupDraft, DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft, DesignConstructionOperandRole};
    use crate::records::topology::extrude_selection::DesignOperandRole;
    let mut scope = continuity_scope(&[DesignPatchContinuity::Connected; 3]);
    scope.try_edit(|draft| {
        draft.frame_length = 442;
        draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![900, 100, 101, 102, 110, 111, 112, 120, 121, 122]);
        draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
        draft.layout_fixture_references();
        draft.layout_fixture_tail();
    }).unwrap();
    let DesignScopePayloadMut::SurfacePatch(boundaries) = scope.payload_mut() else { panic!("SurfacePatch fixture"); };
    boundaries[rejected].record_index = 999;
    let group = |record_index, ordinal, member| DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:test:construction-group#{record_index}"),
        scope_record_index: 1,
        scope_reference_ordinal: ordinal,
        record_index,
        byte_offset: 0,
        class_tag: "277".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located { value: member, offset: 0 }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(DesignConstructionOperandGroupFrameDraft {
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
        }).unwrap(),
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A),
        role_offset: 0,
        paired_class_tag: "260".to_owned().try_into().unwrap(),
        paired_byte_offset: 0,
    }).unwrap();
    (scope, [group(100, 1, 101), group(110, 4, 111), group(120, 7, 121)])
}

#[test]
fn rejected_patch_occupancy_releases_actual_scratch_without_retention() {
    // Three group references use the existing four-slot amortized capacity;
    // occupancy uses exact ten-byte storage from alloc_filled.
    let group_backing = 4 * std::mem::size_of::<&DesignConstructionOperandGroup>();
    let peak = group_backing + 10 * std::mem::size_of::<bool>();
    for rejected in 0..3 {
        let (scope, groups) = occupancy_fixture(rejected);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = u64_from_index(peak);
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(project_surface_patch(&ctx, &scope, &groups, &[], &[]).unwrap().is_none());
        assert!(project_surface_patch(&ctx, &scope, &groups, &[], &[]).unwrap().is_none());
        let storage;
        let (held, held_storage) = ctx.scoped_vector_storage::<u8>(peak, "after rejected patch occupancy").unwrap();
        storage = held_storage;
        assert_eq!(held.capacity(), peak);
        let retained = ctx.copy_retained_text("a", "after rejected patch occupancy").unwrap();
        assert_eq!(retained, "a");
        drop(held);
        drop(storage);
    }
}

#[test]
fn patch_occupancy_preserves_exact_peak_and_original_collection_refusal() {
    let (scope, groups) = occupancy_fixture(2);
    let group_backing = 4 * std::mem::size_of::<&DesignConstructionOperandGroup>();
    let occupancy_backing = 10 * std::mem::size_of::<bool>();
    let peak = u64_from_index(group_backing + occupancy_backing);
    for ceiling in [peak, peak - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = ceiling;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_surface_patch(&ctx, &scope, &groups, &[], &[]);
        if ceiling == peak {
            assert!(result.unwrap().is_none());
        } else {
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("the simultaneous backing must refuse"); };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.operation, "f3d surface-patch reference occupancy");
            assert_eq!(limit.used, u64_from_index(group_backing));
            assert_eq!(limit.additional, u64_from_index(occupancy_backing));
            assert_eq!(limit.limit, ceiling);
            let CodecError::ResourceLimit(later) = ctx.charge_work(0, "after occupancy backing refusal").unwrap_err() else { panic!("the original refusal must remain fused"); };
            assert_eq!(later, limit);
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = project_surface_patch(&ctx, &scope, &groups, &[], &[]).unwrap_err() else { panic!("the original occupancy collection refusal must escape"); };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "f3d surface-patch reference occupancy");
    assert_eq!(limit.used, 3);
    assert_eq!(limit.additional, 10);
    assert_eq!(limit.limit, 3);
    let CodecError::ResourceLimit(later) = ctx.charge_work(0, "after occupancy item refusal").unwrap_err() else { panic!("the original refusal must remain fused"); };
    assert_eq!(later, limit);
}

#[test]
fn patch_candidates_keep_the_original_session_refusal() {
    for codes in [&[][..], &[DesignPatchContinuity::Unknown(9)][..]] {
        let scope = continuity_scope(codes);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(original) = ctx.copy_retained_text("first", "original patch refusal").unwrap_err() else {
            panic!("the original text copy must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "original patch refusal");
        assert_eq!(original.used, 0);
        assert_eq!(original.additional, 5);
        assert_eq!(original.limit, 0);
        let Err(CodecError::ResourceLimit(later)) = surface_patch_boundary_continuities(&ctx, &scope) else {
            panic!("the original refusal must precede any candidate visit");
        };
        assert_eq!(later, original);
        let (scope, groups) = occupancy_fixture(0);
        let Err(CodecError::ResourceLimit(later)) = project_surface_patch(&ctx, &scope, &groups, &[], &[]) else {
            panic!("the original refusal must precede occupancy construction");
        };
        assert_eq!(later, original);
    }
}
