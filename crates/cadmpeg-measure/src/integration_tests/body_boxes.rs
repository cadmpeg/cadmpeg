// SPDX-License-Identifier: Apache-2.0
//! Body boxes from the box pass, against the body rows of a full pass.

use cadmpeg_ir::topology::{BodyKind, EdgeCarrier, LoopBoundary, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

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
use crate::value::{Quantity, Reading};

fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("a quantity serializes")
}

/// The full pass gives the one body of `ir` no box, and the box pass gives
/// the same unavailable box; the full pass's message.
fn no_box_in_either_pass(ir: &CadIr, label: &str) -> String {
    let report = measure(ir, &Selection::default());
    let [row] = report.bodies().as_slice() else {
        panic!("{label}: one body row");
    };
    let Quantity::Unavailable(missing) = row.bbox() else {
        panic!("{label}: the full pass gives no box");
    };
    let boxes = body_boxes(ir).expect("the box pass completes");
    let [body_box] = boxes.as_slice() else {
        panic!("{label}: one body box");
    };
    assert_eq!(json(row.bbox()), json(body_box.bbox()), "{label}");
    missing.message().to_owned()
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
fn side_by_side_squares() -> CadIr {
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
    builder.finish(BodyKind::Sheet)
}

#[test]
fn the_box_pass_boxes_a_plane_face_whose_loops_do_not_nest() {
    let ir = side_by_side_squares();
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

/// The second square's loop, its first coedge or that coedge's edge is
/// missing: the box pass gives no box, as the full pass does, rather than
/// the box of the pieces that remain.
#[test]
fn the_box_pass_gives_no_box_for_a_face_with_a_missing_boundary_record() {
    let squares = side_by_side_squares();
    let second = squares.geometry.faces[0]
        .loops
        .iter()
        .nth(1)
        .expect("two loops")
        .clone();
    let face_loop = squares
        .geometry
        .loops
        .iter()
        .find(|face_loop| face_loop.id == second)
        .expect("the loop resolves");
    let LoopBoundary::Ring(ring) = &face_loop.boundary else {
        panic!("a ring");
    };
    let coedge = ring.coedges()[0].clone();
    let edge = squares
        .geometry
        .coedges
        .iter()
        .find(|record| record.id == coedge)
        .expect("the coedge resolves")
        .edge
        .clone();

    let mut without_loop = squares.clone();
    without_loop
        .geometry
        .loops
        .retain(|face_loop| face_loop.id != second);
    let mut without_coedge = squares.clone();
    without_coedge
        .geometry
        .coedges
        .retain(|record| record.id != coedge);
    let mut without_edge = squares;
    without_edge
        .geometry
        .edges
        .retain(|record| record.id != edge);
    for (label, ir) in [
        ("loop", without_loop),
        ("coedge", without_coedge),
        ("edge", without_edge),
    ] {
        no_box_in_either_pass(&ir, label);
    }
}

/// The cylinder face's ring leaves out the last of its four coedges, every
/// record still present: the ring does not close by vertex identity. The
/// box pass gives no box, as the full pass does, rather than the box of the
/// three coedges that remain.
#[test]
fn the_box_pass_gives_no_box_for_a_ring_that_leaves_out_a_coedge() {
    let mut ir = cylinder_solid(5.0, 10.0);
    let side = ir.geometry.faces[0]
        .loops
        .iter()
        .next()
        .expect("one loop")
        .clone();
    let face_loop = ir
        .geometry
        .loops
        .iter_mut()
        .find(|face_loop| face_loop.id == side)
        .expect("the loop resolves");
    let (coedges, vertex_uses) = {
        let LoopBoundary::Ring(ring) = &face_loop.boundary else {
            panic!("a ring");
        };
        let coedges = ring.coedges();
        assert_eq!(coedges.len(), 4);
        (coedges[..3].to_vec(), ring.vertex_uses().to_vec())
    };
    face_loop
        .replace_ring(coedges, vertex_uses)
        .expect("three distinct coedges");
    let message = no_box_in_either_pass(&ir, "open ring");
    assert!(message.ends_with("boundary.ring-chain-failed"), "{message}");
}

/// The first square's first side states half its length: its piece ends
/// 5 mm short of the next side's start, so the ring closes by vertex
/// identity but not in space. The box pass gives no box, as the full pass
/// does.
#[test]
fn the_box_pass_gives_no_box_for_a_ring_that_does_not_close() {
    let mut ir = side_by_side_squares();
    let side = &mut ir.geometry.edges[0];
    let EdgeCarrier::Bounded(curve, _) = &side.carrier else {
        panic!("a bounded line");
    };
    side.carrier =
        EdgeCarrier::new(Some(curve.clone()), Some([0.0, 5.0])).expect("a bounded carrier");
    let message = no_box_in_either_pass(&ir, "short side");
    assert!(message.ends_with("boundary.not-closed"), "{message}");
}

/// The elbow's face rests on its declared orientation, which no witness
/// confirms: its body box says so and carries the complement box.
#[test]
fn a_body_box_on_an_unconfirmed_orientation_says_so() {
    let boxes = body_boxes(&elbow([0.0, 0.0])).expect("the box pass completes");
    let [body_box] = boxes.as_slice() else {
        panic!("one body box");
    };
    let measured = body_box.bbox().measured().expect("a box");
    assert_eq!(measured.reading(), Some(Reading::Declared));
    let [candidate] = measured.candidates() else {
        panic!("one candidate");
    };
    assert_eq!(candidate.reading(), Reading::Complement);
}
