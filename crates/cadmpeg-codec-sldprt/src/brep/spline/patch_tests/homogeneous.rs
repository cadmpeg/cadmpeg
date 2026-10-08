// SPDX-License-Identifier: Apache-2.0
//! Visited pole and row admission for homogeneous patch values.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsPoles3, WeightedPole3};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::NonZeroReal;

use super::super::patch::{append_homogeneous_pole, homogeneous_grid_poles, homogeneous_poles};

fn point(x: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3::new(x, 2.0, 3.0)).unwrap()
}

#[test]
fn homogeneous_scalar_arithmetic_needs_no_work() {
    for weight in [None, Some(2.0)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut values = Vec::new();
        assert_eq!(append_homogeneous_pole(&ctx, &mut values, point(1.0), weight, 0.5).unwrap(), Some(()));
        assert_eq!(values, if weight.is_some() { vec![1.0, 2.0, 3.0, 2.0] } else { vec![0.5, 1.0, 1.5] });
        ctx.finish_session().unwrap();
    }
}

#[test]
fn homogeneous_traversal_stops_before_the_unvisited_pole_tail() {
    for rational in [false, true] {
        let points = vec![point(1.0e300); 4096];
        let weighted = points.iter().map(|&point| WeightedPole3 { point, weight: NonZeroReal::new(2.0).unwrap() }).collect::<Vec<_>>();
        let curve = if rational { NurbsPoles3::Rational { points: weighted.clone() } } else { NurbsPoles3::Polynomial { points: points.clone() } };
        let grid = if rational { NurbsPoleGrid::Rational { rows: vec![weighted.clone(), weighted] } } else { NurbsPoleGrid::Polynomial { rows: vec![points.clone(), points] } };
        for surface in [false, true] {
            // One point visit, plus one row visit for a surface. Overflow
            // rejects before output allocation and before any later visit.
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = if surface { 2 } else { 1 };
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let output = if surface { homogeneous_grid_poles(&ctx, &grid, 1.0e300) } else { homogeneous_poles(&ctx, &curve, 1.0e300) };
            assert_eq!(output.unwrap(), None);
            ctx.finish_session().unwrap();

            let arena = DecodeArena::new();
            policy.limits.max_work_units -= 1;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let output = if surface { homogeneous_grid_poles(&ctx, &grid, 1.0e300) } else { homogeneous_poles(&ctx, &curve, 1.0e300) };
            let Err(CodecError::ResourceLimit(limit)) = output else { panic!("point visit refusal"); };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "form Parasolid homogeneous patch pole");
            assert_eq!(limit.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        }
    }
}

#[test]
fn empty_homogeneous_rosters_need_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for poles in [NurbsPoles3::Polynomial { points: Vec::new() }, NurbsPoles3::Rational { points: Vec::new() }] {
        assert_eq!(homogeneous_poles(&ctx, &poles, 1.0).unwrap(), Some(Vec::new()));
    }
    for poles in [NurbsPoleGrid::Polynomial { rows: Vec::new() }, NurbsPoleGrid::Rational { rows: Vec::new() }] {
        assert_eq!(homogeneous_grid_poles(&ctx, &poles, 1.0).unwrap(), Some(Vec::new()));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn homogeneous_grid_visits_empty_rows_before_their_points() {
    let rows = vec![Vec::new(); 4096];
    let poles = NurbsPoleGrid::Polynomial { rows };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = homogeneous_grid_poles(&ctx, &poles, 1.0) else {
        panic!("third row visit refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "visit Parasolid homogeneous patch row");
    assert_eq!((limit.used, limit.additional), (2, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn homogeneous_output_uses_pole_visits_and_vector_growth_once() {
    let poles = NurbsPoles3::Polynomial { points: vec![point(1.0), point(4.0)] };
    let grid = NurbsPoleGrid::Polynomial { rows: vec![vec![point(1.0), point(4.0)]] };
    for surface in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two pole visits, one row visit for the grid, and the core vector
        // growth's move of its initial four f64 slots (32 bytes).
        policy.limits.max_work_units = 34 + u64::from(surface);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let output = if surface { homogeneous_grid_poles(&ctx, &grid, 0.5) } else { homogeneous_poles(&ctx, &poles, 0.5) };
        assert_eq!(output.unwrap(), Some(vec![0.5, 1.0, 1.5, 2.0, 1.0, 1.5]));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn rejected_homogeneous_prefix_releases_output_scratch() {
    for rational in [false, true] {
        let points = vec![point(1.0), point(1.0e300)];
        let weighted = points.iter().map(|&point| WeightedPole3 { point, weight: NonZeroReal::new(2.0).unwrap() }).collect::<Vec<_>>();
        let curve = if rational { NurbsPoles3::Rational { points: weighted.clone() } } else { NurbsPoles3::Polynomial { points: points.clone() } };
        let grid = if rational { NurbsPoleGrid::Rational { rows: vec![weighted] } } else { NurbsPoleGrid::Polynomial { rows: vec![points] } };
        for surface in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 2 + u64::from(surface);
            policy.limits.max_retained_bytes = 0;
            // The first pole fills the core Vec's four initial f64 slots.
            policy.limits.max_materialized_bytes = 4 * 8;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let output = if surface { homogeneous_grid_poles(&ctx, &grid, 1.0e300) } else { homogeneous_poles(&ctx, &curve, 1.0e300) };
            assert_eq!(output.unwrap(), None);
            drop(ctx.reserve_scoped(4 * 8, "test released homogeneous prefix").unwrap());
            ctx.finish_session().unwrap();
        }
    }
}
