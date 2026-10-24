// SPDX-License-Identifier: Apache-2.0
//! Directory field admission from original fixed-column fields.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput};

use super::super::{directory_integer, status, GlobalTable, SourceStatus};

/// The typed value of one Directory Entry field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldValue {
    Integer(i64),
    Status(SourceStatus),
    Raw([u8; 8]),
}

/// How one Directory Entry field is admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldKind {
    /// A decimal integer.
    Integer,
    /// The eight-digit status number.
    Status,
    /// Text kept as its eight columns.
    Raw,
}

/// Name and kind of Directory field `number` (1-9 and 11-19).
pub(super) const fn field(number: u8) -> (&'static str, FieldKind) {
    match number {
        1 => ("entity type", FieldKind::Integer),
        2 => ("Parameter Data start", FieldKind::Integer),
        3 => ("structure", FieldKind::Integer),
        4 => ("line font", FieldKind::Integer),
        5 => ("level", FieldKind::Integer),
        6 => ("view", FieldKind::Integer),
        7 => ("transformation", FieldKind::Integer),
        8 => ("label display", FieldKind::Integer),
        9 => ("status", FieldKind::Status),
        11 => ("repeated entity type", FieldKind::Integer),
        12 => ("line weight", FieldKind::Integer),
        13 => ("color", FieldKind::Integer),
        14 => ("Parameter Data count", FieldKind::Integer),
        15 => ("form", FieldKind::Integer),
        16 | 17 => ("reserved", FieldKind::Raw),
        18 => ("label", FieldKind::Raw),
        _ => ("entity subscript", FieldKind::Integer),
    }
}

/// Admit one Directory field as one `iges.directory_field` part. Fields 2
/// and 14 locate the entity's Parameter Data and are wholly framing.
pub(super) fn admit_field(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    number: u8,
    global: GlobalTable,
    output: &mut ParserPartOutput,
) -> Result<Option<FieldValue>, CodecError> {
    let (name, kind) = field(number);
    let framing = if matches!(number, 2 | 14) {
        vec![
            cadmpeg_core::decode::u64_from_index(source.start())
                ..cadmpeg_core::decode::u64_from_index(source.end()),
        ]
    } else {
        Vec::new()
    };
    ctx.decode_parser_part(
        output,
        source,
        super::part_spec("iges.directory_field", framing)?,
        || {
            let bytes = source
                .read_slice(source.start()..source.end())
                .ok_or_else(|| CodecError::malformed("Directory field is unreadable"))?;
            let bytes = <[u8; 8]>::try_from(bytes).map_err(|_| {
                CodecError::malformed("Directory field width differs from eight columns")
            })?;
            Ok(match kind {
                FieldKind::Integer => {
                    FieldValue::Integer(directory_integer(bytes, name, number, global)?)
                }
                FieldKind::Status => FieldValue::Status(status(bytes)?),
                FieldKind::Raw => FieldValue::Raw(bytes),
            })
        },
    )
}

#[cfg(test)]
mod tests;
