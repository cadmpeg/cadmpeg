// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::bezier::HomogeneousBezierSpan;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, WeightedPole3};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::NonZeroReal;
use std::mem::size_of;

fn rail(y: f64, weights: Option<Vec<f64>>) -> NurbsCurve {
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, y, 0.0), Point3::new(1.0, y, 0.0)],
        weights,
        false,
    ).unwrap().unwrap()
}

#[test]
fn ruled_pairing_releases_consumed_rows_before_output_knots() {
    let first = rail(0.0, None);
    let second = rail(1.0, None);
    let weights = [NonZeroReal::new(0.5).unwrap(); 2];
    // Exact collection capacities: two source point rows and weight rows,
    // each with two controls, then two output weighted rows. The current
    // aggregate source reservation lasts until pairing returns. Check both
    // row admission boundaries and its release before the output knot copies.
    let source = 2 * size_of::<Vec<FinitePoint3>>() + 4 * size_of::<FinitePoint3>()
        + 2 * size_of::<Vec<NonZeroReal>>() + 4 * size_of::<NonZeroReal>();
    let output_outer = 2 * size_of::<Vec<WeightedPole3<FinitePoint3>>>();
    let output_row = 2 * size_of::<WeightedPole3<FinitePoint3>>();
    let used = u64_from_index(source + output_outer + output_row);
    let first_used = u64_from_index(source + output_outer);
    let additional = u64_from_index(output_row);
    let peak = used + additional;
    for (cap, refusal_used) in [
        (first_used + additional - 1, Some(first_used)),
        (peak - 1, Some(used)),
        (peak, None),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut output_storage = ctx.reserve_scoped(0, "test ruled output owner").unwrap();
        let result = output_storage.with_storage(|| {
            super::super::same_basis_ruled_surface(&first, &second, &weights, &ctx)
        });
        if let Some(used) = refusal_used {
            let error = result.unwrap_err();
            let CodecError::ResourceLimit(first) = error else {
                panic!("expected actual weighted-row allocation refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "iges ruled same-basis weighted row controls");
            assert_eq!((first.limit, first.used, first.additional), (cap, used, additional));
            drop(output_storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let surface = result.unwrap();
            assert_eq!((surface.u_degree(), surface.v_degree()), (1, 1));
            assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
            assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(surface.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            for u in 0..2 {
                for v in 0..2 {
                    assert_eq!(surface.pole(u, v).unwrap().get(), Point3::new(f64::from(u32::try_from(u).unwrap()), f64::from(u32::try_from(v).unwrap()), 0.0));
                    assert_eq!(surface.weight(u, v).unwrap().get(), 0.5);
                }
            }
            drop(surface);
            drop(output_storage);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn closure_extraction_releases_source_points_and_weights_before_bezier_working_lanes() {
    let curve = rail(0.0, Some(vec![1.0, 0.5]));
    // Positive controls use two exact slots. Bezier working controls, span
    // slots and each span's controls grow from empty to the core minimum
    // of four slots. Both four-knot copies remain live at the last span
    // allocation. Source point and weight copies have already been destroyed.
    let used = u64_from_index(
        2 * size_of::<[f64; 4]>() + 4 * size_of::<f64>()
        + 4 * size_of::<[f64; 4]>() + 4 * size_of::<f64>()
        + 4 * size_of::<HomogeneousBezierSpan>(),
    );
    let additional = u64_from_index(4 * size_of::<[f64; 4]>());
    let peak = used + additional;
    for cap in [peak - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::homogeneous_bezier_spans(&ctx, &curve);
        if cap < peak {
            let CodecError::ResourceLimit(first) = result.unwrap_err() else {
                panic!("expected actual Bezier span allocation refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "Bezier span controls");
            assert_eq!((first.limit, first.used, first.additional), (cap, used, additional));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let spans = result.unwrap().unwrap();
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].domain, [0.0, 1.0]);
            assert_eq!(spans[0].controls, [[0.0, 0.0, 0.0, 1.0], [0.5, 0.0, 0.0, 0.5]]);
            drop(spans);
            ctx.finish_session().unwrap();
        }
    }
}

fn same_basis_source_refusal(cap: u64, operation: &'static str) {
    let first = rail(0.0, None);
    let second = rail(1.0, None);
    let weights = [NonZeroReal::new(0.5).unwrap(); 2];
    let first_before = serde_json::to_value(&first).unwrap();
    let second_before = serde_json::to_value(&second).unwrap();
    // The unit-weight predicate stops at the first non-unit scalar: one
    // actual visit, with no terminal probe. Each source pole and weight row
    // then has one visit. Fixed two-control row copies perform no scan.
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) =
        super::super::same_basis_ruled_surface(&first, &second, &weights, &ctx)
    else {
        panic!("expected actual ruled source visit refusal");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert_eq!(original.operation, operation);
    assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
    for _ in 0..64 {
        for replay in [&weights[..], &[]] {
            assert!(matches!(super::super::same_basis_ruled_surface(&first, &second, replay, &ctx),
                Err(CodecError::ResourceLimit(last)) if last == original));
        }
    }
    assert_eq!(serde_json::to_value(&first).unwrap(), first_before);
    assert_eq!(serde_json::to_value(&second).unwrap(), second_before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == original));
}

#[test]
fn ruled_same_basis_poles_refuse_first_actual_source_visit() {
    same_basis_source_refusal(1, "iges ruled same-basis pole traversal");
}

#[test]
fn ruled_same_basis_poles_refuse_last_actual_source_visit() {
    same_basis_source_refusal(2, "iges ruled same-basis pole traversal");
}

#[test]
fn ruled_same_basis_weights_refuse_first_actual_source_visit() {
    same_basis_source_refusal(3, "iges ruled weight row traversal");
}

#[test]
fn ruled_same_basis_weights_refuse_last_actual_source_visit() {
    same_basis_source_refusal(4, "iges ruled weight row traversal");
}

fn closure_source_refusal(curve: &NurbsCurve, cap: u64, additional: u64, operation: &'static str) {
    let before = serde_json::to_value(curve).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) =
        super::super::homogeneous_bezier_spans(&ctx, curve)
    else {
        panic!("expected closure source or first conversion refusal");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert_eq!(original.operation, operation);
    assert_eq!((original.limit, original.used, original.additional), (cap, cap, additional));
    for _ in 0..64 {
        assert!(matches!(super::super::homogeneous_bezier_spans(&ctx, curve),
            Err(CodecError::ResourceLimit(last)) if last == original));
    }
    assert_eq!(serde_json::to_value(curve).unwrap(), before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == original));
}

#[test]
fn closure_polynomial_poles_refuse_before_complete_infallible_scan() {
    // Both immutable positions exist at indices0..pole_count. The complete
    // infallible pole scan admits two visits together before its first read.
    closure_source_refusal(&rail(0.0, None), 0, 2, "iges surface closure pole traversal");
}

#[test]
fn closure_polynomial_poles_complete_before_first_control_conversion() {
    closure_source_refusal(&rail(0.0, None), 2, 1, "Bezier positive control conversion");
}

#[test]
fn closure_rational_weights_refuse_first_actual_source_visit() {
    closure_source_refusal(&rail(0.0, Some(vec![1.0, 0.5])), 0, 1,
        "iges surface closure weight traversal");
}

#[test]
fn closure_rational_weights_refuse_last_actual_source_visit() {
    closure_source_refusal(&rail(0.0, Some(vec![1.0, 0.5])), 1, 1,
        "iges surface closure weight traversal");
}

#[test]
fn closure_relative_weight_underflow_accepts_exact_work_and_releases_scratch() {
    let curve = rail(0.0, Some(vec![f64::MIN_POSITIVE, f64::MAX]));
    assert_eq!(f64::MIN_POSITIVE / f64::MAX, 0.0);
    let before = serde_json::to_value(&curve).unwrap();
    // Two extracted weights, two extracted poles, two weight validations,
    // two scale visits, then one conversion rejects the vanished weight.
    let work = 2 + 2 + 2 + 2 + 1;
    let source = 2 * size_of::<f64>() + 2 * size_of::<FinitePoint3>();
    let controls = 2 * size_of::<[f64; 4]>();
    let peak = u64_from_index(source + controls);
    for cap in [peak - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::homogeneous_bezier_spans(&ctx, &curve);
        if cap < peak {
            let CodecError::ResourceLimit(original) = result.unwrap_err() else {
                panic!("expected homogeneous control backing refusal");
            };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(original.operation, "iges_surface_closure_controls");
            assert_eq!((original.limit, original.used, original.additional),
                (cap, u64_from_index(source), u64_from_index(controls)));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == original));
        } else {
            assert!(result.unwrap().is_none());
            let released = ctx.reserve_scoped(peak, "test released closure scratch").unwrap();
            drop(released);
            assert!(ctx.resource_refusal().is_none());
            ctx.finish_session().unwrap();
        }
        assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    }
}
