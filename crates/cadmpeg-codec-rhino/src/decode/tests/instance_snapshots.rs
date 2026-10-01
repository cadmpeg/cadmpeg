// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use super::{
    snapshot_instance_links, snapshot_instance_statuses, with_collection_limit, GeometryOutcome,
    UnknownId, UnknownRecord,
};

fn one_instance_link_record() -> UnknownRecord {
    UnknownRecord::unavailable(
        UnknownId::mint("rhino:object:unknown#0").expect("valid identity"),
        0,
        0,
        "",
        vec!["rhino:curve#1".to_string()],
    )
}

#[test]
fn instance_link_snapshot_rows_refuse_collection_limit() {
    let refusal = with_collection_limit(0, |ctx| {
        snapshot_instance_links(ctx, &[one_instance_link_record()])
            .err()
            .expect("one row exceeds the collection limit")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance link snapshot rows"
    ));
}

#[test]
fn instance_link_snapshot_entries_refuse_collection_limit() {
    let refusal = with_collection_limit(1, |ctx| {
        snapshot_instance_links(ctx, &[one_instance_link_record()])
            .err()
            .expect("one entry exceeds the collection limit")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance link snapshot entries"
    ));
}

#[test]
fn instance_link_snapshot_bytes_refuse_materialized_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 12;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let refusal = snapshot_instance_links(&ctx, &[one_instance_link_record()])
        .err()
        .expect("thirteen temporary bytes exceed the limit");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance link snapshot bytes"
    ));
}

#[test]
fn instance_status_snapshot_refuses_collection_limit() {
    let refusal = with_collection_limit(0, |ctx| {
        snapshot_instance_statuses(ctx, &[Some(GeometryOutcome::Decoded)])
            .expect_err("one status exceeds the collection limit")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance status snapshot"
    ));
}

#[test]
fn instance_status_snapshot_bytes_refuse_materialized_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let bytes = std::mem::size_of::<Option<GeometryOutcome>>();
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(bytes - 1);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let refusal = snapshot_instance_statuses(&ctx, &[Some(GeometryOutcome::Decoded)])
        .expect_err("one status exceeds the temporary-byte limit");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance status snapshot bytes"
    ));
}
