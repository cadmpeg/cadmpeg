// SPDX-License-Identifier: Apache-2.0
//! Transfer of `GuiDocument.xml` object appearance into neutral presentation records.

mod schema;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
pub(crate) struct Graph<'ctx> {
    pub(crate) documents: Vec<GuiDocumentRecord>,
    pub(crate) providers: Vec<GuiViewProviderRecord>,
    pub(crate) properties: Vec<GuiPropertyRecord>,
    pub(crate) losses: Vec<LossNote>,
    // Native fields drop before this reservation.
    pub(crate) _native_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

struct AppearancePlan<'ctx> {
    body_updates: Vec<BodyUpdate>,
    body_update_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    appearances: Vec<Appearance>,
    appearance_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    bindings: Vec<AppearanceBinding>,
    binding_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    remove_appearances: HashSet<AppearanceId>,
    remove_appearance_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    removed_binding_count: usize,
    presentation_documents: Vec<PresentationDocument>,
    presentation_document_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    view_presentations: Vec<ViewPresentation>,
    view_presentation_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
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
        .body_update_storage
        .with_storage(|| id.try_clone_for_decode(ctx, "FCStd GUI body update identity"))?;
    let color = color?;
    plan.body_update_storage
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
            body_update_storage: ctx.reserve_scoped(0, "FCStd GUI body updates")?,
            appearances: Vec::new(),
            appearance_storage: ctx.reserve_scoped(0, "FCStd GUI planned appearances")?,
            bindings: Vec::new(),
            binding_storage: ctx.reserve_scoped(0, "FCStd GUI planned bindings")?,
            remove_appearances: HashSet::new(),
            remove_appearance_storage: ctx.reserve_scoped(0, "FCStd GUI removed appearances")?,
            removed_binding_count: 0,
            presentation_documents: Vec::new(),
            presentation_document_storage: ctx.reserve_scoped(0, "FCStd presentation documents")?,
            view_presentations: Vec::new(),
            view_presentation_storage: ctx.reserve_scoped(0, "FCStd view presentations")?,
        })
    }

    fn apply(mut self, ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
        if !self.body_updates.is_empty() {
            let (_position_storage, positions) = ctx
                .with_scoped_storage("FCStd GUI body positions", || {
                    let mut positions = BTreeMap::new();
                    let mut bodies = ir.model.bodies.iter().enumerate().rev();
                    while bodies.len() != 0 {
                        let Some((index, body)) =
                            ctx.next_charged(&mut bodies, "FCStd GUI body index")?
                        else {
                            break;
                        };
                        ctx.insert_btree_map(
                            &mut positions,
                            body.id
                                .try_clone_for_decode(ctx, "FCStd GUI body index identity")?,
                            index,
                            "FCStd GUI body positions",
                        )?;
                    }
                    Ok::<_, CodecError>(positions)
                })
                .map(|(positions, storage)| (storage, positions))?;
            let body_updates = std::mem::take(&mut self.body_updates);
            let mut updates = body_updates.into_iter();
            while updates.len() != 0 {
                let Some(update) = ctx.next_charged(&mut updates, "FCStd GUI body updates")? else {
                    break;
                };
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
            drop(positions);
            drop(_position_storage);
        }
        drop(self.body_update_storage);
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
        drop(std::mem::take(&mut self.remove_appearances));
        drop(self.remove_appearance_storage);
        let appearances = std::mem::take(&mut self.appearances);
        ctx.extend_vec(
            &mut ir.model.appearances,
            appearances,
            "FCStd neutral appearances",
        )?;
        drop(self.appearance_storage);
        let bindings = std::mem::take(&mut self.bindings);
        ctx.extend_vec(
            &mut ir.model.appearance_bindings,
            bindings,
            "FCStd neutral appearance bindings",
        )?;
        drop(self.binding_storage);
        let presentation_documents = std::mem::take(&mut self.presentation_documents);
        ctx.extend_vec(
            &mut ir.model.presentation_documents,
            presentation_documents,
            "FCStd neutral presentation documents",
        )?;
        drop(self.presentation_document_storage);
        let view_presentations = std::mem::take(&mut self.view_presentations);
        ctx.extend_vec(
            &mut ir.model.view_presentations,
            view_presentations,
            "FCStd neutral view presentations",
        )?;
        drop(self.view_presentation_storage);
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

const OBJECT_APPEARANCE_ID_PREFIX: &str = "fcstd:appearance:object#";

fn object_appearance_id(
    ctx: &DecodeContext<'_>,
    provider: &IdentityKey,
) -> Result<AppearanceId, CodecError> {
    AppearanceId::mint(ctx.format_retained(
        format_args!("{OBJECT_APPEARANCE_ID_PREFIX}{provider}"),
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
    map_sources: &'source [ElementMapRecord],
    maps_built: bool,
    _storage: [cadmpeg_core::decode::ScopedReservation<'ctx>; 2],
    map_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

impl<'source, 'ctx> ShapeIndex<'source, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        properties: &'source [PropertyRecord],
        payloads: &'source [ShapePayloadRecord],
        maps: &'source [ElementMapRecord],
    ) -> Result<Self, CodecError> {
        let mut property_storage = ctx.reserve_scoped(0, "FCStd GUI Shape property owners")?;
        let mut properties_by_owner = BTreeMap::new();
        let mut property_sources = properties.iter();
        while property_sources.len() != 0 {
            let Some(property) =
                ctx.next_charged(&mut property_sources, "FCStd GUI Shape property selection")?
            else {
                break;
            };
            if property.name == "Shape" {
                ctx.push_scoped_btree_group(
                    &mut property_storage,
                    &mut properties_by_owner,
                    property.owner.as_str(),
                    || property,
                    0,
                    "FCStd GUI Shape property owners",
                )?;
            }
        }
        let properties = properties_by_owner;
        let (payload_storage, payloads) = ctx
            .collect_scoped_btree_groups(
                payloads
                    .iter()
                    .map(|payload| (payload.property.as_str(), payload)),
                "FCStd GUI Shape payload properties",
            )
            .map(|(payloads, storage)| (storage, payloads))?;
        Ok(Self {
            properties,
            payloads,
            maps: BTreeMap::new(),
            map_sources: maps,
            maps_built: false,
            _storage: [property_storage, payload_storage],
            map_storage: None,
        })
    }

    fn ensure_maps(&mut self, ctx: &'ctx DecodeContext<'_>) -> Result<(), CodecError> {
        if self.maps_built {
            return Ok(());
        }
        let (maps, storage) = ctx.collect_scoped_btree_groups(
            self.map_sources
                .iter()
                .map(|map| (map.property.as_str(), map)),
            "FCStd GUI Shape element maps",
        )?;
        self.maps = maps;
        self.map_storage = Some(storage);
        self.maps_built = true;
        Ok(())
    }
}

struct TopologyIndex<'source, 'ctx> {
    ir: &'source CadIr,
    faces: BTreeMap<&'source str, &'source cadmpeg_ir::ids::FaceId>,
    face_built: bool,
    edges: BTreeMap<&'source str, &'source cadmpeg_ir::ids::EdgeId>,
    edge_built: bool,
    vertices: BTreeMap<&'source str, &'source cadmpeg_ir::ids::VertexId>,
    vertex_built: bool,
    bodies: BTreeMap<&'source str, Vec<(usize, &'source cadmpeg_ir::ids::BodyId)>>,
    body_candidates: Vec<BodyCandidate<'source>>,
    body_candidates_built: bool,
    existing_bindings: usize,
    face_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    edge_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    vertex_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    body_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    body_candidate_storage: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

#[derive(Clone, Copy)]
struct BodyCandidate<'source> {
    key: &'source str,
    ordinal: usize,
    id: &'source cadmpeg_ir::ids::BodyId,
}

impl<'source, 'ctx> TopologyIndex<'source, 'ctx> {
    fn new(ir: &'source CadIr) -> Self {
        Self {
            ir,
            faces: BTreeMap::new(),
            face_built: false,
            edges: BTreeMap::new(),
            edge_built: false,
            vertices: BTreeMap::new(),
            vertex_built: false,
            bodies: BTreeMap::new(),
            body_candidates: Vec::new(),
            body_candidates_built: false,
            existing_bindings: ir.model.appearance_bindings.len(),
            face_storage: None,
            edge_storage: None,
            vertex_storage: None,
            body_storage: None,
            body_candidate_storage: None,
        }
    }

    fn ensure_faces(&mut self, ctx: &'ctx DecodeContext<'_>) -> Result<(), CodecError> {
        if self.face_built {
            return Ok(());
        }
        let (faces, storage) = ctx.collect_scoped_btree_map(
            self.ir
                .model
                .faces
                .iter()
                .rev()
                .map(|face| (face.id.as_str(), &face.id)),
            "FCStd GUI face identities",
        )?;
        self.faces = faces;
        self.face_storage = Some(storage);
        self.face_built = true;
        Ok(())
    }

    fn ensure_edges(&mut self, ctx: &'ctx DecodeContext<'_>) -> Result<(), CodecError> {
        if self.edge_built {
            return Ok(());
        }
        let (edges, storage) = ctx.collect_scoped_btree_map(
            self.ir
                .model
                .edges
                .iter()
                .rev()
                .map(|edge| (edge.id.as_str(), &edge.id)),
            "FCStd GUI edge identities",
        )?;
        self.edges = edges;
        self.edge_storage = Some(storage);
        self.edge_built = true;
        Ok(())
    }

    fn ensure_vertices(&mut self, ctx: &'ctx DecodeContext<'_>) -> Result<(), CodecError> {
        if self.vertex_built {
            return Ok(());
        }
        let (vertices, storage) = ctx.collect_scoped_btree_map(
            self.ir
                .model
                .vertices
                .iter()
                .rev()
                .map(|vertex| (vertex.id.as_str(), &vertex.id)),
            "FCStd GUI vertex identities",
        )?;
        self.vertices = vertices;
        self.vertex_storage = Some(storage);
        self.vertex_built = true;
        Ok(())
    }

    fn ensure_bodies<'payload, I>(
        &mut self,
        ctx: &'ctx DecodeContext<'_>,
        payload_ids: I,
    ) -> Result<(), CodecError>
    where
        I: IntoIterator<Item = &'payload str>,
        I::IntoIter: std::iter::ExactSizeIterator,
    {
        let mut payload_sources = payload_ids.into_iter();
        if payload_sources.len() == 0 {
            return Ok(());
        }
        self.build_body_candidates(ctx)?;
        while payload_sources.len() != 0 {
            let Some(payload_id) =
                ctx.next_charged(&mut payload_sources, "FCStd GUI body payload requests")?
            else {
                break;
            };
            let payload_key =
                crate::native::id_key_charged(ctx, payload_id, "FCStd GUI identity key")?;
            if ctx
                .get_btree_map(
                    &self.bodies,
                    payload_key,
                    "FCStd GUI body payload group lookup",
                )?
                .is_some()
            {
                continue;
            }

            let first = ctx.partition_point(
                &self.body_candidates,
                |candidate| {
                    if ctx.starts_with(
                        candidate.key,
                        payload_key,
                        "FCStd GUI body payload prefix lower bound",
                    )? {
                        let suffix = &candidate.key[payload_key.len()..];
                        Ok(suffix.as_bytes().first().is_none_or(|byte| *byte < b':'))
                    } else {
                        Ok(cadmpeg_ir::ids::comparison::compare(
                            ctx,
                            candidate.key,
                            payload_key,
                            "FCStd GUI body payload prefix lower bound",
                        )? == std::cmp::Ordering::Less)
                    }
                },
                "FCStd GUI body payload prefix lower bound",
            )?;
            let candidates = &self.body_candidates[first..];
            let count = ctx.partition_point(
                candidates,
                |candidate| {
                    if !ctx.starts_with(
                        candidate.key,
                        payload_key,
                        "FCStd GUI body payload prefix range",
                    )? {
                        return Ok(false);
                    }
                    ctx.starts_with(
                        &candidate.key[payload_key.len()..],
                        ":",
                        "FCStd GUI body payload prefix range",
                    )
                },
                "FCStd GUI body payload prefix range",
            )?;
            if count == 0 {
                continue;
            }
            let candidates = &candidates[..count];
            let key = &candidates[0].key[..payload_key.len()];
            let storage = match &mut self.body_storage {
                Some(storage) => storage,
                slot @ None => slot.insert(ctx.reserve_scoped(0, "FCStd GUI body payload groups")?),
            };
            let mut matches = candidates.iter();
            while matches.len() != 0 {
                let Some(candidate) =
                    ctx.next_charged(&mut matches, "FCStd GUI body payload matches")?
                else {
                    break;
                };
                ctx.push_scoped_btree_group(
                    storage,
                    &mut self.bodies,
                    key,
                    || (candidate.ordinal, candidate.id),
                    0,
                    "FCStd GUI body payload groups",
                )?;
            }
            if let Some(bodies) = ctx.get_mut_btree_map(
                &mut self.bodies,
                key,
                "FCStd GUI body payload source-order group",
            )? {
                ctx.sort_unstable_by_key(
                    bodies,
                    |(ordinal, _)| *ordinal,
                    usize::cmp,
                    "FCStd GUI body payload source order",
                )?;
            }
        }
        Ok(())
    }

    fn build_body_candidates(&mut self, ctx: &'ctx DecodeContext<'_>) -> Result<(), CodecError> {
        if self.body_candidates_built {
            return Ok(());
        }
        let mut candidate_storage = ctx.reserve_scoped(0, "FCStd GUI body candidates")?;
        let mut candidates = Vec::new();
        let mut sources = self.ir.model.bodies.iter().enumerate();
        while sources.len() != 0 {
            let Some((ordinal, body)) =
                ctx.next_charged(&mut sources, "FCStd GUI body candidate sources")?
            else {
                break;
            };
            let key = crate::native::id_key_charged(
                ctx,
                body.id.as_str(),
                "FCStd GUI body identity key",
            )?;
            if ctx
                .position_by(
                    key.as_bytes(),
                    |byte| Ok(*byte == b':'),
                    "FCStd GUI body payload key separator",
                )?
                .is_none()
            {
                continue;
            }
            candidate_storage.with_storage(|| {
                ctx.reserve_vec(&mut candidates, 1, "FCStd GUI body candidates")
            })?;
            candidates.push(BodyCandidate {
                key,
                ordinal,
                id: &body.id,
            });
        }
        ctx.sort_unstable_by(
            &mut candidates,
            |candidate| candidate.key,
            str::cmp,
            "FCStd GUI body candidate key order",
        )?;
        self.body_candidates = candidates;
        self.body_candidate_storage = Some(candidate_storage);
        self.body_candidates_built = true;
        Ok(())
    }
}

pub(crate) fn transfer<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &mut CadIr,
    bytes: &[u8],
    sources: &GuiSources<'_, '_>,
) -> Result<Graph<'ctx>, CodecError> {
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
) -> Result<(Graph<'ctx>, AppearancePlan<'ctx>), CodecError> {
    let entries = sources.entries;
    let objects = sources.objects;
    let properties = sources.properties;
    let payloads = sources.payloads;
    let requires_alpha_conversion = sources.requires_alpha_conversion;
    let root = ctx.xml_root_element(xml, "FCStd GUI document root")?;
    let mut plan = AppearancePlan::new(ctx)?;
    let (_child_storage, children) = ctx
        .with_scoped_storage("FCStd GUI document children", || {
            ctx.collect_vec(root.children(), "FCStd GUI document children")
        })
        .map(|(children, storage)| (storage, children))?;
    let mut camera_count = 0;
    let mut camera_candidates = children.iter();
    while camera_candidates.len() != 0 {
        let Some(node) = ctx.next_charged(&mut camera_candidates, "FCStd GUI Camera count")? else {
            break;
        };
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
    let mut native_storage = ctx.reserve_scoped(0, "FCStd GUI native graph")?;
    let mut states = Vec::new();
    let mut state_candidates = children.iter();
    while state_candidates.len() != 0 {
        let Some(node) = ctx.next_charged(&mut state_candidates, "FCStd GUI state selection")?
        else {
            break;
        };
        if node.is_element()
            && !ctx.xml_has_tag_name(*node, "ViewProviderData", "FCStd GUI state tag")?
        {
            let order = states.len();
            native_storage.with_storage(|| {
                ctx.push_vec(
                    &mut states,
                    gui_state(ctx, text, order, *node)?,
                    "FCStd GUI state records",
                )
            })?;
        }
    }
    drop(children);
    drop(_child_storage);
    let document = native_storage.with_storage(|| {
        Ok::<_, CodecError>(GuiDocumentRecord {
        id: "fcstd:gui:document#0".to_owned(),
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
        })
    })?;
    let mut native_providers = Vec::new();
    let mut native_properties = Vec::new();
    let mut losses = Vec::new();
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
    while descendants.len() != 0 {
        let Some(node) = ctx.next_charged(&mut descendants, "FCStd GUI provider node selection")?
        else {
            break;
        };
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
    let mut object_storage = None;
    let mut objects_by_name: Option<HashMap<&str, &str>> = None;
    let mut payload_storage = None;
    let mut payloads_by_owner: Option<BTreeMap<&str, Vec<&str>>> = None;
    let mut shape_index: Option<ShapeIndex<'_, 'ctx>> = None;
    let mut topology_index: Option<TopologyIndex<'_, 'ctx>> = None;
    let mut edge_index: Option<PrimitiveIndex<'_, '_>> = None;
    let mut vertex_index: Option<PrimitiveIndex<'_, '_>> = None;
    let mut providers = providers.into_iter().enumerate();
    while providers.len() != 0 {
        let Some((provider_order, provider)) =
            ctx.next_charged(&mut providers, "FCStd GUI provider transfer")?
        else {
            break;
        };
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
        let object_names = match &mut objects_by_name {
            Some(names) => names,
            slot @ None => {
                let mut storage = ctx.reserve_scoped(0, "FCStd GUI object names")?;
                let mut names = HashMap::new();
                let mut object_sources = objects.iter();
                while object_sources.len() != 0 {
                    let Some(object) =
                        ctx.next_charged(&mut object_sources, "FCStd GUI object source names")?
                    else {
                        break;
                    };
                    storage.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut names,
                            object.name().as_str(),
                            object.id().as_str(),
                            "FCStd GUI object names",
                        )
                    })?;
                }
                object_storage = Some(storage);
                slot.insert(names)
            }
        };
        let Some(object_id) = ctx
            .get_hash_map(object_names, name, "FCStd GUI object name lookup")?
            .copied()
        else {
            native_storage.with_storage(|| append_native_provider(
                ctx,
                text,
                provider,
                provider_order,
                None,
                &mut native_providers,
                &mut native_properties,
            ))?;
            continue;
        };
        let (_key_storage, provider_key) = ctx
            .with_scoped_storage("FCStd GUI provider key storage", || {
                provider_identity_key(ctx, name)
            })
            .map(|(key, storage)| (storage, key))?;
        native_storage.with_storage(|| append_native_provider(
            ctx,
            text,
            provider,
            provider_order,
            Some(object_id),
            &mut native_providers,
            &mut native_properties,
        ))?;
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
        if !payloads.is_empty() && payloads_by_owner.is_none() {
            let (property_id_storage, properties_by_id) = ctx
                .collect_scoped_btree_map(
                    properties
                        .iter()
                        .rev()
                        .map(|property| (property.id.as_str(), property)),
                    "FCStd GUI payload property identities",
                )
                .map(|(properties_by_id, storage)| (storage, properties_by_id))?;
            let mut storage = ctx.reserve_scoped(0, "FCStd GUI payload owners")?;
            let mut owners = BTreeMap::new();
            let mut payload_sources = payloads.iter();
            while payload_sources.len() != 0 {
                let Some(payload) =
                    ctx.next_charged(&mut payload_sources, "FCStd GUI payload ownership")?
                else {
                    break;
                };
                if let Some(property) = ctx.get_btree_map(
                    &properties_by_id,
                    payload.property.as_str(),
                    "FCStd GUI payload property lookup",
                )? {
                    if property.name == "Shape" {
                        ctx.push_scoped_btree_group(
                            &mut storage,
                            &mut owners,
                            property.owner.as_str(),
                            || payload.id.as_str(),
                            0,
                            "FCStd GUI payload owners",
                        )?;
                    }
                }
            }
            drop(properties_by_id);
            drop(property_id_storage);
            payloads_by_owner = Some(owners);
            payload_storage = Some(storage);
        }
        let owned_payloads = if let Some(payloads_by_owner) = payloads_by_owner.as_ref() {
            ctx.get_btree_map(
                payloads_by_owner,
                object_id,
                "FCStd GUI Shape payload owner lookup",
            )?
            .map(Vec::as_slice)
            .unwrap_or_default()
        } else {
            &[]
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
        let (_body_storage, body_ids) = ctx
            .with_scoped_storage("FCStd GUI displayed bodies", || {
                if !owned_payloads.is_empty() {
                    let topology_index = match &mut topology_index {
                        Some(index) => index,
                        slot @ None => slot.insert(TopologyIndex::new(ir)),
                    };
                    topology_index.ensure_bodies(ctx, owned_payloads.iter().copied())?;
                    select_shape_bodies(ctx, &topology_index.bodies, owned_payloads.iter().copied())
                } else {
                    Ok(Vec::new())
                }
            })
            .map(|(body_ids, storage)| (storage, body_ids))?;
        let mut body_updates = body_ids.iter();
        while body_updates.len() != 0 {
            let Some(body_id) =
                ctx.next_charged(&mut body_updates, "FCStd GUI body update sources")?
            else {
                break;
            };
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
                ir,
                &mut shape_index,
                &mut topology_index,
                &mut losses,
            )?;
        }
        let mut payload_prefixes = None;
        if let Some(value) = value_attribute("LineColor", "value")? {
            if payload_prefixes.is_none() {
                payload_prefixes =
                    Some(ctx.with_scoped_storage("FCStd GUI payload prefixes", || {
                        shape_payload_prefixes(ctx, owned_payloads)
                    })?);
            }
            let color = ctx
                .parse_text::<u32>(value, "FCStd GUI presentation scalar")?
                .ok();
            if let Some((prefixes, _)) = payload_prefixes.as_ref() {
                if !prefixes.is_empty() {
                    if let Some(color) =
                        color.map(|value| convert_packed_alpha(value, requires_alpha_conversion))
                    {
                        let width = value_attribute("LineWidth", "value")?;
                        let style = PrimitiveStyle::Line(PrimitiveSize::from_source(ctx, width)?);
                        let provenance = property_provenance("LineWidth")?;
                        let index = match &mut edge_index {
                            Some(index) => {
                                index.add_prefixes(ctx, prefixes)?;
                                index
                            }
                            slot @ None => {
                                slot.insert(PrimitiveIndex::new(ctx, ir, style, prefixes)?)
                            }
                        };
                        transfer_primitive_appearance(
                            ctx,
                            index,
                            &mut plan,
                            &mut losses,
                            PrimitiveAppearanceSource {
                                provider_name: name,
                                object_id,
                                packed_color: color,
                                style,
                                payload_prefixes: prefixes,
                                provenance,
                            },
                        )?;
                    }
                }
            }
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
                ir,
                &mut shape_index,
                &mut topology_index,
                &mut losses,
            )?;
        }
        if let Some(value) = value_attribute("PointColor", "value")? {
            if payload_prefixes.is_none() {
                payload_prefixes =
                    Some(ctx.with_scoped_storage("FCStd GUI payload prefixes", || {
                        shape_payload_prefixes(ctx, owned_payloads)
                    })?);
            }
            let color = ctx
                .parse_text::<u32>(value, "FCStd GUI presentation scalar")?
                .ok();
            if let Some((prefixes, _)) = payload_prefixes.as_ref() {
                if !prefixes.is_empty() {
                    if let Some(color) =
                        color.map(|value| convert_packed_alpha(value, requires_alpha_conversion))
                    {
                        let size = value_attribute("PointSize", "value")?;
                        let style = PrimitiveStyle::Point(PrimitiveSize::from_source(ctx, size)?);
                        let provenance = property_provenance("PointSize")?;
                        let index = match &mut vertex_index {
                            Some(index) => {
                                index.add_prefixes(ctx, prefixes)?;
                                index
                            }
                            slot @ None => {
                                slot.insert(PrimitiveIndex::new(ctx, ir, style, prefixes)?)
                            }
                        };
                        transfer_primitive_appearance(
                            ctx,
                            index,
                            &mut plan,
                            &mut losses,
                            PrimitiveAppearanceSource {
                                provider_name: name,
                                object_id,
                                packed_color: color,
                                style,
                                payload_prefixes: prefixes,
                                provenance,
                            },
                        )?;
                    }
                }
            }
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
                ir,
                &mut shape_index,
                &mut topology_index,
                &mut losses,
            )?;
        }
        drop(payload_prefixes);
        let Some(packed_color) = packed_color else {
            drop(body_ids);
            drop(_body_storage);
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
        plan.appearance_storage.with_storage(|| {
            ctx.reserve_vec(&mut plan.appearances, 1, "FCStd GUI planned appearances")
        })?;
        let appearance = Appearance {
            id: appearance_id,
            name: Some(ctx.format_retained(
                format_args!("{name} shape appearance"),
                "FCStd GUI appearance name",
            )?),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("FCStd ViewProvider ShapeMaterial".to_owned()),
            category: None,
            base_color: Some(decode_color(packed_color, transparency)?),
            textures: Vec::new(),
            properties: material_properties,
        };
        let mut body_bindings = body_ids.into_iter().enumerate();
        while body_bindings.len() != 0 {
            let Some((index, body)) =
                ctx.next_charged(&mut body_bindings, "FCStd GUI body binding sources")?
            else {
                break;
            };
            plan.binding_storage.with_storage(|| {
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
                appearance: appearance
                    .id
                    .try_clone_for_decode(ctx, "FCStd GUI binding appearance identity")?,
                source_entity_id: Some(
                    ctx.copy_retained_text(object_id, "FCStd GUI binding source identity")?,
                ),
                object_type: Some("ViewProvider".to_owned()),
                visible: None,
                channels: BTreeMap::new(),
            });
        }
        drop(body_bindings);
        plan.appearances.push(appearance);
        drop(_body_storage);
    }
    drop(providers);
    drop(provider_storage);
    drop(provider_names);
    drop(provider_name_storage);
    drop(objects_by_name);
    drop(object_storage);
    drop(payloads_by_owner);
    drop(payload_storage);
    drop(edge_index);
    drop(vertex_index);
    let mut graph = Graph {
        documents: native_storage.with_storage(|| ctx.collect_vec(std::iter::once(document), "FCStd GUI document records"))?,
        providers: native_providers,
        properties: native_properties,
        losses,
        _native_storage: Some(native_storage),
    };
    let (material_storage, material_lists) = ctx
        .with_scoped_storage("FCStd GUI material lookup", || {
            validate_gui_list_payloads(ctx, &graph.properties, entries, requires_alpha_conversion)
        })
        .map(|(material_lists, storage)| (storage, material_lists))?;
    let mut material_losses = Vec::new();
    transfer_shape_appearances(
        ctx,
        &mut plan,
        &graph,
        &material_lists,
        sources,
        ir,
        &mut shape_index,
        &mut topology_index,
        &mut material_losses,
    )?;
    drop(shape_index);
    drop(topology_index);
    drop(material_lists);
    drop(material_storage);
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
    graph: &mut Graph<'_>,
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
    while attributes.len() != 0 {
        let Some(attribute) = ctx.next_charged(&mut attributes, operation)? else {
            break;
        };
        let name = ctx.copy_retained_text(attribute.name(), name_operation)?;
        let value = ctx.copy_retained_text(attribute.value(), value_operation)?;
        ctx.insert_btree_map(&mut result, name, value, operation)?;
    }
    Ok(result)
}

type GuiNamedEntries<'ctx> = (
    BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    Vec<cadmpeg_core::text::NamedEntryError>,
    cadmpeg_core::decode::ScopedReservation<'ctx>,
);

fn gui_named_entries<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    record: impl Fn() -> Result<String, CodecError>,
    mut entries: impl ExactSizeIterator<Item = (&'a str, &'a str)>,
) -> Result<GuiNamedEntries<'ctx>, CodecError> {
    use cadmpeg_core::text::{NamedEntryError, NonBlankString};
    let mut kept = BTreeMap::new();
    let mut refused_storage = ctx.reserve_scoped(0, "FCStd GUI refused property storage")?;
    let mut refused = Vec::new();
    while entries.len() != 0 {
        let Some((name, value)) =
            ctx.next_charged(&mut entries, "FCStd GUI named property entries")?
        else {
            break;
        };
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
    graph: &Graph<'_>,
    neutral_schema_version: Option<u32>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let mut state_losses = Vec::new();
    let mut documents = graph.documents.iter();
    while documents.len() != 0 {
        let Some(document) =
            ctx.next_charged(&mut documents, "FCStd presentation document sources")?
        else {
            break;
        };
        let mut presentation = PresentationDocument::new(PresentationId::compose(
            &cadmpeg_ir::identity_namespace!("fcstd", "presentation", "document"),
            cadmpeg_ir::identity_key!("0"),
        ));
        presentation.schema_version = neutral_schema_version;
        presentation.native_ref =
            Some(ctx.copy_retained_text(&document.id, "FCStd presentation document reference")?);
        let mut states = ctx.collection_vec(document.states.len(), "FCStd presentation states")?;
        let mut state_sources = document.states.iter().enumerate();
        while state_sources.len() != 0 {
            let Some((order, state)) =
                ctx.next_charged(&mut state_sources, "FCStd presentation state sources")?
            else {
                break;
            };
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
            let refused_result = charge_refused_gui_keys(ctx, &mut state_losses, &refused);
            drop(refused);
            drop(_refused_storage);
            refused_result?;
            let kind = if state.kind == "Camera" {
                PresentationStateKind::Camera(camera_state_value(ctx, state, &mut state_losses)?)
            } else {
                PresentationStateKind::Native(
                    ctx.copy_retained_text(&state.kind, "FCStd presentation state kind")?,
                )
            };
            let mut assets =
                ctx.collection_vec(state.side_entries.len(), "FCStd presentation assets")?;
            let mut asset_sources = state.side_entries.iter();
            while asset_sources.len() != 0 {
                let Some(entry) =
                    ctx.next_charged(&mut asset_sources, "FCStd presentation asset sources")?
                else {
                    break;
                };
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
        plan.presentation_document_storage.with_storage(|| {
            ctx.reserve_vec(
                &mut plan.presentation_documents,
                1,
                "FCStd presentation documents",
            )
        })?;
        plan.presentation_documents.push(presentation);
    }
    ctx.append_vec(losses, &mut state_losses, "FCStd presentation losses")?;
    if graph.providers.is_empty() {
        return Ok(());
    }

    let (_property_storage, properties) = ctx
        .collect_scoped_btree_groups(
            graph
                .properties
                .iter()
                .map(|property| (property.owner.as_str(), property)),
            "FCStd presentation property owners",
        )
        .map(|(properties, storage)| (storage, properties))?;
    let mut providers = graph.providers.iter();
    while providers.len() != 0 {
        let Some(provider) =
            ctx.next_charged(&mut providers, "FCStd presentation provider sources")?
        else {
            break;
        };
        let owned = ctx
            .get_btree_map(
                &properties,
                provider.id.as_str(),
                "FCStd presentation provider property lookup",
            )?
            .map(Vec::as_slice)
            .unwrap_or_default();
        let (
            entry_storage,
            (
                property_entries,
                line_width_value,
                point_size_value,
                visibility_value,
                display_mode_value,
                selection_style_value,
            ),
        ) = ctx
            .with_scoped_storage("FCStd GUI provider value entries", || {
                let mut entries = Vec::new();
                let mut line_width_value: Option<Option<&str>> = None;
                let mut point_size_value: Option<Option<&str>> = None;
                let mut visibility_value: Option<Option<&str>> = None;
                let mut display_mode_value: Option<Option<&str>> = None;
                let mut selection_style_value: Option<Option<&str>> = None;
                let mut property_sources = owned.iter();
                while property_sources.len() != 0 {
                    let Some(property) = ctx
                        .next_charged(&mut property_sources, "FCStd GUI provider value sources")?
                    else {
                        break;
                    };
                    let value = gui_property_value(ctx, property)?;
                    match (property.name.as_str(), property.type_name.as_str()) {
                        ("LineWidth", "App::PropertyFloatConstraint")
                            if line_width_value.is_none() =>
                        {
                            line_width_value = Some(value);
                        }
                        ("PointSize", "App::PropertyFloatConstraint")
                            if point_size_value.is_none() =>
                        {
                            point_size_value = Some(value);
                        }
                        ("Visibility", "App::PropertyBool") if visibility_value.is_none() => {
                            visibility_value = Some(value);
                        }
                        ("DisplayMode", "App::PropertyEnumeration")
                            if display_mode_value.is_none() =>
                        {
                            display_mode_value = Some(value);
                        }
                        ("SelectionStyle", "App::PropertyEnumeration")
                            if selection_style_value.is_none() =>
                        {
                            selection_style_value = Some(value);
                        }
                        _ => {}
                    }
                    ctx.push_vec(
                        &mut entries,
                        (*property, value),
                        "FCStd GUI provider value entries",
                    )?;
                }
                Ok::<_, CodecError>((
                    entries,
                    line_width_value,
                    point_size_value,
                    visibility_value,
                    display_mode_value,
                    selection_style_value,
                ))
            })
            .map(|(entries, storage)| (storage, entries))?;
        let line_width = line_width_value
            .flatten()
            .map(|value| {
                ctx.parse_text::<f64>(value, "FCStd GUI provider size")
                    .map(Result::ok)
            })
            .transpose()?
            .flatten();
        let point_size = point_size_value
            .flatten()
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
        drop(property_entries);
        drop(entry_storage);
        let refused_result = charge_refused_gui_keys(ctx, losses, &refused);
        drop(refused);
        drop(_refused_storage);
        refused_result?;
        plan.view_presentation_storage.with_storage(|| {
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
            visible: visibility_value.flatten().and_then(parse_bool),
            display_mode: display_mode_value
                .flatten()
                .map(|value| ctx.copy_retained_text(value, "FCStd view display mode"))
                .transpose()?,
            selection_style: selection_style_value
                .flatten()
                .map(|value| ctx.copy_retained_text(value, "FCStd view selection style"))
                .transpose()?,
            line_width,
            point_size,
            properties: provider_properties,
            native_ref: Some(ctx.copy_retained_text(&provider.id, "FCStd view native reference")?),
        });
    }
    drop(properties);
    drop(_property_storage);
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
    let mut refused_sources = refused.iter();
    while refused_sources.len() != 0 {
        let Some(key) = ctx.next_charged(
            &mut refused_sources,
            "FCStd GUI refused property loss sources",
        )?
        else {
            break;
        };
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
    let refused_result = charge_refused_gui_keys(ctx, losses, &refused);
    drop(refused);
    drop(_refused_storage);
    refused_result?;
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

    let (_token_storage, mut tokens) = ctx
        .temporary_vec(
            settings.split_whitespace().count(),
            "FCStd GUI camera tokens",
        )
        .map(|(tokens, storage)| (storage, tokens))?;
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

#[derive(Clone, Copy)]
enum PrimitiveTarget<'source> {
    Edge(&'source cadmpeg_ir::ids::EdgeId),
    Vertex(&'source cadmpeg_ir::ids::VertexId),
}

#[derive(Clone, Copy)]
struct PrimitiveCandidate<'source> {
    key: &'source str,
    ordinal: usize,
    target: PrimitiveTarget<'source>,
}

/// Borrowed primitive identities sorted by key and indexed for requested prefixes.
struct PrimitiveIndex<'source, 'ctx> {
    by_prefix: BTreeMap<&'source str, Vec<(usize, PrimitiveTarget<'source>)>>,
    candidates: Vec<PrimitiveCandidate<'source>>,
    index_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    candidate_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'source, 'ctx> PrimitiveIndex<'source, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        ir: &'source CadIr,
        style: PrimitiveStyle,
        requested_prefixes: &[String],
    ) -> Result<Self, CodecError> {
        let mut index = Self {
            by_prefix: BTreeMap::new(),
            candidates: Vec::new(),
            index_storage: ctx.reserve_scoped(0, "FCStd GUI primitive index")?,
            candidate_storage: ctx.reserve_scoped(0, "FCStd GUI primitive candidates")?,
        };
        let len = match style {
            PrimitiveStyle::Line(_) => ir.model.edges.len(),
            PrimitiveStyle::Point(_) => ir.model.vertices.len(),
        };
        let mut ordinals = 0..len;
        while ordinals.len() != 0 {
            let Some(ordinal) =
                ctx.next_charged(&mut ordinals, "FCStd GUI primitive candidates")?
            else {
                break;
            };
            let (id, target) = match style {
                PrimitiveStyle::Line(_) => {
                    let id = &ir.model.edges[ordinal].id;
                    (id.as_str(), PrimitiveTarget::Edge(id))
                }
                PrimitiveStyle::Point(_) => {
                    let id = &ir.model.vertices[ordinal].id;
                    (id.as_str(), PrimitiveTarget::Vertex(id))
                }
            };
            let key = crate::native::id_key_charged(ctx, id, "FCStd GUI primitive identity key")?;
            index.candidate_storage.with_storage(|| {
                ctx.reserve_vec(&mut index.candidates, 1, "FCStd GUI primitive candidates")
            })?;
            index.candidates.push(PrimitiveCandidate {
                key,
                ordinal,
                target,
            });
        }
        ctx.sort_unstable_by(
            &mut index.candidates,
            |candidate| candidate.key,
            str::cmp,
            "FCStd GUI primitive candidate key order",
        )?;
        index.add_prefixes(ctx, requested_prefixes)?;
        Ok(index)
    }

    fn add_prefixes(
        &mut self,
        ctx: &DecodeContext<'_>,
        prefixes: &[String],
    ) -> Result<(), CodecError> {
        if prefixes.is_empty() {
            return Ok(());
        }
        let mut requested_storage =
            ctx.reserve_scoped(0, "FCStd GUI requested primitive prefixes")?;
        let mut requested = BTreeSet::new();
        let mut prefix_sources = prefixes.iter();
        while prefix_sources.len() != 0 {
            let Some(prefix) = ctx.next_charged(
                &mut prefix_sources,
                "FCStd GUI requested primitive prefix sources",
            )?
            else {
                break;
            };
            requested_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut requested,
                    prefix.as_str(),
                    "FCStd GUI requested primitive prefixes",
                )
            })?;
        }
        let mut prefix_searches = requested.iter();
        while prefix_searches.len() != 0 {
            let Some(prefix) = ctx.next_charged(
                &mut prefix_searches,
                "FCStd GUI requested primitive prefix searches",
            )?
            else {
                break;
            };
            if ctx.contains_key_btree_map(
                &self.by_prefix,
                *prefix,
                "FCStd GUI primitive indexed prefix lookup",
            )? {
                continue;
            }
            let first = ctx.partition_point(
                &self.candidates,
                |candidate| {
                    Ok(ctx.compare(
                        candidate.key,
                        *prefix,
                        "FCStd GUI primitive prefix lower bound",
                    )? == std::cmp::Ordering::Less)
                },
                "FCStd GUI primitive prefix lower bound",
            )?;
            let matching = &self.candidates[first..];
            let count = ctx.partition_point(
                matching,
                |candidate| {
                    ctx.starts_with(candidate.key, *prefix, "FCStd GUI primitive prefix range")
                },
                "FCStd GUI primitive prefix upper bound",
            )?;
            if count == 0 {
                continue;
            }
            let candidates = &matching[..count];
            let key = &candidates[0].key[..prefix.len()];
            let mut matches = candidates.iter();
            while matches.len() != 0 {
                let Some(candidate) =
                    ctx.next_charged(&mut matches, "FCStd GUI primitive prefix matches")?
                else {
                    break;
                };
                ctx.push_scoped_btree_group(
                    &mut self.index_storage,
                    &mut self.by_prefix,
                    key,
                    || (candidate.ordinal, candidate.target),
                    0,
                    "FCStd GUI primitive index",
                )?;
            }
        }
        drop(requested);
        drop(requested_storage);
        Ok(())
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
    let mut payload_sources = payloads.iter();
    while payload_sources.len() != 0 {
        let Some(payload) =
            ctx.next_charged(&mut payload_sources, "FCStd GUI payload prefix sources")?
        else {
            break;
        };
        ctx.reserve_vec(&mut prefixes, 1, "FCStd GUI payload prefixes")?;
        prefixes.push(ctx.retained_suffix(
            crate::native::id_key_charged(ctx, payload, "FCStd GUI identity key")?,
            ":",
            "FCStd GUI payload prefix text",
        )?);
    }
    Ok(prefixes)
}

fn transfer_primitive_appearance(
    ctx: &DecodeContext<'_>,
    index: &PrimitiveIndex<'_, '_>,
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
    let mut selected_storage = ctx.reserve_scoped(0, "FCStd GUI primitive selection")?;
    let mut selected = BTreeMap::new();
    let mut prefixes = payload_prefixes.iter();
    while prefixes.len() != 0 {
        let Some(prefix) =
            ctx.next_charged(&mut prefixes, "FCStd GUI primitive payload prefixes")?
        else {
            break;
        };
        if let Some(candidates) = ctx.get_btree_map(
            &index.by_prefix,
            prefix.as_str(),
            "FCStd GUI primitive prefix lookup",
        )? {
            let mut prefix_candidates = candidates.iter();
            while prefix_candidates.len() != 0 {
                let Some(&(ordinal, target)) = ctx.next_charged(
                    &mut prefix_candidates,
                    "FCStd GUI primitive prefix candidates",
                )?
                else {
                    break;
                };
                selected_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut selected,
                        ordinal,
                        target,
                        "FCStd GUI primitive selection",
                    )
                })?;
            }
        }
    }
    let mut targets = Vec::new();
    let mut selected_targets = selected.into_iter();
    while selected_targets.len() != 0 {
        let Some((_, target)) = ctx.next_charged(
            &mut selected_targets,
            "FCStd GUI primitive selected targets",
        )?
        else {
            break;
        };
        target_storage
            .with_storage(|| ctx.reserve_vec(&mut targets, 1, "FCStd GUI primitive targets"))?;
        targets.push(match target {
            PrimitiveTarget::Edge(id) => AppearanceTarget::Edge(
                id.try_clone_for_decode(ctx, "FCStd GUI primitive target identity")?,
            ),
            PrimitiveTarget::Vertex(id) => AppearanceTarget::Vertex(
                id.try_clone_for_decode(ctx, "FCStd GUI primitive target identity")?,
            ),
        });
    }
    drop(selected_targets);
    drop(selected_storage);
    if targets.is_empty() {
        drop(targets);
        drop(target_storage);
        return Ok(());
    }
    let (_key_storage, provider_key) = ctx
        .with_scoped_storage("FCStd GUI provider key storage", || {
            provider_identity_key(ctx, provider_name)
        })
        .map(|(key, storage)| (storage, key))?;
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
    plan.appearance_storage.with_storage(|| {
        ctx.reserve_vec(&mut plan.appearances, 1, "FCStd GUI planned appearances")
    })?;
    let appearance = Appearance {
        id: appearance_id,
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
    };
    let mut binding_sources = targets.into_iter().enumerate();
    while binding_sources.len() != 0 {
        let Some((index, target)) =
            ctx.next_charged(&mut binding_sources, "FCStd GUI primitive binding sources")?
        else {
            break;
        };
        plan.binding_storage.with_storage(|| {
            ctx.reserve_vec(&mut plan.bindings, 1, "FCStd GUI planned bindings")
        })?;
        plan.bindings.push(AppearanceBinding {
            id: binding_id(
                ctx,
                format_args!("fcstd:appearance:binding#{binding_key}:{provider_key}:{index}"),
            )?,
            target,
            appearance: appearance
                .id
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
    drop(binding_sources);
    plan.appearances.push(appearance);
    drop(target_storage);
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
    while descendants.len() != 0 {
        let Some(value) = ctx.next_charged(&mut descendants, "FCStd GUI state values traversal")?
        else {
            break;
        };
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
    while descendants.len() != 0 {
        let Some(element) =
            ctx.next_charged(&mut descendants, "FCStd GUI state side-reference elements")?
        else {
            break;
        };
        if !element.is_element() {
            continue;
        }
        let mut attributes = element.attributes();
        while attributes.len() != 0 {
            let Some(attribute) =
                ctx.next_charged(&mut attributes, "FCStd GUI state side-reference attributes")?
            else {
                break;
            };
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
    let key_result = ctx.format_scoped(
        format_args!("{kind}:{order}"),
        "FCStd GUI state identity key",
    )?;
    let _key_storage = key_result.1;
    let key = key_result.0;
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
    let id = crate::native::native_id_charged(ctx, "gui-view-provider", name)?;
    ctx.reserve_vec(providers, 1, "FCStd GUI provider records")?;
    let record = GuiViewProviderRecord {
        id,
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
    };
    let id = &record.id;
    let Some(container) = unique_child(ctx, provider, "Properties")? else {
        return Err(gui_malformed(
            ctx,
            format_args!("ViewProvider {name} has no Properties"),
        ));
    };
    let (_node_storage, property_nodes) = ctx
        .with_scoped_storage("FCStd GUI provider property nodes", || {
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
        })
        .map(|(nodes, storage)| (storage, nodes))?;
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
    let mut property_nodes = property_nodes.into_iter().enumerate();
    while property_nodes.len() != 0 {
        let Some((property_order, property)) =
            ctx.next_charged(&mut property_nodes, "FCStd GUI provider property transfer")?
        else {
            break;
        };
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
        while descendants.len() != 0 {
            let Some(value) =
                ctx.next_charged(&mut descendants, "FCStd GUI property values traversal")?
            else {
                break;
            };
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
            let mut values_iter = values.iter();
            while values_iter.len() != 0 {
                let Some(value) =
                    ctx.next_charged(&mut values_iter, "FCStd GUI property side-reference values")?
                else {
                    break;
                };
                let mut attributes = value.attributes.iter();
                while attributes.len() != 0 {
                    let Some((attribute, entry)) = ctx.next_charged(
                        &mut attributes,
                        "FCStd GUI property side-reference attributes",
                    )?
                    else {
                        break;
                    };
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
            id: crate::native::native_child_id_charged(ctx, "gui-property", id, property_name)?,
            owner: ctx.copy_retained_text(id, "FCStd GUI property owner")?,
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
    drop(property_nodes);
    drop(_node_storage);
    drop(names);
    drop(name_storage);
    providers.push(record);
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

fn gui_element_child_count(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
    operation: &'static str,
) -> Result<usize, CodecError> {
    let mut count = 0;
    let mut children = node.children();
    while let Some(child) = ctx.next_charged(&mut children, operation)? {
        count += usize::from(child.is_element());
    }
    Ok(count)
}

fn validate_gui_string_list(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let count = gui_list_count(ctx, root, property_name, "StringList")?;
    let element_count = gui_element_child_count(ctx, root, "FCStd GUI StringList element count")?;
    if element_count != count
        || ctx.any_by(
            root.children(),
            |value| {
                if !value.is_element() {
                    return Ok(false);
                }
                Ok(
                    !ctx.xml_has_tag_name(value, "String", "FCStd GUI value tag")?
                        || ctx
                            .xml_attribute(value, "value", "FCStd GUI value attribute")?
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
        root.children(),
        |value| {
            if value.is_element() {
                has_nested_gui_elements(ctx, value)
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
    let element_count = gui_element_child_count(ctx, root, "FCStd GUI integer-list element count")?;
    if element_count != count
        || ctx.any_by(
            root.children(),
            |value| {
                Ok(value.is_element()
                    && !ctx.xml_has_tag_name(value, "I", "FCStd GUI value tag")?)
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
        root.children(),
        |value| {
            if value.is_element() {
                has_nested_gui_elements(ctx, value)
            } else {
                Ok(false)
            }
        },
        "FCStd GUI integer-list nested value scan",
    )? {
        return Err(gui_nested_value_error(ctx, property_name, tag));
    }
    let mut previous = None;
    let mut values = root.children();
    while let Some(value) = ctx.next_charged(&mut values, "FCStd GUI integer-list values")? {
        if !value.is_element() {
            continue;
        }
        let number_text = ctx
            .xml_attribute(value, "v", "FCStd GUI value attribute")?
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
    let element_count = gui_element_child_count(ctx, root, "FCStd GUI Map element count")?;
    if element_count != count
        || ctx.any_by(
            root.children(),
            |value| {
                Ok(value.is_element()
                    && !ctx.xml_has_tag_name(value, "Item", "FCStd GUI value tag")?)
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
        root.children(),
        |value| {
            if value.is_element() {
                has_nested_gui_elements(ctx, value)
            } else {
                Ok(false)
            }
        },
        "FCStd GUI Map nested item scan",
    )? {
        return Err(gui_nested_value_error(ctx, property_name, "Map item"));
    }
    let mut previous_key = None;
    let mut values = root.children();
    while let Some(value) = ctx.next_charged(&mut values, "FCStd GUI Map values")? {
        if !value.is_element() {
            continue;
        }
        let key = ctx
            .xml_attribute(value, "key", "FCStd GUI value attribute")?
            .ok_or_else(|| {
                gui_malformed(
                    ctx,
                    format_args!("GUI property {property_name} Map item has no key"),
                )
            })?;
        if ctx
            .xml_attribute(value, "value", "FCStd GUI value attribute")?
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
    let element_count = gui_element_child_count(ctx, custom_list, "FCStd GUI custom enumeration element count")?;
    if element_count != count
        || ctx.any_by(
            custom_list.children(),
            |value| {
                if !value.is_element() {
                    return Ok(false);
                }
                Ok(
                    !ctx.xml_has_tag_name(value, "Enum", "FCStd GUI value tag")?
                        || ctx
                            .xml_attribute(value, "value", "FCStd GUI value attribute")?
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
    'references: while descendants.len() != 0 {
        let Some(node) = ctx.next_charged(&mut descendants, "FCStd GUI geometry descendants")?
        else {
            break;
        };
        if !node.is_element() {
            continue;
        }
        let mut attributes = node.attributes();
        while attributes.len() != 0 {
            let Some(attribute) =
                ctx.next_charged(&mut attributes, "FCStd GUI geometry reference attributes")?
            else {
                break;
            };
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
    let Ok(count) = ctx.parse_text::<usize>(count_text, "FCStd GUI TechDraw list count")? else {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            format_args!("{list_tag} has an invalid count"),
        ));
    };
    let record_count = gui_element_child_count(ctx, root, "FCStd GUI TechDraw record count")?;
    if record_count != count {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            format_args!("{list_tag} count does not match its records"),
        ));
    }
    let mut records = root.children();
    while let Some(record) = ctx.next_charged(&mut records, "FCStd GUI TechDraw records")? {
        if !record.is_element() {
            continue;
        }
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
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        Vec<roxmltree::Node<'a, 'input>>,
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
    .map(|(fields, storage)| (storage, fields))
}

fn validate_gui_geom_format_record(
    ctx: &DecodeContext<'_>,
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let (_field_storage, fields) = gui_record_fields(ctx, record, "FCStd GUI GeomFormat fields")?;
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
    let Ok(weight) = ctx.parse_text::<f64>(weight_text, "FCStd GUI GeomFormat weight")? else {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "GeomFormat has an invalid weight",
        ));
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
    let (_field_storage, fields) = gui_record_fields(ctx, record, "FCStd GUI CenterLine fields")?;
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
    for field in &fields[3..7] {
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
    let Ok(count) = ctx.parse_text::<usize>(count_text, "FCStd GUI CenterLine collection count")?
    else {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CenterLine collection has an invalid count",
        ));
    };
    let item_count = gui_element_child_count(ctx, field, "FCStd GUI CenterLine collection item count")?;
    if item_count != count {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "CenterLine collection count does not match its records",
        ));
    }
    let mut items = field.children();
    while let Some(item) = ctx.next_charged(&mut items, "FCStd GUI CenterLine collection items")? {
        if !item.is_element() {
            continue;
        }
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
    let (_field_storage, fields) = gui_record_fields(ctx, record, "FCStd GUI CosmeticEdge fields")?;
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
    let Ok(count) = ctx.parse_text::<usize>(count_text, "FCStd GUI TechDraw Points count")? else {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw Points has an invalid count",
        ));
    };
    let point_count = gui_element_child_count(ctx, field, "FCStd GUI TechDraw point count")?;
    if point_count != count {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw Points count does not match its records",
        ));
    }
    let mut points = field.children();
    while let Some(point) = ctx.next_charged(&mut points, "FCStd GUI TechDraw point records")? {
        if !point.is_element() {
            continue;
        }
        if !ctx.xml_has_tag_name(point, "Point", "FCStd GUI value tag")?
            || has_nested_gui_elements(ctx, point)?
        {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "TechDraw Points has an invalid point record",
            ));
        }
        validate_gui_techdraw_point_values(ctx, point, property_name)?;
    }
    Ok(())
}

fn validate_gui_cosmetic_vertex_record(
    ctx: &DecodeContext<'_>,
    record: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
    let (_field_storage, fields) =
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
    let mut base_fields_match = true;
    for (field, expected_tag) in fields.iter().zip(base_fields) {
        if !ctx.xml_has_tag_name(*field, expected_tag, "FCStd GUI value tag")? {
            base_fields_match = false;
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
    for field in &fields[base_fields.len()..] {
        if has_nested_gui_elements(ctx, *field)? {
            return Err(gui_techdraw_error(
                ctx,
                property_name,
                "CosmeticVertex has a nested field",
            ));
        }
    }
    validate_gui_techdraw_point_values(ctx, fields[0], property_name)?;
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
    validate_gui_techdraw_point_values(ctx, fields[cursor], property_name)?;
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
    validate_gui_techdraw_point_values(ctx, field, property_name)
}

fn validate_gui_techdraw_point_values(
    ctx: &DecodeContext<'_>,
    field: roxmltree::Node<'_, '_>,
    property_name: &str,
) -> Result<(), CodecError> {
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
    let Ok(value) = ctx.parse_text::<f64>(text, "FCStd GUI TechDraw scalar")? else {
        return Err(gui_techdraw_error(
            ctx,
            property_name,
            "TechDraw scalar is invalid",
        ));
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
    let element_count = gui_element_child_count(ctx, root, "FCStd GUI VisualLayerList element count")?;
    if element_count != count
        || ctx.any_by(
            root.children(),
            |layer| {
                Ok(layer.is_element()
                    && !ctx.xml_has_tag_name(layer, "VisualLayer", "FCStd GUI value tag")?)
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
    let mut layers = root.children();
    while let Some(layer) = ctx.next_charged(&mut layers, "FCStd GUI VisualLayer records")? {
        if !layer.is_element() {
            continue;
        }
        if !matches!(
            ctx.xml_attribute(layer, "visible", "FCStd GUI value attribute")?,
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
            .xml_attribute(layer, "linePattern", "FCStd GUI value attribute")?
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
            .xml_attribute(layer, "lineWidth", "FCStd GUI value attribute")?
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
    let mut expression_count = 0_usize;
    let mut expression_children = root.children();
    while let Some(child) = ctx.next_charged(&mut expression_children, "FCStd GUI expression count")? {
        if ctx.xml_has_tag_name(child, "Expression", "FCStd GUI value tag")? {
            expression_count = expression_count
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("GUI expression count overflows"))?;
        }
    }
    if expression_count != count
        || ctx.any_by(
            root.children(),
            |expression| {
                if !ctx.xml_has_tag_name(expression, "Expression", "FCStd GUI value tag")? {
                    return Ok(false);
                }
                Ok(ctx
                    .xml_attribute(expression, "path", "FCStd GUI value attribute")?
                    .is_none()
                    || ctx
                        .xml_attribute(expression, "expression", "FCStd GUI value attribute")?
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
    let element_count = gui_element_child_count(ctx, root, "FCStd GUI GeometryList element count")?;
    if element_count != count
        || ctx.any_by(
            root.children(),
            |geometry| {
                Ok(geometry.is_element()
                    && !ctx.xml_has_tag_name(geometry, "Geometry", "FCStd GUI value tag")?)
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
    let element_count = gui_element_child_count(ctx, root, "FCStd GUI ShapeList element count")?;
    if element_count != count
        || ctx.any_by(
            root.children(),
            |shape| {
                if !shape.is_element() {
                    return Ok(false);
                }
                if !ctx.xml_has_tag_name(shape, "TopoShape", "FCStd GUI value tag")? {
                    return Ok(true);
                }
                Ok(ctx
                    .xml_attribute(shape, "file", "FCStd GUI value attribute")?
                    .is_none()
                    && ctx
                        .xml_attribute(shape, "binary", "FCStd GUI value attribute")?
                        .is_none()
                    && ctx
                        .xml_attribute(shape, "brep", "FCStd GUI value attribute")?
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
    let element_count = gui_element_child_count(ctx, root, "FCStd GUI ConstraintList element count")?;
    if element_count != count
        || ctx.any_by(
            root.children(),
            |constraint| {
                Ok(constraint.is_element()
                    && !ctx.xml_has_tag_name(constraint, "Constrain", "FCStd GUI value tag")?)
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
    let mut property_sources = properties.iter();
    while property_sources.len() != 0 {
        let Some(property) =
            ctx.next_charged(&mut property_sources, "FCStd GUI list property sources")?
        else {
            break;
        };
        if property.side_entries.is_empty() {
            continue;
        }
        if property.type_name == "Part::PropertyTopoShapeList" {
            let mut entry_names = property.side_entries.iter();
            while entry_names.len() != 0 {
                let Some(entry_name) =
                    ctx.next_charged(&mut entry_names, "FCStd GUI shape-list entry references")?
                else {
                    break;
                };
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
                validate_color_list_layout(ctx, view, entry_name)?;
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
    let count = read_color_list_count(ctx, &mut view, entry_name)?;
    let mut colors = ctx.collection_vec(count, "FCStd GUI color-list entries")?;
    let mut indices = 0..count;
    while indices.len() != 0 {
        let Some(_) = ctx.next_charged(&mut indices, "FCStd GUI color-list read")? else {
            break;
        };
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

fn validate_color_list_layout(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    entry_name: &str,
) -> Result<usize, CodecError> {
    let count = read_color_list_count(ctx, &mut view, entry_name)?;
    let byte_count = count.checked_mul(4).ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("color-list entry {entry_name} count exceeds its payload"),
        )
    })?;
    if view.skip(byte_count).is_none() {
        return Err(gui_malformed(
            ctx,
            format_args!("color-list entry {entry_name} count exceeds its payload"),
        ));
    }
    if !view.is_empty() {
        return Err(gui_malformed(
            ctx,
            format_args!("color-list entry {entry_name} has trailing bytes"),
        ));
    }
    Ok(count)
}

fn read_color_list_count(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    entry_name: &str,
) -> Result<usize, CodecError> {
    let count = view.req_u32_le()?;
    view.counted(count.into(), 4)
        .map(|count| count.get())
        .ok_or_else(|| {
            gui_malformed(
                ctx,
                format_args!("color-list entry {entry_name} count exceeds its payload"),
            )
        })
}

fn read_gui_counted<'a>(
    ctx: &DecodeContext<'_>,
    view: &mut View<'a>,
    count: u32,
    element_size: usize,
    mut validate: impl FnMut(&mut View<'a>) -> Option<bool>,
    operation: &'static str,
) -> Result<Option<bool>, CodecError> {
    let Some(count) = view.counted(count.into(), element_size) else {
        return Ok(None);
    };
    let mut indices = 0..count.get();
    while indices.len() != 0 {
        let Some(_) = ctx.next_charged(&mut indices, operation)? else {
            break;
        };
        let Some(valid) = validate(view) else {
            return Ok(None);
        };
        if !valid {
            return Ok(Some(false));
        }
    }
    Ok(Some(true))
}

fn parse_float_list(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    entry_name: &str,
) -> Result<(), CodecError> {
    let count = view.req_u32_le()?;
    let valid = read_gui_counted(
        ctx,
        &mut view,
        count,
        8,
        |view| {
            let value = view.f64_le()?;
            Some(value.is_finite())
        },
        "FCStd GUI float-list entries",
    )?
    .ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("float-list entry {entry_name} count exceeds its payload"),
        )
    })?;
    if !valid {
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
    let valid = read_gui_counted(
        ctx,
        &mut view,
        count,
        24,
        |view| {
            let value = [view.f64_le()?, view.f64_le()?, view.f64_le()?];
            Some(value.iter().all(|scalar| scalar.is_finite()))
        },
        "FCStd GUI vector-list entries",
    )?
    .ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("vector-list entry {entry_name} count exceeds its payload"),
        )
    })?;
    if !valid {
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
    let valid = read_gui_counted(
        ctx,
        &mut view,
        count,
        56,
        |view| {
            let value = [
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
                view.f64_le()?,
            ];
            Some(value.iter().all(|scalar| scalar.is_finite()))
        },
        "FCStd GUI placement-list entries",
    )?
    .ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("placement-list entry {entry_name} count exceeds its payload"),
        )
    })?;
    if !valid {
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
    let valid = read_gui_counted(
        ctx,
        &mut view,
        count,
        20,
        |view| {
            view.i32_le()?;
            let radius1 = view.f64_le()?;
            let radius2 = view.f64_le()?;
            Some(radius1.is_finite() && radius2.is_finite())
        },
        "FCStd GUI fillet-edge entries",
    )?
    .ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("fillet-edges entry {entry_name} count exceeds its payload"),
        )
    })?;
    if !valid {
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
    let count = view.counted(count.into(), 24).ok_or_else(|| {
        gui_malformed(
            ctx,
            format_args!("GUI material list {property_id} count exceeds its payload"),
        )
    })?;
    let mut materials = ctx.collection_vec(count.get(), "FCStd GUI material entries")?;
    let mut indices = 0..count.get();
    while indices.len() != 0 {
        let Some(_) = ctx.next_charged(&mut indices, "FCStd GUI material records")? else {
            break;
        };
        let ambient = view.req_u32_le()?;
        let diffuse = view.req_u32_le()?;
        let specular = view.req_u32_le()?;
        let emissive = view.req_u32_le()?;
        let shininess = view.req_f32_le()?;
        let transparency = view.req_f32_le()?;
        let invalid = || {
            gui_malformed(
                ctx,
                format_args!("GUI material list {property_id} has non-finite scalars"),
            )
        };
        materials.push(GuiMaterial {
            ambient: convert_packed_alpha(ambient, requires_alpha_conversion),
            diffuse: convert_packed_alpha(diffuse, requires_alpha_conversion),
            specular: convert_packed_alpha(specular, requires_alpha_conversion),
            emissive: convert_packed_alpha(emissive, requires_alpha_conversion),
            shininess: FiniteBinary32::new(shininess).ok_or_else(invalid)?,
            transparency: FiniteBinary32::new(transparency).ok_or_else(invalid)?,
            uuid: "",
        });
    }
    if has_strings {
        let mut materials_iter = materials.iter_mut();
        while materials_iter.len() != 0 {
            let Some(material) =
                ctx.next_charged(&mut materials_iter, "FCStd GUI material records")?
            else {
                break;
            };
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

fn transfer_shape_appearances<'source, 'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    plan: &mut AppearancePlan<'_>,
    graph: &Graph<'_>,
    material_lists: &HashMap<&str, Vec<GuiMaterial<'_>>>,
    sources: &GuiSources<'source, '_>,
    ir: &'ir CadIr,
    shape_index: &mut Option<ShapeIndex<'source, 'ctx>>,
    topology_index: &mut Option<TopologyIndex<'ir, 'ctx>>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if material_lists.is_empty() {
        return Ok(());
    }
    let mut appearance_property_storage =
        ctx.reserve_scoped(0, "FCStd GUI ShapeAppearance owners")?;
    let mut appearance_properties = BTreeMap::new();
    let mut property_sources = graph.properties.iter().rev();
    while property_sources.len() != 0 {
        let Some(property) =
            ctx.next_charged(&mut property_sources, "FCStd GUI ShapeAppearance selection")?
        else {
            break;
        };
        if property.name == "ShapeAppearance" && property.type_name == "App::PropertyMaterialList" {
            appearance_property_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut appearance_properties,
                    property.owner.as_str(),
                    property,
                    "FCStd GUI ShapeAppearance owners",
                )
            })?;
        }
    }
    let mut binding_count_storage = None;
    let mut binding_counts: Option<BTreeMap<String, usize>> = None;
    let mut providers = graph.providers.iter();
    while providers.len() != 0 {
        let Some(provider) = ctx.next_charged(&mut providers, "FCStd GUI material providers")?
        else {
            break;
        };
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
        let shape_index = match &mut *shape_index {
            Some(index) => index,
            slot @ None => slot.insert(ShapeIndex::new(
                ctx,
                sources.properties,
                sources.payloads,
                sources.element_maps,
            )?),
        };
        let (_key_storage, provider_key) = ctx
            .with_scoped_storage("FCStd GUI provider key storage", || {
                provider_identity_key(ctx, &provider.name)
            })
            .map(|(key, storage)| (storage, key))?;
        let (_body_storage, body_ids) = ctx
            .with_scoped_storage("FCStd GUI displayed bodies", || {
                if materials.len() == 1 {
                    let topology_index = match &mut *topology_index {
                        Some(index) => index,
                        slot @ None => slot.insert(TopologyIndex::new(ir)),
                    };
                    displayed_shape_bodies(ctx, topology_index, object_id, shape_index)
                } else {
                    Ok(Vec::new())
                }
            })
            .map(|(body_ids, storage)| (storage, body_ids))?;
        let group = displayed_shape_group(ctx, object_id, shape_index, "Face")?;
        let mapped_count = match group {
            None => 0,
            Some(group) => match group.names.len() {
                0 => 0,
                length => length - 1,
            },
        };
        if materials.len() == 1 {
            let counts = match &mut binding_counts {
                Some(counts) => counts,
                slot @ None => {
                    let (counts, storage) =
                        ctx.with_scoped_storage("FCStd GUI legacy binding counts", || {
                            let mut counts = BTreeMap::<String, usize>::new();
                            let mut bindings = plan.bindings.iter();
                            while bindings.len() != 0 {
                                let Some(binding) = ctx.next_charged(
                                    &mut bindings,
                                    "FCStd GUI legacy binding count sources",
                                )?
                                else {
                                    break;
                                };
                                if !binding
                                    .appearance
                                    .as_str()
                                    .starts_with(OBJECT_APPEARANCE_ID_PREFIX)
                                {
                                    continue;
                                }
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
                                    ctx.insert_btree_map(
                                        &mut counts,
                                        key,
                                        1,
                                        "FCStd GUI legacy binding counts",
                                    )?;
                                }
                            }
                            Ok::<_, CodecError>(counts)
                        })?;
                    binding_count_storage = Some(storage);
                    slot.insert(counts)
                }
            };
            let legacy_id = plan
                .remove_appearance_storage
                .with_storage(|| object_appearance_id(ctx, &provider_key))?;
            let removed_count = ctx
                .get_btree_map(
                    counts,
                    legacy_id.as_str(),
                    "FCStd GUI removed binding count lookup",
                )?
                .copied()
                .unwrap_or(0);
            if plan.remove_appearance_storage.with_storage(|| {
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
        if materials.len() > 1 {
            match &mut *topology_index {
                Some(_) => {}
                slot @ None => {
                    *slot = Some(TopologyIndex::new(ir));
                }
            }
        }
        let mut material_sources = materials.iter().enumerate();
        while material_sources.len() != 0 {
            let Some((index, material)) =
                ctx.next_charged(&mut material_sources, "FCStd GUI shape material sources")?
            else {
                break;
            };
            let appearance_id = shape_material_appearance_id(ctx, &provider_key, index)?;
            plan.appearance_storage.with_storage(|| {
                ctx.reserve_vec(&mut plan.appearances, 1, "FCStd GUI planned appearances")
            })?;
            let appearance =
                material_appearance(ctx, appearance_id, &provider.name, index, material)?;
            if materials.len() == 1 {
                let mut bodies = body_ids.iter().enumerate();
                while bodies.len() != 0 {
                    let Some((body_index, body)) =
                        ctx.next_charged(&mut bodies, "FCStd GUI material body bindings")?
                    else {
                        break;
                    };
                    push_body_update(
                        ctx,
                        plan,
                        body,
                        Assignment::Keep,
                        decode_color(material.diffuse, Some(material.transparency.get())).map(Some),
                    )?;
                    plan.binding_storage.with_storage(|| {
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
                        appearance: appearance
                            .id
                            .try_clone_for_decode(ctx, "FCStd GUI binding appearance identity")?,
                        source_entity_id: Some(
                            ctx.copy_retained_text(object_id, "FCStd GUI binding source identity")?,
                        ),
                        object_type: Some("ViewProvider ShapeAppearance".to_owned()),
                        visible: None,
                        channels: BTreeMap::new(),
                    });
                }
            } else if let (Some(group), Some(topology_index)) = (group, topology_index.as_mut()) {
                bind_material_faces(
                    ctx,
                    topology_index,
                    plan,
                    group,
                    index,
                    &appearance.id,
                    (&provider_key, object_id),
                )?;
            }
            plan.appearances.push(appearance);
        }
    }
    drop(appearance_properties);
    drop(appearance_property_storage);
    drop(binding_counts);
    drop(binding_count_storage);
    Ok(())
}

fn displayed_shape_bodies<'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    topology_index: &mut TopologyIndex<'ir, 'ctx>,
    object_id: &str,
    shape_index: &mut ShapeIndex<'_, 'ctx>,
) -> Result<Vec<&'ir cadmpeg_ir::ids::BodyId>, CodecError> {
    let Some(payload) = displayed_shape_payload(ctx, object_id, shape_index)? else {
        return Ok(Vec::new());
    };
    topology_index.ensure_bodies(ctx, std::iter::once(payload.id.as_str()))?;
    select_shape_bodies(
        ctx,
        &topology_index.bodies,
        std::iter::once(payload.id.as_str()),
    )
}

fn select_shape_bodies<'a, 'ir, I>(
    ctx: &DecodeContext<'_>,
    bodies: &BTreeMap<&str, Vec<(usize, &'ir cadmpeg_ir::ids::BodyId)>>,
    payload_ids: I,
) -> Result<Vec<&'ir cadmpeg_ir::ids::BodyId>, CodecError>
where
    I: IntoIterator<Item = &'a str>,
    I::IntoIter: std::iter::ExactSizeIterator,
{
    let mut body_ids = Vec::new();
    let mut payload_ids = payload_ids.into_iter();
    while payload_ids.len() != 0 {
        let Some(payload_id) =
            ctx.next_charged(&mut payload_ids, "FCStd GUI displayed payload identities")?
        else {
            break;
        };
        let payload_key = crate::native::id_key_charged(ctx, payload_id, "FCStd GUI identity key")?;
        let owned = ctx
            .get_btree_map(
                bodies,
                payload_key,
                "FCStd GUI displayed body payload lookup",
            )?
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut body_candidates = owned.iter();
        while body_candidates.len() != 0 {
            let Some((_, body)) =
                ctx.next_charged(&mut body_candidates, "FCStd GUI displayed body candidates")?
            else {
                break;
            };
            ctx.push_vec(&mut body_ids, *body, "FCStd GUI displayed shape bodies")?;
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

fn displayed_shape_group<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    object_id: &str,
    shape_index: &mut ShapeIndex<'a, 'ctx>,
    indexed_name: &'static str,
) -> Result<Option<&'a ElementMapGroup>, CodecError> {
    let Some(payload) = displayed_shape_payload(ctx, object_id, shape_index)? else {
        return Ok(None);
    };
    shape_index.ensure_maps(ctx)?;
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
    let matches_requested_group = |group: &ElementMapGroup| match indexed_name {
        "Face" => group.indexed_name == "Face",
        "Edge" => group.indexed_name == "Edge",
        "Vertex" => group.indexed_name == "Vertex",
        _ => group.indexed_name == indexed_name,
    };
    let Some(group) = ctx.find_by(
        &mut groups,
        |group| Ok(matches_requested_group(group)),
        "FCStd GUI element group search",
    )?
    else {
        return Ok(None);
    };
    if ctx
        .find_by(
            &mut groups,
            |group| Ok(matches_requested_group(group)),
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
        schema: Some("FCStd ShapeAppearance".to_owned()),
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

fn bind_material_faces<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    topology_index: &mut TopologyIndex<'_, 'ctx>,
    plan: &mut AppearancePlan<'_>,
    group: &ElementMapGroup,
    material_index: usize,
    appearance_id: &AppearanceId,
    provider: (&IdentityKey, &str),
) -> Result<(), CodecError> {
    topology_index.ensure_faces(ctx)?;
    let (provider_key, object_id) = provider;
    let mut bound_storage = ctx.reserve_scoped(0, "FCStd GUI material face identities")?;
    let mut bound = HashSet::new();
    let mut names = group.names[material_index + 1].iter();
    while names.len() != 0 {
        let Some(name) = ctx.next_charged(&mut names, "FCStd GUI material face names")? else {
            break;
        };
        let mut topology_ids = name.topology_ids.iter();
        while topology_ids.len() != 0 {
            let Some(topology_id) = ctx.next_charged(
                &mut topology_ids,
                "FCStd GUI material face topology identities",
            )?
            else {
                break;
            };
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
            plan.binding_storage.with_storage(|| {
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
                object_type: Some("ViewProvider ShapeAppearance".to_owned()),
                visible: None,
                channels: {
                    let mut channels = BTreeMap::new();
                    ctx.insert_btree_map(
                        &mut channels,
                        cadmpeg_core::nonblank_literal!("precedence"),
                        "face_over_object".to_owned(),
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

fn transfer_topology_colors<'source, 'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    plan: &mut AppearancePlan<'_>,
    request: TopologyColorRequest<'_>,
    sources: &GuiSources<'source, '_>,
    ir: &'ir CadIr,
    shape_index: &mut Option<ShapeIndex<'source, 'ctx>>,
    topology_index: &mut Option<TopologyIndex<'ir, 'ctx>>,
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
    let count = validate_color_list_layout(ctx, view, entry_name)?;
    let shape_index = match &mut *shape_index {
        Some(index) => index,
        slot @ None => slot.insert(ShapeIndex::new(
            ctx,
            sources.properties,
            sources.payloads,
            sources.element_maps,
        )?),
    };
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
    let topology_index = match &mut *topology_index {
        Some(index) => index,
        slot @ None => slot.insert(TopologyIndex::new(ir)),
    };
    match kind {
        TopologyColorKind::Face => topology_index.ensure_faces(ctx)?,
        TopologyColorKind::Edge => topology_index.ensure_edges(ctx)?,
        TopologyColorKind::Vertex => topology_index.ensure_vertices(ctx)?,
    }
    let (color_storage, colors) = ctx
        .with_scoped_storage("FCStd GUI topology color lookup", || {
            parse_color_list(ctx, view, entry_name, requires_alpha_conversion)
        })
        .map(|(colors, storage)| (storage, colors))?;
    let mut colors = colors.into_iter().enumerate();
    while colors.len() != 0 {
        let Some((index, packed)) =
            ctx.next_charged(&mut colors, "FCStd GUI topology color sources")?
        else {
            break;
        };
        let (_id_storage, appearance_id) = ctx
            .with_scoped_storage("FCStd GUI appearance identity storage", || {
                topology_appearance_id(ctx, kind, provider_key, index)
            })
            .map(|(id, storage)| (storage, id))?;
        let name_groups = if count == 1 {
            group.names.as_slice()
        } else {
            &group.names[index + 1..index + 2]
        };
        let mut emitted_appearance = false;
        let mut bound_storage = ctx.reserve_scoped(0, "FCStd GUI colored topology identities")?;
        let mut bound_topology = HashSet::new();
        let mut names_groups = name_groups.iter();
        while names_groups.len() != 0 {
            let Some(names) =
                ctx.next_charged(&mut names_groups, "FCStd GUI colored topology groups")?
            else {
                break;
            };
            let mut names = names.iter();
            while names.len() != 0 {
                let Some(name) =
                    ctx.next_charged(&mut names, "FCStd GUI colored topology names")?
                else {
                    break;
                };
                let mut topology_ids = name.topology_ids.iter();
                while topology_ids.len() != 0 {
                    let Some(topology_id) = ctx
                        .next_charged(&mut topology_ids, "FCStd GUI colored topology references")?
                    else {
                        break;
                    };
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
                        plan.appearance_storage.with_storage(|| {
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
                    let topology_key = crate::native::id_key_charged(
                        ctx,
                        topology_id.as_str(),
                        "FCStd GUI identity key",
                    )?;
                    let kind_key = topology_binding_kind(kind);
                    plan.binding_storage.with_storage(|| {
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
    drop(colors);
    drop(color_storage);
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
        displayed_shape_bodies, displayed_shape_group, displayed_shape_payload,
        select_shape_bodies, ShapeIndex,
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
    fn absent_body_channels_clear_selected_visibility_and_color() {
        let mut ir = shape_ir();
        ir.model.bodies[0].visible = Some(true);
        ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color::from_rgba8(1, 2, 3, 255));
        let object = crate::native::ObjectRecord {
            identity: crate::native::object_identity::ObjectIdentity::try_new(
                "fcstd:native:object#P".into(),
                "P".into(),
            )
            .expect("object identity"),
            type_name: "Part::Feature".into(),
            persistent_id: None,
            view_type: None,
            attributes: std::collections::BTreeMap::new(),
            dependencies: Vec::new(),
            dependency_allow_partial: None,
            order: 0,
            data: None,
        };
        let mut property = shape_property("property");
        property.owner = "fcstd:native:object#P".into();
        let properties = [property];
        let payloads = [shape_payload("payload", "property")];
        let entries = std::collections::BTreeMap::new();
        let sources = super::GuiSources {
            entries: &entries,
            objects: std::slice::from_ref(&object),
            properties: &properties,
            payloads: &payloads,
            element_maps: &[],
            requires_alpha_conversion: false,
        };
        let text = r#"<Document><Camera settings=""/><ViewProviderData Count="1"><ViewProvider name="P"><Properties Count="0"/></ViewProvider></ViewProviderData></Document>"#;
        let xml = roxmltree::Document::parse(text).expect("GUI XML");
        crate::test_support::with_service_context(text.as_bytes(), |ctx| {
            let (_, plan) = super::transfer_schema_one(ctx, &ir, text, &xml, None, None, &sources)
                .expect("GUI transfer");
            plan.apply(ctx, &mut ir).expect("body assignments");
            assert_eq!(ir.model.bodies[0].visible, None);
            assert_eq!(ir.model.bodies[0].color, None);
        });
    }

    fn material_graph() -> super::Graph<'static> {
        super::Graph {
            providers: vec![crate::native::GuiViewProviderRecord {
                id: "fcstd:native:gui-view-provider#P".into(),
                object: Some(
                    cadmpeg_core::text::NonBlankString::try_from("object")
                        .expect("object identity"),
                ),
                name: "P".into(),
                expanded: None,
                order: 0,
                raw_xml: "<ViewProvider/>".into(),
            }],
            properties: vec![crate::native::GuiPropertyRecord {
                id: "material-property".into(),
                owner: "fcstd:native:gui-view-provider#P".into(),
                name: "ShapeAppearance".into(),
                type_name: "App::PropertyMaterialList".into(),
                status: None,
                order: 0,
                values: Vec::new(),
                side_entries: Vec::new(),
                xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
                    .expect("property XML"),
            }],
            ..Default::default()
        }
    }

    fn material() -> super::GuiMaterial<'static> {
        let zero = cadmpeg_ir::scalar::FiniteBinary32::new(0.0).expect("finite zero");
        super::GuiMaterial {
            ambient: 0,
            diffuse: 0x1122_33ff,
            specular: 0,
            emissive: 0,
            shininess: zero,
            transparency: zero,
            uuid: "",
        }
    }

    fn assert_single_material_ambiguity(maps: &[ElementMapRecord], expected: &str) {
        let ir = shape_ir();
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        let entries = std::collections::BTreeMap::new();
        let graph = material_graph();
        let materials = std::collections::HashMap::from([("material-property", vec![material()])]);
        let sources = super::GuiSources {
            entries: &entries,
            objects: &[],
            properties: &properties,
            payloads: &payloads,
            element_maps: maps,
            requires_alpha_conversion: false,
        };
        crate::test_support::with_service_context(&[], |ctx| {
            let mut plan = super::AppearancePlan::new(ctx).expect("plan storage");
            let error = super::transfer_shape_appearances(
                ctx,
                &mut plan,
                &graph,
                &materials,
                &sources,
                &ir,
                &mut None,
                &mut None,
                &mut Vec::new(),
            )
            .expect_err("ambiguous single-material shape");
            assert!(
                matches!(error, cadmpeg_core::CodecError::Malformed(message) if message == expected)
            );
            assert!(plan.appearances.is_empty());
            assert!(plan.bindings.is_empty());
            assert!(plan.remove_appearances.is_empty());
        });
    }

    #[test]
    fn single_material_rejects_duplicate_face_groups() {
        assert_single_material_ambiguity(
            &[element_map("property", vec![group("Face"), group("Face")])],
            "Shape property property has multiple Face groups",
        );
    }

    #[test]
    fn single_material_rejects_duplicate_element_maps() {
        let map = element_map("property", vec![group("Face")]);
        assert_single_material_ambiguity(
            &[
                map.clone(),
                ElementMapRecord {
                    id: "map-2".into(),
                    ..map
                },
            ],
            "Shape property property has multiple element maps",
        );
    }

    #[test]
    fn single_material_removes_legacy_bindings_without_building_faces() {
        let mut ir = shape_ir();
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        let maps = [element_map("property", vec![group("Face")])];
        let entries = std::collections::BTreeMap::new();
        let graph = material_graph();
        let materials = std::collections::HashMap::from([("material-property", vec![material()])]);
        let sources = super::GuiSources {
            entries: &entries,
            objects: &[],
            properties: &properties,
            payloads: &payloads,
            element_maps: &maps,
            requires_alpha_conversion: false,
        };
        crate::test_support::with_service_context(&[], |ctx| {
            let mut plan = super::AppearancePlan::new(ctx).expect("plan storage");
            let key = super::provider_identity_key(ctx, "P").expect("provider key");
            let legacy_id = super::object_appearance_id(ctx, &key).expect("legacy identity");
            plan.appearances.push(
                super::material_appearance(ctx, legacy_id.clone(), "P", 0, &material())
                    .expect("legacy appearance"),
            );
            for index in 0..2 {
                plan.bindings
                    .push(cadmpeg_ir::appearance::AppearanceBinding {
                        id: cadmpeg_ir::ids::AppearanceBindingId::mint(format!(
                            "fcstd:appearance:binding#legacy:{index}"
                        ))
                        .expect("binding identity"),
                        target: cadmpeg_ir::appearance::AppearanceTarget::Body(
                            ir.model.bodies[0].id.clone(),
                        ),
                        appearance: legacy_id.clone(),
                        source_entity_id: None,
                        object_type: None,
                        visible: None,
                        channels: std::collections::BTreeMap::new(),
                    });
            }
            let mut shape = None;
            let mut topology = None;
            super::transfer_shape_appearances(
                ctx,
                &mut plan,
                &graph,
                &materials,
                &sources,
                &ir,
                &mut shape,
                &mut topology,
                &mut Vec::new(),
            )
            .expect("single-material transfer");
            assert_eq!(plan.removed_binding_count, 2);
            assert!(!topology.as_ref().expect("body demand").face_built);
            assert!(!topology.as_ref().expect("body demand").edge_built);
            assert!(!topology.as_ref().expect("body demand").vertex_built);
            drop(shape);
            drop(topology);
            plan.apply(ctx, &mut ir).expect("material replacement");
            assert_eq!(ir.model.appearances.len(), 1);
            assert_eq!(
                ir.model.appearances[0].id.as_str(),
                "fcstd:appearance:shape-material#P:1"
            );
            assert_eq!(ir.model.appearance_bindings.len(), 1);
            assert_eq!(
                ir.model.appearance_bindings[0].appearance,
                ir.model.appearances[0].id
            );
            assert_eq!(ir.model.bodies[0].color, ir.model.appearances[0].base_color);
        });
    }

    #[test]
    fn source_indexes_build_only_for_requested_channels() {
        crate::test_support::with_service_context(&[], |ctx| {
            let mut ir = cadmpeg_ir::CadIr::empty();
            let edge_id =
                cadmpeg_ir::ids::EdgeId::mint("fcstd:model:edge#edge").expect("edge identity");
            let vertex_id = cadmpeg_ir::ids::VertexId::mint("fcstd:model:vertex#vertex")
                .expect("vertex identity");
            ir.model.edges.push(cadmpeg_ir::topology::Edge {
                id: edge_id.clone(),
                carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
                start: vertex_id.clone(),
                end: vertex_id,
                tolerance: None,
            });
            let mut topology = TopologyIndex::new(&ir);
            assert!(!topology.face_built);
            assert!(!topology.edge_built);
            assert!(!topology.vertex_built);
            assert!(!topology.body_candidates_built);
            topology.ensure_edges(ctx).expect("edge index");
            assert!(!topology.face_built);
            assert!(topology.edge_built);
            assert!(!topology.vertex_built);
            assert!(!topology.body_candidates_built);
            assert_eq!(
                ctx.get_btree_map(
                    &topology.edges,
                    edge_id.as_str(),
                    "test edge identity lookup"
                )
                .expect("edge lookup"),
                Some(&&edge_id)
            );

            let property = shape_property("property");
            let payload = shape_payload("payload", "property");
            let map = element_map("property", vec![group("Face")]);
            let mut shape = ShapeIndex::new(
                ctx,
                std::slice::from_ref(&property),
                std::slice::from_ref(&payload),
                std::slice::from_ref(&map),
            )
            .expect("shape index");
            assert!(!shape.maps_built);
            assert!(displayed_shape_payload(ctx, "object", &shape)
                .expect("payload lookup")
                .is_some());
            assert!(!shape.maps_built);
            assert!(displayed_shape_group(ctx, "object", &mut shape, "Face")
                .expect("group lookup")
                .is_some());
            assert!(shape.maps_built);
        });
    }

    #[test]
    fn topology_color_without_a_displayed_shape_does_not_build_topology_index() {
        crate::test_support::with_service_context(&[], |ctx| {
            let color_bytes = [0_u8; 4];
            let entries = std::collections::BTreeMap::from([(
                "colors".to_owned(),
                cadmpeg_core::decode::View::over_retained(&color_bytes),
            )]);
            let sources = super::GuiSources {
                entries: &entries,
                objects: &[],
                properties: &[],
                payloads: &[],
                element_maps: &[],
                requires_alpha_conversion: false,
            };
            let ir = cadmpeg_ir::CadIr::empty();
            let mut shape_index: Option<super::ShapeIndex<'_, '_>> = None;
            let mut topology_index: Option<super::TopologyIndex<'_, '_>> = None;
            let mut plan = super::AppearancePlan::new(ctx).expect("plan storage");
            super::transfer_topology_colors(
                ctx,
                &mut plan,
                super::TopologyColorRequest {
                    provider_name: "Model",
                    provider_key: &cadmpeg_ir::identity_key!("Model"),
                    object_id: "fcstd:native:object#Model",
                    entry_name: "colors",
                    kind: super::TopologyColorKind::Face,
                    provenance: cadmpeg_ir::SourceProvenance::in_stream(
                        "fcstd",
                        cadmpeg_ir::stream_name!("GuiDocument.xml"),
                        17,
                    ),
                },
                &sources,
                &ir,
                &mut shape_index,
                &mut topology_index,
                &mut Vec::new(),
            )
            .expect("empty color list without a displayed shape");
            assert!(shape_index.is_some());
            assert!(topology_index.is_none());
        });
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
                let mut topology = TopologyIndex::new(&ir);
                let mut shape = ShapeIndex::new(ctx, &properties, &payloads, &[])?;
                displayed_shape_bodies(ctx, &mut topology, "object", &mut shape)
            },
        );
    }

    #[test]
    fn displayed_shape_body_update_identity_refuses_at_materialized_limit() {
        let ir = shape_ir();
        let properties = [shape_property("property")];
        let payloads = [shape_payload("payload", "property")];
        crate::test_support::materialized_refusal_at("FCStd GUI body update identity", |ctx| {
            let mut topology = TopologyIndex::new(&ir);
            let mut shape = ShapeIndex::new(ctx, &properties, &payloads, &[])?;
            let bodies = displayed_shape_bodies(ctx, &mut topology, "object", &mut shape)?;
            let mut plan = super::AppearancePlan::new(ctx)?;
            super::push_body_update(ctx, &mut plan, bodies[0], super::Assignment::Keep, Ok(None))
        });
    }

    #[test]
    fn repeated_payload_body_selection_refuses_at_second_slot() {
        let ir = shape_ir();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let index_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (index_ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &index_policy)
                .expect("index fixture context");
        let mut index = TopologyIndex::new(&ir);
        index
            .ensure_bodies(&index_ctx, std::iter::once("payload"))
            .expect("body index is admitted");
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let error = select_shape_bodies(&ctx, &index.bodies, ["payload", "payload"])
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
        let body_ids = select_shape_bodies(&ctx, &index.bodies, ["payload", "payload"])
            .expect("service policy admits both source occurrences");
        assert_eq!(body_ids, [&ir.model.bodies[0].id, &ir.model.bodies[0].id]);
    }

    #[test]
    fn shape_property_group_refusal_stops_before_unvisited_suffix() {
        let first = shape_property("property");
        let first_properties = [first.clone()];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let work_cap = {
            let policy = cadmpeg_core::decode::DecodePolicy::service();
            let (oracle_ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is within policy");
            let _oracle_index = ShapeIndex::new(&oracle_ctx, &first_properties, &[], &[])
                .expect("one Shape property fits service policy");
            let cadmpeg_core::CodecError::ResourceLimit(work_limit) = oracle_ctx
                .charge_work(u64::MAX, "test work-prefix oracle")
                .expect_err("work-prefix oracle must exceed service policy")
            else {
                panic!("expected a work-unit refusal");
            };
            assert_eq!(
                work_limit.dimension,
                cadmpeg_core::decode::ResourceDimension::WorkUnits
            );
            work_limit.used
        };
        let suffix_len = usize::try_from(work_cap)
            .expect("work prefix fits usize")
            .saturating_mul(16)
            .saturating_add(1_024);
        assert!(u64::try_from(suffix_len).expect("suffix length fits u64") > work_cap);

        let mut properties = Vec::with_capacity(suffix_len.saturating_add(1));
        properties.push(first);
        for index in 0..suffix_len {
            let mut unrelated = shape_property("suffix");
            unrelated.id = format!("suffix-{index}");
            unrelated.name = "Other".into();
            properties.push(unrelated);
        }

        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_work_units = work_cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        let error = match ShapeIndex::new(&ctx, &properties, &[], &[]) {
            Ok(index) => {
                drop(index);
                panic!("first Shape group must refuse at zero collection allowance");
            }
            Err(error) => error,
        };
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(ref failure)
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && failure.operation == "FCStd GUI Shape property owners"
        ));
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
        let duplicate_properties = [property.clone(), duplicate_property];
        let mut shape_index = ShapeIndex::new(
            &ctx,
            &duplicate_properties,
            std::slice::from_ref(&payload),
            std::slice::from_ref(&map),
        )
        .expect("shape index");
        assert!(matches!(
            displayed_shape_group(&ctx, "object", &mut shape_index, "Face"),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_payload = ShapePayloadRecord {
            id: "payload-2".into(),
            ..payload.clone()
        };
        let duplicate_payloads = [payload.clone(), duplicate_payload];
        let mut shape_index = ShapeIndex::new(
            &ctx,
            std::slice::from_ref(&property),
            &duplicate_payloads,
            std::slice::from_ref(&map),
        )
        .expect("shape index");
        assert!(matches!(
            displayed_shape_group(&ctx, "object", &mut shape_index, "Face"),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_map = ElementMapRecord {
            id: "map-2".into(),
            ..map.clone()
        };
        let duplicate_maps = [map, duplicate_map];
        let mut shape_index = ShapeIndex::new(
            &ctx,
            std::slice::from_ref(&property),
            std::slice::from_ref(&payload),
            &duplicate_maps,
        )
        .expect("shape index");
        assert!(matches!(
            displayed_shape_group(&ctx, "object", &mut shape_index, "Face"),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_group = element_map("property", vec![group("Face"), group("Face")]);
        let duplicate_group_properties = [property];
        let duplicate_group_payloads = [payload];
        let duplicate_group_maps = [duplicate_group];
        let mut shape_index = ShapeIndex::new(
            &ctx,
            &duplicate_group_properties,
            &duplicate_group_payloads,
            &duplicate_group_maps,
        )
        .expect("shape index");
        assert!(matches!(
            displayed_shape_group(&ctx, "object", &mut shape_index, "Face"),
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
                let mut shape_index = ShapeIndex::new(ctx, &properties, &[], &[])?;
                displayed_shape_group(ctx, "object", &mut shape_index, "Face")
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
                let mut shape_index = ShapeIndex::new(ctx, &properties, &payloads, &[])?;
                displayed_shape_group(ctx, "object", &mut shape_index, "Face")
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
                let mut shape_index = ShapeIndex::new(ctx, &properties, &payloads, &maps)?;
                displayed_shape_group(ctx, "object", &mut shape_index, "Face")
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
                let mut shape_index = ShapeIndex::new(ctx, &properties, &payloads, &maps)?;
                displayed_shape_group(ctx, "object", &mut shape_index, "Face")
            },
        );
    }
}

#[cfg(test)]
pub(crate) mod tests;
