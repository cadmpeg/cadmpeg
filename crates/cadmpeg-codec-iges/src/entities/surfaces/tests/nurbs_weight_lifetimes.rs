// SPDX-License-Identifier: Apache-2.0

use crate::entities::geometry::SourceSequences;
use crate::global::ProjectedGlobal;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::CadIr;
use std::mem::{align_of, size_of};

const U_COUNT: usize = 512;
const V_COUNT: usize = 2;

fn inputs(
    rational: bool,
) -> (
    crate::directory::DirectoryEntry,
    ParameterRecord,
    ProjectedGlobal,
) {
    let entry = crate::test_support::directory_target(1, 128);
    let mut values = vec![
        128,
        i64::try_from(U_COUNT - 1).unwrap(),
        1,
        1,
        1,
        0,
        0,
        i64::from(!rational),
        0,
        0,
    ];
    values.extend([0, 0]);
    values.extend((1..U_COUNT).map(|index| i64::try_from(index).unwrap()));
    values.push(i64::try_from(U_COUNT - 1).unwrap());
    values.extend([0, 0, 1, 1]);
    values.extend((0..U_COUNT * V_COUNT).map(|index| {
        if rational {
            1 + i64::try_from(index % 2).unwrap()
        } else {
            1
        }
    }));
    for v in 0..V_COUNT {
        for u in 0..U_COUNT {
            values.extend([i64::try_from(u).unwrap(), i64::try_from(v).unwrap(), 0]);
        }
    }
    values.extend([0, i64::try_from(U_COUNT - 1).unwrap(), 0, 1]);
    let record = ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        values.len(),
        values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    );
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).unwrap();
        let (global, _, _) = crate::global::parse(&scan, ctx).unwrap();
        global.length_context().unwrap()
    });
    (entry, record, global)
}

fn prefix_bytes() -> u64 {
    // Two singleton u32-to-borrowed-value trees, then the surviving knot
    // vectors. The equal polynomial weights have no later reader.
    let node = 11 * (size_of::<u32>() + size_of::<&ParameterRecord>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<u32>()
            .max(align_of::<&ParameterRecord>())
            .max(align_of::<usize>());
    u64_from_index(2 * node + (U_COUNT + 2 + V_COUNT + 2) * size_of::<f64>())
}

fn polynomial_boundary(exact_phase: bool) {
    let (entry, record, global) = inputs(false);
    let outer = u64_from_index(U_COUNT * size_of::<Vec<FinitePoint3>>());
    let peak = prefix_bytes() + outer;
    let cap = peak - u64::from(!exact_phase);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    if exact_phase {
        // Two index entries, both knot vectors, all native weights and
        // the outer grid slots. The first two-control row is the next item.
        policy.limits.max_collection_items =
            u64_from_index(2 + (U_COUNT + 2) + (V_COUNT + 2) + U_COUNT * V_COUNT + U_COUNT);
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut ir = CadIr::empty();
    let mut output = ctx.reserve_scoped(0, "test surface output").unwrap();
    let error = output
        .with_storage(|| {
            super::super::project(
                &mut ir,
                std::slice::from_ref(&entry),
                std::slice::from_ref(&record),
                &global,
                &ctx,
                &mut sequences,
            )
            .map(drop)
        })
        .unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected actual phase refusal");
    };
    if exact_phase {
        let items = policy.limits.max_collection_items;
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "iges NURBS surface pole row controls");
        assert_eq!(
            (first.limit, first.used, first.additional),
            (items, items, u64_from_index(V_COUNT))
        );
    } else {
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.operation, "iges NURBS surface pole rows");
        assert_eq!(
            (first.limit, first.used, first.additional),
            (cap, prefix_bytes(), outer)
        );
    }
    for _ in 0..64 {
        for directory in [std::slice::from_ref(&entry), &[]] {
            assert!(
                matches!(super::super::project(&mut ir, directory, std::slice::from_ref(&record),
                &global, &ctx, &mut sequences), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        }
    }
    assert_eq!(ir, CadIr::empty());
    drop(ir);
    drop(sequences);
    drop(output);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn polynomial_surface_releases_weights_before_outer_grid_one_short_refusal() {
    polynomial_boundary(false);
}

#[test]
fn polynomial_surface_exact_grid_peak_reaches_first_control_row() {
    polynomial_boundary(true);
}

#[test]
fn surface_weight_lifetime_preserves_polynomial_and_rational_grids() {
    for rational in [false, true] {
        let (entry, record, global) = inputs(rational);
        let before = record.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let mut ir = CadIr::empty();
        let mut output = ctx.reserve_scoped(0, "test surface output").unwrap();
        let projection = output
            .with_storage(|| {
                super::super::project(
                    &mut ir,
                    std::slice::from_ref(&entry),
                    std::slice::from_ref(&record),
                    &global,
                    &ctx,
                    &mut sequences,
                )
            })
            .unwrap();
        assert!(projection.losses.is_empty());
        assert!(projection.decoded.contains(&1));
        assert_eq!(ir.model.surfaces.len(), 1);
        let surface = &ir.model.surfaces[0];
        assert_eq!(surface.id.as_str(), "iges:model:surface#D1");
        assert!(matches!(
            surface.geometry,
            SurfaceGeometry::Procedural { cache: Some(_), .. }
        ));
        let Some(SolvedSurfaceGeometry::Nurbs(grid)) = surface.geometry.solved_cache() else {
            panic!("expected spline grid cache");
        };
        assert_eq!((grid.u_count(), grid.v_count()), (U_COUNT, V_COUNT));
        for u in 0..U_COUNT {
            for v in 0..V_COUNT {
                assert_eq!(
                    grid.pole(u, v).unwrap().get(),
                    cadmpeg_ir::math::Point3::new(
                        f64::from(u32::try_from(u).unwrap()),
                        f64::from(u32::try_from(v).unwrap()),
                        0.0
                    )
                );
                assert_eq!(
                    grid.weight(u, v).map(cadmpeg_ir::scalar::NonZeroReal::get),
                    rational.then(|| f64::from(u32::try_from(1 + (v * U_COUNT + u) % 2).unwrap()))
                );
            }
        }
        assert_eq!(record, before);
        drop(projection);
        drop(ir);
        drop(sequences);
        drop(output);
        let free = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test all scratch released",
            )
            .unwrap();
        drop(free);
        ctx.finish_session().unwrap();
    }
}

fn grid_row_collection_boundary(rational: bool, completed_rows: usize) {
    let (entry, record, global) = inputs(rational);
    let before = record.clone();
    // Two singleton indexes, both knot vectors, all native weights and the
    // outer row slots precede the two controls in each completed row.
    let items = u64_from_index(
        2 + (U_COUNT + 2) + (V_COUNT + 2) + U_COUNT * V_COUNT + U_COUNT + completed_rows * V_COUNT,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut ir = CadIr::empty();
    let mut output = ctx.reserve_scoped(0, "test surface output").unwrap();
    let error = output
        .with_storage(|| {
            super::super::project(
                &mut ir,
                std::slice::from_ref(&entry),
                std::slice::from_ref(&record),
                &global,
                &ctx,
                &mut sequences,
            )
            .map(drop)
        })
        .unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected row admission refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(
        first.operation,
        if rational {
            "iges NURBS surface weighted row controls"
        } else {
            "iges NURBS surface pole row controls"
        }
    );
    assert_eq!(
        (first.limit, first.used, first.additional),
        (items, items, u64_from_index(V_COUNT))
    );
    assert_eq!(ir, CadIr::empty());
    assert_eq!(record, before);
    for _ in 0..64 {
        for directory in [std::slice::from_ref(&entry), &[]] {
            assert!(matches!(super::super::project(&mut ir, directory,
                std::slice::from_ref(&record), &global, &ctx, &mut sequences),
                Err(CodecError::ResourceLimit(last)) if last == first));
            assert_eq!(ir, CadIr::empty());
            assert_eq!(record, before);
        }
    }
    drop(ir);
    drop(sequences);
    drop(output);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn polynomial_surface_first_row_refuses_before_controls() {
    grid_row_collection_boundary(false, 0);
}

#[test]
fn polynomial_surface_last_row_refuses_after_completed_controls() {
    grid_row_collection_boundary(false, U_COUNT - 1);
}

#[test]
fn rational_surface_first_row_refuses_before_controls() {
    grid_row_collection_boundary(true, 0);
}

#[test]
fn rational_surface_last_row_refuses_after_completed_controls() {
    grid_row_collection_boundary(true, U_COUNT - 1);
}
