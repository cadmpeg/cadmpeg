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
    ctx: &DecodeContext<'_>,
) -> Result<Graph, CodecError> {
    let text = ctx.validate_utf8(bytes, "validate FreeCAD XML UTF-8")?
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
    let root = crate::xml::document_element(ctx, xml)?;
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

    let declared_count = crate::xml::attribute(ctx, objects_node, "Count")?
        .map(|value| {
            ctx.parse_text::<usize>(value, "FCStd object count parse")
                .map(|parsed| parsed.ok())
        })
        .transpose()?
        .flatten()
        .ok_or_else(|| {
            crate::resource::malformed_charged(
                ctx,
                format_args!("{declarations_tag} Count is missing or invalid"),
                "FCStd persistence diagnostic",
            )
        })?;
    let mut actual_count = 0_usize;
    let mut next_declaration = objects_node.first_child();
    while let Some(node) = next_declaration {
        ctx.charge_work(1, "FCStd object declaration framing")?;
        next_declaration = node.next_sibling();
        if !crate::xml::has_tag_name(ctx, node, record_tag)? {
            continue;
        }
        actual_count = actual_count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("FCStd object declaration framing", u64::MAX, u64::MAX)
        })?;
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
    let (object_children, _object_children_storage) =
        crate::xml::children(ctx, objects_node, "FCStd object declarations")?;
    if schema == FcstdDialect::Schema2 && crate::xml::attribute(ctx, objects_node, "Dependencies")?.is_some() {
        return Err(CodecError::Malformed(
            "schema 2 Features cannot carry object dependencies".into(),
        ));
    }

    let dependencies_enabled =
        schema != FcstdDialect::Schema2 && crate::xml::attribute(ctx, objects_node, "Dependencies")?.is_some();
    let mut saw_object_declaration = false;
    for &child in ctx.admit_iter(&object_children, "FCStd object dependency ordering")? {
        if !child.is_element() { continue; }
        if crate::xml::has_tag_name(ctx, child, record_tag)? {
            saw_object_declaration = true;
        } else if saw_object_declaration && crate::xml::has_tag_name(ctx, child, "ObjectDeps")? {
            return Err(CodecError::Malformed(
                "ObjectDeps records must precede object declarations".into(),
            ));
        }
    }
    let mut dependency_records_storage = ctx.reserve_scoped(0, "FCStd object dependency records")?;
    let mut dependency_records = Vec::new();
    for &node in ctx.admit_iter(&object_children, "FCStd object dependency search")? {
        if crate::xml::has_tag_name(ctx, node, "ObjectDeps")? {
            ctx.push_scoped_vec(&mut dependency_records_storage, &mut dependency_records,
                node, "FCStd object dependency records")?;
        }
    }
    if (!dependencies_enabled && !dependency_records.is_empty())
        || (dependencies_enabled && dependency_records.len() != declared_count)
    {
        return Err(CodecError::Malformed(
            "ObjectDeps records do not match the Objects dependency envelope".into(),
        ));
    }
    let mut dependency_storage = ctx.reserve_scoped(0, "FCStd dependency lookup")?;
    let mut dependency_map = HashMap::<String, DependencyInfo>::new();
    for (order, node) in ctx
        .admit_iter(&dependency_records, "FCStd object dependency records")?
        .copied()
        .enumerate()
    {
        let name = dependency_storage
            .with_storage(|| retained_attr(ctx, node, "Name", "FCStd dependency owner name"))?;
        let (dependency_children, _dependency_children_storage) =
            crate::xml::children(ctx, node, "FCStd dependency children")?;
        let mut dependencies = Vec::new();
        for &child in ctx.admit_iter(&dependency_children, "FCStd dependency search")? {
            if !crate::xml::has_tag_name(ctx, child, "Dep")? { continue; }
            let dependency = retained_attr(ctx, child, "Name", "FCStd dependency name")?;
            ctx.push_vec(&mut dependencies, dependency, "FCStd object dependencies")?;
        }
        let dependency_count = crate::xml::attribute(ctx, node, "Count")?
            .map(|value| {
                ctx.parse_text::<usize>(value, "FCStd dependency count parse")
                    .map(|parsed| parsed.ok())
            })
            .transpose()?
            .flatten()
            .ok_or_else(|| {
                CodecError::Malformed("ObjectDeps Count is missing or invalid".into())
            })?;
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
        let allow_partial = match crate::xml::attribute(ctx, node, "AllowPartial")? {
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
        if ctx.contains_key_hash_map(&dependency_map, &name, "FCStd dependency lookup")? {
            return Err(crate::resource::malformed_charged(ctx,
                format_args!("duplicate ObjectDeps name {name}"), "FCStd persistence diagnostic"));
        }
        dependency_storage.with_storage(|| {
            drop(ctx.insert_hash_map(&mut dependency_map, name, info, "FCStd dependency lookup")?);
            Ok::<(), CodecError>(())
        })?;
    }

    let mut data_storage = ctx.reserve_scoped(0, "FCStd object data lookup")?;
    let mut data_by_name = HashMap::new();
    let (data_children, _data_children_storage) =
        crate::xml::children(ctx, data_node, "FCStd object data children")?;
    for &node in ctx.admit_iter(&data_children, "FCStd object data records")? {
        if !crate::xml::has_tag_name(ctx, node, record_tag)? { continue; }
        let name = data_storage
            .with_storage(|| retained_attr(ctx, node, "name", "FCStd object data name"))?;
        if ctx.contains_key_hash_map(&data_by_name, &name, "FCStd object data lookup")? {
            return Err(crate::resource::malformed_charged(ctx,
                format_args!("duplicate ObjectData name {name}"), "FCStd persistence diagnostic"));
        }
        data_storage.with_storage(|| {
            // discarded-value: the preceding lookup excludes replacement.
            let _ = ctx.insert_hash_map(&mut data_by_name, name, node, "FCStd object data lookup")?;
            Ok::<(), CodecError>(())
        })?;
    }
    let data_count = crate::xml::attribute(ctx, data_node, "Count")?
        .map(|value| {
            ctx.parse_text::<usize>(value, "FCStd object data count parse")
                .map(|parsed| parsed.ok())
        })
        .transpose()?
        .flatten()
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
    let mut order = 0usize;
    for &node in ctx.admit_iter(&object_children, "FCStd object declaration records")? {
        if !crate::xml::has_tag_name(ctx, node, record_tag)? { continue; }
        let name = retained_attr(ctx, node, "name", "FCStd object name")?;
        for prior in ctx.admit_iter(&objects, "FCStd duplicate object names")? {
            if ctx.equal(prior.name().as_str(), name.as_str(), "FCStd duplicate object names")? {
                return Err(crate::resource::malformed_charged(
                    ctx,
                    format_args!("duplicate object declaration name {name}"),
                    "FCStd persistence diagnostic",
                ));
            }
        }
        let type_name = retained_attr(ctx, node, "type", "FCStd object type")?;
        let identity = crate::native::object_identity::ObjectIdentity::from_name(ctx, name)?;
        let data_node = ctx.get_hash_map(
            &data_by_name,
            identity.name(),
            "FCStd object data lookup",
        )?;
        let (attribute_nodes, _attribute_nodes_storage) = ctx.with_scoped_storage(
            "FCStd object attribute nodes", || ctx.collect_vec(node.attributes(), "FCStd object attribute nodes"))?;
        let mut attributes = std::collections::BTreeMap::new();
        for attribute in ctx.admit_iter(&attribute_nodes, "FCStd object attribute records")? {
            if matches!(attribute.name(), "name" | "type" | "id" | "ViewType") { continue; }
            let name = ctx.copy_retained_text(attribute.name(), "FCStd object attribute name")?;
            let value = ctx.copy_retained_text(attribute.value(), "FCStd object attribute")?;
            drop(ctx.insert_btree_map(&mut attributes, name, value, "FCStd object attributes")?);
        }
        let dependency = ctx.remove_hash_map(
            &mut dependency_map,
            identity.name(),
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
            Some(dependency) => (dependency.dependencies, dependency.allow_partial),
            None => (Vec::new(), None),
        };
        ctx.push_vec(&mut objects, ObjectRecord {
            identity,
            type_name,
            persistent_id: crate::xml::attribute(ctx, node, "id")?
                .map(|value| {
                    ctx.parse_text::<i64>(value, "FCStd persistent id parse")
                        .map(|parsed| parsed.ok())
                })
                .transpose()?
                .flatten(),
            view_type: crate::xml::attribute(ctx, node, "ViewType")?
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
        }, "FCStd object records")?;
        order = order.checked_add(1).ok_or_else(||
            ctx.refuse_codec_limit("FCStd object declaration order", u64::MAX, u64::MAX))?;
    }

    if !dependency_map.is_empty() {
        return Err(CodecError::Malformed(
            "ObjectDeps names do not match object declarations".into(),
        ));
    }
    if data_by_name.len() != objects.len() {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!("object declarations and {data_tag} identities disagree"),
            "FCStd persistence diagnostic",
        ));
    }
    for object_index in ctx.admit_iter(&(0..objects.len()), "FCStd mutable object visits")? {
        let object = &mut objects[object_index];
        let object_name = object.identity.name();
        for dependency_index in ctx.admit_iter(&(0..object.dependencies.len()), "FCStd mutable dependency visits")? {
            let dependency = &mut object.dependencies[dependency_index];
            if !ctx.contains_key_hash_map(
                &data_by_name,
                dependency,
                "FCStd object data lookup",
            )? {
                return Err(crate::resource::malformed_charged(
                    ctx,
                    format_args!("object {object_name} depends on missing object {dependency}"),
                    "FCStd persistence diagnostic",
                ));
            }
            *dependency = crate::native::native_id_charged(ctx, "object", dependency)?;
        }
    }

    let mut properties = Vec::new();
    let mut extensions = Vec::new();
    let (root_children, _root_children_storage) = crate::xml::children(ctx, root, "FCStd document children")?;
    let mut document_property_nodes_storage = ctx.reserve_scoped(0, "FCStd document property containers")?;
    let mut document_property_nodes = Vec::new();
    for &node in ctx.admit_iter(&root_children, "FCStd document property search")? {
        if crate::xml::has_tag_name(ctx, node, "Properties")? {
            ctx.push_scoped_vec(&mut document_property_nodes_storage, &mut document_property_nodes,
                node, "FCStd document property containers")?;
        }
    }
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
    for object in ctx.admit_iter(&objects, "FCStd object data association")? {
        let data = ctx.get_hash_map(&data_by_name, object.name(), "FCStd object data lookup")?.ok_or_else(|| {
            crate::resource::malformed_charged(
                ctx,
                format_args!("missing ObjectData for {}", object.name()),
                "FCStd persistence diagnostic",
            )
        })?;
        let (child_nodes, _child_nodes_storage) = crate::xml::children(ctx, *data, "FCStd object data children")?;
        let extension_index = ctx.position_by(&child_nodes, |node| crate::xml::has_tag_name(ctx, *node, "Extensions"), "FCStd extension container search")?;
        if let Some(index) = extension_index {
            if ctx.any_by(&child_nodes[index + 1..], |node| crate::xml::has_tag_name(ctx, *node, "Extensions"), "FCStd duplicate extension container search")? {
                return Err(crate::resource::malformed_charged(ctx,
                    format_args!("object {} has multiple direct Extensions containers", object.id()),
                    "FCStd persistence diagnostic"));
            }
        }
        let extension_container = extension_index.map(|index| child_nodes[index]);
        let property_index = ctx.position_by(&child_nodes, |node| crate::xml::has_tag_name(ctx, *node, "Properties"), "FCStd property container search")?;
        if let Some(index) = property_index {
            if ctx.any_by(&child_nodes[index + 1..], |node| crate::xml::has_tag_name(ctx, *node, "Properties"), "FCStd duplicate property container search")? {
                return Err(crate::resource::malformed_charged(ctx,
                    format_args!("object {} has multiple direct Properties containers", object.id()),
                    "FCStd persistence diagnostic"));
            }
        }
        let property_container = property_index.map(|index| child_nodes[index]);
        if let (Some(extensions), Some(properties)) = (extension_container, property_container) {
            if extensions.range().start > properties.range().start {
                return Err(crate::resource::malformed_charged(
                    ctx,
                    format_args!("object {} writes Properties before Extensions", object.id()),
                    "FCStd persistence diagnostic",
                ));
            }
        }
        let mut extension_storage = ctx.reserve_scoped(0, "FCStd extension lookup")?;
        let mut extension_ids_by_start = HashMap::new();
        if let Some(extensions_node) = extension_container {
            let (extension_children, _extension_children_storage) = crate::xml::children(ctx,
                extensions_node, "FCStd extension children")?;
            let mut extension_nodes_storage = ctx.reserve_scoped(0, "FCStd extension nodes")?;
            let mut extension_nodes = Vec::new();
            for &node in ctx.admit_iter(&extension_children, "FCStd extension node search")? {
                if crate::xml::has_tag_name(ctx, node, "Extension")? {
                    ctx.push_scoped_vec(&mut extension_nodes_storage, &mut extension_nodes,
                        node, "FCStd extension nodes")?;
                }
            }
            let declared = crate::xml::attribute(ctx, extensions_node, "Count")?
                .map(|value| {
                    ctx.parse_text::<usize>(value, "FCStd extension count parse")
                        .map(|parsed| parsed.ok())
                })
                .transpose()?
                .flatten()
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
            let mut extension_names = HashSet::new();
            let mut extension_types = HashSet::new();
            for (order, node) in ctx
                .admit_iter(&extension_nodes, "FCStd extension nodes")?
                .copied()
                .enumerate()
            {
                let name = retained_attr(ctx, node, "name", "FCStd extension name")?;
                let type_name = retained_attr(ctx, node, "type", "FCStd extension type")?;
                if ctx.contains_hash_set(
                    &extension_names,
                    &name,
                    "FCStd extension name set",
                )? {
                    return Err(crate::resource::malformed_charged(
                        ctx,
                        format_args!("duplicate extension name {name} for {}", object.id()),
                        "FCStd persistence diagnostic",
                    ));
                }
                let name_copy = extension_storage.with_storage(|| {
                    ctx.copy_retained_text(&name, "FCStd extension name copy")
                })?;
                extension_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut extension_names,
                        name_copy,
                        "FCStd extension name set",
                    )
                })?;
                if ctx.contains_hash_set(
                    &extension_types,
                    &type_name,
                    "FCStd extension type set",
                )? {
                    return Err(crate::resource::malformed_charged(
                        ctx,
                        format_args!("duplicate extension type {type_name} for {}", object.id()),
                        "FCStd persistence diagnostic",
                    ));
                }
                let type_copy = extension_storage.with_storage(|| {
                    ctx.copy_retained_text(&type_name, "FCStd extension type copy")
                })?;
                extension_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut extension_types,
                        type_copy,
                        "FCStd extension type set",
                    )
                })?;
                let id = extension_id(ctx, object.id(), &name, order)?;
                extension_storage.with_storage(|| {
                    let id = ctx.copy_retained_text(&id, "FCStd extension identity copy")?;
                    drop(ctx.insert_hash_map(
                        &mut extension_ids_by_start,
                        node.range().start,
                        id,
                        "FCStd extension identity lookup",
                    )?);
                    Ok::<(), CodecError>(())
                })?;
                ctx.push_vec(&mut extensions, ExtensionRecord {
                    id,
                    owner: ctx.copy_retained_text(object.id(), "FCStd extension owner")?,
                    name,
                    type_name,
                    order,
                    raw_xml: {
                        ctx.copy_retained_text(&text[node.range()], "FCStd extension XML")
                    }?,
                }, "FCStd extension records")?;
            }
        }
        if let Some(container) = property_container {
            parse_properties(text, container, object.id(), &mut properties, ctx)?;
        }
        if let Some(extensions_node) = extension_container {
            let (extension_children, _extension_children_storage) = crate::xml::children(ctx,
                extensions_node, "FCStd extension property children")?;
            for &extension in ctx.admit_iter(&extension_children, "FCStd extension property search")? {
                if !crate::xml::has_tag_name(ctx, extension, "Extension")? { continue; }
                let extension_id = ctx
                    .get_hash_map(
                        &extension_ids_by_start,
                        &extension.range().start,
                        "FCStd extension identity lookup",
                    )?
                    .ok_or_else(|| {
                        crate::resource::malformed_charged(
                            ctx,
                            format_args!("extension under {} has no native identity", object.id()),
                            "FCStd persistence diagnostic",
                        )
                    })?;
                let (property_children, _property_children_storage) = crate::xml::children(ctx,
                    extension, "FCStd extension Properties children")?;
                for &container in ctx.admit_iter(&property_children, "FCStd extension Properties search")? {
                    if !crate::xml::has_tag_name(ctx, container, "Properties")? { continue; }
                    parse_properties(text, container, extension_id, &mut properties, ctx)?;
                }
            }
        }
    }
    for property_index in ctx.admit_iter(&(0..properties.len()), "FCStd mutable property visits")? {
        let property = &mut properties[property_index];
        let crate::native::PropertyBody::Persisted { links, .. } = &mut property.body else {
            continue;
        };
        for link_index in ctx.admit_iter(&(0..links.len()), "FCStd mutable link visits")? {
            let Some(link) = links[link_index].as_mut() else { continue; };
            if link.document().is_none() {
                let Some(target) = link.object() else {
                    continue;
                };
                if ctx.contains_key_hash_map(
                    &data_by_name,
                    target,
                    "FCStd object data lookup",
                )? {
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
    let (children, _children_storage) = crate::xml::children(ctx, container, "FCStd property container children")?;
    let mut property_nodes_storage = ctx.reserve_scoped(0, "FCStd property nodes")?;
    let mut property_nodes = Vec::new();
    for &node in ctx.admit_iter(&children, "FCStd persisted property search")? {
        if crate::xml::has_tag_name(ctx, node, "Property")? {
            ctx.push_scoped_vec(&mut property_nodes_storage, &mut property_nodes, node, "FCStd property nodes")?;
        }
    }
    let mut transient_property_nodes_storage = ctx.reserve_scoped(0, "FCStd transient property nodes")?;
    let mut transient_property_nodes = Vec::new();
    for &node in ctx.admit_iter(&children, "FCStd transient property search")? {
        if crate::xml::has_tag_name(ctx, node, "_Property")? {
            ctx.push_scoped_vec(&mut transient_property_nodes_storage, &mut transient_property_nodes, node, "FCStd transient property nodes")?;
        }
    }
    for (index, node) in ctx
        .admit_iter(&transient_property_nodes, "FCStd property name records")?
        .chain(ctx.admit_iter(&property_nodes, "FCStd property name records")?)
        .enumerate()
    {
        let name = crate::xml::attribute(ctx, *node, "name")?.ok_or_else(|| {
            crate::resource::malformed_charged(
                ctx,
                format_args!("{} element has no name attribute", node.tag_name().name()),
                "FCStd persistence diagnostic",
            )
        })?;
        for prior in ctx
            .admit_iter(&transient_property_nodes, "FCStd duplicate property names")?
            .chain(ctx.admit_iter(&property_nodes, "FCStd duplicate property names")?)
            .take(index)
        {
            let prior_name = crate::xml::attribute(ctx, *prior, "name")?;
            if let Some(prior_name) = prior_name {
                if ctx.equal(prior_name, name, "FCStd duplicate property names")? {
                    return Err(crate::resource::malformed_charged(
                        ctx,
                        format_args!("duplicate property name {name} for {owner}"),
                        "FCStd persistence diagnostic",
                    ));
                }
            }
        }
    }
    let declared = crate::xml::attribute(ctx, container, "Count")?
        .map(|value| {
            ctx.parse_text::<usize>(value, "FCStd property count parse")
                .map(|parsed| parsed.ok())
        })
        .transpose()?
        .flatten()
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
        crate::xml::attribute(ctx, container, "TransientCount")?
            .map_or(Ok(0_usize), |value| {
                ctx.parse_text::<usize>(value, "FCStd transient property count parse")?
                    .map_err(|_| {
                        CodecError::Malformed("Properties TransientCount is invalid".into())
                    })
            })?;
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
        ctx.push_vec(output, PropertyRecord {
            id: crate::native::native_child_id_charged(ctx, "property", owner, &name)?,
            owner: ctx.copy_retained_text(owner, "FCStd transient property owner")?,
            name,
            family: property_family(&type_name),
            type_name,
            status: crate::xml::attribute(ctx, node, "status")?
                .map(|value| {
                    ctx.parse_text::<u64>(value, "FCStd property status parse")
                        .map(|parsed| parsed.ok())
                })
                .transpose()?
                .flatten(),
            body: crate::native::PropertyBody::Transient,
            order,
            xml: crate::native::RetainedXml::from_source(
                ctx,
                &text[node.range()],
                cadmpeg_core::decode::u64_from_index(node.range().start),
                "FCStd transient property XML",
            )?,
        }, "FCStd transient property records")?;
    }
    for (order, node) in ctx
        .admit_iter(&property_nodes, "FCStd persisted property nodes")?
        .copied()
        .enumerate()
    {
        let name = retained_attr(ctx, node, "name", "FCStd persisted property name")?;
        let type_name = retained_attr(ctx, node, "type", "FCStd persisted property type")?;
        let mut retained_value_bytes = 0_usize;
        let (value_nodes, _value_nodes_storage) = crate::xml::descendants(ctx, node, "FCStd property value descendants")?;
        let mut values = Vec::new();
        let mut value_order = 0usize;
        for &value in ctx.admit_iter(&value_nodes, "FCStd property value elements")? {
            if !value.is_element() || value == node { continue; }
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
            ctx.push_vec(&mut values, ValueRecord {
                tag: ctx.copy_retained_text(value.tag_name().name(), "FCStd value tag")?,
                order: value_order,
                attributes: {
                    let (attribute_nodes, _attribute_nodes_storage) = ctx.with_scoped_storage(
                        "FCStd value attribute nodes", || ctx.collect_vec(value.attributes(), "FCStd value attribute nodes"))?;
                    let mut attributes = std::collections::BTreeMap::new();
                    for attribute in ctx.admit_iter(&attribute_nodes, "FCStd value attribute records")? {
                        let name = ctx.copy_retained_text(attribute.name(), "FCStd value attribute name")?;
                        let text = ctx.copy_retained_text(attribute.value(), "FCStd value attribute")?;
                        drop(ctx.insert_btree_map(&mut attributes, name, text, "FCStd value attributes")?);
                    }
                    attributes
                },
                text: value
                    .text()
                    .map(|text| ctx.copy_retained_text(text, "FCStd value text"))
                    .transpose()?,
                raw_xml: ctx.copy_retained_text(&text[value.range()], "FCStd value XML")?,
            }, "FCStd property value records")?;
            value_order = value_order.checked_add(1).ok_or_else(||
                ctx.refuse_codec_limit("FCStd property value order", u64::MAX, u64::MAX))?;
        }
        let links = if link_grammar(&type_name).is_some() {
            parse_link_targets(node, &type_name, ctx)?
        } else {
            Vec::new()
        };
        let mut side_entries = Vec::new();
        for value in ctx.admit_iter(&values, "FCStd property value references")? {
            for (name, entry_name) in
                ctx.admit_iter(&value.attributes, "FCStd value attribute references")?
            {
                let selected = (matches!(name.as_str(), "file" | "File")
                    && !is_xlink_type(&type_name))
                    || (property_family(&type_name) == PropertyFamily::File
                        && matches!(name.as_str(), "name" | "Name"));
                if selected && !entry_name.is_empty() {
                    let entry_name =
                        ctx.copy_retained_text(entry_name, "FCStd side entry name")?;
                    ctx.push_vec(
                        &mut side_entries,
                        entry_name,
                        "FCStd side entry references",
                    )?;
                }
            }
        }
        ctx.push_vec(output, PropertyRecord {
            id: crate::native::native_child_id_charged(ctx, "property", owner, &name)?,
            owner: ctx.copy_retained_text(owner, "FCStd persisted property owner")?,
            name,
            family: property_family(&type_name),
            type_name,
            status: crate::xml::attribute(ctx, node, "status")?
                .map(|value| {
                    ctx.parse_text::<u64>(value, "FCStd property status parse")
                        .map(|parsed| parsed.ok())
                })
                .transpose()?
                .flatten(),
            body: crate::native::PropertyBody::Persisted {
                values,
                links,
                side_entries,
                dynamic: crate::xml::attribute(ctx, node, "group")?
                    .map(|group| -> Result<DynamicPropertyMeta, CodecError> {
                        Ok(DynamicPropertyMeta {
                            group: {
                                ctx.copy_retained_text(group, "FCStd dynamic property group")
                            }?,
                            documentation: crate::xml::attribute(ctx, node, "doc")?
                                .map(|doc| {
                                    ctx.copy_retained_text(
                                        doc,
                                        "FCStd dynamic property documentation",
                                    )
                                })
                                .transpose()?,
                            attributes: crate::xml::attribute(ctx, node, "attr")?
                                .map(|value| {
                                    ctx.parse_text::<i64>(
                                        value,
                                        "FCStd dynamic property attributes parse",
                                    )
                                    .map(|parsed| parsed.ok())
                                })
                                .transpose()?
                                .flatten(),
                            read_only: bool_attr(crate::xml::attribute(ctx, node, "ro")?),
                            hidden: bool_attr(crate::xml::attribute(ctx, node, "hide")?),
                        })
                    })
                    .transpose()?,
            },
            order,
            xml: crate::native::RetainedXml::from_source(
                ctx,
                &text[node.range()],
                cadmpeg_core::decode::u64_from_index(node.range().start),
                "FCStd persisted property XML",
            )?,
        }, "FCStd persisted property records")?;
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
        LinkGrammar::XLink => ctx.collect_indexed_vec(
            1, "FCStd link target records", |_| xlink(root, ctx)),
        LinkGrammar::XLinkSubList => {
            let (children, _children_storage) = counted_children(root, "XLink", type_name, ctx)?;
            ctx.collect_indexed_vec(children.len(), "FCStd link target or subelement records",
                |index| xlink(children[index], ctx))
        }
    }
}

fn reject_nested_link_value(node: roxmltree::Node<'_, '_>, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let (children, _children_storage) = crate::xml::children(ctx, node, "FCStd nested link children")?;
    if ctx.any_by(&children, |child| Ok(child.is_element()), "FCStd nested link search")? {
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
    ctx: &DecodeContext<'_>,
) -> Result<roxmltree::Node<'a, 'input>, CodecError> {
    let (elements, _elements_storage) = crate::xml::children(ctx, property, "FCStd link value children")?;
    let first = ctx.position_by(&elements, |value| Ok(value.is_element()), "FCStd link value search")?.ok_or_else(|| {
        crate::resource::malformed_charged(
            ctx,
            format_args!("{type_name} requires one {tag} value"),
            "FCStd persistence diagnostic",
        )
    })?;
    let value = elements[first];
    if !crate::xml::has_tag_name(ctx, value, tag)?
        || ctx.any_by(&elements[first + 1..], |value| Ok(value.is_element()), "FCStd duplicate link value search")? {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!("{type_name} requires exactly one {tag} value"),
            "FCStd persistence diagnostic",
        ));
    }
    Ok(value)
}

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
    let count_text = crate::xml::attribute(ctx, parent, "count")?.ok_or_else(|| {
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
    let mut actual_count = 0_usize;
    let mut valid_tags = true;
    let mut next = parent.first_child();
    while let Some(child) = next {
        ctx.charge_work(1, "FCStd link child framing")?;
        next = child.next_sibling();
        if !child.is_element() {
            continue;
        }
        actual_count = actual_count.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("FCStd link child framing", u64::MAX, u64::MAX)
        })?;
        valid_tags &= crate::xml::has_tag_name(ctx, child, tag)?;
    }
    if actual_count != count || !valid_tags {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!("{type_name} count={count} but {actual_count} {tag} values were found"),
            "FCStd persistence diagnostic",
        ));
    }
    let (children, _children_storage) = crate::xml::children(ctx, parent, "FCStd link child nodes")?;
    let mut storage = ctx.reserve_scoped(0, "FCStd link nodes")?;
    let mut child_nodes = Vec::new();
    for &child in ctx.admit_iter(&children, "FCStd link node copy")? {
        if child.is_element() {
            ctx.push_scoped_vec(&mut storage, &mut child_nodes, child, "FCStd link nodes")?;
        }
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
    let file = crate::xml::attribute(ctx, node, "file")?
        .map(|value| ctx.copy_retained_text(value, "FCStd link document"))
        .transpose()?;
    let (children, _children_storage) = crate::xml::children(ctx, node, "FCStd XLink children")?;
    let has_children = ctx.any_by(&children, |child| Ok(child.is_element()), "FCStd XLink element children")?;
    let subelements = match (crate::xml::attribute(ctx, node, "sub")?, crate::xml::attribute(ctx, node, "count")?) {
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
        (None, Some(_)) => {
            let count_is_zero = crate::xml::attribute(ctx, node, "count")?
                .map(|count| {
                    ctx.parse_text::<usize>(count, "FCStd XLink count parse")
                        .map(|parsed| parsed.ok() == Some(0))
                })
                .transpose()?
                .unwrap_or(false);
            if count_is_zero {
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
                let (nested, _nested_storage) = crate::xml::children(ctx, child, "FCStd XLink Sub children")?;
                if ctx.any_by(&nested, |value| Ok(value.is_element()), "FCStd XLink Sub nested search")? {
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
    let primary = crate::xml::attribute(ctx, node, primary_attribute)?.ok_or_else(|| {
        crate::resource::malformed_charged(
            ctx,
            format_args!(
                "{} element has no {primary_attribute} attribute",
                node.tag_name().name()
            ),
            "FCStd persistence diagnostic",
        )
    })?;
    ctx.copy_retained_text(
        crate::xml::attribute(ctx, node, "shadowed")?.unwrap_or(primary),
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
    let (attributes, _attributes_storage) = ctx.with_scoped_storage(
        "FCStd link carrier attributes", || ctx.collect_vec(node.attributes(), "FCStd link carrier attributes"))?;
    let unsupported = ctx.find_by(&attributes, |attribute| {
        if ctx.contains(CARRIERS, &attribute.name(), "FCStd link carrier names")? {
            Ok(!ctx.contains(allowed, &attribute.name(), "FCStd link carrier names")?)
        } else {
            Ok(false)
        }
    }, "FCStd link carrier search")?;
    if let Some(attribute) = unsupported {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "{} has unsupported link carrier {}",
                node.tag_name().name(),
                attribute.name()
            ),
            "FCStd persistence diagnostic",
        ));
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
    let mut next = root.first_child();
    while let Some(node) = next {
        ctx.charge_work(1, "FCStd document section search")?;
        next = node.next_sibling();
        if !crate::xml::has_tag_name(ctx, node, tag)? { continue; }
        if selected.is_some() {
            return Err(crate::resource::malformed_charged(ctx,
                format_args!("Document.xml has duplicate {tag} sections"), "FCStd persistence diagnostic"));
        }
        selected = Some(node);
    }
    selected.ok_or_else(|| crate::resource::malformed_charged(ctx,
        format_args!("Document.xml has no {tag} section"), "FCStd persistence diagnostic"))
}

fn extension_id(
    ctx: &DecodeContext<'_>,
    owner: &str,
    name: &str,
    order: usize,
) -> Result<String, CodecError> {
    let (order, _order_storage) = ctx.format_scoped(format_args!("{order}"), "FCStd extension order text")?;
    let (child, _child_storage) = ctx.with_scoped_storage("FCStd extension identity key", ||
        ctx.join_retained(&[order.as_str(), name], ":", "FCStd extension identity key"))?;
    crate::native::native_child_id_charged(ctx, "extension", owner, &child)
}

fn retained_attr(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    name: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let value = crate::xml::attribute(ctx, node, name)?.ok_or_else(|| {
        crate::resource::malformed_charged(
            ctx,
            format_args!("{} element has no {name} attribute", node.tag_name().name()),
            "FCStd persistence diagnostic",
        )
    })?;
    ctx.copy_retained_text(value, operation)
}

fn bool_attr(value: Option<&str>) -> Option<bool> {
    value.map(|value| matches!(value, "1" | "true" | "True" | "TRUE"))
}

#[cfg(test)]
pub(crate) mod tests;
