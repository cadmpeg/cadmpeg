// SPDX-License-Identifier: Apache-2.0
//! Resolved counted and ABR compact-index lanes.

use super::control_index_data_block;
use crate::container::Container;
use crate::om::compact_lane::scan::{abr_lanes, counted_lanes};
use crate::om::compact_lane::{AbrLane, CountedLane};
use cadmpeg_core::decode::DecodeContext;
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

/// Decode complete in-range counted block-index lanes from offset-only stores.
pub(in crate::native) fn data_block_counted_index_lanes(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DataBlockCountedIndexLane>, CodecError> {
    let mut output = Vec::new();
    for (section_ordinal, (entry, section)) in container.indexed_om_sections().into_iter().enumerate() {
        let Some((_, _, records)) = section.as_offset_only() else { continue; };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let block_count = records.len() + 1;
        for (record_ordinal, block) in records.iter().enumerate() {
            let block_ordinal = record_ordinal + 1;
            let Some(source_base) = entry_offset.checked_add(block.offset as u64) else { continue; };
            let mut ordinal = 0usize;
            for lane in counted_lanes(ctx, block.bytes)? {
                let Some(lane) = lane.into_absolute(source_base) else { continue; };
                let Some(frame) = lane.try_resolve_charged(ctx, |atom| {
                    control_index_data_block(section_ordinal, block_count, atom.value())
                })? else { continue; };
                output.push(DataBlockCountedIndexLane {
                    id: format!("nx:om-data-block-counted-index-lanes-{section_ordinal}-{block_ordinal}:lane#{ordinal}"),
                    data_block: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                    ordinal: ordinal as u32,
                    frame,
                });
                ordinal += 1;
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
    for (section_ordinal, (entry, section)) in container.indexed_om_sections().into_iter().enumerate() {
        let Some((_, storage, records)) = section.as_offset_only() else { continue; };
        let Some(storage_offset) = records.first().map(|record| record.offset) else { continue; };
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        let Some(source_base) = entry_offset.checked_add(storage_offset as u64) else { continue; };
        let block_count = records.len() + 1;
        let mut ordinal = 0usize;
        for lane in abr_lanes(ctx, storage)? {
            let Some(frame) = lane.into_absolute(source_base).and_then(|lane| lane.try_resolve(|atom| {
                control_index_data_block(section_ordinal, block_count, atom.value())
            })) else { continue; };
            output.push(DataBlockAbrReferenceLane {
                id: format!("nx:om-data-block-abr-reference-lanes-{section_ordinal}:lane#{ordinal}"),
                section_ordinal: section_ordinal as u32,
                ordinal: ordinal as u32,
                frame,
                source_entry: entry.name.clone(),
            });
            ordinal += 1;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use crate::container;
    use crate::test_support::test_om::offset_only_indexed_om_section;
    use crate::test_support::test_prt::prt_with_named_payloads;

    #[test]
    fn native_abr_lane_resolves_nullable_slots_within_its_offset_store() {
        let mut store = offset_only_indexed_om_section();
        let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
        let end_at = index_start + 3 * 4;
        let end = u32::from_le_bytes(
            store[end_at..end_at + 4]
                .try_into()
                .expect("required invariant"),
        ) as usize;
        let mut lane = vec![0x11, 0x02];
        lane.extend_from_slice(&[0xff; 15]);
        lane.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03]);
        store.splice(end..end, lane.iter().copied());
        store[end_at..end_at + 4].copy_from_slice(&((end + lane.len()) as u32).to_le_bytes());
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
