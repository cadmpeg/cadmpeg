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
