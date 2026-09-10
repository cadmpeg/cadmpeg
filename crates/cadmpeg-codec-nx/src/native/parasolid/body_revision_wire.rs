// SPDX-License-Identifier: Apache-2.0
//! BODY revision length invariant and unchanged JSON fields.

use serde::{Deserialize, Serialize};

use super::ParasolidDeltasBodyRevision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RevisionLengths {
    prefix: u64,
    tail: u64,
}

impl RevisionLengths {
    pub(super) fn from_slices(prefix: &[u8], tail: &[u8]) -> Self {
        // Each byte slice is at most isize::MAX bytes; their sum fits u64.
        Self {
            prefix: prefix.len() as u64,
            tail: tail.len() as u64,
        }
    }

    pub(super) fn prefix(&self) -> u64 {
        self.prefix
    }
    pub(super) fn tail(&self) -> u64 {
        self.tail
    }
    pub(super) fn total(&self) -> u64 {
        self.prefix + self.tail
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct RevisionWire {
    id: String,
    stream_ordinal: u32,
    xmt: u32,
    node_id: u32,
    references: [u32; 8],
    byte_len: u64,
    prefix_byte_len: u64,
    state_tail_byte_len: u64,
    state_tail_sha256: crate::native::hex::Sha256Hex,
    inflated_offset: u64,
}

impl From<ParasolidDeltasBodyRevision> for RevisionWire {
    fn from(value: ParasolidDeltasBodyRevision) -> Self {
        Self {
            id: value.id,
            stream_ordinal: value.stream_ordinal,
            xmt: value.xmt.into(),
            node_id: value.node_id,
            references: value.references,
            byte_len: value.lengths.total(),
            prefix_byte_len: value.lengths.prefix(),
            state_tail_byte_len: value.lengths.tail(),
            state_tail_sha256: value.state_tail_sha256,
            inflated_offset: value.inflated_offset,
        }
    }
}

impl TryFrom<RevisionWire> for ParasolidDeltasBodyRevision {
    type Error = &'static str;

    fn try_from(wire: RevisionWire) -> Result<Self, Self::Error> {
        if wire.prefix_byte_len.checked_add(wire.state_tail_byte_len) != Some(wire.byte_len) {
            return Err(
                "byte_len: must equal prefix_byte_len + state_tail_byte_len without overflow",
            );
        }
        Ok(Self {
            id: wire.id,
            stream_ordinal: wire.stream_ordinal,
            xmt: wire.xmt.try_into()?,
            node_id: wire.node_id,
            references: wire.references,
            lengths: RevisionLengths {
                prefix: wire.prefix_byte_len,
                tail: wire.state_tail_byte_len,
            },
            state_tail_sha256: wire.state_tail_sha256,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ParasolidDeltasBodyRevision;

    #[test]
    fn revision_wire_derives_total_and_rejects_inconsistent_lengths() {
        let json = r#"{"id":"revision","stream_ordinal":0,"xmt":3,"node_id":9,"references":[2,3,4,5,6,7,8,9],"byte_len":36,"prefix_byte_len":32,"state_tail_byte_len":4,"state_tail_sha256":"hash","inflated_offset":10}"#;
        let value: ParasolidDeltasBodyRevision = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        for xmt in [0, 1] {
            let mut wire = serde_json::to_value(&value).unwrap();
            wire["xmt"] = xmt.into();
            let error = serde_json::from_value::<ParasolidDeltasBodyRevision>(wire).unwrap_err();
            assert!(error.to_string().contains("xmt"));
        }
        for (prefix, tail, total) in [(32, 4, 35), (u64::MAX, 1, 0)] {
            let mut wire = serde_json::to_value(&value).unwrap();
            wire["prefix_byte_len"] = prefix.into();
            wire["state_tail_byte_len"] = tail.into();
            wire["byte_len"] = total.into();
            let error = serde_json::from_value::<ParasolidDeltasBodyRevision>(wire).unwrap_err();
            assert!(error.to_string().contains("byte_len"));
        }
    }
}
