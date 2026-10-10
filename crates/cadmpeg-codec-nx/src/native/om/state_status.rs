// SPDX-License-Identifier: Apache-2.0
//! Native status-row metadata and flat wire admission.

use cadmpeg_ir::native::bytes::NativeBytes;

use crate::om::state_index::StateIndexToken;
use crate::om::state_message::StateMessage;
use crate::om::state_status::{StateStatus, StateStatusPayload};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Wire")]
pub(in crate::native) struct OmOperationStateStatus {
    pub(in crate::native) id: String,
    section_link: String,
    ordinal: u32,
    body: StateStatus<String, NativeBytes>,
    source_entry: String,
    source_offset: u64,
}

#[derive(Serialize)]
struct WireView<'a> {
    id: &'a str,
    section_link: &'a str,
    ordinal: u32,
    status_code: u32,
    raw_status_code: NativeBytes<&'a [u8]>,
    object_index: u32,
    raw_object_index: NativeBytes<&'a [u8]>,
    payload: PayloadView<'a>,
    source_entry: &'a str,
    source_offset: u64,
    end_offset: u64,
}

impl Serialize for OmOperationStateStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WireView {
            id: &self.id,
            section_link: &self.section_link,
            ordinal: self.ordinal,
            status_code: self.body.status_code.value(),
            raw_status_code: (self.body.status_code.raw()).into(),
            object_index: self.body.object_index.value(),
            raw_object_index: (self.body.object_index.raw()).into(),
            payload: PayloadView::from(&self.body.payload),
            source_entry: &self.source_entry,
            source_offset: self.source_offset,
            end_offset: self.end_offset(),
        }
        .serialize(serializer)
    }
}

impl OmOperationStateStatus {
    pub(super) fn new(
        id: String,
        section_link: String,
        ordinal: u32,
        body: StateStatus<String, Vec<u8>>,
        source_entry: String,
        source_offset: u64,
    ) -> Option<Self> {
        source_offset.checked_add(u64::try_from(body.byte_len()).ok()?)?;
        let payload = match body.payload {
            StateStatusPayload::Plain => StateStatusPayload::Plain,
            StateStatusPayload::Linked {
                link_code,
                object_index,
            } => StateStatusPayload::Linked {
                link_code,
                object_index,
            },
            StateStatusPayload::Diagnostic(message) => StateStatusPayload::Diagnostic(message),
            StateStatusPayload::Opaque { raw } => StateStatusPayload::Opaque { raw: raw.into() },
        };
        let body = StateStatus {
            status_code: body.status_code,
            object_index: body.object_index,
            payload,
        };
        Some(Self {
            id,
            section_link,
            ordinal,
            body,
            source_entry,
            source_offset,
        })
    }
    pub(in crate::native) fn source_offset(&self) -> u64 {
        self.source_offset
    }
    fn end_offset(&self) -> u64 {
        self.source_offset + cadmpeg_core::decode::u64_from_index(self.body.byte_len())
    }
    #[cfg(test)]
    pub(super) fn body(&self) -> &StateStatus<String, NativeBytes> {
        &self.body
    }
}

#[derive(Deserialize)]
struct Wire {
    id: String,
    section_link: String,
    ordinal: u32,
    status_code: u32,
    raw_status_code: NativeBytes<Vec<u8>>,
    object_index: u32,
    raw_object_index: NativeBytes<Vec<u8>>,
    payload: PayloadWire,
    source_entry: String,
    source_offset: u64,
    end_offset: u64,
}

impl TryFrom<Wire> for OmOperationStateStatus {
    type Error = String;
    fn try_from(wire: Wire) -> Result<Self, Self::Error> {
        let body = StateStatus {
            status_code: StateIndexToken::from_wire(wire.status_code, &wire.raw_status_code)
                .map_err(|error| format!("status_code/raw_status_code: {error}"))?,
            object_index: StateIndexToken::from_wire(wire.object_index, &wire.raw_object_index)
                .map_err(|error| format!("object_index/raw_object_index: {error}"))?,
            payload: wire.payload.try_into()?,
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

#[derive(Deserialize)]
enum PayloadWire {
    Plain,
    Linked {
        link_code: crate::om::state_link::StateLinkCode,
        object_index: u32,
        raw_object_index: NativeBytes<Vec<u8>>,
    },
    Diagnostic(StateMessage<String>),
    Opaque {
        raw: NativeBytes<Vec<u8>>,
    },
}

#[derive(Serialize)]
enum PayloadView<'a> {
    Plain,
    Linked {
        link_code: crate::om::state_link::StateLinkCode,
        object_index: u32,
        raw_object_index: NativeBytes<&'a [u8]>,
    },
    Diagnostic(&'a StateMessage<String>),
    Opaque {
        raw: NativeBytes<&'a [u8]>,
    },
}

impl<'a, B: AsRef<[u8]>> From<&'a StateStatusPayload<String, B>> for PayloadView<'a> {
    fn from(value: &'a StateStatusPayload<String, B>) -> Self {
        match value {
            StateStatusPayload::Plain => Self::Plain,
            StateStatusPayload::Linked {
                link_code,
                object_index,
            } => Self::Linked {
                link_code: *link_code,
                object_index: object_index.value(),
                raw_object_index: object_index.raw().into(),
            },
            StateStatusPayload::Diagnostic(value) => Self::Diagnostic(value),
            StateStatusPayload::Opaque { raw } => Self::Opaque {
                raw: raw.as_ref().into(),
            },
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
            PayloadWire::Opaque { raw } => Self::Opaque {
                raw: raw.into_inner(),
            },
        })
    }
}

impl Serialize for StateStatusPayload<String, Vec<u8>> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PayloadView::from(self).serialize(serializer)
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
    fn om_status_payload_streams_once_with_native_retained_limit() {
        let expected = serde_json::json!({
            "id": "nx:om:status#1", "section_link": "section", "ordinal": 0,
            "status_code": 65, "raw_status_code": "41",
            "object_index": 1, "raw_object_index": "01",
            "payload": {"Opaque":{"raw":"020111"}},
            "source_entry": "om", "source_offset": 0, "end_offset": 5
        });
        let record: OmOperationStateStatus = serde_json::from_value(expected.clone()).unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(&record, expected);
    }

    #[test]
    fn wire_ends_follow_each_payload_form() {
        for (payload, end) in [
            (r#""Plain""#, 3),
            (
                r#"{"Linked":{"link_code":75,"object_index":1,"raw_object_index":"900001"}}"#,
                8,
            ),
            (
                r#"{"Diagnostic":{"declared_length":3,"text":"A","value_marker":160,"value":0,"raw_value":"a00000","count_or_severity":0}}"#,
                15,
            ),
            (r#"{"Opaque":{"raw":"020111"}}"#, 5),
        ] {
            let json = format!(
                r#"{{"id":"status","section_link":"section","ordinal":0,"status_code":65,"raw_status_code":"41","object_index":1,"raw_object_index":"01","payload":{payload},"source_entry":"om","source_offset":0,"end_offset":{end}}}"#
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
