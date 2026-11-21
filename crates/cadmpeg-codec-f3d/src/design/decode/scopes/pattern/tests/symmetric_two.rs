// SPDX-License-Identifier: Apache-2.0
//! The symmetric byte of direction two in a rectangular-pattern payload.

use super::{distribution_inputs, distribution_payload};

#[test]
fn rectangular_pattern_payload_distribution_reads_the_symmetric_byte_of_each_direction() {
    let (scope, construction, direction) = distribution_inputs();
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
    for (one, two) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let mut bytes = distribution_payload(one, None, 0);
        bytes[110] = two;
        let distribution = parse(&bytes).expect("distribution");
        assert_eq!(
            (distribution.symmetric.value, distribution.symmetric.offset),
            (one == 1, 109)
        );
        assert_eq!(
            (
                distribution.symmetric_two.value,
                distribution.symmetric_two.offset
            ),
            (two == 1, 110)
        );
    }
    let mut bytes = distribution_payload(0, None, 0);
    bytes[110] = 2;
    assert!(parse(&bytes).is_none());
}
