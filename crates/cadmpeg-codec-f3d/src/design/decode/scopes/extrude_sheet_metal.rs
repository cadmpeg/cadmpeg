// SPDX-License-Identifier: Apache-2.0
//! Exact extrude admission and sheet-metal operation frames.

use super::{
    class_296_legacy_distance, class_296_legacy_scalar_54, class_296_legacy_scalar_70,
    class_296_legacy_to_face, class_296_symmetric, class_296_to_face, class_296_two_faces,
    class_338_legacy, class_415, compact_extrude, compact_extrude_extent, compact_extrude_mixed,
    early_absent, early_present, edge_flange, edge_flange_286_per_edge, edge_flange_325_per_edge,
    edge_flange_364_width, edge_flange_legacy, edge_flange_multi, exact_fixed_scalar,
    extrude_extent_pair, extrude_fields, extrude_target, f64s_at, flange_to_object, hem_gap,
    hem_rolled, hem_teardrop, is_guid_relaxed, legacy_class_397, legacy_class_415,
    lp_utf16_bounded, marked_record_reference, native_stream, offset_lane, shifted_283,
    shifted_extrude, shifted_reference_aware, shifted_reference_aware_323_symmetric,
    shifted_reference_aware_323_tail, take_reference, DesignBaseFlangeOperation,
    DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeOperation,
    DesignEdgeFlangeWidthParameterSource, DesignEdgeWidthMode, DesignExtrudeExtent,
    DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudePrologueReference,
    DesignExtrudeStart, DesignExtrudeTargetOrdinal, DesignHemOperation, DesignHemParameterOwners,
    DesignParameter, DesignParameterOwner, DesignParameterScope, DesignRuledSurfaceCorner,
    DesignRuledSurfaceMethod, DesignRuledSurfaceOperation, DesignSheetMetalHeightDatum,
    DesignSurfaceStitchOperation, HashSet, IndexedRecordOffsets, View,
};

pub(super) fn exact_extrude_prologue(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    let legacy_class_415 = paired_at
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
    if class_tag == "415" && paired_class_tag == "265" {
        if legacy_class_415 {
            return exact_current_extrude_prologue(
                bytes,
                start,
                reference_count_at,
                reference_members,
                true,
            );
        }
        return legacy_class_415::exact_one_sided_extrude_prologue(
            bytes,
            start,
            paired_at,
            class_tag,
            paired_class_tag,
            reference_count_at,
            reference_members,
        );
    }
    exact_current_extrude_prologue(
        bytes,
        start,
        reference_count_at,
        reference_members,
        legacy_class_415,
    )
    .or_else(|| {
        exact_shifted_reference_aware_extrude_prologue(
            bytes,
            start,
            reference_count_at,
            reference_members,
        )
    })
    .or_else(|| {
        legacy_class_397::exact_symmetric_extrude_prologue(
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
        exact_class_338_two_sided_distance_extrude_prologue(
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
        exact_legacy_shifted_extrude_prologue(bytes, start, reference_count_at, reference_members)
    })
    .or_else(|| exact_compact_shifted_extrude_prologue(bytes, start, reference_count_at))
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
    .or_else(|| exact_legacy_distance_extrude_prologue(bytes, start, reference_count_at))
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
        && reference_count_delta == class_296_to_face::REFERENCE_COUNT as u64
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
        && reference_count_delta == class_296_symmetric::REFERENCE_COUNT as u64
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
        && reference_count_delta == class_296_two_faces::REFERENCE_COUNT as u64
        && (frame_length, reference_member_count) == (536, 13)
}

pub(crate) fn is_class_296_two_sided_to_faces_scope(scope: &DesignParameterScope) -> bool {
    is_class_296_two_sided_to_faces_layout(
        &scope.class_tag,
        &scope.paired_class_tag,
        scope.frame_length,
        scope
            .reference_count_offset
            .saturating_sub(scope.byte_offset),
        scope.reference_members.len(),
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
        && reference_count_delta == class_296_legacy_to_face::REFERENCE_COUNT as u64
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
        && reference_count_delta == class_296_legacy_distance::REFERENCE_COUNT as u64
        && (frame_length, reference_member_count) == (483, 10)
}

fn exact_compact_shifted_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
) -> Option<DesignExtrudePrologue> {
    if View::u32_le_at(bytes, start.checked_add(compact_extrude::PREFIX_CONSTANT)?)? != 1
        || bytes.get(
            start.checked_add(compact_extrude::ZERO_RUN_2)?
                ..start.checked_add(compact_extrude::OPERATION)?,
        )? != [0; 2]
        || reference_count_at.checked_sub(start)? != compact_extrude_extent::REFERENCE_COUNT
    {
        return None;
    }
    let operation_offset = start.checked_add(compact_extrude::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
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
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(compact_extrude::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(compact_extrude::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker: None,
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: operation_offset as u64,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            side_extent_discriminator_offsets[0] as u64,
            side_extent_discriminator_offsets[1] as u64,
        ],
        extent: Some(extent),
        direction_face_extend_offsets: [
            direction_face_extend_offsets[0] as u64,
            direction_face_extend_offsets[1] as u64,
        ],
        direction_reversed,
        direction_reversed_offset: direction_reversed_offset as u64,
        solid_operation,
        solid_operation_offset: solid_operation_offset as u64,
        start: start_support,
        start_offset: start_offset as u64,
    })
}

fn exact_compact_shifted_extrude_mixed_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    if View::u32_le_at(bytes, start.checked_add(compact_extrude::PREFIX_CONSTANT)?)? != 1
        || bytes.get(
            start.checked_add(compact_extrude::ZERO_RUN_2)?
                ..start.checked_add(compact_extrude::OPERATION)?,
        )? != [0; 2]
        || reference_count_at.checked_sub(start)? != compact_extrude_mixed::REFERENCE_COUNT
        || reference_members.len() != 11
    {
        return None;
    }
    let operation_offset = start.checked_add(compact_extrude::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
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
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(compact_extrude::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(compact_extrude::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker: None,
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
        || bytes.get(
            start.checked_add(class_296_to_face::ZERO_RUN_2)?
                ..start.checked_add(class_296_to_face::OPERATION)?,
        )? != [0; 2]
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_to_face::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
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
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(class_296_to_face::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(class_296_to_face::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker: None,
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
        || bytes.get(
            start.checked_add(class_296_symmetric::ZERO_RUN_2)?
                ..start.checked_add(class_296_symmetric::OPERATION)?,
        )? != [0; 2]
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_symmetric::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
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
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(class_296_symmetric::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(class_296_symmetric::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker: None,
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
        || bytes.get(
            start.checked_add(class_296_two_faces::ZERO_RUN_2)?
                ..start.checked_add(class_296_two_faces::OPERATION)?,
        )? != [0; 2]
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_two_faces::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
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
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(class_296_two_faces::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(class_296_two_faces::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    if bytes.get(
        start.checked_add(class_296_two_faces::ZERO_RUN_3_AFTER_START)?
            ..start.checked_add(class_296_two_faces::PROFILE_NORMAL)?,
    )? != [0; 3]
    {
        return None;
    }
    let profile_normal = f64s_at(
        bytes,
        start.checked_add(class_296_two_faces::PROFILE_NORMAL)?,
        3,
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
        let present = match bytes.get(slot_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
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
        operation_prefix_marker: None,
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
        || bytes.get(
            start.checked_add(class_296_legacy_scalar_54::ZERO_BEFORE_REFERENCE)?
                ..start.checked_add(class_296_legacy_scalar_54::REFERENCE)?,
        )? != [0]
        || !reference_members.contains(&marked_record_reference(
            bytes,
            start.checked_add(class_296_legacy_scalar_54::REFERENCE)?,
        )?)
        || bytes.get(
            start
                .checked_add(class_296_legacy_scalar_54::REFERENCE)?
                .checked_add(5)?
                ..start.checked_add(class_296_legacy_scalar_54::OPERATION)?,
        )? != [0; 6]
    {
        return None;
    }
    let operation_offset = start.checked_add(class_296_legacy_scalar_54::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
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
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(class_296_legacy_scalar_54::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(class_296_legacy_scalar_54::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    if bytes.get(
        start.checked_add(class_296_legacy_scalar_54::ZERO_AFTER_START)?
            ..start.checked_add(class_296_legacy_scalar_54::PROFILE_SCALAR_AT_54)?,
    )? != [0; 3]
    {
        return None;
    }
    let profile_scalar_at_54 = f64s_at(
        bytes,
        start.checked_add(class_296_legacy_scalar_54::PROFILE_SCALAR_AT_54)?,
        1,
    )?
    .into_iter()
    .next()?;
    let profile_scalar_at_70 = f64s_at(
        bytes,
        start.checked_add(class_296_legacy_scalar_70::PROFILE_SCALAR_AT_70)?,
        1,
    )?
    .into_iter()
    .next()?;
    let scalar_at_54 = profile_scalar_at_54.is_finite()
        && (profile_scalar_at_54.abs() - 1.0).abs() <= PROFILE_NORMAL_UNIT_EPS
        && bytes.get(
            start.checked_add(class_296_legacy_scalar_54::ZERO_AFTER_SCALAR_AT_54)?
                ..start.checked_add(class_296_legacy_scalar_54::REFERENCE_SLOTS)?,
        )? == [0; 16];
    let scalar_at_70 = profile_scalar_at_70.is_finite()
        && (profile_scalar_at_70.abs() - 1.0).abs() <= PROFILE_NORMAL_UNIT_EPS
        && bytes.get(
            start.checked_add(class_296_legacy_scalar_70::ZERO_BEFORE_SCALAR_AT_70)?
                ..start.checked_add(class_296_legacy_scalar_70::PROFILE_SCALAR_AT_70)?,
        )? == [0; 16];
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
        let present = match bytes.get(slot_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
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
        operation_prefix_marker: None,
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
    let (prefix_value, prefix_value_offset, operation_offset, expected_reference_count_delta) =
        match bytes.get(marker_offset)? {
            0 => (None, None, start.checked_add(early_absent::OPERATION)?, 208),
            1 => {
                let prefix_value_offset = start.checked_add(early_present::PREFIX_VALUE)?;
                let prefix_value = View::u32_le_at(bytes, prefix_value_offset)?;
                if prefix_value != 0 {
                    return None;
                }
                (
                    Some(prefix_value),
                    Some(prefix_value_offset),
                    start.checked_add(early_present::OPERATION)?,
                    212,
                )
            }
            _ => return None,
        };
    if reference_count_at.checked_sub(start)? != expected_reference_count_delta {
        return None;
    }
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let extent_kind_offset = operation_offset.checked_add(4)?;
    let extent_kind = View::u32_le_at(bytes, extent_kind_offset)?;
    if extent_kind != 2 {
        return None;
    }
    let direction_reversed_offset = extent_kind_offset.checked_add(4)?;
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let geometry_kind_offset = direction_reversed_offset.checked_add(1)?;
    let geometry_kind = View::u32_le_at(bytes, geometry_kind_offset)?;
    if !matches!(geometry_kind, 0 | 1) {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyDistance {
        prefix_value,
        prefix_value_offset: prefix_value_offset.map(|offset| offset as u64),
        operation,
        operation_offset: operation_offset as u64,
        extent_kind,
        extent_kind_offset: extent_kind_offset as u64,
        direction_reversed,
        direction_reversed_offset: direction_reversed_offset as u64,
        geometry_kind,
        geometry_kind_offset: geometry_kind_offset as u64,
    })
}

fn exact_current_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
    legacy_class_415_symmetric_distance: bool,
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

    if legacy_class_415_symmetric_distance
        && (View::u32_le_at(bytes, start.checked_add(class_415::PREFIX_CONSTANT)?)? != 1
            || bytes.get(
                start.checked_add(class_415::ZERO_RUN_3)?
                    ..start.checked_add(class_415::OPERATION_PREFIX_MARKER)?,
            )? != [0; 3]
            || bytes.get(start.checked_add(class_415::OPERATION_PREFIX_MARKER)?) != Some(&1))
    {
        return None;
    }
    let direct_offset = start.checked_add(28)?;
    let reference = if bytes.get(start.checked_add(25)?) == Some(&1) {
        let reference_record_index_offset = start.checked_add(26)?;
        let record_index = View::u32_le_at(bytes, reference_record_index_offset)?;
        let prefix_tail = start.checked_add(30)?;
        let candidates = [
            (start.checked_add(37)?, None),
            (start.checked_add(38)?, None),
            (start.checked_add(38)?, Some(start.checked_add(37)?)),
        ]
        .into_iter()
        .filter(|(operation_offset, marker_offset)| {
            let padding_end = marker_offset.unwrap_or(*operation_offset);
            bytes
                .get(prefix_tail..padding_end)
                .is_some_and(|padding| padding.iter().all(|byte| *byte == 0))
                && marker_offset.is_none_or(|offset| bytes.get(offset) == Some(&1))
                && reference_members.contains(&record_index)
                && matches!(View::u32_le_at(bytes, *operation_offset), Some(1..=4))
                && matches!(
                    View::u32_le_at(bytes, operation_offset.saturating_add(4)),
                    Some(1..=3)
                )
                && View::u32_le_at(bytes, operation_offset.saturating_add(8)).is_some()
                && matches!(bytes.get(operation_offset.saturating_add(12)), Some(0 | 1))
                && matches!(bytes.get(operation_offset.saturating_add(13)), Some(0 | 1))
                && matches!(bytes.get(operation_offset.saturating_add(14)), Some(0..=2))
        })
        .collect::<Vec<_>>();
        let [(operation_offset, operation_marker_offset)] = candidates.as_slice() else {
            return None;
        };
        let padding_end = operation_marker_offset.unwrap_or(*operation_offset);
        let trailing_zero_count = u8::try_from(padding_end.checked_sub(prefix_tail)?).ok()?;
        Some((
            *operation_offset,
            DesignExtrudePrologueReference {
                record_index,
                record_index_offset: reference_record_index_offset as u64,
                trailing_zero_count,
                operation_prefix_marker: operation_marker_offset.map(|_| 1),
                operation_prefix_marker_offset: operation_marker_offset
                    .and_then(|offset| u64::try_from(offset).ok()),
            },
        ))
    } else {
        None
    };
    let (operation_offset, reference) = reference
        .map_or((direct_offset, None), |(offset, reference)| {
            (offset, Some(reference))
        });
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
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
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = operation_offset.checked_add(extrude_fields::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = operation_offset.checked_add(extrude_fields::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    let profile_normal_offset = operation_offset.checked_add(extrude_fields::PROFILE_NORMAL)?;
    if bytes
        .get(operation_offset.checked_add(extrude_fields::ZERO_RUN_3)?..profile_normal_offset)?
        != [0; 3]
    {
        return None;
    }
    let profile_normal = f64s_at(bytes, profile_normal_offset, 3)?;
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
    let mut extent_cursor = profile_normal_offset.checked_add(24)?;
    let mut final_slot_reference = None;
    let mut slot_presence = [false; 7];
    for (slot_ordinal, slot_present) in slot_presence.iter_mut().enumerate() {
        match bytes.get(extent_cursor)? {
            0 => extent_cursor = extent_cursor.checked_add(1)?,
            1 => {
                *slot_present = true;
                let record_index = marked_record_reference(bytes, extent_cursor)?;
                if !reference_members.contains(&record_index) {
                    return None;
                }
                if slot_ordinal == 6 {
                    final_slot_reference = Some(record_index);
                }
                extent_cursor = extent_cursor.checked_add(11)?;
            }
            _ => return None,
        }
    }
    if legacy_class_415_symmetric_distance
        && slot_presence != [false, true, true, true, false, true, false]
    {
        return None;
    }
    let first_side_target_ordinal = final_slot_reference.and_then(|record_index| {
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
            scope_reference_ordinal_offset: extent_cursor as u64,
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
        if bytes.get(first_side_extent_offset.checked_add(4)?..second_side_extent_offset)? != [0; 9]
        {
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
            || direction_face_extend_values != [3, 2]
            || side_extent_discriminators != [1, 1])
    {
        return None;
    }
    let extent = exact_extrude_extent(direction_face_extend_values[0], side_extent_discriminators)
        .or_else(|| {
            (legacy_class_415_symmetric_distance
                && direction_face_extend_values == [3, 2]
                && side_extent_discriminators == [1, 1])
            .then_some(DesignExtrudeExtent::SymmetricDistance)
        })?;
    Some(DesignExtrudePrologue::ReferenceAware {
        reference,
        operation,
        operation_offset: operation_offset as u64,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            first_side_extent_offset as u64,
            second_side_extent_offset as u64,
        ],
        first_side_target_ordinal,
        extent,
        direction_face_extend_offsets: [direction_offset as u64, face_extend_offset as u64],
        direction_reversed,
        direction_reversed_offset: direction_reversed_offset as u64,
        solid_operation,
        solid_operation_offset: solid_operation_offset as u64,
        start: start_support,
        start_offset: start_offset as u64,
    })
}

fn exact_shifted_reference_aware_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;
    const CLASS_TAG_OFFSET: usize = 4;
    const CLASS_TAG_LENGTH: usize = 3;

    let primary_class = bytes.get(
        start.checked_add(CLASS_TAG_OFFSET)?
            ..start.checked_add(CLASS_TAG_OFFSET + CLASS_TAG_LENGTH)?,
    )?;
    let has_reference_layout = |reference_count: usize, reference_count_at: usize| {
        reference_count_at.checked_sub(start) == Some(reference_count)
    };
    let (
        frame_length,
        reference_count_offset,
        reference_member_count,
        expected_paired_class,
        trailing_reference_count_offset,
        trailing_reference_offset,
        trailing_reference_padding_offset,
        guid_prefix_offset,
        second_side_extent_offset,
        trailing_reference_is_ordered,
        symmetric_through_all,
    ) = match primary_class {
        b"357" | b"275" | b"361" | b"349" | b"397" => (
            538,
            shifted_reference_aware::REFERENCE_COUNT,
            13,
            match primary_class {
                b"357" => &b"258"[..],
                b"275" | b"361" => &b"262"[..],
                b"349" => &b"266"[..],
                b"397" => &b"262"[..],
                _ => unreachable!(),
            },
            shifted_reference_aware::BODY_GROUP_COUNT,
            shifted_reference_aware::BODY_GROUP_REFERENCE,
            shifted_reference_aware::BODY_GROUP_REFERENCE + 11,
            shifted_reference_aware::BODY_GROUP_GUID_PREFIX,
            shifted_reference_aware::SECOND_SIDE_EXTENT,
            true,
            false,
        ),
        b"323"
            if has_reference_layout(
                shifted_reference_aware::REFERENCE_COUNT,
                reference_count_at,
            ) && reference_members.len() == 11 =>
        {
            (
                516,
                shifted_reference_aware::REFERENCE_COUNT,
                11,
                &b"263"[..],
                shifted_reference_aware_323_tail::TRAILING_REFERENCE_COUNT,
                shifted_reference_aware_323_tail::TRAILING_REFERENCE,
                shifted_reference_aware_323_tail::TRAILING_REFERENCE_PADDING,
                shifted_reference_aware::BODY_GROUP_GUID_PREFIX,
                shifted_reference_aware::SECOND_SIDE_EXTENT,
                false,
                false,
            )
        }
        b"323"
            if has_reference_layout(
                shifted_reference_aware_323_symmetric::REFERENCE_COUNT,
                reference_count_at,
            ) && reference_members.len() == 10 =>
        {
            (
                485,
                shifted_reference_aware_323_symmetric::REFERENCE_COUNT,
                10,
                &b"263"[..],
                shifted_reference_aware_323_symmetric::TRAILING_REFERENCE_COUNT,
                shifted_reference_aware_323_symmetric::TRAILING_REFERENCE,
                shifted_reference_aware_323_symmetric::GUID_PREFIX,
                shifted_reference_aware_323_symmetric::GUID_PREFIX,
                shifted_reference_aware_323_symmetric::SECOND_SIDE_EXTENT,
                true,
                true,
            )
        }
        _ => return None,
    };
    if reference_count_at.checked_sub(start)? != reference_count_offset
        || reference_members.len() != reference_member_count
        || View::u32_le_at(
            bytes,
            start.checked_add(shifted_reference_aware::PREFIX_CONSTANT)?,
        )? != 1
        || bytes.get(
            start.checked_add(shifted_reference_aware::ZERO_RUN_3)?
                ..start.checked_add(shifted_reference_aware::OPERATION)?,
        )? != [0; 3]
        || bytes.get(
            start.checked_add(shifted_reference_aware::ZERO_RUN_3_AFTER_START)?
                ..start.checked_add(shifted_reference_aware::PROFILE_NORMAL)?,
        )? != [0; 3]
    {
        return None;
    }
    let paired_start = start.checked_add(frame_length)?;
    let paired_class = bytes.get(
        paired_start.checked_add(CLASS_TAG_OFFSET)?
            ..paired_start.checked_add(CLASS_TAG_OFFSET + CLASS_TAG_LENGTH)?,
    )?;
    if paired_class != expected_paired_class {
        return None;
    }
    let operation_offset = start.checked_add(shifted_reference_aware::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let direction_face_extend_offsets = [
        start.checked_add(shifted_reference_aware::DIRECTION)?,
        start.checked_add(shifted_reference_aware::FACE_EXTEND)?,
    ];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    let expected_direction_face_extend = if symmetric_through_all {
        [3, 0]
    } else {
        [2, 1]
    };
    if direction_face_extend_values != expected_direction_face_extend {
        return None;
    }
    let direction_reversed_offset =
        start.checked_add(shifted_reference_aware::DIRECTION_REVERSED)?;
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(shifted_reference_aware::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(shifted_reference_aware::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    let profile_normal_offset = start.checked_add(shifted_reference_aware::PROFILE_NORMAL)?;
    let profile_normal = f64s_at(bytes, profile_normal_offset, 3)?;
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
        let present = match bytes.get(slot_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
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
    let second_side_extent_offset = if symmetric_through_all {
        start.checked_add(second_side_extent_offset)?
    } else {
        reference_count_at.checked_sub(4)?
    };
    let side_extent_discriminators = [
        View::u32_le_at(bytes, first_side_extent_offset)?,
        View::u32_le_at(bytes, second_side_extent_offset)?,
    ];
    let extent = exact_extrude_extent(direction_face_extend_values[0], side_extent_discriminators)?;
    let expected_side_extent_discriminators = if symmetric_through_all {
        [4, 4]
    } else {
        [2, 0]
    };
    if side_extent_discriminators != expected_side_extent_discriminators {
        return None;
    }
    let ordered_tail_references_valid = if symmetric_through_all {
        [
            marked_record_reference(
                bytes,
                start.checked_add(
                    shifted_reference_aware_323_symmetric::SYMMETRIC_EXTENT_REFERENCE,
                )?,
            )?,
            marked_record_reference(
                bytes,
                start
                    .checked_add(shifted_reference_aware_323_symmetric::PROFILE_GROUP_REFERENCE)?,
            )?,
        ]
        .iter()
        .all(|record_index| reference_members.contains(record_index))
    } else {
        [
            marked_record_reference(
                bytes,
                start.checked_add(shifted_reference_aware::FIRST_SIDE_OWNER_REFERENCE)?,
            )?,
            marked_record_reference(
                bytes,
                start.checked_add(shifted_reference_aware::SECOND_SIDE_OFFSET_REFERENCE)?,
            )?,
            marked_record_reference(
                bytes,
                start.checked_add(shifted_reference_aware::SECOND_SIDE_TAPER_REFERENCE)?,
            )?,
            marked_record_reference(
                bytes,
                start.checked_add(shifted_reference_aware::PROFILE_GROUP_REFERENCE)?,
            )?,
        ]
        .iter()
        .all(|record_index| reference_members.contains(record_index))
    };
    let trailing_reference =
        marked_record_reference(bytes, start.checked_add(trailing_reference_offset)?)?;
    let trailing_reference_valid = if trailing_reference_is_ordered {
        reference_members.contains(&trailing_reference)
    } else {
        trailing_reference != 0 && !reference_members.contains(&trailing_reference)
    };
    let zero_range = |range_start: usize, range_end: usize| {
        bytes
            .get(start + range_start..start + range_end)
            .is_some_and(|value| value.iter().all(|byte| *byte == 0))
    };
    let tail_fixed_valid = if symmetric_through_all {
        zero_range(
            shifted_reference_aware_323_symmetric::FIRST_SIDE_PADDING,
            shifted_reference_aware_323_symmetric::SECOND_SIDE_EXTENT,
        ) && View::u32_le_at(
            bytes,
            start + shifted_reference_aware_323_symmetric::FIRST_SIDE_EXTENT,
        ) == Some(shifted_reference_aware_323_symmetric::FIRST_SIDE_EXTENT_VALUE)
            && zero_range(
                shifted_reference_aware_323_symmetric::SECOND_SIDE_PADDING,
                shifted_reference_aware_323_symmetric::SYMMETRIC_EXTENT_REFERENCE,
            )
            && zero_range(
                shifted_reference_aware_323_symmetric::SYMMETRIC_EXTENT_PADDING,
                shifted_reference_aware_323_symmetric::PROFILE_GROUP_COUNT,
            )
            && View::u32_le_at(
                bytes,
                start + shifted_reference_aware_323_symmetric::PROFILE_GROUP_COUNT,
            ) == Some(shifted_reference_aware_323_symmetric::PROFILE_GROUP_COUNT_VALUE)
            && zero_range(
                shifted_reference_aware_323_symmetric::PROFILE_GROUP_PADDING,
                shifted_reference_aware_323_symmetric::TRAILING_REFERENCE_COUNT,
            )
            && View::u32_le_at(
                bytes,
                start + shifted_reference_aware_323_symmetric::TRAILING_REFERENCE_COUNT,
            ) == Some(shifted_reference_aware_323_symmetric::TRAILING_REFERENCE_COUNT_VALUE)
    } else {
        bytes
            .get(
                start + shifted_reference_aware::FIRST_SIDE_PADDING
                    ..start + shifted_reference_aware::FIRST_SIDE_DISCRIMINANT,
            )
            .is_some_and(|value| value == [0; 4])
            && View::u32_le_at(
                bytes,
                start + shifted_reference_aware::FIRST_SIDE_DISCRIMINANT,
            ) == Some(1)
            && View::u32_le_at(bytes, start + shifted_reference_aware::FIRST_SIDE_PAYLOAD)
                == Some(2)
            && bytes.get(start + shifted_reference_aware::FIRST_SIDE_SEPARATOR) == Some(&0)
            && bytes
                .get(
                    start + shifted_reference_aware::SECOND_SIDE_OFFSET_PADDING
                        ..start + shifted_reference_aware::SECOND_SIDE_TAPER_REFERENCE,
                )
                .is_some_and(|value| value == [0; 4])
            && bytes
                .get(
                    start + shifted_reference_aware::SECOND_SIDE_TAPER_PADDING
                        ..start + shifted_reference_aware::PROFILE_GROUP_COUNT,
                )
                .is_some_and(|value| value == [0; 5])
            && View::u32_le_at(bytes, start + shifted_reference_aware::PROFILE_GROUP_COUNT)
                == Some(1)
            && zero_range(
                shifted_reference_aware::PROFILE_GROUP_PADDING,
                trailing_reference_count_offset,
            )
            && View::u32_le_at(bytes, start + trailing_reference_count_offset) == Some(1)
            && zero_range(
                trailing_reference_padding_offset,
                shifted_reference_aware::BODY_GROUP_GUID_PREFIX,
            )
    };
    if !ordered_tail_references_valid || !trailing_reference_valid || !tail_fixed_valid {
        return None;
    }
    let (guid, guid_end) =
        lp_utf16_bounded(bytes, start.checked_add(guid_prefix_offset)?, 36..=36)?;
    let expected_guid_end = if symmetric_through_all {
        start.checked_add(guid_prefix_offset)?.checked_add(76)?
    } else {
        second_side_extent_offset.checked_add(1)?
    };
    if !is_guid_relaxed(&guid)
        || guid_end != expected_guid_end
        || bytes.get(guid_end..reference_count_at)? != [0; 3]
    {
        return None;
    }
    Some(DesignExtrudePrologue::ShiftedReferenceAware {
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64::try_from(first_side_extent_offset).ok()?,
            u64::try_from(second_side_extent_offset).ok()?,
        ],
        extent,
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

pub(crate) fn exact_extrude_extent(
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

fn exact_legacy_shifted_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    if View::u32_le_at(bytes, start.checked_add(shifted_extrude::PREFIX_CONSTANT)?)? != 1
        || bytes.get(
            start.checked_add(shifted_extrude::ZERO_RUN_3)?
                ..start.checked_add(shifted_extrude::OPERATION)?,
        )? != [0; 3]
    {
        return None;
    }
    let marker_offset = start.checked_add(shifted_extrude::OPERATION)?;
    let (operation_prefix_marker, operation_prefix_marker_offset, field_shift) =
        if matches!(View::u32_le_at(bytes, marker_offset), Some(1..=4)) {
            (None, None, 0)
        } else if bytes.get(marker_offset) == Some(&1)
            && matches!(
                View::u32_le_at(bytes, marker_offset.checked_add(1)?),
                Some(1..=4)
            )
        {
            (Some(1), Some(marker_offset as u64), 1)
        } else {
            return None;
        };
    let operation_offset = marker_offset.checked_add(field_shift)?;
    let reference_count_delta = reference_count_at
        .checked_sub(start)?
        .checked_sub(field_shift)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let first_extent_offset = operation_offset.checked_add(4)?;
    let second_extent_offset = operation_offset.checked_add(8)?;
    let direction_face_extend_values = [
        View::u32_le_at(bytes, first_extent_offset)?,
        View::u32_le_at(bytes, second_extent_offset)?,
    ];
    if !matches!(direction_face_extend_values[0], 1..=3) {
        return None;
    }
    let two_sided_offsets = || {
        if reference_count_delta == 283 {
            let first_parameter_at =
                start.checked_add(shifted_283::FIRST_PARAMETER_REFERENCE + field_shift)?;
            let first_side_extent_offset =
                start.checked_add(shifted_283::FIRST_SIDE_EXTENT + field_shift)?;
            let second_parameter_at =
                start.checked_add(shifted_283::SECOND_PARAMETER_REFERENCE + field_shift)?;
            let second_side_extent_offset =
                start.checked_add(shifted_283::SECOND_SIDE_EXTENT + field_shift)?;
            let compact_valid = bytes
                .get(start.checked_add(150 + field_shift)?..first_side_extent_offset)?
                == [0; 16]
                && bytes.get(start.checked_add(175 + field_shift)?..second_side_extent_offset)?
                    == [0; 6]
                && [first_parameter_at, second_parameter_at]
                    .into_iter()
                    .map(|offset| marked_record_reference(bytes, offset))
                    .all(|reference| {
                        reference.is_some_and(|value| reference_members.contains(&value))
                    })
                && marked_record_reference(
                    bytes,
                    start.checked_add(shifted_283::TRAILING_ENTITY_REFERENCE + field_shift)?,
                )
                .is_some()
                && bytes.get(
                    start.checked_add(shifted_283::ZERO_RUN_8 + field_shift)?
                        ..start.checked_add(shifted_283::LEN + field_shift)?,
                )? == [0; 8];
            if compact_valid {
                return Some([first_side_extent_offset, second_side_extent_offset]);
            }
        }
        let first_parameter_at = start.checked_add(139 + field_shift)?;
        let first_side_extent_offset = start.checked_add(155 + field_shift)?;
        let first_offset_at = start.checked_add(159 + field_shift)?;
        let second_side_extent_offset = start.checked_add(178 + field_shift)?;
        let second_parameter_at = start.checked_add(182 + field_shift)?;
        if second_parameter_at.checked_add(11)? > reference_count_at
            || bytes.get(start.checked_add(150 + field_shift)?..first_side_extent_offset)? != [0; 5]
            || bytes.get(start.checked_add(170 + field_shift)?..second_side_extent_offset)?
                != [0; 8]
            || [first_parameter_at, first_offset_at, second_parameter_at]
                .into_iter()
                .map(|offset| marked_record_reference(bytes, offset))
                .any(|reference| !reference.is_some_and(|value| reference_members.contains(&value)))
        {
            return None;
        }
        Some([first_side_extent_offset, second_side_extent_offset])
    };
    let candidate = |first_side_extent_offset: usize, default_second_offset: usize| {
        if first_side_extent_offset.checked_add(4)? > reference_count_at {
            return None;
        }
        let first_side_extent = View::u32_le_at(bytes, first_side_extent_offset)?;
        let second_side_extent_offset = if first_side_extent == 2 {
            reference_count_at.checked_sub(4)?
        } else {
            default_second_offset
        };
        if second_side_extent_offset.checked_add(4)? > reference_count_at {
            return None;
        }
        let offsets = [first_side_extent_offset, second_side_extent_offset];
        let discriminators = [
            View::u32_le_at(bytes, offsets[0])?,
            View::u32_le_at(bytes, offsets[1])?,
        ];
        let extent = exact_extrude_extent(direction_face_extend_values[0], discriminators)?;
        Some((offsets, discriminators, extent))
    };
    let (side_extent_discriminator_offsets, side_extent_discriminators, extent) =
        if direction_face_extend_values[0] == 2 {
            let offsets = two_sided_offsets()?;
            let discriminators = [
                View::u32_le_at(bytes, offsets[0])?,
                View::u32_le_at(bytes, offsets[1])?,
            ];
            (
                offsets,
                discriminators,
                exact_extrude_extent(direction_face_extend_values[0], discriminators)?,
            )
        } else {
            let (first_offset, second_offset) = match reference_count_delta {
                262 if bytes.get(operation_offset.checked_add(extrude_fields::START_SUPPORT)?)
                    == Some(&1) =>
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
            candidate(
                start.checked_add(first_offset + field_shift)?,
                start.checked_add(second_offset + field_shift)?,
            )?
        };
    let direction_reversed_offset =
        operation_offset.checked_add(extrude_fields::DIRECTION_REVERSED)?;
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = operation_offset.checked_add(extrude_fields::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = operation_offset.checked_add(extrude_fields::START_SUPPORT)?;
    let start = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker,
        operation_prefix_marker_offset,
        operation,
        operation_offset: operation_offset as u64,
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            side_extent_discriminator_offsets[0] as u64,
            side_extent_discriminator_offsets[1] as u64,
        ],
        extent: Some(extent),
        direction_face_extend_offsets: [first_extent_offset as u64, second_extent_offset as u64],
        direction_reversed,
        direction_reversed_offset: direction_reversed_offset as u64,
        solid_operation,
        solid_operation_offset: solid_operation_offset as u64,
        start,
        start_offset: start_offset as u64,
    })
}

pub(crate) fn exact_class_338_two_sided_distance_extrude_prologue(
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
        || bytes.get(paired_at.checked_add(4)?..paired_at.checked_add(7)?)? != b"262"
        || View::u32_le_at(bytes, start.checked_add(class_338_legacy::PREFIX_CONSTANT)?)?
            != class_338_legacy::PREFIX_CONSTANT_VALUE
        || bytes.get(start.checked_add(24)?..start.checked_add(class_338_legacy::OPERATION)?)?
            != [0; 3]
        || bytes
            .get(start.checked_add(42)?..start.checked_add(class_338_legacy::PROFILE_NORMAL)?)?
            != [0; 3]
        || bytes.get(
            start.checked_add(class_338_legacy::NULL_SCOPE_SCALAR_LANE)?
                ..start.checked_add(class_338_legacy::NULL_SCOPE_SCALAR_LANE + 10)?,
        )? != [1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        || View::u32_le_at(bytes, reference_count_at)? != class_338_legacy::REFERENCE_COUNT_VALUE
    {
        return None;
    }
    let operation_offset = start.checked_add(class_338_legacy::OPERATION)?;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
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
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start.checked_add(class_338_legacy::GEOMETRY_KIND)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start.checked_add(class_338_legacy::START_SUPPORT)?;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };
    let profile_normal = f64s_at(
        bytes,
        start.checked_add(class_338_legacy::PROFILE_NORMAL)?,
        3,
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
    if bytes
        .get(start.checked_add(160)?..start.checked_add(class_338_legacy::FIRST_SIDE_EXTENT)?)?
        != [0; 5]
        || View::u32_le_at(
            bytes,
            start.checked_add(class_338_legacy::FIRST_SIDE_EXTENT)?,
        )? != class_338_legacy::FIRST_SIDE_EXTENT_VALUE
        || bytes.get(
            start.checked_add(180)?..start.checked_add(class_338_legacy::SECOND_SIDE_EXTENT)?,
        )? != [0; 8]
        || View::u32_le_at(
            bytes,
            start.checked_add(class_338_legacy::SECOND_SIDE_EXTENT)?,
        )? != class_338_legacy::SECOND_SIDE_EXTENT_VALUE
    {
        return None;
    }
    let (guid, guid_end) =
        lp_utf16_bounded(bytes, start.checked_add(class_338_legacy::GUID)?, 36..=36)?;
    let expected_guid_end = start.checked_add(279)?;
    if !is_guid_relaxed(&guid)
        || guid_end != expected_guid_end
        || bytes.get(guid_end..reference_count_at)? != [0; 3]
    {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker: None,
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

pub(crate) fn exact_surface_stitch_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    references: &[u32],
) -> Option<DesignSurfaceStitchOperation> {
    if references.len() < 4 || !references.len().is_multiple_of(2) {
        return None;
    }
    let tolerance_record_index = references[references.len() - 2];
    let settings_record_index = references[references.len() - 1];
    let scalar = exact_fixed_scalar(bytes, records, tolerance_record_index)?;
    if scalar.owner_record_index != Some(scope_record_index) || scalar.ordinal != 0 {
        return None;
    }
    let gap_tolerance = scalar.value;
    if !gap_tolerance.is_finite() || gap_tolerance <= 0.0 {
        return None;
    }
    Some(DesignSurfaceStitchOperation {
        gap_tolerance,
        gap_tolerance_offset: scalar.value_offset,
        tolerance_record_index,
        settings_record_index,
    })
}

pub(crate) fn exact_base_flange_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
) -> Option<DesignBaseFlangeOperation> {
    let [profile_group_record_index, profile_record_index, thickness_record_index, settings_record_index] =
        references
    else {
        return None;
    };
    if paired_at.checked_sub(start)? != 416
        || View::u32_le_at(bytes, start + 73)? != 1
        || bytes.get(start + 81) != Some(&1)
        || View::u32_le_at(bytes, start + 82)? != *settings_record_index
        || bytes.get(start + 86..start + 92)? != [0; 6]
        || View::u32_le_at(bytes, start + 92)? != 1
        || bytes.get(start + 112) != Some(&1)
        || View::u32_le_at(bytes, start + 113)? != *thickness_record_index
        || bytes.get(start + 117..start + 123)? != [0; 6]
        || View::u32_le_at(bytes, start + 141)? != 1
        || bytes.get(start + 145) != Some(&1)
        || View::u32_le_at(bytes, start + 146)? != *profile_group_record_index
        || bytes.get(start + 150..start + 156)? != [0; 6]
    {
        return None;
    }
    let thickness = View::f64_le_at(bytes, start + 123)?;
    if !thickness.is_finite() || thickness <= 0.0 {
        return None;
    }
    Some(DesignBaseFlangeOperation {
        thickness,
        thickness_offset: u64::try_from(start + 123).ok()?,
        profile_group_record_index: *profile_group_record_index,
        profile_record_index: *profile_record_index,
        thickness_record_index: *thickness_record_index,
        settings_record_index: *settings_record_index,
    })
}

pub(crate) fn exact_ruled_surface_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignRuledSurfaceOperation> {
    if bytes.get(start.checked_add(11)?..start.checked_add(20)?)? != [0; 9] {
        return None;
    }
    let method_offset = start.checked_add(20)?;
    let method = match View::u32_le_at(bytes, method_offset)? {
        0 => DesignRuledSurfaceMethod::Tangent,
        1 => DesignRuledSurfaceMethod::Normal,
        2 => DesignRuledSurfaceMethod::Direction,
        _ => return None,
    };
    if bytes.get(start.checked_add(24)?..start.checked_add(27)?)? != [0; 3] {
        return None;
    }
    let alternate_face_offset = start.checked_add(27)?;
    let alternate_face = match bytes.get(alternate_face_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let fixed_reference = |at: usize| {
        let mut cursor = at;
        let reference = take_reference(bytes, &mut cursor)?;
        (cursor == at.checked_add(11)?
            && reference.segment.is_none()
            && reference.link_name.is_none())
        .then(|| u32::try_from(reference.target?).ok())?
    };
    let angle_owner_record_index = fixed_reference(start.checked_add(28)?)?;
    let distance_owner_record_index = fixed_reference(start.checked_add(39)?)?;
    let corner_offset = start.checked_add(50)?;
    let corner = match View::u32_le_at(bytes, corner_offset)? {
        0 => DesignRuledSurfaceCorner::Rounded,
        1 => DesignRuledSurfaceCorner::Mitered,
        _ => return None,
    };
    let take_reference_list = |mut cursor: usize| {
        let count = usize::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
        if count > 100_000 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            records.push(fixed_reference(cursor)?);
            cursor = cursor.checked_add(11)?;
        }
        Some((records, cursor))
    };
    let (mut edge_group_record_indices, mut cursor) = take_reference_list(start.checked_add(54)?)?;
    if View::u32_le_at(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let (auxiliary_record_indices, next) = take_reference_list(cursor)?;
    cursor = next;
    if View::u32_le_at(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let (trailing_edge_groups, next) = take_reference_list(cursor)?;
    cursor = next;
    edge_group_record_indices.extend(trailing_edge_groups);
    let (direction_entity_id, direction_end) = lp_utf16_bounded(bytes, cursor, 36..=36)?;
    let direction_absent = direction_entity_id == "00000000-0000-0000-0000-000000000000";
    if direction_end.checked_add(3)? != reference_count_at
        || bytes.get(direction_end..reference_count_at)? != [0; 3]
        || paired_at <= reference_count_at
        || (!direction_absent && !crate::bytes::is_guid_relaxed(&direction_entity_id))
    {
        return None;
    }
    let direction_entity_id = (!direction_absent).then_some(direction_entity_id);
    if reference_members.first() != Some(&distance_owner_record_index)
        || reference_members.get(1) != Some(&angle_owner_record_index)
        || edge_group_record_indices.is_empty()
        || edge_group_record_indices
            .iter()
            .any(|record_index| !reference_members.contains(record_index))
    {
        return None;
    }
    Some(DesignRuledSurfaceOperation {
        method,
        method_offset: method_offset as u64,
        corner,
        corner_offset: corner_offset as u64,
        alternate_face,
        alternate_face_offset: alternate_face_offset as u64,
        angle_owner_record_index,
        distance_owner_record_index,
        edge_group_record_indices,
        auxiliary_record_indices,
        direction_entity_id,
    })
}

/// Optional four-byte scope-header member widths that shift the fixed operation
/// section of a sheet-metal edge treatment.
///
/// The member is not announced by another field, so the true offset of the fixed
/// section is settled by reference agreement instead: exactly one candidate
/// makes every marked slot name a record the ordered reference table lists.
const SHEET_METAL_HEADER_SHIFTS: [usize; 2] = [0, 4];

/// Largest width-distance parameter-owner count a sheet-metal edge-width mode adds.
///
/// The full-edge mode adds none, the symmetric mode one, and the two-sided mode
/// two. A higher count belongs to a frame form this reader does not account for.
const MAX_EDGE_WIDTH_DISTANCE_OWNERS: usize = 2;

pub(crate) fn exact_edge_flange_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    references: &[u32],
) -> Option<DesignEdgeFlangeOperation> {
    // The legacy form is keyed by both class tags. The current form recovers
    // its optional header shift by agreement, so a frame that reads under more
    // than one candidate is refused as ambiguous.
    let mut resolved = None;
    let classed_candidates = match (class_tag, paired_class_tag) {
        ("325", "258") | ("334", "257") => [
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_SINGLE_EDGE_FLANGE_LAYOUT,
            ),
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_MULTI_EDGE_FLANGE_LAYOUT,
            ),
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS325_TWO_SIDED_PER_EDGE_LAYOUT,
            ),
            None,
        ],
        ("364", "261") => [
            None,
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS364_PER_EDGE_WIDTH_LAYOUT,
            ),
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_MULTI_EDGE_FLANGE_LAYOUT,
            ),
            None,
        ],
        ("286", "258") => [
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS286_TWO_SIDED_PER_EDGE_LAYOUT,
            ),
            legacy_edge_flange_operation_at(
                bytes,
                start,
                paired_at,
                references,
                LEGACY_CLASS286_SINGLE_EDGE_FLANGE_LAYOUT,
            ),
            None,
            None,
        ],
        _ => [None, None, None, None],
    };
    for candidate in classed_candidates.into_iter().flatten().chain(
        SHEET_METAL_HEADER_SHIFTS
            .into_iter()
            .flat_map(|header_shift| {
                [
                    edge_flange_operation_at(bytes, start, paired_at, references, header_shift),
                    edge_flange_to_object_operation_at(
                        bytes,
                        start,
                        paired_at,
                        references,
                        header_shift,
                    ),
                ]
                .into_iter()
                .flatten()
            }),
    ) {
        if resolved.is_some() {
            return None;
        }
        resolved = Some(candidate);
    }
    resolved
}

#[derive(Clone, Copy)]
struct LegacyEdgeFlangeLayout {
    frame_length: usize,
    reference_count: usize,
    bend_position_offset: usize,
    edge_count_offset: usize,
    edge_wrapper_offsets: &'static [usize],
    edge_group_offsets: &'static [usize],
    settings_offset: usize,
    height_datum_offset: usize,
    angle_owner_offset: usize,
    height_owner_offset: usize,
    bend_radius_offset: usize,
    result_count_offset: usize,
    result_reference_start: usize,
    result_trailer_start: usize,
    result_separator_offset: usize,
    aggregate_group_offset: usize,
    aggregate_operand_count: usize,
    width_owner_count: usize,
    auxiliary_reference_count: usize,
    width_mode: DesignEdgeWidthMode,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource,
    result_trailers: &'static [u32],
}

const LEGACY_SINGLE_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 494,
    reference_count: 8,
    bend_position_offset: edge_flange_legacy::BEND_POSITION,
    edge_count_offset: edge_flange_legacy::EDGE_COUNT,
    edge_wrapper_offsets: &[edge_flange_legacy::EDGE_WRAPPER_REFERENCE],
    edge_group_offsets: &[edge_flange_legacy::EDGE_GROUP_REFERENCE],
    settings_offset: edge_flange_legacy::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_legacy::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_legacy::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_legacy::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_legacy::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_legacy::RESULT_COUNT,
    result_reference_start: edge_flange_legacy::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_legacy::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_legacy::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_legacy::AGGREGATE_GROUP_REFERENCE,
    aggregate_operand_count: 1,
    width_owner_count: 0,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::FullEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[1, 0],
};

const LEGACY_MULTI_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 591,
    reference_count: 12,
    bend_position_offset: edge_flange_multi::BEND_POSITION,
    edge_count_offset: edge_flange_multi::EDGE_COUNT,
    edge_wrapper_offsets: &[
        edge_flange_multi::EDGE_WRAPPER_ONE_REFERENCE,
        edge_flange_multi::EDGE_WRAPPER_TWO_REFERENCE,
    ],
    edge_group_offsets: &[
        edge_flange_multi::EDGE_GROUP_ONE_REFERENCE,
        edge_flange_multi::EDGE_GROUP_TWO_REFERENCE,
    ],
    settings_offset: edge_flange_multi::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_multi::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_multi::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_multi::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_multi::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_multi::RESULT_COUNT,
    result_reference_start: edge_flange_multi::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_multi::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_multi::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_multi::AGGREGATE_GROUP_REFERENCE,
    aggregate_operand_count: 2,
    width_owner_count: 0,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::FullEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[1, 1, 0],
};

const LEGACY_CLASS325_TWO_SIDED_PER_EDGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 669,
    reference_count: 16,
    bend_position_offset: edge_flange_325_per_edge::BEND_POSITION,
    edge_count_offset: edge_flange_325_per_edge::EDGE_COUNT,
    edge_wrapper_offsets: &[
        edge_flange_325_per_edge::EDGE_WRAPPER_ONE_REFERENCE,
        edge_flange_325_per_edge::EDGE_WRAPPER_TWO_REFERENCE,
    ],
    edge_group_offsets: &[
        edge_flange_325_per_edge::EDGE_GROUP_ONE_REFERENCE,
        edge_flange_325_per_edge::EDGE_GROUP_TWO_REFERENCE,
    ],
    settings_offset: edge_flange_325_per_edge::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_325_per_edge::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_325_per_edge::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_325_per_edge::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_325_per_edge::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_325_per_edge::RESULT_COUNT,
    result_reference_start: edge_flange_325_per_edge::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_325_per_edge::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_325_per_edge::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_325_per_edge::AGGREGATE_GROUP_REFERENCE,
    aggregate_operand_count: 2,
    width_owner_count: 4,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::TwoSidesPerEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[1, 1, 1, 1, 0],
};

const LEGACY_CLASS364_PER_EDGE_WIDTH_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 643,
    reference_count: 14,
    bend_position_offset: edge_flange_364_width::BEND_POSITION,
    edge_count_offset: edge_flange_364_width::EDGE_COUNT,
    edge_wrapper_offsets: &[
        edge_flange_364_width::EDGE_WRAPPER_ONE_REFERENCE,
        edge_flange_364_width::EDGE_WRAPPER_TWO_REFERENCE,
    ],
    edge_group_offsets: &[
        edge_flange_364_width::EDGE_GROUP_ONE_REFERENCE,
        edge_flange_364_width::EDGE_GROUP_TWO_REFERENCE,
    ],
    settings_offset: edge_flange_364_width::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_364_width::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_364_width::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_364_width::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_364_width::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_364_width::RESULT_COUNT,
    result_reference_start: edge_flange_364_width::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_364_width::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_364_width::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_364_width::AGGREGATE_GROUP_REFERENCE,
    aggregate_operand_count: 2,
    width_owner_count: 2,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::SymmetricPerEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[1, 1, 1, 1, 0],
};

const LEGACY_CLASS286_TWO_SIDED_PER_EDGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 801,
    reference_count: 28,
    bend_position_offset: edge_flange_286_per_edge::BEND_POSITION,
    edge_count_offset: edge_flange_286_per_edge::EDGE_COUNT,
    edge_wrapper_offsets: &[
        edge_flange_286_per_edge::EDGE_WRAPPER_ONE_REFERENCE,
        edge_flange_286_per_edge::EDGE_WRAPPER_TWO_REFERENCE,
    ],
    edge_group_offsets: &[
        edge_flange_286_per_edge::EDGE_GROUP_ONE_REFERENCE,
        edge_flange_286_per_edge::EDGE_GROUP_TWO_REFERENCE,
    ],
    settings_offset: edge_flange_286_per_edge::SETTINGS_REFERENCE,
    height_datum_offset: edge_flange_286_per_edge::HEIGHT_DATUM,
    angle_owner_offset: edge_flange_286_per_edge::ANGLE_OWNER_REFERENCE,
    height_owner_offset: edge_flange_286_per_edge::HEIGHT_OWNER_REFERENCE,
    bend_radius_offset: edge_flange_286_per_edge::INSIDE_BEND_RADIUS,
    result_count_offset: edge_flange_286_per_edge::RESULT_COUNT,
    result_reference_start: edge_flange_286_per_edge::RESULT_ONE_REFERENCE,
    result_trailer_start: edge_flange_286_per_edge::RESULT_ONE_TRAILER,
    result_separator_offset: edge_flange_286_per_edge::RESULT_SEPARATOR,
    aggregate_group_offset: edge_flange_286_per_edge::AGGREGATE_GROUP_REFERENCE,
    aggregate_operand_count: 2,
    width_owner_count: 4,
    auxiliary_reference_count: 12,
    width_mode: DesignEdgeWidthMode::TwoSidesPerEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeOffset,
    result_trailers: &[1, 1, 1, 1, 0],
};

const LEGACY_CLASS286_SINGLE_EDGE_FLANGE_LAYOUT: LegacyEdgeFlangeLayout = LegacyEdgeFlangeLayout {
    frame_length: 483,
    reference_count: 8,
    bend_position_offset: 80,
    edge_count_offset: 84,
    edge_wrapper_offsets: &[88],
    edge_group_offsets: &[196],
    settings_offset: 99,
    height_datum_offset: 110,
    angle_owner_offset: 114,
    height_owner_offset: 125,
    bend_radius_offset: 142,
    result_count_offset: 150,
    result_reference_start: 154,
    result_trailer_start: 165,
    result_separator_offset: 169,
    aggregate_group_offset: 173,
    aggregate_operand_count: 1,
    width_owner_count: 0,
    auxiliary_reference_count: 0,
    width_mode: DesignEdgeWidthMode::FullEdge,
    width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
    result_trailers: &[0],
};

/// Read one exact classed `EdgeFlange` form.
fn legacy_edge_flange_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    layout: LegacyEdgeFlangeLayout,
) -> Option<DesignEdgeFlangeOperation> {
    let edge_count = layout.edge_wrapper_offsets.len();
    if references.len() != layout.reference_count
        || paired_at.checked_sub(start)? != layout.frame_length
        || View::u32_le_at(bytes, start.checked_add(layout.edge_count_offset)?)?
            != u32::try_from(edge_count).ok()?
    {
        return None;
    }
    let mut unclaimed = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let edge_wrapper_record_indices = layout
        .edge_wrapper_offsets
        .iter()
        .map(|offset| {
            claim(
                marked_record_reference(bytes, start.checked_add(*offset)?)?,
                &mut unclaimed,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let settings_record_index = claim(
        marked_record_reference(bytes, start.checked_add(layout.settings_offset)?)?,
        &mut unclaimed,
    )?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(
        bytes,
        start.checked_add(layout.height_datum_offset)?,
    )?);
    let angle_owner_record_index = claim(
        marked_record_reference(bytes, start.checked_add(layout.angle_owner_offset)?)?,
        &mut unclaimed,
    )?;
    let height_owner_record_index = claim(
        marked_record_reference(bytes, start.checked_add(layout.height_owner_offset)?)?,
        &mut unclaimed,
    )?;
    let bend_radius_offset = start.checked_add(layout.bend_radius_offset)?;
    let bend_radius = View::f64_le_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    if View::u32_le_at(bytes, start.checked_add(layout.result_count_offset)?)?
        != u32::try_from(layout.result_trailers.len()).ok()?
        || View::u32_le_at(bytes, start.checked_add(layout.result_separator_offset)?)? != 1
    {
        return None;
    }
    let mut result_record_indices = HashSet::new();
    for (ordinal, expected_trailer) in layout.result_trailers.iter().enumerate() {
        let result_offset = layout
            .result_reference_start
            .checked_add(ordinal.checked_mul(15)?)?;
        let result_record_index =
            marked_record_reference(bytes, start.checked_add(result_offset)?)?;
        if !result_record_indices.insert(result_record_index)
            || View::u32_le_at(
                bytes,
                start.checked_add(
                    layout
                        .result_trailer_start
                        .checked_add(ordinal.checked_mul(15)?)?,
                )?,
            )? != *expected_trailer
        {
            return None;
        }
    }
    let aggregate_group_record_index = claim(
        marked_record_reference(bytes, start.checked_add(layout.aggregate_group_offset)?)?,
        &mut unclaimed,
    )?;
    let edge_group_record_indices = layout
        .edge_group_offsets
        .iter()
        .map(|offset| {
            claim(
                marked_record_reference(bytes, start.checked_add(*offset)?)?,
                &mut unclaimed,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let edge_operand_record_indices = edge_group_record_indices
        .iter()
        .map(|record_index| claim(record_index.checked_add(3)?, &mut unclaimed))
        .collect::<Option<Vec<_>>>()?;
    if unclaimed.len()
        != layout.aggregate_operand_count
            + layout.width_owner_count
            + layout.auxiliary_reference_count
    {
        return None;
    }
    let aggregate_operand_start = unclaimed
        .len()
        .checked_sub(layout.aggregate_operand_count)?;
    let aggregate_operand_record_indices = unclaimed.split_off(aggregate_operand_start);
    let width_distance_owner_record_indices = unclaimed
        .drain(..layout.width_owner_count)
        .collect::<Vec<_>>();
    let auxiliary_reference_record_indices = unclaimed;
    let width_distance_owner_record_indices_by_edge =
        if layout.width_mode == DesignEdgeWidthMode::TwoSidesPerEdge {
            if width_distance_owner_record_indices.len() != edge_count.checked_mul(2)? {
                return None;
            }
            width_distance_owner_record_indices
                .chunks_exact(2)
                .map(|pair| [pair[0], pair[1]])
                .collect()
        } else {
            Vec::new()
        };
    Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices,
        edge_group_record_indices,
        edge_operand_record_indices,
        aggregate_group_record_index,
        aggregate_operand_record_indices,
        height_owner_record_index,
        height_extent: DesignEdgeFlangeHeightExtent::Distance,
        angle_owner_record_index,
        width: crate::records::DesignEdgeWidth::from_wire(
            Some(layout.width_mode),
            width_distance_owner_record_indices,
            width_distance_owner_record_indices_by_edge,
        )
        .ok()?,
        auxiliary_reference_record_indices,
        width_parameter_source: layout.width_parameter_source,
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,

        height_datum,
        bend_position: DesignBendPosition::from_code(View::u32_le_at(
            bytes,
            start.checked_add(layout.bend_position_offset)?,
        )?),
    })
}

/// Read the `EdgeFlange` fixed operation section for one candidate header shift
/// and refuse the candidate unless every slot agrees.
fn edge_flange_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignEdgeFlangeOperation> {
    // The ordered reference table is in record-index order, so no role has a
    // fixed table position. Every role is instead named by a marked slot in the
    // fixed operation section, and the operand of a group is the record three
    // after it. The table entries no role claims are the width-distance
    // parameter owners the edge-width mode adds.
    //
    // Only the single-edge form is accounted for. A frame selecting more edges
    // names one edge group and one aggregate group in the same two slots, so
    // neither the further groups nor the order of their operands against the
    // aggregate operands is established, and such a frame is refused.
    if references.len() < 8 {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    let bend_position = DesignBendPosition::from_code(View::u32_le_at(bytes, common)?);
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }
    // Every reference the fixed section names is removed from this pool, so the
    // entries that remain at the end are exactly the unclaimed ones.
    let mut unclaimed: Vec<u32> = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };

    let mut cursor = common.checked_add(edge_flange::EDGE_WRAPPER_REFERENCE)?;
    let edge_wrapper_record_indices = vec![claim(
        marked_record_reference(bytes, cursor)?,
        &mut unclaimed,
    )?];
    cursor = common.checked_add(edge_flange::SETTINGS_REFERENCE)?;
    let settings_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = common.checked_add(edge_flange::HEIGHT_DATUM)?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(bytes, cursor)?);
    cursor = common.checked_add(edge_flange::ANGLE_OWNER_REFERENCE)?;
    let angle_owner_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = common.checked_add(edge_flange::HEIGHT_OWNER_REFERENCE)?;
    let height_owner_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    let bend_radius_offset = common.checked_add(edge_flange::INSIDE_BEND_RADIUS)?;
    let bend_radius = View::f64_le_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let result_count =
        usize::try_from(View::u32_le_at(bytes, bend_radius_offset.checked_add(14)?)?).ok()?;
    // The aggregate-group and role-`0x08` group slots close the section after the
    // result-record run, so they also confirm the recovered result count.
    let aggregate_slot = bend_radius_offset
        .checked_add(22)?
        .checked_add(result_count.checked_mul(15)?)?;
    let aggregate_group_record_index = claim(
        marked_record_reference(bytes, aggregate_slot)?,
        &mut unclaimed,
    )?;
    let first_edge_group = marked_record_reference(bytes, aggregate_slot.checked_add(27)?)?;

    // A group's recipe-backed operand is the record three after the group.
    let aggregate_operand_record_indices = vec![claim(
        aggregate_group_record_index.checked_add(3)?,
        &mut unclaimed,
    )?];
    let edge_group_record_indices = vec![claim(first_edge_group, &mut unclaimed)?];
    let edge_operand_record_indices =
        vec![claim(first_edge_group.checked_add(3)?, &mut unclaimed)?];

    if unclaimed.len() > MAX_EDGE_WIDTH_DISTANCE_OWNERS {
        return None;
    }
    let width_count = unclaimed.len();
    let width_distance_owner_record_indices = unclaimed;

    let expected_length = 493usize
        .checked_add(result_count.checked_mul(15)?)?
        .checked_add(width_count.checked_mul(11)?)?
        .checked_add(header_shift)?;
    if paired_at.checked_sub(start)? != expected_length {
        return None;
    }
    Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices,
        edge_group_record_indices,
        edge_operand_record_indices,
        aggregate_group_record_index,
        aggregate_operand_record_indices,
        height_owner_record_index,
        height_extent: DesignEdgeFlangeHeightExtent::Distance,
        angle_owner_record_index,
        width: crate::records::DesignEdgeWidth::from_wire(
            None,
            width_distance_owner_record_indices,
            Vec::new(),
        )
        .ok()?,
        auxiliary_reference_record_indices: Vec::new(),
        width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,

        height_datum,
        bend_position,
    })
}

/// Read the single-edge `EdgeFlange` form whose height is measured from a
/// selected construction entity.
fn edge_flange_to_object_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignEdgeFlangeOperation> {
    // This form has one target group and one target entity-selection operand in
    // addition to the distance form's roles. The two marked references between
    // the target group and the aggregate group are fixed-frame references, not
    // entries in the scope's ordered reference table, and are retained as
    // native references for rewrite.
    if references.len() != 11 {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    let bend_position = DesignBendPosition::from_code(View::u32_le_at(bytes, common)?);
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }
    let mut unclaimed = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let mut cursor = common.checked_add(edge_flange::EDGE_WRAPPER_REFERENCE)?;
    let edge_wrapper_record_indices = vec![claim(
        marked_record_reference(bytes, cursor)?,
        &mut unclaimed,
    )?];
    cursor = common.checked_add(edge_flange::SETTINGS_REFERENCE)?;
    let settings_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = common.checked_add(edge_flange::HEIGHT_DATUM)?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(View::u32_le_at(bytes, cursor)?);
    cursor = common.checked_add(edge_flange::ANGLE_OWNER_REFERENCE)?;
    let angle_owner_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = common.checked_add(edge_flange::HEIGHT_OWNER_REFERENCE)?;
    let height_owner_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    let bend_radius_offset = common.checked_add(edge_flange::INSIDE_BEND_RADIUS)?;
    let bend_radius = View::f64_le_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let result_count = View::u32_le_at(bytes, bend_radius_offset.checked_add(14)?)?;
    if result_count != 1
        || bytes.get(bend_radius_offset.checked_add(18)?..bend_radius_offset.checked_add(22)?)?
            != [0; 4]
    {
        return None;
    }
    if bytes.get(common.checked_add(89)?..common.checked_add(94)?)? != [0; 5] {
        return None;
    }
    let target_group_record_index = claim(
        marked_record_reference(
            bytes,
            common.checked_add(flange_to_object::TARGET_GROUP_REFERENCE)?,
        )?,
        &mut unclaimed,
    )?;
    if View::u32_le_at(
        bytes,
        common.checked_add(flange_to_object::TARGET_REFERENCE_COUNT)?,
    )? != 2
    {
        return None;
    }
    let reference_record_indices = [
        marked_record_reference(
            bytes,
            common.checked_add(flange_to_object::INSERTED_REFERENCE_ONE)?,
        )?,
        marked_record_reference(
            bytes,
            common.checked_add(flange_to_object::INSERTED_REFERENCE_TWO)?,
        )?,
    ];
    if reference_record_indices[0] == reference_record_indices[1]
        || reference_record_indices
            .iter()
            .any(|record_index| references.contains(record_index))
        || View::u32_le_at(
            bytes,
            common.checked_add(flange_to_object::INSERTED_REFERENCE_COUNT)?,
        )? != 1
        || bytes.get(
            common.checked_add(135)?
                ..common.checked_add(flange_to_object::AGGREGATE_REFERENCE_COUNT)?,
        )? != [0; 4]
        || View::u32_le_at(
            bytes,
            common.checked_add(flange_to_object::AGGREGATE_REFERENCE_COUNT)?,
        )? != 1
        || bytes.get(
            common.checked_add(154)?..common.checked_add(flange_to_object::EDGE_REFERENCE_COUNT)?,
        )? != [0; 12]
        || View::u32_le_at(
            bytes,
            common.checked_add(flange_to_object::EDGE_REFERENCE_COUNT)?,
        )? != 1
    {
        return None;
    }
    let aggregate_group_record_index = claim(
        marked_record_reference(
            bytes,
            common.checked_add(flange_to_object::AGGREGATE_GROUP_REFERENCE)?,
        )?,
        &mut unclaimed,
    )?;
    let edge_group_record_index = claim(
        marked_record_reference(
            bytes,
            common.checked_add(flange_to_object::EDGE_GROUP_REFERENCE)?,
        )?,
        &mut unclaimed,
    )?;
    let target_operand_record_index =
        claim(target_group_record_index.checked_add(3)?, &mut unclaimed)?;
    let aggregate_operand_record_indices = vec![claim(
        aggregate_group_record_index.checked_add(3)?,
        &mut unclaimed,
    )?];
    let edge_group_record_indices = vec![edge_group_record_index];
    let edge_operand_record_indices = vec![claim(
        edge_group_record_index.checked_add(3)?,
        &mut unclaimed,
    )?];
    let [offset_owner_record_index] = unclaimed.as_slice() else {
        return None;
    };
    let expected_length = 576usize.checked_add(header_shift)?;
    if paired_at.checked_sub(start)? != expected_length {
        return None;
    }
    Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices,
        edge_group_record_indices,
        edge_operand_record_indices,
        aggregate_group_record_index,
        aggregate_operand_record_indices,
        height_owner_record_index,
        height_extent: DesignEdgeFlangeHeightExtent::ToObject {
            target_group_record_index,
            target_operand_record_index,
            offset_owner_record_index: *offset_owner_record_index,
            reference_record_indices,
        },
        angle_owner_record_index,
        width: crate::records::DesignEdgeWidth::from_wire(None, Vec::new(), Vec::new())
            .expect("edge flange width"),
        auxiliary_reference_record_indices: Vec::new(),
        width_parameter_source: DesignEdgeFlangeWidthParameterSource::EdgeWidth,
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,

        height_datum,
        bend_position,
    })
}

pub(crate) fn exact_hem_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    parameter_source_kinds: &[(u32, &str)],
) -> Option<DesignHemOperation> {
    // The header shift and form are recovered by agreement, so all candidates
    // are evaluated and a frame that admits more than one is refused.
    let mut resolved = None;
    for header_shift in SHEET_METAL_HEADER_SHIFTS {
        for candidate in [
            hem_gap_length_operation_at(bytes, start, paired_at, references, header_shift),
            hem_radius_angle_operation_at(bytes, start, paired_at, references, header_shift),
            hem_gap_length_radius_operation_at(bytes, start, paired_at, references, header_shift),
        ]
        .into_iter()
        .flatten()
        .filter(|candidate| hem_parameter_kinds_match(candidate, parameter_source_kinds))
        {
            if resolved.is_some() {
                return None;
            }
            resolved = Some(candidate);
        }
    }
    resolved
}

fn hem_parameter_kinds_match(
    operation: &DesignHemOperation,
    parameter_source_kinds: &[(u32, &str)],
) -> bool {
    let has_kind = |record_index: u32, expected: &str| {
        let mut matches = parameter_source_kinds
            .iter()
            .filter(|(owner, _)| *owner == record_index);
        matches.next().is_some_and(|(_, kind)| *kind == expected) && matches.next().is_none()
    };
    match operation.parameter_owners {
        DesignHemParameterOwners::GapLength {
            gap_owner_record_index,
            length_owner_record_index,
        } => {
            has_kind(gap_owner_record_index, "HemGap")
                && has_kind(length_owner_record_index, "HemLength")
        }
        DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index,
            angle_owner_record_index,
        } => {
            has_kind(radius_owner_record_index, "HemRadius")
                && has_kind(angle_owner_record_index, "HemAngle")
        }
        DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index,
            length_owner_record_index,
            radius_owner_record_index,
        } => {
            has_kind(gap_owner_record_index, "HemGap")
                && has_kind(length_owner_record_index, "HemLength")
                && has_kind(radius_owner_record_index, "HemRadius")
        }
    }
}

pub(super) fn bind_hem_operation_from_parameters(
    bytes: &[u8],
    scope: &mut DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) {
    if scope.kind() != crate::records::DesignFeatureKind::Hem {
        return;
    }
    let Some(stream) = native_stream(&scope.id) else {
        return;
    };
    let parameter_source_kinds = parameter_owners
        .iter()
        .filter(|owner| {
            native_stream(&owner.id) == Some(stream)
                && owner.scope_record_index == scope.record_index
                && scope.reference_members.contains(&owner.record_index)
        })
        .flat_map(|owner| {
            parameters
                .iter()
                .filter(move |parameter| {
                    native_stream(&parameter.id) == Some(stream)
                        && parameter.record_index == owner.parameter_record_index
                })
                .map(move |parameter| (owner.record_index, parameter.source_kind.as_str()))
        })
        .collect::<Vec<_>>();
    let Some(start) = usize::try_from(scope.byte_offset).ok() else {
        return;
    };
    let Some(paired_at) = usize::try_from(scope.paired_byte_offset).ok() else {
        return;
    };
    {
        let construction = exact_hem_operation(
            bytes,
            start,
            paired_at,
            &scope.reference_members,
            &parameter_source_kinds,
        );
        if let crate::records::DesignScopePayload::Hem(slot) = &mut scope.payload {
            *slot = construction;
        }
    }
}

/// Read the gap-and-length `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
///
/// The ordered reference table is in record-index order, so every role is taken
/// from the marked slot that names it and each group's operand is the record
/// three after that group. The rolled and teardrop forms place their owner
/// references at other offsets and are handled by their corresponding readers.
fn hem_gap_length_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignHemOperation> {
    if references.len() != 8
        || paired_at.checked_sub(start)? != 494usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }

    let mut unclaimed: Vec<u32> = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Option<u32> {
        claim(
            marked_record_reference(bytes, common.checked_add(offset)?)?,
            pool,
        )
    };

    let edge_wrapper_record_index = slot(hem_gap::EDGE_WRAPPER_REFERENCE, &mut unclaimed)?;
    let settings_record_index = slot(hem_gap::SETTINGS_REFERENCE, &mut unclaimed)?;
    // The two owners are the form's inputs in local-ordinal order.
    let gap_owner_record_index = slot(hem_gap::GAP_OWNER_REFERENCE, &mut unclaimed)?;
    let length_owner_record_index = slot(hem_gap::LENGTH_OWNER_REFERENCE, &mut unclaimed)?;

    let bend_radius_offset = common.checked_add(hem_gap::INSIDE_BEND_RADIUS)?;
    let bend_radius = View::f64_le_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }

    let aggregate_group_record_index = slot(108, &mut unclaimed)?;
    let edge_group_record_index = slot(135, &mut unclaimed)?;
    let aggregate_operand_record_index =
        claim(aggregate_group_record_index.checked_add(3)?, &mut unclaimed)?;
    let edge_operand_record_index = claim(edge_group_record_index.checked_add(3)?, &mut unclaimed)?;
    if !unclaimed.is_empty() {
        return None;
    }

    Some(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index,
        edge_operand_record_index,
        aggregate_group_record_index,
        aggregate_operand_record_index,
        parameter_owners: DesignHemParameterOwners::GapLength {
            gap_owner_record_index,
            length_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
    })
}

/// Read the rolled `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
///
/// Rolled forms keep the two-owner frame length, but their owner slots are at
/// offsets `41` and `54` instead of `42` and `53`. The source parameter kinds
/// assign those slots to radius and angle; the fixed frame only proves their
/// record identities.
fn hem_radius_angle_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignHemOperation> {
    if references.len() != 8
        || paired_at.checked_sub(start)? != 494usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }

    let mut unclaimed = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Option<u32> {
        claim(
            marked_record_reference(bytes, common.checked_add(offset)?)?,
            pool,
        )
    };

    let edge_wrapper_record_index = slot(hem_gap::EDGE_WRAPPER_REFERENCE, &mut unclaimed)?;
    let settings_record_index = slot(hem_gap::SETTINGS_REFERENCE, &mut unclaimed)?;
    let angle_owner_record_index = slot(hem_rolled::ANGLE_OWNER_REFERENCE, &mut unclaimed)?;
    let radius_owner_record_index = slot(hem_rolled::RADIUS_OWNER_REFERENCE, &mut unclaimed)?;
    let bend_radius_offset = common.checked_add(hem_rolled::INSIDE_BEND_RADIUS)?;
    let bend_radius = View::f64_le_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let aggregate_group_record_index = slot(108, &mut unclaimed)?;
    let edge_group_record_index = slot(135, &mut unclaimed)?;
    let aggregate_operand_record_index =
        claim(aggregate_group_record_index.checked_add(3)?, &mut unclaimed)?;
    let edge_operand_record_index = claim(edge_group_record_index.checked_add(3)?, &mut unclaimed)?;
    if !unclaimed.is_empty() {
        return None;
    }

    Some(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index,
        edge_operand_record_index,
        aggregate_group_record_index,
        aggregate_operand_record_index,
        parameter_owners: DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index,
            angle_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
    })
}

/// Read the teardrop `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
fn hem_gap_length_radius_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignHemOperation> {
    if references.len() != 9
        || paired_at.checked_sub(start)? != 515usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if View::u32_le_at(bytes, common.checked_add(edge_flange::EDGE_COUNT)?)? != 1 {
        return None;
    }

    let mut unclaimed = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Option<u32> {
        claim(
            marked_record_reference(bytes, common.checked_add(offset)?)?,
            pool,
        )
    };

    let edge_wrapper_record_index = slot(hem_gap::EDGE_WRAPPER_REFERENCE, &mut unclaimed)?;
    let settings_record_index = slot(hem_gap::SETTINGS_REFERENCE, &mut unclaimed)?;
    let gap_owner_record_index = slot(hem_teardrop::GAP_OWNER_REFERENCE, &mut unclaimed)?;
    let length_owner_record_index = slot(hem_teardrop::LENGTH_OWNER_REFERENCE, &mut unclaimed)?;
    let radius_owner_record_index = slot(hem_teardrop::RADIUS_OWNER_REFERENCE, &mut unclaimed)?;
    let bend_radius_offset = common.checked_add(hem_teardrop::INSIDE_BEND_RADIUS)?;
    let bend_radius = View::f64_le_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let aggregate_group_record_index = slot(118, &mut unclaimed)?;
    let edge_group_record_index = slot(145, &mut unclaimed)?;
    let aggregate_operand_record_index =
        claim(aggregate_group_record_index.checked_add(3)?, &mut unclaimed)?;
    let edge_operand_record_index = claim(edge_group_record_index.checked_add(3)?, &mut unclaimed)?;
    if !unclaimed.is_empty() {
        return None;
    }

    Some(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index,
        edge_operand_record_index,
        aggregate_group_record_index,
        aggregate_operand_record_index,
        parameter_owners: DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index,
            length_owner_record_index,
            radius_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
    })
}
