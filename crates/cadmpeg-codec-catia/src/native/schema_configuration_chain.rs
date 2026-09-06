// SPDX-License-Identifier: Apache-2.0
//! Ordered configuration-row paths and their compatible wire projection.

use super::{
    entity_reference, object_graph_derived_id, CatiaEntityClassByGraphIdentityIndex,
    CatiaEntityReference, CatiaSchemaConfigurationRowLink, CatiaTerminalNullByGraphIndex,
};
use crate::native::entity_record::CatiaEntityRecord;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// One complete ordered schema-configuration chain formed by exact `configrow` links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ChainWire", into = "ChainWire")]
pub struct CatiaSchemaConfigurationRowChain {
    /// Stable identity derived from the graph and stored class identity.
    pub id: String,
    /// Object graph containing every row link.
    pub object_graph: String,
    /// Successor incidences in chain order from the root row.
    links: Vec<CatiaSchemaConfigurationRowChainLink>,
    /// Target of the final row successor.
    pub terminal: CatiaEntityReference,
}

/// One ordered edge in a complete schema-configuration-row successor chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatiaSchemaConfigurationRowChainLink {
    /// Row entity carrying the successor occurrence.
    pub row: CatiaEntityReference,
    /// Byte offset of the successor atom within the row object's payload.
    pub successor_payload_offset: u64,
    /// Same-graph entities strictly between the row and successor.
    ///
    /// Absent when the successor does not follow the row in source order.
    pub intervening_entities: Option<Vec<CatiaEntityReference>>,
}

impl CatiaSchemaConfigurationRowChain {
    pub fn links(&self) -> &[CatiaSchemaConfigurationRowChainLink] {
        &self.links
    }

    #[cfg(test)]
    pub fn links_mut(&mut self) -> &mut [CatiaSchemaConfigurationRowChainLink] {
        &mut self.links
    }

    #[cfg(test)]
    pub fn successor(&self, index: usize) -> Option<&CatiaEntityReference> {
        self.links.get(index)?;
        Some(
            self.links
                .get(index + 1)
                .map_or(&self.terminal, |link| &link.row),
        )
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ChainWire {
    id: String,
    object_graph: String,
    #[serde(default)]
    links: Vec<LinkWire>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LinkWire {
    row: CatiaEntityReference,
    successor_payload_offset: u64,
    successor: CatiaEntityReference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    intervening_entities: Option<Vec<CatiaEntityReference>>,
}

impl From<CatiaSchemaConfigurationRowChain> for ChainWire {
    fn from(chain: CatiaSchemaConfigurationRowChain) -> Self {
        let mut remaining = chain.links.into_iter().peekable();
        let mut links = Vec::with_capacity(remaining.len());
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

pub(super) fn derive_schema_configuration_row_chains(
    records: &[CatiaEntityRecord],
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
) -> Vec<CatiaSchemaConfigurationRowChain> {
    let row_ids = records
        .iter()
        .filter(|entity| entity.schema_configuration_row_link().is_some())
        .map(|entity| (entity.object_graph.as_str(), entity.entity_id))
        .collect::<HashSet<_>>();
    let mut groups = HashMap::<(&str, u32), Vec<(u32, &CatiaSchemaConfigurationRowLink)>>::new();
    for entity in records {
        let Some(link) = &entity.schema_configuration_row_link() else {
            continue;
        };
        groups
            .entry((
                entity.object_graph.as_str(),
                link.class_reference.entity_id(),
            ))
            .or_default()
            .push((entity.entity_id, link));
    }
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(
        |((left_graph, left_root), _), ((right_graph, right_root), _)| {
            left_graph.cmp(right_graph).then(left_root.cmp(right_root))
        },
    );

    groups
        .into_iter()
        .filter_map(|((graph, root), links)| {
            let successors = links.iter().copied().collect::<HashMap<_, _>>();
            if successors.len() != links.len() {
                return None;
            }
            let mut row_ids_in_order = Vec::with_capacity(links.len());
            let mut visited = HashSet::new();
            let mut current = root;
            while let Some(link) = successors.get(&current).copied() {
                if !visited.insert(current) {
                    return None;
                }
                row_ids_in_order.push(current);
                current = link.successor.entity_id();
            }
            if visited.len() != links.len() || row_ids.contains(&(graph, current)) {
                return None;
            }
            let terminal = successors.get(row_ids_in_order.last()?)?.successor.clone();
            let links = row_ids_in_order
                .into_iter()
                .map(|row_id| {
                    let link = successors[&row_id];
                    let successor_id = link.successor.entity_id();
                    CatiaSchemaConfigurationRowChainLink {
                        row: entity_reference(
                            graph,
                            row_id,
                            entities,
                            entity_classes,
                            terminal_nulls,
                        ),
                        successor_payload_offset: link.successor_payload_offset,
                        intervening_entities: (row_id < successor_id).then(|| {
                            records
                                .iter()
                                .filter(|entity| {
                                    entity.object_graph == graph
                                        && entity.entity_id > row_id
                                        && entity.entity_id < successor_id
                                })
                                .map(|entity| {
                                    entity_reference(
                                        graph,
                                        entity.entity_id,
                                        entities,
                                        entity_classes,
                                        terminal_nulls,
                                    )
                                })
                                .collect()
                        }),
                    }
                })
                .collect();
            Some(CatiaSchemaConfigurationRowChain {
                id: object_graph_derived_id(
                    graph,
                    "schema-configuration-row-chain",
                    &root.to_string(),
                )?,
                object_graph: graph.to_string(),
                links,
                terminal,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let wire = serde_json::to_value(&chain).unwrap();
        assert_eq!(wire["links"][0]["successor"], wire["links"][1]["row"]);
        assert_eq!(
            wire["links"][1]["successor"],
            serde_json::to_value(&chain.terminal).unwrap()
        );
        assert_eq!(
            serde_json::from_value::<CatiaSchemaConfigurationRowChain>(wire.clone()).unwrap(),
            chain
        );
        let mut mismatched = wire.clone();
        mismatched["links"][0]["successor"] = serde_json::to_value(&chain.terminal).unwrap();
        assert!(serde_json::from_value::<CatiaSchemaConfigurationRowChain>(mismatched).is_err());
        let mut empty = wire;
        empty["links"] = serde_json::json!([]);
        assert!(serde_json::from_value::<CatiaSchemaConfigurationRowChain>(empty).is_err());
    }
}
