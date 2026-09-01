// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::global::GlobalTable;
use crate::test_support::*;
use crate::IgesCodec;

fn matrix_admits(forms: &(Vec<i64>, bool), form: i64) -> bool {
    forms.0.contains(&form) || (forms.1 && matches!(form, 5001..=9999))
}

fn matrix_range_admits(ranges: &[toml::Value], entity_type: i64, _form: i64) -> bool {
    ranges.iter().any(|range| {
        let min = range["min"].as_integer();
        let max = range["max"].as_integer();
        min.zip(max)
            .is_some_and(|(min, max)| (min..=max).contains(&entity_type))
            && range["forms"].as_str() == Some("any")
    })
}

#[test]
fn envelope_admission_exactly_matches_the_machine_matrix() {
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    let ranges = matrix["entity_range"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for range in &ranges {
        for required in ["name", "domain", "decoder", "destination"] {
            assert!(range[required]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
        }
        for required in ["fixture_classes", "assertions"] {
            assert!(range[required]
                .as_array()
                .is_some_and(|values| !values.is_empty()));
        }
    }
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
                .is_some_and(|forms| matrix_admits(forms, form))
                || matrix_range_admits(&ranges, entity_type, form);
            assert_eq!(
                crate::profile::envelope_a_admits(entity_type, form, GlobalTable::V5Later),
                expected,
                "entity type {entity_type} form {form}"
            );
        }
    }
    for (&entity_type, forms) in &admitted {
        for form in [101, 5000, 5001, 9999, 10000, i64::MAX] {
            let expected =
                matrix_admits(forms, form) || matrix_range_admits(&ranges, entity_type, form);
            assert_eq!(
                crate::profile::envelope_a_admits(entity_type, form, GlobalTable::V5Later),
                expected,
                "high-form probe: entity type {entity_type} form {form}"
            );
        }
    }
    assert!(crate::profile::envelope_a_admits(
        601,
        5001,
        GlobalTable::V5Later
    ));
    assert!(crate::profile::envelope_a_admits(
        10_000,
        i64::MAX,
        GlobalTable::V5Later
    ));
    assert!(!crate::profile::envelope_a_admits(
        700,
        0,
        GlobalTable::V5Later
    ));
    assert!(!crate::profile::envelope_a_admits(
        100_000,
        0,
        GlobalTable::V5Later
    ));
    assert!(!crate::profile::envelope_a_admits(
        i64::MAX,
        i64::MAX,
        GlobalTable::V5Later
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
        (322, 0, false),
        (322, 2, false),
        (422, 0, false),
        (422, 1, false),
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
        (306, 0, true),
        (306, 1, false),
        (134, 0, true),
        (134, 1, false),
        (136, 0, true),
        (136, 1, false),
        (138, 0, true),
        (138, 1, false),
        (146, 0, true),
        (146, 34, true),
        (146, 35, false),
        (148, 0, true),
        (148, 34, true),
        (148, 35, false),
        (180, 0, true),
        (180, 1, false),
        (184, 0, true),
        (184, 1, false),
        (418, 0, true),
        (418, 1, false),
        (402, 5, true),
        (402, 6, true),
        (402, 7, true),
        (402, 8, true),
        (402, 9, true),
        (402, 10, true),
        (402, 11, true),
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
        (406, 4, true),
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
            crate::profile::envelope_a_admits(entity_type, form, GlobalTable::V4_0),
            expected,
            "entity type {entity_type} form {form}"
        );
    }
}

#[test]
fn standard_fem_forms_are_admitted_in_v4_and_v5() {
    for global_table in [GlobalTable::V4_0, GlobalTable::V5_0, GlobalTable::V5Later] {
        for entity_type in [134, 136, 138, 418] {
            assert!(
                crate::profile::envelope_a_admits(entity_type, 0, global_table),
                "{global_table:?} Type {entity_type} Form 0"
            );
            assert!(!crate::profile::envelope_a_admits(
                entity_type,
                1,
                global_table
            ));
        }
        for entity_type in [146, 148] {
            assert!(crate::profile::envelope_a_admits(
                entity_type,
                0,
                global_table
            ));
            assert!(crate::profile::envelope_a_admits(
                entity_type,
                34,
                global_table
            ));
            assert!(!crate::profile::envelope_a_admits(
                entity_type,
                35,
                global_table
            ));
        }
    }
}

#[test]
fn macro_instance_ranges_are_admitted_in_all_fixed_ascii_dialects() {
    for global_table in [GlobalTable::V4_0, GlobalTable::V5_0, GlobalTable::V5Later] {
        for entity_type in [600, 699, 10_000, 99_999] {
            assert!(crate::profile::envelope_a_admits(
                entity_type,
                0,
                global_table
            ));
            assert!(crate::profile::envelope_a_admits(
                entity_type,
                5000,
                global_table
            ));
        }
        for entity_type in [599, 700, 9999, 100_000] {
            assert!(!crate::profile::envelope_a_admits(
                entity_type,
                0,
                global_table
            ));
        }
    }
}

#[test]
fn type230_form1_is_admitted_from_iges_5_0_onward() {
    assert!(!crate::profile::envelope_a_admits(
        230,
        1,
        GlobalTable::V4_0
    ));
    for global_table in [GlobalTable::V5_0, GlobalTable::V5Later] {
        assert!(
            crate::profile::envelope_a_admits(230, 1, global_table),
            "{global_table:?}"
        );
    }
}

#[test]
fn type228_implementor_forms_are_admitted_from_iges_5_0_onward() {
    assert!(!crate::profile::envelope_a_admits(
        228,
        5001,
        GlobalTable::V4_0
    ));
    for global_table in [GlobalTable::V5_0, GlobalTable::V5Later] {
        assert!(
            crate::profile::envelope_a_admits(228, 5001, global_table),
            "{global_table:?}"
        );
    }
}

#[test]
fn v5_0_admission_is_the_4_0_table_plus_v5_0_ecos() {
    let cases = [
        // V5.0 ECO-created entity additions.
        (306, 0, true),
        (141, 0, true),
        (143, 0, true),
        (182, 0, true),
        (204, 0, true),
        (213, 0, true),
        (316, 0, true),
        (180, 0, true),
        (184, 0, true),
        // V5.0 ECO-created form additions.
        (214, 12, true),
        (216, 1, true),
        (216, 2, true),
        (218, 1, true),
        (228, 5001, true),
        (230, 1, true),
        (402, 19, true),
        (402, 20, false),
        (402, 21, false),
        (404, 1, true),
        (406, 19, true),
        (406, 26, true),
        (410, 1, true),
        (416, 3, true),
        (322, 0, true),
        (322, 2, true),
        (422, 0, true),
        (422, 1, true),
        // B-rep and its analytic carriers were held for IGES 5.1.
        (123, 0, false),
        (186, 0, false),
        (190, 0, false),
        (198, 1, false),
        (502, 1, false),
        (514, 1, false),
        (180, 1, false),
        (184, 1, false),
        // Later gray-page and post-5.0 forms remain outside the profile.
        (402, 6, false),
        (402, 8, false),
        (402, 10, false),
        (402, 11, false),
        (402, 22, false),
        (406, 4, false),
        (406, 27, false),
        (406, 36, false),
        (416, 4, false),
    ];
    for (entity_type, form, expected) in cases {
        assert_eq!(
            crate::profile::envelope_a_admits(entity_type, form, GlobalTable::V5_0),
            expected,
            "entity type {entity_type} form {form}"
        );
    }
}

#[test]
fn implementor_defined_property_forms_are_admitted_in_each_fixed_ascii_dialect() {
    for global_table in [GlobalTable::V4_0, GlobalTable::V5_0, GlobalTable::V5Later] {
        assert!(crate::profile::envelope_a_admits(406, 5001, global_table));
        assert!(crate::profile::envelope_a_admits(406, 9999, global_table));
        assert!(!crate::profile::envelope_a_admits(406, 5000, global_table));
        assert!(!crate::profile::envelope_a_admits(406, 10000, global_table));
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
