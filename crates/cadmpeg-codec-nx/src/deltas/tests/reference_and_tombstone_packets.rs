#[test]
fn deltas_tombstone_decodes_compact_and_extended_xmt_identities() {
    let compact = [0, 29, 0, 11, 0, 1];
    let extended = [0, 29, 0xe3, 0xbf, 0, 1];

    assert_eq!(crate::deltas::walk(&compact).tombstones[0].xmt, 11);
    assert_eq!(crate::deltas::walk(&extended).tombstones[0].xmt, 40_000);
}

#[test]
fn deltas_tombstone_is_self_delimiting_before_opaque_bytes() {
    let mut stream = vec![0, 29, 0, 11, 0, 1];
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].xmt, 11);
    assert_eq!(census.bytes_decoded, 6);
    assert_eq!(
        crate::deltas::semantic_residual(&stream),
        vec![0xff; 6]
            .into_iter()
            .chain([0xfe, 0xdc])
            .collect::<Vec<_>>()
    );
}

#[test]
fn deltas_body_revision_retains_prefix_identities_and_bounded_state_tail() {
    let mut bytes = vec![0, 12, 3, 0x10];
    bytes.extend_from_slice(&223u32.to_be_bytes());
    bytes.extend_from_slice(&[0xe3, 0xbf, 0, 1, 1]);
    for reference in [6u16, 1, 1, 1, 1, 1, 1] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    bytes.extend_from_slice(&[0x40, 0x8f, 0x40, 0, 0, 0, 0, 0]);

    let census = crate::deltas::walk(&bytes);

    assert!(census.records.is_empty());
    assert_eq!(census.body_revisions.len(), 1);
    assert_eq!(census.body_revisions[0].xmt, 784);
    assert_eq!(census.body_revisions[0].node_id, 223);
    assert_eq!(
        census.body_revisions[0].references,
        [40_000, 6, 1, 1, 1, 1, 1, 1]
    );
    assert_eq!(census.body_revisions[0].prefix_end, 34);
    assert_eq!(
        &bytes[census.body_revisions[0].prefix_end..census.body_revisions[0].end],
        [0x40, 0x8f, 0x40, 0, 0, 0, 0, 0]
    );
    assert_eq!(census.body_revisions[0].end, bytes.len());
    assert_eq!(census.bytes_decoded, bytes.len());
}

#[test]
fn deltas_reference_state_packets_decode_compact_and_extended_references() {
    let mut packet = vec![0, 1, 0, 1, 0, 4];
    packet.extend_from_slice(&2u16.to_be_bytes());
    packet.extend_from_slice(&3u16.to_be_bytes());
    packet.extend_from_slice(&[0xe3, 0xbf, 0, 1]);
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    for word in [34u32, 6, 11, 22_362, 1] {
        packet.extend_from_slice(&word.to_be_bytes());
    }
    packet.push(65);

    let census = crate::deltas::walk(&packet);

    assert_eq!(census.reference_state_packets.len(), 1);
    assert_eq!(
        census.reference_state_packets[0].frames,
        [crate::deltas::ReferenceStateFrame {
            references: [2, 3, 40_000, 1],
            state_words: [34, 6, 11, 22_362, 1],
            state_byte: 65,
        }]
    );
    assert!(!census.reference_state_packets[0].terminal);
    assert_eq!(census.reference_state_packets[0].offset, 0);
    assert_eq!(census.reference_state_packets[0].end, packet.len());
    assert_eq!(census.bytes_decoded, packet.len());

    let truncated = packet[..packet.len() - 1].to_vec();
    let null_required_reference = [&packet[..6], &[0, 1], &packet[8..]].concat();
    let trailing_byte = [packet.as_slice(), &[0]].concat();
    for malformed in [&truncated, &null_required_reference] {
        assert!(crate::deltas::walk(malformed)
            .reference_state_packets
            .is_empty());
    }
    let trailing_census = crate::deltas::walk(&trailing_byte);
    assert_eq!(trailing_census.reference_state_packets.len(), 1);
    assert_eq!(trailing_census.reference_state_packets[0].end, packet.len());
    assert_eq!(trailing_census.bytes_decoded, packet.len());

    let mut compound = vec![0, 1, 0, 1];
    for (references, words, state_byte) in [
        ([7u16, 1, 8, 1], [0u32; 5], 1),
        ([8, 7, 9, 1], [0, 0, 0, 17, 0], 2),
    ] {
        compound.extend_from_slice(&4u16.to_be_bytes());
        for reference in references {
            compound.extend_from_slice(&reference.to_be_bytes());
        }
        compound.extend_from_slice(&1u16.to_be_bytes());
        for word in words {
            compound.extend_from_slice(&word.to_be_bytes());
        }
        compound.push(state_byte);
    }
    for _ in 0..3 {
        compound.extend_from_slice(&1u16.to_be_bytes());
    }
    compound.extend_from_slice(&1u32.to_be_bytes());

    let compound_census = crate::deltas::walk(&compound);
    assert_eq!(compound_census.reference_state_packets.len(), 1);
    assert_eq!(compound_census.reference_state_packets[0].frames.len(), 2);
    assert!(compound_census.reference_state_packets[0].terminal);
    assert_eq!(
        compound_census.reference_state_packets[0].end,
        compound.len()
    );
    assert_eq!(compound_census.bytes_decoded, compound.len());
}

#[test]
fn deltas_reference_marker_packets_decode_extended_references_atomically() {
    let packet = [
        0xe3, 0xbf, 0x00, 0x01, 0x01, // extended reference 40_000, status
        0x00, 0x01, 0x01, // null reference, status
        0x56, // marker
        0x00, 0x01, 0x01, // null reference, status
    ];

    let census = crate::deltas::walk(&packet);

    assert_eq!(census.reference_marker_packets.len(), 1);
    assert_eq!(census.reference_marker_packets[0].reference, 40_000);
    assert_eq!(census.reference_marker_packets[0].marker, 0x56);
    assert_eq!(census.reference_marker_packets[0].offset, 0);
    assert_eq!(census.reference_marker_packets[0].end, packet.len());
    assert_eq!(census.bytes_decoded, packet.len());

    let truncated = packet[..packet.len() - 1].to_vec();
    let trailing_byte = [packet.as_slice(), &[0]].concat();
    let unknown_marker = [
        0xe3, 0xbf, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x55, 0x00, 0x01, 0x01,
    ]
    .to_vec();
    for malformed in [&truncated, &trailing_byte, &unknown_marker] {
        assert!(crate::deltas::walk(malformed)
            .reference_marker_packets
            .is_empty());
    }
}

#[test]
fn deltas_region_schema_declaration_exposes_a_following_marker_packet() {
    let mut bytes = vec![
        0x00, 0x13, 0x09, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x49, 0x05, 0x66, 0x72, 0x61, 0x6d,
        0x65, 0x00, 0xe6, 0x00, 0x01, 0x43, 0x41, 0x05, 0x6f, 0x77, 0x6e, 0x65, 0x72, 0x00, 0x0c,
        0x00, 0x01, 0x5a,
    ];
    bytes.extend_from_slice(&[0xe3, 0xbf, 0, 1]);
    bytes.extend_from_slice(&5u32.to_be_bytes());
    for reference in [1u16, 3, 1, 9] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    let declaration_end = bytes.len();
    bytes.extend([0, 7, 1, 0, 1, 1, 0x56, 0, 1, 1]);

    let census = crate::deltas::walk(&bytes);

    assert_eq!(census.inline_schema_declarations.len(), 1);
    let declaration = &census.inline_schema_declarations[0];
    assert_eq!(
        declaration.fields,
        crate::deltas::InlineSchemaFields::Region {
            xmt: 40_000,
            state_word: 5,
            references: [1, 3, 1, 9],
        }
    );
    assert_eq!(declaration.offset, 0);
    assert_eq!(declaration.end, declaration_end);
    assert_eq!(census.reference_marker_packets.len(), 1);
    assert_eq!(census.reference_marker_packets[0].offset, declaration_end);
    assert_eq!(census.reference_marker_packets[0].reference, 7);
    assert_eq!(census.bytes_decoded, bytes.len());

    let mut truncated = bytes[..declaration_end - 1].to_vec();
    truncated.extend([0, 7, 1, 0, 1, 1, 0x56, 0, 1, 1]);
    assert!(crate::deltas::walk(&truncated)
        .inline_schema_declarations
        .is_empty());
}

#[test]
fn deltas_body_revision_does_not_absorb_an_adjacent_tagged_reference_lane() {
    let mut bytes = vec![0, 12, 0, 3];
    bytes.extend_from_slice(&223u32.to_be_bytes());
    for reference in [2u16, 3, 4, 5, 6, 7, 8, 9] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    let lane_offset = bytes.len();
    bytes.extend_from_slice(&29u16.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());

    let census = crate::deltas::walk(&bytes);

    assert_eq!(census.body_revisions.len(), 1);
    assert_eq!(
        census.body_revisions[0].prefix_end,
        census.body_revisions[0].end
    );
    assert_eq!(census.body_revisions[0].end, lane_offset);
    assert_eq!(census.tagged_reference_lanes.len(), 1);
    assert_eq!(census.tagged_reference_lanes[0].offset, lane_offset);
    assert_eq!(census.tagged_reference_lanes[0].references, [(29, 10)]);
    assert_eq!(census.bytes_decoded, bytes.len());
}
