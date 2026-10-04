// SPDX-License-Identifier: Apache-2.0
use crate::parasolid::Stream;
use cadmpeg_core::CodecError;

use crate::deltas::inline_schema_fields::InlineSchemaFields;

fn deltas_type_45(xmt: u16) -> Vec<u8> {
    let mut bytes = 45u16.to_be_bytes().to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&xmt.to_be_bytes());
    bytes.extend_from_slice(&1.25f64.to_be_bytes());
    bytes.extend_from_slice(&2.5f64.to_be_bytes());
    bytes
}

fn deltas_event_limit_error(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> CodecError {
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: deltas_type_45(10),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    crate::test_support::with_decode_context_over(
        &streams[0].inflated,
        |_| {},
        |scan_ctx| {
            let census = crate::deltas::census::walk(scan_ctx, &streams[0].inflated).unwrap();

            crate::test_support::with_decode_context_over(
                &[],
                |policy| {
                    configure(policy);
                },
                |ctx| {
                    crate::native::parasolid::parasolid_deltas_events_with_censuses(
                        ctx,
                        &streams,
                        vec![Some(census)],
                    )
                    .err()
                    .expect("deltas event limit refusal")
                },
            )
        },
    )
}

#[test]
fn deltas_event_route_refuses_collection_limit() {
    let error = deltas_event_limit_error(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn deltas_event_route_refuses_retained_limit() {
    let error = deltas_event_limit_error(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn deltas_event_route_refuses_scoped_limit() {
    let error = deltas_event_limit_error(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn deltas_event_route_refuses_work_limit() {
    let error = deltas_event_limit_error(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn deltas_events_retain_bounded_records_tombstones_and_revisions() {
    let mut bytes = vec![0xaa, 0xbb, 0xcc, 0, 12, 0, 3];
    bytes.extend_from_slice(&9u32.to_be_bytes());
    for reference in [2u16, 3, 4, 5, 6, 7, 8, 9] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    let revision_prefix_end = bytes.len();
    let revision_state_tail = [0xde, 0xad, 0xbe, 0xef];
    bytes.extend_from_slice(&revision_state_tail);
    let type_45_offset = bytes.len();
    bytes.extend_from_slice(&45u16.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&1.25f64.to_be_bytes());
    bytes.extend_from_slice(&2.5f64.to_be_bytes());
    bytes.extend_from_slice(&[0xdd, 0xee]);
    let tombstone_offset = bytes.len();
    bytes.extend_from_slice(&[0, 29, 0, 11, 0, 1]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes,
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let census = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::census::walk(ctx, &streams[0].inflated)
    })
    .unwrap();
    let events = crate::test_support::with_decode_context(|ctx| {
        crate::native::parasolid::parasolid_deltas_events_with_censuses(
            ctx,
            &streams,
            vec![Some(census)],
        )
    })
    .unwrap();

    assert_eq!(events.body_revisions.len(), 1);
    assert_eq!(u32::from(events.body_revisions[0].xmt), 3);
    assert_eq!(events.body_revisions[0].node_id, 9);
    assert_eq!(
        events.body_revisions[0].references,
        [2, 3, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(events.body_revisions[0].lengths.prefix(), 32);
    assert_eq!(events.body_revisions[0].lengths.tail(), 4);
    assert_eq!(events.body_revisions[0].lengths.total(), 36);
    assert_eq!(
        events.body_revisions[0].state_tail_sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&revision_state_tail)
    );
    assert_eq!(
        events.body_revisions[0].inflated_offset + events.body_revisions[0].lengths.prefix(),
        cadmpeg_core::decode::u64_from_index(revision_prefix_end)
    );
    assert_eq!(events.records.len(), 1);
    assert_eq!(events.records[0].family.family_name(), "TYPE_45");
    assert_eq!(events.records[0].xmt, 10);
    assert_eq!(
        events.records[0].inflated_offset,
        cadmpeg_core::decode::u64_from_index(type_45_offset)
    );
    assert_eq!(events.records[0].byte_len, 24);
    assert_eq!(events.tombstones.len(), 1);
    assert_eq!(events.tombstones[0].kind.name(), "POINT");
    assert_eq!(events.tombstones[0].xmt, 11);
    assert_eq!(
        serde_json::to_value(&events.tombstones[0]).unwrap()["byte_len"],
        6
    );
    assert_eq!(
        events.tombstones[0].inflated_offset,
        cadmpeg_core::decode::u64_from_index(tombstone_offset)
    );
    assert_eq!(events.residual_spans.len(), 2);
    assert_eq!(events.residual_spans[0].inflated_offset, 0);
    assert_eq!(events.residual_spans[0].byte_len, 3);
    assert_eq!(
        events.residual_spans[0].sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0xaa, 0xbb, 0xcc])
    );
    assert_eq!(
        events.residual_spans[1].inflated_offset,
        cadmpeg_core::decode::u64_from_index(type_45_offset + 24)
    );
    assert_eq!(events.residual_spans[1].byte_len, 2);
}

#[test]
fn deltas_events_subtract_typed_term_use_numeric_tails_from_residuals() {
    let mut bytes = [0xaa, 0xbb].to_vec();
    bytes.extend_from_slice(&41u16.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&20u16.to_be_bytes());
    bytes.extend_from_slice(b"L?");
    for coordinate in [1.0f64, 2.0, 3.0] {
        bytes.extend_from_slice(&coordinate.to_be_bytes());
    }
    let tail_offset = bytes.len();
    for ordinal in 0..8 {
        bytes.extend_from_slice(&(f64::from(ordinal) + 0.5).to_be_bytes());
    }
    bytes.extend_from_slice(&[0xcc, 0xdd, 0xee]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes,
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.term_use_numeric_tails.len(), 1);
    let tail = &events.term_use_numeric_tails[0];
    assert_eq!(tail.term_use_xmt, 20);
    assert_eq!(tail.values.term_use_count(), 1);
    assert_eq!(tail.values.values().len(), 8);
    assert_eq!(tail.values.byte_len(), 64);
    assert_eq!(
        tail.inflated_offset,
        cadmpeg_core::decode::u64_from_index(tail_offset)
    );
    assert_eq!(events.residual_spans.len(), 2);
    assert_eq!(events.residual_spans[0].byte_len, 2);
    assert_eq!(
        events.residual_spans[1].inflated_offset,
        cadmpeg_core::decode::u64_from_index(tail_offset + 64)
    );
    assert_eq!(events.residual_spans[1].byte_len, 3);
}

#[test]
fn deltas_events_subtract_tagged_reference_lanes_from_residuals() {
    let mut bytes = [0xaa, 0xbb, 0xcc].to_vec();
    bytes.extend(deltas_type_45(10));
    let lane_offset = bytes.len();
    bytes.extend([
        0x00, 0x4f, 0x00, 0x0a, // direct type-79 reference
        0x00, 0x50, 0xff, 0xff, 0x00, 0x01, // extended type-80 reference
    ]);
    let lane_end = bytes.len();
    bytes.extend(deltas_type_45(11));
    let suffix_offset = bytes.len();
    bytes.extend([0xdd, 0xee]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.tagged_reference_lanes.len(), 1);
    let lane = &events.tagged_reference_lanes[0];
    assert_eq!(
        Vec::<(u16, u32)>::from(lane.references.clone()),
        [(79, 10), (80, 32_768)]
    );
    assert_eq!(lane.byte_len, 10);
    assert_eq!(
        lane.inflated_offset,
        cadmpeg_core::decode::u64_from_index(lane_offset)
    );
    assert_eq!(
        lane.sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[lane_offset..lane_end])
    );
    assert_eq!(events.residual_spans.len(), 2);
    assert_eq!(events.residual_spans[0].inflated_offset, 0);
    assert_eq!(events.residual_spans[0].byte_len, 3);
    assert_eq!(
        events.residual_spans[1].inflated_offset,
        cadmpeg_core::decode::u64_from_index(suffix_offset)
    );
    assert_eq!(events.residual_spans[1].byte_len, 2);
}

#[test]
fn deltas_events_subtract_transmit_headers_from_residuals() {
    let description = b": TRANSMIT FILE (deltas) created by modeller version 3501171";
    let schema = b"SCH_3501171_35102_13006";
    let mut bytes = b"PS".to_vec();
    bytes.extend_from_slice(
        &(u32::try_from(description.len()).expect("fixture value fits u32")).to_be_bytes(),
    );
    bytes.extend_from_slice(description);
    bytes.extend_from_slice(
        &(u32::try_from(schema.len()).expect("fixture value fits u32")).to_be_bytes(),
    );
    bytes.extend_from_slice(schema);
    bytes.extend_from_slice(&[
        0, 0xe7, 0, 0, 0, 0, 0, 3, 0xff, 0x04, 0x27, 0x04, 0x28, 0, 0,
    ]);
    let header_end = bytes.len();
    bytes.extend([0xaa, 0xbb]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: Some(
                cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_3501171_35102_13006")
                    .expect("the fixture text is a schema token"),
            ),
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.transmit_headers.len(), 1);
    let header = &events.transmit_headers[0];
    assert_eq!(header.id, "nx:s0:deltas-transmit-header#0");
    assert_eq!(header.state.description().as_bytes(), description);
    assert_eq!(header.state.schema().as_bytes(), schema);
    assert_eq!(header.state.references(), [1063, 1064]);
    assert_eq!(
        header.byte_len,
        cadmpeg_core::decode::u64_from_index(header_end)
    );
    assert_eq!(
        header.sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[..header_end])
    );
    assert_eq!(events.residual_spans.len(), 1);
    assert_eq!(
        events.residual_spans[0].inflated_offset,
        cadmpeg_core::decode::u64_from_index(header_end)
    );
    assert_eq!(events.residual_spans[0].byte_len, 2);
}

#[test]
fn deltas_events_retain_terminal_null_references() {
    let mut bytes = [0xaa, 0xbb].to_vec();
    let trailer_offset = bytes.len();
    bytes.extend_from_slice(&[0, 1, 0, 1, 0, 1, 0, 1]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.terminal_null_references.len(), 1);
    let trailer = &events.terminal_null_references[0];
    assert_eq!(trailer.form.references(), [1; 4]);
    assert_eq!(trailer.form.raw().len(), 8);
    assert_eq!(
        trailer.inflated_offset,
        cadmpeg_core::decode::u64_from_index(trailer_offset)
    );
    assert_eq!(
        serde_json::to_value(trailer).unwrap()["sha256"],
        serde_json::json!(
            cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[trailer_offset..]).as_str()
        )
    );
    assert_eq!(events.residual_spans.len(), 1);
    assert_eq!(events.residual_spans[0].inflated_offset, 0);
    assert_eq!(
        events.residual_spans[0].byte_len,
        cadmpeg_core::decode::u64_from_index(trailer_offset)
    );
}

#[test]
fn deltas_events_subtract_reference_type_maps_from_residuals() {
    let mut bytes = [0xaa, 0xbb].to_vec();
    bytes.extend(deltas_type_45(10));
    let map_offset = bytes.len();
    bytes.extend([
        0, 1, 0, 1, 0xe3, 0xbf, 0, 1, 0, 81, 0, 3, 0, 100, 0, 1, 0, 0, 0, 55,
    ]);
    let map_end = bytes.len();
    bytes.extend(deltas_type_45(11));
    let suffix_offset = bytes.len();
    bytes.extend([0xcc, 0xdd]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.reference_type_maps.len(), 1);
    let map = &events.reference_type_maps[0];
    assert_eq!(
        Vec::<(u32, u16)>::from(map.entries.clone()),
        [(40_000, 81), (3, 100)]
    );
    assert_eq!(map.target_kind.map(std::num::NonZeroU16::get), Some(55));
    assert_eq!(map.byte_len, 20);
    assert_eq!(
        map.inflated_offset,
        cadmpeg_core::decode::u64_from_index(map_offset)
    );
    assert_eq!(
        map.sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[map_offset..map_end])
    );
    assert_eq!(events.residual_spans.len(), 2);
    assert_eq!(events.residual_spans[0].byte_len, 2);
    assert_eq!(
        events.residual_spans[1].inflated_offset,
        cadmpeg_core::decode::u64_from_index(suffix_offset)
    );
    assert_eq!(events.residual_spans[1].byte_len, 2);
}

#[test]
fn deltas_events_subtract_reference_state_packets_from_residuals() {
    let mut bytes = [0xaa, 0xbb].to_vec();
    bytes.extend(deltas_type_45(10));
    let packet_offset = bytes.len();
    bytes.extend([0, 1, 0, 1, 0, 4]);
    for reference in [2u16, 3, 4, 1] {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&1u16.to_be_bytes());
    for word in [34u32, 6, 11, 22_362, 1] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    bytes.push(65);
    let packet_end = bytes.len();
    bytes.extend(deltas_type_45(11));
    let suffix_offset = bytes.len();
    bytes.extend([0xcc, 0xdd, 0xee]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.reference_state_packets.len(), 1);
    let packet = &events.reference_state_packets[0];
    assert_eq!(
        packet.frames.as_slice(),
        [crate::deltas::state_frame::ReferenceStateFrame {
            references: [2, 3, 4, 1].try_into().unwrap(),
            state_words: [34, 6, 11, 22_362, 1],
            state_byte: 65,
        }]
    );
    assert!(!packet.terminal);
    assert_eq!(packet.byte_len, 37);
    assert_eq!(
        packet.inflated_offset,
        cadmpeg_core::decode::u64_from_index(packet_offset)
    );
    assert_eq!(
        packet.sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[packet_offset..packet_end])
    );
    assert_eq!(events.residual_spans.len(), 2);
    assert_eq!(events.residual_spans[0].byte_len, 2);
    assert_eq!(
        events.residual_spans[1].inflated_offset,
        cadmpeg_core::decode::u64_from_index(suffix_offset)
    );
    assert_eq!(events.residual_spans[1].byte_len, 3);
}

#[test]
fn deltas_events_retain_schema_reference_preambles() {
    let mut bytes = [0xaa, 0xbb].to_vec();
    bytes.extend(deltas_type_45(10));
    let preamble_offset = bytes.len();
    bytes.extend_from_slice(&300u16.to_be_bytes());
    bytes.extend_from_slice(&4u16.to_be_bytes());
    bytes.push(0xff);
    for reference in [2u16, 3, 1, 1, 1] {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    for state_word in [2u32, 0, 1, 55] {
        bytes.extend_from_slice(&state_word.to_be_bytes());
    }
    bytes.extend_from_slice(&[0, 0, 0]);
    bytes.extend_from_slice(&300u16.to_be_bytes());
    for reference in [1u16, 1] {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&5u16.to_be_bytes());
    for (kind, reference) in [(81u16, 4u16), (82, 5), (81, 6)] {
        bytes.extend_from_slice(&kind.to_be_bytes());
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&82u16.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&9u16.to_be_bytes());
    let preamble_end = bytes.len();
    bytes.extend(deltas_type_45(11));
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.schema_reference_preambles.len(), 1);
    let preamble = &events.schema_reference_preambles[0];
    assert_eq!(preamble.state.identity(), 300);
    assert_eq!(preamble.state.references(), [2, 3]);
    assert_eq!(preamble.state.state_reference(), None);
    assert_eq!(preamble.state.state_words(), [2, 0, 1, 55]);
    assert_eq!(preamble.state.count(), 5);
    assert_eq!(preamble.state.entries(), [(81, 4), (82, 5), (81, 6)]);
    assert_eq!(preamble.state.terminal_value(), 9);
    assert_eq!(
        preamble.inflated_offset,
        cadmpeg_core::decode::u64_from_index(preamble_offset)
    );
    assert_eq!(
        preamble.byte_len,
        cadmpeg_core::decode::u64_from_index(preamble_end - preamble_offset)
    );
    assert_eq!(
        preamble.sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[preamble_offset..preamble_end])
    );
}

#[test]
fn deltas_events_subtract_reference_marker_packets_from_residuals() {
    let mut bytes = [0xaa, 0xbb].to_vec();
    bytes.extend(deltas_type_45(10));
    let packet_offset = bytes.len();
    bytes.extend([0, 9, 1, 0, 1, 1, 0x53, 0, 1, 1]);
    let packet_end = bytes.len();
    bytes.extend(deltas_type_45(11));
    let suffix_offset = bytes.len();
    bytes.extend([0xcc, 0xdd]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.reference_marker_packets.len(), 1);
    let packet = &events.reference_marker_packets[0];
    assert_eq!(u32::from(packet.reference), 9);
    assert_eq!(u8::from(packet.marker), 0x53);
    assert_eq!(packet.byte_len, 10);
    assert_eq!(
        packet.inflated_offset,
        cadmpeg_core::decode::u64_from_index(packet_offset)
    );
    assert_eq!(
        packet.sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[packet_offset..packet_end])
    );
    assert_eq!(events.residual_spans.len(), 2);
    assert_eq!(events.residual_spans[0].byte_len, 2);
    assert_eq!(
        events.residual_spans[1].inflated_offset,
        cadmpeg_core::decode::u64_from_index(suffix_offset)
    );
    assert_eq!(events.residual_spans[1].byte_len, 2);
}

#[test]
fn deltas_events_subtract_inline_schema_declarations_from_residuals() {
    let mut bytes = [0xaa, 0xbb].to_vec();
    bytes.extend(deltas_type_45(10));
    let declaration_offset = bytes.len();
    bytes.extend([
        0x00, 0x13, 0x09, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x49, 0x05, 0x66, 0x72, 0x61, 0x6d,
        0x65, 0x00, 0xe6, 0x00, 0x01, 0x43, 0x41, 0x05, 0x6f, 0x77, 0x6e, 0x65, 0x72, 0x00, 0x0c,
        0x00, 0x01, 0x5a,
    ]);
    bytes.extend_from_slice(&11u16.to_be_bytes());
    bytes.extend_from_slice(&5u32.to_be_bytes());
    for reference in [1u16, 3, 1, 9] {
        bytes.extend_from_slice(&reference.to_be_bytes());
        bytes.push(1);
    }
    let declaration_end = bytes.len();
    bytes.extend(deltas_type_45(11));
    let suffix_offset = bytes.len();
    bytes.extend([0xcc, 0xdd]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.inline_schema_declarations.len(), 1);
    let declaration = &events.inline_schema_declarations[0];
    assert_eq!(
        declaration.fields,
        InlineSchemaFields::Region {
            xmt: 11u32.try_into().unwrap(),
            state_word: 5,
            references: [1, 3, 1, 9],
        }
    );
    assert_eq!(declaration.byte_len, 51);
    assert_eq!(
        declaration.inflated_offset,
        cadmpeg_core::decode::u64_from_index(declaration_offset)
    );
    assert_eq!(
        declaration.sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[declaration_offset..declaration_end])
    );
    assert_eq!(events.residual_spans.len(), 2);
    assert_eq!(events.residual_spans[0].byte_len, 2);
    assert_eq!(
        events.residual_spans[1].inflated_offset,
        cadmpeg_core::decode::u64_from_index(suffix_offset)
    );
    assert_eq!(events.residual_spans[1].byte_len, 2);
}

#[test]
fn deltas_events_subtract_type_150_state_packets_from_residuals() {
    let mut bytes = [0xaa, 0xbb].to_vec();
    bytes.extend(deltas_type_45(10));
    let packet_offset = bytes.len();
    bytes.push(150);
    for (reference, status) in [(1u16, 1), (3, 1), (6_192, 0), (6_193, 1), (6_194, 0)] {
        bytes.extend(reference.to_be_bytes());
        bytes.push(status);
    }
    bytes.push(0x2b);
    let values: [f64; 9] = [-0.025, -0.05, 0.25, 0.0, 1.0, 0.0, 0.0, -0.0, 1.0];
    for value in values {
        bytes.extend(value.to_be_bytes());
    }
    let packet_end = bytes.len();
    bytes.extend(deltas_type_45(11));
    let suffix_offset = bytes.len();
    bytes.extend([0xcc, 0xdd]);
    let streams = [Stream {
        file_offset: 0,
        consumed: 0,
        inflated: bytes.clone(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype: crate::parasolid::ParasolidSubtype::Deltas,
            schema: None,
        },
    }];

    let events = crate::native::parasolid::parasolid_deltas_events(&streams);

    assert_eq!(events.type_150_state_packets.len(), 1);
    let packet = &events.type_150_state_packets[0];
    assert_eq!(packet.state.references(), [1, 3, 6_192, 6_193, 6_194]);
    assert_eq!(u8::from(packet.state.marker), 0x2b);
    assert_eq!(packet.state.values(), values);
    assert_eq!(
        packet.inflated_offset,
        cadmpeg_core::decode::u64_from_index(packet_offset)
    );
    assert_eq!(
        packet.byte_len,
        cadmpeg_core::decode::u64_from_index(packet_end - packet_offset)
    );
    assert_eq!(
        packet.sha256,
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&bytes[packet_offset..packet_end])
    );
    assert_eq!(events.residual_spans.len(), 2);
    assert_eq!(events.residual_spans[0].byte_len, 2);
    assert_eq!(
        events.residual_spans[1].inflated_offset,
        cadmpeg_core::decode::u64_from_index(suffix_offset)
    );
    assert_eq!(events.residual_spans[1].byte_len, 2);
}
