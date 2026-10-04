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
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        raw: &'a [u8],
    ) -> Result<Option<Self>, CodecError> {
        if !raw.len().is_multiple_of(2) {
            return Ok(None);
        }
        let mut view = View::over_retained(raw);
        let mut admitted = ctx.admit_iter(raw, "validate F3D UTF-16 text")?;
        let units = std::iter::from_fn(move || {
            admitted.next()?;
            admitted.next()?;
            view.u16_le()
        });
        let mut utf8_len = 0usize;
        for character in char::decode_utf16(units) {
            let Some(length) = character.ok().map(char::len_utf8) else {
                return Ok(None);
            };
            let Some(next_length) = utf8_len.checked_add(length) else {
                return Err(ctx.refuse_codec_limit(
                    "validate F3D UTF-16 text",
                    cadmpeg_core::decode::u64_from_index(usize::MAX - length),
                    cadmpeg_core::decode::u64_from_index(utf8_len),
                ));
            };
            utf8_len = next_length;
        }
        Ok(Some(Self { raw, utf8_len }))
    }

    pub(crate) fn chars(self) -> impl Iterator<Item = char> + 'a {
        let mut view = View::over_retained(self.raw);
        // Construction validates every code unit before this iterator is available.
        char::decode_utf16(std::iter::from_fn(move || view.u16_le())).flatten()
    }
    fn utf16_units(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<impl Iterator<Item = u16> + 'a, CodecError> {
        let mut view = View::over_retained(self.raw);
        let mut admitted = ctx.admit_iter(self.raw, operation)?;
        Ok(std::iter::from_fn(move || {
            admitted.next()?;
            admitted.next()?;
            view.u16_le()
        }))
    }
    pub(crate) fn is_empty(self) -> bool {
        self.utf8_len == 0
    }
    pub(crate) fn is_guid_hyphenated(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        if self.len() != 36 {
            return Ok(false);
        }
        self.guid_prefix(ctx)
    }
    pub(crate) fn len(self) -> usize {
        self.utf8_len
    }
    pub(crate) fn eq_str(
        self,
        ctx: &DecodeContext<'_>,
        text: &str,
    ) -> Result<bool, CodecError> {
        let decoded = self.to_scoped(ctx, "compare F3D UTF-16 text")?;
        ctx.equal(decoded.0.as_str(), text, "compare F3D UTF-16 text")
    }
    pub(crate) fn eq_ignore_ascii_case(
        self,
        ctx: &DecodeContext<'_>,
        other: Self,
    ) -> Result<bool, CodecError> {
        let left = self.to_scoped(ctx, "compare F3D UTF-16 text")?;
        let right = other.to_scoped(ctx, "compare F3D UTF-16 text")?;
        ctx.eq_ignore_ascii_case(&left.0, &right.0, "compare F3D UTF-16 text")
    }
    pub(crate) fn is_guid_relaxed(self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if !matches!(self.len(), 36..=38) {
            return Ok(false);
        }
        Ok(self.utf16_units(ctx, "validate F3D relaxed UTF-16 GUID")?.all(|unit| {
            let Ok(byte) = u8::try_from(unit) else {
                return false;
            };
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
        }))
    }
    fn guid_prefix(self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if self.len() < 36 {
            return Ok(false);
        }
        Ok(self
            .utf16_units(ctx, "validate F3D UTF-16 GUID prefix")?
            .take(36)
            .enumerate()
            .all(|(index, unit)| {
                let Ok(byte) = u8::try_from(unit) else {
                    return false;
                };
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            }))
    }
    pub(crate) fn is_guid_urn_role(self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if !self.guid_prefix(ctx)? {
            return Ok(false);
        }
        let mut suffix = [0u16; 5];
        let mut count = 0usize;
        for unit in self
            .utf16_units(ctx, "validate F3D UTF-16 GUID role")?
            .skip(36)
            .take(5)
        {
            suffix[count] = unit;
            count += 1;
        }
        if count != suffix.len() {
            return Ok(false);
        }
        ctx.equal(
            &suffix,
            &[
                u16::from(b'_'),
                u16::from(b'u'),
                u16::from(b'r'),
                u16::from(b'n'),
                u16::from(b':'),
            ],
            "compare F3D UTF-16 GUID role",
        )
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

    fn assert_fused_work_refusal<T>(
        ctx: &DecodeContext<'_>,
        result: Result<T, CodecError>,
        operation: &'static str,
    ) {
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
        let refusal = ctx.resource_refusal().expect("operation refusal is fused");
        assert_eq!(refusal.operation, operation);
        let CodecError::ResourceLimit(fused) = ctx.charge_work(0, "repeat UTF-16 test refusal")
            .expect_err("the original refusal remains fused")
        else {
            panic!("resource refusal remains fused");
        };
        assert_eq!(fused, refusal);
    }

    #[test]
    fn utf16_validation_propagates_work_refusal() {
        let raw = [b'A', 0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        assert!(matches!(Utf16View::new(&ctx, &raw), Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits));
    }

    #[test]
    fn borrowed_utf16_retained_copy_refuses_exact_utf8_budget() {
        let raw = [0, 8];
        let view = crate::test_support::with_decode_context(|ctx| {
            Utf16View::new(ctx, &raw).unwrap().unwrap()
        });
        for retained in [2, 3] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = retained;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
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

    #[test]
    fn utf16_guid_scans_propagate_work_refusal() {
        let guid = "01234567-89AB-CDEF-0123-456789ABCDEF";
        let raw: Vec<u8> = guid.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let view = crate::test_support::with_decode_context(|ctx| {
            Utf16View::new(ctx, &raw).unwrap().unwrap()
        });
        let role: Vec<u8> = format!("{guid}_urn:")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let role_view = crate::test_support::with_decode_context(|ctx| {
            Utf16View::new(ctx, &role).unwrap().unwrap()
        });
        crate::test_support::with_decode_context(|ctx| {
            assert!(view.is_guid_hyphenated(ctx).unwrap());
            assert!(view.is_guid_relaxed(ctx).unwrap());
            assert!(role_view.is_guid_urn_role(ctx).unwrap());
        });

        for text in [format!("{}😀", "0".repeat(32)), format!("{}é", "0".repeat(34))] {
            assert_eq!(text.len(), 36);
            let raw: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let view = crate::test_support::with_decode_context(|ctx| {
                Utf16View::new(ctx, &raw).unwrap().unwrap()
            });
            crate::test_support::with_decode_context(|ctx| {
                assert!(!view.is_guid_hyphenated(ctx).unwrap());
                assert!(!view.is_guid_relaxed(ctx).unwrap());
            });
        }

        let unicode_role: Vec<u8> = format!("{guid}_urn😀")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let unicode_role_view = crate::test_support::with_decode_context(|ctx| {
            Utf16View::new(ctx, &unicode_role).unwrap().unwrap()
        });
        crate::test_support::with_decode_context(|ctx| {
            assert!(!unicode_role_view.is_guid_urn_role(ctx).unwrap());
        });

        for check in 0..2 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = if check == 0 {
                view.is_guid_hyphenated(&ctx)
            } else {
                view.is_guid_relaxed(&ctx)
            };
            let operation = if check == 0 {
                "validate F3D UTF-16 GUID prefix"
            } else {
                "validate F3D relaxed UTF-16 GUID"
            };
            assert_fused_work_refusal(&ctx, result, operation);
        }

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(role.len()).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        assert_fused_work_refusal(
            &ctx,
            role_view.is_guid_urn_role(&ctx),
            "validate F3D UTF-16 GUID role",
        );
    }
}
