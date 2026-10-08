// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::card::{FramingDefect, FramingRecoveries, FramingValue, Section};

#[test]
fn framing_values_preserve_each_description_without_owned_text() {
    for (value, expected) in [
        (FramingValue::Literal("no owning Directory Entry"), "no owning Directory Entry"),
        (FramingValue::Number(7), "7"),
        (FramingValue::Sequence(Some(7)), "7"),
        (FramingValue::Sequence(None), "no valid sequence"),
        (FramingValue::PhysicalLine(160), "one physical line of 160 bytes"),
        (FramingValue::FixedCards(2), "2 80-column cards"),
        (FramingValue::BackPointer(Some(3)), "back-pointer 3"),
        (FramingValue::BackPointer(None), "no readable back-pointer"),
        (FramingValue::DeclaredRange(3), "the declared range of D3"),
        (FramingValue::UnusableRange { sequence: 3, start: -1, count: 0 },
            "an unusable declared range for D3 (start -1, count 0)"),
        (FramingValue::CensusRun(2), "the back-pointer census run of 2 card(s)"),
        (FramingValue::TerminateField { section: Section::Start, field: *b"S\xff  12  " },
            "start count S�  12"),
        (FramingValue::TerminateCensus { section: Section::Global, count: 2 },
            "global count 2"),
    ] {
        assert_eq!(value.to_string(), expected);
    }
    // Every invalid byte in the fixed field expands to exactly one replacement.
    assert_eq!(FramingValue::TerminateField {
        section: Section::Parameter, field: [0xff; 8],
    }.to_string(), "parameter-data count ��������");
}

#[test]
fn all_framing_slots_and_merges_need_no_dynamic_storage_or_work() {
    let sections = [Section::Start, Section::Global, Section::Directory, Section::Parameter, Section::Terminate];
    let defects = [FramingDefect::CardBoundary, FramingDefect::Sequence, FramingDefect::ParameterOwner,
        FramingDefect::UnclaimedParameterCard, FramingDefect::TerminateCount];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut held = FramingRecoveries::default();
    let mut earlier = FramingRecoveries::default();
    let mut equal = FramingRecoveries::default();
    for (section_index, section) in sections.into_iter().enumerate().rev() {
        for (defect_index, defect) in defects.into_iter().enumerate().rev() {
            let index = section_index * 5 + defect_index;
            held.record(&ctx, (section, defect), 500 + index, 1600 + index as u64,
                FramingValue::Number(index as u32), FramingValue::Number(1)).unwrap();
            earlier.record(&ctx, (section, defect), 400 + index, 800 + index as u64,
                FramingValue::Number(100 + index as u32), FramingValue::Number(2)).unwrap();
            equal.record(&ctx, (section, defect), 400 + index, 2400 + index as u64,
                FramingValue::Number(999), FramingValue::Number(3)).unwrap();
        }
    }
    held.merge(earlier, &ctx).unwrap();
    held.merge(equal, &ctx).unwrap();
    held.merge(FramingRecoveries::default(), &ctx).unwrap();
    ctx.finish_session().unwrap();

    crate::test_support::with_service_context(&[], |ctx| {
        let held_notes = held.notes(ctx).unwrap();
        let notes = &held_notes.0;
        assert_eq!(notes.len(), 25);
        let tags = ["start:framing", "global:framing", "directory-entry:framing",
            "parameter-data:framing", "terminate:framing"];
        let descriptions = ["a card boundary", "a card sequence", "a Parameter Data card owner",
            "an unclaimed Parameter Data card", "a declared Terminate count"];
        for (index, note) in notes.iter().enumerate() {
            let provenance = note.provenance.as_ref().unwrap();
            assert_eq!(provenance.tag.as_deref(), Some(tags[index / 5]));
            assert_eq!(provenance.offset, 800 + index as u64);
            assert!(note.message.contains(descriptions[index % 5]));
            assert!(note.message.contains(&format!("position {} in the section", 400 + index)));
            assert!(note.message.contains(&format!("which declared {}, and the decoder used 2; 3", 100 + index)));
        }
    });
}

#[test]
fn fixed_framing_operations_preserve_an_existing_refusal_without_mutation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(first) = ctx.charge_work(1, "test prior framing refusal").unwrap_err() else {
        panic!("expected work refusal");
    };
    let mut recoveries = FramingRecoveries::default();
    let before = recoveries.clone();
    assert!(matches!(recoveries.record(&ctx, (Section::Start, FramingDefect::Sequence),
        1, 0, FramingValue::Literal("bad"), FramingValue::Literal("1")),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
    assert_eq!(recoveries, before);
    assert!(matches!(recoveries.merge(FramingRecoveries::default(), &ctx),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
    assert_eq!(recoveries, before);
    assert!(matches!(recoveries.notes(&ctx),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
}

#[test]
fn framing_count_overflow_fuses_its_original_local_refusal() {
    for merge in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut recoveries = FramingRecoveries::default();
        recoveries.record(&ctx, (Section::Start, FramingDefect::Sequence),
            1, 0, FramingValue::Number(1), FramingValue::Number(2)).unwrap();
        recoveries.0[0][1].as_mut().unwrap().count = usize::MAX;
        let before = recoveries.clone();
        let result = if merge {
            let mut incoming = FramingRecoveries::default();
            incoming.record(&ctx, (Section::Start, FramingDefect::Sequence),
                1, 0, FramingValue::Number(1), FramingValue::Number(2)).unwrap();
            recoveries.merge(incoming, &ctx)
        } else {
            recoveries.record(&ctx, (Section::Start, FramingDefect::Sequence),
                1, 0, FramingValue::Number(1), FramingValue::Number(2))
        };
        let Err(CodecError::ResourceLimit(first)) = result else {
            panic!("expected count overflow refusal");
        };
        assert_eq!(first.dimension, ResourceDimension::Codec("iges framing recovery count"));
        assert_eq!(first.operation, "iges framing recovery count");
        assert_eq!(recoveries, before);
        assert_eq!(ctx.resource_refusal(), Some(first.clone()));
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == first
        ));
    }
}
