// SPDX-License-Identifier: Apache-2.0

use super::*;

fn cached_curve() -> Curve {
    Curve {
        id: CurveId::mint("creo:test:cache-curve#1").expect("identity"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
            record: Some(cadmpeg_ir::ids::UnknownId::mint("creo:test:cache-record#1").expect("identity")),
        }),
        source_object: None,
    }
}

#[test]
fn replacement_cache_releases_geometry_and_all_storage_at_drop() {
    let curve = cached_curve();
    let run = |cap, replacements| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut carriers = SourceUnitCarriers::for_decode(&ctx, None);
        let mut curve = curve.clone();
        for _ in 0..replacements {
            let geometry = curve.geometry.clone();
            carriers.replace_curve_geometry(&ctx, &mut curve, geometry)?;
            assert_eq!(carriers.curve_geometry(&curve)?, &curve.geometry);
        }
        drop(carriers);
        let _all_storage = ctx.reserve_scoped(cap, "test released source cache storage")?;
        Ok::<_, CodecError>(())
    };
    let two = crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, |cap| run(cap, 2));
    let many = crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, |cap| run(cap, 64));
    assert_eq!(two, many);
    run(two, 64).expect("replacement storage does not accumulate");
}

#[test]
fn source_surface_removal_releases_key_geometry_and_node_storage() {
    let surface = Surface {
        id: SurfaceId::mint("creo:test:cache-surface#1").expect("identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(cadmpeg_ir::ids::UnknownId::mint("creo:test:cache-record#1").expect("identity")),
        }),
        source_object: None,
    };
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut carriers = SourceUnitCarriers::for_decode(&ctx, None);
        let mut surface = surface.clone();
        let geometry = surface.geometry.clone();
        carriers.replace_surface_geometry(&ctx, &mut surface, geometry)?;
        assert_eq!(carriers.surface_geometry(&surface)?, &surface.geometry);
        carriers.remove_surface(&surface.id)?;
        let _all_storage = ctx.reserve_scoped(cap, "test removed source surface storage")?;
        Ok::<_, CodecError>(())
    };
    let cap = crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, run);
    run(cap).expect("removed surface releases storage before cache drop");
}

#[test]
fn source_cache_lookups_and_removal_propagate_work_refusals() {
    let curve = cached_curve();
    let surface = Surface {
        id: SurfaceId::mint("creo:test:cache-surface#1").expect("identity"),
        geometry: admission_plane(),
        source_object: None,
    };
    crate::test_support::assert_work_boundaries(&[
        "creo source curve geometry lookup",
        "creo source surface geometry lookup",
        "creo source surface removal",
    ], |ctx| {
        let mut carriers = SourceUnitCarriers::for_decode(ctx, None);
        let mut curve = curve.clone();
        let geometry = curve.geometry.clone();
        carriers.replace_curve_geometry(ctx, &mut curve, geometry)?;
        let mut surface = surface.clone();
        let geometry = surface.geometry.clone();
        carriers.replace_surface_geometry(ctx, &mut surface, geometry)?;
        assert_eq!(carriers.curve_geometry(&curve)?, &curve.geometry);
        assert_eq!(carriers.surface_geometry(&surface)?, &surface.geometry);
        carriers.remove_surface(&surface.id)
    });
}
