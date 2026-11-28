// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::topology::FaceLoops;

use super::import;
use crate::brep::primitive::{plane, sphere, Axis};

#[test]
fn a_face_whose_loops_are_all_dropped_is_partial() {
    let mut brep = plane(2.0, 2.0, Axis::Z);
    let loop_ = brep.faces[0].loops[0];
    brep.loops[loop_].oriented_edges.clear();
    brep.loops[loop_].vertex = None;
    let imported = import(&brep, "p", 1.0).unwrap();
    let [face] = imported.faces.as_slice() else {
        panic!("one face");
    };
    assert_eq!(
        face.loops,
        FaceLoops::PartialUnspecified { loops: Vec::new() }
    );
    let [reason] = imported.unstated.as_slice() else {
        panic!("one unstated face: {:?}", imported.unstated);
    };
    assert_eq!(
        reason,
        &format!(
            "{}: no loop with an edge or vertex on a plane, which no face covers whole",
            face.id
        )
    );
}

#[test]
fn a_sphere_whose_pole_loop_is_dropped_is_whole() {
    let imported = import(&sphere(1.0), "s", 1.0).unwrap();
    let [face] = imported.faces.as_slice() else {
        panic!("one face");
    };
    assert_eq!(face.loops, FaceLoops::unspecified(Vec::new()));
    assert!(imported.unstated.is_empty(), "{:?}", imported.unstated);
}

#[test]
fn a_face_with_no_loop_on_a_plane_is_partial() {
    let mut brep = plane(2.0, 2.0, Axis::Z);
    brep.faces[0].loops.clear();
    let imported = import(&brep, "p", 1.0).unwrap();
    let [face] = imported.faces.as_slice() else {
        panic!("one face");
    };
    assert_eq!(
        face.loops,
        FaceLoops::PartialUnspecified { loops: Vec::new() }
    );
    let [reason] = imported.unstated.as_slice() else {
        panic!("one unstated face: {:?}", imported.unstated);
    };
    assert_eq!(
        reason,
        &format!(
            "{}: no loop with an edge or vertex on a plane, which no face covers whole",
            face.id
        )
    );
}
