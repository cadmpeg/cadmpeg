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
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::SourceProvenance;

use crate::brep::ShapePayloadRecord;
use crate::dialect::{classify_gui_schema, GuiSchemaAdmission};
use crate::loss::FreecadLossCode;
use crate::native::{
    ElementMapGroup, ElementMapRecord, GuiDocumentRecord, GuiPropertyRecord, GuiStateRecord,
    GuiViewProviderRecord, ObjectRecord, PropertyRecord, ValueRecord,
};

#[derive(Default)]
pub(crate) struct Graph {
    pub(crate) documents: Vec<GuiDocumentRecord>,
    pub(crate) providers: Vec<GuiViewProviderRecord>,
    pub(crate) properties: Vec<GuiPropertyRecord>,
    pub(crate) losses: Vec<LossNote>,
}

#[derive(Default)]
struct AppearancePlan {
    body_updates: Vec<BodyUpdate>,
    appearances: Vec<Appearance>,
    bindings: Vec<AppearanceBinding>,
    remove_appearances: HashSet<AppearanceId>,
    presentation_documents: Vec<PresentationDocument>,
    view_presentations: Vec<ViewPresentation>,
}

struct BodyUpdate {
    id: cadmpeg_ir::ids::BodyId,
    visible: Assignment<Option<bool>>,
    color: Assignment<Option<Color>>,
}

enum Assignment<T> {
    Keep,
    Set(T),
}

impl AppearancePlan {
    fn apply(self, ir: &mut CadIr) {
        for update in self.body_updates {
            if let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == update.id) {
                if let Assignment::Set(visible) = update.visible {
                    body.visible = visible;
                }
                if let Assignment::Set(color) = update.color {
                    body.color = color;
                }
            }
        }
        ir.model
            .appearance_bindings
            .retain(|binding| !self.remove_appearances.contains(&binding.appearance));
        ir.model
            .appearances
            .retain(|appearance| !self.remove_appearances.contains(&appearance.id));
        ir.model.appearances.extend(self.appearances);
        ir.model.appearance_bindings.extend(self.bindings);
        ir.model
            .presentation_documents
            .extend(self.presentation_documents);
        ir.model.view_presentations.extend(self.view_presentations);
    }
}

struct CameraSettings {
    position: Option<[f64; 3]>,
    orientation: Option<[f64; 4]>,
}

/// Whether the shared application-property registry knows this GUI property.
pub(crate) fn has_registered_property_grammar(property_name: &str, type_name: &str) -> bool {
    gui_value_tag(type_name).is_some()
        || is_gui_link_type(type_name)
        || is_gui_custom_type(type_name)
        || is_visual_layer_list(property_name, type_name)
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
        .map_err(|error| CodecError::malformed(format_args!("invalid GuiDocument.xml: {error}")))?;
    let schema_version = declared_schema_version(xml.root_element())?;
    let GuiSchemaAdmission::Unverified { declaration } = classify_gui_schema(schema_version) else {
        let (graph, plan) = transfer_schema_one(
            ir,
            text,
            &xml,
            entries,
            objects,
            properties,
            payloads,
            element_maps,
            requires_alpha_conversion,
        )?;
        plan.apply(ir);
        return Ok(graph);
    };

    match transfer_schema_one(
        ir,
        text,
        &xml,
        entries,
        objects,
        properties,
        payloads,
        element_maps,
        requires_alpha_conversion,
    ) {
        Ok((mut graph, plan)) => {
            plan.apply(ir);
            graph.losses.push(FreecadLossCode::SourceGuiSchemaUnverified.note(format!(
                "GuiDocument.xml declares schema {declaration}; decoded with the schema-1 vocabulary"
            )));
            Ok(graph)
        }
        Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })) => Ok(Graph {
            losses: vec![FreecadLossCode::SourceGuiSchemaUnverified.note(format!(
                "GuiDocument.xml could not be decoded with the schema-1 vocabulary; declared schema {declaration} is the probable cause: {error}"
            ))],
            ..Graph::default()
        }),
        Err(error) => Err(error),
    }
}

fn declared_schema_version(root: roxmltree::Node<'_, '_>) -> Result<Option<u32>, CodecError> {
    crate::container::canonical_attribute(root, "SchemaVersion", "schemaVersion")?
        .map(|value| {
            value.parse::<u32>().map_err(|_| {
                CodecError::Malformed("GuiDocument.xml SchemaVersion is not an integer".into())
            })
        })
        .transpose()
}

#[allow(clippy::too_many_arguments)]
fn transfer_schema_one(
    ir: &CadIr,
    text: &str,
    xml: &roxmltree::Document<'_>,
    entries: &BTreeMap<String, View<'_>>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
    requires_alpha_conversion: bool,
) -> Result<(Graph, AppearancePlan), CodecError> {
    let root = xml.root_element();
    let schema_version = declared_schema_version(root)?;
    let mut plan = AppearancePlan::default();
    let camera_count = root
        .children()
        .filter(|node| node.has_tag_name("Camera"))
        .count();
    let camera_error = if camera_count == 1 {
        None
    } else {
        Some(format!(
            "GuiDocument.xml schema 1 requires one Camera record, found {camera_count}"
        ))
    };
    if let Some(message) = camera_error {
        return Err(CodecError::Malformed(message));
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
    let mut losses = Vec::new();
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
            return Err(CodecError::malformed(format_args!(
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
            CodecError::malformed(format_args!("ViewProvider {name} has no Properties"))
        })?;
        let property_nodes = properties_node
            .children()
            .filter(|node| node.has_tag_name("Property"))
            .collect::<Vec<_>>();
        let values = property_nodes
            .iter()
            .copied()
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
        let property_provenance = |property_name: &str, type_name: &str| SourceProvenance {
            format: "fcstd".into(),
            stream: "GuiDocument.xml".into(),
            offset: property_nodes
                .iter()
                .find(|property| {
                    property.attribute("name") == Some(property_name)
                        && property.attribute("type") == Some(type_name)
                })
                .map_or(0, |property| property.range().start as u64),
            tag: Some(format!("ViewProvider {name} property {property_name}")),
        };
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
            plan.body_updates.push(BodyUpdate {
                id: body_id.clone(),
                visible: Assignment::Set(visibility),
                color: Assignment::Set(
                    packed_color.map(|packed| decode_color(packed, transparency)),
                ),
            });
        }
        if let Some(file) = values
            .get("DiffuseColor")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                &mut plan,
                name,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Face,
                requires_alpha_conversion,
                property_provenance("DiffuseColor", "App::PropertyColorList"),
                &mut losses,
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
            transfer_edge_appearance(
                ir,
                &mut plan,
                name,
                object_id,
                color,
                width,
                &payload_prefixes,
            );
        }
        if let Some(file) = values
            .get("LineColorArray")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                &mut plan,
                name,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Edge,
                requires_alpha_conversion,
                property_provenance("LineColorArray", "App::PropertyColorList"),
                &mut losses,
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
            transfer_vertex_appearance(
                ir,
                &mut plan,
                name,
                object_id,
                color,
                size,
                &payload_prefixes,
            );
        }
        if let Some(file) = values
            .get("PointColorArray")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                &mut plan,
                name,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Vertex,
                requires_alpha_conversion,
                property_provenance("PointColorArray", "App::PropertyColorList"),
                &mut losses,
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
        plan.appearances.push(Appearance {
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
            plan.bindings.push(AppearanceBinding {
                id: format!("fcstd:appearance:binding#{name}:{index}"),
                target: AppearanceTarget::Body(body),
                appearance: appearance_id.clone(),
                source_entity_id: Some(object_id.to_owned()),
                object_type: Some("ViewProvider".into()),
                visible: None,
                channels: BTreeMap::new(),
            });
        }
    }
    let mut graph = Graph {
        documents: vec![document],
        providers: native_providers,
        properties: native_properties,
        losses,
    };
    let material_lists =
        validate_gui_list_payloads(&graph.properties, entries, requires_alpha_conversion)?;
    let mut material_losses = Vec::new();
    transfer_shape_appearances(
        ir,
        &mut plan,
        &graph,
        &material_lists,
        properties,
        payloads,
        element_maps,
        &mut material_losses,
    )?;
    graph.losses.extend(material_losses);
    transfer_neutral_presentation(&mut plan, &graph)?;
    Ok((graph, plan))
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

fn transfer_neutral_presentation(
    plan: &mut AppearancePlan,
    graph: &Graph,
) -> Result<(), CodecError> {
    for document in &graph.documents {
        let mut camera_states = document
            .states
            .iter()
            .filter(|state| state.kind == "Camera");
        let camera_state = camera_states
            .next()
            .filter(|_| camera_states.next().is_none());
        let camera = camera_state.map(camera_state_value).transpose()?;
        plan.presentation_documents.push(PresentationDocument {
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
            return Err(CodecError::malformed(format_args!(
                "ViewProvider {} has a negative line or point size",
                provider.name
            )));
        }
        plan.view_presentations.push(ViewPresentation {
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

fn camera_state_value(state: &GuiStateRecord) -> Result<CameraState, CodecError> {
    let settings = state
        .attributes
        .get("settings")
        .ok_or_else(|| CodecError::Malformed("GUI Camera has no settings attribute".into()))?;
    let CameraSettings {
        position,
        orientation,
    } = parse_camera_settings(settings)?;
    if position.is_some_and(|value| value.iter().any(|component| !component.is_finite())) {
        return Err(CodecError::Malformed(
            "GUI camera settings position contains a non-finite component".into(),
        ));
    }
    if orientation.is_some_and(|value| value.iter().any(|component| !component.is_finite())) {
        return Err(CodecError::Malformed(
            "GUI camera settings orientation contains a non-finite component".into(),
        ));
    }
    if orientation.is_some_and(|value| value.iter().all(|component| *component == 0.0)) {
        return Err(CodecError::Malformed(
            "GUI camera settings orientation must be nonzero".into(),
        ));
    }
    Ok(CameraState {
        position,
        orientation,
        properties: state.attributes.clone(),
    })
}

fn parse_camera_settings(settings: &str) -> Result<CameraSettings, CodecError> {
    if settings.trim().is_empty() {
        return Ok(CameraSettings {
            position: None,
            orientation: None,
        });
    }

    let tokens = settings.split_whitespace().collect::<Vec<_>>();
    let valid_shape = tokens.len() >= 3
        && tokens[1] == "{"
        && tokens.last() == Some(&"}")
        && matches!(tokens[0], "OrthographicCamera" | "PerspectiveCamera");
    if !valid_shape {
        return Err(CodecError::Malformed(
            "GUI camera settings are not an Inventor camera node".into(),
        ));
    }

    let end = tokens.len() - 1;
    let mut position = None;
    let mut orientation = None;
    let mut index = 2;
    while index < end {
        match tokens[index] {
            "position" => {
                if position.is_some() {
                    return Err(CodecError::Malformed(
                        "GUI camera settings have multiple position fields".into(),
                    ));
                }
                position = Some(camera_field::<3>(&tokens, index + 1, end, "position")?);
                index += 4;
            }
            "orientation" => {
                if orientation.is_some() {
                    return Err(CodecError::Malformed(
                        "GUI camera settings have multiple orientation fields".into(),
                    ));
                }
                orientation = Some(camera_field::<4>(&tokens, index + 1, end, "orientation")?);
                index += 5;
            }
            _ => index += 1,
        }
    }
    Ok(CameraSettings {
        position,
        orientation,
    })
}

fn camera_field<const N: usize>(
    tokens: &[&str],
    start: usize,
    end: usize,
    field: &str,
) -> Result<[f64; N], CodecError> {
    let values = tokens
        .get(start..start.saturating_add(N))
        .filter(|values| values.len() == N && start.saturating_add(N) <= end)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("GUI camera {field} field is incomplete"))
        })?
        .iter()
        .map(|value| {
            value.parse::<f64>().map_err(|_| {
                CodecError::malformed(format_args!("GUI camera {field} field is not numeric"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    values.try_into().map_err(|_| {
        CodecError::malformed(format_args!(
            "GUI camera {field} field has the wrong cardinality"
        ))
    })
}

fn transfer_edge_appearance(
    ir: &CadIr,
    plan: &mut AppearancePlan,
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
    plan.appearances.push(Appearance {
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
        plan.bindings.push(AppearanceBinding {
            id: format!("fcstd:appearance:binding#edge:{provider_name}:{index}"),
            target: AppearanceTarget::Edge(edge),
            appearance: appearance_id.clone(),
            source_entity_id: Some(object_id.to_owned()),
            object_type: Some("ViewProvider Edge".into()),
            visible: None,
            channels: [("precedence".into(), "edge_over_object".into())].into(),
        });
    }
}

fn transfer_vertex_appearance(
    ir: &CadIr,
    plan: &mut AppearancePlan,
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
    plan.appearances.push(Appearance {
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
        plan.bindings.push(AppearanceBinding {
            id: format!("fcstd:appearance:binding#vertex:{provider_name}:{index}"),
            target: AppearanceTarget::Vertex(vertex),
            appearance: appearance_id.clone(),
            source_entity_id: Some(object_id.to_owned()),
            object_type: Some("ViewProvider Vertex".into()),
            visible: None,
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
        return Err(CodecError::malformed(format_args!(
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
            CodecError::malformed(format_args!(
                "ViewProvider {name} has invalid property count"
            ))
        })?;
    if declared != property_nodes.len() {
        return Err(CodecError::malformed(format_args!(
            "ViewProvider {name} declares {declared} properties but contains {}",
            property_nodes.len()
        )));
    }
    let mut property_names = HashSet::new();
    for (property_order, property) in property_nodes.into_iter().enumerate() {
        let property_name = property.attribute("name").ok_or_else(|| {
            CodecError::malformed(format_args!("ViewProvider {name} property has no name"))
        })?;
        if !property_names.insert(property_name) {
            return Err(CodecError::Malformed(
                "ViewProvider has duplicate property names".into(),
            ));
        }
        let type_name = property.attribute("type").ok_or_else(|| {
            CodecError::malformed(format_args!(
                "ViewProvider {name}.{property_name} has no type"
            ))
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
    if is_visual_layer_list(property_name, type_name) {
        return validate_visual_layer_list(property, property_name);
    }
    if is_gui_link_type(type_name) {
        return crate::persistence::validate_link_property(property, type_name);
    }
    match type_name {
        "Mesh::PropertyMeshKernel" => {
            return validate_gui_geometry_value(property, property_name, "Mesh");
        }
        "Points::PropertyPointKernel" => {
            return validate_gui_geometry_value(property, property_name, "Points");
        }
        "TechDraw::PropertyGeomFormatList" => {
            return validate_gui_geom_format_list(property, property_name);
        }
        "TechDraw::PropertyCosmeticVertexList" => {
            return validate_gui_cosmetic_vertex_list(property, property_name);
        }
        "TechDraw::PropertyCosmeticEdgeList" => {
            return validate_gui_cosmetic_edge_list(property, property_name);
        }
        "TechDraw::PropertyCenterLineList" => {
            return validate_gui_center_line_list(property, property_name);
        }
        "App::PropertyExpressionEngine" => {
            return validate_gui_expression_engine(property, property_name);
        }
        "Materials::PropertyMaterial" => {
            return validate_gui_material_reference(property, property_name);
        }
        "Part::PropertyPartShape" => {
            return validate_gui_part_shape(property, property_name);
        }
        "Part::PropertyGeometryList" => {
            return validate_gui_geometry_list(property, property_name);
        }
        "Part::PropertyFilletEdges" => {
            return validate_gui_filletedges(property, property_name);
        }
        "Part::PropertyTopoShapeList" => {
            return validate_gui_shape_list(property, property_name);
        }
        "Sketcher::PropertyConstraintList" => {
            return validate_gui_constraint_list(property, property_name);
        }
        "Part::PropertyShapeHistory" | "Part::PropertyShapeCache" => return Ok(()),
        _ => {}
    }
    let Some(expected_tag) = gui_value_tag(type_name) else {
        return Ok(());
    };
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let root = roots.first().copied().ok_or_else(|| {
        CodecError::malformed(format_args!(
            "GUI property {property_name} requires one {expected_tag} value"
        ))
    })?;
    if !root.has_tag_name(expected_tag) {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires a leading {expected_tag} value"
        )));
    }
    let scalar = |attribute: &str| {
        root.attribute(attribute).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "GUI property {property_name} {expected_tag} has no {attribute} attribute"
            ))
        })
    };
    match expected_tag {
        "Bool" => {
            if parse_bool(scalar("value")?).is_none() {
                return Err(CodecError::malformed(format_args!(
                    "GUI property {property_name} has an invalid Boolean"
                )));
            }
        }
        "Integer" => {
            scalar("value")?.parse::<i64>().map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} has an invalid integer"
                ))
            })?;
            if is_gui_integer_constraint_type(type_name) {
                validate_gui_constraint_attributes(root, property_name, true)?;
            }
            if type_name == "App::PropertyEnumeration" {
                validate_gui_enumeration(&roots, property_name)?;
                return Ok(());
            }
        }
        "Float" => {
            let value = scalar("value")?.parse::<f64>().map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} has an invalid float"
                ))
            })?;
            if !value.is_finite() {
                return Err(CodecError::malformed(format_args!(
                    "GUI property {property_name} has a non-finite float"
                )));
            }
            if is_gui_float_constraint_type(type_name) {
                validate_gui_constraint_attributes(root, property_name, false)?;
            }
        }
        "String" | "Python" | "ColorList" | "MaterialList" => {
            let attribute = if matches!(expected_tag, "ColorList" | "MaterialList") {
                "file"
            } else {
                "value"
            };
            scalar(attribute)?;
            if matches!(expected_tag, "ColorList" | "MaterialList") && has_nested_gui_elements(root)
            {
                return Err(gui_nested_value_error(property_name, expected_tag));
            }
            if type_name == "App::PropertyPersistentObject" {
                if roots.len() != 2 || !roots[1].has_tag_name("PersistentObject") {
                    return Err(CodecError::malformed(format_args!(
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
                        CodecError::malformed(format_args!(
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
                CodecError::malformed(format_args!(
                    "GUI property {property_name} has an invalid color"
                ))
            })?;
        }
        "PropertyVector" => {
            for attribute in ["valueX", "valueY", "valueZ"] {
                let value = scalar(attribute)?.parse::<f64>().map_err(|_| {
                    CodecError::malformed(format_args!(
                        "GUI property {property_name} has an invalid vector"
                    ))
                })?;
                if !value.is_finite() {
                    return Err(CodecError::malformed(format_args!(
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
                return Err(CodecError::malformed(format_args!(
                    "GUI property {property_name} has an invalid Boolean list"
                )));
            }
            if has_nested_gui_elements(root) {
                return Err(gui_nested_value_error(property_name, "BoolList"));
            }
        }
        "StringList" => validate_gui_string_list(root, property_name)?,
        "IntegerList" => validate_gui_integer_list(root, property_name, false)?,
        "IntegerSet" => validate_gui_integer_list(root, property_name, true)?,
        "Map" => validate_gui_map(root, property_name)?,
        "PropertyMatrix" => {
            for row in 1..=4 {
                for column in 1..=4 {
                    let attribute = format!("a{row}{column}");
                    let value = scalar(&attribute)?.parse::<f64>().map_err(|_| {
                        CodecError::malformed(format_args!(
                            "GUI property {property_name} has an invalid matrix value"
                        ))
                    })?;
                    if !value.is_finite() {
                        return Err(CodecError::malformed(format_args!(
                            "GUI property {property_name} has a non-finite matrix value"
                        )));
                    }
                }
            }
        }
        "PropertyPlacement" => validate_gui_placement(root, property_name)?,
        "PropertyRotation" => {
            for attribute in ["A", "Ox", "Oy", "Oz"] {
                let value = scalar(attribute)?.parse::<f64>().map_err(|_| {
                    CodecError::malformed(format_args!(
                        "GUI property {property_name} has an invalid rotation"
                    ))
                })?;
                if !value.is_finite() {
                    return Err(CodecError::malformed(format_args!(
                        "GUI property {property_name} has a non-finite rotation"
                    )));
                }
            }
        }
        "Uuid" | "Path" => {
            scalar("value")?;
        }
        "FloatList" | "VectorList" | "PlacementList" => {
            scalar("file")?;
            if has_nested_gui_elements(root) {
                return Err(gui_nested_value_error(property_name, expected_tag));
            }
        }
        "FileIncluded" => {
            let has_file = root.attribute("file").is_some();
            let has_data = root.attribute("data").is_some();
            if has_file == has_data {
                let message = format!(
                    "GUI property {property_name} FileIncluded requires exactly one file or data attribute"
                );
                return Err(CodecError::Malformed(message));
            }
            if has_nested_gui_elements(root) {
                return Err(gui_nested_value_error(property_name, "FileIncluded"));
            }
        }
        _ => unreachable!("closed GUI value-tag registry"),
    }
    if roots.len() != 1 {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires exactly one {expected_tag} value"
        )));
    }
    Ok(())
}

fn validate_gui_string_list(
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let count = gui_list_count(root, property_name, "StringList")?;
    let values = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if values.len() != count
        || values
            .iter()
            .any(|value| !value.has_tag_name("String") || value.attribute("value").is_none())
    {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} StringList count or value is invalid"
        )));
    }
    if values.iter().any(|value| has_nested_gui_elements(*value)) {
        return Err(gui_nested_value_error(property_name, "StringList value"));
    }
    Ok(())
}

fn validate_gui_integer_list(
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
    require_sorted_unique: bool,
) -> Result<(), CodecError> {
    let tag = if require_sorted_unique {
        "IntegerSet"
    } else {
        "IntegerList"
    };
    let count = gui_list_count(root, property_name, tag)?;
    let values = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if values.len() != count || values.iter().any(|value| !value.has_tag_name("I")) {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} {tag} count or value is invalid"
        )));
    }
    if values.iter().any(|value| has_nested_gui_elements(*value)) {
        return Err(gui_nested_value_error(property_name, tag));
    }
    let mut previous = None;
    for value in values {
        let number = value
            .attribute("v")
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} {tag} value has no v attribute"
                ))
            })?
            .parse::<i64>()
            .map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} {tag} value is not an integer"
                ))
            })?;
        if require_sorted_unique && previous.is_some_and(|previous| number <= previous) {
            return Err(CodecError::malformed(format_args!(
                "GUI property {property_name} IntegerSet is not sorted and unique"
            )));
        }
        previous = Some(number);
    }
    Ok(())
}

fn validate_gui_map(root: roxmltree::Node<'_, '_>, property_name: &str) -> Result<(), CodecError> {
    let count = gui_list_count(root, property_name, "Map")?;
    let values = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if values.len() != count || values.iter().any(|value| !value.has_tag_name("Item")) {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} Map count or item tag is invalid"
        )));
    }
    if values.iter().any(|value| has_nested_gui_elements(*value)) {
        return Err(gui_nested_value_error(property_name, "Map item"));
    }
    let mut previous_key = None;
    for value in values {
        let key = value.attribute("key").ok_or_else(|| {
            CodecError::malformed(format_args!(
                "GUI property {property_name} Map item has no key"
            ))
        })?;
        if value.attribute("value").is_none() {
            return Err(CodecError::malformed(format_args!(
                "GUI property {property_name} Map item has no value"
            )));
        }
        if previous_key.is_some_and(|previous| key <= previous) {
            return Err(CodecError::malformed(format_args!(
                "GUI property {property_name} Map keys are not sorted and unique"
            )));
        }
        previous_key = Some(key);
    }
    Ok(())
}

fn gui_list_count(
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
    tag: &str,
) -> Result<usize, CodecError> {
    root.attribute("count")
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "GUI property {property_name} {tag} has no count"
            ))
        })?
        .parse::<usize>()
        .map_err(|_| {
            CodecError::malformed(format_args!(
                "GUI property {property_name} {tag} has an invalid count"
            ))
        })
}

fn has_nested_gui_elements(node: roxmltree::Node<'_, '_>) -> bool {
    node.children().any(|child| child.is_element())
}

fn gui_nested_value_error(property_name: &str, value_name: &str) -> CodecError {
    let message = format!("GUI property {property_name} {value_name} has nested element values");
    CodecError::Malformed(message)
}

fn validate_gui_constraint_attributes(
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
    integer: bool,
) -> Result<(), CodecError> {
    for attribute in ["min", "max", "step"] {
        let Some(value) = root.attribute(attribute) else {
            continue;
        };
        if integer {
            value.parse::<i64>().map_err(|_| {
                gui_constraint_error(property_name, "an invalid integer", attribute)
            })?;
        } else {
            let value = value
                .parse::<f64>()
                .map_err(|_| gui_constraint_error(property_name, "an invalid float", attribute))?;
            if !value.is_finite() {
                return Err(gui_constraint_error(
                    property_name,
                    "a non-finite",
                    attribute,
                ));
            }
        }
    }
    Ok(())
}

fn gui_constraint_error(property_name: &str, detail: &str, attribute: &str) -> CodecError {
    let message = format!("GUI property {property_name} has {detail} {attribute}");
    CodecError::Malformed(message)
}

fn validate_gui_placement(
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    for attribute in ["Px", "Py", "Pz"] {
        let value = root
            .attribute(attribute)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} placement has no {attribute}"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} placement has an invalid {attribute}"
                ))
            })?;
        if !value.is_finite() {
            return Err(CodecError::malformed(format_args!(
                "GUI property {property_name} placement has a non-finite {attribute}"
            )));
        }
    }
    let axis_attributes = ["A", "Ox", "Oy", "Oz"];
    let quaternion_attributes = ["Q0", "Q1", "Q2", "Q3"];
    let has_axis = root.attribute("A").is_some();
    let orientation = if has_axis {
        &axis_attributes[..]
    } else {
        &quaternion_attributes[..]
    };
    for &attribute in orientation {
        let value = root
            .attribute(attribute)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} placement has no {attribute}"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} placement has an invalid {attribute}"
                ))
            })?;
        if !value.is_finite() {
            return Err(CodecError::malformed(format_args!(
                "GUI property {property_name} placement has a non-finite {attribute}"
            )));
        }
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
        return Err(CodecError::malformed(format_args!(
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
            CodecError::malformed(format_args!(
                "GUI property {property_name} has an invalid custom enumeration count"
            ))
        })?;
    if values.len() != count
        || values
            .iter()
            .any(|value| !value.has_tag_name("Enum") || value.attribute("value").is_none())
    {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} custom enumeration count or value is invalid"
        )));
    }
    Ok(())
}

fn gui_value_tag(type_name: &str) -> Option<&'static str> {
    if GUI_QUANTITY_TYPES.contains(&type_name) {
        return Some("Float");
    }
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
        | "App::PropertyLength"
        | "App::PropertyPrecision" => "Float",
        "App::PropertyFile"
        | "App::PropertyFont"
        | "App::PropertyPersistentObject"
        | "App::PropertyString" => "String",
        "App::PropertyColor" => "PropertyColor",
        "App::PropertyColorList" => "ColorList",
        "App::PropertyMaterial" => "PropertyMaterial",
        "App::PropertyMaterialList" => "MaterialList",
        "App::PropertyVector"
        | "App::PropertyVectorDistance"
        | "App::PropertyPosition"
        | "App::PropertyDirection" => "PropertyVector",
        "App::PropertyBoolList" => "BoolList",
        "App::PropertyFloatList" => "FloatList",
        "App::PropertyIntegerList" => "IntegerList",
        "App::PropertyIntegerSet" => "IntegerSet",
        "App::PropertyStringList" => "StringList",
        "App::PropertyMap" => "Map",
        "App::PropertyMatrix" => "PropertyMatrix",
        "App::PropertyPath" => "Path",
        "App::PropertyPlacement" => "PropertyPlacement",
        "App::PropertyPlacementList" => "PlacementList",
        "App::PropertyPythonObject" => "Python",
        "App::PropertyRotation" => "PropertyRotation",
        "App::PropertyUUID" => "Uuid",
        "App::PropertyVectorList" => "VectorList",
        "App::PropertyFileIncluded" => "FileIncluded",
        _ => return None,
    };
    Some(tag)
}

fn is_gui_integer_constraint_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyIntegerConstraint" | "App::PropertyPercent"
    )
}

fn is_gui_float_constraint_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyAngle"
            | "App::PropertyArea"
            | "App::PropertyFloatConstraint"
            | "App::PropertyLength"
            | "App::PropertyPrecision"
            | "App::PropertyQuantityConstraint"
            | "App::PropertyVolume"
    )
}

const GUI_QUANTITY_TYPES: &[&str] = &[
    "App::PropertyAcceleration",
    "App::PropertyAmountOfSubstance",
    "App::PropertyAngle",
    "App::PropertyArea",
    "App::PropertyCompressiveStrength",
    "App::PropertyCurrentDensity",
    "App::PropertyDensity",
    "App::PropertyDissipationRate",
    "App::PropertyDistance",
    "App::PropertyDynamicViscosity",
    "App::PropertyElectricalCapacitance",
    "App::PropertyElectricalConductance",
    "App::PropertyElectricalConductivity",
    "App::PropertyElectricalInductance",
    "App::PropertyElectricalResistance",
    "App::PropertyElectricCharge",
    "App::PropertySurfaceChargeDensity",
    "App::PropertyVolumeChargeDensity",
    "App::PropertyElectricCurrent",
    "App::PropertyElectricPotential",
    "App::PropertyFrequency",
    "App::PropertyForce",
    "App::PropertyHeatFlux",
    "App::PropertyInverseArea",
    "App::PropertyInverseLength",
    "App::PropertyInverseVolume",
    "App::PropertyKinematicViscosity",
    "App::PropertyLength",
    "App::PropertyLuminousIntensity",
    "App::PropertyMagneticFieldStrength",
    "App::PropertyMagneticFlux",
    "App::PropertyMagneticFluxDensity",
    "App::PropertyMagnetization",
    "App::PropertyElectromagneticPotential",
    "App::PropertyMass",
    "App::PropertyMoment",
    "App::PropertyPressure",
    "App::PropertyPower",
    "App::PropertyQuantity",
    "App::PropertyQuantityConstraint",
    "App::PropertyShearModulus",
    "App::PropertySpecificEnergy",
    "App::PropertySpecificHeat",
    "App::PropertySpeed",
    "App::PropertyStiffness",
    "App::PropertyStiffnessDensity",
    "App::PropertyStress",
    "App::PropertyTemperature",
    "App::PropertyThermalConductivity",
    "App::PropertyThermalExpansionCoefficient",
    "App::PropertyThermalTransferCoefficient",
    "App::PropertyTime",
    "App::PropertyUltimateTensileStrength",
    "App::PropertyVacuumPermittivity",
    "App::PropertyVelocity",
    "App::PropertyVolume",
    "App::PropertyVolumeFlowRate",
    "App::PropertyVolumetricThermalExpansionCoefficient",
    "App::PropertyWork",
    "App::PropertyYieldStrength",
    "App::PropertyYoungsModulus",
];

fn is_visual_layer_list(property_name: &str, type_name: &str) -> bool {
    property_name == "VisualLayerList" && type_name == "BadType"
}

fn is_gui_link_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyLink"
            | "App::PropertyLinkChild"
            | "App::PropertyLinkGlobal"
            | "App::PropertyLinkHidden"
            | "App::PropertyLinkSub"
            | "App::PropertyLinkSubChild"
            | "App::PropertyLinkSubGlobal"
            | "App::PropertyLinkSubHidden"
            | "App::PropertyLinkList"
            | "App::PropertyLinkListChild"
            | "App::PropertyLinkListGlobal"
            | "App::PropertyLinkListHidden"
            | "App::PropertyLinkSubList"
            | "App::PropertyLinkSubListChild"
            | "App::PropertyLinkSubListGlobal"
            | "App::PropertyLinkSubListHidden"
            | "App::PropertyXLink"
            | "App::PropertyXLinkSub"
            | "App::PropertyXLinkSubHidden"
            | "App::PropertyXLinkSubList"
            | "App::PropertyXLinkList"
            | "App::PropertyPlacementLink"
    )
}

fn is_gui_custom_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "Materials::PropertyMaterial"
            | "Part::PropertyPartShape"
            | "Part::PropertyGeometryList"
            | "Part::PropertyShapeHistory"
            | "Part::PropertyFilletEdges"
            | "Part::PropertyShapeCache"
            | "Part::PropertyTopoShapeList"
            | "Sketcher::PropertyConstraintList"
    )
}

fn validate_gui_geometry_value(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
    expected_tag: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        let message =
            format!("GUI property {property_name} requires exactly one {expected_tag} value");
        return Err(CodecError::Malformed(message));
    };
    if !root.has_tag_name(expected_tag) {
        let message =
            format!("GUI property {property_name} requires a leading {expected_tag} value");
        return Err(CodecError::Malformed(message));
    }

    let side_references = property
        .descendants()
        .filter(roxmltree::Node::is_element)
        .flat_map(|node| {
            node.attributes()
                .filter(|attribute| matches!(attribute.name(), "file" | "File"))
                .map(move |attribute| (node, attribute.value()))
        })
        .filter(|(_, value)| !value.is_empty())
        .map(|(node, _)| node)
        .collect::<Vec<_>>();
    let direct_file = root
        .attribute("file")
        .is_some_and(|value| !value.is_empty());
    if side_references.len() != usize::from(direct_file)
        || side_references.first().is_some_and(|node| *node != *root)
    {
        let message = format!(
            "GUI property {property_name} {expected_tag} has an unowned side-entry reference"
        );
        return Err(CodecError::Malformed(message));
    }
    if expected_tag == "Points" {
        validate_gui_points_transform(*root, property_name)?;
    }
    Ok(())
}

fn validate_gui_points_transform(
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(text) = root.attribute("mtrx") else {
        return Ok(());
    };
    let values = text
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>();
    let Ok(values) = values else {
        let message =
            format!("GUI property {property_name} Points transform has an invalid scalar");
        return Err(CodecError::Malformed(message));
    };
    if values.len() != 16 || values.iter().any(|value| !value.is_finite()) {
        let message =
            format!("GUI property {property_name} Points transform must contain 16 finite scalars");
        return Err(CodecError::Malformed(message));
    }
    Ok(())
}

fn validate_gui_geom_format_list(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(gui_techdraw_error(
            property_name,
            "requires exactly one GeomFormatList value",
        ));
    };
    if !root.has_tag_name("GeomFormatList") {
        return Err(gui_techdraw_error(
            property_name,
            "requires a leading GeomFormatList value",
        ));
    }
    let count = root
        .attribute("count")
        .ok_or_else(|| gui_techdraw_error(property_name, "GeomFormatList has no count"))?
        .parse::<usize>()
        .map_err(|_| gui_techdraw_error(property_name, "GeomFormatList has an invalid count"))?;
    let records = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if records.len() != count {
        return Err(gui_techdraw_error(
            property_name,
            "GeomFormatList count does not match its records",
        ));
    }
    for record in records {
        if !record.has_tag_name("GeomFormat")
            || record.attribute("type") != Some("TechDraw::GeomFormat")
        {
            return Err(gui_techdraw_error(
                property_name,
                "GeomFormatList has an invalid record type",
            ));
        }
        validate_gui_geom_format_record(record, property_name)?;
    }
    Ok(())
}

fn validate_gui_geom_format_record(
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let fields = record
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if !(5..=6).contains(&fields.len()) {
        return Err(gui_techdraw_error(
            property_name,
            "GeomFormat has an invalid field sequence",
        ));
    }
    for (field, expected_tag) in
        fields
            .iter()
            .zip(["GeomIndex", "Style", "Weight", "Color", "Visible"])
    {
        if !field.has_tag_name(expected_tag) || field.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(
                property_name,
                "GeomFormat has a nested or out-of-order field",
            ));
        }
        if field.attribute("value").is_none() {
            return Err(gui_techdraw_error(
                property_name,
                "GeomFormat field has no value",
            ));
        }
    }
    if let Some(line_number) = fields.get(5) {
        if !(line_number.has_tag_name("LineNumber") || line_number.has_tag_name("ISOLineNumber"))
            || line_number.children().any(|node| node.is_element())
        {
            return Err(gui_techdraw_error(
                property_name,
                "GeomFormat has an invalid line-number field",
            ));
        }
        parse_gui_techdraw_integer(*line_number, property_name)?;
    }
    parse_gui_techdraw_integer(fields[0], property_name)?;
    parse_gui_techdraw_integer(fields[1], property_name)?;
    let weight = fields[2]
        .attribute("value")
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or_else(|| gui_techdraw_error(property_name, "GeomFormat has an invalid weight"))?;
    if !weight.is_finite() {
        return Err(gui_techdraw_error(
            property_name,
            "GeomFormat has a non-finite weight",
        ));
    }
    let Some(color) = fields[3].attribute("value") else {
        return Err(gui_techdraw_error(property_name, "GeomFormat has no color"));
    };
    if !(color.len() == 7 || color.len() == 9)
        || !color.starts_with('#')
        || !color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(gui_techdraw_error(
            property_name,
            "GeomFormat has an invalid color",
        ));
    }
    if parse_bool(fields[4].attribute("value").unwrap_or_default()).is_none() {
        return Err(gui_techdraw_error(
            property_name,
            "GeomFormat has an invalid visibility",
        ));
    }
    Ok(())
}

fn validate_gui_cosmetic_vertex_list(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(gui_techdraw_error(
            property_name,
            "requires exactly one CosmeticVertexList value",
        ));
    };
    if !root.has_tag_name("CosmeticVertexList") {
        return Err(gui_techdraw_error(
            property_name,
            "requires a leading CosmeticVertexList value",
        ));
    }
    let count = root
        .attribute("count")
        .ok_or_else(|| gui_techdraw_error(property_name, "CosmeticVertexList has no count"))?
        .parse::<usize>()
        .map_err(|_| {
            gui_techdraw_error(property_name, "CosmeticVertexList has an invalid count")
        })?;
    let records = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if records.len() != count {
        return Err(gui_techdraw_error(
            property_name,
            "CosmeticVertexList count does not match its records",
        ));
    }
    for record in records {
        if !record.has_tag_name("CosmeticVertex")
            || record.attribute("type") != Some("TechDraw::CosmeticVertex")
        {
            return Err(gui_techdraw_error(
                property_name,
                "CosmeticVertexList has an invalid record type",
            ));
        }
        validate_gui_cosmetic_vertex_record(record, property_name)?;
    }
    Ok(())
}

fn validate_gui_cosmetic_edge_list(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(gui_techdraw_error(
            property_name,
            "requires exactly one CosmeticEdgeList value",
        ));
    };
    if !root.has_tag_name("CosmeticEdgeList") {
        return Err(gui_techdraw_error(
            property_name,
            "requires a leading CosmeticEdgeList value",
        ));
    }
    let count = root
        .attribute("count")
        .ok_or_else(|| gui_techdraw_error(property_name, "CosmeticEdgeList has no count"))?
        .parse::<usize>()
        .map_err(|_| gui_techdraw_error(property_name, "CosmeticEdgeList has an invalid count"))?;
    let records = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if records.len() != count {
        return Err(gui_techdraw_error(
            property_name,
            "CosmeticEdgeList count does not match its records",
        ));
    }
    for record in records {
        if !record.has_tag_name("CosmeticEdge")
            || record.attribute("type") != Some("TechDraw::CosmeticEdge")
        {
            return Err(gui_techdraw_error(
                property_name,
                "CosmeticEdgeList has an invalid record type",
            ));
        }
        validate_gui_cosmetic_edge_record(record, property_name)?;
    }
    Ok(())
}

fn validate_gui_center_line_list(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(gui_techdraw_error(
            property_name,
            "requires exactly one CenterLineList value",
        ));
    };
    if !root.has_tag_name("CenterLineList") {
        return Err(gui_techdraw_error(
            property_name,
            "requires a leading CenterLineList value",
        ));
    }
    let count = root
        .attribute("count")
        .ok_or_else(|| gui_techdraw_error(property_name, "CenterLineList has no count"))?
        .parse::<usize>()
        .map_err(|_| gui_techdraw_error(property_name, "CenterLineList has an invalid count"))?;
    let records = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if records.len() != count {
        return Err(gui_techdraw_error(
            property_name,
            "CenterLineList count does not match its records",
        ));
    }
    for record in records {
        if !record.has_tag_name("CenterLine")
            || record.attribute("type") != Some("TechDraw::CenterLine")
        {
            return Err(gui_techdraw_error(
                property_name,
                "CenterLineList has an invalid record type",
            ));
        }
        validate_gui_center_line_record(record, property_name)?;
    }
    Ok(())
}

fn validate_gui_center_line_record(
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let fields = record
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let prefix = [
        "Start",
        "End",
        "Mode",
        "HShift",
        "VShift",
        "Rotate",
        "Extend",
        "Type",
        "Flip",
        "Faces",
        "Edges",
        "CLPoints",
        "Style",
        "Weight",
        "Color",
        "Visible",
        "GeometryType",
    ];
    if fields.len() < prefix.len() + 10 + 1 {
        return Err(gui_techdraw_error(
            property_name,
            "CenterLine has an incomplete field sequence",
        ));
    }
    for (field, expected_tag) in fields.iter().take(prefix.len()).zip(prefix) {
        if !field.has_tag_name(expected_tag) {
            return Err(gui_techdraw_error(
                property_name,
                "CenterLine has an out-of-order field",
            ));
        }
    }
    for index in [2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 15, 16] {
        if fields[index].children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(
                property_name,
                "CenterLine has a nested field",
            ));
        }
    }
    validate_gui_techdraw_point(fields[0], property_name)?;
    validate_gui_techdraw_point(fields[1], property_name)?;
    let mode = parse_gui_techdraw_integer_value(fields[2], property_name, "Mode")?;
    if !(0..=2).contains(&mode) {
        return Err(gui_techdraw_error(
            property_name,
            "CenterLine has an unsupported Mode",
        ));
    }
    for field in fields.iter().skip(3).take(4) {
        parse_gui_techdraw_finite(*field, property_name)?;
    }
    let line_type = parse_gui_techdraw_integer_value(fields[7], property_name, "Type")?;
    if !(0..=2).contains(&line_type) {
        return Err(gui_techdraw_error(
            property_name,
            "CenterLine has an unsupported Type",
        ));
    }
    validate_gui_techdraw_boolean(fields[8], property_name)?;
    validate_gui_center_line_string_collection(
        fields[9],
        property_name,
        "Faces",
        "FaceCount",
        "Face",
    )?;
    validate_gui_center_line_string_collection(
        fields[10],
        property_name,
        "Edges",
        "EdgeCount",
        "Edge",
    )?;
    validate_gui_center_line_string_collection(
        fields[11],
        property_name,
        "CLPoints",
        "CLPointCount",
        "CLPoint",
    )?;
    parse_gui_techdraw_integer_named(fields[12], property_name, "Style")?;
    parse_gui_techdraw_finite(fields[13], property_name)?;
    validate_gui_techdraw_color(fields[14], property_name)?;
    validate_gui_techdraw_boolean(fields[15], property_name)?;
    let geometry_type =
        parse_gui_techdraw_integer_value(fields[16], property_name, "GeometryType")?;
    validate_gui_techdraw_geometry_branch(
        &fields,
        prefix.len(),
        property_name,
        geometry_type,
        true,
    )?;
    Ok(())
}

fn validate_gui_center_line_string_collection(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    container_tag: &str,
    count_attribute: &str,
    item_tag: &str,
) -> Result<(), CodecError> {
    if !field.has_tag_name(container_tag) {
        return Err(gui_techdraw_error(
            property_name,
            "CenterLine has an invalid collection field",
        ));
    }
    let count = field
        .attribute(count_attribute)
        .ok_or_else(|| gui_techdraw_error(property_name, "CenterLine collection has no count"))?
        .parse::<usize>()
        .map_err(|_| {
            gui_techdraw_error(property_name, "CenterLine collection has an invalid count")
        })?;
    let items = field
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if items.len() != count {
        return Err(gui_techdraw_error(
            property_name,
            "CenterLine collection count does not match its records",
        ));
    }
    for item in items {
        if !item.has_tag_name(item_tag)
            || item.attribute("value").is_none()
            || item.children().any(|node| node.is_element())
        {
            return Err(gui_techdraw_error(
                property_name,
                "CenterLine collection has an invalid item",
            ));
        }
    }
    Ok(())
}

fn validate_gui_cosmetic_edge_record(
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let fields = record
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if fields.len() < 16 {
        return Err(gui_techdraw_error(
            property_name,
            "CosmeticEdge has an incomplete field sequence",
        ));
    }
    let format_fields = ["Style", "Weight", "Color", "Visible", "GeometryType"];
    for (field, expected_tag) in fields.iter().take(format_fields.len()).zip(format_fields) {
        if !field.has_tag_name(expected_tag) || field.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(
                property_name,
                "CosmeticEdge has a nested or out-of-order format field",
            ));
        }
        if field.attribute("value").is_none() {
            return Err(gui_techdraw_error(
                property_name,
                "CosmeticEdge format field has no value",
            ));
        }
    }
    parse_gui_techdraw_integer_named(fields[0], property_name, "Style")?;
    parse_gui_techdraw_finite(fields[1], property_name)?;
    validate_gui_techdraw_color(fields[2], property_name)?;
    validate_gui_techdraw_boolean(fields[3], property_name)?;
    let geometry_type = fields[4]
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(property_name, "CosmeticEdge GeometryType has no value"))?
        .parse::<i64>()
        .map_err(|_| {
            gui_techdraw_error(property_name, "CosmeticEdge GeometryType is not an integer")
        })?;

    validate_gui_techdraw_geometry_branch(
        &fields,
        format_fields.len(),
        property_name,
        geometry_type,
        false,
    )?;
    Ok(())
}

fn validate_gui_techdraw_geometry_branch(
    fields: &[roxmltree::Node<'_, '_>],
    base_start: usize,
    property_name: &str,
    expected_geometry_type: i64,
    allow_iso_line_number: bool,
) -> Result<(), CodecError> {
    if fields.len() < base_start + 10 {
        return Err(gui_techdraw_error(
            property_name,
            "TechDraw geometry has no complete BaseGeom sequence",
        ));
    }
    validate_gui_techdraw_base_geom(
        &fields[base_start..base_start + 10],
        property_name,
        expected_geometry_type,
    )?;
    let mut cursor = base_start + 10;
    let branch_field_count = match expected_geometry_type {
        1 => 2,
        2 => 9,
        7 => 1,
        _ => {
            return Err(gui_techdraw_error(
                property_name,
                "TechDraw geometry has an unsupported GeometryType",
            ));
        }
    };
    let required_fields = cursor + branch_field_count;
    if fields.len() != required_fields && fields.len() != required_fields + 1 {
        return Err(gui_techdraw_error(
            property_name,
            "TechDraw geometry has an invalid branch field sequence",
        ));
    }

    match expected_geometry_type {
        1 => {
            if !fields[cursor].has_tag_name("Center") {
                return Err(gui_techdraw_error(
                    property_name,
                    "TechDraw circle has an invalid center field",
                ));
            }
            validate_gui_techdraw_point(fields[cursor], property_name)?;
            cursor += 1;
            if !fields[cursor].has_tag_name("Radius") {
                return Err(gui_techdraw_error(
                    property_name,
                    "TechDraw circle has an invalid radius field",
                ));
            }
            parse_gui_techdraw_finite(fields[cursor], property_name)?;
            if fields[cursor].children().any(|node| node.is_element()) {
                return Err(gui_techdraw_error(
                    property_name,
                    "TechDraw circle has a nested radius field",
                ));
            }
            cursor += 1;
        }
        2 => {
            if !fields[cursor].has_tag_name("Center") {
                return Err(gui_techdraw_error(
                    property_name,
                    "TechDraw arc has an invalid center field",
                ));
            }
            validate_gui_techdraw_point(fields[cursor], property_name)?;
            cursor += 1;
            if !fields[cursor].has_tag_name("Radius") {
                return Err(gui_techdraw_error(
                    property_name,
                    "TechDraw arc has an invalid radius field",
                ));
            }
            parse_gui_techdraw_finite(fields[cursor], property_name)?;
            if fields[cursor].children().any(|node| node.is_element()) {
                return Err(gui_techdraw_error(
                    property_name,
                    "TechDraw arc has a nested radius field",
                ));
            }
            cursor += 1;
            for expected_tag in ["Start", "End", "Middle"] {
                if !fields[cursor].has_tag_name(expected_tag) {
                    return Err(gui_techdraw_error(
                        property_name,
                        "TechDraw arc has an out-of-order point field",
                    ));
                }
                validate_gui_techdraw_point(fields[cursor], property_name)?;
                cursor += 1;
            }
            for expected_tag in ["StartAngle", "EndAngle"] {
                if !fields[cursor].has_tag_name(expected_tag) {
                    return Err(gui_techdraw_error(
                        property_name,
                        "TechDraw arc has an out-of-order angle field",
                    ));
                }
                parse_gui_techdraw_finite(fields[cursor], property_name)?;
                if fields[cursor].children().any(|node| node.is_element()) {
                    return Err(gui_techdraw_error(
                        property_name,
                        "TechDraw arc has a nested angle field",
                    ));
                }
                cursor += 1;
            }
            for expected_tag in ["Clockwise", "Large"] {
                if !fields[cursor].has_tag_name(expected_tag) {
                    return Err(gui_techdraw_error(
                        property_name,
                        "TechDraw arc has an out-of-order Boolean field",
                    ));
                }
                validate_gui_techdraw_boolean(fields[cursor], property_name)?;
                cursor += 1;
            }
        }
        7 => {
            if !fields[cursor].has_tag_name("Points") {
                return Err(gui_techdraw_error(
                    property_name,
                    "TechDraw generic geometry has no Points field",
                ));
            }
            validate_gui_techdraw_points(fields[cursor], property_name)?;
            cursor += 1;
        }
        _ => unreachable!("unsupported GeometryType checked above"),
    }

    if cursor < fields.len() {
        let line_number = fields[cursor].has_tag_name("LineNumber")
            || (allow_iso_line_number && fields[cursor].has_tag_name("ISOLineNumber"));
        if cursor + 1 != fields.len() || !line_number {
            return Err(gui_techdraw_error(
                property_name,
                "TechDraw geometry has an invalid trailing field",
            ));
        }
        parse_gui_techdraw_integer_named(fields[cursor], property_name, "LineNumber")?;
        if fields[cursor].children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(
                property_name,
                "TechDraw geometry line number is nested",
            ));
        }
    }
    Ok(())
}

fn validate_gui_techdraw_base_geom(
    fields: &[roxmltree::Node<'_, '_>],
    property_name: &str,
    expected_geometry_type: i64,
) -> Result<(), CodecError> {
    let expected = [
        "GeomType",
        "ExtractType",
        "EdgeClass",
        "HLRVisible",
        "Reversed",
        "Ref3D",
        "Cosmetic",
        "Source",
        "SourceIndex",
        "CosmeticTag",
    ];
    for (field, expected_tag) in fields.iter().zip(expected) {
        if !field.has_tag_name(expected_tag) || field.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(
                property_name,
                "TechDraw BaseGeom has a nested or out-of-order field",
            ));
        }
        if field.attribute("value").is_none() {
            return Err(gui_techdraw_error(
                property_name,
                "TechDraw BaseGeom field has no value",
            ));
        }
    }
    let geometry_type = fields[0]
        .attribute("value")
        .expect("validated GeomType value")
        .parse::<i64>()
        .map_err(|_| gui_techdraw_error(property_name, "TechDraw GeomType is not an integer"))?;
    if geometry_type != expected_geometry_type {
        return Err(gui_techdraw_error(
            property_name,
            "TechDraw GeometryType and GeomType disagree",
        ));
    }
    for index in [1, 2, 5, 7, 8] {
        parse_gui_techdraw_integer_named(fields[index], property_name, expected[index])?;
    }
    for index in [3, 4, 6] {
        validate_gui_techdraw_boolean(fields[index], property_name)?;
    }
    Ok(())
}

fn validate_gui_techdraw_points(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let count = field
        .attribute("PointsCount")
        .ok_or_else(|| gui_techdraw_error(property_name, "TechDraw Points has no count"))?
        .parse::<usize>()
        .map_err(|_| gui_techdraw_error(property_name, "TechDraw Points has an invalid count"))?;
    let points = field
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if points.len() != count {
        return Err(gui_techdraw_error(
            property_name,
            "TechDraw Points count does not match its records",
        ));
    }
    for point in points {
        if !point.has_tag_name("Point") || point.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(
                property_name,
                "TechDraw Points has an invalid point record",
            ));
        }
        validate_gui_techdraw_point(point, property_name)?;
    }
    Ok(())
}

fn validate_gui_cosmetic_vertex_record(
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let fields = record
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if !(15..=16).contains(&fields.len()) {
        return Err(gui_techdraw_error(
            property_name,
            "CosmeticVertex has an invalid field sequence",
        ));
    }
    let base_fields = [
        "Point",
        "Extract",
        "HLRVisible",
        "Ref3D",
        "IsCenter",
        "Cosmetic",
        "CosmeticLink",
        "CosmeticTag",
    ];
    for (field, expected_tag) in fields.iter().zip(base_fields) {
        if !field.has_tag_name(expected_tag) {
            break;
        }
        if field.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(
                property_name,
                "CosmeticVertex has a nested field",
            ));
        }
    }
    if fields
        .iter()
        .zip(base_fields)
        .any(|(field, expected_tag)| !field.has_tag_name(expected_tag))
    {
        return Err(gui_techdraw_error(
            property_name,
            "CosmeticVertex has an out-of-order field",
        ));
    }
    let mut cursor = base_fields.len();
    if fields[cursor].has_tag_name("VertexTag") {
        validate_gui_techdraw_uuid(fields[cursor], property_name, "VertexTag")?;
        cursor += 1;
    }
    let tail_fields = [
        "PermaPoint",
        "LinkGeom",
        "Color",
        "Size",
        "Style",
        "Visible",
        "Tag",
    ];
    if fields.len() != cursor + tail_fields.len()
        || fields[cursor..]
            .iter()
            .zip(tail_fields)
            .any(|(field, expected_tag)| !field.has_tag_name(expected_tag))
    {
        return Err(gui_techdraw_error(
            property_name,
            "CosmeticVertex has an out-of-order field",
        ));
    }
    for field in &fields {
        if field.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(
                property_name,
                "CosmeticVertex has a nested field",
            ));
        }
    }
    validate_gui_techdraw_point(fields[0], property_name)?;
    parse_gui_techdraw_integer_named(fields[1], property_name, "Extract")?;
    validate_gui_techdraw_boolean(fields[2], property_name)?;
    parse_gui_techdraw_integer_named(fields[3], property_name, "Ref3D")?;
    validate_gui_techdraw_boolean(fields[4], property_name)?;
    validate_gui_techdraw_boolean(fields[5], property_name)?;
    parse_gui_techdraw_integer_named(fields[6], property_name, "CosmeticLink")?;
    if fields[7].attribute("value").is_none() {
        return Err(gui_techdraw_error(
            property_name,
            "CosmeticVertex CosmeticTag has no value",
        ));
    }
    validate_gui_techdraw_point(fields[cursor], property_name)?;
    parse_gui_techdraw_integer_named(fields[cursor + 1], property_name, "LinkGeom")?;
    validate_gui_techdraw_color(fields[cursor + 2], property_name)?;
    parse_gui_techdraw_finite(fields[cursor + 3], property_name)?;
    parse_gui_techdraw_integer_named(fields[cursor + 4], property_name, "Style")?;
    validate_gui_techdraw_boolean(fields[cursor + 5], property_name)?;
    validate_gui_techdraw_uuid(fields[cursor + 6], property_name, "Tag")?;
    Ok(())
}

fn validate_gui_techdraw_point(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    if field.children().any(|node| node.is_element()) {
        return Err(gui_techdraw_error(
            property_name,
            "TechDraw point has a nested field",
        ));
    }
    for attribute in ["X", "Y", "Z"] {
        let value = field
            .attribute(attribute)
            .ok_or_else(|| gui_techdraw_error(property_name, "point has no coordinate"))?
            .parse::<f64>()
            .map_err(|_| gui_techdraw_error(property_name, "point has an invalid coordinate"))?;
        if !value.is_finite() {
            return Err(gui_techdraw_error(
                property_name,
                "point has a non-finite coordinate",
            ));
        }
    }
    Ok(())
}

fn validate_gui_techdraw_boolean(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let value = field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(property_name, "TechDraw Boolean has no value"))?;
    if parse_bool(value).is_none() {
        return Err(gui_techdraw_error(
            property_name,
            "TechDraw has an invalid Boolean",
        ));
    }
    Ok(())
}

fn validate_gui_techdraw_color(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let color = field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(property_name, "TechDraw color has no value"))?;
    if !(color.len() == 7 || color.len() == 9)
        || !color.starts_with('#')
        || !color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(gui_techdraw_error(
            property_name,
            "TechDraw has an invalid color",
        ));
    }
    Ok(())
}

fn parse_gui_techdraw_finite(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let value = field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(property_name, "TechDraw scalar has no value"))?
        .parse::<f64>()
        .map_err(|_| gui_techdraw_error(property_name, "TechDraw scalar is invalid"))?;
    if !value.is_finite() {
        return Err(gui_techdraw_error(
            property_name,
            "TechDraw scalar is non-finite",
        ));
    }
    Ok(())
}

fn parse_gui_techdraw_integer_named(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    field_name: &str,
) -> Result<(), CodecError> {
    parse_gui_techdraw_integer_value(field, property_name, field_name).map(|_| ())
}

fn parse_gui_techdraw_integer_value(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    field_name: &str,
) -> Result<i64, CodecError> {
    field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(property_name, "TechDraw integer has no value"))?
        .parse::<i64>()
        .map_err(|_| {
            gui_techdraw_error(
                property_name,
                &format!("TechDraw {field_name} integer is invalid"),
            )
        })
}

fn validate_gui_techdraw_uuid(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    field_name: &str,
) -> Result<(), CodecError> {
    let value = field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(property_name, "CosmeticVertex tag has no value"))?;
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|&index| bytes[index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .filter(|(index, _)| ![8, 13, 18, 23].contains(index))
            .all(|(_, byte)| byte.is_ascii_hexdigit());
    if !valid {
        return Err(gui_techdraw_error(
            property_name,
            &format!("CosmeticVertex {field_name} is not a UUID"),
        ));
    }
    Ok(())
}

fn parse_gui_techdraw_integer(
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(property_name, "GeomFormat field has no value"))?
        .parse::<i64>()
        .map(|_| ())
        .map_err(|_| gui_techdraw_error(property_name, "GeomFormat has an invalid integer"))
}

fn gui_techdraw_error(property_name: &str, detail: &str) -> CodecError {
    let message = format!("GUI property {property_name} {detail}");
    CodecError::Malformed(message)
}

fn validate_visual_layer_list(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires exactly one VisualLayerList value"
        )));
    };
    if !root.has_tag_name("VisualLayerList") {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires a leading VisualLayerList value"
        )));
    }
    let count = root
        .attribute("count")
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "GUI property {property_name} VisualLayerList has no count"
            ))
        })?
        .parse::<usize>()
        .map_err(|_| {
            CodecError::malformed(format_args!(
                "GUI property {property_name} VisualLayerList has an invalid count"
            ))
        })?;
    let layers = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if layers.len() != count
        || layers
            .iter()
            .any(|layer| !layer.has_tag_name("VisualLayer"))
    {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} VisualLayerList count or record tag is invalid"
        )));
    }
    for layer in layers {
        if !matches!(layer.attribute("visible"), Some("true" | "false")) {
            return Err(CodecError::malformed(format_args!(
                "GUI property {property_name} VisualLayer has an invalid visible value"
            )));
        }
        layer
            .attribute("linePattern")
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} VisualLayer has no linePattern"
                ))
            })?
            .parse::<u32>()
            .map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} VisualLayer has an invalid linePattern"
                ))
            })?;
        let line_width = layer
            .attribute("lineWidth")
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} VisualLayer has no lineWidth"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} VisualLayer has an invalid lineWidth"
                ))
            })?;
        if !line_width.is_finite() {
            return Err(CodecError::malformed(format_args!(
                "GUI property {property_name} VisualLayer has a non-finite lineWidth"
            )));
        }
    }
    Ok(())
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
                CodecError::malformed(format_args!(
                    "GUI property {property_name} material has no {attribute}"
                ))
            })?
            .parse::<u32>()
            .map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} material has an invalid {attribute}"
                ))
            })?;
    }
    for attribute in ["shininess", "transparency"] {
        let scalar = value
            .attribute(attribute)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} material has no {attribute}"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                CodecError::malformed(format_args!(
                    "GUI property {property_name} material has an invalid {attribute}"
                ))
            })?;
        if !scalar.is_finite() {
            return Err(CodecError::malformed(format_args!(
                "GUI property {property_name} material has a non-finite {attribute}"
            )));
        }
    }
    Ok(())
}

fn validate_gui_expression_engine(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires one ExpressionEngine value"
        )));
    };
    if !root.has_tag_name("ExpressionEngine") {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires a leading ExpressionEngine value"
        )));
    }
    let count = gui_list_count(*root, property_name, "ExpressionEngine")?;
    let expressions = root
        .children()
        .filter(|child| child.is_element() && child.has_tag_name("Expression"))
        .collect::<Vec<_>>();
    if expressions.len() != count
        || expressions.iter().any(|expression| {
            expression.attribute("path").is_none() || expression.attribute("expression").is_none()
        })
    {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} ExpressionEngine count or expression is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_material_reference(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires one PropertyMaterial value"
        )));
    };
    if !root.has_tag_name("PropertyMaterial") || root.attribute("uuid").is_none() {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} material reference is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_part_shape(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if (roots.is_empty() || roots[0].has_tag_name("Part"))
        && roots
            .iter()
            .skip(1)
            .all(|root| root.has_tag_name("ElementMap"))
    {
        return Ok(());
    }
    Err(CodecError::malformed(format_args!(
        "GUI property {property_name} Part shape value is invalid"
    )))
}

fn validate_gui_geometry_list(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires one GeometryList value"
        )));
    };
    if !root.has_tag_name("GeometryList") {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires a leading GeometryList value"
        )));
    }
    let count = gui_list_count(*root, property_name, "GeometryList")?;
    let geometries = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if geometries.len() != count
        || geometries
            .iter()
            .any(|geometry| !geometry.has_tag_name("Geometry"))
    {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} GeometryList count or record tag is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_filletedges(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires one FilletEdges value"
        )));
    };
    if !root.has_tag_name("FilletEdges") || root.attribute("file").is_none() {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} FilletEdges value is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_shape_list(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires one ShapeList value"
        )));
    };
    if !root.has_tag_name("ShapeList") {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires a leading ShapeList value"
        )));
    }
    let count = gui_list_count(*root, property_name, "ShapeList")?;
    let shapes = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if shapes.len() != count
        || shapes.iter().any(|shape| {
            !shape.has_tag_name("TopoShape")
                || (shape.attribute("file").is_none()
                    && shape.attribute("binary").is_none()
                    && shape.attribute("brep").is_none())
        })
    {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} ShapeList count or record is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_constraint_list(
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let roots = property
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires one ConstraintList value"
        )));
    };
    if !root.has_tag_name("ConstraintList") {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} requires a leading ConstraintList value"
        )));
    }
    let count = gui_list_count(*root, property_name, "ConstraintList")?;
    let constraints = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if constraints.len() != count
        || constraints
            .iter()
            .any(|constraint| !constraint.has_tag_name("Constrain"))
    {
        return Err(CodecError::malformed(format_args!(
            "GUI property {property_name} ConstraintList count or record tag is invalid"
        )));
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
        if property.side_entries.is_empty() {
            continue;
        }
        if property.type_name == "Part::PropertyTopoShapeList" {
            for entry_name in &property.side_entries {
                entries.get(entry_name).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "GUI property {} references missing side entry {entry_name}",
                        property.id
                    ))
                })?;
            }
            continue;
        }
        let entry_name = property
            .side_entries
            .first()
            .expect("nonempty side entries");
        if property.side_entries.len() != 1 {
            return Err(CodecError::malformed(format_args!(
                "GUI property {} references more than one side entry",
                property.id
            )));
        }
        let view = *entries.get(entry_name).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "GUI property {} references missing side entry {entry_name}",
                property.id
            ))
        })?;
        match property.type_name.as_str() {
            "App::PropertyColorList" => {
                parse_color_list(view, entry_name, requires_alpha_conversion)?;
            }
            "App::PropertyFloatList" => {
                parse_float_list(view, entry_name)?;
            }
            "Part::PropertyFilletEdges" => {
                parse_fillet_edges(view, entry_name)?;
            }
            "App::PropertyMaterialList" => {
                let version = property
                    .values
                    .iter()
                    .find(|value| value.tag == "MaterialList")
                    .and_then(|value| value.attributes.get("version"))
                    .map(|value| {
                        value.parse::<u32>().map_err(|_| {
                            CodecError::malformed(format_args!(
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
            "App::PropertyPlacementList" => {
                parse_placement_list(view, entry_name)?;
            }
            "App::PropertyVectorList" => {
                parse_vector_list(view, entry_name)?;
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
            CodecError::malformed(format_args!(
                "color-list entry {entry_name} count exceeds its payload"
            ))
        })?;
    if !view.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "color-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(colors
        .into_iter()
        .map(|value| convert_packed_alpha(value, requires_alpha_conversion))
        .collect())
}

fn parse_float_list(mut view: View<'_>, entry_name: &str) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let values = view
        .read_counted(count.into(), 8, View::f64_le)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "float-list entry {entry_name} count exceeds its payload"
            ))
        })?;
    if values.iter().any(|value| !value.is_finite()) {
        return Err(CodecError::malformed(format_args!(
            "float-list entry {entry_name} has a non-finite value"
        )));
    }
    if !view.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "float-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(())
}

fn parse_vector_list(mut view: View<'_>, entry_name: &str) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let values = view
        .read_counted(count.into(), 24, |view| {
            Some((view.f64_le()?, view.f64_le()?, view.f64_le()?))
        })
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "vector-list entry {entry_name} count exceeds its payload"
            ))
        })?;
    if values
        .iter()
        .flat_map(|value| [value.0, value.1, value.2])
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::malformed(format_args!(
            "vector-list entry {entry_name} has a non-finite value"
        )));
    }
    if !view.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "vector-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(())
}

fn parse_placement_list(mut view: View<'_>, entry_name: &str) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let values = view
        .read_counted(count.into(), 56, |view| {
            Some([
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
            ])
        })
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "placement-list entry {entry_name} count exceeds its payload"
            ))
        })?;
    if values.iter().flatten().any(|value| !value.is_finite()) {
        return Err(CodecError::malformed(format_args!(
            "placement-list entry {entry_name} has a non-finite value"
        )));
    }
    if !view.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "placement-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(())
}

fn parse_fillet_edges(mut view: View<'_>, entry_name: &str) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let values = view
        .read_counted(count.into(), 20, |view| {
            Some((view.i32_le()?, view.f64_le()?, view.f64_le()?))
        })
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "fillet-edges entry {entry_name} count exceeds its payload"
            ))
        })?;
    if values
        .iter()
        .any(|(_, radius1, radius2)| !radius1.is_finite() || !radius2.is_finite())
    {
        return Err(CodecError::malformed(format_args!(
            "fillet-edges entry {entry_name} has a non-finite radius"
        )));
    }
    if !view.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "fillet-edges entry {entry_name} has trailing bytes"
        )));
    }
    Ok(())
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
                CodecError::malformed(format_args!("GUI material list {property_id} is truncated"))
            })?;
            let count = if header < 0 {
                view.u32_le().ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "GUI material list {property_id} is truncated"
                    ))
                })?
            } else {
                header as u32
            };
            (count, false)
        }
        2 => (
            view.u32_le().ok_or_else(|| {
                CodecError::malformed(format_args!("GUI material list {property_id} is truncated"))
            })?,
            false,
        ),
        3 => (
            view.u32_le().ok_or_else(|| {
                CodecError::malformed(format_args!("GUI material list {property_id} is truncated"))
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
            CodecError::malformed(format_args!(
                "GUI material list {property_id} count exceeds its payload"
            ))
        })?;
    for material in &materials {
        if !material.shininess.is_finite() || !material.transparency.is_finite() {
            return Err(CodecError::malformed(format_args!(
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
        return Err(CodecError::malformed(format_args!(
            "GUI material list {property_id} has trailing bytes"
        )));
    }
    Ok(materials)
}

fn read_material_string(view: &mut View<'_>, property_id: &str) -> Result<String, CodecError> {
    let length = view.u32_le().ok_or_else(|| {
        CodecError::malformed(format_args!(
            "GUI material list {property_id} string is truncated"
        ))
    })?;
    let length = view
        .counted(length.into(), 1)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "GUI material list {property_id} string exceeds its payload"
            ))
        })?
        .get();
    String::from_utf8(view.take(length).expect("counted material string").to_vec()).map_err(|_| {
        CodecError::malformed(format_args!(
            "GUI material list {property_id} string is not UTF-8"
        ))
    })
}

#[allow(clippy::too_many_arguments)]
fn transfer_shape_appearances(
    ir: &CadIr,
    plan: &mut AppearancePlan,
    graph: &Graph,
    material_lists: &HashMap<String, Vec<GuiMaterial>>,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
    losses: &mut Vec<LossNote>,
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
        let body_ids = displayed_shape_bodies(ir, object_id, properties, payloads)?;
        let group = displayed_shape_group(object_id, properties, payloads, element_maps, "Face")?;
        let mapped_count = group.map_or(0, |group| group.names.len().saturating_sub(1));
        if materials.len() == 1 {
            let legacy_id = AppearanceId(format!("fcstd:appearance:object#{}", provider.name));
            plan.bindings
                .retain(|binding| binding.appearance != legacy_id);
            plan.appearances
                .retain(|appearance| appearance.id != legacy_id);
            plan.remove_appearances.insert(legacy_id);
        } else {
            let Some(_) = group else {
                continue;
            };
            if mapped_count == 0 {
                continue;
            }
            if materials.len() != mapped_count {
                losses.push(
                    crate::loss::FreecadLossCode::AppearanceTopologyColorCountMismatch
                        .note(format!(
                            "FCStd provider {} ShapeAppearance material count {} does not match {} mapped Face subelements; native material list retained and neutral face override withheld",
                            provider.name,
                            materials.len(),
                            mapped_count
                        ))
                        .with_provenance(SourceProvenance {
                            format: "fcstd".into(),
                            stream: "GuiDocument.xml".into(),
                            offset: property.byte_start,
                            tag: Some(property.id.clone()),
                        }),
                );
                continue;
            }
        }
        for (index, material) in materials.iter().enumerate() {
            let appearance_id = AppearanceId(format!(
                "fcstd:appearance:shape-material#{}:{}",
                provider.name,
                index + 1
            ));
            plan.appearances.push(material_appearance(
                appearance_id.clone(),
                &provider.name,
                index,
                material,
            ));
            if materials.len() == 1 {
                for (body_index, body) in body_ids.iter().enumerate() {
                    plan.body_updates.push(BodyUpdate {
                        id: body.clone(),
                        visible: Assignment::Keep,
                        color: Assignment::Set(Some(decode_color(
                            material.diffuse,
                            Some(material.transparency),
                        ))),
                    });
                    plan.bindings.push(AppearanceBinding {
                        id: format!(
                            "fcstd:appearance:binding#shape-material:{}:{body_index}",
                            provider.name
                        ),
                        target: AppearanceTarget::Body(body.clone()),
                        appearance: appearance_id.clone(),
                        source_entity_id: Some(object_id.to_owned()),
                        object_type: Some("ViewProvider ShapeAppearance".into()),
                        visible: None,
                        channels: BTreeMap::new(),
                    });
                }
            } else if let Some(group) = group {
                bind_material_faces(
                    ir,
                    plan,
                    group,
                    index,
                    &appearance_id,
                    &provider.name,
                    object_id,
                );
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
            return Err(CodecError::malformed(format_args!(
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
        _ => Err(CodecError::malformed(format_args!(
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
            return Err(CodecError::malformed(format_args!(
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
        _ => Err(CodecError::malformed(format_args!(
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
    ir: &CadIr,
    plan: &mut AppearancePlan,
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
        let binding_index = ir.model.appearance_bindings.len() + plan.bindings.len();
        plan.bindings.push(AppearanceBinding {
            id: format!("fcstd:appearance:binding#shape-material:{provider_name}:{binding_index}"),
            target: AppearanceTarget::Face(face),
            appearance: appearance_id.clone(),
            source_entity_id: Some(object_id.to_owned()),
            object_type: Some("ViewProvider ShapeAppearance".into()),
            visible: None,
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
    ir: &CadIr,
    plan: &mut AppearancePlan,
    provider_name: &str,
    object_id: &str,
    entry_name: &str,
    entries: &BTreeMap<String, View<'_>>,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
    kind: TopologyColorKind,
    requires_alpha_conversion: bool,
    provenance: SourceProvenance,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let view = *entries.get(entry_name).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "color list references missing entry {entry_name}"
        ))
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
        losses.push(
            crate::loss::FreecadLossCode::AppearanceTopologyColorCountMismatch
                .note(format!(
                    "FCStd provider {provider_name} {} color count {count} does not match {mapped_count} mapped subelements; native color list retained and neutral override withheld",
                    kind.name()
                ))
                .with_provenance(provenance),
        );
        return Ok(());
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
                plan.appearances.push(Appearance {
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
            plan.bindings.push(AppearanceBinding {
                id: format!(
                    "fcstd:appearance:binding#{lower}:{provider_name}:{}:{}",
                    index + 1,
                    crate::native::id_key(topology_id)
                ),
                target,
                appearance: appearance_id.clone(),
                source_entity_id: Some(object_id.to_owned()),
                object_type: Some(format!("ViewProvider {}", kind.name())),
                visible: None,
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
