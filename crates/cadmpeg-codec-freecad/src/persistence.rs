// SPDX-License-Identifier: Apache-2.0
//! Generic `FreeCAD` object and property graph recovery.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::dialect::FcstdDialect;
use crate::native::{
    DynamicPropertyMeta, ExtensionRecord, LinkTarget, LinkTargetWire, ObjectRecord, PropertyFamily,
    PropertyRecord, ValueRecord,
};

const MAX_OBJECTS: usize = 1_000_000;
const MAX_PROPERTY_VALUE_XML_BYTES: usize = 16 * 1024 * 1024;
const ATTRIBUTE_SEARCH: &str = "FCStd XML attribute search";
const ELEMENT_NAME: &str = "FCStd XML element name";

struct DependencyInfo<'input> {
    dependencies: Vec<&'input str>,
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
    ctx: &DecodeContext<'_>,
) -> Result<Graph, CodecError> {
    let text = ctx
        .validate_utf8(bytes, "validate FreeCAD XML UTF-8")?
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let admitted_xml = ctx.parse_xml(text, "FreeCAD XML tree").map_err(|error| {
        let CodecError::Malformed(error) = error else {
            return error;
        };
        crate::resource::malformed_charged(
            ctx,
            format_args!("invalid Document.xml: {error}"),
            "FCStd persistence diagnostic",
        )
    })?;
    let xml = admitted_xml.document();
    parse_document(
        text,
        xml,
        FcstdDialect::from_schema_version(schema_version),
        ctx,
    )
}

fn parse_document(
    text: &str,
    xml: &roxmltree::Document<'_>,
    schema: FcstdDialect,
    ctx: &DecodeContext<'_>,
) -> Result<Graph, CodecError> {
    let root = ctx.xml_root_element(xml, "FCStd document element search")?;
    // Schema 2 is its own element vocabulary. Every other declared schema, and
    // every undeclared one, is read with the Objects/ObjectData/Object
    // vocabulary: the nearest declared strategy is attempted rather than
    // refused on a discriminant allowlist, and the attempt is self-limiting
    // because an element vocabulary that does not fit fails below exactly as a
    // corrupt schema-4 document does. `crate::dialect` charges the
    // dialect-unverified loss for the undeclared case.
    let (declarations_tag, data_tag, record_tag) = schema.persistence_tags();
    let objects_node = unique_section(root, declarations_tag, ctx)?;
    let data_node = unique_section(root, data_tag, ctx)?;

    let declared_count =
        parsed_attr::<usize>(ctx, objects_node, "Count", "FCStd object count parse")?.ok_or_else(
            || {
                crate::resource::malformed_charged(
                    ctx,
                    format_args!("{declarations_tag} Count is missing or invalid"),
                    "FCStd persistence diagnostic",
                )
            },
        )?;
    // One framing pass counts the declarations, keeps the dependency records
    // and notes a dependency record written after a declaration.
    let mut actual_count = 0_usize;
    let mut dependencies_after_declaration = false;
    let mut dependency_records_storage =
        ctx.reserve_scoped(0, "FCStd object dependency records")?;
    let mut dependency_records = Vec::new();
    let mut declarations = objects_node.children();
    while let Some(node) =
        ctx.next_charged(&mut declarations, "FCStd object declaration framing")?
    {
        if ctx.xml_has_tag_name(node, record_tag, ELEMENT_NAME)? {
            actual_count = actual_count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("FCStd object declaration framing", u64::MAX, u64::MAX)
            })?;
        } else if ctx.xml_has_tag_name(node, "ObjectDeps", ELEMENT_NAME)? {
            dependencies_after_declaration |= actual_count != 0;
            ctx.push_scoped_vec(
                &mut dependency_records_storage,
                &mut dependency_records,
                node,
                "FCStd object dependency records",
            )?;
        }
    }
    if actual_count != declared_count {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "{declarations_tag} Count={declared_count} but {actual_count} declarations were found"
            ),
            "FCStd persistence diagnostic",
        ));
    }
    let object_limit = ctx
        .policy()
        .limits
        .max_entities
        .min(cadmpeg_core::decode::u64_from_index(MAX_OBJECTS));
    let requested_objects = cadmpeg_core::decode::u64_from_index(declared_count);
    if requested_objects > object_limit {
        return Err(ctx.refuse_codec_limit("FCStd object count", object_limit, requested_objects));
    }
    let declares_dependencies = ctx
        .xml_attribute(objects_node, "Dependencies", ATTRIBUTE_SEARCH)?
        .is_some();
    if schema == FcstdDialect::Schema2 && declares_dependencies {
        return Err(CodecError::Malformed(
            "schema 2 Features cannot carry object dependencies".into(),
        ));
    }
    let dependencies_enabled = declares_dependencies;
    if dependencies_after_declaration {
        return Err(CodecError::Malformed(
            "ObjectDeps records must precede object declarations".into(),
        ));
    }
    if (!dependencies_enabled && !dependency_records.is_empty())
        || (dependencies_enabled && dependency_records.len() != declared_count)
    {
        return Err(CodecError::Malformed(
            "ObjectDeps records do not match the Objects dependency envelope".into(),
        ));
    }
    // Names borrow the document text; the tables hold no copies.
    let mut dependency_storage = ctx.reserve_scoped(0, "FCStd dependency lookup")?;
    let mut dependency_map = HashMap::<&str, DependencyInfo<'_>>::new();
    for (order, node) in ctx
        .admit_iter(&dependency_records, "FCStd object dependency records")?
        .copied()
        .enumerate()
    {
        let name = required_attr(ctx, node, "Name")?;
        let mut dependencies = Vec::new();
        let mut children = node.children();
        while let Some(child) = ctx.next_charged(&mut children, "FCStd dependency search")? {
            if !ctx.xml_has_tag_name(child, "Dep", ELEMENT_NAME)? {
                continue;
            }
            let dependency = required_attr(ctx, child, "Name")?;
            ctx.push_scoped_vec(
                &mut dependency_storage,
                &mut dependencies,
                dependency,
                "FCStd object dependencies",
            )?;
        }
        let dependency_count =
            parsed_attr::<usize>(ctx, node, "Count", "FCStd dependency count parse")?.ok_or_else(
                || CodecError::Malformed("ObjectDeps Count is missing or invalid".into()),
            )?;
        if dependency_count != dependencies.len() {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!(
                    "ObjectDeps {name} Count={dependency_count} but {} dependencies were found",
                    dependencies.len()
                ),
                "FCStd persistence diagnostic",
            ));
        }
        let allow_partial = match ctx.xml_attribute(node, "AllowPartial", ATTRIBUTE_SEARCH)? {
            Some(value) => Some(
                ctx.parse_text::<std::num::NonZeroU64>(value, "FCStd allow-partial parse")?
                    .map_err(|_| {
                        CodecError::Malformed("ObjectDeps AllowPartial must be positive".into())
                    })?,
            ),
            None => None,
        };
        let info = DependencyInfo {
            dependencies,
            allow_partial,
            order,
        };
        let replaced = dependency_storage.with_storage(|| {
            ctx.insert_hash_map(&mut dependency_map, name, info, "FCStd dependency lookup")
        })?;
        if replaced.is_some() {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("duplicate ObjectDeps name {name}"),
                "FCStd persistence diagnostic",
            ));
        }
    }

    let mut data_storage = ctx.reserve_scoped(0, "FCStd object data lookup")?;
    let mut data_by_name = HashMap::new();
    let mut data_children = data_node.children();
    while let Some(node) = ctx.next_charged(&mut data_children, "FCStd object data records")? {
        if !ctx.xml_has_tag_name(node, record_tag, ELEMENT_NAME)? {
            continue;
        }
        let name = required_attr(ctx, node, "name")?;
        let replaced = data_storage.with_storage(|| {
            ctx.insert_hash_map(&mut data_by_name, name, node, "FCStd object data lookup")
        })?;
        if replaced.is_some() {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("duplicate ObjectData name {name}"),
                "FCStd persistence diagnostic",
            ));
        }
    }
    let data_count =
        parsed_attr::<usize>(ctx, data_node, "Count", "FCStd object data count parse")?
            .ok_or_else(|| {
                crate::resource::malformed_charged(
                    ctx,
                    format_args!("{data_tag} Count is missing or invalid"),
                    "FCStd persistence diagnostic",
                )
            })?;
    if data_count != data_by_name.len() {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "{data_tag} Count={data_count} but {} records were found",
                data_by_name.len()
            ),
            "FCStd persistence diagnostic",
        ));
    }

    let mut objects: Vec<ObjectRecord> =
        ctx.vector_storage(declared_count, "FCStd object records")?;
    let mut declared_names_storage = ctx.reserve_scoped(0, "FCStd duplicate object names")?;
    let mut declared_names = HashSet::new();
    let mut order = 0usize;
    let mut declarations = objects_node.children();
    while let Some(node) =
        ctx.next_charged(&mut declarations, "FCStd object declaration records")?
    {
        if !ctx.xml_has_tag_name(node, record_tag, ELEMENT_NAME)? {
            continue;
        }
        let declared_name = required_attr(ctx, node, "name")?;
        if !declared_names_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut declared_names,
                declared_name,
                "FCStd duplicate object names",
            )
        })? {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("duplicate object declaration name {declared_name}"),
                "FCStd persistence diagnostic",
            ));
        }
        let name = ctx.copy_retained_text(declared_name, "FCStd object name")?;
        let type_name = retained_attr(ctx, node, "type", "FCStd object type")?;
        let identity = crate::native::object_identity::ObjectIdentity::from_name(ctx, name)?;
        let data_node =
            ctx.get_hash_map(&data_by_name, declared_name, "FCStd object data lookup")?;
        let mut attributes = std::collections::BTreeMap::new();
        let mut attribute_nodes = node.attributes();
        while let Some(attribute) =
            ctx.next_charged(&mut attribute_nodes, "FCStd object attribute records")?
        {
            if matches!(attribute.name(), "name" | "type" | "id" | "ViewType") {
                continue;
            }
            let name = ctx.copy_retained_text(attribute.name(), "FCStd object attribute name")?;
            let value = ctx.copy_retained_text(attribute.value(), "FCStd object attribute")?;
            drop(ctx.insert_btree_map(&mut attributes, name, value, "FCStd object attributes")?);
        }
        let dependency = ctx.remove_hash_map(
            &mut dependency_map,
            declared_name,
            "FCStd dependency lookup",
        )?;
        if dependencies_enabled
            && dependency
                .as_ref()
                .is_none_or(|dependency| dependency.order != order)
        {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("ObjectDeps order does not match object {}", identity.name()),
                "FCStd persistence diagnostic",
            ));
        }
        let (dependencies, dependency_allow_partial) = match dependency {
            Some(dependency) => {
                let mut ids = ctx.vector_storage(
                    dependency.dependencies.len(),
                    "FCStd object dependency identities",
                )?;
                for &target in ctx.admit_iter(
                    &dependency.dependencies,
                    "FCStd object dependency identities",
                )? {
                    if !ctx.contains_key_hash_map(
                        &data_by_name,
                        target,
                        "FCStd object data lookup",
                    )? {
                        return Err(crate::resource::malformed_charged(
                            ctx,
                            format_args!(
                                "object {} depends on missing object {target}",
                                identity.name()
                            ),
                            "FCStd persistence diagnostic",
                        ));
                    }
                    ctx.push_vec(
                        &mut ids,
                        crate::native::native_id_charged(ctx, "object", target)?,
                        "FCStd object dependency identities",
                    )?;
                }
                (ids, dependency.allow_partial)
            }
            None => (Vec::new(), None),
        };
        ctx.push_vec(
            &mut objects,
            ObjectRecord {
                identity,
                type_name,
                persistent_id: parsed_attr::<i64>(ctx, node, "id", "FCStd persistent id parse")?,
                view_type: ctx
                    .xml_attribute(node, "ViewType", ATTRIBUTE_SEARCH)?
                    .map(|value| ctx.copy_retained_text(value, "FCStd object view type"))
                    .transpose()?,
                attributes,
                dependencies,
                dependency_allow_partial,
                order,
                data: data_node
                    .map(|data| {
                        crate::native::RetainedXml::from_source(
                            ctx,
                            &text[data.range()],
                            cadmpeg_core::decode::u64_from_index(data.range().start),
                            "FCStd object XML",
                        )
                    })
                    .transpose()?,
            },
            "FCStd object records",
        )?;
        order = order.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("FCStd object declaration order", u64::MAX, u64::MAX)
        })?;
    }
    drop((declared_names, declared_names_storage));

    if !dependency_map.is_empty() {
        return Err(CodecError::Malformed(
            "ObjectDeps names do not match object declarations".into(),
        ));
    }
    drop((dependency_map, dependency_storage));
    if data_by_name.len() != objects.len() {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!("object declarations and {data_tag} identities disagree"),
            "FCStd persistence diagnostic",
        ));
    }

    let mut properties = Vec::new();
    let mut extensions = Vec::new();
    let mut document_properties = None;
    let mut root_children = root.children();
    while let Some(node) = ctx.next_charged(&mut root_children, "FCStd document property search")? {
        if !ctx.xml_has_tag_name(node, "Properties", ELEMENT_NAME)? {
            continue;
        }
        if document_properties.replace(node).is_some() {
            return Err(CodecError::Malformed(
                "Document.xml has multiple root Properties containers".into(),
            ));
        }
    }
    if let Some(document_properties) = document_properties {
        parse_properties(
            text,
            document_properties,
            &crate::native::native_id("document", "0"),
            &mut properties,
            ctx,
        )?;
    }
    for object in ctx.admit_iter(&objects, "FCStd object data association")? {
        let data = ctx
            .get_hash_map(
                &data_by_name,
                object.name().as_str(),
                "FCStd object data lookup",
            )?
            .ok_or_else(|| {
                crate::resource::malformed_charged(
                    ctx,
                    format_args!("missing ObjectData for {}", object.name()),
                    "FCStd persistence diagnostic",
                )
            })?;
        let mut extension_container = None;
        let mut property_container = None;
        let mut child_nodes = data.children();
        while let Some(node) =
            ctx.next_charged(&mut child_nodes, "FCStd object data container search")?
        {
            let (slot, tag) = if ctx.xml_has_tag_name(node, "Extensions", ELEMENT_NAME)? {
                (&mut extension_container, "Extensions")
            } else if ctx.xml_has_tag_name(node, "Properties", ELEMENT_NAME)? {
                (&mut property_container, "Properties")
            } else {
                continue;
            };
            if slot.replace(node).is_some() {
                return Err(crate::resource::malformed_charged(
                    ctx,
                    format_args!(
                        "object {} has multiple direct {tag} containers",
                        object.id()
                    ),
                    "FCStd persistence diagnostic",
                ));
            }
        }
        if let (Some(extensions), Some(properties)) = (extension_container, property_container) {
            if extensions.range().start > properties.range().start {
                return Err(crate::resource::malformed_charged(
                    ctx,
                    format_args!("object {} writes Properties before Extensions", object.id()),
                    "FCStd persistence diagnostic",
                ));
            }
        }
        // Each extension's node, kept beside the record that names it, so its
        // properties are read after the object's own without a second search.
        let mut extension_nodes_storage = ctx.reserve_scoped(0, "FCStd extension nodes")?;
        let mut extension_nodes = Vec::new();
        let first_extension = extensions.len();
        if let Some(extensions_node) = extension_container {
            let mut children = extensions_node.children();
            while let Some(node) = ctx.next_charged(&mut children, "FCStd extension node search")? {
                if ctx.xml_has_tag_name(node, "Extension", ELEMENT_NAME)? {
                    ctx.push_scoped_vec(
                        &mut extension_nodes_storage,
                        &mut extension_nodes,
                        node,
                        "FCStd extension nodes",
                    )?;
                }
            }
            let declared =
                parsed_attr::<usize>(ctx, extensions_node, "Count", "FCStd extension count parse")?
                    .ok_or_else(|| {
                        CodecError::Malformed("Extensions Count is missing or invalid".into())
                    })?;
            if declared != extension_nodes.len() {
                return Err(crate::resource::malformed_charged(
                    ctx,
                    format_args!(
                        "Extensions Count={declared} but {} records were found for {}",
                        extension_nodes.len(),
                        object.id()
                    ),
                    "FCStd persistence diagnostic",
                ));
            }
            let mut extension_sets_storage = ctx.reserve_scoped(0, "FCStd extension sets")?;
            let mut extension_names = HashSet::new();
            let mut extension_types = HashSet::new();
            for (order, node) in ctx
                .admit_iter(&extension_nodes, "FCStd extension nodes")?
                .copied()
                .enumerate()
            {
                let name = required_attr(ctx, node, "name")?;
                let type_name = required_attr(ctx, node, "type")?;
                if !extension_sets_storage.with_storage(|| {
                    ctx.insert_hash_set(&mut extension_names, name, "FCStd extension name set")
                })? {
                    return Err(crate::resource::malformed_charged(
                        ctx,
                        format_args!("duplicate extension name {name} for {}", object.id()),
                        "FCStd persistence diagnostic",
                    ));
                }
                if !extension_sets_storage.with_storage(|| {
                    ctx.insert_hash_set(&mut extension_types, type_name, "FCStd extension type set")
                })? {
                    return Err(crate::resource::malformed_charged(
                        ctx,
                        format_args!("duplicate extension type {type_name} for {}", object.id()),
                        "FCStd persistence diagnostic",
                    ));
                }
                ctx.push_vec(
                    &mut extensions,
                    ExtensionRecord {
                        id: extension_id(ctx, object.id(), name, order)?,
                        owner: ctx.copy_retained_text(object.id(), "FCStd extension owner")?,
                        name: ctx.copy_retained_text(name, "FCStd extension name")?,
                        type_name: ctx.copy_retained_text(type_name, "FCStd extension type")?,
                        order,
                        raw_xml: ctx
                            .copy_retained_text(&text[node.range()], "FCStd extension XML")?,
                    },
                    "FCStd extension records",
                )?;
            }
        }
        if let Some(container) = property_container {
            parse_properties(text, container, object.id(), &mut properties, ctx)?;
        }
        for (&extension, record) in ctx
            .admit_iter(&extension_nodes, "FCStd extension property search")?
            .zip(&extensions[first_extension..])
        {
            let mut children = extension.children();
            while let Some(container) =
                ctx.next_charged(&mut children, "FCStd extension Properties search")?
            {
                if ctx.xml_has_tag_name(container, "Properties", ELEMENT_NAME)? {
                    parse_properties(text, container, &record.id, &mut properties, ctx)?;
                }
            }
        }
    }
    for property in ctx.admit_iter(&mut properties, "FCStd mutable property visits")? {
        let crate::native::PropertyBody::Persisted { links, .. } = &mut property.body else {
            continue;
        };
        for link in ctx.admit_iter(links, "FCStd mutable link visits")? {
            let Some(link) = link.as_mut() else {
                continue;
            };
            if link.document().is_none() {
                let Some(target) = link.object() else {
                    continue;
                };
                if ctx.contains_key_hash_map(&data_by_name, target, "FCStd object data lookup")? {
                    link.set_object(
                        cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            crate::native::native_id_charged(ctx, "object", target)?,
                            "validate nonblank text",
                        )?
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
    })
}

fn parse_properties(
    text: &str,
    container: roxmltree::Node<'_, '_>,
    owner: &str,
    output: &mut Vec<PropertyRecord>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    // One pass sorts the children into persisted and transient properties and
    // refuses a name either kind repeats; the name set borrows the document.
    let mut nodes_storage = ctx.reserve_scoped(0, "FCStd property nodes")?;
    let mut property_nodes = Vec::new();
    let mut transient_property_nodes = Vec::new();
    let mut names = HashSet::new();
    let mut children = container.children();
    while let Some(node) = ctx.next_charged(&mut children, "FCStd property search")? {
        let nodes = if ctx.xml_has_tag_name(node, "Property", ELEMENT_NAME)? {
            &mut property_nodes
        } else if ctx.xml_has_tag_name(node, "_Property", ELEMENT_NAME)? {
            &mut transient_property_nodes
        } else {
            continue;
        };
        let name = required_attr(ctx, node, "name")?;
        if !nodes_storage.with_storage(|| {
            ctx.insert_hash_set(&mut names, name, "FCStd duplicate property names")
        })? {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("duplicate property name {name} for {owner}"),
                "FCStd persistence diagnostic",
            ));
        }
        ctx.push_scoped_vec(&mut nodes_storage, nodes, node, "FCStd property nodes")?;
    }
    drop(names);
    let declared = parsed_attr::<usize>(ctx, container, "Count", "FCStd property count parse")?
        .ok_or_else(|| CodecError::Malformed("Properties Count is missing or invalid".into()))?;
    if declared != property_nodes.len() {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "Properties Count={declared} but {} properties were found for {owner}",
                property_nodes.len()
            ),
            "FCStd persistence diagnostic",
        ));
    }
    let declared_transient =
        match ctx.xml_attribute(container, "TransientCount", ATTRIBUTE_SEARCH)? {
            Some(value) => ctx
                .parse_text::<usize>(value, "FCStd transient property count parse")?
                .map_err(|_| {
                    CodecError::Malformed("Properties TransientCount is invalid".into())
                })?,
            None => 0,
        };
    if declared_transient != transient_property_nodes.len() {
        return Err(crate::resource::malformed_charged(ctx, format_args!(
            "Properties TransientCount={declared_transient} but {} transient properties were found for {owner}",
            transient_property_nodes.len()
        ), "FCStd persistence diagnostic"));
    }
    for (order, node) in ctx
        .admit_iter(&transient_property_nodes, "FCStd transient property nodes")?
        .copied()
        .enumerate()
    {
        let name = retained_attr(ctx, node, "name", "FCStd transient property name")?;
        let type_name = retained_attr(ctx, node, "type", "FCStd transient property type")?;
        ctx.push_vec(
            output,
            PropertyRecord {
                id: crate::native::native_child_id_charged(ctx, "property", owner, &name)?,
                owner: ctx.copy_retained_text(owner, "FCStd transient property owner")?,
                name,
                family: property_family(&type_name),
                type_name,
                status: parsed_attr::<u64>(ctx, node, "status", "FCStd property status parse")?,
                body: crate::native::PropertyBody::Transient,
                order,
                xml: crate::native::RetainedXml::from_source(
                    ctx,
                    &text[node.range()],
                    cadmpeg_core::decode::u64_from_index(node.range().start),
                    "FCStd transient property XML",
                )?,
            },
            "FCStd transient property records",
        )?;
    }
    for (order, node) in ctx
        .admit_iter(&property_nodes, "FCStd persisted property nodes")?
        .copied()
        .enumerate()
    {
        let name = retained_attr(ctx, node, "name", "FCStd persisted property name")?;
        let type_name = retained_attr(ctx, node, "type", "FCStd persisted property type")?;
        let family = property_family(&type_name);
        let file_carrier_by_file = !is_xlink_type(&type_name);
        let file_carrier_by_name = family == PropertyFamily::File;
        let mut retained_value_bytes = 0_usize;
        let mut values = Vec::new();
        let mut side_entries = Vec::new();
        let mut value_order = 0usize;
        let mut descendants = node.descendants();
        while let Some(value) =
            ctx.next_charged(&mut descendants, "FCStd property value elements")?
        {
            if !value.is_element() || value == node {
                continue;
            }
            let len = value.range().len();
            let total = retained_value_bytes.checked_add(len).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "FCStd property retained value XML",
                    cadmpeg_core::decode::u64_from_index(MAX_PROPERTY_VALUE_XML_BYTES),
                    u64::MAX,
                )
            })?;
            if total > MAX_PROPERTY_VALUE_XML_BYTES {
                return Err(ctx.refuse_codec_limit(
                    "FCStd property retained value XML",
                    cadmpeg_core::decode::u64_from_index(MAX_PROPERTY_VALUE_XML_BYTES),
                    cadmpeg_core::decode::u64_from_index(total),
                ));
            }
            retained_value_bytes = total;
            let mut attributes = std::collections::BTreeMap::new();
            let mut attribute_nodes = value.attributes();
            while let Some(attribute) =
                ctx.next_charged(&mut attribute_nodes, "FCStd value attribute records")?
            {
                let name =
                    ctx.copy_retained_text(attribute.name(), "FCStd value attribute name")?;
                let text = ctx.copy_retained_text(attribute.value(), "FCStd value attribute")?;
                drop(ctx.insert_btree_map(
                    &mut attributes,
                    name,
                    text,
                    "FCStd value attributes",
                )?);
            }
            // The side-entry carriers of this property's family, in attribute
            // name order.
            for (name, entry_name) in
                ctx.admit_iter(&attributes, "FCStd value attribute references")?
            {
                let selected = (file_carrier_by_file && matches!(name.as_str(), "file" | "File"))
                    || (file_carrier_by_name && matches!(name.as_str(), "name" | "Name"));
                if selected && !entry_name.is_empty() {
                    let entry_name = ctx.copy_retained_text(entry_name, "FCStd side entry name")?;
                    ctx.push_vec(&mut side_entries, entry_name, "FCStd side entry references")?;
                }
            }
            ctx.push_vec(
                &mut values,
                ValueRecord {
                    tag: ctx.copy_retained_text(value.tag_name().name(), "FCStd value tag")?,
                    order: value_order,
                    attributes,
                    text: value
                        .text()
                        .map(|text| ctx.copy_retained_text(text, "FCStd value text"))
                        .transpose()?,
                    raw_xml: ctx.copy_retained_text(&text[value.range()], "FCStd value XML")?,
                },
                "FCStd property value records",
            )?;
            value_order = value_order.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("FCStd property value order", u64::MAX, u64::MAX)
            })?;
        }
        let links = if link_grammar(&type_name).is_some() {
            parse_link_targets(node, &type_name, ctx)?
        } else {
            Vec::new()
        };
        let dynamic = match ctx.xml_attribute(node, "group", ATTRIBUTE_SEARCH)? {
            Some(group) => Some(DynamicPropertyMeta {
                group: ctx.copy_retained_text(group, "FCStd dynamic property group")?,
                documentation: ctx
                    .xml_attribute(node, "doc", ATTRIBUTE_SEARCH)?
                    .map(|doc| ctx.copy_retained_text(doc, "FCStd dynamic property documentation"))
                    .transpose()?,
                attributes: parsed_attr::<i64>(
                    ctx,
                    node,
                    "attr",
                    "FCStd dynamic property attributes parse",
                )?,
                read_only: bool_attr(ctx.xml_attribute(node, "ro", ATTRIBUTE_SEARCH)?),
                hidden: bool_attr(ctx.xml_attribute(node, "hide", ATTRIBUTE_SEARCH)?),
            }),
            None => None,
        };
        ctx.push_vec(
            output,
            PropertyRecord {
                id: crate::native::native_child_id_charged(ctx, "property", owner, &name)?,
                owner: ctx.copy_retained_text(owner, "FCStd persisted property owner")?,
                name,
                family,
                type_name,
                status: parsed_attr::<u64>(ctx, node, "status", "FCStd property status parse")?,
                body: crate::native::PropertyBody::Persisted {
                    values,
                    links,
                    side_entries,
                    dynamic,
                },
                order,
                xml: crate::native::RetainedXml::from_source(
                    ctx,
                    &text[node.range()],
                    cadmpeg_core::decode::u64_from_index(node.range().start),
                    "FCStd persisted property XML",
                )?,
            },
            "FCStd persisted property records",
        )?;
    }
    Ok(())
}

pub(crate) fn validate_link_property(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    type_name: &str,
) -> Result<(), CodecError> {
    let grammar_type = if type_name == "App::PropertyPlacementLink" {
        "App::PropertyLink"
    } else {
        type_name
    };
    parse_link_targets(property, grammar_type, ctx).map(|_| ())
}

fn parse_link_targets(
    property: roxmltree::Node<'_, '_>,
    type_name: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<Option<LinkTarget>>, CodecError> {
    let grammar = link_grammar(type_name);
    let Some(grammar) = grammar else {
        return Ok(Vec::new());
    };
    let root = single_value_element(property, grammar.root_tag(), type_name, ctx)?;
    match grammar {
        LinkGrammar::Link => {
            reject_nested_link_value(root, ctx)?;
            let mut targets = ctx.vector_storage(1, "FCStd link target records")?;
            ctx.push_vec(
                &mut targets,
                local_link(root, "value", Vec::new(), ctx)?,
                "FCStd link target records",
            )?;
            Ok(targets)
        }
        LinkGrammar::LinkList => {
            let (children, _children_storage) = counted_children(root, "Link", type_name, ctx)?;
            let mut targets =
                ctx.vector_storage(children.len(), "FCStd link target or subelement records")?;
            for node in ctx
                .admit_iter(&children, "FCStd link target nodes")?
                .copied()
            {
                reject_nested_link_value(node, ctx)?;
                ctx.push_vec(
                    &mut targets,
                    local_link(node, "value", Vec::new(), ctx)?,
                    "FCStd link target or subelement records",
                )?;
            }
            Ok(targets)
        }
        LinkGrammar::LinkSub => {
            let (children, _children_storage) = counted_children(root, "Sub", type_name, ctx)?;
            let mut subelements =
                ctx.vector_storage(children.len(), "FCStd link target or subelement records")?;
            for node in ctx
                .admit_iter(&children, "FCStd link subelement nodes")?
                .copied()
            {
                reject_nested_link_value(node, ctx)?;
                ctx.push_vec(
                    &mut subelements,
                    restored_subelement(node, "value", ctx)?,
                    "FCStd link target or subelement records",
                )?;
            }
            let mut targets = ctx.vector_storage(1, "FCStd link target records")?;
            ctx.push_vec(
                &mut targets,
                local_link(root, "value", subelements, ctx)?,
                "FCStd link target records",
            )?;
            Ok(targets)
        }
        LinkGrammar::LinkSubList => {
            let (children, _children_storage) = counted_children(root, "Link", type_name, ctx)?;
            let mut targets =
                ctx.vector_storage(children.len(), "FCStd link target or subelement records")?;
            for node in ctx
                .admit_iter(&children, "FCStd link target nodes")?
                .copied()
            {
                reject_nested_link_value(node, ctx)?;
                let sub = restored_subelement(node, "sub", ctx)?;
                let mut subelements = ctx.vector_storage(1, "FCStd link subelements")?;
                ctx.push_vec(&mut subelements, sub, "FCStd link subelements")?;
                ctx.push_vec(
                    &mut targets,
                    local_link(node, "obj", subelements, ctx)?,
                    "FCStd link target or subelement records",
                )?;
            }
            Ok(targets)
        }
        LinkGrammar::XLink => {
            ctx.collect_indexed_vec(1, "FCStd link target records", |_| xlink(root, ctx))
        }
        LinkGrammar::XLinkSubList => {
            let (children, _children_storage) = counted_children(root, "XLink", type_name, ctx)?;
            ctx.collect_indexed_vec(
                children.len(),
                "FCStd link target or subelement records",
                |index| xlink(children[index], ctx),
            )
        }
    }
}

fn reject_nested_link_value(
    node: roxmltree::Node<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if has_element_child(node, "FCStd nested link search", ctx)? {
        return Err(CodecError::Malformed(
            "link carrier contains nested element values".into(),
        ));
    }
    Ok(())
}

/// Steps the children until the first element.
fn has_element_child(
    node: roxmltree::Node<'_, '_>,
    operation: &'static str,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut children = node.children();
    while let Some(child) = ctx.next_charged(&mut children, operation)? {
        if child.is_element() {
            return Ok(true);
        }
    }
    Ok(false)
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
    ctx: &DecodeContext<'_>,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let mut elements = property.children();
    let mut value = None;
    while let Some(element) = ctx.next_charged(&mut elements, "FCStd link value search")? {
        if !element.is_element() {
            continue;
        }
        if value.is_some() || !ctx.xml_has_tag_name(element, tag, ELEMENT_NAME)? {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("{type_name} requires exactly one {tag} value"),
                "FCStd persistence diagnostic",
            ));
        }
        value = Some(element);
    }
    value.ok_or_else(|| {
        crate::resource::malformed_charged(
            ctx,
            format_args!("{type_name} requires one {tag} value"),
            "FCStd persistence diagnostic",
        )
    })
}

/// The element children of a counted link carrier, after their count and tag
/// are checked against the carrier's `count` attribute.
fn counted_children<'a, 'input, 'ctx>(
    parent: roxmltree::Node<'a, 'input>,
    tag: &'static str,
    type_name: &str,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<
    (
        Vec<roxmltree::Node<'a, 'input>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let count_text = ctx
        .xml_attribute(parent, "count", ATTRIBUTE_SEARCH)?
        .ok_or_else(|| {
            crate::resource::malformed_charged(
                ctx,
                format_args!(
                    "{} element has no count attribute",
                    parent.tag_name().name()
                ),
                "FCStd persistence diagnostic",
            )
        })?;
    let count = ctx
        .parse_text::<usize>(count_text, "FCStd link count parse")?
        .map_err(|_| {
            crate::resource::malformed_charged(
                ctx,
                format_args!("{type_name} count is invalid"),
                "FCStd persistence diagnostic",
            )
        })?;
    let mut valid_tags = true;
    let mut storage = ctx.reserve_scoped(0, "FCStd link nodes")?;
    let mut child_nodes = Vec::new();
    let mut children = parent.children();
    while let Some(child) = ctx.next_charged(&mut children, "FCStd link child framing")? {
        if !child.is_element() {
            continue;
        }
        valid_tags &= ctx.xml_has_tag_name(child, tag, ELEMENT_NAME)?;
        ctx.push_scoped_vec(&mut storage, &mut child_nodes, child, "FCStd link nodes")?;
    }
    if child_nodes.len() != count || !valid_tags {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "{type_name} count={count} but {} {tag} values were found",
                child_nodes.len()
            ),
            "FCStd persistence diagnostic",
        ));
    }
    Ok((child_nodes, storage))
}

fn local_link(
    node: roxmltree::Node<'_, '_>,
    object_attribute: &str,
    subelements: Vec<String>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<LinkTarget>, CodecError> {
    if object_attribute == "obj" {
        reject_link_aliases(node, &["obj", "sub"], ctx)?;
    } else {
        reject_link_aliases(node, &[object_attribute], ctx)?;
    }
    LinkTarget::optional_from_wire(LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some(ctx.validate_nonblank_text(
            retained_attr(ctx, node, object_attribute, "FCStd link object")?,
            "validate object",
        )?),
        subelements,
    })
    .map_err(CodecError::Malformed)
}

fn xlink(
    node: roxmltree::Node<'_, '_>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<LinkTarget>, CodecError> {
    reject_link_aliases(node, &["name", "file", "sub"], ctx)?;
    let file = ctx
        .xml_attribute(node, "file", ATTRIBUTE_SEARCH)?
        .map(|value| ctx.copy_retained_text(value, "FCStd link document"))
        .transpose()?;
    let has_children = has_element_child(node, "FCStd XLink element children", ctx)?;
    let count = ctx.xml_attribute(node, "count", ATTRIBUTE_SEARCH)?;
    let subelements = match (ctx.xml_attribute(node, "sub", ATTRIBUTE_SEARCH)?, count) {
        (Some(_), None) if !has_children => {
            let mut subelements = ctx.vector_storage(1, "FCStd link subelements")?;
            ctx.push_vec(
                &mut subelements,
                restored_subelement(node, "sub", ctx)?,
                "FCStd link subelements",
            )?;
            subelements
        }
        (Some(_), None) => {
            return Err(CodecError::Malformed(
                "App::PropertyXLink sub carrier has nested values".into(),
            ));
        }
        (None, Some(count)) => {
            if ctx
                .parse_text::<usize>(count, "FCStd XLink count parse")?
                .is_ok_and(|count| count == 0)
            {
                return Err(CodecError::Malformed(
                    "App::PropertyXLink uses count only for one or more Sub values".into(),
                ));
            }
            let (children, _children_storage) =
                counted_children(node, "Sub", "App::PropertyXLink", ctx)?;
            let mut subelements =
                ctx.vector_storage(children.len(), "FCStd link target or subelement records")?;
            for child in ctx
                .admit_iter(&children, "FCStd XLink subelement nodes")?
                .copied()
            {
                if has_element_child(child, "FCStd XLink Sub nested search", ctx)? {
                    return Err(CodecError::Malformed(
                        "App::PropertyXLink Sub carrier has nested values".into(),
                    ));
                }
                ctx.push_vec(
                    &mut subelements,
                    restored_subelement(child, "value", ctx)?,
                    "FCStd link target or subelement records",
                )?;
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
        document: (file.filter(|file| !file.is_empty()))
            .map(|value| ctx.validate_nonblank_text(value, "validate document"))
            .transpose()?,
        document_attribute: Some("file".to_owned()),
        object: Some(ctx.validate_nonblank_text(
            retained_attr(ctx, node, "name", "FCStd link object")?,
            "validate object",
        )?),
        subelements,
    })
    .map_err(CodecError::Malformed)
}

fn restored_subelement(
    node: roxmltree::Node<'_, '_>,
    primary_attribute: &str,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    let primary = required_attr(ctx, node, primary_attribute)?;
    ctx.copy_retained_text(
        ctx.xml_attribute(node, "shadowed", ATTRIBUTE_SEARCH)?
            .unwrap_or(primary),
        "FCStd link subelement",
    )
}

fn reject_link_aliases(
    node: roxmltree::Node<'_, '_>,
    allowed: &[&str],
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    const CARRIERS: &[&str] = &[
        "value", "Value", "object", "Object", "obj", "Obj", "name", "Name", "document", "Document",
        "doc", "Doc", "file", "File", "sub", "Sub",
    ];
    // Each comparison is against a literal carrier name, so it reads at most
    // that literal's bytes; the attribute step is the input-sized work.
    let mut attributes = node.attributes();
    while let Some(attribute) = ctx.next_charged(&mut attributes, "FCStd link carrier search")? {
        let name = attribute.name();
        if CARRIERS.contains(&name) && !allowed.contains(&name) {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!(
                    "{} has unsupported link carrier {name}",
                    node.tag_name().name()
                ),
                "FCStd persistence diagnostic",
            ));
        }
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
    ctx: &DecodeContext<'_>,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let mut selected = None;
    let mut children = root.children();
    while let Some(node) = ctx.next_charged(&mut children, "FCStd document section search")? {
        if !ctx.xml_has_tag_name(node, tag, ELEMENT_NAME)? {
            continue;
        }
        if selected.replace(node).is_some() {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("Document.xml has duplicate {tag} sections"),
                "FCStd persistence diagnostic",
            ));
        }
    }
    selected.ok_or_else(|| {
        crate::resource::malformed_charged(
            ctx,
            format_args!("Document.xml has no {tag} section"),
            "FCStd persistence diagnostic",
        )
    })
}

fn extension_id(
    ctx: &DecodeContext<'_>,
    owner: &str,
    name: &str,
    order: usize,
) -> Result<String, CodecError> {
    let (order, _order_storage) =
        ctx.format_scoped(format_args!("{order}"), "FCStd extension order text")?;
    let (child, _child_storage) = ctx
        .with_scoped_storage("FCStd extension identity key", || {
            ctx.join_retained(&[order.as_str(), name], ":", "FCStd extension identity key")
        })?;
    crate::native::native_child_id_charged(ctx, "extension", owner, &child)
}

/// The attribute's text, refused as malformed when the element omits it.
fn required_attr<'a>(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'a, '_>,
    name: &str,
) -> Result<&'a str, CodecError> {
    ctx.xml_attribute(node, name, ATTRIBUTE_SEARCH)?
        .ok_or_else(|| {
            crate::resource::malformed_charged(
                ctx,
                format_args!("{} element has no {name} attribute", node.tag_name().name()),
                "FCStd persistence diagnostic",
            )
        })
}

fn retained_attr(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    ctx.copy_retained_text(required_attr(ctx, node, name)?, operation)
}

/// The attribute parsed as `T`; absent and unparsable values are both `None`.
fn parsed_attr<T: cadmpeg_core::decode::text::TextScalar>(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
    operation: &'static str,
) -> Result<Option<T>, CodecError> {
    match ctx.xml_attribute(node, name, ATTRIBUTE_SEARCH)? {
        Some(value) => Ok(ctx.parse_text::<T>(value, operation)?.ok()),
        None => Ok(None),
    }
}

fn bool_attr(value: Option<&str>) -> Option<bool> {
    value.map(|value| matches!(value, "1" | "true" | "True" | "TRUE"))
}

#[cfg(test)]
pub(crate) mod tests;
