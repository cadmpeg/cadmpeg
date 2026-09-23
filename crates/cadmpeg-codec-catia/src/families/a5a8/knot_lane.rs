use crate::nurbs::{expand_knots, pole_count};
use cadmpeg_ir::geometry::nurbs::knots_strictly_increasing;

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

    pub(super) fn try_new(distinct: Vec<f64>, multiplicities: Vec<u32>) -> Option<Self> {
        (distinct.len() == multiplicities.len()
            && distinct.iter().all(|value| value.is_finite())
            && knots_strictly_increasing(&distinct))
        .then_some(Self {
            distinct,
            multiplicities,
        })
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

    #[test]
    fn knot_lane_admission_requires_aligned_increasing_values() {
        assert!(A8KnotLane::try_new(vec![0.0, 1.0], vec![2]).is_none());
        assert!(A8KnotLane::try_new(vec![0.0], vec![2, 2]).is_none());
        assert!(A8KnotLane::try_new(vec![1.0, 0.0], vec![2, 2]).is_none());
        assert!(A8KnotLane::try_new(vec![0.0, 0.0], vec![2, 2]).is_none());
        assert!(A8KnotLane::try_new(vec![0.0, f64::INFINITY], vec![2, 2]).is_none());
        let lane =
            A8KnotLane::try_new(vec![0.0, 1.0], vec![2, 2]).expect("aligned increasing knot lane");
        assert_eq!(lane.expanded(), Some(vec![0.0, 0.0, 1.0, 1.0]));
        assert_eq!(lane.pole_count(1), Some(2));
    }
}
