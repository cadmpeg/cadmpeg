// SPDX-License-Identifier: Apache-2.0
//! Raw pcurve pole storage ends after finite pole construction.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::units::FinitePoint2;

use super::super::{nurbs_pcurve, polyline_pcurve};

const POLYLINE: &str = "#1=POLYLINE('',(#2,#3));";
const POLYNOMIAL: &str = "#1=QUASI_UNIFORM_CURVE('',1,(#2,#3),.UNSPECIFIED.,.F.,.F.);";
const RATIONAL: &str = "#1=(B_SPLINE_CURVE(1,(#2,#3),.UNSPECIFIED.,.F.,.F.) QUASI_UNIFORM_CURVE() RATIONAL_B_SPLINE_CURVE((1.,.5)));";

fn parse(source: &str) -> crate::parse::Exchange {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{source}#2=CARTESIAN_POINT('',(0.,0.));#3=CARTESIAN_POINT('',(1.,2.));ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("pcurve fixture")
        .0
}

fn points() -> BTreeMap<u64, Point2> {
    BTreeMap::from([(2, Point2::new(0.0, 0.0)), (3, Point2::new(1.0, 2.0))])
}

fn assert_reusable_raw_storage(source: &str, polyline: bool) {
    let exchange = parse(source);
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let output_bytes = 4 * std::mem::size_of::<f64>() + 2 * std::mem::size_of::<FinitePoint2>();
    let raw_bytes = 4 * std::mem::size_of::<Point2>();
    policy.limits.max_materialized_bytes = (output_bytes + raw_bytes) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test pcurve output").expect("empty output scope");
    let mut losses = Vec::new();
    let record = &exchange.records()[&1];
    let geometry = if polyline {
        polyline_pcurve(1, record, &points, &mut losses, &mut storage, &ctx)
    } else {
        nurbs_pcurve(1, record, &points, &mut losses, &mut storage, &ctx)
    }
    .expect("finite pcurve admission")
    .expect("pcurve geometry");
    let reuse = ctx.reserve_scoped(raw_bytes as u64, "test raw pole reuse")
        .expect("raw poles were destroyed; finite output remains live");
    let PcurveGeometry::Nurbs { nurbs } = &geometry else { panic!("NURBS pcurve") };
    assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(nurbs.control_points(), [
        FinitePoint2::new(Point2::new(0.0, 0.0)).expect("finite first pole"),
        FinitePoint2::new(Point2::new(1.0, 2.0)).expect("finite second pole"),
    ]);
    assert!(losses.is_empty());
    drop(reuse);
    drop(geometry);
    drop(storage);
    ctx.finish_session().expect("all pcurve scratch released");
}

#[test]
fn polyline_pcurve_reuses_raw_poles_with_finite_output_live() {
    assert_reusable_raw_storage(POLYLINE, true);
}

#[test]
fn nurbs_pcurve_reuses_raw_poles_with_finite_output_live() {
    assert_reusable_raw_storage(POLYNOMIAL, false);
}

#[test]
fn rational_pcurve_storage_contains_only_knots_and_finite_poles() {
    let exchange = parse(RATIONAL);
    let points = points();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test pcurve output").expect("empty output scope");
    let mut losses = Vec::new();
    let geometry = nurbs_pcurve(1, &exchange.records()[&1], &points, &mut losses, &mut storage, &ctx)
        .expect("finite pcurve admission").expect("rational pcurve");
    let expected = 4 * std::mem::size_of::<f64>()
        + 2 * std::mem::size_of::<cadmpeg_ir::geometry::pcurve::WeightedPole2<FinitePoint2>>();
    let CodecError::ResourceLimit(first) = ctx.reserve_scoped(u64::MAX, "test pcurve live bytes")
        .expect_err("observation exceeds materialized limit") else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.used, expected as u64);
    assert_eq!(first.additional, u64::MAX);
    let PcurveGeometry::Nurbs { nurbs } = &geometry else { panic!("NURBS pcurve") };
    assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(nurbs.pole_rows().weights(), Some(vec![1.0, 0.5]));
    assert!(losses.is_empty());
    drop(geometry);
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

fn assert_raw_pole_refusal(source: &str, polyline: bool) {
    let exchange = parse(source);
    let points = points();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let used = if polyline { 0 } else { 4 * std::mem::size_of::<f64>() };
    let raw_bytes = 4 * std::mem::size_of::<Point2>();
    policy.limits.max_materialized_bytes = (used + raw_bytes - 1) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test pcurve output").expect("empty output scope");
    let mut losses = Vec::new();
    let record = &exchange.records()[&1];
    let result = if polyline {
        polyline_pcurve(1, record, &points, &mut losses, &mut storage, &ctx)
    } else {
        nurbs_pcurve(1, record, &points, &mut losses, &mut storage, &ctx)
    };
    let CodecError::ResourceLimit(first) = result.expect_err("raw pole storage exceeds limit")
        else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, if polyline { "step_polyline_pcurve_points" } else { "step_nurbs_pcurve_control_points" });
    assert_eq!(first.used, used as u64);
    assert_eq!(first.additional, raw_bytes as u64);
    assert!(losses.is_empty());
    assert!(matches!(ctx.charge_work(0, "later operation"), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn polyline_pcurve_raw_poles_preserve_original_refusal() {
    assert_raw_pole_refusal(POLYLINE, true);
}

#[test]
fn nurbs_pcurve_raw_poles_preserve_original_refusal() {
    assert_raw_pole_refusal(POLYNOMIAL, false);
}
