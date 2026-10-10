// SPDX-License-Identifier: Apache-2.0
//! Parse the legacy class-415 Extrude grammar variants.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use crate::bytes::f64s_at;
use crate::design::decode::byte_fields::zeros_at;
use crate::layout::legacy_class_415_one_sided_distance_extrude_prefix as distance;
use crate::layout::legacy_class_415_one_sided_to_face_extrude_prefix as to_face;
use crate::layout::legacy_class_415_symmetric_extrude_prefix as symmetric;
use crate::records::feature::extrude::{
    DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use cadmpeg_core::decode::View;

#[derive(Clone, Copy)]
enum OneSidedVariant {
    ToFace,
    Distance,
}

pub(crate) fn is_symmetric_distance_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "415"
        && paired_class_tag == "265"
        && reference_count_delta == u64_from_index(symmetric::REFERENCE_COUNT)
        && matches!((frame_length, reference_member_count), (447, 5) | (469, 7))
}

fn variant(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> Option<OneSidedVariant> {
    if class_tag != "415" || paired_class_tag != "265" {
        return None;
    }
    match (frame_length, reference_count_delta, reference_member_count) {
        (481, 278, 9) => Some(OneSidedVariant::ToFace),
        (449, 268, 7) => Some(OneSidedVariant::Distance),
        _ => None,
    }
}

pub(crate) fn is_one_sided_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    variant(
        class_tag,
        paired_class_tag,
        frame_length,
        reference_count_delta,
        reference_member_count,
    )
    .is_some()
}

/// The one-sided class-415 Extrude prologue. The variant fixes the reference
/// run at seven or nine members, so every membership and listing test reads at
/// most nine values.
pub(super) fn exact_one_sided_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;
    let frame_length = paired_at.checked_sub(start)?;
    let reference_count_delta = reference_count_at.checked_sub(start)?;
    let variant = variant(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_delta).ok()?,
        reference_members.len(),
    )?;
    let (
        expected_frame_length,
        reference_count_offset,
        reference_count,
        first_side_extent_offset,
        first_side_extent,
        face_extend,
        first_side_offset_reference,
    ) = match variant {
        OneSidedVariant::ToFace => (
            481,
            to_face::REFERENCE_COUNT,
            index_from_u32(to_face::REFERENCE_COUNT_VALUE),
            to_face::FIRST_SIDE_EXTENT,
            to_face::FIRST_SIDE_EXTENT_VALUE,
            to_face::FACE_EXTEND_VALUE,
            Some(to_face::FIRST_SIDE_OFFSET_REFERENCE),
        ),
        OneSidedVariant::Distance => (
            449,
            distance::REFERENCE_COUNT,
            index_from_u32(distance::REFERENCE_COUNT_VALUE),
            distance::FIRST_SIDE_EXTENT,
            distance::FIRST_SIDE_EXTENT_VALUE,
            distance::FACE_EXTEND_VALUE,
            None,
        ),
    };
    if paired_at != start.checked_add(expected_frame_length)?
        || reference_count_at != start.checked_add(reference_count_offset)?
        || reference_members.len() != reference_count
        || View::u32_le_at(bytes, start.checked_add(to_face::PREFIX_CONSTANT)?)?
            != to_face::PREFIX_CONSTANT_VALUE
        || !zeros_at::<3>(bytes, start + to_face::ZERO_RUN_3)
        || bytes.get(start + to_face::OPERATION_PREFIX_MARKER)
            != Some(&to_face::OPERATION_PREFIX_MARKER_VALUE)
        || !zeros_at::<3>(bytes, start + to_face::ZERO_RUN_3_AFTER_START)
    {
        return None;
    }
    let operation_offset = start + to_face::OPERATION;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let direction_offset = start + to_face::DIRECTION;
    let face_extend_offset = start + to_face::FACE_EXTEND;
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_offset)?,
        View::u32_le_at(bytes, face_extend_offset)?,
    ];
    if direction_face_extend_values != [to_face::DIRECTION_VALUE, face_extend] {
        return None;
    }
    let direction_reversed_offset = start + to_face::DIRECTION_REVERSED;
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start + to_face::GEOMETRY_KIND;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start + to_face::START_SUPPORT;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    let profile_normal = f64s_at::<3>(bytes, start + to_face::PROFILE_NORMAL)?;
    let profile_normal_squared = profile_normal
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if profile_normal
        .iter()
        .any(|component| !component.is_finite())
        || (profile_normal_squared - 1.0).abs() > PROFILE_NORMAL_UNIT_EPS
    {
        return None;
    }
    let first_side_extent_offset = start + first_side_extent_offset;
    let second_side_extent_offset = reference_count_at.checked_sub(4)?;
    let side_extent_discriminators = [
        View::u32_le_at(bytes, first_side_extent_offset)?,
        View::u32_le_at(bytes, second_side_extent_offset)?,
    ];
    if side_extent_discriminators != [first_side_extent, 0] {
        return None;
    }
    let extent = super::extrude::exact_extrude_extent(
        direction_face_extend_values[0],
        side_extent_discriminators,
    )?;
    if let Some(offset) = first_side_offset_reference {
        let record_index =
            super::shared_frames::marked_record_reference(bytes, start.checked_add(offset)?)?;
        if !<&[u32; 9]>::try_from(reference_members)
            .ok()?
            .contains(&record_index)
        {
            return None;
        }
    }
    if View::u32_le_at(bytes, reference_count_at)? != u32::try_from(reference_count).ok()? {
        return None;
    }
    let first_reference_marker = reference_count_at + 4;
    let listed = match variant {
        OneSidedVariant::ToFace => {
            references_listed::<9>(bytes, first_reference_marker, reference_members)
        }
        OneSidedVariant::Distance => {
            references_listed::<7>(bytes, first_reference_marker, reference_members)
        }
    };
    if !listed {
        return None;
    }
    Some(DesignExtrudePrologue::ReferenceAware {
        reference: None,
        operation,
        operation_offset: u64_from_index(operation_offset),
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64_from_index(first_side_extent_offset),
            u64_from_index(second_side_extent_offset),
        ],
        first_side_target_ordinal: None,
        extent,
        direction_face_extend_offsets: [
            u64_from_index(direction_offset),
            u64_from_index(face_extend_offset),
        ],
        direction_reversed,
        direction_reversed_offset: u64_from_index(direction_reversed_offset),
        solid_operation,
        solid_operation_offset: u64_from_index(solid_operation_offset),
        start: start_support,
        start_offset: u64_from_index(start_offset),
    })
}

/// Whether the `N` marked references from `first_marker` list
/// `reference_members` in order, eleven bytes apart.
fn references_listed<const N: usize>(
    bytes: &[u8],
    first_marker: usize,
    reference_members: &[u32],
) -> bool {
    let Ok(members) = <&[u32; N]>::try_from(reference_members) else {
        return false;
    };
    members.iter().enumerate().all(|(ordinal, record_index)| {
        super::shared_frames::marked_record_reference(bytes, first_marker + ordinal * 11)
            == Some(*record_index)
    })
}
