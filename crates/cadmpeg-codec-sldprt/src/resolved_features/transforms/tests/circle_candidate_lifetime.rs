//! Admission and geometry controls for multiple circle-transform candidates.

use crate::resolved_features::grid::{GridCoordinate, GridPoint};
use crate::resolved_features::transforms::{
    dimensioned_circle_surface_transforms, MarkerTransform,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use std::collections::BTreeSet;

fn fixture() -> (Sketch, Vec<Surface>) {
    let sketch = Sketch {
        id: cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#circle-candidates").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    };
    let surfaces = (0..3)
        .map(|index| Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!("test:model:surface#circle-{index}"))
                .unwrap(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(f64::from(index), 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    1.0,
                )
                .unwrap(),
            )),
            source_object: None,
        })
        .collect();
    (sketch, surfaces)
}

const CIRCLES: [((i64, i64), GridCoordinate); 2] = [
    ((10, 0), GridCoordinate::Cell(1)),
    ((11, 0), GridCoordinate::Cell(1)),
];

// Core collect::tree_node_bytes admits 11 lanes, 16 pointer-width slots,
// and two alignment pads per node. All four lookup tables have <=3 keys.
fn tree_node<K, V>() -> u64 {
    let alignment = std::mem::align_of::<K>()
        .max(std::mem::align_of::<V>())
        .max(std::mem::align_of::<usize>());
    cadmpeg_core::decode::u64_from_index(
        11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * alignment,
    )
}

fn materialized_peak() -> (u64, u64) {
    let lookups = tree_node::<GridCoordinate, ()>()
        + tree_node::<GridCoordinate, BTreeSet<GridPoint>>()
        + tree_node::<GridPoint, ()>()
        + tree_node::<GridPoint, &BTreeSet<GridPoint>>();
    // Each aligned-axis score has four translations; each swapped-axis score
    // has six and admits two nodes under (n-1)/5+1. The seven nonidentity
    // scores append 3*4 + 4*6 = 36 rows. The final swapped score grows the
    // amortized vector from 32 to 64 rows, holding its two tree-node bound
    // while core linear_growth reserves the old 32 rows as copy overlap.
    let before_growth = lookups
        + 2 * tree_node::<(i64, i64), usize>()
        + cadmpeg_core::decode::u64_from_index(
            64 * std::mem::size_of::<(MarkerTransform, usize)>(),
        );
    let overlap =
        cadmpeg_core::decode::u64_from_index(32 * std::mem::size_of::<(MarkerTransform, usize)>());
    (before_growth, overlap)
}

#[test]
fn circle_candidates_preserve_all_matching_frames_at_source_derived_peak() {
    let (sketch, surfaces) = fixture();
    let (before_growth, overlap) = materialized_peak();
    let need = before_growth + overlap;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = need;
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(8 * std::mem::size_of::<MarkerTransform>());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let candidates =
        dimensioned_circle_surface_transforms(&ctx, &sketch, &surfaces, &CIRCLES, 1.0).unwrap();
    // The positive-u/negative-v axes have two translations. Negative-u
    // axes have two translations for each v sign. No translation is zero.
    assert_eq!(candidates.len(), 6);
    let mut bindings = candidates
        .iter()
        .map(|candidate| {
            (
                candidate.apply(CIRCLES[0].0).unwrap(),
                candidate.apply(CIRCLES[1].0).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    bindings.sort_unstable();
    assert_eq!(
        bindings,
        [
            ((0, 0), (1, 0)),
            ((1, 0), (0, 0)),
            ((1, 0), (0, 0)),
            ((1, 0), (2, 0)),
            ((2, 0), (1, 0)),
            ((2, 0), (1, 0)),
        ]
    );
    // Returned candidate backing is retained; all materialized query storage
    // must be reusable while the returned values remain live.
    let reuse = ctx
        .reserve_scoped(need, "reuse circle candidate query storage")
        .unwrap();
    drop((candidates, reuse));
}

#[test]
fn circle_candidates_refuse_live_scoring_storage_and_stay_sticky() {
    let (sketch, surfaces) = fixture();
    let (before_growth, _) = materialized_peak();
    let first_score = before_growth
        - cadmpeg_core::decode::u64_from_index(
            64 * std::mem::size_of::<(MarkerTransform, usize)>(),
        )
        - tree_node::<(i64, i64), usize>();
    let mut policy = DecodePolicy::service();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "score SLDPRT compatible marker transforms",
        |cap| {
            let arena = DecodeArena::new();
            let mut bounded = DecodePolicy::service();
            bounded.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &bounded).unwrap();
            dimensioned_circle_surface_transforms(&ctx, &sketch, &surfaces, &CIRCLES, 1.0)
        },
    );
    let CodecError::ResourceLimit(boundary) = &error else {
        panic!("scoring storage boundary");
    };
    assert_eq!(boundary.used + boundary.additional, first_score);
    assert_eq!(
        boundary.operation,
        "score SLDPRT compatible marker transforms"
    );
    // Replay with the located finite limit to check the sticky failure on this context.
    let arena = DecodeArena::new();
    policy.limits.max_materialized_bytes = boundary.limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let repeated =
        dimensioned_circle_surface_transforms(&ctx, &sketch, &surfaces, &CIRCLES, 1.0).unwrap_err();
    assert!(matches!(&repeated, CodecError::ResourceLimit(limit) if limit == boundary));
}

#[test]
fn circle_candidates_refuse_scored_vector_copy_overlap() {
    const OPERATION: &str = "score SLDPRT compatible marker transforms";
    let (sketch, surfaces) = fixture();
    let (before_growth, overlap) = materialized_peak();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        OPERATION,
        |cap| {
            // Raise actual refusals to locate the final live peak. Capacity growth
            // and relocation have the same size and operation, so a size probe
            // cannot distinguish them. The helper replays only the final boundary.
            let mut limit = if cap == u64::MAX { 0 } else { cap };
            let mut peak = None;
            loop {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = dimensioned_circle_surface_transforms(
                    &ctx, &sketch, &surfaces, &CIRCLES, 1.0,
                );
                match result {
                    Err(CodecError::ResourceLimit(refusal)) if cap == u64::MAX => {
                        assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
                        let need = refusal.used.checked_add(refusal.additional).unwrap();
                        assert!(need > limit);
                        limit = need;
                        peak = Some(CodecError::ResourceLimit(refusal));
                    }
                    Ok(_) if cap == u64::MAX => {
                        break Err(peak.expect("scoring has a live allocation peak"));
                    }
                    result => break result,
                }
            }
        },
    );
    let CodecError::ResourceLimit(boundary) = error else {
        panic!("scored-vector relocation must refuse");
    };
    assert_eq!(boundary.used, before_growth);
    assert_eq!(boundary.additional, overlap);
    assert_eq!(boundary.limit, before_growth + overlap - 1);
}

#[test]
fn circle_candidate_scoped_calls_release_score_and_output_backing() {
    let (sketch, surfaces) = fixture();
    let (before_growth, overlap) = materialized_peak();
    let need = before_growth + overlap;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = need;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let (candidates, storage) = ctx
            .with_scoped_storage("circle output workspace", || {
                dimensioned_circle_surface_transforms(&ctx, &sketch, &surfaces, &CIRCLES, 1.0)
            })
            .unwrap();
        assert_eq!(candidates.len(), 6);
        drop(candidates);
        drop(storage);
        ctx.reserve_scoped(need, "reuse all circle candidate storage")
            .unwrap();
    }
    ctx.finish_session().unwrap();
}
