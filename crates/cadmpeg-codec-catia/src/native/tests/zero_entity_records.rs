// SPDX-License-Identifier: Apache-2.0
//! Resource admission for zero-entity native records.

#[test]
fn zero_entity_scan_finds_one_a9_record() {
    let bytes = [0xa9, 0x03, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
    let run = || super::super::zero_entity_records(&bytes, 0..bytes.len());

    assert_eq!(run().len(), 1);
}
