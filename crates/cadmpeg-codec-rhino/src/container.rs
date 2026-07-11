// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino 3DM container scanning and summary construction.

use std::collections::BTreeMap;
use std::io::{Read, SeekFrom};

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;

use crate::chunks::{
    chunk_at, parse_eof, parse_header, verify_checksum, ArchiveVersion, ChecksumStatus,
    FramingError, TCODE_ENDOFFILE, TCODE_ENDOFTABLE,
};

/// Maximum input accepted by the Rhino container scanner.
///
/// The cap limits the one required in-memory input copy and is checked before
/// converting the stream length to allocation-sized offsets.
pub const INPUT_CAP: u64 = 256 * 1024 * 1024;

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
const TCODE_OBJECT_RECORD_TYPE: u32 = 0x82a0_0071;

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
}

/// A table descriptor with explicit source ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Table {
    /// Table typecode.
    pub(crate) typecode: u32,
    /// Complete table chunk range.
    pub(crate) range: std::ops::Range<usize>,
    /// Table body range, excluding the table checksum.
    pub(crate) body: std::ops::Range<usize>,
    /// Direct records in the table.
    pub(crate) records: Vec<Record>,
    /// Object record typecode counts discovered without class parsing.
    pub(crate) object_typecodes: BTreeMap<u32, usize>,
}

/// The result of scanning a complete supported container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Scan {
    /// Complete input bytes.
    pub(crate) data: Vec<u8>,
    /// Parsed archive version.
    pub(crate) archive: ArchiveVersion,
    /// Comment chunk descriptor.
    pub(crate) comment: Record,
    /// Tables in source order.
    pub(crate) tables: Vec<Table>,
    /// Validated EOF descriptor.
    pub(crate) eof_offset: usize,
    /// Recoverable checksum and unknown-record notes.
    pub(crate) warnings: Vec<String>,
}

impl Scan {
    fn version_note(&self) -> String {
        format!("archive version {}", self.archive.value())
    }
}

/// Read the complete input while enforcing [`INPUT_CAP`].
pub(crate) fn read_input(reader: &mut dyn ReadSeek) -> Result<Vec<u8>, CodecError> {
    reader.seek(SeekFrom::Start(0))?;
    let mut data = Vec::new();
    let mut limited = reader.take(INPUT_CAP.saturating_add(1));
    limited.read_to_end(&mut data)?;
    let size = u64::try_from(data.len())
        .map_err(|_| CodecError::Malformed("input size cannot fit in u64".to_string()))?;
    if size > INPUT_CAP {
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
    })
}

fn table_rank(typecode: u32) -> Option<u8> {
    Some(match typecode {
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

fn scan_object_types(
    data: &[u8],
    object: &Record,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<BTreeMap<u32, usize>, CodecError> {
    let mut counts = BTreeMap::new();
    let mut offset = object.body.start;
    while offset < object.body.end {
        let child = chunk_at(data, offset, object.body.end, archive, false)
            .map_err(|error| malformed(&error))?;
        if let Some(note) =
            checksum_warning(data, child.typecode, offset, object.body.end, archive)?
        {
            warnings.push(note);
        }
        if child.typecode == TCODE_OBJECT_RECORD_TYPE {
            if !child.short {
                return Err(CodecError::Malformed(
                    "object record type must be a short chunk".to_string(),
                ));
            }
            let value = u32::try_from(child.value)
                .map_err(|_| CodecError::Malformed("negative object record type".to_string()))?;
            *counts.entry(value).or_insert(0) += 1;
        }
        offset = child.next_offset;
    }
    Ok(counts)
}

/// Scan a V3/V4 or V5–V8 Rhino container.
pub(crate) fn scan(data: Vec<u8>) -> Result<Scan, CodecError> {
    let header = parse_header(&data).map_err(|error| malformed(&error))?;
    let archive = header.archive_version;
    let comment = parse_record(&data, 32, data.len(), archive)?;
    if comment.typecode != TCODE_COMMENT || comment.short {
        return Err(CodecError::Malformed(
            "first post-header chunk is not a long comment".to_string(),
        ));
    }
    let mut warnings = Vec::new();
    if let Some(note) = checksum_warning(&data, comment.typecode, 32, data.len(), archive)? {
        warnings.push(note);
    }
    let mut tables = Vec::new();
    let mut offset = comment.range.end;
    let mut last_rank = 0_u8;
    let mut saw_user = false;
    while offset < data.len() {
        let chunk = chunk_at(&data, offset, data.len(), archive, false)
            .map_err(|error| malformed(&error))?;
        if chunk.typecode == TCODE_ENDOFFILE {
            parse_eof(&data, offset, archive).map_err(|error| malformed(&error))?;
            return Ok(Scan {
                data,
                archive,
                comment,
                tables,
                eof_offset: offset,
                warnings,
            });
        }
        let rank = table_rank(chunk.typecode).ok_or_else(|| {
            CodecError::Malformed(format!("expected table or EOF at offset {offset}"))
        })?;
        if chunk.short {
            return Err(CodecError::Malformed(
                "table chunks must use long framing".to_string(),
            ));
        }
        if chunk.typecode == TCODE_USER {
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
        let mut records = Vec::new();
        let mut object_typecodes = BTreeMap::new();
        let mut child_offset = chunk.body.start;
        let mut terminated = false;
        while child_offset < chunk.body.end {
            let child = chunk_at(&data, child_offset, chunk.body.end, archive, false)
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
            let record = Record {
                typecode: child.typecode,
                range: child_offset..child.next_offset,
                body: child.body,
                short: child.short,
            };
            if !expected_record(chunk.typecode, record.typecode) {
                if known_record(record.typecode) {
                    return Err(CodecError::Malformed(format!(
                        "record typecode {:#x} is invalid in table {:#x}",
                        record.typecode, chunk.typecode
                    )));
                }
                warnings.push(format!(
                    "unknown bounded record {:#x} skipped in table {:#x} at offset {child_offset}",
                    record.typecode, chunk.typecode
                ));
            }
            if let Some(note) = checksum_warning(
                &data,
                record.typecode,
                child_offset,
                chunk.body.end,
                archive,
            )? {
                warnings.push(note);
            }
            if chunk.typecode == TCODE_OBJECTS && record.typecode == TCODE_OBJECT_RECORD {
                for (typecode, count) in scan_object_types(&data, &record, archive, &mut warnings)?
                {
                    *object_typecodes.entry(typecode).or_insert(0) += count;
                }
            }
            records.push(record);
            child_offset = child.next_offset;
        }
        if !terminated {
            return Err(CodecError::Malformed(format!(
                "table {:#x} is missing end-of-table marker",
                chunk.typecode
            )));
        }
        if let Some(note) = checksum_warning(&data, chunk.typecode, offset, data.len(), archive)? {
            warnings.push(note);
        }
        tables.push(Table {
            typecode: chunk.typecode,
            range: offset..chunk.next_offset,
            body: chunk.body,
            records,
            object_typecodes,
        });
        offset = chunk.next_offset;
    }
    Err(CodecError::Malformed(
        "missing end-of-file chunk".to_string(),
    ))
}

/// Build the format-neutral container summary.
pub(crate) fn summarize(scan: &Scan) -> ContainerSummary {
    let mut entries = Vec::with_capacity(scan.tables.len());
    for table in &scan.tables {
        let mut attributes = BTreeMap::new();
        attributes.insert("offset".to_string(), table.range.start.to_string());
        attributes.insert("size".to_string(), table.range.len().to_string());
        attributes.insert("body_offset".to_string(), table.body.start.to_string());
        attributes.insert("record_count".to_string(), table.records.len().to_string());
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
    let mut notes = vec![scan.version_note()];
    notes.extend(scan.warnings.iter().cloned());
    ContainerSummary {
        format: "rhino".to_string(),
        container_kind: "3dm-chunks".to_string(),
        entries,
        notes,
    }
}

fn source_meta(scan: &Scan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "archive_version".to_string(),
        scan.archive.value().to_string(),
    );
    attributes.insert("container_kind".to_string(), "3dm-chunks".to_string());
    SourceMeta {
        format: "rhino".to_string(),
        attributes,
    }
}

/// Build an empty current-version IR and a container-only report.
pub(crate) fn container_only_result(scan: &Scan) -> cadmpeg_ir::codec::DecodeResult {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(scan));
    let mut notes = vec![scan.version_note()];
    notes.extend(scan.warnings.iter().cloned());
    let losses = scan
        .warnings
        .iter()
        .map(|message| LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: message.clone(),
            provenance: None,
        })
        .collect();
    cadmpeg_ir::codec::DecodeResult::new(
        ir,
        DecodeReport {
            format: "rhino".to_string(),
            container_only: true,
            geometry_transferred: false,
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
pub(crate) fn inspect(reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
    let data = read_input(reader)?;
    let header = parse_header(&data).map_err(|error| malformed(&error))?;
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
    Ok(summarize(&scan(data)?))
}

/// Decode a Rhino stream according to the currently supported container depth.
pub(crate) fn decode(
    reader: &mut dyn ReadSeek,
    container_only: bool,
) -> Result<cadmpeg_ir::codec::DecodeResult, CodecError> {
    let data = read_input(reader)?;
    let header = parse_header(&data).map_err(|error| malformed(&error))?;
    if header_only(header.archive_version) {
        return Err(CodecError::NotImplemented(format!(
            "Rhino archive version {} decode is not implemented",
            header.archive_version.value()
        )));
    }
    let scan = scan(data)?;
    if container_only && scan.archive.supports_geometry() {
        return Ok(container_only_result(&scan));
    }
    Err(CodecError::NotImplemented(
        "Rhino entity decode is not implemented".to_string(),
    ))
}
