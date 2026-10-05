// SPDX-License-Identifier: Apache-2.0
//! Parse solid primitive construction frames and their parameter owners.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::extrude_operation_at;
use super::shared_frames::marked_record_reference;
use crate::bytes::f64s_at;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::reference_runs::reference_position;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_guid_end;
use crate::layout::named_solid_primitive_prologue as solid_prologue;
use crate::layout::shifted_cylinder_primitive_352_frame as shifted_cylinder_352;
use crate::layout::shifted_cylinder_primitive_502_frame as shifted_cylinder_502;
use crate::records::{
    feature::{
        extrude::DesignExtrudeOperation,
        primitives::DesignSolidPrimitive,
        scope::{DesignParameterScope, DesignScopePayload},
    },
    identity::{Located, ReferenceRun},
    parameters::DesignParameterOwner,
    sketch_placement::{valid_sketch_transform, SketchPlacementMatrix},
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

pub(super) fn exact_solid_primitive(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignSolidPrimitive>, CodecError> {
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    match scope.payload() {
        DesignScopePayload::SpherePrimitive(_) => {
            let Some(operation_offset) = start.checked_add(25) else {
                return Ok(None);
            };
            let Some(operation) = extrude_operation_at(bytes, operation_offset) else {
                return Ok(None);
            };
            if scope.frame_length() != 462
                || bytes.get(start + 29) != Some(&1)
                || bytes.get(start + 30) != Some(&1)
                || bytes.get(start + 41) != Some(&1)
                || bytes.get(start + 52) != Some(&1)
            {
                return Ok(None);
            }
            let Some(diameter_record_index) = View::u32_le_at(bytes, start + 42) else {
                return Ok(None);
            };
            let Some((diameter, diameter_offset)) =
                exact_primitive_diameter(ctx, bytes, records, diameter_record_index)?
            else {
                return Ok(None);
            };
            let Some((transform, transform_offset)) = primitive_matrix_at(bytes, start, 64) else {
                return Ok(None);
            };
            Ok(Some(DesignSolidPrimitive::Sphere(
                crate::records::feature::primitives::DesignSpherePrimitive {
                    transform,
                    transform_offset,
                    diameter,
                    diameter_record_index,
                    diameter_offset,
                    operation,
                    operation_offset: u64_from_index(operation_offset),
                },
            )))
        }
        DesignScopePayload::TorusPrimitive(_) => {
            let Some(operation_offset) = start.checked_add(25) else {
                return Ok(None);
            };
            let Some(operation) = extrude_operation_at(bytes, operation_offset) else {
                return Ok(None);
            };
            if scope.frame_length() != 486
                || bytes.get(start + 29) != Some(&1)
                || bytes.get(start + 30) != Some(&1)
                || bytes.get(start + 41) != Some(&1)
                || bytes.get(start + 52) != Some(&1)
                || bytes.get(start + 63) != Some(&1)
            {
                return Ok(None);
            }
            let (Some(major_diameter_record_index), Some(minor_diameter_record_index)) = (
                View::u32_le_at(bytes, start + 31),
                View::u32_le_at(bytes, start + 53),
            ) else {
                return Ok(None);
            };
            if major_diameter_record_index == minor_diameter_record_index {
                return Ok(None);
            }
            let Some((major_diameter, major_diameter_offset)) =
                exact_primitive_diameter(ctx, bytes, records, major_diameter_record_index)?
            else {
                return Ok(None);
            };
            let Some((minor_diameter, minor_diameter_offset)) =
                exact_primitive_diameter(ctx, bytes, records, minor_diameter_record_index)?
            else {
                return Ok(None);
            };
            let Some((transform, transform_offset)) = primitive_matrix_at(bytes, start, 75) else {
                return Ok(None);
            };
            Ok(Some(DesignSolidPrimitive::Torus(
                crate::records::feature::primitives::DesignTorusPrimitive {
                    transform,
                    transform_offset,
                    major_diameter,
                    major_diameter_record_index,
                    major_diameter_offset,
                    minor_diameter,
                    minor_diameter_record_index,
                    minor_diameter_offset,
                    operation,
                    operation_offset: u64_from_index(operation_offset),
                },
            )))
        }
        DesignScopePayload::BoxPrimitive(_) => {
            let Some(operation_offset) = exact_named_solid_primitive_operation(bytes, start) else {
                return Ok(None);
            };
            let Some(operation) = extrude_operation_at(bytes, operation_offset) else {
                return Ok(None);
            };
            if scope.frame_length() < 78 || scope.reference_members().len() < 5 {
                return Ok(None);
            }
            let Some([Some(length), Some(width), Some(height), Some(offset_x), Some(offset_y)]) =
                exact_owned_primitive_parameters::<5>(ctx, scope, parameter_owners)?
            else {
                return Ok(None);
            };
            let (Some(length_value), Some(width_value), Some(height_value)) = (
                PositiveReal::new(length.evaluated_value().get()),
                PositiveReal::new(width.evaluated_value().get()),
                PositiveReal::new(height.evaluated_value().get()),
            ) else {
                return Ok(None);
            };
            Ok(Some(DesignSolidPrimitive::Box(
                crate::records::feature::primitives::DesignBoxPrimitive {
                    length: length_value,
                    length_record_index: length.record_index(),
                    length_offset: length.evaluated_value_offset(),
                    width: width_value,
                    width_record_index: width.record_index(),
                    width_offset: width.evaluated_value_offset(),
                    height: height_value,
                    height_record_index: height.record_index(),
                    height_offset: height.evaluated_value_offset(),
                    offset_x: offset_x.evaluated_value(),
                    offset_x_record_index: offset_x.record_index(),
                    offset_x_offset: offset_x.evaluated_value_offset(),
                    offset_y: offset_y.evaluated_value(),
                    offset_y_record_index: offset_y.record_index(),
                    offset_y_offset: offset_y.evaluated_value_offset(),
                    operation,
                    operation_offset: u64_from_index(operation_offset),
                },
            )))
        }
        DesignScopePayload::CylinderPrimitive(_) => {
            let prologue = match exact_named_solid_primitive_operation(bytes, start) {
                Some(operation_offset) => {
                    extrude_operation_at(bytes, operation_offset).map(|operation| {
                        ExactShiftedCylinderPrimitivePrologue {
                            operation,
                            operation_offset,
                            transform: None,
                        }
                    })
                }
                None => exact_shifted_cylinder_primitive_prologue(bytes, scope, start),
            };
            let Some(prologue) = prologue else {
                return Ok(None);
            };
            if scope.frame_length() < 78 || scope.reference_members().len() < 2 {
                return Ok(None);
            }
            let Some([Some(height), Some(diameter)]) =
                exact_owned_primitive_parameters::<2>(ctx, scope, parameter_owners)?
            else {
                return Ok(None);
            };
            let (Some(height_value), Some(diameter_value)) = (
                PositiveReal::new(height.evaluated_value().get()),
                PositiveReal::new(diameter.evaluated_value().get()),
            ) else {
                return Ok(None);
            };
            Ok(Some(DesignSolidPrimitive::Cylinder(
                crate::records::feature::primitives::DesignCylinderPrimitive {
                    height: height_value,
                    height_record_index: height.record_index(),
                    height_offset: height.evaluated_value_offset(),
                    diameter: diameter_value,
                    diameter_record_index: diameter.record_index(),
                    diameter_offset: diameter.evaluated_value_offset(),
                    transform: prologue.transform,
                    operation: prologue.operation,
                    operation_offset: u64_from_index(prologue.operation_offset),
                },
            )))
        }
        _ => Ok(None),
    }
}

/// The finite placement matrix of sixteen little-endian scalars at
/// `relative_offset` in the frame from `start`, and its absolute offset.
fn primitive_matrix_at(
    bytes: &[u8],
    start: usize,
    relative_offset: usize,
) -> Option<(SketchPlacementMatrix, u64)> {
    let matrix_at = start.checked_add(relative_offset)?;
    let values = f64s_at::<16>(bytes, matrix_at)?;
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.into_iter().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    Some((
        SketchPlacementMatrix::try_from(transform).ok()?,
        u64_from_index(matrix_at),
    ))
}

#[derive(Clone, Copy)]
struct ExactShiftedCylinderPrimitivePrologue {
    operation: DesignExtrudeOperation,
    operation_offset: usize,
    transform: Option<Located<SketchPlacementMatrix>>,
}

fn exact_named_solid_primitive_operation(bytes: &[u8], start: usize) -> Option<usize> {
    if !zeros_at::<{ solid_prologue::OPERATION - solid_prologue::ZERO_RUN_9 }>(
        bytes,
        start + solid_prologue::ZERO_RUN_9,
    ) || bytes.get(start + solid_prologue::ZERO_FLAG) != Some(&0)
        || bytes.get(start + solid_prologue::FORM_MARKER) != Some(&1)
    {
        return None;
    }
    start.checked_add(solid_prologue::OPERATION)
}

/// The first reference member and the last four, last first, of a run of
/// exactly `N` members.
fn shifted_cylinder_references<const N: usize>(
    references: &ReferenceRun<u32>,
) -> Option<(u32, [u32; 4])> {
    let values = references.values_array::<N>()?;
    let first = **values.first()?;
    let [.., fourth, third, second, last] = values.as_slice() else {
        return None;
    };
    Some((first, [**last, **second, **third, **fourth]))
}

fn exact_shifted_cylinder_primitive_prologue(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<ExactShiftedCylinderPrimitivePrologue> {
    let compact = match (
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length(),
    ) {
        ("297" | "375", "258", 352) => true,
        ("297" | "375", "258", 502) | ("414", "272", 502) => false,
        _ => return None,
    };
    let prologue = shifted_cylinder_prologue_frame(bytes, scope, start, compact)?;
    let guid_count_at = start
        + if compact {
            shifted_cylinder_352::GUID_CODE_UNIT_COUNT
        } else {
            shifted_cylinder_502::GUID_CODE_UNIT_COUNT
        };
    let guid_end_at = start
        + if compact {
            shifted_cylinder_352::ZERO_RUN_3_AFTER_GUID
        } else {
            shifted_cylinder_502::ZERO_RUN_3_AFTER_GUID
        };
    if fixed_guid_end(bytes, guid_count_at) != Some(guid_end_at) {
        return None;
    }
    Some(prologue)
}

/// The fixed fields of a shifted cylinder frame other than its GUID text.
fn shifted_cylinder_prologue_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
    compact: bool,
) -> Option<ExactShiftedCylinderPrimitivePrologue> {
    let (
        frame_length,
        zero_run_10,
        form_marker,
        operation,
        first_reference,
        second_reference,
        third_reference,
        fourth_reference,
        reference_gap,
    ) = if compact {
        (
            shifted_cylinder_352::LEN,
            shifted_cylinder_352::ZERO_RUN_10,
            shifted_cylinder_352::FORM_MARKER,
            shifted_cylinder_352::OPERATION,
            shifted_cylinder_352::FIRST_REFERENCE,
            shifted_cylinder_352::SECOND_REFERENCE,
            shifted_cylinder_352::THIRD_REFERENCE,
            shifted_cylinder_352::FOURTH_REFERENCE,
            shifted_cylinder_352::REFERENCE_GAP,
        )
    } else {
        (
            shifted_cylinder_502::LEN,
            shifted_cylinder_502::ZERO_RUN_10,
            shifted_cylinder_502::FORM_MARKER,
            shifted_cylinder_502::OPERATION,
            shifted_cylinder_502::FIRST_REFERENCE,
            shifted_cylinder_502::SECOND_REFERENCE,
            shifted_cylinder_502::THIRD_REFERENCE,
            shifted_cylinder_502::FOURTH_REFERENCE,
            shifted_cylinder_502::REFERENCE_GAP,
        )
    };
    // The compact form lists five references and the expanded form seven.
    let (first_member, last_members) = if compact {
        shifted_cylinder_references::<5>(scope.reference_members())?
    } else {
        shifted_cylinder_references::<7>(scope.reference_members())?
    };
    if scope.paired_byte_offset() != u64::try_from(start.checked_add(frame_length)?).ok()?
        || !zeros_at::<10>(bytes, start + zero_run_10)
        || bytes.get(start + form_marker) != Some(&1)
        || bytes.get(start + reference_gap) != Some(&0)
        || !zeros_at::<3>(bytes, start + operation + 1)
    {
        return None;
    }
    let operation_offset = start.checked_add(operation)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let [last, second_last, third_last, fourth_last] = last_members;
    if bytes.get(start + first_reference) != Some(&1)
        || bytes.get(start + first_reference + 1) != Some(&1)
        || View::u32_le_at(bytes, start + first_reference + 2)? != last
        || !zeros_at::<5>(bytes, start + first_reference + 6)
    {
        return None;
    }
    for (relative_offset, expected_record_index) in [
        (second_reference, second_last),
        (third_reference, third_last),
        (fourth_reference, fourth_last),
    ] {
        if marked_record_reference(bytes, start.checked_add(relative_offset)?)
            != Some(expected_record_index)
        {
            return None;
        }
    }
    let absolute = |relative_offset: usize| u64::try_from(start.checked_add(relative_offset)?).ok();
    let (
        reference_count_offset,
        kind_offset,
        feature_ordinal_offset,
        previous_history_state_id_offset,
    ) = if compact {
        (
            shifted_cylinder_352::REFERENCE_COUNT,
            shifted_cylinder_352::KIND,
            shifted_cylinder_352::FEATURE_ORDINAL,
            shifted_cylinder_352::PREVIOUS_HISTORY_STATE_ID,
        )
    } else {
        (
            shifted_cylinder_502::REFERENCE_COUNT,
            shifted_cylinder_502::KIND,
            shifted_cylinder_502::FEATURE_ORDINAL,
            shifted_cylinder_502::PREVIOUS_HISTORY_STATE_ID,
        )
    };
    if scope.reference_count_offset() != absolute(reference_count_offset)?
        || scope.kind_offset() != absolute(kind_offset)?
        || scope.feature_ordinal_offset() != absolute(feature_ordinal_offset)?
        || scope.previous_history_state_id_offset()
            != Some(absolute(previous_history_state_id_offset)?)
    {
        return None;
    }
    let transform = if compact {
        if bytes.get(start + shifted_cylinder_352::COMPACT_TAIL_MARKER) != Some(&1)
            || View::u32_le_at(bytes, start + shifted_cylinder_352::COMPACT_TAIL_COUNT)? != 1
            || marked_record_reference(bytes, start + shifted_cylinder_352::COMPACT_TAIL_REFERENCE)
                .is_none_or(|record_index| record_index == 0)
            || !zeros_at::<8>(bytes, start + shifted_cylinder_352::COMPACT_TAIL_ZERO_RUN_8)
            || View::u32_le_at(bytes, start + shifted_cylinder_352::GUID_CODE_UNIT_COUNT)? != 36
            || !zeros_at::<3>(bytes, start + shifted_cylinder_352::ZERO_RUN_3_AFTER_GUID)
        {
            return None;
        }
        None
    } else {
        if bytes.get(start + shifted_cylinder_502::ZERO_BEFORE_MATRIX) != Some(&0)
            || !zeros_at::<8>(bytes, start + shifted_cylinder_502::ZERO_RUN_8_AFTER_MATRIX)
            || bytes.get(start + shifted_cylinder_502::CONSTRUCTION_REFERENCE) != Some(&1)
            || View::u32_le_at(
                bytes,
                start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 1,
            )? != 0x0100_0000
            || View::u32_le_at(
                bytes,
                start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 5,
            )? != first_member
            || !zeros_at::<6>(
                bytes,
                start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 9,
            )
            || View::u32_le_at(bytes, start + shifted_cylinder_502::GUID_CODE_UNIT_COUNT)? != 36
            || !zeros_at::<3>(bytes, start + shifted_cylinder_502::ZERO_RUN_3_AFTER_GUID)
        {
            return None;
        }
        let values = f64s_at::<16>(bytes, start + shifted_cylinder_502::MATRIX)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        if !valid_sketch_transform(&transform)
            || !cylinder_transform_preserves_projected_geometry(&transform)
        {
            return None;
        }
        Some(Located {
            value: SketchPlacementMatrix::try_from(transform).ok()?,
            offset: u64::try_from(start + shifted_cylinder_502::MATRIX).ok()?,
        })
    };
    Some(ExactShiftedCylinderPrimitivePrologue {
        operation,
        operation_offset,
        transform,
    })
}

fn cylinder_transform_preserves_projected_geometry(transform: &[[f64; 4]; 4]) -> bool {
    const EPS_CYLINDER_FRAME: f64 = 1.0e-10;
    transform[0][3].abs() <= EPS_CYLINDER_FRAME
        && transform[1][3].abs() <= EPS_CYLINDER_FRAME
        && transform[2][3].abs() <= EPS_CYLINDER_FRAME
        && transform[0][2].abs() <= EPS_CYLINDER_FRAME
        && transform[1][2].abs() <= EPS_CYLINDER_FRAME
        && (transform[2][2] - 1.0).abs() <= EPS_CYLINDER_FRAME
}

/// The scope's parameter owners in its stream that it lists as references,
/// indexed by local ordinal. A duplicate ordinal or an ordinal past `N`
/// refuses the scope.
fn exact_owned_primitive_parameters<'a, const N: usize>(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
) -> Result<Option<[Option<&'a DesignParameterOwner>; N]>, CodecError> {
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let mut owner_positions = [None; N];
    let mut position = 0usize;
    let refused = ctx.position_by(
        parameter_owners,
        |owner| {
            let at = position;
            position += 1;
            if owner.scope_record_index() != scope.record_index
                || !in_stream(ctx, owner.id(), stream)?
                || reference_position(
                    ctx,
                    scope.reference_members(),
                    |value| Ok(*value == owner.record_index()),
                    "find F3D primitive owner reference member",
                )?
                .is_none()
            {
                return Ok(false);
            }
            let slot = usize::try_from(owner.local_ordinal())
                .ok()
                .and_then(|ordinal| owner_positions.get_mut(ordinal));
            Ok(slot.is_none_or(|slot| slot.replace(at).is_some()))
        },
        "scan F3D primitive parameter owners",
    )?;
    if refused.is_some() {
        return Ok(None);
    }
    Ok(Some(
        owner_positions.map(|at| at.and_then(|at| parameter_owners.get(at))),
    ))
}

fn exact_primitive_diameter(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Result<Option<(PositiveReal, u64)>, CodecError> {
    Ok(exact_fixed_scalar(ctx, bytes, records, record_index)?
        .and_then(|scalar| Some((PositiveReal::new(scalar.value.get())?, scalar.value_offset))))
}

#[cfg(test)]
mod tests;
