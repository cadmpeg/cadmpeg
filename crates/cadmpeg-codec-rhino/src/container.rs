// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino 3DM container scanning and summary construction.

use crate::loss::Diagnostics;
use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::ContainerSummary;

use crate::chunks::{
    checked_count_bytes, checksum_children_through_class_end, chunk_at, direct_checksum_ranges,
    parse_header, validate_eof, verify_checksum, verify_checksum_ranges, ArchiveVersion,
    BoundedReader, ChecksumStatus, FramingError, TCODE_CRC, TCODE_ENDOFFILE, TCODE_ENDOFTABLE,
    TCODE_SHORT,
};
use crate::instances::{parse_definitions, DefinitionScan};
use crate::layout::file_header;
use crate::objects::{
    degraded_object_record, parse_object_record, resolve_identities, ObjectRecord,
};
use crate::wire::Uuid;
/// Maximum direct table records retained or described in one document.
const TABLE_RECORD_CAP: usize = 1 << 20;

const TCODE_COMMENT: u32 = 0x0000_0001;
const TCODE_TABLE: u32 = 0x1000_0000;
const TCODE_PROPERTIES: u32 = 0x1000_0014;
const TCODE_SETTINGS: u32 = 0x1000_0015;
const TCODE_BITMAP: u32 = 0x1000_0016;
const TCODE_TEXTURE_MAPPING: u32 = 0x1000_0025;
const TCODE_MATERIAL: u32 = 0x1000_0010;
const TCODE_LINETYPE: u32 = 0x1000_0023;
const TCODE_LAYER: u32 = 0x1000_0011;
const TCODE_GROUP: u32 = 0x1000_0018;
const TCODE_OBSOLETE_LAYERSET: u32 = 0x1000_0024;
const TCODE_FONT: u32 = 0x1000_0019;
const TCODE_DIMSTYLE: u32 = 0x1000_0020;
const TCODE_LIGHT: u32 = 0x1000_0012;
const TCODE_HATCH_PATTERN: u32 = 0x1000_0022;
const TCODE_INSTANCE_DEFINITION: u32 = 0x1000_0021;
const TCODE_OBJECTS: u32 = 0x1000_0013;
const TCODE_HISTORY: u32 = 0x1000_0026;
const TCODE_USER: u32 = 0x1000_0017;

const TCODE_OBJECT_RECORD: u32 = 0x2000_8070;
const TCODE_OBJECT_RECORD_HISTORY: u32 = 0x0200_8074;
const TCODE_USER_TABLE_UUID: u32 = 0x2000_8080;
const TCODE_USER_TABLE_RECORD_HEADER: u32 = 0x2000_8082;
const TCODE_BITMAP_RECORD: u32 = 0x2000_8090;
const TCODE_MATERIAL_RECORD: u32 = 0x2000_8040;
const TCODE_LAYER_RECORD: u32 = 0x2000_8050;
const TCODE_LIGHT_RECORD: u32 = 0x2000_8060;
const TCODE_GROUP_RECORD: u32 = 0x2000_8073;
const TCODE_OBSOLETE_LAYERSET_RECORD: u32 = 0x2000_8079;
const TCODE_FONT_RECORD: u32 = 0x2000_8074;
const TCODE_DIMSTYLE_RECORD: u32 = 0x2000_8075;
const TCODE_INSTANCE_DEFINITION_RECORD: u32 = 0x2000_8076;
const TCODE_HATCH_PATTERN_RECORD: u32 = 0x2000_8077;
const TCODE_LINETYPE_RECORD: u32 = 0x2000_8078;
const TCODE_TEXTURE_MAPPING_RECORD: u32 = 0x2000_807a;
const TCODE_HISTORY_RECORD: u32 = 0x2000_807b;
const TCODE_REVISION_HISTORY: u32 = 0x2000_8021;
const TCODE_NOTES: u32 = 0x2000_8022;
const TCODE_PREVIEW: u32 = 0x2000_8023;
const TCODE_APPLICATION: u32 = 0x2000_8024;
const TCODE_COMPRESSED_PREVIEW: u32 = 0x2000_8025;
const TCODE_WRITER_VERSION: u32 = 0xa000_0026;
const TCODE_AS_FILE_NAME: u32 = 0x2000_8027;
const TCODE_UNITS: u32 = 0x2000_8031;
const TCODE_RENDER_MESH_SETTINGS: u32 = 0x2000_8032;
const TCODE_ANALYSIS_MESH_SETTINGS: u32 = 0x2000_8033;
const TCODE_ANNOTATION_SETTINGS: u32 = 0x2000_8034;
const TCODE_NAMED_PLANES: u32 = 0x2000_8035;
const TCODE_NAMED_VIEWS: u32 = 0x2000_8036;
const TCODE_VIEWS: u32 = 0x2000_8037;
const TCODE_CURRENT_LAYER: u32 = 0xa000_0038;
const TCODE_CURRENT_MATERIAL: u32 = 0x2000_8039;
const TCODE_CURRENT_COLOR: u32 = 0x2000_803a;
const TCODE_CURRENT_WIRE_DENSITY: u32 = 0xa000_003c;
const TCODE_RENDER_SETTINGS: u32 = 0x2000_803d;
const TCODE_GRID_DEFAULTS: u32 = 0x2000_803f;
const TCODE_MODEL_URL: u32 = 0x2000_8131;
const TCODE_CURRENT_FONT: u32 = 0xa000_0132;
const TCODE_CURRENT_DIMSTYLE: u32 = 0xa000_0133;
const TCODE_SETTINGS_ATTRIBUTES: u32 = 0x2000_8134;
const TCODE_PLUGIN_LIST: u32 = 0x2000_8135;
const TCODE_RENDER_USERDATA: u32 = 0x2000_8136;
const TCODE_HISTORICAL_UNUSED_SETTINGS: u32 = 0x2000_803e;
const TCODE_ANONYMOUS: u32 = 0x4000_8000;

/// A bounded record descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    /// Record typecode.
    pub(crate) typecode: u32,
    /// Complete chunk range, including header and checksum.
    pub(crate) range: std::ops::Range<usize>,
    form: RecordBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordBody {
    Short(i64),
    Long(std::ops::Range<usize>),
}

impl Record {
    pub(crate) fn short(typecode: u32, range: std::ops::Range<usize>, value: i64) -> Self {
        Self {
            typecode,
            range,
            form: RecordBody::Short(value),
        }
    }

    pub(crate) fn long(
        typecode: u32,
        range: std::ops::Range<usize>,
        body: std::ops::Range<usize>,
    ) -> Self {
        Self {
            typecode,
            range,
            form: RecordBody::Long(body),
        }
    }

    fn from_chunk(chunk: &crate::chunks::Chunk) -> Self {
        match chunk.short_value() {
            Some(value) => Self::short(chunk.typecode, chunk.range(), value),
            None => Self::long(chunk.typecode, chunk.range(), chunk.body()),
        }
    }

    pub(crate) fn body(&self) -> std::ops::Range<usize> {
        match &self.form {
            RecordBody::Short(_) => self.range.end..self.range.end,
            RecordBody::Long(body) => body.clone(),
        }
    }

    pub(crate) fn is_short(&self) -> bool {
        matches!(self.form, RecordBody::Short(_))
    }

    pub(crate) fn short_value(&self) -> Option<i64> {
        match self.form {
            RecordBody::Short(value) => Some(value),
            RecordBody::Long(_) => None,
        }
    }
}

/// A complete direct table record whose payload has no typed owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpaqueRecord {
    /// Containing table typecode.
    pub(crate) table_typecode: u32,
    /// Complete record descriptor.
    pub(crate) record: Record,
}

/// Losses and opaque records from one native-arena install pass.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeInstall {
    /// Losses from records that could not be transferred.
    pub(crate) losses: Vec<LossNote>,
    /// Complete records whose registered class payload was not admitted.
    pub(crate) opaque_records: Vec<OpaqueRecord>,
    /// Settings records that failed to parse, given up as their parts.
    pub(crate) give_ups: Vec<GivenUpPart>,
}

/// A table descriptor whose body is a strict sub-range of its chunk range.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Table {
    /// Table typecode.
    pub(crate) typecode: u32,
    /// Complete table chunk range.
    range: std::ops::Range<usize>,
    /// Table body range, excluding the table header and checksum.
    body: std::ops::Range<usize>,
    /// Chunk bytes outside the body: the header and any checksum.
    framing: NonZeroU32,
    /// Direct records in the table.
    pub(crate) records: Vec<Record>,
    /// Number of direct records, including compactly summarized records.
    record_count: usize,
    /// Object record typecode counts discovered without class parsing.
    object_typecodes: BTreeMap<u32, usize>,
}

impl Table {
    /// A table whose `body` lies strictly inside its chunk `range`.
    ///
    /// Absent when the body escapes the range, fills it exactly, or leaves
    /// more framing bytes than a `u32` counts.
    pub(crate) fn new(
        typecode: u32,
        range: std::ops::Range<usize>,
        body: std::ops::Range<usize>,
        records: Vec<Record>,
        record_count: usize,
        object_typecodes: BTreeMap<u32, usize>,
    ) -> Option<Self> {
        (body.start <= body.end && body.start >= range.start && body.end <= range.end)
            .then_some(())?;
        let framing = range.len().checked_sub(body.len())?;
        let framing = NonZeroU32::new(u32::try_from(framing).ok()?)?;
        Some(Self {
            typecode,
            range,
            body,
            framing,
            records,
            record_count,
            object_typecodes,
        })
    }

    /// Complete table chunk range.
    fn range(&self) -> &std::ops::Range<usize> {
        &self.range
    }

    /// Table body bytes inside `data`, absent when the body is out of view.
    fn body_bytes<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]> {
        data.get(self.body.clone())
    }

    /// Table body range, excluding the table header and checksum.
    fn body(&self) -> &std::ops::Range<usize> {
        &self.body
    }

    /// Table chunk bytes outside the body: the header and any checksum.
    fn framing(&self) -> NonZeroU32 {
        self.framing
    }
}

/// One independently framed span the scan could not read (#215 rule 1).
///
/// The bytes stay in the document through the materialized unknowns stub;
/// `error` is reported once in the part's `record.unreadable` loss, and
/// `dropped` names the neutral records the part built. A part whose end
/// cannot be found extends to the end of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GivenUpPart {
    /// The codec's part name (`rhino.table_chunk`, `rhino.table_record`).
    pub(crate) label: &'static str,
    /// Retained byte range, clamped to the source.
    pub(crate) range: std::ops::Range<usize>,
    /// The classes the part's bytes can hold (§1.3).
    pub(crate) classes: cadmpeg_ir::ArenaClasses,
    /// The table this part belongs to, when its chunk named one. Absent
    /// tables and unidentified chunks name no table.
    pub(crate) table: Option<u32>,
    /// The neutral records the part built, for its loss's `dropped` list.
    pub(crate) dropped: Vec<String>,
    /// The framing or structural failure that gave the part up.
    pub(crate) error: String,
}

/// The result of scanning a complete supported container.
///
/// `data` borrows the root bytes from the decode arena without copying them.
#[derive(Debug, Clone)]
pub(crate) struct Scan<'a> {
    /// Complete input bytes, borrowed from the session root view.
    pub(crate) data: &'a [u8],
    /// Parsed archive version.
    pub(crate) archive: ArchiveVersion,
    /// Tables in source order.
    pub(crate) tables: Vec<Table>,
    /// All object records in source order.
    pub(crate) objects: Vec<ObjectRecord>,
    /// Direct table records retained as opaque source data.
    pub(crate) opaque_records: Vec<OpaqueRecord>,
    /// Parsed instance definitions and recoverable definition diagnostics.
    pub(crate) definitions: DefinitionScan,
    /// Decoded built-in history records in source order.
    pub(crate) history: Vec<crate::history::HistoryRecord>,
    /// Recoverable checksum and unknown-record notes.
    pub(crate) warnings: Diagnostics,
    /// Typed metadata decoded from property, setting, and layer records.
    pub(crate) metadata: crate::settings::DocumentMetadata,
    /// Spans given up under #215 rule 1, in source order.
    pub(crate) give_ups: Vec<GivenUpPart>,
}

impl Scan<'_> {
    /// Derive the document scope by §1.1: a class is held when the document
    /// holds a record of it, or when the request includes it and a part of it
    /// was located. Container-only decodes deliver `{}` (§4.1).
    pub(crate) fn content_scope(
        &self,
        ir: &CadIr,
        requested: cadmpeg_ir::ContentScope,
    ) -> cadmpeg_ir::ContentScope {
        let mut model_part = self
            .tables
            .iter()
            .any(|table| match table_base(table.typecode) {
                TCODE_BITMAP
                | TCODE_TEXTURE_MAPPING
                | TCODE_MATERIAL
                | TCODE_LINETYPE
                | TCODE_LAYER
                | TCODE_OBSOLETE_LAYERSET
                | TCODE_GROUP
                | TCODE_FONT
                | TCODE_DIMSTYLE
                | TCODE_LIGHT
                | TCODE_HATCH_PATTERN
                | TCODE_INSTANCE_DEFINITION
                | TCODE_OBJECTS => true,
                TCODE_PROPERTIES => table.records.iter().any(|record| {
                    matches!(
                        record.typecode,
                        TCODE_REVISION_HISTORY
                            | TCODE_NOTES
                            | TCODE_PREVIEW
                            | TCODE_APPLICATION
                            | TCODE_COMPRESSED_PREVIEW
                    )
                }),
                TCODE_SETTINGS => table
                    .records
                    .iter()
                    .any(|record| expected_record(TCODE_SETTINGS, record.typecode)),
                _ => false,
            });
        let mut design_part = self
            .tables
            .iter()
            .any(|table| table_base(table.typecode) == TCODE_HISTORY)
            || self
                .objects
                .iter()
                .any(|object| self.object_history_part(object));
        // A located-but-given-up table still locates its band's parts (§1.1):
        // the bytes were found, so their classes stay in scope.
        for part in &self.give_ups {
            if let Some(table) = part.table {
                let (model, design) = table_scope_band(table);
                model_part |= model;
                design_part |= design;
            }
        }
        scope_from_parts(ir, model_part, design_part, requested)
    }

    fn object_history_part(&self, object: &ObjectRecord) -> bool {
        let range = object.range();
        let Ok(record) = chunk_at(self.data, range.start, range.end, self.archive, false) else {
            return false;
        };
        let mut offset = record.body().start;
        while offset < record.body().end {
            let Ok(child) = chunk_at(self.data, offset, record.body().end, self.archive, false)
            else {
                return false;
            };
            if child.typecode == TCODE_OBJECT_RECORD_HISTORY {
                return true;
            }
            offset = child.next_offset();
        }
        false
    }
}

/// Which scope band a located table belongs to for §1.1 derivation: the
/// model tables of [`Scan::content_scope`], with settings and properties
/// counted as model and history as design. User tables and unidentified
/// chunks name no band, mirroring the table match.
fn table_scope_band(table: u32) -> (bool, bool) {
    match table_base(table) {
        TCODE_BITMAP
        | TCODE_TEXTURE_MAPPING
        | TCODE_MATERIAL
        | TCODE_LINETYPE
        | TCODE_LAYER
        | TCODE_OBSOLETE_LAYERSET
        | TCODE_GROUP
        | TCODE_FONT
        | TCODE_DIMSTYLE
        | TCODE_LIGHT
        | TCODE_HATCH_PATTERN
        | TCODE_INSTANCE_DEFINITION
        | TCODE_OBJECTS
        | TCODE_SETTINGS
        | TCODE_PROPERTIES => (true, false),
        TCODE_HISTORY => (false, true),
        _ => (false, false),
    }
}

pub(crate) fn scope_from_parts(
    ir: &CadIr,
    model_part: bool,
    design_part: bool,
    requested: cadmpeg_ir::ContentScope,
) -> cadmpeg_ir::ContentScope {
    // §1.1: a class is held when the document holds a record of it, or when
    // the request includes it and a part of it was located. A populated arena
    // always forces its class: the wrapper enforces "populated implies held".
    let model_populated = ir
        .model
        .populated_arena(cadmpeg_ir::ArenaClass::Model)
        .is_some();
    let design_populated = ir
        .model
        .populated_arena(cadmpeg_ir::ArenaClass::Design)
        .is_some();
    cadmpeg_ir::ContentScope {
        model: model_populated || (requested.model && model_part),
        design: design_populated || (requested.design && design_part),
    }
}

/// Borrows the session root bytes after the shared input budget admitted them.
fn acquire(root: View<'_>) -> &[u8] {
    root.window()
}

fn framing_error(error: FramingError) -> CodecError {
    match error {
        FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
        FramingError::Truncated { offset, .. } => CodecError::truncated(
            cadmpeg_core::decode::SourceLocation {
                space: cadmpeg_core::decode::SpaceId::ROOT,
                offset: cadmpeg_core::decode::u64_from_index(offset),
            },
            "rhino chunk framing",
        ),
        other => CodecError::Malformed(other.to_string()),
    }
}

/// Record one given-up part (#215 rule 1). The range is clamped to the
/// source; a part whose end cannot be found extends to the end of the source.
#[allow(clippy::too_many_arguments)]
pub(crate) fn give_up(
    give_ups: &mut Vec<GivenUpPart>,
    label: &'static str,
    offset: usize,
    end: usize,
    data_len: usize,
    classes: cadmpeg_ir::ArenaClasses,
    table: Option<u32>,
    dropped: Vec<String>,
    error: impl std::fmt::Display,
) {
    let start = offset.min(data_len);
    let end = end.clamp(start, data_len);
    give_ups.push(GivenUpPart {
        label,
        range: start..end,
        classes,
        table,
        dropped,
        error: error.to_string(),
    });
}

/// The classes one unread table chunk can hold: a table frames model or
/// design records (#215 rule 1, §1.3).
fn table_chunk_classes() -> cadmpeg_ir::ArenaClasses {
    cadmpeg_ir::ArenaClasses::from([
        cadmpeg_ir::ArenaClass::Model,
        cadmpeg_ir::ArenaClass::Design,
    ])
}

/// The classes one unread settings, objects or properties table record can
/// hold: those tables are model-band (#218 check 2, §1.3).
pub(crate) fn model_classes() -> cadmpeg_ir::ArenaClasses {
    cadmpeg_ir::ArenaClasses::from([cadmpeg_ir::ArenaClass::Model])
}

/// The classes one unread table record can hold, by its table: history
/// records are design-band, every other table is model-band (§1.3).
fn table_record_classes(table_typecode: u32) -> cadmpeg_ir::ArenaClasses {
    if table_base(table_typecode) == TCODE_HISTORY {
        cadmpeg_ir::ArenaClasses::from([cadmpeg_ir::ArenaClass::Design])
    } else {
        model_classes()
    }
}

/// Give up one settings-table record that failed to parse as a
/// `rhino.table_record` part (#215 rule 1, #223 item 9). `dropped` names the
/// typed record the uncut document holds for it.
pub(crate) fn settings_record_give_up(
    give_ups: &mut Vec<GivenUpPart>,
    data_len: usize,
    table_typecode: u32,
    record: &Record,
    dropped: Vec<String>,
    error: &FramingError,
) {
    give_up(
        give_ups,
        "rhino.table_record",
        record.range.start,
        record.range.end,
        data_len,
        table_record_classes(table_typecode),
        Some(table_base(table_typecode)),
        dropped,
        error,
    );
}

/// Mints the stable unknown-record ID for source order.
pub(crate) fn mint_unknown_id(source_order: usize) -> cadmpeg_ir::ids::UnknownId {
    cadmpeg_ir::ids::UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "record"),
        cadmpeg_ir::ids::IdentityKey::zero_padded(
            cadmpeg_core::decode::u64_from_index(source_order),
            6,
        ),
    )
}

/// Mint dropped-record names for the records of a table chunk whose framing
/// failed after its header: the table kind comes from the header, and record
/// starts frame forward within the source. Returns the table family for scope
/// derivation with the names.
fn mint_table_span_dropped(
    data: &[u8],
    archive: ArchiveVersion,
    offset: usize,
    start_order: usize,
) -> (Option<u32>, Vec<String>) {
    let Some(typecode) = data
        .get(offset..offset + 4)
        .and_then(|window| window.try_into().ok())
        .map(u32::from_le_bytes)
    else {
        return (None, Vec::new());
    };
    if typecode & 0xFFFF_0000 != 0x1000_0000 {
        return (None, Vec::new());
    }
    let table = table_base(typecode);
    if table != TCODE_OBJECTS {
        return (Some(table), Vec::new());
    }
    let width = if archive.uses_eight_byte_values() {
        8
    } else {
        4
    };
    let body_start = offset + 4 + width;
    // Even when the body starts beyond the source, the first record starts
    // at the body: name it so a truncated first record still drops by name.
    // The body start derives from the shared prefix, so it matches the full
    // decode's record start.
    if body_start >= data.len() {
        return (
            Some(table),
            mint_object_span_dropped(start_order, &[body_start]),
        );
    }
    let starts = scan_record_starts(data, archive, body_start, data.len());
    // Even when no record header frames, the first record starts at the
    // table body: name it so a truncated first record still drops by name.
    let starts = if starts.is_empty() && body_start < data.len() {
        vec![body_start]
    } else {
        starts
    };
    (Some(table), mint_object_span_dropped(start_order, &starts))
}

/// Frame the record header at `offset` without trusting its declared extent:
/// returns the record start and the next record start, or `None` when no
/// header is present. A record whose declared end exceeds the source still
/// reports its start: its bytes begin a record even when its end cannot be
/// found.
fn frame_record_start(
    data: &[u8],
    archive: ArchiveVersion,
    offset: usize,
) -> Option<(usize, Option<usize>)> {
    let mut reader = crate::chunks::BoundedReader::new(data, offset, data.len()).ok()?;
    let typecode = reader.u32().ok()?;
    let short = typecode & TCODE_SHORT != 0;
    let width = if archive.uses_eight_byte_values() {
        8
    } else {
        4
    };
    let value = if width == 8 {
        reader.i64().ok()?
    } else if !short
        || matches!(
            typecode,
            0x8000_0001 | 0x8000_0002 | 0xa000_0026 | 0x8200_0071
        )
    {
        i64::from(reader.u32().ok()?)
    } else {
        i64::from(reader.i32().ok()?)
    };
    let body_start = reader.position();
    if short || value < 0 {
        return Some((offset, Some(body_start)));
    }
    let declared_end = body_start.checked_add(usize::try_from(value).ok()?)?;
    if declared_end > data.len() {
        return Some((offset, None));
    }
    Some((offset, Some(declared_end)))
}

/// Collect record starts in `[start, end)`, stopping after the first
/// record whose end cannot be found.
fn scan_record_starts(
    data: &[u8],
    archive: ArchiveVersion,
    mut pos: usize,
    end: usize,
) -> Vec<usize> {
    let mut starts = Vec::new();
    while pos < end.min(data.len()) && starts.len() < 65_536 {
        match frame_record_start(data, archive, pos) {
            None => break,
            Some((start, None)) => {
                starts.push(start);
                break;
            }
            Some((start, Some(next))) if next > pos => {
                starts.push(start);
                pos = next;
            }
            _ => break,
        }
    }
    starts
}

/// Mint dropped-record names for objects whose records start at `starts`.
/// Orders and fallback keys derive from framing alone; uuid-keyed records
/// need attributes the truncated span may not hold, so only the stub and the
/// order-keyed homes name those. Over-minting is safe: the comparison only
/// permits the named records to disappear.
fn mint_object_span_dropped(start_order: usize, starts: &[usize]) -> Vec<String> {
    const KEYED: &[&str] = &[
        "rhino:object:body",
        "rhino:object:region",
        "rhino:object:shell",
        "rhino:object:point",
        "rhino:object:vertex",
        "rhino:object:curve",
        "rhino:object:surface",
        "rhino:object:procedural-surface",
        "rhino:object:procedural-curve",
        "rhino:object:tessellation",
        "rhino:object:subd",
        "rhino:object:edge",
        "rhino:object:loop",
        "rhino:object:face",
        "rhino:object:coedge",
        "rhino:object:pcurve",
    ];
    const FEATURES: &[&str] = &[
        "rhino:hatch:feature",
        "rhino:polyedge:feature",
        "rhino:detail:feature",
        "rhino:cage:feature",
        "rhino:curve-on-surface:feature",
    ];
    let mut dropped = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let order = start_order + index;
        dropped.push(mint_unknown_id(order).to_string());
        let key = format!("record-{order:06}-offset-{start}");
        for namespace in KEYED {
            dropped.push(format!("{namespace}#{key}"));
        }
        for namespace in FEATURES {
            dropped.push(format!("{namespace}#{key}"));
        }
        dropped.push(format!("rhino:dimension:annotation#{key}"));
        dropped.push(format!("rhino:object:curve#{key}.detail-boundary"));
        dropped.push(format!("rhino:object:curve#{key}.curve-on-surface-c2"));
        dropped.push(format!("rhino:object:curve#{key}.curve-on-surface-c3"));
        dropped.push(format!("rhino:object:curve#{key}.directrix"));
        dropped.push(format!("rhino:object:body#{key}.caps"));
        dropped.push(format!("rhino:object:body#{key}.cap-0"));
        dropped.push(format!("rhino:object:body#{key}.cap-1"));
        dropped.push(format!("rhino:object:surface#{key}.cap-0"));
        dropped.push(format!("rhino:object:surface#{key}.cap-1"));
        for lateral in 0..8 {
            dropped.push(format!("rhino:object:surface#{key}.lateral-{lateral}"));
            dropped.push(format!("rhino:object:tessellation#{key}.cache-{lateral}"));
        }
        // Bounded index families: compound-curve components (nested two
        // deep), brep C3 slots, extrusion profiles, hatch loops, trims,
        // singular edges and point-cloud points. Over-minting is safe.
        for i in 0..16 {
            dropped.push(format!("rhino:object:curve#{key}.c3-{i}"));
            dropped.push(format!("rhino:object:curve#{key}.root.component-{i}"));
            dropped.push(format!("rhino:object:curve#{key}.component-{i}"));
            dropped.push(format!("rhino:object:curve#{key}.hatch-loop-{i}"));
            dropped.push(format!("rhino:object:curve#{key}.profile-{i}.start"));
            dropped.push(format!("rhino:object:curve#{key}.profile-{i}.end"));
            dropped.push(format!("rhino:object:pcurve#{key}.trim-{i}"));
            dropped.push(format!("rhino:object:edge#{key}.singular-{i}"));
            for j in 0..8 {
                dropped.push(format!("rhino:object:curve#{key}.cap-{i}.profile-{j}"));
                dropped.push(format!("rhino:object:pcurve#{key}.cap-{i}.profile-{j}"));
                dropped.push(format!("rhino:object:point#{key}.cap-{i}.profile-{j}"));
                dropped.push(format!(
                    "rhino:object:curve#{key}.root.component-{i}.component-{j}"
                ));
                dropped.push(format!(
                    "rhino:object:curve#{key}.component-{i}.component-{j}"
                ));
                dropped.push(format!(
                    "rhino:object:curve#{key}.profile-{i}.start.sweep-origin"
                ));
                dropped.push(format!(
                    "rhino:object:curve#{key}.cap-{i}.profile-{j}.sweep-origin"
                ));
                dropped.push(format!("rhino:object:surface#{key}.slot-{j}"));
            }
            dropped.push(format!("rhino:object:point#{key}.vertex-{i}"));
        }
        // Bounded index families: compound-curve components, brep C3 slots,
        // hatch loops and point-cloud points. Over-minting is safe.
        for i in 0..16 {
            dropped.push(format!("rhino:object:curve#{key}.c3-{i}"));
            dropped.push(format!("rhino:object:curve#{key}.root.component-{i}"));
            dropped.push(format!("rhino:object:curve#{key}.component-{i}"));
            dropped.push(format!("rhino:object:curve#{key}.hatch-loop-{i}"));
        }
        for i in 0..128 {
            dropped.push(format!("rhino:object:point#{key}.{i}"));
        }
        dropped.push(format!("rhino:model:occurrence#{order}"));
        dropped.push(format!("rhino:product:occurrence#record-{order:06}"));
        dropped.push(format!("rhino:model:object-view#{order}-0"));
        dropped.push(format!("rhino:model:object-name#{order}-0"));
        dropped.push(format!("rhino:model:display-color-binding#{order}-0"));
        dropped.push(format!("rhino:model:display-color#object-{order}"));
    }
    dropped
}

/// Mint the unknowns-stub identity for a given-up part.
///
/// The address matches the #215 helper's scheme exactly: the codec, the part
/// label, the root stream and the part offset. When two parts of one scan
/// share all four (the two absent required tables at one walk end), the later
/// part takes a numeric nonce so every loss keeps its own home.
fn give_up_part_id(label: &str, offset: u64, nonce: u64) -> cadmpeg_ir::ids::UnknownId {
    use cadmpeg_core::decode::u64_from_index;
    let mut address = Vec::new();
    for field in [label.as_bytes(), <&[u8]>::default()] {
        address.extend_from_slice(&u64_from_index(field.len()).to_be_bytes());
        address.extend_from_slice(field);
    }
    address.extend_from_slice(&offset.to_be_bytes());
    if nonce != 0 {
        address.extend_from_slice(&nonce.to_be_bytes());
    }
    cadmpeg_ir::ids::UnknownId::try_from(format!(
        "rhino:source:part#{}",
        cadmpeg_ir::hash::sha256_hex(&address)
    ))
    .expect("given-up part identity is a valid unknown id")
}

/// One materialized given-up part: its stub identity, the classes its bytes
/// can hold, and the index of its `record.unreadable` loss in the report.
pub(crate) struct GiveUpMark {
    /// The unknowns-stub identity retaining the part's bytes.
    pub(crate) id: cadmpeg_ir::ids::UnknownId,
    /// The classes the part's bytes can hold (§1.3).
    pub(crate) classes: cadmpeg_ir::ArenaClasses,
}
/// Move the scan's given-up parts into their one home (#215 rule 1): one
/// unknowns stub with the part's bytes per part, and one `record.unreadable`
/// loss naming the part's kind, stream, offset, length and error.
///
/// Returns the stub identities with the classes each part's bytes can hold
/// and their loss indices. The caller marks them untyped after the records
/// are retained, since `mark_untyped` requires the record to exist, and fills
/// each loss's `dropped` list once projection has run.
pub(crate) fn materialize_give_ups(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    give_ups: &[GivenUpPart],
    losses: &mut Vec<LossNote>,
    unknowns: &mut Vec<cadmpeg_ir::unknown::UnknownRecord>,
) -> Result<Vec<GiveUpMark>, CodecError> {
    use cadmpeg_core::decode::u64_from_index;
    let mut marks = Vec::new();
    let mut nonces: std::collections::BTreeMap<(String, u64), u64> =
        std::collections::BTreeMap::new();
    for part in give_ups {
        let range = part.range.start.min(data.len())..part.range.end.min(data.len());
        let bytes = &data[range];
        let byte_len = u64_from_index(bytes.len());
        ctx.retain_bytes(byte_len, "given-up part source bytes")?;
        let mut data = ctx.alloc_filled(bytes.len(), 0_u8, "given-up part source bytes")?;
        data.copy_from_slice(bytes);
        let offset = u64_from_index(part.range.start);
        let key = (part.label.to_owned(), offset);
        let counter = nonces.entry(key).or_insert(0);
        let id = give_up_part_id(part.label, offset, *counter);
        *counter += 1;
        unknowns.push(cadmpeg_ir::unknown::UnknownRecord::retained(
            id.clone(),
            offset,
            data,
            Vec::new(),
        ));
        let mut provenance = cadmpeg_ir::SourceProvenance::root("rhino", offset);
        let length = u64_from_index(part.range.len());
        if length != 0 {
            provenance = provenance.with_byte_len(length)?;
        }
        let mut loss = cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
            .note(format!(
                "part {} stream=None offset={offset} length={length}: {}; dropped=[{}]",
                part.label,
                part.error,
                part.dropped.join(", ")
            ))
            .with_provenance(provenance);
        loss.dropped = part.dropped.clone();
        losses.push(loss);
        marks.push(GiveUpMark {
            id,
            classes: part.classes.clone(),
        });
    }
    Ok(marks)
}

fn checksum_children_warning(typecode: u32, offset: usize, error: &FramingError) -> String {
    format!(
        "checksum child framing at offset {offset} for typecode {typecode:#x} could not be verified: {error}"
    )
}

fn checksum_warning(
    data: &[u8],
    typecode: u32,
    offset: usize,
    parent_end: usize,
    archive: ArchiveVersion,
) -> Result<Option<String>, CodecError> {
    let chunk = chunk_at(data, offset, parent_end, archive, false).map_err(framing_error)?;
    let status = if typecode & TCODE_TABLE != 0
        || matches!(
            typecode,
            TCODE_OBJECT_RECORD
                | TCODE_BITMAP_RECORD
                | TCODE_MATERIAL_RECORD
                | TCODE_LAYER_RECORD
                | TCODE_LIGHT_RECORD
                | TCODE_GROUP_RECORD
                | TCODE_OBSOLETE_LAYERSET_RECORD
                | TCODE_FONT_RECORD
                | TCODE_DIMSTYLE_RECORD
                | TCODE_INSTANCE_DEFINITION_RECORD
                | TCODE_HATCH_PATTERN_RECORD
                | TCODE_LINETYPE_RECORD
                | TCODE_TEXTURE_MAPPING_RECORD
                | TCODE_HISTORY_RECORD
        ) {
        crate::chunks::verify_checksum_ranges(
            data,
            &chunk,
            std::iter::empty::<std::ops::Range<usize>>(),
        )
    } else if matches!(
        typecode,
        TCODE_NAMED_PLANES | TCODE_NAMED_VIEWS | TCODE_VIEWS
    ) {
        let children = match list_checksum_children(data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if matches!(
        typecode,
        TCODE_RENDER_MESH_SETTINGS | TCODE_ANALYSIS_MESH_SETTINGS
    ) {
        let children = match mesh_checksum_children(data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_RENDER_SETTINGS {
        let children = match render_settings_checksum_children(data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_SETTINGS_ATTRIBUTES {
        let children = match settings_attributes_checksum_children(data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_PLUGIN_LIST {
        let children = match plugin_list_checksum_children(data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_RENDER_USERDATA {
        let children = match checksum_children_through_class_end(
            data,
            chunk.body().clone(),
            archive,
            "render-settings userdata",
        ) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_COMPRESSED_PREVIEW {
        let children = match compressed_preview_checksum_children(data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_USER_TABLE_UUID {
        let children = match user_table_uuid_checksum_children(data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else {
        verify_checksum(data, &chunk)
    }
    .map_err(framing_error)?;
    match status {
        ChecksumStatus::Mismatch { expected, actual } => Ok(Some(format!(
            "CRC mismatch at offset {offset} for typecode {typecode:#x}: expected {expected:#x}, got {actual:#x}"
        ))),
        _ => Ok(None),
    }
}

/// Returns the nested SubD-display chunk in a version 1.5-or-newer mesh
/// settings payload.
///
/// `ON_MeshParameters::Write()` writes the direct mesh fields first and then
/// calls `ON_SubDDisplayParameters::Write()`. Future minor versions keep that
/// child position; any later bytes remain direct suffix bytes.
fn mesh_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let mut children = Vec::new();
    if let Some(child) = mesh_subd_checksum_child(data, &mut reader, archive)? {
        children.push(child);
    }
    Ok(children)
}

/// Skips the direct mesh-parameter prefix and returns its nested `SubD` child.
fn mesh_subd_checksum_child(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Option<std::ops::Range<usize>>, FramingError> {
    let packed_version = reader.u8()?;
    if packed_version >> 4 != 1 || packed_version & 0x0f < 5 {
        return Ok(None);
    }

    for _ in 0..5 {
        reader.i32()?;
    }
    for _ in 0..4 {
        reader.f64()?;
    }
    for _ in 0..2 {
        reader.i32()?;
    }
    for _ in 0..4 {
        reader.f64()?;
    }
    reader.i32()?;
    reader.i32()?;
    reader.bool()?;
    reader.f64()?;
    reader.u8()?;
    reader.bool()?;

    Ok(Some(take_anonymous_checksum_child(
        data,
        reader,
        archive,
        "mesh SubD display parameters",
    )?))
}

/// Returns the modern anonymous render-settings child, when present.
///
/// Legacy V5 render settings are direct fields beginning with an integer
/// version. Modern V6-and-later settings begin with one anonymous chunk; a
/// direct suffix after that child remains part of the outer checksum.
fn render_settings_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    if View::u32_le_at(data, chunk.body().start) != Some(TCODE_ANONYMOUS) {
        return Ok(Vec::new());
    }
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let child =
        take_anonymous_checksum_child(data, &mut reader, archive, "modern render settings")?;
    Ok(vec![child])
}

/// Returns the complete nested chunks in a settings-attributes body.
fn settings_attributes_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let packed_version = reader.u8()?;
    if packed_version >> 4 != 1 {
        return Ok(Vec::new());
    }
    reader.f64()?;
    reader.take(4)?;
    for _ in 0..3 {
        reader.i32()?;
    }

    let minor = packed_version & 0x0f;
    let mut children = Vec::new();
    if minor >= 1 {
        let child = take_anonymous_checksum_child(
            data,
            &mut reader,
            archive,
            "settings-attributes page units",
        )?;

        children.push(child);
    }
    if minor >= 2 {
        reader.skip(16)?;
    }
    if minor >= 3 {
        reader.skip(24)?;
        let child = take_anonymous_checksum_child(
            data,
            &mut reader,
            archive,
            "settings-attributes earth anchor",
        )?;

        children.push(child);
    }
    if minor >= 4 {
        reader.bool()?;
    }
    if minor >= 5 {
        let child = take_anonymous_checksum_child(
            data,
            &mut reader,
            archive,
            "settings-attributes IO settings",
        )?;

        children.push(child);
    }
    if minor >= 6 {
        if let Some(child) = mesh_subd_checksum_child(data, &mut reader, archive)? {
            children.push(child);
        }
    }
    if minor >= 7 {
        reader.skip(16 * 6)?;
    }
    Ok(children)
}

/// Returns the deflate children in an `ON_WindowsBitmap::WriteCompressed`
/// preview payload.
///
/// The bitmap header is direct data. Each nonzero compressed-buffer record
/// contains a direct uncompressed size, buffer CRC, and method byte. Method 1
/// stores the deflate bytes in a complete anonymous CRC chunk; method 0 stores
/// the bytes directly. A non-contiguous bitmap writes a second buffer after a
/// palette-only first buffer.
fn compressed_preview_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    reader.i32()?;
    reader.i32()?;
    reader.i32()?;
    reader.i16()?;
    let bit_count = reader.u16()?;
    reader.i32()?;
    let image_size = reader.i32()?;
    reader.i32()?;
    reader.i32()?;
    let colors_used = reader.i32()?;
    reader.i32()?;

    if image_size < 0 || colors_used < 0 {
        return Ok(Vec::new());
    }
    let color_count = if colors_used != 0 {
        usize::try_from(colors_used).map_err(|_| FramingError::Overflow {
            offset: reader.position(),
        })?
    } else {
        match bit_count {
            1 => 2,
            4 => 16,
            8 => 256,
            _ => 0,
        }
    };
    let palette_size = color_count.checked_mul(4).ok_or(FramingError::Overflow {
        offset: reader.position(),
    })?;
    let image_size = usize::try_from(image_size).map_err(|_| FramingError::Overflow {
        offset: reader.position(),
    })?;
    let first_size = usize::try_from(reader.u32()?).map_err(|_| FramingError::Overflow {
        offset: reader.position(),
    })?;

    let mut children = Vec::new();
    let contiguous_size = palette_size
        .checked_add(image_size)
        .ok_or(FramingError::Overflow {
            offset: reader.position(),
        })?;
    if first_size == contiguous_size {
        if let Some(child) = compressed_preview_buffer_child(
            data,
            &mut reader,
            archive,
            first_size,
            "compressed preview buffer",
        )? {
            children.push(child);
        }
    } else if image_size > 0 && first_size == palette_size {
        if let Some(child) = compressed_preview_buffer_child(
            data,
            &mut reader,
            archive,
            first_size,
            "compressed preview palette buffer",
        )? {
            children.push(child);
        }
        let second_size = usize::try_from(reader.u32()?).map_err(|_| FramingError::Overflow {
            offset: reader.position(),
        })?;
        if second_size != image_size {
            return Ok(Vec::new());
        }
        if let Some(child) = compressed_preview_buffer_child(
            data,
            &mut reader,
            archive,
            second_size,
            "compressed preview image buffer",
        )? {
            children.push(child);
        }
    } else {
        return Ok(Vec::new());
    }

    Ok(children)
}

/// Reads one `WriteCompressedBuffer` prefix and returns its nested deflate
/// chunk, if method 1 is selected.
fn compressed_preview_buffer_child(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    size: usize,
    label: &str,
) -> Result<Option<std::ops::Range<usize>>, FramingError> {
    if size == 0 {
        return Ok(None);
    }
    reader.skip(4)?;
    let method_offset = reader.position();
    match reader.u8()? {
        0 => {
            reader.skip(size)?;
            Ok(None)
        }
        1 => Ok(Some(take_anonymous_checksum_child(
            data, reader, archive, label,
        )?)),
        method => Err(FramingError::structural(
            method_offset,
            format!("{label} has unsupported compression method {method}"),
        )),
    }
}

/// Takes one long anonymous child and records its complete range.
fn take_anonymous_checksum_child(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    label: &str,
) -> Result<std::ops::Range<usize>, FramingError> {
    let start = reader.position();
    let child = chunk_at(data, start, reader.end(), archive, false)?;
    if child.typecode != TCODE_ANONYMOUS || child.short() {
        return Err(FramingError::structural(
            start,
            format!("{label} must be an anonymous long chunk"),
        ));
    }
    reader.skip(child.next_offset() - start)?;
    Ok(child.range())
}

/// Returns the optional record-header child inside a user-table UUID record.
fn user_table_uuid_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    reader.skip(16)?;
    if reader.remaining() < 4
        || View::u32_le_at(data, reader.position()) != Some(TCODE_USER_TABLE_RECORD_HEADER)
    {
        return Ok(Vec::new());
    }

    let start = reader.position();
    let child = chunk_at(data, start, reader.end(), archive, false)?;
    if child.short() {
        return Err(FramingError::structural(
            start,
            "user-table record header must be a long chunk",
        ));
    }
    Ok(vec![child.range()])
}

/// Returns the complete nested chunks after a counted view-list prefix.
///
/// The list CRC covers the count and any direct suffix bytes, but not these
/// complete child chunks. A malformed child has no recoverable checksum range;
/// the owning view parser reports that framing failure separately.
fn list_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let count = View::i32_le_at(data, chunk.body().start).ok_or(FramingError::Truncated {
        offset: chunk.body().start,
        needed: 4,
    })?;
    let child_count = usize::try_from(count).map_err(|_| {
        FramingError::structural(chunk.body().start, "negative view-list child count")
    })?;
    let mut offset = chunk
        .body()
        .start
        .checked_add(4)
        .ok_or(FramingError::Overflow {
            offset: chunk.body().start,
        })?;
    if offset > chunk.body().end {
        return Err(FramingError::Truncated {
            offset: chunk.body().end,
            needed: offset - chunk.body().end,
        });
    }
    let mut children = Vec::new();
    for _ in 0..child_count {
        let child = chunk_at(data, offset, chunk.body().end, archive, false)?;
        children.push(child.range());
        offset = child.next_offset();
    }
    Ok(children)
}

/// Returns the complete plugin-reference chunks after the packed
/// version/count prefix.
///
/// The plugin-list CRC covers the prefix and any direct suffix bytes, but not
/// these complete anonymous child chunks.
fn plugin_list_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let packed_version = reader.u8()?;
    if packed_version >> 4 != 1 {
        return Ok(Vec::new());
    }
    let count_offset = reader.position();
    let child_count = checked_count_bytes(
        reader.i32()?,
        1,
        reader.remaining(),
        TABLE_RECORD_CAP,
        count_offset,
    )?;
    let mut children = Vec::new();
    for _ in 0..child_count {
        let start = reader.position();
        let child = chunk_at(data, start, reader.end(), archive, false)?;
        if child.typecode != TCODE_ANONYMOUS || child.short() {
            return Err(FramingError::structural(
                start,
                "plugin-list child must be an anonymous long chunk",
            ));
        }
        children.push(child.range());
        reader.skip(child.next_offset() - start)?;
    }
    Ok(children)
}

fn table_rank(typecode: u32) -> Option<u8> {
    // The obsolete layerset occupies the compatibility slot between layer and
    // group; it is not a second layer table and cannot appear elsewhere.
    Some(match typecode & !TCODE_CRC {
        TCODE_PROPERTIES => 1,
        TCODE_SETTINGS => 2,
        TCODE_BITMAP => 3,
        TCODE_TEXTURE_MAPPING => 4,
        TCODE_MATERIAL => 5,
        TCODE_LINETYPE => 6,
        TCODE_LAYER => 7,
        TCODE_OBSOLETE_LAYERSET => 8,
        TCODE_GROUP => 9,
        TCODE_FONT => 10,
        TCODE_DIMSTYLE => 11,
        TCODE_LIGHT => 12,
        TCODE_HATCH_PATTERN => 13,
        TCODE_INSTANCE_DEFINITION => 14,
        TCODE_OBJECTS => 15,
        TCODE_HISTORY => 16,
        TCODE_USER => 17,
        _ => return None,
    })
}

fn table_base(typecode: u32) -> u32 {
    typecode & !TCODE_CRC
}

fn retain_record_descriptors(typecode: u32) -> bool {
    table_base(typecode) != TCODE_USER
}

fn record_is_allowed(table: u32, record: u32, short: bool) -> bool {
    if !expected_record(table_base(table), record) {
        return false;
    }
    if !short {
        return true;
    }
    matches!(
        record,
        TCODE_WRITER_VERSION
            | TCODE_CURRENT_LAYER
            | TCODE_CURRENT_WIRE_DENSITY
            | TCODE_CURRENT_FONT
            | TCODE_CURRENT_DIMSTYLE
    )
}

fn expected_record(table: u32, record: u32) -> bool {
    match table {
        TCODE_BITMAP => record == TCODE_BITMAP_RECORD,
        TCODE_MATERIAL => record == TCODE_MATERIAL_RECORD,
        TCODE_LAYER => record == TCODE_LAYER_RECORD,
        TCODE_LIGHT => record == TCODE_LIGHT_RECORD,
        TCODE_GROUP => record == TCODE_GROUP_RECORD,
        TCODE_OBSOLETE_LAYERSET => record == TCODE_OBSOLETE_LAYERSET_RECORD,
        TCODE_FONT => record == TCODE_FONT_RECORD,
        TCODE_DIMSTYLE => record == TCODE_DIMSTYLE_RECORD,
        TCODE_INSTANCE_DEFINITION => record == TCODE_INSTANCE_DEFINITION_RECORD,
        TCODE_HATCH_PATTERN => record == TCODE_HATCH_PATTERN_RECORD,
        TCODE_LINETYPE => record == TCODE_LINETYPE_RECORD,
        TCODE_TEXTURE_MAPPING => record == TCODE_TEXTURE_MAPPING_RECORD,
        TCODE_HISTORY => record == TCODE_HISTORY_RECORD,
        TCODE_PROPERTIES => matches!(
            record,
            TCODE_REVISION_HISTORY
                | TCODE_NOTES
                | TCODE_PREVIEW
                | TCODE_APPLICATION
                | TCODE_COMPRESSED_PREVIEW
                | TCODE_WRITER_VERSION
                | TCODE_AS_FILE_NAME
        ),
        TCODE_SETTINGS => matches!(
            record,
            TCODE_UNITS
                | TCODE_RENDER_MESH_SETTINGS
                | TCODE_ANALYSIS_MESH_SETTINGS
                | TCODE_ANNOTATION_SETTINGS
                | TCODE_NAMED_PLANES
                | TCODE_NAMED_VIEWS
                | TCODE_VIEWS
                | TCODE_CURRENT_LAYER
                | TCODE_CURRENT_MATERIAL
                | TCODE_CURRENT_COLOR
                | TCODE_CURRENT_WIRE_DENSITY
                | TCODE_RENDER_SETTINGS
                | TCODE_GRID_DEFAULTS
                | TCODE_MODEL_URL
                | TCODE_CURRENT_FONT
                | TCODE_CURRENT_DIMSTYLE
                | TCODE_SETTINGS_ATTRIBUTES
                | TCODE_PLUGIN_LIST
                | TCODE_RENDER_USERDATA
                | TCODE_HISTORICAL_UNUSED_SETTINGS
        ),
        TCODE_OBJECTS => record == TCODE_OBJECT_RECORD,
        TCODE_USER => true,
        _ => false,
    }
}

fn known_record(record: u32) -> bool {
    expected_record(TCODE_PROPERTIES, record)
        || expected_record(TCODE_SETTINGS, record)
        || expected_record(TCODE_BITMAP, record)
        || expected_record(TCODE_TEXTURE_MAPPING, record)
        || expected_record(TCODE_MATERIAL, record)
        || expected_record(TCODE_LINETYPE, record)
        || expected_record(TCODE_LAYER, record)
        || expected_record(TCODE_GROUP, record)
        || expected_record(TCODE_OBSOLETE_LAYERSET, record)
        || expected_record(TCODE_FONT, record)
        || expected_record(TCODE_DIMSTYLE, record)
        || expected_record(TCODE_LIGHT, record)
        || expected_record(TCODE_HATCH_PATTERN, record)
        || expected_record(TCODE_INSTANCE_DEFINITION, record)
        || expected_record(TCODE_OBJECTS, record)
        || expected_record(TCODE_HISTORY, record)
}

/// Scan a V3/V4 or V5–V8 Rhino container.
pub(crate) fn scan<'a>(ctx: &DecodeContext<'_>, data: &'a [u8]) -> Result<Scan<'a>, CodecError> {
    scan_with_record_limit(ctx, data, TABLE_RECORD_CAP)
}

fn count_object_typecode(counts: &mut BTreeMap<u32, usize>, typecode: u32) {
    *counts.entry(typecode).or_insert(0) += 1;
}

/// Discover the writer's boolean encoding before interpreting object records.
/// Only the first properties table owns these semantics, wherever it occurs.
///
/// A chunk this probe cannot frame is given up as one `rhino.table_chunk`
/// part and the probe continues past it; the writer version stays unknown
/// (#223 item 8).
fn first_writer_version(
    data: &[u8],
    archive: ArchiveVersion,
    mut offset: usize,
    give_ups: &mut Vec<GivenUpPart>,
) -> Option<i64> {
    while offset < data.len() {
        let chunk = match chunk_at(data, offset, data.len(), archive, false) {
            Ok(chunk) => chunk,
            Err(error) => {
                give_up(
                    give_ups,
                    "rhino.table_chunk",
                    offset,
                    data.len(),
                    data.len(),
                    table_chunk_classes(),
                    None,
                    Vec::new(),
                    error,
                );
                return None;
            }
        };
        if chunk.typecode == TCODE_ENDOFFILE {
            return None;
        }
        if table_base(chunk.typecode) == TCODE_PROPERTIES {
            if chunk.short() {
                // A short chunk carries no length: the next boundary is
                // unknown, so the rest of the stream is given up.
                give_up(
                    give_ups,
                    "rhino.table_chunk",
                    offset,
                    data.len(),
                    data.len(),
                    table_chunk_classes(),
                    Some(TCODE_PROPERTIES),
                    Vec::new(),
                    "properties table must use long framing",
                );
                return None;
            }
            let mut value = None;
            let mut child_offset = chunk.body().start;
            while child_offset < chunk.body().end {
                let child = match chunk_at(data, child_offset, chunk.body().end, archive, false) {
                    Ok(child) => child,
                    Err(error) => {
                        give_up(
                            give_ups,
                            "rhino.table_chunk",
                            child_offset,
                            chunk.body().end,
                            data.len(),
                            table_chunk_classes(),
                            Some(TCODE_PROPERTIES),
                            Vec::new(),
                            error,
                        );
                        return None;
                    }
                };
                if child.typecode == TCODE_ENDOFTABLE {
                    break;
                }
                if child.typecode == TCODE_WRITER_VERSION && child.short() {
                    match child.value() {
                        Ok(value_read) => value = Some(value_read),
                        Err(error) => {
                            give_up(
                                give_ups,
                                "rhino.table_record",
                                child.header_start,
                                child.next_offset(),
                                data.len(),
                                model_classes(),
                                Some(TCODE_PROPERTIES),
                                Vec::new(),
                                error,
                            );
                        }
                    }
                }
                child_offset = child.next_offset();
            }
            return value;
        }
        offset = chunk.next_offset();
    }
    None
}

fn scan_with_record_limit<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    record_limit: usize,
) -> Result<Scan<'a>, CodecError> {
    let header = parse_header(data).map_err(framing_error)?;
    let archive = header.archive_version;
    let archive_start = header.start_offset;
    let comment_offset = archive_start + file_header::LEN;
    let mut warnings = Diagnostics::new();
    let mut give_ups = Vec::new();
    // The 32-byte start section is located once `parse_header` succeeds. A
    // chunk that cannot be framed after it gives up the rest of the stream
    // instead of refusing the file, so a truncated file still decodes its
    // prefix (#223 items 8 and 14).
    let mut offset = match chunk_at(data, comment_offset, data.len(), archive, false) {
        Ok(chunk) => {
            let first = Record::from_chunk(&chunk);
            if first.typecode & !TCODE_SHORT == TCODE_COMMENT {
                if first.is_short() {
                    warnings.push(format_args!(
                        "introductory comment uses short framing; comment text is unavailable"
                    ));
                }
                if let Some(note) =
                    checksum_warning(data, first.typecode, comment_offset, data.len(), archive)?
                {
                    warnings.push_coded(
                        crate::loss::RhinoLossCode::IntegrityFailure,
                        format_args!("{note}"),
                    );
                }
                first.range.end
            } else {
                warnings.push(format_args!(
                    "introductory comment is absent; reading tables from the first post-header chunk"
                ));
                comment_offset
            }
        }
        Err(error) => {
            give_up(
                &mut give_ups,
                "rhino.table_chunk",
                comment_offset,
                data.len(),
                data.len(),
                table_chunk_classes(),
                None,
                Vec::new(),
                error,
            );
            data.len()
        }
    };
    let writer_version = first_writer_version(data, archive, offset, &mut give_ups);
    let mut tables = Vec::new();
    let mut last_rank = 0_u8;
    let mut saw_user = false;
    let mut saw_properties = false;
    let mut saw_settings = false;
    let mut saw_objects = false;
    let mut saw_end_of_file = false;
    let mut all_objects = Vec::new();

    let mut opaque_records = Vec::new();
    let mut definitions = DefinitionScan::default();
    let mut history = Vec::new();
    let mut record_count = 0_usize;
    while offset < data.len() {
        let chunk = match chunk_at(data, offset, data.len(), archive, false) {
            Ok(chunk) => chunk,
            Err(error) => {
                let (table, dropped) =
                    mint_table_span_dropped(data, archive, offset, all_objects.len());
                give_up(
                    &mut give_ups,
                    "rhino.table_chunk",
                    offset,
                    data.len(),
                    data.len(),
                    table_chunk_classes(),
                    table,
                    dropped,
                    error,
                );
                break;
            }
        };
        if chunk.typecode == TCODE_ENDOFFILE {
            if !saw_settings {
                give_up(
                    &mut give_ups,
                    "rhino.table_chunk",
                    offset,
                    offset,
                    data.len(),
                    model_classes(),
                    None,
                    Vec::new(),
                    "settings table is required but absent",
                );
            }
            if !saw_objects {
                give_up(
                    &mut give_ups,
                    "rhino.table_chunk",
                    offset,
                    offset,
                    data.len(),
                    model_classes(),
                    None,
                    Vec::new(),
                    "object table is required but absent",
                );
            }
            if let Err(error) = validate_eof(data, offset, archive) {
                give_up(
                    &mut give_ups,
                    "rhino.table_chunk",
                    offset,
                    chunk.next_offset(),
                    data.len(),
                    table_chunk_classes(),
                    Some(table_base(chunk.typecode)),
                    Vec::new(),
                    error,
                );
            }
            saw_end_of_file = true;
            break;
        }
        let rank = match table_rank(chunk.typecode) {
            Some(rank) => rank,
            None => {
                // An unknown top-level chunk typecode is one
                // `rhino.table_chunk` part, given up (#223 item 8).
                give_up(
                    &mut give_ups,
                    "rhino.table_chunk",
                    offset,
                    chunk.next_offset(),
                    data.len(),
                    table_chunk_classes(),
                    None,
                    Vec::new(),
                    format_args!("expected table or EOF at offset {offset}"),
                );
                offset = chunk.next_offset().min(data.len());
                continue;
            }
        };
        let duplicate_metadata_table = match table_base(chunk.typecode) {
            TCODE_PROPERTIES => saw_properties,
            TCODE_SETTINGS => saw_settings,
            _ => false,
        };
        match table_base(chunk.typecode) {
            TCODE_PROPERTIES => saw_properties = true,
            TCODE_SETTINGS => saw_settings = true,
            TCODE_OBJECTS => saw_objects = true,
            _ => {}
        }
        if chunk.short() {
            // A short chunk carries no length: the rest of the stream is
            // given up as this part (#223 item 8).
            give_up(
                &mut give_ups,
                "rhino.table_chunk",
                offset,
                data.len(),
                data.len(),
                table_chunk_classes(),
                Some(table_base(chunk.typecode)),
                Vec::new(),
                "table chunks must use long framing",
            );
            offset = data.len();
            break;
        }
        if matches!(
            table_base(chunk.typecode),
            TCODE_PROPERTIES | TCODE_SETTINGS
        ) {
            if duplicate_metadata_table || rank < last_rank || saw_user {
                warnings.push_coded(crate::loss::RhinoLossCode::DuplicateRecordResolved, format_args!("metadata table {:#x} is repeated or out of order; first table owns its metadata", chunk.typecode));
            }
        } else if table_base(chunk.typecode) == TCODE_USER {
            if !saw_user && rank < last_rank {
                give_up(
                    &mut give_ups,
                    "rhino.table_chunk",
                    offset,
                    chunk.next_offset(),
                    data.len(),
                    table_chunk_classes(),
                    Some(TCODE_USER),
                    Vec::new(),
                    "user table is out of order",
                );
                offset = chunk.next_offset().min(data.len());
                continue;
            }
            saw_user = true;
        } else {
            if saw_user || rank <= last_rank {
                let dropped = if table_base(chunk.typecode) == TCODE_OBJECTS {
                    let starts =
                        scan_record_starts(data, archive, chunk.body().start, chunk.body().end);
                    mint_object_span_dropped(all_objects.len(), &starts)
                } else {
                    Vec::new()
                };
                give_up(
                    &mut give_ups,
                    "rhino.table_chunk",
                    offset,
                    chunk.next_offset(),
                    data.len(),
                    table_chunk_classes(),
                    Some(table_base(chunk.typecode)),
                    dropped,
                    format_args!(
                        "table typecode {:#x} is out of order or duplicated",
                        chunk.typecode
                    ),
                );
                offset = chunk.next_offset().min(data.len());
                continue;
            }
            last_rank = rank;
        }
        let retain_records = retain_record_descriptors(chunk.typecode);
        let mut records = Vec::new();
        let mut table_record_count = 0_usize;
        let mut object_typecodes = BTreeMap::new();
        let mut child_offset = chunk.body().start;
        let mut terminated = false;
        while child_offset < chunk.body().end {
            let child = match chunk_at(data, child_offset, chunk.body().end, archive, false) {
                Ok(child) => child,
                Err(error) => {
                    // The record's end cannot be found: give up the rest of
                    // the table and continue the walk after it (#215).
                    let dropped = if table_base(chunk.typecode) == TCODE_OBJECTS {
                        mint_object_span_dropped(
                            all_objects.len(),
                            &scan_record_starts(data, archive, child_offset, chunk.body().end),
                        )
                    } else {
                        Vec::new()
                    };
                    give_up(
                        &mut give_ups,
                        "rhino.table_chunk",
                        child_offset,
                        chunk.body().end,
                        data.len(),
                        table_chunk_classes(),
                        Some(table_base(chunk.typecode)),
                        dropped,
                        error,
                    );
                    break;
                }
            };
            if child.typecode == TCODE_ENDOFTABLE {
                let marker_value = child.short().then(|| child.value()).transpose();
                let marker_value = match marker_value {
                    Ok(marker_value) => marker_value,
                    Err(error) => {
                        let dropped = if table_base(chunk.typecode) == TCODE_OBJECTS {
                            mint_object_span_dropped(
                                all_objects.len(),
                                &scan_record_starts(data, archive, child_offset, chunk.body().end),
                            )
                        } else {
                            Vec::new()
                        };
                        give_up(
                            &mut give_ups,
                            "rhino.table_chunk",
                            child_offset,
                            chunk.body().end,
                            data.len(),
                            table_chunk_classes(),
                            Some(table_base(chunk.typecode)),
                            dropped,
                            error,
                        );
                        break;
                    }
                };
                if !child.short() || marker_value != Some(0) {
                    give_up(
                        &mut give_ups,
                        "rhino.table_chunk",
                        child_offset,
                        chunk.body().end,
                        data.len(),
                        table_chunk_classes(),
                        Some(table_base(chunk.typecode)),
                        if table_base(chunk.typecode) == TCODE_OBJECTS {
                            mint_object_span_dropped(
                                all_objects.len(),
                                &scan_record_starts(data, archive, child_offset, chunk.body().end),
                            )
                        } else {
                            Vec::new()
                        },
                        "end-of-table marker must be short with value zero",
                    );
                    break;
                }
                if child.next_offset() != chunk.body().end {
                    give_up(
                        &mut give_ups,
                        "rhino.table_chunk",
                        child_offset,
                        chunk.body().end,
                        data.len(),
                        table_chunk_classes(),
                        Some(table_base(chunk.typecode)),
                        if table_base(chunk.typecode) == TCODE_OBJECTS {
                            mint_object_span_dropped(
                                all_objects.len(),
                                &scan_record_starts(data, archive, child_offset, chunk.body().end),
                            )
                        } else {
                            Vec::new()
                        },
                        "end-of-table marker is not the final table child",
                    );
                    break;
                }
                terminated = true;
                break;
            }
            record_count = record_count
                .checked_add(1)
                .filter(|count| *count <= record_limit)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "document table record budget of {record_limit} exceeded"
                    ))
                })?;
            table_record_count = table_record_count
                .checked_add(1)
                .filter(|count| *count <= record_limit)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "document table record budget of {record_limit} exceeded"
                    ))
                })?;
            let record = Record::from_chunk(&child);
            let opaque = table_base(chunk.typecode) == TCODE_USER
                || !record_is_allowed(chunk.typecode, record.typecode, record.is_short());
            if !record_is_allowed(chunk.typecode, record.typecode, record.is_short()) {
                if known_record(record.typecode)
                    && !expected_record(TCODE_PROPERTIES, record.typecode)
                    && !expected_record(TCODE_SETTINGS, record.typecode)
                {
                    // A damaged record is one `rhino.table_record` part,
                    // given up; the walk continues past it (#215).
                    give_up(
                        &mut give_ups,
                        "rhino.table_record",
                        child_offset,
                        child.next_offset(),
                        data.len(),
                        table_record_classes(chunk.typecode),
                        Some(table_base(chunk.typecode)),
                        Vec::new(),
                        format_args!(
                            "record typecode {:#x} is invalid or short-framed in table {:#x}",
                            record.typecode, chunk.typecode
                        ),
                    );
                    child_offset = child.next_offset().min(chunk.body().end);
                    continue;
                }
                warnings.push(format_args!(
                "unknown bounded record or inadmissible metadata {:#x} retained in table {:#x} at offset {child_offset}",
                record.typecode, chunk.typecode
                                ));
            }
            if let Some(note) = checksum_warning(
                data,
                record.typecode,
                child_offset,
                chunk.body().end,
                archive,
            )? {
                warnings.push_coded(
                    crate::loss::RhinoLossCode::IntegrityFailure,
                    format_args!("{note}"),
                );
            }
            if table_base(chunk.typecode) == TCODE_OBJECTS && record.typecode == TCODE_OBJECT_RECORD
            {
                let descriptor = match parse_object_record(
                    ctx,
                    data,
                    &record,
                    archive,
                    writer_version,
                    &mut warnings,
                ) {
                    Ok(descriptor) => descriptor,
                    Err(FramingError::Resource(limit)) => {
                        return Err(CodecError::ResourceLimit(limit));
                    }
                    Err(error) => {
                        warnings.push(format_args!(
                            "bounded object record at {child_offset} is malformed: {error}"
                        ));
                        degraded_object_record(&record, &error)
                    }
                };
                let typecode = descriptor.framed().map_or(0, |object| object.object_type);
                count_object_typecode(&mut object_typecodes, typecode);
                all_objects.push(descriptor);
            }
            if opaque || duplicate_metadata_table {
                opaque_records.push(OpaqueRecord {
                    table_typecode: chunk.typecode,
                    record: record.clone(),
                });
            }
            if retain_records && !opaque {
                records.push(record);
            }
            child_offset = child.next_offset();
        }
        if !terminated {
            warnings.push(format_args!(
                "table {:#x} has no end-of-table marker",
                chunk.typecode
            ));
        }
        if let Some(note) =
            checksum_warning(data, chunk.typecode, offset, chunk.next_offset(), archive)?
        {
            warnings.push_coded(
                crate::loss::RhinoLossCode::IntegrityFailure,
                format_args!("{note}"),
            );
        }
        if table_base(chunk.typecode) == TCODE_INSTANCE_DEFINITION {
            let parsed = parse_definitions(ctx, data, &records, archive, chunk.typecode)?;
            definitions = parsed.scan;

            opaque_records.extend(parsed.opaque_records);
        }
        if table_base(chunk.typecode) == TCODE_HISTORY {
            let parsed = crate::history::parse_records(
                ctx,
                data,
                &records,
                archive,
                &mut warnings,
                chunk.typecode,
            )?;
            history = parsed.records;

            opaque_records.extend(parsed.opaque_records);
        }
        let table = match Table::new(
            chunk.typecode,
            offset..chunk.next_offset(),
            chunk.body(),
            records,
            table_record_count,
            object_typecodes,
        ) {
            Some(table) => table,
            None => {
                let dropped = if table_base(chunk.typecode) == TCODE_OBJECTS {
                    let starts = scan_record_starts(data, archive, chunk.body().start, data.len());
                    mint_object_span_dropped(all_objects.len(), &starts)
                } else {
                    Vec::new()
                };
                give_up(
                    &mut give_ups,
                    "rhino.table_chunk",
                    offset,
                    chunk.next_offset(),
                    data.len(),
                    table_chunk_classes(),
                    Some(table_base(chunk.typecode)),
                    dropped,
                    "table chunk declares a body that does not fit its framing",
                );
                offset = chunk.next_offset().min(data.len());
                continue;
            }
        };
        tables.push(table);
        offset = chunk.next_offset().min(data.len());
        if offset >= data.len() {
            // The table's declared end runs past the source: the rest of the
            // stream is already covered by this table's span.
            break;
        }
    }
    if !saw_end_of_file && (!saw_settings || !saw_objects) {
        // The walk ended without an end-of-file chunk (a cut source). Tables
        // that were never located are given up as their parts.
        if !saw_settings {
            give_up(
                &mut give_ups,
                "rhino.table_chunk",
                offset,
                offset,
                data.len(),
                model_classes(),
                None,
                Vec::new(),
                "settings table is required but absent",
            );
        }
        if !saw_objects {
            give_up(
                &mut give_ups,
                "rhino.table_chunk",
                offset,
                offset,
                data.len(),
                model_classes(),
                None,
                Vec::new(),
                "object table is required but absent",
            );
        }
    }
    let mut metadata = crate::settings::parse_metadata(ctx, data, archive, &tables, &mut warnings)?;
    let all_objects = resolve_identities(all_objects, &metadata, &mut warnings)?;

    opaque_records.append(&mut metadata.opaque_records);
    Ok(Scan {
        data,
        archive,
        tables,
        objects: all_objects,
        opaque_records,
        definitions,
        history,
        warnings,
        metadata,
        give_ups,
    })
}

/// Test-only: leak `data` so the borrowed [`Scan`] is `'static`.
#[cfg(test)]
pub(crate) fn scan_owned(data: Vec<u8>) -> Result<Scan<'static>, CodecError> {
    let data = Box::leak(data.into_boxed_slice());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )?;
    scan_with_record_limit(&ctx, data, TABLE_RECORD_CAP)
}

#[cfg(test)]
fn scan_with_test_record_limit(
    data: Vec<u8>,
    record_limit: usize,
) -> Result<Scan<'static>, CodecError> {
    let data = Box::leak(data.into_boxed_slice());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )?;
    scan_with_record_limit(&ctx, data, record_limit)
}

fn insert_summary_attribute(
    attributes: &mut BTreeMap<String, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) {
    let key = key.to_string();
    let value = value.to_string();
    attributes.insert(key, value);
}

fn push_container_note(notes: &mut Vec<String>, value: std::fmt::Arguments<'_>) {
    notes.push(value.to_string());
}

/// Build the format-neutral container summary.
fn summarize(scan: &Scan<'_>) -> cadmpeg_ir::ContainerSummary {
    let mut entries = Vec::new();
    for table in &scan.tables {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            &mut attributes,
            format_args!("offset"),
            format_args!("{}", table.range().start),
        );
        insert_summary_attribute(
            &mut attributes,
            format_args!("size"),
            format_args!("{}", table.range().len()),
        );
        insert_summary_attribute(
            &mut attributes,
            format_args!("body_offset"),
            format_args!("{}", table.body().start),
        );
        insert_summary_attribute(
            &mut attributes,
            format_args!("record_count"),
            format_args!("{}", table.record_count),
        );
        for (typecode, count) in &table.object_typecodes {
            insert_summary_attribute(
                &mut attributes,
                format_args!("object_typecode_{typecode:#x}"),
                format_args!("{count}"),
            );
        }
        let storage = table
            .body_bytes(scan.data)
            .map_or(EntryStorage::unreported(VerbatimLabel::None), |body| {
                EntryStorage::framed_by(VerbatimLabel::None, body.into(), table.framing())
            });

        entries.push(ContainerEntry {
            name: format!("table-{:#x}", table.typecode),
            role: ContainerRole::Table,
            storage,
            attributes,
        });
    }
    let mut classes = BTreeMap::<Uuid, (usize, usize)>::new();
    for object in &scan.objects {
        // The container report groups degraded records under the nil class UUID.
        let class_uuid = object.class_uuid().unwrap_or_else(Uuid::nil);

        let entry = classes.entry(class_uuid).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += object.range().len();
    }
    for (class_uuid, (count, bytes)) in classes {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            &mut attributes,
            format_args!("class_uuid"),
            format_args!("{class_uuid}"),
        );
        insert_summary_attribute(
            &mut attributes,
            format_args!("nil_uuid"),
            format_args!("{}", class_uuid.is_nil()),
        );
        insert_summary_attribute(
            &mut attributes,
            format_args!("count"),
            format_args!("{count}"),
        );
        insert_summary_attribute(
            &mut attributes,
            format_args!("total_record_bytes"),
            format_args!("{bytes}"),
        );

        entries.push(ContainerEntry {
            name: format!("class-{class_uuid}"),
            role: ContainerRole::ObjectClass,
            storage: EntryStorage::verbatim(
                VerbatimLabel::None,
                cadmpeg_core::decode::u64_from_index(bytes),
            ),
            attributes,
        });
    }
    let mut notes = Vec::new();
    push_container_note(
        &mut notes,
        format_args!("archive version {}", scan.archive.value()),
    );
    for warning in scan.warnings.messages() {
        push_container_note(&mut notes, format_args!("{warning}"));
    }
    for diagnostic in scan.definitions.diagnostics() {
        push_container_note(
            &mut notes,
            format_args!("{}", diagnostic.diagnostic.message),
        );
    }
    let matched = dialect_match(scan);
    let mut losses = Vec::new();
    if let Some(loss) = crate::dialect::admission_loss(&matched) {
        losses.push(loss);
    }
    ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched),
        cadmpeg_ir::ContainerKind::ThreeDmChunks,
        entries,
        losses,
        notes,
    )
}

/// Classifies a scanned archive.
///
/// Every report this module builds from a [`Scan`] goes through here, so the
/// container summary, the container-only report, and the source metadata all
/// carry the same match.
pub(crate) fn dialect_match(scan: &Scan<'_>) -> DialectMatch {
    scan.archive
        .classify(scan.metadata.properties.writer_version)
}

/// Path-specific source attributes supplied to the single metadata builder.
pub(crate) enum SourceMetaDetail<'a> {
    /// Facts available from the flat V1 archive.
    FlatLegacyArchive,
    /// Facts reported only by container inspection.
    ContainerOnly(&'a Scan<'a>),
    /// Facts reported only after full decoding.
    Full {
        scan: &'a Scan<'a>,
        attributes: BTreeMap<NonBlankString, String>,
    },
}

fn insert_source_meta_attribute(
    attributes: &mut BTreeMap<NonBlankString, String>,
    key: &'static str,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    let key = key.to_owned();
    let key = NonBlankString::new(key)
        .ok_or_else(|| CodecError::malformed("generated Rhino source metadata key is blank"))?;
    let value = value.to_string();
    attributes.insert(key, value);
    Ok(())
}

/// Builds source metadata; `primary` is the one author of the document's identity.
pub(crate) fn source_meta(
    primary: DialectMatch,
    detail: SourceMetaDetail<'_>,
) -> Result<SourceMeta, CodecError> {
    let attributes = match detail {
        SourceMetaDetail::FlatLegacyArchive => {
            let mut attributes = BTreeMap::new();
            insert_source_meta_attribute(&mut attributes, "archive_version", format_args!("1"))?;
            attributes
        }
        SourceMetaDetail::ContainerOnly(scan) => {
            let mut attributes = BTreeMap::new();
            chunked_source_attributes(scan, &mut attributes)?;
            attributes
        }
        SourceMetaDetail::Full {
            scan,
            attributes: mut full,
        } => {
            chunked_source_attributes(scan, &mut full)?;
            full
        }
    };
    Ok(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(primary),
        attributes,
    ))
}

fn chunked_source_attributes(
    scan: &Scan<'_>,
    attributes: &mut BTreeMap<NonBlankString, String>,
) -> Result<(), CodecError> {
    insert_source_meta_attribute(
        attributes,
        "archive_version",
        format_args!("{}", scan.archive.value()),
    )?;
    insert_source_meta_attribute(attributes, "container_kind", format_args!("3dm-chunks"))?;
    Ok(())
}

/// Build an empty current-version IR and a container-only report.
pub(crate) fn container_only_result(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
) -> Result<Decoded, CodecError> {
    let mut notes = Vec::new();
    push_container_note(
        &mut notes,
        format_args!("archive version {}", scan.archive.value()),
    );
    for warning in scan.warnings.messages() {
        push_container_note(&mut notes, format_args!("{warning}"));
    }
    for diagnostic in scan.definitions.diagnostics() {
        push_container_note(
            &mut notes,
            format_args!("{}", diagnostic.diagnostic.message),
        );
    }
    let mut losses = Vec::new();
    for diagnostic in &scan.warnings {
        losses.push(
            diagnostic
                .code
                .unwrap_or(crate::loss::RhinoLossCode::ContainerScanDiagnostic)
                .note(&diagnostic.message),
        );
    }
    for diagnostic in scan.definitions.diagnostics() {
        losses.push(diagnostic.to_loss());
    }
    let primary = dialect_match(scan);
    if let Some(loss) = crate::dialect::admission_loss(&primary) {
        losses.push(loss);
    }
    let mut ir = CadIr::decoded(source_meta(primary, SourceMetaDetail::ContainerOnly(scan))?);
    let scope = cadmpeg_ir::ContentScope::container();
    ir.scope = scope;
    let mut source_fidelity =
        cadmpeg_ir::SourceFidelity::for_decode(ctx, cadmpeg_ir::Annotations::default());
    let mut unknowns = Vec::new();
    let marks = materialize_give_ups(ctx, scan.data, &scan.give_ups, &mut losses, &mut unknowns)?;
    source_fidelity.retain_unknown_records("rhino", unknowns)?;
    for mark in &marks {
        source_fidelity.mark_untyped(mark.id.as_str(), mark.classes.clone())?;
    }
    Ok(Decoded {
        ir,
        body: DecodeBody {
            transfer: cadmpeg_ir::report::decode::DecodeTransfer {
                scope,
                geometry_transferred: false,
            },
            coverage: cadmpeg_ir::report::decode::Coverage::default(),
            losses,
            notes,
        },
        source_fidelity,
    })
}

/// Inspect a Rhino stream, applying the version-specific scan depth.
pub(crate) fn inspect(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let data = acquire(root);
    let header = parse_header(data).map_err(framing_error)?;
    if !header.archive_version.is_chunked() {
        // The properties table is not read on this path, so no openNURBS
        // writer-version stamp is declared.
        let matched = header.archive_version.classify(None);
        let mut losses = Vec::new();
        if let Some(loss) = crate::dialect::admission_loss(&matched) {
            losses.push(loss);
        }
        let mut notes = Vec::new();
        push_container_note(
            &mut notes,
            format_args!("archive version {}", header.archive_version.value()),
        );
        return Ok(ContainerSummary::classified(
            cadmpeg_core::dialect::DialectLayers::of(matched),
            cadmpeg_ir::ContainerKind::ThreeDmChunks,
            Vec::new(),
            losses,
            notes,
        ));
    }
    Ok(summarize(&scan(ctx, data)?))
}

/// Decode a Rhino stream according to the supported container depth.
pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let data = acquire(root);
    let header = parse_header(data).map_err(framing_error)?;
    if header.archive_version == ArchiveVersion::V1 {
        return crate::legacy::decode_v1(ctx, data);
    }
    let scan = scan(ctx, data)?;
    if ctx.container_only() && scan.archive.is_chunked() {
        return container_only_result(ctx, &scan);
    }
    crate::decode::decode(&scan, crate::mesh::MeshExpand::new(ctx, root))
}

#[cfg(test)]
pub(crate) mod tests;
