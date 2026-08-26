// SPDX-License-Identifier: Apache-2.0
//! Parameter Data quarantine: ownership order, arena records, and accounting.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::report::{DecodeReport, TransferDisposition};

use crate::loss::IgesLossCode;
use crate::test_support::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

/// Zero-based index of the Parameter Data count field in the second card.
const PARAMETER_COUNT_FIELD: usize = 3;

fn strict_options() -> DecodeOptions {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    options
}

fn code_count(report: &DecodeReport, code: IgesLossCode) -> usize {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

fn point_file(parameters: &str) -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00000000",
        parameters: parameters.into(),
    }])
}

fn decode(bytes: Vec<u8>) -> DecodeResult {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn parameter_card_offset(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'P')
        .expect("Parameter card")
        * 81
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
fn a_token_that_is_not_a_number_quarantines_only_that_parameter_data() {
    let bytes = point_file("116,1,2,3x4,0;");
    let card_offset = parameter_card_offset(&bytes);
    let expected_bytes = bytes[card_offset..card_offset + 80].to_vec();

    let result = decode(bytes);

    let native = result.ir().native.namespace("iges").unwrap();
    let entity = &native.arenas["entities"][0];
    assert_eq!(entity.id(), "iges:entity:directory#1");
    assert_eq!(entity.fields()["entity_type"], 116);
    assert!(entity.fields()["parameter_bytes"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(entity.fields()["parameters"].as_array().unwrap().is_empty());
    assert!(result.ir().model.points.is_empty());

    let quarantined = &native.arenas["quarantined_parameter_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].id(), "iges:quarantine:parameter#1");
    let fields = quarantined[0].fields();
    assert_eq!(fields["section"], "parameter-data");
    assert_eq!(fields["sequence"], 1);
    assert_eq!(fields["source_offset"], card_offset);
    assert_eq!(fields["cards"], 1);
    assert_eq!(fields["defect"], "token-not-a-number");
    assert_eq!(
        fields["bytes"].as_array().unwrap(),
        &expected_bytes
            .iter()
            .map(|byte| serde_json::Value::from(*byte))
            .collect::<Vec<_>>()
    );

    assert_eq!(result.report().losses.len(), 1);
    let loss = &result.report().losses[0];
    assert_eq!(loss.code, IgesLossCode::ParameterDataQuarantined.kind());
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("D1:parameter")
    );
    let ledger = &result.report().transfer_ledger.entries;
    let entity_row = ledger
        .iter()
        .find(|entry| entry.source == "D1")
        .expect("typed entity ledger row");
    assert_eq!(
        entity_row.target.as_deref(),
        Some("iges:entity:directory#1")
    );
    assert_eq!(
        entity_row.note.as_deref(),
        Some("native record retained; semantic projection omitted with an attributed loss")
    );
    let quarantine_row = ledger
        .iter()
        .find(|entry| entry.source == "D1:parameter")
        .expect("quarantined parameter ledger row");
    assert_eq!(
        quarantine_row.target.as_deref(),
        Some("iges:quarantine:parameter#1")
    );
    assert_eq!(quarantine_row.disposition, TransferDisposition::Retained);
}

#[test]
fn a_first_token_disagreeing_with_the_entity_type_quarantines_the_parameter_data() {
    let result = decode(point_file("110,1,2,3,0;"));

    let native = result.ir().native.namespace("iges").unwrap();
    let quarantined = &native.arenas["quarantined_parameter_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(
        quarantined[0].fields()["defect"],
        "entity-type-token-mismatch"
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert_eq!(result.report().losses.len(), 1);
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
    let quarantined = &native.arenas["quarantined_parameter_records"];
    assert_eq!(native.arenas["entities"].len(), 2);
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].id(), "iges:quarantine:parameter#3");
    let fields = quarantined[0].fields();
    assert_eq!(fields["cards"], 0);
    assert!(fields["bytes"].as_array().unwrap().is_empty());
    assert_eq!(fields["defect"], "declared-count-zero");
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert!(result
        .report()
        .transfer_ledger
        .entries
        .iter()
        .any(|entry| entry.source == "D3:parameter"
            && entry.target.as_deref() == Some("iges:quarantine:parameter#3")));
}

#[test]
fn a_declared_count_of_zero_defers_to_the_back_pointer_census() {
    let bytes = set_second_card_field(
        &point_file("116,1,2,3,0;"),
        PARAMETER_COUNT_FIELD,
        *b"       0",
    );

    let result = decode(bytes);

    let native = result.ir().native.namespace("iges").unwrap();
    assert!(native.arenas["quarantined_parameter_records"].is_empty());
    assert_eq!(result.ir().model.points.len(), 1);
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

    let native = result.ir().native.namespace("iges").unwrap();
    let quarantined = &native.arenas["quarantined_parameter_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].fields()["defect"], "declared-card-missing");
    assert_eq!(quarantined[0].fields()["cards"], 0);
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
fn every_token_defect_key_names_its_own_failure() {
    for (parameters, defect) in [
        (
            "116,1,2,99999999999999999999H;",
            "hollerith-count-unreadable",
        ),
        ("116,1,2,64Hshort;", "hollerith-payload-truncated"),
        ("116,1,2,0H;", "hollerith-count-zero"),
        ("116,1,2,3x4,0;", "token-not-a-number"),
        ("116,1,2,3 ,0;", "numeric-contains-blanks"),
        ("116,1,2,3,0", "delimiter-missing"),
    ] {
        let result = decode(point_file(parameters));

        let native = result.ir().native.namespace("iges").unwrap();
        let quarantined = &native.arenas["quarantined_parameter_records"];
        assert_eq!(quarantined.len(), 1, "{defect}");
        assert_eq!(quarantined[0].fields()["defect"], defect);
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

    let native = result.ir().native.namespace("iges").unwrap();
    let quarantined = &native.arenas["quarantined_parameter_records"];
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].id(), "iges:quarantine:parameter#3");
    assert_eq!(quarantined[0].fields()["defect"], "no-owned-cards");
    assert_eq!(
        code_count(result.report(), IgesLossCode::CardFramingRecovered),
        1
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert_eq!(result.report().losses.len(), 2);
}

#[test]
fn a_non_ascii_token_byte_quarantines_the_parameter_data() {
    let mut bytes = point_file("116,1,2,3,0;");
    let card_offset = parameter_card_offset(&bytes);
    bytes[card_offset + 4] = 0xff;

    let result = decode(bytes);

    let native = result.ir().native.namespace("iges").unwrap();
    let quarantined = &native.arenas["quarantined_parameter_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].fields()["defect"], "token-not-ascii");
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert_eq!(result.report().losses.len(), 1);
}

#[test]
fn a_record_with_no_delimiter_quarantines_the_parameter_data() {
    let result = decode(point_file("116,1,2,3,0"));

    let native = result.ir().native.namespace("iges").unwrap();
    let quarantined = &native.arenas["quarantined_parameter_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].fields()["defect"], "delimiter-missing");
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert_eq!(result.report().losses.len(), 1);
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
    let quarantined = &native.arenas["quarantined_parameter_records"];
    assert_eq!(native.arenas["entities"].len(), 2);
    assert!(result.ir().model.points.is_empty());
    assert_eq!(quarantined.len(), 2);
    for record in quarantined {
        assert_eq!(record.fields()["defect"], "ownership-conflict");
    }
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        2
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::CardFramingRecovered),
        1
    );
    assert_eq!(result.report().losses.len(), 3);
}

#[test]
fn a_quarantined_parameter_record_refuses_strict_and_survives_container_only() {
    let bytes = point_file("116,1,2,3x4,0;");

    let container_only = IgesCodec
        .decode(
            &mut Cursor::new(bytes.clone()),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        container_only.ir().native.namespace("iges").unwrap().arenas
            ["quarantined_parameter_records"]
            .len(),
        1
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
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::ParameterDataQuarantined.kind().as_str()
        ),
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}
