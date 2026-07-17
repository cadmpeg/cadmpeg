// SPDX-License-Identifier: Apache-2.0
//! Transfer of `GuiDocument.xml` object appearance into neutral presentation records.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, ViewPresentation,
};
use cadmpeg_ir::topology::Color;

use crate::brep::ShapePayloadRecord;
use crate::native::{
    ElementMapRecord, GuiDocumentRecord, GuiPropertyRecord, GuiStateRecord, GuiViewProviderRecord,
    ObjectRecord, PropertyRecord, ValueRecord,
};

#[derive(Default)]
pub(crate) struct Graph {
    pub(crate) documents: Vec<GuiDocumentRecord>,
    pub(crate) providers: Vec<GuiViewProviderRecord>,
    pub(crate) properties: Vec<GuiPropertyRecord>,
}

pub(crate) fn transfer(
    ir: &mut CadIr,
    bytes: &[u8],
    entries: &BTreeMap<String, Vec<u8>>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
) -> Result<Graph, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("GuiDocument.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::Malformed(format!("invalid GuiDocument.xml: {error}")))?;
    let root = xml.root_element();
    let states = root
        .children()
        .filter(roxmltree::Node::is_element)
        .filter(|node| !node.has_tag_name("ViewProviderData"))
        .enumerate()
        .map(|(order, node)| gui_state(text, order, node))
        .collect::<Vec<_>>();
    let document = GuiDocumentRecord {
        id: "fcstd:gui:document#0".into(),
        schema_version: root
            .attribute("SchemaVersion")
            .and_then(|value| value.parse().ok()),
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
            let owner = properties
                .iter()
                .find(|property| property.id == payload.property)?
                .owner
                .as_str();
            Some((owner, payload.id.as_str()))
        })
        .collect::<Vec<_>>();
    let providers = xml
        .descendants()
        .filter(|node| node.has_tag_name("ViewProvider"))
        .collect::<Vec<_>>();
    if let Some(container) = xml
        .descendants()
        .find(|node| node.has_tag_name("ViewProviderData"))
    {
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
    for (provider_order, provider) in providers.into_iter().enumerate() {
        let Some(name) = provider.attribute("name") else {
            return Err(CodecError::Malformed("ViewProvider has no name".into()));
        };
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
        let properties_node = provider
            .children()
            .find(|node| node.has_tag_name("Properties"))
            .ok_or_else(|| {
                CodecError::Malformed(format!("ViewProvider {name} has no Properties"))
            })?;
        let property_nodes = properties_node
            .children()
            .filter(|node| node.has_tag_name("Property"))
            .collect::<Vec<_>>();
        let declared = properties_node
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
        let values = property_nodes
            .into_iter()
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
            .and_then(|value| value.parse::<u32>().ok());
        let material = values.get("ShapeMaterial");
        let body_ids = payloads_by_owner
            .iter()
            .filter(|(owner, _)| *owner == object_id)
            .flat_map(|(_, payload)| {
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
            )?;
        }
        let payload_prefixes = payloads_by_owner
            .iter()
            .filter(|(owner, _)| *owner == object_id)
            .map(|(_, payload)| format!("{}:", crate::native::id_key(payload)))
            .collect::<Vec<_>>();
        if let Some(color) = values
            .get("LineColor")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
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
            )?;
        }
        if let Some(color) = values
            .get("PointColor")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
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
            visual_guid: None,
            physical_token: None,
            schema: Some("FCStd ViewProvider ShapeMaterial".into()),
            category: None,
            base_color: Some(decode_color(packed_color, transparency)),
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
    transfer_neutral_presentation(ir, &graph);
    Ok(graph)
}

fn transfer_neutral_presentation(ir: &mut CadIr, graph: &Graph) {
    for document in &graph.documents {
        let camera_state = document.states.iter().find(|state| state.kind == "Camera");
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
        let active_view = document
            .states
            .iter()
            .find(|state| state.kind == "ActiveView")
            .and_then(|state| state.attributes.get("name"))
            .cloned()
            .or_else(|| document.attributes.get("active").cloned());
        ir.model.presentation_documents.push(PresentationDocument {
            id: PresentationId("fcstd:presentation:document#0".into()),
            schema_version: document.schema_version,
            active_view,
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
        let property_value = |name: &str| {
            owned
                .iter()
                .find(|property| property.name == name)
                .and_then(|property| gui_property_value(property))
        };
        ir.model.view_presentations.push(ViewPresentation {
            id: PresentationId(crate::native::model_id(
                "presentation-view",
                &provider.id,
                "state",
            )),
            object: provider.object.clone(),
            order: provider.order as u32,
            expanded: provider.expanded,
            visible: property_value("Visibility").and_then(parse_bool),
            display_mode: property_value("DisplayMode").map(str::to_owned),
            selection_style: property_value("SelectionStyle").map(str::to_owned),
            line_width: property_value("LineWidth").and_then(|value| value.parse().ok()),
            point_size: property_value("PointSize").and_then(|value| value.parse().ok()),
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
        visual_guid: None,
        physical_token: None,
        schema: Some("FCStd ViewProvider line style".into()),
        category: None,
        base_color: Some(decode_color(packed_color, None)),
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
        visual_guid: None,
        physical_token: None,
        schema: Some("FCStd ViewProvider point style".into()),
        category: None,
        base_color: Some(decode_color(packed_color, None)),
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
    let Some(container) = provider
        .children()
        .find(|node| node.has_tag_name("Properties"))
    else {
        return Err(CodecError::Malformed(format!(
            "ViewProvider {name} has no Properties"
        )));
    };
    for (property_order, property) in container
        .children()
        .filter(|node| node.has_tag_name("Property"))
        .enumerate()
    {
        let property_name = property.attribute("name").ok_or_else(|| {
            CodecError::Malformed(format!("ViewProvider {name} property has no name"))
        })?;
        let type_name = property.attribute("type").ok_or_else(|| {
            CodecError::Malformed(format!("ViewProvider {name}.{property_name} has no type"))
        })?;
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
            .filter(|(attribute, _)| matches!(attribute.as_str(), "file" | "File"))
            .map(|(_, value)| value.clone())
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
    entries: &BTreeMap<String, Vec<u8>>,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
    kind: TopologyColorKind,
) -> Result<(), CodecError> {
    let bytes = entries.get(entry_name).ok_or_else(|| {
        CodecError::Malformed(format!(
            "DiffuseColor references missing entry {entry_name}"
        ))
    })?;
    if bytes.len() < 4 {
        return Err(CodecError::Malformed(format!(
            "DiffuseColor entry {entry_name} is truncated"
        )));
    }
    let count = u32::from_le_bytes(bytes[..4].try_into().expect("four-byte slice")) as usize;
    let expected = 4_usize
        .checked_add(count.checked_mul(4).ok_or_else(|| {
            CodecError::Malformed(format!("DiffuseColor entry {entry_name} count overflows"))
        })?)
        .ok_or_else(|| CodecError::Malformed("DiffuseColor length overflows".into()))?;
    if bytes.len() != expected {
        return Err(CodecError::Malformed(format!(
            "DiffuseColor entry {entry_name} declares {count} colors but has {} bytes",
            bytes.len()
        )));
    }
    let shape_properties = properties
        .iter()
        .filter(|property| property.owner == object_id)
        .filter(|property| {
            payloads
                .iter()
                .any(|payload| payload.property == property.id)
        })
        .map(|property| property.id.as_str())
        .collect::<Vec<_>>();
    let Some(group) = element_maps
        .iter()
        .find(|map| shape_properties.contains(&map.property.as_str()))
        .and_then(|map| map.maps.last())
        .and_then(|map| {
            map.groups
                .iter()
                .find(|group| group.indexed_name == kind.name())
        })
    else {
        return Ok(());
    };
    // FreeCAD uses a single list entry as a uniform color for every mapped subelement.
    if group.names.is_empty() {
        return Ok(());
    }
    if count != 1 && group.names.len() != count {
        return Err(CodecError::Malformed(format!(
            "{provider_name} {} color count {count} does not match {} mapped subelements",
            kind.name(),
            group.names.len()
        )));
    }
    for (index, bytes) in bytes[4..].chunks_exact(4).enumerate() {
        let packed = u32::from_le_bytes(bytes.try_into().expect("four-byte color"));
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
            .then_some(&group.names[index])
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
                    visual_guid: None,
                    physical_token: None,
                    schema: Some(kind.schema().into()),
                    category: None,
                    base_color: Some(decode_color(packed, None)),
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

#[cfg(test)]
mod color_tests {
    use super::decode_color;

    #[test]
    fn packed_alpha_is_used_without_a_transparency_property() {
        let color = decode_color(0x11223340, None);
        assert!((color.a - 64.0 / 255.0).abs() < f32::EPSILON);
    }

    #[test]
    fn transparency_property_overrides_packed_alpha() {
        let color = decode_color(0x11223300, Some(0.25));
        assert!((color.a - 0.75).abs() < f32::EPSILON);
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}
