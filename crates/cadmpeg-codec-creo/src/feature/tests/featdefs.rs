// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId};

use crate::container::{self};
use crate::test_support::*;
use crate::CreoCodec;

#[test]
fn scan_decodes_featdefs_records_and_parameter_frames() {
    let mut payload = b"feat_defs_40\0local_sys\0\xf9\x04\x03".to_vec();
    for _ in 0..3 {
        payload.extend_from_slice(&[0x0f, 0x18, 0xe5]);
    }
    payload.extend_from_slice(b"\xe0\x21transf\0\xf9\x04\x03");
    payload.extend([0xe4; 12]);
    payload.extend_from_slice(b"feat_defs_81\0opaque");
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.features.definitions.len(), 2);
    assert_eq!(scan.features.definitions[0].id, 40);
    assert_eq!(scan.features.definitions[0].parameter_frames.len(), 2);
    assert_eq!(
        scan.features.definitions[0].parameter_frames[0].kind,
        crate::feature::FeatureParameterFrameKind::LocalSystem
    );
    assert_eq!(
        scan.features.definitions[0].parameter_frames[0].decoded_values,
        Some(vec![
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0
        ])
    );
    assert_eq!(
        scan.features.definitions[0].parameter_frames[1].kind,
        crate::feature::FeatureParameterFrameKind::Transform
    );
    assert_eq!(
        scan.features.definitions[0].parameter_frames[1].decoded_values,
        Some(vec![1.0; 12])
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let definitions = &result.ir().native.namespace("creo").unwrap().arenas["feature_definitions"];
    let definition_fields = definitions[0].fields();
    let frames = definition_fields["parameter_frames"]
        .as_array()
        .expect("parameter frames");
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["kind"], "local_system");
    assert_eq!(frames[0]["decoded_values"].as_array().unwrap().len(), 12);
    assert_eq!(frames[0]["decoded_values"][0], 0.0);
    assert_eq!(frames[0]["decoded_values"][2], 1.0);
    assert_eq!(frames[1]["kind"], "transform");
    assert_eq!(frames[1]["decoded_values"].as_array().unwrap().len(), 12);
    assert_eq!(frames[1]["decoded_values"][0], 1.0);
}

#[test]
fn scan_decodes_rank_two_featdefs_local_system() {
    let mut payload = b"feat_defs_40\0local_sys\0\xf9\x04\x03\x0f\x18\xe5\x18\xe5".to_vec();
    payload.extend_from_slice(&[0x2d, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x2d, 0x10, 0, 0, 0, 0, 0, 0]);
    payload.push(0x18);
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data);

    assert_eq!(
        scan.features.definitions[0].parameter_frames[0].decoded_values,
        Some(vec![
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -3.0, -4.0, 0.0
        ])
    );
}

#[test]
fn scan_decodes_featdefs_feature_local_outlines() {
    let mut payload = b"feat_defs_40\0\xe0\x00feat_outl_info\0outline\0\xf9\x02\x03".to_vec();
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend([0x0f; 5]);
    payload.extend_from_slice(b"\xe0\x00post_roll_back\0\xe3\xf7\x01\xf5\x96\x92\x02");
    payload.extend([0xe4; 6]);
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let outlines = &scan.features.definitions[0].outlines;
    assert_eq!(outlines.len(), 2);
    assert_eq!(outlines[0].phase, crate::feature::OutlinePhase::PreRollback);
    assert_eq!(
        outlines[0].local_values,
        vec![
            Some(3.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0)
        ]
    );
    assert_eq!(
        outlines[0].local_value_bodies[0],
        [0x46, 0x08, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(outlines[0].local_value_bodies[1..], vec![vec![0x0f]; 5]);
    assert_eq!(
        outlines[1].phase,
        crate::feature::OutlinePhase::PostRollback
    );
    assert_eq!(outlines[1].local_values, vec![Some(1.0); 6]);
    assert_eq!(outlines[1].local_value_bodies, vec![vec![0xe4]; 6]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let definitions = &result.ir().native.namespace("creo").unwrap().arenas["feature_definitions"];
    let definition_fields = definitions[0].fields();
    let outlines = definition_fields["outlines"].as_array().expect("outlines");
    assert_eq!(outlines.len(), 2);
    assert_eq!(outlines[0]["phase"], "pre_rollback");
    assert_eq!(outlines[0]["local_values"].as_array().unwrap().len(), 6);
    assert_eq!(outlines[0]["local_values"][0], 3.0);
    assert_eq!(
        outlines[0]["local_value_bodies"][0]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(outlines[0]["local_value_bodies"][0][0], 0x46);
    assert_eq!(outlines[1]["phase"], "post_rollback");
    assert_eq!(outlines[1]["local_values"].as_array().unwrap().len(), 6);
    assert_eq!(outlines[1]["local_values"][0], 1.0);
    assert_eq!(outlines[1]["local_value_bodies"][0][0], 0xe4);
}

#[test]
fn scan_stops_feature_local_outlines_at_named_records() {
    let mut payload = b"feat_defs_40\0\xe0\x00feat_outl_info\0outline\0\xf9\x02\x03".to_vec();
    payload.extend_from_slice(&[0x0f, 0xe4]);
    payload.extend_from_slice(b"\xe0\x00post_roll_back\0\xe3\xf7\x01\xf5\x96\x92\x02");
    payload.extend_from_slice(&[0xe4, 0x0f]);
    payload.extend_from_slice(b"\xe0\x00post_regen\0\xe3\xf7\x01\xf5\x96\x92\x02");
    payload.extend([0x0f; 6]);
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let outlines = &scan.features.definitions[0].outlines;
    assert_eq!(outlines.len(), 3);
    assert_eq!(outlines[0].phase, crate::feature::OutlinePhase::PreRollback);
    assert_eq!(
        outlines[0].local_values,
        vec![Some(0.0), Some(1.0), None, None, None, None]
    );
    assert_eq!(
        outlines[0].local_value_bodies,
        vec![vec![0x0f], vec![0xe4], vec![], vec![], vec![], vec![]]
    );
    assert_eq!(
        outlines[1].phase,
        crate::feature::OutlinePhase::PostRollback
    );
    assert_eq!(
        outlines[1].local_values,
        vec![Some(1.0), Some(0.0), None, None, None, None]
    );
    assert_eq!(
        outlines[1].local_value_bodies,
        vec![vec![0xe4], vec![0x0f], vec![], vec![], vec![], vec![]]
    );
    assert_eq!(outlines[2].phase, crate::feature::OutlinePhase::PostRegen);
    assert_eq!(outlines[2].local_values, vec![Some(0.0); 6]);
    assert_eq!(outlines[2].local_value_bodies, vec![vec![0x0f]; 6]);
}

#[test]
fn scan_decodes_featdefs_var_arr_section_points() {
    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[1, 7, 0xe4, 0x0f, 1, 0, 3, 0xe2]);
    payload.extend_from_slice(&[2, 7, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 1, 0, 4, 0xe2]);
    payload.extend_from_slice(&[1, 8, 0xe4, 0x0f, 1, 0, 5, 0xe2]);
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let variables = scan.features.definitions[0]
        .variables
        .as_ref()
        .expect("var_arr");
    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(1));
    assert_eq!(variables.rows.len(), 2);
    assert_eq!(variables.rows[0].value, Some(1.0));
    assert_eq!(variables.rows[0].value_body, [0xe4]);
    assert_eq!(variables.rows[0].guess_body, [0x0f]);
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(0));
    assert_eq!(variables.rows[0].uvar_id, Some(3));
    assert_eq!(variables.rows[1].value, Some(3.0));
    assert_eq!(variables.rows[1].value_body, [0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    assert_eq!(variables.rows[1].guess_body, [0x0f]);
    assert_eq!(variables.rows[1].known, Some(1));
    assert_eq!(variables.rows[1].homogeneity, Some(0));
    assert_eq!(variables.rows[1].uvar_id, Some(4));
    assert_eq!(variables.points.len(), 1);
    assert_eq!(variables.points[0].point_id, 7);
    assert_eq!(variables.points[0].u, Some(1.0));
    assert_eq!(variables.points[0].v, Some(3.0));
}

#[test]
fn scan_decodes_featdefs_var_arr_named_prototype_row() {
    let payload = b"feat_defs_40\0var_arr\0\xf8\x01\xf7\x01\xfb\xe2\
        \xe0\x05type\0\x01\xe0\x08key\0\x07\xe0\x02value\0\xe4\
        \xe0\x02guess\0\x0f\xe0\x06known\0\x01\
        \xe0\x0chomogeneity\0\x02\xe0\x08uvar_id\0\x03\xf1\xf7\x01\xe2"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let variables = scan.features.definitions[0]
        .variables
        .as_ref()
        .expect("var_arr");
    assert_eq!(variables.rows.len(), 1);
    assert_eq!(variables.rows[0].variable_type, 1);
    assert_eq!(variables.rows[0].key, 7);
    assert_eq!(variables.rows[0].value, Some(1.0));
    assert_eq!(variables.rows[0].value_body, [0xe4]);
    assert_eq!(variables.rows[0].guess, Some(0.0));
    assert_eq!(variables.rows[0].guess_body, [0x0f]);
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(2));
    assert_eq!(variables.rows[0].uvar_id, Some(3));
}

#[test]
fn scan_classifies_named_var_arr_guess_sentinel() {
    let payload = b"feat_defs_40\0var_arr\0\xf8\x01\xf7\x01\xfb\xe2\
        \xe0\x05type\0\x01\xe0\x08key\0\x07\xe0\x02value\0\xe4\
        \xe0\x02guess\0\xed\x11\x12\x13\x14\x15\x16\x17\x18\
        \xe0\x06known\0\x01\xe0\x0chomogeneity\0\x02\
        \xe0\x08uvar_id\0\x03\xf1\xf7\x01\xe2"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));

    let variables = scan.features.definitions[0]
        .variables
        .as_ref()
        .expect("var_arr");
    let [row] = variables.rows.as_slice() else {
        panic!("one named variable row");
    };
    assert_eq!(
        row.guess_body,
        [0xed, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18]
    );
    assert!(row.guess_dimension_driven);
}

#[test]
fn scan_decodes_featdefs_segtab_line_and_arc_rows() {
    let mut payload =
        b"feat_defs_40\0segtab_ptr\0\xf8\x05\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[3, 0, 0, 0, 8, 9, 10, 1, 0, 11, 12, 43, 0xe2, 0xe3]);
    payload.extend_from_slice(&[2, 0, 0, 0, 9, 10, 0xf6, 0, 0, 0xf6, 0xf6, 0x80, 0xe3, 0xe2]);
    payload.extend_from_slice(&[0xe3, 0xe2, 0, 0xf6, 0xe2, 0xc0, 0x80]);
    payload.extend_from_slice(&[2, 0, 0, 0, 11, 12, 0xf6, 0, 0, 0xf6, 0xf6, 0, 0xe2]);
    payload.extend_from_slice(&[0xe3, 0xe2, 0, 0xf6, 0xe2]);
    payload.extend_from_slice(&[5, 1, 0, 0xe4, 13, 0xe4, 0xf6, 0, 2, 0xf6, 0xf6, 4, 0xe2]);
    payload.extend_from_slice(b"dimtab_ptr\0");
    payload.extend_from_slice(&[2, 0, 0, 0, 11, 12, 0xf6, 0, 0, 0xf6, 0xf6, 44, 0xe2]);
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let segments = scan.features.definitions[0]
        .segments
        .as_ref()
        .expect("segtab");
    assert_eq!(segments.declared_count, 5);
    assert_eq!(segments.rows.len(), 5);
    assert_eq!(
        segments.rows[0].kind,
        crate::feature::FeatureSegmentKind::Line
    );
    assert_eq!(segments.rows[0].point_ids, [7, 8]);
    assert_eq!(segments.rows[0].center_id, None);
    assert_eq!(segments.rows[0].external_id, 42);
    assert_eq!(
        segments.rows[0].body,
        [2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2]
    );
    assert_eq!(
        segments.rows[1].kind,
        crate::feature::FeatureSegmentKind::Arc
    );
    assert_eq!(segments.rows[1].center_id, Some(10));
    assert_eq!(segments.rows[2].external_id, 227);
    assert_eq!(segments.rows[3].point_ids, [11, 12]);
    assert_eq!(segments.rows[3].external_id, 0);
    assert_eq!(
        segments.rows[4].kind,
        crate::feature::FeatureSegmentKind::Point
    );
    assert_eq!(segments.rows[4].point_ids, [13, 13]);
    assert_eq!(segments.rows[4].external_id, 4);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let native_sketch = &result.ir().native.namespace("creo").unwrap().arenas["sketches"][0];
    assert_eq!(
        native_sketch.fields()["segments"][0]["body"]
            .as_array()
            .expect("segment body")
            .iter()
            .map(|byte| byte.as_u64().expect("byte"))
            .collect::<Vec<_>>(),
        [2, 0, 0, 0, 7, 8, 246, 0, 0, 246, 246, 42, 226]
    );
    let sketch = result
        .ir()
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id.0 == "creo:model:sketch#40")
        .expect("neutral unplaced sketch");
    assert_eq!(
        sketch.placement,
        cadmpeg_ir::sketches::SketchPlacement::Unresolved
    );
    assert_eq!(
        result
            .ir()
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .count(),
        5
    );
    let constraints = result
        .ir()
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == sketch.id)
        .collect::<Vec<_>>();
    assert_eq!(constraints.len(), 7);
    for (field, ordinal) in [("radius", 11), ("radius2", 12)] {
        let constraint = constraints
            .iter()
            .find(|constraint| {
                constraint.id.0 == format!("creo:featdefs:sketch_constraint#40:segtab-{field}:43")
            })
            .expect("segment radius binding");
        let SketchConstraintDefinition::Native {
            native_kind,
            native_properties,
            entities,
            operands,
            ..
        } = &constraint.definition
        else {
            panic!("untyped segment radius binding must remain native");
        };
        assert_eq!(native_kind, &format!("creo:segtab:{field}"));
        assert_eq!(native_properties["dimension_ordinal"], ordinal.to_string());
        assert_eq!(
            entities,
            &[SketchEntityId("creo:featdefs:sketch_entity#40:43".into())]
        );
        assert_eq!(operands[1].native_field.as_deref(), Some(field));
        assert_eq!(operands[1].object_index, ordinal);
    }
    let point_verhor = constraints
        .iter()
        .find(|constraint| constraint.id.0 == "creo:featdefs:sketch_constraint#40:verhor:4")
        .expect("point verhor constraint");
    let SketchConstraintDefinition::Native {
        native_kind,
        native_properties,
        entities,
        operands,
        ..
    } = &point_verhor.definition
    else {
        panic!("point verhor must remain native");
    };
    assert_eq!(native_kind, "creo:segtab:verhor");
    assert_eq!(native_properties["verhor"], "2");
    assert_eq!(
        entities,
        &[SketchEntityId("creo:featdefs:sketch_entity#40:4".into())]
    );
    assert_eq!(operands[0].native_field.as_deref(), Some("ext_id"));
    assert_eq!(operands[0].object_index, 4);
}

#[test]
fn scan_retains_typed_special_segment_rows_in_native_sketch_records() {
    let mut payload =
        b"feat_defs_40\0segtab_ptr\0\xf8\x06\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    payload.extend_from_slice(&[10, 0, 0, 0, 0xf6, 1, 2, 0, 0, 1, 0xf6, 20, 0xe2, 0xe3]);
    payload.extend_from_slice(&[1, 0, 0, 0, 0xf6, 1, 3, 0, 0, 0xf6, 0xf6, 21, 0xe2, 0xe3]);
    payload.extend_from_slice(&[47, 0, 0, 0, 0xf6, 1, 2, 0, 0, 1, 0xf6, 22, 0xe2, 0xe3]);
    payload.extend_from_slice(&[25, 0, 1, 0, 10, 11, 0xf6, 0, 0, 0xf6, 0xf6, 24, 0xe2, 0xe3]);
    payload.extend_from_slice(&[12, 0, 0, 0, 2, 3, 0xf6, 1, 0, 2, 0xf6, 25, 0xe2, 0xe3]);
    payload.extend_from_slice(&[47, 0, 0, 0, 0xf6, 1, 0, 0, 0, 1, 0xf6, 23, 0xe2]);
    payload.extend_from_slice(b"dimtab_ptr\0");
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());
    let segments = scan.features.definitions[0]
        .segments
        .as_ref()
        .expect("segtab");

    assert!(segments.is_complete());
    assert_eq!(segments.circle_rows.len(), 1);
    assert_eq!(segments.point_rows.len(), 1);
    assert_eq!(
        segments
            .centered_line_rows
            .iter()
            .map(|row| (row.external_id, row.center_id))
            .collect::<Vec<_>>(),
        vec![(22, 2), (23, 0)]
    );
    assert_eq!(segments.reference_line_rows.len(), 1);
    assert_eq!(segments.bounded_curve_rows.len(), 1);
    assert!(segments.opaque_rows.is_empty());

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let sketch = &result.ir().native.namespace("creo").unwrap().arenas["sketches"][0];
    assert_eq!(sketch.fields()["circle_segments"][0]["external_id"], 20);
    assert_eq!(sketch.fields()["circle_segments"][0]["center_id"], 2);
    assert_eq!(
        sketch.fields()["circle_segments"][0]["radius_dimension_id"],
        1
    );
    assert_eq!(sketch.fields()["point_segments"][0]["external_id"], 21);
    assert_eq!(sketch.fields()["point_segments"][0]["point_id"], 3);
    assert_eq!(
        sketch.fields()["centered_line_segments"][0]["external_id"],
        22
    );
    assert_eq!(sketch.fields()["centered_line_segments"][0]["center_id"], 2);
    assert_eq!(
        sketch.fields()["centered_line_segments"][1]["external_id"],
        23
    );
    assert_eq!(sketch.fields()["centered_line_segments"][1]["center_id"], 0);
    assert_eq!(
        sketch.fields()["reference_line_segments"][0]["external_id"],
        24
    );
    assert_eq!(
        sketch.fields()["reference_line_segments"][0]["point_ids"][0],
        10
    );
    assert_eq!(
        sketch.fields()["reference_line_segments"][0]["point_ids"][1],
        11
    );
    assert_eq!(
        sketch.fields()["bounded_curve_segments"][0]["external_id"],
        25
    );
    assert_eq!(
        sketch.fields()["bounded_curve_segments"][0]["point_ids"][0],
        2
    );
    assert_eq!(
        sketch.fields()["bounded_curve_segments"][0]["point_ids"][1],
        3
    );
    assert!(sketch.fields()["opaque_segments"]
        .as_array()
        .is_some_and(Vec::is_empty));
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SEGMENT_ROW_COUNT),
        6
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_CIRCLE_SEGMENT_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_POINT_SEGMENT_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_CENTERED_LINE_SEGMENT_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_REFERENCE_LINE_SEGMENT_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_BOUNDED_CURVE_SEGMENT_COUNT),
        1
    );
    assert!(!coverage
        .coverage
        .contains_key("decoded_feature_segment_count"));
    assert_eq!(
        coverage.coverage_count(crate::coverage::RESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT)
            + coverage.coverage_count(crate::coverage::UNRESOLVED_FEATURE_SEGMENT_GEOMETRY_COUNT),
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SEGMENT_ROW_COUNT)
    );
}

#[test]
fn scan_includes_named_segtab_prototype_as_data() {
    let payload = b"feat_defs_40\0segtab_ptr\0\xf8\x01\xf7\x01\xfb\xe2\
        type\0\x02dir\0\xf8\x03\xf6\x00\xe4pointid\0\xf8\x02\x00\x01\
        cntrid\0\xf6arcorient\0\x00verhor\0\x01radius\0\xf6radius2\0\xf6\
        ext_id\0\x04\xf2\xf7\x01\xe2order_table\0";
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload.to_vec())]));
    let segments = scan.features.definitions[0]
        .segments
        .as_ref()
        .expect("segtab");

    assert_eq!(segments.rows.len(), 1);
    assert_eq!(segments.rows[0].external_id, 4);
    assert_eq!(segments.rows[0].point_ids, [0, 1]);
    assert_eq!(segments.rows[0].vertical_horizontal, Some(1));
}

#[test]
fn scan_decodes_featdefs_ent_tab_trimmed_entities() {
    let mut payload =
        b"feat_defs_40\0ent_tab\0\xe3entry_ptr(entity_entry)\0schema\xf2\xf7\x01\xe3".to_vec();
    payload.extend_from_slice(&[42, 0, 100, 101, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(&[43, 0, 101, 102, 103, 0, 0xe3]);
    payload.extend_from_slice(&[0x80, 0xe3, 0, 102, 104, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(b"vert_tab\0");
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let entities = scan.features.definitions[0]
        .trim_entities
        .as_ref()
        .expect("ent_tab");
    assert_eq!(entities.rows.len(), 3);
    assert_eq!(entities.rows[0].external_id, 42);
    assert_eq!(entities.rows[0].vertices, [100, 101]);
    assert_eq!(entities.rows[0].center_vertex, None);
    assert_eq!(entities.rows[0].kind, crate::feature::TrimEntityKind::Line);
    assert_eq!(entities.rows[1].kind, crate::feature::TrimEntityKind::Arc);
    assert_eq!(entities.rows[2].external_id, 227);
    assert_eq!(entities.solved_external_ids, vec![42, 43, 227]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let trim_entities = &result.ir().native.namespace("creo").unwrap().arenas["sketches"][0]
        .fields()["trim_entities"];
    assert_eq!(
        trim_entities.as_array().expect("trim entity array").len(),
        3
    );
    assert_eq!(trim_entities[0]["kind"], "line");
    assert_eq!(trim_entities[1]["kind"], "arc");
}

#[test]
fn scan_decodes_featdefs_vert_tab_entity_pairs() {
    let mut payload =
        b"feat_defs_40\0ent_tab\0\xe3entry_ptr(entity_entry)\0schema\xf2\xf7\x01\xe3".to_vec();
    payload.extend_from_slice(&[42, 0, 100, 101, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(&[43, 0, 100, 102, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(b"vert_tab\0chains\0\xf8\x01\xf7\x80\xa2\xfb\xe2");
    payload.extend_from_slice(b"\xf3\xf7\x80\xa2\xe2\x01\xf8\x01\xf7\x80\xa3\xfb\xe3\xf7\x80\xa4");
    payload.extend_from_slice(&[42, 43, 100, 0]);
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let vertices = scan.features.definitions[0]
        .trim_vertices
        .as_ref()
        .expect("vert_tab");
    assert_eq!(vertices.rows.len(), 1);
    assert_eq!(vertices.rows[0].vertex_id, 100);
    assert_eq!(vertices.rows[0].entities, [42, 43]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let trim_vertices = &result.ir().native.namespace("creo").unwrap().arenas["sketches"][0]
        .fields()["trim_vertices"];
    assert_eq!(
        trim_vertices.as_array().expect("trim vertex array").len(),
        1
    );
    assert_eq!(trim_vertices[0]["vertex_id"], 100);
    assert_eq!(trim_vertices[0]["entities"][0], 42);
    assert_eq!(trim_vertices[0]["entities"][1], 43);
}

#[test]
fn scan_solves_featdefs_trim_vertex_line_intersection() {
    fn variable_row(payload: &mut Vec<u8>, variable_type: u8, key: u8, value: f64) {
        payload.extend_from_slice(&[variable_type, key]);
        match value {
            0.0 => payload.push(0x0f),
            1.0 => payload.push(0xe4),
            2.0 => payload.extend_from_slice(&[0x46, 0, 0, 0, 0, 0, 0, 0]),
            _ => unreachable!("generated fixture uses defined scalar constants"),
        }
        payload.extend_from_slice(&[0x0f, 1, 0, key, 0xe2]);
    }

    let mut payload =
        b"feat_defs_40\0var_arr\0\xf8\x08\xf7\x01\xfb\xe2schema\xf1\xf7\x01\xe2".to_vec();
    for (point, u, v) in [(7, 0.0, 0.0), (8, 2.0, 2.0), (9, 0.0, 2.0), (10, 2.0, 0.0)] {
        variable_row(&mut payload, 1, point, u);
        variable_row(&mut payload, 2, point, v);
    }
    payload.extend_from_slice(b"\xffsegtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2");
    payload.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    payload.extend_from_slice(&[2, 0, 0, 0, 9, 10, 0xf6, 0, 0, 0xf6, 0xf6, 43, 0xe2, 0xe3]);
    payload.extend_from_slice(b"ent_tab\0\xe3entry_ptr(entity_entry)\0schema\xf2\xf7\x01\xe3");
    payload.extend_from_slice(&[42, 0, 100, 101, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(&[43, 0, 100, 102, 0xf6, 0, 0xe3]);
    payload.extend_from_slice(b"vert_tab\0chains\0\xf8\x01\xf7\x80\xa2\xfb\xe2");
    payload.extend_from_slice(b"\xf3\xf7\x80\xa2\xe2\x01\xf8\x01\xf7\x80\xa3\xfb\xe3\xf7\x80\xa4");
    payload.extend_from_slice(&[42, 43, 100, 0]);

    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));
    let vertex = &scan.features.definitions[0]
        .trim_vertices
        .as_ref()
        .expect("vert_tab")
        .rows[0];
    assert_eq!(vertex.section_coordinates, Some([1.0, 1.0]));
}

#[test]
fn scan_decodes_featdefs_generated_entity_order_table() {
    let payload = b"feat_defs_40\0gsec3d_ptr\0order_table\0\xf8\x02\xf7\x81\x02\xfb\xe2\
        \xe0\x01ext_id\0\xe0\x01int_id\0\xe0\x01bitmask\0\
        \xf1\xf7\x81\x02\xe2\x81\x1b\x08\x00\xe2\x81\x36\x0c\x01\xe0\x01next_field\0"
        .to_vec();
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let order = scan.features.definitions[0]
        .order_table
        .as_ref()
        .expect("order_table");
    assert_eq!(order.declared_count, 2);
    assert!(!order.has_prototype);
    assert!(order.is_complete());
    assert_eq!(order.entity_ref, Some(258));
    assert_eq!(order.rows.len(), 2);
    assert_eq!(order.rows[0].external_id, 283);
    assert_eq!(order.rows[0].internal_id, 8);
    assert_eq!(order.rows[0].bitmask, 0);
    assert_eq!(order.external_id(12), Some(310));
    assert_eq!(order.internal_id(283), Some(8));

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let order_rows =
        &result.ir().native.namespace("creo").unwrap().arenas["sketches"][0].fields()["order_rows"];
    assert_eq!(order_rows.as_array().expect("order row array").len(), 2);
    assert_eq!(order_rows[0]["external_id"], 283);
    assert_eq!(order_rows[1]["internal_id"], 12);
}

#[test]
fn scan_decodes_featdefs_dimension_prototype_and_replay() {
    let mut payload = b"feat_defs_40\0\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x03\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a"
        .to_vec();
    payload.extend_from_slice(b"\xf3\xf7\x81\x02\xe2");
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
    payload.extend_from_slice(b"\xf3\xf7\x81\x02\xe2");
    payload.extend_from_slice(&[10, 0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f, 0, 0x18, 44]);
    payload.extend_from_slice(b"\xe0\x00relat_ptr\0");
    let expressions = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02angle=d42\0length=d43+2[mm]\0"
        .to_vec();
    let scan = container::scan_bytes(build_prt(
        "c",
        &[("FeatDefs", payload), ("DEPDB_DATA", expressions)],
    ));

    let dimensions = scan.features.definitions[0]
        .dimensions
        .as_ref()
        .expect("dimtab");
    assert_eq!(dimensions.declared_count, 3);
    assert_eq!(dimensions.entity_ref, Some(258));
    assert_eq!(dimensions.rows.len(), 3);
    assert_eq!(dimensions.rows[0].dimension_type, 10);
    assert_eq!(dimensions.rows[0].value, Some(1.0));
    assert_eq!(
        dimensions.rows[0].value_unit,
        crate::feature::DimensionUnit::Radians
    );
    assert_eq!(dimensions.rows[0].direction_byte, 1);
    assert_eq!(dimensions.rows[0].auxiliary_value, Some(0.0));
    assert_eq!(dimensions.rows[0].value_body, [0xe4]);
    assert_eq!(dimensions.rows[0].auxiliary_body, [0x0f]);
    assert_eq!(dimensions.rows[0].external_id, 42);
    assert_eq!(dimensions.rows[1].value, Some(3.0));
    assert_eq!(
        dimensions.rows[1].value_body,
        [0x46, 0x08, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(dimensions.rows[1].auxiliary_body, [0x18]);
    assert_eq!(
        dimensions.rows[1].value_unit,
        crate::feature::DimensionUnit::Millimeters
    );
    assert_eq!(dimensions.rows[1].auxiliary_value, Some(0.0));
    assert_eq!(dimensions.rows[1].external_id, 43);
    assert_eq!(
        dimensions.rows[2].value,
        Some(f64::from_be_bytes([
            0x3f, 0xd5, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f
        ]))
    );
    assert_eq!(dimensions.rows[2].external_id, 44);
    assert_eq!(scan.curves.expressions.len(), 1);
    assert_eq!(
        scan.curves.expressions[0].assignments[0].value,
        Some(crate::curve::CurveExpressionValue::Angle(
            1.0f64.to_degrees()
        ))
    );
    assert_eq!(
        scan.curves.expressions[0].assignments[1].value,
        Some(crate::curve::CurveExpressionValue::Length(5.0))
    );
}

#[test]
fn scan_decodes_counted_featdefs_constraint_relations() {
    let mut payload = b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x04\xf7\x6a\xfb\xe2\
        \xe0\x01id\0\xe0\x01used\0\xe0\x01type\0\xf1\xf7\x6a\xe2\
        \x34\x00\x05\x01\xf6\xe4\x00\xe6\x0f\x10\x0f\xe4\x00\x00\x00\xe2\
        \x35\x01\x07\x29\x32\xf6\x00\xe6\x0f\x10\x0f\xe4\x01\x2a\x03\xe2"
        .to_vec();
    payload.extend_from_slice(
        b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
          \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
          \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
          \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
          \xf3\xf7\x6b\xe2\
          triples_ptr\0\xf4\x04\xf8\x02\xf7\x6d\xfb\xe2\
          \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\xe0\x01skamp_id\0\x05\
          \xf1\xf7\x6d\xe2\xf6\x09\x05\xe2",
    );
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let relations = scan.features.definitions[0]
        .relations
        .as_ref()
        .expect("relat_ptr");
    assert_eq!(relations.declared_count, 4);
    assert_eq!(relations.entity_ref, Some(106));
    assert_eq!(relations.rows.len(), 2);
    assert_eq!(relations.rows[0].relation_id, 52);
    assert_eq!(relations.rows[0].used, 0);
    assert_eq!(
        relations.rows[0].operands,
        [0x05, 0x01, 0xf6, 0xe4, 0x00, 0xe6, 0x0f, 0x10, 0x0f, 0xe4]
    );
    assert_eq!(
        relations.rows[0].operand_vectors,
        Some([
            [Some(5), Some(1), None, Some(1)],
            [Some(0), Some(0), Some(0), Some(0)],
            [Some(15), Some(16), Some(15), Some(1)],
        ])
    );
    assert_eq!(relations.rows[0].sign, 0);
    assert_eq!(relations.rows[0].dimension_id, 0);
    assert_eq!(relations.rows[0].relation_type, 0);
    assert_eq!(relations.rows[1].relation_id, 53);
    assert_eq!(relations.rows[1].used, 1);
    assert_eq!(relations.rows[1].dimension_id, 42);
    assert_eq!(relations.rows[1].relation_type, 3);
    assert_eq!(relations.skamps.len(), 1);
    assert_eq!(relations.skamps[0].id, 5);
    assert_eq!(relations.skamps[0].kind, 2);
    assert_eq!(relations.skamps[0].items[0].entity_id, 42);
    assert_eq!(relations.skamps[0].items[0].sense, 1);
    let skamp_header = relations.skamp_header.as_ref().expect("skamp header");
    assert_eq!(skamp_header.declared_count, 1);
    assert_eq!(skamp_header.entity_ref, 107);
    assert!(relations.offset < skamp_header.offset);
    assert!(skamp_header.offset <= relations.skamps[0].offset);
    assert_eq!(relations.triples.len(), 2);
    assert_eq!(relations.triples[0].relation_id, Some(7));
    assert_eq!(relations.triples[0].equation_id, Some(8));
    assert_eq!(relations.triples[0].skamp_id, Some(5));
    assert_eq!(relations.triples[1].relation_id, None);
    assert_eq!(relations.triples[1].equation_id, Some(9));
    let triples_header = relations.triples_header.as_ref().expect("triples header");
    assert_eq!(triples_header.declared_count, 2);
    assert_eq!(triples_header.entity_ref, 109);
    assert!(skamp_header.offset < triples_header.offset);
    assert!(triples_header.offset <= relations.triples[0].offset);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let sketch_fields =
        result.ir().native.namespace("creo").unwrap().arenas["sketches"][0].fields();
    let headers = sketch_fields["table_headers"]
        .as_array()
        .expect("table headers");
    let solver = headers
        .iter()
        .find(|header| header["kind"] == "solver_incidences")
        .expect("solver-incidence header");
    assert_eq!(solver["declared_count"], 1);
    assert_eq!(solver["entity_ref"], 107);
    assert_eq!(solver["row_count"], 1);
    let triples = headers
        .iter()
        .find(|header| header["kind"] == "relation_triples")
        .expect("relation-triple header");
    assert_eq!(triples["declared_count"], 2);
    assert_eq!(triples["entity_ref"], 109);
    assert_eq!(triples["row_count"], 2);
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_RELATION_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_RELATION_ROW_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MALFORMED_FEATURE_RELATION_TABLE_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_SKAMP_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_SKAMP_ROW_COUNT),
        0
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_FEATURE_RELATION_TRIPLE_COUNT),
        2
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::MISSING_FEATURE_RELATION_TRIPLE_ROW_COUNT),
        0
    );
}

#[test]
fn scan_decodes_extended_solver_incidences() {
    let payload = b"feat_defs_40\0relat_ptr\0\xf4\x04\xf8\x02\xf7\x6a\xfb\xe2\
        schema\xf1\xf7\x6a\xe2\
        skamp_ptr\0\xf4\x05\xf8\x02\xf7\x6b\xfb\xe2\
        \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
        \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
        \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
        \xf3\xf7\x6b\xe2\
        \xc0\x40\x01\x0e\xc0\x40\x00\x22\xf8\x03\xf7\x6c\xfb\xe2\
        \xf7\x6d\x09\x03\xf1\xf7\x6c\xe2\x0a\x02\xe2\x0b\x03\
        \xe0\x00triples_ptr\0"
        .to_vec();
    let scan = container::scan_bytes(build_prt("c", &[("FeatDefs", payload)]));
    let relations = scan.features.definitions[0]
        .relations
        .as_ref()
        .expect("relat_ptr");

    assert_eq!(relations.skamps.len(), 2);
    assert_eq!(relations.skamps[1].id, 0x4001);
    assert_eq!(relations.skamps[1].kind, 14);
    assert_eq!(relations.skamps[1].flags, 0x4000);
    assert_eq!(relations.skamps[1].status, 34);
    assert_eq!(
        relations.skamps[1]
            .items
            .iter()
            .map(|item| (item.entity_id, item.sense))
            .collect::<Vec<_>>(),
        [(9, 3), (10, 2), (11, 3)]
    );
}

#[test]
fn scan_decodes_featdefs_saved_line_prototype_and_replay() {
    let mut payload = b"feat_defs_40\0\xe0\x00gsec3d_ptr\0\
        \xe0\x00p_saved_result\0\xe3\
        \xe0\x00entity(line)\0\xe3\xf7\x01\x00\xf7\x02\xe2\
        \xf1\xf7\x03\x2a\xe2"
        .to_vec();
    payload.extend_from_slice(&[0x0f, 0xe4, 0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4, 0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(b"\xf0\xf7\x04\xeb\x01\x02\x03\x04\x05\x2b\xe2");
    payload.extend_from_slice(&[0xe4, 0xe4, 0x0f, 0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0xe3]);
    payload.extend_from_slice(b"\xe0\x02local_sys\0");
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let saved = scan.features.definitions[0]
        .saved_section
        .as_ref()
        .expect("p_saved_result");
    assert_eq!(saved.entities.len(), 2);
    let crate::feature::FeatureSavedEntity::Line(first) = &saved.entities[0] else {
        panic!("saved line prototype");
    };
    assert_eq!(first.entity_id, 42);
    assert_eq!(first.references, vec![3]);
    assert_eq!(
        first.endpoints,
        [
            [Some(0.0), Some(1.0), Some(3.0)],
            [Some(1.0), Some(0.0), Some(1.0)]
        ]
    );
    let crate::feature::FeatureSavedEntity::Line(second) = &saved.entities[1] else {
        panic!("saved line replay");
    };
    assert_eq!(second.entity_id, 43);
    assert_eq!(second.references, vec![4]);
    assert_eq!(second.attributes, vec![[1, 2, 3, 4, 5]]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let native_saved = &result.ir().native.namespace("creo").unwrap().arenas["sketches"][0]
        .fields()["saved_entities"];
    for (native, expected) in native_saved
        .as_array()
        .expect("saved entity array")
        .iter()
        .zip([&first.body, &second.body])
    {
        let body = native["body"]
            .as_array()
            .expect("saved line body")
            .iter()
            .map(|byte| byte.as_u64().expect("byte") as u8)
            .collect::<Vec<_>>();
        assert_eq!(&body, expected);
    }
}

#[test]
fn scan_decodes_featdefs_saved_circular_and_dummy_entities() {
    let mut payload = b"feat_defs_40\0\xe0\x00gsec3d_ptr\0\
        \xe0\x00p_saved_result\0\xe3\
        \xe0\x00entity(arc)\0\xe0\x01id\0\x2c\
        \xe0\x02center\0\xf1\xf8\x03\x0f\xe4"
        .to_vec();
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(b"\xe0\x02radius\0\xe4");
    payload.extend_from_slice(b"\xe0\x02end1\0\xf8\x03\x0f\x0f\x0f");
    payload.extend_from_slice(b"\xe0\x02end2\0\xf8\x03\xe4\xe4\xe4");
    payload.extend_from_slice(b"\xe0\x02t0\0\x0f\xe0\x02t1\0\xe4");
    payload.extend_from_slice(
        b"\xe0\x00entity(circle)\0\xe0\x01id\0\x2d\
          \xe0\x02center\0\xf8\x03\x18\xe5\
          \xe0\x02radius\0\xe4",
    );
    payload.extend_from_slice(b"\xe0\x00entity(dummy_ent)\0\xe0\x01id\0\x2e");
    payload.extend_from_slice(b"\xe0\x02local_sys\0");
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());

    let entities = &scan.features.definitions[0]
        .saved_section
        .as_ref()
        .expect("p_saved_result")
        .entities;
    assert_eq!(entities.len(), 3);
    let crate::feature::FeatureSavedEntity::Arc(arc) = &entities[0] else {
        panic!("saved arc");
    };
    assert_eq!(arc.entity_id, 44);
    assert_eq!(arc.center, [Some(0.0), Some(1.0), Some(3.0)]);
    assert_eq!(arc.radius, Some(1.0));
    assert_eq!(arc.parameters, [Some(0.0), Some(1.0)]);
    let crate::feature::FeatureSavedEntity::Circle(circle) = &entities[1] else {
        panic!("saved circle");
    };
    assert_eq!(circle.entity_id, 45);
    assert_eq!(circle.center, [Some(0.0), Some(1.0), Some(0.0)]);
    let crate::feature::FeatureSavedEntity::Dummy(dummy) = &entities[2] else {
        panic!("saved dummy");
    };
    assert_eq!(dummy.entity_id, Some(46));

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let saved = &result.ir().native.namespace("creo").unwrap().arenas["sketches"][0].fields()
        ["saved_entities"];
    assert_eq!(saved.as_array().expect("saved entity array").len(), 3);
    assert_eq!(saved[0]["kind"], "arc");
    assert_eq!(saved[1]["kind"], "circle");
    assert_eq!(saved[2]["kind"], "dummy");
    for (native, expected) in saved.as_array().expect("saved entity array").iter().zip([
        &arc.body,
        &circle.body,
        &dummy.body,
    ]) {
        let body = native["body"]
            .as_array()
            .expect("saved entity body")
            .iter()
            .map(|byte| byte.as_u64().expect("byte") as u8)
            .collect::<Vec<_>>();
        assert_eq!(&body, expected);
    }
}
