// SPDX-License-Identifier: Apache-2.0
//! Borrowed compact lane wire columns.

use super::{DataBlockAbrReferenceLane, DataBlockCountedIndexLane};
use crate::iter_wire::IterWire;
use crate::om::compact::{CompactIndexAtom, RawCompactIndex};
use serde::ser::SerializeMap;
use serde::Serialize;

struct RawNullableIndex(Option<CompactIndexAtom>);

impl Serialize for RawNullableIndex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Some(atom) => atom.raw().serialize(serializer),
            None => [0xff_u8].serialize(serializer),
        }
    }
}

impl Serialize for DataBlockCountedIndexLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        let anchor = self.frame.anchor();
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("data_block", &self.data_block)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry("declared_count", &self.frame.declared_count())?;
        wire.serialize_entry("anchor_index", &anchor.atom.value())?;
        wire.serialize_entry("raw_anchor_index", &RawCompactIndex(anchor.atom))?;
        wire.serialize_entry("anchor_data_block", anchor.target)?;
        wire.serialize_entry(
            "member_indices",
            &IterWire(self.frame.members().map(|index| index.atom.value())),
        )?;
        wire.serialize_entry(
            "raw_member_indices",
            &IterWire(
                self.frame
                    .members()
                    .map(|index| RawCompactIndex(index.atom)),
            ),
        )?;
        wire.serialize_entry(
            "member_data_blocks",
            &IterWire(self.frame.members().map(|index| index.target.as_str())),
        )?;
        wire.serialize_entry("source_offset", &self.frame.offset())?;
        wire.serialize_entry("anchor_source_offset", &anchor.offset)?;
        wire.serialize_entry(
            "member_source_offsets",
            &IterWire(self.frame.members().map(|index| index.offset)),
        )?;
        wire.end()
    }
}

impl Serialize for DataBlockAbrReferenceLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("section_ordinal", &self.section_ordinal)?;
        wire.serialize_entry("ordinal", &self.ordinal)?;
        wire.serialize_entry(
            "slot_indices",
            &self
                .frame
                .slots()
                .map(|slot| slot.atom.map(|index| index.atom.value())),
        )?;
        wire.serialize_entry(
            "raw_slot_indices",
            &self
                .frame
                .slots()
                .map(|slot| RawNullableIndex(slot.atom.map(|index| index.atom))),
        )?;
        wire.serialize_entry(
            "slot_data_blocks",
            &self
                .frame
                .slots()
                .map(|slot| slot.atom.map(|index| index.target.as_str())),
        )?;
        wire.serialize_entry(
            "slot_source_offsets",
            &self.frame.slots().map(|slot| slot.offset),
        )?;
        wire.serialize_entry("source_entry", &self.source_entry)?;
        wire.serialize_entry("source_offset", &self.frame.offset())?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_test_support::native_serialization::assert_native_limit;

    #[test]
    fn counted_lane_borrowed_bytes_and_limit() {
        let json = r#"{"id":"nx:om-counted-lane:lane#0","data_block":"block","ordinal":0,"declared_count":3,"anchor_index":1,"raw_anchor_index":[1],"anchor_data_block":"anchor","member_indices":[2],"raw_member_indices":[[2]],"member_data_blocks":["member"],"source_offset":9,"anchor_source_offset":11,"member_source_offsets":[12]}"#;
        let record: DataBlockCountedIndexLane = serde_json::from_str(json).unwrap();
        let borrowed = serde_json::to_vec(&record).unwrap();
        let owned = serde_json::to_vec(&super::super::wire::DataBlockCountedIndexLaneWire::from(
            record.clone(),
        ))
        .unwrap();
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed, json.as_bytes());
        assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }

    #[test]
    fn abr_lane_borrowed_bytes_and_limit() {
        let slots = std::array::from_fn(|index| {
            (index % 2 == 0).then(|| crate::om::compact::CompactIndexTarget {
                atom: CompactIndexAtom::from_wire(2, &[128, 2]).unwrap(),
                target: "block".to_owned(),
            })
        });
        let frame = crate::om::compact_lane::AbrLane::<String, u64>::new(slots, 9).unwrap();
        let record = DataBlockAbrReferenceLane {
            id: "nx:om-abr-lane:lane#0".to_owned(),
            section_ordinal: 0,
            ordinal: 0,
            frame,
            source_entry: "entry".to_owned(),
        };
        let borrowed = serde_json::to_vec(&record).unwrap();
        let owned = serde_json::to_vec(&super::super::wire::DataBlockAbrReferenceLaneWire::from(
            record.clone(),
        ))
        .unwrap();
        assert_eq!(borrowed, owned);
        assert_native_limit(&record, serde_json::to_value(&record).unwrap());
    }
}
