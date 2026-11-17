// SPDX-License-Identifier: Apache-2.0
//! Body references and the Compute option of rectangular-pattern
//! distribution blocks.

/// `distribution_payload` with `bodies` marked body references, eight zero
/// bytes and the Compute option `compute` in place of the twelve zero bytes
/// and u32 `2`.
fn body_payload(
    v_direction: Option<u32>,
    bodies: &[u32],
    compute: u32,
    distance_type: u8,
) -> Vec<u8> {
    let mut bytes = super::distribution_payload(0, v_direction, distance_type);
    let head = if v_direction.is_some() { 144 } else { 134 };
    let mut block = u32::try_from(bodies.len()).unwrap().to_le_bytes().to_vec();
    for body in bodies {
        super::push_marked_reference(&mut block, *body);
    }
    block.extend_from_slice(&[0; 8]);
    block.extend_from_slice(&compute.to_le_bytes());
    bytes.splice(head..head + 16, block);
    bytes
}

#[test]
fn every_body_count_and_compute_option_reads_the_distance_type() {
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
    for (v_direction, head) in [(None, 134), (Some(24), 144)] {
        for bodies in [&[][..], &[30], &[30, 31]] {
            for compute in 0..=2 {
                for distance_type in [0, 1] {
                    let bytes = body_payload(v_direction, bodies, compute, distance_type);
                    let at = head + 11 * bodies.len();
                    let distribution = parse(&bytes).unwrap_or_else(|| {
                        panic!("{v_direction:?} {bodies:?} {compute} {distance_type}")
                    });
                    assert_eq!(distribution.total_extent.value, distance_type == 1);
                    assert_eq!(
                        distribution.total_extent.offset,
                        u64::try_from(at + 24).unwrap()
                    );
                    assert_eq!(
                        (
                            distribution.compute_option.value,
                            distribution.compute_option.offset
                        ),
                        (compute, u64::try_from(at + 12).unwrap())
                    );
                }
            }
        }
    }
}

#[test]
fn a_distribution_block_with_a_misplaced_field_stays_unread() {
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
    let bytes = body_payload(None, &[30, 31], 1, 0);
    assert!(parse(&bytes).is_some());
    // The body count, the second reference mark, the last zero byte, a
    // Compute option of 3, and the sentinel.
    for (offset, value) in [(134, 3), (149, 2), (167, 1), (168, 3), (176, 0)] {
        let mut corrupt = bytes.clone();
        corrupt[offset] = value;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    // A body count beyond the payload.
    let mut corrupt = bytes.clone();
    corrupt[134..138].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse(&corrupt).is_none());
}

#[test]
fn one_body_reference_after_a_zero_byte_can_store_four_zero_bytes() {
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
    let four_zero = |v_direction: Option<u32>| {
        let mut bytes = body_payload(v_direction, &[30], 2, 1);
        let zeros = if v_direction.is_some() { 159 } else { 149 };
        bytes.drain(zeros..zeros + 4);
        bytes
    };
    let distribution = parse(&four_zero(None)).expect("four zero bytes after a zero byte");
    assert_eq!(
        (
            distribution.total_extent.value,
            distribution.total_extent.offset
        ),
        (true, 165)
    );
    assert_eq!(distribution.compute_option.value, 2);
    assert!(parse(&four_zero(Some(24))).is_none());
}

#[test]
fn a_distribution_block_records_its_seed_kind_and_unregistered_body_references() {
    let (scope, construction, direction) = super::distribution_inputs();
    let browser_node = crate::design::presentation::BROWSER_NODE_TYPE_GUID;
    let types = std::collections::HashMap::from([
        (30, (browser_node, 1)),
        (31, ("19031004-3AB6-4D13-ABBF-C51EE214688D", 1)),
    ]);
    let parse = |bytes: &[u8]| {
        super::super::exact_rectangular_pattern_distribution(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            &scope,
            &construction,
            &[direction],
            &types,
        )
        .unwrap()
    };
    for (seed_kind, bodies, untyped) in [
        (0, &[][..], 0),
        (1, &[30], 0),
        (1, &[30, 31], 1),
        (2, &[32], 1),
    ] {
        let mut bytes = body_payload(None, bodies, 1, 0);
        let at = 134 + 11 * bodies.len();
        bytes[at + 16..at + 20].copy_from_slice(&u32::to_le_bytes(seed_kind));
        let distribution = parse(&bytes);
        assert_eq!(
            (distribution.seed_kind.value, distribution.seed_kind.offset),
            (seed_kind, u64::try_from(at + 16).unwrap())
        );
        assert_eq!(
            (
                distribution.body_references.value,
                distribution.body_references.offset
            ),
            (u32::try_from(bodies.len()).unwrap(), 134)
        );
        assert_eq!(distribution.untyped_body_references, untyped, "{bodies:?}");
    }
}
