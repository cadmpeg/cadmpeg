// SPDX-License-Identifier: Apache-2.0
//! Directory Entry pairs and fixed status fields.

use crate::card::{CardScan, PhysicalLine, Section};
use crate::loss::IgesLossCode;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::SourceProvenance;
use std::collections::BTreeMap;

/// Four two-digit fields in the Directory Entry status number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Status {
    pub(crate) blank: u8,
    pub(crate) subordinate: u8,
    pub(crate) use_flag: u8,
    pub(crate) hierarchy: u8,
}

impl Status {
    pub(crate) fn is_physically_dependent(self) -> bool {
        matches!(self.subordinate, 1 | 3)
    }

    pub(crate) fn is_logically_dependent(self) -> bool {
        matches!(self.subordinate, 2 | 3)
    }
}

/// Lossless typed Directory Entry fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectoryEntry {
    pub(crate) source_offset: u64,
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

impl DirectoryEntry {
    pub(crate) fn loss_provenance(&self) -> cadmpeg_ir::SourceProvenance {
        cadmpeg_ir::SourceProvenance {
            format: "iges".into(),
            stream: "iges".into(),
            offset: self.source_offset,
            tag: Some(format!("directory_entry:D{}", self.sequence)),
        }
    }
}

/// Why one Directory Entry record has no typed fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectoryDefect {
    FieldNotAscii(&'static str),
    FieldNotAnInteger(&'static str),
    StatusNumberInvalid,
    RepeatedEntityTypeMismatch { declared: i64, repeated: i64 },
    UnpairedCard,
}

impl DirectoryDefect {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::FieldNotAscii(_) => "field-not-ascii",
            Self::FieldNotAnInteger(_) => "field-not-an-integer",
            Self::StatusNumberInvalid => "status-number-invalid",
            Self::RepeatedEntityTypeMismatch { .. } => "repeated-entity-type-mismatch",
            Self::UnpairedCard => "unpaired-card",
        }
    }

    fn describe(self) -> String {
        match self {
            Self::FieldNotAscii(name) => format!("the {name} field is not ASCII"),
            Self::FieldNotAnInteger(name) => {
                format!("the {name} field is not a decimal integer")
            }
            Self::StatusNumberInvalid => {
                "the status number is neither blank nor an eight-digit decimal integer".to_owned()
            }
            Self::RepeatedEntityTypeMismatch { declared, repeated } => format!(
                "the repeated entity type {repeated} does not equal the entity type {declared}"
            ),
            Self::UnpairedCard => {
                "the Directory Entry section ends with an unpaired card".to_owned()
            }
        }
    }
}

/// One Directory Entry whose twenty typed fields were not recovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuarantinedDirectoryRecord {
    pub(crate) sequence: u32,
    pub(crate) source_offset: u64,
    pub(crate) cards: usize,
    pub(crate) bytes: Vec<u8>,
    pub(crate) defect: DirectoryDefect,
}

impl QuarantinedDirectoryRecord {
    /// The stable native identity of this quarantined record.
    pub(crate) fn identity(&self) -> String {
        format!("iges:quarantine:directory#{}", self.sequence)
    }

    pub(crate) fn loss_note(&self) -> LossNote {
        IgesLossCode::DirectoryRecordQuarantined
            .note(format!(
                "IGES directory-entry record D{} is quarantined because {}; its {} raw card(s) are retained and no typed field was interpreted",
                self.sequence,
                self.defect.describe(),
                self.cards
            ))
            .with_provenance(SourceProvenance {
                format: "iges".into(),
                stream: "iges".into(),
                offset: self.source_offset,
                tag: Some(format!("directory_entry:D{}", self.sequence)),
            })
    }
}

fn fields(line: &PhysicalLine) -> [[u8; 8]; 9] {
    let mut fields = [[b' '; 8]; 9];
    for (target, source) in fields.iter_mut().zip(line.payload.chunks_exact(8)) {
        target.copy_from_slice(source);
    }
    fields
}

fn integer(field: [u8; 8], name: &'static str) -> Result<i64, DirectoryDefect> {
    let text = std::str::from_utf8(&field)
        .map_err(|_| DirectoryDefect::FieldNotAscii(name))?
        .trim();
    if text.is_empty() {
        return Ok(0);
    }
    text.parse::<i64>()
        .map_err(|_| DirectoryDefect::FieldNotAnInteger(name))
}

fn status(field: [u8; 8]) -> Result<Status, DirectoryDefect> {
    if field.iter().all(|byte| *byte == b' ') {
        return Ok(Status {
            blank: 0,
            subordinate: 0,
            use_flag: 0,
            hierarchy: 0,
        });
    }
    if field.iter().any(|byte| !byte.is_ascii_digit()) {
        return Err(DirectoryDefect::StatusNumberInvalid);
    }
    let digit = |at: usize| field[at] - b'0';
    let pair = |at: usize| digit(at) * 10 + digit(at + 1);
    Ok(Status {
        blank: pair(0),
        subordinate: pair(2),
        use_flag: pair(4),
        hierarchy: pair(6),
    })
}

fn parse_pair(
    first: &PhysicalLine,
    second: &PhysicalLine,
) -> Result<DirectoryEntry, DirectoryDefect> {
    let sequence = first.sequence.unwrap_or_default();
    let first_fields = fields(first);
    let second_fields = fields(second);
    let entity_type = integer(first_fields[0], "entity type")?;
    let repeated_type = integer(second_fields[0], "repeated entity type")?;
    if entity_type != repeated_type {
        return Err(DirectoryDefect::RepeatedEntityTypeMismatch {
            declared: entity_type,
            repeated: repeated_type,
        });
    }
    Ok(DirectoryEntry {
        source_offset: first.offset,
        sequence,
        entity_type,
        parameter_start: integer(first_fields[1], "Parameter Data start")?,
        structure: integer(first_fields[2], "structure")?,
        line_font: integer(first_fields[3], "line font")?,
        level: integer(first_fields[4], "level")?,
        view: integer(first_fields[5], "view")?,
        transform: integer(first_fields[6], "transformation")?,
        label_display: integer(first_fields[7], "label display")?,
        status: status(first_fields[8])?,
        line_weight: integer(second_fields[1], "line weight")?,
        color: integer(second_fields[2], "color")?,
        parameter_line_count: integer(second_fields[3], "Parameter Data count")?,
        form: integer(second_fields[4], "form")?,
        reserved: [second_fields[5], second_fields[6]],
        label: second_fields[7],
        subscript: integer(second_fields[8], "entity subscript")?,
    })
}

fn quarantine(cards: &[&PhysicalLine], defect: DirectoryDefect) -> QuarantinedDirectoryRecord {
    QuarantinedDirectoryRecord {
        sequence: cards
            .first()
            .and_then(|line| line.sequence)
            .unwrap_or_default(),
        source_offset: cards.first().map_or(0, |line| line.offset),
        cards: cards.len(),
        bytes: cards
            .iter()
            .flat_map(|line| line.payload.iter().copied())
            .collect(),
        defect,
    }
}

/// Split the Directory Entry section into typed records and quarantined ones.
pub(crate) fn parse(scan: &CardScan) -> (Vec<DirectoryEntry>, Vec<QuarantinedDirectoryRecord>) {
    let lines = scan
        .lines
        .iter()
        .filter(|line| line.section == Some(Section::Directory))
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut quarantined = Vec::new();
    let mut pairs = lines.chunks_exact(2);
    for pair in pairs.by_ref() {
        match parse_pair(pair[0], pair[1]) {
            Ok(entry) => entries.push(entry),
            Err(defect) => quarantined.push(quarantine(pair, defect)),
        }
    }
    if let Some(unpaired) = pairs.remainder().first() {
        quarantined.push(quarantine(&[unpaired], DirectoryDefect::UnpairedCard));
    }
    (entries, quarantined)
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

#[cfg(test)]
mod quarantine_tests;
#[cfg(test)]
mod tests;
