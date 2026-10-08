// SPDX-License-Identifier: Apache-2.0
//! Candidate rejection before retained surface-knot construction.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

use crate::test_support::parasolid::{f64_array, nurbs_surface_carrier, rational_nurbs_surface_carrier};

use super::scan_surface_carriers;

#[test]
fn zero_surface_weight_rejects_before_retained_knots() {
    let valid = rational_nurbs_surface_carrier(180, 181, 10);
    let mut invalid = valid.clone();
    let control = invalid.windows(3).position(|bytes| bytes == [0, 0x2d, 0x2b]).unwrap();
    let values = super::array_body(&invalid, control, 0x2d).unwrap()
        + crate::layout::bspline_array_header::LEN;
    invalid[values + 3 * 8..values + 4 * 8].copy_from_slice(&0.0_f64.to_be_bytes());

    // This finite native array reaches the raw-pole predicate, unlike the
    // NaN-containing framed/compact alias rejected by candidate equality.
    let error = crate::test_support::work_refusal_at("scan Parasolid surface row poles", |ctx| {
        scan_surface_carriers(ctx, &invalid, &mut Vec::new())
    });
    let CodecError::ResourceLimit(limit) = error else {
        panic!("surface pole visit refusal");
    };
    assert_eq!(limit.operation, "scan Parasolid surface row poles");
    assert_eq!(limit.additional, 1);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    // Match the core proportional MATERIALIZED_BASE ceiling for an empty root.
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "emit Parasolid expanded knots", None);
    let mut refusals = Vec::new();
    assert!(scan_surface_carriers(&ctx, &invalid, &mut refusals).unwrap().is_empty());
    assert!(refusals.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released rejected surface").unwrap());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "emit Parasolid expanded knots") else {
        panic!("unvisited knot emission probe remains armed");
    };
    assert_eq!(limit.operation, "emit Parasolid expanded knots");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    drop(probe);

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut refusals = Vec::new();
    let carriers = scan_surface_carriers(&ctx, &valid, &mut refusals).unwrap();
    assert_eq!(carriers.len(), 1);
    let carrier = carriers.get(&180).unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) = carrier.geometry.solved() else {
        panic!("rational surface carrier");
    };
    assert_eq!((surface.u_degree(), surface.v_degree(), surface.u_count(), surface.v_count()), (1, 1, 2, 2));
    assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert!(surface.weights().is_some());
    assert!(refusals.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn nan_surface_control_alias_is_rejected_before_pole_validation() {
    let mut bytes = nurbs_surface_carrier(180, 181, 10);
    let control = f64_array(0x2d, 182, &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5]);
    let at = bytes.windows(control.len()).position(|window| window == control).unwrap();
    let values = super::array_body(&bytes, at, 0x2d).unwrap()
        + crate::layout::bspline_array_header::LEN;
    bytes[values..values + 8].copy_from_slice(&f64::NAN.to_be_bytes());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let attrs = std::collections::BTreeSet::from([182]);
        let mut storage = ctx.reserve_scoped(0, "test surface aliases").unwrap();
        let arrays = storage.with_storage(|| super::scan_arrays(&ctx, &bytes, Some(&attrs), Some(&attrs))).unwrap();
        let candidates = super::array_candidates(&ctx, &mut storage, &bytes, &arrays.f64s,
            &arrays.compact, 182, |count| Ok(count == 12), |at| View::f64_be_at(&bytes, at)).unwrap();
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|candidate| candidate.as_slice()[0].is_nan()));
        drop(candidates);
        assert!(super::exact_f64_array(&ctx, &mut storage, &bytes, &arrays, 182, 12).unwrap().is_none());
    }
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan Parasolid surface row poles", None);
    let mut refusals = Vec::new();
    assert!(scan_surface_carriers(&ctx, &bytes, &mut refusals).unwrap().is_empty());
    assert!(refusals.is_empty());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "scan Parasolid surface row poles") else {
        panic!("unvisited pole validation probe remains armed");
    };
    assert_eq!(limit.operation, "scan Parasolid surface row poles");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    drop(probe);
}
