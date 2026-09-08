// SPDX-License-Identifier: Apache-2.0
//! GROUP member families with their required node identities.

use super::ParasolidGroupMember;
use crate::deltas::record_family::RecordFamily;
use crate::topology::Graph;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupNodeFamily {
    Body,
    Shell,
    Face,
    Loop,
    Edge,
    Vertex,
    Region,
}
impl GroupNodeFamily {
    fn kind(self) -> crate::framing::node_kind::NodeKind {
        match self {
            Self::Body => crate::framing::node_kind::NodeKind::Body,
            Self::Shell => crate::framing::node_kind::NodeKind::Shell,
            Self::Face => crate::framing::node_kind::NodeKind::Face,
            Self::Loop => crate::framing::node_kind::NodeKind::Loop,
            Self::Edge => crate::framing::node_kind::NodeKind::Edge,
            Self::Vertex => crate::framing::node_kind::NodeKind::Vertex,
            Self::Region => crate::framing::node_kind::NodeKind::Region,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Body => "BODY",
            Self::Shell => "SHELL",
            Self::Face => "FACE",
            Self::Loop => "LOOP",
            Self::Edge => "EDGE",
            Self::Vertex => "VERTEX",
            Self::Region => "REGION",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupMemberTarget {
    Fin,
    Node {
        family: GroupNodeFamily,
        node_id: u32,
        current_xmt: Option<u32>,
    },
}
impl GroupMemberTarget {
    pub(super) fn from_record(record: &RecordFamily) -> Option<Self> {
        let (family, node_id) = match record {
            RecordFamily::Fin { .. } => return Some(Self::Fin),
            RecordFamily::Body { node_id, .. } => (GroupNodeFamily::Body, node_id),
            RecordFamily::Shell { node_id, .. } => (GroupNodeFamily::Shell, node_id),
            RecordFamily::Face { node_id, .. } => (GroupNodeFamily::Face, node_id),
            RecordFamily::Loop { node_id, .. } => (GroupNodeFamily::Loop, node_id),
            RecordFamily::Edge { node_id, .. } => (GroupNodeFamily::Edge, node_id),
            RecordFamily::Vertex { node_id, .. } => (GroupNodeFamily::Vertex, node_id),
            RecordFamily::Region { node_id, .. } => (GroupNodeFamily::Region, node_id),
            _ => return None,
        };
        Some(Self::Node {
            family,
            node_id: *node_id,
            current_xmt: None,
        })
    }

    pub(super) fn resolve(self, graph: &Graph, member_xmt: u32) -> Self {
        match self {
            Self::Fin => Self::Fin,
            Self::Node {
                family, node_id, ..
            } => {
                let current_xmt = graph
                    .get(family.kind(), member_xmt)
                    .filter(|node| node.node_id() == Some(node_id))
                    .map(|node| node.xmt)
                    .or_else(|| graph.unique_xmt_by_node_id(family.kind(), node_id));
                Self::Node {
                    family,
                    node_id,
                    current_xmt,
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct MemberWire {
    id: String,
    partition_stream_ordinal: u32,
    group_xmt: u32,
    group_node_id: u32,
    ordinal: u32,
    list_record_xmt: u32,
    member_xmt: u32,
    member_family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    member_node_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_member_xmt: Option<u32>,
}
impl From<ParasolidGroupMember> for MemberWire {
    fn from(value: ParasolidGroupMember) -> Self {
        let (member_family, member_node_id, current_member_xmt) = match value.target {
            GroupMemberTarget::Fin => ("FIN", None, None),
            GroupMemberTarget::Node {
                family,
                node_id,
                current_xmt,
            } => (family.name(), Some(node_id), current_xmt),
        };
        Self {
            id: value.id,
            partition_stream_ordinal: value.partition_stream_ordinal,
            group_xmt: value.group_xmt,
            group_node_id: value.group_node_id,
            ordinal: value.ordinal,
            list_record_xmt: value.list_record_xmt,
            member_xmt: value.member_xmt,
            member_family: member_family.into(),
            member_node_id,
            current_member_xmt,
        }
    }
}
impl TryFrom<MemberWire> for ParasolidGroupMember {
    type Error = &'static str;
    fn try_from(wire: MemberWire) -> Result<Self, Self::Error> {
        let target = if wire.member_family == "FIN" {
            if wire.member_node_id.is_some() || wire.current_member_xmt.is_some() {
                return Err("member_node_id/current_member_xmt: FIN carries neither identity");
            }
            GroupMemberTarget::Fin
        } else {
            let family = match wire.member_family.as_str() {
                "BODY" => GroupNodeFamily::Body,
                "SHELL" => GroupNodeFamily::Shell,
                "FACE" => GroupNodeFamily::Face,
                "LOOP" => GroupNodeFamily::Loop,
                "EDGE" => GroupNodeFamily::Edge,
                "VERTEX" => GroupNodeFamily::Vertex,
                "REGION" => GroupNodeFamily::Region,
                _ => return Err("member_family: unsupported GROUP member family"),
            };
            GroupMemberTarget::Node {
                family,
                node_id: wire
                    .member_node_id
                    .ok_or("member_node_id: required by member_family")?,
                current_xmt: wire.current_member_xmt,
            }
        };
        Ok(Self {
            id: wire.id,
            partition_stream_ordinal: wire.partition_stream_ordinal,
            group_xmt: wire.group_xmt,
            group_node_id: wire.group_node_id,
            ordinal: wire.ordinal,
            list_record_xmt: wire.list_record_xmt,
            member_xmt: wire.member_xmt,
            target,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_owns_the_node_identity_on_the_wire() {
        for fields in [
            r#""member_family":"FIN""#,
            r#""member_family":"FACE","member_node_id":50,"current_member_xmt":100"#,
        ] {
            let json = format!(
                r#"{{"id":"member","partition_stream_ordinal":4,"group_xmt":10,"group_node_id":7,"ordinal":0,"list_record_xmt":20,"member_xmt":30,{fields}}}"#
            );
            let member: ParasolidGroupMember = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&member).unwrap(), json);
            let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
            wire["member_node_id"] = if fields.contains("FIN") {
                serde_json::json!(50)
            } else {
                serde_json::Value::Null
            };
            assert!(serde_json::from_value::<ParasolidGroupMember>(wire)
                .unwrap_err()
                .to_string()
                .contains("member_node_id"));
        }
    }
}
