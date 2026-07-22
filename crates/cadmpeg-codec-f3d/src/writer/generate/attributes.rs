// SPDX-License-Identifier: Apache-2.0
//! Source-less attribute, link, and tag encoders.

use crate::records::{
    CreationTimestamp, PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::{Body, Coedge, Color, Edge, Face};

use super::native_bytes::{
    native_f64, native_i64, native_ident, native_record_index, native_ref, native_string,
    native_subident,
};
use crate::writer::primitives::f3d_native;

pub(crate) fn source_less_body_key(
    target: &CadIr,
    body: &Body,
    body_ordinal: usize,
) -> Result<i64, CodecError> {
    let native = f3d_native(target)?;
    if let Some(key) = native.as_ref().and_then(|native| {
        native
            .body_native_keys
            .iter()
            .find(|key| key.body == body.id)
    }) {
        return key.asm_body_key.map_or(Ok(-1), |key| {
            i64::try_from(key)
                .map_err(|_| CodecError::NotImplemented("F3D ASM body key exceeds i64::MAX".into()))
        });
    }
    let assigned = target
        .model
        .appearance_bindings
        .iter()
        .find_map(|binding| match &binding.target {
            cadmpeg_ir::appearance::AppearanceTarget::Body(id) if id == &body.id => target
                .model
                .appearances
                .iter()
                .find(|appearance| appearance.id == binding.appearance)
                .and_then(|appearance| appearance.visual_guid.as_deref()),
            _ => None,
        })
        .and_then(|visual_guid| {
            native
                .as_ref()?
                .design_material_assignments
                .iter()
                .find(|assignment| {
                    crate::materials::visual_guid_matches(&assignment.visual_guid, visual_guid)
                })
                .map(|assignment| assignment.asm_body_key)
        });
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

fn persistent_links(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Vec<PersistentDesignLink> {
    let mut links = f3d_native(target)
        .ok()
        .flatten()
        .into_iter()
        .flat_map(|native| native.persistent_design_links)
        .filter(|link| &link.target == entity)
        .collect::<Vec<_>>();
    links.sort_by_key(|link| link.ordinal);
    links
}

pub(crate) fn persistent_subentity_tags(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Vec<PersistentSubentityTag> {
    let mut tags = f3d_native(target)
        .ok()
        .flatten()
        .into_iter()
        .flat_map(|native| native.persistent_subentity_tags)
        .filter(|tag| &tag.target == entity)
        .collect::<Vec<_>>();
    tags.sort_by_key(|tag| tag.ordinal);
    tags
}

fn creation_timestamp(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Option<CreationTimestamp> {
    f3d_native(target)
        .ok()
        .flatten()?
        .creation_timestamps
        .into_iter()
        .find(|timestamp| &timestamp.target == entity)
}

fn timestamp_attribute_ordinal(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Option<usize> {
    let model = &target.model;
    let targets = model
        .bodies
        .iter()
        .map(|item| cadmpeg_ir::attributes::AttributeTarget::Body(item.id.clone()))
        .chain(
            model
                .faces
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Face(item.id.clone())),
        )
        .chain(
            model
                .edges
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Edge(item.id.clone())),
        )
        .chain(
            model
                .coedges
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Coedge(item.id.clone())),
        )
        .chain(
            model
                .vertices
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Vertex(item.id.clone())),
        );
    let mut ordinal = 0;
    for candidate in targets {
        if creation_timestamp(target, &candidate).is_some() {
            if &candidate == entity {
                return Some(ordinal);
            }
            ordinal += 1;
        }
    }
    None
}

fn existing_source_less_attribute_count(target: &CadIr) -> usize {
    source_less_color_count(target)
        + source_less_name_count(target)
        + persistent_body_group_count(target)
        + persistent_face_group_count(target)
        + persistent_edge_group_count(target)
        + target
            .model
            .coedges
            .iter()
            .filter(|coedge| sketch_link(target, coedge).is_some())
            .count()
}

pub(crate) fn timestamp_attribute_ref(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    timestamp_attribute_ordinal(target, entity)
        .map(|ordinal| {
            native_record_index(
                attribute_start,
                existing_source_less_attribute_count(target) + ordinal,
            )
        })
        .transpose()
}

fn body_persistent_links(target: &CadIr, body: &Body) -> Vec<PersistentDesignLink> {
    persistent_links(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
    )
}

fn persistent_body_group_count(target: &CadIr) -> usize {
    target
        .model
        .bodies
        .iter()
        .filter(|body| !body_persistent_links(target, body).is_empty())
        .count()
}

fn face_persistent_tags(target: &CadIr, face: &Face) -> Vec<PersistentSubentityTag> {
    persistent_subentity_tags(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
    )
}

fn edge_persistent_tags(target: &CadIr, edge: &Edge) -> Vec<PersistentSubentityTag> {
    persistent_subentity_tags(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone()),
    )
}

fn persistent_face_group_count(target: &CadIr) -> usize {
    target
        .model
        .faces
        .iter()
        .filter(|face| !face_persistent_tags(target, face).is_empty())
        .count()
}

fn persistent_edge_group_count(target: &CadIr) -> usize {
    target
        .model
        .edges
        .iter()
        .filter(|edge| !edge_persistent_tags(target, edge).is_empty())
        .count()
}

fn body_persistent_attribute_ref(
    target: &CadIr,
    body: &Body,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if body_persistent_links(target, body).is_empty() {
        return Ok(None);
    }
    let ordinal = target
        .model
        .bodies
        .iter()
        .take_while(|candidate| candidate.id != body.id)
        .filter(|candidate| !body_persistent_links(target, candidate).is_empty())
        .count();
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
    if let Some(reference) = body_persistent_attribute_ref(target, body, attribute_start)? {
        return Ok(reference);
    }
    Ok(timestamp_attribute_ref(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
        attribute_start,
    )?
    .unwrap_or(-1))
}

fn face_persistent_attribute_ref(
    target: &CadIr,
    face: &Face,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if face_persistent_tags(target, face).is_empty() {
        return Ok(None);
    }
    let ordinal = target
        .model
        .faces
        .iter()
        .take_while(|candidate| candidate.id != face.id)
        .filter(|candidate| !face_persistent_tags(target, candidate).is_empty())
        .count();
    native_record_index(
        attribute_start,
        source_less_color_count(target)
            + source_less_name_count(target)
            + persistent_body_group_count(target)
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
    if let Some(reference) = face_persistent_attribute_ref(target, face, attribute_start)? {
        return Ok(reference);
    }
    Ok(timestamp_attribute_ref(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
        attribute_start,
    )?
    .unwrap_or(-1))
}

pub(crate) fn edge_persistent_attribute_ref(
    target: &CadIr,
    edge: &Edge,
    edge_ordinal: usize,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if edge_persistent_tags(target, edge).is_empty() {
        return Ok(None);
    }
    let ordinal = target.model.edges[..edge_ordinal]
        .iter()
        .filter(|candidate| !edge_persistent_tags(target, candidate).is_empty())
        .count();
    native_record_index(
        attribute_start,
        source_less_color_count(target)
            + source_less_name_count(target)
            + persistent_body_group_count(target)
            + persistent_face_group_count(target)
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

fn sketch_link(target: &CadIr, coedge: &Coedge) -> Option<SketchCurveLink> {
    f3d_native(target)
        .ok()
        .flatten()?
        .sketch_curve_links
        .into_iter()
        .find(|link| link.coedge == coedge.id)
}

pub(crate) fn sketch_link_attribute_ref(
    target: &CadIr,
    coedge: &Coedge,
    coedge_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if sketch_link(target, coedge).is_none() {
        return Ok(timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Coedge(coedge.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1));
    }
    let preceding = target.model.coedges[..coedge_ordinal]
        .iter()
        .filter(|candidate| sketch_link(target, candidate).is_some())
        .count();
    let color_count = source_less_color_count(target);
    native_record_index(
        attribute_start,
        color_count
            + source_less_name_count(target)
            + persistent_body_group_count(target)
            + persistent_face_group_count(target)
            + persistent_edge_group_count(target)
            + preceding,
    )
}

fn native_persistent_design_attribute(
    records: &mut Vec<u8>,
    links: &[PersistentDesignLink],
    kind: i64,
    next: i64,
) -> Result<(), CodecError> {
    native_subident(records, "ATTRIB_CUSTOM")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
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
            return Err(CodecError::Malformed(format!(
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
    tags: &[PersistentSubentityTag],
    next: i64,
) -> Result<(), CodecError> {
    native_subident(records, "ATTRIB_CUSTOM")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
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
) -> Result<(), CodecError> {
    native_subident(records, "ATTRIB_CUSTOM")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
    native_string(records, "sketch_attrib_def")?;
    for value in [1, 1, 3] {
        native_i64(records, value);
    }
    native_string(
        records,
        &format!(
            "{} 0 {} 0 {} {}",
            link.sketch_curve_id,
            link.signed_reference.unwrap_or(-1),
            link.role,
            link.closure
        ),
    )
}

pub(crate) fn encode_source_less_attributes(
    records: &mut Vec<u8>,
    target: &CadIr,
    attribute_start: i64,
) -> Result<(), CodecError> {
    let model = &target.model;
    if let Some(native) = f3d_native(target)? {
        for (ordinal, timestamp) in native.creation_timestamps.iter().enumerate() {
            if !timestamp.unix_microseconds.is_finite() {
                return Err(CodecError::Malformed(format!(
                    "F3D creation timestamp {} is non-finite",
                    timestamp.id
                )));
            }
            if native.creation_timestamps[..ordinal]
                .iter()
                .any(|before| before.target == timestamp.target)
            {
                return Err(CodecError::Malformed(format!(
                    "multiple F3D creation timestamps target the same entity: {}",
                    timestamp.id
                )));
            }
            if timestamp_attribute_ordinal(target, &timestamp.target).is_none() {
                return Err(CodecError::NotImplemented(format!(
                    "F3D creation timestamp has an unsupported or missing target: {}",
                    timestamp.id
                )));
            }
        }
    }
    for body in model.bodies.iter().filter(|body| body.color.is_some()) {
        let color = body.color.expect("filtered colored body");
        let next = if let Some(reference) = body_name_attribute_ref(target, body, attribute_start)?
        {
            reference
        } else if let Some(reference) =
            body_persistent_attribute_ref(target, body, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1)
        };
        native_color_attribute(records, color, next)?;
        records.push(0x11);
    }
    for face in model.faces.iter().filter(|face| face.color.is_some()) {
        let next = if let Some(reference) = face_name_attribute_ref(target, face, attribute_start)?
        {
            reference
        } else if let Some(reference) =
            face_persistent_attribute_ref(target, face, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1)
        };
        native_color_attribute(records, face.color.expect("filtered colored face"), next)?;
        records.push(0x11);
    }
    for body in model.bodies.iter().filter(|body| body.name.is_some()) {
        let next = if let Some(reference) =
            body_persistent_attribute_ref(target, body, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1)
        };
        native_name_attribute(
            records,
            body.name.as_deref().expect("filtered named body"),
            next,
        )?;
        records.push(0x11);
    }
    for face in model.faces.iter().filter(|face| face.name.is_some()) {
        let next = if let Some(reference) =
            face_persistent_attribute_ref(target, face, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1)
        };
        native_name_attribute(
            records,
            face.name.as_deref().expect("filtered named face"),
            next,
        )?;
        records.push(0x11);
    }
    for body in &model.bodies {
        let links = body_persistent_links(target, body);
        if links.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        native_persistent_design_attribute(records, &links, 3, next)?;
        records.push(0x11);
    }
    for face in &model.faces {
        let tags = face_persistent_tags(target, face);
        if tags.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        native_persistent_subentity_attribute(records, &tags, next)?;
        records.push(0x11);
    }
    for edge in &model.edges {
        let tags = edge_persistent_tags(target, edge);
        if tags.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        native_persistent_subentity_attribute(records, &tags, next)?;
        records.push(0x11);
    }
    for coedge in &model.coedges {
        let Some(link) = sketch_link(target, coedge) else {
            continue;
        };
        let next = timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Coedge(coedge.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        native_sketch_link_attribute(records, &link, next)?;
        records.push(0x11);
    }
    for entity in model
        .bodies
        .iter()
        .map(|item| cadmpeg_ir::attributes::AttributeTarget::Body(item.id.clone()))
        .chain(
            model
                .faces
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Face(item.id.clone())),
        )
        .chain(
            model
                .edges
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Edge(item.id.clone())),
        )
        .chain(
            model
                .coedges
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Coedge(item.id.clone())),
        )
        .chain(
            model
                .vertices
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Vertex(item.id.clone())),
        )
    {
        let Some(timestamp) = creation_timestamp(target, &entity) else {
            continue;
        };
        native_subident(records, "ATTRIB_CUSTOM")?;
        native_ident(records, "attrib")?;
        native_ref(records, -1);
        native_string(records, "Timestamp_attrib_def")?;
        native_i64(records, 1);
        native_f64(records, timestamp.unix_microseconds);
        records.push(0x11);
    }
    Ok(())
}

fn native_name_attribute(records: &mut Vec<u8>, name: &str, next: i64) -> Result<(), CodecError> {
    if name.is_empty() {
        return Err(CodecError::Malformed(
            "source-less F3D display name must not be empty".into(),
        ));
    }
    native_subident(records, "string_attrib")?;
    native_subident(records, "name_attrib")?;
    native_subident(records, "gen")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
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
    if color.a == 1.0 {
        native_subident(records, "rgb_color")?;
        native_subident(records, "st")?;
        native_ident(records, "attrib")?;
        native_ref(records, next);
        native_f64(records, f64::from(color.r));
        native_f64(records, f64::from(color.g));
        native_f64(records, f64::from(color.b));
        return Ok(());
    }
    let quantized = channels.map(|channel| (channel * 255.0).round() as u8);
    let decoded = quantized.map(|channel| f32::from(channel) / 255.0);
    if decoded != channels {
        return Err(CodecError::NotImplemented(
            "source-less F3D translucent direct color requires exact 8-bit channels".into(),
        ));
    }
    native_subident(records, "truecolor")?;
    native_subident(records, "st")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
    let packed = u32::from_be_bytes([quantized[3], quantized[0], quantized[1], quantized[2]]);
    native_i64(records, i64::from(packed));
    Ok(())
}
