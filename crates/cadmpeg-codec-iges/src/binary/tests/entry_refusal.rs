// SPDX-License-Identifier: Apache-2.0

use super::super::{
    normalize_global, padding_is_zero, render_card, render_directory_card,
    render_parameter_line, render_parameter_value, render_terminate, BinaryValue,
    CARD_DATA_WIDTH, MAX_SEQUENCE, PARAMETER_DATA_WIDTH, ValueStream,
};
use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_core::CodecError;

fn malformed_or_original<T>(result: Result<T, CodecError>, original: Option<ResourceLimit>) {
    match original {
        Some(first) => assert!(matches!(result,
            Err(CodecError::ResourceLimit(last)) if last == first)),
        None => assert!(matches!(result, Err(CodecError::Malformed(_)))),
    }
}

#[test]
fn binary_default_parameter_preserves_original_refusal_and_is_free_when_fresh() {
    crate::test_support::with_entry_context(|ctx, original| {
        for language in [false, true] {
            let result = render_parameter_value(&BinaryValue::Default, language, ctx);
            match original {
                Some(first) => assert!(matches!(result,
                    Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(value) if value.is_empty())),
            }
        }
    });
}

#[test]
fn binary_empty_string_parameter_preserves_original_refusal_before_malformed_result() {
    crate::test_support::with_entry_context(|ctx, original| {
        malformed_or_original(render_parameter_value(&BinaryValue::String(Vec::new()), false, ctx), original);
    });
}

#[test]
fn binary_truncated_padding_preserves_original_refusal_before_range_recovery() {
    crate::test_support::with_entry_context(|ctx, original| {
        malformed_or_original(padding_is_zero(&[], 0..1, ctx), original);
    });
}

#[test]
fn binary_invalid_card_preserves_original_refusal_and_output_state() {
    crate::test_support::with_entry_context(|ctx, original| {
        for (data, initial_sequence) in [
            (&[b'x'; CARD_DATA_WIDTH + 1][..], 1),
            (&[][..], 0),
            (&[][..], MAX_SEQUENCE + 1),
        ] {
            let mut output = b"unchanged".to_vec();
            let mut sequence = initial_sequence;
            malformed_or_original(render_card(&mut output, data, b'S', &mut sequence, ctx), original);
            assert_eq!(output, b"unchanged");
            assert_eq!(sequence, initial_sequence);
        }
    });
}

#[test]
fn binary_invalid_terminate_count_preserves_original_refusal_and_output_state() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut output = b"unchanged".to_vec();
        malformed_or_original(render_terminate(&mut output, usize::MAX, 0, 0, 0, ctx), original);
        assert_eq!(output, b"unchanged");
    });
}

#[test]
fn binary_short_global_preserves_original_refusal_before_field_count_recovery() {
    crate::test_support::with_entry_context(|ctx, original| {
        malformed_or_original(normalize_global(&[], ctx), original);
    });
}

#[test]
fn binary_invalid_directory_sequence_preserves_original_refusal_and_output_state() {
    crate::test_support::with_entry_context(|ctx, original| {
        for sequence in [0, MAX_SEQUENCE + 1] {
            let mut output = b"unchanged".to_vec();
            malformed_or_original(render_directory_card(&mut output, &[b' '; CARD_DATA_WIDTH], sequence, ctx), original);
            assert_eq!(output, b"unchanged");
        }
    });
}

#[test]
fn binary_invalid_parameter_card_preserves_original_refusal_and_output_state() {
    crate::test_support::with_entry_context(|ctx, original| {
        for (data, owner, sequence) in [
            (&[b'x'; PARAMETER_DATA_WIDTH + 1][..], 1, 1),
            (&[][..], 0, 1),
            (&[][..], MAX_SEQUENCE + 1, 1),
            (&[][..], 1, 0),
            (&[][..], 1, MAX_SEQUENCE + 1),
        ] {
            let mut output = b"unchanged".to_vec();
            malformed_or_original(render_parameter_line(&mut output, data, owner, sequence, ctx), original);
            assert_eq!(output, b"unchanged");
        }
    });
}

#[test]
fn binary_stream_fixed_primitive_preserves_original_refusal_before_default_or_format_recovery() {
    crate::test_support::with_entry_context(|ctx, original| {
        for format in [0, 7] {
            let mut stream = ValueStream::new(&[], super::lengths(), ctx);
            let result = stream.one(format);
            match original {
                Some(first) => assert!(matches!(result,
                    Err(CodecError::ResourceLimit(last)) if last == first)),
                None if format == 0 => assert!(matches!(result, Ok(BinaryValue::Default))),
                None => assert!(matches!(result, Err(CodecError::Malformed(_)))),
            }
            assert_eq!((stream.bits.byte, stream.bits.bit), (0, 0));
            assert!(stream.pending.is_empty());
        }
    });
}

#[test]
fn binary_stream_scalar_clone_preserves_original_refusal_without_allocating() {
    crate::test_support::with_entry_context(|ctx, original| {
        let stream = ValueStream::new(&[], super::lengths(), ctx);
        for value in [BinaryValue::Default, BinaryValue::Integer(1),
            BinaryValue::Pointer(3), BinaryValue::Real(cadmpeg_ir::scalar::FiniteReal::ZERO)] {
            let result = stream.clone_value(&value);
            match original {
                Some(first) => assert!(matches!(result,
                    Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(copy) if copy == value)),
            }
        }
        assert_eq!((stream.bits.byte, stream.bits.bit), (0, 0));
        assert!(stream.pending.is_empty());
    });
}
