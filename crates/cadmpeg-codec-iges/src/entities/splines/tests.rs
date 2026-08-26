// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

fn type_112_parameters(
    continuity: i64,
    breakpoints: &[f64],
    segments: &[[f64; 12]],
    terminal: [f64; 12],
) -> String {
    let mut values = vec![
        "112".to_owned(),
        "3".to_owned(),
        continuity.to_string(),
        "3".to_owned(),
        segments.len().to_string(),
    ];
    values.extend(breakpoints.iter().map(ToString::to_string));
    values.extend(
        segments
            .iter()
            .flat_map(|segment| segment.iter())
            .map(ToString::to_string),
    );
    values.extend(terminal.into_iter().map(|value| value.to_string()));
    format!("{};", values.join(","))
}

#[test]
fn decode_refuses_a_parametric_spline_segment_count_over_its_projection_limit() {
    let error = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_curve_file_with_parameters(
                b"112,3,1,3,100001;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("iges_spline_segments")
                && limit.limit == 100_000
                && limit.used == 100_000
                && limit.additional == 1
    ));
}

#[test]
fn decode_refuses_a_parametric_spline_surface_over_its_pole_limit() {
    let error = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 114,
                form: 0,
                label: "SPLSURF".into(),
                status: "00000000",
                parameters: "114,3,1,1000,1000;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Codec("iges_spline_surface_poles")
                && limit.limit == 1_000_000
                && limit.used == 1_000_000
                && limit.additional == 8_006_001
    ));
}

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
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::SplineHeaderNotTransferred.kind()));
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
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::SplineHeaderNotTransferred.kind()));
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
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::SplineHeaderNotTransferred.kind()));
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
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::SplineHeaderNotTransferred.kind()));
}

#[test]
fn decode_uses_global_resolution_for_spline_position_continuity() {
    for (resolution, second_start, decoded) in [
        ("0.001", 1_000.000_999, true),
        ("0.001", 1_000.001_000_000_000_2, false),
        ("0", 1_000.0, true),
        ("0", 1_000.000_000_000_000_1, false),
    ] {
        let terminal_x = second_start + 1000.0;
        let parameters = type_112_parameters(
            0,
            &[0.0, 1000.0, 2000.0],
            &[
                [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                [
                    second_start,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                ],
            ],
            [
                terminal_x, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        );
        let result = IgesCodec
            .decode(
                &mut Cursor::new(parametric_spline_curve_file_with_parameters_and_resolution(
                    parameters.as_bytes(),
                    resolution,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(result.ir().model.curves.len(), usize::from(decoded));
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
                .count(),
            usize::from(!decoded)
        );
    }
}

#[test]
fn decode_type_112_h1_compares_unit_tangent_not_parameter_speed() {
    let first = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    for (second_slope, decoded) in [(2.0, true), (-2.0, false)] {
        let second = [
            1.0,
            second_slope,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        let parameters = type_112_parameters(
            1,
            &[0.0, 1.0, 2.0],
            &[first, second],
            [
                1.0 + second_slope,
                second_slope,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
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
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
                .count(),
            usize::from(!decoded)
        );
    }
}

#[test]
fn decode_type_112_h2_compares_curvature_with_arc_length_parameterization() {
    let first = [0.0, 1.0, 0.0, 0.0, 0.0, -2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    for (second_curvature_coefficient, decoded) in [(4.0, true), (5.0, false)] {
        let second = [
            1.0,
            2.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            second_curvature_coefficient,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        let parameters = type_112_parameters(
            2,
            &[0.0, 1.0, 2.0],
            &[first, second],
            [
                3.0,
                2.0,
                0.0,
                0.0,
                -1.0 + second_curvature_coefficient,
                2.0 * second_curvature_coefficient,
                second_curvature_coefficient,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
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
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
                .count(),
            usize::from(!decoded)
        );
    }
}

#[test]
fn decode_keeps_type_112_curve_when_redundant_terminal_block_disagrees() {
    let parameters = type_112_parameters(
        1,
        &[0.0, 1.0],
        &[[0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]],
        [99.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    );
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_curve_file_with_parameters(
                parameters.as_bytes(),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss
                .message
                .contains("terminal derivative block disagrees with the last polynomial")
    }));
}

#[test]
fn decode_rejects_a_degenerate_type_112_segment() {
    let parameters = type_112_parameters(0, &[0.0, 1.0], &[[0.0; 12]], [0.0; 12]);
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_curve_file_with_parameters(
                parameters.as_bytes(),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result.ir().model.curves.is_empty());
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
            .count(),
        1
    );
}
