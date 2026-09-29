// SPDX-License-Identifier: Apache-2.0
//! Construct input-derived identities under the caller decode budget.

use cadmpeg_core::decode::{DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit};
use cadmpeg_core::CodecError;
use std::fmt::{self, Write};

struct Encoded<'a>(&'a str);

impl fmt::Display for Encoded<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for character in self.0.chars() {
            let mut bytes = [0u8; 4];
            let text = character.encode_utf8(&mut bytes);
            if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                for byte in text.as_bytes() {
                    let escaped = [b'%', HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]];
                    let escaped = std::str::from_utf8(&escaped).map_err(|_| fmt::Error)?;
                    formatter.write_str(escaped)?;
                }
            } else {
                formatter.write_str(text)?;
            }
        }
        Ok(())
    }
}

struct Length(usize);

impl fmt::Write for Length {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

fn allocation_refusal(ctx: Option<&DecodeContext<'_>>, operation: &'static str) -> CodecError {
    ctx.map_or_else(|| CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::Codec(operation),
        reason: ResourceFailure::AllocationFailed,
        limit: u64::MAX,
        used: 0,
        additional: 1,
        operation,
    }), |ctx| ctx.refuse_codec_limit(operation, 0, 1))
}

fn encoded_length(ctx: Option<&DecodeContext<'_>>, value: &str, operation: &'static str) -> Result<usize, CodecError> {
    let mut length = Length(0);
    write!(&mut length, "{}", Encoded(value)).map_err(|_| allocation_refusal(ctx, operation))?;
    Ok(length.0)
}

fn format_identity(ctx: Option<&DecodeContext<'_>>, arguments: fmt::Arguments<'_>, operation: &'static str) -> Result<String, CodecError> {
    let mut length = Length(0);
    fmt::write(&mut length, arguments).map_err(|_| allocation_refusal(ctx, operation))?;
    if let Some(ctx) = ctx {
        ctx.charge_retained(u64::try_from(length.0).map_err(|_| allocation_refusal(Some(ctx), operation))?, operation)?;
    }
    let mut text = String::new();
    text.try_reserve_exact(length.0).map_err(|_| allocation_refusal(ctx, operation))?;
    fmt::write(&mut text, arguments).map_err(|_| CodecError::malformed("identity formatting failed"))?;
    Ok(text)
}

pub(super) fn neutral_configuration_id(ctx: Option<&DecodeContext<'_>>, entry: &str, name: &str) -> Result<cadmpeg_ir::features::ConfigurationId, CodecError> {
    let operation = "f3d configuration identifier";
    let entry_len = encoded_length(ctx, entry, operation)?;
    let name_len = encoded_length(ctx, name, operation)?;
    let text = format_identity(ctx, format_args!("f3d:configuration:variant#{entry_len}:{}{name_len}:{}", Encoded(entry), Encoded(name)), operation)?;
    cadmpeg_ir::features::ConfigurationId::mint(text).map_err(|error| CodecError::malformed(format_args!("{error}")))
}

#[cfg(test)]
mod tests;
