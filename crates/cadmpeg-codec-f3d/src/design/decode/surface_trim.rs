// SPDX-License-Identifier: Apache-2.0
//! Decode the auxiliary BRep-cell carrier of a `SurfaceTrim` operation.

use crate::design::decode::meta::cached_owned_primary_frames;
use crate::metastream::PrimaryFrames;
use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::operands::parse_entity_selection_frame;
use crate::design::decode::scopes::shared_frames::exact_indexed_header_at;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::indexed_record_header_at;
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::records::feature::{
    scope::DesignParameterScope,
    surface_ops::{
        DesignSurfaceTrimCellEntry, DesignSurfaceTrimChainRecord, DesignSurfaceTrimOperation,
    },
};
use cadmpeg_core::decode::View;
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
    bytes: &[u8],
    records: &[crate::metastream::PrimaryRecordFrame],
    scope: &DesignParameterScope,
) -> Option<DesignSurfaceTrimOperation> {
    let parsed_prefix = (|| {
        if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
            || scope.reference_members().len() != 4
        {
            return None;
        }
        let selection_record_index = *scope.reference_members().values().nth(3)?;
        let (selection_byte_offset, _) = records.frames(selection_record_index).next()?;
        let selection_class_tag =
            exact_indexed_header_at(bytes, selection_byte_offset, selection_record_index)?;
        let selection = parse_entity_selection_frame(
            bytes,
            records,
            selection_record_index,
            u64::try_from(selection_byte_offset).ok()?,
            &selection_class_tag,
        )?;

        let mut chain_start = usize::try_from(selection.next_byte_offset).ok()?;
        let mut next_chain_record = || {
            let parsed = indexed_record_header_at(bytes, chain_start)?;
            if !records.offsets(parsed.record_index).contains(&chain_start) {
                return None;
            }
            let frame_end = records.next_at_or_after(chain_start.checked_add(11)?)?;
            let frame_length = u64::try_from(frame_end.checked_sub(chain_start)?).ok()?;
            let record = DesignSurfaceTrimChainRecord {
                record_index: parsed.record_index,
                byte_offset: u64::try_from(chain_start).ok()?,
                class_tag: parsed.class_tag,
                frame_length,
            };
            chain_start = frame_end;
            Some(record)
        };
        let chain_records = [next_chain_record()?, next_chain_record()?];

        let cell_table_byte_offset = chain_start;
        let cell_table = indexed_record_header_at(bytes, cell_table_byte_offset)?;
        let cell_table_record_index = cell_table.record_index;
        let cell_table_class_tag = cell_table.class_tag;
        if !matches!(cell_table_class_tag.as_str(), "287" | "325") {
            return None;
        }
        let (primary, paired) = records
            .frames(cell_table_record_index)
            .find(|(primary, _)| *primary == cell_table_byte_offset)?;
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
        Some((
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
        ))
    })();
    let parsed_prefix = parsed_prefix?;
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
    ) = parsed_prefix;

    let mut cell_entries = Vec::new();

    let mut cell_record_indices = HashSet::new();

    let mut cell_ordinals = HashSet::new();
    (|| {
        for ordinal in 0..cell_count_usize {
            let entry_start = entries_start.checked_add(ordinal.checked_mul(19)?)?;
            let cell_record_index = marked_record_reference(bytes, entry_start)?;
            if !cell_record_indices.insert(cell_record_index) {
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
    })()
}

fn surface_trim_cell_frame(
    bytes: &[u8],
    records: &[crate::metastream::PrimaryRecordFrame],
    scope: &DesignParameterScope,
) -> Option<crate::metastream::PrimaryRecordFrame> {
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
        || scope.reference_members().len() != 4
    {
        return None;
    }
    let index = *scope.reference_members().values().nth(3)?;
    let (start, _) = records.frames(index).next()?;
    let class = exact_indexed_header_at(bytes, start, index)?;
    let selection =
        parse_entity_selection_frame(bytes, records, index, u64::try_from(start).ok()?, &class)?;
    let mut at = usize::try_from(selection.next_byte_offset).ok()?;
    for _ in 0..2 {
        let header = indexed_record_header_at(bytes, at)?;
        if !records.offsets(header.record_index).contains(&at) {
            return None;
        }
        at = records.next_at_or_after(at.checked_add(11)?)?;
    }
    let header = indexed_record_header_at(bytes, at)?;
    if !matches!(header.class_tag.as_str(), "287" | "325") {
        return None;
    }
    records
        .iter()
        .find(|frame| frame.start == at && frame.entity_id == u64::from(header.record_index))
        .copied()
}

/// Decode every exact `SurfaceTrim` BRep-cell carrier into its own native arena.
pub(crate) fn decode_surface_trim_operations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parts: &mut cadmpeg_ir::source_fidelity::parts::PartOutput,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignSurfaceTrimOperation>, CodecError> {
    use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartSpec};
    let mut record_offsets = HashMap::<String, Vec<crate::metastream::PrimaryRecordFrame>>::new();
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let source = scan
            .entry_view(&entry.name)
            .ok_or_else(|| CodecError::malformed("SurfaceTrim source View is missing"))?;
        let bytes = source.window();
        let Some(records) =
            cached_owned_primary_frames(&mut record_offsets, stream, bytes, scan, &entry.name)?
        else {
            continue;
        };
        let Some(frame) = surface_trim_cell_frame(bytes, records, scope) else {
            continue;
        };
        let extent = source
            .child(
                source
                    .start()
                    .checked_add(frame.start)
                    .ok_or_else(|| CodecError::malformed("SurfaceTrim table start overflows"))?,
                source
                    .start()
                    .checked_add(frame.end)
                    .ok_or_else(|| CodecError::malformed("SurfaceTrim table end overflows"))?,
            )
            .ok_or_else(|| CodecError::malformed("SurfaceTrim table exceeds its stream"))?;
        let record_id = cadmpeg_ir::ids::Identity::new(design_record_id(
            &entry.name,
            ":design-record-header#",
            cadmpeg_core::decode::u64_from_index(frame.start),
        ))
        .map_err(CodecError::malformed)?;
        let mut spec = PartSpec::new("f3d.bulk_record")
            .holding([cadmpeg_ir::document::ArenaClass::Design])
            .record_id(record_id);
        if extent.location().space != cadmpeg_core::decode::SpaceId::ROOT {
            spec = spec.in_stream(
                cadmpeg_ir::StreamName::try_from(entry.name.clone())
                    .map_err(CodecError::malformed)?,
            );
        }
        let stub_id = spec.unreadable_id(cadmpeg_ir::CodecFormat::F3d, extent)?;
        if parts.fidelity().retained_record(stub_id.as_str()).is_some() {
            continue;
        }
        let Some(Some(mut operation)) = ctx.decode_part(parts, extent, spec, |_| {
            let operation =
                exact_surface_trim_operation(bytes, records, scope).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "SurfaceTrim cell table entity {} has an invalid frame",
                        frame.entity_id
                    ))
                })?;
            Ok(operation
                .cell_entries()
                .iter()
                .all(|cell| !records.offsets(cell.record_index).is_empty())
                .then_some(operation))
        })?
        else {
            continue;
        };
        operation.id = design_record_id(
            &entry.name,
            ":design-surface-trim-operation#",
            scope
                .byte_offset()
                .ok_or_else(crate::records::feature::scope::unlocated_scope)?,
        );

        out.push(operation);
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

#[cfg(test)]
mod tests;
