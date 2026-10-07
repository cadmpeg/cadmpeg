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
    parse_bool, GuiDocumentRecord, GuiPropertyRecord, GuiStateRecord, GuiViewProviderRecord,
    ObjectRecord, PropertyRecord, ValueRecord,
};

use schema::Admission as GuiSchemaAdmission;

#[derive(Default)]
pub(crate) struct Graph {
    pub(crate) documents: Vec<GuiDocumentRecord>,
    pub(crate) providers: Vec<GuiViewProviderRecord>,
    pub(crate) properties: Vec<GuiPropertyRecord>,
    pub(crate) losses: Vec<LossNote>,
}

struct AppearancePlan<'ctx> {
    body_updates: Vec<BodyUpdate>,
    appearances: Vec<Appearance>,
    bindings: Vec<AppearanceBinding>,
    remove_appearances: HashSet<AppearanceId>,
    removed_binding_count: usize,
    presentation_documents: Vec<PresentationDocument>,
    view_presentations: Vec<ViewPresentation>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

struct BodyUpdate {
    id: cadmpeg_ir::ids::BodyId,
    visible: Assignment<Option<bool>>,
    color: Option<Color>,
}

fn push_body_update(
    ctx: &DecodeContext<'_>,
    plan: &mut AppearancePlan<'_>,
    id: &cadmpeg_ir::ids::BodyId,
    visible: Assignment<Option<bool>>,
    color: Result<Option<Color>, CodecError>,
) -> Result<(), CodecError> {
    let id = plan
        .storage
        .with_storage(|| id.try_clone_for_decode(ctx, "FCStd GUI body update identity"))?;
    let color = color?;
    plan.storage
        .with_storage(|| ctx.reserve_vec(&mut plan.body_updates, 1, "FCStd GUI body updates"))?;
    plan.body_updates.push(BodyUpdate { id, visible, color });
    Ok(())
}

enum Assignment<T> {
    Keep,
    Set(T),
}

impl<'ctx> AppearancePlan<'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            body_updates: Vec::new(),
            appearances: Vec::new(),
            bindings: Vec::new(),
            remove_appearances: HashSet::new(),
            removed_binding_count: 0,
            presentation_documents: Vec::new(),
            view_presentations: Vec::new(),
            storage: ctx.reserve_scoped(0, "FCStd GUI appearance plan storage")?,
        })
    }

    fn apply(mut self, ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
        if !self.body_updates.is_empty() {
            let (positions, _position_storage) =
                ctx.with_scoped_storage("FCStd GUI body positions", || {
                    let mut positions = BTreeMap::new();
                    for (index, body) in ctx
                        .admit_iter(&ir.model.bodies, "FCStd GUI body index")?
                        .enumerate()
                        .rev()
                    {
                        ctx.insert_btree_map(
                            &mut positions,
                            body.id
                                .try_clone_for_decode(ctx, "FCStd GUI body index identity")?,
                            index,
                            "FCStd GUI body positions",
                        )?;
                    }
                    Ok::<_, CodecError>(positions)
                })?;
            for update in ctx.admit_iter(self.body_updates, "FCStd GUI body updates")? {
                if let Some(&index) =
                    ctx.get_btree_map(&positions, &update.id, "FCStd GUI body update lookup")?
                {
                    let body = &mut ir.model.bodies[index];
                    if let Assignment::Set(visible) = update.visible {
                        body.visible = visible;
                    }
                    body.color = update.color;
                }
            }
        }
        if !self.remove_appearances.is_empty() {
            ctx.retain_vec(
                &mut self.bindings,
                |binding| {
                    Ok(!ctx.contains_hash_set(
                        &self.remove_appearances,
                        &binding.appearance,
                        "FCStd GUI legacy binding identity",
                    )?)
                },
                "FCStd GUI legacy binding removal",
            )?;
            ctx.retain_vec(
                &mut self.appearances,
                |appearance| {
                    Ok(!ctx.contains_hash_set(
                        &self.remove_appearances,
                        &appearance.id,
                        "FCStd GUI legacy appearance identity",
                    )?)
                },
                "FCStd GUI legacy appearance removal",
            )?;
            ctx.retain_vec(
                &mut ir.model.appearance_bindings,
                |binding| {
                    Ok(!ctx.contains_hash_set(
                        &self.remove_appearances,
                        &binding.appearance,
                        "FCStd GUI appearance binding removal",
                    )?)
                },
                "FCStd GUI appearance binding removal",
            )?;
            ctx.retain_vec(
                &mut ir.model.appearances,
                |appearance| {
                    Ok(!ctx.contains_hash_set(
                        &self.remove_appearances,
                        &appearance.id,
                        "FCStd GUI appearance removal",
                    )?)
                },
                "FCStd GUI appearance removal",
            )?;
        }
        ctx.extend_vec(
            &mut ir.model.appearances,
            self.appearances,
            "FCStd neutral appearances",
        )?;
        ctx.extend_vec(
            &mut ir.model.appearance_bindings,
            self.bindings,
            "FCStd neutral appearance bindings",
        )?;
        ctx.extend_vec(
            &mut ir.model.presentation_documents,
            self.presentation_documents,
            "FCStd neutral presentation documents",
        )?;
        ctx.extend_vec(
            &mut ir.model.view_presentations,
            self.view_presentations,
            "FCStd neutral view presentations",
        )?;
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
    AppearanceId::mint(ctx.format_retained(
        format_args!("fcstd:appearance:object#{provider}"),
        "FCStd GUI object appearance identity",
    )?)
    .map_err(CodecError::malformed)
}

fn edge_appearance_id(
    ctx: &DecodeContext<'_>,
    provider: &IdentityKey,
) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(ctx.format_retained(
        format_args!("fcstd:appearance:edge#{provider}"),
        "FCStd GUI edge appearance identity",
    )?)
    .map_err(CodecError::malformed)
}

fn vertex_appearance_id(
    ctx: &DecodeContext<'_>,
    provider: &IdentityKey,
) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(ctx.format_retained(
        format_args!("fcstd:appearance:vertex#{provider}"),
        "FCStd GUI vertex appearance identity",
    )?)
    .map_err(CodecError::malformed)
}

fn shape_material_appearance_id(
    ctx: &DecodeContext<'_>,
    provider: &IdentityKey,
    index: usize,
) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(ctx.format_retained(
        format_args!("fcstd:appearance:shape-material#{provider}:{}", index + 1),
        "FCStd GUI shape material appearance identity",
    )?)
    .map_err(CodecError::malformed)
}

fn topology_appearance_id(
    ctx: &DecodeContext<'_>,
    kind: TopologyColorKind,
    provider: &IdentityKey,
    index: usize,
) -> Result<AppearanceId, CodecError> {
    let kind = topology_binding_kind(kind);
    AppearanceId::mint(ctx.format_retained(
        format_args!("fcstd:appearance:{kind}#{provider}:{}", index + 1),
        "FCStd GUI topology appearance identity",
    )?)
    .map_err(CodecError::malformed)
}

fn topology_binding_kind(kind: TopologyColorKind) -> IdentityKey {
    match kind {
        TopologyColorKind::Face => cadmpeg_ir::identity_key!("face"),
        TopologyColorKind::Edge => cadmpeg_ir::identity_key!("edge"),
        TopologyColorKind::Vertex => cadmpeg_ir::identity_key!("vertex"),
    }
}

fn binding_id(
    ctx: &DecodeContext<'_>,
    text: std::fmt::Arguments<'_>,
) -> Result<AppearanceBindingId, CodecError> {
    AppearanceBindingId::mint(ctx.format_retained(text, "FCStd GUI appearance binding identity")?)
        .map_err(CodecError::malformed)
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
    pub(crate) objects: &'a [ObjectRecord],
    pub(crate) properties: &'a [PropertyRecord],
    pub(crate) payloads: &'a [ShapePayloadRecord],
    pub(crate) element_maps: &'a [ElementMapRecord],
    pub(crate) requires_alpha_conversion: bool,
}

struct ShapeIndex<'source, 'ctx> {
    properties: BTreeMap<&'source str, Vec<&'source PropertyRecord>>,
    payloads: BTreeMap<&'source str, Vec<&'source ShapePayloadRecord>>,
    maps: BTreeMap<&'source str, Vec<&'source ElementMapRecord>>,
    _storage: [cadmpeg_core::decode::ScopedReservation<'ctx>; 3],
}

impl<'source, 'ctx> ShapeIndex<'source, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        properties: &'source [PropertyRecord],
        payloads: &'source [ShapePayloadRecord],
        maps: &'source [ElementMapRecord],
    ) -> Result<Self, CodecError> {
        let (properties, property_storage) = ctx.collect_scoped_btree_groups(
            ctx.admit_iter(properties, "FCStd GUI Shape property selection")?
                .filter(|property| property.name == "Shape")
                .map(|property| (property.owner.as_str(), property)),
            "FCStd GUI Shape property owners",
        )?;
        let (payloads, payload_storage) = ctx.collect_scoped_btree_groups(
            payloads
                .iter()
                .map(|payload| (payload.property.as_str(), payload)),
            "FCStd GUI Shape payload properties",
        )?;
        let (maps, map_storage) = ctx.collect_scoped_btree_groups(
            maps.iter().map(|map| (map.property.as_str(), map)),
            "FCStd GUI Shape element maps",
        )?;
        Ok(Self {
            properties,
            payloads,
            maps,
            _storage: [property_storage, payload_storage, map_storage],
        })
    }
}

struct TopologyIndex<'source, 'ctx> {
    faces: BTreeMap<&'source str, &'source cadmpeg_ir::ids::FaceId>,
    edges: BTreeMap<&'source str, &'source cadmpeg_ir::ids::EdgeId>,
    vertices: BTreeMap<&'source str, &'source cadmpeg_ir::ids::VertexId>,
    bodies: BTreeMap<&'source str, Vec<&'source cadmpeg_ir::ids::BodyId>>,
    existing_bindings: usize,
    _storage: [cadmpeg_core::decode::ScopedReservation<'ctx>; 4],
}

impl<'source, 'ctx> TopologyIndex<'source, 'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>, ir: &'source CadIr) -> Result<Self, CodecError> {
        let (faces, face_storage) = ctx.collect_scoped_btree_map(
            ir.model
                .faces
                .iter()
                .rev()
                .map(|face| (face.id.as_str(), &face.id)),
            "FCStd GUI face identities",
        )?;
        let (edges, edge_storage) = ctx.collect_scoped_btree_map(
            ir.model
                .edges
                .iter()
                .rev()
                .map(|edge| (edge.id.as_str(), &edge.id)),
            "FCStd GUI edge identities",
        )?;
        let (vertices, vertex_storage) = ctx.collect_scoped_btree_map(
            ir.model
                .vertices
                .iter()
                .rev()
                .map(|vertex| (vertex.id.as_str(), &vertex.id)),
            "FCStd GUI vertex identities",
        )?;
        let mut bodies = BTreeMap::new();
        let mut body_storage = ctx.reserve_scoped(0, "FCStd GUI body payload keys")?;
        for body in ctx.admit_iter(&ir.model.bodies, "FCStd GUI body payload key sources")? {
            let key = ctx
                .split_once(body.id.as_str(), "#", "FCStd GUI body identity key")?
                .map_or(body.id.as_str(), |(_, key)| key);
            let mut suffix = key;
            while let Some((_, rest)) =
                ctx.split_once(suffix, ":", "FCStd GUI body payload key separator")?
            {
                let prefix = &key[..key.len() - rest.len() - 1];
                ctx.push_scoped_btree_group(
                    &mut body_storage,
                    &mut bodies,
                    prefix,
                    || &body.id,
                    0,
                    "FCStd GUI body payload keys",
                )?;
                suffix = rest;
            }
        }
        Ok(Self {
            faces,
            edges,
            vertices,
            bodies,
            existing_bindings: ir.model.appearance_bindings.len(),
            _storage: [face_storage, edge_storage, vertex_storage, body_storage],
        })
    }
}

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    bytes: &[u8],
    sources: &GuiSources<'_, '_>,
) -> Result<Graph, CodecError> {
    let text = ctx
        .validate_utf8(bytes, "validate FreeCAD XML UTF-8")?
        .map_err(|_| CodecError::Malformed("GuiDocument.xml is not UTF-8".into()))?;
    let admitted_xml = ctx.parse_xml(text, "FreeCAD XML tree").map_err(|error| {
        let CodecError::Malformed(error) = error else {
            return error;
        };
        gui_malformed(ctx, format_args!("invalid GuiDocument.xml: {error}"))
    })?;
    let xml = admitted_xml.document();
    let schema_declaration = crate::container::canonical_attribute(
        ctx,
        ctx.xml_root_element(xml, "FCStd GUI document root")?,
        "SchemaVersion",
        "schemaVersion",
    )?;
    let admission = schema::classify(schema_declaration);
    let neutral_schema_version = admission.neutral_schema_version();
    let transferred = transfer_schema_one(
        ctx,
        ir,
        text,
        xml,
        schema_declaration,
        neutral_schema_version,
        sources,
    );
    match (admission, transferred) {
        (GuiSchemaAdmission::Schema1, result) => {
            let (graph, plan) = result?;
            plan.apply(ctx, ir)?;
            Ok(graph)
        }
        (GuiSchemaAdmission::Unverified, Ok((mut graph, plan))) => {
            let declaration = schema_declaration.unwrap_or("missing");
            plan.apply(ctx, ir)?;
            ctx.reserve_vec(&mut graph.losses, 1, "FCStd GUI schema losses")?;
            graph.losses.push(FreecadLossCode::SourceGuiSchemaUnverified.note(
                ctx.format_retained(format_args!(
                    "GuiDocument.xml declares schema {declaration}; decoded with the schema-1 vocabulary"
                ), "FCStd GUI schema loss text")?,
            ));
            Ok(graph)
        }
        (
            GuiSchemaAdmission::Unverified,
            Err(error @ (CodecError::Malformed(_) | CodecError::Truncated { .. })),
        ) => {
            let declaration = schema_declaration.unwrap_or("missing");
            let mut losses = ctx.collection_vec(1, "FCStd GUI schema losses")?;
            losses.push(FreecadLossCode::SourceGuiSchemaUnverified.note(
                ctx.format_retained(format_args!(
                    "GuiDocument.xml could not be decoded with the schema-1 vocabulary; declared schema {declaration} is the probable cause: {error}"
                ), "FCStd GUI schema loss text")?,
            ));
            Ok(Graph {
                losses,
                ..Graph::default()
            })
        }
        (GuiSchemaAdmission::Unverified, Err(error)) => Err(error),
    }
}

fn transfer_schema_one<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    text: &str,
    xml: &roxmltree::Document<'_>,
    schema_declaration: Option<&str>,
    neutral_schema_version: Option<u32>,
    sources: &GuiSources<'_, '_>,
) -> Result<(Graph, AppearancePlan<'ctx>), CodecError> {
    let entries = sources.entries;
    let objects = sources.objects;
    let properties = sources.properties;
    let payloads = sources.payloads;
    let requires_alpha_conversion = sources.requires_alpha_conversion;
    let root = ctx.xml_root_element(xml, "FCStd GUI document root")?;
    let mut plan = AppearancePlan::new(ctx)?;
    let (children, _child_storage) = ctx
        .with_scoped_storage("FCStd GUI document children", || {
            ctx.collect_vec(root.children(), "FCStd GUI document children")
        })?;
    let mut camera_count = 0;
    for node in ctx.admit_iter(&children, "FCStd GUI Camera count")? {
        if ctx.xml_has_tag_name(*node, "Camera", "FCStd GUI state tag")? {
            camera_count += 1;
        }
    }
    if camera_count != 1 {
        return Err(gui_malformed(
            ctx,
            format_args!(
                "GuiDocument.xml schema 1 requires one Camera record, found {camera_count}"
            ),
        ));
    }
    let mut states = Vec::new();
    for node in ctx.admit_iter(&children, "FCStd GUI state selection")? {
        if node.is_element()
            && !ctx.xml_has_tag_name(*node, "ViewProviderData", "FCStd GUI state tag")?
        {
            let order = states.len();
            ctx.push_vec(
                &mut states,
                gui_state(ctx, text, order, *node)?,
                "FCStd GUI state records",
            )?;
        }
    }
    let document = GuiDocumentRecord {
        id: ctx.copy_retained_text("fcstd:gui:document#0", "FCStd GUI document identity")?,
        schema_version: schema_declaration
            .map(|value| ctx.copy_retained_text(value, "FCStd GUI schema declaration"))
            .transpose()?,
        attributes: gui_xml_attributes(
            ctx,
            root,
            "FCStd GUI document attributes",
            "FCStd GUI document attribute name",
            "FCStd GUI document attribute",
        )?,
        states,
    };
    let mut object_storage = ctx.reserve_scoped(0, "FCStd GUI object names")?;
    let mut objects_by_name = HashMap::new();
    for object in ctx.admit_iter(objects, "FCStd GUI object source names")? {
        object_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut objects_by_name,
                object.name().as_str(),
                object.id().as_str(),
                "FCStd GUI object names",
            )
        })?;
    }
    let mut native_providers = Vec::new();
    let mut native_properties = Vec::new();
    let mut losses = Vec::new();
    let shape_index = ShapeIndex::new(ctx, properties, payloads, sources.element_maps)?;
    let topology_index = TopologyIndex::new(ctx, ir)?;
    let (properties_by_id, _property_id_storage) = ctx.collect_scoped_btree_map(
        properties
            .iter()
            .rev()
            .map(|property| (property.id.as_str(), property)),
        "FCStd GUI payload property identities",
    )?;
    let mut payload_storage = ctx.reserve_scoped(0, "FCStd GUI payload owners")?;
    let mut payloads_by_owner = BTreeMap::new();
    for payload in ctx.admit_iter(payloads, "FCStd GUI payload ownership")? {
        if let Some(property) = ctx.get_btree_map(
            &properties_by_id,
            payload.property.as_str(),
            "FCStd GUI payload property lookup",
        )? {
            if property.name == "Shape" {
                ctx.push_scoped_btree_group(
                    &mut payload_storage,
                    &mut payloads_by_owner,
                    property.owner.as_str(),
                    || payload.id.as_str(),
                    0,
                    "FCStd GUI payload owners",
                )?;
            }
        }
    }
    let mut provider_name_storage = ctx.reserve_scoped(0, "FCStd GUI provider names")?;
    let mut provider_names = HashSet::new();
    let mut descendants = xml.descendants();
    let first_view_provider_data = ctx.find_by(
        &mut descendants,
        |node| {
            ctx.xml_has_tag_name(
                *node,
                "ViewProviderData",
                "FCStd GUI provider container tag",
            )
        },
        "FCStd GUI provider container search",
    )?;
    if ctx
        .find_by(
            &mut descendants,
            |node| {
                ctx.xml_has_tag_name(
                    *node,
                    "ViewProviderData",
                    "FCStd GUI provider container tag",
                )
            },
            "FCStd GUI duplicate provider container search",
        )?
        .is_some()
    {
        return Err(CodecError::Malformed(
            "GuiDocument.xml has multiple ViewProviderData containers".into(),
        ));
    }
    let mut provider_storage = ctx.reserve_scoped(0, "FCStd GUI provider nodes")?;
    let mut providers = Vec::new();
    let mut descendants = xml.descendants();
    while let Some(node) =
        ctx.next_charged(&mut descendants, "FCStd GUI provider node selection")?
    {
        if ctx.xml_has_tag_name(node, "ViewProvider", "FCStd GUI provider tag")? {
            provider_storage
                .with_storage(|| ctx.push_vec(&mut providers, node, "FCStd GUI provider nodes"))?;
        }
    }
    if let Some(container) = first_view_provider_data {
        let declared = ctx
            .xml_attribute(container, "Count", "FCStd GUI provider count attribute")?
            .map(|value| {
                ctx.parse_text::<usize>(value, "FCStd GUI provider count")
                    .map(Result::ok)
            })
            .transpose()?
            .flatten()
            .ok_or_else(|| CodecError::Malformed("invalid ViewProviderData Count".into()))?;
        if declared != providers.len() {
            return Err(gui_malformed(
                ctx,
                format_args!(
                    "ViewProviderData Count={declared} but {} records were found",
                    providers.len()
                ),
            ));
        }
    }
    for (provider_order, provider) in ctx
        .admit_iter(providers, "FCStd GUI provider transfer")?
        .enumerate()
    {
        let Some(name) =
            ctx.xml_attribute(provider, "name", "FCStd GUI provider name attribute")?
        else {
            return Err(CodecError::Malformed("ViewProvider has no name".into()));
        };
        if !provider_name_storage.with_storage(|| {
            ctx.insert_hash_set(&mut provider_names, name, "FCStd GUI provider names")
        })? {
            return Err(CodecError::Malformed(
                "GuiDocument.xml has duplicate ViewProvider names".into(),
            ));
        }
        let Some(object_id) = ctx
            .get_hash_map(&objects_by_name, name, "FCStd GUI object name lookup")?
            .copied()
        else {
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
        let (provider_key, _key_storage) = ctx
            .with_scoped_storage("FCStd GUI provider key storage", || {
                provider_identity_key(ctx, name)
            })?;
        append_native_provider(
            ctx,
            text,
            provider,
            provider_order,
            Some(object_id),
            &mut native_providers,
            &mut native_properties,
        )?;
        let properties_node = unique_child(ctx, provider, "Properties")?.ok_or_else(|| {
            gui_malformed(ctx, format_args!("ViewProvider {name} has no Properties"))
        })?;
        let mut value_storage = ctx.reserve_scoped(0, "FCStd GUI presentation values")?;
        let mut values = HashMap::new();
        let mut children = properties_node.children();
        while let Some(property) =
            ctx.next_charged(&mut children, "FCStd GUI presentation property selection")?
        {
            if !ctx.xml_has_tag_name(property, "Property", "FCStd GUI presentation property tag")? {
                continue;
            }
            let Some(property_name) =
                ctx.xml_attribute(property, "name", "FCStd GUI presentation property name")?
            else {
                continue;
            };
            let Some(expected) = presentation_property_type(property_name) else {
                continue;
            };
            if ctx.xml_attribute(property, "type", "FCStd GUI presentation property type")?
                != Some(expected)
            {
                continue;
            }
            if let Some(value) = ctx.find_by(
                property.children(),
                |node| Ok(node.is_element()),
                "FCStd GUI presentation value selection",
            )? {
                value_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut values,
                        property_name,
                        (value, property.range().start),
                        "FCStd GUI presentation values",
                    )
                })?;
            }
        }
        let value_attribute = |property_name: &str,
                               attribute: &str|
         -> Result<Option<&str>, CodecError> {
            match ctx.get_hash_map(
                &values,
                property_name,
                "FCStd GUI presentation value lookup",
            )? {
                Some((node, _)) => {
                    ctx.xml_attribute(*node, attribute, "FCStd GUI presentation value attribute")
                }
                None => Ok(None),
            }
        };
        let property_provenance = |property_name: &str| {
            let offset = ctx
                .get_hash_map(
                    &values,
                    property_name,
                    "FCStd GUI property provenance lookup",
                )?
                .map_or(0, |(_, offset)| {
                    cadmpeg_core::decode::u64_from_index(*offset)
                });
            gui_provider_property_provenance(ctx, name, property_name, offset)
        };
        let visibility = value_attribute("Visibility", "value")?.and_then(parse_bool);
        let transparency = value_attribute("Transparency", "value")?
            .map(|value| {
                ctx.parse_text::<f32>(value, "FCStd GUI presentation scalar")
                    .map(Result::ok)
            })
            .transpose()?
            .flatten()
            .map(|percent| percent / 100.0);
        let packed_color = value_attribute("ShapeColor", "value")?
            .map(|value| {
                ctx.parse_text::<u32>(value, "FCStd GUI presentation scalar")
                    .map(Result::ok)
            })
            .transpose()?
            .flatten()
            .map(|value| convert_packed_alpha(value, requires_alpha_conversion));
        let material = ctx
            .get_hash_map(
                &values,
                "ShapeMaterial",
                "FCStd GUI presentation value lookup",
            )?
            .map(|(node, _)| node);
        let (body_ids, _body_storage) =
            ctx.with_scoped_storage("FCStd GUI displayed bodies", || {
                select_shape_bodies(
                    ctx,
                    &topology_index,
                    ctx.get_btree_map(
                        &payloads_by_owner,
                        object_id,
                        "FCStd GUI Shape payload owner lookup",
                    )?
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .copied(),
                )
            })?;
        for body_id in ctx.admit_iter(&body_ids, "FCStd GUI body update sources")? {
            push_body_update(
                ctx,
                &mut plan,
                body_id,
                Assignment::Set(visibility),
                packed_color
                    .map(|packed| decode_color(packed, transparency))
                    .transpose(),
            )?;
        }
        if let Some(file) = value_attribute("DiffuseColor", "file")? {
            transfer_topology_colors(
                ctx,
                &mut plan,
                TopologyColorRequest {
                    provider_name: name,
                    provider_key: &provider_key,
                    object_id,
                    entry_name: file,
                    kind: TopologyColorKind::Face,
                    provenance: property_provenance("DiffuseColor")?,
                },
                sources,
                &shape_index,
                &topology_index,
                &mut losses,
            )?;
        }
        let (payload_prefixes, _prefix_storage) =
            ctx.with_scoped_storage("FCStd GUI payload prefixes", || {
                shape_payload_prefixes(
                    ctx,
                    ctx.get_btree_map(
                        &payloads_by_owner,
                        object_id,
                        "FCStd GUI Shape payload owner lookup",
                    )?
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                )
            })?;
        if let Some(color) = value_attribute("LineColor", "value")?
            .map(|value| {
                ctx.parse_text::<u32>(value, "FCStd GUI presentation scalar")
                    .map(Result::ok)
            })
            .transpose()?
            .flatten()
            .map(|value| convert_packed_alpha(value, requires_alpha_conversion))
        {
            let width = value_attribute("LineWidth", "value")?;
            transfer_primitive_appearance(
                ctx,
                ir,
                &mut plan,
                &mut losses,
                PrimitiveAppearanceSource {
                    provider_name: name,
                    object_id,
                    packed_color: color,
                    style: PrimitiveStyle::Line(PrimitiveSize::from_source(ctx, width)?),
                    payload_prefixes: &payload_prefixes,
                    provenance: property_provenance("LineWidth")?,
                },
            )?;
        }
        if let Some(file) = value_attribute("LineColorArray", "file")? {
            transfer_topology_colors(
                ctx,
                &mut plan,
                TopologyColorRequest {
                    provider_name: name,
                    provider_key: &provider_key,
                    object_id,
                    entry_name: file,
                    kind: TopologyColorKind::Edge,
                    provenance: property_provenance("LineColorArray")?,
                },
                sources,
                &shape_index,
                &topology_index,
                &mut losses,
            )?;
        }
        if let Some(color) = value_attribute("PointColor", "value")?
            .map(|value| {
                ctx.parse_text::<u32>(value, "FCStd GUI presentation scalar")
                    .map(Result::ok)
            })
            .transpose()?
            .flatten()
            .map(|value| convert_packed_alpha(value, requires_alpha_conversion))
        {
            let size = value_attribute("PointSize", "value")?;
            transfer_primitive_appearance(
                ctx,
                ir,
                &mut plan,
                &mut losses,
                PrimitiveAppearanceSource {
                    provider_name: name,
                    object_id,
                    packed_color: color,
                    style: PrimitiveStyle::Point(PrimitiveSize::from_source(ctx, size)?),
                    payload_prefixes: &payload_prefixes,
                    provenance: property_provenance("PointSize")?,
                },
            )?;
        }
        if let Some(file) = value_attribute("PointColorArray", "file")? {
            transfer_topology_colors(
                ctx,
                &mut plan,
                TopologyColorRequest {
                    provider_name: name,
                    provider_key: &provider_key,
                    object_id,
                    entry_name: file,
                    kind: TopologyColorKind::Vertex,
                    provenance: property_provenance("PointColorArray")?,
                },
                sources,
                &shape_index,
                &topology_index,
                &mut losses,
            )?;
        }
        let Some(packed_color) = packed_color else {
            continue;
        };
        let (appearance_id, _id_storage) = ctx
            .with_scoped_storage("FCStd GUI appearance identity storage", || {
                object_appearance_id(ctx, &provider_key)
            })?;
        let mut material_properties = BTreeMap::new();
        if let Some(material) = material {
            for (source, target) in [
                ("shininess", cadmpeg_core::nonblank_literal!("shininess")),
                (
                    "transparency",
                    cadmpeg_core::nonblank_literal!("material_transparency"),
                ),
            ] {
                if let Some(value) = ctx
                    .xml_attribute(*material, source, "FCStd GUI material scalar attribute")?
                    .map(|value| {
                        ctx.parse_text::<f64>(value, "FCStd GUI presentation scalar")
                            .map(Result::ok)
                    })
                    .transpose()?
                    .flatten()
                    .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                {
                    ctx.insert_btree_map(
                        &mut material_properties,
                        target,
                        value,
                        "FCStd GUI material properties",
                    )?;
                }
            }
        }
        plan.storage.with_storage(|| {
            ctx.reserve_vec(&mut plan.appearances, 1, "FCStd GUI planned appearances")
        })?;
        plan.appearances.push(Appearance {
            id: appearance_id.try_clone_for_decode(ctx, "FCStd GUI appearance identity copy")?,
            name: Some(ctx.format_retained(
                format_args!("{name} shape appearance"),
                "FCStd GUI appearance name",
            )?),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some(ctx.copy_retained_text(
                "FCStd ViewProvider ShapeMaterial",
                "FCStd GUI appearance literal text",
            )?),
            category: None,
            base_color: Some(decode_color(packed_color, transparency)?),
            textures: Vec::new(),
            properties: material_properties,
        });
        for (index, body) in ctx
            .admit_iter(body_ids, "FCStd GUI body binding sources")?
            .enumerate()
        {
            plan.storage.with_storage(|| {
                ctx.reserve_vec(&mut plan.bindings, 1, "FCStd GUI planned bindings")
            })?;
            plan.bindings.push(AppearanceBinding {
                id: binding_id(
                    ctx,
                    format_args!("fcstd:appearance:binding#{provider_key}:{index}"),
                )?,
                target: AppearanceTarget::Body(
                    body.try_clone_for_decode(ctx, "FCStd GUI binding body identity")?,
                ),
                appearance: appearance_id
                    .try_clone_for_decode(ctx, "FCStd GUI binding appearance identity")?,
                source_entity_id: Some(
                    ctx.copy_retained_text(object_id, "FCStd GUI binding source identity")?,
                ),
                object_type: Some(
                    ctx.copy_retained_text("ViewProvider", "FCStd GUI appearance literal text")?,
                ),
                visible: None,
                channels: BTreeMap::new(),
            });
        }
    }
    let mut graph = Graph {
        documents: ctx.collect_vec(std::iter::once(document), "FCStd GUI document records")?,
        providers: native_providers,
        properties: native_properties,
        losses,
    };
    let (material_lists, _material_storage) = ctx
        .with_scoped_storage("FCStd GUI material lookup", || {
            validate_gui_list_payloads(ctx, &graph.properties, entries, requires_alpha_conversion)
        })?;
    let mut material_losses = Vec::new();
    transfer_shape_appearances(
        ctx,
        &mut plan,
        &graph,
        &material_lists,
        &shape_index,
        &topology_index,
        &mut material_losses,
    )?;
    drop(material_lists);
    drop(_material_storage);
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
    ctx.extend_vec(&mut graph.losses, losses, "FCStd GUI graph losses")?;
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
    ctx.reserve_vec(losses, 1, collection_operation)?;
    let text = ctx.format_retained(message, text_operation)?;
    losses.push(code.note(text).with_provenance(provenance));
    Ok(())
}

fn gui_provider_property_provenance(
    ctx: &DecodeContext<'_>,
    provider_name: &str,
    property_name: &str,
    offset: u64,
) -> Result<SourceProvenance, CodecError> {
    let tag = ctx.format_retained(
        format_args!("ViewProvider {provider_name} property {property_name}"),
        "FCStd GUI property provenance tag",
    )?;
    Ok(
        SourceProvenance::in_stream("fcstd", cadmpeg_ir::stream_name!("GuiDocument.xml"), offset)
            .with_tag(tag),
    )
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

fn gui_xml_attributes(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    operation: &'static str,
    name_operation: &'static str,
    value_operation: &'static str,
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut result = BTreeMap::new();
    let mut attributes = node.attributes();
    while let Some(attribute) = ctx.next_charged(&mut attributes, operation)? {
        let name = ctx.copy_retained_text(attribute.name(), name_operation)?;
        let value = ctx.copy_retained_text(attribute.value(), value_operation)?;
        ctx.insert_btree_map(&mut result, name, value, operation)?;
    }
    Ok(result)
}

fn gui_named_entries<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    record: impl Fn() -> Result<String, CodecError>,
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<
    (
        BTreeMap<cadmpeg_core::text::NonBlankString, String>,
        Vec<cadmpeg_core::text::NamedEntryError>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    use cadmpeg_core::text::{NamedEntryError, NonBlankString};
    let mut kept = BTreeMap::new();
    let mut refused = Vec::new();
    let mut refused_storage = ctx.reserve_scoped(0, "FCStd GUI refused property storage")?;
    let mut entries = entries.into_iter();
    while let Some((name, value)) =
        ctx.next_charged(&mut entries, "FCStd GUI named property entries")?
    {
        if ctx.contains_key_btree_map(&kept, name, "FCStd GUI refused property key lookup")? {
            let key = refused_storage
                .with_storage(|| {
                    NonBlankString::for_decode(ctx, name, "FCStd GUI presentation property name")
                        .map_err(CodecError::from)
                })?
                .ok_or_else(|| CodecError::malformed("GUI restated property key is blank"))?;
            let record = refused_storage.with_storage(&record)?;
            refused_storage.with_storage(|| {
                ctx.push_vec(
                    &mut refused,
                    NamedEntryError::Restated { record, key },
                    "FCStd GUI refused property keys",
                )
            })?;
        } else if let Some(key) =
            NonBlankString::for_decode(ctx, name, "FCStd GUI presentation property name")?
        {
            let value = ctx.copy_retained_text(value, "FCStd GUI presentation property value")?;
            ctx.insert_btree_map(&mut kept, key, value, "FCStd GUI presentation property map")?;
        } else {
            let record = refused_storage.with_storage(&record)?;
            refused_storage.with_storage(|| {
                ctx.push_vec(
                    &mut refused,
                    NamedEntryError::Blank { record },
                    "FCStd GUI refused property keys",
                )
            })?;
        }
    }
    Ok((kept, refused, refused_storage))
}

/// Transfers the GUI graph's presentation layer into `plan`.
///
/// A property whose key is blank is charged to `losses` and the rest of the
/// property set survives: a blank key is one unreadable property of one record,
/// not a reason to answer no GUI presentation at all.
fn transfer_neutral_presentation(
    ctx: &DecodeContext<'_>,
    plan: &mut AppearancePlan<'_>,
    graph: &Graph,
    neutral_schema_version: Option<u32>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let mut state_losses = Vec::new();
    for document in ctx.admit_iter(&graph.documents, "FCStd presentation document sources")? {
        let mut presentation = PresentationDocument::new(PresentationId::compose(
            &cadmpeg_ir::identity_namespace!("fcstd", "presentation", "document"),
            cadmpeg_ir::identity_key!("0"),
        ));
        presentation.schema_version = neutral_schema_version;
        presentation.native_ref =
            Some(ctx.copy_retained_text(&document.id, "FCStd presentation document reference")?);
        let mut states = ctx.collection_vec(document.states.len(), "FCStd presentation states")?;
        for (order, state) in ctx
            .admit_iter(&document.states, "FCStd presentation state sources")?
            .enumerate()
        {
            let (attributes, refused, _refused_storage) = gui_named_entries(
                ctx,
                || {
                    ctx.join_retained(
                        &["the gui ", state.kind.as_str(), " state"],
                        "",
                        "FCStd GUI state record name",
                    )
                },
                state
                    .attributes
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            )?;
            charge_refused_gui_keys(ctx, &mut state_losses, &refused)?;
            let kind = if state.kind == "Camera" {
                PresentationStateKind::Camera(camera_state_value(ctx, state, &mut state_losses)?)
            } else {
                PresentationStateKind::Native(
                    ctx.copy_retained_text(&state.kind, "FCStd presentation state kind")?,
                )
            };
            let mut assets =
                ctx.collection_vec(state.side_entries.len(), "FCStd presentation assets")?;
            for entry in ctx.admit_iter(&state.side_entries, "FCStd presentation asset sources")? {
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(states.len()),
            "FCStd presentation state order",
        )?;
        presentation
            .set_states(states)
            .map_err(CodecError::malformed)?;
        plan.storage.with_storage(|| {
            ctx.reserve_vec(
                &mut plan.presentation_documents,
                1,
                "FCStd presentation documents",
            )
        })?;
        plan.presentation_documents.push(presentation);
    }
    ctx.append_vec(losses, &mut state_losses, "FCStd presentation losses")?;

    let (properties, _property_storage) = ctx.collect_scoped_btree_groups(
        graph
            .properties
            .iter()
            .map(|property| (property.owner.as_str(), property)),
        "FCStd presentation property owners",
    )?;
    for provider in ctx.admit_iter(&graph.providers, "FCStd presentation provider sources")? {
        let owned = ctx
            .get_btree_map(
                &properties,
                provider.id.as_str(),
                "FCStd presentation provider property lookup",
            )?
            .map(Vec::as_slice)
            .unwrap_or_default();
        let (property_entries, _entry_storage) =
            ctx.with_scoped_storage("FCStd GUI provider value entries", || {
                let mut entries = Vec::new();
                for property in ctx.admit_iter(owned, "FCStd GUI provider value sources")? {
                    ctx.push_vec(
                        &mut entries,
                        (*property, gui_property_value(ctx, property)?),
                        "FCStd GUI provider value entries",
                    )?;
                }
                Ok::<_, CodecError>(entries)
            })?;
        let (values, _value_storage) = ctx.collect_scoped_btree_map(
            property_entries.iter().rev().map(|(property, value)| {
                (
                    (property.name.as_str(), property.type_name.as_str()),
                    *value,
                )
            }),
            "FCStd GUI provider typed values",
        )?;
        let property_value = |name: &str, type_name: &str| -> Result<Option<&str>, CodecError> {
            Ok(ctx
                .get_btree_map(
                    &values,
                    &(name, type_name),
                    "FCStd GUI provider typed value lookup",
                )?
                .copied()
                .flatten())
        };
        let line_width = property_value("LineWidth", "App::PropertyFloatConstraint")?
            .map(|value| {
                ctx.parse_text::<f64>(value, "FCStd GUI provider size")
                    .map(Result::ok)
            })
            .transpose()?
            .flatten();
        let point_size = property_value("PointSize", "App::PropertyFloatConstraint")?
            .map(|value| {
                ctx.parse_text::<f64>(value, "FCStd GUI provider size")
                    .map(Result::ok)
            })
            .transpose()?
            .flatten();
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
        let (provider_properties, refused, _refused_storage) = gui_named_entries(
            ctx,
            || ctx.copy_retained_text(&provider.id, "FCStd GUI provider record name"),
            property_entries.iter().map(|(property, value)| {
                (
                    property.name.as_str(),
                    value.unwrap_or_else(|| property.xml.text()),
                )
            }),
        )?;
        charge_refused_gui_keys(ctx, losses, &refused)?;
        plan.storage.with_storage(|| {
            ctx.reserve_vec(&mut plan.view_presentations, 1, "FCStd view presentations")
        })?;
        plan.view_presentations.push(ViewPresentation {
            id: PresentationId::mint(crate::native::model_id_charged_at(
                ctx,
                "presentation-view",
                &provider.id,
                "state",
                "FCStd view presentation identity",
            )?)
            .map_err(CodecError::malformed)?,
            object: provider
                .object
                .as_ref()
                .map(|object| ctx.copy_retained_text(object.as_str(), "FCStd view object identity"))
                .transpose()?,
            order: u32::try_from(provider.order).map_err(|_| {
                ctx.refuse_codec_limit(
                    "FreeCAD ordinal",
                    u64::from(u32::MAX),
                    cadmpeg_core::decode::u64_from_index(provider.order),
                )
            })?,
            expanded: provider.expanded,
            visible: property_value("Visibility", "App::PropertyBool")?.and_then(parse_bool),
            display_mode: property_value("DisplayMode", "App::PropertyEnumeration")?
                .map(|value| ctx.copy_retained_text(value, "FCStd view display mode"))
                .transpose()?,
            selection_style: property_value("SelectionStyle", "App::PropertyEnumeration")?
                .map(|value| ctx.copy_retained_text(value, "FCStd view selection style"))
                .transpose()?,
            line_width,
            point_size,
            properties: provider_properties,
            native_ref: Some(ctx.copy_retained_text(&provider.id, "FCStd view native reference")?),
        });
    }
    Ok(())
}

fn gui_property_value<'a>(
    ctx: &DecodeContext<'_>,
    property: &'a GuiPropertyRecord,
) -> Result<Option<&'a str>, CodecError> {
    ctx.find_map(
        &property.values,
        |value| {
            let text = match ctx.get_btree_map(
                &value.attributes,
                "value",
                "FCStd GUI property scalar attribute",
            )? {
                Some(text) => Some(text),
                None => ctx.get_btree_map(
                    &value.attributes,
                    "Value",
                    "FCStd GUI property scalar attribute",
                )?,
            };
            Ok(text.map(String::as_str))
        },
        "FCStd GUI property scalar search",
    )
}

/// One loss per property key the reader could not key, naming the key's own
/// record and, for a restated key, the key.
fn charge_refused_gui_keys(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    refused: &[cadmpeg_core::text::NamedEntryError],
) -> Result<(), CodecError> {
    use cadmpeg_core::text::NamedEntryError;
    for key in ctx.admit_iter(refused, "FCStd GUI refused property loss sources")? {
        let message = match key {
            NamedEntryError::ResourceRefusal(limit) => return Err((*limit).into()),
            NamedEntryError::FormattingRefusal => {
                return Err(CodecError::malformed("cannot format named entry record"))
            }
            NamedEntryError::Blank { record } => ctx.join_retained(
                &[
                    record.as_str(),
                    " states a property with a blank key; the property value is not transferred",
                ],
                "",
                "FCStd GUI refused property note",
            )?,
            NamedEntryError::Restated { record, key } => ctx.join_retained(
                &[
                    record.as_str(),
                    " states the property ",
                    key.as_str(),
                    " a second time; the property value is not transferred",
                ],
                "",
                "FCStd GUI refused property note",
            )?,
        };
        ctx.reserve_vec(losses, 1, "FCStd GUI refused property losses")?;
        losses.push(FreecadLossCode::SourceGuiPropertyKeyBlank.note(message));
    }
    Ok(())
}

fn camera_state_value(
    ctx: &DecodeContext<'_>,
    state: &GuiStateRecord,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<CameraState, CodecError> {
    let settings = ctx
        .get_btree_map(
            &state.attributes,
            "settings",
            "FCStd GUI camera settings lookup",
        )?
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
    let (properties, refused, _refused_storage) = gui_named_entries(
        ctx,
        || {
            ctx.join_retained(
                &["the gui ", state.kind.as_str(), " state"],
                "",
                "FCStd GUI state record name",
            )
        },
        state
            .attributes
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    )?;
    charge_refused_gui_keys(ctx, losses, &refused)?;
    Ok(CameraState {
        position,
        orientation,
        properties,
    })
}

fn parse_camera_settings(
    ctx: &DecodeContext<'_>,
    settings: &str,
) -> Result<CameraSettings, CodecError> {
    if ctx
        .trim_text(settings, "FCStd GUI camera settings trim")?
        .is_empty()
    {
        return Ok(CameraSettings {
            position: None,
            orientation: None,
        });
    }

    let (mut tokens, _token_storage) = ctx.temporary_vec(
        settings.split_whitespace().count(),
        "FCStd GUI camera tokens",
    )?;
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
        let mut dispatch = tokens[index..end].iter();
        let Some(token) = ctx.next_charged(&mut dispatch, "FCStd GUI camera token dispatch")?
        else {
            break;
        };
        match *token {
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
                orientation = Some(camera_field::<4>(
                    ctx,
                    &tokens,
                    index + 1,
                    end,
                    "orientation",
                )?);
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
        gui_malformed(
            ctx,
            format_args!("GUI camera {field} field offset overflows"),
        )
    })?;
    let values = tokens
        .get(start..end_index)
        .filter(|values| values.len() == N && end_index <= end)
        .ok_or_else(|| {
            gui_malformed(ctx, format_args!("GUI camera {field} field is incomplete"))
        })?;
    let mut parsed = [0.0; N];
    for (index, value) in values.iter().enumerate() {
        parsed[index] = ctx
            .parse_text::<f64>(value, "FCStd GUI camera field scalar")?
            .map_err(|_| {
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
    fn from_source(ctx: &DecodeContext<'_>, value: Option<&str>) -> Result<Self, CodecError> {
        Ok(
            match value
                .map(|text| {
                    ctx.parse_text::<f64>(text, "FCStd GUI primitive size")
                        .map(Result::ok)
                })
                .transpose()?
                .flatten()
            {
                None => Self::Absent,
                Some(value) => cadmpeg_ir::scalar::FiniteReal::new(value)
                    .map_or(Self::NonFinite, Self::Admitted),
            },
        )
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
    payloads: &[&str],
) -> Result<Vec<String>, CodecError> {
    let mut prefixes = Vec::new();
    for payload in ctx.admit_iter(payloads, "FCStd GUI payload prefix sources")? {
        ctx.reserve_vec(&mut prefixes, 1, "FCStd GUI payload prefixes")?;
        prefixes.push(
            ctx.retained_suffix(
                ctx.split_once(payload, "#", "FCStd GUI identity key")?
                    .map_or(payload, |(_, key)| key),
                ":",
                "FCStd GUI payload prefix text",
            )?,
        );
    }
    Ok(prefixes)
}

fn transfer_primitive_appearance(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    plan: &mut AppearancePlan<'_>,
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
    let mut target_storage = ctx.reserve_scoped(0, "FCStd GUI primitive targets")?;
    let mut targets = Vec::new();
    match style {
        PrimitiveStyle::Line(_) => {
            for edge in ctx.admit_iter(&ir.model.edges, "FCStd GUI primitive candidates")? {
                let key = ctx
                    .split_once(edge.id.as_str(), "#", "FCStd GUI primitive identity key")?
                    .map_or(edge.id.as_str(), |(_, key)| key);
                if !ctx.any_by(
                    payload_prefixes,
                    |prefix| {
                        ctx.starts_with(key, prefix.as_str(), "FCStd GUI primitive payload prefix")
                    },
                    "FCStd GUI primitive payload prefix search",
                )? {
                    continue;
                }
                target_storage.with_storage(|| {
                    ctx.reserve_vec(&mut targets, 1, "FCStd GUI primitive targets")
                })?;
                targets.push(AppearanceTarget::Edge(
                    edge.id
                        .try_clone_for_decode(ctx, "FCStd GUI primitive target identity")?,
                ));
            }
        }
        PrimitiveStyle::Point(_) => {
            for vertex in ctx.admit_iter(&ir.model.vertices, "FCStd GUI primitive candidates")? {
                let key = ctx
                    .split_once(vertex.id.as_str(), "#", "FCStd GUI primitive identity key")?
                    .map_or(vertex.id.as_str(), |(_, key)| key);
                if !ctx.any_by(
                    payload_prefixes,
                    |prefix| {
                        ctx.starts_with(key, prefix.as_str(), "FCStd GUI primitive payload prefix")
                    },
                    "FCStd GUI primitive payload prefix search",
                )? {
                    continue;
                }
                target_storage.with_storage(|| {
                    ctx.reserve_vec(&mut targets, 1, "FCStd GUI primitive targets")
                })?;
                targets.push(AppearanceTarget::Vertex(
                    vertex
                        .id
                        .try_clone_for_decode(ctx, "FCStd GUI primitive target identity")?,
                ));
            }
        }
    }
    if targets.is_empty() {
        return Ok(());
    }
    let (provider_key, _key_storage) = ctx
        .with_scoped_storage("FCStd GUI provider key storage", || {
            provider_identity_key(ctx, provider_name)
        })?;
    let ((appearance_id, label, property, size, binding_key, object_type, precedence), _id_storage) =
        ctx.with_scoped_storage("FCStd GUI primitive identity storage", || {
            Ok::<_, CodecError>(match style {
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
            })
        })?;
    let admitted_size = match size {
        PrimitiveSize::Admitted(value) if value.get() >= 0.0 => Some(value),
        PrimitiveSize::Absent | PrimitiveSize::Admitted(_) | PrimitiveSize::NonFinite => None,
    };
    if matches!(size, PrimitiveSize::NonFinite | PrimitiveSize::Admitted(_))
        && admitted_size.is_none()
    {
        push_gui_appearance_loss(
            ctx,
            losses,
            FreecadLossCode::AppearancePrimitiveSizeNotTransferred,
            format_args!(
                "FCStd provider {provider_name} {label} size cannot enter the neutral appearance"
            ),
            provenance,
            "FCStd GUI primitive size losses",
            "FCStd GUI primitive size loss text",
        )?;
    }
    plan.storage.with_storage(|| {
        ctx.reserve_vec(&mut plan.appearances, 1, "FCStd GUI planned appearances")
    })?;
    plan.appearances.push(Appearance {
        id: appearance_id.try_clone_for_decode(ctx, "FCStd GUI appearance identity copy")?,
        name: Some(ctx.format_retained(
            format_args!("{provider_name} {label} appearance"),
            "FCStd GUI appearance name",
        )?),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some(ctx.format_retained(
            format_args!("FCStd ViewProvider {label} style"),
            "FCStd GUI appearance schema",
        )?),
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
        properties: {
            let mut properties = BTreeMap::new();
            if let Some(width) = admitted_size {
                ctx.insert_btree_map(
                    &mut properties,
                    property,
                    width,
                    "FCStd GUI primitive properties",
                )?;
            }
            properties
        },
    });
    for (index, target) in ctx
        .admit_iter(targets, "FCStd GUI primitive binding sources")?
        .enumerate()
    {
        plan.storage.with_storage(|| {
            ctx.reserve_vec(&mut plan.bindings, 1, "FCStd GUI planned bindings")
        })?;
        plan.bindings.push(AppearanceBinding {
            id: binding_id(
                ctx,
                format_args!("fcstd:appearance:binding#{binding_key}:{provider_key}:{index}"),
            )?,
            target,
            appearance: appearance_id
                .try_clone_for_decode(ctx, "FCStd GUI binding appearance identity")?,
            source_entity_id: Some(
                ctx.copy_retained_text(object_id, "FCStd GUI binding source identity")?,
            ),
            object_type: Some(
                ctx.copy_retained_text(object_type, "FCStd GUI binding object type")?,
            ),
            visible: None,
            channels: {
                let mut channels = BTreeMap::new();
                ctx.insert_btree_map(
                    &mut channels,
                    cadmpeg_core::nonblank_literal!("precedence"),
                    ctx.copy_retained_text(precedence, "FCStd GUI binding precedence")?,
                    "FCStd GUI binding channels",
                )?;
                channels
            },
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
    let mut values = Vec::new();
    let mut descendants = node.descendants();
    while let Some(value) =
        ctx.next_charged(&mut descendants, "FCStd GUI state values traversal")?
    {
        if !value.is_element() || value == node {
            continue;
        }
        let value_order = values.len();
        ctx.reserve_vec(&mut values, 1, "FCStd GUI state values")?;
        values.push(ValueRecord {
            tag: ctx.copy_retained_text(value.tag_name().name(), "FCStd GUI value tag")?,
            order: value_order,
            attributes: gui_xml_attributes(
                ctx,
                value,
                "FCStd GUI value attributes",
                "FCStd GUI attribute name",
                "FCStd GUI attribute",
            )?,
            text: value
                .text()
                .map(|text| ctx.copy_retained_text(text, "FCStd GUI value text"))
                .transpose()?,
            raw_xml: ctx.copy_retained_text(&text[value.range()], "FCStd GUI value XML")?,
        });
    }
    let mut side_entries = Vec::new();
    let mut descendants = node.descendants();
    while let Some(element) =
        ctx.next_charged(&mut descendants, "FCStd GUI state side-reference elements")?
    {
        if !element.is_element() {
            continue;
        }
        let mut attributes = element.attributes();
        while let Some(attribute) =
            ctx.next_charged(&mut attributes, "FCStd GUI state side-reference attributes")?
        {
            if matches!(attribute.name(), "file" | "File") && !attribute.value().is_empty() {
                ctx.reserve_vec(&mut side_entries, 1, "FCStd GUI side entry references")?;
                side_entries
                    .push(ctx.copy_retained_text(attribute.value(), "FCStd GUI side entry name")?);
            }
        }
    }
    let kind = ctx.copy_retained_text(node.tag_name().name(), "FCStd GUI state kind")?;
    let attributes = gui_xml_attributes(
        ctx,
        node,
        "FCStd GUI state attributes",
        "FCStd GUI state attribute name",
        "FCStd GUI state attribute",
    )?;
    let xml = crate::native::RetainedXml::from_source(
        ctx,
        &text[node.range()],
        cadmpeg_core::decode::u64_from_index(node.range().start),
        "FCStd GUI state XML",
    )?;
    let order = order.to_string();
    let (key, _key_storage) =
        ctx.with_scoped_storage("FCStd GUI state identity storage", || {
            ctx.join_retained(
                &[kind.as_str(), order.as_str()],
                ":",
                "FCStd GUI state identity key",
            )
        })?;
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
    ctx: &DecodeContext<'_>,
    parent: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Result<Option<roxmltree::Node<'a, 'input>>, CodecError> {
    let mut children = parent.children();
    let first = ctx.find_by(
        &mut children,
        |child| ctx.xml_has_tag_name(*child, tag, "FCStd GUI child container tag"),
        "FCStd GUI child container search",
    )?;
    if first.is_some()
        && ctx
            .find_by(
                &mut children,
                |child| ctx.xml_has_tag_name(*child, tag, "FCStd GUI child container tag"),
                "FCStd GUI duplicate child container search",
            )?
            .is_some()
    {
        return Err(CodecError::Malformed(
            "GUI record has multiple child containers".into(),
        ));
    }
    Ok(first)
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
    let name = ctx
        .xml_attribute(provider, "name", "FCStd GUI provider attribute")?
        .ok_or_else(|| CodecError::Malformed("ViewProvider has no name".into()))?;
    let (id, _id_storage) = ctx
        .with_scoped_storage("FCStd GUI provider identity storage", || {
            crate::native::native_id_charged(ctx, "gui-view-provider", name)
        })?;
    ctx.reserve_vec(providers, 1, "FCStd GUI provider records")?;
    providers.push(GuiViewProviderRecord {
        id: ctx.copy_retained_text(&id, "FCStd GUI provider record identity")?,
        object: object
            .map(|object| {
                cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    ctx.copy_retained_text(object, "FCStd GUI provider object identity")?,
                    "validate nonblank text",
                )?
                .ok_or_else(|| {
                    CodecError::Malformed("GUI provider object must not be empty".into())
                })
            })
            .transpose()?,
        name: ctx.copy_retained_text(name, "FCStd GUI provider name")?,
        expanded: ctx
            .xml_attribute(provider, "expanded", "FCStd GUI provider attribute")?
            .and_then(parse_bool),
        order,
        raw_xml: ctx.copy_retained_text(&text[provider.range()], "FCStd GUI provider XML")?,
    });
    let Some(container) = unique_child(ctx, provider, "Properties")? else {
        return Err(gui_malformed(
            ctx,
            format_args!("ViewProvider {name} has no Properties"),
        ));
    };
    let (property_nodes, _node_storage) =
        ctx.with_scoped_storage("FCStd GUI provider property nodes", || {
            let mut nodes = Vec::new();
            let mut children = container.children();
            while let Some(node) =
                ctx.next_charged(&mut children, "FCStd GUI provider property child traversal")?
            {
                if ctx.xml_has_tag_name(node, "Property", "FCStd GUI provider property tag")? {
                    ctx.push_vec(&mut nodes, node, "FCStd GUI provider property nodes")?;
                }
            }
            Ok::<_, CodecError>(nodes)
        })?;
    let declared = ctx
        .xml_attribute(
            container,
            "Count",
            "FCStd GUI provider property count attribute",
        )?
        .map(|value| {
            ctx.parse_text::<usize>(value, "FCStd GUI provider property count")
                .map(Result::ok)
        })
        .transpose()?
        .flatten()
        .ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("ViewProvider {name} has invalid property count"),
            )
        })?;
    if declared != property_nodes.len() {
        return Err(gui_malformed(
            ctx,
            format_args!(
                "ViewProvider {name} declares {declared} properties but contains {}",
                property_nodes.len()
            ),
        ));
    }
    let mut name_storage = ctx.reserve_scoped(0, "FCStd GUI property names")?;
    let mut names = HashSet::new();
    for (property_order, property) in ctx
        .admit_iter(property_nodes, "FCStd GUI provider property transfer")?
        .enumerate()
    {
        let property_name = ctx
            .xml_attribute(property, "name", "FCStd GUI provider attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("ViewProvider {name} property has no name"),
                )
            })?;
        if !name_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut names,
                (id.as_str(), property_name),
                "FCStd GUI property names",
            )
        })? {
            return Err(CodecError::Malformed(
                "ViewProvider has duplicate property names".into(),
            ));
        }
        let type_name = ctx
            .xml_attribute(property, "type", "FCStd GUI provider attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("ViewProvider {name}.{property_name} has no type"),
                )
            })?;
        validate_gui_property(ctx, property, property_name, type_name)?;
        let mut values = Vec::new();
        let mut descendants = property.descendants();
        while let Some(value) =
            ctx.next_charged(&mut descendants, "FCStd GUI property values traversal")?
        {
            if !value.is_element() || value == property {
                continue;
            }
            let value_order = values.len();
            ctx.reserve_vec(&mut values, 1, "FCStd GUI property values")?;
            values.push(ValueRecord {
                tag: ctx.copy_retained_text(value.tag_name().name(), "FCStd GUI value tag")?,
                order: value_order,
                attributes: gui_xml_attributes(
                    ctx,
                    value,
                    "FCStd GUI value attributes",
                    "FCStd GUI attribute name",
                    "FCStd GUI attribute",
                )?,
                text: value
                    .text()
                    .map(|text| ctx.copy_retained_text(text, "FCStd GUI value text"))
                    .transpose()?,
                raw_xml: ctx.copy_retained_text(&text[value.range()], "FCStd GUI value XML")?,
            });
        }
        let mut side_entries = Vec::new();
        if !crate::persistence::is_xlink_type(type_name) {
            for value in ctx.admit_iter(&values, "FCStd GUI property side-reference values")? {
                for (attribute, entry) in ctx.admit_iter(
                    &value.attributes,
                    "FCStd GUI property side-reference attributes",
                )? {
                    if matches!(attribute.as_str(), "file" | "File") && !entry.is_empty() {
                        ctx.reserve_vec(&mut side_entries, 1, "FCStd GUI side entry references")?;
                        side_entries
                            .push(ctx.copy_retained_text(entry, "FCStd GUI side entry name")?);
                    }
                }
            }
        }
        ctx.reserve_vec(properties, 1, "FCStd GUI property records")?;
        properties.push(GuiPropertyRecord {
            id: crate::native::native_child_id_charged(ctx, "gui-property", &id, property_name)?,
            owner: ctx.copy_retained_text(&id, "FCStd GUI property owner")?,
            name: ctx.copy_retained_text(property_name, "FCStd GUI property name")?,
            type_name: ctx.copy_retained_text(type_name, "FCStd GUI property type")?,
            status: ctx
                .xml_attribute(property, "status", "FCStd GUI provider attribute")?
                .map(|value| {
                    ctx.parse_text(value, "FCStd GUI property status")
                        .map(Result::ok)
                })
                .transpose()?
                .flatten(),
            order: property_order,
            values,
            side_entries,
            xml: crate::native::RetainedXml::from_source(
                ctx,
                &text[property.range()],
                cadmpeg_core::decode::u64_from_index(property.range().start),
                "FCStd GUI property XML",
            )?,
        });
    }
    Ok(())
}

fn gui_single_element_child<'a, 'input>(
    ctx: &DecodeContext<'_>,
    parent: roxmltree::Node<'a, 'input>,
    operation: &'static str,
) -> Result<Option<roxmltree::Node<'a, 'input>>, CodecError> {
    let mut children = parent.children();
    let first = ctx.find_by(&mut children, |child| Ok(child.is_element()), operation)?;
    if first.is_some()
        && ctx
            .find_by(&mut children, |child| Ok(child.is_element()), operation)?
            .is_some()
    {
        return Ok(None);
    }
    Ok(first)
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
        return crate::persistence::validate_link_property(ctx, property, type_name);
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
                "TechDraw::GeomFormat",
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
                "TechDraw::CosmeticVertex",
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
                "TechDraw::CosmeticEdge",
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
                "TechDraw::CenterLine",
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
    let mut roots = property.children();
    let root = ctx
        .find_by(
            &mut roots,
            |node| Ok(node.is_element()),
            "FCStd GUI property root elements",
        )?
        .ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("GUI property {property_name} requires one {expected_tag} value"),
            )
        })?;
    let second_root = ctx.find_by(
        &mut roots,
        |node| Ok(node.is_element()),
        "FCStd GUI property root elements",
    )?;
    let has_more_roots = ctx
        .find_by(
            &mut roots,
            |node| Ok(node.is_element()),
            "FCStd GUI property root elements",
        )?
        .is_some();
    if !ctx.xml_has_tag_name(root, expected_tag, "FCStd GUI value tag")? {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires a leading {expected_tag} value"),
        ));
    }
    let scalar = |attribute: &str| -> Result<&str, CodecError> {
        ctx.xml_attribute(root, attribute, "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!(
                        "GUI property {property_name} {expected_tag} has no {attribute} attribute"
                    ),
                )
            })
    };
    match tag {
        GuiValueTag::Bool => {
            if parse_bool(scalar("value")?).is_none() {
                return Err(gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} has an invalid Boolean"),
                ));
            }
        }
        GuiValueTag::Integer => {
            ctx.parse_text::<i64>(scalar("value")?, "FCStd GUI property integer")?
                .map_err(|_| {
                    gui_malformed(
                        ctx,
                        format_args!("GUI property {property_name} has an invalid integer"),
                    )
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
            let value = ctx
                .parse_text::<f64>(scalar("value")?, "FCStd GUI property float")?
                .map_err(|_| {
                    gui_malformed(
                        ctx,
                        format_args!("GUI property {property_name} has an invalid float"),
                    )
                })?;
            if !value.is_finite() {
                return Err(gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} has a non-finite float"),
                ));
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
                && has_nested_gui_elements(ctx, root)?
            {
                return Err(gui_nested_value_error(ctx, property_name, expected_tag));
            }
            if type_name == "App::PropertyPersistentObject" {
                if has_more_roots
                    || !match second_root {
                        Some(node) => {
                            ctx.xml_has_tag_name(node, "PersistentObject", "FCStd GUI value tag")?
                        }
                        None => false,
                    }
                {
                    return Err(gui_malformed(
                        ctx,
                        format_args!(
                        "GUI property {property_name} has an invalid persistent-object envelope"
                    ),
                    ));
                }
                return Ok(());
            }
            if tag == GuiValueTag::MaterialList {
                let version = match ctx.xml_attribute(
                    root,
                    "version",
                    "FCStd GUI value attribute",
                )? {
                    Some(value) => {
                        match ctx.parse_text::<u32>(value, "FCStd GUI material-list version")? {
                            Ok(version) => version,
                            Err(_) => {
                                return Err(gui_malformed(
                                ctx,
                                format_args!(
                                    "GUI property {property_name} has an invalid material-list version"
                                ),
                            ));
                            }
                        }
                    }
                    None => 0,
                };
                if version > 3 {
                    return Err(CodecError::NotImplemented(format!(
                        "FCStd GUI material-list version {version}"
                    )));
                }
            }
        }
        GuiValueTag::PropertyColor => {
            ctx.parse_text::<u32>(scalar("value")?, "FCStd GUI property color")?
                .map_err(|_| {
                    gui_malformed(
                        ctx,
                        format_args!("GUI property {property_name} has an invalid color"),
                    )
                })?;
        }
        GuiValueTag::PropertyVector => {
            for attribute in ["valueX", "valueY", "valueZ"] {
                let value = ctx
                    .parse_text::<f64>(scalar(attribute)?, "FCStd GUI property vector scalar")?
                    .map_err(|_| {
                        gui_malformed(
                            ctx,
                            format_args!("GUI property {property_name} has an invalid vector"),
                        )
                    })?;
                if !value.is_finite() {
                    return Err(gui_malformed(
                        ctx,
                        format_args!("GUI property {property_name} has a non-finite vector"),
                    ));
                }
            }
        }
        GuiValueTag::PropertyMaterial => validate_gui_material(ctx, root, property_name)?,
        GuiValueTag::BoolList => {
            if !ctx.all_by(
                scalar("value")?.bytes(),
                |byte| Ok(matches!(byte, b'0' | b'1')),
                "FCStd GUI Boolean list values",
            )? {
                return Err(gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} has an invalid Boolean list"),
                ));
            }
            if has_nested_gui_elements(ctx, root)? {
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
                    let value = ctx
                        .parse_text::<f64>(scalar(&attribute)?, "FCStd GUI property matrix scalar")?
                        .map_err(|_| {
                            gui_malformed(
                                ctx,
                                format_args!(
                                    "GUI property {property_name} has an invalid matrix value"
                                ),
                            )
                        })?;
                    if !value.is_finite() {
                        return Err(gui_malformed(
                            ctx,
                            format_args!(
                                "GUI property {property_name} has a non-finite matrix value"
                            ),
                        ));
                    }
                }
            }
        }
        GuiValueTag::PropertyPlacement => validate_gui_placement(ctx, root, property_name)?,
        GuiValueTag::PropertyRotation => {
            for attribute in ["A", "Ox", "Oy", "Oz"] {
                let value = ctx
                    .parse_text::<f64>(scalar(attribute)?, "FCStd GUI property rotation scalar")?
                    .map_err(|_| {
                        gui_malformed(
                            ctx,
                            format_args!("GUI property {property_name} has an invalid rotation"),
                        )
                    })?;
                if !value.is_finite() {
                    return Err(gui_malformed(
                        ctx,
                        format_args!("GUI property {property_name} has a non-finite rotation"),
                    ));
                }
            }
        }
        GuiValueTag::Uuid | GuiValueTag::Path => {
            scalar("value")?;
        }
        GuiValueTag::FloatList | GuiValueTag::VectorList | GuiValueTag::PlacementList => {
            scalar("file")?;
            if has_nested_gui_elements(ctx, root)? {
                return Err(gui_nested_value_error(ctx, property_name, expected_tag));
            }
        }
        GuiValueTag::FileIncluded => {
            let has_file = ctx
                .xml_attribute(root, "file", "FCStd GUI value attribute")?
                .is_some();
            let has_data = ctx
                .xml_attribute(root, "data", "FCStd GUI value attribute")?
                .is_some();
            if has_file == has_data {
                return Err(gui_malformed(ctx, format_args!(
                    "GUI property {property_name} FileIncluded requires exactly one file or data attribute"
                )));
            }
            if has_nested_gui_elements(ctx, root)? {
                return Err(gui_nested_value_error(ctx, property_name, "FileIncluded"));
            }
        }
    }
    if second_root.is_some() {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires exactly one {expected_tag} value"),
        ));
    }
    Ok(())
}

fn validate_gui_string_list(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let count = gui_list_count(ctx, root, property_name, "StringList")?;
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI StringList child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI StringList child nodes")
        })?;
    let element_count = ctx
        .admit_iter(&children, "FCStd GUI StringList element count")?
        .filter(|value| value.is_element())
        .count();
    if element_count != count
        || ctx.any_by(
            &children,
            |value| {
                if !value.is_element() {
                    return Ok(false);
                }
                Ok(
                    !ctx.xml_has_tag_name(*value, "String", "FCStd GUI value tag")?
                        || ctx
                            .xml_attribute(*value, "value", "FCStd GUI value attribute")?
                            .is_none(),
                )
            },
            "FCStd GUI StringList value validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} StringList count or value is invalid"),
        ));
    }
    if ctx.any_by(
        &children,
        |value| {
            if value.is_element() {
                has_nested_gui_elements(ctx, *value)
            } else {
                Ok(false)
            }
        },
        "FCStd GUI StringList nested value scan",
    )? {
        return Err(gui_nested_value_error(
            ctx,
            property_name,
            "StringList value",
        ));
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
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI integer-list child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI integer-list child nodes")
        })?;
    let element_count = ctx
        .admit_iter(&children, "FCStd GUI integer-list element count")?
        .filter(|value| value.is_element())
        .count();
    if element_count != count
        || ctx.any_by(
            &children,
            |value| {
                Ok(value.is_element()
                    && !ctx.xml_has_tag_name(*value, "I", "FCStd GUI value tag")?)
            },
            "FCStd GUI integer-list tag validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} {tag} count or value is invalid"),
        ));
    }
    if ctx.any_by(
        &children,
        |value| {
            if value.is_element() {
                has_nested_gui_elements(ctx, *value)
            } else {
                Ok(false)
            }
        },
        "FCStd GUI integer-list nested value scan",
    )? {
        return Err(gui_nested_value_error(ctx, property_name, tag));
    }
    let mut previous = None;
    for value in ctx.admit_iter(&children, "FCStd GUI integer-list values")? {
        if !value.is_element() {
            continue;
        }
        let number_text = ctx
            .xml_attribute(*value, "v", "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} {tag} value has no v attribute"),
                )
            })?;
        let number = ctx
            .parse_text::<i64>(number_text, "FCStd GUI integer-list value")?
            .map_err(|_| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} {tag} value is not an integer"),
                )
            })?;
        if require_sorted_unique && previous.is_some_and(|previous| number <= previous) {
            return Err(gui_malformed(
                ctx,
                format_args!("GUI property {property_name} IntegerSet is not sorted and unique"),
            ));
        }
        previous = Some(number);
    }
    Ok(())
}

fn validate_gui_map(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let count = gui_list_count(ctx, root, property_name, "Map")?;
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI Map child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI Map child nodes")
        })?;
    let element_count = ctx
        .admit_iter(&children, "FCStd GUI Map element count")?
        .filter(|value| value.is_element())
        .count();
    if element_count != count
        || ctx.any_by(
            &children,
            |value| {
                Ok(value.is_element()
                    && !ctx.xml_has_tag_name(*value, "Item", "FCStd GUI value tag")?)
            },
            "FCStd GUI Map item tag validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} Map count or item tag is invalid"),
        ));
    }
    if ctx.any_by(
        &children,
        |value| {
            if value.is_element() {
                has_nested_gui_elements(ctx, *value)
            } else {
                Ok(false)
            }
        },
        "FCStd GUI Map nested item scan",
    )? {
        return Err(gui_nested_value_error(ctx, property_name, "Map item"));
    }
    let mut previous_key = None;
    for value in ctx.admit_iter(&children, "FCStd GUI Map values")? {
        if !value.is_element() {
            continue;
        }
        let key = ctx
            .xml_attribute(*value, "key", "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} Map item has no key"),
                )
            })?;
        if ctx
            .xml_attribute(*value, "value", "FCStd GUI value attribute")?
            .is_none()
        {
            return Err(gui_malformed(
                ctx,
                format_args!("GUI property {property_name} Map item has no value"),
            ));
        }
        if let Some(previous) = previous_key {
            if ctx.compare(key, previous, "FCStd GUI Map key order")? != std::cmp::Ordering::Greater
            {
                return Err(gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} Map keys are not sorted and unique"),
                ));
            }
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
    let count_text = ctx
        .xml_attribute(root, "count", "FCStd GUI value attribute")?
        .ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("GUI property {property_name} {tag} has no count"),
            )
        })?;
    ctx.parse_text::<usize>(count_text, "FCStd GUI list count")?
        .map_err(|_| {
            gui_malformed(
                ctx,
                format_args!("GUI property {property_name} {tag} has an invalid count"),
            )
        })
}

fn has_nested_gui_elements(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
) -> Result<bool, CodecError> {
    ctx.any_by(
        node.children(),
        |child| Ok(child.is_element()),
        "FCStd GUI nested XML element search",
    )
}

fn gui_nested_value_error(
    ctx: &DecodeContext<'_>,
    property_name: &str,
    value_name: &str,
) -> CodecError {
    crate::resource::malformed_charged(
        ctx,
        format_args!("GUI property {property_name} {value_name} has nested element values"),
        "FCStd GUI nested-value diagnostic",
    )
}

fn validate_gui_constraint_attributes(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
    integer: bool,
) -> Result<(), CodecError> {
    for attribute in ["min", "max", "step"] {
        let Some(value) = ctx.xml_attribute(root, attribute, "FCStd GUI value attribute")? else {
            continue;
        };
        if integer {
            ctx.parse_text::<i64>(value, "FCStd GUI constraint integer")?
                .map_err(|_| {
                    gui_constraint_error(ctx, property_name, "an invalid integer", attribute)
                })?;
        } else {
            let value = ctx
                .parse_text::<f64>(value, "FCStd GUI constraint scalar")?
                .map_err(|_| {
                    gui_constraint_error(ctx, property_name, "an invalid float", attribute)
                })?;
            if !value.is_finite() {
                return Err(gui_constraint_error(
                    ctx,
                    property_name,
                    "a non-finite",
                    attribute,
                ));
            }
        }
    }
    Ok(())
}

fn gui_constraint_error(
    ctx: &DecodeContext<'_>,
    property_name: &str,
    detail: &str,
    attribute: &str,
) -> CodecError {
    crate::resource::malformed_charged(
        ctx,
        format_args!("GUI property {property_name} has {detail} {attribute}"),
        "FCStd GUI constraint diagnostic",
    )
}

fn validate_gui_placement(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    for attribute in ["Px", "Py", "Pz"] {
        let text = ctx
            .xml_attribute(root, attribute, "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} placement has no {attribute}"),
                )
            })?;
        let value = ctx
            .parse_text::<f64>(text, "FCStd GUI placement scalar")?
            .map_err(|_| {
                gui_malformed(
                    ctx,
                    format_args!(
                        "GUI property {property_name} placement has an invalid {attribute}"
                    ),
                )
            })?;
        if !value.is_finite() {
            return Err(gui_malformed(
                ctx,
                format_args!("GUI property {property_name} placement has a non-finite {attribute}"),
            ));
        }
    }
    let axis_attributes = ["A", "Ox", "Oy", "Oz"];
    let quaternion_attributes = ["Q0", "Q1", "Q2", "Q3"];
    let has_axis = ctx
        .xml_attribute(root, "A", "FCStd GUI value attribute")?
        .is_some();
    let orientation = if has_axis {
        &axis_attributes[..]
    } else {
        &quaternion_attributes[..]
    };
    for &attribute in orientation {
        let text = ctx
            .xml_attribute(root, attribute, "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} placement has no {attribute}"),
                )
            })?;
        let value = ctx
            .parse_text::<f64>(text, "FCStd GUI placement orientation scalar")?
            .map_err(|_| {
                gui_malformed(
                    ctx,
                    format_args!(
                        "GUI property {property_name} placement has an invalid {attribute}"
                    ),
                )
            })?;
        if !value.is_finite() {
            return Err(gui_malformed(
                ctx,
                format_args!("GUI property {property_name} placement has a non-finite {attribute}"),
            ));
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
    let custom = ctx
        .xml_attribute(integer, "CustomEnum", "FCStd GUI value attribute")?
        .is_some();
    if !custom && custom_list.is_none() {
        return Ok(());
    }
    let custom_list = match custom_list {
        Some(node)
            if custom
                && !has_more_roots
                && ctx.xml_has_tag_name(node, "CustomEnumList", "FCStd GUI value tag")? =>
        {
            node
        }
        _ => {
            return Err(gui_malformed(
                ctx,
                format_args!(
                    "GUI property {property_name} has an invalid custom enumeration envelope"
                ),
            ))
        }
    };
    let count = match ctx.xml_attribute(custom_list, "count", "FCStd GUI value attribute")? {
        Some(value) => {
            match ctx.parse_text::<usize>(value, "FCStd GUI custom enumeration count")? {
                Ok(count) => count,
                Err(_) => {
                    return Err(gui_malformed(
                        ctx,
                        format_args!(
                            "GUI property {property_name} has an invalid custom enumeration count"
                        ),
                    ));
                }
            }
        }
        None => {
            return Err(gui_malformed(
                ctx,
                format_args!(
                    "GUI property {property_name} has an invalid custom enumeration count"
                ),
            ));
        }
    };
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI custom enumeration nodes", || {
            ctx.collect_vec(custom_list.children(), "FCStd GUI custom enumeration nodes")
        })?;
    let element_count = ctx
        .admit_iter(&children, "FCStd GUI custom enumeration element count")?
        .filter(|value| value.is_element())
        .count();
    if element_count != count
        || ctx.any_by(
            &children,
            |value| {
                if !value.is_element() {
                    return Ok(false);
                }
                Ok(
                    !ctx.xml_has_tag_name(*value, "Enum", "FCStd GUI value tag")?
                        || ctx
                            .xml_attribute(*value, "value", "FCStd GUI value attribute")?
                            .is_none(),
                )
            },
            "FCStd GUI custom enumeration value validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!(
                "GUI property {property_name} custom enumeration count or value is invalid"
            ),
        ));
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
    let Some(root) = gui_single_element_child(ctx, property, "FCStd GUI geometry root selection")?
    else {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!("GUI property {property_name} requires exactly one {expected_tag} value"),
            "FCStd GUI geometry diagnostic",
        ));
    };
    if !ctx.xml_has_tag_name(root, expected_tag, "FCStd GUI value tag")? {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!("GUI property {property_name} requires a leading {expected_tag} value"),
            "FCStd GUI geometry diagnostic",
        ));
    }

    let mut descendants = property.descendants();
    let mut first_side_reference = None;
    let mut has_more_side_references = false;
    'references: while let Some(node) =
        ctx.next_charged(&mut descendants, "FCStd GUI geometry descendants")?
    {
        if !node.is_element() {
            continue;
        }
        let mut attributes = node.attributes();
        while let Some(attribute) =
            ctx.next_charged(&mut attributes, "FCStd GUI geometry reference attributes")?
        {
            if matches!(attribute.name(), "file" | "File") && !attribute.value().is_empty() {
                if first_side_reference.is_some() {
                    has_more_side_references = true;
                    break 'references;
                }
                first_side_reference = Some(node);
            }
        }
    }
    let direct_file = ctx
        .xml_attribute(root, "file", "FCStd GUI value attribute")?
        .is_some_and(|value| !value.is_empty());
    if first_side_reference.is_some() != direct_file
        || has_more_side_references
        || first_side_reference.is_some_and(|node| node != root)
    {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "GUI property {property_name} {expected_tag} has an unowned side-entry reference"
            ),
            "FCStd GUI geometry diagnostic",
        ));
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
    let Some(text) = ctx.xml_attribute(root, "mtrx", "FCStd GUI value attribute")? else {
        return Ok(());
    };
    let mut count = 0usize;
    let mut finite = true;
    for token in text.split_whitespace() {
        let value = ctx
            .parse_text::<f64>(token, "FCStd GUI Points transform scalar")?
            .map_err(|_| {
                crate::resource::malformed_charged(
                    ctx,
                    format_args!(
                        "GUI property {property_name} Points transform has an invalid scalar"
                    ),
                    "FCStd GUI Points transform diagnostic",
                )
            })?;
        finite &= value.is_finite();
        count += 1;
    }
    if count != 16 || !finite {
        return Err(crate::resource::malformed_charged(
            ctx,
            format_args!(
                "GUI property {property_name} Points transform must contain 16 finite scalars"
            ),
            "FCStd GUI Points transform diagnostic",
        ));
    }
    Ok(())
}

fn validate_gui_techdraw_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
    list_tag: &str,
    record_tag: &str,
    record_type: &str,
    mut validate: impl FnMut(
        &DecodeContext<'_>,
        roxmltree::Node<'_, '_>,
        &str,
    ) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let Some(root) =
        gui_single_element_child(ctx, property, "FCStd GUI TechDraw list root selection")?
    else {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            format_args!("requires exactly one {list_tag} value"),
        ));
    };
    if !ctx.xml_has_tag_name(root, list_tag, "FCStd GUI value tag")? {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            format_args!("requires a leading {list_tag} value"),
        ));
    }
    let count_text = ctx
        .xml_attribute(root, "count", "FCStd GUI value attribute")?
        .ok_or_else(|| {
            gui_techdraw_error(ctx, property_name, format_args!("{list_tag} has no count"))
        })?;
    let count = match ctx.parse_text::<usize>(count_text, "FCStd GUI TechDraw list count")? {
        Ok(count) => count,
        Err(_) => {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                format_args!("{list_tag} has an invalid count"),
            ));
        }
    };
    let (record_nodes, _record_node_storage) = ctx
        .with_scoped_storage("FCStd GUI TechDraw child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI TechDraw child nodes")
        })?;
    let record_count = ctx
        .admit_iter(&record_nodes, "FCStd GUI TechDraw record count")?
        .filter(|record| record.is_element())
        .count();
    if record_count != count {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            format_args!("{list_tag} count does not match its records"),
        ));
    }
    for record in ctx.admit_iter(&record_nodes, "FCStd GUI TechDraw records")? {
        if !record.is_element() {
            continue;
        }
        let record = *record;
        if !ctx.xml_has_tag_name(record, record_tag, "FCStd GUI value tag")? {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                format_args!("{list_tag} has an invalid record type"),
            ));
        }
        if ctx.xml_attribute(record, "type", "FCStd GUI value attribute")? != Some(record_type) {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                format_args!("{list_tag} has an invalid record type"),
            ));
        }
        validate(ctx, record, property_name)?;
    }
    Ok(())
}

fn gui_record_fields<'ctx, 'a, 'input>(
    ctx: &'ctx DecodeContext<'_>,
    record: roxmltree::Node<'a, 'input>,
    operation: &'static str,
) -> Result<
    (
        Vec<roxmltree::Node<'a, 'input>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage(operation, || {
        let mut fields = Vec::new();
        let mut children = record.children();
        while let Some(child) = ctx.next_charged(&mut children, operation)? {
            if child.is_element() {
                ctx.push_vec(&mut fields, child, operation)?;
            }
        }
        Ok(fields)
    })
}

fn validate_gui_geom_format_record(
    ctx: &DecodeContext<'_>,
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let (fields, _field_storage) = gui_record_fields(ctx, record, "FCStd GUI GeomFormat fields")?;
    if !(5..=6).contains(&fields.len()) {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "GeomFormat has an invalid field sequence",
        ));
    }
    for (field, expected_tag) in
        fields
            .iter()
            .zip(["GeomIndex", "Style", "Weight", "Color", "Visible"])
    {
        if !ctx.xml_has_tag_name(*field, expected_tag, "FCStd GUI value tag")?
            || has_nested_gui_elements(ctx, *field)?
        {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "GeomFormat has a nested or out-of-order field",
            ));
        }
        if ctx
            .xml_attribute(*field, "value", "FCStd GUI value attribute")?
            .is_none()
        {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "GeomFormat field has no value",
            ));
        }
    }
    if let Some(line_number) = fields.get(5) {
        if !(ctx.xml_has_tag_name(*line_number, "LineNumber", "FCStd GUI value tag")?
            || ctx.xml_has_tag_name(*line_number, "ISOLineNumber", "FCStd GUI value tag")?)
            || has_nested_gui_elements(ctx, *line_number)?
        {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "GeomFormat has an invalid line-number field",
            ));
        }
        parse_gui_techdraw_integer(ctx, *line_number, property_name)?;
    }
    parse_gui_techdraw_integer(ctx, fields[0], property_name)?;
    parse_gui_techdraw_integer(ctx, fields[1], property_name)?;
    let weight_text = ctx
        .xml_attribute(fields[2], "value", "FCStd GUI value attribute")?
        .ok_or_else(|| {
            gui_techdraw_error(ctx, property_name, "GeomFormat has an invalid weight")
        })?;
    let weight = match ctx.parse_text::<f64>(weight_text, "FCStd GUI GeomFormat weight")? {
        Ok(weight) => weight,
        Err(_) => {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "GeomFormat has an invalid weight",
            ));
        }
    };
    if !weight.is_finite() {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "GeomFormat has a non-finite weight",
        ));
    }
    let Some(color) = ctx.xml_attribute(fields[3], "value", "FCStd GUI value attribute")? else {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "GeomFormat has no color",
        ));
    };
    if !(color.len() == 7 || color.len() == 9)
        || !color.starts_with('#')
        || !color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "GeomFormat has an invalid color",
        ));
    }
    if parse_bool(
        ctx.xml_attribute(fields[4], "value", "FCStd GUI value attribute")?
            .unwrap_or_default(),
    )
    .is_none()
    {
        return Err(gui_techdraw_error(
            ctx,
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
    let (fields, _field_storage) = gui_record_fields(ctx, record, "FCStd GUI CenterLine fields")?;
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
            ctx,
            property_name,
            "CenterLine has an incomplete field sequence",
        ));
    }
    for (field, expected_tag) in fields[..prefix.len()].iter().zip(prefix) {
        if !ctx.xml_has_tag_name(*field, expected_tag, "FCStd GUI value tag")? {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "CenterLine has an out-of-order field",
            ));
        }
    }
    for index in [2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 15, 16] {
        if has_nested_gui_elements(ctx, fields[index])? {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "CenterLine has a nested field",
            ));
        }
    }
    validate_gui_techdraw_point(ctx, fields[0], property_name)?;
    validate_gui_techdraw_point(ctx, fields[1], property_name)?;
    let mode = parse_gui_techdraw_integer_value(ctx, fields[2], property_name, "Mode")?;
    if !(0..=2).contains(&mode) {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CenterLine has an unsupported Mode",
        ));
    }
    for field in fields[3..7].iter() {
        parse_gui_techdraw_finite(ctx, *field, property_name)?;
    }
    let line_type = parse_gui_techdraw_integer_value(ctx, fields[7], property_name, "Type")?;
    if !(0..=2).contains(&line_type) {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CenterLine has an unsupported Type",
        ));
    }
    validate_gui_techdraw_boolean(ctx, fields[8], property_name)?;
    validate_gui_center_line_string_collection(
        ctx,
        fields[9],
        property_name,
        "Faces",
        "FaceCount",
        "Face",
    )?;
    validate_gui_center_line_string_collection(
        ctx,
        fields[10],
        property_name,
        "Edges",
        "EdgeCount",
        "Edge",
    )?;
    validate_gui_center_line_string_collection(
        ctx,
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
    let geometry_type = TechDrawGeometryType::try_from(parse_gui_techdraw_integer_value(
        ctx,
        fields[16],
        property_name,
        "GeometryType",
    )?)
    .map_err(|()| {
        gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw geometry has an unsupported GeometryType",
        )
    })?;
    validate_gui_techdraw_geometry_branch(
        ctx,
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
    if !ctx.xml_has_tag_name(field, container_tag, "FCStd GUI value tag")? {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CenterLine has an invalid collection field",
        ));
    }
    let count_text = ctx
        .xml_attribute(field, count_attribute, "FCStd GUI value attribute")?
        .ok_or_else(|| {
            gui_techdraw_error(ctx, property_name, "CenterLine collection has no count")
        })?;
    let count =
        match ctx.parse_text::<usize>(count_text, "FCStd GUI CenterLine collection count")? {
            Ok(count) => count,
            Err(_) => {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "CenterLine collection has an invalid count",
                ));
            }
        };
    let (children, _children_storage) =
        ctx.with_scoped_storage("FCStd GUI CenterLine collection child nodes", || {
            ctx.collect_vec(
                field.children(),
                "FCStd GUI CenterLine collection child nodes",
            )
        })?;
    let item_count = ctx
        .admit_iter(&children, "FCStd GUI CenterLine collection item count")?
        .filter(|item| item.is_element())
        .count();
    if item_count != count {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CenterLine collection count does not match its records",
        ));
    }
    for item in ctx.admit_iter(&children, "FCStd GUI CenterLine collection items")? {
        if !item.is_element() {
            continue;
        }
        let item = *item;
        if !ctx.xml_has_tag_name(item, item_tag, "FCStd GUI value tag")?
            || ctx
                .xml_attribute(item, "value", "FCStd GUI value attribute")?
                .is_none()
            || has_nested_gui_elements(ctx, item)?
        {
            return Err(gui_techdraw_error(
                ctx,
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
    let (fields, _field_storage) = gui_record_fields(ctx, record, "FCStd GUI CosmeticEdge fields")?;
    if fields.len() < 16 {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CosmeticEdge has an incomplete field sequence",
        ));
    }
    let format_fields = ["Style", "Weight", "Color", "Visible", "GeometryType"];
    for (field, expected_tag) in fields.iter().take(format_fields.len()).zip(format_fields) {
        if !ctx.xml_has_tag_name(*field, expected_tag, "FCStd GUI value tag")?
            || has_nested_gui_elements(ctx, *field)?
        {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "CosmeticEdge has a nested or out-of-order format field",
            ));
        }
        if ctx
            .xml_attribute(*field, "value", "FCStd GUI value attribute")?
            .is_none()
        {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "CosmeticEdge format field has no value",
            ));
        }
    }
    parse_gui_techdraw_integer_named(ctx, fields[0], property_name, "Style")?;
    parse_gui_techdraw_finite(ctx, fields[1], property_name)?;
    validate_gui_techdraw_color(ctx, fields[2], property_name)?;
    validate_gui_techdraw_boolean(ctx, fields[3], property_name)?;
    let geometry_type_text = ctx
        .xml_attribute(fields[4], "value", "FCStd GUI value attribute")?
        .ok_or_else(|| {
            gui_techdraw_error(ctx, property_name, "CosmeticEdge GeometryType has no value")
        })?;
    let geometry_type =
        match ctx.parse_text::<i64>(geometry_type_text, "FCStd GUI TechDraw GeometryType")? {
            Ok(value) => TechDrawGeometryType::try_from(value),
            Err(_) => {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "CosmeticEdge GeometryType is not an integer",
                ));
            }
        }
        .map_err(|()| {
            gui_techdraw_error(
                ctx,
                property_name,
                "TechDraw geometry has an unsupported GeometryType",
            )
        })?;

    validate_gui_techdraw_geometry_branch(
        ctx,
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
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw geometry has no complete BaseGeom sequence",
        ));
    }
    validate_gui_techdraw_base_geom(
        ctx,
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
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw geometry has an invalid branch field sequence",
        ));
    }

    match expected_geometry_type {
        TechDrawGeometryType::Circle => {
            if !ctx.xml_has_tag_name(fields[cursor], "Center", "FCStd GUI value tag")? {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "TechDraw circle has an invalid center field",
                ));
            }
            validate_gui_techdraw_point(ctx, fields[cursor], property_name)?;
            cursor += 1;
            if !ctx.xml_has_tag_name(fields[cursor], "Radius", "FCStd GUI value tag")? {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "TechDraw circle has an invalid radius field",
                ));
            }
            parse_gui_techdraw_finite(ctx, fields[cursor], property_name)?;
            if has_nested_gui_elements(ctx, fields[cursor])? {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "TechDraw circle has a nested radius field",
                ));
            }
            cursor += 1;
        }
        TechDrawGeometryType::ArcOfCircle => {
            if !ctx.xml_has_tag_name(fields[cursor], "Center", "FCStd GUI value tag")? {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "TechDraw arc has an invalid center field",
                ));
            }
            validate_gui_techdraw_point(ctx, fields[cursor], property_name)?;
            cursor += 1;
            if !ctx.xml_has_tag_name(fields[cursor], "Radius", "FCStd GUI value tag")? {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "TechDraw arc has an invalid radius field",
                ));
            }
            parse_gui_techdraw_finite(ctx, fields[cursor], property_name)?;
            if has_nested_gui_elements(ctx, fields[cursor])? {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "TechDraw arc has a nested radius field",
                ));
            }
            cursor += 1;
            for expected_tag in ["Start", "End", "Middle"] {
                if !ctx.xml_has_tag_name(fields[cursor], expected_tag, "FCStd GUI value tag")? {
                    return Err(gui_techdraw_error(
                        ctx,
                        property_name,
                        "TechDraw arc has an out-of-order point field",
                    ));
                }
                validate_gui_techdraw_point(ctx, fields[cursor], property_name)?;
                cursor += 1;
            }
            for expected_tag in ["StartAngle", "EndAngle"] {
                if !ctx.xml_has_tag_name(fields[cursor], expected_tag, "FCStd GUI value tag")? {
                    return Err(gui_techdraw_error(
                        ctx,
                        property_name,
                        "TechDraw arc has an out-of-order angle field",
                    ));
                }
                parse_gui_techdraw_finite(ctx, fields[cursor], property_name)?;
                if has_nested_gui_elements(ctx, fields[cursor])? {
                    return Err(gui_techdraw_error(
                        ctx,
                        property_name,
                        "TechDraw arc has a nested angle field",
                    ));
                }
                cursor += 1;
            }
            for expected_tag in ["Clockwise", "Large"] {
                if !ctx.xml_has_tag_name(fields[cursor], expected_tag, "FCStd GUI value tag")? {
                    return Err(gui_techdraw_error(
                        ctx,
                        property_name,
                        "TechDraw arc has an out-of-order Boolean field",
                    ));
                }
                validate_gui_techdraw_boolean(ctx, fields[cursor], property_name)?;
                cursor += 1;
            }
        }
        TechDrawGeometryType::Generic => {
            if !ctx.xml_has_tag_name(fields[cursor], "Points", "FCStd GUI value tag")? {
                return Err(gui_techdraw_error(
                    ctx,
                    property_name,
                    "TechDraw generic geometry has no Points field",
                ));
            }
            validate_gui_techdraw_points(ctx, fields[cursor], property_name)?;
            cursor += 1;
        }
    }

    if cursor < fields.len() {
        let line_number =
            ctx.xml_has_tag_name(fields[cursor], "LineNumber", "FCStd GUI value tag")?
                || (allow_iso_line_number
                    && ctx.xml_has_tag_name(
                        fields[cursor],
                        "ISOLineNumber",
                        "FCStd GUI value tag",
                    )?);
        if cursor + 1 != fields.len() || !line_number {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "TechDraw geometry has an invalid trailing field",
            ));
        }
        parse_gui_techdraw_integer_named(ctx, fields[cursor], property_name, "LineNumber")?;
        if has_nested_gui_elements(ctx, fields[cursor])? {
            return Err(gui_techdraw_error(
                ctx,
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
    let geometry_type_value =
        gui_techdraw_base_geom_value(ctx, fields[0], expected[0], property_name)?;
    for (field, expected_tag) in fields.iter().zip(expected).skip(1) {
        gui_techdraw_base_geom_value(ctx, *field, expected_tag, property_name)?;
    }
    let geometry_type = ctx
        .parse_text::<i64>(geometry_type_value, "FCStd GUI TechDraw GeomType")?
        .map_err(|_| {
            gui_techdraw_error(ctx, property_name, "TechDraw GeomType is not an integer")
        })?;
    if geometry_type != expected_geometry_type.as_i64() {
        return Err(gui_techdraw_error(
            ctx,
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
    if !ctx.xml_has_tag_name(field, expected_tag, "FCStd GUI value tag")?
        || has_nested_gui_elements(ctx, field)?
    {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw BaseGeom has a nested or out-of-order field",
        ));
    }
    ctx.xml_attribute(field, "value", "FCStd GUI value attribute")?
        .ok_or_else(|| {
            gui_techdraw_error(ctx, property_name, "TechDraw BaseGeom field has no value")
        })
}

fn validate_gui_techdraw_points(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let count_text = ctx
        .xml_attribute(field, "PointsCount", "FCStd GUI value attribute")?
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw Points has no count"))?;
    let count = match ctx.parse_text::<usize>(count_text, "FCStd GUI TechDraw Points count")? {
        Ok(count) => count,
        Err(_) => {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "TechDraw Points has an invalid count",
            ));
        }
    };
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI TechDraw Points child nodes", || {
            ctx.collect_vec(field.children(), "FCStd GUI TechDraw Points child nodes")
        })?;
    let point_count = ctx
        .admit_iter(&children, "FCStd GUI TechDraw point count")?
        .filter(|point| point.is_element())
        .count();
    if point_count != count {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw Points count does not match its records",
        ));
    }
    for point in ctx.admit_iter(&children, "FCStd GUI TechDraw point records")? {
        if !point.is_element() {
            continue;
        }
        let point = *point;
        if !ctx.xml_has_tag_name(point, "Point", "FCStd GUI value tag")?
            || has_nested_gui_elements(ctx, point)?
        {
            return Err(gui_techdraw_error(
                ctx,
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
    let (fields, _field_storage) =
        gui_record_fields(ctx, record, "FCStd GUI CosmeticVertex fields")?;
    if !(15..=16).contains(&fields.len()) {
        return Err(gui_techdraw_error(
            ctx,
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
        if !ctx.xml_has_tag_name(*field, expected_tag, "FCStd GUI value tag")? {
            break;
        }
        if has_nested_gui_elements(ctx, *field)? {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "CosmeticVertex has a nested field",
            ));
        }
    }
    let mut base_fields_match = true;
    for (field, expected_tag) in fields.iter().zip(base_fields) {
        if !ctx.xml_has_tag_name(*field, expected_tag, "FCStd GUI value tag")? {
            base_fields_match = false;
            break;
        }
    }
    if !base_fields_match {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CosmeticVertex has an out-of-order field",
        ));
    }
    let mut cursor = base_fields.len();
    if ctx.xml_has_tag_name(fields[cursor], "VertexTag", "FCStd GUI value tag")? {
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
    let tail_fields_match = if fields.len() == cursor + tail_fields.len() {
        let mut matches = true;
        for (field, expected_tag) in fields[cursor..].iter().zip(tail_fields) {
            if !ctx.xml_has_tag_name(*field, expected_tag, "FCStd GUI value tag")? {
                matches = false;
                break;
            }
        }
        matches
    } else {
        false
    };
    if !tail_fields_match {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CosmeticVertex has an out-of-order field",
        ));
    }
    for field in fields.iter() {
        if has_nested_gui_elements(ctx, *field)? {
            return Err(gui_techdraw_error(
                ctx,
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
    if ctx
        .xml_attribute(fields[7], "value", "FCStd GUI value attribute")?
        .is_none()
    {
        return Err(gui_techdraw_error(
            ctx,
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
    if has_nested_gui_elements(ctx, field)? {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw point has a nested field",
        ));
    }
    for attribute in ["X", "Y", "Z"] {
        let text = ctx
            .xml_attribute(field, attribute, "FCStd GUI value attribute")?
            .ok_or_else(|| gui_techdraw_error(ctx, property_name, "point has no coordinate"))?;
        let value = ctx
            .parse_text::<f64>(text, "FCStd GUI TechDraw point coordinate")?
            .map_err(|_| {
                gui_techdraw_error(ctx, property_name, "point has an invalid coordinate")
            })?;
        if !value.is_finite() {
            return Err(gui_techdraw_error(
                ctx,
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
    let value = ctx
        .xml_attribute(field, "value", "FCStd GUI value attribute")?
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw Boolean has no value"))?;
    if parse_bool(value).is_none() {
        return Err(gui_techdraw_error(
            ctx,
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
    let color = ctx
        .xml_attribute(field, "value", "FCStd GUI value attribute")?
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw color has no value"))?;
    if !(color.len() == 7 || color.len() == 9)
        || !color.starts_with('#')
        || !color.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(gui_techdraw_error(
            ctx,
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
    let text = ctx
        .xml_attribute(field, "value", "FCStd GUI value attribute")?
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw scalar has no value"))?;
    let value = match ctx.parse_text::<f64>(text, "FCStd GUI TechDraw scalar")? {
        Ok(value) => value,
        Err(_) => {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "TechDraw scalar is invalid",
            ));
        }
    };
    if !value.is_finite() {
        return Err(gui_techdraw_error(
            ctx,
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
    let text = ctx
        .xml_attribute(field, "value", "FCStd GUI value attribute")?
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "TechDraw integer has no value"))?;
    match ctx.parse_text::<i64>(text, "FCStd GUI TechDraw integer")? {
        Ok(value) => Ok(value),
        Err(_) => Err(gui_techdraw_error(
            ctx,
            property_name,
            format_args!("TechDraw {field_name} integer is invalid"),
        )),
    }
}

fn validate_gui_techdraw_uuid(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
    field_name: &str,
) -> Result<(), CodecError> {
    let value = ctx
        .xml_attribute(field, "value", "FCStd GUI value attribute")?
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
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            format_args!("CosmeticVertex {field_name} is not a UUID"),
        ));
    }
    Ok(())
}

fn parse_gui_techdraw_integer(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let text = ctx
        .xml_attribute(field, "value", "FCStd GUI value attribute")?
        .ok_or_else(|| gui_techdraw_error(ctx, property_name, "GeomFormat field has no value"))?;
    match ctx.parse_text::<i64>(text, "FCStd GUI GeomFormat integer")? {
        Ok(_) => Ok(()),
        Err(_) => Err(gui_techdraw_error(
            ctx,
            property_name,
            "GeomFormat has an invalid integer",
        )),
    }
}

fn gui_techdraw_error(
    ctx: &DecodeContext<'_>,
    property_name: &str,
    detail: impl std::fmt::Display,
) -> CodecError {
    crate::resource::malformed_charged(
        ctx,
        format_args!("GUI property {property_name} {detail}"),
        "FCStd GUI TechDraw diagnostic",
    )
}

fn validate_visual_layer_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(root) =
        gui_single_element_child(ctx, property, "FCStd GUI VisualLayerList root selection")?
    else {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires exactly one VisualLayerList value"),
        ));
    };
    if !ctx.xml_has_tag_name(root, "VisualLayerList", "FCStd GUI value tag")? {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires a leading VisualLayerList value"),
        ));
    }
    let count_text = ctx
        .xml_attribute(root, "count", "FCStd GUI value attribute")?
        .ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("GUI property {property_name} VisualLayerList has no count"),
            )
        })?;
    let count = ctx
        .parse_text::<usize>(count_text, "FCStd GUI visual layer count")?
        .map_err(|_| {
            gui_malformed(
                ctx,
                format_args!("GUI property {property_name} VisualLayerList has an invalid count"),
            )
        })?;
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI VisualLayerList child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI VisualLayerList child nodes")
        })?;
    let element_count = ctx
        .admit_iter(&children, "FCStd GUI VisualLayerList element count")?
        .filter(|layer| layer.is_element())
        .count();
    if element_count != count
        || ctx.any_by(
            &children,
            |layer| {
                Ok(layer.is_element()
                    && !ctx.xml_has_tag_name(*layer, "VisualLayer", "FCStd GUI value tag")?)
            },
            "FCStd GUI VisualLayer tag validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!(
                "GUI property {property_name} VisualLayerList count or record tag is invalid"
            ),
        ));
    }
    for layer in ctx.admit_iter(&children, "FCStd GUI VisualLayer records")? {
        if !layer.is_element() {
            continue;
        }
        if !matches!(
            ctx.xml_attribute(*layer, "visible", "FCStd GUI value attribute")?,
            Some("true" | "false")
        ) {
            return Err(gui_malformed(
                ctx,
                format_args!(
                    "GUI property {property_name} VisualLayer has an invalid visible value"
                ),
            ));
        }
        let line_pattern = ctx
            .xml_attribute(*layer, "linePattern", "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} VisualLayer has no linePattern"),
                )
            })?;
        ctx.parse_text::<u32>(line_pattern, "FCStd GUI visual layer pattern")?
            .map_err(|_| {
                gui_malformed(
                    ctx,
                    format_args!(
                        "GUI property {property_name} VisualLayer has an invalid linePattern"
                    ),
                )
            })?;
        let line_width_text = ctx
            .xml_attribute(*layer, "lineWidth", "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} VisualLayer has no lineWidth"),
                )
            })?;
        let line_width = ctx
            .parse_text::<f64>(line_width_text, "FCStd GUI visual layer width")?
            .map_err(|_| {
                gui_malformed(
                    ctx,
                    format_args!(
                        "GUI property {property_name} VisualLayer has an invalid lineWidth"
                    ),
                )
            })?;
        if !line_width.is_finite() {
            return Err(gui_malformed(
                ctx,
                format_args!("GUI property {property_name} VisualLayer has a non-finite lineWidth"),
            ));
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
        let text = ctx
            .xml_attribute(value, attribute, "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} material has no {attribute}"),
                )
            })?;
        ctx.parse_text::<u32>(text, "FCStd GUI material color")?
            .map_err(|_| {
                gui_malformed(
                    ctx,
                    format_args!(
                        "GUI property {property_name} material has an invalid {attribute}"
                    ),
                )
            })?;
    }
    for attribute in ["shininess", "transparency"] {
        let text = ctx
            .xml_attribute(value, attribute, "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} material has no {attribute}"),
                )
            })?;
        let scalar = ctx
            .parse_text::<f64>(text, "FCStd GUI material scalar")?
            .map_err(|_| {
                gui_malformed(
                    ctx,
                    format_args!(
                        "GUI property {property_name} material has an invalid {attribute}"
                    ),
                )
            })?;
        if !scalar.is_finite() {
            return Err(gui_malformed(
                ctx,
                format_args!("GUI property {property_name} material has a non-finite {attribute}"),
            ));
        }
    }
    Ok(())
}

fn validate_gui_expression_engine(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(root) =
        gui_single_element_child(ctx, property, "FCStd GUI ExpressionEngine root selection")?
    else {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires one ExpressionEngine value"),
        ));
    };
    if !ctx.xml_has_tag_name(root, "ExpressionEngine", "FCStd GUI value tag")? {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires a leading ExpressionEngine value"),
        ));
    }
    let count = gui_list_count(ctx, root, property_name, "ExpressionEngine")?;
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI ExpressionEngine child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI ExpressionEngine child nodes")
        })?;
    let mut expression_count = 0_usize;
    for child in ctx.admit_iter(&children, "FCStd GUI expression count")? {
        if ctx.xml_has_tag_name(*child, "Expression", "FCStd GUI value tag")? {
            expression_count = expression_count
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("GUI expression count overflows"))?;
        }
    }
    if expression_count != count
        || ctx.any_by(
            &children,
            |expression| {
                if !ctx.xml_has_tag_name(*expression, "Expression", "FCStd GUI value tag")? {
                    return Ok(false);
                }
                Ok(ctx
                    .xml_attribute(*expression, "path", "FCStd GUI value attribute")?
                    .is_none()
                    || ctx
                        .xml_attribute(*expression, "expression", "FCStd GUI value attribute")?
                        .is_none())
            },
            "FCStd GUI expression attribute validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!(
                "GUI property {property_name} ExpressionEngine count or expression is invalid"
            ),
        ));
    }
    Ok(())
}

fn validate_gui_material_reference(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(root) =
        gui_single_element_child(ctx, property, "FCStd GUI material reference root selection")?
    else {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires one PropertyMaterial value"),
        ));
    };
    if !ctx.xml_has_tag_name(root, "PropertyMaterial", "FCStd GUI value tag")?
        || ctx
            .xml_attribute(root, "uuid", "FCStd GUI value attribute")?
            .is_none()
    {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} material reference is invalid"),
        ));
    }
    Ok(())
}

fn validate_gui_part_shape(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let mut children = property.children();
    let first = ctx.find_by(
        &mut children,
        |child| Ok(child.is_element()),
        "FCStd GUI Part shape first element search",
    )?;
    let first_is_part = match first {
        Some(node) => ctx.xml_has_tag_name(node, "Part", "FCStd GUI Part shape tag")?,
        None => true,
    };
    if first_is_part
        && ctx.all_by(
            children,
            |child| {
                Ok(!child.is_element()
                    || ctx.xml_has_tag_name(child, "ElementMap", "FCStd GUI Part shape tag")?)
            },
            "FCStd GUI Part shape remaining element validation",
        )?
    {
        return Ok(());
    }
    Err(gui_malformed(
        ctx,
        format_args!("GUI property {property_name} Part shape value is invalid"),
    ))
}

fn validate_gui_geometry_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(root) =
        gui_single_element_child(ctx, property, "FCStd GUI GeometryList root selection")?
    else {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires one GeometryList value"),
        ));
    };
    if !ctx.xml_has_tag_name(root, "GeometryList", "FCStd GUI value tag")? {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires a leading GeometryList value"),
        ));
    }
    let count = gui_list_count(ctx, root, property_name, "GeometryList")?;
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI GeometryList child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI GeometryList child nodes")
        })?;
    let element_count = ctx
        .admit_iter(&children, "FCStd GUI GeometryList element count")?
        .filter(|geometry| geometry.is_element())
        .count();
    if element_count != count
        || ctx.any_by(
            &children,
            |geometry| {
                Ok(geometry.is_element()
                    && !ctx.xml_has_tag_name(*geometry, "Geometry", "FCStd GUI value tag")?)
            },
            "FCStd GUI GeometryList tag validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!(
                "GUI property {property_name} GeometryList count or record tag is invalid"
            ),
        ));
    }
    Ok(())
}

fn validate_gui_filletedges(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(root) =
        gui_single_element_child(ctx, property, "FCStd GUI FilletEdges root selection")?
    else {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires one FilletEdges value"),
        ));
    };
    if !ctx.xml_has_tag_name(root, "FilletEdges", "FCStd GUI value tag")?
        || ctx
            .xml_attribute(root, "file", "FCStd GUI value attribute")?
            .is_none()
    {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} FilletEdges value is invalid"),
        ));
    }
    Ok(())
}

fn validate_gui_shape_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(root) = gui_single_element_child(ctx, property, "FCStd GUI ShapeList root selection")?
    else {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires one ShapeList value"),
        ));
    };
    if !ctx.xml_has_tag_name(root, "ShapeList", "FCStd GUI value tag")? {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires a leading ShapeList value"),
        ));
    }
    let count = gui_list_count(ctx, root, property_name, "ShapeList")?;
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI ShapeList child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI ShapeList child nodes")
        })?;
    let element_count = ctx
        .admit_iter(&children, "FCStd GUI ShapeList element count")?
        .filter(|shape| shape.is_element())
        .count();
    if element_count != count
        || ctx.any_by(
            &children,
            |shape| {
                if !shape.is_element() {
                    return Ok(false);
                }
                if !ctx.xml_has_tag_name(*shape, "TopoShape", "FCStd GUI value tag")? {
                    return Ok(true);
                }
                Ok(ctx
                    .xml_attribute(*shape, "file", "FCStd GUI value attribute")?
                    .is_none()
                    && ctx
                        .xml_attribute(*shape, "binary", "FCStd GUI value attribute")?
                        .is_none()
                    && ctx
                        .xml_attribute(*shape, "brep", "FCStd GUI value attribute")?
                        .is_none())
            },
            "FCStd GUI ShapeList record validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} ShapeList count or record is invalid"),
        ));
    }
    Ok(())
}

fn validate_gui_constraint_list(
    ctx: &DecodeContext<'_>,
    property: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let Some(root) =
        gui_single_element_child(ctx, property, "FCStd GUI ConstraintList root selection")?
    else {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires one ConstraintList value"),
        ));
    };
    if !ctx.xml_has_tag_name(root, "ConstraintList", "FCStd GUI value tag")? {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI property {property_name} requires a leading ConstraintList value"),
        ));
    }
    let count = gui_list_count(ctx, root, property_name, "ConstraintList")?;
    let (children, _children_storage) = ctx
        .with_scoped_storage("FCStd GUI ConstraintList child nodes", || {
            ctx.collect_vec(root.children(), "FCStd GUI ConstraintList child nodes")
        })?;
    let element_count = ctx
        .admit_iter(&children, "FCStd GUI ConstraintList element count")?
        .filter(|constraint| constraint.is_element())
        .count();
    if element_count != count
        || ctx.any_by(
            &children,
            |constraint| {
                Ok(constraint.is_element()
                    && !ctx.xml_has_tag_name(*constraint, "Constrain", "FCStd GUI value tag")?)
            },
            "FCStd GUI ConstraintList tag validation",
        )?
    {
        return Err(gui_malformed(
            ctx,
            format_args!(
                "GUI property {property_name} ConstraintList count or record tag is invalid"
            ),
        ));
    }
    Ok(())
}

struct GuiMaterial<'data> {
    ambient: u32,
    diffuse: u32,
    specular: u32,
    emissive: u32,
    shininess: FiniteBinary32,
    transparency: FiniteBinary32,
    uuid: &'data str,
}

fn validate_gui_list_payloads<'property, 'data>(
    ctx: &DecodeContext<'_>,
    properties: &'property [GuiPropertyRecord],
    entries: &BTreeMap<String, View<'data>>,
    requires_alpha_conversion: bool,
) -> Result<HashMap<&'property str, Vec<GuiMaterial<'data>>>, CodecError> {
    let mut material_lists = HashMap::new();
    for property in ctx.admit_iter(properties, "FCStd GUI list property sources")? {
        if property.side_entries.is_empty() {
            continue;
        }
        if property.type_name == "Part::PropertyTopoShapeList" {
            for entry_name in ctx.admit_iter(
                &property.side_entries,
                "FCStd GUI shape-list entry references",
            )? {
                ctx.get_btree_map(entries, entry_name, "FCStd GUI side entry lookup")?
                    .ok_or_else(|| {
                        gui_malformed(
                            ctx,
                            format_args!(
                                "GUI property {} references missing side entry {entry_name}",
                                property.id
                            ),
                        )
                    })?;
            }
            continue;
        }
        let entry_name = property.side_entries.first().ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("GUI property {} has no side entry", property.id),
            )
        })?;
        if property.side_entries.len() != 1 {
            return Err(gui_malformed(
                ctx,
                format_args!(
                    "GUI property {} references more than one side entry",
                    property.id
                ),
            ));
        }
        let view = *ctx
            .get_btree_map(entries, entry_name, "FCStd GUI side entry lookup")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!(
                        "GUI property {} references missing side entry {entry_name}",
                        property.id
                    ),
                )
            })?;
        match property.type_name.as_str() {
            "App::PropertyColorList" => {
                let (_colors, _color_storage) = ctx
                    .with_scoped_storage("FCStd GUI color-list validation storage", || {
                        parse_color_list(ctx, view, entry_name, requires_alpha_conversion)
                    })?;
            }
            "App::PropertyFloatList" => {
                parse_float_list(ctx, view, entry_name)?;
            }
            "Part::PropertyFilletEdges" => {
                parse_fillet_edges(ctx, view, entry_name)?;
            }
            "App::PropertyMaterialList" => {
                let value = ctx.find_by(
                    &property.values,
                    |value| Ok(value.tag == "MaterialList"),
                    "FCStd GUI material-list value search",
                )?;
                let version_text = match value {
                    Some(value) => ctx.get_btree_map(
                        &value.attributes,
                        "version",
                        "FCStd GUI material-list version attribute",
                    )?,
                    None => None,
                };
                let version = version_text
                    .map(|value| {
                        ctx.parse_text::<u32>(value, "FCStd GUI material-list version")?
                            .map_err(|_| {
                                gui_malformed(
                                    ctx,
                                    format_args!(
                                        "GUI material list {} has an invalid version",
                                        property.id
                                    ),
                                )
                            })
                    })
                    .transpose()?
                    .unwrap_or(0);
                ctx.insert_hash_map(
                    &mut material_lists,
                    property.id.as_str(),
                    parse_material_list(
                        ctx,
                        view,
                        version,
                        &property.id,
                        requires_alpha_conversion,
                    )?,
                    "FCStd GUI material lists",
                )?;
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
    let count = view
        .counted(count.into(), 4)
        .ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("color-list entry {entry_name} count exceeds its payload"),
            )
        })?
        .get();
    let mut colors = ctx.collection_vec(count, "FCStd GUI color-list entries")?;
    for _ in ctx.admit_iter(0..count, "FCStd GUI color-list read")? {
        colors.push(convert_packed_alpha(
            view.req_u32_le()?,
            requires_alpha_conversion,
        ));
    }
    if !view.is_empty() {
        return Err(gui_malformed(
            ctx,
            format_args!("color-list entry {entry_name} has trailing bytes"),
        ));
    }
    Ok(colors)
}

fn read_gui_counted<'a, T>(
    ctx: &DecodeContext<'_>,
    view: &mut View<'a>,
    count: u32,
    element_size: usize,
    mut read: impl FnMut(&mut View<'a>) -> Option<T>,
    operation: &'static str,
) -> Result<Option<Vec<T>>, CodecError> {
    let Some(count) = view.counted(count.into(), element_size) else {
        return Ok(None);
    };
    let mut values = ctx.collection_vec(count.get(), operation)?;
    for _ in ctx.admit_iter(0..count.get(), operation)? {
        let Some(value) = read(view) else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok(Some(values))
}

fn parse_float_list(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    entry_name: &str,
) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let (values, _value_storage) =
        ctx.with_scoped_storage("FCStd GUI list validation storage", || {
            read_gui_counted(
                ctx,
                &mut view,
                count,
                8,
                View::f64_le,
                "FCStd GUI float-list entries",
            )
        })?;
    let values = values.ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("float-list entry {entry_name} count exceeds its payload"),
        )
    })?;
    if ctx.any_by(
        &values,
        |value| Ok(!value.is_finite()),
        "FCStd GUI float-list scalar validation",
    )? {
        return Err(gui_malformed(
            ctx,
            format_args!("float-list entry {entry_name} has a non-finite value"),
        ));
    }
    if !view.is_empty() {
        return Err(gui_malformed(
            ctx,
            format_args!("float-list entry {entry_name} has trailing bytes"),
        ));
    }
    Ok(())
}

fn parse_vector_list(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    entry_name: &str,
) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let (values, _value_storage) =
        ctx.with_scoped_storage("FCStd GUI list validation storage", || {
            read_gui_counted(
                ctx,
                &mut view,
                count,
                24,
                |view| Some((view.f64_le()?, view.f64_le()?, view.f64_le()?)),
                "FCStd GUI vector-list entries",
            )
        })?;
    let values = values.ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("vector-list entry {entry_name} count exceeds its payload"),
        )
    })?;
    if ctx.any_by(
        &values,
        |value| Ok(!value.0.is_finite() || !value.1.is_finite() || !value.2.is_finite()),
        "FCStd GUI vector-list scalar validation",
    )? {
        return Err(gui_malformed(
            ctx,
            format_args!("vector-list entry {entry_name} has a non-finite value"),
        ));
    }
    if !view.is_empty() {
        return Err(gui_malformed(
            ctx,
            format_args!("vector-list entry {entry_name} has trailing bytes"),
        ));
    }
    Ok(())
}

fn parse_placement_list(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    entry_name: &str,
) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let (values, _value_storage) =
        ctx.with_scoped_storage("FCStd GUI list validation storage", || {
            read_gui_counted(
                ctx,
                &mut view,
                count,
                56,
                |view| {
                    Some([
                        view.f64_le()?,
                        view.f64_le()?,
                        view.f64_le()?,
                        view.f64_le()?,
                        view.f64_le()?,
                        view.f64_le()?,
                        view.f64_le()?,
                    ])
                },
                "FCStd GUI placement-list entries",
            )
        })?;
    let values = values.ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("placement-list entry {entry_name} count exceeds its payload"),
        )
    })?;
    if ctx.any_by(
        &values,
        |value| Ok(value.iter().any(|scalar| !scalar.is_finite())),
        "FCStd GUI placement-list scalar validation",
    )? {
        return Err(gui_malformed(
            ctx,
            format_args!("placement-list entry {entry_name} has a non-finite value"),
        ));
    }
    if !view.is_empty() {
        return Err(gui_malformed(
            ctx,
            format_args!("placement-list entry {entry_name} has trailing bytes"),
        ));
    }
    Ok(())
}

fn parse_fillet_edges(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    entry_name: &str,
) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let (values, _value_storage) =
        ctx.with_scoped_storage("FCStd GUI list validation storage", || {
            read_gui_counted(
                ctx,
                &mut view,
                count,
                20,
                |view| Some((view.i32_le()?, view.f64_le()?, view.f64_le()?)),
                "FCStd GUI fillet-edge entries",
            )
        })?;
    let values = values.ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("fillet-edges entry {entry_name} count exceeds its payload"),
        )
    })?;
    if ctx.any_by(
        &values,
        |(_, radius1, radius2)| Ok(!radius1.is_finite() || !radius2.is_finite()),
        "FCStd GUI fillet-edge scalar validation",
    )? {
        return Err(gui_malformed(
            ctx,
            format_args!("fillet-edges entry {entry_name} has a non-finite radius"),
        ));
    }
    if !view.is_empty() {
        return Err(gui_malformed(
            ctx,
            format_args!("fillet-edges entry {entry_name} has trailing bytes"),
        ));
    }
    Ok(())
}

fn parse_material_list<'data>(
    ctx: &DecodeContext<'_>,
    mut view: View<'data>,
    version: u32,
    property_id: &str,
    requires_alpha_conversion: bool,
) -> Result<Vec<GuiMaterial<'data>>, CodecError> {
    let (count, has_strings) = match version {
        0 | 1 => {
            let header = view.i32_le().ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI material list {property_id} is truncated"),
                )
            })?;
            let count = if header < 0 {
                view.u32_le().ok_or_else(|| {
                    gui_malformed(
                        ctx,
                        format_args!("GUI material list {property_id} is truncated"),
                    )
                })?
            } else {
                u32::try_from(header).map_err(|_| {
                    gui_malformed(
                        ctx,
                        format_args!("GUI material list {property_id} has a negative count"),
                    )
                })?
            };
            (count, false)
        }
        2 => (
            view.u32_le().ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI material list {property_id} is truncated"),
                )
            })?,
            false,
        ),
        3 => (
            view.u32_le().ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI material list {property_id} is truncated"),
                )
            })?,
            true,
        ),
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "FCStd GUI material-list version {version}"
            )));
        }
    };
    let (raw_materials, _raw_storage) =
        ctx.with_scoped_storage("FCStd GUI raw material storage", || {
            read_gui_counted(
                ctx,
                &mut view,
                count,
                24,
                |view| {
                    Some((
                        [
                            view.u32_le()?,
                            view.u32_le()?,
                            view.u32_le()?,
                            view.u32_le()?,
                        ],
                        [view.f32_le()?, view.f32_le()?],
                    ))
                },
                "FCStd GUI raw material entries",
            )
        })?;
    let raw_materials = raw_materials.ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("GUI material list {property_id} count exceeds its payload"),
        )
    })?;
    let mut materials = ctx.collection_vec(raw_materials.len(), "FCStd GUI material entries")?;
    for ([ambient, diffuse, specular, emissive], [shininess, transparency]) in
        ctx.admit_iter(raw_materials, "FCStd GUI material conversion")?
    {
        let invalid = || {
            gui_malformed(
                ctx,
                format_args!("GUI material list {property_id} has non-finite scalars"),
            )
        };
        materials.push(GuiMaterial {
            ambient,
            diffuse,
            specular,
            emissive,
            shininess: FiniteBinary32::new(shininess).ok_or_else(invalid)?,
            transparency: FiniteBinary32::new(transparency).ok_or_else(invalid)?,
            uuid: "",
        });
    }
    if requires_alpha_conversion {
        for material in ctx.admit_iter(&mut materials, "FCStd GUI material records")? {
            material.ambient = convert_packed_alpha(material.ambient, true);
            material.diffuse = convert_packed_alpha(material.diffuse, true);
            material.specular = convert_packed_alpha(material.specular, true);
            material.emissive = convert_packed_alpha(material.emissive, true);
        }
    }
    if has_strings {
        for material in ctx.admit_iter(&mut materials, "FCStd GUI material records")? {
            read_material_string(ctx, &mut view, property_id)?;
            read_material_string(ctx, &mut view, property_id)?;
            material.uuid = read_material_string(ctx, &mut view, property_id)?;
        }
    }
    if !view.is_empty() {
        return Err(gui_malformed(
            ctx,
            format_args!("GUI material list {property_id} has trailing bytes"),
        ));
    }
    Ok(materials)
}

fn read_material_string<'data>(
    ctx: &DecodeContext<'_>,
    view: &mut View<'data>,
    property_id: &str,
) -> Result<&'data str, CodecError> {
    let length = view.u32_le().ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("GUI material list {property_id} string is truncated"),
        )
    })?;
    let length = view
        .counted(length.into(), 1)
        .ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("GUI material list {property_id} string exceeds its payload"),
            )
        })?
        .get();
    let bytes = view.take(length).ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("GUI material list {property_id} string exceeds its payload"),
        )
    })?;
    let text = ctx
        .validate_utf8(bytes, "FCStd GUI material string UTF-8")?
        .map_err(|_| {
            gui_malformed(
                ctx,
                format_args!("GUI material list {property_id} string is not UTF-8"),
            )
        })?;
    Ok(text)
}

fn transfer_shape_appearances(
    ctx: &DecodeContext<'_>,
    plan: &mut AppearancePlan<'_>,
    graph: &Graph,
    material_lists: &HashMap<&str, Vec<GuiMaterial<'_>>>,
    shape_index: &ShapeIndex<'_, '_>,
    topology_index: &TopologyIndex<'_, '_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if material_lists.is_empty() {
        return Ok(());
    }
    let (binding_counts, _binding_count_storage) =
        ctx.with_scoped_storage("FCStd GUI legacy binding counts", || {
            let mut counts = BTreeMap::<String, usize>::new();
            for binding in
                ctx.admit_iter(&plan.bindings, "FCStd GUI legacy binding count sources")?
            {
                if let Some(count) = ctx.get_mut_btree_map(
                    &mut counts,
                    binding.appearance.as_str(),
                    "FCStd GUI legacy binding count lookup",
                )? {
                    *count += 1;
                } else {
                    let key = ctx.copy_retained_text(
                        binding.appearance.as_str(),
                        "FCStd GUI legacy binding count identity",
                    )?;
                    ctx.insert_btree_map(&mut counts, key, 1, "FCStd GUI legacy binding counts")?;
                }
            }
            Ok::<_, CodecError>(counts)
        })?;
    let (appearance_properties, _appearance_property_storage) = ctx.collect_scoped_btree_map(
        ctx.admit_iter(&graph.properties, "FCStd GUI ShapeAppearance selection")?
            .rev()
            .filter(|property| {
                property.name == "ShapeAppearance"
                    && property.type_name == "App::PropertyMaterialList"
            })
            .map(|property| (property.owner.as_str(), property)),
        "FCStd GUI ShapeAppearance owners",
    )?;
    for provider in ctx.admit_iter(&graph.providers, "FCStd GUI material providers")? {
        let Some(object_id) = provider
            .object
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str)
        else {
            continue;
        };
        let Some(property) = ctx.get_btree_map(
            &appearance_properties,
            provider.id.as_str(),
            "FCStd GUI ShapeAppearance lookup",
        )?
        else {
            continue;
        };
        let Some(materials) = ctx.get_hash_map(
            material_lists,
            property.id.as_str(),
            "FCStd GUI material list lookup",
        )?
        else {
            continue;
        };
        let (provider_key, _key_storage) = ctx
            .with_scoped_storage("FCStd GUI provider key storage", || {
                provider_identity_key(ctx, &provider.name)
            })?;
        let (body_ids, _body_storage) = ctx
            .with_scoped_storage("FCStd GUI displayed bodies", || {
                displayed_shape_bodies(ctx, topology_index, object_id, shape_index)
            })?;
        let group = displayed_shape_group(ctx, object_id, shape_index, "Face")?;
        let mapped_count = match group {
            None => 0,
            Some(group) => match group.names.len() {
                0 => 0,
                length => length - 1,
            },
        };
        if materials.len() == 1 {
            let legacy_id = plan
                .storage
                .with_storage(|| object_appearance_id(ctx, &provider_key))?;
            let removed_count = ctx
                .get_btree_map(
                    &binding_counts,
                    legacy_id.as_str(),
                    "FCStd GUI removed binding count lookup",
                )?
                .copied()
                .unwrap_or(0);
            if plan.storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut plan.remove_appearances,
                    legacy_id,
                    "FCStd GUI removed appearances",
                )
            })? {
                plan.removed_binding_count += removed_count;
            }
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
                    .with_tag(ctx.copy_retained_text(&property.id, "FCStd GUI material loss tag")?),
                    "FCStd GUI material count losses", "FCStd GUI material count loss text")?;
                continue;
            }
        }
        for (index, material) in ctx
            .admit_iter(materials, "FCStd GUI shape material sources")?
            .enumerate()
        {
            let (appearance_id, _id_storage) = ctx
                .with_scoped_storage("FCStd GUI appearance identity storage", || {
                    shape_material_appearance_id(ctx, &provider_key, index)
                })?;
            plan.storage.with_storage(|| {
                ctx.reserve_vec(&mut plan.appearances, 1, "FCStd GUI planned appearances")
            })?;
            plan.appearances.push(material_appearance(
                ctx,
                appearance_id.try_clone_for_decode(ctx, "FCStd GUI appearance identity copy")?,
                &provider.name,
                index,
                material,
            )?);
            if materials.len() == 1 {
                for (body_index, body) in ctx
                    .admit_iter(&body_ids, "FCStd GUI material body bindings")?
                    .enumerate()
                {
                    push_body_update(
                        ctx,
                        plan,
                        body,
                        Assignment::Keep,
                        decode_color(material.diffuse, Some(material.transparency.get())).map(Some),
                    )?;
                    plan.storage.with_storage(|| {
                        ctx.reserve_vec(&mut plan.bindings, 1, "FCStd GUI planned bindings")
                    })?;
                    plan.bindings.push(AppearanceBinding {
                        id: binding_id(
                            ctx,
                            format_args!(
                            "fcstd:appearance:binding#shape-material:{provider_key}:{body_index}"
                        ),
                        )?,
                        target: AppearanceTarget::Body(
                            body.try_clone_for_decode(ctx, "FCStd GUI binding body identity")?,
                        ),
                        appearance: appearance_id
                            .try_clone_for_decode(ctx, "FCStd GUI binding appearance identity")?,
                        source_entity_id: Some(
                            ctx.copy_retained_text(object_id, "FCStd GUI binding source identity")?,
                        ),
                        object_type: Some(ctx.copy_retained_text(
                            "ViewProvider ShapeAppearance",
                            "FCStd GUI appearance literal text",
                        )?),
                        visible: None,
                        channels: BTreeMap::new(),
                    });
                }
            } else if let Some(group) = group {
                bind_material_faces(
                    ctx,
                    topology_index,
                    plan,
                    group,
                    index,
                    &appearance_id,
                    (&provider_key, object_id),
                )?;
            }
        }
    }
    Ok(())
}

fn displayed_shape_bodies(
    ctx: &DecodeContext<'_>,
    topology_index: &TopologyIndex<'_, '_>,
    object_id: &str,
    shape_index: &ShapeIndex<'_, '_>,
) -> Result<Vec<cadmpeg_ir::ids::BodyId>, CodecError> {
    let Some(payload) = displayed_shape_payload(ctx, object_id, shape_index)? else {
        return Ok(Vec::new());
    };
    select_shape_bodies(ctx, topology_index, std::iter::once(payload.id.as_str()))
}

fn select_shape_bodies<'a>(
    ctx: &DecodeContext<'_>,
    topology_index: &TopologyIndex<'_, '_>,
    payload_ids: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<cadmpeg_ir::ids::BodyId>, CodecError> {
    let mut body_ids = Vec::new();
    let mut payload_ids = payload_ids.into_iter();
    while let Some(payload_id) =
        ctx.next_charged(&mut payload_ids, "FCStd GUI displayed payload identities")?
    {
        let payload_key = ctx
            .split_once(payload_id, "#", "FCStd GUI identity key")?
            .map_or(payload_id, |(_, key)| key);
        let owned = ctx
            .get_btree_map(
                &topology_index.bodies,
                payload_key,
                "FCStd GUI displayed body payload lookup",
            )?
            .map(Vec::as_slice)
            .unwrap_or_default();
        for body in ctx.admit_iter(owned, "FCStd GUI displayed body candidates")? {
            ctx.push_vec(
                &mut body_ids,
                body.try_clone_for_decode(ctx, "FCStd GUI displayed body identity")?,
                "FCStd GUI displayed shape bodies",
            )?;
        }
    }
    Ok(body_ids)
}

fn displayed_shape_payload<'a>(
    ctx: &DecodeContext<'_>,
    object_id: &str,
    shape_index: &ShapeIndex<'a, '_>,
) -> Result<Option<&'a ShapePayloadRecord>, CodecError> {
    let owned = ctx
        .get_btree_map(
            &shape_index.properties,
            object_id,
            "FCStd GUI displayed Shape lookup",
        )?
        .map(Vec::as_slice)
        .unwrap_or_default();
    let Some(&property) = owned.first() else {
        return Ok(None);
    };
    if owned.len() > 1 {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("object {object_id} has multiple Shape properties"),
            "FCStd GUI duplicate shape property diagnostic",
        )?));
    }
    let owned = ctx
        .get_btree_map(
            &shape_index.payloads,
            property.id.as_str(),
            "FCStd GUI displayed payload lookup",
        )?
        .map(Vec::as_slice)
        .unwrap_or_default();
    let Some(&payload) = owned.first() else {
        return Ok(None);
    };
    if owned.len() > 1 {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!("Shape property {} has multiple payloads", property.id),
            "FCStd GUI duplicate shape payload diagnostic",
        )?));
    }
    Ok(Some(payload))
}

fn displayed_shape_group<'a>(
    ctx: &DecodeContext<'_>,
    object_id: &str,
    shape_index: &ShapeIndex<'a, '_>,
    indexed_name: &str,
) -> Result<Option<&'a ElementMapGroup>, CodecError> {
    let Some(payload) = displayed_shape_payload(ctx, object_id, shape_index)? else {
        return Ok(None);
    };
    let owned = ctx
        .get_btree_map(
            &shape_index.maps,
            payload.property.as_str(),
            "FCStd GUI displayed element map lookup",
        )?
        .map(Vec::as_slice)
        .unwrap_or_default();
    let Some(&map) = owned.first() else {
        return Ok(None);
    };
    if owned.len() > 1 {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "Shape property {} has multiple element maps",
                payload.property
            ),
            "FCStd GUI duplicate element map diagnostic",
        )?));
    }
    let root = map.maps.root();
    let mut groups = root.groups.iter();
    let Some(group) = ctx.find_by(
        &mut groups,
        |group| {
            ctx.equal(
                group.indexed_name.as_str(),
                indexed_name,
                "FCStd GUI element group name",
            )
        },
        "FCStd GUI element group search",
    )?
    else {
        return Ok(None);
    };
    if ctx
        .find_by(
            &mut groups,
            |group| {
                ctx.equal(
                    group.indexed_name.as_str(),
                    indexed_name,
                    "FCStd GUI element group name",
                )
            },
            "FCStd GUI duplicate element group search",
        )?
        .is_some()
    {
        return Err(CodecError::Malformed(ctx.format_retained(
            format_args!(
                "Shape property {} has multiple {indexed_name} groups",
                payload.property
            ),
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
    material: &GuiMaterial<'_>,
) -> Result<Appearance, CodecError> {
    let scalar = |name: &str, value: f64| {
        cadmpeg_ir::scalar::FiniteReal::new(value)
            .ok_or_else(|| gui_malformed(ctx, format_args!("GUI material {name} is non-finite")))
    };
    Ok(Appearance {
        id,
        name: Some(ctx.format_retained(
            format_args!("{provider_name} face {} material", index + 1),
            "FCStd GUI appearance name",
        )?),
        asset_guid: (!material.uuid.is_empty())
            .then(|| ctx.copy_retained_text(material.uuid, "FCStd GUI material asset GUID"))
            .transpose()?,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some(
            ctx.copy_retained_text("FCStd ShapeAppearance", "FCStd GUI appearance literal text")?,
        ),
        category: None,
        base_color: Some(decode_color(
            material.diffuse,
            Some(material.transparency.get()),
        )?),
        textures: Vec::new(),
        properties: {
            let mut properties = BTreeMap::new();
            for (key, value) in [
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
            ] {
                ctx.insert_btree_map(
                    &mut properties,
                    key,
                    value,
                    "FCStd GUI material appearance properties",
                )?;
            }
            properties
        },
    })
}

fn bind_material_faces(
    ctx: &DecodeContext<'_>,
    topology_index: &TopologyIndex<'_, '_>,
    plan: &mut AppearancePlan<'_>,
    group: &ElementMapGroup,
    material_index: usize,
    appearance_id: &AppearanceId,
    provider: (&IdentityKey, &str),
) -> Result<(), CodecError> {
    let (provider_key, object_id) = provider;
    let mut bound_storage = ctx.reserve_scoped(0, "FCStd GUI material face identities")?;
    let mut bound = HashSet::new();
    for name in ctx.admit_iter(
        &group.names[material_index + 1],
        "FCStd GUI material face names",
    )? {
        for topology_id in ctx.admit_iter(
            &name.topology_ids,
            "FCStd GUI material face topology identities",
        )? {
            if !bound_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut bound,
                    topology_id.as_str(),
                    "FCStd GUI material face identities",
                )
            })? {
                continue;
            }
            let Some(face) = ctx.get_btree_map(
                &topology_index.faces,
                topology_id.as_str(),
                "FCStd GUI material face lookup",
            )?
            else {
                continue;
            };
            let face = face.try_clone_for_decode(ctx, "FCStd GUI binding face identity")?;
            let binding_index =
                topology_index.existing_bindings + plan.bindings.len() - plan.removed_binding_count;
            plan.storage.with_storage(|| {
                ctx.reserve_vec(&mut plan.bindings, 1, "FCStd GUI planned bindings")
            })?;
            plan.bindings.push(AppearanceBinding {
                id: binding_id(
                    ctx,
                    format_args!(
                        "fcstd:appearance:binding#shape-material:{provider_key}:{binding_index}"
                    ),
                )?,
                target: AppearanceTarget::Face(face),
                appearance: appearance_id
                    .try_clone_for_decode(ctx, "FCStd GUI binding appearance identity")?,
                source_entity_id: Some(
                    ctx.copy_retained_text(object_id, "FCStd GUI binding source identity")?,
                ),
                object_type: Some(ctx.copy_retained_text(
                    "ViewProvider ShapeAppearance",
                    "FCStd GUI appearance literal text",
                )?),
                visible: None,
                channels: {
                    let mut channels = BTreeMap::new();
                    ctx.insert_btree_map(
                        &mut channels,
                        cadmpeg_core::nonblank_literal!("precedence"),
                        ctx.copy_retained_text("face_over_object", "FCStd GUI binding precedence")?,
                        "FCStd GUI binding channels",
                    )?;
                    channels
                },
            });
        }
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

struct TopologyColorRequest<'a> {
    provider_name: &'a str,
    provider_key: &'a IdentityKey,
    object_id: &'a str,
    entry_name: &'a str,
    kind: TopologyColorKind,
    provenance: SourceProvenance,
}

fn transfer_topology_colors(
    ctx: &DecodeContext<'_>,
    plan: &mut AppearancePlan<'_>,
    request: TopologyColorRequest<'_>,
    sources: &GuiSources<'_, '_>,
    shape_index: &ShapeIndex<'_, '_>,
    topology_index: &TopologyIndex<'_, '_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let TopologyColorRequest {
        provider_name,
        provider_key,
        object_id,
        entry_name,
        kind,
        provenance,
    } = request;
    let entries = sources.entries;
    let requires_alpha_conversion = sources.requires_alpha_conversion;
    let view = *ctx
        .get_btree_map(entries, entry_name, "FCStd GUI side entry lookup")?
        .ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("color list references missing entry {entry_name}"),
            )
        })?;
    let (colors, _color_storage) = ctx
        .with_scoped_storage("FCStd GUI topology color lookup", || {
            parse_color_list(ctx, view, entry_name, requires_alpha_conversion)
        })?;
    let count = colors.len();
    let Some(group) = displayed_shape_group(ctx, object_id, shape_index, kind.name())? else {
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
    for (index, packed) in ctx
        .admit_iter(colors, "FCStd GUI topology color sources")?
        .enumerate()
    {
        let (appearance_id, _id_storage) = ctx
            .with_scoped_storage("FCStd GUI appearance identity storage", || {
                topology_appearance_id(ctx, kind, provider_key, index)
            })?;
        let name_groups = if count == 1 {
            group.names.as_slice()
        } else {
            &group.names[index + 1..index + 2]
        };
        let mut emitted_appearance = false;
        let mut bound_storage = ctx.reserve_scoped(0, "FCStd GUI colored topology identities")?;
        let mut bound_topology = HashSet::new();
        for names in ctx.admit_iter(name_groups, "FCStd GUI colored topology groups")? {
            for name in ctx.admit_iter(names, "FCStd GUI colored topology names")? {
                for topology_id in
                    ctx.admit_iter(&name.topology_ids, "FCStd GUI colored topology references")?
                {
                    if !bound_storage.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut bound_topology,
                            topology_id.as_str(),
                            "FCStd GUI colored topology identities",
                        )
                    })? {
                        continue;
                    }
                    let target = match kind {
                        TopologyColorKind::Face => ctx
                            .get_btree_map(
                                &topology_index.faces,
                                topology_id.as_str(),
                                "FCStd GUI colored face lookup",
                            )?
                            .map(|id| {
                                id.try_clone_for_decode(ctx, "FCStd GUI binding topology identity")
                                    .map(AppearanceTarget::Face)
                            })
                            .transpose()?,
                        TopologyColorKind::Edge => ctx
                            .get_btree_map(
                                &topology_index.edges,
                                topology_id.as_str(),
                                "FCStd GUI colored edge lookup",
                            )?
                            .map(|id| {
                                id.try_clone_for_decode(ctx, "FCStd GUI binding topology identity")
                                    .map(AppearanceTarget::Edge)
                            })
                            .transpose()?,
                        TopologyColorKind::Vertex => ctx
                            .get_btree_map(
                                &topology_index.vertices,
                                topology_id.as_str(),
                                "FCStd GUI colored vertex lookup",
                            )?
                            .map(|id| {
                                id.try_clone_for_decode(ctx, "FCStd GUI binding topology identity")
                                    .map(AppearanceTarget::Vertex)
                            })
                            .transpose()?,
                    };
                    let Some(target) = target else {
                        continue;
                    };
                    if !emitted_appearance {
                        plan.storage.with_storage(|| {
                            ctx.reserve_vec(
                                &mut plan.appearances,
                                1,
                                "FCStd GUI planned appearances",
                            )
                        })?;
                        plan.appearances.push(Appearance {
                            id: appearance_id
                                .try_clone_for_decode(ctx, "FCStd GUI appearance identity copy")?,
                            name: Some(ctx.format_retained(
                                format_args!(
                                    "{provider_name} {}{} appearance",
                                    kind.name(),
                                    index + 1
                                ),
                                "FCStd GUI appearance name",
                            )?),
                            asset_guid: None,
                            library_id: None,
                            visual_guid: None,
                            physical_token: None,
                            schema: Some(ctx.copy_retained_text(
                                kind.schema(),
                                "FCStd GUI topology appearance schema",
                            )?),
                            category: None,
                            base_color: Some(Color::from_rgba8(
                                u8::try_from((packed >> 24) & 0xff).map_err(|_| {
                                    CodecError::Malformed(
                                        "GUI color channel exceeds byte range".into(),
                                    )
                                })?,
                                u8::try_from((packed >> 16) & 0xff).map_err(|_| {
                                    CodecError::Malformed(
                                        "GUI color channel exceeds byte range".into(),
                                    )
                                })?,
                                u8::try_from((packed >> 8) & 0xff).map_err(|_| {
                                    CodecError::Malformed(
                                        "GUI color channel exceeds byte range".into(),
                                    )
                                })?,
                                u8::try_from(packed & 0xff).map_err(|_| {
                                    CodecError::Malformed(
                                        "GUI color channel exceeds byte range".into(),
                                    )
                                })?,
                            )),
                            textures: Vec::new(),
                            properties: BTreeMap::new(),
                        });
                        emitted_appearance = true;
                    }
                    let topology_key = ctx
                        .split_once(topology_id, "#", "FCStd GUI identity key")?
                        .map_or(topology_id.as_str(), |(_, key)| key);
                    let kind_key = topology_binding_kind(kind);
                    plan.storage.with_storage(|| {
                        ctx.reserve_vec(&mut plan.bindings, 1, "FCStd GUI planned bindings")
                    })?;
                    plan.bindings.push(AppearanceBinding {
                        id: binding_id(
                            ctx,
                            format_args!(
                        "fcstd:appearance:binding#{kind_key}:{provider_key}:{}:{topology_key}",
                        index + 1,
                    ),
                        )?,
                        target,
                        appearance: appearance_id
                            .try_clone_for_decode(ctx, "FCStd GUI binding appearance identity")?,
                        source_entity_id: Some(
                            ctx.copy_retained_text(object_id, "FCStd GUI binding source identity")?,
                        ),
                        object_type: Some(ctx.format_retained(
                            format_args!("ViewProvider {}", kind.name()),
                            "FCStd GUI binding object type",
                        )?),
                        visible: None,
                        channels: {
                            let mut channels = BTreeMap::new();
                            ctx.insert_btree_map(
                                &mut channels,
                                cadmpeg_core::nonblank_literal!("precedence"),
                                ctx.copy_retained_text(
                                    kind.precedence(),
                                    "FCStd GUI binding precedence",
                                )?,
                                "FCStd GUI binding channels",
                            )?;
                            channels
                        },
                    });
                }
            }
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
    use super::{decode_color, parse_material_list, requires_alpha_conversion};

    fn with_context<T>(
        bytes: &[u8],
        f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
    ) -> T {
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

        let materials = with_context(&bytes, |ctx| {
            parse_material_list(
                ctx,
                cadmpeg_core::decode::View::over_retained(&bytes),
                0,
                "property",
                true,
            )
        })
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
            let error = with_context(&bytes, |ctx| {
                parse_material_list(
                    ctx,
                    cadmpeg_core::decode::View::over_retained(&bytes),
                    0,
                    "property",
                    false,
                )
            })
            .err()
            .expect("nonfinite material scalar");
            assert!(error.to_string().contains("has non-finite scalars"));
        }
    }
}

#[cfg(test)]
mod shape_association_tests {
    use super::{
        displayed_shape_bodies, displayed_shape_group, select_shape_bodies, ShapeIndex,
        TopologyIndex,
    };
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
        crate::test_support::assert_collection_refusal_at(
            &[],
            "FCStd GUI displayed shape bodies",
            |ctx| {
                displayed_shape_bodies(
                    ctx,
                    &TopologyIndex::new(ctx, &ir)?,
                    "object",
                    &ShapeIndex::new(ctx, &properties, &payloads, &[])?,
                )
            },
        );
    }

    #[test]
    fn displayed_shape_body_identity_refuses_at_retained_limit() {
        let ir = shape_ir();
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FCStd GUI displayed body identity",
            |ctx| {
                displayed_shape_bodies(
                    ctx,
                    &TopologyIndex::new(ctx, &ir)?,
                    "object",
                    &ShapeIndex::new(ctx, &properties, &payloads, &[])?,
                )
            },
        );
    }

    #[test]
    fn repeated_payload_body_selection_refuses_at_second_slot() {
        let ir = shape_ir();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let index_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (index_ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &index_policy)
                .expect("index fixture context");
        let index = TopologyIndex::new(&index_ctx, &ir).expect("topology index");
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let error = select_shape_bodies(&ctx, &index, ["payload", "payload"])
            .expect_err("second selected body slot must be charged");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref failure)
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && failure.operation == "FCStd GUI displayed shape bodies"),
            "{error:?}"
        );
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let body_ids = select_shape_bodies(&ctx, &index, ["payload", "payload"])
            .expect("service policy admits both source occurrences");
        assert_eq!(
            body_ids,
            [ir.model.bodies[0].id.clone(), ir.model.bodies[0].id.clone()]
        );
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
                &ShapeIndex::new(
                    &ctx,
                    &[property.clone(), duplicate_property],
                    std::slice::from_ref(&payload),
                    std::slice::from_ref(&map)
                )
                .expect("shape index"),
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
                &ShapeIndex::new(
                    &ctx,
                    std::slice::from_ref(&property),
                    &[payload.clone(), duplicate_payload],
                    std::slice::from_ref(&map)
                )
                .expect("shape index"),
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
                &ShapeIndex::new(
                    &ctx,
                    std::slice::from_ref(&property),
                    std::slice::from_ref(&payload),
                    &[map, duplicate_map]
                )
                .expect("shape index"),
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_group = element_map("property", vec![group("Face"), group("Face")]);
        assert!(matches!(
            displayed_shape_group(
                &ctx,
                "object",
                &ShapeIndex::new(&ctx, &[property], &[payload], &[duplicate_group])
                    .expect("shape index"),
                "Face"
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn duplicate_shape_property_diagnostic_refuses_at_retained_limit() {
        let properties = [shape_property("property"), shape_property("property-2")];
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FCStd GUI duplicate shape property diagnostic",
            |ctx| {
                displayed_shape_group(
                    ctx,
                    "object",
                    &ShapeIndex::new(ctx, &properties, &[], &[])?,
                    "Face",
                )
            },
        );
    }

    #[test]
    fn duplicate_shape_payload_diagnostic_refuses_at_retained_limit() {
        let properties = [shape_property("property")];
        let payloads = [
            shape_payload("payload", "property"),
            shape_payload("payload-2", "property"),
        ];
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FCStd GUI duplicate shape payload diagnostic",
            |ctx| {
                displayed_shape_group(
                    ctx,
                    "object",
                    &ShapeIndex::new(ctx, &properties, &payloads, &[])?,
                    "Face",
                )
            },
        );
    }

    #[test]
    fn duplicate_element_map_diagnostic_refuses_at_retained_limit() {
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        let maps = [
            element_map("property", vec![group("Face")]),
            element_map("property", vec![group("Face")]),
        ];
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FCStd GUI duplicate element map diagnostic",
            |ctx| {
                displayed_shape_group(
                    ctx,
                    "object",
                    &ShapeIndex::new(ctx, &properties, &payloads, &maps)?,
                    "Face",
                )
            },
        );
    }

    #[test]
    fn duplicate_element_group_diagnostic_refuses_at_retained_limit() {
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        let maps = [element_map("property", vec![group("Face"), group("Face")])];
        crate::test_support::assert_retained_refusal_at(
            &[],
            "FCStd GUI duplicate element group diagnostic",
            |ctx| {
                displayed_shape_group(
                    ctx,
                    "object",
                    &ShapeIndex::new(ctx, &properties, &payloads, &maps)?,
                    "Face",
                )
            },
        );
    }
}

#[cfg(test)]
pub(crate) mod tests;
