// SPDX-License-Identifier: Apache-2.0

use super::*;

fn triangle() -> [Point2; 3] {
    [
        Point2::new(0.0, 0.0),
        Point2::new(3.0, 0.0),
        Point2::new(0.0, 3.0),
    ]
}

#[test]
fn certified_boundary_borrows_its_loop() {
    let loop_ = super::super::CertifiedProfileLoop::from_vertices(&triangle()).unwrap();
    let boundary = super::super::ProfileBoundary::CertifiedLoop(loop_);
    assert!(matches!(
        boundary.certified_loop(),
        Some(std::borrow::Cow::Borrowed(_))
    ));
}

fn tube_square(
    center: Point2,
    half: f64,
    per_side: usize,
    error: f64,
) -> Vec<super::super::CertifiedPiece> {
    let corners = [
        Point2::new(center.u - half, center.v - half),
        Point2::new(center.u + half, center.v - half),
        Point2::new(center.u + half, center.v + half),
        Point2::new(center.u - half, center.v + half),
    ];
    let steps = f64::from(u32::try_from(per_side).unwrap());
    let mut pieces = Vec::new();
    for (index, start) in corners.iter().enumerate() {
        let end = corners[(index + 1) % corners.len()];
        for step in 0..per_side {
            let at = |ordinal: usize| {
                let fraction = f64::from(u32::try_from(ordinal).unwrap()) / steps;
                Point2::new(
                    start.u + (end.u - start.u) * fraction,
                    start.v + (end.v - start.v) * fraction,
                )
            };
            pieces.push(super::super::CertifiedPiece::Tube(
                super::super::CertifiedCurveTube {
                    start: at(step),
                    end: at(step + 1),
                    error,
                },
            ));
        }
    }
    pieces
}

fn all_pairs_separated(
    left: &[super::super::CertifiedPiece],
    right: &[super::super::CertifiedPiece],
) -> bool {
    left.iter()
        .all(|left| right.iter().all(|right| left.separated_from(right)))
}

#[test]
fn certified_pieces_separated_matches_every_pair_test() {
    let origin = Point2::new(0.0, 0.0);
    let outer = tube_square(origin, 10.0, 40, 0.01);
    let cases = [
        tube_square(origin, 5.0, 40, 0.01),
        tube_square(origin, 9.995, 40, 0.01),
        tube_square(Point2::new(4.0, 0.0), 6.5, 40, 0.01),
        tube_square(Point2::new(30.0, 30.0), 5.0, 40, 0.01),
        tube_square(origin, 10.0, 40, 0.0),
    ];
    let expected = [true, false, false, true, false];
    for (inner, expected) in cases.iter().zip(expected) {
        assert_eq!(all_pairs_separated(&outer, inner), expected);
        assert_eq!(
            super::super::certified_pieces_separated(&outer, inner),
            expected
        );
        assert_eq!(
            super::super::certified_pieces_separated(inner, &outer),
            expected
        );
    }
}

#[test]
fn certified_pieces_separated_rejects_exact_lines_that_share_an_endpoint() {
    let shared = Point2::new(1.0, 1.0);
    let left = [super::super::CertifiedPiece::Exact(
        super::super::ProfileBoundarySegment::Line {
            start: Point2::new(0.0, 0.0),
            end: shared,
        },
    )];
    let right = [super::super::CertifiedPiece::Exact(
        super::super::ProfileBoundarySegment::Line {
            start: shared,
            end: Point2::new(2.0, 0.0),
        },
    )];
    assert!(!super::super::certified_pieces_separated(&left, &right));
}
