//! Lazy delimiters for serialized child-source lists.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(crate) fn child_tokens<'value, 'ctx, 'arena>(
    ctx: &'ctx DecodeContext<'arena>,
    value: &'value str,
    operation: &'static str,
) -> impl Iterator<Item = Result<&'value str, CodecError>> + use<'value, 'ctx, 'arena> {
    let mut start = 0usize;
    let mut characters = value
        .char_indices()
        .chain(std::iter::once((value.len(), ',')));
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let token = ctx.find_map(
            characters.by_ref(),
            |(offset, character)| {
                if character != ',' {
                    return Ok(None);
                }
                let token = value
                    .get(start..offset)
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
                start = offset
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
                Ok(Some(token))
            },
            operation,
        );
        if !matches!(token, Ok(Some(_))) {
            done = true;
        }
        token.transpose()
    })
}

#[cfg(test)]
mod tests;
