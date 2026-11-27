// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::geometry::analytic::{CylinderSurface, PlaneSurface};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{FaceId, LoopId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Face, FaceLoops, Sense};

fn plane() -> PlaneSurface {
    PlaneSurface::try_new(
        Point3::new(0.0, 0.0, 10.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .unwrap()
}

fn cylinder() -> CylinderSurface {
    CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        5.0,
    )
    .unwrap()
}

fn surface(slot: i64, geometry: SolvedSurfaceGeometry) -> Surface {
    Surface {
        id: SurfaceId::mint(format!("f3d:test:surface#{slot}")).unwrap(),
        geometry: SurfaceGeometry::Solved(geometry),
        source_object: None,
        native_ref: None,
    }
}

fn face(slot: i64, surface: i64, sense: Sense) -> Face {
    Face {
        id: FaceId::mint(format!("f3d:test:face#{slot}")).unwrap(),
        shell: ShellId::mint("f3d:test:shell#1").unwrap(),
        surface: SurfaceId::mint(format!("f3d:test:surface#{surface}")).unwrap(),
        sense,
        loops: FaceLoops::unspecified(vec![LoopId::mint(format!("f3d:test:loop#{slot}")).unwrap()]),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    }
}

#[test]
fn planar_and_cylindrical_faces_keep_their_surfaces_and_senses() {
    let brep = cadmpeg_asm::brep::AsmBrep {
        surfaces: vec![
            surface(20, SolvedSurfaceGeometry::Plane(plane())),
            surface(21, SolvedSurfaceGeometry::Cylinder(cylinder())),
            surface(22, SolvedSurfaceGeometry::Unknown { record: None }),
        ],
        faces: vec![
            face(5, 21, Sense::Reversed),
            face(4, 20, Sense::Forward),
            face(6, 22, Sense::Forward),
        ],
        ..Default::default()
    };
    assert_eq!(
        historical_face_samples(&brep),
        vec![
            crate::history_records::AsmHistoricalFaceSample {
                face: 4,
                surface: HistoricalFaceSurface::Plane(plane()),
                reversed: false,
            },
            crate::history_records::AsmHistoricalFaceSample {
                face: 5,
                surface: HistoricalFaceSurface::Cylinder(cylinder()),
                reversed: true,
            },
        ]
    );
}
