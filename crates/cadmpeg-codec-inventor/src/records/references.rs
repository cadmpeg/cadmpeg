// SPDX-License-Identifier: Apache-2.0
//! Tagged metadata reference entries.

use crate::layout::{
    meta_reference_header as header, meta_reference_identifier as identifier,
    meta_reference_pair as pair,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub(crate) key: u16,
    pub(crate) value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", content = "value", rename_all = "snake_case")]
pub(crate) enum Value {
    Identifier1(NativeBytes<[u8; 16]>),
    Identifier2(NativeBytes<[u8; 16]>),
    Pair3([u32; 2]),
}

impl Entry {
    pub(crate) fn read(view: &mut View<'_>) -> Result<Self, CodecError> {
        let key = crate::reader::u16(view, "metadata reference key")?;
        let tag = crate::reader::u8(view, "metadata reference tag")?;
        let value = match tag {
            1 => Value::Identifier1(crate::reader::array(view, "metadata identifier 1")?.into()),
            2 => Value::Identifier2(crate::reader::array(view, "metadata identifier 2")?.into()),
            3 => Value::Pair3(crate::reader::u32_array(view, "metadata reference pair")?),
            _ => {
                return Err(CodecError::malformed(format_args!(
                    "RSe metadata reference tag {tag} is unsupported"
                )));
            }
        };
        Ok(Self { key, value })
    }

    pub(crate) fn encoded_len(&self) -> usize {
        header::LEN
            + match self.value {
                Value::Identifier1(_) | Value::Identifier2(_) => identifier::LEN,
                Value::Pair3(_) => pair::LEN,
            }
    }
}

/// Validate a section-9 payload. A payload that is not a complete tagged
/// sequence is retained when its length is the fixed-entry form: `count`
/// entries of a key, a tag, and a 16-byte identifier.
pub(super) fn validate(payload: View<'_>, count: u32) -> Result<(), CodecError> {
    match validate_tagged(payload, count) {
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) if !is_fixed_entry_form(payload, count) => Err(error),
        _ => Ok(()),
    }
}

/// True when `payload` has the length of `count` fixed-width entries.
pub(crate) fn is_fixed_entry_form(payload: View<'_>, count: u32) -> bool {
    usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(header::LEN + identifier::LEN))
        == Some(payload.window().len())
}

fn validate_tagged(mut payload: View<'_>, count: u32) -> Result<(), CodecError> {
    payload
        .counted(u64::from(count), header::LEN + pair::LEN)
        .ok_or_else(|| CodecError::malformed("RSe metadata reference count exceeds its payload"))?;
    for _ in 0..count {
        Entry::read(&mut payload)?;
    }
    if !payload.is_empty() {
        return Err(CodecError::malformed(
            "RSe metadata reference table has trailing bytes",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
