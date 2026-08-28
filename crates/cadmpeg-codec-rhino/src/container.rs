// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino 3DM container scanning and summary construction.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::units::Units;

use crate::chunks::{
    checked_count_bytes, checksum_children_through_class_end, chunk_at, direct_checksum_ranges,
    parse_eof, parse_header, verify_checksum, verify_checksum_ranges, ArchiveVersion,
    BoundedReader, ChecksumStatus, FramingError, TCODE_CRC, TCODE_ENDOFFILE, TCODE_ENDOFTABLE,
};
use crate::instances::{parse_definitions, DefinitionScan};
use crate::layout::file_header;
use crate::objects::{
    degraded_object_record, parse_object_record, resolve_identities, ObjectDescriptor,
};
use crate::wire::Uuid;
/// Maximum direct table records retained or described in one document.
pub(crate) const TABLE_RECORD_CAP: usize = 1 << 20;

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
    /// Payload/body range, excluding chunk header and checksum.
    pub(crate) body: std::ops::Range<usize>,
    /// Whether the record is a short chunk.
    pub(crate) short: bool,
    /// Inline value for a short chunk, or zero for a long chunk.
    pub(crate) value: i64,
}

/// A complete direct table record whose payload has no typed owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpaqueRecord {
    /// Containing table typecode.
    pub(crate) table_typecode: u32,
    /// Complete record descriptor.
    pub(crate) record: Record,
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
    /// Direct table records retained as opaque source data.
    pub(crate) opaque_records: Vec<OpaqueRecord>,
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

/// Borrows the session root bytes after the shared input budget admitted them.
fn acquire(root: View<'_>) -> &[u8] {
    root.window()
}

fn framing_error(error: FramingError) -> CodecError {
    match error {
        FramingError::Truncated { offset, .. } => CodecError::truncated(
            cadmpeg_core::decode::SourceLocation {
                space: cadmpeg_core::decode::SpaceId::ROOT,
                offset: offset as u64,
            },
            "rhino chunk framing",
        ),
        other => CodecError::Malformed(other.to_string()),
    }
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
                | TCODE_HATCH_PATTERN_RECORD
                | TCODE_LINETYPE_RECORD
                | TCODE_TEXTURE_MAPPING_RECORD
                | TCODE_HISTORY_RECORD
        ) {
        crate::chunks::verify_checksum_ranges(data, &chunk, &[])
    } else if matches!(
        typecode,
        TCODE_NAMED_PLANES | TCODE_NAMED_VIEWS | TCODE_VIEWS
    ) {
        let Ok(children) = list_checksum_children(data, &chunk, archive) else {
            return Ok(None);
        };
        let direct = direct_checksum_ranges(&chunk.body, &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if matches!(
        typecode,
        TCODE_RENDER_MESH_SETTINGS | TCODE_ANALYSIS_MESH_SETTINGS
    ) {
        let Ok(children) = mesh_checksum_children(data, &chunk, archive) else {
            return Ok(None);
        };
        let direct = direct_checksum_ranges(&chunk.body, &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_RENDER_SETTINGS {
        let Ok(children) = render_settings_checksum_children(data, &chunk, archive) else {
            return Ok(None);
        };
        let direct = direct_checksum_ranges(&chunk.body, &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_SETTINGS_ATTRIBUTES {
        let Ok(children) = settings_attributes_checksum_children(data, &chunk, archive) else {
            return Ok(None);
        };
        let direct = direct_checksum_ranges(&chunk.body, &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_PLUGIN_LIST {
        let Ok(children) = plugin_list_checksum_children(data, &chunk, archive) else {
            return Ok(None);
        };
        let direct = direct_checksum_ranges(&chunk.body, &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_RENDER_USERDATA {
        let Ok(children) = checksum_children_through_class_end(
            data,
            chunk.body.clone(),
            archive,
            "render-settings userdata",
        ) else {
            return Ok(None);
        };
        let direct = direct_checksum_ranges(&chunk.body, &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_COMPRESSED_PREVIEW {
        let Ok(children) = compressed_preview_checksum_children(data, &chunk, archive) else {
            return Ok(None);
        };
        let direct = direct_checksum_ranges(&chunk.body, &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_USER_TABLE_UUID {
        let Ok(children) = user_table_uuid_checksum_children(data, &chunk, archive) else {
            return Ok(None);
        };
        let direct = direct_checksum_ranges(&chunk.body, &children).map_err(framing_error)?;
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
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    Ok(mesh_subd_checksum_child(data, &mut reader, archive)?
        .into_iter()
        .collect())
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
    if View::u32_le_at(data, chunk.body.start) != Some(TCODE_ANONYMOUS) {
        return Ok(Vec::new());
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    Ok(vec![take_anonymous_checksum_child(
        data,
        &mut reader,
        archive,
        "modern render settings",
    )?])
}

/// Returns the complete nested chunks in a settings-attributes body.
fn settings_attributes_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
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
        children.push(take_anonymous_checksum_child(
            data,
            &mut reader,
            archive,
            "settings-attributes page units",
        )?);
    }
    if minor >= 2 {
        reader.skip(16)?;
    }
    if minor >= 3 {
        reader.skip(24)?;
        children.push(take_anonymous_checksum_child(
            data,
            &mut reader,
            archive,
            "settings-attributes earth anchor",
        )?);
    }
    if minor >= 4 {
        reader.bool()?;
    }
    if minor >= 5 {
        children.push(take_anonymous_checksum_child(
            data,
            &mut reader,
            archive,
            "settings-attributes IO settings",
        )?);
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
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
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
    if child.typecode != TCODE_ANONYMOUS || child.short {
        return Err(FramingError::structural(
            start,
            format!("{label} must be an anonymous long chunk"),
        ));
    }
    reader.skip(child.next_offset - start)?;
    Ok(child.range())
}

/// Returns the optional record-header child inside a user-table UUID record.
fn user_table_uuid_checksum_children(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    reader.skip(16)?;
    if reader.position() == reader.end() {
        return Ok(Vec::new());
    }

    let start = reader.position();
    let child = chunk_at(data, start, reader.end(), archive, false)?;
    if child.typecode != TCODE_USER_TABLE_RECORD_HEADER || child.short {
        return Ok(Vec::new());
    }
    reader.skip(child.next_offset - start)?;
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
    let count = View::i32_le_at(data, chunk.body.start).ok_or(FramingError::Truncated {
        offset: chunk.body.start,
        needed: 4,
    })?;
    let child_count = usize::try_from(count).unwrap_or(0);
    let mut offset = chunk
        .body
        .start
        .checked_add(4)
        .ok_or(FramingError::Overflow {
            offset: chunk.body.start,
        })?;
    if offset > chunk.body.end {
        return Err(FramingError::Truncated {
            offset: chunk.body.end,
            needed: offset - chunk.body.end,
        });
    }
    let mut children = Vec::new();
    for _ in 0..child_count {
        let child = chunk_at(data, offset, chunk.body.end, archive, false)?;
        if child.next_offset <= offset {
            return Err(FramingError::structural(
                offset,
                "view-list child did not advance",
            ));
        }
        children.push(child.range());
        offset = child.next_offset;
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
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
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
    let mut children = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        let start = reader.position();
        let child = chunk_at(data, start, reader.end(), archive, false)?;
        if child.typecode != TCODE_ANONYMOUS || child.short {
            return Err(FramingError::structural(
                start,
                "plugin-list child must be an anonymous long chunk",
            ));
        }
        if child.next_offset <= start {
            return Err(FramingError::structural(
                start,
                "plugin-list child did not advance",
            ));
        }
        children.push(child.range());
        reader.skip(child.next_offset - start)?;
    }
    Ok(children)
}

fn parse_record(
    data: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<Record, CodecError> {
    let chunk = chunk_at(data, offset, end, archive, false).map_err(framing_error)?;
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
pub(crate) fn scan(data: &[u8]) -> Result<Scan<'_>, CodecError> {
    scan_with_record_limit(data, TABLE_RECORD_CAP)
}

fn scan_with_record_limit(data: &[u8], record_limit: usize) -> Result<Scan<'_>, CodecError> {
    let header = parse_header(data).map_err(framing_error)?;
    let archive = header.archive_version;
    let archive_start = header.start_offset;
    let comment_offset = archive_start + file_header::LEN;
    let comment = parse_record(data, comment_offset, data.len(), archive)?;
    if comment.typecode != TCODE_COMMENT || comment.short {
        return Err(CodecError::Malformed(
            "first post-header chunk is not a long comment".to_string(),
        ));
    }
    let mut warnings = Vec::new();
    if let Some(note) =
        checksum_warning(data, comment.typecode, comment_offset, data.len(), archive)?
    {
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
    let mut opaque_records = Vec::new();
    let mut definitions = DefinitionScan::default();
    let mut history = Vec::new();
    let mut record_count = 0_usize;
    while offset < data.len() {
        let chunk = chunk_at(data, offset, data.len(), archive, false).map_err(framing_error)?;
        if chunk.typecode == TCODE_ENDOFFILE {
            if !saw_properties || !saw_settings || !saw_objects {
                return Err(CodecError::Malformed(
                    "properties, settings, and object tables are required".to_string(),
                ));
            }
            parse_eof(data, offset, archive).map_err(framing_error)?;
            let mut metadata =
                crate::settings::parse_metadata(data, archive, &tables, &mut warnings);
            resolve_identities(&mut all_objects, &metadata, &mut warnings);
            opaque_records.extend(std::mem::take(&mut metadata.opaque_records));
            return Ok(Scan {
                data,
                archive,
                comment,
                tables,
                objects: all_objects,
                opaque_records,
                definitions,
                history,
                eof_offset: offset,
                warnings,
                metadata,
            });
        }
        let rank = table_rank(chunk.typecode).ok_or_else(|| {
            CodecError::malformed(format_args!("expected table or EOF at offset {offset}"))
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
                return Err(CodecError::malformed(format_args!(
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
        let writer_version = if table_base(chunk.typecode) == TCODE_OBJECTS {
            tables
                .iter()
                .rev()
                .filter(|table| table_base(table.typecode) == TCODE_PROPERTIES)
                .flat_map(|table| table.records.iter().rev())
                .find_map(|record| {
                    (record.typecode == TCODE_WRITER_VERSION && record.short)
                        .then_some(record.value)
                })
        } else {
            None
        };
        let mut child_offset = chunk.body.start;
        let mut terminated = false;
        while child_offset < chunk.body.end {
            let child = chunk_at(data, child_offset, chunk.body.end, archive, false)
                .map_err(framing_error)?;
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
                    CodecError::malformed(format_args!(
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
            let opaque = table_base(chunk.typecode) == TCODE_USER
                || !record_is_allowed(chunk.typecode, record.typecode, record.short);
            if !record_is_allowed(chunk.typecode, record.typecode, record.short) {
                if known_record(record.typecode) {
                    return Err(CodecError::malformed(format_args!(
                        "record typecode {:#x} is invalid or short-framed in table {:#x}",
                        record.typecode, chunk.typecode
                    )));
                }
                warnings.push(format!(
                    "unknown bounded record {:#x} skipped in table {:#x} at offset {child_offset}",
                    record.typecode, chunk.typecode
                ));
            }
            if let Some(note) =
                checksum_warning(data, record.typecode, child_offset, chunk.body.end, archive)?
            {
                warnings.push(note);
            }
            if table_base(chunk.typecode) == TCODE_OBJECTS && record.typecode == TCODE_OBJECT_RECORD
            {
                let descriptor = match parse_object_record(
                    data,
                    &record,
                    archive,
                    writer_version,
                    &mut warnings,
                ) {
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
            if opaque {
                opaque_records.push(OpaqueRecord {
                    table_typecode: chunk.typecode,
                    record: record.clone(),
                });
            }
            if retain_records {
                records.push(record);
            }
            child_offset = child.next_offset;
        }
        if !terminated {
            warnings.push(format!(
                "table {:#x} has no end-of-table marker",
                chunk.typecode
            ));
        }
        if let Some(note) =
            checksum_warning(data, chunk.typecode, offset, chunk.next_offset, archive)?
        {
            warnings.push(note);
        }
        if table_base(chunk.typecode) == TCODE_INSTANCE_DEFINITION {
            let parsed = parse_definitions(data, &records, archive, chunk.typecode);
            definitions = parsed.scan;
            opaque_records.extend(parsed.opaque_records);
        }
        if table_base(chunk.typecode) == TCODE_HISTORY {
            let parsed = crate::history::parse_records(
                data,
                &records,
                archive,
                &mut warnings,
                chunk.typecode,
            );
            history = parsed.records;
            opaque_records.extend(parsed.opaque_records);
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

/// Test-only: leak `data` so the borrowed [`Scan`] is `'static`.
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
    let dialects = vec![dialect_match(scan)];
    ContainerSummary {
        dialects,
        format: crate::dialect::FORMAT.to_string(),
        container_kind: "3dm-chunks".to_string(),
        entries,
        notes,
    }
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
        format: crate::dialect::FORMAT.to_string(),
        attributes,
        ..Default::default()
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
        .map(|message| crate::loss::RhinoLossCode::ContainerScanDiagnostic.note(message.clone()))
        .collect();
    losses.extend(scan.definitions.diagnostics.iter().map(|diagnostic| {
        crate::loss::RhinoLossCode::ContainerInstanceDefinitionDegraded
            .note(diagnostic.message.clone())
            .with_provenance(cadmpeg_ir::SourceProvenance {
                format: "rhino".to_string(),
                stream: String::new(),
                offset: diagnostic.source_range.start as u64,
                tag: Some("INSTANCE_DEFINITION_TABLE".to_string()),
            })
    }));
    let dialects = vec![dialect_match(scan)];
    losses.extend(
        cadmpeg_core::dialect::primary_layer(&dialects, crate::dialect::FORMAT)
            .and_then(crate::dialect::admission_loss),
    );
    cadmpeg_ir::codec::DecodeResult::new(
        ir,
        DecodeReport {
            dialects,
            format: crate::dialect::FORMAT.to_string(),
            container_only: true,
            geometry_transferred: false,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses,
            notes,
        },
        cadmpeg_ir::SourceFidelity::default(),
    )
}

/// Return whether a version is inspectable only from its header, the complement
/// of [`ArchiveVersion::is_chunked`].
pub(crate) fn header_only(archive: ArchiveVersion) -> bool {
    !archive.is_chunked()
}

/// Inspect a Rhino stream, applying the version-specific scan depth.
pub(crate) fn inspect(root: View<'_>) -> Result<ContainerSummary, CodecError> {
    let data = acquire(root);
    let header = parse_header(data).map_err(framing_error)?;
    if header_only(header.archive_version) {
        // The properties table is not read on this path, so no openNURBS
        // writer-version stamp is declared.
        let dialects = vec![header.archive_version.classify(None)];
        return Ok(ContainerSummary {
            dialects,
            format: crate::dialect::FORMAT.to_string(),
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

/// Decode a Rhino stream according to the supported container depth.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    container_only: bool,
) -> Result<cadmpeg_ir::codec::DecodeResult, CodecError> {
    let data = acquire(root);
    let header = parse_header(data).map_err(framing_error)?;
    if header.archive_version == ArchiveVersion::V1 {
        return crate::legacy::decode_v1(data);
    }
    let scan = scan(data)?;
    if container_only && scan.archive.is_chunked() {
        return Ok(container_only_result(&scan));
    }
    Ok(crate::decode::decode(
        &scan,
        crate::mesh::MeshExpand::new(ctx, root),
    ))
}

#[cfg(test)]
pub(crate) mod tests;
