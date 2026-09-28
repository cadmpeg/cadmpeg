// SPDX-License-Identifier: Apache-2.0
//! Transfer of `GuiDocument.xml` object appearance into neutral presentation records.

mod schema;

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
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
use crate::native::element_map::{ElementMapGroup, ElementMapRecord};
use crate::native::{
    copy_xml_text, parse_bool, GuiDocumentRecord, GuiPropertyRecord, GuiStateRecord,
    GuiViewProviderRecord, ObjectRecord, PropertyRecord, ValueRecord,
};
use crate::resource::{collection_vec, insert_hash_map, insert_hash_set, reserve_vec_items, reserved_vec, retained_join, retained_string};

use schema::Admission as GuiSchemaAdmission;

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
    color: Option<Color>,
}

fn push_body_update(
    ctx: &DecodeContext<'_>,
    plan: &mut AppearancePlan,
    id: &cadmpeg_ir::ids::BodyId,
    visible: Assignment<Option<bool>>,
    color: Result<Option<Color>, CodecError>,
) -> Result<(), CodecError> {
    let id = crate::resource::copied_identity(ctx, id.as_str(), "FCStd GUI body update identity")?;
    let color = color?;
    reserve_vec_items(ctx, &mut plan.body_updates, 1, "FCStd GUI body updates")?;
    plan.body_updates.push(BodyUpdate { id, visible, color });
    Ok(())
}

enum Assignment<T> {
    Keep,
    Set(T),
}

impl AppearancePlan {
    fn apply(self, ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
        for update in self.body_updates {
            if let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == update.id) {
                if let Assignment::Set(visible) = update.visible {
                    body.visible = visible;
                }
                body.color = update.color;
            }
        }
        ir.model
            .appearance_bindings
            .retain(|binding| !self.remove_appearances.contains(&binding.appearance));
        ir.model
            .appearances
            .retain(|appearance| !self.remove_appearances.contains(&appearance.id));
        reserve_vec_items(ctx, &mut ir.model.appearances, self.appearances.len(), "FCStd neutral appearances")?;
        ir.model.appearances.extend(self.appearances);
        reserve_vec_items(ctx, &mut ir.model.appearance_bindings, self.bindings.len(), "FCStd neutral appearance bindings")?;
        ir.model.appearance_bindings.extend(self.bindings);
        reserve_vec_items(ctx, &mut ir.model.presentation_documents, self.presentation_documents.len(), "FCStd neutral presentation documents")?;
        ir.model
            .presentation_documents
            .extend(self.presentation_documents);
        reserve_vec_items(ctx, &mut ir.model.view_presentations, self.view_presentations.len(), "FCStd neutral view presentations")?;
        ir.model.view_presentations.extend(self.view_presentations);
        Ok(())
    }
}

struct CameraSettings {
    position: Option<[f64; 3]>,
    orientation: Option<[f64; 4]>,
}

/// The admitted native provider identity is the source of every neutral
/// appearance key. The persisted display name remains separate so names such
/// as `A B`, `#`, and the empty string remain legal source values.
fn provider_identity_key(ctx: &DecodeContext<'_>, name: &str) -> Result<IdentityKey, CodecError> {
    crate::native::encoded_segment_charged(ctx, name, "FCStd GUI provider key")
}

fn object_appearance_id(
    ctx: &DecodeContext<'_>,
    provider: &IdentityKey,
) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(crate::resource::retained_format(
        ctx,
        format_args!("fcstd:appearance:object#{provider}"),
        "FCStd GUI object appearance identity",
    )?).map_err(CodecError::malformed)
}

fn edge_appearance_id(ctx: &DecodeContext<'_>, provider: &IdentityKey) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(crate::resource::retained_format(ctx,
        format_args!("fcstd:appearance:edge#{provider}"),
        "FCStd GUI edge appearance identity",
    )?).map_err(CodecError::malformed)
}

fn vertex_appearance_id(ctx: &DecodeContext<'_>, provider: &IdentityKey) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(crate::resource::retained_format(ctx,
        format_args!("fcstd:appearance:vertex#{provider}"),
        "FCStd GUI vertex appearance identity",
    )?).map_err(CodecError::malformed)
}

fn shape_material_appearance_id(
    ctx: &DecodeContext<'_>, provider: &IdentityKey, index: usize,
) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(crate::resource::retained_format(ctx,
        format_args!("fcstd:appearance:shape-material#{provider}:{}", index + 1),
        "FCStd GUI shape material appearance identity",
    )?).map_err(CodecError::malformed)
}

fn topology_appearance_id(
    ctx: &DecodeContext<'_>,
    kind: TopologyColorKind,
    provider: &IdentityKey,
    index: usize,
) -> Result<AppearanceId, CodecError> {
    let kind = topology_binding_kind(kind);
    AppearanceId::mint(crate::resource::retained_format(ctx,
        format_args!("fcstd:appearance:{kind}#{provider}:{}", index + 1),
        "FCStd GUI topology appearance identity",
    )?).map_err(CodecError::malformed)
}

fn topology_binding_kind(kind: TopologyColorKind) -> IdentityKey {
    match kind {
        TopologyColorKind::Face => cadmpeg_ir::identity_key!("face"),
        TopologyColorKind::Edge => cadmpeg_ir::identity_key!("edge"),
        TopologyColorKind::Vertex => cadmpeg_ir::identity_key!("vertex"),
    }
}

fn binding_id(ctx: &DecodeContext<'_>, text: std::fmt::Arguments<'_>) -> Result<AppearanceBindingId, CodecError> {
    AppearanceBindingId::mint(crate::resource::retained_format(
        ctx, text, "FCStd GUI appearance binding identity",
    )?).map_err(CodecError::malformed)
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
    ctx: &DecodeContext<'_>,
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
    ctx.charge_work(bytes.len() as u64, "FCStd GUI XML lexical admission")?;
    if let Some((nodes, _)) = crate::container::xml_envelope_counts(bytes) {
        ctx.charge_collection_items(nodes, "FCStd GUI XML node tree")?;
    }
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| gui_malformed(ctx, format_args!("invalid GuiDocument.xml: {error}")))?;
    let schema_declaration = crate::container::canonical_attribute(
        ctx,
        xml.root_element(),
        "SchemaVersion",
        "schemaVersion",
    )?;
    let admission = schema::classify(schema_declaration.as_deref());
    let neutral_schema_version = admission.neutral_schema_version();
    let transferred = transfer_schema_one(
        ctx,
        ir,
        text,
        &xml,
        schema_declaration.as_deref(),
        neutral_schema_version,
        entries,
        objects,
        properties,
        payloads,
        element_maps,
        requires_alpha_conversion,
    );
    match (admission, transferred) {
        (GuiSchemaAdmission::Schema1, result) => {
            let (graph, plan) = result?;
            plan.apply(ctx, ir)?;
            Ok(graph)
        }
        (GuiSchemaAdmission::Unverified { declaration }, Ok((mut graph, plan))) => {
            let declaration = declaration.as_deref().unwrap_or("missing");
            plan.apply(ctx, ir)?;
            reserve_vec_items(ctx, &mut graph.losses, 1, "FCStd GUI schema losses")?;
            graph.losses.push(FreecadLossCode::SourceGuiSchemaUnverified.note(
                crate::resource::retained_format(ctx, format_args!(
                    "GuiDocument.xml declares schema {declaration}; decoded with the schema-1 vocabulary"
                ), "FCStd GUI schema loss text")?,
            ));
            Ok(graph)
        }
        (
            GuiSchemaAdmission::Unverified { declaration },
            Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })),
        ) => {
            let declaration = declaration.as_deref().unwrap_or("missing");
            let mut losses = collection_vec(ctx, 1, "FCStd GUI schema losses")?;
            losses.push(FreecadLossCode::SourceGuiSchemaUnverified.note(
                crate::resource::retained_format(ctx, format_args!(
                    "GuiDocument.xml could not be decoded with the schema-1 vocabulary; declared schema {declaration} is the probable cause: {error}"
                ), "FCStd GUI schema loss text")?,
            ));
            Ok(Graph { losses, ..Graph::default() })
        }
        (GuiSchemaAdmission::Unverified { .. }, Err(error)) => Err(error),
    }
}

#[allow(clippy::too_many_arguments)]
fn transfer_schema_one(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    text: &str,
    xml: &roxmltree::Document<'_>,
    schema_declaration: Option<&str>,
    neutral_schema_version: Option<u32>,
    entries: &BTreeMap<String, View<'_>>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
    requires_alpha_conversion: bool,
) -> Result<(Graph, AppearancePlan), CodecError> {
    let root = xml.root_element();
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
    let state_count = root
        .children()
        .filter(roxmltree::Node::is_element)
        .filter(|node| !node.has_tag_name("ViewProviderData"))
        .count();
    let mut states = collection_vec(ctx, state_count, "FCStd GUI state records")?;
    for (order, node) in root
        .children()
        .filter(roxmltree::Node::is_element)
        .filter(|node| !node.has_tag_name("ViewProviderData"))
        .enumerate() {
        states.push(gui_state(ctx, text, order, node)?);
    }
    let document = GuiDocumentRecord {
        id: "fcstd:gui:document#0".into(),
        schema_version: schema_declaration
            .map(|value| copy_xml_text(Some(ctx), value, "FCStd GUI schema declaration"))
            .transpose()?,
        attributes: root
            .attributes()
            .map(|attribute| {
                ctx.charge_collection_items(1, "FCStd GUI document attributes")?;
                Ok((
                    copy_xml_text(
                        Some(ctx),
                        attribute.name(),
                        "FCStd GUI document attribute name",
                    )?,
                    copy_xml_text(Some(ctx), attribute.value(), "FCStd GUI document attribute")?,
                ))
            })
            .collect::<Result<_, CodecError>>()?,
        states,
    };
    let mut objects_by_name = HashMap::new();
    for object in objects {
        insert_hash_map(ctx, &mut objects_by_name, object.name.as_str(), object.id.as_str(), "FCStd GUI object names")?;
    }
    let mut native_providers = Vec::new();
    let mut native_properties = Vec::new();
    let mut losses = Vec::new();
    let mut payloads_by_owner = Vec::new();
    for payload in payloads {
        if let Some(property) = properties.iter().find(|property| property.id == payload.property) {
            reserve_vec_items(ctx, &mut payloads_by_owner, 1, "FCStd GUI payload owners")?;
            payloads_by_owner.push((
                property.owner.as_str(),
                property.name.as_str(),
                payload.id.as_str(),
            ));
        }
    }
    let mut view_provider_data = xml
        .descendants()
        .filter(|node| node.has_tag_name("ViewProviderData"));
    let first_view_provider_data = view_provider_data.next();
    if view_provider_data.next().is_some() {
        return Err(CodecError::Malformed(
            "GuiDocument.xml has multiple ViewProviderData containers".into(),
        ));
    }
    let provider_count = xml
        .descendants()
        .filter(|node| node.has_tag_name("ViewProvider"))
        .count();
    ctx.charge_collection_items(provider_count as u64, "FCStd GUI provider nodes")?;
    let mut providers = reserved_vec(ctx, provider_count, "FCStd GUI provider nodes")?;
    providers.extend(xml
        .descendants()
        .filter(|node| node.has_tag_name("ViewProvider")));
    if let Some(container) = first_view_provider_data {
        let declared = container
            .attribute("Count")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| CodecError::Malformed("invalid ViewProviderData Count".into()))?;
        if declared != providers.len() {
            return Err(gui_malformed(ctx, format_args!(
                "ViewProviderData Count={declared} but {} records were found",
                providers.len()
            )));
        }
    }
    for (provider_order, provider) in providers.into_iter().enumerate() {
        let Some(name) = provider.attribute("name") else {
            return Err(CodecError::Malformed("ViewProvider has no name".into()));
        };
        ctx.charge_work(native_providers.len() as u64, "FCStd GUI duplicate provider scan")?;
        if native_providers.iter().any(|record: &GuiViewProviderRecord| record.name == name) {
            return Err(CodecError::Malformed(
                "GuiDocument.xml has duplicate ViewProvider names".into(),
            ));
        }
        let provider_key = provider_identity_key(ctx, name)?;
        let Some(object_id) = objects_by_name.get(name).copied() else {
            append_native_provider(
                ctx,
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
            ctx,
            text,
            provider,
            provider_order,
            Some(object_id),
            &mut native_providers,
            &mut native_properties,
        )?;
        let properties_node = unique_child(provider, "Properties")?.ok_or_else(|| {
            gui_malformed(ctx, format_args!("ViewProvider {name} has no Properties"))
        })?;
        let property_count = properties_node.children().filter(|node| node.has_tag_name("Property")).count();
        let mut property_nodes = collection_vec(ctx, property_count, "FCStd GUI presentation property nodes")?;
        property_nodes.extend(properties_node
            .children()
            .filter(|node| node.has_tag_name("Property")));
        let mut values = HashMap::new();
        for property in property_nodes
            .iter()
            .copied()
            .filter(|property| {
                property
                    .attribute("name")
                    .and_then(presentation_property_type)
                    .is_some_and(|expected| property.attribute("type") == Some(expected))
            })
            {
            if let (Some(name), Some(value)) = (
                property.attribute("name"), property.children().find(roxmltree::Node::is_element),
            ) {
                insert_hash_map(ctx, &mut values, name, value, "FCStd GUI presentation values")?;
            }
        }
        let property_provenance = |property_name: &str, type_name: &str| {
            let offset = property_nodes
                .iter()
                .find(|property| {
                    property.attribute("name") == Some(property_name)
                        && property.attribute("type") == Some(type_name)
                })
                .map_or(0, |property| property.range().start as u64);
            gui_provider_property_provenance(ctx, name, property_name, offset)
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
        let body_ids = select_shape_bodies(ctx, ir, payloads_by_owner.iter()
            .filter(|(owner, property, _)| *owner == object_id && *property == "Shape")
            .map(|(_, _, payload)| *payload))?;
        for body_id in &body_ids {
            push_body_update(ctx, &mut plan, body_id, Assignment::Set(visibility),
                packed_color
                    .map(|packed| decode_color(packed, transparency))
                    .transpose())?;
        }
        if let Some(file) = values
            .get("DiffuseColor")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ctx,
                ir,
                &mut plan,
                name,
                &provider_key,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Face,
                requires_alpha_conversion,
                property_provenance("DiffuseColor", "App::PropertyColorList")?,
                &mut losses,
            )?;
        }
        let payload_prefixes = shape_payload_prefixes(ctx, &payloads_by_owner, object_id)?;
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
                ctx,
                ir,
                &mut plan,
                &mut losses,
                PrimitiveAppearanceSource {
                    provider_name: name,
                    object_id,
                    packed_color: color,
                    style: PrimitiveStyle::Line(PrimitiveSize::from_source(width)),
                    payload_prefixes: &payload_prefixes,
                    provenance: property_provenance("LineWidth", "App::PropertyFloatConstraint")?,
                },
            )?;
        }
        if let Some(file) = values
            .get("LineColorArray")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ctx,
                ir,
                &mut plan,
                name,
                &provider_key,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Edge,
                requires_alpha_conversion,
                property_provenance("LineColorArray", "App::PropertyColorList")?,
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
                .and_then(|value| value.attribute("value"));
            transfer_primitive_appearance(
                ctx,
                ir,
                &mut plan,
                &mut losses,
                PrimitiveAppearanceSource {
                    provider_name: name,
                    object_id,
                    packed_color: color,
                    style: PrimitiveStyle::Point(PrimitiveSize::from_source(size)),
                    payload_prefixes: &payload_prefixes,
                    provenance: property_provenance("PointSize", "App::PropertyFloatConstraint")?,
                },
            )?;
        }
        if let Some(file) = values
            .get("PointColorArray")
            .and_then(|value| value.attribute("file"))
        {
            transfer_topology_colors(
                ctx,
                ir,
                &mut plan,
                name,
                &provider_key,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
                TopologyColorKind::Vertex,
                requires_alpha_conversion,
                property_provenance("PointColorArray", "App::PropertyColorList")?,
                &mut losses,
            )?;
        }
        let Some(packed_color) = packed_color else {
            continue;
        };
        let appearance_id = object_appearance_id(ctx, &provider_key)?;
        let mut material_properties = BTreeMap::new();
        if let Some(material) = material {
            for (source, target) in [
                ("shininess", cadmpeg_core::nonblank_literal!("shininess")),
                (
                    "transparency",
                    cadmpeg_core::nonblank_literal!("material_transparency"),
                ),
            ] {
                if let Some(value) = material
                    .attribute(source)
                    .and_then(|value| value.parse::<f64>().ok())
                    .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                {
                    ctx.charge_collection_items(1, "FCStd GUI material properties")?;
                    material_properties.insert(target, value);
                }
            }
        }
        reserve_vec_items(ctx, &mut plan.appearances, 1, "FCStd GUI planned appearances")?;
        plan.appearances.push(Appearance {
            id: crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI appearance identity copy")?,
            name: Some(crate::resource::retained_format(ctx,
                format_args!("{name} shape appearance"), "FCStd GUI appearance name")?),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("FCStd ViewProvider ShapeMaterial".into()),
            category: None,
            base_color: Some(decode_color(packed_color, transparency)?),
            textures: Vec::new(),
            properties: material_properties,
        });
        for (index, body) in body_ids.into_iter().enumerate() {
            reserve_vec_items(ctx, &mut plan.bindings, 1, "FCStd GUI planned bindings")?;
            plan.bindings.push(AppearanceBinding {
                id: binding_id(ctx, format_args!("fcstd:appearance:binding#{provider_key}:{index}"))?,
                target: AppearanceTarget::Body(body),
                appearance: crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI binding appearance identity")?,
                source_entity_id: Some(retained_string(ctx, object_id, "FCStd GUI binding source identity")?),
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
        validate_gui_list_payloads(ctx, &graph.properties, entries, requires_alpha_conversion)?;
    let mut material_losses = Vec::new();
    transfer_shape_appearances(
        ctx,
        ir,
        &mut plan,
        &graph,
        &material_lists,
        properties,
        payloads,
        element_maps,
        &mut material_losses,
    )?;
    append_graph_losses(ctx, &mut graph, material_losses)?;
    let mut presentation_losses = Vec::new();
    transfer_neutral_presentation(
        ctx,
        &mut plan,
        &graph,
        neutral_schema_version,
        &mut presentation_losses,
    )?;
    append_graph_losses(ctx, &mut graph, presentation_losses)?;
    Ok((graph, plan))
}

fn append_graph_losses(
    ctx: &DecodeContext<'_>,
    graph: &mut Graph,
    losses: Vec<LossNote>,
) -> Result<(), CodecError> {
    reserve_vec_items(ctx, &mut graph.losses, losses.len(), "FCStd GUI graph losses")?;
    graph.losses.extend(losses);
    Ok(())
}

fn push_gui_appearance_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: FreecadLossCode,
    message: std::fmt::Arguments<'_>,
    provenance: SourceProvenance,
    collection_operation: &'static str,
    text_operation: &'static str,
) -> Result<(), CodecError> {
    reserve_vec_items(ctx, losses, 1, collection_operation)?;
    let text = crate::resource::retained_format(ctx, message, text_operation)?;
    losses.push(code.note(text).with_provenance(provenance));
    Ok(())
}

fn gui_provider_property_provenance(
    ctx: &DecodeContext<'_>,
    provider_name: &str,
    property_name: &str,
    offset: u64,
) -> Result<SourceProvenance, CodecError> {
    let tag = crate::resource::retained_format(ctx,
        format_args!("ViewProvider {provider_name} property {property_name}"),
        "FCStd GUI property provenance tag")?;
    Ok(SourceProvenance::in_stream(
        "fcstd", cadmpeg_ir::stream_name!("GuiDocument.xml"), offset,
    ).with_tag(tag))
}

fn gui_malformed(ctx: &DecodeContext<'_>, message: std::fmt::Arguments<'_>) -> CodecError {
    crate::resource::malformed_charged(ctx, message, "FCStd GUI diagnostic")
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
    ctx: &DecodeContext<'_>,
    record: impl Fn() -> Result<String, CodecError>,
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(BTreeMap<cadmpeg_core::text::NonBlankString, String>, Vec<cadmpeg_core::text::NamedEntryError>), CodecError> {
    use cadmpeg_core::text::{NamedEntryError, NonBlankString};
    let mut kept = BTreeMap::new();
    let mut refused = Vec::new();
    for (name, value) in entries {
        let name = retained_string(ctx, name, "FCStd GUI presentation property name")?;
        let value = retained_string(ctx, value, "FCStd GUI presentation property value")?;
        match NonBlankString::new(name) {
            Some(key) if kept.contains_key(&key) => {
                reserve_vec_items(ctx, &mut refused, 1, "FCStd GUI refused property keys")?;
                refused.push(NamedEntryError::Restated {
                    record: record()?,
                    key: NonBlankString::new(retained_string(ctx, key.as_str(), "FCStd GUI restated property key")?)
                        .ok_or_else(|| CodecError::malformed("restated GUI property key became blank"))?,
                });
            }
            Some(key) => {
                ctx.charge_collection_items(1, "FCStd GUI presentation property map")?;
                kept.insert(key, value);
            }
            None => {
                reserve_vec_items(ctx, &mut refused, 1, "FCStd GUI refused property keys")?;
                refused.push(NamedEntryError::Blank { record: record()? });
            }
        }
    }
    Ok((kept, refused))
}

/// Transfers the GUI graph's presentation layer into `plan`.
///
/// A property whose key is blank is charged to `losses` and the rest of the
/// property set survives: a blank key is one unreadable property of one record,
/// not a reason to answer no GUI presentation at all.
fn transfer_neutral_presentation(
    ctx: &DecodeContext<'_>,
    plan: &mut AppearancePlan,
    graph: &Graph,
    neutral_schema_version: Option<u32>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let mut state_losses = Vec::new();
    for document in &graph.documents {
        let mut presentation = PresentationDocument::new(PresentationId::compose(
            &cadmpeg_ir::identity_namespace!("fcstd", "presentation", "document"),
            cadmpeg_ir::identity_key!("0"),
        ));
        presentation.schema_version = neutral_schema_version;
        presentation.native_ref = Some(retained_string(ctx, &document.id, "FCStd presentation document reference")?);
        let mut states = collection_vec(ctx, document.states.len(), "FCStd presentation states")?;
        for (order, state) in document.states.iter().enumerate() {
            let (attributes, refused) = gui_named_entries(
                ctx,
                || retained_join(ctx, &["the gui ", state.kind.as_str(), " state"], "", "FCStd GUI state record name"),
                state.attributes.iter().map(|(name, value)| (name.as_str(), value.as_str())),
            )?;
            charge_refused_gui_keys(ctx, &mut state_losses, &refused)?;
            let kind = if state.kind == "Camera" {
                PresentationStateKind::Camera(camera_state_value(ctx, state, &mut state_losses)?)
            } else {
                PresentationStateKind::Native(retained_string(ctx, &state.kind, "FCStd presentation state kind")?)
            };
            let mut assets = collection_vec(ctx, state.side_entries.len(), "FCStd presentation assets")?;
            for entry in &state.side_entries {
                assets.push(crate::native::native_id_charged(ctx, "entry", entry)?);
            }
            states.push(PresentationState {
                kind,
                order: u32::try_from(order)
                    .map_err(|_| CodecError::malformed("GUI state order exceeds u32"))?,
                attributes,
                assets,
            });
        }
        ctx.charge_work(states.len() as u64, "FCStd presentation state order")?;
        presentation
            .set_states(states)
            .map_err(CodecError::malformed)?;
        reserve_vec_items(ctx, &mut plan.presentation_documents, 1, "FCStd presentation documents")?;
        plan.presentation_documents.push(presentation);
    }
    reserve_vec_items(ctx, losses, state_losses.len(), "FCStd presentation losses")?;
    losses.append(&mut state_losses);

    let mut properties = HashMap::<&str, Vec<&GuiPropertyRecord>>::new();
    for property in &graph.properties {
        if !properties.contains_key(property.owner.as_str()) {
            insert_hash_map(ctx, &mut properties, property.owner.as_str(), Vec::new(), "FCStd presentation property owners")?;
        }
        if let Some(owned) = properties.get_mut(property.owner.as_str()) {
            reserve_vec_items(ctx, owned, 1, "FCStd presentation owner properties")?;
            owned.push(property);
        }
    }
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
        let line_width = line_width
            .map(|value| {
                cadmpeg_ir::scalar::NonNegativeReal::new(value).ok_or_else(|| {
                    CodecError::malformed("line_width must be finite and nonnegative")
                })
            })
            .transpose()?;
        let point_size = point_size
            .map(|value| {
                cadmpeg_ir::scalar::NonNegativeReal::new(value).ok_or_else(|| {
                    CodecError::malformed("point_size must be finite and nonnegative")
                })
            })
            .transpose()?;
        let (provider_properties, refused) = gui_named_entries(
            ctx,
            || retained_string(ctx, &provider.id, "FCStd GUI provider record name"),
            owned.iter().map(|property| (
                property.name.as_str(),
                gui_property_value(property).unwrap_or_else(|| property.xml.text()),
            )),
        )?;
        charge_refused_gui_keys(ctx, losses, &refused)?;
        reserve_vec_items(ctx, &mut plan.view_presentations, 1, "FCStd view presentations")?;
        plan.view_presentations.push(ViewPresentation {
            id: PresentationId::compose(
                &cadmpeg_ir::identity_namespace!("fcstd", "model", "presentation-view"),
                crate::native::model_key(&provider.id, "state").map_err(CodecError::malformed)?,
            ),
            object: provider
                .object
                .as_ref()
                .map(|object| retained_string(ctx, object.as_str(), "FCStd view object identity"))
                .transpose()?,
            order: provider.order as u32,
            expanded: provider.expanded,
            visible: property_value("Visibility", "App::PropertyBool").and_then(parse_bool),
            display_mode: property_value("DisplayMode", "App::PropertyEnumeration")
                .map(|value| retained_string(ctx, value, "FCStd view display mode"))
                .transpose()?,
            selection_style: property_value("SelectionStyle", "App::PropertyEnumeration")
                .map(|value| retained_string(ctx, value, "FCStd view selection style"))
                .transpose()?,
            line_width,
            point_size,
            properties: provider_properties,
            native_ref: Some(retained_string(ctx, &provider.id, "FCStd view native reference")?),
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

/// One loss per property key the reader could not key, naming the key's own
/// record and, for a restated key, the key.
fn charge_refused_gui_keys(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    refused: &[cadmpeg_core::text::NamedEntryError],
) -> Result<(), CodecError> {
    use cadmpeg_core::text::NamedEntryError;
    for key in refused {
        let message = match key {
            NamedEntryError::Blank { record } => retained_join(ctx,
                &[record.as_str(), " states a property with a blank key; the property value is not transferred"],
                "", "FCStd GUI refused property note")?,
            NamedEntryError::Restated { record, key } => retained_join(ctx,
                &[record.as_str(), " states the property ", key.as_str(),
                    " a second time; the property value is not transferred"],
                "", "FCStd GUI refused property note")?,
        };
        reserve_vec_items(ctx, losses, 1, "FCStd GUI refused property losses")?;
        losses.push(
            FreecadLossCode::SourceGuiPropertyKeyBlank
                .note(message),
        );
    }
    Ok(())
}

fn camera_state_value(
    ctx: &DecodeContext<'_>,
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
    } = parse_camera_settings(ctx, settings)?;
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
        ctx,
        || retained_join(ctx, &["the gui ", state.kind.as_str(), " state"], "", "FCStd GUI state record name"),
        state.attributes.iter().map(|(name, value)| (name.as_str(), value.as_str())),
    )?;
    charge_refused_gui_keys(ctx, losses, &refused)?;
    Ok(CameraState {
        position,
        orientation,
        properties,
    })
}

fn parse_camera_settings(ctx: &DecodeContext<'_>, settings: &str) -> Result<CameraSettings, CodecError> {
    if settings.trim().is_empty() {
        return Ok(CameraSettings {
            position: None,
            orientation: None,
        });
    }

    let mut tokens = collection_vec(ctx, settings.split_whitespace().count(), "FCStd GUI camera tokens")?;
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
                position = Some(camera_field::<3>(ctx, &tokens, index + 1, end, "position")?);
                index += 4;
            }
            "orientation" => {
                if orientation.is_some() {
                    return Err(CodecError::Malformed(
                        "GUI camera settings have multiple orientation fields".into(),
                    ));
                }
                orientation = Some(camera_field::<4>(ctx, &tokens, index + 1, end, "orientation")?);
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
    ctx: &DecodeContext<'_>,
    tokens: &[&str],
    start: usize,
    end: usize,
    field: &str,
) -> Result<[f64; N], CodecError> {
    let end_index = start.checked_add(N).ok_or_else(|| {
        gui_malformed(ctx, format_args!("GUI camera {field} field offset overflows"))
    })?;
    let values = tokens.get(start..end_index)
        .filter(|values| values.len() == N && end_index <= end)
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!("GUI camera {field} field is incomplete"))
        })?;
    let mut parsed = [0.0; N];
    for (index, value) in values.iter().enumerate() {
        parsed[index] = value.parse::<f64>().map_err(|_| {
            gui_malformed(ctx, format_args!("GUI camera {field} field is not numeric"))
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
    packed_color: u32,
    style: PrimitiveStyle,
    payload_prefixes: &'a [String],
    provenance: SourceProvenance,
}

fn shape_payload_prefixes(
    ctx: &DecodeContext<'_>,
    payloads_by_owner: &[(&str, &str, &str)],
    object_id: &str,
) -> Result<Vec<String>, CodecError> {
    let mut prefixes = Vec::new();
    for (_, _, payload) in payloads_by_owner.iter()
        .filter(|(owner, property, _)| *owner == object_id && *property == "Shape") {
        reserve_vec_items(ctx, &mut prefixes, 1, "FCStd GUI payload prefixes")?;
        prefixes.push(crate::resource::retained_suffix(ctx,
            crate::native::id_key(payload), ":", "FCStd GUI payload prefix text")?);
    }
    Ok(prefixes)
}

fn transfer_primitive_appearance(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    plan: &mut AppearancePlan,
    losses: &mut Vec<LossNote>,
    source: PrimitiveAppearanceSource<'_>,
) -> Result<(), CodecError> {
    let PrimitiveAppearanceSource {
        provider_name,
        object_id,
        packed_color,
        style,
        payload_prefixes,
        provenance,
    } = source;
    let mut targets = Vec::new();
    match style {
        PrimitiveStyle::Line(_) => {
            for edge in ir.model.edges.iter().filter(|edge| payload_prefixes.iter()
                .any(|prefix| crate::native::id_key(edge.id.as_str()).starts_with(prefix))) {
                reserve_vec_items(ctx, &mut targets, 1, "FCStd GUI primitive targets")?;
                targets.push(AppearanceTarget::Edge(crate::resource::copied_identity(
                    ctx, edge.id.as_str(), "FCStd GUI primitive target identity",
                )?));
            }
        }
        PrimitiveStyle::Point(_) => {
            for vertex in ir.model.vertices.iter().filter(|vertex| payload_prefixes.iter()
                .any(|prefix| crate::native::id_key(vertex.id.as_str()).starts_with(prefix))) {
                reserve_vec_items(ctx, &mut targets, 1, "FCStd GUI primitive targets")?;
                targets.push(AppearanceTarget::Vertex(crate::resource::copied_identity(
                    ctx, vertex.id.as_str(), "FCStd GUI primitive target identity",
                )?));
            }
        }
    }
    if targets.is_empty() {
        return Ok(());
    }
    let provider_key = provider_identity_key(ctx, provider_name)?;
    let (appearance_id, label, property, size, binding_key, object_type, precedence) = match style {
        PrimitiveStyle::Line(width) => (
            edge_appearance_id(ctx, &provider_key)?,
            "line",
            cadmpeg_core::nonblank_literal!("line_width"),
            width,
            cadmpeg_ir::identity_key!("edge"),
            "ViewProvider Edge",
            "edge_over_object",
        ),
        PrimitiveStyle::Point(size) => (
            vertex_appearance_id(ctx, &provider_key)?,
            "point",
            cadmpeg_core::nonblank_literal!("point_size"),
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
        push_gui_appearance_loss(ctx, losses,
            FreecadLossCode::AppearancePrimitiveSizeNotTransferred,
            format_args!("FCStd provider {provider_name} {label} size cannot enter the neutral appearance"),
            provenance, "FCStd GUI primitive size losses", "FCStd GUI primitive size loss text")?;
    }
    reserve_vec_items(ctx, &mut plan.appearances, 1, "FCStd GUI planned appearances")?;
    plan.appearances.push(Appearance {
        id: crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI appearance identity copy")?,
        name: Some(crate::resource::retained_format(ctx,
            format_args!("{provider_name} {label} appearance"), "FCStd GUI appearance name")?),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some(crate::resource::retained_format(ctx,
            format_args!("FCStd ViewProvider {label} style"), "FCStd GUI appearance schema")?),
        category: None,
        base_color: Some(Color::from_rgba8(
            (packed_color >> 24) as u8,
            (packed_color >> 16) as u8,
            (packed_color >> 8) as u8,
            packed_color as u8,
        )),
        textures: Vec::new(),
        properties: admitted_size
            .map(|width| [(property, width)].into())
            .unwrap_or_default(),
    });
    for (index, target) in targets.into_iter().enumerate() {
        reserve_vec_items(ctx, &mut plan.bindings, 1, "FCStd GUI planned bindings")?;
        plan.bindings.push(AppearanceBinding {
            id: binding_id(ctx, format_args!(
                "fcstd:appearance:binding#{binding_key}:{provider_key}:{index}"
            ))?,
            target,
            appearance: crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI binding appearance identity")?,
            source_entity_id: Some(retained_string(ctx, object_id, "FCStd GUI binding source identity")?),
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

fn gui_state(
    ctx: &DecodeContext<'_>,
    text: &str,
    order: usize,
    node: roxmltree::Node<'_, '_>,
) -> Result<GuiStateRecord, CodecError> {
    let value_count = node
        .descendants()
        .filter(|value| value.is_element() && *value != node)
        .count();
    let mut values = collection_vec(ctx, value_count, "FCStd GUI state values")?;
    for (value_order, value) in node.descendants()
        .filter(|value| value.is_element() && *value != node)
        .enumerate() {
        values.push(ValueRecord {
                tag: copy_xml_text(Some(ctx), value.tag_name().name(), "FCStd GUI value tag")?,
                order: value_order,
                attributes: value
                    .attributes()
                    .map(|attribute| {
                        ctx.charge_collection_items(1, "FCStd GUI value attributes")?;
                        Ok((
                            copy_xml_text(Some(ctx), attribute.name(), "FCStd GUI attribute name")?,
                            copy_xml_text(Some(ctx), attribute.value(), "FCStd GUI attribute")?,
                        ))
                    })
                    .collect::<Result<_, CodecError>>()?,
                text: value
                    .text()
                    .map(|text| copy_xml_text(Some(ctx), text, "FCStd GUI value text"))
                    .transpose()?,
                raw_xml: copy_xml_text(Some(ctx), &text[value.range()], "FCStd GUI value XML")?,
            });
    }
    let mut side_entries = Vec::new();
    for value in node
        .descendants()
        .filter(roxmltree::Node::is_element)
        .flat_map(|element| element.attributes())
        .filter(|attribute| matches!(attribute.name(), "file" | "File"))
        .map(|attribute| attribute.value())
        .filter(|value| !value.is_empty()) {
        reserve_vec_items(ctx, &mut side_entries, 1, "FCStd GUI side entry references")?;
        side_entries.push(copy_xml_text(Some(ctx), value, "FCStd GUI side entry name")?);
    }
    let kind = copy_xml_text(Some(ctx), node.tag_name().name(), "FCStd GUI state kind")?;
    let attributes = node
            .attributes()
            .map(|attribute| {
                ctx.charge_collection_items(1, "FCStd GUI state attributes")?;
                Ok((
                    copy_xml_text(
                        Some(ctx),
                        attribute.name(),
                        "FCStd GUI state attribute name",
                    )?,
                    copy_xml_text(Some(ctx), attribute.value(), "FCStd GUI state attribute")?,
                ))
            })
            .collect::<Result<_, CodecError>>()?;
    let xml = crate::native::RetainedXml::from_source(
            Some(ctx),
            &text[node.range()],
            node.range().start as u64,
            "FCStd GUI state XML",
        )?;
    let order = order.to_string();
    let key = retained_join(ctx, &[kind.as_str(), order.as_str()], ":", "FCStd GUI state identity key")?;
    Ok(GuiStateRecord {
        id: crate::native::native_id_charged(ctx, "gui-state", &key)?,
        kind,
        attributes,
        values,
        side_entries,
        xml,
    })
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
    ctx: &DecodeContext<'_>,
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
    let id = crate::native::native_id_charged(ctx, "gui-view-provider", name)?;
    reserve_vec_items(ctx, providers, 1, "FCStd GUI provider records")?;
    providers.push(GuiViewProviderRecord {
        id: retained_string(ctx, &id, "FCStd GUI provider record identity")?,
        object: object
            .map(|object| {
                cadmpeg_core::text::NonBlankString::new(retained_string(
                    ctx, object, "FCStd GUI provider object identity",
                )?).ok_or_else(|| {
                    CodecError::Malformed("GUI provider object must not be empty".into())
                })
            })
            .transpose()?,
        name: copy_xml_text(Some(ctx), name, "FCStd GUI provider name")?,
        expanded: provider.attribute("expanded").and_then(parse_bool),
        order,
        raw_xml: copy_xml_text(Some(ctx), &text[provider.range()], "FCStd GUI provider XML")?,
    });
    let Some(container) = unique_child(provider, "Properties")? else {
        return Err(gui_malformed(ctx, format_args!(
            "ViewProvider {name} has no Properties"
        )));
    };
    let property_nodes = container
        .children()
        .filter(|node| node.has_tag_name("Property"))
        .count();
    let mut nodes = collection_vec(ctx, property_nodes, "FCStd GUI provider property nodes")?;
    nodes.extend(container
        .children()
        .filter(|node| node.has_tag_name("Property")));
    let property_nodes = nodes;
    let declared = container
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "ViewProvider {name} has invalid property count"
            ))
        })?;
    if declared != property_nodes.len() {
        return Err(gui_malformed(ctx, format_args!(
            "ViewProvider {name} declares {declared} properties but contains {}",
            property_nodes.len()
        )));
    }
    for (property_order, property) in property_nodes.into_iter().enumerate() {
        let property_name = property.attribute("name").ok_or_else(|| {
            gui_malformed(ctx, format_args!("ViewProvider {name} property has no name"))
        })?;
        ctx.charge_work(property_order as u64, "FCStd GUI duplicate property scan")?;
        if properties.iter().rev().take(property_order)
            .any(|record| record.owner == id && record.name == property_name) {
            return Err(CodecError::Malformed(
                "ViewProvider has duplicate property names".into(),
            ));
        }
        let type_name = property.attribute("type").ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "ViewProvider {name}.{property_name} has no type"
            ))
        })?;
        validate_gui_property(ctx, property, property_name, type_name)?;
        let value_count = property
            .descendants()
            .filter(|value| value.is_element() && *value != property)
            .count();
        let mut values = collection_vec(ctx, value_count, "FCStd GUI property values")?;
        for (value_order, value) in property.descendants()
            .filter(|value| value.is_element() && *value != property)
            .enumerate() {
            values.push(ValueRecord {
                    tag: copy_xml_text(Some(ctx), value.tag_name().name(), "FCStd GUI value tag")?,
                    order: value_order,
                    attributes: value
                        .attributes()
                        .map(|attribute| {
                            ctx.charge_collection_items(1, "FCStd GUI value attributes")?;
                            Ok((
                                copy_xml_text(
                                    Some(ctx),
                                    attribute.name(),
                                    "FCStd GUI attribute name",
                                )?,
                                copy_xml_text(Some(ctx), attribute.value(), "FCStd GUI attribute")?,
                            ))
                        })
                        .collect::<Result<_, CodecError>>()?,
                    text: value
                        .text()
                        .map(|text| copy_xml_text(Some(ctx), text, "FCStd GUI value text"))
                        .transpose()?,
                    raw_xml: copy_xml_text(Some(ctx), &text[value.range()], "FCStd GUI value XML")?,
                });
        }
        let mut side_entries = Vec::new();
        for value in values
            .iter()
            .flat_map(|value| value.attributes.iter())
            .filter(|(attribute, _)| {
                matches!(attribute.as_str(), "file" | "File")
                    && !crate::persistence::is_xlink_type(type_name)
            })
            .map(|(_, value)| value.as_str())
            .filter(|value| !value.is_empty()) {
            reserve_vec_items(ctx, &mut side_entries, 1, "FCStd GUI side entry references")?;
            side_entries.push(copy_xml_text(Some(ctx), value, "FCStd GUI side entry name")?);
        }
        reserve_vec_items(ctx, properties, 1, "FCStd GUI property records")?;
        properties.push(GuiPropertyRecord {
            id: crate::native::native_child_id_charged(ctx, "gui-property", &id, property_name)?,
            owner: copy_xml_text(Some(ctx), &id, "FCStd GUI property owner")?,
            name: copy_xml_text(Some(ctx), property_name, "FCStd GUI property name")?,
            type_name: copy_xml_text(Some(ctx), type_name, "FCStd GUI property type")?,
            status: property
                .attribute("status")
                .and_then(|value| value.parse().ok()),
            order: property_order,
            values,
            side_entries,
            xml: crate::native::RetainedXml::from_source(
                Some(ctx),
                &text[property.range()],
                property.range().start as u64,
                "FCStd GUI property XML",
            )?,
        });
    }
    Ok(())
}

fn validate_gui_property(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
    type_name: &str,
) -> Result<(), CodecError> {
    if is_visual_layer_list(property_name, type_name) {
        return validate_visual_layer_list(ctx, property, property_name);
    }
    if is_gui_link_type(type_name) {
        return crate::persistence::validate_link_property(property, type_name);
    }
    match type_name {
        "Mesh::PropertyMeshKernel" => {
            return validate_gui_geometry_value(ctx, property, property_name, "Mesh");
        }
        "Points::PropertyPointKernel" => {
            return validate_gui_geometry_value(ctx, property, property_name, "Points");
        }
        "TechDraw::PropertyGeomFormatList" => {
            return validate_gui_techdraw_list(
                ctx,
                property,
                property_name,
                "GeomFormatList",
                "GeomFormat",
                validate_gui_geom_format_record,
            );
        }
        "TechDraw::PropertyCosmeticVertexList" => {
            return validate_gui_techdraw_list(
                ctx,
                property,
                property_name,
                "CosmeticVertexList",
                "CosmeticVertex",
                validate_gui_cosmetic_vertex_record,
            );
        }
        "TechDraw::PropertyCosmeticEdgeList" => {
            return validate_gui_techdraw_list(
                ctx,
                property,
                property_name,
                "CosmeticEdgeList",
                "CosmeticEdge",
                validate_gui_cosmetic_edge_record,
            );
        }
        "TechDraw::PropertyCenterLineList" => {
            return validate_gui_techdraw_list(
                ctx,
                property,
                property_name,
                "CenterLineList",
                "CenterLine",
                validate_gui_center_line_record,
            );
        }
        "App::PropertyExpressionEngine" => {
            return validate_gui_expression_engine(ctx, property, property_name);
        }
        "Materials::PropertyMaterial" => {
            return validate_gui_material_reference(ctx, property, property_name);
        }
        "Part::PropertyPartShape" => {
            return validate_gui_part_shape(ctx, property, property_name);
        }
        "Part::PropertyGeometryList" => {
            return validate_gui_geometry_list(ctx, property, property_name);
        }
        "Part::PropertyFilletEdges" => {
            return validate_gui_filletedges(ctx, property, property_name);
        }
        "Part::PropertyTopoShapeList" => {
            return validate_gui_shape_list(ctx, property, property_name);
        }
        "Sketcher::PropertyConstraintList" => {
            return validate_gui_constraint_list(ctx, property, property_name);
        }
        "Part::PropertyShapeHistory" | "Part::PropertyShapeCache" => return Ok(()),
        _ => {}
    }
    let Some(tag) = gui_value_tag(type_name) else {
        return Ok(());
    };
    let expected_tag = tag.as_str();
    let mut roots = property
        .children()
        .filter(roxmltree::Node::is_element);
    let root = roots.next().ok_or_else(|| {
        gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires one {expected_tag} value"
        ))
    })?;
    let second_root = roots.next();
    let has_more_roots = roots.next().is_some();
    if !root.has_tag_name(expected_tag) {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires a leading {expected_tag} value"
        )));
    }
    let scalar = |attribute: &str| {
        root.attribute(attribute).ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI property {property_name} {expected_tag} has no {attribute} attribute"
            ))
        })
    };
    match tag {
        GuiValueTag::Bool => {
            if parse_bool(scalar("value")?).is_none() {
                return Err(gui_malformed(ctx, format_args!(
                    "GUI property {property_name} has an invalid Boolean"
                )));
            }
        }
        GuiValueTag::Integer => {
            scalar("value")?.parse::<i64>().map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} has an invalid integer"
                ))
            })?;
            if is_gui_integer_constraint_type(type_name) {
                validate_gui_constraint_attributes(ctx, root, property_name, true)?;
            }
            if type_name == "App::PropertyEnumeration" {
                validate_gui_enumeration(ctx, root, second_root, has_more_roots, property_name)?;
                return Ok(());
            }
        }
        GuiValueTag::Float => {
            let value = scalar("value")?.parse::<f64>().map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} has an invalid float"
                ))
            })?;
            if !value.is_finite() {
                return Err(gui_malformed(ctx, format_args!(
                    "GUI property {property_name} has a non-finite float"
                )));
            }
            if is_gui_float_constraint_type(type_name) {
                validate_gui_constraint_attributes(ctx, root, property_name, false)?;
            }
        }
        GuiValueTag::String
        | GuiValueTag::Python
        | GuiValueTag::ColorList
        | GuiValueTag::MaterialList => {
            let attribute = if matches!(tag, GuiValueTag::ColorList | GuiValueTag::MaterialList) {
                "file"
            } else {
                "value"
            };
            scalar(attribute)?;
            if matches!(tag, GuiValueTag::ColorList | GuiValueTag::MaterialList)
                && has_nested_gui_elements(root)
            {
                return Err(gui_nested_value_error(ctx, property_name, expected_tag));
            }
            if type_name == "App::PropertyPersistentObject" {
                if has_more_roots
                    || !second_root.is_some_and(|node| node.has_tag_name("PersistentObject"))
                {
                    return Err(gui_malformed(ctx, format_args!(
                        "GUI property {property_name} has an invalid persistent-object envelope"
                    )));
                }
                return Ok(());
            }
            if tag == GuiValueTag::MaterialList {
                let version = root
                    .attribute("version")
                    .map(str::parse::<u32>)
                    .transpose()
                    .map_err(|_| {
                        gui_malformed(ctx, format_args!(
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
        GuiValueTag::PropertyColor => {
            scalar("value")?.parse::<u32>().map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} has an invalid color"
                ))
            })?;
        }
        GuiValueTag::PropertyVector => {
            for attribute in ["valueX", "valueY", "valueZ"] {
                let value = scalar(attribute)?.parse::<f64>().map_err(|_| {
                    gui_malformed(ctx, format_args!(
                        "GUI property {property_name} has an invalid vector"
                    ))
                })?;
                if !value.is_finite() {
                    return Err(gui_malformed(ctx, format_args!(
                        "GUI property {property_name} has a non-finite vector"
                    )));
                }
            }
        }
        GuiValueTag::PropertyMaterial => validate_gui_material(ctx, root, property_name)?,
        GuiValueTag::BoolList => {
            if !scalar("value")?
                .bytes()
                .all(|byte| matches!(byte, b'0' | b'1'))
            {
                return Err(gui_malformed(ctx, format_args!(
                    "GUI property {property_name} has an invalid Boolean list"
                )));
            }
            if has_nested_gui_elements(root) {
                return Err(gui_nested_value_error(ctx, property_name, "BoolList"));
            }
        }
        GuiValueTag::StringList => validate_gui_string_list(ctx, root, property_name)?,
        GuiValueTag::IntegerList => validate_gui_integer_list(ctx, root, property_name, false)?,
        GuiValueTag::IntegerSet => validate_gui_integer_list(ctx, root, property_name, true)?,
        GuiValueTag::Map => validate_gui_map(ctx, root, property_name)?,
        GuiValueTag::PropertyMatrix => {
            for row in 1..=4 {
                for column in 1..=4 {
                    let attribute = format!("a{row}{column}");
                    let value = scalar(&attribute)?.parse::<f64>().map_err(|_| {
                        gui_malformed(ctx, format_args!(
                            "GUI property {property_name} has an invalid matrix value"
                        ))
                    })?;
                    if !value.is_finite() {
                        return Err(gui_malformed(ctx, format_args!(
                            "GUI property {property_name} has a non-finite matrix value"
                        )));
                    }
                }
            }
        }
        GuiValueTag::PropertyPlacement => validate_gui_placement(ctx, root, property_name)?,
        GuiValueTag::PropertyRotation => {
            for attribute in ["A", "Ox", "Oy", "Oz"] {
                let value = scalar(attribute)?.parse::<f64>().map_err(|_| {
                    gui_malformed(ctx, format_args!(
                        "GUI property {property_name} has an invalid rotation"
                    ))
                })?;
                if !value.is_finite() {
                    return Err(gui_malformed(ctx, format_args!(
                        "GUI property {property_name} has a non-finite rotation"
                    )));
                }
            }
        }
        GuiValueTag::Uuid | GuiValueTag::Path => {
            scalar("value")?;
        }
        GuiValueTag::FloatList | GuiValueTag::VectorList | GuiValueTag::PlacementList => {
            scalar("file")?;
            if has_nested_gui_elements(root) {
                return Err(gui_nested_value_error(ctx, property_name, expected_tag));
            }
        }
        GuiValueTag::FileIncluded => {
            let has_file = root.attribute("file").is_some();
            let has_data = root.attribute("data").is_some();
            if has_file == has_data {
                return Err(gui_malformed(ctx, format_args!(
                    "GUI property {property_name} FileIncluded requires exactly one file or data attribute"
                )));
            }
            if has_nested_gui_elements(root) {
                return Err(gui_nested_value_error(ctx, property_name, "FileIncluded"));
            }
        }
    }
    if second_root.is_some() {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires exactly one {expected_tag} value"
        )));
    }
    Ok(())
}

fn validate_gui_string_list(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let count = gui_list_count(ctx, root, property_name, "StringList")?;
    if root.children().filter(roxmltree::Node::is_element).count() != count
        || root.children().filter(roxmltree::Node::is_element)
            .any(|value| !value.has_tag_name("String") || value.attribute("value").is_none())
    {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} StringList count or value is invalid"
        )));
    }
    if root.children().filter(roxmltree::Node::is_element).any(has_nested_gui_elements) {
        return Err(gui_nested_value_error(ctx, property_name, "StringList value"));
    }
    Ok(())
}

fn validate_gui_integer_list(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
    require_sorted_unique: bool,
) -> Result<(), CodecError> {
    let tag = if require_sorted_unique {
        "IntegerSet"
    } else {
        "IntegerList"
    };
    let count = gui_list_count(ctx, root, property_name, tag)?;
    if root.children().filter(roxmltree::Node::is_element).count() != count
        || root.children().filter(roxmltree::Node::is_element).any(|value| !value.has_tag_name("I")) {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} {tag} count or value is invalid"
        )));
    }
    if root.children().filter(roxmltree::Node::is_element).any(has_nested_gui_elements) {
        return Err(gui_nested_value_error(ctx, property_name, tag));
    }
    let mut previous = None;
    for value in root.children().filter(roxmltree::Node::is_element) {
        let number = value
            .attribute("v")
            .ok_or_else(|| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} {tag} value has no v attribute"
                ))
            })?
            .parse::<i64>()
            .map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} {tag} value is not an integer"
                ))
            })?;
        if require_sorted_unique && previous.is_some_and(|previous| number <= previous) {
            return Err(gui_malformed(ctx, format_args!(
                "GUI property {property_name} IntegerSet is not sorted and unique"
            )));
        }
        previous = Some(number);
    }
    Ok(())
}

fn validate_gui_map(ctx: &DecodeContext<'_>, root: roxmltree::Node<'_, '_>, property_name: &str) -> Result<(), CodecError> {
    let count = gui_list_count(ctx, root, property_name, "Map")?;
    if root.children().filter(roxmltree::Node::is_element).count() != count
        || root.children().filter(roxmltree::Node::is_element).any(|value| !value.has_tag_name("Item")) {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} Map count or item tag is invalid"
        )));
    }
    if root.children().filter(roxmltree::Node::is_element).any(has_nested_gui_elements) {
        return Err(gui_nested_value_error(ctx, property_name, "Map item"));
    }
    let mut previous_key = None;
    for value in root.children().filter(roxmltree::Node::is_element) {
        let key = value.attribute("key").ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI property {property_name} Map item has no key"
            ))
        })?;
        if value.attribute("value").is_none() {
            return Err(gui_malformed(ctx, format_args!(
                "GUI property {property_name} Map item has no value"
            )));
        }
        if previous_key.is_some_and(|previous| key <= previous) {
            return Err(gui_malformed(ctx, format_args!(
                "GUI property {property_name} Map keys are not sorted and unique"
            )));
        }
        previous_key = Some(key);
    }
    Ok(())
}

fn gui_list_count(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
    tag: &str,
) -> Result<usize, CodecError> {
    root.attribute("count")
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI property {property_name} {tag} has no count"
            ))
        })?
        .parse::<usize>()
        .map_err(|_| {
            gui_malformed(ctx, format_args!(
                "GUI property {property_name} {tag} has an invalid count"
            ))
        })
}

fn has_nested_gui_elements(node: roxmltree::Node<'_, '_>) -> bool {
    node.children().any(|child| child.is_element())
}

fn gui_nested_value_error(ctx: &DecodeContext<'_>, property_name: &str, value_name: &str) -> CodecError {
    crate::resource::malformed_charged(ctx,
        format_args!("GUI property {property_name} {value_name} has nested element values"),
        "FCStd GUI nested-value diagnostic")
}

fn validate_gui_constraint_attributes(
    ctx: &DecodeContext<'_>,
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
                gui_constraint_error(ctx, property_name, "an invalid integer", attribute)
            })?;
        } else {
            let value = value
                .parse::<f64>()
                .map_err(|_| gui_constraint_error(ctx, property_name, "an invalid float", attribute))?;
            if !value.is_finite() {
                return Err(gui_constraint_error(ctx,
                    property_name,
                    "a non-finite",
                    attribute,
                ));
            }
        }
    }
    Ok(())
}

fn gui_constraint_error(ctx: &DecodeContext<'_>, property_name: &str, detail: &str, attribute: &str) -> CodecError {
    crate::resource::malformed_charged(ctx,
        format_args!("GUI property {property_name} has {detail} {attribute}"),
        "FCStd GUI constraint diagnostic")
}

fn validate_gui_placement(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    for attribute in ["Px", "Py", "Pz"] {
        let value = root
            .attribute(attribute)
            .ok_or_else(|| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} placement has no {attribute}"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} placement has an invalid {attribute}"
                ))
            })?;
        if !value.is_finite() {
            return Err(gui_malformed(ctx, format_args!(
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
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} placement has no {attribute}"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} placement has an invalid {attribute}"
                ))
            })?;
        if !value.is_finite() {
            return Err(gui_malformed(ctx, format_args!(
                "GUI property {property_name} placement has a non-finite {attribute}"
            )));
        }
    }
    Ok(())
}

fn validate_gui_enumeration(
    ctx: &DecodeContext<'_>,
    integer: roxmltree::Node<'_, '_>,
    custom_list: Option<roxmltree::Node<'_, '_>>,
    has_more_roots: bool,
    property_name: &str,
) -> Result<(), CodecError> {
    let custom = integer.attribute("CustomEnum").is_some();
    if !custom && custom_list.is_none() {
        return Ok(());
    }
    let custom_list = match custom_list {
        Some(node) if custom && !has_more_roots && node.has_tag_name("CustomEnumList") => node,
        _ => return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} has an invalid custom enumeration envelope"
        ))),
    };
    let count = custom_list
        .attribute("count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI property {property_name} has an invalid custom enumeration count"
            ))
        })?;
    if custom_list.children().filter(roxmltree::Node::is_element).count() != count
        || custom_list.children().filter(roxmltree::Node::is_element)
            .any(|value| !value.has_tag_name("Enum") || value.attribute("value").is_none())
    {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} custom enumeration count or value is invalid"
        )));
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

fn validate_gui_geometry_value(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
    expected_tag: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(crate::resource::malformed_charged(ctx,
            format_args!("GUI property {property_name} requires exactly one {expected_tag} value"),
            "FCStd GUI geometry diagnostic"));
    };
    if !root.has_tag_name(expected_tag) {
        return Err(crate::resource::malformed_charged(ctx,
            format_args!("GUI property {property_name} requires a leading {expected_tag} value"),
            "FCStd GUI geometry diagnostic"));
    }

    let mut side_references = property
        .descendants()
        .filter(roxmltree::Node::is_element)
        .flat_map(|node| {
            node.attributes()
                .filter(|attribute| matches!(attribute.name(), "file" | "File"))
                .map(move |attribute| (node, attribute.value()))
        })
        .filter(|(_, value)| !value.is_empty())
        .map(|(node, _)| node);
    let first_side_reference = side_references.next();
    let has_more_side_references = side_references.next().is_some();
    let direct_file = root
        .attribute("file")
        .is_some_and(|value| !value.is_empty());
    if first_side_reference.is_some() != direct_file || has_more_side_references
        || first_side_reference.is_some_and(|node| node != root)
    {
        return Err(crate::resource::malformed_charged(ctx,
            format_args!("GUI property {property_name} {expected_tag} has an unowned side-entry reference"),
            "FCStd GUI geometry diagnostic"));
    }
    if expected_tag == "Points" {
        validate_gui_points_transform(ctx, root, property_name)?;
    }
    Ok(())
}

fn validate_gui_points_transform(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(text) = root.attribute("mtrx") else {
        return Ok(());
    };
    let mut count = 0usize;
    let mut finite = true;
    for token in text.split_whitespace() {
        let value = token.parse::<f64>().map_err(|_| crate::resource::malformed_charged(ctx,
            format_args!("GUI property {property_name} Points transform has an invalid scalar"),
            "FCStd GUI Points transform diagnostic"))?;
        finite &= value.is_finite();
        count += 1;
    }
    if count != 16 || !finite {
        return Err(crate::resource::malformed_charged(ctx,
            format_args!("GUI property {property_name} Points transform must contain 16 finite scalars"),
            "FCStd GUI Points transform diagnostic"));
    }
    Ok(())
}

fn validate_gui_techdraw_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
    list_tag: &str,
    record_tag: &str,
    mut validate: impl FnMut(&DecodeContext<'_>, roxmltree::Node<'_, '_>, &str) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(gui_techdraw_error(ctx,
            property_name,
            &format!("requires exactly one {list_tag} value"),
        ));
    };
    if !root.has_tag_name(list_tag) {
        return Err(gui_techdraw_error(ctx,
            property_name,
            &format!("requires a leading {list_tag} value"),
        ));
    }
    let count = root
        .attribute("count")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, &format!("{list_tag} has no count")))?
        .parse::<usize>()
        .map_err(|_| {
            gui_techdraw_error(ctx, property_name, &format!("{list_tag} has an invalid count"))
        })?;
    if root.children().filter(roxmltree::Node::is_element).count() != count {
        return Err(gui_techdraw_error(ctx,
            property_name,
            &format!("{list_tag} count does not match its records"),
        ));
    }
    for record in root.children().filter(roxmltree::Node::is_element) {
        if !record.has_tag_name(record_tag)
            || record.attribute("type") != Some(format!("TechDraw::{record_tag}").as_str())
        {
            return Err(gui_techdraw_error(ctx,
                property_name,
                &format!("{list_tag} has an invalid record type"),
            ));
        }
        validate(ctx, record, property_name)?;
    }
    Ok(())
}

fn gui_record_fields<'a, 'input>(
    ctx: &DecodeContext<'_>,
    record: roxmltree::Node<'a, 'input>,
    operation: &'static str,
) -> Result<Vec<roxmltree::Node<'a, 'input>>, CodecError> {
    let count = record.children().filter(roxmltree::Node::is_element).count();
    let mut fields = collection_vec(ctx, count, operation)?;
    fields.extend(record.children().filter(roxmltree::Node::is_element));
    Ok(fields)
}

fn validate_gui_geom_format_record(
    ctx: &DecodeContext<'_>,
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let fields = gui_record_fields(ctx, record, "FCStd GUI GeomFormat fields")?;
    if !(5..=6).contains(&fields.len()) {
        return Err(gui_techdraw_error(ctx,
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
            return Err(gui_techdraw_error(ctx,
                property_name,
                "GeomFormat has a nested or out-of-order field",
            ));
        }
        if field.attribute("value").is_none() {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "GeomFormat field has no value",
            ));
        }
    }
    if let Some(line_number) = fields.get(5) {
        if !(line_number.has_tag_name("LineNumber") || line_number.has_tag_name("ISOLineNumber"))
            || line_number.children().any(|node| node.is_element())
        {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "GeomFormat has an invalid line-number field",
            ));
        }
        parse_gui_techdraw_integer(ctx, *line_number, property_name)?;
    }
    parse_gui_techdraw_integer(ctx, fields[0], property_name)?;
    parse_gui_techdraw_integer(ctx, fields[1], property_name)?;
    let weight = fields[2]
        .attribute("value")
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "GeomFormat has an invalid weight"))?;
    if !weight.is_finite() {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "GeomFormat has a non-finite weight",
        ));
    }
    let Some(color) = fields[3].attribute("value") else {
        return Err(gui_techdraw_error(ctx, property_name, "GeomFormat has no color"));
    };
    if !(color.len() == 7 || color.len() == 9)
        || !color.starts_with('#')
        || !color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "GeomFormat has an invalid color",
        ));
    }
    if parse_bool(fields[4].attribute("value").unwrap_or_default()).is_none() {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "GeomFormat has an invalid visibility",
        ));
    }
    Ok(())
}

fn validate_gui_center_line_record(
    ctx: &DecodeContext<'_>,
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let fields = gui_record_fields(ctx, record, "FCStd GUI CenterLine fields")?;
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
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CenterLine has an incomplete field sequence",
        ));
    }
    for (field, expected_tag) in fields.iter().take(prefix.len()).zip(prefix) {
        if !field.has_tag_name(expected_tag) {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "CenterLine has an out-of-order field",
            ));
        }
    }
    for index in [2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 15, 16] {
        if fields[index].children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "CenterLine has a nested field",
            ));
        }
    }
    validate_gui_techdraw_point(ctx, fields[0], property_name)?;
    validate_gui_techdraw_point(ctx, fields[1], property_name)?;
    let mode = parse_gui_techdraw_integer_value(ctx, fields[2], property_name, "Mode")?;
    if !(0..=2).contains(&mode) {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CenterLine has an unsupported Mode",
        ));
    }
    for field in fields.iter().skip(3).take(4) {
        parse_gui_techdraw_finite(ctx, *field, property_name)?;
    }
    let line_type = parse_gui_techdraw_integer_value(ctx, fields[7], property_name, "Type")?;
    if !(0..=2).contains(&line_type) {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CenterLine has an unsupported Type",
        ));
    }
    validate_gui_techdraw_boolean(ctx, fields[8], property_name)?;
    validate_gui_center_line_string_collection(ctx,
        fields[9],
        property_name,
        "Faces",
        "FaceCount",
        "Face",
    )?;
    validate_gui_center_line_string_collection(ctx,
        fields[10],
        property_name,
        "Edges",
        "EdgeCount",
        "Edge",
    )?;
    validate_gui_center_line_string_collection(ctx,
        fields[11],
        property_name,
        "CLPoints",
        "CLPointCount",
        "CLPoint",
    )?;
    parse_gui_techdraw_integer_named(ctx, fields[12], property_name, "Style")?;
    parse_gui_techdraw_finite(ctx, fields[13], property_name)?;
    validate_gui_techdraw_color(ctx, fields[14], property_name)?;
    validate_gui_techdraw_boolean(ctx, fields[15], property_name)?;
    let geometry_type = TechDrawGeometryType::try_from(parse_gui_techdraw_integer_value(ctx,
        fields[16],
        property_name,
        "GeometryType",
    )?)
    .map_err(|()| {
        gui_techdraw_error(ctx,
            property_name,
            "TechDraw geometry has an unsupported GeometryType",
        )
    })?;
    validate_gui_techdraw_geometry_branch(ctx,
        &fields,
        prefix.len(),
        property_name,
        geometry_type,
        true,
    )?;
    Ok(())
}

fn validate_gui_center_line_string_collection(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    container_tag: &str,
    count_attribute: &str,
    item_tag: &str,
) -> Result<(), CodecError> {
    if !field.has_tag_name(container_tag) {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CenterLine has an invalid collection field",
        ));
    }
    let count = field
        .attribute(count_attribute)
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "CenterLine collection has no count"))?
        .parse::<usize>()
        .map_err(|_| {
            gui_techdraw_error(ctx, property_name, "CenterLine collection has an invalid count")
        })?;
    if field.children().filter(roxmltree::Node::is_element).count() != count {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CenterLine collection count does not match its records",
        ));
    }
    for item in field.children().filter(roxmltree::Node::is_element) {
        if !item.has_tag_name(item_tag)
            || item.attribute("value").is_none()
            || item.children().any(|node| node.is_element())
        {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "CenterLine collection has an invalid item",
            ));
        }
    }
    Ok(())
}

fn validate_gui_cosmetic_edge_record(
    ctx: &DecodeContext<'_>,
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let fields = gui_record_fields(ctx, record, "FCStd GUI CosmeticEdge fields")?;
    if fields.len() < 16 {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CosmeticEdge has an incomplete field sequence",
        ));
    }
    let format_fields = ["Style", "Weight", "Color", "Visible", "GeometryType"];
    for (field, expected_tag) in fields.iter().take(format_fields.len()).zip(format_fields) {
        if !field.has_tag_name(expected_tag) || field.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "CosmeticEdge has a nested or out-of-order format field",
            ));
        }
        if field.attribute("value").is_none() {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "CosmeticEdge format field has no value",
            ));
        }
    }
    parse_gui_techdraw_integer_named(ctx, fields[0], property_name, "Style")?;
    parse_gui_techdraw_finite(ctx, fields[1], property_name)?;
    validate_gui_techdraw_color(ctx, fields[2], property_name)?;
    validate_gui_techdraw_boolean(ctx, fields[3], property_name)?;
    let geometry_type = TechDrawGeometryType::try_from(
        fields[4]
            .attribute("value")
            .ok_or_else(|| {
                gui_techdraw_error(ctx, property_name, "CosmeticEdge GeometryType has no value")
            })?
            .parse::<i64>()
            .map_err(|_| {
                gui_techdraw_error(ctx, property_name, "CosmeticEdge GeometryType is not an integer")
            })?,
    )
    .map_err(|()| {
        gui_techdraw_error(ctx,
            property_name,
            "TechDraw geometry has an unsupported GeometryType",
        )
    })?;

    validate_gui_techdraw_geometry_branch(ctx,
        &fields,
        format_fields.len(),
        property_name,
        geometry_type,
        false,
    )?;
    Ok(())
}

/// A `TechDraw` `GeomType` discriminant supported by the GUI validator.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TechDrawGeometryType {
    Circle,
    ArcOfCircle,
    Generic,
}

impl TechDrawGeometryType {
    fn as_i64(self) -> i64 {
        match self {
            Self::Circle => 1,
            Self::ArcOfCircle => 2,
            Self::Generic => 7,
        }
    }
}

impl TryFrom<i64> for TechDrawGeometryType {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Circle),
            2 => Ok(Self::ArcOfCircle),
            7 => Ok(Self::Generic),
            _ => Err(()),
        }
    }
}

fn validate_gui_techdraw_geometry_branch(
    ctx: &DecodeContext<'_>,
    fields: &[roxmltree::Node<'_, '_>],
    base_start: usize,
    property_name: &str,
    expected_geometry_type: TechDrawGeometryType,
    allow_iso_line_number: bool,
) -> Result<(), CodecError> {
    if fields.len() < base_start + 10 {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw geometry has no complete BaseGeom sequence",
        ));
    }
    validate_gui_techdraw_base_geom(ctx,
        &fields[base_start..base_start + 10],
        property_name,
        expected_geometry_type,
    )?;
    let mut cursor = base_start + 10;
    let branch_field_count = match expected_geometry_type {
        TechDrawGeometryType::Circle => 2,
        TechDrawGeometryType::ArcOfCircle => 9,
        TechDrawGeometryType::Generic => 1,
    };
    let required_fields = cursor + branch_field_count;
    if fields.len() != required_fields && fields.len() != required_fields + 1 {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw geometry has an invalid branch field sequence",
        ));
    }

    match expected_geometry_type {
        TechDrawGeometryType::Circle => {
            if !fields[cursor].has_tag_name("Center") {
                return Err(gui_techdraw_error(ctx,
                    property_name,
                    "TechDraw circle has an invalid center field",
                ));
            }
            validate_gui_techdraw_point(ctx, fields[cursor], property_name)?;
            cursor += 1;
            if !fields[cursor].has_tag_name("Radius") {
                return Err(gui_techdraw_error(ctx,
                    property_name,
                    "TechDraw circle has an invalid radius field",
                ));
            }
            parse_gui_techdraw_finite(ctx, fields[cursor], property_name)?;
            if fields[cursor].children().any(|node| node.is_element()) {
                return Err(gui_techdraw_error(ctx,
                    property_name,
                    "TechDraw circle has a nested radius field",
                ));
            }
            cursor += 1;
        }
        TechDrawGeometryType::ArcOfCircle => {
            if !fields[cursor].has_tag_name("Center") {
                return Err(gui_techdraw_error(ctx,
                    property_name,
                    "TechDraw arc has an invalid center field",
                ));
            }
            validate_gui_techdraw_point(ctx, fields[cursor], property_name)?;
            cursor += 1;
            if !fields[cursor].has_tag_name("Radius") {
                return Err(gui_techdraw_error(ctx,
                    property_name,
                    "TechDraw arc has an invalid radius field",
                ));
            }
            parse_gui_techdraw_finite(ctx, fields[cursor], property_name)?;
            if fields[cursor].children().any(|node| node.is_element()) {
                return Err(gui_techdraw_error(ctx,
                    property_name,
                    "TechDraw arc has a nested radius field",
                ));
            }
            cursor += 1;
            for expected_tag in ["Start", "End", "Middle"] {
                if !fields[cursor].has_tag_name(expected_tag) {
                    return Err(gui_techdraw_error(ctx,
                        property_name,
                        "TechDraw arc has an out-of-order point field",
                    ));
                }
                validate_gui_techdraw_point(ctx, fields[cursor], property_name)?;
                cursor += 1;
            }
            for expected_tag in ["StartAngle", "EndAngle"] {
                if !fields[cursor].has_tag_name(expected_tag) {
                    return Err(gui_techdraw_error(ctx,
                        property_name,
                        "TechDraw arc has an out-of-order angle field",
                    ));
                }
                parse_gui_techdraw_finite(ctx, fields[cursor], property_name)?;
                if fields[cursor].children().any(|node| node.is_element()) {
                    return Err(gui_techdraw_error(ctx,
                        property_name,
                        "TechDraw arc has a nested angle field",
                    ));
                }
                cursor += 1;
            }
            for expected_tag in ["Clockwise", "Large"] {
                if !fields[cursor].has_tag_name(expected_tag) {
                    return Err(gui_techdraw_error(ctx,
                        property_name,
                        "TechDraw arc has an out-of-order Boolean field",
                    ));
                }
                validate_gui_techdraw_boolean(ctx, fields[cursor], property_name)?;
                cursor += 1;
            }
        }
        TechDrawGeometryType::Generic => {
            if !fields[cursor].has_tag_name("Points") {
                return Err(gui_techdraw_error(ctx,
                    property_name,
                    "TechDraw generic geometry has no Points field",
                ));
            }
            validate_gui_techdraw_points(ctx, fields[cursor], property_name)?;
            cursor += 1;
        }
    }

    if cursor < fields.len() {
        let line_number = fields[cursor].has_tag_name("LineNumber")
            || (allow_iso_line_number && fields[cursor].has_tag_name("ISOLineNumber"));
        if cursor + 1 != fields.len() || !line_number {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "TechDraw geometry has an invalid trailing field",
            ));
        }
        parse_gui_techdraw_integer_named(ctx, fields[cursor], property_name, "LineNumber")?;
        if fields[cursor].children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "TechDraw geometry line number is nested",
            ));
        }
    }
    Ok(())
}

fn validate_gui_techdraw_base_geom(
    ctx: &DecodeContext<'_>,
    fields: &[roxmltree::Node<'_, '_>],
    property_name: &str,
    expected_geometry_type: TechDrawGeometryType,
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
    let geometry_type_value = gui_techdraw_base_geom_value(ctx, fields[0], expected[0], property_name)?;
    for (field, expected_tag) in fields.iter().zip(expected).skip(1) {
        gui_techdraw_base_geom_value(ctx, *field, expected_tag, property_name)?;
    }
    let geometry_type = geometry_type_value
        .parse::<i64>()
        .map_err(|_| gui_techdraw_error(ctx, property_name, "TechDraw GeomType is not an integer"))?;
    if geometry_type != expected_geometry_type.as_i64() {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw GeometryType and GeomType disagree",
        ));
    }
    for index in [1, 2, 5, 7, 8] {
        parse_gui_techdraw_integer_named(ctx, fields[index], property_name, expected[index])?;
    }
    for index in [3, 4, 6] {
        validate_gui_techdraw_boolean(ctx, fields[index], property_name)?;
    }
    Ok(())
}

fn gui_techdraw_base_geom_value<'a>(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'a, '_>,
    expected_tag: &str,
    property_name: &str,
) -> Result<&'a str, CodecError> {
    if !field.has_tag_name(expected_tag) || field.children().any(|node| node.is_element()) {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw BaseGeom has a nested or out-of-order field",
        ));
    }
    field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw BaseGeom field has no value"))
}

fn validate_gui_techdraw_points(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let count = field
        .attribute("PointsCount")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw Points has no count"))?
        .parse::<usize>()
        .map_err(|_| gui_techdraw_error(ctx, property_name, "TechDraw Points has an invalid count"))?;
    if field.children().filter(roxmltree::Node::is_element).count() != count {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw Points count does not match its records",
        ));
    }
    for point in field.children().filter(roxmltree::Node::is_element) {
        if !point.has_tag_name("Point") || point.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "TechDraw Points has an invalid point record",
            ));
        }
        validate_gui_techdraw_point(ctx, point, property_name)?;
    }
    Ok(())
}

fn validate_gui_cosmetic_vertex_record(
    ctx: &DecodeContext<'_>,
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let fields = gui_record_fields(ctx, record, "FCStd GUI CosmeticVertex fields")?;
    if !(15..=16).contains(&fields.len()) {
        return Err(gui_techdraw_error(ctx,
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
            return Err(gui_techdraw_error(ctx,
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
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CosmeticVertex has an out-of-order field",
        ));
    }
    let mut cursor = base_fields.len();
    if fields[cursor].has_tag_name("VertexTag") {
        validate_gui_techdraw_uuid(ctx, fields[cursor], property_name, "VertexTag")?;
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
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CosmeticVertex has an out-of-order field",
        ));
    }
    for field in &fields {
        if field.children().any(|node| node.is_element()) {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "CosmeticVertex has a nested field",
            ));
        }
    }
    validate_gui_techdraw_point(ctx, fields[0], property_name)?;
    parse_gui_techdraw_integer_named(ctx, fields[1], property_name, "Extract")?;
    validate_gui_techdraw_boolean(ctx, fields[2], property_name)?;
    parse_gui_techdraw_integer_named(ctx, fields[3], property_name, "Ref3D")?;
    validate_gui_techdraw_boolean(ctx, fields[4], property_name)?;
    validate_gui_techdraw_boolean(ctx, fields[5], property_name)?;
    parse_gui_techdraw_integer_named(ctx, fields[6], property_name, "CosmeticLink")?;
    if fields[7].attribute("value").is_none() {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "CosmeticVertex CosmeticTag has no value",
        ));
    }
    validate_gui_techdraw_point(ctx, fields[cursor], property_name)?;
    parse_gui_techdraw_integer_named(ctx, fields[cursor + 1], property_name, "LinkGeom")?;
    validate_gui_techdraw_color(ctx, fields[cursor + 2], property_name)?;
    parse_gui_techdraw_finite(ctx, fields[cursor + 3], property_name)?;
    parse_gui_techdraw_integer_named(ctx, fields[cursor + 4], property_name, "Style")?;
    validate_gui_techdraw_boolean(ctx, fields[cursor + 5], property_name)?;
    validate_gui_techdraw_uuid(ctx, fields[cursor + 6], property_name, "Tag")?;
    Ok(())
}

fn validate_gui_techdraw_point(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    if field.children().any(|node| node.is_element()) {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw point has a nested field",
        ));
    }
    for attribute in ["X", "Y", "Z"] {
        let value = field
            .attribute(attribute)
            .ok_or_else(|| gui_techdraw_error(ctx, property_name, "point has no coordinate"))?
            .parse::<f64>()
            .map_err(|_| gui_techdraw_error(ctx, property_name, "point has an invalid coordinate"))?;
        if !value.is_finite() {
            return Err(gui_techdraw_error(ctx,
                property_name,
                "point has a non-finite coordinate",
            ));
        }
    }
    Ok(())
}

fn validate_gui_techdraw_boolean(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let value = field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw Boolean has no value"))?;
    if parse_bool(value).is_none() {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw has an invalid Boolean",
        ));
    }
    Ok(())
}

fn validate_gui_techdraw_color(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let color = field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw color has no value"))?;
    if !(color.len() == 7 || color.len() == 9)
        || !color.starts_with('#')
        || !color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw has an invalid color",
        ));
    }
    Ok(())
}

fn parse_gui_techdraw_finite(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let value = field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw scalar has no value"))?
        .parse::<f64>()
        .map_err(|_| gui_techdraw_error(ctx, property_name, "TechDraw scalar is invalid"))?;
    if !value.is_finite() {
        return Err(gui_techdraw_error(ctx,
            property_name,
            "TechDraw scalar is non-finite",
        ));
    }
    Ok(())
}

fn parse_gui_techdraw_integer_named(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    field_name: &str,
) -> Result<(), CodecError> {
    parse_gui_techdraw_integer_value(ctx, field, property_name, field_name).map(|_| ())
}

fn parse_gui_techdraw_integer_value(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    field_name: &str,
) -> Result<i64, CodecError> {
    field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw integer has no value"))?
        .parse::<i64>()
        .map_err(|_| {
            gui_techdraw_error(ctx,
                property_name,
                &format!("TechDraw {field_name} integer is invalid"),
            )
        })
}

fn validate_gui_techdraw_uuid(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    field_name: &str,
) -> Result<(), CodecError> {
    let value = field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "CosmeticVertex tag has no value"))?;
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|&index| bytes[index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .filter(|(index, _)| ![8, 13, 18, 23].contains(index))
            .all(|(_, byte)| byte.is_ascii_hexdigit());
    if !valid {
        return Err(gui_techdraw_error(ctx,
            property_name,
            &format!("CosmeticVertex {field_name} is not a UUID"),
        ));
    }
    Ok(())
}

fn parse_gui_techdraw_integer(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    field
        .attribute("value")
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "GeomFormat field has no value"))?
        .parse::<i64>()
        .map(|_| ())
        .map_err(|_| gui_techdraw_error(ctx, property_name, "GeomFormat has an invalid integer"))
}

fn gui_techdraw_error(ctx: &DecodeContext<'_>, property_name: &str, detail: &str) -> CodecError {
    crate::resource::malformed_charged(ctx,
        format_args!("GUI property {property_name} {detail}"),
        "FCStd GUI TechDraw diagnostic")
}

fn validate_visual_layer_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires exactly one VisualLayerList value"
        )));
    };
    if !root.has_tag_name("VisualLayerList") {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires a leading VisualLayerList value"
        )));
    }
    let count = root
        .attribute("count")
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI property {property_name} VisualLayerList has no count"
            ))
        })?
        .parse::<usize>()
        .map_err(|_| {
            gui_malformed(ctx, format_args!(
                "GUI property {property_name} VisualLayerList has an invalid count"
            ))
        })?;
    if root.children().filter(roxmltree::Node::is_element).count() != count
        || root.children().filter(roxmltree::Node::is_element)
            .any(|layer| !layer.has_tag_name("VisualLayer"))
    {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} VisualLayerList count or record tag is invalid"
        )));
    }
    for layer in root.children().filter(roxmltree::Node::is_element) {
        if !matches!(layer.attribute("visible"), Some("true" | "false")) {
            return Err(gui_malformed(ctx, format_args!(
                "GUI property {property_name} VisualLayer has an invalid visible value"
            )));
        }
        layer
            .attribute("linePattern")
            .ok_or_else(|| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} VisualLayer has no linePattern"
                ))
            })?
            .parse::<u32>()
            .map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} VisualLayer has an invalid linePattern"
                ))
            })?;
        let line_width = layer
            .attribute("lineWidth")
            .ok_or_else(|| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} VisualLayer has no lineWidth"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} VisualLayer has an invalid lineWidth"
                ))
            })?;
        if !line_width.is_finite() {
            return Err(gui_malformed(ctx, format_args!(
                "GUI property {property_name} VisualLayer has a non-finite lineWidth"
            )));
        }
    }
    Ok(())
}

fn validate_gui_material(
    ctx: &DecodeContext<'_>,
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
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} material has no {attribute}"
                ))
            })?
            .parse::<u32>()
            .map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} material has an invalid {attribute}"
                ))
            })?;
    }
    for attribute in ["shininess", "transparency"] {
        let scalar = value
            .attribute(attribute)
            .ok_or_else(|| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} material has no {attribute}"
                ))
            })?
            .parse::<f64>()
            .map_err(|_| {
                gui_malformed(ctx, format_args!(
                    "GUI property {property_name} material has an invalid {attribute}"
                ))
            })?;
        if !scalar.is_finite() {
            return Err(gui_malformed(ctx, format_args!(
                "GUI property {property_name} material has a non-finite {attribute}"
            )));
        }
    }
    Ok(())
}

fn validate_gui_expression_engine(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires one ExpressionEngine value"
        )));
    };
    if !root.has_tag_name("ExpressionEngine") {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires a leading ExpressionEngine value"
        )));
    }
    let count = gui_list_count(ctx, root, property_name, "ExpressionEngine")?;
    if root.children().filter(|child| child.is_element() && child.has_tag_name("Expression")).count() != count
        || root.children().filter(|child| child.is_element() && child.has_tag_name("Expression")).any(|expression| {
            expression.attribute("path").is_none() || expression.attribute("expression").is_none()
        })
    {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} ExpressionEngine count or expression is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_material_reference(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires one PropertyMaterial value"
        )));
    };
    if !root.has_tag_name("PropertyMaterial") || root.attribute("uuid").is_none() {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} material reference is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_part_shape(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let first = roots.next();
    if first.is_none_or(|root| root.has_tag_name("Part"))
        && roots.all(|root| root.has_tag_name("ElementMap"))
    {
        return Ok(());
    }
    Err(gui_malformed(ctx, format_args!(
        "GUI property {property_name} Part shape value is invalid"
    )))
}

fn validate_gui_geometry_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires one GeometryList value"
        )));
    };
    if !root.has_tag_name("GeometryList") {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires a leading GeometryList value"
        )));
    }
    let count = gui_list_count(ctx, root, property_name, "GeometryList")?;
    if root.children().filter(roxmltree::Node::is_element).count() != count
        || root.children().filter(roxmltree::Node::is_element)
            .any(|geometry| !geometry.has_tag_name("Geometry"))
    {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} GeometryList count or record tag is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_filletedges(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires one FilletEdges value"
        )));
    };
    if !root.has_tag_name("FilletEdges") || root.attribute("file").is_none() {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} FilletEdges value is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_shape_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires one ShapeList value"
        )));
    };
    if !root.has_tag_name("ShapeList") {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires a leading ShapeList value"
        )));
    }
    let count = gui_list_count(ctx, root, property_name, "ShapeList")?;
    if root.children().filter(roxmltree::Node::is_element).count() != count
        || root.children().filter(roxmltree::Node::is_element).any(|shape| {
            !shape.has_tag_name("TopoShape")
                || (shape.attribute("file").is_none()
                    && shape.attribute("binary").is_none()
                    && shape.attribute("brep").is_none())
        })
    {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} ShapeList count or record is invalid"
        )));
    }
    Ok(())
}

fn validate_gui_constraint_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut roots = property.children().filter(roxmltree::Node::is_element);
    let Some(root) = roots.next().filter(|_| roots.next().is_none()) else {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires one ConstraintList value"
        )));
    };
    if !root.has_tag_name("ConstraintList") {
        return Err(gui_malformed(ctx, format_args!(
            "GUI property {property_name} requires a leading ConstraintList value"
        )));
    }
    let count = gui_list_count(ctx, root, property_name, "ConstraintList")?;
    if root.children().filter(roxmltree::Node::is_element).count() != count
        || root.children().filter(roxmltree::Node::is_element)
            .any(|constraint| !constraint.has_tag_name("Constrain"))
    {
        return Err(gui_malformed(ctx, format_args!(
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
    shininess: FiniteBinary32,
    transparency: FiniteBinary32,
    image: String,
    image_path: String,
    uuid: String,
}

fn validate_gui_list_payloads(
    ctx: &DecodeContext<'_>,
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
                    gui_malformed(ctx, format_args!(
                        "GUI property {} references missing side entry {entry_name}",
                        property.id
                    ))
                })?;
            }
            continue;
        }
        let entry_name = property.side_entries.first().ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI property {} has no side entry",
                property.id
            ))
        })?;
        if property.side_entries.len() != 1 {
            return Err(gui_malformed(ctx, format_args!(
                "GUI property {} references more than one side entry",
                property.id
            )));
        }
        let view = *entries.get(entry_name).ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI property {} references missing side entry {entry_name}",
                property.id
            ))
        })?;
        match property.type_name.as_str() {
            "App::PropertyColorList" => {
                parse_color_list(ctx, view, entry_name, requires_alpha_conversion)?;
            }
            "App::PropertyFloatList" => {
                parse_float_list(ctx, view, entry_name)?;
            }
            "Part::PropertyFilletEdges" => {
                parse_fillet_edges(ctx, view, entry_name)?;
            }
            "App::PropertyMaterialList" => {
                let version = property
                    .values
                    .iter()
                    .find(|value| value.tag == "MaterialList")
                    .and_then(|value| value.attributes.get("version"))
                    .map(|value| {
                        value.parse::<u32>().map_err(|_| {
                            gui_malformed(ctx, format_args!(
                                "GUI material list {} has an invalid version",
                                property.id
                            ))
                        })
                    })
                    .transpose()?
                    .unwrap_or(0);
                insert_hash_map(ctx, &mut material_lists,
                    retained_string(ctx, &property.id, "FCStd GUI material list property identity")?,
                    parse_material_list(ctx, view, version, &property.id, requires_alpha_conversion)?,
                    "FCStd GUI material lists")?;
            }
            "App::PropertyPlacementList" => {
                parse_placement_list(ctx, view, entry_name)?;
            }
            "App::PropertyVectorList" => {
                parse_vector_list(ctx, view, entry_name)?;
            }
            _ => {}
        }
    }
    Ok(material_lists)
}

fn parse_color_list(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    entry_name: &str,
    requires_alpha_conversion: bool,
) -> Result<Vec<u32>, CodecError> {
    let count = view.req_u32_le()?;
    let count = view.counted(count.into(), 4).ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "color-list entry {entry_name} count exceeds its payload"
            ))
        })?.get();
    let mut colors = collection_vec(ctx, count, "FCStd GUI color-list entries")?;
    for _ in 0..count {
        colors.push(convert_packed_alpha(view.req_u32_le()?, requires_alpha_conversion));
    }
    if !view.is_empty() {
        return Err(gui_malformed(ctx, format_args!(
            "color-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(colors)
}

fn read_gui_counted<T>(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    count: u32,
    element_size: usize,
    mut read: impl FnMut(&mut View<'_>) -> Option<T>,
    operation: &'static str,
) -> Result<Option<Vec<T>>, CodecError> {
    let Some(count) = view.counted(count.into(), element_size) else {
        return Ok(None);
    };
    let mut values = collection_vec(ctx, count.get(), operation)?;
    for _ in 0..count.get() {
        let Some(value) = read(view) else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok(Some(values))
}

fn parse_float_list(ctx: &DecodeContext<'_>, mut view: View<'_>, entry_name: &str) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let values = read_gui_counted(ctx, &mut view, count, 8, |view| view.f64_le(), "FCStd GUI float-list entries")?
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "float-list entry {entry_name} count exceeds its payload"
            ))
        })?;
    if values.iter().any(|value| !value.is_finite()) {
        return Err(gui_malformed(ctx, format_args!(
            "float-list entry {entry_name} has a non-finite value"
        )));
    }
    if !view.is_empty() {
        return Err(gui_malformed(ctx, format_args!(
            "float-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(())
}

fn parse_vector_list(ctx: &DecodeContext<'_>, mut view: View<'_>, entry_name: &str) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let values = read_gui_counted(ctx, &mut view, count, 24, |view| {
            Some((view.f64_le()?, view.f64_le()?, view.f64_le()?))
        }, "FCStd GUI vector-list entries")?
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "vector-list entry {entry_name} count exceeds its payload"
            ))
        })?;
    if values
        .iter()
        .flat_map(|value| [value.0, value.1, value.2])
        .any(|value| !value.is_finite())
    {
        return Err(gui_malformed(ctx, format_args!(
            "vector-list entry {entry_name} has a non-finite value"
        )));
    }
    if !view.is_empty() {
        return Err(gui_malformed(ctx, format_args!(
            "vector-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(())
}

fn parse_placement_list(ctx: &DecodeContext<'_>, mut view: View<'_>, entry_name: &str) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let values = read_gui_counted(ctx, &mut view, count, 56, |view| {
            Some([
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
            ])
        }, "FCStd GUI placement-list entries")?
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "placement-list entry {entry_name} count exceeds its payload"
            ))
        })?;
    if values.iter().flatten().any(|value| !value.is_finite()) {
        return Err(gui_malformed(ctx, format_args!(
            "placement-list entry {entry_name} has a non-finite value"
        )));
    }
    if !view.is_empty() {
        return Err(gui_malformed(ctx, format_args!(
            "placement-list entry {entry_name} has trailing bytes"
        )));
    }
    Ok(())
}

fn parse_fillet_edges(ctx: &DecodeContext<'_>, mut view: View<'_>, entry_name: &str) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let values = read_gui_counted(ctx, &mut view, count, 20, |view| {
            Some((view.i32_le()?, view.f64_le()?, view.f64_le()?))
        }, "FCStd GUI fillet-edge entries")?
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "fillet-edges entry {entry_name} count exceeds its payload"
            ))
        })?;
    if values
        .iter()
        .any(|(_, radius1, radius2)| !radius1.is_finite() || !radius2.is_finite())
    {
        return Err(gui_malformed(ctx, format_args!(
            "fillet-edges entry {entry_name} has a non-finite radius"
        )));
    }
    if !view.is_empty() {
        return Err(gui_malformed(ctx, format_args!(
            "fillet-edges entry {entry_name} has trailing bytes"
        )));
    }
    Ok(())
}

fn parse_material_list(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    version: u32,
    property_id: &str,
    requires_alpha_conversion: bool,
) -> Result<Vec<GuiMaterial>, CodecError> {
    let (count, has_strings) = match version {
        0 | 1 => {
            let header = view.i32_le().ok_or_else(|| {
                gui_malformed(ctx, format_args!("GUI material list {property_id} is truncated"))
            })?;
            let count = if header < 0 {
                view.u32_le().ok_or_else(|| {
                    gui_malformed(ctx, format_args!(
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
                gui_malformed(ctx, format_args!("GUI material list {property_id} is truncated"))
            })?,
            false,
        ),
        3 => (
            view.u32_le().ok_or_else(|| {
                gui_malformed(ctx, format_args!("GUI material list {property_id} is truncated"))
            })?,
            true,
        ),
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "FCStd GUI material-list version {version}"
            )));
        }
    };
    let raw_materials = read_gui_counted(ctx, &mut view, count, 24, |view| {
            Some((
                [
                    view.u32_le()?,
                    view.u32_le()?,
                    view.u32_le()?,
                    view.u32_le()?,
                ],
                [view.f32_le()?, view.f32_le()?],
            ))
        }, "FCStd GUI raw material entries")?
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI material list {property_id} count exceeds its payload"
            ))
        })?;
    let mut materials = collection_vec(ctx, raw_materials.len(), "FCStd GUI material entries")?;
    for ([ambient, diffuse, specular, emissive], [shininess, transparency]) in raw_materials {
                let invalid = || {
                    gui_malformed(ctx, format_args!(
                        "GUI material list {property_id} has non-finite scalars"
                    ))
                };
                materials.push(GuiMaterial {
                    ambient,
                    diffuse,
                    specular,
                    emissive,
                    shininess: FiniteBinary32::new(shininess).ok_or_else(invalid)?,
                    transparency: FiniteBinary32::new(transparency).ok_or_else(invalid)?,
                    image: String::new(),
                    image_path: String::new(),
                    uuid: String::new(),
                });
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
            material.image = read_material_string(ctx, &mut view, property_id)?;
            material.image_path = read_material_string(ctx, &mut view, property_id)?;
            material.uuid = read_material_string(ctx, &mut view, property_id)?;
        }
    }
    if !view.is_empty() {
        return Err(gui_malformed(ctx, format_args!(
            "GUI material list {property_id} has trailing bytes"
        )));
    }
    Ok(materials)
}

fn read_material_string(ctx: &DecodeContext<'_>, view: &mut View<'_>, property_id: &str) -> Result<String, CodecError> {
    let length = view.u32_le().ok_or_else(|| {
        gui_malformed(ctx, format_args!(
            "GUI material list {property_id} string is truncated"
        ))
    })?;
    let length = view
        .counted(length.into(), 1)
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!(
                "GUI material list {property_id} string exceeds its payload"
            ))
        })?
        .get();
    let bytes = view.take(length).ok_or_else(|| {
        gui_malformed(ctx, format_args!(
            "GUI material list {property_id} string exceeds its payload"
        ))
    })?;
    String::from_utf8(ctx.copy_retained(bytes, "FCStd GUI material string")?).map_err(|_| {
        gui_malformed(ctx, format_args!(
            "GUI material list {property_id} string is not UTF-8"
        ))
    })
}

#[allow(clippy::too_many_arguments)]
fn transfer_shape_appearances(
    ctx: &DecodeContext<'_>,
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
        let provider_key = provider_identity_key(ctx, &provider.name)?;
        let Some(object_id) = provider
            .object
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str)
        else {
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
        let body_ids = displayed_shape_bodies(ctx, ir, object_id, properties, payloads)?;
        let group = displayed_shape_group(ctx, object_id, properties, payloads, element_maps, "Face")?;
        let mapped_count = match group {
            None => 0,
            Some(group) => match group.names.len() {
                0 => 0,
                length => length - 1,
            },
        };
        if materials.len() == 1 {
            let legacy_id = object_appearance_id(ctx, &provider_key)?;
            plan.bindings
                .retain(|binding| binding.appearance != legacy_id);
            plan.appearances
                .retain(|appearance| appearance.id != legacy_id);
            insert_hash_set(ctx, &mut plan.remove_appearances, legacy_id,
                "FCStd GUI removed appearances")?;
        } else {
            let Some(_) = group else {
                continue;
            };
            if mapped_count == 0 {
                continue;
            }
            if materials.len() != mapped_count {
                push_gui_appearance_loss(ctx, losses,
                    FreecadLossCode::AppearanceTopologyColorCountMismatch,
                    format_args!(
                        "FCStd provider {} ShapeAppearance material count {} does not match {} mapped Face subelements; native material list retained and neutral face override withheld",
                        provider.name, materials.len(), mapped_count
                    ),
                    SourceProvenance::in_stream(
                        "fcstd",
                        cadmpeg_ir::stream_name!("GuiDocument.xml"),
                        property.xml.start(),
                    )
                    .with_tag(retained_string(ctx, &property.id, "FCStd GUI material loss tag")?),
                    "FCStd GUI material count losses", "FCStd GUI material count loss text")?;
                continue;
            }
        }
        for (index, material) in materials.iter().enumerate() {
            let appearance_id = shape_material_appearance_id(ctx, &provider_key, index)?;
            reserve_vec_items(ctx, &mut plan.appearances, 1, "FCStd GUI planned appearances")?;
            plan.appearances.push(material_appearance(
                ctx,
                crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI appearance identity copy")?,
                &provider.name,
                index,
                material,
            )?);
            if materials.len() == 1 {
                for (body_index, body) in body_ids.iter().enumerate() {
                    push_body_update(ctx, plan, body, Assignment::Keep,
                        decode_color(
                            material.diffuse,
                            Some(material.transparency.get()),
                        ).map(Some))?;
                    reserve_vec_items(ctx, &mut plan.bindings, 1, "FCStd GUI planned bindings")?;
                    plan.bindings.push(AppearanceBinding {
                        id: binding_id(ctx, format_args!(
                            "fcstd:appearance:binding#shape-material:{provider_key}:{body_index}"
                        ))?,
                        target: AppearanceTarget::Body(crate::resource::copied_identity(ctx, body.as_str(), "FCStd GUI binding body identity")?),
                        appearance: crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI binding appearance identity")?,
                        source_entity_id: Some(retained_string(ctx, object_id, "FCStd GUI binding source identity")?),
                        object_type: Some("ViewProvider ShapeAppearance".into()),
                        visible: None,
                        channels: BTreeMap::new(),
                    });
                }
            } else if let Some(group) = group {
                bind_material_faces(
                    ctx,
                    ir,
                    plan,
                    group,
                    index,
                    &appearance_id,
                    &provider_key,
                    object_id,
                )?;
            }
        }
    }
    Ok(())
}

fn displayed_shape_bodies(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<cadmpeg_ir::ids::BodyId>, CodecError> {
    let Some(payload) = displayed_shape_payload(ctx, object_id, properties, payloads)? else {
        return Ok(Vec::new());
    };
    select_shape_bodies(ctx, ir, std::iter::once(payload.id.as_str()))
}

fn select_shape_bodies<'a>(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    payload_ids: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<cadmpeg_ir::ids::BodyId>, CodecError> {
    let mut body_ids = Vec::new();
    for payload_id in payload_ids {
        let payload_key = crate::native::id_key(payload_id);
        for body in &ir.model.bodies {
            let body_key = crate::native::id_key(body.id.as_str());
            if body_key.strip_prefix(payload_key)
                .is_some_and(|suffix| suffix.starts_with(':')) {
                reserve_vec_items(ctx, &mut body_ids, 1, "FCStd GUI displayed shape bodies")?;
                body_ids.push(crate::resource::copied_identity(ctx, body.id.as_str(),
                    "FCStd GUI displayed body identity")?);
            }
        }
    }
    Ok(body_ids)
}

fn displayed_shape_payload<'a>(
    ctx: &DecodeContext<'_>,
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &'a [ShapePayloadRecord],
) -> Result<Option<&'a ShapePayloadRecord>, CodecError> {
    let mut shape_properties = properties.iter()
        .filter(|property| property.owner == object_id && property.name == "Shape");
    let Some(property) = shape_properties.next() else { return Ok(None); };
    if shape_properties.next().is_some() {
        return Err(CodecError::Malformed(crate::resource::retained_format(ctx,
            format_args!("object {object_id} has multiple Shape properties"),
            "FCStd GUI duplicate shape property diagnostic",
        )?));
    }
    let mut shape_payloads = payloads.iter().filter(|payload| payload.property == property.id);
    let Some(payload) = shape_payloads.next() else { return Ok(None); };
    if shape_payloads.next().is_some() {
        return Err(CodecError::Malformed(crate::resource::retained_format(ctx,
            format_args!("Shape property {} has multiple payloads", property.id),
            "FCStd GUI duplicate shape payload diagnostic",
        )?));
    }
    Ok(Some(payload))
}

fn displayed_shape_group<'a>(
    ctx: &DecodeContext<'_>,
    object_id: &str,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &'a [ElementMapRecord],
    indexed_name: &str,
) -> Result<Option<&'a ElementMapGroup>, CodecError> {
    let Some(payload) = displayed_shape_payload(ctx, object_id, properties, payloads)? else {
        return Ok(None);
    };
    let mut shape_maps = element_maps.iter().filter(|map| map.property == payload.property);
    let Some(map) = shape_maps.next() else { return Ok(None); };
    if shape_maps.next().is_some() {
        return Err(CodecError::Malformed(crate::resource::retained_format(ctx,
            format_args!("Shape property {} has multiple element maps", payload.property),
            "FCStd GUI duplicate element map diagnostic",
        )?));
    }
    let root = map.maps.root();
    let mut groups = root.groups.iter().filter(|group| group.indexed_name == indexed_name);
    let Some(group) = groups.next() else { return Ok(None); };
    if groups.next().is_some() {
        return Err(CodecError::Malformed(crate::resource::retained_format(ctx,
            format_args!("Shape property {} has multiple {indexed_name} groups", payload.property),
            "FCStd GUI duplicate element group diagnostic",
        )?));
    }
    Ok(Some(group))
}

fn material_appearance(
    ctx: &DecodeContext<'_>,
    id: AppearanceId,
    provider_name: &str,
    index: usize,
    material: &GuiMaterial,
) -> Result<Appearance, CodecError> {
    let scalar = |name: &str, value: f64| {
        cadmpeg_ir::scalar::FiniteReal::new(value)
            .ok_or_else(|| gui_malformed(ctx, format_args!("GUI material {name} is non-finite")))
    };
    Ok(Appearance {
        id,
        name: Some(crate::resource::retained_format(ctx,
            format_args!("{provider_name} face {} material", index + 1), "FCStd GUI appearance name")?),
        asset_guid: (!material.uuid.is_empty())
            .then(|| retained_string(ctx, &material.uuid, "FCStd GUI material asset GUID"))
            .transpose()?,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("FCStd ShapeAppearance".into()),
        category: None,
        base_color: Some(decode_color(
            material.diffuse,
            Some(material.transparency.get()),
        )?),
        textures: Vec::new(),
        properties: [
            (
                cadmpeg_core::nonblank_literal!("ambient_packed"),
                scalar("ambient_packed", f64::from(material.ambient))?,
            ),
            (
                cadmpeg_core::nonblank_literal!("specular_packed"),
                scalar("specular_packed", f64::from(material.specular))?,
            ),
            (
                cadmpeg_core::nonblank_literal!("emissive_packed"),
                scalar("emissive_packed", f64::from(material.emissive))?,
            ),
            (
                cadmpeg_core::nonblank_literal!("shininess"),
                material.shininess.into(),
            ),
            (
                cadmpeg_core::nonblank_literal!("transparency"),
                material.transparency.into(),
            ),
        ]
        .into(),
    })
}

fn bind_material_faces(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    plan: &mut AppearancePlan,
    group: &ElementMapGroup,
    material_index: usize,
    appearance_id: &AppearanceId,
    provider_key: &IdentityKey,
    object_id: &str,
) -> Result<(), CodecError> {
    let mut bound = HashSet::new();
    for topology_id in group.names[material_index + 1]
        .iter()
        .flat_map(|name| &name.topology_ids)
    {
        if !insert_hash_set(ctx, &mut bound, topology_id.as_str(),
            "FCStd GUI material face identities")? {
            continue;
        }
        let Some(face) = ir
            .model
            .faces
            .iter()
            .find(|face| face.id.as_str() == *topology_id)
        else {
            continue;
        };
        let face = crate::resource::copied_identity(ctx, face.id.as_str(),
            "FCStd GUI binding face identity")?;
        let binding_index = ir.model.appearance_bindings.len() + plan.bindings.len();
        reserve_vec_items(ctx, &mut plan.bindings, 1, "FCStd GUI planned bindings")?;
        plan.bindings.push(AppearanceBinding {
            id: binding_id(ctx, format_args!(
                "fcstd:appearance:binding#shape-material:{provider_key}:{binding_index}"
            ))?,
            target: AppearanceTarget::Face(face),
            appearance: crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI binding appearance identity")?,
            source_entity_id: Some(retained_string(ctx, object_id, "FCStd GUI binding source identity")?),
            object_type: Some("ViewProvider ShapeAppearance".into()),
            visible: None,
            channels: [(
                cadmpeg_core::nonblank_literal!("precedence"),
                "face_over_object".into(),
            )]
            .into(),
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
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    plan: &mut AppearancePlan,
    provider_name: &str,
    provider_key: &IdentityKey,
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
        gui_malformed(ctx, format_args!(
            "color list references missing entry {entry_name}"
        ))
    })?;
    let colors = parse_color_list(ctx, view, entry_name, requires_alpha_conversion)?;
    let count = colors.len();
    let Some(group) =
        displayed_shape_group(ctx, object_id, properties, payloads, element_maps, kind.name())?
    else {
        return Ok(());
    };
    // FreeCAD uses a single list entry as a uniform color for every mapped subelement.
    let mapped_count = match group.names.len() {
        0 => 0,
        length => length - 1,
    };
    if mapped_count == 0 {
        return Ok(());
    }
    if count != 1 && mapped_count != count {
        push_gui_appearance_loss(ctx, losses,
            FreecadLossCode::AppearanceTopologyColorCountMismatch,
            format_args!(
                "FCStd provider {provider_name} {} color count {count} does not match {mapped_count} mapped subelements; native color list retained and neutral override withheld",
                kind.name()
            ), provenance, "FCStd GUI topology color losses", "FCStd GUI topology color loss text")?;
        return Ok(());
    }
    for (index, packed) in colors.into_iter().enumerate() {
        let appearance_id = topology_appearance_id(ctx, kind, provider_key, index)?;
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
        {
            if !insert_hash_set(ctx, &mut bound_topology, topology_id.as_str(),
                "FCStd GUI colored topology identities")? {
                continue;
            }
            if !match kind {
                TopologyColorKind::Face => ir
                    .model
                    .faces
                    .iter()
                    .any(|face| face.id.as_str() == topology_id.as_str()),
                TopologyColorKind::Edge => ir
                    .model
                    .edges
                    .iter()
                    .any(|edge| edge.id.as_str() == topology_id.as_str()),
                TopologyColorKind::Vertex => ir
                    .model
                    .vertices
                    .iter()
                    .any(|vertex| vertex.id.as_str() == topology_id.as_str()),
            } {
                continue;
            }
            if !emitted_appearance {
                reserve_vec_items(ctx, &mut plan.appearances, 1, "FCStd GUI planned appearances")?;
                plan.appearances.push(Appearance {
                    id: crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI appearance identity copy")?,
                    name: Some(crate::resource::retained_format(ctx, format_args!(
                        "{provider_name} {}{} appearance",
                        kind.name(),
                        index + 1
                    ), "FCStd GUI appearance name")?),
                    asset_guid: None,
                    library_id: None,
                    visual_guid: None,
                    physical_token: None,
                    schema: Some(kind.schema().into()),
                    category: None,
                    base_color: Some(Color::from_rgba8(
                        (packed >> 24) as u8,
                        (packed >> 16) as u8,
                        (packed >> 8) as u8,
                        packed as u8,
                    )),
                    textures: Vec::new(),
                    properties: BTreeMap::new(),
                });
                emitted_appearance = true;
            }
            let target = match kind {
                TopologyColorKind::Face => ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id.as_str() == topology_id.as_str())
                    .map(|face| crate::resource::copied_identity(ctx, face.id.as_str(),
                        "FCStd GUI binding topology identity").map(AppearanceTarget::Face))
                    .transpose()?,
                TopologyColorKind::Edge => ir
                    .model
                    .edges
                    .iter()
                    .find(|edge| edge.id.as_str() == topology_id.as_str())
                    .map(|edge| crate::resource::copied_identity(ctx, edge.id.as_str(),
                        "FCStd GUI binding topology identity").map(AppearanceTarget::Edge))
                    .transpose()?,
                TopologyColorKind::Vertex => ir
                    .model
                    .vertices
                    .iter()
                    .find(|vertex| vertex.id.as_str() == topology_id.as_str())
                    .map(|vertex| crate::resource::copied_identity(ctx, vertex.id.as_str(),
                        "FCStd GUI binding topology identity").map(AppearanceTarget::Vertex))
                    .transpose()?,
            };
            let Some(target) = target else {
                continue;
            };
            let topology_key = crate::native::id_key(topology_id);
            let kind_key = topology_binding_kind(kind);
            reserve_vec_items(ctx, &mut plan.bindings, 1, "FCStd GUI planned bindings")?;
            plan.bindings.push(AppearanceBinding {
                id: binding_id(ctx, format_args!(
                    "fcstd:appearance:binding#{kind_key}:{provider_key}:{}:{topology_key}",
                    index + 1,
                ))?,
                target,
                appearance: crate::resource::copied_identity(ctx, appearance_id.as_str(), "FCStd GUI binding appearance identity")?,
                source_entity_id: Some(retained_string(ctx, object_id, "FCStd GUI binding source identity")?),
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
        ((value >> 24) & 0xff) as f32 / 255.0,
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        transparency.map_or((value & 0xff) as f32 / 255.0, |value| 1.0 - value),
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
    use super::{decode_color, parse_material_list, requires_alpha_conversion};

    fn with_context<T>(bytes: &[u8], f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .expect("material bytes are within service policy");
        f(&ctx)
    }

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

        let materials = with_context(&bytes, |ctx| parse_material_list(
            ctx, cadmpeg_core::decode::View::over_retained(&bytes), 0, "property", true,
        ))
        .expect("material list");
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
            let error = with_context(&bytes, |ctx| parse_material_list(
                ctx, cadmpeg_core::decode::View::over_retained(&bytes), 0, "property", false,
            ))
            .err()
            .expect("nonfinite material scalar");
            assert!(error.to_string().contains("has non-finite scalars"));
        }
    }
}

#[cfg(test)]
mod shape_association_tests {
    use super::{displayed_shape_bodies, displayed_shape_group, select_shape_bodies};
    use crate::brep::{ShapePayload, ShapePayloadRecord};
    use crate::native::element_map::{ElementMapGroup, ElementMapNode, ElementMapRecord};
    use crate::native::{PropertyFamily, PropertyRecord};

    fn shape_property(id: &str) -> PropertyRecord {
        PropertyRecord {
            id: id.into(),
            owner: "object".into(),
            name: "Shape".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: PropertyFamily::Geometry,
            status: None,
            body: crate::native::PropertyBody::Persisted {
                values: Vec::new(),
                links: Vec::new(),
                side_entries: Vec::new(),
                dynamic: None,
            },
            order: 0,
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).unwrap(),
        }
    }

    fn shape_payload(id: &str, property: &str) -> ShapePayloadRecord {
        ShapePayloadRecord {
            id: id.into(),
            property: property.into(),
            entry: "Shape.brp".into(),
            payload: ShapePayload::Empty,
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
            maps: vec![ElementMapNode { map_id: 1, groups }]
                .try_into()
                .expect("valid shape-association map"),
        }
    }

    fn group(indexed_name: &str) -> ElementMapGroup {
        ElementMapGroup {
            indexed_name: indexed_name.into(),
            children: Vec::new(),
            names: Vec::new(),
        }
    }

    fn shape_ir() -> cadmpeg_ir::CadIr {
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.bodies.push(cadmpeg_ir::topology::Body {
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
    fn displayed_shape_body_collection_refuses_at_caller_limit() {
        let ir = shape_ir();
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let error = displayed_shape_bodies(&ctx, &ir, "object", &properties, &payloads)
            .expect_err("displayed body collection must be charged");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && failure.operation == "FCStd GUI displayed shape bodies"), "{error:?}");
    }

    #[test]
    fn displayed_shape_body_identity_refuses_at_retained_limit() {
        let ir = shape_ir();
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = ir.model.bodies[0].id.as_str().len() as u64 - 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let error = displayed_shape_bodies(&ctx, &ir, "object", &properties, &payloads)
            .expect_err("displayed body identity must be charged");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && failure.operation == "FCStd GUI displayed body identity"), "{error:?}");
    }

    #[test]
    fn repeated_payload_body_selection_refuses_at_second_slot() {
        let ir = shape_ir();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let error = select_shape_bodies(&ctx, &ir, ["payload", "payload"])
            .expect_err("second selected body slot must be charged");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && failure.operation == "FCStd GUI displayed shape bodies"), "{error:?}");
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let body_ids = select_shape_bodies(&ctx, &ir, ["payload", "payload"])
            .expect("service policy admits both source occurrences");
        assert_eq!(body_ids, [ir.model.bodies[0].id.clone(), ir.model.bodies[0].id.clone()]);
    }

    #[test]
    fn rejects_ambiguous_shape_association_candidates() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let property = shape_property("property");
        let payload = shape_payload("payload", "property");
        let map = element_map("property", vec![group("Face")]);

        let duplicate_property = shape_property("property-2");
        assert!(matches!(
            displayed_shape_group(
                &ctx,
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
                &ctx,
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
                &ctx,
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
                &ctx,
                "object",
                &[property],
                &[payload],
                &[duplicate_group],
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn duplicate_shape_property_diagnostic_refuses_at_retained_limit() {
        let properties = [shape_property("property"), shape_property("property-2")];
        crate::test_support::assert_retained_refusal_at(&[],
            "FCStd GUI duplicate shape property diagnostic", |ctx| {
                displayed_shape_group(ctx, "object", &properties, &[], &[], "Face")
            });
    }

    #[test]
    fn duplicate_shape_payload_diagnostic_refuses_at_retained_limit() {
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property"), shape_payload("payload-2", "property")];
        crate::test_support::assert_retained_refusal_at(&[],
            "FCStd GUI duplicate shape payload diagnostic", |ctx| {
                displayed_shape_group(ctx, "object", &properties, &payloads, &[], "Face")
            });
    }

    #[test]
    fn duplicate_element_map_diagnostic_refuses_at_retained_limit() {
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        let maps = [element_map("property", vec![group("Face")]),
            element_map("property", vec![group("Face")])];
        crate::test_support::assert_retained_refusal_at(&[],
            "FCStd GUI duplicate element map diagnostic", |ctx| {
                displayed_shape_group(ctx, "object", &properties, &payloads, &maps, "Face")
            });
    }

    #[test]
    fn duplicate_element_group_diagnostic_refuses_at_retained_limit() {
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        let maps = [element_map("property", vec![group("Face"), group("Face")])];
        crate::test_support::assert_retained_refusal_at(&[],
            "FCStd GUI duplicate element group diagnostic", |ctx| {
                displayed_shape_group(ctx, "object", &properties, &payloads, &maps, "Face")
            });
    }
}

#[cfg(test)]
pub(crate) mod tests;
