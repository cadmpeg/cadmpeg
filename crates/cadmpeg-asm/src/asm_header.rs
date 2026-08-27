// SPDX-License-Identifier: Apache-2.0
//! Parse ASM `BinaryFile4` and `BinaryFile8` headers and locate history data.
//!
//! [`parse`] reads format words, product strings, scale, and tolerances.
//! [`record_stream_start`] locates the first SAB record, and
//! [`solved_record_limit`] returns the exact boundary between solved BREP
//! records and the optional construction-history partition.
//!
//! `BinaryFile8` layout: `0..15` magic `ASM BinaryFile8`, `15..19`
//! little-endian u32 ACIS save-format version, `19..31` zero, `31..39`
//! little-endian u64 entity count, `39..47` little-endian u64 flags. Bit 0 marks a history
//! partition and bits 1 to 7 hold the save format's revision number. The three
//! `0x07`-tagged UTF-8 strings (`product_family`,
//! `product_version_string`, `save_date`) begin at byte 47.
//!
//! `BinaryFile4` layout: `0..15` magic `ASM BinaryFile4`, then four
//! little-endian u32 words: `15..19` ACIS save-format version, `19..23`
//! record count, `23..27` entity count, `27..31` flags, with the same bit
//! assignment as `BinaryFile8`. The string region begins at byte 31.
//!
//! In both widths, three `0x06`-tagged little-endian f64s (`scale`, `resabs`,
//! `resnor`) follow the strings, then the SAB record stream.

use crate::kernel_header::{read_string_region, KernelHeader};
use crate::layout::asmheader_binaryfile4 as bf4;
use crate::layout::asmheader_binaryfile8 as bf8;
use cadmpeg_core::decode::View;

/// The ASM magic prefix common to both widths.
const MAGIC_PREFIX: &[u8] = b"ASM BinaryFile";

/// Returns `true` if `bytes` begins with an ASM `BinaryFile` magic: the
/// 15-byte prefix `ASM BinaryFile4` or `ASM BinaryFile8`. Byte 15 is the
/// save-format version's low byte in both widths, not part of the magic.
pub fn has_asm_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 16 && bytes.starts_with(MAGIC_PREFIX) && matches!(bytes[14], b'4' | b'8')
}

/// The integer and reference width `bytes` declares, in bytes. A slice without
/// a readable header is read at the `BinaryFile8` width, which is the width of
/// every construction the decoder synthesizes.
pub fn stream_ref_width(bytes: &[u8]) -> usize {
    parse(bytes).map_or(8, |header| usize::from(header.width))
}

/// Byte offset of the string region for the stream's declared width, or `None`
/// when the width is unrecognized.
fn string_region_start(bytes: &[u8]) -> Option<usize> {
    match bytes[14] {
        b'8' => Some(bf8::LEN),
        b'4' => Some(bf4::LEN),
        _ => None,
    }
}

/// Parse the header of a decompressed ASM stream. Returns `None` if the magic
/// is absent. Fields that cannot be read (short stream or unexpected tags) are
/// left `None` rather than guessed.
pub fn parse(bytes: &[u8]) -> Option<KernelHeader> {
    if !has_asm_magic(bytes) {
        return None;
    }
    let width = bytes[14] - b'0';
    let mut header = KernelHeader {
        width,
        save_format_version: None,
        record_count: None,
        entity_count: None,
        flags: None,
        product_family: None,
        product_version: None,
        save_date: None,
        scale: None,
        linear: None,
        angular: None,
    };

    match width {
        8 => {
            header.save_format_version = View::u32_le_at(bytes, bf8::SAVE_FORMAT_VERSION);
            header.entity_count = View::u64_le_at(bytes, bf8::ENTITY_COUNT);
            header.flags = View::u64_le_at(bytes, bf8::FLAGS);
        }
        4 => {
            header.save_format_version = View::u32_le_at(bytes, bf4::SAVE_FORMAT_VERSION);
            header.record_count = View::u32_le_at(bytes, bf4::RECORD_COUNT);
            header.entity_count = View::u32_le_at(bytes, bf4::ENTITY_COUNT).map(u64::from);
            header.flags = View::u32_le_at(bytes, bf4::FLAGS).map(u64::from);
        }
        _ => return Some(header),
    }

    // The three product strings and three tolerance doubles follow the fixed
    // word block. Parse them by tag rather than fixed offset so differing
    // string lengths do not desync the doubles.
    let Some(start) = string_region_start(bytes) else {
        return Some(header);
    };
    let (strings, doubles, _) = read_string_region(bytes, start);

    let mut it = strings.into_iter();
    header.product_family = it.next();
    header.product_version = it.next();
    header.save_date = it.next();
    let mut dit = doubles.into_iter();
    header.scale = dit.next();
    header.linear = dit.next();
    header.angular = dit.next();

    Some(header)
}

/// Byte offset at which the SAB record stream begins, i.e. the first byte after
/// the fixed header words, the three `0x07`-tagged product strings, and the
/// three `0x06`-tagged tolerance doubles. The record stream's first record is
/// the `asmheader`, which is `RecordTable` index 0. Returns `None` for streams
/// without a recognized header layout.
pub fn record_stream_start(bytes: &[u8]) -> Option<usize> {
    let header = parse(bytes)?;
    record_stream_start_with_header(bytes, &header)
}

/// Byte offset at which the SAB record stream begins, using an already-parsed
/// ASM header.
pub fn record_stream_start_with_header(bytes: &[u8], header: &KernelHeader) -> Option<usize> {
    let start = match header.width {
        8 => bf8::LEN,
        4 => bf4::LEN,
        _ => return None,
    };
    let (strings, doubles, cur) = read_string_region(bytes, start);
    (strings.len() == 3 && doubles.len() == 3).then_some(cur)
}

/// The record that opens the serialized history partition.
const HISTORY_PREAMBLE_RECORD: &str = "Begin-of-ASM-History-Data";

/// Exact byte boundary between solved BREP records and construction history.
///
/// The SAB framer establishes record boundaries from token widths and subtype
/// depth. A current history stream contains a `Begin-of-ASM-History-Data`
/// preamble record; its record start is the partition boundary. Earlier
/// streams can begin directly with `delta_state`; for those, the first
/// unframed record after the solved sequence is accepted only when its exact
/// identifier token is `delta_state`. Raw substring search is deliberately not
/// used because string payloads can contain the same bytes.
pub fn solved_record_limit(bytes: &[u8]) -> Option<usize> {
    let header = parse(bytes)?;
    solved_record_limit_with_header(bytes, &header)
}

/// Exact solved-record boundary, using an already-parsed ASM header.
pub fn solved_record_limit_with_header(bytes: &[u8], header: &KernelHeader) -> Option<usize> {
    if !header.has_history_partition() {
        return None;
    }
    let start = record_stream_start_with_header(bytes, header)?;
    let records = crate::sab::frame(bytes, start, bytes.len(), usize::from(header.width)).ok()?;
    if let Some(preamble) = records
        .iter()
        .find(|record| record.name == HISTORY_PREAMBLE_RECORD)
    {
        return Some(preamble.offset);
    }

    let mut next = match records.last() {
        Some(record) => record.offset.checked_add(record.len)?,
        None => start,
    };
    while bytes.get(next) == Some(&0x11) {
        next += 1;
    }
    exact_identifier_at(bytes, next, "delta_state").then_some(next)
}

fn exact_identifier_at(bytes: &[u8], at: usize, expected: &str) -> bool {
    let Some((&0x0d, rest)) = bytes.get(at..).and_then(|tail| tail.split_first()) else {
        return false;
    };
    let Some((&length, payload)) = rest.split_first() else {
        return false;
    };
    usize::from(length) == expected.len()
        && payload.get(..usize::from(length)) == Some(expected.as_bytes())
}
