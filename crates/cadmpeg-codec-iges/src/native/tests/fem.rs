// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use serde_json::json;

use crate::test_support::{owned_test_file_with_global, OwnedTestEntity};
use crate::IgesCodec;

const GLOBAL_V4: &[u8] = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,7Hproduct,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
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

fn finite_element_parameters(topology_type: i64, node_count: usize, element_type: &str) -> String {
    let nodes = (0..node_count).map(|_| "1").collect::<Vec<_>>().join(",");
    format!(
        "136,{topology_type},{node_count},{nodes},{name_length}H{element_type};",
        name_length = element_type.len()
    )
}

fn fem_topology_entities(topologies: &[(i64, usize, &str)]) -> Vec<OwnedTestEntity> {
    let mut entities = vec![OwnedTestEntity {
        entity_type: 134,
        form: 0,
        label: "NODE".into(),
        status: "00000000",
        parameters: "134,1.0,2.0,3.0,0;".into(),
    }];
    entities.extend(
        topologies.iter().map(
            |&(topology_type, node_count, element_type)| OwnedTestEntity {
                entity_type: 136,
                form: 0,
                label: format!("E{topology_type}"),
                status: "00000000",
                parameters: finite_element_parameters(topology_type, node_count, element_type),
            },
        ),
    );
    entities
}

fn assert_fem_topologies(global: &[u8], topologies: &[(i64, usize, &str)]) {
    let bytes = owned_test_file_with_global(&fem_topology_entities(topologies), global);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let fem = &result.ir().native.namespace("iges").unwrap().arenas["fem_entities"];
    assert_eq!(fem.len(), topologies.len() + 1);

    for &(topology_type, node_count, element_type) in topologies {
        let element = fem
            .iter()
            .find(|record| {
                record.fields()["kind"] == "finite_element"
                    && record.fields()["topology_type"] == topology_type
            })
            .unwrap();
        assert_eq!(element.fields()["declared_node_count"], node_count);
        assert_eq!(
            element.fields()["nodes"].as_array().unwrap().len(),
            node_count
        );
        assert_eq!(
            element.fields()["element_type"],
            json!(element_type.as_bytes())
        );
    }
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
fn v5_fem_topology_additions_preserve_their_declared_connectivity() {
    const TOPOLOGIES: &[(i64, usize, &str)] = &[
        (34, 2, "OMASS"),
        (35, 4, "OFBEAM"),
        (36, 3, "PBEAM"),
        (37, 3, "CBEAM"),
        (38, 21, "CPSOW"),
    ];
    assert_fem_topologies(GLOBAL_V5_0, TOPOLOGIES);
}

#[test]
fn implementor_defined_fem_topology_is_retained_in_iges_4_and_5_0() {
    const TOPOLOGIES: &[(i64, usize, &str)] = &[(5001, 1, "USER")];
    assert_fem_topologies(GLOBAL_V4, TOPOLOGIES);
    assert_fem_topologies(GLOBAL_V5_0, TOPOLOGIES);
}

#[test]
fn finite_element_missing_node_keeps_its_declared_slot() {
    let bytes = owned_test_file_with_global(
        &[
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
                label: "OMASS".into(),
                status: "00000000",
                parameters: "136,34,2,1,0,5HOMASS;".into(),
            },
        ],
        GLOBAL_V5_0,
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let element = result.ir().native.namespace("iges").unwrap().arenas["fem_entities"]
        .iter()
        .find(|record| record.fields()["kind"] == "finite_element")
        .unwrap();
    assert_eq!(
        element.fields()["nodes"],
        json!(["iges:entity:directory#1", null])
    );
}

#[test]
fn finite_element_additional_property_group_is_retained_on_generic_entity() {
    let bytes = owned_test_file_with_global(
        &[
            OwnedTestEntity {
                entity_type: 134,
                form: 0,
                label: "NODE".into(),
                status: "00000000",
                parameters: "134,1.0,2.0,3.0,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 406,
                form: 1,
                label: "PROPERTY".into(),
                status: "00000000",
                parameters: "406,1,1;".into(),
            },
            OwnedTestEntity {
                entity_type: 136,
                form: 0,
                label: "ELEMENT".into(),
                status: "00000000",
                parameters: "136,5001,1,1,4HUSER,0,1,3;".into(),
            },
        ],
        GLOBAL_V5_0,
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let entity = result.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|record| record.fields()["directory_sequence"] == 5)
        .unwrap();
    assert_eq!(
        entity.fields()["property_links"],
        json!(["iges:entity:directory#3"])
    );
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
