// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn line(weights: Option<[f64; 2]>) -> NurbsCurve {
    NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        weights.map(Vec::from), false).unwrap().unwrap()
}

fn search(ctx: &DecodeContext<'_>, curve: &NurbsCurve, point: f64, seed: f64)
    -> Result<Option<crate::scalar::FiniteReal>, CodecError>
{
    nurbs_curve_parameter_near_point(ctx, curve, Point3::new(point, 0.0, 0.0), 0.0, seed)
}

#[test]
fn curve_inverse_borrows_poles_and_weights_with_real_boundary_and_derivative_storage() {
    // Boundaries and the reached first-derivative basis each use four
    // amortized f64 backing slots. Their two live vectors overlap.
    let bytes = u64::try_from(8 * std::mem::size_of::<crate::scalar::FiniteReal>()).unwrap();
    for weights in [None, Some([1.0, 1.0]), Some([2.0_f64.powi(-900); 2]), Some([2.0_f64.powi(900); 2])] {
        let curve = line(weights);
        // Speed: four knots, three order pairs, four two-pole passes.
        // Then two boundary copies, two witness visits, one Newton pair
        // and two first-derivative basis producer advances. The two-pole
        // point/homogeneous sums and inline bases perform fixed work.
        // Rational rows also have the original two positivity visits.
        let work = 4 + 3 + 4 * 2 + 2 + 2 + 1 + 2 + if weights.is_some() { 2 } else { 0 };
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_collection_items = 4;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = work;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        // Seed differs from the witness: the real Newton tangent runs.
        assert_eq!(search(&ctx, &curve, 0.25, 0.5).unwrap(), Some(crate::scalar::FiniteReal::new(0.25).unwrap()));
        let reuse = ctx.reserve_scoped_limit(bytes, "inverse boundary backing released").unwrap();
        drop(reuse);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn curve_inverse_preserves_failed_first_derivative_budget_and_exact_scratch_shortages() {
    let boundary_bytes = u64::try_from(4 * std::mem::size_of::<crate::scalar::FiniteReal>()).unwrap();
    for weights in [None, Some([1.0, 1.0]), Some([2.0_f64.powi(-900); 2]), Some([2.0_f64.powi(900); 2])] {
        let curve = line(weights);
        let before_derivative = 4 + 3 + 4 * 2 + 2 + 2 + 1 + if weights.is_some() { 2 } else { 0 };
        // Preserve the first failed success control's actual input and cap.
        // It reaches the derivative producer with no work left.
        for dimension in [ResourceDimension::WorkUnits, ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = before_derivative + 2;
            policy.limits.max_materialized_bytes = 2 * boundary_bytes;
            policy.limits.max_collection_items = 4;
            policy.limits.max_retained_bytes = 0;
            match dimension {
                ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = before_derivative;
                    policy.limits.max_materialized_bytes = boundary_bytes;
                    policy.limits.max_collection_items = 2;
                }
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes -= 1,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items -= 1,
                _ => unreachable!("actual reached derivative dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(CodecError::ResourceLimit(original)) = search(&ctx, &curve, 0.25, 0.5)
                else { panic!("actual first derivative work or backing must refuse"); };
            assert_eq!(original.dimension, dimension);
            assert_eq!(original.operation, if dimension == ResourceDimension::WorkUnits {
                "IR B-spline derivative work"
            } else { "IR B-spline derivative basis" });
            if dimension == ResourceDimension::WorkUnits {
                assert_eq!((original.limit, original.used, original.additional), (before_derivative, before_derivative, 1));
            }
            assert!(matches!(search(&ctx, &curve, f64::NAN, f64::NAN),
                Err(CodecError::ResourceLimit(limit)) if limit == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
        }
    }
}

#[test]
fn curve_inverse_keeps_exact_boundary_shortages_and_original_fused_errors() {
    let bytes = u64::try_from(4 * std::mem::size_of::<crate::scalar::FiniteReal>()).unwrap();
    for rational in [false, true] {
        let curve = line(rational.then_some([1.0, 1.0]));
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = bytes;
            policy.limits.max_collection_items = 2;
            policy.limits.max_retained_bytes = 0;
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes -= 1,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items -= 1,
                _ => unreachable!("actual boundary storage dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(CodecError::ResourceLimit(original)) = search(&ctx, &curve, 0.25, 0.5)
                else { panic!("actual boundary backing must refuse before allocation"); };
            assert_eq!(original.dimension, dimension);
            assert_eq!(original.operation, "IR curve inversion boundaries");
            let result = nurbs_curve_parameter_near_point(&ctx, &curve, Point3::new(f64::NAN, 0.0, 0.0), f64::NAN, f64::NAN);
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
        }
    }
}

#[test]
fn curve_inverse_stops_at_actual_negative_weight_before_allocating_boundaries() {
    for (weights, visits) in [([-1.0, 1.0], 1), ([1.0, -1.0], 2)] {
        let curve = line(Some(weights));
        for cap in 0..=visits {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_retained_bytes = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = search(&ctx, &curve, 0.25, 0.5);
            if cap < visits {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("next actual weight visit must refuse"); };
                assert_eq!(original.operation, "IR curve inversion weight scan");
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
            } else {
                assert_eq!(result.unwrap(), None);
                assert_eq!(ctx.resource_refusal(), None);
                ctx.finish_session().unwrap();
            }
        }
    }
}
