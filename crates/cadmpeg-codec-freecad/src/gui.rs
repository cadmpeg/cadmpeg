// SPDX-License-Identifier: Apache-2.0
//! Transfer of `GuiDocument.xml` object appearance into neutral presentation records.

mod appearance_producer;
mod identities;
mod material_attributes;
mod materials;
mod presentation;
use materials::transfer_shape_appearances;
pub(crate) mod payloads;
mod schema;
pub(crate) mod source;
pub(crate) mod value_grammar;

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, IdentityKey};
use cadmpeg_ir::presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, PresentationStateKind,
    ViewPresentation,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteBinary32;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::SourceProvenance;

use crate::brep::ShapePayloadRecord;
use crate::loss::FreecadLossCode;
use crate::native::{
    parse_bool, GuiDocumentRecord, GuiPropertyRecord, GuiStateRecord, GuiViewProviderRecord,
    ObjectRecord, PropertyRecord, ValueRecord,
};
use crate::topology_transfer::TopologyOccurrence;

/// Checks that typed local target slots agree with the retained GUI carriers.
pub(crate) fn validate_property_links(property: &GuiPropertyRecord) -> Result<(), CodecError> {
    if property.type_name != "App::PropertyEnumeration"
        && property
            .values
            .iter()
            .any(|value| value.item_index.is_some())
    {
        return Err(CodecError::malformed(
            "GUI enumeration item ordinal belongs to a different runtime property",
        ));
    }
    crate::writer::xml::validate_gui(property)
}

#[derive(Default)]
pub(crate) struct Graph {
    pub(crate) entry_contents: Vec<crate::native::entry_contents::EntryContentUpdate>,
    pub(crate) documents: Vec<GuiDocumentRecord>,
    pub(crate) providers: Vec<GuiViewProviderRecord>,
    pub(crate) provider_sources: Vec<(String, std::ops::Range<usize>)>,
    pub(crate) value_sources: Vec<crate::fidelity::ValueSourceRanges>,
    pub(crate) state_sources: BTreeMap<String, crate::fidelity::ValueSourceRanges>,
    pub(crate) property_sources: BTreeMap<String, crate::fidelity::ValueSourceRanges>,
    pub(crate) properties: Vec<GuiPropertyRecord>,
    pub(crate) losses: Vec<LossNote>,
    /// The GUI stream was not admitted into typed records.
    pub(crate) untyped: bool,
}

#[derive(Default)]
struct AppearancePlan {
    body_updates: Vec<BodyUpdate>,
    body_visibility_owners: HashSet<String>,
    appearances: Vec<Appearance>,
    attributes: Vec<cadmpeg_ir::attributes::SourceAttribute>,
    bindings: Vec<AppearanceBinding>,
    remove_appearances: HashSet<AppearanceId>,
    presentation_documents: Vec<PresentationDocument>,
    view_presentations: Vec<ViewPresentation>,
}

struct BodyUpdate {
    id: cadmpeg_ir::ids::BodyId,
    visible: Option<bool>,
}

fn push_body_update(
    plan: &mut AppearancePlan,
    id: &cadmpeg_ir::ids::BodyId,
    visible: Option<bool>,
) {
    let id = id.clone();
    plan.body_updates.push(BodyUpdate { id, visible });
}

impl AppearancePlan {
    fn apply(self, ir: &mut CadIr) {
        for update in self.body_updates {
            if let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == update.id) {
                body.visible = update.visible;
            }
        }
        ir.model
            .appearance_bindings
            .retain(|binding| !self.remove_appearances.contains(&binding.appearance));
        ir.model
            .appearances
            .retain(|appearance| !self.remove_appearances.contains(&appearance.id));
        ir.model.appearances.extend(self.appearances);
        ir.model.attributes.extend(self.attributes);
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

/// The admitted native provider identity is the source of every neutral
/// appearance key. The persisted display name remains separate so names such
/// as `A B`, `#`, and the empty string remain legal source values.
fn provider_identity_key(name: &str) -> IdentityKey {
    IdentityKey::encode_segment(name)
}

fn object_appearance_id(provider: &IdentityKey) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(format!("fcstd:appearance:object#{provider}")).map_err(CodecError::malformed)
}

fn edge_appearance_id(provider: &IdentityKey) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(format!("fcstd:appearance:edge#{provider}")).map_err(CodecError::malformed)
}

fn vertex_appearance_id(provider: &IdentityKey) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(format!("fcstd:appearance:vertex#{provider}")).map_err(CodecError::malformed)
}

fn shape_material_appearance_id(
    provider: &IdentityKey,
    index: usize,
) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(format!(
        "fcstd:appearance:shape-material#{provider}:{}",
        index + 1
    ))
    .map_err(CodecError::malformed)
}

fn topology_appearance_id(
    kind: TopologyColorKind,
    provider: &IdentityKey,
    index: usize,
) -> Result<AppearanceId, CodecError> {
    let kind = topology_binding_kind(kind);
    AppearanceId::mint(format!("fcstd:appearance:{kind}#{provider}:{}", index + 1))
        .map_err(CodecError::malformed)
}

fn topology_binding_kind(kind: TopologyColorKind) -> IdentityKey {
    match kind {
        TopologyColorKind::Face => cadmpeg_ir::identity_key!("face"),
        TopologyColorKind::Edge => cadmpeg_ir::identity_key!("edge"),
        TopologyColorKind::Vertex => cadmpeg_ir::identity_key!("vertex"),
    }
}

fn binding_id(text: std::fmt::Arguments<'_>) -> Result<AppearanceBindingId, CodecError> {
    AppearanceBindingId::mint(text.to_string()).map_err(CodecError::malformed)
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

pub(crate) struct GuiSources<'a, 'b> {
    pub(crate) entries: &'a BTreeMap<String, View<'b>>,
    pub(crate) entry_owners: Option<&'a crate::container::parts::EntryOwners<'b>>,
    pub(crate) entry_interpretations: Option<&'a payloads::sources::EntryInterpretations<'b>>,
    pub(crate) gui_entry_interpretations: Option<&'a payloads::sources::EntryInterpretations<'b>>,
    pub(crate) declared_entries: Option<&'a std::collections::BTreeSet<String>>,
    pub(crate) declared_objects: Option<&'a std::collections::BTreeSet<String>>,
    pub(crate) admitted_objects: Option<&'a std::collections::BTreeSet<String>>,
    pub(crate) entry_parents:
        Option<&'a BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>>,
    pub(crate) objects: &'a [ObjectRecord],
    pub(crate) object_sources:
        Option<&'a BTreeMap<String, crate::persistence::object_parts::ObjectSource<'b>>>,
    pub(crate) properties: &'a [PropertyRecord],
    pub(crate) payloads: &'a [ShapePayloadRecord],
    pub(crate) topology_occurrences: &'a [TopologyOccurrence],
    pub(crate) requires_alpha_conversion: bool,
}

#[cfg(test)]
pub(crate) fn transfer<'source>(
    ctx: &DecodeContext<'source>,
    ir: &mut CadIr,
    bytes: &[u8],
    sources: &GuiSources<'_, 'source>,
) -> Result<Graph, CodecError> {
    let source = sources
        .entries
        .get("GuiDocument.xml")
        .copied()
        .ok_or_else(|| CodecError::malformed("GUI transfer has no original source View"))?;
    if source.window().as_ptr() != bytes.as_ptr() || source.window().len() != bytes.len() {
        return Err(CodecError::malformed(
            "GUI transfer bytes differ from its source View",
        ));
    }
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    source::parse(ctx, source)?.transfer(ctx, ir, sources, &mut output)
}

pub(crate) fn project_native(
    ir: &mut CadIr,
    namespace: &cadmpeg_ir::native::NativeNamespace,
    sources: &GuiSources<'_, '_>,
) -> Result<Graph, CodecError> {
    let mut graph = Graph {
        documents: namespace.arena_as("gui_documents")?,
        providers: namespace.arena_as("gui_view_providers")?,
        properties: namespace.arena_as("gui_properties")?,
        ..Graph::default()
    };
    let schema = schema::classify(
        graph
            .documents
            .first()
            .and_then(|document| document.schema_version.as_deref()),
    )
    .neutral_schema_version();
    let (plan, mut losses) = project_graph(
        ir,
        &graph,
        sources,
        schema,
        &mut appearance_producer::Current,
    )?;
    plan.apply(ir);
    graph.losses.append(&mut losses);
    Ok(graph)
}

pub(crate) fn project_records(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    graph: &mut Graph,
    sources: &GuiSources<'_, '_>,
) -> Result<(), CodecError> {
    let schema = schema::classify(
        graph
            .documents
            .first()
            .and_then(|document| document.schema_version.as_deref()),
    )
    .neutral_schema_version();
    let (plan, mut losses) = project_graph(
        ir,
        graph,
        sources,
        schema,
        &mut appearance_producer::Decoded { ctx, sources },
    )?;
    plan.apply(ir);
    graph.losses.append(&mut losses);
    Ok(())
}

fn project_graph(
    ir: &CadIr,
    graph: &Graph,
    sources: &GuiSources<'_, '_>,
    neutral_schema_version: Option<u32>,
    producer: &mut impl appearance_producer::Producer,
) -> Result<(AppearancePlan, Vec<LossNote>), CodecError> {
    let mut plan = AppearancePlan::default();
    let mut losses = Vec::new();
    let requires_alpha_conversion = sources.requires_alpha_conversion;
    let mut payloads_by_owner = Vec::new();
    for payload in sources.payloads {
        if let Some(property) = sources
            .properties
            .iter()
            .find(|property| property.id == payload.property)
        {
            payloads_by_owner.push((
                property.owner.as_str(),
                property.name.as_str(),
                payload.id.as_str(),
            ));
        }
    }
    for provider in &graph.providers {
        let name = provider.name.as_str();
        let provider_key = provider_identity_key(name);
        let property_nodes = graph
            .properties
            .iter()
            .filter(|property| property.owner == provider.id)
            .collect::<Vec<_>>();
        let mut values = HashMap::new();
        for property in &property_nodes {
            if presentation_property_type(&property.name) == Some(property.type_name.as_str()) {
                if let Some(value) = property.values.iter().find(|value| value.parent.is_none()) {
                    values.insert(property.name.as_str(), value);
                }
            }
        }
        let property_provenance = |property_name: &str, type_name: &str| {
            let offset = property_nodes
                .iter()
                .find(|property| property.name == property_name && property.type_name == type_name)
                .and_then(|property| graph.property_sources.get(&property.id))
                .map_or(0, |source| {
                    cadmpeg_core::decode::u64_from_index(source.record_range().start)
                });
            gui_provider_property_provenance(name, property_name, offset)
        };
        let visibility = values
            .get("Visibility")
            .and_then(|value| value.attribute("value"))
            .and_then(parse_bool);
        let transparency = values
            .get("Transparency")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<f32>().ok())
            .map(|percent| percent / 100.0);
        let packed_color = values
            .get("ShapeColor")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
            .map(|value| convert_packed_alpha(value, requires_alpha_conversion));
        let material = values.get("ShapeMaterial");
        let Some(object_id) = provider.object.as_ref().map(|object| object.as_str()) else {
            if packed_color.is_some() || material.is_some() {
                plan.appearances.push(object_style(
                    name,
                    &provider_key,
                    packed_color,
                    material.copied(),
                    transparency,
                    &property_nodes,
                    requires_alpha_conversion,
                )?);
            }
            continue;
        };
        let body_ids = {
            select_shape_bodies(
                ir,
                payloads_by_owner
                    .iter()
                    .filter(|(owner, property, _)| *owner == object_id && *property == "Shape")
                    .map(|(_, _, payload)| *payload),
            )
        };
        if !body_ids.is_empty() {
            plan.body_visibility_owners.insert(object_id.to_owned());
        }
        if let Some(visible) = visibility {
            for body_id in &body_ids {
                push_body_update(&mut plan, body_id, Some(visible));
            }
        }
        if let Some(file) = values
            .get("DiffuseColor")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                &mut plan,
                TopologyColorRequest {
                    provider_name: name,
                    provider_key: &provider_key,
                    object_id,
                    entry_name: file,
                    property: &property_nodes
                        .iter()
                        .find(|property| {
                            property.name == "DiffuseColor"
                                && property.type_name == "App::PropertyColorList"
                        })
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "registered color property has no source identity",
                            )
                        })?
                        .id,
                    kind: TopologyColorKind::Face,
                    provenance: property_provenance("DiffuseColor", "App::PropertyColorList"),
                },
                sources,
                &mut losses,
                producer,
            )?;
        }
        let payload_prefixes = shape_payload_prefixes(&payloads_by_owner, object_id);
        if let Some(color) = values
            .get("LineColor")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
            .map(|value| convert_packed_alpha(value, requires_alpha_conversion))
        {
            let width = values
                .get("LineWidth")
                .and_then(|value| value.attribute("value"));
            transfer_primitive_appearance(
                ir,
                &mut plan,
                &mut losses,
                PrimitiveAppearanceSource {
                    provider_name: name,
                    object_id,
                    native_ref: property_nodes
                        .iter()
                        .find(|property| property.name == "LineColor")
                        .map(|property| property.id.as_str()),
                    packed_color: color,
                    style: PrimitiveStyle::Line(PrimitiveSize::from_source(width)),
                    payload_prefixes: &payload_prefixes,
                    provenance: property_provenance("LineWidth", "App::PropertyFloatConstraint"),
                },
            )?;
        }
        if let Some(file) = values
            .get("LineColorArray")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                &mut plan,
                TopologyColorRequest {
                    provider_name: name,
                    provider_key: &provider_key,
                    object_id,
                    entry_name: file,
                    property: &property_nodes
                        .iter()
                        .find(|property| {
                            property.name == "LineColorArray"
                                && property.type_name == "App::PropertyColorList"
                        })
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "registered color property has no source identity",
                            )
                        })?
                        .id,
                    kind: TopologyColorKind::Edge,
                    provenance: property_provenance("LineColorArray", "App::PropertyColorList"),
                },
                sources,
                &mut losses,
                producer,
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
                .and_then(|value| value.attribute("value"));
            transfer_primitive_appearance(
                ir,
                &mut plan,
                &mut losses,
                PrimitiveAppearanceSource {
                    provider_name: name,
                    object_id,
                    native_ref: property_nodes
                        .iter()
                        .find(|property| property.name == "PointColor")
                        .map(|property| property.id.as_str()),
                    packed_color: color,
                    style: PrimitiveStyle::Point(PrimitiveSize::from_source(size)),
                    payload_prefixes: &payload_prefixes,
                    provenance: property_provenance("PointSize", "App::PropertyFloatConstraint"),
                },
            )?;
        }
        if let Some(file) = values
            .get("PointColorArray")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ir,
                &mut plan,
                TopologyColorRequest {
                    provider_name: name,
                    provider_key: &provider_key,
                    object_id,
                    entry_name: file,
                    property: &property_nodes
                        .iter()
                        .find(|property| {
                            property.name == "PointColorArray"
                                && property.type_name == "App::PropertyColorList"
                        })
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "registered color property has no source identity",
                            )
                        })?
                        .id,
                    kind: TopologyColorKind::Vertex,
                    provenance: property_provenance("PointColorArray", "App::PropertyColorList"),
                },
                sources,
                &mut losses,
                producer,
            )?;
        }
        if packed_color.is_none() && material.is_none() {
            continue;
        }
        let appearance = object_style(
            name,
            &provider_key,
            packed_color,
            material.copied(),
            transparency,
            &property_nodes,
            requires_alpha_conversion,
        )?;
        let appearance_id = appearance.id.clone();
        plan.appearances.push(appearance);
        for (index, body) in body_ids.into_iter().enumerate() {
            plan.bindings.push(AppearanceBinding {
                native_ref: None,
                id: binding_id(format_args!(
                    "fcstd:appearance:binding#{provider_key}:{index}"
                ))?,
                target: AppearanceTarget::Body(body.clone()),
                appearance: appearance_id.clone(),
                source_entity_id: Some(object_id.to_owned()),
                object_type: Some("ViewProvider".into()),
                visible: None,
                channels: BTreeMap::new(),
            });
        }
    }
    let material_lists = payloads::material_lists(ir, graph, requires_alpha_conversion)?;
    transfer_shape_appearances(
        ir,
        &mut plan,
        graph,
        &material_lists,
        sources,
        &mut losses,
        producer,
    )?;
    presentation::transfer(
        &mut plan,
        graph,
        neutral_schema_version,
        &mut losses,
        producer,
    )?;
    Ok((plan, losses))
}

fn object_style(
    name: &str,
    provider_key: &IdentityKey,
    packed_color: Option<u32>,
    material: Option<&ValueRecord>,
    transparency: Option<f32>,
    property_nodes: &[&GuiPropertyRecord],
    requires_alpha_conversion: bool,
) -> Result<Appearance, CodecError> {
    let material_color = material
        .and_then(|material| material.attribute("diffuseColor"))
        .and_then(|value| value.parse::<u32>().ok())
        .map(|value| convert_packed_alpha(value, requires_alpha_conversion))
        .map(|packed| decode_color(packed, None))
        .transpose()?;
    let appearance_id = object_appearance_id(provider_key)?;
    let mut material_properties = BTreeMap::new();
    {
        for (source, target) in [
            ("shininess", cadmpeg_core::nonblank_literal!("shininess")),
            (
                "transparency",
                cadmpeg_core::nonblank_literal!("material_transparency"),
            ),
        ] {
            if let Some(value) = material
                .and_then(|material| material.attribute(source))
                .and_then(|value| value.parse::<f64>().ok())
                .and_then(cadmpeg_ir::scalar::FiniteReal::new)
            {
                material_properties.insert(target, value);
            }
        }
    }
    Ok(Appearance {
        native_ref: property_nodes
            .iter()
            .find(|property| {
                property.name
                    == if packed_color.is_some() {
                        "ShapeColor"
                    } else {
                        "ShapeMaterial"
                    }
            })
            .map(|property| property.id.clone()),
        id: appearance_id.clone(),
        name: Some(format!("{name} shape appearance")),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("FCStd ViewProvider ShapeMaterial".into()),
        category: None,
        base_color: packed_color
            .map(|packed| decode_color(packed, transparency))
            .transpose()?
            .or(material_color),
        textures: Vec::new(),
        properties: material_properties,
    })
}

impl ValueRecord {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

fn push_gui_appearance_loss(
    losses: &mut Vec<LossNote>,
    code: FreecadLossCode,
    message: std::fmt::Arguments<'_>,
    provenance: SourceProvenance,
) {
    losses.push(code.note(message.to_string()).with_provenance(provenance));
}

fn gui_provider_property_provenance(
    provider_name: &str,
    property_name: &str,
    offset: u64,
) -> SourceProvenance {
    let tag = format!("ViewProvider {provider_name} property {property_name}");
    SourceProvenance::in_stream("fcstd", cadmpeg_ir::stream_name!("GuiDocument.xml"), offset)
        .with_tag(tag)
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

fn gui_named_entries<'a>(
    record: impl Fn() -> String,
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> (
    BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    Vec<cadmpeg_core::text::NamedEntryError>,
) {
    use cadmpeg_core::text::{NamedEntryError, NonBlankString};
    let mut kept = BTreeMap::new();
    let mut refused = Vec::new();
    for (name, value) in entries {
        match NonBlankString::new(name.to_owned()) {
            Some(key) if kept.contains_key(&key) => {
                refused.push(NamedEntryError::Restated {
                    record: record(),
                    key,
                });
            }
            Some(key) => {
                kept.insert(key, value.to_owned());
            }
            None => {
                refused.push(NamedEntryError::Blank { record: record() });
            }
        }
    }
    (kept, refused)
}

/// Transfers the GUI graph's presentation layer into `plan`.
///
/// A property whose key is blank is charged to `losses` and the rest of the
/// property set survives: a blank key is one unreadable property of one record,
/// not a reason to answer no GUI presentation at all.

fn gui_property_value(property: &GuiPropertyRecord) -> Option<&str> {
    property.values.iter().find_map(|value| {
        value
            .attributes
            .get("value")
            .or_else(|| value.attributes.get("Value"))
            .map(String::as_str)
    })
}

/// One loss per property key the reader could not key, naming the key's own
/// record and, for a restated key, the key.
fn note_refused_gui_keys(
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    refused: &[cadmpeg_core::text::NamedEntryError],
) {
    use cadmpeg_core::text::NamedEntryError;
    for key in refused {
        let message = match key {
            NamedEntryError::Blank { record } => format!(
                "{record} states a property with a blank key; the property value is not transferred"
            ),
            NamedEntryError::Restated { record, key } => format!(
                "{record} states the property {key} a second time; the property value is not transferred"
            ),
        };
        losses.push(FreecadLossCode::SourceGuiPropertyKeyBlank.note(message));
    }
}

fn camera_state_value(
    state: &GuiStateRecord,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<CameraState, CodecError> {
    let settings = state
        .attributes
        .get("settings")
        .ok_or_else(|| CodecError::Malformed("GUI Camera has no settings attribute".into()))?;
    let CameraSettings {
        position,
        orientation,
    } = parse_camera_settings(settings)?;
    let position = position
        .map(|value| {
            cadmpeg_ir::units::FiniteVector::new(value)
                .ok_or_else(|| CodecError::malformed("camera position must be finite"))
        })
        .transpose()?;
    let orientation = orientation
        .map(|value| {
            cadmpeg_ir::units::NonzeroVector::new(value).ok_or_else(|| {
                CodecError::malformed("camera orientation must be finite and nonzero")
            })
        })
        .transpose()?;
    let (properties, refused) = gui_named_entries(
        || format!("the gui {} state", state.kind),
        state
            .attributes
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    );
    note_refused_gui_keys(losses, &refused);
    Ok(CameraState {
        position,
        orientation,
        properties,
    })
}

fn parse_camera_settings(settings: &str) -> Result<CameraSettings, CodecError> {
    if settings.trim().is_empty() {
        return Ok(CameraSettings {
            position: None,
            orientation: None,
        });
    }

    let mut tokens = Vec::new();
    tokens.extend(settings.split_whitespace());
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
    let end_index = start.checked_add(N).ok_or_else(|| {
        CodecError::malformed(format_args!("GUI camera {field} field offset overflows"))
    })?;
    let values = tokens
        .get(start..end_index)
        .filter(|values| values.len() == N && end_index <= end)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("GUI camera {field} field is incomplete"))
        })?;
    let mut parsed = [0.0; N];
    for (index, value) in values.iter().enumerate() {
        parsed[index] = value.parse::<f64>().map_err(|_| {
            CodecError::malformed(format_args!("GUI camera {field} field is not numeric"))
        })?;
    }
    Ok(parsed)
}

#[derive(Clone, Copy)]
enum PrimitiveStyle {
    Line(PrimitiveSize),
    Point(PrimitiveSize),
}

#[derive(Clone, Copy)]
enum PrimitiveSize {
    Absent,
    Admitted(cadmpeg_ir::scalar::FiniteReal),
    NonFinite,
}

impl PrimitiveSize {
    fn from_source(value: Option<&str>) -> Self {
        match value.and_then(|text| text.parse::<f64>().ok()) {
            None => Self::Absent,
            Some(value) => {
                cadmpeg_ir::scalar::FiniteReal::new(value).map_or(Self::NonFinite, Self::Admitted)
            }
        }
    }
}

struct PrimitiveAppearanceSource<'a> {
    provider_name: &'a str,
    object_id: &'a str,
    native_ref: Option<&'a str>,
    packed_color: u32,
    style: PrimitiveStyle,
    payload_prefixes: &'a [String],
    provenance: SourceProvenance,
}

fn shape_payload_prefixes(
    payloads_by_owner: &[(&str, &str, &str)],
    object_id: &str,
) -> Vec<String> {
    let mut prefixes = Vec::new();
    for (_, _, payload) in payloads_by_owner
        .iter()
        .filter(|(owner, property, _)| *owner == object_id && *property == "Shape")
    {
        prefixes.push(format!("{}:", crate::native::id_key(payload)));
    }
    prefixes
}

fn transfer_primitive_appearance(
    ir: &CadIr,
    plan: &mut AppearancePlan,
    losses: &mut Vec<LossNote>,
    source: PrimitiveAppearanceSource<'_>,
) -> Result<(), CodecError> {
    let PrimitiveAppearanceSource {
        provider_name,
        object_id,
        native_ref,
        packed_color,
        style,
        payload_prefixes,
        provenance,
    } = source;
    let mut targets = Vec::new();
    match style {
        PrimitiveStyle::Line(_) => {
            for edge in ir.model.edges.iter().filter(|edge| {
                payload_prefixes
                    .iter()
                    .any(|prefix| crate::native::id_key(edge.id.as_str()).starts_with(prefix))
            }) {
                targets.push(AppearanceTarget::Edge(edge.id.clone()));
            }
        }
        PrimitiveStyle::Point(_) => {
            for vertex in ir.model.vertices.iter().filter(|vertex| {
                payload_prefixes
                    .iter()
                    .any(|prefix| crate::native::id_key(vertex.id.as_str()).starts_with(prefix))
            }) {
                targets.push(AppearanceTarget::Vertex(vertex.id.clone()));
            }
        }
    }
    if targets.is_empty() {
        return Ok(());
    }
    let provider_key = provider_identity_key(provider_name);
    let (appearance_id, label, size, binding_key, object_type, precedence) = match style {
        PrimitiveStyle::Line(width) => (
            edge_appearance_id(&provider_key)?,
            "line",
            width,
            cadmpeg_ir::identity_key!("edge"),
            "ViewProvider Edge",
            "edge_over_object",
        ),
        PrimitiveStyle::Point(size) => (
            vertex_appearance_id(&provider_key)?,
            "point",
            size,
            cadmpeg_ir::identity_key!("vertex"),
            "ViewProvider Vertex",
            "vertex_over_object",
        ),
    };
    let admitted_size = match size {
        PrimitiveSize::Admitted(value) if value.get() >= 0.0 => Some(value),
        PrimitiveSize::Absent | PrimitiveSize::Admitted(_) | PrimitiveSize::NonFinite => None,
    };
    if matches!(size, PrimitiveSize::NonFinite | PrimitiveSize::Admitted(_))
        && admitted_size.is_none()
    {
        push_gui_appearance_loss(
            losses,
            FreecadLossCode::AppearancePrimitiveSizeNotTransferred,
            format_args!(
                "FCStd provider {provider_name} {label} size cannot enter the neutral appearance"
            ),
            provenance,
        );
    }
    plan.appearances.push(Appearance {
        native_ref: native_ref.map(ToOwned::to_owned),
        id: appearance_id.clone(),
        name: Some(format!("{provider_name} {label} appearance")),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some(format!("FCStd ViewProvider {label} style")),
        category: None,
        base_color: Some(Color::from_rgba8(
            u8::try_from((packed_color >> 24) & 0xff).map_err(|_| {
                CodecError::Malformed("GUI color channel exceeds byte range".into())
            })?,
            u8::try_from((packed_color >> 16) & 0xff).map_err(|_| {
                CodecError::Malformed("GUI color channel exceeds byte range".into())
            })?,
            u8::try_from((packed_color >> 8) & 0xff).map_err(|_| {
                CodecError::Malformed("GUI color channel exceeds byte range".into())
            })?,
            u8::try_from(packed_color & 0xff).map_err(|_| {
                CodecError::Malformed("GUI color channel exceeds byte range".into())
            })?,
        )),
        textures: Vec::new(),
        properties: BTreeMap::new(),
    });
    for (index, target) in targets.into_iter().enumerate() {
        plan.bindings.push(AppearanceBinding {
            native_ref: native_ref.map(ToOwned::to_owned),
            id: binding_id(format_args!(
                "fcstd:appearance:binding#{binding_key}:{provider_key}:{index}"
            ))?,
            target,
            appearance: appearance_id.clone(),
            source_entity_id: Some(object_id.to_owned()),
            object_type: Some(object_type.into()),
            visible: None,
            channels: [(
                cadmpeg_core::nonblank_literal!("precedence"),
                precedence.into(),
            )]
            .into(),
        });
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GuiValueTag {
    Bool,
    Integer,
    Float,
    String,
    PropertyColor,
    ColorList,
    PropertyMaterial,
    MaterialList,
    PropertyVector,
    BoolList,
    FloatList,
    IntegerList,
    IntegerSet,
    StringList,
    Map,
    PropertyMatrix,
    Path,
    PropertyPlacement,
    PlacementList,
    Python,
    PropertyRotation,
    Uuid,
    VectorList,
    FileIncluded,
}

impl GuiValueTag {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "Bool",
            Self::Integer => "Integer",
            Self::Float => "Float",
            Self::String => "String",
            Self::PropertyColor => "PropertyColor",
            Self::ColorList => "ColorList",
            Self::PropertyMaterial => "PropertyMaterial",
            Self::MaterialList => "MaterialList",
            Self::PropertyVector => "PropertyVector",
            Self::BoolList => "BoolList",
            Self::FloatList => "FloatList",
            Self::IntegerList => "IntegerList",
            Self::IntegerSet => "IntegerSet",
            Self::StringList => "StringList",
            Self::Map => "Map",
            Self::PropertyMatrix => "PropertyMatrix",
            Self::Path => "Path",
            Self::PropertyPlacement => "PropertyPlacement",
            Self::PlacementList => "PlacementList",
            Self::Python => "Python",
            Self::PropertyRotation => "PropertyRotation",
            Self::Uuid => "Uuid",
            Self::VectorList => "VectorList",
            Self::FileIncluded => "FileIncluded",
        }
    }
}

fn gui_value_tag(type_name: &str) -> Option<GuiValueTag> {
    if GUI_QUANTITY_TYPES.contains(&type_name) {
        return Some(GuiValueTag::Float);
    }
    let tag = match type_name {
        "App::PropertyBool" => GuiValueTag::Bool,
        "App::PropertyEnumeration"
        | "App::PropertyInteger"
        | "App::PropertyIntegerConstraint"
        | "App::PropertyPercent" => GuiValueTag::Integer,
        "App::PropertyAngle"
        | "App::PropertyDistance"
        | "App::PropertyFloat"
        | "App::PropertyFloatConstraint"
        | "App::PropertyLength"
        | "App::PropertyPrecision" => GuiValueTag::Float,
        "App::PropertyFile"
        | "App::PropertyFont"
        | "App::PropertyPersistentObject"
        | "App::PropertyString" => GuiValueTag::String,
        "App::PropertyColor" => GuiValueTag::PropertyColor,
        "App::PropertyColorList" => GuiValueTag::ColorList,
        "App::PropertyMaterial" => GuiValueTag::PropertyMaterial,
        "App::PropertyMaterialList" => GuiValueTag::MaterialList,
        "App::PropertyVector"
        | "App::PropertyVectorDistance"
        | "App::PropertyPosition"
        | "App::PropertyDirection" => GuiValueTag::PropertyVector,
        "App::PropertyBoolList" => GuiValueTag::BoolList,
        "App::PropertyFloatList" => GuiValueTag::FloatList,
        "App::PropertyIntegerList" => GuiValueTag::IntegerList,
        "App::PropertyIntegerSet" => GuiValueTag::IntegerSet,
        "App::PropertyStringList" => GuiValueTag::StringList,
        "App::PropertyMap" => GuiValueTag::Map,
        "App::PropertyMatrix" => GuiValueTag::PropertyMatrix,
        "App::PropertyPath" => GuiValueTag::Path,
        "App::PropertyPlacement" => GuiValueTag::PropertyPlacement,
        "App::PropertyPlacementList" => GuiValueTag::PlacementList,
        "App::PropertyPythonObject" => GuiValueTag::Python,
        "App::PropertyRotation" => GuiValueTag::PropertyRotation,
        "App::PropertyUUID" => GuiValueTag::Uuid,
        "App::PropertyVectorList" => GuiValueTag::VectorList,
        "App::PropertyFileIncluded" => GuiValueTag::FileIncluded,
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GuiMaterial {
    pub(crate) ambient: u32,
    pub(crate) diffuse: u32,
    pub(crate) specular: u32,
    pub(crate) emissive: u32,
    pub(crate) shininess: FiniteBinary32,
    pub(crate) transparency: FiniteBinary32,
    pub(crate) image: Option<String>,
    pub(crate) image_path: Option<String>,
    pub(crate) uuid: Option<String>,
}

fn displayed_shape_bodies(
    ir: &CadIr,
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<cadmpeg_ir::ids::BodyId>, CodecError> {
    let Some(payload) = displayed_shape_payload(object_id, properties, payloads)? else {
        return Ok(Vec::new());
    };
    Ok(select_shape_bodies(
        ir,
        std::iter::once(payload.id.as_str()),
    ))
}

pub(crate) fn select_shape_bodies<'a>(
    ir: &CadIr,
    payload_ids: impl IntoIterator<Item = &'a str>,
) -> Vec<cadmpeg_ir::ids::BodyId> {
    let mut body_ids = Vec::new();
    for payload_id in payload_ids {
        let payload_key = crate::native::id_key(payload_id);
        for body in &ir.model.bodies {
            let body_key = crate::native::id_key(body.id.as_str());
            if body_key
                .strip_prefix(payload_key)
                .is_some_and(|suffix| suffix.starts_with(':'))
            {
                body_ids.push(body.id.clone());
            }
        }
    }
    body_ids
}

fn displayed_shape_payload<'a>(
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &'a [ShapePayloadRecord],
) -> Result<Option<&'a ShapePayloadRecord>, CodecError> {
    let mut shape_properties = properties
        .iter()
        .filter(|property| property.owner == object_id && property.name == "Shape");
    let Some(property) = shape_properties.next() else {
        return Ok(None);
    };
    if shape_properties.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "object {object_id} has multiple Shape properties"
        )));
    }
    let mut shape_payloads = payloads
        .iter()
        .filter(|payload| payload.property == property.id);
    let Some(payload) = shape_payloads.next() else {
        return Ok(None);
    };
    if shape_payloads.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "Shape property {} has multiple payloads",
            property.id
        )));
    }
    Ok(Some(payload))
}

fn displayed_shape_topology<'a>(
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    occurrences: &'a [TopologyOccurrence],
    indexed_name: &str,
) -> Result<Vec<Vec<&'a str>>, CodecError> {
    let Some(payload) = displayed_shape_payload(object_id, properties, payloads)? else {
        return Ok(Vec::new());
    };
    let mut indexed = BTreeMap::<usize, Vec<&str>>::new();
    for occurrence in occurrences.iter().filter(|occurrence| {
        occurrence.property == payload.property && occurrence.indexed_name == indexed_name
    }) {
        indexed
            .entry(occurrence.source_index)
            .or_default()
            .push(&occurrence.topology_id);
    }
    if indexed.keys().copied().ne(1..=indexed.len()) {
        return Ok(Vec::new());
    }
    Ok(indexed.into_values().collect())
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

struct TopologyColorRequest<'a> {
    provider_name: &'a str,
    provider_key: &'a IdentityKey,
    object_id: &'a str,
    entry_name: &'a str,
    property: &'a str,
    kind: TopologyColorKind,
    provenance: SourceProvenance,
}

fn transfer_topology_colors(
    ir: &CadIr,
    plan: &mut AppearancePlan,
    request: TopologyColorRequest<'_>,
    sources: &GuiSources<'_, '_>,
    losses: &mut Vec<LossNote>,
    producer: &mut impl appearance_producer::Producer,
) -> Result<(), CodecError> {
    let TopologyColorRequest {
        provider_name,
        provider_key,
        object_id,
        entry_name,
        property,
        kind,
        provenance,
    } = request;
    let entry_id = crate::native::native_id("entry", entry_name);
    let entries = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::EntryRecord>("entries"))
        .transpose()?
        .unwrap_or_default();
    let Some(values) = entries
        .iter()
        .find(|entry| entry.id() == entry_id)
        .and_then(|entry| {
            entry.contents().iter().find_map(|content| match content {
                crate::native::entry_contents::EntryContents::Color { values } => Some(values),
                _ => None,
            })
        })
    else {
        return Ok(());
    };
    let count = values.len();
    let group = displayed_shape_topology(
        object_id,
        sources.properties,
        sources.payloads,
        sources.topology_occurrences,
        kind.name(),
    )?;
    let mapped_count = group.len();
    let matched = mapped_count > 0 && (count == 1 || count == mapped_count);
    if mapped_count > 0 && count != 0 && count != 1 && count != mapped_count {
        push_gui_appearance_loss(losses, FreecadLossCode::AppearanceTopologyColorCountMismatch, format_args!("FCStd provider {provider_name} {} color count {count} does not match {mapped_count} mapped subelements; indexed bindings withheld", kind.name()), provenance);
    }
    let default_appearance = match kind {
        TopologyColorKind::Face => None,
        TopologyColorKind::Edge => Some(edge_appearance_id(provider_key)?),
        TopologyColorKind::Vertex => Some(vertex_appearance_id(provider_key)?),
    };
    if let Some(default_appearance) = default_appearance.filter(|_| matched) {
        let overridden = group
            .iter()
            .enumerate()
            .filter(|(index, _)| values[if count == 1 { 0 } else { *index }].is_some())
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect::<HashSet<_>>();
        plan.bindings.retain(|binding| {
            if binding.appearance != default_appearance {
                return true;
            }
            let topology_id = match &binding.target {
                AppearanceTarget::Edge(id) => id.as_str(),
                AppearanceTarget::Vertex(id) => id.as_str(),
                _ => return true,
            };
            !overridden.contains(topology_id)
        });
    }
    for (index, value) in values.iter().enumerate() {
        let Some(value) = value else { continue };
        let appearance_id = topology_appearance_id(kind, provider_key, index)?;
        plan.appearances
            .push(producer.color(appearance_producer::ColorRequest {
                id: appearance_id.clone(),
                provider: provider_name,
                property,
                object: object_id,
                entry: &entry_id,
                index,
                kind,
                value: *value,
                alpha: sources.requires_alpha_conversion,
            })?);
        if !matched {
            continue;
        }
        let uniform_names = (count == 1)
            .then_some(&group)
            .into_iter()
            .flat_map(|groups| groups.iter().flatten());
        let indexed_names = (count != 1)
            .then_some(&group[index])
            .into_iter()
            .flat_map(|names| names.iter());
        let mut bound_topology = HashSet::new();
        for &topology_id in uniform_names.chain(indexed_names) {
            if !bound_topology.insert(topology_id) {
                continue;
            }
            let target = match kind {
                TopologyColorKind::Face => ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id.as_str() == topology_id)
                    .map(|face| AppearanceTarget::Face(face.id.clone())),
                TopologyColorKind::Edge => ir
                    .model
                    .edges
                    .iter()
                    .find(|edge| edge.id.as_str() == topology_id)
                    .map(|edge| AppearanceTarget::Edge(edge.id.clone())),
                TopologyColorKind::Vertex => ir
                    .model
                    .vertices
                    .iter()
                    .find(|vertex| vertex.id.as_str() == topology_id)
                    .map(|vertex| AppearanceTarget::Vertex(vertex.id.clone())),
            };
            let Some(target) = target else {
                continue;
            };
            let topology_key = crate::native::id_key(topology_id);
            let kind_key = topology_binding_kind(kind);
            plan.bindings.push(AppearanceBinding {
                native_ref: None,
                id: binding_id(format_args!(
                    "fcstd:appearance:binding#{kind_key}:{provider_key}:{}:{topology_key}",
                    index + 1,
                ))?,
                target,
                appearance: appearance_id.clone(),
                source_entity_id: Some(object_id.to_owned()),
                object_type: Some(format!("ViewProvider {}", kind.name())),
                visible: None,
                channels: [(
                    cadmpeg_core::nonblank_literal!("precedence"),
                    kind.precedence().into(),
                )]
                .into(),
            });
        }
    }
    Ok(())
}

fn decode_color(value: u32, transparency: Option<f32>) -> Result<Color, CodecError> {
    Color::new(
        f32::from(
            u8::try_from((value >> 24) & 0xff).map_err(|_| {
                CodecError::Malformed("GUI color channel exceeds byte range".into())
            })?,
        ) / 255.0,
        f32::from(
            u8::try_from((value >> 16) & 0xff).map_err(|_| {
                CodecError::Malformed("GUI color channel exceeds byte range".into())
            })?,
        ) / 255.0,
        f32::from(
            u8::try_from((value >> 8) & 0xff).map_err(|_| {
                CodecError::Malformed("GUI color channel exceeds byte range".into())
            })?,
        ) / 255.0,
        transparency.map_or(
            f32::from(u8::try_from(value & 0xff).map_err(|_| {
                CodecError::Malformed("GUI color channel exceeds byte range".into())
            })?) / 255.0,
            |value| 1.0 - value,
        ),
    )
    .ok_or_else(|| CodecError::Malformed("GUI color components must be in [0, 1]".into()))
}

fn convert_packed_alpha(value: u32, required: bool) -> u32 {
    if required {
        (value & 0xffff_ff00) | (0xff - (value & 0xff))
    } else {
        value
    }
}

#[cfg(test)]
mod color_tests {
    use super::{decode_color, requires_alpha_conversion};

    #[test]
    fn packed_alpha_is_used_without_a_transparency_property() {
        let color = decode_color(0x1122_3340, None).expect("valid color");
        assert!((color.a() - 64.0 / 255.0).abs() < f32::EPSILON);
    }

    #[test]
    fn transparency_property_overrides_packed_alpha() {
        let color = decode_color(0x1122_3300, Some(0.25)).expect("valid color");
        assert!((color.a() - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn program_version_selects_legacy_alpha_conversion() {
        assert!(requires_alpha_conversion(Some("0.21R33668")));
        assert!(requires_alpha_conversion(Some("1.0R39109")));
        assert!(!requires_alpha_conversion(Some("1.1R42000")));
        assert!(!requires_alpha_conversion(Some("cadmpeg")));
        assert!(!requires_alpha_conversion(None));
    }

    fn actual_material_reader(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
        use cadmpeg_ir::Codec;
        let xml=b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='property' type='App::PropertyMaterialList'><MaterialList file='Materials' version='0'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
        let archive=crate::test_support::test_archive::archive_entries(&[("Document.xml",b"<Document SchemaVersion='4'><Objects Count='0'/><ObjectData Count='0'/></Document>"),("GuiDocument.xml",xml),("Materials",bytes)]);
        crate::FcstdCodec
            .decode(
                &mut std::io::Cursor::new(archive),
                &cadmpeg_ir::DecodeOptions::default(),
            )
            .unwrap()
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

        let output = actual_material_reader(&bytes);
        let graph = super::Graph {
            properties: output
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as("gui_properties")
                .unwrap(),
            ..Default::default()
        };
        assert!(output
            .source_fidelity()
            .parts()
            .values()
            .all(|part| !matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp { .. }
            )));
        let materials = super::payloads::material_lists(output.ir(), &graph, true)
            .unwrap()
            .into_values()
            .next()
            .unwrap()
            .materials
            .into_values()
            .collect::<Vec<_>>();
        assert_eq!(materials.len(), 1);
        assert_eq!(materials[0].ambient, 0x1122_33ff);
        assert_eq!(materials[0].diffuse, 0x4455_66bf);
        assert_eq!(materials[0].specular, 0x7788_997f);
        assert_eq!(materials[0].emissive, 0xaabb_cc00);
    }

    #[test]
    fn material_list_rejects_nonfinite_source_scalars() {
        for [shininess, transparency] in [[f32::NAN, 0.25], [0.5, f32::INFINITY]] {
            let mut bytes = 1_u32.to_le_bytes().to_vec();
            for color in [0_u32; 4] {
                bytes.extend_from_slice(&color.to_le_bytes());
            }
            bytes.extend_from_slice(&shininess.to_le_bytes());
            bytes.extend_from_slice(&transparency.to_le_bytes());
            // #215 bounds a nonfinite source scalar to its actual material row.
            let output = actual_material_reader(&bytes);
            let namespace = output.ir().native.namespace("fcstd").unwrap();
            assert_eq!(
                namespace
                    .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
                    .unwrap()
                    .len(),
                1
            );
            let entries = namespace
                .arena_as::<crate::native::EntryRecord>("entries")
                .unwrap();
            let slots = entries
                .iter()
                .flat_map(|entry| entry.contents())
                .find_map(|content| match content {
                    crate::native::entry_contents::EntryContents::Material { values, .. } => {
                        Some(values)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(slots.len(), 1); // The readable count establishes the original slot.
            assert!(slots[0].fixed.is_none());
            let failed = output
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| {
                    matches!(
                        part.state(),
                        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp { .. }
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(failed.len(), 1);
            assert_eq!(failed[0].level().as_str(), "freecad.material_list_material");
            assert_eq!(failed[0].byte_len(), 24);
        }
    }
}

#[cfg(test)]
mod shape_association_tests {
    use super::{displayed_shape_topology, select_shape_bodies};
    use crate::brep::{ShapePayload, ShapePayloadRecord};
    use crate::native::{PropertyFamily, PropertyRecord};

    fn shape_property(id: &str) -> PropertyRecord {
        PropertyRecord {
            dynamic: None,
            id: id.into(),
            owner: "object".into(),
            name: "Shape".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: PropertyFamily::Geometry,
            status: None,
            body: Some(crate::native::PropertyBody::Persisted {
                values: Default::default(),
                links: Vec::new(),
                side_entries: Vec::new(),
            }),
            order: 0,
        }
    }

    fn shape_payload(id: &str, property: &str) -> ShapePayloadRecord {
        ShapePayloadRecord {
            id: id.into(),
            rows: Vec::new(),
            property: property.into(),
            entry: "Shape.brp".into(),
            payload: ShapePayload::Empty,
        }
    }

    fn shape_ir() -> cadmpeg_ir::CadIr {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.bodies.push(cadmpeg_ir::topology::Body {
            native_ref: None,
            id: cadmpeg_ir::ids::BodyId::mint("fcstd:model:body#payload:1").expect("body identity"),
            kind: cadmpeg_ir::topology::BodyKind::default(),
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir
    }

    #[test]
    fn repeated_payload_body_selection_keeps_both_occurrences() {
        let ir = shape_ir();
        let body_ids = select_shape_bodies(&ir, ["payload", "payload"]);
        assert_eq!(
            body_ids,
            [ir.model.bodies[0].id.clone(), ir.model.bodies[0].id.clone()]
        );
    }

    #[test]
    fn topology_color_indices_ignore_transfer_order_and_require_complete_prefix() {
        let property = shape_property("property");
        let payload = shape_payload("payload", "property");
        let occurrence = |index, id: &str| crate::topology_transfer::TopologyOccurrence {
            property: "property".into(),
            indexed_name: "Face",
            source_index: index,
            topology_id: id.into(),
        };
        let occurrences = [
            occurrence(2, "second-face"),
            occurrence(1, "first-face"),
            occurrence(1, "first-face-other-body"),
        ];
        assert_eq!(
            displayed_shape_topology(
                "object",
                std::slice::from_ref(&property),
                std::slice::from_ref(&payload),
                &occurrences,
                "Face"
            )
            .unwrap(),
            vec![
                vec!["first-face", "first-face-other-body"],
                vec!["second-face"]
            ]
        );
        assert!(displayed_shape_topology(
            "object",
            &[property],
            &[payload],
            &occurrences[..1],
            "Face"
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn rejects_ambiguous_shape_association_candidates() {
        let property = shape_property("property");
        let payload = shape_payload("payload", "property");

        let duplicate_property = shape_property("property-2");
        assert!(matches!(
            displayed_shape_topology(
                "object",
                &[property.clone(), duplicate_property],
                std::slice::from_ref(&payload),
                &[],
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_payload = ShapePayloadRecord {
            id: "payload-2".into(),
            ..payload.clone()
        };
        assert!(matches!(
            displayed_shape_topology(
                "object",
                std::slice::from_ref(&property),
                &[payload.clone(), duplicate_payload],
                &[],
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[cfg(test)]
pub(crate) mod tests;
