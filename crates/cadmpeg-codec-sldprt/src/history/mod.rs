// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` Keywords XML feature history.

pub(crate) mod bind;
pub(crate) mod classify;
pub(crate) mod configuration;
mod encode;
pub(crate) mod extrude_direction;
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
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
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
fn keyed_attributes<'name, 'value>(
    losses: &mut Vec<LossNote>,
    record: &str,
    entries: impl IntoIterator<Item = (&'name str, &'value str)>,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let mut kept = BTreeMap::new();
    for (name, value) in entries {
        if name.chars().all(char::is_whitespace) {
            report_unkeyed_property(losses, record, None);
            continue;
        }
        if kept.contains_key(name) {
            report_unkeyed_property(losses, record, Some(name));
            continue;
        }
        if let Some(name) = cadmpeg_core::text::NonBlankString::new(name.to_owned()) {
            kept.insert(name, value.to_owned());
        }
    }
    kept
}

fn report_unkeyed_property(losses: &mut Vec<LossNote>, record: &str, duplicate: Option<&str>) {
    let message = match duplicate {
        Some(key) => format!(
            "{record} states the property {key} a second time; the property is not transferred"
        ),
        None => {
            format!("{record} states a property with a blank key; the property is not transferred")
        }
    };
    losses.push(crate::loss::SldprtLossCode::SourcePropertyKeyBlank.note(message));
}

/// Whether a member name is a name of a Keywords history member:
/// `KeyWords`, a compound-file `KeyWords__ZLB` stream, or a written
/// `Keywords-<n>` member.
fn names_history(name: Option<&str>) -> bool {
    let Some(leaf) = name.and_then(|name| name.rsplit('/').next()) else {
        return false;
    };
    let leaf = leaf.to_ascii_lowercase();
    leaf == "keywords"
        || leaf == "keywords__zlb"
        || leaf.strip_prefix("keywords-").is_some_and(|index| {
            !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// Charges a Keywords history member that no record is read from.
fn report_unread(losses: &mut Vec<LossNote>, member: &str, cause: &str) {
    losses.push(crate::loss::SldprtLossCode::HistoryUnread.note(format!(
        "Keywords member {member}: {cause}; no configuration or feature of the history is read"
    )));
}

/// Charges an `Application` element of a Keywords root, which the reader does
/// not take as a feature and which no document record holds.
fn report_application(
    losses: &mut Vec<LossNote>,
    member: &str,
    application: roxmltree::Node<'_, '_>,
) {
    let name = application
        .attribute("Name")
        .map_or_else(|| "without a Name".to_owned(), |name| format!("{name:?}"));
    let children = application
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|child| match child.attribute("Name") {
            Some(name) => format!("{} {name:?}", child.tag_name().name()),
            None => child.tag_name().name().to_owned(),
        })
        .collect::<Vec<_>>();
    let held = if children.is_empty() {
        "no child element".to_owned()
    } else {
        format!("the child elements {}", children.join(", "))
    };
    losses.push(crate::loss::SldprtLossCode::HistoryApplicationUnread.note(format!(
        "Keywords member {member}: the Application element {name} and {held} are not features and no document record holds them; they are not read"
    )));
}

pub(crate) fn histories(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<FeatureHistory>, CodecError> {
    scan.sections()
        .try_fold(Vec::new(), |mut histories, section| {
            let source = section.ordinal();
            let stream = section.source_stream();
            let named = names_history(section.name());
            let Some(payload) = section.xml_payload() else {
                if named {
                    report_unread(losses, stream.as_str(), "no XML payload is decoded from it");
                }
                return Ok(histories);
            };
            let Some(text) = crate::container::xml_text(payload) else {
                if named {
                    report_unread(
                        losses,
                        stream.as_str(),
                        "its bytes are not UTF-8 or UTF-16LE text",
                    );
                }
                return Ok(histories);
            };
            let doc = match ctx.parse_xml(text.as_str(), "decode XML tree") {
                Ok(tree) => tree,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    if named {
                        let cause = match error {
                            CodecError::Malformed(message) => {
                                format!("its XML does not parse: {message}")
                            }
                            refused => format!("its XML is refused: {refused}"),
                        };
                        report_unread(losses, stream.as_str(), &cause);
                    }
                    return Ok(histories);
                }
            };
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                return Ok(histories);
            }
            let applications = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Application")
                .collect::<Vec<_>>();
            for application in &applications {
                report_application(losses, stream.as_str(), *application);
            }
            let parent = format!("sldprt:history:feature-history#{source}");
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .try_fold(Vec::new(), |mut configurations, (ordinal, node)| {
                    let id = format!("sldprt:history:configuration#{source}:{ordinal}");
                    crate::annotations::note(
                        annotations,
                        id.as_str(),
                        stream,
                        cadmpeg_core::decode::u64_from_index(node.range().start),
                        "Configuration",
                        Exactness::ByteExact,
                    )?;
                    let properties = keyed_attributes(
                        losses,
                        &id,
                        node.attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "Name" | "Material" | "SourceIndex")
                            })
                            .map(|attribute| (attribute.name(), attribute.value())),
                    );
                    configurations.push(Configuration {
                        id,
                        parent: parent.clone(),
                        ordinal: u32::try_from(ordinal).map_err(|_| {
                            CodecError::malformed(
                                "SLDPRT history configuration ordinal exceeds u32",
                            )
                        })?,
                        source_index: node
                            .attribute("SourceIndex")
                            .and_then(|value| value.parse().ok()),
                        name: node.attribute("Name").unwrap_or("").to_owned(),
                        material: node
                            .attribute("Material")
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned),
                        properties,
                    });
                    Ok::<_, CodecError>(configurations)
                })?;
            let feature_nodes = || {
                root.descendants().filter(|node| {
                    node.is_element()
                        && !matches!(
                            node.tag_name().name(),
                            "Keywords" | "Configuration" | "Dimension"
                        )
                        && !node
                            .ancestors()
                            .any(|ancestor| applications.contains(&ancestor))
                })
            };
            let feature_ids = feature_nodes()
                .enumerate()
                .map(|(ordinal, node)| {
                    (
                        node.range().start,
                        format!("sldprt:history:feature#{source}:{ordinal}"),
                    )
                })
                .collect::<HashMap<_, _>>();
            let features = feature_nodes().enumerate().try_fold(
                Vec::new(),
                |mut features, (ordinal, node)| {
                    let source_id = feature_ids.get(&node.range().start).ok_or_else(|| {
                        CodecError::malformed("missing SLDPRT history feature identity")
                    })?;
                    let id = source_id.clone();
                    crate::annotations::note(
                        annotations,
                        id.as_str(),
                        stream,
                        cadmpeg_core::decode::u64_from_index(node.range().start),
                        node.tag_name().name(),
                        Exactness::ByteExact,
                    )?;
                    let properties = keyed_attributes(
                        losses,
                        &id,
                        node.attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "id" | "Name" | "Type" | "Suppressed")
                            })
                            .map(|attribute| (attribute.name(), attribute.value())),
                    );
                    let content = node
                        .children()
                        .filter_map(|child| {
                            if child.is_text() {
                                let value = child.text().unwrap_or_default().trim();
                                (!value.is_empty()).then(|| FeatureContent::Text(value.to_owned()))
                            } else if !child.is_element() {
                                None
                            } else if child.tag_name().name() == "Dimension" {
                                child
                                    .attribute("Name")
                                    .map(|name| FeatureContent::Dimension(name.to_owned()))
                            } else {
                                feature_ids
                                    .get(&child.range().start)
                                    .map(|id| FeatureContent::Feature(id.clone()))
                            }
                        })
                        .collect::<Vec<_>>();
                    let mut dimension_properties = BTreeMap::new();
                    for dimension in node.children().filter(|child| {
                        child.is_element() && child.tag_name().name() == "Dimension"
                    }) {
                        let Some(name) = dimension.attribute("Name") else {
                            continue;
                        };
                        let properties = keyed_attributes(
                            losses,
                            name,
                            dimension
                                .attributes()
                                .filter(|attribute| attribute.name() != "Name")
                                .map(|attribute| (attribute.name(), attribute.value())),
                        );
                        if properties.is_empty() {
                            continue;
                        }
                        if let Some(previous) = dimension_properties.get_mut(name) {
                            *previous = properties;
                        } else {
                            dimension_properties.insert(name.to_owned(), properties);
                        }
                    }
                    let mut parameters = BTreeMap::new();
                    for dimension in node.children().filter(|child| {
                        child.is_element() && child.tag_name().name() == "Dimension"
                    }) {
                        let Some(name) = dimension.attribute("Name") else {
                            continue;
                        };
                        if name.chars().all(char::is_whitespace) {
                            continue;
                        }
                        let value = dimension.text().unwrap_or_default().trim();
                        if let Some(previous) = parameters.get_mut(name) {
                            value.clone_into(previous);
                        } else if let Some(name) =
                            cadmpeg_core::text::NonBlankString::new(name.to_owned())
                        {
                            parameters.insert(name, value.to_owned());
                        }
                    }
                    let text = if node.children().any(|child| child.is_element()) {
                        None
                    } else {
                        node.text()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                    };
                    features.push(Feature {
                        id,
                        parent: parent.clone(),
                        xml_tag: node.tag_name().name().to_owned(),
                        tree_parent: node.ancestors().skip(1).find_map(|ancestor| {
                            let record_id = feature_ids.get(&ancestor.range().start)?;
                            Some(crate::records::TreeParent::Record {
                                record_id: record_id.clone(),
                                source_id: ancestor
                                    .attribute("id")
                                    .and_then(|value| FeatureSource::try_from(value).ok()),
                            })
                        }),
                        source_id: node
                            .attribute("id")
                            .and_then(|value| FeatureSource::try_from(value).ok()),
                        ordinal: u32::try_from(ordinal).map_err(|_| {
                            CodecError::malformed("SLDPRT history feature ordinal exceeds u32")
                        })?,
                        name: node.attribute("Name").unwrap_or("").to_owned(),
                        kind: node
                            .attribute("Type")
                            .unwrap_or_else(|| node.tag_name().name())
                            .to_owned(),
                        input_class: None,
                        suppressed: node
                            .attribute("Suppressed")
                            .is_some_and(|value| matches!(value, "1" | "true" | "True")),
                        parameters,
                        dimension_properties,
                        properties,
                        text,
                        content,
                    });
                    Ok::<_, CodecError>(features)
                },
            )?;
            let mut configuration_ordinal = 0usize;
            let content = root
                .children()
                .filter_map(|child| {
                    if child.is_text() {
                        let value = child.text().unwrap_or_default().trim();
                        (!value.is_empty()).then(|| HistoryContent::Text(value.to_owned()))
                    } else if !child.is_element() {
                        None
                    } else if child.tag_name().name() == "Configuration" {
                        let id = format!(
                            "sldprt:history:configuration#{source}:{configuration_ordinal}"
                        );
                        configuration_ordinal += 1;
                        Some(HistoryContent::Configuration(id))
                    } else {
                        feature_ids
                            .get(&child.range().start)
                            .map(|id| HistoryContent::Feature(id.clone()))
                    }
                })
                .collect::<Vec<_>>();
            let id = parent;
            crate::annotations::note(
                annotations,
                id.as_str(),
                stream,
                0,
                "Keywords",
                Exactness::ByteExact,
            )?;
            let mut properties = keyed_attributes(
                losses,
                &id,
                root.attributes()
                    .filter(|attribute| attribute.name() != "Name")
                    .map(|attribute| (attribute.name(), attribute.value())),
            );
            let mut spans: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
            for node in feature_nodes() {
                let Some(raw) = node.attribute("id") else {
                    continue;
                };
                if crate::records::FeatureSource::try_from(raw).is_err() {
                    continue;
                }
                let range = node.range();
                let Some(len) = range.end.checked_sub(range.start) else {
                    return Err(CodecError::malformed(
                        "SLDPRT history feature range ends before it starts",
                    ));
                };
                spans
                    .entry(raw.to_owned())
                    .or_default()
                    .push((range.start, len));
            }
            let mut duplicates: Vec<(String, Vec<(usize, usize)>)> = spans
                .into_iter()
                .filter(|(_, occurrences)| occurrences.len() > 1)
                .collect();
            duplicates.sort();
            duplicates.truncate(8);
            if !duplicates.is_empty() {
                let ids = duplicates
                    .iter()
                    .map(|(source, _)| source.clone())
                    .collect::<Vec<_>>()
                    .join(",");
                let span_text = duplicates
                    .iter()
                    .map(|(source, occurrences)| {
                        let list = occurrences
                            .iter()
                            .map(|(offset, len)| format!("{offset}+{len}"))
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("source {source} at {list}")
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                for (key, value) in [
                    ("duplicate_source_ids", ids),
                    ("duplicate_source_spans", span_text),
                ] {
                    if let Some(key) = cadmpeg_core::text::NonBlankString::new(key.to_owned()) {
                        properties.insert(key, value);
                    }
                }
            }
            histories.push(FeatureHistory {
                id,
                part_name: root
                    .attribute("Name")
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                properties,
                content,
                configurations,
                features,
            });
            Ok(histories)
        })
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
