// SPDX-License-Identifier: Apache-2.0
//! Consolidated edge nodes and their vertex-identity arena join.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::native::edge_definition::CatiaConsolidatedEdgeDefinition;
use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth};

use super::{
    CatiaAllocationReferenceEncoding, CatiaConsolidatedAnalyticCircleBinding,
    CatiaConsolidatedClass25Descriptor, CatiaConsolidatedEdgeUses, CatiaConsolidatedVertexIdentity,
};

/// One structurally complete width-coded class-`0x5e` edge node.
#[derive(Debug, Clone, PartialEq)]
pub struct CatiaConsolidatedEdgeNode {
    /// Stable native-record identity.
    pub id: String,
    /// Record byte offset.
    pub byte_offset: u64,
    /// Zero-based bounded record-source ordinal.
    pub source_index: usize,
    /// Header-token width in bytes.
    pub width: ConsolidatedFrameWidth,
    /// Independent framing flag.
    pub flag: ConsolidatedFrameFlag,
    /// Width-coded header token.
    pub header_token: u32,
    /// Owning compact class-`0x62` packet and frame ordinal.
    pub allocation: Option<(String, u32)>,
    /// Allocation-local curve-support reference.
    pub curve_ref: u32,
    /// Middle reference pair. These are endpoint addresses only when an
    /// allocation walk or complete edge-use run proves that layout.
    pub vertex_refs: [u32; 2],
    /// Resolved structural endpoint records in edge direction.
    pub endpoint_records: Option<[u64; 2]>,
    /// Final reference pair. Complete edge-use runs interpret these as
    /// allocation-local side selectors; other layouts retain them untyped.
    pub parameter_selectors: [u32; 2],
    /// Wire addressing forms of curve, vertex, and parameter references.
    pub reference_encodings: [CatiaAllocationReferenceEncoding; 5],
    /// Decoded value of the one-byte terminal allocation reference.
    pub terminal_value: u32,
    /// Wire addressing form of the terminal allocation reference.
    pub terminal_encoding: CatiaAllocationReferenceEncoding,
    /// Terminal layout byte.
    pub tail: u8,
    /// Adjacent class-`0x23..=0x25` edge-definition frame.
    pub definition: Option<CatiaConsolidatedEdgeDefinition>,
    /// Adjacent oriented uses whose references close on this edge node.
    pub uses: Option<CatiaConsolidatedEdgeUses>,
    /// Analytic circle carrier structurally bound by an adjacent six-record run.
    pub analytic_circle: Option<CatiaConsolidatedAnalyticCircleBinding>,
    /// Typed class-`0x18` descriptor bound to a class-`0x25` edge run.
    pub class25_descriptor: Option<CatiaConsolidatedClass25Descriptor>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct CatiaConsolidatedEdgeNodeWire {
    id: String,
    byte_offset: u64,
    source_index: usize,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    header_token: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allocation_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allocation_ordinal: Option<u32>,
    curve_ref: u32,
    vertex_refs: [u32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endpoint_records: Option<[u64; 2]>,
    vertices: [String; 2],
    parameter_selectors: [u32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reference_encodings: Option<[CatiaAllocationReferenceEncoding; 5]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminal_value: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminal_encoding: Option<CatiaAllocationReferenceEncoding>,
    tail: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    definition: Option<CatiaConsolidatedEdgeDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uses: Option<CatiaConsolidatedEdgeUses>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    analytic_circle: Option<CatiaConsolidatedAnalyticCircleBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class25_descriptor: Option<CatiaConsolidatedClass25Descriptor>,
}

impl CatiaConsolidatedEdgeNodeWire {
    fn from_node(value: CatiaConsolidatedEdgeNode, vertices: [String; 2]) -> Self {
        let (allocation_owner, allocation_ordinal) = match value.allocation {
            Some((owner, ordinal)) => (Some(owner), Some(ordinal)),
            None => (None, None),
        };
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            source_index: value.source_index,
            width: value.width,
            flag: value.flag,
            header_token: value.header_token,
            allocation_owner,
            allocation_ordinal,
            curve_ref: value.curve_ref,
            vertex_refs: value.vertex_refs,
            endpoint_records: value.endpoint_records,
            vertices,
            parameter_selectors: value.parameter_selectors,
            reference_encodings: Some(value.reference_encodings),
            terminal_value: Some(value.terminal_value),
            terminal_encoding: Some(value.terminal_encoding),
            tail: value.tail,
            definition: value.definition,
            uses: value.uses,
            analytic_circle: value.analytic_circle,
            class25_descriptor: value.class25_descriptor,
        }
    }
}

impl TryFrom<CatiaConsolidatedEdgeNodeWire> for CatiaConsolidatedEdgeNode {
    type Error = String;

    fn try_from(wire: CatiaConsolidatedEdgeNodeWire) -> Result<Self, Self::Error> {
        let allocation = match (wire.allocation_owner, wire.allocation_ordinal) {
            (Some(owner), Some(ordinal)) => Some((owner, ordinal)),
            (None, None) => None,
            _ => {
                return Err(
                    "consolidated edge node allocation owner and ordinal are both-or-neither"
                        .to_owned(),
                );
            }
        };
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            source_index: wire.source_index,
            width: wire.width,
            flag: wire.flag,
            header_token: wire.header_token,
            allocation,
            curve_ref: wire.curve_ref,
            vertex_refs: wire.vertex_refs,
            endpoint_records: wire.endpoint_records,
            parameter_selectors: wire.parameter_selectors,
            reference_encodings: wire
                .reference_encodings
                .ok_or_else(|| "consolidated edge node requires reference_encodings".to_owned())?,
            terminal_value: wire
                .terminal_value
                .ok_or_else(|| "consolidated edge node requires terminal_value".to_owned())?,
            terminal_encoding: wire
                .terminal_encoding
                .ok_or_else(|| "consolidated edge node requires terminal_encoding".to_owned())?,
            tail: wire.tail,
            definition: wire.definition,
            uses: wire.uses,
            analytic_circle: wire.analytic_circle,
            class25_descriptor: wire.class25_descriptor,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum IdentityKey {
    EndpointRecord(u64),
    Unresolved(usize, Option<String>, u32),
}

fn node_identity_key(node: &CatiaConsolidatedEdgeNode, endpoint: usize) -> IdentityKey {
    node.endpoint_records.map_or_else(
        || {
            IdentityKey::Unresolved(
                node.source_index,
                node.allocation.as_ref().map(|(owner, _)| owner.clone()),
                node.vertex_refs[endpoint],
            )
        },
        |records| IdentityKey::EndpointRecord(records[endpoint]),
    )
}

fn identity_index(identities: &[CatiaConsolidatedVertexIdentity]) -> HashMap<IdentityKey, &str> {
    identities
        .iter()
        .map(|identity| {
            let key = identity.endpoint_record.map_or_else(
                || {
                    IdentityKey::Unresolved(
                        identity.source_index,
                        identity.allocation_owner.clone(),
                        identity.identity,
                    )
                },
                IdentityKey::EndpointRecord,
            );
            (key, identity.id.as_str())
        })
        .collect()
}

fn joined_vertices<'a>(
    node: &CatiaConsolidatedEdgeNode,
    index: &HashMap<IdentityKey, &'a str>,
) -> [&'a str; 2] {
    if node.endpoint_records.is_none() && node.uses.is_none() {
        return ["", ""];
    }
    std::array::from_fn(|endpoint| {
        index
            .get(&node_identity_key(node, endpoint))
            .copied()
            .unwrap_or("")
    })
}

pub(super) fn edge_node_wires(
    nodes: Vec<CatiaConsolidatedEdgeNode>,
    identities: &[CatiaConsolidatedVertexIdentity],
) -> Vec<CatiaConsolidatedEdgeNodeWire> {
    let index = identity_index(identities);
    nodes
        .into_iter()
        .map(|node| {
            let vertices = joined_vertices(&node, &index).map(str::to_owned);
            CatiaConsolidatedEdgeNodeWire::from_node(node, vertices)
        })
        .collect()
}

pub(super) fn load_edge_nodes(
    wires: Vec<CatiaConsolidatedEdgeNodeWire>,
    identities: &[CatiaConsolidatedVertexIdentity],
) -> Result<Vec<CatiaConsolidatedEdgeNode>, String> {
    let index = identity_index(identities);
    wires
        .into_iter()
        .map(|mut wire| {
            let vertices = std::mem::take(&mut wire.vertices);
            let node = CatiaConsolidatedEdgeNode::try_from(wire)?;
            if vertices.each_ref().map(String::as_str) != joined_vertices(&node, &index) {
                return Err(format!(
                    "consolidated edge node `{}` vertices differ from the vertex-identity arena",
                    node.id
                ));
            }
            Ok(node)
        })
        .collect()
}

#[cfg(test)]
impl super::CatiaNative {
    pub(super) fn vertex_identity_ids(&self, node: &CatiaConsolidatedEdgeNode) -> [&str; 2] {
        joined_vertices(node, &identity_index(&self.consolidated_vertex_identities))
    }
}

pub(super) fn consolidated_vertex_identities(
    nodes: &[CatiaConsolidatedEdgeNode],
) -> Vec<CatiaConsolidatedVertexIdentity> {
    let mut identities = Vec::<CatiaConsolidatedVertexIdentity>::new();
    let mut identity_indices = HashMap::<IdentityKey, usize>::new();
    for node in nodes {
        if node.endpoint_records.is_none() && node.uses.is_none() {
            continue;
        }
        for (endpoint, identity) in node.vertex_refs.into_iter().enumerate() {
            let endpoint_record = node.endpoint_records.map(|records| records[endpoint]);
            let key = node_identity_key(node, endpoint);
            let index = *identity_indices.entry(key).or_insert_with(|| {
                let index = identities.len();
                identities.push(CatiaConsolidatedVertexIdentity {
                    id: format!("catia:consolidated:vertex-identity#{index}"),
                    identity,
                    source_index: node.source_index,
                    endpoint_record,
                    reference_values: vec![identity],
                    allocation_owner: node.allocation.as_ref().map(|(owner, _)| owner.clone()),
                    incident_edge_nodes: Vec::new(),
                });
                index
            });
            let vertex = &mut identities[index];
            if !vertex.reference_values.contains(&identity) {
                vertex.reference_values.push(identity);
            }
            if vertex.incident_edge_nodes.last() != Some(&node.id) {
                vertex.incident_edge_nodes.push(node.id.clone());
            }
        }
    }
    identities
}

#[cfg(test)]
mod tests {
    use crate::native::CatiaNative;
    use crate::test_support::a5_native_edge_run_stream;

    #[test]
    fn vertex_wire_join_preserves_ids_and_rejects_conflicting_ids() {
        let native = CatiaNative::decode(&a5_native_edge_run_stream(6, 139, 142));
        let mut wire = serde_json::to_value(&native).expect("serialize native arenas");
        assert_eq!(
            wire["consolidated_edge_nodes"][0]["vertices"],
            serde_json::json!([
                "catia:consolidated:vertex-identity#0",
                "catia:consolidated:vertex-identity#1"
            ])
        );
        assert_eq!(
            serde_json::from_value::<CatiaNative>(wire.clone()).expect("load joined vertices"),
            native
        );
        wire["consolidated_edge_nodes"][0]["vertices"][0] =
            serde_json::json!("catia:consolidated:vertex-identity#1");
        let error = serde_json::from_value::<CatiaNative>(wire)
            .expect_err("reject conflicting vertex join");
        assert!(error.to_string().contains("vertices differ"));
    }
}
