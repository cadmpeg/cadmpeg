// SPDX-License-Identifier: Apache-2.0
//! Wide-string validation and copying use the original decode session.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};

fn wide_chunk(format: u8, text: &[u8]) -> Vec<u8> {
    let mut payload = vec![format];
    payload.extend_from_slice(text);
    crate::test_support::test_dump::crc_chunk(ArchiveVersion::V8, 0x4000_8001, &payload)
}

#[test]
fn wide_utf8_preserves_unicode_nuls_and_exact_copy_work() {
    for text in ["Arial", "Aé\0😀", ""] {
        let bytes = wide_chunk(1, text.as_bytes());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2 * u64::try_from(text.len()).unwrap();
        policy.limits.max_retained_bytes = u64::try_from(text.len()).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        assert_eq!(crate::presentation::wide_string(&ctx, &bytes, &mut reader, ArchiveVersion::V8).unwrap(), text);
        assert_eq!(reader.position(), bytes.len());
        assert_eq!(ctx.resource_refusal(), None);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn malformed_wide_utf8_preserves_the_body_end_error_and_reader_position() {
    let bytes = wide_chunk(1, &[0xff]);
    let end = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false).unwrap().body().end;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    let error = crate::presentation::wide_string(&ctx, &bytes, &mut reader, ArchiveVersion::V8).unwrap_err();
    assert!(matches!(error, FramingError::Structural { offset, message } if offset == end && message == "wide string is not UTF-8"));
    assert_eq!(reader.position(), 0);
    ctx.finish_session().unwrap();
}

fn assert_refusal(text: &[u8], work: u64, retained: u64, operation: &'static str,
    dimension: ResourceDimension, used: u64) {
    let bytes = wide_chunk(1, text);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    let FramingError::Resource(refusal) = crate::presentation::wide_string(&ctx, &bytes, &mut reader, ArchiveVersion::V8).unwrap_err()
        else { panic!("wide-string resource refusal"); };
    assert_eq!(refusal.operation, operation);
    assert_eq!(refusal.dimension, dimension);
    assert_eq!(refusal.used, used);
    assert_eq!(refusal.additional, u64::try_from(text.len()).unwrap());
    assert_eq!(reader.position(), 0);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn wide_utf8_refuses_before_validating_valid_text() {
    let text = "Aé😀".as_bytes();
    assert_refusal(text, u64::try_from(text.len()).unwrap() - 1, 0,
        "validate Rhino wide string UTF-8", ResourceDimension::WorkUnits, 0);
}

#[test]
fn wide_utf8_refuses_before_validating_invalid_text() {
    assert_refusal(&[0xff], 0, 0, "validate Rhino wide string UTF-8", ResourceDimension::WorkUnits, 0);
}

#[test]
fn wide_utf8_copy_refuses_after_validation_work() {
    let text = "Aé😀".as_bytes();
    let size = u64::try_from(text.len()).unwrap();
    assert_refusal(text, size, size, "Rhino wide string", ResourceDimension::WorkUnits, size);
}

#[test]
fn wide_utf8_retained_copy_refuses_its_exact_byte_count() {
    let text = "Aé😀".as_bytes();
    let size = u64::try_from(text.len()).unwrap();
    assert_refusal(text, 2 * size, 0, "Rhino wide string", ResourceDimension::RetainedBytes, 0);
}

#[test]
fn format_zero_empty_wide_string_has_no_work_or_storage_fee() {
    let bytes = wide_chunk(0, &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert_eq!(crate::presentation::wide_string(&ctx, &bytes, &mut reader, ArchiveVersion::V8).unwrap(), "");
    assert_eq!(reader.position(), bytes.len());
    ctx.finish_session().unwrap();
}

#[test]
fn format_zero_wide_string_returns_the_original_sticky_refusal() {
    let bytes = wide_chunk(0, &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "wide string first refusal").unwrap_err()
        else { panic!("original work refusal"); };
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert!(matches!(crate::presentation::wide_string(&ctx, &bytes, &mut reader, ArchiveVersion::V8), Err(FramingError::Resource(sticky)) if sticky == original));
    assert_eq!(reader.position(), 0);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn unsupported_wide_format_does_not_scan_its_payload() {
    let bytes = wide_chunk(2, &[0xff; 1024]);
    let start = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false).unwrap().body().start;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert!(matches!(crate::presentation::wide_string(&ctx, &bytes, &mut reader, ArchiveVersion::V8), Err(FramingError::Structural { offset, message }) if offset == start && message == "wide-string format is unsupported"));
    assert_eq!(reader.position(), 0);
    ctx.finish_session().unwrap();
}
