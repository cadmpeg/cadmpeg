// SPDX-License-Identifier: Apache-2.0
//! A stopped face visitor retains the original refusal and exact face lane.

use crate::design::feature_project::visit_revolve_face_candidates;
use crate::records::dimensions::DesignRecipeReference;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const ORIGINAL: &str = "original F3D Revolve visitor refusal";

fn operand(lane: u8) -> DesignFaceOperand {
    let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#20", "scope_record_index": 12,
        "scope_reference_ordinal": 0, "record_index": 20, "byte_offset": 0,
        "class_tag": "375", "paired_byte_offset": 16, "paired_class_tag": "259",
        "recipe_record_index": 23, "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:recipe#23", "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "", "recipe_references": [], "recipe_kind": "face",
        "recipe_program_offset": 48, "recipe_program": [0, -1],
        "recipe_node_offsets": [], "recipe_nodes": [],
        "candidate_faces": ["f3d:test:face#1", "f3d:test:face#2"],
        "next_record_index": 24, "next_byte_offset": 160
    }))
    .unwrap();
    let faces = operand.candidate_faces.clone();
    if lane != 0 {
        operand.recipe_references.push(DesignRecipeReference {
            selector: 1,
            selector_offset: 0,
            token: "face".into(),
            token_offset: 0,
            design_reference: 1,
            design_reference_offset: 0,
            candidate_faces: if lane == 1 { faces.clone() } else { Vec::new() },
            candidate_edges: Vec::new(),
            alternate_selector_faces: if lane == 2 { faces } else { Vec::new() },
            alternate_selector_edges: Vec::new(),
        });
    }
    operand
}

fn assert_callback_refusal(returned: Option<bool>) {
    for lane in 0..3 {
        let mut operand = operand(lane);
        let first = operand.candidate_faces[0].clone();
        let tail = operand.candidate_faces[1].clone();
        if lane == 0 {
            operand
                .candidate_faces
                .extend((0..4096).map(|_| tail.clone()));
        } else if lane == 1 {
            operand.recipe_references[0]
                .candidate_faces
                .extend((0..4096).map(|_| tail.clone()));
        } else {
            operand.recipe_references[0]
                .alternate_selector_faces
                .extend((0..4096).map(|_| tail.clone()));
        }
        let original_operand = operand.clone();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
            let mut visits = 0;
            let result = visit_revolve_face_candidates(ctx, &operand, |face| {
                visits += 1;
                assert_eq!(face, &first);
                assert!(matches!(
                    ctx.charge_work(1, ORIGINAL),
                    Err(CodecError::ResourceLimit(_))
                ));
                returned.ok_or_else(|| CodecError::malformed("ordinary Revolve visitor error"))
            });
            let original = ctx.resource_refusal().expect("visitor stored its refusal");
            assert_eq!(original.operation, ORIGINAL);
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.additional, 1);
            assert!(matches!(result, Err(CodecError::ResourceLimit(later)) if later == original));
            assert_eq!(visits, 1);
            assert!(matches!(visit_revolve_face_candidates(ctx, &operand, |_| {
                visits += 1;
                Ok(true)
            }), Err(CodecError::ResourceLimit(later)) if later == original));
            assert_eq!(visits, 1);
        });
        assert_eq!(operand, original_operand);
    }
}

#[test]
fn revolve_visitor_false_keeps_original_refusal() {
    assert_callback_refusal(Some(false));
}

#[test]
fn revolve_visitor_true_keeps_original_refusal() {
    assert_callback_refusal(Some(true));
}

#[test]
fn revolve_visitor_ordinary_error_keeps_original_refusal() {
    assert_callback_refusal(None);
}

#[test]
fn revolve_visitor_ordinary_error_preserves_first_face_and_unread_suffix() {
    for lane in 0..3 {
        let operand = operand(lane);
        crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
            let mut visits = 0;
            let result = visit_revolve_face_candidates(ctx, &operand, |face| {
                visits += 1;
                assert_eq!(face, &operand.candidate_faces[0]);
                Err(CodecError::malformed("ordinary Revolve visitor error"))
            });
            assert!(
                matches!(result, Err(CodecError::Malformed(message)) if message == "ordinary Revolve visitor error")
            );
            assert_eq!(visits, 1);
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn revolve_visitor_false_stops_at_actual_face_without_extra_work() {
    for (lane, reached) in [(0, 2), (1, 3), (2, 4)] {
        let operand = operand(lane);
        let mut policy = DecodePolicy::service();
        // Face lane selection includes its reached end probe on the fallback.
        // The alternate lane also reaches the empty primary candidate end.
        policy.limits.max_work_units = reached;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut visits = 0;
            assert!(!visit_revolve_face_candidates(ctx, &operand, |face| {
                visits += 1;
                assert_eq!(face, &operand.candidate_faces[0]);
                Ok(false)
            })
            .unwrap());
            assert_eq!(visits, 1);
            assert_eq!(ctx.resource_refusal(), None);
            let Err(CodecError::ResourceLimit(after)) =
                ctx.charge_work(1, "observe reached Revolve visitor work")
            else {
                panic!("the exact reached traversal used the ceiling");
            };
            assert_eq!((after.used, after.additional), (reached, 1));
        });
    }
}

#[test]
fn revolve_visitor_preserves_candidate_then_alternate_source_order() {
    let mut operand = operand(1);
    operand.recipe_references[0].candidate_faces.truncate(1);
    operand.recipe_references[0]
        .alternate_selector_faces
        .push(operand.candidate_faces[1].clone());
    let original = operand.clone();
    crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
        let mut visited = Vec::new();
        assert!(visit_revolve_face_candidates(ctx, &operand, |face| {
            visited.push(face.as_str());
            Ok(true)
        })
        .unwrap());
        assert_eq!(visited, ["f3d:test:face#1", "f3d:test:face#2"]);
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(operand, original);
}
