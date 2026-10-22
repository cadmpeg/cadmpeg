// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};

use super::*;

fn context(arena: &DecodeArena) -> DecodeContext<'_> {
    DecodeContext::new(arena, &DecodePolicy::service(), DecodeDepth::Full)
}

#[test]
fn actual_nested_captures_leave_only_unconsumed_nonframing_bytes() {
    let bytes = [1, 2, 3, 4, 5, 6, 7, 8];
    let arena = DecodeArena::new();
    let (ctx, view) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let view = ctx.name_stream(view, "file").unwrap();
    ctx.record("outer", view, || {
        let mut cursor = view;
        assert_eq!(cursor.u16_le(), Some(513));
        ctx.record("inner", view.child(4, 8).unwrap(), || {
            let mut cursor = view.child(4, 8).unwrap();
            assert_eq!(cursor.u16_le(), Some(1541));
            Ok(())
        })
    })
    .unwrap();
    let layouts = ctx
        .take_record_captures()
        .unwrap()
        .into_iter()
        .map(RecordLayout::from_capture)
        .collect();
    let result = partition_stream(&ctx, "file", 8, &layouts, &[2..3], &[7..8], &[]).unwrap();
    assert_eq!(result.reads(), &[0..2, 4..6]);
    assert_eq!(result.exempt(), &[2..3, 7..8]);
    assert_eq!(result.b1(), &[3..4, 6..7]);
    assert_eq!(result.byte_len(), 8);
}

#[test]
fn given_up_content_does_not_turn_framing_into_b1() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let result =
        partition_stream(&ctx, "file", 12, &BTreeMap::new(), &[0..12], &[], &[4..8]).unwrap();
    assert_eq!(result.exempt(), &[0..12]);
    assert!(result.b1().is_empty());
}

#[test]
fn insignificant_bytes_inside_a_given_up_part_are_b1_and_its_framing_is_not() {
    // #214 §3.1: every given-up part is B1; #215: framing bytes are never B1.
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let result = partition_stream(
        &ctx,
        "file",
        12,
        &BTreeMap::new(),
        &[0..2, 5..6],
        &[2..4, 8..10],
        &[3..9],
    )
    .unwrap();
    assert_eq!(result.exempt(), &[0..3, 5..6, 9..10]);
    assert_eq!(result.b1(), &[3..5, 6..9, 10..12]);
}

#[test]
fn unconsumed_stream_length_does_not_allocate_per_byte() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let result = partition_stream(&ctx, "huge", u64::MAX, &BTreeMap::new(), &[], &[], &[]).unwrap();
    assert_eq!(result.b1(), &[0..u64::MAX]);
    assert!(result.reads().is_empty());
    assert!(result.exempt().is_empty());
    let empty = partition_stream(&ctx, "empty", 0, &BTreeMap::new(), &[], &[], &[]).unwrap();
    assert!(empty.b1().is_empty());
}

#[test]
fn adjacent_and_nested_exclusions_coalesce_independent_of_input_order() {
    let arena = DecodeArena::new();
    let ctx = context(&arena);
    let result = partition_stream(
        &ctx,
        "file",
        16,
        &BTreeMap::new(),
        &[8..10, 3..5, 1..3, 2..4],
        &[10..12, 8..9],
        &[],
    )
    .unwrap();
    assert_eq!(result.exempt(), &[1..5, 8..12]);
    assert_eq!(result.b1(), &[0..1, 5..8, 12..16]);
}

#[test]
fn invalid_masks_and_record_extents_are_rejected() {
    let arena = DecodeArena::new();
    for invalid in [3..3, 5..2, 0..9] {
        let ctx = context(&arena);
        assert!(partition_stream(&ctx, "file", 8, &BTreeMap::new(), &[invalid], &[], &[]).is_err());
    }
    let layout: RecordLayout = serde_json::from_value(serde_json::json!({
        "stream":"file", "offset":7, "byte_len":2,
        "reads":{}, "pointers":{}, "payloads":{}
    }))
    .unwrap();
    let layouts = BTreeMap::from([("record".into(), layout)]);
    assert!(partition_stream(&context(&arena), "file", 8, &layouts, &[], &[], &[]).is_err());
}

#[test]
fn read_bounds_are_checked_in_the_stream_that_owns_each_read() {
    let arena = DecodeArena::new();
    let layout: RecordLayout = serde_json::from_value(serde_json::json!({
        "stream":"expanded", "offset":0, "byte_len":12,
        "reads":{"file":[{"start":3,"end":9}]}, "pointers":{}, "payloads":{}
    }))
    .unwrap();
    let layouts = BTreeMap::from([("record".into(), layout)]);
    assert!(partition_stream(&context(&arena), "file", 8, &layouts, &[], &[], &[]).is_err());
    let expanded =
        partition_stream(&context(&arena), "expanded", 12, &layouts, &[], &[], &[]).unwrap();
    assert_eq!(expanded.b1(), &[0..12]);
}

#[test]
fn interval_difference_agrees_with_independent_small_byte_sets() {
    let arena = DecodeArena::new();
    let layout: RecordLayout = serde_json::from_value(serde_json::json!({
        "stream":"file", "offset":0, "byte_len":8,
        "reads":{"file":[{"start":0,"end":8}]}, "pointers":{}, "payloads":{}
    }))
    .unwrap();
    let layouts = BTreeMap::from([("record".into(), layout)]);
    for start in 0..8 {
        for end in start + 1..=8 {
            let ctx = context(&arena);
            let result =
                partition_stream(&ctx, "file", 8, &layouts, &[1..4, 6..8], &[], &[start..end])
                    .unwrap();
            for byte in 0..8 {
                let read = result.reads().iter().any(|span| span.contains(&byte));
                let exempt = result.exempt().iter().any(|span| span.contains(&byte));
                let b1 = result.b1().iter().any(|span| span.contains(&byte));
                assert_eq!(usize::from(read) + usize::from(exempt) + usize::from(b1), 1);
                let expected_b1 = (start..end).contains(&byte)
                    && !((1..4).contains(&byte) || (6..8).contains(&byte));
                assert_eq!(b1, expected_b1, "byte {byte}, given-up {start}..{end}");
            }
        }
    }
}

#[test]
fn interval_storage_obeys_the_callers_retained_byte_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let error = partition_stream(&ctx, "file", 8, &BTreeMap::new(), &[1..2], &[], &[]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(_)));
}
