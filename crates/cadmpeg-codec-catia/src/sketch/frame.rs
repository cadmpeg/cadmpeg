// SPDX-License-Identifier: Apache-2.0
//! Placement selected by the first exact sketch-roster incidence.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::SketchPlacement;

use crate::native::entity_record::CatiaEntityRecord;
use crate::native::{CatiaDesignObject, CatiaObjectRecord, CatiaObjectRecordReferenceSource};
use crate::unique_index::UniqueIndex;
use crate::value_block::ValueField;

/// Transfer placements for admitted sketch declarations.
pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::document::CadIr,
    native: &crate::native::CatiaNative,
    feature_transfer: &crate::design_feature::DesignFeatureTransfer,
    graph_scope: &crate::decode::ModelingGraphScope,
) -> Result<(), CodecError> {
    let records = super::unique_object_records(ctx, native)?;
    let entities = super::unique_entity_records(ctx, native)?;
    let objects = super::unique_design_objects(ctx, native)?;
    for sketch in &mut ir.model.sketches {
        ctx.charge_work(1, "catia_sketch_frame_transfer")?;
        let Some(object) = sketch
            .native_ref
            .as_deref()
            .and_then(|id| objects.get(id))
            .copied()
        else {
            continue;
        };
        if !graph_scope.contains(object.parent.as_str()) {
            continue;
        }
        let Some(owner_id) = object.owner_record.as_deref() else {
            continue;
        };
        if !feature_transfer.sketch_owner_records.contains(owner_id) {
            continue;
        }
        let Some(owner) = records.get(owner_id).copied() else {
            continue;
        };
        if owner.parent != object.parent
            || owner.design_object.as_deref() != object.owner_design_object.as_deref()
        {
            continue;
        }
        if let Some(placement) = placement(ctx, owner, &records, &entities, &objects)? {
            sketch.placement = placement;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameProgram {
    Absent,
    Controls([u8; 9]),
    Conflicting,
}

fn frame_program(
    ctx: &DecodeContext<'_>,
    entity: &CatiaEntityRecord,
) -> Result<FrameProgram, CodecError> {
    let fields = entity.value_fields_charged(ctx)?;
    let packets = entity.value_packets(ctx, &fields)?;
    let mut result = FrameProgram::Absent;
    for window in fields.windows(17) {
        ctx.charge_work(1, "catia_sketch_frame_program")?;
        if !matches!(
            &window[..7],
            [
                ValueField::Atom {
                    value: 2,
                    width: 1,
                    ..
                },
                ValueField::Atom {
                    value: 6,
                    width: 1,
                    ..
                },
                ValueField::Opcode { code: 0xe8, .. },
                ValueField::Atom {
                    value: 3848,
                    width: 2,
                    ..
                },
                ValueField::Separator { .. },
                ValueField::Atom {
                    value: 10,
                    width: 1,
                    ..
                },
                ValueField::Atom {
                    value: 1,
                    width: 1,
                    ..
                },
            ]
        ) || !matches!(window[16], ValueField::Terminator { .. })
        {
            continue;
        }
        let ValueField::Atom { offset: start, .. } = window[0] else {
            continue;
        };
        let ValueField::Terminator { offset: end } = window[16] else {
            continue;
        };
        let mut overlaps = false;
        for packet in &packets {
            ctx.charge_work(1, "catia_sketch_frame_packet_boundary")?;
            if packet
                .byte_range()
                .is_some_and(|range| start < range.end && range.start <= end)
            {
                overlaps = true;
                break;
            }
        }
        if overlaps {
            continue;
        }
        let mut controls = [0_u8; 9];
        let mut complete = true;
        for (slot, field) in controls.iter_mut().zip(&window[7..16]) {
            match field {
                ValueField::Opcode {
                    code: code @ (0xe7 | 0xe8),
                    ..
                } => *slot = *code,
                _ => complete = false,
            }
        }
        if !complete {
            continue;
        }
        match result {
            FrameProgram::Absent => result = FrameProgram::Controls(controls),
            FrameProgram::Controls(previous) if previous == controls => {}
            _ => return Ok(FrameProgram::Conflicting),
        }
    }
    Ok(result)
}

fn paired_entity<'a>(
    record: &CatiaObjectRecord,
    entities: &UniqueIndex<&'a str, &'a CatiaEntityRecord>,
) -> Option<&'a CatiaEntityRecord> {
    let entity = entities.get(record.entity_record()?).copied()?;
    (entity.object_graph == record.parent
        && entity.object_record == record.id
        && Some(entity.entity_id) == record.entity_id())
    .then_some(entity)
}

fn placement(
    ctx: &DecodeContext<'_>,
    declaration: &CatiaObjectRecord,
    records: &UniqueIndex<&str, &CatiaObjectRecord>,
    entities: &UniqueIndex<&str, &CatiaEntityRecord>,
    objects: &UniqueIndex<&str, &CatiaDesignObject>,
) -> Result<Option<SketchPlacement>, CodecError> {
    let mut first_items = declaration.references.iter().filter(|reference| {
        matches!(
            reference.source(),
            CatiaObjectRecordReferenceSource::ListItem {
                list_payload_offset: 0,
                item_ordinal: 0,
            }
        )
    });
    let (Some(first), None) = (first_items.next(), first_items.next()) else {
        return Ok(None);
    };
    let Some(target) = first.target().and_then(|id| records.get(id)).copied() else {
        return Ok(None);
    };
    if first.is_null()
        || target.parent != declaration.parent
        || target.entity_id() != Some(first.entity_id())
        || target.design_object.as_deref() != first.design_object()
    {
        return Ok(None);
    }
    let Some(group) = target
        .design_object
        .as_deref()
        .and_then(|id| objects.get(id))
        .copied()
    else {
        return Ok(None);
    };
    if group.parent != declaration.parent
        || target.owner_entity_id() != Some(group.owner_entity_id)
        || !group.fields.contains(&target.id)
    {
        return Ok(None);
    }
    let Some(entity) = paired_entity(target, entities) else {
        return Ok(None);
    };
    let FrameProgram::Controls(controls) = frame_program(ctx, entity)? else {
        return Ok(None);
    };
    for field_id in &group.fields {
        ctx.charge_work(1, "catia_sketch_frame_group")?;
        let Some(field) = records.get(field_id.as_str()).copied() else {
            return Ok(None);
        };
        if field.parent != group.parent
            || field.design_object.as_deref() != Some(group.id.as_str())
            || field.owner_entity_id() != Some(group.owner_entity_id)
        {
            return Ok(None);
        }
        let Some(entity) = paired_entity(field, entities) else {
            return Ok(None);
        };
        if field.id == target.id {
            continue;
        }
        match frame_program(ctx, entity)? {
            FrameProgram::Absent => {}
            FrameProgram::Controls(copy) if copy == controls => {}
            _ => return Ok(None),
        }
    }
    let values = controls.map(|code| if code == 0xe7 { 1.0 } else { 0.0 });
    let horizontal = Vector3::new(values[0], values[1], values[2]);
    let vertical = Vector3::new(values[3], values[4], values[5]);
    if horizontal.dot(horizontal) != 1.0
        || vertical.dot(vertical) != 1.0
        || horizontal.dot(vertical) != 0.0
    {
        return Ok(None);
    }
    let normal = horizontal.cross(vertical);
    if normal.dot(normal) != 1.0 {
        return Ok(None);
    }
    Ok(SketchPlacement::try_resolved(
        Point3::new(values[6], values[7], values[8]),
        normal,
        horizontal,
    )
    .ok())
}

#[cfg(test)]
mod tests;
