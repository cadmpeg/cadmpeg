// SPDX-License-Identifier: Apache-2.0

use super::*;

const ARRANGEMENT_FACE_TEST_TOLERANCE: f64 = 1.0e-7;

#[test]
fn arrangement_admitted_route_keeps_two_faces() {
    let (sketch, entities, _, _) = super::coincident_circle_arc_arrangement();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let budget = super::local_arrangement_budget();
    let faces = super::sketch_arrangement_faces(
        &sketch,
        &entities,
        ARRANGEMENT_FACE_TEST_TOLERANCE,
        &budget,
        &ctx,
    )
    .unwrap()
    .unwrap();
    assert_eq!(faces.len(), 2);
}
