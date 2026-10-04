//! Line reference directions and revolution axis inputs.
use cadmpeg_ir::features::PlanarProfileRef;

use super::component_paths::is_profile_feature_object;
use super::curves::compact_bounded_curve_tangent;
use super::endpoints::{
    compact_indexed_curve_endpoint_indices, extended_wide_horizontal_relation_endpoint_indices,
    marker_is_selected_construction_line, roster_curve_endpoint_markers,
    wide_indexed_curve_endpoint_indices,
};
use super::grid::quantize;
use super::scalars::feature_object_name;
use super::transforms::{sketch_frame_marker_transform, MarkerTransform};
use super::{is_class_token, CLASS_MARKER, SKETCH_MARKER};
use crate::layout::temporary_axis_reference_nine_scalar as temporary_axis;
use crate::records::FeatureSource;
use crate::records::ObjectId;
use crate::records::{FeatureInputLane, FeatureInputName, SketchInputEntity, SketchInputKind};
use cadmpeg_core::convert::f64_from_i64;
use cadmpeg_core::decode::index_from_u64;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::units::{SumSquaresUnitVector3, UnitVector3};
use cadmpeg_ir::{
    features::{FeatureDefinition, FeatureOperation},
    scalar::PositiveLength,
};
use std::collections::{HashMap, HashSet};

const EPS_AXES_BIND_PROFILE_REVOLUTION_AXES_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_CONSTRUCTION_AXIS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_GENERATED_SURFACE_AXIS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_PRINCIPAL_AXIS_ENDPOINTS_E9: f64 = 1e-9;

fn square_sum_unit_direction(values: [f64; 3]) -> Option<UnitVector3> {
    let [x, y, z] = values;
    SumSquaresUnitVector3::new(Vector3::new(x, y, z)).map(SumSquaresUnitVector3::normalized)
}

pub(super) fn line_reference_direction(
    ctx: &DecodeContext<'_>, payload: &[u8], class_offset: u64,
) -> Result<Option<UnitVector3>, CodecError> {
    let Ok(class_offset) = usize::try_from(class_offset) else {
        return Ok(None);
    };
    let direction_at = |offset: usize| {
        square_sum_unit_direction([
            View::f64_le_at(payload, offset)?,
            View::f64_le_at(payload, offset + 8)?,
            View::f64_le_at(payload, offset + 16)?,
        ])
    };
    let mut direction_storage = ctx.reserve_scoped(0, "hold SLDPRT line reference directions")?;
    let mut directions = Vec::new();
    if payload.get(class_offset + 136..class_offset + 144)
        == Some(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff])
        && payload.get(class_offset + 148..class_offset + 152) == Some(&[0xf8, 0x2a, 0, 0])
    {
        if let Some(direction) = direction_at(class_offset + 200) {
            direction_storage.with_storage(|| ctx.push_vec(&mut directions, direction, "collect SLDPRT line reference directions"))?;
        }
    }
    if payload.get(class_offset + 144..class_offset + 156)
        == Some(&[
            0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff,
        ])
        && payload.get(class_offset + 160..class_offset + 164) == Some(&[0xf8, 0x2a, 0, 0])
    {
        if let Some(direction) = direction_at(class_offset + 220) {
            direction_storage.with_storage(|| ctx.push_vec(&mut directions, direction, "collect SLDPRT line reference directions"))?;
        }
    }
    // Both declared layouts are evaluated before selecting the direction so
    // a future overlapping layout cannot win merely by branch order.
    directions.dedup();
    let [direction] = directions.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*direction))
}

pub(super) fn declared_line_reference_directions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class_offset: u64,
    object_end: usize,
) -> Result<Vec<UnitVector3>, CodecError> {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];

    let Ok(class_offset) = usize::try_from(class_offset) else {
        return Ok(Vec::new());
    };
    let Some(end) = super::DeclaredEnd::of(object_end, payload.len()).map(super::DeclaredEnd::get)
    else {
        return Ok(Vec::new());
    };
    let mut directions = Vec::new();
    if let Some(direction) = line_reference_direction(ctx, &payload[..end], u64_from_index(class_offset))?
    {
        ctx.reserve_vec(
            &mut directions,
            1,
            "collect SLDPRT declared line directions",
        )?;
        directions.push(direction);
    }
    let Some(final_handle) = end
        .checked_sub(88)
        .filter(|final_handle| *final_handle >= class_offset)
    else {
        return Ok(directions);
    };
    for handle in class_offset..=final_handle {
        ctx.charge_work(1, "scan SLDPRT declared line directions")?;
        if payload.get(handle..handle + HANDLES.len()) != Some(HANDLES.as_slice()) {
            continue;
        }
        let scalar = |relative: usize| {
            let offset = handle.checked_add(relative)?;
            let value = View::f64_le_at(payload, offset)?;
            value.is_finite().then_some(value)
        };
        let direction_at = |relative: usize| {
            square_sum_unit_direction([
                View::f64_le_at(payload, handle.checked_add(relative)?)?,
                View::f64_le_at(payload, handle.checked_add(relative + 8)?)?,
                View::f64_le_at(payload, handle.checked_add(relative + 16)?)?,
            ])
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
                ctx.reserve_vec(
                    &mut directions,
                    1,
                    "collect SLDPRT declared line directions",
                )?;
                directions.push(candidate);
            }
        }
    }
    Ok(directions)
}

pub(super) fn linear_pattern_display_directions(
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    names: &[FeatureInputName],
    expected_spacing_m: [Option<f64>; 2],
) -> Vec<UnitVector3> {
    const VALUE_OFFSET: usize = 32;
    const DIRECTION_OFFSET: usize = 161;
    const LENGTH_TOLERANCE_M: f64 = 1e-8;

    let Some(end) = super::DeclaredEnd::of(object_end, payload.len()).map(super::DeclaredEnd::get)
    else {
        return Vec::new();
    };
    ["D3", "D4"]
        .into_iter()
        .zip(expected_spacing_m)
        .filter_map(|(dimension_name, expected)| {
            let expected = expected?;
            let mut records = names.iter().filter(|name| {
                name.object_id == Some(ObjectId::Absent)
                    && name.value == dimension_name
                    && (u64_from_index(object_start)..u64_from_index(end)).contains(&name.offset)
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
            square_sum_unit_direction([
                View::f64_le_at(payload, direction_offset)?,
                View::f64_le_at(payload, direction_offset.checked_add(8)?)?,
                View::f64_le_at(payload, direction_offset.checked_add(16)?)?,
            ])
        })
        .collect()
}

pub(super) fn typed_linear_pattern_dimensions(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Result<Option<(PositiveLength, u32)>, CodecError> {
    const OPERATION: &str = "look up SLDPRT linear pattern dimensions";
    let parameter = |class_name: &str| {
        let name = (|| {
            let mut classes = lane.classes.iter().filter(|class| {
                class.name == class_name
                    && (u64_from_index(object_start)..u64_from_index(object_end))
                        .contains(&class.offset)
            });
            let class = classes.next().filter(|_| classes.next().is_none())?;
            let class_offset = usize::try_from(class.offset).ok()?;
            let name_end = class_offset.checked_add(128)?.min(object_end);
            let mut names = lane.names.iter().filter(|name| {
                name.object_id == Some(ObjectId::Absent)
                    && (u64_from_index(class_offset)..u64_from_index(name_end)).contains(&name.offset)
                    && feature.parameters.contains_key(name.value.as_str())
            });
            names.next().filter(|_| names.next().is_none())
        })();
        match name {
            Some(name) => ctx.get_btree_map(&feature.parameters, name.value.as_str(), OPERATION),
            None => Ok(None),
        }
    };
    let Some(count) = parameter("moNumberDim_c")?
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|count| *count > 0)
    else {
        return Ok(None);
    };
    let spacing = parameter("ParallelPlaneDistanceDim_c")?
        .and_then(|value| crate::history::literals::parse_positive_dimension_length_mm(value));
    Ok(spacing.map(|spacing| (spacing, count)))
}

#[cfg(test)]
pub(super) fn compact_line_reference_direction(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    excluded_handles: &[usize],
) -> Result<Option<UnitVector3>, CodecError> {
    let directions = compact_line_reference_directions(
        ctx,
        payload,
        object_start,
        object_end,
        excluded_handles,
    )?;
    let [direction] = directions.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*direction))
}

pub(super) fn compact_line_reference_directions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    excluded_handles: &[usize],
) -> Result<Vec<UnitVector3>, CodecError> {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let Some(end) = super::DeclaredEnd::of(object_end, payload.len()).map(super::DeclaredEnd::get)
    else {
        return Ok(Vec::new());
    };
    let Some(final_handle) = end.checked_sub(80).filter(|end| *end >= object_start) else {
        return Ok(Vec::new());
    };
    let mut unique_directions = Vec::new();
    for handle in object_start..=final_handle {
        ctx.charge_work(1, "scan SLDPRT compact line directions")?;
        if excluded_handles.contains(&handle) {
            continue;
        }
        let Some(record) = payload.get(handle..end) else {
            continue;
        };
        let Some(address) = View::u32_le_at(record, 12) else {
            continue;
        };
        if record[..8] != HANDLES || record[8..12] != [0; 4] {
            continue;
        }
        let direction_at = |offset: usize| {
            square_sum_unit_direction([
                View::f64_le_at(record, offset)?,
                View::f64_le_at(record, offset + 8)?,
                View::f64_le_at(record, offset + 16)?,
            ])
        };
        let mut direction_storage = ctx.reserve_scoped(0, "hold SLDPRT compact line directions")?;
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
            if directions.len() == 1 {
                let candidate = directions[0];
                if !unique_directions.contains(&candidate) {
                    ctx.reserve_vec(
                        &mut unique_directions,
                        1,
                        "collect SLDPRT compact line directions",
                    )?;
                    unique_directions.push(candidate);
                }
            }
            continue;
        }
        let shifted_nine_scalar_trailer = record.get(96..104) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
            && record.get(104..116) == Some(&[0; 12])
            && tagged_token(116)
            && record.get(118..134) == Some(&[0; 16])
            && record.get(134..136) == Some(&[0xff; 2]);
        if record.get(16..24) == Some(&[0; 8]) && shifted_nine_scalar_trailer {
            if let Some(direction) = direction_at(72) {
                direction_storage.with_storage(|| ctx.push_vec(&mut directions, direction, "collect SLDPRT compact line directions"))?;
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
                direction_storage.with_storage(|| ctx.push_vec(&mut directions, direction, "collect SLDPRT compact line directions"))?;
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
                direction_storage.with_storage(|| ctx.push_vec(&mut directions, direction, "collect SLDPRT compact line directions"))?;
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
                direction_storage.with_storage(|| ctx.push_vec(&mut directions, direction, "collect SLDPRT compact line directions"))?;
            }
        }
        if directions.is_empty()
            && record.get(16..32) == Some(&[0; 16])
            && record.get(88..104) == Some(&[0; 16])
            && record.get(104..112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
            && record.get(112..136) == Some(&[0; 24])
        {
            if let Some(direction) = direction_at(64) {
                direction_storage.with_storage(|| ctx.push_vec(&mut directions, direction, "collect SLDPRT compact line directions"))?;
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
                direction_storage.with_storage(|| ctx.push_vec(&mut directions, direction, "collect SLDPRT compact line directions"))?;
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
                let mut distinct_storage = ctx.reserve_scoped(0, "hold SLDPRT distinct compact line directions")?;
                let mut distinct = Vec::new();
                for candidate in candidates {
                    if !distinct.contains(&candidate) {
                        distinct_storage.with_storage(|| ctx.push_vec(&mut distinct, candidate, "collect SLDPRT distinct compact line directions"))?;
                    }
                }
                if let [direction] = distinct.as_slice() {
                    direction_storage.with_storage(|| ctx.push_vec(&mut directions, *direction, "collect SLDPRT compact line directions"))?;
                }
            } else {
                directions.extend(direction_at(56));
            }
        }
        // A record is usable only when every matching layout agrees.  Never
        // let the order of the recognizers choose between distinct vectors.
        directions.dedup();
        if directions.len() == 1 {
            let candidate = directions[0];
            if !unique_directions.contains(&candidate) {
                ctx.reserve_vec(
                    &mut unique_directions,
                    1,
                    "collect SLDPRT compact line directions",
                )?;
                unique_directions.push(candidate);
            }
        }
    }
    Ok(unique_directions)
}

fn revolution_line_reference_inputs(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    profile_sources: &HashSet<u32>,
) -> Result<Option<(u32, cadmpeg_ir::features::FinitePoint3, UnitVector3)>, CodecError> {
    const HANDLE: [u8; 4] = [0xc7, 0xcf, 0xff, 0xff];
    const NATIVE_TO_IR: f64 = 1000.0;

    let Some(search_end) = super::DeclaredEnd::of(object_end, payload.len()) else {
        return Ok(None);
    };
    let search_end = search_end.get();
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
        Some((
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                x * NATIVE_TO_IR,
                y * NATIVE_TO_IR,
                z * NATIVE_TO_IR,
            ))?,
            square_sum_unit_direction([dx, dy, dz])?,
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
    let Some(scan_end) = search_end.checked_sub(64) else {
        return Ok(None);
    };
    let mut candidates = Vec::new();
    for handle_start in object_start..scan_end {
        ctx.charge_work(1, "scan SLDPRT revolution line references")?;
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
                if let (Some(origin), Some(direction)) = (
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                        x * NATIVE_TO_IR,
                        y * NATIVE_TO_IR,
                        z * NATIVE_TO_IR,
                    )),
                    square_sum_unit_direction([dx, dy, dz]),
                ) {
                    ctx.reserve_vec(
                        &mut candidates,
                        1,
                        "collect SLDPRT revolution line references",
                    )?;
                    candidates.push((handle_start, 6, (source, origin, direction)));
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
                        ctx.reserve_vec(
                            &mut candidates,
                            1,
                            "collect SLDPRT revolution line references",
                        )?;
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
                        ctx.reserve_vec(
                            &mut candidates,
                            1,
                            "collect SLDPRT revolution line references",
                        )?;
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
                        ctx.reserve_vec(
                            &mut candidates,
                            1,
                            "collect SLDPRT revolution line references",
                        )?;
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
                ctx.reserve_vec(
                    &mut candidates,
                    1,
                    "collect SLDPRT revolution line references",
                )?;
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
                            ctx.reserve_vec(
                                &mut candidates,
                                1,
                                "collect SLDPRT revolution line references",
                            )?;
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
            let Some(handles_end) = handle_start.checked_add(handle_count * HANDLE.len()) else {
                return Ok(None);
            };
            if (0..handle_count).any(|index| {
                let offset = handle_start + index * HANDLE.len();
                payload.get(offset..offset + HANDLE.len()) != Some(HANDLE.as_slice())
            }) || payload.get(handles_end..handles_end + 4) != Some(&[0; 4])
            {
                continue;
            }
            let Some(address) = View::u32_le_at(payload, handles_end + 4) else {
                return Ok(None);
            };
            if address == 0 {
                continue;
            }
            let mut unique_source = None;
            let mut conflicting_source = false;
            let Some(source_end) = handle_start.checked_sub(15) else {
                continue;
            };
            for offset in object_start..source_end {
                ctx.charge_work(1, "scan SLDPRT revolution profile sources")?;
                if let Some(source) = source_cell(offset) {
                    if unique_source.is_some_and(|known| known != source) {
                        conflicting_source = true;
                        break;
                    }
                    unique_source = Some(source);
                }
            }
            let Some(source) = unique_source.filter(|_| !conflicting_source) else {
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
                    let Some(direction) = square_sum_unit_direction([dx, dy, dz]) else {
                        continue;
                    };
                    let Some(origin) = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                        x * NATIVE_TO_IR,
                        y * NATIVE_TO_IR,
                        z * NATIVE_TO_IR,
                    )) else {
                        continue;
                    };
                    let candidate = (source, origin, direction);
                    let ranked = (handle_start, scalar_count, candidate);
                    if !candidates.contains(&ranked) {
                        ctx.reserve_vec(
                            &mut candidates,
                            1,
                            "collect SLDPRT revolution line references",
                        )?;
                        candidates.push(ranked);
                    }
                }
            }
        }
    }
    let mut ranks = HashMap::<usize, usize>::new();
    for (handle, rank, _) in &candidates {
        if let Some(current) = ranks.get_mut(handle) {
            *current = (*current).max(*rank);
        } else {
            ctx.insert_hash_map(
                &mut ranks,
                *handle,
                *rank,
                "index SLDPRT revolution line reference ranks",
            )?;
        }
    }
    let mut selected = Vec::new();
    for (handle, rank, candidate) in candidates {
        if ranks.get(&handle) == Some(&rank) {
            ctx.reserve_vec(
                &mut selected,
                1,
                "collect SLDPRT ranked revolution references",
            )?;
            selected.push(candidate);
        }
    }
    let mut candidates = selected;
    ctx.stable_sort_by_key(
        &mut candidates,
        |value| {
            (
                value.0,
                [
                    value.1.x.to_bits(),
                    value.1.y.to_bits(),
                    value.1.z.to_bits(),
                    value.2.as_raw().x.to_bits(),
                    value.2.as_raw().y.to_bits(),
                    value.2.as_raw().z.to_bits(),
                ],
            )
        },
        Ord::cmp,
        "sort SLDPRT revolution line references",
    )?;
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*candidate))
}

pub(super) fn temporary_axis_reference(
    payload: &[u8],
    object_start: usize,
    object_end: usize,
) -> Option<(cadmpeg_ir::features::FinitePoint3, UnitVector3)> {
    const NATIVE_TO_IR: f64 = 1000.0;

    let end = super::DeclaredEnd::of(object_end, payload.len())?.get();
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
        let origin = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
            frame[0] * NATIVE_TO_IR,
            frame[1] * NATIVE_TO_IR,
            frame[2] * NATIVE_TO_IR,
        ))?;
        let direction = square_sum_unit_direction([frame[6], frame[7], frame[8]])?;
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
        (next_class < end).then_some((origin, direction))
    });
    let first = candidates.next()?;
    candidates
        .all(|candidate| candidate == first)
        .then_some(first)
}

fn push_revolution_vote<T>(
    ctx: &DecodeContext<'_>,
    votes: &mut HashMap<String, Vec<T>>,
    id: &str,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(1, operation)?;
    if let Some(values) = ctx.get_mut_hash_map(votes, id, operation)? {
        ctx.reserve_vec(values, 1, operation)?;
        values.push(value);
        return Ok(());
    }
    ctx.reserve_map(votes, 1, operation)?;
    let id = ctx.format_retained(format_args!("{id}"), "retain SLDPRT revolution vote ID")?;
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, 1, operation)?;
    values.push(value);
    votes.insert(id, values);
    Ok(())
}

/// Add profile ownership and placed axes carried by revolution reference records.
pub(crate) fn enrich_history_revolution_inputs(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    let mut name_counts = HashMap::<String, usize>::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, "count SLDPRT revolution feature names")?;
        if let Some(count) = ctx.get_mut_hash_map(&mut name_counts, feature.name.as_str(), "lookup SLDPRT revolution feature name count")? {
            *count = count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "count SLDPRT revolution feature names",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
        } else {
            ctx.reserve_map(&mut name_counts, 1, "index SLDPRT revolution feature names")?;
            let name = ctx.format_retained(
                format_args!("{}", feature.name),
                "retain SLDPRT revolution feature name",
            )?;
            name_counts.insert(name, 1);
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.source_id.is_none() && is_profile_feature_object(feature))
    {
        if name_counts.get(feature.name.as_str()) != Some(&1) {
            continue;
        }
        let mut object_ids = Vec::new();
        for lane in lanes {
            ctx.charge_work(1, "find SLDPRT revolution profile source")?;
            if let Some(id) =
                feature_object_name(feature, lane).and_then(|name| name.object_id?.value())
            {
                ctx.reserve_vec(
                    &mut object_ids,
                    1,
                    "collect SLDPRT revolution profile sources",
                )?;
                object_ids.push(id);
            }
        }
        ctx.sort_unstable_by(
            &mut object_ids,
            |value| value,
            Ord::cmp,
            "sort SLDPRT revolution profile sources",
        )?;
        object_ids.dedup();
        if let [object_id] = object_ids.as_slice() {
            feature.source_id = FeatureSource::from_value(*object_id);
        }
    }
    let mut profile_sources = Vec::new();
    let mut profile_source_owner = HashMap::<String, usize>::new();
    for (history_index, history) in histories.iter().enumerate() {
        let mut sources = HashSet::new();
        for feature in history
            .features
            .iter()
            .filter(|feature| is_profile_feature_object(feature))
        {
            for source in feature.source_value().into_iter().chain(
                lanes
                    .iter()
                    .filter_map(|lane| feature_object_name(feature, lane)?.object_id?.value()),
            ) {
                ctx.charge_work(1, "index SLDPRT revolution profile sources")?;
                ctx.insert_hash_set(
                    &mut sources,
                    source,
                    "index SLDPRT revolution profile sources",
                )?;
            }
        }
        ctx.reserve_vec(
            &mut profile_sources,
            1,
            "collect SLDPRT revolution profile sets",
        )?;
        profile_sources.push(sources);
        for feature in &history.features {
            ctx.charge_work(1, "index SLDPRT revolution profile owners")?;
            if let Some(owner) = ctx.get_mut_hash_map(&mut profile_source_owner, feature.id.as_str(), "lookup SLDPRT revolution profile owner")? {
                *owner = history_index;
            } else {
                ctx.reserve_map(
                    &mut profile_source_owner,
                    1,
                    "index SLDPRT revolution profile owners",
                )?;
                let id = ctx.format_retained(
                    format_args!("{}", feature.id),
                    "retain SLDPRT revolution profile owner",
                )?;
                profile_source_owner.insert(id, history_index);
            }
        }
    }
    let mut profiles = HashMap::<String, Vec<Option<u32>>>::new();
    let mut inputs =
        HashMap::<String, Vec<Option<(cadmpeg_ir::features::FinitePoint3, UnitVector3)>>>::new();
    for lane in lanes {
        for history in histories.iter() {
            let mut objects = Vec::new();
            for feature in &history.features {
                ctx.charge_work(1, "scan SLDPRT revolution feature objects")?;
                if let Some(name) = feature_object_name(feature, lane) {
                    ctx.reserve_vec(&mut objects, 1, "collect SLDPRT revolution feature objects")?;
                    objects.push((name.offset, feature));
                }
            }
            ctx.sort_unstable_by(
                &mut objects,
                |value| &value.0,
                Ord::cmp,
                "sort SLDPRT revolution feature objects",
            )?;
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
                    .and_then(|feature| feature_object_name(feature, lane)?.object_id?.value());
                let Some(known_profiles) = profile_source_owner
                    .get(feature.id.as_str())
                    .and_then(|owner| profile_sources.get(*owner))
                else {
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
                    ctx,
                    &lane.native_payload,
                    start,
                    end,
                    known_profiles,
                )?;
                let placed_axis = line_reference
                    .map(|(_, origin, direction)| (origin, direction))
                    .or_else(|| temporary_axis_reference(&lane.native_payload, start, end));
                push_revolution_vote(
                    ctx,
                    &mut profiles,
                    &feature.id,
                    immediate_profile.or_else(|| line_reference.map(|input| input.0)),
                    "collect SLDPRT revolution profile votes",
                )?;
                push_revolution_vote(
                    ctx,
                    &mut inputs,
                    &feature.id,
                    placed_axis,
                    "collect SLDPRT revolution axis votes",
                )?;
            }
        }
    }
    for history in histories.iter_mut() {
        for feature in &mut history.features {
            ctx.charge_work(1, "resolve SLDPRT revolution votes")?;
            if !feature.properties.contains_key("Profile") {
                if let Some(votes) = profiles.get(&feature.id) {
                    if let Some(Some(first)) = votes.first() {
                        if votes.iter().all(|vote| vote == &Some(*first))
                            && profile_source_owner
                                .get(feature.id.as_str())
                                .and_then(|owner| profile_sources.get(*owner))
                                .is_some_and(|sources| sources.contains(first))
                        {
                            let value = ctx.format_retained(
                                format_args!("{first}"),
                                "retain SLDPRT revolution profile property",
                            )?;
                            ctx.insert_btree_map(
                                &mut feature.properties,
                                cadmpeg_core::nonblank_literal!("Profile"),
                                value,
                                "insert SLDPRT revolution profile property",
                            )?;
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
                let origin = ctx.format_retained(
                    format_args!(
                        "{}mm,{}mm,{}mm",
                        first.0.get().x,
                        first.0.get().y,
                        first.0.get().z
                    ),
                    "retain SLDPRT revolution axis origin",
                )?;
                let direction = ctx.format_retained(
                    format_args!(
                        "{},{},{}",
                        first.1.as_raw().x,
                        first.1.as_raw().y,
                        first.1.as_raw().z
                    ),
                    "retain SLDPRT revolution axis direction",
                )?;
                ctx.insert_btree_map(
                    &mut feature.properties,
                    cadmpeg_core::nonblank_literal!("AxisOrigin"),
                    origin,
                    "insert SLDPRT revolution axis properties",
                )?;
                ctx.insert_btree_map(
                    &mut feature.properties,
                    cadmpeg_core::nonblank_literal!("AxisDirection"),
                    direction,
                    "insert SLDPRT revolution axis properties",
                )?;
            }
        }
    }
    Ok(())
}

/// Bind revolution axes from profile records or complete coaxial generated geometry.
pub(crate) fn bind_profile_revolution_axes(
    ctx: &DecodeContext<'_>,
    model_features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    sketches: &[Sketch],
    surfaces: &[Surface],
) -> Result<(), CodecError> {
    let mut native_by_id = HashMap::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, "index SLDPRT native revolution features")?;
        ctx.insert_hash_map(
            &mut native_by_id,
            feature.id.as_str(),
            feature,
            "index SLDPRT native revolution features",
        )?;
    }
    let mut model_by_id = HashMap::new();
    for (index, feature) in model_features.iter().enumerate() {
        ctx.charge_work(1, "index SLDPRT model revolution features")?;
        ctx.insert_hash_map(
            &mut model_by_id,
            &feature.id,
            index,
            "index SLDPRT model revolution features",
        )?;
    }
    let mut sketch_by_id = HashMap::new();
    for sketch in sketches {
        ctx.charge_work(1, "index SLDPRT revolution sketches")?;
        ctx.insert_hash_map(
            &mut sketch_by_id,
            &sketch.id,
            sketch,
            "index SLDPRT revolution sketches",
        )?;
    }
    let mut assignments = Vec::<(usize, cadmpeg_ir::features::RevolutionAxis)>::new();

    for (feature_index, feature) in model_features.iter().enumerate() {
        let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        if construction.axis().is_some() {
            continue;
        }
        let Some(profile) = construction.profile() else {
            continue;
        };
        let (profile_native, sketch_id) = match profile {
            PlanarProfileRef::Feature(profile_id) => {
                let Some(&profile_index) = model_by_id.get(profile_id) else {
                    continue;
                };
                let profile_feature = &model_features[profile_index];
                let FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                }) = profile_feature.evaluation.definition()
                else {
                    continue;
                };
                let Some(native) = profile_feature.native_ref.as_deref() else {
                    continue;
                };
                (native, sketch)
            }
            PlanarProfileRef::Sketch(sketch_id) => {
                let mut owners = model_features.iter().filter(|candidate| {
                    matches!(
                    candidate.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(candidate)),
                    }) if candidate == sketch_id
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
            PlanarProfileRef::Generated { .. }
            | PlanarProfileRef::SketchProfiles { .. }
            | PlanarProfileRef::SketchRegions { .. }
            | PlanarProfileRef::SketchEntities { .. }
            | PlanarProfileRef::SketchSelection { .. }
            | PlanarProfileRef::HistoricalFaces { .. }
            | PlanarProfileRef::Unresolved(_)
            | PlanarProfileRef::Native(_)
            | PlanarProfileRef::Faces(_) => continue,
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
            construction.extent(),
            Some(cadmpeg_ir::features::RevolveExtent::OneSided {
                termination: cadmpeg_ir::features::AngularTermination::Angle { angle },
            }) if (angle.get().abs() - std::f64::consts::TAU).abs() <= EPS_AXES_BIND_PROFILE_REVOLUTION_AXES_E9
        ) {
            surfaces
        } else {
            &[]
        };
        let mut candidates = Vec::new();
        for lane in lanes {
            ctx.charge_work(1, "scan SLDPRT revolution axis lanes")?;
            if let Some(axis) = profile_roster_construction_axis(
                ctx,
                lane,
                profile_native,
                sketch,
                generated_axis_surfaces,
            )? {
                ctx.reserve_vec(
                    &mut candidates,
                    1,
                    "collect SLDPRT revolution axis candidates",
                )?;
                candidates.push(axis);
            }
        }
        ctx.stable_sort_by_key(
            &mut candidates,
            |value| {
                [
                    value.origin.x.to_bits(),
                    value.origin.y.to_bits(),
                    value.origin.z.to_bits(),
                    value.direction.x.to_bits(),
                    value.direction.y.to_bits(),
                    value.direction.z.to_bits(),
                ]
            },
            Ord::cmp,
            "sort SLDPRT revolution axis candidates",
        )?;
        candidates.dedup();
        if let [axis] = candidates.as_slice() {
            ctx.reserve_vec(
                &mut assignments,
                1,
                "collect SLDPRT revolution axis assignments",
            )?;
            assignments.push((
                feature_index,
                axis.try_clone_for_decode(ctx, "SLDPRT revolution axis assignment copy")?,
            ));
        }
    }

    for (index, axis) in assignments {
        model_features[index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Revolve {
                construction, ..
            }) = definition
            {
                if construction.axis().is_none() {
                    construction.set_axis(Some(axis));
                }
            }
        });
    }
    Ok(())
}

fn profile_roster_construction_axis(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    sketch: &Sketch,
    surfaces: &[Surface],
) -> Result<Option<cadmpeg_ir::features::RevolutionAxis>, CodecError> {
    const QUANTUM: f64 = 1e-8;
    const NATIVE_TO_IR: f64 = 1000.0;
    let Some((origin, normal, u_axis)) = sketch.resolved_placement() else {
        return Ok(None);
    };

    let mut markers = Vec::new();
    for marker in &lane.sketch_entities {
        ctx.reserve_vec(&mut markers, 1, "collect SLDPRT revolution profile markers")?;
        markers.push(marker);
    }
    let mut first_axis = None;
    let mut second_axis = None;
    for marker in lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
    {
        let Some(offset) = index_from_u64(marker.offset()) else {
            continue;
        };
        if !marker_is_selected_construction_line(&lane.native_payload, offset) {
            continue;
        }
        let endpoints = roster_curve_endpoint_markers(ctx, &lane.native_payload, marker, &markers)?;
        let [start, end] = endpoints.as_slice() else {
            continue;
        };
        let endpoints = [*start, *end];
        if first_axis.is_none() {
            first_axis = Some(endpoints);
        } else {
            second_axis = Some(endpoints);
            break;
        }
    }
    let native_endpoints = match (first_axis, second_axis) {
        (Some(endpoints), None) => {
            let (Some(start), Some(end)) = (endpoints[0].coordinates_m, endpoints[1].coordinates_m)
            else {
                return Ok(None);
            };
            Some([start.get(), end.get()])
        }
        (None, None) => {
            if let Some(endpoints) =
                profile_roster_implicit_axis_endpoints(ctx, lane, profile_native, &markers)?
            {
                let (Some(start), Some(end)) =
                    (endpoints[0].coordinates_m, endpoints[1].coordinates_m)
                else {
                    return Ok(None);
                };
                Some([start.get(), end.get()])
            } else {
                match profile_roster_origin_axis_endpoints(ctx, lane, profile_native, &markers)? {
                    Some(endpoints) => Some(endpoints),
                    None => profile_roster_principal_axis_endpoints(
                        ctx,
                        lane,
                        profile_native,
                        &markers,
                    )?,
                }
            }
        }
        _ => return Ok(None),
    };
    let Some(transform) = sketch_frame_marker_transform(sketch, QUANTUM) else {
        return Ok(None);
    };
    let Some([native_start, native_end]) = native_endpoints else {
        return profile_generated_surface_axis(
            ctx,
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
            f64_from_i64(point.0)? * QUANTUM,
            f64_from_i64(point.1)? * QUANTUM,
        ))
    };
    let (Some(start), Some(end)) = (project(native_start), project(native_end)) else {
        return Ok(None);
    };
    let v_axis = normal.cross(u_axis.get());
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
    if !length.is_finite() || length <= EPS_AXES_PROFILE_ROSTER_CONSTRUCTION_AXIS_E9 {
        return Ok(None);
    }
    let (Some(origin), Some(direction)) = (
        cadmpeg_ir::features::FinitePoint3::new(start),
        UnitVector3::normalized_by_square_sum_division(delta),
    ) else {
        return Ok(None);
    };
    Ok(Some(cadmpeg_ir::features::RevolutionAxis {
        origin,
        direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
        reference: None,
    }))
}

fn profile_generated_surface_axis(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&SketchInputEntity],
    sketch: &Sketch,
    transform: &MarkerTransform,
    surfaces: &[Surface],
) -> Result<Option<cadmpeg_ir::features::RevolutionAxis>, CodecError> {
    const QUANTUM: f64 = 1e-8;
    const NATIVE_TO_IR: f64 = 1000.0;
    const LINE_TOLERANCE: f64 = 1e-6;
    let Some((origin, normal, u_axis)) = sketch.resolved_placement() else {
        return Ok(None);
    };

    let Some(mut axis) = common_generated_surface_axis(surfaces) else {
        return Ok(None);
    };
    let relative_origin = Vector3::new(
        axis.origin.x - origin.x,
        axis.origin.y - origin.y,
        axis.origin.z - origin.z,
    );
    if axis.direction.dot(normal.get()).abs() > EPS_AXES_PROFILE_GENERATED_SURFACE_AXIS_E9
        || relative_origin.dot(normal.get()).abs() > LINE_TOLERANCE
    {
        return Ok(None);
    }
    let origin_offset = Vector3::new(
        origin.x - axis.origin.x,
        origin.y - axis.origin.y,
        origin.z - axis.origin.z,
    );
    let perpendicular = origin_offset.cross(axis.direction.get());
    if perpendicular.norm() <= LINE_TOLERANCE {
        axis.origin = origin;
    } else {
        let projection = origin_offset.dot(axis.direction.get());
        let Some(projected_origin) = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
            axis.origin.x + projection * axis.direction.x,
            axis.origin.y + projection * axis.direction.y,
            axis.origin.z + projection * axis.direction.z,
        )) else {
            return Ok(None);
        };
        axis.origin = projected_origin;
    }
    let mut endpoint_ids = HashSet::new();
    let v_axis = normal.cross(u_axis.get());
    let mut observed_one = false;
    let mut observed_two = false;
    let mut off_axis = false;
    let mut positive = false;
    let mut negative = false;
    for curve in markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
    {
        let curve_endpoints =
            roster_curve_endpoint_markers(ctx, &lane.native_payload, curve, markers)?;
        for endpoint in curve_endpoints
            .into_iter()
            .filter(|endpoint| endpoint.object_index().is_some())
        {
            ctx.charge_work(1, "scan SLDPRT generated revolution axis endpoints")?;
            if endpoint_ids.contains(endpoint.id()) {
                continue;
            }
            ctx.insert_hash_set(
                &mut endpoint_ids,
                endpoint.id(),
                "index SLDPRT generated revolution axis endpoints",
            )?;
            let Some(coordinates) = endpoint.coordinates_m else {
                return Ok(None);
            };
            let [u, v] = coordinates.get();
            let Some(point) = transform.apply(quantize(
                Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR),
                QUANTUM,
            )) else {
                return Ok(None);
            };
            let (Some(u_cells), Some(v_cells)) = (f64_from_i64(point.0), f64_from_i64(point.1))
            else {
                return Ok(None);
            };
            let point = Point3::new(
                origin.x + u_cells * QUANTUM * u_axis.x + v_cells * QUANTUM * v_axis.x,
                origin.y + u_cells * QUANTUM * u_axis.y + v_cells * QUANTUM * v_axis.y,
                origin.z + u_cells * QUANTUM * u_axis.z + v_cells * QUANTUM * v_axis.z,
            );
            let relative = Vector3::new(
                point.x - axis.origin.x,
                point.y - axis.origin.y,
                point.z - axis.origin.z,
            );
            let side = axis.direction.cross(relative).dot(normal.get());
            observed_two |= observed_one;
            observed_one = true;
            off_axis |= side.abs() > LINE_TOLERANCE;
            positive |= side > LINE_TOLERANCE;
            negative |= side < -LINE_TOLERANCE;
        }
    }
    if !observed_two || !off_axis || (positive && negative) {
        return Ok(None);
    }
    Ok(Some(axis))
}

fn common_generated_surface_axis(
    surfaces: &[Surface],
) -> Option<cadmpeg_ir::features::RevolutionAxis> {
    const DIRECTION_TOLERANCE: f64 = 1e-9;
    const LINE_TOLERANCE: f64 = 1e-6;

    let mut axes = surfaces
        .iter()
        .filter_map(|surface| match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => Some((
                cylinder_surface.origin().get(),
                *cylinder_surface.frame().axis(),
            )),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                Some((cone_surface.origin().get(), *cone_surface.frame().axis()))
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                Some((torus_surface.center().get(), *torus_surface.frame().axis()))
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => None,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => None,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => None,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(_)) => None,
            SurfaceGeometry::Procedural { .. } => None,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(_)) => None,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => None,
        });
    let (origin, mut direction) = axes.next()?;
    let second = axes.next()?;
    for (candidate_origin, candidate_direction) in std::iter::once(second).chain(axes) {
        let origin_delta = Vector3::new(
            candidate_origin.x - origin.x,
            candidate_origin.y - origin.y,
            candidate_origin.z - origin.z,
        );
        let direction_cross = direction.as_raw().cross(*candidate_direction.as_raw());
        let line_offset = origin_delta.cross(*direction.as_raw());
        if direction_cross.norm() > DIRECTION_TOLERANCE || line_offset.norm() > LINE_TOLERANCE {
            return None;
        }
    }
    let raw = *direction.as_raw();
    if raw.x < -DIRECTION_TOLERANCE
        || (raw.x.abs() <= DIRECTION_TOLERANCE && raw.y < -DIRECTION_TOLERANCE)
        || (raw.x.abs() <= DIRECTION_TOLERANCE && raw.y.abs() <= DIRECTION_TOLERANCE && raw.z < 0.0)
    {
        direction = direction.reversed();
    }
    let direction_raw = *direction.as_raw();
    let origin_projection = Vector3::new(origin.x, origin.y, origin.z).dot(direction_raw);
    let origin = Point3::new(
        origin.x - origin_projection * direction_raw.x,
        origin.y - origin_projection * direction_raw.y,
        origin.z - origin_projection * direction_raw.z,
    );
    Some(cadmpeg_ir::features::RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::new(origin)?,
        direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
        reference: None,
    })
}

fn profile_curve_endpoint_ids<'a>(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&'a SketchInputEntity],
    indexed_only: bool,
) -> Result<HashSet<&'a str>, CodecError> {
    let mut ids = HashSet::new();
    for curve in markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
    {
        ctx.charge_work(1, "scan SLDPRT profile curve endpoints")?;
        for endpoint in roster_curve_endpoint_markers(ctx, &lane.native_payload, curve, markers)? {
            if (indexed_only && endpoint.object_index().is_none()) || ids.contains(endpoint.id()) {
                continue;
            }
            ctx.insert_hash_set(
                &mut ids,
                endpoint.id(),
                "index SLDPRT profile curve endpoints",
            )?;
        }
    }
    Ok(ids)
}

fn profile_roster_origin_axis_endpoints(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&SketchInputEntity],
) -> Result<Option<[[f64; 2]; 2]>, CodecError> {
    let curve_endpoints = profile_curve_endpoint_ids(ctx, lane, profile_native, markers, true)?;
    let mut unreferenced_points = markers.iter().copied().filter(|marker| {
        marker.feature_ref.as_deref() == Some(profile_native)
            && matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
            && marker.coordinates_m.is_some()
            && !curve_endpoints.contains(marker.id())
    });
    let (Some(origin), None) = (unreferenced_points.next(), unreferenced_points.next()) else {
        return Ok(None);
    };
    let Some(coordinates) = origin.coordinates_m else {
        return Ok(None);
    };
    let [origin_u, origin_v] = coordinates.get();
    if origin_u.abs() > EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
        || origin_v.abs() > EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
    {
        return Ok(None);
    }
    let candidates = markers
        .iter()
        .copied()
        .filter(|marker| marker.object_index().is_some() && curve_endpoints.contains(marker.id()))
        .filter_map(|marker| {
            let end = marker.coordinates_m?.get();
            let endpoints = [[origin_u, origin_v], end];
            bounded_profile_axis_coordinates(profile_native, markers, &curve_endpoints, endpoints)
                .then_some(endpoints)
        });
    let mut candidates_sorted = Vec::new();
    for candidate in candidates {
        ctx.reserve_vec(
            &mut candidates_sorted,
            1,
            "collect SLDPRT origin axis candidates",
        )?;
        candidates_sorted.push(candidate);
    }
    ctx.stable_sort_by_key(
        &mut candidates_sorted,
        |value| (value[1][0], value[1][1]),
        |left, right| left.0.total_cmp(&right.0).then(left.1.total_cmp(&right.1)),
        "sort SLDPRT origin axis candidates",
    )?;
    let mut lines = Vec::<[[f64; 2]; 2]>::new();
    for candidate in candidates_sorted {
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
        ctx.reserve_vec(&mut lines, 1, "collect SLDPRT distinct origin axis lines")?;
        lines.push(candidate);
    }
    let incidence = |line: &[[f64; 2]; 2]| {
        let [line_u, line_v] = [line[1][0] - origin_u, line[1][1] - origin_v];
        markers
            .iter()
            .filter(|marker| {
                marker.object_index().is_some() && curve_endpoints.contains(marker.id())
            })
            .filter_map(|marker| {
                marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
            })
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
    let Some(maximum_incidence) = lines.iter().map(incidence).max() else {
        return Ok(None);
    };
    let mut selected = lines
        .iter()
        .filter(|line| incidence(line) == maximum_incidence);
    let (Some(axis), None) = (selected.next(), selected.next()) else {
        return Ok(None);
    };
    Ok(Some(*axis))
}

fn profile_roster_principal_axis_endpoints(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&SketchInputEntity],
) -> Result<Option<[[f64; 2]; 2]>, CodecError> {
    let curve_endpoints = profile_curve_endpoint_ids(ctx, lane, profile_native, markers, true)?;
    let incidence = |axis: &[[f64; 2]; 2]| {
        let [axis_u, axis_v] = axis[1];
        markers
            .iter()
            .filter(|marker| curve_endpoints.contains(marker.id()))
            .filter_map(|marker| {
                marker
                    .coordinates_m
                    .map(cadmpeg_ir::units::FiniteVector::get)
            })
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
    let Some(maximum_incidence) = candidates.iter().map(|(count, _)| *count).max() else {
        return Ok(None);
    };
    if maximum_incidence < 2 {
        return Ok(None);
    }
    let mut selected_storage = ctx.reserve_scoped(0, "hold SLDPRT principal axis candidates")?;
    let selected = selected_storage.with_storage(|| ctx.collect_vec(
        candidates.iter().filter(|(count, _)| *count == maximum_incidence),
        "collect SLDPRT principal axis candidates",
    ))?;
    let [(_, axis)] = selected.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*axis))
}

fn profile_roster_implicit_axis_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    markers: &[&'a SketchInputEntity],
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    let curve_candidates = markers.iter().copied().filter(|marker| {
        let Ok(offset) = usize::try_from(marker.offset()) else {
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
    let mut curve_storage = ctx.reserve_scoped(0, "hold SLDPRT implicit axis curves")?;
    let curve_candidates = curve_storage.with_storage(|| ctx.collect_vec(
        curve_candidates.take(2), "collect SLDPRT implicit axis curves",
    ))?;
    let curve_endpoints = profile_curve_endpoint_ids(ctx, lane, profile_native, markers, false)?;
    let candidates = markers.iter().copied().filter(|marker| {
        marker.feature_ref.as_deref() == Some(profile_native)
            && matches!(
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
            && marker.coordinates_m.is_some()
            && !curve_endpoints.contains(marker.id())
    });
    let mut unreferenced_points = Vec::new();
    for marker in candidates {
        ctx.reserve_vec(
            &mut unreferenced_points,
            1,
            "collect SLDPRT unreferenced profile points",
        )?;
        unreferenced_points.push(marker);
    }
    if let [start, end] = unreferenced_points.as_slice() {
        let endpoints = [*start, *end];
        if bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints) {
            return Ok(Some(endpoints));
        }
    }
    let mut endpoint_storage = ctx.reserve_scoped(0, "hold SLDPRT implicit axis endpoints")?;
    let selected_endpoints = endpoint_storage.with_storage(|| ctx.collect_vec(
        unreferenced_points.iter().copied().filter(|marker| {
            index_from_u64(marker.offset()).is_some_and(|offset| {
                lane.native_payload.get(offset + 76..offset + 80) == Some(&1u32.to_le_bytes())
            })
        }).take(3), "collect SLDPRT implicit axis endpoints",
    ))?;
    if let [start, end] = selected_endpoints.as_slice() {
        let endpoints = [*start, *end];
        if bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints) {
            return Ok(Some(endpoints));
        }
    }
    if let [end] = selected_endpoints.as_slice() {
        let owned_markers = markers
            .iter()
            .copied()
            .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native));
        let mut owned = Vec::new();
        for marker in owned_markers {
            ctx.reserve_vec(&mut owned, 1, "collect SLDPRT owned profile markers")?;
            owned.push(marker);
        }
        ctx.sort_unstable_by_key(
            &mut owned,
            |value| value.offset(),
            Ord::cmp,
            "sldprt profile axis owned markers sort",
        )?;
        if let Some(start) = owned
            .windows(2)
            .find_map(|pair| (pair[1].id() == end.id()).then_some(pair[0]))
            .filter(|marker| {
                marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            })
        {
            let endpoints = [start, *end];
            if bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints)
            {
                return Ok(Some(endpoints));
            }
        }
    }
    let mut boundary_relations = Vec::new();
    for candidate in markers
        .iter()
        .copied()
        .filter(|marker| marker.feature_ref.as_deref() == Some(profile_native))
    {
        if !index_from_u64(candidate.offset()).is_some_and(|offset| {
            extended_wide_horizontal_relation_endpoint_indices(&lane.native_payload, offset)
                .is_some()
        }) {
            continue;
        }
        let endpoints =
            roster_curve_endpoint_markers(ctx, &lane.native_payload, candidate, markers)?;
        let [start, end] = endpoints.as_slice() else {
            continue;
        };
        let endpoints = [*start, *end];
        if !bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints) {
            continue;
        }
        ctx.reserve_vec(
            &mut boundary_relations,
            1,
            "collect SLDPRT profile boundary relations",
        )?;
        boundary_relations.push(endpoints);
    }
    ctx.sort_unstable_by_key(
        &mut boundary_relations,
        |value| [value[0].offset(), value[1].offset()],
        Ord::cmp,
        "sldprt profile axis boundary relations sort",
    )?;
    boundary_relations.dedup_by_key(|endpoints| [endpoints[0].id(), endpoints[1].id()]);
    match boundary_relations.as_slice() {
        [endpoints] => return Ok(Some(*endpoints)),
        [] => {}
        _ => return Ok(None),
    }
    let [candidate] = curve_candidates.as_slice() else {
        return Ok(None);
    };
    let endpoints = roster_curve_endpoint_markers(ctx, &lane.native_payload, candidate, markers)?;
    let [start, end] = endpoints.as_slice() else {
        return Ok(None);
    };
    let endpoints = [*start, *end];
    Ok(
        bounded_profile_axis_endpoints(profile_native, markers, &curve_endpoints, endpoints)
            .then_some(endpoints),
    )
}

fn bounded_profile_axis_endpoints(
    profile_native: &str,
    markers: &[&SketchInputEntity],
    curve_endpoints: &HashSet<&str>,
    endpoints: [&SketchInputEntity; 2],
) -> bool {
    let [Some(start), Some(end)] = endpoints.map(|endpoint| {
        endpoint
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
    }) else {
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
                marker.kind(),
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
            && marker.object_index().is_some()
            && curve_endpoints.contains(marker.id()))
        .then_some(
            marker
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get),
        )
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
