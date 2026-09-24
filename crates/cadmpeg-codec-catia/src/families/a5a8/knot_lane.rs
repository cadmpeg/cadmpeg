use crate::nurbs::{expand_knots, pole_count};
use cadmpeg_ir::geometry::nurbs::knots_strictly_increasing;
use cadmpeg_ir::scalar::FiniteReal;

/// Distinct finite increasing knots paired with their multiplicities.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct A8KnotLane {
    distinct: Vec<f64>,
    multiplicities: Vec<u32>,
}

impl A8KnotLane {
    pub(super) fn distinct(&self) -> &[f64] {
        &self.distinct
    }

    /// Multiplicity of each distinct knot.
    #[cfg(test)]
    pub(super) fn multiplicities(&self) -> &[u32] {
        &self.multiplicities
    }

    pub(super) fn try_new(distinct: Vec<FiniteReal>, multiplicities: Vec<u32>) -> Option<Self> {
        let distinct = distinct
            .into_iter()
            .map(FiniteReal::get)
            .collect::<Vec<_>>();
        (distinct.len() == multiplicities.len() && knots_strictly_increasing(&distinct)).then_some(
            Self {
                distinct,
                multiplicities,
            },
        )
    }

    pub(super) fn expanded(&self) -> Option<Vec<f64>> {
        expand_knots(&self.distinct, &self.multiplicities)
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
        assert!(A8KnotLane::try_new(finite_lane(&[0.0, 1.0]), vec![2]).is_none());
        assert!(A8KnotLane::try_new(finite_lane(&[0.0]), vec![2, 2]).is_none());
        assert!(A8KnotLane::try_new(finite_lane(&[1.0, 0.0]), vec![2, 2]).is_none());
        assert!(A8KnotLane::try_new(finite_lane(&[0.0, 0.0]), vec![2, 2]).is_none());
        let lane = A8KnotLane::try_new(finite_lane(&[0.0, 1.0]), vec![2, 2])
            .expect("aligned increasing knot lane");
        assert_eq!(lane.expanded(), Some(vec![0.0, 0.0, 1.0, 1.0]));
        assert_eq!(lane.pole_count(1), Some(2));
    }
}
