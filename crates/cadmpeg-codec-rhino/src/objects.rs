// SPDX-License-Identifier: Apache-2.0
//! Rhino object-record identity and framing.

use std::collections::HashSet;
use std::fmt;
use std::ops::Range;

use crate::chunks::{chunk_at, verify_checksum, ArchiveVersion, ChecksumStatus, FramingError};
use crate::container::Record;
use crate::settings::{self, DocumentMetadata, SourceRange, Xform};

const OBJECT_RECORD_TYPE: u32 = 0x82a0_0071;
const OBJECT_RECORD_ATTRIBUTES: u32 = 0x0200_8072;
const OBJECT_RECORD_ATTRIBUTES_USERDATA: u32 = 0x0200_0073;
const OBJECT_RECORD_HISTORY: u32 = 0x0200_8074;
const OBJECT_RECORD_END: u32 = 0x82a0_007f;
const OPENNURBS_CLASS: u32 = 0x0002_7ffa;
const CLASS_USERDATA: u32 = 0x0002_7ffd;
const CLASS_USERDATA_HEADER: u32 = 0x0002_fff9;
const CLASS_UUID: u32 = 0x0002_fffb;
const CLASS_DATA: u32 = 0x0002_fffc;
const CLASS_END: u32 = 0x8202_7fff;
const ANONYMOUS: u32 = 0x4000_8000;
const HISTORY_HEADER: u32 = 0x0200_8075;
const HISTORY_DATA: u32 = 0x0200_8076;
const HIDDEN_OBJECT_MODE: u8 = 2;
const IDEF_OBJECT_MODE: u8 = 3;

/// A UUID in canonical textual byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Uuid {
    bytes: [u8; 16],
}

impl Uuid {
    /// Parses the mixed-endian UUID wire representation.
    pub(crate) fn from_wire(bytes: [u8; 16]) -> Self {
        let mut canonical = [0; 16];
        for index in 0..4 {
            canonical[index] = bytes[3 - index];
        }
        for index in 0..2 {
            canonical[4 + index] = bytes[5 - index];
            canonical[6 + index] = bytes[7 - index];
        }
        canonical[8..].copy_from_slice(&bytes[8..]);
        Self { bytes: canonical }
    }

    /// Returns the nil UUID.
    pub(crate) fn nil() -> Self {
        Self { bytes: [0; 16] }
    }

    /// Returns whether this UUID is nil.
    pub(crate) fn is_nil(self) -> bool {
        self == Self::nil()
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.bytes.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A class-userdata descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserdataDescriptor {
    /// Complete wrapper range.
    pub(crate) range: Range<usize>,
    /// Packed wrapper version.
    pub(crate) version: (u8, u8),
    /// Userdata class UUID.
    pub(crate) class_uuid: Uuid,
    /// Userdata item UUID.
    pub(crate) item_uuid: Uuid,
    /// Copy count.
    pub(crate) copy_count: i32,
    /// Transform byte range.
    pub(crate) transform_range: Range<usize>,
    /// Optional application UUID.
    pub(crate) application_uuid: Option<Uuid>,
    /// Optional last-saved-as-goo flag.
    pub(crate) last_saved_as_goo: Option<bool>,
    /// Optional userdata archive version.
    pub(crate) archive_version: Option<i32>,
    /// Optional userdata writer version.
    pub(crate) writer_version: Option<i32>,
    /// Anonymous payload range, excluding its framing.
    pub(crate) payload_range: Range<usize>,
    /// Unknown future-version payload range.
    pub(crate) unknown_version: bool,
}

/// An attribute-userdata record, retained independently of object attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttributeUserdataDescriptor {
    /// Complete userdata chunk range.
    pub(crate) range: Range<usize>,
    /// Whether the class-userdata framing was recognized.
    pub(crate) known: bool,
    /// Userdata class UUID when the framing supplied one.
    pub(crate) class_uuid: Option<Uuid>,
    /// Userdata item UUID when the framing supplied one.
    pub(crate) item_uuid: Option<Uuid>,
    /// Bounded anonymous payload range.
    pub(crate) payload_range: Option<Range<usize>>,
}

/// Raw object attributes decoded from an object-attributes chunk.
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
    /// Raw color source selector.
    pub(crate) color_source: u8,
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
    /// Resolved layer UUID.
    pub(crate) layer_id: Option<Uuid>,
    /// Resolved layer name.
    pub(crate) layer_name: Option<String>,
    /// Effective display color.
    pub(crate) effective_color: Option<[u8; 4]>,
    /// Effective visibility after layer combination.
    pub(crate) effective_visible: bool,
    /// Raw object mode.
    pub(crate) object_mode: u8,
    /// Whether the object-mode marks a definition member.
    pub(crate) definition_member: bool,
    /// Object frame transform.
    pub(crate) object_frame: Option<Xform>,
    /// Complete source range.
    pub(crate) source: SourceRange,
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

/// A fully framed Rhino object record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjectDescriptor {
    /// Complete object-record range.
    pub(crate) range: Range<usize>,
    /// Object type filter bits.
    pub(crate) object_type: u32,
    /// Class UUID.
    pub(crate) class_uuid: Uuid,
    /// Class-data payload range.
    pub(crate) class_data_range: Range<usize>,
    /// Parsed object attributes, if valid.
    pub(crate) attributes: Option<ObjectAttributes>,
    /// Whether the framed attributes payload degraded during parsing.
    pub(crate) attributes_degraded: bool,
    /// Attribute-userdata descriptors.
    pub(crate) attributes_userdata: Vec<AttributeUserdataDescriptor>,
    /// Resolved source identity.
    pub(crate) identity: Option<SourceIdentity>,
    /// Class userdata descriptors.
    pub(crate) userdata: Vec<UserdataDescriptor>,
    /// Optional attributes range.
    pub(crate) attributes_range: Option<Range<usize>>,
    /// Optional attributes body range, excluding framing and checksum.
    pub(crate) attributes_body_range: Option<Range<usize>>,
    /// Optional attribute-userdata range.
    pub(crate) attributes_userdata_range: Option<Range<usize>>,
    /// Optional attribute-userdata body range, excluding framing.
    pub(crate) attributes_userdata_body_range: Option<Range<usize>>,
    /// Optional history descriptor.
    pub(crate) history: Option<HistoryDescriptor>,
    /// Unknown bounded trailer child ranges.
    pub(crate) unknown_trailer: Vec<Range<usize>>,
    /// Checksum warning messages.
    pub(crate) checksum_warnings: Vec<String>,
    /// Object-local attribute and identity warnings.
    pub(crate) warnings: Vec<String>,
}

/// A fully framed `OpenNURBS` class wrapper used by table records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassDescriptor {
    /// Class UUID.
    pub(crate) class_uuid: Uuid,
    /// Class-data payload range, excluding its chunk framing.
    pub(crate) class_data_range: Range<usize>,
}

struct Bytes<'a> {
    data: &'a [u8],
    position: usize,
    end: usize,
}

impl<'a> Bytes<'a> {
    fn new(bytes: &'a [u8], range: Range<usize>) -> Self {
        Self {
            data: bytes,
            position: range.start,
            end: range.end,
        }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], FramingError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(FramingError::Overflow {
                offset: self.position,
            })?;
        if end > self.end {
            return Err(FramingError::OutOfBounds {
                offset: self.position,
                end,
                bound: self.end,
            });
        }
        let result = &self.data[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn u8(&mut self) -> Result<u8, FramingError> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32, FramingError> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("length checked"),
        ))
    }

    fn uuid(&mut self) -> Result<Uuid, FramingError> {
        Ok(Uuid::from_wire(
            self.take(16)?.try_into().expect("length checked"),
        ))
    }
}

fn malformed_at(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn child(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
    class_uuid: bool,
) -> Result<crate::chunks::Chunk, FramingError> {
    chunk_at(bytes, offset, end, archive, class_uuid)
}

fn require_long(chunk: &crate::chunks::Chunk, typecode: u32) -> Result<(), FramingError> {
    if chunk.typecode != typecode || chunk.short {
        return Err(malformed_at(
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
    if chunk.typecode != typecode || !chunk.short || chunk.value != 0 {
        return Err(malformed_at(
            chunk.header_start,
            format!(
                "expected short zero chunk {typecode:#x}, got {:#x}",
                chunk.typecode
            ),
        ));
    }
    Ok(())
}

fn chunk_range(chunk: &crate::chunks::Chunk) -> Range<usize> {
    chunk.range()
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

/// Parses a table-record `OpenNURBS` class wrapper without decoding its payload.
pub(crate) fn parse_class_wrapper(
    bytes: &[u8],
    body: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<ClassDescriptor, FramingError> {
    let wrapper = child(bytes, body.start, body.end, archive, false)?;
    require_long(&wrapper, OPENNURBS_CLASS)?;
    let uuid_chunk = child(bytes, wrapper.body.start, wrapper.body.end, archive, true)?;
    require_long(&uuid_chunk, CLASS_UUID)?;
    if uuid_chunk.declared_end - uuid_chunk.body_start != 20 {
        return Err(malformed_at(
            uuid_chunk.header_start,
            "class UUID chunk must have a 20-byte body",
        ));
    }
    if let Some(note) = checksum_warning(bytes, &uuid_chunk)? {
        warnings.push(note);
    }
    let class_uuid = Uuid::from_wire(
        bytes[uuid_chunk.body.start..uuid_chunk.body.start + 16]
            .try_into()
            .expect("UUID length checked"),
    );
    let data_chunk = child(
        bytes,
        uuid_chunk.next_offset,
        wrapper.body.end,
        archive,
        false,
    )?;
    require_long(&data_chunk, CLASS_DATA)?;
    if let Some(note) = checksum_warning(bytes, &data_chunk)? {
        warnings.push(note);
    }
    let end_chunk = child(
        bytes,
        data_chunk.next_offset,
        wrapper.body.end,
        archive,
        false,
    )?;
    require_short_zero(&end_chunk, CLASS_END)?;
    if end_chunk.next_offset != wrapper.body.end || wrapper.next_offset != body.end {
        return Err(malformed_at(
            end_chunk.header_start,
            "class wrapper has trailing bytes",
        ));
    }
    Ok(ClassDescriptor {
        class_uuid,
        class_data_range: data_chunk.body,
    })
}

fn parse_userdata(
    bytes: &[u8],
    wrapper: &crate::chunks::Chunk,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<UserdataDescriptor, FramingError> {
    let mut reader = Bytes::new(bytes, wrapper.body.clone());
    let packed = reader.u8()?;
    let version = (packed >> 4, packed & 0x0f);
    if version.0 == 1 {
        let class_uuid = reader.uuid()?;
        let item_uuid = reader.uuid()?;
        let copy_count = reader.i32()?;
        let transform_start = reader.position;
        reader.take(16 * 8)?;
        let transform_range = transform_start..reader.position;
        let payload = child(bytes, reader.position, wrapper.body.end, archive, false)?;
        require_long(&payload, ANONYMOUS)?;
        if let Some(note) = checksum_warning(bytes, &payload)? {
            warnings.push(note);
        }
        if payload.next_offset != wrapper.body.end {
            return Err(malformed_at(
                wrapper.body.end,
                "userdata wrapper has trailing bytes",
            ));
        }
        return Ok(UserdataDescriptor {
            range: chunk_range(wrapper),
            version,
            class_uuid,
            item_uuid,
            copy_count,
            transform_range,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: payload.body,
            unknown_version: false,
        });
    }
    if version.0 != 2 {
        if let Some(note) = checksum_warning(bytes, wrapper)? {
            warnings.push(note);
        }
        return Ok(UserdataDescriptor {
            range: chunk_range(wrapper),
            version,
            class_uuid: Uuid::nil(),
            item_uuid: Uuid::nil(),
            copy_count: 0,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: wrapper.body.clone(),
            unknown_version: true,
        });
    }
    let header = child(bytes, reader.position, wrapper.body.end, archive, false)?;
    require_long(&header, CLASS_USERDATA_HEADER)?;
    if let Some(note) = checksum_warning(bytes, &header)? {
        warnings.push(note);
    }
    let mut header_reader = Bytes::new(bytes, header.body.clone());
    let class_uuid = header_reader.uuid()?;
    let item_uuid = header_reader.uuid()?;
    let copy_count = header_reader.i32()?;
    let transform_start = header_reader.position;
    header_reader.take(16 * 8)?;
    let transform_range = transform_start..header_reader.position;
    let application_uuid = (version.1 >= 1).then(|| header_reader.uuid()).transpose()?;
    let last_saved_as_goo = if version.1 >= 2 {
        let value = header_reader.u8()?;
        if value > 1 {
            return Err(malformed_at(
                header_reader.position - 1,
                "last-saved-as-goo must be encoded as 0 or 1",
            ));
        }
        Some(value != 0)
    } else {
        None
    };
    let archive_version = (version.1 >= 2).then(|| header_reader.i32()).transpose()?;
    let writer_version = (version.1 >= 2).then(|| header_reader.i32()).transpose()?;
    if header_reader.position != header.body.end {
        return Err(malformed_at(
            header_reader.position,
            "userdata header has trailing bytes",
        ));
    }
    let payload = child(bytes, header.next_offset, wrapper.body.end, archive, false)?;
    require_long(&payload, ANONYMOUS)?;
    if let Some(note) = checksum_warning(bytes, &payload)? {
        warnings.push(note);
    }
    if payload.next_offset != wrapper.body.end {
        return Err(malformed_at(
            payload.next_offset,
            "userdata wrapper has trailing bytes",
        ));
    }
    Ok(UserdataDescriptor {
        range: chunk_range(wrapper),
        version,
        class_uuid,
        item_uuid,
        copy_count,
        transform_range,
        application_uuid,
        last_saved_as_goo,
        archive_version,
        writer_version,
        payload_range: payload.body,
        unknown_version: false,
    })
}

fn parse_history(
    bytes: &[u8],
    wrapper: &crate::chunks::Chunk,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<HistoryDescriptor, FramingError> {
    let mut reader = Bytes::new(bytes, wrapper.body.clone());
    let packed = reader.u8()?;
    let mut offset = reader.position;
    let mut header_range = None;
    let mut data_range = None;
    while offset < wrapper.body.end {
        let item = child(bytes, offset, wrapper.body.end, archive, false)?;
        match item.typecode {
            HISTORY_HEADER if header_range.is_none() && data_range.is_none() => {
                require_long(&item, HISTORY_HEADER)?;
                if let Some(note) = checksum_warning(bytes, &item)? {
                    warnings.push(note);
                }
                header_range = Some(chunk_range(&item));
            }
            HISTORY_DATA if data_range.is_none() => {
                require_long(&item, HISTORY_DATA)?;
                if let Some(note) = checksum_warning(bytes, &item)? {
                    warnings.push(note);
                }
                data_range = Some(chunk_range(&item));
            }
            _ => {
                return Err(malformed_at(
                    item.header_start,
                    "history child is duplicate or out of order",
                ))
            }
        }
        offset = item.next_offset;
    }
    Ok(HistoryDescriptor {
        range: chunk_range(wrapper),
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
        Err(malformed_at(offset, format!("{label} is not finite")))
    }
}

fn validate_attribute_selectors(
    object_mode: u8,
    selectors: [u8; 5],
    offset: usize,
) -> Result<(), FramingError> {
    if object_mode & 0x0f > 5 {
        return Err(malformed_at(offset, "unknown object-mode discriminant"));
    }
    if selectors.iter().any(|selector| *selector > 3) {
        return Err(malformed_at(offset, "unknown attribute source selector"));
    }
    Ok(())
}

pub(crate) fn parse_attributes(
    bytes: &[u8],
    body_range: Range<usize>,
    source_range: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<ObjectAttributes, FramingError> {
    let mut reader = crate::chunks::BoundedReader::new(bytes, body_range.start, body_range.end)?;
    let version = {
        let value = reader.u8()?;
        (value >> 4, value & 0x0f)
    };
    if version.0 == 1 {
        if archive.value() >= 50 || version.1 > 8 {
            return Err(malformed_at(
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
        let color_source = reader.u8()?;
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
            reader.bool()?
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
                warnings,
            )?)
        } else {
            None
        };
        validate_attribute_selectors(
            object_mode,
            [
                color_source,
                linetype_source,
                material_source,
                plot_color_source,
                plot_weight_source,
            ],
            body_range.start,
        )?;
        let obsolete_thickness =
            finite_attribute(obsolete_thickness, body_range.start, "obsolete thickness")?;
        let obsolete_scale = finite_attribute(obsolete_scale, body_range.start, "obsolete scale")?;
        finish_attributes(&reader, "fixed object attributes")?;
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
        });
    }
    if version.0 != 2 || archive.value() < 50 || version.1 > 13 {
        return Err(malformed_at(
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
        color_source: 0,
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
    };
    while reader.remaining() > 0 {
        let item = reader.u8()?;
        if item == 0 {
            finish_attributes(&reader, "tagged object attributes")?;
            return Ok(attributes);
        }
        let gate = match item {
            1..=21 => 0,
            22 => 1,
            23..=26 => 2,
            27..=28 => 3,
            29..=32 => 4,
            33 => 5,
            34..=35 => 6,
            36 => 8,
            37 => 9,
            38 => 10,
            39 => 11,
            40 => 12,
            41 => 13,
            _ => {
                return Err(malformed_at(
                    reader.position() - 1,
                    format!("unknown future object-attributes item {item}"),
                ))
            }
        };
        if version.1 < gate {
            return Err(malformed_at(
                reader.position() - 1,
                format!("attribute item {item} precedes its version gate"),
            ));
        }
        match item {
            1 => attributes.name = settings::utf16(&mut reader)?,
            2 => attributes.url = settings::utf16(&mut reader)?,
            3 => attributes.linetype_index = reader.i32()?,
            4 => attributes.material_index = reader.i32()?,
            5 => {
                attributes.rendering_range = Some(settings::parse_rendering_attributes(
                    bytes,
                    &mut reader,
                    archive,
                    warnings,
                )?);
            }
            6 => attributes.color = reader.take(4)?.try_into().expect("color width checked"),
            7 => attributes.plot_color = reader.take(4)?.try_into().expect("color width checked"),
            8 => {
                attributes.plot_weight =
                    finite_attribute(reader.f64()?, reader.position(), "plot weight")?;
            }
            9 => attributes.decoration = i32::from(reader.u8()?),
            10 => attributes.wire_density = reader.i32()?,
            11 => attributes.visible = reader.bool()?,
            12 => attributes.object_mode = reader.u8()?,
            13 => attributes.color_source = reader.u8()?,
            14 => attributes.plot_color_source = reader.u8()?,
            15 => attributes.plot_weight_source = reader.u8()?,
            16 => attributes.material_source = reader.u8()?,
            17 => attributes.linetype_source = reader.u8()?,
            18 => {
                let count = reader.i32()?;
                let bytes = bounded_count(&reader, count, 4)?;
                attributes.groups.clear();
                for _ in 0..bytes / 4 {
                    attributes.groups.push(reader.i32()?);
                }
            }
            19 => attributes.active_space = reader.u8()?,
            20 => attributes.viewport_id = uuid_reader(&mut reader)?,
            21 => {
                let count = reader.i32()?;
                let bytes = bounded_count(&reader, count, 32)?;
                attributes.display_materials.clear();
                for _ in 0..bytes / 32 {
                    attributes
                        .display_materials
                        .push((uuid_reader(&mut reader)?, uuid_reader(&mut reader)?));
                }
            }
            22 => attributes.display_order = reader.i32()?,
            23 => attributes.line_cap_source = reader.u8()?,
            24 => attributes.line_cap_style = reader.u8()?,
            25 => attributes.line_join_source = reader.u8()?,
            26 => attributes.line_join_style = reader.u8()?,
            27 => attributes.clip_participation_source = reader.u8()?,
            28 => {
                attributes.clipping_proof = reader.bool()?;
                let count = reader.i32()?;
                let bytes = bounded_count(&reader, count, 16)?;
                attributes.clipping_plane_ids.clear();
                for _ in 0..bytes / 16 {
                    attributes
                        .clipping_plane_ids
                        .push(uuid_reader(&mut reader)?);
                }
            }
            29 => attributes.section_attributes_source = reader.u8()?,
            30 => attributes.hatch_pattern_index = reader.i32()?,
            31 => {
                attributes.section_hatch_scale =
                    finite_attribute(reader.f64()?, reader.position(), "section hatch scale")?;
            }
            32 => {
                attributes.section_hatch_rotation =
                    finite_attribute(reader.f64()?, reader.position(), "section hatch rotation")?;
            }
            33 => {
                attributes.linetype_pattern_scale =
                    finite_attribute(reader.f64()?, reader.position(), "linetype scale")?;
            }
            34 => {
                attributes.hatch_background =
                    reader.take(4)?.try_into().expect("color width checked");
            }
            35 => attributes.hatch_boundary_visible = reader.bool()?,
            36 => attributes.object_frame = Some(settings::xform(&mut reader)?),
            37 => attributes.section_fill_rule = reader.u8()?,
            38 => {
                attributes.embedded_linetype = Some(settings::parse_direct_linetype(
                    bytes,
                    &mut reader,
                    archive,
                    warnings,
                )?);
            }
            39 => {
                attributes.embedded_section_style = Some(settings::parse_direct_section_style(
                    bytes,
                    &mut reader,
                    archive,
                    warnings,
                )?);
            }
            40 => attributes.clipping_plane_label_style = reader.u8()?,
            41 => attributes.selective_clipping_list = reader.bool()?,
            _ => unreachable!(),
        }
        validate_attribute_selectors(
            attributes.object_mode,
            [
                attributes.color_source,
                attributes.linetype_source,
                attributes.material_source,
                attributes.plot_color_source,
                attributes.plot_weight_source,
            ],
            reader.position(),
        )?;
    }
    Err(malformed_at(
        reader.end(),
        "tagged object attributes are missing terminator",
    ))
}

fn uuid_reader(reader: &mut crate::chunks::BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("UUID width checked"),
    ))
}

fn finish_attributes(
    reader: &crate::chunks::BoundedReader<'_>,
    label: &str,
) -> Result<(), FramingError> {
    if reader.remaining() != 0 {
        return Err(malformed_at(
            reader.position(),
            format!("{label} has trailing bytes"),
        ));
    }
    Ok(())
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
        let item = match child(bytes, offset, range.end, archive, false) {
            Ok(item) => item,
            Err(error) => {
                warnings.push(format!("attribute userdata degraded at {offset}: {error}"));
                break;
            }
        };
        if item.typecode != CLASS_USERDATA || item.short {
            warnings.push(format!(
                "unknown attribute userdata chunk {:#x} at {}",
                item.typecode, item.header_start
            ));
            result.push(AttributeUserdataDescriptor {
                range: item.range(),
                known: false,
                class_uuid: None,
                item_uuid: None,
                payload_range: None,
            });
        } else {
            match parse_userdata(bytes, &item, archive, warnings) {
                Ok(value) => result.push(AttributeUserdataDescriptor {
                    range: value.range,
                    known: !value.unknown_version,
                    class_uuid: (!value.unknown_version).then_some(value.class_uuid),
                    item_uuid: (!value.unknown_version).then_some(value.item_uuid),
                    payload_range: (!value.unknown_version).then_some(value.payload_range),
                }),
                Err(error) => warnings.push(format!(
                    "attribute userdata at {} degraded: {error}",
                    item.header_start
                )),
            }
        }
        offset = item.next_offset;
    }
    result
}

fn resolve_identity(
    descriptor: &mut ObjectDescriptor,
    metadata: &DocumentMetadata,
    warnings: &mut Vec<String>,
    index: usize,
    seen_ids: &mut HashSet<Uuid>,
) {
    let attributes = descriptor.attributes.as_ref();
    let object_id = attributes.map_or(Uuid::nil(), |value| value.object_id);
    let layer_index = attributes.map_or(-1, |value| value.layer_index);
    let layer = metadata
        .layers
        .iter()
        .find(|value| value.index == layer_index);
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
    let color_selector = attributes.map_or(0, |value| value.color_source);
    let color = match color_selector {
        0 => layer.map(|value| value.color),
        1 => object_color,
        2 => {
            warnings.push(format!(
                "object {object_id} material color remains unresolved"
            ));
            None
        }
        3 if definition_member => {
            warnings.push(format!(
                "object {object_id} parent color remains unresolved"
            ));
            None
        }
        3 => layer.map(|value| value.color),
        _ => {
            warnings.push(format!(
                "object {object_id} has invalid color source {color_selector}"
            ));
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
    descriptor.identity = Some(SourceIdentity {
        source_id,
        object_id,
        class_uuid: descriptor.class_uuid,
        name,
        layer_index,
        layer_id: layer.and_then(|value| value.id),
        layer_name: layer.map(|value| value.name.clone()),
        effective_color: color,
        effective_visible: visible,
        object_mode,
        definition_member,
        object_frame: attributes.and_then(|value| value.object_frame),
        source: SourceRange {
            range: descriptor.range.clone(),
        },
    });
}

/// Parses one bounded object record and returns identity plus child ranges.
pub(crate) fn parse_object_record(
    bytes: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    global_warnings: &mut Vec<String>,
) -> Result<ObjectDescriptor, FramingError> {
    let mut warnings = Vec::new();
    if record.typecode != 0x2000_8070 || record.short {
        return Err(malformed_at(
            record.range.start,
            "object record must be long-framed",
        ));
    }
    let mut offset = record.body.start;
    let type_chunk = child(bytes, offset, record.body.end, archive, false)?;
    if type_chunk.typecode != OBJECT_RECORD_TYPE || !type_chunk.short {
        return Err(malformed_at(
            type_chunk.header_start,
            "object type must be the first short child",
        ));
    }
    let object_type = u32::try_from(type_chunk.value)
        .map_err(|_| malformed_at(type_chunk.header_start, "negative object type"))?;
    offset = type_chunk.next_offset;
    let class = child(bytes, offset, record.body.end, archive, false)?;
    require_long(&class, OPENNURBS_CLASS)?;
    offset = class.body.start;
    let uuid_chunk = child(bytes, offset, class.body.end, archive, true)?;
    require_long(&uuid_chunk, CLASS_UUID)?;
    if uuid_chunk.declared_end - uuid_chunk.body_start != 20 {
        return Err(malformed_at(
            uuid_chunk.header_start,
            "class UUID chunk must have a 20-byte body",
        ));
    }
    if let Some(note) = checksum_warning(bytes, &uuid_chunk)? {
        warnings.push(note);
    }
    let class_uuid = Uuid::from_wire(
        bytes[uuid_chunk.body.clone()]
            .try_into()
            .expect("UUID length checked"),
    );
    offset = uuid_chunk.next_offset;
    let data_chunk = child(bytes, offset, class.body.end, archive, false)?;
    require_long(&data_chunk, CLASS_DATA)?;
    if let Some(note) = checksum_warning(bytes, &data_chunk)? {
        warnings.push(note);
    }
    let class_data_range = data_chunk.body.clone();
    offset = data_chunk.next_offset;
    let mut userdata = Vec::new();
    let mut class_end_seen = false;
    while offset < class.body.end {
        let item = child(bytes, offset, class.body.end, archive, false)?;
        if item.typecode == CLASS_USERDATA {
            require_long(&item, CLASS_USERDATA)?;
            userdata.push(parse_userdata(bytes, &item, archive, &mut warnings)?);
            offset = item.next_offset;
        } else {
            require_short_zero(&item, CLASS_END)?;
            offset = item.next_offset;
            class_end_seen = true;
            break;
        }
    }
    if !class_end_seen || offset != class.body.end {
        return Err(malformed_at(
            class.body.end,
            "class wrapper has trailing bytes",
        ));
    }
    let mut attributes_range = None;
    let mut attributes_body_range = None;
    let mut attributes_userdata_range = None;
    let mut attributes_userdata_body_range = None;
    let mut history = None;
    let mut unknown_trailer = Vec::new();
    let mut phase = 0_u8;
    let mut object_end_seen = false;
    while offset < record.body.end {
        let item = child(bytes, offset, record.body.end, archive, false)?;
        if item.typecode == OBJECT_RECORD_END {
            require_short_zero(&item, OBJECT_RECORD_END)?;
            if item.next_offset != record.body.end {
                return Err(malformed_at(item.header_start, "object end is not final"));
            }
            offset = item.next_offset;
            object_end_seen = true;
            break;
        }
        match item.typecode {
            OBJECT_RECORD_ATTRIBUTES if phase == 0 => {
                require_long(&item, OBJECT_RECORD_ATTRIBUTES)?;
                attributes_range = Some(item.range());
                attributes_body_range = Some(item.body.clone());
                phase = 1;
            }
            OBJECT_RECORD_ATTRIBUTES_USERDATA if phase <= 1 => {
                require_long(&item, OBJECT_RECORD_ATTRIBUTES_USERDATA)?;
                attributes_userdata_range = Some(item.range());
                attributes_userdata_body_range = Some(item.body.clone());
                phase = 2;
            }
            OBJECT_RECORD_HISTORY if phase <= 2 => {
                require_long(&item, OBJECT_RECORD_HISTORY)?;
                history = Some(parse_history(bytes, &item, archive, &mut warnings)?);
                phase = 3;
            }
            _ if !item.short && phase >= 3 => {
                unknown_trailer.push(chunk_range(&item));
            }
            _ => {
                return Err(malformed_at(
                    item.header_start,
                    "object trailer child is out of order or malformed",
                ))
            }
        }
        if let Some(note) = checksum_warning(bytes, &item)? {
            warnings.push(note);
        }
        offset = item.next_offset;
    }
    if !object_end_seen || offset != record.body.end {
        return Err(malformed_at(
            record.body.end,
            "object record is missing object end",
        ));
    }
    let mut attributes_degraded = false;
    let attributes = attributes_body_range.as_ref().and_then(|body_range| {
        match parse_attributes(
            bytes,
            body_range.clone(),
            attributes_range
                .clone()
                .unwrap_or_else(|| body_range.clone()),
            archive,
            &mut warnings,
        ) {
            Ok(value) => Some(value),
            Err(error) => {
                attributes_degraded = true;
                warnings.push(format!(
                    "object attributes at {} degraded: {error}",
                    body_range.start
                ));
                None
            }
        }
    });
    let attributes_userdata = attributes_userdata_body_range
        .as_ref()
        .map(|range| parse_attribute_userdata(bytes, range.clone(), archive, &mut warnings))
        .unwrap_or_default();
    Ok(ObjectDescriptor {
        range: record.range.clone(),
        object_type,
        class_uuid,
        class_data_range,
        attributes,
        attributes_degraded,
        attributes_userdata,
        identity: None,
        userdata,
        attributes_range,
        attributes_body_range,
        attributes_userdata_range,
        attributes_userdata_body_range,
        history,
        unknown_trailer,
        checksum_warnings: {
            global_warnings.extend(warnings.iter().cloned());
            warnings
        },
        warnings: Vec::new(),
    })
}

/// Resolves per-object source identity after document layer metadata is known.
pub(crate) fn resolve_identities(
    objects: &mut [ObjectDescriptor],
    metadata: &DocumentMetadata,
    warnings: &mut Vec<String>,
) {
    let mut seen_ids = HashSet::new();
    for (index, object) in objects.iter_mut().enumerate() {
        let mut local_warnings = Vec::new();
        resolve_identity(object, metadata, &mut local_warnings, index, &mut seen_ids);
        warnings.extend(local_warnings.iter().cloned());
        object.warnings.extend(local_warnings);
    }
}
