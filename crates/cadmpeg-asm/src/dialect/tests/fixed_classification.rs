// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::dialect::{match_header, ACIS_ASM_BINARYFILE_4};

#[test]
fn verified_acis_majors_select_admitted_rows_without_allocation() {
    crate::test_support::with_zero_context(|ctx| {
        for (major, row) in [(217, ACIS_SAVE_FORMAT_217), (218, ACIS_SAVE_FORMAT_218)] {
            let result = acis_match(ctx, Some(major));
            assert_eq!(
                result.expect("verified row is fixed"),
                DialectMatch::admitted(row)
            );
        }
    });
}

#[test]
fn fixed_kernel_headers_select_seven_dialect_classes() {
    let empty = header(RefWidth::Four, None);
    let verified = header(RefWidth::Four, Some(21_700));
    let wide = header(RefWidth::Eight, None);
    crate::test_support::with_zero_context(|ctx| {
        for (input, expected) in [
            (
                KernelHeaderRef::Unknown,
                DialectMatch::refused(ACIS_UNKNOWN),
            ),
            (
                KernelHeaderRef::Asm(&empty),
                DialectMatch::admitted(ACIS_ASM_BINARYFILE_4),
            ),
            (
                KernelHeaderRef::Asm(&wide),
                DialectMatch::admitted(ACIS_ASM_BINARYFILE_8),
            ),
            (
                KernelHeaderRef::Acis(&verified),
                DialectMatch::admitted(ACIS_SAVE_FORMAT_217),
            ),
            (
                KernelHeaderRef::TextAsm(&empty.metadata),
                DialectMatch::admitted(ACIS_TEXT_ASM),
            ),
            (
                KernelHeaderRef::TextAcis(&verified.metadata),
                DialectMatch::admitted(ACIS_TEXT_ACIS),
            ),
            (
                KernelHeaderRef::TextAcis(&empty.metadata),
                DialectMatch::residual(ACIS_TEXT_ACIS),
            ),
        ] {
            let result = match_header(ctx, input);
            assert_eq!(result.expect("fixed row needs no resources"), expected);
        }
    });
}

#[test]
fn absent_kernel_declarations_select_three_dialect_classes() {
    let empty = header(RefWidth::Four, None);
    crate::test_support::with_zero_context(|ctx| {
        for (input, expected) in [
            (
                KernelHeaderRef::Unknown,
                DialectMatch::refused(ACIS_UNKNOWN),
            ),
            (
                KernelHeaderRef::TextAsm(&empty.metadata),
                DialectMatch::admitted(ACIS_TEXT_ASM),
            ),
            (
                KernelHeaderRef::TextAcis(&empty.metadata),
                DialectMatch::residual(ACIS_TEXT_ACIS),
            ),
        ] {
            let result = classify(ctx, input);
            assert_eq!(
                result.expect("absent declarations need no resources"),
                expected
            );
        }
    });
}
