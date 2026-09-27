// SPDX-License-Identifier: Apache-2.0
//! Fallible formatting for text retained by the STEP decoder.

use std::fmt::{self, Write};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

pub(crate) fn charged_format(
    ctx: &DecodeContext<'_>,
    operation: &'static str,
    arguments: fmt::Arguments<'_>,
) -> Result<String, CodecError> {
    let mut output = ChargedText {
        ctx,
        operation,
        text: String::new(),
        refusal: None,
    };
    if output.write_fmt(arguments).is_err() {
        return Err(match output.refusal {
            Some(error) => error,
            None => ctx.refuse_codec_limit(operation, 0, 1),
        });
    }
    Ok(output.text)
}

struct ChargedText<'a, 'arena> {
    ctx: &'a DecodeContext<'arena>,
    operation: &'static str,
    text: String,
    refusal: Option<CodecError>,
}

impl Write for ChargedText<'_, '_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if let Err(error) = self
            .ctx
            .charge_retained(u64_from_index(text.len()), self.operation)
        {
            self.refusal = Some(error);
            return Err(fmt::Error);
        }
        if self.text.try_reserve(text.len()).is_err() {
            self.refusal = Some(self.ctx.refuse_codec_limit(
                self.operation,
                0,
                u64_from_index(text.len()),
            ));
            return Err(fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    use super::charged_format;

    #[test]
    fn charged_format_refuses_retained_text_before_growth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits policy");
        let error = charged_format(&ctx, "step_test_format", format_args!("prefix {suffix}", suffix = "input"))
            .expect_err("formatted text exceeds three bytes");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "step_test_format"
        ));
    }
}
