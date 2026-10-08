// SPDX-License-Identifier: Apache-2.0
//! Intersection construction releases rejected children and temporary controls.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

use super::{solved_curve, Chart};

fn chart() -> Chart {
    Chart {
        endpoints: [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        interior_points: Vec::new(),
        base_parameter: 0.0,
        base_scale: 1.0,
        chordal_error: 0.01,
    }
}

#[test]
fn repeated_chart_point_releases_rejected_parameter_storage() {
    let mut chart = chart();
    // Bare chart framing permits a repeated interior point when endpoints differ.
    chart.interior_points.push(chart.endpoints[0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut refusals = crate::lane_refusal::LaneRefusals::new();
    assert!(solved_curve(&ctx, &chart, chart.endpoints[0], chart.endpoints[1], 9, &mut refusals).unwrap().is_none());
    assert!(refusals.take_records().is_empty());
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released rejected chart parameters").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn overflowing_chart_parameters_retain_only_the_refusal_record() {
    let mut chart = chart();
    // The native scale is finite and nonzero; multiplying the endpoint distance
    // yields infinity. The two parameters are ordered but IR rejects the knots.
    chart.base_scale = f64::MAX;
    let message = "sldprt intersection chart curve for record attr 9: knots contains a non-finite value";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One exact formatted string and the initial four String slots in the sink.
    let retained = u64::try_from(message.len() + 4 * std::mem::size_of::<String>()).unwrap();
    policy.limits.max_retained_bytes = retained;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut refusals = crate::lane_refusal::LaneRefusals::new();
    assert!(solved_curve(&ctx, &chart, chart.endpoints[0], chart.endpoints[1], 9, &mut refusals).unwrap().is_none());
    assert_eq!(refusals.take_records(), [message]);
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released rejected intersection curve").unwrap());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test intersection loss-only boundary") else {
        panic!("exact refusal-record retained boundary");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional), (retained, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn accepted_chart_retains_only_parameters_knots_and_converted_poles() {
    let chart = chart();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Exact two-parameter and four-knot arrays, then two finite converted poles.
    let retained = u64::try_from(2 * 8 + 4 * 8 + 2 * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>()).unwrap();
    policy.limits.max_retained_bytes = retained;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut refusals = crate::lane_refusal::LaneRefusals::new();
    let (geometry, parameters, reversed) = solved_curve(&ctx, &chart, chart.endpoints[0], chart.endpoints[1], 9, &mut refusals).unwrap().unwrap();
    assert_eq!(parameters, [0.0, 2.0]);
    assert!(!reversed);
    let Some(SolvedCurveGeometry::Nurbs(curve)) = geometry.solved() else {
        panic!("intersection spline geometry");
    };
    assert_eq!(curve.degree(), 1);
    assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 2.0, 2.0]);
    assert_eq!(curve.control_points().len(), 2);
    assert_eq!(curve.control_points()[1].get(), cadmpeg_ir::math::Point3::new(2000.0, 0.0, 0.0));
    assert!(refusals.take_records().is_empty());
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released temporary intersection controls").unwrap());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test intersection output-only boundary") else {
        panic!("exact retained output boundary");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional), (retained, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn intersection_construction_preserves_the_original_ir_work_refusal() {
    let chart = chart();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "IR NURBS admitted poles", Some(1));
    let mut refusals = crate::lane_refusal::LaneRefusals::new();
    let Err(CodecError::ResourceLimit(limit)) = solved_curve(&ctx, &chart, chart.endpoints[0], chart.endpoints[1], 9, &mut refusals) else {
        panic!("original IR work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "IR NURBS admitted poles");
    assert_eq!(limit.additional, 1);
    assert!(refusals.take_records().is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.charge_work(1, "later operation"), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
