// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::{admission::EvaluationAdmission, ModelCurveRequest};
use crate::geometry::PlacedCurve;
use crate::transform::Transform;
use cadmpeg_core::decode::WorkBudget;

fn curve(degree: u32, width: f64, endpoint: f64, common: f64, periodic: bool) -> SolvedCurveGeometry {
    let support = usize::try_from(degree + 1).unwrap();
    let mut knots = vec![0.0; support]; knots.extend(std::iter::repeat_n(width, support));
    let mut points = vec![Point3::new(0.0, 0.0, 0.0); support]; points[support - 1].x = endpoint;
    let mut weights = vec![common; support]; weights[support - 1] = 2.0 * common;
    SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(),
        degree, knots, points, Some(weights), periodic).unwrap().unwrap())
}

fn higher(admission: EvaluationAdmission<'_, '_>, geometry: &SolvedCurveGeometry, t: f64)
    -> Result<crate::eval::curve_higher::CurveHigher, EvaluationFailure<()>>
{
    crate::eval::curve_higher::stored_higher(&decode::Scratch::new(admission), geometry,
        FiniteReal::new(t).unwrap(), Err(EvaluationFailure::NoValue), Err(EvaluationFailure::NoValue), ModelCurveRequest::Fourth)
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= 128.0 * f64::EPSILON * expected.abs(), "{actual:?} versus {expected:?}");
}

#[test]
fn quadratic_and_cubic_fourth_obey_the_real_quotient_laws_with_zero_variable_budgets() {
    for degree in [2, 3] {
        for exponent in [-900, 0, 900] {
            for sign in [-1.0, 1.0] {
                let geometry = curve(degree, 1.0, 1.0, sign * 2.0_f64.powi(exponent), false);
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 0; policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0; policy.limits.max_collection_items = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
                    for t in [0.0_f64, 0.25, 0.5, 1.0] {
                        // Direct differentiation of 2t^2/(1+t^2), or 2t^3/(1+t^3).
                        let expected = if degree == 2 {
                            -48.0 * (1.0 - 10.0 * t.powi(2) + 5.0 * t.powi(4)) / (1.0 + t.powi(2)).powi(5)
                        } else {
                            -144.0 * t.powi(2) * (5.0 - 17.0 * t.powi(3) + 5.0 * t.powi(6)) / (1.0 + t.powi(3)).powi(5)
                        };
                        let result = higher(admission, &geometry, t).unwrap();
                        close(result.fourth.unwrap().x, expected);
                        assert_eq!(result.third, crate::eval::curve_higher::stored_third(&decode::Scratch::new(admission),
                            &geometry, FiniteReal::new(t).unwrap(), Err(EvaluationFailure::NoValue)));
                    }
                }
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn requested_fourth_preserves_extended_spans_subnormals_and_separate_final_order_failures() {
    let admission = EvaluationAdmission::Standard;
    let width = 2.0_f64.powi(-350);
    for degree in [2, 3] {
        let geometry = curve(degree, width, 2.0_f64.powi(-1000), 1.0, false);
        let expected = if degree == 2 { 58368.0 / 3125.0 } else {
            -144.0 * 0.25 * (5.0 - 17.0 / 8.0 + 5.0 / 64.0) / (9.0_f64 / 8.0).powi(5)
        };
        close(higher(admission, &geometry, width / 2.0).unwrap().fourth.unwrap().x,
            expected * 2.0_f64.powi(400));
        let overflow = curve(degree, 2.0_f64.powi(-600), 2.0_f64.powi(-1000), 1.0, false);
        let result = higher(admission, &overflow, 2.0_f64.powi(-601)).unwrap();
        assert!(result.third.is_ok()); assert_eq!(result.fourth, Err(EvaluationFailure::NonFinite(())));
    }
    // At t=0 the quadratic has Third0 but Fourth=-48*MAX, which overflows.
    let result = higher(admission, &curve(2, 1.0, f64::MAX, 1.0, false), 0.0).unwrap();
    assert_eq!(result.third, Ok(FiniteVector3::ZERO));
    assert_eq!(result.fourth, Err(EvaluationFailure::NonFinite(())));
    assert_eq!(higher(admission, &curve(2, 1.0, f64::from_bits(1), 1.0, false), 0.0)
        .unwrap().fourth.unwrap().x, -f64::from_bits(48));
    // At t=0 the cubic has Third=12*MAX but exact Fourth0.
    let result = higher(admission, &curve(3, 1.0, f64::MAX, 1.0, false), 0.0).unwrap();
    assert_eq!(result.third, Err(EvaluationFailure::NonFinite(())));
    assert_eq!(result.fourth, Ok(FiniteVector3::ZERO));
    // At s=1 the cubic Fourth is31.5; physical width4 divides by256.
    let result = higher(admission, &curve(3, 4.0, f64::MAX, 1.0, false), 4.0).unwrap();
    assert!(result.third.is_ok());
    close(result.fourth.unwrap().x, (31.5 / 256.0) * f64::MAX);
    let periodic = curve(2, 1.0, 1.0, 1.0, true);
    let expected = higher(admission, &periodic, 0.5).unwrap().fourth;
    for t in [-0.5, 1.5, 100.5] { assert_eq!(higher(admission, &periodic, t).unwrap().fourth, expected); }
}

#[test]
fn degree_four_fourth_uses_true_five_lane_backing_and_the_same_real_support_prefix() {
    let geometry = curve(4, 1.0, 1.0, 1.0, false);
    // Two support5 buffers: initialize10 + rows4 + cells14 + poles5 =33.
    let bytes = u64::try_from(10 * std::mem::size_of::<[f64; 5]>()).unwrap();
    for trigger in 0..5 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes - u64::from(trigger == 0);
        policy.limits.max_collection_items = 10 - u64::from(trigger == 1);
        policy.limits.max_work_units = 33 - u64::from(trigger == 2); policy.limits.max_retained_bytes = 0;
        if trigger == 3 { policy.limits.max_recursion_depth = 0; }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = higher(EvaluationAdmission::Decode(&ctx), &geometry, 0.0);
        if trigger < 4 {
            let Err(EvaluationFailure::ResourceLimit(original)) = result else { panic!("next real operation must refuse") };
            assert_eq!(original.dimension, match trigger { 0 => ResourceDimension::MaterializedBytes,
                1 => ResourceDimension::CollectionItems, 2 => ResourceDimension::WorkUnits, _ => ResourceDimension::RecursionDepth });
            assert_eq!(original.operation, match trigger { 0 | 1 => "IR requested curve basis storage",
                2 => "IR requested curve homogeneous support", _ => "geometry evaluation nesting" });
            assert_eq!((original.limit, original.used, original.additional), match trigger {
                0 => (bytes - 1, bytes / 2, bytes / 2), 1 => (9, 5, 5), 2 => (32, 32, 1), _ => (0, 0, 1) });
            assert!(matches!(higher(EvaluationAdmission::Decode(&ctx), &geometry, -1.0), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        } else {
            let result = result.unwrap();
            assert_eq!(result.third, Ok(FiniteVector3::ZERO)); assert_eq!(result.fourth.unwrap().x, 48.0);
            drop(ctx.reserve_scoped_limit(bytes, "five lane backing released").unwrap()); ctx.finish_session().unwrap();
        }
    }
    for cap in [32, 33] {
        let work = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&work, |admission| higher(admission, &geometry, 0.0));
        if cap == 32 { assert!(matches!(result, Err(EvaluationFailure::NoValue))); }
        else { assert_eq!(result.unwrap().fourth.unwrap().x, 48.0); }
        assert_eq!(work.consumed(), cap);
    }
}

#[test]
fn general_fourth_maps_each_real_placement_under_decode_and_standard_admission() {
    let source = curve(2, 1.0, 1.0, 1.0, false);
    let wrap = |basis| SolvedCurveGeometry::Transformed(PlacedCurve::try_new(Box::new(basis),
        Transform::affine([[2.0, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0]]).unwrap()).unwrap());
    let placed = wrap(wrap(source));
    for cap in [0, 1, 2] {
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0; policy.limits.max_retained_bytes = 0; policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = higher(EvaluationAdmission::Decode(&ctx), &placed, 0.5);
        if cap < 2 {
            let Err(EvaluationFailure::ResourceLimit(original)) = result else { panic!("next actual placement must refuse") };
            assert_eq!(original.operation, "IR curve higher source traversal");
            assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
        } else { close(result.unwrap().fourth.unwrap().x, 4.0 * 58368.0 / 3125.0); ctx.finish_session().unwrap(); }
        let cap = usize::try_from(cap).unwrap();
        let work = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&work, |admission| higher(admission, &placed, 0.5));
        if cap < 2 { assert!(matches!(result, Err(EvaluationFailure::NoValue))); }
        else { close(result.unwrap().fourth.unwrap().x, 4.0 * 58368.0 / 3125.0); }
        assert_eq!(work.consumed(), cap);
    }
}

#[test]
fn actual_selected_basis_fourth_loss_preserves_completed_third_and_source_constant_law() {
    let h = 2.0_f64.powi(-400);
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.0, h, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0, 4.0, 4.0];
    for constant in [false, true] {
        let mut points = vec![Point3::new(0.0, 0.0, 0.0); 9];
        if !constant { points[4].x = 1.0; }
        let mut weights = vec![1.0; 9]; weights[4] = 2.0;
        let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(),
            4, knots.clone(), points, Some(weights), false).unwrap().unwrap());
        let result = higher(EvaluationAdmission::Standard, &geometry, 0.0).unwrap();
        assert_eq!(result.third, Ok(FiniteVector3::ZERO));
        assert_eq!(result.fourth, if constant { Ok(FiniteVector3::ZERO) } else { Err(EvaluationFailure::NoValue) });
    }
}

#[test]
fn rational_model_fourth_retains_replica_subset_even_chain_and_all_completed_lower_orders() {
    use crate::geometry::{Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition};
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    use crate::ids::{CurveId, ProceduralCurveId};
    for degree in [1, 2, 3] {
        let mut ir = crate::CadIr::empty();
        let source = CurveId::mint("test:model:curve#rational-fourth-source").unwrap();
        ir.model.curves.push(Curve { id: source.clone(), geometry: CurveGeometry::Solved(
            curve(degree, 1.0, 1.0, 1.0, false)), source_object: None });
        let subset = CurveId::mint("test:model:curve#rational-fourth-subset").unwrap();
        let replica = CurveId::mint("test:model:curve#rational-fourth-replica").unwrap();
        for id in [&subset, &replica] {
            let name = if id == &subset { "rational-fourth-subset" } else { "rational-fourth-replica" };
            let construction = ProceduralCurveId::mint(format!("test:model:construction#{name}")).unwrap();
            ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Procedural { construction: construction.clone(), cache: None }, source_object: None });
            let definition = if id == &subset {
                ProceduralCurveDefinition::Subset(SubsetCurveConstruction::try_new(source.clone(), [0.0, 1.0], false, None).unwrap())
            } else {
                ProceduralCurveDefinition::Replica { source: subset.clone(), transform: Transform::affine([
                    [2.0, 0.0, 0.0, 5.0], [0.0, 3.0, 0.0, 7.0], [0.0, 0.0, 4.0, 11.0],
                ]).unwrap() }
            };
            ir.model.add_procedural_curve(&crate::document::admission::StandardAdmission, id,
                ProceduralCurve::new(construction, definition)).unwrap().unwrap();
        }
        let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
        let arena = DecodeArena::new(); let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let expected = match degree { 1 => -3.0, 2 => 12.0, _ => 63.0 };
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let evaluate = |request| crate::eval::model_curve_differential_by_id(admission, &index, &replica, 0.0, request).unwrap();
            let result = evaluate(ModelCurveRequest::Fourth);
            close(result.fourth.unwrap().x, expected);
            let lower = evaluate(ModelCurveRequest::Third);
            assert_eq!(result.point, lower.point); assert_eq!(result.tangent, lower.tangent);
            assert_eq!(result.acceleration, lower.acceleration); assert_eq!(result.third, lower.third);
            assert_eq!(lower.fourth, Err(EvaluationFailure::NoValue));
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn recovered_degree_five_fourth_reaches_the_real_rational_owner_with_original_visit_limits() {
    let h = 2.0_f64.powi(-342);
    let knots = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, h, 1.0, 2.0, 3.0, 4.0,
        5.0, 5.0, 5.0, 5.0, 5.0, 5.0];
    let mut points = vec![Point3::new(0.0, 0.0, 0.0); 11]; points[4].x = 1.0;
    let geometry = SolvedCurveGeometry::Nurbs(NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 5, knots, points, Some(vec![1.0; 11]), false)
        .unwrap().unwrap());
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::try_from(12 * std::mem::size_of::<[f64; 5]>()).unwrap();
    policy.limits.max_collection_items = 12;
    // Two six-row initializations12 + degree rows5 + cells20 + support6.
    policy.limits.max_work_units = 43;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let actual = higher(EvaluationAdmission::Decode(&ctx), &geometry, 0.0).unwrap();
    assert_eq!(actual.third, Ok(FiniteVector3::ZERO));
    // W=1, and C=B4 starts (5/6)t^4/h, hence physical C4=20/h.
    assert_eq!(actual.fourth.unwrap().x, 20.0 / h);
    drop(ctx.reserve_scoped_limit(policy.limits.max_materialized_bytes, "recovered row backing released").unwrap());
    ctx.finish_session().unwrap();
    let work = WorkBudget::new(43);
    let actual = EvaluationAdmission::Standard.within_work_slice(&work, |admission| higher(admission, &geometry, 0.0)).unwrap();
    assert_eq!(actual.third, Ok(FiniteVector3::ZERO)); assert_eq!(actual.fourth.unwrap().x, 20.0 / h);
    assert_eq!(work.consumed(), 43);
}
