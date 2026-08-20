// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::DecodeReport;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

fn code_count(report: &DecodeReport, code: IgesLossCode) -> usize {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

#[test]
fn decode_general_note_defaulted_final_string_claims_no_trailing_property_group() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00000200",
            parameters: "212,2,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 7,
            label: "PROP".into(),
            status: "00000200",
            parameters: "406,1,1HX;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = &native.arenas["entities"][0];
    let annotation = &native.arenas["annotations"][0];

    assert!(entity.fields()["property_links"][0].is_null());
    assert_eq!(annotation.fields()["declared_string_count"], 2);
    assert_eq!(annotation.fields()["strings"].as_array().unwrap().len(), 2);
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterCountOverdeclared),
        0
    );
}

#[test]
fn decode_new_general_note_reads_a_final_string_present_in_part() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 213,
        form: 0,
        label: "NOTE".into(),
        status: "00000200",
        parameters: "213,1,1,0,0,0,0,0,0,0,0,0,2,1,1,1,1,0,1,0,0H,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,1HA,1,1,1,1,0,1,0,0H,2;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];
    let fields = annotation.fields();
    let strings = fields["strings"].as_array().unwrap();

    assert_eq!(fields["declared_string_count"], 2);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[1]["text"]["declared_character_count"], 2);
    assert!(strings[1]["text"]["text"].is_null());
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterCountOverdeclared),
        0
    );
}

fn general_note_string_fields(text: &str) -> Vec<String> {
    vec![
        text.len().to_string(),
        "20".into(),
        "4".into(),
        "1".into(),
        std::f64::consts::FRAC_PI_2.to_string(),
        "0".into(),
        "0".into(),
        "0".into(),
        "1".into(),
        "2".into(),
        "0".into(),
        format!("{}H{text}", text.len()),
    ]
}

fn new_general_note_string_fields(text: &str) -> Vec<String> {
    vec![
        "0".into(),
        "2".into(),
        "3".into(),
        "-0.5".into(),
        "0".into(),
        "18".into(),
        "0".into(),
        "4HTUNL".into(),
        text.len().to_string(),
        "12".into(),
        "3".into(),
        "1".into(),
        std::f64::consts::FRAC_PI_2.to_string(),
        "0".into(),
        "0".into(),
        "0".into(),
        "2".into(),
        "18".into(),
        "0".into(),
        format!("{}H{text}", text.len()),
    ]
}

fn general_note_parameters(declared: usize, complete: &[&str], partial_fields: usize) -> String {
    let mut tokens = vec!["212".to_string(), declared.to_string()];
    for text in complete {
        tokens.extend(general_note_string_fields(text));
    }
    tokens.extend(
        general_note_string_fields("")
            .into_iter()
            .take(partial_fields),
    );
    format!("{};", tokens.join(","))
}

fn new_general_note_parameters(
    declared: usize,
    complete: &[&str],
    partial_fields: usize,
) -> String {
    let mut tokens = vec![
        "213".to_string(),
        "40".into(),
        "20".into(),
        "2".into(),
        "0".into(),
        "20".into(),
        "0".into(),
        "0".into(),
        "0".into(),
        "18".into(),
        "0".into(),
        "-5".into(),
        declared.to_string(),
    ];
    for text in complete {
        tokens.extend(new_general_note_string_fields(text));
    }
    tokens.extend(
        new_general_note_string_fields("")
            .into_iter()
            .take(partial_fields),
    );
    format!("{};", tokens.join(","))
}

fn note_file(entity_type: i64, parameters: String) -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type,
        form: 0,
        label: "NOTE".into(),
        status: "00000100",
        parameters,
    }])
}

#[test]
fn decode_general_note_defaulted_final_string_keeps_every_declared_string() {
    let bytes = note_file(212, general_note_parameters(2, &["ALPHA"], 5));
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];
    let fields = annotation.fields();
    let strings = fields["strings"].as_array().unwrap();

    assert_eq!(fields["declared_string_count"], 2);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0]["text"][0], 65);
    assert_eq!(strings[1]["declared_character_count"], 0);
    assert!(strings[1]["text"].is_null());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_new_general_note_defaulted_final_string_agrees_with_the_neutral_projection() {
    let bytes = note_file(213, new_general_note_parameters(2, &["TOL!"], 9));
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];
    let fields = annotation.fields();
    let strings = fields["strings"].as_array().unwrap();

    assert_eq!(fields["declared_string_count"], 2);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0]["text"]["text"][0], 84);
    assert_eq!(strings[1]["text"]["declared_character_count"], 0);
    assert!(strings[1]["text"]["text"].is_null());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_general_note_surplus_tokens_read_the_declared_strings_and_refuse_the_projection() {
    let declared = 1;
    let complete = ["ALPHA"];
    let bytes = note_file(212, general_note_parameters(declared, &complete, 2));
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];
    let fields = annotation.fields();
    let strings = fields["strings"].as_array().unwrap();

    assert_eq!(fields["declared_string_count"], declared);
    assert_eq!(strings.len(), complete.len());
    assert_eq!(strings[0]["text"][0], u64::from(b'A'));
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterCountOverdeclared),
        0
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterBoundaryAmbiguous),
        0
    );
    assert_eq!(
        code_count(result.report(), IgesLossCode::EntityNotProjected),
        1
    );
}

#[test]
fn decode_new_general_note_overdeclared_count_reads_no_string_and_charges_the_loss() {
    let bytes = note_file(213, new_general_note_parameters(2, &["TOL!"], 0));

    for container_only in [false, true] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(bytes.clone()),
                &DecodeOptions {
                    container_only,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];

        assert_eq!(annotation.fields()["declared_string_count"], 2);
        assert!(annotation.fields()["strings"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            code_count(result.report(), IgesLossCode::ParameterCountOverdeclared),
            1
        );
        assert_eq!(
            code_count(result.report(), IgesLossCode::EntityNotProjected),
            usize::from(!container_only)
        );
    }

    let mut strict = DecodeOptions::default();
    strict.policy.mode = DecodeMode::Strict;
    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict)
        .unwrap_err();
    match error {
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::ParameterCountOverdeclared.kind().as_str()
        ),
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}
