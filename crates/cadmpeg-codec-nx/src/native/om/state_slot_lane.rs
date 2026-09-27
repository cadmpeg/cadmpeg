// SPDX-License-Identifier: Apache-2.0
//! Native operation-state slot-lane metadata and wire admission.

use crate::om::state_index::StateIndexToken;
use crate::om::state_slot_lane::StateSlotLane;
use crate::om::state_slots::StateSlots;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Wire")]
pub(in crate::native) struct OmOperationStateSlotLane {
    pub(in crate::native) id: String,
    pub(super) section_link: String,
    pub(super) ordinal: u32,
    pub(in crate::native) frame: StateSlotLane<u64>,
    pub(super) source_entry: String,
}

#[derive(Serialize)]
struct SlotLaneRef<'a> {
    id: &'a str,
    section_link: &'a str,
    ordinal: u32,
    slots: &'a StateSlots<Option<StateIndexToken>>,
    source_entry: &'a str,
    source_offset: u64,
    end_offset: u64,
}

impl Serialize for OmOperationStateSlotLane {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SlotLaneRef {
            id: &self.id,
            section_link: &self.section_link,
            ordinal: self.ordinal,
            slots: self.frame.slots(),
            source_entry: &self.source_entry,
            source_offset: self.frame.offset(),
            end_offset: self.frame.end_offset(),
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct Wire {
    id: String,
    section_link: String,
    ordinal: u32,
    slots: StateSlots<Option<StateIndexToken>>,
    source_entry: String,
    source_offset: u64,
    end_offset: u64,
}

#[cfg(test)]
impl From<OmOperationStateSlotLane> for Wire {
    fn from(value: OmOperationStateSlotLane) -> Self {
        Self {
            id: value.id,
            section_link: value.section_link,
            ordinal: value.ordinal,
            source_offset: value.frame.offset(),
            end_offset: value.frame.end_offset(),
            slots: value.frame.into_slots(),
            source_entry: value.source_entry,
        }
    }
}

impl TryFrom<Wire> for OmOperationStateSlotLane {
    type Error = String;
    fn try_from(wire: Wire) -> Result<Self, Self::Error> {
        let frame = StateSlotLane::new(wire.source_offset, wire.slots)?;
        if wire.end_offset != frame.end_offset() {
            return Err("end_offset: disagrees with the slot-lane tokens".into());
        }
        Ok(Self {
            id: wire.id,
            section_link: wire.section_link,
            ordinal: wire.ordinal,
            frame,
            source_entry: wire.source_entry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::OmOperationStateSlotLane;

    #[test]
    fn wire_end_is_derived_from_slots_and_fixed_framing() {
        let json = r#"{"id":"lane","section_link":"section","ordinal":0,"slots":[],"source_entry":"om","source_offset":10,"end_offset":15}"#;
        let lane: OmOperationStateSlotLane = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&lane).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&lane).unwrap(),
            serde_json::to_vec(&super::Wire::from(lane.clone())).unwrap()
        );
        let wire: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut mismatch = wire.clone();
        mismatch["end_offset"] = 16.into();
        assert!(serde_json::from_value::<OmOperationStateSlotLane>(mismatch)
            .unwrap_err()
            .to_string()
            .contains("end_offset"));
        let mut boundary = wire.clone();
        boundary["source_offset"] = (u64::MAX - 5).into();
        boundary["end_offset"] = u64::MAX.into();
        assert!(serde_json::from_value::<OmOperationStateSlotLane>(boundary).is_ok());
        let mut overflow = wire;
        overflow["source_offset"] = (u64::MAX - 4).into();
        assert!(serde_json::from_value::<OmOperationStateSlotLane>(overflow)
            .unwrap_err()
            .to_string()
            .contains("source_offset"));
    }

    #[test]
    fn slot_lane_native_limit_refuses_before_string_copy() {
        let json = r#"{"id":"nx:om:state-slot-lane#0","section_link":"section","ordinal":0,"slots":[],"source_entry":"om","source_offset":10,"end_offset":15}"#;
        let lane: OmOperationStateSlotLane = serde_json::from_str(json).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &lane,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
