// SPDX-License-Identifier: Apache-2.0
//! Generic `FreeCAD` object and property graph recovery.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::native::{
    DynamicPropertyMeta, ExtensionRecord, LinkTarget, LinkTargetWire, ObjectRecord, PropertyFamily,
    PropertyRecord, ValueRecord,
};

const MAX_OBJECTS: usize = 1_000_000;
const MAX_PROPERTY_VALUE_XML_BYTES: usize = 16 * 1024 * 1024;

/// Persistence vocabulary follows the numeric schema value, independently of
/// whether its source spelling matches a catalog discriminant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Vocabulary {
    Features,
    Objects,
}

impl Vocabulary {
    pub(crate) fn from_declaration(declared: &str) -> Result<Self, CodecError> {
        match declared.parse::<u32>() {
            Ok(2) => Ok(Self::Features),
            Ok(_) => Ok(Self::Objects),
            Err(_) => Err(CodecError::Malformed(
                "Document.xml SchemaVersion is invalid".into(),
            )),
        }
    }

    pub(crate) const fn tags(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Features => ("Features", "FeatureData", "Feature"),
            Self::Objects => ("Objects", "ObjectData", "Object"),
        }
    }
}

struct DependencyInfo {
    dependencies: Vec<String>,
    allow_partial: Option<std::num::NonZeroU64>,
    order: usize,
}

/// Recovered persistence graph.
pub(crate) struct Graph {
    /// Declared objects.
    pub(crate) objects: Vec<ObjectRecord>,
    /// Dynamic extensions.
    pub(crate) extensions: Vec<ExtensionRecord>,
    /// Document and object properties.
    pub(crate) properties: Vec<PropertyRecord>,
    /// Diagnostics for source-only persistence metadata.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Required property payload references must resolve. Missing naming sidecars
/// stay in exact value XML, but cannot enter the admitted reference graph.
pub(crate) fn resolve_side_entries(
    properties: &mut [PropertyRecord],
    entries: &std::collections::BTreeMap<String, cadmpeg_core::decode::View<'_>>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    for property in properties {
        let crate::native::PropertyBody::Persisted {
            values,
            side_entries,
            ..
        } = &mut property.body
        else {
            continue;
        };
        if side_entries.iter().all(|name| entries.contains_key(name)) {
            continue;
        }
        let mut naming_files = HashSet::new();
        let mut required_files = HashSet::new();
        for value in values {
            for (key, name) in &value.attributes {
                if !matches!(key.as_str(), "file" | "File") || name.is_empty() {
                    continue;
                }
                if matches!(
                    value.tag.as_str(),
                    "StringHasher" | "StringHasher2" | "ElementMap" | "ElementMap2"
                ) {
                    naming_files.insert(name.as_str());
                } else if property.family == PropertyFamily::Geometry {
                    required_files.insert(name.as_str());
                }
            }
        }
        for name in side_entries
            .iter()
            .filter(|name| !entries.contains_key(*name))
        {
            if required_files.contains(name.as_str()) {
                return Err(CodecError::malformed(format_args!(
                    "property {} references missing side entry {name}",
                    property.id
                )));
            }
            let code = if naming_files.contains(name.as_str()) {
                crate::loss::FreecadLossCode::ElementMapMetadataUnresolved
            } else {
                crate::loss::FreecadLossCode::PersistenceSideEntryUnresolved
            };
            losses.push(code.note(
                format!("property {} references unavailable optional side entry {name}; reference remains in source XML", property.id)
            ));
        }
        side_entries.retain(|name| entries.contains_key(name));
    }
    Ok(())
}

/// Recover the persistence graph from `Document.xml` bytes.
pub(crate) fn parse_with_context(
    bytes: &[u8],
    schema_version: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Graph, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let admitted_xml = ctx.parse_xml(text, "FreeCAD XML tree").map_err(|error| {
        let CodecError::Malformed(error) = error else {
            return error;
        };
        CodecError::malformed(format_args!("invalid Document.xml: {error}"))
    })?;
    let xml = admitted_xml.document();
    parse_document(xml, Vocabulary::from_declaration(schema_version)?)
}

/// Recover the persistence graph from an already admitted document.
pub(crate) fn parse_document(
    xml: &roxmltree::Document<'_>,
    schema: Vocabulary,
) -> Result<Graph, CodecError> {
    let text = xml.input_text();
    let root = xml.root_element();
    let mut losses = Vec::new();
    // Schema 2 is its own element vocabulary. Every other declared schema, and
    // every undeclared one, is read with the Objects/ObjectData/Object
    // vocabulary: the nearest declared strategy is attempted rather than
    // refused on a discriminant allowlist, and the attempt is self-limiting
    // because an element vocabulary that does not fit fails below exactly as a
    // corrupt schema-4 document does. `crate::dialect` charges the
    // dialect-unverified loss for the undeclared case.
    let (declarations_tag, data_tag, record_tag) = schema.tags();
    let objects_node = unique_section(root, declarations_tag)?;
    let data_node = unique_section(root, data_tag)?;

    let actual_count = objects_node
        .children()
        .filter(|node| node.has_tag_name(record_tag))
        .count();
    check_count(objects_node, "Count", actual_count, &mut losses);
    if actual_count > MAX_OBJECTS {
        return Err(CodecError::NotImplemented(format!(
            "FCStd documents with more than {MAX_OBJECTS} objects ({actual_count} declared)"
        )));
    }
    if schema == Vocabulary::Features && objects_node.attribute("Dependencies").is_some() {
        return Err(CodecError::Malformed(
            "schema 2 Features cannot carry object dependencies".into(),
        ));
    }

    let dependencies_enabled =
        schema != Vocabulary::Features && objects_node.attribute("Dependencies").is_some();
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
    let dependency_records = objects_node
        .children()
        .filter(|node| node.has_tag_name("ObjectDeps"))
        .collect::<Vec<_>>();
    if (!dependencies_enabled && !dependency_records.is_empty())
        || (dependencies_enabled && dependency_records.len() != actual_count)
    {
        return Err(CodecError::Malformed(
            "ObjectDeps records do not match the Objects dependency envelope".into(),
        ));
    }
    let mut dependency_map = HashMap::<String, DependencyInfo>::new();
    for (order, node) in dependency_records.into_iter().enumerate() {
        let name = retained_attr(node, "Name")?;
        let mut dependencies = Vec::new();
        for child in node.children().filter(|child| child.has_tag_name("Dep")) {
            dependencies.push(retained_attr(child, "Name")?);
        }
        check_count(node, "Count", dependencies.len(), &mut losses);
        let allow_partial = node
            .attribute("AllowPartial")
            .map(str::parse::<std::num::NonZeroU64>)
            .transpose()
            .map_err(|_| {
                CodecError::Malformed("ObjectDeps AllowPartial must be positive".into())
            })?;
        if dependency_map.contains_key(&name) {
            return Err(CodecError::malformed(format_args!(
                "duplicate ObjectDeps name {name}"
            )));
        }
        dependency_map.insert(
            name,
            DependencyInfo {
                dependencies,
                allow_partial,
                order,
            },
        );
    }

    let mut data_by_name = HashMap::new();
    for node in data_node
        .children()
        .filter(|node| node.has_tag_name(record_tag))
    {
        let name = retained_attr(node, "name")?;
        if data_by_name.contains_key(&name) {
            return Err(CodecError::malformed(format_args!(
                "duplicate ObjectData name {name}"
            )));
        }
        data_by_name.insert(name, node);
    }
    check_count(data_node, "Count", data_by_name.len(), &mut losses);

    let mut objects: Vec<ObjectRecord> = Vec::new();
    let mut object_names = HashSet::new();
    for (order, node) in objects_node
        .children()
        .filter(|node| node.has_tag_name(record_tag))
        .enumerate()
    {
        let name = required_attr(node, "name")?;
        if !object_names.insert(name) {
            return Err(CodecError::malformed(format_args!(
                "duplicate object declaration name {name}"
            )));
        }
        let name = name.to_owned();
        let type_name = retained_attr(node, "type")?;
        let identity = crate::native::object_identity::ObjectIdentity::from_name(name);
        let data_node = data_by_name.get(identity.name());
        let attributes = node
            .attributes()
            .filter(|attribute| !matches!(attribute.name(), "name" | "type" | "id" | "ViewType"))
            .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
            .collect();
        let dependency = dependency_map.remove(identity.name());
        if dependencies_enabled
            && dependency
                .as_ref()
                .is_none_or(|dependency| dependency.order != order)
        {
            return Err(CodecError::malformed(format_args!(
                "ObjectDeps order does not match object {}",
                identity.name()
            )));
        }
        let (dependencies, dependency_allow_partial) = match dependency {
            Some(dependency) => (dependency.dependencies, dependency.allow_partial),
            None => (Vec::new(), None),
        };
        objects.push(ObjectRecord {
            identity,
            type_name,
            persistent_id: node.attribute("id").and_then(|value| value.parse().ok()),
            view_type: node.attribute("ViewType").map(str::to_owned),
            attributes,
            dependencies,
            dependency_allow_partial,
            order,
            data: data_node
                .map(|data| {
                    crate::native::RetainedXml::from_source(
                        &text[data.range()],
                        cadmpeg_core::decode::u64_from_index(data.range().start),
                    )
                })
                .transpose()?,
        });
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
    for object in &mut objects {
        let object_name = object.identity.name();
        for dependency in &mut object.dependencies {
            if !data_by_name.contains_key(dependency) {
                return Err(CodecError::malformed(format_args!(
                    "object {object_name} depends on missing object {dependency}"
                )));
            }
            *dependency = crate::native::native_id("object", dependency.as_str());
        }
    }

    let mut properties = Vec::new();
    let mut extensions = Vec::new();
    let document_property_nodes = root
        .children()
        .filter(|node| node.has_tag_name("Properties"))
        .collect::<Vec<_>>();
    match document_property_nodes.as_slice() {
        [] => {}
        [document_properties] => {
            parse_properties(
                text,
                *document_properties,
                &crate::native::native_id("document", "0"),
                &mut properties,
                &mut losses,
            )?;
        }
        _ => {
            return Err(CodecError::Malformed(
                "Document.xml has multiple root Properties containers".into(),
            ));
        }
    }
    for object in &objects {
        let data = data_by_name.get(object.name()).ok_or_else(|| {
            CodecError::malformed(format_args!("missing ObjectData for {}", object.name()))
        })?;
        let child_nodes = data
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        let mut extension_containers = child_nodes
            .iter()
            .filter(|node| node.has_tag_name("Extensions"))
            .copied();
        let extension_container = extension_containers.next();
        if extension_containers.next().is_some() {
            return Err(CodecError::malformed(format_args!(
                "object {} has multiple direct Extensions containers",
                object.id()
            )));
        }
        let mut property_containers = child_nodes
            .iter()
            .filter(|node| node.has_tag_name("Properties"))
            .copied();
        let property_container = property_containers.next();
        if property_containers.next().is_some() {
            return Err(CodecError::malformed(format_args!(
                "object {} has multiple direct Properties containers",
                object.id()
            )));
        }
        if let (Some(extensions), Some(properties)) = (extension_container, property_container) {
            if extensions.range().start > properties.range().start {
                return Err(CodecError::malformed(format_args!(
                    "object {} writes Properties before Extensions",
                    object.id()
                )));
            }
        }
        let mut extension_ids_by_start = HashMap::new();
        if let Some(extensions_node) = extension_container {
            let extension_nodes = extensions_node
                .children()
                .filter(|node| node.has_tag_name("Extension"))
                .collect::<Vec<_>>();
            check_count(extensions_node, "Count", extension_nodes.len(), &mut losses);
            let mut extension_names = HashSet::new();
            let mut extension_types = HashSet::new();
            for (order, node) in extension_nodes.into_iter().enumerate() {
                let name = retained_attr(node, "name")?;
                let type_name = retained_attr(node, "type")?;
                if extension_names.contains(&name) {
                    return Err(CodecError::malformed(format_args!(
                        "duplicate extension name {name} for {}",
                        object.id()
                    )));
                }
                extension_names.insert(name.clone());
                if extension_types.contains(&type_name) {
                    return Err(CodecError::malformed(format_args!(
                        "duplicate extension type {type_name} for {}",
                        object.id()
                    )));
                }
                extension_types.insert(type_name.clone());
                let id = extension_id(object.id(), &name, order);
                extension_ids_by_start.insert(node.range().start, id.clone());

                extensions.push(ExtensionRecord {
                    id,
                    owner: object.id().to_owned(),
                    name,
                    type_name,
                    order,
                    raw_xml: text[node.range()].to_owned(),
                });
            }
        }
        if let Some(container) = property_container {
            parse_properties(text, container, object.id(), &mut properties, &mut losses)?;
        }
        if let Some(extensions_node) = extension_container {
            for extension in extensions_node
                .children()
                .filter(|node| node.has_tag_name("Extension"))
            {
                let extension_id = extension_ids_by_start
                    .get(&extension.range().start)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "extension under {} has no native identity",
                            object.id()
                        ))
                    })?;
                for container in extension
                    .children()
                    .filter(|node| node.has_tag_name("Properties"))
                {
                    parse_properties(text, container, extension_id, &mut properties, &mut losses)?;
                }
            }
        }
    }
    for property in &mut properties {
        let crate::native::PropertyBody::Persisted { links, .. } = &mut property.body else {
            continue;
        };
        for link in links.iter_mut().flatten() {
            if link.document().is_none() {
                let Some(target) = link.object() else {
                    continue;
                };
                if data_by_name.contains_key(target) {
                    link.set_object(
                        cadmpeg_core::text::NonBlankString::new(crate::native::native_id(
                            "object", target,
                        ))
                        .ok_or_else(|| {
                            CodecError::malformed("link object identity must not be empty")
                        })?,
                    );
                }
            }
        }
    }

    Ok(Graph {
        objects,
        extensions,
        properties,
        losses,
    })
}

/// XML element boundaries and identities own the population, not its redundant count.
fn check_count(
    node: roxmltree::Node<'_, '_>,
    field: &str,
    actual: usize,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let declared = node.attribute(field);
    if declared.and_then(|value| value.parse::<usize>().ok()) != Some(actual) {
        losses.push(
            crate::loss::FreecadLossCode::PersistenceCountNoncanonical.note(format!(
                "{} {field}={declared:?}; reading {actual} framed elements",
                node.tag_name().name()
            )),
        );
    }
}

fn parse_properties(
    text: &str,
    container: roxmltree::Node<'_, '_>,
    owner: &str,
    output: &mut Vec<PropertyRecord>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let property_nodes = container
        .children()
        .filter(|node| node.has_tag_name("Property"))
        .collect::<Vec<_>>();
    let transient_property_nodes = container
        .children()
        .filter(|node| node.has_tag_name("_Property"))
        .collect::<Vec<_>>();
    let mut names = HashSet::new();
    for node in transient_property_nodes.iter().chain(property_nodes.iter()) {
        let name = node.attribute("name").ok_or_else(|| {
            CodecError::malformed(format_args!(
                "{} element has no name attribute",
                node.tag_name().name()
            ))
        })?;
        if !names.insert(name) {
            return Err(CodecError::malformed(format_args!(
                "duplicate property name {name} for {owner}"
            )));
        }
    }
    check_count(container, "Count", property_nodes.len(), losses);
    if container.attribute("TransientCount").is_some() || !transient_property_nodes.is_empty() {
        check_count(
            container,
            "TransientCount",
            transient_property_nodes.len(),
            losses,
        );
    }
    for (order, node) in transient_property_nodes.into_iter().enumerate() {
        let name = retained_attr(node, "name")?;
        let type_name = retained_attr(node, "type")?;

        output.push(PropertyRecord {
            id: crate::native::native_child_id("property", owner, &name),
            owner: owner.to_owned(),
            name,
            family: property_family(&type_name),
            type_name,
            status: node
                .attribute("status")
                .and_then(|value| value.parse().ok()),
            body: crate::native::PropertyBody::Transient,
            order,
            xml: crate::native::RetainedXml::from_source(
                &text[node.range()],
                cadmpeg_core::decode::u64_from_index(node.range().start),
            )?,
        });
    }
    for (order, node) in property_nodes.into_iter().enumerate() {
        let name = retained_attr(node, "name")?;
        let type_name = retained_attr(node, "type")?;
        let mut payload_error = None;
        let links = if link_grammar(&type_name).is_some() {
            match parse_link_targets(node, &type_name) {
                Ok(links) => links,
                Err(CodecError::Malformed(detail)) => {
                    losses.push(
                        crate::loss::FreecadLossCode::PersistencePayloadUnresolved.note(format!(
                            "property {owner}/{name} is unreadable: {detail}; exact XML retained"
                        )),
                    );
                    payload_error = Some(detail);
                    Vec::new()
                }
                Err(error) => return Err(error),
            }
        } else {
            Vec::new()
        };
        // Nested value elements copy overlapping XML ranges, so their total
        // is bounded per property. A larger payload stays as its exact XML.
        let value_xml_bytes = node
            .descendants()
            .filter(|value| payload_error.is_none() && value.is_element() && *value != node)
            .try_fold(0_usize, |total, value| {
                total.checked_add(value.range().len())
            });
        if value_xml_bytes.is_none_or(|bytes| bytes > MAX_PROPERTY_VALUE_XML_BYTES) {
            let detail = format!(
                "value XML copies exceed the {MAX_PROPERTY_VALUE_XML_BYTES}-byte value copy bound"
            );
            losses.push(
                crate::loss::FreecadLossCode::PersistencePayloadUnresolved.note(format!(
                    "property {owner}/{name} is unreadable: {detail}; exact XML retained"
                )),
            );
            payload_error = Some(detail);
        }
        let mut values = Vec::new();
        for (value_order, value) in node
            .descendants()
            .filter(|value| payload_error.is_none() && value.is_element() && *value != node)
            .enumerate()
        {
            values.push(ValueRecord {
                tag: value.tag_name().name().to_owned(),
                order: value_order,
                attributes: value
                    .attributes()
                    .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                    .collect(),
                text: value.text().map(str::to_owned),
                raw_xml: text[value.range()].to_owned(),
            });
        }
        let mut side_entries = Vec::new();
        for value in &values {
            for (name, entry_name) in &value.attributes {
                let selected = (matches!(name.as_str(), "file" | "File")
                    && !is_xlink_type(&type_name))
                    || (property_family(&type_name) == PropertyFamily::File
                        && matches!(name.as_str(), "name" | "Name"));
                if selected && !entry_name.is_empty() {
                    side_entries.push(entry_name.to_owned());
                }
            }
        }
        output.push(PropertyRecord {
            id: crate::native::native_child_id("property", owner, &name),
            owner: owner.to_owned(),
            name,
            family: property_family(&type_name),
            type_name,
            status: node
                .attribute("status")
                .and_then(|value| value.parse().ok()),
            body: if let Some(detail) = payload_error {
                crate::native::PropertyBody::Unreadable(detail)
            } else {
                crate::native::PropertyBody::Persisted {
                    values,
                    links,
                    side_entries,
                    dynamic: node.attribute("group").map(|group| DynamicPropertyMeta {
                        group: group.to_owned(),
                        documentation: node.attribute("doc").map(str::to_owned),
                        attributes: node.attribute("attr").and_then(|value| value.parse().ok()),
                        read_only: bool_attr(node.attribute("ro")),
                        hidden: bool_attr(node.attribute("hide")),
                    }),
                }
            },
            order,
            xml: crate::native::RetainedXml::from_source(
                &text[node.range()],
                cadmpeg_core::decode::u64_from_index(node.range().start),
            )?,
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
) -> Result<Vec<Option<LinkTarget>>, CodecError> {
    let grammar = link_grammar(type_name);
    let Some(grammar) = grammar else {
        return Ok(Vec::new());
    };
    let root = single_value_element(property, grammar.root_tag(), type_name)?;
    match grammar {
        LinkGrammar::Link => {
            reject_nested_link_value(root)?;
            Ok(vec![local_link(root, "value", Vec::new())?])
        }
        LinkGrammar::LinkList => {
            let children = counted_children(root, "Link", type_name)?;
            let mut targets = Vec::new();
            for node in children {
                reject_nested_link_value(node)?;
                targets.push(local_link(node, "value", Vec::new())?);
            }
            Ok(targets)
        }
        LinkGrammar::LinkSub => {
            let children = counted_children(root, "Sub", type_name)?;
            let mut subelements = Vec::new();
            for node in children {
                reject_nested_link_value(node)?;
                subelements.push(restored_subelement(node, "value")?);
            }
            Ok(vec![local_link(root, "value", subelements)?])
        }
        LinkGrammar::LinkSubList => {
            let children = counted_children(root, "Link", type_name)?;
            let mut targets = Vec::new();
            for node in children {
                reject_nested_link_value(node)?;
                let sub = restored_subelement(node, "sub")?;
                targets.push(local_link(node, "obj", vec![sub])?);
            }
            Ok(targets)
        }
        LinkGrammar::XLink => Ok(vec![xlink(root)?]),
        LinkGrammar::XLinkSubList => {
            let children = counted_children(root, "XLink", type_name)?;
            let mut targets = Vec::new();
            for node in children {
                targets.push(xlink(node)?);
            }
            Ok(targets)
        }
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
    XLinkSubList,
}

impl LinkGrammar {
    fn root_tag(self) -> &'static str {
        match self {
            Self::Link => "Link",
            Self::LinkList => "LinkList",
            Self::LinkSub => "LinkSub",
            Self::LinkSubList => "LinkSubList",
            Self::XLink => "XLink",
            Self::XLinkSubList => "XLinkSubList",
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
        "App::PropertyXLinkSubList" | "App::PropertyXLinkList" => LinkGrammar::XLinkSubList,
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
    let count = parent
        .attribute("count")
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "{} element has no count attribute",
                parent.tag_name().name()
            ))
        })?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("{type_name} count is invalid")))?;
    let mut actual_count = 0_usize;
    let mut valid_tags = true;
    for child in parent.children().filter(roxmltree::Node::is_element) {
        actual_count += 1;
        valid_tags &= child.has_tag_name(tag);
    }
    if actual_count != count || !valid_tags {
        return Err(CodecError::malformed(format_args!(
            "{type_name} count={count} but {actual_count} {tag} values were found"
        )));
    }
    Ok(parent.children().filter(roxmltree::Node::is_element))
}

fn local_link(
    node: roxmltree::Node<'_, '_>,
    object_attribute: &str,
    subelements: Vec<String>,
) -> Result<Option<LinkTarget>, CodecError> {
    if object_attribute == "obj" {
        reject_link_aliases(node, &["obj", "sub"])?;
    } else {
        reject_link_aliases(node, &[object_attribute])?;
    }
    LinkTarget::optional_from_wire(LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some(retained_attr(node, object_attribute)?),
        subelements,
    })
    .map_err(CodecError::Malformed)
}

fn xlink(node: roxmltree::Node<'_, '_>) -> Result<Option<LinkTarget>, CodecError> {
    reject_link_aliases(node, &["name", "file", "sub"])?;
    let file = node.attribute("file").map(str::to_owned);
    let has_children = node.children().any(|child| child.is_element());
    let subelements = match (node.attribute("sub"), node.attribute("count")) {
        (Some(_), None) if !has_children => vec![restored_subelement(node, "sub")?],
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
            let children = counted_children(node, "Sub", "App::PropertyXLink")?;
            let mut subelements = Vec::new();
            for child in children {
                if child.children().any(|value| value.is_element()) {
                    return Err(CodecError::Malformed(
                        "App::PropertyXLink Sub carrier has nested values".into(),
                    ));
                }
                subelements.push(restored_subelement(child, "value")?);
            }
            subelements
        }
        (None, None) if !has_children => Vec::new(),
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
    LinkTarget::optional_from_wire(LinkTargetWire {
        document: file.filter(|file| !file.is_empty()),
        document_attribute: Some("file".to_owned()),
        object: Some(retained_attr(node, "name")?),
        subelements,
    })
    .map_err(CodecError::Malformed)
}

fn restored_subelement(
    node: roxmltree::Node<'_, '_>,
    primary_attribute: &str,
) -> Result<String, CodecError> {
    let primary = node.attribute(primary_attribute).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{} element has no {primary_attribute} attribute",
            node.tag_name().name()
        ))
    })?;
    Ok(node.attribute("shadowed").unwrap_or(primary).to_owned())
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
    let mut sections = root.children().filter(|node| node.has_tag_name(tag));
    let first = sections
        .next()
        .ok_or_else(|| CodecError::malformed(format_args!("Document.xml has no {tag} section")))?;
    if sections.next().is_some() {
        Err(CodecError::malformed(format_args!(
            "Document.xml has duplicate {tag} sections"
        )))
    } else {
        Ok(first)
    }
}

fn extension_id(owner: &str, name: &str, order: usize) -> String {
    crate::native::native_child_id("extension", owner, &format!("{order}:{name}"))
}

fn retained_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Result<String, CodecError> {
    required_attr(node, name).map(str::to_owned)
}

fn required_attr<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Result<&'a str, CodecError> {
    node.attribute(name).ok_or_else(|| {
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
