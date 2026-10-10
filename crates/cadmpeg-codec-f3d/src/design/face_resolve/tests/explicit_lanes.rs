// SPDX-License-Identifier: Apache-2.0
use super::{face, reference, stable_bounded_face_operand};
use crate::design::face_resolve::explicit_bounded_face_candidates;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn operand() -> DesignFaceOperand {
    let mut operand = stable_bounded_face_operand();
    operand.historical_support_contexts.clear();
    operand.candidate_faces.clear();
    operand.unreferenced_candidate_faces.clear();
    operand.alternate_selector_candidate_faces.clear();
    operand.recipe_references = vec![
        reference(12, "last", 9),
        reference(10, "first", 9),
        reference(12, "duplicate", 9),
        reference(11, "middle", 9),
        reference(13, "smaller", 2),
    ];
    operand
}

#[test]
fn largest_explicit_lane_deduplicates_and_orders_faces() {
    let operand = operand();
    let selected = crate::test_support::with_decode_context(|ctx| {
        explicit_bounded_face_candidates(ctx, &operand)
    })
    .unwrap();
    assert_eq!(selected, Some(vec![face(10), face(11), face(12)]));
}

#[test]
fn equally_large_explicit_lanes_are_ambiguous() {
    let mut operand = operand();
    operand
        .recipe_references
        .extend([reference(14, "second", 2), reference(15, "third", 2)]);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            explicit_bounded_face_candidates(ctx, &operand)
        })
        .unwrap(),
        None
    );
}

#[test]
fn alternate_reference_excludes_its_whole_explicit_lane() {
    let mut operand = operand();
    operand.recipe_references[0]
        .alternate_selector_faces
        .push(face(20));
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            explicit_bounded_face_candidates(ctx, &operand)
        })
        .unwrap(),
        Some(vec![face(13)])
    );
}

#[test]
fn explicit_lane_selection_refuses_before_traversal() {
    let operand = operand();
    let operation = "f3d explicit bounded lane selection";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| explicit_bounded_face_candidates(ctx, &operand),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}
