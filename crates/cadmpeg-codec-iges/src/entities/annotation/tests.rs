// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

use super::{
    fill_pattern_valid, fixed_or_variable_valid, justification_valid, mirror_flag_valid,
    vertical_text_flag_valid,
};

#[test]
fn decode_preserves_general_note_text_runs_and_new_note_control_codes() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_annotation_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas["annotations"];
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
fn decode_applies_new_general_note_defaults_with_positive_metrics() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(defaulted_new_general_note_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];
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
    let annotation = &result.ir().native.namespace("iges").unwrap().arenas["annotations"][0];
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
fn decode_rejects_negative_text_box_dimensions_at_cadir_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(negative_text_box_dimensions_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let annotations = &result.ir().native.namespace("iges").unwrap().arenas["annotations"];
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
        assert!(fill_pattern_valid(value), "admitted fill pattern {value}");
    }
    for value in [
        21, 23, 24, 25, 27, 30, 31, 33, 35, 37, 39, 43, 44, 45, 47, 48, 49, 51, 269,
    ] {
        assert!(!fill_pattern_valid(value), "reserved fill pattern {value}");
    }
}

#[test]
fn decode_types_every_leader_arrow_form_and_segment_chain() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(leader_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas["annotations"];
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
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas["annotations"];
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
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas["annotations"];
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
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas["annotations"];
    let symbol = annotations
        .iter()
        .find(|annotation| annotation.fields()["kind"] == "general_symbol")
        .unwrap();
    assert_eq!(symbol.fields()["note"], "iges:presentation:annotation#D1");
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
