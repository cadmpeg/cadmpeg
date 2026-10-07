// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::directory::{DirectoryEntry, SourceStatus};
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use crate::test_support::test_drawing_and_trimming::{
    dimension_forms_file, general_symbol_form_file, inverted_sectioned_area_file,
    inverted_sectioned_area_file_with_global, legacy_dimension_and_label_forms_file,
    out_of_table_sectioned_area_pattern_file, symbol_and_sectioned_area_file,
};
use crate::test_support::test_owned::{owned_test_file_with_global, OwnedTestEntity};
use crate::test_support::test_solids_and_structure::test_annotation_variants::{
    leader_forms_file, out_of_table_annotation_font_values_file,
};
use crate::test_support::test_solids_and_structure::{
    defaulted_new_general_note_file, malformed_general_note_parameter_types_file,
    negative_text_box_dimensions_file, new_general_note_character_set_file,
    new_general_note_even_character_set_pointer_file, new_general_note_type_310_character_set_file,
    new_general_note_wrong_type_character_set_pointer_file,
    omitted_character_count_new_general_note_file, omitted_character_metrics_new_general_note_file,
    omitted_font_style_new_general_note_file, text_annotation_file,
    variable_spacing_default_new_general_note_file, zero_character_metrics_new_general_note_file,
};
use crate::IgesCodec;

use crate::entities::presentation::general_note_font_valid_for_global_table;
use crate::global::GlobalTable;

use super::{
    dimension_enclosure_type_allowed, fill_pattern_valid_for_global_table, fixed_or_variable_valid,
    flag_or_label_valid, general_note_string_count_valid, general_note_text_valid_for_global_table,
    general_symbol_note_valid, justification_valid, leader_valid_for_global_table,
    mirror_flag_valid, new_general_note_charset_valid, new_general_note_font_valid,
    sectioned_area_curve_coplanar, sectioned_area_valid, vertical_text_flag_valid,
};

fn assert_section_refusal(bytes: &[u8], operation: &str, dimension: ResourceDimension) {
    cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("unsupported test dimension"),
        }
        IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions { policy, ..DecodeOptions::default() })
            .map_err(|failure| match failure { DecodeFailure::Codec(error) => error, other => panic!("unexpected decode failure: {other:?}") })
    });
}

#[test]
fn sectioned_area_coplanarity_refuses_active_nodes_and_recursion() {
    let bytes = symbol_and_sectioned_area_file();
    assert_section_refusal(
        &bytes,
        "iges section active curves",
        ResourceDimension::CollectionItems,
    );
    assert_section_refusal(
        &bytes,
        "iges coplanar curve recursion",
        ResourceDimension::RecursionDepth,
    );
}

#[test]
fn malformed_flag_note_width_sum_refuses_without_overflow() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut validation = super::AnnotationValidation::new(&ctx).unwrap();

    let mut flag = leader_entry(0);
    flag.entity_type = 208;
    let mut note_entry = leader_entry(0);
    note_entry.sequence = 3;
    note_entry.entity_type = 212;
    let entries = BTreeMap::from([(3, &note_entry)]);
    let record = |sequence, values: Vec<TokenValue>| {
        ParameterRecord::from_test_tokens(
            sequence,
            1..2,
            Vec::new(),
            values.len(),
            values
                .into_iter()
                .map(|value| Token { value, span: 0..0 })
                .collect(),
            Vec::new(),
        )
    };
    let flag_record = record(
        1,
        vec![
            TokenValue::Integer(208),
            TokenValue::real(0.0),
            TokenValue::real(0.0),
            TokenValue::real(0.0),
            TokenValue::real(0.0),
            TokenValue::Integer(3),
            TokenValue::Integer(0),
        ],
    );
    let mut note_values = vec![TokenValue::Omitted; 15];
    note_values[0] = TokenValue::Integer(212);
    note_values[1] = TokenValue::Integer(2);
    note_values[2] = TokenValue::Integer(i64::MAX);
    note_values[14] = TokenValue::Integer(i64::MAX);
    let note_record = record(3, note_values);
    let records = BTreeMap::from([(3, &note_record)]);
    assert!(!flag_or_label_valid(
        &flag,
        &flag_record,
        &entries,
        &records,
        GlobalTable::V5_0, &mut validation).unwrap());
}

#[test]
fn general_note_forms_follow_the_section_4_60_string_minima() {
    let cases = [
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
    ];
    for (form, minimum) in cases {
        assert!(crate::profile::general_note_form_admitted(form));
        assert!(general_note_string_count_valid(form, minimum));
        if minimum > 1 {
            assert!(!general_note_string_count_valid(form, minimum - 1));
        }
        assert!(super::classify(212, form).is_some());
    }
    for form in [-1, 9, 99, 103, 104, 106, 5001] {
        assert!(!crate::profile::general_note_form_admitted(form));
        assert!(!general_note_string_count_valid(form, 1));
        assert!(super::classify(212, form).is_none());
    }
}

#[test]
fn decode_preserves_general_note_text_runs_and_new_note_control_codes() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_annotation_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
    assert_eq!(annotations.len(), 2);
    assert_eq!(annotations[0].fields()["kind"], "general_note");
    assert_eq!(
        annotations[0].fields()["strings"].as_array().unwrap().len(),
        2
    );
    assert_eq!(annotations[0].fields()["strings"][0]["text"][0], 65);
    assert_eq!(annotations[0].fields()["strings"][1]["mirror"], 1);
    assert_eq!(annotations[0].fields()["strings"][1]["vertical"], 1);
    assert_eq!(annotations[1].fields()["kind"], "new_general_note");
    assert_eq!(annotations[1].fields()["justification"], 2);
    assert_eq!(
        annotations[1].fields()["strings"][0]["control_codes"][0],
        84
    );
    assert_eq!(annotations[1].fields()["strings"][0]["text"]["text"][3], 33);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn general_note_kanji_text_uses_biased_jis_hex_pairs() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    assert!(general_note_text_valid_for_global_table(
        b"34413B7A",
        2001,
        GlobalTable::V5_0,
        false, &ctx).unwrap());
    assert!(general_note_text_valid_for_global_table(
        b"",
        2001,
        GlobalTable::V5_0,
        false, &ctx).unwrap());
    assert!(!general_note_text_valid_for_global_table(
        b"34413B7",
        2001,
        GlobalTable::V5_0,
        false, &ctx).unwrap());
    assert!(!general_note_text_valid_for_global_table(
        b"34413B7G",
        2001,
        GlobalTable::V5_0,
        false, &ctx).unwrap());
    assert!(!general_note_text_valid_for_global_table(
        b"20413B7A",
        2001,
        GlobalTable::V5_0,
        false, &ctx).unwrap());
    assert!(general_note_text_valid_for_global_table(
        b" ",
        2001,
        GlobalTable::V5_0,
        true, &ctx).unwrap());
    assert!(!general_note_text_valid_for_global_table(
        b"34413B7A",
        2001,
        GlobalTable::V4_0,
        false, &ctx).unwrap());
}

#[test]
fn decode_accepts_and_retains_v5_0_kanji_general_note() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(v5_0_kanji_general_note_file("34413B7A")),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
    assert_eq!(
        annotations[0].fields()["strings"][0]["text"],
        serde_json::json!([51, 52, 52, 49, 51, 66, 55, 65])
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_malformed_v5_0_kanji_general_note_text() {
    for text in ["34413B7", "34413B7G", "20413B7A"] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(v5_0_kanji_general_note_file(text)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()),
            "text {text:?}: {:#?}",
            result.report().losses
        );
        let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
        assert_eq!(
            annotations[0].fields()["strings"][0]["text"],
            serde_json::json!(text.as_bytes())
        );
    }
}

fn v5_0_kanji_general_note_file(text: &str) -> Vec<u8> {
    let parameters = format!(
        "212,1,{length},20,4,2001,1.5707963267948966,0,0,0,0,0,0,{length}H{text};",
        length = text.len()
    );
    owned_test_file_with_global(
        &[OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "KANJI".into(),
            status: "00000100",
            parameters,
        }],
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;",
    )
}

#[test]
fn decode_applies_new_general_note_defaults_with_positive_metrics() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(defaulted_new_general_note_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"][0];
    assert_eq!(annotation.fields()["kind"], "new_general_note");
    assert_eq!(annotation.fields()["strings"][0]["fixed_or_variable"], 0);
    assert!(annotation.fields()["strings"][0]["control_codes"].is_null());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_applies_variable_spacing_default() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(variable_spacing_default_new_general_note_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"][0];
    assert_eq!(annotation.fields()["strings"][0]["fixed_or_variable"], 1);
    assert!(annotation.fields()["strings"][0]["character_spacing"].is_null());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_requires_new_general_note_character_count() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(omitted_character_count_new_general_note_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.semantic_annotations.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_does_not_default_wrong_typed_general_note_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(malformed_general_note_parameter_types_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .count();
    assert_eq!(losses, 2, "{:#?}", result.report().losses);
}

#[test]
fn decode_rejects_zero_new_general_note_character_metrics() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(zero_character_metrics_new_general_note_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.semantic_annotations.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_rejects_omitted_new_general_note_character_metrics() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(omitted_character_metrics_new_general_note_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.semantic_annotations.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_rejects_omitted_new_general_note_font_style() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(omitted_font_style_new_general_note_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.semantic_annotations.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_accepts_new_general_note_character_set_table_and_default() {
    for character_set in ["", "1", "1001", "1002", "1003", "2001", "3001"] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(new_general_note_character_set_file(character_set)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "character set {character_set:?}: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn decode_rejects_new_general_note_character_set_outside_table() {
    for character_set in ["0", "4", "1000", "3002"] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(new_general_note_character_set_file(character_set)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(result.ir().model.semantic_annotations.is_empty());
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()),
            "character set {character_set}: {:#?}",
            result.report().losses
        );
        assert_eq!(
            result.ir().native.namespace("iges").unwrap().arenas()["annotations"].len(),
            1
        );
    }
}

#[test]
fn decode_accepts_new_general_note_type_310_character_set_pointer() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(new_general_note_type_310_character_set_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_new_general_note_even_or_wrong_type_character_set_pointer() {
    for bytes in [
        new_general_note_even_character_set_pointer_file(),
        new_general_note_wrong_type_character_set_pointer_file(),
    ] {
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(result.ir().model.semantic_annotations.is_empty());
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    }
}

#[test]
fn decode_rejects_out_of_table_annotation_font_values() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(out_of_table_annotation_font_values_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 2, "{:#?}", result.report().losses);
    assert!(result.ir().model.semantic_annotations.is_empty());
}

#[test]
fn decode_rejects_out_of_table_sectioned_area_pattern() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(out_of_table_sectioned_area_pattern_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(result.ir().model.semantic_annotations.is_empty());
}

#[test]
fn decode_rejects_negative_text_box_dimensions_at_cadir_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(negative_text_box_dimensions_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
    assert_eq!(annotations.len(), 2);
    assert!(result.ir().model.semantic_annotations.is_empty());
    assert!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
            .count()
            >= 2
    );
}

#[test]
fn drawing_and_presentation_enumerations_match_the_iges_tables() {
    let entries = BTreeMap::new();
    for value in [
        0, 1, 2, 3, 6, 12, 13, 14, 17, 18, 19, 1001, 1002, 1003, 2001, 3001,
    ] {
        assert!(
            general_note_font_valid_for_global_table(value, &entries, GlobalTable::V5Later),
            "font code {value}"
        );
    }
    for value in [-1, 4, 5, 7, 1000, 3002] {
        assert!(
            !general_note_font_valid_for_global_table(value, &entries, GlobalTable::V5Later),
            "font code {value}"
        );
    }
    for value in [1, 2, 3, 6, 12, 13, 14, 17, 18, 19] {
        assert!(new_general_note_font_valid(value), "font style {value}");
    }
    for value in [0, 4, 5, 7, 1001, -1] {
        assert!(!new_general_note_font_valid(value), "font style {value}");
    }
    let entries = BTreeMap::new();
    for value in [1, 1001, 1002, 1003, 2001, 3001] {
        assert!(
            new_general_note_charset_valid(value, &entries),
            "character set {value}"
        );
    }
    for value in [0, 4, 1000, 3002, -1] {
        assert!(
            !new_general_note_charset_valid(value, &entries),
            "character set {value}"
        );
    }

    for value in 0..=3 {
        assert!(justification_valid(value));
    }
    assert!(!justification_valid(-1));
    assert!(!justification_valid(4));

    for value in 0..=1 {
        assert!(fixed_or_variable_valid(value));
        assert!(vertical_text_flag_valid(value));
    }
    assert!(!fixed_or_variable_valid(-1));
    assert!(!fixed_or_variable_valid(2));
    assert!(!vertical_text_flag_valid(-1));
    assert!(!vertical_text_flag_valid(2));

    for value in 0..=2 {
        assert!(mirror_flag_valid(value));
    }
    assert!(!mirror_flag_valid(-1));
    assert!(!mirror_flag_valid(3));

    for value in [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 22, 26, 28, 29,
        32, 34, 36, 38, 40, 41, 42, 46, 50, 60, 70, 72, 80, 82, 84, 86, 90, 92, 94, 110, 124, 134,
        136, 140, 142, 152, 154, 156, 157, 158, 159, 172, 174, 178, 210, 220, 224, 226, 234, 236,
        240, 244, 246, 252, 254, 256, 262, 264, 265, 266, 268,
    ] {
        assert!(
            fill_pattern_valid_for_global_table(value, GlobalTable::V5Later),
            "admitted fill pattern {value}"
        );
    }
    for value in [
        21, 23, 24, 25, 27, 30, 31, 33, 35, 37, 39, 43, 44, 45, 47, 48, 49, 51, 269,
    ] {
        assert!(
            !fill_pattern_valid_for_global_table(value, GlobalTable::V5Later),
            "reserved fill pattern {value}"
        );
    }
}

#[test]
fn sectioned_area_fill_patterns_follow_the_declared_dialect() {
    assert!(fill_pattern_valid_for_global_table(19, GlobalTable::V4_0));
    assert!(!fill_pattern_valid_for_global_table(20, GlobalTable::V4_0));
    assert!(fill_pattern_valid_for_global_table(20, GlobalTable::V5_0));
    assert!(fill_pattern_valid_for_global_table(
        268,
        GlobalTable::V5Later
    ));
    assert!(!fill_pattern_valid_for_global_table(
        269,
        GlobalTable::V5Later
    ));
}

#[test]
fn sectioned_area_curve_coplanarity_uses_model_space_geometry() {
    crate::test_support::with_service_context(&[], |decode_ctx| {
        let mut ir = CadIr::empty();
        for (sequence, z) in [(1, 0.0), (3, 0.0)] {
            ir.model.curves.push(Curve {
                id: CurveId::mint(format!("iges:model:curve#D{sequence}"))
                    .expect("identity grammar"),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::new(0.0, 0.0, z),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        1.0,
                    )
                    .unwrap(),
                )),
                source_object: None,
            });
        }
        let pattern_plane = (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0));
        assert!({
            let mut index = None;
            decode_ctx.all_by([1, 3], |sequence| sectioned_area_curve_coplanar(
                (&ir, &mut index), sequence, pattern_plane, 0.001, decode_ctx,
            ), "test section boundary traversal")
        }
        .unwrap());
        if let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) =
            &mut ir.model.curves[1].geometry
        {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            let mut center = center;
            center.z = 0.01;
            *circle_curve = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                center,
                *axis,
                *ref_direction,
                radius,
            )
            .unwrap();
        }
        assert!(!{
            let mut index = None;
            decode_ctx.all_by([1, 3], |sequence| sectioned_area_curve_coplanar(
                (&ir, &mut index), sequence, pattern_plane, 0.001, decode_ctx,
            ), "test section boundary traversal")
        }
        .unwrap());

        let entry = |sequence, entity_type| DirectoryEntry {
            source_offset: 0,
            sequence,
            entity_type,
            parameter_start: 0,
            structure: 0,
            line_font: 0,
            level: 0,
            view: 0,
            transform: 0,
            label_display: 0,
            status: SourceStatus::from_codes([0, 0, 0, 0]),
            line_weight: 0,
            color: 0,
            parameter_line_count: 1,
            form: 0,
            reserved: [[b' '; 8]; 2],
            label: [b' '; 8],
            subscript: 0,
        };
        let boundary = entry(1, 100);
        let island = entry(3, 100);
        let entries = BTreeMap::from([(1, &boundary), (3, &island)]);
        let record_values = [
            TokenValue::Integer(230),
            TokenValue::Integer(1),
            TokenValue::Integer(2),
            TokenValue::real(0.0),
            TokenValue::real(0.0),
            TokenValue::real(0.0),
            TokenValue::real(std::f64::consts::FRAC_PI_4),
            TokenValue::real(0.0),
            TokenValue::Integer(1),
            TokenValue::Integer(3),
        ];
        let record = ParameterRecord::from_test_tokens(
            5,
            1..2,
            Vec::new(),
            record_values.len(),
            record_values
                .into_iter()
                .map(|value| Token { value, span: 0..0 })
                .collect(),
            Vec::new(),
        );
        assert!(sectioned_area_valid(
            (&ir, &mut None),
            &record,
            &entries,
            0,
            super::SectionedAreaContext {
                global_table: GlobalTable::V4_0,
                transform: Transform::identity(),
                length_factor: 1.0,
                resolution: 0.001
            },
            decode_ctx
        )
        .unwrap());
        assert!(!sectioned_area_valid(
            (&ir, &mut None),
            &record,
            &entries,
            0,
            super::SectionedAreaContext {
                global_table: GlobalTable::V5_0,
                transform: Transform::identity(),
                length_factor: 1.0,
                resolution: 0.001
            },
            decode_ctx
        )
        .unwrap());
        if let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) =
            &mut ir.model.curves[0].geometry
        {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            let mut center = center;
            center.z = 0.01;
            *circle_curve = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                center,
                *axis,
                *ref_direction,
                radius,
            )
            .unwrap();
        }
        let translated_pattern_plane = Transform::affine([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.01],
        ])
        .expect("affine transform");
        assert!(sectioned_area_valid(
            (&ir, &mut None),
            &record,
            &entries,
            0,
            super::SectionedAreaContext {
                global_table: GlobalTable::V5_0,
                transform: translated_pattern_plane,
                length_factor: 1.0,
                resolution: 0.001
            },
            decode_ctx
        )
        .unwrap());
    });
}

#[test]
fn sectioned_area_form1_allows_a_null_boundary_and_requires_an_island() {
    crate::test_support::with_service_context(&[], |decode_ctx| {
        let mut ir = CadIr::empty();
        for sequence in [1, 3] {
            ir.model.curves.push(Curve {
                id: CurveId::mint(format!("iges:model:curve#D{sequence}"))
                    .expect("identity grammar"),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        1.0,
                    )
                    .unwrap(),
                )),
                source_object: None,
            });
        }
        let entry = |sequence, entity_type| DirectoryEntry {
            source_offset: 0,
            sequence,
            entity_type,
            parameter_start: 0,
            structure: 0,
            line_font: 0,
            level: 0,
            view: 0,
            transform: 0,
            label_display: 0,
            status: SourceStatus::from_codes([0, 0, 0, 0]),
            line_weight: 0,
            color: 0,
            parameter_line_count: 1,
            form: 0,
            reserved: [[b' '; 8]; 2],
            label: [b' '; 8],
            subscript: 0,
        };
        let island = entry(3, 100);
        let entries = BTreeMap::from([(3, &island)]);
        let record = |island_count: i64| {
            let values = [
                TokenValue::Integer(230),
                TokenValue::Integer(0),
                TokenValue::Integer(2),
                TokenValue::real(0.0),
                TokenValue::real(0.0),
                TokenValue::real(0.0),
                TokenValue::real(1.0),
                TokenValue::real(std::f64::consts::FRAC_PI_4),
                TokenValue::Integer(island_count),
                TokenValue::Integer(3),
            ];
            ParameterRecord::from_test_tokens(
                5,
                1..2,
                Vec::new(),
                if island_count == 0 { 9 } else { values.len() },
                values
                    .into_iter()
                    .map(|value| Token { value, span: 0..0 })
                    .collect(),
                Vec::new(),
            )
        };

        assert!(sectioned_area_valid(
            (&ir, &mut None),
            &record(1),
            &entries,
            1,
            super::SectionedAreaContext {
                global_table: GlobalTable::V5_0,
                transform: Transform::identity(),
                length_factor: 1.0,
                resolution: 0.001
            },
            decode_ctx
        )
        .unwrap());
        assert!(!sectioned_area_valid(
            (&ir, &mut None),
            &record(0),
            &entries,
            1,
            super::SectionedAreaContext {
                global_table: GlobalTable::V5_0,
                transform: Transform::identity(),
                length_factor: 1.0,
                resolution: 0.001
            },
            decode_ctx
        )
        .unwrap());
        assert!(!sectioned_area_valid(
            (&ir, &mut None),
            &record(1),
            &entries,
            0,
            super::SectionedAreaContext {
                global_table: GlobalTable::V5_0,
                transform: Transform::identity(),
                length_factor: 1.0,
                resolution: 0.001
            },
            decode_ctx
        )
        .unwrap());
    });
}

#[test]
fn point_dimension_enclosure_types_follow_the_declared_dialect() {
    assert!(dimension_enclosure_type_allowed(100, 0, GlobalTable::V4_0));
    assert!(dimension_enclosure_type_allowed(102, 0, GlobalTable::V4_0));
    assert!(!dimension_enclosure_type_allowed(
        106,
        63,
        GlobalTable::V4_0
    ));
    assert!(dimension_enclosure_type_allowed(106, 63, GlobalTable::V5_0));
}

fn leader_record(arrowhead_height: f64, arrowhead_width: f64) -> ParameterRecord {
    let values = [
        TokenValue::Integer(214),
        TokenValue::Integer(1),
        TokenValue::real(arrowhead_height),
        TokenValue::real(arrowhead_width),
        TokenValue::real(1.0),
        TokenValue::real(2.0),
        TokenValue::real(3.0),
        TokenValue::real(4.0),
        TokenValue::real(5.0),
    ];
    ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        values.len(),
        values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect(),
        Vec::new(),
    )
}

fn leader_entry(form: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type: 214,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: SourceStatus::from_codes([0, 0, 1, 0]),
        line_weight: 0,
        color: 0,
        parameter_line_count: 1,
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

#[test]
fn leader_arrow_dimensions_follow_the_declared_dialect() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    let record = leader_record(1.0, 2.0);
    let entry = leader_entry(5);

    assert!(leader_valid_for_global_table(
        &entry,
        &record,
        GlobalTable::V4_0, &ctx).unwrap());
    assert!(!leader_valid_for_global_table(
        &entry,
        &record,
        GlobalTable::V5Later, &ctx).unwrap());

    let record = leader_record(1.0, 2.0);
    let entry = leader_entry(4);
    assert!(leader_valid_for_global_table(
        &entry,
        &record,
        GlobalTable::V4_0, &ctx).unwrap());
    assert!(!leader_valid_for_global_table(
        &entry,
        &record,
        GlobalTable::V5Later, &ctx).unwrap());
}

#[test]
fn general_symbol_zero_note_pointer_follows_the_declared_dialect() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut validation = super::AnnotationValidation::new(&ctx).unwrap();

    let record = ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        4,
        [228, 0, 1, 0]
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    );
    let entries = BTreeMap::new();
    let records = BTreeMap::new();

    assert!(!general_symbol_note_valid(
        &record,
        &entries,
        &records,
        0,
        GlobalTable::V4_0, &mut validation).unwrap());
    assert!(general_symbol_note_valid(
        &record,
        &entries,
        &records,
        0,
        GlobalTable::V5_0, &mut validation).unwrap());
    assert!(!general_symbol_note_valid(
        &record,
        &entries,
        &records,
        1,
        GlobalTable::V5_0, &mut validation).unwrap());
}

#[test]
fn decode_types_every_leader_arrow_form_and_segment_chain() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(leader_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
    assert_eq!(annotations.len(), 12);
    let mut forms = Vec::new();
    for annotation in annotations {
        assert_eq!(annotation.fields()["kind"], "leader");
        forms.push(annotation.fields()["form"].as_i64().unwrap());
        assert_eq!(annotation.fields()["arrowhead"][2], 3.0);
        assert_eq!(
            annotation.fields()["segment_tails"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(annotation.fields()["segment_tails"][1][1], 4.0);
    }
    forms.sort_unstable();
    assert_eq!(forms, (1..=12).collect::<Vec<_>>());
    let no_arrow = annotations
        .iter()
        .find(|annotation| annotation.fields()["form"] == 4)
        .unwrap();
    assert_eq!(no_arrow.fields()["arrowhead_size"][0], 0.0);
    let circle = annotations
        .iter()
        .find(|annotation| annotation.fields()["form"] == 5)
        .unwrap();
    assert_eq!(circle.fields()["arrowhead_size"][0], 2.0);
    assert_eq!(circle.fields()["arrowhead_size"][1], 2.0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_dimension_component_roles_for_every_admitted_form() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(dimension_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
    let kinds = annotations
        .iter()
        .filter_map(|annotation| annotation.fields()["kind"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "linear_dimension")
            .count(),
        3
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "ordinate_dimension")
            .count(),
        2
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "point_dimension")
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "radius_dimension")
            .count(),
        2
    );
    let point = annotations
        .iter()
        .find(|annotation| annotation.fields()["kind"] == "point_dimension")
        .unwrap();
    assert_eq!(point.fields()["note"], "iges:presentation:annotation#D1");
    assert_eq!(point.fields()["leader"], "iges:presentation:annotation#D3");
    assert_eq!(point.fields()["enclosure"], "iges:entity:directory#7");
    let radius = annotations
        .iter()
        .find(|annotation| {
            annotation.fields()["kind"] == "radius_dimension" && annotation.fields()["form"] == 1
        })
        .unwrap();
    assert_eq!(radius.fields()["center"][0], 10.0);
    assert_eq!(
        radius.fields()["leaders"][1],
        "iges:presentation:annotation#D9"
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::DisplayDataNotProjected.kind()));
}

#[test]
fn decode_types_angular_curve_diameter_flag_and_label_annotations() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(legacy_dimension_and_label_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
    for kind in [
        "angular_dimension",
        "curve_dimension",
        "diameter_dimension",
        "flag_note",
        "general_label",
    ] {
        assert!(annotations
            .iter()
            .any(|annotation| annotation.fields()["kind"] == kind));
    }
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::DisplayDataNotProjected.kind()));
}

#[test]
fn decode_types_general_symbol_components_and_section_fill_definition() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(symbol_and_sectioned_area_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
    let symbol = annotations
        .iter()
        .find(|annotation| annotation.fields()["kind"] == "general_symbol")
        .unwrap();
    assert_eq!(symbol.fields()["note"], "iges:presentation:annotation#D1");
    assert_eq!(symbol.fields()["declared_geometry_count"], 1);
    assert_eq!(symbol.fields()["declared_leader_count"], 1);
    assert_eq!(symbol.fields()["geometry"][0], "iges:entity:directory#3");
    assert_eq!(
        symbol.fields()["leaders"][0],
        "iges:presentation:annotation#D5"
    );
    let section = annotations
        .iter()
        .find(|annotation| annotation.fields()["kind"] == "sectioned_area")
        .unwrap();
    assert_eq!(section.fields()["boundary"], "iges:entity:directory#9");
    assert_eq!(section.fields()["fill_pattern"], 2);
    assert_eq!(section.fields()["pattern_spacing"], 1.0);
    assert_eq!(section.fields()["islands"][0], "iges:entity:directory#11");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_general_symbol_standard_forms_preserves_form_in_iges_4_0_and_5_0() {
    let globals = [
        (b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;".as_slice(), "4.0"),
        (b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;".as_slice(), "5.0"),
    ];
    for (global, version) in globals {
        for form in 1..=3 {
            let result = IgesCodec
                .decode(
                    &mut Cursor::new(general_symbol_form_file(form, global)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            let symbol = result.ir().native.namespace("iges").unwrap().arenas()["annotations"]
                .iter()
                .find(|annotation| annotation.fields()["kind"] == "general_symbol")
                .unwrap();
            assert_eq!(
                result.report().dialects().unwrap().primary().declared()["effective_version"],
                version
            );
            assert_eq!(symbol.fields()["form"], form);
            assert_eq!(symbol.fields()["note"], "iges:presentation:annotation#D1");
            assert_eq!(symbol.fields()["geometry"][0], "iges:entity:directory#3");
            assert_eq!(
                symbol.fields()["leaders"][0],
                "iges:presentation:annotation#D5"
            );
            assert!(result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()));
        }
    }
}

#[test]
fn decode_general_symbol_implementor_form_is_admitted_in_iges_5_0() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(general_symbol_form_file(5001, global)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let symbol = result.ir().native.namespace("iges").unwrap().arenas()["annotations"]
        .iter()
        .find(|annotation| annotation.fields()["kind"] == "general_symbol")
        .unwrap();
    assert_eq!(symbol.fields()["form"], 5001);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_type230_form1_preserves_inverted_crosshatching() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(inverted_sectioned_area_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"];
    assert_eq!(annotations.len(), 1);
    let section = &annotations[0];
    assert_eq!(section.fields()["kind"], "sectioned_area");
    assert_eq!(section.fields()["form"], 1);
    assert!(section.fields()["boundary"].is_null());
    assert_eq!(section.fields()["islands"][0], "iges:entity:directory#1");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_type230_form1_is_admitted_in_iges_5_0() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(inverted_sectioned_area_file_with_global(global)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        result.report().dialects().unwrap().primary().declared()["effective_version"],
        "5.0"
    );
    let section = &result.ir().native.namespace("iges").unwrap().arenas()["annotations"][0];
    assert_eq!(section.fields()["form"], 1);
    assert!(section.fields()["boundary"].is_null());
    assert_eq!(section.fields()["islands"][0], "iges:entity:directory#1");
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::EntityNotProjected.kind() }));
}

#[test]
fn annotation_traversal_refusals_reach_decode() {
    let bytes = text_annotation_file();
    for operation in ["iges annotation directory traversal", "iges annotation validation traversal"] {
        assert_section_refusal(&bytes, operation, ResourceDimension::WorkUnits);
    }
}

#[test]
fn shared_note_and_leader_validation_stays_linear() {
    let count = 2_000;
    let mut note_entry = leader_entry(0);
    note_entry.entity_type = 212;
    note_entry.sequence = 1;
    note_entry.status = SourceStatus::from_codes([0, 1, 1, 0]);
    let mut leader = leader_entry(4);
    leader.sequence = 3;
    leader.status = SourceStatus::from_codes([0, 1, 1, 0]);
    let record = |sequence, values: Vec<TokenValue>| ParameterRecord::from_test_tokens(sequence, 1..2, Vec::new(), values.len(), values.into_iter().map(|value| Token { value, span: 0..0 }).collect(), Vec::new());
    let mut values = vec![TokenValue::Integer(212), TokenValue::Integer(count)];
    for _ in 0..count {
        values.extend([TokenValue::Integer(0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::Integer(1), TokenValue::real(std::f64::consts::FRAC_PI_2), TokenValue::real(0.0), TokenValue::Integer(0), TokenValue::Integer(0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::String(Vec::new())]);
    }
    let note = record(1, values);
    let leader_record = leader_record(0.0, 0.0);
    let label = record(5, vec![TokenValue::Integer(210), TokenValue::Integer(1), TokenValue::Integer(1), TokenValue::Integer(3)]);
    let flag = record(7, vec![TokenValue::Integer(208), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::Integer(1), TokenValue::Integer(1), TokenValue::Integer(3)]);
    let mut flag_entry = leader_entry(0);
    flag_entry.entity_type = 208;
    flag_entry.sequence = 7;
    let mut label_entry = leader_entry(0);
    label_entry.entity_type = 210;
    label_entry.sequence = 5;
    let entries = BTreeMap::from([(1, &note_entry), (3, &leader), (5, &label_entry)]);
    let records = BTreeMap::from([(1, &note), (3, &leader_record), (5, &label)]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 100_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut validation = super::AnnotationValidation::new(&ctx).unwrap();
    for _ in 0..count {
        assert!(flag_or_label_valid(&label_entry, &label, &entries, &records, GlobalTable::V5Later, &mut validation).unwrap());
        assert!(flag_or_label_valid(&flag_entry, &flag, &entries, &records, GlobalTable::V5Later, &mut validation).unwrap());
    }
    drop(validation);
    ctx.finish_session().unwrap();
}

#[test]
fn sectioned_area_shared_geometry_index_stays_linear() {
    const MAX_WORK_PER_BOUNDARY: u64 = 4_096;
    let mut ir = CadIr::empty();
    for sequence in (1..=2_000_u32).map(|index| index * 2 - 1) {
        ir.model.curves.push(Curve {
            id: CurveId::mint(format!("iges:model:curve#D{sequence}")).unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0), 1.0,
                ).unwrap(),
            )),
            source_object: None,
        });
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One shared model index and one single-node active set per boundary fit
    // this envelope, including the core tree-node movement charges.
    policy.limits.max_work_units = 2_000 * MAX_WORK_PER_BOUNDARY;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = None;
    let plane = (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0));
    for sequence in (1..=2_000_u32).map(|index| index * 2 - 1) {
        assert!(sectioned_area_curve_coplanar((&ir, &mut index), sequence, plane, 0.001, &ctx).unwrap());
    }
    drop(index);
    ctx.finish_session().unwrap();
}

#[test]
fn noncoplanar_section_boundary_skips_island_geometry_work() {
    let mut ir = CadIr::empty();
    for (sequence, z) in [(1, 1.0), (3, 0.0)] {
        ir.model.curves.push(Curve {
            id: CurveId::mint(format!("iges:model:curve#D{sequence}")).unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    Point3::new(0.0, 0.0, z), Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0), 1.0,
                ).unwrap(),
            )),
            source_object: None,
        });
    }
    let mut boundary = leader_entry(0);
    boundary.entity_type = 100;
    let mut island = leader_entry(0);
    island.entity_type = 100;
    island.sequence = 3;
    let entries = BTreeMap::from([(1, &boundary), (3, &island)]);
    let count = 20_000;
    let mut values = vec![TokenValue::Integer(230), TokenValue::Integer(1), TokenValue::Integer(0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::real(0.0), TokenValue::Integer(i64::try_from(count).unwrap())];
    values.resize(9 + count, TokenValue::Integer(3));
    let record = ParameterRecord::from_test_tokens(5, 1..2, Vec::new(), values.len(), values.into_iter().map(|value| Token { value, span: 0..0 }).collect(), Vec::new());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The island pointer checks fit; admitting an unvisited second pass does not.
    policy.limits.max_work_units = 30_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!sectioned_area_valid((&ir, &mut None), &record, &entries, 0, super::SectionedAreaContext {
        global_table: GlobalTable::V5Later, transform: Transform::identity(), length_factor: 1.0, resolution: 0.001,
    }, &ctx).unwrap());
    ctx.finish_session().unwrap();
}
