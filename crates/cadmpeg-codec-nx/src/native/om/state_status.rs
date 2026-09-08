// SPDX-License-Identifier: Apache-2.0
//! Native status-row metadata and flat wire admission.

use crate::om::state_index::StateIndexToken;
use crate::om::state_message::StateMessage;
use crate::om::state_status::{StateStatus, StateStatusPayload};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub(crate) struct OmOperationStateStatus {
    pub(crate) id: String,
    pub(crate) section_link: String,
    pub(crate) ordinal: u32,
    body: StateStatus<String, Vec<u8>>,
    pub(crate) source_entry: String,
    source_offset: u64,
}

impl OmOperationStateStatus {
    pub(crate) fn new(
        id: String,
        section_link: String,
        ordinal: u32,
        body: StateStatus<String, Vec<u8>>,
        source_entry: String,
        source_offset: u64,
    ) -> Option<Self> {
        source_offset.checked_add(u64::try_from(body.byte_len()).ok()?)?;
        Some(Self {
            id,
            section_link,
            ordinal,
            body,
            source_entry,
            source_offset,
        })
    }
    pub(crate) fn source_offset(&self) -> u64 {
        self.source_offset
    }
    pub(crate) fn end_offset(&self) -> u64 {
        self.source_offset + self.body.byte_len() as u64
    }
    #[cfg(test)]
    pub(crate) fn body(&self) -> &StateStatus<String, Vec<u8>> {
        &self.body
    }
}

#[derive(Serialize, Deserialize)]
struct Wire {
    id: String,
    section_link: String,
    ordinal: u32,
    status_code: u32,
    raw_status_code: Vec<u8>,
    object_index: u32,
    raw_object_index: Vec<u8>,
    payload: StateStatusPayload<String, Vec<u8>>,
    source_entry: String,
    source_offset: u64,
    end_offset: u64,
}

impl From<OmOperationStateStatus> for Wire {
    fn from(value: OmOperationStateStatus) -> Self {
        let end_offset = value.end_offset();
        Self {
            id: value.id,
            section_link: value.section_link,
            ordinal: value.ordinal,
            status_code: value.body.status_code.value(),
            raw_status_code: value.body.status_code.raw().to_vec(),
            object_index: value.body.object_index.value(),
            raw_object_index: value.body.object_index.raw().to_vec(),
            payload: value.body.payload,
            source_entry: value.source_entry,
            source_offset: value.source_offset,
            end_offset,
        }
    }
}

impl TryFrom<Wire> for OmOperationStateStatus {
    type Error = String;
    fn try_from(wire: Wire) -> Result<Self, Self::Error> {
        let body = StateStatus {
            status_code: StateIndexToken::from_wire(wire.status_code, &wire.raw_status_code)
                .map_err(|error| format!("status_code/raw_status_code: {error}"))?,
            object_index: StateIndexToken::from_wire(wire.object_index, &wire.raw_object_index)
                .map_err(|error| format!("object_index/raw_object_index: {error}"))?,
            payload: wire.payload,
        };
        let value = Self::new(
            wire.id,
            wire.section_link,
            wire.ordinal,
            body,
            wire.source_entry,
            wire.source_offset,
        )
        .ok_or("source_offset: status-row extent overflows")?;
        if wire.end_offset != value.end_offset() {
            return Err("end_offset: disagrees with status-row payload".into());
        }
        Ok(value)
    }
}

#[derive(Serialize, Deserialize)]
enum PayloadWire {
    Plain,
    Linked {
        link_code: crate::om::state_link::StateLinkCode,
        object_index: u32,
        raw_object_index: Vec<u8>,
    },
    Diagnostic(StateMessage<String>),
    Opaque {
        raw: Vec<u8>,
    },
}

impl From<StateStatusPayload<String, Vec<u8>>> for PayloadWire {
    fn from(value: StateStatusPayload<String, Vec<u8>>) -> Self {
        match value {
            StateStatusPayload::Plain => Self::Plain,
            StateStatusPayload::Linked {
                link_code,
                object_index,
            } => Self::Linked {
                link_code,
                object_index: object_index.value(),
                raw_object_index: object_index.raw().to_vec(),
            },
            StateStatusPayload::Diagnostic(value) => Self::Diagnostic(value),
            StateStatusPayload::Opaque { raw } => Self::Opaque { raw },
        }
    }
}

impl TryFrom<PayloadWire> for StateStatusPayload<String, Vec<u8>> {
    type Error = String;
    fn try_from(wire: PayloadWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            PayloadWire::Plain => Self::Plain,
            PayloadWire::Linked {
                link_code,
                object_index,
                raw_object_index,
            } => Self::Linked {
                link_code,
                object_index: StateIndexToken::from_wire(object_index, &raw_object_index)
                    .map_err(|error| format!("object_index/raw_object_index: {error}"))?,
            },
            PayloadWire::Diagnostic(value) => Self::Diagnostic(value),
            PayloadWire::Opaque { raw } => Self::Opaque { raw },
        })
    }
}

impl Serialize for StateStatusPayload<String, Vec<u8>> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PayloadWire::from(self.clone()).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for StateStatusPayload<String, Vec<u8>> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(PayloadWire::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::OmOperationStateStatus;

    #[test]
    fn wire_ends_follow_each_payload_form() {
        for (payload, end) in [
            (r#""Plain""#, 3),
            (
                r#"{"Linked":{"link_code":75,"object_index":1,"raw_object_index":[144,0,1]}}"#,
                8,
            ),
            (
                r#"{"Diagnostic":{"declared_length":3,"text":"A","value_marker":160,"value":0,"raw_value":[160,0,0],"count_or_severity":0}}"#,
                15,
            ),
            (r#"{"Opaque":{"raw":[2,1,17]}}"#, 5),
        ] {
            let json = format!(
                r#"{{"id":"status","section_link":"section","ordinal":0,"status_code":65,"raw_status_code":[65],"object_index":1,"raw_object_index":[1],"payload":{payload},"source_entry":"om","source_offset":0,"end_offset":{end}}}"#
            );
            let row: OmOperationStateStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&row).unwrap(), json);
            let wire: serde_json::Value = serde_json::from_str(&json).unwrap();
            let mut mismatch = wire.clone();
            mismatch["end_offset"] = (end + 1).into();
            assert!(serde_json::from_value::<OmOperationStateStatus>(mismatch)
                .unwrap_err()
                .to_string()
                .contains("end_offset"));
            let mut boundary = wire.clone();
            boundary["source_offset"] = (u64::MAX - end).into();
            boundary["end_offset"] = u64::MAX.into();
            assert!(serde_json::from_value::<OmOperationStateStatus>(boundary).is_ok());
            let mut overflow = wire;
            overflow["source_offset"] = (u64::MAX - end + 1).into();
            assert!(serde_json::from_value::<OmOperationStateStatus>(overflow)
                .unwrap_err()
                .to_string()
                .contains("source_offset"));
        }
    }
}
