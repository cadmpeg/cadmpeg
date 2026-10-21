// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn unsupported_free_procedural_geometry_does_not_block_a_valid_solid() {
    let mut source = cadmpeg_ir::examples::unit_cube().unwrap();
    let original_body = source.model.bodies[0].clone();
    let curve = cadmpeg_ir::ids::CurveId::mint("synthetic:free-control:curve#line").unwrap();
    let construction =
        cadmpeg_ir::ids::ProceduralSurfaceId::mint("synthetic:free-control:construction#plane")
            .unwrap();
    let surface = cadmpeg_ir::ids::SurfaceId::mint("synthetic:free-control:surface#plane").unwrap();
    source.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        parameter_range: None,
        source_object: None,
    });
    source
        .model
        .procedural_surfaces
        .push(ProceduralSurface::new(
            construction.clone(),
            ProceduralSurfaceDefinition::Extrusion(
                cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::legacy(
                    curve,
                    None,
                    cadmpeg_ir::features::FiniteVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
                    None,
                    None,
                ),
            ),
            None,
        ));
    source.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction,
            cache: None,
        },
        source_object: None,
    });
    source.finalize();
    let before = cadmpeg_ir::validate_neutral(&source, vec![]).unwrap();
    assert!(before.is_ok(), "{before:#?}");
    let mut losses = vec![];
    let admitted = admit_bodies(&source, &mut losses).unwrap();
    assert_eq!(admitted.model.bodies, vec![original_body]);
    assert_eq!(admitted.model.faces.len(), 6);
    assert_eq!(admitted.model.edges.len(), 12);
    assert_eq!(admitted.model.points.len(), 8);
    assert!(!admitted.model.surfaces.iter().any(|row| row.id == surface));
    assert!(losses.iter().any(
        |loss| loss.code == F3dLossCode::WriterGeometryOmitted.kind()
            && loss.message.contains(surface.as_str())
    ));
    let after = cadmpeg_ir::validate_neutral(&admitted, losses).unwrap();
    assert!(after.is_ok(), "{after:#?}");
}
