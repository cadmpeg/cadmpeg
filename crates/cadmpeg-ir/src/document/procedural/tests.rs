// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::geometry::procedural_curves::ProceduralCurveDefinition;
use crate::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use crate::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn curves(count: usize) -> (Topology, Vec<(CurveId, ProceduralCurve)>) {
    let mut geometry = Topology::default();
    let rows = (0..count)
        .map(|index| {
            let owner = CurveId::mint(format!("test:model:curve#{index:04}")).unwrap();
            geometry.curves.push(Curve {
                id: owner.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                parameter_range: None,
                source_object: None,
                native_ref: None,
            });
            let procedural = ProceduralCurve::new(
                ProceduralCurveId::mint(format!("test:model:construction#{index:04}")).unwrap(),
                ProceduralCurveDefinition::Exact { cache: None },
            );
            (owner, procedural)
        })
        .collect();
    (geometry, rows)
}

fn surfaces(count: usize) -> (Topology, Vec<(SurfaceId, ProceduralSurface)>) {
    let mut geometry = Topology::default();
    let rows = (0..count)
        .map(|index| {
            let owner = SurfaceId::mint(format!("test:model:surface#{index:04}")).unwrap();
            geometry.surfaces.push(Surface {
                id: owner.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                source_object: None,
                native_ref: None,
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
    (geometry, rows)
}

#[test]
fn indexed_curve_batch_preserves_single_attachment_rejections() {
    for case in 0..6 {
        let (mut geometry, mut rows) = curves(2);
        match case {
            0 => geometry.procedural_curves.push(rows[0].1.clone()),
            1 => rows[0].0 = CurveId::mint("test:model:curve#missing").unwrap(),
            2 => {
                geometry.curves[1].geometry = CurveGeometry::Procedural {
                    construction: rows[0].1.id.clone(),
                    cache: None,
                }
            }
            3 => {
                geometry.curves[0].geometry = CurveGeometry::Procedural {
                    construction: rows[1].1.id.clone(),
                    cache: None,
                }
            }
            4 => {
                geometry.curves[0].geometry = CurveGeometry::Procedural {
                    construction: rows[0].1.id.clone(),
                    cache: Some(SolvedCurveGeometry::Unknown { record: None }),
                }
            }
            5 => {
                geometry.curves[0].geometry = CurveGeometry::Procedural {
                    construction: rows[0].1.id.clone(),
                    cache: None,
                };
            }
            _ => unreachable!(),
        }
        let before = geometry.clone();
        let mut scanned = geometry.clone();
        let expected = scanned.add_procedural_curve(&rows[0].0, rows[0].1.clone());
        let actual = geometry
            .add_procedural_curves(vec![rows.remove(0)])
            .unwrap();
        assert_eq!(
            actual.as_ref().map_err(ToString::to_string),
            expected.as_ref().map_err(ToString::to_string)
        );
        if actual.is_err() {
            assert_eq!(geometry, before);
        } else {
            assert_eq!(geometry, scanned);
        }
    }
}

#[test]
fn indexed_batch_rejects_repeated_constructions_and_owners_after_the_accepted_prefix() {
    for same_construction in [false, true] {
        let (mut geometry, mut rows) = surfaces(2);
        if same_construction {
            rows[1].1 = rows[0].1.clone();
        } else {
            rows[1].0 = rows[0].0.clone();
        }
        let before = geometry.clone();
        assert!(geometry.add_procedural_surfaces(rows).unwrap().is_err());
        assert_eq!(geometry.surfaces[1], before.surfaces[1]);
        assert_eq!(geometry.procedural_surfaces.len(), 1);
        assert!(geometry.surfaces[0]
            .geometry
            .procedural_construction()
            .is_some());
    }
}

#[test]
fn indexed_admission_rejects_another_model_and_a_changed_owner_slot() {
    for another_model in [false, true] {
        let (mut geometry, mut rows) = curves(2);
        let mut admission = ProceduralAdmission::new(&geometry);
        let (owner, procedural) = rows.remove(0);
        admission
            .add_curve(&mut geometry, &owner, procedural)
            .unwrap()
            .unwrap();
        let (owner, procedural) = rows.remove(0);
        let error = if another_model {
            let mut other = geometry.clone();
            admission
                .add_curve(&mut other, &owner, procedural)
                .unwrap_err()
        } else {
            geometry.curves.swap(0, 1);
            admission
                .add_curve(&mut geometry, &owner, procedural)
                .unwrap_err()
        };
        assert!(matches!(error, CodecError::Malformed(_)));
        assert_eq!(geometry.procedural_curves.len(), 1);
    }
}

#[test]
fn procedural_round_trip_and_snapshot_keep_thousands_of_owner_rows() {
    let (mut geometry, rows) = curves(4096);
    geometry.add_procedural_curves(rows).unwrap().unwrap();
    let (surface_model, rows) = surfaces(4096);
    geometry.surfaces = surface_model.surfaces;
    geometry.add_procedural_surfaces(rows).unwrap().unwrap();
    let encoded = crate::test_support::to_value(&geometry).unwrap();
    let reconstructed: Topology = crate::test_support::from_value(&encoded).unwrap();
    assert_eq!(reconstructed, geometry);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let snapshot = crate::test_support::to_value(
        geometry
            .geometry_snapshot(&crate::document::Model::default(), "brep")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot["procedural_curves"], encoded["procedural_curves"]);
    assert_eq!(
        snapshot["procedural_surfaces"],
        encoded["procedural_surfaces"]
    );
    ctx.finish_session().unwrap();
}

#[test]
fn procedural_batches_link_every_carrier_to_its_construction() {
    let (mut geometry, rows) = curves(4096);
    geometry.add_procedural_curves(rows).unwrap().unwrap();
    assert_eq!(geometry.procedural_curves.len(), 4096);
    for (curve, procedural) in geometry.curves.iter().zip(&geometry.procedural_curves) {
        assert_eq!(
            curve.geometry.procedural_construction(),
            Some(&procedural.id)
        );
        assert!(matches!(
            curve.geometry.solved_cache(),
            Some(SolvedCurveGeometry::Unknown { record: None })
        ));
    }
    let (mut geometry, rows) = surfaces(4096);
    geometry.add_procedural_surfaces(rows).unwrap().unwrap();
    assert_eq!(geometry.procedural_surfaces.len(), 4096);
    for (surface, procedural) in geometry.surfaces.iter().zip(&geometry.procedural_surfaces) {
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
    let (mut geometry, rows) = curves(4096);
    let carriers = std::mem::take(&mut geometry.curves);
    let mut admission = ProceduralAdmission::new(&geometry);
    for (carrier, (owner, procedural)) in carriers.into_iter().zip(rows) {
        geometry.curves.push(carrier);
        admission
            .add_curve(&mut geometry, &owner, procedural)
            .unwrap()
            .unwrap();
    }
    assert_eq!(geometry.procedural_curves.len(), 4096);
}
