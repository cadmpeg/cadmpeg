// SPDX-License-Identifier: Apache-2.0
//! Decode the auxiliary BRep-cell carrier of a `SurfaceTrim` operation.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::operands::parse_entity_selection_frame;
use crate::design::decode::scopes::shared_frames::exact_indexed_header_at;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::{
    cached_owned_record_offsets, indexed_record_header_at, next_indexed_record_offset,
    IndexedRecordHeader, IndexedRecordOffsets,
};
use crate::design::decode::text::design_record_id_charged;
use crate::ids::native_stream;
use crate::records::feature::{
    scope::DesignParameterScope,
    surface_ops::{
        DesignSurfaceTrimCellEntry, DesignSurfaceTrimChainRecord, DesignSurfaceTrimOperation,
    },
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
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
    if !matches!(
        scope.payload(),
        crate::records::feature::scope::DesignScopePayload::SurfaceTrim
    ) {
        return Ok(None);
    }
    let Some([_, _, _, &selection_record_index]) = scope.reference_members().values_array::<4>()
    else {
        return Ok(None);
    };
    let Some((selection_byte_offset, _)) = records.first_frame(selection_record_index) else {
        return Ok(None);
    };
    let Some(selection_class_tag) =
        exact_indexed_header_at(bytes, selection_byte_offset, selection_record_index)
    else {
        return Ok(None);
    };
    let Some(selection_byte_offset_u64) = u64::try_from(selection_byte_offset).ok() else {
        return Ok(None);
    };
    let Ok(selection_class_tag) = std::str::from_utf8(selection_class_tag) else {
        return Ok(None);
    };
    let Some(selection) = parse_entity_selection_frame(
        ctx,
        bytes,
        selection_record_index,
        selection_byte_offset_u64,
        selection_class_tag,
    )?
    else {
        return Ok(None);
    };

    let Some(mut chain_start) = usize::try_from(selection.next_byte_offset).ok() else {
        return Ok(None);
    };
    let mut chain_headers = [None; 2];
    for slot in &mut chain_headers {
        let Some(header) = indexed_record_header_at(bytes, chain_start) else {
            return Ok(None);
        };
        let Some(frame_end) = next_indexed_record_offset(ctx, bytes, chain_start + 11)? else {
            return Ok(None);
        };
        *slot = Some((header, frame_end));
        chain_start = frame_end;
    }
    let [Some(first_chain), Some(second_chain)] = chain_headers else {
        return Ok(None);
    };

    let cell_table_byte_offset = chain_start;
    let Some(cell_table) = indexed_record_header_at(bytes, cell_table_byte_offset) else {
        return Ok(None);
    };
    if !matches!(cell_table.class_tag, b"287" | b"325") {
        return Ok(None);
    }
    let cell_table_record_index = cell_table.record_index;
    let Some(paired) = records.frame_at(ctx, cell_table_record_index, cell_table_byte_offset)?
    else {
        return Ok(None);
    };
    let Some(cell_table_paired_class_tag) =
        exact_indexed_header_at(bytes, paired, cell_table_record_index)
    else {
        return Ok(None);
    };
    let Some(prefix) = surface_trim_cell_table_prefix(bytes, cell_table_byte_offset, paired) else {
        return Ok(None);
    };

    let mut cell_entries =
        ctx.vector_storage(prefix.cell_count, "f3d surface-trim cell entries")?;
    let mut cell_record_indices = HashSet::new();
    let mut cell_ordinals = HashSet::new();
    for ordinal in ctx.admit_iter(
        &(0..prefix.cell_count),
        "scan F3D surface-trim cell entries",
    )? {
        // `cell_count * 19` fits: the prefix bounded the entry table by `paired`.
        let entry_start = prefix.entries_start + ordinal * 19;
        let Some(cell_record_index) = marked_record_reference(bytes, entry_start) else {
            return Ok(None);
        };
        let Some(ordinal_value) = View::u64_le_at(bytes, entry_start + 11) else {
            return Ok(None);
        };
        if ordinal_value == 0
            || ordinal_value > u64::from(prefix.trailing_value)
            || records.offsets(cell_record_index).is_empty()
            || !ctx.insert_hash_set(
                &mut cell_record_indices,
                cell_record_index,
                "f3d surface-trim cell record indices",
            )?
            || !ctx.insert_hash_set(
                &mut cell_ordinals,
                ordinal_value,
                "f3d surface-trim cell ordinals",
            )?
        {
            return Ok(None);
        }
        ctx.push_vec(
            &mut cell_entries,
            DesignSurfaceTrimCellEntry {
                record_index: cell_record_index,
                record_reference_offset: u64_from_index(entry_start + 1),
                ordinal: ordinal_value,
                ordinal_offset: u64_from_index(entry_start + 11),
            },
            "f3d surface-trim cell entries",
        )?;
    }
    let chain_records = [
        surface_trim_chain_record(ctx, first_chain)?,
        surface_trim_chain_record(ctx, second_chain)?,
    ];
    let operation = DesignSurfaceTrimOperation::try_from(
        crate::records::feature::surface_ops::DesignSurfaceTrimOperationWire {
            id: String::new(),
            scope_record_index: scope.record_index,
            selection_record_index,
            selection_byte_offset: selection_byte_offset_u64,
            selection_next_record_index: selection.next_record_index,
            selection_next_byte_offset: selection.next_byte_offset,
            chain_records,
            cell_table_record_index,
            cell_table_byte_offset: u64_from_index(cell_table_byte_offset),
            cell_table_class_tag: cell_table
                .retain_class_tag(ctx, "copy F3D surface-trim cell table class tag")?,
            cell_table_frame_length: u64_from_index(paired - cell_table_byte_offset),
            cell_table_paired_class_tag: crate::design::decode::text::retain_class_tag(
                ctx,
                cell_table_paired_class_tag,
                "copy F3D surface-trim paired class tag",
            )?,
            cell_table_paired_byte_offset: u64_from_index(paired),
            cell_count_offset: u64_from_index(prefix.cell_count_offset),
            cell_entries,
            trailing_value: prefix.trailing_value,
            trailing_value_offset: u64_from_index(prefix.trailing_value_offset),
            trailing_zero_offset: u64_from_index(prefix.trailing_zero_offset),
        },
    );
    Ok(operation.ok())
}

/// Fixed fields of a `SurfaceTrim` cell table that opens at `start` and whose
/// paired header is at `paired`.
struct SurfaceTrimCellTablePrefix {
    cell_count: usize,
    cell_count_offset: usize,
    entries_start: usize,
    trailing_value: u32,
    trailing_value_offset: usize,
    trailing_zero_offset: usize,
}

fn surface_trim_cell_table_prefix(
    bytes: &[u8],
    start: usize,
    paired: usize,
) -> Option<SurfaceTrimCellTablePrefix> {
    if bytes.get(start.checked_add(11)?..)?.first_chunk::<10>()? != &[0; 10] {
        return None;
    }
    let cell_count_offset = start.checked_add(21)?;
    let cell_count = usize::try_from(View::u32_le_at(bytes, cell_count_offset)?).ok()?;
    let entries_start = cell_count_offset.checked_add(4)?;
    let trailing_value_offset = entries_start.checked_add(cell_count.checked_mul(19)?)?;
    let trailing_zero_offset = trailing_value_offset.checked_add(4)?;
    if paired != trailing_zero_offset.checked_add(4)?
        || View::u32_le_at(bytes, trailing_zero_offset) != Some(0)
    {
        return None;
    }
    let trailing_value = View::u32_le_at(bytes, trailing_value_offset)?;
    (trailing_value != 0).then_some(SurfaceTrimCellTablePrefix {
        cell_count,
        cell_count_offset,
        entries_start,
        trailing_value,
        trailing_value_offset,
        trailing_zero_offset,
    })
}

fn surface_trim_chain_record(
    ctx: &DecodeContext<'_>,
    (header, frame_end): (IndexedRecordHeader<'_>, usize),
) -> Result<DesignSurfaceTrimChainRecord, CodecError> {
    Ok(DesignSurfaceTrimChainRecord {
        record_index: header.record_index,
        byte_offset: u64_from_index(header.offset),
        class_tag: header.retain_class_tag(ctx, "copy F3D surface-trim chain class tag")?,
        // The frame ends at the next header, after this one.
        frame_length: u64_from_index(frame_end - header.offset),
    })
}

/// Decode every exact `SurfaceTrim` BRep-cell carrier into its own native arena.
pub(crate) fn decode_surface_trim_operations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignSurfaceTrimOperation>, CodecError> {
    let mut record_offsets = HashMap::<String, IndexedRecordOffsets>::new();
    let mut out = Vec::new();
    for scope in ctx
        .admit_iter(scopes, "scan F3D SurfaceTrim scopes")?
        .filter(|scope| {
            matches!(
                scope.payload(),
                crate::records::feature::scope::DesignScopePayload::SurfaceTrim
            )
        })
    {
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
