// SPDX-License-Identifier: Apache-2.0
//! Structured neutral values use the registered FreeCAD map and link grammars.

use super::*;

pub(super) fn write(
    name: &str,
    values: &[AttributeValue],
    names: &HashMap<String, String>,
) -> Option<String> {
    if let [AttributeValue::Map(entries)] = values {
        let content = entries
            .iter()
            .map(|entry| {
                xml::element(
                    "Item",
                    &[("key", entry.key.clone()), ("value", entry.value.clone())],
                )
            })
            .collect::<String>();
        return Some(xml::property(
            name,
            "App::PropertyMap",
            &format!("<Map count=\"{}\">{content}</Map>", entries.len()),
        ));
    }
    if let [AttributeValue::SubReference {
        object,
        subelements,
    }] = values
    {
        let target = names.get(object)?;
        let children = subelements
            .iter()
            .map(|value| xml::element("Sub", &[("value", value.clone())]))
            .collect::<String>();
        return Some(xml::property(
            name,
            "App::PropertyLinkSub",
            &format!(
                "<LinkSub value=\"{}\" count=\"{}\">{children}</LinkSub>",
                xml::escaped(target),
                subelements.len()
            ),
        ));
    }
    if let [AttributeValue::ExternalReference { .. }] = values {
        return Some(xml::property(
            name,
            "App::PropertyXLink",
            &xlink(&values[0], names)?,
        ));
    }
    if values.len() > 1
        && values.iter().all(|value| {
            matches!(
                value,
                AttributeValue::Reference(_)
                    | AttributeValue::SubReference { .. }
                    | AttributeValue::ExternalReference { .. }
            )
        })
    {
        let children = values
            .iter()
            .map(|value| xlink(value, names))
            .collect::<Option<String>>()?;
        return Some(xml::property(
            name,
            "App::PropertyXLinkSubList",
            &format!(
                "<XLinkSubList count=\"{}\">{children}</XLinkSubList>",
                values.len()
            ),
        ));
    }
    None
}

pub(in crate::writer) fn xlink(
    value: &AttributeValue,
    names: &HashMap<String, String>,
) -> Option<String> {
    let (object, document, subelements) = match value {
        AttributeValue::Reference(object) => (names.get(object)?, None, &[][..]),
        AttributeValue::SubReference {
            object,
            subelements,
        } => (names.get(object)?, None, subelements.as_slice()),
        AttributeValue::ExternalReference {
            document,
            object,
            subelements,
        } => (object, Some(document), subelements.as_slice()),
        _ => return None,
    };
    let mut attributes = vec![("name", object.clone())];
    if let Some(document) = document {
        attributes.push(("file", document.clone()));
    }
    if subelements.is_empty() {
        return Some(xml::element("XLink", &attributes));
    }
    attributes.push(("count", subelements.len().to_string()));
    let opening = xml::element("XLink", &attributes);
    let children = subelements
        .iter()
        .map(|value| xml::element("Sub", &[("value", value.clone())]))
        .collect::<String>();
    Some(format!(
        "{}{children}</XLink>",
        opening.strip_suffix("/>")?.to_owned() + ">"
    ))
}

/// Rebuild only the value lane; the current native property owns its envelope
/// and any same-position fields outside the neutral value grammar.
pub(super) fn through_schema(
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
    original: &crate::native::PropertyRecord,
    names: &HashMap<String, String>,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    let generated = preferred(
        &attribute.name,
        &attribute.values,
        &original.type_name,
        names,
    )
    .or_else(|| write(&attribute.name, &attribute.values, names));
    let Some(generated) = generated else {
        return Ok(None);
    };
    encode_generated(attribute, original, names, &generated)
}

pub(in crate::writer) fn encode_generated(
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
    original: &crate::native::PropertyRecord,
    names: &HashMap<String, String>,
    generated: &str,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    let document =
        roxmltree::Document::parse(generated).map_err(cadmpeg_core::CodecError::malformed)?;
    let root = document.root_element();
    let mut property = original.clone();
    property.name.clone_from(&attribute.name);
    property.type_name = root
        .attribute("type")
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("generated attribute has no runtime"))?
        .to_owned();
    let mut indices = HashMap::new();
    let mut values = Vec::new();
    for node in root
        .descendants()
        .filter(roxmltree::Node::is_element)
        .skip(1)
    {
        let order = values.len();
        let parent = node
            .parent()
            .and_then(|parent| indices.get(&parent.id()).copied());
        let tag = node.tag_name().name();
        let previous = original
            .values()
            .get(order)
            .filter(|value| value.tag == tag && value.parent == parent);
        let mut attributes = previous
            .map(|value| value.attributes.clone())
            .unwrap_or_default();
        let owned: &[&str] = match tag {
            "Map" | "LinkList" | "LinkSubList" | "XLinkSubList" | "ExpressionEngine" => &["count"],
            "Item" => &["key", "value"],
            "Link" => {
                if property.type_name == "App::PropertyLinkSubList" {
                    &["obj", "sub"]
                } else {
                    &["value"]
                }
            }
            "LinkSub" => &["value", "count"],
            "XLink" => &["name", "file", "sub", "count"],
            "Sub" => &["value"],
            "Expression" => &["path", "expression", "comment"],
            "XLinks" => &["count", "docs", "hidden"],
            "DocMap" => &["index", "name", "label"],
            _ => &[],
        };
        for field in owned {
            attributes.remove(*field);
        }
        attributes.extend(
            node.attributes()
                .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned())),
        );
        values.push(crate::native::ValueRecord {
            order,
            parent,
            tag: tag.to_owned(),
            attributes,
            decoded_json: None,
            text: None,
            link_index: None,
            item_index: None,
        });
        indices.insert(node.id(), order);
    }
    let links = attribute
        .values
        .iter()
        .filter_map(|value| {
            let (object, document, subelements) = match value {
                AttributeValue::Reference(object) => (object.clone(), None, Vec::new()),
                AttributeValue::SubReference {
                    object,
                    subelements,
                } => (object.clone(), None, subelements.clone()),
                AttributeValue::ExternalReference {
                    object,
                    document,
                    subelements,
                } => (object.clone(), Some(document.clone()), subelements.clone()),
                _ => return None,
            };
            Some(
                crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
                    object: Some(object),
                    document_attribute: document.as_ref().map(|_| "file".to_owned()),
                    document,
                    subelements,
                })
                .map_err(cadmpeg_core::CodecError::malformed),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    property.body = Some(crate::native::PropertyBody::Persisted {
        values: values.into(),
        links,
        side_entries: Vec::new(),
    });
    super::encode(&property, names)
}

fn preferred(
    name: &str,
    values: &[AttributeValue],
    kind: &str,
    names: &HashMap<String, String>,
) -> Option<String> {
    match crate::persistence::link_grammar(kind) {
        Some(crate::persistence::LinkGrammar::LinkList) if values.iter().all(|value| matches!(value, AttributeValue::Reference(_))) => {
            let children = values.iter().map(|value| match value { AttributeValue::Reference(id) => Some(xml::element("Link", &[("value", names.get(id)?.clone())])), _ => None }).collect::<Option<String>>()?;
            Some(xml::property(name, kind, &format!("<LinkList count=\"{}\">{children}</LinkList>", values.len())))
        }
        Some(crate::persistence::LinkGrammar::LinkSubList) if values.iter().all(|value| matches!(value, AttributeValue::SubReference { subelements, .. } if subelements.len() == 1)) => {
            let children = values.iter().map(|value| match value { AttributeValue::SubReference { object, subelements } => Some(xml::element("Link", &[("obj", names.get(object)?.clone()), ("sub", subelements[0].clone())])), _ => None }).collect::<Option<String>>()?;
            Some(xml::property(name, kind, &format!("<LinkSubList count=\"{}\">{children}</LinkSubList>", values.len())))
        }
        Some(crate::persistence::LinkGrammar::XLinkSubList) => {
            let children = values.iter().map(|value| xlink(value, names)).collect::<Option<String>>()?;
            Some(xml::property(name, kind, &format!("<XLinkSubList count=\"{}\">{children}</XLinkSubList>", values.len())))
        }
        _ => None,
    }
}
