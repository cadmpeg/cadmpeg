// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::topology::Sense;

use crate::test_support::parasolid::{line_carrier, triangle_body};

/// Decode the triangle with its first edge on a compact line through the
/// edge's first two vertices, carrying `marker`.
fn decode_marked_line_edge(marker: u8) -> (String, String, Sense) {
    let mut body = triangle_body();
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
    let mut carrier = line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    carrier[18] = marker;
    body.extend(carrier);
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let brep =
        super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("curve-orientation")).unwrap();
    let edge = brep
        .edges
        .iter()
        .find(|edge| edge.curve().is_some())
        .unwrap();
    let coedge = brep
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge.id)
        .unwrap();
    (
        edge.start.as_str().to_owned(),
        edge.end.as_str().to_owned(),
        coedge.sense,
    )
}

#[test]
fn along_marker_keeps_the_stored_edge_direction() {
    let (start, end, sense) = decode_marked_line_edge(0x2d);
    assert!(start.ends_with("#50"), "{start}");
    assert!(end.ends_with("#51"), "{end}");
    assert_eq!(sense, Sense::Forward);
}

#[test]
fn against_marker_reverses_the_stored_edge_direction() {
    let (start, end, sense) = decode_marked_line_edge(0x2b);
    assert!(start.ends_with("#51"), "{start}");
    assert!(end.ends_with("#50"), "{end}");
    assert_eq!(sense, Sense::Reversed);
}
