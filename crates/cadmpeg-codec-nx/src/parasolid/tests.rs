// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::test_support::extract_streams;
use crate::test_support::test_bytes::zlib_compress;
use crate::test_support::test_bytes::zlib_compress_at_level;
use crate::test_support::test_om::segment_stream_payload;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::test_support::test_prt::prt_with_partition;
use crate::test_support::test_prt::single_part_prt;
use crate::test_support::test_streams::parasolid_entity_records_stream;
use crate::test_support::test_streams::partition_stream;
use crate::test_support::test_streams::primary_point_stream;

fn token(text: &str) -> cadmpeg_parasolid::OwnedSchemaToken {
    cadmpeg_parasolid::OwnedSchemaToken::try_from(text).expect("the fixture text is a schema token")
}

#[test]
fn chart_layout_requires_a_recognized_counted_transmit_description() {
    use super::ParasolidSubtype;
    use crate::intersection::ChartPointLayout;

    for (description, expected) in [
        (
            ": TRANSMIT FILE (partition) created by test",
            Some(ChartPointLayout::Xyz3),
        ),
        (
            ": TRANSMIT FILE (deltas) created by test",
            Some(ChartPointLayout::Ext11),
        ),
        (
            ": TRANSMIT FILE created by test",
            Some(ChartPointLayout::Xyz3),
        ),
        (": MESH FILE created by test", None),
        (": UNKNOWN (deltas) created by test", None),
        (": TRANSMIT FILE (deltas)unknown", None),
    ] {
        let mut bytes = b"PS".to_vec();
        bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(description.as_bytes());
        bytes.extend_from_slice(b"\0\0\0\x0fSCH_TEST_1_9999\0 (partition) (deltas)");
        let super::StreamBody::Parasolid { subtype, .. } = super::classify(&bytes) else {
            panic!("retained Parasolid payload");
        };
        assert_eq!(subtype.chart_point_layout(), expected, "{description}");
        let truncated_count = u32::try_from(bytes.len() + 1).unwrap().to_be_bytes();
        bytes[2..6].copy_from_slice(&truncated_count);
        assert!(matches!(
            super::classify(&bytes),
            super::StreamBody::Parasolid {
                subtype: ParasolidSubtype::Unresolved,
                ..
            }
        ));
    }
}

#[test]
fn unresolved_chart_descriptions_preserve_bytes_without_admitting_hvecs() {
    let original =
        crate::test_support::test_streams::two_support_charted_intersection_curve_stream();
    let description_end =
        6 + usize::try_from(cadmpeg_core::decode::View::u32_be_at(&original, 2).unwrap()).unwrap();
    for description in [": MESH FILE created by test", ": UNKNOWN created by test"] {
        let mut stream = b"PS".to_vec();
        stream.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
        stream.extend_from_slice(description.as_bytes());
        stream.extend_from_slice(&original[description_end..]);
        let bytes = prt_with_partition(&stream);
        crate::test_support::with_decode_context_over(
            &bytes,
            |_| {},
            |ctx| {
                let decoded =
                    crate::decode::decode(ctx, cadmpeg_core::decode::View::over_retained(&bytes))
                        .unwrap();
                let ir = serde_json::to_value(&decoded.ir).unwrap();
                assert!(ir["native"]["nx"].is_object());
                for arena in ["parasolid_chart_records", "parasolid_term_use_records"] {
                    assert!(
                        ir["native"]["nx"][arena]
                            .as_array()
                            .is_none_or(Vec::is_empty),
                        "{description}: {arena}"
                    );
                }
                assert!(!decoded
                    .ir
                    .model
                    .procedural_curves
                    .iter()
                    .any(|curve| matches!(
                        curve.definition(),
                        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
                    )));
                assert_eq!(
                    decoded
                        .source_fidelity
                        .retained_record("nx:container:parasolid#0")
                        .unwrap()
                        .data(),
                    Some(stream.as_slice())
                );
            },
        );
    }
}

#[test]
fn legacy_stream_boundaries_require_complete_transmit_headers() {
    crate::test_support::with_decode_context(|ctx| {
        let mut bytes = b"prefix".to_vec();
        bytes.extend_from_slice(b"PS\x00\x00not a header");
        let first = bytes.len();
        let first_description = b": TRANSMIT FILE (partition) created by test";
        bytes.extend_from_slice(b"PS");
        bytes.extend_from_slice(
            &(u32::try_from(first_description.len()).expect("fixture value fits u32"))
                .to_be_bytes(),
        );
        bytes.extend_from_slice(first_description);
        bytes.extend_from_slice(b"payload PS\x00\x00not a header");
        let second = bytes.len();
        let second_description = b": TRANSMIT FILE (deltas) created by test";
        bytes.extend_from_slice(b"PS");
        bytes.extend_from_slice(
            &(u32::try_from(second_description.len()).expect("fixture value fits u32"))
                .to_be_bytes(),
        );
        bytes.extend_from_slice(second_description);

        assert_eq!(
            super::legacy_stream_start(ctx, &bytes, 0).unwrap(),
            Some(first)
        );
        assert_eq!(
            super::legacy_stream_start(ctx, &bytes, first + 4).unwrap(),
            Some(second)
        );
        assert_eq!(
            super::legacy_stream_start(ctx, &bytes, second + 4).unwrap(),
            None
        );
    });
}

#[test]
fn legacy_short_sections_are_bounded_by_complete_transmit_headers() {
    let mut bytes = Vec::new();
    let first_description = b": TRANSMIT FILE (partition)";
    bytes.extend_from_slice(b"PS");
    bytes.extend_from_slice(
        &(u32::try_from(first_description.len()).expect("fixture value fits u32")).to_be_bytes(),
    );
    bytes.extend_from_slice(first_description);
    let second = bytes.len();
    let second_description = b": TRANSMIT FILE (deltas)";
    bytes.extend_from_slice(b"PS");
    bytes.extend_from_slice(
        &(u32::try_from(second_description.len()).expect("fixture value fits u32")).to_be_bytes(),
    );
    bytes.extend_from_slice(second_description);
    bytes.extend_from_slice(&[0; 64]);

    crate::test_support::with_decode_context_over(
        &bytes,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);

            let streams = super::extract_legacy_streams(ctx, root).unwrap();

            assert_eq!(streams.len(), 2);
            assert_eq!(streams[0].file_offset, 0);
            assert_eq!(streams[1].file_offset, second);
        },
    );
}

#[test]
fn parasolid_entity_51_records_retain_layout_selected_references() {
    crate::test_support::with_decode_context(|ctx| {
        let mut bytes = vec![0, 0x51];
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in 3..=8u16 {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&[0xaa, 0xbb]);

        let records = crate::parasolid::entity_51_records(ctx, &bytes)
            .unwrap()
            .records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].offset, 0);
        assert_eq!(records[0].byte_len, 26);
        assert_eq!(u32::from(records[0].xmt), 10);
        assert_eq!(records[0].sequence.get(), 2);
        assert_eq!(records[0].definition_xmt, 0x21);
        assert_eq!(records[0].leading_references, [3, 4, 5, 6, 7]);
        assert_eq!(records[0].trailing_references.values(), [8]);
        assert_eq!(
            crate::parasolid::entity_51_record_at(ctx, &bytes, 0).unwrap(),
            Some(records[0].clone())
        );
        assert!(crate::parasolid::entity_51_record_at(ctx, &bytes[..25], 0)
            .unwrap()
            .is_none());
    });
}

#[test]
fn parasolid_entity_51_definition_uses_extended_xmt_framing() {
    crate::test_support::with_decode_context(|ctx| {
        let mut bytes = vec![0, 0x51];
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&(-7_233i16).to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        for reference in 3..=8u16 {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }

        let record = crate::parasolid::entity_51_record_at(ctx, &bytes, 0)
            .unwrap()
            .unwrap();
        assert_eq!(record.definition_xmt, 40_000);
        assert_eq!(record.byte_len, 28);
        assert!(crate::parasolid::entity_51_record_at(ctx, &bytes[..27], 0)
            .unwrap()
            .is_none());
    });
}

#[test]
fn parasolid_entity_51_reference_count_is_five_plus_flags() {
    crate::test_support::with_decode_context(|ctx| {
        for flags in 1..=0x20u32 {
            let mut direct = vec![0, 0x51];
            direct.extend_from_slice(&flags.to_be_bytes());
            direct.extend_from_slice(&10u16.to_be_bytes());
            direct.extend_from_slice(&2u32.to_be_bytes());
            direct.extend_from_slice(&0x21u16.to_be_bytes());
            for reference in 0..flags + 5 {
                direct.extend_from_slice(
                    &(u16::try_from(reference).expect("fixture value fits u16") + 3).to_be_bytes(),
                );
            }
            direct.extend_from_slice(&[0xaa, 0xbb]);

            let record = crate::parasolid::entity_51_record_at(ctx, &direct, 0)
                .unwrap()
                .unwrap();
            assert_eq!(record.leading_references.len(), 5);
            assert_eq!(
                record.trailing_references.values().len(),
                usize::try_from(flags).expect("fixture value fits usize")
            );
            assert_eq!(record.byte_len, direct.len() - 2);
            assert!(
                crate::parasolid::entity_51_record_at(ctx, &direct[..direct.len() - 3], 0)
                    .unwrap()
                    .is_none()
            );

            let mut prefixed = vec![0, 0x51];
            prefixed.extend_from_slice(&flags.to_be_bytes());
            prefixed.extend_from_slice(&10u16.to_be_bytes());
            prefixed.extend_from_slice(&2u32.to_be_bytes());
            prefixed.extend_from_slice(&0x21u16.to_be_bytes());
            for reference in 0..flags + 5 {
                prefixed.push(u8::from(reference % 2 == 0));
                prefixed.extend_from_slice(
                    &(u16::try_from(reference).expect("fixture value fits u16") + 3).to_be_bytes(),
                );
            }
            prefixed.push(0);
            prefixed.extend_from_slice(&[0xaa, 0xbb]);

            let record = crate::parasolid::entity_51_record_at(ctx, &prefixed, 0)
                .unwrap()
                .unwrap();
            assert_eq!(record.leading_references.len(), 5);
            assert_eq!(
                record.trailing_references.values().len(),
                usize::try_from(flags).expect("fixture value fits usize")
            );
            assert_eq!(record.byte_len, prefixed.len() - 2);
            assert!(
                crate::parasolid::entity_51_record_at(ctx, &prefixed[..prefixed.len() - 3], 0)
                    .unwrap()
                    .is_none()
            );
        }
    });
}

#[test]
fn parasolid_entity_51_rejects_nonzero_upper_flag_bytes() {
    crate::test_support::with_decode_context(|ctx| {
        let mut bytes = vec![0, 0x51];
        bytes.extend_from_slice(&0x0100_0001u32.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&0x21u16.to_be_bytes());
        for reference in 3..=8u16 {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }

        assert!(crate::parasolid::entity_51_record_at(ctx, &bytes, 0)
            .unwrap()
            .is_none());
    });
}

#[test]
fn parasolid_field_names_require_a_complete_nonempty_reference_lane() {
    crate::test_support::with_decode_context(|ctx| {
        let bytes = [
            0xaa, 0x00, 0x63, 0x00, 0x00, 0x00, 0x03, 0x00, 0x19, 0x00, 0x1c, 0x00, 0x1d, 0x00,
            0x1e, 0xbb,
        ];
        let records = crate::parasolid::field_names_records(ctx, &bytes)
            .unwrap()
            .records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].offset, 1);
        assert_eq!(records[0].byte_len, 14);
        assert_eq!(u32::from(records[0].xmt), 25);
        assert_eq!(
            records[0]
                .name_xmts
                .as_slice()
                .iter()
                .copied()
                .map(u32::from)
                .collect::<Vec<_>>(),
            [28, 29, 30]
        );
        assert!(crate::parasolid::field_names_record_at(
            ctx,
            &bytes[..14],
            1,
            &mut ctx.reserve_scoped(0, "test NX field-name lane").unwrap()
        )
        .unwrap()
        .is_none());

        let empty = [0x00, 0x63, 0, 0, 0, 0, 0, 25];
        assert!(crate::parasolid::field_names_records(ctx, &empty)
            .unwrap()
            .records
            .is_empty());
    });
}

#[test]
fn parasolid_field_name_scan_does_not_admit_nested_counted_candidates() {
    crate::test_support::with_decode_context(|ctx| {
        let mut bytes = vec![0x00, 0x63];
        bytes.extend_from_slice(&6u32.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&[
            0x00, 0x63, 0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x20, 0x00, 0x30, 0x00, 0x00, 0x40,
        ]);

        let records = crate::parasolid::field_names_records(ctx, &bytes)
            .unwrap()
            .records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].offset, 0);
        assert_eq!(
            records[0]
                .name_xmts
                .as_slice()
                .iter()
                .copied()
                .map(u32::from)
                .collect::<Vec<_>>(),
            [99, 256, 256, 8192, 12288, 64]
        );
    });
}

#[test]
fn partition_values_require_a_unique_entity_reference() {
    crate::test_support::with_decode_context(|ctx| {
        let mut bytes = parasolid_entity_records_stream();
        let owned = crate::parasolid::referenced_value_record_offsets(ctx, &bytes)
            .unwrap()
            .0;
        let (candidates, _candidate_guard) = super::value_records::value_record_candidates(
            ctx,
            &bytes,
            &[100, 101, 102].into_iter().collect(),
        )
        .unwrap();
        assert_eq!(candidates.len(), 3);
        assert!(
            owned.is_empty(),
            "leading references cannot establish value ownership"
        );

        bytes.extend_from_slice(&[0, 98]);
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&200u16.to_be_bytes());
        bytes.extend_from_slice(&[0, b'N', 0, b'X']);

        assert_eq!(
            crate::parasolid::referenced_value_record_offsets(ctx, &bytes)
                .unwrap()
                .0,
            owned
        );
    });
}

#[test]
fn partition_character_values_can_be_owned_by_a_field_name_list() {
    crate::test_support::with_decode_context(|ctx| {
        let mut bytes = parasolid_entity_records_stream();
        let definition_offset = crate::parasolid::attribute_definitions(ctx, &bytes)
            .unwrap()
            .records[0]
            .offset;
        bytes[definition_offset + 24..definition_offset + 26].copy_from_slice(&34u16.to_be_bytes());
        bytes.extend_from_slice(&[0, 0x63]);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&34u16.to_be_bytes());
        bytes.extend_from_slice(&37u16.to_be_bytes());
        let unicode_offset = bytes.len();
        bytes.extend_from_slice(&[0, 98]);
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&37u16.to_be_bytes());
        bytes.extend_from_slice(&[0, b'N', 0, b'X']);

        assert!(
            crate::parasolid::referenced_value_record_offsets(ctx, &bytes)
                .unwrap()
                .0
                .contains(&unicode_offset)
        );
    });
}

#[test]
fn parasolid_extraction_classifies_partition_and_schema() {
    let f = single_part_prt();
    let streams = extract_streams(&f);
    let part = streams
        .iter()
        .find(|s| s.kind() == StreamKind::Partition)
        .expect("a partition stream");
    assert_eq!(part.schema_token(), Some(&token("SCH_TEST_1_9999")));
    assert!(part.inflated.starts_with(b"PS\x00\x00"));
}

#[test]
fn external_reference_empty_record_parser_requires_the_complete_form() {
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1]),
        Some(false)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 1]),
        Some(true)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 0]),
        None
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0]),
        None
    );
}

#[test]
fn external_reference_tail_pairs_require_adjacent_complete_tokens() {
    let bytes = [
        0xff, 0xe0, 0x12, 0x34, 0x56, 0x78, 0xca, 0xbc, 0xde, 0xf0, 0xe0, 0x00, 0x00, 0x00, 0x01,
        0x00,
    ];
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::container::parse_extref_reference_pairs(ctx, &bytes)
        })
        .expect("tail pair resources"),
        vec![(
            1,
            0x1234_5678,
            crate::om::reference_value::Tagged28::try_from(0x0abc_def0).unwrap()
        )]
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::container::parse_extref_reference_pairs(ctx, &bytes[10..])
    })
    .expect("tail pair resources")
    .is_empty());
}

#[test]
fn extraction_uses_ug_part_bounds_and_all_standard_zlib_headers() {
    let part = zlib_compress_at_level(&partition_stream(), 6);
    assert_eq!(&part[..2], b"\x78\x9c");

    let mut decoy_stream = partition_stream();
    let schema = b"SCH_TEST_1_9999";
    let decoy = b"SCH_FAKE_1_9999";
    let pos = decoy_stream
        .windows(schema.len())
        .position(|w| w == schema)
        .unwrap();
    decoy_stream[pos..pos + schema.len()].copy_from_slice(decoy);
    let decoy = zlib_compress(&decoy_stream);

    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", part),
        ("/Root/FastLoad/JT", decoy),
    ]);

    let streams = extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].schema_token(), Some(&token("SCH_TEST_1_9999")));
}

#[test]
fn extraction_accepts_short_complete_zlib_members_in_ug_part() {
    let inflated = b"PS\0\0SCH_X";
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", zlib_compress(inflated))]);

    let streams = extract_streams(&file);

    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].inflated, inflated);
    assert_eq!(streams[0].kind(), StreamKind::Plain);
    assert_eq!(streams[0].schema_token(), Some(&token("SCH_X")));
}

#[test]
fn extraction_rejects_zlib_members_with_invalid_integrity_trailers() {
    let compressed = zlib_compress(&partition_stream());
    let mut corrupt = compressed.clone();
    *corrupt.last_mut().expect("zlib integrity trailer") ^= 0x01;
    let corrupt = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", corrupt)]);
    assert!(extract_streams(&corrupt).is_empty());

    let truncated = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        compressed[..compressed.len() - 1].to_vec(),
    )]);
    assert!(extract_streams(&truncated).is_empty());

    let mut indexed = segment_stream_payload();
    *indexed.last_mut().expect("indexed zlib integrity trailer") ^= 0x01;
    let indexed = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", indexed)]);

    crate::test_support::with_decode_context_over(
        &indexed,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&indexed);

            let container = crate::test_support::with_decode_context(|ctx| {
                container::scan_bytes(ctx, indexed.clone())
            })
            .expect("test SPLMSSTR container");
            assert!(parasolid::extract_streams(ctx, root, &container).is_err());
        },
    );
}

#[test]
fn extraction_refuses_inflated_stream_copy_when_retained_budget_is_exhausted() {
    let file = prt_with_partition(&partition_stream());

    crate::test_support::with_decode_context_over(
        &file,
        |policy| {
            policy.limits.max_retained_bytes =
                1 + cadmpeg_test_support::decode::arena_registry_bytes();
        },
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&file);

            let container = crate::test_support::with_decode_context(|ctx| {
                container::scan_bytes(ctx, file.clone())
            })
            .expect("test SPLMSSTR container");

            assert!(matches!(
                parasolid::extract_streams(ctx, root, &container),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                        && limit.operation == "retain NX inflated stream"
            ));
        },
    );
}

#[test]
fn extraction_uses_ordered_segment_wrappers_in_indexed_payloads() {
    let decoy = zlib_compress(
        b"PS\0\0 (partition) SCH_DECOY_1_9999 unindexed payload with more than sixty-four inflated bytes........",
    );
    let real = zlib_compress(&crate::test_support::test_bytes::transmit_preamble(
        ": TRANSMIT FILE (deltas) created by test indexed payload",
        "SCH_REAL_1_9999",
    ));
    let mut payload = Vec::new();
    for word in [0_u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&decoy);
    let wrapper_offset = payload.len();
    payload[0..4].copy_from_slice(
        &u32::try_from(wrapper_offset)
            .expect("synthetic wrapper offset")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
    payload.extend_from_slice(&u32::try_from(real.len()).unwrap().to_le_bytes());
    payload.extend_from_slice(&real);

    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let streams = extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].kind(), StreamKind::Deltas);
    assert_eq!(streams[0].schema_token(), Some(&token("SCH_REAL_1_9999")));
}

fn indexed_member_payload(header: &[u8], member: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32_u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&[0; 8]);
    payload.extend_from_slice(header);
    payload.extend_from_slice(member);
    payload
}

#[test]
fn partial_indexes_retain_structural_members_without_reordering_indexed_members() {
    let structural = zlib_compress(&primary_point_stream());
    let decoy = zlib_compress(b"PS\0\0 (partition) SCH_DECOY text without owned records");
    let indexed = zlib_compress(&crate::test_support::test_bytes::transmit_preamble(
        ": TRANSMIT FILE (deltas) created by partial index test",
        "SCH_INDEXED_1_13006",
    ));
    for reverse_index in [false, true] {
        let mut payload = vec![0; 24];
        for (at, word) in [(12, 1_u32), (16, 1), (20, 24)] {
            payload[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        let mut physical_offsets = Vec::new();
        let mut wrapper_offsets = Vec::new();
        for ordinal in 0..5 {
            if ordinal % 2 == 0 {
                physical_offsets.push(payload.len());
                payload.extend_from_slice(&structural);
            } else {
                wrapper_offsets.push(payload.len());
                payload.extend_from_slice(&0xa000_0000_u32.to_le_bytes());
                for _ in 0..2 {
                    payload.extend_from_slice(&u32::try_from(indexed.len()).unwrap().to_le_bytes());
                }
                payload.extend_from_slice(&0_u32.to_le_bytes());
                physical_offsets.push(payload.len());
                payload.extend_from_slice(&indexed);
            }
            payload.extend_from_slice(&decoy);
        }
        if reverse_index {
            wrapper_offsets.reverse();
        }
        for (word, offset) in wrapper_offsets.into_iter().enumerate() {
            payload[word * 4..word * 4 + 4]
                .copy_from_slice(&u32::try_from(offset).unwrap().to_le_bytes());
        }
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
        let container = crate::test_support::with_decode_context(|ctx| {
            container::scan_bytes(ctx, file.clone())
        })
        .unwrap();
        let (part_start, _) = container
            .entries
            .iter()
            .find(|entry| entry.name == "/Root/UG_PART/UG_PART")
            .unwrap()
            .file_span()
            .unwrap();
        let offsets = extract_streams(&file)
            .iter()
            .map(|stream| stream.file_offset - usize::try_from(part_start).unwrap())
            .collect::<Vec<_>>();
        let expected = if reverse_index {
            [0, 2, 3, 1, 4]
                .map(|ordinal| physical_offsets[ordinal])
                .to_vec()
        } else {
            physical_offsets
        };
        assert_eq!(offsets, expected, "reverse_index={reverse_index}");
    }
}

#[test]
fn indexed_wrapper_families_bound_the_exact_compressed_member() {
    let inflated = crate::test_support::test_bytes::transmit_preamble(
        ": TRANSMIT FILE (deltas) created by indexed wrapper test",
        "SCH_TEST_1_13006",
    );
    let member = zlib_compress(&inflated);
    let member_len = u32::try_from(member.len()).unwrap();
    for (marker, fixed_len, extension, code, version) in [
        (0x8000_0003_u32, 8, 3, 0_u32, 0_u16),
        (0xa000_0000, 16, 0, 0, 0),
        (0xc000_0005, 33, 5, 37, 4),
        (0xc000_0007, 45, 7, 45, 5),
    ] {
        let mut header = vec![0; fixed_len + extension];
        header[..4].copy_from_slice(&marker.to_le_bytes());
        header[4..8].copy_from_slice(&member_len.to_le_bytes());
        if fixed_len == 16 {
            header[8..12].copy_from_slice(&member_len.to_le_bytes());
        } else if fixed_len >= 33 {
            header[8..12].copy_from_slice(&code.to_le_bytes());
            header[12..14].copy_from_slice(&version.to_le_bytes());
            if fixed_len == 45 {
                header[14..18].copy_from_slice(&member_len.to_le_bytes());
            }
            header[18..fixed_len].fill(0xa5);
        }
        header[fixed_len..].fill(0x78);
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            indexed_member_payload(&header, &member),
        )]);
        let container = crate::test_support::with_decode_context(|ctx| {
            container::scan_bytes(ctx, file.clone())
        })
        .unwrap();
        let wrappers = container.segment_stream_wrappers().collect::<Vec<_>>();
        assert_eq!(wrappers.len(), 1, "{marker:x}");
        assert_eq!(wrappers[0].wrapper_byte_len, fixed_len + extension);
        assert_eq!(wrappers[0].compressed_byte_len, member.len());
        let streams = extract_streams(&file);
        assert_eq!(streams.len(), 1, "{marker:x}");
        assert_eq!(streams[0].file_offset, wrappers[0].zlib_offset);
        assert_eq!(streams[0].consumed, u64::from(member_len));
        assert_eq!(streams[0].inflated, inflated);
    }
}

#[test]
fn duplicate_indexed_starts_require_one_compressed_extent_in_either_order() {
    let member = zlib_compress(b"indexed preview");
    for difference in [-1_i32, 0, 1] {
        for reverse in [false, true] {
            let actual = u32::try_from(member.len()).unwrap();
            let other = actual.checked_add_signed(difference).unwrap();
            let mut header = vec![0; 24];
            header[..4].copy_from_slice(&0x8000_0010_u32.to_le_bytes());
            header[4..8].copy_from_slice(&actual.to_le_bytes());
            header[16..20].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
            header[20..24].copy_from_slice(&other.to_le_bytes());
            let mut payload = indexed_member_payload(&header, &member);
            let offsets = if reverse { [48_u32, 32] } else { [32_u32, 48] };
            for (word, offset) in offsets.into_iter().enumerate() {
                payload[word * 4..word * 4 + 4].copy_from_slice(&offset.to_le_bytes());
            }
            payload.push(0);
            let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
            crate::test_support::with_decode_context_over(
                &file,
                |_| {},
                |ctx| {
                    let container = container::scan_bytes(ctx, file.clone()).unwrap();
                    assert_eq!(container.segment_stream_wrappers().count(), 2);
                    let result = parasolid::extract_streams(
                        ctx,
                        cadmpeg_core::decode::View::over_retained(&file),
                        &container,
                    );
                    if difference == 0 {
                        assert_eq!(result.unwrap().len(), 1);
                    } else {
                        assert!(
                            matches!(result, Err(cadmpeg_core::CodecError::Malformed(_))),
                            "difference={difference}, reverse={reverse}"
                        );
                    }
                },
            );
        }
    }
}

#[test]
fn overlapping_indexed_members_are_rejected_in_either_order() {
    use std::io::Write;

    let inner = zlib_compress(&primary_point_stream());
    let mut inner_header = 0x8000_0000_u32.to_le_bytes().to_vec();
    inner_header.extend_from_slice(&u32::try_from(inner.len()).unwrap().to_le_bytes());
    let mut literal = primary_point_stream();
    literal.extend_from_slice(&inner_header);
    literal.extend_from_slice(&inner);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
    encoder.write_all(&literal).unwrap();
    let outer = encoder.finish().unwrap();
    let inner_at = outer
        .windows(inner_header.len())
        .position(|bytes| bytes == inner_header)
        .unwrap();
    assert_eq!(
        &outer[inner_at + inner_header.len()..][..inner.len()],
        inner
    );
    let mut outer_header = 0x8000_0000_u32.to_le_bytes().to_vec();
    outer_header.extend_from_slice(&u32::try_from(outer.len()).unwrap().to_le_bytes());
    for reverse in [false, true] {
        let mut payload = indexed_member_payload(&outer_header, &outer);
        let mut offsets = [32_u32, u32::try_from(40 + inner_at).unwrap()];
        if reverse {
            offsets.reverse();
        }
        for (word, offset) in offsets.into_iter().enumerate() {
            payload[word * 4..word * 4 + 4].copy_from_slice(&offset.to_le_bytes());
        }
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
        crate::test_support::with_decode_context_over(
            &file,
            |_| {},
            |ctx| {
                let container = container::scan_bytes(ctx, file.clone()).unwrap();
                assert_eq!(container.segment_stream_wrappers().count(), 2);
                let result = parasolid::extract_streams(
                    ctx,
                    cadmpeg_core::decode::View::over_retained(&file),
                    &container,
                );
                assert!(
                    matches!(result, Err(cadmpeg_core::CodecError::Malformed(message))
                if message.contains("overlapping indexed NX members"))
                );
            },
        );
    }
}

#[test]
fn supplemental_members_cannot_enclose_an_indexed_member() {
    use std::io::Write;

    let inner_bytes = primary_point_stream();
    let inner = zlib_compress(&inner_bytes);
    let mut inner_header = 0x8000_0000_u32.to_le_bytes().to_vec();
    inner_header.extend_from_slice(&u32::try_from(inner.len()).unwrap().to_le_bytes());
    let mut outer_bytes = primary_point_stream();
    outer_bytes.extend_from_slice(&inner_header);
    outer_bytes.extend_from_slice(&inner);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
    encoder.write_all(&outer_bytes).unwrap();
    let outer = encoder.finish().unwrap();
    let inner_wrapper = outer
        .windows(inner_header.len())
        .position(|bytes| bytes == inner_header)
        .unwrap();
    let mut payload = vec![0; 24];
    for (at, word) in [
        (0, u32::try_from(24 + inner_wrapper).unwrap()),
        (12, 1),
        (16, 1),
        (20, 24),
    ] {
        payload[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&outer);
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let streams = extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].inflated, inner_bytes);
    assert_eq!(streams[0].consumed, u64::try_from(inner.len()).unwrap());
}

#[test]
fn unresolved_index_targets_do_not_address_nearby_structural_members() {
    let expanded = primary_point_stream();
    let member = zlib_compress(&expanded);
    let length = u32::try_from(member.len()).unwrap();
    for (fixed_len, code, version) in [(33, 37_u32, 4_u16), (45, 45, 5)] {
        let mut header = vec![0; fixed_len];
        header[..4].copy_from_slice(&0xc000_0000_u32.to_le_bytes());
        header[4..8].copy_from_slice(&length.to_le_bytes());
        header[8..12].copy_from_slice(&code.to_le_bytes());
        header[12..14].copy_from_slice(&version.to_le_bytes());
        if fixed_len == 45 {
            header[14..18].copy_from_slice(&length.to_le_bytes());
        }
        header.extend_from_slice(&[0; 4]);
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            indexed_member_payload(&header, &member),
        )]);
        let container = crate::test_support::with_decode_context(|ctx| {
            container::scan_bytes(ctx, file.clone())
        })
        .unwrap();
        let wrapper = container.segment_stream_wrappers().next().unwrap();
        let streams = extract_streams(&file);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].file_offset, wrapper.zlib_offset + 4);
        assert_eq!(streams[0].inflated, expanded);
    }
}

#[test]
fn indexed_member_length_must_equal_the_checksum_validated_extent() {
    let member = zlib_compress(b"an indexed preview with a declared compressed extent");
    for (marker, fixed_len, code, version) in [
        (0x8000_0000_u32, 8, 0_u32, 0_u16),
        (0xa000_0000, 16, 0, 0),
        (0xc000_0000, 33, 37, 4),
        (0xc000_0000, 45, 45, 5),
    ] {
        for declared_len in [member.len() - 1, member.len() + 1] {
            let length = u32::try_from(declared_len).unwrap().to_le_bytes();
            let mut header = vec![0; fixed_len];
            header[..4].copy_from_slice(&marker.to_le_bytes());
            header[4..8].copy_from_slice(&length);
            if fixed_len == 16 {
                header[8..12].copy_from_slice(&length);
            } else if fixed_len >= 33 {
                header[8..12].copy_from_slice(&code.to_le_bytes());
                header[12..14].copy_from_slice(&version.to_le_bytes());
                if fixed_len == 45 {
                    header[14..18].copy_from_slice(&length);
                }
            }
            let mut payload = indexed_member_payload(&header, &member);
            payload.push(0);
            let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
            crate::test_support::with_decode_context_over(
                &file,
                |_| {},
                |ctx| {
                    let container = container::scan_bytes(ctx, file.clone()).unwrap();
                    let root = cadmpeg_core::decode::View::over_retained(&file);
                    assert!(matches!(
                        parasolid::extract_streams(ctx, root, &container),
                        Err(cadmpeg_core::CodecError::Malformed(_))
                    ));
                },
            );
        }
    }
}

#[test]
fn indexed_wrapper_headers_reject_inconsistent_lengths_versions_and_truncation() {
    let member = zlib_compress(b"indexed member");
    let member_len = u32::try_from(member.len()).unwrap();
    let mut a0 = vec![0; 16];
    a0[..4].copy_from_slice(&0xa000_0000_u32.to_le_bytes());
    a0[4..8].copy_from_slice(&member_len.to_le_bytes());
    a0[8..12].copy_from_slice(&member_len.to_le_bytes());
    let mut c0 = vec![0; 45];
    c0[..4].copy_from_slice(&0xc000_0000_u32.to_le_bytes());
    c0[4..8].copy_from_slice(&member_len.to_le_bytes());
    c0[8..12].copy_from_slice(&45_u32.to_le_bytes());
    c0[12..14].copy_from_slice(&5_u16.to_le_bytes());
    c0[14..18].copy_from_slice(&member_len.to_le_bytes());
    let mut c0_33 = vec![0; 33];
    c0_33[..4].copy_from_slice(&0xc000_0000_u32.to_le_bytes());
    c0_33[4..8].copy_from_slice(&member_len.to_le_bytes());
    c0_33[8..12].copy_from_slice(&37_u32.to_le_bytes());
    c0_33[12..14].copy_from_slice(&4_u16.to_le_bytes());
    for (valid, bad_offsets) in [
        (&a0, &[8, 12][..]),
        (&c0, &[8, 12, 14][..]),
        (&c0_33, &[8, 12, 14][..]),
    ] {
        for &offset in bad_offsets {
            let mut header = valid.clone();
            header[offset] ^= 1;
            let file = prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                indexed_member_payload(&header, &member),
            )]);
            let container =
                crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
                    .unwrap();
            assert_eq!(container.segment_stream_wrappers().count(), 0);
        }
        for end in 0..valid.len() {
            let file = prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                indexed_member_payload(&valid[..end], &[]),
            )]);
            let container =
                crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
                    .unwrap();
            assert_eq!(container.segment_stream_wrappers().count(), 0);
        }
    }
}

#[test]
fn extraction_falls_back_to_unindexed_primary_geometry_when_index_has_no_parasolid() {
    let decoy = zlib_compress(
        b"PS\0\0 (partition) SCH_DECOY_1_9999 unindexed text without structural records",
    );
    let real = zlib_compress(&primary_point_stream());
    let indexed_preview = zlib_compress(b"preview payload");
    let mut payload = Vec::new();
    for word in [0_u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&decoy);
    payload.extend_from_slice(&real);
    let wrapper_offset = payload.len();
    payload[0..4].copy_from_slice(
        &u32::try_from(wrapper_offset)
            .expect("synthetic wrapper offset")
            .to_le_bytes(),
    );
    payload.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
    payload.extend_from_slice(&u32::try_from(indexed_preview.len()).unwrap().to_le_bytes());
    payload.extend_from_slice(&indexed_preview);

    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let streams = extract_streams(&file);

    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|stream| {
        stream.kind() == StreamKind::Partition
            && stream.schema_token() == Some(&token("SCH_TEST_13006"))
    }));
    assert!(streams
        .iter()
        .all(|stream| { stream.schema_token() != Some(&token("SCH_DECOY_1_9999")) }));
}

#[test]
fn unindexed_primary_candidates_require_schema_owned_records() {
    crate::test_support::with_decode_context(|ctx| {
        let bytes = primary_point_stream();
        assert!(super::structural_stream_candidate(ctx, StreamKind::Partition, &bytes).unwrap());
        let mut unknown = bytes.clone();
        let version = unknown
            .windows(5)
            .position(|window| window == b"13006")
            .unwrap();
        unknown[version..version + 5].copy_from_slice(b"99999");
        assert!(!super::structural_stream_candidate(ctx, StreamKind::Partition, &unknown).unwrap());
        let point = bytes
            .windows(2)
            .position(|window| window == [0, 29])
            .unwrap();
        assert!(!super::structural_stream_candidate(
            ctx,
            StreamKind::Partition,
            &bytes[..point + 39]
        )
        .unwrap());
    });
}

#[test]
fn legal_owner_flags_carry_binary_values_and_exact_layout_length() {
    use crate::parasolid::LegalOwnerFlags;

    let flags = LegalOwnerFlags::try_from(&[1; 14][..]).unwrap();
    assert_eq!(flags.as_slice(), &[true; 14]);
    assert_eq!(&flags.padded()[..14], &[1; 14]);
    assert_eq!(&flags.padded()[14..], &[0; 2]);
    assert!(LegalOwnerFlags::try_from(&[0; 15][..]).is_err());
    assert!(LegalOwnerFlags::try_from(&[2; 16][..]).is_err());
}

#[test]
fn counted_value_identities_require_nonempty_payloads() {
    crate::test_support::with_decode_context(|ctx| {
        for tag in [0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x62] {
            let bytes = [0, tag, 0, 0, 0, 0, 0, 17, 0];
            assert!(
                crate::parasolid::value_records::entity_value_record_identity_at(ctx, &bytes, 0)
                    .unwrap()
                    .is_none()
            );
        }
    });
}

#[test]
fn a_packed_member_reporting_fewer_bytes_than_a_zlib_member_holds_is_refused_by_name() {
    use crate::parasolid::{packed_member_advance, MIN_ZLIB_MEMBER_LEN};

    let error = packed_member_advance(4096, 1).expect_err("one consumed byte is not a member");
    let message = error.to_string();
    assert!(message.contains("nx packed member"), "{message}");
    assert!(message.contains("4096"), "{message}");
    assert!(message.contains(" 1 consumed source bytes"), "{message}");

    // The shortest member a decompressor can report is admitted, and the scan
    // advances by exactly the bytes the member consumed.
    assert_eq!(
        packed_member_advance(
            4096,
            cadmpeg_core::decode::u64_from_index(MIN_ZLIB_MEMBER_LEN)
        )
        .expect("the shortest zlib member"),
        MIN_ZLIB_MEMBER_LEN
    );
    assert_eq!(
        packed_member_advance(4096, 4321).expect("a member of ordinary length"),
        4321
    );
}

#[test]
fn attribute_reference_lanes_refuse_scoped_storage() {
    let bytes = [0, 0x63, 0, 0, 0, 1, 0, 17, 0, 18];
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_materialized_bytes = 0,
        |ctx| {
            assert!(
                matches!(crate::parasolid::field_names_records(ctx, &bytes), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "NX field-name reference lanes")
            );
        },
    );
}

#[test]
fn attribute_identifier_index_refuses_scoped_storage() {
    let bytes = [0, 0x4f, 0, 0, 0, 1, 0, 17, b'A'];
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_materialized_bytes = 0,
        |ctx| {
            assert!(
                matches!(crate::parasolid::attribute_definitions(ctx, &bytes), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "NX attribute identifier index")
            );
        },
    );
}

#[test]
fn owned_value_discovery_refuses_before_building_child_lanes() {
    let bytes = [0, 0x63, 0, 0, 0, 1, 0, 17, 0, 18];
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_materialized_bytes = 0,
        |ctx| {
            assert!(
                matches!(crate::parasolid::referenced_value_record_offsets(ctx, &bytes), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "NX field-name reference lanes")
            );
        },
    );
}

#[test]
fn numeric_identity_probe_uses_no_payload_storage() {
    let bytes = [0, 0x52, 0, 0, 0, 1, 0, 17, 0, 0, 0, 9];
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_collection_items = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            assert_eq!(
                crate::parasolid::value_records::entity_value_record_identity_at(ctx, &bytes, 0)
                    .unwrap(),
                Some((0x52, 17, 12))
            );
        },
    );
}

#[test]
fn numeric_value_payload_refuses_retained_storage() {
    let bytes = [0, 0x52, 0, 0, 0, 1, 0, 17, 0, 0, 0, 9];
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            assert!(
                matches!(crate::parasolid::value_records::entity_value_records_at(ctx, &bytes, [0]), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "NX numeric value payload")
            );
        },
    );
}

#[test]
fn unicode_value_conversion_refuses_scoped_scratch() {
    let bytes = [0, 0x62, 0, 0, 0, 1, 0, 17, 0, 65];
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_materialized_bytes = 0,
        |ctx| {
            assert!(
                matches!(crate::parasolid::value_records::entity_value_records_at(ctx, &bytes, [0]), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "NX Unicode conversion scratch")
            );
        },
    );
}

#[test]
fn embedded_stream_scan_refuses_exhausted_work() {
    let bytes = [0; 64];
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            let mut streams = Vec::new();
            assert!(
                matches!(super::append_all_zlib_streams(ctx, cadmpeg_core::decode::View::over_retained(&bytes), 0, &mut streams, false), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "scan NX embedded stream bytes")
            );
        },
    );
}

#[test]
fn legacy_stream_scan_refuses_exhausted_work() {
    let bytes = [0; 64];
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            assert!(
                matches!(super::extract_legacy_streams(ctx, cadmpeg_core::decode::View::over_retained(&bytes)), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "scan NX legacy stream headers")
            );
        },
    );
}

#[test]
fn legacy_stream_slots_refuse_exhausted_collection() {
    let description = b"TRANSMIT FILE";
    let mut bytes = vec![b'P', b'S'];
    bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(description);
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_collection_items = 0,
        |ctx| {
            assert!(
                matches!(super::extract_legacy_streams(ctx, cadmpeg_core::decode::View::over_retained(&bytes)), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "NX embedded stream slots")
            );
        },
    );
}

#[test]
fn attribute_duplicate_identifier_index_refuses_lookup_work() {
    let record = [0, 0x4f, 0, 0, 0, 1, 0, 17, b'A'];
    let bytes = record.repeat(2);
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_work_units = 21,
        |ctx| {
            assert!(
                matches!(crate::parasolid::attribute_identifiers(ctx, &bytes), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "resolve duplicate NX attribute identifier")
            );
        },
    );
}

#[test]
fn structural_entity_references_do_not_own_value_records() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&10_u16.to_be_bytes());
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&0x21_u16.to_be_bytes());
    for reference in 3..=8_u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&[0xaa, 0xbb]);
    let value_offset = bytes.len();
    bytes.extend_from_slice(&[0, 98]);
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&3_u16.to_be_bytes());
    bytes.extend_from_slice(&[0, b'N', 0, b'X']);
    crate::test_support::with_decode_context(|ctx| {
        let (owned, _) = super::referenced_value_record_offsets(ctx, &bytes).unwrap();
        assert!(!owned.contains(&value_offset));
        let (references, _) =
            super::referenced_value_xmts(ctx, &bytes, super::ValueMultiplicity::UniqueSnapshot)
                .unwrap();
        assert!(!references.contains(&3));
        assert!(references.contains(&8));
    });
    bytes[24..26].copy_from_slice(&3_u16.to_be_bytes());
    crate::test_support::with_decode_context(|ctx| {
        let (owned, _) = super::referenced_value_record_offsets(ctx, &bytes).unwrap();
        assert!(owned.contains(&value_offset));
    });
}

#[test]
fn value_ownership_membership_uses_the_identity_index() {
    let references = (0..10_000).collect();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 400,
        |ctx| {
            assert!(super::value_is_referenced(
                ctx,
                &references,
                1234,
                "resolve NX value ownership"
            )
            .unwrap());
            assert!(!super::value_is_referenced(
                ctx,
                &references,
                20_000,
                "resolve NX field-name ownership"
            )
            .unwrap());
            assert!(ctx.resource_refusal().is_none());
        },
    );
}

#[test]
fn value_ownership_membership_keeps_the_session_work_fuse() {
    let references = [1234].into_iter().collect();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            let error =
                super::value_is_referenced(ctx, &references, 1234, "resolve NX value ownership")
                    .unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "resolve NX value ownership" && ctx.resource_refusal() == Some(limit))
            );
        },
    );
}

#[test]
fn attribute_owner_grouping_is_admitted_once() {
    let mut bytes = Vec::new();
    for xmt in 501..3501_u16 {
        bytes.extend_from_slice(&[0, 0x51]);
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&xmt.to_be_bytes());
        bytes.extend_from_slice(&2_u32.to_be_bytes());
        bytes.extend_from_slice(&0x21_u16.to_be_bytes());
        for reference in 3..=8_u16 {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.extend_from_slice(&[0xaa, 0xbb]);
    }
    // The shared grouping helper bills N * (N + 1) / 2 units. An extra
    // caller-side full-map charge would bill another N * (N - 1) / 2,
    // exceeding this budget before the linear framing scans finish.
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_work_units = 8_000_000,
        |ctx| {
            let (references, _reservation) =
                super::referenced_value_xmts(ctx, &bytes, super::ValueMultiplicity::UniqueSnapshot)
                    .expect("group insertion work is billed once");
            assert!(references.contains(&8));
            assert!(ctx.resource_refusal().is_none());
        },
    );
}

#[test]
fn unresolved_index_targets_do_not_probe_for_nonstructural_members() {
    let member = zlib_compress(b"PS\0\0 (partition) SCH_DECOY text without owned records");
    let length = u32::try_from(member.len()).unwrap();
    for (fixed_len, code, version) in [(33, 37_u32, 4_u16), (45, 45, 5)] {
        let mut header = vec![0; fixed_len];
        header[..4].copy_from_slice(&0xc000_0000_u32.to_le_bytes());
        header[4..8].copy_from_slice(&length.to_le_bytes());
        header[8..12].copy_from_slice(&code.to_le_bytes());
        header[12..14].copy_from_slice(&version.to_le_bytes());
        if fixed_len == 45 {
            header[14..18].copy_from_slice(&length.to_le_bytes());
        }
        header.extend_from_slice(&[0; 4]);
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            indexed_member_payload(&header, &member),
        )]);
        let container = crate::test_support::with_decode_context(|ctx| {
            container::scan_bytes(ctx, file.clone())
        })
        .unwrap();
        assert_eq!(container.segment_stream_wrappers().count(), 1);
        assert!(extract_streams(&file).is_empty(), "fixed_len={fixed_len}");
    }
}
