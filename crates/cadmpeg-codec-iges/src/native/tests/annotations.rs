// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::code_count;
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

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
    new_general_note_string_fields_with_font(text, "1")
}

fn new_general_note_string_fields_with_font(text: &str, font: &str) -> Vec<String> {
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
        font.to_owned(),
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

fn new_general_note_header(declared: usize) -> Vec<String> {
    vec![
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
    ]
}

fn new_general_note_parameters(
    declared: usize,
    complete: &[&str],
    partial_fields: usize,
) -> String {
    let mut tokens = new_general_note_header(declared);
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
    note_file_with_form(entity_type, 0, parameters)
}

fn note_file_with_form(entity_type: i64, form: i64, parameters: String) -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type,
        form,
        label: "NOTE".into(),
        status: "00000100",
        parameters,
    }])
}

#[test]
fn decode_v5_general_note_one_blank_string_as_null() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let file = |global: &[u8]| {
        owned_test_file_with_global(
            &[OwnedTestEntity {
                entity_type: 212,
                form: 0,
                label: "NOTE".into(),
                status: "00000100",
                parameters: general_note_parameters(1, &[" "], 0),
            }],
            global,
        )
    };

    let v4 = IgesCodec
        .decode(&mut Cursor::new(file(global_v4)), &DecodeOptions::default())
        .unwrap();
    let v4_text = &v4.ir().native.namespace("iges").unwrap().arenas["annotations"][0].fields()
        ["strings"][0]["text"];
    assert_eq!(v4_text[0], u64::from(b' '));

    let v5 = IgesCodec
        .decode(&mut Cursor::new(file(global_v5)), &DecodeOptions::default())
        .unwrap();
    let v5_text =
        &v5.ir().native.namespace("iges").unwrap().arenas["annotations"][0].fields()["strings"][0];
    assert_eq!(v5_text["declared_character_count"], 1);
    assert!(v5_text["text"].is_null());
}

#[test]
fn decode_general_note_preserves_non_simple_form() {
    let bytes = note_file_with_form(212, 7, general_note_parameters(2, &["TOP", "BOTTOM"], 0));
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];

    assert_eq!(annotation.fields()["form"], 7);
    assert_eq!(annotation.fields()["strings"].as_array().unwrap().len(), 2);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_general_note_accepts_each_standard_form_at_its_minimum_count() {
    for (form, count) in [
        (0, 1),
        (1, 2),
        (2, 2),
        (3, 2),
        (4, 2),
        (5, 3),
        (6, 1),
        (7, 1),
        (8, 1),
        (100, 4),
        (101, 8),
        (102, 9),
        (105, 12),
    ] {
        let strings = vec!["X"; count];
        let bytes = note_file_with_form(212, form, general_note_parameters(count, &strings, 0));
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];

        assert_eq!(annotation.fields()["form"], form);
        assert_eq!(
            annotation.fields()["strings"].as_array().unwrap().len(),
            count
        );
        assert!(
            result.report().losses.is_empty(),
            "form {form}: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn decode_general_note_keeps_primary_projection_when_trailing_groups_are_invalid() {
    let primary = general_note_parameters(1, &["ALPHA"], 0);
    let parameters = format!("{},1,2;", primary.trim_end_matches(';'));
    let bytes = note_file_with_form(212, 7, parameters);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];

    assert_eq!(annotation.fields()["form"], 7);
    assert_eq!(annotation.fields()["strings"].as_array().unwrap().len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
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
    assert_eq!(strings[0]["text"][0], u64::from(b'A'));
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
    assert_eq!(strings[0]["text"]["text"][0], u64::from(b'T'));
    assert_eq!(strings[1]["text"]["declared_character_count"], 0);
    assert!(strings[1]["text"]["text"].is_null());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_general_note_surplus_tokens_read_the_declared_strings_and_refuse_the_semantic_projection()
{
    let complete = ["ALPHA"];
    let declared = complete.len();
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

    let strict = DecodeOptions {
        policy: DecodePolicy {
            mode: DecodeMode::Strict,
            ..DecodePolicy::default()
        },
        ..DecodeOptions::default()
    };
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

#[test]
fn decode_new_general_note_resolves_only_the_text_font_pointer_that_names_a_type_310() {
    let fonts = ["-1", "-5"];
    let mut tokens = new_general_note_header(fonts.len());
    for (index, font) in fonts.into_iter().enumerate() {
        tokens.extend(new_general_note_string_fields_with_font(
            &format!("RUN{index}"),
            font,
        ));
    }
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "FONT".into(),
            status: "00000200",
            parameters: "310,101,4HBASE,,10,1,65,8,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "NOTE".into(),
            status: "00000100",
            parameters: format!("{};", tokens.join(",")),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let annotation = &native.arenas["annotations"][0];
    let fields = annotation.fields();
    let strings = fields["strings"].as_array().unwrap();

    assert_eq!(strings.len(), fonts.len());
    assert_eq!(
        strings[0]["text"]["font_definition"],
        "iges:presentation:text-font#D1"
    );
    assert!(strings[1]["text"]["font_definition"].is_null());
    assert_eq!(strings[0]["text"]["font_code"], -1);
    assert_eq!(strings[1]["text"]["font_code"], -5);

    let note = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    let note_fields = note.fields();
    let pointers = note_fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|reference| reference["expected"] == "type-310-form-0")
        .collect::<Vec<_>>();

    assert_eq!(pointers.len(), fonts.len());
    // The font pointer sits eleven tokens into each twenty-token 213 string
    // block: the eight-token prefix plus the text run's three-slot offset.
    assert_eq!(pointers[0]["parameter_index"], 24);
    assert_eq!(pointers[0]["resolution"], "resolved");
    assert_eq!(pointers[1]["parameter_index"], 44);
    assert_eq!(pointers[1]["resolution"], "dangling");
}
