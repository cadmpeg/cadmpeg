// SPDX-License-Identifier: Apache-2.0

use crate::native::features::construction_records::data_block_object_frames;
use crate::test_support::test_prt::prt_with_named_payloads;

fn data_block_object_frame_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let frame = [
        0x73, 0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02,
        0x01, 0x02, 0x80, 0xa4,
    ];
    let mut store_records = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    store_records[0] = &frame;
    let payload = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "EXTRUDE", Vec::new())],
        &store_records,
    );
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("synthetic data block object frame container");
    crate::test_support::with_decode_context(|ctx| container.indexed_om_sections(ctx).map(|_| ()))
        .expect("cached offset-store source");
    let records =
        crate::test_support::with_decode_context(|ctx| data_block_object_frames(ctx, &container))
            .expect("data block object frame projection");
    assert_eq!(records.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| data_block_object_frames(ctx, &container).unwrap_err(),
    )
}

#[test]
fn data_block_object_frame_route_refuses_collection_limit() {
    let error =
        data_block_object_frame_route_refusal(|policy| policy.limits.max_collection_items = 1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "NX data block object frames"),
        "{error:?}"
    );
}

#[test]
fn data_block_object_frame_route_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "NX data block object frames",
        |limit| {
            Err::<(), _>(data_block_object_frame_route_refusal(|policy| {
                policy.limits.max_retained_bytes = limit;
            }))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "NX data block object frames"),
        "{error:?}"
    );
}

#[test]
fn data_block_object_frame_route_refuses_work_limit() {
    let error = data_block_object_frame_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn data_block_object_frame_owned_route_refuses_scoped_limit() {
    let store_records = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "EXTRUDE", Vec::new())],
        &store_records,
    );
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]),
        )
    })
    .expect("owned offset-store source");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            let error = data_block_object_frames(ctx, &container).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
            );
        },
    );
}
