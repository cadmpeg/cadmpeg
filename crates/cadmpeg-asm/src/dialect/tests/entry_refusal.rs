// SPDX-License-Identifier: Apache-2.0
use crate::dialect::{acis_match, classify, match_header, unverified_message, KernelHeaderRef,
    ACIS_SAVE_FORMAT_217, ACIS_SAVE_FORMAT_218, ACIS_TEXT_ACIS, ACIS_TEXT_ASM, ACIS_UNKNOWN,
    ACIS_ASM_BINARYFILE_4, ACIS_ASM_BINARYFILE_8};
use cadmpeg_core::CodecError;
use cadmpeg_core::dialect::DialectMatch;
use super::header;
use crate::kernel_header::RefWidth;

#[test]
fn verified_acis_match_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for (major, row) in [(217, ACIS_SAVE_FORMAT_217), (218, ACIS_SAVE_FORMAT_218)] {
            let result = acis_match(ctx, Some(major));
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert_eq!(result.expect("verified row is fixed"), DialectMatch::admitted(row)),
            }
        }
    });
}

#[test]
fn fixed_kernel_match_preserves_original_refusal() {
    let empty = header(RefWidth::Four, None);
    let verified = header(RefWidth::Four, Some(21_700));
    let wide = header(RefWidth::Eight, None);
    crate::test_support::with_entry_context(|ctx, original| {
        for (input, expected) in [
            (KernelHeaderRef::Unknown, DialectMatch::refused(ACIS_UNKNOWN)),
            (KernelHeaderRef::Asm(&empty), DialectMatch::admitted(ACIS_ASM_BINARYFILE_4)),
            (KernelHeaderRef::Asm(&wide), DialectMatch::admitted(ACIS_ASM_BINARYFILE_8)),
            (KernelHeaderRef::Acis(&verified), DialectMatch::admitted(ACIS_SAVE_FORMAT_217)),
            (KernelHeaderRef::TextAsm(&empty.metadata), DialectMatch::admitted(ACIS_TEXT_ASM)),
            (KernelHeaderRef::TextAcis(&verified.metadata), DialectMatch::admitted(ACIS_TEXT_ACIS)),
            (KernelHeaderRef::TextAcis(&empty.metadata), DialectMatch::residual(ACIS_TEXT_ACIS)),
        ] {
            let result = match_header(ctx, input);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert_eq!(result.expect("fixed row needs no resources"), expected),
            }
        }
    });
}

#[test]
fn absent_kernel_declarations_preserve_original_refusal() {
    let empty = header(RefWidth::Four, None);
    crate::test_support::with_entry_context(|ctx, original| {
        for (input, expected) in [
            (KernelHeaderRef::Unknown, DialectMatch::refused(ACIS_UNKNOWN)),
            (KernelHeaderRef::TextAsm(&empty.metadata), DialectMatch::admitted(ACIS_TEXT_ASM)),
            (KernelHeaderRef::TextAcis(&empty.metadata), DialectMatch::residual(ACIS_TEXT_ACIS)),
        ] {
            let result = classify(ctx, input);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert_eq!(result.expect("absent declarations need no resources"), expected),
            }
        }
    });
}

#[test]
fn absent_kernel_recovery_message_preserves_original_refusal() {
    let matches = [
        DialectMatch::admitted(cadmpeg_core::dialect_id!("test:text")),
        DialectMatch::admitted(ACIS_SAVE_FORMAT_217),
        DialectMatch::refused(ACIS_UNKNOWN),
    ];
    crate::test_support::with_entry_context(|ctx, original| {
        for matched in &matches {
            let result = unverified_message(ctx, "subject", matched);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(None))),
            }
        }
    });
}
