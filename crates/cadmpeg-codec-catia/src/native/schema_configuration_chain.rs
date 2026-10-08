// SPDX-License-Identifier: Apache-2.0
//! Ordered configuration-row paths and their compatible wire projection.

use super::{
    entity_reference, CatiaEntityReference, CatiaEntityReferenceIndex,
    CatiaSchemaConfigurationRowLink,
};
use crate::native::entity_record::CatiaEntityRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// One complete ordered schema-configuration chain formed by exact `configrow` links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ChainWire", into = "ChainWire")]
pub(crate) struct CatiaSchemaConfigurationRowChain {
    /// Stable identity derived from the graph and stored class identity.
    pub(super) id: String,
    /// Object graph containing every row link.
    pub(super) object_graph: String,
    /// Successor incidences in chain order from the root row.
    links: Vec<CatiaSchemaConfigurationRowChainLink>,
    /// Target of the final row successor.
    pub(crate) terminal: CatiaEntityReference,
}

/// One ordered edge in a complete schema-configuration-row successor chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatiaSchemaConfigurationRowChainLink {
    /// Row entity carrying the successor occurrence.
    pub(super) row: CatiaEntityReference,
    /// Byte offset of the successor atom within the row object's payload.
    pub(super) successor_payload_offset: u64,
    /// Same-graph entities strictly between the row and successor.
    ///
    /// Absent when the successor does not follow the row in source order.
    pub(crate) intervening_entities: Option<Vec<CatiaEntityReference>>,
}

impl CatiaSchemaConfigurationRowChain {
    pub(crate) fn links(&self) -> &[CatiaSchemaConfigurationRowChainLink] {
        &self.links
    }

    #[cfg(test)]
    pub(super) fn links_mut(&mut self) -> &mut [CatiaSchemaConfigurationRowChainLink] {
        &mut self.links
    }

    #[cfg(test)]
    pub(super) fn successor(&self, index: usize) -> Option<&CatiaEntityReference> {
        self.links.get(index)?;
        Some(
            self.links
                .get(index + 1)
                .map_or(&self.terminal, |link| &link.row),
        )
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct ChainWire {
    id: String,
    object_graph: String,
    #[serde(default)]
    links: Vec<LinkWire>,
}

#[derive(Serialize, Deserialize)]
struct LinkWire {
    row: CatiaEntityReference,
    successor_payload_offset: u64,
    successor: CatiaEntityReference,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_intervening_entities"
    )]
    intervening_entities: Option<Vec<CatiaEntityReference>>,
}

impl From<CatiaSchemaConfigurationRowChain> for ChainWire {
    fn from(chain: CatiaSchemaConfigurationRowChain) -> Self {
        let mut remaining = chain.links.into_iter().peekable();
        let mut links = Vec::new();
        while let Some(link) = remaining.next() {
            let successor = remaining
                .peek()
                .map_or(&chain.terminal, |next| &next.row)
                .clone();
            links.push(LinkWire {
                row: link.row,
                successor_payload_offset: link.successor_payload_offset,
                successor,
                intervening_entities: link.intervening_entities,
            });
        }
        Self {
            id: chain.id,
            object_graph: chain.object_graph,
            links,
        }
    }
}

impl ChainWire {
    pub(super) fn from_charged(
        ctx: &DecodeContext<'_>,
        chain: CatiaSchemaConfigurationRowChain,
    ) -> Result<Self, CodecError> {
        let mut remaining = chain.links.into_iter().peekable();
        let mut links = Vec::new();
        while let Some(link) =
            ctx.next_charged(&mut remaining, "catia_configuration_chain_wire_visits")?
        {
            let successor = copy_reference(
                ctx,
                remaining.peek().map_or(&chain.terminal, |next| &next.row),
            )?;
            ctx.push_vec(
                &mut links,
                LinkWire {
                    row: link.row,
                    successor_payload_offset: link.successor_payload_offset,
                    successor,
                    intervening_entities: link.intervening_entities,
                },
                "catia_configuration_chain_wire_links",
            )?;
        }
        Ok(Self {
            id: chain.id,
            object_graph: chain.object_graph,
            links,
        })
    }
}

impl TryFrom<ChainWire> for CatiaSchemaConfigurationRowChain {
    type Error = &'static str;

    fn try_from(wire: ChainWire) -> Result<Self, Self::Error> {
        let terminal = wire
            .links
            .last()
            .ok_or("configuration row chain is empty")?
            .successor
            .clone();
        if wire
            .links
            .windows(2)
            .any(|pair| pair[0].successor != pair[1].row)
        {
            return Err("configuration row successor disagrees with next row");
        }
        Ok(Self {
            id: wire.id,
            object_graph: wire.object_graph,
            terminal,
            links: wire
                .links
                .into_iter()
                .map(|link| CatiaSchemaConfigurationRowChainLink {
                    row: link.row,
                    successor_payload_offset: link.successor_payload_offset,
                    intervening_entities: link.intervening_entities,
                })
                .collect(),
        })
    }
}

/// Entity positions of each graph, ordered by entity id and then position.
type EntitiesByIdentity<'a> = BTreeMap<&'a str, Vec<(u32, usize)>>;

fn entities_by_identity<'a>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    records: &'a [CatiaEntityRecord],
) -> Result<EntitiesByIdentity<'a>, CodecError> {
    const OPERATION: &str = "catia_configuration_between_index";
    let mut groups = EntitiesByIdentity::new();
    storage.with_storage(|| -> Result<(), CodecError> {
        for (index, entity) in ctx
            .admit_iter(records, "catia_configuration_between_entity_visits")?
            .enumerate()
        {
            ctx.push_btree_group(
                &mut groups,
                entity.object_graph.as_str(),
                (entity.entity_id, index),
                OPERATION,
                OPERATION,
            )?;
        }
        Ok(())
    })?;
    for (_, group) in ctx.admit_iter(&mut groups, "catia_configuration_between_groups")? {
        ctx.sort_unstable_by_key(group, |row| *row, Ord::cmp, OPERATION)?;
    }
    Ok(groups)
}

/// Derives the configuration row chains from the entity records; `row_link`
/// gives the configuration row link of the record at a position.
pub(super) fn derive_schema_configuration_row_chains<'a>(
    ctx: &DecodeContext<'_>,
    records: &'a [CatiaEntityRecord],
    row_link: impl Fn(usize) -> Option<&'a CatiaSchemaConfigurationRowLink>,
    references: &CatiaEntityReferenceIndex<'_>,
) -> Result<Vec<CatiaSchemaConfigurationRowChain>, CodecError> {
    // Row identities, groups, successor maps and the identity index are
    // dropped once the chains are built.
    let mut scratch = ctx.reserve_scoped(0, "catia_configuration_scratch")?;
    let mut row_ids = HashSet::new();
    let mut groups = BTreeMap::<(&str, u32), Vec<(u32, &CatiaSchemaConfigurationRowLink)>>::new();
    for (index, entity) in ctx
        .admit_iter(records, "catia_configuration_record_visits")?
        .enumerate()
    {
        let Some(link) = row_link(index) else {
            continue;
        };
        scratch.with_storage(|| -> Result<(), CodecError> {
            ctx.insert_hash_set(
                &mut row_ids,
                (entity.object_graph.as_str(), entity.entity_id),
                "catia_configuration_row_ids",
            )?;
            ctx.push_btree_group(
                &mut groups,
                (
                    entity.object_graph.as_str(),
                    link.class_reference.entity_id(),
                ),
                (entity.entity_id, link),
                "catia_configuration_groups",
                "catia_configuration_group_links",
            )
        })?;
    }

    let mut identities = None;
    let mut chains = Vec::new();
    for (&(graph, root), links) in
        ctx.admit_iter(&groups, "catia_configuration_sorted_group_visits")?
    {
        const SUCCESSORS: &str = "catia_configuration_successors";
        let mut group_scratch = ctx.reserve_scoped(0, SUCCESSORS)?;
        let mut successors = HashMap::new();
        for &(row, link) in ctx.admit_iter(links, "catia_configuration_group_link_visits")? {
            group_scratch
                .with_storage(|| ctx.insert_hash_map(&mut successors, row, link, SUCCESSORS))?;
        }
        if successors.len() != links.len() {
            continue;
        }
        let mut row_ids_in_order = Vec::new();
        let mut visited = HashSet::new();
        let mut current = root;
        while let Some(&link) = ctx.get_hash_map(&successors, &current, SUCCESSORS)? {
            if !group_scratch.with_storage(|| {
                ctx.insert_hash_set(&mut visited, current, "catia_configuration_visited")
            })? {
                break;
            }
            group_scratch.with_storage(|| {
                ctx.push_vec(&mut row_ids_in_order, current, "catia_configuration_order")
            })?;
            current = link.successor.entity_id();
        }
        if visited.len() != links.len()
            || ctx.contains_hash_set(&row_ids, &(graph, current), "catia_configuration_row_ids")?
        {
            continue;
        }
        let Some(last_row) = row_ids_in_order.last() else {
            continue;
        };
        let Some(last_link) = ctx.get_hash_map(&successors, last_row, SUCCESSORS)? else {
            continue;
        };
        let terminal = copy_reference(ctx, &last_link.successor)?;
        let mut chain_links = Vec::new();
        for &row_id in ctx.admit_iter(&row_ids_in_order, "catia_configuration_order_visits")? {
            let Some(link) = ctx.get_hash_map(&successors, &row_id, SUCCESSORS)? else {
                continue;
            };
            let successor_id = link.successor.entity_id();
            let intervening_entities = if row_id < successor_id {
                if identities.is_none() {
                    identities = Some(entities_by_identity(ctx, &mut scratch, records)?);
                }
                Some(intervening_entities(
                    ctx,
                    identities.as_ref(),
                    records,
                    graph,
                    row_id..successor_id,
                    references,
                )?)
            } else {
                None
            };
            let row = entity_reference(ctx, graph, row_id, references)?;
            ctx.push_vec(
                &mut chain_links,
                CatiaSchemaConfigurationRowChainLink {
                    row,
                    successor_payload_offset: link.successor_payload_offset,
                    intervening_entities,
                },
                "catia_configuration_chain_links",
            )?;
        }
        let Some((namespace, graph_key)) =
            ctx.split_once(graph, "#", "catia_configuration_graph_namespace")?
        else {
            continue;
        };
        let Some((format, rest)) =
            ctx.split_once(namespace, ":", "catia_configuration_namespace_components")?
        else {
            continue;
        };
        let Some((scope, kind)) =
            ctx.split_once(rest, ":", "catia_configuration_namespace_components")?
        else {
            continue;
        };
        if ctx.contains_text(kind, ":", "catia_configuration_namespace_components")? {
            continue;
        }
        let id = ctx.format_retained(
            format_args!("{format}:{scope}:schema-configuration-row-chain#{graph_key}:{root}"),
            "catia_configuration_chain_id",
        )?;
        let object_graph = ctx.copy_retained_text(graph, "catia_configuration_graph_id")?;
        ctx.push_vec(
            &mut chains,
            CatiaSchemaConfigurationRowChain {
                id,
                object_graph,
                links: chain_links,
                terminal,
            },
            "catia_configuration_chains",
        )?;
    }
    Ok(chains)
}

/// The graph's entities with an id strictly between a row and its successor,
/// in record order.
fn intervening_entities(
    ctx: &DecodeContext<'_>,
    identities: Option<&EntitiesByIdentity<'_>>,
    records: &[CatiaEntityRecord],
    graph: &str,
    rows: std::ops::Range<u32>,
    references: &CatiaEntityReferenceIndex<'_>,
) -> Result<Vec<CatiaEntityReference>, CodecError> {
    const OPERATION: &str = "catia_configuration_between";
    let Some(group) = identities
        .map(|identities| ctx.get_btree_map(identities, graph, OPERATION))
        .transpose()?
        .flatten()
    else {
        return Ok(Vec::new());
    };
    let first = ctx.partition_point(group, |(id, _)| Ok(*id <= rows.start), OPERATION)?;
    let last = ctx.partition_point(group, |(id, _)| Ok(*id < rows.end), OPERATION)?;
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut positions = storage
        .with_storage(|| ctx.copy_slice(group.get(first..last).unwrap_or_default(), OPERATION))?;
    ctx.sort_unstable_by_key(&mut positions, |(_, index)| *index, Ord::cmp, OPERATION)?;
    let mut between = Vec::new();
    ctx.reserve_vec(&mut between, positions.len(), OPERATION)?;
    for &(_, index) in ctx.admit_iter(&positions, "catia_configuration_between_entity_rows")? {
        let Some(entity) = records.get(index) else {
            continue;
        };
        between.push(entity_reference(ctx, graph, entity.entity_id, references)?);
    }
    Ok(between)
}

fn copy_reference(
    ctx: &DecodeContext<'_>,
    reference: &CatiaEntityReference,
) -> Result<CatiaEntityReference, CodecError> {
    Ok(match reference {
        CatiaEntityReference::Null { entity_id } => CatiaEntityReference::Null {
            entity_id: *entity_id,
        },
        CatiaEntityReference::Unresolved { entity_id } => CatiaEntityReference::Unresolved {
            entity_id: *entity_id,
        },
        CatiaEntityReference::Resolved {
            entity_id,
            entity,
            class_name,
        } => CatiaEntityReference::Resolved {
            entity_id: *entity_id,
            entity: ctx.copy_retained_text(entity, "catia_configuration_entity_id")?,
            class_name: class_name
                .as_ref()
                .map(|name| ctx.copy_retained_text(name, "catia_configuration_class_name"))
                .transpose()?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::super::CatiaEntityReference;
    use super::{CatiaSchemaConfigurationRowChain, CatiaSchemaConfigurationRowChainLink};

    #[test]
    fn configuration_chain_wire_refuses_before_successor_link_growth() {
        let chain = CatiaSchemaConfigurationRowChain {
            id: "chain".to_owned(),
            object_graph: "graph".to_owned(),
            links: vec![CatiaSchemaConfigurationRowChainLink {
                row: CatiaEntityReference::Unresolved { entity_id: 1 },
                successor_payload_offset: 5,
                intervening_entities: None,
            }],
            terminal: CatiaEntityReference::Unresolved { entity_id: 2 },
        };
        let refused = crate::test_support::with_collection_limit(0, |ctx| {
            super::ChainWire::from_charged(ctx, chain.clone())
        });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_configuration_chain_wire_links")
        );
        let charged = crate::test_support::with_service_context(|ctx| {
            super::ChainWire::from_charged(ctx, chain.clone())
        })
        .expect("service budget admits successor wire");
        let original: super::ChainWire = chain.into();
        assert_eq!(
            serde_json::to_value(charged).expect("charged wire"),
            serde_json::to_value(original).expect("legacy wire")
        );
    }

    #[test]
    fn wire_successors_follow_rows_and_keep_the_terminal() {
        let chain = CatiaSchemaConfigurationRowChain {
            id: "chain".to_owned(),
            object_graph: "graph".to_owned(),
            links: [1, 2]
                .map(|entity_id| CatiaSchemaConfigurationRowChainLink {
                    row: CatiaEntityReference::Unresolved { entity_id },
                    successor_payload_offset: 5,
                    intervening_entities: None,
                })
                .to_vec(),
            terminal: CatiaEntityReference::Unresolved { entity_id: 3 },
        };
        let wire = serde_json::to_value(&chain).expect("serialize row chain");
        assert_eq!(wire["links"][0]["successor"], wire["links"][1]["row"]);
        assert_eq!(
            wire["links"][1]["successor"],
            serde_json::to_value(&chain.terminal).expect("serialize terminal")
        );
        assert_eq!(
            serde_json::from_value::<CatiaSchemaConfigurationRowChain>(wire.clone())
                .expect("valid row chain"),
            chain
        );
        let mut mismatched = wire.clone();
        mismatched["links"][0]["successor"] =
            serde_json::to_value(&chain.terminal).expect("serialize terminal");
        assert!(serde_json::from_value::<CatiaSchemaConfigurationRowChain>(mismatched).is_err());
        let mut empty = wire;
        empty["links"] = serde_json::json!([]);
        assert!(serde_json::from_value::<CatiaSchemaConfigurationRowChain>(empty).is_err());
    }
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_intervening_entities,
    Vec<CatiaEntityReference>,
    "intervening_entities"
);
