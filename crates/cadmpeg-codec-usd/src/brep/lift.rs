// SPDX-License-Identifier: Apache-2.0
//! The space curve `S(c(t))` of a NURBS pcurve `c` on a NURBS surface `S`,
//! as an exact NURBS curve.
//!
//! Between the pcurve knots and the parameters where the pcurve crosses an
//! interior knot line of the surface, the homogeneous composition
//! `w(t)^(p + q) · Sʷ(c(t))` is one polynomial of degree at most
//! `d·(p + q)`, for a pcurve of degree `d` and weight `w` on a surface of
//! degrees `(p, q)`; a lower power of `w` can clear the denominator too.
//! Each piece is recovered by interpolation in the Bernstein basis at the
//! lowest degree, and the lowest power of `w`, that reproduces it. The
//! pieces join at knots of full multiplicity, which are then removed as far
//! as the curve allows.

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_ir::math::portable::Portable;

use super::convert::{surface_homogeneous, surface_point};
use super::nurbs::Nurbs;
use super::{Form, NurbsCurveRecord, NurbsSurfaceRecord};

/// The highest degree of a lifted curve.
const MAX_DEGREE: usize = 30;
/// The most pieces of one lifted curve.
const MAX_PIECES: usize = 4096;
/// The most piece fits, over all degrees and weight powers, for one curve.
const MAX_FITS: usize = 100_000;
/// Halvings of a Bézier segment that isolate a knot-line crossing.
const CROSSING_DEPTH: usize = 52;
/// Parameters closer than this fraction of the pcurve domain are one break.
const BREAK_MERGE: f64 = 1e-12;
/// Points of each piece where a fit is compared with the composition.
const CHECK_POINTS: usize = 9;
/// Agreement of a fitted piece with the composition, as a fraction of the
/// extent of the surface control net.
const FIT_AGREEMENT: f64 = 1e-9;

/// The weight of `curve` at `t`.
fn weight_at(curve: &Nurbs<2>, t: f64) -> Option<f64> {
    Nurbs::<1> {
        degree: curve.degree,
        knots: curve.knots.clone(),
        points: curve.weights.iter().map(|weight| [*weight]).collect(),
        weights: curve.weights.iter().map(|_| 1.0).collect(),
    }
    .point_at(t)
    .map(|[weight]| weight)
}

/// Splits Bernstein coefficients at the middle of their interval.
fn halves(coefficients: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut work = coefficients.to_vec();
    let mut left = Vec::with_capacity(work.len());
    let mut right = Vec::with_capacity(work.len());
    while let Some(first) = work.first() {
        left.push(*first);
        right.push(*work.last().unwrap_or(first));
        work = work
            .windows(2)
            .map(|pair| 0.5 * (pair[0] + pair[1]))
            .collect();
    }
    right.reverse();
    (left, right)
}

/// Pushes to `out` the parameters in `[low, high]` where the polynomial of
/// Bernstein coefficients `coefficients` vanishes or changes sign.
fn sign_changes(coefficients: &[f64], low: f64, high: f64, depth: usize, out: &mut Vec<f64>) {
    let (Some(first), Some(last)) = (coefficients.first(), coefficients.last()) else {
        return;
    };
    if *first == 0.0 {
        out.push(low);
    }
    if *last == 0.0 {
        out.push(high);
    }
    let signed = || coefficients.iter().filter(|value| **value != 0.0);
    if signed().all(|value| *value > 0.0) || signed().all(|value| *value < 0.0) {
        return;
    }
    let middle = f64::midpoint(low, high);
    if depth == 0 {
        out.push(middle);
        return;
    }
    let (left, right) = halves(coefficients);
    sign_changes(&left, low, middle, depth - 1, out);
    sign_changes(&right, middle, high, depth - 1, out);
}

/// The distinct interior knots of a surface direction.
fn interior_knots(knots: &[f64], order: usize, count: usize) -> Vec<f64> {
    let (Some(low), Some(high)) = (knots.get(order - 1), knots.get(count)) else {
        return Vec::new();
    };
    let mut values: Vec<f64> = knots
        .iter()
        .copied()
        .filter(|knot| low < knot && knot < high)
        .collect();
    values.dedup();
    values
}

/// The parameters of `curve` on `[start, end]` that bound polynomial pieces
/// of its composition with `surface`, in increasing order.
fn breaks(
    surface: &NurbsSurfaceRecord,
    curve: &Nurbs<2>,
    [start, end]: [f64; 2],
) -> Option<Vec<f64>> {
    let mut values = vec![start, end];
    values.extend(
        curve
            .knots
            .iter()
            .copied()
            .filter(|knot| start < *knot && *knot < end),
    );
    let mut spans: Vec<f64> = values.clone();
    spans.sort_by(f64::total_cmp);
    spans.dedup();
    let lines = [
        interior_knots(&surface.u_knots, surface.u_order, surface.u_count),
        interior_knots(&surface.v_knots, surface.v_order, surface.v_count),
    ];
    for pair in spans.windows(2) {
        let bezier = curve.segment(pair[0], pair[1])?;
        for (axis, knots) in lines.iter().enumerate() {
            for knot in knots {
                let coefficients: Vec<f64> = bezier
                    .points
                    .iter()
                    .zip(&bezier.weights)
                    .map(|(point, weight)| weight * (point[axis] - knot))
                    .collect();
                sign_changes(&coefficients, pair[0], pair[1], CROSSING_DEPTH, &mut values);
                if values.len() > MAX_PIECES {
                    return None;
                }
            }
        }
    }
    values.sort_by(f64::total_cmp);
    let merge = BREAK_MERGE * (end - start);
    let mut merged: Vec<f64> = Vec::with_capacity(values.len());
    for value in values {
        match merged.last() {
            Some(last) if value - last <= merge => {}
            _ => merged.push(value),
        }
    }
    if let Some(last) = merged.last_mut() {
        *last = end;
    }
    (merged.len() >= 2).then_some(merged)
}

/// The Bernstein basis of degree `degree` at `s`.
fn bernstein(degree: usize, s: f64) -> Vec<f64> {
    let mut values = vec![1.0];
    for _ in 0..degree {
        let mut next = Vec::with_capacity(values.len() + 1);
        next.push((1.0 - s) * values[0]);
        for pair in values.windows(2) {
            next.push(s * pair[0] + (1.0 - s) * pair[1]);
        }
        next.push(s * values[values.len() - 1]);
        values = next;
    }
    values
}

/// Solves `matrix · x = rhs` by elimination with partial pivoting.
fn solve(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<[f64; 4]>) -> Option<Vec<[f64; 4]>> {
    let size = matrix.len();
    for column in 0..size {
        let pivot = (column..size).max_by(|a, b| {
            matrix[*a][column]
                .abs()
                .total_cmp(&matrix[*b][column].abs())
        })?;
        if matrix[pivot][column] == 0.0 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let (pivot_rows, rows) = matrix.split_at_mut(column + 1);
        let pivot_row = &pivot_rows[column];
        let (pivot_rhs, rest) = rhs.split_at_mut(column + 1);
        let pivot_value = pivot_rhs[column];
        for (row, target) in rows.iter_mut().zip(rest.iter_mut()) {
            let factor = row[column] / pivot_row[column];
            for (entry, value) in row.iter_mut().zip(pivot_row).skip(column) {
                *entry -= factor * value;
            }
            for (entry, value) in target.iter_mut().zip(pivot_value) {
                *entry -= factor * value;
            }
        }
    }
    let mut out = rhs.clone();
    for row in (0..size).rev() {
        for axis in 0..4 {
            let mut value = rhs[row][axis];
            for entry in row + 1..size {
                value -= matrix[row][entry] * out[entry][axis];
            }
            out[row][axis] = value / matrix[row][row];
        }
    }
    Some(out)
}

/// The Euclidean point of a homogeneous Bézier piece at `s`.
fn bezier_point(coefficients: &[[f64; 4]], s: f64) -> Option<[f64; 3]> {
    let basis = bernstein(coefficients.len() - 1, s);
    let mut sum = [0.0; 4];
    for (coefficient, value) in coefficients.iter().zip(basis) {
        for axis in 0..4 {
            sum[axis] += coefficient[axis] * value;
        }
    }
    (sum[3] > 0.0).then(|| [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]])
}

/// The composition of `surface` and `curve`.
struct Composition<'a> {
    surface: &'a NurbsSurfaceRecord,
    curve: &'a Nurbs<2>,
    rational: bool,
}

impl Composition<'_> {
    /// The composition at `t` in homogeneous form, scaled by the pcurve
    /// weight to the power `power`.
    fn homogeneous(&self, t: f64, power: i32) -> Option<[f64; 4]> {
        let uv = self.curve.point_at(t)?;
        let mut point = surface_homogeneous(self.surface, uv)?;
        if !self.rational {
            let weight = point[3];
            return (weight != 0.0)
                .then(|| [point[0] / weight, point[1] / weight, point[2] / weight, 1.0]);
        }
        let factor = weight_at(self.curve, t)?.powi(power);
        for value in &mut point {
            *value *= factor;
        }
        Some(point)
    }

    fn point(&self, t: f64) -> Option<[f64; 3]> {
        surface_point(self.surface, self.curve.point_at(t)?)
    }

    /// The Bézier piece of degree `degree` on `[low, high]` of the
    /// composition scaled by the pcurve weight to the power `power`, when it
    /// reproduces the composition within `agreement`.
    fn fit(
        &self,
        [low, high]: [f64; 2],
        degree: usize,
        power: i32,
        agreement: f64,
    ) -> Option<Vec<[f64; 4]>> {
        let degree_value = f64_from_index(degree)?;
        let mut matrix = Vec::with_capacity(degree + 1);
        let mut rhs = Vec::with_capacity(degree + 1);
        for node in 0..=degree {
            let s = if degree == 0 {
                0.5
            } else {
                0.5 * (1.0
                    - (std::f64::consts::PI * f64_from_index(node)? / degree_value).portable_cos())
            };
            matrix.push(bernstein(degree, s));
            rhs.push(self.homogeneous(low + s * (high - low), power)?);
        }
        let coefficients = solve(matrix, rhs)?;
        for check in 0..CHECK_POINTS {
            let s = (f64_from_index(check)? + 0.5) / f64_from_index(CHECK_POINTS)?;
            let fitted = bezier_point(&coefficients, s)?;
            let exact = self.point(low + s * (high - low))?;
            let gap = (0..3)
                .map(|axis| (fitted[axis] - exact[axis]).powi(2))
                .sum::<f64>()
                .sqrt();
            if gap.partial_cmp(&agreement) == Some(std::cmp::Ordering::Greater) || gap.is_nan() {
                return None;
            }
        }
        coefficients
            .iter()
            .all(|row| row[3] > 0.0)
            .then_some(coefficients)
    }
}

/// The diagonal of the bounding box of the control points of `surface`.
fn extent(surface: &NurbsSurfaceRecord) -> f64 {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in &surface.points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    (0..3)
        .map(|axis| (high[axis] - low[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// The space curve of `curve` on `surface`, over the domain of `curve`.
pub(crate) fn lifted(
    surface: &NurbsSurfaceRecord,
    curve: &Nurbs<2>,
) -> Option<NurbsCurveRecord<3>> {
    let domain = curve.domain()?;
    let power = surface.u_order + surface.v_order - 2;
    let bound = (curve.degree * power).max(1);
    if bound > MAX_DEGREE {
        return None;
    }
    let rational = surface
        .weights
        .iter()
        .chain(&curve.weights)
        .any(|weight| *weight != 1.0);
    let composition = Composition {
        surface,
        curve,
        rational,
    };
    let agreement = FIT_AGREEMENT * extent(surface).max(1.0);
    let breaks = breaks(surface, curve, domain)?;
    let most = i32::try_from(power).ok()?;
    let powers: Vec<i32> = if curve.weights.iter().all(|weight| *weight == 1.0) {
        vec![most]
    } else {
        (0..=most).collect()
    };
    let fits = (breaks.len() - 1)
        .checked_mul(bound)?
        .checked_mul(powers.len())?;
    if fits > MAX_FITS {
        return None;
    }
    let (degree, pieces) = (1..=bound).find_map(|degree| {
        powers.iter().find_map(|power| {
            let pieces = breaks
                .windows(2)
                .map(|pair| composition.fit([pair[0], pair[1]], degree, *power, agreement))
                .collect::<Option<Vec<_>>>()?;
            Some((degree, pieces))
        })
    })?;
    let mut knots: Vec<f64> = std::iter::repeat_n(breaks[0], degree + 1).collect();
    let mut rows: Vec<[f64; 4]> = Vec::new();
    for (position, piece) in pieces.iter().enumerate() {
        let skip = usize::from(position > 0);
        rows.extend(piece.iter().skip(skip));
        let end = breaks[position + 1];
        let multiplicity = if position + 1 == pieces.len() {
            degree + 1
        } else {
            degree
        };
        knots.extend(std::iter::repeat_n(end, multiplicity));
    }
    let (points, weights) = rows
        .iter()
        .map(|row| {
            if composition.rational {
                ([row[0] / row[3], row[1] / row[3], row[2] / row[3]], row[3])
            } else {
                ([row[0], row[1], row[2]], 1.0)
            }
        })
        .unzip();
    let reduced = Nurbs {
        degree,
        knots,
        points,
        weights,
    }
    .without_knots(agreement);
    Some(NurbsCurveRecord {
        order: degree + 1,
        points: reduced.points,
        weights: reduced.weights,
        knots: reduced.knots,
        form: Form::Open,
    })
}

#[cfg(test)]
mod tests;
