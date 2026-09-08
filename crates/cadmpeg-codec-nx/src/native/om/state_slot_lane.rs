// SPDX-License-Identifier: Apache-2.0
//! Native operation-state slot-lane metadata and wire admission.

use crate::om::state_index::StateIndexToken;
use crate::om::state_slot_lane::StateSlotLane;
use crate::om::state_slots::StateSlots;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub(crate) struct OmOperationStateSlotLane {
    pub(crate) id: String,
    pub(crate) section_link: String,
    pub(crate) ordinal: u32,
    pub(crate) frame: StateSlotLane<u64>,
    pub(crate) source_entry: String,
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
}
