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
fn decode_defaults_unused_uniform_offset_scalars_to_zero() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(uniform_offset_circle_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let offset = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D3")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Circle { radius, .. } = offset.geometry else {
        panic!("expected an exact circular offset carrier");
    };
    assert_eq!(radius, 1.5);
    let edge = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0 == "iges:model:edge#D3")
        .unwrap();
    assert_eq!(edge.param_range, Some([0.0, std::f64::consts::FRAC_PI_2]));
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_maps_absolute_arc_parameters_to_the_neutral_domain() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(offset_quarter_circle_with_absolute_native_parameters()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let edge = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0 == "iges:model:edge#D3")
        .expect("offset arc");
    assert_eq!(edge.param_range, Some([0.0, std::f64::consts::FRAC_PI_2]));
    let start = result
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.start)
        .and_then(|vertex| {
            result
                .ir()
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
        })
        .expect("offset start point");
    assert_eq!(start.position, cadmpeg_ir::math::Point3::new(0.0, 1.5, 0.0));
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_does_not_default_an_unused_offset_pointer() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(uniform_offset_circle_file_with_parameters(
                b"130,1,1,,,,0.5,,,,0,0,1,0,1.5707963267948966;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| curve.id.0 != "iges:model:curve#D3"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("DE2 is not explicit integer zero") }));
}

#[test]
fn decode_retains_an_offset_with_an_unsupported_base_parameter_mapping() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(offset_nurbs_base_without_exact_mapping_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| curve.id.0 != "iges:model:curve#D3"));
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["entities"].len(),
        2
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_applies_declared_real_significance_to_curve_offset_normals() {
    for (normal_z, decoded) in [
        (".9999995", true),
        (".99999949", false),
        (".9999999D0", false),
    ] {
        let parameters = format!("130,1,1,0,,,0.5,,,,0,0,{normal_z},0,1.5707963267948966;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(uniform_offset_circle_file_with_parameters(
                    parameters.as_bytes(),
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id.0 == "iges:model:curve#D3");
        assert_eq!(offset, decoded, "{normal_z}");
        if !decoded {
            assert!(result.report().losses.iter().any(|loss| loss
                .message
                .contains("offset plane normal is not a unit vector")));
        }
    }
}

#[test]
fn decode_solves_a_parameter_linear_line_offset() {
    for (basis_code, expected_basis) in [
        (1, cadmpeg_ir::geometry::CurveOffsetLawBasis::ArcLength),
        (2, cadmpeg_ir::geometry::CurveOffsetLawBasis::Parameter),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(linear_offset_line_file(basis_code)),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.0 == "iges:model:curve#D3")
            .unwrap();
        let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &offset.geometry else {
            panic!("expected an exact degree-one offset carrier");
        };
        assert_eq!(nurbs.knots, vec![0.0, 0.0, 10.0, 10.0]);
        assert_eq!(
            nurbs.control_points,
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 3.0, 0.0),
            ]
        );
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
            distance_law:
                Some(cadmpeg_ir::geometry::CurveOffsetDistanceLaw::Linear {
                    basis,
                    distances,
                    control_range,
                }),
            ..
        } = &result.ir().model.procedural_curves[0].definition
        else {
            panic!("expected a retained linear offset law");
        };
        assert_eq!(*basis, expected_basis);
        assert_eq!(*distances, [1.0, 3.0]);
        assert_eq!(*control_range, [0.0, 10.0]);
        assert!(result.report().losses.is_empty());
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_solves_a_polynomial_coordinate_function_offset() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(function_offset_line_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let offset = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &offset.geometry else {
        panic!("expected an exact function-offset carrier");
    };
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 10.0, 10.0]);
    assert_eq!(
        nurbs.control_points,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 3.0, 0.0),
        ]
    );
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
        distance_law:
            Some(cadmpeg_ir::geometry::CurveOffsetDistanceLaw::Coordinate {
                function,
                coordinate,
                basis,
                function_parameter_offset,
                function_parameter_scale,
            }),
        ..
    } = &result.ir().model.procedural_curves[0].definition
    else {
        panic!("expected a retained coordinate-function offset law");
    };
    assert_eq!(function.0, "iges:model:curve#D3");
    assert_eq!(*coordinate, 2);
    assert_eq!(*basis, cadmpeg_ir::geometry::CurveOffsetLawBasis::Parameter);
    assert_eq!(*function_parameter_offset, 0.0);
    assert_eq!(*function_parameter_scale, 0.1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
