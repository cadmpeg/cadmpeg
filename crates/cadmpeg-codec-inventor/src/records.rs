// SPDX-License-Identifier: Apache-2.0
//! Exact Inventor `RSe` metadata tables and bulk-record framing.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::meta_body_prefix as meta_prefix;
use crate::layout::meta_type_descriptor as type_desc;

const SECTION_COUNT: usize = 11;
const TERMINAL_ID_LEN: usize = 16;
const SECTION_11_PAYLOAD_LEN: usize = 0x48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockDescriptor {
    pub(crate) ordinal: u32,
    pub(crate) stored: bool,
    pub(crate) payload_len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TypeDescriptor {
    pub(crate) index: u8,
    pub(crate) id: [u8; 16],
    pub(crate) fields: [(u16, u32); 2],
}

/// An `RSe` metadata section number from 1 through 11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum MetaSectionNumber {
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Eleven = 11,
}

impl TryFrom<u8> for MetaSectionNumber {
    type Error = String;

    fn try_from(number: u8) -> Result<Self, Self::Error> {
        match number {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            4 => Ok(Self::Four),
            5 => Ok(Self::Five),
            6 => Ok(Self::Six),
            7 => Ok(Self::Seven),
            8 => Ok(Self::Eight),
            9 => Ok(Self::Nine),
            10 => Ok(Self::Ten),
            11 => Ok(Self::Eleven),
            _ => Err(format!(
                "metadata section number {number} is outside 1..=11"
            )),
        }
    }
}

impl From<MetaSectionNumber> for u8 {
    fn from(number: MetaSectionNumber) -> Self {
        number as Self
    }
}

impl std::fmt::Display for MetaSectionNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        u8::from(*self).fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReverseSectionNumber {
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Eleven,
}

impl From<ReverseSectionNumber> for MetaSectionNumber {
    fn from(number: ReverseSectionNumber) -> Self {
        match number {
            ReverseSectionNumber::Five => Self::Five,
            ReverseSectionNumber::Six => Self::Six,
            ReverseSectionNumber::Seven => Self::Seven,
            ReverseSectionNumber::Eight => Self::Eight,
            ReverseSectionNumber::Nine => Self::Nine,
            ReverseSectionNumber::Ten => Self::Ten,
            ReverseSectionNumber::Eleven => Self::Eleven,
        }
    }
}

impl std::fmt::Display for ReverseSectionNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        MetaSectionNumber::from(*self).fmt(f)
    }
}

#[derive(Debug)]
pub(crate) struct MetaSection<'a> {
    pub(crate) number: MetaSectionNumber,
    pub(crate) discriminator: u32,
    pub(crate) payload: View<'a>,
}

#[derive(Debug)]
pub(crate) struct MetaTables<'a> {
    pub(crate) prefix: [u16; 7],
    pub(crate) blocks: Vec<BlockDescriptor>,
    pub(crate) types: Vec<TypeDescriptor>,
    pub(crate) sections: [MetaSection<'a>; SECTION_COUNT],
    pub(crate) terminal_id: [u8; 16],
}

#[derive(Debug)]
pub(crate) struct RseRecordFrame<'a> {
    pub(crate) ordinal: u32,
    pub(crate) selector: u32,
    pub(crate) type_id: [u8; 16],
    pub(crate) payload_offset: u64,
    pub(crate) payload: View<'a>,
    pub(crate) trailing_length_written: bool,
    pub(crate) trailer: View<'a>,
}

impl RseRecordFrame<'_> {
    pub(crate) fn type_index(&self) -> u8 {
        self.selector as u8
    }
    pub(crate) fn payload_len(&self) -> u32 {
        self.payload.window().len() as u32
    }
    pub(crate) fn trailing_payload_len(&self) -> u32 {
        if self.trailing_length_written {
            self.payload_len()
        } else {
            0
        }
    }
}

#[derive(Debug)]
pub(crate) struct RseRecordTable<'a> {
    pub(crate) records: Vec<RseRecordFrame<'a>>,
    pub(crate) stream_trailer: View<'a>,
}

pub(crate) fn parse_meta_tables<'a>(
    ctx: &DecodeContext<'a>,
    body: View<'a>,
) -> Result<MetaTables<'a>, CodecError> {
    let mut view = body;
    let prefix =
        crate::reader::u16_array::<{ meta_prefix::LEN / 2 }>(&mut view, "metadata prefix")?;

    // The terminal id is the last 16 bytes of the body and bounds the reverse
    // section walk, so it is read before the forward sections claim the span
    // between them.
    let terminal_position = body
        .end()
        .checked_sub(TERMINAL_ID_LEN)
        .ok_or_else(|| CodecError::truncated(body.location(), "metadata terminal id"))?;
    let mut terminal = crate::reader::at(body, terminal_position, "metadata terminal id")?;
    let terminal_start = terminal.read_len();
    let terminal_id =
        crate::reader::array::<TERMINAL_ID_LEN>(&mut terminal, "metadata terminal id")?;

    let (block_count, section_1_payload, section_1_footer) =
        counted_section(&mut view, 4, "block-size table")?;
    ctx.charge_collection_items(block_count as u64, "admit Inventor RSe block descriptors")?;
    let mut blocks = Vec::with_capacity(block_count);
    let mut sizes = section_1_payload;
    for ordinal in 0..block_count {
        let encoded = crate::reader::u32(&mut sizes, "block-size entry")?;
        blocks.push(BlockDescriptor {
            ordinal: ordinal as u32,
            stored: encoded & 0x8000_0000 != 0,
            payload_len: encoded & 0x7fff_ffff,
        });
    }
    let section_1 = MetaSection {
        number: MetaSectionNumber::One,
        discriminator: block_count as u32,
        payload: section_1_payload,
    };

    let (section_2_count, section_2_payload, _) = counted_section(&mut view, 10, "section 2")?;
    let section_2 = MetaSection {
        number: MetaSectionNumber::Two,
        discriminator: section_2_count as u32,
        payload: section_2_payload,
    };
    let (section_3_count, section_3_payload, _) = counted_section(&mut view, 28, "section 3")?;
    let section_3 = MetaSection {
        number: MetaSectionNumber::Three,
        discriminator: section_3_count as u32,
        payload: section_3_payload,
    };
    let (type_count, section_4_payload, section_4_footer) =
        counted_section(&mut view, type_desc::LEN, "type table")?;
    if type_count > 256 {
        return Err(CodecError::Malformed(
            "RSe type table has more than 256 entries".into(),
        ));
    }
    // The test above bounds `type_count` at 256 and `SECTION_COUNT` is 11, so
    // the charge is at most 267 and `u64` holds it exactly.
    ctx.charge_collection_items(
        type_count as u64 + SECTION_COUNT as u64,
        "admit Inventor RSe metadata tables",
    )?;
    let mut types = Vec::with_capacity(type_count);
    for index in 0..type_count {
        let entry = child(
            section_4_payload,
            index * type_desc::LEN,
            (index + 1) * type_desc::LEN,
            "type descriptor",
        )?;
        let mut entry = crate::pmdc::Cursor::new(entry);
        types.push(TypeDescriptor {
            index: index as u8,
            id: entry.take_array("type descriptor id")?,
            fields: [
                (
                    entry.u16("type descriptor field 0 key")?,
                    entry.u32("type descriptor field 0 value")?,
                ),
                (
                    entry.u16("type descriptor field 1 key")?,
                    entry.u32("type descriptor field 1 value")?,
                ),
            ],
        });
    }

    let section_4 = MetaSection {
        number: MetaSectionNumber::Four,
        discriminator: type_count as u32,
        payload: section_4_payload,
    };

    let mut end = terminal_start;
    let mut payload_len = SECTION_11_PAYLOAD_LEN;
    let mut reverse = |number| reverse_section(body, number, &mut end, &mut payload_len);
    let section_11 = reverse(ReverseSectionNumber::Eleven)?;
    let section_10 = reverse(ReverseSectionNumber::Ten)?;
    let section_9 = reverse(ReverseSectionNumber::Nine)?;
    let section_8 = reverse(ReverseSectionNumber::Eight)?;
    let section_7 = reverse(ReverseSectionNumber::Seven)?;
    let section_6 = reverse(ReverseSectionNumber::Six)?;
    let section_5 = reverse(ReverseSectionNumber::Five)?;
    if end != section_4_footer {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata section chain ends at {end}, expected {section_4_footer}"
        )));
    }
    let sections = [
        section_1, section_2, section_3, section_4, section_5, section_6, section_7, section_8,
        section_9, section_10, section_11,
    ];
    let section_1_end = meta_prefix::LEN + 4 + block_count * 4;
    if section_1_footer != section_1_end {
        return Err(CodecError::malformed(format_args!(
            "RSe block-size table ends at {section_1_footer}, expected {section_1_end}"
        )));
    }
    Ok(MetaTables {
        prefix,
        blocks,
        types,
        sections,
        terminal_id,
    })
}

pub(crate) fn frame_bulk_records<'a>(
    ctx: &DecodeContext<'a>,
    bulk: View<'a>,
    tables: &MetaTables<'_>,
    segment_version_major: u8,
) -> Result<RseRecordTable<'a>, CodecError> {
    let stored_count = tables.blocks.iter().filter(|block| block.stored).count();
    ctx.charge_collection_items(stored_count as u64, "admit Inventor RSe record frames")?;
    let mut cursor = Cursor::new(bulk);
    let mut records = Vec::with_capacity(stored_count);
    for block in tables.blocks.iter().filter(|block| block.stored) {
        let selector = cursor.u32("record type selector")?;
        let type_index = selector as u8;
        let descriptor = tables.types.get(type_index as usize).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "RSe record {} selects absent type index {type_index}",
                block.ordinal
            ))
        })?;
        let payload_offset = cursor.position() as u64;
        let payload = cursor.view(block.payload_len as usize, "record payload")?;
        let trailing_payload_len = cursor.u32("record trailing payload length")?;
        if trailing_payload_len != 0 && trailing_payload_len != block.payload_len {
            return Err(CodecError::malformed(format_args!(
                "RSe record {} declares payload length {} but trails with {trailing_payload_len}",
                block.ordinal, block.payload_len
            )));
        }
        let trailer_start = cursor.position();
        if uses_extended_record_trailer(segment_version_major) {
            parse_extended_record_trailer(ctx, &mut cursor)?;
        }
        let trailer = child(bulk, trailer_start, cursor.position(), "record trailer")?;
        records.push(RseRecordFrame {
            ordinal: block.ordinal,
            selector,
            type_id: descriptor.id,
            payload_offset,
            payload,
            trailing_length_written: trailing_payload_len != 0,
            trailer,
        });
    }
    let trailer_start = cursor.position();
    let trailer_marker = cursor.u32("stream trailer marker")?;
    if trailer_marker != u32::MAX {
        return Err(CodecError::malformed(format_args!(
            "RSe stream trailer marker is {trailer_marker:#010x}"
        )));
    }
    let stream_trailer = child(bulk, trailer_start, bulk.window().len(), "stream trailer")?;
    Ok(RseRecordTable {
        records,
        stream_trailer,
    })
}

fn reverse_section<'a>(
    body: View<'a>,
    number: ReverseSectionNumber,
    end: &mut usize,
    payload_len: &mut usize,
) -> Result<MetaSection<'a>, CodecError> {
    // The previous section's back span states `payload_len`, so it is a whole
    // `u32` wide and its sum with the 8-byte header passes a 32-bit `usize`.
    // The two subtractions state that sum without forming it: the chain holds
    // the section only when `end` covers the header and then the payload.
    let header = end
        .checked_sub(8)
        .and_then(|after_header| after_header.checked_sub(*payload_len))
        .ok_or_else(|| CodecError::Malformed("RSe metadata section chain underflows".into()))?;
    let mut view = crate::reader::at(body, body.start() + header, "metadata section back span")?;
    let previous_span = crate::reader::u32(&mut view, "metadata section back span")? as usize;
    let discriminator = crate::reader::u32(&mut view, "metadata section discriminator")?;
    if previous_span < 4 {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata section {number} has invalid back span {previous_span}"
        )));
    }
    let payload = child(body, header + 8, *end, "metadata section payload")?;
    validate_reverse_section(number, discriminator, payload.window().len())?;
    *end = header;
    *payload_len = previous_span - 4;
    Ok(MetaSection {
        number: number.into(),
        discriminator,
        payload,
    })
}

/// Reads one counted metadata section from the live forward cursor, leaving it
/// on the byte after the section's footer span.
fn counted_section<'a>(
    view: &mut View<'a>,
    item_size: usize,
    name: &'static str,
) -> Result<(usize, View<'a>, usize), CodecError> {
    let count = crate::reader::u32(view, name)? as usize;
    if count > 1_000_000 {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata {name} count exceeds 1000000"
        )));
    }
    // The test above bounds `count` at 1000000 and the four callers pass an
    // `item_size` of 4, 10, 28 and 28, so the payload is at most 28000000
    // bytes, which a 32-bit `usize` holds.
    let payload_len = count * item_size;
    let payload_start = view.read_len();
    crate::reader::take(view, payload_len, name)?;
    // `take` advances by exactly `payload_len` on success, so the window offset
    // it reached is the payload's exclusive end.
    let footer = view.read_len();
    let span = crate::reader::u32(view, name)? as usize;
    let expected_span = 4 + payload_len;
    if span != expected_span {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata {name} spans {span} bytes, expected {expected_span}"
        )));
    }
    Ok((count, child(*view, payload_start, footer, name)?, footer))
}

fn validate_reverse_section(
    number: ReverseSectionNumber,
    discriminator: u32,
    payload_len: usize,
) -> Result<(), CodecError> {
    if number != ReverseSectionNumber::Five && discriminator > 1_000_000 {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata section {number} count exceeds 1000000"
        )));
    }
    let item_size = match number {
        ReverseSectionNumber::Seven => {
            if discriminator == 0 {
                0
            } else if payload_len / discriminator as usize >= 0x4c {
                return Ok(());
            } else {
                32
            }
        }
        ReverseSectionNumber::Eight => 20,
        ReverseSectionNumber::Nine => 19,
        ReverseSectionNumber::Ten => 8,
        ReverseSectionNumber::Eleven => 4,
        ReverseSectionNumber::Five | ReverseSectionNumber::Six => return Ok(()),
    };
    // Sections 7 through 11 are the only numbers that reach here, and each one
    // passed the test above, so `discriminator` is at most 1000000. The match
    // states an `item_size` of at most 32, so the product is at most 32000000,
    // which a 32-bit `usize` holds.
    let expected = discriminator as usize * item_size;
    if payload_len != expected {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata section {number} stores {payload_len} bytes for {discriminator} entries of {item_size} bytes"
        )));
    }
    Ok(())
}

fn uses_extended_record_trailer(segment_version_major: u8) -> bool {
    segment_version_major > 18
}

fn parse_extended_record_trailer(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<(), CodecError> {
    if !cursor.record_trailer_presence()? {
        return Ok(());
    }
    let property_count = cursor.u32("record trailer property count")?;
    if property_count & 0x8000_0000 != 0 {
        return Ok(());
    }
    if property_count > 65_536 {
        return Err(CodecError::Malformed(
            "RSe record trailer property count exceeds 65536".into(),
        ));
    }
    ctx.charge_collection_items(
        property_count as u64,
        "admit Inventor RSe record trailer properties",
    )?;
    for _ in 0..property_count {
        cursor.sized_bytes(65_536, "record trailer property name")?;
        match cursor.u32("record trailer property type")? {
            1 => cursor.skip(3, "record trailer property")?,
            3 | 7 => cursor.skip(4, "record trailer property")?,
            8 | 10 => cursor.skip(6, "record trailer property")?,
            11 => cursor.skip(10, "record trailer property")?,
            14 => {
                cursor.skip(2, "record trailer byte-array type")?;
                let len = cursor.u32("record trailer byte-array length")? as usize;
                cursor.skip(len, "record trailer byte array")?;
            }
            value => {
                return Err(CodecError::NotImplemented(format!(
                    "RSe record trailer property type {value} is not implemented"
                )));
            }
        }
    }
    let list_type = cursor.u16("record trailer list type")?;
    let list_marker = cursor.u16("record trailer list marker")?;
    if list_type != 6 || list_marker != 0x3000 {
        return Err(CodecError::malformed(format_args!(
            "RSe record trailer list marker is ({list_type:#06x}, {list_marker:#06x})"
        )));
    }
    let reference_count = cursor.u32("record trailer reference count")?;
    if reference_count > 65_536 {
        return Err(CodecError::Malformed(
            "RSe record trailer reference count exceeds 65536".into(),
        ));
    }
    ctx.charge_collection_items(
        reference_count as u64,
        "admit Inventor RSe record trailer references",
    )?;
    if reference_count != 0 {
        cursor.skip(8, "record trailer reference header")?;
        for _ in 0..reference_count {
            cursor.sized_bytes(65_536, "record trailer reference name")?;
            cursor.skip(4, "record trailer reference value")?;
        }
    }
    Ok(())
}

fn child<'a>(
    parent: View<'a>,
    start: usize,
    end: usize,
    name: &str,
) -> Result<View<'a>, CodecError> {
    parent
        .child(parent.start() + start, parent.start() + end)
        .ok_or_else(|| CodecError::malformed(format_args!("RSe {name} range is invalid")))
}

struct Cursor<'a> {
    source: View<'a>,
}

#[cfg(test)]
use crate::test_support::test_fixtures::push_u32;

#[cfg(test)]
pub(crate) fn synthetic_meta_table_body() -> Vec<u8> {
    let mut body = Vec::new();
    for value in [3_u16, 0, 2, 1, 0, 4, 0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    test_counted(&mut body, &[0x8000_0003, 0x8000_0005], 4);
    test_counted(&mut body, &[], 10);
    test_counted(&mut body, &[], 28);
    push_u32(&mut body, 1);
    body.extend_from_slice(&[0x55; 16]);
    body.extend_from_slice(&1_u16.to_le_bytes());
    push_u32(&mut body, 2);
    body.extend_from_slice(&3_u16.to_le_bytes());
    push_u32(&mut body, 4);
    push_u32(&mut body, 32);

    let payloads = [0_usize, 0, 0, 0, 0, 0, SECTION_11_PAYLOAD_LEN];
    let counts = [u32::MAX, 0, 0, 0, 0, 0, 18];
    push_u32(&mut body, counts[0]);
    body.resize(body.len() + payloads[0], 0);
    for index in 1..payloads.len() {
        push_u32(&mut body, payloads[index - 1] as u32 + 4);
        push_u32(&mut body, counts[index]);
        body.resize(body.len() + payloads[index], 0);
    }
    body.extend_from_slice(&[0x77; 16]);
    body
}

#[cfg(test)]
fn test_counted(body: &mut Vec<u8>, values: &[u32], item_size: usize) {
    push_u32(body, values.len() as u32);
    for value in values {
        push_u32(body, *value);
    }
    body.resize(body.len() + values.len() * (item_size - 4), 0);
    push_u32(body, (4 + values.len() * item_size) as u32);
}

impl<'a> Cursor<'a> {
    const fn new(source: View<'a>) -> Self {
        Self { source }
    }

    /// The offset already read, relative to the start of the window.
    fn position(&self) -> usize {
        self.source.read_len()
    }

    /// Reads the extended record trailer presence flag, which states only
    /// whether a trailer follows.
    fn record_trailer_presence(&mut self) -> Result<bool, CodecError> {
        let value = crate::reader::u8(&mut self.source, "record trailer presence")?;
        match value {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(CodecError::malformed(format_args!(
                "RSe record trailer presence is {value}"
            ))),
        }
    }

    fn u16(&mut self, name: &'static str) -> Result<u16, CodecError> {
        crate::reader::u16(&mut self.source, name)
    }

    fn u32(&mut self, name: &'static str) -> Result<u32, CodecError> {
        crate::reader::u32(&mut self.source, name)
    }

    fn skip(&mut self, len: usize, name: &'static str) -> Result<(), CodecError> {
        self.view(len, name).map(|_| ())
    }

    fn view(&mut self, len: usize, name: &'static str) -> Result<View<'a>, CodecError> {
        let start = self.position();
        // A record trailer byte array states its length as a whole `u32`, so
        // `len` reaches 4294967295 and its sum with any non-zero offset passes
        // a 32-bit `usize`.
        let end = start
            .checked_add(len)
            .ok_or_else(|| CodecError::malformed(format_args!("RSe {name} range overflows")))?;
        crate::reader::take(&mut self.source, len, name)?;
        child(self.source, start, end, name)
    }

    fn sized_bytes(&mut self, maximum: usize, name: &'static str) -> Result<(), CodecError> {
        let len = self.u32(name)? as usize;
        if len > maximum {
            return Err(CodecError::malformed(format_args!(
                "RSe {name} exceeds {maximum} bytes"
            )));
        }
        self.skip(len, name)
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    use super::*;

    #[test]
    fn metadata_tables_frame_forward_and_backward_sections() {
        let body = meta_fixture();
        with_view(&body, |ctx, view| {
            let planted_prefix = [3_u16, 0, 2, 1, 0, 4, 0]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            assert_eq!(&body[..14], planted_prefix.as_slice());
            let tables = parse_meta_tables(ctx, view).expect("synthetic metadata tables parse");
            assert_eq!(tables.prefix, [3, 0, 2, 1, 0, 4, 0]);
            assert_eq!(tables.blocks.len(), 2);
            assert_eq!(tables.types.len(), 1);
            assert_eq!(tables.types[0].id, [0x55; 16]);
            assert_eq!(tables.types[0].fields, [(1, 2), (3, 4)]);
            assert_eq!(tables.sections.len(), 11);
            assert_eq!(tables.sections[10].payload.window().len(), 0x48);
        });
    }

    #[test]
    fn bulk_records_require_lengths_types_and_stream_exhaustion() {
        let meta = meta_fixture();
        with_view(&meta, |ctx, meta_view| {
            let tables =
                parse_meta_tables(ctx, meta_view).expect("synthetic metadata tables parse");
            let mut bulk = Vec::new();
            push_u32(&mut bulk, 0x100);
            bulk.extend_from_slice(&[0x41; 3]);
            push_u32(&mut bulk, 3);
            push_u32(&mut bulk, 0x100);
            bulk.extend_from_slice(&[0x42; 5]);
            push_u32(&mut bulk, 5);
            push_u32(&mut bulk, u32::MAX);
            bulk.extend_from_slice(&[7, 8]);
            with_view(&bulk, |ctx, bulk_view| {
                let records = frame_bulk_records(ctx, bulk_view, &tables, 18)
                    .expect("synthetic bulk records parse");
                assert_eq!(records.records.len(), 2);
                assert_eq!(records.records[1].payload.window(), &[0x42; 5]);
                assert_eq!(
                    records.stream_trailer.window(),
                    &[0xff, 0xff, 0xff, 0xff, 7, 8]
                );
            });
            bulk[24] = 0;
            with_view(&bulk, |ctx, bulk_view| {
                assert!(frame_bulk_records(ctx, bulk_view, &tables, 18).is_err());
            });
        });
    }

    #[test]
    fn only_zero_and_one_state_extended_record_trailer_presence() {
        for byte in 0..=u8::MAX {
            let bytes = [byte];
            with_view(&bytes, |ctx, view| {
                let mut cursor = Cursor::new(view);
                let observed = match cursor.record_trailer_presence() {
                    Ok(true) => "present".to_owned(),
                    Ok(false) => "absent".to_owned(),
                    Err(error) => error.to_string(),
                };
                let expected = match byte {
                    0 => "absent".to_owned(),
                    1 => "present".to_owned(),
                    value => format!("malformed container: RSe record trailer presence is {value}"),
                };
                assert_eq!(observed, expected);

                let mut record = Cursor::new(view);
                let through_record = match parse_extended_record_trailer(ctx, &mut record) {
                    Ok(()) => "accepted".to_owned(),
                    Err(error) => error.to_string(),
                };
                if byte > 1 {
                    assert_eq!(through_record, expected);
                } else {
                    assert_ne!(through_record, expected);
                }
            });
        }
    }

    #[test]
    fn an_absent_record_trailer_presence_byte_is_truncation() {
        with_view(&[], |_ctx, view| {
            let mut cursor = Cursor::new(view);
            let observed = match cursor.record_trailer_presence() {
                Ok(present) => format!("read {present}"),
                Err(error) => error.to_string(),
            };
            assert_eq!(
                observed,
                "truncated input during record trailer presence at space 0 offset 0"
            );
        });
    }

    /// The truncation a read reports, as its variant, field and offset,
    /// without an unwrap on the route.
    fn truncation<T: std::fmt::Debug>(result: Result<T, CodecError>) -> String {
        match result {
            Ok(value) => format!("the read succeeded with {value:?}"),
            Err(CodecError::Truncated {
                location,
                operation,
            }) => format!("Truncated {operation} at offset {}", location.offset),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn a_truncated_rse_record_read_is_located_and_names_its_field() {
        let short = [0_u8; 1];
        for (field, text) in [
            (
                "record trailer list type",
                truncation(
                    Cursor::new(View::over_retained(&short)).u16("record trailer list type"),
                ),
            ),
            (
                "record type selector",
                truncation(Cursor::new(View::over_retained(&short)).u32("record type selector")),
            ),
            (
                "record payload",
                truncation(Cursor::new(View::over_retained(&short)).view(2, "record payload")),
            ),
            (
                "record trailer presence",
                truncation(Cursor::new(View::over_retained(&[])).record_trailer_presence()),
            ),
        ] {
            assert_eq!(text, format!("Truncated {field} at offset 0"));
        }
    }

    #[test]
    fn a_truncated_metadata_table_body_is_located_and_names_its_field() {
        for (bytes, expected) in [
            (Vec::new(), "Truncated metadata prefix at offset 0"),
            (vec![0; 14], "Truncated metadata terminal id at offset 0"),
            (vec![0; 16], "Truncated block-size table at offset 14"),
        ] {
            with_view(&bytes, |ctx, view| {
                assert_eq!(truncation(parse_meta_tables(ctx, view)), expected);
            });
        }
    }

    #[test]
    fn a_metadata_terminal_id_under_the_window_start_is_located() {
        let bytes = [0_u8; 30];
        with_view(&bytes, |ctx, view| {
            let body = view.child(10, 24).expect("a 14-byte child of 30 bytes");
            assert_eq!(
                truncation(parse_meta_tables(ctx, body)),
                "Truncated metadata terminal id at offset 8"
            );
        });
    }

    #[test]
    fn a_reverse_section_the_forward_sections_leave_no_room_for_underflows() {
        // Fourteen prefix bytes, four empty counted sections and the terminal
        // id. Section 11 declares a 0x48-byte payload, which the 46 bytes
        // before the terminal id cannot hold behind its 8-byte header.
        let mut body = vec![0_u8; meta_prefix::LEN];
        for _ in 0..4 {
            push_u32(&mut body, 0);
            push_u32(&mut body, 4);
        }
        body.extend_from_slice(&[0_u8; TERMINAL_ID_LEN]);
        with_view(&body, |ctx, view| {
            let observed = match parse_meta_tables(ctx, view) {
                Ok(tables) => format!("the body parsed with {} sections", tables.sections.len()),
                Err(error) => error.to_string(),
            };
            assert_eq!(
                observed,
                "malformed container: RSe metadata section chain underflows"
            );
        });
    }

    #[test]
    fn a_record_trailer_byte_array_states_a_length_a_32_bit_usize_cannot_offset() {
        // One property of type 14, whose byte-array length is the largest
        // `u32`. `Cursor::view` adds it to the 19 bytes already read: a 32-bit
        // `usize` cannot hold that sum and states the refusal, a 64-bit one
        // holds it and the take refuses the absent bytes instead.
        let mut trailer = vec![1_u8];
        push_u32(&mut trailer, 1);
        push_u32(&mut trailer, 0);
        push_u32(&mut trailer, 14);
        trailer.extend_from_slice(&[0, 0]);
        push_u32(&mut trailer, u32::MAX);
        assert_eq!(trailer.len(), 19);
        with_view(&trailer, |ctx, view| {
            let mut cursor = Cursor::new(view);
            let observed = match parse_extended_record_trailer(ctx, &mut cursor) {
                Ok(()) => "the record trailer parsed".to_owned(),
                Err(error) => error.to_string(),
            };
            let expected = if cfg!(target_pointer_width = "32") {
                "malformed container: RSe record trailer byte array range overflows"
            } else {
                "truncated input during record trailer byte array at space 0 offset 19"
            };
            assert_eq!(observed, expected);
        });
    }

    fn meta_fixture() -> Vec<u8> {
        synthetic_meta_table_body()
    }

    fn with_view(bytes: &[u8], test: impl FnOnce(&DecodeContext<'_>, View<'_>)) {
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic RSe data fits policy");
        test(&ctx, view);
    }
    #[test]
    fn metadata_section_numbers_have_a_closed_numeric_wire_form() {
        for number in 0..=u8::MAX {
            let parsed =
                serde_json::from_value::<super::MetaSectionNumber>(serde_json::json!(number));
            if (1..=11).contains(&number) {
                assert_eq!(
                    serde_json::to_value(parsed.expect("section number is in 1..=11"))
                        .expect("section number is in 1..=11"),
                    serde_json::json!(number)
                );
            } else {
                assert!(parsed.is_err());
            }
        }
    }
}
