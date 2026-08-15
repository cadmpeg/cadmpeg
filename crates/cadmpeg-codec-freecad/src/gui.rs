// SPDX-License-Identifier: Apache-2.0
//! Transfer of `GuiDocument.xml` object appearance into neutral presentation records.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, ViewPresentation,
};
use cadmpeg_ir::topology::Color;

use crate::brep::ShapePayloadRecord;
use crate::native::{
    ElementMapGroup, ElementMapRecord, GuiDocumentRecord, GuiPropertyRecord, GuiStateRecord,
    GuiViewProviderRecord, ObjectRecord, PropertyRecord, ValueRecord,
};

#[derive(Default)]
pub(crate) struct Graph {
    pub(crate) documents: Vec<GuiDocumentRecord>,
    pub(crate) providers: Vec<GuiViewProviderRecord>,
    pub(crate) properties: Vec<GuiPropertyRecord>,
}

/// Whether the shared application-property registry knows this GUI property type.
pub(crate) fn has_registered_property_grammar(type_name: &str) -> bool {
    gui_value_tag(type_name).is_some()
        || !matches!(
            crate::persistence::property_family(type_name),
            crate::native::PropertyFamily::Unknown
        )
}

pub(crate) fn requires_alpha_conversion(program_version: Option<&str>) -> bool {
    program_version.is_some_and(|version| version.starts_with('0') || version.starts_with("1.0"))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn transfer(
    ir: &mut CadIr,
    bytes: &[u8],
    entries: &BTreeMap<String, View<'_>>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
    requires_alpha_conversion: bool,
) -> Result<Graph, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("GuiDocument.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::Malformed(format!("invalid GuiDocument.xml: {error}")))?;
    let root = xml.root_element();
    let schema_version = root
        .attribute("SchemaVersion")
        .and_then(|value| value.parse().ok());
    let camera_count = root
        .children()
        .filter(|node| node.has_tag_name("Camera"))
        .count();
    if schema_version == Some(1) && camera_count != 1 {
        return Err(CodecError::Malformed(format!(
            "GuiDocument.xml schema 1 requires one Camera record, found {camera_count}"
        )));
    }
    let states = root
        .children()
        .filter(roxmltree::Node::is_element)
        .filter(|node| !node.has_tag_name("ViewProviderData"))
        .enumerate()
        .map(|(order, node)| gui_state(text, order, node))
        .collect::<Vec<_>>();
    let document = GuiDocumentRecord {
        id: "fcstd:gui:document#0".into(),
        schema_version,
        attributes: root
            .attributes()
            .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
            .collect(),
        states,
    };
    let objects_by_name = objects
        .iter()
        .map(|object| (object.name.as_str(), object.id.as_str()))
        .collect::<HashMap<_, _>>();
    let mut native_providers = Vec::new();
    let mut native_properties = Vec::new();
    let payloads_by_owner = payloads
        .iter()
        .filter_map(|payload| {
            let property = properties
                .iter()
                .find(|property| property.id == payload.property)?;
            Some((
                property.owner.as_str(),
                property.name.as_str(),
                payload.id.as_str(),
            ))
        })
        .collect::<Vec<_>>();
    let mut view_provider_data = xml
        .descendants()
        .filter(|node| node.has_tag_name("ViewProviderData"));
    let first_view_provider_data = view_provider_data.next();
    if view_provider_data.next().is_some() {
        return Err(CodecError::Malformed(
            "GuiDocument.xml has multiple ViewProviderData containers".into(),
        ));
    }
    let providers = xml
        .descendants()
        .filter(|node| node.has_tag_name("ViewProvider"))
        .collect::<Vec<_>>();
    if let Some(container) = first_view_provider_data {
        let declared = container
            .attribute("Count")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| CodecError::Malformed("invalid ViewProviderData Count".into()))?;
        if declared != providers.len() {
            return Err(CodecError::Malformed(format!(
                "ViewProviderData Count={declared} but {} records were found",
                providers.len()
            )));
        }
    }
    let mut provider_names = HashSet::new();
    for (provider_order, provider) in providers.into_iter().enumerate() {
        let Some(name) = provider.attribute("name") else {
            return Err(CodecError::Malformed("ViewProvider has no name".into()));
        };
        if !provider_names.insert(name) {
            return Err(CodecError::Malformed(
                "GuiDocument.xml has duplicate ViewProvider names".into(),
            ));
        }
        let Some(object_id) = objects_by_name.get(name).copied() else {
            append_native_provider(
                text,
                provider,
                provider_order,
                None,
                &mut native_providers,
                &mut native_properties,
            )?;
            continue;
        };
        append_native_provider(
            text,
            provider,
            provider_order,
            Some(object_id),
            &mut native_providers,
            &mut native_properties,
        )?;
        let properties_node = unique_child(provider, "Properties")?.ok_or_else(|| {
            CodecError::Malformed(format!("ViewProvider {name} has no Properties"))
        })?;
        let property_nodes = properties_node
            .children()
            .filter(|node| node.has_tag_name("Property"))
            .collect::<Vec<_>>();
        let values = property_nodes
            .into_iter()
            .filter(|property| {
                property
                    .attribute("name")
                    .and_then(presentation_property_type)
                    .is_some_and(|expected| property.attribute("type") == Some(expected))
            })
            .filter_map(|property| {
                Some((
                    property.attribute("name")?,
                    property.children().find(roxmltree::Node::is_element)?,
                ))
            })
            .collect::<HashMap<_, _>>();
        let visibility = values
            .get("Visibility")
            .and_then(|value| value.attribute("value"))
            .and_then(parse_bool);
        let transparency = values
            .get("Transparency")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<f32>().ok())
            .map(|percent| (percent / 100.0).clamp(0.0, 1.0));
        let packed_color = values
            .get("ShapeColor")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
            .map(|value| convert_packed_alpha(value, requires_alpha_conversion));
        let material = values.get("ShapeMaterial");
        let body_ids = payloads_by_owner
            .iter()
            .filter(|(owner, property, _)| *owner == object_id && *property == "Shape")
            .flat_map(|(_, _, payload)| {
                ir.model
                    .bodies
                    .iter()
                    .filter(move |body| {
                        crate::native::id_key(&body.id.0)
                            .starts_with(&format!("{}:", crate::native::id_key(payload)))
                    })
                    .map(|body| body.id.clone())
            })
            .collect::<Vec<_>>();
        for body_id in &body_ids {
            if let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == *body_id) {
                body.visible = visibility;
                body.color = packed_color.map(|packed| decode_color(packed, transparency));
            }
        }
        if let Some(file) = values
            .get("DiffuseColor")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                name,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Face,
                requires_alpha_conversion,
            )?;
        }
        let payload_prefixes = payloads_by_owner
            .iter()
            .filter(|(owner, property, _)| *owner == object_id && *property == "Shape")
            .map(|(_, _, payload)| format!("{}:", crate::native::id_key(payload)))
            .collect::<Vec<_>>();
        if let Some(color) = values
            .get("LineColor")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
            .map(|value| convert_packed_alpha(value, requires_alpha_conversion))
        {
            let width = values
                .get("LineWidth")
                .and_then(|value| value.attribute("value"))
                .and_then(|value| value.parse::<f64>().ok());
            transfer_edge_appearance(ir, name, object_id, color, width, &payload_prefixes);
        }
        if let Some(file) = values
            .get("LineColorArray")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                name,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Edge,
                requires_alpha_conversion,
            )?;
        }
        if let Some(color) = values
            .get("PointColor")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
            .map(|value| convert_packed_alpha(value, requires_alpha_conversion))
        {
            let size = values
                .get("PointSize")
                .and_then(|value| value.attribute("value"))
                .and_then(|value| value.parse::<f64>().ok());
            transfer_vertex_appearance(ir, name, object_id, color, size, &payload_prefixes);
        }
        if let Some(file) = values
            .get("PointColorArray")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                name,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Vertex,
                requires_alpha_conversion,
            )?;
        }
        let Some(packed_color) = packed_color else {
            continue;
        };
        let appearance_id = AppearanceId(format!("fcstd:appearance:object#{name}"));
        let mut material_properties = BTreeMap::new();
        if let Some(material) = material {
            for (source, target) in [
                ("shininess", "shininess"),
                ("transparency", "material_transparency"),
            ] {
                if let Some(value) = material
                    .attribute(source)
                    .and_then(|value| value.parse::<f64>().ok())
                {
                    material_properties.insert(target.into(), value);
                }
            }
        }
        ir.model.appearances.push(Appearance {
            id: appearance_id.clone(),
            name: Some(format!("{name} shape appearance")),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("FCStd ViewProvider ShapeMaterial".into()),
            category: None,
            base_color: Some(decode_color(packed_color, transparency)),
            textures: Vec::new(),
            properties: material_properties,
        });
        for (index, body) in body_ids.into_iter().enumerate() {
            ir.model.appearance_bindings.push(AppearanceBinding {
                id: format!("fcstd:appearance:binding#{name}:{index}"),
                target: AppearanceTarget::Body(body),
                appearance: appearance_id.clone(),
                source_entity_id: Some(object_id.to_owned()),
                object_type: Some("ViewProvider".into()),
                channels: BTreeMap::new(),
            });
        }
    }
    let graph = Graph {
        documents: vec![document],
        providers: native_providers,
        properties: native_properties,
    };
    let material_lists =
        validate_gui_list_payloads(&graph.properties, entries, requires_alpha_conversion)?;
    transfer_shape_appearances(
        ir,
        &graph,
        &material_lists,
        properties,
        payloads,
        element_maps,
    )?;
    transfer_neutral_presentation(ir, &graph)?;
    Ok(graph)
}

fn presentation_property_type(name: &str) -> Option<&'static str> {
    match name {
        "Visibility" => Some("App::PropertyBool"),
        "DisplayMode" | "SelectionStyle" => Some("App::PropertyEnumeration"),
        "Transparency" => Some("App::PropertyPercent"),
        "ShapeColor" | "LineColor" | "PointColor" => Some("App::PropertyColor"),
        "ShapeMaterial" => Some("App::PropertyMaterial"),
        "DiffuseColor" | "LineColorArray" | "PointColorArray" => Some("App::PropertyColorList"),
        "ShapeAppearance" => Some("App::PropertyMaterialList"),
        "LineWidth" | "PointSize" => Some("App::PropertyFloatConstraint"),
        _ => None,
    }
}

fn transfer_neutral_presentation(ir: &mut CadIr, graph: &Graph) -> Result<(), CodecError> {
    for document in &graph.documents {
        let mut camera_states = document
            .states
            .iter()
            .filter(|state| state.kind == "Camera");
        let camera_state = camera_states
            .next()
            .filter(|_| camera_states.next().is_none());
        let camera = camera_state.map(|state| CameraState {
            position: state
                .values
                .iter()
                .find(|value| value.tag == "Position")
                .and_then(|value| vector3(&value.attributes)),
            orientation: state
                .attributes
                .get("orientation")
                .and_then(|value| parse_vector::<4>(value)),
            properties: state.attributes.clone(),
        });
        ir.model.presentation_documents.push(PresentationDocument {
            id: PresentationId("fcstd:presentation:document#0".into()),
            schema_version: document.schema_version,
            active_view: None,
            camera,
            states: document
                .states
                .iter()
                .map(|state| PresentationState {
                    kind: state.kind.clone(),
                    order: state.order as u32,
                    attributes: state.attributes.clone(),
                    assets: state
                        .side_entries
                        .iter()
                        .map(|entry| crate::native::native_id("entry", entry))
                        .collect(),
                })
                .collect(),
            native_ref: Some(document.id.clone()),
        });
    }

    let properties = graph.properties.iter().fold(
        HashMap::<&str, Vec<&GuiPropertyRecord>>::new(),
        |mut map, property| {
            map.entry(property.owner.as_str())
                .or_default()
                .push(property);
            map
        },
    );
    for provider in &graph.providers {
        let owned = properties
            .get(provider.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let property_value = |name: &str, type_name: &str| {
            owned
                .iter()
                .find(|property| property.name == name && property.type_name == type_name)
                .and_then(|property| gui_property_value(property))
        };
        let line_width = property_value("LineWidth", "App::PropertyFloatConstraint")
            .and_then(|value| value.parse::<f64>().ok());
        let point_size = property_value("PointSize", "App::PropertyFloatConstraint")
            .and_then(|value| value.parse::<f64>().ok());
        if line_width.is_some_and(|value| value < 0.0)
            || point_size.is_some_and(|value| value < 0.0)
        {
            return Err(CodecError::Malformed(format!(
                "ViewProvider {} has a negative line or point size",
                provider.name
            )));
        }
        ir.model.view_presentations.push(ViewPresentation {
            id: PresentationId(crate::native::model_id(
                "presentation-view",
                &provider.id,
                "state",
            )),
            object: provider.object.clone(),
            order: provider.order as u32,
            expanded: provider.expanded,
            visible: property_value("Visibility", "App::PropertyBool").and_then(parse_bool),
            display_mode: property_value("DisplayMode", "App::PropertyEnumeration")
                .map(str::to_owned),
            selection_style: property_value("SelectionStyle", "App::PropertyEnumeration")
                .map(str::to_owned),
            line_width,
            point_size,
            properties: owned
                .iter()
                .map(|property| {
                    (
                        property.name.clone(),
                        gui_property_value(property)
                            .map_or_else(|| property.raw_xml.clone(), str::to_owned),
                    )
                })
                .collect(),
            native_ref: Some(provider.id.clone()),
        });
    }
    Ok(())
}

fn gui_property_value(property: &GuiPropertyRecord) -> Option<&str> {
    property.values.iter().find_map(|value| {
        value
            .attributes
            .get("value")
            .or_else(|| value.attributes.get("Value"))
            .map(String::as_str)
    })
}

fn vector3(attributes: &BTreeMap<String, String>) -> Option<[f64; 3]> {
    Some([
        attributes.get("x")?.parse().ok()?,
        attributes.get("y")?.parse().ok()?,
        attributes.get("z")?.parse().ok()?,
    ])
}

fn parse_vector<const N: usize>(value: &str) -> Option<[f64; N]> {
    let values = value
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<Vec<f64>, _>>()
        .ok()?;
    values.try_into().ok()
}

fn transfer_edge_appearance(
    ir: &mut CadIr,
    provider_name: &str,
    object_id: &str,
    packed_color: u32,
    width: Option<f64>,
    payload_prefixes: &[String],
) {
    let edges = ir
        .model
        .edges
        .iter()
        .filter(|edge| {
            payload_prefixes
                .iter()
                .any(|prefix| crate::native::id_key(&edge.id.0).starts_with(prefix))
        })
        .map(|edge| edge.id.clone())
        .collect::<Vec<_>>();
    if edges.is_empty() {
        return;
    }
    let appearance_id = AppearanceId(format!("fcstd:appearance:edge#{provider_name}"));
    ir.model.appearances.push(Appearance {
        id: appearance_id.clone(),
        name: Some(format!("{provider_name} line appearance")),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("FCStd ViewProvider line style".into()),
        category: None,
        base_color: Some(decode_color(packed_color, None)),
        textures: Vec::new(),
        properties: width
            .filter(|width| width.is_finite() && *width >= 0.0)
            .map(|width| [("line_width".into(), width)].into())
            .unwrap_or_default(),
    });
    for (index, edge) in edges.into_iter().enumerate() {
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: format!("fcstd:appearance:binding#edge:{provider_name}:{index}"),
            target: AppearanceTarget::Edge(edge),
            appearance: appearance_id.clone(),
            source_entity_id: Some(object_id.to_owned()),
            object_type: Some("ViewProvider Edge".into()),
            channels: [("precedence".into(), "edge_over_object".into())].into(),
        });
    }
}

fn transfer_vertex_appearance(
    ir: &mut CadIr,
    provider_name: &str,
    object_id: &str,
    packed_color: u32,
    size: Option<f64>,
    payload_prefixes: &[String],
) {
    let vertices = ir
        .model
        .vertices
        .iter()
        .filter(|vertex| {
            payload_prefixes
                .iter()
                .any(|prefix| crate::native::id_key(&vertex.id.0).starts_with(prefix))
        })
        .map(|vertex| vertex.id.clone())
        .collect::<Vec<_>>();
    if vertices.is_empty() {
        return;
    }
    let appearance_id = AppearanceId(format!("fcstd:appearance:vertex#{provider_name}"));
    ir.model.appearances.push(Appearance {
        id: appearance_id.clone(),
        name: Some(format!("{provider_name} point appearance")),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("FCStd ViewProvider point style".into()),
        category: None,
        base_color: Some(decode_color(packed_color, None)),
        textures: Vec::new(),
        properties: size
            .filter(|size| size.is_finite() && *size >= 0.0)
            .map(|size| [("point_size".into(), size)].into())
            .unwrap_or_default(),
    });
    for (index, vertex) in vertices.into_iter().enumerate() {
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: format!("fcstd:appearance:binding#vertex:{provider_name}:{index}"),
            target: AppearanceTarget::Vertex(vertex),
            appearance: appearance_id.clone(),
            source_entity_id: Some(object_id.to_owned()),
            object_type: Some("ViewProvider Vertex".into()),
            channels: [("precedence".into(), "vertex_over_object".into())].into(),
        });
    }
}

fn gui_state(text: &str, order: usize, node: roxmltree::Node<'_, '_>) -> GuiStateRecord {
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
    let side_entries = node
        .descendants()
        .filter(roxmltree::Node::is_element)
        .flat_map(|element| {
            element
                .attributes()
                .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                .collect::<Vec<_>>()
        })
        .filter(|(name, _)| matches!(name.as_str(), "file" | "File"))
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
        .collect();
    GuiStateRecord {
        id: crate::native::native_id("gui-state", format!("{}:{order}", node.tag_name().name())),
        kind: node.tag_name().name().to_owned(),
        order,
        attributes: node
            .attributes()
            .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
            .collect(),
        values,
        side_entries,
        raw_xml: text[node.range()].to_owned(),
        byte_start: node.range().start as u64,
        byte_end: node.range().end as u64,
    }
}

fn unique_child<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Result<Option<roxmltree::Node<'a, 'input>>, CodecError> {
    let mut children = parent
        .children()
        .filter(|child| child.is_element() && child.has_tag_name(tag));
    let Some(first) = children.next() else {
        return Ok(None);
    };
    if children.next().is_some() {
        return Err(CodecError::Malformed(
            "GUI record has multiple child containers".into(),
        ));
    }
    Ok(Some(first))
}

fn append_native_provider(
    text: &str,
    provider: roxmltree::Node<'_, '_>,
    order: usize,
    object: Option<&str>,
    providers: &mut Vec<GuiViewProviderRecord>,
    properties: &mut Vec<GuiPropertyRecord>,
) -> Result<(), CodecError> {
    let name = provider
        .attribute("name")
        .ok_or_else(|| CodecError::Malformed("ViewProvider has no name".into()))?;
    let id = crate::native::native_id("gui-view-provider", name);
    providers.push(GuiViewProviderRecord {
        id: id.clone(),
        object: object.map(str::to_owned),
        name: name.to_owned(),
        expanded: provider.attribute("expanded").and_then(parse_bool),
        order,
        raw_xml: text[provider.range()].to_owned(),
    });
    let Some(container) = unique_child(provider, "Properties")? else {
        return Err(CodecError::Malformed(format!(
            "ViewProvider {name} has no Properties"
        )));
    };
    let property_nodes = container
        .children()
        .filter(|node| node.has_tag_name("Property"))
        .collect::<Vec<_>>();
    let declared = container
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::Malformed(format!("ViewProvider {name} has invalid property count"))
        })?;
    if declared != property_nodes.len() {
        return Err(CodecError::Malformed(format!(
            "ViewProvider {name} declares {declared} properties but contains {}",
            property_nodes.len()
        )));
    }
    let mut property_names = HashSet::new();
    for (property_order, property) in property_nodes.into_iter().enumerate() {
        let property_name = property.attribute("name").ok_or_else(|| {
            CodecError::Malformed(format!("ViewProvider {name} property has no name"))
        })?;
        if !property_names.insert(property_name) {
            return Err(CodecError::Malformed(
                "ViewProvider has duplicate property names".into(),
            ));
        }
        let type_name = property.attribute("type").ok_or_else(|| {
            CodecError::Malformed(format!("ViewProvider {name}.{property_name} has no type"))
        })?;
        validate_gui_property(property, property_name, type_name)?;
        let values = property
            .descendants()
            .filter(|value| value.is_element() && *value != property)
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
        let side_entries = values
            .iter()
            .flat_map(|value| value.attributes.iter())
            .filter(|(attribute, _)| {
                matches!(attribute.as_str(), "file" | "File")
                    && !crate::persistence::is_xlink_type(type_name)
            })
            .map(|(_, value)| value.clone())
            .filter(|value| !value.is_empty())
            .collect();
        properties.push(GuiPropertyRecord {
            id: crate::native::native_child_id("gui-property", &id, property_name),
            owner: id.clone(),
            name: property_name.to_owned(),
            type_name: type_name.to_owned(),
            status: property
                .attribute("status")
                .and_then(|value| value.parse().ok()),
            order: property_order,
            values,
            side_entries,
            raw_xml: text[property.range()].to_owned(),
            byte_start: property.range().start as u64,
            byte_end: property.range().end as u64,
        });
    }
    Ok(())
}

fn validate_gui_property(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
    type_name: &str,
) -> Result<(), CodecError> {
    let Some(expected_tag) = gui_value_tag(type_name) else {
        return Ok(());
    };
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let root = roots.first().copied().ok_or_else(|| {
        CodecError::Malformed(format!(
            "GUI property {property_name} requires one {expected_tag} value"
        ))
    })?;
    if !root.has_tag_name(expected_tag) {
        return Err(CodecError::Malformed(format!(
            "GUI property {property_name} requires a leading {expected_tag} value"
        )));
    }
    let scalar = |attribute: &str| {
        root.attribute(attribute).ok_or_else(|| {
            CodecError::Malformed(format!(
                "GUI property {property_name} {expected_tag} has no {attribute} attribute"
            ))
        })
    };
    match expected_tag {
        "Bool" => {
            if parse_bool(scalar("value")?).is_none() {
                return Err(CodecError::Malformed(format!(
                    "GUI property {property_name} has an invalid Boolean"
                )));
            }
        }
        "Integer" => {
            scalar("value")?.parse::<i64>().map_err(|_| {
                CodecError::Malformed(format!(
                    "GUI property {property_name} has an invalid integer"
                ))
            })?;
            if type_name == "App::PropertyEnumeration" {
                validate_gui_enumeration(&roots, property_name)?;
                return Ok(());
            }
        }
        "Float" => {
            let value = scalar("value")?.parse::<f64>().map_err(|_| {
                CodecError::Malformed(format!("GUI property {property_name} has an invalid float"))
            })?;
            if !value.is_finite() {
                return Err(CodecError::Malformed(format!(
                    "GUI property {property_name} has a non-finite float"
                )));
            }
        }
        "String" | "Python" | "ColorList" | "MaterialList" => {
            let attribute = if matches!(expected_tag, "ColorList" | "MaterialList") {
                "file"
            } else {
                "value"
            };
            scalar(attribute)?;
            if type_name == "App::PropertyPersistentObject" {
                if roots.len() != 2 || !roots[1].has_tag_name("PersistentObject") {
                    return Err(CodecError::Malformed(format!(
                        "GUI property {property_name} has an invalid persistent-object envelope"
                    )));
                }
                return Ok(());
            }
            if expected_tag == "MaterialList" {
                let version = root
                    .attribute("version")
                    .map(str::parse::<u32>)
                    .transpose()
                    .map_err(|_| {
                        CodecError::Malformed(format!(
                            "GUI property {property_name} has an invalid material-list version"
                        ))
                    })?
                    .unwrap_or(0);
                if version > 3 {
                    return Err(CodecError::NotImplemented(format!(
                        "FCStd GUI material-list version {version}"
                    )));
                }
            }
        }
        "PropertyColor" => {
            scalar("value")?.parse::<u32>().map_err(|_| {
                CodecError::Malformed(format!("GUI property {property_name} has an invalid color"))
            })?;
        }
        "PropertyVector" => {
            for attribute in ["valueX", "valueY", "valueZ"] {
                let value = scalar(attribute)?.parse::<f64>().map_err(|_| {
                    CodecError::Malformed(format!(
                        "GUI property {property_name} has an invalid vector"
                    ))
                })?;
                if !value.is_finite() {
                    return Err(CodecError::Malformed(format!(
                        "GUI property {property_name} has a non-finite vector"
                    )));
                }
            }
        }
        "PropertyMaterial" => validate_gui_material(root, property_name)?,
        "BoolList" => {
            if !scalar("value")?
                .bytes()
                .all(|byte| matches!(byte, b'0' | b'1'))
            {
                return Err(CodecError::Malformed(format!(
                    "GUI property {property_name} has an invalid Boolean list"
                )));
            }
        }
        _ => unreachable!("closed GUI value-tag registry"),
    }
    if roots.len() != 1 {
        return Err(CodecError::Malformed(format!(
            "GUI property {property_name} requires exactly one {expected_tag} value"
        )));
    }
    Ok(())
}

fn validate_gui_enumeration(
    roots: &[roxmltree::Node<'_, '_>],
    property_name: &str,
) -> Result<(), CodecError> {
    let custom = roots[0].attribute("CustomEnum").is_some();
    if !custom && roots.len() == 1 {
        return Ok(());
    }
    if !custom || roots.len() != 2 || !roots[1].has_tag_name("CustomEnumList") {
        return Err(CodecError::Malformed(format!(
            "GUI property {property_name} has an invalid custom enumeration envelope"
        )));
    }
    let values = roots[1]
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let count = roots[1]
        .attribute("count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "GUI property {property_name} has an invalid custom enumeration count"
            ))
        })?;
    if values.len() != count
        || values
            .iter()
            .any(|value| !value.has_tag_name("Enum") || value.attribute("value").is_none())
    {
        return Err(CodecError::Malformed(format!(
            "GUI property {property_name} custom enumeration count or value is invalid"
        )));
    }
    Ok(())
}

fn gui_value_tag(type_name: &str) -> Option<&'static str> {
    let tag = match type_name {
        "App::PropertyBool" => "Bool",
        "App::PropertyEnumeration"
        | "App::PropertyInteger"
        | "App::PropertyIntegerConstraint"
        | "App::PropertyPercent" => "Integer",
        "App::PropertyAngle"
        | "App::PropertyDistance"
        | "App::PropertyFloat"
        | "App::PropertyFloatConstraint"
        | "App::PropertyLength" => "Float",
        "App::PropertyFile"
        | "App::PropertyFont"
        | "App::PropertyPersistentObject"
        | "App::PropertyString" => "String",
        "App::PropertyColor" => "PropertyColor",
        "App::PropertyColorList" => "ColorList",
        "App::PropertyMaterial" => "PropertyMaterial",
        "App::PropertyMaterialList" => "MaterialList",
        "App::PropertyVector" => "PropertyVector",
        "App::PropertyBoolList" => "BoolList",
        "App::PropertyPythonObject" => "Python",
        _ => return None,
    };
    Some(tag)
}

fn validate_gui_material(
    value: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    for attribute in [
        "ambientColor",
        "diffuseColor",
        "specularColor",
        "emissiveColor",
    ] {
        value
            .attribute(attribute)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "GUI property {property_name} material has no {attribute}"
                ))
            })?
            .parse::<u32>()
            .map_err(|_| {
                CodecError::Malformed(format!(
                    "GUI property {property_name} material has an invalid {attribute}"
                ))
            })?;
    }
    for attribute in ["shininess", "transparency"] {
        let scalar = value
            .attribute(attribute)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "GUI property {property_name} material has no {attribute}"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                CodecError::Malformed(format!(
                    "GUI property {property_name} material has an invalid {attribute}"
                ))
            })?;
        if !scalar.is_finite() {
            return Err(CodecError::Malformed(format!(
                "GUI property {property_name} material has a non-finite {attribute}"
            )));
        }
    }
    Ok(())
}

#[derive(Clone)]
struct GuiMaterial {
    ambient: u32,
    diffuse: u32,
    specular: u32,
    emissive: u32,
    shininess: f32,
    transparency: f32,
    image: String,
    image_path: String,
    uuid: String,
}

fn validate_gui_list_payloads(
    properties: &[GuiPropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
    requires_alpha_conversion: bool,
) -> Result<HashMap<String, Vec<GuiMaterial>>, CodecError> {
    let mut material_lists = HashMap::new();
    for property in properties {
        let Some(entry_name) = property.side_entries.first() else {
            continue;
        };
        if property.side_entries.len() != 1 {
            return Err(CodecError::Malformed(format!(
                "GUI property {} references more than one side entry",
                property.id
            )));
        }
        let view = *entries.get(entry_name).ok_or_else(|| {
            CodecError::Malformed(format!(
                "GUI property {} references missing side entry {entry_name}",
                property.id
            ))
        })?;
        match property.type_name.as_str() {
            "App::PropertyColorList" => {
                parse_color_list(view, entry_name, requires_alpha_conversion)?;
            }
            "App::PropertyMaterialList" => {
                let version = property
                    .values
                    .iter()
                    .find(|value| value.tag == "MaterialList")
                    .and_then(|value| value.attributes.get("version"))
                    .map(|value| {
                        value.parse::<u32>().map_err(|_| {
                            CodecError::Malformed(format!(
                                "GUI material list {} has an invalid version",
                                property.id
                            ))
                        })
                    })
                    .transpose()?
                    .unwrap_or(0);
                material_lists.insert(
                    property.id.clone(),
                    parse_material_list(view, version, &property.id, requires_alpha_conversion)?,
                );
            }
            _ => {}
        }
    }
    Ok(material_lists)
}

fn parse_color_list(
    mut view: View<'_>,
    entry_name: &str,
    requires_alpha_conversion: bool,
) -> Result<Vec<u32>, CodecError> {
    let count = view.req_u32_le()?;
    let colors = view
        .read_counted(count.into(), 4, View::u32_le)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "color-list entry {entry_name} count exceeds its payload"
            ))
        })?;
    if !view.is_empty() {
        return Err(CodecError::Malformed(format!(
            "color-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(colors
        .into_iter()
        .map(|value| convert_packed_alpha(value, requires_alpha_conversion))
        .collect())
}

fn parse_material_list(
    mut view: View<'_>,
    version: u32,
    property_id: &str,
    requires_alpha_conversion: bool,
) -> Result<Vec<GuiMaterial>, CodecError> {
    let (count, has_strings) = match version {
        0 | 1 => {
            let header = view.i32_le().ok_or_else(|| {
                CodecError::Malformed(format!("GUI material list {property_id} is truncated"))
            })?;
            let count = if header < 0 {
                view.u32_le().ok_or_else(|| {
                    CodecError::Malformed(format!("GUI material list {property_id} is truncated"))
                })?
            } else {
                header as u32
            };
            (count, false)
        }
        2 => (
            view.u32_le().ok_or_else(|| {
                CodecError::Malformed(format!("GUI material list {property_id} is truncated"))
            })?,
            false,
        ),
        3 => (
            view.u32_le().ok_or_else(|| {
                CodecError::Malformed(format!("GUI material list {property_id} is truncated"))
            })?,
            true,
        ),
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "FCStd GUI material-list version {version}"
            )));
        }
    };
    let mut materials = view
        .read_counted(count.into(), 24, |view| {
            Some(GuiMaterial {
                ambient: view.u32_le()?,
                diffuse: view.u32_le()?,
                specular: view.u32_le()?,
                emissive: view.u32_le()?,
                shininess: view.f32_le()?,
                transparency: view.f32_le()?,
                image: String::new(),
                image_path: String::new(),
                uuid: String::new(),
            })
        })
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "GUI material list {property_id} count exceeds its payload"
            ))
        })?;
    for material in &materials {
        if !material.shininess.is_finite() || !material.transparency.is_finite() {
            return Err(CodecError::Malformed(format!(
                "GUI material list {property_id} has non-finite scalars"
            )));
        }
    }
    if requires_alpha_conversion {
        for material in &mut materials {
            material.ambient = convert_packed_alpha(material.ambient, true);
            material.diffuse = convert_packed_alpha(material.diffuse, true);
            material.specular = convert_packed_alpha(material.specular, true);
            material.emissive = convert_packed_alpha(material.emissive, true);
        }
    }
    if has_strings {
        for material in &mut materials {
            material.image = read_material_string(&mut view, property_id)?;
            material.image_path = read_material_string(&mut view, property_id)?;
            material.uuid = read_material_string(&mut view, property_id)?;
        }
    }
    if !view.is_empty() {
        return Err(CodecError::Malformed(format!(
            "GUI material list {property_id} has trailing bytes"
        )));
    }
    Ok(materials)
}

fn read_material_string(view: &mut View<'_>, property_id: &str) -> Result<String, CodecError> {
    let length = view.u32_le().ok_or_else(|| {
        CodecError::Malformed(format!(
            "GUI material list {property_id} string is truncated"
        ))
    })?;
    let length = view
        .counted(length.into(), 1)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "GUI material list {property_id} string exceeds its payload"
            ))
        })?
        .get();
    String::from_utf8(view.take(length).expect("counted material string").to_vec()).map_err(|_| {
        CodecError::Malformed(format!(
            "GUI material list {property_id} string is not UTF-8"
        ))
    })
}

fn transfer_shape_appearances(
    ir: &mut CadIr,
    graph: &Graph,
    material_lists: &HashMap<String, Vec<GuiMaterial>>,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
) -> Result<(), CodecError> {
    for provider in &graph.providers {
        let Some(object_id) = provider.object.as_deref() else {
            continue;
        };
        let Some(property) = graph.properties.iter().find(|property| {
            property.owner == provider.id
                && property.name == "ShapeAppearance"
                && property.type_name == "App::PropertyMaterialList"
        }) else {
            continue;
        };
        let Some(materials) = material_lists.get(&property.id) else {
            continue;
        };
        if materials.is_empty() {
            continue;
        }
        let body_ids = displayed_shape_bodies(ir, object_id, properties, payloads)?;
        let group = displayed_shape_group(object_id, properties, payloads, element_maps, "Face")?;
        let mapped_count = group.map_or(0, |group| group.names.len().saturating_sub(1));
        if materials.len() == 1 {
            let legacy_id = AppearanceId(format!("fcstd:appearance:object#{}", provider.name));
            ir.model
                .appearance_bindings
                .retain(|binding| binding.appearance != legacy_id);
            ir.model
                .appearances
                .retain(|appearance| appearance.id != legacy_id);
        } else {
            let Some(_) = group else {
                continue;
            };
            if mapped_count == 0 {
                continue;
            }
            if materials.len() != mapped_count {
                continue;
            }
        }
        for (index, material) in materials.iter().enumerate() {
            let appearance_id = AppearanceId(format!(
                "fcstd:appearance:shape-material#{}:{}",
                provider.name,
                index + 1
            ));
            ir.model.appearances.push(material_appearance(
                appearance_id.clone(),
                &provider.name,
                index,
                material,
            ));
            if materials.len() == 1 {
                for (body_index, body) in body_ids.iter().enumerate() {
                    if let Some(body) = ir.model.bodies.iter_mut().find(|item| item.id == *body) {
                        body.color =
                            Some(decode_color(material.diffuse, Some(material.transparency)));
                    }
                    ir.model.appearance_bindings.push(AppearanceBinding {
                        id: format!(
                            "fcstd:appearance:binding#shape-material:{}:{body_index}",
                            provider.name
                        ),
                        target: AppearanceTarget::Body(body.clone()),
                        appearance: appearance_id.clone(),
                        source_entity_id: Some(object_id.to_owned()),
                        object_type: Some("ViewProvider ShapeAppearance".into()),
                        channels: BTreeMap::new(),
                    });
                }
            } else if let Some(group) = group {
                bind_material_faces(ir, group, index, &appearance_id, &provider.name, object_id);
            }
        }
    }
    Ok(())
}

fn displayed_shape_bodies(
    ir: &CadIr,
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<cadmpeg_ir::ids::BodyId>, CodecError> {
    Ok(displayed_shape_payload(object_id, properties, payloads)?
        .into_iter()
        .flat_map(|payload| {
            let prefix = format!("{}:", crate::native::id_key(&payload.id));
            ir.model
                .bodies
                .iter()
                .filter(move |body| crate::native::id_key(&body.id.0).starts_with(&prefix))
                .map(|body| body.id.clone())
        })
        .collect())
}

fn displayed_shape_payload<'a>(
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &'a [ShapePayloadRecord],
) -> Result<Option<&'a ShapePayloadRecord>, CodecError> {
    let shape_properties = properties
        .iter()
        .filter(|property| property.owner == object_id && property.name == "Shape")
        .collect::<Vec<_>>();
    let property = match shape_properties.as_slice() {
        [] => return Ok(None),
        [property] => property,
        _ => {
            return Err(CodecError::Malformed(format!(
                "object {object_id} has multiple Shape properties"
            )));
        }
    };
    let shape_payloads = payloads
        .iter()
        .filter(|payload| payload.property == property.id)
        .collect::<Vec<_>>();
    match shape_payloads.as_slice() {
        [] => Ok(None),
        [payload] => Ok(Some(payload)),
        _ => Err(CodecError::Malformed(format!(
            "Shape property {} has multiple payloads",
            property.id
        ))),
    }
}

fn displayed_shape_group<'a>(
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &'a [ElementMapRecord],
    indexed_name: &str,
) -> Result<Option<&'a ElementMapGroup>, CodecError> {
    let Some(payload) = displayed_shape_payload(object_id, properties, payloads)? else {
        return Ok(None);
    };
    let shape_maps = element_maps
        .iter()
        .filter(|map| map.property == payload.property)
        .collect::<Vec<_>>();
    let map = match shape_maps.as_slice() {
        [] => return Ok(None),
        [map] => map,
        _ => {
            return Err(CodecError::Malformed(format!(
                "Shape property {} has multiple element maps",
                payload.property
            )));
        }
    };
    let Some(root) = map.maps.last() else {
        return Ok(None);
    };
    let groups = root
        .groups
        .iter()
        .filter(|group| group.indexed_name == indexed_name)
        .collect::<Vec<_>>();
    match groups.as_slice() {
        [] => Ok(None),
        [group] => Ok(Some(group)),
        _ => Err(CodecError::Malformed(format!(
            "Shape property {} has multiple {indexed_name} groups",
            payload.property
        ))),
    }
}

fn material_appearance(
    id: AppearanceId,
    provider_name: &str,
    index: usize,
    material: &GuiMaterial,
) -> Appearance {
    Appearance {
        id,
        name: Some(format!("{provider_name} face {} material", index + 1)),
        asset_guid: (!material.uuid.is_empty()).then(|| material.uuid.clone()),
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("FCStd ShapeAppearance".into()),
        category: None,
        base_color: Some(decode_color(material.diffuse, Some(material.transparency))),
        textures: Vec::new(),
        properties: [
            ("ambient_packed".into(), f64::from(material.ambient)),
            ("specular_packed".into(), f64::from(material.specular)),
            ("emissive_packed".into(), f64::from(material.emissive)),
            ("shininess".into(), f64::from(material.shininess)),
            ("transparency".into(), f64::from(material.transparency)),
        ]
        .into(),
    }
}

fn bind_material_faces(
    ir: &mut CadIr,
    group: &ElementMapGroup,
    material_index: usize,
    appearance_id: &AppearanceId,
    provider_name: &str,
    object_id: &str,
) {
    let mut bound = HashSet::new();
    for topology_id in group.names[material_index + 1]
        .iter()
        .flat_map(|name| &name.topology_ids)
        .filter(|id| bound.insert((*id).clone()))
    {
        let Some(face) = ir
            .model
            .faces
            .iter()
            .find(|face| face.id.0 == *topology_id)
            .map(|face| face.id.clone())
        else {
            continue;
        };
        let binding_index = ir.model.appearance_bindings.len();
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: format!("fcstd:appearance:binding#shape-material:{provider_name}:{binding_index}"),
            target: AppearanceTarget::Face(face),
            appearance: appearance_id.clone(),
            source_entity_id: Some(object_id.to_owned()),
            object_type: Some("ViewProvider ShapeAppearance".into()),
            channels: [("precedence".into(), "face_over_object".into())].into(),
        });
    }
}

#[derive(Clone, Copy)]
enum TopologyColorKind {
    Face,
    Edge,
    Vertex,
}

impl TopologyColorKind {
    fn name(self) -> &'static str {
        match self {
            Self::Face => "Face",
            Self::Edge => "Edge",
            Self::Vertex => "Vertex",
        }
    }

    fn schema(self) -> &'static str {
        match self {
            Self::Face => "FCStd DiffuseColor",
            Self::Edge => "FCStd LineColorArray",
            Self::Vertex => "FCStd PointColorArray",
        }
    }

    fn precedence(self) -> &'static str {
        match self {
            Self::Face => "face_over_object",
            Self::Edge => "edge_array_over_line",
            Self::Vertex => "vertex_array_over_point",
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn transfer_topology_colors(
    ir: &mut CadIr,
    provider_name: &str,
    object_id: &str,
    entry_name: &str,
    entries: &BTreeMap<String, View<'_>>,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
    kind: TopologyColorKind,
    requires_alpha_conversion: bool,
) -> Result<(), CodecError> {
    let view = *entries.get(entry_name).ok_or_else(|| {
        CodecError::Malformed(format!("color list references missing entry {entry_name}"))
    })?;
    let colors = parse_color_list(view, entry_name, requires_alpha_conversion)?;
    let count = colors.len();
    let Some(group) =
        displayed_shape_group(object_id, properties, payloads, element_maps, kind.name())?
    else {
        return Ok(());
    };
    // FreeCAD uses a single list entry as a uniform color for every mapped subelement.
    let mapped_count = group.names.len().saturating_sub(1);
    if mapped_count == 0 {
        return Ok(());
    }
    if count != 1 && mapped_count != count {
        return Err(CodecError::Malformed(format!(
            "{provider_name} {} color count {count} does not match {} mapped subelements",
            kind.name(),
            mapped_count
        )));
    }
    for (index, packed) in colors.into_iter().enumerate() {
        let lower = kind.name().to_ascii_lowercase();
        let appearance_id = AppearanceId(format!(
            "fcstd:appearance:{lower}#{provider_name}:{}",
            index + 1
        ));
        let uniform_names = (count == 1)
            .then_some(&group.names)
            .into_iter()
            .flat_map(|groups| groups.iter().flatten());
        let indexed_names = (count != 1)
            .then_some(&group.names[index + 1])
            .into_iter()
            .flat_map(|names| names.iter());
        let mut emitted_appearance = false;
        let mut bound_topology = HashSet::new();
        for topology_id in uniform_names
            .chain(indexed_names)
            .flat_map(|name| &name.topology_ids)
            .filter(|id| bound_topology.insert((*id).clone()))
            .filter(|id| match kind {
                TopologyColorKind::Face => ir.model.faces.iter().any(|face| face.id.0 == **id),
                TopologyColorKind::Edge => ir.model.edges.iter().any(|edge| edge.id.0 == **id),
                TopologyColorKind::Vertex => {
                    ir.model.vertices.iter().any(|vertex| vertex.id.0 == **id)
                }
            })
        {
            if !emitted_appearance {
                ir.model.appearances.push(Appearance {
                    id: appearance_id.clone(),
                    name: Some(format!(
                        "{provider_name} {}{} appearance",
                        kind.name(),
                        index + 1
                    )),
                    asset_guid: None,
                    library_id: None,
                    visual_guid: None,
                    physical_token: None,
                    schema: Some(kind.schema().into()),
                    category: None,
                    base_color: Some(decode_color(packed, None)),
                    textures: Vec::new(),
                    properties: BTreeMap::new(),
                });
                emitted_appearance = true;
            }
            let target = match kind {
                TopologyColorKind::Face => {
                    AppearanceTarget::Face(cadmpeg_ir::ids::FaceId(topology_id.clone()))
                }
                TopologyColorKind::Edge => {
                    AppearanceTarget::Edge(cadmpeg_ir::ids::EdgeId(topology_id.clone()))
                }
                TopologyColorKind::Vertex => {
                    AppearanceTarget::Vertex(cadmpeg_ir::ids::VertexId(topology_id.clone()))
                }
            };
            ir.model.appearance_bindings.push(AppearanceBinding {
                id: format!(
                    "fcstd:appearance:binding#{lower}:{provider_name}:{}:{}",
                    index + 1,
                    crate::native::id_key(topology_id)
                ),
                target,
                appearance: appearance_id.clone(),
                source_entity_id: Some(object_id.to_owned()),
                object_type: Some(format!("ViewProvider {}", kind.name())),
                channels: [("precedence".into(), kind.precedence().into())].into(),
            });
        }
    }
    Ok(())
}

fn decode_color(value: u32, transparency: Option<f32>) -> Color {
    Color {
        r: ((value >> 24) & 0xff) as f32 / 255.0,
        g: ((value >> 16) & 0xff) as f32 / 255.0,
        b: ((value >> 8) & 0xff) as f32 / 255.0,
        a: transparency.map_or((value & 0xff) as f32 / 255.0, |value| 1.0 - value),
    }
}

fn convert_packed_alpha(value: u32, required: bool) -> u32 {
    if required {
        (value & 0xffff_ff00) | (0xff - (value & 0xff))
    } else {
        value
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod color_tests {
    use super::{decode_color, parse_material_list, requires_alpha_conversion};

    #[test]
    fn packed_alpha_is_used_without_a_transparency_property() {
        let color = decode_color(0x1122_3340, None);
        assert!((color.a - 64.0 / 255.0).abs() < f32::EPSILON);
    }

    #[test]
    fn transparency_property_overrides_packed_alpha() {
        let color = decode_color(0x1122_3300, Some(0.25));
        assert!((color.a - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn program_version_selects_legacy_alpha_conversion() {
        assert!(requires_alpha_conversion(Some("0.21R33668")));
        assert!(requires_alpha_conversion(Some("1.0R39109")));
        assert!(!requires_alpha_conversion(Some("1.1R42000")));
        assert!(!requires_alpha_conversion(Some("cadmpeg")));
        assert!(!requires_alpha_conversion(None));
    }

    #[test]
    fn legacy_material_list_accepts_the_negative_version_marker() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        for color in [0x1122_3300_u32, 0x4455_6640, 0x7788_9980, 0xaabb_ccff] {
            bytes.extend_from_slice(&color.to_le_bytes());
        }
        bytes.extend_from_slice(&0.5_f32.to_le_bytes());
        bytes.extend_from_slice(&0.25_f32.to_le_bytes());

        let materials = parse_material_list(
            cadmpeg_core::decode::View::over_retained(&bytes),
            0,
            "property",
            true,
        )
        .expect("material list");
        assert_eq!(materials.len(), 1);
        assert_eq!(materials[0].ambient, 0x1122_33ff);
        assert_eq!(materials[0].diffuse, 0x4455_66bf);
        assert_eq!(materials[0].specular, 0x7788_997f);
        assert_eq!(materials[0].emissive, 0xaabb_cc00);
    }
}

#[cfg(test)]
mod shape_association_tests {
    use super::displayed_shape_group;
    use crate::brep::{ShapePayloadForm, ShapePayloadRecord};
    use crate::native::{
        ElementMapGroup, ElementMapNode, ElementMapRecord, PropertyFamily, PropertyRecord,
    };

    fn shape_property(id: &str) -> PropertyRecord {
        PropertyRecord {
            id: id.into(),
            owner: "object".into(),
            name: "Shape".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: PropertyFamily::Geometry,
            status: None,
            transient: false,
            dynamic: None,
            order: 0,
            values: Vec::new(),
            links: Vec::new(),
            side_entries: Vec::new(),
            raw_xml: "<Property/>".into(),
            byte_start: 0,
            byte_end: 11,
        }
    }

    fn shape_payload(id: &str, property: &str) -> ShapePayloadRecord {
        ShapePayloadRecord {
            id: id.into(),
            property: property.into(),
            entry: "Shape.brp".into(),
            form: ShapePayloadForm::Empty,
            text: None,
            binary: None,
        }
    }

    fn element_map(property: &str, groups: Vec<ElementMapGroup>) -> ElementMapRecord {
        ElementMapRecord {
            id: "map".into(),
            property: property.into(),
            version: "1.0".into(),
            hasher_index: None,
            source_entry: None,
            map_id: 1,
            declared_count: 0,
            postfixes: Vec::new(),
            maps: vec![ElementMapNode {
                index: 1,
                map_id: 1,
                groups,
            }],
        }
    }

    fn group(indexed_name: &str) -> ElementMapGroup {
        ElementMapGroup {
            indexed_name: indexed_name.into(),
            children: Vec::new(),
            names: Vec::new(),
        }
    }

    #[test]
    fn rejects_ambiguous_shape_association_candidates() {
        let property = shape_property("property");
        let payload = shape_payload("payload", "property");
        let map = element_map("property", vec![group("Face")]);

        let duplicate_property = shape_property("property-2");
        assert!(matches!(
            displayed_shape_group(
                "object",
                &[property.clone(), duplicate_property],
                std::slice::from_ref(&payload),
                std::slice::from_ref(&map),
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_payload = ShapePayloadRecord {
            id: "payload-2".into(),
            ..payload.clone()
        };
        assert!(matches!(
            displayed_shape_group(
                "object",
                std::slice::from_ref(&property),
                &[payload.clone(), duplicate_payload],
                std::slice::from_ref(&map),
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_map = ElementMapRecord {
            id: "map-2".into(),
            ..map.clone()
        };
        assert!(matches!(
            displayed_shape_group(
                "object",
                std::slice::from_ref(&property),
                std::slice::from_ref(&payload),
                &[map, duplicate_map],
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_group = element_map("property", vec![group("Face"), group("Face")]);
        assert!(matches!(
            displayed_shape_group(
                "object",
                &[property],
                &[payload],
                &[duplicate_group],
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[cfg(test)]
pub(crate) mod tests;
