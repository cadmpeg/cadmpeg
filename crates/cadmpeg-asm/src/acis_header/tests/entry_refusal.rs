// SPDX-License-Identifier: Apache-2.0
use crate::acis_header::{parse, record_stream_start, record_stream_start_with_header,
    solved_record_limit, solved_record_limit_with_header};
use crate::kernel_header::{BinaryHeader, KernelHeader, RefWidth};
use cadmpeg_core::CodecError;

fn header_without_history(width: RefWidth) -> BinaryHeader {
    BinaryHeader {
        width,
        metadata: KernelHeader {
            save_format_version: None,
            entity_count: None,
            flags: None,
            product_family: None,
            product_version: None,
            save_date: None,
            scale: None,
            linear: None,
            angular: None,
        },
    }
}

#[test]
fn acis_absent_parse_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for bytes in [b"".as_slice(), b"wrong-header".as_slice()] {
            let result = parse(ctx, bytes);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(None))),
            }
        }
    });
}

#[test]
fn acis_absent_record_start_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for bytes in [b"".as_slice(), b"wrong-header".as_slice()] {
            let result = record_stream_start(ctx, bytes);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(None))),
            }
        }
    });
}

#[test]
fn acis_absent_solved_limit_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for bytes in [b"".as_slice(), b"wrong-header".as_slice()] {
            let result = solved_record_limit(ctx, bytes);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(None))),
            }
        }
    });
}

#[test]
fn acis_header_width_recovery_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for width in [RefWidth::Four, RefWidth::Eight] {
            let header = header_without_history(width);
            let result = record_stream_start_with_header(ctx, &[], &header);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(None))),
            }
        }
    });
}

#[test]
fn acis_absent_history_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let header = header_without_history(RefWidth::Four);
        let result = solved_record_limit_with_header(ctx, &[], &header);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(None))),
        }
    });
}
