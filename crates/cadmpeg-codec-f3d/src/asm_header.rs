// SPDX-License-Identifier: Apache-2.0
//! ASM `BinaryFile` header parsing and history-partition location.
//!
//! Implements only what the f3d container spec §3 (ASM binary header) and §4
//! (history partition) establish, and nothing past it: the tag-level SAB record
//! stream is intentionally not decoded here.
//!
//! `BinaryFile8` layout: `0..16` magic `ASM BinaryFile8<`, `16..24` zero,
//! `24..32` big-endian u64 per-file version word, `32..40` big-endian `3`,
//! `40..48` big-endian `7`. The three `0x07`-tagged UTF-8 strings
//! (`product_family`, `product_version_string`, `save_date`) begin at byte 47:
//! the schema word's low byte (`0x07`) is reused as the first string's tag.
//!
//! `BinaryFile4` layout: `0..15` magic `ASM BinaryFile4` (no `<`), then four
//! little-endian u32 words — `15..19` ASM release, `19..23` record count,
//! `23..27` entity count, `27..31` flags (bit 0 set when the stream carries a
//! history partition). The string region begins at byte 31.
//!
//! In both widths, three `0x06`-tagged little-endian f64s (`scale`, `resabs`,
//! `resnor`) follow the strings, then the SAB record stream.

/// The recognized header fields of a Fusion ASM BREP stream.
#[derive(Debug, Clone, PartialEq)]
pub struct AsmHeader {
    /// Integer width the stream declares (`4` or `8`), from `ASM BinaryFileN`.
    pub width: u8,
    /// Per-file version/save word (big-endian u64 at offset 24). `BinaryFile8`
    /// only.
    pub version_word: Option<u64>,
    /// ASM binary format version (big-endian u64 at offset 32, constant `3`).
    /// `BinaryFile8` only.
    pub format_version: Option<u64>,
    /// ASM binary schema version (big-endian u64 at offset 40, constant `7`).
    /// `BinaryFile8` only.
    pub schema_version: Option<u64>,
    /// ASM release word (little-endian u32 at offset 15, e.g. `22700` for ASM
    /// 227). `BinaryFile4` only.
    pub release: Option<u32>,
    /// Record-count word (little-endian u32 at offset 19; `0` when unwritten).
    /// `BinaryFile4` only.
    pub record_count: Option<u32>,
    /// Entity-count word (little-endian u32 at offset 23). `BinaryFile4` only.
    pub entity_count: Option<u32>,
    /// Flags word (little-endian u32 at offset 27); bit 0 is set when the
    /// stream carries a history partition. `BinaryFile4` only.
    pub flags: Option<u32>,
    /// `product_family`, e.g. `Autodesk Neutron`.
    pub product_family: Option<String>,
    /// `product_version_string`, e.g. `ASM 231.6.3.65535 OSX`.
    pub product_version: Option<String>,
    /// `save_date`, the last export/save time string.
    pub save_date: Option<String>,
    /// Kernel `scale` slot (metadata, not a coordinate multiplier — spec §3).
    pub scale: Option<f64>,
    /// Absolute distance tolerance `resabs`.
    pub linear: Option<f64>,
    /// Normal tolerance `resnor`.
    pub angular: Option<f64>,
}

/// The ASM magic prefix common to both widths.
const MAGIC_PREFIX: &[u8] = b"ASM BinaryFile";

/// Byte offset at which the three `0x07`-tagged product strings begin in a
/// `BinaryFile8` header. The three big-endian header words sit at offsets 24,
/// 32, and 40; the word at 40 is the constant schema version `7`, so its low
/// byte at offset 47 is `0x07` — and that byte doubles as the first string's
/// `TAG_UTF8_U8` tag. The string region therefore begins at 47, one byte before
/// the nominal end of the word block.
const BF8_STRING_REGION_START: usize = 47;

/// Byte offset at which the string region begins in a `BinaryFile4` header:
/// directly after the 15-byte magic and four little-endian u32 words.
const BF4_STRING_REGION_START: usize = 31;

/// Returns `true` if `bytes` begins with an ASM `BinaryFile` magic. The
/// `BinaryFile8` magic is 16 bytes ending in `<`; the `BinaryFile4` magic is
/// the 15-byte prefix alone (byte 15 is the release word's low byte).
pub fn has_asm_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 16
        && bytes.starts_with(MAGIC_PREFIX)
        && match bytes[14] {
            b'8' => bytes[15] == b'<',
            b'4' => true,
            _ => false,
        }
}

/// Byte offset of the string region for the stream's declared width, or `None`
/// when the width is unrecognized.
fn string_region_start(bytes: &[u8]) -> Option<usize> {
    match bytes[14] {
        b'8' => Some(BF8_STRING_REGION_START),
        b'4' => Some(BF4_STRING_REGION_START),
        _ => None,
    }
}

/// Parse the header of a decompressed BREP stream. Returns `None` if the magic
/// is absent. Fields that cannot be read (short stream or unexpected tags) are
/// left `None` rather than guessed.
pub fn parse(bytes: &[u8]) -> Option<AsmHeader> {
    if !has_asm_magic(bytes) {
        return None;
    }
    let width = bytes[14] - b'0';
    let mut header = AsmHeader {
        width,
        version_word: None,
        format_version: None,
        schema_version: None,
        release: None,
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
            header.version_word = read_be_u64(bytes, 24);
            header.format_version = read_be_u64(bytes, 32);
            header.schema_version = read_be_u64(bytes, 40);
        }
        4 => {
            header.release = read_le_u32(bytes, 15);
            header.record_count = read_le_u32(bytes, 19);
            header.entity_count = read_le_u32(bytes, 23);
            header.flags = read_le_u32(bytes, 27);
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
    if !has_asm_magic(bytes) {
        return None;
    }
    let start = string_region_start(bytes)?;
    let (strings, doubles, cur) = read_string_region(bytes, start);
    (strings.len() == 3 && doubles.len() == 3).then_some(cur)
}

/// Read up to three `0x07`-tagged strings then up to three `0x06`-tagged
/// doubles starting at `start`. Returns what was read and the offset just past
/// the last successfully read element.
fn read_string_region(bytes: &[u8], start: usize) -> (Vec<String>, Vec<f64>, usize) {
    let mut cur = start;
    let mut strings = Vec::new();
    while strings.len() < 3 {
        match read_u8_string(bytes, cur) {
            Some((s, next)) => {
                strings.push(s);
                cur = next;
            }
            None => break,
        }
    }
    let mut doubles = Vec::new();
    while doubles.len() < 3 {
        match read_tagged_f64(bytes, cur) {
            Some((v, next)) => {
                doubles.push(v);
                cur = next;
            }
            None => break,
        }
    }
    (strings, doubles, cur)
}

/// The literal record-name leaf of a construction-history node (spec §4a). The
/// active model slice contains none of these; the first occurrence marks the
/// history boundary.
const DELTA_STATE: &[u8] = b"delta_state";

/// Byte offset of the first `delta_state` marker, i.e. the end of the active
/// solved-model slice (spec §4a). `None` if the stream carries no history
/// partition (as `.smb` construction snapshots do not).
pub fn first_delta_state_offset(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(DELTA_STATE.len())
        .position(|w| w == DELTA_STATE)
}

fn read_be_u64(bytes: &[u8], at: usize) -> Option<u64> {
    bytes.get(at..at + 8).map(|s| {
        u64::from_be_bytes(
            s.try_into()
                .expect("invariant: bytes.get(at..at+8) is an 8-byte slice"),
        )
    })
}

fn read_le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).map(|s| {
        u32::from_le_bytes(
            s.try_into()
                .expect("invariant: bytes.get(at..at+4) is a 4-byte slice"),
        )
    })
}

/// Read a `0x07`-tagged UTF-8 string (tag byte, u8 length, bytes). Returns the
/// decoded string and the offset just past it.
fn read_u8_string(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    if *bytes.get(at)? != 0x07 {
        return None;
    }
    let len = *bytes.get(at + 1)? as usize;
    let start = at + 2;
    let slice = bytes.get(start..start + len)?;
    let s = std::str::from_utf8(slice).ok()?.to_string();
    Some((s, start + len))
}

/// Read a `0x06`-tagged little-endian f64 (tag byte then 8 bytes). Returns the
/// value and the offset just past it.
fn read_tagged_f64(bytes: &[u8], at: usize) -> Option<(f64, usize)> {
    if *bytes.get(at)? != 0x06 {
        return None;
    }
    let slice = bytes.get(at + 1..at + 9)?;
    Some((
        f64::from_le_bytes(
            slice
                .try_into()
                .expect("invariant: bytes.get(at+1..at+9) is an 8-byte slice"),
        ),
        at + 9,
    ))
}
