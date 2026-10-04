// SPDX-License-Identifier: Apache-2.0
//! Decode the auxiliary BRep-cell carrier of a `SurfaceTrim` operation.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::operands::parse_entity_selection_frame;
use crate::design::decode::scopes::shared_frames::exact_indexed_header_at;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::{
    cached_owned_record_offsets, indexed_record_header_at, next_indexed_record_offset,
    IndexedRecordOffsets,
};
use crate::design::decode::text::design_record_id_charged;
use crate::ids::native_stream;
use crate::records::feature::{
    scope::DesignParameterScope,
    surface_ops::{
        DesignSurfaceTrimCellEntry, DesignSurfaceTrimChainRecord, DesignSurfaceTrimOperation,
    },
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode the exact auxiliary BRep-cell carrier of a `SurfaceTrim` scope.
///
/// The carrier is reached from the trimming entity selection. Two indexed
/// records precede the cell table. The table itself is class-287 or class-325
/// and has one 19-byte entry for each marked cell reference. The entries are
/// cells selected for removal, and the trailing value is the total cell count
/// of the operation's partition.
fn exact_surface_trim_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignSurfaceTrimOperation>, CodecError> {
    let parsed_prefix = (|| {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
            || scope.reference_members().len() != 4
        {
            return None;
        }
        let reference_members = scope.reference_members();
        let unlocated_members = match ctx.admit_iter(
            reference_members.unlocated_values().unwrap_or(&[]),
            "find F3D SurfaceTrim selection reference",
        ) {
            Ok(members) => members,
            Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
        };
        let located_members = match ctx.admit_iter(
            reference_members.located_rows().unwrap_or(&[]),
            "find F3D SurfaceTrim selection reference",
        ) {
            Ok(members) => members,
            Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
        };
        let selection_record_index = *unlocated_members
            .chain(located_members.map(|member| &member.value))
            .nth(3)?;
        let (selection_byte_offset, _) = match records
            .frames(ctx, selection_record_index)
        {
            Ok(mut frames) => frames.next()?,
            Err(error) => return Some(Err(error)),
        };
        let selection_class_tag =
            exact_indexed_header_at(bytes, selection_byte_offset, selection_record_index)?;
        let selection = match parse_entity_selection_frame(
            ctx,
            bytes,
            selection_record_index,
            u64::try_from(selection_byte_offset).ok()?,
            &selection_class_tag,
        )? {
            Ok(selection) => selection,
            Err(error) => return Some(Err(error)),
        };

        let mut chain_start = usize::try_from(selection.next_byte_offset).ok()?;
        let mut next_chain_record = || -> Option<Result<DesignSurfaceTrimChainRecord, CodecError>> {
            let parsed = indexed_record_header_at(bytes, chain_start)?;
            let frame_end = match next_indexed_record_offset(ctx, bytes, chain_start.checked_add(11)?) {
                Ok(Some(frame_end)) => frame_end,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            let frame_length = u64::try_from(frame_end.checked_sub(chain_start)?).ok()?;
            let record = DesignSurfaceTrimChainRecord {
                record_index: parsed.record_index,
                byte_offset: u64::try_from(chain_start).ok()?,
                class_tag: parsed.class_tag,
                frame_length,
            };
            chain_start = frame_end;
            Some(Ok(record))
        };
        let first_chain_record = match next_chain_record()? {
            Ok(record) => record,
            Err(error) => return Some(Err(error)),
        };
        let second_chain_record = match next_chain_record()? {
            Ok(record) => record,
            Err(error) => return Some(Err(error)),
        };
        let chain_records = [first_chain_record, second_chain_record];

        let cell_table_byte_offset = chain_start;
        let cell_table = indexed_record_header_at(bytes, cell_table_byte_offset)?;
        let cell_table_record_index = cell_table.record_index;
        let cell_table_class_tag = cell_table.class_tag;
        if !matches!(cell_table_class_tag.as_str(), "287" | "325") {
            return None;
        }
        let (primary, paired) = match records.frames(ctx, cell_table_record_index) {
            Ok(mut frames) => frames.find(|(primary, _)| *primary == cell_table_byte_offset)?,
            Err(error) => return Some(Err(error)),
        };
        let cell_table_paired_class_tag =
            exact_indexed_header_at(bytes, paired, cell_table_record_index)?;
        if bytes.get(cell_table_byte_offset + 11..cell_table_byte_offset + 21)? != [0; 10] {
            return None;
        }
        let cell_count_offset = cell_table_byte_offset.checked_add(21)?;
        let cell_count = View::u32_le_at(bytes, cell_count_offset)?;
        let cell_count_usize = usize::try_from(cell_count).ok()?;
        let entries_start = cell_count_offset.checked_add(4)?;
        let entries_bytes = cell_count_usize.checked_mul(19)?;
        let trailing_value_offset = entries_start.checked_add(entries_bytes)?;
        let trailing_zero_offset = trailing_value_offset.checked_add(4)?;
        let expected_paired = trailing_zero_offset.checked_add(4)?;
        if paired != expected_paired || View::u32_le_at(bytes, trailing_zero_offset) != Some(0) {
            return None;
        }
        let trailing_value = View::u32_le_at(bytes, trailing_value_offset)?;
        if trailing_value == 0 {
            return None;
        }
        let total_cells = u64::from(trailing_value);
        Some(Ok((
            selection_record_index,
            selection_byte_offset,
            selection,
            chain_records,
            cell_table_record_index,
            cell_table_class_tag,
            cell_table_paired_class_tag,
            cell_count_offset,
            cell_count_usize,
            entries_start,
            trailing_value_offset,
            trailing_zero_offset,
            trailing_value,
            total_cells,
            primary,
            paired,
        )))
    })();
    let Some(parsed_prefix) = parsed_prefix else {
        return Ok(None);
    };
    let (
        selection_record_index,
        selection_byte_offset,
        selection,
        chain_records,
        cell_table_record_index,
        cell_table_class_tag,
        cell_table_paired_class_tag,
        cell_count_offset,
        cell_count_usize,
        entries_start,
        trailing_value_offset,
        trailing_zero_offset,
        trailing_value,
        total_cells,
        primary,
        paired,
    ) = parsed_prefix?;

    let mut cell_entries = Vec::new();
    ctx.reserve_vec(
        &mut cell_entries,
        cell_count_usize,
        "f3d surface-trim cell entries",
    )?;

    let mut cell_record_indices = HashSet::new();
    ctx.reserve_set(
        &mut cell_record_indices,
        cell_count_usize,
        "f3d surface-trim cell record indices",
    )?;

    let mut cell_ordinals = HashSet::new();
    ctx.reserve_set(
        &mut cell_ordinals,
        cell_count_usize,
        "f3d surface-trim cell ordinals",
    )?;
    let parsed = (|| {
        for ordinal in 0..cell_count_usize {
            let entry_start = entries_start.checked_add(ordinal.checked_mul(19)?)?;
            let cell_record_index = marked_record_reference(bytes, entry_start)?;
            if !cell_record_indices.insert(cell_record_index) {
                return None;
            }
            if records.offsets(cell_record_index).is_empty() {
                return None;
            }
            let cell_record_reference_offset = u64::try_from(entry_start.checked_add(1)?).ok()?;
            let ordinal_offset = u64::try_from(entry_start.checked_add(11)?).ok()?;
            let ordinal_value = View::u64_le_at(bytes, entry_start.checked_add(11)?)?;
            if ordinal_value == 0
                || ordinal_value > total_cells
                || !cell_ordinals.insert(ordinal_value)
            {
                return None;
            }
            cell_entries.push(DesignSurfaceTrimCellEntry {
                record_index: cell_record_index,
                record_reference_offset: cell_record_reference_offset,
                ordinal: ordinal_value,
                ordinal_offset,
            });
        }
        DesignSurfaceTrimOperation::try_from(
            crate::records::feature::surface_ops::DesignSurfaceTrimOperationWire {
                id: String::new(),
                scope_record_index: scope.record_index,
                selection_record_index,
                selection_byte_offset: u64::try_from(selection_byte_offset).ok()?,
                selection_next_record_index: selection.next_record_index,
                selection_next_byte_offset: selection.next_byte_offset,
                chain_records,
                cell_table_record_index,
                cell_table_byte_offset: u64::try_from(primary).ok()?,
                cell_table_class_tag,
                cell_table_frame_length: u64::try_from(paired.checked_sub(primary)?).ok()?,
                cell_table_paired_class_tag: cell_table_paired_class_tag.try_into().ok()?,
                cell_table_paired_byte_offset: u64::try_from(paired).ok()?,
                cell_count_offset: u64::try_from(cell_count_offset).ok()?,
                cell_entries,
                trailing_value,
                trailing_value_offset: u64::try_from(trailing_value_offset).ok()?,
                trailing_zero_offset: u64::try_from(trailing_zero_offset).ok()?,
            },
        )
        .ok()
    })();
    Ok(parsed)
}

/// Decode every exact `SurfaceTrim` BRep-cell carrier into its own native arena.
pub(crate) fn decode_surface_trim_operations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignSurfaceTrimOperation>, CodecError> {
    let mut record_offsets = HashMap::<String, IndexedRecordOffsets>::new();
    let mut out = Vec::new();
    for scope in ctx.admit_iter(scopes, "scan F3D SurfaceTrim scopes")?.filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = cached_owned_record_offsets(ctx, &mut record_offsets, stream, bytes)?;
        let Some(mut operation) = exact_surface_trim_operation(ctx, bytes, records, scope)? else {
            continue;
        };
        operation.id = design_record_id_charged(
            ctx,
            &entry.name,
            ":design-surface-trim-operation#",
            scope.byte_offset(),
            "f3d surface-trim operation identifier",
        )?;

        ctx.reserve_vec(&mut out, 1, "f3d surface-trim operations")?;
        out.push(operation);
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design surface_trim 1",
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests;
