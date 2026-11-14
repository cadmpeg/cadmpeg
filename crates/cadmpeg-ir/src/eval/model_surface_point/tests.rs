// SPDX-License-Identifier: Apache-2.0
use crate::eval::admission::EvaluationAdmission;
use crate::eval::{model_surface_point_by_id, EvaluationFailure};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::surface_payloads::{
    AxisRevolutionSurfaceConstruction, LinearSweepSurfaceConstruction, OffsetSurfaceConstruction,
};
use crate::geometry::{
    Curve, CurveGeometry, LegacyExtensionFlags, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, Surface, SurfaceGeometry,
};
use crate::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use crate::index::ModelIndex;
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::CadIr;

fn fixture(revolution: bool, parallel: bool, distance: f64) -> (CadIr, SurfaceId, SurfaceId) {
    let curve = CurveId::mint("test:model:curve#profile").unwrap();
    let support = SurfaceId::mint("test:model:surface#support").unwrap();
    let offset = SurfaceId::mint("test:model:surface#offset").unwrap();
    let construction =
        ProceduralSurfaceId::mint("test:model:surface-construction#support").unwrap();
    let offset_construction =
        ProceduralSurfaceId::mint("test:model:surface-construction#offset").unwrap();
    let poles = if revolution {
        vec![Point3::new(5.0, 0.0, 0.0), Point3::new(5.0, 0.0, 2.0)]
    } else {
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)]
    };
    let definition = if revolution {
        ProceduralSurfaceDefinition::AxisRevolution(
            AxisRevolutionSurfaceConstruction::try_new(
                curve.clone(),
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        )
    } else {
        ProceduralSurfaceDefinition::LinearSweep(
            LinearSweepSurfaceConstruction::try_new(
                curve.clone(),
                if parallel {
                    Vector3::new(1.0, 0.0, 0.0)
                } else {
                    Vector3::new(0.0, 0.0, 1.0)
                },
            )
            .unwrap(),
        )
    };
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve,
        source_object: None,
        parameter_range: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0], poles, None, false).unwrap(),
        )),
    });
    ir.model.surfaces = vec![
        Surface {
            id: support.clone(),
            source_object: None,
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
        },
        Surface {
            id: offset.clone(),
            source_object: None,
            geometry: SurfaceGeometry::Procedural {
                construction: offset_construction.clone(),
                cache: None,
            },
        },
    ];
    ir.model.procedural_surfaces = vec![
        ProceduralSurface::new(construction, definition, None),
        ProceduralSurface::new(
            offset_construction,
            ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::legacy(
                support.clone(),
                FiniteReal::new(distance).unwrap(),
                None,
                None,
                false,
                LegacyExtensionFlags::Absent {},
                None,
            )),
            None,
        ),
    ];
    (ir, support, offset)
}

#[test]
fn indexed_surface_points_preserve_cacheless_construction_geometry() {
    for revolution in [false, true] {
        let (ir, _, _) = fixture(revolution, false, 2.0);
        let geometry = &ir.model.surfaces[0].geometry;
        let index = ModelIndex::build(&ir);
        for (u, v) in [(0.0, 0.25), (0.25, 0.75), (0.9, 1.0)] {
            let point = crate::eval::model_surface_point_in_index(
                EvaluationAdmission::Standard,
                &index,
                geometry,
                u,
                v,
            )
            .unwrap()
            .get();
            let expected = if revolution {
                Point3::new(5.0 * u.cos(), 5.0 * u.sin(), 2.0 * v)
            } else {
                Point3::new(4.0 * u, 0.0, v)
            };
            assert!(point.distance(expected) < 1.0e-12);
        }
    }
}

#[test]
fn indexed_surface_points_reject_missing_constructions() {
    let (mut ir, _, _) = fixture(false, false, 2.0);
    ir.model.procedural_surfaces.remove(0);
    let index = ModelIndex::build(&ir);
    assert_eq!(
        crate::eval::model_surface_point_in_index(
            EvaluationAdmission::Standard,
            &index,
            &ir.model.surfaces[0].geometry,
            0.25,
            3.0,
        ),
        Err(EvaluationFailure::NoValue)
    );
}

#[test]
fn indexed_surface_points_keep_the_first_construction_with_an_identity() {
    let (mut ir, _, _) = fixture(false, false, 2.0);
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        ir.model.procedural_surfaces[0].id.clone(),
        ProceduralSurfaceDefinition::LinearSweep(
            LinearSweepSurfaceConstruction::try_new(
                ir.model.curves[0].id.clone(),
                Vector3::new(0.0, 0.0, 7.0),
            )
            .unwrap(),
        ),
        None,
    ));
    let index = ModelIndex::build(&ir);
    let point = crate::eval::model_surface_point_in_index(
        EvaluationAdmission::Standard,
        &index,
        &ir.model.surfaces[0].geometry,
        0.25,
        3.0,
    )
    .unwrap()
    .get();
    assert!(point.distance(Point3::new(1.0, 0.0, 3.0)) < 1.0e-12);
}

#[test]
fn cacheless_sweep_offsets_read_the_support_normal_with_both_polarities() {
    for distance in [-2.0, 2.0] {
        for revolution in [false, true] {
            let (ir, _, offset) = fixture(revolution, false, distance);
            let index = ModelIndex::build(&ir);
            let (u, v) = if revolution { (0.0, 0.25) } else { (0.25, 3.0) };
            let point =
                model_surface_point_by_id(EvaluationAdmission::Standard, &index, &offset, u, v)
                    .unwrap()
                    .get();
            let expected = if revolution {
                Point3::new(5.0 + distance, 0.0, 0.5)
            } else {
                Point3::new(1.0, -distance, 3.0)
            };
            assert!(point.distance(expected) < 1.0e-12);
        }
    }
}

#[test]
fn a_parallel_sweep_retains_its_point_and_withholds_its_offset_normal() {
    let (ir, support, offset) = fixture(false, true, 2.0);
    let index = ModelIndex::build(&ir);
    assert_eq!(
        model_surface_point_by_id(EvaluationAdmission::Standard, &index, &support, 0.25, 3.0)
            .unwrap()
            .get(),
        Point3::new(4.0, 0.0, 0.0)
    );
    assert_eq!(
        model_surface_point_by_id(EvaluationAdmission::Standard, &index, &offset, 0.25, 3.0),
        Err(EvaluationFailure::NoValue)
    );
}
