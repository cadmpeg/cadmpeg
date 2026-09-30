// SPDX-License-Identifier: Apache-2.0
//! Admit native text storage and byte work before construction.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::fmt;

struct RetainedText<'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    operation: &'static str,
    text: String,
    refusal: Option<CodecError>,
}

impl fmt::Write for RetainedText<'_, '_> {
    fn write_str(&mut self, fragment: &str) -> fmt::Result {
        let admitted = reserve_retained_string(self.ctx, &mut self.text, fragment.len(), self.operation);
        if let Err(error) = admitted {
            self.refusal = Some(error);
            return Err(fmt::Error);
        }
        self.text.push_str(fragment);
        Ok(())
    }
}

pub(crate) fn format_retained(
    ctx: &DecodeContext<'_>,
    message: fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut output = RetainedText { ctx, operation, text: String::new(), refusal: None };
    if fmt::write(&mut output, message).is_err() {
        return Err(output.refusal.unwrap_or_else(|| CodecError::malformed("cannot format retained text")));
    }
    Ok(output.text)
}

pub(crate) fn reserve_retained_string(
    ctx: &DecodeContext<'_>,
    text: &mut String,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(u64_from_index(additional), operation)?;
    ctx.reserve_retained_string(text, additional, operation)
}

pub(crate) fn reserve_scoped_string<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: usize,
    operation: &'static str,
) -> Result<(String, ScopedReservation<'ctx>), CodecError> {
    ctx.charge_work(u64_from_index(bytes), operation)?;
    ctx.reserve_scoped_string(bytes, operation)
}
