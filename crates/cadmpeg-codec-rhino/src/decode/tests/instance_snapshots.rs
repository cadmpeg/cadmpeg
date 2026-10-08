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
        .expect("one temporary row exceeds the limit");
    // Scoped byte admission uses the byte operation before collection admission.
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
    use cadmpeg_core::decode::ResourceDimension;
    let refusal = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "Rhino instance status snapshot",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let result = snapshot_instance_statuses(&ctx, &[Some(GeometryOutcome::Decoded)]);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(ref limit)) = result {
                assert_eq!(ctx.resource_refusal(), Some(*limit));
            }
            result.map(|(statuses, _storage)| statuses)
        },
    );
    // copy_slice admits one status slot and its size under its own operation.
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino instance status snapshot"
                && limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn instance_snapshots_admit_work_and_hold_scoped_storage() {
    for links in [false, true] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = if links {
            snapshot_instance_links(&ctx, &[one_instance_link_record()])
                .err()
                .unwrap()
        } else {
            snapshot_instance_statuses(&ctx, &[Some(GeometryOutcome::Decoded)]).unwrap_err()
        };
        let cadmpeg_core::CodecError::ResourceLimit(first) = error else {
            panic!("snapshot work must refuse");
        };
        assert_eq!(
            first.dimension,
            cadmpeg_core::decode::ResourceDimension::WorkUnits
        );
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first)
        );
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let links = snapshot_instance_links(&ctx, &[one_instance_link_record()]).unwrap();
    let statuses = snapshot_instance_statuses(&ctx, &[Some(GeometryOutcome::Decoded)]).unwrap();
    assert_eq!(links.links, [vec!["rhino:curve#1".to_string()]]);
    assert_eq!(statuses.0, [Some(GeometryOutcome::Decoded)]);
    drop(statuses);
    drop(links);
    let all_storage = ctx
        .reserve_scoped(4096, "instance snapshots released")
        .unwrap();
    drop(all_storage);
    ctx.finish_session().unwrap();
}


#[test]
fn instance_status_snapshot_charges_one_visit_per_status() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // copy_slice visits two status elements; status width does not add work.
    policy.limits.max_work_units = 2;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let statuses = [Some(GeometryOutcome::Decoded), Some(GeometryOutcome::Failed)];
    let copied = snapshot_instance_statuses(&ctx, &statuses).expect("two visits fit");
    assert_eq!(copied.0, statuses);
    drop(copied);
    ctx.finish_session().unwrap();
}
