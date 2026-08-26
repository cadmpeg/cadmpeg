//! Line reference directions and revolution axis inputs.

use super::component_paths::is_profile_feature_object;
use super::curves::compact_bounded_curve_tangent;
use super::endpoints::{
    compact_indexed_curve_endpoint_indices, extended_wide_horizontal_relation_endpoint_indices,
    marker_is_selected_construction_line, roster_curve_endpoint_markers,
    wide_indexed_curve_endpoint_indices,
};
use super::scalars::feature_object_name;
use super::transforms::{quantize, sketch_frame_marker_transform, MarkerTransform};
use super::{is_class_token, CLASS_MARKER, SKETCH_MARKER};
use crate::layout::temporary_axis_reference_nine_scalar as temporary_axis;
use crate::records::{FeatureInputLane, FeatureInputName, SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::View;
use cadmpeg_ir::features::{FeatureDefinition, Length};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use std::collections::{HashMap, HashSet};

const TEMPORARY_AXIS_UNIT_DIRECTION_EPS: f64 = 1.0e-9;
const EPS_AXES_LINE_REFERENCE_DIRECTION_E9: f64 = 1e-9;
const EPS_AXES_DECLARED_LINE_REFERENCE_DIRECTIONS_E9: f64 = 1e-9;
const EPS_AXES_CANONICAL_UNIT_DIRECTION_E12: f64 = 1e-12;
const EPS_AXES_LINEAR_PATTERN_DISPLAY_DIRECTIONS_E9: f64 = 1e-9;
const EPS_AXES_COMPACT_LINE_REFERENCE_DIRECTIONS_E9: f64 = 1e-9;
const EPS_AXES_REVOLUTION_LINE_REFERENCE_INPUTS_E9: f64 = 1e-9;
const EPS_AXES_BIND_PROFILE_REVOLUTION_AXES_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_CONSTRUCTION_AXIS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_GENERATED_SURFACE_AXIS_E9: f64 = 1e-9;
const EPS_AXES_COMMON_GENERATED_SURFACE_AXIS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_PRINCIPAL_AXIS_ENDPOINTS_E9: f64 = 1e-9;

pub(super) fn line_reference_direction(payload: &[u8], class_offset: u64) -> Option<Vector3> {
    let class_offset = usize::try_from(class_offset).ok()?;
    let scalar = |offset: usize| {
        let value = View::f64_le_at(payload, offset)?;
        value.is_finite().then_some(value)
    };
    let direction_at = |offset: usize| {
        let direction = Vector3::new(scalar(offset)?, scalar(offset + 8)?, scalar(offset + 16)?);
        let norm =
            (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z)
                .sqrt();
        ((norm - 1.0).abs() <= EPS_AXES_LINE_REFERENCE_DIRECTION_E9).then_some(Vector3::new(
            direction.x / norm,
            direction.y / norm,
            direction.z / norm,
        ))
    };
    let mut directions = Vec::new();
    if payload.get(class_offset + 136..class_offset + 144)
        == Some(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff])
        && payload.get(class_offset + 148..class_offset + 152) == Some(&[0xf8, 0x2a, 0, 0])
    {
        if let Some(direction) = direction_at(class_offset + 200) {
            directions.push(direction);
        }
    }
    if payload.get(class_offset + 144..class_offset + 156)
        == Some(&[
            0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff,
        ])
        && payload.get(class_offset + 160..class_offset + 164) == Some(&[0xf8, 0x2a, 0, 0])
    {
        if let Some(direction) = direction_at(class_offset + 220) {
            directions.push(direction);
        }
    }
    // Both declared layouts are evaluated before selecting the direction so
    // a future overlapping layout cannot win merely by branch order.
    directions.dedup();
    let [direction] = directions.as_slice() else {
        return None;
    };
    Some(*direction)
}

pub(super) fn declared_line_reference_directions(
    payload: &[u8],
    class_offset: u64,
    object_end: usize,
) -> Vec<Vector3> {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];

    let Ok(class_offset) = usize::try_from(class_offset) else {
        return Vec::new();
    };
    let end = object_end.min(payload.len());
    let mut directions = line_reference_direction(&payload[..end], class_offset as u64)
        .into_iter()
        .collect::<Vec<_>>();
    let Some(final_handle) = end
        .checked_sub(88)
        .filter(|final_handle| *final_handle >= class_offset)
    else {
        return directions;
    };
    for handle in class_offset..=final_handle {
        if payload.get(handle..handle + HANDLES.len()) != Some(HANDLES.as_slice()) {
            continue;
        }
        let scalar = |relative: usize| {
            let offset = handle.checked_add(relative)?;
            let value = View::f64_le_at(payload, offset)?;
            value.is_finite().then_some(value)
        };
        let direction_at = |relative: usize| {
            let direction = Vector3::new(
                scalar(relative)?,
                scalar(relative + 8)?,
                scalar(relative + 16)?,
            );
            let norm =
                (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z)
                    .sqrt();
            ((norm - 1.0).abs() <= EPS_AXES_DECLARED_LINE_REFERENCE_DIRECTIONS_E9).then_some(
                Vector3::new(direction.x / norm, direction.y / norm, direction.z / norm),
            )
        };
        let addressed = payload.get(handle + 8..handle + 12) == Some(&[0; 4])
            && View::u32_le_at(payload, handle + 12).is_some_and(|address| address != 0);
        let compact_long_form = (payload.get(handle + 88..handle + 104) == Some(&[0; 16])
            && payload.get(handle + 104..handle + 112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
            && payload.get(handle + 112..handle + 136) == Some(&[0; 24]))
            || (payload.get(handle + 104..handle + 112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
                && payload.get(handle + 112..handle + 124) == Some(&[0; 12]));
        let candidate = if addressed
            && !compact_long_form
            && payload.get(handle + 16..handle + 32) == Some(&[0; 16])
            && (32..88)
                .step_by(8)
                .all(|relative| scalar(relative).is_some())
        {
            direction_at(64)
        } else {
            None
        };
        if let Some(candidate) = candidate {
            if !directions.contains(&candidate) {
                directions.push(candidate);
            }
        }
    }
    directions
}

pub(super) fn canonical_unit_direction(direction: Vector3) -> Vector3 {
    let component = |value: f64| {
        if value.abs() <= EPS_AXES_CANONICAL_UNIT_DIRECTION_E12 {
            0.0
        } else {
            value
        }
    };
    Vector3::new(
        component(direction.x),
        component(direction.y),
        component(direction.z),
    )
}

pub(super) fn linear_pattern_display_directions(
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    names: &[FeatureInputName],
    expected_spacing_m: [Option<f64>; 2],
) -> Vec<Vector3> {
    const VALUE_OFFSET: usize = 32;
    const DIRECTION_OFFSET: usize = 161;
    const LENGTH_TOLERANCE_M: f64 = 1e-8;

    let end = object_end.min(payload.len());
    ["D3", "D4"]
        .into_iter()
        .zip(expected_spacing_m)
        .filter_map(|(dimension_name, expected)| {
            let expected = expected?;
            let mut records = names.iter().filter(|name| {
                name.object_id == Some(u32::MAX)
                    && name.value == dimension_name
                    && usize::try_from(name.offset)
                        .is_ok_and(|offset| (object_start..end).contains(&offset))
            });
            let name = records.next()?;
            if records.next().is_some() {
                return None;
            }
            let offset = usize::try_from(name.offset).ok()?;
            let value_offset = offset.checked_add(VALUE_OFFSET)?;
            let direction_offset = offset.checked_add(DIRECTION_OFFSET)?;
            if direction_offset.checked_add(24)? > end {
                return None;
            }
            let stored_spacing = View::f64_le_at(payload, value_offset)?;
            if !stored_spacing.is_finite()
                || stored_spacing <= 0.0
                || (stored_spacing - expected).abs() > LENGTH_TOLERANCE_M
            {
                return None;
            }
            let scalar = |relative: usize| {
                let scalar_offset = direction_offset.checked_add(relative)?;
                let value = View::f64_le_at(payload, scalar_offset)?;
                value.is_finite().then_some(value)
            };
            let direction = Vector3::new(scalar(0)?, scalar(8)?, scalar(16)?);
            let norm =
                (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z)
                    .sqrt();
            ((norm - 1.0).abs() <= EPS_AXES_LINEAR_PATTERN_DISPLAY_DIRECTIONS_E9).then_some(
                Vector3::new(direction.x / norm, direction.y / norm, direction.z / norm),
            )
        })
        .collect()
}

pub(super) fn typed_linear_pattern_dimensions(
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Option<(Length, u32)> {
    let parameter = |class_name: &str| {
        let mut classes = lane.classes.iter().filter(|class| {
            class.name == class_name
                && usize::try_from(class.offset)
                    .is_ok_and(|offset| (object_start..object_end).contains(&offset))
        });
        let class = classes.next().filter(|_| classes.next().is_none())?;
        let class_offset = usize::try_from(class.offset).ok()?;
        let name_end = class_offset.checked_add(128)?.min(object_end);
        let mut names = lane.names.iter().filter(|name| {
            name.object_id == Some(u32::MAX)
                && usize::try_from(name.offset)
                    .is_ok_and(|offset| (class_offset..name_end).contains(&offset))
                && feature.parameters.contains_key(name.value.as_str())
        });
        let name = names.next().filter(|_| names.next().is_none())?;
        feature.parameters.get(name.value.as_str())
    };
    let count = parameter("moNumberDim_c")?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|count| *count > 0)?;
    let spacing = crate::history::parse_positive_dimension_length_mm(parameter(
        "ParallelPlaneDistanceDim_c",
    )?)?;
    Some((Length(spacing), count))
}

#[cfg(test)]
pub(super) fn compact_line_reference_direction(
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    excluded_handles: &[usize],
) -> Option<Vector3> {
    let directions =
        compact_line_reference_directions(payload, object_start, object_end, excluded_handles);
    let [direction] = directions.as_slice() else {
        return None;
    };
    Some(*direction)
}

pub(super) fn compact_line_reference_directions(
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    excluded_handles: &[usize],
) -> Vec<Vector3> {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let end = object_end.min(payload.len());
    let Some(final_handle) = end.checked_sub(80).filter(|end| *end >= object_start) else {
        return Vec::new();
    };
    let mut candidates = (object_start..=final_handle).flat_map(|handle| {
        if excluded_handles.contains(&handle) {
            return Vec::new();
        }
        let Some(record) = payload.get(handle..end) else {
            return Vec::new();
        };
        let Some(address) = View::u32_le_at(record, 12) else {
            return Vec::new();
        };
        if record[..8] != HANDLES || record[8..12] != [0; 4] {
            return Vec::new();
        }
        let scalar = |offset: usize| {
            let value = View::f64_le_at(record, offset)?;
            value.is_finite().then_some(value)
        };
        let direction_at = |offset: usize| {
            let direction =
                Vector3::new(scalar(offset)?, scalar(offset + 8)?, scalar(offset + 16)?);
            let norm =
                (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z)
                    .sqrt();
            ((norm - 1.0).abs() <= EPS_AXES_COMPACT_LINE_REFERENCE_DIRECTIONS_E9).then_some(
                Vector3::new(direction.x / norm, direction.y / norm, direction.z / norm),
            )
        };
        let mut directions = Vec::new();
        let tagged_token = |offset: usize| {
            record
                .get(offset..offset + 2)
                .and_then(|bytes| View::u16_le_at(bytes, 0))
                .is_some_and(is_class_token)
        };
        if address == 0 {
            let unshifted_termination = record.get(88..104) == Some(&[0; 16])
                && record.get(104..112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
                && record.get(112..136) == Some(&[0; 24]);
            if record.get(16..32) == Some(&[0; 16]) && unshifted_termination {
                directions.extend(direction_at(64));
            }
            let terminated =
                record.get(80..84) == Some(&[0; 4]) && (tagged_token(84) || record.len() == 84);
            if record.get(16..24) == Some(&[0; 8]) && terminated {
                directions.extend(direction_at(56));
            }
            directions.dedup();
            return if directions.len() == 1 {
                directions
            } else {
                Vec::new()
            };
        }
        let shifted_nine_scalar_trailer = record.get(96..104) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
            && record.get(104..116) == Some(&[0; 12])
            && tagged_token(116)
            && record.get(118..134) == Some(&[0; 16])
            && record.get(134..136) == Some(&[0xff; 2]);
        if record.get(16..24) == Some(&[0; 8]) && shifted_nine_scalar_trailer {
            if let Some(direction) = direction_at(72) {
                directions.push(direction);
            }
        }
        let shifted_seven_scalar_trailer = (record.get(80..116) == Some(&[0; 36])
            && tagged_token(116)
            && record.get(118..134) == Some(&[0; 16])
            && record.get(134..136) == Some(&[0xff; 2]))
            || (record.get(80..88) == Some(&[0; 8])
                && record
                    .get(88..96)
                    .and_then(|bytes| {
                        Some([View::u32_le_at(bytes, 0)?, View::u32_le_at(bytes, 4)?])
                    })
                    .is_some_and(|values| values.into_iter().all(|value| value != 0)));
        if record.get(16..24) == Some(&[0; 8]) && shifted_seven_scalar_trailer {
            if let Some(direction) = direction_at(56) {
                directions.push(direction);
            }
        }
        let tagged_trailer = record.get(88..104) == Some(&[0; 16])
            && ((record.get(104..124) == Some(&[0; 20])
                && tagged_token(124)
                && record.get(126..142) == Some(&[0; 16])
                && record.get(142..144) == Some(&[0xff; 2]))
                || (record.get(104..112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
                    && record.get(112..122) == Some(&[0; 10])
                    && tagged_token(122)
                    && record.get(124..140) == Some(&[0; 16])
                    && record.get(140..142) == Some(&[0xff; 2]))
                || (record.get(104..112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
                    && record.get(112..124) == Some(&[0; 12])
                    && tagged_token(124)
                    && record.get(126..142) == Some(&[0; 16])
                    && record.get(142..144) == Some(&[0xff; 2])));
        if directions.is_empty() && record.get(16..32) == Some(&[0; 16]) && tagged_trailer {
            if let Some(direction) = direction_at(64) {
                directions.push(direction);
            }
        }
        let seven_scalar_trailer = record.get(88..96).is_some_and(|bytes| bytes != [0; 8])
            || (record.get(88..122) == Some(&[0; 34])
                && tagged_token(122)
                && record.get(124..140) == Some(&[0; 16])
                && record.get(140..142) == Some(&[0xff; 2]))
            || (record.get(88..102) == Some(&[0; 14])
                && record.get(102..110) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
                && record.get(110..122) == Some(&[0; 12])
                && tagged_token(122)
                && record.get(124..140) == Some(&[0; 16])
                && record.get(140..142) == Some(&[0xff; 2]));
        if directions.is_empty() && record.get(16..32) == Some(&[0; 16]) && seven_scalar_trailer {
            if let Some(direction) = direction_at(64) {
                directions.push(direction);
            }
        }
        if directions.is_empty()
            && record.get(16..32) == Some(&[0; 16])
            && record.get(88..104) == Some(&[0; 16])
            && record.get(104..112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
            && record.get(112..136) == Some(&[0; 24])
        {
            if let Some(direction) = direction_at(64) {
                directions.push(direction);
            }
        }
        if directions.is_empty()
            && record.get(16..32) == Some(&[0; 16])
            && record.get(104..112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
            && (record.get(112..128) == Some(&[0; 16])
                || record.get(112..126).is_some_and(|tail| {
                    tail[..12] == [0; 12] && View::u16_le_at(tail, 12).is_some_and(is_class_token)
                }))
        {
            if let Some(direction) = direction_at(80) {
                directions.push(direction);
            }
        }
        // The final branch is the legacy unshifted fallback.  It is only a
        // candidate when no addressed layout matched; otherwise its
        // overlapping scalar window would manufacture a second width.
        if directions.is_empty() && record.get(16..24) == Some(&[0; 8]) {
            if record.get(80..88) == Some(&[0; 8]) {
                let candidates = [direction_at(64), direction_at(72)]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                let mut distinct = Vec::new();
                for candidate in candidates {
                    if !distinct.contains(&candidate) {
                        distinct.push(candidate);
                    }
                }
                if let [direction] = distinct.as_slice() {
                    directions.push(*direction);
                }
            } else {
                directions.extend(direction_at(56));
            }
        }
        // A record is usable only when every matching layout agrees.  Never
        // let the order of the recognizers choose between distinct vectors.
        directions.dedup();
        if directions.len() == 1 {
            directions
        } else {
            Vec::new()
        }
    });
    let mut directions = Vec::new();
    for candidate in &mut candidates {
        if !directions.contains(&candidate) {
            directions.push(candidate);
        }
    }
    directions
}

pub(super) fn revolution_line_reference_inputs(
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    profile_sources: &HashSet<u32>,
) -> Option<(u32, Point3, Vector3)> {
    const HANDLE: [u8; 4] = [0xc7, 0xcf, 0xff, 0xff];
    const NATIVE_TO_IR: f64 = 1000.0;

    let search_end = object_end.min(payload.len());
    let scalar = |offset: usize| {
        let value = View::f64_le_at(payload, offset)?;
        (value.is_finite() && value.abs() <= 1.0e6).then_some(value)
    };
    let source_cell = |offset: usize| {
        let source = View::u32_le_at(payload, offset)?;
        let identity = View::u32_le_at(payload, offset + 4)?;
        let token = View::u16_le_at(payload, offset + 8)?;
        (profile_sources.contains(&source)
            && identity != 0
            && is_class_token(token)
            && payload.get(offset + 12..offset + 16) == Some(&[0xff; 4]))
        .then_some(source)
    };
    let axis_record = |frame: usize, scalar_count: usize| {
        let x = scalar(frame)?;
        let y = scalar(frame + 8)?;
        let z = scalar(frame + 16)?;
        let direction_offset = frame + (scalar_count.checked_sub(3)?) * 8;
        let dx = scalar(direction_offset)?;
        let dy = scalar(direction_offset + 8)?;
        let dz = scalar(direction_offset + 16)?;
        let norm = (dx * dx + dy * dy + dz * dz).sqrt();
        ((norm - 1.0).abs() <= EPS_AXES_REVOLUTION_LINE_REFERENCE_INPUTS_E9).then_some((
            Point3::new(x * NATIVE_TO_IR, y * NATIVE_TO_IR, z * NATIVE_TO_IR),
            Vector3::new(dx / norm, dy / norm, dz / norm),
        ))
    };
    let next_class_after_zeros = |record_end: usize, maximum_padding: usize| {
        (record_end..=record_end + maximum_padding).find(|offset| {
            payload.get(record_end..*offset).is_some_and(|padding| {
                padding.iter().all(|byte| *byte == 0)
                    && payload.get(*offset..*offset + 4) == Some(CLASS_MARKER)
            })
        })
    };
    let mut candidates = Vec::new();
    for handle_start in object_start..search_end.saturating_sub(64) {
        if payload.get(handle_start..handle_start + 4) != Some(HANDLE.as_slice()) {
            continue;
        }
        if handle_start >= object_start + 44
            && payload.get(handle_start + 4..handle_start + 8) == Some(HANDLE.as_slice())
            && payload.get(handle_start - 28..handle_start - 24) == Some(&1u32.to_le_bytes())
            && payload.get(handle_start - 24..handle_start - 20) == Some(&1u32.to_le_bytes())
            && payload.get(handle_start - 20..handle_start - 16) == Some(&[0; 4])
            && payload.get(handle_start - 12..handle_start) == Some(&[0; 12])
            && View::u32_le_at(payload, handle_start - 16).is_some_and(|address| address != 0)
            && payload.get(handle_start + 8..handle_start + 24) == Some(&[0; 16])
        {
            let source_offset = handle_start - 44;
            if let Some(source) = source_cell(source_offset) {
                let handles_end = handle_start + 8;
                let frame_gap = 16;
                let frame = handles_end + frame_gap;
                let Some((x, y, z, dx, dy, dz)) = scalar(frame)
                    .zip(scalar(frame + 8))
                    .zip(scalar(frame + 16))
                    .zip(scalar(frame + 24))
                    .zip(scalar(frame + 32))
                    .zip(scalar(frame + 40))
                    .map(|(((((x, y), z), dx), dy), dz)| (x, y, z, dx, dy, dz))
                else {
                    continue;
                };
                let record_end = frame + 48;
                let Some(next_record) = (record_end..=record_end + 24)
                    .find(|offset| payload.get(*offset..*offset + 4) == Some(CLASS_MARKER))
                else {
                    continue;
                };
                if payload
                    .get(record_end..next_record)
                    .is_some_and(|bytes| bytes.iter().any(|byte| *byte != 0))
                {
                    continue;
                }
                let norm = (dx * dx + dy * dy + dz * dz).sqrt();
                if (norm - 1.0).abs() <= EPS_AXES_REVOLUTION_LINE_REFERENCE_INPUTS_E9 {
                    candidates.push((
                        handle_start,
                        6,
                        (
                            source,
                            Point3::new(x * NATIVE_TO_IR, y * NATIVE_TO_IR, z * NATIVE_TO_IR),
                            Vector3::new(dx / norm, dy / norm, dz / norm),
                        ),
                    ));
                }
            }
        }
        if handle_start >= object_start + 48
            && payload.get(handle_start + 4..handle_start + 8) == Some(HANDLE.as_slice())
            && payload.get(handle_start + 8..handle_start + 12) == Some(HANDLE.as_slice())
            && payload.get(handle_start - 32..handle_start - 28) == Some(&[0; 4])
            && View::u32_le_at(payload, handle_start - 28).is_some_and(|variant| variant != 0)
            && payload.get(handle_start - 24..handle_start - 20) == Some(&1u32.to_le_bytes())
            && payload.get(handle_start - 20..handle_start - 16) == Some(&[0; 4])
            && View::u32_le_at(payload, handle_start - 16).is_some_and(|address| address != 0)
            && payload.get(handle_start - 12..handle_start) == Some(&[0; 12])
        {
            let source_offset = handle_start - 48;
            if let Some(source) = source_cell(source_offset) {
                let handles_end = handle_start + 12;
                let compact_frame = handles_end + 4;
                let compact_end = compact_frame + 9 * 8;
                if payload.get(handles_end..handles_end + 4) == Some(&[0; 4])
                    && payload.get(handles_end + 4..handles_end + 12) == Some(&[0; 8])
                    && next_class_after_zeros(compact_end, 24).is_some()
                {
                    if let Some(axis) = axis_record(compact_frame, 9) {
                        candidates.push((handle_start, 9, (source, axis.0, axis.1)));
                    }
                }
                let addressed_frame = handles_end + 24;
                let addressed_end = addressed_frame + 8 * 8;
                if payload.get(handles_end..handles_end + 4) == Some(&[0; 4])
                    && View::u32_le_at(payload, handles_end + 4).is_some_and(|address| address != 0)
                    && payload.get(handles_end + 8..handles_end + 20) == Some(&[0; 12])
                    && payload.get(handles_end + 20..handles_end + 24) == Some(&[0xff; 4])
                    && next_class_after_zeros(addressed_end, 24).is_some()
                {
                    if let Some(axis) = axis_record(addressed_frame, 8) {
                        candidates.push((handle_start, 8, (source, axis.0, axis.1)));
                    }
                }
            }
        }
        if handle_start >= object_start + 48
            && payload.get(handle_start + 4..handle_start + 8) == Some(HANDLE.as_slice())
            && payload.get(handle_start - 32..handle_start - 28) == Some(&[0; 4])
            && View::u32_le_at(payload, handle_start - 28).is_some_and(|variant| variant != 0)
            && payload.get(handle_start - 24..handle_start - 20) == Some(&1u32.to_le_bytes())
            && payload.get(handle_start - 20..handle_start - 16) == Some(&[0; 4])
            && View::u32_le_at(payload, handle_start - 16).is_some_and(|address| address != 0)
            && payload.get(handle_start - 12..handle_start) == Some(&[0; 12])
        {
            let source_offset = handle_start - 48;
            let frame = handle_start + 8;
            let record_end = frame + 8 * 8;
            if let Some(source) = source_cell(source_offset) {
                if payload.get(frame..frame + 8) == Some(&[0; 8])
                    && next_class_after_zeros(record_end, 24).is_some()
                {
                    if let Some(axis) = axis_record(frame, 8) {
                        candidates.push((handle_start, 8, (source, axis.0, axis.1)));
                    }
                }
            }
        }
        if handle_start >= object_start + 44
            && payload.get(handle_start + 4..handle_start + 8) == Some(HANDLE.as_slice())
            && View::u32_le_at(payload, handle_start - 28).is_some_and(|variant| variant != 0)
            && payload.get(handle_start - 24..handle_start - 20) == Some(&1u32.to_le_bytes())
            && payload.get(handle_start - 20..handle_start - 16) == Some(&[0; 4])
            && View::u32_le_at(payload, handle_start - 16).is_some_and(|address| address != 0)
            && payload.get(handle_start - 12..handle_start) == Some(&[0; 12])
            && payload.get(handle_start + 8..handle_start + 12) == Some(&[0; 4])
            && View::u32_le_at(payload, handle_start + 12).is_some_and(|address| address != 0)
            && payload.get(handle_start + 16..handle_start + 24) == Some(&[0; 8])
            && payload
                .get(handle_start + 80..handle_start + 88)
                .is_some_and(|cell| cell.iter().any(|byte| *byte != 0))
        {
            let source_offset = handle_start - 44;
            if let (Some(source), Some(axis)) = (
                source_cell(source_offset),
                axis_record(handle_start + 24, 7),
            ) {
                candidates.push((handle_start, 7, (source, axis.0, axis.1)));
                continue;
            }
        }
        if handle_start >= object_start + 48
            && payload.get(handle_start - 32..handle_start - 28) == Some(&[0; 4])
            && payload.get(handle_start - 28..handle_start - 24) == Some(&1u32.to_le_bytes())
            && payload.get(handle_start - 24..handle_start - 20) == Some(&1u32.to_le_bytes())
            && payload.get(handle_start - 20..handle_start - 16) == Some(&[0; 4])
            && View::u32_le_at(payload, handle_start - 16).is_some_and(|address| address != 0)
            && payload.get(handle_start - 12..handle_start) == Some(&[0; 12])
        {
            let source_offset = handle_start - 48;
            if let Some(source) = source_cell(source_offset) {
                let two_handles = payload.get(handle_start + 4..handle_start + 8)
                    == Some(HANDLE.as_slice())
                    && payload.get(handle_start + 8..handle_start + 12) == Some(&[0; 4])
                    && View::u32_le_at(payload, handle_start + 12)
                        .is_some_and(|address| address != 0);
                let three_handles = payload.get(handle_start + 4..handle_start + 8)
                    == Some(HANDLE.as_slice())
                    && payload.get(handle_start + 8..handle_start + 12) == Some(HANDLE.as_slice())
                    && payload.get(handle_start + 12..handle_start + 16) == Some(&[0; 4])
                    && View::u32_le_at(payload, handle_start + 16)
                        .is_some_and(|address| address != 0)
                    && payload.get(handle_start + 20..handle_start + 24) == Some(&[0; 4]);
                let layout = if two_handles {
                    Some((handle_start + 16, 8))
                } else if three_handles {
                    Some((handle_start + 24, 9))
                } else {
                    None
                };
                if let Some((frame, scalar_count)) = layout {
                    let record_end = frame + scalar_count * 8;
                    if next_class_after_zeros(record_end, 24).is_some() {
                        if let Some(axis) = axis_record(frame, scalar_count) {
                            candidates.push((handle_start, scalar_count, (source, axis.0, axis.1)));
                            continue;
                        }
                    }
                }
            }
        }
        if handle_start >= object_start + 4
            && payload.get(handle_start - 4..handle_start) == Some(HANDLE.as_slice())
        {
            continue;
        }
        for handle_count in [2usize, 3] {
            let handles_end = handle_start.checked_add(handle_count * HANDLE.len())?;
            if (0..handle_count).any(|index| {
                let offset = handle_start + index * HANDLE.len();
                payload.get(offset..offset + HANDLE.len()) != Some(HANDLE.as_slice())
            }) || payload.get(handles_end..handles_end + 4) != Some(&[0; 4])
            {
                continue;
            }
            let address = View::u32_le_at(payload, handles_end + 4)?;
            if address == 0 {
                continue;
            }
            let mut source_cells = (object_start..handle_start.saturating_sub(15))
                .filter_map(source_cell)
                .collect::<Vec<_>>();
            source_cells.sort_unstable();
            source_cells.dedup();
            let [source] = source_cells.as_slice() else {
                continue;
            };
            for frame_gap in [0usize, 4, 8] {
                if payload
                    .get(handles_end + 8..handles_end + 8 + frame_gap)
                    .is_none_or(|bytes| bytes.iter().any(|byte| *byte != 0))
                {
                    continue;
                }
                let frame = handles_end + 8 + frame_gap;
                let Some((x, y, z)) = scalar(frame)
                    .zip(scalar(frame + 8))
                    .zip(scalar(frame + 16))
                    .map(|((x, y), z)| (x, y, z))
                else {
                    continue;
                };
                for scalar_count in [6usize, 8, 9] {
                    let direction_offset = frame + (scalar_count - 3) * 8;
                    let Some((dx, dy, dz)) = scalar(direction_offset)
                        .zip(scalar(direction_offset + 8))
                        .zip(scalar(direction_offset + 16))
                        .map(|((x, y), z)| (x, y, z))
                    else {
                        continue;
                    };
                    let record_end = frame + scalar_count * 8;
                    let Some(next_record) = (record_end..=record_end + 24).find(|offset| {
                        payload.get(*offset..*offset + 4) == Some(CLASS_MARKER)
                            || View::u16_le_at(payload, *offset).is_some_and(is_class_token)
                    }) else {
                        continue;
                    };
                    if payload
                        .get(record_end..next_record)
                        .is_some_and(|bytes| bytes.iter().any(|byte| *byte != 0))
                    {
                        continue;
                    }
                    let norm = (dx * dx + dy * dy + dz * dz).sqrt();
                    if (norm - 1.0).abs() > EPS_AXES_REVOLUTION_LINE_REFERENCE_INPUTS_E9 {
                        continue;
                    }
                    let candidate = (
                        *source,
                        Point3::new(x * NATIVE_TO_IR, y * NATIVE_TO_IR, z * NATIVE_TO_IR),
                        Vector3::new(dx / norm, dy / norm, dz / norm),
                    );
                    let ranked = (handle_start, scalar_count, candidate);
                    if !candidates.contains(&ranked) {
                        candidates.push(ranked);
                    }
                }
            }
        }
    }
    let ranks = candidates.iter().fold(
        HashMap::<usize, usize>::new(),
        |mut ranks, (handle, rank, _)| {
            ranks
                .entry(*handle)
                .and_modify(|current| *current = (*current).max(*rank))
                .or_insert(*rank);
            ranks
        },
    );
    let mut candidates = candidates
        .into_iter()
        .filter_map(|(handle, rank, candidate)| {
            (ranks.get(&handle) == Some(&rank)).then_some(candidate)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(source, origin, direction)| {
        (
            *source,
            [
                origin.x.to_bits(),
                origin.y.to_bits(),
                origin.z.to_bits(),
                direction.x.to_bits(),
                direction.y.to_bits(),
                direction.z.to_bits(),
            ],
        )
    });
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(super) fn temporary_axis_reference(
    payload: &[u8],
    object_start: usize,
    object_end: usize,
) -> Option<(Point3, Vector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;

    let end = object_end.min(payload.len());
    let last_declaration = end.checked_sub(temporary_axis::LEN)?;
    let mut candidates = (object_start..=last_declaration).filter_map(|declaration| {
        if payload.get(
            declaration + temporary_axis::CLASS_MARKER
                ..declaration
                    + temporary_axis::CLASS_MARKER
                    + temporary_axis::CLASS_MARKER_VALUE.len(),
        ) != Some(&temporary_axis::CLASS_MARKER_VALUE)
            || View::u16_le_at(payload, declaration + temporary_axis::NAME_LENGTH)
                != Some(temporary_axis::NAME_LENGTH_VALUE)
            || payload.get(
                declaration + temporary_axis::NAME
                    ..declaration + temporary_axis::NAME + temporary_axis::NAME_VALUE.len(),
            ) != Some(&temporary_axis::NAME_VALUE)
            || payload.get(
                declaration + temporary_axis::HANDLES
                    ..declaration + temporary_axis::HANDLES + temporary_axis::HANDLES_VALUE.len(),
            ) != Some(&temporary_axis::HANDLES_VALUE)
            || payload.get(
                declaration + temporary_axis::ZERO_BEFORE_ADDRESS
                    ..declaration
                        + temporary_axis::ZERO_BEFORE_ADDRESS
                        + temporary_axis::ZERO_BEFORE_ADDRESS_VALUE.len(),
            ) != Some(&temporary_axis::ZERO_BEFORE_ADDRESS_VALUE)
            || View::u32_le_at(payload, declaration + temporary_axis::STREAM_ADDRESS)
                .is_none_or(|address| address == 0)
        {
            return None;
        }
        let mut frame = [0.0; 9];
        for (index, scalar) in frame.iter_mut().enumerate() {
            let offset = declaration + temporary_axis::AXIS_FRAME + index * 8;
            let value = View::f64_le_at(payload, offset)?;
            if !value.is_finite() || value.abs() > 1.0e6 {
                return None;
            }
            *scalar = value;
        }
        let origin = Point3::new(
            frame[0] * NATIVE_TO_IR,
            frame[1] * NATIVE_TO_IR,
            frame[2] * NATIVE_TO_IR,
        );
        let direction = Vector3::new(frame[6], frame[7], frame[8]);
        let norm =
            (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z)
                .sqrt();
        if (norm - 1.0).abs() > TEMPORARY_AXIS_UNIT_DIRECTION_EPS {
            return None;
        }
        let record_end = declaration + temporary_axis::NEXT_CLASS_MARKER;
        let last_next_class = end.checked_sub(temporary_axis::NEXT_CLASS_MARKER_VALUE.len())?;
        let search_end = record_end.checked_add(24)?.min(last_next_class);
        let next_class = (record_end..=search_end).find(|offset| {
            payload.get(record_end..*offset).is_some_and(|padding| {
                padding.iter().all(|byte| *byte == 0)
                    && payload.get(*offset..*offset + temporary_axis::NEXT_CLASS_MARKER_VALUE.len())
                        == Some(&temporary_axis::NEXT_CLASS_MARKER_VALUE)
            })
        })?;
        (next_class < end).then_some((
            origin,
            Vector3::new(direction.x / norm, direction.y / norm, direction.z / norm),
        ))
    });
    let first = candidates.next()?;
    candidates
        .all(|candidate| candidate == first)
        .then_some(first)
}

/// Add profile ownership and placed axes carried by revolution reference records.
pub(crate) fn enrich_history_revolution_inputs(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let name_counts = histories.iter().flat_map(|history| &history.features).fold(
        HashMap::<String, usize>::new(),
        |mut counts, feature| {
            *counts.entry(feature.name.clone()).or_default() += 1;
            counts
        },
    );
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.source_id.is_none() && is_profile_feature_object(feature))
    {
        if name_counts.get(feature.name.as_str()) != Some(&1) {
            continue;
        }
        let mut object_ids = lanes
            .iter()
            .filter_map(|lane| feature_object_name(feature, lane)?.object_id)
            .collect::<Vec<_>>();
        object_ids.sort_unstable();
        object_ids.dedup();
        if let [object_id] = object_ids.as_slice() {
            feature.source_id = Some(object_id.to_string());
        }
    }
    let mut profile_sources = HashMap::<String, HashSet<u32>>::new();
    for history in histories.iter() {
        let sources = history
            .features
            .iter()
            .filter(|feature| is_profile_feature_object(feature))
            .flat_map(|feature| {
                feature
                    .source_id
                    .as_deref()
                    .and_then(|source| source.parse::<u32>().ok())
                    .into_iter()
                    .chain(
                        lanes
                            .iter()
                            .filter_map(|lane| feature_object_name(feature, lane)?.object_id),
                    )
            })
            .collect::<HashSet<_>>();
        for feature in &history.features {
            profile_sources.insert(feature.id.clone(), sources.clone());
        }
    }
    let mut profiles = HashMap::<String, Vec<Option<u32>>>::new();
    let mut inputs = HashMap::<String, Vec<Option<(Point3, Vector3)>>>::new();
    for lane in lanes {
        for history in histories.iter() {
            let mut objects = history
                .features
                .iter()
                .filter_map(|feature| Some((feature_object_name(feature, lane)?.offset, feature)))
                .collect::<Vec<_>>();
            objects.sort_unstable_by_key(|(offset, _)| *offset);
            for (index, &(start, feature)) in objects.iter().enumerate() {
                if !matches!(
                    feature.input_class.as_deref(),
                    Some("moRevolution_c" | "moRevCut_c")
                ) {
                    continue;
                }
                let immediate_profile = index
                    .checked_sub(1)
                    .and_then(|index| objects.get(index))
                    .map(|(_, feature)| *feature)
                    .filter(|feature| is_profile_feature_object(feature))
                    .and_then(|feature| feature_object_name(feature, lane)?.object_id);
                let Some(known_profiles) = profile_sources.get(&feature.id) else {
                    continue;
                };
                let Some(start) = usize::try_from(start).ok() else {
                    continue;
                };
                let end = objects
                    .get(index + 1)
                    .and_then(|(offset, _)| usize::try_from(*offset).ok())
                    .unwrap_or(lane.native_payload.len());
                let line_reference = revolution_line_reference_inputs(
                    &lane.native_payload,
                    start,
                    end,
                    known_profiles,
                );
                let placed_axis = line_reference
                    .map(|(_, origin, direction)| (origin, direction))
                    .or_else(|| temporary_axis_reference(&lane.native_payload, start, end));
                profiles
                    .entry(feature.id.clone())
                    .or_default()
                    .push(immediate_profile.or_else(|| line_reference.map(|input| input.0)));
                inputs
                    .entry(feature.id.clone())
                    .or_default()
                    .push(placed_axis);
            }
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        if !feature.properties.contains_key("Profile") {
            if let Some(votes) = profiles.get(&feature.id) {
                if let Some(Some(first)) = votes.first() {
                    if votes.iter().all(|vote| vote == &Some(*first))
                        && profile_sources
                            .get(&feature.id)
                            .is_some_and(|sources| sources.contains(first))
                    {
                        feature
                            .properties
                            .insert("Profile".into(), first.to_string());
                    }
                }
            }
        }
        let Some(votes) = inputs.get(&feature.id) else {
            continue;
        };
        let Some(Some(first)) = votes.first() else {
            continue;
        };
        if !votes.iter().all(|vote| vote.as_ref() == Some(first)) {
            continue;
        }
        if !feature.properties.contains_key("AxisOrigin")
            && !feature.properties.contains_key("AxisDirection")
        {
            feature.properties.insert(
                "AxisOrigin".into(),
                format!("{}mm,{}mm,{}mm", first.0.x, first.0.y, first.0.z),
            );
            feature.properties.insert(
                "AxisDirection".into(),
                format!("{},{},{}", first.1.x, first.1.y, first.1.z),
            );
        }
    }
}

/// Bind revolution axes from profile records or complete coaxial generated geometry.
pub(crate) fn bind_profile_revolution_axes(
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    sketches: &[Sketch],
    surfaces: &[Surface],
) {
    let native_by_id = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let model_by_id = model_features
        .iter()
        .enumerate()
        .map(|(index, feature)| (&feature.id, index))
        .collect::<HashMap<_, _>>();
    let sketch_by_id = sketches
        .iter()
        .map(|sketch| (&sketch.id, sketch))
        .collect::<HashMap<_, _>>();
    let mut assignments = Vec::<(usize, cadmpeg_ir::features::RevolutionAxis)>::new();

    for (feature_index, feature) in model_features.iter().enumerate() {
        let FeatureDefinition::Revolve { construction, .. } = &feature.definition else {
            continue;
        };
        if construction.axis.is_some() {
            continue;
        }
        let Some(profile) = construction.profile.as_ref() else {
            continue;
        };
        let (profile_native, sketch_id) = match profile {
            cadmpeg_ir::features::ProfileRef::Feature(profile_id) => {
                let Some(&profile_index) = model_by_id.get(profile_id) else {
                    continue;
                };
                let profile_feature = &model_features[profile_index];
                let FeatureDefinition::Sketch {
                    space: cadmpeg_ir::features::SketchSpace::Planar,
                    sketch: Some(sketch),
                    ..
                } = &profile_feature.definition
                else {
                    continue;
                };
                let Some(native) = profile_feature.native_ref.as_deref() else {
                    continue;
                };
                (native, sketch)
            }
            cadmpeg_ir::features::ProfileRef::Sketch(sketch_id) => {
                let mut owners = model_features.iter().filter(|candidate| {
                    matches!(
                        &candidate.definition,
                        FeatureDefinition::Sketch {
                            space: cadmpeg_ir::features::SketchSpace::Planar,
                            sketch: Some(candidate),
                            ..
                        } if candidate == sketch_id
                    )
                });
                let Some(owner) = owners.next() else {
                    continue;
                };
                if owners.next().is_some() {
                    continue;
                }
                let Some(native) = owner.native_ref.as_deref() else {
                    continue;
                };
                (native, sketch_id)
            }
            cadmpeg_ir::features::ProfileRef::Generated { .. }
            | cadmpeg_ir::features::ProfileRef::SketchProfiles { .. }
            | cadmpeg_ir::features::ProfileRef::SketchRegions { .. }
            | cadmpeg_ir::features::ProfileRef::SketchEntities { .. }
            | cadmpeg_ir::features::ProfileRef::SketchSelection { .. }
            | cadmpeg_ir::features::ProfileRef::SpatialSketchProfiles { .. }
            | cadmpeg_ir::features::ProfileRef::SpatialSketchSelection { .. }
            | cadmpeg_ir::features::ProfileRef::HistoricalFaces { .. }
            | cadmpeg_ir::features::ProfileRef::Unresolved(_)
            | cadmpeg_ir::features::ProfileRef::Native(_)
            | cadmpeg_ir::features::ProfileRef::Faces(_) => continue,
        };
        if !native_by_id
            .get(profile_native)
            .is_some_and(|feature| is_profile_feature_object(feature))
        {
            continue;
        }
        let Some(sketch) = sketch_by_id.get(sketch_id).copied() else {
            continue;
        };
        let generated_axis_surfaces = if matches!(
            construction.extent.as_ref(),
            Some(cadmpeg_ir::features::RevolveExtent::OneSided {
                termination: cadmpeg_ir::features::Termination::Angle { angle },
            }) if (angle.0.abs() - std::f64::consts::TAU).abs() <= EPS_AXES_BIND_PROFILE_REVOLUTION_AXES_E9
        ) {
            surfaces
        } else {
            &[]
        };
        let mut candidates = lanes
            .iter()
            .filter_map(|lane| {
                profile_roster_construction_axis(
                    lane,
                    profile_native,
                    sketch,
                    generated_axis_surfaces,
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|axis| {
            [
                axis.origin.x.to_bits(),
                axis.origin.y.to_bits(),
                axis.origin.z.to_bits(),
                axis.direction.x.to_bits(),
                axis.direction.y.to_bits(),
                axis.direction.z.to_bits(),
            ]
        });
        candidates.dedup();
        if let [axis] = candidates.as_slice() {
            assignments.push((feature_index, *axis));
        }
    }

    for (index, axis) in assignments {
        if let FeatureDefinition::Revolve { construction, .. } =
            &mut model_features[index].definition
        {
            if construction.axis.is_none() {
                construction.axis = Some(axis);
            }
        }
    }
}

pub(super) fn profile_roster_construction_axis(
    lane: &FeatureInputLane,
    profile_native: &str,
    sketch: &Sketch,
    surfaces: &[Surface],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    const QUANTUM: f64 = 1e-8;
    const NATIVE_TO_IR: f64 = 1000.0;
    let (origin, normal, u_axis) = sketch.resolved_placement()?;

    let markers = lane.sketch_entities.iter().collect::<Vec<_>>();
    let mut axes = lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
        .filter_map(|marker| {
            let offset = usize::try_from(marker.offset).ok()?;
            if !marker_is_selected_construction_line(&lane.native_payload, offset) {
                return None;
            }
            let endpoints = roster_curve_endpoint_markers(&lane.native_payload, marker, &markers);
            let [start, end] = endpoints.as_slice() else {
                return None;
            };
            Some([*start, *end])
        });
    let native_endpoints = match (axes.next(), axes.next()) {
        (Some(endpoints), None) => Some([endpoints[0].coordinates_m?, endpoints[1].coordinates_m?]),
        (None, None) => {
            if let Some(endpoints) =
                profile_roster_implicit_axis_endpoints(lane, profile_native, &markers)
            {
                Some([endpoints[0].coordinates_m?, endpoints[1].coordinates_m?])
            } else {
                profile_roster_origin_axis_endpoints(lane, profile_native, &markers).or_else(|| {
                    profile_roster_principal_axis_endpoints(lane, profile_native, &markers)
                })
            }
        }
        _ => return None,
    };
    let transform = sketch_frame_marker_transform(sketch, QUANTUM)?;
    let Some([native_start, native_end]) = native_endpoints else {
        return profile_generated_surface_axis(
            lane,
            profile_native,
            &markers,
            sketch,
            &transform,
            surfaces,
        );
    };
    let project = |point: [f64; 2]| {
        let point = transform.apply(quantize(
            Point2::new(point[0] * NATIVE_TO_IR, point[1] * NATIVE_TO_IR),
            QUANTUM,
        ))?;
        Some(Point2::new(
            point.0 as f64 * QUANTUM,
            point.1 as f64 * QUANTUM,
        ))
    };
    let start = project(native_start)?;
    let end = project(native_end)?;
    let v_axis = normal.cross(u_axis);
    let point = |point: Point2| {
        Point3::new(
            origin.x + point.u * u_axis.x + point.v * v_axis.x,
            origin.y + point.u * u_axis.y + point.v * v_axis.y,
            origin.z + point.u * u_axis.z + point.v * v_axis.z,
        )
    };
    let start = point(start);
    let end = point(end);
    let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
    let length = (delta.x * delta.x + delta.y * delta.y + delta.z * delta.z).sqrt();
    (length.is_finite() && length > EPS_AXES_PROFILE_ROSTER_CONSTRUCTION_AXIS_E9).then_some(
        cadmpeg_ir::features::RevolutionAxis {
            origin: start,
            direction: Vector3::new(delta.x / length, delta.y / length, delta.z / length),
        },
    )
}

fn profile_generated_surface_axis(
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&SketchInputEntity],
    sketch: &Sketch,
    transform: &MarkerTransform,
    surfaces: &[Surface],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    const QUANTUM: f64 = 1e-8;
    const NATIVE_TO_IR: f64 = 1000.0;
    const LINE_TOLERANCE: f64 = 1e-6;
    let (origin, normal, u_axis) = sketch.resolved_placement()?;

    let mut axis = common_generated_surface_axis(surfaces)?;
    let relative_origin = Vector3::new(
        axis.origin.x - origin.x,
        axis.origin.y - origin.y,
        axis.origin.z - origin.z,
    );
    if axis.direction.dot(normal).abs() > EPS_AXES_PROFILE_GENERATED_SURFACE_AXIS_E9
        || relative_origin.dot(normal).abs() > LINE_TOLERANCE
    {
        return None;
    }
    let origin_offset = Vector3::new(
        origin.x - axis.origin.x,
        origin.y - axis.origin.y,
        origin.z - axis.origin.z,
    );
    let perpendicular = origin_offset.cross(axis.direction);
    if perpendicular.norm() <= LINE_TOLERANCE {
        axis.origin = origin;
    } else {
        let projection = origin_offset.dot(axis.direction);
        axis.origin = Point3::new(
            axis.origin.x + projection * axis.direction.x,
            axis.origin.y + projection * axis.direction.y,
            axis.origin.z + projection * axis.direction.z,
        );
    }
    let curve_endpoints = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
        .flat_map(|curve| roster_curve_endpoint_markers(&lane.native_payload, curve, markers))
        .filter(|endpoint| endpoint.object_index.is_some())
        .collect::<Vec<_>>();
    let mut endpoint_ids = HashSet::new();
    let v_axis = normal.cross(u_axis);
    let mut sides = Vec::new();
    for endpoint in curve_endpoints {
        if !endpoint_ids.insert(endpoint.id.as_str()) {
            continue;
        }
        let [u, v] = endpoint.coordinates_m?;
        let point = transform.apply(quantize(
            Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
            QUANTUM,
        ))?;
        let point = Point3::new(
            origin.x + point.0 as f64 * QUANTUM * u_axis.x + point.1 as f64 * QUANTUM * v_axis.x,
            origin.y + point.0 as f64 * QUANTUM * u_axis.y + point.1 as f64 * QUANTUM * v_axis.y,
            origin.z + point.0 as f64 * QUANTUM * u_axis.z + point.1 as f64 * QUANTUM * v_axis.z,
        );
        let relative = Vector3::new(
            point.x - axis.origin.x,
            point.y - axis.origin.y,
            point.z - axis.origin.z,
        );
        sides.push(axis.direction.cross(relative).dot(normal));
    }
    if sides.len() < 2
        || !sides.iter().any(|side| side.abs() > LINE_TOLERANCE)
        || (sides.iter().any(|side| *side > LINE_TOLERANCE)
            && sides.iter().any(|side| *side < -LINE_TOLERANCE))
    {
        return None;
    }
    Some(axis)
}

pub(super) fn common_generated_surface_axis(
    surfaces: &[Surface],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    const DIRECTION_TOLERANCE: f64 = 1e-9;
    const LINE_TOLERANCE: f64 = 1e-6;

    let axes = surfaces
        .iter()
        .filter_map(|surface| match &surface.geometry {
            SurfaceGeometry::Cylinder { origin, axis, .. }
            | SurfaceGeometry::Cone { origin, axis, .. } => Some((*origin, *axis)),
            SurfaceGeometry::Torus { center, axis, .. } => Some((*center, *axis)),
            SurfaceGeometry::Plane { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Nurbs(_)
            | SurfaceGeometry::Polygonal { .. }
            | SurfaceGeometry::Procedural { .. }
            | SurfaceGeometry::Transformed { .. }
            | SurfaceGeometry::Unknown { .. } => None,
        })
        .collect::<Vec<_>>();
    let [(origin, direction), ..] = axes.as_slice() else {
        return None;
    };
    if axes.len() < 2 {
        return None;
    }
    let length = direction.norm();
    if !length.is_finite() || length <= EPS_AXES_COMMON_GENERATED_SURFACE_AXIS_E9 {
        return None;
    }
    let mut direction = Vector3::new(
        direction.x / length,
        direction.y / length,
        direction.z / length,
    );
    for (candidate_origin, candidate_direction) in &axes[1..] {
        let candidate_length = candidate_direction.norm();
        if !candidate_length.is_finite()
            || candidate_length <= EPS_AXES_COMMON_GENERATED_SURFACE_AXIS_E9
        {
            return None;
        }
        let candidate_direction = Vector3::new(
            candidate_direction.x / candidate_length,
            candidate_direction.y / candidate_length,
            candidate_direction.z / candidate_length,
        );
        let origin_delta = Vector3::new(
            candidate_origin.x - origin.x,
            candidate_origin.y - origin.y,
            candidate_origin.z - origin.z,
        );
        let direction_cross = direction.cross(candidate_direction);
        let line_offset = origin_delta.cross(direction);
        if direction_cross.norm() > DIRECTION_TOLERANCE || line_offset.norm() > LINE_TOLERANCE {
            return None;
        }
    }
    if direction.x < -DIRECTION_TOLERANCE
        || (direction.x.abs() <= DIRECTION_TOLERANCE && direction.y < -DIRECTION_TOLERANCE)
        || (direction.x.abs() <= DIRECTION_TOLERANCE
            && direction.y.abs() <= DIRECTION_TOLERANCE
            && direction.z < 0.0)
    {
        direction = Vector3::new(-direction.x, -direction.y, -direction.z);
    }
    let origin_projection = Vector3::new(origin.x, origin.y, origin.z).dot(direction);
    let origin = Point3::new(
        origin.x - origin_projection * direction.x,
        origin.y - origin_projection * direction.y,
        origin.z - origin_projection * direction.z,
    );
    Some(cadmpeg_ir::features::RevolutionAxis { origin, direction })
}

pub(super) fn profile_roster_origin_axis_endpoints(
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&SketchInputEntity],
) -> Option<[[f64; 2]; 2]> {
    let curve_endpoints = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
        .flat_map(|curve| roster_curve_endpoint_markers(&lane.native_payload, curve, markers))
        .filter(|endpoint| endpoint.object_index.is_some())
        .map(|endpoint| endpoint.id.as_str())
        .collect::<HashSet<_>>();
    let unreferenced_points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref.as_deref() == Some(profile_native)
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && marker.coordinates_m.is_some()
                && !curve_endpoints.contains(marker.id.as_str())
        })
        .collect::<Vec<_>>();
    let [origin] = unreferenced_points.as_slice() else {
        return None;
    };
    let [origin_u, origin_v] = origin.coordinates_m?;
    if origin_u.abs() > EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
        || origin_v.abs() > EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
    {
        return None;
    }
    let mut candidates = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.object_index.is_some() && curve_endpoints.contains(marker.id.as_str())
        })
        .filter_map(|marker| {
            let end = marker.coordinates_m?;
            let endpoints = [[origin_u, origin_v], end];
            bounded_profile_axis_coordinates(profile_native, markers, &curve_endpoints, endpoints)
                .then_some(endpoints)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left[1][0]
            .total_cmp(&right[1][0])
            .then(left[1][1].total_cmp(&right[1][1]))
    });
    let mut lines = Vec::<[[f64; 2]; 2]>::new();
    for candidate in candidates {
        let [u, v] = [candidate[1][0] - origin_u, candidate[1][1] - origin_v];
        if lines.iter().any(|line| {
            let [line_u, line_v] = [line[1][0] - origin_u, line[1][1] - origin_v];
            (u * line_v - v * line_u).abs()
                <= EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
                    * u.hypot(v)
                    * line_u.hypot(line_v)
        }) {
            continue;
        }
        lines.push(candidate);
    }
    let incidence = |line: &[[f64; 2]; 2]| {
        let [line_u, line_v] = [line[1][0] - origin_u, line[1][1] - origin_v];
        markers
            .iter()
            .filter(|marker| {
                marker.object_index.is_some() && curve_endpoints.contains(marker.id.as_str())
            })
            .filter_map(|marker| marker.coordinates_m)
            .filter(|[u, v]| {
                let relative_u = u - origin_u;
                let relative_v = v - origin_v;
                (relative_u * line_v - relative_v * line_u).abs()
                    <= EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
                        * relative_u.hypot(relative_v)
                        * line_u.hypot(line_v)
            })
            .count()
    };
    let maximum_incidence = lines.iter().map(incidence).max()?;
    let selected = lines
        .iter()
        .filter(|line| incidence(line) == maximum_incidence)
        .collect::<Vec<_>>();
    let [axis] = selected.as_slice() else {
        return None;
    };
    Some(**axis)
}

pub(super) fn profile_roster_principal_axis_endpoints(
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&SketchInputEntity],
) -> Option<[[f64; 2]; 2]> {
    let curve_endpoints = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
        .flat_map(|curve| roster_curve_endpoint_markers(&lane.native_payload, curve, markers))
        .filter(|endpoint| endpoint.object_index.is_some())
        .map(|endpoint| endpoint.id.as_str())
        .collect::<HashSet<_>>();
    let incidence = |axis: &[[f64; 2]; 2]| {
        let [axis_u, axis_v] = axis[1];
        markers
            .iter()
            .filter(|marker| curve_endpoints.contains(marker.id.as_str()))
            .filter_map(|marker| marker.coordinates_m)
            .filter(|[u, v]| {
                (u * axis_v - v * axis_u).abs()
                    <= EPS_AXES_PROFILE_ROSTER_PRINCIPAL_AXIS_ENDPOINTS_E9
            })
            .count()
    };
    let axes = [[[0.0, 0.0], [1.0, 0.0]], [[0.0, 0.0], [0.0, 1.0]]];
    let candidates = axes
        .into_iter()
        .filter(|axis| {
            bounded_profile_axis_coordinates(profile_native, markers, &curve_endpoints, *axis)
        })
        .map(|axis| (incidence(&axis), axis))
        .collect::<Vec<_>>();
    let maximum_incidence = candidates.iter().map(|(count, _)| *count).max()?;
    if maximum_incidence < 2 {
        return None;
    }
    let selected = candidates
        .iter()
        .filter(|(count, _)| *count == maximum_incidence)
        .collect::<Vec<_>>();
    let [(_, axis)] = selected.as_slice() else {
        return None;
    };
    Some(*axis)
}

fn profile_roster_implicit_axis_endpoints<'a>(
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&'a SketchInputEntity],
) -> Option<[&'a SketchInputEntity; 2]> {
    let curve_candidates = markers.iter().copied().filter(|marker| {
        let Ok(offset) = usize::try_from(marker.offset) else {
            return false;
        };
        if marker.feature_ref.as_deref() != Some(profile_native) {
            return false;
        }
        let current_code_two = lane
            .native_payload
            .get(offset..offset + SKETCH_MARKER.len())
            == Some(SKETCH_MARKER)
            && lane.native_payload.get(offset + 17..offset + 21) == Some(&2u32.to_le_bytes())
            && (compact_indexed_curve_endpoint_indices(&lane.native_payload, offset).is_some()
                || wide_indexed_curve_endpoint_indices(&lane.native_payload, offset).is_some());
        let detailed_indexed_curve = View::u32_le_at(&lane.native_payload, offset + 17)
            .is_some_and(|code| matches!(code, 0 | 2))
            && compact_bounded_curve_tangent(&lane.native_payload, offset).is_some();
        current_code_two || detailed_indexed_curve
    });
    let curve_candidates = curve_candidates.collect::<Vec<_>>();
    let curve_endpoints = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
        .flat_map(|curve| roster_curve_endpoint_markers(&lane.native_payload, curve, markers))
        .map(|endpoint| endpoint.id.as_str())
        .collect::<HashSet<_>>();
    let unreferenced_points = markers
        .iter()
        .copied()
        .filter(|marker| {
            marker.feature_ref.as_deref() == Some(profile_native)
                && matches!(
                    marker.kind,
                    SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                )
                && marker.coordinates_m.is_some()
                && !curve_endpoints.contains(marker.id.as_str())
        })
        .collect::<Vec<_>>();
    if let [start, end] = unreferenced_points.as_slice() {
        let endpoints = [*start, *end];
        if bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints) {
            return Some(endpoints);
        }
    }
    let selected_endpoints = unreferenced_points
        .iter()
        .copied()
        .filter(|marker| {
            usize::try_from(marker.offset).ok().is_some_and(|offset| {
                lane.native_payload.get(offset + 76..offset + 80) == Some(&1u32.to_le_bytes())
            })
        })
        .collect::<Vec<_>>();
    if let [start, end] = selected_endpoints.as_slice() {
        let endpoints = [*start, *end];
        if bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints) {
            return Some(endpoints);
        }
    }
    if let [end] = selected_endpoints.as_slice() {
        let mut owned = markers
            .iter()
            .copied()
            .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
            .collect::<Vec<_>>();
        owned.sort_unstable_by_key(|marker| marker.offset);
        if let Some(start) = owned
            .windows(2)
            .find_map(|pair| (pair[1].id == end.id).then_some(pair[0]))
            .filter(|marker| {
                marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind,
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            })
        {
            let endpoints = [start, *end];
            if bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints)
            {
                return Some(endpoints);
            }
        }
    }
    let mut boundary_relations = markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
        .filter(|marker| {
            usize::try_from(marker.offset).ok().is_some_and(|offset| {
                extended_wide_horizontal_relation_endpoint_indices(&lane.native_payload, offset)
                    .is_some()
            })
        })
        .filter_map(|candidate| {
            let endpoints = roster_curve_endpoint_markers(&lane.native_payload, candidate, markers);
            let [start, end] = endpoints.as_slice() else {
                return None;
            };
            let endpoints = [*start, *end];
            bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints)
                .then_some(endpoints)
        })
        .collect::<Vec<_>>();
    boundary_relations.sort_unstable_by_key(|endpoints| [endpoints[0].offset, endpoints[1].offset]);
    boundary_relations
        .dedup_by_key(|endpoints| [endpoints[0].id.as_str(), endpoints[1].id.as_str()]);
    match boundary_relations.as_slice() {
        [endpoints] => return Some(*endpoints),
        [] => {}
        _ => return None,
    }
    let [candidate] = curve_candidates.as_slice() else {
        return None;
    };
    let endpoints = roster_curve_endpoint_markers(&lane.native_payload, candidate, markers);
    let [start, end] = endpoints.as_slice() else {
        return None;
    };
    let endpoints = [*start, *end];
    bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints)
        .then_some(endpoints)
}

pub(super) fn bounded_profile_axis_endpoints(
    profile_native: &str,
    markers: &[&SketchInputEntity],
    curve_endpoints: &HashSet<&str>,
    endpoints: [&SketchInputEntity; 2],
) -> bool {
    let [Some(start), Some(end)] = endpoints.map(|endpoint| endpoint.coordinates_m) else {
        return false;
    };
    bounded_profile_axis_coordinates(profile_native, markers, curve_endpoints, [start, end])
}

fn bounded_profile_axis_coordinates(
    profile_native: &str,
    markers: &[&SketchInputEntity],
    curve_endpoints: &HashSet<&str>,
    endpoints: [[f64; 2]; 2],
) -> bool {
    const TOLERANCE_M: f64 = 1e-9;

    let [[start_u, start_v], [end_u, end_v]] = endpoints;
    let delta_u = end_u - start_u;
    let delta_v = end_v - start_v;
    let length = delta_u.hypot(delta_v);
    if !length.is_finite() || length <= TOLERANCE_M {
        return false;
    }
    let tangent_u = delta_u / length;
    let tangent_v = delta_v / length;
    let mut minimum_side = f64::INFINITY;
    let mut maximum_side = f64::NEG_INFINITY;
    let mut observed = false;
    for [u, v] in markers.iter().filter_map(|marker| {
        (marker.feature_ref.as_deref() == Some(profile_native)
            && matches!(
                marker.kind,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
            && marker.object_index.is_some()
            && curve_endpoints.contains(marker.id.as_str()))
        .then_some(marker.coordinates_m)
        .flatten()
    }) {
        let relative_u = u - start_u;
        let relative_v = v - start_v;
        let side = relative_u * -tangent_v + relative_v * tangent_u;
        observed = true;
        minimum_side = minimum_side.min(side);
        maximum_side = maximum_side.max(side);
    }
    observed && (minimum_side >= -TOLERANCE_M || maximum_side <= TOLERANCE_M)
}

#[cfg(test)]
mod axes_tests;
