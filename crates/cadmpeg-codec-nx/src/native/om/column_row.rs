// SPDX-License-Identifier: Apache-2.0
//! Native column rows retain one checked source frame with resolved targets.

use super::{column_storage_block_at, control_index_data_block, retained_om_index_id};
use crate::container::Container;
use crate::om::column_row::{IndexRow, LinkedRow, TargetRow};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
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
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockIndexRow>, CodecError> {
    project_column_rows(
        ctx,
        container,
        |storage| crate::om::column_row::scan::index_rows(ctx, storage),
        |row, section, block_count, source_base| {
            let offset = row.offset();
            let [a, b, c, d] = row.indices().map(|index| {
                control_index_data_block(ctx, section, block_count, index.atom.value())
            });
            let mut targets = [a?, b?, c?, d?].into_iter();
            let Some(frame) = row
                .into_absolute(source_base)
                .and_then(|row| row.try_resolve(|_| targets.next().flatten()))
            else {
                return Ok(None);
            };
            Ok(Some((offset, frame)))
        },
        |section_ordinal, section_number, ordinal, frame, source_entry, opening| {
            Ok(DataBlockIndexRow {
                id: retained_om_index_id(
                    ctx,
                    "nx:om-data-block-index-rows-",
                    section_ordinal,
                    ":row#",
                    u64::from(ordinal),
                    "NX index row id",
                )?,
                section_ordinal: section_number,
                ordinal,
                frame,
                source_entry,
                opening_data_block: opening.0,
                opening_block_offset: opening.1,
            })
        },
    )
}

/// Decode complete in-range linked index rows from column storage.
pub(in crate::native) fn data_block_linked_index_rows(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockLinkedIndexRow>, CodecError> {
    project_column_rows(
        ctx,
        container,
        |storage| crate::om::column_row::scan::linked_rows(ctx, storage),
        |row, section, block_count, source_base| {
            let offset = row.offset();
            let target = control_index_data_block(
                ctx,
                section,
                block_count,
                row.target_index().atom.value(),
            )?;
            let [a, b, c] = row.indices().map(|index| {
                control_index_data_block(ctx, section, block_count, index.atom.value())
            });
            let mut targets = [target, a?, b?, c?].into_iter();
            let Some(frame) = row
                .into_absolute(source_base)
                .and_then(|row| row.try_resolve(|_| targets.next().flatten()))
            else {
                return Ok(None);
            };
            Ok(Some((offset, frame)))
        },
        |section_ordinal, section_number, ordinal, frame, source_entry, opening| {
            Ok(DataBlockLinkedIndexRow {
                id: retained_om_index_id(
                    ctx,
                    "nx:om-data-block-linked-index-rows-",
                    section_ordinal,
                    ":row#",
                    u64::from(ordinal),
                    "NX linked index row id",
                )?,
                section_ordinal: section_number,
                ordinal,
                frame,
                source_entry,
                opening_data_block: opening.0,
                opening_block_offset: opening.1,
            })
        },
    )
}

/// Decode complete in-range target-index rows from column storage.
pub(in crate::native) fn data_block_target_index_rows(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockTargetIndexRow>, CodecError> {
    project_column_rows(
        ctx,
        container,
        |storage| crate::om::column_row::scan::target_rows(ctx, storage),
        |row, section, block_count, source_base| {
            let offset = row.offset();
            let target = control_index_data_block(
                ctx,
                section,
                block_count,
                row.target_index().atom.value(),
            )?;
            let [a, b, c] = row.indices().map(|index| {
                control_index_data_block(ctx, section, block_count, index.atom.value())
            });
            let mut targets = [target, a?, b?, c?].into_iter();
            let Some(frame) = row
                .into_absolute(source_base)
                .and_then(|row| row.try_resolve(|_| targets.next().flatten()))
            else {
                return Ok(None);
            };
            Ok(Some((offset, frame)))
        },
        |section_ordinal, section_number, ordinal, frame, source_entry, opening| {
            Ok(DataBlockTargetIndexRow {
                id: retained_om_index_id(
                    ctx,
                    "nx:om-data-block-target-index-rows-",
                    section_ordinal,
                    ":row#",
                    u64::from(ordinal),
                    "NX target index row id",
                )?,
                section_ordinal: section_number,
                ordinal,
                frame,
                source_entry,
                opening_data_block: opening.0,
                opening_block_offset: opening.1,
            })
        },
    )
}

/// One owner for section framing, source locations and admitted row ordinals.
fn project_column_rows<R, F, T>(
    ctx: &DecodeContext<'_>,
    container: &Container,
    scan: impl Fn(&[u8]) -> Result<Vec<R>, CodecError>,
    resolve: impl Fn(R, usize, usize, u64) -> Result<Option<(usize, F)>, CodecError>,
    project: impl Fn(usize, u32, u32, F, String, (String, u32)) -> Result<T, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let mut result = Vec::new();
    let sections = container.indexed_om_sections(ctx)?;
    for (section_ordinal, (entry, section)) in ctx
        .admit_iter(&sections, "NX column row input sections")?
        .enumerate()
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
            .ok_or_else(|| ctx.refuse_codec_limit("NX column row source base", 0, 1))?;
        let block_count = records
            .len()
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX column row block count", 0, 1))?;
        let rows = scan(storage)?;
        let mut ordinal = 0usize;
        let rows_count = rows.len();
        let mut rows = rows.into_iter();
        for _index in ctx.admit_iter(&(0..rows_count), "NX column row rows visits")? {
            let Some(row) = rows.next() else {
                break;
            };
            let Some((offset, frame)) = resolve(row, section_ordinal, block_count, source_base)?
            else {
                continue;
            };
            let Some(opening_offset) = storage_offset.checked_add(offset) else {
                continue;
            };
            let Some((block_ordinal, block_offset)) =
                column_storage_block_at(ctx, records, opening_offset)?
            else {
                continue;
            };
            ctx.reserve_vec(&mut result, 1, "NX native column rows")?;
            let section_number = u32::try_from(section_ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX column row section ordinal", 0, 1))?;
            let row_number = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX column row ordinal", 0, 1))?;
            let opening = (
                retained_om_index_id(
                    ctx,
                    "nx:om-data-blocks-",
                    section_ordinal,
                    ":block#",
                    u64_from_index(block_ordinal),
                    "NX column row opening block",
                )?,
                block_offset,
            );
            let source_entry = ctx.copy_retained_text(&entry.name, "NX column row source entry")?;
            result.push(project(
                section_ordinal,
                section_number,
                row_number,
                frame,
                source_entry,
                opening,
            )?);
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit("NX column row ordinal", 0, 1))?;
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
    use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
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
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
            .expect("column row container")
    }

    type Route = fn(&DecodeContext<'_>, &Container<'_>) -> Result<usize, CodecError>;

    fn index_count(
        ctx: &DecodeContext<'_>,
        container: &Container<'_>,
    ) -> Result<usize, CodecError> {
        Ok(data_block_index_rows(ctx, container)?.len())
    }

    fn linked_count(
        ctx: &DecodeContext<'_>,
        container: &Container<'_>,
    ) -> Result<usize, CodecError> {
        Ok(data_block_linked_index_rows(ctx, container)?.len())
    }

    fn target_count(
        ctx: &DecodeContext<'_>,
        container: &Container<'_>,
    ) -> Result<usize, CodecError> {
        Ok(data_block_target_index_rows(ctx, container)?.len())
    }

    fn route_refusal(row: &[u8], route: Route, dimension: ResourceDimension) -> CodecError {
        let container = column_container(row);

        let adjust: fn(&mut cadmpeg_core::decode::DecodePolicy) = match dimension {
            ResourceDimension::CollectionItems => |policy| {
                policy.limits.max_collection_items = 0;
            },
            ResourceDimension::RetainedBytes => |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            ResourceDimension::WorkUnits => |policy| {
                policy.limits.max_work_units = 0;
            },
            _ => panic!("unsupported test dimension"),
        };
        crate::test_support::with_decode_context_over(&[], adjust, |ctx| {
            route(ctx, &container).expect_err("column row resource refusal")
        })
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
            assert_eq!(
                crate::test_support::with_decode_context(|ctx| route(ctx, &container))
                    .expect("resolved row"),
                1
            );
        }
    }

    macro_rules! route_limit_test {
        ($name:ident, $row:ident, $route:ident, $dimension:ident) => {
            #[test]
            fn $name() {
                let error = route_refusal($row, $route, ResourceDimension::$dimension);
                assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::$dimension), "{error:?}");
            }
        };
    }

    route_limit_test!(
        native_index_row_refuses_collection_limit,
        INDEX_ROW,
        index_count,
        CollectionItems
    );
    route_limit_test!(
        native_index_row_refuses_retained_limit,
        INDEX_ROW,
        index_count,
        RetainedBytes
    );
    route_limit_test!(
        native_index_row_refuses_work_limit,
        INDEX_ROW,
        index_count,
        WorkUnits
    );
    route_limit_test!(
        native_linked_row_refuses_collection_limit,
        LINKED_ROW,
        linked_count,
        CollectionItems
    );
    route_limit_test!(
        native_linked_row_refuses_retained_limit,
        LINKED_ROW,
        linked_count,
        RetainedBytes
    );
    route_limit_test!(
        native_linked_row_refuses_work_limit,
        LINKED_ROW,
        linked_count,
        WorkUnits
    );
    route_limit_test!(
        native_target_row_refuses_collection_limit,
        TARGET_ROW,
        target_count,
        CollectionItems
    );
    route_limit_test!(
        native_target_row_refuses_retained_limit,
        TARGET_ROW,
        target_count,
        RetainedBytes
    );
    route_limit_test!(
        native_target_row_refuses_work_limit,
        TARGET_ROW,
        target_count,
        WorkUnits
    );
}
