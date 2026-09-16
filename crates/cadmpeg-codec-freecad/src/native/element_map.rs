// SPDX-License-Identifier: Apache-2.0
//! Admitted native element-map nodes and persistent-name bindings.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One persisted element map owned by an exact-shape property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMapRecord {
    /// Stable map identity.
    pub id: String,
    /// Owning shape property identity.
    pub property: String,
    /// Version discriminator carried by the shape value.
    pub version: String,
    /// Document string-table index used by mapped names.
    pub hasher_index: Option<usize>,
    /// Referenced side entry, or `None` for inline data.
    pub source_entry: Option<String>,
    /// Native map identity.
    pub map_id: u64,
    /// Optional XML element-map count retained as metadata; it does not frame
    /// or have to equal the native map stream.
    pub declared_count: usize,
    /// Ordered postfix dictionary.
    pub postfixes: Vec<String>,
    /// Ordered child-map records; the last record is the owning shape map.
    pub maps: ElementMapNodes,
}

/// A nonempty sequence whose last node is the owning shape map.
///
/// The one-based node index belongs to the serialized sequence, so it is
/// checked while that sequence is admitted and derived again when it is
/// written. An admitted node cannot carry an index that disagrees with its
/// position.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<ElementMapNodeWire>")]
pub struct ElementMapNodes(Vec<ElementMapNode>);

impl TryFrom<Vec<ElementMapNode>> for ElementMapNodes {
    type Error = String;

    fn try_from(nodes: Vec<ElementMapNode>) -> Result<Self, Self::Error> {
        if nodes.is_empty() {
            return Err("maps must contain a root node".to_owned());
        }
        for (position, node) in nodes.iter().enumerate() {
            let node_index = position + 1;
            for group in &node.groups {
                for (child_index, descriptor) in group.children.iter().enumerate() {
                    let location = || {
                        format!(
                            "element-map node {node_index} group {} child {child_index}",
                            group.indexed_name
                        )
                    };
                    let mut fields = descriptor.split_ascii_whitespace();
                    let words: [Option<&str>; 7] = std::array::from_fn(|_| fields.next());
                    let [Some(index), Some(offset), Some(count), Some(tag), Some(map), Some(_), Some(string_ids)] =
                        words
                    else {
                        return Err(format!(
                            "{} descriptor must contain seven fields",
                            location()
                        ));
                    };
                    if fields.next().is_some() {
                        return Err(format!(
                            "{} descriptor must contain seven fields",
                            location()
                        ));
                    }
                    let mut string_ids = string_ids.split('.');
                    if string_ids.next() != Some("0")
                        || string_ids.any(|id| id.parse::<i64>().is_err())
                    {
                        return Err(format!(
                            "{} has an invalid child string-id list",
                            location()
                        ));
                    }
                    for (name, word) in [
                        ("index", index),
                        ("offset", offset),
                        ("count", count),
                        ("tag", tag),
                        ("mapIndex", map),
                    ] {
                        let value = word
                            .parse::<i64>()
                            .map_err(|_| format!("{} has an invalid {name}", location()))?;
                        if name != "tag" && i32::try_from(value).is_err() {
                            return Err(format!(
                                "{} {name} exceeds signed 32-bit range",
                                location()
                            ));
                        }
                        if matches!(name, "index" | "offset") && value < 0 {
                            return Err(format!("{} has a negative {name}", location()));
                        }
                        if name == "mapIndex"
                            && usize::try_from(value).map_or(true, |index| index >= node_index)
                        {
                            return Err(format!(
                                "{} mapIndex {value} does not name a prior map",
                                location()
                            ));
                        }
                    }
                }
            }
        }
        Ok(Self(nodes))
    }
}

impl TryFrom<Vec<ElementMapNodeWire>> for ElementMapNodes {
    type Error = String;

    fn try_from(wire_nodes: Vec<ElementMapNodeWire>) -> Result<Self, Self::Error> {
        let mut nodes = Vec::with_capacity(wire_nodes.len());
        for (position, wire) in wire_nodes.into_iter().enumerate() {
            // Every position is below the vector's representable length.
            let expected_index = position + 1;
            if wire.index != expected_index {
                return Err(format!(
                    "maps[{position}].index must equal {expected_index}, got {}",
                    wire.index
                ));
            }
            nodes.push(ElementMapNode {
                map_id: wire.map_id,
                groups: wire.groups,
            });
        }
        Self::try_from(nodes)
    }
}

impl Serialize for ElementMapNodes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for (position, node) in self.0.iter().enumerate() {
            sequence.serialize_element(&ElementMapNodeWireRef {
                index: position + 1,
                map_id: node.map_id,
                groups: &node.groups,
            })?;
        }
        sequence.end()
    }
}

impl From<ElementMapNodes> for Vec<ElementMapNode> {
    fn from(nodes: ElementMapNodes) -> Self {
        nodes.0
    }
}

impl ElementMapNodes {
    /// Construct one root from legacy name groups, which have no child maps.
    pub fn from_root_names(
        map_id: u64,
        groups: BTreeMap<String, Vec<Vec<ElementMappedName>>>,
    ) -> Self {
        Self(vec![ElementMapNode {
            map_id,
            groups: groups
                .into_iter()
                .map(|(indexed_name, names)| ElementMapGroup {
                    indexed_name,
                    children: Vec::new(),
                    names,
                })
                .collect(),
        }])
    }

    /// Returns the owning shape map.
    pub fn root(&self) -> &ElementMapNode {
        &self.0[self.0.len() - 1]
    }

    /// Add a topology binding without exposing child-map descriptors for mutation.
    pub fn bind_root_topology(&mut self, indexed_name: &str, source_index: usize, id: &str) {
        let index = self.0.len() - 1;
        for group in &mut self.0[index].groups {
            if group.indexed_name != indexed_name {
                continue;
            }
            let Some(names) = group.names.get_mut(source_index) else {
                continue;
            };
            for name in names {
                if !name.topology_ids.iter().any(|existing| existing == id) {
                    name.topology_ids.push(id.to_owned());
                }
            }
        }
    }

    /// Returns nodes in serialized order.
    pub fn iter(&self) -> std::slice::Iter<'_, ElementMapNode> {
        self.0.iter()
    }
}

impl std::ops::Index<usize> for ElementMapNodes {
    type Output = ElementMapNode;
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

/// One map node, including recursively referenced child maps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementMapNode {
    /// Native node identity.
    pub map_id: u64,
    /// Ordered indexed-element groups.
    pub groups: Vec<ElementMapGroup>,
}

#[derive(Deserialize)]
struct ElementMapNodeWire {
    index: usize,
    map_id: u64,
    groups: Vec<ElementMapGroup>,
}

#[derive(Serialize)]
struct ElementMapNodeWireRef<'a> {
    index: usize,
    map_id: u64,
    groups: &'a [ElementMapGroup],
}

/// Persistent-name chains for one native topology kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMapGroup {
    /// Native indexed-name prefix such as `Face`, `Edge`, or `Vertex`.
    pub indexed_name: String,
    /// Child-map descriptors retained exactly.
    pub children: Vec<String>,
    /// One entry per transient indexed element, in index order.
    pub names: Vec<Vec<ElementMappedName>>,
}

/// One persistent mapped-name encoding and its neutral topology bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMappedName {
    /// Exact encoded mapped name.
    pub encoded: String,
    /// Decoded base and postfix when all dictionary references are valid.
    pub resolved: Option<String>,
    /// Referenced persistent string identities.
    pub string_ids: Vec<i64>,
    /// Neutral topology ids for every placed occurrence of this element.
    pub topology_ids: Vec<String>,
}

#[cfg(test)]
mod tests;
