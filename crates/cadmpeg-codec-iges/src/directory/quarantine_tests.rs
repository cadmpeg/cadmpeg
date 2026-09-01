// SPDX-License-Identifier: Apache-2.0
//! Directory Entry quarantine: the two-list parse, its arena, and its ledger.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::report::{DecodeReport, TransferDisposition};

use crate::loss::IgesLossCode;
use crate::test_support::{card, owned_test_file, owned_test_file_with_global, OwnedTestEntity};
use crate::IgesCodec;

/// Zero-based index of the level field inside the first Directory card.
const LEVEL_FIELD: usize = 4;

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

fn decode(bytes: Vec<u8>) -> DecodeResult {
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

#[test]
fn a_non_integer_directory_field_quarantines_the_two_card_pair() {
    let bytes = corrupt_field(&two_point_file(), 2, LEVEL_FIELD, *b"     abc");
    let first_card = directory_card_offset(&bytes, 2);
    let expected_bytes = [
        &bytes[first_card..first_card + 80],
        &bytes[first_card + 81..first_card + 161],
    ]
    .concat();

    let result = decode(bytes.clone());

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas["entities"].len(), 1);
    assert_eq!(native.arenas["entities"][0].id(), "iges:entity:directory#1");
    let quarantined = &native.arenas["quarantined_directory_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].id(), "iges:quarantine:directory#3");
    let fields = quarantined[0].fields();
    assert_eq!(fields["section"], "directory-entry");
    assert_eq!(fields["sequence"], 3);
    assert_eq!(fields["source_offset"], first_card);
    assert_eq!(fields["cards"], 2);
    assert_eq!(fields["defect"], "field-not-an-integer");
    assert_eq!(
        fields["bytes"].as_array().unwrap(),
        &expected_bytes
            .iter()
            .map(|byte| serde_json::Value::from(*byte))
            .collect::<Vec<_>>()
    );
    assert_eq!(result.report().losses.len(), 1);
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
    assert_eq!(row.target.as_deref(), Some("iges:quarantine:directory#3"));
    assert_eq!(row.disposition, TransferDisposition::Retained);

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(summary.notes.contains(&"entities=1".into()));
}

#[test]
fn every_directory_defect_key_names_its_own_failure() {
    for (card_index, field, value, defect) in [
        (2, LEVEL_FIELD, *b"     \xff\xfe\xfd", "field-not-ascii"),
        (2, LEVEL_FIELD, *b"     abc", "field-not-an-integer"),
        (2, 8, *b"0000 201", "status-number-invalid"),
        (3, 0, *b"     110", "repeated-entity-type-mismatch"),
    ] {
        let bytes = corrupt_field(&two_point_file(), card_index, field, value);

        let result = decode(bytes);

        let native = result.ir().native.namespace("iges").unwrap();
        let quarantined = &native.arenas["quarantined_directory_records"];
        assert_eq!(quarantined.len(), 1, "{defect}");
        assert_eq!(quarantined[0].fields()["defect"], defect);
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
            native.arenas["entities"].len(),
            1,
            "card {card_index}, field {field}"
        );
        let quarantined = &native.arenas["quarantined_directory_records"];
        assert_eq!(quarantined.len(), 1, "card {card_index}, field {field}");
        assert_eq!(
            quarantined[0].fields()["defect"],
            "field-blank-not-allowed",
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
    let quarantined = &native.arenas["quarantined_directory_records"];
    assert_eq!(native.arenas["entities"].len(), 2);
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].id(), "iges:quarantine:directory#5");
    let fields = quarantined[0].fields();
    assert_eq!(fields["cards"], 1);
    assert_eq!(fields["defect"], "unpaired-card");
    assert_eq!(fields["bytes"].as_array().unwrap().len(), 80);
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
    let bytes = corrupt_field(&bytes, 2, LEVEL_FIELD, *b"     abc");

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
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { loss_code, .. } => {
            assert_eq!(loss_code, IgesLossCode::PointerUnresolved.kind().as_str());
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn the_outbound_pointers_of_a_quarantined_record_are_not_analyzed() {
    let mut bytes = two_point_file();
    let color_field = directory_card_offset(&bytes, 3) + 2 * 8;
    bytes[color_field..color_field + 8].copy_from_slice(b"      -1");
    let bytes = corrupt_field(&bytes, 2, LEVEL_FIELD, *b"     abc");

    let result = decode(bytes);

    assert_eq!(
        code_count(result.report(), IgesLossCode::PointerUnresolved),
        0
    );
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        code_count(result.report(), IgesLossCode::DirectoryRecordQuarantined),
        1
    );
}

#[test]
fn a_quarantined_directory_record_refuses_a_strict_decode_and_survives_container_only() {
    let bytes = corrupt_field(&two_point_file(), 2, LEVEL_FIELD, *b"     abc");

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
            ["quarantined_directory_records"]
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
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::DirectoryRecordQuarantined.kind().as_str()
        ),
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}
