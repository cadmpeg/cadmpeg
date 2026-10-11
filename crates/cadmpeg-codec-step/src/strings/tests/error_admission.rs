// SPDX-License-Identifier: Apache-2.0
//! Syntax errors admit their owned text and preserve offsets and messages.

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::implementation_level::ImplementationLevel;
use crate::strings::{decode_with_context, StringDecodeFailure};

fn assert_error(input: &[u8], level: ImplementationLevel, offset: usize, message: &str) {
    crate::test_support::with_service_context(input, |input, ctx| {
        let Err(StringDecodeFailure::Invalid(error)) = decode_with_context(input, level, ctx)
        else {
            panic!("syntax error required");
        };
        assert_eq!(error.offset, offset);
        assert_eq!(error.message, message);
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_policy_context(input, &policy, |input, ctx| {
        let Err(StringDecodeFailure::Resource(CodecError::ResourceLimit(refusal))) =
            decode_with_context(input, level, ctx)
        else {
            panic!("owned syntax error text must refuse storage");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "STEP string error message");
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

#[test]
fn invalid_byte_escape_admits_error_text() {
    assert_error(
        b"\\X\\GG",
        ImplementationLevel::LegacyEdition1,
        3,
        "byte escape contains non-hexadecimal digits",
    );
}

#[test]
fn invalid_direct_utf8_admits_error_text() {
    assert_error(
        b"a\xff",
        ImplementationLevel::Edition3Class1,
        1,
        "invalid UTF-8 direct string bytes",
    );
}

#[test]
fn invalid_wide_digits_admit_error_text() {
    assert_error(
        b"\\X2\\GGGG\\X0\\",
        ImplementationLevel::LegacyEdition1,
        4,
        "wide escape contains non-hexadecimal digits",
    );
}

#[test]
fn invalid_wide_scalar_admits_error_text() {
    assert_error(
        b"\\X4\\00110000\\X0\\",
        ImplementationLevel::LegacyEdition1,
        4,
        "wide escape contains an invalid Unicode scalar",
    );
}
