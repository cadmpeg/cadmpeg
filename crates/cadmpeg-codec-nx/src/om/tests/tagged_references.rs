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

fn tagged_field(index: &[u8]) -> Vec<u8> {
    [
        [0x01, 0x02, 0x17].as_slice(),
        index,
        [0xff, 0x80, 0x00, 0x00, 0x02].as_slice(),
    ]
    .concat()
}

#[test]
fn direct_tagged_references_retain_canonical_indices_and_bounds() {
    let first = tagged_field(&[0x6a]);
    let second = tagged_field(&[0x86, 0x45]);
    let third = tagged_field(&[0x90, 0x12, 0x34]);
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
        operation_tagged_references(record(&payload, payload_offset)),
        [
            OperationTaggedReference {
                offset: payload_offset + first_start,
                tag: 0x17,
                object_index: 0x6a,
                raw_object_index: vec![0x6a],
                object_index_offset: payload_offset + first_start + 3,
                end_offset: payload_offset + first_start + first.len(),
            },
            OperationTaggedReference {
                offset: payload_offset + second_start,
                tag: 0x17,
                object_index: 0x645,
                raw_object_index: vec![0x86, 0x45],
                object_index_offset: payload_offset + second_start + 3,
                end_offset: payload_offset + second_start + second.len(),
            },
            OperationTaggedReference {
                offset: payload_offset + third_start,
                tag: 0x17,
                object_index: 0x1234,
                raw_object_index: vec![0x90, 0x12, 0x34],
                object_index_offset: payload_offset + third_start + 3,
                end_offset: payload_offset + third_start + third.len(),
            },
        ]
    );
}

#[test]
fn direct_tagged_references_do_not_admit_nested_or_incomplete_frames() {
    let nested = [
        0x01, 0x02, 0x17, 0x81, 0x23, 0x97, 0x75, 0x01, 0x02, 0x11, 0x86, 0x45, 0xff,
    ];
    let incomplete = [0x01, 0x02, 0x17, 0x81, 0x23, 0xff, 0x80, 0x00, 0x00];
    let noncanonical = [0x01, 0x02, 0x17, 0x80, 0x45, 0xff, 0x80, 0x00, 0x00, 0x02];

    for payload in [&nested[..], &incomplete[..], &noncanonical[..]] {
        assert!(operation_tagged_references(record(payload, 500)).is_empty());
    }
}
