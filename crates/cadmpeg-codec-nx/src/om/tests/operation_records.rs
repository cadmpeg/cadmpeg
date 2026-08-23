// SPDX-License-Identifier: Apache-2.0
//! Operation-header validation and record-boundary tests.

use super::*;

#[test]
fn unlabeled_operation_header_still_bounds_adjacent_records() {
    const HEADER: &[u8] = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff";
    let mut bytes = Vec::new();
    bytes.extend(b"\x80\xcd\x01\x04\x01\x2f\x5a\x36\xe2\xeb\x1c\x43\x2d\xff\xff");
    bytes.extend(b"\xff\xff\xff\xff\x03\x07BLOCK\0owned");
    bytes.extend(HEADER);
    bytes.extend(b"\xff\xff\xff\xff\x01\x02\x0b\x21\x97\x75\x01\x02\x10\x22\xff");
    bytes.extend(HEADER);
    bytes.extend(b"\xff\xff\xff\xff\x03\x08SKETCH\0tail");

    let labels = operation_labels(&bytes, 100);
    let records = operation_records_with_labels_and_ordinals(&bytes, 100, &labels);
    assert_eq!(labels.len(), 2);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].0, 0);
    assert_eq!(records[0].1.label.value, "BLOCK");
    assert_eq!(records[0].1.payload, b"owned");
    assert_eq!(records[1].0, 2);
    assert_eq!(records[1].1.label.value, "SKETCH");
    assert_eq!(records[1].1.payload, b"tail");
}

#[test]
fn every_body_identity_opens_a_body_write_frame() {
    let payload = [
        0x01, 0x02, 0x11, 0x80, 0xa9, 0x97, 0x75, 0x01, 0x02, 0x10, 0x86, 0x93, 0xff,
    ];
    let label = OperationLabel {
        header_offset: 100,
        offset: 100,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    let writes = operation_body_write_frames(record);
    let [write] = writes.as_slice() else {
        panic!("one body-write frame");
    };
    assert_eq!(write.body_identity, 0x11);
    assert_eq!(write.group_node, 0xa9);
    assert_eq!(write.body_image_object_index, 0x693);

    let mut invalid_endpoint = payload;
    invalid_endpoint[9] = 0x11;
    assert!(operation_body_write_frames(OperationRecord {
        bytes: &invalid_endpoint,
        payload: &invalid_endpoint,
        ..record
    })
    .is_empty());
}
