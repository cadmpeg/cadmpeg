// SPDX-License-Identifier: Apache-2.0
//! Parameter Data quarantine: ownership order, arena records, and accounting.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::test_support::{code_count, decode, given_up_count, given_up_parts, strict_options};
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
    let (_, retained_parameters) = crate::test_support::retained_fixed_ascii(&result);
    assert!(!retained_parameters.records[0].bytes.is_empty());
    assert_eq!(
        entity.fields().unwrap()["parameters"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(entity.fields().unwrap()["parameters"][3].is_null());
    for (index, value) in [(0, 116), (1, 1), (2, 2), (4, 0)] {
        assert_eq!(
            entity.fields().unwrap()["parameters"][index]["value"]["value"],
            value
        );
    }
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    assert!(result.ir().model.points.is_empty());
    crate::test_support::parameter_parts::assert_one_parameter_field(&result, &bytes, 8, b"3x4,");
    assert!(matches!(
        IgesCodec.decode(&mut Cursor::new(bytes), &strict_options()),
        Err(cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. })
    ));
}

#[test]
fn a_first_token_disagreeing_with_the_entity_type_gives_up_the_parameter_record() {
    let bytes = point_file("110,1,2,3,0;");
    let start = parameter_card_offset(&bytes);

    let result = decode(bytes.clone());

    let parts = given_up_parts(result.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].offset(), u64::try_from(start).unwrap());
    assert_eq!(parts[0].data(), Some(&bytes[start..start + 81]));
    assert_eq!(given_up_count(result.report(), "iges.parameter_record"), 1);
    assert!(result.report().losses[0]
        .message
        .contains("the first token disagrees with the Directory Entry entity type"));
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
    let quarantined = &native.arenas()["quarantined_parameter_records"];
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].id(), "iges:quarantine:parameter#3");
    let fields = quarantined[0].fields().unwrap();
    assert_eq!(fields["cards"], 0);
    assert!(fields["bytes"].as_str().unwrap().is_empty());
    assert_eq!(fields["defect"], "declared-count-zero");
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
    assert!(cadmpeg_ir::index::ModelIndex::build(result.ir()).contains(
        "iges:quarantine:parameter#3",
        cadmpeg_ir::index::StandardIndex
    ));
    assert!(serde_json::to_value(result.report())
        .unwrap()
        .get("transfer_ledger")
        .is_none());
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
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        code_count(result.report(), IgesLossCode::CardFramingRecovered),
        1
    );
}

#[test]
fn a_declared_card_that_does_not_exist_gives_up_the_parameter_section() {
    let bytes = set_second_card_field(
        &point_file("116,1,2,3,0;"),
        PARAMETER_COUNT_FIELD,
        *b"       4",
    );
    let card_offset = parameter_card_offset(&bytes);
    let back_pointer = card_offset + 64;
    let mut bytes = bytes;
    bytes[back_pointer..back_pointer + 8].copy_from_slice(b"      99");

    let result = decode(bytes.clone());

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 1);
    assert!(result.ir().model.points.is_empty());
    let parts = given_up_parts(result.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].offset(), u64::try_from(card_offset).unwrap());
    assert_eq!(parts[0].data(), Some(&bytes[card_offset..card_offset + 81]));
    assert_eq!(given_up_count(result.report(), "iges.parameter_section"), 1);
    assert_eq!(result.report().losses.len(), 1);
}

#[test]
fn every_token_defect_names_its_own_failure() {
    for (parameters, failure) in [
        (
            "116,1,2,99999999999999999999H;",
            "a Hollerith byte count is unreadable",
        ),
        ("116,1,2,64Hshort;", "a Hollerith payload is truncated"),
        ("116,1,2,0H;", "a Hollerith byte count is zero"),
        ("116,1,2,3,0", "a delimiter is missing"),
    ] {
        let result = decode(point_file(parameters));

        assert_eq!(
            given_up_count(result.report(), "iges.parameter_record"),
            1,
            "{failure}"
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains(failure)),
            "{failure}"
        );
    }
}

#[test]
fn a_non_ascii_literal_retains_its_framed_record() {
    let mut bytes = point_file("116,1,2,3,0;");
    let card_offset = parameter_card_offset(&bytes);
    bytes[card_offset + 4] = 0xff;
    let result = decode(bytes.clone());
    assert!(result.ir().native.namespace("iges").unwrap().arenas()
        ["quarantined_parameter_records"]
        .is_empty());
    crate::test_support::parameter_parts::assert_one_parameter_field(&result, &bytes, 4, b"\xff,");
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas()["entities"][0].fields().unwrap();
    assert_eq!(fields["parameters"].as_array().unwrap().len(), 5);
    assert!(fields["parameters"][1].is_null());
    for (index, value) in [(0, 116), (2, 2), (3, 3), (4, 0)] {
        assert_eq!(fields["parameters"][index]["value"]["value"], value);
    }
    assert!(result.ir().model.points.is_empty());
}

#[test]
fn a_record_with_no_delimiter_gives_up_the_parameter_record() {
    let bytes = point_file("116,1,2,3,0");
    let start = parameter_card_offset(&bytes);

    let result = decode(bytes.clone());

    let parts = given_up_parts(result.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].data(), Some(&bytes[start..start + 81]));
    assert_eq!(given_up_count(result.report(), "iges.parameter_record"), 1);
    assert!(result.report().losses[0]
        .message
        .contains("a delimiter is missing"));
    assert_eq!(result.report().losses.len(), 1);
}

#[test]
fn two_declared_ranges_claiming_one_card_share_one_given_up_source_part() {
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

    let result = decode(bytes.clone());

    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert!(result.ir().model.points.is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let fidelity = result.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(
        crate::test_support::given_up_count(result.report(), "iges.parameter_record"),
        1
    );
    let start = parameter_card_offset(&bytes);
    assert_eq!(part.offset(), u64::try_from(start).unwrap());
    assert_eq!(part.byte_len(), 81);
    assert_eq!(part.data(), Some(&bytes[start..start + 81]));
    let unreadable: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(unreadable[0].message.contains("ownership-conflict"));
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        0
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::CardFramingRecovered),
        1
    );
    assert_eq!(result.report().losses.len(), 2);
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
    assert_eq!(
        container_only
            .ir()
            .native
            .namespace("iges")
            .unwrap()
            .arenas()["quarantined_parameter_records"]
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
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => assert_eq!(
            rejection.loss().code.to_string(),
            IgesLossCode::ParameterDataQuarantined.kind().to_string()
        ),
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn parameter_quarantine_record_names_its_identity_and_owned_range() {
    let record = super::QuarantinedParameterRecord {
        sequence: 3,
        ownership: super::QuarantinedCards::Owned {
            range: 2..3,
            bytes: Vec::new(),
            first_offset: 160,
        },
        failing_offset: None,
        defect: super::ParameterDefect::NoOwnedCards,
    };
    assert_eq!(
        serde_json::to_value(&record).unwrap()["id"],
        "iges:quarantine:parameter#3"
    );
    let loss = record.loss_note().unwrap();
    assert_eq!(
        loss.provenance.unwrap().tag.as_deref(),
        Some("D3:parameter")
    );
    assert!(loss.message.contains("D3 (P2 through P2)"));
}
