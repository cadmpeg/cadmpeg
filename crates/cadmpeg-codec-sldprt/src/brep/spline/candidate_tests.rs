// SPDX-License-Identifier: Apache-2.0
//! Rejected spline candidates release temporary geometry storage.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

use crate::test_support::parasolid::{f64_array, nurbs_surface_carrier, rational_nurbs_surface_carrier};

use super::scan_surface_carriers;

#[test]
fn undersized_curve_knots_release_the_expanded_values() {
    use crate::test_support::parasolid::{nurbs_curve_carrier, u16_array};

    let mut bytes = nurbs_curve_carrier(170, 171);
    let original = u16_array(173, &[3, 3]);
    let at = bytes.windows(original.len()).position(|window| window == original).unwrap();
    bytes.splice(at..at + original.len(), u16_array(173, &[1, 1]));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut refusals = Vec::new();
    assert!(super::scan_curve_carriers(&ctx, &bytes, &mut refusals).unwrap().is_empty());
    assert!(refusals.is_empty());
    // Two expanded knots fail the six-knot requirement; no child survives.
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released short curve knots").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn ir_rejected_splines_retain_only_the_ordered_loss_output() {
    use crate::test_support::parasolid::{nurbs_curve_carrier, rational_linear_nurbs_curve_carrier};
    use cadmpeg_ir::report::loss::LossNote;

    for (mut bytes, curve, last_coordinate, attr) in [
        (nurbs_curve_carrier(170, 171), true, 6, 170),
        (rational_linear_nurbs_curve_carrier(170, 171), true, 4, 170),
        (nurbs_surface_carrier(180, 181, 10), false, 9, 180),
        (rational_nurbs_surface_carrier(180, 181, 10), false, 12, 180),
    ] {
        let control = bytes.windows(3).position(|window| window == [0, 0x2d, 0x2b]).unwrap();
        let values = super::array_body(&bytes, control, 0x2d).unwrap()
            + crate::layout::bspline_array_header::LEN;
        // Native coordinates stay finite; length conversion overflows at the
        // last pole, after IR has allocated its converted prefix/grid rows.
        let coordinate = values + last_coordinate * 8;
        bytes[coordinate..coordinate + 8].copy_from_slice(&f64::MAX.to_be_bytes());
        let kind = if curve { "curve" } else { "surface" };
        let expected_message = format!(
            "{kind} carrier attribute {attr}: control_points contains a non-finite point; \
             the carrier is not emitted and the entities that reference it fall back \
             to an untyped support."
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Exact formatted message bytes plus the initial four LossNote slots.
        // Candidate knots, converted poles and IR diagnostic text are scratch.
        let retained = u64::try_from(expected_message.len() + 4 * std::mem::size_of::<LossNote>()).unwrap();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut refusals = Vec::new();
        let empty = if curve {
            super::scan_curve_carriers(&ctx, &bytes, &mut refusals).unwrap().is_empty()
        } else {
            scan_surface_carriers(&ctx, &bytes, &mut refusals).unwrap().is_empty()
        };
        assert!(empty);
        assert_eq!(refusals.len(), 1);
        assert_eq!(refusals[0].code.local_code(), "geometry.spline-lanes-unpaired");
        assert_eq!(refusals[0].message, expected_message);
        drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test released IR rejected candidate").unwrap());
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test loss-only retained boundary") else {
            panic!("exact loss-only retained boundary");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((limit.used, limit.additional), (retained, 1));
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn accepted_spline_children_are_committed_before_publication() {
    use crate::test_support::parasolid::nurbs_curve_carrier;
    use cadmpeg_ir::features::FinitePoint3;

    // Curve: eight expanded-knot slots and three converted poles.
    let curve_bytes = 8 * 8 + 3 * std::mem::size_of::<FinitePoint3>();
    // Surface: two four-knot axes, two grid rows and two poles per row.
    let surface_bytes = 2 * 4 * 8 + 2 * std::mem::size_of::<Vec<FinitePoint3>>()
        + 2 * 2 * std::mem::size_of::<FinitePoint3>();
    for (bytes, curve, operation, additional) in [
        (nurbs_curve_carrier(170, 171), true, "retain Parasolid curve candidate", curve_bytes),
        (nurbs_surface_carrier(180, 181, 10), false, "retain Parasolid surface candidate", surface_bytes),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut refusals = Vec::new();
        let result = if curve {
            super::scan_curve_carriers(&ctx, &bytes, &mut refusals).map(|_| ())
        } else {
            scan_surface_carriers(&ctx, &bytes, &mut refusals).map(|_| ())
        };
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("candidate publication refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, operation);
        assert_eq!((limit.used, limit.additional), (0, u64::try_from(additional).unwrap()));
        assert!(refusals.is_empty());
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

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
