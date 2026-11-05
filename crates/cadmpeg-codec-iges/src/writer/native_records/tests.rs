// SPDX-License-Identifier: Apache-2.0

use super::append::append;
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
        Some(ir.tolerances.linear.expect("stated linear tolerance").get()),
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
        Some(ir.tolerances.linear.expect("stated linear tolerance").get()),
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
fn native_dispositions_distinguish_emitted_and_unwritable_level_records() {
    use cadmpeg_ir::codec::write::NativeDisposition;

    let mut ir = CadIr::empty();
    let emitted = "iges:presentation:definition-levels#D1";
    let omitted = "iges:presentation:definition-levels#D3";
    install(
        &mut ir,
        "definition_levels",
        emitted,
        &json!({"levels": [1, 2]}),
    );
    install(
        &mut ir,
        "definition_levels",
        omitted,
        &json!({"levels": "invalid"}),
    );
    let export = append(&ir, &mut Vec::new(), crate::IgesVersion::V5_3);
    let dispositions = crate::writer::synthesized_native_dispositions(
        &ir,
        &export.losses,
        &export.regenerated,
        &BTreeSet::new(),
    );
    assert_eq!(
        dispositions.record_disposition("iges", "definition_levels", emitted),
        NativeDisposition::Regenerated
    );
    assert_eq!(
        dispositions.record_disposition("iges", "definition_levels", omitted),
        NativeDisposition::Dropped
    );
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
        Some(ir.tolerances.linear.expect("stated linear tolerance").get()),
    )
    .expect("finite-element node encodes");
    let cards: Vec<_> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| line.get(72) == Some(&b'D'))
        .collect();
    assert_eq!(cards.len(), 4);
    assert_eq!(&cards[3][64..72], b"       5");
}

#[test]
fn reemitted_macro_definition_redecodes_to_its_defined_type() {
    use crate::test_support::test_owned::{owned_test_file_with_structures, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let file = owned_test_file_with_structures(
        &[
            OwnedTestEntity {
                entity_type: 306,
                form: 0,
                label: "MACRODEF".into(),
                status: "00000200",
                parameters: "306,MACRO,621,X,Y;LET Z=0;ENDM;".into(),
            },
            OwnedTestEntity {
                entity_type: 621,
                form: 0,
                label: "MACRO".into(),
                status: "00000000",
                parameters: "621,1.0,2.0;".into(),
            },
        ],
        &[(3, -1)],
    );
    let decoded = crate::IgesCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("macro fixture decodes");
    let native = decoded
        .ir()
        .native
        .namespace("iges")
        .expect("iges namespace decodes");
    assert_eq!(native.arenas()["macro_definitions"].len(), 1);
    assert_eq!(native.arenas()["macro_instances"].len(), 1);
    let synthesized = super::super::synthesize(decoded.ir(), crate::IgesVersion::V5_3)
        .expect("macro records synthesize");
    assert!(
        synthesized
            .losses
            .iter()
            .all(|loss| loss.code == crate::loss::IgesLossCode::NoExportableSolids.kind()),
        "only the no-geometry loss: {:?}",
        synthesized.losses
    );
    let round_trip = crate::IgesCodec
        .decode(
            &mut Cursor::new(synthesized.bytes),
            &DecodeOptions::default(),
        )
        .expect("re-emitted macros decode");
    let after = round_trip
        .ir()
        .native
        .namespace("iges")
        .expect("iges namespace re-decodes");
    assert_eq!(after.arenas()["macro_definitions"].len(), 1);
    assert_eq!(
        after.arenas()["macro_definitions"][0].field("defined_entity_type"),
        native.arenas()["macro_definitions"][0].field("defined_entity_type"),
    );
    assert_eq!(after.arenas()["macro_instances"].len(), 1);
    let instance = &after.arenas()["macro_instances"][0];
    assert_eq!(
        instance.field("entity_type"),
        native.arenas()["macro_instances"][0].field("entity_type"),
    );
    assert_eq!(
        instance
            .field("parameters")
            .as_ref()
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(2),
    );
    assert!(instance.field("macro_definition").is_some());
}

#[test]
fn reemitted_definition_levels_redecode_to_their_level_set() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let file = owned_test_file(&[OwnedTestEntity {
        entity_type: 406,
        form: 1,
        label: "LEVELS".into(),
        status: "00000000",
        parameters: "406,2,0,1;".into(),
    }]);
    let decoded = crate::IgesCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("levels fixture decodes");
    let native = decoded
        .ir()
        .native
        .namespace("iges")
        .expect("iges namespace decodes");
    assert_eq!(native.arenas()["definition_levels"].len(), 1);
    let synthesized = super::super::synthesize(decoded.ir(), crate::IgesVersion::V5_3)
        .expect("levels synthesize");
    let round_trip = crate::IgesCodec
        .decode(
            &mut Cursor::new(synthesized.bytes),
            &DecodeOptions::default(),
        )
        .expect("re-emitted levels decode");
    let after = round_trip
        .ir()
        .native
        .namespace("iges")
        .expect("iges namespace re-decodes");
    assert_eq!(after.arenas()["definition_levels"].len(), 1);
    assert_eq!(
        after.arenas()["definition_levels"][0].field("levels"),
        native.arenas()["definition_levels"][0].field("levels"),
    );
}

#[test]
fn edited_definition_levels_replace_the_source_set() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_solids_and_structure::definition_levels_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("levels fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.model
        .presentation_layers
        .iter_mut()
        .find(|layer| layer.name == "7")
        .expect("level seven")
        .name = "8".into();
    let output =
        super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("levels synthesize");
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("output decodes");
    let names = after
        .ir()
        .model
        .presentation_layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["2", "8", "11"]));
    assert!(after
        .ir()
        .model
        .presentation_layers
        .iter()
        .all(|layer| !layer.items.is_empty()));
}

#[test]
fn neutral_multiple_levels_write_a_definition_set() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
    use std::io::Cursor;

    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_curves_and_surfaces::line_file(0)),
            &DecodeOptions::default(),
        )
        .expect("line fixture decodes");
    let mut ir = decoded.ir().clone();
    let curve = ir.model.curves[0].id.clone();
    ir.model.presentation_layers.clear();
    for number in [2, 4] {
        ir.model.presentation_layers.push(PresentationLayer {
            id: cadmpeg_ir::ids::LayerId::mint(format!("test:presentation:layer#{number}"))
                .unwrap(),
            native_ref: None,
            name: number.to_string(),
            parent: None,
            description: None,
            visible: None,
            items: vec![PresentationItem::Curve {
                curve: curve.clone(),
            }],
        });
    }
    let output = super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("line synthesizes");
    assert_eq!(
        output.coverage.presentation_layers,
        cadmpeg_ir::codec::write::ArenaDisposition::Written
    );
    assert!(!output
        .losses
        .iter()
        .any(|loss| loss.message.contains("presentation layers")));
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("output decodes");
    let names = after
        .ir()
        .model
        .presentation_layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["2", "4"]));
    assert!(after
        .ir()
        .model
        .presentation_layers
        .iter()
        .all(|layer| !layer.items.is_empty()));
    ir.model.presentation_layers[1].name = "not-a-level-number".into();
    let omitted_id = ir.model.presentation_layers[1].id.to_string();
    let output = super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("line synthesizes");
    assert_eq!(
        output.coverage.presentation_layers,
        cadmpeg_ir::codec::write::ArenaDisposition::Reported
    );
    assert!(output
        .losses
        .iter()
        .any(|loss| loss.message.contains(&omitted_id)));
}

#[test]
fn edited_neutral_box_dimensions_and_placement_replace_native_values() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::{
        Dimension, FeatureDefinition, FeatureOperation, FeatureRigidPlacement, PrimitiveSolid,
        PrimitiveSolidKind,
    };
    use cadmpeg_ir::scalar::Length;
    use std::io::Cursor;

    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_solids_and_structure::primitive_solids_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("primitive fixture decodes");
    let mut ir = decoded.ir().clone();
    let feature = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.native_ref.as_deref() == Some("iges:solid:primitive#D1"))
        .expect("box feature");
    let desired = FeatureRigidPlacement::new(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 8.0],
            [0.0, 1.0, 0.0, 9.0],
            [0.0, 0.0, 1.0, 10.0],
        ])
        .unwrap(),
    )
    .unwrap();
    feature.evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Primitive {
            solid, placement, ..
        }) = definition
        else {
            panic!("box operation")
        };
        let PrimitiveSolidKind::Box { width, height, .. } = solid.kind() else {
            panic!("box")
        };
        *solid = PrimitiveSolid::new(PrimitiveSolidKind::Box {
            length: Dimension::Literal(Length::new(7.0).unwrap()),
            width: width.clone(),
            height: height.clone(),
        })
        .unwrap();
        *placement = Some(desired);
    });
    let output =
        super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("primitive synthesis");
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("output decodes");
    let box_feature = after.ir().model.features.iter().find(|feature| {
        matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Primitive { solid, .. }) if matches!(solid.kind(), PrimitiveSolidKind::Box { .. }))
    }).expect("redecoded box");
    let FeatureDefinition::Operation(FeatureOperation::Primitive {
        solid, placement, ..
    }) = box_feature.evaluation.definition()
    else {
        unreachable!()
    };
    let PrimitiveSolidKind::Box { length, .. } = solid.kind() else {
        unreachable!()
    };
    assert_eq!(length.literal().unwrap().get(), 7.0);
    assert_eq!(*placement, Some(desired));
}

#[test]
fn edited_primitive_dimensions_round_trip_from_neutral_features() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::{
        Dimension, FeatureDefinition, FeatureOperation, PrimitiveSolid, PrimitiveSolidKind,
        RadialDimension,
    };
    use cadmpeg_ir::scalar::Length;
    use std::io::Cursor;

    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_solids_and_structure::primitive_solids_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("primitive fixture decodes");
    let mut ir = decoded.ir().clone();
    let mut expected = BTreeMap::new();
    let changed = |value: &mut Dimension<Length>| {
        *value = Dimension::Literal(Length::new(value.literal().unwrap().get() + 1.0).unwrap());
    };
    for feature in &mut ir.model.features {
        let name = feature.name.clone().expect("source label");
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Primitive { solid, .. }) =
                definition
            else {
                return;
            };
            let mut kind = solid.kind().clone();
            match &mut kind {
                PrimitiveSolidKind::Box { length, .. } => changed(length),
                PrimitiveSolidKind::Wedge { zmax, z2max, .. } => {
                    changed(zmax);
                    changed(z2max);
                }
                PrimitiveSolidKind::Cylinder { height, .. }
                | PrimitiveSolidKind::Cone { height, .. } => changed(height),
                PrimitiveSolidKind::Sphere { radius, .. } => {
                    let RadialDimension::Radius(value) = radius else {
                        panic!("radius")
                    };
                    changed(value);
                }
                PrimitiveSolidKind::Torus { major_radius, .. } => {
                    let RadialDimension::Radius(value) = major_radius else {
                        panic!("radius")
                    };
                    changed(value);
                }
                PrimitiveSolidKind::Ellipsoid { x_radius, .. } => {
                    let RadialDimension::Radius(value) = x_radius else {
                        panic!("radius")
                    };
                    changed(value);
                }
                PrimitiveSolidKind::Prism { .. } => panic!("unexpected prism"),
            }
            expected.insert(name, kind.clone());
            *solid = PrimitiveSolid::new(kind).expect("changed dimensions admit");
        });
    }
    let output =
        super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("primitive synthesis");
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("output decodes");
    for feature in &after.ir().model.features {
        let FeatureDefinition::Operation(FeatureOperation::Primitive { solid, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let name = feature.name.as_ref().expect("primitive label");
        assert_eq!(Some(solid.kind()), expected.remove(name).as_ref(), "{name}");
    }
    assert!(expected.is_empty(), "missing primitives: {expected:?}");
}

#[test]
fn primitive_feature_order_follows_current_ordinals() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_solids_and_structure::primitive_solids_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("primitive fixture decodes");
    let mut ir = decoded.ir().clone();
    let first = ir
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("BLOCK"))
        .unwrap();
    let second = ir
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("DEFAULT"))
        .unwrap();
    let ordinal = ir.model.features[first].ordinal;
    ir.model.features[first].ordinal = ir.model.features[second].ordinal;
    ir.model.features[second].ordinal = ordinal;
    let output =
        super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("primitive synthesis");
    assert_eq!(
        output.coverage.features,
        cadmpeg_ir::codec::write::ArenaDisposition::Written
    );
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("output decodes");
    let ordinal = |name: &str| {
        after
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
            .ordinal
    };
    assert!(ordinal("DEFAULT") < ordinal("BLOCK"));
}

#[test]
fn source_less_primitive_feature_synthesizes_without_native_records() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PrimitiveSolidKind};
    use std::io::Cursor;

    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_solids_and_structure::primitive_solids_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("primitive fixture decodes");
    let mut feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("BLOCK"))
        .unwrap()
        .clone();
    feature.native_ref = None;
    feature.ordinal = 0;
    let mut ir = CadIr::empty();
    ir.model.features.push(feature);
    let output = super::super::synthesize(&ir, crate::IgesVersion::V5_3)
        .expect("source-less primitive synthesis");
    assert_eq!(
        output.coverage.features,
        cadmpeg_ir::codec::write::ArenaDisposition::Written
    );
    assert!(!output
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::NoExportableSolids.kind()));
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("output decodes");
    assert!(after.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Primitive { solid, .. })
            if matches!(solid.kind(), PrimitiveSolidKind::Box { .. })
    )));
}

#[test]
fn mixed_source_less_and_linked_primitives_follow_ordinals() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(
                crate::test_support::test_solids_and_structure::primitive_solids_file(),
            ),
            &DecodeOptions::default(),
        )
        .expect("primitive fixture decodes");
    let mut ir = decoded.ir().clone();
    let mut feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("BLOCK"))
        .unwrap()
        .clone();
    feature.id = cadmpeg_ir::features::FeatureId::mint("test:design:feature#new").unwrap();
    feature.native_ref = None;
    feature.name = Some("NEW".into());
    feature.ordinal = u64::try_from(ir.model.features.len()).unwrap();
    ir.model.features.push(feature);
    let output =
        super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("primitive synthesis");
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("output decodes");
    let new = after
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("NEW"))
        .unwrap();
    assert_eq!(
        new.ordinal,
        u64::try_from(ir.model.features.len() - 1).unwrap()
    );
}

/// Decode one committed golden fixture, regenerate it, and require the named
/// native arenas to agree field for field after normalizing record identity.
fn assert_fixture_round_trips(fixture: &str, arenas: &[&str]) {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_test_support::golden::Harness;
    use std::io::Cursor;
    let harness = Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == fixture)
        .expect("the round-trip fixture is committed");
    let decoded = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the round-trip fixture decodes");
    let before = decoded
        .ir()
        .native
        .namespace("iges")
        .expect("iges namespace decodes");
    let synthesized = super::super::synthesize(decoded.ir(), crate::IgesVersion::V5_3)
        .expect("the round-trip fixture synthesizes");
    let round_trip = crate::IgesCodec
        .decode(
            &mut Cursor::new(synthesized.bytes),
            &DecodeOptions::default(),
        )
        .expect("the regenerated fixture decodes");
    let after = round_trip
        .ir()
        .native
        .namespace("iges")
        .expect("iges namespace re-decodes");
    for arena in arenas {
        let mut before_rows = before.arenas()[*arena].clone();
        let mut after_rows = after.arenas().get(*arena).cloned().unwrap_or_default();
        assert_eq!(
            before_rows.len(),
            after_rows.len(),
            "{fixture}: {arena} loses records"
        );
        for rows in [&mut before_rows, &mut after_rows] {
            for row in rows.iter_mut() {
                let mut value = serde_json::to_value(&*row).expect("record serializes");
                normalize_record(&mut value);
                *row = serde_json::from_value(value).expect("record normalizes");
            }
        }
        // Arena row order is a storage artifact: decode stable-sorts rows
        // by identity (`arena_from`), and re-emission renumbers every
        // Directory sequence, so row positions permute even when the record
        // population is identical. Compare the populations as multisets.
        let mut before_serialized: Vec<String> = before_rows
            .iter()
            .map(|row| serde_json::to_string(row).expect("record serializes"))
            .collect();
        let mut after_serialized: Vec<String> = after_rows
            .iter()
            .map(|row| serde_json::to_string(row).expect("record serializes"))
            .collect();
        before_serialized.sort();
        after_serialized.sort();
        assert_eq!(
            before_serialized, after_serialized,
            "{fixture}: {arena} drifts"
        );
    }
}

/// Replace every Directory sequence in record identities and references with
/// a stable placeholder, so reordered re-emission still compares.
///
/// Boolean operand `raw` values are source-sequenced pointers the decoder
/// retains alongside the resolved entity; no re-sequenced re-emission can
/// reproduce them (reported to 222-iges-e with the record named).
fn normalize_record(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            let mut normalized = String::with_capacity(text.len());
            let mut rest = text.as_str();
            while let Some(hash) = rest.find('#') {
                let (head, tail) = rest.split_at(hash);
                normalized.push_str(head);
                if let Some(numbered) = tail.strip_prefix("#D") {
                    let digits: String = numbered.chars().take_while(|c| c.is_numeric()).collect();
                    if !digits.is_empty() {
                        normalized.push_str("#DN");
                        rest = &numbered[digits.len()..];
                        continue;
                    }
                }
                if let Some(numbered) = tail.strip_prefix('#') {
                    let digits: String = numbered.chars().take_while(|c| c.is_numeric()).collect();
                    if !digits.is_empty() {
                        normalized.push_str("#N");
                        rest = &numbered[digits.len()..];
                        continue;
                    }
                }
                normalized.push('#');
                rest = &tail[1..];
            }
            normalized.push_str(rest);
            *text = normalized;
        }
        serde_json::Value::Array(items) => {
            for item in items {
                normalize_record(item);
            }
        }
        serde_json::Value::Object(fields) => {
            let is_operand = fields
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind == "operand");
            for (key, value) in fields.iter_mut() {
                if is_operand && key == "raw" {
                    *value = serde_json::Value::Null;
                    continue;
                }
                normalize_record(value);
            }
        }
        _ => {}
    }
}

#[test]
fn reemitted_products_redecode_to_their_typed_records() {
    assert_fixture_round_trips(
        "solid_assembly",
        &["solid_assemblies", "primitive_solids", "transformations"],
    );
    assert_fixture_round_trips(
        "explicit_tetrahedron_solid_with_boolean",
        &[
            "solid_assemblies",
            "solid_instances",
            "boolean_trees",
            "primitive_solids",
        ],
    );
    assert_fixture_round_trips("solid_instance", &["solid_instances", "primitive_solids"]);
    assert_fixture_round_trips(
        "nested_subfigure",
        &["subfigure_definitions", "subfigure_instances"],
    );
    assert_fixture_round_trips(
        "connected_network_subfigure",
        &["network_definitions", "network_instances", "connect_points"],
    );
    assert_fixture_round_trips(
        "procedural_and_boolean_solids",
        &[
            "boolean_trees",
            "selected_components",
            "primitive_solids",
            "procedural_solids",
        ],
    );
    assert_fixture_round_trips(
        "patterned_instance",
        &["rectangular_arrays", "circular_arrays"],
    );
    assert_fixture_round_trips("external_reference_forms", &["external_references"]);
    assert_fixture_round_trips("nested_transformed_point", &["transformations"]);
}

#[test]
fn reemitted_drawings_redecode_to_their_typed_records() {
    assert_fixture_round_trips("drawing_with_properties", &["drawings", "views"]);
    assert_fixture_round_trips("view_forms", &["views"]);
    assert_fixture_round_trips("view_visibility_forms", &["view_visibility", "views"]);
    assert_fixture_round_trips("dimension_forms", &["annotations"]);
    assert_fixture_round_trips("leader_forms", &["annotations"]);
}

#[test]
fn boolean_feature_order_follows_current_ordinals() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let entities = [
        (158, 0, "SPHERE1", "158,1,0,0,0;"),
        (158, 0, "SPHERE2", "158,1,3,0,0;"),
        (180, 0, "FIRST", "180,3,-1,-3,1;"),
        (180, 0, "SECOND", "180,3,-1,-3,1;"),
    ]
    .into_iter()
    .map(|(entity_type, form, label, parameters)| OwnedTestEntity {
        entity_type,
        form,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    })
    .collect::<Vec<_>>();
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .expect("Boolean fixtures decode");
    let mut ir = decoded.ir().clone();
    let first = ir
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("FIRST"))
        .expect("first Boolean");
    let second = ir
        .model
        .features
        .iter()
        .position(|feature| feature.name.as_deref() == Some("SECOND"))
        .expect("second Boolean");
    let ordinal = ir.model.features[first].ordinal;
    ir.model.features[first].ordinal = ir.model.features[second].ordinal;
    ir.model.features[second].ordinal = ordinal;
    let output = super::super::synthesize(&ir, crate::IgesVersion::V5_3)
        .expect("Boolean features synthesize");
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("Boolean features re-decode");
    let ordinal = |name: &str| {
        after
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("written Boolean feature")
            .ordinal
    };
    assert!(ordinal("SECOND") < ordinal("FIRST"));
}

#[test]
fn edited_rectangular_pattern_grid_reaches_output() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::patterns::{PatternKind, PatternTransform};
    use cadmpeg_ir::features::{Dimension, FeatureDefinition, FeatureOperation};
    use std::io::Cursor;
    let entities = [
        (116, "BASE", "116,0,0,0,0;"),
        (412, "RECT", "412,1,1,0,0,0,2,1,4,2,0.0,0,0;"),
    ]
    .into_iter()
    .map(|(entity_type, label, parameters)| OwnedTestEntity {
        entity_type,
        form: 0,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    })
    .collect::<Vec<_>>();
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .expect("rectangular array decodes");
    let mut ir = decoded.ir().clone();
    let feature = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("RECT"))
        .expect("rectangular feature");
    let mut definition = feature.evaluation.definition().clone();
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = &mut definition
    else {
        panic!("pattern")
    };
    let mut transform = pattern.definition().clone();
    let PatternTransform::Linear { count, .. } = &mut transform else {
        panic!("linear pattern")
    };
    *count = Dimension::Literal(3);
    *pattern = PatternKind::new(transform).expect("edited grid remains valid");
    feature.evaluation.set_definition(definition);
    let output =
        super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("edited array synthesizes");
    let export_losses = output.losses.clone();
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("array re-decodes");
    let feature = after
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("RECT"))
        .unwrap_or_else(|| {
            panic!(
                "written array feature; export: {export_losses:?}; decode: {:?}; features: {:?}",
                after.report().losses,
                after.ir().model.features
            )
        });
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
        feature.evaluation.definition()
    else {
        panic!("written pattern")
    };
    let PatternTransform::Linear { count, .. } = pattern.definition() else {
        panic!("written linear pattern")
    };
    assert_eq!(*count, Dimension::Literal(3));
    let native = &after.ir().native.namespace("iges").unwrap().arenas()["rectangular_arrays"][0];
    assert_eq!(
        native
            .field("do_dont_flag")
            .and_then(|value| value.as_i64()),
        Some(0)
    );
}

#[test]
fn unsupported_rectangular_omission_mask_is_reported() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::patterns::{PatternKind, PatternTransform};
    use cadmpeg_ir::features::{Dimension, FeatureDefinition, FeatureOperation};
    use std::io::Cursor;
    let entities = [
        (116, "BASE", "116,0,0,0,0;"),
        (412, "RECT", "412,1,1,0,0,0,2,1,4,2,0.0,1,1,2;"),
    ]
    .into_iter()
    .map(|(entity_type, label, parameters)| OwnedTestEntity {
        entity_type,
        form: 0,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    })
    .collect::<Vec<_>>();
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let feature = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("RECT"))
        .unwrap();
    let feature_id = feature.id.to_string();
    let mut definition = feature.evaluation.definition().clone();
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = &mut definition
    else {
        panic!("pattern")
    };
    let mut transform = pattern.definition().clone();
    let PatternTransform::Linear { count, .. } = &mut transform else {
        panic!("linear pattern")
    };
    *count = Dimension::Literal(3);
    *pattern = PatternKind::new(transform).unwrap();
    feature.evaluation.set_definition(definition);
    let output = super::super::synthesize(&ir, crate::IgesVersion::V5_3).unwrap();
    assert!(output
        .losses
        .iter()
        .any(|loss| loss.message.contains(&feature_id)));
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .unwrap();
    assert!(after.report().losses.iter().all(|loss| !loss
        .message
        .contains("array base, dimensions, selection mask")));
}

#[test]
fn rectangular_do_mask_keeps_its_current_grid_positions() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let entities = [
        (116, "BASE", "116,0,0,0,0;"),
        (412, "RECT", "412,1,1,0,0,0,4,1,4,2,0.0,2,0,1,3;"),
    ]
    .into_iter()
    .map(|(entity_type, label, parameters)| OwnedTestEntity {
        entity_type,
        form: 0,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    })
    .collect::<Vec<_>>();
    let decoded = crate::IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .expect("DO mask decodes");
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.name.as_deref() == Some("RECT")));
    let output =
        super::super::synthesize(decoded.ir(), crate::IgesVersion::V5_3).expect("DO mask writes");
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .expect("DO mask re-decodes");
    let native = &after.ir().native.namespace("iges").unwrap().arenas()["rectangular_arrays"][0];
    assert_eq!(
        native
            .field("do_dont_flag")
            .and_then(|value| value.as_i64()),
        Some(0)
    );
    assert_eq!(native.field("positions"), Some(json!([1, 3])));
    assert!(after
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.name.as_deref() == Some("RECT")));
}
