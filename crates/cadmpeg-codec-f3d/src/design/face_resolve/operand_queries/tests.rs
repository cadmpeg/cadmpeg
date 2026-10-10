// SPDX-License-Identifier: Apache-2.0

use super::unique_face_operand;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure, ResourceLimit,
};
use cadmpeg_core::CodecError;

fn operand() -> DesignFaceOperand {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#200", "scope_record_index": 100,
        "scope_reference_ordinal": 0, "group_record_index": 150, "group_member_ordinal": 0,
        "record_index": 200, "byte_offset": 0, "class_tag": "271",
        "paired_byte_offset": 325, "paired_class_tag": "261",
        "recipe_record_index": 203, "recipe_record_byte_offset": 341,
        "recipe_id": "f3d:test:recipe#201", "recipe_prefix_offset": 352,
        "recipe_prefix_bytes": "", "recipe_references": [], "recipe_kind": "bounded_face",
        "recipe_program_offset": 0, "recipe_program": [], "recipe_node_offsets": [],
        "recipe_nodes": [], "candidate_faces": ["f3d:brep:entity#10"],
        "next_record_index": 202, "next_byte_offset": 469
    }))
    .unwrap()
}

fn group() -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:construction-group#150", "scope_record_index": 100,
        "scope_reference_ordinal": 0, "record_index": 150, "byte_offset": 0,
        "class_tag": "338", "role": 0, "members": [200], "member_offsets": [0],
        "frame": {"member_count_offset": 0, "opaque_index": 1, "opaque_index_offset": 18,
            "opaque_scalar": 0.0, "opaque_scalar_offset": 22, "variant": false},
        "role_offset": 0, "paired_class_tag": "261", "paired_byte_offset": 0
    }))
    .unwrap()
}

fn policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn failure(error: CodecError, cap: u64, operation: &'static str) -> ResourceLimit {
    let CodecError::ResourceLimit(actual) = error else {
        panic!("expected original refusal: {error:?}");
    };
    assert_eq!(
        actual,
        ResourceLimit {
            dimension: ResourceDimension::WorkUnits,
            reason: ResourceFailure::BudgetExceeded,
            limit: cap,
            used: cap,
            additional: 1,
            operation
        }
    );
    actual
}

#[test]
fn unique_operand_single_source_is_allowed_at_exact_work_and_refuses_one_below() {
    let operands = [operand()];
    let before = serde_json::to_value(&operands).unwrap();
    // One actual record step, reverse suffix through colon, and equal stream byte pairs.
    let suffix = "face-operand#200".len() + 1;
    let work = u64::try_from(1 + suffix + "f3d:test".len()).unwrap();
    for cap in [work, work - 1] {
        let arena = DecodeArena::new();
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = unique_face_operand(&ctx, &operands, Some("f3d:test"), 100, 200);
        if cap == work {
            assert!(std::ptr::eq(result.unwrap().unwrap(), &operands[0]));
            ctx.finish_session().unwrap();
        } else {
            let original = failure(result.unwrap_err(), cap, "compare F3D face operand stream");
            assert!(
                matches!(unique_face_operand(&ctx, &[], None, 0, 0), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        }
        assert_eq!(serde_json::to_value(&operands).unwrap(), before);
    }
}

#[test]
fn unique_operand_second_match_rejects_without_reading_long_third_record() {
    let first = operand();
    let second = first.clone();
    let mut third = first.clone();
    third.id = "unvisited".repeat(1024);
    let operands = [first, second, third];
    let before = serde_json::to_value(&operands).unwrap();
    let one = 1 + "face-operand#200".len() + 1 + "f3d:test".len();
    let cap = u64::try_from(2 * one).unwrap();
    let arena = DecodeArena::new();
    let policy = policy(cap);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        unique_face_operand(&ctx, &operands, Some("f3d:test"), 100, 200)
            .unwrap()
            .is_none()
    );
    assert_eq!(serde_json::to_value(&operands).unwrap(), before);
    ctx.finish_session().unwrap();
}

#[test]
fn unique_operand_first_stream_mismatch_keeps_equal_length_tail_unvisited() {
    let mut first = operand();
    first.id = format!("aaa{}:x", "tail".repeat(1024));
    let expected = format!("bbb{}", "tail".repeat(1024));
    let operands = [first];
    let before = serde_json::to_value(&operands).unwrap();
    // Record, two reverse bytes, first mismatching stream byte, and the end probe.
    for cap in [5, 4] {
        let arena = DecodeArena::new();
        let policy = policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = unique_face_operand(&ctx, &operands, Some(&expected), 100, 200);
        if cap == 5 {
            assert!(result.unwrap().is_none());
            ctx.finish_session().unwrap();
        } else {
            let original = failure(result.unwrap_err(), cap, "visit F3D face operand");
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        }
        assert_eq!(serde_json::to_value(&operands).unwrap(), before);
    }
}

#[test]
fn unique_operand_missing_stream_keeps_original_none_none_match() {
    let mut first = operand();
    first.id = "plain".into();
    let operands = [first];
    // Record step and five reverse bytes plus the separator search end probe.
    let arena = DecodeArena::new();
    let policy = policy(7);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(std::ptr::eq(
        unique_face_operand(&ctx, &operands, None, 100, 200)
            .unwrap()
            .unwrap(),
        &operands[0]
    ));
    ctx.finish_session().unwrap();
}

#[test]
fn unique_operand_empty_source_is_free_and_propagates_original_refusal() {
    let arena = DecodeArena::new();
    let policy = policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(unique_face_operand(&ctx, &[], Some("unused"), 100, 200)
        .unwrap()
        .is_none());
    let original = failure(
        ctx.charge_work(1, "original operand query refusal")
            .unwrap_err(),
        0,
        "original operand query refusal",
    );
    assert!(
        matches!(unique_face_operand(&ctx, &[], None, 0, 0), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
}

#[test]
fn actual_face_group_refuses_before_operand_visit_with_original_native_source() {
    let operands = [operand()];
    let group = group();
    let before = (
        serde_json::to_value(&operands).unwrap(),
        serde_json::to_value(&group).unwrap(),
    );
    // Group stream suffix through colon, then the actual first member visit.
    let cap = u64::try_from("construction-group#150".len() + 1 + 1).unwrap();
    let arena = DecodeArena::new();
    let policy = policy(cap);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = failure(
        crate::design::face_resolve::resolved_face_group(&ctx, &group, &operands).unwrap_err(),
        cap,
        "visit F3D face operand",
    );
    assert_eq!(
        (
            serde_json::to_value(&operands).unwrap(),
            serde_json::to_value(&group).unwrap()
        ),
        before
    );
    assert!(
        matches!(crate::design::face_resolve::resolved_face_group(&ctx, &group, &[]), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
}

#[test]
fn actual_face_group_empty_source_skips_unused_group_stream() {
    let mut group = group();
    group.id = "unused".repeat(1024);
    let before = serde_json::to_value(&group).unwrap();
    let arena = DecodeArena::new();
    let policy = policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        crate::design::face_resolve::resolved_face_group(&ctx, &group, &[])
            .unwrap()
            .is_none()
    );
    assert_eq!(serde_json::to_value(&group).unwrap(), before);
    ctx.finish_session().unwrap();
}
