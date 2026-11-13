// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};

use super::finish;
use crate::refine::Mesh;
use crate::surface::Carrier;

#[test]
fn surface_normals_follow_the_winding_without_losing_their_direction() {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(1000);
    let carrier = Carrier::new(&index, &work, &ir.model.faces[0].surface).unwrap();
    let mesh = Mesh {
        uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
        points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        normals: vec![Some(Vector3::new(0.6, 0.0, -0.8)); 3],
        scales: vec![[1.0, 1.0]; 3],
        triangles: vec![Some([0, 1, 2])],
        fixed: BTreeSet::new(),
    };
    let result = finish(&carrier, mesh, 1.0, Vec::new(), 0.0);
    for normal in result.normals {
        assert!((normal.x + 0.6).abs() < 1e-12);
        assert!((normal.z - 0.8).abs() < 1e-12);
    }
}

#[test]
fn face_mesh_spatial_and_transform_queries_dispatch_via_cadmpeg_gpu() {
    use cadmpeg_gpu::RayQuery3;

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(1000);
    let carrier = Carrier::new(&index, &work, &ir.model.faces[0].surface).unwrap();
    let mesh = Mesh {
        uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
        points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ],
        normals: vec![Some(Vector3::new(0.0, 0.0, 1.0)); 3],
        scales: vec![[1.0, 1.0]; 3],
        triangles: vec![Some([0, 1, 2])],
        fixed: BTreeSet::new(),
    };
    let face_mesh = finish(&carrier, mesh, 1.0, Vec::new(), 0.0);

    let matrix = [
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let shifted_positions = face_mesh.transform_positions(&matrix);
    assert!((shifted_positions[0].x - 3.0).abs() < 1e-12);

    let mut other = face_mesh.clone();
    other.positions = shifted_positions;
    assert!((face_mesh.hausdorff_distance(&other) - 3.0).abs() < 1e-12);

    let rays = [RayQuery3 {
        origin: Point3::new(0.25, 0.25, -1.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
        t_min: 0.0,
        t_max: 10.0,
    }];
    let hits = face_mesh.intersect_rays(&rays);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].hit);
    assert!((hits[0].t - 1.0).abs() < 1e-12);

    let closest = face_mesh.closest_points(&[Point3::new(0.25, 0.25, 2.0)]);
    assert_eq!(closest.len(), 1);
    assert!((closest[0].distance_sq - 4.0).abs() < 1e-12);
}
