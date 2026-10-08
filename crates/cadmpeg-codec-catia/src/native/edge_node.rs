// SPDX-License-Identifier: Apache-2.0
//! Consolidated edge nodes and their vertex-identity arena join.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::native::edge_definition::CatiaConsolidatedEdgeDefinition;
use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth, WidthCodedToken};

use super::{
    CatiaAllocationReferenceEncoding, CatiaConsolidatedAnalyticCircleBinding,
    CatiaConsolidatedClass25Descriptor, CatiaConsolidatedEdgeUses, CatiaConsolidatedVertexIdentity,
};

/// One structurally complete width-coded class-`0x5e` edge node.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CatiaConsolidatedEdgeNode {
    /// Stable native-record identity.
    pub(super) id: String,
    /// Record byte offset.
    pub(super) byte_offset: u64,
    /// Zero-based bounded record-source ordinal.
    pub(super) source_index: usize,
    /// Header token with its checked encoded width.
    pub(super) token: WidthCodedToken,
    /// Independent framing flag.
    pub(super) flag: ConsolidatedFrameFlag,
    /// Owning compact class-`0x62` packet and frame ordinal.
    pub(super) allocation: Option<(String, u32)>,
    /// Allocation-local curve-support reference.
    pub(super) curve_ref: u32,
    /// Middle reference pair. These are endpoint addresses only when an
    /// allocation walk or complete edge-use run proves that layout.
    pub(super) vertex_refs: [u32; 2],
    /// Resolved structural endpoint records in edge direction.
    pub(super) endpoint_records: Option<[u64; 2]>,
    /// Final reference pair. Complete edge-use runs interpret these as
    /// allocation-local side selectors; other layouts retain them untyped.
    pub(super) parameter_selectors: [u32; 2],
    /// Wire addressing forms of curve, vertex, and parameter references.
    pub(super) reference_encodings: [CatiaAllocationReferenceEncoding; 5],
    /// Decoded value of the one-byte terminal allocation reference.
    pub(super) terminal_value: u32,
    /// Wire addressing form of the terminal allocation reference.
    pub(super) terminal_encoding: CatiaAllocationReferenceEncoding,
    /// Terminal layout byte.
    pub(super) tail: u8,
    /// Adjacent class-`0x23..=0x25` edge-definition frame.
    pub(crate) definition: Option<CatiaConsolidatedEdgeDefinition>,
    /// Adjacent oriented uses whose references close on this edge node.
    pub(super) uses: Option<CatiaConsolidatedEdgeUses>,
    /// Analytic circle carrier structurally bound by an adjacent six-record run.
    pub(crate) analytic_circle: Option<CatiaConsolidatedAnalyticCircleBinding>,
    /// Typed class-`0x18` descriptor bound to a class-`0x25` edge run.
    pub(crate) class25_descriptor: Option<CatiaConsolidatedClass25Descriptor>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct CatiaConsolidatedEdgeNodeWire {
    id: String,
    byte_offset: u64,
    source_index: usize,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    header_token: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_allocation_owner"
    )]
    allocation_owner: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_allocation_ordinal"
    )]
    allocation_ordinal: Option<u32>,
    curve_ref: u32,
    vertex_refs: [u32; 2],
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_endpoint_records"
    )]
    endpoint_records: Option<[u64; 2]>,
    vertices: [String; 2],
    parameter_selectors: [u32; 2],
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reference_encodings"
    )]
    reference_encodings: Option<[CatiaAllocationReferenceEncoding; 5]>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_terminal_value"
    )]
    terminal_value: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_terminal_encoding"
    )]
    terminal_encoding: Option<CatiaAllocationReferenceEncoding>,
    tail: u8,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_definition"
    )]
    definition: Option<CatiaConsolidatedEdgeDefinition>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_uses"
    )]
    uses: Option<CatiaConsolidatedEdgeUses>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_analytic_circle"
    )]
    analytic_circle: Option<CatiaConsolidatedAnalyticCircleBinding>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_class25_descriptor"
    )]
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
            width: value.token.width(),
            flag: value.flag,
            header_token: value.token.value(),
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
            token: WidthCodedToken::new(wire.width, wire.header_token)?,
            flag: wire.flag,
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
enum IdentityKey<'owner> {
    EndpointRecord(u64),
    Unresolved(usize, Option<&'owner str>, u32),
}

impl cadmpeg_core::decode::cost::DecodeCost for IdentityKey<'_> {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::EndpointRecord(record) => (1_u8, record).decode_cost(ctx, operation),
            Self::Unresolved(index, owner, vertex) => {
                (1_u8, index, owner, vertex).decode_cost(ctx, operation)
            }
        }
    }
}

fn node_identity_key(node: &CatiaConsolidatedEdgeNode, endpoint: usize) -> IdentityKey<'_> {
    node.endpoint_records.map_or_else(
        || {
            IdentityKey::Unresolved(
                node.source_index,
                node.allocation.as_ref().map(|(owner, _)| owner.as_str()),
                node.vertex_refs[endpoint],
            )
        },
        |records| IdentityKey::EndpointRecord(records[endpoint]),
    )
}

fn identity_index(
    identities: &[CatiaConsolidatedVertexIdentity],
) -> HashMap<IdentityKey<'_>, &str> {
    identities
        .iter()
        .map(|identity| {
            let key = identity.endpoint_record.map_or_else(
                || {
                    IdentityKey::Unresolved(
                        identity.source_index,
                        identity.allocation_owner.as_deref(),
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
    index: &HashMap<IdentityKey<'_>, &'a str>,
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

fn identity_index_charged<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    identities: &'a [CatiaConsolidatedVertexIdentity],
) -> Result<HashMap<IdentityKey<'a>, &'a str>, cadmpeg_core::CodecError> {
    let mut index = HashMap::new();
    for identity in ctx.admit_iter(identities, "catia_native_edge_wire_identity_visits")? {
        let key = if let Some(record) = identity.endpoint_record {
            IdentityKey::EndpointRecord(record)
        } else {
            IdentityKey::Unresolved(
                identity.source_index,
                identity.allocation_owner.as_deref(),
                identity.identity,
            )
        };
        ctx.insert_hash_map(
            &mut index,
            key,
            identity.id.as_str(),
            "catia_native_edge_wire_index",
        )?;
    }
    Ok(index)
}

fn joined_vertex_charged<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    node: &CatiaConsolidatedEdgeNode,
    endpoint: usize,
    index: &HashMap<IdentityKey<'_>, &'a str>,
) -> Result<&'a str, cadmpeg_core::CodecError> {
    if node.endpoint_records.is_none() && node.uses.is_none() {
        return Ok("");
    }
    let key = node_identity_key(node, endpoint);
    Ok(ctx
        .get_hash_map(index, &key, "catia_native_edge_wire_lookup")?
        .copied()
        .unwrap_or(""))
}

pub(super) fn edge_node_wires_charged(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nodes: Vec<CatiaConsolidatedEdgeNode>,
    identities: &[CatiaConsolidatedVertexIdentity],
) -> Result<Vec<CatiaConsolidatedEdgeNodeWire>, cadmpeg_core::CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "CATIA edge wire identity lookup")?;
    let index = index_storage.with_storage(|| identity_index_charged(ctx, identities))?;
    ctx.try_collect_vec(
        nodes
            .into_iter()
            .map(|node| -> Result<_, cadmpeg_core::CodecError> {
                let vertices = [
                    ctx.copy_retained_text(
                        joined_vertex_charged(ctx, &node, 0, &index)?,
                        "catia_native_edge_wire_vertex_id",
                    )?,
                    ctx.copy_retained_text(
                        joined_vertex_charged(ctx, &node, 1, &index)?,
                        "catia_native_edge_wire_vertex_id",
                    )?,
                ];
                Ok(CatiaConsolidatedEdgeNodeWire::from_node(node, vertices))
            }),
        "catia_native_edge_wires",
    )
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nodes: &[CatiaConsolidatedEdgeNode],
) -> Result<Vec<CatiaConsolidatedVertexIdentity>, cadmpeg_core::CodecError> {
    let mut identities = Vec::<CatiaConsolidatedVertexIdentity>::new();
    let mut identity_storage = ctx.reserve_scoped(0, "CATIA vertex identity lookup")?;
    let mut identity_indices = HashMap::<IdentityKey, usize>::new();
    let mut reference_members = std::collections::HashSet::new();
    for node in ctx.admit_iter(nodes, "catia_native_vertex_identity_nodes")? {
        if node.endpoint_records.is_none() && node.uses.is_none() {
            continue;
        }
        for (endpoint, identity) in node.vertex_refs.into_iter().enumerate() {
            let endpoint_record = node.endpoint_records.map(|records| records[endpoint]);
            let key = node_identity_key(node, endpoint);
            let index = if let Some(&index) = ctx.get_hash_map(
                &identity_indices,
                &key,
                "catia_native_vertex_identity_lookup",
            )? {
                index
            } else {
                let index = identities.len();
                let id = ctx.format_retained(
                    format_args!("catia:consolidated:vertex-identity#{index:00}"),
                    "catia_native_vertex_identity_id",
                )?;
                let mut reference_values = Vec::new();
                ctx.push_vec(
                    &mut reference_values,
                    identity,
                    "catia_native_vertex_identity_references",
                )?;
                let allocation_owner = node
                    .allocation
                    .as_ref()
                    .map(|(owner, _)| {
                        ctx.copy_retained_text(owner, "catia_native_vertex_identity_owner")
                    })
                    .transpose()?;
                ctx.push_vec(
                    &mut identities,
                    CatiaConsolidatedVertexIdentity {
                        id,
                        identity,
                        source_index: node.source_index,
                        endpoint_record,
                        reference_values,
                        allocation_owner,
                        incident_edge_nodes: Vec::new(),
                    },
                    "catia_native_vertex_identities",
                )?;
                identity_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut identity_indices,
                        key,
                        index,
                        "catia_native_vertex_identity_index",
                    )
                })?;
                identity_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut reference_members,
                        (index, identity),
                        "catia_native_vertex_identity_reference_checks",
                    )
                })?;
                index
            };
            let vertex = &mut identities[index];
            if identity_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut reference_members,
                    (index, identity),
                    "catia_native_vertex_identity_reference_checks",
                )
            })? {
                ctx.push_vec(
                    &mut vertex.reference_values,
                    identity,
                    "catia_native_vertex_identity_references",
                )?;
            }
            let repeated_edge = match vertex.incident_edge_nodes.last() {
                Some(last) => ctx.equal_bytes(
                    last.as_bytes(),
                    node.id.as_bytes(),
                    "catia_native_vertex_incident_edge_checks",
                )?,
                None => false,
            };
            if !repeated_edge {
                let edge_id =
                    ctx.copy_retained_text(&node.id, "catia_native_vertex_incident_edge_id")?;
                ctx.push_vec(
                    &mut vertex.incident_edge_nodes,
                    edge_id,
                    "catia_native_vertex_incident_edges",
                )?;
            }
        }
    }
    Ok(identities)
}

#[cfg(test)]
mod tests {
    use crate::native::CatiaNative;
    use crate::test_support::test_a5_bound::a5_native_edge_run_stream;
    use cadmpeg_test_support::refusal::{refusal, states_the_key};

    #[test]
    fn native_edge_node_rejects_a_token_wider_than_its_declared_width() {
        let native = CatiaNative::decode(&a5_native_edge_run_stream(6, 139, 142));
        let mut wire = serde_json::to_value(&native).expect("native wire")
            ["consolidated_edge_nodes"][0]
            .clone();
        wire["width"] = serde_json::json!(1);
        wire["header_token"] = serde_json::json!(256);
        let wire = serde_json::from_value::<super::CatiaConsolidatedEdgeNodeWire>(wire)
            .expect("wire shape");
        assert!(super::CatiaConsolidatedEdgeNode::try_from(wire)
            .expect_err("token exceeds width")
            .contains("does not fit"));
    }

    #[test]
    fn identity_lookup_keys_borrow_allocation_names() {
        let native = CatiaNative::decode(&a5_native_edge_run_stream(6, 139, 142));
        let owner = "allocation-owner".to_owned();
        let mut node = native.consolidated_edge_nodes[0].clone();
        node.endpoint_records = None;
        node.allocation = Some((owner, 0));
        let key = super::node_identity_key(&node, 0);
        let super::IdentityKey::Unresolved(_, Some(key_owner), _) = key else {
            panic!("borrowed unresolved identity required")
        };
        assert_eq!(
            key_owner.as_ptr(),
            node.allocation.as_ref().expect("allocation").0.as_ptr()
        );
        let identities = [super::CatiaConsolidatedVertexIdentity {
            id: String::new(),
            identity: node.vertex_refs[0],
            source_index: node.source_index,
            endpoint_record: None,
            allocation_owner: Some(key_owner.to_owned()),
            reference_values: Vec::new(),
            incident_edge_nodes: Vec::new(),
        }];
        crate::test_support::with_retained_limit(0, |ctx| {
            let mut index_storage = ctx
                .reserve_scoped(0, "CATIA borrowed identity lookup")
                .expect("lookup storage");
            let index = index_storage
                .with_storage(|| super::identity_index_charged(ctx, &identities))
                .expect("borrowed index owners");
            assert_eq!(
                super::joined_vertex_charged(ctx, &node, 0, &index).expect("borrowed lookup owner"),
                ""
            );
        });
    }

    #[test]
    fn native_edge_wire_projection_refuses_before_identity_index_growth() {
        let native = CatiaNative::decode(&a5_native_edge_run_stream(6, 139, 142));
        let service = crate::test_support::with_service_context(|ctx| {
            super::edge_node_wires_charged(
                ctx,
                native.consolidated_edge_nodes.clone(),
                &native.consolidated_vertex_identities,
            )
        })
        .expect("service edge-wire budget");
        let original = super::edge_node_wires(
            native.consolidated_edge_nodes.clone(),
            &native.consolidated_vertex_identities,
        );
        assert_eq!(
            serde_json::to_value(service).expect("serialize charged edge wires"),
            serde_json::to_value(original).expect("serialize original edge wires"),
        );
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            super::edge_node_wires_charged(
                ctx,
                native.consolidated_edge_nodes.clone(),
                &native.consolidated_vertex_identities,
            )
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_native_edge_wire_index")
        );
    }

    #[test]
    fn repeated_vertex_identity_keys_do_not_accumulate_retained_owner_copies() {
        let native = CatiaNative::decode(&a5_native_edge_run_stream(6, 139, 142));
        let mut node = native.consolidated_edge_nodes[0].clone();
        node.endpoint_records = None;
        node.allocation = Some(("allocation-owner".to_owned(), 0));
        assert!(node.uses.is_some());
        let expected = crate::test_support::with_service_context(|ctx| {
            super::consolidated_vertex_identities(ctx, std::slice::from_ref(&node))
        })
        .expect("single node identities");
        let retained = expected
            .iter()
            .map(|identity| {
                identity.id.len()
                    + identity.allocation_owner.as_ref().map_or(0, String::len)
                    + identity
                        .incident_edge_nodes
                        .iter()
                        .map(String::len)
                        .sum::<usize>()
            })
            .sum::<usize>();
        let retained = retained
            + 4 * std::mem::size_of::<super::CatiaConsolidatedVertexIdentity>()
            + expected.len() * 4 * (std::mem::size_of::<u32>() + std::mem::size_of::<String>());
        let nodes = (0..64).map(|_| node.clone()).collect::<Vec<_>>();
        crate::test_support::with_retained_limit(
            u64::try_from(retained).expect("identity retained bytes"),
            |ctx| {
                assert_eq!(
                    super::consolidated_vertex_identities(ctx, &nodes)
                        .expect("arena names and output vector slots are retained"),
                    expected
                );
            },
        );
    }

    #[test]
    fn native_vertex_identities_refuse_before_nested_growth() {
        let native = CatiaNative::decode(&a5_native_edge_run_stream(6, 139, 142));
        assert!(!native.consolidated_edge_nodes.is_empty());
        let service = crate::test_support::with_service_context(|ctx| {
            super::consolidated_vertex_identities(ctx, &native.consolidated_edge_nodes)
        })
        .expect("service vertex identity budget");
        assert_eq!(service, native.consolidated_vertex_identities);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            super::consolidated_vertex_identities(ctx, &native.consolidated_edge_nodes)
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_native_vertex_identity_references")
        );
    }

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

    /// A top-level optional key on an edge node names itself in its refusal.
    #[test]
    fn a_top_level_edge_node_key_names_itself_in_its_refusal() {
        for key in [
            "allocation_owner",
            "allocation_ordinal",
            "endpoint_records",
            "terminal_value",
            "terminal_encoding",
        ] {
            states_the_key(key, &refusal::<super::CatiaConsolidatedEdgeNodeWire>(key));
        }
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_allocation_owner, String, "allocation_owner");
cadmpeg_core::named_optional_field!(deserialize_allocation_ordinal, u32, "allocation_ordinal");
cadmpeg_core::named_optional_field!(deserialize_endpoint_records, [u64; 2], "endpoint_records");
cadmpeg_core::named_optional_field!(
    deserialize_reference_encodings,
    [CatiaAllocationReferenceEncoding; 5],
    "reference_encodings"
);
cadmpeg_core::named_optional_field!(deserialize_terminal_value, u32, "terminal_value");
cadmpeg_core::named_optional_field!(
    deserialize_terminal_encoding,
    CatiaAllocationReferenceEncoding,
    "terminal_encoding"
);
cadmpeg_core::named_optional_field!(
    deserialize_definition,
    CatiaConsolidatedEdgeDefinition,
    "definition"
);
cadmpeg_core::named_optional_field!(deserialize_uses, CatiaConsolidatedEdgeUses, "uses");
cadmpeg_core::named_optional_field!(
    deserialize_analytic_circle,
    CatiaConsolidatedAnalyticCircleBinding,
    "analytic_circle"
);
cadmpeg_core::named_optional_field!(
    deserialize_class25_descriptor,
    CatiaConsolidatedClass25Descriptor,
    "class25_descriptor"
);
