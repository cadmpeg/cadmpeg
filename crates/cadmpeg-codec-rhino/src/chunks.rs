// SPDX-License-Identifier: Apache-2.0
//! Rhino 3DM headers, chunks, checksums, and bounded readers.

use std::fmt;

use cadmpeg_ir::wire::cursor::Cursor;

/// The fixed ASCII prefix of a 3DM file header.
pub(crate) const MAGIC: &[u8; 24] = b"3D Geometry File Format ";
/// The end-of-file chunk typecode.
pub(crate) const TCODE_ENDOFFILE: u32 = 0x0000_7fff;
/// The short table terminator typecode.
pub(crate) const TCODE_ENDOFTABLE: u32 = 0xffff_ffff;
/// The legacy summary chunk typecode.
pub(crate) const TCODE_SUMMARY: u32 = 0x0000_0002;
/// The V1 class UUID chunk typecode.
pub(crate) const TCODE_CLASS_UUID: u32 = 0x0002_fffb;
/// The bit marking a short chunk.
pub(crate) const TCODE_SHORT: u32 = 0x8000_0000;
/// The bit marking a CRC-bearing chunk.
pub(crate) const TCODE_CRC: u32 = 0x0000_8000;

const TCODE_CLASS_WRAPPER: u32 = 0x0002_7ffa;
const TCODE_CLASS_USERDATA: u32 = 0x0002_7ffd;
const TCODE_CLASS_USERDATA_HEADER: u32 = 0x0002_fff9;
const TCODE_CLASS_DATA: u32 = 0x0002_fffc;
const TCODE_CLASS_END: u32 = 0x8002_7fff;

/// Archive versions understood by the chunk layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveVersion {
    /// Archive version 1.
    V1,
    /// Archive version 2.
    V2,
    /// Archive version 3.
    V3,
    /// Archive version 4.
    V4,
    /// Legacy archive version 5.
    LegacyV5,
    /// Archive version 5 with the modern grammar.
    V5,
    /// Archive version 6.
    V6,
    /// Archive version 7.
    V7,
    /// Archive version 8.
    V8,
    /// A syntactically valid archive version outside the supported bands.
    Other(u64),
}

impl ArchiveVersion {
    fn classify(value: u64) -> Self {
        match value {
            1 => Self::V1,
            2 => Self::V2,
            3 => Self::V3,
            4 => Self::V4,
            5 => Self::LegacyV5,
            50 => Self::V5,
            60 => Self::V6,
            70 => Self::V7,
            80 => Self::V8,
            other => Self::Other(other),
        }
    }

    /// Returns the decimal archive version.
    pub(crate) fn value(self) -> u64 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
            Self::V3 => 3,
            Self::V4 => 4,
            Self::LegacyV5 => 5,
            Self::V5 => 50,
            Self::V6 => 60,
            Self::V7 => 70,
            Self::V8 => 80,
            Self::Other(value) => value,
        }
    }

    /// Returns whether chunks use eight-byte values.
    pub(crate) fn uses_eight_byte_values(self) -> bool {
        self.value() >= 50
    }

    /// Returns whether V1's optional EOF marker is allowed.
    pub(crate) fn allows_optional_eof(self) -> bool {
        matches!(self, Self::V1)
    }
}

/// A validated 32-byte 3DM header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    /// Decimal archive version.
    pub(crate) archive_version: ArchiveVersion,
}

/// Errors that mean the byte stream cannot be safely framed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FramingError {
    /// The input ended before a required field.
    Truncated { offset: usize, needed: usize },
    /// The fixed header grammar was invalid.
    InvalidHeader,
    /// A length or count was invalid.
    InvalidLength { offset: usize, value: i128 },
    /// A structural framing rule was violated.
    Structural { offset: usize, message: String },
    /// Arithmetic overflow occurred while deriving a boundary.
    Overflow { offset: usize },
    /// A derived boundary exceeded its containing bound.
    OutOfBounds {
        offset: usize,
        end: usize,
        bound: usize,
    },
    /// A required EOF marker was missing.
    MissingEof,
    /// The EOF file-size field disagreed with the input.
    FileSizeMismatch { declared: u64, actual: usize },
    /// The chunk typecode violates a framing invariant.
    InvalidTypecode(u32),
}

impl fmt::Display for FramingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { offset, needed } => {
                write!(f, "truncated at {offset}, need {needed} bytes")
            }
            Self::InvalidHeader => f.write_str("invalid 3DM header"),
            Self::InvalidLength { offset, value } => {
                write!(f, "invalid length {value} at {offset}")
            }
            Self::Structural { offset, message } => {
                write!(f, "framing error at {offset}: {message}")
            }
            Self::Overflow { offset } => write!(f, "offset arithmetic overflow at {offset}"),
            Self::OutOfBounds { offset, end, bound } => {
                write!(f, "range {offset}..{end} exceeds bound {bound}")
            }
            Self::MissingEof => f.write_str("missing end-of-file chunk"),
            Self::FileSizeMismatch { declared, actual } => {
                write!(f, "EOF declares {declared} bytes, input has {actual}")
            }
            Self::InvalidTypecode(typecode) => write!(f, "invalid typecode {typecode:#x}"),
        }
    }
}

impl std::error::Error for FramingError {}

/// Parses the exact 32-byte file header.
pub(crate) fn parse_header(bytes: &[u8]) -> Result<Header, FramingError> {
    if bytes.len() < 32 {
        return Err(FramingError::Truncated {
            offset: bytes.len(),
            needed: 32 - bytes.len(),
        });
    }
    if &bytes[..24] != MAGIC {
        return Err(FramingError::InvalidHeader);
    }
    let version = &bytes[24..32];
    let first_digit = version
        .iter()
        .position(u8::is_ascii_digit)
        .ok_or(FramingError::InvalidHeader)?;
    if version[..first_digit].iter().any(|byte| *byte != b' ')
        || version[first_digit..]
            .iter()
            .any(|byte| !byte.is_ascii_digit())
    {
        return Err(FramingError::InvalidHeader);
    }
    let value = std::str::from_utf8(&version[first_digit..])
        .map_err(|_| FramingError::InvalidHeader)?
        .parse::<u64>()
        .map_err(|_| FramingError::InvalidHeader)?;
    if value == 0 {
        return Err(FramingError::InvalidHeader);
    }
    Ok(Header {
        archive_version: ArchiveVersion::classify(value),
    })
}

/// A reader whose cursor and end are explicit offsets in an in-memory buffer.
///
/// The read position, bounds, and every byte read are delegated to the shared
/// poisoned [`Cursor`]; `bytes` and `end` are retained locally to serve
/// [`backing_bytes`](Self::backing_bytes)/[`end`](Self::end) and to reconstruct
/// the exact [`FramingError`] the cursor's fault kinds do not distinguish.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BoundedReader<'a> {
    bytes: &'a [u8],
    end: usize,
    cursor: Cursor<'a>,
}

impl<'a> BoundedReader<'a> {
    /// Creates a reader over `start..end`.
    pub(crate) fn new(bytes: &'a [u8], start: usize, end: usize) -> Result<Self, FramingError> {
        if start > end || end > bytes.len() {
            return Err(FramingError::OutOfBounds {
                offset: start,
                end,
                bound: bytes.len(),
            });
        }
        Ok(Self {
            bytes,
            end,
            cursor: Cursor::new(bytes).window(start, end),
        })
    }

    /// Returns the absolute cursor offset.
    pub(crate) fn position(&self) -> usize {
        self.cursor.position()
    }

    /// Returns the absolute end offset.
    pub(crate) fn end(&self) -> usize {
        self.end
    }

    /// Returns the complete backing byte slice.
    pub(crate) fn backing_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Returns a reader over the unread bounded bytes.
    pub(crate) fn unread(&self) -> Result<Self, FramingError> {
        Self::new(self.bytes, self.cursor.position(), self.end)
    }

    /// Returns the unread byte count.
    pub(crate) fn remaining(&self) -> usize {
        self.end - self.cursor.position()
    }

    /// Skips exactly `count` bytes.
    pub(crate) fn skip(&mut self, count: usize) -> Result<(), FramingError> {
        self.take(count).map(|_| ())
    }

    /// Reads a byte.
    pub(crate) fn u8(&mut self) -> Result<u8, FramingError> {
        let bytes = self.take(1)?;
        Ok(bytes[0])
    }

    /// Reads a little-endian unsigned 32-bit value.
    pub(crate) fn u32(&mut self) -> Result<u32, FramingError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    /// Reads a little-endian signed 32-bit value.
    pub(crate) fn i32(&mut self) -> Result<i32, FramingError> {
        Ok(self.u32()? as i32)
    }

    /// Reads a little-endian unsigned 64-bit value.
    pub(crate) fn u64(&mut self) -> Result<u64, FramingError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    /// Reads a little-endian signed 64-bit value.
    pub(crate) fn i64(&mut self) -> Result<i64, FramingError> {
        Ok(self.u64()? as i64)
    }

    /// Reads a little-endian signed 16-bit value.
    pub(crate) fn i16(&mut self) -> Result<i16, FramingError> {
        let bytes = self.take(2)?;
        Ok(i16::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    /// Reads a little-endian unsigned 16-bit value.
    pub(crate) fn u16(&mut self) -> Result<u16, FramingError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    /// Reads a little-endian IEEE-754 binary64 value.
    pub(crate) fn f64(&mut self) -> Result<f64, FramingError> {
        let bytes = self.take(8)?;
        Ok(f64::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    /// Reads an archive boolean encoded as one byte.
    pub(crate) fn bool(&mut self) -> Result<bool, FramingError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(FramingError::Structural {
                offset: self.position() - 1,
                message: format!("boolean value {value} is not 0 or 1"),
            }),
        }
    }

    /// Reads a little-endian IEEE-754 binary32 value.
    pub(crate) fn f32(&mut self) -> Result<f32, FramingError> {
        let bytes = self.take(4)?;
        Ok(f32::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    /// Returns a bounded slice and advances the cursor.
    ///
    /// An offset overflow keeps the distinct [`FramingError::Overflow`] message
    /// that the poisoned cursor folds into a truncation; any bounded read past
    /// [`end`](Self::end) becomes [`FramingError::OutOfBounds`] at the same
    /// trigger point the old `require` check raised it.
    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8], FramingError> {
        let start = self.cursor.position();
        let end = start
            .checked_add(count)
            .ok_or(FramingError::Overflow { offset: start })?;
        let result = self.cursor.take(count);
        if self.cursor.is_poisoned() {
            return Err(FramingError::OutOfBounds {
                offset: start,
                end,
                bound: self.end,
            });
        }
        Ok(result)
    }

    /// Reads a fixed-width byte array.
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], FramingError> {
        Ok(self.take(N)?.try_into().expect("array length checked"))
    }
}

/// Checks an untrusted signed count before converting it or allocating.
pub(crate) fn checked_count_bytes(
    count: i32,
    element_size: usize,
    remaining: usize,
    allocation_limit: usize,
    offset: usize,
) -> Result<usize, FramingError> {
    if count < 0 {
        return Err(FramingError::InvalidLength {
            offset,
            value: count as i128,
        });
    }
    let count = usize::try_from(count).map_err(|_| FramingError::Overflow { offset })?;
    if count > allocation_limit {
        return Err(FramingError::InvalidLength {
            offset,
            value: count as i128,
        });
    }
    let bytes = count
        .checked_mul(element_size)
        .ok_or(FramingError::Overflow { offset })?;
    if bytes > remaining {
        return Err(FramingError::OutOfBounds {
            offset,
            end: offset.saturating_add(bytes),
            bound: offset + remaining,
        });
    }
    Ok(bytes)
}

/// Whether checksum validation is selected for a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChecksumKind {
    /// No checksum is present.
    None,
    /// V1 CRC-CCITT checksum, stored in two bytes.
    Crc16,
    /// V2+ IEEE CRC32 checksum, stored in four bytes.
    Crc32,
}

/// Selects the checksum algorithm without treating V1's CRC bit as CRC32.
pub(crate) fn checksum_kind(
    archive: ArchiveVersion,
    typecode: u32,
    class_uuid: bool,
) -> ChecksumKind {
    if archive == ArchiveVersion::V1
        && (typecode & 0x0001_0000 != 0
            || typecode == TCODE_SUMMARY
            || class_uuid
            || typecode == TCODE_CLASS_UUID)
    {
        ChecksumKind::Crc16
    } else if archive.value() >= 2 && (typecode & TCODE_CRC != 0 || class_uuid) {
        ChecksumKind::Crc32
    } else {
        ChecksumKind::None
    }
}

/// A parsed chunk header and all ranges derived from its declared boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// Offset of the chunk typecode.
    pub(crate) header_start: usize,
    /// Raw typecode.
    pub(crate) typecode: u32,
    /// Whether the short bit is set.
    pub(crate) short: bool,
    /// Short value, or long body length.
    pub(crate) value: i64,
    /// Offset immediately after the typecode and value/length field.
    pub(crate) body_start: usize,
    /// End of the declared span, exclusive.
    pub(crate) declared_end: usize,
    /// Body bytes excluding a trailing checksum.
    pub(crate) body: std::ops::Range<usize>,
    /// Trailing checksum bytes, when selected.
    pub(crate) checksum: Option<std::ops::Range<usize>>,
    /// Offset of the next chunk.
    pub(crate) next_offset: usize,
    /// Checksum algorithm selected by the archive and typecode.
    pub(crate) checksum_kind: ChecksumKind,
}

impl Chunk {
    /// Returns the complete chunk range, including header and checksum.
    pub(crate) fn range(&self) -> std::ops::Range<usize> {
        self.header_start..self.next_offset
    }
}

/// Parses a chunk at `offset`, constrained by `parent_end`.
pub(crate) fn chunk_at(
    bytes: &[u8],
    offset: usize,
    parent_end: usize,
    archive: ArchiveVersion,
    class_uuid: bool,
) -> Result<Chunk, FramingError> {
    let mut reader = BoundedReader::new(bytes, offset, parent_end)?;
    let typecode = reader.u32()?;
    if typecode & 0x0000_4000 != 0
        && typecode != TCODE_ENDOFFILE
        && typecode != TCODE_ENDOFTABLE
        && !matches!(
            typecode,
            TCODE_CLASS_WRAPPER
                | TCODE_CLASS_USERDATA
                | TCODE_CLASS_USERDATA_HEADER
                | TCODE_CLASS_UUID
                | TCODE_CLASS_DATA
                | TCODE_CLASS_END
        )
    {
        return Err(FramingError::InvalidTypecode(typecode));
    }
    let short = typecode & TCODE_SHORT != 0;
    let width = if archive.uses_eight_byte_values() {
        8
    } else {
        4
    };
    let value = if width == 8 {
        reader.i64()?
    } else {
        reader.i32()? as i64
    };
    let body_start = reader.position();
    if short {
        return Ok(Chunk {
            header_start: offset,
            typecode,
            short: true,
            value,
            body_start,
            declared_end: body_start,
            body: body_start..body_start,
            checksum: None,
            next_offset: body_start,
            checksum_kind: ChecksumKind::None,
        });
    }
    if value < 0 {
        return Err(FramingError::InvalidLength {
            offset,
            value: value as i128,
        });
    }
    let declared_length = usize::try_from(value).map_err(|_| FramingError::Overflow { offset })?;
    let declared_end = body_start
        .checked_add(declared_length)
        .ok_or(FramingError::Overflow { offset })?;
    if declared_end > parent_end {
        return Err(FramingError::OutOfBounds {
            offset,
            end: declared_end,
            bound: parent_end,
        });
    }
    if typecode == TCODE_ENDOFFILE && declared_length != width {
        return Err(FramingError::InvalidLength {
            offset,
            value: value as i128,
        });
    }
    let kind = checksum_kind(archive, typecode, class_uuid);
    let checksum_width = match kind {
        ChecksumKind::None => 0,
        ChecksumKind::Crc16 => 2,
        ChecksumKind::Crc32 => 4,
    };
    if declared_length < checksum_width {
        return Err(FramingError::Truncated {
            offset: body_start,
            needed: checksum_width,
        });
    }
    let body_end = declared_end - checksum_width;
    Ok(Chunk {
        header_start: offset,
        typecode,
        short: false,
        value,
        body_start,
        declared_end,
        body: body_start..body_end,
        checksum: (checksum_width != 0).then_some(body_end..declared_end),
        next_offset: declared_end,
        checksum_kind: kind,
    })
}

/// Result of validating a selected trailing checksum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChecksumStatus {
    /// No checksum was selected.
    NotPresent,
    /// The stored checksum matched.
    Valid,
    /// The stored checksum did not match; framing remains recoverable.
    Mismatch { expected: u32, actual: u32 },
}

/// Computes the non-reflected V1 CRC-CCITT variant.
pub(crate) fn crc16(seed: u16, bytes: &[u8]) -> u16 {
    let mut crc = seed;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Verifies a parsed chunk's checksum without changing its recoverable boundary.
pub(crate) fn verify_checksum(bytes: &[u8], chunk: &Chunk) -> Result<ChecksumStatus, FramingError> {
    verify_checksum_ranges(bytes, chunk, std::slice::from_ref(&chunk.body))
}

/// Verifies a chunk checksum over its direct byte ranges.
///
/// Container checksums exclude complete nested chunks. Callers pass the
/// ordered ranges written directly at the container's nesting level.
pub(crate) fn verify_checksum_ranges(
    bytes: &[u8],
    chunk: &Chunk,
    ranges: &[std::ops::Range<usize>],
) -> Result<ChecksumStatus, FramingError> {
    let Some(checksum) = chunk.checksum.as_ref() else {
        return Ok(ChecksumStatus::NotPresent);
    };
    let stored = &bytes[checksum.clone()];
    if ranges
        .iter()
        .any(|range| range.start < chunk.body.start || range.end > chunk.body.end)
    {
        return Err(FramingError::Structural {
            offset: chunk.body.start,
            message: "checksum range escapes chunk body".to_string(),
        });
    }
    match chunk.checksum_kind {
        ChecksumKind::Crc16 => {
            let actual = u32::from(u16::from_le_bytes(stored.try_into().map_err(|_| {
                FramingError::Truncated {
                    offset: checksum.start,
                    needed: 2,
                }
            })?));
            let expected = u32::from(
                ranges
                    .iter()
                    .fold(1, |crc, range| crc16(crc, &bytes[range.clone()])),
            );
            Ok(if expected == actual {
                ChecksumStatus::Valid
            } else {
                ChecksumStatus::Mismatch { expected, actual }
            })
        }
        ChecksumKind::Crc32 => {
            let actual =
                u32::from_le_bytes(stored.try_into().map_err(|_| FramingError::Truncated {
                    offset: checksum.start,
                    needed: 4,
                })?);
            let mut hasher = crc32fast::Hasher::new();
            for range in ranges {
                hasher.update(&bytes[range.clone()]);
            }
            let expected = hasher.finalize();
            Ok(if expected == actual {
                ChecksumStatus::Valid
            } else {
                ChecksumStatus::Mismatch { expected, actual }
            })
        }
        ChecksumKind::None => Ok(ChecksumStatus::NotPresent),
    }
}

/// Decodes a packed one-byte payload version.
#[cfg(test)]
pub(crate) fn packed_version(value: u8) -> (i32, i32) {
    (i32::from(value >> 4), i32::from(value & 0x0f))
}

/// Decodes an anonymous little-endian `(i32 major, i32 minor)` version.
#[cfg(test)]
pub(crate) fn anonymous_version(
    reader: &mut BoundedReader<'_>,
) -> Result<(i32, i32), FramingError> {
    Ok((reader.i32()?, reader.i32()?))
}

/// The validated EOF marker and its declared file size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Eof {
    /// Offset of the EOF chunk.
    pub(crate) offset: usize,
    /// File size stored by the archive.
    pub(crate) file_size: u64,
}

/// Parses and validates EOF semantics for a complete input buffer.
pub(crate) fn parse_eof(
    bytes: &[u8],
    offset: usize,
    archive: ArchiveVersion,
) -> Result<Option<Eof>, FramingError> {
    if offset == bytes.len() && archive.allows_optional_eof() {
        return Ok(None);
    }
    if offset >= bytes.len() {
        return Err(FramingError::MissingEof);
    }
    let chunk = chunk_at(bytes, offset, bytes.len(), archive, false)?;
    if chunk.typecode != TCODE_ENDOFFILE
        || chunk.short
        || chunk.body.len()
            != if archive.uses_eight_byte_values() {
                8
            } else {
                4
            }
        || chunk.next_offset != bytes.len()
    {
        return Err(FramingError::MissingEof);
    }
    let file_size = if archive.uses_eight_byte_values() {
        u64::from_le_bytes(
            bytes[chunk.body.clone()]
                .try_into()
                .expect("EOF width checked"),
        )
    } else {
        u64::from(u32::from_le_bytes(
            bytes[chunk.body.clone()]
                .try_into()
                .expect("EOF width checked"),
        ))
    };
    if file_size != bytes.len() as u64 {
        return Err(FramingError::FileSizeMismatch {
            declared: file_size,
            actual: bytes.len(),
        });
    }
    Ok(Some(Eof { offset, file_size }))
}
