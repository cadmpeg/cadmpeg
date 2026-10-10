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
    let before_growth = lookups + 2 * tree_node::<(i64, i64), usize>();
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
fn circle_candidates_refuse_exact_scored_copy_overlap_and_stay_sticky() {
    let (sketch, surfaces) = fixture();
    let (before_growth, overlap) = materialized_peak();
    let need = before_growth + overlap;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = need - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        dimensioned_circle_surface_transforms(&ctx, &sketch, &surfaces, &CIRCLES, 1.0).unwrap_err();
    assert!(matches!(
        &error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "score SLDPRT compatible marker transforms"
                && limit.used == before_growth
                && limit.additional == overlap
    ));
    assert!(matches!(
        ctx.reserve_scoped(0, "refusal stays sticky"),
        Err(CodecError::ResourceLimit(limit))
            if matches!(&error, CodecError::ResourceLimit(original) if original == &limit)
    ));
}
