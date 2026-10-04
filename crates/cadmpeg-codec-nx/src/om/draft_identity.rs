// SPDX-License-Identifier: Apache-2.0
//! Exact draft identity prefixes and bounded identity frames.

use super::compact::{CompactIndexAtom, NullableCompactIndex};
use super::discriminators::DraftIdentityBranch;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

/// Decoded prefix fields used only at the native wire boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DraftIdentityForm {
    IndexedBranch {
        first_index: u32,
        second_index: Option<u32>,
        branch: DraftIdentityBranch,
    },
    Tagged {
        index: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Prefix {
    IndexedBranch {
        first: CompactIndexAtom,
        second: Option<CompactIndexAtom>,
        branch: DraftIdentityBranch,
    },
    Tagged {
        index: Option<CompactIndexAtom>,
    },
}

impl Prefix {
    fn read(bytes: &[u8]) -> Option<Self> {
        if bytes.first() != Some(&0x41) {
            return None;
        }
        if bytes.get(1) == Some(&0xf0) {
            let index = NullableCompactIndex::read(bytes, 2)?;
            let end = 2 + index.raw().len();
            (bytes.get(end..end + 3) == Some(&[0xff, 0x02, 0x01]))
                .then_some(Self::Tagged { index: index.atom })
        } else {
            let first = CompactIndexAtom::read(bytes.get(1..)?)?;
            let at = 1 + first.raw().len();
            if bytes.get(at) != Some(&0xf0) {
                return None;
            }
            let second = NullableCompactIndex::read(bytes, at + 1)?;
            let at = at + 1 + second.raw().len();
            let branch = DraftIdentityBranch::try_from(*bytes.get(at)?).ok()?;
            (bytes.get(at + 1) == Some(&0x01)).then_some(Self::IndexedBranch {
                first,
                second: second.atom,
                branch,
            })
        }
    }

    #[cfg(test)]
    fn raw(&self) -> Vec<u8> {
        let (bytes, len) = self.raw_stack();
        bytes[..len].to_vec()
    }

    fn raw_stack(&self) -> ([u8; 8], usize) {
        fn nullable(atom: Option<&CompactIndexAtom>) -> &[u8] {
            atom.map_or(&[0xff], CompactIndexAtom::raw)
        }
        let mut bytes = [0_u8; 8];
        let mut at = 0;
        let mut append = |part: &[u8]| {
            bytes[at..at + part.len()].copy_from_slice(part);
            at += part.len();
        };
        match self {
            Self::IndexedBranch {
                first,
                second,
                branch,
            } => {
                append(&[0x41]);
                append(first.raw());
                append(&[0xf0]);
                append(nullable(second.as_ref()));
                append(&[u8::from(*branch), 0x01]);
            }
            Self::Tagged { index } => {
                append(&[0x41, 0xf0]);
                append(nullable(index.as_ref()));
                append(&[0xff, 0x02, 0x01]);
            }
        }
        (bytes, at)
    }

    fn byte_len(&self) -> usize {
        let nullable_len =
            |atom: &Option<CompactIndexAtom>| atom.as_ref().map_or(1, |atom| atom.raw().len());
        match self {
            Self::IndexedBranch { first, second, .. } => {
                4 + first.raw().len() + nullable_len(second)
            }
            Self::Tagged { index } => 5 + nullable_len(index),
        }
    }

    fn form(&self) -> DraftIdentityForm {
        match self {
            Self::IndexedBranch {
                first,
                second,
                branch,
            } => DraftIdentityForm::IndexedBranch {
                first_index: first.value(),
                second_index: second.map(CompactIndexAtom::value),
                branch: *branch,
            },
            Self::Tagged { index } => DraftIdentityForm::Tagged {
                index: index.map(CompactIndexAtom::value),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DraftIdentityFrame {
    prefix: Prefix,
    identity: String,
    offset: u64,
}

impl DraftIdentityFrame {
    pub(super) fn read(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        offset: usize,
    ) -> Result<Option<Self>, CodecError> {
        let Some(prefix) = bytes.get(offset..).and_then(Prefix::read) else {
            return Ok(None);
        };
        let Some(start) = offset.checked_add(prefix.byte_len()) else {
            return Ok(None);
        };
        let Some(tail) = bytes.get(start..) else {
            return Ok(None);
        };
        let len = tail
            .iter()
            .take_while(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
            .count();
        if len == 0 || tail.get(len) != Some(&b'?') {
            return Ok(None);
        }
        let Some(text) = std::str::from_utf8(&tail[..len]).ok() else {
            return Ok(None);
        };
        let mut identity = ctx.retained_string(len, "NX draft identity text")?;
        ctx.append_retained(&mut identity, text, "NX admitted text append")?;
        Ok(Some(Self {
            prefix,
            identity,
            offset: u64_from_index(offset),
        }))
    }

    pub(crate) fn from_wire(
        prefix: &[u8],
        form: DraftIdentityForm,
        identity: String,
        offset: u64,
    ) -> Result<Self, &'static str> {
        let parsed = Prefix::read(prefix)
            .filter(|parsed| parsed.byte_len() == prefix.len())
            .ok_or("prefix is not a complete draft identity prefix")?;
        if parsed.form() != form {
            return Err("form disagrees with prefix");
        }
        if identity.is_empty()
            || !identity
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("identity must contain nonempty lowercase hexadecimal digits");
        }
        offset
            .checked_add(cadmpeg_core::decode::u64_from_index(parsed.byte_len()))
            .ok_or("payload_offset overflows identity_payload_offset")?;
        Ok(Self {
            prefix: parsed,
            identity,
            offset,
        })
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
    #[cfg(test)]
    pub(crate) fn prefix(&self) -> Vec<u8> {
        self.prefix.raw()
    }
    pub(crate) fn prefix_stack(&self) -> ([u8; 8], usize) {
        self.prefix.raw_stack()
    }
    pub(crate) fn form(&self) -> DraftIdentityForm {
        self.prefix.form()
    }
    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }
    pub(crate) fn identity_offset(&self) -> u64 {
        self.offset + cadmpeg_core::decode::u64_from_index(self.prefix.byte_len())
    }
}

#[cfg(test)]
mod tests {
    use super::{DraftIdentityForm, DraftIdentityFrame};

    #[test]
    fn draft_identity_text_refuses_retained_limit() {
        let bytes = b"A\xf0\x27\xff\x02\x01abc123?";

        crate::test_support::with_decode_context_over(
            bytes,
            |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                let error = DraftIdentityFrame::read(ctx, bytes, 0).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
                );
            },
        );
    }

    #[test]
    fn tagged_prefix_retains_alternate_compact_and_null_encodings() {
        for prefix in [
            &b"A\xf0\x00\xff\x02\x01"[..],
            &b"A\xf0\x80\x00\xff\x02\x01"[..],
            &b"A\xf0\xff\xff\x02\x01"[..],
        ] {
            let index = if prefix[2] == 0xff { None } else { Some(0) };
            let frame = DraftIdentityFrame::from_wire(
                prefix,
                DraftIdentityForm::Tagged { index },
                "0af".into(),
                10,
            )
            .unwrap();
            assert_eq!(frame.prefix(), prefix);
            assert_eq!(
                frame.identity_offset(),
                10 + cadmpeg_core::decode::u64_from_index(prefix.len())
            );
            let mut bytes = prefix.to_vec();
            bytes.extend_from_slice(b"0af?");
            let parsed = crate::test_support::with_decode_context(|ctx| {
                DraftIdentityFrame::read(ctx, &bytes, 0)
            })
            .unwrap()
            .unwrap();
            assert_eq!(parsed.prefix(), prefix);
            assert_eq!(parsed.form(), frame.form());
            let limit = u64::MAX - cadmpeg_core::decode::u64_from_index(prefix.len());
            assert_eq!(
                DraftIdentityFrame::from_wire(prefix, frame.form(), "0".into(), limit)
                    .unwrap()
                    .identity_offset(),
                u64::MAX
            );
            assert!(
                DraftIdentityFrame::from_wire(prefix, frame.form(), "0".into(), limit + 1).is_err()
            );
        }
    }
}
