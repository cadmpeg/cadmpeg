// SPDX-License-Identifier: Apache-2.0
//! Error evidence survives actual destination and session failures.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure,
};
use cadmpeg_core::target::{TargetCatalog, TargetRefusal};

use super::EncodeError;

fn read_only_file() -> File {
    File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("crate manifest opens read-only")
}

fn write_byte(writer: &mut impl Write) -> Result<(), EncodeError> {
    writer.write_all(b"x")?;
    Ok(())
}

fn seek_before_start(writer: &mut impl Seek) -> Result<(), EncodeError> {
    writer.seek(SeekFrom::Current(-1))?;
    Ok(())
}

#[test]
fn actual_write_failure_retains_os_error_evidence() {
    let mut file = read_only_file();
    let expected = file
        .write_all(b"x")
        .expect_err("a read-only handle cannot write a byte");
    let error = write_byte(&mut file).expect_err("write error propagates through question mark");
    let EncodeError::Io(actual) = error else {
        panic!("an actual write failure must remain Io");
    };
    assert_eq!(actual.kind(), expected.kind());
    assert_eq!(actual.raw_os_error(), expected.raw_os_error());
    assert_eq!(actual.to_string(), expected.to_string());
}

#[test]
fn actual_seek_failure_retains_os_error_evidence() {
    let mut file = read_only_file();
    let expected = file
        .seek(SeekFrom::Current(-1))
        .expect_err("a file cannot seek before its beginning");
    let error =
        seek_before_start(&mut file).expect_err("seek error propagates through question mark");
    let EncodeError::Io(actual) = error else {
        panic!("an actual seek failure must remain Io");
    };
    assert_eq!(actual.kind(), expected.kind());
    assert_eq!(actual.raw_os_error(), expected.raw_os_error());
    assert_eq!(actual.to_string(), expected.to_string());
}

#[test]
fn actual_session_limit_retains_every_evidence_field() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 3;
    assert!(DecodeContext::from_root_bytes_limit(b"tri", &arena, &policy).is_ok());
    let original = DecodeContext::from_root_bytes_limit(b"four", &arena, &policy)
        .err()
        .expect("four input bytes exceed the caller's three-byte allowance");
    assert_eq!(original.dimension, ResourceDimension::InputBytes);
    assert_eq!(original.reason, ResourceFailure::BudgetExceeded);
    assert_eq!(original.limit, 3);
    assert_eq!(original.used, 4);
    assert_eq!(original.additional, 1);
    let error = EncodeError::from(original);
    let EncodeError::ResourceLimit(actual) = error else {
        panic!("a session refusal must remain ResourceLimit");
    };
    assert_eq!(actual, original);
}

#[test]
fn unresolved_target_retains_catalog_and_requested_token() {
    let original = TargetRefusal::unknown_explicit("future", TargetCatalog::empty("cadir"));
    let expected = original.clone();
    let error = EncodeError::UnsupportedTarget(Box::new(original));
    let EncodeError::UnsupportedTarget(actual) = &error else {
        panic!("a target refusal must retain its typed evidence");
    };
    assert_eq!(actual.as_ref(), &expected);
    assert_eq!(error.to_string(), expected.to_string());
    assert_eq!(actual.format(), "cadir");
    assert!(actual.available().is_empty());
}

#[test]
fn actual_encode_buffer_limit_becomes_the_same_typed_encode_error() {
    use crate::codec::write::EncodeSession;
    let mut limits = cadmpeg_core::decode::ResourceLimits::service();
    limits.max_retained_bytes = 3;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy {
        limits,
        ..cadmpeg_core::decode::DecodePolicy::service()
    };
    let ctx = cadmpeg_core::decode::DecodeContext::new(
        &arena,
        &policy,
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    let session = EncodeSession::from_context(&ctx).expect("one caller account");
    let mut buffer = session.buffer();
    buffer
        .append(b"tri", "retain first record")
        .expect("exact caller limit");
    let original = buffer
        .append(b"x", "retain next record")
        .expect_err("real encoded bytes exceed the caller limit");
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(original.reason, ResourceFailure::BudgetExceeded);
    assert_eq!(
        (original.limit, original.used, original.additional),
        (3, 3, 1)
    );
    let EncodeError::ResourceLimit(actual) = EncodeError::from(original) else {
        panic!("actual encode session failure must retain its typed resource evidence")
    };
    assert_eq!(actual, original);
    assert_eq!(buffer.as_bytes(), b"tri");
}
