// SPDX-License-Identifier: Apache-2.0
//! Source layout of the `Extrude` prologue: the field offsets its scope
//! capture states, in the frame layout its class pair and reference table
//! select.

use cadmpeg_core::decode::u64_from_index;

use super::super::design_stream;
use crate::design::decode::scopes::extrude::is_class_296_legacy_one_sided_distance_layout;
use crate::design::decode::scopes::extrude::is_class_296_legacy_one_sided_to_face_layout;
use crate::design::decode::scopes::extrude::is_class_296_one_sided_to_face_layout;
use crate::design::decode::scopes::extrude::is_class_296_symmetric_distance_layout;
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_layout;
use crate::design::decode::scopes::legacy_class_397::Class397SymmetricFrame;
use crate::design::decode::scopes::legacy_class_415;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_54 as class_296_legacy_prefix;
use crate::layout::class_296_261_legacy_one_sided_distance_tail as class_296_legacy_distance;
use crate::layout::class_296_261_legacy_one_sided_to_face_tail as class_296_legacy_to_face;
use crate::layout::class_296_261_one_sided_to_face_extrude_prefix as class_296_to_face;
use crate::layout::class_296_261_symmetric_distance_extrude_prefix as class_296_symmetric;
use crate::layout::class_296_261_two_sided_to_faces_extrude_prefix as class_296_two_faces;
use crate::layout::legacy_class_397_symmetric_extrude_frame as class_397;
use crate::layout::legacy_class_415_symmetric_extrude_prefix as class_415;
use crate::records;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::scope::{DesignParameterScope, LocatedScopeFrame};
use crate::records::identity::Sourced;
use crate::records::topology::extrude_selection::DesignOperandRole;

/// The source offset of an optional fixed marker: absent when the prologue
/// stores no marker, and `None` when a stored marker has no pointer.
fn marker_offset(marker: Option<Sourced<()>>) -> Option<Option<u64>> {
    match marker {
        None => Some(None),
        Some(marker) => marker.offset.map(Some),
    }
}

/// Whether the field offsets of an `Extrude` prologue hold in the frame
/// layout of its located scope. A prologue offset without a pointer fails.
pub(super) fn extrude_prologue_layout_holds(
    native: &crate::native::F3dNative,
    scope: &DesignParameterScope,
    frame: LocatedScopeFrame,
    prologue: DesignExtrudePrologue,
) -> bool {
    let native_stream = design_stream(&scope.id);
    match prologue {
        DesignExtrudePrologue::LegacyDistance {
            prefix_zero,
            operation_offset: Some(operation_offset),
            extent_kind_offset: Some(extent_kind_offset),
            direction_reversed_offset: Some(direction_reversed_offset),
            solid_operation_offset: Some(solid_operation_offset),
            ..
        } => {
            let Some(prefix_zero_offset) = marker_offset(prefix_zero) else {
                return false;
            };
            let marker_offset = frame.byte_offset().checked_add(20);
            let prefix_valid = match prefix_zero_offset {
                None => {
                    marker_offset
                        .and_then(|offset| offset.checked_add(1))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                        && frame
                            .byte_offset()
                            .checked_add(208)
                            .is_some_and(|expected_offset| {
                                frame.reference_count_offset() == expected_offset
                            })
                }
                Some(offset) => {
                    marker_offset
                        .and_then(|offset| offset.checked_add(1))
                        .is_some_and(|expected_offset| offset == expected_offset)
                        && offset
                            .checked_add(4)
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                        && frame
                            .byte_offset()
                            .checked_add(212)
                            .is_some_and(|expected_offset| {
                                frame.reference_count_offset() == expected_offset
                            })
                }
            };
            prefix_valid
                && operation_offset
                    .checked_add(4)
                    .is_some_and(|expected_offset| extent_kind_offset == expected_offset)
                && extent_kind_offset
                    .checked_add(4)
                    .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                && direction_reversed_offset
                    .checked_add(1)
                    .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
        }
        DesignExtrudePrologue::ShiftedReferenceAware {
            operation_offset: Some(operation_offset),
            direction_face_extend_values,
            side_extent_discriminators,
            side_extent_discriminator_offsets: Some(side_extent_discriminator_offsets),
            extent,
            direction_face_extend_offsets: Some(direction_face_extend_offsets),
            direction_reversed_offset: Some(direction_reversed_offset),
            solid_operation_offset: Some(solid_operation_offset),
            start_offset: Some(start_offset),
            ..
        } => {
            let expected_layout =
                match (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) {
                    ("357", "258") | ("275" | "361", "262") => Some((
                        538_u64,
                        292_u64,
                        13_usize,
                        [2, 1],
                        [2, 0],
                        records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                        288_u64,
                    )),
                    ("349", "266") => Some((
                        538_u64,
                        292_u64,
                        13_usize,
                        [2, 1],
                        [2, 0],
                        records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                        288_u64,
                    )),
                    ("323", "263")
                        if frame.byte_offset().checked_add(292).is_some_and(
                            |expected_offset| frame.reference_count_offset() == expected_offset,
                        ) =>
                    {
                        Some((
                            516_u64,
                            292_u64,
                            11_usize,
                            [2, 1],
                            [2, 0],
                            records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                            288_u64,
                        ))
                    }
                    ("323", "263")
                        if frame.byte_offset().checked_add(272).is_some_and(
                            |expected_offset| frame.reference_count_offset() == expected_offset,
                        ) =>
                    {
                        Some((
                            485_u64,
                            272_u64,
                            10_usize,
                            [3, 0],
                            [4, 4],
                            records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll,
                            129_u64,
                        ))
                    }
                    _ => None,
                };
            expected_layout.is_some_and(
                |(
                    frame_length,
                    reference_count_offset,
                    reference_member_count,
                    expected_direction_face_extend_values,
                    expected_side_extent_discriminators,
                    expected_extent,
                    second_side_extent_offset,
                )| {
                    frame.frame_length() == frame_length
                        && frame.byte_offset().checked_add(frame_length).is_some_and(
                            |expected_offset| frame.paired_byte_offset() == expected_offset,
                        )
                        && frame
                            .byte_offset()
                            .checked_add(reference_count_offset)
                            .is_some_and(|expected_offset| {
                                frame.reference_count_offset() == expected_offset
                            })
                        && scope.reference_members().len() == reference_member_count
                        && frame
                            .byte_offset()
                            .checked_add(27)
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                        && direction_face_extend_values == expected_direction_face_extend_values
                        && side_extent_discriminators == expected_side_extent_discriminators
                        && extent == expected_extent
                        && frame
                            .byte_offset()
                            .checked_add(116)
                            .zip(frame.byte_offset().checked_add(second_side_extent_offset))
                            .map(|(first, second)| [first, second])
                            .is_some_and(|expected_offset| {
                                side_extent_discriminator_offsets == expected_offset
                            })
                        && frame
                            .byte_offset()
                            .checked_add(31)
                            .zip(frame.byte_offset().checked_add(35))
                            .map(|(first, second)| [first, second])
                            .is_some_and(|expected_offset| {
                                direction_face_extend_offsets == expected_offset
                            })
                        && frame
                            .byte_offset()
                            .checked_add(39)
                            .is_some_and(|expected_offset| {
                                direction_reversed_offset == expected_offset
                            })
                        && frame
                            .byte_offset()
                            .checked_add(40)
                            .is_some_and(|expected_offset| {
                                solid_operation_offset == expected_offset
                            })
                        && frame
                            .byte_offset()
                            .checked_add(41)
                            .is_some_and(|expected_offset| start_offset == expected_offset)
                },
            )
        }
        DesignExtrudePrologue::ReferenceAware {
            reference,
            operation_offset: Some(operation_offset),
            direction_face_extend_values,
            side_extent_discriminators,
            side_extent_discriminator_offsets: Some(side_extent_discriminator_offsets),
            first_side_target_ordinal,
            extent,
            direction_face_extend_offsets: Some(direction_face_extend_offsets),
            direction_reversed_offset: Some(direction_reversed_offset),
            solid_operation_offset: Some(solid_operation_offset),
            start_offset: Some(start_offset),
            ..
        } => {
            let prefix_valid = reference.map_or(
                frame
                    .byte_offset()
                    .checked_add(28)
                    .is_some_and(|expected_offset| operation_offset == expected_offset),
                |reference| {
                    let (Some(record_index_offset), Some(operation_prefix_marker_offset)) = (
                        reference.record_index_offset,
                        marker_offset(reference.operation_prefix_marker),
                    ) else {
                        return false;
                    };
                    let padding_end = record_index_offset.checked_add(4).and_then(|offset| {
                        offset.checked_add(u64::from(reference.trailing_zero_count))
                    });
                    let marker_valid = match operation_prefix_marker_offset {
                        None => Some(operation_offset) == padding_end,
                        Some(marker_offset) => {
                            Some(marker_offset) == padding_end
                                && marker_offset.checked_add(1).is_some_and(|expected_offset| {
                                    operation_offset == expected_offset
                                })
                        }
                    };
                    frame
                        .byte_offset()
                        .checked_add(26)
                        .is_some_and(|expected_offset| record_index_offset == expected_offset)
                        && matches!(reference.trailing_zero_count, 7 | 8)
                        && marker_valid
                        && scope
                            .reference_members()
                            .values()
                            .any(|value| value == &reference.record_index)
                },
            );
            let target_ordinal_valid = first_side_target_ordinal.is_none_or(|target| {
                usize::try_from(target.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal).copied())
                    .is_some_and(|record_index| {
                        let mut groups =
                            native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == record_index
                                        && group.scope_reference_ordinal
                                            == target.scope_reference_ordinal
                                        && group.role() == DesignOperandRole::ROLE_0X5
                                        && group.extrude_role().is_none()
                                });
                        target
                            .scope_reference_ordinal_offset
                            .and_then(|offset| offset.checked_add(5))
                            == Some(side_extent_discriminator_offsets[0])
                            && groups.next().is_some()
                            && groups.next().is_none()
                    })
            });
            let target_prefix_length = if first_side_target_ordinal.is_some() {
                5
            } else {
                0
            };
            let legacy_class_415_layout = frame
                .reference_count_offset()
                .checked_sub(frame.byte_offset())
                .is_some_and(|reference_count_delta| {
                    legacy_class_415::is_symmetric_distance_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        frame.frame_length(),
                        reference_count_delta,
                        scope.reference_members().len(),
                    )
                });
            let legacy_class_415_one_sided_layout = frame
                .reference_count_offset()
                .checked_sub(frame.byte_offset())
                .is_some_and(|reference_count_delta| {
                    legacy_class_415::is_one_sided_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        frame.frame_length(),
                        reference_count_delta,
                        scope.reference_members().len(),
                    )
                });
            let legacy_class_415_extent = legacy_class_415_layout
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_415::OPERATION))
                    .is_some_and(|expected_offset| operation_offset == expected_offset)
                && direction_face_extend_values == [3, 2]
                && side_extent_discriminators == [1, 1]
                && extent == records::feature::extrude::DesignExtrudeExtent::SymmetricDistance
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_415::FIRST_SIDE_EXTENT))
                    .zip(
                        frame
                            .byte_offset()
                            .checked_add(u64_from_index(class_415::SECOND_SIDE_EXTENT)),
                    )
                    .map(|(first, second)| [first, second])
                    .is_some_and(|expected_offset| {
                        side_extent_discriminator_offsets == expected_offset
                    })
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_415::DIRECTION))
                    .zip(
                        frame
                            .byte_offset()
                            .checked_add(u64_from_index(class_415::FACE_EXTEND)),
                    )
                    .map(|(first, second)| [first, second])
                    .is_some_and(|expected_offset| {
                        direction_face_extend_offsets == expected_offset
                    })
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_415::DIRECTION_REVERSED))
                    .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_415::GEOMETRY_KIND))
                    .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_415::START_SUPPORT))
                    .is_some_and(|expected_offset| start_offset == expected_offset);
            let first_side_offset_valid = operation_offset
                .checked_add(49)
                .and_then(|offset| offset.checked_add(target_prefix_length))
                .and_then(|offset| side_extent_discriminator_offsets[0].checked_sub(offset))
                .is_some_and(|slot_expansion| {
                    slot_expansion <= 70 && slot_expansion.is_multiple_of(10)
                });
            let second_side_offset_valid = (if side_extent_discriminators[0] == 2 {
                frame.reference_count_offset().checked_sub(4)
            } else {
                side_extent_discriminator_offsets[0].checked_add(13)
            })
            .is_some_and(|expected_offset| side_extent_discriminator_offsets[1] == expected_offset)
                || (legacy_class_415_one_sided_layout
                    && frame.reference_count_offset().checked_sub(4).is_some_and(
                        |expected_offset| side_extent_discriminator_offsets[1] == expected_offset,
                    ));
            let standard_extent = matches!(
                (
                    direction_face_extend_values[0],
                    side_extent_discriminators,
                    extent,
                ),
                (
                    1,
                    [1, 0],
                    records::feature::extrude::DesignExtrudeExtent::OneSidedDistance
                ) | (
                    1,
                    [2, 0],
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace
                ) | (
                    1,
                    [3, 0],
                    records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext
                ) | (
                    1,
                    [4, 0],
                    records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll
                ) | (
                    2,
                    [2, 0],
                    records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces
                ) | (
                    2,
                    [1, 1],
                    records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance
                ) | (
                    3,
                    [1, 0],
                    records::feature::extrude::DesignExtrudeExtent::SymmetricDistance
                ) | (
                    3,
                    [4, 4],
                    records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll
                )
            );
            prefix_valid
                && matches!(direction_face_extend_values[0], 1..=3)
                && (standard_extent || legacy_class_415_extent)
                && first_side_target_ordinal.is_none_or(|_| side_extent_discriminators[0] == 2)
                && target_ordinal_valid
                && first_side_offset_valid
                && second_side_offset_valid
                && operation_offset
                    .checked_add(4)
                    .zip(operation_offset.checked_add(8))
                    .map(|(first, second)| [first, second])
                    .is_some_and(|expected_offset| direction_face_extend_offsets == expected_offset)
                && operation_offset
                    .checked_add(14)
                    .is_some_and(|expected_offset| start_offset == expected_offset)
                && operation_offset
                    .checked_add(13)
                    .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                && operation_offset
                    .checked_add(12)
                    .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                && side_extent_discriminator_offsets[1]
                    .checked_add(4)
                    .is_some_and(|end| end <= frame.reference_count_offset())
        }
        DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker,
            operation_offset: Some(operation_offset),
            direction_face_extend_values,
            side_extent_discriminators,
            side_extent_discriminator_offsets: Some(side_extent_discriminator_offsets),
            extent,
            direction_face_extend_offsets: Some(direction_face_extend_offsets),
            direction_reversed_offset: Some(direction_reversed_offset),
            solid_operation_offset: Some(solid_operation_offset),
            start_offset: Some(start_offset),
            ..
        } => {
            let Some(operation_prefix_marker_offset) = marker_offset(operation_prefix_marker)
            else {
                return false;
            };
            let field_shift = match operation_prefix_marker_offset {
                None if frame
                    .byte_offset()
                    .checked_add(27)
                    .is_some_and(|expected_offset| operation_offset == expected_offset) =>
                {
                    Some(0)
                }
                Some(marker_offset)
                    if frame
                        .byte_offset()
                        .checked_add(27)
                        .is_some_and(|expected_offset| marker_offset == expected_offset)
                        && marker_offset
                            .checked_add(1)
                            .is_some_and(|expected_offset| operation_offset == expected_offset) =>
                {
                    Some(1)
                }
                _ => None,
            };
            let compact_extent_offsets = if operation_prefix_marker_offset.is_none()
                && frame
                    .byte_offset()
                    .checked_add(26)
                    .is_some_and(|expected_offset| operation_offset == expected_offset)
            {
                frame
                    .reference_count_offset()
                    .checked_sub(frame.byte_offset())
                    .and_then(|offset| match offset {
                        251 => frame
                            .byte_offset()
                            .checked_add(105)
                            .zip(frame.byte_offset().checked_add(109))
                            .map(|(first, second)| [first, second]),
                        281 => frame
                            .byte_offset()
                            .checked_add(124)
                            .zip(frame.byte_offset().checked_add(128))
                            .map(|(first, second)| [first, second]),
                        _ => None,
                    })
            } else {
                None
            };
            let class_296_extent_offsets = if frame
                .reference_count_offset()
                .checked_sub(frame.byte_offset())
                .is_some_and(|reference_count_offset| {
                    is_class_296_one_sided_to_face_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        frame.frame_length(),
                        reference_count_offset,
                        scope.reference_members().len(),
                    )
                })
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_296_to_face::OPERATION))
                    .is_some_and(|expected_offset| operation_offset == expected_offset)
            {
                frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_296_to_face::FIRST_SIDE_EXTENT))
                    .zip(
                        frame
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_to_face::SECOND_SIDE_EXTENT)),
                    )
                    .map(|(first, second)| [first, second])
            } else {
                None
            };
            let class_296_symmetric_extent_offsets = if frame
                .reference_count_offset()
                .checked_sub(frame.byte_offset())
                .is_some_and(|reference_count_offset| {
                    is_class_296_symmetric_distance_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        frame.frame_length(),
                        reference_count_offset,
                        scope.reference_members().len(),
                    )
                })
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_296_symmetric::OPERATION))
                    .is_some_and(|expected_offset| operation_offset == expected_offset)
            {
                frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_296_symmetric::FIRST_SIDE_EXTENT))
                    .zip(
                        frame
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_symmetric::SECOND_SIDE_EXTENT)),
                    )
                    .map(|(first, second)| [first, second])
            } else {
                None
            };
            let class_296_two_faces_extent_offsets = if frame
                .reference_count_offset()
                .checked_sub(frame.byte_offset())
                .is_some_and(|reference_count_offset| {
                    is_class_296_two_sided_to_faces_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        frame.frame_length(),
                        reference_count_offset,
                        scope.reference_members().len(),
                    )
                })
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_296_two_faces::OPERATION))
                    .is_some_and(|expected_offset| operation_offset == expected_offset)
            {
                frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_296_two_faces::FIRST_SIDE_EXTENT))
                    .zip(
                        frame
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_two_faces::SECOND_SIDE_EXTENT)),
                    )
                    .map(|(first, second)| [first, second])
            } else {
                None
            };
            let class_296_legacy_to_face_extent_offsets =
                if frame
                    .reference_count_offset()
                    .checked_sub(frame.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_legacy_one_sided_to_face_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            frame.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && frame
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    frame
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::FIRST_SIDE_EXTENT))
                        .zip(frame.byte_offset().checked_add(u64_from_index(
                            class_296_legacy_to_face::SECOND_SIDE_EXTENT,
                        )))
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
            let class_296_legacy_distance_extent_offsets = if frame
                .reference_count_offset()
                .checked_sub(frame.byte_offset())
                .is_some_and(|reference_count_offset| {
                    is_class_296_legacy_one_sided_distance_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        frame.frame_length(),
                        reference_count_offset,
                        scope.reference_members().len(),
                    )
                })
                && frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_296_legacy_prefix::OPERATION))
                    .is_some_and(|expected_offset| operation_offset == expected_offset)
            {
                frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_296_legacy_prefix::FIRST_SIDE_EXTENT))
                    .zip(frame.byte_offset().checked_add(u64_from_index(
                        class_296_legacy_distance::SECOND_SIDE_EXTENT,
                    )))
                    .map(|(first, second)| [first, second])
            } else {
                None
            };
            let class_397_frame = frame
                .reference_count_offset()
                .checked_sub(frame.byte_offset())
                .and_then(|reference_count_offset| {
                    Class397SymmetricFrame::new(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        frame.frame_length(),
                        reference_count_offset,
                        scope.reference_members().len(),
                    )
                });
            let extent_valid = if let Some(symmetric_frame) = class_397_frame {
                operation_prefix_marker_offset.is_none()
                    && frame
                        .byte_offset()
                        .checked_add(u64_from_index(class_397::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                    && direction_face_extend_values
                        == [class_397::DIRECTION_VALUE, class_397::FACE_EXTEND_VALUE]
                    && extent.is_some()
                    && extent
                        == symmetric_frame
                            .extent(direction_face_extend_values[0], side_extent_discriminators)
            } else if compact_extent_offsets.is_some() {
                matches!(
                    (
                        direction_face_extend_values,
                        side_extent_discriminators,
                        extent,
                    ),
                    (
                        [1, _],
                        [1, 0],
                        Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
                    ) | (
                        [3, _],
                        [1, 0],
                        Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
                    ) | (
                        [2, 0],
                        [1, 2],
                        Some(
                            records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace
                        )
                    )
                )
            } else if class_296_extent_offsets.is_some() {
                direction_face_extend_values[0] == 1
                    && matches!(direction_face_extend_values[1], 1 | 2)
                    && side_extent_discriminators == [2, 0]
                    && extent
                        == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
            } else if class_296_symmetric_extent_offsets.is_some() {
                direction_face_extend_values == [3, 2]
                    && side_extent_discriminators == [1, 0]
                    && extent
                        == Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
            } else if class_296_two_faces_extent_offsets.is_some() {
                direction_face_extend_values[0] == 2
                    && matches!(direction_face_extend_values[1], 1 | 2)
                    && side_extent_discriminators == [2, 0]
                    && extent
                        == Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces)
            } else if class_296_legacy_to_face_extent_offsets.is_some() {
                direction_face_extend_values == [1, 1]
                    && side_extent_discriminators == [2, 0]
                    && extent
                        == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
            } else if class_296_legacy_distance_extent_offsets.is_some() {
                direction_face_extend_values == [1, 2]
                    && side_extent_discriminators == [1, 0]
                    && extent
                        == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
            } else {
                matches!(direction_face_extend_values[0], 1..=3)
                    && matches!(
                        (
                            direction_face_extend_values[0],
                            side_extent_discriminators,
                            extent,
                        ),
                        (
                            1,
                            [1, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
                        ) | (
                            1,
                            [2, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                        ) | (
                            1,
                            [3, 0],
                            Some(
                                records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext
                            ),
                        ) | (
                            1,
                            [4, 0],
                            Some(
                                records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll
                            )
                        ) | (
                            2,
                            [1, 1],
                            Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance)
                        ) | (
                            3,
                            [1, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
                        ) | (
                            3,
                            [4, 4],
                            Some(
                                records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll
                            )
                        )
                    )
            };
            let side_offsets_valid = if class_397_frame.is_some() {
                frame
                    .byte_offset()
                    .checked_add(u64_from_index(class_397::FIRST_SIDE_EXTENT))
                    .zip(
                        frame
                            .byte_offset()
                            .checked_add(u64_from_index(class_397::SECOND_SIDE_EXTENT)),
                    )
                    .map(|(first, second)| [first, second])
                    .is_some_and(|expected_offset| {
                        side_extent_discriminator_offsets == expected_offset
                    })
            } else {
                compact_extent_offsets
                    .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                    || class_296_extent_offsets
                        .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                    || class_296_symmetric_extent_offsets
                        .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                    || class_296_two_faces_extent_offsets
                        .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                    || class_296_legacy_to_face_extent_offsets
                        .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                    || class_296_legacy_distance_extent_offsets
                        .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                    || field_shift.is_some_and(|field_shift| {
                        (if direction_face_extend_values[0] == 2 {
                            if frame
                                .reference_count_offset()
                                .checked_sub(frame.byte_offset())
                                .and_then(|offset| offset.checked_sub(field_shift))
                                == Some(283)
                            {
                                frame
                                    .byte_offset()
                                    .checked_add(166 + field_shift)
                                    .zip(frame.byte_offset().checked_add(181 + field_shift))
                                    .map(|(first, second)| [first, second])
                            } else {
                                frame
                                    .byte_offset()
                                    .checked_add(155 + field_shift)
                                    .zip(frame.byte_offset().checked_add(178 + field_shift))
                                    .map(|(first, second)| [first, second])
                            }
                        } else if side_extent_discriminators[0] == 2 {
                            let first_offset = side_extent_discriminator_offsets[0];
                            if matches!(
                                first_offset
                                    .checked_sub(frame.byte_offset())
                                    .and_then(|offset| offset.checked_sub(field_shift)),
                                Some(106 | 116)
                            ) {
                                frame
                                    .reference_count_offset()
                                    .checked_sub(4)
                                    .map(|second| [first_offset, second])
                            } else {
                                Some([0, 0])
                            }
                        } else if frame
                            .byte_offset()
                            .checked_add(116 + field_shift)
                            .zip(frame.byte_offset().checked_add(129 + field_shift))
                            .map(|(first, second)| [first, second])
                            .is_some_and(|expected_offset| {
                                side_extent_discriminator_offsets == expected_offset
                            })
                        {
                            Some(side_extent_discriminator_offsets)
                        } else if frame
                            .byte_offset()
                            .checked_add(116 + field_shift)
                            .is_some_and(|expected_offset| {
                                side_extent_discriminator_offsets[0] == expected_offset
                            })
                        {
                            frame
                                .byte_offset()
                                .checked_add(116 + field_shift)
                                .zip(frame.byte_offset().checked_add(130 + field_shift))
                                .map(|(first, second)| [first, second])
                        } else {
                            frame
                                .byte_offset()
                                .checked_add(106 + field_shift)
                                .zip(frame.byte_offset().checked_add(110 + field_shift))
                                .map(|(first, second)| [first, second])
                        })
                        .is_some_and(|expected_offset| {
                            side_extent_discriminator_offsets == expected_offset
                        })
                    })
            };
            (field_shift.is_some()
                || compact_extent_offsets.is_some()
                || class_296_extent_offsets.is_some()
                || class_296_symmetric_extent_offsets.is_some()
                || class_296_two_faces_extent_offsets.is_some()
                || class_296_legacy_to_face_extent_offsets.is_some()
                || class_296_legacy_distance_extent_offsets.is_some())
                && extent_valid
                && side_offsets_valid
                && operation_offset
                    .checked_add(4)
                    .zip(operation_offset.checked_add(8))
                    .map(|(first, second)| [first, second])
                    .is_some_and(|expected_offset| direction_face_extend_offsets == expected_offset)
                && operation_offset
                    .checked_add(14)
                    .is_some_and(|expected_offset| start_offset == expected_offset)
                && operation_offset
                    .checked_add(13)
                    .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                && operation_offset
                    .checked_add(12)
                    .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                && direction_face_extend_offsets[1] < frame.reference_count_offset()
        }
        _ => false,
    }
}
