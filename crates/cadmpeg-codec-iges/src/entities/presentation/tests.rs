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

use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

use super::{general_note_font_valid, mirror_flag_valid, standard_color, vertical_text_flag_valid};

#[test]
fn presentation_enumerations_match_the_iges_tables() {
    let entries = BTreeMap::new();
    for value in [
        0, 1, 2, 3, 6, 12, 13, 14, 17, 18, 19, 1001, 1002, 1003, 2001, 3001,
    ] {
        assert!(
            general_note_font_valid(value, &entries),
            "font code {value}"
        );
    }
    for value in [-1, 4, 5, 7, 1000, 3002] {
        assert!(
            !general_note_font_valid(value, &entries),
            "font code {value}"
        );
    }

    for value in 0..=2 {
        assert!(mirror_flag_valid(value));
    }
    assert!(!mirror_flag_valid(-1));
    assert!(!mirror_flag_valid(3));

    for value in 0..=1 {
        assert!(vertical_text_flag_valid(value));
    }
    assert!(!vertical_text_flag_valid(-1));
    assert!(!vertical_text_flag_valid(2));

    assert!(standard_color(1).is_some());
    assert!(standard_color(8).is_some());
    assert!(standard_color(0).is_none());
    assert!(standard_color(9).is_none());
}

#[test]
fn decode_applies_standard_body_color_and_face_color_override() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(colored_explicit_vertex_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = result
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D11")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(
        body.color,
        Some(cadmpeg_ir::topology::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        })
    );
    assert_eq!(body.visible, Some(true));
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D11:D9")
        .unwrap();
    assert_eq!(
        face.color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.2,
            g: 0.4,
            b: 0.6,
            a: 1.0,
        })
    );
    assert!(result
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id.0 == "iges:appearance:color#D13"
            && appearance.name.as_deref() == Some("custom")));
    assert_eq!(result.ir().model.appearance_bindings.len(), 2);
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.version, 2);
    assert_eq!(native.arenas["colors"].len(), 1);
    assert_eq!(
        native.arenas["colors"][0].id(),
        "iges:presentation:color#D13"
    );
    assert_eq!(native.arenas["colors"][0].fields()["red_percent"], 20.0);
    assert_eq!(native.arenas["display_attributes"].len(), 7);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_types_template_and_visible_blank_line_fonts() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(line_font_definitions_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let line_fonts = &native.arenas["line_fonts"];
    assert_eq!(line_fonts.len(), 2);
    assert_eq!(line_fonts[0].id(), "iges:presentation:line-font#D3");
    assert_eq!(line_fonts[0].fields()["kind"], "template");
    assert_eq!(line_fonts[0].fields()["tangent_oriented"], true);
    assert_eq!(
        line_fonts[0].fields()["template"],
        "iges:entity:directory#1"
    );
    assert_eq!(line_fonts[1].fields()["kind"], "visible_blank_pattern");
    assert_eq!(line_fonts[1].fields()["segment_count"], 5);
    assert_eq!(
        line_fonts[1].fields()["hexadecimal_pattern"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![49, 54]
    );
    let line_display = native.arenas["display_attributes"]
        .iter()
        .find(|record| record.id() == "iges:presentation:display-attributes#D7")
        .unwrap();
    assert_eq!(line_display.fields()["line_font_number"], -5);
    assert_eq!(
        line_display.fields()["line_font_definition"],
        "iges:presentation:line-font#D5"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_out_of_table_text_template_font() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(out_of_table_text_template_font_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::DisplayDataNotProjected.kind()));
}

#[test]
fn decode_types_definition_levels_and_directory_level_links() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(definition_levels_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let levels = &native.arenas["definition_levels"];
    assert_eq!(levels.len(), 1);
    assert_eq!(levels[0].id(), "iges:presentation:definition-levels#D1");
    assert_eq!(levels[0].fields()["declared_count"], 3);
    assert_eq!(
        levels[0].fields()["levels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![2, 7, 11]
    );
    let line = native.arenas["display_attributes"]
        .iter()
        .find(|record| record.id() == "iges:presentation:display-attributes#D3")
        .unwrap();
    assert_eq!(line.fields()["level_number"], -1);
    assert_eq!(
        line.fields()["level_definition"],
        "iges:presentation:definition-levels#D1"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_resolves_directory_line_weight_to_millimetres() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(weighted_line_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let display = &result.ir().native.namespace("iges").unwrap().arenas["display_attributes"][0];
    assert_eq!(display.fields()["line_weight_number"], 1);
    assert_eq!(display.fields()["line_weight_mm"], 1.0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_distinguishes_absolute_and_incremental_text_templates() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_display_template_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let templates = &result.ir().native.namespace("iges").unwrap().arenas["text_templates"];
    assert_eq!(templates.len(), 2);
    let absolute = templates
        .iter()
        .find(|template| template.fields()["form"] == 0)
        .unwrap();
    assert_eq!(absolute.fields()["origin_or_increment"][0], 10.0);
    assert_eq!(absolute.fields()["origin_or_increment"][1], 20.0);
    let incremental = templates
        .iter()
        .find(|template| template.fields()["form"] == 1)
        .unwrap();
    assert_eq!(incremental.fields()["font_code"], 18);
    assert_eq!(incremental.fields()["mirror"], 1);
    assert_eq!(incremental.fields()["vertical"], 1);
    assert_eq!(incremental.fields()["origin_or_increment"][0], 2.0);
    assert_eq!(incremental.fields()["origin_or_increment"][1], -1.0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_text_font_glyphs_and_supersession() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_font_definition_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let fonts = &result.ir().native.namespace("iges").unwrap().arenas["text_fonts"];
    assert_eq!(fonts.len(), 2);
    let base = fonts
        .iter()
        .find(|font| font.fields()["font_code"] == 101)
        .unwrap();
    assert_eq!(base.fields()["characters"].as_array().unwrap().len(), 2);
    assert_eq!(base.fields()["characters"][0]["character_code"], 65);
    assert_eq!(
        base.fields()["characters"][0]["motions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(base.fields()["characters"][0]["motions"][0]["pen_up"].is_null());
    assert_eq!(
        base.fields()["characters"][0]["motions"][1]["pen_up"],
        false
    );
    assert_eq!(base.fields()["characters"][1]["declared_motion_count"], 0);
    let modification = fonts
        .iter()
        .find(|font| font.fields()["font_code"] == 102)
        .unwrap();
    assert_eq!(
        modification.fields()["supersedes_definition"],
        "iges:presentation:text-font#D1"
    );
    assert_eq!(
        modification.fields()["characters"][0]["motions"][0]["pen_up"],
        true
    );
    let template = &result.ir().native.namespace("iges").unwrap().arenas["text_templates"][0];
    assert_eq!(
        template.fields()["font_definition"],
        "iges:presentation:text-font#D3"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}
