// SPDX-License-Identifier: Apache-2.0
//! Read component paste relations without requiring a neutral occurrence join.

use super::shared_frames::rigid_transform_at;
use crate::bytes::lp_utf16_bounded;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::layout::{
    copy_paste_identity_source_prefix as copy_identity_prefix, copy_paste_prefix as copy_prefix,
    cut_paste_prefix as cut_prefix, indexed_design_record_header as indexed_header,
    paste_context_owner_tail as context_tail,
};
use crate::records::feature::occurrence_transfer::{
    DesignOccurrenceTransfer, DesignOccurrenceTransferSource, DesignPasteOwner, COMPONENT_TYPE,
    CONTEXT_TYPE, OWNER_TYPE,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::View;

fn zeros(view: &mut View<'_>, count: usize) -> Option<()> {
    view.take(count)?
        .iter()
        .all(|byte| *byte == 0)
        .then_some(())
}

fn reference(view: &mut View<'_>) -> Option<u32> {
    if view.u8()? != 1 {
        return None;
    }
    let index = view.u32_le()?;
    zeros(view, 6)?;
    (index != 0).then_some(index)
}

fn reference_field(view: &mut View<'_>) -> Option<(Option<u32>, bool)> {
    let marker = view.u8()?;
    let index = view.u32_le()?;
    let trailing = view.take(6)?;
    let valid = marker == 1 && index != 0 && trailing.iter().all(|byte| *byte == 0);
    Some((valid.then_some(index), valid))
}

fn zero_field(view: &mut View<'_>, count: usize) -> Option<bool> {
    Some(view.take(count)?.iter().all(|byte| *byte == 0))
}

fn owner(bytes: &[u8], paired: usize, index: u32) -> Option<u64> {
    (View::u32_le_at(bytes, paired + 7)? == index).then_some(())?;
    let mut view = View::over_retained(bytes);
    view.seek(paired + 11)?;
    zeros(&mut view, 8)?;
    if view.u8()? != 1 {
        return None;
    }
    let value = view.u64_le()?;
    zeros(&mut view, 2)?;
    Some(value)
}

/// Read an occurrence relation. A `CopyPaste` relation can also take the
/// 58-byte form, which stores byte `1` after the carrier reference.
fn relation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    index: u32,
    admit_marked_gap: bool,
) -> Option<(usize, u32, u32, u64)> {
    let at = records.first_at_or_after(0, index)?;
    let mut view = View::over_retained(bytes);
    view.seek(at + 11)?;
    zeros(&mut view, 10)?;
    let occurrence = reference(&mut view)?;
    if admit_marked_gap && bytes.get(view.position()) == Some(&1) {
        view.take(1)?;
    }
    zeros(&mut view, 2)?;
    reference(&mut view)?;
    zeros(&mut view, 1)?;
    let scope = reference(&mut view)?;
    let paired = view.position();
    if next_indexed_record_offset(bytes, at + 1)? != paired {
        return None;
    }
    Some((at, occurrence, scope, owner(bytes, paired, index)?))
}

fn component_owner(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &std::collections::HashMap<u64, (&str, u32)>,
    collection: u64,
) -> Option<DesignPasteOwner> {
    if !types.get(&collection)?.0.eq_ignore_ascii_case(OWNER_TYPE) {
        return None;
    }
    let collection_record_index = u32::try_from(collection).ok()?;
    let collection_at = records.first_at_or_after(0, collection_record_index)?;
    let mut found = None;
    for (&context, &(kind, _)) in types {
        if !kind.eq_ignore_ascii_case(CONTEXT_TYPE) {
            continue;
        }
        let context_record_index = u32::try_from(context).ok()?;
        for (_, paired) in records.frames(context_record_index) {
            if paired.checked_add(context_tail::LEN - context_tail::PAIRED_HEADER)? != collection_at
            {
                continue;
            }
            let mut view = View::over_retained(bytes);
            view.seek(paired.checked_sub(context_tail::PAIRED_HEADER)?)?;
            if reference(&mut view)? != collection_record_index {
                continue;
            }
            reference(&mut view)?;
            let component = owner(bytes, paired, context_record_index)?;
            if !types
                .get(&component)?
                .0
                .eq_ignore_ascii_case(COMPONENT_TYPE)
            {
                return None;
            }
            let candidate = DesignPasteOwner {
                collection: collection_record_index,
                context: context_record_index,
                component: u32::try_from(component).ok()?,
            };
            if found.replace(candidate).is_some() {
                return None;
            }
        }
    }
    found
}

#[derive(Clone)]
struct CopyPastePrefix {
    relation_record_index: Option<u32>,
    table_relation_record_index: Option<u32>,
    source_transform: Option<SketchPlacementMatrix>,
    source_matrix_at: Option<usize>,
    target_transform: Option<SketchPlacementMatrix>,
    target_matrix_at: Option<usize>,
    table_at: Option<usize>,
    complete: bool,
    progress: u8,
}

pub(super) struct CompactCopyPastePrefix {
    relation_record_index: u32,
    source_at: usize,
    source_transform: SketchPlacementMatrix,
    target_at: usize,
    target_transform: SketchPlacementMatrix,
    pub(super) table_at: usize,
}

/// Read the compact `CopyPaste` prefix, or its form with four more prologue
/// zero bytes, through its one-member reference table.
pub(super) fn compact_copy_paste_prefix(
    bytes: &[u8],
    start: usize,
) -> Option<CompactCopyPastePrefix> {
    let prefix = copy_paste_prefix(bytes, start)?;
    let source_at = prefix.source_matrix_at?;
    let source_transform = prefix.source_transform?;
    let target_at = prefix.target_matrix_at?;
    let target_transform = prefix.target_transform?;
    Some(CompactCopyPastePrefix {
        relation_record_index: prefix.relation_record_index?,
        source_at,
        source_transform,
        target_at,
        target_transform,
        table_at: prefix.table_at?,
    })
}

pub(super) fn copy_paste_prefix_table(bytes: &[u8], start: usize) -> Option<usize> {
    copy_paste_prefix(bytes, start)?.table_at
}

pub(super) fn cut_paste_prefix_table(bytes: &[u8], start: usize) -> Option<usize> {
    let mut candidates = [0, 4]
        .into_iter()
        .filter_map(|shift| cut_paste_prefix_table_with_shift(bytes, start, shift));
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn cut_paste_prefix_table_with_shift(bytes: &[u8], start: usize, shift: usize) -> Option<usize> {
    let mut view = View::over_retained(bytes);
    view.seek(start.checked_add(indexed_header::LEN)?)?;
    zeros(
        &mut view,
        cut_prefix::REMOVAL_REFERENCE - indexed_header::LEN - usize::from(shift != 0),
    )?;
    if shift != 0 {
        if view.u8()? != 1 {
            return None;
        }
        zeros(&mut view, 4)?;
    }
    if view.position() != start.checked_add(cut_prefix::REMOVAL_REFERENCE + shift)? {
        return None;
    }
    reference(&mut view)?;
    reference(&mut view)?;
    if view.u8()? != 1 {
        return None;
    }
    reference(&mut view)?;
    zeros(&mut view, 4)?;
    if view.position() != start.checked_add(cut_prefix::RELATION_REFERENCE + shift)? {
        return None;
    }
    let relation = reference(&mut view)?;
    if view.position() != start.checked_add(cut_prefix::PLACEMENT_FLAG + shift)? {
        return None;
    }
    if view.u8()? != 1 {
        return None;
    }
    if view.position() != start.checked_add(cut_prefix::IDENTITY_FLAG + shift)? {
        return None;
    }
    match view.u8()? {
        1 => {}
        0 => {
            view.take(128)?;
        }
        _ => return None,
    }
    let (_, after_key) = lp_utf16_bounded(bytes, view.position(), 36..=36)?;
    view.seek(after_key)?;
    zeros(&mut view, 3)?;
    let table_at = view.position();
    if view.u32_le()? != 1 || reference(&mut view)? != relation {
        return None;
    }
    Some(table_at)
}

fn copy_paste_prefix(bytes: &[u8], start: usize) -> Option<CopyPastePrefix> {
    let candidates = [0, 4].map(|shift| copy_paste_prefix_with_shift(bytes, start, shift, None));
    let mut complete = candidates
        .iter()
        .filter(|candidate| candidate.complete)
        .cloned();
    let candidate = complete.next()?;
    complete.next().is_none().then_some(candidate)
}

fn partial_copy_paste_prefix(
    bytes: &[u8],
    start: usize,
    scope_table_at: Option<usize>,
) -> CopyPastePrefix {
    let candidates =
        [0, 4].map(|shift| copy_paste_prefix_with_shift(bytes, start, shift, scope_table_at));
    let mut complete = candidates
        .iter()
        .filter(|candidate| candidate.complete && candidate.table_at == scope_table_at)
        .cloned();
    if let Some(candidate) = complete.next() {
        if complete.next().is_none() {
            return candidate;
        }
        return CopyPastePrefix {
            progress: 0,
            ..CopyPastePrefix::default()
        };
    }
    let mut candidates = candidates.into_iter();
    let mut first = candidates.next().unwrap_or_default();
    let mut second = candidates.next().unwrap_or_default();
    if first.table_at != scope_table_at {
        first.complete = false;
        first.table_relation_record_index = None;
    }
    if second.table_at != scope_table_at {
        second.complete = false;
        second.table_relation_record_index = None;
    }
    if first.progress > second.progress {
        first
    } else if second.progress > first.progress {
        second
    } else {
        CopyPastePrefix::default()
    }
}

impl Default for CopyPastePrefix {
    fn default() -> Self {
        Self {
            relation_record_index: None,
            table_relation_record_index: None,
            source_transform: None,
            source_matrix_at: None,
            target_transform: None,
            target_matrix_at: None,
            table_at: None,
            complete: false,
            progress: 0,
        }
    }
}

fn copy_paste_prefix_with_shift(
    bytes: &[u8],
    start: usize,
    shift: usize,
    scope_table_at: Option<usize>,
) -> CopyPastePrefix {
    let mut prefix = CopyPastePrefix::default();
    let mut view = View::over_retained(bytes);
    let Some(preamble_at) = start.checked_add(indexed_header::LEN) else {
        return prefix;
    };
    if view.seek(preamble_at).is_none() {
        return prefix;
    }
    let Some(prefix_zeroes) = zero_field(
        &mut view,
        copy_identity_prefix::COMPANION_REFERENCE - indexed_header::LEN + shift,
    ) else {
        return prefix;
    };
    if !prefix_zeroes {
        return prefix;
    }
    prefix.progress = 1;
    let Some((_, companion_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    let mut all_valid = companion_valid;
    let Some(source_flag_at) =
        start.checked_add(copy_identity_prefix::SOURCE_IDENTITY_FLAG + shift)
    else {
        return prefix;
    };
    if view.position() != source_flag_at {
        return prefix;
    }
    prefix.progress = 2;
    let Some(source_flag) = view.u8() else {
        return prefix;
    };
    prefix.progress = 3;
    let source_transform = match source_flag {
        1 => Some(SketchPlacementMatrix::IDENTITY),
        0 => {
            let source_at = view.position();
            let Some(expected_source_at) = start.checked_add(copy_prefix::SOURCE_TRANSFORM + shift)
            else {
                return prefix;
            };
            if source_at != expected_source_at {
                return prefix;
            }
            prefix.source_matrix_at = Some(source_at);
            if scope_table_at.is_some_and(|table_at| {
                source_at
                    .checked_add(128)
                    .is_none_or(|matrix_end| matrix_end > table_at)
            }) {
                return prefix;
            }
            let transform = rigid_transform_at(bytes, source_at);
            if view.take(128).is_none() {
                return prefix;
            }
            transform
        }
        _ => return prefix,
    };
    prefix.source_transform = source_transform;
    prefix.progress = 4;
    let second_companion_offset = if source_flag == 1 {
        copy_identity_prefix::SECOND_COMPANION_REFERENCE
    } else {
        copy_prefix::SECOND_COMPANION_REFERENCE
    };
    let Some(second_companion_at) = start.checked_add(second_companion_offset + shift) else {
        return prefix;
    };
    if view.position() != second_companion_at {
        return prefix;
    }
    let Some((_, second_companion_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    all_valid &= second_companion_valid;
    prefix.progress = 5;
    let zero_run_4_offset = if source_flag == 1 {
        copy_identity_prefix::ZERO_RUN_4
    } else {
        copy_prefix::ZERO_RUN_4
    };
    let Some(zero_run_4_at) = start.checked_add(zero_run_4_offset + shift) else {
        return prefix;
    };
    if view.position() != zero_run_4_at {
        return prefix;
    }
    let Some(separator_valid) = zero_field(&mut view, 4) else {
        return prefix;
    };
    all_valid &= separator_valid;
    prefix.progress = 6;
    let relation_offset = if source_flag == 1 {
        copy_identity_prefix::RELATION_REFERENCE
    } else {
        copy_prefix::RELATION_REFERENCE
    };
    let Some(relation_at) = start.checked_add(relation_offset + shift) else {
        return prefix;
    };
    if view.position() != relation_at {
        return prefix;
    }
    let Some((relation, relation_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    all_valid &= relation_valid;
    prefix.relation_record_index = relation;
    prefix.progress = 7;
    let placement_flag_offset = if source_flag == 1 {
        copy_identity_prefix::PLACEMENT_FLAG
    } else {
        copy_prefix::PLACEMENT_FLAG
    };
    let Some(placement_flag_at) = start.checked_add(placement_flag_offset + shift) else {
        return prefix;
    };
    if view.position() != placement_flag_at {
        return prefix;
    }
    let Some(relation_marker) = view.u8() else {
        return prefix;
    };
    all_valid &= relation_marker == 1;
    prefix.progress = 8;
    let identity_flag_offset = if source_flag == 1 {
        copy_identity_prefix::IDENTITY_FLAG
    } else {
        copy_prefix::IDENTITY_FLAG
    };
    let Some(identity_flag_at) = start.checked_add(identity_flag_offset + shift) else {
        return prefix;
    };
    if view.position() != identity_flag_at {
        return prefix;
    }
    let Some(target_flag) = view.u8() else {
        return prefix;
    };
    prefix.progress = 9;
    let target_transform = match target_flag {
        1 => Some(SketchPlacementMatrix::IDENTITY),
        0 => {
            let target_at = view.position();
            prefix.target_matrix_at = Some(target_at);
            if scope_table_at.is_some_and(|table_at| {
                target_at
                    .checked_add(128)
                    .is_none_or(|matrix_end| matrix_end > table_at)
            }) {
                return prefix;
            }
            let transform = rigid_transform_at(bytes, target_at);
            if view.take(128).is_none() {
                return prefix;
            }
            transform
        }
        _ => return prefix,
    };
    prefix.target_transform = target_transform;
    prefix.progress = 10;
    let (key, after_key) = match lp_utf16_bounded(bytes, view.position(), 36..=36) {
        Some(value) => value,
        None => return prefix,
    };
    all_valid &= key == "00000000-0000-0000-0000-000000000000";
    if view.seek(after_key).is_none() {
        return prefix;
    }
    prefix.progress = 11;
    let Some(table_zeroes) = zero_field(&mut view, 3) else {
        return prefix;
    };
    all_valid &= table_zeroes;
    let table_at = view.position();
    prefix.table_at = Some(table_at);
    prefix.progress = 12;
    let Some(table_count) = view.u32_le() else {
        return prefix;
    };
    all_valid &= table_count == 1;
    let Some((table_relation, table_relation_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    prefix.table_relation_record_index = table_relation;
    all_valid &= table_relation_valid && table_relation == relation;
    prefix.complete = all_valid;
    prefix.progress = 13;
    prefix
}

pub(super) fn exact_occurrence_transfer(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &std::collections::HashMap<u64, (&str, u32)>,
) -> Option<DesignOccurrenceTransfer> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let (shift, is_cut, identity) = match (scope.kind_name(), scope.frame_length()) {
        ("CutPaste", 277) => (0, true, true),
        ("CutPaste", 405) => (0, true, false),
        ("CutPaste", 409) => (4, true, false),
        ("CopyPaste", _) => (0, false, false),
        _ => return None,
    };
    let relation_index = *scope.reference_members().values().next()?;
    let (relation_at, occurrence_index, scope_index, target_owner) =
        relation(bytes, records, relation_index, !is_cut)?;
    if scope_index != scope.record_index || relation_at >= start {
        return None;
    }
    let carrier = records.first_at_or_after(0, occurrence_index)?;
    if carrier >= relation_at {
        return None;
    }
    let mut carrier_view = View::over_retained(bytes);
    carrier_view.seek(carrier + 11)?;
    zeros(&mut carrier_view, 8)?;
    if carrier_view.u8()? != 1 || carrier_view.u32_le()? != 1 {
        return None;
    }
    let component = reference(&mut carrier_view)?;
    if !types
        .get(&u64::from(component))?
        .0
        .eq_ignore_ascii_case(COMPONENT_TYPE)
    {
        return None;
    }
    let mut view = View::over_retained(bytes);
    view.seek(start + 11)?;
    let (target_transform, target_transform_offset, source) = if is_cut {
        if shift == 4 {
            zeros(&mut view, 9)?;
            if view.u8()? != 1 {
                return None;
            }
            zeros(&mut view, 4)?;
        } else {
            zeros(&mut view, 10)?;
        }
        let removal_index = reference(&mut view)?;
        reference(&mut view)?;
        if view.u8()? != 1 {
            return None;
        }
        reference(&mut view)?;
        zeros(&mut view, 4)?;
        if reference(&mut view)? != relation_index
            || view.u8()? != 1
            || view.u8()? != u8::from(identity)
        {
            return None;
        }
        let transform_at = view.position();
        let transform = if identity {
            SketchPlacementMatrix::IDENTITY
        } else {
            let transform = rigid_transform_at(bytes, transform_at)?;
            view.take(128)?;
            transform
        };
        let (key, after_key) = lp_utf16_bounded(bytes, view.position(), 36..=36)?;
        view.seek(after_key)?;
        zeros(&mut view, 3)?;
        let removal = records.first_at_or_after(0, removal_index)?;
        if removal >= start || relation_at >= removal {
            return None;
        }
        let mut removed = View::over_retained(bytes);
        removed.seek(removal + 11)?;
        zeros(&mut removed, 22)?;
        let source_relation = reference(&mut removed)?;
        let (removed_key, after_removed_key) =
            lp_utf16_bounded(bytes, removed.position(), 36..=36)?;
        if removed_key != key {
            return None;
        }
        removed.seek(after_removed_key)?;
        zeros(&mut removed, 7)?;
        removed.u32_le()?;
        let (kind, _) = lp_utf16_bounded(bytes, removed.position(), 14..=14)?;
        if kind != "RemoveInstance" {
            return None;
        }
        let paired = records.first_at_or_after(removal + 11, removal_index)?;
        let source_owner = owner(bytes, paired, removal_index)?;
        let (source_at, source_occurrence, _, source_relation_owner) =
            relation(bytes, records, source_relation, false)?;
        if source_at >= relation_at
            || source_occurrence != occurrence_index
            || source_relation_owner != source_owner
        {
            return None;
        }
        (
            transform,
            (!identity)
                .then(|| u64::try_from(transform_at).ok())
                .flatten(),
            DesignOccurrenceTransferSource::Cut {
                removal_record_index: removal_index,
                source_relation_record_index: source_relation,
                source_owner: component_owner(bytes, records, types, source_owner)?,
                occurrence_key: key.try_into().ok()?,
            },
        )
    } else {
        let paired_at = usize::try_from(scope.paired_byte_offset()).ok()?;
        let prefix = compact_copy_paste_prefix(bytes.get(..paired_at)?, start)?;
        if prefix.relation_record_index != relation_index {
            return None;
        }
        view.seek(prefix.table_at)?;
        (
            prefix.target_transform,
            Some(u64::try_from(prefix.target_at).ok()?),
            DesignOccurrenceTransferSource::Copy {
                source_transform: prefix.source_transform,
                source_transform_offset: u64::try_from(prefix.source_at).ok()?,
            },
        )
    };
    if u64::try_from(view.position()).ok()? != scope.reference_count_offset() {
        return None;
    }
    let value = DesignOccurrenceTransfer {
        relation_record_index: relation_index,
        occurrence_record_index: occurrence_index,
        component_record_index: u64::from(component),
        target_owner: component_owner(bytes, records, types, target_owner)?,
        target_transform,
        target_transform_offset,
        source,
    };
    value
        .links_valid(
            scope,
            |index| u64::try_from(records.first_at_or_after(0, index)?).ok(),
            |index, guid| {
                types
                    .get(&u64::from(index))
                    .is_some_and(|(kind, _)| kind.eq_ignore_ascii_case(guid))
            },
        )
        .then_some(value)
}

pub(super) fn occurrence_transfer_intent(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &std::collections::HashMap<u64, (&str, u32)>,
) -> Option<crate::records::feature::occurrence_transfer::DesignOccurrenceTransferIntent> {
    use crate::records::feature::occurrence_transfer::{
        DesignOccurrenceTransferIntent, DesignOccurrenceTransferMode,
    };
    let mode = match scope.kind_name() {
        "CopyPaste" => DesignOccurrenceTransferMode::Copy,
        "CutPaste" => DesignOccurrenceTransferMode::Reparent,
        _ => return None,
    };
    let start = usize::try_from(scope.byte_offset()).ok();
    let scope_table_at = usize::try_from(scope.reference_count_offset()).ok();
    let prefix_end = scope_table_at
        .and_then(|table_at| table_at.checked_add(4 + 11))
        .map(|end| end.min(bytes.len()));
    let prefix_bytes = prefix_end.and_then(|end| bytes.get(..end));
    let copy_prefix = (mode == DesignOccurrenceTransferMode::Copy)
        .then(|| {
            start.zip(prefix_bytes).map(|(start, prefix_bytes)| {
                partial_copy_paste_prefix(prefix_bytes, start, scope_table_at)
            })
        })
        .flatten();
    let cut_prefix = (mode == DesignOccurrenceTransferMode::Reparent)
        .then(|| {
            start.zip(prefix_bytes).map(|(start, prefix_bytes)| {
                partial_cut_paste_prefix(prefix_bytes, start, scope_table_at)
            })
        })
        .flatten();
    let mut scope_references = scope.reference_members().values().copied();
    let scope_relation = match (scope_references.next(), scope_references.next()) {
        (Some(index), None) if index != 0 => Some(index),
        _ => None,
    };
    let prefix_relation = copy_prefix
        .as_ref()
        .and_then(|prefix| prefix.relation_record_index)
        .or_else(|| {
            cut_prefix
                .as_ref()
                .and_then(|prefix| prefix.relation_record_index)
        });
    let table_relation = copy_prefix
        .as_ref()
        .and_then(|prefix| prefix.table_relation_record_index)
        .or_else(|| {
            cut_prefix
                .as_ref()
                .and_then(|prefix| prefix.table_relation_record_index)
        });
    let relation_index = scope_relation.filter(|index| {
        prefix_relation.is_none_or(|prefix| prefix == *index)
            && table_relation.is_none_or(|table| table == *index)
    });
    let source_transform = copy_prefix
        .as_ref()
        .and_then(|prefix| prefix.source_transform);
    let target_transform = copy_prefix
        .as_ref()
        .and_then(|prefix| prefix.target_transform)
        .or_else(|| {
            cut_prefix
                .as_ref()
                .and_then(|prefix| prefix.target_transform)
        });
    let relation_links = relation_index.and_then(|relation| {
        start.and_then(|scope_at| {
            relation_targets(
                bytes,
                records,
                types,
                relation,
                Some(scope.record_index),
                mode == DesignOccurrenceTransferMode::Copy,
                scope_at,
            )
        })
    });
    let carrier = relation_links.as_ref().and_then(|links| {
        start.and_then(|scope_at| {
            occurrence_carrier_component(
                bytes,
                records,
                types,
                links.occurrence_record_index,
                links.byte_offset,
                scope_at,
            )
            .map(|component| (links.occurrence_record_index, component))
        })
    });
    let occurrence_record_index = carrier.as_ref().map(|(index, _)| *index);
    let component_record_index = carrier.and_then(|(_, component)| component);
    let target_owner_component_record_index = relation_links
        .as_ref()
        .and_then(|links| links.owner_collection)
        .and_then(|collection| {
            start.and_then(|scope_at| {
                component_owner_partial(bytes, records, types, collection, scope_at)
            })
        })
        .map(|owner| owner.component);
    let source_owner_component_record_index = match (
        mode,
        cut_prefix.as_ref(),
        occurrence_record_index,
        relation_links.as_ref(),
        start,
    ) {
        (
            DesignOccurrenceTransferMode::Reparent,
            Some(prefix),
            Some(occurrence),
            Some(target_relation),
            Some(scope_at),
        ) => cut_source_owner_component(
            bytes,
            records,
            types,
            prefix,
            occurrence,
            target_relation,
            scope_at,
        ),
        _ => None,
    };
    Some(DesignOccurrenceTransferIntent {
        mode,
        relation_record_index: relation_index,
        occurrence_record_index,
        component_record_index,
        source_owner_component_record_index,
        target_owner_component_record_index,
        source_transform: (mode == DesignOccurrenceTransferMode::Copy)
            .then_some(source_transform)
            .flatten(),
        target_transform,
    })
}

#[derive(Clone, Default)]
struct CutPastePrefix {
    relation_record_index: Option<u32>,
    table_relation_record_index: Option<u32>,
    removal_record_index: Option<u32>,
    occurrence_key: Option<String>,
    target_transform: Option<SketchPlacementMatrix>,
    table_at: Option<usize>,
    complete: bool,
    progress: u8,
}

fn partial_cut_paste_prefix(
    bytes: &[u8],
    start: usize,
    scope_table_at: Option<usize>,
) -> CutPastePrefix {
    let candidates =
        [0, 4].map(|shift| cut_paste_prefix_with_shift(bytes, start, shift, scope_table_at));
    let mut complete = candidates
        .iter()
        .filter(|candidate| candidate.complete && candidate.table_at == scope_table_at)
        .cloned();
    if let Some(candidate) = complete.next() {
        if complete.next().is_none() {
            return candidate;
        }
        return CutPastePrefix::default();
    }
    let mut candidates = candidates.into_iter();
    let mut first = candidates.next().unwrap_or_default();
    let mut second = candidates.next().unwrap_or_default();
    if first.table_at != scope_table_at {
        first.complete = false;
        first.table_relation_record_index = None;
    }
    if second.table_at != scope_table_at {
        second.complete = false;
        second.table_relation_record_index = None;
    }
    if first.progress > second.progress {
        first
    } else if second.progress > first.progress {
        second
    } else {
        CutPastePrefix::default()
    }
}

fn cut_paste_prefix_with_shift(
    bytes: &[u8],
    start: usize,
    shift: usize,
    scope_table_at: Option<usize>,
) -> CutPastePrefix {
    let mut prefix = CutPastePrefix::default();
    let mut view = View::over_retained(bytes);
    let Some(preamble_at) = start.checked_add(indexed_header::LEN) else {
        return prefix;
    };
    if view.seek(preamble_at).is_none() {
        return prefix;
    }
    let zero_run = cut_prefix::REMOVAL_REFERENCE - indexed_header::LEN - usize::from(shift != 0);
    if zero_field(&mut view, zero_run) != Some(true) {
        return prefix;
    }
    prefix.progress = 1;
    if shift != 0 {
        if view.u8() != Some(1) || zero_field(&mut view, 4) != Some(true) {
            return prefix;
        }
    }
    let Some(removal_at) = start.checked_add(cut_prefix::REMOVAL_REFERENCE + shift) else {
        return prefix;
    };
    if view.position() != removal_at {
        return prefix;
    }
    let Some((removal, removal_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    prefix.removal_record_index = removal;
    let mut all_valid = removal_valid;
    prefix.progress = 2;
    let Some((_, first_companion_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    all_valid &= first_companion_valid;
    prefix.progress = 3;
    let Some(companion_flag) = view.u8() else {
        return prefix;
    };
    all_valid &= companion_flag == 1;
    prefix.progress = 4;
    let Some((_, second_companion_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    all_valid &= second_companion_valid;
    prefix.progress = 5;
    let Some(zero_run_valid) = zero_field(&mut view, 4) else {
        return prefix;
    };
    all_valid &= zero_run_valid;
    let Some(relation_at) = start.checked_add(cut_prefix::RELATION_REFERENCE + shift) else {
        return prefix;
    };
    if view.position() != relation_at {
        return prefix;
    }
    prefix.progress = 6;
    let Some((relation, relation_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    all_valid &= relation_valid;
    prefix.relation_record_index = relation;
    prefix.progress = 7;
    let Some(placement_marker) = view.u8() else {
        return prefix;
    };
    all_valid &= placement_marker == 1;
    let Some(identity_flag_at) = start.checked_add(cut_prefix::IDENTITY_FLAG + shift) else {
        return prefix;
    };
    if view.position() != identity_flag_at {
        return prefix;
    }
    prefix.progress = 8;
    let Some(identity_flag) = view.u8() else {
        return prefix;
    };
    let target_transform = match identity_flag {
        1 => Some(SketchPlacementMatrix::IDENTITY),
        0 => {
            let transform_at = view.position();
            if scope_table_at.is_some_and(|table_at| {
                transform_at
                    .checked_add(128)
                    .is_none_or(|matrix_end| matrix_end > table_at)
            }) {
                return prefix;
            }
            let transform = rigid_transform_at(bytes, transform_at);
            if view.take(128).is_none() {
                return prefix;
            }
            transform
        }
        _ => return prefix,
    };
    prefix.target_transform = target_transform;
    prefix.progress = 9;
    let (key, after_key) = match lp_utf16_bounded(bytes, view.position(), 36..=36) {
        Some(value) => value,
        None => return prefix,
    };
    prefix.occurrence_key = Some(key);
    if view.seek(after_key).is_none() {
        return prefix;
    }
    prefix.progress = 10;
    let Some(table_zeroes) = zero_field(&mut view, 3) else {
        return prefix;
    };
    all_valid &= table_zeroes;
    prefix.table_at = Some(view.position());
    let Some(table_count) = view.u32_le() else {
        return prefix;
    };
    let Some((table_relation, table_relation_valid)) = reference_field(&mut view) else {
        return prefix;
    };
    all_valid &= table_count == 1 && table_relation_valid && table_relation == relation;
    prefix.table_relation_record_index = table_relation;
    prefix.complete = all_valid;
    prefix.progress = 11;
    prefix
}

struct RelationTargets {
    byte_offset: usize,
    occurrence_record_index: u32,
    owner_collection: Option<u64>,
}

fn relation_targets(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &std::collections::HashMap<u64, (&str, u32)>,
    index: u32,
    expected_scope: Option<u32>,
    admit_marked_gap: bool,
    scope_at: usize,
) -> Option<RelationTargets> {
    let mut found = None;
    for at in records.offsets(index).iter().copied() {
        if at >= scope_at {
            continue;
        }
        let mut view = View::over_retained(bytes);
        if view.seek(at.checked_add(11)?).is_none() || zeros(&mut view, 10).is_none() {
            continue;
        }
        let Some(occurrence_record_index) = reference(&mut view) else {
            continue;
        };
        if bytes.get(view.position()) == Some(&1) {
            if !admit_marked_gap || view.take(1).is_none() {
                continue;
            }
        }
        if zeros(&mut view, 2).is_none()
            || reference(&mut view).is_none()
            || zeros(&mut view, 1).is_none()
        {
            continue;
        }
        let Some(scope_record_index) = reference(&mut view) else {
            continue;
        };
        if expected_scope.is_some_and(|expected| expected != scope_record_index) {
            continue;
        }
        let paired = view.position();
        if next_indexed_record_offset(bytes, at.checked_add(1)?) != Some(paired)
            || records.first_at_or_after(at.checked_add(1)?, index) != Some(paired)
        {
            continue;
        }
        let owner_collection = owner(bytes, paired, index).filter(|collection| {
            *collection != 0
                && types
                    .get(collection)
                    .is_some_and(|(type_guid, _)| type_guid.eq_ignore_ascii_case(OWNER_TYPE))
        });
        if found.is_some() {
            return None;
        }
        found = Some(RelationTargets {
            byte_offset: at,
            occurrence_record_index,
            owner_collection,
        });
    }
    found
}

fn occurrence_carrier_component(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &std::collections::HashMap<u64, (&str, u32)>,
    occurrence_record_index: u32,
    relation_at: usize,
    scope_at: usize,
) -> Option<Option<u64>> {
    if !types
        .get(&u64::from(occurrence_record_index))
        .is_some_and(|(type_guid, _)| {
            type_guid.eq_ignore_ascii_case(
                crate::records::feature::assembly_features::OCCURRENCE_PLACEMENT_TYPE_GUID,
            )
        })
    {
        return None;
    }
    let mut found = None;
    for carrier_at in records.offsets(occurrence_record_index).iter().copied() {
        if carrier_at >= relation_at || carrier_at >= scope_at {
            continue;
        }
        let mut carrier = View::over_retained(bytes);
        if carrier.seek(carrier_at.checked_add(11)?).is_none()
            || zeros(&mut carrier, 8).is_none()
            || carrier.u8() != Some(1)
            || carrier.u32_le() != Some(1)
        {
            continue;
        }
        let component = reference_field(&mut carrier)
            .and_then(|(index, valid)| valid.then_some(index).flatten())
            .filter(|component| {
                let Some(component_at) = records.first_at_or_after(0, *component) else {
                    return false;
                };
                types
                    .get(&u64::from(*component))
                    .is_some_and(|(type_guid, _)| type_guid.eq_ignore_ascii_case(COMPONENT_TYPE))
                    && component_at < relation_at
                    && component_at < scope_at
            })
            .map(u64::from);
        if found.is_some() {
            return None;
        }
        found = Some(component);
    }
    found
}

fn component_owner_partial(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &std::collections::HashMap<u64, (&str, u32)>,
    collection: u64,
    scope_at: usize,
) -> Option<DesignPasteOwner> {
    if collection == 0 || !types.get(&collection)?.0.eq_ignore_ascii_case(OWNER_TYPE) {
        return None;
    }
    let collection_record_index = u32::try_from(collection).ok()?;
    let collection_at = records.first_at_or_after(0, collection_record_index)?;
    if collection_at >= scope_at {
        return None;
    }
    let mut view = View::over_retained(bytes);
    let mut found = None;
    for (&context, &(context_type, _)) in types {
        if !context_type.eq_ignore_ascii_case(CONTEXT_TYPE) {
            continue;
        }
        let Ok(context_record_index) = u32::try_from(context) else {
            continue;
        };
        for (context_at, paired) in records.frames(context_record_index) {
            let Some(next_at) = context_at.checked_add(1) else {
                continue;
            };
            if paired.checked_add(context_tail::LEN - context_tail::PAIRED_HEADER)
                != Some(collection_at)
                || next_indexed_record_offset(bytes, next_at) != Some(paired)
                || records.first_at_or_after(next_at, context_record_index) != Some(paired)
            {
                continue;
            }
            if context_at >= collection_at || context_at >= scope_at {
                continue;
            }
            let Some(paired_payload) = paired.checked_sub(context_tail::PAIRED_HEADER) else {
                continue;
            };
            if view.seek(paired_payload).is_none() {
                continue;
            }
            let Some((collection_ref, collection_valid)) = reference_field(&mut view) else {
                continue;
            };
            let Some((_, companion_valid)) = reference_field(&mut view) else {
                continue;
            };
            if !collection_valid
                || collection_ref != Some(collection_record_index)
                || !companion_valid
            {
                continue;
            }
            let Some(component) = owner(bytes, paired, context_record_index) else {
                continue;
            };
            if component == 0
                || !types
                    .get(&component)
                    .is_some_and(|(type_guid, _)| type_guid.eq_ignore_ascii_case(COMPONENT_TYPE))
            {
                continue;
            }
            let Ok(component_record_index) = u32::try_from(component) else {
                continue;
            };
            let Some(component_at) = records.first_at_or_after(0, component_record_index) else {
                continue;
            };
            if component_at >= context_at || component_at >= scope_at {
                continue;
            }
            let candidate = DesignPasteOwner {
                collection: collection_record_index,
                context: context_record_index,
                component: component_record_index,
            };
            if found.replace(candidate).is_some() {
                return None;
            }
        }
    }
    found
}

fn cut_source_owner_component(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &std::collections::HashMap<u64, (&str, u32)>,
    prefix: &CutPastePrefix,
    occurrence_record_index: u32,
    target_relation: &RelationTargets,
    scope_at: usize,
) -> Option<u32> {
    let removal_record_index = prefix.removal_record_index?;
    let occurrence_key = prefix.occurrence_key.as_deref()?;
    let mut found = None;
    for removal_at in records.offsets(removal_record_index).iter().copied() {
        if removal_at >= scope_at {
            continue;
        }
        let mut removed = View::over_retained(bytes);
        if removed.seek(removal_at.checked_add(11)?).is_none() || zeros(&mut removed, 22).is_none()
        {
            continue;
        }
        let Some(source_relation_index) = reference(&mut removed) else {
            continue;
        };
        let (key, after_key) = match lp_utf16_bounded(bytes, removed.position(), 36..=36) {
            Some(value) => value,
            None => continue,
        };
        if key != occurrence_key
            || removed.seek(after_key).is_none()
            || zeros(&mut removed, 7).is_none()
            || removed.u32_le().is_none()
        {
            continue;
        }
        let (kind, after_kind) = match lp_utf16_bounded(bytes, removed.position(), 14..=14) {
            Some(value) => value,
            None => continue,
        };
        if kind != "RemoveInstance" {
            continue;
        }
        if removed.seek(after_kind).is_none() {
            continue;
        }
        let paired = removed.position();
        if next_indexed_record_offset(bytes, removal_at.checked_add(1)?) != Some(paired)
            || records.first_at_or_after(removal_at.checked_add(1)?, removal_record_index)
                != Some(paired)
            || target_relation.byte_offset >= removal_at
        {
            continue;
        }
        let Some(source_collection) = owner(bytes, paired, removal_record_index) else {
            continue;
        };
        if source_collection == 0
            || !types
                .get(&source_collection)
                .is_some_and(|(type_guid, _)| type_guid.eq_ignore_ascii_case(OWNER_TYPE))
        {
            continue;
        }
        let Some(source_relation) = relation_targets(
            bytes,
            records,
            types,
            source_relation_index,
            None,
            false,
            scope_at,
        ) else {
            continue;
        };
        if source_relation.byte_offset >= target_relation.byte_offset
            || source_relation.occurrence_record_index != occurrence_record_index
            || source_relation.owner_collection != Some(source_collection)
            || occurrence_carrier_component(
                bytes,
                records,
                types,
                occurrence_record_index,
                source_relation.byte_offset,
                scope_at,
            )
            .is_none()
        {
            continue;
        }
        let Some(source_owner) =
            component_owner_partial(bytes, records, types, source_collection, scope_at)
        else {
            continue;
        };
        if found.replace(source_owner.component).is_some() {
            return None;
        }
    }
    found
}

#[cfg(test)]
mod tests;
