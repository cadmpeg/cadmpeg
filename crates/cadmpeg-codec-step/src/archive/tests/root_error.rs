// SPDX-License-Identifier: Apache-2.0
//! Root validation admits the error text that escapes directory storage.

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension, View};
use cadmpeg_core::CodecError;
use zip::CompressionMethod;

fn assert_root_error(bytes: &[u8], expected: &str) {
    crate::test_support::with_service_context(bytes, |bytes, ctx| {
        let error = crate::archive::open_root(ctx, View::over_retained(bytes))
            .err()
            .expect("root error");
        match error {
            CodecError::Malformed(message)
            | CodecError::NotImplemented(message)
            | CodecError::WrongFormat(message) => assert_eq!(message, expected),
            error => panic!("unexpected error: {error}"),
        }
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_policy_context(bytes, &policy, |bytes, ctx| {
        let Some(CodecError::ResourceLimit(refusal)) =
            crate::archive::open_root(ctx, View::over_retained(bytes)).err()
        else {
            panic!("escaping root error text must refuse retained storage");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "STEP ZIP root error");
    });
}

fn alter_header(bytes: &mut [u8], local: usize, central: usize, value: u16) {
    let start = bytes
        .windows(4)
        .position(|bytes| bytes == b"PK\x01\x02")
        .expect("synthetic central header");
    cadmpeg_test_support::bytes::put_u16(bytes, local, value);
    cadmpeg_test_support::bytes::put_u16(bytes, start + central, value);
}

#[test]
fn missing_zip_root_admits_error_text() {
    let bytes = super::step_zip(&[("safe", b"".as_slice(), CompressionMethod::Stored)]);
    assert_root_error(&bytes, "STEP ZIP has no required root ISO-10303.p21");
}

#[test]
fn unicode_zip_name_admits_error_text() {
    let mut bytes = super::step_zip(&[(
        crate::archive::ROOT_NAME,
        b"".as_slice(),
        CompressionMethod::Stored,
    )]);
    alter_header(&mut bytes, 6, 8, 1 << 11);
    assert_root_error(&bytes, "STEP ZIP uses prohibited Unicode filename support");
}

#[test]
fn zstd_zip_member_admits_error_text() {
    let mut bytes = super::step_zip(&[(
        crate::archive::ROOT_NAME,
        b"".as_slice(),
        CompressionMethod::Stored,
    )]);
    alter_header(&mut bytes, 8, 10, 93);
    assert_root_error(
        &bytes,
        "STEP ZIP requires PKZIP 2.04g stored or Deflate entries",
    );
}
