// SPDX-License-Identifier: Apache-2.0
//! Empty userdata sources execute no search visits.

use crate::chunks::ArchiveVersion;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
        &ctx,
        &[],
        ArchiveVersion::V8,
        &[],
        &[],
        0,
        &mut losses,
    )
    .unwrap();
    assert!(geometry.is_empty());
    assert_eq!(geometry.capacity(), 0);
    assert!(attributes.is_empty());
    assert_eq!(attributes.capacity(), 0);
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}
