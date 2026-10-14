// SPDX-License-Identifier: Apache-2.0
//! Complete named-target envelopes in reconstructed construction payloads.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use super::reference_index::FeatureReferenceToken;
use crate::printable_string::PrintableString;

const PREFIX: &[u8] = &crate::layout::named_construction_target_prefix::MAGIC_VALUE;
const NAME_PREFIX: &[u8] = &crate::layout::named_construction_name_prefix::MAGIC_VALUE;

/// Feature-reference token with the shortest encoding required by this envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "FeatureReferenceToken")]
pub(crate) struct NamedTargetReference(FeatureReferenceToken);

impl TryFrom<FeatureReferenceToken> for NamedTargetReference {
    type Error = &'static str;

    fn try_from(token: FeatureReferenceToken) -> Result<Self, Self::Error> {
        let width = match token.value() {
            0..=127 => 1,
            128..=4095 => 2,
            _ => 3,
        };
        if token.byte_len() != width {
            return Err("raw_object_index: named target requires a canonical feature reference");
        }
        Ok(Self(token))
    }
}

impl Serialize for NamedTargetReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl NamedTargetReference {
    pub(crate) fn value(self) -> u32 {
        self.0.value()
    }

    pub(crate) fn byte_len(self) -> u8 {
        self.0.byte_len()
    }
}

pub(crate) struct NamedTarget<'a> {
    pub(crate) reference: NamedTargetReference,
    pub(crate) value: PrintableString<&'a str>,
    pub(crate) offset: usize,
}

impl<'a> NamedTarget<'a> {
    fn read(bytes: &'a [u8], offset: usize) -> Option<Self> {
        let field = bytes.get(offset..)?;
        if !field.starts_with(PREFIX) {
            return None;
        }
        let token = FeatureReferenceToken::read(field.get(PREFIX.len()..)?)?;
        let reference = NamedTargetReference::try_from(token).ok()?;
        let name_at = PREFIX.len() + usize::from(reference.byte_len());
        if !field.get(name_at..)?.starts_with(NAME_PREFIX) {
            return None;
        }
        let length_at = name_at + NAME_PREFIX.len();
        let length = usize::from(*field.get(length_at)?).checked_sub(2)?;
        let start = length_at + 1;
        let end = start.checked_add(length)?;
        if field.get(end) != Some(&0) {
            return None;
        }
        let value = PrintableString::new(std::str::from_utf8(field.get(start..end)?).ok()?).ok()?;
        Some(Self {
            reference,
            value,
            offset,
        })
    }

    pub(crate) fn reference_offset(&self) -> usize {
        self.offset + PREFIX.len()
    }

    pub(crate) fn name_offset(&self) -> usize {
        self.reference_offset() + usize::from(self.reference.byte_len()) + 2
    }
}

pub(crate) fn named_targets<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Vec<NamedTarget<'a>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX named targets",
    )?;
    let mut targets = Vec::new();
    for (offset, window) in bytes.windows(PREFIX.len()).enumerate() {
        if window == PREFIX {
            if let Some(target) = NamedTarget::read(bytes, offset) {
                ctx.reserve_vec(&mut targets, 1, "NX named targets")?;
                targets.push(target);
            }
        }
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::{NamedTarget, NamedTargetReference};
    use crate::om::reference_index::FeatureReferenceToken;

    #[test]
    fn complete_named_envelopes_require_shortest_reference_and_exact_name_frame() {
        let mut frame = vec![0xe0, 0x3c, 1, 0x16, 0xa1];
        for (value, token) in [
            (0, vec![0]),
            (127, vec![127]),
            (128, vec![0x80, 128]),
            (4095, vec![0x8f, 255]),
            (4096, vec![0x90, 16, 0]),
        ] {
            frame.truncate(5);
            frame.extend(token);
            frame.extend([0xa7, 1, 0x66, 0x32, 3, 6]);
            frame.extend(b"Name\0");
            let target = NamedTarget::read(&frame, 0).unwrap();
            assert_eq!(target.reference.value(), value);
            assert_eq!(target.value.as_str(), "Name");
            for end in 0..frame.len() {
                assert!(NamedTarget::read(&frame[..end], 0).is_none());
            }
        }
        for (value, width) in [(1, 2), (1, 3), (128, 3)] {
            assert!(NamedTargetReference::try_from(
                FeatureReferenceToken::with_width(value, width).unwrap()
            )
            .is_err());
        }
        for offset in [0, 1, 2, 3, 4, 8, 9, 10, 11, 12, 18] {
            let mut wrong = frame.clone();
            wrong[offset] ^= 255;
            assert!(NamedTarget::read(&wrong, 0).is_none());
        }
        let mut wrong = frame;
        wrong[14] = 0x1f;
        assert!(NamedTarget::read(&wrong, 0).is_none());
    }
}
