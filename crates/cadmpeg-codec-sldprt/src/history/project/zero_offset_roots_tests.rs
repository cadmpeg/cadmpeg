// SPDX-License-Identifier: Apache-2.0
//! Lifetime and refusal controls for zero-distance offset chain workspaces.

use super::zero_offset_roots;
use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;

const OPERATION: &str = "resolve SLDPRT zero offset plane chains";
const PARENTS: [Option<usize>; 2] = [Some(1), None];

fn storage_bounds() -> (u64, u64, u64) {
    // alloc_filled uses exact vector_storage. Core linear_growth gives the
    // path and result Vec<usize> four slots at their first amortized growth.
    let array = u64_from_index(PARENTS.len() * std::mem::size_of::<Option<usize>>());
    let path = u64_from_index(4 * std::mem::size_of::<usize>());
    let output = path;
    (2 * array + path, array, output)
}

#[test]
fn zero_offset_roots_retire_workspace_before_output_and_keep_only_output_live() {
    let (peak, _, output) = storage_bounds();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = peak;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let mut storage = ctx.reserve_scoped(0, OPERATION).unwrap();
        let roots = zero_offset_roots(&ctx, &mut storage, &PARENTS).unwrap();
        assert_eq!(roots, [1, 1]);
        let reuse = ctx
            .reserve_scoped(peak - output, "reuse while zero offset output is live")
            .unwrap();
        drop(reuse);
        drop((roots, storage));
        let full = ctx
            .reserve_scoped(peak, "reuse zero offset output storage")
            .unwrap();
        drop(full);
    }
}

#[test]
fn zero_offset_roots_refuse_at_the_exact_workspace_peak_and_keep_original_refusal() {
    let (peak, array, output) = storage_bounds();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = peak - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, OPERATION).unwrap();
    let error = zero_offset_roots(&ctx, &mut storage, &PARENTS).unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected workspace resource refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, OPERATION);
    assert_eq!(first.used, 2 * array);
    assert_eq!(first.additional, output);
    assert_eq!(ctx.resource_refusal(), Some(first.clone()));
    let CodecError::ResourceLimit(next) = ctx.reserve_scoped(0, "sticky refusal").unwrap_err()
    else {
        panic!("expected original sticky refusal");
    };
    assert_eq!(next, first);
}

fn principal_feature() -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#principal").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::DatumPrincipalPlane {
                    plane: cadmpeg_ir::features::PrincipalPlane::Top,
                },
            ),
        ),
        native_ref: None,
    }
}

fn binding_storage_bounds() -> (u64, u64) {
    // The one position HashMap uses core's first four buckets plus alignment,
    // control and tail bytes. It dies in plane_facts before frame construction.
    let hash = u64_from_index(
        4 * std::mem::size_of::<(&str, usize)>() + std::mem::align_of::<(&str, usize)>().max(16)
            - 1
            + 4
            + 16,
    );
    let facts = u64_from_index(4 * std::mem::size_of::<super::PlaneFacts>());
    let references = u64_from_index(4 * std::mem::size_of::<Option<usize>>());
    let roots = u64_from_index(4 * std::mem::size_of::<usize>());
    let frames = u64_from_index(4 * std::mem::size_of::<Option<super::PlaneFrame>>());
    let before_frames = facts + references + roots;
    let peak = before_frames + frames;
    // One parent: two exact root/index slots and four path slots coexist
    // with the four-slot parent vector, before the parent vector retires.
    let chain = facts
        + references
        + references
        + u64_from_index(2 * std::mem::size_of::<Option<usize>>())
        + roots;
    assert!(facts + references + hash < peak);
    assert!(chain < peak);
    (before_frames, frames)
}

#[test]
fn offset_binding_retires_parent_vector_before_frame_construction() {
    let source = principal_feature();
    let (before_frames, frames) = binding_storage_bounds();
    let peak = before_frames + frames;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = peak;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let mut features = [source.clone()];
        super::bind_offset_plane_references(&ctx, &mut features).unwrap();
        assert_eq!(features, [source.clone()]);
        let full = ctx
            .reserve_scoped(peak, "reuse offset binding workspace")
            .unwrap();
        drop(full);
    }
}

#[test]
fn offset_binding_frame_growth_refuses_at_exact_peak_and_keeps_original_refusal() {
    let mut features = [principal_feature()];
    let (before_frames, frames) = binding_storage_bounds();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = before_frames + frames - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::bind_offset_plane_references(&ctx, &mut features).unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected frame growth refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "bind SLDPRT offset plane references");
    assert_eq!(first.used, before_frames);
    assert_eq!(first.additional, frames);
    assert_eq!(ctx.resource_refusal(), Some(first.clone()));
    let CodecError::ResourceLimit(next) =
        ctx.reserve_scoped(0, "sticky frame refusal").unwrap_err()
    else {
        panic!("expected original frame refusal");
    };
    assert_eq!(next, first);
}
