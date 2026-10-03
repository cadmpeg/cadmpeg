// SPDX-License-Identifier: Apache-2.0
//! Text comparison under the caller's decode work budget.

use std::cmp::Ordering;

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

/// Compare text in byte order, admitting only the bytes actually compared.
pub(crate) fn compare(
    ctx: &DecodeContext<'_>, first: &str, second: &str, operation: &'static str,
) -> Result<Ordering, ResourceLimit> {
    ctx.charge_work_limit(1, operation)?;
    compare_bytes(ctx, first, second, operation)
}

fn compare_bytes(
    ctx: &DecodeContext<'_>, first: &str, second: &str, operation: &'static str,
) -> Result<Ordering, ResourceLimit> {
    for (first, second) in first.as_bytes().iter().zip(second.as_bytes()) {
        ctx.charge_work_limit(1, operation)?;
        let order = first.cmp(second);
        if order != Ordering::Equal { return Ok(order); }
    }
    Ok(first.len().cmp(&second.len()))
}

/// Test text equality, admitting the length gate before byte comparisons.
pub(crate) fn equal(
    ctx: &DecodeContext<'_>, first: &str, second: &str, operation: &'static str,
) -> Result<bool, ResourceLimit> {
    ctx.charge_work_limit(1, operation)?;
    if first.len() != second.len() {
        return Ok(false);
    }
    Ok(compare_bytes(ctx, first, second, operation)? == Ordering::Equal)
}

#[cfg(test)]
mod tests;
