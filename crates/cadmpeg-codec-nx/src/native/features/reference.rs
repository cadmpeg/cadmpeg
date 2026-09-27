// SPDX-License-Identifier: Apache-2.0
//! A construction reference with its resolved target and source position.

use crate::om::reference_index::ReferenceIndexToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConstructionReference<B, T = ReferenceIndexToken> {
    pub(super) token: T,
    pub(super) data_block: B,
    pub(super) source_offset: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Body11ContinuationWire {
    id: String,
    operation_label: String,
    body_reference_ordinal: u32,
    body_object_index: u32,
    continuation_index: u32,
    raw_continuation_index: Vec<u8>,
    continuation_source_offset: u64,
    terminal_object_index: u32,
    raw_terminal_object_index: Vec<u8>,
    terminal_source_offset: u64,
}

impl serde::Serialize for super::FeatureOperationBody11Continuation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut wire = serializer.serialize_map(None)?;
        wire.serialize_entry("id", &self.id)?;
        wire.serialize_entry("operation_label", &self.operation_label)?;
        wire.serialize_entry("body_reference_ordinal", &self.body_reference_ordinal)?;
        wire.serialize_entry("body_object_index", &self.body_object_index)?;
        wire.serialize_entry("continuation_index", &self.continuation.atom.value())?;
        wire.serialize_entry("raw_continuation_index", self.continuation.atom.raw())?;
        wire.serialize_entry("continuation_source_offset", &self.continuation.offset)?;
        wire.serialize_entry("terminal_object_index", &self.terminal.value())?;
        wire.serialize_entry("raw_terminal_object_index", self.terminal.raw())?;
        wire.serialize_entry("terminal_source_offset", &self.terminal_source_offset)?;
        wire.end()
    }
}

#[cfg(test)]
impl From<super::FeatureOperationBody11Continuation> for Body11ContinuationWire {
    fn from(value: super::FeatureOperationBody11Continuation) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            body_reference_ordinal: value.body_reference_ordinal,
            body_object_index: value.body_object_index,
            continuation_index: value.continuation.atom.value(),
            raw_continuation_index: value.continuation.atom.raw().to_vec(),
            continuation_source_offset: value.continuation.offset,
            terminal_object_index: value.terminal.value(),
            raw_terminal_object_index: value.terminal.raw().to_vec(),
            terminal_source_offset: value.terminal_source_offset,
        }
    }
}

impl TryFrom<Body11ContinuationWire> for super::FeatureOperationBody11Continuation {
    type Error = String;

    fn try_from(wire: Body11ContinuationWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            body_reference_ordinal: wire.body_reference_ordinal,
            body_object_index: wire.body_object_index,
            continuation: crate::om::compact::LocatedCompactIndex {
                atom: crate::om::compact::CompactIndexAtom::from_wire(
                    wire.continuation_index,
                    &wire.raw_continuation_index,
                )
                .map_err(|error| format!("continuation_index: {error}"))?,
                offset: wire.continuation_source_offset,
            },
            terminal: ReferenceIndexToken::from_wire(
                wire.terminal_object_index,
                &wire.raw_terminal_object_index,
            )
            .map_err(|error| format!("terminal_object_index: {error}"))?,
            terminal_source_offset: wire.terminal_source_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{super::FeatureOperationBody11Continuation, Body11ContinuationWire};

    #[test]
    fn body11_continuation_borrowed_wire_matches_owned_bytes_and_retained_limit() {
        let json = r#"{"id":"nx:feature:body11-continuation#0","operation_label":"operation","body_reference_ordinal":0,"body_object_index":114,"continuation_index":67,"raw_continuation_index":[128,67],"continuation_source_offset":126,"terminal_object_index":113,"raw_terminal_object_index":[113],"terminal_source_offset":131}"#;
        let record: FeatureOperationBody11Continuation = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_vec(&record).unwrap(), json.as_bytes());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&Body11ContinuationWire::from(record.clone())).unwrap()
        );
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::from_str::<serde_json::Value>(json).unwrap(),
        );
    }
}
