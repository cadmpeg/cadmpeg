// SPDX-License-Identifier: Apache-2.0
//! Exact draft operation scopes.

use super::shared_frames::exact_fixed_scalar;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::records::feature::direct_face::DesignDraftOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) fn exact_draft_operation_with_owners(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignDraftOperation>, CodecError> {
    let parsed = (|| -> Option<Result<DesignDraftOperation, CodecError>> {
        // The frame is variable-length and carries six or more references, so no
        // frame length or reference count identifies the record. The ordered
        // reference table is in record-index order, so the two scalar lanes hold no
        // fixed position in it either: they sort before the operand groups in one
        // document and after them in another. The lanes are identified by their own
        // properties instead. They are the only scope-owned fixed scalars among the
        // references, and their local ordinals order them.
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Draft)
            || scope.reference_members().len() < 6
        {
            return None;
        }
        let scope_stream = native_stream(&scope.id);
        let mut lanes = scope
            .reference_members()
            .values()
            .filter_map(|record_index| {
                match exact_fixed_scalar(ctx, bytes, records, *record_index) {
                    Ok(Some(scalar)) => (scalar.owner_record_index == Some(scope.record_index))
                        .then_some(Ok((
                            *record_index,
                            u32::from(scalar.ordinal),
                            scalar.value,
                            scalar.value_offset,
                        ))),
                    Ok(None) => {
                        let mut owners = parameter_owners.iter().filter(|owner| {
                            owner.record_index() == *record_index
                                && owner.scope_record_index() == scope.record_index
                                && scope_stream
                                    .is_none_or(|stream| native_stream(owner.id()) == Some(stream))
                        });
                        let Some(owner) = owners.next() else {
                            return None;
                        };
                        if owners.next().is_some() {
                            return None;
                        }
                        Some(Ok((
                            *record_index,
                            owner.local_ordinal(),
                            owner.evaluated_value(),
                            owner.evaluated_value_offset(),
                        )))
                    }
                    Err(error) => Some(Err(error)),
                }
            });
        let first = match lanes.next() {
            Some(Ok(lane)) => Some(lane),
            Some(Err(error)) => return Some(Err(error)),
            None => None,
        };
        let second = match lanes.next() {
            Some(Ok(lane)) => Some(lane),
            Some(Err(error)) => return Some(Err(error)),
            None => None,
        };
        let third = match lanes.next() {
            Some(Ok(lane)) => Some(lane),
            Some(Err(error)) => return Some(Err(error)),
            None => None,
        };
        let (Some(mut first), Some(mut second), None) = (first, second, third) else {
            return None;
        };
        if first.1 > second.1 {
            std::mem::swap(&mut first, &mut second);
        }
        let (angle_record_index, angle_ordinal, angle, angle_offset) = first;
        let (opposite_angle_record_index, opposite_ordinal, opposite, opposite_offset) = second;
        if angle_ordinal != 0 || opposite_ordinal != 1 || opposite.get() != 0.0 {
            return None;
        }
        Some(Ok(DesignDraftOperation {
            angle: cadmpeg_ir::scalar::Angle::from_assigned_real(angle),
            angle_record_index,
            angle_offset,
            opposite_angle_record_index,
            opposite_angle_offset: opposite_offset,
        }))
    })();
    parsed.transpose()
}

pub(super) fn contains_consecutive_guid_pair(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<bool, CodecError> {
    for (at, _) in ctx
        .admit_iter(bytes, "scan F3D consecutive GUID pair candidates")?
        .enumerate()
    {
        let Some(after_first) = crate::design::decode::text::relaxed_guid_end(ctx, bytes, at)?
        else {
            continue;
        };
        if crate::design::decode::text::relaxed_guid_end(ctx, bytes, after_first)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests;
