// SPDX-License-Identifier: Apache-2.0
//! Fit splines as cubic Bézier pieces.
//!
//! A fit spline passes through its points. Its interpolant is the natural
//! cubic spline in the chord-length parameter: the second derivative vanishes
//! at both ends, and the first and second derivatives agree at every interior
//! point. Each span is a cubic Hermite curve, which is a cubic Bézier curve.

/// A 2-vector.
pub(super) type V2 = [f64; 2];

const ZERO: V2 = [0.0; 2];

/// The chord length floor that keeps a repeated point finite.
const CHORD_FLOOR: f64 = 1e-9;

/// The cubic Bézier pieces of the natural interpolant through `points`, one
/// per span. `None` for fewer than two points.
pub(super) fn bezier_pieces(points: &[V2]) -> Option<Vec<[V2; 4]>> {
    let count = points.len();
    if count < 2 {
        return None;
    }
    let chords: Vec<f64> = points
        .windows(2)
        .map(|pair| dist(pair[0], pair[1]).max(CHORD_FLOOR))
        .collect();
    let interior = interior_second_derivatives(points, &chords);
    let second_at = |index: usize| {
        if index == 0 || index == count - 1 {
            ZERO
        } else {
            interior[index - 1]
        }
    };
    Some(
        (0..count - 1)
            .map(|span| {
                let chord = chords[span];
                let (start, end) = (points[span], points[span + 1]);
                let slope = scale(sub(end, start), 1.0 / chord);
                let (left, right) = (second_at(span), second_at(span + 1));
                let start_derivative = lin(slope, 1.0, lin(left, 2.0, right, 1.0), -chord / 6.0);
                let end_derivative = lin(slope, 1.0, lin(left, 1.0, right, 2.0), chord / 6.0);
                [
                    start,
                    lin(start, 1.0, scale(start_derivative, chord), 1.0 / 3.0),
                    lin(end, 1.0, scale(end_derivative, chord), -1.0 / 3.0),
                    end,
                ]
            })
            .collect(),
    )
}

/// The second derivatives at the interior points, by the tridiagonal solve of
/// the natural spline system.
fn interior_second_derivatives(points: &[V2], chords: &[f64]) -> Vec<V2> {
    let interior = points.len() - 2;
    let mut diagonal: Vec<f64> = (0..interior)
        .map(|index| 2.0 * (chords[index] + chords[index + 1]))
        .collect();
    let mut rhs: Vec<V2> = (0..interior)
        .map(|index| {
            lin(
                sub(points[index + 2], points[index + 1]),
                6.0 / chords[index + 1],
                sub(points[index + 1], points[index]),
                -6.0 / chords[index],
            )
        })
        .collect();
    for index in 1..interior {
        let weight = chords[index] / diagonal[index - 1];
        diagonal[index] -= weight * chords[index];
        rhs[index] = lin(rhs[index], 1.0, rhs[index - 1], -weight);
    }
    let mut solution: Vec<V2> = Vec::with_capacity(interior);
    for index in (0..interior).rev() {
        let following = solution
            .last()
            .map_or(ZERO, |&next| scale(next, chords[index + 1]));
        solution.push(scale(
            lin(rhs[index], 1.0, following, -1.0),
            1.0 / diagonal[index],
        ));
    }
    solution.reverse();
    solution
}

/// The knot vector on [0, 1] and the poles of the clamped cubic B-spline
/// whose spans are the given Bézier pieces.
pub(super) fn bezier_chain(pieces: &[[V2; 4]]) -> (Vec<f64>, Vec<V2>) {
    let total = pieces.iter().fold(0.0_f64, |count, _| count + 1.0);
    let mut knots = vec![0.0; 4];
    let mut join = 0.0;
    for _ in 1..pieces.len() {
        join += 1.0 / total;
        knots.extend([join; 3]);
    }
    knots.extend([1.0; 4]);
    let mut poles = vec![pieces[0][0]];
    for piece in pieces {
        poles.extend_from_slice(&piece[1..]);
    }
    (knots, poles)
}

fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn scale(a: V2, factor: f64) -> V2 {
    [a[0] * factor, a[1] * factor]
}

fn lin(a: V2, factor_a: f64, b: V2, factor_b: f64) -> V2 {
    [
        a[0] * factor_a + b[0] * factor_b,
        a[1] * factor_a + b[1] * factor_b,
    ]
}

fn dist(a: V2, b: V2) -> f64 {
    let difference = sub(a, b);
    difference[0].hypot(difference[1])
}

#[cfg(test)]
mod tests {
    use super::{bezier_chain, bezier_pieces, V2};

    fn second_derivative_at_start(piece: &[V2; 4]) -> V2 {
        [
            piece[0][0] - 2.0 * piece[1][0] + piece[2][0],
            piece[0][1] - 2.0 * piece[1][1] + piece[2][1],
        ]
    }

    fn second_derivative_at_end(piece: &[V2; 4]) -> V2 {
        [
            piece[1][0] - 2.0 * piece[2][0] + piece[3][0],
            piece[1][1] - 2.0 * piece[2][1] + piece[3][1],
        ]
    }

    #[test]
    fn the_pieces_join_the_points_in_order() {
        let points = [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0], [4.0, 4.0]];
        let pieces = bezier_pieces(&points).unwrap();
        assert_eq!(pieces.len(), 3);
        for (index, piece) in pieces.iter().enumerate() {
            assert_eq!(piece[0], points[index]);
            assert_eq!(piece[3], points[index + 1]);
        }
    }

    #[test]
    fn the_ends_have_zero_curvature() {
        let points = [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0], [4.0, 4.0], [6.0, 3.0]];
        let pieces = bezier_pieces(&points).unwrap();
        let first = second_derivative_at_start(&pieces[0]);
        let last = second_derivative_at_end(&pieces[pieces.len() - 1]);
        for component in 0..2 {
            assert!(first[component].abs() < 1e-9);
            assert!(last[component].abs() < 1e-9);
        }
    }

    #[test]
    fn the_second_derivative_is_continuous_at_each_point() {
        let points = [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0], [4.0, 4.0], [6.0, 3.0]];
        let pieces = bezier_pieces(&points).unwrap();
        let chord = |index: usize| {
            let difference = [
                points[index + 1][0] - points[index][0],
                points[index + 1][1] - points[index][1],
            ];
            difference[0].hypot(difference[1])
        };
        for (index, join) in pieces.windows(2).enumerate() {
            let before = second_derivative_at_end(&join[0]);
            let after = second_derivative_at_start(&join[1]);
            let (before_scale, after_scale) = (chord(index).powi(2), chord(index + 1).powi(2));
            for component in 0..2 {
                let left = before[component] / before_scale;
                let right = after[component] / after_scale;
                assert!((left - right).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn two_points_give_one_straight_span() {
        let pieces = bezier_pieces(&[[0.0, 0.0], [3.0, 3.0]]).unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0][1], [1.0, 1.0]);
        assert_eq!(pieces[0][2], [2.0, 2.0]);
    }

    #[test]
    fn one_point_has_no_span() {
        assert!(bezier_pieces(&[[0.0, 0.0]]).is_none());
    }

    #[test]
    fn the_chain_knots_repeat_at_each_join() {
        let pieces = bezier_pieces(&[[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]).unwrap();
        let (knots, poles) = bezier_chain(&pieces);
        assert_eq!(
            knots,
            vec![0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0]
        );
        assert_eq!(poles.len(), knots.len() - 4);
        assert_eq!(poles[0], [0.0, 0.0]);
        assert_eq!(poles[poles.len() - 1], [2.0, 0.0]);
    }
}
