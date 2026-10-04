// SPDX-License-Identifier: Apache-2.0
//! Exact legacy class 383, 388 and 412 assembly operand paths.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::retain_class_tag;
use crate::design::decode::text::{
    fixed_guid_ascii, fixed_guid_end, fixed_relaxed_guid_text, fixed_utf16_ascii_eq,
};
use crate::layout::assembly_class_383_258_frame_359_identity as class_383_identity;
use crate::layout::assembly_class_383_258_frame_378_carrier as class_383_carrier;
use crate::layout::assembly_class_383_258_frame_387_child as class_383_child;
use crate::layout::assembly_class_383_258_frame_387_leading as class_383_leading;
use crate::layout::assembly_class_383_258_frame_394 as class_383_face;
use crate::layout::assembly_class_383_258_scope_1011 as class_383_scope;
use crate::layout::assembly_class_388_266_scope_968 as class_388_assemble;
use crate::layout::assembly_legacy_class_369_path_wrapper_one as class_369_wrapper_one;
use crate::layout::assembly_legacy_class_369_path_wrapper_two as class_369_wrapper_two;
use crate::layout::assembly_legacy_class_412_path_425 as class_412_path;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

/// Reference members of a class-388 `Assemble` scope.
const CLASS_388_REFERENCE_COUNT: usize = index_from_u32(class_388_assemble::REFERENCE_COUNT_VALUE);

/// Reference members of a class-383 `Assemble` scope.
const CLASS_383_REFERENCE_COUNT: usize = 38;

pub(super) fn exact_legacy_class_388_scope(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<()>, CodecError> {
    if scope.class_tag.as_str() != "388"
        || scope.paired_class_tag.as_str() != "266"
        || scope.frame_length() != u64_from_index(class_388_assemble::LEN)
    {
        return Ok(None);
    }
    let (Some(members), Ok(start), Ok(paired)) = (
        scope
            .reference_members()
            .values_array::<CLASS_388_REFERENCE_COUNT>(),
        usize::try_from(scope.byte_offset()),
        usize::try_from(scope.paired_byte_offset()),
    ) else {
        return Ok(None);
    };
    if paired != start + class_388_assemble::LEN || !class_388_scope_layout(bytes, start) {
        return Ok(None);
    }
    let identity_at = start + class_388_assemble::COMPONENT_IDENTITY;
    if fixed_guid_end(ctx, bytes, identity_at)? != Some(identity_at + 76) {
        return Ok(None);
    }
    if fixed_utf16_ascii_eq(
        ctx,
        bytes,
        start + class_388_assemble::KIND_CODE_UNIT_COUNT,
        "Assemble",
    )? != Some(start + class_388_assemble::FEATURE_ORDINAL)
        || View::u32_le_at(bytes, start + class_388_assemble::FEATURE_ORDINAL)
            != Some(scope.feature_ordinal.get())
    {
        return Ok(None);
    }
    let entries_match = members
        .into_iter()
        .enumerate()
        .all(|(ordinal, record_index)| {
            let at = start
                + class_388_assemble::REFERENCE_ENTRIES
                + ordinal * ASSEMBLY_MARKED_REFERENCE_LEN;
            marked_record_reference(bytes, at) == Some(*record_index)
        });
    Ok(entries_match.then_some(()))
}

/// The fixed fields of a class-388 scope frame at `start`: zero runs, flags,
/// the two operand-path locator references and the external component
/// reference.
fn class_388_scope_layout(bytes: &[u8], start: usize) -> bool {
    let locators = [
        start + class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES,
        start + class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES + 11,
    ]
    .map(|at| marked_record_reference(bytes, at));
    zeros_at::<{ class_388_assemble::SCOPE_FLAGS - 11 }>(bytes, start + 11)
        && bytes_at::<6>(bytes, start + class_388_assemble::SCOPE_FLAGS)
            == Some(&class_388_assemble::SCOPE_FLAGS_VALUE)
        && zeros_at::<{ class_388_assemble::FIRST_OPERAND_REFERENCE - 26 }>(bytes, start + 26)
        && bytes.get(start + 39) == Some(&0)
        && bytes.get(start + 179) == Some(&0)
        && View::u32_le_at(
            bytes,
            start + class_388_assemble::OPERAND_PATH_LOCATOR_COUNT,
        ) == Some(class_388_assemble::OPERAND_PATH_LOCATOR_COUNT_VALUE)
        && bytes_at::<4>(bytes, start + class_388_assemble::REFERENCE_TRAILER)
            == Some(&class_388_assemble::REFERENCE_TRAILER_VALUE)
        && View::u32_le_at(bytes, start + class_388_assemble::KIND_CODE_UNIT_COUNT)
            == Some(class_388_assemble::KIND_CODE_UNIT_COUNT_VALUE)
        && matches!(
            locators,
            [Some(first), Some(second)] if first != 0 && second != 0 && first != second
        )
        && marked_record_reference(
            bytes,
            start + class_388_assemble::EXTERNAL_COMPONENT_REFERENCE,
        )
        .is_some_and(|component| component != 0)
}

pub(super) const ASSEMBLY_MARKED_REFERENCE_LEN: usize = 11;

pub(super) const CLASS_388_OWNER_REFERENCE_ORDINALS: [usize; 28] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 30, 31,
    32, 33,
];

pub(super) const CLASS_383_OWNER_REFERENCE_ORDINALS: [usize; 20] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 20, 21, 22, 23, 33, 34, 35, 36,
];

#[derive(Clone, Copy)]
struct LegacyClass383OperandSpec {
    leading_ordinal: usize,
    leading_identity_ordinal: usize,
    child_ordinal: usize,
    child_identity_ordinal: usize,
    first_face_ordinal: usize,
    first_face_identity_ordinal: usize,
    second_face_ordinal: usize,
    second_face_identity_ordinal: usize,
    placement_owner_start: usize,
    carrier_ordinal: usize,
    scope_operand_reference_offset: usize,
}

const CLASS_383_OPERAND_SPECS: [LegacyClass383OperandSpec; 2] = [
    LegacyClass383OperandSpec {
        leading_ordinal: 12,
        leading_identity_ordinal: 13,
        child_ordinal: 14,
        child_identity_ordinal: 15,
        first_face_ordinal: 16,
        first_face_identity_ordinal: 17,
        second_face_ordinal: 18,
        second_face_identity_ordinal: 19,
        placement_owner_start: 20,
        carrier_ordinal: 24,
        scope_operand_reference_offset: class_383_scope::FIRST_OPERAND_REFERENCE,
    },
    LegacyClass383OperandSpec {
        leading_ordinal: 25,
        leading_identity_ordinal: 26,
        child_ordinal: 27,
        child_identity_ordinal: 28,
        first_face_ordinal: 29,
        first_face_identity_ordinal: 30,
        second_face_ordinal: 31,
        second_face_identity_ordinal: 32,
        placement_owner_start: 33,
        carrier_ordinal: 37,
        scope_operand_reference_offset: class_383_scope::SECOND_OPERAND_REFERENCE,
    },
];

pub(super) fn exact_legacy_class_383_operand_paths(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frames: &[DesignAssemblyOperandFrame; 2],
) -> Result<Option<[DesignAssemblyOperandPath; 2]>, CodecError> {
    if !crate::design::assembly::legacy_class_383_258_scope(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    ) {
        return Ok(None);
    }
    let Some(members) = scope
        .reference_members()
        .values_array::<CLASS_383_REFERENCE_COUNT>()
    else {
        return Ok(None);
    };
    let members = members.map(|member| *member);
    let [first_spec, second_spec] = CLASS_383_OPERAND_SPECS;
    let Some(first) = exact_legacy_class_383_operand_path(
        ctx, bytes, records, scope, &members, &frames[0], first_spec,
    )?
    else {
        return Ok(None);
    };
    let Some(second) = exact_legacy_class_383_operand_path(
        ctx,
        bytes,
        records,
        scope,
        &members,
        &frames[1],
        second_spec,
    )?
    else {
        return Ok(None);
    };
    Ok(Some([first, second]))
}

fn exact_legacy_class_383_operand_path(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    members: &[u32; CLASS_383_REFERENCE_COUNT],
    frame: &DesignAssemblyOperandFrame,
    spec: LegacyClass383OperandSpec,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    let member = |ordinal: usize| members.get(ordinal).copied();
    let (
        Some(leading_record_index),
        Some(leading_identity_record_index),
        Some(child_record_index),
        Some(child_identity_record_index),
        Some(first_face_record_index),
        Some(first_face_identity_record_index),
        Some(second_face_record_index),
        Some(second_face_identity_record_index),
        Some(carrier_record_index),
    ) = (
        member(spec.leading_ordinal),
        member(spec.leading_identity_ordinal),
        member(spec.child_ordinal),
        member(spec.child_identity_ordinal),
        member(spec.first_face_ordinal),
        member(spec.first_face_identity_ordinal),
        member(spec.second_face_ordinal),
        member(spec.second_face_identity_ordinal),
        member(spec.carrier_ordinal),
    )
    else {
        return Ok(None);
    };
    let Some(placement_owners) = members
        .get(spec.placement_owner_start..)
        .and_then(<[u32]>::first_chunk::<4>)
    else {
        return Ok(None);
    };
    let record_frame = |record_index, class_tag, frame_length| {
        exact_legacy_class_383_record_frame(
            ctx,
            bytes,
            records,
            record_index,
            class_tag,
            frame_length,
        )
    };
    let Some((leading_at, leading_paired_at)) =
        record_frame(leading_record_index, b"387", class_383_leading::LEN)?
    else {
        return Ok(None);
    };
    let Some((leading_identity_at, _)) = record_frame(
        leading_identity_record_index,
        b"359",
        class_383_identity::LEN,
    )?
    else {
        return Ok(None);
    };
    let Some((child_at, child_paired_at)) =
        record_frame(child_record_index, b"387", class_383_child::LEN)?
    else {
        return Ok(None);
    };
    let Some((child_identity_at, _)) =
        record_frame(child_identity_record_index, b"359", class_383_identity::LEN)?
    else {
        return Ok(None);
    };
    let Some((first_face_at, _)) =
        record_frame(first_face_record_index, b"394", class_383_face::LEN)?
    else {
        return Ok(None);
    };
    let Some((first_face_identity_at, _)) = record_frame(
        first_face_identity_record_index,
        b"359",
        class_383_identity::LEN,
    )?
    else {
        return Ok(None);
    };
    let Some((second_face_at, _)) =
        record_frame(second_face_record_index, b"394", class_383_face::LEN)?
    else {
        return Ok(None);
    };
    let Some((second_face_identity_at, _)) = record_frame(
        second_face_identity_record_index,
        b"359",
        class_383_identity::LEN,
    )?
    else {
        return Ok(None);
    };
    let Some((carrier_at, carrier_paired_at)) =
        record_frame(carrier_record_index, b"378", class_383_carrier::LEN)?
    else {
        return Ok(None);
    };
    let marked = |at: usize, offset: usize| marked_record_reference(bytes, at + offset);
    let structural_checks = [
        leading_paired_at == leading_at + class_383_leading::LEN,
        child_paired_at == child_at + class_383_child::LEN,
        marked(leading_at, class_383_leading::IDENTITY_REFERENCE)
            == Some(leading_identity_record_index),
        marked(leading_at, class_383_leading::SCOPE_REFERENCE) == Some(scope.record_index),
        marked(child_at, class_383_child::IDENTITY_REFERENCE) == Some(child_identity_record_index),
        marked(child_at, class_383_child::LEADING_REFERENCE) == Some(leading_record_index),
        marked(child_at, class_383_child::SCOPE_REFERENCE) == Some(scope.record_index),
        marked(first_face_at, class_383_face::IDENTITY_REFERENCE)
            == Some(first_face_identity_record_index),
        marked(first_face_at, class_383_face::SCOPE_REFERENCE) == Some(scope.record_index),
        marked(second_face_at, class_383_face::IDENTITY_REFERENCE)
            == Some(second_face_identity_record_index),
        marked(second_face_at, class_383_face::SCOPE_REFERENCE) == Some(scope.record_index),
        marked(leading_identity_at, class_383_identity::SCOPE_REFERENCE)
            == Some(scope.record_index),
        marked(child_identity_at, class_383_identity::SCOPE_REFERENCE) == Some(scope.record_index),
        marked(first_face_identity_at, class_383_identity::SCOPE_REFERENCE)
            == Some(scope.record_index),
        marked(second_face_identity_at, class_383_identity::SCOPE_REFERENCE)
            == Some(scope.record_index),
        marked(carrier_at, class_383_carrier::CHILD_REFERENCE) == Some(child_record_index),
        marked(carrier_at, class_383_carrier::SECOND_FACE_REFERENCE)
            == Some(second_face_record_index),
        marked(carrier_at, class_383_carrier::FIRST_FACE_REFERENCE)
            == Some(first_face_record_index),
        marked(carrier_at, class_383_carrier::REPEATED_CHILD_REFERENCE) == Some(child_record_index),
        marked(carrier_at, class_383_carrier::REPEATED_FIRST_FACE_REFERENCE)
            == Some(first_face_record_index),
        marked(
            carrier_at,
            class_383_carrier::REPEATED_SECOND_FACE_REFERENCE,
        ) == Some(second_face_record_index),
        marked(carrier_at, class_383_carrier::SCOPE_REFERENCE) == Some(scope.record_index),
        carrier_paired_at == carrier_at + class_383_carrier::LEN,
        rigid_transform_at(bytes, carrier_at + class_383_carrier::TRANSFORM)
            .is_some_and(|transform| transform == frame.transform),
    ];
    if structural_checks.contains(&false)
        || placement_owners
            .iter()
            .enumerate()
            .any(|(ordinal, record_index)| {
                marked(
                    carrier_at,
                    class_383_carrier::PLACEMENT_OWNER_REFERENCES
                        + ordinal * ASSEMBLY_MARKED_REFERENCE_LEN,
                ) != Some(*record_index)
            })
    {
        return Ok(None);
    }
    // Every identity record of the operand repeats the leading identity's
    // occurrence and identity GUIDs.
    let Some(leading_guids) = class_383_identity_guid_units(ctx, bytes, leading_identity_at)?
    else {
        return Ok(None);
    };
    for identity_at in [
        child_identity_at,
        first_face_identity_at,
        second_face_identity_at,
    ] {
        if class_383_identity_guid_units(ctx, bytes, identity_at)? != Some(leading_guids) {
            return Ok(None);
        }
    }
    let scope_at = usize::try_from(scope.byte_offset()).ok();
    let locator = scope_at
        .and_then(|at| at.checked_add(spec.scope_operand_reference_offset))
        .and_then(|at| exact_same_segment_record_reference(bytes, at));
    let Some((locator_record_index, locator_reference_offset)) =
        locator.filter(|(record_index, _)| *record_index == carrier_record_index)
    else {
        return Ok(None);
    };
    let Some((_, locator_scope_reference_offset)) =
        exact_same_segment_record_reference(bytes, carrier_at + class_383_carrier::SCOPE_REFERENCE)
            .filter(|(record_index, _)| *record_index == scope.record_index)
    else {
        return Ok(None);
    };
    let Some((_, wrapper_reference_offset)) = exact_same_segment_record_reference(
        bytes,
        leading_at + class_383_leading::IDENTITY_REFERENCE,
    ) else {
        return Ok(None);
    };
    let occurrence_guid_offset =
        u64_from_index(leading_identity_at + class_383_identity::OCCURRENCE_GUID + 4);
    let identity_guid_offset =
        u64_from_index(leading_identity_at + class_383_identity::IDENTITY_GUID + 4);
    let [occurrence_guid, identity_guid] = leading_guids;
    let operation = "copy F3D legacy operand path class tag";
    let link = DesignAssemblyOperandPathLink {
        locator_reference_offset,
        locator_record_index,
        locator_class_tag: retain_class_tag(ctx, b"378", operation)?,
        locator_byte_offset: u64_from_index(carrier_at),
        locator_scope_reference_offset,
        wrapper_record_index: leading_identity_record_index,
        wrapper_reference_offset,
        wrapper_class_tag: retain_class_tag(ctx, b"359", operation)?,
        wrapper_byte_offset: u64_from_index(leading_identity_at),
        path_reference_offset: occurrence_guid_offset,
    };
    let occurrence_guids = retained_guid_lane(
        ctx,
        &occurrence_guid,
        occurrence_guid_offset,
        "f3d legacy occurrence GUIDs",
    )?;
    let identity_guids = retained_guid_lane(
        ctx,
        &identity_guid,
        identity_guid_offset,
        "collect F3D legacy path identity GUIDs",
    )?;
    Ok(DesignAssemblyOperandPath::try_new(
        link,
        leading_identity_record_index,
        retain_class_tag(ctx, b"386", operation)?,
        u64_from_index(leading_identity_at),
        occurrence_guids,
        identity_guids,
    )
    .ok())
}

/// A one-GUID lane: the GUID's text copied into retained storage.
fn retained_guid_lane(
    ctx: &DecodeContext<'_>,
    guid: &[u8; 36],
    offset: u64,
    operation: &'static str,
) -> Result<Vec<Located<DesignRelaxedGuidText>>, CodecError> {
    let text = std::str::from_utf8(guid)
        .map_err(|_| CodecError::malformed("validated F3D relaxed GUID is not ASCII"))?;
    let value =
        DesignRelaxedGuidText::try_from(ctx.copy_retained_text(text, "retain F3D relaxed GUID")?)
            .map_err(CodecError::malformed)?;
    let mut lane = ctx.vector_storage(1, operation)?;
    ctx.push_vec(&mut lane, Located { value, offset }, operation)?;
    Ok(lane)
}

/// The only frame of `record_index` that spans `frame_length` bytes from a
/// `class_tag` header to a class-258 header.
fn exact_legacy_class_383_record_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &[u8; 3],
    frame_length: usize,
) -> Result<Option<(usize, usize)>, CodecError> {
    let mut candidate = None;
    for (start, paired_at) in records.frames(ctx, record_index)? {
        if Some(paired_at) == start.checked_add(frame_length)
            && exact_indexed_header_at(bytes, start, record_index) == Some(class_tag)
            && exact_indexed_header_at(bytes, paired_at, record_index) == Some(b"258")
            && candidate.replace((start, paired_at)).is_some()
        {
            return Ok(None);
        }
    }
    Ok(candidate)
}

/// The occurrence and identity GUIDs of the class-383 identity record at
/// `start`, validated in place without copying.
fn class_383_identity_guid_units(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<[[u8; 36]; 2]>, CodecError> {
    let occurrence_at = start + class_383_identity::OCCURRENCE_GUID;
    let identity_at = start + class_383_identity::IDENTITY_GUID;
    let Some((occurrence, after_occurrence)) = fixed_guid_ascii(ctx, bytes, occurrence_at)? else {
        return Ok(None);
    };
    if after_occurrence != identity_at {
        return Ok(None);
    }
    Ok(fixed_guid_ascii(ctx, bytes, identity_at)?.map(|(identity, _)| [occurrence, identity]))
}

/// Whether the first indexed-record header at or after `position` opens at
/// `expected`. The search reads no further than the end of that header, so a
/// missing header costs at most the distance to `expected`.
fn next_header_is(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
    expected: usize,
) -> Result<bool, CodecError> {
    let Some(window) = expected.checked_add(11).and_then(|end| bytes.get(..end)) else {
        return Ok(false);
    };
    Ok(next_indexed_record_offset(ctx, window, position)? == Some(expected))
}

/// A validated class-388 operand path: the locator frame, its path wrapper
/// and the class-412 path records between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LegacyClass388Envelope {
    locator_record_index: u32,
    locator_reference_offset: u64,
    locator_at: usize,
    locator_scope_reference_offset: u64,
    wrapper_record_index: u32,
    wrapper_reference_offset: u64,
    wrapper_at: usize,
    /// The header that closes the wrapper.
    wrapper_end: usize,
    /// The class-412 path records in order; the second is present for a
    /// two-path wrapper.
    paths: [Option<usize>; 2],
    final_path_reference_offset: u64,
}

pub(super) fn exact_legacy_class_388_operand_paths(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<[DesignAssemblyOperandPath; 2]>, CodecError> {
    if !matches!(
        crate::design::assembly::AssemblyScopeGeneration::new(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str()
        )
        .operand_frame_variant(),
        Some(crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388)
    ) || scope.reference_members().len() != CLASS_388_REFERENCE_COUNT
    {
        return Ok(None);
    }
    let (Ok(scope_at), Some(search_start)) = (
        usize::try_from(scope.byte_offset()),
        usize::try_from(scope.paired_byte_offset())
            .ok()
            .and_then(|paired| paired.checked_add(11)),
    ) else {
        return Ok(None);
    };
    let mut envelopes = [None, None];
    for (slot, relative_offset) in envelopes.iter_mut().zip([
        class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES,
        class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES + 11,
    ]) {
        let Some((locator_record_index, locator_reference_offset)) = scope_at
            .checked_add(relative_offset)
            .and_then(|at| exact_same_segment_record_reference(bytes, at))
        else {
            return Ok(None);
        };
        let offsets = records.offsets(locator_record_index);
        let first = ctx.partition_point(
            offsets,
            |offset| Ok(*offset < search_start),
            "find F3D legacy operand locator offsets",
        )?;
        let mut candidate = None;
        for locator_at in ctx.admit_iter(
            offsets.get(first..).unwrap_or_default(),
            "scan F3D legacy operand locator offsets",
        )? {
            let Some(envelope) = exact_legacy_class_388_envelope(
                ctx,
                bytes,
                records,
                scope,
                (locator_record_index, locator_reference_offset),
                *locator_at,
            )?
            else {
                continue;
            };
            if candidate.replace(envelope).is_some() {
                return Ok(None);
            }
        }
        *slot = candidate;
    }
    let [Some(first), Some(second)] = envelopes else {
        return Ok(None);
    };
    if first.locator_record_index == second.locator_record_index
        || first.wrapper_record_index == second.wrapper_record_index
        || (first.locator_at < second.wrapper_end && second.locator_at < first.wrapper_end)
    {
        return Ok(None);
    }
    let Some(first) = legacy_class_388_operand_path(ctx, bytes, &first)? else {
        return Ok(None);
    };
    let Some(second) = legacy_class_388_operand_path(ctx, bytes, &second)? else {
        return Ok(None);
    };
    Ok(Some([first, second]))
}

/// Validate the operand path whose locator frame opens at `locator_at`,
/// without copying any of its GUIDs.
fn exact_legacy_class_388_envelope(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    (locator_record_index, locator_reference_offset): (u32, u64),
    locator_at: usize,
) -> Result<Option<LegacyClass388Envelope>, CodecError> {
    let locator_end = locator_at + path_locator::LEN;
    let Some(locator) = legacy_path_locator(bytes, scope, locator_record_index, locator_at) else {
        return Ok(None);
    };
    if !next_header_is(ctx, bytes, locator_at + 1, locator_end)? {
        return Ok(None);
    }
    let (locator_scope_reference_offset, (wrapper_record_index, wrapper_reference_offset)) =
        locator;
    let wrapper_offsets = records.offsets(wrapper_record_index);
    let first_wrapper = ctx.partition_point(
        wrapper_offsets,
        |offset| Ok(*offset < locator_end),
        "find F3D legacy operand wrapper offsets",
    )?;
    let mut wrapper = None;
    for wrapper_at in ctx.admit_iter(
        wrapper_offsets.get(first_wrapper..).unwrap_or_default(),
        "scan F3D legacy operand wrapper offsets",
    )? {
        if exact_indexed_header_at(bytes, *wrapper_at, wrapper_record_index) == Some(b"369")
            && wrapper.replace(*wrapper_at).is_some()
        {
            return Ok(None);
        }
    }
    let Some(wrapper_at) = wrapper else {
        return Ok(None);
    };
    let Some((path_count, wrapper_length, path_reference_offset)) =
        legacy_path_wrapper(bytes, wrapper_at)
    else {
        return Ok(None);
    };
    let wrapper_end = wrapper_at + wrapper_length;
    if locator_record_index
        .checked_add(path_count)
        .and_then(|index| index.checked_add(1))
        != Some(wrapper_record_index)
        || !next_header_is(ctx, bytes, wrapper_at + 1, wrapper_end)?
    {
        return Ok(None);
    }
    // The locator's closing header opens the first path record, and each path
    // record closes at the header of the next one.
    let mut path_at = locator_end;
    let mut paths = [None; 2];
    let mut final_path_reference_offset = None;
    for (ordinal, slot) in paths
        .iter_mut()
        .enumerate()
        .take(index_from_u32(path_count))
    {
        let Some(path_record_index) = u32::try_from(ordinal + 1)
            .ok()
            .and_then(|delta| locator_record_index.checked_add(delta))
        else {
            return Ok(None);
        };
        let path_end = path_at + class_412_path::LEN;
        if exact_indexed_header_at(bytes, path_at, path_record_index) != Some(b"412")
            || !next_header_is(ctx, bytes, path_at + 1, path_end)?
            || !legacy_class_412_path_layout(ctx, bytes, path_at)?
        {
            return Ok(None);
        }
        let Some((referenced_path_record_index, reference_offset)) =
            exact_same_segment_record_reference(
                bytes,
                wrapper_at + path_reference_offset + ordinal * 11,
            )
        else {
            return Ok(None);
        };
        if referenced_path_record_index != path_record_index {
            return Ok(None);
        }
        *slot = Some(path_at);
        final_path_reference_offset = Some(reference_offset);
        path_at = path_end;
    }
    let Some(final_path_reference_offset) = final_path_reference_offset else {
        return Ok(None);
    };
    if path_at != wrapper_at {
        return Ok(None);
    }
    Ok(Some(LegacyClass388Envelope {
        locator_record_index,
        locator_reference_offset,
        locator_at,
        locator_scope_reference_offset,
        wrapper_record_index,
        wrapper_reference_offset,
        wrapper_at,
        wrapper_end,
        paths,
        final_path_reference_offset,
    }))
}

/// The fixed fields of the class-451 locator frame at `locator_at`: its
/// scope backlink offset and the wrapper it references.
fn legacy_path_locator(
    bytes: &[u8],
    scope: &DesignParameterScope,
    locator_record_index: u32,
    locator_at: usize,
) -> Option<(u64, (u32, u64))> {
    if exact_indexed_header_at(bytes, locator_at, locator_record_index) != Some(b"451")
        || !zeros_at::<{ path_locator::NONZERO_RECORD_REFERENCE - path_locator::ZERO_RUN_10 }>(
            bytes,
            locator_at + path_locator::ZERO_RUN_10,
        )
        || exact_same_segment_record_reference(
            bytes,
            locator_at + path_locator::NONZERO_RECORD_REFERENCE,
        )?
        .0 == 0
        || bytes.get(locator_at + path_locator::ZERO_32) != Some(&0)
        || rigid_transform_at(bytes, locator_at + path_locator::TRANSFORM).is_none()
        || bytes.get(locator_at + path_locator::ZERO_161) != Some(&0)
    {
        return None;
    }
    let (scope_record_index, locator_scope_reference_offset) =
        exact_same_segment_record_reference(bytes, locator_at + path_locator::SCOPE_BACKLINK)?;
    let wrapper =
        exact_same_segment_record_reference(bytes, locator_at + path_locator::WRAPPER_REFERENCE)?;
    if scope_record_index != scope.record_index
        || wrapper.0 == 0
        || View::u32_le_at(bytes, locator_at + path_locator::CONSTANT_TWO) != Some(2)
        || !zeros_at::<{ path_locator::LEN - path_locator::ZERO_TAIL_2 }>(
            bytes,
            locator_at + path_locator::ZERO_TAIL_2,
        )
    {
        return None;
    }
    Some((locator_scope_reference_offset, wrapper))
}

/// The path count, frame length and path-reference offset of the class-369
/// path wrapper at `wrapper_at`.
fn legacy_path_wrapper(bytes: &[u8], wrapper_at: usize) -> Option<(u32, usize, usize)> {
    if !zeros_at::<{ class_369_wrapper_one::WRAPPER_MARKER - 11 }>(bytes, wrapper_at + 11)
        || bytes.get(wrapper_at + class_369_wrapper_one::WRAPPER_MARKER) != Some(&1)
    {
        return None;
    }
    match View::u32_le_at(bytes, wrapper_at + class_369_wrapper_one::PATH_COUNT)? {
        class_369_wrapper_one::PATH_COUNT_VALUE => Some((
            class_369_wrapper_one::PATH_COUNT_VALUE,
            class_369_wrapper_one::LEN,
            class_369_wrapper_one::PATH_REFERENCE,
        )),
        class_369_wrapper_two::PATH_COUNT_VALUE => Some((
            class_369_wrapper_two::PATH_COUNT_VALUE,
            class_369_wrapper_two::LEN,
            class_369_wrapper_two::PATH_REFERENCES,
        )),
        _ => None,
    }
}

/// Offsets of the four identity GUIDs of a class-412 path record.
const CLASS_412_IDENTITY_GUIDS: [usize; 4] = [
    class_412_path::FIRST_IDENTITY_GUID,
    class_412_path::SECOND_IDENTITY_GUID,
    class_412_path::THIRD_IDENTITY_GUID,
    class_412_path::FOURTH_IDENTITY_GUID,
];

/// Whether the class-412 path record at `start` has its fixed layout: an
/// occurrence GUID, four identity GUIDs around a separator, and a zero tail.
/// Each GUID is a counted field of exactly 36 code units, so it ends where the
/// next member begins.
fn legacy_class_412_path_layout(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<bool, CodecError> {
    if !zeros_at::<{ class_412_path::PATH_MARKER - 11 }>(bytes, start + 11)
        || bytes.get(start + class_412_path::PATH_MARKER)
            != Some(&class_412_path::PATH_MARKER_VALUE)
        || !zeros_at::<{ class_412_path::OCCURRENCE_GUID - class_412_path::PATH_MARKER - 1 }>(
            bytes,
            start + class_412_path::PATH_MARKER + 1,
        )
        || View::u64_le_at(bytes, start + class_412_path::IDENTITY_SEPARATOR)
            != Some(class_412_path::IDENTITY_SEPARATOR_VALUE)
        || View::u32_le_at(bytes, start + class_412_path::PATH_TAIL_COUNT)
            != Some(class_412_path::PATH_TAIL_COUNT_VALUE)
        || !zeros_at::<{ class_412_path::LEN - class_412_path::PATH_TAIL_COUNT - 4 }>(
            bytes,
            start + class_412_path::PATH_TAIL_COUNT + 4,
        )
    {
        return Ok(false);
    }
    for relative_offset in [class_412_path::OCCURRENCE_GUID]
        .into_iter()
        .chain(CLASS_412_IDENTITY_GUIDS)
    {
        if fixed_guid_end(ctx, bytes, start + relative_offset)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The four identity GUIDs of the validated class-412 path record at `start`,
/// copied into retained storage.
fn legacy_class_412_identity_guids(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<Vec<Located<DesignRelaxedGuidText>>>, CodecError> {
    let operation = "collect F3D legacy path identity GUIDs";
    let mut identity_guids = ctx.vector_storage(CLASS_412_IDENTITY_GUIDS.len(), operation)?;
    for relative_offset in CLASS_412_IDENTITY_GUIDS {
        let identity_at = start + relative_offset;
        let Some((value, _)) = fixed_relaxed_guid_text(ctx, bytes, identity_at)? else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut identity_guids,
            Located {
                value,
                offset: u64_from_index(identity_at + 4),
            },
            operation,
        )?;
    }
    Ok(Some(identity_guids))
}

/// The operand path of a validated envelope: the occurrence GUID of every
/// path record and the identity GUIDs of the final one, copied into retained
/// storage.
fn legacy_class_388_operand_path(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    envelope: &LegacyClass388Envelope,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    let [Some(first_path_at), second_path_at] = envelope.paths else {
        return Ok(None);
    };
    let final_path_at = second_path_at.unwrap_or(first_path_at);
    let path_count: u32 = if second_path_at.is_some() { 2 } else { 1 };
    let mut occurrence_guids = ctx.vector_storage(
        index_from_u32(path_count),
        "f3d legacy occurrence GUIDs",
    )?;
    for path_at in envelope.paths.into_iter().flatten() {
        let occurrence_at = path_at + class_412_path::OCCURRENCE_GUID;
        let Some((value, _)) = fixed_relaxed_guid_text(ctx, bytes, occurrence_at)? else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut occurrence_guids,
            Located {
                value,
                offset: u64_from_index(occurrence_at + 4),
            },
            "f3d legacy occurrence GUIDs",
        )?;
    }
    let Some(identity_guids) = legacy_class_412_identity_guids(ctx, bytes, final_path_at)? else {
        return Ok(None);
    };
    let Some(final_record_index) = envelope.locator_record_index.checked_add(path_count) else {
        return Ok(None);
    };
    let operation = "copy F3D legacy operand path class tag";
    Ok(DesignAssemblyOperandPath::try_new(
        DesignAssemblyOperandPathLink {
            locator_reference_offset: envelope.locator_reference_offset,
            locator_record_index: envelope.locator_record_index,
            locator_class_tag: retain_class_tag(ctx, b"451", operation)?,
            locator_byte_offset: u64_from_index(envelope.locator_at),
            locator_scope_reference_offset: envelope.locator_scope_reference_offset,
            wrapper_record_index: envelope.wrapper_record_index,
            wrapper_reference_offset: envelope.wrapper_reference_offset,
            wrapper_class_tag: retain_class_tag(ctx, b"369", operation)?,
            wrapper_byte_offset: u64_from_index(envelope.wrapper_at),
            path_reference_offset: envelope.final_path_reference_offset,
        },
        final_record_index,
        retain_class_tag(ctx, b"412", operation)?,
        u64_from_index(final_path_at),
        occurrence_guids,
        identity_guids,
    )
    .ok())
}

#[cfg(test)]
mod tests;
