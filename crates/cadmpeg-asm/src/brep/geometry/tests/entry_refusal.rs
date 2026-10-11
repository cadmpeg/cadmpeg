// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::{Point3, Vector3};

fn rational_arc_storage_boundary(degree: usize, last_underflow: bool, exact: bool) {
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    const MAX_WEIGHT: f64 = 1.0e308;
    const TINY_WEIGHT: f64 = f64::MIN_POSITIVE / 2.0;
    let count = 4 * degree + 1;
    let mut knots = Vec::new();
    for span in 0..=4 {
        let repeats = if span == 0 || span == 4 {
            degree + 1
        } else {
            degree
        };
        knots.extend(std::iter::repeat_n(f64::from(span), repeats));
    }
    let underflow = if last_underflow { count - 1 } else { 0 };
    let weights = (0..count)
        .map(|index| {
            if index == underflow {
                TINY_WEIGHT
            } else {
                MAX_WEIGHT
            }
        })
        .collect();
    let points = (0..count).map(|_| Point3::new(0.0, 0.0, 0.0)).collect();
    let source = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        u32::try_from(degree).unwrap(),
        knots,
        points,
        Some(weights),
        false,
    )
    .expect("trusted source admission")
    .expect("valid rational lanes");
    let before = serde_json::to_value(&source).unwrap();
    let bytes = u64_from_index(count * std::mem::size_of::<[f64; 4]>());
    let cap = bytes - u64::from(!exact);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64_from_index(count);
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::rational_four_arc_circle(&ctx, &source);
    if exact {
        assert!(result.is_none());
        assert_eq!(serde_json::to_value(&source).unwrap(), before);
        let released = ctx
            .reserve_scoped(bytes, "test rational arc scratch released")
            .unwrap();
        drop(released);
        ctx.finish_session().unwrap();
    } else {
        let Some(Err(CodecError::ResourceLimit(first))) = result else {
            panic!("expected homogeneous storage refusal");
        };
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.operation, "ASM rational four-arc homogeneous poles");
        assert_eq!((first.limit, first.used, first.additional), (cap, 0, bytes));
        for _ in 0..64 {
            assert!(
                matches!(super::super::rational_four_arc_circle(&ctx, &source),
                    Some(Err(CodecError::ResourceLimit(last))) if last == first)
            );
            assert_eq!(serde_json::to_value(&source).unwrap(), before);
        }
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn asm_rational_arc_storage_refuses_one_short_before_pole_execution() {
    for degree in [2, 64] {
        for last in [false, true] {
            rational_arc_storage_boundary(degree, last, false);
        }
    }
}

#[test]
fn asm_rational_arc_first_weight_underflow_recovers_and_releases_storage() {
    for degree in [2, 64] {
        rational_arc_storage_boundary(degree, false, true);
    }
}

#[test]
fn asm_rational_arc_last_weight_underflow_recovers_and_releases_storage() {
    for degree in [2, 64] {
        rational_arc_storage_boundary(degree, true, true);
    }
}

#[test]
fn asm_line_reversal_negates_direction_without_allocation() {
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("line fixture"),
    ));
    let reversed = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(-1.0, 0.0, 0.0))
            .expect("reversed line fixture"),
    ));
    crate::test_support::with_zero_context(|ctx| {
        let mut geometry = line.clone();
        let result = super::super::reverse_curve_geometry(ctx, &mut geometry);
        
                result.expect("fixed line reversal is free");
                assert_eq!(geometry, reversed);
            
    });
}
