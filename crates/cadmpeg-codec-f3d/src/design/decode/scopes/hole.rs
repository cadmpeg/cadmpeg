// SPDX-License-Identifier: Apache-2.0
//! Exact hole constructions and hole face selections.

use super::parameter_scope::payload_prologue;
use super::point_data::record_name_end;
use super::shared_frames::find_frame;
use crate::bytes::finite_reals_at;
use crate::bytes::take_reference;
use crate::design::decode::operands::parse_entity_selection_frame;
use crate::design::decode::sketch::indexed_record_header_at;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::retain_class_tag;
use crate::records::feature::hole;
use crate::records::feature::hole::DesignHoleConstruction;
use crate::records::feature::hole::DesignHoleFaceSelection;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
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
///
/// The carrier frame must be the only one among the scope's references. Its
/// input run and the face selection are copied into the output only after
/// that.
pub(in crate::design::decode) fn exact_hole_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
    required_scope_kind: &scope::DesignFeatureKind,
) -> Result<Option<DesignHoleConstruction>, CodecError> {
    // The required kind is a known kind, whose name is a short literal.
    if scope.kind_name().as_bytes() != required_scope_kind.as_str().as_bytes() {
        return Ok(None);
    }
    let mut candidate = None;
    // Each reference is admitted as the search reaches it; the search stops at
    // a second carrier frame.
    for &record_index in scope.reference_members().values() {
        ctx.charge_work(1, "scan F3D Hole scope references")?;
        let Some((type_guid, version)) = stream_types.get(&u64::from(record_index)).copied() else {
            continue;
        };
        // A fixed 36-byte comparison.
        if !matches!(version, 1 | 4) || type_guid.as_bytes() != HOLE_POINT_DATA_TYPE_GUID.as_bytes()
        {
            continue;
        }
        let second = find_frame(
            ctx,
            records,
            record_index,
            |start, paired_at| {
                let Some(payload_at) =
                    record_name_end(ctx, bytes, start, "validate F3D hole ASCII field")?
                else {
                    return Ok(false);
                };
                let Some(frame) = hole_carrier_frame(ctx, bytes, paired_at, payload_at, version)?
                else {
                    return Ok(false);
                };
                Ok(candidate
                    .replace((record_index, start, paired_at, frame))
                    .is_some())
            },
            "scan F3D Hole carrier frames",
        )?;
        if second.is_some() {
            return Ok(None);
        }
    }
    let Some((record_index, start, paired_at, frame)) = candidate else {
        return Ok(None);
    };
    let Some(mut construction) =
        hole_construction(ctx, bytes, record_index, start, paired_at, frame)?
    else {
        return Ok(None);
    };
    construction.face_selection =
        exact_hole_face_selection(ctx, bytes, records, scope, stream_types)?;
    Ok(Some(construction))
}

/// The only direct face selection among the frames of the scope's references.
fn exact_hole_face_selection(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<DesignHoleFaceSelection>, CodecError> {
    let mut candidate = None;
    // Each reference is admitted as the search reaches it; the search stops at
    // a second selection.
    for &record_index in scope.reference_members().values() {
        ctx.charge_work(1, "scan F3D Hole face-selection scope references")?;
        let Some((type_guid, 1)) = stream_types.get(&u64::from(record_index)).copied() else {
            continue;
        };
        // A fixed 36-byte comparison.
        if type_guid.as_bytes() != HOLE_FACE_SELECTION_TYPE_GUID.as_bytes() {
            continue;
        }
        let second = find_frame(
            ctx,
            records,
            record_index,
            |start, _| {
                let Some(header) = indexed_record_header_at(bytes, start) else {
                    return Ok(false);
                };
                let Ok(class_tag) = std::str::from_utf8(header.class_tag) else {
                    return Ok(false);
                };
                let Some(mut frame) = parse_entity_selection_frame(
                    ctx,
                    bytes,
                    record_index,
                    u64_from_index(start),
                    class_tag,
                )?
                else {
                    return Ok(false);
                };
                let Ok(asset_id) = crate::records::mesh::DesignRelaxedGuidText::try_from(
                    std::mem::take(&mut frame.asset_id),
                ) else {
                    return Ok(false);
                };
                let Ok(context_id) = crate::records::mesh::DesignRelaxedGuidText::try_from(
                    std::mem::take(&mut frame.context_id),
                ) else {
                    return Ok(false);
                };
                Ok(candidate
                    .replace((frame, asset_id, context_id, header.class_tag))
                    .is_some())
            },
            "scan F3D Hole face-selection frames",
        )?;
        if second.is_some() {
            return Ok(None);
        }
    }
    // The class tag is copied only for the selection that is kept.
    let Some((frame, asset_id, context_id, class_tag)) = candidate else {
        return Ok(None);
    };
    Ok(Some(DesignHoleFaceSelection {
        record_index: frame.record_index,
        byte_offset: frame.byte_offset,
        class_tag: retain_class_tag(ctx, *class_tag, "copy F3D class tag")?,
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
    }))
}

/// Field offsets of a validated point-and-direction carrier frame.
#[derive(Clone, Copy)]
struct HoleCarrierFrame {
    position_at: usize,
    direction_at: usize,
    point_parameters_at: usize,
    reference_type: u32,
    reference_type_at: usize,
    /// Version four's tangent-point prefix and the offset of its data.
    tangent_point: Option<(u8, usize)>,
    /// Byte offset of the first marked reference of the counted input run.
    inputs_at: usize,
    input_count: usize,
}

/// Validate the carrier payload at `payload_at`, closed by the paired header
/// at `paired_at`, without copying its input run.
fn hole_carrier_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    paired_at: usize,
    payload_at: usize,
    version: u32,
) -> Result<Option<HoleCarrierFrame>, CodecError> {
    let Some(body) = bytes.get(..paired_at) else {
        return Ok(None);
    };
    let Some(cursor) = payload_prologue(body, payload_at, paired_at) else {
        return Ok(None);
    };
    let Some(frame) = hole_carrier_layout(body, cursor, paired_at, version) else {
        return Ok(None);
    };
    // The count is read from the frame, so each reference is admitted as the
    // scan reaches it; the scan stops at the first unmarked one.
    let mut cursor = frame.inputs_at;
    for _ in 0..frame.input_count {
        ctx.charge_work(1, "scan F3D Hole input records")?;
        if input_record(body, &mut cursor).is_none() {
            return Ok(None);
        }
    }
    let closes_at_paired_header = match version {
        4 => cursor == paired_at,
        // Version one keeps further class members before the paired header,
        // so the input run ends at the next header, which must be that one.
        _ => next_indexed_record_offset(ctx, bytes, cursor)? == Some(paired_at),
    };
    Ok(closes_at_paired_header.then_some(frame))
}

/// The fixed-width members after the payload prologue that ends at `cursor`.
fn hole_carrier_layout(
    body: &[u8],
    cursor: usize,
    paired_at: usize,
    version: u32,
) -> Option<HoleCarrierFrame> {
    let _bounding_box_index = View::u32_le_at(body, cursor)?;
    let position_at = cursor.checked_add(4)?;
    let direction_at = position_at.checked_add(24)?;
    let point_parameters_at = direction_at.checked_add(24)?;
    let reference_type_at = point_parameters_at.checked_add(16)?;
    let reference_type = View::u32_le_at(body, reference_type_at)?;
    let mut cursor = reference_type_at.checked_add(4)?;
    let tangent_point = match version {
        4 => {
            let prefix = *body.get(cursor)?;
            let data_at = cursor.checked_add(1)?;
            finite_reals_at::<3>(body, data_at)?;
            cursor = data_at.checked_add(24)?;
            Some((prefix, data_at))
        }
        1 => None,
        _ => return None,
    };
    let input_count = usize::try_from(View::u32_le_at(body, cursor)?).ok()?;
    let inputs_at = cursor.checked_add(4)?;
    if input_count == 0 || input_count > paired_at.checked_sub(inputs_at)? {
        return None;
    }
    finite_reals_at::<3>(body, position_at)?;
    finite_reals_at::<2>(body, point_parameters_at)?;
    let direction: [FiniteReal; 3] = finite_reals_at(body, direction_at)?;
    let direction_norm = direction
        .iter()
        .map(|component| component.get() * component.get())
        .sum::<f64>();
    if (direction_norm - 1.0).abs() > EPS_HOLE_DIRECTION_NORM {
        return None;
    }
    Some(HoleCarrierFrame {
        position_at,
        direction_at,
        point_parameters_at,
        reference_type,
        reference_type_at,
        tangent_point,
        inputs_at,
        input_count,
    })
}

/// The marked input reference at `cursor` with the offset of its record
/// index, advancing past it.
fn input_record(body: &[u8], cursor: &mut usize) -> Option<crate::records::identity::Located<u32>> {
    let offset = u64_from_index(cursor.checked_add(1)?);
    let reference = take_reference(body, cursor)?;
    Some(crate::records::identity::Located {
        value: u32::try_from(reference.target()?).ok()?,
        offset,
    })
}

/// The construction of a validated carrier frame, with its input run copied
/// into retained storage.
fn hole_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    point_record_index: u32,
    start: usize,
    paired_at: usize,
    frame: HoleCarrierFrame,
) -> Result<Option<DesignHoleConstruction>, CodecError> {
    let Some(body) = bytes.get(..paired_at) else {
        return Ok(None);
    };
    let (Some(position), Some(direction), Some(point_parameters)) = (
        finite_reals_at(body, frame.position_at),
        finite_reals_at(body, frame.direction_at),
        finite_reals_at(body, frame.point_parameters_at),
    ) else {
        return Ok(None);
    };
    let tangent_point_data = match frame.tangent_point {
        Some((prefix, data_at)) => {
            let Some(value) = finite_reals_at(body, data_at) else {
                return Ok(None);
            };
            Some(hole::DesignHoleTangentPoint {
                prefix,
                data: crate::records::identity::Located {
                    value,
                    offset: u64_from_index(data_at),
                },
            })
        }
        None => None,
    };
    // `hole_carrier_frame` read every input, so the copy runs to the end.
    let mut input_records = ctx.vector_storage(frame.input_count, "f3d Hole input records")?;
    let mut cursor = frame.inputs_at;
    for _ in ctx.admit_iter(&(0..frame.input_count), "scan F3D Hole input records")? {
        let Some(input) = input_record(body, &mut cursor) else {
            return Ok(None);
        };
        ctx.push_vec(&mut input_records, input, "f3d Hole input records")?;
    }
    Ok(Some(DesignHoleConstruction {
        point_record_index,
        point_record_byte_offset: u64_from_index(start),
        position,
        position_offset: u64_from_index(frame.position_at),
        direction,
        direction_offset: u64_from_index(frame.direction_at),
        point_parameters,
        point_parameter_offsets: [
            u64_from_index(frame.point_parameters_at),
            u64_from_index(frame.point_parameters_at + 8),
        ],
        reference_type: frame.reference_type,
        reference_type_offset: u64_from_index(frame.reference_type_at),
        tangent_point_data,
        input_records,
        face_selection: None,
    }))
}

#[cfg(test)]
mod tests;
