// SPDX-License-Identifier: Apache-2.0
//! Ordered graph lookup admits every inspected byte before comparison.

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
    ctx.binary_search_by(
        rows,
        |row| Ok(compare(ctx, key(row), query, "compare F3D BREP graph ID")?),
        "compare F3D BREP graph ID",
    )
}

#[cfg(test)]
mod tests;
