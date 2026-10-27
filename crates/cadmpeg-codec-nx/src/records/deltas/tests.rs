// SPDX-License-Identifier: Apache-2.0
use super::{DeltasEnd, DeltasRecords, EventContent, StopReason};
use crate::test_support::{owned_deltas, test_pmarks, with_decode_context};

#[test]
fn all_documented_body_plans_share_first_and_ordinary_field_boundaries() {
    for form in [27, 29, 34, 35, 36] {
        let (declaration, ordinary, fields) = owned_deltas::body(form);
        let mut bytes = test_pmarks::prefix(1);
        let first_start = bytes.len();
        bytes.extend(&declaration);
        bytes.extend_from_slice(&ordinary[2..]);
        let ordinary_start = bytes.len();
        bytes.extend(&ordinary);
        bytes.extend(test_pmarks::TERMINATOR);
        let expected_end = bytes.len();
        bytes.extend([0xde, 0xad, 0, 12, 255]);
        with_decode_context(|_ctx| {
            let owner = DeltasRecords::read(&bytes).unwrap();
            assert_eq!(owner.definitions().len(), 3);
            assert_eq!(owner.census().instance_count(), 2);
            assert_eq!(owner.suffix(), expected_end..bytes.len());
            assert_eq!(owner.census().bytes_decoded(), expected_end);
            for (event, start, identity) in [
                (
                    &owner.events()[2],
                    first_start,
                    first_start + declaration.len(),
                ),
                (&owner.events()[3], ordinary_start, ordinary_start + 2),
            ] {
                let EventContent::Instance(instance) = event.content() else {
                    panic!("instance");
                };
                assert_eq!(event.range().start, start);
                assert_eq!(instance.identity_at(), identity);
                assert_eq!(instance.definition().plan().fields.len(), usize::from(form));
                let mut at = identity + 2;
                for ((span, descriptor), (name, width)) in instance
                    .fields()
                    .iter()
                    .zip(&instance.definition().plan().fields)
                    .zip(&fields)
                {
                    assert_eq!(descriptor.name.as_str(), *name);
                    assert_eq!(*span, at..at + width);
                    at += width;
                }
                assert_eq!(at, event.range().end);
            }
            let (EventContent::Instance(first), EventContent::Instance(second)) =
                (owner.events()[2].content(), owner.events()[3].content())
            else {
                panic!("instances");
            };
            assert!(std::ptr::eq(first.definition(), second.definition()));
        });
    }
}

#[test]
fn incomplete_first_use_commits_neither_definition_nor_instance() {
    let prefix = test_pmarks::prefix(1);
    let bytes = owned_deltas::body_stream(36);
    for end in prefix.len() + 1..bytes.len() {
        with_decode_context(|_ctx| {
            let owner = DeltasRecords::read(&bytes[..end]).unwrap();
            assert_eq!(owner.definitions().len(), 2, "end {end}");
            assert_eq!(owner.events().len(), 2, "end {end}");
            assert_eq!(owner.suffix(), prefix.len()..end);
        });
    }
}

#[test]
fn prefix_is_transactional_and_preserves_complete_header() {
    let bytes = test_pmarks::prefix(1);
    for end in test_pmarks::header().len()..bytes.len() {
        with_decode_context(|_ctx| {
            let owner = DeltasRecords::read(&bytes[..end]).unwrap();
            assert_eq!(owner.events().len(), 1);
            assert!(owner.definitions().is_empty());
            assert_eq!(owner.suffix(), test_pmarks::header().len()..end);
        });
    }
}

#[test]
fn no_resynchronization_or_reference_based_ownership_after_a_stop() {
    for gap in [vec![0xde, 0xad], vec![0, 1, 0, 1], test_pmarks::packet()] {
        let mut bytes = test_pmarks::prefix(1);
        let closed = gap.starts_with(&test_pmarks::TERMINATOR);
        let stop = bytes.len() + if closed { 4 } else { 0 };
        bytes.extend(gap);
        let (declaration, ordinary, _) = owned_deltas::body(36);
        bytes.extend(declaration);
        bytes.extend(&ordinary[2..]);
        with_decode_context(|_ctx| {
            let owner = DeltasRecords::read(&bytes).unwrap();
            assert_eq!(
                owner.end(),
                &DeltasEnd::Stopped {
                    offset: stop,
                    reason: if closed {
                        StopReason::InvalidMap
                    } else {
                        StopReason::UnsupportedPacket
                    }
                }
            );
            assert_eq!(owner.events().len(), if closed { 3 } else { 2 });
            assert_eq!(owner.suffix(), stop..bytes.len());
        });
    }
}

#[test]
fn headerless_and_unsupported_bases_own_no_bytes() {
    let (declaration, ordinary, _) = owned_deltas::body(36);
    let mut headerless = declaration;
    headerless.extend(&ordinary[2..]);
    let mut unsupported = owned_deltas::body_stream(36);
    let at = unsupported.windows(5).position(|w| w == b"13006").unwrap();
    unsupported[at..at + 5].copy_from_slice(b"11004");
    for bytes in [headerless, unsupported] {
        with_decode_context(|_ctx| {
            let owner = DeltasRecords::read(&bytes).unwrap();
            assert!(owner.events().is_empty());
            assert_eq!(owner.suffix(), 0..bytes.len());
        });
    }
}

#[test]
fn positive_integer_and_status_admission_are_independent() {
    let prefix = test_pmarks::prefix(1);
    let region = owned_deltas::region(7, 3, b'S');
    // REGION identity and first pointer positions are literal fixture offsets.
    for (at, width, replacement, admitted) in [
        (3, 2, vec![0, 0], false),
        (3, 2, vec![0, 1], false),
        (3, 2, vec![0xff, 0xff, 0, 0], false),
        (3, 2, vec![0xff, 0xff, 0, 1], true),
        (3, 2, vec![0x80, 0, 0, 1], true),
        (9, 2, vec![0, 0], false),
        (9, 2, vec![0, 1], true),
        (9, 2, vec![0xff, 0xff, 0, 0], false),
        (9, 2, vec![0xff, 0xff, 0, 1], true),
        (11, 1, vec![0], true),
        (11, 1, vec![1], true),
        (11, 1, vec![2], false),
    ] {
        let mut changed = region.clone();
        changed.splice(at..at + width, replacement);
        let mut bytes = prefix.clone();
        bytes.extend(changed);
        let bytes = test_pmarks::complete(bytes);
        with_decode_context(|_ctx| {
            let owner = DeltasRecords::read(&bytes).unwrap();
            assert_eq!(owner.census().instance_count(), usize::from(admitted));
            assert_eq!(
                owner.suffix().start,
                if admitted { bytes.len() } else { prefix.len() }
            );
        });
    }
}

#[test]
fn counted_opaque_fields_own_embedded_schema_and_record_markers() {
    let mut payload = test_pmarks::complete(owned_deltas::body_stream(36));
    payload.extend([0, 19, 255, 0, 3]);
    let mut bytes = test_pmarks::prefix(1);
    // ENTITY_54 base: one variable character field, followed by a BODY-shaped payload.
    bytes.extend([0, 84, 255]);
    bytes.extend(u32::try_from(payload.len()).unwrap().to_be_bytes());
    bytes.extend([0, 9]);
    bytes.extend(payload);
    let bytes = test_pmarks::complete(bytes);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.census().instance_count(), 1);
        assert_eq!(owner.definitions().last().unwrap().plan().kind, 84);
        assert_eq!(owner.end(), &DeltasEnd::Complete);
    });
}

#[test]
fn all_world_layouts_share_the_same_first_and_ordinary_instance_decoder() {
    for (count, length, pointers) in [(15, 48, 11), (18, 57, 14), (19, 60, 15)] {
        let mut bytes = test_pmarks::prefix(1);
        let first = bytes.len();
        bytes.extend(crate::test_support::world::first_node(count));
        let second = bytes.len();
        bytes.extend(crate::test_support::world::record(count, false));
        let end = bytes.len();
        bytes.extend(test_pmarks::TERMINATOR);
        bytes.extend([0xde, 0xad]);
        with_decode_context(|_ctx| {
            let owner = crate::records::DeltasRecords::read(&bytes).unwrap();
            assert_eq!(owner.census().instance_count(), 2);
            assert_eq!(owner.events()[2].range(), first..second);
            assert_eq!(owner.events()[3].range(), second..end);
            let EventContent::Instance(instance) = owner.events()[3].content() else {
                panic!("WORLD");
            };
            assert_eq!(
                instance
                    .fields()
                    .iter()
                    .map(std::iter::ExactSizeIterator::len)
                    .sum::<usize>()
                    + 2,
                length
            );
            assert_eq!(
                instance
                    .definition()
                    .plan()
                    .fields
                    .iter()
                    .filter(|f| f.data_type == crate::records::DataType::Pointer)
                    .count(),
                pointers
            );
            assert_eq!(owner.suffix(), end + 4..bytes.len());
        });
    }
}

#[test]
fn block_identity_and_storage_use_the_generic_owner() {
    let mut bytes = crate::test_support::test_pmarks::prefix(1);
    let start = bytes.len();
    bytes.extend(b"\0\x4a\x04CI\x10index_map_offset\0\0\0\x01\x01dCCZ");
    bytes.extend(1u32.to_be_bytes());
    bytes.extend([0, 20]);
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(0u32.to_be_bytes());
    bytes.extend([0, 21, 1, 0, 30, 1]);
    let instance_end = bytes.len();
    let bytes = test_pmarks::complete(bytes);
    crate::test_support::with_decode_context(|_ctx| {
        let owner = crate::records::DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.census().instance_count(), 1);
        assert_eq!(owner.events()[2].range(), start..instance_end);
        for end in start + 1..instance_end {
            assert_eq!(
                crate::records::DeltasRecords::read(&bytes[..end])
                    .unwrap()
                    .census()
                    .instance_count(),
                0
            );
        }
    });
}

#[test]
fn constructed_base_body_ff_and_ordinary_extended_identity_share_one_plan() {
    let (_, ordinary, fields) = owned_deltas::body(27);
    let end = 4 + fields[..23].iter().map(|(_, width)| width).sum::<usize>();
    let mut bytes = test_pmarks::prefix(1);
    let first = bytes.len();
    bytes.extend([0, 12, 255]);
    bytes.extend(&ordinary[2..end]);
    let second = bytes.len();
    bytes.extend([0, 12, 255, 255, 0, 1]);
    bytes.extend(&ordinary[4..end]);
    let bytes = test_pmarks::complete(bytes);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.census().instance_count(), 2);
        assert_eq!(owner.definitions()[2].declaration(), first..first + 3);
        assert_eq!(owner.definitions()[2].plan().fields.len(), 23);
        let EventContent::Instance(instance) = owner.events()[3].content() else {
            panic!("BODY");
        };
        assert_eq!(u32::from(instance.xmt()), 32768);
        assert_eq!(instance.identity_at(), second + 2);
        assert_eq!(owner.suffix().start, bytes.len());
    });
}

#[test]
fn custom_type_metadata_and_identifier_values_do_not_change_boundaries() {
    let mut prefix = test_pmarks::prefix(1);
    let maximum = test_pmarks::header().len() - 6;
    prefix[maximum..maximum + 2].copy_from_slice(&500u16.to_be_bytes());
    // Custom type 240, one integer field; no base-edit terminator belongs here.
    let declaration = b"\0\xf0\x01\x07MY_KIND\x06custom\x05value\0\0\0\x01\x01d";
    for identity in [2u16, 32000] {
        let mut bytes = prefix.clone();
        bytes.extend(declaration);
        bytes.extend(identity.to_be_bytes());
        bytes.extend(u32::MAX.to_be_bytes());
        let first_end = bytes.len();
        bytes.extend([0, 240]);
        bytes.extend((identity + 1).to_be_bytes());
        bytes.extend(7u32.to_be_bytes());
        let instance_end = bytes.len();
        let bytes = test_pmarks::complete(bytes);
        with_decode_context(|_ctx| {
            let owner = DeltasRecords::read(&bytes).unwrap();
            assert_eq!(owner.census().instance_count(), 2);
            let plan = owner.definitions()[2].plan();
            assert_eq!(plan.custom_name.as_deref(), Some("MY_KIND"));
            assert_eq!(plan.custom_description.as_deref(), Some("custom"));
            assert_eq!(owner.events()[2].range(), prefix.len()..first_end);
            assert_eq!(owner.events()[3].range(), first_end..instance_end);
        });
    }
}

#[test]
fn list_fields_use_declared_storage_and_reject_every_truncation_and_status_two() {
    let payload = [
        0, 7, 0, 0, 0, 0, 4, 0, 0, 3, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 2, 0, 0, 0, 20, 0, 0, 0, 21, 0,
        53, 0, 0, 52, 0,
    ];
    let mut prefix = test_pmarks::prefix(1);
    let start = prefix.len();
    // Constructed modern field order; ff would select the different V13 base LIST.
    prefix.extend(owned_deltas::hex("00460b44444444444444444444444441076e6f64655f696400000001016441096c6973745f74797065000000010175410a6e6f7472616e736d697400000001016c41056f776e657203f5000141046e65787400460001410870726576696f757300460001410b6c6973745f6c656e677468000000010164410c626c6f636b5f6c656e677468000000010164410c66696e6765725f696e646578000000010164410c66696e6765725f626c6f636b03f40001410a6c6973745f626c6f636b03f400015a"));
    let mut bytes = prefix.clone();
    bytes.extend(payload);
    let instance_end = bytes.len();
    let bytes = test_pmarks::complete(bytes);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.census().instance_count(), 1);
        assert_eq!(owner.events()[2].range(), start..instance_end);
        for end in start + 1..instance_end {
            assert_eq!(
                DeltasRecords::read(&bytes[..end])
                    .unwrap()
                    .census()
                    .instance_count(),
                0
            );
        }
        for status in [10, 13, 16, 31, 34] {
            let mut bad = bytes.clone();
            bad[prefix.len() + status] = 2;
            assert_eq!(
                DeltasRecords::read(&bad).unwrap().census().instance_count(),
                0
            );
        }
    });
}

#[test]
fn plain_base_header_and_explicit_base_edits_use_the_same_prefix_fields() {
    let original = test_pmarks::prefix(1);
    let header_end = test_pmarks::header().len();
    let pmark_at = header_end + 15;
    let mut plain = b"PS".to_vec();
    for text in [b"Transmit (deltas)".as_slice(), b"SCH_3501171_13006"] {
        plain.extend(u32::try_from(text.len()).unwrap().to_be_bytes());
        plain.extend(text);
    }
    plain.extend(0u32.to_be_bytes());
    plain.extend([0, 3]);
    plain.extend(&original[header_end + 3..pmark_at]);
    plain.extend([0, 4]);
    plain.extend(&original[pmark_at + 3..]);
    let mut edited = original.clone();
    edited.splice(
        pmark_at + 2..pmark_at + 3,
        [
            10, b'C', b'C', b'C', b'C', b'C', b'C', b'C', b'C', b'C', b'C', b'Z',
        ],
    );
    edited.splice(header_end + 2..header_end + 3, [2, b'C', b'C', b'Z']);
    for bytes in [original, plain, edited] {
        let bytes = test_pmarks::complete(bytes);
        with_decode_context(|_ctx| {
            let owner = DeltasRecords::read(&bytes).unwrap();
            assert_eq!(owner.events().len(), 6);
            assert_eq!(owner.definitions().len(), 2);
            assert_eq!(owner.end(), &DeltasEnd::Complete);
            let EventContent::PartitionPrefix(state) = owner.events()[1].content() else {
                panic!("prefix");
            };
            let value = serde_json::to_value(state).unwrap();
            assert_eq!(value["partition"]["xmt"], 49947);
            assert_eq!(value["current_pmark"]["xmt"], 49948);
            assert_eq!(value["current_pmark"]["identifier"], 0x1234_5678u32);
        });
    }
}

fn assert_stop(
    bytes: &[u8],
    offset: usize,
    reason: StopReason,
    instances: usize,
    definitions: usize,
) {
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(bytes).unwrap();
        assert_eq!(owner.end(), &DeltasEnd::Stopped { offset, reason });
        assert_eq!(owner.suffix(), offset..bytes.len());
        assert_eq!(owner.census().instance_count(), instances);
        assert_eq!(owner.definitions().len(), definitions);
        assert_eq!(
            owner
                .events()
                .iter()
                .map(|event| event.range().len())
                .sum::<usize>(),
            offset
        );
    });
}

fn map_start() -> Vec<u8> {
    let mut bytes = test_pmarks::prefix(1);
    bytes.extend(test_pmarks::TERMINATOR);
    bytes
}

#[test]
fn owner_rejects_non_deltas_description_at_its_own_boundary() {
    let mut bytes = test_pmarks::complete(test_pmarks::prefix(1));
    let at = bytes
        .windows(8)
        .position(|window| window == b"(deltas)")
        .unwrap();
    bytes[at..at + 8].copy_from_slice(b"(deltaz)");
    assert_stop(&bytes, 0, StopReason::InvalidHeader, 0, 0);
}

#[test]
fn enclosure_requires_closed_sections_and_its_exact_end_form() {
    let prefix = test_pmarks::prefix(1);
    assert_stop(&prefix, prefix.len(), StopReason::UnclosedSection, 0, 2);
    let mut bytes = map_start();
    assert_stop(&bytes, bytes.len(), StopReason::MissingSection, 0, 2);
    let map = bytes.len();
    bytes.extend(test_pmarks::EMPTY_MAP);
    assert_stop(&bytes, map, StopReason::UnclosedSection, 0, 2);
    bytes.extend(test_pmarks::TERMINATOR);
    assert_stop(&bytes, bytes.len(), StopReason::MissingSection, 0, 2);
    let final_at = bytes.len();
    bytes.extend(test_pmarks::TERMINATOR);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.end(), &DeltasEnd::Complete);
        assert!(matches!(
            owner.events().last().unwrap().content(),
            EventContent::Metadata(super::Metadata::FinalTerminator)
        ));
    });
    bytes.push(0);
    assert_stop(&bytes, final_at, StopReason::UnsupportedPacket, 0, 2);
}

#[test]
fn maps_require_canonical_entries_zero_slot_bounded_kind_and_word() {
    let prefix = map_start();
    let start = prefix.len();
    for (map, reason) in [
        (vec![0, 0], StopReason::InvalidMap),
        (vec![255, 255, 0, 0], StopReason::InvalidMap),
        (vec![128, 0, 0, 1], StopReason::InvalidMap),
        (vec![0, 2, 0, 1], StopReason::InvalidMap),
        (vec![0, 2, 0, 240], StopReason::InvalidMap),
        (vec![0, 1, 0, 2, 0, 0], StopReason::InvalidMap),
        (vec![0, 1, 0, 0, 128, 0], StopReason::UnresolvedMapWordWidth),
    ] {
        let mut bytes = prefix.clone();
        bytes.extend(map);
        bytes.extend(test_pmarks::TERMINATOR);
        bytes.extend(test_pmarks::TERMINATOR);
        assert_stop(&bytes, start, reason, 0, 2);
    }
    let map = [0xff, 0xff, 0, 1, 0, 2, 0, 7, 0, 239, 0, 1, 0, 0, 0x7f, 0xff];
    for end in 0..map.len() {
        let mut bytes = prefix.clone();
        bytes.extend(&map[..end]);
        assert_stop(
            &bytes,
            start,
            if end == 0 {
                StopReason::MissingSection
            } else {
                StopReason::InvalidMap
            },
            0,
            2,
        );
    }
    let mut bytes = prefix;
    bytes.extend(map);
    for tail in [vec![], vec![0], vec![0, 1], vec![0, 1, 0]] {
        let mut truncated = bytes.clone();
        truncated.extend(tail);
        assert_stop(&truncated, start, StopReason::UnclosedSection, 0, 2);
    }
    bytes.extend(test_pmarks::TERMINATOR);
    bytes.extend(test_pmarks::TERMINATOR);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.end(), &DeltasEnd::Complete);
        let EventContent::Metadata(super::Metadata::Map {
            entries,
            terminal_word,
        }) = owner.events()[3].content()
        else {
            panic!("map");
        };
        assert_eq!(
            serde_json::to_value(entries).unwrap(),
            serde_json::json!([[32768, 2], [7, 239]])
        );
        assert_eq!(u16::from(*terminal_word), 0x7fff);
    });
}

#[test]
fn pmark_sections_accept_repeated_heads_then_nodes_and_eof() {
    let mut bytes = map_start();
    bytes.extend(test_pmarks::EMPTY_MAP);
    bytes.extend(test_pmarks::TERMINATOR);
    let start = bytes.len();
    let mark = test_pmarks::node(40_000, 1, 49_948, 0x0001_0000);
    for end in 2..mark.len() {
        let mut truncated = bytes.clone();
        truncated.extend(&mark[..end]);
        assert_stop(&truncated, start, StopReason::InvalidPmark, 0, 2);
    }
    bytes.extend(&mark);
    bytes.extend(&mark);
    let region = owned_deltas::region(7, 3, b'S');
    bytes.extend(&region);
    let terminator = bytes.len();
    bytes.extend(test_pmarks::TERMINATOR);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.end(), &DeltasEnd::Complete);
        assert_eq!(owner.census().instance_count(), 1);
        assert_eq!(
            owner
                .events()
                .iter()
                .filter(|event| matches!(
                    event.content(),
                    EventContent::Metadata(super::Metadata::Pmark(_))
                ))
                .count(),
            2
        );
    });
    let mut late = bytes[..terminator].to_vec();
    late.extend(mark);
    late.extend(test_pmarks::TERMINATOR);
    assert_stop(&late, start, StopReason::UnsupportedPacket, 0, 2);
    for tail in [vec![0, 4], test_pmarks::EMPTY_MAP.to_vec()] {
        let mut unsupported = map_start();
        let map = unsupported.len();
        unsupported.extend(test_pmarks::EMPTY_MAP);
        unsupported.extend(tail);
        unsupported.extend(test_pmarks::TERMINATOR);
        assert_stop(&unsupported, map, StopReason::UnsupportedPacket, 0, 2);
    }
}

#[test]
fn enclosing_transition_selects_pmark_when_its_fields_also_form_a_map_prefix() {
    let mut mark = test_pmarks::node(12, 6, 81, 0x0001_0000);
    mark[8..12].copy_from_slice(&[0, 5, 0, 82]);
    mark[12..24].copy_from_slice(&[0, 1, 0, 0, 0, 1, 0, 0, 0, 4, 0, 5]);
    with_decode_context(|_ctx| {
        let mut local = cadmpeg_core::decode::View::over_retained(&mark);
        assert!(super::read_map(&mut local, 239).is_ok());
        assert_eq!(local.position(), 18);
        let mut bytes = map_start();
        bytes.extend(test_pmarks::EMPTY_MAP);
        bytes.extend(test_pmarks::TERMINATOR);
        let start = bytes.len();
        bytes.extend(&mark);
        bytes.extend(test_pmarks::TERMINATOR);
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.end(), &DeltasEnd::Complete);
        let event = owner
            .events()
            .iter()
            .find(|event| event.range().start == start)
            .unwrap();
        assert_eq!(event.range(), start..start + mark.len());
        assert!(matches!(
            event.content(),
            EventContent::Metadata(super::Metadata::Pmark(_))
        ));
    });
}

#[test]
fn first_uses_are_staged_until_closure_and_shared_across_sections() {
    let mut bytes = map_start();
    let start = bytes.len();
    bytes.extend(test_pmarks::EMPTY_MAP);
    bytes.extend(owned_deltas::region(7, 3, b'S'));
    assert_stop(&bytes, start, StopReason::UnclosedSection, 0, 2);
    let (_, ordinary, _) = owned_deltas::body(36);
    let mut failed = bytes.clone();
    failed.extend(ordinary);
    failed.extend(test_pmarks::TERMINATOR);
    assert_stop(&failed, start, StopReason::InvalidDeclaration, 0, 2);
    bytes.extend(test_pmarks::TERMINATOR);
    let pmark_start = bytes.len();
    bytes.extend(test_pmarks::node(9, 1, 1, 0));
    let mut region = owned_deltas::region(7, 4, b'V');
    region.remove(2);
    bytes.extend(region);
    assert_stop(&bytes, pmark_start, StopReason::UnclosedSection, 1, 3);
    bytes.extend(test_pmarks::TERMINATOR);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.end(), &DeltasEnd::Complete);
        assert_eq!(owner.census().instance_count(), 2);
        let instances = owner
            .events()
            .iter()
            .filter_map(|event| match event.content() {
                EventContent::Instance(instance) => Some(instance),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(std::ptr::eq(
            instances[0].definition(),
            instances[1].definition()
        ));
    });
    let mut cycled = bytes.clone();
    cycled.extend(test_pmarks::EMPTY_MAP);
    cycled.extend(test_pmarks::TERMINATOR);
    cycled.extend(test_pmarks::TERMINATOR);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&cycled).unwrap();
        assert_eq!(owner.end(), &DeltasEnd::Complete);
        assert_eq!(owner.census().instance_count(), 2);
        assert_eq!(
            owner
                .events()
                .iter()
                .filter(|event| matches!(
                    event.content(),
                    EventContent::Metadata(super::Metadata::Map { .. })
                ))
                .count(),
            2
        );
    });
    bytes.extend([0xde, 0xad]);
    let stop = bytes.len() - 2;
    bytes.extend(test_pmarks::EMPTY_MAP);
    bytes.extend(test_pmarks::TERMINATOR);
    assert_stop(&bytes, stop, StopReason::InvalidMap, 2, 3);
}

#[test]
fn unsupported_heads_and_invalid_prefix_or_pmark_fields_do_not_commit_sections() {
    for head in [
        vec![0, 0],
        vec![0, 1, 0, 2],
        vec![0, 3],
        vec![0, 4],
        vec![0, 240],
    ] {
        let mut bytes = test_pmarks::prefix(1);
        let start = bytes.len();
        bytes.extend(head);
        bytes.extend(test_pmarks::TERMINATOR);
        assert_stop(&bytes, start, StopReason::UnsupportedPacket, 0, 2);
    }
    let mut header = test_pmarks::prefix(1);
    let user_fields = test_pmarks::header().len() - 4;
    header[user_fields..user_fields + 4].copy_from_slice(&1u32.to_be_bytes());
    assert_stop(&header, 0, StopReason::InvalidHeader, 0, 0);
    let mut prefix = test_pmarks::prefix(1);
    let logical = prefix.len() - 5;
    prefix[logical] = 2;
    assert_stop(
        &prefix,
        test_pmarks::header().len(),
        StopReason::UnsupportedPacket,
        0,
        0,
    );
    let mut bytes = map_start();
    bytes.extend(test_pmarks::EMPTY_MAP);
    bytes.extend(test_pmarks::TERMINATOR);
    let start = bytes.len();
    let mark = test_pmarks::node(8, 1, 1, 0);
    for replacement in [vec![255, 255, 0, 0], vec![0, 1]] {
        let mut changed = mark.clone();
        changed.splice(2..4, replacement);
        let mut invalid = bytes.clone();
        invalid.extend(changed);
        invalid.extend(test_pmarks::TERMINATOR);
        assert_stop(&invalid, start, StopReason::InvalidPmark, 0, 2);
    }
    bytes.extend(mark);
    let (declaration, ordinary, _) = owned_deltas::body(36);
    bytes.extend(declaration);
    bytes.extend(&ordinary[2..]);
    assert_stop(&bytes, start, StopReason::UnclosedSection, 0, 2);
    bytes.extend(test_pmarks::TERMINATOR);
    with_decode_context(|_ctx| {
        let owner = DeltasRecords::read(&bytes).unwrap();
        assert_eq!(owner.census().instance_count(), 1);
        assert_eq!(owner.definitions().len(), 3);
        assert_eq!(owner.end(), &DeltasEnd::Complete);
    });
    let end = bytes.len();
    bytes.extend(test_pmarks::node(9, 1, 1, 0));
    assert_stop(&bytes, end, StopReason::InvalidMap, 1, 3);
}

#[test]
fn schema_edit_counts_and_variable_count_overflow_rollback_first_use_state() {
    for count in [35, 37] {
        let (mut declaration, ordinary, _) = owned_deltas::body(36);
        declaration[2] = count;
        let mut bytes = test_pmarks::prefix(1);
        let start = bytes.len();
        bytes.extend(declaration);
        bytes.extend(&ordinary[2..]);
        let bytes = test_pmarks::complete(bytes);
        assert_stop(&bytes, start, StopReason::InvalidDeclaration, 0, 2);
    }
    let mut bytes = test_pmarks::prefix(1);
    let start = bytes.len();
    bytes.extend([0, 2, 255]);
    bytes.extend(u32::MAX.to_be_bytes());
    bytes.extend([0, 8, b'W']);
    let bytes = test_pmarks::complete(bytes);
    assert_stop(&bytes, start, StopReason::InvalidInstance, 0, 2);
}
