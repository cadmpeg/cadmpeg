// SPDX-License-Identifier: Apache-2.0

use crate::native::features::construction_records::data_block_object_frames;
use crate::test_support::test_prt::prt_with_named_payloads;

#[test]
fn data_block_object_frame_route_admits_fixture() {
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
    let container = crate::container::scan_bytes(file.as_slice())
        .expect("synthetic data block object frame container");
    container
        .indexed_om_sections()
        .expect("cached offset-store source");
    let records =
        crate::test_support::with_decode_context(|ctx| data_block_object_frames(ctx, &container))
            .expect("data block object frame projection");
    assert_eq!(records.len(), 1);
}
