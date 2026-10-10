// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::document::admission::StandardAdmission;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

fn curves(count: usize) -> (Model, Vec<(CurveId, ProceduralCurve)>) {
    let mut model = Model::default();
    let rows = (0..count)
        .map(|index| {
            let owner = CurveId::mint(format!("test:model:curve#{index:04}")).unwrap();
            model.curves.push(Curve {
                id: owner.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                parameter_range: None,
                source_object: None,
            });
            let procedural = ProceduralCurve::new(
                ProceduralCurveId::mint(format!("test:model:construction#{index:04}")).unwrap(),
                ProceduralCurveDefinition::Exact { cache: None },
            );
            (owner, procedural)
        })
        .collect();
    (model, rows)
}

fn surfaces(count: usize) -> (Model, Vec<(SurfaceId, ProceduralSurface)>) {
    let mut model = Model::default();
    let rows = (0..count)
        .map(|index| {
            let owner = SurfaceId::mint(format!("test:model:surface#{index:04}")).unwrap();
            model.surfaces.push(Surface {
                id: owner.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
            });
            let procedural = ProceduralSurface::new(
                ProceduralSurfaceId::mint(format!("test:model:construction#{index:04}")).unwrap(),
                ProceduralSurfaceDefinition::Unknown {
                    record: None,
                    cache: None,
                },
                None,
            );
            (owner, procedural)
        })
        .collect();
    (model, rows)
}

#[test]
fn indexed_procedural_batches_admit_thousands_below_repeated_scan_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8_000_000;
    let (mut model, rows) = curves(4096);
    let mut scanned = model.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = rows
        .iter()
        .try_for_each(|(owner, procedural)| {
            scanned
                .add_procedural_curve(&ctx, owner, procedural.clone())?
                .map_err(CodecError::malformed)
        })
        .unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    model.add_procedural_curves(&ctx, rows).unwrap().unwrap();
    assert_eq!(model.procedural_curves.len(), 4096);
    for (curve, procedural) in model.curves.iter().zip(&model.procedural_curves) {
        assert_eq!(
            curve.geometry.procedural_construction(),
            Some(&procedural.id)
        );
        assert!(matches!(
            curve.geometry.solved_cache(),
            Some(SolvedCurveGeometry::Unknown { record: None })
        ));
    }
    ctx.finish_session().unwrap();

    let (mut model, rows) = surfaces(4096);
    let mut scanned = model.clone();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = rows
        .iter()
        .try_for_each(|(owner, procedural)| {
            scanned
                .add_procedural_surface(&ctx, owner, procedural.clone())?
                .map_err(CodecError::malformed)
        })
        .unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    model.add_procedural_surfaces(&ctx, rows).unwrap().unwrap();
    assert_eq!(model.procedural_surfaces.len(), 4096);
    for (surface, procedural) in model.surfaces.iter().zip(&model.procedural_surfaces) {
        assert_eq!(
            surface.geometry.procedural_construction(),
            Some(&procedural.id)
        );
        assert!(matches!(
            surface.geometry.solved_cache(),
            Some(SolvedSurfaceGeometry::Unknown { record: None })
        ));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn indexed_curve_batch_preserves_single_attachment_rejections() {
    for case in 0..6 {
        let (mut model, mut rows) = curves(2);
        match case {
            0 => model.procedural_curves.push(rows[0].1.clone()),
            1 => rows[0].0 = CurveId::mint("test:model:curve#missing").unwrap(),
            2 => {
                model.curves[1].geometry = CurveGeometry::Procedural {
                    construction: rows[0].1.id.clone(),
                    cache: None,
                }
            }
            3 => {
                model.curves[0].geometry = CurveGeometry::Procedural {
                    construction: rows[1].1.id.clone(),
                    cache: None,
                }
            }
            4 => {
                model.curves[0].geometry = CurveGeometry::Procedural {
                    construction: rows[0].1.id.clone(),
                    cache: Some(SolvedCurveGeometry::Unknown { record: None }),
                }
            }
            5 => {
                model.curves[0].geometry = CurveGeometry::Procedural {
                    construction: rows[0].1.id.clone(),
                    cache: None,
                };
            }
            _ => unreachable!(),
        }
        let before = model.clone();
        let mut scanned = model.clone();
        let expected = scanned
            .add_procedural_curve(&StandardAdmission, &rows[0].0, rows[0].1.clone())
            .unwrap();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let actual = model
            .add_procedural_curves(&ctx, vec![rows.remove(0)])
            .unwrap();
        assert_eq!(
            actual.as_ref().map_err(ToString::to_string),
            expected.as_ref().map_err(ToString::to_string)
        );
        if actual.is_err() {
            assert_eq!(model, before);
        } else {
            assert_eq!(model, scanned);
        }
    }
}

#[test]
fn indexed_batch_rejects_repeated_constructions_and_owners_after_the_accepted_prefix() {
    for same_construction in [false, true] {
        let (mut model, mut rows) = surfaces(2);
        if same_construction {
            rows[1].1 = rows[0].1.clone();
        } else {
            rows[1].0 = rows[0].0.clone();
        }
        let before = model.clone();
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        assert!(model.add_procedural_surfaces(&ctx, rows).unwrap().is_err());
        assert_eq!(model.surfaces[1], before.surfaces[1]);
        assert_eq!(model.procedural_surfaces.len(), 1);
        assert!(model.surfaces[0]
            .geometry
            .procedural_construction()
            .is_some());
    }
}

#[test]
fn indexed_batch_preserves_storage_and_work_refusals() {
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            _ => unreachable!(),
        }
        let (mut model, rows) = curves(2);
        let before = model.clone();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(original) =
            model.add_procedural_curves(&ctx, rows).unwrap_err()
        else {
            panic!("resource refusal");
        };
        assert_eq!(original.dimension, dimension);
        assert_eq!(model, before);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }
}

#[test]
fn indexed_admission_tracks_appended_carriers_and_releases_its_storage() {
    let (mut model, rows) = curves(4096);
    let carriers = std::mem::take(&mut model.curves);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8_000_000;
    policy.limits.max_materialized_bytes = 8_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut admission = ProceduralAdmission::new(&ctx, &model).unwrap();
    for (carrier, (owner, procedural)) in carriers.into_iter().zip(rows) {
        model.curves.push(carrier);
        admission
            .add_curve(&mut model, &owner, procedural)
            .unwrap()
            .unwrap();
    }
    assert_eq!(model.procedural_curves.len(), 4096);
    drop(admission);
    drop(
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "procedural indexes released",
        )
        .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn indexed_admission_rejects_another_model_and_a_changed_owner_slot() {
    for another_model in [false, true] {
        let (mut model, mut rows) = curves(2);
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut admission = ProceduralAdmission::new(&ctx, &model).unwrap();
        let (owner, procedural) = rows.remove(0);
        admission
            .add_curve(&mut model, &owner, procedural)
            .unwrap()
            .unwrap();
        let (owner, procedural) = rows.remove(0);
        let error = if another_model {
            let mut other = model.clone();
            admission
                .add_curve(&mut other, &owner, procedural)
                .unwrap_err()
        } else {
            model.curves.swap(0, 1);
            admission
                .add_curve(&mut model, &owner, procedural)
                .unwrap_err()
        };
        assert!(matches!(error, CodecError::Malformed(_)));
        assert_eq!(model.procedural_curves.len(), 1);
    }
}

#[test]
fn text_index_compares_fingerprint_collisions_before_accepting_a_key() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut index = TextIndex::default();
    let hash = index.fingerprint(&ctx, "requested").unwrap();
    // All three entries share a fingerprint; only the byte-equal key matches.
    index.buckets.insert(
        hash,
        vec![
            ("different".to_owned(), 1),
            ("request".to_owned(), 2),
            ("requested".to_owned(), 3),
        ],
    );
    assert_eq!(index.get(&ctx, "requested").unwrap(), Some(&3));
    *index.get_mut(&ctx, "requested").unwrap().unwrap() = 4;
    assert_eq!(index.get(&ctx, "requested").unwrap(), Some(&4));
}

#[test]
fn procedural_round_trip_and_snapshot_keep_thousands_of_owner_rows() {
    let (mut model, rows) = curves(4096);
    model
        .add_procedural_curves(&cadmpeg_test_support::service_decode_context(), rows)
        .unwrap()
        .unwrap();
    let (surface_model, rows) = surfaces(4096);
    model.surfaces = surface_model.surfaces;
    model
        .add_procedural_surfaces(&cadmpeg_test_support::service_decode_context(), rows)
        .unwrap()
        .unwrap();
    let encoded = serde_json::to_value(&model).unwrap();
    let reconstructed: Model = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(reconstructed, model);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let snapshot = serde_json::to_value(model.geometry_snapshot(&ctx, "brep").unwrap()).unwrap();
    assert_eq!(snapshot["procedural_curves"], encoded["procedural_curves"]);
    assert_eq!(
        snapshot["procedural_surfaces"],
        encoded["procedural_surfaces"]
    );
    ctx.finish_session().unwrap();
}
