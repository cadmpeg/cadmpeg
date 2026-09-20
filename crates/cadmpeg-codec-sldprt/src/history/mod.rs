// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

pub(crate) mod bind;
pub(crate) mod classify;
pub(crate) mod configuration;
mod encode;
pub(crate) mod hash;
pub(crate) mod literals;
pub(crate) mod parameters;
pub(crate) mod project;
pub(crate) mod selections;
pub(crate) mod write;

use self::classify::classless_builtin_node;

use crate::container::ContainerScan;
use crate::records::FeatureSource;
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap};

/// Keys one source element's attributes, charging every key the reader cannot
/// key.
///
/// The element is still transferred. A key holding no non-whitespace character
/// cannot be asked for, and a key the element states twice is already taken, so
/// the charge names the element and, for a restated key, the key itself.
fn keyed_attributes(
    losses: &mut Vec<LossNote>,
    record: &str,
    entries: impl IntoIterator<Item = (String, String)>,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let (kept, refused) = cadmpeg_core::text::named_entries_reporting(record, entries);
    for key in refused {
        losses.push(
            crate::loss::SldprtLossCode::SourcePropertyKeyBlank
                .note(format!("{key}; the property is not transferred")),
        );
    }
    kept
}

/// The key a history record id carries: the ordinal of the section it was read
/// from and its position in that section.
///
/// Composed from the two integers, so the key is key text by construction and
/// the projection recovers it from the id without a fallible re-parse.
fn history_record_key(source: usize, ordinal: usize) -> cadmpeg_ir::ids::IdentityKey {
    cadmpeg_ir::ids::IdentityKey::from(source).colon(ordinal)
}

pub(crate) fn histories(
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Vec<FeatureHistory> {
    scan.sections()
        .filter_map(|section| {
            let source = section.ordinal();
            let text = crate::container::xml_text(section.payload())?;
            let doc = roxmltree::Document::parse(&text).ok()?;
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                return None;
            }
            let stream = section.source_stream();
            let parent = format!("sldprt:history:feature-history#{source}");
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .map(|(ordinal, node)| {
                    let id = format!(
                        "sldprt:history:configuration#{}",
                        history_record_key(source, ordinal)
                    );
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        stream,
                        node.range().start as u64,
                        "Configuration",
                        Exactness::ByteExact,
                    );
                    let properties = keyed_attributes(
                        losses,
                        &id,
                        node.attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "Name" | "Material" | "SourceIndex")
                            })
                            .map(|attribute| {
                                (attribute.name().to_string(), attribute.value().to_string())
                            }),
                    );
                    Configuration {
                        id,
                        parent: parent.clone(),
                        ordinal: ordinal as u32,
                        source_index: node
                            .attribute("SourceIndex")
                            .and_then(|value| value.parse().ok()),
                        name: node.attribute("Name").unwrap_or("").into(),
                        material: node
                            .attribute("Material")
                            .filter(|value| !value.is_empty())
                            .map(str::to_string),
                        properties,
                    }
                })
                .collect();
            let feature_nodes = root
                .descendants()
                .filter(|node| {
                    node.is_element()
                        && !matches!(
                            node.tag_name().name(),
                            "Keywords" | "Configuration" | "Dimension"
                        )
                })
                .collect::<Vec<_>>();
            let feature_ids = feature_nodes
                .iter()
                .enumerate()
                .map(|(ordinal, node)| {
                    (
                        node.range().start,
                        format!(
                            "sldprt:history:feature#{}",
                            history_record_key(source, ordinal)
                        ),
                    )
                })
                .collect::<HashMap<_, _>>();
            let features = feature_nodes
                .into_iter()
                .enumerate()
                .map(|(ordinal, node)| {
                    let id = feature_ids[&node.range().start].clone();
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        stream,
                        node.range().start as u64,
                        node.tag_name().name(),
                        Exactness::ByteExact,
                    );
                    let properties = keyed_attributes(
                        losses,
                        &id,
                        node.attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "id" | "Name" | "Type" | "Suppressed")
                            })
                            .map(|attribute| {
                                (attribute.name().to_string(), attribute.value().to_string())
                            }),
                    );
                    Feature {
                        id,
                        parent: parent.clone(),
                        xml_tag: node.tag_name().name().into(),
                        tree_parent: node.ancestors().skip(1).find_map(|ancestor| {
                            let record_id = feature_ids.get(&ancestor.range().start)?.clone();
                            Some(crate::records::TreeParent::Record {
                                record_id,
                                source_id: ancestor
                                    .attribute("id")
                                    .and_then(|value| FeatureSource::try_from(value).ok()),
                            })
                        }),
                        source_id: node
                            .attribute("id")
                            .and_then(|value| FeatureSource::try_from(value).ok()),
                        ordinal: ordinal as u32,
                        name: node.attribute("Name").unwrap_or("").into(),
                        kind: node
                            .attribute("Type")
                            .unwrap_or_else(|| node.tag_name().name())
                            .into(),
                        input_class: None,
                        suppressed: node
                            .attribute("Suppressed")
                            .is_some_and(|value| matches!(value, "1" | "true" | "True")),
                        parameters: node
                            .children()
                            .filter(|child| {
                                child.is_element() && child.tag_name().name() == "Dimension"
                            })
                            .filter_map(|dimension| {
                                Some((
                                    cadmpeg_core::text::NonBlankString::new(
                                        dimension.attribute("Name")?,
                                    )?,
                                    dimension.text().unwrap_or_default().trim().into(),
                                ))
                            })
                            .collect::<BTreeMap<_, _>>(),
                        dimension_properties: node
                            .children()
                            .filter(|child| {
                                child.is_element() && child.tag_name().name() == "Dimension"
                            })
                            .filter_map(|dimension| {
                                let name = dimension.attribute("Name")?;
                                let properties = dimension
                                    .attributes()
                                    .filter(|attribute| attribute.name() != "Name")
                                    .map(|attribute| {
                                        (
                                            attribute.name().to_string(),
                                            attribute.value().to_string(),
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                let properties = keyed_attributes(losses, name, properties);
                                (!properties.is_empty()).then(|| (name.into(), properties))
                            })
                            .collect(),
                        properties,
                        text: (!node.children().any(|child| child.is_element()))
                            .then(|| node.text().map(str::trim).unwrap_or_default().to_string())
                            .filter(|value| !value.is_empty()),
                        content: node
                            .children()
                            .filter_map(|child| {
                                if child.is_text() {
                                    let value = child.text()?.trim();
                                    return (!value.is_empty())
                                        .then(|| FeatureContent::Text(value.into()));
                                }
                                if !child.is_element() {
                                    return None;
                                }
                                if child.tag_name().name() == "Dimension" {
                                    return child
                                        .attribute("Name")
                                        .map(|name| FeatureContent::Dimension(name.into()));
                                }
                                feature_ids
                                    .get(&child.range().start)
                                    .cloned()
                                    .map(FeatureContent::Feature)
                            })
                            .collect(),
                    }
                })
                .collect::<Vec<_>>();
            let configuration_ids = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .map(|(ordinal, node)| {
                    (
                        node.range().start,
                        format!(
                            "sldprt:history:configuration#{}",
                            history_record_key(source, ordinal)
                        ),
                    )
                })
                .collect::<HashMap<_, _>>();
            let content = root
                .children()
                .filter_map(|child| {
                    if child.is_text() {
                        let value = child.text()?.trim();
                        return (!value.is_empty()).then(|| HistoryContent::Text(value.into()));
                    }
                    if !child.is_element() {
                        return None;
                    }
                    configuration_ids
                        .get(&child.range().start)
                        .cloned()
                        .map(HistoryContent::Configuration)
                        .or_else(|| {
                            feature_ids
                                .get(&child.range().start)
                                .cloned()
                                .map(HistoryContent::Feature)
                        })
                })
                .collect();
            let id = parent;
            crate::annotations::note(
                annotations,
                id.clone(),
                stream,
                0,
                "Keywords",
                Exactness::ByteExact,
            );
            let properties = keyed_attributes(
                losses,
                &id,
                root.attributes()
                    .filter(|attribute| attribute.name() != "Name")
                    .map(|attribute| (attribute.name().to_string(), attribute.value().to_string())),
            );
            Some(FeatureHistory {
                id,
                part_name: root
                    .attribute("Name")
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                properties,
                content,
                configurations,
                features,
            })
        })
        .collect()
}

pub(crate) fn enrich_scene_classes(
    histories: &mut [FeatureHistory],
    scene_classes: &HashMap<u32, String>,
) {
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let Some(source) = feature.source_value() else {
            continue;
        };
        if feature.input_class.is_none() && classless_builtin_node(feature) {
            feature.input_class = scene_classes.get(&source).cloned();
        }
    }
}

#[cfg(test)]
pub(in crate::history) mod tests;
