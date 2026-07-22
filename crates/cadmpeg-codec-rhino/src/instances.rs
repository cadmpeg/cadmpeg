// SPDX-License-Identifier: Apache-2.0
//! Rhino instance-definition and instance-reference records.

use std::collections::HashSet;
use std::ops::Range;

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

use crate::chunks::{
    checked_count_bytes, chunk_at, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus,
    FramingError,
};
use crate::container::Record;
use crate::objects::parse_class_wrapper;
use crate::settings::{bbox, utf16};
use crate::wire::Uuid;

const INSTANCE_DEFINITION_UUID: Uuid = Uuid::from_canonical([
    0x26, 0xf8, 0xbf, 0xf6, 0x26, 0x18, 0x41, 0x7f, 0xa1, 0x58, 0x15, 0x3d, 0x64, 0xa9, 0x49, 0x89,
]);
const INSTANCE_REFERENCE_UUID: Uuid = Uuid::from_canonical([
    0xf9, 0xcf, 0xb6, 0x38, 0xb9, 0xd4, 0x43, 0x40, 0x87, 0xe3, 0xc5, 0x6e, 0x78, 0x65, 0xd9, 0x6a,
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

fn structural(reader: &BoundedReader<'_>, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset: reader.position(),
        message: message.into(),
    }
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("length checked"),
    ))
}

fn finish(reader: &BoundedReader<'_>, label: &str) -> Result<(), FramingError> {
    if reader.remaining() == 0 {
        Ok(())
    } else {
        Err(structural(reader, format!("{label} has trailing bytes")))
    }
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
        return Err(structural(reader, format!("{label} is not anonymous")));
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
    if version != (1, 0) {
        return Err(structural(&payload, format!("unsupported {label} version")));
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
    let unit =
        i32::try_from(payload.u32()?).map_err(|_| structural(&payload, "unit value overflow"))?;
    let meters_per_unit = payload.f64()?;
    if !meters_per_unit.is_finite() || meters_per_unit <= 0.0 {
        return Err(structural(&payload, "meters-per-unit is invalid"));
    }
    let custom_name = utf16(&mut payload)?;
    finish(&payload, "unit detail")?;
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
        return Err(structural(reader, "missing model-component attributes"));
    }
    checksum_warning(data, &chunk, "model-component attributes", warnings)?;
    let mut payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if (payload.i32()?, payload.i32()?) != (1, 0) {
        return Err(structural(
            &payload,
            "unsupported model-component attributes version",
        ));
    }
    let serial_status = payload.u8()?;
    match serial_status {
        0 | 2 => {}
        1 => payload.skip(12)?,
        _ => return Err(structural(&payload, "invalid model serial status")),
    }
    let id = match payload.u8()? {
        0 | 2 => Uuid::nil(),
        1 => uuid(&mut payload)?,
        _ => return Err(structural(&payload, "invalid model UUID status")),
    };
    match payload.u8()? {
        0 | 2 => {}
        1 => payload.skip(4)?,
        _ => return Err(structural(&payload, "invalid component type status")),
    }
    let index = match payload.u8()? {
        0 | 2 => None,
        1 => Some(payload.i32()?),
        _ => return Err(structural(&payload, "invalid component index status")),
    };
    let name = match payload.u8()? {
        0 | 2 => String::new(),
        1 => utf16(&mut payload)?,
        _ => return Err(structural(&payload, "invalid component name status")),
    };
    finish(&payload, "model-component attributes")?;
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
    if version.0 != 1 || !(0..=1).contains(&version.1) {
        return Err(structural(&payload, "unsupported file-reference version"));
    }
    let full_path = utf16(&mut payload)?;
    let relative_path = utf16(&mut payload)?;
    let hash = chunk_at(data, payload.position(), payload.end(), archive, false)?;
    if hash.typecode != ANONYMOUS || hash.short {
        return Err(structural(&payload, "missing content-hash chunk"));
    }
    let mut hash_payload = BoundedReader::new(data, hash.body.start, hash.body.end)?;
    if (hash_payload.i32()?, hash_payload.i32()?) != (1, 0) {
        return Err(structural(
            &hash_payload,
            "unsupported content-hash version",
        ));
    }
    let byte_count = hash_payload.u64()?;
    let hash_time = hash_payload.u64()?;
    let content_time = hash_payload.u64()?;
    let mut read_sha1 = |payload: &mut BoundedReader<'a>| -> Result<[u8; 20], FramingError> {
        let digest = chunk_at(data, payload.position(), payload.end(), archive, false)?;
        if digest.typecode != ANONYMOUS || digest.short {
            return Err(structural(payload, "missing SHA-1 chunk"));
        }
        checksum_warning(data, &digest, "SHA-1 hash", warnings)?;
        let mut bytes = BoundedReader::new(data, digest.body.start, digest.body.end)?;
        if (bytes.i32()?, bytes.i32()?) != (1, 0) || bytes.remaining() != 20 {
            return Err(structural(&bytes, "unsupported SHA-1 version"));
        }
        let value = bytes.array()?;
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
    finish(&hash_payload, "content hash")?;
    payload.skip(hash.next_offset - payload.position())?;
    let path_status = payload.u32()?;
    let embedded_file_id = if version.1 >= 1 {
        Some(uuid(&mut payload)?)
    } else {
        None
    };
    finish(&payload, "file reference")?;
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
) -> Result<(), FramingError> {
    let count = reader.i32()?;
    let count = usize::try_from(count).map_err(|_| structural(reader, "negative object count"))?;
    if count > MAX_MEMBERS {
        return Err(structural(reader, "object array exceeds item limit"));
    }
    for _ in 0..count {
        let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
        if chunk.short {
            return Err(structural(reader, "object array item is short-framed"));
        }
        reader.skip(chunk.next_offset - reader.position())?;
    }
    Ok(())
}

fn reference_settings<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<Range<usize>, FramingError> {
    let (chunk, mut payload) = anonymous(data, reader, archive, "reference settings", warnings)?;
    skip_object_array(data, &mut payload, archive)?;
    skip_object_array(data, &mut payload, archive)?;
    if payload.bool()? {
        let parent = chunk_at(data, payload.position(), payload.end(), archive, false)?;
        if parent.short {
            return Err(structural(
                &payload,
                "reference parent layer is short-framed",
            ));
        }
        payload.skip(parent.next_offset - payload.position())?;
    }
    finish(&payload, "reference settings")?;
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
    if version.0 != 1 || !(6..=7).contains(&version.1) {
        return Err(structural(&reader, "unsupported V5 definition version"));
    }
    let id = uuid(&mut reader)?;
    if id.is_nil() {
        return Err(structural(&reader, "definition UUID is nil"));
    }
    let member_ids = members(&mut reader)?;
    let name = utf16(&mut reader)?;
    let description = utf16(&mut reader)?;
    let url = utf16(&mut reader)?;
    let url_tag = utf16(&mut reader)?;
    let _bounds = bbox(&mut reader)?;
    let mut kind = v5_definition_kind(reader.u32()?);
    let legacy_linked_path = utf16(&mut reader)?;
    if matches!(
        kind,
        DefinitionKind::Linked | DefinitionKind::LinkedAndEmbedded
    ) && legacy_linked_path.is_empty()
    {
        kind = DefinitionKind::Static;
    }
    let legacy_checksum_range = Some(legacy_checksum(&mut reader)?);
    let unit =
        i32::try_from(reader.u32()?).map_err(|_| structural(&reader, "unit value overflow"))?;
    let meters_per_unit = reader.f64()?;
    if !meters_per_unit.is_finite() {
        return Err(structural(&reader, "meters-per-unit is not finite"));
    }
    let legacy_relative_path = reader.bool()?;
    let mut units = unit_detail(data, &mut reader, archive, warnings)?;
    if units.unit == 11 {
        units.meters_per_unit = meters_per_unit;
    } else if units.unit != unit {
        return Err(structural(
            &reader,
            "legacy and detailed unit values disagree",
        ));
    }
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
    // The failed V6-WIP linked-layer flag follows the optional file reference.
    if version.1 >= 7 && reader.remaining() == 1 {
        let _ = reader.bool()?;
    }
    finish(&reader, "V5 instance definition")?;
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
    let (_outer_chunk, mut reader) =
        anonymous(data, &mut outer, archive, "instance definition", warnings)?;
    finish(&outer, "instance-definition wrapper")?;
    let (index, id, name) = model_component(data, &mut reader, archive, warnings)?;
    if id.is_nil() {
        return Err(structural(&reader, "definition UUID is nil"));
    }
    let kind = v6_definition_kind(reader.u32()?);
    let units = unit_detail(data, &mut reader, archive, warnings)?;
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
        let (_linked_chunk, mut linked) =
            anonymous(data, &mut reader, archive, "linked type", warnings)?;
        linked_file = Some(file_reference(data, &mut linked, archive, warnings)?);
        linked_depth = linked.i32()?;
        linked_appearance = linked.u32()?;
        if linked.bool()? {
            reference_settings_range =
                Some(reference_settings(data, &mut linked, archive, warnings)?);
        }
        finish(&linked, "linked type")?;
    }
    finish(&reader, "instance definition")?;
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
        if packed >> 4 != 1 || !(6..=7).contains(&(packed & 0x0f)) {
            return Err(structural(&outer, "unsupported V5 definition version"));
        }
        let _definition_id = uuid(&mut outer)?;
        return members(&mut outer);
    }
    let (_chunk, mut reader) = anonymous(
        data,
        &mut outer,
        archive,
        "instance definition",
        &mut Vec::new(),
    )?;
    finish(&outer, "instance-definition wrapper")?;
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
            let class = parse_class_wrapper(data, record.body.clone(), archive, &mut warnings)?;
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
            let definition = if v5_layout {
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

/// Parses an exact packed 1.0 instance-reference payload.
pub(crate) fn parse_reference(
    data: &[u8],
    range: Range<usize>,
) -> Result<InstanceReference, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    if reader.u8()? != 0x10 {
        return Err(structural(&reader, "instance reference version is not 1.0"));
    }
    let definition_id = uuid(&mut reader)?;
    if definition_id.is_nil() {
        return Err(structural(
            &reader,
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
    finish(&reader, "instance reference")?;
    if !rows.iter().flatten().all(|value| value.is_finite()) {
        return Err(structural(&reader, "instance transform is not finite"));
    }
    if rows[3] != [0.0, 0.0, 0.0, 1.0] {
        return Err(structural(&reader, "instance transform is not affine"));
    }
    let determinant = determinant3(&rows);
    if !determinant.is_finite() || determinant == 0.0 {
        return Err(structural(&reader, "instance transform is singular"));
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

/// Composes transforms as `parent * child`.
pub(crate) fn compose(parent: Transform, child: Transform) -> Transform {
    let mut rows = [[0.0; 4]; 4];
    for (row_index, row) in rows.iter_mut().enumerate() {
        for (column_index, value) in row.iter_mut().enumerate() {
            *value = (0..4)
                .map(|inner| parent.rows[row_index][inner] * child.rows[inner][column_index])
                .sum();
        }
    }
    Transform { rows }
}

/// Applies an affine transform to a point.
pub(crate) fn point(transform: Transform, value: Point3) -> Point3 {
    Point3::new(
        transform.rows[0][0] * value.x
            + transform.rows[0][1] * value.y
            + transform.rows[0][2] * value.z
            + transform.rows[0][3],
        transform.rows[1][0] * value.x
            + transform.rows[1][1] * value.y
            + transform.rows[1][2] * value.z
            + transform.rows[1][3],
        transform.rows[2][0] * value.x
            + transform.rows[2][1] * value.y
            + transform.rows[2][2] * value.z
            + transform.rows[2][3],
    )
}

/// Applies the inverse-transpose linear transform and normalizes the result.
pub(crate) fn normal(transform: Transform, value: Vector3) -> Option<Vector3> {
    let determinant = determinant3(&transform.rows);
    if determinant == 0.0 || !determinant.is_finite() {
        return None;
    }
    let m = transform.rows;
    let x = ((m[1][1] * m[2][2] - m[1][2] * m[2][1]) * value.x
        + (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * value.y
        + (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * value.z)
        / determinant;
    let y = ((m[0][2] * m[2][1] - m[0][1] * m[2][2]) * value.x
        + (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * value.y
        + (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * value.z)
        / determinant;
    let z = ((m[0][1] * m[1][2] - m[0][2] * m[1][1]) * value.x
        + (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * value.y
        + (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * value.z)
        / determinant;
    let length = (x * x + y * y + z * z).sqrt();
    (length.is_finite() && length > 0.0).then(|| Vector3::new(x / length, y / length, z / length))
}

/// Returns whether a class UUID denotes an instance reference.
pub(crate) fn is_reference_class(class_uuid: Uuid) -> bool {
    class_uuid == INSTANCE_REFERENCE_UUID
}

#[cfg(test)]
mod tests {
    use super::{anonymous, compose, normal, parse_reference, point, scale_translation};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::transform::Transform;

    use crate::chunks::{ArchiveVersion, BoundedReader};

    #[test]
    fn parent_child_composition_uses_column_point_order() {
        let mut parent = Transform::identity();
        parent.rows[0][3] = 10.0;
        let mut child = Transform::identity();
        child.rows[0][0] = 2.0;
        assert_eq!(
            point(compose(parent, child), Point3::new(1.0, 0.0, 0.0)),
            Point3::new(12.0, 0.0, 0.0)
        );
    }

    #[test]
    fn translation_scales_once_without_scaling_linear_coefficients() {
        let mut source = Transform::identity();
        source.rows[0][0] = 2.0;
        source.rows[1][3] = 3.0;
        let scaled = scale_translation(source, 25.4).expect("finite translation");
        assert_eq!(scaled.rows[0][0], 2.0);
        assert_eq!(scaled.rows[1][3], 76.199_999_999_999_99);
    }

    #[test]
    fn translation_scaling_rejects_overflow() {
        let mut source = Transform::identity();
        source.rows[0][3] = f64::MAX;
        assert!(scale_translation(source, 2.0).is_none());
    }

    #[test]
    fn anonymous_instance_crc_mismatch_warns_and_consumes_boundary() {
        let body = [1_i32.to_le_bytes(), 0_i32.to_le_bytes()].concat();
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(
            &i64::try_from(body.len() + 4)
                .expect("required invariant")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
        let crc = bytes.len() - 1;
        bytes[crc] ^= 1;
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
        let mut warnings = Vec::new();
        let (_, payload) = anonymous(
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            "instance test",
            &mut warnings,
        )
        .expect("recoverable anonymous chunk");
        assert_eq!(reader.remaining(), 0);
        assert_eq!(payload.remaining(), 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("instance test CRC mismatch"));
    }

    #[test]
    fn normals_use_inverse_transpose_and_normalization() {
        let transform = Transform {
            rows: [
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 0.5, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        assert_eq!(
            normal(transform, Vector3::new(1.0, 0.0, 1.0)),
            Some(Vector3::new(
                0.242_535_625_036_332_97,
                0.0,
                0.970_142_500_145_331_9
            ))
        );
    }

    fn reference_bytes(transform: Transform) -> Vec<u8> {
        let mut bytes = vec![0x10];
        bytes.extend_from_slice(&[
            0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ]);
        for value in transform.rows.into_iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn instance_reference_requires_exact_finite_invertible_affine_payload() {
        let valid = reference_bytes(Transform::identity());
        let parsed = parse_reference(&valid, 0..valid.len()).expect("required invariant");
        assert_eq!(
            parsed.definition_id.to_string(),
            "00112233-4455-6677-8899-aabbccddeeff"
        );
        assert_eq!(parsed.transform, Transform::identity());

        let mut singular = Transform::identity();
        singular.rows[2][2] = 0.0;
        let singular = reference_bytes(singular);
        assert!(parse_reference(&singular, 0..singular.len()).is_err());

        let mut projective = Transform::identity();
        projective.rows[3][0] = 1.0;
        let projective = reference_bytes(projective);
        assert!(parse_reference(&projective, 0..projective.len()).is_err());

        let mut trailing = valid;
        trailing.push(0);
        assert!(parse_reference(&trailing, 0..trailing.len()).is_err());
    }

    #[test]
    fn instance_reference_rejects_nil_definition_and_nonfinite_transform() {
        let mut nil = reference_bytes(Transform::identity());
        nil[1..17].fill(0);
        assert!(parse_reference(&nil, 0..nil.len()).is_err());

        let mut nonfinite = Transform::identity();
        nonfinite.rows[1][2] = f64::NAN;
        let nonfinite = reference_bytes(nonfinite);
        assert!(parse_reference(&nonfinite, 0..nonfinite.len()).is_err());
    }
}
