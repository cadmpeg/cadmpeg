// SPDX-License-Identifier: Apache-2.0
//! Directory Entry pairs and fixed status fields.

use crate::card::{Card, CardScan, PhysicalLine, Section};

use crate::global::GlobalTable;
use crate::loss::IgesLossCode;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::SourceProvenance;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;

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
        match self {
            Self::EntityType => 0,
            Self::Structure => 1,
            Self::LineFont => 2,
            Self::Level => 3,
            Self::View => 4,
            Self::Transform => 5,
            Self::LabelDisplay => 6,
            Self::Status => 7,
            Self::LineWeight => 8,
            Self::Color => 9,
            Self::Form => 10,
            Self::ReservedFirst => 11,
            Self::ReservedSecond => 12,
            Self::Label => 13,
            Self::Subscript => 14,
        }
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
    fn parse(value: u8) -> Option<Self> {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub(crate) fn admitted_loss_provenance(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<cadmpeg_ir::SourceProvenance, CodecError> {
        let format = ctx.format_retained(format_args!("iges"), "iges loss source format")?;
        let tag = ctx.format_retained(
            format_args!("directory_entry:D{}", self.sequence),
            "iges loss directory tag",
        )?;
        Ok(cadmpeg_ir::SourceProvenance::in_stream(
            format,
            cadmpeg_ir::stream_name!("iges"),
            self.source_offset,
        )
        .with_tag(tag))
    }

    #[cfg(test)]
    pub(crate) fn loss_provenance(&self) -> cadmpeg_ir::SourceProvenance {
        cadmpeg_ir::SourceProvenance::in_stream(
            "iges",
            cadmpeg_ir::stream_name!("iges"),
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

#[derive(Debug)]
enum DirectoryParseError {
    Defect(DirectoryDefect),
    Refusal(CodecError),
}

impl From<DirectoryDefect> for DirectoryParseError {
    fn from(defect: DirectoryDefect) -> Self {
        Self::Defect(defect)
    }
}

impl From<CodecError> for DirectoryParseError {
    fn from(error: CodecError) -> Self {
        Self::Refusal(error)
    }
}

impl Serialize for DirectoryDefect {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

impl DirectoryDefect {
    fn key(self) -> &'static str {
        match self {
            Self::FieldNotAscii(_) => "field-not-ascii",
            Self::FieldNotAnInteger(_) => "field-not-an-integer",
            Self::FieldBlankNotAllowed(_) => "field-blank-not-allowed",
            Self::StatusNumberInvalid => "status-number-invalid",
            Self::RepeatedEntityTypeMismatch { .. } => "repeated-entity-type-mismatch",
            Self::UnpairedCard => "unpaired-card",
        }
    }
}

impl fmt::Display for DirectoryDefect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::FieldNotAscii(name) => write!(formatter, "the {name} field is not ASCII"),
            Self::FieldNotAnInteger(name) => {
                write!(formatter, "the {name} field is not a decimal integer")
            }
            Self::FieldBlankNotAllowed(name) => write!(
                formatter,
                "the {name} field is blank and IGES 4.0 defines no default"
            ),
            Self::StatusNumberInvalid => formatter
                .write_str("the status number is neither blank nor an eight-digit decimal integer"),
            Self::RepeatedEntityTypeMismatch { declared, repeated } => write!(
                formatter,
                "the repeated entity type {repeated} does not equal the entity type {declared}"
            ),
            Self::UnpairedCard => {
                formatter.write_str("the Directory Entry section ends with an unpaired card")
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
    pub(crate) fn identity(&self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        ctx.format_retained(
            format_args!("iges:quarantine:directory#{}", self.sequence),
            "iges directory quarantine identity",
        )
    }

    pub(crate) fn loss_note(&self, ctx: &DecodeContext<'_>) -> Result<LossNote, CodecError> {
        let message = ctx.format_retained(format_args!(
                "IGES directory-entry record D{} is quarantined because {}; its {} raw card(s) are retained and no typed field was interpreted",
                self.sequence,
                self.defect,
                self.cards()
            ), "iges directory quarantine loss message")?;
        let tag = ctx.format_retained(
            format_args!("directory_entry:D{}", self.sequence),
            "iges directory quarantine loss tag",
        )?;
        let code = IgesLossCode::DirectoryRecordQuarantined;
        ctx.charge_retained(
            4 + cadmpeg_core::decode::u64_from_index(code.code().len()),
            "iges directory quarantine loss kind",
        )?;
        ctx.charge_retained(4, "iges directory quarantine loss source format")?;
        Ok(code.note(message).with_provenance(
            SourceProvenance::in_stream(
                "iges",
                cadmpeg_ir::stream_name!("iges"),
                self.source_offset,
            )
            .with_tag(tag),
        ))
    }
}

fn fields(line: &PhysicalLine<'_>) -> [[u8; 8]; 9] {
    let mut fields = [[b' '; 8]; 9];
    for (target, source) in fields.iter_mut().zip(line.payload.chunks_exact(8)) {
        target.copy_from_slice(source);
    }
    fields
}

fn integer(
    field: [u8; 8],
    name: &'static str,
    ctx: &DecodeContext<'_>,
) -> Result<i64, DirectoryParseError> {
    let text = std::str::from_utf8(&field)
        .map_err(|_| DirectoryDefect::FieldNotAscii(name))?
        .trim();
    if text.is_empty() {
        return Ok(0);
    }
    ctx.parse_text::<i64>(text, "iges directory integer value")?
        .map_err(|_| DirectoryParseError::Defect(DirectoryDefect::FieldNotAnInteger(name)))
}

fn directory_integer(
    field: [u8; 8],
    name: &'static str,
    number: u8,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<i64, DirectoryParseError> {
    if matches!(global_table, GlobalTable::V4_0)
        && matches!(number, 1 | 2 | 11 | 14)
        && field.iter().all(|byte| *byte == b' ')
    {
        return Err(DirectoryParseError::Defect(
            DirectoryDefect::FieldBlankNotAllowed(name),
        ));
    }
    integer(field, name, ctx)
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
    first: &PhysicalLine<'_>,
    second: &PhysicalLine<'_>,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<DirectoryEntry, DirectoryParseError> {
    let first_fields = fields(first);
    let second_fields = fields(second);
    let entity_type = directory_integer(first_fields[0], "entity type", 1, global_table, ctx)?;
    let repeated_type = directory_integer(
        second_fields[0],
        "repeated entity type",
        11,
        global_table,
        ctx,
    )?;
    if entity_type != repeated_type {
        return Err(DirectoryParseError::Defect(
            DirectoryDefect::RepeatedEntityTypeMismatch {
                declared: entity_type,
                repeated: repeated_type,
            },
        ));
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
            ctx,
        )?,
        structure: directory_integer(first_fields[2], "structure", 3, global_table, ctx)?,
        line_font: directory_integer(first_fields[3], "line font", 4, global_table, ctx)?,
        level: directory_integer(first_fields[4], "level", 5, global_table, ctx)?,
        view: directory_integer(first_fields[5], "view", 6, global_table, ctx)?,
        transform: directory_integer(first_fields[6], "transformation", 7, global_table, ctx)?,
        label_display: directory_integer(first_fields[7], "label display", 8, global_table, ctx)?,
        status: status(first_fields[8], global_table)?,
        line_weight: directory_integer(second_fields[1], "line weight", 12, global_table, ctx)?,
        color: directory_integer(second_fields[2], "color", 13, global_table, ctx)?,
        parameter_line_count: directory_integer(
            second_fields[3],
            "Parameter Data count",
            14,
            global_table,
            ctx,
        )?,
        form: directory_integer(second_fields[4], "form", 15, global_table, ctx)?,
        reserved: [second_fields[5], second_fields[6]],
        label: second_fields[7],
        subscript: directory_integer(second_fields[8], "entity subscript", 19, global_table, ctx)?,
    })
}

/// Keep the one or two cards of a defective entry as its record bytes.
fn quarantine(
    first: &Card<'_>,
    second: Option<&Card<'_>>,
    defect: DirectoryDefect,
    ctx: &DecodeContext<'_>,
) -> Result<QuarantinedDirectoryRecord, CodecError> {
    let second = second.map_or(&[][..], |card| card.line.payload);
    let bytes_len = first
        .line
        .payload
        .len()
        .checked_add(second.len())
        .ok_or_else(|| ctx.refuse_codec_limit("iges quarantined directory bytes", u64::MAX, 1))?;
    let mut bytes = ctx.collection_vec(bytes_len, "iges quarantined directory bytes")?;
    ctx.extend_from_slice(
        &mut bytes,
        first.line.payload,
        "iges quarantined directory bytes",
    )?;
    ctx.extend_from_slice(&mut bytes, second, "iges quarantined directory bytes")?;
    Ok(QuarantinedDirectoryRecord {
        sequence: first.sequence,
        source_offset: first.line.offset,
        bytes,
        defect,
    })
}

/// Split the Directory Entry section into typed records and quarantined ones.
///
/// Entries come out in card order, so their sequences strictly increase; see
/// [`entry_by_sequence`].
pub(crate) fn parse(
    scan: &CardScan<'_>,
    global_table: GlobalTable,
    ctx: &DecodeContext<'_>,
) -> Result<(Vec<DirectoryEntry>, Vec<QuarantinedDirectoryRecord>), CodecError> {
    let cards = scan.section(Section::Directory);
    let mut entries = Vec::new();
    let mut quarantined = Vec::new();
    let mut pairs = cards.chunks_exact(2);
    while let Some(pair) = ctx.next_charged(&mut pairs, "iges directory card pairs")? {
        let [first, second] = pair else {
            continue;
        };
        ctx.charge_entities(1, "iges_directory_entries")?;
        match parse_pair(first.sequence, &first.line, &second.line, global_table, ctx) {
            Ok(entry) => {
                ctx.reserve_vec(&mut entries, 1, "iges directory entries")?;
                entries.push(entry);
            }
            Err(DirectoryParseError::Defect(defect)) => {
                ctx.reserve_vec(&mut quarantined, 1, "iges quarantined directory entries")?;
                quarantined.push(quarantine(first, Some(second), defect, ctx)?);
            }
            Err(DirectoryParseError::Refusal(error)) => return Err(error),
        }
    }
    if let Some(unpaired) = pairs.remainder().first() {
        ctx.charge_entities(1, "iges_directory_entries")?;
        ctx.reserve_vec(&mut quarantined, 1, "iges quarantined directory entries")?;
        quarantined.push(quarantine(
            unpaired,
            None,
            DirectoryDefect::UnpairedCard,
            ctx,
        )?);
    }
    Ok((entries, quarantined))
}

/// The entry with `sequence` in a directory whose sequences strictly increase,
/// as [`parse`] emits them and any filtering of its output keeps them.
pub(crate) fn entry_by_sequence<'a>(
    directory: &'a [DirectoryEntry],
    sequence: u32,
    ctx: &DecodeContext<'_>,
) -> Result<Option<&'a DirectoryEntry>, CodecError> {
    Ok(ctx
        .binary_search_by_key(
            directory,
            &sequence,
            |entry| Ok(entry.sequence),
            "iges directory sequence lookup",
        )?
        .ok()
        .and_then(|index| directory.get(index)))
}

pub(crate) fn summary_notes<'ctx>(
    entries: &[DirectoryEntry],
    ctx: &'ctx DecodeContext<'_>,
) -> Result<(Vec<String>, ScopedReservation<'ctx>), CodecError> {
    let mut census_storage = ctx.reserve_scoped(0, "iges directory summary groups")?;
    let mut census = BTreeMap::<(i64, i64), usize>::new();
    let mut source = entries.iter();
    while let Some(entry) = ctx.next_charged(&mut source, "iges directory summary groups")? {
        census_storage.with_storage(|| {
            ctx.admit_btree_entry(
                &census,
                &(entry.entity_type, entry.form),
                "iges directory summary groups",
            )?;
            *census.entry((entry.entity_type, entry.form)).or_default() += 1;
            Ok::<(), CodecError>(())
        })?;
    }
    let mut storage = ctx.reserve_scoped(0, "iges directory summary notes")?;
    let mut notes = Vec::new();
    ctx.reserve_scoped_vec(&mut storage, &mut notes, 1, "iges directory summary notes")?;
    notes.push(ctx.format_retained(
        format_args!("entities={}", entries.len()),
        "iges directory summary text",
    )?);
    let mut grouped = census.into_iter();
    while let Some(((entity_type, form), count)) =
        ctx.next_charged(&mut grouped, "iges directory summary notes")?
    {
        ctx.reserve_scoped_vec(&mut storage, &mut notes, 1, "iges directory summary notes")?;
        notes.push(ctx.format_retained(
            format_args!("entity.{entity_type}.form.{form}={count}"),
            "iges directory summary text",
        )?);
    }
    drop(grouped);
    drop(census_storage);
    Ok((notes, storage))
}

#[cfg(test)]
mod quarantine_tests;
#[cfg(test)]
mod tests;
