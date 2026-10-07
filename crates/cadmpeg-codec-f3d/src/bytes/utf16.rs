// SPDX-License-Identifier: Apache-2.0
//! Validated UTF-16 text borrowed from a counted source field.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use std::fmt::Write;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Utf16View<'a> {
    raw: &'a [u8],
    utf8_len: usize,
}

impl<'a> Utf16View<'a> {
    /// Validate counted UTF-16LE code units after admitting their scan.
    pub(crate) fn new(ctx: &DecodeContext<'_>, raw: &'a [u8]) -> Result<Option<Self>, CodecError> {
        if !raw.len().is_multiple_of(2) {
            return Ok(None);
        }
        let mut view = View::over_retained(raw);
        let mut utf8_len = 0usize;
        let mut characters = char::decode_utf16(std::iter::from_fn(|| view.u16_le()));
        while let Some(character) = ctx.next_charged(&mut characters, "validate F3D UTF-16 text")? {
            let Ok(character) = character else {
                return Ok(None);
            };
            // Measure UTF-8 storage without constructing decoded text.
            let Some(next) = utf8_len.checked_add(character.len_utf8()) else {
                return Ok(None);
            };
            utf8_len = next;
        }
        Ok(Some(Self { raw, utf8_len }))
    }

    /// The decoded characters. Consumers admit their own traversal.
    pub(crate) fn chars(self) -> impl Iterator<Item = char> + 'a {
        let mut view = View::over_retained(self.raw);
        // Construction validates every code unit before this iterator is available.
        char::decode_utf16(std::iter::from_fn(move || view.u16_le())).flatten()
    }
    pub(crate) fn is_empty(self) -> bool {
        self.utf8_len == 0
    }
    /// Whether this is a 36-character hyphenated hexadecimal GUID. The check
    /// reads at most 36 characters.
    pub(crate) fn is_guid_hyphenated(self) -> bool {
        self.len() == 36 && self.guid_prefix()
    }
    /// UTF-8 length of the decoded text.
    pub(crate) fn len(self) -> usize {
        self.utf8_len
    }
    /// Whether the decoded text equals `text`. Unequal lengths need no scan.
    pub(crate) fn eq_str(self, ctx: &DecodeContext<'_>, text: &str) -> Result<bool, CodecError> {
        if self.len() != text.len() {
            return Ok(false);
        }
        ctx.all_by(
            self.chars().zip(text.chars()),
            |(left, right)| Ok(left == right),
            "compare F3D UTF-16 text",
        )
    }
    /// Whether both texts are equal under ASCII case folding. Unequal lengths
    /// need no scan.
    pub(crate) fn eq_ignore_ascii_case(
        self,
        ctx: &DecodeContext<'_>,
        other: Self,
    ) -> Result<bool, CodecError> {
        if self.len() != other.len() {
            return Ok(false);
        }
        ctx.all_by(
            self.chars().zip(other.chars()),
            |(left, right)| Ok(left.eq_ignore_ascii_case(&right)),
            "compare F3D UTF-16 text",
        )
    }
    /// Whether this is a 36- to 38-character GUID-like token. The check reads
    /// at most 38 characters.
    pub(crate) fn is_guid_relaxed(self) -> bool {
        matches!(self.len(), 36..=38)
            && self.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
    }
    /// Whether the first 36 characters form a hyphenated hexadecimal GUID.
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
    /// Whether this is a GUID followed by `_urn:`. The check reads at most 41
    /// characters.
    pub(crate) fn is_guid_urn_role(self) -> bool {
        self.guid_prefix() && self.chars().skip(36).take(5).eq("_urn:".chars())
    }
    pub(crate) fn to_retained(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        ctx.utf16le_text(self.raw, self.raw.len() / 2, false, operation)
    }

    pub(crate) fn to_scoped<'ctx>(
        self,
        ctx: &'ctx DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'ctx>), CodecError> {
        ctx.utf16le_scoped_text(self.raw, self.raw.len() / 2, false, operation)
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
    use super::Utf16View;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn units(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn utf16_validation_refuses_before_scanning() {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "validate F3D UTF-16 text",
            0,
            |ctx| Utf16View::new(ctx, &[b'A', 0]).map(|_| ()),
        );
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "validate F3D UTF-16 text")
        );
    }

    #[test]
    fn utf16_comparisons_charge_only_equal_lengths() {
        let raw = units("Ab");
        let longer = units("aBc");
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "compare F3D UTF-16 text",
            0,
            |ctx| {
                let view = Utf16View::new(ctx, &raw)?.unwrap();
                let longer = Utf16View::new(ctx, &longer)?.unwrap();
                assert!(!view.eq_str(ctx, "Abc").unwrap());
                assert!(!view.eq_ignore_ascii_case(ctx, longer).unwrap());
                view.eq_str(ctx, "Ab")
            },
        );
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "compare F3D UTF-16 text")
        );
    }

    #[test]
    fn utf16_invalid_first_scalar_does_not_charge_tail() {
        let mut raw = 0xdc00_u16.to_le_bytes().to_vec();
        raw.extend(units(&"a".repeat(128)));
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            assert_eq!(Utf16View::new(ctx, &raw).unwrap(), None);
            assert_eq!(ctx.resource_refusal(), None);
        });
    }

    #[test]
    fn utf16_guid_shapes_need_no_admission() {
        let guid = "01234567-89AB-CDEF-0123-456789ABCDEF";
        crate::test_support::with_decode_context(|ctx| {
            let raw = units(guid);
            let view = Utf16View::new(ctx, &raw).unwrap().unwrap();
            assert!(view.is_guid_hyphenated());
            assert!(view.is_guid_relaxed());
            let role = units(&format!("{guid}_urn:"));
            assert!(Utf16View::new(ctx, &role)
                .unwrap()
                .unwrap()
                .is_guid_urn_role());
            for text in [
                format!("{}😀", "0".repeat(32)),
                format!("{}é", "0".repeat(34)),
            ] {
                assert_eq!(text.len(), 36);
                let raw = units(&text);
                let view = Utf16View::new(ctx, &raw).unwrap().unwrap();
                assert!(!view.is_guid_hyphenated());
                assert!(!view.is_guid_relaxed());
            }
            let unicode_role = units(&format!("{guid}_urn😀"));
            assert!(!Utf16View::new(ctx, &unicode_role)
                .unwrap()
                .unwrap()
                .is_guid_urn_role());
        });
    }

    #[test]
    fn borrowed_utf16_retained_copy_refuses_exact_utf8_budget() {
        let raw = [0, 8];
        for retained in [2, 3] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = retained;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let view = Utf16View::new(&ctx, &raw).unwrap().unwrap();
            let result = view.to_retained(&ctx, "F3D UTF-16 test");
            if retained == 2 {
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes && limit.additional == 3));
            } else {
                assert_eq!(result.unwrap(), "ࠀ");
                assert!(ctx.charge_retained(1, "after F3D text").is_err());
            }
        }
    }
}
