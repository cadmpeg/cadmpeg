// SPDX-License-Identifier: Apache-2.0

use super::*;

fn record(payload: &[u8], payload_offset: usize) -> OperationRecord<'_> {
    OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset,
        payload,
        label: OperationLabel {
            header_offset: 100,
            offset: 119,
            value: "EXTRUDE",
            object_indices: [None; 4],
            object_index_offsets: [115, 116, 117, 118],
        },
    }
}

fn data_block_field(index: &[u8]) -> Vec<u8> {
    [
        [0x01, 0x02, 0x03].as_slice(),
        index,
        [0x01, 0x00, 0x00, 0x00, 0x00, 0x00].as_slice(),
    ]
    .concat()
}

#[test]
fn operation_data_block_references_retain_canonical_indices_and_bounds() {
    let first = data_block_field(&[0x6a]);
    let second = data_block_field(&[0x86, 0x45]);
    let third = data_block_field(&[0x90, 0x12, 0x34]);
    let prefix = [0xaa, 0xbb];
    let first_start = prefix.len();
    let second_start = first_start + first.len() + 1;
    let third_start = second_start + second.len() + 1;
    let payload = [
        prefix.as_slice(),
        first.as_slice(),
        [0xcc].as_slice(),
        second.as_slice(),
        [0xdd].as_slice(),
        third.as_slice(),
    ]
    .concat();
    let payload_offset = 700;

    assert_eq!(
        operation_data_block_references(record(&payload, payload_offset)),
        [
            OperationDataBlockReference {
                offset: payload_offset + first_start,
                object_index: 0x6a,
                raw_object_index: vec![0x6a],
                object_index_offset: payload_offset + first_start + 3,
                end_offset: payload_offset + first_start + first.len(),
            },
            OperationDataBlockReference {
                offset: payload_offset + second_start,
                object_index: 0x645,
                raw_object_index: vec![0x86, 0x45],
                object_index_offset: payload_offset + second_start + 3,
                end_offset: payload_offset + second_start + second.len(),
            },
            OperationDataBlockReference {
                offset: payload_offset + third_start,
                object_index: 0x1234,
                raw_object_index: vec![0x90, 0x12, 0x34],
                object_index_offset: payload_offset + third_start + 3,
                end_offset: payload_offset + third_start + third.len(),
            },
        ]
    );
}

#[test]
fn operation_data_block_references_reject_noncanonical_and_incomplete_frames() {
    let noncanonical = data_block_field(&[0x80, 0x45]);
    let incomplete = [0x01, 0x02, 0x03, 0x81, 0x23, 0x01, 0x00, 0x00];
    let wrong_suffix = [
        0x01, 0x02, 0x03, 0x81, 0x23, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let null_index = [0x01, 0x02, 0x03, 0xff, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];

    for payload in [
        &noncanonical[..],
        &incomplete[..],
        &wrong_suffix[..],
        &null_index[..],
    ] {
        assert!(operation_data_block_references(record(payload, 500)).is_empty());
    }
}
