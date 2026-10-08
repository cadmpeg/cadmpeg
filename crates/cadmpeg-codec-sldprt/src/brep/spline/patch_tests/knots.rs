// SPDX-License-Identifier: Apache-2.0
//! Early knot-expansion rejection and temporary-prefix ownership.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::expanded_knots;

#[test]
fn excessive_knot_multiplicity_does_not_visit_the_tail() {
    let values = vec![0.0; 4096];
    let multiplicities = vec![2; 4096];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(expanded_knots(&ctx, &values, &multiplicities, 1).unwrap(), None);
    ctx.finish_session().unwrap();

    let arena = DecodeArena::new();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = expanded_knots(&ctx, &values, &multiplicities, 1) else {
        panic!("first paired visit refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "scan Parasolid knot values");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn excessive_knot_multiplicity_releases_the_emitted_prefix() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two paired visits and the one emitted knot before rejection.
    policy.limits.max_work_units = 3;
    policy.limits.max_retained_bytes = 0;
    // The core Vec's initial four f64 slots hold the one emitted knot.
    policy.limits.max_materialized_bytes = 4 * 8;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(expanded_knots(&ctx, &[0.0, 1.0, 2.0], &[1, 2, 1], 2).unwrap(), None);
    drop(ctx.reserve_scoped(4 * 8, "test released expanded knot prefix").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn empty_paired_knot_rosters_do_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(expanded_knots(&ctx, &[], &[], 0).unwrap(), Some(Vec::new()));
    assert_eq!(expanded_knots(&ctx, &[0.0], &[], 1).unwrap(), Some(Vec::new()));
    assert_eq!(expanded_knots(&ctx, &[], &[1], 1).unwrap(), Some(Vec::new()));
    ctx.finish_session().unwrap();
}

#[test]
fn curve_scanner_rejects_excessive_multiplicity_without_retaining_knots() {
    use crate::test_support::parasolid::{nurbs_curve_carrier, u16_array};
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;

    let original = nurbs_curve_carrier(170, 171);
    let old = u16_array(173, &[3, 3]);
    let at = original.windows(old.len()).position(|window| window == old).unwrap();
    // Three poles and degree two require six expanded knots. These inputs
    // reject at the first pair, or after emitting a one-knot prefix.
    for multiplicities in [[7, 3], [1, 6]] {
        let mut bytes = original.clone();
        bytes.splice(at..at + old.len(), u16_array(173, &multiplicities));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan Parasolid knot values", Some(2));
        let mut refusals = Vec::new();
        assert!(super::super::scan_curve_carriers(&ctx, &bytes, &mut refusals).unwrap().is_empty());
        assert!(refusals.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(2, "scan Parasolid knot values") else {
            panic!("paired-tail precharge probe remains armed");
        };
        assert_eq!(limit.operation, "scan Parasolid knot values");
        assert_eq!(limit.additional, 2);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        drop(probe);
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut refusals = Vec::new();
    let carriers = super::super::scan_curve_carriers(&ctx, &original, &mut refusals).unwrap();
    assert_eq!(carriers.len(), 1);
    let carrier = carriers.get(&170).unwrap();
    assert!(matches!(carrier.geometry.solved(), Some(SolvedCurveGeometry::Nurbs(curve))
        if curve.degree() == 2 && curve.pole_count() == 3 && curve.knots().as_slice() == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]));
    assert!(refusals.is_empty());
    ctx.finish_session().unwrap();
}
