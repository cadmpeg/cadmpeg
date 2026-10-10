// SPDX-License-Identifier: Apache-2.0

use super::super::clamp_edge_ranges_to_carrier_domains;
use crate::brep::AsmBrep;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Edge, EdgeCarrier};
use cadmpeg_ir::units::FiniteVector;

const EPS_ENDPOINT_NOISE: f64 = 1.0e-10;
const CURVE_ID: &str = "sat:brep:curve#domain";

fn input(range: [f64; 2]) -> AsmBrep {
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None, false,
    ).unwrap().unwrap();
    AsmBrep {
        curves: vec![Curve {
            id: CurveId::mint(CURVE_ID).unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            source_object: None,
        }],
        edges: vec![Edge {
            id: EdgeId::mint("sat:brep:edge#domain").unwrap(),
            carrier: EdgeCarrier::new(Some(CurveId::mint(CURVE_ID).unwrap()), Some(range)).unwrap(),
            start: VertexId::mint("sat:brep:vertex#a").unwrap(),
            end: VertexId::mint("sat:brep:vertex#b").unwrap(), tolerance: None,
        }],
        ..Default::default()
    }
}

#[test]
fn edge_clamping_keeps_original_identity_without_retained_copy() {
    for (range, expected) in [
        ([-EPS_ENDPOINT_NOISE, 0.5], [0.0, 0.5]),
        ([0.25, 1.0 + EPS_ENDPOINT_NOISE], [0.25, 1.0]),
        ([-EPS_ENDPOINT_NOISE, 1.0 + EPS_ENDPOINT_NOISE], [0.0, 1.0]),
        ([0.25, 0.75], [0.25, 0.75]),
        ([-0.01, 1.01], [-0.01, 1.01]),
    ] {
        let mut out = input(range);
        let expected_input = input(range);
        let identity_backing = out.edges[0].curve().unwrap().as_str().as_ptr();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        clamp_edge_ranges_to_carrier_domains(&ctx, &mut out).unwrap();
        assert_eq!(out.edges[0].param_range().map(FiniteVector::get), Some(expected));
        assert_eq!(out.edges[0].curve().unwrap().as_str(), CURVE_ID);
        assert_eq!(out.edges[0].curve().unwrap().as_str().as_ptr(), identity_backing);
        assert_eq!(out.curves, expected_input.curves);
        assert_eq!(out.edges[0].id, expected_input.edges[0].id);
        assert_eq!(out.edges[0].start, expected_input.edges[0].start);
        assert_eq!(out.edges[0].end, expected_input.edges[0].end);
        assert_eq!(out.edges[0].tolerance, expected_input.edges[0].tolerance);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn edge_clamping_preserves_invalid_interval_error_and_original_carrier() {
    let range = [-EPS_ENDPOINT_NOISE, -0.5 * EPS_ENDPOINT_NOISE];
    let mut out = input(range);
    let expected = input(range);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(clamp_edge_ranges_to_carrier_domains(&ctx, &mut out),
        Err(CodecError::Malformed(message)) if message == "edge param_range must be finite and ordered"));
    assert_eq!(out.curves, expected.curves);
    assert_eq!(out.edges, expected.edges);
    ctx.finish_session().unwrap();
}

#[test]
fn edge_clamping_preserves_original_refusal_before_any_mutation() {
    let mut out = input([-EPS_ENDPOINT_NOISE, 1.0]);
    let expected = input([-EPS_ENDPOINT_NOISE, 1.0]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original clamp refusal")
    else { panic!("expected original refusal"); };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for _ in 0..64 {
        assert!(matches!(clamp_edge_ranges_to_carrier_domains(&ctx, &mut out),
            Err(CodecError::ResourceLimit(last)) if last == first));
        assert_eq!(out.curves, expected.curves);
        assert_eq!(out.edges, expected.edges);
        assert!(matches!(clamp_edge_ranges_to_carrier_domains(&ctx, &mut AsmBrep::default()),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

fn rejected_carriers() -> AsmBrep {
    AsmBrep {
        curves: (1..=3).map(|index| Curve {
            id: CurveId::mint(format!("sat:brep:curve#{index}")).unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        }).collect(),
        ..Default::default()
    }
}

fn domain_source_refusal(work: u64) {
    let mut out = rejected_carriers();
    let expected = rejected_carriers();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = clamp_edge_ranges_to_carrier_domains(&ctx, &mut out)
    else { panic!("expected domain source refusal"); };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM edge domain curves");
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    assert_eq!(out.curves, expected.curves);
    for _ in 0..64 {
        for mut replay in [rejected_carriers(), AsmBrep::default()] {
            assert!(matches!(clamp_edge_ranges_to_carrier_domains(&ctx, &mut replay),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn edge_domain_source_refuses_before_first_curve() { domain_source_refusal(0); }

#[test]
fn edge_domain_source_refuses_before_last_curve() { domain_source_refusal(2); }

#[test]
fn edge_domain_source_accepts_exact_rejected_carrier_visits_and_empty_input() {
    for (mut out, work) in [(rejected_carriers(), 3), (AsmBrep::default(), 0)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        clamp_edge_ranges_to_carrier_domains(&ctx, &mut out).unwrap();
        assert!(out.edges.is_empty());
        assert_eq!(out.curves.len(), usize::try_from(work).unwrap());
        if work != 0 { assert_eq!(out.curves, rejected_carriers().curves); }
        ctx.finish_session().unwrap();
    }
}

fn skipped_edge_carriers() -> AsmBrep {
    let carriers = [EdgeCarrier::Free,
        EdgeCarrier::unbounded(Some(CurveId::mint(CURVE_ID).unwrap())),
        EdgeCarrier::new(None, Some([1.0, 0.0])).unwrap()];
    AsmBrep {
        edges: carriers.into_iter().enumerate().map(|(index, carrier)| Edge {
            id: EdgeId::mint(format!("sat:brep:edge#{}", index + 1)).unwrap(), carrier,
            start: VertexId::mint("sat:brep:vertex#a").unwrap(),
            end: VertexId::mint("sat:brep:vertex#b").unwrap(), tolerance: None,
        }).collect(),
        ..Default::default()
    }
}

fn edge_source_refusal(work: u64) {
    let mut out = skipped_edge_carriers();
    let expected = skipped_edge_carriers();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = clamp_edge_ranges_to_carrier_domains(&ctx, &mut out)
    else { panic!("expected edge source refusal"); };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "ASM edge range clamp");
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    assert_eq!(out.edges, expected.edges);
    for _ in 0..64 {
        for mut replay in [skipped_edge_carriers(), AsmBrep::default()] {
            assert!(matches!(clamp_edge_ranges_to_carrier_domains(&ctx, &mut replay),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn edge_clamp_source_refuses_before_first_edge() { edge_source_refusal(0); }

#[test]
fn edge_clamp_source_refuses_before_last_edge() { edge_source_refusal(2); }

#[test]
fn edge_clamp_source_accepts_exact_skipped_carrier_visits() {
    let mut out = skipped_edge_carriers();
    let expected = skipped_edge_carriers();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    clamp_edge_ranges_to_carrier_domains(&ctx, &mut out).unwrap();
    assert_eq!(out.edges, expected.edges);
    ctx.finish_session().unwrap();
}
