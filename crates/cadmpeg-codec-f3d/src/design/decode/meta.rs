// SPDX-License-Identifier: Apache-2.0
//! Parse Design segment metadata and the ordered feature timeline.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::container::{ContainerEntry, ContainerRole};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;

use crate::bytes::utf16::Utf16View;
use crate::bytes::{lp_ascii_filtered_view, take_reference, Reference};
use crate::container::ContainerScan;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::record_streams::record_stream;
use crate::design::decode::reference_runs::admit_reference_values;
use crate::design::decode::sketch::{indexed_record_header_at, IndexedRecordHeader};
use crate::design::decode::text::{design_record_id_charged, fixed_guid_ascii, retain_class_tag};
use crate::metastream::MetaStream;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::{
    entity_header::{
        BaseTypeGuid, DesignFeatureTimeline, SegmentType, SegmentTypeData, DESIGN_MODULE_FUSION,
    },
    recipes::DesignComponentNamingSpace,
};

const COMPONENT_MODULE: &str = "Component";
const COMPONENT_NAMING_SPACE_BASE_TYPE_GUID: &str = "21F379C8-CAFD-4985-B461-767673A4C502";
const COMPONENT_UUID_RESERVED_LENGTHS: [usize; 2] = [2, 3];
const META_STREAM_SUFFIX: &str = "MetaStream.dat";
const BULK_STREAM_SUFFIX: &str = "BulkStream.dat";

/// Stable Design type identity of the record that owns the ordered feature
/// scope list.
pub(crate) const FEATURE_TIMELINE_TYPE_GUID: &str = "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B";
pub(crate) const FEATURE_TIMELINE_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
pub(crate) const FEATURE_TIMELINE_TYPE_VERSIONS: [u32; 2] = [2, 3];

/// Whether a type-table row has the exact registration metadata of a supported
/// feature-timeline frame.
pub(crate) fn is_supported_feature_timeline_type(design_type: &SegmentTypeData) -> bool {
    FEATURE_TIMELINE_TYPE_VERSIONS.contains(&design_type.version)
        && design_type.module == DESIGN_MODULE_FUSION
        && has_base_type(design_type, FEATURE_TIMELINE_BASE_TYPE_GUID)
}

/// Whether `guid` equals `text` without ASCII case. A relaxed GUID holds at
/// most 38 bytes and texts of another length differ without a scan, so the
/// comparison reads a constant number of bytes.
pub(super) fn guid_matches(guid: &DesignRelaxedGuidText, text: &str) -> bool {
    guid.as_str().eq_ignore_ascii_case(text)
}

/// Whether `design_type` names `base` as its base type, without ASCII case.
pub(super) fn has_base_type(design_type: &SegmentTypeData, base: &str) -> bool {
    design_type
        .base_type_guid
        .value()
        .is_some_and(|value| guid_matches(value, base))
}

/// Whether `design_type` registers component entities with UUID-bound naming
/// spaces.
fn is_component_naming_type(design_type: &SegmentTypeData) -> bool {
    design_type.module == COMPONENT_MODULE
        && has_base_type(design_type, COMPONENT_NAMING_SPACE_BASE_TYPE_GUID)
}

/// Whether `design_type` is the feature-timeline type.
fn is_feature_timeline_type(design_type: &SegmentTypeData) -> bool {
    guid_matches(&design_type.type_guid, FEATURE_TIMELINE_TYPE_GUID)
}

/// The name prefix of `entry` when it is a Design `MetaStream`.
fn design_meta_prefix<'a>(scan: &ContainerScan, entry: &'a ContainerEntry) -> Option<&'a str> {
    if !scan.is_design_stream(entry, ContainerRole::Metastream) {
        return None;
    }
    entry.name.strip_suffix(META_STREAM_SUFFIX)
}

/// The first archive entry named `prefix` followed by `suffix`. Only names of
/// the combined length are compared.
fn sibling_entry<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    prefix: &str,
    suffix: &'static str,
    operation: &'static str,
) -> Result<Option<&'a ContainerEntry>, CodecError> {
    let Some(length) = prefix.len().checked_add(suffix.len()) else {
        return Ok(None);
    };
    ctx.find_by(
        &scan.entries,
        |entry| {
            Ok(entry.name.len() == length
                && entry.name.ends_with(suffix)
                && ctx.starts_with(&entry.name, prefix, operation)?)
        },
        operation,
    )
}

fn paired_bulk_entry_name<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    prefix: &str,
) -> Result<&'a str, CodecError> {
    match sibling_entry(
        ctx,
        scan,
        prefix,
        BULK_STREAM_SUFFIX,
        "find F3D paired Design BulkStream",
    )? {
        Some(entry) => Ok(&entry.name),
        None => Err(crate::design::text::malformed_design(
            ctx,
            format_args!("entry {prefix}{BULK_STREAM_SUFFIX} not found"),
        )),
    }
}

/// Decode the type table of every Design `MetaStream` entry.
pub(crate) fn decode_types(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SegmentType>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx.admit_iter(&scan.entries, "scan F3D Design MetaStream entries")? {
        if design_meta_prefix(scan, entry).is_none() {
            continue;
        }
        let meta = scan.parsed_metastream(ctx, &entry.name)?;
        for design_type in ctx.admit_iter(&meta.types, "copy F3D Design type table")? {
            let copied = copy_design_type(ctx, design_type, &entry.name)?;
            ctx.push_vec(&mut out, copied, "f3d design type table")?;
        }
    }
    Ok(out)
}

fn copy_design_type(
    ctx: &DecodeContext<'_>,
    design_type: &SegmentTypeData,
    stream: &str,
) -> Result<SegmentType, CodecError> {
    use crate::records::identity::ReferenceRun;

    let entities = if let Some(rows) = design_type.entities.located_rows() {
        let mut copied = Vec::new();
        ctx.extend_from_slice(&mut copied, rows, "f3d design type registered entities")?;
        ReferenceRun::located(copied)
    } else {
        let mut copied = Vec::new();
        ctx.extend_from_slice(
            &mut copied,
            design_type.entities.unlocated_values().unwrap_or(&[]),
            "f3d design type registered entities",
        )?;
        ReferenceRun::unlocated(copied)
    };
    let module = ctx.copy_retained_text(&design_type.module, "f3d design type module")?;
    let type_guid = design_type
        .type_guid
        .try_clone_for_decode(ctx, "f3d design type GUID")?;
    let base_type_guid = match &design_type.base_type_guid {
        BaseTypeGuid::Absent => BaseTypeGuid::Absent,
        BaseTypeGuid::EmptyRoot { offset } => BaseTypeGuid::EmptyRoot { offset: *offset },
        BaseTypeGuid::Guid { value, offset } => BaseTypeGuid::Guid {
            value: value.try_clone_for_decode(ctx, "f3d design base type GUID")?,
            offset: *offset,
        },
    };
    let id = design_record_id_charged(
        ctx,
        stream,
        ":design-type#",
        design_type.byte_offset,
        "f3d design type id suffix",
    )?;
    SegmentType::try_new(
        id,
        SegmentTypeData {
            byte_offset: design_type.byte_offset,
            type_guid,
            type_guid_offset: design_type.type_guid_offset,
            base_type_guid,
            version: design_type.version,
            version_offset: design_type.version_offset,
            module,
            entities,
        },
    )
    .map_err(CodecError::Malformed)
}

/// Each `(entity, type ordinal)` registration of a component naming type in
/// `meta`, in ascending order, held under `reservation`.
fn component_naming_registrations(
    ctx: &DecodeContext<'_>,
    reservation: &mut ScopedReservation<'_>,
    meta: &MetaStream,
) -> Result<Vec<(u64, usize)>, CodecError> {
    let mut registrations = Vec::new();
    for (ordinal, design_type) in ctx
        .admit_iter(&meta.types, "scan F3D component naming types")?
        .enumerate()
    {
        if !is_component_naming_type(design_type) {
            continue;
        }
        for entity_id in admit_reference_values(
            ctx,
            &design_type.entities,
            "scan F3D component naming entities",
        )? {
            ctx.push_scoped_vec(
                reservation,
                &mut registrations,
                (*entity_id, ordinal),
                "f3d component naming registered entities",
            )?;
        }
    }
    ctx.sort_unstable_by_key(
        &mut registrations,
        |registration| *registration,
        Ord::cmp,
        "sort F3D component naming registrations",
    )?;
    Ok(registrations)
}

/// Component naming spaces bound in one Design `BulkStream`.
struct ComponentBindings<'s, 'ctx> {
    bulk_name: &'s str,
    /// Output position of the naming space of each bound component.
    by_component: BTreeMap<u64, usize>,
    reservation: ScopedReservation<'ctx>,
}

impl ComponentBindings<'_, '_> {
    /// Bind `component_record_index` to the context UUID at
    /// `context_uuid_offset`, found through the marker at `marker`. A later
    /// binding of the same component must repeat the UUID.
    fn bind(
        &mut self,
        ctx: &DecodeContext<'_>,
        out: &mut Vec<DesignComponentNamingSpace>,
        marker: usize,
        component_record_index: u64,
        context_uuid: &str,
        context_uuid_offset: usize,
    ) -> Result<(), CodecError> {
        if let Some(&slot) = ctx.get_btree_map(
            &self.by_component,
            &component_record_index,
            "find F3D component naming space",
        )? {
            let existing = out.get(slot).ok_or_else(|| {
                CodecError::malformed("F3D component naming space is outside its output")
            })?;
            // Both UUIDs hold 36 bytes.
            if existing.context_uuid.as_str() != context_uuid {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                    "Design component {component_record_index} has conflicting context UUID bindings"
                ),
                ));
            }
            return Ok(());
        }
        let byte_offset = u64::try_from(marker)
            .map_err(|_| ctx.refuse_codec_limit("f3d component naming marker offset", 0, 1))?;
        let id = design_record_id_charged(
            ctx,
            self.bulk_name,
            ":design-component-naming-space#",
            byte_offset,
            "f3d component naming space id suffix",
        )?;
        let context_uuid = DesignRelaxedGuidText::try_from(
            ctx.copy_retained_text(context_uuid, "retain F3D component context UUID")?,
        )
        .map_err(CodecError::Malformed)?;
        let context_uuid_offset = u64::try_from(context_uuid_offset)
            .map_err(|_| ctx.refuse_codec_limit("f3d component naming UUID offset", 0, 1))?;
        let slot = out.len();
        self.reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut self.by_component,
                component_record_index,
                slot,
                "f3d component naming spaces by entity",
            )
        })?;
        ctx.push_vec(
            out,
            DesignComponentNamingSpace {
                id,
                byte_offset,
                component_record_index,
                context_uuid,
                context_uuid_offset,
            },
            "f3d component naming spaces output",
        )
    }
}

/// The text of a GUID whose code units were validated as ASCII.
fn ascii_guid(guid: &[u8; 36]) -> Result<&str, CodecError> {
    std::str::from_utf8(guid)
        .map_err(|_| CodecError::malformed("validated F3D relaxed GUID is not ASCII"))
}

/// Whether the `length` reserved bytes at `at` are zero.
fn reserved_zeros(bytes: &[u8], at: usize, length: usize) -> bool {
    match length {
        2 => zeros_at::<2>(bytes, at),
        3 => zeros_at::<3>(bytes, at),
        _ => false,
    }
}

/// Bind every marked component UUID in `bytes`: marker `1` after a byte other
/// than `1`, a registered component entity ID, reserved zero bytes and an
/// exact 36-unit UUID.
fn bind_marked_component_uuids(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    registrations: &[(u64, usize)],
    bindings: &mut ComponentBindings<'_, '_>,
    out: &mut Vec<DesignComponentNamingSpace>,
) -> Result<(), CodecError> {
    for reserved_len in COMPONENT_UUID_RESERVED_LENGTHS {
        let prefix_len = 1 + 8 + reserved_len;
        let Some(markers) = bytes
            .len()
            .checked_sub(4)
            .and_then(|last| last.checked_sub(prefix_len))
        else {
            continue;
        };
        for marker in ctx.admit_iter(&(0..markers), "scan F3D component naming UUID offsets")? {
            let uuid_offset = marker + prefix_len;
            if bytes[marker] != 1
                || (marker > 0 && bytes[marker - 1] == 1)
                || !reserved_zeros(bytes, marker + 9, reserved_len)
            {
                continue;
            }
            let Some(component_record_index) = View::u64_le_at(bytes, marker + 1) else {
                continue;
            };
            if ctx
                .binary_search_by(
                    registrations,
                    |(entity_id, _)| Ok(entity_id.cmp(&component_record_index)),
                    "find F3D component naming registration",
                )?
                .is_err()
            {
                continue;
            }
            let Some((context_uuid, _)) = fixed_guid_ascii(bytes, uuid_offset) else {
                continue;
            };
            bindings.bind(
                ctx,
                out,
                marker,
                component_record_index,
                ascii_guid(&context_uuid)?,
                uuid_offset,
            )?;
        }
    }
    Ok(())
}

/// Bind every local reference in `bytes` whose inline type is a component
/// naming type that registers the target, followed by an exact 36-unit UUID.
fn bind_referenced_component_uuids(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &MetaStream,
    registrations: &[(u64, usize)],
    bindings: &mut ComponentBindings<'_, '_>,
    out: &mut Vec<DesignComponentNamingSpace>,
) -> Result<(), CodecError> {
    for marker in ctx.admit_iter(&(0..bytes.len()), "scan F3D component naming references")? {
        let mut uuid_offset = marker;
        let Some(reference) = take_reference(bytes, &mut uuid_offset) else {
            continue;
        };
        let Some((component_record_index, Some(inline_type_guid))) = reference.local() else {
            continue;
        };
        let first = ctx.partition_point(
            registrations,
            |(entity_id, _)| Ok(*entity_id < component_record_index),
            "find F3D component reference registrations",
        )?;
        let following = registrations.get(first..).unwrap_or(&[]);
        let count = ctx.partition_point(
            following,
            |(entity_id, _)| Ok(*entity_id == component_record_index),
            "find F3D component reference registrations",
        )?;
        if !ctx.any_by(
            following.get(..count).unwrap_or(&[]),
            |(_, ordinal)| {
                Ok(meta.types.get(*ordinal).is_some_and(|design_type| {
                    guid_matches(&design_type.type_guid, inline_type_guid)
                }))
            },
            "match F3D component reference type",
        )? {
            continue;
        }
        let Some((context_uuid, _)) = fixed_guid_ascii(bytes, uuid_offset) else {
            continue;
        };
        bindings.bind(
            ctx,
            out,
            marker,
            component_record_index,
            ascii_guid(&context_uuid)?,
            uuid_offset,
        )?;
    }
    Ok(())
}

/// Decode each component entity's UUID-bound local naming space.
pub(crate) fn decode_component_naming_spaces(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignComponentNamingSpace>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx.admit_iter(
        &scan.entries,
        "scan F3D component naming MetaStream entries",
    )? {
        let Some(prefix) = design_meta_prefix(scan, entry) else {
            continue;
        };
        let meta = scan.parsed_metastream(ctx, &entry.name)?;
        let mut registration_reservation =
            ctx.reserve_scoped(0, "f3d component naming registered entities")?;
        let registrations =
            component_naming_registrations(ctx, &mut registration_reservation, &meta)?;
        if registrations.is_empty() {
            continue;
        }
        let bulk_name = paired_bulk_entry_name(ctx, scan, prefix)?;
        let bytes = scan.entry_bytes(bulk_name)?;
        let mut bindings = ComponentBindings {
            bulk_name,
            by_component: BTreeMap::new(),
            reservation: ctx.reserve_scoped(0, "f3d component naming spaces by entity")?,
        };
        bind_marked_component_uuids(ctx, bytes, &registrations, &mut bindings, &mut out)?;
        bind_referenced_component_uuids(
            ctx,
            bytes,
            &meta,
            &registrations,
            &mut bindings,
            &mut out,
        )?;
        // Registrations ascend by entity, so the first unbound one is the
        // least unbound component.
        if let Some((missing, _)) = ctx.find_by(
            &registrations,
            |(entity_id, _)| {
                Ok(!ctx.contains_key_btree_map(
                    &bindings.by_component,
                    entity_id,
                    "find unbound F3D component",
                )?)
            },
            "find unbound F3D component",
        )? {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!("Design component {missing} has no context UUID binding"),
            ));
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design meta 1",
    )?;
    Ok(out)
}

/// Parse the `MetaStream` paired with one Design `BulkStream`.
pub(crate) fn metadata_for_bulk_stream(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    bulk_entry_name: &str,
) -> Result<Option<std::rc::Rc<MetaStream>>, CodecError> {
    let prefix = bulk_entry_name
        .strip_suffix(BULK_STREAM_SUFFIX)
        .ok_or_else(|| CodecError::Malformed("Design stream has no BulkStream suffix".into()))?;
    let Some(meta_entry) = sibling_entry(
        ctx,
        scan,
        prefix,
        META_STREAM_SUFFIX,
        "find paired F3D MetaStream",
    )?
    else {
        return Ok(None);
    };
    scan.parsed_metastream(ctx, &meta_entry.name).map(Some)
}

/// One live Design record selected by the primary index and resolved through
/// its segment-local class tag.
#[derive(Clone)]
pub(in crate::design::decode) struct DesignPrimaryFrame<'a> {
    pub(super) entity_id: u64,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) design_type: &'a SegmentTypeData,
}

/// The type-table row that a dynamic class code selects, with its ordinal.
fn dynamic_type(meta: &MetaStream, class_code: u32) -> Option<(usize, &SegmentTypeData)> {
    let ordinal = usize::try_from(class_code).ok()?.checked_sub(256)?;
    Some((ordinal, meta.types.get(ordinal)?))
}

/// The record header at `at` when it repeats `expected_entity_id` as a 32-bit
/// record index or a 64-bit entity ID that ends by `end`.
fn record_header(
    bytes: &[u8],
    at: usize,
    end: usize,
    expected_entity_id: u64,
) -> Option<IndexedRecordHeader<'_>> {
    let header = indexed_record_header_at(bytes, at)?;
    let after_tag = at.checked_add(7)?;
    let indexed_matches = after_tag
        .checked_add(4)
        .is_some_and(|entity_end| entity_end <= end)
        && u64::from(header.record_index) == expected_entity_id;
    let named_matches = after_tag
        .checked_add(8)
        .is_some_and(|entity_end| entity_end <= end)
        && View::u64_le_at(bytes, after_tag) == Some(expected_entity_id);
    (indexed_matches || named_matches).then_some(header)
}

/// Resolve every live sibling record from the primary index. The primary
/// header must repeat the indexed entity ID and select a type-table row that
/// registers that entity. A secondary entry supplies the exact end of the
/// primary class-member sequence and must point to a nested header for the same
/// entity.
pub(super) fn design_primary_frames<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &'a MetaStream,
) -> Result<Vec<DesignPrimaryFrame<'a>>, CodecError> {
    let indexed = crate::metastream::primary_record_frames(ctx, meta, bytes.len())?;
    let mut reservation = ctx.reserve_scoped(0, "f3d registered primary entities")?;
    let mut registered = Vec::new();
    for (ordinal, design_type) in ctx
        .admit_iter(&meta.types, "scan F3D primary record types")?
        .enumerate()
    {
        for entity_id in admit_reference_values(
            ctx,
            &design_type.entities,
            "scan F3D primary entity registrations",
        )? {
            ctx.push_scoped_vec(
                &mut reservation,
                &mut registered,
                (ordinal, *entity_id),
                "f3d registered primary entities",
            )?;
        }
    }
    ctx.sort_unstable_by_key(
        &mut registered,
        |registration| *registration,
        Ord::cmp,
        "sort F3D registered primary entities",
    )?;
    let mut frames = Vec::new();
    ctx.reserve_capacity(&mut frames, indexed.len(), "f3d design primary frames")?;
    for frame in ctx.admit_iter(&indexed, "scan F3D indexed primary record frames")? {
        let entity_id = frame.entity_id;
        let Some(header) = record_header(bytes, frame.start, frame.end, entity_id) else {
            return Err(CodecError::Malformed(
                "F3D primary record index points to an invalid record header".into(),
            ));
        };
        let Some((type_ordinal, design_type)) = dynamic_type(meta, header.class_code) else {
            return Err(CodecError::Malformed(
                "F3D primary record class tag is outside its type table".into(),
            ));
        };
        if ctx
            .binary_search(
                &registered,
                &(type_ordinal, entity_id),
                "find F3D primary entity registration",
            )?
            .is_err()
        {
            return Err(CodecError::Malformed(
                "F3D primary record type does not register its indexed entity ID".into(),
            ));
        }
        if frame.member_end < frame.end {
            let Some(nested) = record_header(bytes, frame.member_end, frame.end, entity_id) else {
                return Err(CodecError::Malformed(
                    "F3D secondary record index points to an invalid nested header".into(),
                ));
            };
            if dynamic_type(meta, nested.class_code).is_none() {
                return Err(CodecError::Malformed(
                    "F3D secondary record header is incompatible with its primary record".into(),
                ));
            }
        }
        let class_tag = header.retain_class_tag(ctx, "copy F3D class tag")?;
        ctx.push_vec(
            &mut frames,
            DesignPrimaryFrame {
                entity_id,
                class_tag,
                start: frame.start,
                end: frame.end,
                design_type,
            },
            "f3d design primary frames",
        )?;
    }
    Ok(frames)
}

/// One primary `BulkStream` frame selected through a registered Design type.
#[derive(Clone, Copy)]
pub(super) struct TypedPrimaryFrame<'a> {
    pub(super) entity_id: u64,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) design_type: &'a SegmentTypeData,
}

/// Resolve every entity registered to `type_guid` through the sibling
/// `MetaStream` primary index and verify its dynamic class tag. The returned
/// frames are charged as retained.
pub(super) fn typed_primary_frames<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &'a MetaStream,
    type_guid: &str,
    record_kind: &str,
) -> Result<Vec<TypedPrimaryFrame<'a>>, CodecError> {
    let (frames, storage) =
        TypedFrameSource::new(bytes, meta).frames(ctx, type_guid, record_kind)?;
    storage.commit()?;
    Ok(frames)
}

/// The primary frames of one `BulkStream`, from which the frames of each
/// registered type are selected. The primary frames are resolved on the first
/// selection and held under a scoped reservation for the later ones; each
/// selection checks the registrations of its own type first.
pub(super) struct TypedFrameSource<'a, 'b, 'ctx> {
    bytes: &'b [u8],
    meta: &'a MetaStream,
    primary_frames: Option<(Vec<DesignPrimaryFrame<'a>>, ScopedReservation<'ctx>)>,
}

impl<'a, 'b, 'ctx> TypedFrameSource<'a, 'b, 'ctx> {
    pub(super) fn new(bytes: &'b [u8], meta: &'a MetaStream) -> Self {
        Self {
            bytes,
            meta,
            primary_frames: None,
        }
    }

    /// Resolve every entity registered to `type_guid` and verify its dynamic
    /// class tag. The frames are held under the returned reservation.
    pub(super) fn frames(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        type_guid: &str,
        record_kind: &str,
    ) -> Result<(Vec<TypedPrimaryFrame<'a>>, ScopedReservation<'ctx>), CodecError> {
        let mut reservation = ctx.reserve_scoped(0, "f3d typed primary entities")?;
        // Each registration of the type with its position in table order.
        let mut typed = Vec::new();
        for design_type in
            ctx.admit_iter(&self.meta.types, "scan F3D typed primary record types")?
        {
            if !guid_matches(&design_type.type_guid, type_guid) {
                continue;
            }
            for &entity_id in admit_reference_values(
                ctx,
                &design_type.entities,
                "scan F3D typed primary entities",
            )? {
                let position = typed.len();
                ctx.push_scoped_vec(
                    &mut reservation,
                    &mut typed,
                    (entity_id, position),
                    "f3d typed primary entities",
                )?;
            }
        }
        ctx.sort_unstable_by_key(
            &mut typed,
            |registration| *registration,
            Ord::cmp,
            "sort F3D typed primary entities",
        )?;
        // The first repeated registration in table order is the repeat at the
        // least position.
        let mut first_repeat: Option<(usize, u64)> = None;
        let mut previous = None;
        for &(entity_id, position) in
            ctx.admit_iter(&typed, "find repeated F3D typed primary entity")?
        {
            if previous.replace(entity_id) == Some(entity_id)
                && first_repeat.is_none_or(|(least, _)| position < least)
            {
                first_repeat = Some((position, entity_id));
            }
        }
        if let Some((_, entity_id)) = first_repeat {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D Design {record_kind} entity {entity_id} is registered more than once"
                ),
            ));
        }

        let mut resolved = Vec::new();
        let mut frames = Vec::new();
        let mut frames_storage = ctx.reserve_scoped(0, "f3d typed primary frames")?;
        let (primary_frames, _) = match &mut self.primary_frames {
            Some(primary_frames) => primary_frames,
            slot @ None => {
                slot.insert(ctx.with_scoped_storage("f3d design primary frames", || {
                    design_primary_frames(ctx, self.bytes, self.meta)
                })?)
            }
        };
        for primary_frame in ctx.admit_iter(&*primary_frames, "scan F3D resolved primary frames")? {
            if !guid_matches(&primary_frame.design_type.type_guid, type_guid) {
                continue;
            }
            ctx.push_scoped_vec(
                &mut reservation,
                &mut resolved,
                primary_frame.entity_id,
                "f3d resolved primary entities",
            )?;
            ctx.push_scoped_vec(
                &mut frames_storage,
                &mut frames,
                TypedPrimaryFrame {
                    entity_id: primary_frame.entity_id,
                    start: primary_frame.start,
                    end: primary_frame.end,
                    design_type: primary_frame.design_type,
                },
                "f3d typed primary frames",
            )?;
        }
        ctx.sort_unstable_by_key(
            &mut resolved,
            |entity_id| *entity_id,
            Ord::cmp,
            "sort F3D resolved primary entities",
        )?;
        // Typed entities ascend, so the first unresolved one is the least.
        if let Some((entity_id, _)) = ctx.find_by(
            &typed,
            |(entity_id, _)| {
                Ok(ctx
                    .binary_search(&resolved, entity_id, "find unresolved F3D typed entity")?
                    .is_err())
            },
            "find unresolved F3D typed entity",
        )? {
            return Err(crate::design::text::malformed_design(ctx, format_args!(
                "F3D Design {record_kind} entity {entity_id} has no primary record of its registered class"
            )));
        }
        Ok((frames, frames_storage))
    }
}

/// Type GUID and record version keyed by the Design entity ids that carry the
/// type in the sibling `BulkStream`.
pub(super) fn stream_types_by_entity<'a>(
    ctx: &DecodeContext<'_>,
    types: &'a [SegmentType],
    bulk_entry_name: &str,
) -> Result<HashMap<u64, (&'a str, u32)>, CodecError> {
    let mut by_entity = HashMap::new();
    let Some(prefix) = bulk_entry_name.strip_suffix(BULK_STREAM_SUFFIX) else {
        return Ok(by_entity);
    };
    for design_type in ctx.admit_iter(types, "scan F3D types by stream entity")? {
        let Some(scope) = record_stream(ctx, design_type.id())? else {
            continue;
        };
        if !meta_scope_matches_bulk(ctx, scope, prefix)? {
            continue;
        }
        for entity_id in
            admit_reference_values(ctx, &design_type.entities, "scan F3D stream type entities")?
        {
            ctx.insert_hash_map(
                &mut by_entity,
                *entity_id,
                (design_type.type_guid.as_str(), design_type.version),
                "f3d stream types by entity",
            )?;
        }
    }
    Ok(by_entity)
}

/// Compare an encoded native `MetaStream` scope with the name prefix of a
/// sibling `BulkStream` without materializing either name. Each prefix
/// character is admitted as the comparison reaches it.
fn meta_scope_matches_bulk(
    ctx: &DecodeContext<'_>,
    scope: &str,
    bulk_prefix: &str,
) -> Result<bool, CodecError> {
    let Some(encoded) = scope
        .strip_prefix("f3d:")
        .and_then(|scope| scope.strip_suffix(META_STREAM_SUFFIX))
    else {
        return Ok(false);
    };
    let mut observed = encoded.bytes();
    for character in bulk_prefix.chars() {
        ctx.charge_work(1, "scan F3D BulkStream scope prefix")?;
        let mut buffer = [0; 4];
        let bytes = character.encode_utf8(&mut buffer).as_bytes();
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for &byte in bytes {
                if observed.next() != Some(b'%')
                    || observed.next() != Some(HEX[usize::from(byte >> 4)])
                    || observed.next() != Some(HEX[usize::from(byte & 0x0f)])
                {
                    return Ok(false);
                }
            }
        } else if !bytes.iter().all(|byte| observed.next() == Some(*byte)) {
            return Ok(false);
        }
    }
    Ok(observed.next().is_none())
}

fn local_reference(
    ctx: &DecodeContext<'_>,
    reference: &Reference<&str, Utf16View<'_>>,
    type_guids_by_entity: &HashMap<u64, Vec<&DesignRelaxedGuidText>>,
) -> Result<Option<u64>, CodecError> {
    let Some((target, inline_type_guid)) = reference.local() else {
        return Ok(None);
    };
    if let Some(inline_type_guid) = inline_type_guid {
        let Some(registered_type_guids) = type_guids_by_entity.get(&target) else {
            return Ok(None);
        };
        if !ctx.any_by(
            registered_type_guids,
            |registered| Ok(guid_matches(registered, inline_type_guid)),
            "match F3D registered timeline types",
        )? {
            return Ok(None);
        }
    }
    Ok(Some(target))
}

/// Fixed layout at the start of a feature-timeline frame.
struct TimelineHead<'a> {
    class_tag: &'a [u8; 3],
    context_reference_offset: usize,
    context_reference: Reference<&'a str, Utf16View<'a>>,
    item_count_offset: usize,
}

/// The header with `expected` class code and entity ID at `start`, a bounded
/// graphic ASCII payload, two zero bytes and the context reference.
fn timeline_head(bytes: &[u8], start: usize, expected: (u32, u64)) -> Option<TimelineHead<'_>> {
    let (expected_class_code, expected_entity_id) = expected;
    let header = indexed_record_header_at(bytes, start)?;
    let after_tag = start.checked_add(7)?;
    if header.class_code != expected_class_code
        || View::u64_le_at(bytes, after_tag)? != expected_entity_id
    {
        return None;
    }
    let (_, payload) = lp_ascii_filtered_view(
        bytes,
        after_tag.checked_add(8)?,
        0..=2000,
        u8::is_ascii_graphic,
    )?;
    if !zeros_at::<2>(bytes, payload) {
        return None;
    }
    let mut at = payload.checked_add(2)?;
    let context_reference_offset = at.checked_add(1)?;
    let context_reference = take_reference(bytes, &mut at)?;
    Some(TimelineHead {
        class_tag: header.class_tag,
        context_reference_offset,
        context_reference,
        item_count_offset: at,
    })
}

fn parse_feature_timeline_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    frame: std::ops::Range<usize>,
    expected: (u32, u64),
    source_ordinal: u32,
    type_guids_by_entity: &HashMap<u64, Vec<&DesignRelaxedGuidText>>,
) -> Result<Option<DesignFeatureTimeline>, CodecError> {
    let (_, expected_entity_id) = expected;
    let Some(head) = timeline_head(bytes, frame.start, expected) else {
        return Ok(None);
    };
    let Some(context_record_index) =
        local_reference(ctx, &head.context_reference, type_guids_by_entity)?
            .and_then(std::num::NonZeroU64::new)
    else {
        return Ok(None);
    };
    let mut at = head.item_count_offset;
    let Some(count) = View::u32_le_at(bytes, at).and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(next_at) = at.checked_add(4) else {
        return Ok(None);
    };
    at = next_at;
    let Some(remaining) = frame.end.checked_sub(at) else {
        return Ok(None);
    };
    if count > remaining / 11 {
        return Ok(None);
    }

    // The items grow as they are read, because a malformed item ends the
    // frame before its count.
    let mut items = Vec::new();
    for _ in 0..count {
        ctx.charge_work(1, "read F3D timeline items")?;
        let Some(target_offset) = at.checked_add(1) else {
            return Ok(None);
        };
        let Some(reference) = take_reference(bytes, &mut at) else {
            return Ok(None);
        };
        let Some(target) = local_reference(ctx, &reference, type_guids_by_entity)? else {
            return Ok(None);
        };
        let Some(target_offset) = u64::try_from(target_offset).ok() else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut items,
            crate::records::identity::Located {
                value: target,
                offset: target_offset,
            },
            "admit F3D timeline item slots",
        )?;
    }
    if at != frame.end {
        return Ok(None);
    }

    let Some(frame_start) = u64::try_from(frame.start).ok() else {
        return Ok(None);
    };
    let Some(frame_length) = frame
        .end
        .checked_sub(frame.start)
        .and_then(|value| u64::try_from(value).ok())
    else {
        return Ok(None);
    };
    let Some(context_reference_offset) = u64::try_from(head.context_reference_offset).ok() else {
        return Ok(None);
    };
    let Some(item_count_offset) = u64::try_from(head.item_count_offset).ok() else {
        return Ok(None);
    };
    let frame = match crate::records::entity_header::DesignTimelineFrame::new(
        crate::records::admission::RecordAdmission::Charged(ctx),
        frame_start,
        frame_length,
        context_reference_offset,
        item_count_offset,
        items,
    ) {
        Ok(frame) => frame,
        Err(crate::records::entity_header::DesignTimelineFrameError::Resource(error)) => {
            return Err(error);
        }
        Err(crate::records::entity_header::DesignTimelineFrameError::Invalid(_)) => {
            return Ok(None);
        }
    };
    let class_tag = retain_class_tag(ctx, *head.class_tag, "copy F3D class tag")?;
    let Some(record_index) = std::num::NonZeroU64::new(expected_entity_id) else {
        return Ok(None);
    };
    let id = design_record_id_charged(
        ctx,
        stream,
        ":design-feature-timeline#",
        frame_start,
        "retain F3D timeline identity",
    )?;
    Ok(DesignFeatureTimeline::try_new(
        id,
        frame,
        class_tag,
        record_index,
        source_ordinal,
        context_record_index,
    )
    .ok())
}

/// Whether `meta` registers feature-timeline types. Every one must carry a
/// supported version and the exact registration metadata.
fn has_feature_timeline_types(
    ctx: &DecodeContext<'_>,
    meta: &MetaStream,
) -> Result<bool, CodecError> {
    let mut found = false;
    let mut unsupported_version = false;
    let mut incompatible = false;
    for design_type in ctx.admit_iter(&meta.types, "validate F3D feature-timeline types")? {
        if !is_feature_timeline_type(design_type) {
            continue;
        }
        found = true;
        if !FEATURE_TIMELINE_TYPE_VERSIONS.contains(&design_type.version) {
            unsupported_version = true;
        } else if !is_supported_feature_timeline_type(design_type) {
            incompatible = true;
        }
    }
    if unsupported_version {
        return Err(CodecError::NotImplemented(
            "unsupported Design feature-timeline record version".into(),
        ));
    }
    if incompatible {
        return Err(CodecError::Malformed(
            "Design feature-timeline type has incompatible registration metadata".into(),
        ));
    }
    Ok(found)
}

/// Decode the exact counted scope list that carries authored feature order.
pub(crate) fn decode_feature_timelines(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignFeatureTimeline>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx.admit_iter(&scan.entries, "scan F3D feature-timeline MetaStreams")? {
        let Some(prefix) = design_meta_prefix(scan, entry) else {
            continue;
        };
        let meta = scan.parsed_metastream(ctx, &entry.name)?;
        if !has_feature_timeline_types(ctx, &meta)? {
            continue;
        }
        let Some(record_pair_width) = std::num::NonZeroUsize::new(2) else {
            return Err(CodecError::malformed(
                "F3D MetaStream record pair width is zero",
            ));
        };
        if ctx
            .admit_iter(&meta.records, "validate F3D MetaStream record offsets")?
            .windows(record_pair_width)
            .any(|pair| pair[0].bulk_offset >= pair[1].bulk_offset)
        {
            return Err(CodecError::Malformed(
                "Design MetaStream record offsets are not strictly increasing".into(),
            ));
        }
        let bulk_name = paired_bulk_entry_name(ctx, scan, prefix)?;
        let bytes = scan.entry_bytes(bulk_name)?;
        let mut index_reservation = ctx.reserve_scoped(0, "index F3D timeline types")?;
        let mut type_guids_by_entity = HashMap::<u64, Vec<&DesignRelaxedGuidText>>::new();
        for design_type in ctx.admit_iter(&meta.types, "index F3D timeline types")? {
            for entity_id in
                admit_reference_values(ctx, &design_type.entities, "index F3D timeline entities")?
            {
                index_reservation.with_storage(|| {
                    ctx.push_hash_group(
                        &mut type_guids_by_entity,
                        *entity_id,
                        &design_type.type_guid,
                        "index F3D timeline entity",
                        "index F3D timeline type GUID",
                    )
                })?;
            }
        }
        let mut source_ordinal = 0_u32;
        for (type_ordinal, design_type) in ctx
            .admit_iter(&meta.types, "scan F3D feature-timeline types")?
            .enumerate()
        {
            if !is_feature_timeline_type(design_type) {
                continue;
            }
            let expected_class_code = u32::try_from(type_ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_add(256))
                .filter(|class_code| *class_code <= 999)
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Design feature-timeline class tag is not three digits".into(),
                    )
                })?;
            for &entity_id in admit_reference_values(
                ctx,
                &design_type.entities,
                "scan F3D feature-timeline entities",
            )? {
                let entity_source_ordinal = source_ordinal;
                source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                    CodecError::Malformed("Design feature-timeline ordinal exceeds u32".into())
                })?;
                let mut matches = ctx
                    .admit_iter(&meta.records, "find F3D timeline primary record")?
                    .enumerate()
                    .filter(|(_, record)| record.entity_id == entity_id);
                let Some((record_ordinal, record)) = matches.next() else {
                    return Err(CodecError::Malformed(
                        "Design feature timeline has no unique primary record-index entry".into(),
                    ));
                };
                if matches.next().is_some() {
                    return Err(CodecError::Malformed(
                        "Design feature timeline has no unique primary record-index entry".into(),
                    ));
                }
                let start = usize::try_from(record.bulk_offset).map_err(|_| {
                    CodecError::Malformed("Design feature-timeline offset exceeds usize".into())
                })?;
                let end = meta.records.get(record_ordinal + 1).map_or_else(
                    || Ok(bytes.len()),
                    |next| {
                        usize::try_from(next.bulk_offset).map_err(|_| {
                            CodecError::Malformed(
                                "Design feature-timeline end exceeds usize".into(),
                            )
                        })
                    },
                )?;
                if start >= end || end > bytes.len() {
                    return Err(CodecError::Malformed(
                        "Design feature-timeline record extent is outside its BulkStream".into(),
                    ));
                }
                let timeline = parse_feature_timeline_record(
                    ctx,
                    bytes,
                    bulk_name,
                    start..end,
                    (expected_class_code, entity_id),
                    entity_source_ordinal,
                    &type_guids_by_entity,
                )?
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Design feature-timeline record does not match its exact frame".into(),
                    )
                })?;
                ctx.push_vec(&mut out, timeline, "retain F3D feature timeline")?;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
