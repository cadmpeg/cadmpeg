// SPDX-License-Identifier: Apache-2.0
//! Validated UTF-16 text borrowed from a counted source field.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use std::fmt::Write;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Utf16View<'a> {
    raw: &'a [u8],
    utf8_len: usize,
}

impl<'a> Utf16View<'a> {
    pub(crate) fn new(raw: &'a [u8]) -> Option<Self> {
        if !raw.len().is_multiple_of(2) {
            return None;
        }
        let mut view = View::over_retained(raw);
        let mut utf8_len = 0usize;
        for character in char::decode_utf16(std::iter::from_fn(|| view.u16_le())) {
            utf8_len = utf8_len.checked_add(character.ok()?.len_utf8())?;
        }
        Some(Self { raw, utf8_len })
    }

    pub(crate) fn chars(self) -> impl Iterator<Item = char> + 'a {
        let mut view = View::over_retained(self.raw);
        // Construction validates every code unit before this iterator is available.
        char::decode_utf16(std::iter::from_fn(move || view.u16_le())).flatten()
    }
    pub(crate) fn is_empty(self) -> bool {
        self.utf8_len == 0
    }
    pub(crate) fn is_guid_hyphenated(self) -> bool {
        self.len() == 36 && self.guid_prefix()
    }
    pub(crate) fn len(self) -> usize {
        self.utf8_len
    }
    pub(crate) fn eq_str(self, text: &str) -> bool {
        self.chars().eq(text.chars())
    }
    pub(crate) fn eq_ignore_ascii_case(self, other: Self) -> bool {
        self.chars()
            .map(|character| character.to_ascii_lowercase())
            .eq(other
                .chars()
                .map(|character| character.to_ascii_lowercase()))
    }
    pub(crate) fn is_guid_relaxed(self) -> bool {
        matches!(self.len(), 36..=38)
            && self.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
    }
    fn guid_prefix(self) -> bool {
        self.len() >= 36
            && self.chars().take(36).enumerate().all(|(index, character)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    character == '-'
                } else {
                    character.is_ascii_hexdigit()
                }
            })
    }
    pub(crate) fn is_guid_urn_role(self) -> bool {
        self.guid_prefix() && self.chars().skip(36).take(5).eq("_urn:".chars())
    }
    pub(crate) fn to_retained(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.charge_copy(ctx, operation)?;
        let mut text = ctx.retained_string(self.utf8_len, operation)?;
        text.extend(self.chars());
        Ok(text)
    }

    pub(crate) fn to_scoped<'ctx>(
        self,
        ctx: &'ctx DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'ctx>), CodecError> {
        self.charge_copy(ctx, operation)?;
        let mut reservation = ctx.reserve_scoped(0, operation)?;
        let mut text = String::new();
        ctx.reserve_scoped_string(&mut reservation, &mut text, self.utf8_len, operation)?;
        text.extend(self.chars());
        Ok((text, reservation))
    }

    fn charge_copy(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let work = u64_from_index(self.raw.len())
            .checked_add(u64_from_index(self.utf8_len))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, operation)
    }
}

impl std::fmt::Display for Utf16View<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for character in self.chars() {
            formatter.write_char(character)?;
        }
        Ok(())
    }
}
impl std::fmt::Debug for Utf16View<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("\"")?;
        for character in self.chars() {
            if character == '\'' {
                formatter.write_char(character)?;
            } else {
                for escaped in character.escape_debug() {
                    formatter.write_char(escaped)?;
                }
            }
        }
        formatter.write_str("\"")
    }
}
