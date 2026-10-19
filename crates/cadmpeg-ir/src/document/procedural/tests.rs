// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
        let expected = scanned.add_procedural_curve(&rows[0].0, rows[0].1.clone());
        let actual = model.add_procedural_curves(vec![rows.remove(0)]).unwrap();
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
        assert!(model.add_procedural_surfaces(rows).unwrap().is_err());
        assert_eq!(model.surfaces[1], before.surfaces[1]);
        assert_eq!(model.procedural_surfaces.len(), 1);
        assert!(model.surfaces[0]
            .geometry
            .procedural_construction()
            .is_some());
    }
}

#[test]
fn indexed_admission_rejects_another_model_and_a_changed_owner_slot() {
    for another_model in [false, true] {
        let (mut model, mut rows) = curves(2);
        let mut admission = ProceduralAdmission::new(&model);
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
fn procedural_round_trip_and_snapshot_keep_thousands_of_owner_rows() {
    let (mut model, rows) = curves(4096);
    model.add_procedural_curves(rows).unwrap().unwrap();
    let (surface_model, rows) = surfaces(4096);
    model.surfaces = surface_model.surfaces;
    model.add_procedural_surfaces(rows).unwrap().unwrap();
    let encoded = serde_json::to_value(&model).unwrap();
    let reconstructed: Model = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(reconstructed, model);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let snapshot = serde_json::to_value(model.geometry_snapshot("brep").unwrap()).unwrap();
    assert_eq!(snapshot["procedural_curves"], encoded["procedural_curves"]);
    assert_eq!(
        snapshot["procedural_surfaces"],
        encoded["procedural_surfaces"]
    );
    ctx.finish_session().unwrap();
}

#[test]
fn procedural_batches_link_every_carrier_to_its_construction() {
    let (mut model, rows) = curves(4096);
    model.add_procedural_curves(rows).unwrap().unwrap();
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
    let (mut model, rows) = surfaces(4096);
    model.add_procedural_surfaces(rows).unwrap().unwrap();
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
}

#[test]
fn indexed_admission_tracks_carriers_appended_after_it_was_built() {
    let (mut model, rows) = curves(4096);
    let carriers = std::mem::take(&mut model.curves);
    let mut admission = ProceduralAdmission::new(&model);
    for (carrier, (owner, procedural)) in carriers.into_iter().zip(rows) {
        model.curves.push(carrier);
        admission
            .add_curve(&mut model, &owner, procedural)
            .unwrap()
            .unwrap();
    }
    assert_eq!(model.procedural_curves.len(), 4096);
}
