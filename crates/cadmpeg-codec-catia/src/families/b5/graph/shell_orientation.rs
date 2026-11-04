// SPDX-License-Identifier: Apache-2.0
//! Source-closed shell membership for a selected standard plane population.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::units::UnitVector3;

use super::{frame_payload, parse_face_record, parse_surface, B5Record, B5Surface, ObjectFrame};
use crate::families::b5::vecmath::components;
use crate::wire::tokens::{counted_refs, object_ref};

const EPS_SELECTED_PLANE_CHART_PARITY: f64 = 1.0e-9;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::families) struct SelectedShellPlane {
    pub(in crate::families) native_chart_normal: UnitVector3,
    pub(in crate::families) stored_member: i16,
}

impl SelectedShellPlane {
    pub(in crate::families) fn forward_on(
        &self,
        neutral_normal: UnitVector3,
        standard_forward: bool,
    ) -> Option<bool> {
        if standard_forward != (self.stored_member == -1) {
            return None;
        }
        let dot = components(&self.native_chart_normal)
            .into_iter()
            .zip(components(&neutral_normal))
            .map(|(left, right)| left * right)
            .sum::<f64>();
        if (dot.abs() - 1.0).abs() > EPS_SELECTED_PLANE_CHART_PARITY {
            return None;
        }
        Some((self.stored_member == -1) == (dot > 0.0))
    }
}

fn reference_lane(record: &B5Record) -> Option<(Vec<u32>, usize)> {
    let (references, position) = counted_refs(&record.payload, true)?;
    (!references.is_empty()
        && references.iter().copied().collect::<BTreeSet<_>>().len() == references.len())
    .then_some((references, position))
}

fn roster(record: &B5Record) -> Option<Vec<(u32, i16)>> {
    let (faces, mut position) = reference_lane(record)?;
    let count = usize::try_from(object_ref(&record.payload, &mut position, true)?).ok()?;
    if count != faces.len() || !matches!(record.payload.get(position), Some(0x03 | 0x05 | 0x0d)) {
        return None;
    }
    position = position.checked_add(1)?;
    let end = position.checked_add(count.checked_mul(2)?)?;
    if record.payload.get(end..) != Some(&[0x03]) {
        return None;
    }
    faces
        .into_iter()
        .enumerate()
        .map(|(index, face)| {
            let member = View::i16_le_at(&record.payload, position + 2 * index)?;
            matches!(member, -1 | 1).then_some((face, member))
        })
        .collect()
}

fn for_body(
    root: &B5Record,
    records: &BTreeMap<u32, &B5Record>,
    selected_faces: &BTreeSet<u32>,
) -> Option<BTreeMap<u32, SelectedShellPlane>> {
    let (shells, position) = reference_lane(root)?;
    let count_control = u8::try_from(shells.len())
        .ok()?
        .checked_mul(4)?
        .checked_add(1)?;
    if root.payload.get(position..) != Some(&[count_control, 0x0d]) {
        return None;
    }
    let mut found_faces = BTreeSet::new();
    let mut planes = BTreeMap::new();
    for shell in shells {
        let shell_record = records.get(&shell)?;
        if shell_record.class != 0x60 {
            return None;
        }
        let (rosters, position) = reference_lane(shell_record)?;
        if position != shell_record.payload.len() {
            return None;
        }
        for roster_id in rosters {
            let roster_record = records.get(&roster_id)?;
            if roster_record.class != 0x61 {
                return None;
            }
            for (face, stored_member) in roster(roster_record)? {
                if !found_faces.insert(face) {
                    return None;
                }
                let face_record = records.get(&face)?;
                let parsed = parse_face_record(face_record)?;
                if parsed.terminal_control.is_none() || parsed.references.len() < 2 {
                    return None;
                }
                let carrier = parsed.references[0];
                let Some(carrier_record) = records.get(&carrier) else {
                    continue;
                };
                let Some(B5Surface::Plane { frame, .. }) = parse_surface(carrier_record) else {
                    continue;
                };
                planes.insert(
                    face,
                    SelectedShellPlane {
                        native_chart_normal: *frame.axis(),
                        stored_member,
                    },
                );
            }
        }
    }
    (found_faces == *selected_faces).then_some(planes)
}

fn from_records(
    input: &[B5Record],
    selected_faces: &BTreeSet<u32>,
) -> Option<BTreeMap<u32, SelectedShellPlane>> {
    if selected_faces.is_empty() {
        return None;
    }
    let mut records = BTreeMap::new();
    for record in input {
        if record.family != 0xb5 || !matches!(record.class, 0x27 | 0x5f | 0x60 | 0x61 | 0x65) {
            continue;
        }
        if records.insert(record.object_id, record).is_some() {
            return None;
        }
    }
    let mut selected = None;
    for root in input
        .iter()
        .filter(|record| record.family == 0xb5 && record.class == 0x65)
    {
        if let Some(candidate) = for_body(root, &records, selected_faces) {
            if selected.replace(candidate).is_some() {
                return None;
            }
        }
    }
    selected
}

pub(in crate::families) fn from_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    frames: &[ObjectFrame],
    selected_faces: &BTreeSet<u32>,
) -> Result<Option<BTreeMap<u32, SelectedShellPlane>>, CodecError> {
    let selected_frames = || {
        frames.iter().filter(|frame| {
            frame.family == 0xb5 && matches!(frame.class, 0x27 | 0x5f | 0x60 | 0x61 | 0x65)
        })
    };
    let empty = B5Record {
        offset: 0,
        family: 0,
        class: 0,
        object_id: 0,
        payload: Vec::new(),
    };
    let mut records = ctx.alloc_filled(
        selected_frames().count(),
        empty,
        "catia selected shell source records",
    )?;
    for (record, frame) in records.iter_mut().zip(selected_frames()) {
        let Some(payload) = frame_payload(bytes, frame) else {
            return Ok(None);
        };
        let mut retained =
            ctx.alloc_filled(payload.len(), 0, "catia selected shell source payload")?;
        retained.copy_from_slice(payload);
        *record = B5Record {
            offset: frame.start,
            family: frame.family,
            class: frame.class,
            object_id: frame.object_id,
            payload: retained,
        };
    }
    Ok(from_records(&records, selected_faces))
}

#[cfg(test)]
mod tests;
