// SPDX-License-Identifier: Apache-2.0

#[test]
fn ambiguous_fixed_record_does_not_choose_an_identity_or_swallow_its_successor() {
    let mut stream = vec![0, 13, 0xff, 0, 9];
    stream.extend_from_slice(&29u32.to_be_bytes());
    for reference in [1_u16, 3, 1, 224, 1, 1, 83, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    let successor_offset = stream.len();
    stream.extend_from_slice(&[0, 19, 0, 13]);
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1_u16, 3, 83, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    assert!(super::super::fixed_layout(&stream, 0, 13, super::super::SHELL, 0).is_some());
    assert!(super::super::fixed_layout(&stream, 0, 13, super::super::SHELL, 1).is_some());
    let census = crate::test_support::with_decode_context(|ctx| {
        assert!(
            super::super::consume_fixed(ctx, &stream, 0, 13, super::super::SHELL, None)?.is_none()
        );
        crate::deltas::census::walk(ctx, &stream)
    })
    .unwrap();
    assert!(census.records.iter().all(|record| record.kind() != 13));
    let region = census
        .records
        .iter()
        .find(|record| record.kind() == 19)
        .unwrap();
    assert_eq!(region.offset, successor_offset);
    assert_eq!(region.xmt, 13);
    assert_eq!(region.family.node_id(), Some(7));
}

#[test]
fn ambiguous_fixed_payload_does_not_expose_a_tombstone_pattern() {
    let mut stream = vec![0, 13, 0xff, 0, 9];
    stream.extend_from_slice(&29u32.to_be_bytes());
    for reference in [1_u16, 3, 15, 256, 1, 1, 83, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    let successor_offset = stream.len();
    stream.extend_from_slice(&[0, 19, 0, 13]);
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1_u16, 3, 83, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    assert!(super::super::fixed_layout(&stream, 0, 13, super::super::SHELL, 0).is_some());
    assert!(super::super::fixed_layout(&stream, 0, 13, super::super::SHELL, 1).is_some());
    assert_eq!(super::super::compact_tombstone(&stream, 15), Some(257));
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    assert!(census.records.iter().all(|record| record.kind() != 13));
    assert!(census.tombstones.is_empty());
    let region = census
        .records
        .iter()
        .find(|record| record.kind() == 19)
        .unwrap();
    assert_eq!(region.offset, successor_offset);
}

#[test]
fn base_intersection_does_not_consume_its_successors_type_tag() {
    let mut stream = b"PS".to_vec();
    for text in [": TRANSMIT FILE (deltas) ", "SCH_1_1_13006"] {
        stream.extend_from_slice(&u32::try_from(text.len()).unwrap().to_be_bytes());
        stream.extend_from_slice(text.as_bytes());
    }
    stream.extend_from_slice(&141u16.to_be_bytes());
    stream.extend_from_slice(&0u32.to_be_bytes());
    let intersection_offset = stream.len();
    stream.extend_from_slice(&[0, 38, 0xff, 0, 100]);
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1_u16, 3, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [6_u16, 7, 20, 21, 22] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(0);
    }
    let successor_offset = stream.len();
    stream.extend_from_slice(&[0, 41, 0, 0, 0, 1, 0, 21, b'L']);
    stream.extend_from_slice(&[0; 88]);
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    let record = census.records.iter().find(|r| r.kind() == 38).unwrap();
    assert_eq!(record.offset, intersection_offset);
    assert_eq!(record.end, successor_offset);
    assert_eq!(
        record.family.references(),
        [1, 3, 1, 1, 1, 6, 7, 20, 21, 22]
    );
}

#[test]
fn type_45_declared_array_does_not_claim_a_finite_successor_word() {
    let mut stream = vec![0, 45, 0, 0, 0, 1, 0, 9];
    stream.extend_from_slice(&2.0f64.to_be_bytes());
    let end = stream.len();
    // A normal binary64 successor cannot add an element to the primary
    // schema's declared one-element array.
    stream.extend_from_slice(&3.0f64.to_be_bytes());
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    let record = census.records.iter().find(|r| r.kind() == 45).unwrap();
    assert_eq!(record.end, end);
    assert_eq!(record.canonical_bytes, stream[..end]);
}
