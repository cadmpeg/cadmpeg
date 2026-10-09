// SPDX-License-Identifier: Apache-2.0

use super::super::{
    detect_card_stride, detect_fixed_ascii, fused_card_count, scan_with_context, terminate_counts,
    Card, FramingRecoveries, LineEnding, PhysicalLine, Section, SectionRuns,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;

#[test]
fn short_card_stride_preserves_original_refusal_before_fixed_absence() {
    crate::test_support::with_entry_context(|ctx, original| {
        for prefix in [&[][..], &[b' '; 80][..], &[b' '; 160][..]] {
            let result = detect_card_stride(prefix, ctx);
            match original {
                Some(first) => assert!(matches!(result,
                    Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(Confidence::No))),
            }
        }
    });
}

#[test]
fn invalid_fixed_ascii_prefix_preserves_original_refusal_before_fixed_absence() {
    crate::test_support::with_entry_context(|ctx, original| {
        for prefix in [&[][..], &b"x\n"[..], &[b' '; 80][..]] {
            let result = detect_fixed_ascii(prefix, ctx);
            match original {
                Some(first) => assert!(matches!(result,
                    Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(Confidence::No))),
            }
        }
    });
}

#[test]
fn unaligned_fused_card_preserves_original_refusal_before_fixed_absence() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = fused_card_count(b"not-a-card", ctx);
        match original {
            Some(first) => assert!(matches!(result,
                Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(None))),
        }
    });
}

#[test]
fn absent_or_short_terminate_preserves_original_refusal_without_recovery_changes() {
    let short = Card {
        section: Section::Terminate,
        sequence: 1,
        line: PhysicalLine { offset: 0, payload: b"short", ending: LineEnding::None },
    };
    crate::test_support::with_entry_context(|ctx, original| {
        for cards in [&[][..], &[short][..]] {
            let mut runs: SectionRuns = std::array::from_fn(|_| 0..0);
            runs[Section::Terminate.index()] = 0..cards.len();
            let mut recoveries = FramingRecoveries::default();
            let result = terminate_counts(cards, &runs, &mut recoveries, ctx);
            match original {
                Some(first) => assert!(matches!(result,
                    Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(result.is_ok()),
            }
            assert_eq!(recoveries, FramingRecoveries::default());
        }
    });
}

#[test]
fn empty_scan_preserves_original_refusal_before_wrong_format_result() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = scan_with_context(&[], ctx);
        match original {
            Some(first) => assert!(matches!(result,
                Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Err(CodecError::WrongFormat(_)))),
        }
    });
}
