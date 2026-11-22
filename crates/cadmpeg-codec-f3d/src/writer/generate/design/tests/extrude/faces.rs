// SPDX-License-Identifier: Apache-2.0
//! Row 10 where several side faces lie on one surface: each loop entry
//! takes the face whose edge on the start cap runs between the ends of its
//! curve (f3d.md §3.1, "Side faces").

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::portable::Portable;

use super::super::{loss, write};
use super::arcs::{extruded, Side, Stroke};
use super::{kinds, TOLERANCE};

/// The model points of the vertices of the face named `token`.
pub(super) fn corners(decoded: &CadIr, token: &str) -> Vec<[f64; 3]> {
    let native = crate::test_support::native_test::f3d_native(decoded);
    let Some(AttributeTarget::Face(face)) = native
        .persistent_subentity_tags
        .iter()
        .find(|tag| tag.token.as_str() == token)
        .map(|tag| &tag.target)
    else {
        panic!("token {token} names a face");
    };
    let geometry = &decoded.geometry;
    let loops = geometry
        .loops
        .iter()
        .filter(|face_loop| &face_loop.face == face)
        .map(|face_loop| &face_loop.id)
        .collect::<Vec<_>>();
    geometry
        .coedges
        .iter()
        .filter(|coedge| loops.contains(&&coedge.owner_loop))
        .filter_map(|coedge| geometry.edges.iter().find(|edge| edge.id == coedge.edge))
        .flat_map(cadmpeg_ir::topology::Edge::vertices)
        .filter_map(|vertex| geometry.vertices.iter().find(|v| &v.id == vertex))
        .filter_map(|vertex| geometry.points.iter().find(|p| p.id == vertex.point))
        .map(|p| {
            let at = p.position().get();
            [at.x, at.y, at.z]
        })
        .collect()
}

/// Whether the side face named by each token has corners at both ends of
/// its curve on the start cap, `z = 0`.
pub(super) fn sides_follow_curves(decoded: &CadIr, expected: &[(u64, [f64; 2], [f64; 2])]) {
    for (token, a, b) in expected {
        let corners = corners(decoded, &token.to_string());
        let holds = |end: [f64; 2]| {
            corners.iter().any(|corner| {
                (corner[0] - end[0]).abs() <= TOLERANCE
                    && (corner[1] - end[1]).abs() <= TOLERANCE
                    && corner[2].abs() <= TOLERANCE
            })
        };
        assert!(holds(*a) && holds(*b), "token {token}: {corners:?}");
    }
}

/// The closed polygon through `corners` as source lines.
pub(super) fn lines(corners: &[[f64; 2]]) -> Vec<Stroke> {
    corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(a, b)| Stroke::Line(*a, *b))
        .collect()
}

/// A prism loop of lines through `corners`.
pub(super) fn flat(corners: &[[f64; 2]]) -> Vec<([f64; 2], Side)> {
    corners.iter().map(|corner| (*corner, Side::Line)).collect()
}

/// Two square holes side by side: their lower lines lie on one plane, and
/// so do their upper lines.
#[test]
fn a_plate_with_aligned_holes_is_written_as_an_extrude() {
    let outer = [[0.0, 0.0], [30.0, 0.0], [30.0, 20.0], [0.0, 20.0]];
    let left = [[5.0, 5.0], [10.0, 5.0], [10.0, 10.0], [5.0, 10.0]];
    let right = [[20.0, 5.0], [25.0, 5.0], [25.0, 10.0], [20.0, 10.0]];
    let strokes = [lines(&outer), lines(&left), lines(&right)].concat();
    let clockwise = |[a, b, c, d]: [[f64; 2]; 4]| flat(&[a, d, c, b]);
    let ir = extruded(&strokes, &[flat(&outer), clockwise(left), clockwise(right)]);
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    // The exterior from its last line; each hole clockwise from its last
    // line, in the order of its first curve.
    sides_follow_curves(
        &decoded,
        &[
            (3, [0.0, 20.0], [0.0, 0.0]),
            (4, [0.0, 0.0], [30.0, 0.0]),
            (5, [30.0, 0.0], [30.0, 20.0]),
            (6, [30.0, 20.0], [0.0, 20.0]),
            (7, [5.0, 10.0], [5.0, 5.0]),
            (8, [10.0, 10.0], [5.0, 10.0]),
            (9, [10.0, 5.0], [10.0, 10.0]),
            (10, [5.0, 5.0], [10.0, 5.0]),
            (11, [20.0, 10.0], [20.0, 5.0]),
            (12, [25.0, 10.0], [20.0, 10.0]),
            (13, [25.0, 5.0], [25.0, 10.0]),
            (14, [20.0, 5.0], [25.0, 5.0]),
        ],
    );
}

/// A slot open to the top edge splits the edge into two lines on one plane.
#[test]
fn a_plate_with_an_open_slot_is_written_as_an_extrude() {
    let strokes = [
        Stroke::Line([0.0, 0.0], [20.0, 0.0]),
        Stroke::Line([20.0, 0.0], [20.0, 10.0]),
        Stroke::Line([20.0, 10.0], [13.0, 10.0]),
        Stroke::Line([13.0, 10.0], [13.0, 5.0]),
        Stroke::Arc([10.0, 5.0], 3.0, [PI, 0.0]),
        Stroke::Line([7.0, 5.0], [7.0, 10.0]),
        Stroke::Line([7.0, 10.0], [0.0, 10.0]),
        Stroke::Line([0.0, 10.0], [0.0, 0.0]),
    ];
    let ir = extruded(
        &strokes,
        &[vec![
            ([0.0, 0.0], Side::Line),
            ([20.0, 0.0], Side::Line),
            ([20.0, 10.0], Side::Line),
            ([13.0, 10.0], Side::Line),
            (
                [13.0, 5.0],
                Side::Arc {
                    center: [10.0, 5.0],
                    counterclockwise: false,
                },
            ),
            ([7.0, 5.0], Side::Line),
            ([7.0, 10.0], Side::Line),
            ([0.0, 10.0], Side::Line),
        ]],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    sides_follow_curves(
        &decoded,
        &[
            (3, [0.0, 10.0], [0.0, 0.0]),
            (4, [0.0, 0.0], [20.0, 0.0]),
            (5, [20.0, 0.0], [20.0, 10.0]),
            (6, [20.0, 10.0], [13.0, 10.0]),
            (7, [13.0, 10.0], [13.0, 5.0]),
            (8, [13.0, 5.0], [7.0, 5.0]),
            (9, [7.0, 5.0], [7.0, 10.0]),
            (10, [7.0, 10.0], [0.0, 10.0]),
        ],
    );
}

/// Two arcs of one circle with a flat between them at each side: the two
/// arc faces lie on one cylinder, and their ends tell them apart.
#[test]
fn two_arcs_on_one_circle_are_written_as_an_extrude() {
    let half = 3.0_f64.portable_atan2(4.0);
    let strokes = [
        Stroke::Arc([0.0, 0.0], 5.0, [TAU - half, half]),
        Stroke::Line([4.0, 3.0], [-4.0, 3.0]),
        Stroke::Arc([0.0, 0.0], 5.0, [PI - half, PI + half]),
        Stroke::Line([-4.0, -3.0], [4.0, -3.0]),
    ];
    let arc = Side::Arc {
        center: [0.0, 0.0],
        counterclockwise: true,
    };
    let ir = extruded(
        &strokes,
        &[vec![
            ([4.0, -3.0], arc),
            ([4.0, 3.0], Side::Line),
            ([-4.0, 3.0], arc),
            ([-4.0, -3.0], Side::Line),
        ]],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    sides_follow_curves(
        &decoded,
        &[
            (3, [-4.0, -3.0], [4.0, -3.0]),
            (4, [4.0, -3.0], [4.0, 3.0]),
            (5, [4.0, 3.0], [-4.0, 3.0]),
            (6, [-4.0, 3.0], [-4.0, -3.0]),
        ],
    );
}

/// A circle drawn as two arcs: both arc faces lie on one cylinder between
/// the same two points, so neither arc names one face.
#[test]
fn a_circle_of_two_arcs_stays_in_the_base_feature() {
    let strokes = [
        Stroke::Arc([0.0, 0.0], 5.0, [0.0, PI]),
        Stroke::Arc([0.0, 0.0], 5.0, [PI, 0.0]),
    ];
    let arc = Side::Arc {
        center: [0.0, 0.0],
        counterclockwise: true,
    };
    let ir = extruded(&strokes, &[vec![([5.0, 0.0], arc), ([-5.0, 0.0], arc)]]);
    let (report, decoded) = write(&ir);
    assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
    let missing = loss(&report, "export/evaluation.missing").join("\n");
    assert!(
        missing.contains("its faces do not match its profile"),
        "{missing}"
    );
}
