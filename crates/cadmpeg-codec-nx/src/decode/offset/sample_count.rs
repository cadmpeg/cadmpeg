// SPDX-License-Identifier: Apache-2.0
//! The sample density the coarse parameter search takes along one surface
//! direction.

/// The number of samples the coarse parameter search takes along one surface
/// direction.
///
/// `coarse_model_surface_parameters` walks the direction's domain in
/// `get() - 1` equal intervals. One sample states no interval and two state
/// only the two domain ends, so the floor is three: the two ends and the
/// midpoint, two intervals.
///
/// The ceiling is nine, the count every route with no control-point count to
/// read already takes -- the recursion-depth limit, a surface the index does
/// not hold, a procedural construction the index does not hold, and every
/// solved geometry that is not a NURBS surface. A direction with more control
/// points than that is sampled at the density of a direction whose control
/// points are unknown.
///
/// The field is private to this module, which holds nothing but the type, its
/// two constructors and its two readers. `CoarseSampleCount(0)` is spellable
/// nowhere else, so `intervals()` has no zero to state and no underflow to
/// refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CoarseSampleCount(usize);

impl CoarseSampleCount {
    /// Two intervals: both domain ends and the midpoint.
    const FLOOR: usize = 3;

    /// The count taken when the control-point count is unknown.
    const CEILING: usize = 9;

    /// The count for a direction carrying `control_points` control points: one
    /// sample per control point and one more, held inside the floor and the
    /// ceiling.
    pub(super) fn for_control_points(control_points: usize) -> Self {
        match control_points {
            0 | 1 => Self(Self::FLOOR),
            2..=8 => Self(control_points + 1),
            _ => Self(Self::CEILING),
        }
    }

    /// The count taken where no control-point count is there to read.
    pub(super) const fn unknown() -> Self {
        Self(Self::CEILING)
    }

    /// The number of samples, at least [`Self::FLOOR`].
    pub(super) fn get(self) -> usize {
        self.0
    }

    /// The number of equal intervals the samples divide the domain into, at
    /// least two.
    pub(super) fn intervals(self) -> usize {
        self.0 - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_coarse_sample_count_holds_the_floor_the_range_and_the_ceiling() {
        // Below the floor: no control point and one control point both state
        // the floor, because `control_points + 1` is one or two and neither
        // states two intervals.
        assert_eq!(CoarseSampleCount::for_control_points(0).get(), 3);
        assert_eq!(CoarseSampleCount::for_control_points(1).get(), 3);

        // In range: one sample per control point and one more.
        for control_points in 2..=8usize {
            assert_eq!(
                CoarseSampleCount::for_control_points(control_points).get(),
                control_points + 1
            );
        }

        // Above the ceiling.
        assert_eq!(CoarseSampleCount::for_control_points(9).get(), 9);
        assert_eq!(CoarseSampleCount::for_control_points(usize::MAX).get(), 9);

        // Every count divides its domain into at least two intervals, so the
        // consumer's division has no zero divisor to state.
        for control_points in [0, 1, 2, 5, 8, 9, usize::MAX] {
            assert!(CoarseSampleCount::for_control_points(control_points).intervals() >= 2);
        }
        assert_eq!(CoarseSampleCount::unknown().get(), 9);
    }
}
