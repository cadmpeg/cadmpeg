// SPDX-License-Identifier: Apache-2.0
//! Ordered graph lookup admits every inspected byte before comparison.

use std::cmp::Ordering;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::comparison::compare;

pub(in crate::brep) fn position<T>(
    ctx: &DecodeContext<'_>,
    rows: &[T],
    query: &str,
    key: impl Fn(&T) -> &str,
) -> Result<Result<usize, usize>, CodecError> {
    ctx.charge_work(0, "compare F3D BREP graph ID")?;
    let mut low = 0;
    let mut high = rows.len();
    while low < high {
        ctx.charge_work(1, "compare F3D BREP graph ID")?;
        let middle = usize::midpoint(low, high);
        match compare(ctx, key(&rows[middle]), query, "compare F3D BREP graph ID")? {
            Ordering::Less => low = middle + 1,
            Ordering::Greater => high = middle,
            Ordering::Equal => return Ok(Ok(middle)),
        }
    }
    Ok(Err(low))
}

#[cfg(test)]
mod tests;
