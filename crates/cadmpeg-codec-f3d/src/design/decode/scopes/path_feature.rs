// SPDX-License-Identifier: Apache-2.0
//! Exact path-feature construction scopes.

use super::fixed_parameters::unique_revolve_angle_owner;
use super::parameter_scope::parameter_scope_payload_length;
use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::extrude_operation_at;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::layout::class_403_revolve_scope_frame as class_403_revolve;
use crate::layout::compact_loft_operation_prefix as compact_loft;
use crate::layout::marker_one_revolve_prologue as revolve;
use crate::layout::referenced_angle_revolve_prologue as referenced_revolve;
use crate::layout::two_sided_revolve_prologue as two_sided_revolve;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;

/// A Symmetric or Two Sides fixed-angle `Revolve` scope. It has the frame
/// length and reference count of the 372-byte one-sided form, whose arm
/// refuses it, so this form is tried first.
fn two_sided_revolve_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignPathFeatureConstruction> {
    if !matches!(
        design_feature_family(&scope.kind()),
        Some(DesignFeatureFamily::Revolve)
    ) {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let symmetric = match (
        bytes.get(start + two_sided_revolve::SYMMETRIC)?,
        bytes.get(start + two_sided_revolve::TWO_SIDES)?,
    ) {
        (1, 0) => true,
        (0, 1) => false,
        _ => return None,
    };
    if parameter_scope_payload_length(scope) != Some(372)
        || scope.reference_members().len() != 7
        || bytes.get(start + two_sided_revolve::MARKER) != Some(&1)
        || View::u32_le_at(bytes, start + two_sided_revolve::ZERO_VALUE) != Some(0)
        || View::u32_le_at(bytes, start + two_sided_revolve::EXTENT_KIND) != Some(2)
        || bytes.get(start + two_sided_revolve::DIRECTION_KIND) != Some(&0)
        || !matches!(
            bytes.get(start + two_sided_revolve::PROJECT_AXIS),
            Some(0 | 1)
        )
        || bytes.get(start + two_sided_revolve::SOLID) != Some(&1)
    {
        return None;
    }
    let opposite_record_index = marked_record_reference(
        bytes,
        start + two_sided_revolve::OPPOSITE_ANGLE_REFERENCE_MARKER,
    )?;
    let angle_record_index =
        marked_record_reference(bytes, start + two_sided_revolve::ANGLE_REFERENCE_MARKER)?;
    let angle = unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
    let opposite =
        unique_revolve_angle_owner(scope, parameter_owners, Some(opposite_record_index), 1)?;
    // Symmetric mirrors one angle; both owners then hold the same value.
    if symmetric && angle.evaluated_value() != opposite.evaluated_value() {
        return None;
    }
    let operation_offset = start + two_sided_revolve::OPERATION;
    Some(DesignPathFeatureConstruction::Revolve(
        path_features::DesignRevolveConstruction {
            operation: extrude_operation_at(bytes, operation_offset)?,
            operation_offset: u64::try_from(operation_offset).ok()?,
            angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
            angle_record_index,
            angle_offset: angle.evaluated_value_offset(),
            opposite_angle: None,
            second_side: Some(path_features::DesignRevolveSecondSide {
                symmetric,
                angle: cadmpeg_ir::scalar::PositiveAngle::new(opposite.evaluated_value().get())?,
                angle_record_index: opposite_record_index,
                angle_offset: opposite.evaluated_value_offset(),
            }),
        },
    ))
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
    if let Some(construction) = two_sided_revolve_construction(bytes, scope, parameter_owners) {
        return Some(construction);
    }
    match design_feature_family(&scope.kind())? {
        DesignFeatureFamily::Revolve
            if matches!(scope.reference_members().len(), 6 | 8)
                && bytes.get(start + revolve::MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + revolve::ZERO_VALUE) == Some(0)
                && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0)
                && View::u32_le_at(bytes, start + revolve::STRUCTURAL_CONSTANT) == Some(1) =>
        {
            let angle = unique_revolve_angle_owner(scope, parameter_owners, None, 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + revolve::OPERATION).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    angle_record_index: angle.record_index(),
                    angle_offset: angle.evaluated_value_offset(),
                    second_side: None,
                    opposite_angle: None,
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if parameter_scope_payload_length(scope) == Some(351)
                && scope.reference_members().len() == 6
                && bytes.get(start + referenced_revolve::MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + referenced_revolve::ZERO_VALUE) == Some(0)
                && View::u32_le_at(bytes, start + referenced_revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + referenced_revolve::DIRECTION_KIND) == Some(&0)
                && matches!(
                    bytes.get(start + referenced_revolve::PROJECT_AXIS),
                    Some(0 | 1)
                )
                && View::u32_le_at(bytes, start + referenced_revolve::STRUCTURAL_CONSTANT)
                    == Some(1) =>
        {
            let angle_record_index =
                marked_record_reference(bytes, start + referenced_revolve::ANGLE_REFERENCE_MARKER)?;
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + referenced_revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + referenced_revolve::OPERATION).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    angle_record_index,
                    angle_offset: angle.evaluated_value_offset(),
                    second_side: None,
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
                    second_side: None,
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
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + 21)?,
                    operation_offset: u64::try_from(start + 21).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    angle_record_index,
                    angle_offset: angle.evaluated_value_offset(),
                    second_side: None,
                    opposite_angle: None,
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if scope.class_tag.as_str() == "403"
                && scope.paired_class_tag.as_str() == "258"
                && parameter_scope_payload_length(scope) == Some(373)
                && scope.reference_members().len() == 8
                && View::u32_le_at(bytes, start + class_403_revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + class_403_revolve::DIRECTION_KIND..start + 31)
                    == Some(&[0, 1]) =>
        {
            let angle_record_index =
                marked_record_reference(bytes, start + class_403_revolve::ANGLE_REFERENCE_MARKER)?;
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + class_403_revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + class_403_revolve::OPERATION).ok()?,
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    angle_record_index,
                    angle_offset: angle.evaluated_value_offset(),
                    second_side: None,
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
        _ => None,
    }
}

#[cfg(test)]
mod tests;
