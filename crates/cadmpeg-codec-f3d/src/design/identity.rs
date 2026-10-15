// SPDX-License-Identifier: Apache-2.0
//! Construct input-derived identities.

use cadmpeg_core::CodecError;
use std::fmt::{self, Write};

struct Encoded<'a>(&'a str, bool);

impl fmt::Display for Encoded<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for character in self.0.chars() {
            let character = if self.1 {
                character.to_ascii_lowercase()
            } else {
                character
            };
            let mut bytes = [0u8; 4];
            let text = character.encode_utf8(&mut bytes);
            if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                for byte in text.as_bytes() {
                    let escaped = [
                        b'%',
                        HEX[usize::from(byte >> 4)],
                        HEX[usize::from(byte & 0x0f)],
                    ];
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

fn encoded_length(value: &str, operation: &'static str) -> Result<usize, CodecError> {
    let mut length = Length(0);
    write!(&mut length, "{}", Encoded(value, false))
        .map_err(|_| CodecError::malformed(format_args!("{operation} length overflows")))?;
    Ok(length.0)
}

fn mint<T: TryFrom<String>>(text: String) -> Result<T, CodecError>
where
    T::Error: fmt::Display,
{
    T::try_from(text)
        .map_err(|error| crate::design::text::malformed_design(format_args!("{error}")))
}

/// Fallible twin of `crate::ids::neutral_feature_id` for the two face-selection resolvers whose callers in `history/selection.rs` propagate a `Result`.
pub(super) fn neutral_feature_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<cadmpeg_ir::features::FeatureId, CodecError> {
    let operation = "f3d feature identifier";
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let kind = scope.kind_name();
    let stream_len = encoded_length(stream, operation)?;
    let kind_len = encoded_length(kind, operation)?;
    mint(format!(
        "f3d:model:feature#{stream_len}:{}{kind_len}:{}{}:{}",
        Encoded(stream, false),
        Encoded(kind, false),
        scope.feature_ordinal.get(),
        scope.record_index
    ))
}

#[cfg(test)]
mod tests;
