// SPDX-License-Identifier: Apache-2.0
//! Author legacy sketch containers and isolated point members.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{SketchEntityId, SketchGeometryDefinition, SketchId};

use super::presentation::{GeneratedDesignRegistry, GeneratedDesignType};
use super::records::{encode_design_metastream, native_lp_ascii, native_lp_utf16};

pub(super) struct SketchStream {
    pub(super) bulk: Vec<u8>,
    pub(super) meta: Vec<u8>,
    pub(super) sketches: Vec<SketchId>,
    pub(super) entities: Vec<SketchEntityId>,
}

pub(super) fn encode(target: &CadIr) -> Result<Option<SketchStream>, CodecError> {
    let mut bulk = Vec::new();
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut containers = Vec::new();
    let mut heads = Vec::new();
    let mut points = Vec::new();
    let mut companions = Vec::new();
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut next = 1u32;
    for sketch in &target.model.sketches {
        let Some((origin, normal, axis)) = sketch.resolved_placement() else {
            continue;
        };
        let origin = origin.get();
        let normal = normal.get();
        let axis = axis.get();
        if normal.norm() != 1.0
            || axis.norm() != 1.0
            || normal.dot(axis) != 0.0
            || sketch.native_ref.is_some()
            || sketch.configuration.is_some()
            || sketch.visible.is_some()
            || !sketch.profiles.is_empty()
            || sketch.name.as_deref() != Some(format!("Sketch_{next}").as_str())
            || target
                .model
                .sketch_constraints
                .iter()
                .any(|row| row.sketch == sketch.id)
        {
            continue;
        }
        let members = target
            .model
            .sketch_entities
            .iter()
            .filter(|row| row.sketch == sketch.id)
            .collect::<Vec<_>>();
        if members.iter().any(|row| {
            row.construction
                || row.native_ref.is_some()
                || row.geometry_ref.is_some()
                || !row.endpoint_refs.is_empty()
                || !matches!(
                    row.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                )
        }) {
            continue;
        }
        let count = u32::try_from(members.len())
            .map_err(|_| CodecError::InvalidInput("F3D sketch member count exceeds u32".into()))?;
        let end = next
            .checked_add(2)
            .and_then(|v| count.checked_mul(2).and_then(|n| v.checked_add(n)))
            .ok_or_else(|| {
                CodecError::InvalidInput("F3D sketch record index exceeds u32".into())
            })?;
        let container = next;
        let head = next + 1;
        mark(&mut primary, container, bulk.len())?;
        header(&mut bulk, "256", container)?;
        bulk.extend_from_slice(&0u32.to_le_bytes());
        bulk.extend_from_slice(&[0; 5]);
        bulk.push(0);
        native_lp_utf16(&mut bulk, &format!("Sketch_{container}"))?;
        bulk.extend_from_slice(&u64::MAX.to_le_bytes());
        bulk.push(1);
        bulk.extend_from_slice(&count.to_le_bytes());
        for ordinal in 0..count {
            reference(&mut bulk, container + 2 + ordinal * 2);
        }
        mark(&mut secondary, container, bulk.len())?;
        header(&mut bulk, "257", container)?;
        bulk.extend_from_slice(&[0; 8]);
        reference(&mut bulk, head);
        let ordinal = u32::try_from(sketches.len())
            .ok()
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| CodecError::InvalidInput("F3D sketch ordinal exceeds u32".into()))?;
        bulk.extend_from_slice(&ordinal.to_le_bytes());
        bulk.extend_from_slice(&[0; 7]);
        bulk.extend_from_slice(&count.to_le_bytes());
        for ordinal in 0..count {
            reference(&mut bulk, container + 2 + ordinal * 2);
        }
        mark(&mut primary, head, bulk.len())?;
        header(&mut bulk, "258", head)?;
        bulk.extend_from_slice(&[0; 11]);
        let v_axis = normal.cross(axis);
        for value in [
            axis.x,
            v_axis.x,
            normal.x,
            origin.x / 10.0,
            axis.y,
            v_axis.y,
            normal.y,
            origin.y / 10.0,
            axis.z,
            v_axis.z,
            normal.z,
            origin.z / 10.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ] {
            bulk.extend_from_slice(&value.to_le_bytes());
        }
        bulk.extend_from_slice(&[0, 1]);
        bulk.extend_from_slice(&[0; 10]);
        containers.push(u64::from(container));
        heads.push(u64::from(head));
        for (ordinal, member) in members.iter().enumerate() {
            let point = container
                + 2
                + u32::try_from(ordinal).map_err(|_| {
                    CodecError::InvalidInput("F3D sketch member count exceeds u32".into())
                })? * 2;
            let companion = point + 1;
            let SketchGeometryDefinition::Point { position } = member.geometry.definition() else {
                continue;
            };
            let position = position.get();
            mark(&mut primary, point, bulk.len())?;
            header(&mut bulk, "259", point)?;
            bulk.extend_from_slice(&[0; 10]);
            reference(&mut bulk, companion);
            bulk.push(0);
            bulk.extend_from_slice(&(position.u / 10.0).to_le_bytes());
            bulk.extend_from_slice(&(position.v / 10.0).to_le_bytes());
            bulk.extend_from_slice(&[0; 20]);
            bulk.extend_from_slice(&1f32.to_le_bytes());
            bulk.extend_from_slice(&[0; 12]);
            bulk.extend_from_slice(&1f32.to_le_bytes());
            bulk.extend_from_slice(&1f32.to_le_bytes());
            bulk.extend_from_slice(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
            reference(&mut bulk, companion);
            reference(&mut bulk, container);
            mark(&mut primary, companion, bulk.len())?;
            header(&mut bulk, "260", companion)?;
            bulk.extend_from_slice(&[0; 10]);
            bulk.extend_from_slice(&0u32.to_le_bytes());
            bulk.push(0);
            reference(&mut bulk, point);
            points.push(u64::from(point));
            companions.push(u64::from(companion));
            entities.push(member.id().clone());
        }
        sketches.push(sketch.id.clone());
        next = end;
    }
    if sketches.is_empty() {
        return Ok(None);
    }
    let types = [
        (
            "44A64366-4BD3-4B24-881A-F94C206E8F2D",
            "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9",
            16,
            "MSketch",
            containers,
        ),
        (
            "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9",
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "Geometry",
            vec![],
        ),
        (
            "F47A46FB-DA27-4AFA-8A72-1B38FA596E23",
            "63D6AD42-466A-40B7-9CCD-EB21BC500EF6",
            0,
            "Geometry",
            heads,
        ),
        (
            "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB",
            "8E9D588C-28EF-43C4-B9A6-AA5430A68D4B",
            0,
            "Geometry",
            points,
        ),
        (
            "362B7EC3-0F09-47C8-A3BE-DC066715CDAE",
            "63D6AD42-466A-40B7-9CCD-EB21BC500EF6",
            0,
            "Geometry",
            companions,
        ),
    ]
    .into_iter()
    .map(|(guid, base, version, module, entity_ids)| {
        Ok(GeneratedDesignType {
            type_guid: guid.to_owned().try_into().map_err(CodecError::Malformed)?,
            base_type_guid: Some(base.to_owned().try_into().map_err(CodecError::Malformed)?),
            version,
            module: module.into(),
            entity_ids,
        })
    })
    .collect::<Result<Vec<_>, CodecError>>()?;
    let registry = GeneratedDesignRegistry {
        types,
        body_map: None,
        browser_nodes: None,
    };
    let meta = encode_design_metastream(&registry, &primary, &secondary)?
        .ok_or_else(|| CodecError::Malformed("missing generated sketch metadata".into()))?;
    Ok(Some(SketchStream {
        bulk,
        meta,
        sketches,
        entities,
    }))
}

fn mark(
    rows: &mut Vec<crate::metastream::RecordIndexEntry>,
    id: u32,
    offset: usize,
) -> Result<(), CodecError> {
    rows.push(crate::metastream::RecordIndexEntry {
        entity_id: u64::from(id),
        bulk_offset: u64::try_from(offset)
            .map_err(|_| CodecError::InvalidInput("F3D sketch stream exceeds u64".into()))?,
    });
    Ok(())
}
fn header(out: &mut Vec<u8>, tag: &str, id: u32) -> Result<(), CodecError> {
    native_lp_ascii(out, tag)?;
    out.extend_from_slice(&id.to_le_bytes());
    Ok(())
}
fn reference(out: &mut Vec<u8>, id: u32) {
    out.push(1);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}
