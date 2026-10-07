use crate::nurbs::pole_count;
use cadmpeg_ir::scalar::FiniteReal;

/// Distinct finite increasing knots paired with their multiplicities.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct A8KnotLane {
    distinct: Vec<FiniteReal>,
    multiplicities: Vec<u32>,
}

impl A8KnotLane {
    pub(super) fn copy_charged(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            distinct: ctx.copy_slice(&self.distinct, "catia_a8_copied_distinct_knots")?,
            multiplicities: ctx
                .copy_slice(&self.multiplicities, "catia_a8_copied_multiplicities")?,
        })
    }

    /// Multiplicity of each distinct knot.
    #[cfg(test)]
    pub(super) fn multiplicities(&self) -> &[u32] {
        &self.multiplicities
    }

    pub(super) fn try_new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        distinct: Vec<FiniteReal>,
        multiplicities: Vec<u32>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if distinct.is_empty() || distinct.len() != multiplicities.len() {
            return Ok(None);
        }
        let admitted = ctx.all_by(
            &multiplicities,
            |&value| Ok(value > 0),
            "catia_a8_multiplicity_admission",
        )? && ctx.all_by(
            distinct.windows(2),
            |pair| Ok(pair[0].get() < pair[1].get()),
            "catia_a8_knot_order_admission",
        )?;
        Ok(admitted.then_some(Self {
            distinct,
            multiplicities,
        }))
    }

    pub(super) fn expanded(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<f64>, cadmpeg_core::CodecError> {
        let count = ctx.fold(
            &self.multiplicities,
            0usize,
            |sum, &value| {
                usize::try_from(value)
                    .ok()
                    .and_then(|repeats| sum.checked_add(repeats))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("catia_a8_expanded_knots", u64::MAX - 1, u64::MAX)
                    })
            },
            "catia_a8_knot_expansion_scan",
        )?;
        let mut expanded = Vec::new();
        ctx.reserve_capacity(&mut expanded, count, "catia_a8_expanded_knots")?;
        for (knot, &multiplicity) in ctx
            .admit_iter(&self.distinct, "catia_a8_distinct_knot_visits")?
            .zip(&self.multiplicities)
        {
            let length = usize::try_from(multiplicity)
                .ok()
                .and_then(|repeats| expanded.len().checked_add(repeats))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_a8_expanded_knots", u64::MAX - 1, u64::MAX)
                })?;
            ctx.resize_vec(&mut expanded, length, knot.get(), "catia_a8_expanded_knots")?;
        }
        Ok(expanded)
    }

    pub(super) fn pole_count(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        degree: u32,
    ) -> Result<Option<u32>, cadmpeg_core::CodecError> {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(self.multiplicities.len()),
            "catia_a8_pole_count_scan",
        )?;
        Ok(pole_count(&self.multiplicities, degree))
    }
}

#[cfg(test)]
mod tests {
    use super::A8KnotLane;
    use crate::test_support::test_b5::finite_lane;

    #[test]
    fn knot_lane_admission_requires_aligned_increasing_values() {
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                ctx,
                vec![],
                vec![]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                ctx,
                finite_lane(&[0.0, 1.0]),
                vec![0, 2]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                ctx,
                finite_lane(&[0.0, 1.0]),
                vec![2]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                ctx,
                finite_lane(&[0.0]),
                vec![2, 2]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                ctx,
                finite_lane(&[1.0, 0.0]),
                vec![2, 2]
            ))
            .expect("service admission")
            .is_none()
        );
        assert!(
            crate::test_support::with_service_context(|ctx| A8KnotLane::try_new(
                ctx,
                finite_lane(&[0.0, 0.0]),
                vec![2, 2]
            ))
            .expect("service admission")
            .is_none()
        );
        let lane = crate::test_support::with_service_context(|ctx| {
            A8KnotLane::try_new(ctx, finite_lane(&[0.0, 1.0]), vec![2, 2])
        })
        .expect("service admission")
        .expect("aligned increasing knot lane");
        assert_eq!(
            crate::test_support::with_service_context(|ctx| lane.expanded(ctx))
                .expect("service collection budget"),
            vec![0.0, 0.0, 1.0, 1.0]
        );
        assert_eq!(
            crate::test_support::with_service_context(|ctx| lane.pole_count(ctx, 1))
                .expect("service count work"),
            Some(2)
        );
    }

    #[test]
    fn knot_expansion_refuses_the_callers_collection_limit() {
        let lane = crate::test_support::with_service_context(|ctx| {
            A8KnotLane::try_new(ctx, finite_lane(&[0.0, 1.0]), vec![2, 2])
        })
        .expect("service admission")
        .expect("valid knot lane");
        let limited = crate::test_support::with_collection_limit(3, |ctx| lane.expanded(ctx));
        assert!(matches!(limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == "catia_a8_expanded_knots"));
    }

    #[test]
    fn knot_expansion_scan_refuses_the_callers_work_limit() {
        let lane = crate::test_support::with_service_context(|ctx| {
            A8KnotLane::try_new(ctx, finite_lane(&[0.0, 1.0]), vec![2, 2])
        })
        .expect("service admission")
        .expect("valid knot lane");
        let result = crate::test_support::with_work_limit(1, |ctx| lane.expanded(ctx));
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "catia_a8_knot_expansion_scan"
        ));
    }

    #[test]
    fn knot_expansion_charges_each_visit_and_written_knot_once() {
        let lane = crate::test_support::with_service_context(|ctx| {
            A8KnotLane::try_new(ctx, finite_lane(&[0.0, 1.0]), vec![2, 2])
        })
        .expect("service admission")
        .expect("valid knot lane");
        for operation in ["catia_a8_distinct_knot_visits", "catia_a8_expanded_knots"] {
            let refused =
                crate::test_support::with_work_refusal(operation, |ctx| lane.expanded(ctx));
            assert!(matches!(refused,
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.operation == operation));
        }
        assert_eq!(
            // Two multiplicity visits, two distinct knot visits and four knot writes.
            crate::test_support::with_work_limit(8, |ctx| lane.expanded(ctx))
                .expect("scan and emission fit the work limit"),
            vec![0.0, 0.0, 1.0, 1.0]
        );
        assert!(crate::test_support::with_work_limit(7, |ctx| lane.expanded(ctx)).is_err());
    }

    #[test]
    fn copied_knot_lane_refuses_each_caller_collection_limit() {
        let lane = crate::test_support::with_service_context(|ctx| {
            A8KnotLane::try_new(ctx, finite_lane(&[0.0, 1.0]), vec![2, 2])
        })
        .expect("service admission")
        .expect("valid knot lane");
        for (limit, operation) in [
            (1, "catia_a8_copied_distinct_knots"),
            (2, "catia_a8_copied_multiplicities"),
        ] {
            let limited =
                crate::test_support::with_collection_limit(limit, |ctx| lane.copy_charged(ctx));
            assert!(
                matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
                if error.operation == operation)
            );
        }
        assert_eq!(
            crate::test_support::with_service_context(|ctx| lane.copy_charged(ctx))
                .expect("service budget"),
            lane
        );
    }

    #[test]
    fn knot_lane_admission_stops_at_the_first_invalid_value() {
        let rejected = crate::test_support::with_work_limit(1, |ctx| {
            A8KnotLane::try_new(ctx, finite_lane(&[0.0, 1.0, 2.0]), vec![0, 1, 1])
        });
        assert!(matches!(rejected, Ok(None)));
    }

    #[test]
    fn knot_lane_admission_refuses_the_callers_work_limit() {
        let result = crate::test_support::with_work_limit(0, |ctx| {
            A8KnotLane::try_new(ctx, finite_lane(&[0.0, 1.0]), vec![1, 1])
        });
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
    }
}
