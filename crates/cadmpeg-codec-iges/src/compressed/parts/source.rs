// SPDX-License-Identifier: Apache-2.0
//! Physical Compressed ASCII line and inherited field locations.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::super::{malformed, CompressedField, DirectoryFields, ParsedDirectoryRecord, FIELDS};
use super::append;

pub(super) struct Line<'a> {
    pub(super) payload: View<'a>,
    pub(super) extent: View<'a>,
}

pub(super) fn lines<'a>(
    source: View<'a>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<Line<'a>>, CodecError> {
    let mut out = Vec::new();
    let mut cursor = source.start();
    while cursor < source.end() {
        let tail = source
            .child(cursor, source.end())
            .ok_or_else(|| malformed("line exceeds source"))?;
        let count = tail
            .window()
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'))
            .unwrap_or(tail.window().len());
        let end = cursor
            .checked_add(count)
            .ok_or_else(|| malformed("line end overflows"))?;
        let ending = match tail.window().get(count) {
            Some(b'\r') if tail.window().get(count + 1) == Some(&b'\n') => 2,
            Some(_) => 1,
            None => 0,
        };
        let next = end
            .checked_add(ending)
            .ok_or_else(|| malformed("line ending overflows"))?;
        append(
            ctx,
            &mut out,
            Line {
                payload: source
                    .child(cursor, end)
                    .ok_or_else(|| malformed("line payload exceeds source"))?,
                extent: source
                    .child(cursor, next)
                    .ok_or_else(|| malformed("line ending exceeds source"))?,
            },
        )?;
        cursor = next;
    }
    Ok(out)
}

#[derive(Clone, Copy)]
pub(super) struct Fields<'a>(
    [Option<View<'a>>; 16],
    /// The specifiers of the current record: the whole specifier and the
    /// absolute start of its value.
    [Option<(View<'a>, usize)>; 16],
);

/// The optional Directory fields by field number.
const OPTIONAL_FIELDS: [(usize, &str); 7] = [
    (4, "line font"),
    (5, "level"),
    (6, "view"),
    (8, "label display"),
    (12, "line weight"),
    (13, "color"),
    (19, "entity subscript"),
];

impl<'a> Fields<'a> {
    pub(super) fn apply(
        previous: Option<Self>,
        lines: &[Line<'a>],
        start: usize,
        parsed: &ParsedDirectoryRecord,
        delimiter: u8,
        ctx: &DecodeContext<'_>,
        framing: &mut Vec<std::ops::Range<u64>>,
    ) -> Result<Self, CodecError> {
        let mut fields = previous.unwrap_or(Self([None; 16], [None; 16]));
        fields.1 = [None; 16];
        for (index, line) in lines.iter().enumerate().take(parsed.next).skip(start) {
            let bytes = line.payload.window();
            let begin = if index == start {
                super::super::parse_sequence(bytes, 1, "Data record")?.1
            } else {
                0
            };
            let end = bytes[begin..]
                .iter()
                .position(|byte| *byte == delimiter)
                .map_or(bytes.len(), |at| begin + at);
            let mut at = begin;
            while at < end {
                let number_begin = at + 1;
                let underscore = bytes[number_begin..end]
                    .iter()
                    .position(|byte| *byte == b'_')
                    .map(|n| number_begin + n)
                    .ok_or_else(|| malformed("specifier underscore is absent"))?;
                let number = std::str::from_utf8(&bytes[number_begin..underscore])
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| malformed("specifier number is unreadable"))?;
                let slot = FIELDS
                    .iter()
                    .position(|(_, candidate)| *candidate == number)
                    .ok_or_else(|| malformed("specifier field is unknown"))?;
                let value_begin = underscore + 1;
                let value_end = bytes[value_begin..end]
                    .iter()
                    .position(|byte| *byte == b'@')
                    .map_or(end, |n| value_begin + n);
                let base = line.payload.start();
                let frame_end = if number == 14 { value_end } else { value_begin };
                append(
                    ctx,
                    framing,
                    cadmpeg_core::decode::u64_from_index(base + at)
                        ..cadmpeg_core::decode::u64_from_index(base + frame_end),
                )?;
                fields.0[slot] = Some(
                    line.payload
                        .child(base + value_begin, base + value_end)
                        .ok_or_else(|| malformed("specifier exceeds its original line"))?,
                );
                fields.1[slot] = Some((
                    line.payload
                        .child(base + at, base + value_end)
                        .ok_or_else(|| malformed("specifier exceeds its original line"))?,
                    base + value_begin,
                ));
                at = value_end;
            }
        }
        Ok(fields)
    }

    pub(super) fn values(&self, ctx: &DecodeContext<'_>) -> Result<DirectoryFields, CodecError> {
        let mut values = Vec::new();
        for (slot, value) in self.0.into_iter().enumerate() {
            let view = value.ok_or_else(|| {
                malformed(format!(
                    "Directory field {} has no explicit or inherited source value",
                    FIELDS[slot].1
                ))
            })?;
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(view.window().len()),
                "IGES inherited Directory field",
            )?;
            let mut bytes =
                ctx.alloc_filled(view.window().len(), 0u8, "IGES inherited Directory field")?;
            bytes.copy_from_slice(view.window());
            append(ctx, &mut values, std::rc::Rc::new(bytes))?;
        }
        Ok(DirectoryFields(values.try_into().map_err(|_| {
            malformed("Directory field count differs from16")
        })?))
    }

    /// The optional-field specifiers that the current record states: the
    /// field name, the whole specifier and the absolute start of its value.
    pub(super) fn optional_specifiers(
        &self,
    ) -> impl Iterator<Item = (&'static str, View<'a>, usize)> + '_ {
        OPTIONAL_FIELDS.into_iter().filter_map(|(number, name)| {
            let slot = FIELDS
                .iter()
                .position(|(_, candidate)| *candidate == number)?;
            let (specifier, value_begin) = self.1.get(slot).copied().flatten()?;
            Some((name, specifier, value_begin))
        })
    }

    pub(super) fn count(&self) -> Result<usize, CodecError> {
        let view = self.0[CompressedField::ParameterLineCount.slot()]
            .ok_or_else(|| malformed("Parameter line-count source is absent"))?;
        let count = std::str::from_utf8(view.window())
            .ok()
            .and_then(|text| text.trim().parse::<usize>().ok())
            .ok_or_else(|| malformed("Parameter line count is unreadable"))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
