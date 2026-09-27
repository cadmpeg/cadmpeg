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
    output.try_reserve_exact(additional).map_err(|_| allocation_failed(additional, operation))
}

/// Describe a refused evaluator reserve without a decode session.
pub(crate) fn allocation_failed(additional: usize, operation: &'static str) -> ResourceLimit {
    let requested = u64::try_from(additional).unwrap_or(u64::MAX);
    ResourceLimit {
        dimension: ResourceDimension::Codec(operation),
        reason: ResourceFailure::AllocationFailed,
        limit: requested,
        used: requested,
        additional: 0,
        operation,
    }
}
