// SPDX-License-Identifier: Apache-2.0
//! Surface comparison prefixes and carrier equality.

use super::super::same_surface_geometry;
use crate::test_support::{resource_refusal_at, with_decode_context, with_decode_context_over};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{ProceduralSurfaceId, UnknownId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::transform::Transform;

fn surface(
    rational: bool,
    first_knot: f64,
    first_pole: f64,
    first_weight: f64,
    reversed: bool,
) -> SurfaceGeometry {
    with_decode_context(|ctx| {
        let mut knots = vec![first_knot];
        knots.extend((0..64).map(|i| cadmpeg_core::convert::f64_from_index(i).unwrap()));
        knots.push(63.0);
        let mut points = vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 64];
        points[0][0].x = first_pole;
        let weights = rational.then(|| {
            let mut weights = vec![vec![1.0; 2]; 64];
            weights[0][0] = first_weight;
            weights
        });
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                ctx,
                NurbsSurfaceAxis::new(1, knots, false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                NurbsSurfaceLanes::new(points, weights),
                reversed,
            )
            .unwrap()
            .unwrap(),
        ))
    })
}

fn cached(geometry: &SurfaceGeometry) -> SurfaceGeometry {
    let SurfaceGeometry::Solved(geometry) = geometry else {
        panic!("solved fixture")
    };
    SurfaceGeometry::Procedural {
        construction: ProceduralSurfaceId::mint("test:model:entity#surface-construction").unwrap(),
        cache: Some(geometry.clone()),
    }
}

fn placed(geometry: &SurfaceGeometry) -> SurfaceGeometry {
    let SurfaceGeometry::Solved(geometry) = geometry else {
        panic!("solved fixture")
    };
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        PlacedSurface::try_new(Box::new(geometry.clone()), Transform::identity()).unwrap(),
    ))
}

#[test]
fn surface_comparison_charges_equal_lanes_and_only_the_read_prefix() {
    for rational in [false, true] {
        let first = surface(rational, 0.0, 0.0, 1.0, false);
        let unequal_knot = surface(rational, -1.0, 0.0, 1.0, false);
        let unequal_pole = surface(rational, 0.0, 1.0, 1.0, false);
        for (second, work, matches) in [
            (&first, 262, true),
            (&unequal_knot, 1, false),
            (&unequal_pole, 72, false),
        ] {
            with_decode_context_over(
                &[],
                |policy| policy.limits.max_work_units = work,
                |ctx| {
                    assert_eq!(
                        same_surface_geometry(ctx, &first, second, "nx test surface comparison")
                            .unwrap(),
                        matches
                    );
                    assert_eq!(ctx.resource_refusal(), None);
                    // Read all admitted units: 66 u knots + 4 v knots + 64
                    // rows + 128 poles for equality; one row and one pole
                    // after the knots for a first-pole mismatch.
                    let limit = ctx
                        .charge_work_limit(1, "nx test surface comparison census")
                        .unwrap_err();
                    assert_eq!(limit.used, work);
                },
            );
        }
        for geometry in [first.clone(), placed(&first)] {
            resource_refusal_at(
                &[],
                ResourceDimension::WorkUnits,
                "nx test surface comparison",
                |ctx| {
                    Ok(same_surface_geometry(
                        ctx,
                        &geometry,
                        &geometry,
                        "nx test surface comparison",
                    )?)
                },
            );
        }
        let cached = cached(&first);
        let SurfaceGeometry::Procedural { construction, .. } = &cached else {
            panic!("cached fixture")
        };
        let work = cadmpeg_core::decode::u64_from_index(construction.as_str().len()) + 262;
        with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = work,
            |ctx| {
                assert!(same_surface_geometry(
                    ctx,
                    &cached,
                    &cached,
                    "nx test cached surface comparison"
                )
                .unwrap());
                let limit = ctx
                    .charge_work_limit(1, "nx test cached comparison census")
                    .unwrap_err();
                assert_eq!(limit.used, work);
            },
        );
        let reversed = surface(rational, 0.0, 0.0, 1.0, true);
        with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                assert!(!same_surface_geometry(
                    ctx,
                    &first,
                    &reversed,
                    "nx test surface comparison"
                )
                .unwrap());
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
        if rational {
            let unequal_weight = surface(true, 0.0, 0.0, 2.0, false);
            with_decode_context_over(
                &[],
                |policy| policy.limits.max_work_units = 72,
                |ctx| {
                    assert!(!same_surface_geometry(
                        ctx,
                        &first,
                        &unequal_weight,
                        "nx test surface comparison"
                    )
                    .unwrap());
                    assert_eq!(ctx.resource_refusal(), None);
                },
            );
        }
    }
}

#[test]
fn surface_comparison_preserves_payload_equality_across_carrier_forms() {
    let polynomial = surface(false, 0.0, 0.0, 1.0, false);
    let rational = surface(true, 0.0, 0.0, 1.0, false);
    let mut carriers = vec![
        polynomial.clone(),
        rational.clone(),
        surface(false, -1.0, 0.0, 1.0, false),
        surface(false, 0.0, 1.0, 1.0, false),
        surface(true, 0.0, 0.0, 2.0, false),
        surface(false, 0.0, 0.0, 1.0, true),
        placed(&polynomial),
        placed(&rational),
        cached(&polynomial),
        cached(&rational),
        SurfaceGeometry::Procedural {
            construction: ProceduralSurfaceId::mint("test:model:entity#surface-construction")
                .unwrap(),
            cache: None,
        },
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(UnknownId::mint("test:model:entity#surface-record").unwrap()),
        }),
    ];
    let mut other_construction = cached(&polynomial);
    if let SurfaceGeometry::Procedural { construction, .. } = &mut other_construction {
        *construction = ProceduralSurfaceId::mint("test:model:entity#other-construction").unwrap();
    }
    carriers.push(other_construction);
    with_decode_context(|ctx| {
        for first in &carriers {
            for second in &carriers {
                assert_eq!(
                    same_surface_geometry(ctx, first, second, "nx test carrier equality").unwrap(),
                    first == second,
                );
            }
        }
    });
}
