// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::global::Dialect;
use crate::test_support::*;
use crate::IgesCodec;

fn matrix_admits(forms: &(Vec<i64>, bool), form: i64) -> bool {
    forms.0.contains(&form) || (forms.1 && matches!(form, 5001..=9999))
}

#[test]
fn envelope_admission_exactly_matches_the_machine_matrix() {
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    let mut admitted = BTreeMap::<i64, (Vec<i64>, bool)>::new();
    for entity in matrix["entity"].as_array().unwrap() {
        let entity_type = entity["type"].as_integer().unwrap();
        let forms = entity["forms"]
            .as_array()
            .map(|forms| {
                forms
                    .iter()
                    .map(|form| form.as_integer().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        let implementor_defined = entity["forms"].as_str() == Some("implementor-defined")
            || entity
                .get("implementor_defined")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
        assert!(admitted
            .insert(entity_type, (forms, implementor_defined))
            .is_none());
        for required in ["name", "domain", "decoder", "destination"] {
            assert!(entity[required]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
        }
        for required in ["fixture_classes", "assertions"] {
            assert!(entity[required]
                .as_array()
                .is_some_and(|values| !values.is_empty()));
        }
    }
    for entity_type in 0..=600 {
        for form in -1..=100 {
            let expected = admitted
                .get(&entity_type)
                .is_some_and(|forms| matrix_admits(forms, form));
            assert_eq!(
                crate::profile::envelope_a_admits(entity_type, form, Dialect::V5_3),
                expected,
                "entity type {entity_type} form {form}"
            );
        }
    }
    for (&entity_type, forms) in &admitted {
        for form in [101, 5000, 5001, 9999, 10000, i64::MAX] {
            let expected = matrix_admits(forms, form);
            assert_eq!(
                crate::profile::envelope_a_admits(entity_type, form, Dialect::V5_3),
                expected,
                "high-form probe: entity type {entity_type} form {form}"
            );
        }
    }
    assert!(!crate::profile::envelope_a_admits(601, 5001, Dialect::V5_3));
    assert!(!crate::profile::envelope_a_admits(
        i64::MAX,
        i64::MAX,
        Dialect::V5_3
    ));
}

#[test]
fn v4_admission_matches_its_entity_and_form_table() {
    let cases = [
        (123, 0, false),
        (141, 0, false),
        (143, 0, false),
        (182, 0, false),
        (186, 0, false),
        (190, 0, false),
        (192, 0, false),
        (194, 0, false),
        (196, 0, false),
        (198, 0, false),
        (204, 0, false),
        (213, 0, false),
        (316, 0, false),
        (502, 1, false),
        (504, 1, false),
        (508, 1, false),
        (510, 1, false),
        (514, 1, false),
        (110, 0, true),
        (110, 1, false),
        (110, 2, false),
        (118, 0, true),
        (118, 1, true),
        (214, 11, true),
        (214, 12, false),
        (216, 0, true),
        (216, 1, false),
        (218, 0, true),
        (218, 1, false),
        (125, 0, true),
        (125, 4, true),
        (125, 5, false),
        (402, 5, true),
        (402, 6, false),
        (402, 7, true),
        (402, 8, false),
        (402, 9, true),
        (402, 10, false),
        (402, 11, false),
        (402, 12, true),
        (402, 16, true),
        (402, 17, false),
        (402, 18, true),
        (402, 19, false),
        (402, 21, false),
        (404, 0, true),
        (404, 1, false),
        (228, 0, true),
        (228, 1, true),
        (228, 3, true),
        (228, 4, false),
        (228, 5001, false),
        (230, 0, true),
        (230, 1, false),
        (406, 3, true),
        (406, 4, false),
        (406, 18, true),
        (406, 19, false),
        (406, 36, false),
        (406, 5001, true),
        (406, 9999, true),
        (406, 10000, false),
        (410, 0, true),
        (410, 1, false),
        (416, 2, true),
        (416, 3, false),
        (416, 4, false),
        (430, 0, true),
        (430, 1, false),
    ];
    for (entity_type, form, expected) in cases {
        assert_eq!(
            crate::profile::envelope_a_admits(entity_type, form, Dialect::V4_0),
            expected,
            "entity type {entity_type} form {form}"
        );
    }
}

#[test]
fn type230_form1_is_admitted_from_iges_5_0_onward() {
    assert!(!crate::profile::envelope_a_admits(230, 1, Dialect::V4_0));
    for dialect in [Dialect::V5_0, Dialect::V5_1, Dialect::V5_2, Dialect::V5_3] {
        assert!(
            crate::profile::envelope_a_admits(230, 1, dialect),
            "{dialect:?}"
        );
    }
}

#[test]
fn type228_implementor_forms_are_admitted_from_iges_5_0_onward() {
    assert!(!crate::profile::envelope_a_admits(228, 5001, Dialect::V4_0));
    for dialect in [Dialect::V5_0, Dialect::V5_1, Dialect::V5_2, Dialect::V5_3] {
        assert!(
            crate::profile::envelope_a_admits(228, 5001, dialect),
            "{dialect:?}"
        );
    }
}

#[test]
fn implementor_defined_property_forms_are_admitted_in_each_fixed_ascii_dialect() {
    for dialect in [
        Dialect::V4_0,
        Dialect::V5_0,
        Dialect::V5_1,
        Dialect::V5_2,
        Dialect::V5_3,
    ] {
        assert!(crate::profile::envelope_a_admits(406, 5001, dialect));
        assert!(crate::profile::envelope_a_admits(406, 9999, dialect));
        assert!(!crate::profile::envelope_a_admits(406, 5000, dialect));
        assert!(!crate::profile::envelope_a_admits(406, 10000, dialect));
    }
}

#[test]
fn decode_names_forms_outside_the_closed_envelope() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 430,
        form: 2,
        label: "BADFORM".into(),
        status: "00000000",
        parameters: "430,0;".into(),
    }]);
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            == "IGES entity type 430 form 2 is outside the Fixed ASCII mechanical/document envelope"
    }));
}
