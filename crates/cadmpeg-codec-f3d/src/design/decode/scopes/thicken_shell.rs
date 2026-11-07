// SPDX-License-Identifier: Apache-2.0
//! Exact legacy thicken and shell scope frames.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::{fixed_guid_end, fixed_utf16_ascii_eq};
use crate::layout::shell_compact_slot_scope_prefix as shell_compact;
use crate::layout::shell_marked_slot_scope_prefix as shell_marked;
use crate::layout::shell_wall_slot_scope_prefix as shell_slots;
use crate::layout::thicken_class_347_scope_frame as thicken_347;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;

const WALL_SLOT_LEN: usize = 11;
const CORNERED_ZERO_RUN: usize = 12;
const MARKED_ZERO_RUN: usize = 8;
const COUNTED_ZERO_RUN: usize = 4;
const TABLE_ZERO_RUN: usize = 3;

/// Decode class-347/258 Thicken, whose group precedes its scalar. The class
/// pair and 291-byte frame are part of admission for this distinct grammar.
pub(super) fn exact_legacy_thicken_class_347(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    if scope.class_tag.as_str() != "347"
        || scope.paired_class_tag.as_str() != "258"
        || scope.frame_length() != u64::try_from(thicken_347::LEN).ok()?
        || scope.reference_members().len() != 3
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + thicken_347::ZERO_RUN_10..start + thicken_347::FEATURE_FORM)
        != Some(&[0; 10])
        || View::u32_le_at(bytes, start + thicken_347::FEATURE_FORM)? != 4
        || View::u32_le_at(bytes, start + thicken_347::GROUP_FORM)? != 1
        || marked_record_reference(bytes, start + thicken_347::GROUP_REFERENCE)?
            != scope.reference_members().values().next().copied()?
        || bytes.get(start + thicken_347::SCALAR_PREFIX..start + thicken_347::SCALAR_REFERENCE)
            != Some(&[1, 1])
        || View::u32_le_at(bytes, start + thicken_347::AUXILIARY_COUNT)? != 1
        || marked_record_reference(bytes, start + thicken_347::AUXILIARY_REFERENCE).is_none()
        || bytes.get(start + thicken_347::ZERO_RUN_8..start + thicken_347::GUID_CODE_UNIT_COUNT)
            != Some(&[0; 8])
        || View::u32_le_at(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT)? != 36
        || bytes.get(start + thicken_347::ZERO_RUN_3..start + thicken_347::REFERENCE_COUNT)
            != Some(&[0; 3])
        || View::u32_le_at(bytes, start + thicken_347::REFERENCE_COUNT)? != 3
        || View::u32_le_at(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT)? != 7
    {
        return None;
    }
    let guid_end = fixed_guid_end(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT)?;
    if guid_end != start + thicken_347::ZERO_RUN_3 {
        return None;
    }
    let kind_end =
        fixed_utf16_ascii_eq(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT, "Thicken")?;
    if kind_end != start + thicken_347::FEATURE_ORDINAL {
        return None;
    }
    let reference_entries = [
        thicken_347::GROUP_REFERENCE_ENTRY,
        thicken_347::MEMBER_REFERENCE_ENTRY,
        thicken_347::SCALAR_REFERENCE_ENTRY,
    ];
    for (offset, expected) in reference_entries
        .into_iter()
        .zip(scope.reference_members().values().copied())
    {
        if marked_record_reference(bytes, start + offset) != Some(expected) {
            return None;
        }
    }
    let thickness_record_index =
        marked_record_reference(bytes, start + thicken_347::SCALAR_REFERENCE)?;
    if scope.reference_members().values().next_back().copied()? != thickness_record_index {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    (scalar.value.get() != 0.0).then_some(DesignDirectFaceOperation::Thicken(
        direct_face::DesignThickenOperation {
            signed_thickness: scalar.value,
            thickness_record_index,
            thickness_offset: scalar.value_offset,
        },
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WallSlotPrefix {
    Cornered,
    Compact,
    Marked,
    Counted,
}

impl WallSlotPrefix {
    const ALL: [Self; 4] = [Self::Cornered, Self::Compact, Self::Marked, Self::Counted];

    fn slots_at(self, bytes: &[u8], start: usize) -> Option<usize> {
        let at = |offset: usize| bytes.get(start.checked_add(offset)?).copied();
        let zero = |from: usize, to: usize| {
            bytes
                .get(start.checked_add(from)?..start.checked_add(to)?)
                .map(|run| run.iter().all(|byte| *byte == 0))
        };
        let flag = |offset: usize| matches!(at(offset), Some(0 | 1));
        let marked = || {
            zero(shell_marked::ZERO_RUN_9, shell_marked::FEATURE_FORM) == Some(true)
                && at(shell_marked::FEATURE_FORM) == Some(shell_marked::FEATURE_FORM_VALUE)
                && flag(shell_marked::FLAG)
                && zero(shell_marked::ZERO_RUN_3, shell_marked::SLOT_MARKER) == Some(true)
                && at(shell_marked::SLOT_MARKER) == Some(shell_marked::SLOT_MARKER_VALUE)
        };
        let (admitted, slots) = match self {
            Self::Cornered => (
                zero(shell_slots::ZERO_RUN_10, shell_slots::TANGENT_CHAIN) == Some(true)
                    && flag(shell_slots::TANGENT_CHAIN)
                    && flag(shell_slots::ROUNDED_CORNERS),
                shell_slots::LEN,
            ),
            Self::Compact => (
                zero(shell_compact::ZERO_RUN_10, shell_compact::FLAG) == Some(true)
                    && flag(shell_compact::FLAG),
                shell_compact::LEN,
            ),
            Self::Marked => (marked(), shell_marked::MARKED_SLOTS),
            Self::Counted => (
                marked() && at(shell_marked::MARKED_SLOTS) == Some(0),
                shell_marked::LEN,
            ),
        };
        admitted.then(|| start.checked_add(slots)).flatten()
    }
}

struct WallSlotFrame {
    prefix: WallSlotPrefix,
    inside: Option<(u32, usize)>,
    outside: Option<(u32, usize)>,
    group: u32,
    auxiliary: Vec<u32>,
}

fn wall_slot(bytes: &[u8], at: usize) -> Option<(Option<(u32, usize)>, usize)> {
    match bytes.get(at)? {
        0 => Some((None, at.checked_add(1)?)),
        1 => Some((
            Some((marked_record_reference(bytes, at)?, at)),
            at.checked_add(WALL_SLOT_LEN)?,
        )),
        _ => None,
    }
}

fn zero_run_end(bytes: &[u8], at: usize, len: usize) -> Option<usize> {
    let end = at.checked_add(len)?;
    bytes
        .get(at..end)?
        .iter()
        .all(|byte| *byte == 0)
        .then_some(end)
}

fn wall_slot_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
    prefix: WallSlotPrefix,
) -> Option<WallSlotFrame> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let (inside, cursor) = wall_slot(bytes, prefix.slots_at(bytes, start)?)?;
    let (outside, cursor) = wall_slot(bytes, cursor)?;
    let mut auxiliary = Vec::new();
    let group_count_at = match prefix {
        WallSlotPrefix::Cornered => zero_run_end(bytes, cursor, CORNERED_ZERO_RUN)?,
        WallSlotPrefix::Compact | WallSlotPrefix::Marked => {
            zero_run_end(bytes, cursor, MARKED_ZERO_RUN)?
        }
        WallSlotPrefix::Counted => {
            let count_at = zero_run_end(bytes, cursor, COUNTED_ZERO_RUN)?;
            let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
            let first = count_at.checked_add(4)?;
            let end = first.checked_add(count.checked_mul(WALL_SLOT_LEN)?)?;
            auxiliary = bytes
                .get(first..end)?
                .chunks_exact(WALL_SLOT_LEN)
                .map(|entry| marked_record_reference(entry, 0))
                .collect::<Option<Vec<_>>>()?;
            zero_run_end(bytes, end, COUNTED_ZERO_RUN)?
        }
    };
    if View::u32_le_at(bytes, group_count_at)? != 1 {
        return None;
    }
    let group_at = group_count_at.checked_add(4)?;
    let group = marked_record_reference(bytes, group_at)?;
    let guid_end = fixed_guid_end(bytes, group_at.checked_add(WALL_SLOT_LEN)?)?;
    let table_at = zero_run_end(bytes, guid_end, TABLE_ZERO_RUN)?;
    (u64::try_from(table_at).ok()? == scope.reference_count_offset()).then_some(WallSlotFrame {
        prefix,
        inside,
        outside,
        group,
        auxiliary,
    })
}

pub(super) fn exact_shell_wall_slots(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    let mut frames = WallSlotPrefix::ALL
        .into_iter()
        .filter_map(|prefix| wall_slot_frame(bytes, scope, prefix));
    let (Some(frame), None) = (frames.next(), frames.next()) else {
        return None;
    };
    let table = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    let walls = [frame.inside, frame.outside]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let ordered = match frame.prefix {
        WallSlotPrefix::Cornered => {
            let scalars_at = table.len().checked_sub(walls.len())?;
            scalars_at >= 2
                && table.first() == Some(&frame.group)
                && walls
                    .iter()
                    .map(|(record, _)| *record)
                    .eq(table[scalars_at..].iter().copied())
        }
        WallSlotPrefix::Compact | WallSlotPrefix::Marked | WallSlotPrefix::Counted => {
            matches!(walls.as_slice(), [(record, _)]
                if table.len() >= 3
                    && *record != frame.group
                    && table.contains(record)
                    && table.contains(&frame.group))
        }
    };
    if !ordered {
        return None;
    }
    let wall = |record: u32| {
        let scalar = exact_fixed_scalar(bytes, records, record)?;
        Some((
            cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?,
            record,
            scalar.value_offset,
        ))
    };
    let (first_record, first_at) = walls.first().copied()?;
    let (thickness, thickness_record_index, thickness_offset) = wall(first_record)?;
    let both = match (frame.inside, frame.outside) {
        (Some(_), Some((record, _))) => {
            let (thickness, thickness_record_index, thickness_offset) = wall(record)?;
            Some(direct_face::DesignShellOutsideWall {
                thickness,
                thickness_record_index,
                thickness_offset,
            })
        }
        _ => None,
    };
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let wall_slots =
        (frame.prefix == WallSlotPrefix::Cornered).then(|| direct_face::DesignShellWallSlots {
            tangent_chain: bytes.get(start + shell_slots::TANGENT_CHAIN) == Some(&1),
            rounded_corners: bytes.get(start + shell_slots::ROUNDED_CORNERS) == Some(&1),
            outside: both,
        });
    Some(DesignDirectFaceOperation::Shell(
        direct_face::DesignShellOperation {
            thickness,
            thickness_record_index,
            thickness_offset,
            outward: frame.inside.is_none(),
            outward_offset: u64::try_from(first_at).ok()?,
            wall_slots,
            auxiliary_references: frame.auxiliary,
        },
    ))
}
