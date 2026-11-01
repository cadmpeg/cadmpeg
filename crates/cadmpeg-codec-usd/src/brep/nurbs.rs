// SPDX-License-Identifier: Apache-2.0
//! NURBS curve operations the B-rep writer needs: reversal, extraction of a
//! parameter segment, and the exact rational form of a harmonic arc.
use cadmpeg_ir::math::portable::Portable;

/// Ternary-search steps that refine a nearest-point parameter.
const NEAREST_ITERATIONS: usize = 80;

/// A NURBS curve of `N` coordinates with one weight per control point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Nurbs<const N: usize> {
    pub(crate) degree: usize,
    pub(crate) knots: Vec<f64>,
    pub(crate) points: Vec<[f64; N]>,
    pub(crate) weights: Vec<f64>,
}

impl<const N: usize> Nurbs<N> {
    /// The parameter domain `[knots[p], knots[n]]`.
    pub(crate) fn domain(&self) -> Option<[f64; 2]> {
        Some([
            *self.knots.get(self.degree)?,
            *self.knots.get(self.points.len())?,
        ])
    }

    /// The same curve traversed the other way, on the reflected knot vector
    /// `a + b − k`.
    pub(crate) fn reversed(&self) -> Option<Self> {
        let [first, last] = [*self.knots.first()?, *self.knots.last()?];
        Some(Self {
            degree: self.degree,
            knots: self
                .knots
                .iter()
                .rev()
                .map(|knot| first + last - knot)
                .collect(),
            points: self.points.iter().rev().copied().collect(),
            weights: self.weights.iter().rev().copied().collect(),
        })
    }

    fn homogeneous(&self) -> Vec<Vec<f64>> {
        self.points
            .iter()
            .zip(&self.weights)
            .map(|(point, weight)| {
                let mut row: Vec<f64> = point.iter().map(|value| value * weight).collect();
                row.push(*weight);
                row
            })
            .collect()
    }

    fn from_homogeneous(degree: usize, knots: Vec<f64>, rows: &[Vec<f64>]) -> Option<Self> {
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for row in rows {
            let weight = *row.get(N)?;
            if weight == 0.0 || !weight.is_finite() {
                return None;
            }
            let mut point = [0.0; N];
            for (axis, value) in point.iter_mut().enumerate() {
                *value = row[axis] / weight;
            }
            points.push(point);
            weights.push(weight);
        }
        Some(Self {
            degree,
            knots,
            points,
            weights,
        })
    }

    /// Inserts knot `value` until its multiplicity is the degree.
    fn saturate(degree: usize, knots: &mut Vec<f64>, rows: &mut Vec<Vec<f64>>, value: f64) {
        let multiplicity = knots.iter().filter(|knot| **knot == value).count();
        for _ in multiplicity..degree {
            let Some(span) = knots
                .windows(2)
                .enumerate()
                .rev()
                .find(|(_, pair)| pair[0] <= value && value < pair[1])
                .map(|(index, _)| index)
            else {
                return;
            };
            let mut inserted = Vec::new();
            for index in 0..=rows.len() {
                let row = if index + degree <= span {
                    rows[index].clone()
                } else if index > span {
                    rows[index - 1].clone()
                } else {
                    let denominator = knots[index + degree] - knots[index];
                    let alpha = if denominator == 0.0 {
                        0.0
                    } else {
                        (value - knots[index]) / denominator
                    };
                    rows[index]
                        .iter()
                        .zip(&rows[index - 1])
                        .map(|(current, previous)| alpha * current + (1.0 - alpha) * previous)
                        .collect()
                };
                inserted.push(row);
            }
            *rows = inserted;
            knots.insert(span + 1, value);
        }
    }

    /// The part of the curve on `[start, end]`, `start < end`, clamped at
    /// both ends.
    pub(crate) fn segment(&self, start: f64, end: f64) -> Option<Self> {
        let [low, high] = self.domain()?;
        if !(low <= start && start < end && end <= high) {
            return None;
        }
        let degree = self.degree;
        let mut knots = self.knots.clone();
        let mut rows = self.homogeneous();
        Self::saturate(degree, &mut knots, &mut rows, start);
        Self::saturate(degree, &mut knots, &mut rows, end);
        // With a knot of multiplicity `degree` at indices `r..` the curve
        // passes through control point `r - 1` there.
        let last_start = knots.iter().rposition(|knot| *knot == start)?;
        let first_end = knots.iter().position(|knot| *knot == end)?;
        let point_start = last_start.checked_sub(degree)?;
        let point_end = first_end.checked_sub(1)?;
        if point_end < point_start {
            return None;
        }
        let mut segment_knots: Vec<f64> = std::iter::repeat_n(start, degree + 1).collect();
        segment_knots.extend_from_slice(knots.get(last_start + 1..first_end)?);
        segment_knots.extend(std::iter::repeat_n(end, degree + 1));
        let segment_rows = rows.get(point_start..=point_end)?;
        let curve = Self::from_homogeneous(degree, segment_knots, segment_rows)?;
        (curve.knots.len() == curve.points.len() + degree + 1).then_some(curve)
    }

    /// The point at parameter `t` of the domain, by de Boor's algorithm.
    pub(crate) fn point_at(&self, t: f64) -> Option<[f64; N]> {
        let [low, high] = self.domain()?;
        let t = t.clamp(low, high);
        let degree = self.degree;
        let last = self.points.len().checked_sub(1)?;
        let span = if t >= high {
            last
        } else {
            self.knots
                .partition_point(|knot| *knot <= t)
                .checked_sub(1)?
                .clamp(degree, last)
        };
        // Homogeneous rows of the `degree + 1` control points of the span,
        // one after another.
        let width = N + 1;
        let first = span.checked_sub(degree)?;
        let mut work = Vec::with_capacity((degree + 1) * width);
        for index in first..=span {
            let weight = *self.weights.get(index)?;
            work.extend(self.points.get(index)?.iter().map(|value| value * weight));
            work.push(weight);
        }
        for level in 1..=degree {
            for j in (level..=degree).rev() {
                let index = first + j;
                let denominator = self.knots[index + degree + 1 - level] - self.knots[index];
                let alpha = if denominator == 0.0 {
                    0.0
                } else {
                    (t - self.knots[index]) / denominator
                };
                for axis in 0..width {
                    let before = work[(j - 1) * width + axis];
                    let value = &mut work[j * width + axis];
                    *value = alpha * *value + (1.0 - alpha) * before;
                }
            }
        }
        let row = work.get(degree * width..)?;
        let weight = *row.get(N)?;
        if weight == 0.0 {
            return None;
        }
        let mut point = [0.0; N];
        for (axis, value) in point.iter_mut().enumerate() {
            *value = row[axis] / weight;
        }
        Some(point)
    }

    /// The parameter of the domain whose point is nearest `target`.
    pub(crate) fn nearest(&self, target: [f64; N]) -> Option<f64> {
        let [low, high] = self.domain()?;
        let distance = |t: f64| {
            self.point_at(t).map(|point| {
                point
                    .iter()
                    .zip(target)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f64>()
            })
        };
        let samples = 4 * (self.points.len() + 1) * (self.degree + 1);
        let step = (high - low) / cadmpeg_core::convert::f64_from_index(samples)?;
        let mut best = (low, distance(low)?);
        for sample in 1..=samples {
            let t = low + step * cadmpeg_core::convert::f64_from_index(sample)?;
            let value = distance(t)?;
            if value < best.1 {
                best = (t, value);
            }
        }
        let (mut left, mut right) = ((best.0 - step).max(low), (best.0 + step).min(high));
        for _ in 0..NEAREST_ITERATIONS {
            let first = left + (right - left) / 3.0;
            let second = right - (right - left) / 3.0;
            if distance(first)? <= distance(second)? {
                right = second;
            } else {
                left = first;
            }
        }
        Some(f64::midpoint(left, right))
    }

    /// Maps each control point; weights and knots are unchanged.
    pub(crate) fn map(&self, map: impl Fn([f64; N]) -> [f64; N]) -> Self {
        Self {
            degree: self.degree,
            knots: self.knots.clone(),
            points: self.points.iter().map(|point| map(*point)).collect(),
            weights: self.weights.clone(),
        }
    }
}

/// The rational quadratic form of the harmonic arc
/// `center + cos t · a + sin t · b` for `t` from `start` to `end`.
pub(crate) fn harmonic_arc<const N: usize>(
    center: [f64; N],
    a: [f64; N],
    b: [f64; N],
    start: f64,
    end: f64,
) -> Option<Nurbs<N>> {
    let sweep = end - start;
    if sweep == 0.0 || !sweep.is_finite() {
        return None;
    }
    let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
    let count = cadmpeg_core::convert::truncate_f64_to_usize(pieces)?;
    let step = sweep / pieces;
    let at = |angle: f64, scale: f64| {
        let mut point = center;
        for axis in 0..N {
            point[axis] +=
                scale * (angle.portable_cos() * a[axis] + angle.portable_sin() * b[axis]);
        }
        point
    };
    let half = step / 2.0;
    let middle_weight = half.portable_cos();
    let mut points = vec![at(start, 1.0)];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0, 0.0, 0.0];
    for piece in 0..count {
        let from = start + step * cadmpeg_core::convert::f64_from_index(piece)?;
        points.push(at(from + half, 1.0 / middle_weight));
        weights.push(middle_weight);
        points.push(at(from + step, 1.0));
        weights.push(1.0);
        let knot = cadmpeg_core::convert::f64_from_index(piece + 1)?;
        knots.push(knot);
        knots.push(knot);
    }
    if let Some(last) = knots.last().copied() {
        knots.push(last);
    }
    Some(Nurbs {
        degree: 2,
        knots,
        points,
        weights,
    })
}

#[cfg(test)]
mod tests;
