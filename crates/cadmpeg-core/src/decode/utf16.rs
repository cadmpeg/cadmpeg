// SPDX-License-Identifier: Apache-2.0
//! Strict UTF-16LE text decoding with charged scans and retained storage.

use crate::CodecError;

use super::{u64_from_index, DecodeContext, View};

impl DecodeContext<'_> {
    /// Validates a UTF-16LE window and retains its exact UTF-8 text size.
    pub fn utf16le_text(
        &self,
        bytes: &[u8],
        units: usize,
        trim_nul: bool,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let byte_len = units
            .checked_mul(2)
            .ok_or_else(|| CodecError::malformed("UTF-16LE byte length overflow"))?;
        let bytes = bytes
            .get(..byte_len)
            .ok_or_else(|| CodecError::malformed("truncated UTF-16LE text"))?;
        self.charge_work(u64_from_index(units), operation)?;
        let mut length = 0_usize;
        for character in characters(bytes, trim_nul) {
            let character = character
                .map_err(|_| CodecError::malformed("invalid UTF-16LE surrogate sequence"))?;
            length = length
                .checked_add(character.len_utf8())
                .ok_or_else(|| CodecError::malformed("UTF-16LE text length overflow"))?;
        }
        self.charge_work(u64_from_index(units), operation)?;
        self.charge_work(u64_from_index(length), operation)?;
        let mut text = self.retained_string(length, operation)?;
        for character in characters(bytes, trim_nul) {
            text.push(character
                .map_err(|_| CodecError::malformed("invalid UTF-16LE surrogate sequence"))?);
        }
        Ok(text)
    }
}

fn characters(
    bytes: &[u8],
    trim_nul: bool,
) -> impl Iterator<Item = Result<char, std::char::DecodeUtf16Error>> + '_ {
    let mut view = View::over_retained(bytes);
    char::decode_utf16(std::iter::from_fn(move || view.u16_le())
        .take_while(move |unit| !trim_nul || *unit != 0))
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[test]
    fn utf16le_text_charges_exact_retained_utf8_bytes() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 10;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let bytes = [0x41, 0, 0xe9, 0, 0x00, 0x08, 0x3d, 0xd8, 0x00, 0xde];
        assert_eq!(ctx.utf16le_text(&bytes, 5, false, "UTF-16 test").unwrap(), "Aéࠀ😀");
        assert!(matches!(ctx.charge_retained(1, "after UTF-16"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 10));
    }

    #[test]
    fn utf16le_text_refuses_retained_limit_before_allocation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        assert!(matches!(ctx.utf16le_text(&[0x00, 0x08], 1, false, "UTF-16 test"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.used == 0 && limit.additional == 3 && limit.operation == "UTF-16 test"));
    }

    #[test]
    fn utf16le_text_rejects_odd_byte_window() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default()).unwrap();
        assert!(matches!(ctx.utf16le_text(&[0x41], 1, false, "UTF-16 test"), Err(CodecError::Malformed(_))));
        assert!(matches!(ctx.utf16le_text(&[], usize::MAX, false, "UTF-16 test"), Err(CodecError::Malformed(_))));
    }

    #[test]
    fn utf16le_text_rejects_invalid_surrogates_before_retaining() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        for bytes in [&[0, 0xd8][..], &[0, 0xdc][..], &[0, 0xd8, 0x41, 0][..]] {
            assert!(matches!(ctx.utf16le_text(bytes, bytes.len() / 2, false, "UTF-16 test"), Err(CodecError::Malformed(_))));
        }
    }

    #[test]
    fn utf16le_text_trims_at_first_nul() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        assert_eq!(ctx.utf16le_text(&[0x41, 0, 0, 0, 0, 0xd8], 3, true, "UTF-16 test").unwrap(), "A");
        assert_eq!(ctx.utf16le_text(&[0, 0], 1, true, "UTF-16 test").unwrap(), "");
        assert!(matches!(ctx.charge_retained(1, "after UTF-16"), Err(CodecError::ResourceLimit(_))));
    }

    #[test]
    fn utf16le_text_refuses_work_before_scanning() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        assert!(matches!(ctx.utf16le_text(&[0x41, 0], 1, false, "UTF-16 test"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
    }
}
