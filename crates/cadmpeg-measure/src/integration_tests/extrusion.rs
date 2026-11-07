// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
use cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::select::Selection;
use crate::test_support::builders::{assert_measured, measure, Builder};

const BOX_TOLERANCE: f64 = 1.0e-8;

fn oblique_extrusion(unknown_cache: bool) -> CadIr {
    let mut builder = Builder::new();
    let vertices = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [4.0, 6.0, 0.0],
        [2.0, 6.0, 0.0],
    ]
    .map(|point| builder.vertex(point));
    let ring = (0..4)
        .map(|i| {
            (
                builder.line(&vertices[i], &vertices[(i + 1) % 4]),
                Sense::Forward,
            )
        })
        .collect();
    let surface = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(&surface, Sense::Forward, &[ring], true);
    let mut ir = builder.finish(BodyKind::Sheet);
    let curve = CurveId::mint("test:model:curve#extrusion-directrix").unwrap();
    let construction =
        ProceduralSurfaceId::mint("test:model:surface-construction#extrusion").unwrap();
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                NurbsPoles3::from_lanes(
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                    None,
                )
                .unwrap(),
                false,
            )
            .unwrap(),
        )),
        parameter_range: None,
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Extrusion(ExtrusionSurfaceConstruction::legacy(
            curve,
            None,
            FiniteVector3::new(Vector3::new(1.0, 3.0, 0.0)).unwrap(),
            None,
            None,
        )),
        None,
    ));
    ir.model
        .surfaces
        .iter_mut()
        .find(|item| item.id == surface)
        .unwrap()
        .geometry = SurfaceGeometry::Procedural {
        construction,
        cache: unknown_cache.then_some(SolvedSurfaceGeometry::Unknown { record: None }),
    };
    ir.finalize();
    ir
}

#[test]
fn an_oblique_bspline_extrusion_measures_a_face_with_3d_boundaries() {
    for unknown_cache in [false, true] {
        for placed in [false, true] {
            let mut ir = oblique_extrusion(unknown_cache);
            if placed {
                ir.model.bodies[0].transform = Some(
                    Transform::affine([
                        [-2.0, 0.0, 0.0, 10.0],
                        [0.0, 2.0, 0.0, -4.0],
                        [0.0, 0.0, 2.0, 3.0],
                    ])
                    .unwrap(),
                );
            }
            let report = measure(
                &ir,
                &Selection {
                    all_faces: true,
                    ..Selection::default()
                },
            );
            let [face] = report.faces().as_slice() else {
                panic!("one face")
            };
            let expected_area = if placed { 48.0 } else { 12.0 };
            assert_measured(face.measures().area(), expected_area, expected_area);
            let bbox = face
                .measures()
                .bbox()
                .measured()
                .expect("bounded face")
                .value();
            for (actual, expected) in bbox
                .min
                .coordinates()
                .into_iter()
                .chain(bbox.max.coordinates())
                .zip(if placed {
                    [2.0, -4.0, 3.0, 10.0, 8.0, 3.0]
                } else {
                    [0.0, 0.0, 0.0, 4.0, 6.0, 0.0]
                })
            {
                assert!((actual.get() - expected).abs() < BOX_TOLERANCE);
            }
        }
    }
}
