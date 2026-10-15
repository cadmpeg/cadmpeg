// SPDX-License-Identifier: Apache-2.0

use super::super::{frame_sections, physical_lines, FramingRecoveries, LineEnding,
    PhysicalLine, Section, UnframedLine, CARD_WIDTH};
use crate::test_support::test_cards::card_with_ending;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fused_cards() -> Vec<u8> {
    let mut bytes = card_with_ending(b"first", b'S', 1, b"");
    bytes.extend(card_with_ending(b"second", b'S', 2, b""));
    bytes.extend(card_with_ending(b"third", b'G', 1, b""));
    bytes
}

fn fused_prefix_work(bytes: &[u8]) -> u64 {
    // Current framing prepays the byte search, then visits one physical line.
    // The core marker predicate visits three cards and its current end probe.
    u64::try_from(bytes.len()).unwrap() + 1 + 3 + 1
}

#[test]
fn physical_card_source_refuses_one_visit_after_marker_validation() {
    let bytes = fused_cards();
    let work = fused_prefix_work(&bytes);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = physical_lines(&bytes, &ctx) else {
        panic!("expected first physical card source refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges physical cards");
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    for replay in [bytes.as_slice(), &[]] {
        assert!(matches!(physical_lines(replay, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn physical_card_allocation_refuses_after_one_visit_without_admitting_the_tail() {
    let bytes = fused_cards();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = fused_prefix_work(&bytes) + 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = physical_lines(&bytes, &ctx) else {
        panic!("expected first physical card allocation refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges_cards");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [bytes.as_slice(), &[]] {
        assert!(matches!(physical_lines(replay, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn physical_card_source_preserves_fused_payloads_offsets_and_sequences() {
    let bytes = fused_cards();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = fused_prefix_work(&bytes) + 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let lines = physical_lines(&bytes, &ctx).unwrap();
    assert_eq!(lines.len(), 3);
    for (index, (line, (section, sequence))) in lines.iter().zip([
        (Section::Start, 1), (Section::Start, 2), (Section::Global, 1),
    ]).enumerate() {
        assert_eq!(line.line.offset, u64::try_from(index * CARD_WIDTH).unwrap());
        assert_eq!(line.line.payload, &bytes[index * CARD_WIDTH..(index + 1) * CARD_WIDTH]);
        assert_eq!(line.line.ending, LineEnding::None);
        assert_eq!(line.section, Some(section));
        assert_eq!(line.sequence, Some(sequence));
        assert_eq!(line.fused_cards, (index == 0).then_some(3));
    }
    drop(lines);
    ctx.finish_session().unwrap();
}

#[test]
fn empty_physical_input_executes_no_source_visit_or_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(physical_lines(&[], &ctx).unwrap().is_empty());
    ctx.finish_session().unwrap();
}

fn raw_lines() -> Vec<UnframedLine<'static>> {
    [Section::Start, Section::Global, Section::Terminate]
        .into_iter().enumerate().map(|(index, section)| UnframedLine {
            line: PhysicalLine {
                offset: u64::try_from(index * CARD_WIDTH).unwrap(),
                payload: b"source payload", ending: LineEnding::None,
            },
            section: Some(section), sequence: Some(1), fused_cards: None,
        }).collect()
}

#[test]
fn framed_card_source_refuses_one_visit_before_section_recovery() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut recoveries = FramingRecoveries::default();
    let Err(CodecError::ResourceLimit(first)) = frame_sections(raw_lines(), &mut recoveries, &ctx) else {
        panic!("expected first framed card source refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges framed card traversal");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [raw_lines(), Vec::new()] {
        assert!(matches!(frame_sections(replay, &mut recoveries, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn framed_card_source_stops_at_the_first_malformed_line_without_admitting_the_tail() {
    let mut lines = raw_lines();
    lines[0].section = None;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::Malformed(message)) = frame_sections(
        lines, &mut FramingRecoveries::default(), &ctx,
    ) else {
        panic!("expected the first unsequenced physical line to stop framing");
    };
    assert_eq!(message, "IGES physical line at offset 0 is unsequenced before Terminate");
    ctx.finish_session().unwrap();
}

#[test]
fn framed_card_source_accepts_exact_visits_and_preserves_section_runs() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (cards, trailing, runs) = frame_sections(
        raw_lines(), &mut FramingRecoveries::default(), &ctx,
    ).unwrap();
    assert_eq!(cards.len(), 3);
    assert!(trailing.is_empty());
    assert_eq!(runs, [0..1, 1..2, 0..0, 0..0, 2..3]);
    for (index, (card, section)) in cards.iter().zip([
        Section::Start, Section::Global, Section::Terminate,
    ]).enumerate() {
        assert_eq!(card.section, section);
        assert_eq!(card.sequence, 1);
        assert_eq!(card.line.offset, u64::try_from(index * CARD_WIDTH).unwrap());
        assert_eq!(card.line.payload, b"source payload");
        assert_eq!(card.line.ending, LineEnding::None);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn empty_framed_input_executes_no_source_visit_before_its_grammar_rejection() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::Malformed(message)) = frame_sections(
        Vec::new(), &mut FramingRecoveries::default(), &ctx,
    ) else {
        panic!("expected empty framing grammar rejection");
    };
    assert_eq!(message, "IGES Fixed ASCII requires Start through Terminate sections");
    ctx.finish_session().unwrap();
}
