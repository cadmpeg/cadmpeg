// SPDX-License-Identifier: Apache-2.0
//! Admit formatted native text before each retained fragment copy.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
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
        let admitted = self.ctx.charge_work(u64_from_index(fragment.len()), self.operation)
            .and_then(|()| self.ctx.reserve_retained_string(&mut self.text, fragment.len(), self.operation));
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
