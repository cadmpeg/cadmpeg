// SPDX-License-Identifier: Apache-2.0

use super::{reference, stable_bounded_face_operand};
use crate::records::recipes::ConstructionRecipeKind;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn historical_referenced_face_candidates_refuse_limits() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_kind = ConstructionRecipeKind::Face;
    operand.recipe_references = vec![reference(10, "selected", 1)];
    for (retained, items, dimension, operation) in [
        (
            0,
            1,
            ResourceDimension::RetainedBytes,
            "f3d historical face candidate id",
        ),
        (
            100,
            0,
            ResourceDimension::CollectionItems,
            "f3d historical face candidate",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::face_resolve::historical_face_operand_candidates(Some(&ctx), &operand),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
    }
}

#[test]
fn historical_fallback_face_candidates_refuse_limits() {
    let operand = stable_bounded_face_operand();
    for (retained, items, dimension, operation) in [
        (
            0,
            1,
            ResourceDimension::RetainedBytes,
            "f3d historical fallback face id",
        ),
        (
            100,
            0,
            ResourceDimension::CollectionItems,
            "f3d historical fallback face",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::face_resolve::historical_face_operand_candidates(Some(&ctx), &operand),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
    }
}

#[test]
fn nested_bounded_face_candidates_refuse_limits() {
    let mut operand = stable_bounded_face_operand();
    operand.candidate_faces.clear();
    operand.unreferenced_candidate_faces.clear();
    operand.alternate_selector_candidate_faces.clear();
    operand.recipe_references = vec![reference(10, "selected", 1)];
    for (retained, items, dimension, operation) in [
        (
            0,
            1,
            ResourceDimension::RetainedBytes,
            "f3d nested bounded face candidate id",
        ),
        (
            100,
            0,
            ResourceDimension::CollectionItems,
            "f3d nested bounded face candidate",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_collection_items = items;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::face_resolve::nested_bounded_face_history_candidates(Some(&ctx), &operand),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
    }
}
