// SPDX-License-Identifier: Apache-2.0
//! Document-owned values are born in their actual XML property scopes.

use crate::native::PropertyRecord;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use std::collections::BTreeMap;

mod value_parts;

#[derive(Default)]
pub(crate) struct RootProperties {
    pub(crate) properties: Vec<PropertyRecord>,
    pub(crate) sources: BTreeMap<String, crate::fidelity::ValueSourceRanges>,
    pub(crate) given_up: bool,
}

pub(crate) fn selected(node: roxmltree::Node<'_, '_>) -> bool {
    let tag = match node.attribute("type") {
        Some("App::PropertyString") => "String",
        Some(
            "App::PropertyInteger" | "App::PropertyIntegerConstraint" | "App::PropertyPercent",
        ) => "Integer",
        Some("App::PropertyBool") => "Bool",
        Some(
            "App::PropertyFloat"
            | "App::PropertyFloatConstraint"
            | "App::PropertyLength"
            | "App::PropertyAngle"
            | "App::PropertyDistance"
            | "App::PropertyQuantity",
        ) => "Float",
        Some("App::PropertyVector") => "PropertyVector",
        _ => return false,
    };
    if !node.has_tag_name("Property")
        || node
            .descendants()
            .filter(roxmltree::Node::is_element)
            .any(|value| {
                value.attributes().any(|attribute| {
                    matches!(attribute.name(), "file" | "File") && !attribute.value().is_empty()
                })
            })
    {
        return false;
    }
    if node.attribute("group").is_some()
        && ["ro", "hide"].into_iter().any(|key| {
            node.attributes()
                .find(|attribute| attribute.namespace().is_none() && attribute.name() == key)
                .is_some_and(|attribute| crate::native::parse_bool(attribute.value()).is_none())
        })
    {
        return false;
    }
    let mut elements = node.children().filter(roxmltree::Node::is_element);
    let Some(value) = elements.next() else {
        return false;
    };
    let string = tag == "String";
    if !value.has_tag_name(tag)
        || elements.next().is_some()
        || value.children().any(|child| child.is_element())
    {
        return false;
    }
    if !string && value.first_child().is_some() {
        return false;
    }
    if tag == "PropertyVector"
        && super::direct_vector_value(node, "App::PropertyVector") != Some(value)
    {
        return false;
    }
    if let Some(text) = value.first_child().filter(roxmltree::Node::is_text) {
        let raw = &node.document().input_text().as_bytes()[text.range()];
        let suffix = &node.document().input_text().as_bytes()[text.range().end..value.range().end];
        if value.last_child() != Some(text) || raw.contains(&b'<') || !suffix.starts_with(b"</") {
            return false;
        }
    }
    true
}

pub(crate) fn decode<'source>(
    ctx: &DecodeContext<'source>,
    view: View<'source>,
    xml: &roxmltree::Document<'_>,
    output: &mut PartOutput,
) -> Result<Option<RootProperties>, CodecError> {
    if ctx.container_only() {
        return Ok(None);
    }
    let mut containers = xml
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Properties"));
    let Some(container) = containers.next() else {
        return Ok(None);
    };
    if containers.next().is_some() {
        return Err(CodecError::malformed(
            "Document.xml has multiple root Properties containers",
        ));
    }
    if !container.children().any(selected) {
        return Ok(None);
    }
    let source = super::source::Source::new(ctx, view, xml)?;
    let (extent, fields) = crate::xml_framing::element_fields(ctx, view, container)?;
    let properties_level = PartLevel::parse("freecad.properties")
        .ok_or_else(|| CodecError::malformed("properties level is absent from Appendix C"))?;
    let result = ctx.decode_part(
        output,
        extent,
        PartSpec::new(properties_level, fields),
        |properties_stage| {
            let count = container
                .children()
                .filter(|node| node.has_tag_name("Property") || node.has_tag_name("_Property"))
                .count();
            let bytes = count
                .checked_mul(size_of::<Option<&str>>())
                .ok_or_else(|| {
                    CodecError::malformed("document property name inventory overflows")
                })?;
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(bytes),
                "FreeCAD document property names",
            )?;
            let mut names = ctx.alloc_filled(count, None, "FreeCAD document property names")?;
            for (name, property) in names.iter_mut().zip(
                container
                    .children()
                    .filter(|node| node.has_tag_name("Property") || node.has_tag_name("_Property")),
            ) {
                *name = property.attribute("name");
                if name.is_none() && !selected(property) {
                    return Err(CodecError::malformed(
                        "property element has no name attribute",
                    ));
                }
            }
            names.sort_unstable();
            if names
                .windows(2)
                .any(|pair| pair[0].is_some() && pair[0] == pair[1])
            {
                return Err(CodecError::malformed("duplicate document property name"));
            }
            let mut result = RootProperties::default();
            for (order, node) in container
                .children()
                .filter(|node| node.has_tag_name("Property"))
                .enumerate()
            {
                if !selected(node) {
                    continue;
                }
                let (extent, fields) = crate::xml_framing::element_fields(ctx, view, node)?;
                let level = PartLevel::parse("freecad.property").ok_or_else(|| {
                    CodecError::malformed("property level is absent from Appendix C")
                })?;
                if let Some(property) = ctx.decode_part(
                    properties_stage,
                    extent,
                    PartSpec::new(level, fields),
                    |stage| {
                        let owner = crate::native::native_id("document", "0");
                        let name = super::required_attr(node, "name")?;
                        let id = crate::native::native_child_id("property", &owner, name);
                        source.record(&id, node, || {
                            value_parts::decode(ctx, view, &source, node, &owner, order, stage)
                        })
                    },
                )? {
                    result.sources.insert(
                        property.id.clone(),
                        crate::fidelity::property_source(node, &property)?,
                    );
                    result.properties.push(property);
                }
            }
            Ok(result)
        },
    )?;
    Ok(Some(result.unwrap_or(RootProperties {
        given_up: true,
        ..RootProperties::default()
    })))
}
