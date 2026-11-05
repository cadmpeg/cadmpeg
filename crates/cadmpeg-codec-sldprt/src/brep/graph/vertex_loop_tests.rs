// SPDX-License-Identifier: Apache-2.0

//! Vertex loops: a FACE whose loop chain holds a ring loop and a vertex loop.

use crate::test_support::parasolid::sequence::{sequence_of, F, NULL_REAL};
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
    sequence_of(&nodes)
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

#[test]
fn a_face_keeps_its_vertex_loop_beside_its_ring_loop_on_the_scanner_path() {
    use crate::test_support::parasolid::{
        framed_topology_vertex, loop_head, triangle_body, world_point,
    };
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
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
    let brep = super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("vertex-loop"), false)
        .expect("triangle decodes");
    assert_vertex_loop_beside_ring(&brep, "#53");
}
