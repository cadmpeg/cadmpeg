// SPDX-License-Identifier: Apache-2.0
//! Actual Rhino staging bytes share the caller's fallible retained allowance.

use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::EncodeSession;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::CadIr;

use crate::RhinoArchiveVersion;

fn emit(ir: &CadIr, retained: u64) -> Result<Vec<u8>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let session = cadmpeg_core::encode::EncodeSession::from_context(&ctx)?;
    let (bytes, _) = super::super::write(&ctx, ir, RhinoArchiveVersion::V8, &session)?;
    ctx.finish_session()?;
    Ok(bytes.into_vec())
}

#[test]
fn the_exact_retained_archive_boundary_keeps_all_native_bytes() {
    let ir = CadIr::empty();
    let expected = emit(&ir, 1_000_000).expect("bounded empty native archive");
    let size = cadmpeg_core::decode::u64_from_index(expected.len());
    assert_eq!(
        emit(&ir, size).expect("all emitted bytes fit exactly"),
        expected
    );
    let CodecError::ResourceLimit(error) =
        emit(&ir, size - 1).expect_err("the complete EOF record exceeds one fewer byte")
    else {
        panic!("retained caller limit")
    };
    assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(error.limit, size - 1);
    assert!(error.used <= error.limit);
    assert!(error.additional > 0);
    assert_eq!(error.used + error.additional, size);
    assert_eq!(error.operation, "retain Rhino archive bytes");
}

#[test]
fn a_zero_byte_budget_declines_before_retaining_the_archive_header() {
    let CodecError::ResourceLimit(error) =
        emit(&CadIr::empty(), 0).expect_err("the actual header requires retained bytes")
    else {
        panic!("real caller allowance")
    };
    assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((error.limit, error.used), (0, 0));
    assert_eq!(
        error.additional,
        cadmpeg_core::decode::u64_from_index(crate::layout::file_header::LEN)
    );
    assert_eq!(error.operation, "retain Rhino archive bytes");
}

#[test]
fn brep_and_final_archive_staging_share_one_actual_live_allowance() {
    let ir = super::polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 3.0, 0.0),
    ]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).expect("caller account");
    let plan = super::super::prepare_write(&ctx, &ir, RhinoArchiveVersion::V8, &session)
        .expect("actual native Brep records");
    let staged_brep = cadmpeg_core::decode::u64_from_index(plan.brep_records.capacity());
    assert!(staged_brep > 0);
    let expected = emit(&ir, 1_000_000).expect("source-less Brep native archive");
    // The archive is built in the Brep staging buffer, so the peak is the
    // archive alone.
    let peak = cadmpeg_core::decode::u64_from_index(expected.len());
    assert_eq!(emit(&ir, peak).expect("the archive fits exactly"), expected);
    let CodecError::ResourceLimit(error) =
        emit(&ir, peak - 1).expect_err("the complete archive exceeds one fewer byte")
    else {
        panic!("shared retained-byte ceiling")
    };
    assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(error.limit, peak - 1);
    assert!(error.used <= error.limit);
    assert!(error.additional > 0);
    assert_eq!(error.used + error.additional, peak);
}

#[test]
fn actual_archive_header_shares_preexisting_native_retention() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let header = cadmpeg_core::decode::u64_from_index(crate::layout::file_header::LEN);
    policy.limits.max_retained_bytes = header + 5;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let native = Box::new(*b"native");
    ctx.retain_bytes(6, "preexisting native fields")
        .expect("native fields");
    let session = EncodeSession::from_context(&ctx).expect("one account");
    let CodecError::ResourceLimit(error) =
        super::super::write(&ctx, &CadIr::empty(), RhinoArchiveVersion::V8, &session)
            .err()
            .expect("header plus native exceeds caller limit")
    else {
        panic!("actual retained refusal")
    };
    assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        (error.limit, error.used, error.additional),
        (header + 5, 6, header)
    );
    assert_eq!(error.operation, "retain Rhino archive bytes");
    assert_eq!(native.as_slice(), b"native");
    assert_eq!(session.finish(), Err(error));
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == error)
    );
}
