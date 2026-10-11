// SPDX-License-Identifier: Apache-2.0
//! Source-closed `GSMProject` diameter ranges and their cylinder-face targets.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::FaceId;
use serde::{Deserialize, Serialize};

use crate::decode::ModelingGraphScope;
use crate::entity_table::RangeIntervalSlot;
use crate::native::entity_record::CatiaEntityRecord;
use crate::native::{CatiaNative, CatiaObjectRecord};
use crate::unique_index::UniqueIndex;

const AXIS_TOLERANCE_MM: f64 = 1e-4;
const RADIUS_TOLERANCE_MM: f64 = 1e-5;
const FRAME_TOLERANCE: f64 = 1e-10;

/// One feature-local diameter incidence. This is not a PMI annotation owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectRangeTarget {
    /// Exact paired native entity carrying the same-storage frame.
    pub(crate) frame_entity: String,
    /// The two faces on the unique radius-matched cylinder axis through its origin.
    pub(crate) faces: [FaceId; 2],
}

fn diameter(entity: &CatiaEntityRecord) -> Option<f64> {
    let [definition] = entity.definition_schema_selections.as_slice() else {
        return None;
    };
    let [profile, project, range] = entity.value_schema_selections.as_slice() else {
        return None;
    };
    let interval = entity.range_interval.as_ref()?;
    if entity.lead != 2
        || definition.name.as_deref() != Some("T")
        || profile.name != "ProfileList"
        || project.name != "GSMProject"
        || range.name != "Range"
        || interval.range.entry != range.entry
        || interval.range.ordinal != range.ordinal
        || interval.range.offset != range.offset
        || !matches!(
            interval.interval.slots,
            Some([
                RangeIntervalSlot::Unset { .. },
                RangeIntervalSlot::Unset { .. }
            ])
        )
    {
        return None;
    }
    let value = f64::from_bits(interval.nominal.as_ref()?.bits);
    (value.is_finite() && value > 0.0).then_some(value)
}

fn frame_program(entity: &CatiaEntityRecord) -> bool {
    let [first, second] = entity.definition_schema_selections.as_slice() else {
        return false;
    };
    if first.name.as_deref() != Some("_Elts") || second.name.as_deref() != Some("_Elts") {
        return false;
    }
    let mut matches = entity
        .value_schema_selections
        .iter()
        .enumerate()
        .filter(|(_, s)| s.name == "R");
    let Some((index, selected)) = matches.next() else {
        return false;
    };
    if matches.next().is_some() {
        return false;
    }
    let Some(start) = usize::try_from(selected.offset)
        .ok()
        .and_then(|p| p.checked_add(5))
    else {
        return false;
    };
    let end = match entity.value_schema_selections.get(index + 1) {
        Some(next) => match usize::try_from(next.offset) {
            Ok(offset) => offset,
            Err(_) => return false,
        },
        None => entity.value_payload().len(),
    };
    matches!(
        entity.value_payload().get(start..end),
        Some([
            0xfe,
            0x80..=0xd0,
            0xdc,
            0x72,
            0xe8,
            0xd4,
            0x08,
            0x37,
            0x82,
            0xfe,
            0x80..=0xd0,
            0xe2,
            0xe5,
            0xe8,
            0xc0,
            0x1c,
            0x37,
            0x82,
            0xfe
        ])
    )
}

fn paired<'a>(
    record: &CatiaObjectRecord,
    entities: &UniqueIndex<(&str, u32), &'a CatiaEntityRecord>,
) -> Option<&'a CatiaEntityRecord> {
    let entity = entities
        .get(&(record.parent.as_str(), record.entity_id()?))
        .copied()?;
    (entity.object_graph == record.parent
        && entity.object_record == record.id
        && Some(entity.id.as_str()) == record.entity_record())
    .then_some(entity)
}

fn scalar(entity: &CatiaEntityRecord, position: usize) -> Option<f64> {
    let [selection] = entity.value_schema_selections.as_slice() else {
        return None;
    };
    if selection.name != "GSMProject" {
        return None;
    }
    let suffix = entity.record_suffix();
    let (atom, code) = if position < 3 {
        (0x96, 0xdc)
    } else {
        (0x88, 0xea)
    };
    let first_atom = if (6..9).contains(&position) {
        0x84
    } else {
        0x82
    };
    if suffix.len() < 7 || suffix[0] != first_atom || suffix[1..3] != [atom, 0x82] {
        return None;
    }
    let dialect = match (suffix[3], suffix.get(suffix.len() - 2..)) {
        (c, Some([0x81, 0xdb])) if c == code => true,
        (0xdf, Some([0x81, 0x92])) if position < 3 => true,
        (0xed, Some([0x81, 0x92])) if position >= 3 => true,
        _ => false,
    };
    if !dialect {
        return None;
    }
    // E8 is a zero coordinate only at the third matrix lane in this complete frame.
    if position == 5 && suffix.len() == 7 && suffix[4] == 0xe8 {
        return Some(0.0);
    }
    if position == 5 || suffix.len() != 15 || suffix[4] != 0xe6 {
        return None;
    }
    let value = View::f64_le_at(suffix, 5)?;
    value.is_finite().then_some(value)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn terminal_references(frame: &CatiaObjectRecord) -> Option<[u32; 14]> {
    use crate::object_graph::{ListItem, PayloadField};
    let fields = &frame.payload.fields;
    let start = frame.payload.size.checked_sub(86)?;
    let mut references = [(0_u32, 0_usize); 14];
    if let Some(
        [PayloadField::List {
            declared_count: 15,
            items,
            offset,
        }, PayloadField::Terminator],
    ) = fields.len().checked_sub(2).and_then(|p| fields.get(p..))
    {
        if items.len() != 14 || offset.checked_add(2) != Some(start) {
            return None;
        }
        for (target, item) in references.iter_mut().zip(items) {
            let ListItem::Reference { value, offset } = item else {
                return None;
            };
            *target = (*value, *offset);
        }
    } else {
        let tail = fields.len().checked_sub(16).and_then(|p| fields.get(p..))?;
        if !matches!(tail[15], PayloadField::Terminator)
            || !matches!(tail[14], PayloadField::Atom { value: 129, offset } if offset == start + 84)
        {
            return None;
        }
        for (target, field) in references.iter_mut().zip(&tail[..14]) {
            let PayloadField::Reference { value, offset } = field else {
                return None;
            };
            *target = (*value, *offset);
        }
    }
    let mut ids = [0; 14];
    for (position, (id, offset)) in references.into_iter().enumerate() {
        if offset != start + position * 6 || ids[..position].contains(&id) {
            return None;
        }
        ids[position] = id;
    }
    Some(ids)
}

fn origin(
    ctx: &DecodeContext<'_>,
    frame: &CatiaObjectRecord,
    records: &UniqueIndex<&str, &CatiaObjectRecord>,
    entities: &UniqueIndex<(&str, u32), &CatiaEntityRecord>,
) -> Result<Option<[f64; 3]>, CodecError> {
    // Parse only the terminal 14-reference production, not arbitrary payload references.
    let Some(ids) = terminal_references(frame) else {
        return Ok(None);
    };
    let mut values = [0.0; 12];
    for (position, id) in ids.into_iter().enumerate() {
        ctx.charge_work(1, "catia_project_frame_reference")?;
        let Some(entity) = entities.get(&(frame.parent.as_str(), id)).copied() else {
            return Ok(None);
        };
        let Some(record) = records.get(entity.object_record.as_str()).copied() else {
            return Ok(None);
        };
        if record.parent != frame.parent
            || record.entity_record() != Some(entity.id.as_str())
            || record.entity_id() != Some(id)
        {
            return Ok(None);
        }
        if position < 12 {
            let Some(value) = scalar(entity, position) else {
                return Ok(None);
            };
            values[position] = value;
        }
    }
    let matrix = [
        values[3..6].try_into().expect("three lanes"),
        values[6..9].try_into().expect("three lanes"),
        values[9..12].try_into().expect("three lanes"),
    ];
    for i in 0..3 {
        for j in 0..3 {
            let residual = (dot(matrix[i], matrix[j]) - f64::from(i == j)).abs();
            if !residual.is_finite() || residual > FRAME_TOLERANCE {
                return Ok(None);
            }
        }
    }
    let residual = (dot(matrix[0], cross(matrix[1], matrix[2])) - 1.0).abs();
    if !residual.is_finite() || residual > FRAME_TOLERANCE {
        return Ok(None);
    }
    Ok(Some(
        values[..3].try_into().expect("three origin coordinates"),
    ))
}

fn faces(
    ctx: &DecodeContext<'_>,
    model: &Model,
    origin: [f64; 3],
    diameter: f64,
) -> Result<Option<[FaceId; 2]>, CodecError> {
    let mut carriers = Vec::new();
    for surface in &model.surfaces {
        ctx.charge_work(1, "catia_project_cylinder_search")?;
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) = &surface.geometry
        else {
            continue;
        };
        if (cylinder.radius().get() - diameter / 2.0).abs() > RADIUS_TOLERANCE_MM {
            continue;
        }
        let axis: [f64; 3] = (*cylinder.frame().axis().as_raw()).into();
        let point: [f64; 3] = (*cylinder.origin().as_raw()).into();
        let delta = std::array::from_fn(|i| origin[i] - point[i]);
        let perpendicular = cross(delta, axis);
        let distance = perpendicular[0]
            .hypot(perpendicular[1])
            .hypot(perpendicular[2]);
        if !distance.is_finite() || distance > AXIS_TOLERANCE_MM {
            continue;
        }
        if carriers.len() == 2 {
            return Ok(None);
        }
        ctx.push_vec(
            &mut carriers,
            (&surface.id, axis, point),
            "catia_project_cylinders",
        )?;
    }
    let [first, second] = carriers.as_slice() else {
        return Ok(None);
    };
    if first.0 == second.0 || (dot(first.1, second.1).abs() - 1.0).abs() > FRAME_TOLERANCE {
        return Ok(None);
    }
    let separation = cross(std::array::from_fn(|i| second.2[i] - first.2[i]), first.1);
    let distance = separation[0].hypot(separation[1]).hypot(separation[2]);
    if !distance.is_finite() || distance > AXIS_TOLERANCE_MM {
        return Ok(None);
    }
    let mut selected = Vec::new();
    let mut used = [0_u8; 2];
    for face in &model.faces {
        ctx.charge_work(1, "catia_project_face_search")?;
        if &face.surface == first.0 || &face.surface == second.0 {
            if selected.len() == 2 {
                return Ok(None);
            }
            used[usize::from(&face.surface == second.0)] += 1;
            ctx.push_vec(
                &mut selected,
                face.id.try_clone_for_decode(ctx, "catia_project_face_id")?,
                "catia_project_faces",
            )?;
        }
    }
    ctx.sort_unstable_by(
        &mut selected,
        Ord::cmp,
        |id| id.as_str().len(),
        "catia_project_face_sort",
    )?;
    if used != [1, 1] || selected[0] == selected[1] {
        return Ok(None);
    }
    let Ok(result) = selected.try_into() else {
        return Ok(None);
    };
    Ok(Some(result))
}

/// Retain the exact feature-local frame and geometry incidence without PMI aggregation.
pub(crate) fn bind(
    ctx: &DecodeContext<'_>,
    native: &mut CatiaNative,
    model: &Model,
    scope: &ModelingGraphScope,
) -> Result<(), CodecError> {
    let mut has_project_range = false;
    for entity in &native.entity_records {
        ctx.charge_work(1, "catia_project_range_admission")?;
        if scope.contains(entity.object_graph.as_str()) && diameter(entity).is_some() {
            has_project_range = true;
            break;
        }
    }
    if !has_project_range {
        return Ok(());
    }
    let identities = UniqueIndex::collect(
        ctx,
        native
            .entity_records
            .iter()
            .map(|e| ((e.object_graph.as_str(), e.entity_id), e)),
        "catia_project_identities",
    )?;
    let records = UniqueIndex::collect(
        ctx,
        native
            .object_graphs
            .iter()
            .flat_map(|g| g.records.iter())
            .map(|r| (r.id.as_str(), r)),
        "catia_project_records",
    )?;
    let mut frames = UniqueIndex::new();
    for record in native.object_graphs.iter().flat_map(|g| &g.records) {
        ctx.charge_work(1, "catia_project_frame_search")?;
        if record.lead != 0x54 {
            continue;
        }
        let Some(entity) = paired(record, &identities) else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entity.value_schema_selections.len()),
            "catia_project_frame_program",
        )?;
        if frame_program(entity) {
            if let Some(storage) = record.storage_ref() {
                frames.insert(
                    ctx,
                    (record.parent.as_str(), storage),
                    (record, entity),
                    "catia_project_frames",
                )?;
            }
        }
    }
    let mut bindings = Vec::new();
    for (index, entity) in native.entity_records.iter().enumerate() {
        ctx.charge_work(1, "catia_project_range_search")?;
        if !scope.contains(entity.object_graph.as_str()) {
            continue;
        }
        let Some(diameter) = diameter(entity) else {
            continue;
        };
        let Some(record) = records.get(entity.object_record.as_str()).copied() else {
            continue;
        };
        if record.lead != 0x14
            || paired(record, &identities).map(|e| e.id.as_str()) != Some(entity.id.as_str())
        {
            continue;
        }
        let Some(storage) = record.storage_ref().filter(|s| *s != 0) else {
            continue;
        };
        let Some((frame, frame_entity)) = frames.get(&(record.parent.as_str(), storage)) else {
            continue;
        };
        let Some(origin) = origin(ctx, frame, &records, &identities)? else {
            continue;
        };
        let Some(faces) = faces(ctx, model, origin, diameter)? else {
            continue;
        };
        let target = ProjectRangeTarget {
            frame_entity: ctx.copy_retained_text(&frame_entity.id, "catia_project_frame_entity")?,
            faces,
        };
        ctx.push_vec(&mut bindings, (index, target), "catia_project_bindings")?;
    }
    for (index, target) in bindings {
        native.entity_records[index]
            .range_interval
            .as_mut()
            .expect("admitted complete interval")
            .project_target = Some(target);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
