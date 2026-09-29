// SPDX-License-Identifier: Apache-2.0
//! Format retained Design text under the caller budget.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::fmt;

pub(super) fn format_design_text(
    ctx: Option<&DecodeContext<'_>>,
    arguments: fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let Some(ctx) = ctx else { return Ok(arguments.to_string()); };
    struct Length(usize);
    impl fmt::Write for Length {
        fn write_str(&mut self, part: &str) -> fmt::Result {
            self.0 = self.0.checked_add(part.len()).ok_or(fmt::Error)?;
            Ok(())
        }
    }
    let mut length = Length(0);
    fmt::write(&mut length, arguments)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_retained(u64::try_from(length.0)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?, operation)?;
    let mut text = String::new();
    text.try_reserve_exact(length.0)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    fmt::write(&mut text, arguments)
        .map_err(|_| CodecError::malformed("Design text formatting failed"))?;
    Ok(text)
}

/// Preserve a malformed diagnostic or return its resource refusal.
pub(super) fn malformed_design(
    ctx: Option<&DecodeContext<'_>>,
    arguments: fmt::Arguments<'_>,
) -> CodecError {
    match format_design_text(ctx, arguments, "f3d Design diagnostic") {
        Ok(text) => CodecError::Malformed(text),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests;
