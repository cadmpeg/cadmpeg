// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::consolidated_records;

fn graph(fitted: bool, duplicate: bool) -> Vec<u8> {
    let frame = |kind, body: Vec<u8>| {
        let mut bytes = vec![
            0xb2,
            3,
            kind,
            u8::try_from(body.len()).expect("small record"),
            5,
        ];
        bytes.extend(body);
        bytes
    };
    let mut line = vec![5, 2];
    for v in [0.0_f64, 0.0, 1.0] {
        line.extend(v.to_le_bytes());
    }
    let mut bytes = frame(0x18, line.clone());
    bytes.extend(frame(0x18, line));
    let mut map = vec![
        0x82,
        9,
        if duplicate { 9 } else { 5 },
        if fitted { 0x0a } else { 0x0b },
        0x87,
        0x0d,
    ];
    for v in [
        0.0_f64,
        1.0,
        if fitted { 0.001 } else { 0.0 },
        0.0,
        1.0,
        1.0,
        0.0,
    ] {
        map.extend(v.to_le_bytes());
    }
    if fitted {
        map.extend(0.004_f64.to_le_bytes());
    }
    bytes.extend(frame(0x25, map));
    bytes
}

#[test]
fn native_map_accuracy_requires_a_fitted_relation_and_unique_operands() {
    let resolve = |index: usize, reference: AllocationReference| {
        let value = usize::try_from(reference.value).ok()?;
        match reference.encoding {
            AllocationReferenceEncoding::BackwardDistance => index.checked_sub(value),
            AllocationReferenceEncoding::OwnedChild => index.checked_add(1)?.checked_add(value),
            _ => None,
        }
    };
    for (fitted, duplicate, expected) in [
        (true, false, Some(0.004)),
        (false, false, None),
        (true, true, None),
    ] {
        let bytes = graph(fitted, duplicate);
        let records = consolidated_records(&bytes);
        assert_eq!(records.len(), 3);
        assert_eq!(
            mapped_fit_tolerance(&bytes, &records, 2, 0, &resolve),
            expected
        );
        assert_eq!(
            mapped_fit_tolerance(&bytes, &records, 2, 77, &resolve),
            None
        );
    }
}
