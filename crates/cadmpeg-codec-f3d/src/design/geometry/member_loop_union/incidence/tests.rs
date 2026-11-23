// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::scalar::PositiveLength;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// The linear tolerance of these pieces, in centimetres.
const EPS_SAME_POINT: f64 = 1.0e-7;

#[test]
fn a_circle_with_no_node_is_one_piece() {
    assert_eq!(circle_piece(&[], 1, 1, EPS_SAME_POINT), Some([0.0, TAU]));
    assert_eq!(circle_piece(&[], 2, 1, EPS_SAME_POINT), None);
    assert_eq!(circle_piece(&[], 1, 2, EPS_SAME_POINT), None);
}

#[test]
fn circle_pieces_start_with_the_piece_that_holds_angle_zero() {
    let angles = [1.0, 2.0, 4.0, 5.0];
    assert_eq!(
        circle_piece(&angles, 1, 4, EPS_SAME_POINT),
        Some([5.0, 1.0 + TAU])
    );
    assert_eq!(
        circle_piece(&angles, 2, 4, EPS_SAME_POINT),
        Some([1.0, 2.0])
    );
    assert_eq!(
        circle_piece(&angles, 3, 4, EPS_SAME_POINT),
        Some([2.0, 4.0])
    );
    assert_eq!(
        circle_piece(&angles, 4, 4, EPS_SAME_POINT),
        Some([4.0, 5.0])
    );
}

#[test]
fn a_node_at_angle_zero_starts_the_first_circle_piece() {
    let angles = [0.0, PI, 4.0];
    assert_eq!(circle_piece(&angles, 1, 3, EPS_SAME_POINT), Some([0.0, PI]));
    assert_eq!(
        circle_piece(&angles, 3, 3, EPS_SAME_POINT),
        Some([4.0, TAU])
    );
}

#[test]
fn circle_pieces_reject_a_wrong_count_or_ordinal() {
    let angles = [1.0, 2.0, 4.0, 5.0];
    assert_eq!(circle_piece(&angles, 1, 3, EPS_SAME_POINT), None);
    assert_eq!(circle_piece(&angles, 0, 4, EPS_SAME_POINT), None);
    assert_eq!(circle_piece(&angles, 5, 4, EPS_SAME_POINT), None);
}

fn arc(start_angle: f64, end_angle: f64) -> ProfileBoundarySegment {
    ProfileBoundarySegment::Arc {
        center: Point2::new(1.0, 2.0),
        radius: PositiveLength::try_from(3.0).unwrap(),
        start_angle,
        end_angle,
    }
}

#[test]
fn an_arc_piece_is_covered_by_edges_on_its_circle() {
    let piece = arc(FRAC_PI_2, PI);
    // Two edges that meet cover the piece, in either direction.
    assert!(piece_is_covered(
        &piece,
        &[arc(FRAC_PI_2, 2.0), arc(PI, 2.0)],
        EPS_SAME_POINT
    ));
    // An edge that wraps past the angle origin covers it.
    assert!(piece_is_covered(
        &arc(-1.0, 1.0),
        &[arc(TAU - 2.0, TAU + 2.0)],
        EPS_SAME_POINT
    ));
    // A gap leaves it uncovered.
    assert!(!piece_is_covered(
        &piece,
        &[arc(FRAC_PI_2, 2.0), arc(2.5, PI)],
        EPS_SAME_POINT
    ));
}

#[test]
fn an_arc_piece_is_not_covered_by_edges_on_another_circle() {
    let other_radius = ProfileBoundarySegment::Arc {
        center: Point2::new(1.0, 2.0),
        radius: PositiveLength::try_from(4.0).unwrap(),
        start_angle: FRAC_PI_2,
        end_angle: PI,
    };
    let other_center = ProfileBoundarySegment::Arc {
        center: Point2::new(1.5, 2.0),
        radius: PositiveLength::try_from(3.0).unwrap(),
        start_angle: FRAC_PI_2,
        end_angle: PI,
    };
    let piece = arc(FRAC_PI_2, PI);
    assert!(!piece_is_covered(&piece, &[other_radius], EPS_SAME_POINT));
    assert!(!piece_is_covered(&piece, &[other_center], EPS_SAME_POINT));
}

#[test]
fn a_line_piece_is_covered_only_by_collinear_edges() {
    let line = |a: (f64, f64), b: (f64, f64)| ProfileBoundarySegment::Line {
        start: Point2::new(a.0, a.1),
        end: Point2::new(b.0, b.1),
    };
    let piece = line((0.0, 0.0), (4.0, 0.0));
    assert!(piece_is_covered(
        &piece,
        &[
            line((3.0, 0.0), (1.0, 0.0)),
            line((-1.0, 0.0), (1.5, 0.0)),
            line((2.0, 0.0), (5.0, 0.0))
        ],
        EPS_SAME_POINT
    ));
    assert!(!piece_is_covered(
        &piece,
        &[line((0.0, 0.0), (1.0, 0.0)), line((2.0, 0.0), (4.0, 0.0))],
        EPS_SAME_POINT
    ));
    assert!(!piece_is_covered(
        &piece,
        &[line((0.0, 1.0), (4.0, 1.0))],
        EPS_SAME_POINT
    ));
}
