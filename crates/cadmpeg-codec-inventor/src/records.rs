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

#[derive(Debug)]
pub(crate) struct MetaSection<'a> {
    pub(crate) number: u8,
    pub(crate) discriminator: u32,
    pub(crate) payload: View<'a>,
}

#[derive(Debug)]
pub(crate) struct MetaTables<'a> {
    pub(crate) prefix: [u16; 7],
    pub(crate) blocks: Vec<BlockDescriptor>,
    pub(crate) types: Vec<TypeDescriptor>,
    pub(crate) sections: Vec<MetaSection<'a>>,
    pub(crate) terminal_id: [u8; 16],
}

#[derive(Debug)]
pub(crate) struct RseRecordFrame<'a> {
    pub(crate) ordinal: u32,
    pub(crate) selector: u32,
    pub(crate) type_index: u8,
    pub(crate) type_id: [u8; 16],
    pub(crate) payload_offset: u64,
    pub(crate) payload: View<'a>,
    pub(crate) declared_payload_len: u32,
    pub(crate) trailing_payload_len: u32,
    pub(crate) trailer: View<'a>,
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
    let bytes = body.window();
    if bytes.len() < meta_prefix::LEN + TERMINAL_ID_LEN {
        return Err(CodecError::Malformed(
            "truncated RSe metadata table body".into(),
        ));
    }
    let mut prefix = [0; 7];
    for (index, value) in prefix.iter_mut().enumerate() {
        *value = read_u16(bytes, index * 2, "metadata prefix")?;
    }

    let mut offset = meta_prefix::LEN;
    let (block_count, section_1_payload, section_1_footer, next) =
        counted_section(body, offset, 4, "block-size table")?;
    if block_count > 1_000_000 {
        return Err(CodecError::Malformed(
            "RSe block-size count exceeds 1000000".into(),
        ));
    }
    ctx.charge_collection_items(block_count as u64, "admit Inventor RSe block descriptors")?;
    let mut blocks = Vec::with_capacity(block_count);
    for ordinal in 0..block_count {
        let encoded = read_u32(section_1_payload.window(), ordinal * 4, "block-size entry")?;
        blocks.push(BlockDescriptor {
            ordinal: ordinal as u32,
            stored: encoded & 0x8000_0000 != 0,
            payload_len: encoded & 0x7fff_ffff,
        });
    }
    let mut sections = vec![MetaSection {
        number: 1,
        discriminator: block_count as u32,
        payload: section_1_payload,
    }];
    offset = next;

    let (section_2_count, section_2_payload, _, next) =
        counted_section(body, offset, 10, "section 2")?;
    sections.push(MetaSection {
        number: 2,
        discriminator: section_2_count as u32,
        payload: section_2_payload,
    });
    offset = next;
    let (section_3_count, section_3_payload, _, next) =
        counted_section(body, offset, 28, "section 3")?;
    sections.push(MetaSection {
        number: 3,
        discriminator: section_3_count as u32,
        payload: section_3_payload,
    });
    offset = next;
    let (type_count, section_4_payload, section_4_footer, _) =
        counted_section(body, offset, type_desc::LEN, "type table")?;
    if type_count > 256 {
        return Err(CodecError::Malformed(
            "RSe type table has more than 256 entries".into(),
        ));
    }
    ctx.charge_collection_items(
        type_count.saturating_add(SECTION_COUNT) as u64,
        "admit Inventor RSe metadata tables",
    )?;
    let mut types = Vec::with_capacity(type_count);
    for index in 0..type_count {
        let entry = section_4_payload
            .window()
            .get(index * type_desc::LEN..index * type_desc::LEN + type_desc::LEN)
            .expect("counted type-table payload has exact entries");
        let mut id = [0; 16];
        id.copy_from_slice(&entry[type_desc::TYPE_ID..type_desc::FIELD_0_KIND]);
        types.push(TypeDescriptor {
            index: index as u8,
            id,
            fields: [
                (
                    View::u16_le_at(entry, type_desc::FIELD_0_KIND).expect("two-byte field"),
                    View::u32_le_at(entry, type_desc::FIELD_0_VALUE).expect("four-byte field"),
                ),
                (
                    View::u16_le_at(entry, type_desc::FIELD_1_KIND).expect("two-byte field"),
                    View::u32_le_at(entry, type_desc::FIELD_1_VALUE).expect("four-byte field"),
                ),
            ],
        });
    }
    sections.push(MetaSection {
        number: 4,
        discriminator: type_count as u32,
        payload: section_4_payload,
    });

    let terminal_start = bytes.len() - TERMINAL_ID_LEN;
    let mut terminal_id = [0; 16];
    terminal_id.copy_from_slice(&bytes[terminal_start..]);
    let mut end = terminal_start;
    let mut payload_len = SECTION_11_PAYLOAD_LEN;
    let mut reverse_sections = Vec::with_capacity(7);
    for number in (5_u8..=11).rev() {
        let header = end
            .checked_sub(payload_len.saturating_add(8))
            .ok_or_else(|| CodecError::Malformed("RSe metadata section chain underflows".into()))?;
        let previous_span = read_u32(bytes, header, "metadata section back span")? as usize;
        let discriminator = read_u32(bytes, header + 4, "metadata section discriminator")?;
        if previous_span < 4 {
            return Err(CodecError::malformed(format_args!(
                "RSe metadata section {number} has invalid back span {previous_span}"
            )));
        }
        let payload = child(body, header + 8, end, "metadata section payload")?;
        validate_reverse_section(number, discriminator, payload.window().len())?;
        reverse_sections.push(MetaSection {
            number,
            discriminator,
            payload,
        });
        end = header;
        payload_len = previous_span - 4;
    }
    if end != section_4_footer {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata section chain ends at {end}, expected {section_4_footer}"
        )));
    }
    reverse_sections.reverse();
    sections.extend(reverse_sections);
    debug_assert_eq!(sections.len(), SECTION_COUNT);
    debug_assert_eq!(section_1_footer, meta_prefix::LEN + 4 + block_count * 4);
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
        let payload_offset = cursor.position as u64;
        let payload = cursor.view(block.payload_len as usize, "record payload")?;
        let trailing_payload_len = cursor.u32("record trailing payload length")?;
        if trailing_payload_len != 0 && trailing_payload_len != block.payload_len {
            return Err(CodecError::malformed(format_args!(
                "RSe record {} declares payload length {} but trails with {trailing_payload_len}",
                block.ordinal, block.payload_len
            )));
        }
        let trailer_start = cursor.position;
        if uses_extended_record_trailer(segment_version_major) {
            parse_extended_record_trailer(ctx, &mut cursor)?;
        }
        let trailer = child(bulk, trailer_start, cursor.position, "record trailer")?;
        records.push(RseRecordFrame {
            ordinal: block.ordinal,
            selector,
            type_index,
            type_id: descriptor.id,
            payload_offset,
            payload,
            declared_payload_len: block.payload_len,
            trailing_payload_len,
            trailer,
        });
    }
    let trailer_start = cursor.position;
    let trailer_marker = cursor.u32("stream trailer marker")?;
    if trailer_marker != u32::MAX {
        return Err(CodecError::malformed(format_args!(
            "RSe stream trailer marker is {trailer_marker:#010x}"
        )));
    }
    let stream_trailer = child(bulk, trailer_start, bulk.window().len(), "stream trailer")?;
    cursor.position = bulk.window().len();
    cursor.finish()?;
    Ok(RseRecordTable {
        records,
        stream_trailer,
    })
}

fn counted_section<'a>(
    body: View<'a>,
    offset: usize,
    item_size: usize,
    name: &str,
) -> Result<(usize, View<'a>, usize, usize), CodecError> {
    let bytes = body.window();
    let count = read_u32(bytes, offset, name)? as usize;
    if count > 1_000_000 {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata {name} count exceeds 1000000"
        )));
    }
    let payload_len = count.checked_mul(item_size).ok_or_else(|| {
        CodecError::malformed(format_args!("RSe metadata {name} length overflows"))
    })?;
    let payload_start = offset + 4;
    let footer = payload_start.checked_add(payload_len).ok_or_else(|| {
        CodecError::malformed(format_args!("RSe metadata {name} range overflows"))
    })?;
    let span = read_u32(bytes, footer, name)? as usize;
    let expected_span = 4 + payload_len;
    if span != expected_span {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata {name} spans {span} bytes, expected {expected_span}"
        )));
    }
    Ok((
        count,
        child(body, payload_start, footer, name)?,
        footer,
        footer + 4,
    ))
}

fn validate_reverse_section(
    number: u8,
    discriminator: u32,
    payload_len: usize,
) -> Result<(), CodecError> {
    if number == 5 {
        return Ok(());
    }
    if discriminator > 1_000_000 {
        return Err(CodecError::malformed(format_args!(
            "RSe metadata section {number} count exceeds 1000000"
        )));
    }
    if number == 6 {
        return Ok(());
    }
    let item_size = match number {
        7 => {
            if discriminator == 0 {
                0
            } else if payload_len / discriminator as usize >= 0x4c {
                return Ok(());
            } else {
                32
            }
        }
        8 => 20,
        9 => 19,
        10 => 8,
        11 => 4,
        _ => unreachable!("validated reverse section number"),
    };
    let expected = (discriminator as usize)
        .checked_mul(item_size)
        .ok_or_else(|| CodecError::Malformed("RSe metadata section length overflows".into()))?;
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
    if cursor.u8("record trailer presence")? == 0 {
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

fn read_u16(bytes: &[u8], offset: usize, name: &str) -> Result<u16, CodecError> {
    View::u16_le_at(bytes, offset)
        .ok_or_else(|| CodecError::malformed(format_args!("truncated RSe {name}")))
}

fn read_u32(bytes: &[u8], offset: usize, name: &str) -> Result<u32, CodecError> {
    View::u32_le_at(bytes, offset)
        .ok_or_else(|| CodecError::malformed(format_args!("truncated RSe {name}")))
}

struct Cursor<'a> {
    source: View<'a>,
    position: usize,
}

#[cfg(test)]
pub(crate) fn synthetic_meta_table_body() -> Vec<u8> {
    let mut body = Vec::new();
    for value in [3_u16, 0, 2, 1, 0, 4, 0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    test_counted(&mut body, &[0x8000_0003, 0x8000_0005], 4);
    test_counted(&mut body, &[], 10);
    test_counted(&mut body, &[], 28);
    push_test_u32(&mut body, 1);
    body.extend_from_slice(&[0x55; 16]);
    body.extend_from_slice(&1_u16.to_le_bytes());
    push_test_u32(&mut body, 2);
    body.extend_from_slice(&3_u16.to_le_bytes());
    push_test_u32(&mut body, 4);
    push_test_u32(&mut body, 32);

    let payloads = [0_usize, 0, 0, 0, 0, 0, SECTION_11_PAYLOAD_LEN];
    let counts = [u32::MAX, 0, 0, 0, 0, 0, 18];
    push_test_u32(&mut body, counts[0]);
    body.resize(body.len() + payloads[0], 0);
    for index in 1..payloads.len() {
        push_test_u32(&mut body, payloads[index - 1] as u32 + 4);
        push_test_u32(&mut body, counts[index]);
        body.resize(body.len() + payloads[index], 0);
    }
    body.extend_from_slice(&[0x77; 16]);
    body
}

#[cfg(test)]
fn test_counted(body: &mut Vec<u8>, values: &[u32], item_size: usize) {
    push_test_u32(body, values.len() as u32);
    for value in values {
        push_test_u32(body, *value);
    }
    body.resize(body.len() + values.len() * (item_size - 4), 0);
    push_test_u32(body, (4 + values.len() * item_size) as u32);
}

#[cfg(test)]
fn push_test_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

impl<'a> Cursor<'a> {
    const fn new(source: View<'a>) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    fn remaining(&self) -> usize {
        self.source.window().len().saturating_sub(self.position)
    }

    fn u8(&mut self, name: &str) -> Result<u8, CodecError> {
        let value = *self
            .source
            .window()
            .get(self.position)
            .ok_or_else(|| CodecError::malformed(format_args!("truncated RSe {name}")))?;
        self.position += 1;
        if name == "record trailer presence" && value > 1 {
            return Err(CodecError::malformed(format_args!(
                "RSe record trailer presence is {value}"
            )));
        }
        Ok(value)
    }

    fn u16(&mut self, name: &str) -> Result<u16, CodecError> {
        let value = read_u16(self.source.window(), self.position, name)?;
        self.position += 2;
        Ok(value)
    }

    fn u32(&mut self, name: &str) -> Result<u32, CodecError> {
        let value = read_u32(self.source.window(), self.position, name)?;
        self.position += 4;
        Ok(value)
    }

    fn skip(&mut self, len: usize, name: &str) -> Result<(), CodecError> {
        self.view(len, name).map(|_| ())
    }

    fn view(&mut self, len: usize, name: &str) -> Result<View<'a>, CodecError> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| CodecError::malformed(format_args!("RSe {name} range overflows")))?;
        let view = child(self.source, self.position, end, name)?;
        self.position = end;
        Ok(view)
    }

    fn sized_bytes(&mut self, maximum: usize, name: &str) -> Result<(), CodecError> {
        let len = self.u32(name)? as usize;
        if len > maximum {
            return Err(CodecError::malformed(format_args!(
                "RSe {name} exceeds {maximum} bytes"
            )));
        }
        self.skip(len, name)
    }

    fn finish(self) -> Result<(), CodecError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(CodecError::malformed(format_args!(
                "RSe record stream has {} trailing bytes",
                self.remaining()
            )))
        }
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

    fn meta_fixture() -> Vec<u8> {
        synthetic_meta_table_body()
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn with_view(bytes: &[u8], test: impl FnOnce(&DecodeContext<'_>, View<'_>)) {
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic RSe data fits policy");
        test(&ctx, view);
    }
}
