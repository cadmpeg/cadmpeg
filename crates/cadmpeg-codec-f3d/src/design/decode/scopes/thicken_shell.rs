// SPDX-License-Identifier: Apache-2.0
//! Exact thicken and shell scope frames.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_guid_end;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::layout::shell_compact_slot_scope_prefix as shell_compact;
use crate::layout::shell_marked_slot_scope_prefix as shell_marked;
use crate::layout::shell_wall_slot_scope_prefix as shell_slots;
use crate::layout::thicken_scope_prologue as thicken;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::direct_face::DesignThickenBoolean;
use crate::records::feature::direct_face::DesignThickenBooleanOperation;
use crate::records::feature::direct_face::DesignThickenOperation;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_ir::report::loss::LossNote;

const WALL_SLOT_LEN: usize = 11;
const MARKED_REFERENCE_LEN: usize = 11;
const U32_LEN: usize = 4;
const THICKEN_OPERATIONS: std::ops::RangeInclusive<u32> = 1..=4;
const THICKEN_NEW_BODY: u32 = 4;
const TABLE_ZERO_RUN: usize = 3;
const CORNERED_ZERO_RUN: usize = 12;
const MARKED_ZERO_RUN: usize = 8;
const COUNTED_ZERO_RUN: usize = 4;

pub(super) fn exact_thicken_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    losses: &mut Vec<LossNote>,
) -> Option<DesignThickenOperation> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Thicken) {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    zero_run_end(
        bytes,
        start + thicken::ZERO_RUN_9,
        thicken::PROLOGUE_MARKER - thicken::ZERO_RUN_9,
    )?;
    let prologue_end = match bytes.get(start + thicken::PROLOGUE_MARKER)? {
        0 => start + thicken::LEN,
        1 if View::u32_le_at(bytes, start + thicken::LEN)? == 0 => start + thicken::LEN + U32_LEN,
        _ => return None,
    };
    let (operation_at, prologue_flag) =
        if THICKEN_OPERATIONS.contains(&View::u32_le_at(bytes, prologue_end)?) {
            (prologue_end, None)
        } else {
            match bytes.get(prologue_end)? {
                flag @ (0 | 1) => (prologue_end + 1, Some((prologue_end, *flag))),
                _ => return None,
            }
        };
    let operation = View::u32_le_at(bytes, operation_at)?;
    if !THICKEN_OPERATIONS.contains(&operation) {
        return None;
    }
    let group_count = View::u32_le_at(bytes, operation_at + U32_LEN)?;
    let mut cursor = operation_at + 2 * U32_LEN;
    let typed = crate::bytes::typed_local_reference_target(bytes, cursor).is_some();
    let reference_len = if typed {
        crate::bytes::TYPED_LOCAL_REFERENCE_LEN
    } else {
        MARKED_REFERENCE_LEN
    };
    let table = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    for _ in 0..group_count {
        groups.push(crate::bytes::local_reference_target(bytes, cursor, typed)?);
        cursor += reference_len;
    }
    let flags_at = cursor;
    let flags = bytes.get(flags_at..flags_at + 2)?;
    if flags.iter().any(|flag| *flag > 1) {
        return None;
    }
    cursor += 2;
    let thickness_record_index = crate::bytes::local_reference_target(bytes, cursor, typed)?;
    cursor += reference_len;
    let operands = if table.first() == Some(&thickness_record_index) {
        table.get(1..)?
    } else if table.last() == Some(&thickness_record_index) {
        table.get(..table.len() - 1)?
    } else {
        return None;
    };
    if groups.first() != operands.first() || groups.is_empty() {
        return None;
    }
    let mut remaining = operands.iter();
    for group in &groups {
        remaining.by_ref().find(|operand| *operand == group)?;
    }
    let runs = [cursor, cursor + U32_LEN]
        .into_iter()
        .filter(|runs_at| {
            *runs_at == cursor || View::u32_le_at(bytes, cursor).is_some_and(|word| word == 0)
        })
        .find_map(|runs_at| thicken_reference_runs(bytes, runs_at, typed, scope))?;
    if !runs[1].is_empty() {
        return None;
    }
    let boolean = match (operation, runs[2].as_slice()) {
        (THICKEN_NEW_BODY, []) => None,
        (THICKEN_NEW_BODY, _) | (_, [] | [_, _, ..]) => return None,
        (_, [target]) => {
            if !operands.contains(target) {
                return None;
            }
            Some(DesignThickenBoolean {
                operation: match operation {
                    1 => DesignThickenBooleanOperation::Join,
                    2 => DesignThickenBooleanOperation::Cut,
                    _ => DesignThickenBooleanOperation::Intersect,
                },
                operation_offset: u64::try_from(operation_at).ok()?,
                target_group_record_index: *target,
            })
        }
    };
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    if scalar.value.get() == 0.0 {
        return None;
    }
    let prologue = prologue_flag.map_or_else(String::new, |(at, flag)| {
        format!(" and prologue flag byte {flag:02x} at byte {at}")
    });
    losses.push(crate::loss::F3dLossCode::FeatureThickenFlagsAssumed.note(format!(
        "Thicken scope {} stores flag bytes {:02x} {:02x} at byte {flags_at}{prologue}; the Thicken settings these flags select are unresolved (DR-313), and a Thicken projected from the scope takes its side from the sign of the thickness.",
        scope.id, flags[0], flags[1]
    )));
    Some(DesignThickenOperation {
        signed_thickness: scalar.value,
        thickness_record_index,
        thickness_offset: scalar.value_offset,
        boolean,
    })
}

fn thicken_reference_runs(
    bytes: &[u8],
    mut cursor: usize,
    typed: bool,
    scope: &DesignParameterScope,
) -> Option<[Vec<u32>; 3]> {
    let reference_len = if typed {
        crate::bytes::TYPED_LOCAL_REFERENCE_LEN
    } else {
        MARKED_REFERENCE_LEN
    };
    let mut runs = [Vec::new(), Vec::new(), Vec::new()];
    for run in &mut runs {
        let count = View::u32_le_at(bytes, cursor)?;
        cursor = cursor.checked_add(U32_LEN)?;
        for _ in 0..count {
            run.push(crate::bytes::local_reference_target(bytes, cursor, typed)?);
            cursor = cursor.checked_add(reference_len)?;
        }
    }
    let table_at = zero_run_end(bytes, fixed_guid_end(bytes, cursor)?, TABLE_ZERO_RUN)?;
    (u64::try_from(table_at).ok()? == scope.reference_count_offset()).then_some(runs)
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

    fn flag(self) -> Option<(usize, &'static str)> {
        match self {
            Self::Cornered => None,
            Self::Compact => Some((shell_compact::FLAG, "compact")),
            Self::Marked => Some((shell_marked::FLAG, "marked")),
            Self::Counted => Some((shell_marked::FLAG, "counted")),
        }
    }

    fn slots_at(self, bytes: &[u8], start: usize) -> Option<usize> {
        let at = |offset: usize| bytes.get(start.checked_add(offset)?).copied();
        let zero = |from: usize, to: usize| {
            start
                .checked_add(from)
                .and_then(|at| zero_run_end(bytes, at, to.checked_sub(from)?))
                .is_some()
        };
        let flag = |offset: usize| matches!(at(offset), Some(0 | 1));
        let marked = || {
            zero(shell_marked::ZERO_RUN_9, shell_marked::FEATURE_FORM)
                && at(shell_marked::FEATURE_FORM) == Some(shell_marked::FEATURE_FORM_VALUE)
                && flag(shell_marked::FLAG)
                && zero(shell_marked::ZERO_RUN_3, shell_marked::SLOT_MARKER)
                && at(shell_marked::SLOT_MARKER) == Some(shell_marked::SLOT_MARKER_VALUE)
        };
        let (admitted, slots) = match self {
            Self::Cornered => (
                zero(shell_slots::ZERO_RUN_10, shell_slots::TANGENT_CHAIN)
                    && flag(shell_slots::TANGENT_CHAIN)
                    && flag(shell_slots::ROUNDED_CORNERS),
                shell_slots::LEN,
            ),
            Self::Compact => (
                zero(shell_compact::ZERO_RUN_10, shell_compact::FLAG) && flag(shell_compact::FLAG),
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
    losses: &mut Vec<LossNote>,
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
    let prefix_flag = match frame.prefix.flag() {
        Some((flag, name)) => {
            let at = start.checked_add(flag)?;
            let stored = direct_face::DesignShellPrefixFlag {
                value: bytes.get(at) == Some(&1),
                offset: u64::try_from(at).ok()?,
            };
            Some((stored, flag, name))
        }
        None => None,
    };
    let outward_offset = u64::try_from(first_at).ok()?;
    let wall_slots =
        (frame.prefix == WallSlotPrefix::Cornered).then(|| direct_face::DesignShellWallSlots {
            tangent_chain: bytes.get(start + shell_slots::TANGENT_CHAIN) == Some(&1),
            rounded_corners: bytes.get(start + shell_slots::ROUNDED_CORNERS) == Some(&1),
            outside: both,
        });
    if let Some((flag, header_offset, name)) = prefix_flag {
        losses.push(crate::loss::F3dLossCode::FeatureShellPrefixFlagAssumed.note(format!(
            "Shell scope {} stores Boolean {} at byte {} (primary-header offset {header_offset} of its {name} prefix); the Shell option this Boolean selects is unresolved (DR-315), and the present wall slot sets the wall direction.",
            scope.id,
            u8::from(flag.value),
            flag.offset,
        )));
    }
    Some(DesignDirectFaceOperation::Shell(
        direct_face::DesignShellOperation {
            thickness,
            thickness_record_index,
            thickness_offset,
            outward: frame.inside.is_none(),
            outward_offset,
            wall_slots,
            prefix_flag: prefix_flag.map(|(flag, _, _)| flag),
            auxiliary_references: frame.auxiliary,
        },
    ))
}
