// SPDX-License-Identifier: Apache-2.0
//! Decode admission at early validation and byte transformation boundaries.

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

#[test]
fn deltas_attdef_validation_stops_at_first_invalid_slot() {
    let mut references = vec![1; 20_000];
    references[0] = 2;
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 1;
        },
        |ctx| {
            let validation =
                crate::deltas::attdef_state::AttdefState::from_wire(ctx, 2, 20_000, 0, references)
                    .expect("only the invalid first inactive slot is visited");
            assert_eq!(
                validation.unwrap_err(),
                "references: inactive slots must be null"
            );
        },
    );
    let mut references = vec![2; 20_000];
    references[0] = 1;
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 2;
        },
        |ctx| {
            let validation = crate::deltas::attdef_state::AttdefState::from_wire(
                ctx, 2, 20_000, 20_000, references,
            )
            .expect("empty inactive end probe and first active slot are visited");
            assert_eq!(
                validation.unwrap_err(),
                "references: active slots must be non-null"
            );
        },
    );
}

#[test]
fn deltas_preamble_validation_stops_at_first_invalid_entry() {
    for (first, expected) in [
        ((81, 1), "entries.reference: must exceed one"),
        ((83, 2), "entries.kind: must be 81 or 82"),
    ] {
        let mut entries = vec![(81, 2); 20_000];
        entries[0] = first;
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 1;
            },
            |ctx| {
                let validation = crate::deltas::preamble_state::PreambleState::from_wire(
                    ctx,
                    crate::deltas::preamble_state::PreambleFields {
                        identity: 2,
                        references: [2, 3],
                        state_references: [1; 3],
                        state_words: [0, 0, 1, 0],
                        count: 1,
                        entries,
                        terminal_value: 0,
                    },
                )
                .expect("only the first invalid entry is visited");
                assert_eq!(validation.unwrap_err(), expected);
            },
        );
    }
}

#[test]
fn deltas_transmit_validation_stops_at_first_invalid_character() {
    let description = format!("(deltas)\n{}", "x".repeat(20_000));
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            // Marker search visits one fixed window. The printable scan visits
            // the eight marker characters and the invalid newline.
            policy.limits.max_work_units = 10;
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let validation = crate::deltas::transmit_state::TransmitState::from_wire(
                ctx,
                &description,
                "SCH_A",
                [2, 3],
            )
            .expect("validation stops before the printable suffix and retains no text");
            assert_eq!(
                validation.unwrap_err(),
                "description: require printable ASCII containing (deltas)"
            );
        },
    );
}

#[test]
fn deltas_body_header_discriminant_does_not_scan_attdef_payload() {
    let mut bytes = crate::deltas::ATTDEF_LIST_SCHEMA_HEADER.to_vec();
    bytes.extend_from_slice(&20_000_u32.to_be_bytes());
    bytes.extend_from_slice(&2_u16.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    for _ in 0..20_001 {
        bytes.extend_from_slice(&[0, 1, 1]);
    }
    bytes.extend_from_slice(&[0xfe; 3]);
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &bytes))
            .expect("complete ATTDEF census setup");
    assert!(matches!(
        census.inline_schema_declarations[0].fields,
        crate::deltas::inline_schema_fields::InlineSchemaFields::AttdefList { .. }
    ));
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_work_units = 10_000;
        },
        |ctx| {
            assert!(crate::deltas::inline_body_states(ctx, &bytes, &census)
                .expect("fixed discriminant work is independent of slot count")
                .0
                .is_empty());
        },
    );
}

#[test]
fn deltas_type_45_validation_stops_at_first_invalid_scalar() {
    let mut bytes = 45_u16.to_be_bytes().to_vec();
    bytes.extend_from_slice(&20_000_u32.to_be_bytes());
    bytes.extend_from_slice(&3_u16.to_be_bytes());
    bytes.extend_from_slice(&f64::NAN.to_be_bytes());
    bytes.extend(vec![0; 20_000 * 8]);
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            policy.limits.max_work_units = 1;
        },
        |ctx| {
            assert!(crate::deltas::consume_type_45(ctx, &bytes, 0)
                .expect("only the first invalid lane scalar is visited")
                .is_none());
        },
    );
}

#[test]
fn deltas_merge_copy_refuses_at_named_work_boundary() {
    let partition = [0xff; 10];
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &[]))
            .expect("census setup");
    let error = crate::test_support::resource_refusal_at(
        &partition,
        ResourceDimension::WorkUnits,
        "NX merged partition copy",
        |ctx| crate::deltas::merge_full_records_with_census(ctx, &partition, &[], &census),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX merged partition copy"));
}

#[test]
fn deltas_residual_mask_refuses_at_named_work_boundary() {
    let bytes = [0xff; 10];
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &[]))
            .expect("census setup");
    let error = crate::test_support::resource_refusal_at(
        &bytes,
        ResourceDimension::WorkUnits,
        "NX semantic residual mask",
        |ctx| crate::deltas::semantic_residual_with_census(ctx, &bytes, &census),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX semantic residual mask"));
}

#[test]
fn deltas_merge_retains_only_the_returned_partition() {
    let partition = [0xff; 10];
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &[]))
            .expect("census setup");
    crate::test_support::with_decode_context_over(
        &partition,
        |policy| {
            policy.limits.max_retained_bytes = 10;
        },
        |ctx| {
            assert_eq!(
                crate::deltas::merge_full_records_with_census(ctx, &partition, &[], &census)
                    .expect("temporary scopes and graphs do not consume retained bytes"),
                partition
            );
        },
    );
}

#[test]
fn deltas_terminal_event_selection_preserves_offset_and_input_order() {
    use crate::deltas::record_kind::RecordKind;
    use crate::deltas::{count_unmatched_events, MergeEvent};
    use std::collections::BTreeMap;
    let full = |offset| MergeEvent::Full { offset };
    let tombstone = |offset| MergeEvent::Tombstone {
        offset,
        kind: RecordKind::Body,
    };
    for (events, expected) in [
        (vec![tombstone(10)], 1),
        (vec![full(9), tombstone(10)], 0),
        (vec![tombstone(10), full(9)], 0),
        (vec![tombstone(9), full(10)], 0),
        (vec![full(10), tombstone(10)], 1),
        (vec![tombstone(10), full(10)], 0),
        (vec![full(10), tombstone(10), full(9)], 0),
        (vec![tombstone(10), tombstone(9)], 1),
    ] {
        crate::test_support::with_decode_context(|ctx| {
            let graph = crate::topology::Graph::parse(ctx, &[]).unwrap();
            let counts = count_unmatched_events(ctx, BTreeMap::from([((12, 3), events)]), &graph)
                .expect("event selection");
            assert_eq!(counts.get("BODY").copied().unwrap_or(0), expected);
        });
    }
}

#[test]
fn deltas_invalid_lane_conversion_keeps_partial_storage_temporary() {
    use crate::deltas::reference_lanes::{MapEntries, TaggedReferences};
    let mut tagged = vec![(79, 2); 20_000];
    tagged[2] = (79, 1);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 16;
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            assert_eq!(
                TaggedReferences::from_wire(ctx, tagged)
                    .expect(
                        "validation stops at the third entry without retaining the valid prefix"
                    )
                    .unwrap_err(),
                "references.reference: must exceed one"
            );
        },
    );
    let mut entries = vec![(2, 11); 20_000];
    entries[2] = (1, 11);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 16;
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            assert_eq!(
                MapEntries::from_wire(ctx, entries)
                    .expect(
                        "validation stops at the third entry without retaining the valid prefix"
                    )
                    .unwrap_err(),
                "entries.reference: one is the terminal clause"
            );
        },
    );
}

#[test]
fn deltas_gap_result_slots_stay_temporary_until_census_transfer() {
    let bytes = [0, 2, 1, 0, 1, 1, 83, 0, 1, 1];
    let census =
        crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &[]))
            .expect("empty census setup");
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            let (packets, _storage) = crate::deltas::reference_marker_packets(ctx, &bytes, &census)
                .expect("the transfer vector holds temporary slots");
            assert_eq!(packets.len(), 1);
            assert_eq!(u32::from(packets[0].reference), 2);
            assert_eq!((packets[0].offset, packets[0].end), (0, bytes.len()));
        },
    );
}

#[test]
fn deltas_fixed_canonical_bytes_are_retained_copies() {
    let mut bytes = 91_u16.to_be_bytes().to_vec();
    bytes.extend_from_slice(&2_u16.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    for _ in 0..6 {
        bytes.extend_from_slice(&[0, 2, 1]);
    }
    let record = crate::test_support::with_decode_context(|ctx| {
        crate::deltas::consume_type_91(ctx, &bytes, 0)
    })
    .expect("fixed parsing")
    .expect("complete fixed record");
    assert_eq!(record.canonical_bytes, bytes);
    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            assert!(matches!(crate::deltas::consume_type_91(ctx, &bytes, 0),
                Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes));
        },
    );
}
