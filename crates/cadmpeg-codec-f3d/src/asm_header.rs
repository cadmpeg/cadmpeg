// SPDX-License-Identifier: Apache-2.0
//! ASM `BinaryFile` header parsing and history-partition location.
//!
//! Implements only what the f3d container spec §5 (ASM binary header) and §4
//! (history partition) establish, and nothing past it: the tag-level SAB record
//! stream is intentionally not decoded here. Byte layout (BinaryFile8):
//! `0..16` magic `ASM BinaryFile8<`, `16..24` zero, `24..32` big-endian u64
//! per-file version word, `32..40` big-endian `3`, `40..48` big-endian `7`.
//! The three `0x07`-tagged UTF-8 strings (`product_family`,
//! `product_version_string`, `save_date`) begin at byte 47: the schema word's
//! low byte (`0x07`) is reused as the first string's tag (see
//! [`STRING_REGION_START`]). Three `0x06`-tagged little-endian f64s (`scale`,
//! `resabs`, `resnor`) follow, then the SAB record stream.

/// The recognized header fields of a Fusion ASM BREP stream.
#[derive(Debug, Clone, PartialEq)]
pub struct AsmHeader {
    /// Integer width the stream declares (`4` or `8`), from `ASM BinaryFileN<`.
    pub width: u8,
    /// Per-file version/save word (big-endian u64 at offset 24). Present for the
    /// `BinaryFile8` layout only; the spec does not document `BinaryFile4`.
    pub version_word: Option<u64>,
    /// ASM binary format version (constant `3` on the corpus).
    pub format_version: Option<u64>,
    /// ASM binary schema version (constant `7` on the corpus).
    pub schema_version: Option<u64>,
    /// `product_family`, e.g. `Autodesk Neutron`.
    pub product_family: Option<String>,
    /// `product_version_string`, e.g. `ASM 231.6.3.65535 OSX`.
    pub product_version: Option<String>,
    /// `save_date`, the last export/save time string.
    pub save_date: Option<String>,
    /// Kernel `scale` slot (corpus-constant `60.0`; metadata, not a coordinate
    /// multiplier — spec §5).
    pub scale: Option<f64>,
    /// Absolute distance tolerance `resabs`.
    pub resabs: Option<f64>,
    /// Normal tolerance `resnor`.
    pub resnor: Option<f64>,
}

/// The ASM magic prefix common to both widths.
const MAGIC_PREFIX: &[u8] = b"ASM BinaryFile";

/// Byte offset at which the three `0x07`-tagged product strings begin in a
/// `BinaryFile8` header. The three big-endian header words sit at offsets 24,
/// 32, and 40; the word at 40 is the constant schema version `7`, so its low
/// byte at offset 47 is `0x07` — and that byte doubles as the first string's
/// `TAG_UTF8_U8` tag. The string region therefore begins at 47, one byte before
/// the nominal end of the word block.
const STRING_REGION_START: usize = 47;

/// Returns `true` if `bytes` begins with an ASM `BinaryFile` magic.
pub fn has_asm_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 16
        && bytes.starts_with(MAGIC_PREFIX)
        && bytes[14].is_ascii_digit()
        && bytes[15] == b'<'
}

/// Parse the header of a decompressed BREP stream. Returns `None` if the magic
/// is absent. Fields that cannot be read (short stream, `BinaryFile4`, or
/// unexpected tags) are left `None` rather than guessed.
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
        product_family: None,
        product_version: None,
        save_date: None,
        scale: None,
        resabs: None,
        resnor: None,
    };

    // Only the BinaryFile8 layout is documented; do not fabricate fields for
    // other widths.
    if width != 8 || bytes.len() < 48 {
        return Some(header);
    }

    header.version_word = read_be_u64(bytes, 24);
    header.format_version = read_be_u64(bytes, 32);
    header.schema_version = read_be_u64(bytes, 40);

    // The three product strings and three tolerance doubles follow the fixed
    // prefix, beginning at byte `STRING_REGION_START`. Parse them by tag rather
    // than fixed offset so differing string lengths do not desync the doubles.
    let mut cur = STRING_REGION_START;
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

    let mut it = strings.into_iter();
    header.product_family = it.next();
    header.product_version = it.next();
    header.save_date = it.next();
    let mut dit = doubles.into_iter();
    header.scale = dit.next();
    header.resabs = dit.next();
    header.resnor = dit.next();

    Some(header)
}

/// Byte offset at which the SAB record stream begins, i.e. the first byte after
/// the fixed `BinaryFile8` header (the three `0x07`-tagged product strings and
/// three `0x06`-tagged tolerance doubles). The record stream's first record is
/// the `asmheader`, which is `RecordTable` index 0. Returns `None` for streams
/// without the documented `BinaryFile8` layout (e.g. `BinaryFile4`), whose
/// record framing this codec does not decode.
pub fn record_stream_start(bytes: &[u8]) -> Option<usize> {
    if !has_asm_magic(bytes) {
        return None;
    }
    let width = bytes[14] - b'0';
    if width != 8 || bytes.len() < 48 {
        return None;
    }
    let mut cur = STRING_REGION_START;
    let mut strings = 0;
    while strings < 3 {
        match read_u8_string(bytes, cur) {
            Some((_, next)) => {
                strings += 1;
                cur = next;
            }
            None => return None,
        }
    }
    let mut doubles = 0;
    while doubles < 3 {
        match read_tagged_f64(bytes, cur) {
            Some((_, next)) => {
                doubles += 1;
                cur = next;
            }
            None => return None,
        }
    }
    Some(cur)
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
