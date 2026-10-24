// SPDX-License-Identifier: Apache-2.0
use super::traversal_order;
use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{LoopBoundary, Sense};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn a_continuous_serialized_order_is_the_traversal() {
    let ends = [("a", "b"), ("b", "c"), ("c", "a")];
    assert_eq!(traversal_order(&ends), [0, 1, 2]);
}

#[test]
fn a_reversed_serialized_order_chains_from_its_first_use() {
    let ends = [("a", "b"), ("d", "a"), ("c", "d"), ("b", "c")];
    assert_eq!(traversal_order(&ends), [0, 3, 2, 1]);
}

#[test]
fn a_permuted_serialized_order_chains_from_its_first_use() {
    let ends = [("b", "a"), ("c", "b"), ("a", "d"), ("d", "c")];
    assert_eq!(traversal_order(&ends), [0, 2, 3, 1]);
}

#[test]
fn a_seam_wire_chains_through_its_closed_edges() {
    let ends = [("w", "v"), ("w", "w"), ("v", "w"), ("v", "v")];
    let order = traversal_order(&ends);
    assert_eq!(order, [0, 3, 2, 1]);
}

#[test]
fn several_closed_chains_keep_the_serialized_order() {
    let ends = [("x", "y"), ("x", "y"), ("y", "x"), ("y", "x")];
    assert_eq!(traversal_order(&ends), [0, 1, 2, 3]);
}

#[test]
fn an_open_chain_keeps_the_serialized_order() {
    let ends = [("b", "c"), ("a", "b"), ("c", "d")];
    assert_eq!(traversal_order(&ends), [0, 1, 2]);
}

#[test]
fn a_long_reversed_serialized_order_chains_from_its_first_use() {
    const USES: usize = 16_000;
    let ends: Vec<(usize, usize)> = std::iter::once((0, 1))
        .chain((1..USES).map(|position| (USES - position, (USES - position + 1) % USES)))
        .collect();
    let traversal: Vec<usize> = std::iter::once(0).chain((1..USES).rev()).collect();
    assert!(
        traversal_order(&ends) == traversal,
        "the reversed wire resolves to its traversal"
    );
}

#[test]
fn an_empty_wire_has_no_traversal() {
    let ends: [(&str, &str); 0] = [];
    assert!(traversal_order(&ends).is_empty());
}

/// A unit square face on the plane `z = 0` with normal `+Z`. Its wire
/// serializes the edge uses A→B, D→A, C→D, B→C.
const REVERSED_SQUARE_WIRE: &[u8] = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 4
1 0 0 0 1 0 0
1 1 0 0 0 1 0
1 0 1 0 1 0 0
1 0 0 0 0 1 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 10
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ve 0.001 1 1 0 0 0 1001000 *
Ve 0.001 0 1 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +10 0 -9 0 *
Ed 0.001 1 1 0 1 2 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 3 0 0 1 0 1001000 +7 0 -8 0 *
Ed 0.001 1 1 0 1 4 0 0 1 0 1001000 +10 0 -7 0 *
Wi 1001000 +6 0 -3 0 -4 0 +5 0 *
Fa 0 0.001 1 0 1001000 +2 0 *
+1 0 *";

#[test]
fn a_face_ring_runs_in_traversal_order_whatever_the_wire_serialization() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Shape.brp", REVERSED_SQUARE_WIRE),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("square face");
    let model = &result.ir().model;
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.faces[0].sense, Sense::Forward);
    let [face_loop] = model.loops.as_slice() else {
        panic!("one loop expected");
    };
    let LoopBoundary::Ring(ring) = &face_loop.boundary else {
        panic!("edge loop expected");
    };
    let point_of = |vertex: &cadmpeg_ir::ids::VertexId| -> Point3 {
        let vertex = model
            .vertices
            .iter()
            .find(|item| item.id == *vertex)
            .expect("vertex");
        model
            .points
            .iter()
            .find(|item| item.id == vertex.point)
            .expect("point")
            .position()
            .get()
    };
    let oriented: Vec<(Point3, Point3)> = ring
        .coedges()
        .iter()
        .map(|id| {
            let coedge = model
                .coedges
                .iter()
                .find(|item| item.id == *id)
                .expect("coedge");
            let edge = model
                .edges
                .iter()
                .find(|item| item.id == coedge.edge)
                .expect("edge");
            match coedge.sense {
                Sense::Forward => (point_of(&edge.start), point_of(&edge.end)),
                Sense::Reversed => (point_of(&edge.end), point_of(&edge.start)),
            }
        })
        .collect();
    let starts: Vec<Point3> = oriented.iter().map(|(start, _)| *start).collect();
    let corner = |x: f64, y: f64| Point3::new(x, y, 0.0);
    assert_eq!(
        starts,
        [
            corner(0.0, 0.0),
            corner(1.0, 0.0),
            corner(1.0, 1.0),
            corner(0.0, 1.0)
        ]
    );
    for (index, (_, end)) in oriented.iter().enumerate() {
        assert_eq!(*end, oriented[(index + 1) % oriented.len()].0);
    }
}
