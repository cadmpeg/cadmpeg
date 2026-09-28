// SPDX-License-Identifier: Apache-2.0
//! Fallible scratch storage for evaluators over admitted NURBS geometry.

use cadmpeg_core::decode::{ResourceDimension, ResourceFailure, ResourceLimit};

/// Allocate a scratch vector bounded by the admitted control or knot count.
pub(crate) fn filled<T: Clone>(
    count: usize,
    value: T,
    operation: &'static str,
) -> Result<Vec<T>, ResourceLimit> {
    let mut output = Vec::new();
    reserve_exact(&mut output, count, operation)?;
    output.resize(count, value);
    Ok(output)
}

/// Reserve scratch capacity before copying admitted geometry into a vector.
pub(crate) fn reserve_exact<T>(
    output: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), ResourceLimit> {
    output
        .try_reserve_exact(additional)
        .map_err(|_| allocation_failed(additional, operation))
}

/// Describe a refused evaluator reserve without a decode session.
pub(crate) fn allocation_failed(additional: usize, operation: &'static str) -> ResourceLimit {
    let requested = cadmpeg_core::decode::u64_from_index(additional);
    ResourceLimit {
        dimension: ResourceDimension::Codec(operation),
        reason: ResourceFailure::AllocationFailed,
        limit: requested,
        used: requested,
        additional: 0,
        operation,
    }
}

#[cfg(test)]
mod tests {
    use super::{filled, reserve_exact};
    use cadmpeg_core::decode::{ResourceDimension, ResourceFailure};

    #[test]
    fn evaluator_scratch_refuses_unrepresentable_control_count() {
        let error = filled(usize::MAX, 0_u8, "IR test control scratch")
            .expect_err("unrepresentable control count must be refused");
        assert_eq!(
            error.dimension,
            ResourceDimension::Codec("IR test control scratch")
        );
        assert_eq!(error.reason, ResourceFailure::AllocationFailed);
        assert_eq!(error.operation, "IR test control scratch");
    }

    #[test]
    fn evaluator_reserve_refuses_unrepresentable_knot_count_without_mutation() {
        let mut knots = vec![0_u8, 1_u8];
        let error = reserve_exact(&mut knots, usize::MAX, "IR test knot scratch")
            .expect_err("unrepresentable knot count must be refused");
        assert_eq!(
            error.dimension,
            ResourceDimension::Codec("IR test knot scratch")
        );
        assert_eq!(error.reason, ResourceFailure::AllocationFailed);
        assert_eq!(knots, [0, 1]);
    }
}
