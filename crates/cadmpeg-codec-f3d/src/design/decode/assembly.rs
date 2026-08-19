// SPDX-License-Identifier: Apache-2.0
//! Parse exact legacy As-built assembly alignment frames.

use crate::layout::assembly_as_built_421_frame_297 as as_built_421_frame_297;
use crate::layout::assembly_as_built_421_frame_327 as as_built_421_frame_327;
use crate::layout::assembly_as_built_421_frame_376 as as_built_421_frame_376;
use crate::layout::assembly_as_built_421_frame_448 as as_built_421_frame_448;
use crate::layout::assembly_as_built_421_scope as as_built_421;
use crate::records::{
    DesignAssemblyLimits, DesignAssemblySolvedFrame, DesignParameterOwner, DesignParameterScope,
};
use cadmpeg_core::decode::View;

use super::scopes::{exact_indexed_header_at, marked_record_reference, rigid_transform_at};
use super::sketch::IndexedRecordOffsets;

pub(crate) struct LegacyAsBuilt421Alignment {
    pub(crate) angle: f64,
    pub(crate) offset: [f64; 3],
    pub(crate) owner_record_indices: Vec<u32>,
    pub(crate) value_offsets: Vec<u64>,
    pub(crate) limits: DesignAssemblyLimits,
}

pub(crate) fn exact_legacy_as_built_421_alignment(
    bytes: &[u8],
    scope: &DesignParameterScope,
    lanes: &[&DesignParameterOwner],
) -> Option<LegacyAsBuilt421Alignment> {
    let generation = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    )?;
    if scope.kind != "As-built" || lanes.len() != 6 || scope.reference_members.len() != 11 {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if usize::try_from(scope.paired_byte_offset).ok()? != start.checked_add(as_built_421::LEN)?
        || scope.reference_count_offset
            != u64::try_from(start.checked_add(as_built_421::REFERENCE_COUNT)?).ok()?
        || View::u32_le_at(bytes, start.checked_add(as_built_421::REFERENCE_COUNT)?)?
            != as_built_421::REFERENCE_COUNT_VALUE
        || View::u32_le_at(bytes, start.checked_add(as_built_421::KIND_LENGTH)?)?
            != as_built_421::KIND_LENGTH_VALUE
        || scope.feature_ordinal_offset
            != u64::try_from(start.checked_add(as_built_421::FEATURE_ORDINAL)?).ok()?
    {
        return None;
    }
    for (ordinal, record_index) in scope.reference_members.iter().enumerate() {
        let reference_at = start
            .checked_add(as_built_421::REFERENCE_ENTRIES.checked_add(ordinal.checked_mul(11)?)?)?;
        if marked_record_reference(bytes, reference_at)? != *record_index
            || bytes.get(reference_at.checked_add(5)?..reference_at.checked_add(11)?)? != [0; 6]
            || scope.reference_member_offsets.get(ordinal).copied()
                != Some(u64::try_from(reference_at.checked_add(1)?).ok()?)
        {
            return None;
        }
    }
    if bytes.get(
        start.checked_add(as_built_421::KIND_LENGTH)?..start.checked_add(as_built_421::KIND)?,
    )? != as_built_421::KIND_LENGTH_VALUE.to_le_bytes()
        || bytes.get(
            start.checked_add(as_built_421::REFERENCE_TRAILER)?
                ..start.checked_add(as_built_421::KIND_LENGTH)?,
        )? != as_built_421::REFERENCE_TRAILER_VALUE
    {
        return None;
    }
    if lanes
        .iter()
        .any(|owner| owner.class_tag != generation.owner_class_tag() || owner.frame_length != 103)
    {
        return None;
    }
    let [offset_x, offset_y, offset_z, angle, limit_first, limit_second] = lanes else {
        return None;
    };
    let alignment_owner_record_indices = [
        offset_x.record_index,
        offset_y.record_index,
        offset_z.record_index,
        angle.record_index,
    ];
    let source_limit_owner_record_indices = [limit_first.record_index, limit_second.record_index];
    if scope.reference_members.get(4..8) != Some(alignment_owner_record_indices.as_slice())
        || scope.reference_members.get(9..11) != Some(source_limit_owner_record_indices.as_slice())
    {
        return None;
    }
    let (minimum_owner, maximum_owner) = if generation.reverse_limit_order() {
        (limit_second, limit_first)
    } else {
        (limit_first, limit_second)
    };
    let kind = generation.limit_kind();
    let minimum = minimum_owner.evaluated_value;
    let maximum = maximum_owner.evaluated_value;
    let limit_owner_record_indices = [minimum_owner.record_index, maximum_owner.record_index];
    let limit_value_offsets = [
        minimum_owner.evaluated_value_offset,
        maximum_owner.evaluated_value_offset,
    ];
    if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
        return None;
    }
    Some(LegacyAsBuilt421Alignment {
        angle: angle.evaluated_value,
        offset: [
            offset_x.evaluated_value,
            offset_y.evaluated_value,
            offset_z.evaluated_value,
        ],
        owner_record_indices: vec![
            angle.record_index,
            offset_x.record_index,
            offset_y.record_index,
            offset_z.record_index,
        ],
        value_offsets: vec![
            angle.evaluated_value_offset,
            offset_x.evaluated_value_offset,
            offset_y.evaluated_value_offset,
            offset_z.evaluated_value_offset,
        ],
        limits: DesignAssemblyLimits {
            kind,
            minimum,
            maximum,
            owner_record_indices: limit_owner_record_indices,
            value_offsets: limit_value_offsets,
        },
    })
}

pub(crate) fn exact_legacy_as_built_421_solved_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignAssemblySolvedFrame> {
    let generation = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    )?;
    if scope.kind != "As-built"
        || scope.reference_members.len() != 11
        || scope.reference_member_offsets.len() != 11
    {
        return None;
    }
    let frame_record_index = *scope.reference_members.get(8)?;
    let expected_class_tag = generation.frame_class_tag();
    let mut frame_candidates =
        records
            .offsets(frame_record_index)
            .iter()
            .copied()
            .filter(|frame_start| {
                exact_indexed_header_at(bytes, *frame_start, frame_record_index).as_deref()
                    == Some(expected_class_tag)
            });
    let frame_start = frame_candidates.next()?;
    if frame_candidates.next().is_some() {
        return None;
    }
    let (frame_length, matrix_prefix, transform_offset, matrix_prefix_value) = match generation {
        crate::design::assembly::LegacyAsBuilt421Generation::Class364 => (
            as_built_421_frame_376::LEN,
            as_built_421_frame_376::MATRIX_PREFIX,
            as_built_421_frame_376::MATRIX,
            as_built_421_frame_376::MATRIX_PREFIX_VALUE,
        ),
        crate::design::assembly::LegacyAsBuilt421Generation::Class420 => (
            as_built_421_frame_327::LEN,
            as_built_421_frame_327::MATRIX_PREFIX,
            as_built_421_frame_327::MATRIX,
            as_built_421_frame_327::MATRIX_PREFIX_VALUE,
        ),
        crate::design::assembly::LegacyAsBuilt421Generation::Class417 => (
            as_built_421_frame_448::LEN,
            as_built_421_frame_448::MATRIX_PREFIX,
            as_built_421_frame_448::MATRIX,
            as_built_421_frame_448::MATRIX_PREFIX_VALUE,
        ),
        crate::design::assembly::LegacyAsBuilt421Generation::Class457 => (
            as_built_421_frame_297::LEN,
            as_built_421_frame_297::MATRIX_PREFIX,
            as_built_421_frame_297::MATRIX,
            as_built_421_frame_297::MATRIX_PREFIX_VALUE,
        ),
    };
    if frame_length != generation.frame_length()
        || matrix_prefix != generation.matrix_prefix()
        || transform_offset != generation.matrix_offset()
    {
        return None;
    }
    if exact_indexed_header_at(
        bytes,
        frame_start.checked_add(frame_length)?,
        frame_record_index,
    )
    .as_deref()
        != Some(generation.frame_paired_class_tag())
    {
        return None;
    }
    if bytes
        .get(frame_start.checked_add(matrix_prefix)?..frame_start.checked_add(transform_offset)?)?
        != matrix_prefix_value
    {
        return None;
    }
    let transform_at = frame_start.checked_add(transform_offset)?;
    Some(DesignAssemblySolvedFrame {
        reference_record_index: frame_record_index,
        reference_offset: scope.reference_member_offsets[8],
        record_byte_offset: u64::try_from(frame_start).ok()?,
        class_tag: expected_class_tag.into(),
        transform: rigid_transform_at(bytes, transform_at)?,
        transform_offset: u64::try_from(transform_at).ok()?,
    })
}
