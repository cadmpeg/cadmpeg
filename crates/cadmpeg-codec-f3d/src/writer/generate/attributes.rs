// SPDX-License-Identifier: Apache-2.0
//! Source-less attribute, link, and tag encoders.

use std::collections::HashMap;

use crate::native::F3dNative;
use crate::records::{
    recipes::CreationTimestamp,
    sketch_links::{
        PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
        SKETCH_LINK_SENSE_UNCONSTRAINED,
    },
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::{Body, Coedge, Color, Edge, Face};

use super::native_bytes::{
    native_f64, native_i64, native_ident, native_record_index, native_ref, native_string,
    native_subident,
};

struct IndexedTimestamp<'a> {
    source: &'a CreationTimestamp,
    ordinal: Option<usize>,
}

pub(super) struct AttributeIndex<'a> {
    creation_timestamps: &'a [CreationTimestamp],
    extra_owner_attributes: Vec<&'a cadmpeg_ir::attributes::SourceAttribute>,
    body_group_ordinals: HashMap<String, usize>,
    face_group_ordinals: HashMap<String, usize>,
    edge_group_ordinals: HashMap<String, usize>,
    sketch_ordinals: HashMap<String, usize>,
    body_links: HashMap<String, Vec<&'a PersistentDesignLink>>,
    face_tags: HashMap<String, Vec<&'a PersistentSubentityTag>>,
    edge_tags: HashMap<String, Vec<&'a PersistentSubentityTag>>,
    body_timestamps: HashMap<String, IndexedTimestamp<'a>>,
    face_timestamps: HashMap<String, IndexedTimestamp<'a>>,
    edge_timestamps: HashMap<String, IndexedTimestamp<'a>>,
    coedge_timestamps: HashMap<String, IndexedTimestamp<'a>>,
    vertex_timestamps: HashMap<String, IndexedTimestamp<'a>>,
    coedge_sketch_links: HashMap<String, &'a SketchCurveLink>,
    body_keys: HashMap<&'a str, &'a cadmpeg_asm::brep::records::BodyNativeKey>,
}

/// Record-table starts of the face and coedge sections, which only a solid
/// body target writes.
#[derive(Clone, Copy)]
pub(super) struct SurfaceOwnerStarts {
    pub(super) face: i64,
    pub(super) coedge: i64,
}

/// Record-table starts used to write the owner field of generated attributes.
#[derive(Clone, Copy)]
pub(super) struct AttributeOwnerStarts {
    pub(super) body: i64,
    /// Present only when the target has a face/coedge section.
    pub(super) surface: Option<SurfaceOwnerStarts>,
    pub(super) edge: i64,
    pub(super) vertex: i64,
}

impl<'a> AttributeIndex<'a> {
    pub(super) fn new(target: &'a CadIr, native: &'a F3dNative) -> Self {
        let mut body_links: HashMap<_, Vec<_>> = HashMap::new();
        let mut body_keys = HashMap::new();
        for key in &native.body_native_keys {
            if let Some(body) = target
                .model
                .bodies
                .iter()
                .find(|body| key.owns_native_ref(body.native_ref.as_deref()))
            {
                body_keys.entry(body.id.as_str()).or_insert(key);
            }
        }
        for link in &native.persistent_design_links {
            if let Some(body) = crate::brep::link_neutral_id(
                &target.model.bodies,
                &target.model.faces,
                &target.model.edges,
                &target.model.vertices,
                &target.model.coedges,
                &link.target,
                link.target_coedge,
            ) {
                body_links.entry(body).or_default().push(link);
            }
        }
        for group in body_links.values_mut() {
            group.sort_by_key(|link| link.ordinal);
        }
        let mut face_tags: HashMap<_, Vec<_>> = HashMap::new();
        let mut edge_tags: HashMap<_, Vec<_>> = HashMap::new();
        for tag in &native.persistent_subentity_tags {
            let neutral = crate::brep::link_neutral_id(
                &target.model.bodies,
                &target.model.faces,
                &target.model.edges,
                &target.model.vertices,
                &target.model.coedges,
                &tag.target,
                tag.target_coedge,
            );
            // Tags are derived for faces and edges only; anything else stays unindexed.
            if let Some(id) = neutral {
                if target.model.faces.iter().any(|face| face.id.as_str() == id) {
                    face_tags.entry(id).or_default().push(tag);
                } else if target.model.edges.iter().any(|edge| edge.id.as_str() == id) {
                    edge_tags.entry(id).or_default().push(tag);
                }
            }
        }
        for group in face_tags.values_mut().chain(edge_tags.values_mut()) {
            group.sort_by_key(|tag| tag.ordinal);
        }
        let mut body_timestamps = HashMap::new();
        let mut face_timestamps = HashMap::new();
        let mut edge_timestamps = HashMap::new();
        let mut coedge_timestamps = HashMap::new();
        let mut vertex_timestamps = HashMap::new();
        for timestamp in &native.creation_timestamps {
            let neutral = crate::brep::link_neutral_id(
                &target.model.bodies,
                &target.model.faces,
                &target.model.edges,
                &target.model.vertices,
                &target.model.coedges,
                &timestamp.target,
                timestamp.target_coedge,
            );
            let Some(id) = neutral else {
                continue;
            };
            let entry = if target
                .model
                .bodies
                .iter()
                .any(|body| body.id.as_str() == id)
            {
                &mut body_timestamps
            } else if target.model.faces.iter().any(|face| face.id.as_str() == id) {
                &mut face_timestamps
            } else if target.model.edges.iter().any(|edge| edge.id.as_str() == id) {
                &mut edge_timestamps
            } else if target
                .model
                .coedges
                .iter()
                .any(|coedge| coedge.id.as_str() == id)
            {
                &mut coedge_timestamps
            } else if target
                .model
                .vertices
                .iter()
                .any(|vertex| vertex.id.as_str() == id)
            {
                &mut vertex_timestamps
            } else {
                continue;
            };
            entry.entry(id).or_insert(IndexedTimestamp {
                source: timestamp,
                ordinal: None,
            });
        }
        let mut coedge_sketch_links = HashMap::new();
        for link in &native.sketch_curve_links {
            if let Some(id) = crate::brep::link_neutral_id(
                &target.model.bodies,
                &target.model.faces,
                &target.model.edges,
                &target.model.vertices,
                &target.model.coedges,
                &link.target,
                link.target_coedge,
            )
            .filter(|id| {
                target
                    .model
                    .coedges
                    .iter()
                    .any(|coedge| coedge.id.as_str() == id)
            }) {
                coedge_sketch_links.entry(id).or_insert(link);
            }
        }
        let mut split_ordinal = 0;
        macro_rules! split_timestamp_ordinals {
            ($items:expr, $timestamps:expr) => {
                for item in $items {
                    if let Some(timestamp) = $timestamps.get_mut(item.id.as_str()) {
                        timestamp.ordinal = Some(split_ordinal);
                        split_ordinal += 1;
                    }
                }
            };
        }
        split_timestamp_ordinals!(&target.model.bodies, body_timestamps);
        split_timestamp_ordinals!(&target.model.faces, face_timestamps);
        split_timestamp_ordinals!(&target.model.edges, edge_timestamps);
        split_timestamp_ordinals!(&target.model.coedges, coedge_timestamps);
        split_timestamp_ordinals!(&target.model.vertices, vertex_timestamps);
        let body_group_ordinals = target
            .model
            .bodies
            .iter()
            .filter(|body| body_links.contains_key(body.id.as_str()))
            .enumerate()
            .map(|(ordinal, body)| (body.id.as_str().to_owned(), ordinal))
            .collect();
        let face_group_ordinals = target
            .model
            .faces
            .iter()
            .filter(|face| face_tags.contains_key(face.id.as_str()))
            .enumerate()
            .map(|(ordinal, face)| (face.id.as_str().to_owned(), ordinal))
            .collect();
        let edge_group_ordinals = target
            .model
            .edges
            .iter()
            .filter(|edge| edge_tags.contains_key(edge.id.as_str()))
            .enumerate()
            .map(|(ordinal, edge)| (edge.id.as_str().to_owned(), ordinal))
            .collect();
        let sketch_ordinals = target
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge_sketch_links.contains_key(coedge.id.as_str()))
            .enumerate()
            .map(|(ordinal, coedge)| (coedge.id.as_str().to_owned(), ordinal))
            .collect();
        let bodies = target
            .model
            .bodies
            .iter()
            .map(|body| (&body.id, body))
            .collect::<HashMap<_, _>>();
        let faces = target
            .model
            .faces
            .iter()
            .map(|face| (&face.id, face))
            .collect::<HashMap<_, _>>();
        let edges = target
            .model
            .edges
            .iter()
            .map(|edge| &edge.id)
            .collect::<std::collections::HashSet<_>>();
        let vertices = target
            .model
            .vertices
            .iter()
            .map(|vertex| &vertex.id)
            .collect::<std::collections::HashSet<_>>();
        let coedges = target
            .model
            .coedges
            .iter()
            .map(|coedge| &coedge.id)
            .collect::<std::collections::HashSet<_>>();
        let mut attribute_counts = std::collections::BTreeMap::new();
        for attribute in &target.model.attributes {
            *attribute_counts.entry(&attribute.target).or_insert(0usize) += 1;
        }
        let extra_owner_attributes = target.model.attributes.iter().filter(|attribute| {
            let owner_is_free = match &attribute.target {
                AttributeTarget::Body(id) => bodies.contains_key(id)
                    && !body_links.contains_key(id.as_str())
                    && !body_timestamps.contains_key(id.as_str()),
                AttributeTarget::Face(id) => faces.contains_key(id)
                    && !face_tags.contains_key(id.as_str())
                    && !face_timestamps.contains_key(id.as_str()),
                AttributeTarget::Edge(id) => edges.contains(id)
                    && !edge_tags.contains_key(id.as_str()) && !edge_timestamps.contains_key(id.as_str()),
                AttributeTarget::Vertex(id) => vertices.contains(id) && !vertex_timestamps.contains_key(id.as_str()),
                AttributeTarget::Coedge(id) => coedges.contains(id)
                    && !coedge_sketch_links.contains_key(id.as_str()) && !coedge_timestamps.contains_key(id.as_str()),
                _ => false,
            };
            owner_is_free && attribute_counts.get(&attribute.target) == Some(&1)
                && compact_custom_shape(attribute)
                && attribute.values.iter().all(|value| match value {
                    Some(cadmpeg_ir::attributes::AttributeValue::String(text)) => u8::try_from(text.len()).is_ok(),
                    _ => true,
                })
                && matches!(attribute.values.first(), Some(Some(cadmpeg_ir::attributes::AttributeValue::Reference(reference))) if reference == "f3d:brep:entity#-1")
        }).collect();
        Self {
            creation_timestamps: &native.creation_timestamps,
            extra_owner_attributes,
            body_group_ordinals,
            face_group_ordinals,
            edge_group_ordinals,
            sketch_ordinals,
            body_links,
            face_tags,
            edge_tags,
            body_timestamps,
            face_timestamps,
            edge_timestamps,
            coedge_timestamps,
            vertex_timestamps,
            coedge_sketch_links,
            body_keys,
        }
    }

    fn body_group_count(&self) -> usize {
        self.body_group_ordinals.len()
    }

    fn face_group_count(&self) -> usize {
        self.face_group_ordinals.len()
    }

    fn edge_group_count(&self) -> usize {
        self.edge_group_ordinals.len()
    }

    fn indexed_timestamp(
        &self,
        target: &cadmpeg_ir::attributes::AttributeTarget,
    ) -> Option<&IndexedTimestamp<'a>> {
        use cadmpeg_ir::attributes::AttributeTarget;
        match target {
            AttributeTarget::Body(id) => self.body_timestamps.get(id.as_str()),
            AttributeTarget::Face(id) => self.face_timestamps.get(id.as_str()),
            AttributeTarget::Edge(id) => self.edge_timestamps.get(id.as_str()),
            AttributeTarget::Coedge(id) => self.coedge_timestamps.get(id.as_str()),
            AttributeTarget::Vertex(id) => self.vertex_timestamps.get(id.as_str()),
            _ => None,
        }
    }
    fn timestamp(&self, target: &AttributeTarget) -> Option<&'a CreationTimestamp> {
        self.indexed_timestamp(target)
            .map(|timestamp| timestamp.source)
    }

    fn timestamp_ordinal(&self, target: &AttributeTarget) -> Option<usize> {
        self.indexed_timestamp(target)
            .and_then(|timestamp| timestamp.ordinal)
    }
}

pub(super) fn source_less_body_key(
    index: &AttributeIndex<'_>,
    body: &Body,
    body_ordinal: usize,
) -> Result<i64, CodecError> {
    if let Some(key) = index.body_keys.get(body.id.as_str()) {
        return key.asm_body_key.map_or(Ok(-1), |key| {
            i64::try_from(key)
                .map_err(|_| CodecError::InvalidInput("F3D ASM body key exceeds i64::MAX".into()))
        });
    }
    let key = cadmpeg_core::decode::u64_from_index(body_ordinal) + 1;
    i64::try_from(key)
        .map_err(|_| CodecError::InvalidInput("F3D ASM body key exceeds i64::MAX".into()))
}

#[derive(Clone, Copy)]
enum ColorOwner {
    Body,
    Face,
}

fn color_attribute_ref(
    model: &cadmpeg_ir::document::Model,
    ordinal: usize,
    owner: ColorOwner,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    let preceding = match owner {
        ColorOwner::Body => model.bodies[..ordinal]
            .iter()
            .filter(|body| body.color.is_some())
            .count(),
        ColorOwner::Face => {
            model
                .bodies
                .iter()
                .filter(|body| body.color.is_some())
                .count()
                + model.faces[..ordinal]
                    .iter()
                    .filter(|face| face.color.is_some())
                    .count()
        }
    };
    native_record_index(attribute_start, preceding)
}

fn persistent_links<'i, 'n>(
    index: &'i AttributeIndex<'n>,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> &'i [&'n PersistentDesignLink] {
    if let cadmpeg_ir::attributes::AttributeTarget::Body(id) = entity {
        index.body_links.get(id.as_str()).map_or(&[], Vec::as_slice)
    } else {
        &[]
    }
}

fn persistent_subentity_tags<'i, 'n>(
    index: &'i AttributeIndex<'n>,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> &'i [&'n PersistentSubentityTag] {
    match entity {
        cadmpeg_ir::attributes::AttributeTarget::Face(id) => {
            index.face_tags.get(id.as_str()).map_or(&[], Vec::as_slice)
        }
        cadmpeg_ir::attributes::AttributeTarget::Edge(id) => {
            index.edge_tags.get(id.as_str()).map_or(&[], Vec::as_slice)
        }
        _ => &[],
    }
}

fn creation_timestamp<'a>(
    index: &'a AttributeIndex<'_>,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Option<&'a CreationTimestamp> {
    index.timestamp(entity)
}

fn timestamp_attribute_ordinal(
    index: &AttributeIndex<'_>,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Option<usize> {
    index.timestamp_ordinal(entity)
}

fn existing_source_less_attribute_count(target: &CadIr, index: &AttributeIndex<'_>) -> usize {
    source_less_color_count(target)
        + source_less_name_count(target)
        + index.body_group_count()
        + index.face_group_count()
        + index.edge_group_count()
        + index.coedge_sketch_links.len()
}

pub(super) fn timestamp_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    timestamp_attribute_ordinal(index, entity)
        .map(|ordinal| {
            native_record_index(
                attribute_start,
                existing_source_less_attribute_count(target, index) + ordinal,
            )
        })
        .transpose()
}

fn body_persistent_links<'i, 'n>(
    index: &'i AttributeIndex<'n>,
    body: &Body,
) -> &'i [&'n PersistentDesignLink] {
    persistent_links(
        index,
        &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
    )
}

fn face_persistent_tags<'i, 'n>(
    index: &'i AttributeIndex<'n>,
    face: &Face,
) -> &'i [&'n PersistentSubentityTag] {
    persistent_subentity_tags(
        index,
        &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
    )
}

fn edge_persistent_tags<'i, 'n>(
    index: &'i AttributeIndex<'n>,
    edge: &Edge,
) -> &'i [&'n PersistentSubentityTag] {
    persistent_subentity_tags(
        index,
        &cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone()),
    )
}

fn body_persistent_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    body: &Body,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if body_persistent_links(index, body).is_empty() {
        return Ok(None);
    }
    let ordinal = index.body_group_ordinals[body.id.as_str()];
    let color_count = source_less_color_count(target);
    native_record_index(
        attribute_start,
        color_count + source_less_name_count(target) + ordinal,
    )
    .map(Some)
}

fn body_name_attribute_ref(
    target: &CadIr,
    body: &Body,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if body.name.is_none() {
        return Ok(None);
    }
    let ordinal = target
        .model
        .bodies
        .iter()
        .take_while(|candidate| candidate.id != body.id)
        .filter(|candidate| candidate.name.is_some())
        .count();
    native_record_index(attribute_start, source_less_color_count(target) + ordinal).map(Some)
}

/// A source-less attribute owner with a color/name/tag chain.
#[derive(Clone, Copy)]
pub(super) enum AttributeOwner<'a> {
    Body(&'a Body),
    Face(&'a Face),
}

pub(super) fn owner_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    owner: AttributeOwner<'_>,
    ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    let (has_color, color_owner) = match owner {
        AttributeOwner::Body(body) => (body.color.is_some(), ColorOwner::Body),
        AttributeOwner::Face(face) => (face.color.is_some(), ColorOwner::Face),
    };
    if has_color {
        return color_attribute_ref(&target.model, ordinal, color_owner, attribute_start);
    }
    let name = match owner {
        AttributeOwner::Body(body) => body_name_attribute_ref(target, body, attribute_start),
        AttributeOwner::Face(face) => face_name_attribute_ref(target, face, attribute_start),
    }?;
    if let Some(reference) = name {
        return Ok(reference);
    }
    let persistent = match owner {
        AttributeOwner::Body(body) => {
            body_persistent_attribute_ref(target, index, body, attribute_start)
        }
        AttributeOwner::Face(face) => {
            face_persistent_attribute_ref(target, index, face, attribute_start)
        }
    }?;
    if let Some(reference) = persistent {
        return Ok(reference);
    }
    let entity = match owner {
        AttributeOwner::Body(body) => AttributeTarget::Body(body.id.clone()),
        AttributeOwner::Face(face) => AttributeTarget::Face(face.id.clone()),
    };
    if let Some(reference) = timestamp_attribute_ref(target, index, &entity, attribute_start)? {
        return Ok(reference);
    }
    Ok(extra_attribute_ref(target, index, &entity, attribute_start)?.unwrap_or(-1))
}

pub(super) fn extra_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    entity: &AttributeTarget,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    index
        .extra_owner_attributes
        .iter()
        .position(|attribute| attribute.target == *entity)
        .map(|ordinal| {
            native_record_index(
                attribute_start,
                existing_source_less_attribute_count(target, index)
                    + index.creation_timestamps.len()
                    + ordinal,
            )
        })
        .transpose()
}

fn face_persistent_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    face: &Face,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if face_persistent_tags(index, face).is_empty() {
        return Ok(None);
    }
    let ordinal = index.face_group_ordinals[face.id.as_str()];
    native_record_index(
        attribute_start,
        source_less_color_count(target)
            + source_less_name_count(target)
            + index.body_group_count()
            + ordinal,
    )
    .map(Some)
}

fn face_name_attribute_ref(
    target: &CadIr,
    face: &Face,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if face.name.is_none() {
        return Ok(None);
    }
    let ordinal = target
        .model
        .faces
        .iter()
        .take_while(|candidate| candidate.id != face.id)
        .filter(|candidate| candidate.name.is_some())
        .count();
    let body_name_count = target
        .model
        .bodies
        .iter()
        .filter(|body| body.name.is_some())
        .count();
    native_record_index(
        attribute_start,
        source_less_color_count(target) + body_name_count + ordinal,
    )
    .map(Some)
}

pub(super) fn edge_persistent_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    edge: &Edge,
    _edge_ordinal: usize,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if edge_persistent_tags(index, edge).is_empty() {
        return Ok(None);
    }
    let ordinal = index.edge_group_ordinals[edge.id.as_str()];
    native_record_index(
        attribute_start,
        source_less_color_count(target)
            + source_less_name_count(target)
            + index.body_group_count()
            + index.face_group_count()
            + ordinal,
    )
    .map(Some)
}

fn source_less_color_count(target: &CadIr) -> usize {
    target
        .model
        .bodies
        .iter()
        .filter(|body| body.color.is_some())
        .count()
        + target
            .model
            .faces
            .iter()
            .filter(|face| face.color.is_some())
            .count()
}

fn source_less_name_count(target: &CadIr) -> usize {
    target
        .model
        .bodies
        .iter()
        .filter(|body| body.name.is_some())
        .count()
        + target
            .model
            .faces
            .iter()
            .filter(|face| face.name.is_some())
            .count()
}

fn sketch_link<'a>(index: &'a AttributeIndex<'_>, coedge: &Coedge) -> Option<&'a SketchCurveLink> {
    index.coedge_sketch_links.get(coedge.id.as_str()).copied()
}

fn coedge_sketch_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    coedge: &Coedge,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if sketch_link(index, coedge).is_none() {
        return Ok(None);
    }
    let preceding = index.sketch_ordinals[coedge.id.as_str()];
    native_record_index(
        attribute_start,
        source_less_color_count(target)
            + source_less_name_count(target)
            + index.body_group_count()
            + index.face_group_count()
            + index.edge_group_count()
            + preceding,
    )
    .map(Some)
}

pub(super) fn sketch_link_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    coedge: &Coedge,
    _coedge_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if let Some(reference) = coedge_sketch_attribute_ref(target, index, coedge, attribute_start)? {
        return Ok(reference);
    }
    let entity = AttributeTarget::Coedge(coedge.id.clone());
    Ok(
        timestamp_attribute_ref(target, index, &entity, attribute_start)?
            .or(extra_attribute_ref(
                target,
                index,
                &entity,
                attribute_start,
            )?)
            .unwrap_or(-1),
    )
}

fn attribute_before_timestamp(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    entity: &AttributeTarget,
    owner_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    match entity {
        AttributeTarget::Body(id) => {
            let body = target
                .model
                .bodies
                .get(owner_ordinal)
                .filter(|body| body.id == *id)
                .ok_or_else(|| CodecError::malformed(format_args!("missing F3D body {id}")))?;
            if let Some(reference) =
                body_persistent_attribute_ref(target, index, body, attribute_start)?
            {
                return Ok(reference);
            }
            if let Some(reference) = body_name_attribute_ref(target, body, attribute_start)? {
                return Ok(reference);
            }
            if body.color.is_none() {
                Ok(-1)
            } else {
                color_attribute_ref(
                    &target.model,
                    owner_ordinal,
                    ColorOwner::Body,
                    attribute_start,
                )
            }
        }
        AttributeTarget::Face(id) => {
            let face = target
                .model
                .faces
                .get(owner_ordinal)
                .filter(|face| face.id == *id)
                .ok_or_else(|| CodecError::malformed(format_args!("missing F3D face {id}")))?;
            if let Some(reference) =
                face_persistent_attribute_ref(target, index, face, attribute_start)?
            {
                return Ok(reference);
            }
            if let Some(reference) = face_name_attribute_ref(target, face, attribute_start)? {
                return Ok(reference);
            }
            if face.color.is_none() {
                Ok(-1)
            } else {
                color_attribute_ref(
                    &target.model,
                    owner_ordinal,
                    ColorOwner::Face,
                    attribute_start,
                )
            }
        }
        AttributeTarget::Edge(id) => {
            let edge = target
                .model
                .edges
                .get(owner_ordinal)
                .filter(|edge| edge.id == *id)
                .ok_or_else(|| CodecError::malformed(format_args!("missing F3D edge {id}")))?;
            Ok(
                edge_persistent_attribute_ref(target, index, edge, owner_ordinal, attribute_start)?
                    .unwrap_or(-1),
            )
        }
        AttributeTarget::Coedge(id) => {
            let coedge = target
                .model
                .coedges
                .get(owner_ordinal)
                .filter(|coedge| coedge.id == *id)
                .ok_or_else(|| CodecError::malformed(format_args!("missing F3D coedge {id}")))?;
            Ok(coedge_sketch_attribute_ref(target, index, coedge, attribute_start)?.unwrap_or(-1))
        }
        AttributeTarget::Vertex(id) => {
            if target
                .model
                .vertices
                .get(owner_ordinal)
                .is_some_and(|vertex| vertex.id == *id)
            {
                Ok(-1)
            } else {
                Err(CodecError::malformed(format_args!(
                    "missing F3D vertex {id}"
                )))
            }
        }
        _ => Err(CodecError::NotImplemented(
            "source-less F3D timestamp has an unsupported attribute target".into(),
        )),
    }
}

fn native_attribute_base(records: &mut Vec<u8>, next: i64, previous: i64, owner: i64) {
    native_ref(records, -1);
    native_i64(records, -1);
    native_ref(records, next);
    native_ref(records, previous);
    native_ref(records, owner);
}

fn compact_custom_shape(attribute: &cadmpeg_ir::attributes::SourceAttribute) -> bool {
    use cadmpeg_ir::attributes::AttributeValue;
    attribute
        .name
        .split('-')
        .all(|part| !part.is_empty() && u8::try_from(part.len()).is_ok())
        && matches!(
            attribute.values.first(),
            Some(Some(AttributeValue::Reference(_)))
        )
        && attribute.values[1..].iter().all(|value| match value {
            Some(
                AttributeValue::String(_)
                | AttributeValue::Integer(_)
                | AttributeValue::Float(_)
                | AttributeValue::Boolean(_),
            ) => true,
            Some(AttributeValue::Vector(values)) => matches!(values.len(), 2 | 3),
            Some(AttributeValue::Reference(value)) => value == "f3d:brep:entity#-1",
            _ => false,
        })
}

fn compact_timestamp_payload<'a>(
    target: &'a CadIr,
    entity: &AttributeTarget,
    index: &AttributeIndex<'_>,
) -> Option<&'a cadmpeg_ir::attributes::SourceAttribute> {
    use cadmpeg_ir::attributes::AttributeValue;
    let timestamp = index.timestamp(entity)?;
    let candidates = target
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.target == *entity && compact_custom_shape(attribute))
        .collect::<Vec<_>>();
    let indexed = candidates
        .iter()
        .copied()
        .filter(|attribute| {
            cadmpeg_asm::brep::attributes::attribute_key(attribute)
                == timestamp.record_index.to_string()
        })
        .collect::<Vec<_>>();
    match indexed.as_slice() {
        [attribute] => Some(*attribute),
        [] => {
            let mut family = candidates.into_iter().filter(|attribute| {
                matches!(attribute.values.get(1), Some(Some(AttributeValue::String(value))) if value == "Timestamp_attrib_def")
            });
            let first = family.next()?;
            family.next().is_none().then_some(first)
        }
        _ => None,
    }
}

fn compact_custom_payload<'a>(
    target: &'a CadIr,
    entity: &AttributeTarget,
    kind: &str,
    index: &AttributeIndex<'_>,
) -> Option<&'a cadmpeg_ir::attributes::SourceAttribute> {
    use cadmpeg_ir::attributes::AttributeValue;
    let mut matches = target.model.attributes.iter().filter(|attribute| {
        attribute.target == *entity
            && compact_custom_shape(attribute)
            && matches!(attribute.values.get(1), Some(Some(AttributeValue::String(value))) if value == kind)
    });
    let attribute = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let Some(Some(AttributeValue::Reference(reference))) = attribute.values.first() else {
        return None;
    };
    if index.indexed_timestamp(entity).is_some() {
        let next = compact_timestamp_payload(target, entity, index)?;
        let expected = next.id.as_str().replace(":attribute#", ":entity#");
        if reference != &expected {
            return None;
        }
    } else if !reference.ends_with(":entity#-1") {
        return None;
    }
    Some(attribute)
}

pub(in crate::writer) fn represented_attributes(
    target: &CadIr,
    native: &F3dNative,
) -> std::collections::BTreeSet<String> {
    let index = AttributeIndex::new(target, native);
    target
        .model
        .attributes
        .iter()
        .filter_map(|attribute| {
            let schema = match &attribute.target {
                _ if index
                    .extra_owner_attributes
                    .iter()
                    .any(|record| record.id == attribute.id) =>
                {
                    true
                }
                AttributeTarget::Body(id)
                    if target
                        .model
                        .bodies
                        .iter()
                        .enumerate()
                        .any(|(ordinal, body)| {
                            body.id == *id
                                && body.name.is_none()
                                && index.body_links.get(id.as_str()).is_none_or(Vec::is_empty)
                                && index.indexed_timestamp(&attribute.target).is_none()
                                && body.color.is_some_and(|color| {
                                    i64::try_from(ordinal)
                                        .ok()
                                        .and_then(|ordinal| ordinal.checked_add(1))
                                        .is_some_and(|owner| {
                                            retained_color_values(
                                                target,
                                                &attribute.target,
                                                color,
                                                -1,
                                                owner,
                                            )
                                            .is_some()
                                        })
                                })
                        })
                        && attribute.name == "rgb_color-st-attrib" =>
                {
                    true
                }
                AttributeTarget::Body(id)
                    if index
                        .body_links
                        .get(id.as_str())
                        .is_some_and(|links| !links.is_empty())
                        && compact_custom_payload(
                            target,
                            &attribute.target,
                            "generic_tag_attrib_def",
                            &index,
                        )
                        .is_some_and(|record| record.id == attribute.id) =>
                {
                    true
                }
                AttributeTarget::Coedge(id)
                    if index.coedge_sketch_links.contains_key(id.as_str())
                        && compact_custom_payload(
                            target,
                            &attribute.target,
                            "sketch_attrib_def",
                            &index,
                        )
                        .is_some_and(|record| record.id == attribute.id) =>
                {
                    true
                }
                entity
                    if index.indexed_timestamp(entity).is_some()
                        && compact_timestamp_payload(target, entity, &index)
                            .is_some_and(|record| record.id == attribute.id) =>
                {
                    true
                }
                _ => false,
            };
            schema.then(|| attribute.id.as_str().to_owned())
        })
        .collect()
}

fn native_compact_custom(
    records: &mut Vec<u8>,
    next: i64,
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
) -> Result<(), CodecError> {
    use cadmpeg_ir::attributes::AttributeValue;
    let mut parts = attribute.name.split('-').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_some() {
            native_subident(records, part)?;
        } else {
            native_ident(records, part)?;
        }
    }
    native_ref(records, next);
    for value in &attribute.values[1..] {
        match value {
            Some(AttributeValue::String(value)) => native_string(records, value),
            Some(AttributeValue::Integer(value)) => native_i64(records, *value),
            Some(AttributeValue::Float(value)) => native_f64(records, value.get()),
            Some(AttributeValue::Boolean(value)) => {
                records.push(crate::writer::primitives::native_bool(*value));
            }
            Some(AttributeValue::Reference(value)) if value == "f3d:brep:entity#-1" => {
                native_ref(records, -1);
            }
            Some(AttributeValue::Vector(values)) if matches!(values.len(), 2 | 3) => {
                records.push(if values.len() == 2 { 0x16 } else { 0x14 });
                for value in values {
                    records.extend_from_slice(&value.get().to_le_bytes());
                }
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "unsupported compact F3D attribute value".into(),
                ))
            }
        }
    }
    Ok(())
}

fn native_persistent_design_attribute(
    records: &mut Vec<u8>,
    links: &[&PersistentDesignLink],
    kind: i64,
    next: i64,
    previous: i64,
    owner: i64,
) -> Result<(), CodecError> {
    native_subident(records, "ATTRIB_CUSTOM")?;
    native_ident(records, "attrib")?;
    native_attribute_base(records, next, previous, owner);
    native_string(records, "generic_tag_attrib_def");
    for value in [kind, kind, -1] {
        native_i64(records, value);
    }
    native_string(records, "generic_tag_attrib_def ");
    native_i64(
        records,
        i64::try_from(links.len())
            .map_err(|_| CodecError::InvalidInput("too many persistent body IDs".into()))?,
    );
    for link in links {
        native_i64(records, kind);
        native_string(records, link.design_id.as_str());
        for value in [link.design_reference, 0, 0] {
            native_i64(records, value);
        }
    }
    Ok(())
}

fn native_persistent_subentity_attribute(
    records: &mut Vec<u8>,
    tags: &[&PersistentSubentityTag],
    next: i64,
    previous: i64,
    owner: i64,
) -> Result<(), CodecError> {
    native_subident(records, "ATTRIB_CUSTOM")?;
    native_ident(records, "attrib")?;
    native_attribute_base(records, next, previous, owner);
    native_string(records, "generic_tag_attrib_def");
    for value in [3, 3, -1] {
        native_i64(records, value);
    }
    native_string(records, "generic_tag_attrib_def ");
    native_i64(
        records,
        i64::try_from(tags.len())
            .map_err(|_| CodecError::InvalidInput("too many persistent subentity tags".into()))?,
    );
    for tag in tags {
        native_i64(records, tag.selector);
        native_string(records, tag.token.as_str());
        native_i64(records, 0);
        native_i64(
            records,
            i64::try_from(tag.design_references.len()).map_err(|_| {
                CodecError::InvalidInput("too many persistent subentity references".into())
            })?,
        );
        for reference in &tag.design_references {
            native_i64(records, *reference);
        }
        native_i64(records, 0);
    }
    Ok(())
}

fn native_sketch_link_attribute(
    records: &mut Vec<u8>,
    link: &SketchCurveLink,
    next: i64,
    owner: i64,
) -> Result<(), CodecError> {
    native_subident(records, "ATTRIB_CUSTOM")?;
    native_ident(records, "attrib")?;
    native_attribute_base(records, next, -1, owner);
    native_string(records, "sketch_attrib_def");
    for value in [1, 1, 3] {
        native_i64(records, value);
    }
    native_string(
        records,
        &format!(
            "{} {} {} 0 {} {}",
            link.sketch_curve_id,
            link.ref_b,
            link.sense
                .map_or(SKETCH_LINK_SENSE_UNCONSTRAINED, i64::from),
            link.role,
            link.closure
        ),
    );
    Ok(())
}

pub(super) fn encode_source_less_attributes(
    records: &mut Vec<u8>,
    target: &CadIr,
    index: &AttributeIndex<'_>,
    attribute_start: i64,
    owners: AttributeOwnerStarts,
    output: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<(), CodecError> {
    let model = &target.model;
    let (faces, coedges) = match owners.surface {
        Some(surface) => (
            model
                .faces
                .iter()
                .enumerate()
                .map(|(ordinal, face)| (ordinal, face, surface.face))
                .collect::<Vec<_>>(),
            model
                .coedges
                .iter()
                .enumerate()
                .map(|(ordinal, coedge)| (ordinal, coedge, surface.coedge))
                .collect::<Vec<_>>(),
        ),
        None => {
            if model.faces.iter().any(|face| {
                face.color.is_some()
                    || face.name.is_some()
                    || !face_persistent_tags(index, face).is_empty()
                    || creation_timestamp(index, &AttributeTarget::Face(face.id.clone())).is_some()
            }) || model.coedges.iter().any(|coedge| {
                sketch_link(index, coedge).is_some()
                    || creation_timestamp(index, &AttributeTarget::Coedge(coedge.id.clone()))
                        .is_some()
            }) {
                return Err(CodecError::NotImplemented(
                    "source-less F3D attribute target has no native owner section".into(),
                ));
            }
            (Vec::new(), Vec::new())
        }
    };
    for (ordinal, timestamp) in index.creation_timestamps.iter().enumerate() {
        if index.creation_timestamps[..ordinal].iter().any(|before| {
            before.target == timestamp.target && before.target_coedge == timestamp.target_coedge
        }) {
            return Err(CodecError::malformed(format_args!(
                "multiple F3D creation timestamps target the same entity: {}",
                timestamp.id
            )));
        }
        if crate::brep::link_neutral_id(
            &target.model.bodies,
            &target.model.faces,
            &target.model.edges,
            &target.model.vertices,
            &target.model.coedges,
            &timestamp.target,
            timestamp.target_coedge,
        )
        .is_none()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D creation timestamp has an unsupported or missing target: {}",
                timestamp.id
            )));
        }
    }
    for (body_ordinal, body, color) in model
        .bodies
        .iter()
        .enumerate()
        .filter_map(|(ordinal, body)| Some((ordinal, body, body.color?)))
    {
        let owner_target = AttributeTarget::Body(body.id.clone());
        let next = if let Some(reference) = body_name_attribute_ref(target, body, attribute_start)?
        {
            reference
        } else if let Some(reference) =
            body_persistent_attribute_ref(target, index, body, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(target, index, &owner_target, attribute_start)?
                .or(extra_attribute_ref(
                    target,
                    index,
                    &owner_target,
                    attribute_start,
                )?)
                .unwrap_or(-1)
        };
        output.charge_entities(1, "F3D BREP record")?;
        native_color_attribute(
            records,
            color,
            next,
            native_record_index(owners.body, body_ordinal)?,
            retained_color_values(
                target,
                &owner_target,
                color,
                next,
                native_record_index(owners.body, body_ordinal)?,
            ),
        )?;
        records.push(0x11);
    }
    for (face_ordinal, face, face_start, color) in faces
        .iter()
        .copied()
        .filter_map(|(ordinal, face, start)| Some((ordinal, face, start, face.color?)))
    {
        let owner_target = AttributeTarget::Face(face.id.clone());
        let next = if let Some(reference) = face_name_attribute_ref(target, face, attribute_start)?
        {
            reference
        } else if let Some(reference) =
            face_persistent_attribute_ref(target, index, face, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(target, index, &owner_target, attribute_start)?
                .or(extra_attribute_ref(
                    target,
                    index,
                    &owner_target,
                    attribute_start,
                )?)
                .unwrap_or(-1)
        };
        output.charge_entities(1, "F3D BREP record")?;
        native_color_attribute(
            records,
            color,
            next,
            native_record_index(face_start, face_ordinal)?,
            retained_color_values(
                target,
                &owner_target,
                color,
                next,
                native_record_index(face_start, face_ordinal)?,
            ),
        )?;
        records.push(0x11);
    }
    for (body_ordinal, body, name) in model
        .bodies
        .iter()
        .enumerate()
        .filter_map(|(ordinal, body)| Some((ordinal, body, body.name.as_deref()?)))
    {
        let owner_target = AttributeTarget::Body(body.id.clone());
        let next = if let Some(reference) =
            body_persistent_attribute_ref(target, index, body, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(target, index, &owner_target, attribute_start)?
                .or(extra_attribute_ref(
                    target,
                    index,
                    &owner_target,
                    attribute_start,
                )?)
                .unwrap_or(-1)
        };
        let previous = if body.color.is_none() {
            -1
        } else {
            color_attribute_ref(
                &target.model,
                body_ordinal,
                ColorOwner::Body,
                attribute_start,
            )?
        };
        output.charge_entities(1, "F3D BREP record")?;
        native_name_attribute(
            records,
            name,
            next,
            previous,
            native_record_index(owners.body, body_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (face_ordinal, face, face_start, name) in faces
        .iter()
        .copied()
        .filter_map(|(ordinal, face, start)| Some((ordinal, face, start, face.name.as_deref()?)))
    {
        let owner_target = AttributeTarget::Face(face.id.clone());
        let next = if let Some(reference) =
            face_persistent_attribute_ref(target, index, face, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(target, index, &owner_target, attribute_start)?
                .or(extra_attribute_ref(
                    target,
                    index,
                    &owner_target,
                    attribute_start,
                )?)
                .unwrap_or(-1)
        };
        let previous = if face.color.is_none() {
            -1
        } else {
            color_attribute_ref(
                &target.model,
                face_ordinal,
                ColorOwner::Face,
                attribute_start,
            )?
        };
        output.charge_entities(1, "F3D BREP record")?;
        native_name_attribute(
            records,
            name,
            next,
            previous,
            native_record_index(face_start, face_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (body_ordinal, body) in model.bodies.iter().enumerate() {
        let links = body_persistent_links(index, body);
        if links.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            index,
            &AttributeTarget::Body(body.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        let previous =
            if let Some(reference) = body_name_attribute_ref(target, body, attribute_start)? {
                reference
            } else if body.color.is_none() {
                -1
            } else {
                color_attribute_ref(
                    &target.model,
                    body_ordinal,
                    ColorOwner::Body,
                    attribute_start,
                )?
            };
        output.charge_entities(1, "F3D BREP record")?;
        if let Some(payload) = compact_custom_payload(
            target,
            &AttributeTarget::Body(body.id.clone()),
            "generic_tag_attrib_def",
            index,
        ) {
            native_compact_custom(records, next, payload)?;
        } else {
            native_persistent_design_attribute(
                records,
                links,
                3,
                next,
                previous,
                native_record_index(owners.body, body_ordinal)?,
            )?;
        }
        records.push(0x11);
    }
    for (face_ordinal, face, face_start) in faces.iter().copied() {
        let tags = face_persistent_tags(index, face);
        if tags.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            index,
            &AttributeTarget::Face(face.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        let previous =
            if let Some(reference) = face_name_attribute_ref(target, face, attribute_start)? {
                reference
            } else if face.color.is_none() {
                -1
            } else {
                color_attribute_ref(
                    &target.model,
                    face_ordinal,
                    ColorOwner::Face,
                    attribute_start,
                )?
            };
        output.charge_entities(1, "F3D BREP record")?;
        native_persistent_subentity_attribute(
            records,
            tags,
            next,
            previous,
            native_record_index(face_start, face_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (edge_ordinal, edge) in model.edges.iter().enumerate() {
        let tags = edge_persistent_tags(index, edge);
        if tags.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            index,
            &AttributeTarget::Edge(edge.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        output.charge_entities(1, "F3D BREP record")?;
        native_persistent_subentity_attribute(
            records,
            tags,
            next,
            -1,
            native_record_index(owners.edge, edge_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (coedge_ordinal, coedge, coedge_start) in coedges.iter().copied() {
        let Some(link) = sketch_link(index, coedge) else {
            continue;
        };
        let next = timestamp_attribute_ref(
            target,
            index,
            &AttributeTarget::Coedge(coedge.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        output.charge_entities(1, "F3D BREP record")?;
        if let Some(attribute) = compact_custom_payload(
            target,
            &AttributeTarget::Coedge(coedge.id.clone()),
            "sketch_attrib_def",
            index,
        ) {
            native_compact_custom(records, next, attribute)?;
        } else {
            native_sketch_link_attribute(
                records,
                link,
                next,
                native_record_index(coedge_start, coedge_ordinal)?,
            )?;
        }
        records.push(0x11);
    }
    for (entity, owner_start, owner_ordinal) in
        model
            .bodies
            .iter()
            .enumerate()
            .map(|(ordinal, item)| (AttributeTarget::Body(item.id.clone()), owners.body, ordinal))
            .chain(faces.iter().copied().map(|(ordinal, item, face_start)| {
                (AttributeTarget::Face(item.id.clone()), face_start, ordinal)
            }))
            .chain(model.edges.iter().enumerate().map(|(ordinal, item)| {
                (AttributeTarget::Edge(item.id.clone()), owners.edge, ordinal)
            }))
            .chain(
                coedges
                    .iter()
                    .copied()
                    .map(|(ordinal, item, coedge_start)| {
                        (
                            AttributeTarget::Coedge(item.id.clone()),
                            coedge_start,
                            ordinal,
                        )
                    }),
            )
            .chain(model.vertices.iter().enumerate().map(|(ordinal, item)| {
                (
                    AttributeTarget::Vertex(item.id.clone()),
                    owners.vertex,
                    ordinal,
                )
            }))
    {
        let Some(timestamp) = creation_timestamp(index, &entity) else {
            continue;
        };
        let previous =
            attribute_before_timestamp(target, index, &entity, owner_ordinal, attribute_start)?;
        output.charge_entities(1, "F3D BREP record")?;
        if let Some(payload) = compact_timestamp_payload(target, &entity, index) {
            native_compact_custom(records, -1, payload)?;
        } else {
            native_subident(records, "ATTRIB_CUSTOM")?;
            native_ident(records, "attrib")?;
            native_attribute_base(
                records,
                -1,
                previous,
                native_record_index(owner_start, owner_ordinal)?,
            );
            native_string(records, "Timestamp_attrib_def");
            native_i64(records, 1);
            native_f64(records, timestamp.unix_microseconds.get());
        }
        records.push(0x11);
    }
    for attribute in &index.extra_owner_attributes {
        output.charge_entities(1, "F3D BREP record")?;
        native_compact_custom(records, -1, attribute)?;
        records.push(0x11);
    }
    Ok(())
}

fn native_name_attribute(
    records: &mut Vec<u8>,
    name: &str,
    next: i64,
    previous: i64,
    owner: i64,
) -> Result<(), CodecError> {
    native_subident(records, "string_attrib")?;
    native_subident(records, "name_attrib")?;
    native_subident(records, "gen")?;
    native_ident(records, "attrib")?;
    native_attribute_base(records, next, previous, owner);
    for flag in [1, 1, 1, 1] {
        native_i64(records, flag);
    }
    native_string(records, "name");
    native_string(records, name);
    Ok(())
}

fn retained_color_values(
    target: &CadIr,
    entity: &AttributeTarget,
    color: Color,
    next: i64,
    owner: i64,
) -> Option<[f64; 4]> {
    use cadmpeg_ir::attributes::AttributeValue;
    let mut records =
        target.model.attributes.iter().filter(|attribute| {
            attribute.target == *entity && attribute.name == "rgb_color-st-attrib"
        });
    let attribute = records.next()?;
    if records.next().is_some() {
        return None;
    }
    let [Some(AttributeValue::Reference(reserved)), Some(AttributeValue::Integer(-1)), Some(AttributeValue::Reference(next_ref)), Some(AttributeValue::Reference(previous)), Some(AttributeValue::Reference(owner_ref)), Some(AttributeValue::Float(r)), Some(AttributeValue::Float(g)), Some(AttributeValue::Float(b)), Some(AttributeValue::Float(a))] =
        attribute.values.as_slice()
    else {
        return None;
    };
    if !reserved.ends_with(":entity#-1")
        || !previous.ends_with(":entity#-1")
        || !next_ref.ends_with(&format!(":entity#{next}"))
        || !owner_ref.ends_with(&format!(":entity#{owner}"))
    {
        return None;
    }
    let values = [r.get(), g.get(), b.get(), a.get()];
    let channels = values.map(cadmpeg_core::convert::f32_from_f64);
    (channels
        == [
            Some(color.r()),
            Some(color.g()),
            Some(color.b()),
            Some(color.a()),
        ])
    .then_some(values)
}

fn native_color_attribute(
    records: &mut Vec<u8>,
    color: Color,
    next: i64,
    owner: i64,
    retained: Option<[f64; 4]>,
) -> Result<(), CodecError> {
    if color.a() != 1.0 {
        return Err(CodecError::NotImplemented(
            "source-less F3D direct color requires opaque RGB channels".into(),
        ));
    }
    native_subident(records, "rgb_color")?;
    native_subident(records, "st")?;
    native_ident(records, "attrib")?;
    native_attribute_base(records, next, -1, owner);
    for value in retained.unwrap_or([
        f64::from(color.r()),
        f64::from(color.g()),
        f64::from(color.b()),
        1.0,
    ]) {
        native_f64(records, value);
    }
    Ok(())
}
