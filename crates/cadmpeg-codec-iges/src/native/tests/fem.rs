// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use serde_json::json;

use crate::test_support::{owned_test_file_with_global, OwnedTestEntity};
use crate::IgesCodec;

const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
const GLOBAL_V5_0: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

fn fem_entities() -> Vec<OwnedTestEntity> {
    vec![
        OwnedTestEntity {
            entity_type: 134,
            form: 0,
            label: "NODE".into(),
            status: "00000000",
            parameters: "134,1.0,2.0,3.0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 136,
            form: 0,
            label: "ELEMENT".into(),
            status: "00000000",
            parameters: "136,1,1,1,4HBEAM;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "CASE".into(),
            status: "00000000",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 11,
            label: "LOAD".into(),
            status: "00000000",
            parameters: "406,7,5,1,1,3,10,20;".into(),
        },
        OwnedTestEntity {
            entity_type: 138,
            form: 0,
            label: "DISP".into(),
            status: "00000000",
            parameters: "138,1,5,1,1,1,0.1,0.2,0.3,0.01,0.02,0.03;".into(),
        },
        OwnedTestEntity {
            entity_type: 146,
            form: 3,
            label: "NRESULT".into(),
            status: "00000000",
            parameters: "146,5,0,0.0,3,1,1,1,1.0,2.0,3.0;".into(),
        },
        OwnedTestEntity {
            entity_type: 148,
            form: 3,
            label: "ERESULT".into(),
            status: "00000000",
            parameters: "148,5,0,0.0,3,0,1,1,3,1,1,0,1,0,3,4.0,5.0,6.0;".into(),
        },
        OwnedTestEntity {
            entity_type: 418,
            form: 0,
            label: "CONSTRNT".into(),
            status: "00000000",
            parameters: "418,1,1,1,7;".into(),
        },
    ]
}

fn decode_fem(global: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    let bytes = owned_test_file_with_global(&fem_entities(), global);
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn assert_fem_namespace(global: &[u8]) {
    let result = decode_fem(global);
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.version, 6);
    let fem = &native.arenas["fem_entities"];
    assert_eq!(fem.len(), 6);

    let node = fem
        .iter()
        .find(|record| record.fields()["kind"] == "node")
        .unwrap();
    assert_eq!(node.fields()["source_entity"], "iges:entity:directory#1");
    assert_eq!(node.fields()["coordinates"], json!([1.0, 2.0, 3.0]));

    let element = fem
        .iter()
        .find(|record| record.fields()["kind"] == "finite_element")
        .unwrap();
    assert_eq!(element.fields()["topology_type"], 1);
    assert_eq!(element.fields()["declared_node_count"], 1);
    assert_eq!(
        element.fields()["nodes"],
        json!(["iges:entity:directory#1"])
    );
    assert_eq!(element.fields()["element_type"], json!([66, 69, 65, 77]));

    let displacement = fem
        .iter()
        .find(|record| record.fields()["kind"] == "nodal_displacement_rotation")
        .unwrap();
    assert_eq!(
        displacement.fields()["case_descriptions"],
        json!(["iges:entity:directory#5"])
    );
    assert_eq!(
        displacement.fields()["nodes"][0]["translations"],
        json!([[0.1, 0.2, 0.3]])
    );
    assert_eq!(
        displacement.fields()["nodes"][0]["rotations"],
        json!([[0.01, 0.02, 0.03]])
    );

    let nodal_results = fem
        .iter()
        .find(|record| record.fields()["kind"] == "nodal_results")
        .unwrap();
    assert_eq!(nodal_results.fields()["expected_value_count"], 3);
    assert_eq!(
        nodal_results.fields()["nodes"][0]["values"],
        json!([1.0, 2.0, 3.0])
    );

    let element_results = fem
        .iter()
        .find(|record| record.fields()["kind"] == "element_results")
        .unwrap();
    assert_eq!(element_results.fields()["expected_value_count"], 3);
    assert_eq!(
        element_results.fields()["elements"][0]["element"],
        "iges:entity:directory#3"
    );
    assert_eq!(
        element_results.fields()["elements"][0]["report_locations"],
        json!([0])
    );
    assert_eq!(
        element_results.fields()["elements"][0]["values"],
        json!([4.0, 5.0, 6.0])
    );

    let load = fem
        .iter()
        .find(|record| record.fields()["kind"] == "nodal_load_constraint")
        .unwrap();
    assert_eq!(load.fields()["node"], "iges:entity:directory#1");
    assert_eq!(
        load.fields()["case_references"],
        json!(["iges:entity:directory#7"])
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("is outside the Fixed ASCII mechanical/document envelope")
    }));
}

#[test]
fn standard_fem_entities_decode_in_iges_4_and_5_0() {
    assert_fem_namespace(GLOBAL_V4);
    assert_fem_namespace(GLOBAL_V5_0);
}

#[test]
fn incomplete_element_result_items_do_not_allocate_or_project_values() {
    let bytes = owned_test_file_with_global(
        &[OwnedTestEntity {
            entity_type: 148,
            form: 3,
            label: "ERESULT".into(),
            status: "00000000",
            parameters: "148,0,0,0.0,3,0,1,1,3,1,1,0,1,0,3,4.0,5.0;".into(),
        }],
        GLOBAL_V5_0,
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let record = &result.ir().native.namespace("iges").unwrap().arenas["fem_entities"][0];
    assert_eq!(record.fields()["kind"], "element_results");
    assert!(record.fields()["elements"].as_array().unwrap().is_empty());
}
