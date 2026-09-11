// SPDX-License-Identifier: Apache-2.0
//! Directory Entry pairs and fixed status fields.

use crate::card::{CardScan, PhysicalLine, Section};
use crate::global::GlobalTable;
use crate::loss::IgesLossCode;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::SourceProvenance;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

/// The stored directory fields shared by Binary and Compressed ASCII.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum DirectoryFieldSlot {
    EntityType,
    Structure,
    LineFont,
    Level,
    View,
    Transform,
    LabelDisplay,
    Status,
    LineWeight,
    Color,
    Form,
    ReservedFirst,
    ReservedSecond,
    Label,
    Subscript,
}

impl DirectoryFieldSlot {
    pub(crate) const fn slot(self) -> usize {
        self as usize
    }
}

/// Renders one right-justified eight-column Directory Entry field.
pub(crate) fn render_field(bytes: &[u8]) -> Result<[u8; 8], cadmpeg_core::CodecError> {
    if bytes.len() > 8 {
        return Err(cadmpeg_core::CodecError::malformed(
            "IGES Directory field exceeds eight columns",
        ));
    }
    let mut output = [b' '; 8];
    output[8 - bytes.len()..].copy_from_slice(bytes);
    Ok(output)
}

/// Source status fields. Undefined numeric values remain available to native serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct SourceStatus {
    #[serde(rename = "blank_status")]
    blank: u8,
    #[serde(rename = "subordinate_status")]
    subordinate: u8,
    use_flag: u8,
    #[serde(rename = "hierarchy_status")]
    hierarchy: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hierarchy {
    GlobalTopDown,
    GlobalDefer,
    Property,
}

impl Hierarchy {
    pub(crate) fn parse(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::GlobalTopDown),
            1 => Some(Self::GlobalDefer),
            2 => Some(Self::Property),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subordinate {
    Independent,
    Physically,
    Logically,
    Both,
}

impl Subordinate {
    pub(crate) fn parse(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Independent),
            1 => Some(Self::Physically),
            2 => Some(Self::Logically),
            3 => Some(Self::Both),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UseFlag {
    Geometry,
    Annotation,
    Definition,
    Other,
    LogicalPositional,
    Parametric,
    Construction,
}

impl UseFlag {
    pub(crate) fn parse(value: u8, global_table: GlobalTable) -> Option<Self> {
        match value {
            0 => Some(Self::Geometry),
            1 => Some(Self::Annotation),
            2 => Some(Self::Definition),
            3 => Some(Self::Other),
            4 => Some(Self::LogicalPositional),
            5 => Some(Self::Parametric),
            6 if !matches!(global_table, GlobalTable::V4_0) => Some(Self::Construction),
            _ => None,
        }
    }
}

impl SourceStatus {
    /// Whether the source blank status is visible (00).
    pub(crate) fn is_visible(self) -> bool {
        self.blank == 0
    }

    pub(crate) fn subordinate(self) -> Option<Subordinate> {
        Subordinate::parse(self.subordinate)
    }

    pub(crate) fn use_flag(self, global_table: GlobalTable) -> Option<UseFlag> {
        UseFlag::parse(self.use_flag, global_table)
    }

    pub(crate) fn hierarchy(self) -> Option<Hierarchy> {
        Hierarchy::parse(self.hierarchy)
    }

    pub(crate) fn use_flag_code(self) -> u8 {
        self.use_flag
    }

    pub(crate) fn is_physically_dependent(self) -> bool {
        matches!(
            self.subordinate(),
            Some(Subordinate::Physically | Subordinate::Both)
        )
    }

    pub(crate) fn is_logically_dependent(self) -> bool {
        matches!(
            self.subordinate(),
            Some(Subordinate::Logically | Subordinate::Both)
        )
    }

    #[cfg(test)]
    pub(crate) fn from_codes([blank, subordinate, use_flag, hierarchy]: [u8; 4]) -> Self {
        Self {
            blank,
            subordinate,
            use_flag,
            hierarchy,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_blank(&mut self, value: u8) {
        self.blank = value;
    }

    #[cfg(test)]
    pub(crate) fn set_subordinate(&mut self, value: u8) {
        self.subordinate = value;
    }

    #[cfg(test)]
    pub(crate) fn set_use_flag(&mut self, value: u8) {
        self.use_flag = value;
    }

    #[cfg(test)]
    pub(crate) fn set_hierarchy(&mut self, value: u8) {
        self.hierarchy = value;
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
    pub(crate) status: SourceStatus,
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
        cadmpeg_ir::SourceProvenance::in_stream(
            "iges",
            const { cadmpeg_ir::StreamName::literal("iges") },
            self.source_offset,
        )
        .with_tag(format!("directory_entry:D{}", self.sequence))
    }
}

/// Why one Directory Entry record has no typed fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectoryDefect {
    FieldNotAscii(&'static str),
    FieldNotAnInteger(&'static str),
    FieldBlankNotAllowed(&'static str),
    StatusNumberInvalid,
    RepeatedEntityTypeMismatch { declared: i64, repeated: i64 },
    UnpairedCard,
}

impl Serialize for DirectoryDefect {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

impl DirectoryDefect {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::FieldNotAscii(_) => "field-not-ascii",
            Self::FieldNotAnInteger(_) => "field-not-an-integer",
            Self::FieldBlankNotAllowed(_) => "field-blank-not-allowed",
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
            Self::FieldBlankNotAllowed(name) => {
                format!("the {name} field is blank and IGES 4.0 defines no default")
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
    pub(crate) bytes: Vec<u8>,
    pub(crate) defect: DirectoryDefect,
}

impl QuarantinedDirectoryRecord {
    pub(crate) fn cards(&self) -> usize {
        self.bytes.len() / crate::card::CARD_WIDTH
    }

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
                self.cards()
            ))
        .with_provenance(
            SourceProvenance::in_stream("iges", const { cadmpeg_ir::StreamName::literal("iges") }, self.source_offset)
                .with_tag(format!("directory_entry:D{}", self.sequence)),
        )
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

fn directory_integer(
    field: [u8; 8],
    name: &'static str,
    number: u8,
    global_table: GlobalTable,
) -> Result<i64, DirectoryDefect> {
    if matches!(global_table, GlobalTable::V4_0)
        && matches!(number, 1 | 2 | 11 | 14)
        && field.iter().all(|byte| *byte == b' ')
    {
        return Err(DirectoryDefect::FieldBlankNotAllowed(name));
    }
    integer(field, name)
}

fn status(field: [u8; 8], global_table: GlobalTable) -> Result<SourceStatus, DirectoryDefect> {
    if field.iter().all(|byte| *byte == b' ') {
        return Ok(SourceStatus {
            blank: 0,
            subordinate: 0,
            use_flag: 0,
            hierarchy: 0,
        });
    }
    let mut digits = [b'0'; 8];
    if matches!(
        global_table,
        GlobalTable::Legacy | GlobalTable::V4_0 | GlobalTable::V5_0
    ) {
        let first_digit = field
            .iter()
            .position(u8::is_ascii_digit)
            .ok_or(DirectoryDefect::StatusNumberInvalid)?;
        if field[..first_digit].iter().any(|byte| *byte != b' ')
            || field[first_digit..]
                .iter()
                .any(|byte| !byte.is_ascii_digit())
        {
            return Err(DirectoryDefect::StatusNumberInvalid);
        }
        digits[first_digit..].copy_from_slice(&field[first_digit..]);
    } else {
        if field.iter().any(|byte| !byte.is_ascii_digit()) {
            return Err(DirectoryDefect::StatusNumberInvalid);
        }
        digits = field;
    }
    let digit = |at: usize| digits[at] - b'0';
    let pair = |at: usize| digit(at) * 10 + digit(at + 1);
    Ok(SourceStatus {
        blank: pair(0),
        subordinate: pair(2),
        use_flag: pair(4),
        hierarchy: pair(6),
    })
}

fn parse_pair(
    sequence: u32,
    first: &PhysicalLine,
    second: &PhysicalLine,
    global_table: GlobalTable,
) -> Result<DirectoryEntry, DirectoryDefect> {
    let first_fields = fields(first);
    let second_fields = fields(second);
    let entity_type = directory_integer(first_fields[0], "entity type", 1, global_table)?;
    let repeated_type =
        directory_integer(second_fields[0], "repeated entity type", 11, global_table)?;
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
        parameter_start: directory_integer(
            first_fields[1],
            "Parameter Data start",
            2,
            global_table,
        )?,
        structure: directory_integer(first_fields[2], "structure", 3, global_table)?,
        line_font: directory_integer(first_fields[3], "line font", 4, global_table)?,
        level: directory_integer(first_fields[4], "level", 5, global_table)?,
        view: directory_integer(first_fields[5], "view", 6, global_table)?,
        transform: directory_integer(first_fields[6], "transformation", 7, global_table)?,
        label_display: directory_integer(first_fields[7], "label display", 8, global_table)?,
        status: status(first_fields[8], global_table)?,
        line_weight: directory_integer(second_fields[1], "line weight", 12, global_table)?,
        color: directory_integer(second_fields[2], "color", 13, global_table)?,
        parameter_line_count: directory_integer(
            second_fields[3],
            "Parameter Data count",
            14,
            global_table,
        )?,
        form: directory_integer(second_fields[4], "form", 15, global_table)?,
        reserved: [second_fields[5], second_fields[6]],
        label: second_fields[7],
        subscript: directory_integer(second_fields[8], "entity subscript", 19, global_table)?,
    })
}

fn quarantine(
    first: (u32, &PhysicalLine),
    rest: &[(u32, &PhysicalLine)],
    defect: DirectoryDefect,
) -> QuarantinedDirectoryRecord {
    QuarantinedDirectoryRecord {
        sequence: first.0,
        source_offset: first.1.offset,
        bytes: std::iter::once(first.1)
            .chain(rest.iter().map(|(_, line)| *line))
            .flat_map(|line| line.payload.iter().copied())
            .collect(),
        defect,
    }
}

/// Split the Directory Entry section into typed records and quarantined ones.
pub(crate) fn parse(
    scan: &CardScan,
    global_table: GlobalTable,
) -> (Vec<DirectoryEntry>, Vec<QuarantinedDirectoryRecord>) {
    let lines = scan.section(Section::Directory).collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut quarantined = Vec::new();
    let mut pairs = lines.chunks_exact(2);
    for pair in pairs.by_ref() {
        match parse_pair(pair[0].0, pair[0].1, pair[1].1, global_table) {
            Ok(entry) => entries.push(entry),
            Err(defect) => quarantined.push(quarantine(pair[0], &pair[1..], defect)),
        }
    }
    if let Some(unpaired) = pairs.remainder().first() {
        quarantined.push(quarantine(*unpaired, &[], DirectoryDefect::UnpairedCard));
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
