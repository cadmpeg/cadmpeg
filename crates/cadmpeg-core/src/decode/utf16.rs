// SPDX-License-Identifier: Apache-2.0
//! UTF-16LE text decoding with charged scans and owned storage.

use crate::CodecError;

use super::{u64_from_index, DecodeContext, ScopedReservation, View};

#[derive(Clone, Copy)]
enum Surrogates {
    Reject,
    Replace,
}

impl DecodeContext<'_> {
    /// Validates a UTF-16LE window and retains its exact UTF-8 text size.
    pub fn utf16le_text(
        &self,
        bytes: &[u8],
        units: usize,
        trim_nul: bool,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let (bytes, length) =
            admit_text(self, bytes, units, trim_nul, Surrogates::Reject, operation)?;
        let mut text = self.retained_string(length, operation)?;
        write_text(&mut text, bytes, trim_nul, Surrogates::Reject)?;
        Ok(text)
    }

    /// Decodes strict UTF-16LE into storage held by a scoped reservation.
    pub fn utf16le_scoped_text<'ctx>(
        &'ctx self,
        bytes: &[u8],
        units: usize,
        trim_nul: bool,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'ctx>), CodecError> {
        let (bytes, length) =
            admit_text(self, bytes, units, trim_nul, Surrogates::Reject, operation)?;
        let (mut text, reservation) = self.scoped_string(length, operation)?;
        write_text(&mut text, bytes, trim_nul, Surrogates::Reject)?;
        Ok((text, reservation))
    }

    /// Retains exact UTF-8 text while replacing unpaired surrogates with U+FFFD.
    pub fn utf16le_lossy_text(
        &self,
        bytes: &[u8],
        units: usize,
        trim_nul: bool,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let (bytes, length) =
            admit_text(self, bytes, units, trim_nul, Surrogates::Replace, operation)?;
        let mut text = self.retained_string(length, operation)?;
        write_text(&mut text, bytes, trim_nul, Surrogates::Replace)?;
        Ok(text)
    }

    /// Holds replacement UTF-8 text in an exact-byte scoped reservation.
    pub fn utf16le_lossy_scoped_text<'ctx>(
        &'ctx self,
        bytes: &[u8],
        units: usize,
        trim_nul: bool,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'ctx>), CodecError> {
        let (bytes, length) =
            admit_text(self, bytes, units, trim_nul, Surrogates::Replace, operation)?;
        let (mut text, reservation) = self.scoped_string(length, operation)?;
        write_text(&mut text, bytes, trim_nul, Surrogates::Replace)?;
        Ok((text, reservation))
    }
}

fn admit_text<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    units: usize,
    trim_nul: bool,
    surrogates: Surrogates,
    operation: &'static str,
) -> Result<(&'a [u8], usize), CodecError> {
    let byte_len = units
        .checked_mul(2)
        .ok_or_else(|| CodecError::malformed("UTF-16LE byte length overflow"))?;
    let bytes = bytes
        .get(..byte_len)
        .ok_or_else(|| CodecError::malformed("truncated UTF-16LE text"))?;
    ctx.charge_work(u64_from_index(units), operation)?;
    let mut length = 0_usize;
    for character in characters(bytes, trim_nul, surrogates) {
        let character =
            character.map_err(|_| CodecError::malformed("invalid UTF-16LE surrogate sequence"))?;
        length = length
            .checked_add(character.len_utf8())
            .ok_or_else(|| CodecError::malformed("UTF-16LE text length overflow"))?;
    }
    ctx.charge_work(u64_from_index(units), operation)?;
    ctx.charge_work(u64_from_index(length), operation)?;
    Ok((bytes, length))
}

fn write_text(
    text: &mut String,
    bytes: &[u8],
    trim_nul: bool,
    surrogates: Surrogates,
) -> Result<(), CodecError> {
    for character in characters(bytes, trim_nul, surrogates) {
        text.push(
            character.map_err(|_| CodecError::malformed("invalid UTF-16LE surrogate sequence"))?,
        );
    }
    Ok(())
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
    fn utf16le_scoped_text_holds_exact_temporary_bytes() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 3;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root context");
        let (text, reservation) = ctx
            .utf16le_scoped_text(&[0, 8], 1, false, "scoped UTF-16 test")
            .expect("three-byte scoped text");
        assert_eq!(text, "ࠀ");
        drop(text);
        drop(reservation);
        let (text, _reservation) = ctx
            .utf16le_scoped_text(&[0, 8], 1, false, "scoped UTF-16 test")
            .expect("released temporary storage");
        assert_eq!(text, "ࠀ");
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
