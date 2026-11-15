// SPDX-License-Identifier: Apache-2.0
//! Counted heads of rectangular-pattern distribution blocks that the counted
//! head test leaves out: u32 `1` after the head, and the head after a
//! V-direction reference.

/// `distribution_payload` with a counted head followed by u32 `1`: u32 `1`, a
/// marked reference, four zero bytes, u32 `1`, and a zero word in place of
/// the twelve zero bytes, u32 `2`, and zero word.
fn counted_one_payload(distance_type: u8) -> Vec<u8> {
    let mut bytes = super::distribution_payload(0, None, distance_type);
    let mut head = Vec::new();
    head.extend_from_slice(&1u32.to_le_bytes());
    super::push_marked_reference(&mut head, 30);
    head.extend_from_slice(&[0; 4]);
    head.extend_from_slice(&1u32.to_le_bytes());
    head.extend_from_slice(&0u32.to_le_bytes());
    bytes.splice(134..154, head);
    bytes
}

#[test]
fn a_counted_head_followed_by_u32_one_reads_the_distance_type() {
    let (scope, construction, direction) = super::distribution_inputs();
    let parse = |bytes: &[u8]| {
        super::super::exact_rectangular_pattern_distribution(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            &scope,
            &construction,
            &[direction],
        )
    };

    let spacing = parse(&counted_one_payload(0)).expect("counted spacing distribution");
    assert_eq!(
        (spacing.total_extent.value, spacing.total_extent.offset),
        (false, 165)
    );
    assert!(!spacing.symmetric.value);
    let extent = parse(&counted_one_payload(1)).expect("counted extent distribution");
    assert!(extent.total_extent.value);

    let bytes = counted_one_payload(0);
    // The slot count, the reference mark, the zero run, the fixed word, the
    // sentinel, the distance type, and the entry count.
    for (offset, value) in [
        (134, 2),
        (138, 2),
        (149, 1),
        (153, 3),
        (161, 0),
        (165, 2),
        (166, 3),
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset] = value;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    // After a V-direction reference, a counted head of four zero bytes stays unread.
    let mut referenced = super::distribution_payload(0, Some(24), 0);
    let head = bytes[134..170].to_vec();
    referenced.splice(144..173, head);
    assert!(parse(&referenced).is_none());
}

/// `distribution_payload` with a V-direction reference and a counted head
/// of eight zero bytes: u32 `1`, a marked reference, eight zero bytes, u32 `1`, and
/// a word in place of the twelve zero bytes, u32 `2`, and zero word.
fn referenced_counted_payload(distance_type: u8) -> Vec<u8> {
    let mut bytes = super::distribution_payload(0, Some(24), distance_type);
    let mut head = Vec::new();
    head.extend_from_slice(&1u32.to_le_bytes());
    super::push_marked_reference(&mut head, 30);
    head.extend_from_slice(&[0; 8]);
    head.extend_from_slice(&1u32.to_le_bytes());
    head.extend_from_slice(&1u32.to_le_bytes());
    bytes.splice(144..164, head);
    bytes
}

#[test]
fn a_counted_head_after_a_v_direction_reference_reads_the_distance_type() {
    let (scope, construction, direction) = super::distribution_inputs();
    let parse = |bytes: &[u8]| {
        super::super::exact_rectangular_pattern_distribution(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            &scope,
            &construction,
            &[direction],
        )
    };
    let spacing = parse(&referenced_counted_payload(0)).expect("referenced counted spacing");
    assert_eq!(
        (spacing.total_extent.value, spacing.total_extent.offset),
        (false, 179)
    );
    assert!(
        parse(&referenced_counted_payload(1))
            .expect("referenced counted extent")
            .total_extent
            .value
    );

    let bytes = referenced_counted_payload(0);
    // The slot count, the reference mark, the last byte of the eight-byte
    // zero run, the fixed word, the sentinel, the distance type, and the
    // entry count.
    for (offset, value) in [
        (144, 2),
        (148, 2),
        (166, 1),
        (167, 3),
        (175, 0),
        (179, 2),
        (180, 3),
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset] = value;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    // After a zero V-direction byte, a counted head of eight zero bytes stays unread.
    let mut unreferenced = super::distribution_payload(0, None, 0);
    unreferenced.splice(134..163, bytes[144..184].iter().copied());
    assert!(parse(&unreferenced).is_none());
}
