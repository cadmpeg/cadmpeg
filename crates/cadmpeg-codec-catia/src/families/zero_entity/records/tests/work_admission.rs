// SPDX-License-Identifier: Apache-2.0
//! Work admission for zero-entity record bindings.

#[test]
fn zero_face_loop_binding_rows_preserve_work_refusal() {
    const OPERATION: &str = "catia_zero_face_loop_binding_rows";
    let stream = crate::test_support::test_zero_entity::zero_entity_face_loop_support_stream();
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::zero_entity_support_runs_in_range(
            ctx,
            &stream,
            0..stream.len(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    };
    let runs = crate::test_support::with_service_context(run).expect("service budget");
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0]
            .face
            .as_ref()
            .expect("face")
            .loops
            .as_ref()
            .expect("loop roster")
            .len(),
        1
    );
    let result = crate::test_support::with_work_refusal(OPERATION, |ctx| {
        let result = run(ctx);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == OPERATION)
    );
}

#[test]
fn zero_support_slot_range_preserves_work_refusal() {
    const OPERATION: &str = "catia_zero_binding_values";
    let mut stream = crate::test_support::test_zero_entity::zero_entity_face_loop_support_stream();
    let support_slot = 0x6a + 12 + 13;
    stream[support_slot..support_slot + 4].copy_from_slice(&1u32.to_le_bytes());
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::zero_entity_support_runs_in_range(
            ctx,
            &stream,
            0..stream.len(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    };
    let runs = crate::test_support::with_service_context(run).expect("service budget");
    let loops = runs[0]
        .face
        .as_ref()
        .expect("face")
        .loops
        .as_ref()
        .expect("loop roster");
    assert_eq!(loops[0].support_record_ordinals, [2]);
    let result = crate::test_support::with_work_refusal(OPERATION, |ctx| {
        let result = run(ctx);
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == OPERATION)
    );
}

#[test]
fn zero_member_ranges_preserve_maximum_bounded_count() {
    let count = std::num::NonZeroUsize::new(cadmpeg_core::decode::index_from_u32(u32::MAX))
        .expect("nonzero bounded count");
    let members = super::super::ZeroEntityLoopMembers::try_new(u32::MAX, 1, count)
        .expect("largest member count fits below terminal");
    let mut ids = members.member_ids();
    assert_eq!(ids.next(), Some(u32::MAX - 1));
    assert_eq!(ids.next_back(), Some(0));
    let mut slots = members.support_slots();
    assert_eq!(slots.next(), Some(1));
    assert_eq!(slots.next_back(), Some(u32::MAX));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn zero_member_ranges_reject_count_above_identifier_extent() {
    let count = std::num::NonZeroUsize::new(
        usize::try_from(u64::from(u32::MAX) + 1).expect("64-bit count"),
    )
    .expect("nonzero count");
    assert!(super::super::ZeroEntityLoopMembers::try_new(u32::MAX, 1, count).is_none());
}
