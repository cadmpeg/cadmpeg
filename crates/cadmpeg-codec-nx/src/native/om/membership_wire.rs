// SPDX-License-Identifier: Apache-2.0
//! Wire words derived from RMFastLoad membership values and counts.

use super::{RmFastLoadObjectId, RmFastLoadObjectIdTable};
use crate::container::membership::ObjectIdMembers;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct TableWire {
    id: String,
    members: Vec<String>,
    raw_count: [u8; 4],
    source_entry: String,
    registry_source_offset: u64,
    source_offset: u64,
}

impl From<RmFastLoadObjectIdTable> for TableWire {
    fn from(value: RmFastLoadObjectIdTable) -> Self {
        let raw_count = value.raw_count();
        Self {
            id: value.id, members: value.members.into_vec(), raw_count,
            source_entry: value.source_entry,
            registry_source_offset: value.registry_source_offset,
            source_offset: value.source_offset,
        }
    }
}
impl TryFrom<TableWire> for RmFastLoadObjectIdTable {
    type Error = &'static str;
    fn try_from(wire: TableWire) -> Result<Self, Self::Error> {
        let members = ObjectIdMembers::new(wire.members)?;
        if wire.raw_count != members.count().to_le_bytes() {
            return Err("raw_count: does not encode the members length");
        }
        Ok(Self {
            id: wire.id, members, source_entry: wire.source_entry,
            registry_source_offset: wire.registry_source_offset,
            source_offset: wire.source_offset,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct MemberWire {
    id: String,
    table: String,
    ordinal: u32,
    value: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stable_identity: Option<String>,
    raw: [u8; 4],
    source_offset: u64,
}
impl From<RmFastLoadObjectId> for MemberWire {
    fn from(value: RmFastLoadObjectId) -> Self {
        let raw = value.raw();
        Self {
            id: value.id, table: value.table, ordinal: value.ordinal,
            value: value.value, stable_identity: value.stable_identity, raw,
            source_offset: value.source_offset,
        }
    }
}
impl TryFrom<MemberWire> for RmFastLoadObjectId {
    type Error = &'static str;
    fn try_from(wire: MemberWire) -> Result<Self, Self::Error> {
        if wire.raw != wire.value.to_le_bytes() {
            return Err("raw: does not encode value");
        }
        Ok(Self {
            id: wire.id, table: wire.table, ordinal: wire.ordinal,
            value: wire.value, stable_identity: wire.stable_identity,
            source_offset: wire.source_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_keeps_wire_order_and_rejects_inconsistent_words() {
        let json = r#"{"id":"table","members":["a","b"],"raw_count":[2,0,0,0],"source_entry":"entry","registry_source_offset":10,"source_offset":20}"#;
        let table: RmFastLoadObjectIdTable = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&table).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["raw_count"] = serde_json::json!([1,0,0,0]);
        assert!(serde_json::from_value::<RmFastLoadObjectIdTable>(wire).unwrap_err().to_string().contains("raw_count"));

        let json = r#"{"id":"a","table":"table","ordinal":0,"value":4294967295,"raw":[255,255,255,255],"source_offset":24}"#;
        let member: RmFastLoadObjectId = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&member).unwrap(), json);
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["raw"] = serde_json::json!([0,0,0,0]);
        assert!(serde_json::from_value::<RmFastLoadObjectId>(wire).unwrap_err().to_string().contains("raw"));
    }
}
