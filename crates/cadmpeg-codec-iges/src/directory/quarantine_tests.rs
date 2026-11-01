// SPDX-License-Identifier: Apache-2.0
//! Directory Entry quarantine: the two-list parse and its native arena.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::test_cards::card;
use crate::test_support::test_owned::{
    owned_test_file, owned_test_file_with_global, OwnedTestEntity,
};
use crate::test_support::{code_count, decode, given_up_count, given_up_parts, strict_options};
use crate::IgesCodec;

/// Zero-based index of the Parameter Data pointer in the first Directory card.
const PARAMETER_FIELD: usize = 1;

fn two_point_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ])
}

const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";

fn v4_two_point_file() -> Vec<u8> {
    owned_test_file_with_global(
        &[
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "FIRST".into(),
                status: "00000000",
                parameters: "116,1,2,3,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "SECOND".into(),
                status: "00000000",
                parameters: "116,4,5,6,0;".into(),
            },
        ],
        GLOBAL_V4,
    )
}

fn directory_card_offset(bytes: &[u8], index: usize) -> usize {
    bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'D')
        .map(|first| (first + index) * 81)
        .expect("Directory card")
}

/// Overwrite one eight-byte field of one Directory card.
fn corrupt_field(bytes: &[u8], card_index: usize, field: usize, value: [u8; 8]) -> Vec<u8> {
    let mut corrupted = bytes.to_vec();
    let start = directory_card_offset(bytes, card_index) + field * 8;
    corrupted[start..start + 8].copy_from_slice(&value);
    corrupted
}

#[test]
fn a_non_integer_directory_field_gives_up_its_field_part() {
    let bytes = corrupt_field(&two_point_file(), 2, PARAMETER_FIELD, *b"     abc");
    let field_start = directory_card_offset(&bytes, 2) + PARAMETER_FIELD * 8;

    let result = decode(bytes.clone());

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert_eq!(
        native.arenas()["entities"][0].id(),
        "iges:entity:directory#1"
    );
    let parts = given_up_parts(result.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert!(!parts[0].untyped().is_empty());
    assert_eq!(parts[0].offset(), u64::try_from(field_start).unwrap());
    assert_eq!(parts[0].byte_len(), 8);
    assert_eq!(parts[0].data(), Some(&bytes[field_start..field_start + 8]));
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(given_up_count(result.report(), "iges.directory_field"), 1);
    assert!(serde_json::to_value(result.report())
        .unwrap()
        .get("transfer_ledger")
        .is_none());

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(summary.notes.contains(&"entities=1".into()));
}

#[test]
fn every_directory_defect_names_its_own_failure() {
    for (card_index, field, value, part, failure) in [
        (
            2,
            PARAMETER_FIELD,
            *b"     \xff\xfe\xfd",
            "iges.directory_field",
            "the Parameter Data start field is not ASCII",
        ),
        (
            2,
            PARAMETER_FIELD,
            *b"     abc",
            "iges.directory_field",
            "the Parameter Data start field is not a decimal integer",
        ),
        (
            2,
            8,
            *b"0000 201",
            "iges.directory_field",
            "the status number is neither blank nor an eight-digit decimal integer",
        ),
        (
            3,
            0,
            *b"     110",
            "iges.directory_entry",
            "the repeated entity type 110 does not equal the entity type 116",
        ),
    ] {
        let bytes = corrupt_field(&two_point_file(), card_index, field, value);

        let result = decode(bytes);

        assert_eq!(given_up_count(result.report(), part), 1, "{failure}");
        assert_eq!(result.report().losses.len(), 1, "{failure}");
        assert!(
            result.report().losses[0].message.contains(failure),
            "{failure}"
        );
    }
}

#[test]
fn v4_blank_no_default_directory_fields_give_up_their_field() {
    for (card_index, field) in [(2, 0), (2, 1), (3, 0), (3, 3)] {
        let bytes = corrupt_field(&v4_two_point_file(), card_index, field, *b"        ");
        let field_start = directory_card_offset(&bytes, card_index) + field * 8;
        let result = decode(bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        assert_eq!(
            native.arenas()["entities"].len(),
            1,
            "card {card_index}, field {field}"
        );
        let parts = given_up_parts(result.source_fidelity());
        assert_eq!(parts.len(), 1, "card {card_index}, field {field}");
        assert_eq!(parts[0].offset(), u64::try_from(field_start).unwrap());
        assert_eq!(parts[0].byte_len(), 8);
        assert_eq!(
            given_up_count(result.report(), "iges.directory_field"),
            1,
            "card {card_index}, field {field}"
        );
        assert!(
            result.report().losses[0]
                .message
                .contains("is blank and IGES 4.0 defines no default"),
            "card {card_index}, field {field}"
        );
    }
}

#[test]
fn an_unpaired_trailing_directory_card_is_given_up_on_its_own() {
    let mut bytes = two_point_file();
    let parameter_start = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'P')
        .expect("Parameter card")
        * 81;
    let unpaired = card(b"     116       5", b'D', 5);
    bytes.splice(parameter_start..parameter_start, unpaired.clone());

    let result = decode(bytes);

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 2);
    let parts = given_up_parts(result.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].offset(), u64::try_from(parameter_start).unwrap());
    assert_eq!(parts[0].data(), Some(unpaired.as_slice()));
    assert_eq!(given_up_count(result.report(), "iges.directory_entry"), 1);
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("the Directory Entry section ends with an unpaired card")));
}

#[test]
fn a_pointer_into_a_given_up_record_does_not_resolve() {
    let mut bytes = two_point_file();
    let color_field = directory_card_offset(&bytes, 1) + 2 * 8;
    bytes[color_field..color_field + 8].copy_from_slice(b"      -3");
    let bytes = corrupt_field(&bytes, 2, PARAMETER_FIELD, *b"     abc");

    let result = decode(bytes.clone());

    assert_eq!(
        code_count(result.report(), IgesLossCode::PointerUnresolved),
        1
    );
    assert_eq!(given_up_count(result.report(), "iges.directory_field"), 1);

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options())
        .unwrap_err();
    match error {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => {
            // #215 §4.5: `record.unreadable` carries a strict floor, so the
            // caller's strict rejection names the given-up part first.
            assert_eq!(rejection.loss().code.local_code(), "record.unreadable");
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn the_outbound_pointers_of_a_given_up_record_are_not_analyzed() {
    let mut bytes = two_point_file();
    let color_field = directory_card_offset(&bytes, 3) + 2 * 8;
    bytes[color_field..color_field + 8].copy_from_slice(b"      -1");
    let bytes = corrupt_field(&bytes, 2, PARAMETER_FIELD, *b"     abc");

    let result = decode(bytes);

    assert_eq!(
        code_count(result.report(), IgesLossCode::PointerUnresolved),
        0
    );
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(given_up_count(result.report(), "iges.directory_field"), 1);
}

#[test]
fn a_quarantined_directory_record_refuses_a_strict_decode_and_survives_container_only() {
    let bytes = corrupt_field(&two_point_file(), 2, PARAMETER_FIELD, *b"     abc");

    let container_only = IgesCodec
        .decode(
            &mut Cursor::new(bytes.clone()),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        container_only
            .ir()
            .native
            .namespace("iges")
            .unwrap()
            .arenas()["quarantined_directory_records"]
            .len(),
        1
    );
    assert_eq!(
        code_count(
            container_only.report(),
            IgesLossCode::DirectoryRecordQuarantined
        ),
        1
    );

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options())
        .unwrap_err();
    match error {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => assert_eq!(
            rejection.loss().code.to_string(),
            IgesLossCode::DirectoryRecordQuarantined.kind().to_string()
        ),
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn directory_quarantine_record_names_its_identity_and_defect() {
    let record = super::QuarantinedDirectoryRecord {
        sequence: 3,
        source_offset: 160,
        bytes: Vec::new(),
        defect: super::DirectoryDefect::FieldNotAnInteger("level"),
    };
    assert_eq!(
        serde_json::to_value(&record).unwrap()["id"],
        "iges:quarantine:directory#3"
    );
    let loss = record.loss_note();
    assert_eq!(
        loss.provenance.unwrap().tag.as_deref(),
        Some("directory_entry:D3")
    );
    assert!(loss
        .message
        .contains("the level field is not a decimal integer"));
}
