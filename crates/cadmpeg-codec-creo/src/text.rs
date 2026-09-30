// SPDX-License-Identifier: Apache-2.0
//! Charged text copying shared by Creo container and feature readers.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(crate) fn copy_lossy_text(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut text = String::new();
    let mut remaining = bytes;
    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                ctx.try_reserve_retained_text(&mut text, valid.len(), operation)?;
                text.push_str(valid);
                return Ok(text);
            }
            Err(error) => {
                let valid_len = error.valid_up_to();
                let valid = std::str::from_utf8(&remaining[..valid_len])
                    .map_err(|_| CodecError::malformed("creo feature entity UTF-8 prefix"))?;
                let growth = valid_len
                    .checked_add('\u{fffd}'.len_utf8())
                    .ok_or_else(|| CodecError::malformed("creo feature entity name length"))?;
                ctx.try_reserve_retained_text(&mut text, growth, operation)?;
                text.push_str(valid);
                text.push('\u{fffd}');
                let invalid_len = error.error_len().unwrap_or(remaining.len() - valid_len);
                remaining = &remaining[valid_len + invalid_len..];
            }
        }
    }
}
