// SPDX-License-Identifier: Apache-2.0
//! A Fixed ASCII file with no Terminate card decodes every complete record.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::dialect::Admission;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeFailure, DecodeOptions, DecodeResult};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::only_match;
use crate::test_support::test_cards::{card, card_with_ending};
use crate::IgesCodec;

const GLOBAL: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
const BYTE_CUT_OFFSETS: [usize; 3] = [1, 40, 79];
const PROPERTY_CASES: usize = 56;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Start,
    Global,
    Directory,
    Parameter,
    Terminate,
}

struct Card {
    bytes: Vec<u8>,
    section: Section,
}

struct Entity {
    directory_sequence: u32,
    /// Index of the Parameter Data card that carries the record delimiter.
    semicolon_card: usize,
    parameter_cards: Vec<usize>,
}

struct Synthetic {
    cards: Vec<Card>,
    global_card_count: usize,
    entities: Vec<Entity>,
}

struct PlannedEntity {
    directory_sequence: u32,
    fragments: &'static [&'static [u8]],
    semicolon_fragment: usize,
}

struct Outcome {
    directory: usize,
    point_ids: BTreeSet<String>,
    points: usize,
    curves: usize,
    vertices: usize,
    framing: usize,
    dialect_unverified: usize,
}

fn synthetic() -> Synthetic {
    let global_payloads =
        crate::global::layout_global_cards(GLOBAL).expect("the Global record lays out");
    let mut cards = vec![Card {
        bytes: card(b"generated fixture", b'S', 1),
        section: Section::Start,
    }];
    for (index, payload) in global_payloads.iter().enumerate() {
        let sequence = u32::try_from(index + 1).expect("global sequence");
        cards.push(Card {
            bytes: card(payload, b'G', sequence),
            section: Section::Global,
        });
    }
    let global_card_count = global_payloads.len();
    let planned = [
        PlannedEntity {
            directory_sequence: 1,
            fragments: &[b"116,10.0,20.0,30.0;"],
            semicolon_fragment: 0,
        },
        PlannedEntity {
            directory_sequence: 3,
            fragments: &[b"116,40.0,", b"50.0,60.0;"],
            semicolon_fragment: 1,
        },
        PlannedEntity {
            directory_sequence: 5,
            fragments: &[b"116,70.0,80.0,90.0;", b"comment text"],
            semicolon_fragment: 0,
        },
    ];
    let mut parameter_start = 1_u32;
    for entity in &planned {
        let start = parameter_start.to_string();
        let count = entity.fragments.len().to_string();
        cards.push(Card {
            bytes: directory_pair_first(&start, entity.directory_sequence),
            section: Section::Directory,
        });
        cards.push(Card {
            bytes: directory_pair_second(&count, entity.directory_sequence + 1),
            section: Section::Directory,
        });
        parameter_start += u32::try_from(entity.fragments.len()).expect("parameter count");
    }
    let mut entities = Vec::new();
    let mut parameter_sequence = 1_u32;
    for entity in &planned {
        let mut parameter_cards = Vec::new();
        for fragment in entity.fragments {
            parameter_cards.push(cards.len());
            cards.push(Card {
                bytes: crate::test_support::test_cards::parameter_card(
                    fragment,
                    entity.directory_sequence,
                    parameter_sequence,
                ),
                section: Section::Parameter,
            });
            parameter_sequence += 1;
        }
        entities.push(Entity {
            directory_sequence: entity.directory_sequence,
            semicolon_card: parameter_cards[entity.semicolon_fragment],
            parameter_cards,
        });
    }
    let directory_cards = planned.len() * 2;
    let parameter_cards = usize::try_from(parameter_sequence - 1).expect("parameter cards");
    cards.push(Card {
        bytes: card(
            format!("S0000001G{global_card_count:07}D{directory_cards:07}P{parameter_cards:07}")
                .as_bytes(),
            b'T',
            1,
        ),
        section: Section::Terminate,
    });
    for built in &cards {
        assert_eq!(built.bytes.len(), 81);
    }
    Synthetic {
        cards,
        global_card_count,
        entities,
    }
}

fn directory_pair_first(parameter_start: &str, sequence: u32) -> Vec<u8> {
    crate::test_support::test_cards::directory_card(
        [
            "116",
            parameter_start,
            "0",
            "0",
            "0",
            "0",
            "0",
            "0",
            "00000000",
        ],
        sequence,
    )
}

fn directory_pair_second(parameter_count: &str, sequence: u32) -> Vec<u8> {
    crate::test_support::test_cards::directory_card(
        ["116", "0", "0", parameter_count, "0", "", "", "POINT", "0"],
        sequence,
    )
}

impl Synthetic {
    fn bytes(&self) -> Vec<u8> {
        self.prefix(self.cards.len())
    }

    fn prefix(&self, count: usize) -> Vec<u8> {
        self.cards
            .iter()
            .take(count)
            .flat_map(|built| built.bytes.iter().copied())
            .collect()
    }

    fn without_terminate(&self) -> Vec<u8> {
        self.prefix(self.cards.len() - 1)
    }
}

fn decode(bytes: &[u8]) -> Result<DecodeResult, DecodeFailure> {
    IgesCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
}

fn assert_codec_error(bytes: &[u8], label: &str, pred: impl FnOnce(&CodecError) -> bool) {
    match decode(bytes) {
        Err(DecodeFailure::Codec(error)) if pred(&error) => {}
        other => panic!("{label}: {other:?}"),
    }
}

fn loss_count(losses: &[LossNote], code: IgesLossCode) -> usize {
    losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

fn directory_count(ir: &CadIr) -> usize {
    let namespace = ir.native.namespace("iges").expect("iges namespace");
    let arena = |name: &str| namespace.arenas().get(name).map_or(0, Vec::len);
    arena("entities") + arena("quarantined_directory_records")
}

fn point_ids(ir: &CadIr) -> BTreeSet<String> {
    ir.model
        .points
        .iter()
        .map(|point| {
            point
                .source_object
                .as_ref()
                .expect("projected point names its directory entry")
                .object_id
                .as_str()
                .to_owned()
        })
        .collect()
}

fn outcome_of(decoded: &DecodeResult) -> Outcome {
    let ir = decoded.ir();
    Outcome {
        directory: directory_count(ir),
        point_ids: point_ids(ir),
        points: ir.model.points.len(),
        curves: ir.model.curves.len(),
        vertices: ir.model.vertices.len(),
        framing: loss_count(&decoded.report().losses, IgesLossCode::CardFramingRecovered),
        dialect_unverified: loss_count(
            &decoded.report().losses,
            IgesLossCode::SourceDialectUnverified,
        ),
    }
}

fn global_is_complete(file: &Synthetic, included: usize) -> bool {
    included > file.global_card_count
}

fn directory_records(file: &Synthetic, included: usize) -> usize {
    let directory_start = 1 + file.global_card_count;
    let directory_cards = file.entities.len() * 2;
    let present = included
        .saturating_sub(directory_start)
        .min(directory_cards);
    present.div_ceil(2)
}

fn projected_ids(file: &Synthetic, included: usize) -> BTreeSet<String> {
    file.entities
        .iter()
        .filter(|entity| entity.semicolon_card < included)
        .map(|entity| format!("D{}", entity.directory_sequence))
        .collect()
}

fn parameter_owner_recovery(file: &Synthetic, included: usize) -> bool {
    file.entities.iter().any(|entity| {
        let present = entity
            .parameter_cards
            .iter()
            .filter(|index| **index < included)
            .count();
        present > 0 && present < entity.parameter_cards.len()
    })
}

fn framing_notes(file: &Synthetic, included: usize) -> usize {
    1 + usize::from(parameter_owner_recovery(file, included))
}

fn assert_dialect(decoded: &DecodeResult, global_complete: bool) {
    let matched = only_match(decoded.report().dialects());
    let declared = matched.declared();
    let attributes = &decoded.ir().source.as_ref().expect("source").attributes;
    assert_eq!(
        attributes.get("representation").map(String::as_str),
        Some("fixed-ascii")
    );
    if global_complete {
        assert_eq!(matched.dialect().as_str(), "iges:5.3-fixed-ascii");
        assert_eq!(matched.admission(), &Admission::Admitted);
        assert_eq!(declared["version_flag"], "11");
        assert_eq!(declared["effective_version"], "5.3");
        assert!(!declared.contains_key("version_flag_declaration"));
        assert_eq!(attributes["native_units"], "MM");
        return;
    }
    assert_eq!(matched.dialect().as_str(), "iges:unknown");
    assert_eq!(
        matched.admission(),
        &Admission::Unverified {
            using: cadmpeg_core::dialect::Grammar::of(&crate::dialect::IGES_5_3_FIXED_ASCII),
        }
    );
    assert_eq!(declared["representation"], "fixed-ascii");
    for key in [
        "version_flag",
        "effective_version",
        "effective_version_flag",
        "version_flag_declaration",
    ] {
        assert!(!declared.contains_key(key), "{key} in {declared:?}");
    }
    for key in ["native_units", "parameter_delimiter", "record_delimiter"] {
        assert!(!attributes.contains_key(key), "{key} in {attributes:?}");
    }
}

fn assert_truncated(label: &str, bytes: &[u8], file: &Synthetic, included: usize) -> Outcome {
    let decoded = decode(bytes).unwrap_or_else(|error| panic!("{label} decodes: {error}"));
    let outcome = outcome_of(&decoded);
    let global_complete = global_is_complete(file, included);
    let ended = file.cards[included - 1].section;
    assert_eq!(
        matches!(ended, Section::Start | Section::Global),
        !global_complete,
        "{label} ends in {ended:?}"
    );
    assert_eq!(
        outcome.directory,
        directory_records(file, included),
        "{label} directory records"
    );
    assert_eq!(
        outcome.point_ids,
        projected_ids(file, included),
        "{label} projected points"
    );
    assert_eq!(outcome.points, outcome.point_ids.len(), "{label}");
    assert_eq!(outcome.vertices, outcome.points, "{label}");
    assert_eq!(outcome.curves, 0, "{label}");
    assert_eq!(
        outcome.framing,
        framing_notes(file, included),
        "{label} framing losses"
    );
    assert_eq!(
        outcome.dialect_unverified,
        usize::from(!global_complete),
        "{label} dialect"
    );
    assert_dialect(&decoded, global_complete);
    outcome
}

#[test]
fn property_covers_every_boundary_of_the_synthetic_file() {
    let file = synthetic();
    assert_eq!(file.global_card_count, 2);
    assert_eq!(file.entities.len(), 3);
    assert!(file.entities.iter().any(|entity| {
        entity.parameter_cards.len() > 1
            && entity.semicolon_card == *entity.parameter_cards.last().unwrap()
    }));
    assert!(file
        .entities
        .iter()
        .any(|entity| { entity.semicolon_card < *entity.parameter_cards.last().unwrap() }));
    assert_eq!(file.cards.len(), 15);
    assert!(BYTE_CUT_OFFSETS
        .iter()
        .all(|offset| (1..80).contains(offset)));
    assert_eq!(
        (file.cards.len() - 1) * (1 + BYTE_CUT_OFFSETS.len()),
        PROPERTY_CASES
    );
}

#[test]
fn complete_synthetic_file_decodes_without_framing_or_dialect_loss() {
    let file = synthetic();
    let decoded = decode(&file.bytes()).expect("the complete file decodes");
    let outcome = outcome_of(&decoded);
    assert_eq!(outcome.directory, file.entities.len());
    assert_eq!(
        outcome.point_ids,
        file.entities
            .iter()
            .map(|entity| format!("D{}", entity.directory_sequence))
            .collect()
    );
    assert_eq!(outcome.points, file.entities.len());
    assert_eq!(outcome.vertices, file.entities.len());
    assert_eq!(outcome.curves, 0);
    assert_eq!(outcome.framing, 0);
    assert_eq!(outcome.dialect_unverified, 0);
    assert!(
        decoded.report().losses.is_empty(),
        "{:#?}",
        decoded.report().losses
    );
    assert_dialect(&decoded, true);
}

#[test]
fn file_without_a_terminate_card_keeps_every_entity() {
    let file = synthetic();
    let complete = decode(&file.bytes()).expect("the complete file decodes");
    let complete_outcome = outcome_of(&complete);
    let truncated = assert_truncated(
        "file without a Terminate card",
        &file.without_terminate(),
        &file,
        file.cards.len() - 1,
    );
    assert_eq!(truncated.point_ids, complete_outcome.point_ids);
    assert_eq!(truncated.points, complete_outcome.points);
    assert_eq!(truncated.curves, complete_outcome.curves);
    assert_eq!(truncated.vertices, complete_outcome.vertices);
    assert_eq!(truncated.directory, complete_outcome.directory);
    assert_eq!(truncated.framing, 1);
    assert_eq!(
        decode(&file.without_terminate())
            .expect("truncated file")
            .report()
            .losses
            .len(),
        complete.report().losses.len() + 1
    );
}

#[test]
fn truncated_prefixes_and_byte_cuts_keep_complete_records() {
    let file = synthetic();
    let complete = decode(&file.bytes()).expect("the complete file decodes");
    let complete_outcome = outcome_of(&complete);
    let mut previous_directory = 0_usize;
    let mut previous_ids = BTreeSet::new();
    let mut executed = 0_usize;
    for included in 1..file.cards.len() {
        let outcome = assert_truncated(
            &format!("prefix of {included} cards"),
            &file.prefix(included),
            &file,
            included,
        );
        assert!(outcome.directory <= complete_outcome.directory);
        assert!(outcome.directory >= previous_directory);
        assert!(outcome.point_ids.is_subset(&complete_outcome.point_ids));
        assert!(previous_ids.is_subset(&outcome.point_ids));
        previous_directory = outcome.directory;
        previous_ids = outcome.point_ids;
        executed += 1;
    }
    for index in 1..file.cards.len() {
        for offset in BYTE_CUT_OFFSETS {
            let mut bytes = file.prefix(index);
            bytes.extend_from_slice(&file.cards[index].bytes[..offset]);
            let outcome = assert_truncated(
                &format!("byte {offset} of card {index}"),
                &bytes,
                &file,
                index,
            );
            let prefix = assert_truncated(
                &format!("prefix equivalent to card {index}"),
                &file.prefix(index),
                &file,
                index,
            );
            assert_eq!(outcome.directory, prefix.directory);
            assert_eq!(outcome.point_ids, prefix.point_ids);
            assert_eq!(outcome.framing, prefix.framing);
            assert_eq!(outcome.dialect_unverified, prefix.dialect_unverified);
            executed += 1;
        }
    }
    assert_eq!(executed, PROPERTY_CASES);
}

#[test]
fn one_start_card_decodes_to_an_empty_unverified_model() {
    for ending in [b"".as_slice(), b"\n", b"\r", b"\r\n"] {
        let bytes = card_with_ending(b"generated fixture", b'S', 1, ending);
        assert_eq!(
            cadmpeg_test_support::detection::confidence(&IgesCodec, &bytes),
            Confidence::No
        );
        let decoded = decode(&bytes).unwrap_or_else(|error| panic!("{ending:?} decodes: {error}"));
        let outcome = outcome_of(&decoded);
        assert_eq!(outcome.directory, 0);
        assert_eq!(outcome.points, 0);
        assert_eq!(outcome.curves, 0);
        assert_eq!(outcome.vertices, 0);
        assert!(outcome.point_ids.is_empty());
        assert_eq!(outcome.framing, 1);
        assert_eq!(outcome.dialect_unverified, 1);
        assert_dialect(&decoded, false);
        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(bytes),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .expect("one Start card inspects");
        assert_eq!(
            only_match(summary.dialects()).dialect().as_str(),
            "iges:unknown"
        );
        assert_eq!(
            loss_count(&summary.losses, IgesLossCode::CardFramingRecovered),
            1
        );
        assert_eq!(
            loss_count(&summary.losses, IgesLossCode::SourceDialectUnverified),
            1
        );
    }
}

#[test]
fn two_terminators_after_one_start_card_stay_unrecognized() {
    let bytes = card_with_ending(b"generated fixture", b'S', 1, b"\n\n");
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&IgesCodec, &bytes),
        Confidence::No
    );
    assert_codec_error(&bytes, "two terminators are not one Start card", |error| {
        matches!(error, CodecError::WrongFormat(_))
    });
}

#[test]
fn blank_sequence_start_card_is_not_fixed_ascii() {
    let mut bytes = vec![b' '; 80];
    let text = b"generated fixture";
    bytes[..text.len()].copy_from_slice(text);
    bytes[72] = b'S';
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&IgesCodec, &bytes),
        Confidence::No
    );
    assert_codec_error(&bytes, "a blank sequence does not classify", |error| {
        matches!(error, CodecError::WrongFormat(_))
    });
}

#[test]
fn global_record_cut_mid_value_claims_no_version_or_unit() {
    let file = synthetic();
    let last_global = file.global_card_count;
    assert!(last_global >= 2);
    let payload = &file.cards[last_global].bytes[..72];
    let semicolon = payload
        .iter()
        .rposition(|byte| *byte == b';')
        .expect("the last Global card carries the record delimiter");
    assert!(semicolon > 0);
    let mut bytes = file.prefix(last_global);
    bytes.extend_from_slice(&file.cards[last_global].bytes[..semicolon]);
    assert_truncated("Global record cut mid-value", &bytes, &file, last_global);
}

#[test]
fn unmarked_line_before_the_final_line_is_malformed() {
    let file = synthetic();
    let complete = file.bytes();
    let without_terminate = file.without_terminate();
    // After the Start card and the first Global card, so classification still
    // accepts the stream and the refusal comes from section framing.
    let after_two_cards = 81 * 2;
    let after_global = (1 + file.global_card_count) * 81;
    for (label, bytes) in [
        (
            "blank line with Terminate",
            insert_line(&complete, after_two_cards, b"\n"),
        ),
        (
            "blank line without Terminate",
            insert_line(&without_terminate, after_two_cards, b"\n"),
        ),
        (
            "short line with Terminate",
            insert_line(&complete, after_global, b"short\n"),
        ),
    ] {
        assert_codec_error(&bytes, label, |error| {
            matches!(error, CodecError::Malformed(_))
        });
    }
}

fn insert_line(bytes: &[u8], at: usize, line: &[u8]) -> Vec<u8> {
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(line);
    out.extend_from_slice(&bytes[at..]);
    out
}
