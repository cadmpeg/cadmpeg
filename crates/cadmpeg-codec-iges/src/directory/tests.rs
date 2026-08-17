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

use super::Status;

#[test]
fn subordinate_switch_dependency_bits_follow_the_four_defined_values() {
    for (subordinate, physical, logical) in [
        (0, false, false),
        (1, true, false),
        (2, false, true),
        (3, true, true),
    ] {
        let status = Status {
            blank: 0,
            subordinate,
            use_flag: 0,
            hierarchy: 0,
        };
        assert_eq!(status.is_physically_dependent(), physical);
        assert_eq!(status.is_logically_dependent(), logical);
    }
}

#[test]
fn blank_directory_status_defaults_to_zero_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "BLANK".into(),
                status: "        ",
                parameters: "116,1,2,3,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn full_width_directory_status_supplies_zero_groups() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "STATUS".into(),
                status: "00000201",
                parameters: "116,1,2,3,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let entity = &result.ir().native.namespace("iges").unwrap().arenas["entities"][0];

    assert_eq!(entity.fields()["blank_status"], 0);
    assert_eq!(entity.fields()["subordinate_status"], 0);
    assert_eq!(entity.fields()["use_flag"], 2);
    assert_eq!(entity.fields()["hierarchy_status"], 1);
}

#[test]
fn directory_status_rejects_leading_embedded_or_trailing_blanks() {
    for status in ["     201", "0000 201", "0000020 "] {
        let error = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "STATUS".into(),
                    status,
                    parameters: "116,1,2,3,0;".into(),
                }])),
                &DecodeOptions::default(),
            )
            .unwrap_err();
        assert!(matches!(error, CodecError::Malformed(_)));
    }
}

#[test]
fn inspect_reports_directory_entity_and_form_census() {
    let bytes = point_file();

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"entities=1".into()));
    assert!(summary.notes.contains(&"entity.116.form.0=1".into()));
    assert!(summary.notes.contains(&"parameter_records=1".into()));
    assert!(summary.notes.contains(&"parameter_tokens=4".into()));
}

#[test]
fn decode_treats_subordinate_switch_three_as_physically_dependent() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report().geometry_transferred);
    assert!(result.report().losses.is_empty());
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas["directions"].len(), 1);
    let direction_fields = native.arenas["directions"][0].fields();
    let components = direction_fields["components"].as_array().unwrap();
    assert_eq!(components[0], 2.0);
    assert_eq!(components[1], -3.0);
    assert_eq!(components[2], 4.0);
    assert_eq!(
        native.arenas["directions"][0].fields()["physically_dependent"],
        true
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
