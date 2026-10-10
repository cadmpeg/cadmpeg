// SPDX-License-Identifier: Apache-2.0
//! Polynomial pole backing remains scratch until a carrier survives admission.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

use super::super::{nurbs_curve, nurbs_surface, polyline};

const POLYLINE: &str = "#1=POLYLINE('',(#2,#3));";
const CURVE: &str = "#1=QUASI_UNIFORM_CURVE('',1,(#2,#3),.UNSPECIFIED.,.F.,.F.);";
const SURFACE: &str = "#1=QUASI_UNIFORM_SURFACE('',1,1,((#2,#3),(#4,#5)),.UNSPECIFIED.,.F.,.F.,.F.);";
const POLE_BYTES: usize = 4 * std::mem::size_of::<FinitePoint3>();
const KNOT_BYTES: usize = 4 * std::mem::size_of::<f64>();
const GRID_BYTES: usize = 2 * POLE_BYTES + 4 * std::mem::size_of::<Vec<FinitePoint3>>();

fn parse(source: &str) -> crate::parse::Exchange {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{source}#2=CARTESIAN_POINT('',(0.,0.,0.));#3=CARTESIAN_POINT('',(1.,0.,0.));#4=CARTESIAN_POINT('',(0.,1.,0.));#5=CARTESIAN_POINT('',(1.,1.,0.));#99=UNKNOWN_POINT();ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("polynomial fixture").0
}

fn points() -> BTreeMap<u64, FinitePoint3> {
    BTreeMap::from([
        (2, FinitePoint3::ZERO),
        (3, FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("finite pole")),
        (4, FinitePoint3::new(Point3::new(0.0, 1.0, 0.0)).expect("finite pole")),
        (5, FinitePoint3::new(Point3::new(1.0, 1.0, 0.0)).expect("finite pole")),
    ])
}

fn assert_rejected_curve_releases_poles(source: &str, is_polyline: bool) {
    let exchange = parse(source);
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::try_from(POLE_BYTES + KNOT_BYTES).expect("test storage fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");
    let mut losses = Vec::new();
    let record = &exchange.records()[&1];
    let result = if is_polyline {
        polyline(1, record, &points, (&mut losses, &mut loss_storage), &ctx)
    } else {
        nurbs_curve(1, record, &points, (&mut losses, &mut loss_storage), &ctx)
    }.expect("scratch admission");
    assert!(result.is_none());
    assert!(losses.is_empty());
    let reuse = ctx.reserve_scoped(u64::try_from(POLE_BYTES).expect("test storage fits u64"), "test rejected polynomial reuse")
        .expect("destroyed point backing was released");
    drop(reuse);
    drop(loss_storage);
    ctx.finish_session().expect("unrefused session");
}

#[test]
fn polynomial_polyline_missing_reference_releases_partial_poles() {
    assert_rejected_curve_releases_poles("#1=POLYLINE('',(#2,#99));", true);
}

#[test]
fn polynomial_polyline_short_lane_releases_partial_poles() {
    assert_rejected_curve_releases_poles("#1=POLYLINE('',(#2));", true);
}

#[test]
fn polynomial_nurbs_curve_missing_reference_releases_partial_poles() {
    assert_rejected_curve_releases_poles("#1=QUASI_UNIFORM_CURVE('',1,(#2,#99),.UNSPECIFIED.,.F.,.F.);", false);
}

#[test]
fn polynomial_nurbs_surface_missing_reference_releases_rows_and_partial_poles() {
    let exchange = parse("#1=QUASI_UNIFORM_SURFACE('',1,1,((#2,#3),(#4,#99)),.UNSPECIFIED.,.F.,.F.,.F.);");
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::try_from(GRID_BYTES).expect("test storage fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");
    let mut losses = Vec::new();
    assert!(nurbs_surface(1, &exchange.records()[&1], &points, (&mut losses, &mut loss_storage), &ctx)
        .expect("scratch admission").is_none());
    assert!(losses.is_empty());
    let reuse = ctx.reserve_scoped(u64::try_from(GRID_BYTES).expect("test storage fits u64"), "test rejected polynomial grid reuse")
        .expect("destroyed row and point backing was released");
    drop(reuse);
    drop(loss_storage);
    ctx.finish_session().expect("unrefused session");
}

fn assert_curve_output(curve: &NurbsCurve, points: &BTreeMap<u64, FinitePoint3>) {
    assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(curve.pole_rows().points(), [points[&2], points[&3]]);
    assert!(curve.pole_rows().weights().is_none());
}

fn assert_retained_curve(source: &str, is_polyline: bool) {
    let exchange = parse(source);
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let output_bytes = POLE_BYTES + KNOT_BYTES;
    policy.limits.max_retained_bytes = u64::try_from(output_bytes).expect("test storage fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");
    let mut losses = Vec::new();
    let record = &exchange.records()[&1];
    let curve = if is_polyline {
        polyline(1, record, &points, (&mut losses, &mut loss_storage), &ctx)
    } else {
        nurbs_curve(1, record, &points, (&mut losses, &mut loss_storage), &ctx)
    }.expect("retained admission").expect("polynomial curve");
    assert_curve_output(&curve, &points);
    assert!(losses.is_empty());
    let CodecError::ResourceLimit(first) = ctx.charge_retained(u64::MAX, "test polynomial live backing")
        .expect_err("observation exceeds retained limit") else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(first.used, u64::try_from(output_bytes).expect("test storage fits u64"));
    assert_eq!(first.additional, u64::MAX);
    drop(curve);
    drop(loss_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn polynomial_polyline_transfers_only_surviving_poles_and_knots() {
    assert_retained_curve(POLYLINE, true);
}

#[test]
fn polynomial_nurbs_curve_transfers_only_surviving_poles() {
    assert_retained_curve(CURVE, false);
}

#[test]
fn polynomial_nurbs_surface_transfers_surviving_rows_and_poles() {
    let exchange = parse(SURFACE);
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let output_bytes = GRID_BYTES + 2 * KNOT_BYTES;
    policy.limits.max_retained_bytes = u64::try_from(output_bytes).expect("test storage fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");
    let mut losses = Vec::new();
    let surface = nurbs_surface(1, &exchange.records()[&1], &points, (&mut losses, &mut loss_storage), &ctx)
        .expect("retained admission").expect("polynomial surface");
    assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_grid(), [vec![points[&2], points[&3]], vec![points[&4], points[&5]]]);
    assert!(surface.pole_grid().weights().is_none());
    assert!(losses.is_empty());
    let CodecError::ResourceLimit(first) = ctx.charge_retained(u64::MAX, "test polynomial grid live backing")
        .expect_err("observation exceeds retained limit") else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(first.used, u64::try_from(output_bytes).expect("test storage fits u64"));
    assert_eq!(first.additional, u64::MAX);
    drop(surface);
    drop(loss_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

fn assert_first_pole_refusal(source: &str, is_polyline: bool) {
    let exchange = parse(source);
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Curve knots remain in the candidate when the first pole is admitted.
    let knot_bytes = if is_polyline { 0 } else { KNOT_BYTES };
    policy.limits.max_materialized_bytes = u64::try_from(knot_bytes + POLE_BYTES - 1).expect("test storage fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");
    let mut losses = Vec::new();
    let record = &exchange.records()[&1];
    let result = if is_polyline {
        polyline(1, record, &points, (&mut losses, &mut loss_storage), &ctx)
    } else {
        nurbs_curve(1, record, &points, (&mut losses, &mut loss_storage), &ctx)
    };
    let CodecError::ResourceLimit(first) = result.expect_err("first pole backing exceeds cap")
        else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, if is_polyline { "step_polyline_points" } else { "step_nurbs_curve_control_points" });
    assert_eq!(first.used, u64::try_from(knot_bytes).expect("test storage fits u64"));
    assert_eq!(first.additional, u64::try_from(POLE_BYTES).expect("test storage fits u64"));
    assert!(losses.is_empty());
    assert!(matches!(ctx.charge_work(0, "later operation"), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    drop(loss_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn polynomial_polyline_preserves_first_pole_materialized_refusal() {
    assert_first_pole_refusal(POLYLINE, true);
}

#[test]
fn polynomial_nurbs_curve_preserves_first_pole_materialized_refusal() {
    assert_first_pole_refusal(CURVE, false);
}

#[test]
fn polynomial_nurbs_surface_preserves_first_pole_materialized_refusal() {
    let exchange = parse(SURFACE);
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(POLE_BYTES - 1).expect("test storage fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");
    let mut losses = Vec::new();
    let CodecError::ResourceLimit(first) = nurbs_surface(1, &exchange.records()[&1], &points, (&mut losses, &mut loss_storage), &ctx)
        .expect_err("first pole backing exceeds cap") else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "step_nurbs_surface_control_points");
    assert_eq!(first.used, 0);
    assert_eq!(first.additional, u64::try_from(POLE_BYTES).expect("test storage fits u64"));
    assert!(losses.is_empty());
    assert!(matches!(ctx.charge_work(0, "later operation"), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    drop(loss_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn polynomial_polyline_transfer_preserves_ambient_scoped_owner() {
    let exchange = parse(POLYLINE);
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let output_bytes = POLE_BYTES + KNOT_BYTES;
    policy.limits.max_materialized_bytes = u64::try_from(output_bytes).expect("test storage fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test polynomial output owner").expect("empty owner");
    let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");
    let mut losses = Vec::new();
    let curve = storage.with_storage(|| polyline(1, &exchange.records()[&1], &points, (&mut losses, &mut loss_storage), &ctx))
        .expect("ambient scope admission").expect("curve");
    assert_curve_output(&curve, &points);
    assert!(losses.is_empty());
    drop(curve);
    drop(storage);
    let reuse = ctx.reserve_scoped(u64::try_from(output_bytes).expect("test storage fits u64"), "test destroyed polynomial output reuse")
        .expect("ambient owner released destroyed output");
    drop(reuse);
    drop(loss_storage);
    ctx.finish_session().expect("unrefused session");
}
