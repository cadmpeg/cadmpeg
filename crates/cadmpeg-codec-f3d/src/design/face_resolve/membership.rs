// SPDX-License-Identifier: Apache-2.0
//! Face identity and stable slot membership through the caller's session.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::FaceId;

pub(super) fn contains_face(
    ctx: &DecodeContext<'_>,
    values: &[FaceId],
    target: &FaceId,
    operation: &'static str,
) -> Result<bool, CodecError> {
    ctx.charge_work(0, operation)?;
    if values.is_empty() {
        return Ok(false);
    }
    ctx.any_by(
        values,
        |candidate| ctx.equal_bytes(candidate.as_str().as_bytes(), target.as_str().as_bytes(), operation),
        operation,
    )
}

pub(super) fn contains_slot(
    ctx: &DecodeContext<'_>,
    values: &[i64],
    target: i64,
    operation: &'static str,
) -> Result<bool, CodecError> {
    ctx.charge_work(0, operation)?;
    if values.is_empty() {
        return Ok(false);
    }
    ctx.any_by(values, |candidate| Ok(*candidate == target), operation)
}

#[cfg(test)]
mod tests;
