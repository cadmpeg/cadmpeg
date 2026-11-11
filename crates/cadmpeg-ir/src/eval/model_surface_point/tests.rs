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
