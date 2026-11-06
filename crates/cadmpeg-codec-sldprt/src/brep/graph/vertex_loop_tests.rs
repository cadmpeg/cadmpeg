// SPDX-License-Identifier: Apache-2.0

//! Vertex loops: a FACE whose loop chain holds a ring loop and a vertex loop.

use crate::test_support::parasolid::sequence::{sequence_of, F, NULL_REAL};
use crate::test_support::parasolid::{
    framed_topology_fin, framed_topology_vertex, loop_head, triangle_body, world_point,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::topology::LoopBoundary;

fn decode_stream(payload: &[u8]) -> super::Brep {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    let header = crate::parasolid::stream_header(payload)
        .expect("synthetic header parses")
        .expect("synthetic header present");
    super::decode_bodies(
        &ctx,
        &[(payload, &header)],
        &cadmpeg_ir::stream_name!("vertex-loop"),
    )
    .expect("stream decodes")
}

/// A partition node sequence: FACE 5 on PLANE 20 with ring LOOP 6 (FIN 7 on
/// EDGE 8, a CIRCLE 21 closed at VERTEX 9) followed by vertex LOOP 11 (FIN 12
/// at VERTEX 13 on the circle centre). `point` includes POINT 14 of VERTEX 13.
fn two_loop_face(point: bool) -> (Vec<u8>, usize) {
    sequence_of(&two_loop_face_nodes(point))
}

type Node = (u16, u16, Vec<F>);

fn two_loop_face_nodes(point: bool) -> Vec<Node> {
    use F::{B, C, I, P, R, V};
    #[rustfmt::skip]
    let mut nodes = vec![
        (12, 2, vec![
            I(10), P(1), P(1), P(1), P(1), P(1), P(1), R(1000.0), R(1.0e-8), P(1), P(1), P(1),
            B(1), P(1), B(1), B(0), P(4), P(1), P(1), P(1), P(3), P(1), P(1),
        ]),
        (19, 3, vec![I(2), P(1), P(2), P(1), P(1), P(4), C(b'S')]),
        (13, 4, vec![I(3), P(1), P(2), P(1), P(5), P(1), P(1), P(3), P(1)]),
        (14, 5, crate::test_support::parasolid::sequence::face_fields()),
        (15, 6, vec![I(5), P(1), P(7), P(5), P(11)]),
        (15, 11, vec![I(11), P(1), P(12), P(5), P(1)]),
        (17, 7, vec![P(1), P(6), P(7), P(7), P(9), P(1), P(8), P(1), P(1), C(b'+')]),
        (17, 12, vec![P(1), P(11), P(12), P(12), P(13), P(1), P(1), P(1), P(1), C(b'?')]),
        (16, 8, vec![
            I(6), P(1), R(NULL_REAL), P(7), P(1), P(1), P(21), P(1), P(1), P(2),
        ]),
        (18, 9, vec![I(7), P(1), P(7), P(1), P(1), P(10), R(2.0e-8), P(2)]),
        (18, 13, vec![I(12), P(1), P(12), P(1), P(1), P(14), R(2.0e-8), P(2)]),
        (29, 10, vec![I(8), P(1), P(9), P(1), P(1), V([0.002, 0.0, 0.0])]),
        (50, 20, vec![
            I(14), P(1), P(1), P(1), P(1), P(1), C(b'+'),
            V([0.0; 3]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0]),
        ]),
        (31, 21, vec![
            I(15), P(1), P(1), P(1), P(1), P(1), C(b'+'),
            V([0.0; 3]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0]), R(0.002),
        ]),
    ];
    if point {
        nodes.push((29, 14, vec![I(13), P(1), P(13), P(1), P(1), V([0.0; 3])]));
    }
    nodes
}

fn assert_vertex_loop_beside_ring(brep: &super::Brep, vertex_suffix: &str) {
    assert_eq!(brep.faces.len(), 1);
    let face_loops: Vec<_> = brep.faces[0].loops.iter().collect();
    assert_eq!(face_loops.len(), 2);
    let boundaries: Vec<_> = face_loops
        .iter()
        .map(|id| {
            &brep
                .loops
                .iter()
                .find(|lp| lp.id == **id)
                .expect("face loop")
                .boundary
        })
        .collect();
    assert_eq!(
        boundaries
            .iter()
            .filter(|boundary| matches!(boundary, LoopBoundary::Ring(_)))
            .count(),
        1
    );
    let vertex = boundaries
        .iter()
        .find_map(|boundary| match boundary {
            LoopBoundary::Vertex { vertex, .. } => Some(vertex),
            LoopBoundary::Ring(_) => None,
        })
        .expect("vertex loop");
    assert!(vertex.as_str().ends_with(vertex_suffix));
    assert!(brep
        .vertices
        .iter()
        .any(|candidate| candidate.id == *vertex));
}

#[test]
fn a_face_keeps_its_vertex_loop_beside_its_ring_loop_on_the_node_path() {
    let (payload, _) = two_loop_face(true);
    let brep = decode_stream(&payload);
    assert_vertex_loop_beside_ring(&brep, "#13");
    assert_eq!(brep.stats.withheld_vertex_loop_faces, 0);
}

#[test]
fn a_face_whose_vertex_loop_names_no_resolved_vertex_is_withheld() {
    let (payload, _) = two_loop_face(false);
    let brep = decode_stream(&payload);
    assert!(brep.faces.is_empty());
    assert_eq!(brep.stats.withheld_vertex_loop_faces, 1);
}

/// The triangle body whose FACE 10 holds ring LOOP 20 followed by vertex
/// LOOP 21 (FIN 33 at VERTEX 53 on POINT 63).
fn ring_and_vertex_loop_triangle() -> Vec<u8> {
    let mut body = triangle_body();
    let head = loop_head(20, 30, 10);
    let at = body
        .windows(head.len())
        .position(|window| window == head.as_slice())
        .expect("triangle loop");
    body[at + 14..at + 16].copy_from_slice(&21u16.to_be_bytes());
    let last_point = world_point(62, [0.0, 1.0, 0.0]);
    let at = body
        .windows(last_point.len())
        .position(|window| window == last_point.as_slice())
        .expect("triangle point")
        + last_point.len();
    let mut vertex_loop = loop_head(21, 33, 10);
    vertex_loop.extend([0x00, 0x11]);
    for reference in [33u16, 0, 21, 33, 33, 53, 1, 1, 0, 0] {
        vertex_loop.extend(reference.to_be_bytes());
    }
    vertex_loop.push(b'?');
    vertex_loop.extend(framed_topology_vertex(
        53,
        0,
        [0, 0, 0, 0, 63],
        None,
        900,
        false,
        false,
    ));
    vertex_loop.extend(world_point(63, [0.25, 0.25, 0.0]));
    body.splice(at..at, vertex_loop);
    body
}

fn decode_scanned(body: &[u8]) -> super::Brep {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    super::decode_body(&ctx, body, &cadmpeg_ir::stream_name!("vertex-loop"), false)
        .expect("triangle decodes")
}

#[test]
fn a_face_keeps_its_vertex_loop_beside_its_ring_loop_on_the_scanner_path() {
    let brep = decode_scanned(&ring_and_vertex_loop_triangle());
    assert_vertex_loop_beside_ring(&brep, "#53");
}

#[test]
fn a_face_with_a_valid_ring_and_an_ambiguous_vertex_loop_fin_is_withheld() {
    let mut body = ring_and_vertex_loop_triangle();
    let mut conflicting = vec![0x00, 0x11];
    for reference in [33u16, 0, 21, 33, 33, 54, 1, 1, 0, 0] {
        conflicting.extend(reference.to_be_bytes());
    }
    conflicting.push(b'?');
    body.extend(conflicting);
    let brep = decode_scanned(&body);
    assert!(brep.faces.is_empty());
    assert_eq!(brep.stats.unresolved_topology_fins, 1);
    assert_eq!(brep.stats.withheld_ambiguous_fin_faces, 1);
}

#[test]
fn a_face_with_a_vertex_loop_and_an_ambiguous_ring_fin_is_withheld() {
    let mut body = ring_and_vertex_loop_triangle();
    body.extend(framed_topology_fin(
        31,
        [1, 20, 0, 32, 51, 0, 41, 0, 0],
        false,
        false,
        false,
    ));
    let brep = decode_scanned(&body);
    assert!(brep.faces.is_empty());
    assert_eq!(brep.stats.unresolved_topology_fins, 1);
    assert_eq!(brep.stats.withheld_ambiguous_fin_faces, 1);
}

/// FACE 30 on `OFFSET_SURF` 40 over PLANE 20, the carrier of FACE 5 whose
/// vertex loop names no resolved vertex, with ring LOOP 31 (FIN 32 on EDGE
/// 33, a CIRCLE 41 closed at VERTEX 34).
#[test]
fn a_retained_face_on_a_withheld_face_carrier_emits_a_hidden_support() {
    use F::{C, I, L, P, R, V};
    let mut nodes = two_loop_face_nodes(false);
    let face = nodes
        .iter_mut()
        .find(|(kind, index, _)| (*kind, *index) == (14, 5))
        .expect("face 5");
    face.2[3] = P(30);
    #[rustfmt::skip]
    nodes.extend([
        (14, 30, vec![
            I(20), P(1), R(NULL_REAL), P(1), P(5), P(31), P(4), P(40), C(b'+'),
            P(1), P(1), P(1), P(1), P(1),
        ]),
        (15, 31, vec![I(21), P(1), P(32), P(30), P(1)]),
        (17, 32, vec![P(1), P(31), P(32), P(32), P(34), P(1), P(33), P(1), P(1), C(b'+')]),
        (16, 33, vec![
            I(22), P(1), R(NULL_REAL), P(32), P(1), P(1), P(41), P(1), P(1), P(2),
        ]),
        (18, 34, vec![I(23), P(1), P(32), P(1), P(1), P(35), R(2.0e-8), P(2)]),
        (29, 35, vec![I(24), P(1), P(34), P(1), P(1), V([0.002, 0.0, 0.002])]),
        (60, 40, vec![
            I(25), P(1), P(1), P(1), P(1), P(1), C(b'+'), C(b'V'), L(false), P(20),
            R(0.002), R(1.0),
        ]),
        (31, 41, vec![
            I(26), P(1), P(1), P(1), P(1), P(1), C(b'+'),
            V([0.0, 0.0, 0.002]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0]), R(0.002),
        ]),
    ]);
    let (payload, _) = sequence_of(&nodes);
    let brep = decode_stream(&payload);
    assert_eq!(brep.stats.withheld_vertex_loop_faces, 1);
    assert_eq!(brep.faces.len(), 1);
    assert!(!brep
        .surfaces
        .iter()
        .any(|surface| surface.id == super::id_surf(5)));
    assert!(brep
        .surfaces
        .iter()
        .any(|surface| surface.id == super::id_hidden_support_surface(20)));
}
