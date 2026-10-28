// SPDX-License-Identifier: Apache-2.0
//! Property sets in the root compound default-property stream.

use cadmpeg_core::decode::{index_from_u32, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeValue;
use std::mem::size_of;

use crate::layout::{
    oleps_dictionary_entry_prefix as dictionary_entry, oleps_dictionary_header as dictionary,
    oleps_property_identifier_offset as directory, oleps_property_set_header as set_header,
    oleps_section_descriptor as descriptor, oleps_stream_header as header,
    oleps_typed_value_header as typed,
};

use super::{Alias, SavedValue, Table};

const USER_DEFINED: [u8; 16] = [
    0x05, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];
const DOCUMENT_SUMMARY: [u8; 16] = [
    0x02, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];

pub(super) fn table(ctx: &DecodeContext<'_>, payload: &[u8]) -> Result<Table, CodecError> {
    let root = View::over_retained(payload);
    field(root, 0, header::LEN)?;
    if u16_at(root, header::BYTE_ORDER)? != header::BYTE_ORDER_VALUE
        || !matches!(u16_at(root, header::VERSION)?, 0 | 1)
    {
        return Err(unsupported("default-property stream header"));
    }
    let count = index_from_u32(u32_at(root, header::SECTION_COUNT)?);
    if !(1..=2).contains(&count) {
        return Err(unsupported("default-property section count"));
    }
    let descriptors_end = header::LEN + count * descriptor::LEN;
    field(root, header::LEN, count * descriptor::LEN)?;
    let mut selected = None;
    let mut spans = [(0, 0); 2];
    for (index, span) in spans.iter_mut().enumerate().take(count) {
        let record = field(root, header::LEN + index * descriptor::LEN, descriptor::LEN)?;
        let fmtid = field(record, descriptor::FMTID, USER_DEFINED.len())?.window();
        if count == 2
            && fmtid
                != if index == 0 {
                    DOCUMENT_SUMMARY
                } else {
                    USER_DEFINED
                }
        {
            return Err(unsupported("default-property two-section FMTID order"));
        }
        let start = index_from_u32(u32_at(record, descriptor::SECTION_OFFSET)?);
        if start < descriptors_end || !start.is_multiple_of(4) {
            return Err(unsupported("default-property section offset"));
        }
        let length = index_from_u32(u32_at(root, start + set_header::SIZE)?);
        if length < set_header::LEN {
            return Err(unsupported("default-property section length"));
        }
        let section = field(root, start, length)?;
        *span = (start, start + length);
        if fmtid == USER_DEFINED && selected.replace(section).is_some() {
            return Err(unsupported("default-property repeated UserDefined set"));
        }
    }
    if count == 2 && spans[0].0 < spans[1].1 && spans[1].0 < spans[0].1 {
        return Err(unsupported("default-property overlapping sections"));
    }
    parse_set(
        ctx,
        selected.ok_or_else(|| unsupported("default-property missing UserDefined set"))?,
    )
}

fn parse_set(ctx: &DecodeContext<'_>, section: View<'_>) -> Result<Table, CodecError> {
    let count = u32_at(section, set_header::PROPERTY_COUNT)?;
    let mut input = field(
        section,
        set_header::LEN,
        section.window().len() - set_header::LEN,
    )?;
    let count = input
        .counted(u64::from(count), directory::LEN)
        .ok_or_else(|| unsupported("default-property directory count"))?
        .get();
    let directory_end = set_header::LEN + count * directory::LEN;
    let mut entries =
        ctx.alloc_filled(count, (0u32, 0usize), "allocate default-property directory")?;
    for entry in &mut entries {
        let record = input.req_take_child(directory::LEN)?;
        let pid = u32_at(record, directory::PID)?;
        let at = index_from_u32(u32_at(record, directory::VALUE_OFFSET)?);
        if at < directory_end || at >= section.window().len() || !at.is_multiple_of(4) {
            return Err(unsupported("default-property value offset"));
        }
        *entry = (pid, at);
    }
    if !entries.windows(2).all(|pair| pair[0].1 < pair[1].1) {
        return Err(unsupported("default-property directory offset order"));
    }
    if entries.iter().filter(|(pid, _)| *pid == 0).count() != 1 {
        return Err(unsupported("default-property dictionary identity"));
    }
    let mut codepages = entries.iter().enumerate().filter(|(_, (pid, _))| *pid == 1);
    let Some((index, _)) = codepages.next() else {
        return Err(unsupported("default-property CodePage identity"));
    };
    if codepages.next().is_some() {
        return Err(unsupported("default-property CodePage identity"));
    }
    let control = value_view(section, &entries, index)?;
    if u16_at(control, typed::TYPE)? != 2
        || u16_at(control, typed::PADDING)? != typed::PADDING_VALUE
    {
        return Err(unsupported("default-property CodePage type"));
    }
    let codepage = u16_at(control, typed::LEN)?;
    if !matches!(codepage, 65001 | 1200) {
        return Err(CodecError::NotImplemented(format!(
            "default-property codepage {codepage} is retained without a text decoder"
        )));
    }
    let mut table = Table {
        values: ctx.alloc_filled(
            count,
            SavedValue {
                pid: 0,
                value: None,
                source_type: "unsupported",
            },
            "allocate saved property values",
        )?,
        aliases: Vec::new(),
    };
    for (index, (pid, _)) in entries.iter().enumerate() {
        let value = value_view(section, &entries, index)?;
        if *pid == 0 {
            read_dictionary(ctx, value, codepage, &mut table.aliases)?;
        } else {
            let kind = u16_at(value, typed::TYPE)?;
            if u16_at(value, typed::PADDING)? != typed::PADDING_VALUE {
                return Err(unsupported("default-property typed padding"));
            }
            let mut body = field(value, typed::LEN, value.window().len() - typed::LEN)?;
            let (value, source_type) = scalar(ctx, kind, codepage, &mut body)?;
            if source_type != "unsupported"
                && (body.remaining() > 3 || body.unread().iter().any(|byte| *byte != 0))
            {
                return Err(unsupported("default-property typed extent padding"));
            }
            table.values[index] = SavedValue {
                pid: *pid,
                value,
                source_type,
            };
        }
    }
    table.values.retain(|value| value.pid != 0);
    Ok(table)
}

fn value_view<'a>(
    section: View<'a>,
    entries: &[(u32, usize)],
    index: usize,
) -> Result<View<'a>, CodecError> {
    let start = entries[index].1;
    let end = entries
        .get(index + 1)
        .map_or(section.window().len(), |entry| entry.1);
    field(section, start, end - start)
}

fn read_dictionary(
    ctx: &DecodeContext<'_>,
    value: View<'_>,
    codepage: u16,
    aliases: &mut Vec<Alias>,
) -> Result<(), CodecError> {
    let declared = u32_at(value, dictionary::ENTRY_COUNT)?;
    let mut input = field(
        value,
        dictionary::LEN,
        value.window().len() - dictionary::LEN,
    )?;
    let count = input
        .counted(u64::from(declared), dictionary_entry::LEN)
        .ok_or_else(|| unsupported("default-property dictionary count"))?
        .get();
    let mut decoded = ctx.alloc_filled(
        count,
        Alias {
            pid: 0,
            name: String::new(),
        },
        "allocate saved dictionary aliases",
    )?;
    for alias in &mut decoded {
        let record = input.req_take_child(dictionary_entry::LEN)?;
        let pid = u32_at(record, dictionary_entry::PID)?;
        let length = index_from_u32(u32_at(record, dictionary_entry::NAME_LENGTH)?);
        let byte_count = length
            .checked_mul(if codepage == 1200 { 2 } else { 1 })
            .ok_or_else(|| unsupported("default-property dictionary string length"))?;
        let text = text(ctx, input.req_take(byte_count)?, codepage)?;
        if codepage == 1200 {
            let padding = (4 - (input.position() - value.start()) % 4) % 4;
            if input.req_take(padding)?.iter().any(|byte| *byte != 0) {
                return Err(unsupported("default-property dictionary padding"));
            }
        }
        *alias = Alias { name: text, pid };
    }
    if input.remaining() > 3 || input.unread().iter().any(|byte| *byte != 0) {
        return Err(unsupported("default-property dictionary trailing bytes"));
    }
    decoded.retain(|alias| !alias.name.is_empty());
    if aliases.is_empty() {
        *aliases = decoded;
    } else {
        return Err(unsupported("default-property repeated dictionary"));
    }
    Ok(())
}

fn scalar(
    ctx: &DecodeContext<'_>,
    kind: u16,
    codepage: u16,
    body: &mut View<'_>,
) -> Result<(Option<AttributeValue>, &'static str), CodecError> {
    let (value, tag) = match kind {
        0 => (None, "empty"),
        1 => (None, "null"),
        2 => (
            Some(AttributeValue::Integer(i64::from(body.req_i16_le()?))),
            "i2",
        ),
        3 => (
            Some(AttributeValue::Integer(i64::from(body.req_i32_le()?))),
            "i4",
        ),
        4 => (AttributeValue::float(f64::from(body.req_f32_le()?)), "r4"),
        5 => (AttributeValue::float(body.req_f64_le()?), "r8"),
        11 => (
            match body.req_i16_le()? {
                0 => Some(AttributeValue::Boolean(false)),
                -1 => Some(AttributeValue::Boolean(true)),
                _ => None,
            },
            "bool",
        ),
        16 => {
            let byte = i64::from(body.req_u8()?);
            (
                Some(AttributeValue::Integer(if byte < 128 {
                    byte
                } else {
                    byte - 256
                })),
                "i1",
            )
        }
        17 => (
            Some(AttributeValue::Integer(i64::from(body.req_u8()?))),
            "ui1",
        ),
        18 => (
            Some(AttributeValue::Integer(i64::from(body.req_u16_le()?))),
            "ui2",
        ),
        19 => (
            Some(AttributeValue::Integer(i64::from(body.req_u32_le()?))),
            "ui4",
        ),
        20 => (Some(AttributeValue::Integer(body.req_i64_le()?)), "i8"),
        21 => (
            i64::try_from(body.req_u64_le()?)
                .ok()
                .map(AttributeValue::Integer),
            "ui8",
        ),
        30 | 31 => {
            let count = index_from_u32(body.req_u32_le()?);
            let encoding = if kind == 31 { 1200 } else { codepage };
            let byte_count = count
                .checked_mul(if encoding == 1200 { 2 } else { 1 })
                .ok_or_else(|| unsupported("default-property scalar string length"))?;
            let value = text(ctx, body.req_take(byte_count)?, encoding)?;
            (
                Some(AttributeValue::String(value)),
                if kind == 30 { "lpstr" } else { "lpwstr" },
            )
        }
        _ => (None, "unsupported"),
    };
    Ok((value, tag))
}

fn text(ctx: &DecodeContext<'_>, bytes: &[u8], codepage: u16) -> Result<String, CodecError> {
    match codepage {
        65001 => {
            let body = bytes
                .strip_suffix(&[0])
                .ok_or_else(|| unsupported("default-property UTF8 terminator"))?;
            let text = std::str::from_utf8(body)
                .map_err(|_| unsupported("default-property UTF8 encoding"))?;
            ctx.retain_bytes(u64_from_index(text.len()), "retain saved property UTF8")?;
            Ok(text.to_owned())
        }
        1200 => {
            let body = bytes
                .strip_suffix(&[0, 0])
                .filter(|body| body.len() % 2 == 0)
                .ok_or_else(|| unsupported("default-property UTF16 terminator"))?;
            let maximum = (body.len() / 2)
                .checked_mul(3)
                .ok_or_else(|| unsupported("default-property UTF16 allocation length"))?;
            ctx.retain_bytes(u64_from_index(maximum), "admit saved property UTF16 text")?;
            ctx.utf16le_text(
                body,
                body.len() / 2,
                false,
                "decode saved default-property UTF16",
            )
        }
        _ => Err(unsupported("default-property text codepage")),
    }
}

fn field(view: View<'_>, relative: usize, length: usize) -> Result<View<'_>, CodecError> {
    let start = view
        .start()
        .checked_add(relative)
        .ok_or_else(|| unsupported("default-property field offset"))?;
    let end = start
        .checked_add(length)
        .ok_or_else(|| unsupported("default-property field length"))?;
    view.child(start, end)
        .ok_or_else(|| unsupported("default-property field bounds"))
}

fn u16_at(view: View<'_>, relative: usize) -> Result<u16, CodecError> {
    Ok(field(view, relative, size_of::<u16>())?.req_u16_le()?)
}

fn u32_at(view: View<'_>, relative: usize) -> Result<u32, CodecError> {
    Ok(field(view, relative, size_of::<u32>())?.req_u32_le()?)
}

fn unsupported(what: &str) -> CodecError {
    CodecError::malformed(what)
}
