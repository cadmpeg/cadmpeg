// SPDX-License-Identifier: Apache-2.0
//! Homogeneous Bezier control-polygon splitting, restriction, and chord bounds.

use crate::geometry::nurbs::scratch;
use crate::math::Point3;
use cadmpeg_core::decode::ResourceLimit;

pub(super) struct HomogeneousBezierSplit {
    left: Vec<[f64; 4]>,
    point: [f64; 4],
    right_reversed: Vec<[f64; 4]>,
}

impl HomogeneousBezierSplit {
    pub(super) fn into_polygons(mut self) -> (Vec<[f64; 4]>, Vec<[f64; 4]>) {
        self.left.push(self.point);
        self.right_reversed.push(self.point);
        self.right_reversed.reverse();
        (self.left, self.right_reversed)
    }
}

fn split_homogeneous_bezier(
    controls: &[[f64; 4]],
    parameter: f64,
) -> Result<Option<HomogeneousBezierSplit>, ResourceLimit> {
    if !parameter.is_finite() || !(0.0..=1.0).contains(&parameter) {
        return Ok(None);
    }
    split_homogeneous_bezier_with(controls, |left, right| {
        std::array::from_fn(|axis| (1.0 - parameter) * left[axis] + parameter * right[axis])
    })
}

pub(super) fn split_homogeneous_bezier_midpoint(
    controls: &[[f64; 4]],
) -> Result<Option<HomogeneousBezierSplit>, ResourceLimit> {
    split_homogeneous_bezier_with(controls, |left, right| {
        std::array::from_fn(|axis| 0.5 * (left[axis] + right[axis]))
    })
}

fn split_homogeneous_bezier_with(
    controls: &[[f64; 4]],
    blend: impl Fn([f64; 4], [f64; 4]) -> [f64; 4],
) -> Result<Option<HomogeneousBezierSplit>, ResourceLimit> {
    let Some((&first, rest)) = controls.split_first() else {
        return Ok(None);
    };
    let mut first = first;
    let mut rest = scratch::filled(rest.len(), [0.0; 4], "IR Bezier split controls")?;
    rest.copy_from_slice(&controls[1..]);
    let mut next = scratch::filled(rest.len(), [0.0; 4], "IR Bezier split level")?;
    let mut left = scratch::filled(rest.len(), [0.0; 4], "IR Bezier split left")?;
    let mut right = scratch::filled(rest.len(), [0.0; 4], "IR Bezier split right")?;
    scratch::reserve_exact(&mut left, 1, "IR Bezier split final left point")?;
    scratch::reserve_exact(&mut right, 1, "IR Bezier split final right point")?;
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
    Ok(Some(HomogeneousBezierSplit {
        left,
        point: first,
        right_reversed: right,
    }))
}

/// The restricted polygon has one control per admitted input control.
pub(super) fn restrict_homogeneous_bezier(
    controls: &[[f64; 4]],
    start: f64,
    end: f64,
) -> Result<Option<Vec<[f64; 4]>>, ResourceLimit> {
    if start > end {
        let Some(mut restricted) = restrict_homogeneous_bezier(controls, end, start)? else {
            return Ok(None);
        };
        restricted.reverse();
        return Ok(Some(restricted));
    }
    if start == end {
        let Some(point) = split_homogeneous_bezier(controls, start)?.map(|split| split.point)
        else {
            return Ok(None);
        };
        return scratch::filled(controls.len(), point, "ir_bezier_collapsed_controls").map(Some);
    }
    let Some(left) =
        split_homogeneous_bezier(controls, end)?.map(HomogeneousBezierSplit::into_polygons)
    else {
        return Ok(None);
    };
    let left = left.0;
    if start == 0.0 {
        return Ok(Some(left));
    }
    let relative_start = start / end;
    Ok(split_homogeneous_bezier(&left, relative_start)?.map(|split| split.into_polygons().1))
}

pub(super) fn binomial_coefficient(degree: usize, index: usize) -> f64 {
    let index = index.min(degree - index);
    (1..=index).fold(1.0, |value, factor| {
        value * (degree - index + factor) as f64 / factor as f64
    })
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
        let previous_factor = index as f64 / elevated_degree as f64;
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
