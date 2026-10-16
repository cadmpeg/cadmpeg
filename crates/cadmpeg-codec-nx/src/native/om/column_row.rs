// SPDX-License-Identifier: Apache-2.0
//! Native column rows retain one checked source frame with resolved targets.

use super::{column_storage_block_at, control_index_data_block};
use crate::container::Container;
use crate::om::column_row::{IndexRow, LinkedRow, TargetRow};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

mod borrowed_wires;
mod wire;

/// Self-framed index row in contiguous offset-store column storage.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "wire::DataBlockIndexRowWire")]
pub(in crate::native) struct DataBlockIndexRow {
    /// Globally unique row identity.
    pub(in crate::native) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(in crate::native) section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub(in crate::native) ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub(in crate::native) frame: IndexRow<String, u64>,
    /// Directory entry containing the offset-only store.
    pub(in crate::native) source_entry: String,
    /// Column block containing the row's opening byte.
    pub(in crate::native) opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub(in crate::native) opening_block_offset: u32,
}

/// Self-framed linked index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "wire::DataBlockLinkedIndexRowWire")]
pub(in crate::native) struct DataBlockLinkedIndexRow {
    /// Globally unique row identity.
    pub(in crate::native) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(in crate::native) section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub(in crate::native) ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub(in crate::native) frame: LinkedRow<String, u64>,
    /// Directory entry containing the store.
    pub(in crate::native) source_entry: String,
    /// Column block containing the row's opening byte.
    pub(in crate::native) opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub(in crate::native) opening_block_offset: u32,
}

/// Self-framed target-index row in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "wire::DataBlockTargetIndexRowWire")]
pub(in crate::native) struct DataBlockTargetIndexRow {
    /// Globally unique row identity.
    pub(in crate::native) id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub(in crate::native) section_ordinal: u32,
    /// Zero-based row order within the section's column storage.
    pub(in crate::native) ordinal: u32,
    /// Complete row with resolved targets and derived token positions.
    pub(in crate::native) frame: TargetRow<String, u64>,
    /// Directory entry containing the store.
    pub(in crate::native) source_entry: String,
    /// Column block containing the row's opening byte.
    pub(in crate::native) opening_data_block: String,
    /// Byte offset of the row opening within `opening_data_block`.
    pub(in crate::native) opening_block_offset: u32,
}

/// Decode complete index rows from offset-store column storage.
pub(in crate::native) fn data_block_index_rows(
    container: &Container,
) -> Result<Vec<DataBlockIndexRow>, CodecError> {
    project_column_rows(
        container,
        crate::om::column_row::scan::index_rows,
        |row, section, block_count, source_base| {
            let offset = row.offset();
            let [a, b, c, d] = row
                .indices()
                .map(|index| control_index_data_block(section, block_count, index.atom.value()));
            let mut targets = [a, b, c, d].into_iter();
            let frame = row
                .into_absolute(source_base)
                .and_then(|row| row.try_resolve(|_| targets.next().flatten()))?;
            Some((offset, frame))
        },
        |section_ordinal, section_number, ordinal, frame, source_entry, opening| {
            DataBlockIndexRow {
                id: format!("nx:om-data-block-index-rows-{section_ordinal}:row#{ordinal}"),
                section_ordinal: section_number,
                ordinal,
                frame,
                source_entry,
                opening_data_block: opening.0,
                opening_block_offset: opening.1,
            }
        },
    )
}

/// Decode complete in-range linked index rows from column storage.
pub(in crate::native) fn data_block_linked_index_rows(
    container: &Container,
) -> Result<Vec<DataBlockLinkedIndexRow>, CodecError> {
    project_column_rows(
        container,
        crate::om::column_row::scan::linked_rows,
        |row, section, block_count, source_base| {
            let offset = row.offset();
            let target =
                control_index_data_block(section, block_count, row.target_index().atom.value());
            let [a, b, c] = row
                .indices()
                .map(|index| control_index_data_block(section, block_count, index.atom.value()));
            let mut targets = [target, a, b, c].into_iter();
            let frame = row
                .into_absolute(source_base)
                .and_then(|row| row.try_resolve(|_| targets.next().flatten()))?;
            Some((offset, frame))
        },
        |section_ordinal, section_number, ordinal, frame, source_entry, opening| {
            DataBlockLinkedIndexRow {
                id: format!("nx:om-data-block-linked-index-rows-{section_ordinal}:row#{ordinal}"),
                section_ordinal: section_number,
                ordinal,
                frame,
                source_entry,
                opening_data_block: opening.0,
                opening_block_offset: opening.1,
            }
        },
    )
}

/// Decode complete in-range target-index rows from column storage.
pub(in crate::native) fn data_block_target_index_rows(
    container: &Container,
) -> Result<Vec<DataBlockTargetIndexRow>, CodecError> {
    project_column_rows(
        container,
        crate::om::column_row::scan::target_rows,
        |row, section, block_count, source_base| {
            let offset = row.offset();
            let target =
                control_index_data_block(section, block_count, row.target_index().atom.value());
            let [a, b, c] = row
                .indices()
                .map(|index| control_index_data_block(section, block_count, index.atom.value()));
            let mut targets = [target, a, b, c].into_iter();
            let frame = row
                .into_absolute(source_base)
                .and_then(|row| row.try_resolve(|_| targets.next().flatten()))?;
            Some((offset, frame))
        },
        |section_ordinal, section_number, ordinal, frame, source_entry, opening| {
            DataBlockTargetIndexRow {
                id: format!("nx:om-data-block-target-index-rows-{section_ordinal}:row#{ordinal}"),
                section_ordinal: section_number,
                ordinal,
                frame,
                source_entry,
                opening_data_block: opening.0,
                opening_block_offset: opening.1,
            }
        },
    )
}

/// One owner for section framing, source locations and admitted row ordinals.
fn project_column_rows<R, F, T>(
    container: &Container,
    scan: impl Fn(&[u8]) -> Vec<R>,
    resolve: impl Fn(R, usize, usize, u64) -> Option<(usize, F)>,
    project: impl Fn(usize, u32, u32, F, String, (String, u32)) -> T,
) -> Result<Vec<T>, CodecError> {
    let mut result = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        let Some((_, storage, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(storage_offset) = records.first().map(|record| record.offset) else {
            continue;
        };
        let source_base = entry
            .file_span()
            .map_or(0, |(offset, _)| offset)
            .checked_add(u64_from_index(storage_offset))
            .ok_or_else(|| CodecError::malformed("NX column row source base overflows"))?;
        let block_count = records.len() + 1;
        let rows = scan(storage);
        let mut ordinal = 0usize;
        for row in rows {
            let Some((offset, frame)) = resolve(row, section_ordinal, block_count, source_base)
            else {
                continue;
            };
            let Some(opening_offset) = storage_offset.checked_add(offset) else {
                continue;
            };

            let Some((block_ordinal, block_offset)) =
                column_storage_block_at(records, opening_offset)
            else {
                continue;
            };

            let section_number = u32::try_from(section_ordinal).map_err(|_| {
                CodecError::malformed("NX column row section ordinal overflows u32")
            })?;
            let row_number = u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("NX column row ordinal overflows u32"))?;
            let opening = (
                format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                block_offset,
            );
            let source_entry = entry.name.clone();
            result.push(project(
                section_ordinal,
                section_number,
                row_number,
                frame,
                source_entry,
                opening,
            ));
            ordinal += 1;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        data_block_index_rows, data_block_linked_index_rows, data_block_target_index_rows,
    };
    use crate::container::{self, Container};
    use crate::test_support::test_om::offset_only_indexed_om_section;
    use crate::test_support::test_prt::prt_with_named_payloads;

    use cadmpeg_core::CodecError;

    const INDEX_ROW: &[u8] =
        b"\x2d\x02\x0b\x2a\x93\x8a\x03\x01\x01\x01\x01\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    const LINKED_ROW: &[u8] = b"\x02\x0b\x83\x93\x93\x8c\x16\x01\xff\xff\x90\xfe\x01\x01\x01\x00\x47\x03\x04\x01\xc0\x44\x04\x00";
    const TARGET_ROW: &[u8] =
        b"\x02\x01\x01\x01\x16\x01\xff\xff\x90\xfe\x01\x01\x01\x00\x47\x03\x07\x01\xc0\x44\x04\x00";

    fn column_container(row: &[u8]) -> Container<'static> {
        let mut section = offset_only_indexed_om_section();
        section.extend_from_slice(row);
        let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
        let end_at = index_start + 3 * 4;
        let section_len = u32::try_from(section.len()).expect("test section length");
        section[end_at..end_at + 4].copy_from_slice(&section_len.to_le_bytes());
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", section)]);
        container::scan_bytes(file).expect("column row container")
    }

    type Route = fn(&Container<'_>) -> Result<usize, CodecError>;

    fn index_count(container: &Container<'_>) -> Result<usize, CodecError> {
        Ok(data_block_index_rows(container)?.len())
    }

    fn linked_count(container: &Container<'_>) -> Result<usize, CodecError> {
        Ok(data_block_linked_index_rows(container)?.len())
    }

    fn target_count(container: &Container<'_>) -> Result<usize, CodecError> {
        Ok(data_block_target_index_rows(container)?.len())
    }

    #[test]
    fn native_column_row_routes_keep_resolved_frames() {
        let routes: [(_, Route); 3] = [
            (INDEX_ROW, index_count),
            (LINKED_ROW, linked_count),
            (TARGET_ROW, target_count),
        ];
        for (row, route) in routes {
            let container = column_container(row);
            assert_eq!(route(&container).expect("resolved row"), 1);
        }
    }
}
