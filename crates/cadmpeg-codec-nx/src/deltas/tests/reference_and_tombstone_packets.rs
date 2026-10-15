#[test]
fn deltas_tombstone_decodes_compact_and_extended_xmt_identities() {
    let compact = [0, 29, 0, 11, 0, 1];
    let extended = [0, 29, 0xe3, 0xbf, 0, 1];

    assert_eq!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &compact))
            .unwrap()
            .tombstones[0]
            .xmt,
        11
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &extended))
            .unwrap()
            .tombstones[0]
            .xmt,
        40_000
    );
}

#[test]
fn deltas_tombstone_is_self_delimiting_before_opaque_bytes() {
    let mut stream = vec![0, 29, 0, 11, 0, 1];
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &stream))
            .unwrap();
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].xmt, 11);
    assert_eq!(census.bytes_decoded(), 6);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::semantic_residual(
            ctx, &stream
        ))
        .unwrap(),
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

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &bytes))
            .unwrap();

    assert!(census.records.is_empty());
    assert_eq!(census.body_revisions.len(), 1);
    assert_eq!(u32::from(census.body_revisions[0].xmt), 784);
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
    assert_eq!(census.bytes_decoded(), bytes.len());
}

#[test]
fn deltas_pmark_packets_decode_primary_fields_with_partition_context() {
    use crate::test_support::test_pmarks;
    let prefix = test_pmarks::prefix(40_000);
    let mut bytes = prefix.clone();
    bytes.extend(test_pmarks::packet());
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &bytes))
            .unwrap();
    assert_eq!(census.pmark_packets.len(), 1);
    let packet = &census.pmark_packets[0];
    assert_eq!(packet.nodes.as_slice().len(), 1);
    let node = packet.nodes.as_slice()[0];
    assert_eq!(node.xmt(), 40_000);
    assert_eq!(node.preceding(), 1);
    assert_eq!(node.first_following(), 49_948);
    assert_eq!(node.identifier(), 0x1234_5677);
    assert!(!packet.terminal);
    assert_eq!(packet.offset, prefix.len());
    assert_eq!(packet.end, bytes.len());
    assert_eq!(census.bytes_decoded(), bytes.len());
    for malformed in [test_pmarks::packet(), bytes[..bytes.len() - 1].to_vec()] {
        let census = crate::test_support::with_decode_context(|ctx| {
            crate::deltas::census::walk(ctx, &malformed)
        })
        .unwrap();
        assert!(census.pmark_packets.is_empty());
    }
}

#[test]
fn deltas_reference_marker_packets_decode_extended_references_atomically() {
    let packet = [
        0xe3, 0xbf, 0x00, 0x01, 0x01, // extended reference 40_000, status
        0x00, 0x01, 0x01, // null reference, status
        0x56, // marker
        0x00, 0x01, 0x01, // null reference, status
    ];

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &packet))
            .unwrap();

    assert_eq!(census.reference_marker_packets.len(), 1);
    assert_eq!(
        u32::from(census.reference_marker_packets[0].reference),
        40_000
    );
    assert_eq!(u8::from(census.reference_marker_packets[0].marker), 0x56);
    assert_eq!(census.reference_marker_packets[0].offset, 0);
    assert_eq!(census.reference_marker_packets[0].end, packet.len());
    assert_eq!(census.bytes_decoded(), packet.len());

    let truncated = packet[..packet.len() - 1].to_vec();
    let trailing_byte = [packet.as_slice(), &[0]].concat();
    let unknown_marker = [
        0xe3, 0xbf, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x55, 0x00, 0x01, 0x01,
    ]
    .to_vec();
    for malformed in [&truncated, &trailing_byte, &unknown_marker] {
        assert!(
            crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
                ctx, malformed
            ))
            .unwrap()
            .reference_marker_packets
            .is_empty()
        );
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
    for reference in [10u16, 11] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(0);
    }
    bytes.push(b'S');
    bytes.extend_from_slice(&[0, 3, 1]);
    let declaration_end = bytes.len();
    bytes.extend([0, 7, 1, 0, 1, 1, 0x56, 0, 1, 1]);

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &bytes))
            .unwrap();

    assert_eq!(census.schema_nodes.len(), 1);
    let declaration = &census.schema_nodes[0];
    let fields = serde_json::to_value(&declaration.fields).unwrap();
    assert_eq!(fields["definition"]["kind"], 19);
    assert_eq!(fields["definition"]["fields"].as_array().unwrap().len(), 9);
    assert_eq!(fields["schema_byte_len"], 33);
    assert_eq!(fields["xmt"], 40_000);
    assert_eq!(
        fields["values"][0],
        serde_json::json!([{"type":"integer","value":5}])
    );
    assert_eq!(
        fields["values"][7],
        serde_json::json!([{"type":"char","value":83}])
    );
    assert_eq!(declaration.offset, 0);
    assert_eq!(declaration.end, declaration_end);
    assert_eq!(census.reference_marker_packets.len(), 1);
    assert_eq!(census.reference_marker_packets[0].offset, declaration_end);
    assert_eq!(u32::from(census.reference_marker_packets[0].reference), 7);
    assert_eq!(census.bytes_decoded(), bytes.len());

    // A following zero could itself complete the final pointer status. Test
    // the genuinely incomplete input, without supplying replacement storage.
    let truncated = bytes[..declaration_end - 1].to_vec();
    assert!(
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(
            ctx, &truncated
        ))
        .unwrap()
        .schema_nodes
        .is_empty()
    );
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

    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &bytes))
            .unwrap();

    assert_eq!(census.body_revisions.len(), 1);
    assert_eq!(
        census.body_revisions[0].prefix_end,
        census.body_revisions[0].end
    );
    assert_eq!(census.body_revisions[0].end, lane_offset);
    assert_eq!(census.tagged_reference_lanes.len(), 1);
    assert_eq!(census.tagged_reference_lanes[0].offset, lane_offset);
    assert_eq!(
        Vec::<(u16, u32)>::from(census.tagged_reference_lanes[0].references.clone()),
        [(29, 10)]
    );
    assert_eq!(census.bytes_decoded(), bytes.len());
}
