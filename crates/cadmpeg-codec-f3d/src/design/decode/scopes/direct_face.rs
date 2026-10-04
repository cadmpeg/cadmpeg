// SPDX-License-Identifier: Apache-2.0
//! Exact direct-face, move and scale operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::parameter_scope::parameter_scope_payload_length;
use super::point_data::exact_point_data_construction;
use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use super::thicken_shell::exact_legacy_thicken_class_347;
use super::thicken_shell::exact_shell_class_369_261;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::records::feature::body_ops::DesignScaleOperation;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::direct_face::DesignMoveOperation;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use std::collections::HashMap;

pub(super) fn exact_direct_face_operation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignDirectFaceOperation>, cadmpeg_core::CodecError> {
    (|| {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Shell)
        && scope.class_tag.as_str() == "369"
        && scope.paired_class_tag.as_str() == "261"
    {
        return match exact_shell_class_369_261(ctx, bytes, records, scope) {
            Ok(Some(operation)) => Some(Ok(operation)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        };
    }
    match design_feature_family(&scope.kind())? {
        DesignFeatureFamily::OffsetFaces
            if matches!(
                (
                    (match parameter_scope_payload_length(ctx, scope) {
                Ok(length) => length,
                Err(error) => return Some(Err(error)),
            }),
                    scope.reference_members().len()
                ),
                (Some(264), 4) | (Some(253), 3)
            ) && bytes.get(start + 25) == Some(&1) =>
        {
            let distance_record_index = View::u32_le_at(bytes, start + 26)?;
            if scope.reference_members().values().next_back() != Some(&distance_record_index) {
                return None;
            }
            let scalar = match exact_fixed_scalar(ctx, bytes, records, distance_record_index) {
                Ok(Some(scalar)) => scalar,
                Ok(None) => return None,
            Err(error) => return Some(Err(error)),
            };
            Some(Ok(DesignDirectFaceOperation::OffsetFaces(
                direct_face::DesignOffsetFacesOperation {
                    distance: scalar.value,
                    distance_record_index,
                    distance_offset: scalar.value_offset,
                },
            )))
        }
        DesignFeatureFamily::Thicken if scope.reference_members().len() >= 3 => {
            match exact_legacy_thicken_class_347(ctx, bytes, records, scope) {
                Ok(Some(operation)) => return Some(Ok(operation)),
                Ok(None) => {}
                Err(error) => return Some(Err(error)),
            }
            let (reference_offset, thickness_is_first) = match match parameter_scope_payload_length(ctx, scope) {
                Ok(length) => length,
                Err(error) => return Some(Err(error)),
            }
            {
                Some(length)
                    if length
                        == 276
                            + 11 * u64::try_from(
                                scope.reference_members().len().checked_sub(2)?,
                            )
                            .ok()?
                        && bytes.get(start + 34) == Some(&1)
                        && (match super::parameter_scope::reference_members(
                            ctx,
                            scope.reference_members(),
                            "scan F3D Thicken scope references",
                        ) {
                            Ok(mut members) => {
                                View::u32_le_at(bytes, start + 35) == members.nth(1)
                            }
                            Err(error) => return Some(Err(error)),
                        })
                        && bytes.get(start + 39..start + 45) == Some(&[0; 6])
                        && matches!(bytes.get(start + 45), Some(0 | 1))
                        && bytes.get(start + 46..start + 48) == Some(&[1, 1])
                        && View::u32_le_at(bytes, start + 48)
                            == scope.reference_members().values().next().copied() =>
                {
                    (47, true)
                }
                Some(281)
                    if matches!(bytes.get(start + 45), Some(0 | 1))
                        && bytes.get(start + 46) == Some(&1) =>
                {
                    (46, false)
                }
                Some(287) if bytes.get(start + 47) == Some(&1) => (47, false),
                _ => return None,
            };
            let thickness_record_index = View::u32_le_at(bytes, start + reference_offset + 1)?;
            let expected_thickness = if thickness_is_first {
                scope.reference_members().values().next()
            } else {
                scope.reference_members().values().next_back()
            };
            if expected_thickness != Some(&thickness_record_index) {
                return None;
            }
            let scalar = match exact_fixed_scalar(ctx, bytes, records, thickness_record_index) {
                Ok(Some(scalar)) => scalar,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            if scalar.value.get() == 0.0 {
                return None;
            }
            Some(Ok(DesignDirectFaceOperation::Thicken(
                direct_face::DesignThickenOperation {
                    signed_thickness: scalar.value,
                    thickness_record_index,
                    thickness_offset: scalar.value_offset,
                },
            )))
        }
        DesignFeatureFamily::Shell if scope.reference_members().len() == 3 => {
            let (thickness_record_index, thickness_is_first, outward, outward_offset) =
                match match parameter_scope_payload_length(ctx, scope) {
                Ok(length) => length,
                Err(error) => return Some(Err(error)),
            } {
                    Some(268)
                        if bytes.get(start + 11..start + 20) == Some(&[0; 9])
                            && bytes.get(start + 20) == Some(&1)
                            && matches!(bytes.get(start + 21), Some(0 | 1))
                            && bytes.get(start + 22..start + 25) == Some(&[0; 3])
                            && bytes.get(start + 25..start + 27) == Some(&[1, 0])
                            && bytes.get(start + 27) == Some(&1)
                            && bytes.get(start + 32..start + 51) == Some(&[0; 19])
                            && View::u32_le_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && (match super::parameter_scope::reference_members(
                                ctx,
                                scope.reference_members(),
                                "scan F3D Shell scope references",
                            ) {
                                Ok(mut members) => {
                                    View::u32_le_at(bytes, start + 56) == members.nth(1)
                                }
                                Err(error) => return Some(Err(error)),
                            })
                            && bytes.get(start + 60..start + 66) == Some(&[0; 6]) =>
                    {
                        (
                            View::u32_le_at(bytes, start + 28)?,
                            true,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    Some(268)
                        if matches!(bytes.get(start + 25), Some(0 | 1))
                            && bytes.get(start + 26) == Some(&0)
                            && bytes.get(start + 27) == Some(&1)
                            && View::u32_le_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && View::u32_le_at(bytes, start + 56)
                                == scope.reference_members().values().next().copied() =>
                    {
                        (
                            View::u32_le_at(bytes, start + 28)?,
                            false,
                            bytes[start + 25] != 0,
                            start + 25,
                        )
                    }
                    Some(258)
                        if bytes.get(start + 11..start + 21) == Some(&[0; 10])
                            && matches!(bytes.get(start + 21), Some(0 | 1))
                            && bytes.get(start + 22) == Some(&1)
                            && bytes.get(start + 27..start + 42) == Some(&[0; 15])
                            && View::u32_le_at(bytes, start + 42) == Some(1)
                            && bytes.get(start + 46) == Some(&1)
                            && View::u32_le_at(bytes, start + 47)
                                == scope.reference_members().values().next().copied()
                            && bytes.get(start + 51..start + 57) == Some(&[0; 6]) =>
                    {
                        (
                            View::u32_le_at(bytes, start + 23)?,
                            false,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    _ => return None,
                };
            let expected_thickness = if thickness_is_first {
                scope.reference_members().values().next()
            } else {
                scope.reference_members().values().next_back()
            };
            if expected_thickness != Some(&thickness_record_index) {
                return None;
            }
            let scalar = match exact_fixed_scalar(ctx, bytes, records, thickness_record_index) {
                Ok(Some(scalar)) => scalar,
                Ok(None) => return None,
            Err(error) => return Some(Err(error)),
            };
            Some(Ok(DesignDirectFaceOperation::Shell(
                direct_face::DesignShellOperation {
                    thickness: cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?,
                    thickness_record_index,
                    thickness_offset: scalar.value_offset,
                    outward,
                    outward_offset: u64::try_from(outward_offset).ok()?,
                },
            )))
        }
        _ => None,
    }
    })()
    .transpose()
}

pub(super) fn exact_move_operation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignMoveOperation>, cadmpeg_core::CodecError> {
    (|| {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Move) {
        return None;
    }
    let mut candidate = None;
    let reference_members = match super::parameter_scope::reference_members(
        ctx,
        scope.reference_members(),
        "scan F3D Move scope reference members",
    ) {
        Ok(members) => members,
        Err(error) => return Some(Err(error)),
    };
    for record_index in reference_members {
        let frames = match records.frames(ctx, record_index) {
            Ok(frames) => frames,
            Err(error) => return Some(Err(error)),
        };
        for (start, paired) in frames {
            let class_tag_len = usize::try_from(View::u32_le_at(bytes, start)?).ok()?;
            if class_tag_len > 2000 {
                return None;
            }
            let after_tag = start.checked_add(4)?.checked_add(class_tag_len)?;
            let class_tag_bytes = bytes.get(start + 4..after_tag)?;
            let class_tag = match ctx.validate_utf8(
                class_tag_bytes,
                "validate F3D Move class tag UTF-8",
            ) {
                Ok(Ok(class_tag)) => class_tag,
                Ok(Err(_)) => return None,
                Err(error) => return Some(Err(error)),
            };
            let graphic = match ctx.admit_iter(
                class_tag.as_bytes(),
                "validate F3D Move class tag",
            ) {
                Ok(mut bytes) => bytes.all(u8::is_ascii_graphic),
                Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
            };
            if !graphic {
                return None;
            }
            let frame_length = paired.checked_sub(start)?;
            if View::u32_le_at(bytes, after_tag) != Some(record_index)
                || bytes.get(start + 11..start + 43) != Some(&[0; 32])
            {
                continue;
            }
            let Some((form_offset, transform_offset)) =
                move_transform_layout(class_tag, frame_length)
            else {
                continue;
            };
            let expected_paired_class = match class_tag {
                "447" => Some("263"),
                "456" => Some("258"),
                _ => None,
            };
            if expected_paired_class.is_some_and(|expected| {
                lp_ascii_filtered_view(bytes, paired, 3..=3, u8::is_ascii_digit)
                    .is_none_or(|(paired_class_tag, _)| paired_class_tag != expected)
            }) {
                continue;
            }
            if bytes.get(start + 47) != Some(&0) {
                continue;
            }
            let Ok(form) =
                direct_face::DesignMoveForm::try_from(View::u32_le_at(bytes, start + form_offset)?)
            else {
                continue;
            };
            let mut view = View::over_retained(bytes.get(start + transform_offset..)?);
            let mut transform = [[0.0; 4]; 4];
            for row in &mut transform {
                for value in row {
                    *value = view.f64_le()?;
                }
            }
            let Ok(transform) =
                crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
            else {
                continue;
            };
            let next = DesignMoveOperation {
                transform,
                transform_offset: u64_from_index(start + transform_offset),
                transform_record_index: record_index,
                form,
                form_offset: u64_from_index(start + form_offset),
            };
            if candidate.replace(next).is_some() {
                return None;
            }
        }
    }
    candidate.map(Ok)
    })()
    .transpose()
}

/// Return the fixed envelope offsets admitted for one Move transform class.
///
/// The legacy classes carry the same matrix envelope as the current classes;
/// their class tags are the generation discriminator. Keeping the admission
/// keyed by class avoids treating an arbitrary 253-byte record as a transform.
fn move_transform_layout(class_tag: &str, frame_length: usize) -> Option<(usize, usize)> {
    let admitted = match class_tag {
        "296" | "362" | "433" | "447" if frame_length == 253 => true,
        "349" if matches!(frame_length, 254 | 274) => true,
        "368" if frame_length == 254 => true,
        "293" | "393" | "442" | "451" | "456" if frame_length == 253 => true,
        _ => false,
    };
    admitted.then_some((43, 48))
}

pub(super) fn exact_scale_operation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<DesignScaleOperation>, cadmpeg_core::CodecError> {
    (|| {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Scale) {
            return None;
        }
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let (body_group_record_index, center_record_index, uniform_factor_offset, center) =
            if (match parameter_scope_payload_length(ctx, scope) {
                Ok(length) => length,
                Err(error) => return Some(Err(error)),
            }) == Some(303)
                && scope.reference_members().len() == 5
            {
                let [factor_record_index, body_group_record_index, _, _, center_record_index] =
                    scope.reference_members().values_array()?;
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
                let point_record_indices = match ctx.admit_iter(
                    std::slice::from_ref(center_record_index),
                    "scan F3D Scale center point references",
                ) {
                    Ok(indices) => indices.copied(),
                    Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
                };
                let center = match exact_point_data_construction(
                    ctx,
                    bytes,
                    records,
                    point_record_indices,
                    stream_types,
                ) {
                    Ok(point) => point.map(|point| (point.position, point.position_offset)),
                    Err(error) => return Some(Err(error)),
                };
                (
                    *body_group_record_index,
                    *center_record_index,
                    start + 25,
                    center,
                )
            } else if scope.kind() == scope::DesignFeatureKind::Scale
                && matches!(scope.reference_members().len(), 5 | 6)
                && scope.frame_length()
                    == 307 + u64::try_from(scope.reference_members().len() - 5).ok()? * 11
            {
                let mut references = scope.reference_members().values();
                let factor_record_index = references.next()?;
                let body_group_record_index = references.next()?;
                let center_record_index = references.next_back()?;
                if bytes.get(start + 16..start + 21)? != [0; 5]
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
                let point_record_indices = match ctx.admit_iter(
                    std::slice::from_ref(center_record_index),
                    "scan F3D Scale center point references",
                ) {
                    Ok(indices) => indices.copied(),
                    Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
                };
                let point = match exact_point_data_construction(
                    ctx,
                    bytes,
                    records,
                    point_record_indices,
                    stream_types,
                ) {
                    Ok(Some(point)) => point,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                (
                    *body_group_record_index,
                    *center_record_index,
                    start + 21,
                    Some((point.position, point.position_offset)),
                )
            } else {
                return None;
            };
        let uniform_factor =
            cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, uniform_factor_offset)?)?;
        Some(Ok(DesignScaleOperation {
            body_group_record_index,
            center_record_index,
            center_position: center
                .map(|(value, offset)| crate::records::identity::Located { value, offset }),
            uniform_factor,
            uniform_factor_offset: u64_from_index(uniform_factor_offset),
        }))
    })()
    .transpose()
}
