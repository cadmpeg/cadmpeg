// SPDX-License-Identifier: Apache-2.0
//! Generic `FreeCAD` object and property graph recovery.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::native::{
    DynamicPropertyMeta, ExtensionRecord, LinkTarget, ObjectRecord, PropertyFamily, PropertyRecord,
    ValueRecord,
};

const MAX_OBJECTS: usize = 1_000_000;
const MAX_PROPERTY_VALUE_XML_BYTES: usize = 16 * 1024 * 1024;

struct DependencyInfo {
    dependencies: Vec<String>,
    allow_partial: Option<i64>,
    order: usize,
}

/// Recovered persistence graph.
pub struct Graph {
    /// Declared objects.
    pub objects: Vec<ObjectRecord>,
    /// Dynamic extensions.
    pub extensions: Vec<ExtensionRecord>,
    /// Document and object properties.
    pub properties: Vec<PropertyRecord>,
}

/// Recover the persistence graph without interpreting geometry.
pub fn parse(bytes: &[u8]) -> Result<Graph, CodecError> {
    parse_with_context(bytes, None)
}

/// Recover the persistence graph, charging retained property XML against the session.
pub fn parse_with_context(
    bytes: &[u8],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Graph, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::malformed(format_args!("invalid Document.xml: {error}")))?;
    let root = xml.root_element();
    let schema = crate::container::canonical_attribute(root, "SchemaVersion", "schemaVersion")?
        .ok_or_else(|| {
            CodecError::Malformed("Document element has no SchemaVersion attribute".into())
        })?;
    let (declarations_tag, data_tag, record_tag) = match schema.as_str() {
        "2" => ("Features", "FeatureData", "Feature"),
        "3" | "4" => ("Objects", "ObjectData", "Object"),
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "FCStd SchemaVersion={schema} persistence layout"
            )));
        }
    };
    let objects_node = unique_section(root, declarations_tag)?;
    let data_node = unique_section(root, data_tag)?;

    let declared_count = objects_node
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "{declarations_tag} Count is missing or invalid"
            ))
        })?;
    let object_limit = ctx
        .and_then(|ctx| usize::try_from(ctx.policy().limits.max_entities).ok())
        .map_or(MAX_OBJECTS, |policy| policy.min(MAX_OBJECTS));
    if declared_count > object_limit {
        return Err(CodecError::Malformed("object count limit exceeded".into()));
    }
    if schema == "2" && objects_node.attribute("Dependencies").is_some() {
        return Err(CodecError::Malformed(
            "schema 2 Features cannot carry object dependencies".into(),
        ));
    }

    let dependencies_enabled = schema != "2" && objects_node.attribute("Dependencies").is_some();
    let mut saw_object_declaration = false;
    for child in objects_node.children().filter(roxmltree::Node::is_element) {
        if child.has_tag_name(record_tag) {
            saw_object_declaration = true;
        } else if saw_object_declaration && child.has_tag_name("ObjectDeps") {
            return Err(CodecError::Malformed(
                "ObjectDeps records must precede object declarations".into(),
            ));
        }
    }
    let dependency_nodes = objects_node
        .children()
        .filter(|node| node.has_tag_name("ObjectDeps"))
        .collect::<Vec<_>>();
    if (!dependencies_enabled && !dependency_nodes.is_empty())
        || (dependencies_enabled && dependency_nodes.len() != declared_count)
    {
        return Err(CodecError::Malformed(
            "ObjectDeps records do not match the Objects dependency envelope".into(),
        ));
    }
    let mut dependency_map = HashMap::<String, DependencyInfo>::new();
    for (order, node) in dependency_nodes.into_iter().enumerate() {
        let name = required_attr(node, "Name")?;
        let dependencies = node
            .children()
            .filter(|child| child.has_tag_name("Dep"))
            .map(|child| required_attr(child, "Name"))
            .collect::<Result<Vec<_>, _>>()?;
        let dependency_count = node
            .attribute("Count")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| {
                CodecError::Malformed("ObjectDeps Count is missing or invalid".into())
            })?;
        if dependency_count != dependencies.len() {
            return Err(CodecError::malformed(format_args!(
                "ObjectDeps {name} Count={dependency_count} but {} dependencies were found",
                dependencies.len()
            )));
        }
        let allow_partial = node
            .attribute("AllowPartial")
            .map(str::parse::<i64>)
            .transpose()
            .map_err(|_| CodecError::Malformed("ObjectDeps AllowPartial is invalid".into()))?;
        if allow_partial.is_some_and(|value| value <= 0) {
            return Err(CodecError::Malformed(
                "ObjectDeps AllowPartial must be positive".into(),
            ));
        }
        if dependency_map
            .insert(
                name.clone(),
                DependencyInfo {
                    dependencies,
                    allow_partial,
                    order,
                },
            )
            .is_some()
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate ObjectDeps name {name}"
            )));
        }
    }

    let mut data_by_name = HashMap::new();
    for node in data_node
        .children()
        .filter(|node| node.has_tag_name(record_tag))
    {
        let name = required_attr(node, "name")?;
        if data_by_name.insert(name.clone(), node).is_some() {
            return Err(CodecError::malformed(format_args!(
                "duplicate ObjectData name {name}"
            )));
        }
    }
    let data_count = data_node
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!("{data_tag} Count is missing or invalid"))
        })?;
    if data_count != data_by_name.len() {
        return Err(CodecError::malformed(format_args!(
            "{data_tag} Count={data_count} but {} records were found",
            data_by_name.len()
        )));
    }

    let mut objects = Vec::new();
    let mut object_names = HashSet::new();
    for (order, node) in objects_node
        .children()
        .filter(|node| node.has_tag_name(record_tag))
        .enumerate()
    {
        let name = required_attr(node, "name")?;
        if !object_names.insert(name.clone()) {
            return Err(CodecError::malformed(format_args!(
                "duplicate object declaration name {name}"
            )));
        }
        let type_name = required_attr(node, "type")?;
        let id = object_id(&name);
        let data_node = data_by_name.get(&name);
        let raw_xml = data_node.map(|data| text[data.range()].to_owned());
        let attributes = node
            .attributes()
            .filter(|attribute| !matches!(attribute.name(), "name" | "type" | "id" | "ViewType"))
            .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
            .collect();
        let dependency = dependency_map.remove(&name);
        if dependencies_enabled
            && dependency
                .as_ref()
                .is_none_or(|dependency| dependency.order != order)
        {
            return Err(CodecError::malformed(format_args!(
                "ObjectDeps order does not match object {name}"
            )));
        }
        objects.push(ObjectRecord {
            id,
            name: name.clone(),
            type_name,
            persistent_id: node.attribute("id").and_then(|value| value.parse().ok()),
            view_type: node.attribute("ViewType").map(str::to_owned),
            attributes,
            dependencies: dependency
                .as_ref()
                .map_or_else(Vec::new, |dependency| dependency.dependencies.clone()),
            dependency_allow_partial: dependency.and_then(|dependency| dependency.allow_partial),
            order,
            raw_xml,
            byte_start: data_node.map(|data| data.range().start as u64),
            byte_end: data_node.map(|data| data.range().end as u64),
        });
    }

    if declared_count != objects.len() {
        return Err(CodecError::malformed(format_args!(
            "{declarations_tag} Count={declared_count} but {} declarations were found",
            objects.len()
        )));
    }
    if !dependency_map.is_empty() {
        return Err(CodecError::Malformed(
            "ObjectDeps names do not match object declarations".into(),
        ));
    }
    if data_by_name.len() != objects.len() {
        return Err(CodecError::malformed(format_args!(
            "object declarations and {data_tag} identities disagree"
        )));
    }
    let declared_names = objects
        .iter()
        .map(|object| object.name.clone())
        .collect::<std::collections::HashSet<_>>();
    for object in &mut objects {
        for dependency in &mut object.dependencies {
            if !declared_names.contains(dependency) {
                return Err(CodecError::malformed(format_args!(
                    "object {} depends on missing object {dependency}",
                    object.name
                )));
            }
            *dependency = object_id(dependency);
        }
    }

    let mut properties = Vec::new();
    let mut extensions = Vec::new();
    let document_properties = root
        .children()
        .filter(|node| node.has_tag_name("Properties"))
        .collect::<Vec<_>>();
    match document_properties.as_slice() {
        [] => {}
        [document_properties] => {
            parse_properties(
                text,
                *document_properties,
                &crate::native::native_id("document", "0"),
                &mut properties,
                ctx,
            )?;
        }
        _ => {
            return Err(CodecError::Malformed(
                "Document.xml has multiple root Properties containers".into(),
            ));
        }
    }
    for object in &objects {
        let data = data_by_name.get(&object.name).ok_or_else(|| {
            CodecError::malformed(format_args!("missing ObjectData for {}", object.name))
        })?;
        let children = data
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        let extension_containers = children
            .iter()
            .filter(|node| node.has_tag_name("Extensions"))
            .copied()
            .collect::<Vec<_>>();
        if extension_containers.len() > 1 {
            return Err(malformed(format!(
                "object {} has multiple direct Extensions containers",
                object.id
            )));
        }
        let property_containers = children
            .iter()
            .filter(|node| node.has_tag_name("Properties"))
            .copied()
            .collect::<Vec<_>>();
        if property_containers.len() > 1 {
            return Err(malformed(format!(
                "object {} has multiple direct Properties containers",
                object.id
            )));
        }
        if let (Some(extensions), Some(properties)) =
            (extension_containers.first(), property_containers.first())
        {
            if extensions.range().start > properties.range().start {
                return Err(malformed(format!(
                    "object {} writes Properties before Extensions",
                    object.id
                )));
            }
        }
        let mut extension_ids_by_start = HashMap::new();
        if let Some(extensions_node) = extension_containers.first() {
            let nodes = extensions_node
                .children()
                .filter(|node| node.has_tag_name("Extension"))
                .collect::<Vec<_>>();
            let declared = extensions_node
                .attribute("Count")
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| {
                    CodecError::Malformed("Extensions Count is missing or invalid".into())
                })?;
            if declared != nodes.len() {
                return Err(CodecError::malformed(format_args!(
                    "Extensions Count={declared} but {} records were found for {}",
                    nodes.len(),
                    object.id
                )));
            }
            let mut extension_names = HashSet::new();
            let mut extension_types = HashSet::new();
            for (order, node) in nodes.into_iter().enumerate() {
                let name = required_attr(node, "name")?;
                let type_name = required_attr(node, "type")?;
                if !extension_names.insert(name.clone()) {
                    return Err(malformed(format!(
                        "duplicate extension name {name} for {}",
                        object.id
                    )));
                }
                if !extension_types.insert(type_name.clone()) {
                    return Err(malformed(format!(
                        "duplicate extension type {type_name} for {}",
                        object.id
                    )));
                }
                let id = extension_id(&object.id, &name, order);
                extension_ids_by_start.insert(node.range().start, id.clone());
                extensions.push(ExtensionRecord {
                    id,
                    owner: object.id.clone(),
                    name,
                    type_name,
                    order,
                    raw_xml: text[node.range()].to_owned(),
                });
            }
        }
        for container in property_containers {
            parse_properties(text, container, &object.id, &mut properties, ctx)?;
        }
        if let Some(extensions_node) = extension_containers.first() {
            for extension in extensions_node
                .children()
                .filter(|node| node.has_tag_name("Extension"))
            {
                let extension_id = extension_ids_by_start
                    .get(&extension.range().start)
                    .ok_or_else(|| {
                        malformed(format!(
                            "extension under {} has no native identity",
                            object.id
                        ))
                    })?;
                for container in extension
                    .children()
                    .filter(|node| node.has_tag_name("Properties"))
                {
                    parse_properties(text, container, extension_id, &mut properties, ctx)?;
                }
            }
        }
    }
    for property in &mut properties {
        for link in &mut property.links {
            if let Some(target) = &mut link.object {
                if declared_names.contains(target) {
                    *target = object_id(target);
                }
            }
        }
    }

    Ok(Graph {
        objects,
        extensions,
        properties,
    })
}

fn parse_properties(
    text: &str,
    container: roxmltree::Node<'_, '_>,
    owner: &str,
    output: &mut Vec<PropertyRecord>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<(), CodecError> {
    let nodes = container
        .children()
        .filter(|node| node.has_tag_name("Property"))
        .collect::<Vec<_>>();
    let transient_nodes = container
        .children()
        .filter(|node| node.has_tag_name("_Property"))
        .collect::<Vec<_>>();
    let mut property_names = HashSet::new();
    for node in transient_nodes.iter().chain(nodes.iter()) {
        let name = required_attr(*node, "name")?;
        if !property_names.insert(name.clone()) {
            return Err(CodecError::malformed(format_args!(
                "duplicate property name {name} for {owner}"
            )));
        }
    }
    let declared = container
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::Malformed("Properties Count is missing or invalid".into()))?;
    if declared != nodes.len() {
        return Err(CodecError::malformed(format_args!(
            "Properties Count={declared} but {} properties were found for {owner}",
            nodes.len()
        )));
    }
    let declared_transient =
        container
            .attribute("TransientCount")
            .map_or(Ok(0_usize), |value| {
                value.parse::<usize>().map_err(|_| {
                    CodecError::Malformed("Properties TransientCount is invalid".into())
                })
            })?;
    if declared_transient != transient_nodes.len() {
        return Err(CodecError::malformed(format_args!(
            "Properties TransientCount={declared_transient} but {} transient properties were found for {owner}",
            transient_nodes.len()
        )));
    }
    for (order, node) in transient_nodes.into_iter().enumerate() {
        let name = required_attr(node, "name")?;
        let type_name = required_attr(node, "type")?;
        output.push(PropertyRecord {
            id: crate::native::native_child_id("property", owner, &name),
            owner: owner.to_owned(),
            name,
            family: property_family(&type_name),
            type_name,
            status: node
                .attribute("status")
                .and_then(|value| value.parse().ok()),
            transient: true,
            dynamic: None,
            order,
            values: Vec::new(),
            links: Vec::new(),
            side_entries: Vec::new(),
            raw_xml: text[node.range()].to_owned(),
            byte_start: node.range().start as u64,
            byte_end: node.range().end as u64,
        });
    }
    for (order, node) in nodes.into_iter().enumerate() {
        let name = required_attr(node, "name")?;
        let type_name = required_attr(node, "type")?;
        let mut retained_value_bytes = 0_usize;
        let values = node
            .descendants()
            .filter(|value| value.is_element() && *value != node)
            .enumerate()
            .map(|(value_order, value)| {
                let len = value.range().len();
                retained_value_bytes = retained_value_bytes
                    .checked_add(len)
                    .filter(|total| *total <= MAX_PROPERTY_VALUE_XML_BYTES)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "property {name} retained value XML limit exceeded"
                        ))
                    })?;
                if let Some(ctx) = ctx {
                    ctx.charge_retained(
                        u64::try_from(len).unwrap_or(u64::MAX),
                        "fcstd_property_value_xml",
                        None,
                    )?;
                }
                Ok(ValueRecord {
                    tag: value.tag_name().name().to_owned(),
                    order: value_order,
                    attributes: value
                        .attributes()
                        .map(|attribute| {
                            (attribute.name().to_owned(), attribute.value().to_owned())
                        })
                        .collect(),
                    text: value.text().map(str::to_owned),
                    raw_xml: text[value.range()].to_owned(),
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let links = if link_grammar(&type_name).is_some() {
            parse_link_targets(node, &type_name)?
        } else {
            Vec::new()
        };
        let side_entries = values
            .iter()
            .flat_map(|value| {
                value
                    .attributes
                    .iter()
                    .filter(|(name, _)| {
                        (matches!(name.as_str(), "file" | "File") && !is_xlink_type(&type_name))
                            || (property_family(&type_name) == PropertyFamily::File
                                && matches!(name.as_str(), "name" | "Name"))
                    })
                    .map(|(_, value)| value.clone())
            })
            .filter(|value| !value.is_empty())
            .collect();
        output.push(PropertyRecord {
            id: crate::native::native_child_id("property", owner, &name),
            owner: owner.to_owned(),
            name,
            family: property_family(&type_name),
            type_name,
            status: node
                .attribute("status")
                .and_then(|value| value.parse().ok()),
            transient: false,
            dynamic: node.attribute("group").map(|group| DynamicPropertyMeta {
                group: group.to_owned(),
                documentation: node.attribute("doc").map(str::to_owned),
                attributes: node.attribute("attr").and_then(|value| value.parse().ok()),
                read_only: bool_attr(node.attribute("ro")),
                hidden: bool_attr(node.attribute("hide")),
            }),
            order,
            values,
            links,
            side_entries,
            raw_xml: text[node.range()].to_owned(),
            byte_start: node.range().start as u64,
            byte_end: node.range().end as u64,
        });
    }
    Ok(())
}

pub(crate) fn validate_link_property(
    property: roxmltree::Node<'_, '_>,
    type_name: &str,
) -> Result<(), CodecError> {
    let grammar_type = if type_name == "App::PropertyPlacementLink" {
        "App::PropertyLink"
    } else {
        type_name
    };
    parse_link_targets(property, grammar_type).map(|_| ())
}

fn parse_link_targets(
    property: roxmltree::Node<'_, '_>,
    type_name: &str,
) -> Result<Vec<LinkTarget>, CodecError> {
    let grammar = link_grammar(type_name);
    let Some(grammar) = grammar else {
        return Ok(Vec::new());
    };
    let root = single_value_element(property, grammar.root_tag(), type_name)?;
    match grammar {
        LinkGrammar::Link => {
            reject_nested_link_value(root)?;
            Ok(vec![local_link(root, "value", &[])?])
        }
        LinkGrammar::LinkList => counted_children(root, "Link", type_name)?
            .map(|node| {
                reject_nested_link_value(node)?;
                local_link(node, "value", &[])
            })
            .collect(),
        LinkGrammar::LinkSub => {
            let subelements = counted_children(root, "Sub", type_name)?
                .map(|node| {
                    reject_nested_link_value(node)?;
                    restored_subelement(node, "value")
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(vec![local_link(root, "value", &subelements)?])
        }
        LinkGrammar::LinkSubList => counted_children(root, "Link", type_name)?
            .map(|node| {
                reject_nested_link_value(node)?;
                let sub = restored_subelement(node, "sub")?;
                local_link(node, "obj", &[sub])
            })
            .collect(),
        LinkGrammar::XLink => Ok(vec![xlink(root)?]),
        LinkGrammar::XLinkList => counted_children(root, "XLink", type_name)?
            .map(xlink)
            .collect(),
    }
}

fn reject_nested_link_value(node: roxmltree::Node<'_, '_>) -> Result<(), CodecError> {
    if node.children().any(|child| child.is_element()) {
        return Err(CodecError::Malformed(
            "link carrier contains nested element values".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum LinkGrammar {
    Link,
    LinkList,
    LinkSub,
    LinkSubList,
    XLink,
    XLinkList,
}

impl LinkGrammar {
    fn root_tag(self) -> &'static str {
        match self {
            Self::Link => "Link",
            Self::LinkList => "LinkList",
            Self::LinkSub => "LinkSub",
            Self::LinkSubList => "LinkSubList",
            Self::XLink => "XLink",
            Self::XLinkList => "XLinkSubList",
        }
    }
}

fn link_grammar(type_name: &str) -> Option<LinkGrammar> {
    let grammar = match type_name {
        "App::PropertyLink"
        | "App::PropertyLinkChild"
        | "App::PropertyLinkGlobal"
        | "App::PropertyLinkHidden" => LinkGrammar::Link,
        "App::PropertyLinkList"
        | "App::PropertyLinkListChild"
        | "App::PropertyLinkListGlobal"
        | "App::PropertyLinkListHidden" => LinkGrammar::LinkList,
        "App::PropertyLinkSub"
        | "App::PropertyLinkSubChild"
        | "App::PropertyLinkSubGlobal"
        | "App::PropertyLinkSubHidden" => LinkGrammar::LinkSub,
        "App::PropertyLinkSubList"
        | "App::PropertyLinkSubListChild"
        | "App::PropertyLinkSubListGlobal"
        | "App::PropertyLinkSubListHidden" => LinkGrammar::LinkSubList,
        "App::PropertyXLink" | "App::PropertyXLinkSub" | "App::PropertyXLinkSubHidden" => {
            LinkGrammar::XLink
        }
        "App::PropertyXLinkSubList" | "App::PropertyXLinkList" => LinkGrammar::XLinkList,
        _ => return None,
    };
    Some(grammar)
}

fn single_value_element<'a, 'input>(
    property: roxmltree::Node<'a, 'input>,
    tag: &str,
    type_name: &str,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let mut elements = property.children().filter(roxmltree::Node::is_element);
    let value = elements.next().ok_or_else(|| {
        CodecError::malformed(format_args!("{type_name} requires one {tag} value"))
    })?;
    if !value.has_tag_name(tag) || elements.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "{type_name} requires exactly one {tag} value"
        )));
    }
    Ok(value)
}

fn counted_children<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    tag: &'static str,
    type_name: &str,
) -> Result<impl Iterator<Item = roxmltree::Node<'a, 'input>>, CodecError> {
    let count = required_attr(parent, "count")?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("{type_name} count is invalid")))?;
    let children = parent
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if children.len() != count || children.iter().any(|child| !child.has_tag_name(tag)) {
        return Err(CodecError::malformed(format_args!(
            "{type_name} count={count} but {} {tag} values were found",
            children.len()
        )));
    }
    Ok(children.into_iter())
}

fn local_link(
    node: roxmltree::Node<'_, '_>,
    object_attribute: &str,
    subelements: &[String],
) -> Result<LinkTarget, CodecError> {
    if object_attribute == "obj" {
        reject_link_aliases(node, &["obj", "sub"])?;
    } else {
        reject_link_aliases(node, &[object_attribute])?;
    }
    Ok(LinkTarget {
        document: None,
        document_attribute: None,
        object: Some(required_attr(node, object_attribute)?),
        subelements: subelements.to_vec(),
    })
}

fn xlink(node: roxmltree::Node<'_, '_>) -> Result<LinkTarget, CodecError> {
    reject_link_aliases(node, &["name", "file", "sub"])?;
    let file = node.attribute("file").map(str::to_owned);
    let children = node
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let subelements = match (node.attribute("sub"), node.attribute("count")) {
        (Some(_), None) if children.is_empty() => vec![restored_subelement(node, "sub")?],
        (Some(_), None) => {
            return Err(CodecError::Malformed(
                "App::PropertyXLink sub carrier has nested values".into(),
            ));
        }
        (None, Some(_)) => {
            if node
                .attribute("count")
                .and_then(|count| count.parse::<usize>().ok())
                == Some(0)
            {
                return Err(CodecError::Malformed(
                    "App::PropertyXLink uses count only for one or more Sub values".into(),
                ));
            }
            counted_children(node, "Sub", "App::PropertyXLink")?
                .map(|child| {
                    if child.children().any(|value| value.is_element()) {
                        return Err(CodecError::Malformed(
                            "App::PropertyXLink Sub carrier has nested values".into(),
                        ));
                    }
                    restored_subelement(child, "value")
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        (None, None) if children.is_empty() => Vec::new(),
        (None, None) => {
            return Err(CodecError::Malformed(
                "App::PropertyXLink has nested values without a sub carrier".into(),
            ));
        }
        (Some(_), Some(_)) => {
            return Err(CodecError::Malformed(
                "App::PropertyXLink has both sub and count carriers".into(),
            ));
        }
    };
    Ok(LinkTarget {
        document: file.as_ref().filter(|value| !value.is_empty()).cloned(),
        document_attribute: file.map(|_| "file".into()),
        object: Some(required_attr(node, "name")?),
        subelements,
    })
}

fn restored_subelement(
    node: roxmltree::Node<'_, '_>,
    primary_attribute: &str,
) -> Result<String, CodecError> {
    let primary = required_attr(node, primary_attribute)?;
    Ok(node.attribute("shadowed").unwrap_or(&primary).to_owned())
}

fn reject_link_aliases(node: roxmltree::Node<'_, '_>, allowed: &[&str]) -> Result<(), CodecError> {
    const CARRIERS: &[&str] = &[
        "value", "Value", "object", "Object", "obj", "Obj", "name", "Name", "document", "Document",
        "doc", "Doc", "file", "File", "sub", "Sub",
    ];
    if let Some(attribute) = node.attributes().find(|attribute| {
        CARRIERS.contains(&attribute.name()) && !allowed.contains(&attribute.name())
    }) {
        return Err(CodecError::malformed(format_args!(
            "{} has unsupported link carrier {}",
            node.tag_name().name(),
            attribute.name()
        )));
    }
    Ok(())
}

pub(crate) fn property_family(type_name: &str) -> PropertyFamily {
    match type_name {
        "App::PropertyPythonObject" => PropertyFamily::PythonObject,
        "App::PropertyExpression" | "App::PropertyExpressionEngine" => PropertyFamily::Expression,
        "App::PropertyLink"
        | "App::PropertyLinkChild"
        | "App::PropertyLinkGlobal"
        | "App::PropertyLinkHidden"
        | "App::PropertyLinkList"
        | "App::PropertyLinkListChild"
        | "App::PropertyLinkListGlobal"
        | "App::PropertyLinkListHidden"
        | "App::PropertyLinkSub"
        | "App::PropertyLinkSubChild"
        | "App::PropertyLinkSubGlobal"
        | "App::PropertyLinkSubHidden"
        | "App::PropertyLinkSubList"
        | "App::PropertyLinkSubListChild"
        | "App::PropertyLinkSubListGlobal"
        | "App::PropertyLinkSubListHidden"
        | "App::PropertyXLink"
        | "App::PropertyXLinkList"
        | "App::PropertyXLinkSub"
        | "App::PropertyXLinkSubHidden"
        | "App::PropertyXLinkSubList" => PropertyFamily::Link,
        "App::PropertyFile" | "App::PropertyFileIncluded" => PropertyFamily::File,
        "Part::PropertyPartShape"
        | "Part::PropertyGeometryList"
        | "Mesh::PropertyMeshKernel"
        | "Points::PropertyPointKernel" => PropertyFamily::Geometry,
        "App::PropertyPlacement" | "App::PropertyPlacementList" => PropertyFamily::Placement,
        "App::PropertyMatrix" => PropertyFamily::Matrix,
        "App::PropertyVector" | "App::PropertyVectorList" => PropertyFamily::Vector,
        "App::PropertyEnumeration" => PropertyFamily::Enumeration,
        "App::PropertyAcceleration"
        | "App::PropertyAngle"
        | "App::PropertyArea"
        | "App::PropertyDistance"
        | "App::PropertyForce"
        | "App::PropertyLength"
        | "App::PropertyPressure"
        | "App::PropertyQuantity"
        | "App::PropertyQuantityConstraint"
        | "App::PropertySpeed"
        | "App::PropertyVolume" => PropertyFamily::Quantity,
        "App::PropertyMap" => PropertyFamily::Map,
        "App::PropertyBoolList"
        | "App::PropertyFloatList"
        | "App::PropertyIntegerList"
        | "App::PropertyIntegerSet"
        | "App::PropertyStringList"
        | "Part::PropertyTopoShapeList"
        | "Sketcher::PropertyConstraintList"
        | "TechDraw::PropertyCenterLineList"
        | "TechDraw::PropertyCosmeticEdgeList"
        | "TechDraw::PropertyCosmeticVertexList"
        | "TechDraw::PropertyGeomFormatList" => PropertyFamily::List,
        "App::PropertyPath"
        | "App::PropertyString"
        | "App::PropertyUUID"
        | "Path::PropertyPath" => PropertyFamily::String,
        "App::PropertyBool"
        | "App::PropertyFloat"
        | "App::PropertyFloatConstraint"
        | "App::PropertyInteger"
        | "App::PropertyIntegerConstraint"
        | "App::PropertyPercent" => PropertyFamily::Scalar,
        _ => PropertyFamily::Unknown,
    }
}

pub(crate) fn is_xlink_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyXLink"
            | "App::PropertyXLinkList"
            | "App::PropertyXLinkSub"
            | "App::PropertyXLinkSubHidden"
            | "App::PropertyXLinkSubList"
    )
}

fn unique_section<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let sections = root
        .children()
        .filter(|node| node.has_tag_name(tag))
        .collect::<Vec<_>>();
    match sections.as_slice() {
        [section] => Ok(*section),
        [] => Err(CodecError::malformed(format_args!(
            "Document.xml has no {tag} section"
        ))),
        _ => Err(CodecError::malformed(format_args!(
            "Document.xml has duplicate {tag} sections"
        ))),
    }
}

fn object_id(name: &str) -> String {
    crate::native::native_id("object", name)
}

fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}

fn extension_id(owner: &str, name: &str, order: usize) -> String {
    crate::native::native_child_id("extension", owner, &format!("{order}:{name}"))
}

fn required_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Result<String, CodecError> {
    node.attribute(name).map(str::to_owned).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{} element has no {name} attribute",
            node.tag_name().name()
        ))
    })
}

fn bool_attr(value: Option<&str>) -> Option<bool> {
    value.map(|value| matches!(value, "1" | "true" | "True" | "TRUE"))
}

#[cfg(test)]
pub(crate) mod tests;
