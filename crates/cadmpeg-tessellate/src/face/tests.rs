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
