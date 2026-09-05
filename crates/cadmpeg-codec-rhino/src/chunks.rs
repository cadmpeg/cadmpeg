// SPDX-License-Identifier: Apache-2.0
//! Rhino 3DM headers, chunks, checksums, and bounded readers.

use std::fmt;

use cadmpeg_core::decode::View;

use crate::layout::file_header;
use crate::layout::token;

/// The fixed ASCII prefix of a 3DM file header.
pub(crate) const MAGIC: &[u8; 24] = &file_header::MAGIC_VALUE;
/// The end-of-file chunk typecode.
pub(crate) const TCODE_ENDOFFILE: u32 = token::END_OF_FILE;
/// The short table terminator typecode.
pub(crate) const TCODE_ENDOFTABLE: u32 = token::END_OF_TABLE;
/// The legacy summary chunk typecode.
pub(crate) const TCODE_SUMMARY: u32 = 0x0200_0013;
const TCODE_V1_OPENNURBS_CLASS_UUID: u32 = 0x0002_fffd;
/// The bit marking a short chunk.
pub(crate) const TCODE_SHORT: u32 = token::TCODE_SHORT;
/// The bit marking a CRC-bearing chunk.
pub(crate) const TCODE_CRC: u32 = token::TCODE_CRC;
/// The short marker that terminates an `OpenNURBS` class child stream.
pub(crate) const TCODE_CLASS_END: u32 = 0x8002_7fff;

const CHECKSUM_CHILD_CAP: usize = 1 << 20;

// The first strict boolean reader version in the encoded openNURBS version
// form: 6.0.2017-08-24. Older files also store this value as YYYYMMDDn.
const STRICT_BOOLEAN_VERSION_ENCODED: i64 = 2_348_836_140;
const STRICT_BOOLEAN_VERSION_DATE: i64 = 201_708_240;

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
    /// Archive version 9.
    V9,
    /// A syntactically valid archive version outside the supported bands.
    Other(u64),
}

impl ArchiveVersion {
    /// Partitions the archive-version word. The sole read discriminant of this
    /// format; `crate::dialect` assigns the result its registry identity.
    pub(crate) fn from_word(value: u64) -> Self {
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
            90 => Self::V9,
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
            Self::V9 => 90,
            Self::Other(value) => value,
        }
    }

    /// Returns whether chunks use eight-byte values.
    pub(crate) fn uses_eight_byte_values(self) -> bool {
        self.value() >= 50
    }

    /// Returns whether the archive word selects the chunked grammar.
    pub(crate) const fn is_chunked(self) -> bool {
        !matches!(self, Self::V1)
    }

    /// Returns whether V1's optional EOF marker is allowed.
    pub(crate) fn allows_optional_eof(self) -> bool {
        matches!(self, Self::V1)
    }
}

/// A validated 32-byte 3DM header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    /// Byte offset of the 32-byte start section.
    pub(crate) start_offset: usize,
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
}

impl FramingError {
    pub(crate) fn structural(offset: usize, message: impl Into<String>) -> Self {
        Self::Structural {
            offset,
            message: message.into(),
        }
    }
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
        }
    }
}

impl std::error::Error for FramingError {}

/// Parses the exact 32-byte file header.
pub(crate) fn parse_header(bytes: &[u8]) -> Result<Header, FramingError> {
    let search_end = bytes
        .len()
        .min(33_554_432_usize.saturating_add(MAGIC.len()));
    let start_offset = bytes[..search_end]
        .windows(MAGIC.len())
        .position(|window| window == MAGIC)
        .ok_or(FramingError::InvalidHeader)?;
    let header_end = start_offset.saturating_add(file_header::LEN);
    if bytes.len() < header_end {
        return Err(FramingError::Truncated {
            offset: bytes.len(),
            needed: header_end - bytes.len(),
        });
    }
    let version = &bytes[start_offset + file_header::ARCHIVE_VERSION..header_end];
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
        start_offset,
        archive_version: ArchiveVersion::from_word(value),
    })
}

/// A reader whose cursor and end are explicit offsets in an in-memory buffer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BoundedReader<'a> {
    bytes: &'a [u8],
    view: View<'a>,
}

impl<'a> BoundedReader<'a> {
    /// Creates a reader over `start..end`.
    pub(crate) fn new(bytes: &'a [u8], start: usize, end: usize) -> Result<Self, FramingError> {
        let view =
            View::over_retained(bytes)
                .child(start, end)
                .ok_or(FramingError::OutOfBounds {
                    offset: start,
                    end,
                    bound: bytes.len(),
                })?;
        Ok(Self { bytes, view })
    }

    /// Returns the absolute cursor offset.
    pub(crate) fn position(&self) -> usize {
        self.view.position()
    }

    /// Returns the absolute end offset.
    pub(crate) fn end(&self) -> usize {
        self.view.end()
    }

    /// Returns the complete backing byte slice.
    pub(crate) fn backing_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Returns a reader over the unread bounded bytes.
    pub(crate) fn unread(&self) -> Result<Self, FramingError> {
        Self::new(self.bytes, self.view.position(), self.view.end())
    }

    /// Returns the unread byte count.
    pub(crate) fn remaining(&self) -> usize {
        self.view.remaining()
    }

    /// Skips exactly `count` bytes.
    pub(crate) fn skip(&mut self, count: usize) -> Result<(), FramingError> {
        self.need(count)?;
        self.view.skip(count).expect("length checked");
        Ok(())
    }

    /// Skips the unread suffix of this bounded payload.
    ///
    /// The chunk boundary has already been validated by [`chunk_at`].  A
    /// parser that has consumed all fields known to its payload version must
    /// therefore advance to that boundary instead of treating later fields
    /// as a framing failure.  Truncation and overrun still fail at the field
    /// read that reaches beyond the bound.
    pub(crate) fn skip_remaining(&mut self) -> Result<usize, FramingError> {
        let count = self.remaining();
        self.skip(count)?;
        Ok(count)
    }

    /// Reads a byte.
    pub(crate) fn u8(&mut self) -> Result<u8, FramingError> {
        self.need(1)?;
        Ok(self.view.u8().expect("length checked"))
    }

    /// Reads a little-endian unsigned 32-bit value.
    pub(crate) fn u32(&mut self) -> Result<u32, FramingError> {
        self.need(4)?;
        Ok(self.view.u32_le().expect("length checked"))
    }

    /// Reads a little-endian signed 32-bit value.
    pub(crate) fn i32(&mut self) -> Result<i32, FramingError> {
        self.need(4)?;
        Ok(self.view.i32_le().expect("length checked"))
    }

    /// Reads a little-endian unsigned 64-bit value.
    pub(crate) fn u64(&mut self) -> Result<u64, FramingError> {
        self.need(8)?;
        Ok(self.view.u64_le().expect("length checked"))
    }

    /// Reads a little-endian signed 64-bit value.
    pub(crate) fn i64(&mut self) -> Result<i64, FramingError> {
        self.need(8)?;
        Ok(self.view.i64_le().expect("length checked"))
    }

    /// Reads a little-endian signed 16-bit value.
    pub(crate) fn i16(&mut self) -> Result<i16, FramingError> {
        self.need(2)?;
        Ok(self.view.i16_le().expect("length checked"))
    }

    /// Reads a little-endian unsigned 16-bit value.
    pub(crate) fn u16(&mut self) -> Result<u16, FramingError> {
        self.need(2)?;
        Ok(self.view.u16_le().expect("length checked"))
    }

    /// Reads a little-endian IEEE-754 binary64 value.
    pub(crate) fn f64(&mut self) -> Result<f64, FramingError> {
        self.need(8)?;
        Ok(self.view.f64_le().expect("length checked"))
    }

    /// Reads an archive boolean encoded as one byte.
    pub(crate) fn bool(&mut self) -> Result<bool, FramingError> {
        Ok(self.u8()? != 0)
    }

    /// Reads an archive boolean with the writer-version validation rule.
    ///
    /// A missing writer version keeps the historical permissive behavior. Raw
    /// character fields must call [`BoundedReader::u8`] instead.
    pub(crate) fn bool_with_writer_version(
        &mut self,
        writer_version: Option<i64>,
    ) -> Result<bool, FramingError> {
        let offset = self.position();
        let value = self.u8()?;
        let strict = writer_version.is_some_and(|version| {
            version >= STRICT_BOOLEAN_VERSION_ENCODED
                || (STRICT_BOOLEAN_VERSION_DATE..1_000_000_000).contains(&version)
        });
        if strict && value > 1 {
            return Err(FramingError::structural(
                offset,
                "archive boolean must be encoded as 0 or 1",
            ));
        }
        Ok(value != 0)
    }

    /// Reads a little-endian IEEE-754 binary32 value.
    pub(crate) fn f32(&mut self) -> Result<f32, FramingError> {
        self.need(4)?;
        Ok(self.view.f32_le().expect("length checked"))
    }

    /// Returns a bounded slice and advances the cursor.
    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8], FramingError> {
        self.need(count)?;
        Ok(self.view.take(count).expect("length checked"))
    }

    /// Reads a fixed-width byte array.
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], FramingError> {
        self.need(N)?;
        Ok(self.view.array().expect("array length checked"))
    }

    fn need(&self, count: usize) -> Result<(), FramingError> {
        let offset = self.view.position();
        let end = offset
            .checked_add(count)
            .ok_or(FramingError::Overflow { offset })?;
        if end > self.view.end() {
            Err(FramingError::OutOfBounds {
                offset,
                end,
                bound: self.view.end(),
            })
        } else {
            Ok(())
        }
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

/// Trailing checksum algorithm selected for a long chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChecksumKind {
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
) -> Option<ChecksumKind> {
    if archive == ArchiveVersion::V1
        && (typecode & 0x0001_0000 != 0
            || typecode == TCODE_SUMMARY
            || class_uuid
            || typecode == TCODE_V1_OPENNURBS_CLASS_UUID)
    {
        Some(ChecksumKind::Crc16)
    } else if archive.value() >= 2 && (typecode & TCODE_CRC != 0 || class_uuid) {
        Some(ChecksumKind::Crc32)
    } else {
        None
    }
}

/// Short inline value, or long body plus optional trailing checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChunkBody {
    /// Short chunk: the inline value, and no payload bytes.
    Short(i64),
    /// Long chunk: payload range and optional trailing checksum.
    Long {
        /// Body bytes excluding a trailing checksum.
        body: std::ops::Range<usize>,
        /// Trailing checksum algorithm and bytes, when selected.
        checksum: Option<(ChecksumKind, std::ops::Range<usize>)>,
    },
}

/// A parsed chunk header and all ranges derived from its declared boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// Offset of the chunk typecode.
    pub(crate) header_start: usize,
    /// Raw typecode.
    pub(crate) typecode: u32,
    /// Offset immediately after the typecode and value/length field.
    pub(crate) body_start: usize,
    /// Short value or long payload.
    pub(crate) form: ChunkBody,
}

impl Chunk {
    /// Returns the complete chunk range, including header and checksum.
    pub(crate) fn range(&self) -> std::ops::Range<usize> {
        self.header_start..self.next_offset()
    }

    /// Returns whether this chunk carries an inline short value.
    pub(crate) fn short(&self) -> bool {
        matches!(self.form, ChunkBody::Short(_))
    }

    /// Returns the short value, or the declared long-body length.
    pub(crate) fn value(&self) -> i64 {
        match &self.form {
            ChunkBody::Short(value) => *value,
            ChunkBody::Long { body, checksum } => {
                let checksum_len = checksum.as_ref().map_or(0, |(_, range)| range.len());
                i64::try_from(body.len() + checksum_len).expect("chunk body length fits i64")
            }
        }
    }

    /// Returns the payload range. Short chunks have an empty range at `body_start`.
    pub(crate) fn body(&self) -> std::ops::Range<usize> {
        match &self.form {
            ChunkBody::Short(_) => self.body_start..self.body_start,
            ChunkBody::Long { body, .. } => body.clone(),
        }
    }

    /// Returns the exclusive end of the declared span.
    pub(crate) fn declared_end(&self) -> usize {
        self.next_offset()
    }

    /// Returns the offset of the next chunk.
    pub(crate) fn next_offset(&self) -> usize {
        match &self.form {
            ChunkBody::Short(_) => self.body_start,
            ChunkBody::Long { body, checksum } => {
                checksum.as_ref().map_or(body.end, |(_, range)| range.end)
            }
        }
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
    let short = typecode & TCODE_SHORT != 0;
    let width = if archive.uses_eight_byte_values() {
        8
    } else {
        4
    };
    let value = if width == 8 {
        reader.i64()?
    } else if !short
        || matches!(
            typecode,
            0x8000_0001 | 0x8000_0002 | 0xa000_0026 | 0x8200_0071
        )
    {
        i64::from(reader.u32()?)
    } else {
        i64::from(reader.i32()?)
    };
    let body_start = reader.position();
    if short || value < 0 {
        return Ok(Chunk {
            header_start: offset,
            typecode,
            body_start,
            form: ChunkBody::Short(value),
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
    if typecode == TCODE_ENDOFFILE && declared_length < width {
        return Err(FramingError::InvalidLength {
            offset,
            value: value as i128,
        });
    }
    let kind = checksum_kind(archive, typecode, class_uuid);
    let checksum_width = match kind {
        None => 0,
        Some(ChecksumKind::Crc16) => 2,
        Some(ChecksumKind::Crc32) => 4,
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
        body_start,
        form: ChunkBody::Long {
            body: body_start..body_end,
            checksum: kind.map(|kind| (kind, body_end..declared_end)),
        },
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

/// Computes the augmented non-reflected V1 CRC-CCITT variant.
pub(crate) fn crc16(seed: u16, bytes: &[u8]) -> u16 {
    let mut crc = seed;
    for byte in bytes {
        let mut table = crc & 0xff00;
        for _ in 0..8 {
            table = if table & 0x8000 != 0 {
                (table << 1) ^ 0x1021
            } else {
                table << 1
            };
        }
        crc = (crc << 8) ^ u16::from(*byte) ^ table;
    }
    crc
}

/// Verifies a parsed chunk's checksum without changing its recoverable boundary.
pub(crate) fn verify_checksum(bytes: &[u8], chunk: &Chunk) -> Result<ChecksumStatus, FramingError> {
    let body = chunk.body();
    verify_checksum_ranges(bytes, chunk, std::slice::from_ref(&body))
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
    let Some((kind, checksum)) = (match &chunk.form {
        ChunkBody::Long {
            checksum: Some((kind, range)),
            ..
        } => Some((*kind, range.clone())),
        _ => None,
    }) else {
        return Ok(ChecksumStatus::NotPresent);
    };
    let body = chunk.body();
    let stored = &bytes[checksum.clone()];
    if ranges
        .iter()
        .any(|range| range.start < body.start || range.end > body.end)
    {
        return Err(FramingError::Structural {
            offset: body.start,
            message: "checksum range escapes chunk body".to_string(),
        });
    }
    match kind {
        ChecksumKind::Crc16 => {
            let actual = u32::from(View::u16_le_at(stored, 0).ok_or(FramingError::Truncated {
                offset: checksum.start,
                needed: 2,
            })?);
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
            let actual = View::u32_le_at(stored, 0).ok_or(FramingError::Truncated {
                offset: checksum.start,
                needed: 4,
            })?;
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
    }
}

/// Returns the parent-level byte ranges after complete child chunks are removed.
pub(crate) fn direct_checksum_ranges(
    body: &std::ops::Range<usize>,
    children: &[std::ops::Range<usize>],
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut children = children.to_vec();
    children.sort_by_key(|range| range.start);
    let mut cursor = body.start;
    let mut direct = Vec::with_capacity(children.len() + 1);
    for child in children {
        if child.start < cursor || child.end < child.start || child.end > body.end {
            return Err(FramingError::Structural {
                offset: child.start,
                message: "nested checksum range overlaps or escapes its parent".to_string(),
            });
        }
        if cursor < child.start {
            direct.push(cursor..child.start);
        }
        cursor = child.end;
    }
    if cursor < body.end {
        direct.push(cursor..body.end);
    }
    Ok(direct)
}

/// Frames complete nested chunks through a short zero class-end marker.
pub(crate) fn checksum_children_through_class_end(
    data: &[u8],
    body: std::ops::Range<usize>,
    archive: ArchiveVersion,
    context: &str,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let mut children = Vec::new();
    loop {
        if reader.position() == reader.end() {
            return Err(FramingError::structural(
                reader.end(),
                format!("{context} is missing its class end"),
            ));
        }
        let start = reader.position();
        let child = chunk_at(data, start, reader.end(), archive, false)?;
        if child.next_offset() <= start {
            return Err(FramingError::structural(
                start,
                format!("{context} child did not advance"),
            ));
        }
        if children.len() >= CHECKSUM_CHILD_CAP {
            return Err(FramingError::InvalidLength {
                offset: start,
                value: children.len() as i128,
            });
        }
        children.push(child.range());
        reader.skip(child.next_offset() - start)?;
        if child.typecode == TCODE_CLASS_END {
            if !child.short() || child.value() != 0 {
                return Err(FramingError::structural(
                    start,
                    format!("{context} class end must be a short zero chunk"),
                ));
            }
            return Ok(children);
        }
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
    let body = chunk.body();
    if chunk.typecode != TCODE_ENDOFFILE
        || chunk.short()
        || body.len()
            < if archive.uses_eight_byte_values() {
                8
            } else {
                4
            }
    {
        return Err(FramingError::MissingEof);
    }
    let mut body = BoundedReader::new(bytes, body.start, body.end)?;
    let file_size = if archive.uses_eight_byte_values() {
        body.u64()?
    } else {
        u64::from(body.u32()?)
    };
    Ok(Some(Eof { offset, file_size }))
}

#[cfg(test)]
mod direct_range_tests {
    use super::*;

    #[test]
    fn direct_checksum_ranges_exclude_complete_sorted_children() {
        assert_eq!(
            direct_checksum_ranges(&(10..50), &[30..40, 15..20]).expect("valid nesting"),
            vec![10..15, 20..30, 40..50]
        );
    }

    #[test]
    fn direct_checksum_ranges_reject_overlap() {
        assert!(direct_checksum_ranges(&(10..50), &[15..30, 20..40]).is_err());
    }
}

#[cfg(test)]
mod tests;
