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
    let carrier = Carrier::new(&index, &work, &ir.geometry.faces[0].surface).unwrap();
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
        anisotropy: [1.0, 1.0],
    };
    let result = finish(&carrier, mesh, 1.0, Vec::new(), 0.0);
    for normal in result.normals {
        assert!((normal.x + 0.6).abs() < 1e-12);
        assert!((normal.z - 0.8).abs() < 1e-12);
    }
}

/// Three corners on the parameter line `u = 1`, which the unit sphere maps
/// onto its meridian: their triangle stands in the meridian plane, covers
/// no part of the face, and is left out.
#[test]
fn a_triangle_on_one_parameter_line_is_left_out() {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(1000);
    let carrier = Carrier::new(&index, &work, &ir.geometry.faces[0].surface).unwrap();
    let meridian = |v: f64| Point3::new(v.cos(), 0.0, v.sin());
    let mesh = Mesh {
        uv: vec![[1.0, 0.0], [1.0, 0.5], [1.0, 1.0], [0.5, 0.5]],
        points: vec![
            meridian(0.0),
            meridian(0.5),
            meridian(1.0),
            Point3::new(0.0, -1.0, 0.0),
        ],
        normals: vec![None; 4],
        scales: vec![[1.0, 1.0]; 4],
        triangles: vec![Some([0, 1, 2]), Some([0, 2, 3])],
        fixed: BTreeSet::new(),
        anisotropy: [1.0, 1.0],
    };
    let result = finish(&carrier, mesh, 1.0, Vec::new(), 0.0);
    assert_eq!(result.triangles, vec![[0, 2, 3]]);
}

/// Three corners on a diagonal parameter line: the chord between two
/// boundary samples stands for a trace the surface bends, and their
/// triangle keeps the area between that trace and the chord.
#[test]
fn a_triangle_on_a_diagonal_parameter_line_stays() {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(1000);
    let carrier = Carrier::new(&index, &work, &ir.geometry.faces[0].surface).unwrap();
    let mesh = Mesh {
        uv: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 0.5, 0.1),
            Point3::new(1.0, 1.0, 0.0),
        ],
        normals: vec![None; 3],
        scales: vec![[1.0, 1.0]; 3],
        triangles: vec![Some([0, 1, 2])],
        fixed: BTreeSet::new(),
        anisotropy: [1.0, 1.0],
    };
    let result = finish(&carrier, mesh, 1.0, Vec::new(), 0.0);
    assert_eq!(result.triangles, vec![[0, 1, 2]]);
}
