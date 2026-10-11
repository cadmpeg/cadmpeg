// SPDX-License-Identifier: Apache-2.0
//! Parsed geometry and its storage have the same lifetime.

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ScopedReservation};
use crate::curves::DecodedGeometry;

fn reject_polyline(bytes: &[u8], offset: usize, message: &str) {
    for direct in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 64 * 1024;
        policy.limits.max_materialized_bytes = 64 * 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
        for _ in 0..8 {
            let error = if direct {
                let mut reader = BoundedReader::new(bytes, 0, bytes.len()).unwrap();
                super::super::read_polyline(&ctx, &mut reader, MillimeterScale::IDENTITY, None)
                    .expect_err("malformed polyline")
            } else {
                super::super::decode(&ctx, bytes, super::super::POLYLINE, 0..bytes.len(),
                    MillimeterScale::IDENTITY, ArchiveVersion::V8).expect_err("malformed polyline")
            };
            assert!(matches!(error,
                GeometryError::Malformed(FramingError::Structural { offset: actual_offset, message: actual_message })
                    if actual_offset == offset && actual_message == message));
        }
        ctx.charge_retained(policy.limits.max_retained_bytes, "test recovered retained reuse")
            .expect("failed geometry retains no backing");
        let scratch = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test recovered scratch reuse")
            .expect("failed geometry leaves no scratch");
        drop(scratch);
        ctx.finish_session().expect("recoverable geometry error");
    }
}

#[test]
fn polyline_parameter_count_rejection_releases_geometry() {
    let mut bytes = two_point_polyline_bytes();
    bytes[53..57].copy_from_slice(&0_i32.to_le_bytes());
    reject_polyline(&bytes, 57, "polyline parameter count mismatch");
}

#[test]
fn polyline_decreasing_parameters_release_geometry() {
    let mut bytes = two_point_polyline_bytes();
    bytes[65..73].copy_from_slice(&9.0_f64.to_le_bytes());
    reject_polyline(&bytes, 73, "polyline parameters are not increasing");
}

#[test]
fn polyline_invalid_dimension_releases_geometry() {
    let mut bytes = two_point_polyline_bytes();
    bytes[73..77].copy_from_slice(&1_i32.to_le_bytes());
    reject_polyline(&bytes, 77, "polyline dimension is invalid");
}

#[test]
fn polyline_success_retains_only_final_poles_and_knots() {
    let bytes = two_point_polyline_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let output_bytes = 2 * std::mem::size_of::<FinitePoint3>() + 4 * std::mem::size_of::<FiniteReal>();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(output_bytes);
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let decoded = super::super::decode(&ctx, &bytes, super::super::POLYLINE, 0..bytes.len(),
        MillimeterScale::IDENTITY, ArchiveVersion::V8).expect("exact final geometry storage");
    let DecodedGeometry::Curve { curve: DecodedCurve::Leaf { geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)), .. } } = decoded else {
        panic!("polyline NURBS carrier");
    };
    assert_eq!(curve.pole_count(), 2);
    assert_eq!(curve.knots().as_slice(), &[10.0, 10.0, 12.0, 12.0]);
    let scratch = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test polyline scratch reuse")
        .expect("parameter scratch was released");
    drop(scratch);
    ctx.finish_session().expect("exact retained storage");
}

#[test]
fn joined_segments_release_consumed_geometry_before_output_growth() {
    const SEGMENTS: usize = 128;
    let segments = (0..SEGMENTS).map(|index| {
        let start = cadmpeg_core::convert::f64_from_index(index).unwrap();
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(start, 0.0, 0.0), Point3::new(start + 1.0, 0.0, 0.0)],
            None, false).unwrap().unwrap()
    }).collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Elevation keeps the container and all completed curves, plus one span's scratch.
    // The final point growth keeps one curve, 256 new slots and 128 overlap slots.
    let segment_bytes = 2 * std::mem::size_of::<FinitePoint3>() + 4 * std::mem::size_of::<FiniteReal>();
    let container_bytes = SEGMENTS * std::mem::size_of::<(NurbsCurve, ScopedReservation<'_>)>();
    let elevation_peak = container_bytes + SEGMENTS * segment_bytes + 512;
    let joining_peak = container_bytes + segment_bytes
        + 3 * SEGMENTS * std::mem::size_of::<Point3>()
        + 2 * SEGMENTS * std::mem::size_of::<f64>();
    let peak = elevation_peak.max(joining_peak);
    let unreleased_peak = SEGMENTS * (std::mem::size_of::<NurbsCurve>() + segment_bytes)
        + 3 * SEGMENTS * std::mem::size_of::<Point3>();
    assert!(peak < unreleased_peak, "cap excludes the consumed curves' stale backing");
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(peak);
    policy.limits.max_retained_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let joined = super::super::join_nurbs_curves(&ctx, None, segments, 0, None, None)
        .expect("output growth reuses each consumed curve's storage");
    assert_eq!(joined.degree(), 1);
    assert_eq!(joined.pole_count(), SEGMENTS + 1);
    assert_eq!(joined.knots().len(), SEGMENTS + 3);
    assert_eq!(joined.knots()[0], 0.0);
    assert_eq!(joined.knots()[SEGMENTS + 2], 128.0);
    for (index, point) in joined.control_points().iter().enumerate() {
        assert_eq!(point.get(), Point3::new(cadmpeg_core::convert::f64_from_index(index).unwrap(), 0.0, 0.0));
    }
    let (_, knots, poles, _) = joined.into_parts();
    let knots = knots.into_values();
    let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } = poles else {
        panic!("polynomial joined poles");
    };
    let retained = points.capacity() * std::mem::size_of::<FinitePoint3>()
        + knots.capacity() * std::mem::size_of::<f64>();
    ctx.charge_retained(policy.limits.max_retained_bytes - cadmpeg_core::decode::u64_from_index(retained),
        "test joined retained reuse").expect("only final geometry remains retained");
    let scratch = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test joined scratch reuse")
        .expect("all segment and join scratch was released");
    drop(scratch);
    ctx.finish_session().expect("join fits its live storage");
}

#[test]
fn rejected_join_releases_final_candidate_lanes() {
    let fixture = || NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, f64::MAX, f64::MAX],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None, false).unwrap().unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 64 * 1024;
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..8 {
        let error = super::super::join_nurbs_curves(&ctx, None, vec![fixture(), fixture()], 0, None, None)
            .expect_err("shifted second domain overflows the final knot lane");
        assert!(matches!(error, GeometryError::Malformed(_)));
        assert!(error.to_string().contains("non-finite"));
    }
    ctx.charge_retained(policy.limits.max_retained_bytes, "test rejected join retained reuse")
        .expect("failed join keeps no final lanes");
    let scratch = ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test rejected join scratch reuse")
        .expect("failed join releases inputs and intermediate geometry");
    drop(scratch);
    ctx.finish_session().expect("recoverable knot overflow");
}
