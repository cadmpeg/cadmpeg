// SPDX-License-Identifier: Apache-2.0
//! Actual first-visit refusal for fallible SubD traversals.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;

use super::super::{ComponentBase, ComponentPointer, RawLevel, RawVertex, SubdError};

#[test]
fn subd_level_validation_refuses_only_first_vertex_visit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let level = RawLevel {
        source_offset: 0,
        vertices: (1..=1024).map(|archive_id| RawVertex {
            base: ComponentBase { source_offset: 0, archive_id },
            point: FinitePoint3::ZERO,
            tag: Some(cadmpeg_ir::subd::SubdVertexTag::Smooth),
            edges: Vec::new(),
            faces: Vec::new(),
        }).collect(),
        edges: Vec::new(),
        faces: Vec::new(),
        _storage: ctx.reserve_scoped(0, "test SubD raw level storage").unwrap(),
    };
    let error = super::super::validate_level(&ctx, &level, 0)
        .expect_err("refuse one actual vertex visit");
    let SubdError::Resource(refusal) = error else {
        panic!("SubD first-visit resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino validate level traversal");
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    drop(level);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn subd_incidence_comparison_refuses_only_first_pointer_visit() {
    let pointers = vec![ComponentPointer { archive_id: 1, direction: false }; 1024];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = super::super::compare_incidence(&ctx, &pointers, None, "test incidence")
        .expect_err("refuse one actual pointer visit");
    let SubdError::Resource(refusal) = error else {
        panic!("SubD incidence first-visit resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino compare incidence traversal");
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}
