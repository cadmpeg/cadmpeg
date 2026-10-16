// SPDX-License-Identifier: Apache-2.0

use super::{PrimaryEnd, PrimaryRecords, StopReason};
use crate::test_support::with_decode_context;

fn header(version: u16, embedded: bool) -> Vec<u8> {
    let description = b": TRANSMIT FILE (partition) primary contract";
    let schema = if embedded {
        format!("SCH_999_999_{version}")
    } else {
        format!("SCH_999_{version}")
    };
    let mut bytes = b"PS".to_vec();
    bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(description);
    bytes.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(schema.as_bytes());
    if embedded {
        bytes.extend_from_slice(&250u16.to_be_bytes());
    }
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes
}

fn edge(first: bool, identity: u16) -> Vec<u8> {
    let mut bytes = vec![0, 16];
    if first {
        bytes.push(255);
    }
    bytes.extend_from_slice(&identity.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&0f64.to_be_bytes());
    for _ in 0..7 {
        bytes.extend_from_slice(&[0, 1]);
    }
    bytes
}

const END: [u8; 4] = [0, 1, 0, 1];

fn group(first: bool, identity: u16, selector: u8) -> Vec<u8> {
    let mut bytes = 90u16.to_be_bytes().to_vec();
    if first {
        bytes.push(255);
    }
    bytes.extend_from_slice(&identity.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 2, 255, 255, 0, 1, 0, 1]);
    bytes.push(selector);
    bytes.extend_from_slice(&[0, 1]);
    bytes
}

#[test]
fn primary_group_projection_ends_at_its_last_pointer_without_status_bytes() {
    with_decode_context(|ctx| {
        for version in [11004, 13006] {
            for embedded in [false, true] {
                let mut bytes = header(version, embedded);
                let mut ends = Vec::new();
                for selector in 1..=10u8 {
                    bytes.extend(group(
                        selector == 1 && embedded,
                        u16::from(selector + 2),
                        selector,
                    ));
                    ends.push(bytes.len());
                }
                bytes.extend(END);
                let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
                let groups = parsed.groups(ctx).unwrap();
                assert_eq!(groups.len(), 10);
                for (index, group) in groups.iter().enumerate() {
                    assert_eq!(group.node_id, u32::MAX);
                    assert_eq!(group.references, [1, 2, 32768, 1, 1]);
                    assert_eq!(usize::from(u8::from(group.selector)), index + 1);
                    assert_eq!(group.end, ends[index]);
                    assert_eq!(group.xmt, u32::try_from(index + 3).unwrap());
                }
                assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
                let truncated = PrimaryRecords::read(ctx, &bytes[..ends[9] - 1])
                    .unwrap()
                    .unwrap();
                assert_eq!(truncated.groups(ctx).unwrap().len(), 9);
                assert!(matches!(
                    truncated.end,
                    PrimaryEnd::Stopped {
                        reason: StopReason::IncompleteField,
                        ..
                    }
                ));
            }
        }
    });
}

#[test]
fn primary_versions_keep_full_node_ids_and_terminator_suffix() {
    with_decode_context(|ctx| {
        for version in [11004, 13006] {
            for embedded in [false, true] {
                let mut bytes = header(version, embedded);
                let start = bytes.len();
                bytes.extend(edge(embedded, 11));
                let end = bytes.len();
                bytes.extend(END);
                bytes.extend_from_slice(&[0, 16, 0, 11]);
                let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
                assert_eq!(parsed.records.len(), 1);
                let record = &parsed.records[0];
                assert_eq!(record.definition.kind, 16);
                assert_eq!(u32::from(record.xmt), 11);
                assert_eq!(record.range, start..end);
                assert_eq!(record.identity_at, start + 2 + usize::from(embedded));
                assert_eq!(&bytes[record.fields[0].clone()], &u32::MAX.to_be_bytes());
                assert_eq!(
                    parsed.end,
                    PrimaryEnd::Terminated {
                        terminator: end..end + 4,
                        suffix: end + 4..end + 8
                    }
                );
            }
        }
    });
}

#[test]
fn primary_first_use_and_variable_fields_own_payload_tags() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, true);
        bytes.extend(edge(true, 11));
        let array_start = bytes.len();
        let mut payload = edge(false, 22);
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        bytes.extend_from_slice(&[0, 82, 255]);
        bytes.extend_from_slice(&u32::try_from(payload.len() / 4).unwrap().to_be_bytes());
        bytes.extend_from_slice(&[0, 30]);
        bytes.extend_from_slice(&payload);
        let array_end = bytes.len();
        bytes.extend(edge(false, 12));
        let end = bytes.len();
        bytes.extend(END);
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(
            parsed
                .records
                .iter()
                .map(|record| record.definition.kind)
                .collect::<Vec<_>>(),
            [16, 82, 16]
        );
        assert_eq!(parsed.records[1].range, array_start..array_end);
        assert_eq!(parsed.records[2].identity_at, array_end + 2);
        assert_eq!(parsed.records[2].range.end, end);
        for truncated in array_start + 1..array_end {
            let partial = PrimaryRecords::read(ctx, &bytes[..truncated])
                .unwrap()
                .unwrap();
            assert_eq!(partial.records.len(), 1);
            assert!(
                matches!(partial.end, PrimaryEnd::Stopped { offset, .. } if offset == array_start)
            );
        }
    });
}

#[test]
fn primary_schema_edit_does_not_copy_omitted_tail() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, true);
        let start = bytes.len();
        bytes.extend_from_slice(&[0, 16, 1, b'C', b'Z', 0, 11]);
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());
        let first_end = bytes.len();
        bytes.extend_from_slice(&[0, 16, 0, 12]);
        bytes.extend_from_slice(&7u32.to_be_bytes());
        bytes.extend(END);
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(parsed.records.len(), 2);
        assert_eq!(parsed.records[0].range, start..first_end);
        assert_eq!(parsed.records[0].fields.len(), 1);
        assert_eq!(parsed.records[1].fields.len(), 1);
        assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
    });
}

#[test]
fn primary_new_nontransmitted_variable_field_still_has_a_count() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, true);
        bytes.extend_from_slice(&[0, 250, 1, 3]);
        bytes.extend_from_slice(b"NEW");
        bytes.push(4);
        bytes.extend_from_slice(b"Node");
        bytes.push(6);
        bytes.extend_from_slice(b"hidden");
        bytes.extend_from_slice(&[0, 0, 0, 2, 1, b'd', 0]);
        bytes.extend_from_slice(&123u32.to_be_bytes());
        bytes.extend_from_slice(&[0, 11]);
        let end = bytes.len();
        bytes.extend(END);
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].variable_length, Some(123));
        assert_eq!(parsed.records[0].fields.len(), 1);
        assert_eq!(parsed.records[0].fields[0], end..end);
        assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
    });
}

#[test]
fn primary_duplicate_identity_keeps_both_physical_records() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, true);
        bytes.extend(edge(true, 11));
        bytes.extend(edge(false, 11));
        bytes.extend(END);
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(parsed.records.len(), 2);
        assert_eq!(parsed.records[0].xmt, parsed.records[1].xmt);
        assert_eq!(parsed.records[0].range.end, parsed.records[1].range.start);
    });
}

#[test]
fn primary_missing_terminator_keeps_prefix_and_deltas_is_not_primary() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, true);
        bytes.extend(edge(true, 11));
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(
            parsed.end,
            PrimaryEnd::Stopped {
                offset: bytes.len(),
                reason: StopReason::MissingTerminator
            }
        );
        let mut deltas = header(13006, true);
        let at = deltas
            .windows(11)
            .position(|w| w == b"(partition)")
            .unwrap();
        deltas[at..at + 11].copy_from_slice(b"(deltas)   ");
        assert!(PrimaryRecords::read(ctx, &deltas).unwrap().is_none());
    });
}

#[test]
fn primary_extended_identity_is_not_a_first_use_escape() {
    with_decode_context(|ctx| {
        let compact = edge(false, 11);
        let mut bytes = header(13006, false);
        bytes.extend_from_slice(&compact[..2]);
        bytes.extend_from_slice(&[255, 254, 0, 1]);
        bytes.extend_from_slice(&compact[4..]);
        let end = bytes.len();
        bytes.extend(END);
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(u32::from(parsed.records[0].xmt), 32_769);
        assert_eq!(parsed.records[0].range.end, end);
        assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
    });
}

#[test]
fn primary_hvec_uses_three_double_values_per_element() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, true);
        bytes.extend_from_slice(&[0, 41, 255]);
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&[0, 11, b'T']);
        for value in 0..6 {
            bytes.extend_from_slice(&f64::from(value).to_be_bytes());
        }
        let end = bytes.len();
        bytes.extend(END);
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].range.end, end);
        assert_eq!(parsed.records[0].fields[1].len(), 48);
        assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
    });
}

#[test]
fn primary_unknown_scalar_stops_without_resynchronizing() {
    with_decode_context(|ctx| {
        for scalar in *b"qt" {
            let mut bytes = header(13006, true);
            bytes.extend(edge(true, 11));
            let start = bytes.len();
            bytes.extend_from_slice(&[0, 250, 1, 3]);
            bytes.extend_from_slice(b"NEW");
            bytes.push(4);
            bytes.extend_from_slice(b"Node");
            bytes.push(6);
            bytes.extend_from_slice(b"opaque");
            bytes.extend_from_slice(&[0, 0, 0, 1, 1, scalar, 0, 12]);
            bytes.extend(edge(false, 13));
            bytes.extend(END);
            let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
            assert_eq!(parsed.records.len(), 1);
            assert_eq!(
                parsed.end,
                PrimaryEnd::Stopped {
                    offset: start,
                    reason: StopReason::UnsupportedScalar
                }
            );
        }
    });
}

#[test]
fn primary_empty_unknown_scalar_array_needs_no_scalar_width() {
    with_decode_context(|ctx| {
        for scalar in *b"qt" {
            let mut bytes = header(13006, true);
            let start = bytes.len();
            bytes.extend_from_slice(&[0, 250, 1, 3]);
            bytes.extend_from_slice(b"NEW");
            bytes.push(4);
            bytes.extend_from_slice(b"Node");
            bytes.push(6);
            bytes.extend_from_slice(b"opaque");
            bytes.extend_from_slice(&[0, 0, 0, 2, 1, scalar, 1]);
            bytes.extend_from_slice(&0u32.to_be_bytes());
            bytes.extend_from_slice(&[0, 12]);
            let end = bytes.len();
            bytes.extend(group(true, 13, 4));
            bytes.extend(END);
            let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
            assert_eq!(parsed.records.len(), 2);
            assert_eq!(parsed.records[0].range, start..end);
            assert_eq!(parsed.records[0].fields[0], end..end);
            assert_eq!(parsed.groups(ctx).unwrap().len(), 1);
            assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
        }
    });
}

#[test]
fn primary_partition_description_needs_no_suffix() {
    with_decode_context(|ctx| {
        let description = b": TRANSMIT FILE (partition)";
        let original = header(13006, true);
        let old_length =
            usize::try_from(cadmpeg_core::decode::View::u32_be_at(&original, 2).unwrap()).unwrap();
        let mut bytes = b"PS".to_vec();
        bytes.extend_from_slice(&u32::try_from(description.len()).unwrap().to_be_bytes());
        bytes.extend(description);
        bytes.extend_from_slice(&original[6 + old_length..]);
        bytes.extend(group(true, 11, 4));
        bytes.extend(END);
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.groups(ctx).unwrap().len(), 1);
        assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
    });
}

#[test]
fn primary_empty_tag_values_preserves_following_group() {
    with_decode_context(|ctx| {
        for version in [11004, 13006] {
            for embedded in [false, true] {
                let mut bytes = header(version, embedded);
                bytes.extend_from_slice(&[0, 88]);
                if embedded {
                    bytes.push(255);
                }
                bytes.extend_from_slice(&0u32.to_be_bytes());
                bytes.extend_from_slice(&[0, 2]);
                bytes.extend(group(embedded, 3, 4));
                bytes.extend(END);
                let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
                assert_eq!(parsed.records.len(), 2);
                assert!(parsed.records[0].fields[0].is_empty());
                assert_eq!(parsed.groups(ctx).unwrap().len(), 1);
                assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
            }
        }
    });
}

#[test]
fn primary_zero_field_edit_owns_empty_instances() {
    with_decode_context(|ctx| {
        let mut bytes = header(13006, true);
        let start = bytes.len();
        bytes.extend_from_slice(&[0, 16, 0, b'Z', 0, 11, 0, 16, 0, 12]);
        let end = bytes.len();
        bytes.extend(END);
        let parsed = PrimaryRecords::read(ctx, &bytes).unwrap().unwrap();
        assert_eq!(parsed.records.len(), 2);
        assert_eq!(parsed.records[0].range, start..start + 6);
        assert_eq!(parsed.records[1].range, start + 6..end);
        assert!(parsed.records.iter().all(|record| record.fields.is_empty()));
        assert!(matches!(parsed.end, PrimaryEnd::Terminated { .. }));
        assert!(crate::records::read(ctx, &bytes[start..], 0)
            .unwrap()
            .is_none());
    });
}
