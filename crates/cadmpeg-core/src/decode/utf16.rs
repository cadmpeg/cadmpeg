// SPDX-License-Identifier: Apache-2.0
//! UTF-16LE text decoding.

use crate::CodecError;

use super::{DecodeContext, View};

#[derive(Clone, Copy)]
enum Surrogates {
    Reject,
    Replace,
}

impl DecodeContext<'_> {
    /// Decodes `units` UTF-16LE code units, rejecting unpaired surrogates.
    pub fn utf16le_text(
        &self,
        bytes: &[u8],
        units: usize,
        trim_nul: bool,
        _operation: &'static str,
    ) -> Result<String, CodecError> {
        decode_text(bytes, units, trim_nul, Surrogates::Reject)
    }

    /// Decodes `units` UTF-16LE code units, replacing unpaired surrogates with
    /// U+FFFD.
    pub fn utf16le_lossy_text(
        &self,
        bytes: &[u8],
        units: usize,
        trim_nul: bool,
        _operation: &'static str,
    ) -> Result<String, CodecError> {
        decode_text(bytes, units, trim_nul, Surrogates::Replace)
    }
}

fn decode_text(
    bytes: &[u8],
    units: usize,
    trim_nul: bool,
    surrogates: Surrogates,
) -> Result<String, CodecError> {
    let byte_len = units
        .checked_mul(2)
        .ok_or_else(|| CodecError::malformed("UTF-16LE byte length overflow"))?;
    let bytes = bytes
        .get(..byte_len)
        .ok_or_else(|| CodecError::malformed("truncated UTF-16LE text"))?;
    characters(bytes, trim_nul, surrogates)
        .collect::<Result<String, _>>()
        .map_err(|_| CodecError::malformed("invalid UTF-16LE surrogate sequence"))
}

fn characters(
    bytes: &[u8],
    trim_nul: bool,
    surrogates: Surrogates,
) -> impl Iterator<Item = Result<char, std::char::DecodeUtf16Error>> + '_ {
    let mut view = View::over_retained(bytes);
    char::decode_utf16(
        std::iter::from_fn(move || view.u16_le()).take_while(move |unit| !trim_nul || *unit != 0),
    )
    .map(move |character| match (surrogates, character) {
        (Surrogates::Replace, Err(_)) => Ok(char::REPLACEMENT_CHARACTER),
        (_, character) => character,
    })
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::CodecError;

    #[test]
    fn utf16le_lossy_text_trims_first_nul() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");
        assert_eq!(
            ctx.utf16le_lossy_text(&[0, 0xd8, 0, 0, 0, 0xdc], 3, true, "replacement test")
                .expect("terminated replacement text"),
            "�"
        );
    }

    #[test]
    fn utf16le_text_rejects_odd_byte_window() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("empty root context");
        assert!(matches!(
            ctx.utf16le_text(&[0x41], 1, false, "UTF-16 test"),
            Err(CodecError::Malformed(_))
        ));
        assert!(matches!(
            ctx.utf16le_text(&[], usize::MAX, false, "UTF-16 test"),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn utf16le_text_rejects_invalid_surrogates_before_retaining() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root context");
        for bytes in [&[0, 0xd8][..], &[0, 0xdc][..], &[0, 0xd8, 0x41, 0][..]] {
            assert!(matches!(
                ctx.utf16le_text(bytes, bytes.len() / 2, false, "UTF-16 test"),
                Err(CodecError::Malformed(_))
            ));
        }
    }
}
