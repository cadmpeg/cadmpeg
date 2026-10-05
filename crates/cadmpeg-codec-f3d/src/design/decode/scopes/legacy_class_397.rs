// SPDX-License-Identifier: Apache-2.0
//! Parse the legacy class-397 symmetric-distance Extrude grammar.

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use super::extrude::ExtrudeScopeFrame;
use crate::bytes::f64s_at;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::text::fixed_guid_end;
use crate::layout::legacy_class_397_symmetric_extrude_frame as symmetric;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use cadmpeg_core::decode::View;

/// Checked class, frame length, and reference-run layout for the class-397 grammar.
#[derive(Clone, Copy)]
pub(crate) struct Class397SymmetricFrame(());

impl Class397SymmetricFrame {
    /// Admit the fixed class-397 symmetric-distance frame layout.
    pub(crate) fn new(
        class_tag: &str,
        paired_class_tag: &str,
        frame_length: u64,
        reference_count_offset: u64,
        reference_count: usize,
    ) -> Option<Self> {
        (class_tag == "397"
            && paired_class_tag == "262"
            && frame_length == u64_from_index(symmetric::LEN)
            && reference_count_offset == u64_from_index(symmetric::REFERENCE_COUNT)
            && reference_count == index_from_u32(symmetric::REFERENCE_COUNT_VALUE))
        .then_some(Self(()))
    }

    /// The symmetric-distance extent admitted by this frame grammar.
    pub(crate) fn extent(
        self,
        direction: u32,
        side_extent_discriminators: [u32; 2],
    ) -> Option<DesignExtrudeExtent> {
        match (self, direction, side_extent_discriminators) {
            (Self(()), 3, [1, 1]) => Some(DesignExtrudeExtent::SymmetricDistance),
            _ => None,
        }
    }
}

pub(super) fn exact_symmetric_extrude_prologue(
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
    let Some(prefix) = symmetric_prefix(
        bytes,
        start,
        paired_at,
        class_tag,
        paired_class_tag,
        reference_count_at,
        reference_members,
    ) else {
        return Ok(None);
    };
    let guid_offset = start + symmetric::GUID;
    let Some(guid_end) = fixed_guid_end(ctx, bytes, guid_offset)? else {
        return Ok(None);
    };
    let reference_count_offset = start + symmetric::REFERENCE_COUNT;
    if guid_end != guid_offset + 76
        || !zeros_at::<3>(bytes, guid_end)
        || View::u32_le_at(bytes, reference_count_offset) != Some(symmetric::REFERENCE_COUNT_VALUE)
    {
        return Ok(None);
    }
    Ok(Some(prefix))
}

/// The prologue fields before the GUID of a class-397 symmetric-distance
/// frame. The frame admission fixes the reference run at eight members, so
/// each reference-slot membership test reads at most eight values.
fn symmetric_prefix(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    class_tag: &str,
    paired_class_tag: &str,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    const PROFILE_NORMAL_UNIT_EPS: f64 = 1.0e-12;

    let frame = Class397SymmetricFrame::new(
        class_tag,
        paired_class_tag,
        u64::try_from(paired_at.checked_sub(start)?).ok()?,
        u64::try_from(reference_count_at.checked_sub(start)?).ok()?,
        reference_members.len(),
    )?;
    let reference_members = <&[u32; 8]>::try_from(reference_members).ok()?;
    if View::u32_le_at(bytes, start.checked_add(symmetric::PREFIX_CONSTANT)?)?
        != symmetric::PREFIX_CONSTANT_VALUE
        || !zeros_at::<3>(bytes, start + symmetric::PREFIX_CONSTANT + 4)
    {
        return None;
    }

    let operation_offset = start + symmetric::OPERATION;
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let direction_face_extend_offsets =
        [start + symmetric::DIRECTION, start + symmetric::FACE_EXTEND];
    let direction_face_extend_values = [
        View::u32_le_at(bytes, direction_face_extend_offsets[0])?,
        View::u32_le_at(bytes, direction_face_extend_offsets[1])?,
    ];
    if direction_face_extend_values != [symmetric::DIRECTION_VALUE, symmetric::FACE_EXTEND_VALUE] {
        return None;
    }

    let direction_reversed_offset = start + symmetric::DIRECTION_REVERSED;
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = start + symmetric::GEOMETRY_KIND;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let start_offset = start + symmetric::START_SUPPORT;
    let start_support = match bytes.get(start_offset)? {
        0 => DesignExtrudeStart::ProfilePlane,
        1 => DesignExtrudeStart::OffsetProfilePlane,
        2 => DesignExtrudeStart::FromFace,
        _ => return None,
    };

    let profile_normal = f64s_at::<3>(bytes, start + symmetric::PROFILE_NORMAL)?;
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

    let mut slot_offset = start + symmetric::REFERENCE_SLOTS;
    for expected_present in [true, true, true, true, false, true, false] {
        let present = match bytes.get(slot_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        if present != expected_present {
            return None;
        }
        if present {
            let record_index = super::shared_frames::marked_record_reference(bytes, slot_offset)?;
            if !reference_members.contains(&record_index) {
                return None;
            }
            slot_offset += 11;
        } else {
            slot_offset += 1;
        }
    }
    let first_side_extent_offset = start + symmetric::FIRST_SIDE_EXTENT;
    if slot_offset != first_side_extent_offset {
        return None;
    }

    let second_side_extent_offset = start + symmetric::SECOND_SIDE_EXTENT;
    let side_extent_discriminators = [
        View::u32_le_at(bytes, first_side_extent_offset)?,
        View::u32_le_at(bytes, second_side_extent_offset)?,
    ];
    let extent = frame.extent(direction_face_extend_values[0], side_extent_discriminators)?;
    if !zeros_at::<9>(bytes, first_side_extent_offset + 4) {
        return None;
    }

    Some(DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation,
        operation_offset: u64_from_index(operation_offset),
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            u64_from_index(first_side_extent_offset),
            u64_from_index(second_side_extent_offset),
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
