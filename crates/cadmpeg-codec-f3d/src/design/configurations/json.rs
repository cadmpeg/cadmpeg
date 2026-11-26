// SPDX-License-Identifier: Apache-2.0
//! Parse configuration JSON, admitting each nested value against the decode depth limit.

use crate::design::json_value::ValueSeed;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::de::DeserializeSeed;
use serde_json::Value;

pub(super) fn parse_configuration_payload(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
    bytes: &[u8],
) -> Result<Value, CodecError> {
    let malformed = |error: &dyn std::fmt::Display| {
        CodecError::malformed(format_args!(
            "invalid F3D configuration JSON {entry_name}: {error}"
        ))
    };
    let text = std::str::from_utf8(bytes).map_err(|error| malformed(&error))?;
    let mut refusal = None;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let parsed = ValueSeed {
        ctx,
        refusal: &mut refusal,
    }
    .deserialize(&mut deserializer)
    .and_then(|value| {
        deserializer.end()?;
        Ok(value)
    });
    match parsed {
        Ok(value) => Ok(value),
        Err(error) => match refusal {
            Some(error) => Err(error),
            None => Err(malformed(&error)),
        },
    }
}

#[cfg(test)]
mod tests;
