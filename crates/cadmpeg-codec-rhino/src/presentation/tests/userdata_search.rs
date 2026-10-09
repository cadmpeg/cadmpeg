// SPDX-License-Identifier: Apache-2.0
//! Empty userdata sources execute no search visits and preserve a refused session.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use crate::chunks::ArchiveVersion;

#[test]
fn empty_user_string_sources_have_no_terminal_work_or_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut losses = Vec::new();
    let (geometry, attributes) = crate::presentation::first_user_string_records(
        &ctx, &[], ArchiveVersion::V8, &[], &[], 0, &mut losses,
    ).unwrap();
    assert!(geometry.is_empty());
    assert_eq!(geometry.capacity(), 0);
    assert!(attributes.is_empty());
    assert_eq!(attributes.capacity(), 0);
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn empty_user_string_sources_keep_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "Rhino original search refusal").unwrap_err()
        else { panic!("original work refusal"); };
    let mut losses = Vec::new();
    let CodecError::ResourceLimit(refusal) = crate::presentation::first_user_string_records(
        &ctx, &[], ArchiveVersion::V8, &[], &[], 0, &mut losses,
    ).err().expect("a refused session cannot search")
        else { panic!("original work refusal stays typed"); };
    assert_eq!(refusal, original);
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}
