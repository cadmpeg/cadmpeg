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
        let name = ctx.format_retained(
            format_args!("{name}"),
            "retain SLDPRT history property name",
        )?;
        let value = ctx.format_retained(
            format_args!("{value}"),
            "retain SLDPRT history property value",
        )?;
        if let Some(name) = cadmpeg_core::text::NonBlankString::for_decode(ctx, name, "validate nonblank text")? {
            ctx.insert_btree_map(&mut kept, name, value, "index SLDPRT history properties")?;
        }
    }
    Ok(kept)
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
                ctx.refuse_codec_limit(
                    "retain SLDPRT history property loss",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?,
        None => BLANK.len(),
    };
    let needed = record.len().checked_add(extra).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "retain SLDPRT history property loss",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    let mut message = String::new();
    ctx.try_reserve_retained_text(&mut message, needed, "retain SLDPRT history property loss")?;
    match duplicate {
        Some(key) => write!(message, "{record}{RESTATED_BEFORE}{key}{RESTATED_AFTER}"),
        None => write!(message, "{record}{BLANK}"),
    }
    .map_err(|_| {
        ctx.refuse_codec_limit(
            "retain SLDPRT history property loss",
            u64::MAX - 1,
            u64::MAX,
        )
    })?;
    ctx.reserve_vec(losses, 1, "collect SLDPRT history property losses")?;
    losses.push(crate::loss::SldprtLossCode::SourcePropertyKeyBlank.note(message));
    Ok(())
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
            )?
            else {
                return Ok(histories);
            };
            let admitted_doc = match ctx.parse_xml(text.as_str(), "decode XML tree") {
                Ok(tree) => tree,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => {
                    return Ok(histories);
                }
            };
            let doc = admitted_doc.document();
            let root = doc.root_element();
            if !root.tag_name().name().contains("Keywords") {
                return Ok(histories);
            }
            let stream = section.source_stream();
            let parent = ctx.format_retained(
                format_args!("sldprt:history:feature-history#{source}"),
                "retain SLDPRT history identity",
            )?;
            let configurations = root
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "Configuration")
                .enumerate()
                .try_fold(Vec::new(), |mut configurations, (ordinal, node)| {
                    let id = ctx.format_retained(
                        format_args!("sldprt:history:configuration#{source}:{ordinal}"),
                        "retain SLDPRT configuration identity",
                    )?;
                    crate::annotations::note(
                        ctx,
                        annotations,
                        id.as_str(),
                        stream,
                        cadmpeg_core::decode::u64_from_index(node.range().start),
                        "Configuration",
                        Exactness::ByteExact,
                    )?;
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
                    ctx.reserve_vec(
                        &mut configurations,
                        1,
                        "collect SLDPRT history configurations",
                    )?;
                    configurations.push(Configuration {
                        id,
                        parent: ctx.format_retained(
                            format_args!("{parent}"),
                            "retain SLDPRT history parent identity",
                        )?,
                        ordinal: u32::try_from(ordinal).map_err(|_| {
                            ctx.refuse_codec_limit(
                                "index SLDPRT history configuration ordinals",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?,
                        source_index: node
                            .attribute("SourceIndex")
                            .and_then(|value| value.parse().ok()),
                        name: ctx.format_retained(
                            format_args!("{}", node.attribute("Name").unwrap_or("")),
                            "retain SLDPRT configuration name",
                        )?,
                        material: node
                            .attribute("Material")
                            .filter(|value| !value.is_empty())
                            .map(|value| {
                                ctx.format_retained(
                                    format_args!("{value}"),
                                    "retain SLDPRT configuration material",
                                )
                            })
                            .transpose()?,
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
            let feature_ids = feature_nodes().enumerate().try_fold(
                HashMap::new(),
                |mut ids, (ordinal, node)| {
                    ctx.insert_hash_map(
                        &mut ids,
                        node.range().start,
                        ctx.format_retained(
                            format_args!("sldprt:history:feature#{source}:{ordinal}"),
                            "retain SLDPRT feature identity",
                        )?,
                        "index SLDPRT history feature IDs",
                    )?;
                    Ok::<_, CodecError>(ids)
                },
            )?;
            let features = feature_nodes().enumerate().try_fold(
                Vec::new(),
                |mut features, (ordinal, node)| {
                    let source_id = feature_ids.get(&node.range().start).ok_or_else(|| {
                        CodecError::malformed("missing SLDPRT history feature identity")
                    })?;
                    let id = ctx.format_retained(
                        format_args!("{source_id}"),
                        "retain SLDPRT feature identity",
                    )?;
                    crate::annotations::note(
                        ctx,
                        annotations,
                        id.as_str(),
                        stream,
                        cadmpeg_core::decode::u64_from_index(node.range().start),
                        node.tag_name().name(),
                        Exactness::ByteExact,
                    )?;
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
                    ctx.reserve_vec(&mut features, 1, "collect SLDPRT history features")?;
                    let content = node.children().try_fold(Vec::new(), |mut content, child| {
                        let item = if child.is_text() {
                            let value = child.text().unwrap_or_default().trim();
                            (!value.is_empty())
                                .then(|| {
                                    ctx.format_retained(
                                        format_args!("{value}"),
                                        "retain SLDPRT feature content text",
                                    )
                                    .map(FeatureContent::Text)
                                })
                                .transpose()?
                        } else if !child.is_element() {
                            None
                        } else if child.tag_name().name() == "Dimension" {
                            child
                                .attribute("Name")
                                .map(|name| {
                                    ctx.format_retained(
                                        format_args!("{name}"),
                                        "retain SLDPRT feature dimension content",
                                    )
                                    .map(FeatureContent::Dimension)
                                })
                                .transpose()?
                        } else {
                            feature_ids
                                .get(&child.range().start)
                                .map(|id| {
                                    ctx.format_retained(
                                        format_args!("{id}"),
                                        "retain SLDPRT feature content identity",
                                    )
                                    .map(FeatureContent::Feature)
                                })
                                .transpose()?
                        };
                        if let Some(item) = item {
                            ctx.reserve_vec(&mut content, 1, "collect SLDPRT feature content")?;
                            content.push(item);
                        }
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
                            let name = ctx.format_retained(
                                format_args!("{name}"),
                                "retain SLDPRT dimension property name",
                            )?;
                            ctx.insert_btree_map(
                                &mut dimension_properties,
                                name,
                                properties,
                                "index SLDPRT dimension properties",
                            )?;
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
                            *previous = ctx.format_retained(
                                format_args!("{value}"),
                                "retain SLDPRT parameter value",
                            )?;
                        } else {
                            let name = ctx.format_retained(
                                format_args!("{name}"),
                                "retain SLDPRT parameter name",
                            )?;
                            let value = ctx.format_retained(
                                format_args!("{value}"),
                                "retain SLDPRT parameter value",
                            )?;
                            if let Some(name) = cadmpeg_core::text::NonBlankString::for_decode(ctx, name, "validate nonblank text")? {
                                ctx.insert_btree_map(
                                    &mut parameters,
                                    name,
                                    value,
                                    "index SLDPRT parameters",
                                )?;
                            }
                        }
                    }
                    let text = if node.children().any(|child| child.is_element()) {
                        None
                    } else {
                        node.text()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|value| {
                                ctx.format_retained(
                                    format_args!("{value}"),
                                    "retain SLDPRT feature text",
                                )
                            })
                            .transpose()?
                    };
                    features.push(Feature {
                        id,
                        parent: ctx.format_retained(
                            format_args!("{parent}"),
                            "retain SLDPRT history parent identity",
                        )?,
                        xml_tag: ctx.format_retained(
                            format_args!("{}", node.tag_name().name()),
                            "retain SLDPRT feature XML tag",
                        )?,
                        tree_parent: node
                            .ancestors()
                            .skip(1)
                            .find_map(|ancestor| {
                                let record_id = feature_ids.get(&ancestor.range().start)?;
                                Some(
                                    ctx.format_retained(
                                        format_args!("{record_id}"),
                                        "retain SLDPRT ancestor identity",
                                    )
                                    .map(|record_id| {
                                        crate::records::TreeParent::Record {
                                            record_id,
                                            source_id: ancestor.attribute("id").and_then(|value| {
                                                FeatureSource::try_from(value).ok()
                                            }),
                                        }
                                    }),
                                )
                            })
                            .transpose()?,
                        source_id: node
                            .attribute("id")
                            .and_then(|value| FeatureSource::try_from(value).ok()),
                        ordinal: u32::try_from(ordinal).map_err(|_| {
                            ctx.refuse_codec_limit(
                                "index SLDPRT history feature ordinals",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?,
                        name: ctx.format_retained(
                            format_args!("{}", node.attribute("Name").unwrap_or("")),
                            "retain SLDPRT feature name",
                        )?,
                        kind: ctx.format_retained(
                            format_args!(
                                "{}",
                                node.attribute("Type")
                                    .unwrap_or_else(|| node.tag_name().name())
                            ),
                            "retain SLDPRT feature kind",
                        )?,
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
            let content = root.children().try_fold(Vec::new(), |mut content, child| {
                let item = if child.is_text() {
                    let value = child.text().unwrap_or_default().trim();
                    (!value.is_empty())
                        .then(|| {
                            ctx.format_retained(
                                format_args!("{value}"),
                                "retain SLDPRT history content text",
                            )
                            .map(HistoryContent::Text)
                        })
                        .transpose()?
                } else if !child.is_element() {
                    None
                } else if child.tag_name().name() == "Configuration" {
                    let id = ctx.format_retained(
                        format_args!(
                            "sldprt:history:configuration#{source}:{configuration_ordinal}"
                        ),
                        "retain SLDPRT configuration content identity",
                    )?;
                    configuration_ordinal =
                        configuration_ordinal.checked_add(1).ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "number SLDPRT configuration content",
                                u64::MAX,
                                u64::MAX,
                            )
                        })?;
                    Some(HistoryContent::Configuration(id))
                } else {
                    feature_ids
                        .get(&child.range().start)
                        .map(|id| {
                            ctx.format_retained(
                                format_args!("{id}"),
                                "retain SLDPRT history content identity",
                            )
                            .map(HistoryContent::Feature)
                        })
                        .transpose()?
                };
                if let Some(item) = item {
                    ctx.reserve_vec(&mut content, 1, "collect SLDPRT history content")?;
                    content.push(item);
                }
                Ok::<_, CodecError>(content)
            })?;
            let id = parent;
            crate::annotations::note(
                ctx,
                annotations,
                id.as_str(),
                stream,
                0,
                "Keywords",
                Exactness::ByteExact,
            )?;
            let properties = keyed_attributes(
                ctx,
                losses,
                &id,
                root.attributes()
                    .filter(|attribute| attribute.name() != "Name")
                    .map(|attribute| (attribute.name(), attribute.value())),
            )?;
            ctx.reserve_vec(&mut histories, 1, "collect SLDPRT feature histories")?;
            histories.push(FeatureHistory {
                id,
                part_name: root
                    .attribute("Name")
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        ctx.format_retained(
                            format_args!("{value}"),
                            "retain SLDPRT history part name",
                        )
                    })
                    .transpose()?,
                properties,
                content,
                configurations,
                features,
            });
            Ok(histories)
        })
}

pub(crate) fn enrich_scene_classes(
    ctx: &DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    scene_classes: &HashMap<u32, String>,
) -> Result<(), CodecError> {
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let Some(source) = feature.source_value() else {
            continue;
        };
        if feature.input_class.is_none() && classless_builtin_node(feature) {
            feature.input_class = scene_classes
                .get(&source)
                .map(|name| {
                    ctx.format_retained(format_args!("{name}"), "retain SLDPRT scene feature class")
                })
                .transpose()?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(in crate::history) mod tests;
