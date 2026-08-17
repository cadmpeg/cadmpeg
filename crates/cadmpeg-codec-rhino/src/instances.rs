// SPDX-License-Identifier: Apache-2.0
//! Rhino instance-definition and instance-reference records.

use std::collections::HashSet;
use std::ops::Range;

use cadmpeg_ir::transform::Transform;

use crate::chunks::{
    checked_count_bytes, chunk_at, direct_checksum_ranges, verify_checksum, verify_checksum_ranges,
    ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
};
use crate::container::Record;
use crate::objects::{parse_class_wrapper_with_userdata, UserdataDescriptor};
use crate::settings::{bbox, utf16};
use crate::wire::Uuid;

const INSTANCE_DEFINITION_UUID: Uuid = Uuid::from_canonical([
    0x26, 0xf8, 0xbf, 0xf6, 0x26, 0x18, 0x41, 0x7f, 0xa1, 0x58, 0x15, 0x3d, 0x64, 0xa9, 0x49, 0x89,
]);
const INSTANCE_REFERENCE_UUID: Uuid = Uuid::from_canonical([
    0xf9, 0xcf, 0xb6, 0x38, 0xb9, 0xd4, 0x43, 0x40, 0x87, 0xe3, 0xc5, 0x6e, 0x78, 0x65, 0xd9, 0x6a,
]);
const IDEF_ALTERNATIVE_PATH_USERDATA: Uuid = Uuid::from_canonical([
    0xf4, 0x2d, 0x96, 0x71, 0x21, 0xeb, 0x46, 0x92, 0x9b, 0x9a, 0xbc, 0x35, 0x07, 0xff, 0x28, 0xf5,
]);
const OPENNURBS5_APPLICATION: Uuid = Uuid::from_canonical([
    0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30, 0xd4,
]);
const ANONYMOUS: u32 = 0x4000_8000;
const MODEL_ATTRIBUTES: u32 = 0x4000_8002;
const MAX_MEMBERS: usize = 1 << 20;

/// Semantic kind of an instance definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionKind {
    /// Definition whose members are stored in this archive.
    Static,
    /// Linked definition with an embedded local member copy.
    LinkedAndEmbedded,
    /// External linked definition without a required local member copy.
    Linked,
    /// Explicitly unset or unrecognized definition type.
    Unset,
}

/// Serialized units carried by an instance definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnitDetail {
    /// Raw unit-system value.
    pub(crate) unit: i32,
    /// Meters per unit.
    pub(crate) meters_per_unit: f64,
    /// Custom-unit name, empty for standard units.
    pub(crate) custom_name: String,
}

/// Content identity carried by an external file reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentHash {
    /// Referenced byte count.
    pub(crate) byte_count: u64,
    /// Hash acquisition time.
    pub(crate) hash_time: u64,
    /// Referenced content modification time.
    pub(crate) content_time: u64,
    /// SHA-1 of the normalized file name.
    pub(crate) name_sha1: [u8; 20],
    /// SHA-1 of the file content.
    pub(crate) content_sha1: [u8; 20],
}

/// Structured external file reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileReference {
    /// Complete serialized range.
    pub(crate) source_range: Range<usize>,
    /// Stored full path.
    pub(crate) full_path: String,
    /// Stored relative path.
    pub(crate) relative_path: String,
    /// Stored content identity.
    pub(crate) content_hash: ContentHash,
    /// Raw path-status value.
    pub(crate) path_status: u32,
    /// Optional embedded image/file component identity.
    pub(crate) embedded_file_id: Option<Uuid>,
}

/// Complete parsed instance-definition table record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstanceDefinition {
    /// Complete table-record range.
    pub(crate) source_range: Range<usize>,
    /// Definition UUID.
    pub(crate) id: Uuid,
    /// Ordered source member UUIDs.
    pub(crate) members: Vec<Uuid>,
    /// Component archive index when present.
    pub(crate) index: Option<i32>,
    /// Component name.
    pub(crate) name: String,
    /// Description.
    pub(crate) description: String,
    /// URL.
    pub(crate) url: String,
    /// URL tag.
    pub(crate) url_tag: String,
    /// Semantic definition kind.
    pub(crate) kind: DefinitionKind,
    /// Definition units.
    pub(crate) units: UnitDetail,
    /// V5 linked full path.
    pub(crate) legacy_linked_path: String,
    /// V5 linked relative path.
    pub(crate) legacy_relative_linked_path: String,
    /// Exact serialized V5 linked-file checksum range.
    pub(crate) legacy_checksum_range: Option<Range<usize>>,
    /// Legacy relative-path selector.
    pub(crate) legacy_relative_path: bool,
    /// Nested linked-definition depth.
    pub(crate) linked_depth: i32,
    /// Linked-component appearance selector.
    pub(crate) linked_appearance: u32,
    /// Complete structured linked-file-reference chunk.
    pub(crate) file_reference_range: Option<Range<usize>>,
    /// Structured linked-file reference.
    pub(crate) file_reference: Option<FileReference>,
    /// Referenced-component settings retained as a complete bounded chunk.
    pub(crate) reference_settings_range: Option<Range<usize>>,
}

/// Parsed and validated instance-reference payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstanceReference {
    /// Referenced definition UUID.
    pub(crate) definition_id: Uuid,
    /// Affine transform in source length units.
    pub(crate) transform: Transform,
}

/// Result of scanning the instance-definition table.
#[derive(Debug, Clone, Default)]
pub(crate) struct DefinitionScan {
    /// Valid definitions in source order.
    pub(crate) definitions: Vec<InstanceDefinition>,
    /// Definition UUIDs that were duplicated and are therefore ambiguous.
    pub(crate) ambiguous_ids: HashSet<Uuid>,
    /// Union of member UUIDs from every safely parseable definition prefix.
    pub(crate) member_object_ids: HashSet<Uuid>,
    /// Recoverable per-record diagnostics.
    pub(crate) diagnostics: Vec<DefinitionDiagnostic>,
}

/// Recoverable instance-definition parser diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DefinitionDiagnostic {
    /// Human-readable diagnostic.
    pub(crate) message: String,
    /// Complete table-record range.
    pub(crate) source_range: Range<usize>,
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("length checked"),
    ))
}

fn finish(reader: &mut BoundedReader<'_>, _label: &str) -> Result<(), FramingError> {
    reader.skip_remaining()?;
    Ok(())
}

fn checksum_warning(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<(), FramingError> {
    if matches!(
        verify_checksum(data, chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "{label} CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    Ok(())
}

fn checksum_warning_excluding(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    children: &[Range<usize>],
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<(), FramingError> {
    let direct = direct_checksum_ranges(&chunk.body, children)?;
    if matches!(
        verify_checksum_ranges(data, chunk, &direct)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "{label} CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    Ok(())
}

fn v5_definition_kind(value: u32) -> DefinitionKind {
    match value {
        0 | 1 => DefinitionKind::Static,
        2 => DefinitionKind::LinkedAndEmbedded,
        3 => DefinitionKind::Linked,
        _ => DefinitionKind::Unset,
    }
}

fn v6_definition_kind(value: u32) -> DefinitionKind {
    match value {
        1 => DefinitionKind::Static,
        2 => DefinitionKind::LinkedAndEmbedded,
        3 => DefinitionKind::Linked,
        _ => DefinitionKind::Unset,
    }
}

fn members(reader: &mut BoundedReader<'_>) -> Result<Vec<Uuid>, FramingError> {
    let count = reader.i32()?;
    let bytes = checked_count_bytes(
        count,
        16,
        reader.remaining(),
        MAX_MEMBERS,
        reader.position(),
    )?;
    let count = bytes / 16;
    (0..count).map(|_| uuid(reader)).collect()
}

fn anonymous_versioned<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    label: &str,
    verify_container_crc: bool,
    warnings: &mut Vec<String>,
) -> Result<(crate::chunks::Chunk, BoundedReader<'a>, (i32, i32)), FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(FramingError::structural(
            reader.position(),
            format!("{label} is not anonymous"),
        ));
    }
    if verify_container_crc {
        checksum_warning(data, &chunk, label, warnings)?;
    }
    let mut payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let version = (payload.i32()?, payload.i32()?);
    reader.skip(chunk.next_offset - reader.position())?;
    Ok((chunk, payload, version))
}

fn anonymous<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<(crate::chunks::Chunk, BoundedReader<'a>), FramingError> {
    let (chunk, payload, version) =
        anonymous_versioned(data, reader, archive, label, true, warnings)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            payload.position(),
            format!("unsupported {label} version"),
        ));
    }
    Ok((chunk, payload))
}

fn unit_detail<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<UnitDetail, FramingError> {
    let (_chunk, mut payload) = anonymous(data, reader, archive, "unit detail", warnings)?;
    let unit = i32::try_from(payload.u32()?)
        .map_err(|_| FramingError::structural(payload.position(), "unit value overflow"))?;
    let meters_per_unit = payload.f64()?;
    if !meters_per_unit.is_finite() || meters_per_unit <= 0.0 {
        return Err(FramingError::structural(
            payload.position(),
            "meters-per-unit is invalid",
        ));
    }
    let custom_name = utf16(&mut payload)?;
    finish(&mut payload, "unit detail")?;
    Ok(UnitDetail {
        unit,
        meters_per_unit,
        custom_name,
    })
}

fn model_component(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(Option<i32>, Uuid, String), FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != MODEL_ATTRIBUTES || chunk.short {
        return Err(FramingError::structural(
            reader.position(),
            "missing model-component attributes",
        ));
    }
    checksum_warning(data, &chunk, "model-component attributes", warnings)?;
    let mut payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let major = payload.i32()?;
    let minor = payload.i32()?;
    if major != 1 || minor < 0 {
        return Err(FramingError::structural(
            payload.position(),
            "unsupported model-component attributes version",
        ));
    }
    let serial_status = payload.u8()?;
    match serial_status {
        0 | 2 => {}
        1 => payload.skip(12)?,
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid model serial status",
            ))
        }
    }
    let id = match payload.u8()? {
        0 | 2 => Uuid::nil(),
        1 => uuid(&mut payload)?,
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid model UUID status",
            ))
        }
    };
    match payload.u8()? {
        0 | 2 => {}
        1 => payload.skip(4)?,
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid component type status",
            ))
        }
    }
    let index = match payload.u8()? {
        0 | 2 => None,
        1 => Some(payload.i32()?),
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid component index status",
            ))
        }
    };
    let name = match payload.u8()? {
        0 | 2 => String::new(),
        1 => utf16(&mut payload)?,
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid component name status",
            ))
        }
    };
    finish(&mut payload, "model-component attributes")?;
    reader.skip(chunk.next_offset - reader.position())?;
    Ok((index, id, name))
}

pub(crate) fn file_reference<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<FileReference, FramingError> {
    let (chunk, mut payload, version) =
        anonymous_versioned(data, reader, archive, "file reference", false, warnings)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            payload.position(),
            "unsupported file-reference version",
        ));
    }
    let full_path = utf16(&mut payload)?;
    let relative_path = utf16(&mut payload)?;
    let hash = chunk_at(data, payload.position(), payload.end(), archive, false)?;
    if hash.typecode != ANONYMOUS || hash.short {
        return Err(FramingError::structural(
            payload.position(),
            "missing content-hash chunk",
        ));
    }
    let mut hash_payload = BoundedReader::new(data, hash.body.start, hash.body.end)?;
    let hash_major = hash_payload.i32()?;
    let hash_minor = hash_payload.i32()?;
    if hash_major != 1 || hash_minor < 0 {
        return Err(FramingError::structural(
            hash_payload.position(),
            "unsupported content-hash version",
        ));
    }
    let byte_count = hash_payload.u64()?;
    let hash_time = hash_payload.u64()?;
    let content_time = hash_payload.u64()?;
    let mut digest_ranges = Vec::with_capacity(2);
    let mut read_sha1 = |payload: &mut BoundedReader<'a>| -> Result<[u8; 20], FramingError> {
        let digest = chunk_at(data, payload.position(), payload.end(), archive, false)?;
        if digest.typecode != ANONYMOUS || digest.short {
            return Err(FramingError::structural(
                payload.position(),
                "missing SHA-1 chunk",
            ));
        }
        checksum_warning(data, &digest, "SHA-1 hash", warnings)?;
        let mut bytes = BoundedReader::new(data, digest.body.start, digest.body.end)?;
        let digest_major = bytes.i32()?;
        let digest_minor = bytes.i32()?;
        if digest_major != 1 || digest_minor < 0 {
            return Err(FramingError::structural(
                bytes.position(),
                "unsupported SHA-1 version",
            ));
        }
        let value = bytes.array()?;
        bytes.skip_remaining()?;
        digest_ranges.push(digest.range());
        payload.skip(digest.next_offset - payload.position())?;
        Ok(value)
    };
    let content_hash = ContentHash {
        byte_count,
        hash_time,
        content_time,
        name_sha1: read_sha1(&mut hash_payload)?,
        content_sha1: read_sha1(&mut hash_payload)?,
    };
    finish(&mut hash_payload, "content hash")?;
    checksum_warning_excluding(data, &hash, &digest_ranges, "content hash", warnings)?;
    payload.skip(hash.next_offset - payload.position())?;
    let path_status = payload.u32()?;
    let embedded_file_id = if version.1 >= 1 {
        Some(uuid(&mut payload)?)
    } else {
        None
    };
    finish(&mut payload, "file reference")?;
    checksum_warning_excluding(
        data,
        &chunk,
        std::slice::from_ref(&hash.range()),
        "file reference",
        warnings,
    )?;
    Ok(FileReference {
        source_range: chunk.range(),
        full_path,
        relative_path,
        content_hash,
        path_status,
        embedded_file_id: embedded_file_id.filter(|id| !id.is_nil()),
    })
}

fn legacy_checksum(reader: &mut BoundedReader<'_>) -> Result<Range<usize>, FramingError> {
    let start = reader.position();
    reader.skip(48)?;
    Ok(start..reader.position())
}

fn skip_object_array(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<Range<usize>>, FramingError> {
    let count = reader.i32()?;
    let count = usize::try_from(count)
        .map_err(|_| FramingError::structural(reader.position(), "negative object count"))?;
    if count > MAX_MEMBERS {
        return Err(FramingError::structural(
            reader.position(),
            "object array exceeds item limit",
        ));
    }
    let mut ranges = Vec::with_capacity(count);
    for _ in 0..count {
        let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
        if chunk.short {
            return Err(FramingError::structural(
                reader.position(),
                "object array item is short-framed",
            ));
        }
        ranges.push(chunk.range());
        reader.skip(chunk.next_offset - reader.position())?;
    }
    Ok(ranges)
}

fn reference_settings<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<Range<usize>, FramingError> {
    let (chunk, mut payload, version) =
        anonymous_versioned(data, reader, archive, "reference settings", false, warnings)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            payload.position(),
            "unsupported reference settings version",
        ));
    }
    let mut implementation_range = None;
    if payload.bool()? {
        let (implementation, mut implementation_payload, implementation_version) =
            anonymous_versioned(
                data,
                &mut payload,
                archive,
                "reference settings implementation",
                false,
                warnings,
            )?;
        if implementation_version.0 != 1 || implementation_version.1 < 0 {
            return Err(FramingError::structural(
                implementation_payload.position(),
                "unsupported reference settings implementation version",
            ));
        }
        let mut children = skip_object_array(data, &mut implementation_payload, archive)?;
        children.extend(skip_object_array(
            data,
            &mut implementation_payload,
            archive,
        )?);
        if implementation_payload.bool()? {
            let parent = chunk_at(
                data,
                implementation_payload.position(),
                implementation_payload.end(),
                archive,
                false,
            )?;
            if parent.short {
                return Err(FramingError::structural(
                    implementation_payload.position(),
                    "reference parent layer is short-framed",
                ));
            }
            children.push(parent.range());
            implementation_payload.skip(parent.next_offset - implementation_payload.position())?;
        }
        finish(
            &mut implementation_payload,
            "reference settings implementation",
        )?;
        checksum_warning_excluding(
            data,
            &implementation,
            &children,
            "reference settings implementation",
            warnings,
        )?;
        implementation_range = Some(implementation.range());
    }
    finish(&mut payload, "reference settings")?;
    let children = implementation_range
        .as_ref()
        .map_or_else(Vec::new, |range| vec![range.clone()]);
    checksum_warning_excluding(data, &chunk, &children, "reference settings", warnings)?;
    Ok(chunk.range())
}

fn parse_v5(
    data: &[u8],
    source_range: Range<usize>,
    range: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<InstanceDefinition, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    let version = (packed >> 4, packed & 0x0f);
    if version.0 != 1 || version.1 < 6 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported V5 definition version",
        ));
    }
    let id = uuid(&mut reader)?;
    if id.is_nil() {
        return Err(FramingError::structural(
            reader.position(),
            "definition UUID is nil",
        ));
    }
    let member_ids = members(&mut reader)?;
    let name = utf16(&mut reader)?;
    let description = utf16(&mut reader)?;
    let url = utf16(&mut reader)?;
    let url_tag = utf16(&mut reader)?;
    let _bounds = bbox(&mut reader)?;
    let mut kind = v5_definition_kind(reader.u32()?);
    let mut legacy_linked_path = utf16(&mut reader)?;
    if matches!(
        kind,
        DefinitionKind::Linked | DefinitionKind::LinkedAndEmbedded
    ) && legacy_linked_path.is_empty()
    {
        kind = DefinitionKind::Static;
    }
    if !matches!(
        kind,
        DefinitionKind::Linked | DefinitionKind::LinkedAndEmbedded
    ) {
        legacy_linked_path.clear();
    }
    let legacy_checksum_range = Some(legacy_checksum(&mut reader)?);
    let unit = i32::try_from(reader.u32()?)
        .map_err(|_| FramingError::structural(reader.position(), "unit value overflow"))?;
    let meters_per_unit = reader.f64()?;
    if !meters_per_unit.is_finite() {
        return Err(FramingError::structural(
            reader.position(),
            "meters-per-unit is not finite",
        ));
    }
    let legacy_relative_path = reader.bool()?;
    let legacy_relative_linked_path = if legacy_relative_path {
        std::mem::take(&mut legacy_linked_path)
    } else {
        String::new()
    };
    let units = unit_detail(data, &mut reader, archive, warnings)?;
    let _ = (unit, meters_per_unit);
    let linked_depth = reader.i32()?;
    let mut linked_appearance = reader.u32()?;
    if matches!(kind, DefinitionKind::Linked) && !matches!(linked_appearance, 1 | 2) {
        linked_appearance = if archive.value() < 50 { 1 } else { 2 };
    }
    let file_reference = if version.1 >= 7 && reader.bool()? {
        Some(file_reference(data, &mut reader, archive, warnings)?)
    } else {
        None
    };
    // Version 1.7 has an abandoned V6-WIP tail. Its fields have no stable grammar.
    if version.1 >= 7 {
        reader.skip(reader.remaining())?;
    }
    finish(&mut reader, "V5 instance definition")?;
    Ok(InstanceDefinition {
        source_range,
        id,
        members: member_ids,
        index: None,
        name,
        description,
        url,
        url_tag,
        kind,
        units,
        legacy_linked_path,
        legacy_relative_linked_path,
        legacy_checksum_range,
        legacy_relative_path,
        linked_depth,
        linked_appearance,
        file_reference_range: file_reference
            .as_ref()
            .map(|value| value.source_range.clone()),
        file_reference,
        reference_settings_range: None,
    })
}

fn parse_v6(
    data: &[u8],
    source_range: Range<usize>,
    range: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<InstanceDefinition, FramingError> {
    let mut outer = BoundedReader::new(data, range.start, range.end)?;
    let (outer_chunk, mut reader, outer_version) = anonymous_versioned(
        data,
        &mut outer,
        archive,
        "instance definition",
        false,
        warnings,
    )?;
    if outer_version.0 != 1 || outer_version.1 < 0 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported instance definition version",
        ));
    }
    finish(&mut outer, "instance-definition wrapper")?;
    let component_start = reader.position();
    let (index, id, name) = model_component(data, &mut reader, archive, warnings)?;
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child, not its offsets.
    let mut outer_children = vec![component_start..reader.position()];
    if id.is_nil() {
        return Err(FramingError::structural(
            reader.position(),
            "definition UUID is nil",
        ));
    }
    let kind = v6_definition_kind(reader.u32()?);
    let units_start = reader.position();
    let units = unit_detail(data, &mut reader, archive, warnings)?;
    outer_children.push(units_start..reader.position());
    let description = utf16(&mut reader)?;
    let url = utf16(&mut reader)?;
    let url_tag = utf16(&mut reader)?;
    let _bounds = bbox(&mut reader)?;
    let member_ids = if reader.bool()? {
        members(&mut reader)?
    } else {
        Vec::new()
    };
    let mut linked_depth = 0;
    let mut linked_appearance = 0;
    let mut linked_file = None;
    let mut reference_settings_range = None;
    if reader.bool()? {
        let (linked_chunk, mut linked, linked_version) =
            anonymous_versioned(data, &mut reader, archive, "linked type", false, warnings)?;
        if linked_version.0 != 1 || linked_version.1 < 0 {
            return Err(FramingError::structural(
                linked.position(),
                "unsupported linked-type version",
            ));
        }
        linked_file = Some(file_reference(data, &mut linked, archive, warnings)?);
        let mut linked_children = vec![linked_file
            .as_ref()
            .expect("file reference assigned")
            .source_range
            .clone()];
        linked_depth = linked.i32()?;
        linked_appearance = linked.u32()?;
        if linked.bool()? {
            reference_settings_range =
                Some(reference_settings(data, &mut linked, archive, warnings)?);
            linked_children.push(
                reference_settings_range
                    .as_ref()
                    .expect("reference settings assigned")
                    .clone(),
            );
        }
        finish(&mut linked, "linked type")?;
        checksum_warning_excluding(
            data,
            &linked_chunk,
            &linked_children,
            "linked type",
            warnings,
        )?;
        outer_children.push(linked_chunk.range());
    }
    finish(&mut reader, "instance definition")?;
    checksum_warning_excluding(
        data,
        &outer_chunk,
        &outer_children,
        "instance definition",
        warnings,
    )?;
    Ok(InstanceDefinition {
        source_range,
        id,
        members: member_ids,
        index,
        name,
        description,
        url,
        url_tag,
        kind,
        units,
        legacy_linked_path: String::new(),
        legacy_relative_linked_path: String::new(),
        legacy_checksum_range: None,
        legacy_relative_path: false,
        linked_depth,
        linked_appearance,
        file_reference_range: linked_file.as_ref().map(|value| value.source_range.clone()),
        file_reference: linked_file,
        reference_settings_range,
    })
}

fn extract_member_ids(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    v5_layout: bool,
) -> Result<Vec<Uuid>, FramingError> {
    let mut outer = BoundedReader::new(data, range.start, range.end)?;
    if v5_layout {
        let packed = outer.u8()?;
        if packed >> 4 != 1 || packed & 0x0f < 6 {
            return Err(FramingError::structural(
                outer.position(),
                "unsupported V5 definition version",
            ));
        }
        let _definition_id = uuid(&mut outer)?;
        return members(&mut outer);
    }
    let (_chunk, mut reader, version) = anonymous_versioned(
        data,
        &mut outer,
        archive,
        "instance definition",
        false,
        &mut Vec::new(),
    )?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported instance definition version",
        ));
    }
    finish(&mut outer, "instance-definition wrapper")?;
    let _component = model_component(data, &mut reader, archive, &mut Vec::new())?;
    let _kind = reader.u32()?;
    let _units = unit_detail(data, &mut reader, archive, &mut Vec::new())?;
    let _description = utf16(&mut reader)?;
    let _url = utf16(&mut reader)?;
    let _url_tag = utf16(&mut reader)?;
    let _bounds = bbox(&mut reader)?;
    if reader.bool()? {
        members(&mut reader)
    } else {
        Ok(Vec::new())
    }
}

fn parse_idef_alternative_path(
    data: &[u8],
    userdata: &UserdataDescriptor,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(String, bool), FramingError> {
    let mut reader = BoundedReader::new(
        data,
        userdata.payload_range.start,
        userdata.payload_range.end,
    )?;
    let (_chunk, mut payload, version) = anonymous_versioned(
        data,
        &mut reader,
        archive,
        "instance-definition alternate path",
        true,
        warnings,
    )?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            payload.position(),
            "unsupported instance-definition alternate-path version",
        ));
    }
    let path = utf16(&mut payload)?;
    let relative = payload.bool()?;
    finish(&mut payload, "instance-definition alternate path")?;
    finish(&mut reader, "instance-definition alternate-path userdata")?;
    Ok((path, relative))
}

fn apply_idef_alternative_path(
    data: &[u8],
    userdata: &[UserdataDescriptor],
    archive: ArchiveVersion,
    definition: &mut InstanceDefinition,
    warnings: &mut Vec<String>,
) {
    if !matches!(
        definition.kind,
        DefinitionKind::Linked | DefinitionKind::LinkedAndEmbedded
    ) {
        return;
    }

    for item in userdata.iter().filter(|item| {
        item.class_uuid == IDEF_ALTERNATIVE_PATH_USERDATA
            && item.item_uuid == IDEF_ALTERNATIVE_PATH_USERDATA
            && (item.application_uuid.is_none()
                || item.application_uuid == Some(OPENNURBS5_APPLICATION))
    }) {
        let (path, relative) = match parse_idef_alternative_path(data, item, archive, warnings) {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!(
                    "instance-definition alternate-path userdata at offset {} was dropped: {error}",
                    item.range.start
                ));
                continue;
            }
        };
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        if let Some(reference) = definition.file_reference.as_mut() {
            if relative {
                if reference.relative_path.is_empty() {
                    path.clone_into(&mut reference.relative_path);
                }
            } else if reference.full_path.is_empty() {
                path.clone_into(&mut reference.full_path);
            }
        } else if relative {
            if definition.legacy_relative_linked_path.is_empty() {
                path.clone_into(&mut definition.legacy_relative_linked_path);
                definition.legacy_relative_path = true;
            }
        } else if definition.legacy_linked_path.is_empty() {
            path.clone_into(&mut definition.legacy_linked_path);
        }
    }
}

/// Parses all instance-definition records without losing framing after a bad record.
pub(crate) fn parse_definitions(
    data: &[u8],
    records: &[Record],
    archive: ArchiveVersion,
) -> DefinitionScan {
    let mut result = DefinitionScan::default();
    let mut seen = HashSet::new();
    for record in records {
        let parsed = (|| {
            let mut warnings = Vec::new();
            let (class, userdata) = parse_class_wrapper_with_userdata(
                data,
                record.body.clone(),
                archive,
                &mut warnings,
            )?;
            if class.class_uuid != INSTANCE_DEFINITION_UUID {
                return Err(FramingError::Structural {
                    offset: record.range.start,
                    message: "instance-definition record has wrong class UUID".to_string(),
                });
            }
            let first = data
                .get(class.class_data_range.start)
                .copied()
                .unwrap_or_default();
            let v5_layout =
                archive == ArchiveVersion::V5 || (archive == ArchiveVersion::V6 && first != 0x00);
            if let Ok(member_ids) =
                extract_member_ids(data, class.class_data_range.clone(), archive, v5_layout)
            {
                result.member_object_ids.extend(member_ids);
            }
            let mut definition = if v5_layout {
                parse_v5(
                    data,
                    record.range.clone(),
                    class.class_data_range,
                    archive,
                    &mut warnings,
                )
            } else {
                parse_v6(
                    data,
                    record.range.clone(),
                    class.class_data_range,
                    archive,
                    &mut warnings,
                )
            }?;
            apply_idef_alternative_path(data, &userdata, archive, &mut definition, &mut warnings);
            for warning in warnings {
                result.diagnostics.push(DefinitionDiagnostic {
                    message: warning,
                    source_range: record.range.clone(),
                });
            }
            Ok(definition)
        })();
        match parsed {
            Ok(definition) if seen.insert(definition.id) => {
                result
                    .member_object_ids
                    .extend(definition.members.iter().copied());
                result.definitions.push(definition);
            }
            Ok(definition) => {
                result
                    .member_object_ids
                    .extend(definition.members.iter().copied());
                result.ambiguous_ids.insert(definition.id);
                result.definitions.retain(|value| value.id != definition.id);
                result.diagnostics.push(DefinitionDiagnostic {
                    message: format!("duplicate instance definition UUID {}", definition.id),
                    source_range: record.range.clone(),
                });
            }
            Err(error) => result.diagnostics.push(DefinitionDiagnostic {
                message: format!("instance definition retained: {error}"),
                source_range: record.range.clone(),
            }),
        }
    }
    result
}

fn determinant3(rows: &[[f64; 4]; 4]) -> f64 {
    rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
        - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
        + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0])
}

/// Parses a packed major-1 instance-reference payload.
pub(crate) fn parse_reference(
    data: &[u8],
    range: Range<usize>,
) -> Result<InstanceReference, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let version = reader.u8()?;
    if version >> 4 != 1 {
        return Err(FramingError::structural(
            reader.position(),
            "instance reference major version is not 1",
        ));
    }
    let definition_id = uuid(&mut reader)?;
    if definition_id.is_nil() {
        return Err(FramingError::structural(
            reader.position(),
            "instance reference definition UUID is nil",
        ));
    }
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for value in row {
            *value = reader.f64()?;
        }
    }
    let _bounds = bbox(&mut reader)?;
    finish(&mut reader, "instance reference")?;
    if !rows.iter().flatten().all(|value| value.is_finite()) {
        return Err(FramingError::structural(
            reader.position(),
            "instance transform is not finite",
        ));
    }
    if rows[3] != [0.0, 0.0, 0.0, 1.0] {
        return Err(FramingError::structural(
            reader.position(),
            "instance transform is not affine",
        ));
    }
    let determinant = determinant3(&rows);
    if !determinant.is_finite() || determinant == 0.0 {
        return Err(FramingError::structural(
            reader.position(),
            "instance transform is singular",
        ));
    }
    Ok(InstanceReference {
        definition_id,
        transform: Transform { rows },
    })
}

/// Converts source-unit translation coefficients to canonical millimeters.
pub(crate) fn scale_translation(mut transform: Transform, scale: f64) -> Option<Transform> {
    for row in transform.rows.iter_mut().take(3) {
        row[3] = crate::wire::scaled_coordinate(row[3], scale)?;
    }
    Some(transform)
}

/// Returns whether a class UUID denotes an instance reference.
pub(crate) fn is_reference_class(class_uuid: Uuid) -> bool {
    class_uuid == INSTANCE_REFERENCE_UUID
}

#[cfg(test)]
pub(crate) mod tests;
