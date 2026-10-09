// SPDX-License-Identifier: Apache-2.0
use crate::kernel_header::{read_string_region, read_u8_string_span, scan_string_region};
use cadmpeg_core::CodecError;

#[test]
fn kernel_absent_string_span_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for bytes in [b"".as_slice(), &[0], &[7], &[7, 3, b'a']] {
            let result = read_u8_string_span(ctx, bytes, 0);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(None))),
            }
        }
    });
}

#[test]
fn kernel_absent_string_region_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for bytes in [b"".as_slice(), &[0], &[7], &[7, 3, b'a']] {
            let result = read_string_region(ctx, bytes, 0);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => {
                    let region = result.expect("fixed missing fields need no resources");
                    assert_eq!(region.strings, [None, None, None]);
                    assert_eq!(region.doubles, [None, None, None]);
                }
            }
        }
    });
}

#[test]
fn kernel_absent_string_scan_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for bytes in [b"".as_slice(), &[0], &[7], &[7, 3, b'a']] {
            let result = scan_string_region(ctx, bytes, 0);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok((0, 0, 0)))),
            }
        }
    });
}
