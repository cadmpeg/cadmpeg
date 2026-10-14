// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_datum_csys_constructions;
use crate::test_support::test_om::composed_feature_history_payload_over_sort_scratch;
use crate::test_support::test_prt::prt_with_named_payloads;

#[test]
fn datum_csys_admits_fixture() {
    let mut frame = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for _ in 0..8 {
        frame.extend_from_slice(&[0xf0, 0x03]);
    }
    frame.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    let store = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload = composed_feature_history_payload_over_sort_scratch(
        &[(&[3, 0xff, 0xff, 0xff], "DATUM_CSYS", frame)],
        &store,
    );
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic datum CSYS container");
    let admitted =
        feature_datum_csys_constructions(&container).expect("admitted datum CSYS constructions");
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].frame.members().len(), 8);
}
