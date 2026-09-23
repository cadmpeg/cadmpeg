// SPDX-License-Identifier: Apache-2.0
//! Exact draft operation scopes.

use super::shared_frames::exact_fixed_scalar;
use crate::bytes::lp_utf16_bounded;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::records::feature::direct_face::DesignDraftOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;

pub(super) fn exact_draft_operation_with_owners(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignDraftOperation> {
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
            if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index) {
                return (scalar.owner_record_index == Some(scope.record_index)).then_some((
                    *record_index,
                    u32::from(scalar.ordinal),
                    scalar.value,
                    scalar.value_offset,
                ));
            }
            let owners = parameter_owners
                .iter()
                .filter(|owner| {
                    owner.record_index() == *record_index
                        && owner.scope_record_index() == scope.record_index
                        && scope_stream
                            .is_none_or(|stream| native_stream(owner.id()) == Some(stream))
                        && owner.evaluated_value().is_finite()
                })
                .collect::<Vec<_>>();
            let [owner] = owners.as_slice() else {
                return None;
            };
            Some((
                *record_index,
                owner.local_ordinal(),
                owner.evaluated_value(),
                owner.evaluated_value_offset(),
            ))
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(|(_, ordinal, _, _)| *ordinal);
    let [(angle_record_index, angle_ordinal, angle, angle_offset), (opposite_angle_record_index, opposite_ordinal, opposite, opposite_offset)] =
        lanes.as_slice()
    else {
        return None;
    };
    if *angle_ordinal != 0 || *opposite_ordinal != 1 || !angle.is_finite() || *opposite != 0.0 {
        return None;
    }
    Some(DesignDraftOperation {
        angle: cadmpeg_ir::scalar::Angle::new(*angle)?,
        angle_record_index: *angle_record_index,
        angle_offset: *angle_offset,
        opposite_angle_record_index: *opposite_angle_record_index,
        opposite_angle_offset: *opposite_offset,
    })
}

pub(super) fn contains_consecutive_guid_pair(bytes: &[u8]) -> bool {
    (0..bytes.len()).any(|at| {
        lp_utf16_bounded(bytes, at, 1..=256)
            .filter(|(first, _)| crate::bytes::is_guid_relaxed(first))
            .and_then(|(_, after_first)| lp_utf16_bounded(bytes, after_first, 1..=256))
            .is_some_and(|(second, _)| crate::bytes::is_guid_relaxed(&second))
    })
}
