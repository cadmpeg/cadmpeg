// SPDX-License-Identifier: Apache-2.0
//! Exact path-feature construction scopes and pipe owner lanes.

use super::fixed_parameters::unique_revolve_angle_owner;
use super::parameter_scope::parameter_scope_payload_length;
use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::extrude_operation_at;
use super::shared_frames::marked_record_reference;
use super::shared_frames::FixedScalarFrame;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::reference_runs::reference_position;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::class_403_revolve_scope_frame as class_403_revolve;
use crate::layout::compact_loft_operation_prefix as compact_loft;
use crate::layout::fixed_pipe_operation_prefix as fixed_pipe;
use crate::layout::legacy_pipe_operation_prefix as legacy_pipe;
use crate::layout::marker_one_revolve_prologue as revolve;
use crate::records::feature::path_features;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::feature::surface_ops;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

/// The four pipe lanes carried by class-342 parameter owners of the scope,
/// one owner for each local ordinal. Each owner must belong to the scope's
/// stream and be one of its reference members. The search stops at the first
/// owner whose ordinal repeats or falls outside the four lanes.
fn exact_pipe_owner_lanes(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<[(u32, FixedScalarFrame); 4]>, CodecError> {
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let mut lanes = [None; 4];
    let rejected = ctx.any_by(
        parameter_owners,
        |owner| {
            if owner.scope_record_index() != scope.record_index
                || owner.class_tag().as_str() != "342"
                || owner.frame_length() != 103
                || !in_stream(ctx, owner.id(), stream)?
                || reference_position(
                    ctx,
                    scope.reference_members(),
                    |value| Ok(*value == owner.record_index()),
                    "find F3D pipe owner reference member",
                )?
                .is_none()
            {
                return Ok(false);
            }
            let (Some(slot), Ok(ordinal)) = (
                usize::try_from(owner.local_ordinal())
                    .ok()
                    .and_then(|ordinal| lanes.get_mut(ordinal)),
                u8::try_from(owner.local_ordinal()),
            ) else {
                return Ok(true);
            };
            let lane = FixedScalarFrame {
                owner_record_index: Some(scope.record_index),
                ordinal,
                value: owner.evaluated_value(),
                value_offset: owner.evaluated_value_offset(),
            };
            Ok(slot.replace((owner.record_index(), lane)).is_some())
        },
        "scan F3D pipe parameter owners",
    )?;
    let [Some(first), Some(second), Some(third), Some(fourth)] = lanes else {
        return Ok(None);
    };
    Ok((!rejected).then_some([first, second, third, fourth]))
}

/// One optional owned fixed scalar per lane, with its record index.
type ScalarLanes<const N: usize> = [Option<(u32, FixedScalarFrame)>; N];

/// The first `N` reference members, in member order, whose fixed scalar frame
/// the scope owns. `None` when a further member's frame is owned too; a slot
/// stays empty when fewer members qualify. The search stops at the member
/// after the `N`th lane.
fn owned_scalar_lanes<const N: usize>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    operation: &'static str,
) -> Result<Option<ScalarLanes<N>>, CodecError> {
    let mut lanes = [None; N];
    let mut count = 0;
    let extra = reference_position(
        ctx,
        scope.reference_members(),
        |record_index| {
            let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, *record_index)? else {
                return Ok(false);
            };
            if scalar.owner_record_index != Some(scope.record_index) {
                return Ok(false);
            }
            let Some(slot) = lanes.get_mut(count) else {
                return Ok(true);
            };
            *slot = Some((*record_index, scalar));
            count += 1;
            Ok(false)
        },
        operation,
    )?;
    Ok(extra.is_none().then_some(lanes))
}

/// Whether the lanes carry local ordinals zero, one, and so on in order.
fn ordinals_in_order(lanes: &[(u32, FixedScalarFrame)]) -> bool {
    lanes
        .iter()
        .enumerate()
        .all(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) == ordinal)
}

pub(super) fn exact_path_feature_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignPathFeatureConstruction>, CodecError> {
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    match scope.payload() {
        DesignScopePayload::Revolve(_) => {
            exact_revolve_construction(ctx, bytes, records, scope, parameter_owners, start)
        }
        DesignScopePayload::Loft(_) => exact_loft_construction(ctx, bytes, scope, start),
        DesignScopePayload::Sweep(_) => exact_sweep_construction(ctx, bytes, records, scope, start),
        DesignScopePayload::Pipe(_) => {
            exact_pipe_construction(ctx, bytes, records, scope, parameter_owners, start)
        }
        _ => Ok(None),
    }
}

/// A revolve whose operation code is at `operation_at`.
fn revolve_construction(
    bytes: &[u8],
    operation_at: usize,
    angle: f64,
    angle_record_index: u32,
    angle_offset: u64,
    opposite_angle: Option<crate::records::identity::Located<u32>>,
) -> Option<DesignPathFeatureConstruction> {
    Some(DesignPathFeatureConstruction::Revolve(
        path_features::DesignRevolveConstruction {
            operation: extrude_operation_at(bytes, operation_at)?,
            operation_offset: u64_from_index(operation_at),
            angle: cadmpeg_ir::scalar::PositiveAngle::new(angle)?,
            angle_record_index,
            angle_offset,
            opposite_angle,
        },
    ))
}

/// The first revolve form whose fixed fields match the scope decides the
/// result.
fn exact_revolve_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
    start: usize,
) -> Result<Option<DesignPathFeatureConstruction>, CodecError> {
    let members = scope.reference_members();
    if matches!(members.len(), 6 | 8)
        && bytes.get(start + revolve::MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + revolve::ZERO_VALUE) == Some(0)
        && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
        && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0)
        && View::u32_le_at(bytes, start + revolve::STRUCTURAL_CONSTANT) == Some(1)
    {
        let Some(angle) = unique_revolve_angle_owner(ctx, scope, parameter_owners, None)? else {
            return Ok(None);
        };
        return Ok(revolve_construction(
            bytes,
            start + revolve::OPERATION,
            angle.evaluated_value().get(),
            angle.record_index(),
            angle.evaluated_value_offset(),
            None,
        ));
    }
    let payload_length = parameter_scope_payload_length(ctx, scope)?;
    if payload_length == Some(372)
        && members.len() == 7
        && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
        && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0)
    {
        let Some([Some((angle_record_index, angle)), Some((opposite_record_index, opposite))]) =
            owned_scalar_lanes::<2>(
                ctx,
                bytes,
                records,
                scope,
                "scan F3D revolve angle reference members",
            )?
        else {
            return Ok(None);
        };
        if angle.ordinal != 0
            || opposite.ordinal != 1
            || angle.value.get() <= 0.0
            || opposite.value.get() != 0.0
        {
            return Ok(None);
        }
        return Ok(revolve_construction(
            bytes,
            start + revolve::OPERATION,
            angle.value.get(),
            angle_record_index,
            angle.value_offset,
            Some(crate::records::identity::Located {
                value: opposite_record_index,
                offset: opposite.value_offset,
            }),
        ));
    }
    if scope.class_tag.as_str() == "407"
        && scope.paired_class_tag.as_str() == "258"
        && payload_length == Some(363)
        && View::u32_le_at(bytes, start + 25) == Some(2)
        && bytes.get(start + 29) == Some(&0)
        && View::u32_le_at(bytes, start + 30) == Some(1)
        && bytes.get(start + 34) == Some(&1)
        && zeros_at::<2>(bytes, start + 43)
    {
        let Some([_, _, _, _, _, _, angle_member, _]) = members.values_array::<8>() else {
            return Ok(None);
        };
        let angle_record_index = View::u64_le_at(bytes, start + 35)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| value == angle_member);
        let Some(angle_record_index) = angle_record_index else {
            return Ok(None);
        };
        let Some(angle) =
            unique_revolve_angle_owner(ctx, scope, parameter_owners, Some(angle_record_index))?
        else {
            return Ok(None);
        };
        return Ok(revolve_construction(
            bytes,
            start + 21,
            angle.evaluated_value().get(),
            angle_record_index,
            angle.evaluated_value_offset(),
            None,
        ));
    }
    if scope.class_tag.as_str() == "403"
        && scope.paired_class_tag.as_str() == "258"
        && scope.frame_length() == 387
        && members.len() == 8
        && View::u32_le_at(bytes, start + class_403_revolve::EXTENT_KIND) == Some(2)
        && bytes_at::<2>(bytes, start + class_403_revolve::DIRECTION_KIND) == Some(&[0, 1])
    {
        let Some(angle_record_index) =
            marked_record_reference(bytes, start + class_403_revolve::ANGLE_REFERENCE_MARKER)
        else {
            return Ok(None);
        };
        let Some(angle) =
            unique_revolve_angle_owner(ctx, scope, parameter_owners, Some(angle_record_index))?
        else {
            return Ok(None);
        };
        return Ok(revolve_construction(
            bytes,
            start + class_403_revolve::OPERATION,
            angle.evaluated_value().get(),
            angle_record_index,
            angle.evaluated_value_offset(),
            None,
        ));
    }
    Ok(None)
}

fn exact_loft_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignPathFeatureConstruction>, CodecError> {
    let operation_at = if zeros_at::<{ compact_loft::ONE_RUN_4 - compact_loft::ZERO_RUN_10 }>(
        bytes,
        start + compact_loft::ZERO_RUN_10,
    ) && bytes_at::<{ compact_loft::OPERATION - compact_loft::ONE_RUN_4 }>(
        bytes,
        start + compact_loft::ONE_RUN_4,
    ) == Some(&[1; 4])
        && bytes.get(start + compact_loft::ZERO_FLAG) == Some(&0)
        && View::u32_le_at(bytes, start + compact_loft::ALL_ONES) == Some(u32::MAX)
        && zeros_at::<{ compact_loft::LEN - compact_loft::ZERO_RUN_11 }>(
            bytes,
            start + compact_loft::ZERO_RUN_11,
        ) {
        start + compact_loft::OPERATION
    } else if parameter_scope_payload_length(ctx, scope)?.is_some_and(|length| length >= 368) {
        start + 29
    } else {
        return Ok(None);
    };
    Ok(extrude_operation_at(bytes, operation_at).map(|operation| {
        DesignPathFeatureConstruction::Loft(path_features::DesignLoftConstruction {
            operation,
            operation_offset: u64_from_index(operation_at),
        })
    }))
}

fn exact_sweep_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignPathFeatureConstruction>, CodecError> {
    let Some([Some(first), Some(second), Some(third), Some(fourth), Some(fifth), Some(sixth)]) =
        owned_scalar_lanes::<6>(
            ctx,
            bytes,
            records,
            scope,
            "scan F3D sweep reference members",
        )?
    else {
        return Ok(None);
    };
    let lanes = [first, second, third, fourth, fifth, sixth];
    if !ordinals_in_order(&lanes) {
        return Ok(None);
    }
    Ok(extrude_operation_at(bytes, start + 25).map(|operation| {
        DesignPathFeatureConstruction::Sweep(path_features::DesignSweepConstruction {
            operation,
            operation_offset: u64_from_index(start + 25),
            values: lanes.map(|(_, scalar)| scalar.value),
            record_indexes: lanes.map(|(record_index, _)| record_index),
            value_offsets: lanes.map(|(_, scalar)| scalar.value_offset),
        })
    }))
}

fn exact_pipe_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
    start: usize,
) -> Result<Option<DesignPathFeatureConstruction>, CodecError> {
    let legacy_prefix_layout = matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("405", "259") | ("475", "260")
    );
    let owner_layout = matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("421", "257")
    );
    if legacy_prefix_layout
        && (!zeros_at::<{ legacy_pipe::PREFIX_MARKER - legacy_pipe::ZERO_RUN_9 }>(
            bytes,
            start + legacy_pipe::ZERO_RUN_9,
        ) || bytes.get(start + legacy_pipe::PREFIX_MARKER)
            != Some(&legacy_pipe::PREFIX_MARKER_VALUE)
            || !zeros_at::<{ legacy_pipe::OPERATION - legacy_pipe::ZERO_RUN_5 }>(
                bytes,
                start + legacy_pipe::ZERO_RUN_5,
            ))
    {
        return Ok(None);
    }
    let lanes = if owner_layout {
        exact_pipe_owner_lanes(ctx, scope, parameter_owners)?
    } else {
        match owned_scalar_lanes::<4>(
            ctx,
            bytes,
            records,
            scope,
            "scan F3D pipe reference members",
        )? {
            Some([Some(a), Some(b), Some(c), Some(d)]) => Some([a, b, c, d]),
            _ => None,
        }
    };
    let Some(lanes) = lanes.filter(|lanes| ordinals_in_order(lanes)) else {
        return Ok(None);
    };
    let (operation_offset, section_shape_offset, filled_offset) = if legacy_prefix_layout {
        (
            start + legacy_pipe::OPERATION,
            start + legacy_pipe::SECTION_SHAPE,
            start + legacy_pipe::FILLED,
        )
    } else {
        (
            start + fixed_pipe::OPERATION,
            start + fixed_pipe::SECTION_SHAPE,
            start + fixed_pipe::FILLED,
        )
    };
    let filled = match bytes.get(filled_offset) {
        Some(0) => false,
        Some(1) => true,
        _ => return Ok(None),
    };
    let (Some(&section_shape), Some(operation)) = (
        bytes.get(section_shape_offset),
        extrude_operation_at(bytes, operation_offset),
    ) else {
        return Ok(None);
    };
    Ok(Some(DesignPathFeatureConstruction::Pipe(
        path_features::DesignPipeConstruction {
            operation,
            operation_offset: u64_from_index(operation_offset),
            section_shape: surface_ops::DesignPipeSectionShape::from_code(section_shape),
            section_shape_offset: u64_from_index(section_shape_offset),
            filled,
            filled_offset: u64_from_index(filled_offset),
            values: lanes.map(|(_, scalar)| scalar.value),
            record_indexes: lanes.map(|(record_index, _)| record_index),
            value_offsets: lanes.map(|(_, scalar)| scalar.value_offset),
        },
    )))
}

#[cfg(test)]
mod tests;
