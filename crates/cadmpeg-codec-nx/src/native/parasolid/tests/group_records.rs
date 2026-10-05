// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

use super::stream;

pub(super) fn group_record(xmt: u16, node_id: u32, linked_reference: u16) -> Vec<u8> {
    let mut bytes = vec![0, 90];
    bytes.extend_from_slice(&xmt.to_be_bytes());
    bytes.extend_from_slice(&node_id.to_be_bytes());
    for reference in [3u16, 4, 5, 6] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    bytes.push(4);
    bytes.extend_from_slice(&linked_reference.to_be_bytes());
    bytes.push(0);
    bytes
}

fn group_record_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let streams = [stream(
        crate::parasolid::ParasolidSubtype::Partition,
        "SCH_TEST",
        group_record(10, 7, 8),
    )];
    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            crate::native::parasolid::parasolid_groups(ctx, &streams, &BTreeMap::new(), &events)
                .map(|groups| groups.records)
                .expect_err("GROUP record limit refusal")
        },
    )
}

#[test]
fn group_record_route_refuses_collection_limit() {
    let error = group_record_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn group_record_route_refuses_retained_limit() {
    let error = group_record_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn group_record_route_refuses_scoped_limit() {
    let error = group_record_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn group_record_route_refuses_work_limit() {
    let error = group_record_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
