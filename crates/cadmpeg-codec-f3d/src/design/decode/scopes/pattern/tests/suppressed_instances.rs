// SPDX-License-Identifier: Apache-2.0
//! Suppressed-instance keys in the suffix of a rectangular-pattern payload.

/// `distribution_payload` whose suffix stores the count of `keys` and `keys`.
fn suppressed_payload(keys: &[u32]) -> Vec<u8> {
    let mut bytes = super::distribution_payload(0, None, 0);
    // The suppressed count is the four bytes before the eleven-byte header.
    let zeros = bytes.len() - 11 - 4;
    let mut list = u32::try_from(keys.len()).unwrap().to_le_bytes().to_vec();
    for key in keys {
        list.extend_from_slice(&key.to_le_bytes());
    }
    bytes.splice(zeros..zeros + 4, list);
    bytes
}

#[test]
fn a_payload_suffix_reads_its_suppressed_instance_keys() {
    let (scope, construction, direction) = super::distribution_inputs();
    let parse = |bytes: &[u8]| {
        super::super::exact_rectangular_pattern_distribution(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            &scope,
            &construction,
            &[direction],
            &std::collections::HashMap::new(),
        )
    };
    let none = parse(&suppressed_payload(&[])).expect("no suppressed instance");
    assert!(none.suppressed.value.is_empty());
    // Positions (1, 0), (1, 1) and (3, 2) of the 5 by 3 grid.
    let bytes = suppressed_payload(&[3, 5, 19]);
    let distribution = parse(&bytes).expect("three suppressed instances");
    assert_eq!(distribution.suppressed.value, [3, 5, 19]);
    assert_eq!(
        distribution.suppressed.offset,
        u64::try_from(bytes.len() - 11 - 16).unwrap()
    );
    // The seed, a repeated key, a position past direction one's count, a
    // position past direction two's count, and key zero.
    for keys in [&[1][..], &[3, 3], &[21], &[7], &[0]] {
        assert!(parse(&suppressed_payload(keys)).is_none(), "{keys:?}");
    }
}
