// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

const EPS_PLACED_POINT: f64 = 1.0e-12;

/// `ON_Plane::Transform` maps both plane axes, so a plane surface keeps the
/// point of every `(u, v)` under an orientation-reversing transform.
#[test]
fn plane_surface_keeps_its_parameterization_under_a_reflection() {
    let plane = PlaneSurface::try_new(
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("plane surface");
    let rows = [
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
    ];
    let reflection = Transform::affine(rows).expect("affine reflection");
    let mut surface = Surface {
        id: cadmpeg_ir::ids::SurfaceId::try_from("rhino:test:surface#plane".to_owned())
            .expect("surface id"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)),
        source_object: None,
    };
    super::super::transform_surface(&mut surface, reflection)
        .unwrap_or_else(|_| panic!("plane transform"));
    let SurfaceGeometry::Solved(placed) = &surface.geometry else {
        panic!("solved plane");
    };
    for (u, v) in [(0.0, 0.0), (2.0, 0.0), (0.0, 3.0), (-1.5, 4.0)] {
        let source = cadmpeg_ir::eval::decode::surface_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &SolvedSurfaceGeometry::Plane(plane),
            u,
            v,
        )
        .expect("source point")
        .get();
        let expected = Point3::new(
            rows[0][0] * source.x + rows[0][1] * source.y + rows[0][2] * source.z + rows[0][3],
            rows[1][0] * source.x + rows[1][1] * source.y + rows[1][2] * source.z + rows[1][3],
            rows[2][0] * source.x + rows[2][1] * source.y + rows[2][2] * source.z + rows[2][3],
        );
        let actual = cadmpeg_ir::eval::decode::surface_point_solved(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            placed,
            u,
            v,
        )
        .expect("placed point")
        .get();
        assert!(
            actual.distance(expected) <= EPS_PLACED_POINT,
            "u={u} v={v} actual={actual:?} expected={expected:?}"
        );
    }
}
