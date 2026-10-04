// SPDX-License-Identifier: Apache-2.0

use super::{add_face, add_square_face, model_with_body, set_shell_faces};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;

#[test]
fn circular_arc_trim_disambiguates_coincident_planar_supports() {
    let mut model = model_with_body();
    let target = add_square_face(&mut model, "arc-target", 0.0);
    let competitor = add_face(
        &mut model,
        "arc-competitor",
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        [
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
    );
    for (curve_id, radius) in [
        ("synthetic:test:curve#curve-arc-competitor-0", 2.0),
        ("synthetic:test:curve#curve-arc-competitor-2", 1.0),
    ] {
        model
            .curves
            .iter_mut()
            .find(|curve| curve.id.as_str() == curve_id)
            .unwrap()
            .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .unwrap(),
        ));
    }
    set_shell_faces(&mut model, vec![target.clone(), competitor]);
    model.tessellations.push(
        Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint(
                "synthetic:test:tessellation#arc-trim-mesh",
            )
            .expect("valid identity"),
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.25, -0.75, 0.0),
                    Point3::new(1.75, -0.75, 0.0),
                    Point3::new(1.0, -0.25, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners!(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#arc-trim-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![target]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
}
