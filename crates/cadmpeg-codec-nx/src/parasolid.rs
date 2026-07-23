// SPDX-License-Identifier: Apache-2.0
//! Extract and classify compressed streams in an NX part payload.
//!
//! [`extract_streams`] scans the canonical `/Root/UG_PART/UG_PART` file span for
//! valid zlib headers. An inflated `PS 00 00` prologue identifies Parasolid
//! neutral-binary data and supplies its subtype and optional `SCH_` schema token.
//! Other inflated payloads are classified as [`StreamKind::Preview`].
#![deny(clippy::disallowed_methods)]

use std::io::Read;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_ir::parasolid::{has_prologue, schema_token};
use flate2::read::ZlibDecoder;

use crate::container::Container;

/// Classification of an inflated payload in the part stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// A Parasolid `(partition)` body snapshot.
    Partition,
    /// A Parasolid `(deltas)` edit overlay.
    Deltas,
    /// A cached Parasolid body without a partition or deltas subtype.
    Plain,
    /// An inflated non-Parasolid payload, such as preview or metadata data.
    Preview,
}

impl StreamKind {
    /// Return the stable label used in summaries and reports.
    pub fn label(self) -> &'static str {
        match self {
            StreamKind::Partition => "partition",
            StreamKind::Deltas => "deltas",
            StreamKind::Plain => "plain",
            StreamKind::Preview => "preview",
        }
    }

    /// Return whether this kind contains Parasolid neutral-binary records.
    pub fn is_parasolid(self) -> bool {
        !matches!(self, StreamKind::Preview)
    }
}

/// A located and inflated stream from the canonical part payload.
#[derive(Debug, Clone)]
pub struct Stream {
    /// Byte offset of the `78 01` zlib header in the source file.
    pub file_offset: usize,
    /// Compressed input bytes the decoder consumed at `file_offset`.
    ///
    /// The physical extent `[file_offset, file_offset + consumed)` in the source.
    pub consumed: u64,
    /// Inflated bytes.
    pub inflated: Vec<u8>,
    /// Payload classification.
    pub kind: StreamKind,
    /// The Parasolid `SCH_<version>` token, when present.
    pub schema: Option<String>,
}

/// One Parasolid attribute-class declaration preceding its field record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeDefinition<'a> {
    /// Inflated-stream offset of the `00 4f` tag.
    pub offset: usize,
    /// Stream-local definition record identity.
    pub xmt: u16,
    /// Exact printable class name.
    pub name: &'a str,
    /// Declared number of fields in the following `00 50` record.
    pub field_count: u32,
    /// Stream-local identity of the field record.
    pub field_record_xmt: u16,
    /// Ordered catalog references in the field-record header.
    pub field_record_references: [u16; 2],
    /// Two field-record header words following the catalog references.
    pub field_record_header_words: [u16; 2],
    /// Exact 26-byte descriptor prefix following the field-record header.
    pub field_descriptor_prefix: [u8; 26],
    /// One serialized field code for every declared field.
    pub field_codes: &'a [u8],
}

/// One framed type-81 Parasolid entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity51Record {
    /// Inflated-stream offset of the `00 51` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Record flags preceding the identity.
    pub flags: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Serialized sequence value.
    pub sequence: u32,
    /// Layout discriminator selecting the reference count.
    pub discriminator: u16,
    /// Ordered stream-local references.
    pub references: Vec<u32>,
}

/// One self-framed printable type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity54StringRecord<'a> {
    /// Inflated-stream offset of the `00 54` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Nonempty printable string value.
    pub value: &'a str,
}

/// One counted type-82 unsigned-integer value record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity52IntegerRecord {
    /// Inflated-stream offset of the `00 52` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered big-endian unsigned values.
    pub values: Vec<u32>,
}

/// One counted type-83 binary64 value record.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity53DoubleRecord {
    /// Inflated-stream offset of the `00 53` tag.
    pub offset: usize,
    /// Exact framed record length.
    pub byte_len: usize,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite big-endian binary64 values.
    pub values: Vec<f64>,
}

/// Decode counted type-82 unsigned-integer records.
pub fn entity_52_integer_records(bytes: &[u8]) -> Vec<Entity52IntegerRecord> {
    counted_value_records(bytes, 0x52, 4, |value| {
        Some(u32::from_be_bytes(value.try_into().ok()?))
    })
    .into_iter()
    .map(|record| Entity52IntegerRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values: record.values,
    })
    .collect()
}

/// Decode counted type-83 finite binary64 records.
pub fn entity_53_double_records(bytes: &[u8]) -> Vec<Entity53DoubleRecord> {
    counted_value_records(bytes, 0x53, 8, |value| {
        let value = f64::from_be_bytes(value.try_into().ok()?);
        value.is_finite().then_some(value)
    })
    .into_iter()
    .map(|record| Entity53DoubleRecord {
        offset: record.offset,
        byte_len: record.byte_len,
        xmt: record.xmt,
        values: record.values,
    })
    .collect()
}

struct CountedValueRecord<T> {
    offset: usize,
    byte_len: usize,
    xmt: u32,
    values: Vec<T>,
}

fn counted_value_records<T>(
    bytes: &[u8],
    tag: u8,
    value_width: usize,
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Vec<CountedValueRecord<T>> {
    let mut records = Vec::new();
    for offset in 0..bytes.len().saturating_sub(10) {
        if bytes.get(offset..offset + 2) != Some(&[0, tag]) {
            continue;
        }
        let mut at = offset + 2;
        if bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let Some(count) = bytes
            .get(at..at + 4)
            .map(|value| u32::from_be_bytes(value.try_into().expect("four bytes")) as usize)
            .filter(|count| *count > 0)
        else {
            continue;
        };
        at += 4;
        let Some(xmt) = read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1) else {
            continue;
        };
        let Some(values_end) = count
            .checked_mul(value_width)
            .and_then(|length| at.checked_add(length))
        else {
            continue;
        };
        let Some(value_bytes) = bytes.get(at..values_end) else {
            continue;
        };
        let Some(values) = value_bytes
            .chunks_exact(value_width)
            .map(&decode)
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        records.push(CountedValueRecord {
            offset,
            byte_len: values_end - offset,
            xmt,
            values,
        });
    }
    records
}

/// Decode self-framed printable type-84 string records.
pub fn entity_54_string_records(bytes: &[u8]) -> Vec<Entity54StringRecord<'_>> {
    let mut records = Vec::new();
    for offset in 0..bytes.len().saturating_sub(10) {
        if bytes.get(offset..offset + 2) != Some(&[0x00, 0x54]) {
            continue;
        }
        let mut at = offset + 2;
        if bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let Some(length) = bytes
            .get(at..at + 4)
            .map(|value| u32::from_be_bytes(value.try_into().expect("four bytes")) as usize)
            .filter(|length| *length > 0)
        else {
            continue;
        };
        at += 4;
        let Some(xmt) = read_xmt(bytes, &mut at).filter(|xmt| *xmt > 1) else {
            continue;
        };
        let Some(end) = at.checked_add(length) else {
            continue;
        };
        let Some(value) = bytes.get(at..end).filter(|value| {
            value
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        }) else {
            continue;
        };
        if bytes.get(end) != Some(&0) {
            continue;
        }
        let Ok(value) = std::str::from_utf8(value) else {
            continue;
        };
        records.push(Entity54StringRecord {
            offset,
            byte_len: end + 1 - offset,
            xmt,
            value,
        });
    }
    records
}

/// Decode framed type-81 entity/attribute-list records.
pub fn entity_51_records(bytes: &[u8]) -> Vec<Entity51Record> {
    let mut records = Vec::new();
    for offset in 0..bytes.len().saturating_sub(25) {
        if bytes.get(offset..offset + 2) != Some(&[0x00, 0x51]) {
            continue;
        }
        let mut at = offset + 2;
        if bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let Some(flags) = bytes
            .get(at..at + 4)
            .map(|value| u32::from_be_bytes(value.try_into().expect("four bytes")))
        else {
            continue;
        };
        at += 4;
        let Some(xmt) = read_xmt(bytes, &mut at) else {
            continue;
        };
        let Some(sequence) = bytes
            .get(at..at + 4)
            .map(|value| u32::from_be_bytes(value.try_into().expect("four bytes")))
        else {
            continue;
        };
        at += 4;
        let Some(discriminator) = bytes
            .get(at..at + 2)
            .map(|value| u16::from_be_bytes(value.try_into().expect("two bytes")))
        else {
            continue;
        };
        at += 2;
        let low_flag = (flags & 0xff) as u8;
        if xmt <= 1 || sequence == 0 || !(1..=0x20).contains(&low_flag) {
            continue;
        }
        let reference_count = match (discriminator, low_flag) {
            (0x0018 | 0x0020 | 0x0025, 1) => 6,
            (0x001d | 0x001e, 2) => 7,
            (0x0020 | 0x0024 | 0x0027, 4) => 9,
            _ => 6,
        };
        let Some(references) = entity_51_references(bytes, &mut at, reference_count) else {
            continue;
        };
        records.push(Entity51Record {
            offset,
            byte_len: at - offset,
            flags,
            xmt,
            sequence,
            discriminator,
            references,
        });
    }
    records
}

fn entity_51_references(bytes: &[u8], at: &mut usize, count: usize) -> Option<Vec<u32>> {
    let start = *at;
    if bytes.get(*at) == Some(&1) {
        let mut prefixed_at = *at;
        let mut references = Vec::new();
        for _ in 0..count {
            if bytes.get(prefixed_at) != Some(&1) {
                references.clear();
                break;
            }
            prefixed_at += 1;
            references.push(read_xmt(bytes, &mut prefixed_at)?);
        }
        if references.len() == count && bytes.get(prefixed_at) == Some(&0) {
            *at = prefixed_at + 1;
            return Some(references);
        }
    }
    *at = start;
    (0..count).map(|_| read_xmt(bytes, at)).collect()
}

fn read_xmt(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let first = i16::from_be_bytes([*bytes.get(*at)?, *bytes.get(*at + 1)?]);
    *at += 2;
    if first >= 0 {
        return Some(first as u32);
    }
    let quotient = u16::from_be_bytes([*bytes.get(*at)?, *bytes.get(*at + 1)?]);
    *at += 2;
    Some(u32::from(quotient) * 32_767 + u32::from(first.unsigned_abs()))
}

/// Decode length-bounded `00 4f` attribute-class declarations.
pub fn attribute_definitions(bytes: &[u8]) -> Vec<AttributeDefinition<'_>> {
    let mut definitions = Vec::new();
    let mut at = 0;
    while at + 9 <= bytes.len() {
        if bytes.get(at..at + 2) != Some(&[0x00, 0x4f]) {
            at += 1;
            continue;
        }
        let escaped = bytes.get(at + 2) == Some(&0xff);
        let header = at + 2 + usize::from(escaped);
        let Some(length_bytes) = bytes.get(header..header + 4) else {
            break;
        };
        let name_len = u32::from_be_bytes(length_bytes.try_into().expect("four bytes")) as usize;
        let Some(identity) = bytes.get(header + 4..header + 6) else {
            break;
        };
        let name_start = header + 6;
        let Some(name_end) = name_start.checked_add(name_len) else {
            at += 1;
            continue;
        };
        let Some(name_bytes) = bytes.get(name_start..name_end) else {
            at += 1;
            continue;
        };
        let Some(field_header) = bytes.get(name_end..name_end + 16) else {
            at += 1;
            continue;
        };
        if name_len == 0
            || !name_bytes
                .iter()
                .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_control())
            || field_header.get(0..2) != Some(&[0x00, 0x50])
        {
            at += 1;
            continue;
        }
        let Ok(name) = std::str::from_utf8(name_bytes) else {
            at += 1;
            continue;
        };
        let field_count = u32::from_be_bytes(field_header[2..6].try_into().expect("four bytes"));
        let descriptor_start = name_end + 16;
        let Some(descriptor_end) = descriptor_start.checked_add(26) else {
            at += 1;
            continue;
        };
        let Some(field_codes_end) = descriptor_end.checked_add(field_count as usize) else {
            at += 1;
            continue;
        };
        let Some(field_descriptor_prefix) = bytes
            .get(descriptor_start..descriptor_end)
            .and_then(|value| value.try_into().ok())
        else {
            at += 1;
            continue;
        };
        let Some(field_codes) = bytes.get(descriptor_end..field_codes_end) else {
            at += 1;
            continue;
        };
        definitions.push(AttributeDefinition {
            offset: at,
            xmt: u16::from_be_bytes([identity[0], identity[1]]),
            name,
            field_count,
            field_record_xmt: u16::from_be_bytes(field_header[6..8].try_into().expect("two bytes")),
            field_record_references: [
                u16::from_be_bytes(field_header[8..10].try_into().expect("two bytes")),
                u16::from_be_bytes(field_header[10..12].try_into().expect("two bytes")),
            ],
            field_record_header_words: [
                u16::from_be_bytes(field_header[12..14].try_into().expect("two bytes")),
                u16::from_be_bytes(field_header[14..16].try_into().expect("two bytes")),
            ],
            field_descriptor_prefix,
            field_codes,
        });
        at = field_codes_end;
    }
    definitions
}

/// The minimum inflated length for a candidate to count as a real stream; below
/// this a `78 01` match is almost certainly a coincidence in packed data.
const MIN_INFLATED: usize = 64;

/// Chunk size for streaming inflated output through the expander.
const INFLATE_CHUNK: usize = 8192;

/// Locates, inflates, and classifies zlib streams in `/Root/UG_PART/UG_PART`.
pub fn extract_streams<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    container: &Container,
) -> Result<Vec<Stream>, CodecError> {
    let Some((part_offset, part_size)) = container
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/UG_PART/UG_PART")
        .and_then(|entry| entry.file_span)
    else {
        return Ok(Vec::new());
    };
    let (Ok(start), Ok(size)) = (usize::try_from(part_offset), usize::try_from(part_size)) else {
        return Ok(Vec::new());
    };
    let Some(end) = start.checked_add(size) else {
        return Ok(Vec::new());
    };
    let part_view = ctx.register_slice(
        root,
        ByteRange {
            start: start as u64,
            end: end as u64,
        },
    )?;
    let part = part_view.window();

    let mut streams = Vec::new();
    let mut i = 0usize;
    while i + 2 <= part.len() {
        if is_zlib_header(part[i], part[i + 1]) {
            if let Some((inflated, consumed)) = inflate_stream(ctx, part_view, i)? {
                let (kind, schema) = classify(&inflated);
                streams.push(Stream {
                    file_offset: start + i,
                    consumed,
                    inflated,
                    kind,
                    schema,
                });
                // Resume past the bytes this member consumed, not at the next
                // byte: a spurious `78 xx` zlib header inside the compressed
                // body would otherwise inflate into a second stream whose source
                // extent [file_offset, file_offset+consumed) overlaps this
                // member's, double-attributing the same compressed bytes to two
                // decompression origins. Skipping the consumed run keeps packed
                // members' input extents disjoint.
                i = i.saturating_add((consumed as usize).max(2));
                continue;
            }
        }
        i += 1;
    }
    Ok(streams)
}

/// Inflates one zlib member that meets [`MIN_INFLATED`].
fn inflate_stream<'a>(
    ctx: &DecodeContext<'a>,
    part_view: View<'a>,
    offset: usize,
) -> Result<Option<(Vec<u8>, u64)>, CodecError> {
    let Some(source) = part_view.child(offset, part_view.end()) else {
        return Ok(None);
    };
    let mut decoder = ZlibDecoder::new(source.window());
    let mut writer = ctx.begin_expand(source, ExpandSpec::Unknown)?;
    let mut inflated = Vec::new();
    let mut chunk = [0u8; INFLATE_CHUNK];
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                writer.write(&chunk[..read])?;
                inflated.extend_from_slice(&chunk[..read]);
            }
            Err(_) => break,
        }
    }
    if inflated.len() < MIN_INFLATED {
        return Ok(None);
    }
    let consumed = decoder.total_in();
    writer.finalize()?;
    Ok(Some((inflated, consumed)))
}

/// A zlib header has compression method 8 and a 16-bit header divisible by
/// 31. NX uses the standard `78 01`, `78 9c`, and `78 da` variants, but the
/// predicate accepts every standards-conforming FLG byte rather than treating a
/// compression level as a format discriminator.
#[allow(clippy::manual_is_multiple_of)] // `is_multiple_of` exceeds the workspace MSRV.
fn is_zlib_header(cmf: u8, flg: u8) -> bool {
    cmf & 0x0f == 8 && cmf >> 4 <= 7 && u16::from_be_bytes([cmf, flg]).is_multiple_of(31)
}

/// Classify an inflated payload from its prologue text and read the schema token.
///
/// The `PS\0\0` prologue test and the `SCH_` schema read are the shared platform
/// primitives [`cadmpeg_ir::parasolid::has_prologue`] and
/// [`cadmpeg_ir::parasolid::schema_token`]; the subtype split
/// (`(partition)`/`(deltas)`) is nx-specific and stays here.
fn classify(inflated: &[u8]) -> (StreamKind, Option<String>) {
    if !has_prologue(inflated) {
        return (StreamKind::Preview, None);
    }
    let window = &inflated[..inflated.len().min(512)];
    let kind = if contains(window, b"(partition)") {
        StreamKind::Partition
    } else if contains(window, b"(deltas)") {
        StreamKind::Deltas
    } else {
        StreamKind::Plain
    };
    (kind, schema_token(inflated))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
