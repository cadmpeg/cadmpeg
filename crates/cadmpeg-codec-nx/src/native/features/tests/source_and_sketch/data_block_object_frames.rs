// SPDX-License-Identifier: Apache-2.0

use crate::native::features::construction_records::data_block_object_frames;
use crate::test_support::test_prt::prt_with_named_payloads;

fn data_block_object_frame_route_refusal(
    dimension: cadmpeg_core::decode::ResourceDimension,
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

    crate::test_support::resource_refusal_at(
        &file,
        dimension,
        if dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits {
            "scan NX data-block object frames"
        } else {
            "NX data block object frames"
        },
        |ctx| data_block_object_frames(ctx, &container),
    )
}

#[test]
fn data_block_object_frame_route_refuses_collection_limit() {
    let error = data_block_object_frame_route_refusal(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems && limit.operation == "NX data block object frames")
    );
}

#[test]
fn data_block_object_frame_route_refuses_retained_limit() {
    let error = data_block_object_frame_route_refusal(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.operation == "NX data block object frames")
    );
}

#[test]
fn data_block_object_frame_route_refuses_work_limit() {
    let error =
        data_block_object_frame_route_refusal(cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
