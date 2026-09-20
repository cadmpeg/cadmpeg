// SPDX-License-Identifier: Apache-2.0
//! Exact hole constructions and hole face selections.

use super::parameter_scope::payload_prologue;
use crate::bytes::f64s_at;
use crate::bytes::lp_ascii_filtered;
use crate::bytes::take_reference;
use crate::design::decode::operands::parse_entity_selection_frame;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::hole;
use crate::records::feature::hole::DesignHoleConstruction;
use crate::records::feature::hole::DesignHoleFaceSelection;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use std::collections::HashMap;

/// Type GUID of the point-and-direction carrier selected by a `Hole` scope.
const HOLE_POINT_DATA_TYPE_GUID: &str = "F2A7590D-6654-4674-B393-A2AEF4FEC48A";

/// Type GUID of the direct persistent face selection carried by a `Hole`.
const HOLE_FACE_SELECTION_TYPE_GUID: &str = "5A1BF548-241F-46FD-9FB5-E4B05126EB9D";

/// Accepted norm error for a serialized Hole drilling direction.
const EPS_HOLE_DIRECTION_NORM: f64 = 1.0e-12;

/// Decode the exact point-and-direction carrier owned by a `Hole` scope.
///
/// The carrier's versioned base level is distinct from the `WorkPoint` level:
/// it writes the position, direction, two construction parameters, `refType`,
/// and a counted input-reference run. Version four inserts tangent-point data
/// before that run; version one omits it and retains additional class members
/// before the paired header. The type GUID and version select the layout; the
/// dynamic class tag does not.
pub(in crate::design::decode) fn exact_hole_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignHoleConstruction> {
    if scope.kind() != scope::DesignFeatureKind::Hole {
        return None;
    }
    let face_selection = exact_hole_face_selection(bytes, records, scope, stream_types);
    let mut candidates = Vec::new();
    for record_index in scope.reference_members().values() {
        let Some((type_guid, version)) = stream_types.get(&u64::from(*record_index)) else {
            continue;
        };
        if *type_guid != HOLE_POINT_DATA_TYPE_GUID || !matches!(*version, 1 | 4) {
            continue;
        }
        for (start, paired_at) in records.frames(*record_index) {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || after_tag != start + 7
                || View::u32_le_at(bytes, after_tag) != Some(*record_index)
            {
                continue;
            }
            let Some((_name, payload_at)) =
                lp_ascii_filtered(bytes, after_tag + 8, 0..=256, u8::is_ascii_graphic)
            else {
                continue;
            };
            if let Some(candidate) = hole_construction_frame_at(
                bytes,
                start,
                paired_at,
                payload_at,
                *record_index,
                *version,
                face_selection.clone(),
            ) {
                candidates.push(candidate);
            }
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn exact_hole_face_selection(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignHoleFaceSelection> {
    let mut candidates = Vec::new();
    for record_index in scope.reference_members().values() {
        if stream_types.get(&u64::from(*record_index)) != Some(&(HOLE_FACE_SELECTION_TYPE_GUID, 1))
        {
            continue;
        }
        for (start, _paired_at) in records.frames(*record_index) {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            let Ok(class_tag) = crate::records::references::DesignClassTag::try_from(class_tag)
            else {
                continue;
            };
            if after_tag != start + 7 || View::u32_le_at(bytes, after_tag) != Some(*record_index) {
                continue;
            }
            let Some(frame) = parse_entity_selection_frame(
                bytes,
                *record_index,
                u64::try_from(start).ok()?,
                class_tag.as_str(),
            ) else {
                continue;
            };
            let Ok(asset_id) =
                crate::records::mesh::DesignRelaxedGuidText::try_from(frame.asset_id)
            else {
                continue;
            };
            let Ok(context_id) =
                crate::records::mesh::DesignRelaxedGuidText::try_from(frame.context_id)
            else {
                continue;
            };
            candidates.push(DesignHoleFaceSelection {
                record_index: frame.record_index,
                byte_offset: frame.byte_offset,
                class_tag,
                asset_id,
                asset_id_offset: frame.asset_id_offset,
                context_id,
                context_id_offset: frame.context_id_offset,
                identity_record_index: frame.identity_record_index,
                identity_record_offset: frame.identity_record_offset,
                primary_identity: frame.primary_identity,
                primary_identity_offset: frame.primary_identity_offset,
                secondary: frame.secondary,
                historical_face_candidates: Vec::new(),
                next_record_index: frame.next_record_index,
                next_byte_offset: frame.next_byte_offset,
            });
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn hole_construction_frame_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    payload_at: usize,
    point_record_index: u32,
    version: u32,
    face_selection: Option<DesignHoleFaceSelection>,
) -> Option<DesignHoleConstruction> {
    let body = bytes.get(..paired_at)?;
    let mut cursor = payload_prologue(body, payload_at, paired_at)?;
    let _bounding_box_index = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    let position_at = cursor;
    cursor = cursor.checked_add(24)?;
    let direction_at = cursor;
    cursor = cursor.checked_add(24)?;
    let point_parameters_at = cursor;
    cursor = cursor.checked_add(16)?;
    let reference_type_at = cursor;
    let reference_type = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    let tangent_point_data = if version == 4 {
        let prefix = *body.get(cursor)?;
        cursor = cursor.checked_add(1)?;
        let tangent_point_data_at = cursor;
        cursor = cursor.checked_add(24)?;
        let tangent_point_data: [f64; 3] =
            f64s_at(body, tangent_point_data_at, 3)?.try_into().ok()?;
        Some(hole::DesignHoleTangentPoint {
            prefix,
            data: crate::records::identity::Located {
                value: tangent_point_data,
                offset: u64::try_from(tangent_point_data_at).ok()?,
            },
        })
    } else if version == 1 {
        None
    } else {
        return None;
    };
    let input_count = usize::try_from(View::u32_le_at(body, cursor)?).ok()?;
    cursor = cursor.checked_add(4)?;
    if input_count == 0 || input_count > paired_at.checked_sub(cursor)? {
        return None;
    }
    let position: [f64; 3] = f64s_at(body, position_at, 3)?.try_into().ok()?;
    let direction: [f64; 3] = f64s_at(body, direction_at, 3)?.try_into().ok()?;
    let point_parameters: [f64; 2] = f64s_at(body, point_parameters_at, 2)?.try_into().ok()?;
    let direction_norm = direction
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if position
        .iter()
        .chain(direction.iter())
        .chain(point_parameters.iter())
        .chain(
            tangent_point_data
                .iter()
                .flat_map(|tangent| tangent.data.value.iter()),
        )
        .any(|value| !value.is_finite())
        || (direction_norm - 1.0).abs() > EPS_HOLE_DIRECTION_NORM
    {
        return None;
    }
    let mut input_records = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        let reference_at = cursor;
        let reference = take_reference(body, &mut cursor)?;
        let target = u32::try_from(reference.target()?).ok()?;
        input_records.push(crate::records::identity::Located {
            value: target,
            offset: u64::try_from(reference_at.checked_add(1)?).ok()?,
        });
    }
    if (version == 4 && cursor != paired_at)
        || (version == 1 && next_indexed_record_offset(bytes, cursor)? != paired_at)
    {
        return None;
    }
    Some(DesignHoleConstruction {
        point_record_index,
        point_record_byte_offset: u64::try_from(start).ok()?,
        position,
        position_offset: u64::try_from(position_at).ok()?,
        direction,
        direction_offset: u64::try_from(direction_at).ok()?,
        point_parameters,
        point_parameter_offsets: [
            u64::try_from(point_parameters_at).ok()?,
            u64::try_from(point_parameters_at.checked_add(8)?).ok()?,
        ],
        reference_type,
        reference_type_offset: u64::try_from(reference_type_at).ok()?,
        tangent_point_data,
        input_records,
        face_selection,
    })
}

#[cfg(test)]
mod tests;
