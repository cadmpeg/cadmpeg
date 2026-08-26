// SPDX-License-Identifier: Apache-2.0
//! Source-less attribute, link, and tag encoders.

use std::collections::HashMap;

use crate::native::F3dNative;
use crate::records::{
    CreationTimestamp, PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
    SKETCH_LINK_SENSE_UNCONSTRAINED,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::{Body, Coedge, Color, Edge, Face};

use super::native_bytes::{
    native_f64, native_i64, native_ident, native_record_index, native_ref, native_string,
    native_subident,
};

pub(crate) struct AttributeIndex<'a> {
    creation_timestamps: &'a [CreationTimestamp],
    body_group_ordinals: HashMap<String, usize>,
    face_group_ordinals: HashMap<String, usize>,
    edge_group_ordinals: HashMap<String, usize>,
    sketch_ordinals: HashMap<String, usize>,
    body_group_count: usize,
    face_group_count: usize,
    edge_group_count: usize,
    body_links: HashMap<&'a str, Vec<&'a PersistentDesignLink>>,
    face_tags: HashMap<&'a str, Vec<&'a PersistentSubentityTag>>,
    edge_tags: HashMap<&'a str, Vec<&'a PersistentSubentityTag>>,
    body_timestamps: HashMap<&'a str, &'a CreationTimestamp>,
    face_timestamps: HashMap<&'a str, &'a CreationTimestamp>,
    edge_timestamps: HashMap<&'a str, &'a CreationTimestamp>,
    coedge_timestamps: HashMap<&'a str, &'a CreationTimestamp>,
    vertex_timestamps: HashMap<&'a str, &'a CreationTimestamp>,
    coedge_sketch_links: HashMap<&'a str, &'a SketchCurveLink>,
    body_timestamp_ordinals: HashMap<&'a str, usize>,
    face_timestamp_ordinals: HashMap<&'a str, usize>,
    edge_timestamp_ordinals: HashMap<&'a str, usize>,
    coedge_timestamp_ordinals: HashMap<&'a str, usize>,
    vertex_timestamp_ordinals: HashMap<&'a str, usize>,
    body_keys: HashMap<&'a str, &'a cadmpeg_asm::brep::records::BodyNativeKey>,
    assigned_body_keys: HashMap<&'a str, u64>,
}

/// Record-table starts used to write the owner field of generated attributes.
#[derive(Clone, Copy)]
pub(crate) struct AttributeOwnerStarts {
    pub(crate) body: i64,
    pub(crate) face: Option<i64>,
    pub(crate) coedge: Option<i64>,
    pub(crate) edge: i64,
    pub(crate) vertex: i64,
}

fn native_owner(start: Option<i64>, ordinal: usize) -> Result<i64, CodecError> {
    native_record_index(
        start.ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D attribute target has no native owner section".into(),
            )
        })?,
        ordinal,
    )
}

impl<'a> AttributeIndex<'a> {
    pub(crate) fn new(target: &'a CadIr, native: &'a F3dNative) -> Result<Self, CodecError> {
        let mut body_links: HashMap<_, Vec<_>> = HashMap::new();
        let mut body_keys = HashMap::new();
        for key in &native.body_native_keys {
            body_keys.entry(key.body.as_str()).or_insert(key);
        }
        let mut material_body_keys = HashMap::new();
        for assignment in &native.design_material_assignments {
            if let Some(appearance) =
                crate::materials::appearance_for_assignment(&target.model.appearances, assignment)?
            {
                material_body_keys
                    .entry(appearance.id.as_str())
                    .or_insert(assignment.asm_body_key);
            }
        }
        let mut assigned_body_keys = HashMap::new();
        for binding in &target.model.appearance_bindings {
            let cadmpeg_ir::appearance::AppearanceTarget::Body(body) = &binding.target else {
                continue;
            };
            if let Some(key) = material_body_keys.get(binding.appearance.as_str()) {
                assigned_body_keys.entry(body.as_str()).or_insert(*key);
            }
        }
        for link in &native.persistent_design_links {
            if let cadmpeg_ir::attributes::AttributeTarget::Body(id) = &link.target {
                body_links.entry(id.as_str()).or_default().push(link);
            }
        }
        for group in body_links.values_mut() {
            group.sort_by_key(|link| link.ordinal);
        }
        let mut face_tags: HashMap<_, Vec<_>> = HashMap::new();
        let mut edge_tags: HashMap<_, Vec<_>> = HashMap::new();
        for tag in &native.persistent_subentity_tags {
            match &tag.target {
                cadmpeg_ir::attributes::AttributeTarget::Face(id) => {
                    face_tags.entry(id.as_str()).or_default().push(tag);
                }
                cadmpeg_ir::attributes::AttributeTarget::Edge(id) => {
                    edge_tags.entry(id.as_str()).or_default().push(tag);
                }
                _ => {}
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
            match &timestamp.target {
                cadmpeg_ir::attributes::AttributeTarget::Body(id) => {
                    body_timestamps.entry(id.as_str()).or_insert(timestamp);
                }
                cadmpeg_ir::attributes::AttributeTarget::Face(id) => {
                    face_timestamps.entry(id.as_str()).or_insert(timestamp);
                }
                cadmpeg_ir::attributes::AttributeTarget::Edge(id) => {
                    edge_timestamps.entry(id.as_str()).or_insert(timestamp);
                }
                cadmpeg_ir::attributes::AttributeTarget::Coedge(id) => {
                    coedge_timestamps.entry(id.as_str()).or_insert(timestamp);
                }
                cadmpeg_ir::attributes::AttributeTarget::Vertex(id) => {
                    vertex_timestamps.entry(id.as_str()).or_insert(timestamp);
                }
                _ => {}
            }
        }
        let mut coedge_sketch_links = HashMap::new();
        for link in &native.sketch_curve_links {
            if let cadmpeg_ir::attributes::AttributeTarget::Coedge(id) = &link.target {
                coedge_sketch_links.entry(id.as_str()).or_insert(link);
            }
        }
        let mut body_timestamp_ordinals = HashMap::new();
        let mut face_timestamp_ordinals = HashMap::new();
        let mut edge_timestamp_ordinals = HashMap::new();
        let mut coedge_timestamp_ordinals = HashMap::new();
        let mut vertex_timestamp_ordinals = HashMap::new();
        let mut split_ordinal = 0;
        macro_rules! split_timestamp_ordinals {
            ($items:expr, $timestamps:expr, $ordinals:expr) => {
                for item in $items {
                    if $timestamps.contains_key(item.id.as_str()) {
                        $ordinals.insert(item.id.as_str(), split_ordinal);
                        split_ordinal += 1;
                    }
                }
            };
        }
        split_timestamp_ordinals!(
            &target.model.bodies,
            body_timestamps,
            body_timestamp_ordinals
        );
        split_timestamp_ordinals!(
            &target.model.faces,
            face_timestamps,
            face_timestamp_ordinals
        );
        split_timestamp_ordinals!(
            &target.model.edges,
            edge_timestamps,
            edge_timestamp_ordinals
        );
        split_timestamp_ordinals!(
            &target.model.coedges,
            coedge_timestamps,
            coedge_timestamp_ordinals
        );
        split_timestamp_ordinals!(
            &target.model.vertices,
            vertex_timestamps,
            vertex_timestamp_ordinals
        );
        let body_group_count = target
            .model
            .bodies
            .iter()
            .filter(|body| body_links.contains_key(body.id.as_str()))
            .count();
        let face_group_count = target
            .model
            .faces
            .iter()
            .filter(|face| face_tags.contains_key(face.id.as_str()))
            .count();
        let edge_group_count = target
            .model
            .edges
            .iter()
            .filter(|edge| edge_tags.contains_key(edge.id.as_str()))
            .count();
        let body_group_ordinals = target
            .model
            .bodies
            .iter()
            .filter(|body| body_links.contains_key(body.id.as_str()))
            .enumerate()
            .map(|(ordinal, body)| (body.id.0.clone(), ordinal))
            .collect();
        let face_group_ordinals = target
            .model
            .faces
            .iter()
            .filter(|face| face_tags.contains_key(face.id.as_str()))
            .enumerate()
            .map(|(ordinal, face)| (face.id.0.clone(), ordinal))
            .collect();
        let edge_group_ordinals = target
            .model
            .edges
            .iter()
            .filter(|edge| edge_tags.contains_key(edge.id.as_str()))
            .enumerate()
            .map(|(ordinal, edge)| (edge.id.0.clone(), ordinal))
            .collect();
        let sketch_ordinals = target
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge_sketch_links.contains_key(coedge.id.as_str()))
            .enumerate()
            .map(|(ordinal, coedge)| (coedge.id.0.clone(), ordinal))
            .collect();
        Ok(Self {
            creation_timestamps: &native.creation_timestamps,
            body_group_ordinals,
            face_group_ordinals,
            edge_group_ordinals,
            sketch_ordinals,
            body_group_count,
            face_group_count,
            edge_group_count,
            body_links,
            face_tags,
            edge_tags,
            body_timestamps,
            face_timestamps,
            edge_timestamps,
            coedge_timestamps,
            vertex_timestamps,
            coedge_sketch_links,
            body_timestamp_ordinals,
            face_timestamp_ordinals,
            edge_timestamp_ordinals,
            coedge_timestamp_ordinals,
            vertex_timestamp_ordinals,
            body_keys,
            assigned_body_keys,
        })
    }

    fn timestamp(
        &self,
        target: &cadmpeg_ir::attributes::AttributeTarget,
    ) -> Option<&'a CreationTimestamp> {
        use cadmpeg_ir::attributes::AttributeTarget;
        match target {
            AttributeTarget::Body(id) => self.body_timestamps.get(id.as_str()).copied(),
            AttributeTarget::Face(id) => self.face_timestamps.get(id.as_str()).copied(),
            AttributeTarget::Edge(id) => self.edge_timestamps.get(id.as_str()).copied(),
            AttributeTarget::Coedge(id) => self.coedge_timestamps.get(id.as_str()).copied(),
            AttributeTarget::Vertex(id) => self.vertex_timestamps.get(id.as_str()).copied(),
            _ => None,
        }
    }

    fn timestamp_ordinal(&self, target: &cadmpeg_ir::attributes::AttributeTarget) -> Option<usize> {
        use cadmpeg_ir::attributes::AttributeTarget;
        match target {
            AttributeTarget::Body(id) => self.body_timestamp_ordinals.get(id.as_str()).copied(),
            AttributeTarget::Face(id) => self.face_timestamp_ordinals.get(id.as_str()).copied(),
            AttributeTarget::Edge(id) => self.edge_timestamp_ordinals.get(id.as_str()).copied(),
            AttributeTarget::Coedge(id) => self.coedge_timestamp_ordinals.get(id.as_str()).copied(),
            AttributeTarget::Vertex(id) => self.vertex_timestamp_ordinals.get(id.as_str()).copied(),
            _ => None,
        }
    }
}

pub(crate) fn source_less_body_key(
    index: &AttributeIndex<'_>,
    body: &Body,
    body_ordinal: usize,
) -> Result<i64, CodecError> {
    if let Some(key) = index.body_keys.get(body.id.as_str()) {
        return key.asm_body_key.map_or(Ok(-1), |key| {
            i64::try_from(key)
                .map_err(|_| CodecError::NotImplemented("F3D ASM body key exceeds i64::MAX".into()))
        });
    }
    let assigned = index.assigned_body_keys.get(body.id.as_str()).copied();
    let key = assigned.unwrap_or(
        u64::try_from(body_ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(1))
            .ok_or_else(|| CodecError::NotImplemented("F3D body ordinal exceeds u64".into()))?,
    );
    i64::try_from(key)
        .map_err(|_| CodecError::NotImplemented("F3D ASM body key exceeds i64::MAX".into()))
}

fn color_attribute_ref(
    model: &cadmpeg_ir::document::Model,
    color: Option<Color>,
    ordinal: usize,
    body: bool,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if color.is_none() {
        return Ok(-1);
    }
    let preceding = if body {
        model.bodies[..ordinal]
            .iter()
            .filter(|body| body.color.is_some())
            .count()
    } else {
        model
            .bodies
            .iter()
            .filter(|body| body.color.is_some())
            .count()
            + model.faces[..ordinal]
                .iter()
                .filter(|face| face.color.is_some())
                .count()
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

pub(crate) fn persistent_subentity_tags<'i, 'n>(
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
        + index.body_group_count
        + index.face_group_count
        + index.edge_group_count
        + index.coedge_sketch_links.len()
}

pub(crate) fn timestamp_attribute_ref(
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
    let color_count = target
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
            .count();
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

pub(crate) fn owner_color_or_body_tag_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    body: &Body,
    body_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if body.color.is_some() {
        return color_attribute_ref(
            &target.model,
            body.color,
            body_ordinal,
            true,
            attribute_start,
        );
    }
    if let Some(reference) = body_name_attribute_ref(target, body, attribute_start)? {
        return Ok(reference);
    }
    if let Some(reference) = body_persistent_attribute_ref(target, index, body, attribute_start)? {
        return Ok(reference);
    }
    Ok(timestamp_attribute_ref(
        target,
        index,
        &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
        attribute_start,
    )?
    .unwrap_or(-1))
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
            + index.body_group_count
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

pub(crate) fn owner_color_or_face_tag_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    face: &Face,
    face_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if face.color.is_some() {
        return color_attribute_ref(
            &target.model,
            face.color,
            face_ordinal,
            false,
            attribute_start,
        );
    }
    if let Some(reference) = face_name_attribute_ref(target, face, attribute_start)? {
        return Ok(reference);
    }
    if let Some(reference) = face_persistent_attribute_ref(target, index, face, attribute_start)? {
        return Ok(reference);
    }
    Ok(timestamp_attribute_ref(
        target,
        index,
        &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
        attribute_start,
    )?
    .unwrap_or(-1))
}

pub(crate) fn edge_persistent_attribute_ref(
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
            + index.body_group_count
            + index.face_group_count
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
            + index.body_group_count
            + index.face_group_count
            + index.edge_group_count
            + preceding,
    )
    .map(Some)
}

pub(crate) fn sketch_link_attribute_ref(
    target: &CadIr,
    index: &AttributeIndex<'_>,
    coedge: &Coedge,
    _coedge_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if let Some(reference) = coedge_sketch_attribute_ref(target, index, coedge, attribute_start)? {
        return Ok(reference);
    }
    Ok(timestamp_attribute_ref(
        target,
        index,
        &AttributeTarget::Coedge(coedge.id.clone()),
        attribute_start,
    )?
    .unwrap_or(-1))
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
            color_attribute_ref(
                &target.model,
                body.color,
                owner_ordinal,
                true,
                attribute_start,
            )
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
            color_attribute_ref(
                &target.model,
                face.color,
                owner_ordinal,
                false,
                attribute_start,
            )
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
    native_string(records, "generic_tag_attrib_def")?;
    for value in [kind, kind, -1] {
        native_i64(records, value);
    }
    native_string(records, "generic_tag_attrib_def ")?;
    native_i64(
        records,
        i64::try_from(links.len())
            .map_err(|_| CodecError::NotImplemented("too many persistent body IDs".into()))?,
    );
    for link in links {
        if link.entity_kind != kind {
            return Err(CodecError::malformed(format_args!(
                "persistent design link {} has entity kind {}, expected {kind}",
                link.id, link.entity_kind
            )));
        }
        native_i64(records, link.entity_kind);
        native_string(records, &link.design_id)?;
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
    native_string(records, "generic_tag_attrib_def")?;
    for value in [3, 3, -1] {
        native_i64(records, value);
    }
    native_string(records, "generic_tag_attrib_def ")?;
    native_i64(
        records,
        i64::try_from(tags.len())
            .map_err(|_| CodecError::NotImplemented("too many persistent subentity tags".into()))?,
    );
    for tag in tags {
        native_i64(records, tag.selector);
        native_string(records, &tag.token)?;
        native_i64(records, 0);
        native_i64(
            records,
            i64::try_from(tag.design_references.len()).map_err(|_| {
                CodecError::NotImplemented("too many persistent subentity references".into())
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
    native_string(records, "sketch_attrib_def")?;
    for value in [1, 1, 3] {
        native_i64(records, value);
    }
    native_string(
        records,
        &format!(
            "{} {} {} 0 {} {}",
            link.sketch_curve_id,
            link.ref_b,
            link.sense.unwrap_or(SKETCH_LINK_SENSE_UNCONSTRAINED),
            link.role,
            link.closure
        ),
    )
}

pub(crate) fn encode_source_less_attributes(
    records: &mut Vec<u8>,
    target: &CadIr,
    index: &AttributeIndex<'_>,
    attribute_start: i64,
    owners: AttributeOwnerStarts,
) -> Result<(), CodecError> {
    let model = &target.model;
    for (ordinal, timestamp) in index.creation_timestamps.iter().enumerate() {
        if !timestamp.unix_microseconds.is_finite() {
            return Err(CodecError::malformed(format_args!(
                "F3D creation timestamp {} is non-finite",
                timestamp.id
            )));
        }
        if index.creation_timestamps[..ordinal]
            .iter()
            .any(|before| before.target == timestamp.target)
        {
            return Err(CodecError::malformed(format_args!(
                "multiple F3D creation timestamps target the same entity: {}",
                timestamp.id
            )));
        }
        if timestamp_attribute_ordinal(index, &timestamp.target).is_none() {
            return Err(CodecError::NotImplemented(format!(
                "F3D creation timestamp has an unsupported or missing target: {}",
                timestamp.id
            )));
        }
    }
    for (body_ordinal, body) in model
        .bodies
        .iter()
        .enumerate()
        .filter(|(_, body)| body.color.is_some())
    {
        let color = body.color.expect("filtered colored body");
        let owner_target = AttributeTarget::Body(body.id.clone());
        let next = if let Some(reference) = body_name_attribute_ref(target, body, attribute_start)?
        {
            reference
        } else if let Some(reference) =
            body_persistent_attribute_ref(target, index, body, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(target, index, &owner_target, attribute_start)?.unwrap_or(-1)
        };
        native_color_attribute(
            records,
            color,
            next,
            native_owner(Some(owners.body), body_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (face_ordinal, face) in model
        .faces
        .iter()
        .enumerate()
        .filter(|(_, face)| face.color.is_some())
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
            timestamp_attribute_ref(target, index, &owner_target, attribute_start)?.unwrap_or(-1)
        };
        native_color_attribute(
            records,
            face.color.expect("filtered colored face"),
            next,
            native_owner(owners.face, face_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (body_ordinal, body) in model
        .bodies
        .iter()
        .enumerate()
        .filter(|(_, body)| body.name.is_some())
    {
        let owner_target = AttributeTarget::Body(body.id.clone());
        let next = if let Some(reference) =
            body_persistent_attribute_ref(target, index, body, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(target, index, &owner_target, attribute_start)?.unwrap_or(-1)
        };
        let previous = color_attribute_ref(
            &target.model,
            body.color,
            body_ordinal,
            true,
            attribute_start,
        )?;
        native_name_attribute(
            records,
            body.name.as_deref().expect("filtered named body"),
            next,
            previous,
            native_owner(Some(owners.body), body_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (face_ordinal, face) in model
        .faces
        .iter()
        .enumerate()
        .filter(|(_, face)| face.name.is_some())
    {
        let owner_target = AttributeTarget::Face(face.id.clone());
        let next = if let Some(reference) =
            face_persistent_attribute_ref(target, index, face, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(target, index, &owner_target, attribute_start)?.unwrap_or(-1)
        };
        let previous = color_attribute_ref(
            &target.model,
            face.color,
            face_ordinal,
            false,
            attribute_start,
        )?;
        native_name_attribute(
            records,
            face.name.as_deref().expect("filtered named face"),
            next,
            previous,
            native_owner(owners.face, face_ordinal)?,
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
            } else {
                color_attribute_ref(
                    &target.model,
                    body.color,
                    body_ordinal,
                    true,
                    attribute_start,
                )?
            };
        native_persistent_design_attribute(
            records,
            links,
            3,
            next,
            previous,
            native_owner(Some(owners.body), body_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (face_ordinal, face) in model.faces.iter().enumerate() {
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
            } else {
                color_attribute_ref(
                    &target.model,
                    face.color,
                    face_ordinal,
                    false,
                    attribute_start,
                )?
            };
        native_persistent_subentity_attribute(
            records,
            tags,
            next,
            previous,
            native_owner(owners.face, face_ordinal)?,
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
        native_persistent_subentity_attribute(
            records,
            tags,
            next,
            -1,
            native_owner(Some(owners.edge), edge_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (coedge_ordinal, coedge) in model.coedges.iter().enumerate() {
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
        native_sketch_link_attribute(
            records,
            link,
            next,
            native_owner(owners.coedge, coedge_ordinal)?,
        )?;
        records.push(0x11);
    }
    for (entity, owner_start, owner_ordinal) in
        model
            .bodies
            .iter()
            .enumerate()
            .map(|(ordinal, item)| {
                (
                    AttributeTarget::Body(item.id.clone()),
                    Some(owners.body),
                    ordinal,
                )
            })
            .chain(model.faces.iter().enumerate().map(|(ordinal, item)| {
                (AttributeTarget::Face(item.id.clone()), owners.face, ordinal)
            }))
            .chain(model.edges.iter().enumerate().map(|(ordinal, item)| {
                (
                    AttributeTarget::Edge(item.id.clone()),
                    Some(owners.edge),
                    ordinal,
                )
            }))
            .chain(model.coedges.iter().enumerate().map(|(ordinal, item)| {
                (
                    AttributeTarget::Coedge(item.id.clone()),
                    owners.coedge,
                    ordinal,
                )
            }))
            .chain(model.vertices.iter().enumerate().map(|(ordinal, item)| {
                (
                    AttributeTarget::Vertex(item.id.clone()),
                    Some(owners.vertex),
                    ordinal,
                )
            }))
    {
        let Some(timestamp) = creation_timestamp(index, &entity) else {
            continue;
        };
        let previous =
            attribute_before_timestamp(target, index, &entity, owner_ordinal, attribute_start)?;
        native_subident(records, "ATTRIB_CUSTOM")?;
        native_ident(records, "attrib")?;
        native_attribute_base(
            records,
            -1,
            previous,
            native_owner(owner_start, owner_ordinal)?,
        );
        native_string(records, "Timestamp_attrib_def")?;
        native_i64(records, 1);
        native_f64(records, timestamp.unix_microseconds);
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
    if name.is_empty() {
        return Err(CodecError::Malformed(
            "source-less F3D display name must not be empty".into(),
        ));
    }
    native_subident(records, "string_attrib")?;
    native_subident(records, "name_attrib")?;
    native_subident(records, "gen")?;
    native_ident(records, "attrib")?;
    native_attribute_base(records, next, previous, owner);
    for flag in [1, 1, 1, 1] {
        native_i64(records, flag);
    }
    native_string(records, "name")?;
    native_string(records, name)
}

fn native_color_attribute(
    records: &mut Vec<u8>,
    color: Color,
    next: i64,
    owner: i64,
) -> Result<(), CodecError> {
    let channels = [color.r, color.g, color.b, color.a];
    if channels
        .iter()
        .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
    {
        return Err(CodecError::Malformed(
            "source-less F3D color channels must be finite and in [0, 1]".into(),
        ));
    }
    if color.a != 1.0 {
        return Err(CodecError::NotImplemented(
            "source-less F3D direct color requires opaque RGB channels".into(),
        ));
    }
    native_subident(records, "rgb_color")?;
    native_subident(records, "st")?;
    native_ident(records, "attrib")?;
    native_attribute_base(records, next, -1, owner);
    native_f64(records, f64::from(color.r));
    native_f64(records, f64::from(color.g));
    native_f64(records, f64::from(color.b));
    native_f64(records, 1.0);
    Ok(())
}
