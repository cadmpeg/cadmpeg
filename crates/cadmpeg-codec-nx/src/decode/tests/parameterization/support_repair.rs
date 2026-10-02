// SPDX-License-Identifier: Apache-2.0
//! Decode-owner unit tests.

use crate::decode::support_uv::{invalidate_inconsistent_support_uv, SerializedSupportUv};
use crate::test_support::test_streams::{
    prt_with_ext11_intersection,
    two_support_charted_intersection_curve_stream_with_second_plane_axis,
    two_support_ext11_charted_intersection_curve_stream,
};
use crate::NxCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{pcurve::PcurveGeometry, ProceduralCurveDefinition};
use cadmpeg_ir::math::Point2;
use cadmpeg_test_support::{edit, EditableDecodeResult};
use std::io::Cursor;

#[test]
fn analytic_uv_completion_replaces_a_sentinel_contaminated_support_lane() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let stream = two_support_ext11_charted_intersection_curve_stream(false);
        let partition =
            two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
        let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let mut result = EditableDecodeResult::from(result);
        let procedural_id = result.ir().model.procedural_curves[0].id.clone();
        {
            let mut ir = result.ir_mut();
            ir.model.procedural_curves[0].edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    panic!("typed intersection");
                };
                edit::replace(context, |previous| {
                    let mut sides = previous.sides().clone();
                    let range = previous.parameter_range().endpoints();
                    let discontinuities =
                        cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
                    {
                        let context_sides: &mut [cadmpeg_ir::geometry::IntcurveSupportSide; 2] =
                            &mut sides;

                        let Some(support) = (*context_sides)[0].pcurve.as_mut() else {
                            panic!("NURBS support lane");
                        };
                        let PcurveGeometry::Nurbs { nurbs } = &mut support.geometry else {
                            panic!("NURBS support lane");
                        };
                        nurbs
                            .try_map_control_points(|pole_index, point| {
                                if pole_index == 1 {
                                    cadmpeg_ir::units::FinitePoint2::new(Point2::new(
                                        crate::decode::MISSING_TOLERANCE,
                                        crate::decode::MISSING_TOLERANCE,
                                    ))
                                    .ok_or(())
                                } else {
                                    Ok(point)
                                }
                            }, &cadmpeg_test_support::service_decode_context()).expect("pole edit admission")
                            .unwrap();
                    };
                    cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                        sides,
                        range,
                        discontinuities,
                    )
                })
                .unwrap();
            });
        }
        let pending = vec![(
            procedural_id,
            crate::intersection::chart_samples::ChartSamples::from_test_values(
                vec![
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
                ],
                vec![0.0, 0.01],
            )
            .unwrap(),
            0.01,
            SerializedSupportUv::default(),
        )];

        crate::decode::support_uv::complete_support_uv(
            geometry_ctx,
            &mut result.ir_mut(),
            &pending,
        )
        .unwrap();

        let ProceduralCurveDefinition::Intersection { context, .. } =
            &result.ir().model.procedural_curves[0].definition()
        else {
            panic!("typed intersection");
        };
        let Some(support) = context.sides()[0].pcurve.as_ref() else {
            panic!("NURBS support lane");
        };
        let PcurveGeometry::Nurbs { nurbs } = &support.geometry else {
            panic!("NURBS support lane");
        };
        assert!(nurbs.control_points().iter().all(|point| {
            point.u.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
                && point.v.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
        }));
        assert!(
            cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
                .expect("resource allocation did not fail")
                .is_ok()
        );
    });
}

#[test]
fn analytic_uv_completion_replaces_a_finite_mismatched_support_lane() {
    crate::test_support::with_decode_context(|geometry_ctx| {
        let stream = two_support_ext11_charted_intersection_curve_stream(false);
        let partition =
            two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
        let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let mut result = EditableDecodeResult::from(result);
        let procedural_id = result.ir().model.procedural_curves[0].id.clone();
        {
            let mut ir = result.ir_mut();
            ir.model.procedural_curves[0].edit_definition(|definition| {
                let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                    panic!("typed intersection");
                };
                edit::replace(context, |previous| {
                    let mut sides = previous.sides().clone();
                    let range = previous.parameter_range().endpoints();
                    let discontinuities =
                        cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
                    {
                        let context_sides: &mut [cadmpeg_ir::geometry::IntcurveSupportSide; 2] =
                            &mut sides;

                        let Some(support) = (*context_sides)[0].pcurve.as_mut() else {
                            panic!("NURBS support lane");
                        };
                        let PcurveGeometry::Nurbs { nurbs } = &mut support.geometry else {
                            panic!("NURBS support lane");
                        };
                        nurbs
                            .try_map_control_points(|_, point| {
                                let point = point.get();
                                cadmpeg_ir::units::FinitePoint2::new(Point2::new(
                                    point.u + 100.0,
                                    point.v,
                                ))
                                .ok_or(())
                            }, &cadmpeg_test_support::service_decode_context()).expect("pole edit admission")
                            .unwrap();
                    };
                    cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                        sides,
                        range,
                        discontinuities,
                    )
                })
                .unwrap();
            });
        }
        let pending = vec![(
            procedural_id,
            crate::intersection::chart_samples::ChartSamples::from_test_values(
                vec![
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
                ],
                vec![0.0, 0.01],
            )
            .unwrap(),
            0.01,
            SerializedSupportUv::default(),
        )];

        invalidate_inconsistent_support_uv(geometry_ctx, &mut result.ir_mut(), &pending);
        crate::decode::support_uv::complete_support_uv(
            geometry_ctx,
            &mut result.ir_mut(),
            &pending,
        )
        .unwrap();

        assert!(
            cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
                .expect("resource allocation did not fail")
                .is_ok()
        );
    });
}
