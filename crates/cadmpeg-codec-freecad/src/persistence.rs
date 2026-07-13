// SPDX-License-Identifier: Apache-2.0
//! Generic schema-4 object and property graph recovery.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::codec::CodecError;

use crate::native::{
    DynamicPropertyMeta, ExtensionRecord, LinkTarget, ObjectRecord, PropertyFamily, PropertyRecord,
    ValueRecord,
};

/// Recovered persistence graph.
pub struct Graph {
    /// Declared objects.
    pub objects: Vec<ObjectRecord>,
    /// Dynamic extensions.
    pub extensions: Vec<ExtensionRecord>,
    /// Document and object properties.
    pub properties: Vec<PropertyRecord>,
}

/// Recover the schema-4 persistence graph without interpreting geometry.
pub fn parse(bytes: &[u8]) -> Result<Graph, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::Malformed(format!("invalid Document.xml: {error}")))?;
    let root = xml.root_element();
    let objects_node = root
        .children()
        .find(|node| node.has_tag_name("Objects"))
        .ok_or_else(|| CodecError::Malformed("Document.xml has no Objects section".into()))?;
    let data_node = root
        .children()
        .find(|node| node.has_tag_name("ObjectData"))
        .ok_or_else(|| CodecError::Malformed("Document.xml has no ObjectData section".into()))?;

    let mut dependency_map = HashMap::<String, Vec<String>>::new();
    for node in objects_node
        .children()
        .filter(|node| node.has_tag_name("ObjectDeps"))
    {
        let name = required_attr(node, "Name")?;
        let dependencies = node
            .children()
            .filter(|child| child.has_tag_name("Dep"))
            .map(|child| required_attr(child, "Name"))
            .collect::<Result<Vec<_>, _>>()?;
        dependency_map.insert(name, dependencies);
    }

    let data_by_name = data_node
        .children()
        .filter(|node| node.has_tag_name("Object"))
        .filter_map(|node| node.attribute("name").map(|name| (name.to_owned(), node)))
        .collect::<HashMap<_, _>>();

    let mut objects = Vec::new();
    for (order, node) in objects_node
        .children()
        .filter(|node| node.has_tag_name("Object"))
        .enumerate()
    {
        let name = required_attr(node, "name")?;
        let type_name = required_attr(node, "type")?;
        let id = object_id(&name);
        let raw_xml = data_by_name
            .get(&name)
            .map(|data| text[data.range()].to_owned());
        let attributes = node
            .attributes()
            .filter(|attribute| !matches!(attribute.name(), "name" | "type" | "id" | "ViewType"))
            .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
            .collect();
        objects.push(ObjectRecord {
            id,
            name: name.clone(),
            type_name,
            persistent_id: node.attribute("id").and_then(|value| value.parse().ok()),
            view_type: node.attribute("ViewType").map(str::to_owned),
            attributes,
            dependencies: dependency_map.remove(&name).unwrap_or_default(),
            order,
            raw_xml,
        });
    }

    let declared_count = objects_node
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::Malformed("Objects Count is missing or invalid".into()))?;
    if declared_count != objects.len() {
        return Err(CodecError::Malformed(format!(
            "Objects Count={declared_count} but {} declarations were found",
            objects.len()
        )));
    }
    if data_by_name.len() != objects.len() {
        return Err(CodecError::Malformed(
            "object declarations and ObjectData identities disagree".into(),
        ));
    }
    let declared_names = objects
        .iter()
        .map(|object| object.name.clone())
        .collect::<std::collections::HashSet<_>>();
    for object in &mut objects {
        for dependency in &mut object.dependencies {
            if !declared_names.contains(dependency) {
                return Err(CodecError::Malformed(format!(
                    "object {} depends on missing object {dependency}",
                    object.name
                )));
            }
            *dependency = object_id(dependency);
        }
    }

    let mut properties = Vec::new();
    let mut extensions = Vec::new();
    if let Some(document_properties) = root.children().find(|node| node.has_tag_name("Properties"))
    {
        parse_properties(
            text,
            document_properties,
            "fcstd:document#0",
            &mut properties,
        )?;
    }
    for object in &objects {
        let data = data_by_name.get(&object.name).ok_or_else(|| {
            CodecError::Malformed(format!("missing ObjectData for {}", object.name))
        })?;
        for extensions_node in data
            .descendants()
            .filter(|node| node.has_tag_name("Extensions"))
        {
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
                return Err(CodecError::Malformed(format!(
                    "Extensions Count={declared} but {} records were found for {}",
                    nodes.len(),
                    object.id
                )));
            }
            for (order, node) in nodes.into_iter().enumerate() {
                let name = required_attr(node, "name")?;
                extensions.push(ExtensionRecord {
                    id: extension_id(&object.id, &name, order),
                    owner: object.id.clone(),
                    name,
                    type_name: required_attr(node, "type")?,
                    order,
                    raw_xml: text[node.range()].to_owned(),
                });
            }
        }
        for container in data
            .descendants()
            .filter(|node| node.has_tag_name("Properties"))
        {
            let property_owner = container
                .ancestors()
                .find(|ancestor| ancestor.has_tag_name("Extension"))
                .and_then(|extension| {
                    let name = extension.attribute("name")?;
                    extensions
                        .iter()
                        .find(|record| record.owner == object.id && record.name == name)
                        .map(|record| record.id.clone())
                })
                .unwrap_or_else(|| object.id.clone());
            parse_properties(text, container, &property_owner, &mut properties)?;
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
) -> Result<(), CodecError> {
    let nodes = container
        .children()
        .filter(|node| node.has_tag_name("Property"))
        .collect::<Vec<_>>();
    let transient_nodes = container
        .children()
        .filter(|node| node.has_tag_name("_Property"))
        .collect::<Vec<_>>();
    let declared = container
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::Malformed("Properties Count is missing or invalid".into()))?;
    if declared != nodes.len() {
        return Err(CodecError::Malformed(format!(
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
        return Err(CodecError::Malformed(format!(
            "Properties TransientCount={declared_transient} but {} transient properties were found for {owner}",
            transient_nodes.len()
        )));
    }
    for (order, node) in transient_nodes.into_iter().enumerate() {
        let name = required_attr(node, "name")?;
        let type_name = required_attr(node, "type")?;
        output.push(PropertyRecord {
            id: format!("{owner}:property:{name}"),
            owner: owner.to_owned(),
            name,
            family: classify_property(&type_name),
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
        let values = node
            .descendants()
            .filter(|value| value.is_element() && *value != node)
            .enumerate()
            .map(|(value_order, value)| ValueRecord {
                tag: value.tag_name().name().to_owned(),
                order: value_order,
                attributes: value
                    .attributes()
                    .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                    .collect(),
                text: value.text().map(str::to_owned),
                raw_xml: text[value.range()].to_owned(),
            })
            .collect::<Vec<_>>();
        let links = if type_name.contains("PropertyLink") {
            values.iter().flat_map(link_targets).collect()
        } else {
            Vec::new()
        };
        let side_entries = if type_name.contains("PropertyFile") {
            values
                .iter()
                .flat_map(|value| {
                    value
                        .attributes
                        .iter()
                        .filter(|(name, _)| {
                            matches!(name.as_str(), "file" | "File" | "name" | "Name")
                        })
                        .map(|(_, value)| value.clone())
                })
                .collect()
        } else {
            Vec::new()
        };
        output.push(PropertyRecord {
            id: format!("{owner}:property:{name}"),
            owner: owner.to_owned(),
            name,
            family: classify_property(&type_name),
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

fn classify_property(type_name: &str) -> PropertyFamily {
    if type_name.contains("PropertyPythonObject") {
        PropertyFamily::PythonObject
    } else if type_name.contains("PropertyExpression") {
        PropertyFamily::Expression
    } else if type_name.contains("PropertyLink") {
        PropertyFamily::Link
    } else if type_name.contains("PropertyFile") {
        PropertyFamily::File
    } else if type_name.contains("PropertyPlacement") || type_name.contains("PropertyTransform") {
        PropertyFamily::Placement
    } else if type_name.contains("PropertyMatrix") {
        PropertyFamily::Matrix
    } else if type_name.contains("PropertyVector") {
        PropertyFamily::Vector
    } else if type_name.contains("PropertyEnumeration") {
        PropertyFamily::Enumeration
    } else if type_name.contains("PropertyQuantity")
        || type_name.contains("PropertyLength")
        || type_name.contains("PropertyDistance")
        || type_name.contains("PropertyAngle")
        || type_name.contains("PropertyArea")
        || type_name.contains("PropertyVolume")
        || type_name.contains("PropertySpeed")
        || type_name.contains("PropertyAcceleration")
        || type_name.contains("PropertyPressure")
        || type_name.contains("PropertyForce")
    {
        PropertyFamily::Quantity
    } else if type_name.contains("PropertyMap") {
        PropertyFamily::Map
    } else if type_name.contains("List") {
        PropertyFamily::List
    } else if type_name.contains("PropertyString")
        || type_name.contains("PropertyPath")
        || type_name.contains("PropertyUUID")
    {
        PropertyFamily::String
    } else if type_name.contains("PropertyBool")
        || type_name.contains("PropertyInteger")
        || type_name.contains("PropertyFloat")
        || type_name.contains("PropertyPercent")
    {
        PropertyFamily::Scalar
    } else {
        PropertyFamily::Unknown
    }
}

fn link_targets(value: &ValueRecord) -> Vec<LinkTarget> {
    let object = attribute_any(
        &value.attributes,
        &[
            "value", "Value", "object", "Object", "obj", "Obj", "name", "Name",
        ],
    );
    let document = attribute_any(&value.attributes, &["document", "Document", "doc", "Doc"]);
    let subelements = value
        .attributes
        .iter()
        .filter(|(name, _)| name.to_ascii_lowercase().contains("sub"))
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    if object.is_some() || document.is_some() || !subelements.is_empty() {
        vec![LinkTarget {
            document,
            object,
            subelements,
        }]
    } else {
        Vec::new()
    }
}

fn attribute_any(attributes: &BTreeMap<String, String>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| attributes.get(*name).cloned())
}

fn object_id(name: &str) -> String {
    format!("fcstd:object:{name}")
}

fn extension_id(owner: &str, name: &str, order: usize) -> String {
    format!("{owner}:extension:{order}:{name}")
}

fn required_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Result<String, CodecError> {
    node.attribute(name).map(str::to_owned).ok_or_else(|| {
        CodecError::Malformed(format!(
            "{} element has no {name} attribute",
            node.tag_name().name()
        ))
    })
}

fn bool_attr(value: Option<&str>) -> Option<bool> {
    value.map(|value| matches!(value, "1" | "true" | "True" | "TRUE"))
}
