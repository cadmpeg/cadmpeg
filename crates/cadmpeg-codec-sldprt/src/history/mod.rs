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
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

/// Keys one source element's attributes, charging every key the reader cannot
/// key.
///
/// The element is still transferred. A key holding no non-whitespace character
/// cannot be asked for, and a key the element states twice is already taken, so
/// the charge names the element and, for a restated key, the key itself.
fn keyed_attributes<'name, 'value>(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    record: &str,
    entries: impl IntoIterator<Item = (&'name str, &'value str)>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut kept = BTreeMap::new();
    for (name, value) in entries {
        if name.chars().all(char::is_whitespace) {
            report_unkeyed_property(ctx, losses, record, None)?;
            continue;
        }
        if kept.contains_key(name) {
            report_unkeyed_property(ctx, losses, record, Some(name))?;
            continue;
        }
        let name = copy_history_text(ctx, name, "retain SLDPRT history property name")?;
        let value = copy_history_text(ctx, value, "retain SLDPRT history property value")?;
        if let Some(name) = cadmpeg_core::text::NonBlankString::new(name) {
            ctx.charge_collection_items(1, "index SLDPRT history properties")?;
            kept.insert(name, value);
        }
    }
    Ok(kept)
}

fn copy_history_text(
    ctx: &DecodeContext<'_>,
    source: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut copy = String::new();
    ctx.reserve_retained_string(&mut copy, source.len(), operation)?;
    copy.push_str(source);
    Ok(copy)
}

fn report_unkeyed_property(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    record: &str,
    duplicate: Option<&str>,
) -> Result<(), CodecError> {
    const BLANK: &str = " states a property with a blank key; the property is not transferred";
    const RESTATED_BEFORE: &str = " states the property ";
    const RESTATED_AFTER: &str = " a second time; the property is not transferred";
    let extra = match duplicate {
        Some(key) => [RESTATED_BEFORE.len(), key.len(), RESTATED_AFTER.len()]
            .into_iter()
            .try_fold(0_usize, usize::checked_add)
            .ok_or_else(|| {
                ctx.refuse_codec_limit("retain SLDPRT history property loss", u64::MAX - 1, u64::MAX)
            })?,
        None => BLANK.len(),
    };
    let needed = record.len().checked_add(extra).ok_or_else(|| {
        ctx.refuse_codec_limit("retain SLDPRT history property loss", u64::MAX - 1, u64::MAX)
    })?;
    let mut message = String::new();
    ctx.reserve_retained_string(
        &mut message,
        needed,
        "retain SLDPRT history property loss",
    )?;
    match duplicate {
        Some(key) => write!(message, "{record}{RESTATED_BEFORE}{key}{RESTATED_AFTER}"),
        None => write!(message, "{record}{BLANK}"),
    }
    .map_err(|_| {
        ctx.refuse_codec_limit("retain SLDPRT history property loss", u64::MAX - 1, u64::MAX)
    })?;
    ctx.reserve_collection_vec(losses, 1, "collect SLDPRT history property losses")?;
    losses.push(crate::loss::SldprtLossCode::SourcePropertyKeyBlank.note(message));
    Ok(())
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<FeatureHistory>, CodecError> {
    scan.sections()
        .try_fold(Vec::new(), |mut histories, section| {
            let source = section.ordinal();
            let Some(text) = crate::container::xml_text_charged(
                ctx,
                section.payload(),
                "materialize SLDPRT history XML",
            )? else {
                return Ok(histories);
            };
            let Ok(doc) = roxmltree::Document::parse(text.as_str()) else {
                return Ok(histories);
            };
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                return Ok(histories);
            }
            let stream = section.source_stream();
            let parent = format!("sldprt:history:feature-history#{source}");
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .try_fold(Vec::new(), |mut configurations, (ordinal, node)| {
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
                        ctx,
                        losses,
                        &id,
                        node.attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "Name" | "Material" | "SourceIndex")
                            })
                            .map(|attribute| (attribute.name(), attribute.value())),
                    )?;
                    ctx.reserve_collection_vec(
                        &mut configurations,
                        1,
                        "collect SLDPRT history configurations",
                    )?;
                    configurations.push(Configuration {
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
                })
            };
            let feature_ids = feature_nodes()
                .enumerate()
                .try_fold(HashMap::new(), |mut ids, (ordinal, node)| {
                    ctx.charge_collection_items(1, "index SLDPRT history feature IDs")?;
                    ids.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit("index SLDPRT history feature IDs", u64::MAX - 1, u64::MAX)
                    })?;
                    ids.insert(
                        node.range().start,
                        format!(
                            "sldprt:history:feature#{}",
                            history_record_key(source, ordinal)
                        ),
                    );
                    Ok::<_, CodecError>(ids)
                })?;
            let features = feature_nodes()
                .enumerate()
                .try_fold(Vec::new(), |mut features, (ordinal, node)| {
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
                        ctx,
                        losses,
                        &id,
                        node.attributes()
                            .filter(|attribute| {
                                !matches!(attribute.name(), "id" | "Name" | "Type" | "Suppressed")
                            })
                            .map(|attribute| (attribute.name(), attribute.value())),
                    )?;
                    ctx.reserve_collection_vec(
                        &mut features,
                        1,
                        "collect SLDPRT history features",
                    )?;
                    let content = node
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
                        .try_fold(Vec::new(), |mut content, item| {
                            ctx.reserve_collection_vec(
                                &mut content,
                                1,
                                "collect SLDPRT feature content",
                            )?;
                            content.push(item);
                            Ok::<_, CodecError>(content)
                        })?;
                    let mut dimension_properties = BTreeMap::new();
                    for dimension in node.children().filter(|child| {
                        child.is_element() && child.tag_name().name() == "Dimension"
                    }) {
                        let Some(name) = dimension.attribute("Name") else {
                            continue;
                        };
                        let properties = keyed_attributes(
                            ctx,
                            losses,
                            name,
                            dimension
                                .attributes()
                                .filter(|attribute| attribute.name() != "Name")
                                .map(|attribute| (attribute.name(), attribute.value())),
                        )?;
                        if properties.is_empty() {
                            continue;
                        }
                        if let Some(previous) = dimension_properties.get_mut(name) {
                            *previous = properties;
                        } else {
                            let name = copy_history_text(
                                ctx,
                                name,
                                "retain SLDPRT dimension property name",
                            )?;
                            ctx.charge_collection_items(
                                1,
                                "index SLDPRT dimension properties",
                            )?;
                            dimension_properties.insert(name, properties);
                        }
                    }
                    features.push(Feature {
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
                        dimension_properties,
                        properties,
                        text: (!node.children().any(|child| child.is_element()))
                            .then(|| node.text().map(str::trim).unwrap_or_default().to_string())
                            .filter(|value| !value.is_empty()),
                        content,
                    });
                    Ok::<_, CodecError>(features)
                })?;
            let mut configuration_ordinal = 0;
            let content = root
                .children()
                .try_fold(Vec::new(), |mut content, child| {
                    let item = (|| {
                        if child.is_text() {
                            let value = child.text()?.trim();
                            return (!value.is_empty())
                                .then(|| HistoryContent::Text(value.into()));
                        }
                        if !child.is_element() {
                            return None;
                        }
                        if child.tag_name().name() == "Configuration" {
                            let id = format!(
                                "sldprt:history:configuration#{}",
                                history_record_key(source, configuration_ordinal)
                            );
                            configuration_ordinal += 1;
                            return Some(HistoryContent::Configuration(id));
                        }
                        feature_ids
                            .get(&child.range().start)
                            .cloned()
                            .map(HistoryContent::Feature)
                    })();
                    if let Some(item) = item {
                        ctx.reserve_collection_vec(
                            &mut content,
                            1,
                            "collect SLDPRT history content",
                        )?;
                        content.push(item);
                    }
                    Ok::<_, CodecError>(content)
                })?;
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
                ctx,
                losses,
                &id,
                root.attributes()
                    .filter(|attribute| attribute.name() != "Name")
                    .map(|attribute| (attribute.name(), attribute.value())),
            )?;
            ctx.reserve_collection_vec(&mut histories, 1, "collect SLDPRT feature histories")?;
            histories.push(FeatureHistory {
                id,
                part_name: root
                    .attribute("Name")
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
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
