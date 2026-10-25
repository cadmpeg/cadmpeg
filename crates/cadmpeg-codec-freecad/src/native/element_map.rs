// SPDX-License-Identifier: Apache-2.0
//! Admitted native element-map nodes and persistent-name bindings.

use serde::{Deserialize, Serialize};
pub(crate) mod legacy;
mod rewrite;
pub(crate) use legacy::LegacyMapRecord;

cadmpeg_core::named_optional_field!(
    deserialize_legacy_records,
    Vec<Option<LegacyMapRecord>>,
    "legacy_records"
);

/// One persisted element map owned by an exact-shape property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ElementMapRecordWire")]
pub(crate) struct ElementMapRecord {
    /// Stable map identity.
    pub(crate) id: String,
    /// Owning shape property identity.
    pub(crate) property: String,
    /// Version discriminator carried by the shape value.
    pub(crate) version: String,
    /// Original document string-table selector.
    pub(crate) hasher_index: Option<usize>,
    /// Typed string-table record resolved from the original selector.
    pub(crate) string_table: Option<String>,
    /// Referenced side entry, or `None` for inline data.
    pub(crate) source_entry: Option<String>,
    /// Native map identity.
    pub(crate) map_id: u64,
    /// Optional XML element-map count retained as metadata; it does not frame
    /// or have to equal the native map stream.
    pub(crate) declared_count: Option<usize>,
    /// Canonical source-positioned legacy rows, absent for the modern grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) legacy_records: Option<Vec<Option<LegacyMapRecord>>>,
    /// Ordered postfix dictionary.
    pub(crate) postfixes: Vec<Option<String>>,
    /// Ordered child-map records; the last record is the owning shape map.
    pub(crate) maps: ElementMapNodes,
}

#[derive(Deserialize)]
struct ElementMapRecordWire {
    id: String,
    property: String,
    version: String,
    hasher_index: Option<usize>,
    string_table: Option<String>,
    source_entry: Option<String>,
    map_id: u64,
    declared_count: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_legacy_records")]
    legacy_records: Option<Vec<Option<LegacyMapRecord>>>,
    postfixes: Vec<Option<String>>,
    maps: ElementMapNodes,
}
impl TryFrom<ElementMapRecordWire> for ElementMapRecord {
    type Error = String;
    fn try_from(wire: ElementMapRecordWire) -> Result<Self, Self::Error> {
        if let Some(rows) = &wire.legacy_records {
            let expected = legacy::project(rows)?;
            if wire.map_id != 0 || !wire.postfixes.is_empty() || wire.maps != expected {
                return Err(
                    "legacy element-map fields disagree with canonical legacy_records".into(),
                );
            }
        } else {
            for node in wire.maps.iter() {
                for name in node
                    .groups
                    .iter()
                    .flatten()
                    .flat_map(|group| &group.names)
                    .flatten()
                    .flatten()
                {
                    let parsed =
                        crate::element_map::parse_mapped_name(&name.encoded, &wire.postfixes)
                            .map_err(|error| format!("invalid modern mapped name: {error}"))?;
                    if parsed.resolved != name.resolved || parsed.string_ids != name.string_ids {
                        return Err("modern mapped-name derived fields disagree with encoded/current dictionary".into());
                    }
                }
            }
        }
        Ok(Self {
            id: wire.id,
            property: wire.property,
            version: wire.version,
            hasher_index: wire.hasher_index,
            string_table: wire.string_table,
            source_entry: wire.source_entry,
            map_id: wire.map_id,
            declared_count: wire.declared_count,
            legacy_records: wire.legacy_records,
            postfixes: wire.postfixes,
            maps: wire.maps,
        })
    }
}

/// A nonempty sequence whose last node is the owning shape map.
///
/// The one-based node index belongs to the serialized sequence, so it is
/// checked while that sequence is admitted and derived again when it is
/// written. An admitted node cannot carry an index that disagrees with its
/// position.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<Option<ElementMapNodeWire>>")]
pub(crate) struct ElementMapNodes {
    nodes: Vec<Option<ElementMapNode>>,
    root: ElementMapNode,
}

impl TryFrom<Vec<ElementMapNode>> for ElementMapNodes {
    type Error = String;

    fn try_from(nodes: Vec<ElementMapNode>) -> Result<Self, Self::Error> {
        Self::try_sparse(nodes.into_iter().map(Some).collect())
    }
}

impl ElementMapNodes {
    pub(crate) fn validate_child(
        node_index: usize,
        indexed_name: &str,
        child_index: usize,
        descriptor: &str,
    ) -> Result<(), String> {
        let location = || {
            format!(
                "element-map node {node_index} group {} child {child_index}",
                indexed_name
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
        if string_ids.next() != Some("0") || string_ids.any(|id| id.parse::<i64>().is_err()) {
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
                return Err(format!("{} {name} exceeds signed 32-bit range", location()));
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
        Ok(())
    }

    pub(crate) fn try_sparse(mut nodes: Vec<Option<ElementMapNode>>) -> Result<Self, String> {
        if nodes.is_empty() || nodes.last().is_none_or(Option::is_none) {
            return Err("maps must contain a root node".to_owned());
        }
        for (position, node) in nodes.iter().enumerate() {
            let Some(node) = node else {
                continue;
            };
            for group in node.groups.iter().flatten() {
                for (child_index, descriptor) in group.children.iter().enumerate() {
                    let Some(descriptor) = descriptor else {
                        continue;
                    };
                    Self::validate_child(
                        position + 1,
                        &group.indexed_name,
                        child_index,
                        descriptor,
                    )?;
                    let map_index = descriptor
                        .split_ascii_whitespace()
                        .nth(4)
                        .and_then(|value| value.parse::<usize>().ok())
                        .ok_or_else(|| "invalid child map index".to_owned())?;
                    if map_index != 0 && nodes.get(map_index - 1).is_none_or(Option::is_none) {
                        return Err("child descriptor needs an absent map node".to_owned());
                    }
                }
            }
        }
        let root = nodes
            .pop()
            .flatten()
            .ok_or_else(|| "maps must contain a root node".to_owned())?;
        Ok(Self { nodes, root })
    }
}

impl TryFrom<Vec<Option<ElementMapNodeWire>>> for ElementMapNodes {
    type Error = String;

    fn try_from(wire_nodes: Vec<Option<ElementMapNodeWire>>) -> Result<Self, Self::Error> {
        let mut nodes = Vec::new();
        for (position, wire) in wire_nodes.into_iter().enumerate() {
            let Some(wire) = wire else {
                nodes.push(None);
                continue;
            };
            // Every position is below the vector's representable length.
            let expected_index = position + 1;
            if wire.index != expected_index {
                return Err(format!(
                    "maps[{position}].index must equal {expected_index}, got {}",
                    wire.index
                ));
            }
            nodes.push(Some(ElementMapNode {
                map_id: wire.map_id,
                groups: wire.groups,
            }));
        }
        Self::try_sparse(nodes)
    }
}

impl Serialize for ElementMapNodes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let mut sequence = serializer.serialize_seq(Some(self.nodes.len() + 1))?;
        for (position, node) in self
            .nodes
            .iter()
            .map(Option::as_ref)
            .chain(std::iter::once(Some(&self.root)))
            .enumerate()
        {
            let Some(node) = node else {
                sequence.serialize_element(&Option::<ElementMapNodeWireRef<'_>>::None)?;
                continue;
            };
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
        nodes
            .nodes
            .into_iter()
            .flatten()
            .chain(std::iter::once(nodes.root))
            .collect()
    }
}

impl ElementMapNodes {
    /// Returns the owning shape map.
    pub(crate) fn root(&self) -> &ElementMapNode {
        &self.root
    }

    pub(crate) fn slot_count(&self) -> usize {
        self.nodes.len() + 1
    }

    pub(crate) fn indexed(&self) -> impl Iterator<Item = (usize, &ElementMapNode)> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| node.as_ref().map(|node| (index + 1, node)))
            .chain(std::iter::once((self.nodes.len() + 1, &self.root)))
    }

    /// Returns nodes in serialized order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ElementMapNode> {
        self.nodes
            .iter()
            .filter_map(Option::as_ref)
            .chain(std::iter::once(&self.root))
    }
}

#[cfg(test)]
impl std::ops::Index<usize> for ElementMapNodes {
    type Output = ElementMapNode;
    fn index(&self, index: usize) -> &Self::Output {
        if index == self.nodes.len() {
            &self.root
        } else {
            self.nodes[index].as_ref().unwrap()
        }
    }
}

/// One map node, including recursively referenced child maps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElementMapNode {
    /// Native node identity.
    pub(crate) map_id: u64,
    /// Ordered indexed-element groups.
    pub(crate) groups: Vec<Option<ElementMapGroup>>,
}

#[derive(Deserialize)]
struct ElementMapNodeWire {
    index: usize,
    map_id: u64,
    groups: Vec<Option<ElementMapGroup>>,
}

#[derive(Serialize)]
struct ElementMapNodeWireRef<'a> {
    index: usize,
    map_id: u64,
    groups: &'a [Option<ElementMapGroup>],
}

/// Persistent-name chains for one native topology kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ElementMapGroup {
    /// Native indexed-name prefix such as `Face`, `Edge`, or `Vertex`.
    pub(crate) indexed_name: String,
    /// Child-map descriptors retained exactly.
    pub(crate) children: Vec<Option<String>>,
    /// One entry per transient indexed element, in index order.
    pub(crate) names: Vec<Option<Vec<ElementMappedName>>>,
}

/// One persistent mapped-name encoding and its string selectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ElementMappedName {
    /// Exact encoded mapped name.
    pub(crate) encoded: String,
    /// Decoded base and postfix when all dictionary references are valid.
    pub(crate) resolved: Option<String>,
    /// Referenced persistent string identities.
    pub(crate) string_ids: Vec<i64>,
}

#[cfg(test)]
mod tests;
