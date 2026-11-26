// SPDX-License-Identifier: Apache-2.0
//! Directory Entry quarantine: the two-list parse, its part, and its ledger.
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::wire;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::decode::TransferDisposition;

use crate::loss::IgesLossCode;
use crate::test_support::test_cards::card;
use crate::test_support::test_owned::{
    owned_test_file, owned_test_file_with_global, OwnedTestEntity,
};
use crate::test_support::{
    assert_gives_up, code_count, decode, given_up_part_bytes, strict_options,
};
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
fn a_non_integer_directory_field_quarantines_the_two_card_pair() {
    let bytes = corrupt_field(&two_point_file(), 2, PARAMETER_FIELD, *b"     abc");
    let first_card = directory_card_offset(&bytes, 2);

    let result = decode(bytes.clone());

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert_eq!(
        native.arenas()["entities"][0].id(),
        "iges:entity:directory#1"
    );
    assert_eq!(
        given_up_part_bytes(&result, ("directory-entry", 3)),
        &bytes[first_card..first_card + 161]
    );
    assert_eq!(result.report().losses.len(), 2);
    assert_gives_up(result.report(), &[("directory-entry", 3)]);
    let unreadable = &result.report().losses[1];
    assert!(unreadable
        .message
        .contains(&format!(" stream=\"\" offset={first_card} length=161 ")));
    assert!(unreadable
        .message
        .contains("the Parameter Data start field is not a decimal integer"));
    assert_eq!(
        code_count(result.report(), IgesLossCode::DirectoryRecordQuarantined),
        1
    );
    let row = result
        .report()
        .transfer_ledger
        .entries
        .iter()
        .find(|entry| entry.source == "D3")
        .expect("quarantined directory ledger row");
    let stub = unreadable
        .message
        .split(" record=")
        .nth(1)
        .and_then(|rest| rest.split(": ").next());
    assert_eq!(row.target(), stub);
    assert_eq!(
        wire::field::<cadmpeg_ir::report::decode::TransferDisposition>(&(row.outcome), "kind"),
        TransferDisposition::Retained
    );

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
    for (card_index, field, value, defect) in [
        (
            2,
            PARAMETER_FIELD,
            *b"     \xff\xfe\xfd",
            "field is not ASCII",
        ),
        (
            2,
            PARAMETER_FIELD,
            *b"     abc",
            "field is not a decimal integer",
        ),
        (2, 8, *b"0000 201", "the status number is neither blank"),
        (3, 0, *b"     110", "the repeated entity type 110"),
    ] {
        let bytes = corrupt_field(&two_point_file(), card_index, field, value);

        let result = decode(bytes);

        given_up_part_bytes(&result, ("directory-entry", 3));
        assert!(
            result.report().losses.iter().any(|loss| loss.code
                == IgesLossCode::DirectoryRecordQuarantined.kind()
                && loss.message.contains(defect)),
            "{:#?}",
            result.report().losses
        );
        assert_eq!(
            code_count(result.report(), IgesLossCode::DirectoryRecordQuarantined),
            1,
            "{defect}"
        );
    }
}

#[test]
fn v4_blank_no_default_directory_fields_quarantine_the_record() {
    for (card_index, field) in [(2, 0), (2, 1), (3, 0), (3, 3)] {
        let bytes = corrupt_field(&v4_two_point_file(), card_index, field, *b"        ");
        let result = decode(bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        assert_eq!(
            native.arenas()["entities"].len(),
            1,
            "card {card_index}, field {field}"
        );
        given_up_part_bytes(&result, ("directory-entry", 3));
        assert!(
            result.report().losses.iter().any(|loss| loss.code
                == IgesLossCode::DirectoryRecordQuarantined.kind()
                && loss
                    .message
                    .contains("field is blank and IGES 4.0 defines no default")),
            "card {card_index}, field {field}"
        );
        assert_eq!(
            code_count(result.report(), IgesLossCode::DirectoryRecordQuarantined),
            1,
            "card {card_index}, field {field}"
        );
    }
}

#[test]
fn an_unpaired_trailing_directory_card_is_quarantined_on_its_own() {
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
    assert_eq!(
        given_up_part_bytes(&result, ("directory-entry", 5)),
        &unpaired[..80]
    );
    assert!(result.report().losses.iter().any(|loss| loss.code
        == IgesLossCode::DirectoryRecordQuarantined.kind()
        && loss
            .message
            .contains("the Directory Entry section ends with an unpaired card")));
    assert_eq!(
        code_count(result.report(), IgesLossCode::DirectoryRecordQuarantined),
        1
    );
}

#[test]
fn a_pointer_into_a_quarantined_record_does_not_resolve() {
    let mut bytes = two_point_file();
    let color_field = directory_card_offset(&bytes, 1) + 2 * 8;
    bytes[color_field..color_field + 8].copy_from_slice(b"      -3");
    let bytes = corrupt_field(&bytes, 2, PARAMETER_FIELD, *b"     abc");

    let result = decode(bytes.clone());

    assert_eq!(
        code_count(result.report(), IgesLossCode::PointerUnresolved),
        1
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::DirectoryRecordQuarantined),
        1
    );

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options())
        .unwrap_err();
    match error {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => {
            assert_eq!(
                rejection.loss().code.to_string(),
                IgesLossCode::PointerUnresolved.kind().to_string()
            );
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn the_outbound_pointers_of_a_quarantined_record_are_not_analyzed() {
    let mut bytes = two_point_file();
    let color_field = directory_card_offset(&bytes, 3) + 2 * 8;
    bytes[color_field..color_field + 8].copy_from_slice(b"      -1");
    let bytes = corrupt_field(&bytes, 2, PARAMETER_FIELD, *b"     abc");

    let result = decode(bytes);

    assert_eq!(
        code_count(result.report(), IgesLossCode::PointerUnresolved),
        0
    );
    assert_eq!(result.report().losses.len(), 2);
    assert_gives_up(result.report(), &[("directory-entry", 3)]);
    assert_eq!(
        code_count(result.report(), IgesLossCode::DirectoryRecordQuarantined),
        1
    );
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
    given_up_part_bytes(&container_only, ("directory-entry", 3));
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
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => {
            crate::test_support::assert_given_up(rejection.loss(), ("directory-entry", 3));
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn directory_quarantine_record_names_its_defect() {
    let record = super::QuarantinedDirectoryRecord {
        sequence: 3,
        source_offset: 160,
        source_length: Some(160),
        bytes: Vec::new(),
        defect: super::DirectoryDefect::FieldNotAnInteger("level"),
    };
    let loss = record.loss_note();
    assert_eq!(
        loss.provenance.unwrap().tag.as_deref(),
        Some("directory_entry:D3")
    );
    assert!(loss
        .message
        .contains("the level field is not a decimal integer"));
}
