// SPDX-License-Identifier: Apache-2.0
//! Parameter Data quarantine: ownership order, parts, and accounting.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::ParameterDefect;
use crate::loss::IgesLossCode;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::test_support::{
    assert_gives_up, code_count, decode, given_up_part_bytes, strict_options,
};
use crate::IgesCodec;

/// Zero-based index of the Parameter Data count field in the second card.
const PARAMETER_COUNT_FIELD: usize = 3;

fn point_file(parameters: &str) -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00000000",
        parameters: parameters.into(),
    }])
}

fn parameter_card_offset(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'P')
        .expect("Parameter card")
        * 81
}

/// Asserts that `result` quarantines the Parameter Data of `sequence` for
/// `defect` and gives up its part, and returns the part's bytes.
fn quarantined(
    result: &cadmpeg_ir::codec::DecodeResult,
    sequence: u32,
    defect: ParameterDefect,
) -> Vec<u8> {
    assert!(
        result.report().losses.iter().any(|loss| loss.code
            == IgesLossCode::ParameterDataQuarantined.kind()
            && loss.message.contains(&format!("D{sequence} ("))
            && loss.message.contains(defect.describe())),
        "{:#?}",
        result.report().losses
    );
    given_up_part_bytes(result, ("parameter-data", sequence))
}

fn no_quarantine_part(result: &cadmpeg_ir::codec::DecodeResult) {
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        0
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.starts_with("part parameter-data ")));
}

/// Overwrite one eight-byte field of the second Directory card.
fn set_second_card_field(bytes: &[u8], field: usize, value: [u8; 8]) -> Vec<u8> {
    let mut updated = bytes.to_vec();
    let start = (bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'D')
        .expect("Directory card")
        + 1)
        * 81
        + field * 8;
    updated[start..start + 8].copy_from_slice(&value);
    updated
}

#[test]
fn an_unreadable_literal_retains_the_other_fields_and_refuses_required_geometry() {
    let bytes = point_file("116,1,2,3x4,0;");
    let result = decode(bytes.clone());
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = &native.arenas()["entities"][0];
    assert!(!entity.fields().unwrap()["parameter_bytes"]
        .as_str()
        .unwrap()
        .is_empty());
    assert_eq!(
        entity.fields().unwrap()["parameters"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        entity.fields().unwrap()["parameters"][3]["value"]["kind"],
        "unreadable"
    );
    no_quarantine_part(&result);
    assert!(result.ir().geometry.points.is_empty());
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterLiteralUnusable),
        1
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::GeometryNotProjected),
        1
    );
    assert!(matches!(
        IgesCodec.decode(&mut Cursor::new(bytes), &strict_options()),
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. })
    ));
}

#[test]
fn a_first_token_disagreeing_with_the_entity_type_quarantines_the_parameter_data() {
    let bytes = point_file("110,1,2,3,0;");
    let card_offset = parameter_card_offset(&bytes);

    let result = decode(bytes.clone());

    assert_eq!(
        quarantined(&result, 1, ParameterDefect::EntityTypeTokenMismatch),
        &bytes[card_offset..card_offset + 80]
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert_eq!(result.report().losses.len(), 2);
    assert_gives_up(result.report(), &[("parameter-data", 1)]);
}

#[test]
fn a_non_null_entity_declaring_zero_cards_gets_a_zero_card_quarantine_record() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "EMPTY".into(),
            status: "00000000",
            parameters: String::new(),
        },
    ]);

    let result = decode(bytes);

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert_eq!(result.ir().geometry.points.len(), 1);
    assert!(quarantined(&result, 3, ParameterDefect::DeclaredCountZero).is_empty());
    assert_eq!(result.report().losses.len(), 2);
    assert_gives_up(result.report(), &[("parameter-data", 3)]);
    assert!(result.report().losses[1].message.contains(" length=0 "));
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    let stub = result.report().losses[1]
        .message
        .split(" record=")
        .nth(1)
        .and_then(|rest| rest.split(": ").next());
    assert!(result
        .report()
        .transfer_ledger
        .entries
        .iter()
        .any(|entry| entry.source == "D3:parameter" && entry.target() == stub));
}

#[test]
fn a_declared_count_of_zero_defers_to_the_back_pointer_census() {
    let bytes = set_second_card_field(
        &point_file("116,1,2,3,0;"),
        PARAMETER_COUNT_FIELD,
        *b"       0",
    );

    let result = decode(bytes);

    no_quarantine_part(&result);
    assert_eq!(result.ir().geometry.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        code_count(result.report(), IgesLossCode::CardFramingRecovered),
        1
    );
}

#[test]
fn a_declared_card_that_does_not_exist_quarantines_the_parameter_data() {
    let bytes = set_second_card_field(
        &point_file("116,1,2,3,0;"),
        PARAMETER_COUNT_FIELD,
        *b"       4",
    );
    let card_offset = parameter_card_offset(&bytes);
    let back_pointer = card_offset + 64;
    let mut bytes = bytes;
    bytes[back_pointer..back_pointer + 8].copy_from_slice(b"      99");

    let result = decode(bytes);

    assert!(quarantined(&result, 1, ParameterDefect::DeclaredCardMissing).is_empty());
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::CardFramingRecovered),
        1
    );
}

#[test]
fn every_token_defect_names_its_own_failure() {
    for (parameters, defect) in [
        (
            "116,1,2,99999999999999999999H;",
            ParameterDefect::HollerithCountUnreadable,
        ),
        (
            "116,1,2,64Hshort;",
            ParameterDefect::HollerithPayloadTruncated,
        ),
        ("116,1,2,0H;", ParameterDefect::HollerithCountZero),
        ("116,1,2,3,0", ParameterDefect::DelimiterMissing),
    ] {
        let result = decode(point_file(parameters));

        assert_eq!(quarantined(&result, 1, defect).len(), 80, "{defect:?}");
    }
}

#[test]
fn an_entity_owning_no_card_under_either_rule_is_quarantined() {
    let mut bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "ORPHAN".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ]);
    let directory = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'D')
        .expect("Directory card");
    let start_field = (directory + 2) * 81 + 8;
    bytes[start_field..start_field + 8].copy_from_slice(b"       0");
    let second_card = parameter_card_offset(&bytes) + 81 + 64;
    bytes[second_card..second_card + 8].copy_from_slice(b"      99");

    let result = decode(bytes);

    assert_eq!(result.ir().geometry.points.len(), 1);
    assert!(quarantined(&result, 3, ParameterDefect::NoOwnedCards).is_empty());
    assert_eq!(
        code_count(result.report(), IgesLossCode::CardFramingRecovered),
        1
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert_eq!(result.report().losses.len(), 3);
    assert_gives_up(result.report(), &[("parameter-data", 3)]);
}

#[test]
fn a_non_ascii_literal_retains_its_framed_record() {
    let mut bytes = point_file("116,1,2,3,0;");
    let card_offset = parameter_card_offset(&bytes);
    bytes[card_offset + 4] = 0xff;
    let result = decode(bytes);
    no_quarantine_part(&result);
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterLiteralUnusable),
        1
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::GeometryNotProjected),
        1
    );
}

#[test]
fn a_record_with_no_delimiter_quarantines_the_parameter_data() {
    let result = decode(point_file("116,1,2,3,0"));

    quarantined(&result, 1, ParameterDefect::DelimiterMissing);
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert_eq!(result.report().losses.len(), 2);
    assert_gives_up(result.report(), &[("parameter-data", 1)]);
}

#[test]
fn two_declared_ranges_claiming_one_card_quarantine_both_records() {
    let mut bytes = owned_test_file(&[
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
    ]);
    let start_field = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'D')
        .map(|first| (first + 2) * 81 + 8)
        .expect("Directory card");
    bytes[start_field..start_field + 8].copy_from_slice(b"       1");

    let result = decode(bytes);

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert!(result.ir().geometry.points.is_empty());
    for sequence in [1, 3] {
        assert!(result.report().losses.iter().any(|loss| loss.code
            == IgesLossCode::ParameterDataQuarantined.kind()
            && loss.message.contains(&format!("D{sequence} ("))
            && loss
                .message
                .contains(ParameterDefect::OwnershipConflict.describe())));
    }
    // Both declared ranges start at one card, so one part holds it.
    assert_eq!(
        quarantined(&result, 1, ParameterDefect::OwnershipConflict).len(),
        80
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        2
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::CardFramingRecovered),
        1
    );
    assert_eq!(result.report().losses.len(), 4);
    assert_gives_up(result.report(), &[("parameter-data", 1)]);
}

#[test]
fn a_quarantined_parameter_record_refuses_strict_and_survives_container_only() {
    let bytes = point_file("116,1,2,64Hshort;");

    let container_only = IgesCodec
        .decode(
            &mut Cursor::new(bytes.clone()),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    quarantined(
        &container_only,
        1,
        ParameterDefect::HollerithPayloadTruncated,
    );
    assert_eq!(
        code_count(
            container_only.report(),
            IgesLossCode::ParameterDataQuarantined
        ),
        1
    );

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options())
        .unwrap_err();
    match error {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => {
            crate::test_support::assert_given_up(rejection.loss(), ("parameter-data", 1));
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn parameter_quarantine_record_names_its_owned_range() {
    let record = super::QuarantinedParameterRecord {
        sequence: 3,
        ownership: super::QuarantinedCards::Owned {
            range: 2..3,
            bytes: Vec::new(),
            first_offset: 160,
            length: 80,
        },
        failing_offset: None,
        defect: super::ParameterDefect::NoOwnedCards,
    };
    let loss = record.loss_note().unwrap();
    assert_eq!(
        loss.provenance.unwrap().tag.as_deref(),
        Some("D3:parameter")
    );
    assert!(loss.message.contains("D3 (P2 through P2)"));
}
