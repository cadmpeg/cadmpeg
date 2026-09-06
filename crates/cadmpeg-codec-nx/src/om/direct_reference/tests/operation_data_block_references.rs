// SPDX-License-Identifier: Apache-2.0

use crate::om::{OperationLabel, OperationRecord};
use crate::om::direct_reference::{operation_reference_fields, DirectReferenceFrame, ReferenceFieldKind};

fn record(payload: &[u8], payload_offset: usize) -> OperationRecord<'_> {
    OperationRecord {
        bytes: payload,
        payload_offset,
        payload,
        label: OperationLabel {
            header: crate::om::header_references::OperationHeader::<usize>::new(100, crate::om::header_references::HeaderReferences([None; 4])).unwrap(),
            value: "EXTRUDE",
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
        operation_reference_fields(record(&payload, payload_offset), ReferenceFieldKind::DataBlock03),
        [
            {
                let frame = DirectReferenceFrame::<usize>::new(ReferenceFieldKind::DataBlock03, crate::om::reference_index::CanonicalFeatureReferenceToken::from_wire(0x6a, &[0x6a]).unwrap(), payload_offset + first_start).unwrap();
                assert_eq!(frame.object_offset(), payload_offset + first_start + 3);
                assert_eq!(frame.offset() + usize::from(frame.byte_len()), payload_offset + first_start + first.len());
                frame
            },
            {
                let frame = DirectReferenceFrame::<usize>::new(ReferenceFieldKind::DataBlock03, crate::om::reference_index::CanonicalFeatureReferenceToken::from_wire(0x645, &[0x86, 0x45]).unwrap(), payload_offset + second_start).unwrap();
                assert_eq!(frame.object_offset(), payload_offset + second_start + 3);
                assert_eq!(frame.offset() + usize::from(frame.byte_len()), payload_offset + second_start + second.len());
                frame
            },
            {
                let frame = DirectReferenceFrame::<usize>::new(ReferenceFieldKind::DataBlock03, crate::om::reference_index::CanonicalFeatureReferenceToken::from_wire(0x1234, &[0x90, 0x12, 0x34]).unwrap(), payload_offset + third_start).unwrap();
                assert_eq!(frame.object_offset(), payload_offset + third_start + 3);
                assert_eq!(frame.offset() + usize::from(frame.byte_len()), payload_offset + third_start + third.len());
                frame
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
        assert!(operation_reference_fields(record(payload, 500), ReferenceFieldKind::DataBlock03).is_empty());
    }
}
