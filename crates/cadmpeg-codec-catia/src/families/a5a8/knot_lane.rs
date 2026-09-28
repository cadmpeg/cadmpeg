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
        (distinct.len() == multiplicities.len()
            && distinct.windows(2).all(|pair| pair[0].get() < pair[1].get()))
        .then_some(Self {
            distinct,
            multiplicities,
        })
    }

    pub(super) fn expanded(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Vec<f64>>, cadmpeg_core::CodecError> {
        let count = self.multiplicities.iter().try_fold(0usize, |sum, &value| {
            sum.checked_add(usize::try_from(value).ok()?)
        });
        let Some(count) = count else { return Ok(None) };
        let mut expanded = Vec::new();
        crate::resource::reserve_vec(ctx, &mut expanded, count, "catia_a8_expanded_knots")?;
        for (knot, &multiplicity) in self.distinct.iter().zip(&self.multiplicities) {
            let Some(repeats) = usize::try_from(multiplicity).ok() else { return Ok(None) };
            expanded.extend(std::iter::repeat_n(knot.get(), repeats));
        }
        Ok(Some(expanded))
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
        assert_eq!(crate::test_support::with_service_context(|ctx| lane.expanded(ctx))
            .expect("service collection budget"), Some(vec![0.0, 0.0, 1.0, 1.0]));
        assert_eq!(lane.pole_count(1), Some(2));
    }

    #[test]
    fn knot_expansion_refuses_collection_limit_before_reservation() {
        let lane = A8KnotLane::try_new(finite_lane(&[0.0, 1.0]), vec![2, 2])
            .expect("valid knot lane");
        let limited = crate::test_support::with_collection_limit(3, |ctx| lane.expanded(ctx));
        assert!(matches!(limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_a8_expanded_knots"));
    }
}
