// SPDX-License-Identifier: Apache-2.0
//! Directory Entry pairs and fixed status fields.

use crate::card::{CardScan, PhysicalLine, Section};
use cadmpeg_ir::codec::CodecError;
use std::collections::BTreeMap;

/// Four two-digit fields in the Directory Entry status number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Status {
    pub(crate) blank: u8,
    pub(crate) subordinate: u8,
    pub(crate) use_flag: u8,
    pub(crate) hierarchy: u8,
}

/// Lossless typed Directory Entry fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectoryEntry {
    pub(crate) sequence: u32,
    pub(crate) entity_type: i64,
    pub(crate) parameter_start: i64,
    pub(crate) structure: i64,
    pub(crate) line_font: i64,
    pub(crate) level: i64,
    pub(crate) view: i64,
    pub(crate) transform: i64,
    pub(crate) label_display: i64,
    pub(crate) status: Status,
    pub(crate) line_weight: i64,
    pub(crate) color: i64,
    pub(crate) parameter_line_count: i64,
    pub(crate) form: i64,
    pub(crate) reserved: [[u8; 8]; 2],
    pub(crate) label: [u8; 8],
    pub(crate) subscript: i64,
}

fn malformed(sequence: u32, message: impl Into<String>) -> CodecError {
    CodecError::Malformed(format!(
        "IGES Directory Entry D{sequence}: {}",
        message.into()
    ))
}

fn fields(line: &PhysicalLine) -> Result<[[u8; 8]; 9], CodecError> {
    let sequence = line.sequence.unwrap_or_default();
    let data = line
        .payload
        .get(..72)
        .ok_or_else(|| malformed(sequence, "card is shorter than 72 data bytes"))?;
    let mut fields = [[b' '; 8]; 9];
    for (target, source) in fields.iter_mut().zip(data.chunks_exact(8)) {
        target.copy_from_slice(source);
    }
    Ok(fields)
}

fn integer(field: [u8; 8], sequence: u32, name: &str) -> Result<i64, CodecError> {
    let text = std::str::from_utf8(&field)
        .map_err(|_| malformed(sequence, format!("{name} is not ASCII")))?
        .trim();
    if text.is_empty() {
        return Ok(0);
    }
    text.parse::<i64>()
        .map_err(|_| malformed(sequence, format!("{name} is not a decimal integer")))
}

fn status(field: [u8; 8], sequence: u32) -> Result<Status, CodecError> {
    if field.iter().any(|byte| !byte.is_ascii_digit()) {
        return Err(malformed(
            sequence,
            "status number is not eight decimal digits",
        ));
    }
    let pair = |at: usize| (field[at] - b'0') * 10 + field[at + 1] - b'0';
    Ok(Status {
        blank: pair(0),
        subordinate: pair(2),
        use_flag: pair(4),
        hierarchy: pair(6),
    })
}

fn parse_pair(first: &PhysicalLine, second: &PhysicalLine) -> Result<DirectoryEntry, CodecError> {
    let sequence = first.sequence.unwrap_or_default();
    if sequence % 2 != 1 || second.sequence != sequence.checked_add(1) {
        return Err(malformed(sequence, "cards are not an odd/even pair"));
    }
    let first_fields = fields(first)?;
    let second_fields = fields(second)?;
    let entity_type = integer(first_fields[0], sequence, "entity type")?;
    let repeated_type = integer(second_fields[0], sequence, "repeated entity type")?;
    if entity_type != repeated_type {
        return Err(malformed(
            sequence,
            format!("repeated entity type {repeated_type} does not equal {entity_type}"),
        ));
    }
    Ok(DirectoryEntry {
        sequence,
        entity_type,
        parameter_start: integer(first_fields[1], sequence, "Parameter Data start")?,
        structure: integer(first_fields[2], sequence, "structure")?,
        line_font: integer(first_fields[3], sequence, "line font")?,
        level: integer(first_fields[4], sequence, "level")?,
        view: integer(first_fields[5], sequence, "view")?,
        transform: integer(first_fields[6], sequence, "transformation")?,
        label_display: integer(first_fields[7], sequence, "label display")?,
        status: status(first_fields[8], sequence)?,
        line_weight: integer(second_fields[1], sequence, "line weight")?,
        color: integer(second_fields[2], sequence, "color")?,
        parameter_line_count: integer(second_fields[3], sequence, "Parameter Data count")?,
        form: integer(second_fields[4], sequence, "form")?,
        reserved: [second_fields[5], second_fields[6]],
        label: second_fields[7],
        subscript: integer(second_fields[8], sequence, "entity subscript")?,
    })
}

pub(crate) fn parse(scan: &CardScan) -> Result<Vec<DirectoryEntry>, CodecError> {
    let lines = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Directory))
        .collect::<Vec<_>>();
    if lines.len() % 2 != 0 {
        return Err(CodecError::Malformed(
            "IGES Directory Entry section has an unpaired card".into(),
        ));
    }
    lines
        .chunks_exact(2)
        .map(|pair| parse_pair(pair[0], pair[1]))
        .collect()
}

pub(crate) fn summary_notes(entries: &[DirectoryEntry]) -> Vec<String> {
    let mut census = BTreeMap::<(i64, i64), usize>::new();
    for entry in entries {
        *census.entry((entry.entity_type, entry.form)).or_default() += 1;
    }
    std::iter::once(format!("entities={}", entries.len()))
        .chain(census.into_iter().map(|((entity_type, form), count)| {
            format!("entity.{entity_type}.form.{form}={count}")
        }))
        .collect()
}
