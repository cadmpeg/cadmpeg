// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino 3DM container scanning and summary construction.

use crate::loss::Diagnostics;
use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::ContainerSummary;

use crate::chunks::{
    admitted_vec, checked_count_bytes, checksum_children_through_class_end, chunk_at,
    direct_checksum_ranges, parse_header, validate_eof, verify_checksum, verify_checksum_ranges,
    ArchiveVersion, BoundedReader, ChecksumStatus, FramingError, TCODE_CRC, TCODE_ENDOFFILE,
    TCODE_ENDOFTABLE,
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
    comment: Record,
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
    /// Validated EOF descriptor.
    eof_offset: usize,
    /// Recoverable checksum and unknown-record notes.
    pub(crate) warnings: Diagnostics,
    /// Typed metadata decoded from property, setting, and layer records.
    pub(crate) metadata: crate::settings::DocumentMetadata,
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
                offset: offset as u64,
            },
            "rhino chunk framing",
        ),
        other => CodecError::Malformed(other.to_string()),
    }
}

fn checksum_children_warning(typecode: u32, offset: usize, error: &FramingError) -> String {
    format!(
        "checksum child framing at offset {offset} for typecode {typecode:#x} could not be verified: {error}"
    )
}

fn checksum_warning(
    ctx: &DecodeContext<'_>,
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
        crate::chunks::verify_checksum_ranges(data, &chunk, &[])
    } else if matches!(
        typecode,
        TCODE_NAMED_PLANES | TCODE_NAMED_VIEWS | TCODE_VIEWS
    ) {
        let mut reservation = ctx.reserve_scoped(0, "Rhino view checksum ranges")?;
        let children = match list_checksum_children(ctx, data, &chunk, archive, &mut reservation) {
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
        let children = match mesh_checksum_children(ctx, data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_RENDER_SETTINGS {
        let children = match render_settings_checksum_children(ctx, data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_SETTINGS_ATTRIBUTES {
        let children = match settings_attributes_checksum_children(ctx, data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_PLUGIN_LIST {
        let children = match plugin_list_checksum_children(ctx, data, &chunk, archive) {
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
            ctx,
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
        let children = match compressed_preview_checksum_children(ctx, data, &chunk, archive) {
            Ok(children) => children,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                return Ok(Some(checksum_children_warning(typecode, offset, &error)));
            }
        };
        let direct = direct_checksum_ranges(&chunk.body(), &children).map_err(framing_error)?;
        verify_checksum_ranges(data, &chunk, &direct)
    } else if typecode == TCODE_USER_TABLE_UUID {
        let children = match user_table_uuid_checksum_children(ctx, data, &chunk, archive) {
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
    ctx: &DecodeContext<'_>,
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<Vec<std::ops::Range<usize>>, FramingError> {
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let mut children = Vec::new();
    if let Some(child) = mesh_subd_checksum_child(data, &mut reader, archive)? {
        crate::chunks::reserve_admitted_vec(ctx, &mut children, 1, "Rhino mesh checksum children")?;
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
    ctx: &DecodeContext<'_>,
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
    let mut children =
        crate::chunks::admitted_vec(ctx, 1, "Rhino render settings checksum children")?;
    children.push(child);
    Ok(children)
}

/// Returns the complete nested chunks in a settings-attributes body.
fn settings_attributes_checksum_children(
    ctx: &DecodeContext<'_>,
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
        crate::chunks::reserve_admitted_vec(
            ctx,
            &mut children,
            1,
            "Rhino settings checksum children",
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
        crate::chunks::reserve_admitted_vec(
            ctx,
            &mut children,
            1,
            "Rhino settings checksum children",
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
        crate::chunks::reserve_admitted_vec(
            ctx,
            &mut children,
            1,
            "Rhino settings checksum children",
        )?;
        children.push(child);
    }
    if minor >= 6 {
        if let Some(child) = mesh_subd_checksum_child(data, &mut reader, archive)? {
            crate::chunks::reserve_admitted_vec(
                ctx,
                &mut children,
                1,
                "Rhino settings checksum children",
            )?;
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
    ctx: &DecodeContext<'_>,
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
            crate::chunks::reserve_admitted_vec(
                ctx,
                &mut children,
                1,
                "Rhino preview checksum children",
            )?;
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
            crate::chunks::reserve_admitted_vec(
                ctx,
                &mut children,
                1,
                "Rhino preview checksum children",
            )?;
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
            crate::chunks::reserve_admitted_vec(
                ctx,
                &mut children,
                1,
                "Rhino preview checksum children",
            )?;
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
    ctx: &DecodeContext<'_>,
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
    let mut children = crate::chunks::admitted_vec(ctx, 1, "Rhino user table checksum children")?;
    children.push(child.range());
    Ok(children)
}

/// Returns the complete nested chunks after a counted view-list prefix.
///
/// The list CRC covers the count and any direct suffix bytes, but not these
/// complete child chunks. A malformed child has no recoverable checksum range;
/// the owning view parser reports that framing failure separately.
fn list_checksum_children(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    archive: ArchiveVersion,
    reservation: &mut ScopedReservation<'_>,
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
    let first_child_offset = offset;
    for _ in 0..child_count {
        let child = chunk_at(data, offset, chunk.body().end, archive, false)?;
        offset = child.next_offset();
    }
    ctx.charge_work(
        u64::try_from(child_count).map_err(|_| FramingError::Overflow {
            offset: first_child_offset,
        })?,
        "Rhino view checksum child ranges",
    )
    .map_err(|error| match error {
        CodecError::ResourceLimit(limit) => FramingError::Resource(limit),
        other => FramingError::structural(first_child_offset, other.to_string()),
    })?;
    let range_bytes =
        u64::try_from(std::mem::size_of::<std::ops::Range<usize>>()).map_err(|_| {
            FramingError::Overflow {
                offset: first_child_offset,
            }
        })?;
    let total_bytes = u64::try_from(child_count)
        .ok()
        .and_then(|count| count.checked_mul(range_bytes))
        .ok_or(FramingError::Overflow {
            offset: first_child_offset,
        })?;
    reservation.grow(total_bytes).map_err(|error| match error {
        CodecError::ResourceLimit(limit) => FramingError::Resource(limit),
        other => FramingError::structural(first_child_offset, other.to_string()),
    })?;
    let mut children = Vec::new();
    children.try_reserve_exact(child_count).map_err(|_| {
        FramingError::Resource(cadmpeg_core::decode::ResourceLimit {
            dimension: cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            reason: cadmpeg_core::decode::ResourceFailure::AllocationFailed,
            limit: u64::MAX,
            used: 0,
            additional: total_bytes,
            operation: "Rhino view checksum ranges",
        })
    })?;
    offset = first_child_offset;
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
    ctx: &DecodeContext<'_>,
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
    let mut children = admitted_vec(ctx, child_count, "Rhino plugin-list child ranges")?;
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

fn count_object_typecode(
    ctx: &DecodeContext<'_>,
    counts: &mut BTreeMap<u32, usize>,
    typecode: u32,
) -> Result<(), CodecError> {
    if !counts.contains_key(&typecode) {
        ctx.charge_collection_items(1, "Rhino object typecode counts")?;
    }
    *counts.entry(typecode).or_insert(0) += 1;
    Ok(())
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
    let comment = Record::from_chunk(
        &chunk_at(data, comment_offset, data.len(), archive, false).map_err(framing_error)?,
    );
    if comment.typecode != TCODE_COMMENT || comment.is_short() {
        return Err(CodecError::Malformed(
            "first post-header chunk is not a long comment".to_string(),
        ));
    }
    let mut warnings = Diagnostics::new();
    if let Some(note) = checksum_warning(
        ctx,
        data,
        comment.typecode,
        comment_offset,
        data.len(),
        archive,
    )? {
        warnings.push_coded_admitted(ctx, crate::loss::RhinoLossCode::IntegrityFailure, format_args!("{note}"))?;
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
            validate_eof(data, offset, archive).map_err(framing_error)?;
            let mut metadata =
                crate::settings::parse_metadata(ctx, data, archive, &tables, &mut warnings)?;
            let all_objects = resolve_identities(ctx, all_objects, &metadata, &mut warnings)?;
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
        if chunk.short() {
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
                .filter(|record| record.typecode == TCODE_WRITER_VERSION)
                .find_map(Record::short_value)
        } else {
            None
        };
        let mut child_offset = chunk.body().start;
        let mut terminated = false;
        while child_offset < chunk.body().end {
            let child = chunk_at(data, child_offset, chunk.body().end, archive, false)
                .map_err(framing_error)?;
            if child.typecode == TCODE_ENDOFTABLE {
                if !child.short() || child.value() != 0 {
                    return Err(CodecError::Malformed(
                        "end-of-table marker must be short with value zero".to_string(),
                    ));
                }
                if child.next_offset() != chunk.body().end {
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
                if known_record(record.typecode) {
                    return Err(CodecError::malformed(format_args!(
                        "record typecode {:#x} is invalid or short-framed in table {:#x}",
                        record.typecode, chunk.typecode
                    )));
                }
                warnings.push_admitted(ctx, format_args!(
                    "unknown bounded record {:#x} skipped in table {:#x} at offset {child_offset}",
                    record.typecode, chunk.typecode
                ))?;
            }
            if let Some(note) = checksum_warning(
                ctx,
                data,
                record.typecode,
                child_offset,
                chunk.body().end,
                archive,
            )? {
                warnings.push_coded_admitted(ctx, crate::loss::RhinoLossCode::IntegrityFailure, format_args!("{note}"))?;
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
                        return Err(CodecError::ResourceLimit(limit))
                    }
                    Err(error) => {
                        warnings.push_admitted(ctx, format_args!(
                            "bounded object record at {child_offset} is malformed: {error}"
                        ))?;
                        degraded_object_record(ctx, &record, &error)?
                    }
                };
                let typecode = descriptor.framed().map_or(0, |object| object.object_type);
                count_object_typecode(ctx, &mut object_typecodes, typecode)?;
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
            child_offset = child.next_offset();
        }
        if !terminated {
            warnings.push_admitted(ctx, format_args!(
                "table {:#x} has no end-of-table marker",
                chunk.typecode
            ))?;
        }
        if let Some(note) = checksum_warning(
            ctx,
            data,
            chunk.typecode,
            offset,
            chunk.next_offset(),
            archive,
        )? {
            warnings.push_coded_admitted(ctx, crate::loss::RhinoLossCode::IntegrityFailure, format_args!("{note}"))?;
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
        let table = Table::new(
            chunk.typecode,
            offset..chunk.next_offset(),
            chunk.body(),
            records,
            table_record_count,
            object_typecodes,
        )
        .ok_or_else(|| {
            framing_error(FramingError::structural(
                offset,
                "table chunk declares a body that does not fit its framing",
            ))
        })?;
        tables.push(table);
        offset = chunk.next_offset();
    }
    Err(CodecError::Malformed(
        "missing end-of-file chunk".to_string(),
    ))
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
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "Rhino container summary attributes")?;
    let key = crate::wire::admitted_format(ctx, key, "Rhino container summary attribute key")?;
    let value = crate::wire::admitted_format(ctx, value, "Rhino container summary attribute value")?;
    attributes.insert(key, value);
    Ok(())
}

fn push_container_note(
    ctx: &DecodeContext<'_>,
    notes: &mut Vec<String>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    crate::wire::reserve_collection(ctx, notes, 1, "Rhino container summary notes")?;
    notes.push(crate::wire::admitted_format(
        ctx,
        value,
        "Rhino container summary note text",
    )?);
    Ok(())
}

/// Build the format-neutral container summary.
fn summarize(ctx: &DecodeContext<'_>, scan: &Scan<'_>) -> Result<ContainerSummary, CodecError> {
    let mut entries = Vec::new();
    for table in &scan.tables {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(ctx, &mut attributes, format_args!("offset"), format_args!("{}", table.range().start))?;
        insert_summary_attribute(ctx, &mut attributes, format_args!("size"), format_args!("{}", table.range().len()))?;
        insert_summary_attribute(ctx, &mut attributes, format_args!("body_offset"), format_args!("{}", table.body().start))?;
        insert_summary_attribute(ctx, &mut attributes, format_args!("record_count"), format_args!("{}", table.record_count))?;
        for (typecode, count) in &table.object_typecodes {
            insert_summary_attribute(ctx, &mut attributes, format_args!("object_typecode_{typecode:#x}"), format_args!("{count}"))?;
        }
        let storage = table
            .body_bytes(scan.data)
            .map_or(EntryStorage::unreported(VerbatimLabel::None), |body| {
                EntryStorage::framed_by(VerbatimLabel::None, body.into(), table.framing())
            });
        crate::wire::reserve_collection(ctx, &mut entries, 1, "Rhino container summary entries")?;
        entries.push(ContainerEntry {
            name: crate::wire::admitted_format(ctx, format_args!("table-{:#x}", table.typecode), "Rhino container entry name")?,
            role: ContainerRole::Table,
            storage,
            attributes,
        });
    }
    let mut classes = BTreeMap::<Uuid, (usize, usize)>::new();
    for object in &scan.objects {
        // The container report groups degraded records under the nil class UUID.
        let class_uuid = object.class_uuid().unwrap_or_else(Uuid::nil);
        if !classes.contains_key(&class_uuid) {
            ctx.charge_collection_items(1, "Rhino container class groups")?;
        }
        let entry = classes.entry(class_uuid).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += object.range().len();
    }
    for (class_uuid, (count, bytes)) in classes {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(ctx, &mut attributes, format_args!("class_uuid"), format_args!("{class_uuid}"))?;
        insert_summary_attribute(ctx, &mut attributes, format_args!("nil_uuid"), format_args!("{}", class_uuid.is_nil()))?;
        insert_summary_attribute(ctx, &mut attributes, format_args!("count"), format_args!("{count}"))?;
        insert_summary_attribute(ctx, &mut attributes, format_args!("total_record_bytes"), format_args!("{bytes}"))?;
        crate::wire::reserve_collection(ctx, &mut entries, 1, "Rhino container summary entries")?;
        entries.push(ContainerEntry {
            name: crate::wire::admitted_format(ctx, format_args!("class-{class_uuid}"), "Rhino container entry name")?,
            role: ContainerRole::ObjectClass,
            storage: EntryStorage::verbatim(VerbatimLabel::None, bytes as u64),
            attributes,
        });
    }
    let mut notes = Vec::new();
    push_container_note(ctx, &mut notes, format_args!("archive version {}", scan.archive.value()))?;
    for warning in scan.warnings.messages() {
        push_container_note(ctx, &mut notes, format_args!("{warning}"))?;
    }
    for diagnostic in scan.definitions.diagnostics() {
        push_container_note(ctx, &mut notes, format_args!("{}", diagnostic.diagnostic.message))?;
    }
    let matched = dialect_match(scan);
    let mut losses = Vec::new();
    if let Some(loss) = crate::dialect::admission_loss(ctx, &matched)? {
        crate::wire::reserve_collection(ctx, &mut losses, 1, "Rhino container summary losses")?;
        losses.push(loss);
    }
    Ok(ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched),
        cadmpeg_ir::ContainerKind::ThreeDmChunks,
        entries,
        losses,
        notes,
    ))
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
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<NonBlankString, String>,
    key: &'static str,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "Rhino source metadata attributes")?;
    let key = crate::wire::copy_retained_string(ctx, key, "Rhino source metadata key")?;
    let key = NonBlankString::new(key)
        .ok_or_else(|| CodecError::malformed("generated Rhino source metadata key is blank"))?;
    let value = crate::wire::admitted_format(ctx, value, "Rhino source metadata value")?;
    attributes.insert(key, value);
    Ok(())
}

/// Builds source metadata; `primary` is the one author of the document's identity.
pub(crate) fn source_meta(
    ctx: &DecodeContext<'_>,
    primary: DialectMatch,
    detail: SourceMetaDetail<'_>,
) -> Result<SourceMeta, CodecError> {
    let attributes = match detail {
        SourceMetaDetail::FlatLegacyArchive => {
            let mut attributes = BTreeMap::new();
            insert_source_meta_attribute(ctx, &mut attributes, "archive_version", format_args!("1"))?;
            attributes
        }
        SourceMetaDetail::ContainerOnly(scan) => {
            let mut attributes = BTreeMap::new();
            chunked_source_attributes(ctx, scan, &mut attributes)?;
            insert_source_meta_attribute(ctx, &mut attributes, "comment_offset", format_args!("{}", scan.comment.range.start))?;
            insert_source_meta_attribute(ctx, &mut attributes, "eof_offset", format_args!("{}", scan.eof_offset))?;
            insert_source_meta_attribute(ctx, &mut attributes, "table_count", format_args!("{}", scan.tables.len()))?;
            insert_source_meta_attribute(ctx, &mut attributes, "instance_definition_count", format_args!("{}", scan.definitions.definitions().len()))?;
            attributes
        }
        SourceMetaDetail::Full {
            scan,
            attributes: mut full,
        } => {
            chunked_source_attributes(ctx, scan, &mut full)?;
            full
        }
    };
    Ok(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(primary),
        attributes,
    ))
}

fn chunked_source_attributes(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    attributes: &mut BTreeMap<NonBlankString, String>,
) -> Result<(), CodecError> {
    insert_source_meta_attribute(ctx, attributes, "archive_version", format_args!("{}", scan.archive.value()))?;
    insert_source_meta_attribute(ctx, attributes, "container_kind", format_args!("3dm-chunks"))?;
    Ok(())
}

/// Build an empty current-version IR and a container-only report.
pub(crate) fn container_only_result(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
) -> Result<Decoded, CodecError> {
    let mut notes = Vec::new();
    push_container_note(ctx, &mut notes, format_args!("archive version {}", scan.archive.value()))?;
    for warning in scan.warnings.messages() {
        push_container_note(ctx, &mut notes, format_args!("{warning}"))?;
    }
    for diagnostic in scan.definitions.diagnostics() {
        push_container_note(ctx, &mut notes, format_args!("{}", diagnostic.diagnostic.message))?;
    }
    let mut losses = Vec::new();
    for diagnostic in scan.warnings.iter() {
        crate::wire::reserve_collection(ctx, &mut losses, 1, "Rhino container-only losses")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(diagnostic.message.len()),
            "Rhino container-only loss message",
        )?;
        losses.push(
            diagnostic
                .code
                .unwrap_or(crate::loss::RhinoLossCode::ContainerScanDiagnostic)
                .note(&diagnostic.message),
        );
    }
    for diagnostic in scan.definitions.diagnostics() {
        crate::wire::reserve_collection(ctx, &mut losses, 1, "Rhino container-only losses")?;
        losses.push(diagnostic.to_loss(ctx)?);
    }
    let primary = dialect_match(scan);
    if let Some(loss) = crate::dialect::admission_loss(ctx, &primary)? {
        crate::wire::reserve_collection(ctx, &mut losses, 1, "Rhino container-only losses")?;
        losses.push(loss);
    }
    let ir = CadIr::decoded(source_meta(ctx, primary, SourceMetaDetail::ContainerOnly(scan))?);
    Ok(Decoded {
        ir,
        body: DecodeBody {
            transfer: cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
            coverage: cadmpeg_ir::report::decode::Coverage::default(),
            losses,
            notes,
            transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
        },
        source_fidelity: cadmpeg_ir::SourceFidelity::default(),
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
        if let Some(loss) = crate::dialect::admission_loss(ctx, &matched)? {
            crate::wire::reserve_collection(ctx, &mut losses, 1, "Rhino container summary losses")?;
            losses.push(loss);
        }
        let mut notes = Vec::new();
        push_container_note(ctx, &mut notes, format_args!("archive version {}", header.archive_version.value()))?;
        return Ok(ContainerSummary::classified(
            cadmpeg_core::dialect::DialectLayers::of(matched),
            cadmpeg_ir::ContainerKind::ThreeDmChunks,
            Vec::new(),
            losses,
            notes,
        ));
    }
    summarize(ctx, &scan(ctx, data)?)
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
