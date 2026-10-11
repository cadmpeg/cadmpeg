// SPDX-License-Identifier: Apache-2.0
//! An exhausted viewport source has no next child to admit.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::chunks::{ArchiveVersion, FramingError};

#[test]
fn empty_viewport_userdata_has_no_terminal_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut losses = Vec::new();
    let error = super::super::scan_viewport_userdata(&ctx, &[], 0..0, ArchiveVersion::V8, &mut losses)
        .err().expect("missing class end");
    assert!(matches!(error, FramingError::Structural { offset: 0, message }
        if message == "view viewport userdata is missing its class end"));
    assert!(losses.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn exhausted_viewport_userdata_admits_only_its_actual_child() {
    let archive = ArchiveVersion::V8;
    let bytes = super::short_chunk(archive, crate::chunks::TCODE_SHORT | 7, 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut losses = Vec::new();
    let error = super::super::scan_viewport_userdata(&ctx, &bytes, 0..bytes.len(), archive, &mut losses)
        .err().expect("missing class end after one child");
    assert!(matches!(error, FramingError::Structural { offset, message }
        if offset == bytes.len() && message == "view viewport userdata is missing its class end"));
    assert!(losses.is_empty());
    ctx.finish_session().unwrap();
}
