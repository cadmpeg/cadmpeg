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

const HISTORY_OPERATION: &str = "decode SLDPRT history XML";

/// Keys one source element's attributes, charging every key the reader cannot
/// key.
///
/// The element is still transferred. A key holding no non-whitespace character
/// cannot be asked for, and a key the element states twice is already taken, so
/// the charge names the element and, for a restated key, the key itself.
/// Attributes named in `excluded` are not properties.
fn keyed_attributes(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    record: &str,
    node: roxmltree::Node<'_, '_>,
    excluded: &[&str],
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut kept = BTreeMap::new();
    let mut attributes = node.attributes();
    while let Some(attribute) =
        ctx.next_charged(&mut attributes, "scan SLDPRT element attributes")?
    {
        let (name, value) = (attribute.name(), attribute.value());
        if excluded.contains(&name) {
            continue;
        }
        if ctx.all_by(
            name.chars(),
            |character| Ok(character.is_whitespace()),
            "scan SLDPRT property name characters",
        )? {
            report_unkeyed_property(ctx, losses, record, None)?;
            continue;
        }
        if ctx.contains_key_btree_map(&kept, name, "test SLDPRT map key")? {
            report_unkeyed_property(ctx, losses, record, Some(name))?;
            continue;
        }
        let name = ctx.copy_retained_text(name, "retain SLDPRT history property name")?;
        let value = ctx.copy_retained_text(value, "retain SLDPRT history property value")?;
        if let Some(name) =
            cadmpeg_core::text::NonBlankString::for_decode(ctx, name, "validate nonblank text")?
        {
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
    const OPERATION: &str = "retain SLDPRT history property loss";
    const BLANK: &str = " states a property with a blank key; the property is not transferred";
    const RESTATED_BEFORE: &str = " states the property ";
    const RESTATED_AFTER: &str = " a second time; the property is not transferred";
    let message = match duplicate {
        Some(key) => ctx.format_retained(
            format_args!("{record}{RESTATED_BEFORE}{key}{RESTATED_AFTER}"),
            OPERATION,
        )?,
        None => ctx.format_retained(format_args!("{record}{BLANK}"), OPERATION)?,
    };
    ctx.push_vec(
        losses,
        crate::loss::SldprtLossCode::SourcePropertyKeyBlank.note(message),
        "collect SLDPRT history property losses",
    )
}

/// The text of a text node without surrounding whitespace, when any remains.
fn trimmed_text<'a>(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'a, '_>,
) -> Result<Option<&'a str>, CodecError> {
    let Some(text) = node.text() else {
        return Ok(None);
    };
    let text = ctx.trim_text(text, "trim SLDPRT history text")?;
    Ok((!text.is_empty()).then_some(text))
}

fn is_element_named(node: roxmltree::Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

pub(crate) fn histories(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<FeatureHistory>, CodecError> {
    let mut histories = Vec::new();
    for section in scan.sections(ctx)? {
        let source = section.ordinal();
        let Some(text) = crate::container::xml_text_charged(
            ctx,
            section.payload(),
            "materialize SLDPRT history XML",
        )?
        else {
            continue;
        };
        let admitted_doc = match ctx.parse_xml(text.as_str(), "decode XML tree") {
            Ok(tree) => tree,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => continue,
        };
        let root = ctx.xml_root_element(admitted_doc.document(), HISTORY_OPERATION)?;
        if !ctx.contains_text(root.tag_name().name(), "Keywords", HISTORY_OPERATION)? {
            continue;
        }
        let history = history(
            ctx,
            source,
            section.source_stream(),
            root,
            annotations,
            losses,
        )?;
        ctx.push_vec(&mut histories, history, "collect SLDPRT feature histories")?;
    }
    Ok(histories)
}

/// Decode one Keywords document into its history record.
fn history(
    ctx: &DecodeContext<'_>,
    source: usize,
    stream: &cadmpeg_ir::StreamName,
    root: roxmltree::Node<'_, '_>,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<FeatureHistory, CodecError> {
    let parent = ctx.format_retained(
        format_args!("sldprt:history:feature-history#{source}"),
        "retain SLDPRT history identity",
    )?;
    let mut configurations = Vec::new();
    let mut children = root.children();
    while let Some(node) = ctx.next_charged(&mut children, HISTORY_OPERATION)? {
        if !is_element_named(node, "Configuration") {
            continue;
        }
        let ordinal = configurations.len();
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
        let properties =
            keyed_attributes(ctx, losses, &id, node, &["Name", "Material", "SourceIndex"])?;
        let source_index = match ctx.xml_attribute(node, "SourceIndex", HISTORY_OPERATION)? {
            Some(value) => ctx.parse_text(value, HISTORY_OPERATION)?.ok(),
            None => None,
        };
        let name = ctx
            .xml_attribute(node, "Name", HISTORY_OPERATION)?
            .unwrap_or("");
        let material = match ctx.xml_attribute(node, "Material", HISTORY_OPERATION)? {
            Some(value) if !value.is_empty() => {
                Some(ctx.copy_retained_text(value, "retain SLDPRT configuration material")?)
            }
            _ => None,
        };
        let configuration = Configuration {
            id,
            parent: ctx.copy_retained_text(&parent, "retain SLDPRT history parent identity")?,
            ordinal: u32::try_from(ordinal).map_err(|_| {
                ctx.refuse_codec_limit(
                    "index SLDPRT history configuration ordinals",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?,
            source_index,
            name: ctx.copy_retained_text(name, "retain SLDPRT configuration name")?,
            material,
            properties,
        };
        ctx.push_vec(
            &mut configurations,
            configuration,
            "collect SLDPRT history configurations",
        )?;
    }
    let is_feature_node = |node: &roxmltree::Node<'_, '_>| {
        node.is_element()
            && !matches!(
                node.tag_name().name(),
                "Keywords" | "Configuration" | "Dimension"
            )
    };
    let mut feature_ids = HashMap::new();
    let mut ordinal = 0_usize;
    let mut descendants = root.descendants();
    while let Some(node) = ctx.next_charged(&mut descendants, HISTORY_OPERATION)? {
        if !is_feature_node(&node) {
            continue;
        }
        let id = ctx.format_retained(
            format_args!("sldprt:history:feature#{source}:{ordinal}"),
            "retain SLDPRT feature identity",
        )?;
        ctx.insert_hash_map(
            &mut feature_ids,
            node.range().start,
            id,
            "index SLDPRT history feature IDs",
        )?;
        ordinal += 1;
    }
    let mut features = Vec::new();
    let mut descendants = root.descendants();
    while let Some(node) = ctx.next_charged(&mut descendants, HISTORY_OPERATION)? {
        if !is_feature_node(&node) {
            continue;
        }
        let scope = DocumentScope {
            parent: &parent,
            stream,
            feature_ids: &feature_ids,
        };
        let feature = feature(ctx, &scope, node, features.len(), annotations, losses)?;
        ctx.push_vec(&mut features, feature, "collect SLDPRT history features")?;
    }
    let mut configuration_ordinal = 0usize;
    let mut content = Vec::new();
    let mut children = root.children();
    while let Some(child) = ctx.next_charged(&mut children, HISTORY_OPERATION)? {
        let item = if child.is_text() {
            trimmed_text(ctx, child)?
                .map(|value| {
                    ctx.copy_retained_text(value, "retain SLDPRT history content text")
                        .map(HistoryContent::Text)
                })
                .transpose()?
        } else if !child.is_element() {
            None
        } else if child.tag_name().name() == "Configuration" {
            let id = ctx.format_retained(
                format_args!("sldprt:history:configuration#{source}:{configuration_ordinal}"),
                "retain SLDPRT configuration content identity",
            )?;
            configuration_ordinal += 1;
            Some(HistoryContent::Configuration(id))
        } else {
            ctx.get_hash_map(&feature_ids, &child.range().start, HISTORY_OPERATION)?
                .map(|id| {
                    ctx.copy_retained_text(id, "retain SLDPRT history content identity")
                        .map(HistoryContent::Feature)
                })
                .transpose()?
        };
        if let Some(item) = item {
            ctx.push_vec(&mut content, item, "collect SLDPRT history content")?;
        }
    }
    crate::annotations::note(
        ctx,
        annotations,
        parent.as_str(),
        stream,
        0,
        "Keywords",
        Exactness::ByteExact,
    )?;
    let properties = keyed_attributes(ctx, losses, &parent, root, &["Name"])?;
    let part_name = match ctx.xml_attribute(root, "Name", HISTORY_OPERATION)? {
        Some(value) if !value.is_empty() => {
            Some(ctx.copy_retained_text(value, "retain SLDPRT history part name")?)
        }
        _ => None,
    };
    Ok(FeatureHistory {
        id: parent,
        part_name,
        properties,
        content,
        configurations,
        features,
    })
}

/// What every feature element of one Keywords document shares.
struct DocumentScope<'a> {
    /// The history record identity.
    parent: &'a str,
    stream: &'a cadmpeg_ir::StreamName,
    /// Record identities by element start offset.
    feature_ids: &'a HashMap<usize, String>,
}

/// Decode one feature element.
fn feature(
    ctx: &DecodeContext<'_>,
    scope: &DocumentScope<'_>,
    node: roxmltree::Node<'_, '_>,
    ordinal: usize,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<Feature, CodecError> {
    let DocumentScope {
        parent,
        stream,
        feature_ids,
    } = *scope;
    let source_id = ctx
        .get_hash_map(feature_ids, &node.range().start, HISTORY_OPERATION)?
        .ok_or_else(|| CodecError::malformed("missing SLDPRT history feature identity"))?;
    let id = ctx.copy_retained_text(source_id, "retain SLDPRT feature identity")?;
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
        node,
        &["id", "Name", "Type", "Suppressed"],
    )?;
    let mut content = Vec::new();
    let mut dimension_properties = BTreeMap::new();
    let mut parameters = BTreeMap::new();
    let mut has_element_child = false;
    let mut children = node.children();
    while let Some(child) = ctx.next_charged(&mut children, HISTORY_OPERATION)? {
        has_element_child |= child.is_element();
        let item = if child.is_text() {
            trimmed_text(ctx, child)?
                .map(|value| {
                    ctx.copy_retained_text(value, "retain SLDPRT feature content text")
                        .map(FeatureContent::Text)
                })
                .transpose()?
        } else if !child.is_element() {
            None
        } else if child.tag_name().name() == "Dimension" {
            let name = ctx.xml_attribute(child, "Name", HISTORY_OPERATION)?;
            if let Some(name) = name {
                dimension(
                    ctx,
                    losses,
                    child,
                    name,
                    &mut dimension_properties,
                    &mut parameters,
                )?;
            }
            name.map(|name| {
                ctx.copy_retained_text(name, "retain SLDPRT feature dimension content")
                    .map(FeatureContent::Dimension)
            })
            .transpose()?
        } else {
            ctx.get_hash_map(feature_ids, &child.range().start, HISTORY_OPERATION)?
                .map(|id| {
                    ctx.copy_retained_text(id, "retain SLDPRT feature content identity")
                        .map(FeatureContent::Feature)
                })
                .transpose()?
        };
        if let Some(item) = item {
            ctx.push_vec(&mut content, item, "collect SLDPRT feature content")?;
        }
    }
    let text = if has_element_child {
        None
    } else {
        trimmed_text(ctx, node)?
            .map(|value| ctx.copy_retained_text(value, "retain SLDPRT feature text"))
            .transpose()?
    };
    let tree_parent = ctx.find_map(
        node.ancestors().skip(1),
        |ancestor| {
            let Some(record_id) =
                ctx.get_hash_map(feature_ids, &ancestor.range().start, HISTORY_OPERATION)?
            else {
                return Ok(None);
            };
            let record_id = ctx.copy_retained_text(record_id, "retain SLDPRT ancestor identity")?;
            let source_id = ctx
                .xml_attribute(ancestor, "id", HISTORY_OPERATION)?
                .and_then(|value| FeatureSource::try_from(value).ok());
            Ok(Some(crate::records::TreeParent::Record {
                record_id,
                source_id,
            }))
        },
        HISTORY_OPERATION,
    )?;
    let tag = node.tag_name().name();
    let kind = ctx
        .xml_attribute(node, "Type", HISTORY_OPERATION)?
        .unwrap_or(tag);
    Ok(Feature {
        id,
        parent: ctx.copy_retained_text(parent, "retain SLDPRT history parent identity")?,
        xml_tag: ctx.copy_retained_text(tag, "retain SLDPRT feature XML tag")?,
        tree_parent,
        source_id: ctx
            .xml_attribute(node, "id", HISTORY_OPERATION)?
            .and_then(|value| FeatureSource::try_from(value).ok()),
        ordinal: u32::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit(
                "index SLDPRT history feature ordinals",
                u64::MAX - 1,
                u64::MAX,
            )
        })?,
        name: ctx.copy_retained_text(
            ctx.xml_attribute(node, "Name", HISTORY_OPERATION)?
                .unwrap_or(""),
            "retain SLDPRT feature name",
        )?,
        kind: ctx.copy_retained_text(kind, "retain SLDPRT feature kind")?,
        input_class: None,
        suppressed: ctx
            .xml_attribute(node, "Suppressed", HISTORY_OPERATION)?
            .is_some_and(|value| matches!(value, "1" | "true" | "True")),
        parameters,
        dimension_properties,
        properties,
        text,
        content,
    })
}

/// Record one named dimension child: its value as a parameter and its other
/// attributes as dimension properties. A later dimension of the same name
/// replaces an earlier one.
fn dimension(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
    dimension_properties: &mut BTreeMap<
        String,
        BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    >,
    parameters: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<(), CodecError> {
    let properties = keyed_attributes(ctx, losses, name, node, &["Name"])?;
    if !properties.is_empty() {
        if let Some(previous) = ctx.get_mut_btree_map(
            dimension_properties,
            name,
            "look up mutable SLDPRT ordered key",
        )? {
            *previous = properties;
        } else {
            let name = ctx.copy_retained_text(name, "retain SLDPRT dimension property name")?;
            ctx.insert_btree_map(
                dimension_properties,
                name,
                properties,
                "index SLDPRT dimension properties",
            )?;
        }
    }
    if ctx.all_by(
        name.chars(),
        |character| Ok(character.is_whitespace()),
        "scan SLDPRT dimension name characters",
    )? {
        return Ok(());
    }
    let value = match node.text() {
        Some(text) => ctx.trim_text(text, "trim SLDPRT history text")?,
        None => "",
    };
    if let Some(previous) =
        ctx.get_mut_btree_map(parameters, name, "look up mutable SLDPRT ordered key")?
    {
        *previous = ctx.copy_retained_text(value, "retain SLDPRT parameter value")?;
        return Ok(());
    }
    let name = ctx.copy_retained_text(name, "retain SLDPRT parameter name")?;
    let value = ctx.copy_retained_text(value, "retain SLDPRT parameter value")?;
    if let Some(name) =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, name, "validate nonblank text")?
    {
        ctx.insert_btree_map(parameters, name, value, "index SLDPRT parameters")?;
    }
    Ok(())
}

pub(crate) fn enrich_scene_classes(
    ctx: &DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    scene_classes: &HashMap<u32, String>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "retain SLDPRT scene feature class";
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&mut history.features, OPERATION)? {
            let Some(source) = feature.source_value() else {
                continue;
            };
            if feature.input_class.is_none() && classless_builtin_node(feature) {
                feature.input_class = scene_classes
                    .get(&source)
                    .map(|name| ctx.copy_retained_text(name, OPERATION))
                    .transpose()?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(in crate::history) mod tests;
