// SPDX-License-Identifier: Apache-2.0
//! Persistent string records retain successful peers after a damaged payload.

use super::{checked_count, spec, text, Scanner};
use crate::native::StringTableEntry;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use std::collections::BTreeSet;
use std::ops::Range;

pub(in crate::element_map) struct Table {
    pub(in crate::element_map) entries: Vec<Option<StringTableEntry>>,
    pub(in crate::element_map) unavailable_ids: BTreeSet<i64>,
}

pub(in crate::element_map) fn table<'source>(
    ctx: &DecodeContext<'_>,
    view: View<'source>,
    count: Option<usize>,
    side: bool,
    shape_owned: bool,
    spacing: Option<&super::Spacing<'_, 'source>>,
    output: &mut PartOutput,
) -> Result<Option<Table>, CodecError> {
    let mut scanner = Scanner::tracking(view, spacing);
    let count = if side {
        let start = scanner.whitespace_end()?;
        let _magic = scanner.token_range()?;
        let version = scanner.token_range()?;
        let count_range = scanner.token_range()?;
        let end = scanner.whitespace_end()?;
        let framed_count = scanner
            .navigation(count_range.clone())?
            .parse::<usize>()
            .map_err(|_| CodecError::malformed("invalid string-table framing count"))?;
        if framed_count > super::super::MAX_TABLE_ENTRIES {
            return Err(CodecError::malformed(
                "string-table framing count exceeds limit",
            ));
        }
        let extent = scanner.extent(start..end)?;
        let framing = [
            count_range.start - start..count_range.end - start,
            end.checked_sub(1)
                .ok_or_else(|| CodecError::malformed("string table header extent is empty"))?
                - start..end - start,
        ];
        ctx.decode_part(
            output,
            extent,
            spec("freecad.string_table_header", extent, &framing)?,
            |_stage| {
                let mut header = Scanner::new(extent);
                header.expect("StringTableStart")?;
                header.expect("v1")?;
                let actual = header.count(super::super::MAX_TABLE_ENTRIES)?;
                if count.is_some_and(|expected| actual != expected) {
                    return Err(CodecError::malformed(
                        "string-table XML and side-entry counts disagree",
                    ));
                }
                Ok(actual)
            },
        )?;
        if scanner.navigation(version)? != "v1" {
            return Ok(None);
        }
        framed_count
    } else {
        count.ok_or_else(|| CodecError::malformed("inline string table has no count"))?
    };
    checked_count(ctx, count, view, "string-table record count exceeds input")?;
    let mut entries = ctx.alloc_filled(count, None, "FreeCAD string-table records")?;
    let level = if side {
        "freecad.string_table_record"
    } else if shape_owned {
        "freecad.shape_string_hasher_record"
    } else {
        "freecad.string_hasher_record"
    };
    let mut previous = Some(StringTableEntry {
        string_id: 0,
        flags: 0,
        components: Vec::new(),
        payload: String::new(),
    });
    let mut declared_previous = previous.as_ref().map(Header::from);
    let mut declared_previous_id = Some(0_i64);
    let mut unavailable_ids = BTreeSet::new();
    let mut known = std::collections::HashSet::new();
    let mut failed = false;
    for index in 0..count {
        if failed && scanner.done()? {
            break;
        }
        scanner.skip()?;
        let start = scanner.at;
        let (end, framing) = frame_record(&mut scanner)?;
        let extent = scanner.extent(start..end)?;
        let fields = framing
            .into_iter()
            .map(|range| range.start - start..range.end - start)
            .collect::<Vec<_>>();
        // Navigation identifies dependencies; it never supplies semantic values
        // for an admitted record. A missing producer prunes its consumers under
        // #214 Rule 2 rather than creating a second source failure.
        let mut navigation = Scanner::new(extent);
        let header = navigation
            .token_range()
            .and_then(|range| navigation.navigation(range))
            .ok();
        let declared =
            header.and_then(|header| Header::parse(header, declared_previous.as_ref()).ok());
        let declared_id = header.and_then(|header| declared_identity(header, declared_previous_id));
        let dependent = (header.is_some_and(|header| header.starts_with('-'))
            && previous.is_none())
            || declared.as_ref().is_some_and(|header| {
                header
                    .components
                    .iter()
                    .any(|id| unavailable_ids.contains(id))
            });
        let entry = ctx
            .decode_part(output, extent, spec(level, extent, &fields)?, |_stage| {
                if dependent {
                    return Ok(None);
                }
                let entry = record(extent, previous.as_ref())?;
                if entry.components.iter().any(|id| !known.contains(id)) {
                    return Err(CodecError::malformed(
                        "string components reference unavailable prior ids",
                    ));
                }
                if known.contains(&entry.string_id) {
                    return Err(CodecError::malformed("duplicate persistent string id"));
                }
                Ok(Some(entry))
            })?
            .flatten();
        declared_previous = declared;
        declared_previous_id = declared_id;
        if let Some(entry) = entry {
            known.insert(entry.string_id);
            unavailable_ids.remove(&entry.string_id);
            previous = Some(entry.clone());
            entries[index] = Some(entry);
        } else {
            if let Some(id) = declared_id {
                if !known.contains(&id) {
                    unavailable_ids.insert(id);
                }
            }
            previous = None;
            failed = true;
        }
    }
    if !scanner.done()? {
        return Err(CodecError::malformed(
            "string table contains records beyond declared count",
        ));
    }
    Ok(Some(Table {
        entries,
        unavailable_ids,
    }))
}

fn frame_record(scanner: &mut Scanner<'_, '_>) -> Result<(usize, Vec<Range<usize>>), CodecError> {
    let mut framing = Vec::new();
    let header_range = scanner.token_range()?;
    let header = &scanner.view.window()[header_range.clone()];
    let separator = header
        .iter()
        .position(|byte| *byte == b'.')
        .ok_or_else(|| CodecError::malformed("string record flag delimiter is absent"))?;
    let flag_start = header_range.start + separator + 1;
    let flag_end = scanner.view.window()[flag_start..header_range.end]
        .iter()
        .position(|byte| *byte == b'.')
        .map_or(header_range.end, |at| flag_start + at);
    let flags_text = scanner.navigation(flag_start..flag_end)?;
    framing.push(
        header_range.start + separator..if flag_end < header_range.end {
            flag_end + 1
        } else {
            flag_end
        },
    );
    let flags = u64::from_str_radix(flags_text, 16)
        .map_err(|_| CodecError::malformed("invalid string framing flags"))?;
    if flags & 8 == 0 {
        let header_end = scanner.at;
        scanner.skip()?;
        if header_end < scanner.at {
            framing.push(header_end..scanner.at);
        }
        let count_start = scanner.at;
        while scanner
            .view
            .window()
            .get(scanner.at)
            .is_some_and(u8::is_ascii_digit)
        {
            scanner.at += 1;
        }
        if scanner.at == count_start || scanner.view.window().get(scanner.at) != Some(&b':') {
            return Err(CodecError::malformed(
                "string text framing has no decimal line count",
            ));
        }
        let count = scanner
            .navigation(count_start..scanner.at)?
            .parse::<usize>()
            .map_err(|_| CodecError::malformed("invalid string line count"))?;
        scanner.at += 1;
        framing.push(count_start..scanner.at);
        for _ in 0..=count {
            let Some(newline) = scanner.view.window()[scanner.at..]
                .iter()
                .position(|byte| *byte == b'\n')
            else {
                // #215 cut-short sources: the intact header locates this
                // final record through the carrier's actual EOF. Its leaf
                // parser rejects the missing terminator without row resync.
                scanner.at = scanner.view.window().len();
                framing.retain(|range| !range.is_empty());
                return Ok((scanner.at, framing));
            };
            scanner.at += newline + 1;
        }
        framing.push(scanner.at - 1..scanner.at);
    } else {
        for present in [flags & (0x10 | 0x20 | 0x40) == 0, flags & 4 == 0] {
            if present {
                let before = scanner.at;
                let token = scanner.token_range()?;
                if before < token.start {
                    framing.push(before..token.start);
                }
            }
        }
        let token_end = scanner.at;
        scanner.skip()?;
        if token_end < scanner.at {
            framing.push(token_end..scanner.at);
        }
    }
    framing.retain(|range| !range.is_empty());
    framing.sort_by_key(|range| range.start);
    Ok((scanner.at, framing))
}

pub(in crate::element_map) fn record(
    view: View<'_>,
    previous: Option<&StringTableEntry>,
) -> Result<StringTableEntry, CodecError> {
    let mut scanner = Scanner::new(view);
    let header = scanner.token()?;
    let header = Header::parse(header, previous.map(Header::from).as_ref())?;
    let Header {
        string_id,
        flags,
        components,
        ..
    } = header;
    let payload = if flags & 8 == 0 {
        scanner.skip()?;
        let start = scanner.at;
        while scanner
            .view
            .window()
            .get(scanner.at)
            .is_some_and(u8::is_ascii_digit)
        {
            scanner.at += 1;
        }
        let count = text(view, start..scanner.at)?
            .parse::<usize>()
            .map_err(|_| CodecError::malformed("invalid text line count"))?;
        if scanner.view.window().get(scanner.at) != Some(&b':') {
            return Err(CodecError::malformed("text line count has no colon"));
        }
        scanner.at += 1;
        let start = scanner.at;
        for _ in 0..=count {
            let newline = scanner.view.window()[scanner.at..]
                .iter()
                .position(|byte| *byte == b'\n')
                .ok_or_else(|| CodecError::malformed("text line delimiter is absent"))?;
            scanner.at += newline + 1;
        }
        text(view, start..scanner.at - 1)?.to_owned()
    } else {
        let mut values = Vec::new();
        if flags & (0x10 | 0x20 | 0x40) == 0 {
            values.push(scanner.token()?.to_owned());
        }
        if flags & 4 == 0 {
            values.push(scanner.token()?.to_owned());
        }
        values.join(" ")
    };
    Ok(StringTableEntry {
        string_id,
        flags,
        components,
        payload,
    })
}

/// Source addresses used only to locate dependent records. Accepted values are
/// always read again by `record` within their genuine leaf transaction.
struct Header {
    string_id: i64,
    flags: u64,
    components: Vec<i64>,
}

impl From<&StringTableEntry> for Header {
    fn from(entry: &StringTableEntry) -> Self {
        Self {
            string_id: entry.string_id,
            flags: entry.flags,
            components: entry.components.clone(),
        }
    }
}

impl Header {
    fn parse(header: &str, previous: Option<&Self>) -> Result<Self, CodecError> {
        let mut fields = header.split('.');
        let encoded = fields
            .next()
            .ok_or_else(|| CodecError::malformed("string id is absent"))?;
        let flags = u64::from_str_radix(
            fields
                .next()
                .ok_or_else(|| CodecError::malformed("string flags are absent"))?,
            16,
        )
        .map_err(|_| CodecError::malformed("invalid string flags"))?;
        let relative = encoded.starts_with('-');
        let value = super::super::parse_hex(encoded, "string id")?;
        let string_id = if relative {
            previous
                .ok_or_else(|| {
                    CodecError::malformed("relative string id has no admitted predecessor")
                })?
                .string_id
                .checked_sub(value)
                .ok_or_else(|| CodecError::malformed("relative string id overflows"))?
        } else {
            value
        };
        let mut components = Vec::new();
        for (index, field) in fields.enumerate() {
            let encoded = super::super::parse_hex(field, "string component")?;
            let component = if relative {
                match previous.and_then(|entry| entry.components.get(index)) {
                    Some(value) => value.checked_add(encoded),
                    None => string_id.checked_sub(encoded),
                }
                .ok_or_else(|| CodecError::malformed("relative string component overflows"))?
            } else {
                encoded
            };
            components.push(component);
        }
        Ok(Self {
            string_id,
            flags,
            components,
        })
    }
}

fn declared_identity(header: &str, previous: Option<i64>) -> Option<i64> {
    let encoded = header.split('.').next()?;
    let id = super::super::parse_hex(encoded, "string id").ok()?;
    if encoded.starts_with('-') {
        previous?.checked_sub(id)
    } else {
        Some(id)
    }
}
