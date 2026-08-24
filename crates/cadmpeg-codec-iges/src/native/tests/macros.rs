// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use serde_json::json;

use crate::loss::IgesLossCode;
use crate::test_support::{owned_test_file_with_global_and_directory_fields, OwnedTestEntity};
use crate::IgesCodec;

const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
const GLOBAL_V5_3: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";

fn macro_entities() -> Vec<OwnedTestEntity> {
    vec![
        OwnedTestEntity {
            entity_type: 306,
            form: 0,
            label: "MACRODEF".into(),
            status: "00000000",
            parameters: "306,MACRO,621,X,Y;LET Z=0;ENDM;".into(),
        },
        OwnedTestEntity {
            entity_type: 621,
            form: 0,
            label: "MACRO".into(),
            status: "00000000",
            parameters: "621,1.0,2.0;".into(),
        },
    ]
}

#[test]
fn macro_definition_and_instance_are_retained_in_v4_and_v5_profiles() {
    for global in [GLOBAL_V4, GLOBAL_V5_0, GLOBAL_V5_3] {
        let bytes = owned_test_file_with_global_and_directory_fields(
            &macro_entities(),
            global,
            &[],
            &[],
            &[],
            &[],
            &[(3, -1)],
        );
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        let definitions = &native.arenas["macro_definitions"];
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].fields()["defined_entity_type"], 621);
        assert_eq!(
            definitions[0].fields()["macro_statement"],
            json!(b"306,MACRO,621,X,Y")
        );
        assert_eq!(
            definitions[0].fields()["language_statements"],
            json!([b"LET Z=0"])
        );
        assert_eq!(definitions[0].fields()["end_statement"], json!(b"ENDM"));

        let instances = &native.arenas["macro_instances"];
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].fields()["entity_type"], 621);
        assert_eq!(
            instances[0].fields()["macro_definition"],
            "iges:entity:directory#1"
        );
        assert!(instances[0].fields()["macro_library"].is_null());
        assert_eq!(
            instances[0].fields()["parameters"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(!result.ir().native.namespace("iges").unwrap().arenas["entities"].is_empty());
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityOutsideEnvelope.kind()));
    }
}

#[test]
fn malformed_macro_definition_is_quarantined_without_outside_envelope_loss() {
    let bytes = owned_test_file_with_global_and_directory_fields(
        &[OwnedTestEntity {
            entity_type: 306,
            form: 0,
            label: "BADMACRO".into(),
            status: "00000000",
            parameters: "306,MACRO,621,X;".into(),
        }],
        GLOBAL_V5_0,
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    assert!(!native.arenas.contains_key("macro_definitions"));
    assert_eq!(native.arenas["quarantined_parameter_records"].len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::ParameterDataQuarantined.kind()
            && loss.message.contains("no ENDM statement")
    }));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityOutsideEnvelope.kind()));
}

#[test]
fn macro_instance_retains_a_type416_library_reference() {
    let bytes = owned_test_file_with_global_and_directory_fields(
        &[
            OwnedTestEntity {
                entity_type: 621,
                form: 7,
                label: "LIBMACRO".into(),
                status: "00000000",
                parameters: "621,1.0;".into(),
            },
            OwnedTestEntity {
                entity_type: 416,
                form: 0,
                label: "LIBRARY".into(),
                status: "00000000",
                parameters: "416,0H;".into(),
            },
        ],
        GLOBAL_V5_0,
        &[],
        &[],
        &[],
        &[],
        &[(1, -3)],
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let instances = &result.ir().native.namespace("iges").unwrap().arenas["macro_instances"];
    assert_eq!(instances.len(), 1);
    assert!(instances[0].fields()["macro_definition"].is_null());
    assert_eq!(
        instances[0].fields()["macro_library"],
        "iges:entity:directory#3"
    );
}
