// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, WeightedPole3};
use cadmpeg_ir::math::Point3;

fn weighted_source_case(count: usize, work: u64, operation: Option<&'static str>) {
    let rows: Vec<Vec<_>> = (0..2).map(|row| (0..count).map(|pole|
        FinitePoint3::new(Point3::new(f64::from(row),
            f64::from(u32::try_from(pole).unwrap()), 0.0)).unwrap()).collect()).collect();
    let weights: Vec<Vec<_>> = (0..2).map(|_| (0..count).map(|_| 1.0).collect()).collect();
    let before_rows = rows.clone();
    let before_weights = weights.clone();
    // All retry inputs are fixture setup before the measured session.
    let replays: Vec<_> = if operation.is_some() {
        (0..64).map(|_| (rows.clone(), weights.clone())).collect()
    } else { Vec::new() };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = u64::try_from(2 + 2 * count).unwrap();
    policy.limits.max_retained_bytes = u64::try_from(
        2 * std::mem::size_of::<Vec<WeightedPole3<FinitePoint3>>>()
            + 2 * count * std::mem::size_of::<WeightedPole3<FinitePoint3>>()).unwrap();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::pair_admitted_surface_poles(&ctx, rows, Some(weights),
        "test weighted outer rows", "test weighted row controls");
    if let Some(operation) = operation {
        let Err(CodecError::ResourceLimit(first)) = result else {
            panic!("expected the next weighted source visit to refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, operation);
        assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
        for (rows, weights) in replays {
            assert_eq!(rows, before_rows);
            assert_eq!(weights, before_weights);
            assert!(matches!(super::super::pair_admitted_surface_poles(
                &ctx, rows, Some(weights), "test weighted outer rows", "test weighted row controls"),
                Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(matches!(super::super::pair_admitted_surface_poles::<f64>(
                &ctx, Vec::new(), None, "test weighted outer rows", "test weighted row controls"),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    } else {
        let NurbsPoleGrid::Rational { rows } = result.unwrap().unwrap() else {
            panic!("expected the complete rational pole grid");
        };
        assert_eq!(rows.len(), 2);
        for (row, expected) in rows.iter().zip(before_rows) {
            assert_eq!(row.len(), count);
            for (pole, point) in row.iter().zip(expected) {
                assert_eq!(pole.point, point);
                assert_eq!(pole.weight.get(), 1.0);
            }
        }
        drop(rows);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn surface_weighted_first_row_refuses_before_any_pole() {
    for count in [4, 64] { weighted_source_case(count, 0, Some("iges surface weighted row traversal")); }
}

#[test]
fn surface_weighted_last_row_refuses_after_exact_completed_pole_population() {
    for count in [4, 64] { weighted_source_case(count, u64::try_from(count + 1).unwrap(),
        Some("iges surface weighted row traversal")); }
}

#[test]
fn surface_weighted_first_pole_refuses_after_one_row_visit() {
    for count in [4, 64] { weighted_source_case(count, 1, Some("iges surface weighted pole traversal")); }
}

#[test]
fn surface_weighted_last_pole_refuses_after_exact_completed_source_population() {
    for count in [4, 64] { weighted_source_case(count, u64::try_from(2 * count + 1).unwrap(),
        Some("iges surface weighted pole traversal")); }
}

#[test]
fn surface_weighted_grid_accepts_exact_row_and_pole_visits() {
    for count in [4, 64] { weighted_source_case(count, u64::try_from(2 * count + 2).unwrap(), None); }
}
