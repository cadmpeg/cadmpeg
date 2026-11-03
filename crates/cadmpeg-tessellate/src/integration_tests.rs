// SPDX-License-Identifier: Apache-2.0
//! Face tessellations over the IR example documents.

use cadmpeg_ir::math::{Point3, Vector3};

use crate::plane::{cross, dot, sub};
use crate::{FaceMesh, Options, Tessellator};

/// Agreement of an area with its closed form.
const AREA_TOLERANCE: f64 = 1e-9;

fn area(mesh: &FaceMesh) -> f64 {
    mesh.triangles
        .iter()
        .map(|[a, b, c]| {
            let (pa, pb, pc) = (mesh.positions[*a], mesh.positions[*b], mesh.positions[*c]);
            let normal = cross(sub(pb, pa), sub(pc, pa));
            dot(normal, normal).sqrt() / 2.0
        })
        .sum()
}

fn facing(mesh: &FaceMesh, triangle: [usize; 3]) -> Vector3 {
    let [a, b, c] = triangle;
    let points: [Point3; 3] = [mesh.positions[a], mesh.positions[b], mesh.positions[c]];
    cross(sub(points[1], points[0]), sub(points[2], points[0]))
}

#[test]
fn each_cube_face_is_two_outward_triangles_of_its_area() {
    let cube = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut tessellator = Tessellator::new(&cube, Options::default());
    let mut total = 0.0;
    for face in &cube.model.faces {
        let mesh = tessellator.face(&face.id).unwrap();
        assert_eq!(mesh.triangles.len(), 2, "{}", face.id);
        total += area(&mesh);
        for triangle in &mesh.triangles {
            let normal = facing(&mesh, *triangle);
            let corner = mesh.normals[triangle[0]];
            assert!(dot(normal, corner) > 0.0, "{}", face.id);
        }
        assert!(mesh.deviation <= AREA_TOLERANCE);
    }
    let side = cube
        .model
        .points
        .iter()
        .map(|point| point.position().get().x)
        .fold(0.0, f64::max)
        - cube
            .model
            .points
            .iter()
            .map(|point| point.position().get().x)
            .fold(f64::INFINITY, f64::min);
    assert!((total - 6.0 * side * side).abs() <= AREA_TOLERANCE * side * side);
}
