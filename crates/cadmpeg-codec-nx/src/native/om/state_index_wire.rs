// SPDX-License-Identifier: Apache-2.0
//! Native operation-state index projections at the JSON boundary.

use super::{
    OmAuditTrailRow, OmOperationStateCounter, OmOperationStateJournalRow,
    OmOperationStateMessageBody, OmOperationStateStatus, OmOperationStateStatusPayload,
    OmRollForwardStateRow,
};
use crate::om::state_index::StateIndexToken;
use crate::om::state_slots::StateSlots;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct OmAuditTrailRowWire {
    id: String,
    section_link: String,
    ordinal: u32,
    raw_ordinal: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frame_selector: Option<u8>,
    timestamp: u32,
    #[serde(flatten)]
    value: crate::om::state_tagged_value::StateTaggedValue,
    raw: Vec<u8>,
    source_entry: String,
    source_offset: u64,
    end_offset: u64,
}

impl From<OmAuditTrailRow> for OmAuditTrailRowWire {
    fn from(value: OmAuditTrailRow) -> Self {
        let record = value.record();
        let end_offset = value.end_offset();
        Self {
            id: value.id,
            section_link: value.section_link,
            ordinal: record.ordinal.value(),
            raw_ordinal: record.ordinal.raw().to_vec(),
            frame_selector: record.frame_selector,
            timestamp: record.timestamp,
            value: record.value,
            raw: record.raw(),
            source_entry: value.source_entry,
            source_offset: value.source_offset,
            end_offset,
        }
    }
}

impl TryFrom<OmAuditTrailRowWire> for OmAuditTrailRow {
    type Error = String;
    fn try_from(wire: OmAuditTrailRowWire) -> Result<Self, Self::Error> {
        let record = crate::om::audit::AuditRecord {
            ordinal: StateIndexToken::from_wire(wire.ordinal, &wire.raw_ordinal)
                .map_err(|error| format!("ordinal/raw_ordinal: {error}"))?,
            frame_selector: wire.frame_selector,
            timestamp: wire.timestamp,
            value: wire.value,
        };
        if wire.raw != record.raw() {
            return Err("raw: disagrees with the audit fields".to_string());
        }
        let value = Self::new(
            wire.id,
            wire.section_link,
            record,
            wire.source_entry,
            wire.source_offset,
        )
        .ok_or("source_offset: audit extent exceeds u64")?;
        if wire.end_offset != value.end_offset() {
            return Err("end_offset: disagrees with the audit frame extent".to_string());
        }
        Ok(value)
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct OmOperationStateJournalRowWire {
    timestamp: u32,
    #[serde(flatten)]
    value: crate::om::state_tagged_value::StateTaggedValue,
    schema_id: u32,
    raw_schema_id: Vec<u8>,
    state_ordinal: u32,
    raw_state_ordinal: Vec<u8>,
    source_offset: u64,
    end_offset: u64,
}

impl From<OmOperationStateJournalRow> for OmOperationStateJournalRowWire {
    fn from(value: OmOperationStateJournalRow) -> Self {
        Self {
            timestamp: value.timestamp,
            value: value.value,
            schema_id: value.schema_id.value(),
            raw_schema_id: value.schema_id.raw().to_vec(),
            state_ordinal: value.state_ordinal.value(),
            raw_state_ordinal: value.state_ordinal.raw().to_vec(),
            source_offset: value.source_offset,
            end_offset: value.end_offset,
        }
    }
}

impl TryFrom<OmOperationStateJournalRowWire> for OmOperationStateJournalRow {
    type Error = String;
    fn try_from(wire: OmOperationStateJournalRowWire) -> Result<Self, Self::Error> {
        Ok(Self {
            timestamp: wire.timestamp,
            value: wire.value,
            schema_id: StateIndexToken::from_wire(wire.schema_id, &wire.raw_schema_id)
                .map_err(|error| format!("schema_id/raw_schema_id: {error}"))?,
            state_ordinal: StateIndexToken::from_wire(wire.state_ordinal, &wire.raw_state_ordinal)
                .map_err(|error| format!("state_ordinal/raw_state_ordinal: {error}"))?,
            source_offset: wire.source_offset,
            end_offset: wire.end_offset,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct OmOperationStateCounterWire {
    id: String,
    section_link: String,
    ordinal: u32,
    row_kind: crate::om::discriminators::OperationStateCounterKind,
    object_index: u32,
    raw_object_index: Vec<u8>,
    introduced_state: u8,
    modified_state: u8,
    object_index_source_offset: u64,
    source_entry: String,
    source_offset: u64,
}

impl From<OmOperationStateCounter> for OmOperationStateCounterWire {
    fn from(value: OmOperationStateCounter) -> Self {
        Self {
            id: value.id,
            section_link: value.section_link,
            ordinal: value.ordinal,
            row_kind: value.row_kind,
            object_index: value.object_index.value(),
            raw_object_index: value.object_index.raw().to_vec(),
            introduced_state: value.introduced_state,
            modified_state: value.modified_state,
            object_index_source_offset: value.object_index_source_offset,
            source_entry: value.source_entry,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<OmOperationStateCounterWire> for OmOperationStateCounter {
    type Error = String;
    fn try_from(wire: OmOperationStateCounterWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            section_link: wire.section_link,
            ordinal: wire.ordinal,
            row_kind: wire.row_kind,
            object_index: StateIndexToken::from_wire(wire.object_index, &wire.raw_object_index)
                .map_err(|error| format!("object_index/raw_object_index: {error}"))?,
            introduced_state: wire.introduced_state,
            modified_state: wire.modified_state,
            object_index_source_offset: wire.object_index_source_offset,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct OmOperationStateStatusWire {
    id: String,
    section_link: String,
    ordinal: u32,
    status_code: u32,
    raw_status_code: Vec<u8>,
    object_index: u32,
    raw_object_index: Vec<u8>,
    payload: OmOperationStateStatusPayload,
    source_entry: String,
    source_offset: u64,
    end_offset: u64,
}

impl From<OmOperationStateStatus> for OmOperationStateStatusWire {
    fn from(value: OmOperationStateStatus) -> Self {
        Self {
            id: value.id,
            section_link: value.section_link,
            ordinal: value.ordinal,
            status_code: value.status_code.value(),
            raw_status_code: value.status_code.raw().to_vec(),
            object_index: value.object_index.value(),
            raw_object_index: value.object_index.raw().to_vec(),
            payload: value.payload,
            source_entry: value.source_entry,
            source_offset: value.source_offset,
            end_offset: value.end_offset,
        }
    }
}

impl TryFrom<OmOperationStateStatusWire> for OmOperationStateStatus {
    type Error = String;
    fn try_from(wire: OmOperationStateStatusWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            section_link: wire.section_link,
            ordinal: wire.ordinal,
            status_code: StateIndexToken::from_wire(wire.status_code, &wire.raw_status_code)
                .map_err(|error| format!("status_code/raw_status_code: {error}"))?,
            object_index: StateIndexToken::from_wire(wire.object_index, &wire.raw_object_index)
                .map_err(|error| format!("object_index/raw_object_index: {error}"))?,
            payload: wire.payload,
            source_entry: wire.source_entry,
            source_offset: wire.source_offset,
            end_offset: wire.end_offset,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct OmOperationStateSlotWire {
    ordinal: u32,
    object_index: Option<u32>,
    raw_object_index: Vec<u8>,
}

impl OmOperationStateSlotWire {
    fn from_slot(ordinal: u32, value: Option<StateIndexToken>) -> Self {
        Self {
            ordinal,
            object_index: value.map(StateIndexToken::value),
            raw_object_index: value
                .as_ref()
                .map_or_else(|| vec![0xff], |index| index.raw().to_vec()),
        }
    }

    fn into_slot(self, ordinal: u32) -> Result<Option<StateIndexToken>, String> {
        if self.ordinal != ordinal {
            return Err("slots.ordinal: disagrees with slot position".to_string());
        }
        match (self.object_index, self.raw_object_index.as_slice()) {
            (None, [0xff]) => Ok(None),
            (Some(value), raw) => StateIndexToken::from_wire(value, raw)
                .map(Some)
                .map_err(|error| format!("object_index/raw_object_index: {error}")),
            _ => Err("object_index/raw_object_index: null requires the ff token".to_string()),
        }
    }
}

impl Serialize for StateSlots<Option<StateIndexToken>> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.len()))?;
        for (ordinal, slot) in self.iter() {
            sequence.serialize_element(&OmOperationStateSlotWire::from_slot(ordinal, *slot))?;
        }
        sequence.end()
    }
}

impl<'de> Deserialize<'de> for StateSlots<Option<StateIndexToken>> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let slots = StateSlots::new(Vec::<OmOperationStateSlotWire>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)?;
        slots
            .try_map_slots(|ordinal, slot| slot.into_slot(ordinal))
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
pub(super) enum OmRollForwardStateRowWire {
    List {
        ordinal: u32,
        object_index: u32,
        raw_object_index: Vec<u8>,
        position: u32,
        raw_position: Vec<u8>,
        source_offset: u64,
    },
    Pair {
        ordinal: u32,
        tag: crate::om::discriminators::OperationStatePairTag,
        first: u32,
        raw_first: Vec<u8>,
        second: u32,
        raw_second: Vec<u8>,
        source_offset: u64,
    },
}

impl OmRollForwardStateRowWire {
    pub(super) fn from_row(ordinal: u8, value: OmRollForwardStateRow) -> Self {
        let ordinal = u32::from(ordinal);
        match value {
            OmRollForwardStateRow::List {
                object_index,
                position,
                source_offset,
            } => Self::List {
                ordinal,
                object_index: object_index.value(),
                raw_object_index: object_index.raw().to_vec(),
                position: position.value(),
                raw_position: position.raw().to_vec(),
                source_offset,
            },
            OmRollForwardStateRow::Pair {
                tag,
                first,
                second,
                source_offset,
            } => Self::Pair {
                ordinal,
                tag,
                first: first.value(),
                raw_first: first.raw().to_vec(),
                second: second.value(),
                raw_second: second.raw().to_vec(),
                source_offset,
            },
        }
    }

    pub(super) fn into_row(self, expected_ordinal: u8) -> Result<OmRollForwardStateRow, String> {
        let ordinal = match &self {
            Self::List { ordinal, .. } | Self::Pair { ordinal, .. } => *ordinal,
        };
        if ordinal != u32::from(expected_ordinal) {
            return Err("rows.ordinal: disagrees with row position".to_string());
        }
        Ok(match self {
            OmRollForwardStateRowWire::List {
                ordinal: _,
                object_index,
                raw_object_index,
                position,
                raw_position,
                source_offset,
            } => OmRollForwardStateRow::List {
                object_index: StateIndexToken::from_wire(object_index, &raw_object_index)
                    .map_err(|error| format!("object_index/raw_object_index: {error}"))?,
                position: StateIndexToken::from_wire(position, &raw_position)
                    .map_err(|error| format!("position/raw_position: {error}"))?,
                source_offset,
            },
            OmRollForwardStateRowWire::Pair {
                ordinal: _,
                tag,
                first,
                raw_first,
                second,
                raw_second,
                source_offset,
            } => OmRollForwardStateRow::Pair {
                tag,
                first: StateIndexToken::from_wire(first, &raw_first)
                    .map_err(|error| format!("first/raw_first: {error}"))?,
                second: StateIndexToken::from_wire(second, &raw_second)
                    .map_err(|error| format!("second/raw_second: {error}"))?,
                source_offset,
            },
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) enum OmOperationStateStatusPayloadWire {
    Plain,
    Linked {
        link_code: crate::om::state_link::StateLinkCode,
        object_index: u32,
        raw_object_index: Vec<u8>,
    },
    Diagnostic(OmOperationStateMessageBody),
    Opaque {
        raw: Vec<u8>,
    },
}

impl From<OmOperationStateStatusPayload> for OmOperationStateStatusPayloadWire {
    fn from(value: OmOperationStateStatusPayload) -> Self {
        match value {
            OmOperationStateStatusPayload::Plain => Self::Plain,
            OmOperationStateStatusPayload::Linked {
                link_code,
                object_index,
            } => Self::Linked {
                link_code,
                object_index: object_index.value(),
                raw_object_index: object_index.raw().to_vec(),
            },
            OmOperationStateStatusPayload::Diagnostic(value) => Self::Diagnostic(value),
            OmOperationStateStatusPayload::Opaque { raw } => Self::Opaque { raw },
        }
    }
}

impl TryFrom<OmOperationStateStatusPayloadWire> for OmOperationStateStatusPayload {
    type Error = String;
    fn try_from(wire: OmOperationStateStatusPayloadWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            OmOperationStateStatusPayloadWire::Plain => Self::Plain,
            OmOperationStateStatusPayloadWire::Linked {
                link_code,
                object_index,
                raw_object_index,
            } => Self::Linked {
                link_code,
                object_index: StateIndexToken::from_wire(object_index, &raw_object_index)
                    .map_err(|error| format!("object_index/raw_object_index: {error}"))?,
            },
            OmOperationStateStatusPayloadWire::Diagnostic(value) => Self::Diagnostic(value),
            OmOperationStateStatusPayloadWire::Opaque { raw } => Self::Opaque { raw },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preserves_wire<T: Serialize + serde::de::DeserializeOwned>(json: &str) {
        let value: T = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
    }

    fn preserves_row_wire(json: &str) {
        let wire: OmRollForwardStateRowWire = serde_json::from_str(json).unwrap();
        let row = wire.into_row(0).unwrap();
        assert_eq!(
            serde_json::to_string(&OmRollForwardStateRowWire::from_row(0, row)).unwrap(),
            json
        );
    }

    fn preserves_slot_wire(ordinal: u32, json: &str) {
        let wire: OmOperationStateSlotWire = serde_json::from_str(json).unwrap();
        let slot = wire.into_slot(ordinal).unwrap();
        assert_eq!(
            serde_json::to_string(&OmOperationStateSlotWire::from_slot(ordinal, slot)).unwrap(),
            json
        );
    }

    #[test]
    fn audit_wire_rejects_raw_and_extent_disagreement() {
        let json = r#"{"id":"audit","section_link":"section","ordinal":2,"raw_ordinal":[2],"timestamp":0,"value_marker":160,"value":0,"raw_value":[160,0,0],"raw":[4,2,19,224,0,0,0,0,160,0,0],"source_entry":"om","source_offset":0,"end_offset":11}"#;
        let wire: serde_json::Value = serde_json::from_str(json).unwrap();
        for (field, replacement) in [
            ("raw", serde_json::json!([])),
            ("end_offset", serde_json::json!(12)),
            ("source_offset", serde_json::json!(u64::MAX)),
        ] {
            let mut invalid = wire.clone();
            invalid[field] = replacement;
            assert!(serde_json::from_value::<OmAuditTrailRow>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
        let mut boundary = wire;
        boundary["source_offset"] = (u64::MAX - 11).into();
        boundary["end_offset"] = u64::MAX.into();
        let row = serde_json::from_value::<OmAuditTrailRow>(boundary).unwrap();
        assert_eq!(row.end_offset(), u64::MAX);
    }

    #[test]
    fn state_index_records_preserve_scalar_and_token_fields() {
        preserves_wire::<OmAuditTrailRow>(
            r#"{"id":"audit","section_link":"section","ordinal":2,"raw_ordinal":[2],"timestamp":0,"value_marker":160,"value":0,"raw_value":[160,0,0],"raw":[4,2,19,224,0,0,0,0,160,0,0],"source_entry":"om","source_offset":0,"end_offset":11}"#,
        );
        preserves_wire::<OmAuditTrailRow>(
            r#"{"id":"audit","section_link":"section","ordinal":2,"raw_ordinal":[2],"frame_selector":7,"timestamp":0,"value_marker":160,"value":0,"raw_value":[160,0,0],"raw":[4,2,19,4,5,7,0,224,0,0,0,0,160,0,0],"source_entry":"om","source_offset":0,"end_offset":15}"#,
        );
        preserves_wire::<OmOperationStateJournalRow>(
            r#"{"timestamp":0,"value_marker":160,"value":0,"raw_value":[160,0,0],"schema_id":1,"raw_schema_id":[128,1],"state_ordinal":2,"raw_state_ordinal":[241,0,2],"source_offset":0,"end_offset":14}"#,
        );
        preserves_wire::<OmOperationStateCounter>(
            r#"{"id":"counter","section_link":"section","ordinal":0,"row_kind":1,"object_index":0,"raw_object_index":[0],"introduced_state":0,"modified_state":0,"object_index_source_offset":2,"source_entry":"om","source_offset":0}"#,
        );
        preserves_wire::<OmOperationStateStatus>(
            r#"{"id":"status","section_link":"section","ordinal":0,"status_code":65,"raw_status_code":[65],"object_index":1,"raw_object_index":[1],"payload":"Plain","source_entry":"om","source_offset":0,"end_offset":3}"#,
        );
        preserves_wire::<OmOperationStateStatusPayload>(
            r#"{"Linked":{"link_code":75,"object_index":1,"raw_object_index":[1]}}"#,
        );
        preserves_row_wire(
            r#"{"List":{"ordinal":0,"object_index":0,"raw_object_index":[144,0,0],"position":1,"raw_position":[1],"source_offset":0}}"#,
        );
        preserves_row_wire(
            r#"{"Pair":{"ordinal":0,"tag":79,"first":0,"raw_first":[0],"second":1,"raw_second":[1],"source_offset":0}}"#,
        );
        preserves_slot_wire(
            0,
            r#"{"ordinal":0,"object_index":null,"raw_object_index":[255]}"#,
        );
        preserves_slot_wire(
            1,
            r#"{"ordinal":1,"object_index":255,"raw_object_index":[144,0,255]}"#,
        );
    }

    #[test]
    fn index_slots_reject_null_value_and_token_disagreement() {
        for json in [
            r#"{"ordinal":0,"object_index":null,"raw_object_index":[]}"#,
            r#"{"ordinal":0,"object_index":null,"raw_object_index":[0]}"#,
            r#"{"ordinal":0,"object_index":0,"raw_object_index":[255]}"#,
            r#"{"ordinal":0,"object_index":1,"raw_object_index":[0]}"#,
            r#"{"ordinal":0,"object_index":0,"raw_object_index":[144,0]}"#,
            r#"{"ordinal":0,"object_index":0,"raw_object_index":[0,0]}"#,
        ] {
            assert!(serde_json::from_str::<OmOperationStateSlotWire>(json)
                .unwrap()
                .into_slot(0)
                .unwrap_err()
                .to_string()
                .contains("object_index/raw_object_index"));
        }
    }
}
