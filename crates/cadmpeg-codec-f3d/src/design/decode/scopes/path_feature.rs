// SPDX-License-Identifier: Apache-2.0
//! Exact path-feature construction scopes and pipe owner lanes.

use super::fixed_parameters::unique_revolve_angle_owner;
use super::parameter_scope::parameter_scope_payload_length;
use super::shared_frames::marked_record_reference;
use super::shared_frames::FixedScalarFrame;
use super::shared_frames::{exact_fixed_scalar, extrude_operation_at};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::layout::class_403_revolve_scope_frame as class_403_revolve;
use crate::layout::compact_loft_operation_prefix as compact_loft;
use crate::layout::fixed_pipe_operation_prefix as fixed_pipe;
use crate::layout::legacy_pipe_operation_prefix as legacy_pipe;
use crate::layout::marker_one_revolve_prologue as revolve;
use crate::layout::referenced_angle_revolve_prologue as referenced_revolve;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PipePrefix {
    operation: DesignExtrudeOperation,
    operation_offset: u64,
    is_hollow: bool,
    is_hollow_offset: u64,
    opaque_post_operation: [u8; 2],
    opaque_post_operation_offset: u64,
}

fn exact_pipe_prefix(bytes: &[u8], start: usize) -> Option<PipePrefix> {
    let frame = bytes.get(start..)?;
    let (hollow_at, operation_at, opaque_at) =
        if frame.get(fixed_pipe::ZERO_RUN_10..fixed_pipe::IS_HOLLOW) == Some(&[0; 10]) {
            (
                fixed_pipe::IS_HOLLOW,
                fixed_pipe::OPERATION,
                fixed_pipe::OPAQUE_POST_OPERATION,
            )
        } else if frame.get(legacy_pipe::ZERO_RUN_9..legacy_pipe::PREFIX_MARKER) == Some(&[0; 9])
            && frame.get(legacy_pipe::PREFIX_MARKER) == Some(&legacy_pipe::PREFIX_MARKER_VALUE)
            && frame.get(legacy_pipe::ZERO_RUN_4..legacy_pipe::IS_HOLLOW) == Some(&[0; 4])
        {
            (
                legacy_pipe::IS_HOLLOW,
                legacy_pipe::OPERATION,
                legacy_pipe::OPAQUE_POST_OPERATION,
            )
        } else {
            return None;
        };
    let is_hollow = match *frame.get(hollow_at)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(PipePrefix {
        operation: extrude_operation_at(frame, operation_at)?,
        operation_offset: u64::try_from(start.checked_add(operation_at)?).ok()?,
        is_hollow,
        is_hollow_offset: u64::try_from(start.checked_add(hollow_at)?).ok()?,
        opaque_post_operation: frame
            .get(opaque_at..opaque_at.checked_add(2)?)?
            .try_into()
            .ok()?,
        opaque_post_operation_offset: u64::try_from(start.checked_add(opaque_at)?).ok()?,
    })
}

fn pipe_owner_belongs_to_scope(scope: &DesignParameterScope, owner: &DesignParameterOwner) -> bool {
    native_stream(&scope.id).is_some_and(|stream| {
        native_stream(owner.id()) == Some(stream)
            && owner.scope_record_index() == Some(scope.record_index)
            && scope
                .reference_members()
                .values()
                .any(|value| value == &owner.record_index())
    })
}

fn exact_pipe_owner_lanes(
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<[(u32, FixedScalarFrame); 4]> {
    let mut owners = [None; 4];
    for owner in parameter_owners
        .iter()
        .filter(|owner| pipe_owner_belongs_to_scope(scope, owner))
    {
        let ordinal = usize::try_from(owner.local_ordinal()?).ok()?;
        let slot = owners.get_mut(ordinal)?;
        if slot.replace(owner).is_some() {
            return None;
        }
    }
    let [Some(first), Some(second), Some(third), Some(fourth)] = owners else {
        return None;
    };
    let lane = |owner: &DesignParameterOwner| {
        Some((
            owner.record_index(),
            FixedScalarFrame {
                owner_record_index: Some(scope.record_index),
                ordinal: u8::try_from(owner.local_ordinal()?).ok()?,
                value: owner.evaluated_value(),
                value_offset: owner.evaluated_value_offset()?,
            },
        ))
    };
    Some([lane(first)?, lane(second)?, lane(third)?, lane(fourth)?])
}

pub(super) fn exact_path_feature_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignPathFeatureConstruction> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let operation = |offset| {
        Some(match View::u32_le_at(bytes, offset)? {
            1 => DesignExtrudeOperation::Join,
            2 => DesignExtrudeOperation::Cut,
            3 => DesignExtrudeOperation::Intersect,
            4 => DesignExtrudeOperation::NewBody,
            _ => return None,
        })
    };
    match design_feature_family(&scope.kind())? {
        DesignFeatureFamily::Revolve
            if matches!(scope.reference_members().len(), 6 | 8)
                && bytes.get(start + revolve::MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + revolve::ZERO_VALUE) == Some(0)
                && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0)
                && View::u32_le_at(bytes, start + revolve::STRUCTURAL_CONSTANT) == Some(1) =>
        {
            let angle = unique_revolve_angle_owner(scope, parameter_owners, None)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + revolve::OPERATION).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    angle_record_index: angle.record_index(),
                    angle_offset: angle.evaluated_value_offset()?,
                    opposite_angle: None,
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if scope.frame_length() == 365
                && scope.reference_members().len() == 6
                && bytes.get(start + referenced_revolve::MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + referenced_revolve::ZERO_VALUE) == Some(0)
                && View::u32_le_at(bytes, start + referenced_revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + referenced_revolve::DIRECTION_KIND) == Some(&0)
                && bytes.get(start + referenced_revolve::FLAG) == Some(&1)
                && View::u32_le_at(bytes, start + referenced_revolve::STRUCTURAL_CONSTANT)
                    == Some(1) =>
        {
            let angle_record_index =
                marked_record_reference(bytes, start + referenced_revolve::ANGLE_REFERENCE_MARKER)?;
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index))?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + referenced_revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + referenced_revolve::OPERATION).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    angle_record_index,
                    angle_offset: angle.evaluated_value_offset()?,
                    opposite_angle: None,
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if parameter_scope_payload_length(scope) == Some(372)
                && scope.reference_members().len() == 7
                && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0) =>
        {
            let mut lanes = scope
                .reference_members()
                .values()
                .filter_map(|record_index| {
                    let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
                    (scalar.owner_record_index == Some(scope.record_index))
                        .then_some((*record_index, scalar))
                });
            let ((angle_record_index, angle), (opposite_angle_record_index, opposite)) =
                (lanes.next()?, lanes.next()?);
            if lanes.next().is_some() {
                return None;
            }
            if angle.ordinal != 0
                || opposite.ordinal != 1
                || angle.value.get() <= 0.0
                || opposite.value.get() != 0.0
            {
                return None;
            }
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + revolve::OPERATION).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.value.get())?,
                    angle_record_index,
                    angle_offset: angle.value_offset,
                    opposite_angle: Some(crate::records::identity::Located {
                        value: opposite_angle_record_index,
                        offset: opposite.value_offset,
                    }),
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if scope.class_tag.as_str() == "407"
                && scope.paired_class_tag.as_str() == "258"
                && parameter_scope_payload_length(scope) == Some(363)
                && scope.reference_members().len() == 8
                && View::u32_le_at(bytes, start + 25) == Some(2)
                && bytes.get(start + 29) == Some(&0)
                && View::u32_le_at(bytes, start + 30) == Some(1)
                && bytes.get(start + 34) == Some(&1)
                && bytes.get(start + 43..start + 45) == Some(&[0; 2]) =>
        {
            let angle_record_index = u32::try_from(View::u64_le_at(bytes, start + 35)?).ok()?;
            if scope.reference_members().values().nth(6) != Some(&angle_record_index) {
                return None;
            }
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index))?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + 21)?,
                    operation_offset: u64::try_from(start + 21).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    angle_record_index,
                    angle_offset: angle.evaluated_value_offset()?,
                    opposite_angle: None,
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if scope.class_tag.as_str() == "403"
                && scope.paired_class_tag.as_str() == "258"
                && scope.frame_length() == 387
                && scope.reference_members().len() == 8
                && View::u32_le_at(bytes, start + class_403_revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + class_403_revolve::DIRECTION_KIND..start + 31)
                    == Some(&[0, 1]) =>
        {
            let angle_record_index =
                marked_record_reference(bytes, start + class_403_revolve::ANGLE_REFERENCE_MARKER)?;
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index))?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + class_403_revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + class_403_revolve::OPERATION).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    angle_record_index,
                    angle_offset: angle.evaluated_value_offset()?,
                    opposite_angle: None,
                },
            ))
        }
        DesignFeatureFamily::Loft
            if bytes.get(start + compact_loft::ZERO_RUN_10..start + compact_loft::ONE_RUN_4)
                == Some(&[0; 10])
                && bytes.get(start + compact_loft::ONE_RUN_4..start + compact_loft::OPERATION)
                    == Some(&[1; 4])
                && bytes.get(start + compact_loft::ZERO_FLAG) == Some(&0)
                && View::u32_le_at(bytes, start + compact_loft::ALL_ONES) == Some(u32::MAX)
                && bytes.get(start + compact_loft::ZERO_RUN_11..start + compact_loft::LEN)
                    == Some(&[0; 11]) =>
        {
            Some(DesignPathFeatureConstruction::Loft(
                path_features::DesignLoftConstruction {
                    operation: operation(start + compact_loft::OPERATION)?,
                    operation_offset: u64::try_from(start + compact_loft::OPERATION).ok()?,
                },
            ))
        }
        DesignFeatureFamily::Loft
            if parameter_scope_payload_length(scope).is_some_and(|length| length >= 368) =>
        {
            Some(DesignPathFeatureConstruction::Loft(
                path_features::DesignLoftConstruction {
                    operation: operation(start + 29)?,
                    operation_offset: u64::try_from(start + 29).ok()?,
                },
            ))
        }
        DesignFeatureFamily::Sweep => {
            let mut candidate_lanes =
                scope
                    .reference_members()
                    .values()
                    .filter_map(|record_index| {
                        let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
                        (scalar.owner_record_index == Some(scope.record_index))
                            .then_some((*record_index, scalar))
                    });
            let lanes = [
                candidate_lanes.next()?,
                candidate_lanes.next()?,
                candidate_lanes.next()?,
                candidate_lanes.next()?,
                candidate_lanes.next()?,
                candidate_lanes.next()?,
            ];
            if candidate_lanes.next().is_some() {
                return None;
            }
            if lanes
                .iter()
                .enumerate()
                .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
            {
                return None;
            }
            Some(DesignPathFeatureConstruction::Sweep(
                path_features::DesignSweepConstruction {
                    operation: operation(start + 25)?,
                    operation_offset: u64::try_from(start + 25).ok()?,
                    values: lanes.map(|(_, scalar)| scalar.value),
                    record_indexes: lanes.map(|(record_index, _)| record_index),
                    value_offsets: lanes.map(|(_, scalar)| scalar.value_offset),
                },
            ))
        }
        DesignFeatureFamily::Pipe => {
            let end = usize::try_from(scope.paired_byte_offset()).ok()?;
            let prefix = exact_pipe_prefix(bytes.get(..end)?, start)?;
            let lanes = if parameter_owners
                .iter()
                .any(|owner| pipe_owner_belongs_to_scope(scope, owner))
            {
                exact_pipe_owner_lanes(scope, parameter_owners)?
            } else {
                let mut matched = [None; 4];
                for record_index in scope.reference_members().values() {
                    let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index) else {
                        continue;
                    };
                    if scalar.owner_record_index != Some(scope.record_index) {
                        continue;
                    }
                    let slot = matched.get_mut(usize::from(scalar.ordinal))?;
                    if slot.replace((*record_index, scalar)).is_some() {
                        return None;
                    }
                }
                let [Some(first), Some(second), Some(third), Some(fourth)] = matched else {
                    return None;
                };
                [first, second, third, fourth]
            };
            if lanes
                .iter()
                .enumerate()
                .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
            {
                return None;
            }
            Some(DesignPathFeatureConstruction::Pipe(
                path_features::DesignPipeConstruction {
                    operation: prefix.operation,
                    operation_offset: prefix.operation_offset,
                    is_hollow: prefix.is_hollow,
                    is_hollow_offset: prefix.is_hollow_offset,
                    opaque_post_operation: prefix.opaque_post_operation.into(),
                    opaque_post_operation_offset: prefix.opaque_post_operation_offset,
                    values: lanes.map(|(_, scalar)| scalar.value),
                    record_indexes: lanes.map(|(record_index, _)| record_index),
                    value_offsets: lanes.map(|(_, scalar)| scalar.value_offset),
                },
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
