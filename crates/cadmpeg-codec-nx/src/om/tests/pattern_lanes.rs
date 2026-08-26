// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn om_pattern_counted_reference_lane_requires_exact_terminator() {
    const TRAILER: [u8; 19] = [
        0x00, 0x00, 0x00, 0x37, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0x38, 0xff, 0x01, 0xff, 0xff,
        0xff, 0xff, 0x01, 0xff,
    ];
    let references = [[0xf1, 0x06, 0xb1], [0xf1, 0x06, 0xb2], [0xf1, 0x06, 0xb3]];
    let mut payload = vec![0xaa, 0x01, references.len() as u8 + 1];
    for reference in &references {
        payload.extend_from_slice(reference);
    }
    payload.extend_from_slice(&TRAILER);
    let payload_offset = 200;
    let record = OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset,
        payload: &payload,
        label: OperationLabel {
            header_offset: 100,
            offset: 119,
            value: "Pattern Feature",
            object_indices: [None; 4],
            object_index_offsets: [115, 116, 117, 118],
        },
    };
    let lane = pattern_payload_counted_reference_lane(record).expect("complete lane");
    assert_eq!(lane.offset, payload_offset + 1);
    assert_eq!(lane.declared_count, 4);
    assert_eq!(
        lane.references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [0x06b1, 0x06b2, 0x06b3]
    );
    assert_eq!(
        lane.references
            .iter()
            .map(|reference| reference.raw_object_index.clone())
            .collect::<Vec<_>>(),
        references
            .iter()
            .map(|reference| reference.to_vec())
            .collect::<Vec<_>>()
    );

    let mut malformed = payload.clone();
    malformed.pop();
    assert!(pattern_payload_counted_reference_lane(OperationRecord {
        bytes: &malformed,
        payload: &malformed,
        ..record
    })
    .is_none());
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(pattern_payload_counted_reference_lane(OperationRecord {
        bytes: &ambiguous,
        payload: &ambiguous,
        ..record
    })
    .is_none());
}
