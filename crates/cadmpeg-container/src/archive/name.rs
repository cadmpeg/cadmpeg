// SPDX-License-Identifier: Apache-2.0
//! Typed ZIP filename reads from admitted declaration fields.

use super::{u16_at, u32_at, EntryRecord};
use crate::layout::{central_header, local_header};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn raw_source<'a>(root: View<'a>, entry: &EntryRecord) -> Result<View<'a>, CodecError> {
    let (fixed, length_at) = if entry.is_recovered() {
        (local_header::LEN, local_header::NAME_LENGTH)
    } else {
        (central_header::LEN, central_header::NAME_LENGTH)
    };
    let declaration = entry.declaration_range();
    let start = declaration
        .start
        .checked_add(u64_from_index(fixed))
        .ok_or_else(|| CodecError::malformed("ZIP filename offset overflows"))?;
    let length = u64::from(u16_at(
        root.window(),
        declaration.start + u64_from_index(length_at),
    )?);
    let end = start
        .checked_add(length)
        .filter(|end| *end <= declaration.end)
        .ok_or_else(|| CodecError::malformed("ZIP filename escapes its declaration"))?;
    relative(root, start, end)
}

fn relative(root: View<'_>, start: u64, end: u64) -> Result<View<'_>, CodecError> {
    let start = usize::try_from(start)
        .ok()
        .and_then(|at| root.start().checked_add(at))
        .ok_or_else(|| CodecError::malformed("ZIP filename start exceeds memory"))?;
    let end = usize::try_from(end)
        .ok()
        .and_then(|at| root.start().checked_add(at))
        .ok_or_else(|| CodecError::malformed("ZIP filename end exceeds memory"))?;
    root.child(start, end)
        .ok_or_else(|| CodecError::malformed("ZIP filename escapes archive"))
}

pub(super) fn read(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    entry: &EntryRecord,
) -> Result<String, CodecError> {
    let mut raw = raw_source(root, entry)?;
    let bytes = raw.req_take(raw.remaining())?;
    let mut selected = bytes;
    if !entry.is_recovered() {
        let raw_end = u64_from_index(raw.end() - root.start());
        let extra_length = u64::from(u16_at(
            root.window(),
            entry.declaration_range().start + u64_from_index(central_header::EXTRA_LENGTH),
        )?);
        let extra_end = raw_end
            .checked_add(extra_length)
            .filter(|end| *end <= entry.declaration_range().end)
            .ok_or_else(|| CodecError::malformed("ZIP filename extra escapes declaration"))?;
        let mut at = raw_end;
        let mut unicode = None;
        let mut original_crc = None;
        while at.checked_add(4).is_some_and(|end| end <= extra_end) {
            let tag = u16_at(root.window(), at)?;
            let size = u64::from(u16_at(root.window(), at + 2)?);
            let end = at
                .checked_add(4)
                .and_then(|value| value.checked_add(size))
                .filter(|end| *end <= extra_end)
                .ok_or_else(|| CodecError::malformed("ZIP filename extra is truncated"))?;
            if tag == 0x7075 && size >= 5 {
                let content = relative(root, at + 9, end)?;
                let original_crc = *original_crc.get_or_insert_with(|| crc32fast::hash(bytes));
                if u32_at(root.window(), at + 5)? == original_crc
                    && std::str::from_utf8(content.window()).is_ok()
                {
                    unicode = Some((relative(root, at + 5, at + 9)?, content, original_crc));
                }
            }
            at = end;
        }
        if let Some((mut checksum, mut content, original_crc)) = unicode {
            let expected = checksum.req_u32_le()?;
            let content = content.req_take(content.remaining())?;
            if expected != original_crc || std::str::from_utf8(content).is_err() {
                return Err(CodecError::malformed(
                    "ZIP selected Unicode name differs from declaration",
                ));
            }
            selected = content;
        }
    }
    let lossy = entry.is_recovered() && entry.uses_utf8_name_encoding();
    let utf8 = !entry.is_recovered() && std::str::from_utf8(selected).is_ok();
    let mut length = Some(0usize);
    characters(selected, lossy || utf8, |ch| {
        length = length.and_then(|length| length.checked_add(ch.len_utf8()));
    });
    let length =
        length.ok_or_else(|| CodecError::malformed("ZIP decoded filename length overflows"))?;
    let mut output = ctx.alloc_filled(length, 0u8, "ZIP decoded filename")?;
    let mut at = 0;
    characters(selected, lossy || utf8, |ch| {
        let mut encoded = [0; 4];
        let encoded = ch.encode_utf8(&mut encoded).as_bytes();
        output[at..at + encoded.len()].copy_from_slice(encoded);
        at += encoded.len();
    });
    String::from_utf8(output)
        .map_err(|_| CodecError::malformed("ZIP filename decoder emitted non-UTF-8"))
}

fn characters(bytes: &[u8], utf8: bool, mut emit: impl FnMut(char)) {
    if !utf8 {
        for byte in bytes {
            emit(cp437(*byte));
        }
        return;
    }
    for chunk in bytes.utf8_chunks() {
        for ch in chunk.valid().chars() {
            emit(ch);
        }
        if !chunk.invalid().is_empty() {
            emit(char::REPLACEMENT_CHARACTER);
        }
    }
}

pub(super) fn cp437(byte: u8) -> char {
    const HIGH: [char; 128] = [
        'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ',
        'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú',
        'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡',
        '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟',
        '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘',
        '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ',
        '∞', 'φ', 'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²',
        '■', '\u{a0}',
    ];
    if byte < 128 {
        char::from(byte)
    } else {
        HIGH[usize::from(byte - 128)]
    }
}

#[cfg(test)]
mod tests;
