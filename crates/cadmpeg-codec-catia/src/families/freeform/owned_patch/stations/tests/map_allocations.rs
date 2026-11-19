// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::wire::records::consolidated_records;

#[test]
fn owned_class24_map_preserves_target_and_source_domains() {
    let frame = |class: u8, payload: &[u8]| {
        let mut bytes = vec![
            0xb2,
            3,
            class,
            u8::try_from(payload.len()).expect("bounded payload"),
            5,
        ];
        bytes.extend_from_slice(payload);
        bytes
    };
    let mut mapping = vec![0x82, 3, 7, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 2.0, 0.0001, 0.0, 1.0, 2.0, 0.0, 0.0001] {
        mapping.extend(value.to_le_bytes());
    }
    let mut line = vec![5, 2];
    for value in [0.0_f64, 0.0, 1.0] {
        line.extend(value.to_le_bytes());
    }
    let mut bytes = frame(0x24, &mapping);
    bytes.extend(frame(0x18, &line));
    bytes.extend(frame(0x18, &line));
    let records = consolidated_records(&bytes);
    let resolve = |index: usize, reference: AllocationReference| {
        if reference.encoding == AllocationReferenceEncoding::OwnedChild {
            index
                .checked_add(1)?
                .checked_add(usize::try_from(reference.value).ok()?)
        } else {
            None
        }
    };
    assert_eq!(
        mapped_range(&bytes, &records, 0, 1, &resolve),
        Some(([0.0, 2.0], true))
    );
    assert_eq!(
        mapped_range(&bytes, &records, 0, 2, &resolve),
        Some(([0.0, 1.0], true))
    );
}
