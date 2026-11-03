// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::writer::encode_file;
use cadmpeg_ir::ids::Identity;
use serde_json::json;
use std::collections::BTreeMap;

fn install(ir: &mut CadIr, arena: &str, id: &str, fields: &Value) {
    ir.native
        .namespace_mut("iges")
        .arenas_mut()
        .entry(arena.into())
        .or_default()
        .push(
            NativeRecord::new(
                Identity::new(id).expect("valid native fixture record identity"),
                fields
                    .as_object()
                    .expect("native fixture fields are an object")
                    .clone(),
            )
            .expect("fixture native fields admit as a record"),
        );
}

fn owner() -> Entity {
    let mut owner = entity(
        110,
        0,
        vec![
            "0".into(),
            "0".into(),
            "0".into(),
            "1".into(),
            "0".into(),
            "0".into(),
        ],
    );
    owner.source_targets.push("iges:entity:directory#1".into());
    owner
}

#[test]
fn directory_pattern_number_is_written_from_typed_display() {
    let mut ir = CadIr::empty();
    install(
        &mut ir,
        "display_attributes",
        "iges:presentation:display#D1",
        &json!({
            "source_entity": "iges:entity:directory#1", "line_font_number": 3,
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(matches!(
        entities[0].line_font,
        Some(DirectoryLineFont::Number(3))
    ));
    let bytes = encode_file(
        &ir,
        &entities,
        &BTreeMap::new(),
        crate::IgesVersion::V5_3,
        ir.tolerances.linear.get(),
    )
    .expect("typed directory pattern encodes");
    let directory = bytes
        .split(|byte| *byte == b'\n')
        .find(|line| line.get(72) == Some(&b'D'))
        .expect("encoded file contains a directory record");
    assert_eq!(&directory[24..32], b"       3");
}

#[test]
fn reemitted_font_definition_uses_current_directory_pointer() {
    let mut ir = CadIr::empty();
    let font = "iges:presentation:line-font#D99";
    install(
        &mut ir,
        "line_fonts",
        font,
        &json!({
            "source_entity":"iges:entity:directory#99", "kind":"visible_blank_pattern",
            "fallback_line_font_number": 1, "segment_count": 2, "lengths": [2.0, 1.0],
            "hexadecimal_pattern": "41",
        }),
    );
    install(
        &mut ir,
        "display_attributes",
        "iges:presentation:display#D1",
        &json!({
            "source_entity":"iges:entity:directory#1", "line_font_number": -99,
            "line_font_definition": font,
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(font));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].type_code, 304);
    assert!(matches!(
        entities[1].line_font,
        Some(DirectoryLineFont::Number(1))
    ));
    assert!(matches!(
        entities[0].line_font,
        Some(DirectoryLineFont::Definition(1))
    ));
    let bytes = encode_file(
        &ir,
        &entities,
        &BTreeMap::new(),
        crate::IgesVersion::V5_3,
        ir.tolerances.linear.get(),
    )
    .expect("current font definition encodes");
    let directory = bytes
        .split(|byte| *byte == b'\n')
        .find(|line| line.get(72) == Some(&b'D'))
        .expect("encoded font file contains a directory record");
    assert_eq!(&directory[24..32], b"      -3");
}

#[test]
fn units_data_preserves_its_current_owner_link() {
    // The writer states the scale as an IGES real; compare its value, not
    // its digit layout.
    const EPS_UNIT_SCALE_REAL: f64 = 1.0e-12;
    let mut ir = CadIr::empty();
    let id = "iges:metadata:units-data#D99";
    install(
        &mut ir,
        "units_data",
        id,
        &json!({
            "source_entity":"iges:entity:directory#99", "declared_count":1,
            "units":[{"unit_type":"4c454e475448", "unit_value":"4d4d", "scale_factor":1.0}],
            "owners":["iges:entity:directory#1"],
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(id));
    assert_eq!(entities[0].property_links, [1]);
    assert_eq!(entities[1].type_code, 316);
    let parameters = std::str::from_utf8(&entities[1].parameter_body)
        .expect("units-data parameter body is ASCII");
    let scale = parameters
        .strip_prefix("1,6HLENGTH,2HMM,")
        .and_then(|rest| rest.strip_suffix(';'))
        .expect("units-data parameter body holds one unit and its scale");
    let scale: f64 = scale
        .replace('D', "e")
        .parse()
        .expect("units-data scale is an IGES real");
    assert!((scale - 1.0).abs() <= EPS_UNIT_SCALE_REAL);
}

#[test]
fn missing_units_owner_drops_the_record_with_its_identity() {
    let mut ir = CadIr::empty();
    let id = "iges:metadata:units-data#D99";
    install(
        &mut ir,
        "units_data",
        id,
        &json!({
            "source_entity":"iges:entity:directory#99", "declared_count":0,
            "units":[], "owners":["iges:entity:directory#missing"],
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(entities.len(), 1);
    assert!(!exported.regenerated.contains(id));
    // #234 drops the IGES-specific passthrough charge: the export report
    // names the omitted record through `export/native.records-omitted`,
    // computed from the dropped native disposition.
    assert!(exported.losses.is_empty());
}

#[test]
fn reemitted_node_writes_coordinates_and_subscript() {
    let mut ir = CadIr::empty();
    let id = "iges:fem:node#D11";
    install(
        &mut ir,
        "fem_entities",
        id,
        &json!({
            "source_entity":"iges:entity:directory#11", "kind":"node", "form":0,
            "node_number":5, "coordinates":[1.0,2.0,3.0],
            "definition_transformation":null, "displacement_transformation":null,
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(id));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].type_code, 134);
    assert_eq!(entities[1].subscript, 5);
    assert_eq!(
        std::str::from_utf8(&entities[1].parameter_body).expect("ASCII body"),
        "1.0000000000000000D0,2.0000000000000000D0,3.0000000000000000D0,0;",
    );
}

#[test]
fn reemitted_node_with_definition_transform_is_dropped() {
    let mut ir = CadIr::empty();
    let id = "iges:fem:node#D11";
    install(
        &mut ir,
        "fem_entities",
        id,
        &json!({
            "source_entity":"iges:entity:directory#11", "kind":"node", "form":0,
            "node_number":5, "coordinates":[1.0,2.0,3.0],
            "definition_transformation":"iges:entity:directory#21",
            "displacement_transformation":null,
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(entities.len(), 1);
    assert!(!exported.regenerated.contains(id));
}

#[test]
fn reemitted_element_links_nodes_and_keeps_missing_pointers_null() {
    let mut ir = CadIr::empty();
    let id = "iges:fem:element#D12";
    install(
        &mut ir,
        "fem_entities",
        id,
        &json!({
            "source_entity":"iges:entity:directory#12", "kind":"finite_element", "form":0,
            "element_number":7, "topology_type":3, "declared_node_count":2,
            "nodes":["iges:entity:directory#1",null], "element_type":"4E41444F",
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(id));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].type_code, 136);
    assert_eq!(entities[1].subscript, 7);
    assert_eq!(
        std::str::from_utf8(&entities[1].parameter_body).expect("ASCII body"),
        "3,2,@R0@,0,4HNADO;",
    );
}

#[test]
fn reemitted_element_with_mismatched_count_is_dropped() {
    let mut ir = CadIr::empty();
    let id = "iges:fem:element#D12";
    install(
        &mut ir,
        "fem_entities",
        id,
        &json!({
            "source_entity":"iges:entity:directory#12", "kind":"finite_element", "form":0,
            "element_number":7, "topology_type":3, "declared_node_count":2,
            "nodes":["iges:entity:directory#1"], "element_type":"4E41444F",
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert_eq!(entities.len(), 1);
    assert!(!exported.regenerated.contains(id));
}

#[test]
fn reemitted_displacement_writes_one_translation_rotation_pair_per_case() {
    let mut ir = CadIr::empty();
    let id = "iges:fem:nodal-displacement-rotation#D13";
    install(
        &mut ir,
        "fem_entities",
        id,
        &json!({
            "source_entity":"iges:entity:directory#13", "kind":"nodal_displacement_rotation", "form":0,
            "declared_case_count":1, "case_descriptions":[null], "declared_node_count":1,
            "nodes":[{
                "identifier":2, "node":"iges:entity:directory#1",
                "translations":[[0.0,0.0,0.0]], "rotations":[[1.0,0.0,0.0]], "values":[],
            }],
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(id));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].type_code, 138);
    assert_eq!(
        std::str::from_utf8(&entities[1].parameter_body).expect("ASCII body"),
        "1,0,1,2,@R0@,0,0,0,1.0000000000000000D0,0,0;",
    );
}

#[test]
fn reemitted_nodal_results_write_declared_values_per_node() {
    let mut ir = CadIr::empty();
    let id = "iges:fem:nodal-results#D14";
    install(
        &mut ir,
        "fem_entities",
        id,
        &json!({
            "source_entity":"iges:entity:directory#14", "kind":"nodal_results", "form":1,
            "analysis_case_number":9, "analysis_note":null, "subcase_number":2, "time":0.5,
            "declared_value_count":1, "expected_value_count":1, "declared_node_count":1,
            "nodes":[{
                "identifier":3, "node":null,
                "translations":[], "rotations":[], "values":[4.0],
            }],
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(id));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].type_code, 146);
    assert_eq!(entities[1].subscript, 9);
    assert_eq!(
        std::str::from_utf8(&entities[1].parameter_body).expect("ASCII body"),
        "0,2,5.0000000000000000D-1,1,1,3,0,4.0000000000000000D0;",
    );
}

#[test]
fn reemitted_element_results_write_locations_and_values_per_element() {
    let mut ir = CadIr::empty();
    let id = "iges:fem:element-results#D15";
    install(
        &mut ir,
        "fem_entities",
        id,
        &json!({
            "source_entity":"iges:entity:directory#15", "kind":"element_results", "form":4,
            "analysis_case_number":0, "analysis_note":null, "subcase_number":1, "time":0.0,
            "declared_value_count":6, "expected_value_count":6, "result_report_flag":0,
            "declared_element_count":1,
            "elements":[{
                "identifier":1, "element":"iges:entity:directory#1",
                "topology_type":3, "layers":1, "data_layer_flag":0,
                "report_locations":[2], "declared_value_count":6,
                "values":[0.0,0.0,0.0,0.0,0.0,0.0],
            }],
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(id));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].type_code, 148);
    assert_eq!(
        std::str::from_utf8(&entities[1].parameter_body).expect("ASCII body"),
        "0,1,0,6,0,1,1,@R0@,3,1,0,1,2,6,0,0,0,0,0,0;",
    );
}

#[test]
fn reemitted_load_constraint_links_node_and_case_properties() {
    let mut ir = CadIr::empty();
    let id = "iges:fem:nodal-load-constraint#D16";
    install(
        &mut ir,
        "fem_entities",
        id,
        &json!({
            "source_entity":"iges:entity:directory#16", "kind":"nodal_load_constraint", "form":0,
            "declared_case_count":1, "load_constraint_type":1,
            "node":"iges:entity:directory#1", "case_references":[null],
        }),
    );
    let mut entities = vec![owner()];
    let exported = append(&ir, &mut entities, crate::IgesVersion::V5_3);
    assert!(exported.losses.is_empty());
    assert!(exported.regenerated.contains(id));
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[1].type_code, 418);
    assert_eq!(
        std::str::from_utf8(&entities[1].parameter_body).expect("ASCII body"),
        "1,1,@R0@,0;",
    );
}

#[test]
fn reemitted_node_carries_its_subscript_into_the_directory() {
    let mut ir = CadIr::empty();
    install(
        &mut ir,
        "fem_entities",
        "iges:fem:node#D11",
        &json!({
            "source_entity":"iges:entity:directory#11", "kind":"node", "form":0,
            "node_number":5, "coordinates":[1.0,2.0,3.0],
            "definition_transformation":null, "displacement_transformation":null,
        }),
    );
    let mut entities = vec![owner()];
    append(&ir, &mut entities, crate::IgesVersion::V5_3);
    let bytes = encode_file(
        &ir,
        &entities,
        &BTreeMap::new(),
        crate::IgesVersion::V5_3,
        ir.tolerances.linear.get(),
    )
    .expect("finite-element node encodes");
    let cards: Vec<_> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| line.get(72) == Some(&b'D'))
        .collect();
    assert_eq!(cards.len(), 4);
    assert_eq!(&cards[3][64..72], b"       5");
}
