// SPDX-License-Identifier: Apache-2.0
//! Rhino 3DM headers, chunks, checksums, and bounded readers.

use std::borrow::Borrow;
use std::fmt;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};

use crate::layout::file_header;
use crate::layout::token;

/// The fixed ASCII prefix of a 3DM file header.
pub(crate) const MAGIC: &[u8; 24] = &file_header::MAGIC_VALUE;
/// The end-of-file chunk typecode.
pub(crate) const TCODE_ENDOFFILE: u32 = token::END_OF_FILE;
/// The short table terminator typecode.
pub(crate) const TCODE_ENDOFTABLE: u32 = token::END_OF_TABLE;
/// The legacy summary chunk typecode.
const TCODE_SUMMARY: u32 = 0x0200_0013;
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
    fn allows_optional_eof(self) -> bool {
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
    /// A structural rule was violated by a derived or already-decoded value
    /// that has no byte position of its own.
    Unpositioned { message: String },
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
    /// The active decode session refused a resource request.
    Resource(cadmpeg_core::decode::ResourceLimit),
}

impl From<cadmpeg_core::CodecError> for FramingError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        match error {
            cadmpeg_core::CodecError::ResourceLimit(limit) => Self::Resource(limit),
            other => Self::unpositioned(other.to_string()),
        }
    }
}

impl FramingError {
    pub(crate) fn structural(offset: usize, message: impl Into<String>) -> Self {
        Self::Structural {
            offset,
            message: message.into(),
        }
    }

    pub(crate) fn unpositioned(message: impl Into<String>) -> Self {
        Self::Unpositioned {
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
            Self::Unpositioned { message } => write!(f, "framing error: {message}"),
            Self::Overflow { offset } => write!(f, "offset arithmetic overflow at {offset}"),
            Self::OutOfBounds { offset, end, bound } => {
                write!(f, "range {offset}..{end} exceeds bound {bound}")
            }
            Self::MissingEof => f.write_str("missing end-of-file chunk"),
            Self::Resource(limit) => cadmpeg_core::CodecError::ResourceLimit(*limit).fmt(f),
        }
    }
}

impl std::error::Error for FramingError {}

/// Parses the exact 32-byte file header.
pub(crate) fn parse_header(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Header, FramingError> {
    const MAX_HEADER_SEARCH: usize = 33_554_432 + MAGIC.len();
    let search_end = bytes.len().min(MAX_HEADER_SEARCH);
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(search_end),
        "Rhino header magic scan",
    )?;
    let start_offset = bytes[..search_end]
        .windows(MAGIC.len())
        .position(|window| window == MAGIC)
        .ok_or(FramingError::InvalidHeader)?;
    let header_end = start_offset
        .checked_add(file_header::LEN)
        .ok_or(FramingError::Overflow {
            offset: start_offset,
        })?;
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
        self.take(count).map(|_| ())
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

    /// Returns a bounded slice and advances the cursor.
    ///
    /// The read is the bound: the slice it answers is `count` bytes long, so
    /// no caller restates the length it already asked for.
    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8], FramingError> {
        let offset = self.view.position();
        match self.view.take(count) {
            Some(bytes) => Ok(bytes),
            None => Err(self.short_read(offset, count)),
        }
    }

    /// Reads a fixed-width byte array.
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], FramingError> {
        let offset = self.view.position();
        match self.view.array() {
            Some(bytes) => Ok(bytes),
            None => Err(self.short_read(offset, N)),
        }
    }

    /// The framing failure for a read of `count` bytes at `offset` that does
    /// not fit this reader's bound.
    fn short_read(&self, offset: usize, count: usize) -> FramingError {
        match offset.checked_add(count) {
            Some(end) => FramingError::OutOfBounds {
                offset,
                end,
                bound: self.view.end(),
            },
            None => FramingError::Overflow { offset },
        }
    }
}

/// The fixed-width little-endian readers, each answering the value its bound
/// proved.
macro_rules! bounded_readers {
    ($(($name:ident, $probe:ident, $ty:ty, $size:literal)),* $(,)?) => {
        impl BoundedReader<'_> {
            $(
                #[doc = concat!(
                    "Reads a little-endian `",
                    stringify!($ty),
                    "` from the bounded payload."
                )]
                pub(crate) fn $name(&mut self) -> Result<$ty, FramingError> {
                    let offset = self.view.position();
                    match self.view.$probe() {
                        Some(value) => Ok(value),
                        None => Err(self.short_read(offset, $size)),
                    }
                }
            )*
        }
    };
}

bounded_readers!(
    (u8, u8, u8, 1),
    (i16, i16_le, i16, 2),
    (u16, u16_le, u16, 2),
    (i32, i32_le, i32, 4),
    (u32, u32_le, u32, 4),
    (i64, i64_le, i64, 8),
    (u64, u64_le, u64, 8),
    (f32, f32_le, f32, 4),
    (f64, f64_le, f64, 8),
);

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
            value: i128::from(count),
        });
    }
    let count = usize::try_from(count).map_err(|_| FramingError::Overflow { offset })?;
    if count > allocation_limit {
        return Err(FramingError::InvalidLength {
            offset,
            value: i128::from(cadmpeg_core::decode::u64_from_index(count)),
        });
    }
    let bytes = count
        .checked_mul(element_size)
        .ok_or(FramingError::Overflow { offset })?;
    if bytes > remaining {
        let end = offset
            .checked_add(bytes)
            .ok_or(FramingError::Overflow { offset })?;
        let bound = offset
            .checked_add(remaining)
            .ok_or(FramingError::Overflow { offset })?;
        return Err(FramingError::OutOfBounds { offset, end, bound });
    }
    Ok(bytes)
}

/// Trailing checksum algorithm selected for a long chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChecksumKind {
    /// V1 CRC-CCITT checksum, stored in two bytes.
    Crc16,
    /// V2+ IEEE CRC32 checksum, stored in four bytes.
    Crc32,
}

impl ChecksumKind {
    fn width(self) -> usize {
        match self {
            Self::Crc16 => 2,
            Self::Crc32 => 4,
        }
    }
}

/// Selects the checksum algorithm without treating V1's CRC bit as CRC32.
fn checksum_kind(archive: ArchiveVersion, typecode: u32, class_uuid: bool) -> Option<ChecksumKind> {
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
enum ChunkBody {
    /// Short chunk: the inline value, and no payload bytes.
    Short { value: i64, end: usize },
    /// Long chunk: payload range and optional trailing checksum.
    Long {
        /// Body bytes excluding a trailing checksum.
        body: std::ops::Range<usize>,
        /// Trailing checksum algorithm, when selected.
        checksum: Option<ChecksumKind>,
    },
}

/// A parsed chunk header and all ranges derived from its declared boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// Offset of the chunk typecode.
    pub(crate) header_start: usize,
    /// Raw typecode.
    pub(crate) typecode: u32,
    /// Short value or long payload.
    form: ChunkBody,
}

impl Chunk {
    /// Returns the complete chunk range, including header and checksum.
    pub(crate) fn range(&self) -> std::ops::Range<usize> {
        self.header_start..self.next_offset()
    }

    /// Returns whether this chunk carries an inline short value.
    pub(crate) fn short(&self) -> bool {
        matches!(self.form, ChunkBody::Short { .. })
    }

    /// Returns the inline value of a short chunk.
    pub(crate) fn short_value(&self) -> Option<i64> {
        match self.form {
            ChunkBody::Short { value, .. } => Some(value),
            ChunkBody::Long { .. } => None,
        }
    }

    /// Returns the short value, or the declared long-body length.
    pub(crate) fn value(&self) -> Result<i64, FramingError> {
        match &self.form {
            ChunkBody::Short { value, .. } => Ok(*value),
            ChunkBody::Long { body, checksum } => {
                // chunk_at derives this span from a nonnegative i64 length.
                i64::try_from(body.len() + checksum.map_or(0, ChecksumKind::width)).map_err(|_| {
                    FramingError::structural(self.header_start, "chunk length exceeds i64")
                })
            }
        }
    }

    /// Returns the payload range. Short chunks have an empty range at the header end.
    pub(crate) fn body(&self) -> std::ops::Range<usize> {
        match &self.form {
            ChunkBody::Short { end, .. } => *end..*end,
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
            ChunkBody::Short { end, .. } => *end,
            ChunkBody::Long { body, checksum } => {
                body.end + checksum.map_or(0, ChecksumKind::width)
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
            form: ChunkBody::Short {
                value,
                end: body_start,
            },
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
            value: i128::from(value),
        });
    }
    let kind = checksum_kind(archive, typecode, class_uuid);
    let checksum_width = kind.map_or(0, ChecksumKind::width);
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
        form: ChunkBody::Long {
            body: body_start..body_end,
            checksum: kind,
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

/// Checks a chunk and records an integrity diagnostic for a checksum mismatch.
pub(crate) fn warn_checksum(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    label: &str,
    warnings: &mut crate::loss::Diagnostics,
) -> Result<(), FramingError> {
    if matches!(
        verify_checksum(ctx, data, chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::IntegrityFailure,
            format_args!("{label} CRC mismatch at offset {}", chunk.header_start),
        )?;
    }
    Ok(())
}

/// Verifies a parsed chunk's checksum without changing its recoverable boundary.
pub(crate) fn verify_checksum(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    chunk: &Chunk,
) -> Result<ChecksumStatus, FramingError> {
    let body = chunk.body();
    verify_checksum_ranges(ctx, bytes, chunk, std::iter::once(Ok(body)))
}

/// Verifies a chunk checksum over its direct byte ranges.
///
/// Container checksums exclude complete nested chunks. Callers pass the
/// ordered ranges written directly at the container's nesting level.
pub(crate) fn verify_checksum_ranges<I, R>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    chunk: &Chunk,
    ranges: I,
) -> Result<ChecksumStatus, FramingError>
where
    I: Clone + IntoIterator<Item = Result<R, FramingError>>,
    R: Borrow<std::ops::Range<usize>>,
{
    let ChunkBody::Long {
        body,
        checksum: Some(kind),
    } = &chunk.form
    else {
        return Ok(ChecksumStatus::NotPresent);
    };
    let checksum_end = body
        .end
        .checked_add(kind.width())
        .ok_or(FramingError::Overflow { offset: body.end })?;
    let checksum = body.end..checksum_end;
    let stored = bytes.get(checksum.clone()).ok_or(FramingError::Truncated {
        offset: body.end,
        needed: kind.width(),
    })?;
    for range in ranges.clone() {
        let range = range?;
        ctx.charge_work(1, "Rhino checksum range validation")?;
        let range = range.borrow();
        if range.start < body.start || range.end > body.end || range.start > range.end {
            return Err(FramingError::structural(
                body.start,
                "checksum range escapes chunk body",
            ));
        }
    }
    match kind {
        ChecksumKind::Crc16 => {
            let actual = u32::from(View::u16_le_at(stored, 0).ok_or(FramingError::Truncated {
                offset: checksum.start,
                needed: 2,
            })?);
            let mut crc = 1;
            for range in ranges {
                let range = range?;
                let range = range.borrow();
                let data = bytes.get(range.clone()).ok_or_else(|| {
                    FramingError::structural(range.start, "checksum range escapes input")
                })?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(data.len()),
                    "Rhino chunk checksum bytes",
                )?;
                crc = crc16(crc, data);
            }
            let expected = u32::from(crc);
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
                let range = range?;
                let range = range.borrow();
                let data = bytes.get(range.clone()).ok_or_else(|| {
                    FramingError::structural(range.start, "checksum range escapes input")
                })?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(data.len()),
                    "Rhino chunk checksum bytes",
                )?;
                hasher.update(data);
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

/// Ordered child ranges, with a live reservation for temporary storage.
enum ChecksumChildren<'a> {
    Borrowed(&'a [std::ops::Range<usize>]),
    Temporary {
        values: Vec<std::ops::Range<usize>>,
        _reservation: ScopedReservation<'a>,
    },
}

impl ChecksumChildren<'_> {
    fn as_slice(&self) -> &[std::ops::Range<usize>] {
        match self {
            Self::Borrowed(values) => values,
            Self::Temporary { values, .. } => values,
        }
    }
}

/// Validated parent-level checksum bytes with admitted child ordering.
pub(crate) struct DirectChecksumRanges<'a> {
    ctx: &'a DecodeContext<'a>,
    body: std::ops::Range<usize>,
    children: ChecksumChildren<'a>,
}

#[derive(Clone)]
pub(crate) struct DirectChecksumRangeIter<'a> {
    ctx: &'a DecodeContext<'a>,
    children: std::slice::Iter<'a, std::ops::Range<usize>>,
    cursor: usize,
    end: usize,
    complete: bool,
}

impl Iterator for DirectChecksumRangeIter<'_> {
    type Item = Result<std::ops::Range<usize>, FramingError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.complete {
            return None;
        }
        while !self.children.as_slice().is_empty() {
            if let Err(error) = self.ctx.charge_work(1, "Rhino checksum child traversal") {
                self.complete = true;
                return Some(Err(error.into()));
            }
            let child = self.children.next()?;
            let start = self.cursor;
            self.cursor = child.end;
            if start < child.start {
                return Some(Ok(start..child.start));
            }
        }
        self.complete = true;
        (self.cursor < self.end).then_some(Ok(self.cursor..self.end))
    }
}

impl<'a> IntoIterator for &'a DirectChecksumRanges<'_> {
    type Item = Result<std::ops::Range<usize>, FramingError>;
    type IntoIter = DirectChecksumRangeIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        DirectChecksumRangeIter {
            ctx: self.ctx,
            children: self.children.as_slice().iter(),
            cursor: self.body.start,
            end: self.body.end,
            complete: false,
        }
    }
}

/// Returns the parent-level byte ranges after complete child chunks are removed.
pub(crate) fn direct_checksum_ranges<'a>(
    ctx: &'a DecodeContext<'a>,
    body: &std::ops::Range<usize>,
    children: &'a [std::ops::Range<usize>],
) -> Result<DirectChecksumRanges<'a>, FramingError> {
    let mut pairs = children.windows(2);
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(pairs.len()),
        "Rhino checksum child order scan",
    )?;
    let children = if pairs.all(|pair| pair[0].start <= pair[1].start) {
        ChecksumChildren::Borrowed(children)
    } else {
        let mut values = Vec::new();
        let reservation = ctx
            .reserve_temporary_vec(
                &mut values,
                children.len(),
                "Rhino checksum child ordering copy",
            )
            .map_err(FramingError::Resource)?;
        ctx.charge_work_limit(
            cadmpeg_core::decode::u64_from_index(children.len()),
            "Rhino checksum child ordering copy",
        )
        .map_err(FramingError::Resource)?;
        values.extend_from_slice(children);
        ctx.stable_sort_by(
            &mut values,
            |value| &value.start,
            Ord::cmp,
            "Rhino checksum child ordering sort",
        )?;
        ChecksumChildren::Temporary {
            values,
            _reservation: reservation,
        }
    };
    let mut cursor = body.start;
    for child in children.as_slice() {
        ctx.charge_work(1, "Rhino checksum child validation")?;
        if child.start < cursor || child.end < child.start || child.end > body.end {
            return Err(FramingError::Structural {
                offset: child.start,
                message: "nested checksum range overlaps or escapes its parent".to_string(),
            });
        }
        cursor = child.end;
    }
    Ok(DirectChecksumRanges {
        ctx,
        body: body.clone(),
        children,
    })
}

/// Frames complete nested chunks through a short zero class-end marker.
pub(crate) fn checksum_children_through_class_end(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
        if children.len() >= CHECKSUM_CHILD_CAP {
            return Err(ctx
                .refuse_codec_limit(
                    "Rhino class-end checksum children",
                    cadmpeg_core::decode::u64_from_index(CHECKSUM_CHILD_CAP),
                    cadmpeg_core::decode::u64_from_index(CHECKSUM_CHILD_CAP + 1),
                )
                .into());
        }
        ctx.reserve_vec(&mut children, 1, "Rhino class-end checksum children")
            .map_err(crate::chunks::FramingError::from)?;
        children.push(child.range());
        reader.skip(child.next_offset() - start)?;
        if child.typecode == TCODE_CLASS_END {
            if !child.short() || child.value()? != 0 {
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
fn packed_version(value: u8) -> (i32, i32) {
    (i32::from(value >> 4), i32::from(value & 0x0f))
}

/// Decodes an anonymous little-endian `(i32 major, i32 minor)` version.
#[cfg(test)]
fn anonymous_version(reader: &mut BoundedReader<'_>) -> Result<(i32, i32), FramingError> {
    Ok((reader.i32()?, reader.i32()?))
}

/// Validates EOF framing for a complete input buffer.
pub(crate) fn validate_eof(
    bytes: &[u8],
    offset: usize,
    archive: ArchiveVersion,
) -> Result<(), FramingError> {
    if offset == bytes.len() && archive.allows_optional_eof() {
        return Ok(());
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
    Ok(())
}

#[cfg(test)]
mod direct_range_tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    use super::{
        direct_checksum_ranges, verify_checksum_ranges, ChecksumKind, ChecksumStatus, Chunk,
        ChunkBody, FramingError, TCODE_CRC,
    };

    #[test]
    fn direct_checksum_ranges_exclude_complete_sorted_children() {
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), false);
        let children = [30..40, 15..20];
        let direct = direct_checksum_ranges(&ctx, &(10..50), &children).expect("valid nesting");
        assert_eq!(
            (&direct)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .expect("admitted traversal"),
            vec![10..15, 20..30, 40..50]
        );
    }

    #[test]
    fn direct_checksum_ranges_reject_overlap() {
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), false);
        assert!(matches!(
            direct_checksum_ranges(&ctx, &(10..50), &[15..30, 20..40]),
            Err(FramingError::Structural { .. })
        ));
    }

    #[test]
    fn direct_checksum_ranges_order_scan_refuses_work_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let ctx = DecodeContext::new(&arena, &policy, false);
        assert!(matches!(
            direct_checksum_ranges(&ctx, &(10..50), &[15..20, 30..40]),
            Err(FramingError::Resource(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "Rhino checksum child order scan"
                    && Some(limit) == ctx.resource_refusal()
        ));
    }

    #[test]
    fn direct_checksum_ranges_validation_refuses_work_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The one adjacent comparison fits; the first validation step does not.
        policy.limits.max_work_units = 1;
        let ctx = DecodeContext::new(&arena, &policy, false);
        assert!(matches!(
            direct_checksum_ranges(&ctx, &(10..50), &[15..20, 30..40]),
            Err(FramingError::Resource(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "Rhino checksum child validation"
                    && Some(limit) == ctx.resource_refusal()
        ));
    }

    #[test]
    fn direct_checksum_ranges_unsorted_sort_refuses_work_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Admit the order comparison, two copied ranges, and the core key scan.
        policy.limits.max_work_units = 1 + 2 + 2;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let children = [30..40, 15..20];
        assert!(matches!(
            direct_checksum_ranges(&ctx, &(10..50), &children),
            Err(FramingError::Resource(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "Rhino checksum child ordering sort"
                    && Some(limit) == ctx.resource_refusal()
        ));
        assert_eq!(children, [30..40, 15..20]);
    }

    #[test]
    fn direct_checksum_ranges_unsorted_copy_refuses_work_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let ctx = DecodeContext::new(&arena, &policy, false);
        assert!(matches!(
            direct_checksum_ranges(&ctx, &(10..50), &[30..40, 15..20]),
            Err(FramingError::Resource(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "Rhino checksum child ordering copy"
                    && Some(limit) == ctx.resource_refusal()
        ));
    }

    #[test]
    fn direct_checksum_ranges_unsorted_storage_refuses_materialized_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let ctx = DecodeContext::new(&arena, &policy, false);
        assert!(matches!(
            direct_checksum_ranges(&ctx, &(10..50), &[30..40, 15..20]),
            Err(FramingError::Resource(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && Some(limit) == ctx.resource_refusal()
        ));
    }

    #[test]
    fn direct_checksum_ranges_unsorted_storage_refuses_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let ctx = DecodeContext::new(&arena, &policy, false);
        assert!(matches!(
            direct_checksum_ranges(&ctx, &(10..50), &[30..40, 15..20]),
            Err(FramingError::Resource(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && Some(limit) == ctx.resource_refusal()
        ));
    }

    #[test]
    fn direct_checksum_ranges_sorted_children_need_no_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let children = [15..20, 30..40];
        let direct = direct_checksum_ranges(&ctx, &(10..50), &children).expect("borrowed children");
        assert_eq!(
            (&direct)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .expect("admitted traversal"),
            vec![10..15, 20..30, 40..50]
        );
    }

    #[test]
    fn direct_checksum_ranges_unsorted_equal_starts_keep_input_order() {
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), false);
        let children = [30..40, 15..15, 15..20];
        let direct = direct_checksum_ranges(&ctx, &(10..50), &children).expect("valid empty child");
        assert_eq!(
            (&direct)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .expect("admitted traversal"),
            vec![10..15, 20..30, 40..50]
        );
    }

    #[test]
    fn direct_checksum_ranges_unsorted_storage_is_released() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let storage =
            cadmpeg_core::decode::u64_from_index(2 * std::mem::size_of::<std::ops::Range<usize>>());
        policy.limits.max_materialized_bytes = storage;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 2;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let children = [30..40, 15..20];
        let direct = direct_checksum_ranges(&ctx, &(10..50), &children).expect("scoped ordering");
        assert_eq!(
            (&direct)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .expect("admitted traversal"),
            vec![10..15, 20..30, 40..50]
        );
        drop(direct);
        assert!(ctx
            .reserve_scoped(storage, "reuse checksum ordering storage")
            .is_ok());
    }

    #[test]
    fn direct_checksum_ranges_child_walk_refuses_instead_of_quiet_none() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two order comparisons and three validation steps exhaust construction work.
        policy.limits.max_work_units = 2 + 3;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let children = [10..15, 15..20, 20..25];
        let direct =
            direct_checksum_ranges(&ctx, &(10..25), &children).expect("admitted construction");
        let mut iter = (&direct).into_iter();
        assert!(
            matches!(iter.next(), Some(Err(FramingError::Resource(limit)))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "Rhino checksum child traversal"
                && Some(limit) == ctx.resource_refusal())
        );
        assert!(iter.next().is_none());
    }

    #[test]
    fn direct_checksum_ranges_child_walk_is_linear() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Each of the three children needs one traversal step after construction.
        policy.limits.max_work_units = 2 + 3 + 3;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let children = [10..15, 15..20, 20..25];
        let direct =
            direct_checksum_ranges(&ctx, &(10..25), &children).expect("admitted construction");
        assert_eq!((&direct).into_iter().next(), None);
        assert!(matches!(ctx.charge_work(1, "after checksum traversal"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.used == 8));
    }

    fn assert_verification_walk_refusal(work: u64, kind: ChecksumKind) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let children = [0..4, 4..8];
        let direct =
            direct_checksum_ranges(&ctx, &(0..8), &children).expect("admitted construction");
        let chunk = Chunk {
            header_start: 0,
            typecode: TCODE_CRC,
            form: ChunkBody::Long {
                body: 0..8,
                checksum: Some(kind),
            },
        };
        let mut bytes = vec![0; 8];
        match kind {
            ChecksumKind::Crc16 => bytes.extend(1u16.to_le_bytes()),
            ChecksumKind::Crc32 => bytes.extend(crc32fast::hash(&[]).to_le_bytes()),
        }
        assert!(
            matches!(verify_checksum_ranges(&ctx, &bytes, &chunk, &direct),
            Err(FramingError::Resource(limit))
                if limit.operation == "Rhino checksum child traversal"
                    && limit.used == work && Some(limit) == ctx.resource_refusal())
        );
    }

    #[test]
    fn direct_checksum_ranges_verify_propagates_validation_walk_refusal() {
        // Construction takes three units; validation must charge its own child walk.
        assert_verification_walk_refusal(3, ChecksumKind::Crc32);
    }

    #[test]
    fn direct_checksum_ranges_verify_propagates_crc32_walk_refusal() {
        // Validation consumes two more units; hashing must charge a fresh child walk.
        assert_verification_walk_refusal(5, ChecksumKind::Crc32);
    }

    #[test]
    fn direct_checksum_ranges_verify_propagates_crc16_walk_refusal() {
        assert_verification_walk_refusal(5, ChecksumKind::Crc16);
    }

    #[test]
    fn direct_checksum_ranges_verify_preserves_valid_checksum() {
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), false);
        let children = [2..4, 6..8];
        let direct = direct_checksum_ranges(&ctx, &(0..10), &children).expect("valid children");
        let chunk = Chunk {
            header_start: 0,
            typecode: TCODE_CRC,
            form: ChunkBody::Long {
                body: 0..10,
                checksum: Some(ChecksumKind::Crc32),
            },
        };
        let mut bytes: Vec<u8> = (0..10).collect();
        bytes.extend(crc32fast::hash(&[0, 1, 4, 5, 8, 9]).to_le_bytes());
        assert_eq!(
            verify_checksum_ranges(&ctx, &bytes, &chunk, &direct),
            Ok(ChecksumStatus::Valid)
        );
    }
}

#[cfg(test)]
mod tests;
