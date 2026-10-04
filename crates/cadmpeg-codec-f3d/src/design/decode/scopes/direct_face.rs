// SPDX-License-Identifier: Apache-2.0
//! Exact direct-face, move and scale operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::parameter_scope::parameter_scope_payload_length;
use super::point_data::exact_point_data_construction;
use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use super::thicken_shell::exact_legacy_thicken_class_347;
use super::thicken_shell::exact_shell_class_369_261;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::reference_runs::admit_reference_values;
use crate::design::decode::sketch::{indexed_record_header_at, IndexedRecordOffsets};
use crate::records::feature::body_ops::DesignScaleOperation;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::direct_face::DesignMoveOperation;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::identity::ReferenceRun;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

pub(super) fn exact_direct_face_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignDirectFaceOperation>, CodecError> {
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    let references = scope.reference_members();
    match scope.payload() {
        DesignScopePayload::Shell(_) | DesignScopePayload::Schale(_)
            if scope.class_tag.as_str() == "369" && scope.paired_class_tag.as_str() == "261" =>
        {
            exact_shell_class_369_261(ctx, bytes, records, scope)
        }
        DesignScopePayload::OffsetFaces(_) | DesignScopePayload::DecalerLesFaces(_) => {
            let payload_length = parameter_scope_payload_length(ctx, scope)?;
            if !matches!(
                (payload_length, references.len()),
                (Some(264), 4) | (Some(253), 3)
            ) || bytes.get(start + 25) != Some(&1)
            {
                return Ok(None);
            }
            let Some(distance_record_index) = View::u32_le_at(bytes, start + 26) else {
                return Ok(None);
            };
            if references.values().next_back() != Some(&distance_record_index) {
                return Ok(None);
            }
            let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, distance_record_index)?
            else {
                return Ok(None);
            };
            Ok(Some(DesignDirectFaceOperation::OffsetFaces(
                direct_face::DesignOffsetFacesOperation {
                    distance: scalar.value,
                    distance_record_index,
                    distance_offset: scalar.value_offset,
                },
            )))
        }
        DesignScopePayload::Thicken(_) if references.len() >= 3 => {
            if let Some(operation) = exact_legacy_thicken_class_347(ctx, bytes, records, scope)? {
                return Ok(Some(operation));
            }
            let payload_length = parameter_scope_payload_length(ctx, scope)?;
            let Some(thickness_record_index) =
                thicken_thickness_reference(bytes, start, payload_length, references)
            else {
                return Ok(None);
            };
            let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, thickness_record_index)?
            else {
                return Ok(None);
            };
            if scalar.value.get() == 0.0 {
                return Ok(None);
            }
            Ok(Some(DesignDirectFaceOperation::Thicken(
                direct_face::DesignThickenOperation {
                    signed_thickness: scalar.value,
                    thickness_record_index,
                    thickness_offset: scalar.value_offset,
                },
            )))
        }
        DesignScopePayload::Shell(_) | DesignScopePayload::Schale(_) => {
            let Some(references) = references.values_array::<3>() else {
                return Ok(None);
            };
            let payload_length = parameter_scope_payload_length(ctx, scope)?;
            let Some(layout) = shell_layout(bytes, start, payload_length, references) else {
                return Ok(None);
            };
            let Some(scalar) =
                exact_fixed_scalar(ctx, bytes, records, layout.thickness_record_index)?
            else {
                return Ok(None);
            };
            let Some(thickness) = cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get()) else {
                return Ok(None);
            };
            Ok(Some(DesignDirectFaceOperation::Shell(
                direct_face::DesignShellOperation {
                    thickness,
                    thickness_record_index: layout.thickness_record_index,
                    thickness_offset: scalar.value_offset,
                    outward: layout.outward,
                    outward_offset: u64_from_index(layout.outward_offset),
                },
            )))
        }
        _ => Ok(None),
    }
}

/// The thickness reference of a Thicken scope frame at `start`, which must be
/// the first or last scope reference as its layout requires.
fn thicken_thickness_reference(
    bytes: &[u8],
    start: usize,
    payload_length: Option<u64>,
    references: &ReferenceRun<u32>,
) -> Option<u32> {
    let first = references.values().next().copied();
    let last = references.values().next_back().copied();
    let grouped_length = u64::try_from(references.len().checked_sub(2)?)
        .ok()?
        .checked_mul(11)?
        .checked_add(276)?;
    let (reference_offset, thickness_is_first) = match payload_length? {
        length
            if length == grouped_length
                && bytes.get(start + 34) == Some(&1)
                && View::u32_le_at(bytes, start + 35) == references.values().nth(1).copied()
                && zeros_at::<6>(bytes, start + 39)
                && matches!(bytes.get(start + 45), Some(0 | 1))
                && bytes_at::<2>(bytes, start + 46) == Some(&[1, 1])
                && View::u32_le_at(bytes, start + 48) == first =>
        {
            (47, true)
        }
        281 if matches!(bytes.get(start + 45), Some(0 | 1))
            && bytes.get(start + 46) == Some(&1) =>
        {
            (46, false)
        }
        287 if bytes.get(start + 47) == Some(&1) => (47, false),
        _ => return None,
    };
    let thickness_record_index = View::u32_le_at(bytes, start + reference_offset + 1)?;
    let expected = if thickness_is_first { first } else { last };
    (expected == Some(thickness_record_index)).then_some(thickness_record_index)
}

struct ShellLayout {
    thickness_record_index: u32,
    outward: bool,
    outward_offset: usize,
}

/// The thickness reference and outward flag of a three-reference Shell scope
/// frame at `start`. The thickness reference is the first or last scope
/// reference as the layout requires.
fn shell_layout(
    bytes: &[u8],
    start: usize,
    payload_length: Option<u64>,
    [first, second, last]: [&u32; 3],
) -> Option<ShellLayout> {
    let (thickness_record_index, thickness_is_first, outward_offset) = match payload_length? {
        268 if zeros_at::<9>(bytes, start + 11)
            && bytes.get(start + 20) == Some(&1)
            && matches!(bytes.get(start + 21), Some(0 | 1))
            && zeros_at::<3>(bytes, start + 22)
            && bytes_at::<2>(bytes, start + 25) == Some(&[1, 0])
            && bytes.get(start + 27) == Some(&1)
            && zeros_at::<19>(bytes, start + 32)
            && View::u32_le_at(bytes, start + 51) == Some(1)
            && bytes.get(start + 55) == Some(&1)
            && View::u32_le_at(bytes, start + 56) == Some(*second)
            && zeros_at::<6>(bytes, start + 60) =>
        {
            (View::u32_le_at(bytes, start + 28)?, true, start + 21)
        }
        268 if matches!(bytes.get(start + 25), Some(0 | 1))
            && bytes.get(start + 26) == Some(&0)
            && bytes.get(start + 27) == Some(&1)
            && View::u32_le_at(bytes, start + 51) == Some(1)
            && bytes.get(start + 55) == Some(&1)
            && View::u32_le_at(bytes, start + 56) == Some(*first) =>
        {
            (View::u32_le_at(bytes, start + 28)?, false, start + 25)
        }
        258 if zeros_at::<10>(bytes, start + 11)
            && matches!(bytes.get(start + 21), Some(0 | 1))
            && bytes.get(start + 22) == Some(&1)
            && zeros_at::<15>(bytes, start + 27)
            && View::u32_le_at(bytes, start + 42) == Some(1)
            && bytes.get(start + 46) == Some(&1)
            && View::u32_le_at(bytes, start + 47) == Some(*first)
            && zeros_at::<6>(bytes, start + 51) =>
        {
            (View::u32_le_at(bytes, start + 23)?, false, start + 21)
        }
        _ => return None,
    };
    let expected = if thickness_is_first { first } else { last };
    if *expected != thickness_record_index {
        return None;
    }
    Some(ShellLayout {
        thickness_record_index,
        outward: *bytes.get(outward_offset)? != 0,
        outward_offset,
    })
}

/// The only Move transform among the frames of the scope's references.
pub(super) fn exact_move_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignMoveOperation>, CodecError> {
    if !matches!(scope.payload(), DesignScopePayload::Move(_)) {
        return Ok(None);
    }
    let mut candidate = None;
    for record_index in admit_reference_values(
        ctx,
        scope.reference_members(),
        "scan F3D Move scope reference members",
    )? {
        for (start, paired) in records.frames(ctx, *record_index)? {
            let Some(next) = move_frame_at(bytes, start, paired, *record_index) else {
                continue;
            };
            if candidate.replace(next).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(candidate)
}

/// The Move transform of the frame of `record_index` from `start` to its
/// paired header at `paired`. The admitted frame lengths hold the form word
/// and the matrix inside the frame.
fn move_frame_at(
    bytes: &[u8],
    start: usize,
    paired: usize,
    record_index: u32,
) -> Option<DesignMoveOperation> {
    let class_tag = exact_indexed_header_at(bytes, start, record_index)?;
    if !zeros_at::<32>(bytes, start + 11) {
        return None;
    }
    let (form_offset, transform_offset) = move_transform_layout(class_tag, paired - start)?;
    let expected_paired_class = match class_tag {
        b"447" => Some(b"263"),
        b"456" => Some(b"258"),
        _ => None,
    };
    if expected_paired_class.is_some_and(|expected| {
        indexed_record_header_at(bytes, paired).map(|header| header.class_tag) != Some(expected)
    }) || bytes.get(start + 47) != Some(&0)
    {
        return None;
    }
    let form =
        direct_face::DesignMoveForm::try_from(View::u32_le_at(bytes, start + form_offset)?).ok()?;
    let transform = rigid_transform_at(bytes, start + transform_offset)?;
    Some(DesignMoveOperation {
        transform,
        transform_offset: u64_from_index(start + transform_offset),
        transform_record_index: record_index,
        form,
        form_offset: u64_from_index(start + form_offset),
    })
}

/// Return the fixed envelope offsets admitted for one Move transform class.
///
/// The legacy classes carry the same matrix envelope as the current classes;
/// their class tags are the generation discriminator. Keeping the admission
/// keyed by class avoids treating an arbitrary 253-byte record as a transform.
fn move_transform_layout(class_tag: &[u8; 3], frame_length: usize) -> Option<(usize, usize)> {
    let admitted = match class_tag {
        b"296" | b"362" | b"433" | b"447" if frame_length == 253 => true,
        b"349" if matches!(frame_length, 254 | 274) => true,
        b"368" if frame_length == 254 => true,
        b"293" | b"393" | b"442" | b"451" | b"456" if frame_length == 253 => true,
        _ => false,
    };
    admitted.then_some((43, 48))
}

pub(super) fn exact_scale_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<DesignScaleOperation>, CodecError> {
    if !matches!(
        scope.payload(),
        DesignScopePayload::Scale(_) | DesignScopePayload::Massstab(_)
    ) {
        return Ok(None);
    }
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    let references = scope.reference_members();
    let (layout, center) =
        if references.len() == 5 && parameter_scope_payload_length(ctx, scope)? == Some(303) {
            let Some(layout) = scale_303_layout(bytes, start, references) else {
                return Ok(None);
            };
            let center = exact_point_data_construction(
                ctx,
                bytes,
                records,
                std::iter::once(layout.center_record_index),
                stream_types,
            )?;
            (layout, center)
        } else if matches!(scope.payload(), DesignScopePayload::Scale(_))
            && matches!(references.len(), 5 | 6)
            && scope.frame_length() == if references.len() == 5 { 307 } else { 318 }
        {
            let Some(layout) = scale_307_layout(bytes, start, references) else {
                return Ok(None);
            };
            let Some(center) = exact_point_data_construction(
                ctx,
                bytes,
                records,
                std::iter::once(layout.center_record_index),
                stream_types,
            )?
            else {
                return Ok(None);
            };
            (layout, Some(center))
        } else {
            return Ok(None);
        };
    let Some(uniform_factor) = View::f64_le_at(bytes, layout.uniform_factor_offset)
        .and_then(cadmpeg_ir::scalar::PositiveReal::new)
    else {
        return Ok(None);
    };
    Ok(Some(DesignScaleOperation {
        body_group_record_index: layout.body_group_record_index,
        center_record_index: layout.center_record_index,
        center_position: center.map(|point| crate::records::identity::Located {
            value: point.position,
            offset: point.position_offset,
        }),
        uniform_factor,
        uniform_factor_offset: u64_from_index(layout.uniform_factor_offset),
    }))
}

struct ScaleLayout {
    body_group_record_index: u32,
    center_record_index: u32,
    uniform_factor_offset: usize,
}

/// The 303-byte-payload Scale frame at `start`: factor, body group and
/// center references at fixed positions of a five-reference run.
fn scale_303_layout(
    bytes: &[u8],
    start: usize,
    references: &ReferenceRun<u32>,
) -> Option<ScaleLayout> {
    let [factor_record_index, body_group_record_index, _, _, center_record_index] =
        references.values_array()?;
    if View::u32_le_at(bytes, start + 20)? != 1
        || bytes.get(start + 24) != Some(&0)
        || marked_record_reference(bytes, start + 33)? != *center_record_index
        || marked_record_reference(bytes, start + 44)? != *factor_record_index
        || View::u32_le_at(bytes, start + 55)? != 1
        || bytes.get(start + 59) != Some(&0)
        || View::u32_le_at(bytes, start + 60)? != 1
        || View::u32_le_at(bytes, start + 64)? != 1
        || marked_record_reference(bytes, start + 68)? != *body_group_record_index
    {
        return None;
    }
    Some(ScaleLayout {
        body_group_record_index: *body_group_record_index,
        center_record_index: *center_record_index,
        uniform_factor_offset: start + 25,
    })
}

/// The 307- or 318-byte Scale frame at `start`: the factor and body group are
/// the first two references and the center is the last.
fn scale_307_layout(
    bytes: &[u8],
    start: usize,
    references: &ReferenceRun<u32>,
) -> Option<ScaleLayout> {
    let mut values = references.values();
    let factor_record_index = values.next()?;
    let body_group_record_index = values.next()?;
    let center_record_index = values.next_back()?;
    if !zeros_at::<5>(bytes, start + 16)
        || marked_record_reference(bytes, start + 29)? != *center_record_index
        || marked_record_reference(bytes, start + 40)? != *factor_record_index
        || View::u32_le_at(bytes, start + 51)? != 1
        || bytes.get(start + 55) != Some(&0)
        || View::u32_le_at(bytes, start + 56)? != 1
        || View::u32_le_at(bytes, start + 60)? != 1
        || marked_record_reference(bytes, start + 64)? != *body_group_record_index
    {
        return None;
    }
    Some(ScaleLayout {
        body_group_record_index: *body_group_record_index,
        center_record_index: *center_record_index,
        uniform_factor_offset: start + 21,
    })
}
