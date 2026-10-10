// SPDX-License-Identifier: Apache-2.0

use super::super::parameter_slot;
use super::{checked_tabulated_cylinder_directrix, placed_tabulated_cylinder_directrix};
use crate::decode::sweep::nurbs::signed_unit_chart;
use cadmpeg_ir::math::Point3;

fn tabulated_directrix_limit_fixture() -> (
    crate::surface::TabulatedCylinderCurveReplay,
    crate::surface::SurfaceParameterRecord,
) {
    let replay = crate::surface::TabulatedCylinderCurveReplay {
        body: Vec::new(),
        surface_id: 7,
        curve_id: 9,
        curve_type: 0x13,
        flip: 1,
        tangent_condition: 0,
        degree: 3,
        parameter_body: Vec::new(),
        control_point_ids: [1, 2, 3, 4],
        successor_reference: 5,
        control_point_bodies: std::array::from_fn(|_| Vec::new().into()),
        control_points: [
            Some([1.0, 2.0]),
            Some([2.0, 2.5]),
            Some([3.0, 3.5]),
            Some([4.0, 4.0]),
        ],
        terminal_reference: 6,
        offset: 0,
        surface_row_offset: 0,
    };
    let parameters = crate::surface::SurfaceParameterRecord {
        surface_id: 7,
        body: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Tabulated {
                variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
                frame: crate::surface::TabulatedCylinderFrame::new(
                    [1.0, 2.0, 5.0, 4.0, 4.0, 10.0],
                    [0xa2, 0x42, 0x88, 0xa3, 0x18, 0x8a],
                )
                .expect("finite frame fixture"),
            },
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    (replay, parameters)
}

fn tabulated_directrix_limit_error(limit: u64) -> cadmpeg_core::CodecError {
    let (replay, parameters) = tabulated_directrix_limit_fixture();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    checked_tabulated_cylinder_directrix(
        &ctx,
        &replay,
        &parameters,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect_err("tabulated directrix exceeds collection limit")
}

#[test]
fn tabulated_directrix_controls_refuse_collection_limit() {
    assert!(matches!(
        tabulated_directrix_limit_error(3),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "creo tabulated-cylinder directrix controls"
    ));
}

#[test]
fn tabulated_directrix_knots_refuse_collection_limit() {
    assert!(matches!(
        tabulated_directrix_limit_error(11),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "creo tabulated-cylinder directrix knots"
    ));
}

#[test]
fn tabulated_directrix_constructor_refuses_typed_poles() {
    assert!(
        matches!(tabulated_directrix_limit_error(15), cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "IR NURBS admitted poles")
    );
    let (replay, parameters) = tabulated_directrix_limit_fixture();
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| checked_tabulated_cylinder_directrix(
            ctx,
            &replay,
            &parameters,
            None,
            &mut crate::lane_refusal::LaneRefusals::new()
        ))
        .expect("service directrix constructor")
        .is_some()
    );
}

#[test]
fn tabulated_cylinder_frame_places_a_unique_cubic_chart() {
    let mut replay = crate::surface::TabulatedCylinderCurveReplay {
        body: Vec::new(),
        surface_id: 7,
        curve_id: 9,
        curve_type: 0x13,
        flip: 1,
        tangent_condition: 0,
        degree: 3,
        parameter_body: vec![],
        control_point_ids: [1, 2, 3, 4],
        successor_reference: 5,
        control_point_bodies: std::array::from_fn(|_| vec![].into()),
        control_points: [
            Some([1.0, 2.0]),
            Some([2.0, 2.5]),
            Some([3.0, 3.5]),
            Some([4.0, 4.0]),
        ],
        terminal_reference: 6,
        offset: 0,
        surface_row_offset: 0,
    };
    let parameters = crate::surface::SurfaceParameterRecord {
        surface_id: 7,
        body: vec![],
        scalar_tokens: vec![],
        opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
            raw: vec![0x00, 0x0c, 0x9a].into(),
            offset: 3,
        }],
        scalar_frames: vec![
            crate::surface::SurfaceParameterScalarFrame {
                offset: 0,
                slots: [0.0, 0.0, 1.0].into_iter().map(parameter_slot).collect(),
            },
            crate::surface::SurfaceParameterScalarFrame {
                offset: 6,
                slots: [13.0, 22.0, 5.0, 10.0, 20.0, 10.0]
                    .into_iter()
                    .map(parameter_slot)
                    .collect(),
            },
        ],
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Extrusion(
                crate::surface::ExtrusionVariant::TabulatedCylinder,
            ),
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };

    let (curve, sweep) = placed_tabulated_cylinder_directrix(
        &replay,
        &parameters,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("placement");
    assert_eq!(curve.control_points()[0], Point3::new(-13.0, -20.0, 5.0));
    assert_eq!(curve.control_points()[3], Point3::new(-10.0, -22.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    let mut broad_signed_frame = parameters;
    broad_signed_frame.scalar_frames.truncate(1);
    broad_signed_frame.carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Tabulated {
            variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
            frame: crate::surface::TabulatedCylinderFrame::new(
                [1.0, 2.0, 5.0, 4.0, 4.0, 10.0],
                [0xa2, 0x42, 0x88, 0xa3, 0x18, 0x8a],
            )
            .expect("finite frame fixture"),
        },
    );
    let (curve, sweep) = placed_tabulated_cylinder_directrix(
        &replay,
        &broad_signed_frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("broad signed-DICT placement");
    assert_eq!(curve.control_points()[0], Point3::new(1.0, 2.0, 5.0));
    assert_eq!(curve.control_points()[3], Point3::new(4.0, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    broad_signed_frame.scalar_frames.clear();
    let (curve, sweep) = placed_tabulated_cylinder_directrix(
        &replay,
        &broad_signed_frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("complete frame supplies its signed sweep");
    assert_eq!(curve.control_points()[0], Point3::new(1.0, 2.0, 5.0));
    assert_eq!(curve.control_points()[3], Point3::new(4.0, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    broad_signed_frame.carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Tabulated {
            variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
            frame: crate::surface::TabulatedCylinderFrame::new(
                [1.0, 1.0, 2.0, 4.0, 4.0, 4.0],
                [0xa2, 0x42, 0x88, 0xa3, 0x18, 0x8a],
            )
            .expect("finite frame fixture"),
        },
    );
    assert!(placed_tabulated_cylinder_directrix(
        &replay,
        &broad_signed_frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .is_none());

    broad_signed_frame.carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Tabulated {
            variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
            frame: crate::surface::TabulatedCylinderFrame::new(
                [29.0, 5.0, 2.0, -26.0, 10.0, 4.0],
                [0x4a, 0x46, 0x2f, 0x46, 0x46, 0x2e],
            )
            .expect("finite frame fixture"),
        },
    );
    replay.control_points[1] = Some([10.0, -5.0]);
    assert!(
        placed_tabulated_cylinder_directrix(
            &replay,
            &broad_signed_frame,
            None,
            &mut crate::lane_refusal::LaneRefusals::new()
        )
        .is_none(),
        "the offset layout requires its prototype chart origin"
    );
    let (curve, sweep) = placed_tabulated_cylinder_directrix(
        &replay,
        &broad_signed_frame,
        Some([-30.0, 0.0, 0.0]),
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("independently signed offset placement");
    assert_eq!(curve.control_points()[0], Point3::new(-29.0, 5.0, 2.0));
    assert_eq!(curve.control_points()[1], Point3::new(-20.0, 5.0, -5.0));
    assert_eq!(curve.control_points()[3], Point3::new(-26.0, 5.0, 4.0));
    assert_eq!(sweep, [0.0, 5.0, 0.0]);

    broad_signed_frame.carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Tabulated {
            variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
            frame: crate::surface::TabulatedCylinderFrame::new(
                [1.0, 2.0, 5.0, 4.0, 4.0, 10.0],
                [0xdd, 0xa1, 0x9e, 0xd8, 0xa2, 0x9e],
            )
            .expect("finite frame fixture"),
        },
    );
    replay.control_points[1] = Some([2.0, 2.5]);
    let (curve, sweep) = placed_tabulated_cylinder_directrix(
        &replay,
        &broad_signed_frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("scalar encodings do not change the coordinate chart");
    assert_eq!(curve.control_points()[0], Point3::new(1.0, 2.0, 5.0));
    assert_eq!(curve.control_points()[3], Point3::new(4.0, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);

    broad_signed_frame.carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Tabulated {
            variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
            frame: crate::surface::TabulatedCylinderFrame::new(
                [1.0, 1.0, 2.0, 4.0, 4.0, 4.0],
                [0xdd, 0xa1, 0x9e, 0xd8, 0xa2, 0x9e],
            )
            .expect("finite frame fixture"),
        },
    );
    assert!(placed_tabulated_cylinder_directrix(
        &replay,
        &broad_signed_frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .is_none());

    replay.control_points = [
        Some([1.0, 2.0]),
        Some([2.0, 2.5]),
        Some([3.0, 3.5]),
        Some([4.0, 4.0]),
    ];
    broad_signed_frame.carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Tabulated {
            variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
            frame: crate::surface::TabulatedCylinderFrame::new(
                [-11.25, 2.0, 5.0, -8.25, 4.0, 10.0],
                [0x46, 0x46, 0x2f, 0x46, 0x46, 0x2e],
            )
            .expect("finite frame fixture"),
        },
    );
    let (curve, sweep) = placed_tabulated_cylinder_directrix(
        &replay,
        &broad_signed_frame,
        Some([-12.25, 0.0, 0.0]),
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("prototype chart origin supplies an arbitrary intercept");
    assert_eq!(curve.control_points()[0], Point3::new(-11.25, 2.0, 5.0));
    assert_eq!(curve.control_points()[3], Point3::new(-8.25, 4.0, 5.0));
    assert_eq!(sweep, [0.0, 0.0, 5.0]);
    assert!(placed_tabulated_cylinder_directrix(
        &replay,
        &broad_signed_frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .is_none());
}

#[test]
fn tabulated_cylinder_offset_chart_resolves_signed_unit_axes() {
    assert_eq!(
        signed_unit_chart(
            [33.480_874_469_5, 34.047_445_706_6],
            [3.480_874_469_5, 4.047_445_706_6],
            30.0,
        ),
        Some((1.0, -30.0))
    );
    assert_eq!(
        signed_unit_chart(
            [0.576_336_341_1, 0.746_308_064_9],
            [-0.746_308_064_9, -0.576_336_341_1],
            0.0,
        ),
        Some((-1.0, 0.0))
    );
    assert_eq!(
        signed_unit_chart(
            [21.592_186_587_7, 21.604_574_667_3],
            [8.407_813_412_3, -8.395_425_332_7],
            30.0,
        ),
        Some((1.0, -30.0))
    );
    assert_eq!(signed_unit_chart([1.0, 2.0], [4.0, 5.0], 30.0), None);
}

#[test]
fn zero_offset_2d_tabulated_frame_retains_the_stored_span() {
    let replay = crate::surface::TabulatedCylinderCurveReplay {
        body: Vec::new(),
        surface_id: 815,
        curve_id: 1,
        curve_type: 0x13,
        flip: 1,
        tangent_condition: 0,
        degree: 3,
        parameter_body: Vec::new(),
        control_point_ids: [1, 2, 3, 4],
        successor_reference: 0,
        control_point_bodies: std::array::from_fn(|_| Vec::new().into()),
        control_points: [
            Some([2.603_530_729_189_511_6, -6.634_758_301_120_719]),
            Some([2.486_761_892_214_414, -6.583_162_851_673_087]),
            Some([2.403_937_662_020_322, -6.519_347_555_976_829]),
            Some([2.355_057_866_495_792, -6.440_596_814_034_794]),
        ],
        terminal_reference: 0,
        offset: 0,
        surface_row_offset: 0,
    };
    let body = vec![
        0x18, 0xe4, 0x0f, 0x00, 0x0c, 0x9a, 0x8d, 0xd7, 0x28, 0x94, 0x26, 0x4b, 0xb2, 0x2d, 0x19,
        0xc3, 0x2b, 0xcf, 0xac, 0x01, 0x44, 0x9e, 0x1e, 0xb8, 0x51, 0xeb, 0x85, 0x1f, 0x8f, 0xd4,
        0x07, 0xeb, 0x3f, 0xff, 0xf8, 0x2d, 0x1a, 0x89, 0xfe, 0x14, 0x80, 0xb6, 0x48, 0x9e, 0x85,
        0x1e, 0xb8, 0x51, 0xeb, 0x85,
    ];
    let tabulated_cylinder_frame = crate::surface::decode_tabulated_cylinder_frame(
        &body,
        &crate::scalar::ScalarCache::default(),
    )
    .map(|(frame, _)| frame);
    let parameters = crate::surface::SurfaceParameterRecord {
        surface_id: 815,
        body,
        scalar_tokens: Vec::new(),
        opaque_spans: vec![crate::surface::SurfaceParameterOpaqueSpan {
            raw: vec![0, 0x0c, 0x9a].into(),
            offset: 3,
        }],
        scalar_frames: vec![crate::surface::SurfaceParameterScalarFrame {
            offset: 0,
            slots: vec![
                parameter_slot(0.0),
                parameter_slot(1.0),
                parameter_slot(0.0),
            ],
        }],
        carrier: tabulated_cylinder_frame.map_or(
            crate::surface::SurfaceParameterCarrier::Unresolved(
                crate::surface::SurfaceKind::Extrusion(
                    crate::surface::ExtrusionVariant::TabulatedCylinder,
                ),
            ),
            |frame| {
                crate::surface::SurfaceParameterCarrier::Resolved(
                    crate::surface::InlineSurfaceCarrier::Tabulated {
                        variant: crate::surface::ExtrusionVariant::TabulatedCylinder,
                        frame,
                    },
                )
            },
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let (curve, sweep) = placed_tabulated_cylinder_directrix(
        &replay,
        &parameters,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("zero-offset directrix placement");
    assert_eq!(
        curve.control_points()[0],
        Point3::new(-2.603_530_729_189_511_6, 6.634_758_301_120_719, 4.78)
    );
    assert_eq!(
        curve.control_points()[3],
        Point3::new(-2.355_057_866_495_792, 6.440_596_814_034_794, 4.78)
    );
    assert_eq!(sweep, [0.0, 0.0, 0.099_999_999_999_999_64]);
}
