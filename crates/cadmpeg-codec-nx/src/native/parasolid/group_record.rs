// SPDX-License-Identifier: Apache-2.0
//! Exact source scope and wire fields of GROUP records.

use super::ParasolidGroupRecord;
use crate::deltas::group::{GroupReferenceStatus, GroupSelector};
use crate::parasolid::StreamKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupOrigin {
    Partition { stream_ordinal: u32 },
    Deltas { stream_ordinal: u32, partition_stream_ordinal: Option<u32> },
}
impl GroupOrigin {
    pub(crate) fn stream_ordinal(self) -> u32 {
        match self { Self::Partition { stream_ordinal } | Self::Deltas { stream_ordinal, .. } => stream_ordinal }
    }
    pub(crate) fn stream_kind(self) -> StreamKind {
        match self { Self::Partition { .. } => StreamKind::Partition, Self::Deltas { .. } => StreamKind::Deltas }
    }
    pub(crate) fn partition_stream_ordinal(self) -> Option<u32> {
        match self { Self::Partition { stream_ordinal } => Some(stream_ordinal), Self::Deltas { partition_stream_ordinal, .. } => partition_stream_ordinal }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct GroupWire {
    id: String,
    stream_ordinal: u32,
    stream_kind: StreamKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    partition_stream_ordinal: Option<u32>,
    xmt: u32,
    node_id: u32,
    references: Vec<u32>,
    selector: GroupSelector,
    linked_reference_status: GroupReferenceStatus,
    byte_len: u64,
    inflated_offset: u64,
}
impl From<ParasolidGroupRecord> for GroupWire {
    fn from(value: ParasolidGroupRecord) -> Self {
        Self {
            id: value.id, stream_ordinal: value.origin.stream_ordinal(),
            stream_kind: value.origin.stream_kind(), partition_stream_ordinal: value.origin.partition_stream_ordinal(),
            xmt: value.xmt, node_id: value.node_id, references: value.references.to_vec(),
            selector: value.selector, linked_reference_status: value.linked_reference_status,
            byte_len: value.byte_len, inflated_offset: value.inflated_offset,
        }
    }
}
impl TryFrom<GroupWire> for ParasolidGroupRecord {
    type Error = &'static str;
    fn try_from(wire: GroupWire) -> Result<Self, Self::Error> {
        let origin = match wire.stream_kind {
            StreamKind::Partition if wire.partition_stream_ordinal == Some(wire.stream_ordinal) => GroupOrigin::Partition { stream_ordinal: wire.stream_ordinal },
            StreamKind::Deltas => GroupOrigin::Deltas { stream_ordinal: wire.stream_ordinal, partition_stream_ordinal: wire.partition_stream_ordinal },
            StreamKind::Partition => return Err("partition_stream_ordinal: a partition owns its stream namespace"),
            StreamKind::Plain | StreamKind::Preview => return Err("stream_kind: GROUP requires partition or deltas"),
        };
        let references = wire.references.try_into().map_err(|_| "references: GROUP requires five entries")?;
        Ok(Self {
            id: wire.id, origin, xmt: wire.xmt, node_id: wire.node_id, references,
            selector: wire.selector, linked_reference_status: wire.linked_reference_status,
            byte_len: wire.byte_len, inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_wire_preserves_scopes_and_rejects_invalid_controls() {
        for scope in [
            r#""stream_ordinal":4,"stream_kind":"partition","partition_stream_ordinal":4"#,
            r#""stream_ordinal":5,"stream_kind":"deltas","partition_stream_ordinal":4"#,
            r#""stream_ordinal":5,"stream_kind":"deltas""#,
        ] {
            let json = format!(r#"{{"id":"group",{scope},"xmt":10,"node_id":7,"references":[3,4,5,6,7],"selector":4,"linked_reference_status":0,"byte_len":20,"inflated_offset":0}}"#);
            let group: ParasolidGroupRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&group).unwrap(), json);
            for (field, invalid) in [("selector", serde_json::json!(3)), ("linked_reference_status", serde_json::json!(2)), ("stream_kind", serde_json::json!("plain")), ("references", serde_json::json!([1,2]))] {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire[field] = invalid;
                let error = serde_json::from_value::<ParasolidGroupRecord>(wire).unwrap_err();
                assert!(error.to_string().contains(field), "{error}");
            }
            if scope.contains("partition\"") {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire["partition_stream_ordinal"] = serde_json::json!(3);
                assert!(serde_json::from_value::<ParasolidGroupRecord>(wire).unwrap_err().to_string().contains("partition_stream_ordinal"));
            }
        }
    }
}
