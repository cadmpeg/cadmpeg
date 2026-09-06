// SPDX-License-Identifier: Apache-2.0
//! Legacy lane columns, checked against the complete frame at deserialization.

use serde::{Deserialize, Serialize};
use super::{DataBlockCountedIndexLane, DataBlockAbrReferenceLane};
use crate::om::compact::{CompactIndexAtom, CompactIndexTarget, CountedIndexMembers};
use crate::om::compact_lane::{AbrLane, CountedLane};

#[derive(Serialize, Deserialize)]
pub(super) struct DataBlockCountedIndexLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Owning block in the native `data_blocks` arena.
    data_block: String,
    /// Zero-based lane order within the block.
    ordinal: u32,
    /// Serialized count including the anchor and terminal slot.
    declared_count: u8,
    /// Decoded anchoring block index.
    anchor_index: u32,
    /// Exact serialized anchor token.
    raw_anchor_index: Vec<u8>,
    /// Same-section block addressed by the anchor.
    anchor_data_block: String,
    /// Ordered decoded member block indices.
    member_indices: Vec<u32>,
    /// Exact serialized member tokens in lane order.
    raw_member_indices: Vec<Vec<u8>>,
    /// Ordered same-section blocks addressed by the members.
    member_data_blocks: Vec<String>,
    /// Absolute file offset of the opening `01` marker.
    source_offset: u64,
    /// Absolute file offset of the anchoring compact index.
    anchor_source_offset: u64,
    /// Ordered absolute file offsets of member compact indices.
    member_source_offsets: Vec<u64>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct DataBlockAbrReferenceLaneWire {
    /// Globally unique lane identity.
    id: String,
    /// Zero-based indexed-section ordinal within the container.
    section_ordinal: u32,
    /// Zero-based lane order within the section's column storage.
    ordinal: u32,
    /// Sixteen ordered nullable serialized block indices.
    slot_indices: [Option<u32>; 16],
    /// Exact compact-index tokens in slot order.
    raw_slot_indices: [Vec<u8>; 16],
    /// Sixteen ordered nullable same-section block identities.
    slot_data_blocks: [Option<String>; 16],
    /// Absolute file offsets of the sixteen compact-index tokens.
    slot_source_offsets: [u64; 16],
    /// Directory entry containing the offset-only store.
    source_entry: String,
    /// Absolute file offset of the opening `11` marker.
    source_offset: u64,
}

impl From<DataBlockCountedIndexLane> for DataBlockCountedIndexLaneWire {
    fn from(value: DataBlockCountedIndexLane) -> Self {
        let anchor = value.frame.anchor();
        Self {
            id: value.id, data_block: value.data_block, ordinal: value.ordinal,
            declared_count: value.frame.declared_count(),
            anchor_index: anchor.atom.value(), raw_anchor_index: anchor.atom.raw().to_vec(),
            anchor_data_block: anchor.target.clone(),
            member_indices: value.frame.members().map(|index| index.atom.value()).collect(),
            raw_member_indices: value.frame.members().map(|index| index.atom.raw().to_vec()).collect(),
            member_data_blocks: value.frame.members().map(|index| index.target.clone()).collect(),
            source_offset: value.frame.offset(), anchor_source_offset: anchor.offset,
            member_source_offsets: value.frame.members().map(|index| index.offset).collect(),
        }
    }
}

impl TryFrom<DataBlockCountedIndexLaneWire> for DataBlockCountedIndexLane {
    type Error = String;
    fn try_from(wire: DataBlockCountedIndexLaneWire) -> Result<Self, Self::Error> {
        let count = wire.member_indices.len();
        if wire.raw_member_indices.len() != count || wire.member_data_blocks.len() != count || wire.member_source_offsets.len() != count {
            return Err("member_indices/raw_member_indices/member_data_blocks/member_source_offsets: unequal lengths".into());
        }
        let members = wire.member_indices.into_iter().zip(wire.raw_member_indices).zip(wire.member_data_blocks)
            .map(|((index, raw), target)| Ok(CompactIndexTarget {
                atom: CompactIndexAtom::from_wire(index, &raw).map_err(|error| format!("member_indices/raw_member_indices: {error}"))?, target,
            })).collect::<Result<Vec<_>, String>>()?;
        let members = CountedIndexMembers::new(members)?;
        if wire.declared_count != members.declared_count() {
            return Err("declared_count: must equal member count plus anchor and terminator".into());
        }
        let anchor = CompactIndexTarget {
            atom: CompactIndexAtom::from_wire(wire.anchor_index, &wire.raw_anchor_index).map_err(|error| format!("anchor_index/raw_anchor_index: {error}"))?,
            target: wire.anchor_data_block,
        };
        let frame = CountedLane::<String, u64>::new(anchor, members, wire.source_offset).ok_or("source_offset: counted lane extent overflows")?;
        if frame.anchor().offset != wire.anchor_source_offset { return Err("anchor_source_offset: inconsistent with counted lane".into()); }
        if frame.members().map(|index| index.offset).ne(wire.member_source_offsets) { return Err("member_source_offsets: inconsistent with compact token widths".into()); }
        Ok(Self { id: wire.id, data_block: wire.data_block, ordinal: wire.ordinal, frame })
    }
}

impl From<DataBlockAbrReferenceLane> for DataBlockAbrReferenceLaneWire {
    fn from(value: DataBlockAbrReferenceLane) -> Self {
        let slots = value.frame.slots();
        Self {
            id: value.id, section_ordinal: value.section_ordinal, ordinal: value.ordinal,
            slot_indices: slots.each_ref().map(|slot| slot.atom.map(|index| index.atom.value())),
            raw_slot_indices: slots.each_ref().map(|slot| slot.atom.map_or_else(|| vec![0xff], |index| index.atom.raw().to_vec())),
            slot_data_blocks: slots.each_ref().map(|slot| slot.atom.map(|index| index.target.clone())),
            slot_source_offsets: slots.each_ref().map(|slot| slot.offset),
            source_entry: value.source_entry, source_offset: value.frame.offset(),
        }
    }
}

impl TryFrom<DataBlockAbrReferenceLaneWire> for DataBlockAbrReferenceLane {
    type Error = String;
    fn try_from(wire: DataBlockAbrReferenceLaneWire) -> Result<Self, Self::Error> {
        let mut slots = std::array::from_fn(|_| None);
        for (slot, ((raw, index), target)) in slots.iter_mut().zip(wire.raw_slot_indices.into_iter().zip(wire.slot_indices).zip(wire.slot_data_blocks)) {
            *slot = match (index, target) {
                (None, None) if raw == [0xff] => None,
                (Some(index), Some(target)) => Some(CompactIndexTarget {
                    atom: CompactIndexAtom::from_wire(index, &raw).map_err(|error| format!("slot_indices/raw_slot_indices: {error}"))?, target,
                }),
                _ => return Err("slot_indices/slot_data_blocks/raw_slot_indices: inconsistent null or target token".into()),
            };
        }
        let frame = AbrLane::<String, u64>::new(slots, wire.source_offset).ok_or("source_offset: ABR lane extent overflows")?;
        if frame.slots().each_ref().map(|slot| slot.offset) != wire.slot_source_offsets {
            return Err("slot_source_offsets: inconsistent with compact token widths".into());
        }
        Ok(Self { id: wire.id, section_ordinal: wire.section_ordinal, ordinal: wire.ordinal, source_entry: wire.source_entry, frame })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counted_wire_rejects_each_inconsistent_derived_position() {
        let json = serde_json::json!({
            "id": "lane", "data_block": "block", "ordinal": 0, "declared_count": 4,
            "anchor_index": 1, "raw_anchor_index": [128, 1], "anchor_data_block": "anchor",
            "member_indices": [2, 3], "raw_member_indices": [[128, 2], [3]], "member_data_blocks": ["a", "b"],
            "source_offset": 9, "anchor_source_offset": 11, "member_source_offsets": [13, 15],
        });
        let lane: DataBlockCountedIndexLane = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(lane).unwrap(), json);
        for field in ["source_offset", "anchor_source_offset"] {
            let mut invalid = json.clone();
            invalid[field] = serde_json::json!(u64::MAX);
            let error = serde_json::from_value::<DataBlockCountedIndexLane>(invalid).unwrap_err();
            assert!(error.to_string().contains(field), "{error}");
        }
        for member in 0..2 {
            let mut invalid = json.clone();
            invalid["member_source_offsets"][member] = serde_json::json!(u64::MAX);
            let error = serde_json::from_value::<DataBlockCountedIndexLane>(invalid).unwrap_err();
            assert!(error.to_string().contains("member_source_offsets"), "{error}");
        }
    }

    #[test]
    fn abr_wire_rejects_each_inconsistent_derived_position() {
        let frame = AbrLane::<String, u64>::new(std::array::from_fn(|i| {
            (i % 2 == 0).then(|| CompactIndexTarget { atom: CompactIndexAtom::from_wire(2, &[128, 2]).unwrap(), target: "block".into() })
        }), 9).unwrap();
        let lane = DataBlockAbrReferenceLane { id: "lane".into(), section_ordinal: 0, ordinal: 0, source_entry: "entry".into(), frame };
        let json = serde_json::to_value(&lane).unwrap();
        let decoded: DataBlockAbrReferenceLane = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(decoded, lane);
        assert_eq!(json["slot_source_offsets"], serde_json::json!([10, 12, 13, 15, 16, 18, 19, 21, 22, 24, 25, 27, 28, 30, 31, 33]));
        for slot in 0..16 {
            let mut invalid = json.clone();
            invalid["slot_source_offsets"][slot] = serde_json::json!(u64::MAX);
            let error = serde_json::from_value::<DataBlockAbrReferenceLane>(invalid).unwrap_err();
            assert!(error.to_string().contains("slot_source_offsets"), "{error}");
        }
        let mut invalid = json;
        invalid["source_offset"] = serde_json::json!(u64::MAX);
        let error = serde_json::from_value::<DataBlockAbrReferenceLane>(invalid).unwrap_err();
        assert!(error.to_string().contains("source_offset"), "{error}");
    }
    #[test]
    fn counted_lane_wire_preserves_member_columns() {
        let json = r#"{"id":"lane","data_block":"block","ordinal":0,"declared_count":3,"anchor_index":1,"raw_anchor_index":[1],"anchor_data_block":"anchor","member_indices":[2],"raw_member_indices":[[2]],"member_data_blocks":["member"],"source_offset":9,"anchor_source_offset":11,"member_source_offsets":[12]}"#;
        let lane: crate::native::om::compact_lane::DataBlockCountedIndexLane = serde_json::from_str(json).unwrap();
        assert_eq!(
            lane.frame.members().map(|index| (index.atom, index.target.as_str(), index.offset)).collect::<Vec<_>>(),
            [(crate::om::compact::CompactIndexAtom::read(&[2]).unwrap(), "member", 12)]
        );
        assert_eq!(serde_json::to_string(&lane).unwrap(), json);
        for field in [
            "member_indices",
            "raw_member_indices",
            "member_data_blocks",
            "member_source_offsets",
        ] {
            let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
            malformed[field] = serde_json::json!([]);
            assert!(serde_json::from_value::<crate::native::om::compact_lane::DataBlockCountedIndexLane>(malformed).is_err());
        }
        for (field, invalid) in [
            ("declared_count", serde_json::json!(4)),
            ("raw_anchor_index", serde_json::json!([255])),
            ("raw_member_indices", serde_json::json!([[3]])),
        ] {
            let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
            malformed[field] = invalid;
            let error = serde_json::from_value::<crate::native::om::compact_lane::DataBlockCountedIndexLane>(malformed).unwrap_err();
            assert!(error.to_string().contains(field), "{error}");
        }

    }

    #[test]
    fn abr_lane_wire_preserves_sixteen_nullable_columns() {
        let json = r#"{"id":"lane","section_ordinal":0,"ordinal":0,"slot_indices":[2,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null],"raw_slot_indices":[[2],[255],[255],[255],[255],[255],[255],[255],[255],[255],[255],[255],[255],[255],[255],[255]],"slot_data_blocks":["block",null,null,null,null,null,null,null,null,null,null,null,null,null,null,null],"slot_source_offsets":[10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25],"source_entry":"entry","source_offset":9}"#;
        let lane: crate::native::om::compact_lane::DataBlockAbrReferenceLane = serde_json::from_str(json).unwrap();
        assert_eq!(
            (lane.frame.slots()[0].atom.map(|index| (index.atom, index.target.as_str())), lane.frame.slots()[0].offset),
            (Some((crate::om::compact::CompactIndexAtom::read(&[2]).unwrap(), "block")), 10)
        );
        assert!(lane.frame.slots()[1..].iter().all(|slot| slot.atom.is_none()));
        assert_eq!(serde_json::to_string(&lane).unwrap(), json);
        for field in [
            "slot_indices",
            "raw_slot_indices",
            "slot_data_blocks",
            "slot_source_offsets",
        ] {
            let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
            malformed[field].as_array_mut().unwrap().pop();
            assert!(serde_json::from_value::<crate::native::om::compact_lane::DataBlockAbrReferenceLane>(malformed).is_err());
        }
        for field in ["slot_indices", "slot_data_blocks"] {
            let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
            malformed[field][0] = serde_json::Value::Null;
            assert!(serde_json::from_value::<crate::native::om::compact_lane::DataBlockAbrReferenceLane>(malformed).is_err());
        }
        for (slot, raw) in [(0, vec![3]), (1, vec![0])] {
            let mut malformed: serde_json::Value = serde_json::from_str(json).unwrap();
            malformed["raw_slot_indices"][slot] = serde_json::json!(raw);
            let error = serde_json::from_value::<crate::native::om::compact_lane::DataBlockAbrReferenceLane>(malformed).unwrap_err();
            assert!(error.to_string().contains("raw_slot_indices"), "{error}");
        }

    }


}
