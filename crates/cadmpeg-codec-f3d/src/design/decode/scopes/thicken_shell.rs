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
use crate::records::feature::direct_face::DesignThickenOperation;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_ir::report::loss::LossNote;

const WALL_SLOT_LEN: usize = 11;
const MARKED_REFERENCE_LEN: usize = 11;
const U32_LEN: usize = 4;
const TABLE_ZERO_RUN: usize = 3;
const CORNERED_ZERO_RUN: usize = 12;
const MARKED_ZERO_RUN: usize = 8;
const COUNTED_ZERO_RUN: usize = 4;

struct ThickenFrame<'a> {
    groups: &'a [u8],
    identities: &'a [u8],
    typed: bool,
    thickness_record_index: u32,
    flags: [u8; 2],
    boolean: Option<crate::records::feature::direct_face::DesignThickenBoolean>,
    prologue_flag: Option<(usize, u8)>,
}

pub(super) fn exact_thicken_operation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    losses: &mut Vec<LossNote>,
) -> Result<Option<DesignThickenOperation>, cadmpeg_core::CodecError> {
    let Some(frame) = thicken_frame(bytes, scope) else {
        return Ok(None);
    };
    let Some(group_record_indices) =
        materialize_thicken_references(ctx, frame.groups, frame.typed)?
    else {
        return Ok(None);
    };
    let Some(identity_record_indices) =
        materialize_thicken_references(ctx, frame.identities, frame.typed)?
    else {
        return Ok(None);
    };
    let scalar = exact_fixed_scalar(bytes, records, frame.thickness_record_index);
    if scalar.is_none() {
        losses.push(crate::loss::F3dLossCode::ThickenThicknessUnresolved.note(format!(
            "Thicken {} retains scalar reference {} as its unresolved signed thickness because its definition is missing, ambiguous or outside the scalar grammar.",
            scope.id, frame.thickness_record_index
        )));
    }
    if let Some((at, 1)) = frame.prologue_flag {
        losses.push(crate::loss::F3dLossCode::FeatureThickenFlagsAssumed.note(format!(
            "Thicken scope {} assumes prologue flag byte 01 at byte {at} preserves its decoded result, symmetry, tangent chain and signed thickness (DR-313).",
            scope.id
        )));
    }
    Ok(Some(DesignThickenOperation {
        signed_thickness: scalar.as_ref().map(|value| value.value),
        thickness_record_index: frame.thickness_record_index,
        thickness_offset: scalar.map(|value| value.value_offset),
        symmetric: frame.flags[0] == 1,
        tangent_chain: frame.flags[1] == 1,
        group_record_indices,
        identity_record_indices,
        boolean: frame.boolean,
    }))
}

fn thicken_frame<'a>(bytes: &'a [u8], scope: &DesignParameterScope) -> Option<ThickenFrame<'a>> {
    use crate::records::feature::direct_face::{
        DesignThickenBoolean, DesignThickenBooleanOperation,
    };

    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Thicken) {
        return None;
    }
    let prefix_end = usize::try_from(scope.reference_count_offset()).ok()?;
    let bytes = bytes.get(..prefix_end)?;
    let start = usize::try_from(scope.byte_offset()).ok()?;
    zero_run_end(
        bytes,
        start.checked_add(thicken::ZERO_RUN_9)?,
        thicken::PROLOGUE_MARKER - thicken::ZERO_RUN_9,
    )?;
    let prologue_end = match bytes.get(start.checked_add(thicken::PROLOGUE_MARKER)?)? {
        0 => start.checked_add(thicken::LEN)?,
        1 if View::u32_le_at(bytes, start.checked_add(thicken::LEN)?)? == 0 => {
            start.checked_add(thicken::LEN)?.checked_add(U32_LEN)?
        }
        _ => return None,
    };
    let (operation_at, prologue_flag) = if matches!(View::u32_le_at(bytes, prologue_end)?, 1..=4) {
        (prologue_end, None)
    } else {
        let flag = *bytes.get(prologue_end)?;
        if flag > 1 {
            return None;
        }
        (prologue_end.checked_add(1)?, Some((prologue_end, flag)))
    };
    let operation = View::u32_le_at(bytes, operation_at)?;
    if !matches!(operation, 1..=4) {
        return None;
    }
    let mut cursor = operation_at.checked_add(U32_LEN)?;
    if View::u32_le_at(bytes, cursor)? == 0 {
        return None;
    }
    let typed =
        crate::bytes::typed_local_reference_target(bytes, cursor.checked_add(U32_LEN)?).is_some();
    let width = thicken_reference_width(typed);
    let groups = counted_thicken_references(bytes, &mut cursor, typed)?;
    let flags_end = cursor.checked_add(2)?;
    let flags = [*bytes.get(cursor)?, *bytes.get(cursor.checked_add(1)?)?];
    if flags.iter().any(|flag| *flag > 1) {
        return None;
    }
    let thickness_record_index = crate::bytes::local_reference_target(bytes, flags_end, typed)?;
    cursor = flags_end.checked_add(width)?;
    let members = scope.reference_members();
    let scalar_first = members.values().next().copied() == Some(thickness_record_index);
    if !scalar_first && members.values().last().copied() != Some(thickness_record_index) {
        return None;
    }
    let operand_count = members.len().checked_sub(1)?;
    let operands = || {
        members
            .values()
            .skip(usize::from(scalar_first))
            .take(operand_count)
    };
    let mut remaining = operands();
    let first_group = crate::bytes::local_reference_target(groups, 0, typed)?;
    if first_group == thickness_record_index || remaining.next().copied() != Some(first_group) {
        return None;
    }
    for chunk in groups.chunks_exact(width).skip(1) {
        let group = crate::bytes::local_reference_target(chunk, 0, typed)?;
        if group == thickness_record_index {
            return None;
        }
        remaining.by_ref().find(|reference| **reference == group)?;
    }
    let runs = [Some(cursor), cursor.checked_add(U32_LEN)]
        .into_iter()
        .flatten()
        .filter(|at| *at == cursor || View::u32_le_at(bytes, cursor) == Some(0))
        .find_map(|at| thicken_reference_runs(bytes, at, typed))?;
    if !runs[1].is_empty() {
        return None;
    }
    let boolean = match operation {
        4 if runs[2].is_empty() => None,
        1..=3 if runs[2].len() == width => {
            let target = crate::bytes::local_reference_target(runs[2], 0, typed)?;
            if target == thickness_record_index || !operands().any(|reference| *reference == target)
            {
                return None;
            }
            Some(DesignThickenBoolean {
                operation: match operation {
                    1 => DesignThickenBooleanOperation::Join,
                    2 => DesignThickenBooleanOperation::Cut,
                    _ => DesignThickenBooleanOperation::Intersect,
                },
                operation_offset: cadmpeg_core::decode::u64_from_index(operation_at),
                target_group_record_index: target,
            })
        }
        _ => return None,
    };
    Some(ThickenFrame {
        groups,
        identities: runs[0],
        typed,
        thickness_record_index,
        flags,
        boolean,
        prologue_flag,
    })
}

fn thicken_reference_width(typed: bool) -> usize {
    if typed {
        crate::bytes::TYPED_LOCAL_REFERENCE_LEN
    } else {
        MARKED_REFERENCE_LEN
    }
}

fn counted_thicken_references<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    typed: bool,
) -> Option<&'a [u8]> {
    let mut view = View::over_retained(bytes.get(*cursor..)?);
    let count = view.u32_le()?;
    let width = thicken_reference_width(typed);
    let count = view.counted(u64::from(count), width)?.get();
    let window = view.take(count.checked_mul(width)?)?;
    if window
        .chunks_exact(width)
        .any(|chunk| crate::bytes::local_reference_target(chunk, 0, typed).is_none())
    {
        return None;
    }
    *cursor = cursor.checked_add(view.position())?;
    Some(window)
}

fn thicken_reference_runs(bytes: &[u8], mut cursor: usize, typed: bool) -> Option<[&[u8]; 3]> {
    let mut runs = [&[][..]; 3];
    for run in &mut runs {
        *run = counted_thicken_references(bytes, &mut cursor, typed)?;
    }
    let table_at = zero_run_end(bytes, fixed_guid_end(bytes, cursor)?, TABLE_ZERO_RUN)?;
    (table_at == bytes.len()).then_some(runs)
}

fn materialize_thicken_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    typed: bool,
) -> Result<Option<Vec<u32>>, cadmpeg_core::CodecError> {
    let width = thicken_reference_width(typed);
    if !bytes.len().is_multiple_of(width) {
        return Ok(None);
    }
    let Some(count) = View::over_retained(bytes).counted(
        cadmpeg_core::decode::u64_from_index(bytes.len() / width),
        width,
    ) else {
        return Ok(None);
    };
    let mut values = ctx.alloc_filled(count.get(), 0u32, "F3D Thicken references")?;
    for (value, chunk) in values.iter_mut().zip(bytes.chunks_exact(width)) {
        let Some(reference) = crate::bytes::local_reference_target(chunk, 0, typed) else {
            return Ok(None);
        };
        *value = reference;
    }
    Ok(Some(values))
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
