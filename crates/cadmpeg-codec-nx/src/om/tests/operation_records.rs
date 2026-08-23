// SPDX-License-Identifier: Apache-2.0
//! Operation-header validation and record-boundary tests.

use super::*;

#[test]
fn unlabeled_operation_header_still_bounds_adjacent_records() {
    const HEADER: &[u8] = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff";
    let mut bytes = Vec::new();
    bytes.extend(HEADER);
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
