// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino 3DM container scanning and summary construction.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::units::Units;

use crate::chunks::{
    chunk_at, parse_eof, parse_header, verify_checksum, ArchiveVersion, ChecksumStatus,
    FramingError, TCODE_CRC, TCODE_ENDOFFILE, TCODE_ENDOFTABLE,
};
use crate::instances::{parse_definitions, DefinitionScan};
use crate::loss::RhinoLossCode;
use crate::objects::{
    degraded_object_record, parse_object_record, resolve_identities, ObjectDescriptor,
};
use crate::wire::Uuid;

/// Maximum input accepted by the Rhino container scanner.
///
/// This codec-local cap bounds the addressable offset space indexed by the
/// chunk walker independently of the platform input limit.
pub(crate) const INPUT_CAP: u64 = 256 * 1024 * 1024;
/// Maximum direct table records retained or described in one document.
///
/// Bounds record-descriptor amplification from an attacker-controlled table body independently of the
/// codec-local input limit; kept as defense in depth.
pub(crate) const TABLE_RECORD_CAP: usize = 1 << 20;

const TCODE_COMMENT: u32 = 0x0000_0001;
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

/// A bounded record descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    /// Record typecode.
    pub(crate) typecode: u32,
    /// Complete chunk range, including header and checksum.
    pub(crate) range: std::ops::Range<usize>,
    /// Payload/body range, excluding chunk header and checksum.
    pub(crate) body: std::ops::Range<usize>,
    /// Whether the record is a short chunk.
    pub(crate) short: bool,
    /// Inline value for a short chunk, or zero for a long chunk.
    pub(crate) value: i64,
}

/// A table descriptor with explicit source ranges.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Table {
    /// Table typecode.
    pub(crate) typecode: u32,
    /// Complete table chunk range.
    pub(crate) range: std::ops::Range<usize>,
    /// Table body range, excluding the table checksum.
    pub(crate) body: std::ops::Range<usize>,
    /// Direct records in the table.
    pub(crate) records: Vec<Record>,
    /// Number of direct records, including compactly summarized records.
    pub(crate) record_count: usize,
    /// Object record typecode counts discovered without class parsing.
    pub(crate) object_typecodes: BTreeMap<u32, usize>,
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
    /// Comment chunk descriptor.
    pub(crate) comment: Record,
    /// Tables in source order.
    pub(crate) tables: Vec<Table>,
    /// All object records in source order.
    pub(crate) objects: Vec<ObjectDescriptor>,
    /// Parsed instance definitions and recoverable definition diagnostics.
    pub(crate) definitions: DefinitionScan,
    /// Decoded built-in history records in source order.
    pub(crate) history: Vec<crate::history::HistoryRecord>,
    /// Validated EOF descriptor.
    pub(crate) eof_offset: usize,
    /// Recoverable checksum and unknown-record notes.
    pub(crate) warnings: Vec<String>,
    /// Typed metadata decoded from property, setting, and layer records.
    pub(crate) metadata: crate::settings::DocumentMetadata,
}

impl Scan<'_> {
    fn version_note(&self) -> String {
        format!("archive version {}", self.archive.value())
    }
}

/// Borrow the session root bytes, enforcing the codec-local input ceiling.
fn acquire(root: View<'_>) -> Result<&[u8], CodecError> {
    let data = root.window();
    if data.len() as u64 > INPUT_CAP {
        return Err(CodecError::Malformed(format!(
            "input exceeds Rhino size cap of {INPUT_CAP} bytes"
        )));
    }
    Ok(data)
}

fn malformed(error: &FramingError) -> CodecError {
    CodecError::Malformed(error.to_string())
}

fn checksum_warning(
    data: &[u8],
    typecode: u32,
    offset: usize,
    parent_end: usize,
    archive: ArchiveVersion,
) -> Result<Option<String>, CodecError> {
    let chunk =
        chunk_at(data, offset, parent_end, archive, false).map_err(|error| malformed(&error))?;
    match verify_checksum(data, &chunk).map_err(|error| malformed(&error))? {
        ChecksumStatus::Mismatch { expected, actual } => Ok(Some(format!(
            "CRC mismatch at offset {offset} for typecode {typecode:#x}: expected {expected:#x}, got {actual:#x}"
        ))),
        _ => Ok(None),
    }
}

fn parse_record(
    data: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<Record, CodecError> {
    let chunk = chunk_at(data, offset, end, archive, false).map_err(|error| malformed(&error))?;
    Ok(Record {
        typecode: chunk.typecode,
        range: offset..chunk.next_offset,
        body: chunk.body,
        short: chunk.short,
        value: chunk.value,
    })
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

/// Scan a Rhino decode root into its V3/V4 or V5–V8 container structure.
///
/// Folds the [`acquire`] byte extraction and codec-local input ceiling into the
/// decode-root entry that `decode` and `inspect` share. `_ctx` is taken for
/// parity with the other container codecs' `scan(ctx, root)` signature; the
/// chunk walk is a pure function of the source bytes gated by per-chunk framing
/// checks, so it charges no decode budget.
pub(crate) fn scan<'a>(_ctx: &DecodeContext<'_>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    scan_with_record_limit(acquire(root)?, TABLE_RECORD_CAP)
}

fn scan_with_record_limit(data: &[u8], record_limit: usize) -> Result<Scan<'_>, CodecError> {
    let header = parse_header(data).map_err(|error| malformed(&error))?;
    let archive = header.archive_version;
    let comment = parse_record(data, 32, data.len(), archive)?;
    if comment.typecode != TCODE_COMMENT || comment.short {
        return Err(CodecError::Malformed(
            "first post-header chunk is not a long comment".to_string(),
        ));
    }
    let mut warnings = Vec::new();
    if let Some(note) = checksum_warning(data, comment.typecode, 32, data.len(), archive)? {
        warnings.push(note);
    }
    let mut tables = Vec::new();
    let mut offset = comment.range.end;
    let mut last_rank = 0_u8;
    let mut saw_user = false;
    let mut saw_properties = false;
    let mut saw_settings = false;
    let mut saw_objects = false;
    let mut all_objects = Vec::new();
    let mut definitions = DefinitionScan::default();
    let mut history = Vec::new();
    let mut record_count = 0_usize;
    while offset < data.len() {
        let chunk = chunk_at(data, offset, data.len(), archive, false)
            .map_err(|error| malformed(&error))?;
        if chunk.typecode == TCODE_ENDOFFILE {
            if !saw_properties || !saw_settings || !saw_objects {
                return Err(CodecError::Malformed(
                    "properties, settings, and object tables are required".to_string(),
                ));
            }
            parse_eof(data, offset, archive).map_err(|error| malformed(&error))?;
            let metadata = crate::settings::parse_metadata(data, archive, &tables, &mut warnings);
            resolve_identities(&mut all_objects, &metadata, &mut warnings);
            return Ok(Scan {
                data,
                archive,
                comment,
                tables,
                objects: all_objects,
                definitions,
                history,
                eof_offset: offset,
                warnings,
                metadata,
            });
        }
        let rank = table_rank(chunk.typecode).ok_or_else(|| {
            CodecError::Malformed(format!("expected table or EOF at offset {offset}"))
        })?;
        match table_base(chunk.typecode) {
            TCODE_PROPERTIES => saw_properties = true,
            TCODE_SETTINGS => saw_settings = true,
            TCODE_OBJECTS => saw_objects = true,
            _ => {}
        }
        if chunk.short {
            return Err(CodecError::Malformed(
                "table chunks must use long framing".to_string(),
            ));
        }
        if table_base(chunk.typecode) == TCODE_USER {
            if !saw_user && rank < last_rank {
                return Err(CodecError::Malformed(
                    "user table is out of order".to_string(),
                ));
            }
            saw_user = true;
        } else {
            if saw_user || rank <= last_rank {
                return Err(CodecError::Malformed(format!(
                    "table typecode {:#x} is out of order or duplicated",
                    chunk.typecode
                )));
            }
            last_rank = rank;
        }
        let retain_records = retain_record_descriptors(chunk.typecode);
        let mut records = Vec::new();
        let mut table_record_count = 0_usize;
        let mut object_typecodes = BTreeMap::new();
        let mut child_offset = chunk.body.start;
        let mut terminated = false;
        while child_offset < chunk.body.end {
            let child = chunk_at(data, child_offset, chunk.body.end, archive, false)
                .map_err(|error| malformed(&error))?;
            if child.typecode == TCODE_ENDOFTABLE {
                if !child.short || child.value != 0 {
                    return Err(CodecError::Malformed(
                        "end-of-table marker must be short with value zero".to_string(),
                    ));
                }
                if child.next_offset != chunk.body.end {
                    return Err(CodecError::Malformed(
                        "end-of-table marker is not the final table child".to_string(),
                    ));
                }
                terminated = true;
                break;
            }
            record_count = record_count
                .checked_add(1)
                .filter(|count| *count <= record_limit)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "document table record budget of {record_limit} exceeded"
                    ))
                })?;
            table_record_count = table_record_count
                .checked_add(1)
                .expect("document record budget bounds table count");
            let record = Record {
                typecode: child.typecode,
                range: child_offset..child.next_offset,
                body: child.body,
                short: child.short,
                value: child.value,
            };
            if !record_is_allowed(chunk.typecode, record.typecode, record.short) {
                if known_record(record.typecode) {
                    return Err(CodecError::Malformed(format!(
                        "record typecode {:#x} is invalid or short-framed in table {:#x}",
                        record.typecode, chunk.typecode
                    )));
                }
                if record.short {
                    return Err(CodecError::Malformed(format!(
                        "unknown table record {:#x} is short-framed",
                        record.typecode
                    )));
                }
                warnings.push(format!(
                    "unknown bounded record {:#x} skipped in table {:#x} at offset {child_offset}",
                    record.typecode, chunk.typecode
                ));
            }
            // A layer wrapper has no direct bytes, and object records use a
            // zero outer CRC. Checksummed leaf children remain independently
            // validated.
            let zero_nested_container_crc =
                matches!(record.typecode, TCODE_OBJECT_RECORD | TCODE_LAYER_RECORD)
                    && child
                        .checksum
                        .as_ref()
                        .is_some_and(|range| data[range.clone()].iter().all(|byte| *byte == 0));
            if !zero_nested_container_crc {
                if let Some(note) =
                    checksum_warning(data, record.typecode, child_offset, chunk.body.end, archive)?
                {
                    warnings.push(note);
                }
            }
            if table_base(chunk.typecode) == TCODE_OBJECTS && record.typecode == TCODE_OBJECT_RECORD
            {
                let descriptor = match parse_object_record(data, &record, archive, &mut warnings) {
                    Ok(descriptor) => descriptor,
                    Err(error) => {
                        warnings.push(format!(
                            "bounded object record at {child_offset} is malformed: {error}"
                        ));
                        degraded_object_record(&record, &error)
                    }
                };
                *object_typecodes.entry(descriptor.object_type).or_insert(0) += 1;
                all_objects.push(descriptor);
            }
            if retain_records {
                records.push(record);
            }
            child_offset = child.next_offset;
        }
        if !terminated {
            return Err(CodecError::Malformed(format!(
                "table {:#x} is missing end-of-table marker",
                chunk.typecode
            )));
        }
        if let Some(note) =
            checksum_warning(data, chunk.typecode, offset, chunk.next_offset, archive)?
        {
            warnings.push(note);
        }
        if table_base(chunk.typecode) == TCODE_INSTANCE_DEFINITION {
            definitions = parse_definitions(data, &records, archive);
        }
        if table_base(chunk.typecode) == TCODE_HISTORY {
            history = crate::history::parse_records(data, &records, archive, &mut warnings);
        }
        tables.push(Table {
            typecode: chunk.typecode,
            range: offset..chunk.next_offset,
            body: chunk.body,
            records,
            record_count: table_record_count,
            object_typecodes,
        });
        offset = chunk.next_offset;
    }
    Err(CodecError::Malformed(
        "missing end-of-file chunk".to_string(),
    ))
}

/// Scans an owned buffer, leaking it so the borrowed [`Scan`] is `'static`.
///
/// Test-only. Production decode borrows the arena-backed root view; unit tests
/// construct throwaway buffers, and leaking each keeps their `Scan` free of a
/// borrow the test must thread. The leak is bounded by the fixture count in a
/// short-lived test process.
#[cfg(test)]
pub(crate) fn scan_owned(data: Vec<u8>) -> Result<Scan<'static>, CodecError> {
    scan_with_record_limit(Box::leak(data.into_boxed_slice()), TABLE_RECORD_CAP)
}

#[cfg(test)]
pub(crate) fn scan_with_test_record_limit(
    data: Vec<u8>,
    record_limit: usize,
) -> Result<Scan<'static>, CodecError> {
    scan_with_record_limit(Box::leak(data.into_boxed_slice()), record_limit)
}

/// Build the format-neutral container summary.
pub(crate) fn summarize(scan: &Scan<'_>) -> ContainerSummary {
    let mut entries = Vec::with_capacity(scan.tables.len());
    for table in &scan.tables {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_string(), table.range.start.to_string());
        attributes.insert("size".to_string(), table.range.len().to_string());
        attributes.insert("body_offset".to_string(), table.body.start.to_string());
        attributes.insert("record_count".to_string(), table.record_count.to_string());
        for (typecode, count) in &table.object_typecodes {
            attributes.insert(format!("object_typecode_{typecode:#x}"), count.to_string());
        }
        entries.push(ContainerEntry {
            name: format!("table-{:#x}", table.typecode),
            role: "table".to_string(),
            compression: "none".to_string(),
            compressed_size: table.range.len() as u64,
            uncompressed_size: table.body.len() as u64,
            attributes,
        });
    }
    let mut classes = BTreeMap::<Uuid, (usize, usize)>::new();
    for object in &scan.objects {
        let entry = classes.entry(object.class_uuid).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += object.range.len();
    }
    for (class_uuid, (count, bytes)) in classes {
        let mut attributes = BTreeMap::new();
        attributes.insert("class_uuid".to_string(), class_uuid.to_string());
        attributes.insert("nil_uuid".to_string(), class_uuid.is_nil().to_string());
        attributes.insert("count".to_string(), count.to_string());
        attributes.insert("total_record_bytes".to_string(), bytes.to_string());
        entries.push(ContainerEntry {
            name: format!("class-{class_uuid}"),
            role: "object-class".to_string(),
            compression: "none".to_string(),
            compressed_size: bytes as u64,
            uncompressed_size: bytes as u64,
            attributes,
        });
    }
    let mut notes = vec![scan.version_note()];
    notes.extend(scan.warnings.iter().cloned());
    notes.extend(
        scan.definitions
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone()),
    );
    ContainerSummary {
        format: "rhino".to_string(),
        container_kind: "3dm-chunks".to_string(),
        entries,
        notes,
    }
}

fn source_meta(scan: &Scan<'_>) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "archive_version".to_string(),
        scan.archive.value().to_string(),
    );
    attributes.insert("container_kind".to_string(), "3dm-chunks".to_string());
    attributes.insert(
        "comment_offset".to_string(),
        scan.comment.range.start.to_string(),
    );
    attributes.insert("eof_offset".to_string(), scan.eof_offset.to_string());
    attributes.insert("table_count".to_string(), scan.tables.len().to_string());
    attributes.insert(
        "instance_definition_count".to_string(),
        scan.definitions.definitions.len().to_string(),
    );
    SourceMeta {
        format: "rhino".to_string(),
        attributes,
    }
}

/// Build an empty current-version IR and a container-only report.
pub(crate) fn container_only_result(scan: &Scan<'_>) -> cadmpeg_ir::codec::DecodeResult {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(scan));
    let mut notes = vec![scan.version_note()];
    notes.extend(scan.warnings.iter().cloned());
    notes.extend(
        scan.definitions
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone()),
    );
    let mut losses: Vec<_> = scan
        .warnings
        .iter()
        .map(|message| RhinoLossCode::ScanWarning.note(message.clone()))
        .collect();
    losses.extend(scan.definitions.diagnostics.iter().map(|diagnostic| {
        RhinoLossCode::InstanceDefinitionsRetained.note_with_provenance(
            diagnostic.message.clone(),
            cadmpeg_ir::LossProvenance {
                format: "rhino".to_string(),
                stream: String::new(),
                offset: diagnostic.source_range.start as u64,
                tag: Some("INSTANCE_DEFINITION_TABLE".to_string()),
            },
        )
    }));
    cadmpeg_ir::codec::DecodeResult::new(
        ir,
        DecodeReport {
            format: "rhino".to_string(),
            container_only: true,
            geometry_transferred: false,
            coverage: std::collections::BTreeMap::new(),
            losses,
            notes,
        },
    )
}

/// Return whether a version is inspectable only from its header.
pub(crate) fn header_only(archive: ArchiveVersion) -> bool {
    matches!(
        archive,
        ArchiveVersion::V1
            | ArchiveVersion::V2
            | ArchiveVersion::LegacyV5
            | ArchiveVersion::Other(_)
    )
}

/// Inspect a Rhino stream, applying the version-specific scan depth.
pub(crate) fn inspect(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let header = parse_header(acquire(root)?).map_err(|error| malformed(&error))?;
    if header_only(header.archive_version) {
        return Ok(ContainerSummary {
            format: "rhino".to_string(),
            container_kind: "3dm-chunks".to_string(),
            entries: Vec::new(),
            notes: vec![format!(
                "archive version {}",
                header.archive_version.value()
            )],
        });
    }
    Ok(summarize(&scan(ctx, root)?))
}

/// Decode a Rhino stream according to the supported container depth.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    container_only: bool,
) -> Result<cadmpeg_ir::codec::DecodeResult, CodecError> {
    let header = parse_header(acquire(root)?).map_err(|error| malformed(&error))?;
    if header_only(header.archive_version) {
        return Err(CodecError::NotImplemented(format!(
            "Rhino archive version {} decode is not implemented",
            header.archive_version.value()
        )));
    }
    let scan = scan(ctx, root)?;
    if container_only
        && matches!(
            scan.archive,
            ArchiveVersion::V3
                | ArchiveVersion::V4
                | ArchiveVersion::V5
                | ArchiveVersion::V6
                | ArchiveVersion::V7
                | ArchiveVersion::V8
        )
    {
        return Ok(container_only_result(&scan));
    }
    Ok(crate::decode::decode(
        &scan,
        crate::mesh::MeshExpand::new(ctx, root),
    ))
}
