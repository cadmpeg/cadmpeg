// SPDX-License-Identifier: Apache-2.0
//! Redundant native configuration identities across Features and Keywords.

use std::collections::{BTreeMap, BTreeSet};

use super::ManifestConfigurationIndex;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ConfigurationBindings {
    keywords: Vec<Keyword>,
    documents: Vec<Manifest>,
    resolved: BTreeMap<String, Option<usize>>,
    names_by_index: BTreeMap<usize, Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Keyword {
    name: Option<String>,
    source_index: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Manifest {
    configurations: Vec<Configuration>,
    models: Vec<Model>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Configuration {
    name: Option<String>,
    index: Option<String>,
    model: Option<String>,
    most_recent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Model {
    id: Option<String>,
    name: Option<String>,
    index: Option<String>,
}

fn decimal(value: Option<&str>) -> Option<usize> {
    let value = value.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))?;
    value.parse().ok()
}

impl ConfigurationBindings {
    pub(super) fn record_keywords(&mut self, document: &roxmltree::Document<'_>) {
        self.keywords.extend(
            document
                .descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .map(|node| Keyword {
                    name: node.attribute("Name").map(str::to_owned),
                    source_index: node.attribute("SourceIndex").map(str::to_owned),
                }),
        );
    }

    pub(super) fn record_features(&mut self, document: &roxmltree::Document<'_>) {
        let mut manifest = Manifest::default();
        for node in document.descendants().filter(roxmltree::Node::is_element) {
            match node.tag_name().name() {
                "swConfiguration" => manifest.configurations.push(Configuration {
                    name: node.attribute("swName").map(str::to_owned),
                    index: node.attribute("swID").map(str::to_owned),
                    model: node.attribute("swModelRef").map(str::to_owned),
                    most_recent: node
                        .attribute("swMostRecentConfiguration")
                        .map(str::to_owned),
                }),
                "swModel" => manifest.models.push(Model {
                    id: node.attribute("id").map(str::to_owned),
                    name: node.attribute("swConfigurationName").map(str::to_owned),
                    index: node.attribute("swConfigurationId").map(str::to_owned),
                }),
                _ => {}
            }
        }
        self.documents.push(manifest);
    }

    pub(super) fn has_manifest_rows(&self) -> bool {
        self.documents.iter().any(|d| !d.configurations.is_empty())
    }

    fn rows(&self) -> impl Iterator<Item = (&Manifest, &Configuration)> {
        self.documents.iter().flat_map(|document| {
            document
                .configurations
                .iter()
                .map(move |row| (document, row))
        })
    }

    pub(super) fn finish(&mut self) {
        let mut keywords = BTreeMap::<&str, Vec<&Keyword>>::new();
        let mut explicit_counts = BTreeMap::<usize, usize>::new();
        let mut explicit_names = BTreeMap::new();
        for keyword in &self.keywords {
            if let Some(index) = decimal(keyword.source_index.as_deref()) {
                *explicit_counts.entry(index).or_default() += 1;
                explicit_names.insert(index, keyword.name.as_deref());
            }
            if let Some(name) = keyword.name.as_deref() {
                keywords.entry(name).or_default().push(keyword);
            }
        }
        let mut rows = BTreeMap::<&str, Vec<(usize, &Configuration)>>::new();
        let mut counts = BTreeMap::<usize, usize>::new();
        let mut row_names = BTreeMap::new();
        let mut models = Vec::new();
        for (document_index, document) in self.documents.iter().enumerate() {
            let mut by_id = BTreeMap::<&str, Vec<&Model>>::new();
            for model in &document.models {
                if let Some(id) = model.id.as_deref() {
                    by_id.entry(id).or_default().push(model);
                }
            }
            models.push(by_id);
            for row in &document.configurations {
                if let Some(name) = row.name.as_deref() {
                    rows.entry(name).or_default().push((document_index, row));
                }
                if let Some(index) = decimal(row.index.as_deref()) {
                    *counts.entry(index).or_default() += 1;
                    row_names.insert(index, row.name.as_deref());
                }
            }
        }
        let mut resolved = BTreeMap::new();
        let names: BTreeSet<_> = keywords.keys().chain(rows.keys()).copied().collect();
        for name in names {
            let index = (|| {
                let keyword = match keywords.get(name).map(Vec::as_slice) {
                    None => None,
                    Some([keyword]) => Some(*keyword),
                    Some(_) => return None,
                };
                if name.is_empty() {
                    return None;
                }
                let manifest_index = (|| {
                    let [(document, row)] = rows.get(name)?.as_slice() else {
                        return None;
                    };
                    let index = decimal(row.index.as_deref())?;
                    if counts.get(&index) != Some(&1)
                        || explicit_counts.get(&index).is_some_and(|count| *count > 1)
                        || explicit_names
                            .get(&index)
                            .is_some_and(|other| *other != Some(name))
                    {
                        return None;
                    }
                    let reference = row.model.as_deref().filter(|s| !s.is_empty())?;
                    let [model] = models[*document].get(reference)?.as_slice() else {
                        return None;
                    };
                    if model.name.as_deref() != Some(name)
                        || model.index.as_deref() != row.index.as_deref()
                    {
                        return None;
                    }
                    Some(index)
                })();
                let Some(raw_index) = keyword.and_then(|keyword| keyword.source_index.as_deref())
                else {
                    return manifest_index;
                };
                let index = decimal(Some(raw_index))?;
                if explicit_counts.get(&index) != Some(&1)
                    || counts.get(&index).is_some_and(|count| *count > 1)
                    || rows.get(name).is_some_and(|rows| rows.len() > 1)
                    || row_names
                        .get(&index)
                        .copied()
                        .flatten()
                        .is_some_and(|other| other != name)
                    || manifest_index.is_some_and(|manifest| manifest != index)
                {
                    return None;
                }
                Some(index)
            })();
            resolved.insert(name.to_owned(), index);
        }
        let mut names_by_index = BTreeMap::new();
        for (name, index) in &resolved {
            if let Some(index) = index {
                names_by_index
                    .entry(*index)
                    .and_modify(|value| *value = None)
                    .or_insert_with(|| Some(name.clone()));
            }
        }
        self.resolved = resolved;
        self.names_by_index = names_by_index;
    }

    pub(super) fn index_for(&self, name: &str) -> ManifestConfigurationIndex {
        if !self.has_manifest_rows() {
            return ManifestConfigurationIndex::Absent;
        }
        self.resolved.get(name).copied().flatten().map_or(
            ManifestConfigurationIndex::Unresolved,
            ManifestConfigurationIndex::Resolved,
        )
    }

    pub(super) fn name_for(&self, index: usize) -> Option<&str> {
        self.names_by_index.get(&index).and_then(Option::as_deref)
    }

    pub(super) fn active(&self) -> super::ManifestActiveConfiguration {
        if !self.has_manifest_rows() {
            return super::ManifestActiveConfiguration::Absent;
        }
        let mut rows = self
            .rows()
            .filter(|(_, r)| r.most_recent.as_deref() == Some("YES"));
        let Some((document, row)) = rows.next() else {
            return super::ManifestActiveConfiguration::Ambiguous;
        };
        if rows.next().is_some() {
            return super::ManifestActiveConfiguration::Ambiguous;
        }
        let Some(index) = decimal(row.index.as_deref()) else {
            return super::ManifestActiveConfiguration::Ambiguous;
        };
        if self
            .rows()
            .filter(|(_, row)| decimal(row.index.as_deref()) == Some(index))
            .count()
            != 1
        {
            return super::ManifestActiveConfiguration::Ambiguous;
        }
        let Some(name) = self.name_for(index) else {
            return super::ManifestActiveConfiguration::Ambiguous;
        };
        if row.name.as_deref().is_some_and(|value| value != name) {
            return super::ManifestActiveConfiguration::Ambiguous;
        }
        if let Some(reference) = row.model.as_deref() {
            if reference.is_empty() {
                return super::ManifestActiveConfiguration::Ambiguous;
            }
            let mut models = document
                .models
                .iter()
                .filter(|model| model.id.as_deref() == Some(reference));
            let Some(model) = models.next() else {
                return super::ManifestActiveConfiguration::Ambiguous;
            };
            if models.next().is_some()
                || model.name.as_deref().is_some_and(|value| value != name)
                || model
                    .index
                    .as_deref()
                    .is_some_and(|value| Some(value) != row.index.as_deref())
            {
                return super::ManifestActiveConfiguration::Ambiguous;
            }
        }
        super::ManifestActiveConfiguration::Unique(index, Some(name.to_owned()))
    }
}

#[cfg(test)]
mod tests;
