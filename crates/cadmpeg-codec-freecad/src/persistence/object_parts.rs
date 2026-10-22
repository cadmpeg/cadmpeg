// SPDX-License-Identifier: Apache-2.0
//! Source-owned dependency-free application declarations and selected properties.

use super::{document_properties, ObjectRecord, Vocabulary};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct ObjectDeclarations {
    pub(crate) objects: Vec<ObjectRecord>,
    pub(crate) properties: BTreeMap<String, document_properties::RootProperties>,
    pub(crate) data_sources: BTreeMap<String, std::ops::Range<usize>>,
}

fn supported(xml: &roxmltree::Document<'_>, schema: Vocabulary) -> bool {
    let (declarations, data, record) = schema.tags();
    let root = xml.root_element();
    let (Ok(declarations), Ok(data)) = (
        super::unique_section(root, declarations),
        super::unique_section(root, data),
    ) else {
        return false;
    };
    let objects = || {
        declarations
            .children()
            .filter(|node| node.has_tag_name(record))
    };
    let data_nodes = || data.children().filter(|node| node.has_tag_name(record));
    let mut names = std::collections::BTreeSet::new();
    let mut data_by_name = BTreeMap::new();
    for node in data_nodes() {
        let Some(name) = node.attribute("name") else {
            return false;
        };
        if data_by_name.insert(name, node).is_some() {
            return false;
        }
    }
    objects().count() > 0
        && objects().count() <= super::MAX_OBJECTS
        && declarations.attribute("Dependencies").is_none()
        && declarations
            .children()
            .filter(roxmltree::Node::is_element)
            .all(|node| node.has_tag_name(record))
        && data
            .children()
            .filter(roxmltree::Node::is_element)
            .all(|node| node.has_tag_name(record))
        && objects().count() == data_nodes().count()
        && objects().all(|object| {
            let Some(name) = object.attribute("name") else {
                return false;
            };
            names.insert(name)
                && data_by_name.contains_key(name)
                && !object.children().any(|node| node.is_element())
                && object.attributes().all(|attribute| {
                    attribute.namespace().is_none() && matches!(attribute.name(), "name" | "type")
                })
        })
        && data_nodes().all(|node| {
            let mut children = node.children().filter(roxmltree::Node::is_element);
            let Some(properties) = children.next() else {
                return false;
            };
            children.next().is_none()
                && properties.has_tag_name("Properties")
                && node
                    .attributes()
                    .all(|attribute| attribute.namespace().is_none() && attribute.name() == "name")
                && properties
                    .children()
                    .filter(roxmltree::Node::is_element)
                    .all(document_properties::selected)
        })
}

pub(crate) fn decode<'source>(
    ctx: &DecodeContext<'source>,
    view: View<'source>,
    xml: &roxmltree::Document<'_>,
    schema: Vocabulary,
    output: &mut PartOutput,
) -> Result<Option<ObjectDeclarations>, CodecError> {
    if ctx.container_only() || !supported(xml, schema) {
        return Ok(None);
    }
    let (objects_tag, data_tag, record_tag) = schema.tags();
    let declarations = super::unique_section(xml.root_element(), objects_tag)?;
    let data = super::unique_section(xml.root_element(), data_tag)?;
    let data_by_name = data
        .children()
        .filter(|node| node.has_tag_name(record_tag))
        .map(|node| Ok((super::required_attr(node, "name")?, node)))
        .collect::<Result<BTreeMap<_, _>, CodecError>>()?;
    let source = super::source::Source::new(ctx, view, xml)?;
    let (extent, fields) = crate::xml_framing::element_fields(ctx, view, declarations)?;
    let objects = ctx
        .decode_part(output, extent, spec("freecad.objects", fields)?, |stage| {
            let mut objects = Vec::new();
            for (order, node) in declarations
                .children()
                .filter(|node| node.has_tag_name(record_tag))
                .enumerate()
            {
                let name = super::required_attr(node, "name")?;
                let owner = data_by_name.get(name).copied().ok_or_else(|| {
                    CodecError::malformed("object declaration has no matched data")
                })?;
                let (extent, fields) = crate::xml_framing::element_fields(ctx, view, node)?;
                if let Some(object) = ctx.decode_part(
                    stage,
                    extent,
                    spec("freecad.object_declaration", fields)?,
                    |stage| {
                        let id = crate::native::native_id("object", name);
                        source.record(&id, node, || {
                            let object = super::parse_object(
                                node,
                                Some(owner),
                                order,
                                None,
                                false,
                                Some(&source),
                            )?;
                            stage.insert_native(
                                ctx,
                                "objects",
                                crate::native::object_declaration::ObjectDeclaration::new(
                                    object.clone(),
                                )?,
                            )?;
                            Ok(object)
                        })
                    },
                )? {
                    objects.try_reserve(1).map_err(|_| {
                        ResourceLimit::allocation_failed(
                            ResourceDimension::RetainedBytes,
                            ctx.policy().limits.max_retained_bytes,
                            u64_from_index(size_of::<ObjectRecord>()),
                            "FreeCAD object header handoff",
                        )
                    })?;
                    objects.push(object);
                }
            }
            Ok(objects)
        })?
        .unwrap_or_default();
    let mut result = ObjectDeclarations {
        objects,
        ..ObjectDeclarations::default()
    };
    let by_name = result
        .objects
        .iter()
        .map(|object| (object.name().as_str(), object.id().as_str()))
        .collect::<BTreeMap<_, _>>();
    let (extent, fields) = crate::xml_framing::element_fields(ctx, view, data)?;
    result.data_sources = ctx
        .decode_part(
            output,
            extent,
            spec("freecad.object_data_section", fields)?,
            |stage| {
                let mut sources = BTreeMap::new();
                for node in data.children().filter(|node| node.has_tag_name(record_tag)) {
                    let (extent, fields) = crate::xml_framing::element_fields(ctx, view, node)?;
                    if let Some((id, range)) = ctx.decode_part(
                        stage,
                        extent,
                        spec("freecad.object_data", fields)?,
                        |_stage| {
                            let name = super::required_attr(node, "name")?;
                            let id = by_name.get(name).ok_or_else(|| {
                                CodecError::malformed(
                                    "ObjectData has no readable object declaration",
                                )
                            })?;
                            Ok(((*id).to_owned(), node.range()))
                        },
                    )? {
                        sources.insert(id, range);
                    }
                }
                Ok(sources)
            },
        )?
        .unwrap_or_default();
    for object in &result.objects {
        let node = data_by_name
            .get(object.name().as_str())
            .copied()
            .ok_or_else(|| CodecError::malformed("object property owner has no matched data"))?;
        let container = node
            .children()
            .find(|node| node.has_tag_name("Properties"))
            .ok_or_else(|| CodecError::malformed("object has no direct Properties"))?;
        let properties =
            document_properties::decode_owned(ctx, view, xml, container, object.id(), output)?;
        result.properties.insert(object.id().clone(), properties);
    }
    Ok(Some(result))
}

fn spec(level: &str, fields: Vec<std::ops::Range<u64>>) -> Result<PartSpec, CodecError> {
    Ok(PartSpec::new(
        PartLevel::parse(level)
            .ok_or_else(|| CodecError::malformed("object part level is absent from Appendix C"))?,
        fields,
    ))
}
