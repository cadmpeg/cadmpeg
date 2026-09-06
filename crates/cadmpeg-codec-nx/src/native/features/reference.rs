// SPDX-License-Identifier: Apache-2.0
//! A construction reference with its resolved target and source position.

use crate::om::reference_index::ReferenceIndexToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstructionReference<B, T = ReferenceIndexToken> {
    pub(crate) token: T,
    pub(crate) data_block: B,
    pub(crate) source_offset: u64,
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
