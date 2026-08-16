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
    depth_clipping_valid, display_flag_valid, has_in_plane_component, standard_color_valid,
    standard_line_font_valid,
};

#[test]
fn view_up_component_test_is_scale_invariant() {
    assert!(has_in_plane_component(
        [0.0, 0.0, 1.0e-200],
        [0.0, 1.0e-200, 0.0]
    ));
    assert!(has_in_plane_component(
        [1.0e200, 0.0, 0.0],
        [1.0e200, 1.0e184, 0.0]
    ));
    assert!(!has_in_plane_component(
        [1.0e200, 0.0, 0.0],
        [1.0e200, 0.0, 0.0]
    ));
}

#[test]
fn drawing_enumerations_match_the_iges_tables() {
    for value in 0..=3 {
        assert!(depth_clipping_valid(value));
    }
    assert!(!depth_clipping_valid(-1));
    assert!(!depth_clipping_valid(4));

    for value in 0..=1 {
        assert!(display_flag_valid(value));
    }
    assert!(!display_flag_valid(-1));
    assert!(!display_flag_valid(2));

    for value in 1..=5 {
        assert!(standard_line_font_valid(value));
    }
    assert!(!standard_line_font_valid(0));
    assert!(!standard_line_font_valid(6));

    for value in 0..=8 {
        assert!(standard_color_valid(value));
    }
    assert!(!standard_color_valid(-1));
    assert!(!standard_color_valid(9));
}

#[test]
fn decode_rejects_file_duplicate_drawing_sheet_ids() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(duplicate_drawing_sheet_ids_file()),
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
    let tags = losses
        .iter()
        .map(|loss| {
            loss.provenance
                .as_ref()
                .and_then(|provenance| provenance.tag.as_deref())
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(tags, ["directory_entry:D3", "directory_entry:D9"]);
}

#[test]
fn decode_accepts_distinct_drawing_sheet_id_pairs() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(distinct_drawing_sheet_ids_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_drawing_sheet_id_referenced_by_two_drawings() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(shared_drawing_sheet_id_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1, "{:#?}", result.report().losses);
    assert_eq!(
        losses[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D3")
    );
}

#[test]
fn decode_types_orthographic_and_perspective_views() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let views = &result.ir().native.namespace("iges").unwrap().arenas["views"];
    assert_eq!(views.len(), 3);
    assert_eq!(views[0].fields()["projection"], "orthographic_parallel");
    assert!(views[0].fields()["scale"].is_null());
    assert_eq!(
        views[0].fields()["clipping_planes"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(views[1].fields()["projection"], "perspective");
    assert_eq!(views[1].fields()["view_plane_normal"][2], 1.0);
    assert_eq!(views[1].fields()["center_of_projection"][2], 10.0);
    assert_eq!(views[1].fields()["clipping_window"][0], -2.0);
    assert_eq!(views[1].fields()["depth_clipping"], 3);
    assert_eq!(views[2].fields()["view_plane_normal"][2], 1.0e-200);
    assert_eq!(views[2].fields()["view_up"][1], 1.0e-200);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_out_of_table_depth_clipping_indicator() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(out_of_table_depth_clipping_view_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_rejects_out_of_table_segmented_display_flag() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(out_of_table_segmented_display_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_applies_defaults_and_accepts_zero_text_box_dimensions() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(defaulted_text_and_view_fields_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_does_not_default_wrong_typed_view_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(malformed_view_parameter_type_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .count();
    assert_eq!(losses, 1, "{:#?}", result.report().losses);
}

#[test]
fn decode_types_view_visibility_and_display_overrides() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_visibility_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let visibility = &result.ir().native.namespace("iges").unwrap().arenas["view_visibility"];
    assert_eq!(visibility.len(), 2);
    assert_eq!(visibility[0].fields()["form"], 3);
    assert_eq!(
        visibility[0].fields()["displays"][0]["view"],
        "iges:presentation:view#D1"
    );
    assert!(visibility[0].fields()["displays"][0]["line_font"].is_null());
    assert_eq!(visibility[1].fields()["form"], 4);
    assert_eq!(visibility[1].fields()["displays"][0]["line_font"], 1);
    assert_eq!(visibility[1].fields()["displays"][0]["color"], 2);
    assert_eq!(visibility[1].fields()["displays"][0]["line_weight"], 3);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_ordered_segmented_view_display() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(segmented_view_visibility_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let segmented =
        &result.ir().native.namespace("iges").unwrap().arenas["segmented_visibility"][0];
    assert_eq!(segmented.fields()["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(segmented.fields()["blocks"][0]["breakpoint"], 0.5);
    assert_eq!(segmented.fields()["blocks"][0]["color"]["kind"], "omitted");
    assert_eq!(segmented.fields()["blocks"][1]["breakpoint"], 1.0);
    assert_eq!(segmented.fields()["blocks"][1]["color"]["value"], 2);
    assert_eq!(segmented.fields()["blocks"][1]["line_font"]["value"], 3);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_types_drawing_view_placement_annotations_and_sheet_properties() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(drawing_with_properties_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let drawing = &result.ir().native.namespace("iges").unwrap().arenas["drawings"][0];
    assert_eq!(drawing.fields()["form"], 1);
    assert_eq!(
        drawing.fields()["views"][0]["view"],
        "iges:presentation:view#D1"
    );
    assert_eq!(drawing.fields()["views"][0]["origin"][0], 10.0);
    assert_eq!(drawing.fields()["views"][0]["rotation"], 0.5);
    assert_eq!(
        drawing.fields()["annotations"][0],
        "iges:entity:directory#3"
    );
    assert_eq!(drawing.fields()["size"][0], 210.0);
    assert_eq!(drawing.fields()["size"][1], 297.0);
    assert_eq!(drawing.fields()["units_flag"], 2);
    assert_eq!(drawing.fields()["units_name"][0], 77);
    assert_eq!(drawing.fields()["name"][0], 68);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_reports_conflicting_drawing_property_values() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(drawing_with_conflicting_size_properties_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let drawing = &result.ir().native.namespace("iges").unwrap().arenas["drawings"][0];

    assert!(drawing.fields()["size"].is_null());
    assert_eq!(drawing.fields()["ambiguous_property_forms"][0], 16);
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::DrawingPropertyAmbiguous.kind())
        .expect("ambiguous drawing property loss");
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D13")
    );
}

#[test]
fn decode_types_view_list_with_required_back_pointers() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file(true)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let view_list = result.ir().native.namespace("iges").unwrap().arenas["associativities"]
        .iter()
        .find(|value| value.fields()["kind"] == "view_list")
        .unwrap();
    assert_eq!(view_list.fields()["declared_visible_count"], 1);
    assert_eq!(view_list.fields()["view"], "iges:entity:directory#1");
    assert_eq!(
        view_list.fields()["visible_entities"][0],
        "iges:entity:directory#5"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );

    let missing = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(missing.report().losses.iter().any(|loss| {
        loss.message.contains("entity type 402 form 6")
            && loss.message.contains("predefined associativity")
    }));
}
