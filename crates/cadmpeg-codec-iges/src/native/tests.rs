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

#[test]
fn every_admitted_entity_form_routes_to_a_typed_decoder_or_native_retention_loss() {
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    let entities = matrix["entity"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entity| {
            let entity_type = entity["type"].as_integer().unwrap();
            let forms = entity["forms"].as_array().map_or_else(
                || vec![5001, 9999],
                |forms| {
                    forms
                        .iter()
                        .map(|form| form.as_integer().unwrap())
                        .collect()
                },
            );
            forms.into_iter().map(move |form| OwnedTestEntity {
                entity_type,
                form,
                label: format!("E{entity_type}"),
                status: "00000000",
                parameters: format!("{entity_type};"),
            })
        })
        .collect::<Vec<_>>();
    let bytes = owned_test_file(&entities);

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let generic_fallthroughs = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.message
                .ends_with("retained without neutral projection")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        generic_fallthroughs,
        vec![
            "IGES entity type 124 form 0 retained without neutral projection",
            "IGES entity type 124 form 1 retained without neutral projection",
            "IGES entity type 124 form 10 retained without neutral projection",
            "IGES entity type 124 form 11 retained without neutral projection",
            "IGES entity type 124 form 12 retained without neutral projection",
        ]
    );
}

#[test]
fn decode_bounds_declared_attribute_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 322,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000200",
        parameters: "322,,0,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    let definitions =
        &result.ir().native.namespace("iges").unwrap().arenas["attribute_table_definitions"];
    assert_eq!(definitions.len(), 1);
    assert!(definitions[0].fields()["attributes"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("attribute-table definition")));
}

#[test]
fn decode_stops_cursor_records_after_an_overlong_nested_count() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "FONTCNT".into(),
            status: "00000200",
            parameters: "310,1,1HA,0,1,2,65,0,0,99,66,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 302,
            form: 0,
            label: "CLASSCNT".into(),
            status: "00000200",
            parameters: "302,2,0,0,99,1,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 322,
            form: 1,
            label: "ATTRCNT".into(),
            status: "00000200",
            parameters: "322,4HATTR,0,2,1,1,99,2,3,1,42;".into(),
        },
        OwnedTestEntity {
            entity_type: 322,
            form: 2,
            label: "ATTRPAIR".into(),
            status: "00000200",
            parameters: "322,4HPAIR,0,1,1,1,2,10,0,20;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    let characters = native.arenas["text_fonts"][0].fields()["characters"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(characters.len(), 1);
    assert_eq!(characters[0]["declared_motion_count"], 99);
    assert!(characters[0]["motions"].as_array().unwrap().is_empty());

    let classes = native.arenas["associativities"][0].fields()["classes"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0]["declared_item_count"], 99);
    assert!(classes[0]["item_types"].as_array().unwrap().is_empty());

    let definitions = &native.arenas["attribute_table_definitions"];
    for definition in definitions {
        let attributes = definition.fields()["attributes"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(attributes.len(), 1);
        assert!(attributes[0]["values"].as_array().unwrap().is_empty());
    }
    assert_eq!(
        definitions[0].fields()["attributes"][0]["declared_value_count"],
        99
    );
    assert_eq!(
        definitions[1].fields()["attributes"][0]["declared_value_count"],
        2
    );
}

#[test]
fn decode_bounds_declared_brep_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 502,
        form: 1,
        label: "BADCOUNT".into(),
        status: "00010000",
        parameters: "502,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("vertex-list count")));
}

#[test]
fn decode_bounds_declared_trimming_counts_by_record_tokens() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BADCOUNT".into(),
            status: "00010000",
            parameters: "141,0,1,1,9223372036854775807;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("boundary segment count")));
}

#[test]
fn decode_bounds_declared_presentation_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 310,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000200",
        parameters: "310,1,1HA,,1,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("font header")));
    let fonts = &result.ir().native.namespace("iges").unwrap().arenas["text_fonts"];
    assert!(fonts[0].fields()["characters"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_bounds_declared_annotation_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 212,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00010100",
        parameters: "212,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("text count")));
    let annotations = &result.ir().native.namespace("iges").unwrap().arenas["annotations"];
    assert!(annotations[0].fields()["strings"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_bounds_declared_drawing_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 404,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000000",
        parameters: "404,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("drawing view placements")));
    let drawings = &result.ir().native.namespace("iges").unwrap().arenas["drawings"];
    assert!(drawings[0].fields()["views"].as_array().unwrap().is_empty());
}

#[test]
fn decode_bounds_declared_solid_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 180,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000000",
        parameters: "180,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("Boolean postfix length")));
    let trees = &result.ir().native.namespace("iges").unwrap().arenas["boolean_trees"];
    assert!(trees[0].fields()["terms"].as_array().unwrap().is_empty());
}

#[test]
fn decode_preserves_native_entities_and_graph() {
    let bytes = point_file();

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().source.as_ref().unwrap().format, "iges");
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["document_local_sha256"],
        crate::document_digest(result.ir())
    );
    assert_eq!(
        result
            .source_fidelity()
            .retained_record(crate::SOURCE_IMAGE_ID)
            .unwrap()
            .data
            .as_deref(),
        Some(bytes.as_slice())
    );
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.version, 2);
    assert_eq!(native.arenas["cards"].len(), 7);
    assert_eq!(native.arenas["entities"].len(), 1);
    assert!(native.arenas["colors"].is_empty());
    assert_eq!(native.arenas["display_attributes"].len(), 1);
    assert!(!native.arenas.contains_key("opaque_bytes"));
    assert_eq!(native.arenas["entities"][0].id(), "iges:entity:directory#1");
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 1.0);
    assert_eq!(result.ir().model.points[0].position.y, 2.0);
    assert_eq!(result.ir().model.points[0].position.z, 3.0);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert!(result.report().geometry_transferred);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message == "IGES entity type 116 form 0 retained without neutral projection"
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
