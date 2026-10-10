// SPDX-License-Identifier: Apache-2.0
//! Admitted native element-map nodes and persistent-name bindings.

use cadmpeg_core::decode::admission::{Admission, StandardAdmission};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

/// Owns scoped scratch data and releases it before its reservation.
pub(crate) struct ScopedData<'ctx, T> {
    pub(crate) data: T,
    pub(crate) _storage: ScopedReservation<'ctx>,
}
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One persisted element map owned by an exact-shape property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ElementMapRecord {
    /// Stable map identity.
    pub(crate) id: String,
    /// Owning shape property identity.
    pub(crate) property: String,
    /// Version discriminator carried by the shape value.
    pub(crate) version: String,
    /// Document string-table index used by mapped names.
    pub(crate) hasher_index: Option<usize>,
    /// Referenced side entry, or `None` for inline data.
    pub(crate) source_entry: Option<String>,
    /// Native map identity.
    pub(crate) map_id: u64,
    /// Optional XML element-map count retained as metadata; it does not frame
    /// or have to equal the native map stream.
    pub(crate) declared_count: usize,
    /// Ordered postfix dictionary.
    pub(crate) postfixes: Vec<String>,
    /// Ordered child-map records; the last record is the owning shape map.
    pub(crate) maps: ElementMapNodes,
}

/// A nonempty sequence whose last node is the owning shape map.
///
/// The one-based node index belongs to the serialized sequence, so it is
/// checked while that sequence is admitted and derived again when it is
/// written. An admitted node cannot carry an index that disagrees with its
/// position.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<ElementMapNodeWire>")]
pub(crate) struct ElementMapNodes(Vec<ElementMapNode>);

impl ElementMapNodes {
    pub(crate) fn from_nodes<A: Admission>(
        nodes: Vec<ElementMapNode>,
        admission: &A,
    ) -> Result<Result<Self, String>, A::Error> {
        if nodes.is_empty() {
            return Ok(Err(admission.format_retained(
                format_args!("maps must contain a root node"),
                "FreeCAD element-map validation diagnostic",
            )?));
        }
        for (position, node) in nodes.iter().enumerate() {
            admission.charge_work(1, "FreeCAD element-map validation nodes")?;
            let node_index = position + 1;
            for group in &node.groups {
                admission.charge_work(1, "FreeCAD element-map validation groups")?;
                for (child_index, descriptor) in group.children.iter().enumerate() {
                    admission.charge_work(1, "FreeCAD element-map validation children")?;
                    let group_name = group.indexed_name.as_str();
                    let mut field_offset = 0;
                    let mut words = [None; 7];
                    for word in &mut words {
                        *word = descriptor_word(descriptor, &mut field_offset, admission)?;
                    }
                    let [Some(index), Some(offset), Some(count), Some(tag), Some(map), Some(_), Some(string_ids)] =
                        words
                    else {
                        return Ok(Err(admission.format_retained(format_args!(
                            "element-map node {node_index} group {group_name} child {child_index} descriptor must contain seven fields"
                        ), "FreeCAD element-map validation diagnostic")?));
                    };
                    if descriptor_word(descriptor, &mut field_offset, admission)?.is_some() {
                        return Ok(Err(admission.format_retained(format_args!(
                            "element-map node {node_index} group {group_name} child {child_index} descriptor must contain seven fields"
                        ), "FreeCAD element-map validation diagnostic")?));
                    }
                    let mut ids = string_ids.as_bytes().iter().enumerate();
                    let mut id_start = 0;
                    let mut first_id = true;
                    let mut valid_ids = true;
                    loop {
                        let next = if ids.len() == 0 {
                            None
                        } else {
                            admission.charge_work(1, "FreeCAD element-map child string-id scan")?;
                            ids.next()
                        };
                        let (end, done) = match next {
                            Some((index, b'.')) => (index, false),
                            Some(_) => continue,
                            None => (string_ids.len(), true),
                        };
                        let id = &string_ids[id_start..end];
                        if first_id {
                            if id != "0" {
                                valid_ids = false;
                                break;
                            }
                            first_id = false;
                        } else {
                            admission.charge_work(
                                cadmpeg_core::decode::u64_from_index(id.len()),
                                "FreeCAD element-map child string-id number",
                            )?;
                            if id.parse::<i64>().is_err() {
                                valid_ids = false;
                                break;
                            }
                        }
                        id_start = end + 1;
                        if done {
                            break;
                        }
                    }
                    if !valid_ids {
                        return Ok(Err(admission.format_retained(format_args!(
                            "element-map node {node_index} group {group_name} child {child_index} has an invalid child string-id list"
                        ), "FreeCAD element-map validation diagnostic")?));
                    }
                    for (name, word) in [
                        ("index", index),
                        ("offset", offset),
                        ("count", count),
                        ("tag", tag),
                        ("mapIndex", map),
                    ] {
                        admission.charge_work(
                            cadmpeg_core::decode::u64_from_index(word.len()),
                            "FreeCAD element-map child number",
                        )?;
                        let Ok(value) = word.parse::<i64>() else {
                            return Ok(Err(admission.format_retained(format_args!(
                                    "element-map node {node_index} group {group_name} child {child_index} has an invalid {name}"
                                ), "FreeCAD element-map validation diagnostic")?));
                        };
                        if name != "tag" && i32::try_from(value).is_err() {
                            return Ok(Err(admission.format_retained(format_args!(
                                "element-map node {node_index} group {group_name} child {child_index} {name} exceeds signed 32-bit range"
                            ), "FreeCAD element-map validation diagnostic")?));
                        }
                        if matches!(name, "index" | "offset") && value < 0 {
                            return Ok(Err(admission.format_retained(format_args!(
                                "element-map node {node_index} group {group_name} child {child_index} has a negative {name}"
                            ), "FreeCAD element-map validation diagnostic")?));
                        }
                        if name == "mapIndex"
                            && usize::try_from(value).map_or(true, |index| index >= node_index)
                        {
                            return Ok(Err(admission.format_retained(format_args!(
                                "element-map node {node_index} group {group_name} child {child_index} mapIndex {value} does not name a prior map"
                            ), "FreeCAD element-map validation diagnostic")?));
                        }
                    }
                }
            }
        }
        Ok(Ok(Self(nodes)))
    }
}

impl TryFrom<Vec<ElementMapNode>> for ElementMapNodes {
    type Error = String;
    fn try_from(nodes: Vec<ElementMapNode>) -> Result<Self, Self::Error> {
        match Self::from_nodes(nodes, &StandardAdmission) {
            Ok(result) => result,
            Err(error) => match error {},
        }
    }
}

fn descriptor_word<'a, A: Admission>(
    text: &'a str,
    offset: &mut usize,
    admission: &A,
) -> Result<Option<&'a str>, A::Error> {
    let bytes = text.as_bytes();
    while *offset < bytes.len() {
        admission.charge_work(1, "FreeCAD element-map child descriptor scan")?;
        if !bytes[*offset].is_ascii_whitespace() {
            break;
        }
        *offset += 1;
    }
    let start = *offset;
    while *offset < bytes.len() {
        admission.charge_work(1, "FreeCAD element-map child descriptor scan")?;
        if bytes[*offset].is_ascii_whitespace() {
            break;
        }
        *offset += 1;
    }
    Ok((start != *offset).then_some(&text[start..*offset]))
}

impl TryFrom<Vec<ElementMapNodeWire>> for ElementMapNodes {
    type Error = String;

    fn try_from(wire_nodes: Vec<ElementMapNodeWire>) -> Result<Self, Self::Error> {
        let mut nodes = Vec::new();
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
    pub(crate) fn from_root_names(
        ctx: &DecodeContext<'_>,
        map_id: u64,
        mut groups: ScopedData<'_, BTreeMap<String, Vec<Vec<ElementMappedName>>>>,
    ) -> Result<Self, CodecError> {
        let mut root_groups =
            ctx.collection_vec(groups.data.len(), "FreeCAD legacy root map groups")?;
        let values = std::mem::take(&mut groups.data);
        for (indexed_name, names) in ctx.admit_iter(values, "FreeCAD legacy root map group scan")? {
            root_groups.push(ElementMapGroup {
                indexed_name,
                children: Vec::new(),
                names,
            });
        }
        drop(groups);
        let mut nodes = ctx.collection_vec(1, "FreeCAD legacy root map node")?;
        nodes.push(ElementMapNode {
            map_id,
            groups: root_groups,
        });
        Ok(Self(nodes))
    }

    /// Returns the owning shape map.
    pub(crate) fn root(&self) -> &ElementMapNode {
        &self.0[self.0.len() - 1]
    }

    /// Add a topology binding without exposing child-map descriptors for mutation.
    pub(crate) fn bind_root_topology<'a>(
        &mut self,
        ctx: &DecodeContext<'_>,
        bindings: impl IntoIterator<Item = (&'a str, usize, &'a str)>,
    ) -> Result<(), CodecError> {
        let root = self.0.len() - 1;
        let mut group_storage = ctx.reserve_scoped(0, "FreeCAD element topology group index")?;
        let mut groups = BTreeMap::new();
        let mut source_groups = self.0[root].groups.iter_mut();
        while source_groups.len() != 0 {
            let Some(group) =
                ctx.next_charged(&mut source_groups, "FreeCAD element topology group scan")?
            else {
                break;
            };
            group_storage.with_storage(|| {
                ctx.push_btree_group(
                    &mut groups,
                    group.indexed_name.as_str(),
                    &mut group.names,
                    "FreeCAD element topology group index",
                    "FreeCAD element topology group members",
                )
            })?;
        }
        let mut bindings = bindings.into_iter();
        while bindings.size_hint().1 != Some(0) {
            let Some((indexed_name, source_index, id)) =
                ctx.next_charged(&mut bindings, "FreeCAD element topology binding scan")?
            else {
                break;
            };
            let Some(matches) = ctx.get_mut_btree_map(
                &mut groups,
                indexed_name,
                "FreeCAD element topology group lookup",
            )?
            else {
                continue;
            };
            let mut matching_groups = matches.iter_mut();
            while matching_groups.len() != 0 {
                let Some(names) = ctx.next_charged(
                    &mut matching_groups,
                    "FreeCAD element topology matching groups",
                )?
                else {
                    break;
                };
                let Some(names) = names.get_mut(source_index) else {
                    continue;
                };
                let mut name_iter = names.iter_mut();
                while name_iter.len() != 0 {
                    let Some(name) =
                        ctx.next_charged(&mut name_iter, "FreeCAD element topology name scan")?
                    else {
                        break;
                    };
                    if !ctx.any_by(
                        &name.topology_ids,
                        |existing| {
                            ctx.equal(
                                existing.as_str(),
                                id,
                                "FreeCAD element topology identity comparison",
                            )
                        },
                        "FreeCAD element topology identity search",
                    )? {
                        ctx.reserve_vec(
                            &mut name.topology_ids,
                            1,
                            "FreeCAD element topology bindings",
                        )?;
                        name.topology_ids
                            .push(ctx.copy_retained_text(id, "FreeCAD element topology identity")?);
                    }
                }
            }
        }
        Ok(())
    }

    /// Returns nodes in serialized order.
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, ElementMapNode> {
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
pub(crate) struct ElementMapNode {
    /// Native node identity.
    pub(crate) map_id: u64,
    /// Ordered indexed-element groups.
    pub(crate) groups: Vec<ElementMapGroup>,
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
pub(crate) struct ElementMapGroup {
    /// Native indexed-name prefix such as `Face`, `Edge`, or `Vertex`.
    pub(crate) indexed_name: String,
    /// Child-map descriptors retained exactly.
    pub(crate) children: Vec<String>,
    /// One entry per transient indexed element, in index order.
    pub(crate) names: Vec<Vec<ElementMappedName>>,
}

/// One persistent mapped-name encoding and its neutral topology bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ElementMappedName {
    /// Exact encoded mapped name.
    pub(crate) encoded: String,
    /// Decoded base and postfix when all dictionary references are valid.
    pub(crate) resolved: Option<String>,
    /// Referenced persistent string identities.
    pub(crate) string_ids: Vec<i64>,
    /// Neutral topology ids for every placed occurrence of this element.
    pub(crate) topology_ids: Vec<String>,
}

#[cfg(test)]
mod tests;
