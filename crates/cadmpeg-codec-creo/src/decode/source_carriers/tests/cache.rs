// SPDX-License-Identifier: Apache-2.0

use super::*;

fn cached_curve() -> Curve {
    Curve {
        id: CurveId::mint("creo:test:cache-curve#1").expect("identity"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
            record: Some(
                cadmpeg_ir::ids::UnknownId::mint("creo:test:cache-record#1").expect("identity"),
            ),
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
    let two = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| run(cap, 2),
    );
    let many = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| run(cap, 64),
    );
    assert_eq!(two, many);
    run(two, 64).expect("replacement storage does not accumulate");
}

#[test]
fn source_surface_removal_releases_key_geometry_and_node_storage() {
    let surface = Surface {
        id: SurfaceId::mint("creo:test:cache-surface#1").expect("identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(
                cadmpeg_ir::ids::UnknownId::mint("creo:test:cache-record#1").expect("identity"),
            ),
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
    let cap =
        crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, run);
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
    crate::test_support::assert_work_boundaries(
        &[
            "creo source curve geometry lookup",
            "creo source surface geometry lookup",
            "creo source surface removal",
        ],
        |ctx| {
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
        },
    );
}

#[test]
fn cached_model_positions_skip_unrelated_arena_rows() {
    let target_curve = cached_curve();
    let target_surface = Surface {
        id: SurfaceId::mint("creo:test:cache-surface#1").expect("identity"),
        geometry: admission_plane(),
        source_object: None,
    };
    let run = |cap, prefix| {
        let mut ir = CadIr::empty();
        ir.model.curves = Vec::with_capacity(prefix + 1);
        let mut other_curve = target_curve.clone();
        other_curve.id = CurveId::mint("creo:test:other-curve#1").expect("identity");
        ir.model.curves.resize(prefix, other_curve);
        ir.model.surfaces = Vec::with_capacity(prefix + 1);
        let mut other_surface = target_surface.clone();
        other_surface.id = SurfaceId::mint("creo:test:other-surface#1").expect("identity");
        ir.model.surfaces.resize(prefix, other_surface);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut carriers = SourceUnitCarriers::for_decode(&ctx, None);
        carriers.admit_curve(&ctx, &mut ir, target_curve.clone())?;
        carriers.admit_surface(&ctx, &mut ir, target_surface.clone())?;
        let geometry = carriers.source_curve_by_id(
            &ctx,
            &ir,
            &target_curve.id,
            "test cached curve search",
            "test cached curve comparison",
        )?;
        assert_eq!(geometry, Some(&target_curve.geometry));
        carriers.admit_pcurve(&ctx, &mut ir, admission_pcurve(), &target_surface.id)?;
        assert_eq!(ir.model.pcurves, vec![admission_pcurve()]);
        Ok::<_, CodecError>(())
    };
    let small =
        crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| {
            run(cap, 0)
        });
    let large =
        crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| {
            run(cap, 64)
        });
    assert_eq!(small, large);
    run(small, 64).expect("cached model positions avoid unrelated rows");
}

#[test]
fn cached_curve_positions_validate_arena_mutations_and_latest_source_geometry() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut carriers = SourceUnitCarriers::for_decode(ctx, None);
        let mut ir = CadIr::empty();
        let curve = cached_curve();
        carriers.admit_curve(ctx, &mut ir, curve.clone())?;
        let replacement = CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
        carriers.replace_curve_geometry(ctx, &mut ir.model.curves[0], replacement.clone())?;
        let get = |ir: &CadIr| {
            carriers
                .source_curve_by_id(
                    ctx,
                    ir,
                    &curve.id,
                    "test changed curve search",
                    "test changed curve comparison",
                )
                .map(Option::<&CurveGeometry>::cloned)
        };
        assert_eq!(get(&ir)?, Some(replacement));
        let mut other = curve.clone();
        other.id = CurveId::mint("creo:test:other-curve#1").expect("identity");
        ir.model.curves.insert(0, other);
        assert_eq!(get(&ir)?, Some(ir.model.curves[1].geometry.clone()));
        ir.model.curves.clear();
        assert_eq!(get(&ir)?, None);
        Ok::<_, CodecError>(())
    })
    .expect("source cache does not imply model membership");
}

#[test]
fn cached_surface_positions_fall_back_after_arena_mutation() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut carriers = SourceUnitCarriers::for_decode(ctx, None);
        let mut ir = CadIr::empty();
        let surface = Surface {
            id: SurfaceId::mint("creo:test:cache-surface#1").expect("identity"),
            geometry: admission_plane(),
            source_object: None,
        };
        carriers.admit_surface(ctx, &mut ir, surface.clone())?;
        let mut other = surface.clone();
        other.id = SurfaceId::mint("creo:test:other-surface#1").expect("identity");
        ir.model.surfaces.insert(0, other);
        carriers.admit_pcurve(ctx, &mut ir, admission_pcurve(), &surface.id)?;
        assert_eq!(ir.model.pcurves, vec![admission_pcurve()]);
        ir.model.surfaces.clear();
        let error = carriers
            .admit_pcurve(ctx, &mut ir, admission_pcurve(), &surface.id)
            .expect_err("surface must exist in the model");
        assert!(matches!(error, CodecError::Malformed(_)));
        assert_eq!(ir.model.pcurves, vec![admission_pcurve()]);
        Ok::<_, CodecError>(())
    })
    .expect("source cache does not imply owning surface membership");
}
