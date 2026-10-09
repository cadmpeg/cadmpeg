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

fn cached_surface(id: &'static str, record: &'static str) -> Surface {
    Surface {
        id: SurfaceId::mint(id).expect("identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(cadmpeg_ir::ids::UnknownId::mint(record).expect("identity")),
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
fn source_surface_node_storage_stays_live_until_last_removal() {
    const PROBE_OPERATION: &str = "test source surface live storage probe";

    let surface_a = cached_surface(
        "creo:test:cache-surface-a#1",
        "creo:test:cache-record-a#1",
    );
    let surface_b = cached_surface(
        "creo:test:cache-surface-b#1",
        "creo:test:cache-record-b#1",
    );
    let run = |cap: u64,
               include_a: bool,
               remove_a: bool,
               remove_b: bool,
               probe_bytes: Option<u64>| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut carriers = SourceUnitCarriers::for_decode(&ctx, None);
        let mut ir = CadIr::empty();
        if include_a {
            carriers.admit_surface(&ctx, &mut ir, surface_a.clone())?;
            assert_eq!(carriers.surface_geometry(&surface_a)?, &surface_a.geometry);
        }
        carriers.admit_surface(&ctx, &mut ir, surface_b.clone())?;
        assert_eq!(carriers.surface_geometry(&surface_b)?, &surface_b.geometry);
        if remove_a {
            carriers.remove_surface(&surface_a.id)?;
        }
        if !remove_b {
            assert_eq!(carriers.surface_geometry(&surface_b)?, &surface_b.geometry);
        }
        if remove_b {
            carriers.remove_surface(&surface_b.id)?;
        }
        if let Some(probe_bytes) = probe_bytes {
            let _probe = ctx.reserve_scoped(probe_bytes, PROBE_OPERATION)?;
        }
        Ok::<_, CodecError>(())
    };

    let two_entry_peak = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| run(cap, true, true, false, None),
    );
    let one_entry_peak = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        None,
        |cap| run(cap, false, false, false, None),
    );
    let probe_bytes = two_entry_peak
        .max(one_entry_peak)
        .checked_add(1)
        .expect("probe exceeds setup peak");
    let measured_live_bytes = |include_a, remove_a, remove_b| {
        let below_probe = crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            Some(PROBE_OPERATION),
            |cap| run(cap, include_a, remove_a, remove_b, Some(probe_bytes)),
        );
        below_probe
            .checked_add(1)
            .expect("probe boundary")
            .checked_sub(probe_bytes)
            .expect("probe exceeds the setup peak")
    };

    let after_removing_a = measured_live_bytes(true, true, false);
    let with_b_only = measured_live_bytes(false, false, false);
    assert!(after_removing_a > 0);
    assert_eq!(after_removing_a, with_b_only);
    assert_eq!(measured_live_bytes(true, true, true), 0);
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

#[test]
fn owned_sketch_nurbs_scaling_preserves_source_and_refuses_overflow_before_insertion() {
    let geometry = |x: f64, rational: bool| {
        SketchGeometry::nurbs(
            cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![cadmpeg_ir::math::Point2::new(x, 0.0); 2],
                rational.then(|| vec![1.0, 2.0]),
                false,
            )
            .expect("fixture construction admitted")
            .expect("finite source curve"),
        )
    };
    for rational in [false, true] {
        for x in [1.0, f64::MAX] {
            let source = geometry(x, rational);
            let entity = SketchEntity::new(
                cadmpeg_ir::sketches::SketchEntityId::mint("creo:test:owned-nurbs#1").expect("entity ID"),
                cadmpeg_ir::sketches::SketchId::mint("creo:test:sketch#1").expect("sketch ID"),
                source.clone(),
            );
            let expected_id = entity.id().clone();
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut carriers = SourceUnitCarriers::for_decode(&ctx, PositiveReal::new(2.0));
            let mut ir = CadIr::empty();
            let result = carriers.admit_sketch_entities(&ctx, &mut ir, vec![entity]);
            if x == 1.0 {
                result.expect("finite scaling admitted");
                assert_eq!(ir.model.sketch_entities.len(), 1);
                let actual = &ir.model.sketch_entities[0];
                assert_eq!(actual.id(), &expected_id);
                assert_eq!(actual.geometry, geometry(2.0, rational));
                assert_eq!(carriers.sketch_geometry(actual).expect("source lookup"), &source);
            } else {
                assert!(matches!(result, Err(CodecError::NotImplemented(_))));
                assert!(ir.model.sketch_entities.is_empty());
                assert!(carriers.sketch_entities.is_empty());
            }
            assert_eq!(ctx.resource_refusal(), None);
        }
    }
}
