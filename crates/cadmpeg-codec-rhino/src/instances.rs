// SPDX-License-Identifier: Apache-2.0
//! Rhino instance-definition and instance-reference records.

use crate::loss::{Diagnostics, RhinoDiagnostic, RhinoLossCode};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::transform::Transform;
use serde::{ser::SerializeStruct, Serialize};

use crate::chunks::{
    checked_count_bytes, chunk_at, direct_checksum_ranges, verify_checksum_ranges, ArchiveVersion,
    BoundedReader, ChecksumStatus, FramingError,
};
use crate::container::{OpaqueRecord, Record};
use crate::objects::{parse_class_wrapper_with_userdata, ClassUserdata, UserdataDescriptor};
use crate::settings::{bbox, utf16_retained, MillimeterScale};
use crate::wire::{uuid, Uuid};

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
const UNSET_POSITIVE_VALUE: f64 = 1.234_321_012_343_21e308;

/// Semantic kind of an instance definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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

/// Source unit metadata. The stored scale defines a physical unit only for custom units.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnitDetail {
    unit: u32,
    meters_per_unit: UnitScale,
    custom_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum UnitScale {
    Custom(PositiveReal),
    OtherBits(u64),
}

impl UnitDetail {
    fn new(unit: u32, meters_per_unit: f64, custom_name: String) -> Result<Self, &'static str> {
        let meters_per_unit = if unit == 11 {
            UnitScale::Custom(
                PositiveReal::new(meters_per_unit)
                    .filter(|value| value.get() < UNSET_POSITIVE_VALUE)
                    .ok_or("custom meters-per-unit is invalid")?,
            )
        } else {
            UnitScale::OtherBits(meters_per_unit.to_bits())
        };
        Ok(Self {
            unit,
            meters_per_unit,
            custom_name,
        })
    }

    #[cfg(test)]
    fn meters_per_unit_bits(&self) -> u64 {
        match self.meters_per_unit {
            UnitScale::Custom(value) => value.get().to_bits(),
            UnitScale::OtherBits(bits) => bits,
        }
    }
}

impl Serialize for UnitDetail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("UnitDetail", 3)?;
        record.serialize_field("unit_system", &self.unit)?;
        match self.meters_per_unit {
            UnitScale::Custom(scale) => record.serialize_field("meters_per_unit", &scale)?,
            UnitScale::OtherBits(bits) => {
                let scale = f64::from_bits(bits);
                if scale.is_finite() {
                    record.serialize_field("meters_per_unit", &scale)?;
                } else {
                    record.serialize_field("meters_per_unit_bits", &bits)?;
                }
            }
        }
        record.serialize_field("custom_unit_name", &self.custom_name)?;
        record.end()
    }
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

/// Source of an instance-definition external link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkSource {
    /// No linked path or structured file reference.
    None,
    /// Legacy full path without a relative-path alternative.
    LegacyFull(NonBlankString),
    /// Preferred legacy relative path, with an optional full-path alternative.
    LegacyRelative {
        relative_path: NonBlankString,
        full_path: Option<NonBlankString>,
    },
    /// Structured `ON_FileReference` payload.
    Structured(FileReference),
}

impl LinkSource {
    fn from_legacy(full_path: String, relative_path: String) -> Self {
        let full_path = NonBlankString::new(full_path);
        if let Some(relative_path) = NonBlankString::new(relative_path) {
            Self::LegacyRelative {
                relative_path,
                full_path,
            }
        } else {
            full_path.map_or(Self::None, Self::LegacyFull)
        }
    }
}

/// Complete parsed instance-definition table record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstanceDefinition {
    /// Complete table-record range.
    pub(crate) source_range: Range<usize>,
    /// Definition UUID.
    id: Uuid,
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
    /// Nested linked-definition depth.
    pub(crate) linked_depth: i32,
    /// Linked-component appearance selector.
    pub(crate) linked_appearance: u32,
    /// Exclusive linked-file source.
    pub(crate) link: LinkSource,
}

impl InstanceDefinition {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }
}

#[cfg(test)]
impl InstanceDefinition {
    fn file_reference(&self) -> Option<&FileReference> {
        match &self.link {
            LinkSource::Structured(value) => Some(value),
            _ => None,
        }
    }

    fn legacy_linked_path(&self) -> &str {
        match &self.link {
            LinkSource::LegacyFull(path) => path.as_str(),
            LinkSource::LegacyRelative {
                full_path: Some(path),
                ..
            } => path.as_str(),
            _ => "",
        }
    }

    fn legacy_relative_linked_path(&self) -> &str {
        match &self.link {
            LinkSource::LegacyRelative { relative_path, .. } => relative_path.as_str(),
            _ => "",
        }
    }

    fn legacy_relative_path(&self) -> bool {
        matches!(self.link, LinkSource::LegacyRelative { .. })
    }
}

/// Parsed and validated instance-reference payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InstanceReference {
    /// Referenced definition UUID.
    definition_id: Uuid,
    /// Affine transform in source length units.
    transform: Transform,
}

impl InstanceReference {
    pub(crate) fn definition_id(&self) -> Uuid {
        self.definition_id
    }

    pub(crate) fn transform(&self) -> Transform {
        self.transform
    }
}

/// Result of scanning the instance-definition table.
#[derive(Debug, Clone, Default)]
pub(crate) struct DefinitionScan {
    /// Valid definitions in source order.
    definitions: Vec<InstanceDefinition>,
    /// Definition UUIDs that were duplicated and are therefore ambiguous.
    ambiguous_ids: HashSet<Uuid>,
    /// Union of member UUIDs from every safely parseable definition prefix.
    member_object_ids: HashSet<Uuid>,
    /// Recoverable per-record diagnostics.
    diagnostics: Vec<DefinitionDiagnostic>,
}

impl DefinitionScan {
    pub(crate) fn definitions(&self) -> &[InstanceDefinition] {
        &self.definitions
    }

    pub(crate) fn is_ambiguous(&self, id: Uuid) -> bool {
        self.ambiguous_ids.contains(&id)
    }

    pub(crate) fn contains_member(&self, id: Uuid) -> bool {
        self.member_object_ids.contains(&id)
    }

    pub(crate) fn diagnostics(&self) -> &[DefinitionDiagnostic] {
        &self.diagnostics
    }
}

/// Result of scanning instance-definition records.
#[derive(Debug, Clone, Default)]
pub(crate) struct DefinitionParse {
    /// Typed definitions and diagnostics.
    pub(crate) scan: DefinitionScan,
    /// Complete records whose registered class payload was not admitted.
    pub(crate) opaque_records: Vec<OpaqueRecord>,
}

/// Recoverable instance-definition parser diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DefinitionDiagnostic {
    /// Diagnostic with the classification supplied by its producer.
    pub(crate) diagnostic: RhinoDiagnostic,
    /// Complete table-record range.
    source_range: Range<usize>,
}

impl DefinitionDiagnostic {
    pub(crate) fn to_loss(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<cadmpeg_ir::report::loss::LossNote, cadmpeg_core::CodecError> {
        let message = crate::wire::admitted_format(
            ctx,
            format_args!(
                "instance-definition record at offset {}: {}",
                self.source_range.start, self.diagnostic.message
            ),
            "Rhino instance-definition loss text",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(message.len()),
            "Rhino instance-definition loss message",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index("INSTANCE_DEFINITION_TABLE".len()),
            "Rhino instance-definition loss tag",
        )?;
        Ok(self
            .diagnostic
            .code
            .unwrap_or(RhinoLossCode::ContainerInstanceDefinitionDegraded)
            .note(&message)
            .with_provenance(
                cadmpeg_ir::SourceProvenance::root("rhino", self.source_range.start as u64)
                    .with_tag("INSTANCE_DEFINITION_TABLE"),
            ))
    }
}

/// Renders digest bytes as retained lowercase hexadecimal.
pub(crate) fn hex(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let byte_len = bytes
        .len()
        .checked_mul(2)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("hex digest length overflow"))?;
    let mut value = crate::wire::admitted_retained_string(ctx, byte_len, operation)?;
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(value)
}

fn checksum_warning_excluding(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    children: &[Range<usize>],
    label: &str,
    warnings: &mut Diagnostics,
) -> Result<(), FramingError> {
    let direct = direct_checksum_ranges(&chunk.body(), children)?;
    if matches!(
        verify_checksum_ranges(data, chunk, &direct)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push_coded_admitted(
            ctx,
            crate::loss::RhinoLossCode::IntegrityFailure,
            format_args!("{label} CRC mismatch at offset {}", chunk.header_start),
        )?;
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

fn members(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
) -> Result<Vec<Uuid>, FramingError> {
    let count = reader.i32()?;
    let bytes = checked_count_bytes(
        count,
        16,
        reader.remaining(),
        MAX_MEMBERS,
        reader.position(),
    )?;
    let count = bytes / 16;
    let mut values = crate::chunks::admitted_vec(ctx, count, "Rhino instance member UUIDs")?;
    for _ in 0..count {
        values.push(uuid(reader)?);
    }
    Ok(values)
}

fn anonymous_versioned<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    label: &str,
    verify_container_crc: bool,
    warnings: &mut Diagnostics,
) -> Result<(crate::chunks::Chunk, BoundedReader<'a>, (i32, i32)), FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            reader.position(),
            format!("{label} is not anonymous"),
        ));
    }
    if verify_container_crc {
        crate::chunks::warn_checksum(ctx, data, &chunk, label, warnings)?;
    }
    let mut payload = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let version = (payload.i32()?, payload.i32()?);
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok((chunk, payload, version))
}

fn anonymous<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    label: &str,
    warnings: &mut Diagnostics,
) -> Result<(crate::chunks::Chunk, BoundedReader<'a>), FramingError> {
    let (chunk, payload, version) =
        anonymous_versioned(ctx, data, reader, archive, label, true, warnings)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            payload.position(),
            format!("unsupported {label} version"),
        ));
    }
    Ok((chunk, payload))
}

fn unit_detail<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<UnitDetail, FramingError> {
    let (_chunk, mut payload) = anonymous(ctx, data, reader, archive, "unit detail", warnings)?;
    let unit = payload.u32()?;
    let meters_per_unit = payload.f64()?;
    let custom_name = utf16_retained(ctx, &mut payload, "Rhino instance unit name")?;
    let standard_scale = if unit == 0 {
        Some(1.0)
    } else {
        i32::try_from(unit)
            .ok()
            .and_then(crate::settings::standard_scale)
            .map(|scale| scale / 1000.0)
    };
    if unit != 11
        && (!custom_name.is_empty() || standard_scale.is_some_and(|scale| scale != meters_per_unit))
    {
        warnings.push_coded_admitted(ctx, RhinoLossCode::RedundantFieldRepaired, format_args!(
            "redundant instance unit detail contradicts unit {unit}; meters-per-unit {meters_per_unit} and custom name {custom_name:?} retained"
        ))?;
    }
    payload.skip_remaining()?;
    UnitDetail::new(unit, meters_per_unit, custom_name)
        .map_err(|message| FramingError::structural(payload.position(), message))
}

fn model_component(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<(Option<i32>, Uuid, String), FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != MODEL_ATTRIBUTES || chunk.short() {
        return Err(FramingError::structural(
            reader.position(),
            "missing model-component attributes",
        ));
    }
    crate::chunks::warn_checksum(ctx, data, &chunk, "model-component attributes", warnings)?;
    let mut payload = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
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
            ));
        }
    }
    let id = match payload.u8()? {
        0 | 2 => Uuid::nil(),
        1 => uuid(&mut payload)?,
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid model UUID status",
            ));
        }
    };
    match payload.u8()? {
        0 | 2 => {}
        1 => payload.skip(4)?,
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid component type status",
            ));
        }
    }
    let index = match payload.u8()? {
        0 | 2 => None,
        1 => Some(payload.i32()?),
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid component index status",
            ));
        }
    };
    let name = match payload.u8()? {
        0 | 2 => String::new(),
        1 => utf16_retained(ctx, &mut payload, "Rhino instance component name")?,
        _ => {
            return Err(FramingError::structural(
                payload.position(),
                "invalid component name status",
            ));
        }
    };
    payload.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok((index, id, name))
}

pub(crate) fn file_reference<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<FileReference, FramingError> {
    let (chunk, mut payload, version) = anonymous_versioned(
        ctx,
        data,
        reader,
        archive,
        "file reference",
        false,
        warnings,
    )?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            payload.position(),
            "unsupported file-reference version",
        ));
    }
    let full_path = utf16_retained(ctx, &mut payload, "Rhino file reference full path")?;
    let relative_path = utf16_retained(ctx, &mut payload, "Rhino file reference relative path")?;
    let hash = chunk_at(data, payload.position(), payload.end(), archive, false)?;
    if hash.typecode != ANONYMOUS || hash.short() {
        return Err(FramingError::structural(
            payload.position(),
            "missing content-hash chunk",
        ));
    }
    let mut hash_payload = BoundedReader::new(data, hash.body().start, hash.body().end)?;
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
    let mut read_sha1 =
        |payload: &mut BoundedReader<'a>| -> Result<([u8; 20], Range<usize>), FramingError> {
            let digest = chunk_at(data, payload.position(), payload.end(), archive, false)?;
            if digest.typecode != ANONYMOUS || digest.short() {
                return Err(FramingError::structural(
                    payload.position(),
                    "missing SHA-1 chunk",
                ));
            }
            crate::chunks::warn_checksum(ctx, data, &digest, "SHA-1 hash", warnings)?;
            let mut bytes = BoundedReader::new(data, digest.body().start, digest.body().end)?;
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
            payload.skip(digest.next_offset() - payload.position())?;
            Ok((value, digest.range()))
        };
    let (name_sha1, name_range) = read_sha1(&mut hash_payload)?;
    let (content_sha1, content_range) = read_sha1(&mut hash_payload)?;
    let content_hash = ContentHash {
        byte_count,
        hash_time,
        content_time,
        name_sha1,
        content_sha1,
    };
    hash_payload.skip_remaining()?;
    checksum_warning_excluding(
        ctx,
        data,
        &hash,
        &[name_range, content_range],
        "content hash",
        warnings,
    )?;
    payload.skip(hash.next_offset() - payload.position())?;
    let path_status = payload.u32()?;
    let embedded_file_id = if version.1 >= 1 {
        Some(uuid(&mut payload)?)
    } else {
        None
    };
    payload.skip_remaining()?;
    checksum_warning_excluding(
        ctx,
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

fn skip_object_array(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    ranges: &mut Vec<Range<usize>>,
) -> Result<(), FramingError> {
    let count = reader.i32()?;
    let count = usize::try_from(count)
        .map_err(|_| FramingError::structural(reader.position(), "negative object count"))?;
    if count > MAX_MEMBERS {
        return Err(FramingError::structural(
            reader.position(),
            "object array exceeds item limit",
        ));
    }
    crate::chunks::reserve_admitted_vec(ctx, ranges, count, "Rhino reference object ranges")?;
    for _ in 0..count {
        let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
        if chunk.short() {
            return Err(FramingError::structural(
                reader.position(),
                "object array item is short-framed",
            ));
        }
        ranges.push(chunk.range());
        reader.skip(chunk.next_offset() - reader.position())?;
    }
    Ok(())
}

fn reference_settings<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<Range<usize>, FramingError> {
    let (chunk, mut payload, version) = anonymous_versioned(
        ctx,
        data,
        reader,
        archive,
        "reference settings",
        false,
        warnings,
    )?;
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
                ctx,
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
        let mut children = Vec::new();
        skip_object_array(
            ctx,
            data,
            &mut implementation_payload,
            archive,
            &mut children,
        )?;
        skip_object_array(
            ctx,
            data,
            &mut implementation_payload,
            archive,
            &mut children,
        )?;
        if implementation_payload.bool()? {
            let parent = chunk_at(
                data,
                implementation_payload.position(),
                implementation_payload.end(),
                archive,
                false,
            )?;
            if parent.short() {
                return Err(FramingError::structural(
                    implementation_payload.position(),
                    "reference parent layer is short-framed",
                ));
            }
            crate::chunks::reserve_admitted_vec(
                ctx,
                &mut children,
                1,
                "Rhino reference parent layer range",
            )?;
            children.push(parent.range());
            implementation_payload
                .skip(parent.next_offset() - implementation_payload.position())?;
        }
        implementation_payload.skip_remaining()?;
        checksum_warning_excluding(
            ctx,
            data,
            &implementation,
            &children,
            "reference settings implementation",
            warnings,
        )?;
        implementation_range = Some(implementation.range());
    }
    payload.skip_remaining()?;
    checksum_warning_excluding(
        ctx,
        data,
        &chunk,
        implementation_range.as_slice(),
        "reference settings",
        warnings,
    )?;
    Ok(chunk.range())
}

fn parse_v5(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    source_range: Range<usize>,
    range: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
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
    let member_ids = members(ctx, &mut reader)?;
    let name = utf16_retained(ctx, &mut reader, "Rhino instance name")?;
    let description = utf16_retained(ctx, &mut reader, "Rhino instance description")?;
    let url = utf16_retained(ctx, &mut reader, "Rhino instance URL")?;
    let url_tag = utf16_retained(ctx, &mut reader, "Rhino instance URL tag")?;
    let _bounds = bbox(&mut reader)?;
    let mut kind = v5_definition_kind(reader.u32()?);
    let mut legacy_linked_path = utf16_retained(ctx, &mut reader, "Rhino instance linked path")?;
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
    reader.skip(48)?;
    // The complete unit-detail child replaces these legacy unit fields.
    reader.skip(12)?;
    let legacy_relative_path = reader.bool()?;
    let legacy_relative_linked_path = if legacy_relative_path {
        std::mem::take(&mut legacy_linked_path)
    } else {
        String::new()
    };
    let units = unit_detail(ctx, data, &mut reader, archive, warnings)?;
    let linked_depth = reader.i32()?;
    let mut linked_appearance = reader.u32()?;
    if matches!(kind, DefinitionKind::Linked) && !matches!(linked_appearance, 1 | 2) {
        linked_appearance = if archive.value() < 50 { 1 } else { 2 };
    }
    let file_reference = if version.1 >= 7 && reader.bool()? {
        Some(file_reference(ctx, data, &mut reader, archive, warnings)?)
    } else {
        None
    };
    // Version 1.7 has an abandoned V6-WIP tail. Its fields have no stable grammar.
    reader.skip_remaining()?;
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
        linked_depth,
        linked_appearance,
        link: match file_reference {
            Some(value) => LinkSource::Structured(value),
            None => LinkSource::from_legacy(legacy_linked_path, legacy_relative_linked_path),
        },
    })
}

fn parse_v6(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    source_range: Range<usize>,
    range: Range<usize>,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<InstanceDefinition, FramingError> {
    let mut outer = BoundedReader::new(data, range.start, range.end)?;
    let (outer_chunk, mut reader, outer_version) = anonymous_versioned(
        ctx,
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
    outer.skip_remaining()?;
    let component_start = reader.position();
    let (index, id, name) = model_component(ctx, data, &mut reader, archive, warnings)?;
    let mut outer_children =
        crate::chunks::admitted_vec(ctx, 1, "Rhino instance definition checksum children")?;
    outer_children.push(component_start..reader.position());
    if id.is_nil() {
        return Err(FramingError::structural(
            reader.position(),
            "definition UUID is nil",
        ));
    }
    let kind = v6_definition_kind(reader.u32()?);
    let units_start = reader.position();
    let units = unit_detail(ctx, data, &mut reader, archive, warnings)?;
    crate::chunks::reserve_admitted_vec(
        ctx,
        &mut outer_children,
        1,
        "Rhino instance definition checksum children",
    )?;
    outer_children.push(units_start..reader.position());
    let description = utf16_retained(ctx, &mut reader, "Rhino instance description")?;
    let url = utf16_retained(ctx, &mut reader, "Rhino instance URL")?;
    let url_tag = utf16_retained(ctx, &mut reader, "Rhino instance URL tag")?;
    let _bounds = bbox(&mut reader)?;
    let member_ids = if reader.bool()? {
        members(ctx, &mut reader)?
    } else {
        Vec::new()
    };
    let mut linked_depth = 0;
    let mut linked_appearance = 0;
    let linked_file = if reader.bool()? {
        let (linked_chunk, mut linked, linked_version) = anonymous_versioned(
            ctx,
            data,
            &mut reader,
            archive,
            "linked type",
            false,
            warnings,
        )?;
        if linked_version.0 != 1 || linked_version.1 < 0 {
            return Err(FramingError::structural(
                linked.position(),
                "unsupported linked-type version",
            ));
        }
        let reference = file_reference(ctx, data, &mut linked, archive, warnings)?;
        let mut linked_children =
            crate::chunks::admitted_vec(ctx, 1, "Rhino linked definition checksum children")?;
        linked_children.push(reference.source_range.clone());
        linked_depth = linked.i32()?;
        linked_appearance = linked.u32()?;
        if linked.bool()? {
            let range = reference_settings(ctx, data, &mut linked, archive, warnings)?;
            crate::chunks::reserve_admitted_vec(
                ctx,
                &mut linked_children,
                1,
                "Rhino linked definition checksum children",
            )?;
            linked_children.push(range);
        }
        linked.skip_remaining()?;
        checksum_warning_excluding(
            ctx,
            data,
            &linked_chunk,
            &linked_children,
            "linked type",
            warnings,
        )?;
        crate::chunks::reserve_admitted_vec(
            ctx,
            &mut outer_children,
            1,
            "Rhino instance definition checksum children",
        )?;
        outer_children.push(linked_chunk.range());
        Some(reference)
    } else {
        None
    };
    reader.skip_remaining()?;
    checksum_warning_excluding(
        ctx,
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
        linked_depth,
        linked_appearance,
        link: linked_file.map_or(LinkSource::None, LinkSource::Structured),
    })
}

fn skip_definition_child(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    typecode: u32,
) -> Result<(), FramingError> {
    let child = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if child.typecode != typecode || child.short() {
        return Err(FramingError::structural(
            reader.position(),
            "unexpected definition metadata child",
        ));
    }
    reader.skip(child.next_offset() - reader.position())
}

fn extract_member_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
        return members(ctx, &mut outer);
    }
    let (_chunk, mut reader, version) = anonymous_versioned(
        ctx,
        data,
        &mut outer,
        archive,
        "instance definition",
        false,
        &mut Diagnostics::new(),
    )?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported instance definition version",
        ));
    }
    outer.skip_remaining()?;
    // Recovery needs the outer field boundaries, independent of metadata admission.
    skip_definition_child(data, &mut reader, archive, MODEL_ATTRIBUTES)?;
    reader.skip(4)?; // definition kind
    skip_definition_child(data, &mut reader, archive, ANONYMOUS)?;
    for _ in 0..3 {
        let count = usize::try_from(reader.u32()?).map_err(|_| FramingError::Overflow {
            offset: reader.position(),
        })?;
        let length = count.checked_mul(2).ok_or(FramingError::Overflow {
            offset: reader.position(),
        })?;
        reader.skip(length)?;
    }
    reader.skip(48)?; // bounding box
    if reader.bool()? {
        members(ctx, &mut reader)
    } else {
        Ok(Vec::new())
    }
}

fn parse_idef_alternative_path(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    userdata: &ClassUserdata,
    archive: ArchiveVersion,
    warnings: &mut Diagnostics,
) -> Result<(String, bool), FramingError> {
    let mut reader = BoundedReader::new(
        data,
        userdata.payload_range.start,
        userdata.payload_range.end,
    )?;
    let (_chunk, mut payload, version) = anonymous_versioned(
        ctx,
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
    let path = utf16_retained(ctx, &mut payload, "Rhino instance alternate path")?;
    let relative = payload.bool()?;
    payload.skip_remaining()?;
    reader.skip_remaining()?;
    Ok((path, relative))
}

fn apply_idef_alternative_path(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    userdata: &[UserdataDescriptor],
    archive: ArchiveVersion,
    definition: &mut InstanceDefinition,
    warnings: &mut Diagnostics,
) -> Result<bool, FramingError> {
    if !matches!(
        definition.kind,
        DefinitionKind::Linked | DefinitionKind::LinkedAndEmbedded
    ) {
        return Ok(false);
    }

    let mut degraded = false;
    for item in userdata
        .iter()
        .filter_map(UserdataDescriptor::known)
        .filter(|item| {
            item.class_uuid == IDEF_ALTERNATIVE_PATH_USERDATA
                && item.item_uuid == IDEF_ALTERNATIVE_PATH_USERDATA
                && (item.application_uuid.is_none()
                    || item.application_uuid == Some(OPENNURBS5_APPLICATION))
        })
    {
        let (path, relative) = match parse_idef_alternative_path(ctx, data, item, archive, warnings)
        {
            Ok(value) => value,
            Err(error @ FramingError::Resource(_)) => return Err(error),
            Err(error) => {
                degraded = true;
                warnings.push_admitted(
                    ctx,
                    format_args!(
                    "instance-definition alternate-path userdata at offset {} was dropped: {error}",
                    item.range.start
                ),
                )?;
                continue;
            }
        };
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(path.trim().len()),
            "Rhino instance alternate path key",
        )?;
        let Some(path) = NonBlankString::new(path.trim()) else {
            continue;
        };
        match &mut definition.link {
            LinkSource::Structured(reference) => {
                if relative {
                    if reference.relative_path.is_empty() {
                        reference.relative_path = crate::wire::copy_retained_string(
                            ctx,
                            path.as_str(),
                            "Rhino instance relative path",
                        )?;
                    }
                } else if reference.full_path.is_empty() {
                    reference.full_path = crate::wire::copy_retained_string(
                        ctx,
                        path.as_str(),
                        "Rhino instance full path",
                    )?;
                }
            }
            LinkSource::LegacyFull(full_path) => {
                if relative {
                    let copied_full_path = crate::wire::copy_retained_string(
                        ctx,
                        full_path.as_str(),
                        "Rhino instance full path",
                    )?;
                    let copied_full_path =
                        NonBlankString::new(copied_full_path).ok_or_else(|| {
                            FramingError::structural(
                                item.range.start,
                                "invalid copied instance full path",
                            )
                        })?;
                    definition.link = LinkSource::LegacyRelative {
                        relative_path: path,
                        full_path: Some(copied_full_path),
                    };
                }
            }
            LinkSource::LegacyRelative { full_path, .. } => {
                if !relative && full_path.is_none() {
                    *full_path = Some(path);
                }
            }
            LinkSource::None => {
                definition.link = if relative {
                    LinkSource::LegacyRelative {
                        relative_path: path,
                        full_path: None,
                    }
                } else {
                    LinkSource::LegacyFull(path)
                };
            }
        }
    }
    Ok(degraded)
}

/// Parses all instance-definition records without losing framing after a bad record.
fn insert_opaque_index(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    indices: &mut BTreeSet<usize>,
    index: usize,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_collection_items(1, "Rhino opaque instance definition indexes")?;
    indices.insert(index);
    Ok(())
}

pub(crate) fn parse_definitions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    records: &[Record],
    archive: ArchiveVersion,
    table_typecode: u32,
) -> Result<DefinitionParse, cadmpeg_core::CodecError> {
    let mut result = DefinitionParse::default();
    let mut seen = HashMap::new();
    let mut opaque_indices = BTreeSet::new();
    for (source_order, record) in records.iter().enumerate() {
        let mut warnings = Diagnostics::new();
        let parsed = (|| {
            let (class, userdata) = parse_class_wrapper_with_userdata(
                ctx,
                data,
                record.body(),
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
            match extract_member_ids(
                ctx,
                data,
                class.class_data_range.clone(),
                archive,
                v5_layout,
            ) {
                Ok(member_ids) => {
                    crate::wire::reserve_hash_set(
                        ctx,
                        &mut result.scan.member_object_ids,
                        member_ids.len(),
                        "Rhino instance member identities",
                    )?;
                    result.scan.member_object_ids.extend(member_ids);
                }
                Err(FramingError::Resource(limit)) => return Err(FramingError::Resource(limit)),
                Err(_) => {}
            }
            let mut definition = if v5_layout {
                parse_v5(
                    ctx,
                    data,
                    record.range.clone(),
                    class.class_data_range,
                    archive,
                    &mut warnings,
                )
            } else {
                parse_v6(
                    ctx,
                    data,
                    record.range.clone(),
                    class.class_data_range,
                    archive,
                    &mut warnings,
                )
            }?;
            let userdata_degraded = apply_idef_alternative_path(
                ctx,
                data,
                &userdata,
                archive,
                &mut definition,
                &mut warnings,
            )?;
            Ok((definition, userdata_degraded))
        })();
        crate::wire::reserve_collection(
            ctx,
            &mut result.scan.diagnostics,
            warnings.len(),
            "Rhino instance definition diagnostics",
        )?;
        result
            .scan
            .diagnostics
            .extend(warnings.into_iter().map(|diagnostic| DefinitionDiagnostic {
                diagnostic,
                source_range: record.range.clone(),
            }));
        match parsed {
            Ok((definition, userdata_degraded)) => {
                if userdata_degraded {
                    insert_opaque_index(ctx, &mut opaque_indices, source_order)?;
                }
                crate::wire::reserve_hash_set(
                    ctx,
                    &mut result.scan.member_object_ids,
                    definition.members.len(),
                    "Rhino instance member identities",
                )?;
                result
                    .scan
                    .member_object_ids
                    .extend(definition.members.iter().copied());
                if let Some(&first) = seen.get(&definition.id) {
                    insert_opaque_index(ctx, &mut opaque_indices, first)?;
                    insert_opaque_index(ctx, &mut opaque_indices, source_order)?;
                    let duplicate_message = || {
                        crate::wire::admitted_format(
                            ctx,
                            format_args!("duplicate instance definition UUID {}", definition.id),
                            "Rhino duplicate instance definition diagnostic",
                        )
                    };
                    if !result.scan.ambiguous_ids.contains(&definition.id) {
                        crate::wire::reserve_hash_set(
                            ctx,
                            &mut result.scan.ambiguous_ids,
                            1,
                            "Rhino ambiguous instance definitions",
                        )?;
                        result.scan.ambiguous_ids.insert(definition.id);
                        crate::wire::reserve_collection(
                            ctx,
                            &mut result.scan.diagnostics,
                            1,
                            "Rhino instance definition diagnostics",
                        )?;
                        result.scan.diagnostics.push(DefinitionDiagnostic {
                            diagnostic: RhinoDiagnostic {
                                code: Some(RhinoLossCode::ContainerInstanceDefinitionDegraded),
                                message: duplicate_message()?,
                            },
                            source_range: records[first].range.clone(),
                        });
                    }
                    crate::wire::reserve_collection(
                        ctx,
                        &mut result.scan.diagnostics,
                        1,
                        "Rhino instance definition diagnostics",
                    )?;
                    result.scan.diagnostics.push(DefinitionDiagnostic {
                        diagnostic: RhinoDiagnostic {
                            code: Some(RhinoLossCode::ContainerInstanceDefinitionDegraded),
                            message: duplicate_message()?,
                        },
                        source_range: record.range.clone(),
                    });
                } else {
                    crate::wire::reserve_hash_map(
                        ctx,
                        &mut seen,
                        1,
                        "Rhino instance definition identities",
                    )?;
                    seen.insert(definition.id, source_order);
                    crate::wire::reserve_collection(
                        ctx,
                        &mut result.scan.definitions,
                        1,
                        "Rhino instance definitions",
                    )?;
                    result.scan.definitions.push(definition);
                }
            }
            Err(FramingError::Resource(limit)) => {
                return Err(cadmpeg_core::CodecError::ResourceLimit(limit));
            }
            Err(error) => {
                crate::wire::reserve_collection(
                    ctx,
                    &mut result.scan.diagnostics,
                    1,
                    "Rhino instance definition diagnostics",
                )?;
                result.scan.diagnostics.push(DefinitionDiagnostic {
                    diagnostic: RhinoDiagnostic {
                        code: Some(RhinoLossCode::ContainerInstanceDefinitionDegraded),
                        message: crate::wire::admitted_format(
                            ctx,
                            format_args!("instance definition retained: {error}"),
                            "Rhino instance definition diagnostic",
                        )?,
                    },
                    source_range: record.range.clone(),
                });
                insert_opaque_index(ctx, &mut opaque_indices, source_order)?;
            }
        }
    }
    result
        .scan
        .definitions
        .retain(|definition| !result.scan.ambiguous_ids.contains(&definition.id));
    let mut opaque_records = crate::wire::admitted_collection(
        ctx,
        opaque_indices.len(),
        "Rhino opaque instance definitions",
    )?;
    for index in opaque_indices {
        opaque_records.push(OpaqueRecord {
            table_typecode,
            record: records[index].clone(),
        });
    }
    result.opaque_records = opaque_records;
    Ok(result)
}

/// Parses a packed major-1 instance-reference payload.
// ON_InstanceRef::SingularTransformationTolerance applies to inverse * source,
// relative to the magnitude of the column it compares.
const EPS_INVERSE_IDENTITY: f64 = 1.0e-6;

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
    let rows_offset = reader.position();
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for value in row {
            *value = reader.f64()?;
        }
    }
    let _bounds = bbox(&mut reader)?;
    reader.skip_remaining()?;
    if rows[3] != [0.0, 0.0, 0.0, 1.0] {
        return Err(FramingError::structural(
            reader.position(),
            "instance transform is not affine",
        ));
    }
    let transform = Transform::affine([rows[0], rows[1], rows[2]])
        .ok_or_else(|| FramingError::structural(rows_offset, "instance transform is not finite"))?;
    let inverse = transform.try_inverse_affine().map_err(|error| {
        FramingError::structural(
            reader.position(),
            format!("instance transform inverse: {error}"),
        )
    })?;
    let residual = inverse.compose(transform).map_err(|error| {
        FramingError::structural(
            reader.position(),
            format!("instance transform inverse product: {error}"),
        )
    })?;
    let source_rows = transform.affine_rows();
    // The composition is exact, so a column states its inverse only to the
    // relative precision of its own coefficients: a translation near the
    // finite limit cancels to the units it cannot represent, not to zero.
    let tolerance: [f64; 4] = std::array::from_fn(|column| {
        EPS_INVERSE_IDENTITY
            * source_rows
                .iter()
                .fold(1.0_f64, |scale, row| scale.max(row[column].abs()))
    });
    if residual
        .affine_rows()
        .iter()
        .zip(Transform::identity().affine_rows())
        .any(|(actual, expected)| {
            actual
                .iter()
                .zip(expected)
                .enumerate()
                .any(|(column, (actual, expected))| (actual - expected).abs() > tolerance[column])
        })
    {
        return Err(FramingError::structural(
            reader.position(),
            "instance transform inverse product is not identity",
        ));
    }
    Ok(InstanceReference {
        definition_id,
        transform,
    })
}

/// Converts source-unit translation coefficients to canonical millimeters.
pub(crate) fn scale_translation(transform: Transform, scale: MillimeterScale) -> Option<Transform> {
    let rows = transform.affine_rows();
    let translation = cadmpeg_ir::features::FiniteVector3::from_components(
        crate::wire::scaled_coordinate(rows[0][3], scale)?,
        crate::wire::scaled_coordinate(rows[1][3], scale)?,
        crate::wire::scaled_coordinate(rows[2][3], scale)?,
    );
    Some(transform.with_translation(translation))
}

/// Returns whether a class UUID denotes an instance reference.
pub(crate) fn is_reference_class(class_uuid: Uuid) -> bool {
    class_uuid == INSTANCE_REFERENCE_UUID
}

#[cfg(test)]
pub(crate) mod tests;
