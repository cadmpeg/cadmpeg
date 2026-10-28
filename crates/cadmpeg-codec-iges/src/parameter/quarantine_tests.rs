// SPDX-License-Identifier: Apache-2.0
//! Parameter Data quarantine: ownership order, arena records, and accounting.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::test_support::{code_count, decode, strict_options};
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
fn a_first_token_disagreeing_with_the_entity_type_quarantines_the_parameter_data() {
    let result = decode(point_file("110,1,2,3,0;"));

    let native = result.ir().native.namespace("iges").unwrap();
    let quarantined = &native.arenas()["quarantined_parameter_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(
        quarantined[0].fields().unwrap()["defect"],
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
    assert!(cadmpeg_ir::index::ModelIndex::build(result.ir())
        .contains("iges:quarantine:parameter#3", cadmpeg_ir::index::StandardIndex));
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
    let quarantined = &native.arenas()["quarantined_parameter_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(
        quarantined[0].fields().unwrap()["defect"],
        "declared-card-missing"
    );
    assert_eq!(quarantined[0].fields().unwrap()["cards"], 0);
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
        ("116,1,2,3,0", "delimiter-missing"),
    ] {
        let result = decode(point_file(parameters));

        let native = result.ir().native.namespace("iges").unwrap();
        let quarantined = &native.arenas()["quarantined_parameter_records"];
        assert_eq!(quarantined.len(), 1, "{defect}");
        assert_eq!(quarantined[0].fields().unwrap()["defect"], defect);
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
fn a_record_with_no_delimiter_quarantines_the_parameter_data() {
    let result = decode(point_file("116,1,2,3,0"));

    let native = result.ir().native.namespace("iges").unwrap();
    let quarantined = &native.arenas()["quarantined_parameter_records"];
    assert_eq!(quarantined.len(), 1);
    assert_eq!(
        quarantined[0].fields().unwrap()["defect"],
        "delimiter-missing"
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterDataQuarantined),
        1
    );
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
    let unknowns = native
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    let fidelity = result.source_fidelity();
    let part = &fidelity.parts()[&unknowns[0].id];
    assert_eq!(part.level().as_str(), "iges.parameter_record");
    assert_eq!(
        part.state(),
        &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
    );
    let start = parameter_card_offset(&bytes);
    assert_eq!(part.offset(), u64::try_from(start).unwrap());
    assert_eq!(part.byte_len(), 81);
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(&bytes[start..start + 81])
    );
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
