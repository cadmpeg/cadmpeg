// SPDX-License-Identifier: Apache-2.0
//! Candidate boundaries for repeated inverse queries.

use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;

use super::MAX_CACHED_CURVE_POINTS;

struct BoundaryPoint {
    parameter: FiniteReal,
    position: FinitePoint3,
    order: usize,
}

pub(super) struct BoundaryIndex {
    points: Vec<BoundaryPoint>,
    axis: usize,
}

fn coordinates(point: FinitePoint3) -> [f64; 3] {
    point.get().into()
}

impl BoundaryIndex {
    pub(super) fn new(
        boundaries: &[FiniteReal],
        position: impl Fn(FiniteReal) -> Option<FinitePoint3>,
    ) -> Option<Self> {
        if boundaries.len() > MAX_CACHED_CURVE_POINTS {
            return None;
        }
        let mut points = Vec::new();
        points.try_reserve(boundaries.len()).ok()?;
        let mut previous = None;
        let mut minimum = [f64::INFINITY; 3];
        let mut maximum = [f64::NEG_INFINITY; 3];
        for (order, parameter) in boundaries.iter().copied().enumerate() {
            if previous == Some(parameter) {
                continue;
            }
            previous = Some(parameter);
            let position = position(parameter)?;
            for (axis, value) in coordinates(position).into_iter().enumerate() {
                minimum[axis] = minimum[axis].min(value);
                maximum[axis] = maximum[axis].max(value);
            }
            points.push(BoundaryPoint {
                parameter,
                position,
                order,
            });
        }
        let axis = (0..3).max_by(|left, right| {
            (maximum[*left] - minimum[*left]).total_cmp(&(maximum[*right] - minimum[*right]))
        })?;
        points.sort_unstable_by(|left, right| {
            coordinates(left.position)[axis]
                .total_cmp(&coordinates(right.position)[axis])
                .then_with(|| left.order.cmp(&right.order))
        });
        Some(Self { points, axis })
    }

    pub(super) fn witness(
        &self,
        point: Point3,
        seed: FiniteReal,
        tolerance: f64,
    ) -> Option<FiniteReal> {
        let point_axis = <[f64; 3]>::from(point)[self.axis];
        let radius = tolerance.next_up();
        let lower = (point_axis - radius).next_down();
        let upper = (point_axis + radius).next_up();
        let start = self
            .points
            .partition_point(|entry| coordinates(entry.position)[self.axis] < lower);
        let end = self
            .points
            .partition_point(|entry| coordinates(entry.position)[self.axis] <= upper);
        let mut nearest = None;
        let mut nearest_distance = f64::INFINITY;
        let mut nearest_order = usize::MAX;
        for entry in &self.points[start..end] {
            let distance = (entry.parameter.get() - seed.get()).abs();
            if distance.is_finite()
                && (distance < nearest_distance
                    || (distance == nearest_distance && entry.order < nearest_order))
                && entry.position.get().distance(point) <= tolerance
            {
                nearest = Some(entry.parameter);
                nearest_distance = distance;
                nearest_order = entry.order;
            }
        }
        nearest
    }
}

#[cfg(test)]
mod tests;
