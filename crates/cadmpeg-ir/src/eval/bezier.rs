// SPDX-License-Identifier: Apache-2.0
//! Homogeneous Bezier control-polygon splitting, restriction, and chord bounds.

use crate::math::Point3;
use cadmpeg_core::convert::f64_from_index;

pub(super) struct HomogeneousBezierSplit {
    left: Vec<[f64; 4]>,
    point: [f64; 4],
    right_reversed: Vec<[f64; 4]>,
}

impl HomogeneousBezierSplit {
    pub(super) fn into_polygons(mut self) -> [Vec<[f64; 4]>; 2] {
        self.left.push(self.point);
        self.right_reversed.push(self.point);
        self.right_reversed.reverse();
        [self.left, self.right_reversed]
    }
}

fn split_homogeneous_bezier(
    controls: &[[f64; 4]],
    parameter: f64,
) -> Option<HomogeneousBezierSplit> {
    if !parameter.is_finite() || !(0.0..=1.0).contains(&parameter) {
        return None;
    }
    split_homogeneous_bezier_with(controls, |left, right| {
        std::array::from_fn(|axis| (1.0 - parameter) * left[axis] + parameter * right[axis])
    })
}

pub(super) fn split_homogeneous_bezier_midpoint(
    controls: &[[f64; 4]],
) -> Option<HomogeneousBezierSplit> {
    split_homogeneous_bezier_with(controls, |left, right| {
        std::array::from_fn(|axis| 0.5 * (left[axis] + right[axis]))
    })
}

fn split_homogeneous_bezier_with(
    controls: &[[f64; 4]],
    blend: impl Fn([f64; 4], [f64; 4]) -> [f64; 4],
) -> Option<HomogeneousBezierSplit> {
    let (&first, input_rest) = controls.split_first()?;
    let mut first = first;
    let mut rest = input_rest.to_vec();
    let mut next = vec![[0.0; 4]; rest.len()];
    let mut left = vec![[0.0; 4]; rest.len()];
    let mut right = vec![[0.0; 4]; rest.len()];
    let mut remaining = rest.len();
    let mut level = 0;
    while remaining > 0 {
        let second = rest[0];
        left[level] = first;
        right[level] = rest[remaining - 1];
        first = blend(first, second);
        for index in 0..remaining - 1 {
            next[index] = blend(rest[index], rest[index + 1]);
        }
        std::mem::swap(&mut rest, &mut next);
        remaining -= 1;
        level += 1;
    }
    Some(HomogeneousBezierSplit {
        left,
        point: first,
        right_reversed: right,
    })
}

/// The restricted polygon has one control per input control.
pub(super) fn restrict_homogeneous_bezier(
    controls: &[[f64; 4]],
    start: f64,
    end: f64,
) -> Option<Vec<[f64; 4]>> {
    let reverse = start > end;
    let (start, end) = if reverse { (end, start) } else { (start, end) };
    let mut result = if start == end {
        let split = split_homogeneous_bezier(controls, start)?;
        vec![split.point; controls.len()]
    } else {
        let [left, _] = split_homogeneous_bezier(controls, end)?.into_polygons();
        if start == 0.0 {
            left
        } else {
            let [_, right] = split_homogeneous_bezier(&left, start / end)?.into_polygons();
            right
        }
    };
    if reverse {
        result.reverse();
    }
    Some(result)
}

pub(super) fn binomial_coefficient(degree: usize, index: usize) -> Option<f64> {
    let index = index.min(degree - index);
    let mut value = 1.0;
    for factor in 1..=index {
        let numerator = f64_from_index(degree - index + factor)?;
        let denominator = f64_from_index(factor)?;
        value = value * numerator / denominator;
    }
    Some(value)
}

pub(super) fn point_on_chord(chord: [Point3; 2], parameter: f64) -> Point3 {
    Point3::new(
        chord[0].x + parameter * (chord[1].x - chord[0].x),
        chord[0].y + parameter * (chord[1].y - chord[0].y),
        chord[0].z + parameter * (chord[1].z - chord[0].z),
    )
}

pub(super) fn rational_curve_chord_bound(controls: &[[f64; 4]], chord: [Point3; 2]) -> Option<f64> {
    let degree = controls.len().checked_sub(1)?;
    let elevated_degree = degree + 1;
    let mut bound = 0.0_f64;
    let mut coordinate_scale = chord
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .fold(1.0_f64, |scale, coordinate| scale.max(coordinate.abs()));
    for index in 0..=elevated_degree {
        let previous = index.checked_sub(1).and_then(|index| controls.get(index));
        let current = controls.get(index);
        let (Some(numerator), Some(denominator)) =
            (f64_from_index(index), f64_from_index(elevated_degree))
        else {
            return None;
        };
        let previous_factor = numerator / denominator;
        let current_factor = 1.0 - previous_factor;
        let weight = previous_factor * previous.map_or(0.0, |control| control[3])
            + current_factor * current.map_or(0.0, |control| control[3]);
        if !weight.is_finite() || weight <= 0.0 {
            return None;
        }
        let mut residual_norm = 0.0_f64;
        for (axis, chord_coordinates) in [
            [chord[0].x, chord[1].x],
            [chord[0].y, chord[1].y],
            [chord[0].z, chord[1].z],
        ]
        .into_iter()
        .enumerate()
        {
            let coordinate = previous_factor * previous.map_or(0.0, |control| control[axis])
                + current_factor * current.map_or(0.0, |control| control[axis]);
            let weighted_chord =
                current_factor * current.map_or(0.0, |control| control[3]) * chord_coordinates[0]
                    + previous_factor
                        * previous.map_or(0.0, |control| control[3])
                        * chord_coordinates[1];
            let residual = (coordinate - weighted_chord) / weight;
            if !residual.is_finite() {
                return None;
            }
            residual_norm = residual_norm.hypot(residual);
            coordinate_scale = coordinate_scale.max((coordinate / weight).abs());
        }
        bound = bound.max(residual_norm);
    }
    let rounding_margin = 256.0 * f64::EPSILON * coordinate_scale.max(bound);
    (bound.is_finite() && rounding_margin.is_finite()).then_some(bound + rounding_margin)
}

#[cfg(test)]
mod tests {
    use super::{restrict_homogeneous_bezier, split_homogeneous_bezier_midpoint};

    #[test]
    fn homogeneous_split_returns_both_polygons() {
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        let [left, right] = split_homogeneous_bezier_midpoint(&controls)
            .expect("nonempty")
            .into_polygons();
        assert_eq!(&*left, &[[0.0, 0.0, 0.0, 1.0], [0.5, 0.0, 0.0, 1.0]]);
        assert_eq!(&*right, &[[0.5, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]]);
    }

    #[test]
    fn homogeneous_restriction_preserves_forward_reverse_and_collapsed_polygons() {
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        for (start, end) in [(0.25, 0.75), (0.75, 0.25), (0.5, 0.5), (0.0, 1.0)] {
            let result =
                restrict_homogeneous_bezier(&controls, start, end).expect("valid interval");
            assert_eq!(&*result, &[[start, 0.0, 0.0, 1.0], [end, 0.0, 0.0, 1.0]]);
        }
    }

    #[test]
    fn bezier_bounds_compute_coefficients_and_chord_margin() {
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        let chord = [
            crate::math::Point3::new(0.0, 0.0, 0.0),
            crate::math::Point3::new(1.0, 0.0, 0.0),
        ];
        assert_eq!(super::binomial_coefficient(4, 2), Some(6.0));
        assert_eq!(
            super::rational_curve_chord_bound(&controls, chord),
            Some(256.0 * f64::EPSILON)
        );
    }
}
