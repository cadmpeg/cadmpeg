// SPDX-License-Identifier: Apache-2.0
//! Rhino object-record identity and framing.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::chunks::{
    chunk_at, direct_checksum_ranges, verify_checksum, verify_checksum_ranges, ArchiveVersion,
    BoundedReader, ChecksumStatus, FramingError,
};
use crate::container::Record;
use crate::layout::class_uuid_chunk_body as class_uuid_body;
use crate::settings::{self, DocumentMetadata, SourceRange, Xform};
use crate::wire::Uuid;

const OBJECT_RECORD_TYPE: u32 = 0x8200_0071;
const OBJECT_RECORD_ATTRIBUTES: u32 = 0x0200_8072;
const OBJECT_RECORD_ATTRIBUTES_USERDATA: u32 = 0x0200_0073;
const OBJECT_RECORD_HISTORY: u32 = 0x0200_8074;
const OBJECT_RECORD_END: u32 = 0x8200_007f;
const OPENNURBS_CLASS: u32 = 0x0002_7ffa;
const CLASS_USERDATA: u32 = 0x0002_7ffd;
const CLASS_USERDATA_HEADER: u32 = 0x0002_fff9;
const CLASS_UUID: u32 = 0x0002_fffb;
const CLASS_DATA: u32 = 0x0002_fffc;
const CLASS_END: u32 = 0x8002_7fff;
const ANONYMOUS: u32 = 0x4000_8000;
const LEGACY_OBJECT_ATTRIBUTES_CUTOFF: i64 = 200_712_190;
pub(crate) const USER_STRING_LIST: Uuid = Uuid::from_canonical([
    0xce, 0x28, 0xde, 0x29, 0xf4, 0xc5, 0x4f, 0xaa, 0xa5, 0x0a, 0xc3, 0xa6, 0x84, 0x9b, 0x63, 0x29,
]);
pub(crate) const OBSOLETE_CUSTOM_MESH_USERDATA: Uuid = Uuid::from_canonical([
    0x69, 0xf2, 0x76, 0x95, 0x30, 0x11, 0x4f, 0xba, 0x82, 0xc1, 0xe5, 0x29, 0xf2, 0x5b, 0x5f, 0xd9,
]);
pub(crate) const PER_OBJECT_MESH_PARAMETERS_USERDATA: Uuid = Uuid::from_canonical([
    0xb5, 0x62, 0x8c, 0xa9, 0x82, 0xc4, 0x4c, 0xae, 0x98, 0x83, 0x48, 0x7b, 0x3e, 0x4a, 0xb2, 0x8b,
]);
const HISTORY_HEADER: u32 = 0x0200_8075;
const HISTORY_DATA: u32 = 0x0200_8076;
const HIDDEN_OBJECT_MODE: u8 = 1;
const IDEF_OBJECT_MODE: u8 = 3;

/// A class-userdata descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UserdataDescriptor {
    Known(ClassUserdata),
    UnknownVersion {
        range: Range<usize>,
        version: (u8, u8),
        payload_range: Range<usize>,
    },
}

/// Fields supplied by recognized class-userdata framing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassUserdata {
    pub(crate) range: Range<usize>,
    pub(crate) version: (u8, u8),
    pub(crate) class_uuid: Uuid,
    pub(crate) item_uuid: Uuid,
    pub(crate) copy_count: i32,
    pub(crate) transform_range: Range<usize>,
    pub(crate) application_uuid: Option<Uuid>,
    pub(crate) save_context: Option<UserdataSaveContext>,
    pub(crate) payload_range: Range<usize>,
}

/// Save metadata stored together by class-userdata version 2.2 and later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UserdataSaveContext {
    pub(crate) last_saved_as_goo: bool,
    pub(crate) archive_version: i32,
    pub(crate) writer_version: i32,
}

impl UserdataDescriptor {
    pub(crate) fn known(&self) -> Option<&ClassUserdata> {
        match self {
            Self::Known(value) => Some(value),
            Self::UnknownVersion { .. } => None,
        }
    }
}

/// An attribute-userdata record, retained independently of object attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttributeUserdataDescriptor {
    Unknown { range: Range<usize> },
    Known(AttributeUserdata),
}

/// Fields supplied by recognized attribute-userdata framing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeUserdata {
    pub(crate) range: Range<usize>,
    pub(crate) class_uuid: Uuid,
    pub(crate) item_uuid: Uuid,
    pub(crate) application_uuid: Option<Uuid>,
    pub(crate) writer_version: Option<i64>,
    pub(crate) payload_range: Range<usize>,
}

impl AttributeUserdataDescriptor {
    pub(crate) fn known(&self) -> Option<&AttributeUserdata> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown { .. } => None,
        }
    }
}

/// Raw object attributes decoded from an object-attributes chunk.
/// Which source an object's display color is taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorSource {
    Layer,
    Object,
    Material,
    Parent,
    /// A selector outside the documented set, retained as written.
    Invalid(u8),
}

impl ColorSource {
    pub(crate) fn parse(raw: u8) -> Self {
        match raw {
            0 => Self::Layer,
            1 => Self::Object,
            2 => Self::Material,
            3 => Self::Parent,
            other => Self::Invalid(other),
        }
    }

    pub(crate) fn as_byte(self) -> u8 {
        match self {
            Self::Layer => 0,
            Self::Object => 1,
            Self::Material => 2,
            Self::Parent => 3,
            Self::Invalid(raw) => raw,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ObjectAttributes {
    /// Complete source range.
    pub(crate) source: SourceRange,
    /// Packed attribute version.
    pub(crate) version: (u8, u8),
    /// Raw object UUID.
    pub(crate) object_id: Uuid,
    /// Raw layer archive index.
    pub(crate) layer_index: i32,
    /// Raw render-material archive index.
    pub(crate) material_index: i32,
    /// Raw object color.
    pub(crate) color: [u8; 4],
    /// Obsolete line style width.
    pub(crate) obsolete_line_style: i16,
    /// Obsolete line style index.
    pub(crate) obsolete_line_style_index: i16,
    /// Obsolete thickness.
    pub(crate) obsolete_thickness: f64,
    /// Obsolete scale.
    pub(crate) obsolete_scale: f64,
    /// Raw visibility.
    pub(crate) visible: bool,
    /// Color source selector.
    pub(crate) color_source: ColorSource,
    /// Raw linetype source selector.
    pub(crate) linetype_source: u8,
    /// Raw material source selector.
    pub(crate) material_source: u8,
    /// Raw plot-color source selector.
    pub(crate) plot_color_source: u8,
    /// Raw plot-weight source selector.
    pub(crate) plot_weight_source: u8,
    /// Raw linetype archive index.
    pub(crate) linetype_index: i32,
    /// Raw plot color.
    pub(crate) plot_color: [u8; 4],
    /// Raw plot weight in millimeters.
    pub(crate) plot_weight: f64,
    /// Raw object mode.
    pub(crate) object_mode: u8,
    /// Raw decoration flags.
    pub(crate) decoration: i32,
    /// Raw wire density.
    pub(crate) wire_density: i32,
    /// Raw object name.
    pub(crate) name: String,
    /// Raw object URL.
    pub(crate) url: String,
    /// Bounded rendering-attributes payload range.
    pub(crate) rendering_range: Option<Range<usize>>,
    /// Referenced group indexes.
    pub(crate) groups: Vec<i32>,
    /// Viewport/display-material pairs.
    pub(crate) display_materials: Vec<(Uuid, Uuid)>,
    /// Active space selector.
    pub(crate) active_space: u8,
    /// Viewport selector.
    pub(crate) viewport_id: Uuid,
    /// Display order.
    pub(crate) display_order: i32,
    /// Clipping-plane participation selector.
    pub(crate) clip_participation_source: u8,
    /// Clipping proof flag.
    pub(crate) clipping_proof: bool,
    /// Clipping-plane UUIDs.
    pub(crate) clipping_plane_ids: Vec<Uuid>,
    /// Section-attributes source selector.
    pub(crate) section_attributes_source: u8,
    /// Hatch-pattern archive index.
    pub(crate) hatch_pattern_index: i32,
    /// Section-hatch scale.
    pub(crate) section_hatch_scale: f64,
    /// Section-hatch rotation.
    pub(crate) section_hatch_rotation: f64,
    /// Linetype-pattern scale.
    pub(crate) linetype_pattern_scale: f64,
    /// Hatch background color.
    pub(crate) hatch_background: [u8; 4],
    /// Whether hatch boundaries are visible.
    pub(crate) hatch_boundary_visible: bool,
    /// Whether a detail requests its display-mode background.
    pub(crate) detail_background_visible: bool,
    /// Object frame transform.
    pub(crate) object_frame: Option<Xform>,
    /// Section-fill rule.
    pub(crate) section_fill_rule: u8,
    /// Obsolete line-cap source.
    pub(crate) line_cap_source: u8,
    /// Obsolete line-cap style.
    pub(crate) line_cap_style: u8,
    /// Obsolete line-join source.
    pub(crate) line_join_source: u8,
    /// Obsolete line-join style.
    pub(crate) line_join_style: u8,
    /// Clipping-plane label style.
    pub(crate) clipping_plane_label_style: u8,
    /// Obsolete selective-clipping-list flag.
    pub(crate) selective_clipping_list: bool,
    /// Direct embedded linetype.
    pub(crate) embedded_linetype: Option<settings::EmbeddedDescriptor>,
    /// Direct embedded section style.
    pub(crate) embedded_section_style: Option<settings::EmbeddedDescriptor>,
    /// Per-object custom render-mesh settings.
    pub(crate) custom_render_mesh: Option<settings::MeshParameters>,
    /// Per-object mesh modifier userdata.
    pub(crate) mesh_modifiers: Option<crate::mesh_modifiers::MeshModifiers>,
}

/// Resolved source identity and display state for one object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceIdentity {
    /// Stable source identifier.
    pub(crate) source_id: String,
    /// Raw object UUID.
    pub(crate) object_id: Uuid,
    /// Object class UUID.
    pub(crate) class_uuid: Uuid,
    /// Object name.
    pub(crate) name: String,
    /// Raw layer archive index.
    pub(crate) layer_index: i32,
    /// Resolved layer from the document layer table.
    pub(crate) layer: Option<LayerRef>,
    /// Effective display color.
    pub(crate) effective_color: Option<[u8; 4]>,
    /// Effective visibility after layer combination.
    pub(crate) effective_visible: bool,
    /// Raw object mode.
    pub(crate) object_mode: u8,
    /// Object frame transform.
    pub(crate) object_frame: Option<Xform>,
    /// Complete source range.
    pub(crate) source: SourceRange,
}

/// Resolved layer identity from one document lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayerRef {
    /// Layer UUID, when the layer record carries one.
    pub(crate) id: Option<Uuid>,
    /// Layer name.
    pub(crate) name: String,
}

/// Builds a stable source ID without minting a `CadIr` entity ID.
pub(crate) fn stable_source_id(scope: &str, kind: &str, key: &str) -> String {
    format!("rhino:{scope}:{kind}#{key}")
}

/// A bounded object-history descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryDescriptor {
    /// Complete history wrapper range.
    pub(crate) range: Range<usize>,
    /// Packed history version.
    pub(crate) version: (u8, u8),
    /// History header child range.
    pub(crate) header_range: Option<Range<usize>>,
    /// History data child range.
    pub(crate) data_range: Option<Range<usize>>,
}

/// Attribute payload admission state for a framed object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AttributeState {
    Missing,
    Degraded,
    Parsed(Box<ObjectAttributes>),
}

impl AttributeState {
    pub(crate) fn parsed(&self) -> Option<&ObjectAttributes> {
        match self {
            Self::Parsed(value) => Some(value),
            Self::Missing | Self::Degraded => None,
        }
    }
}

/// A fully framed Rhino object record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectDescriptor<I = SourceIdentity> {
    /// Complete object-record range.
    pub(crate) range: Range<usize>,
    /// Object type filter bits.
    pub(crate) object_type: u32,
    /// Class UUID.
    pub(crate) class_uuid: Uuid,
    /// Class-data payload range.
    pub(crate) class_data_range: Range<usize>,
    /// Attribute payload admission state.
    pub(crate) attributes: AttributeState,
    /// Attribute-userdata descriptors.
    pub(crate) attributes_userdata: Vec<AttributeUserdataDescriptor>,
    /// Resolved source identity.
    pub(crate) identity: I,
    /// Class userdata descriptors.
    pub(crate) userdata: Vec<UserdataDescriptor>,
    /// Optional history descriptor.
    pub(crate) history: Option<HistoryDescriptor>,
    /// Unknown bounded trailer child ranges.
    pub(crate) unknown_trailer: Vec<Range<usize>>,
    /// Checksum warning messages.
    pub(crate) checksum_warnings: Vec<String>,
    /// Object-local attribute and identity warnings.
    pub(crate) warnings: Vec<String>,
}

/// A scanned object record: framed contents, or a degraded outer range.
#[derive(Debug, Clone, PartialEq)]
// Framed records are the common case; retain their descriptors inline during table traversal.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ObjectRecord<I = SourceIdentity> {
    /// Bounded inner framing was malformed; only the outer range survived.
    Degraded {
        /// Complete object-record range.
        range: Range<usize>,
        /// Framing failure note.
        warning: String,
    },
    /// Fully framed object record.
    Framed(ObjectDescriptor<I>),
}

impl<I> ObjectRecord<I> {
    pub(crate) fn range(&self) -> Range<usize> {
        match self {
            Self::Degraded { range, .. } => range.clone(),
            Self::Framed(object) => object.range.clone(),
        }
    }

    pub(crate) fn framed(&self) -> Option<&ObjectDescriptor<I>> {
        match self {
            Self::Framed(object) => Some(object),
            Self::Degraded { .. } => None,
        }
    }

    pub(crate) fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded { .. })
    }

    pub(crate) fn class_uuid(&self) -> Option<Uuid> {
        self.framed().map(|object| object.class_uuid)
    }
}

impl ObjectRecord {
    pub(crate) fn identity(&self) -> Option<&SourceIdentity> {
        self.framed().map(|object| &object.identity)
    }
}

/// A fully framed Rhino class wrapper used by table records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassDescriptor {
    /// Class UUID.
    pub(crate) class_uuid: Uuid,
    /// Class-data payload range, excluding its chunk framing.
    pub(crate) class_data_range: Range<usize>,
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn require_long(chunk: &crate::chunks::Chunk, typecode: u32) -> Result<(), FramingError> {
    if chunk.typecode != typecode || chunk.short() {
        return Err(FramingError::structural(
            chunk.header_start,
            format!(
                "expected long chunk {typecode:#x}, got {:#x}",
                chunk.typecode
            ),
        ));
    }
    Ok(())
}

fn require_short_zero(chunk: &crate::chunks::Chunk, typecode: u32) -> Result<(), FramingError> {
    if chunk.typecode != typecode || !chunk.short() || chunk.value() != 0 {
        return Err(FramingError::structural(
            chunk.header_start,
            format!(
                "expected short zero chunk {typecode:#x}, got {:#x}",
                chunk.typecode
            ),
        ));
    }
    Ok(())
}

fn checksum_warning(
    bytes: &[u8],
    chunk: &crate::chunks::Chunk,
) -> Result<Option<String>, FramingError> {
    match verify_checksum(bytes, chunk)? {
        ChecksumStatus::Mismatch { expected, actual } => Ok(Some(format!(
            "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
            chunk.header_start, chunk.typecode
        ))),
        _ => Ok(None),
    }
}

fn checksum_warning_excluding(
    bytes: &[u8],
    chunk: &crate::chunks::Chunk,
    children: &[Range<usize>],
) -> Result<Option<String>, FramingError> {
    let direct = direct_checksum_ranges(&chunk.body(), children)?;
    match verify_checksum_ranges(bytes, chunk, &direct)? {
        ChecksumStatus::Mismatch { expected, actual } => Ok(Some(format!(
            "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
            chunk.header_start, chunk.typecode
        ))),
        _ => Ok(None),
    }
}

/// Parses a table-record Rhino class wrapper without decoding its payload.
pub(crate) fn parse_class_wrapper(
    bytes: &[u8],
    body: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<ClassDescriptor, FramingError> {
    parse_class_wrapper_with_userdata(bytes, body, archive, warnings)
        .map(|(descriptor, _)| descriptor)
}

/// Parses a class wrapper and retains its ordered class-userdata descriptors.
pub(crate) fn parse_class_wrapper_with_userdata(
    bytes: &[u8],
    body: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(ClassDescriptor, Vec<UserdataDescriptor>), FramingError> {
    let wrapper = chunk_at(bytes, body.start, body.end, archive, false)?;
    require_long(&wrapper, OPENNURBS_CLASS)?;
    let uuid_chunk = chunk_at(
        bytes,
        wrapper.body().start,
        wrapper.body().end,
        archive,
        true,
    )?;
    require_long(&uuid_chunk, CLASS_UUID)?;
    if uuid_chunk.declared_end() - uuid_chunk.body().start != class_uuid_body::LEN {
        return Err(FramingError::structural(
            uuid_chunk.header_start,
            "class UUID chunk must have a 20-byte body",
        ));
    }
    if let Some(note) = checksum_warning(bytes, &uuid_chunk)? {
        warnings.push(note);
    }
    let class_uuid = Uuid::from_wire(
        bytes[uuid_chunk.body().start..uuid_chunk.body().start + class_uuid_body::CRC32]
            .try_into()
            .expect("UUID length checked"),
    );
    if class_uuid == Uuid::nil() {
        if uuid_chunk.next_offset() != wrapper.body().end {
            return Err(FramingError::structural(
                uuid_chunk.next_offset(),
                "null class wrapper has trailing bytes",
            ));
        }
        return Ok((
            ClassDescriptor {
                class_uuid,
                class_data_range: uuid_chunk.next_offset()..uuid_chunk.next_offset(),
            },
            Vec::new(),
        ));
    }
    let data_chunk = chunk_at(
        bytes,
        uuid_chunk.next_offset(),
        wrapper.body().end,
        archive,
        false,
    )?;
    require_long(&data_chunk, CLASS_DATA)?;
    // CLASS_DATA checksum coverage is defined by the concrete class grammar.
    // The wrapper scanner cannot distinguish direct bytes from embedded chunks,
    // so it must not report a checksum result for this mixed payload.
    let mut offset = data_chunk.next_offset();
    let mut end_seen = false;
    let mut userdata = Vec::new();
    while offset < wrapper.body().end {
        let item = chunk_at(bytes, offset, wrapper.body().end, archive, false)?;
        if item.typecode == CLASS_USERDATA {
            require_long(&item, CLASS_USERDATA)?;
            userdata.push(parse_userdata(bytes, &item, archive, warnings)?);
            offset = item.next_offset();
        } else {
            require_short_zero(&item, CLASS_END)?;
            offset = item.next_offset();
            end_seen = true;
            break;
        }
    }
    if !end_seen || offset != wrapper.body().end || wrapper.next_offset() != body.end {
        return Err(FramingError::structural(
            offset,
            "class wrapper has trailing bytes",
        ));
    }
    Ok((
        ClassDescriptor {
            class_uuid,
            class_data_range: data_chunk.body(),
        },
        userdata,
    ))
}

/// Parses one class-userdata chunk shared by object and render-settings wrappers.
pub(crate) fn parse_userdata(
    bytes: &[u8],
    wrapper: &crate::chunks::Chunk,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<UserdataDescriptor, FramingError> {
    let mut reader = BoundedReader::new(bytes, wrapper.body().start, wrapper.body().end)?;
    let packed = reader.u8()?;
    let version = (packed >> 4, packed & 0x0f);
    if version.0 == 1 {
        let class_uuid = uuid(&mut reader)?;
        let item_uuid = uuid(&mut reader)?;
        let copy_count = reader.i32()?;
        let transform_start = reader.position();
        reader.take(16 * 8)?;
        let transform_range = transform_start..reader.position();
        let payload = chunk_at(bytes, reader.position(), wrapper.body().end, archive, false)?;
        require_long(&payload, ANONYMOUS)?;
        if let Some(note) = checksum_warning_excluding(bytes, wrapper, &[payload.range()])? {
            warnings.push(note);
        }
        return Ok(UserdataDescriptor::Known(ClassUserdata {
            range: wrapper.range(),
            version,
            class_uuid,
            item_uuid,
            copy_count,
            transform_range,
            application_uuid: None,
            save_context: None,
            payload_range: payload.body(),
        }));
    }
    if version.0 != 2 {
        return Ok(UserdataDescriptor::UnknownVersion {
            range: wrapper.range(),
            version,
            payload_range: wrapper.body().clone(),
        });
    }
    let header = chunk_at(bytes, reader.position(), wrapper.body().end, archive, false)?;
    require_long(&header, CLASS_USERDATA_HEADER)?;
    if let Some(note) = checksum_warning(bytes, &header)? {
        warnings.push(note);
    }
    let mut header_reader = BoundedReader::new(bytes, header.body().start, header.body().end)?;
    let class_uuid = uuid(&mut header_reader)?;
    let item_uuid = uuid(&mut header_reader)?;
    let copy_count = header_reader.i32()?;
    let transform_start = header_reader.position();
    header_reader.take(16 * 8)?;
    let transform_range = transform_start..header_reader.position();
    let application_uuid = (version.1 >= 1)
        .then(|| uuid(&mut header_reader))
        .transpose()?;
    let save_context = if version.1 >= 2 {
        let value = header_reader.u8()?;
        if value > 1 {
            return Err(FramingError::structural(
                header_reader.position() - 1,
                "last-saved-as-goo must be encoded as 0 or 1",
            ));
        }
        Some(UserdataSaveContext {
            last_saved_as_goo: value != 0,
            archive_version: header_reader.i32()?,
            writer_version: header_reader.i32()?,
        })
    } else {
        None
    };
    header_reader.skip_remaining()?;
    let payload = chunk_at(
        bytes,
        header.next_offset(),
        wrapper.body().end,
        archive,
        false,
    )?;
    require_long(&payload, ANONYMOUS)?;
    reader.skip(payload.next_offset() - reader.position())?;
    reader.skip_remaining()?;
    if let Some(note) =
        checksum_warning_excluding(bytes, wrapper, &[header.range(), payload.range()])?
    {
        warnings.push(note);
    }
    Ok(UserdataDescriptor::Known(ClassUserdata {
        range: wrapper.range(),
        version,
        class_uuid,
        item_uuid,
        copy_count,
        transform_range,
        application_uuid,
        save_context,
        payload_range: payload.body(),
    }))
}

/// Reads the built-in `ON_UserStringList` payload from its outer userdata child.
pub(crate) fn parse_user_string_list(
    bytes: &[u8],
    payload_range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<Vec<(String, String)>, FramingError> {
    let list = chunk_at(
        bytes,
        payload_range.start,
        payload_range.end,
        archive,
        false,
    )?;
    require_long(&list, ANONYMOUS)?;
    let mut reader = BoundedReader::new(bytes, list.body().start, list.body().end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor < 0 {
        return Err(FramingError::structural(
            list.body().start,
            "user-string list version is unsupported",
        ));
    }
    let count = reader.i32()?;
    let count_bytes = bounded_count(&reader, count, 1)?;
    let mut values = Vec::with_capacity(count_bytes);
    for _ in 0..count_bytes {
        let entry = chunk_at(bytes, reader.position(), list.body().end, archive, false)?;
        require_long(&entry, ANONYMOUS)?;
        let mut entry_reader = BoundedReader::new(bytes, entry.body().start, entry.body().end)?;
        let entry_major = entry_reader.i32()?;
        let entry_minor = entry_reader.i32()?;
        if entry_major != 1 || entry_minor < 0 {
            return Err(FramingError::structural(
                entry.body().start,
                "user-string entry version is unsupported",
            ));
        }
        let key = settings::utf16(&mut entry_reader)?;
        let value = settings::utf16(&mut entry_reader)?;
        entry_reader.skip_remaining()?;
        values.push((key, value));
        reader.skip(entry.next_offset() - reader.position())?;
    }
    reader.skip_remaining()?;
    Ok(values)
}

fn parse_history(
    bytes: &[u8],
    wrapper: &crate::chunks::Chunk,
    archive: ArchiveVersion,
) -> Result<HistoryDescriptor, FramingError> {
    let mut reader = BoundedReader::new(bytes, wrapper.body().start, wrapper.body().end)?;
    let packed = reader.u8()?;
    let mut offset = reader.position();
    let mut header_range = None;
    let mut data_range = None;
    while offset < wrapper.body().end {
        let item = chunk_at(bytes, offset, wrapper.body().end, archive, false)?;
        match item.typecode {
            HISTORY_HEADER if header_range.is_none() && data_range.is_none() => {
                require_long(&item, HISTORY_HEADER)?;
                header_range = Some(item.range());
            }
            HISTORY_DATA if data_range.is_none() => {
                require_long(&item, HISTORY_DATA)?;
                data_range = Some(item.range());
            }
            _ => {
                return Err(FramingError::structural(
                    item.header_start,
                    "history child is duplicate or out of order",
                ));
            }
        }
        offset = item.next_offset();
    }
    Ok(HistoryDescriptor {
        range: wrapper.range(),
        version: (packed >> 4, packed & 0x0f),
        header_range,
        data_range,
    })
}

fn bounded_count(
    reader: &crate::chunks::BoundedReader<'_>,
    count: i32,
    width: usize,
) -> Result<usize, FramingError> {
    crate::chunks::checked_count_bytes(count, width, reader.remaining(), 1 << 16, reader.position())
}

fn finite_attribute(value: f64, offset: usize, label: &str) -> Result<f64, FramingError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(FramingError::structural(
            offset,
            format!("{label} is not finite"),
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AttributeItem {
    Name,
    Url,
    LinetypeIndex,
    MaterialIndex,
    RenderingAttributes,
    Color,
    PlotColor,
    PlotWeight,
    Decoration,
    WireDensity,
    Visible,
    ObjectMode,
    ColorSource,
    PlotColorSource,
    PlotWeightSource,
    MaterialSource,
    LinetypeSource,
    Groups,
    ActiveSpace,
    ViewportId,
    DisplayMaterials,
    DisplayOrder,
    LineCapSource,
    LineCapStyle,
    LineJoinSource,
    LineJoinStyle,
    ClipParticipationSource,
    Clipping,
    SectionAttributesSource,
    HatchPatternIndex,
    SectionHatchScale,
    SectionHatchRotation,
    LinetypePatternScale,
    HatchBackground,
    HatchBoundaryVisible,
    ObjectFrame,
    SectionFillRule,
    EmbeddedLinetype,
    EmbeddedSectionStyle,
    ClippingPlaneLabelStyle,
    SelectiveClippingList,
    DetailBackgroundVisible,
}

impl AttributeItem {
    fn from_raw(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Name,
            2 => Self::Url,
            3 => Self::LinetypeIndex,
            4 => Self::MaterialIndex,
            5 => Self::RenderingAttributes,
            6 => Self::Color,
            7 => Self::PlotColor,
            8 => Self::PlotWeight,
            9 => Self::Decoration,
            10 => Self::WireDensity,
            11 => Self::Visible,
            12 => Self::ObjectMode,
            13 => Self::ColorSource,
            14 => Self::PlotColorSource,
            15 => Self::PlotWeightSource,
            16 => Self::MaterialSource,
            17 => Self::LinetypeSource,
            18 => Self::Groups,
            19 => Self::ActiveSpace,
            20 => Self::ViewportId,
            21 => Self::DisplayMaterials,
            22 => Self::DisplayOrder,
            23 => Self::LineCapSource,
            24 => Self::LineCapStyle,
            25 => Self::LineJoinSource,
            26 => Self::LineJoinStyle,
            27 => Self::ClipParticipationSource,
            28 => Self::Clipping,
            29 => Self::SectionAttributesSource,
            30 => Self::HatchPatternIndex,
            31 => Self::SectionHatchScale,
            32 => Self::SectionHatchRotation,
            33 => Self::LinetypePatternScale,
            34 => Self::HatchBackground,
            35 => Self::HatchBoundaryVisible,
            36 => Self::ObjectFrame,
            37 => Self::SectionFillRule,
            38 => Self::EmbeddedLinetype,
            39 => Self::EmbeddedSectionStyle,
            40 => Self::ClippingPlaneLabelStyle,
            41 => Self::SelectiveClippingList,
            42 => Self::DetailBackgroundVisible,
            _ => return None,
        })
    }

    fn minimum_minor(self) -> u8 {
        match self {
            Self::Name
            | Self::Url
            | Self::LinetypeIndex
            | Self::MaterialIndex
            | Self::RenderingAttributes
            | Self::Color
            | Self::PlotColor
            | Self::PlotWeight
            | Self::Decoration
            | Self::WireDensity
            | Self::Visible
            | Self::ObjectMode
            | Self::ColorSource
            | Self::PlotColorSource
            | Self::PlotWeightSource
            | Self::MaterialSource
            | Self::LinetypeSource
            | Self::Groups
            | Self::ActiveSpace
            | Self::ViewportId
            | Self::DisplayMaterials => 0,
            Self::DisplayOrder => 1,
            Self::LineCapSource
            | Self::LineCapStyle
            | Self::LineJoinSource
            | Self::LineJoinStyle => 2,
            Self::ClipParticipationSource | Self::Clipping => 3,
            Self::SectionAttributesSource
            | Self::HatchPatternIndex
            | Self::SectionHatchScale
            | Self::SectionHatchRotation => 4,
            Self::LinetypePatternScale => 5,
            Self::HatchBackground | Self::HatchBoundaryVisible => 6,
            Self::ObjectFrame => 8,
            Self::SectionFillRule => 9,
            Self::EmbeddedLinetype => 10,
            Self::EmbeddedSectionStyle => 11,
            Self::ClippingPlaneLabelStyle => 12,
            Self::SelectiveClippingList | Self::DetailBackgroundVisible => 13,
        }
    }
}

pub(crate) fn parse_attributes(
    bytes: &[u8],
    body_range: Range<usize>,
    source_range: Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    warnings: &mut Vec<String>,
) -> Result<ObjectAttributes, FramingError> {
    let mut reader = crate::chunks::BoundedReader::new(bytes, body_range.start, body_range.end)?;
    let version = {
        let value = reader.u8()?;
        (value >> 4, value & 0x0f)
    };
    if version.0 == 1 {
        if (archive.value() >= 5
            && writer_version.is_some_and(|version| version >= LEGACY_OBJECT_ATTRIBUTES_CUTOFF))
            || version.1 > 8
        {
            return Err(FramingError::structural(
                body_range.start,
                "unsupported fixed object-attributes version",
            ));
        }
        let object_id = uuid_reader(&mut reader)?;
        let layer_index = reader.i32()?;
        let material_index = reader.i32()?;
        let color = reader.take(4)?.try_into().expect("color width checked");
        let obsolete_line_style = reader.i16()?;
        let obsolete_line_style_index = reader.i16()?;
        let obsolete_thickness = reader.f64()?;
        let obsolete_scale = reader.f64()?;
        let wire_density = reader.i32()?;
        let object_mode = reader.u8()?;
        let color_source = ColorSource::parse(reader.u8()?);
        let linetype_source = reader.u8()?;
        let material_source = reader.u8()?;
        let name = settings::utf16(&mut reader)?;
        let url = settings::utf16(&mut reader)?;
        let groups = if version.1 >= 1 {
            let count = reader.i32()?;
            let bytes = bounded_count(&reader, count, 4)?;
            let mut values = Vec::with_capacity(bytes / 4);
            for _ in 0..bytes / 4 {
                values.push(reader.i32()?);
            }
            values
        } else {
            Vec::new()
        };
        let visible = if version.1 >= 2 {
            reader.bool_with_writer_version(writer_version)?
        } else {
            object_mode & 0x0f != HIDDEN_OBJECT_MODE
        };
        let display_materials = if version.1 >= 3 {
            let count = reader.i32()?;
            let bytes = bounded_count(&reader, count, 32)?;
            let mut values = Vec::with_capacity(bytes / 32);
            for _ in 0..bytes / 32 {
                values.push((uuid_reader(&mut reader)?, uuid_reader(&mut reader)?));
            }
            values
        } else {
            Vec::new()
        };
        let (decoration, plot_color_source, plot_color, plot_weight_source, plot_weight) =
            if version.1 >= 4 {
                (
                    reader.i32()?,
                    reader.u8()?,
                    reader.take(4)?.try_into().expect("color width checked"),
                    reader.u8()?,
                    finite_attribute(reader.f64()?, reader.position(), "plot weight")?,
                )
            } else {
                (0, 0, [0; 4], 0, 0.0)
            };
        let linetype_index = if version.1 >= 5 { reader.i32()? } else { -1 };
        let (active_space, viewport_id, explicit_display_materials) = if version.1 >= 6 {
            let active_space = reader.u8()?;
            let count = reader.i32()?;
            let bytes = bounded_count(&reader, count, 32)?;
            let mut values = Vec::with_capacity(bytes / 32);
            for _ in 0..bytes / 32 {
                values.push((uuid_reader(&mut reader)?, uuid_reader(&mut reader)?));
            }
            (active_space, Uuid::nil(), values)
        } else {
            (0, Uuid::nil(), Vec::new())
        };
        let rendering_range = if version.1 >= 7 {
            Some(settings::parse_rendering_attributes(
                bytes,
                &mut reader,
                archive,
                settings::RenderingAttributesKind::Object,
                warnings,
            )?)
        } else {
            None
        };
        let obsolete_thickness =
            finite_attribute(obsolete_thickness, body_range.start, "obsolete thickness")?;
        let obsolete_scale = finite_attribute(obsolete_scale, body_range.start, "obsolete scale")?;
        reader.skip_remaining()?;
        return Ok(ObjectAttributes {
            source: SourceRange {
                range: source_range.clone(),
            },
            version,
            object_id,
            layer_index,
            material_index,
            color,
            obsolete_line_style,
            obsolete_line_style_index,
            obsolete_thickness,
            obsolete_scale,
            visible,
            color_source,
            linetype_source,
            material_source,
            plot_color_source,
            plot_weight_source,
            linetype_index,
            plot_color,
            plot_weight,
            object_mode,
            decoration,
            wire_density,
            name,
            url,
            rendering_range,
            groups,
            display_materials: if explicit_display_materials.is_empty() {
                display_materials
            } else {
                explicit_display_materials
            },
            active_space,
            viewport_id,
            display_order: 0,
            clip_participation_source: 0,
            clipping_proof: false,
            clipping_plane_ids: Vec::new(),
            section_attributes_source: 0,
            hatch_pattern_index: -1,
            section_hatch_scale: 1.0,
            section_hatch_rotation: 0.0,
            linetype_pattern_scale: 1.0,
            hatch_background: [0; 4],
            hatch_boundary_visible: false,
            detail_background_visible: false,
            object_frame: None,
            section_fill_rule: 0,
            line_cap_source: 0,
            line_cap_style: 0,
            line_join_source: 0,
            line_join_style: 0,
            clipping_plane_label_style: 0,
            selective_clipping_list: false,
            embedded_linetype: None,
            embedded_section_style: None,
            custom_render_mesh: None,
            mesh_modifiers: None,
        });
    }
    if version.0 != 2 || archive.value() < 50 {
        return Err(FramingError::structural(
            body_range.start,
            "unsupported tagged object-attributes version",
        ));
    }
    let object_id = uuid_reader(&mut reader)?;
    let layer_index = reader.i32()?;
    let mut attributes = ObjectAttributes {
        source: SourceRange {
            range: source_range,
        },
        version,
        object_id,
        layer_index,
        material_index: -1,
        color: [0; 4],
        obsolete_line_style: 0,
        obsolete_line_style_index: 0,
        obsolete_thickness: 0.0,
        obsolete_scale: 1.0,
        visible: true,
        color_source: ColorSource::Layer,
        linetype_source: 0,
        material_source: 0,
        plot_color_source: 0,
        plot_weight_source: 0,
        linetype_index: -1,
        plot_color: [0; 4],
        plot_weight: 0.0,
        object_mode: 0,
        decoration: 0,
        wire_density: 1,
        name: String::new(),
        url: String::new(),
        rendering_range: None,
        groups: Vec::new(),
        display_materials: Vec::new(),
        active_space: 0,
        viewport_id: Uuid::nil(),
        display_order: 0,
        clip_participation_source: 0,
        clipping_proof: false,
        clipping_plane_ids: Vec::new(),
        section_attributes_source: 0,
        hatch_pattern_index: -1,
        section_hatch_scale: 1.0,
        section_hatch_rotation: 0.0,
        linetype_pattern_scale: 1.0,
        hatch_background: [0; 4],
        hatch_boundary_visible: false,
        detail_background_visible: false,
        object_frame: None,
        section_fill_rule: 0,
        line_cap_source: 0,
        line_cap_style: 0,
        line_join_source: 0,
        line_join_style: 0,
        clipping_plane_label_style: 0,
        selective_clipping_list: false,
        embedded_linetype: None,
        embedded_section_style: None,
        custom_render_mesh: None,
        mesh_modifiers: None,
    };
    let mut last_item = None;
    while reader.remaining() > 0 {
        let item = reader.u8()?;
        if item == 0 {
            reader.skip_remaining()?;
            return Ok(attributes);
        }
        let Some(attribute_item) = AttributeItem::from_raw(item) else {
            // Item values have no length prefix. The source reader consumes
            // only an unknown ID and lets the enclosing chunk boundary
            // discard the value bytes it cannot type.
            reader.skip_remaining()?;
            return Ok(attributes);
        };
        if last_item.is_some_and(|last| attribute_item <= last)
            || version.1 < attribute_item.minimum_minor()
        {
            // This is the source reader's ordered cascade. The ID has been
            // consumed, but its value has no generic width, so the rest stays
            // at the containing attributes boundary.
            reader.skip_remaining()?;
            return Ok(attributes);
        }
        last_item = Some(attribute_item);
        match attribute_item {
            AttributeItem::Name => attributes.name = settings::utf16(&mut reader)?,
            AttributeItem::Url => attributes.url = settings::utf16(&mut reader)?,
            AttributeItem::LinetypeIndex => attributes.linetype_index = reader.i32()?,
            AttributeItem::MaterialIndex => attributes.material_index = reader.i32()?,
            AttributeItem::RenderingAttributes => {
                attributes.rendering_range = Some(settings::parse_rendering_attributes(
                    bytes,
                    &mut reader,
                    archive,
                    settings::RenderingAttributesKind::Object,
                    warnings,
                )?);
            }
            AttributeItem::Color => {
                attributes.color = reader.take(4)?.try_into().expect("color width checked");
            }
            AttributeItem::PlotColor => {
                attributes.plot_color = reader.take(4)?.try_into().expect("color width checked");
            }
            AttributeItem::PlotWeight => {
                attributes.plot_weight =
                    finite_attribute(reader.f64()?, reader.position(), "plot weight")?;
            }
            AttributeItem::Decoration => attributes.decoration = i32::from(reader.u8()?),
            AttributeItem::WireDensity => attributes.wire_density = reader.i32()?,
            AttributeItem::Visible => {
                attributes.visible = reader.bool_with_writer_version(writer_version)?;
            }
            AttributeItem::ObjectMode => attributes.object_mode = reader.u8()?,
            AttributeItem::ColorSource => {
                attributes.color_source = ColorSource::parse(reader.u8()?);
            }
            AttributeItem::PlotColorSource => attributes.plot_color_source = reader.u8()?,
            AttributeItem::PlotWeightSource => attributes.plot_weight_source = reader.u8()?,
            AttributeItem::MaterialSource => attributes.material_source = reader.u8()?,
            AttributeItem::LinetypeSource => attributes.linetype_source = reader.u8()?,
            AttributeItem::Groups => {
                let count = reader.i32()?;
                let bytes = bounded_count(&reader, count, 4)?;
                attributes.groups.clear();
                for _ in 0..bytes / 4 {
                    attributes.groups.push(reader.i32()?);
                }
            }
            AttributeItem::ActiveSpace => attributes.active_space = reader.u8()?,
            AttributeItem::ViewportId => attributes.viewport_id = uuid_reader(&mut reader)?,
            AttributeItem::DisplayMaterials => {
                let count = reader.i32()?;
                let bytes = bounded_count(&reader, count, 32)?;
                attributes.display_materials.clear();
                for _ in 0..bytes / 32 {
                    attributes
                        .display_materials
                        .push((uuid_reader(&mut reader)?, uuid_reader(&mut reader)?));
                }
            }
            AttributeItem::DisplayOrder => attributes.display_order = reader.i32()?,
            AttributeItem::LineCapSource => attributes.line_cap_source = reader.u8()?,
            AttributeItem::LineCapStyle => attributes.line_cap_style = reader.u8()?,
            AttributeItem::LineJoinSource => attributes.line_join_source = reader.u8()?,
            AttributeItem::LineJoinStyle => attributes.line_join_style = reader.u8()?,
            AttributeItem::ClipParticipationSource => {
                attributes.clip_participation_source = reader.u8()?;
            }
            AttributeItem::Clipping => {
                attributes.clipping_proof = reader.bool_with_writer_version(writer_version)?;
                attributes.clipping_plane_ids = read_uuid_list(&mut reader, archive)?;
            }
            AttributeItem::SectionAttributesSource => {
                attributes.section_attributes_source = reader.u8()?;
            }
            AttributeItem::HatchPatternIndex => attributes.hatch_pattern_index = reader.i32()?,
            AttributeItem::SectionHatchScale => {
                attributes.section_hatch_scale =
                    finite_attribute(reader.f64()?, reader.position(), "section hatch scale")?;
            }
            AttributeItem::SectionHatchRotation => {
                attributes.section_hatch_rotation =
                    finite_attribute(reader.f64()?, reader.position(), "section hatch rotation")?;
            }
            AttributeItem::LinetypePatternScale => {
                attributes.linetype_pattern_scale =
                    finite_attribute(reader.f64()?, reader.position(), "linetype scale")?;
            }
            AttributeItem::HatchBackground => {
                attributes.hatch_background =
                    reader.take(4)?.try_into().expect("color width checked");
            }
            AttributeItem::HatchBoundaryVisible => {
                attributes.hatch_boundary_visible =
                    reader.bool_with_writer_version(writer_version)?;
            }
            AttributeItem::DetailBackgroundVisible => {
                attributes.detail_background_visible =
                    reader.bool_with_writer_version(writer_version)?;
            }
            AttributeItem::ObjectFrame => {
                attributes.object_frame = Some(settings::xform(&mut reader)?);
            }
            AttributeItem::SectionFillRule => attributes.section_fill_rule = reader.u8()?,
            AttributeItem::EmbeddedLinetype => {
                attributes.embedded_linetype = Some(settings::parse_direct_linetype(
                    bytes,
                    &mut reader,
                    archive,
                    warnings,
                )?);
            }
            AttributeItem::EmbeddedSectionStyle => {
                attributes.embedded_section_style = Some(settings::parse_direct_section_style(
                    bytes,
                    &mut reader,
                    archive,
                    warnings,
                )?);
            }
            AttributeItem::ClippingPlaneLabelStyle => {
                attributes.clipping_plane_label_style = reader.u8()?;
            }
            AttributeItem::SelectiveClippingList => {
                attributes.selective_clipping_list =
                    reader.bool_with_writer_version(writer_version)?;
            }
        }
    }
    Err(FramingError::structural(
        reader.end(),
        "tagged object attributes are missing terminator",
    ))
}

pub(crate) fn read_uuid_list(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<Uuid>, FramingError> {
    let chunk = chunk_at(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
        false,
    )?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            reader.position(),
            "UUID list wrapper is invalid",
        ));
    }
    let mut payload =
        BoundedReader::new(reader.backing_bytes(), chunk.body().start, chunk.body().end)?;
    let version = (payload.i32()?, payload.i32()?);
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            payload.position(),
            "UUID list version is unsupported",
        ));
    }
    let count = payload.i32()?;
    let bytes = bounded_count(&payload, count, 16)?;
    let mut values = Vec::with_capacity(bytes / 16);
    for _ in 0..bytes / 16 {
        values.push(uuid_reader(&mut payload)?);
    }
    payload.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(values)
}

fn uuid_reader(reader: &mut crate::chunks::BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("UUID width checked"),
    ))
}

pub(crate) fn parse_attribute_userdata(
    bytes: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Vec<AttributeUserdataDescriptor> {
    let mut result = Vec::new();
    let mut offset = range.start;
    while offset < range.end {
        let item = match chunk_at(bytes, offset, range.end, archive, false) {
            Ok(item) => item,
            Err(error) => {
                warnings.push(format!("attribute userdata degraded at {offset}: {error}"));
                break;
            }
        };
        if item.typecode == CLASS_END {
            if let Err(error) = require_short_zero(&item, CLASS_END) {
                warnings.push(format!("attribute userdata end degraded: {error}"));
            }
            break;
        }
        if item.typecode != CLASS_USERDATA || item.short() {
            warnings.push(format!(
                "unknown attribute userdata chunk {:#x} at {}",
                item.typecode, item.header_start
            ));
            result.push(AttributeUserdataDescriptor::Unknown {
                range: item.range(),
            });
        } else {
            match parse_userdata(bytes, &item, archive, warnings) {
                Ok(UserdataDescriptor::Known(ClassUserdata {
                    range,
                    class_uuid,
                    item_uuid,
                    application_uuid,
                    save_context,
                    payload_range,
                    ..
                })) => result.push(AttributeUserdataDescriptor::Known(AttributeUserdata {
                    range,
                    class_uuid,
                    item_uuid,
                    application_uuid,
                    writer_version: save_context
                        .map(|value| i64::from(value.writer_version as u32)),
                    payload_range,
                })),
                Ok(UserdataDescriptor::UnknownVersion { range, .. }) => {
                    result.push(AttributeUserdataDescriptor::Unknown { range });
                }
                Err(error) => warnings.push(format!(
                    "attribute userdata at {} degraded: {error}",
                    item.header_start
                )),
            }
        }
        offset = item.next_offset();
    }
    result
}

/// Applies the recognized carriers owned by one object-attributes stream.
pub(crate) fn apply_attribute_userdata(
    bytes: &[u8],
    attributes: &mut ObjectAttributes,
    descriptors: &[AttributeUserdataDescriptor],
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) {
    let modern_custom_mesh = parse_per_object_mesh_userdata(bytes, descriptors, archive, warnings);
    let obsolete_custom_mesh =
        parse_obsolete_custom_mesh_userdata(bytes, descriptors, archive, warnings);
    attributes.custom_render_mesh = obsolete_custom_mesh.or(modern_custom_mesh);
    attributes.mesh_modifiers =
        crate::mesh_modifiers::parse_attribute_userdata(bytes, descriptors, archive, warnings);
}

fn parse_obsolete_custom_mesh_userdata(
    bytes: &[u8],
    descriptors: &[AttributeUserdataDescriptor],
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Option<settings::MeshParameters> {
    let descriptor = descriptors
        .iter()
        .filter_map(AttributeUserdataDescriptor::known)
        .find(|descriptor| {
            descriptor.class_uuid == OBSOLETE_CUSTOM_MESH_USERDATA
                && descriptor.item_uuid == OBSOLETE_CUSTOM_MESH_USERDATA
        })?;
    let payload_range = descriptor.payload_range.clone();
    let parsed = (|| {
        let mut reader = BoundedReader::new(bytes, payload_range.start, payload_range.end)?;
        let _legacy_value = reader.i32()?;
        let in_use = reader.bool_with_writer_version(descriptor.writer_version)?;
        let mut mesh = settings::parse_mesh_parameters(bytes, &mut reader, archive, true)?;
        reader.skip_remaining()?;

        // Read3dmObject converts this carrier into the modern per-object
        // userdata, whose setter forces these two logical fields.
        mesh.custom_settings_enabled = Some(in_use);
        mesh.custom_settings = Some(true);
        mesh.compute_curvature = false;
        Ok::<_, FramingError>(mesh)
    })();
    match parsed {
        Ok(mesh) => Some(mesh),
        Err(error) => {
            warnings.push(format!(
                "obsolete custom mesh userdata at {} dropped: {error}",
                descriptor.range.start
            ));
            None
        }
    }
}

fn parse_per_object_mesh_userdata(
    bytes: &[u8],
    descriptors: &[AttributeUserdataDescriptor],
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Option<settings::MeshParameters> {
    let descriptor = descriptors
        .iter()
        .filter_map(AttributeUserdataDescriptor::known)
        .find(|descriptor| {
            descriptor.class_uuid == PER_OBJECT_MESH_PARAMETERS_USERDATA
                && descriptor.item_uuid == PER_OBJECT_MESH_PARAMETERS_USERDATA
        })?;
    let payload_range = descriptor.payload_range.clone();
    let parsed = (|| {
        let outer = chunk_at(
            bytes,
            payload_range.start,
            payload_range.end,
            archive,
            false,
        )?;
        require_long(&outer, ANONYMOUS)?;
        let mut outer_reader = BoundedReader::new(bytes, outer.body().start, outer.body().end)?;
        let major = outer_reader.i32()?;
        let _minor = outer_reader.i32()?;
        if major != 1 {
            return Err(FramingError::structural(
                outer.body().start,
                "per-object mesh userdata version is unsupported",
            ));
        }
        let inner = chunk_at(
            bytes,
            outer_reader.position(),
            outer.body().end,
            archive,
            false,
        )?;
        require_long(&inner, ANONYMOUS)?;
        if inner.value() <= 0 || inner.body().is_empty() {
            return Err(FramingError::structural(
                inner.header_start,
                "per-object mesh userdata mesh child is empty",
            ));
        }
        let mut mesh_reader = BoundedReader::new(bytes, inner.body().start, inner.body().end)?;
        let mut mesh = settings::parse_mesh_parameters(bytes, &mut mesh_reader, archive, true)?;
        mesh_reader.skip_remaining()?;
        outer_reader.skip_remaining()?;

        // ON_PerObjectMeshParameters::Read applies these class invariants after
        // reading the nested mesh body.
        mesh.custom_settings = Some(true);
        mesh.compute_curvature = false;
        Ok::<_, FramingError>(mesh)
    })();
    match parsed {
        Ok(mesh) => Some(mesh),
        Err(error) => {
            warnings.push(format!(
                "per-object mesh userdata at {} dropped: {error}",
                descriptor.range.start
            ));
            None
        }
    }
}

fn resolve_identity(
    descriptor: &ObjectDescriptor<()>,
    layers: &HashMap<i32, &crate::settings::LayerRecord>,
    warnings: &mut Vec<String>,
    index: usize,
    seen_ids: &mut HashSet<Uuid>,
) -> SourceIdentity {
    let attributes = descriptor.attributes.parsed();
    let object_id = attributes.map_or(Uuid::nil(), |value| value.object_id);
    let layer_index = attributes.map_or(-1, |value| value.layer_index);
    let layer = layers.get(&layer_index).copied();
    if attributes.is_some() && layer.is_none() {
        warnings.push(format!(
            "object {object_id} references missing layer index {layer_index}"
        ));
    }
    let object_color = attributes.map(|value| value.color);
    let object_visible = attributes.is_none_or(|value| value.visible);
    let visible = object_visible && layer.is_none_or(|value| value.visible);
    let name = attributes.map_or_else(String::new, |value| value.name.clone());
    let object_mode = attributes.map_or(0, |value| value.object_mode);
    let definition_member = object_mode & 0x0f == IDEF_OBJECT_MODE;
    let color_selector = attributes.map_or(ColorSource::Layer, |value| value.color_source);
    let color = match color_selector {
        ColorSource::Layer => layer.map(|value| value.color),
        ColorSource::Object => object_color,
        ColorSource::Material => {
            warnings.push(format!(
                "object {object_id} material color remains unresolved"
            ));
            None
        }
        ColorSource::Parent if definition_member => {
            warnings.push(format!(
                "object {object_id} parent color remains unresolved"
            ));
            None
        }
        ColorSource::Parent => layer.map(|value| value.color),
        ColorSource::Invalid(raw) => {
            warnings.push(format!("object {object_id} has invalid color source {raw}"));
            None
        }
    };
    let source_key = if object_id.is_nil() {
        warnings.push(format!(
            "object at {} has nil object UUID",
            descriptor.range.start
        ));
        format!("record-{index:06}-offset-{}", descriptor.range.start)
    } else if !seen_ids.insert(object_id) {
        warnings.push(format!("duplicate object UUID {object_id}"));
        format!("record-{index:06}-offset-{}", descriptor.range.start)
    } else {
        object_id.to_string()
    };
    let source_id = stable_source_id("object", "record", &source_key);
    SourceIdentity {
        source_id,
        object_id,
        class_uuid: descriptor.class_uuid,
        name,
        layer_index,
        layer: layer.map(|value| LayerRef {
            id: value.id,
            name: value.name.clone(),
        }),
        effective_color: color,
        effective_visible: visible,
        object_mode,
        object_frame: attributes.and_then(|value| value.object_frame),
        source: SourceRange {
            range: descriptor.range.clone(),
        },
    }
}

/// Parses one bounded object record and returns identity plus child ranges.
pub(crate) fn parse_object_record(
    bytes: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    global_warnings: &mut Vec<String>,
) -> Result<ObjectRecord<()>, FramingError> {
    let mut warnings = Vec::new();
    if record.typecode != 0x2000_8070 || record.is_short() {
        return Err(FramingError::structural(
            record.range.start,
            "object record must be long-framed",
        ));
    }
    let mut offset = record.body().start;
    let type_chunk = chunk_at(bytes, offset, record.body().end, archive, false)?;
    if type_chunk.typecode != OBJECT_RECORD_TYPE || !type_chunk.short() {
        return Err(FramingError::structural(
            type_chunk.header_start,
            "object type must be the first short child",
        ));
    }
    let object_type = u32::try_from(type_chunk.value())
        .map_err(|_| FramingError::structural(type_chunk.header_start, "negative object type"))?;
    offset = type_chunk.next_offset();
    let class = chunk_at(bytes, offset, record.body().end, archive, false)?;
    require_long(&class, OPENNURBS_CLASS)?;
    offset = class.body().start;
    let uuid_chunk = chunk_at(bytes, offset, class.body().end, archive, true)?;
    require_long(&uuid_chunk, CLASS_UUID)?;
    if uuid_chunk.declared_end() - uuid_chunk.body().start != class_uuid_body::LEN {
        return Err(FramingError::structural(
            uuid_chunk.header_start,
            "class UUID chunk must have a 20-byte body",
        ));
    }
    if let Some(note) = checksum_warning(bytes, &uuid_chunk)? {
        warnings.push(note);
    }
    let class_uuid = Uuid::from_wire(
        bytes[uuid_chunk.body().clone()]
            .try_into()
            .expect("UUID length checked"),
    );
    offset = uuid_chunk.next_offset();
    let data_chunk = chunk_at(bytes, offset, class.body().end, archive, false)?;
    require_long(&data_chunk, CLASS_DATA)?;
    // CLASS_DATA is mixed by definition. Its concrete family reader owns
    // checksum validation because only that grammar identifies direct bytes.
    let class_data_range = data_chunk.body().clone();
    offset = data_chunk.next_offset();
    let mut userdata = Vec::new();
    let mut class_end_seen = false;
    while offset < class.body().end {
        let item = chunk_at(bytes, offset, class.body().end, archive, false)?;
        if item.typecode == CLASS_USERDATA {
            require_long(&item, CLASS_USERDATA)?;
            userdata.push(parse_userdata(bytes, &item, archive, &mut warnings)?);
            offset = item.next_offset();
        } else {
            require_short_zero(&item, CLASS_END)?;
            offset = item.next_offset();
            class_end_seen = true;
            break;
        }
    }
    if !class_end_seen || offset != class.body().end {
        return Err(FramingError::structural(
            class.body().end,
            "class wrapper has trailing bytes",
        ));
    }
    let mut attributes_chunk = None;
    let mut attributes_userdata_body_range = None;
    let mut history = None;
    let mut unknown_trailer = Vec::new();
    let mut phase = 0_u8;
    let mut object_end_seen = false;
    while offset < record.body().end {
        let item = chunk_at(bytes, offset, record.body().end, archive, false)?;
        if item.typecode == OBJECT_RECORD_END {
            require_short_zero(&item, OBJECT_RECORD_END)?;
            if item.next_offset() != record.body().end {
                return Err(FramingError::structural(
                    item.header_start,
                    "object end is not final",
                ));
            }
            offset = item.next_offset();
            object_end_seen = true;
            break;
        }
        match item.typecode {
            OBJECT_RECORD_ATTRIBUTES if phase == 0 => {
                require_long(&item, OBJECT_RECORD_ATTRIBUTES)?;
                attributes_chunk = Some(item.clone());
                phase = 1;
            }
            OBJECT_RECORD_ATTRIBUTES_USERDATA if phase <= 1 => {
                require_long(&item, OBJECT_RECORD_ATTRIBUTES_USERDATA)?;
                attributes_userdata_body_range = Some(item.body().clone());
                phase = 2;
            }
            OBJECT_RECORD_HISTORY if phase <= 2 => {
                require_long(&item, OBJECT_RECORD_HISTORY)?;
                let descriptor = parse_history(bytes, &item, archive)?;
                let children = descriptor
                    .header_range
                    .iter()
                    .chain(descriptor.data_range.iter())
                    .cloned()
                    .collect::<Vec<_>>();
                if let Some(note) = checksum_warning_excluding(bytes, &item, &children)? {
                    warnings.push(note);
                }
                history = Some(descriptor);
                phase = 3;
            }
            _ if !item.short() => {
                unknown_trailer.push(item.range());
                phase = 3;
            }
            _ => {
                return Err(FramingError::structural(
                    item.header_start,
                    "object trailer child is out of order or malformed",
                ));
            }
        }
        offset = item.next_offset();
    }
    if !object_end_seen || offset != record.body().end {
        return Err(FramingError::structural(
            record.body().end,
            "object record is missing object end",
        ));
    }
    let mut attributes = attributes_chunk
        .as_ref()
        .map_or(AttributeState::Missing, |chunk| {
            match parse_attributes(
                bytes,
                chunk.body(),
                chunk.range(),
                archive,
                writer_version,
                &mut warnings,
            ) {
                Ok(value) => AttributeState::Parsed(Box::new(value)),
                Err(error) => {
                    warnings.push(format!(
                        "object attributes at {} degraded: {error}",
                        chunk.body().start
                    ));
                    AttributeState::Degraded
                }
            }
        });
    if let Some(item) = attributes_chunk.as_ref() {
        let children = attributes
            .parsed()
            .and_then(|value| value.rendering_range.clone())
            .into_iter()
            .collect::<Vec<_>>();
        if let Some(note) = checksum_warning_excluding(bytes, item, &children)? {
            warnings.push(note);
        }
    }
    let attributes_userdata = attributes_userdata_body_range
        .as_ref()
        .map(|range| parse_attribute_userdata(bytes, range.clone(), archive, &mut warnings))
        .unwrap_or_default();
    if let AttributeState::Parsed(attributes) = &mut attributes {
        apply_attribute_userdata(
            bytes,
            attributes,
            &attributes_userdata,
            archive,
            &mut warnings,
        );
    }
    Ok(ObjectRecord::Framed(ObjectDescriptor {
        range: record.range.clone(),
        object_type,
        class_uuid,
        class_data_range,
        attributes,
        attributes_userdata,
        identity: (),
        userdata,
        history,
        unknown_trailer,
        checksum_warnings: {
            global_warnings.extend(warnings.iter().cloned());
            warnings
        },
        warnings: Vec::new(),
    }))
}

/// Builds a range-preserving descriptor for a malformed bounded object record.
pub(crate) fn degraded_object_record(record: &Record, error: &FramingError) -> ObjectRecord<()> {
    ObjectRecord::Degraded {
        range: record.range.clone(),
        warning: format!(
            "bounded object record at {} degraded: {error}",
            record.range.start
        ),
    }
}

/// Resolves per-object source identity after document layer metadata is known.
pub(crate) fn resolve_identities(
    objects: Vec<ObjectRecord<()>>,
    metadata: &DocumentMetadata,
    warnings: &mut Vec<String>,
) -> Vec<ObjectRecord> {
    let mut seen_ids = HashSet::new();
    let mut layers = HashMap::with_capacity(metadata.layers.len());
    for layer in &metadata.layers {
        layers.entry(layer.index).or_insert(layer);
    }
    objects
        .into_iter()
        .enumerate()
        .map(|(index, object)| match object {
            ObjectRecord::Degraded { range, warning } => ObjectRecord::Degraded { range, warning },
            ObjectRecord::Framed(mut object) => {
                let mut local_warnings = Vec::new();
                let identity =
                    resolve_identity(&object, &layers, &mut local_warnings, index, &mut seen_ids);
                warnings.extend(local_warnings.iter().cloned());
                object.warnings.extend(local_warnings);
                ObjectRecord::Framed(ObjectDescriptor {
                    identity,
                    range: object.range,
                    object_type: object.object_type,
                    class_uuid: object.class_uuid,
                    class_data_range: object.class_data_range,
                    attributes: object.attributes,
                    attributes_userdata: object.attributes_userdata,
                    userdata: object.userdata,
                    history: object.history,
                    unknown_trailer: object.unknown_trailer,
                    checksum_warnings: object.checksum_warnings,
                    warnings: object.warnings,
                })
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests;
