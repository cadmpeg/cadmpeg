// SPDX-License-Identifier: Apache-2.0
//! Exact extrude admission frames.

use cadmpeg_core::decode::u64_from_index;

use super::legacy_class_397;
use super::legacy_class_415;
use super::shared_frames::extrude_operation_at;
use super::shared_frames::marked_record_reference;
use crate::bytes::f64s_at;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::text::fixed_guid_end;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_54 as class_296_legacy_scalar_54;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_70 as class_296_legacy_scalar_70;
use crate::layout::class_296_261_legacy_one_sided_distance_tail as class_296_legacy_distance;
use crate::layout::class_296_261_legacy_one_sided_to_face_tail as class_296_legacy_to_face;
use crate::layout::class_296_261_one_sided_to_face_extrude_prefix as class_296_to_face;
use crate::layout::class_296_261_symmetric_distance_extrude_prefix as class_296_symmetric;
use crate::layout::class_296_261_two_sided_to_faces_extrude_prefix as class_296_two_faces;
use crate::layout::compact_shifted_extrude_extent_and_table_prefix as compact_extrude_extent;
use crate::layout::compact_shifted_extrude_mixed_extent_and_table_prefix as compact_extrude_mixed;
use crate::layout::compact_shifted_extrude_prologue as compact_extrude;
use crate::layout::current_extrude_non_target_extent_pair as extrude_extent_pair;
use crate::layout::current_extrude_operation_fields as extrude_fields;
use crate::layout::current_extrude_shape_target_extent_prefix as extrude_target;
use crate::layout::early_distance_extrude_absent_prefix as early_absent;
use crate::layout::early_distance_extrude_present_prefix as early_present;
use crate::layout::legacy_class_338_two_sided_distance_extrude_frame as class_338_legacy;
use crate::layout::legacy_class_415_symmetric_extrude_prefix as class_415;
use crate::layout::shifted_extrude_offset_283_two_sided_tail as shifted_283;
use crate::layout::shifted_extrude_offset_profile_extent_lane as offset_lane;
use crate::layout::shifted_extrude_prologue as shifted_extrude;
use crate::layout::shifted_reference_aware_extrude_class_323_symmetric_prefix as shifted_reference_aware_323_symmetric;
use crate::layout::shifted_reference_aware_extrude_class_323_tail as shifted_reference_aware_323_tail;
use crate::layout::shifted_reference_aware_extrude_scope_prefix as shifted_reference_aware;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudePrologueReference;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::extrude::DesignExtrudeTargetOrdinal;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;

fn flag_byte_at(bytes: &[u8], offset: usize) -> Option<bool> {
    match bytes.get(offset)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn extrude_start_at(bytes: &[u8], offset: usize) -> Option<DesignExtrudeStart> {
    match bytes.get(offset)? {
        0 => Some(DesignExtrudeStart::ProfilePlane),
        1 => Some(DesignExtrudeStart::OffsetProfilePlane),
        2 => Some(DesignExtrudeStart::FromFace),
        _ => None,
    }
}

/// The enclosing parameter-scope frame an Extrude prologue reads: its primary
/// and paired header offsets and class tags, and the reference-count offset.
#[derive(Clone, Copy)]
pub(super) struct ExtrudeScopeFrame<'tag> {
    pub(super) start: usize,
    pub(super) paired_at: usize,
    pub(super) class_tag: &'tag str,
    pub(super) paired_class_tag: &'tag str,
    pub(super) reference_count_at: usize,
}

pub(super) fn exact_extrude_prologue(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    frame: ExtrudeScopeFrame<'_>,
    reference_members: &[u32],
) -> Result<Option<DesignExtrudePrologue>, cadmpeg_core::CodecError> {
    let ExtrudeScopeFrame {
        start,
        paired_at,
        class_tag,
        paired_class_tag,
        reference_count_at,
    } = frame;
    if class_tag == "415" && paired_class_tag == "265" {
        let symmetric_distance = paired_at
            .checked_sub(start)
            .zip(reference_count_at.checked_sub(start))
            .and_then(|(frame_length, reference_count_delta)| {
                Some(legacy_class_415::is_symmetric_distance_layout(
                    class_tag,
                    paired_class_tag,
                    u64::try_from(frame_length).ok()?,
                    u64::try_from(reference_count_delta).ok()?,
                    reference_members.len(),
                ))
            })
            .unwrap_or(false);
        if symmetric_distance {
            return exact_current_extrude_prologue(
                ctx,
                bytes,
                start,
                reference_count_at,
                reference_members,
                true,
            );
        }
        return Ok(legacy_class_415::exact_one_sided_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        ));
    }
    if let Some(prologue) = exact_current_extrude_prologue(
        ctx,
        bytes,
        start,
        reference_count_at,
        reference_members,
        false,
    )? {
        return Ok(Some(prologue));
    }
    if let Some(prologue) = exact_shifted_reference_aware_extrude_prologue(
        bytes,
        start,
        reference_count_at,
        reference_members,
    ) {
        return Ok(Some(prologue));
    }
    if let Some(prologue) =
        legacy_class_397::exact_symmetric_extrude_prologue(bytes, frame, reference_members)
    {
        return Ok(Some(prologue));
    }
    if let Some(prologue) =
        exact_class_338_two_sided_distance_extrude_prologue(bytes, frame, reference_members)
    {
        return Ok(Some(prologue));
    }
    if let Some(prologue) = exact_legacy_shifted_extrude_prologue(
        ctx,
        bytes,
        start,
        reference_count_at,
        reference_members,
    )? {
        return Ok(Some(prologue));
    }
    Ok(
        exact_compact_shifted_extrude_prologue(bytes, start, reference_count_at)
            .or_else(|| {
                exact_compact_shifted_extrude_mixed_prologue(
                    bytes,
                    start,
                    reference_count_at,
                    reference_members,
                )
            })
            .or_else(|| {
                exact_class_296_one_sided_to_face_extrude_prologue(
                    bytes,
                    start,
                    paired_at,
                    class_tag,
                    paired_class_tag,
                    reference_count_at,
                    reference_members,
                )
            })
            .or_else(|| {
                exact_class_296_symmetric_distance_extrude_prologue(
                    bytes,
                    start,
                    paired_at,
                    class_tag,
                    paired_class_tag,
                    reference_count_at,
                    reference_members,
                )
            })
            .or_else(|| {
                exact_class_296_two_sided_to_faces_extrude_prologue(
                    bytes,
                    start,
                    paired_at,
                    class_tag,
                    paired_class_tag,
                    reference_count_at,
                    reference_members,
                )
            })
            .or_else(|| {
                exact_class_296_legacy_one_sided_extrude_prologue(
                    bytes,
                    start,
                    paired_at,
                    class_tag,
                    paired_class_tag,
                    reference_count_at,
                    reference_members,
                )
            })
            .or_else(|| exact_legacy_distance_extrude_prologue(bytes, start, reference_count_at)),
    )
}

pub(crate) fn is_class_296_one_sided_to_face_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_to_face::REFERENCE_COUNT)
        && matches!(
            (frame_length, reference_member_count),
            (440, 7) | (462, 9) | (473, 10)
        )
}

pub(crate) fn is_class_296_symmetric_distance_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_symmetric::REFERENCE_COUNT)
        && (frame_length, reference_member_count) == (450, 7)
}

pub(crate) fn is_class_296_two_sided_to_faces_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_two_faces::REFERENCE_COUNT)
        && (frame_length, reference_member_count) == (536, 13)
}

pub(crate) fn is_class_296_two_sided_to_faces_scope(scope: &DesignParameterScope) -> bool {
    let Some(reference_count_offset) = scope
        .reference_count_offset()
        .checked_sub(scope.byte_offset())
    else {
        return false;
    };
    is_class_296_two_sided_to_faces_layout(
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length(),
        reference_count_offset,
        scope.reference_members().len(),
    ) && scope
        .extrude_prologue()
        .and_then(DesignExtrudePrologue::extent)
        == Some(DesignExtrudeExtent::TwoSidedToFaces)
}

pub(crate) fn is_class_296_legacy_one_sided_to_face_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_legacy_to_face::REFERENCE_COUNT)
        && (frame_length, reference_member_count) == (515, 12)
}

pub(crate) fn is_class_296_legacy_one_sided_distance_layout(
    class_tag: &str,
    paired_class_tag: &str,
    frame_length: u64,
    reference_count_delta: u64,
    reference_member_count: usize,
) -> bool {
    class_tag == "296"
        && paired_class_tag == "261"
        && reference_count_delta == u64_from_index(class_296_legacy_distance::REFERENCE_COUNT)
        && (frame_length, reference_member_count) == (483, 10)
}

fn exact_compact_shifted_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
) -> Option<DesignExtrudePrologue> {
    if View::u32_le_at(bytes, start.checked_add(compact_extrude::PREFIX_CONSTANT)?)? != 1
        || !zeros_at::<2>(bytes, start.checked_add(compact_extrude::ZERO_RUN_2)?)
        || reference_count_at.checked_sub(start)? != compact_extrude_extent::REFERENCE_COUNT
    {
        return None;
    }
    let operation_offset = start.checked_add(compact_extrude::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(compact_extrude::DIRECTION)?,
        start.checked_add(compact_extrude::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if !matches!(direction_face_extend_values[0], 1 | 3) {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(compact_extrude_extent::FIRST_SIDE_EXTENT)?,
        start.checked_add(compact_extrude_extent::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [1, 0] {
        return None;
    }
    let extent = exact_extrude_extent(direction_face_extend_values[0], side_extent_discriminators)?;
    let direction_reversed_offset = start.checked_add(compact_extrude::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(compact_extrude::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(compact_extrude::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64_from_index(operation_offset),
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64_from_index(side_extent_discriminator_offsets[0]),
            u64_from_index(side_extent_discriminator_offsets[1]),
        ],
        extent: Some(extent),
        direction_face_extend_offsets: [
            u64_from_index(direction_face_extend_offsets[0]),
            u64_from_index(direction_face_extend_offsets[1]),
        ],
        direction_reversed,
        direction_reversed_offset: u64_from_index(direction_reversed_offset),
        solid_operation,
        solid_operation_offset: u64_from_index(solid_operation_offset),
        start: start_support,
        start_offset: u64_from_index(start_offset),
    })
}

fn exact_compact_shifted_extrude_mixed_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    if View::u32_le_at(bytes, start.checked_add(compact_extrude::PREFIX_CONSTANT)?)? != 1
        || !zeros_at::<2>(bytes, start.checked_add(compact_extrude::ZERO_RUN_2)?)
        || reference_count_at.checked_sub(start)? != compact_extrude_mixed::REFERENCE_COUNT
        || reference_members.len() != 11
    {
        return None;
    }
    let operation_offset = start.checked_add(compact_extrude::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(compact_extrude::DIRECTION)?,
        start.checked_add(compact_extrude::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values != [2, 0] {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(compact_extrude_mixed::FIRST_SIDE_EXTENT)?,
        start.checked_add(compact_extrude_mixed::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [1, 2] {
        return None;
    }
    let direction_reversed_offset = start.checked_add(compact_extrude::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(compact_extrude::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(compact_extrude::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(side_extent_discriminator_offsets[0]).ok()?,
            u64::try_from(side_extent_discriminator_offsets[1]).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::TwoSidedDistanceToFace),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

fn exact_class_296_one_sided_to_face_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    let frame_length = paired_at.checked_sub(start)?;
    if !is_class_296_one_sided_to_face_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        return None;
    }
    if View::u32_le_at(
        bytes,
        start.checked_add(class_296_to_face::PREFIX_CONSTANT)?,
    )? != 1
        || !zeros_at::<2>(bytes, start.checked_add(class_296_to_face::ZERO_RUN_2)?)
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_to_face::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_296_to_face::DIRECTION)?,
        start.checked_add(class_296_to_face::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values[0] != 1 || !matches!(direction_face_extend_values[1], 1 | 2) {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(class_296_to_face::FIRST_SIDE_EXTENT)?,
        start.checked_add(class_296_to_face::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [2, 0]
        || side_extent_discriminator_offsets[1].checked_add(4)? != reference_count_at
    {
        return None;
    }
    let direction_reversed_offset = start.checked_add(class_296_to_face::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_296_to_face::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_296_to_face::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(side_extent_discriminator_offsets[0]).ok()?,
            u64::try_from(side_extent_discriminator_offsets[1]).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::OneSidedToFace),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

fn exact_class_296_symmetric_distance_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    let frame_length = paired_at.checked_sub(start)?;
    if !is_class_296_symmetric_distance_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        return None;
    }
    if View::u32_le_at(
        bytes,
        start.checked_add(class_296_symmetric::PREFIX_CONSTANT)?,
    )? != 1
        || !zeros_at::<2>(bytes, start.checked_add(class_296_symmetric::ZERO_RUN_2)?)
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_symmetric::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_296_symmetric::DIRECTION)?,
        start.checked_add(class_296_symmetric::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values != [3, 2] {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(class_296_symmetric::FIRST_SIDE_EXTENT)?,
        start.checked_add(class_296_symmetric::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [1, 0]
        || side_extent_discriminator_offsets[1].checked_add(4)? != reference_count_at
    {
        return None;
    }
    let direction_reversed_offset = start.checked_add(class_296_symmetric::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_296_symmetric::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_296_symmetric::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(side_extent_discriminator_offsets[0]).ok()?,
            u64::try_from(side_extent_discriminator_offsets[1]).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::SymmetricDistance),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

/// The class-296 two-sided to-faces prologue. The layout fixes the reference
/// run at thirteen members, so each membership test reads at most thirteen
/// values.
fn exact_class_296_two_sided_to_faces_extrude_prologue(
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
    if !is_class_296_two_sided_to_faces_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        return None;
    }
    if View::u32_le_at(
        bytes,
        start.checked_add(class_296_two_faces::PREFIX_CONSTANT)?,
    )? != 1
        || !zeros_at::<2>(bytes, start.checked_add(class_296_two_faces::ZERO_RUN_2)?)
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_two_faces::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_296_two_faces::DIRECTION)?,
        start.checked_add(class_296_two_faces::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values[0] != 2 || !matches!(direction_face_extend_values[1], 1 | 2) {
        return None;
    }
    let direction_reversed_offset = start.checked_add(class_296_two_faces::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_296_two_faces::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_296_two_faces::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    if !zeros_at::<3>(
        bytes,
        start.checked_add(class_296_two_faces::ZERO_RUN_3_AFTER_START)?,
    ) {
        return None;
    }
    let profile_normal = f64s_at::<3>(
        bytes,
        start.checked_add(class_296_two_faces::PROFILE_NORMAL)?,
    )?;
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
    let mut slot_offset = start.checked_add(class_296_two_faces::REFERENCE_SLOTS)?;
    for expected_present in [false, false, false, true, true, true, true] {
        let present = flag_byte_at(bytes, slot_offset)?;
        if present != expected_present {
            return None;
        }
        if present {
            let record_index = marked_record_reference(bytes, slot_offset)?;
            if !reference_members.contains(&record_index) {
                return None;
            }
            slot_offset = slot_offset.checked_add(11)?;
        } else {
            slot_offset = slot_offset.checked_add(1)?;
        }
    }
    if slot_offset != start.checked_add(class_296_two_faces::FIRST_SIDE_EXTENT)? {
        return None;
    }
    let side_extent_discriminator_offsets = [
        start.checked_add(class_296_two_faces::FIRST_SIDE_EXTENT)?,
        start.checked_add(class_296_two_faces::SECOND_SIDE_EXTENT)?,
    ];
    let side_extent_discriminators = [
        View::u32_le_at(bytes, side_extent_discriminator_offsets[0])?,
        View::u32_le_at(bytes, side_extent_discriminator_offsets[1])?,
    ];
    if side_extent_discriminators != [2, 0]
        || side_extent_discriminator_offsets[1].checked_add(4)? != reference_count_at
    {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(side_extent_discriminator_offsets[0]).ok()?,
            u64::try_from(side_extent_discriminator_offsets[1]).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::TwoSidedToFaces),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

/// The class-296 legacy one-sided prologue. The layouts fix the reference
/// run at ten or twelve members, so each membership test reads at most twelve
/// values.
fn exact_class_296_legacy_one_sided_extrude_prologue(
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
    let (is_to_face, face_extend, first_extent) = if is_class_296_legacy_one_sided_to_face_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        (true, 1, 2)
    } else if is_class_296_legacy_one_sided_distance_layout(
        class_tag,
        paired_class_tag,
        u64::try_from(frame_length).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    ) {
        (false, 2, 1)
    } else {
        return None;
    };
    if View::u32_le_at(
        bytes,
        start.checked_add(class_296_legacy_scalar_54::PREFIX_CONSTANT)?,
    )? != 1
        || !zeros_at::<1>(
            bytes,
            start.checked_add(class_296_legacy_scalar_54::ZERO_BEFORE_REFERENCE)?,
        )
        || !reference_members.contains(&marked_record_reference(
            bytes,
            start.checked_add(class_296_legacy_scalar_54::REFERENCE)?,
        )?)
        || !zeros_at::<6>(
            bytes,
            start.checked_add(class_296_legacy_scalar_54::REFERENCE + 5)?,
        )
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_legacy_scalar_54::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_296_legacy_scalar_54::DIRECTION)?,
        start.checked_add(class_296_legacy_scalar_54::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values != [1, face_extend] {
        return None;
    }
    let direction_reversed_offset =
        start.checked_add(class_296_legacy_scalar_54::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_296_legacy_scalar_54::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_296_legacy_scalar_54::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    if !zeros_at::<3>(
        bytes,
        start.checked_add(class_296_legacy_scalar_54::ZERO_AFTER_START)?,
    ) {
        return None;
    }
    let profile_scalar_at_54 = f64s_at::<1>(
        bytes,
        start.checked_add(class_296_legacy_scalar_54::PROFILE_SCALAR_AT_54)?,
    )?
    .into_iter()
    .next()?;
    let profile_scalar_at_70 = f64s_at::<1>(
        bytes,
        start.checked_add(class_296_legacy_scalar_70::PROFILE_SCALAR_AT_70)?,
    )?
    .into_iter()
    .next()?;
    let scalar_at_54 = profile_scalar_at_54.is_finite()
        && (profile_scalar_at_54.abs() - 1.0).abs() <= PROFILE_NORMAL_UNIT_EPS
        && zeros_at::<16>(
            bytes,
            start.checked_add(class_296_legacy_scalar_54::ZERO_AFTER_SCALAR_AT_54)?,
        );
    let scalar_at_70 = profile_scalar_at_70.is_finite()
        && (profile_scalar_at_70.abs() - 1.0).abs() <= PROFILE_NORMAL_UNIT_EPS
        && zeros_at::<16>(
            bytes,
            start.checked_add(class_296_legacy_scalar_70::ZERO_BEFORE_SCALAR_AT_70)?,
        );
    if !scalar_at_54 && !scalar_at_70 {
        return None;
    }
    let mut slot_offset = start.checked_add(class_296_legacy_scalar_54::REFERENCE_SLOTS)?;
    let slot_presence = if is_to_face {
        [true, false, false, true, false, true, true]
    } else {
        [true, false, true, true, false, true, false]
    };
    for expected_present in slot_presence {
        let present = flag_byte_at(bytes, slot_offset)?;
        if present != expected_present {
            return None;
        }
        if present {
            let record_index = marked_record_reference(bytes, slot_offset)?;
            if !reference_members.contains(&record_index) {
                return None;
            }
            slot_offset = slot_offset.checked_add(11)?;
        } else {
            slot_offset = slot_offset.checked_add(1)?;
        }
    }
    let first_side_extent_offset =
        start.checked_add(class_296_legacy_scalar_54::FIRST_SIDE_EXTENT)?;
    if slot_offset != first_side_extent_offset
        || View::u32_le_at(bytes, first_side_extent_offset)? != first_extent
    {
        return None;
    }
    let second_side_extent_offset = reference_count_at.checked_sub(4)?;
    if View::u32_le_at(bytes, second_side_extent_offset)? != 0
        || second_side_extent_offset.checked_add(4)? != reference_count_at
    {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators: [first_extent, 0],
        side_extent_discriminator_offsets: [
            u64::try_from(first_side_extent_offset).ok()?,
            u64::try_from(second_side_extent_offset).ok()?,
        ],
        extent: Some(if is_to_face {
            DesignExtrudeExtent::OneSidedToFace
        } else {
            DesignExtrudeExtent::OneSidedDistance
        }),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

fn exact_legacy_distance_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
) -> Option<DesignExtrudePrologue> {
    let marker_offset = start.checked_add(early_absent::ABSENT_PREFIX)?;
    let (prefix_zero_offset, operation_offset, expected_reference_count_delta) =
        match bytes.get(marker_offset)? {
            0 => (None, start.checked_add(early_absent::OPERATION)?, 208),
            1 => {
                let prefix_value_offset = start.checked_add(early_present::PREFIX_VALUE)?;
                let prefix_value = View::u32_le_at(bytes, prefix_value_offset)?;
                if prefix_value != 0 {
                    return None;
                }
                (
                    Some(u64::try_from(prefix_value_offset).ok()?),
                    start.checked_add(early_present::OPERATION)?,
                    212,
                )
            }
            _ => return None,
        };
    if reference_count_at.checked_sub(start)? != expected_reference_count_delta {
        return None;
    }
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let extent_kind_offset = operation_offset.checked_add(4)?;
    let extent_kind = View::u32_le_at(bytes, extent_kind_offset)?;
    if extent_kind != 2 {
        return None;
    }
    let direction_reversed_offset = extent_kind_offset.checked_add(4)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let geometry_kind_offset = direction_reversed_offset.checked_add(1)?;
    let solid_operation = match View::u32_le_at(bytes, geometry_kind_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(DesignExtrudePrologue::LegacyDistance {
        prefix_zero_offset,
        operation,
        operation_offset: u64_from_index(operation_offset),
        extent_kind_offset: u64_from_index(extent_kind_offset),
        direction_reversed,
        direction_reversed_offset: u64_from_index(direction_reversed_offset),
        solid_operation,
        solid_operation_offset: u64_from_index(geometry_kind_offset),
    })
}

/// Whether the operation slot at `operation_offset` reads as an operation,
/// a direction, a face-extend value and the three flag bytes after them.
fn is_operation_candidate(bytes: &[u8], operation_offset: usize) -> bool {
    let field = |delta: usize| operation_offset.checked_add(delta);
    matches!(View::u32_le_at(bytes, operation_offset), Some(1..=4))
        && matches!(
            field(4).and_then(|at| View::u32_le_at(bytes, at)),
            Some(1..=3)
        )
        && field(8).and_then(|at| View::u32_le_at(bytes, at)).is_some()
        && matches!(field(12).and_then(|at| bytes.get(at)), Some(0 | 1))
        && matches!(field(13).and_then(|at| bytes.get(at)), Some(0 | 1))
        && matches!(field(14).and_then(|at| bytes.get(at)), Some(0..=2))
}

/// The operation offset and the leading reference of a current Extrude
/// prologue. Without a marked reference at offset 25 the operation sits at
/// offset 28 and the reference is `None`. With one, exactly one placement of
/// the operation after the reference may read as an operation: after seven
/// or eight zero bytes, or after seven zero bytes and a marker.
fn current_extrude_reference(
    bytes: &[u8],
    start: usize,
) -> Option<(usize, Option<DesignExtrudePrologueReference>)> {
    if bytes.get(start.checked_add(25)?) != Some(&1) {
        return Some((start.checked_add(28)?, None));
    }
    let record_index_offset = start.checked_add(26)?;
    let record_index = View::u32_le_at(bytes, record_index_offset)?;
    let prefix_tail = start.checked_add(30)?;
    let after_seven_zeros = start.checked_add(37)?;
    let after_eight_zeros = start.checked_add(38)?;
    let seven_zeros = zeros_at::<7>(bytes, prefix_tail);
    let eighth_byte = bytes.get(after_seven_zeros).copied();
    let candidates = [
        (seven_zeros, after_seven_zeros, None),
        (
            seven_zeros && eighth_byte == Some(0),
            after_eight_zeros,
            None,
        ),
        (
            seven_zeros && eighth_byte == Some(1),
            after_eight_zeros,
            Some(after_seven_zeros),
        ),
    ];
    let mut found = None;
    for (padding_valid, operation_offset, marker_offset) in candidates {
        if !padding_valid || !is_operation_candidate(bytes, operation_offset) {
            continue;
        }
        if found.replace((operation_offset, marker_offset)).is_some() {
            return None;
        }
    }
    let (operation_offset, operation_marker_offset) = found?;
    let padding_end = operation_marker_offset.unwrap_or(operation_offset);
    Some((
        operation_offset,
        Some(DesignExtrudePrologueReference {
            record_index,
            record_index_offset: u64_from_index(record_index_offset),
            trailing_zero_count: u8::try_from(padding_end.checked_sub(prefix_tail)?).ok()?,
            operation_prefix_marker_offset: operation_marker_offset.map(u64_from_index),
        }),
    ))
}

/// The fixed operation fields of a current Extrude prologue, from the
/// operation through the unit profile normal.
struct CurrentExtrudeFields {
    operation: crate::records::feature::extrude::DesignExtrudeOperation,
    operation_offset: usize,
    direction_offset: usize,
    face_extend_offset: usize,
    direction_face_extend_values: [u32; 2],
    direction_reversed: bool,
    direction_reversed_offset: usize,
    solid_operation: bool,
    solid_operation_offset: usize,
    start_support: DesignExtrudeStart,
    start_offset: usize,
    /// The first of the seven reference slots after the profile normal.
    slots: usize,
    reference: Option<DesignExtrudePrologueReference>,
}

fn current_extrude_fields(
    bytes: &[u8],
    operation_offset: usize,
    reference: Option<DesignExtrudePrologueReference>,
) -> Option<CurrentExtrudeFields> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_offset = operation_offset.checked_add(extrude_fields::DIRECTION)?;
    let face_extend_offset = operation_offset.checked_add(extrude_fields::FACE_EXTEND)?;
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_offset)?,
        View::u32_le_at(bytes, face_extend_offset)?,
    ];
    if !matches!(direction_face_extend_values[0], 1..=3) {
        return None;
    }
    let direction_reversed_offset =
        operation_offset.checked_add(extrude_fields::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = operation_offset.checked_add(extrude_fields::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = operation_offset.checked_add(extrude_fields::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    let profile_normal_offset = operation_offset.checked_add(extrude_fields::PROFILE_NORMAL)?;
    if !zeros_at::<3>(
        bytes,
        operation_offset.checked_add(extrude_fields::ZERO_RUN_3)?,
    ) {
        return None;
    }
    let profile_normal = f64s_at::<3>(bytes, profile_normal_offset)?;
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
    Some(CurrentExtrudeFields {
        operation,
        operation_offset,
        direction_offset,
        face_extend_offset,
        direction_face_extend_values,
        direction_reversed,
        direction_reversed_offset,
        solid_operation,
        solid_operation_offset,
        start_support,
        start_offset,
        slots: profile_normal_offset.checked_add(24)?,
        reference,
    })
}

/// The reference slots of a current Extrude prologue that follow its fixed
/// fields.
struct CurrentExtrudeSlots {
    /// The byte after the last slot.
    end: usize,
    presence: [bool; 7],
    /// The record named by the seventh slot.
    final_reference: Option<u32>,
}

/// The current reference-aware Extrude prologue. The reference run has no
/// fixed length, so each membership test is a charged search that stops at
/// the first match.
fn exact_current_extrude_prologue(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
    legacy_class_415_symmetric_distance: bool,
) -> Result<Option<DesignExtrudePrologue>, cadmpeg_core::CodecError> {
    if legacy_class_415_symmetric_distance
        && (start
            .checked_add(class_415::PREFIX_CONSTANT)
            .and_then(|at| View::u32_le_at(bytes, at))
            != Some(1)
            || !start
                .checked_add(class_415::ZERO_RUN_3)
                .is_some_and(|at| zeros_at::<3>(bytes, at))
            || start
                .checked_add(class_415::OPERATION_PREFIX_MARKER)
                .and_then(|at| bytes.get(at))
                != Some(&1))
    {
        return Ok(None);
    }
    let Some((operation_offset, reference)) = current_extrude_reference(bytes, start) else {
        return Ok(None);
    };
    if let Some(reference) = &reference {
        if !ctx.contains(
            reference_members,
            &reference.record_index,
            "search F3D current Extrude candidate reference member",
        )? {
            return Ok(None);
        }
    }
    let Some(fields) = current_extrude_fields(bytes, operation_offset, reference) else {
        return Ok(None);
    };
    let mut slots = CurrentExtrudeSlots {
        end: fields.slots,
        presence: [false; 7],
        final_reference: None,
    };
    for (slot_ordinal, slot_present) in slots.presence.iter_mut().enumerate() {
        match bytes.get(slots.end) {
            Some(0) => slots.end += 1,
            Some(1) => {
                let Some(record_index) = marked_record_reference(bytes, slots.end) else {
                    return Ok(None);
                };
                if !ctx.contains(
                    reference_members,
                    &record_index,
                    "search F3D current Extrude slot reference members",
                )? {
                    return Ok(None);
                }
                *slot_present = true;
                if slot_ordinal == 6 {
                    slots.final_reference = Some(record_index);
                }
                slots.end += 11;
            }
            _ => return Ok(None),
        }
    }
    Ok(current_extrude_extent(
        bytes,
        start,
        reference_count_at,
        reference_members,
        legacy_class_415_symmetric_distance,
        &fields,
        &slots,
    ))
}

/// The side extents after the reference slots of a current Extrude prologue,
/// and the prologue they complete.
fn current_extrude_extent(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
    legacy_class_415_symmetric_distance: bool,
    fields: &CurrentExtrudeFields,
    slots: &CurrentExtrudeSlots,
) -> Option<DesignExtrudePrologue> {
    if legacy_class_415_symmetric_distance
        && slots.presence != [false, true, true, true, false, true, false]
    {
        return None;
    }
    let mut extent_cursor = slots.end;
    let first_side_target_ordinal = slots.final_reference.and_then(|record_index| {
        let scope_reference_ordinal = View::u32_le_at(bytes, extent_cursor)?;
        let ordinal = usize::try_from(scope_reference_ordinal).ok()?;
        if reference_members.get(ordinal) != Some(&record_index)
            || bytes.get(extent_cursor.checked_add(extrude_target::ZERO_SEPARATOR)?) != Some(&0)
            || View::u32_le_at(
                bytes,
                extent_cursor.checked_add(extrude_target::FIRST_SIDE_EXTENT)?,
            ) != Some(2)
        {
            return None;
        }
        Some(DesignExtrudeTargetOrdinal {
            scope_reference_ordinal,
            scope_reference_ordinal_offset: u64_from_index(extent_cursor),
        })
    });
    if first_side_target_ordinal.is_some() {
        extent_cursor = extent_cursor.checked_add(5)?;
    }
    let first_side_extent_offset = extent_cursor;
    let first_side_extent = View::u32_le_at(bytes, first_side_extent_offset)?;
    let second_side_extent_offset = if first_side_extent == 2 {
        reference_count_at.checked_sub(4)?
    } else {
        let second_side_extent_offset =
            first_side_extent_offset.checked_add(extrude_extent_pair::SECOND_SIDE_EXTENT)?;
        if !zeros_at::<9>(bytes, first_side_extent_offset.checked_add(4)?) {
            return None;
        }
        second_side_extent_offset
    };
    if second_side_extent_offset < first_side_extent_offset.checked_add(4)?
        || second_side_extent_offset.checked_add(4)? > reference_count_at
    {
        return None;
    }
    let side_extent_discriminators = [
        first_side_extent,
        View::u32_le_at(bytes, second_side_extent_offset)?,
    ];
    if legacy_class_415_symmetric_distance
        && (extent_cursor != start.checked_add(class_415::FIRST_SIDE_EXTENT)?
            || first_side_extent_offset != start.checked_add(class_415::FIRST_SIDE_EXTENT)?
            || second_side_extent_offset != start.checked_add(class_415::SECOND_SIDE_EXTENT)?
            || reference_count_at != start.checked_add(class_415::REFERENCE_COUNT)?
            || fields.direction_face_extend_values != [3, 2]
            || side_extent_discriminators != [1, 1])
    {
        return None;
    }
    let extent = exact_extrude_extent(
        fields.direction_face_extend_values[0],
        side_extent_discriminators,
    )
    .or_else(|| {
        (legacy_class_415_symmetric_distance
            && fields.direction_face_extend_values == [3, 2]
            && side_extent_discriminators == [1, 1])
        .then_some(DesignExtrudeExtent::SymmetricDistance)
    })?;
    Some(DesignExtrudePrologue::ReferenceAware {
        reference: fields.reference,
        operation: fields.operation,
        operation_offset: u64_from_index(fields.operation_offset),
        direction_face_extend_values: fields.direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64_from_index(first_side_extent_offset),
            u64_from_index(second_side_extent_offset),
        ],
        first_side_target_ordinal,
        extent,
        direction_face_extend_offsets: [
            u64_from_index(fields.direction_offset),
            u64_from_index(fields.face_extend_offset),
        ],
        direction_reversed: fields.direction_reversed,
        direction_reversed_offset: u64_from_index(fields.direction_reversed_offset),
        solid_operation: fields.solid_operation,
        solid_operation_offset: u64_from_index(fields.solid_operation_offset),
        start: fields.start_support,
        start_offset: u64_from_index(fields.start_offset),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TailForm {
    Ordered,
    Unordered,
    SymmetricThroughAll,
}

#[derive(Clone, Copy)]
struct ShiftedReferenceAwareLayout {
    frame_length: usize,
    reference_count_offset: usize,
    reference_member_count: usize,
    expected_paired_class: &'static [u8; 3],
    trailing_reference_offset: usize,
    guid_prefix_offset: usize,
    second_side_extent_offset: usize,
    tail_form: TailForm,
}

/// The shifted reference-aware layout named by the primary class tag of the
/// frame at `start`.
fn shifted_reference_aware_layout(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_member_count: usize,
) -> Option<ShiftedReferenceAwareLayout> {
    let ordered = ShiftedReferenceAwareLayout {
        frame_length: 538,
        reference_count_offset: shifted_reference_aware::REFERENCE_COUNT,
        reference_member_count: 13,
        expected_paired_class: b"258",
        trailing_reference_offset: shifted_reference_aware::BODY_GROUP_REFERENCE,
        guid_prefix_offset: shifted_reference_aware::BODY_GROUP_GUID_PREFIX,
        second_side_extent_offset: shifted_reference_aware::SECOND_SIDE_EXTENT,
        tail_form: TailForm::Ordered,
    };
    let reference_count_delta = reference_count_at.checked_sub(start);
    Some(match bytes_at::<3>(bytes, start.checked_add(4)?)? {
        b"357" => ordered,
        b"275" | b"361" | b"397" => ShiftedReferenceAwareLayout {
            expected_paired_class: b"262",
            ..ordered
        },
        b"349" => ShiftedReferenceAwareLayout {
            expected_paired_class: b"266",
            ..ordered
        },
        b"323"
            if reference_count_delta == Some(shifted_reference_aware::REFERENCE_COUNT)
                && reference_member_count == 11 =>
        {
            ShiftedReferenceAwareLayout {
                frame_length: 516,
                reference_count_offset: shifted_reference_aware::REFERENCE_COUNT,
                reference_member_count: 11,
                expected_paired_class: b"263",
                trailing_reference_offset: shifted_reference_aware_323_tail::TRAILING_REFERENCE,
                guid_prefix_offset: shifted_reference_aware::BODY_GROUP_GUID_PREFIX,
                second_side_extent_offset: shifted_reference_aware::SECOND_SIDE_EXTENT,
                tail_form: TailForm::Unordered,
            }
        }
        b"323"
            if reference_count_delta
                == Some(shifted_reference_aware_323_symmetric::REFERENCE_COUNT)
                && reference_member_count == 10 =>
        {
            ShiftedReferenceAwareLayout {
                frame_length: 485,
                reference_count_offset: shifted_reference_aware_323_symmetric::REFERENCE_COUNT,
                reference_member_count: 10,
                expected_paired_class: b"263",
                trailing_reference_offset:
                    shifted_reference_aware_323_symmetric::TRAILING_REFERENCE,
                guid_prefix_offset: shifted_reference_aware_323_symmetric::GUID_PREFIX,
                second_side_extent_offset:
                    shifted_reference_aware_323_symmetric::SECOND_SIDE_EXTENT,
                tail_form: TailForm::SymmetricThroughAll,
            }
        }
        _ => return None,
    })
}

/// The fixed fields between the side extents and the body-group GUID of a
/// shifted reference-aware frame at `start`.
fn shifted_reference_aware_tail_fixed(bytes: &[u8], start: usize, tail_form: TailForm) -> bool {
    use shifted_reference_aware as tail;
    use shifted_reference_aware_323_symmetric as symmetric;
    let u32_at = |offset: usize| View::u32_le_at(bytes, start + offset);
    match tail_form {
        TailForm::SymmetricThroughAll => {
            zeros_at::<9>(bytes, start + symmetric::FIRST_SIDE_PADDING)
                && u32_at(symmetric::FIRST_SIDE_EXTENT) == Some(symmetric::FIRST_SIDE_EXTENT_VALUE)
                && zeros_at::<6>(bytes, start + symmetric::SECOND_SIDE_PADDING)
                && zeros_at::<5>(bytes, start + symmetric::SYMMETRIC_EXTENT_PADDING)
                && u32_at(symmetric::PROFILE_GROUP_COUNT)
                    == Some(symmetric::PROFILE_GROUP_COUNT_VALUE)
                && zeros_at::<8>(bytes, start + symmetric::PROFILE_GROUP_PADDING)
                && u32_at(symmetric::TRAILING_REFERENCE_COUNT)
                    == Some(symmetric::TRAILING_REFERENCE_COUNT_VALUE)
        }
        TailForm::Ordered | TailForm::Unordered => {
            let trailing_group = if tail_form == TailForm::Ordered {
                // The body group count follows eight zero bytes; its
                // reference runs up to the GUID.
                zeros_at::<8>(bytes, start + tail::PROFILE_GROUP_PADDING)
                    && u32_at(tail::BODY_GROUP_COUNT) == Some(1)
            } else {
                // The trailing count follows the profile group directly; eight
                // zero bytes separate its reference from the GUID.
                u32_at(shifted_reference_aware_323_tail::TRAILING_REFERENCE_COUNT) == Some(1)
                    && zeros_at::<8>(
                        bytes,
                        start + shifted_reference_aware_323_tail::TRAILING_REFERENCE_PADDING,
                    )
            };
            zeros_at::<4>(bytes, start + tail::FIRST_SIDE_PADDING)
                && u32_at(tail::FIRST_SIDE_DISCRIMINANT) == Some(1)
                && u32_at(tail::FIRST_SIDE_PAYLOAD) == Some(2)
                && bytes.get(start + tail::FIRST_SIDE_SEPARATOR) == Some(&0)
                && zeros_at::<4>(bytes, start + tail::SECOND_SIDE_OFFSET_PADDING)
                && zeros_at::<5>(bytes, start + tail::SECOND_SIDE_TAPER_PADDING)
                && u32_at(tail::PROFILE_GROUP_COUNT) == Some(1)
                && trailing_group
        }
    }
}

/// The shifted reference-aware prologue at `start` up to its body-group
/// GUID, with the GUID's count offset and the offset its payload must end at.
/// The layouts fix the reference run at ten to thirteen members, so each
/// membership test reads at most thirteen values.
fn shifted_reference_aware_fields(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<(DesignExtrudePrologue, usize, usize)> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;
    let layout =
        shifted_reference_aware_layout(bytes, start, reference_count_at, reference_members.len())?;
    if reference_count_at.checked_sub(start)? != layout.reference_count_offset
        || reference_members.len() != layout.reference_member_count
        || View::u32_le_at(
            bytes,
            start.checked_add(shifted_reference_aware::PREFIX_CONSTANT)?,
        )? != 1
        || !zeros_at::<3>(
            bytes,
            start.checked_add(shifted_reference_aware::ZERO_RUN_3)?,
        )
        || !zeros_at::<3>(
            bytes,
            start.checked_add(shifted_reference_aware::ZERO_RUN_3_AFTER_START)?,
        )
        || bytes_at::<3>(bytes, start.checked_add(layout.frame_length + 4)?)
            != Some(layout.expected_paired_class)
    {
        return None;
    }
    let operation_offset = start.checked_add(shifted_reference_aware::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(shifted_reference_aware::DIRECTION)?,
        start.checked_add(shifted_reference_aware::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    let symmetric = layout.tail_form == TailForm::SymmetricThroughAll;
    if direction_face_extend_values != if symmetric { [3, 0] } else { [2, 1] } {
        return None;
    }
    let direction_reversed_offset =
        start.checked_add(shifted_reference_aware::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(shifted_reference_aware::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(shifted_reference_aware::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    let profile_normal_offset = start.checked_add(shifted_reference_aware::PROFILE_NORMAL)?;
    let profile_normal = f64s_at::<3>(bytes, profile_normal_offset)?;
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
    let mut slot_offset = start.checked_add(shifted_reference_aware::REFERENCE_SLOTS)?;
    for expected_present in [false, false, false, true, true, true, true] {
        let present = flag_byte_at(bytes, slot_offset)?;
        if present != expected_present {
            return None;
        }
        if present {
            let record_index = marked_record_reference(bytes, slot_offset)?;
            if !reference_members.contains(&record_index) {
                return None;
            }
            slot_offset = slot_offset.checked_add(11)?;
        } else {
            slot_offset = slot_offset.checked_add(1)?;
        }
    }
    let first_side_extent_offset = start.checked_add(shifted_reference_aware::FIRST_SIDE_EXTENT)?;
    if slot_offset != first_side_extent_offset {
        return None;
    }
    let second_side_extent_offset = if symmetric {
        start.checked_add(layout.second_side_extent_offset)?
    } else {
        reference_count_at.checked_sub(4)?
    };
    let side_extent_discriminators = [
        View::u32_le_at(bytes, first_side_extent_offset)?,
        View::u32_le_at(bytes, second_side_extent_offset)?,
    ];
    let extent = exact_extrude_extent(direction_face_extend_values[0], side_extent_discriminators)?;
    if side_extent_discriminators != if symmetric { [4, 4] } else { [2, 0] } {
        return None;
    }
    let tail_reference = |offset: usize| marked_record_reference(bytes, start.checked_add(offset)?);
    let ordered_tail_references_valid = if symmetric {
        [
            tail_reference(shifted_reference_aware_323_symmetric::SYMMETRIC_EXTENT_REFERENCE)?,
            tail_reference(shifted_reference_aware_323_symmetric::PROFILE_GROUP_REFERENCE)?,
        ]
        .iter()
        .all(|record_index| reference_members.contains(record_index))
    } else {
        [
            tail_reference(shifted_reference_aware::FIRST_SIDE_OWNER_REFERENCE)?,
            tail_reference(shifted_reference_aware::SECOND_SIDE_OFFSET_REFERENCE)?,
            tail_reference(shifted_reference_aware::SECOND_SIDE_TAPER_REFERENCE)?,
            tail_reference(shifted_reference_aware::PROFILE_GROUP_REFERENCE)?,
        ]
        .iter()
        .all(|record_index| reference_members.contains(record_index))
    };
    let trailing_reference = tail_reference(layout.trailing_reference_offset)?;
    let trailing_reference_valid = if layout.tail_form == TailForm::Unordered {
        trailing_reference != 0 && !reference_members.contains(&trailing_reference)
    } else {
        reference_members.contains(&trailing_reference)
    };
    if !ordered_tail_references_valid
        || !trailing_reference_valid
        || !shifted_reference_aware_tail_fixed(bytes, start, layout.tail_form)
    {
        return None;
    }
    let guid_at = start.checked_add(layout.guid_prefix_offset)?;
    let expected_guid_end = if symmetric {
        guid_at.checked_add(76)?
    } else {
        second_side_extent_offset.checked_add(1)?
    };
    Some((
        DesignExtrudePrologue::ShiftedReferenceAware {
            operation,
            operation_offset: u64_from_index(operation_offset),
            direction_face_extend_values,
            side_extent_discriminators,
            side_extent_discriminator_offsets: [
                u64_from_index(first_side_extent_offset),
                u64_from_index(second_side_extent_offset),
            ],
            extent,
            direction_face_extend_offsets: [
                u64_from_index(direction_face_extend_offsets[0]),
                u64_from_index(direction_face_extend_offsets[1]),
            ],
            direction_reversed,
            direction_reversed_offset: u64_from_index(direction_reversed_offset),
            solid_operation,
            solid_operation_offset: u64_from_index(solid_operation_offset),
            start: start_support,
            start_offset: u64_from_index(start_offset),
        },
        guid_at,
        expected_guid_end,
    ))
}

/// The shifted reference-aware Extrude prologue. Each layout fixes the
/// reference run at ten, eleven or thirteen members, so each membership test
/// reads at most thirteen values.
fn exact_shifted_reference_aware_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    let (prologue, guid_at, expected_guid_end) =
        shifted_reference_aware_fields(bytes, start, reference_count_at, reference_members)?;
    let guid_end = fixed_guid_end(bytes, guid_at);
    (guid_end == Some(expected_guid_end)
        && expected_guid_end.checked_add(3) == Some(reference_count_at)
        && zeros_at::<3>(bytes, expected_guid_end))
    .then_some(prologue)
}

pub(super) fn exact_extrude_extent(
    direction: u32,
    side_extent_discriminators: [u32; 2],
) -> Option<DesignExtrudeExtent> {
    match (direction, side_extent_discriminators) {
        (1, [1, 0]) => Some(DesignExtrudeExtent::OneSidedDistance),
        (1, [2, 0]) => Some(DesignExtrudeExtent::OneSidedToFace),
        (1, [3, 0]) => Some(DesignExtrudeExtent::OneSidedThroughNext),
        (1, [4, 0]) => Some(DesignExtrudeExtent::OneSidedThroughAll),
        (2, [2, 0]) => Some(DesignExtrudeExtent::TwoSidedToFaces),
        (2, [1, 1]) => Some(DesignExtrudeExtent::TwoSidedDistance),
        (3, [1, 0]) => Some(DesignExtrudeExtent::SymmetricDistance),
        (3, [4, 4]) => Some(DesignExtrudeExtent::SymmetricThroughAll),
        _ => None,
    }
}

/// The operation fields of a legacy shifted Extrude prologue, which sit one
/// byte later when a marker precedes the operation.
struct LegacyShiftedHead {
    operation_prefix_marker_offset: Option<u64>,
    field_shift: usize,
    reference_count_delta: usize,
    operation: crate::records::feature::extrude::DesignExtrudeOperation,
    operation_offset: usize,
    first_extent_offset: usize,
    second_extent_offset: usize,
    direction_face_extend_values: [u32; 2],
}

fn legacy_shifted_head(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
) -> Option<LegacyShiftedHead> {
    if View::u32_le_at(bytes, start.checked_add(shifted_extrude::PREFIX_CONSTANT)?)? != 1
        || !zeros_at::<3>(bytes, start.checked_add(shifted_extrude::ZERO_RUN_3)?)
    {
        return None;
    }
    let marker_offset = start.checked_add(shifted_extrude::OPERATION)?;
    let (operation_prefix_marker_offset, field_shift) =
        if matches!(View::u32_le_at(bytes, marker_offset), Some(1..=4)) {
            (None, 0)
        } else if bytes.get(marker_offset) == Some(&1)
            && matches!(
                View::u32_le_at(bytes, marker_offset.checked_add(1)?),
                Some(1..=4)
            )
        {
            (Some(u64_from_index(marker_offset)), 1)
        } else {
            return None;
        };
    let operation_offset = marker_offset.checked_add(field_shift)?;
    let reference_count_delta = reference_count_at
        .checked_sub(start)?
        .checked_sub(field_shift)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let first_extent_offset = operation_offset.checked_add(4)?;
    let second_extent_offset = operation_offset.checked_add(8)?;
    let direction_face_extend_values = [
        View::u32_le_at(bytes, first_extent_offset)?,
        View::u32_le_at(bytes, second_extent_offset)?,
    ];
    if !matches!(direction_face_extend_values[0], 1..=3) {
        return None;
    }
    Some(LegacyShiftedHead {
        operation_prefix_marker_offset,
        field_shift,
        reference_count_delta,
        operation,
        operation_offset,
        first_extent_offset,
        second_extent_offset,
        direction_face_extend_values,
    })
}

/// The side-extent offsets of a two-sided legacy shifted prologue. The
/// 283-byte compact tail is tried first; the general tail names three
/// parameter references. Each named reference must be a scope reference.
fn legacy_shifted_two_sided_offsets(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
    head: &LegacyShiftedHead,
) -> Result<Option<[usize; 2]>, cadmpeg_core::CodecError> {
    let shift = head.field_shift;
    let at = |offset: usize| start.checked_add(offset + shift);
    let is_member =
        |offset: usize, operation: &'static str| match marked_record_reference(bytes, offset) {
            Some(reference) => ctx.contains(reference_members, &reference, operation),
            None => Ok(false),
        };
    if head.reference_count_delta == 283 {
        let (
            Some(first_parameter_at),
            Some(first_side_extent_offset),
            Some(second_parameter_at),
            Some(second_side_extent_offset),
            Some(trailing_entity_at),
            Some(padding_at),
            Some(gap_at),
            Some(tail_at),
        ) = (
            at(shifted_283::FIRST_PARAMETER_REFERENCE),
            at(shifted_283::FIRST_SIDE_EXTENT),
            at(shifted_283::SECOND_PARAMETER_REFERENCE),
            at(shifted_283::SECOND_SIDE_EXTENT),
            at(shifted_283::TRAILING_ENTITY_REFERENCE),
            at(150),
            at(175),
            at(shifted_283::ZERO_RUN_8),
        )
        else {
            return Ok(None);
        };
        // A padding run past the end of the bytes rejects the prologue.
        let Some(padding) = bytes_at::<16>(bytes, padding_at) else {
            return Ok(None);
        };
        let mut compact_valid = *padding == [0; 16];
        if compact_valid {
            let Some(gap) = bytes_at::<6>(bytes, gap_at) else {
                return Ok(None);
            };
            compact_valid = *gap == [0; 6];
        }
        for offset in [first_parameter_at, second_parameter_at] {
            if !compact_valid {
                break;
            }
            compact_valid = is_member(
                offset,
                "search F3D compact shifted Extrude parameter references",
            )?;
        }
        if compact_valid && marked_record_reference(bytes, trailing_entity_at).is_some() {
            let Some(tail) =
                bytes_at::<{ shifted_283::LEN - shifted_283::ZERO_RUN_8 }>(bytes, tail_at)
            else {
                return Ok(None);
            };
            if *tail == [0; 8] {
                return Ok(Some([first_side_extent_offset, second_side_extent_offset]));
            }
        }
    }
    let (
        Some(first_parameter_at),
        Some(first_side_extent_offset),
        Some(first_offset_at),
        Some(second_side_extent_offset),
        Some(second_parameter_at),
        Some(first_padding_at),
        Some(second_padding_at),
    ) = (
        at(139),
        at(155),
        at(159),
        at(178),
        at(182),
        at(150),
        at(170),
    )
    else {
        return Ok(None);
    };
    if second_parameter_at
        .checked_add(11)
        .is_none_or(|end| end > reference_count_at)
        || !zeros_at::<5>(bytes, first_padding_at)
        || !zeros_at::<8>(bytes, second_padding_at)
    {
        return Ok(None);
    }
    for offset in [first_parameter_at, first_offset_at, second_parameter_at] {
        if !is_member(offset, "search F3D shifted Extrude parameter references")? {
            return Ok(None);
        }
    }
    Ok(Some([first_side_extent_offset, second_side_extent_offset]))
}

/// The side-extent offsets, discriminators and extent of a one-sided or
/// symmetric legacy shifted prologue, at the offsets its reference-count
/// delta names.
fn legacy_shifted_side_extents(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    head: &LegacyShiftedHead,
) -> Option<([usize; 2], [u32; 2], DesignExtrudeExtent)> {
    let (first_offset, second_offset) = match head.reference_count_delta {
        262 if bytes.get(
            head.operation_offset
                .checked_add(extrude_fields::START_SUPPORT)?,
        ) == Some(&1) =>
        {
            (
                offset_lane::FIRST_SIDE_EXTENT,
                offset_lane::SECOND_SIDE_EXTENT,
            )
        }
        252 | 262 | 263 | 692 => (106, 110),
        272 | 283 => (
            offset_lane::FIRST_SIDE_EXTENT,
            offset_lane::SECOND_SIDE_EXTENT,
        ),
        294 => (116, 129),
        _ => return None,
    };
    let first_side_extent_offset = start.checked_add(first_offset + head.field_shift)?;
    if first_side_extent_offset.checked_add(4)? > reference_count_at {
        return None;
    }
    let second_side_extent_offset = if View::u32_le_at(bytes, first_side_extent_offset)? == 2 {
        reference_count_at.checked_sub(4)?
    } else {
        start.checked_add(second_offset + head.field_shift)?
    };
    if second_side_extent_offset.checked_add(4)? > reference_count_at {
        return None;
    }
    side_extents_at(
        bytes,
        head.direction_face_extend_values[0],
        [first_side_extent_offset, second_side_extent_offset],
    )
}

/// The side-extent discriminators at `offsets` and the extent they state
/// with `direction`.
fn side_extents_at(
    bytes: &[u8],
    direction: u32,
    offsets: [usize; 2],
) -> Option<([usize; 2], [u32; 2], DesignExtrudeExtent)> {
    let discriminators = [
        View::u32_le_at(bytes, offsets[0])?,
        View::u32_le_at(bytes, offsets[1])?,
    ];
    let extent = exact_extrude_extent(direction, discriminators)?;
    Some((offsets, discriminators, extent))
}

/// The legacy shifted Extrude prologue. The reference run has no fixed
/// length, so each membership test is a charged search that stops at the
/// first match.
fn exact_legacy_shifted_extrude_prologue(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Result<Option<DesignExtrudePrologue>, cadmpeg_core::CodecError> {
    let Some(head) = legacy_shifted_head(bytes, start, reference_count_at) else {
        return Ok(None);
    };
    let side_extents = if head.direction_face_extend_values[0] == 2 {
        let Some(offsets) = legacy_shifted_two_sided_offsets(
            ctx,
            bytes,
            start,
            reference_count_at,
            reference_members,
            &head,
        )?
        else {
            return Ok(None);
        };
        side_extents_at(bytes, head.direction_face_extend_values[0], offsets)
    } else {
        legacy_shifted_side_extents(bytes, start, reference_count_at, &head)
    };
    Ok(side_extents.and_then(|side_extents| legacy_shifted_prologue(bytes, &head, side_extents)))
}

/// The legacy shifted prologue with the side extents its caller located.
fn legacy_shifted_prologue(
    bytes: &[u8],
    head: &LegacyShiftedHead,
    (side_extent_discriminator_offsets, side_extent_discriminators, extent): (
        [usize; 2],
        [u32; 2],
        DesignExtrudeExtent,
    ),
) -> Option<DesignExtrudePrologue> {
    let direction_reversed_offset = head
        .operation_offset
        .checked_add(extrude_fields::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = head
        .operation_offset
        .checked_add(extrude_fields::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = head
        .operation_offset
        .checked_add(extrude_fields::START_SUPPORT)?;
    let start = extrude_start_at(bytes, start_offset)?;
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: head.operation_prefix_marker_offset,
        operation: head.operation,
        operation_offset: u64_from_index(head.operation_offset),
        direction_face_extend_values: head.direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64_from_index(side_extent_discriminator_offsets[0]),
            u64_from_index(side_extent_discriminator_offsets[1]),
        ],
        extent: Some(extent),
        direction_face_extend_offsets: [
            u64_from_index(head.first_extent_offset),
            u64_from_index(head.second_extent_offset),
        ],
        direction_reversed,
        direction_reversed_offset: u64_from_index(direction_reversed_offset),
        solid_operation,
        solid_operation_offset: u64_from_index(solid_operation_offset),
        start,
        start_offset: u64_from_index(start_offset),
    })
}

/// The class-338 two-sided distance prologue before its GUID. The layout
/// fixes the reference run at ten members, so each membership test reads at
/// most ten values.
fn class_338_two_sided_distance_fields(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

    if class_tag != "338"
        || paired_class_tag != "262"
        || paired_at.checked_sub(start)? != class_338_legacy::LEN
        || reference_count_at.checked_sub(start)? != class_338_legacy::REFERENCE_COUNT
        || reference_members.len() != 10
        || bytes_at::<3>(bytes, paired_at.checked_add(4)?) != Some(b"262")
        || View::u32_le_at(bytes, start.checked_add(class_338_legacy::PREFIX_CONSTANT)?)?
            != class_338_legacy::PREFIX_CONSTANT_VALUE
        || !zeros_at::<3>(bytes, start.checked_add(24)?)
        || !zeros_at::<3>(bytes, start.checked_add(42)?)
        || bytes_at::<10>(
            bytes,
            start.checked_add(class_338_legacy::NULL_SCOPE_SCALAR_LANE)?,
        ) != Some(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        || View::u32_le_at(bytes, reference_count_at)? != class_338_legacy::REFERENCE_COUNT_VALUE
    {
        return None;
    }
    let operation_offset = start.checked_add(class_338_legacy::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let direction_face_extend_offsets = [
        start.checked_add(class_338_legacy::DIRECTION)?,
        start.checked_add(class_338_legacy::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values
        != [
            class_338_legacy::DIRECTION_VALUE,
            class_338_legacy::FACE_EXTEND_VALUE,
        ]
    {
        return None;
    }
    let direction_reversed_offset = start.checked_add(class_338_legacy::DIRECTION_REVERSED)?;
    let direction_reversed = flag_byte_at(bytes, direction_reversed_offset)?;
    let solid_operation_offset = start.checked_add(class_338_legacy::GEOMETRY_KIND)?;
    let solid_operation = flag_byte_at(bytes, solid_operation_offset)?;
    let start_offset = start.checked_add(class_338_legacy::START_SUPPORT)?;
    let start_support = extrude_start_at(bytes, start_offset)?;
    let profile_normal = f64s_at::<3>(bytes, start.checked_add(class_338_legacy::PROFILE_NORMAL)?)?;
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
    for offset in [
        class_338_legacy::FIRST_SIDE_PARAMETER_REFERENCE,
        class_338_legacy::PROFILE_GROUP_REFERENCE,
        class_338_legacy::BODY_GROUP_REFERENCE,
    ] {
        let record_index = marked_record_reference(bytes, start.checked_add(offset)?)?;
        if !reference_members.contains(&record_index) {
            return None;
        }
    }
    if !zeros_at::<5>(bytes, start.checked_add(160)?)
        || View::u32_le_at(
            bytes,
            start.checked_add(class_338_legacy::FIRST_SIDE_EXTENT)?,
        )? != class_338_legacy::FIRST_SIDE_EXTENT_VALUE
        || !zeros_at::<8>(bytes, start.checked_add(180)?)
        || View::u32_le_at(
            bytes,
            start.checked_add(class_338_legacy::SECOND_SIDE_EXTENT)?,
        )? != class_338_legacy::SECOND_SIDE_EXTENT_VALUE
    {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators: [
            class_338_legacy::FIRST_SIDE_EXTENT_VALUE,
            class_338_legacy::SECOND_SIDE_EXTENT_VALUE,
        ],
        side_extent_discriminator_offsets: [
            u64::try_from(start.checked_add(class_338_legacy::FIRST_SIDE_EXTENT)?).ok()?,
            u64::try_from(start.checked_add(class_338_legacy::SECOND_SIDE_EXTENT)?).ok()?,
        ],
        extent: Some(DesignExtrudeExtent::TwoSidedDistance),
        direction_face_extend_offsets: [
            u64::try_from(direction_face_extend_offsets[0]).ok()?,
            u64::try_from(direction_face_extend_offsets[1]).ok()?,
        ],
        direction_reversed,
        direction_reversed_offset: u64::try_from(direction_reversed_offset).ok()?,
        solid_operation,
        solid_operation_offset: u64::try_from(solid_operation_offset).ok()?,
        start: start_support,
        start_offset: u64::try_from(start_offset).ok()?,
    })
}

/// The class-338 two-sided distance Extrude prologue. Its GUID ends three
/// zero bytes before the reference count.
fn exact_class_338_two_sided_distance_extrude_prologue(
    bytes: &[u8],
    frame: ExtrudeScopeFrame<'_>,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    let ExtrudeScopeFrame {
        start,
        paired_at,
        class_tag,
        paired_class_tag,
        reference_count_at,
    } = frame;
    let prologue = class_338_two_sided_distance_fields(
        bytes,
        start,
        paired_at,
        class_tag,
        paired_class_tag,
        reference_count_at,
        reference_members,
    )?;
    // The fields lie inside the frame, so these offsets do not overflow.
    let guid_end = start + 279;
    (fixed_guid_end(bytes, start + class_338_legacy::GUID) == Some(guid_end)
        && zeros_at::<3>(bytes, guid_end))
    .then_some(prologue)
}

#[cfg(test)]
mod tests;
