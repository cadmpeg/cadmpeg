// SPDX-License-Identifier: Apache-2.0
//! Bounded projections of body-local reference closures.
use super::{CutOptions, Document};
use crate::report::check::Finding;
use cadmpeg_core::CodecError;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct Graph<'a> {
    nodes: Vec<(&'static str, &'a str)>,
    ids: BTreeMap<&'a str, usize>,
    edges: Vec<Vec<usize>>,
}

impl<'a> Graph<'a> {
    pub(crate) fn add(&mut self, arena: &'static str, id: &'a str) {
        self.nodes.push((arena, id));
    }

    pub(crate) fn index(&mut self) {
        self.ids
            .extend(self.nodes.iter().enumerate().map(|(i, (_, id))| (*id, i)));
        self.edges.resize_with(self.nodes.len(), Vec::new);
    }

    pub(crate) fn references<T: crate::schema::EntitySchema>(
        &mut self,
        entity: &T,
    ) -> Result<(), CodecError> {
        let value = serde_json::to_value(entity).map_err(CodecError::malformed)?;
        let Some(&index) = self.ids.get(entity.identity()) else {
            return Ok(());
        };
        crate::reference_walk::visit_edges(
            &value,
            "",
            true,
            &|id| self.ids.get(id).copied().into_iter().collect(),
            &mut |path, target| {
                let field = path.split('.').next().unwrap_or_default();
                if !matches!(
                    field,
                    "source_object" | "product_definition" | "source_properties" | "source_content"
                ) && !matches!(self.nodes[target].0, "occurrences" | "product_definitions")
                {
                    self.edges[index].push(target);
                }
                true
            },
        );
        Ok(())
    }

    fn owner(&self, index: usize) -> Option<usize> {
        let parent = match self.nodes[index].0 {
            "bodies" => return Some(index),
            "regions" => "bodies",
            "shells" => "regions",
            "faces" => "shells",
            "loops" => "faces",
            "coedges" => "loops",
            _ => return None,
        };
        self.edges[index]
            .iter()
            .find(|target| self.nodes[**target].0 == parent)
            .and_then(|target| self.owner(*target))
    }

    fn closure(&self, finding: &Finding) -> BTreeSet<String> {
        let roots: BTreeSet<usize> = finding
            .entity
            .iter()
            .chain(&finding.related)
            .filter_map(|id| self.ids.get(id.as_str()).copied())
            .collect();
        let bodies: BTreeSet<usize> = roots
            .iter()
            .filter_map(|index| self.owner(*index))
            .collect();
        let whole_body = roots.iter().any(|index| self.nodes[*index].0 == "bodies");
        let mut keep = roots;
        loop {
            let before = keep.len();
            for index in keep.clone() {
                for target in &self.edges[index] {
                    if !whole_body
                        && matches!(
                            (self.nodes[index].0, self.nodes[*target].0),
                            ("bodies", "regions") | ("regions", "shells")
                        )
                    {
                        continue;
                    }
                    keep.insert(*target);
                }
            }
            for (index, targets) in self.edges.iter().enumerate() {
                if !bodies.is_empty()
                    && self
                        .owner(index)
                        .is_some_and(|owner| !bodies.contains(&owner))
                {
                    continue;
                }
                if targets.iter().any(|target| {
                    keep.contains(target)
                        && matches!(
                            (self.nodes[index].0, self.nodes[*target].0),
                            ("coedges", "edges")
                                | ("loops", "coedges")
                                | ("faces", "loops")
                                | ("shells", "faces")
                                | ("regions", "shells")
                                | ("bodies", "regions")
                        )
                }) {
                    keep.insert(index);
                }
            }
            if keep.len() == before {
                break;
            }
        }
        keep.into_iter()
            .map(|index| self.nodes[index].1.to_owned())
            .collect()
    }
}

/// Select body-local rows before applying serialization limits.
pub fn select(
    document: &Document,
    finding: &Finding,
    options: &CutOptions,
) -> Result<Option<Document>, CodecError> {
    let graph = document.model.repro_graph(finding)?;
    let mut ids = graph.closure(finding);
    if ids.len() > options.max_rows {
        return Ok(None);
    }
    let mut selected = Document::empty();
    selected.fragment = true;
    selected.source = document
        .source
        .as_ref()
        .map(crate::document::SourceMeta::repro_header);
    selected.tolerances = document.tolerances;
    selected.model = document.model.repro_select(&ids);
    let mut streams: BTreeSet<String> = finding
        .entity
        .iter()
        .chain(&finding.related)
        .filter(|id| !graph.ids.contains_key(id.as_str()))
        .chain(
            selected
                .model
                .features
                .iter()
                .filter_map(|feature| feature.native_ref.as_ref()),
        )
        .filter_map(|id| stream(id))
        .collect();
    let companions: Vec<String> = streams
        .iter()
        .filter_map(|scope| {
            scope
                .strip_suffix("/BulkStream.dat")
                .map(|prefix| format!("{prefix}/MetaStream.dat"))
        })
        .collect();
    streams.extend(companions);
    let mut native_bytes = 0usize;
    for (codec, namespace) in &document.native.0 {
        for (arena, records) in namespace.arenas() {
            let mut rows = Vec::new();
            for record in records.iter().filter(|record| {
                streams.iter().any(|scope| {
                    record
                        .id()
                        .strip_prefix(scope)
                        .is_some_and(|tail| tail.starts_with(':'))
                })
            }) {
                if selected.model.entity_count()
                    + selected
                        .native
                        .0
                        .values()
                        .flat_map(|ns| ns.arenas().values())
                        .map(Vec::len)
                        .sum::<usize>()
                    + rows.len()
                    >= options.max_rows
                {
                    return Ok(None);
                }
                let Some(bytes) = native_bytes
                    .checked_add(record.text_len())
                    .filter(|bytes| *bytes <= options.max_serialized_bytes)
                else {
                    return Ok(None);
                };
                native_bytes = bytes;
                rows.push(record.clone());
            }
            if !rows.is_empty() {
                selected
                    .native
                    .namespace_mut(codec)
                    .arenas_mut()
                    .insert(arena.clone(), rows);
            }
        }
    }
    if !selected.native.0.is_empty() {
        let mut referenced = BTreeSet::new();
        for namespace in selected.native.0.values() {
            for record in namespace.arenas().values().flatten() {
                let value = serde_json::to_value(record).map_err(CodecError::malformed)?;
                crate::reference_walk::visit_edges(
                    &value,
                    "",
                    true,
                    &|id| graph.ids.get(id).copied().into_iter().collect(),
                    &mut |_, index| {
                        referenced.insert(graph.nodes[index].1.to_owned());
                        true
                    },
                );
            }
        }
        drop(graph);
        let mut expanded = finding.clone();
        expanded.related.extend(referenced);
        ids.extend(document.model.repro_graph(&expanded)?.closure(&expanded));
        if ids.len() > options.max_rows {
            return Ok(None);
        }
        selected.model = document.model.repro_select(&ids);
    }
    if let Err(error) = super::projection_size(&selected, options) {
        match &error {
            CodecError::Malformed(message) | CodecError::InvalidInput(message)
                if message.contains("fragment byte limit exceeded")
                    || message.contains("fragment row limit exceeded") =>
            {
                return Ok(None)
            }
            _ => return Err(error),
        }
    }
    let mut value = serde_json::to_value(&selected).map_err(CodecError::malformed)?;
    if let Some(model) = value["model"].as_object_mut() {
        for (arena, rows) in model {
            if let Some(rows) = rows.as_array_mut() {
                for row in rows {
                    if let Some(object) = row.as_object_mut() {
                        object.remove("source_object");
                        object.remove("product_definition");
                        let child = match arena.as_str() {
                            "bodies" => Some("regions"),
                            "regions" => Some("shells"),
                            _ => None,
                        };
                        if let Some(children) = child
                            .and_then(|field| object.get_mut(field))
                            .and_then(Value::as_array_mut)
                        {
                            children.retain(|id| id.as_str().is_some_and(|id| ids.contains(id)));
                        }
                    }
                }
            }
        }
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(CodecError::malformed)
}

fn stream(id: &str) -> Option<String> {
    let mut parts = id.splitn(3, ':');
    Some(format!("{}:{}", parts.next()?, parts.next()?))
}
