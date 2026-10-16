// SPDX-License-Identifier: Apache-2.0
//! Exact source scope and wire fields of GROUP records.

use super::ParasolidGroupRecord;
use crate::deltas::group::GroupReferenceStatus;
use crate::parasolid::StreamKind;
use crate::records::group::GroupSelector;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::native) enum GroupOrigin {
    Partition {
        stream_ordinal: u32,
    },
    Deltas {
        stream_ordinal: u32,
        partition_stream_ordinal: Option<u32>,
        linked_reference_status: GroupReferenceStatus,
    },
}
impl GroupOrigin {
    pub(in crate::native) fn stream_ordinal(self) -> u32 {
        match self {
            Self::Partition { stream_ordinal } | Self::Deltas { stream_ordinal, .. } => {
                stream_ordinal
            }
        }
    }
    pub(super) fn stream_kind(self) -> StreamKind {
        match self {
            Self::Partition { .. } => StreamKind::Partition,
            Self::Deltas { .. } => StreamKind::Deltas,
        }
    }
    pub(in crate::native) fn partition_stream_ordinal(self) -> Option<u32> {
        match self {
            Self::Partition { stream_ordinal } => Some(stream_ordinal),
            Self::Deltas {
                partition_stream_ordinal,
                ..
            } => partition_stream_ordinal,
        }
    }
    fn linked_reference_status(self) -> Option<GroupReferenceStatus> {
        match self {
            Self::Partition { .. } => None,
            Self::Deltas {
                linked_reference_status,
                ..
            } => Some(linked_reference_status),
        }
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(super) struct GroupWire {
    id: String,
    stream_ordinal: u32,
    stream_kind: StreamKind,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_partition_stream_ordinal"
    )]
    partition_stream_ordinal: Option<u32>,
    xmt: u32,
    node_id: u32,
    references: Vec<u32>,
    selector: GroupSelector,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_linked_reference_status"
    )]
    linked_reference_status: Option<GroupReferenceStatus>,
    byte_len: u64,
    inflated_offset: u64,
}
#[derive(Serialize)]
struct GroupRef<'a> {
    id: &'a str,
    stream_ordinal: u32,
    stream_kind: StreamKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    partition_stream_ordinal: Option<u32>,
    xmt: u32,
    node_id: u32,
    references: &'a [u32],
    selector: GroupSelector,
    #[serde(skip_serializing_if = "Option::is_none")]
    linked_reference_status: Option<GroupReferenceStatus>,
    byte_len: u64,
    inflated_offset: u64,
}

impl Serialize for ParasolidGroupRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        GroupRef {
            id: &self.id,
            stream_ordinal: self.origin.stream_ordinal(),
            stream_kind: self.origin.stream_kind(),
            partition_stream_ordinal: self.origin.partition_stream_ordinal(),
            xmt: self.xmt,
            node_id: self.node_id,
            references: &self.references,
            selector: self.selector,
            linked_reference_status: self.origin.linked_reference_status(),
            byte_len: self.byte_len,
            inflated_offset: self.inflated_offset,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<ParasolidGroupRecord> for GroupWire {
    fn from(value: ParasolidGroupRecord) -> Self {
        Self {
            id: value.id,
            stream_ordinal: value.origin.stream_ordinal(),
            stream_kind: value.origin.stream_kind(),
            partition_stream_ordinal: value.origin.partition_stream_ordinal(),
            xmt: value.xmt,
            node_id: value.node_id,
            references: value.references.to_vec(),
            selector: value.selector,
            linked_reference_status: value.origin.linked_reference_status(),
            byte_len: value.byte_len,
            inflated_offset: value.inflated_offset,
        }
    }
}
impl TryFrom<GroupWire> for ParasolidGroupRecord {
    type Error = &'static str;
    fn try_from(wire: GroupWire) -> Result<Self, Self::Error> {
        let origin = match wire.stream_kind {
            StreamKind::Partition if wire.partition_stream_ordinal == Some(wire.stream_ordinal) => {
                if wire.linked_reference_status.is_some() {
                    return Err("linked_reference_status: absent in primary GROUP records");
                }
                GroupOrigin::Partition {
                    stream_ordinal: wire.stream_ordinal,
                }
            }
            StreamKind::Deltas => GroupOrigin::Deltas {
                stream_ordinal: wire.stream_ordinal,
                partition_stream_ordinal: wire.partition_stream_ordinal,
                linked_reference_status: wire
                    .linked_reference_status
                    .ok_or("linked_reference_status: required for Deltas GROUP records")?,
            },
            StreamKind::Partition => {
                return Err("partition_stream_ordinal: a partition owns its stream namespace")
            }
            StreamKind::Plain | StreamKind::Preview => {
                return Err("stream_kind: GROUP requires partition or deltas")
            }
        };
        let references = wire
            .references
            .try_into()
            .map_err(|_| "references: GROUP requires five entries")?;
        Ok(Self {
            id: wire.id,
            origin,
            xmt: wire.xmt,
            node_id: wire.node_id,
            references,
            selector: wire.selector,
            byte_len: wire.byte_len,
            inflated_offset: wire.inflated_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ParasolidGroupRecord, GROUP_RECORD_CLONE_COUNT};
    use super::GroupWire;

    #[test]
    fn group_wire_preserves_scopes_and_rejects_invalid_controls() {
        for scope in [
            r#""stream_ordinal":4,"stream_kind":"partition","partition_stream_ordinal":4"#,
            r#""stream_ordinal":5,"stream_kind":"deltas","partition_stream_ordinal":4"#,
            r#""stream_ordinal":5,"stream_kind":"deltas""#,
        ] {
            let status = if scope.contains("\"deltas\"") {
                ",\"linked_reference_status\":0"
            } else {
                ""
            };
            let json = format!(
                r#"{{"id":"group",{scope},"xmt":10,"node_id":7,"references":[3,4,5,6,7],"selector":4{status},"byte_len":20,"inflated_offset":0}}"#
            );
            let group: ParasolidGroupRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&group).unwrap(), json);
            assert_eq!(
                serde_json::to_vec(&group).unwrap(),
                serde_json::to_vec(&GroupWire::from(group.clone())).unwrap()
            );
            for selector in 1..=10u8 {
                let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
                value["selector"] = serde_json::json!(selector);
                let decoded = serde_json::from_value::<ParasolidGroupRecord>(value).unwrap();
                assert_eq!(u8::from(decoded.selector), selector);
            }
            let mut null_status: serde_json::Value = serde_json::from_str(&json).unwrap();
            null_status["linked_reference_status"] = serde_json::Value::Null;
            assert!(serde_json::from_value::<ParasolidGroupRecord>(null_status)
                .unwrap_err()
                .to_string()
                .contains("linked_reference_status"));
            if scope.contains("\"deltas\"") {
                let mut missing: serde_json::Value = serde_json::from_str(&json).unwrap();
                missing
                    .as_object_mut()
                    .unwrap()
                    .remove("linked_reference_status");
                assert!(serde_json::from_value::<ParasolidGroupRecord>(missing)
                    .unwrap_err()
                    .to_string()
                    .contains("linked_reference_status"));
            }
            for (field, invalid) in [
                ("selector", serde_json::json!(11)),
                ("linked_reference_status", serde_json::json!(2)),
                ("stream_kind", serde_json::json!("plain")),
                ("references", serde_json::json!([1, 2])),
            ] {
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire[field] = invalid;
                let error = serde_json::from_value::<ParasolidGroupRecord>(wire).unwrap_err();
                assert!(error.to_string().contains(field), "{error}");
            }
            if scope.contains("partition\"") {
                let mut with_status: serde_json::Value = serde_json::from_str(&json).unwrap();
                with_status["linked_reference_status"] = serde_json::json!(0);
                assert!(serde_json::from_value::<ParasolidGroupRecord>(with_status)
                    .unwrap_err()
                    .to_string()
                    .contains("linked_reference_status"));
                let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
                wire["partition_stream_ordinal"] = serde_json::json!(3);
                assert!(serde_json::from_value::<ParasolidGroupRecord>(wire)
                    .unwrap_err()
                    .to_string()
                    .contains("partition_stream_ordinal"));
            }
        }
    }

    #[test]
    fn group_record_retained_limit_refuses_before_clone() {
        let json = r#"{"id":"nx:s4:group#10","stream_ordinal":4,"stream_kind":"partition","partition_stream_ordinal":4,"xmt":10,"node_id":7,"references":[3,4,5,6,7],"selector":4,"byte_len":20,"inflated_offset":0}"#;
        let record: ParasolidGroupRecord = serde_json::from_str(json).unwrap();
        let expected: serde_json::Value = serde_json::from_str(json).unwrap();
        GROUP_RECORD_CLONE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(&record, expected);
        GROUP_RECORD_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_partition_stream_ordinal,
    u32,
    "partition_stream_ordinal"
);
cadmpeg_core::named_optional_field!(
    deserialize_linked_reference_status,
    GroupReferenceStatus,
    "linked_reference_status"
);
