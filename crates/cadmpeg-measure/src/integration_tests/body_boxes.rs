// SPDX-License-Identifier: Apache-2.0
//! Body boxes from the box pass, against the body rows of a full pass.

use cadmpeg_ir::ids::LoopId;
use cadmpeg_ir::topology::{BodyKind, EdgeCarrier, FaceLoops, LoopBoundary, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::compare::nurbs_rectangle;
use super::cone_apex_rings::spike;
use super::extrusion::oblique_extrusion;
use super::tori::elbow;
use super::witnesses::capsule;
use crate::body_boxes;
use crate::select::Selection;
use crate::test_support::box_pass::{agreements, Agreement};
use crate::test_support::builders::{
    cuboid, cylinder_solid, measure, plate_with_hole, rotated_cuboid, rotation, Builder, Frame,
};
use crate::value::{BoxKind, Quantity, Reading};

fn json<T: serde::Serialize>(value: &T) -> serde_json::Value {
    cadmpeg_core::serde_erase::json::to_value(value).expect("a quantity serializes")
}

/// The agreement of each body of `ir`.
fn agreement_of(ir: &CadIr) -> Vec<Agreement> {
    agreements(ir)
        .expect("both passes complete")
        .into_iter()
        .map(|body| body.agreement)
        .collect()
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
    assert_eq!(agreement_of(ir), [Agreement::Unboxed], "{label}");
    missing.message().to_owned()
}

/// `ir` with the loops of its first face replaced by `loops`, unclassified;
/// every loop record stays.
fn listing(ir: &CadIr, loops: Vec<LoopId>) -> CadIr {
    let mut ir = ir.clone();
    ir.geometry.faces[0].loops = FaceLoops::unspecified(loops);
    ir
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
        assert!(
            agreement_of(&ir)
                .iter()
                .all(|agreement| *agreement == Agreement::Equal),
            "{label}"
        );
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
fn the_box_pass_gives_no_box_for_a_plane_face_whose_loops_do_not_nest() {
    let message = no_box_in_either_pass(&side_by_side_squares(), "side-by-side squares");
    assert!(message.ends_with("boundary.nesting-invalid"), "{message}");
}

/// The second square's loop, its first coedge or that coedge's edge is
/// missing: the box pass gives no box, as the full pass does, rather than
/// the box of the pieces that remain.
#[test]
fn the_box_pass_gives_no_box_for_a_face_with_a_missing_boundary_record() {
    let squares = side_by_side_squares();
    let second = squares.geometry.faces[0]
        .loops
        .stated()
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
        .stated()
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

/// The cylinder band `r = 10`, `z ∈ [0, 30]` between its two circles. Each
/// circle alone wraps the axis and closes no region: listing one of the two
/// loops gives no box in either pass, rather than the box of that circle.
#[test]
fn the_box_pass_gives_no_box_for_a_cylinder_face_that_lists_one_of_its_two_loops() {
    let mut builder = Builder::new();
    let low = builder.vertex([10.0, 0.0, 0.0]);
    let high = builder.vertex([10.0, 0.0, 30.0]);
    let bottom = builder.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 10.0, &low);
    let top = builder.circle(
        [0.0, 0.0, 30.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        10.0,
        &high,
    );
    let surface = builder.cylinder(Frame::z_up([0.0; 3]), 10.0);
    builder.face(
        &surface,
        Sense::Forward,
        &[vec![(top, Sense::Reversed)], vec![(bottom, Sense::Forward)]],
        true,
    );
    let band = builder.finish(BodyKind::Sheet);
    assert_eq!(agreement_of(&band), [Agreement::Equal]);
    let loops = band.geometry.faces[0]
        .loops
        .stated()
        .cloned()
        .collect::<Vec<_>>();
    for kept in &loops {
        let message = no_box_in_either_pass(&listing(&band, vec![kept.clone()]), kept.as_str());
        assert!(message.ends_with("boundary.nesting-invalid"), "{message}");
    }
}

/// The square `[0, 40]²` with two holes of radius 5, listed without its
/// outer loop: no single loop encloses the other, so neither pass gives a
/// box, rather than the box of the holes.
#[test]
fn the_box_pass_gives_no_box_for_a_plane_face_that_leaves_out_its_outer_loop() {
    let mut builder = Builder::new();
    let corners = [
        builder.vertex([0.0, 0.0, 0.0]),
        builder.vertex([40.0, 0.0, 0.0]),
        builder.vertex([40.0, 40.0, 0.0]),
        builder.vertex([0.0, 40.0, 0.0]),
    ];
    let sides: Vec<_> = (0..4)
        .map(|index| {
            (
                builder.line(&corners[index], &corners[(index + 1) % 4]),
                Sense::Forward,
            )
        })
        .collect();
    let mut rings = vec![sides];
    for x in [12.0, 28.0] {
        let seam = builder.vertex([x + 5.0, 20.0, 0.0]);
        let hole = builder.circle([x, 20.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 5.0, &seam);
        rings.push(vec![(hole, Sense::Reversed)]);
    }
    let surface = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(&surface, Sense::Forward, &rings, true);
    let plate = builder.finish(BodyKind::Sheet);
    assert_eq!(agreement_of(&plate), [Agreement::Equal]);
    let holes = plate.geometry.faces[0]
        .loops
        .stated()
        .cloned()
        .collect::<Vec<_>>()[1..]
        .to_vec();
    let message = no_box_in_either_pass(&listing(&plate, holes), "holes only");
    assert!(message.ends_with("boundary.nesting-invalid"), "{message}");
}

/// A plane face and a cylinder face whose one loop is a vertex loop: the
/// face is unbounded, so neither pass gives a box, rather than the box of
/// the point.
#[test]
fn the_box_pass_gives_no_box_for_a_face_bounded_by_a_vertex_loop() {
    let mut builder = Builder::new();
    let point = builder.vertex([1.0, 2.0, 0.0]);
    let plane = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.vertex_face(&plane, Sense::Forward, &[point]);
    let message = no_box_in_either_pass(&builder.finish(BodyKind::Sheet), "plane");
    assert!(message.ends_with("boundary.unbounded-face"), "{message}");

    let mut builder = Builder::new();
    let point = builder.vertex([10.0, 0.0, 5.0]);
    let cylinder = builder.cylinder(Frame::z_up([0.0; 3]), 10.0);
    builder.vertex_face(&cylinder, Sense::Forward, &[point]);
    let message = no_box_in_either_pass(&builder.finish(BodyKind::Sheet), "cylinder");
    assert!(message.ends_with("boundary.unbounded-face"), "{message}");
}

/// The NURBS rectangle whose loop record is missing has no region, and the
/// full pass gives no box. The box pass bounds it by its control hull,
/// which contains the whole carrier whatever the boundary says.
#[test]
fn the_box_pass_bounds_a_nurbs_face_without_a_region_by_its_control_hull() {
    let mut ir = nurbs_rectangle(None);
    let lost = ir.geometry.faces[0]
        .loops
        .stated()
        .cloned()
        .collect::<Vec<_>>();
    ir.geometry
        .loops
        .retain(|face_loop| !lost.contains(&face_loop.id));
    let report = measure(&ir, &Selection::default());
    let [row] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    assert!(row.bbox().measured().is_none());
    let boxes = body_boxes(&ir).expect("the box pass completes");
    let [body_box] = boxes.as_slice() else {
        panic!("one body box");
    };
    let measured = body_box.bbox().measured().expect("a box");
    assert_eq!(measured.reading(), None);
    let value = measured.value();
    assert_eq!(value.kind, BoxKind::Conservative);
    assert_eq!(value.boundary, None);
    let (low, high) = (value.min.get(), value.max.get());
    assert_eq!([low.x, low.y, low.z], [0.0, 0.0, 0.0]);
    assert_eq!([high.x, high.y, high.z], [2.0, 1.0, 0.0]);
    assert_eq!(agreement_of(&ir), [Agreement::ControlHull]);
}
