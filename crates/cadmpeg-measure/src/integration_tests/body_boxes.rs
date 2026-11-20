// SPDX-License-Identifier: Apache-2.0
//! Body boxes from the box pass, against the body rows of a full pass.

use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::transform::Transform;

use super::compare::nurbs_rectangle;
use super::cone_apex_rings::spike;
use super::extrusion::oblique_extrusion;
use super::tori::elbow;
use super::witnesses::capsule;
use crate::body_boxes;
use crate::select::Selection;
use crate::test_support::builders::{
    assert_tight_box, cuboid, cylinder_solid, measure, plate_with_hole, rotated_cuboid, rotation,
    Builder,
};
use crate::value::Quantity;

fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("a quantity serializes")
}

#[test]
fn the_box_pass_reports_the_body_boxes_of_a_full_pass() {
    let turn = Transform::affine([
        [0.0, -1.0, 0.0, 1.5],
        [1.0, 0.0, 0.0, -0.5],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite rotation");
    let documents = vec![
        ("cuboid", cuboid([0.0; 3], [1.0, 2.0, 3.0])),
        (
            "rotated cuboid",
            rotated_cuboid(
                &rotation([1.0, 1.0, 0.0], 0.7),
                [5.0, -2.0, 1.0],
                [0.0; 3],
                [1.0, 2.0, 3.0],
            ),
        ),
        ("cylinder", cylinder_solid(5.0, 10.0)),
        ("plate with a hole", plate_with_hole(true)),
        ("turned NURBS patch", nurbs_rectangle(Some(turn))),
        ("capsule", capsule(Sense::Forward)),
        ("torus elbow", elbow([0.0, 0.0])),
        ("cone spike", spike()),
        ("oblique extrusion", oblique_extrusion(false)),
    ];
    for (label, ir) in documents {
        let report = measure(&ir, &Selection::default());
        let boxes = body_boxes(&ir).expect("the box pass completes");
        assert_eq!(boxes.len(), report.bodies().len(), "{label}");
        for (row, body_box) in report.bodies().iter().zip(&boxes) {
            assert_eq!(row.body(), body_box.body(), "{label}");
            assert!(row.bbox().measured().is_some(), "{label}");
            assert_eq!(json(row.bbox()), json(body_box.bbox()), "{label}");
            assert_eq!(
                row.vertex_box().as_ref().map(json),
                body_box.vertex_box().as_ref().map(json),
                "{label}"
            );
        }
    }
}

/// Two side-by-side squares as the loops of one plane face: no loop
/// encloses the other, so the region does not classify.
#[test]
fn the_box_pass_boxes_a_plane_face_whose_loops_do_not_nest() {
    let mut builder = Builder::new();
    let mut rings = Vec::new();
    for x in [0.0, 20.0] {
        let corners = [
            [x, 0.0, 0.0],
            [x + 10.0, 0.0, 0.0],
            [x + 10.0, 10.0, 0.0],
            [x, 10.0, 0.0],
        ]
        .map(|point| builder.vertex(point));
        let ring: Vec<_> = (0..4)
            .map(|index| {
                (
                    builder.line(&corners[index], &corners[(index + 1) % 4]),
                    Sense::Forward,
                )
            })
            .collect();
        rings.push(ring);
    }
    let surface = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(&surface, Sense::Forward, &rings, false);
    let ir = builder.finish(BodyKind::Sheet);

    let report = measure(&ir, &Selection::default());
    let [row] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    let Quantity::Unavailable(missing) = row.bbox() else {
        panic!("the full pass gives no box");
    };
    assert!(
        missing.message().ends_with("boundary.nesting-invalid"),
        "{}",
        missing.message()
    );
    let boxes = body_boxes(&ir).expect("the box pass completes");
    let [body_box] = boxes.as_slice() else {
        panic!("one body box");
    };
    assert_tight_box(body_box.bbox(), [0.0; 3], [30.0, 10.0, 0.0], 30.0);
}
