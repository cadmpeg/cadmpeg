// SPDX-License-Identifier: Apache-2.0
//! Text comparison under the caller's decode work budget.

use std::cmp::Ordering;

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

pub(crate) mod sealed {
    pub trait TextWork {}
    impl TextWork for cadmpeg_core::decode::DecodeContext<'_> {}
}

/// Work policy for byte-wise text comparisons.
pub trait TextWork: sealed::TextWork {
    /// The policy's original refusal, or an infallible standard operation.
    type Error;
    #[doc(hidden)]
    fn comparison_work(&self, count: u64, operation: &'static str) -> Result<(), Self::Error>;
}

impl TextWork for DecodeContext<'_> {
    type Error = ResourceLimit;
    fn comparison_work(&self, count: u64, operation: &'static str) -> Result<(), ResourceLimit> {
        self.charge_work_limit(count, operation)
    }
}

/// Compare text in byte order, admitting only the bytes actually compared.
pub fn compare<P: TextWork>(
    ctx: &P,
    first: &str,
    second: &str,
    operation: &'static str,
) -> Result<Ordering, P::Error> {
    ctx.comparison_work(1, operation)?;
    compare_bytes(ctx, first, second, operation)
}

fn compare_bytes<P: TextWork>(
    ctx: &P,
    first: &str,
    second: &str,
    operation: &'static str,
) -> Result<Ordering, P::Error> {
    for (first, second) in first.as_bytes().iter().zip(second.as_bytes()) {
        ctx.comparison_work(1, operation)?;
        let order = first.cmp(second);
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(first.len().cmp(&second.len()))
}

/// Test text equality, admitting the length gate before byte comparisons.
pub fn equal<P: TextWork>(
    ctx: &P,
    first: &str,
    second: &str,
    operation: &'static str,
) -> Result<bool, P::Error> {
    ctx.comparison_work(1, operation)?;
    if first.len() != second.len() {
        return Ok(false);
    }
    Ok(compare_bytes(ctx, first, second, operation)? == Ordering::Equal)
}

/// Sort values stably by identity text, leaving values already in order
/// unsorted.
///
/// Arenas are usually stored in identity order already, so the common case
/// costs core's charged neighbour pass, and an out-of-order input pays for that
/// pass and the charged sort.
pub(crate) fn stable_sort_by_identity<T>(
    ctx: &DecodeContext<'_>,
    values: &mut [T],
    identity: impl Fn(&T) -> &str,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if !ctx.is_sorted_by(values, &identity, Ord::cmp, operation)? {
        ctx.stable_sort_by(values, identity, Ord::cmp, operation)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
