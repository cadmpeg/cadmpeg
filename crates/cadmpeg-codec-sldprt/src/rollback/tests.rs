// SPDX-License-Identifier: Apache-2.0
//! Synthetic tests of the current mark, the mark records and their join.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use cadmpeg_parasolid::records::DeltasRecords;
use cadmpeg_parasolid::test_support::test_pmarks;

use super::{
    join, mark_chain, mark_class_index, mark_records, token_naming_marks, MarkChain, MarkClass,
    MarkRecord, Reading, Unread,
};
use crate::carchive::reader::MarkOwner;
use crate::layout::pmark_record_head as head;

/// The map index of `moPMarkRecord_c` in the synthetic class maps.
const MARK_INDEX: u32 = 0x2b;
/// The node index of the initial mark in the synthetic deltas transmit.
const INITIAL_XMT: u32 = 40_000;
/// The current and initial mark ids `test_pmarks` writes.
const CURRENT_ID: u32 = 0x1234_5678;
const INITIAL_ID: u32 = 0x1234_5677;

/// A record head: mark, form and form word at their table offsets.
fn record_head(mark: u32, form: u32, form_word: u32) -> Vec<u8> {
    let mut bytes = vec![0; head::LEN];
    for (at, value) in [
        (head::MARK, mark),
        (head::FORM, form),
        (head::FORM_WORD, form_word),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// A `ResolvedFeatures` stream: the map count, a u16, then `objects`.
fn resolved(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = 9_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0, 0]);
    for object in objects {
        bytes.extend_from_slice(object);
    }
    bytes
}

fn short_tag() -> Vec<u8> {
    (0x8000 | u16::try_from(MARK_INDEX).unwrap())
        .to_le_bytes()
        .to_vec()
}

#[test]
fn records_follow_the_class_token_in_stream_order() {
    let filler = vec![0x11; 5];
    let objects = [
        filler.clone(),
        [short_tag(), record_head(7, 0, 0)].concat(),
        filler.clone(),
        [short_tag(), record_head(9, 1, 0)].concat(),
        [short_tag(), record_head(12, 0x00ab_cdef, 1)].concat(),
        [short_tag(), record_head(13, 7, 2)].concat(),
        filler,
    ];
    let stream = resolved(&objects);
    let marks = mark_records(&stream, MARK_INDEX)
        .iter()
        .map(|record| record.mark)
        .collect::<Vec<_>>();
    assert_eq!(marks, [7, 9, 12]);
    let first = mark_records(&stream, MARK_INDEX)[0];
    assert_eq!(
        first.at,
        6 + objects[0].len() + short_tag().len(),
        "a record's body starts after its class token"
    );
}

#[test]
fn a_class_index_past_the_short_range_takes_the_long_token() {
    let index = 0x9000_u32;
    let mut tag = 0x7fff_u16.to_le_bytes().to_vec();
    tag.extend_from_slice(&(0x8000_0000 | index).to_le_bytes());
    let stream = resolved(&[[tag, record_head(31, 0, 0)].concat()]);
    assert_eq!(
        mark_records(&stream, index),
        [MarkRecord {
            at: 6 + 6,
            mark: 31
        }]
    );
    assert!(mark_records(&stream, MARK_INDEX).is_empty());
}

/// A deltas transmit at the current mark whose preceding mark is the initial
/// mark.
fn two_mark_transmit() -> Vec<u8> {
    let mut bytes = test_pmarks::prefix(INITIAL_XMT);
    bytes.extend(test_pmarks::TERMINATOR);
    bytes.extend(test_pmarks::EMPTY_MAP);
    bytes.extend(test_pmarks::TERMINATOR);
    bytes.extend(test_pmarks::node(INITIAL_XMT, 1, 49_948, INITIAL_ID));
    bytes.extend(test_pmarks::TERMINATOR);
    bytes.extend(test_pmarks::EMPTY_MAP);
    bytes.extend(test_pmarks::TERMINATOR);
    bytes.extend(test_pmarks::TERMINATOR);
    bytes
}

#[test]
fn the_deltas_prefix_gives_the_current_mark_and_the_main_line() {
    let bytes = two_mark_transmit();
    let records = DeltasRecords::read(&bytes).unwrap();
    assert_eq!(
        mark_chain(&records),
        Some(MarkChain {
            current: CURRENT_ID,
            main: vec![CURRENT_ID, INITIAL_ID],
        })
    );
    let lone = test_pmarks::complete(test_pmarks::prefix(1));
    assert_eq!(
        mark_chain(&DeltasRecords::read(&lone).unwrap()),
        Some(MarkChain {
            current: CURRENT_ID,
            main: vec![CURRENT_ID],
        })
    );
    assert_eq!(mark_chain(&DeltasRecords::read(b"XX").unwrap()), None);
}

fn chain(main: &[u32]) -> MarkChain {
    MarkChain {
        current: main[0],
        main: main.to_vec(),
    }
}

fn records(marks: &[u32]) -> Vec<MarkRecord> {
    marks
        .iter()
        .enumerate()
        .map(|(index, &mark)| MarkRecord {
            at: 100 * (index + 1),
            mark,
        })
        .collect()
}

fn owners(names: &[Option<&str>]) -> Vec<MarkOwner> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| MarkOwner {
            at: 100 * (index + 1),
            end: 100 * (index + 1) + 50,
            owner: name.map(str::to_owned),
        })
        .collect()
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn the_current_mark_at_the_last_record_is_the_end() {
    let marks = chain(&[11, 8, 5, 1]);
    let held = owners(&[Some("Boss-Extrude1"), None, Some("Fillet1")]);
    assert_eq!(
        join(&marks, &records(&[5, 8, 11]), &held),
        Reading::End {
            last: "Fillet1".to_owned(),
            recorded: names(&["Boss-Extrude1", "Fillet1"]),
        }
    );
    assert_eq!(
        join(&marks, &records(&[5, 8, 11]), &held[..2]),
        Reading::Unread(Unread::HolderUnresolved { current: 11 }),
        "the End reading names the feature that holds the last record"
    );
}

#[test]
fn the_current_mark_at_an_earlier_record_rolls_back_after_its_feature() {
    let holders = [
        Some("Boss-Extrude1"),
        Some("Cut-Extrude1"),
        Some("Fillet1"),
        Some("Chamfer1"),
    ];
    let reading = join(
        &chain(&[8, 5, 1]),
        &records(&[5, 8, 14, 17]),
        &owners(&holders),
    );
    assert_eq!(
        reading,
        Reading::RolledBack {
            after: "Cut-Extrude1".to_owned(),
            later: vec!["Fillet1".to_owned(), "Chamfer1".to_owned()],
            recorded: names(&["Boss-Extrude1", "Cut-Extrude1", "Fillet1", "Chamfer1"]),
        }
    );
}

#[test]
fn marks_that_fix_no_position_name_their_reason() {
    let walk = owners(&[Some("A"), None, Some("C")]);
    assert_eq!(
        join(&chain(&[14, 11, 8, 1]), &records(&[8, 11]), &walk),
        Reading::Unread(Unread::AfterLastFeature {
            current: 14,
            last: 11
        })
    );
    assert_eq!(
        join(&chain(&[14, 1]), &records(&[8, 11]), &walk),
        Reading::Unread(Unread::NoFeatureMark { current: 14 })
    );
    assert_eq!(
        join(&chain(&[11, 8, 1]), &records(&[11, 8]), &walk),
        Reading::Unread(Unread::LaterMarkOnMainLine { current: 11 })
    );
    assert_eq!(
        join(&chain(&[8, 1]), &records(&[8, 21, 24]), &walk),
        Reading::Unread(Unread::HolderUnresolved { current: 8 })
    );
    assert_eq!(
        join(&chain(&[8, 1]), &[], &walk),
        Reading::Unread(Unread::NoMarkRecord)
    );
}

fn tagged(index: u16, mark: u32) -> Vec<u8> {
    [
        (0x8000 | index).to_le_bytes().to_vec(),
        record_head(mark, 0, 0),
    ]
    .concat()
}

#[test]
fn an_unreconciled_map_takes_the_token_whose_records_name_the_most_marks() {
    let marks = chain(&[11, 8, 5, 1]);
    let stream = resolved(&[
        tagged(0x2b, 5),
        tagged(0x30, 5),
        tagged(0x2b, 8),
        tagged(0x31, 1),
        tagged(0x31, 1),
        tagged(0x2b, 11),
    ]);
    assert_eq!(token_naming_marks(&stream, Some(0x40), &marks), Some(0x2b));
    assert_eq!(
        token_naming_marks(&stream, Some(0x2b), &marks),
        None,
        "a token at or past the map count names no class of the configuration"
    );
    assert_eq!(
        token_naming_marks(&stream, None, &marks),
        Some(0x2b),
        "a stream that states no map count bounds no index"
    );
    let tied = resolved(&[tagged(0x2b, 5), tagged(0x30, 8)]);
    assert_eq!(token_naming_marks(&tied, Some(0x40), &marks), None);
    let initial = resolved(&[tagged(0x31, 1)]);
    assert_eq!(
        token_naming_marks(&initial, Some(0x40), &marks),
        None,
        "no feature records the initial mark"
    );
}

#[test]
fn a_token_whose_heads_mostly_miss_the_main_line_names_no_record() {
    let marks = chain(&[4, 2, 1]);
    let sequence = [2, 3, 4, 5].map(|value| tagged(0x30, value));
    let stream = resolved(&[sequence.concat(), tagged(0x2b, 4)]);
    assert_eq!(token_naming_marks(&stream, None, &marks), Some(0x2b));
}

#[test]
fn a_reconciled_map_without_the_mark_class_bounds_the_token_scan_by_its_count() {
    let marks = chain(&[11, 8, 5, 1]);
    let stream = resolved(&[tagged(0x2b, 5), tagged(0x2b, 8), tagged(0x2b, 11)]);
    let map = std::collections::BTreeMap::from([(0x10, "moOtherRecord_c".to_owned())]);
    assert_eq!(
        mark_class_index(Some(&map), Some(0x2b), &stream, &marks),
        Err(Unread::NoMarkClass),
        "a token at or past the map count names no class of the configuration"
    );
    assert_eq!(
        mark_class_index(Some(&map), Some(0x40), &stream, &marks),
        Ok(MarkClass::Scanned(0x2b))
    );
    assert_eq!(
        mark_class_index(None, Some(0x40), &stream, &marks),
        Ok(MarkClass::Scanned(0x2b))
    );
}

#[test]
fn a_map_that_names_the_mark_class_gives_its_index() {
    let marks = chain(&[11, 8, 5, 1]);
    let stream = resolved(&[tagged(0x2b, 5), tagged(0x2b, 8), tagged(0x2b, 11)]);
    let map = std::collections::BTreeMap::from([(0x30, "moPMarkRecord_c".to_owned())]);
    assert_eq!(
        mark_class_index(Some(&map), Some(0x40), &stream, &marks),
        Ok(MarkClass::Mapped(0x30)),
        "the map's index stands over the token the scan would take"
    );
}
