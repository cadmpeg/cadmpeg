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
fn decode_converts_bicubic_power_patches_to_an_exact_nurbs_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir().model.surfaces[0].geometry
    else {
        panic!("expected a bicubic NURBS carrier");
    };
    assert_eq!((surface.u_degree, surface.v_degree), (3, 3));
    assert_eq!((surface.u_count, surface.v_count), (4, 4));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_converts_piecewise_power_splines_to_exact_cubic_nurbs() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir().model.curves[0].geometry
    else {
        panic!("expected a cubic NURBS carrier");
    };
    assert_eq!(nurbs.degree, 3);
    assert_eq!(
        nurbs.knots,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]
    );
    assert_eq!(nurbs.control_points.len(), 7);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            None,
            1.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.5, 0.0, 0.0))
    );
    assert_eq!(result.ir().model.edges[0].param_range, Some([0.0, 2.0]));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_converts_nonzero_cubic_power_terms_on_a_nonunit_interval() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nonlinear_parametric_spline_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let CurveGeometry::Nurbs(nurbs) = &result.ir().model.curves[0].geometry else {
        panic!("expected a cubic NURBS carrier");
    };
    let point = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        None,
        3.25,
    )
    .expect("converted curve evaluates");
    let expected = Point3::new(16.0, -1.546_875, 0.164_062_5);
    assert!(point.distance(expected) < 1.0e-12, "{point:?}");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_converts_nonzero_bicubic_cross_terms_on_nonunit_intervals() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nonlinear_parametric_spline_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let SurfaceGeometry::Nurbs(surface) = &result.ir().model.surfaces[0].geometry else {
        panic!("expected a bicubic NURBS carrier");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 1.5, -0.75),
        Some(Point3::new(95.496_093_75, 268.464_843_75, -95.496_093_75,))
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_propagates_declared_precision_through_parametric_spline_segments() {
    for (first_slope, second_start, terminal_x, decoded, terminal_loss) in [
        ("1.", "1000.009999", "2000.012", true, false),
        ("1.", "1000.010001", "2000.02", false, false),
        ("1.D0", "1000.004D0", "2000.004D0", false, false),
        ("1.", "1000.004", "2000.1", true, true),
    ] {
        let parameters = format!(
            "112,3,0,3,2,0,1000,2000,0,{first_slope},0,0,0,0,0,0,0,0,0,0,{second_start},1.,0,0,0,0,0,0,0,0,0,0,{terminal_x},1.,0,0,0,0,0,0,0,0,0,0;"
        );
        let result = IgesCodec
            .decode(
                &mut Cursor::new(parametric_spline_curve_file_with_parameters(
                    parameters.as_bytes(),
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(result.ir().model.curves.len(), usize::from(decoded));
        let has_terminal_loss = result.report().losses.iter().any(|loss| {
            loss.message
                .contains("terminal derivative block disagrees with the last polynomial")
        });
        assert_eq!(has_terminal_loss, terminal_loss);
        if !decoded {
            assert!(result.report().losses.iter().any(|loss| loss
                .message
                .contains("spline segments violate planar or positional continuity")));
        }
    }
}
