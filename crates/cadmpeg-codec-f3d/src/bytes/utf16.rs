// SPDX-License-Identifier: Apache-2.0
//! Validated UTF-16 text borrowed from a counted source field.

use cadmpeg_core::decode::View;
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
        let mut ascii_units = 0_usize;
        for chunk in raw.chunks_exact(2) {
            if chunk[1] == 0 && chunk[0] < 0x80 {
                ascii_units += 1;
            } else {
                let mut view = View::over_retained(&raw[ascii_units * 2..]);
                let mut utf8_len = ascii_units;
                for character in char::decode_utf16(std::iter::from_fn(|| view.u16_le())) {
                    utf8_len = utf8_len.checked_add(character.ok()?.len_utf8())?;
                }
                return Some(Self { raw, utf8_len });
            }
        }
        Some(Self {
            raw,
            utf8_len: ascii_units,
        })
    }

    /// Validate UTF-16LE bytes while rejecting any Unicode control character
    /// immediately during the single scan (avoids allocating or scanning past
    /// random binary prefixes in speculative scans).
    pub(crate) fn new_no_control(raw: &'a [u8]) -> Option<Self> {
        if !raw.len().is_multiple_of(2) {
            return None;
        }
        let mut ascii_units = 0_usize;
        for chunk in raw.chunks_exact(2) {
            if chunk[1] == 0 && chunk[0] < 0x80 {
                if chunk[0] < 0x20 || chunk[0] == 0x7f {
                    return None;
                }
                ascii_units += 1;
            } else {
                let mut view = View::over_retained(&raw[ascii_units * 2..]);
                let mut utf8_len = ascii_units;
                for character in char::decode_utf16(std::iter::from_fn(|| view.u16_le())) {
                    let ch = character.ok()?;
                    if ch.is_control() {
                        return None;
                    }
                    utf8_len = utf8_len.checked_add(ch.len_utf8())?;
                }
                return Some(Self { raw, utf8_len });
            }
        }
        Some(Self {
            raw,
            utf8_len: ascii_units,
        })
    }

    pub(crate) fn to_owned_string(self) -> String {
        let mut out = String::with_capacity(self.utf8_len);
        if self.utf8_len * 2 == self.raw.len() {
            for chunk in self.raw.chunks_exact(2) {
                out.push(char::from(chunk[0]));
            }
            return out;
        }
        for character in self.chars() {
            out.push(character);
        }
        out
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

#[cfg(test)]
mod tests {
    #[test]
    fn utf16_view_no_control_rejects_control_and_accepts_text() {
        let ok_bytes: Vec<u8> = "Scale_1\u{00e9}"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let view = super::Utf16View::new_no_control(&ok_bytes).unwrap();
        assert_eq!(view.to_owned_string(), "Scale_1\u{00e9}");

        let ctrl_bytes: Vec<u8> = "a\nb".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(super::Utf16View::new_no_control(&ctrl_bytes).is_none());

        let del_bytes: Vec<u8> = "a\x7fb".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(super::Utf16View::new_no_control(&del_bytes).is_none());
    }
}
