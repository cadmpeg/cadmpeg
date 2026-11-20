// SPDX-License-Identifier: Apache-2.0
//! NURBS curve operations the B-rep writer needs: reversal, extraction of a
//! parameter segment, and the exact rational form of a harmonic arc.
use cadmpeg_ir::math::portable::Portable;

/// Ternary-search steps that refine a nearest-point parameter.
const NEAREST_ITERATIONS: usize = 80;
/// Samples of each knot span that compare a curve with its knots removed
/// against the original.
const REMOVAL_CHECKS: usize = 8;

/// The distance between two homogeneous rows.
fn row_distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// `a · x + b · y` of two homogeneous rows.
fn combined(a: f64, x: &[f64], b: f64, y: &[f64]) -> Vec<f64> {
    x.iter().zip(y).map(|(x, y)| a * x + b * y).collect()
}

/// Removes the knot at index `last` (its last occurrence, of multiplicity
/// `multiplicity`) up to `multiplicity` times from `knots` and `rows`, while
/// each removal changes the homogeneous control polygon by at most
/// `tolerance` (The NURBS Book, A5.8). Returns the number of removals.
fn remove_knot(
    degree: usize,
    knots: &mut Vec<f64>,
    rows: &mut Vec<Vec<f64>>,
    last: usize,
    multiplicity: usize,
    tolerance: f64,
) -> Option<usize> {
    let value = *knots.get(last)?;
    let order = degree + 1;
    let mut first = last.checked_sub(degree)?;
    let mut end = last.checked_sub(multiplicity)?;
    if first == 0 || end + 1 >= rows.len() {
        return Some(0);
    }
    let mut removed = 0;
    while removed < multiplicity {
        let offset = first - 1;
        let width = end + 2 - offset;
        let mut temp: Vec<Vec<f64>> = (0..width).map(|_| Vec::new()).collect();
        temp[0].clone_from(&rows[offset]);
        temp[width - 1].clone_from(&rows[end + 1]);
        let (mut i, mut j) = (first, end);
        let (mut ii, mut jj) = (1, end - offset);
        while j > i + removed {
            let alpha_i = (value - knots[i]) / (knots[i + order + removed] - knots[i]);
            let alpha_j = (value - knots[j - removed]) / (knots[j + order] - knots[j - removed]);
            temp[ii] = combined(
                1.0 / alpha_i,
                &rows[i],
                -(1.0 - alpha_i) / alpha_i,
                &temp[ii - 1],
            );
            temp[jj] = combined(
                1.0 / (1.0 - alpha_j),
                &rows[j],
                -alpha_j / (1.0 - alpha_j),
                &temp[jj + 1],
            );
            i += 1;
            ii += 1;
            j -= 1;
            jj -= 1;
        }
        let removable = if j < i + removed {
            row_distance(&temp[ii - 1], &temp[jj + 1]) <= tolerance
        } else {
            let alpha_i = (value - knots[i]) / (knots[i + order + removed] - knots[i]);
            let blend = combined(
                alpha_i,
                temp.get(ii + removed + 1)?,
                1.0 - alpha_i,
                &temp[ii - 1],
            );
            row_distance(&rows[i], &blend) <= tolerance
        };
        if !removable {
            break;
        }
        let (mut i, mut j) = (first, end);
        while j > i + removed {
            rows[i].clone_from(&temp[i - offset]);
            rows[j].clone_from(&temp[j - offset]);
            i += 1;
            j -= 1;
        }
        removed += 1;
        first = first.checked_sub(1)?;
        end += 1;
        if first == 0 || end + 1 >= rows.len() {
            break;
        }
    }
    if removed == 0 {
        return Some(0);
    }
    for index in last + 1..knots.len() {
        knots[index - removed] = knots[index];
    }
    knots.truncate(knots.len() - removed);
    let fout = (2 * last - multiplicity - degree) / 2;
    let (mut i, mut j) = (fout, fout);
    for step in 1..removed {
        if step % 2 == 1 {
            i += 1;
        } else {
            j -= 1;
        }
    }
    for index in i + 1..rows.len() {
        rows[j] = rows[index].clone();
        j += 1;
    }
    rows.truncate(j);
    Some(removed)
}

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

    /// The same curve with each interior knot removed as often as the curve
    /// allows within `tolerance`, or the curve itself when no knot can be
    /// removed or the reduced curve departs from it by more than
    /// `tolerance`.
    pub(crate) fn without_knots(&self, tolerance: f64) -> Self {
        self.reduced(tolerance).unwrap_or_else(|| self.clone())
    }

    fn reduced(&self, tolerance: f64) -> Option<Self> {
        let [low, high] = self.domain()?;
        let mut knots = self.knots.clone();
        let mut rows = self.homogeneous();
        let lightest = self.weights.iter().copied().fold(f64::INFINITY, f64::min);
        let largest = self
            .points
            .iter()
            .map(|point| point.iter().map(|value| value * value).sum::<f64>().sqrt())
            .fold(0.0, f64::max);
        let row_tolerance = tolerance * lightest / (1.0 + largest);
        let mut interior: Vec<f64> = knots
            .iter()
            .copied()
            .filter(|knot| low < *knot && *knot < high)
            .collect();
        interior.dedup();
        let mut removed = 0;
        for value in interior {
            let last = knots.iter().rposition(|knot| *knot == value)?;
            let multiplicity = knots.iter().filter(|knot| **knot == value).count();
            removed += remove_knot(
                self.degree,
                &mut knots,
                &mut rows,
                last,
                multiplicity,
                row_tolerance,
            )?;
        }
        if removed == 0 {
            return None;
        }
        let reduced = Self::from_homogeneous(self.degree, knots, &rows)?;
        if reduced.knots.len() != reduced.points.len() + self.degree + 1 {
            return None;
        }
        let spans = self.points.len().checked_sub(self.degree)?.max(1);
        let count = spans.checked_mul(REMOVAL_CHECKS)?;
        let count_value = cadmpeg_core::convert::f64_from_index(count)?;
        for index in 0..=count {
            let t =
                low + (high - low) * cadmpeg_core::convert::f64_from_index(index)? / count_value;
            let (a, b) = (self.point_at(t)?, reduced.point_at(t)?);
            let gap = a
                .iter()
                .zip(b)
                .map(|(x, y)| (x - y).powi(2))
                .sum::<f64>()
                .sqrt();
            if !matches!(
                gap.partial_cmp(&tolerance),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ) {
                return None;
            }
        }
        Some(reduced)
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
