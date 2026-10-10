// SPDX-License-Identifier: Apache-2.0
//! Resolved counted and ABR compact-index lanes.

use super::{control_index_data_block, retained_om_index_id};
use crate::container::Container;
use crate::om::compact_lane::scan::{abr_lanes, counted_lanes};
use crate::om::compact_lane::{AbrLane, CountedLane};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::Deserialize;
use wire::{DataBlockAbrReferenceLaneWire, DataBlockCountedIndexLaneWire};

mod borrowed_wires;
mod wire;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DataBlockCountedIndexLaneWire")]
pub(in crate::native) struct DataBlockCountedIndexLane {
    id: String,
    data_block: String,
    ordinal: u32,
    frame: CountedLane<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DataBlockAbrReferenceLaneWire")]
pub(in crate::native) struct DataBlockAbrReferenceLane {
    pub(in crate::native) id: String,
    section_ordinal: u32,
    ordinal: u32,
    pub(in crate::native) frame: AbrLane<String, u64>,
    source_entry: String,
}

fn counted_lane_id(
    ctx: &DecodeContext<'_>,
    section_ordinal: usize,
    block_ordinal: usize,
    ordinal: usize,
) -> Result<String, CodecError> {
    use std::fmt::Write;

    fn digits(mut value: usize) -> usize {
        let mut digits = 1;
        while value >= 10 {
            value /= 10;
            digits += 1;
        }
        digits
    }
    let operation = "NX counted lane id";
    let length = "nx:om-data-block-counted-index-lanes-"
        .len()
        .checked_add(digits(section_ordinal))
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(digits(block_ordinal)))
        .and_then(|length| length.checked_add(":lane#".len()))
        .and_then(|length| length.checked_add(digits(ordinal)))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, 1))?;
    ctx.charge_work(u64_from_index(length), operation)?;
    let mut id = ctx.retained_string(length, operation)?;
    write!(
        &mut id,
        "nx:om-data-block-counted-index-lanes-{section_ordinal}-{block_ordinal}:lane#{ordinal}"
    )
    .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(id)
}

/// Decode complete in-range counted block-index lanes from offset-only stores.
pub(in crate::native) fn data_block_counted_index_lanes(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockCountedIndexLane>, CodecError> {
    let mut output = Vec::new();
    let sections = container.indexed_om_sections(ctx)?;
    for (section_ordinal, (entry, section)) in ctx
        .admit_iter(&sections, "NX compact lane input sections")?
        .enumerate()
    {
        let Some((_, _, records)) = section.as_offset_only() else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let block_count = records
            .len()
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX counted lane block count", 0, 1))?;
        for (record_ordinal, block) in records.iter().enumerate() {
            let block_ordinal = record_ordinal
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit("NX counted lane block ordinal", 0, 1))?;
            let Some(source_base) = entry_offset.checked_add(u64_from_index(block.offset)) else {
                continue;
            };
            let mut ordinal = 0usize;
            let lanes = counted_lanes(ctx, block.bytes)?;
            for lane in ctx.admit_iter(lanes, "NX compact lane lanes visits")? {
                let Some(lane) = lane.into_absolute(source_base) else {
                    continue;
                };
                let Some(frame) = lane.try_resolve_charged(ctx, |atom| {
                    control_index_data_block(ctx, section_ordinal, block_count, atom.value())
                })?
                else {
                    continue;
                };
                let row_ordinal = u32::try_from(ordinal)
                    .map_err(|_| ctx.refuse_codec_limit("NX counted lane ordinal", 0, 1))?;
                ctx.reserve_vec(&mut output, 1, "NX native counted index lanes")?;
                output.push(DataBlockCountedIndexLane {
                    id: counted_lane_id(ctx, section_ordinal, block_ordinal, ordinal)?,
                    data_block: retained_om_index_id(
                        ctx,
                        "nx:om-data-blocks-",
                        section_ordinal,
                        ":block#",
                        u64_from_index(block_ordinal),
                        "NX counted lane data block",
                    )?,
                    ordinal: row_ordinal,
                    frame,
                });
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit("NX counted lane ordinal", 0, 1))?;
            }
        }
    }
    Ok(output)
}

/// Decode complete in-range `ABR` reference lanes from offset-store column storage.
pub(in crate::native) fn data_block_abr_reference_lanes(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockAbrReferenceLane>, CodecError> {
    let mut output = Vec::new();
    let sections = container.indexed_om_sections(ctx)?;
    for (section_ordinal, (entry, section)) in ctx
        .admit_iter(&sections, "NX compact lane input sections")?
        .enumerate()
    {
        let Some((_, storage, records)) = section.as_offset_only() else {
            continue;
        };
        let Some(storage_offset) = records.first().map(|record| record.offset) else {
            continue;
        };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let Some(source_base) = entry_offset.checked_add(u64_from_index(storage_offset)) else {
            continue;
        };
        let block_count = records
            .len()
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("NX ABR lane block count", 0, 1))?;
        let mut ordinal = 0usize;
        let lanes = abr_lanes(ctx, storage)?;
        for lane in ctx.admit_iter(lanes, "NX compact lane lanes visits")? {
            let Some(lane) = lane.into_absolute(source_base) else {
                continue;
            };
            let Some(frame) = lane.try_resolve(|atom| {
                control_index_data_block(ctx, section_ordinal, block_count, atom.value())
            })?
            else {
                continue;
            };
            let section_number = u32::try_from(section_ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX ABR lane section ordinal", 0, 1))?;
            let row_ordinal = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX ABR lane ordinal", 0, 1))?;
            ctx.reserve_vec(&mut output, 1, "NX native ABR reference lanes")?;
            output.push(DataBlockAbrReferenceLane {
                id: retained_om_index_id(
                    ctx,
                    "nx:om-data-block-abr-reference-lanes-",
                    section_ordinal,
                    ":lane#",
                    u64_from_index(ordinal),
                    "NX ABR lane id",
                )?,
                section_ordinal: section_number,
                ordinal: row_ordinal,
                frame,
                source_entry: ctx.copy_retained_text(&entry.name, "NX ABR lane source entry")?,
            });
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit("NX ABR lane ordinal", 0, 1))?;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use crate::container;
    use crate::test_support::test_om::offset_only_indexed_om_section;
    use crate::test_support::test_prt::prt_with_named_payloads;
    use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
    use cadmpeg_core::CodecError;

    const COUNTED: &[u8] = &[0x01, 0x03, 0x01, 0x01, 0x01, 0x11];
    const ABR: &[u8] = &[
        0x11, 0x02, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03,
    ];

    fn lane_container(lane: &[u8]) -> container::Container<'static> {
        let mut store = offset_only_indexed_om_section();
        store.extend_from_slice(lane);
        let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
        let end_at = index_start + 3 * 4;
        let end = u32::try_from(store.len()).expect("test store length");
        store[end_at..end_at + 4].copy_from_slice(&end.to_le_bytes());
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", store)]);
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
            .expect("lane container")
    }

    type Route = fn(&DecodeContext<'_>, &container::Container<'_>) -> Result<usize, CodecError>;

    fn counted_count(
        ctx: &DecodeContext<'_>,
        container: &container::Container<'_>,
    ) -> Result<usize, CodecError> {
        Ok(super::data_block_counted_index_lanes(ctx, container)?.len())
    }

    fn abr_count(
        ctx: &DecodeContext<'_>,
        container: &container::Container<'_>,
    ) -> Result<usize, CodecError> {
        Ok(super::data_block_abr_reference_lanes(ctx, container)?.len())
    }

    fn lane_refusal(lane: &[u8], route: Route, dimension: ResourceDimension) -> CodecError {
        let container = lane_container(lane);

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
            route(ctx, &container).expect_err("lane resource refusal")
        })
    }

    #[test]
    fn native_compact_lane_routes_keep_resolved_frames() {
        let routes: [(_, Route); 2] = [(COUNTED, counted_count), (ABR, abr_count)];
        for (lane, route) in routes {
            let container = lane_container(lane);
            assert_eq!(
                crate::test_support::with_decode_context(|ctx| route(ctx, &container))
                    .expect("resolved lane"),
                1
            );
        }
    }

    macro_rules! lane_limit_test {
        ($name:ident, $lane:ident, $route:ident, $dimension:ident) => {
            #[test]
            fn $name() {
                let error = lane_refusal($lane, $route, ResourceDimension::$dimension);
                assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::$dimension), "{error:?}");
            }
        };
    }

    lane_limit_test!(
        native_counted_lane_refuses_collection_limit,
        COUNTED,
        counted_count,
        CollectionItems
    );
    lane_limit_test!(
        native_counted_lane_refuses_retained_limit,
        COUNTED,
        counted_count,
        RetainedBytes
    );
    lane_limit_test!(
        native_counted_lane_refuses_work_limit,
        COUNTED,
        counted_count,
        WorkUnits
    );
    lane_limit_test!(
        native_abr_lane_refuses_collection_limit,
        ABR,
        abr_count,
        CollectionItems
    );
    lane_limit_test!(
        native_abr_lane_refuses_retained_limit,
        ABR,
        abr_count,
        RetainedBytes
    );
    lane_limit_test!(
        native_abr_lane_refuses_work_limit,
        ABR,
        abr_count,
        WorkUnits
    );

    #[test]
    fn native_abr_lane_resolves_nullable_slots_within_its_offset_store() {
        let mut store = offset_only_indexed_om_section();
        let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
        let end_at = index_start + 3 * 4;
        let end = cadmpeg_core::decode::index_from_u32(u32::from_le_bytes(
            store[end_at..end_at + 4]
                .try_into()
                .expect("required invariant"),
        ));
        let mut lane = vec![0x11, 0x02];
        lane.extend_from_slice(&[0xff; 15]);
        lane.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03]);
        store.splice(end..end, lane.iter().copied());
        store[end_at..end_at + 4].copy_from_slice(
            &(u32::try_from(end + lane.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", store)]);
        let container =
            crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
                .expect("required invariant");

        let lanes = crate::test_support::with_decode_context(|ctx| {
            crate::native::om::compact_lane::data_block_abr_reference_lanes(ctx, &container)
        })
        .unwrap();
        assert_eq!(lanes.len(), 1);
        assert_eq!(
            lanes[0].frame.slots()[0]
                .atom
                .map(|target| target.atom.value()),
            Some(2)
        );
        assert_eq!(
            lanes[0].frame.slots()[0]
                .atom
                .map(|target| target.target.as_str()),
            Some("nx:om-data-blocks-0:block#2")
        );
        assert!(lanes[0].frame.slots()[1..]
            .iter()
            .all(|slot| slot.atom.is_none()));
        assert_eq!(lanes[0].frame.slots().len(), 16);
        assert_eq!(
            lanes[0].frame.slots()[0].offset,
            lanes[0].frame.offset() + 1
        );
    }
}
