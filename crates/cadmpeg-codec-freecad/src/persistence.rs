// SPDX-License-Identifier: Apache-2.0
//! Generic `FreeCAD` object and property graph recovery.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::dialect::FcstdDialect;
use crate::native::{
    copy_xml_text, malformed, DynamicPropertyMeta, ExtensionRecord, LinkTarget, LinkTargetWire,
    ObjectRecord, PropertyFamily, PropertyRecord, ValueRecord,
};
use crate::resource::{decode_reserved_vec, reserve_charged_vec_items};

const MAX_OBJECTS: usize = 1_000_000;
const MAX_PROPERTY_VALUE_XML_BYTES: usize = 16 * 1024 * 1024;

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
}

/// Recover the persistence graph, charging retained property XML against the session.
pub(crate) fn parse_with_context(
    bytes: &[u8],
    schema_version: &str,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Graph, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    if let Some(ctx) = ctx {
        ctx.charge_work(
            bytes.len() as u64,
            "FCStd persistence XML lexical admission",
        )?;
    }
    if let (Some(ctx), Some((nodes, _))) = (ctx, crate::container::xml_envelope_counts(bytes)) {
        ctx.charge_collection_items(nodes, "FCStd persistence XML node tree")?;
    }
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| crate::resource::malformed_optional(ctx,
            format_args!("invalid Document.xml: {error}"),
            "FCStd persistence diagnostic"))?;
    parse_document(
        text,
        &xml,
        FcstdDialect::from_schema_version(schema_version),
        ctx,
    )
}

fn charge_items(
    ctx: Option<&DecodeContext<'_>>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count as u64, operation)?;
    }
    Ok(())
}

fn parse_document(
    text: &str,
    xml: &roxmltree::Document<'_>,
    schema: FcstdDialect,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Graph, CodecError> {
    let root = xml.root_element();
    // Schema 2 is its own element vocabulary. Every other declared schema, and
    // every undeclared one, is read with the Objects/ObjectData/Object
    // vocabulary: the nearest declared strategy is attempted rather than
    // refused on a discriminant allowlist, and the attempt is self-limiting
    // because an element vocabulary that does not fit fails below exactly as a
    // corrupt schema-4 document does. `crate::dialect` charges the
    // dialect-unverified loss for the undeclared case.
    let (declarations_tag, data_tag, record_tag) = schema.persistence_tags();
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
    charge_items(ctx, declared_count, "FCStd object declarations")?;
    if schema == FcstdDialect::Schema2 && objects_node.attribute("Dependencies").is_some() {
        return Err(CodecError::Malformed(
            "schema 2 Features cannot carry object dependencies".into(),
        ));
    }

    let dependencies_enabled =
        schema != FcstdDialect::Schema2 && objects_node.attribute("Dependencies").is_some();
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
    let dependency_node_count = objects_node
        .children()
        .filter(|node| node.has_tag_name("ObjectDeps"))
        .count();
    charge_items(
        ctx,
        dependency_node_count,
        "FCStd object dependency records",
    )?;
    let dependency_nodes = objects_node
        .children()
        .filter(|node| node.has_tag_name("ObjectDeps"));
    let mut dependency_records = decode_reserved_vec(ctx, dependency_node_count, "FCStd object dependency records")?;
    dependency_records.extend(dependency_nodes);
    if (!dependencies_enabled && !dependency_records.is_empty())
        || (dependencies_enabled && dependency_records.len() != declared_count)
    {
        return Err(CodecError::Malformed(
            "ObjectDeps records do not match the Objects dependency envelope".into(),
        ));
    }
    let mut dependency_map = HashMap::<String, DependencyInfo>::new();
    for (order, node) in dependency_records.into_iter().enumerate() {
        let name = retained_attr(ctx, node, "Name", "FCStd dependency owner name")?;
        let dependency_item_count = node
            .children()
            .filter(|child| child.has_tag_name("Dep"))
            .count();
        charge_items(ctx, dependency_item_count, "FCStd object dependencies")?;
        let mut dependencies = decode_reserved_vec(ctx, dependency_item_count, "FCStd object dependencies")?;
        for child in node.children().filter(|child| child.has_tag_name("Dep")) {
            dependencies.push(retained_attr(ctx, child, "Name", "FCStd dependency name")?);
        }
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
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "FCStd dependency lookup")?;
            dependency_map.try_reserve(1).map_err(|_| crate::resource::collection_allocation_failed(ctx, 1, "FCStd dependency lookup"))?;
        }
        dependency_map.insert(name, DependencyInfo { dependencies, allow_partial, order });
    }

    let mut data_by_name = HashMap::new();
    for node in data_node
        .children()
        .filter(|node| node.has_tag_name(record_tag))
    {
        charge_items(ctx, 1, "FCStd object data lookup")?;
        if let Some(ctx) = ctx {
            data_by_name.try_reserve(1).map_err(|_| crate::resource::collection_allocation_failed(ctx, 1, "FCStd object data lookup"))?;
        }
        let name = retained_attr(ctx, node, "name", "FCStd object data name")?;
        if data_by_name.contains_key(&name) {
            return Err(CodecError::malformed(format_args!(
                "duplicate ObjectData name {name}"
            )));
        }
        data_by_name.insert(name, node);
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

    charge_items(ctx, declared_count, "FCStd object records")?;
    let mut objects: Vec<ObjectRecord> = decode_reserved_vec(ctx, declared_count, "FCStd object records")?;
    for (order, node) in objects_node
        .children()
        .filter(|node| node.has_tag_name(record_tag))
        .enumerate()
    {
        let name = retained_attr(ctx, node, "name", "FCStd object name")?;
        for prior in &objects {
            if let Some(ctx) = ctx {
                ctx.charge_work(1, "FCStd duplicate object names")?;
            }
            if prior.name == name {
            return Err(CodecError::malformed(format_args!(
                "duplicate object declaration name {name}"
            )));
            }
        }
        let type_name = retained_attr(ctx, node, "type", "FCStd object type")?;
        let id = object_id(ctx, &name)?;
        let data_node = data_by_name.get(&name);
        let attributes = node
            .attributes()
            .filter(|attribute| !matches!(attribute.name(), "name" | "type" | "id" | "ViewType"))
            .map(|attribute| {
                charge_items(ctx, 1, "FCStd object attributes")?;
                Ok((
                    copy_xml_text(ctx, attribute.name(), "FCStd object attribute name")?,
                    copy_xml_text(ctx, attribute.value(), "FCStd object attribute")?,
                ))
            })
            .collect::<Result<_, CodecError>>()?;
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
        let (dependencies, dependency_allow_partial) = match dependency {
            Some(dependency) => (dependency.dependencies, dependency.allow_partial),
            None => (Vec::new(), None),
        };
        objects.push(ObjectRecord {
            id,
            name,
            type_name,
            persistent_id: node.attribute("id").and_then(|value| value.parse().ok()),
            view_type: node.attribute("ViewType").map(|value| copy_xml_text(ctx, value, "FCStd object view type")).transpose()?,
            attributes,
            dependencies,
            dependency_allow_partial,
            order,
            data: data_node
                .map(|data| {
                    crate::native::RetainedXml::from_source(
                        ctx,
                        &text[data.range()],
                        data.range().start as u64,
                        "FCStd object XML",
                    )
                })
                .transpose()?,
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
    for object in &mut objects {
        for dependency in &mut object.dependencies {
            if !data_by_name.contains_key(dependency) {
                return Err(CodecError::malformed(format_args!(
                    "object {} depends on missing object {dependency}",
                    object.name
                )));
            }
            *dependency = object_id(ctx, dependency)?;
        }
    }

    let mut properties = Vec::new();
    let mut extensions = Vec::new();
    let document_property_containers = root
        .children()
        .filter(|node| node.has_tag_name("Properties"))
        .count();
    charge_items(
        ctx,
        document_property_containers,
        "FCStd document property containers",
    )?;
    let document_properties = root
        .children()
        .filter(|node| node.has_tag_name("Properties"));
    let mut document_property_nodes = decode_reserved_vec(ctx, document_property_containers, "FCStd document property containers")?;
    document_property_nodes.extend(document_properties);
    match document_property_nodes.as_slice() {
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
        let children_count = data.children().filter(roxmltree::Node::is_element).count();
        charge_items(ctx, children_count, "FCStd object data children")?;
        let children = data
            .children()
            .filter(roxmltree::Node::is_element);
        let mut child_nodes = decode_reserved_vec(ctx, children_count, "FCStd object data children")?;
        child_nodes.extend(children);
        let mut extension_containers = child_nodes
            .iter()
            .filter(|node| node.has_tag_name("Extensions"))
            .copied();
        let extension_container = extension_containers.next();
        if extension_containers.next().is_some() {
            return Err(malformed(format!(
                "object {} has multiple direct Extensions containers",
                object.id
            )));
        }
        let mut property_containers = child_nodes
            .iter()
            .filter(|node| node.has_tag_name("Properties"))
            .copied();
        let property_container = property_containers.next();
        if property_containers.next().is_some() {
            return Err(malformed(format!(
                "object {} has multiple direct Properties containers",
                object.id
            )));
        }
        if let (Some(extensions), Some(properties)) =
            (extension_container, property_container)
        {
            if extensions.range().start > properties.range().start {
                return Err(malformed(format!(
                    "object {} writes Properties before Extensions",
                    object.id
                )));
            }
        }
        let mut extension_ids_by_start = HashMap::new();
        if let Some(extensions_node) = extension_container {
            let extension_count = extensions_node
                .children()
                .filter(|node| node.has_tag_name("Extension"))
                .count();
            charge_items(ctx, extension_count, "FCStd extension nodes")?;
            let nodes = extensions_node
                .children()
                .filter(|node| node.has_tag_name("Extension"));
            let mut extension_nodes = decode_reserved_vec(ctx, extension_count, "FCStd extension nodes")?;
            extension_nodes.extend(nodes);
            let declared = extensions_node
                .attribute("Count")
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| {
                    CodecError::Malformed("Extensions Count is missing or invalid".into())
                })?;
            if declared != extension_nodes.len() {
                return Err(CodecError::malformed(format_args!(
                    "Extensions Count={declared} but {} records were found for {}",
                    extension_nodes.len(),
                    object.id
                )));
            }
            let mut extension_names = HashSet::new();
            let mut extension_types = HashSet::new();
            for (order, node) in extension_nodes.into_iter().enumerate() {
                let name = retained_attr(ctx, node, "name", "FCStd extension name")?;
                let type_name = retained_attr(ctx, node, "type", "FCStd extension type")?;
                if extension_names.contains(&name) {
                    return Err(malformed(format!(
                        "duplicate extension name {name} for {}",
                        object.id
                    )));
                }
                charge_items(ctx, 1, "FCStd extension name set")?;
                if let Some(ctx) = ctx {
                    extension_names.try_reserve(1).map_err(|_| crate::resource::collection_allocation_failed(ctx, 1, "FCStd extension name set"))?;
                }
                extension_names.insert(copy_xml_text(ctx, &name, "FCStd extension name copy")?);
                if extension_types.contains(&type_name) {
                    return Err(malformed(format!(
                        "duplicate extension type {type_name} for {}",
                        object.id
                    )));
                }
                charge_items(ctx, 1, "FCStd extension type set")?;
                if let Some(ctx) = ctx {
                    extension_types.try_reserve(1).map_err(|_| crate::resource::collection_allocation_failed(ctx, 1, "FCStd extension type set"))?;
                }
                extension_types.insert(copy_xml_text(ctx, &type_name, "FCStd extension type copy")?);
                let id = extension_id(ctx, &object.id, &name, order)?;
                charge_items(ctx, 1, "FCStd extension identity lookup")?;
                if let Some(ctx) = ctx {
                    extension_ids_by_start.try_reserve(1).map_err(|_| crate::resource::collection_allocation_failed(ctx, 1, "FCStd extension identity lookup"))?;
                }
                extension_ids_by_start.insert(node.range().start, copy_xml_text(ctx, &id, "FCStd extension identity copy")?);
                charge_items(ctx, 1, "FCStd extension records")?;
                reserve_charged_vec_items(ctx, &mut extensions, 1, "FCStd extension records")?;
                extensions.push(ExtensionRecord {
                    id,
                    owner: copy_xml_text(ctx, &object.id, "FCStd extension owner")?,
                    name,
                    type_name,
                    order,
                    raw_xml: copy_xml_text(ctx, &text[node.range()], "FCStd extension XML")?,
                });
            }
        }
        if let Some(container) = property_container {
            parse_properties(text, container, &object.id, &mut properties, ctx)?;
        }
        if let Some(extensions_node) = extension_container {
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
                        cadmpeg_core::text::NonBlankString::new(object_id(ctx, target)?).ok_or_else(
                            || CodecError::malformed("link object identity must not be empty"),
                        )?,
                    );
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
    let node_count = container
        .children()
        .filter(|node| node.has_tag_name("Property"))
        .count();
    charge_items(ctx, node_count, "FCStd property nodes")?;
    let nodes = container
        .children()
        .filter(|node| node.has_tag_name("Property"));
    let mut property_nodes = decode_reserved_vec(ctx, node_count, "FCStd property nodes")?;
    property_nodes.extend(nodes);
    let transient_node_count = container
        .children()
        .filter(|node| node.has_tag_name("_Property"))
        .count();
    charge_items(ctx, transient_node_count, "FCStd transient property nodes")?;
    let transient_nodes = container
        .children()
        .filter(|node| node.has_tag_name("_Property"));
    let mut transient_property_nodes = decode_reserved_vec(ctx, transient_node_count, "FCStd transient property nodes")?;
    transient_property_nodes.extend(transient_nodes);
    let all_nodes = transient_property_nodes.iter().chain(property_nodes.iter());
    for (index, node) in all_nodes.clone().enumerate() {
        let name = node.attribute("name").ok_or_else(|| {
            CodecError::malformed(format_args!("{} element has no name attribute", node.tag_name().name()))
        })?;
        for prior in all_nodes.clone().take(index) {
            if let Some(ctx) = ctx {
                ctx.charge_work(1, "FCStd duplicate property names")?;
            }
            if prior.attribute("name") == Some(name) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate property name {name} for {owner}"
                )));
            }
        }
    }
    let declared = container
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::Malformed("Properties Count is missing or invalid".into()))?;
    if declared != property_nodes.len() {
        return Err(CodecError::malformed(format_args!(
            "Properties Count={declared} but {} properties were found for {owner}",
            property_nodes.len()
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
    if declared_transient != transient_property_nodes.len() {
        return Err(CodecError::malformed(format_args!(
            "Properties TransientCount={declared_transient} but {} transient properties were found for {owner}",
            transient_property_nodes.len()
        )));
    }
    for (order, node) in transient_property_nodes.into_iter().enumerate() {
        let name = retained_attr(ctx, node, "name", "FCStd transient property name")?;
        let type_name = retained_attr(ctx, node, "type", "FCStd transient property type")?;
        charge_items(ctx, 1, "FCStd transient property records")?;
        reserve_charged_vec_items(ctx, output, 1, "FCStd transient property records")?;
        output.push(PropertyRecord {
            id: property_id(ctx, owner, &name)?,
            owner: copy_xml_text(ctx, owner, "FCStd transient property owner")?,
            name,
            family: property_family(&type_name),
            type_name,
            status: node
                .attribute("status")
                .and_then(|value| value.parse().ok()),
            body: crate::native::PropertyBody::Transient,
            order,
            xml: crate::native::RetainedXml::from_source(
                ctx,
                &text[node.range()],
                node.range().start as u64,
                "FCStd transient property XML",
            )?,
        });
    }
    for (order, node) in property_nodes.into_iter().enumerate() {
        let name = retained_attr(ctx, node, "name", "FCStd persisted property name")?;
        let type_name = retained_attr(ctx, node, "type", "FCStd persisted property type")?;
        let mut retained_value_bytes = 0_usize;
        let value_count = node
            .descendants()
            .filter(|value| value.is_element() && *value != node)
            .count();
        charge_items(ctx, value_count, "FCStd property value records")?;
        let mut values = decode_reserved_vec(ctx, value_count, "FCStd property value records")?;
        for (value_order, value) in node.descendants()
            .filter(|value| value.is_element() && *value != node)
            .enumerate()
        {
                let len = value.range().len();
                retained_value_bytes = retained_value_bytes
                    .checked_add(len)
                    .filter(|total| *total <= MAX_PROPERTY_VALUE_XML_BYTES)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "property {name} retained value XML limit exceeded"
                        ))
                    })?;
                values.push(ValueRecord {
                    tag: copy_xml_text(ctx, value.tag_name().name(), "FCStd value tag")?,
                    order: value_order,
                    attributes: value
                        .attributes()
                        .map(|attribute| {
                            charge_items(ctx, 1, "FCStd value attributes")?;
                            Ok((
                                copy_xml_text(ctx, attribute.name(), "FCStd value attribute name")?,
                                copy_xml_text(ctx, attribute.value(), "FCStd value attribute")?,
                            ))
                        })
                        .collect::<Result<_, CodecError>>()?,
                    text: value
                        .text()
                        .map(|text| copy_xml_text(ctx, text, "FCStd value text"))
                        .transpose()?,
                    raw_xml: copy_xml_text(ctx, &text[value.range()], "FCStd value XML")?,
                });
        }
        let links = if link_grammar(&type_name).is_some() {
            parse_link_targets(node, &type_name, ctx)?
        } else {
            Vec::new()
        };
        let mut side_entries = Vec::new();
        for value in &values {
            for (name, entry_name) in &value.attributes {
                let selected = (matches!(name.as_str(), "file" | "File") && !is_xlink_type(&type_name))
                    || (property_family(&type_name) == PropertyFamily::File
                        && matches!(name.as_str(), "name" | "Name"));
                if selected && !entry_name.is_empty() {
                    charge_items(ctx, 1, "FCStd side entry references")?;
                    reserve_charged_vec_items(ctx, &mut side_entries, 1, "FCStd side entry references")?;
                    side_entries.push(copy_xml_text(ctx, entry_name, "FCStd side entry name")?);
                }
            }
        }
        charge_items(ctx, 1, "FCStd persisted property records")?;
        reserve_charged_vec_items(ctx, output, 1, "FCStd persisted property records")?;
        output.push(PropertyRecord {
            id: property_id(ctx, owner, &name)?,
            owner: copy_xml_text(ctx, owner, "FCStd persisted property owner")?,
            name,
            family: property_family(&type_name),
            type_name,
            status: node
                .attribute("status")
                .and_then(|value| value.parse().ok()),
            body: crate::native::PropertyBody::Persisted {
                values,
                links,
                side_entries,
                dynamic: node
                    .attribute("group")
                    .map(|group| -> Result<DynamicPropertyMeta, CodecError> {
                        Ok(DynamicPropertyMeta {
                            group: copy_xml_text(ctx, group, "FCStd dynamic property group")?,
                            documentation: node
                                .attribute("doc")
                                .map(|doc| {
                                    copy_xml_text(ctx, doc, "FCStd dynamic property documentation")
                                })
                                .transpose()?,
                            attributes: node.attribute("attr").and_then(|value| value.parse().ok()),
                            read_only: bool_attr(node.attribute("ro")),
                            hidden: bool_attr(node.attribute("hide")),
                        })
                    })
                    .transpose()?,
            },
            order,
            xml: crate::native::RetainedXml::from_source(
                ctx,
                &text[node.range()],
                node.range().start as u64,
                "FCStd persisted property XML",
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
    parse_link_targets(property, grammar_type, None).map(|_| ())
}

fn parse_link_targets(
    property: roxmltree::Node<'_, '_>,
    type_name: &str,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Vec<Option<LinkTarget>>, CodecError> {
    let grammar = link_grammar(type_name);
    let Some(grammar) = grammar else {
        return Ok(Vec::new());
    };
    let root = single_value_element(property, grammar.root_tag(), type_name)?;
    match grammar {
        LinkGrammar::Link => {
            reject_nested_link_value(root)?;
            charge_items(ctx, 1, "FCStd link target records")?;
            let mut targets = decode_reserved_vec(ctx, 1, "FCStd link target records")?;
            targets.push(local_link(root, "value", Vec::new(), ctx)?);
            Ok(targets)
        }
        LinkGrammar::LinkList => {
            let children = counted_children(root, "Link", type_name, ctx)?;
            let mut targets = decode_reserved_vec(ctx, children.len(), "FCStd link target or subelement records")?;
            for node in children {
                reject_nested_link_value(node)?;
                targets.push(local_link(node, "value", Vec::new(), ctx)?);
            }
            Ok(targets)
        }
        LinkGrammar::LinkSub => {
            let children = counted_children(root, "Sub", type_name, ctx)?;
            let mut subelements = decode_reserved_vec(ctx, children.len(), "FCStd link target or subelement records")?;
            for node in children {
                reject_nested_link_value(node)?;
                subelements.push(restored_subelement(node, "value", ctx)?);
            }
            charge_items(ctx, 1, "FCStd link target records")?;
            let mut targets = decode_reserved_vec(ctx, 1, "FCStd link target records")?;
            targets.push(local_link(root, "value", subelements, ctx)?);
            Ok(targets)
        }
        LinkGrammar::LinkSubList => {
            let children = counted_children(root, "Link", type_name, ctx)?;
            let mut targets = decode_reserved_vec(ctx, children.len(), "FCStd link target or subelement records")?;
            for node in children {
                reject_nested_link_value(node)?;
                let sub = restored_subelement(node, "sub", ctx)?;
                charge_items(ctx, 1, "FCStd link subelements")?;
                let mut subelements = decode_reserved_vec(ctx, 1, "FCStd link subelements")?;
                subelements.push(sub);
                targets.push(local_link(node, "obj", subelements, ctx)?);
            }
            Ok(targets)
        }
        LinkGrammar::XLink => {
            charge_items(ctx, 1, "FCStd link target records")?;
            let mut targets = decode_reserved_vec(ctx, 1, "FCStd link target records")?;
            targets.push(xlink(root, ctx)?);
            Ok(targets)
        }
        LinkGrammar::XLinkSubList => {
            let children = counted_children(root, "XLink", type_name, ctx)?;
            let mut targets = decode_reserved_vec(ctx, children.len(), "FCStd link target or subelement records")?;
            for node in children {
                targets.push(xlink(node, ctx)?);
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
    ctx: Option<&DecodeContext<'_>>,
) -> Result<impl ExactSizeIterator<Item = roxmltree::Node<'a, 'input>>, CodecError> {
    let count = parent.attribute("count").ok_or_else(|| {
        CodecError::malformed(format_args!("{} element has no count attribute", parent.tag_name().name()))
    })?
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("{type_name} count is invalid")))?;
    charge_items(ctx, count, "FCStd link nodes")?;
    charge_items(ctx, count, "FCStd link target or subelement records")?;
    let children = parent
        .children()
        .filter(roxmltree::Node::is_element);
    let mut child_nodes = decode_reserved_vec(ctx, count, "FCStd link nodes")?;
    child_nodes.extend(children);
    if child_nodes.len() != count || child_nodes.iter().any(|child| !child.has_tag_name(tag)) {
        return Err(CodecError::malformed(format_args!(
            "{type_name} count={count} but {} {tag} values were found",
            child_nodes.len()
        )));
    }
    Ok(child_nodes.into_iter())
}

fn local_link(
    node: roxmltree::Node<'_, '_>,
    object_attribute: &str,
    subelements: Vec<String>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<LinkTarget>, CodecError> {
    if object_attribute == "obj" {
        reject_link_aliases(node, &["obj", "sub"])?;
    } else {
        reject_link_aliases(node, &[object_attribute])?;
    }
    LinkTarget::optional_from_wire(LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some(retained_attr(
            ctx,
            node,
            object_attribute,
            "FCStd link object",
        )?),
        subelements,
    })
    .map_err(CodecError::Malformed)
}

fn xlink(
    node: roxmltree::Node<'_, '_>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<LinkTarget>, CodecError> {
    reject_link_aliases(node, &["name", "file", "sub"])?;
    let file = node
        .attribute("file")
        .map(|value| copy_xml_text(ctx, value, "FCStd link document"))
        .transpose()?;
    let child_count = node.children().filter(roxmltree::Node::is_element).count();
    charge_items(ctx, child_count, "FCStd XLink children")?;
    let children = node
        .children()
        .filter(roxmltree::Node::is_element);
    let mut child_nodes = decode_reserved_vec(ctx, child_count, "FCStd XLink children")?;
    child_nodes.extend(children);
    let subelements = match (node.attribute("sub"), node.attribute("count")) {
        (Some(_), None) if child_nodes.is_empty() => {
            charge_items(ctx, 1, "FCStd link subelements")?;
            let mut subelements = decode_reserved_vec(ctx, 1, "FCStd link subelements")?;
            subelements.push(restored_subelement(node, "sub", ctx)?);
            subelements
        }
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
            let children = counted_children(node, "Sub", "App::PropertyXLink", ctx)?;
            let mut subelements = decode_reserved_vec(ctx, children.len(), "FCStd link target or subelement records")?;
            for child in children {
                if child.children().any(|value| value.is_element()) {
                    return Err(CodecError::Malformed(
                        "App::PropertyXLink Sub carrier has nested values".into(),
                    ));
                }
                subelements.push(restored_subelement(child, "value", ctx)?);
            }
            subelements
        }
        (None, None) if child_nodes.is_empty() => Vec::new(),
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
        object: Some(retained_attr(ctx, node, "name", "FCStd link object")?),
        subelements,
    })
    .map_err(CodecError::Malformed)
}

fn restored_subelement(
    node: roxmltree::Node<'_, '_>,
    primary_attribute: &str,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<String, CodecError> {
    let primary = node.attribute(primary_attribute).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{} element has no {primary_attribute} attribute",
            node.tag_name().name()
        ))
    })?;
    copy_xml_text(
        ctx,
        node.attribute("shadowed").unwrap_or(primary),
        "FCStd link subelement",
    )
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
    let mut sections = root
        .children()
        .filter(|node| node.has_tag_name(tag));
    let first = sections.next().ok_or_else(|| CodecError::malformed(format_args!(
            "Document.xml has no {tag} section"
        )))?;
    if sections.next().is_some() {
        Err(CodecError::malformed(format_args!(
            "Document.xml has duplicate {tag} sections"
        )))
    } else {
        Ok(first)
    }
}

fn object_id(ctx: Option<&DecodeContext<'_>>, name: &str) -> Result<String, CodecError> {
    match ctx {
        Some(ctx) => crate::native::native_id_charged(ctx, "object", name),
        None => Ok(crate::native::native_id("object", name)),
    }
}

fn property_id(ctx: Option<&DecodeContext<'_>>, owner: &str, name: &str) -> Result<String, CodecError> {
    match ctx {
        Some(ctx) => crate::native::native_child_id_charged(ctx, "property", owner, name),
        None => Ok(crate::native::native_child_id("property", owner, name)),
    }
}

fn extension_id(
    ctx: Option<&DecodeContext<'_>>,
    owner: &str,
    name: &str,
    order: usize,
) -> Result<String, CodecError> {
    let order = order.to_string();
    match ctx {
        Some(ctx) => {
            let child = crate::resource::retained_join(ctx, &[&order, name], ":", "FCStd extension identity key")?;
            crate::native::native_child_id_charged(ctx, "extension", owner, &child)
        }
        None => Ok(crate::native::native_child_id("extension", owner, &format!("{order}:{name}"))),
    }
}

fn retained_attr(
    ctx: Option<&DecodeContext<'_>>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let value = node.attribute(name).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{} element has no {name} attribute",
            node.tag_name().name()
        ))
    })?;
    copy_xml_text(ctx, value, operation)
}

fn bool_attr(value: Option<&str>) -> Option<bool> {
    value.map(|value| matches!(value, "1" | "true" | "True" | "TRUE"))
}

#[cfg(test)]
pub(crate) mod tests;
