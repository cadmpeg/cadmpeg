// SPDX-License-Identifier: Apache-2.0
//! Check findings through the crate facade, with expectations from the
//! listed rings and the stated senses of built documents.

use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::CadIr;

use crate::session::{findings, MeasureOptions};
use crate::test_support::{plate_with_hole, service_context};

fn run(ir: &CadIr) -> Vec<Finding> {
    let ctx = service_context();
    let found = findings(&ctx, ir, MeasureOptions::default()).expect("findings");
    ctx.finish_session().expect("session stays unrefused");
    found
}

/// The cube with the listed coedges of its first loop permuted by `order`.
fn cube_with_listing(order: [usize; 4]) -> (CadIr, String, String) {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let face_loop = &mut cube.model.loops[0];
    let listed = face_loop.coedges().to_vec();
    let permuted = order.map(|index| listed[index].clone()).to_vec();
    face_loop.replace_ring(permuted, Vec::new()).expect("ring");
    let loop_id = face_loop.id.as_str().to_owned();
    let face = face_loop.face.as_str().to_owned();
    (cube, loop_id, face)
}

fn only(found: &[Finding], check: Check) -> &Finding {
    let matching: Vec<&Finding> = found
        .iter()
        .filter(|finding| finding.check == check)
        .collect();
    let [finding] = matching.as_slice() else {
        panic!("one {check} finding: {found:#?}");
    };
    finding
}

#[test]
fn a_closed_cube_and_an_open_plate_have_no_findings() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    assert_eq!(run(&cube), Vec::new());
    assert_eq!(run(&plate_with_hole(true)), Vec::new());
}

#[test]
fn a_reversed_ring_listing_is_a_ring_order_warning() {
    let (cube, loop_id, face) = cube_with_listing([0, 3, 2, 1]);
    let found = run(&cube);
    let finding = only(&found, Check::RingOrder);
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.entity.as_deref(), Some(loop_id.as_str()));
    assert_eq!(finding.related, [face]);
    assert!(finding.message.contains("reversed"), "{}", finding.message);
    assert!(
        found
            .iter()
            .all(|finding| finding.check == Check::RingOrder),
        "the traversal and its winding are unchanged: {found:#?}"
    );
}

#[test]
fn a_ring_listed_in_another_order_is_a_ring_order_warning() {
    let (cube, loop_id, _) = cube_with_listing([0, 2, 1, 3]);
    let found = run(&cube);
    let finding = only(&found, Check::RingOrder);
    assert_eq!(finding.entity.as_deref(), Some(loop_id.as_str()));
    assert!(
        finding.message.contains("out of traversal order"),
        "{}",
        finding.message
    );
}

#[test]
fn a_ring_whose_coedges_do_not_chain_is_a_ring_order_warning() {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let loop_id = cube.model.loops[0].id.clone();
    let first = cube.model.loops[0].coedges()[0].clone();
    let coedge = cube
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == first)
        .expect("coedge");
    coedge.sense = match coedge.sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    };
    let found = run(&cube);
    let finding = only(&found, Check::RingOrder);
    assert_eq!(finding.entity.as_deref(), Some(loop_id.as_str()));
    assert!(
        finding.message.contains("no closed traversal"),
        "{}",
        finding.message
    );
}

#[test]
fn a_flipped_cube_face_is_an_orientation_warning_on_the_face_and_its_body() {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let flipped = cube.model.faces[0].id.as_str().to_owned();
    cube.model.faces[0].sense = match cube.model.faces[0].sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    };
    let body = cube.model.bodies[0].id.as_str().to_owned();
    let found = run(&cube);
    assert!(found.iter().all(
        |finding| finding.check == Check::Orientation && finding.severity == Severity::Warning
    ));
    let face = found
        .iter()
        .find(|finding| finding.entity.as_deref() == Some(flipped.as_str()))
        .expect("a face finding");
    assert!(face.message.contains("loop winding"), "{}", face.message);
    let body_finding = found
        .iter()
        .find(|finding| finding.entity.as_deref() == Some(body.as_str()))
        .expect("a body finding");
    assert!(
        body_finding.message.contains("volume.orientation-conflict"),
        "{}",
        body_finding.message
    );
    assert_eq!(body_finding.related, [flipped]);
    assert_eq!(found.len(), 2, "{found:#?}");
}
