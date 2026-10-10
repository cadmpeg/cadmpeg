// SPDX-License-Identifier: Apache-2.0
//! Exact draft operation scopes.

use super::shared_frames::exact_fixed_scalar;
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::reference_runs::reference_position;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::relaxed_guid_end;
use crate::records::feature::direct_face::DesignDraftOperation;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

/// One scalar lane of a Draft scope: its record index, local ordinal, value
/// and value offset.
type DraftLane = (u32, u32, FiniteReal, u64);

pub(super) fn exact_draft_operation_with_owners(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignDraftOperation>, CodecError> {
    // The frame is variable-length and carries six or more references, so no
    // frame length or reference count identifies the record. The ordered
    // reference table is in record-index order, so the two scalar lanes hold no
    // fixed position in it either: they sort before the operand groups in one
    // document and after them in another. The lanes are identified by their own
    // properties instead. They are the only scope-owned fixed scalars among the
    // references, and their local ordinals order them.
    if !matches!(scope.payload(), DesignScopePayload::Draft(_))
        || scope.reference_members().len() < 6
    {
        return Ok(None);
    }
    let scope_stream = record_stream(ctx, &scope.id)?;
    let mut lanes = [None; 2];
    let mut lane_count = 0;
    // The scan stops at a third lane.
    let third_lane = reference_position(
        ctx,
        scope.reference_members(),
        |record_index| {
            let Some(lane) = draft_lane(
                ctx,
                bytes,
                records,
                scope,
                scope_stream,
                parameter_owners,
                *record_index,
            )?
            else {
                return Ok(false);
            };
            let Some(slot) = lanes.get_mut(lane_count) else {
                return Ok(true);
            };
            *slot = Some(lane);
            lane_count += 1;
            Ok(false)
        },
        "scan F3D Draft scope references",
    )?;
    if third_lane.is_some() {
        return Ok(None);
    }
    let [Some(mut first), Some(mut second)] = lanes else {
        return Ok(None);
    };
    if first.1 > second.1 {
        std::mem::swap(&mut first, &mut second);
    }
    let (angle_record_index, angle_ordinal, angle, angle_offset) = first;
    let (opposite_angle_record_index, opposite_ordinal, opposite, opposite_offset) = second;
    if angle_ordinal != 0 || opposite_ordinal != 1 || opposite.get() != 0.0 {
        return Ok(None);
    }
    Ok(Some(DesignDraftOperation {
        angle: cadmpeg_ir::scalar::Angle::from_assigned_real(angle),
        angle_record_index,
        angle_offset,
        opposite_angle_record_index,
        opposite_angle_offset: opposite_offset,
    }))
}

/// The scalar lane carried by reference `record_index`: a fixed scalar owned by
/// the scope, or else the value of the only parameter owner of that record in
/// the scope's stream.
fn draft_lane(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_stream: Option<&str>,
    parameter_owners: &[DesignParameterOwner],
    record_index: u32,
) -> Result<Option<DraftLane>, CodecError> {
    if let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, record_index)? {
        return Ok(
            (scalar.owner_record_index == Some(scope.record_index)).then_some((
                record_index,
                u32::from(scalar.ordinal),
                scalar.value,
                scalar.value_offset,
            )),
        );
    }
    let is_lane_owner = |owner: &DesignParameterOwner| -> Result<bool, CodecError> {
        if owner.record_index() != record_index || owner.scope_record_index() != scope.record_index
        {
            return Ok(false);
        }
        match scope_stream {
            Some(stream) => in_stream(ctx, owner.id(), stream),
            None => Ok(true),
        }
    };
    let operation = "find F3D Draft lane parameter owner";
    let Some(index) = ctx.position_by(parameter_owners, is_lane_owner, operation)? else {
        return Ok(None);
    };
    let owner = &parameter_owners[index];
    let rest = &parameter_owners[index + 1..];
    if ctx.any_by(rest, is_lane_owner, operation)? {
        return Ok(None);
    }
    Ok(Some((
        record_index,
        owner.local_ordinal(),
        owner.evaluated_value(),
        owner.evaluated_value_offset(),
    )))
}

/// Whether two counted relaxed GUIDs follow each other anywhere in `bytes`.
/// Candidate positions are visited in order and each is admitted before its
/// GUID test, so the search pays only for the positions it reaches.
pub(super) fn contains_consecutive_guid_pair(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<bool, CodecError> {
    // `position_by` visits the slots in order, so the visit count is the
    // candidate's byte position.
    let mut at = 0;
    ctx.any_by(
        bytes,
        |_| {
            let candidate = at;
            at += 1;
            let Some(after_first) = relaxed_guid_end(bytes, candidate) else {
                return Ok(false);
            };
            Ok(relaxed_guid_end(bytes, after_first).is_some())
        },
        "scan F3D consecutive GUID pair candidates",
    )
}

#[cfg(test)]
mod tests;
