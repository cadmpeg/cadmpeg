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
fn decode_refuses_a_copious_tuple_count_over_its_projection_limit() {
    let error = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(12, b"106,2,1000001;", "00000000")),
            &DecodeOptions::default(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("iges_copious_tuples")
                && limit.limit == 1_000_000
                && limit.used == 1_000_000
                && limit.additional == 1
    ));
}

#[test]
fn decode_projects_copious_linear_paths_with_segment_parameters() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                12,
                b"106,2,3,0,0,0,1,0,0,1,2,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(path) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a degree-one path carrier");
    };
    assert_eq!(path.degree, 1);
    assert_eq!(path.knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(1, &path.knots, &path.control_points, None, 1.5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0))
    );
    assert_eq!(result.ir().model.edges[0].param_range, Some([0.0, 2.0]));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_coincident_segments_in_a_copious_linear_path() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                12,
                b"106,2,4,0,0,0,0,0,0,1,0,0,1,1,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(path) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a degree-one path carrier");
    };
    assert_eq!(path.control_points[0], path.control_points[1]);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_closes_form_63_with_the_global_minimum_resolution() {
    for (gap, decoded) in [("0.000999", true), ("0.001001", false)] {
        let parameters = format!("106,1,3,0,0,0,1,0,0,{gap};");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(copious_data_file(63, parameters.as_bytes(), "00000000")),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(
            result.ir().model.curves.len(),
            usize::from(decoded),
            "{gap}"
        );
        if decoded {
            assert_eq!(result.ir().model.edges[0].tolerance, Some(0.001));
            assert_eq!(
                result.ir().model.edges[0].start,
                result.ir().model.edges[0].end
            );
            assert!(result.report().losses.is_empty());
            let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
            assert!(validation.is_ok(), "{:#?}", validation.findings);
        } else {
            assert!(result.report().losses.iter().any(|loss| loss
                .message
                .contains("endpoints disagree beyond the minimum resolution")));
        }
    }
}

#[test]
fn decode_rejects_a_copious_interpretation_that_disagrees_with_its_form() {
    let bytes = copious_data_file(11, b"106,2,2,0,0,0,1,0,0;", "00000000");
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.message
                .contains("interpretation flag disagrees with the entity form")
        })
        .expect("copious-data projection loss");
    let provenance = loss.provenance.as_ref().expect("Directory provenance");
    assert_eq!(provenance.format, "iges");
    assert_eq!(provenance.stream, "iges");
    assert_eq!(provenance.tag.as_deref(), Some("directory_entry:D1"));
    assert_eq!(bytes[provenance.offset as usize + 72], b'D');
    let transfer = &result.report().transfer_ledger.entries[0];
    assert_eq!(transfer.source, "D1");
    assert_eq!(
        transfer.note.as_deref(),
        Some("native record retained; semantic projection omitted with an attributed loss")
    );
}

#[test]
fn strict_decode_rejects_an_attributed_projection_loss() {
    let bytes = copious_data_file(11, b"106,2,2,0,0,0,1,0,0;", "00000000");
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &options)
        .unwrap_err();

    assert!(error.to_string().contains(&format!(
        "strict mode rejects {}",
        IgesLossCode::EntityNotProjected.kind()
    )));
    assert!(error
        .to_string()
        .contains("interpretation flag disagrees with the entity form"));
}

#[test]
fn decode_separates_copious_points_vectors_and_presentation_forms() {
    let points = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                3,
                b"106,3,2,1,2,3,0,0,1,4,5,6,1,0,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(points.ir().model.points.len(), 2);
    assert_eq!(points.ir().model.vertices.len(), 2);
    let native = points.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas["copious_data"].len(), 1);
    assert_eq!(
        native.arenas["copious_data"][0].fields()["tuples"][0][5],
        1.0
    );
    assert!(points.report().losses.is_empty());

    let witness = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(40, b"106,1,3,0,0,0,1,0,2,0;", "00000100")),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!witness.report().geometry_transferred);
    assert!(witness.ir().model.curves.is_empty());
    assert!(witness.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(witness.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
