use crate::nurbs::pole_count;
use cadmpeg_ir::scalar::FiniteReal;

/// Distinct finite increasing knots paired with their multiplicities.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct A8KnotLane {
    distinct: Vec<FiniteReal>,
    multiplicities: Vec<u32>,
}

impl A8KnotLane {
    /// Multiplicity of each distinct knot.
    #[cfg(test)]
    pub(super) fn multiplicities(&self) -> &[u32] {
        &self.multiplicities
    }

    pub(super) fn try_new(distinct: Vec<FiniteReal>, multiplicities: Vec<u32>) -> Option<Self> {
        (!distinct.is_empty()
            && distinct.len() == multiplicities.len()
            && multiplicities.iter().all(|&value| value > 0)
            && distinct
                .windows(2)
                .all(|pair| pair[0].get() < pair[1].get()))
        .then_some(Self {
            distinct,
            multiplicities,
        })
    }

    /// Expanded knot vector of a lane whose pole count exceeds `degree`.
    ///
    /// The multiplicities sum to the pole count plus `degree + 1`, so a lane
    /// with more poles than its degree expands to fewer than twice its poles.
    /// Another lane states no carrier and expands to `None`.
    pub(super) fn expanded(&self, degree: u32) -> Option<Vec<f64>> {
        if self.pole_count(degree)? <= degree {
            return None;
        }
        let mut expanded = Vec::new();
        for (knot, &multiplicity) in self.distinct.iter().zip(&self.multiplicities) {
            let repeats = cadmpeg_core::decode::index_from_u32(multiplicity);
            expanded.extend(std::iter::repeat_with(|| knot.get()).take(repeats));
        }
        Some(expanded)
    }

    pub(super) fn pole_count(&self, degree: u32) -> Option<u32> {
        pole_count(&self.multiplicities, degree)
    }
}

#[cfg(test)]
mod tests {
    use super::A8KnotLane;
    use crate::test_support::test_b5::finite_lane;

    #[test]
    fn knot_lane_admission_requires_aligned_increasing_values() {
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(vec![], vec![]))
                .expect("service admission")
                .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                finite_lane(&[0.0, 1.0]),
                vec![0, 2]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                finite_lane(&[0.0, 1.0]),
                vec![2]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                finite_lane(&[0.0]),
                vec![2, 2]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                finite_lane(&[1.0, 0.0]),
                vec![2, 2]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                finite_lane(&[0.0, 0.0]),
                vec![2, 2]
            ))
            .expect("service admission")
            .is_none()
        );
        let lane = crate::test_support::with_service_context(|ctx| {
            A8KnotLane::try_new(finite_lane(&[0.0, 1.0]), vec![2, 2])
        })
        .expect("service admission")
        .expect("aligned increasing knot lane");
        assert_eq!(
            crate::test_support::with_service_context(|ctx| lane.expanded(ctx))
                .expect("service collection budget"),
            vec![0.0, 0.0, 1.0, 1.0]
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| lane.pole_count(1))
                .expect("service count work"),
            Some(2)
        );
    }
}
