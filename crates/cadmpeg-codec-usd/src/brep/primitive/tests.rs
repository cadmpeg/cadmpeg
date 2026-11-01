// SPDX-License-Identifier: Apache-2.0

use super::{cube, plane, revolved, sphere, Axis, Segment};
use crate::brep::Brep;
use cadmpeg_ir::math::portable::Portable;

/// The uses of each edge: `(forward, reversed)`.
fn uses(brep: &Brep) -> Vec<(usize, usize)> {
    let mut uses = vec![(0, 0); brep.edges.len()];
    for oriented in &brep.oriented_edges {
        let slot = &mut uses[oriented.edge];
        if oriented.orientation {
            slot.0 += 1;
        } else {
            slot.1 += 1;
        }
    }
    uses
}

fn closed(brep: &Brep) {
    assert!(
        uses(brep).iter().all(|uses| *uses == (1, 1)),
        "{:?}",
        uses(brep)
    );
    assert_eq!(brep.solids.len(), 1);
    assert!(brep.shells[0].closed);
}

#[test]
fn a_cube_is_six_faces_on_twelve_edges() {
    let brep = cube(2.0);
    assert_eq!(
        (brep.vertices.len(), brep.edges.len(), brep.faces.len()),
        (8, 12, 6)
    );
    closed(&brep);
}

#[test]
fn a_cylinder_has_two_caps_and_a_seamed_side() {
    let points = [[0.0, -1.0], [1.0, -1.0], [1.0, 1.0], [0.0, 1.0]];
    let brep = revolved(&points, &[Segment::Line; 3], Axis::X);
    assert_eq!(
        (brep.vertices.len(), brep.edges.len(), brep.faces.len()),
        (2, 3, 3)
    );
    closed(&brep);
    // The axis is x: the circles lie in planes of constant x.
    for vertex in &brep.vertices {
        assert!((vertex[0].abs() - 1.0).abs() < 1e-12, "{vertex:?}");
    }
}

#[test]
fn a_cone_ends_at_an_apex_vertex() {
    let points = [[0.0, -1.0], [1.0, -1.0], [0.0, 1.0]];
    let brep = revolved(&points, &[Segment::Line; 2], Axis::Z);
    assert_eq!(
        (brep.vertices.len(), brep.edges.len(), brep.faces.len()),
        (2, 2, 2)
    );
    closed(&brep);
    let cone = &brep.analytic_surfaces[1];
    assert_eq!(cone.params, [1.0, 0.5_f64.portable_atan()]);
    assert_eq!(cone.frame.axis, [0.0, 0.0, -1.0]);
}

#[test]
fn a_capsule_has_two_hemispheres() {
    let points = [[0.0, -1.0], [0.5, -0.5], [0.5, 0.5], [0.0, 1.0]];
    let segments = [
        Segment::Arc { center: -0.5 },
        Segment::Line,
        Segment::Arc { center: 0.5 },
    ];
    let brep = revolved(&points, &segments, Axis::Z);
    assert_eq!(
        (brep.vertices.len(), brep.edges.len(), brep.faces.len()),
        (4, 5, 3)
    );
    closed(&brep);
}

#[test]
fn a_sphere_is_one_face_and_a_plane_one_open_face() {
    let ball = sphere(1.0);
    assert_eq!((ball.faces.len(), ball.edges.len()), (1, 0));
    assert!(ball.shells[0].closed);
    let sheet = plane(2.0, 4.0, Axis::Y);
    assert_eq!((sheet.faces.len(), sheet.edges.len()), (1, 4));
    assert!(!sheet.shells[0].closed);
    assert!(sheet.solids.is_empty());
    assert_eq!(sheet.analytic_surfaces[0].frame.axis, [0.0, 1.0, 0.0]);
    // The length runs along z when the axis is y.
    assert!(sheet.vertices.iter().all(|vertex| vertex[2].abs() == 2.0));
}
