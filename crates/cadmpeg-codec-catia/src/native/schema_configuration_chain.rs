// SPDX-License-Identifier: Apache-2.0
//! Ordered configuration-row paths and their compatible wire projection.

use super::{CatiaEntityReference, CatiaEntityReferenceIndex, CatiaSchemaConfigurationRowLink};
use crate::native::entity_record::CatiaEntityRecord;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    /// Absent when the successor does not follow the row in source order, or
    /// when keeping them would pass [`MAX_INTERVENING_ENTITIES_PER_RECORD`].
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

const INTERVENING_ENTITY_OPERATION: &str = "retain CATIA configuration-row intervening entities";

/// Intervening references all chains may keep, per entity record. Intervals
/// of one chain can nest, so the references grow with the square of its row
/// count; a link past the allowance keeps no intervening order.
pub(crate) const MAX_INTERVENING_ENTITIES_PER_RECORD: usize = 8;

/// Configuration-row chains and the links whose intervening order was not kept.
#[derive(Debug)]
pub(super) struct SchemaConfigurationRowChains {
    pub(super) chains: Vec<CatiaSchemaConfigurationRowChain>,
    /// Links left without intervening entities at the per-record allowance.
    pub(super) bounded_links: usize,
}

pub(super) fn derive_schema_configuration_row_chains(
    ctx: &DecodeContext<'_>,
    records: &[CatiaEntityRecord],
    entity_references: &CatiaEntityReferenceIndex<'_>,
) -> Result<SchemaConfigurationRowChains, CodecError> {
    derive_schema_configuration_row_chains_within(
        ctx,
        records,
        entity_references,
        records
            .len()
            .checked_mul(MAX_INTERVENING_ENTITIES_PER_RECORD)
            .unwrap_or(records.len()),
    )
}

pub(super) fn derive_schema_configuration_row_chains_within(
    ctx: &DecodeContext<'_>,
    records: &[CatiaEntityRecord],
    entity_references: &CatiaEntityReferenceIndex<'_>,
    intervening_allowance: usize,
) -> Result<SchemaConfigurationRowChains, CodecError> {
    let mut row_ids = HashSet::new();
    let mut groups = HashMap::<(&str, u32), Vec<(u32, &CatiaSchemaConfigurationRowLink)>>::new();
    for entity in records {
        let Some(link) = &entity.schema_configuration_row_link() else {
            continue;
        };
        row_ids.insert((entity.object_graph.as_str(), entity.entity_id));
        let key = (
            entity.object_graph.as_str(),
            link.class_reference.entity_id(),
        );

        groups
            .entry(key)
            .or_default()
            .push((entity.entity_id, link));
    }
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(
        |((left_graph, left_root), _), ((right_graph, right_root), _)| {
            left_graph.cmp(right_graph).then(left_root.cmp(right_root))
        },
    );

    let mut graph_entities = HashMap::<&str, Vec<(u32, usize)>>::new();
    if !groups.is_empty() {
        for (index, entity) in records.iter().enumerate() {
            graph_entities
                .entry(entity.object_graph.as_str())
                .or_default()
                .push((entity.entity_id, index));
        }
        for entities in graph_entities.values_mut() {
            entities.sort_unstable();
        }
    }

    let mut chains = Vec::new();
    let mut remaining_intervening = intervening_allowance;
    let mut bounded_links = 0usize;
    for ((graph, root), links) in groups {
        let mut successors = HashMap::new();
        for (row, link) in &links {
            successors.insert(*row, *link);
        }
        if successors.len() != links.len() {
            continue;
        }
        let mut row_ids_in_order = Vec::new();
        let mut visited = HashSet::new();
        let mut current = root;
        while let Some(link) = successors.get(&current).copied() {
            if !visited.insert(current) {
                break;
            }
            row_ids_in_order.push(current);
            current = link.successor.entity_id();
        }
        if visited.len() != links.len() || row_ids.contains(&(graph, current)) {
            continue;
        }
        let Some(last_row) = row_ids_in_order.last() else {
            continue;
        };
        let Some(last_link) = successors.get(last_row) else {
            continue;
        };
        let terminal = last_link.successor.clone();
        let Some((namespace, graph_key)) = graph.split_once('#') else {
            continue;
        };
        let mut components = namespace.split(':');
        let (Some(format), Some(scope), Some(_kind), None) = (
            components.next(),
            components.next(),
            components.next(),
            components.next(),
        ) else {
            continue;
        };
        let id = format!("{format}:{scope}:schema-configuration-row-chain#{graph_key}:{root}");
        let mut chain_links = Vec::new();
        for row_id in row_ids_in_order {
            let link = successors[&row_id];
            let successor_id = link.successor.entity_id();
            let intervening_entities = if row_id < successor_id {
                let entities = graph_entities.get(graph).map_or(&[][..], Vec::as_slice);
                let first = entities.partition_point(|&(id, _)| id <= row_id);
                let end = entities.partition_point(|&(id, _)| id < successor_id);
                let entities = &entities[first..end];
                if let Some(remaining) = remaining_intervening.checked_sub(entities.len()) {
                    remaining_intervening = remaining;
                    let mut in_record_order = entities
                        .iter()
                        .map(|&(id, index)| (index, id))
                        .collect::<Vec<_>>();
                    in_record_order.sort_unstable();
                    let mut between = Vec::new();
                    for (_, entity_id) in in_record_order {
                        let reference = entity_references.reference(ctx, graph, entity_id)?;
                        ctx.retain_bytes(
                            u64_from_index(reference.entity().map_or(0, str::len)),
                            INTERVENING_ENTITY_OPERATION,
                        )?;
                        between.push(reference);
                    }
                    Some(between)
                } else {
                    bounded_links += 1;
                    None
                }
            } else {
                None
            };
            let row = entity_references.reference(ctx, graph, row_id)?;
            chain_links.push(CatiaSchemaConfigurationRowChainLink {
                row,
                successor_payload_offset: link.successor_payload_offset,
                intervening_entities,
            });
        }
        let object_graph = graph.to_owned();
        chains.push(CatiaSchemaConfigurationRowChain {
            id,
            object_graph,
            links: chain_links,
            terminal,
        });
    }
    Ok(SchemaConfigurationRowChains {
        chains,
        bounded_links,
    })
}

#[cfg(test)]
mod tests {
    use super::super::CatiaEntityReference;
    use super::{CatiaSchemaConfigurationRowChain, CatiaSchemaConfigurationRowChainLink};

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
