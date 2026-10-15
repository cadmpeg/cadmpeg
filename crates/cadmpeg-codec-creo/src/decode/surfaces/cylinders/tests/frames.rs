// SPDX-License-Identifier: Apache-2.0

use super::*;

fn axial_interval_candidate(origin: [f64; 3]) -> crate::surface::PositionalCylinderFrame {
    crate::surface::PositionalCylinderFrame::new(
        origin,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        4.0,
        Some(6.0),
    )
    .expect("valid positional cylinder frame")
}

#[test]
fn axial_interval_corner_frame_requires_a_unique_tangent_maximum() {
    let candidates = [
        axial_interval_candidate([10.0, 7.0, 9.0]),
        axial_interval_candidate([10.0, 7.0, 5.0]),
        axial_interval_candidate([10.0, 3.0, 5.0]),
        axial_interval_candidate([10.0, 3.0, 9.0]),
    ];
    let y_support = PlaneEquation {
        origin: [0.0, 3.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let z_support = PlaneEquation {
        origin: [0.0, 0.0, 5.0],
        normal: [0.0, 0.0, 1.0],
    };
    let cap = PlaneEquation {
        origin: [10.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    };

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            unique_tangent_axial_interval_corner_frame(
                ctx,
                &candidates,
                &[y_support, z_support, cap],
            )
        })
        .expect("service corner search admitted"),
        Some(candidates[0])
    );
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        unique_tangent_axial_interval_corner_frame(ctx, &candidates, &[y_support])
    })
    .expect("service incomplete corner search admitted")
    .is_none());
}

#[test]
fn support_tangent_frame_selects_the_uniquely_witnessed_origin_sign() {
    let stored = crate::surface::PositionalCylinderFrame::new(
        [-29.8, 5.25, 6.76],
        [1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        0.25,
        None,
    )
    .expect("valid positional cylinder frame");
    let tangent = PlaneEquation {
        origin: [0.0, -5.5, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let unrelated_parallel = PlaneEquation {
        origin: [0.0, 6.51, 0.0],
        normal: [0.0, 1.0, 0.0],
    };

    let selected = crate::decode::with_test_decode_ctx(|ctx| {
        unique_support_tangent_cylinder_frame(ctx, stored, &[tangent, unrelated_parallel])
    })
    .expect("service tangent search admitted")
    .expect("unique tangent origin");
    assert_eq!(selected.frame().origin(), [-29.8, -5.25, 6.76]);
}

#[test]
fn support_tangent_frame_requires_a_matching_axis_aligned_support() {
    let stored = axial_interval_candidate([10.0, 7.0, 9.0]);
    let unmatched = PlaneEquation {
        origin: [0.0, 20.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let oblique = PlaneEquation {
        origin: [0.0, 3.0, 0.0],
        normal: [0.0, 1.0, 1.0],
    };

    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        unique_support_tangent_cylinder_frame(ctx, stored, &[unmatched])
    })
    .expect("service unmatched search admitted")
    .is_none());
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        unique_support_tangent_cylinder_frame(ctx, stored, &[oblique])
    })
    .expect("service oblique search admitted")
    .is_none());
}

fn support_tangent_limit_error(operation: &'static str) -> cadmpeg_core::CodecError {
    let stored = crate::surface::PositionalCylinderFrame::new(
        [-29.8, 5.25, 6.76],
        [1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        0.25,
        None,
    )
    .expect("valid positional cylinder frame");
    let tangent = PlaneEquation {
        origin: [0.0, -5.5, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| unique_support_tangent_cylinder_frame(ctx, stored, &[tangent]),
    )
}

#[test]
fn support_tangent_initial_origin_refuses_collection_limit() {
    let error = support_tangent_limit_error("creo support tangent initial origins");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo support tangent initial origins")
    );
}

#[test]
fn support_tangent_witness_plane_refuses_collection_limit() {
    let error = support_tangent_limit_error("creo support tangent witness planes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo support tangent witness planes")
    );
}

#[test]
fn support_tangent_next_origin_refuses_collection_limit() {
    let error = support_tangent_limit_error("creo support tangent next origins");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo support tangent next origins")
    );
}

#[test]
fn round_edge_support_frame_selects_one_offset_line() {
    let frame = crate::decode::with_test_decode_ctx(|ctx| {
        super::super::round_edge_cylinder_frame(
            ctx,
            crate::surface::Type24RoundEdgeEnvelope {
                parameter_interval: [0.25, 5.25],
                vertices: [[1.0, 0.2, 3.0], [1.2, 0.0, 8.0]],
                generated_entity_reference: None,
            },
            0.2,
            &[
                PlaneEquation {
                    origin: [1.0, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
                PlaneEquation {
                    origin: [0.0, 0.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                },
            ],
        )
    })
    .expect("service round-edge frame search admitted")
    .expect("one offset round-edge cylinder");

    assert_eq!(frame.frame().origin(), [1.2, 0.2, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
    assert_eq!(frame.radius().get(), 0.2);
    assert_eq!(frame.length().map(PositiveLength::get), Some(5.0));
}

#[test]
fn perpendicular_round_edge_supports_solve_their_radius() {
    let frame = crate::decode::with_test_decode_ctx(|ctx| {
        super::super::perpendicular_round_edge_cylinder_frame(
            ctx,
            crate::surface::Type24RoundEdgeEnvelope {
                parameter_interval: [0.25, 5.25],
                vertices: [[1.0, 0.2, 3.0], [1.2, 0.0, 8.0]],
                generated_entity_reference: None,
            },
            &[
                PlaneEquation {
                    origin: [1.0, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
                PlaneEquation {
                    origin: [0.0, 0.0, 0.0],
                    normal: [0.0, 1.0, 0.0],
                },
            ],
        )
    })
    .expect("service perpendicular round-edge frame search admitted")
    .expect("one endpoint-solved perpendicular round cylinder");

    assert!(frame
        .frame()
        .origin()
        .into_iter()
        .zip([1.2, 0.2, 0.0])
        .all(|(actual, expected)| (actual - expected).abs() < EPS_TEST_GEOMETRY));
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert!((frame.radius().get() - 0.2).abs() < EPS_TEST_GEOMETRY);
    assert_eq!(frame.length().map(PositiveLength::get), Some(5.0));
}

#[test]
fn round_edge_support_frame_rejects_parallel_supports() {
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::round_edge_cylinder_frame(
            ctx,
            crate::surface::Type24RoundEdgeEnvelope {
                parameter_interval: [0.0, 1.0],
                vertices: [[1.0, 0.2, 0.0], [1.0, 0.0, 1.0]],
                generated_entity_reference: Some(17),
            },
            0.2,
            &[
                PlaneEquation {
                    origin: [1.0, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
                PlaneEquation {
                    origin: [2.0, 0.0, 0.0],
                    normal: [1.0, 0.0, 0.0],
                },
            ],
        ))
        .expect("service parallel round-edge frame search admitted")
        .is_none()
    );
}

#[test]
fn round_envelope_rejects_an_extra_reference_circle() {
    let circle = |entity_id, axis, start: [f64; 3], end: [f64; 3]| {
        let mut center = start;
        let radial_lane = (0..3)
            .find(|lane| start[*lane] != end[*lane])
            .expect("distinct cap endpoints");
        center[radial_lane] = end[radial_lane];
        crate::reference::ReferenceCircle::try_new(
            entity_id,
            crate::reference::ReferenceCircleCenter::Stored(
                cadmpeg_ir::features::FinitePoint3::new(center.into())
                    .expect("finite circle center"),
            ),
            cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("positive radius"),
            cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::from(axis))
                .expect("unit axis"),
            [
                cadmpeg_ir::features::FinitePoint3::new(start.into()).expect("finite start"),
                cadmpeg_ir::features::FinitePoint3::new(end.into()).expect("finite end"),
            ],
            0,
        )
        .expect("checked reference geometry")
    };
    let envelope = crate::surface::Type24RoundEnvelope {
        diameter: 2.0,
        extent_endpoints: [[3.5, 8.0, -6.0], [5.5, 10.0, -4.0]],
    };
    let first = circle(367, [0.0, 0.0, 1.0], [3.5, 8.0, -6.0], [5.5, 10.0, -6.0]);
    let second = circle(368, [0.0, 0.0, -1.0], [5.5, 10.0, -4.0], [3.5, 8.0, -4.0]);
    let duplicate_first = circle(369, [0.0, 0.0, 1.0], [3.5, 8.0, -6.0], [5.5, 10.0, -6.0]);

    assert!(super::super::reference_cap_bound_round_frame(
        envelope,
        &[&first, &second, &duplicate_first]
    )
    .is_none());
}

mod admission_visits;
