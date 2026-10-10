// SPDX-License-Identifier: Apache-2.0
//! Query one native face operand without building an index.

use crate::ids::native_stream_charged;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Reject the second matching record; retain the source's first record order.
pub(super) fn unique_face_operand<'ir>(
    ctx: &DecodeContext<'_>,
    operands: &'ir [DesignFaceOperand],
    stream: Option<&str>,
    scope_record_index: u32,
    record_index: u32,
) -> Result<Option<&'ir DesignFaceOperand>, CodecError> {

    if operands.is_empty() {
        return Ok(None);
    }
    let mut input = operands.iter();
    let mut matches =
        |operand: &'ir DesignFaceOperand| -> Result<Option<&'ir DesignFaceOperand>, CodecError> {
            let actual =
                native_stream_charged(ctx, &operand.id, "resolve F3D face operand stream")?;
            let same_stream = match (actual, stream) {
                (Some(actual), Some(expected)) => ctx.equal_bytes(
                    actual.as_bytes(),
                    expected.as_bytes(),
                    "compare F3D face operand stream",
                )?,
                (None, None) => true,
                _ => false,
            };
            Ok((same_stream
                && operand.scope_record_index == scope_record_index
                && operand.record_index() == record_index)
                .then_some(operand))
        };
    let Some(first) = ctx.find_map(input.by_ref(), &mut matches, "visit F3D face operand")? else {
        return Ok(None);
    };
    if !input.as_slice().is_empty()
        && ctx
            .find_map(input, matches, "visit F3D face operand")?
            .is_some()
    {
        return Ok(None);
    }
    Ok(Some(first))
}

#[cfg(test)]
mod tests;
