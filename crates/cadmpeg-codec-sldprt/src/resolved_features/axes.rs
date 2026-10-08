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
use super::scalars::lane_object_names;
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
use std::collections::{BTreeSet, HashMap, HashSet};

const EPS_AXES_BIND_PROFILE_REVOLUTION_AXES_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_CONSTRUCTION_AXIS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_GENERATED_SURFACE_AXIS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9: f64 = 1e-9;
const EPS_AXES_PROFILE_ROSTER_PRINCIPAL_AXIS_ENDPOINTS_E9: f64 = 1e-9;

fn square_sum_unit_direction(values: [f64; 3]) -> Option<UnitVector3> {
    let [x, y, z] = values;
    SumSquaresUnitVector3::new(Vector3::new(x, y, z)).map(SumSquaresUnitVector3::normalized)
}

/// The direction a declared line reference stores at one of its two fixed
/// layouts, when exactly one direction is present.
pub(super) fn line_reference_direction(payload: &[u8], class_offset: u64) -> Option<UnitVector3> {
    let class_offset = usize::try_from(class_offset).ok()?;
    let direction_at = |offset: usize| {
        square_sum_unit_direction([
            View::f64_le_at(payload, offset)?,
            View::f64_le_at(payload, offset + 8)?,
            View::f64_le_at(payload, offset + 16)?,
        ])
    };
    let short = (payload.get(class_offset + 136..class_offset + 144)
        == Some(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff])
        && payload.get(class_offset + 148..class_offset + 152) == Some(&[0xf8, 0x2a, 0, 0]))
    .then(|| direction_at(class_offset + 200))
    .flatten();
    let long = (payload.get(class_offset + 144..class_offset + 156)
        == Some(&[
            0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff,
        ])
        && payload.get(class_offset + 160..class_offset + 164) == Some(&[0xf8, 0x2a, 0, 0]))
    .then(|| direction_at(class_offset + 220))
    .flatten();
    // Both declared layouts are evaluated before selecting the direction so
    // a future overlapping layout cannot win merely by branch order.
    match (short, long) {
        (Some(short), Some(long)) => (short == long).then_some(short),
        (direction, None) | (None, direction) => direction,
    }
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
    if let Some(direction) = line_reference_direction(&payload[..end], u64_from_index(class_offset))
    {
        ctx.push_vec(
            &mut directions,
            direction,
            "collect SLDPRT declared line directions",
        )?;
    }
    let Some(final_handle) = end
        .checked_sub(88)
        .filter(|final_handle| *final_handle >= class_offset)
    else {
        return Ok(directions);
    };
    for handle in ctx.admit_iter(
        &(class_offset..=final_handle),
        "scan SLDPRT declared line directions",
    )? {
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
            push_distinct_direction(
                ctx,
                &mut directions,
                candidate,
                "collect SLDPRT declared line directions",
            )?;
        }
    }
    Ok(directions)
}

/// Appends a direction unless an equal one is already present.
fn push_distinct_direction(
    ctx: &DecodeContext<'_>,
    directions: &mut Vec<UnitVector3>,
    candidate: UnitVector3,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !ctx.any_by(
        &*directions,
        |direction| Ok(*direction == candidate),
        operation,
    )? {
        ctx.push_vec(directions, candidate, operation)?;
    }
    Ok(())
}

pub(super) fn linear_pattern_display_directions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    object_start: usize,
    object_end: usize,
    names: &[FeatureInputName],
    expected_spacing_m: [Option<f64>; 2],
) -> Result<Vec<UnitVector3>, CodecError> {
    const VALUE_OFFSET: usize = 32;
    const DIRECTION_OFFSET: usize = 161;
    const LENGTH_TOLERANCE_M: f64 = 1e-8;
    const OPERATION: &str = "find SLDPRT linear pattern display directions";

    let mut directions = Vec::new();
    let Some(end) = super::DeclaredEnd::of(object_end, payload.len()).map(super::DeclaredEnd::get)
    else {
        return Ok(directions);
    };
    for (dimension_name, expected) in ["D3", "D4"].into_iter().zip(expected_spacing_m) {
        let Some(expected) = expected else {
            continue;
        };
        let record = |name: &&FeatureInputName| {
            Ok(name.object_id == Some(ObjectId::Absent)
                && name.value == dimension_name
                && (u64_from_index(object_start)..u64_from_index(end)).contains(&name.offset))
        };
        let mut remaining = names.iter();
        let Some(name) = ctx.find_by(&mut remaining, record, OPERATION)? else {
            continue;
        };
        if ctx.any_by(&mut remaining, |name| record(&name), OPERATION)? {
            continue;
        }
        let direction = (|| {
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
        })();
        if let Some(direction) = direction {
            ctx.push_vec(&mut directions, direction, OPERATION)?;
        }
    }
    Ok(directions)
}

pub(super) fn typed_linear_pattern_dimensions(
    ctx: &DecodeContext<'_>,
    feature: &crate::records::Feature,
    lane: &FeatureInputLane,
    object_start: usize,
    object_end: usize,
) -> Result<Option<(PositiveLength, u32)>, CodecError> {
    const OPERATION: &str = "look up SLDPRT linear pattern dimensions";
    let parameter = |class_name: &str| -> Result<Option<&String>, CodecError> {
        let declared = |class: &&crate::records::FeatureInputClass| {
            Ok(
                (u64_from_index(object_start)..u64_from_index(object_end)).contains(&class.offset)
                    && ctx.equal(class.name.as_str(), class_name, OPERATION)?,
            )
        };
        let mut classes = lane.classes.iter();
        let Some(class) = ctx.find_by(&mut classes, declared, OPERATION)? else {
            return Ok(None);
        };
        if ctx.any_by(&mut classes, |class| declared(&class), OPERATION)? {
            return Ok(None);
        }
        let Some(class_offset) = usize::try_from(class.offset).ok() else {
            return Ok(None);
        };
        let Some(name_end) = class_offset.checked_add(128).map(|end| end.min(object_end)) else {
            return Ok(None);
        };
        let named = |name: &&FeatureInputName| {
            Ok(name.object_id == Some(ObjectId::Absent)
                && (u64_from_index(class_offset)..u64_from_index(name_end)).contains(&name.offset)
                && ctx.contains_key_btree_map(
                    &feature.parameters,
                    name.value.as_str(),
                    OPERATION,
                )?)
        };
        let mut names = lane.names.iter();
        let Some(name) = ctx.find_by(&mut names, named, OPERATION)? else {
            return Ok(None);
        };
        if ctx.any_by(&mut names, |name| named(&name), OPERATION)? {
            return Ok(None);
        }
        ctx.get_btree_map(&feature.parameters, name.value.as_str(), OPERATION)
    };
    let count = match parameter("moNumberDim_c")? {
        Some(value) => ctx
            .parse_text::<u32>(
                ctx.trim_text(value, "trim SLDPRT linear pattern count")?,
                "parse SLDPRT linear pattern count",
            )?
            .ok()
            .filter(|count| *count > 0),
        None => None,
    };
    let Some(count) = count else {
        return Ok(None);
    };
    let Some(spacing) = parameter("ParallelPlaneDistanceDim_c")? else {
        return Ok(None);
    };
    crate::history::literals::admit_literal(ctx, spacing, OPERATION)?;
    Ok(
        crate::history::literals::parse_positive_dimension_length_mm(spacing)
            .map(|spacing| (spacing, count)),
    )
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
    let mut excluded_storage = ctx.reserve_scoped(0, "hold SLDPRT excluded line handles")?;
    let mut excluded = excluded_storage.with_storage(|| {
        ctx.collect_vec(
            excluded_handles.iter().copied(),
            "hold SLDPRT excluded line handles",
        )
    })?;
    ctx.sort_unstable_by(
        &mut excluded,
        |handle| handle,
        Ord::cmp,
        "hold SLDPRT excluded line handles",
    )?;
    // Handles are visited in ascending order, so the excluded handles are
    // consumed in one merged pass.
    let mut next_excluded = 0;
    let mut unique_directions = Vec::new();
    for handle in ctx.admit_iter(
        &(object_start..=final_handle),
        "scan SLDPRT compact line directions",
    )? {
        let mut is_excluded = false;
        while let Some(candidate) = excluded.get(next_excluded) {
            if *candidate > handle {
                break;
            }
            ctx.charge_work(1, "hold SLDPRT excluded line handles")?;
            is_excluded |= *candidate == handle;
            next_excluded += 1;
        }
        if is_excluded {
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
                if let Some(direction) = direction_at(64) {
                    direction_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut directions,
                            direction,
                            "hold SLDPRT compact line directions",
                        )
                    })?;
                }
            }
            let terminated =
                record.get(80..84) == Some(&[0; 4]) && (tagged_token(84) || record.len() == 84);
            if record.get(16..24) == Some(&[0; 8]) && terminated {
                if let Some(direction) = direction_at(56) {
                    direction_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut directions,
                            direction,
                            "hold SLDPRT compact line directions",
                        )
                    })?;
                }
            }
            directions.dedup();
            if let [candidate] = directions.as_slice() {
                push_distinct_direction(
                    ctx,
                    &mut unique_directions,
                    *candidate,
                    "collect SLDPRT compact line directions",
                )?;
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
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut directions,
                        direction,
                        "collect SLDPRT compact line directions",
                    )
                })?;
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
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut directions,
                        direction,
                        "collect SLDPRT compact line directions",
                    )
                })?;
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
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut directions,
                        direction,
                        "collect SLDPRT compact line directions",
                    )
                })?;
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
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut directions,
                        direction,
                        "collect SLDPRT compact line directions",
                    )
                })?;
            }
        }
        if directions.is_empty()
            && record.get(16..32) == Some(&[0; 16])
            && record.get(88..104) == Some(&[0; 16])
            && record.get(104..112) == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
            && record.get(112..136) == Some(&[0; 24])
        {
            if let Some(direction) = direction_at(64) {
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut directions,
                        direction,
                        "collect SLDPRT compact line directions",
                    )
                })?;
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
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut directions,
                        direction,
                        "collect SLDPRT compact line directions",
                    )
                })?;
            }
        }
        // The final branch is the legacy unshifted fallback.  It is only a
        // candidate when no addressed layout matched; otherwise its
        // overlapping scalar window would manufacture a second width.
        if directions.is_empty() && record.get(16..24) == Some(&[0; 8]) {
            if record.get(80..88) == Some(&[0; 8]) {
                let distinct = match (direction_at(64), direction_at(72)) {
                    (Some(first), Some(second)) => (first == second).then_some(first),
                    (direction, None) | (None, direction) => direction,
                };
                if let Some(direction) = distinct {
                    direction_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut directions,
                            direction,
                            "collect SLDPRT compact line directions",
                        )
                    })?;
                }
            } else if let Some(direction) = direction_at(56) {
                direction_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut directions,
                        direction,
                        "hold SLDPRT compact line directions",
                    )
                })?;
            }
        }
        // A record is usable only when every matching layout agrees.  Never
        // let the order of the recognizers choose between distinct vectors.
        directions.dedup();
        if let [candidate] = directions.as_slice() {
            push_distinct_direction(
                ctx,
                &mut unique_directions,
                *candidate,
                "collect SLDPRT compact line directions",
            )?;
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
    const OPERATION: &str = "collect SLDPRT revolution line references";
    const RANKS: &str = "index SLDPRT revolution line reference ranks";
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
    let source_cell = |offset: usize| -> Result<Option<u32>, CodecError> {
        let source = (|| {
            let source = View::u32_le_at(payload, offset)?;
            let identity = View::u32_le_at(payload, offset + 4)?;
            let token = View::u16_le_at(payload, offset + 8)?;
            (identity != 0
                && is_class_token(token)
                && payload.get(offset + 12..offset + 16) == Some(&[0xff; 4]))
            .then_some(source)
        })();
        match source {
            Some(source)
                if ctx.contains_hash_set(
                    profile_sources,
                    &source,
                    "find SLDPRT revolution profile source",
                )? =>
            {
                Ok(Some(source))
            }
            _ => Ok(None),
        }
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
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut candidates = Vec::new();
    // The profile-source cells of the object in offset order, found on first
    // use; `consistent_until` is the number of leading cells that share the
    // first cell's source.
    let mut source_cells: Option<(Vec<(usize, u32)>, usize)> = None;
    for handle_start in ctx.admit_iter(
        &(object_start..scan_end),
        "scan SLDPRT revolution line references",
    )? {
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
            if let Some(source) = source_cell(source_offset)? {
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
                    storage.with_storage(|| {
                        ctx.push_vec(
                            &mut candidates,
                            (handle_start, 6, (source, origin, direction)),
                            OPERATION,
                        )
                    })?;
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
            if let Some(source) = source_cell(source_offset)? {
                let handles_end = handle_start + 12;
                let compact_frame = handles_end + 4;
                let compact_end = compact_frame + 9 * 8;
                if payload.get(handles_end..handles_end + 4) == Some(&[0; 4])
                    && payload.get(handles_end + 4..handles_end + 12) == Some(&[0; 8])
                    && next_class_after_zeros(compact_end, 24).is_some()
                {
                    if let Some(axis) = axis_record(compact_frame, 9) {
                        storage.with_storage(|| {
                            ctx.push_vec(
                                &mut candidates,
                                (handle_start, 9, (source, axis.0, axis.1)),
                                OPERATION,
                            )
                        })?;
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
                        storage.with_storage(|| {
                            ctx.push_vec(
                                &mut candidates,
                                (handle_start, 8, (source, axis.0, axis.1)),
                                OPERATION,
                            )
                        })?;
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
            if let Some(source) = source_cell(source_offset)? {
                if payload.get(frame..frame + 8) == Some(&[0; 8])
                    && next_class_after_zeros(record_end, 24).is_some()
                {
                    if let Some(axis) = axis_record(frame, 8) {
                        storage.with_storage(|| {
                            ctx.push_vec(
                                &mut candidates,
                                (handle_start, 8, (source, axis.0, axis.1)),
                                OPERATION,
                            )
                        })?;
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
                source_cell(source_offset)?,
                axis_record(handle_start + 24, 7),
            ) {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut candidates,
                        (handle_start, 7, (source, axis.0, axis.1)),
                        OPERATION,
                    )
                })?;
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
            if let Some(source) = source_cell(source_offset)? {
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
                            storage.with_storage(|| {
                                ctx.push_vec(
                                    &mut candidates,
                                    (handle_start, scalar_count, (source, axis.0, axis.1)),
                                    OPERATION,
                                )
                            })?;
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
            let Some(source_end) = handle_start.checked_sub(15) else {
                continue;
            };
            if source_cells.is_none() {
                let mut cells = Vec::new();
                for offset in ctx.admit_iter(
                    &(object_start..scan_end),
                    "scan SLDPRT revolution profile sources",
                )? {
                    if let Some(source) = source_cell(offset)? {
                        storage.with_storage(|| {
                            ctx.push_vec(
                                &mut cells,
                                (offset, source),
                                "scan SLDPRT revolution profile sources",
                            )
                        })?;
                    }
                }
                let consistent_until = match cells.first() {
                    Some(&(_, first)) => ctx
                        .position_by(
                            &cells,
                            |(_, source)| Ok(*source != first),
                            "scan SLDPRT revolution profile sources",
                        )?
                        .unwrap_or(cells.len()),
                    None => 0,
                };
                source_cells = Some((cells, consistent_until));
            }
            let Some((cells, consistent_until)) = source_cells.as_ref() else {
                continue;
            };
            let preceding = ctx.partition_point(
                cells,
                |(offset, _)| Ok(*offset < source_end),
                "scan SLDPRT revolution profile sources",
            )?;
            if preceding == 0 || preceding > *consistent_until {
                continue;
            }
            let source = cells[0].1;
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
                    if !ctx.any_by(&candidates, |known| Ok(*known == ranked), OPERATION)? {
                        storage
                            .with_storage(|| ctx.push_vec(&mut candidates, ranked, OPERATION))?;
                    }
                }
            }
        }
    }
    let mut ranks = HashMap::<usize, usize>::new();
    for (handle, rank, _) in ctx.admit_iter(&candidates, RANKS)? {
        if let Some(current) = ctx.get_mut_hash_map(&mut ranks, handle, RANKS)? {
            *current = (*current).max(*rank);
        } else {
            storage.with_storage(|| ctx.insert_hash_map(&mut ranks, *handle, *rank, RANKS))?;
        }
    }
    let mut selected = Vec::new();
    for (handle, rank, candidate) in ctx.admit_iter(candidates, RANKS)? {
        if ctx.get_hash_map(&ranks, &handle, RANKS)? == Some(&rank) {
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut selected,
                    candidate,
                    "collect SLDPRT ranked revolution references",
                )
            })?;
        }
    }
    // Only a unique reference resolves, so the selection needs no order.
    let candidates = selected;
    let Some((first, rest)) = candidates.split_first() else {
        return Ok(None);
    };
    Ok(ctx
        .all_by(rest, |candidate| Ok(candidate == first), RANKS)?
        .then_some(*first))
}

pub(super) fn temporary_axis_reference(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    object_start: usize,
    object_end: usize,
) -> Result<Option<(cadmpeg_ir::features::FinitePoint3, UnitVector3)>, CodecError> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const OPERATION: &str = "scan SLDPRT temporary axis references";

    let Some(end) = super::DeclaredEnd::of(object_end, payload.len()).map(super::DeclaredEnd::get)
    else {
        return Ok(None);
    };
    let Some(last_declaration) = end.checked_sub(temporary_axis::LEN) else {
        return Ok(None);
    };
    // Each declaration reads a fixed record; the search charges the offsets it
    // visits and stops at the first disagreeing axis.
    let candidate_at = |declaration: usize| {
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
    };
    let mut declarations = object_start..=last_declaration;
    let Some(first) = ctx.find_map(
        &mut declarations,
        |declaration| Ok(candidate_at(declaration)),
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    let disagrees = ctx.any_by(
        &mut declarations,
        |declaration| Ok(candidate_at(declaration).is_some_and(|candidate| candidate != first)),
        OPERATION,
    )?;
    Ok((!disagrees).then_some(first))
}

fn push_revolution_vote<T>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    votes: &mut HashMap<String, Vec<T>>,
    id: &str,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(values) = ctx.get_mut_hash_map(votes, id, operation)? {
        return storage.with_storage(|| ctx.push_vec(values, value, operation));
    }
    let id =
        storage.with_storage(|| ctx.copy_retained_text(id, "retain SLDPRT revolution vote ID"))?;
    storage.with_storage(|| ctx.push_hash_group(votes, id, value, operation, operation))
}

/// Add profile ownership and placed axes carried by revolution reference records.
pub(crate) fn enrich_history_revolution_inputs(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), CodecError> {
    const RESOLVE: &str = "resolve SLDPRT revolution votes";
    const SOURCES: &str = "index SLDPRT revolution profile sources";
    let mut storage = ctx.reserve_scoped(0, "SLDPRT revolution input workspace")?;
    let unique_names = super::unique_feature_names(ctx, &mut storage, histories, true)?;
    let object_names = lane_object_names(ctx, &mut storage, lanes)?;
    let mut unique = unique_names.iter();
    for history in ctx.admit_iter(&mut *histories, "find SLDPRT revolution profile source")? {
        for feature in ctx.admit_iter(
            &mut history.features,
            "find SLDPRT revolution profile source",
        )? {
            let name_is_unique = unique.next() == Some(&true);
            if !name_is_unique || feature.source_id.is_some() || !is_profile_feature_object(feature)
            {
                continue;
            }
            let mut object_storage = ctx.reserve_scoped(0, SOURCES)?;
            let mut object_ids = Vec::new();
            for names in ctx.admit_iter(&object_names, "find SLDPRT revolution profile source")? {
                if let Some(id) = names
                    .of(ctx, feature)?
                    .and_then(|name| name.object_id?.value())
                {
                    object_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut object_ids,
                            id,
                            "collect SLDPRT revolution profile sources",
                        )
                    })?;
                }
            }
            let Some((&first, rest)) = object_ids.split_first() else {
                continue;
            };
            if ctx.all_by(
                rest,
                |id| Ok(*id == first),
                "collect SLDPRT revolution profile sources",
            )? {
                feature.source_id = FeatureSource::from_value(first);
            }
        }
    }
    let mut profile_sources = Vec::new();
    let mut profile_source_owner = HashMap::<String, usize>::new();
    for (history_index, history) in ctx.admit_iter(&*histories, SOURCES)?.enumerate() {
        let mut sources = HashSet::new();
        for feature in ctx.admit_iter(&history.features, SOURCES)? {
            if !is_profile_feature_object(feature) {
                continue;
            }
            if let Some(source) = feature.source_value() {
                storage.with_storage(|| ctx.insert_hash_set(&mut sources, source, SOURCES))?;
            }
            for names in ctx.admit_iter(&object_names, SOURCES)? {
                if let Some(source) = names
                    .of(ctx, feature)?
                    .and_then(|name| name.object_id?.value())
                {
                    storage.with_storage(|| ctx.insert_hash_set(&mut sources, source, SOURCES))?;
                }
            }
        }
        storage.with_storage(|| {
            ctx.push_vec(
                &mut profile_sources,
                sources,
                "collect SLDPRT revolution profile sets",
            )
        })?;
        for feature in
            ctx.admit_iter(&history.features, "index SLDPRT revolution profile owners")?
        {
            if let Some(owner) = ctx.get_mut_hash_map(
                &mut profile_source_owner,
                feature.id.as_str(),
                "lookup SLDPRT revolution profile owner",
            )? {
                *owner = history_index;
            } else {
                let id = storage.with_storage(|| {
                    ctx.copy_retained_text(&feature.id, "retain SLDPRT revolution profile owner")
                })?;
                storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut profile_source_owner,
                        id,
                        history_index,
                        "index SLDPRT revolution profile owners",
                    )
                })?;
            }
        }
    }
    let known_profiles = |ctx: &DecodeContext<'_>, id: &str| {
        Ok::<_, CodecError>(
            ctx.get_hash_map(
                &profile_source_owner,
                id,
                "lookup SLDPRT revolution profile owner",
            )?
            .and_then(|owner| profile_sources.get(*owner)),
        )
    };
    let mut profiles = HashMap::<String, Vec<Option<u32>>>::new();
    let mut inputs =
        HashMap::<String, Vec<Option<(cadmpeg_ir::features::FinitePoint3, UnitVector3)>>>::new();
    for (lane, names) in ctx
        .admit_iter(lanes, "scan SLDPRT revolution feature objects")?
        .zip(&object_names)
    {
        for history in ctx.admit_iter(&*histories, "scan SLDPRT revolution feature objects")? {
            let mut object_storage = ctx.reserve_scoped(0, SOURCES)?;
            let mut objects = Vec::new();
            for feature in
                ctx.admit_iter(&history.features, "scan SLDPRT revolution feature objects")?
            {
                if let Some(name) = names.of(ctx, feature)? {
                    object_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut objects,
                            (name.offset, feature),
                            "collect SLDPRT revolution feature objects",
                        )
                    })?;
                }
            }
            ctx.sort_unstable_by(
                &mut objects,
                |value| &value.0,
                Ord::cmp,
                "sort SLDPRT revolution feature objects",
            )?;
            for (index, &(start, feature)) in ctx
                .admit_iter(&objects, "scan SLDPRT revolution feature objects")?
                .enumerate()
            {
                if !matches!(
                    feature.input_class.as_deref(),
                    Some("moRevolution_c" | "moRevCut_c")
                ) {
                    continue;
                }
                let immediate_profile = match index
                    .checked_sub(1)
                    .and_then(|index| objects.get(index))
                    .map(|(_, feature)| *feature)
                    .filter(|feature| is_profile_feature_object(feature))
                {
                    Some(profile) => names
                        .of(ctx, profile)?
                        .and_then(|name| name.object_id?.value()),
                    None => None,
                };
                let Some(known_profiles) = known_profiles(ctx, feature.id.as_str())? else {
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
                let placed_axis = match line_reference {
                    Some((_, origin, direction)) => Some((origin, direction)),
                    None => temporary_axis_reference(ctx, &lane.native_payload, start, end)?,
                };
                push_revolution_vote(
                    ctx,
                    &mut storage,
                    &mut profiles,
                    &feature.id,
                    immediate_profile.or_else(|| line_reference.map(|input| input.0)),
                    "collect SLDPRT revolution profile votes",
                )?;
                push_revolution_vote(
                    ctx,
                    &mut storage,
                    &mut inputs,
                    &feature.id,
                    placed_axis,
                    "collect SLDPRT revolution axis votes",
                )?;
            }
        }
    }
    for history in ctx.admit_iter(&mut *histories, RESOLVE)? {
        for feature in ctx.admit_iter(&mut history.features, RESOLVE)? {
            if !ctx.contains_key_btree_map(&feature.properties, "Profile", RESOLVE)? {
                if let Some(votes) = ctx.get_hash_map(&profiles, feature.id.as_str(), RESOLVE)? {
                    if let Some(Some(first)) = votes.first() {
                        if ctx.all_by(votes, |vote| Ok(*vote == Some(*first)), RESOLVE)?
                            && known_profiles(ctx, feature.id.as_str())?
                                .map(|sources| ctx.contains_hash_set(sources, first, RESOLVE))
                                .transpose()?
                                == Some(true)
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
            let Some(votes) = ctx.get_hash_map(&inputs, feature.id.as_str(), RESOLVE)? else {
                continue;
            };
            let Some(Some(first)) = votes.first() else {
                continue;
            };
            if !ctx.all_by(votes, |vote| Ok(vote.as_ref() == Some(first)), RESOLVE)? {
                continue;
            }
            if !ctx.contains_key_btree_map(&feature.properties, "AxisOrigin", RESOLVE)?
                && !ctx.contains_key_btree_map(&feature.properties, "AxisDirection", RESOLVE)?
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
    const INDEX: &str = "index SLDPRT revolution features";
    let mut storage = ctx.reserve_scoped(0, INDEX)?;
    let mut native_by_id = HashMap::new();
    for history in ctx.admit_iter(histories, INDEX)? {
        for feature in ctx.admit_iter(&history.features, INDEX)? {
            storage.with_storage(|| {
                ctx.insert_hash_map(&mut native_by_id, feature.id.as_str(), feature, INDEX)
            })?;
        }
    }
    let mut model_by_id = HashMap::new();
    for (index, feature) in ctx.admit_iter(&*model_features, INDEX)?.enumerate() {
        storage
            .with_storage(|| ctx.insert_hash_map(&mut model_by_id, &feature.id, index, INDEX))?;
    }
    let mut sketch_by_id = HashMap::new();
    for sketch in ctx.admit_iter(sketches, INDEX)? {
        storage
            .with_storage(|| ctx.insert_hash_map(&mut sketch_by_id, &sketch.id, sketch, INDEX))?;
    }
    // The one model feature that places each planar sketch.
    let (sketch_owners, _owner_storage) = ctx.unique_index(
        ctx.admit_iter(&*model_features, INDEX)?
            .filter_map(|feature| match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                }) => Some((sketch, feature)),
                _ => None,
            }),
        INDEX,
    )?;
    let mut lane_markers = Vec::new();
    for lane in ctx.admit_iter(lanes, INDEX)? {
        let markers = LaneMarkerIndex::new(ctx, lane.sketch_entities.iter())?;
        storage.with_storage(|| ctx.push_vec(&mut lane_markers, markers, INDEX))?;
    }
    let mut assignments = Vec::<(usize, cadmpeg_ir::features::RevolutionAxis)>::new();

    for (feature_index, feature) in ctx.admit_iter(&*model_features, INDEX)?.enumerate() {
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
                let Some(&profile_index) = ctx.get_hash_map(&model_by_id, profile_id, INDEX)?
                else {
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
                let Some(Some(owner)) = ctx.get_hash_map(&sketch_owners, sketch_id, INDEX)? else {
                    continue;
                };
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
        if !ctx
            .get_hash_map(&native_by_id, profile_native, INDEX)?
            .is_some_and(|feature| is_profile_feature_object(feature))
        {
            continue;
        }
        let Some(sketch) = ctx.get_hash_map(&sketch_by_id, sketch_id, INDEX)?.copied() else {
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
        let mut candidate_storage = ctx.reserve_scoped(0, INDEX)?;
        let mut candidates = Vec::new();
        for (lane, markers) in ctx
            .admit_iter(lanes, "scan SLDPRT revolution axis lanes")?
            .zip(&lane_markers)
        {
            if let Some(axis) = profile_roster_construction_axis_in(
                ctx,
                lane,
                markers,
                profile_native,
                sketch,
                generated_axis_surfaces,
            )? {
                candidate_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut candidates,
                        axis,
                        "collect SLDPRT revolution axis candidates",
                    )
                })?;
            }
        }
        let Some((first, rest)) = candidates.split_first() else {
            continue;
        };
        if ctx.all_by(
            rest,
            |axis| ctx.equal(axis, first, "collect SLDPRT revolution axis candidates"),
            "collect SLDPRT revolution axis candidates",
        )? {
            let axis = first.try_clone_for_decode(ctx, "SLDPRT revolution axis assignment copy")?;
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut assignments,
                    (feature_index, axis),
                    "collect SLDPRT revolution axis assignments",
                )
            })?;
        }
    }
    drop((native_by_id, model_by_id, sketch_owners));
    for (index, axis) in ctx.admit_iter(assignments, "assign SLDPRT revolution axes")? {
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

#[cfg(test)]
fn profile_roster_construction_axis(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    sketch: &Sketch,
    surfaces: &[Surface],
) -> Result<Option<cadmpeg_ir::features::RevolutionAxis>, CodecError> {
    let markers = LaneMarkerIndex::new(ctx, lane.sketch_entities.iter())?;
    profile_roster_construction_axis_in(ctx, lane, &markers, profile_native, sketch, surfaces)
}

struct LaneMarkerIndex<'marker, 'ctx> {
    all: Vec<&'marker SketchInputEntity>,
    by_feature: HashMap<&'marker str, Vec<&'marker SketchInputEntity>>,
    by_id: HashMap<&'marker str, Vec<(usize, &'marker SketchInputEntity)>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'marker, 'ctx> LaneMarkerIndex<'marker, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        markers: impl IntoIterator<Item = &'marker SketchInputEntity>,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT revolution profile markers";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut all = Vec::new();
        let mut by_feature = HashMap::new();
        let mut by_id = HashMap::new();
        let mut remaining = markers.into_iter().enumerate();
        while let Some((index, marker)) = ctx.next_charged(&mut remaining, OPERATION)? {
            storage.with_storage(|| {
                ctx.push_vec(&mut all, marker, OPERATION)?;
                ctx.push_hash_group(
                    &mut by_id,
                    marker.id(),
                    (index, marker),
                    OPERATION,
                    OPERATION,
                )?;
                if let Some(feature) = marker.feature_ref.as_deref() {
                    ctx.push_hash_group(&mut by_feature, feature, marker, OPERATION, OPERATION)?;
                }
                Ok::<_, CodecError>(())
            })?;
        }
        Ok(Self {
            all,
            by_feature,
            by_id,
            _storage: storage,
        })
    }
}

fn profile_roster_construction_axis_in(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    roster: &LaneMarkerIndex<'_, '_>,
    profile_native: &str,
    sketch: &Sketch,
    surfaces: &[Surface],
) -> Result<Option<cadmpeg_ir::features::RevolutionAxis>, CodecError> {
    const QUANTUM: f64 = 1e-8;
    const NATIVE_TO_IR: f64 = 1000.0;
    const OPERATION: &str = "find SLDPRT revolution construction axis";
    let Some((origin, normal, u_axis)) = sketch.resolved_placement() else {
        return Ok(None);
    };
    let markers = &roster.all;
    let owned = ctx
        .get_hash_map(
            &roster.by_feature,
            profile_native,
            "collect SLDPRT owned profile markers",
        )?
        .map_or(&[][..], Vec::as_slice);
    let geometry = crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
        ctx,
        markers,
        crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
            ctx,
            &lane.native_payload,
        )?,
    )?;
    let construction_axis = |marker: &&SketchInputEntity| {
        let Some(offset) = index_from_u64(marker.offset()) else {
            return Ok(None);
        };
        if !marker_is_selected_construction_line(&lane.native_payload, offset) {
            return Ok(None);
        }
        let (endpoints, _endpoint_storage) = ctx.with_scoped_storage(OPERATION, || {
            roster_curve_endpoint_markers(ctx, &lane.native_payload, marker, markers, &geometry)
        })?;
        Ok(match endpoints.as_slice() {
            [start, end] => Some([*start, *end]),
            _ => None,
        })
    };
    let mut remaining = owned.iter();
    let first_axis = ctx.find_map(&mut remaining, &construction_axis, OPERATION)?;
    let second_axis = match first_axis {
        Some(_) => ctx.find_map(&mut remaining, construction_axis, OPERATION)?,
        None => None,
    };
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
                profile_roster_implicit_axis_endpoints(ctx, lane, profile_native, roster)?
            {
                let (Some(start), Some(end)) =
                    (endpoints[0].coordinates_m, endpoints[1].coordinates_m)
                else {
                    return Ok(None);
                };
                Some([start.get(), end.get()])
            } else {
                match profile_roster_origin_axis_endpoints(ctx, lane, profile_native, roster)? {
                    Some(endpoints) => Some(endpoints),
                    None => {
                        profile_roster_principal_axis_endpoints(ctx, lane, profile_native, roster)?
                    }
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
            ctx, lane, owned, markers, sketch, &transform, surfaces,
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
    owned: &[&SketchInputEntity],
    markers: &[&SketchInputEntity],
    sketch: &Sketch,
    transform: &MarkerTransform,
    surfaces: &[Surface],
) -> Result<Option<cadmpeg_ir::features::RevolutionAxis>, CodecError> {
    const QUANTUM: f64 = 1e-8;
    const NATIVE_TO_IR: f64 = 1000.0;
    const LINE_TOLERANCE: f64 = 1e-6;
    const OPERATION: &str = "scan SLDPRT generated revolution axis endpoints";
    let Some((origin, normal, u_axis)) = sketch.resolved_placement() else {
        return Ok(None);
    };

    let Some(mut axis) = common_generated_surface_axis(ctx, surfaces)? else {
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
    let geometry = crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
        ctx,
        markers,
        crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
            ctx,
            &lane.native_payload,
        )?,
    )?;
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut endpoint_ids = HashSet::new();
    let v_axis = normal.cross(u_axis.get());
    let mut observed_one = false;
    let mut observed_two = false;
    let mut off_axis = false;
    let mut positive = false;
    let mut negative = false;
    for curve in ctx.admit_iter(owned, OPERATION)? {
        let (curve_endpoints, _endpoint_storage) = ctx.with_scoped_storage(OPERATION, || {
            roster_curve_endpoint_markers(ctx, &lane.native_payload, curve, markers, &geometry)
        })?;
        for endpoint in ctx.admit_iter(curve_endpoints, OPERATION)? {
            if endpoint.object_index().is_none()
                || !storage.with_storage(|| {
                    ctx.insert_hash_set(&mut endpoint_ids, endpoint.id(), OPERATION)
                })?
            {
                continue;
            }
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
    ctx: &DecodeContext<'_>,
    surfaces: &[Surface],
) -> Result<Option<cadmpeg_ir::features::RevolutionAxis>, CodecError> {
    const DIRECTION_TOLERANCE: f64 = 1e-9;
    const LINE_TOLERANCE: f64 = 1e-6;
    const OPERATION: &str = "find SLDPRT common generated surface axis";

    let surface_axis = |surface: &Surface| match &surface.geometry {
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
    };
    let mut remaining = surfaces.iter();
    let Some((origin, mut direction)) = ctx.find_map(
        &mut remaining,
        |surface| Ok(surface_axis(surface)),
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    let disagrees = |(candidate_origin, candidate_direction): (Point3, UnitVector3)| {
        let origin_delta = Vector3::new(
            candidate_origin.x - origin.x,
            candidate_origin.y - origin.y,
            candidate_origin.z - origin.z,
        );
        let direction_cross = direction.as_raw().cross(*candidate_direction.as_raw());
        let line_offset = origin_delta.cross(*direction.as_raw());
        direction_cross.norm() > DIRECTION_TOLERANCE || line_offset.norm() > LINE_TOLERANCE
    };
    let Some(second) = ctx.find_map(
        &mut remaining,
        |surface| Ok(surface_axis(surface)),
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    if disagrees(second)
        || ctx.any_by(
            &mut remaining,
            |surface| Ok(surface_axis(surface).is_some_and(disagrees)),
            OPERATION,
        )?
    {
        return Ok(None);
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
    Ok(
        cadmpeg_ir::features::FinitePoint3::new(origin).map(|origin| {
            cadmpeg_ir::features::RevolutionAxis {
                origin,
                direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
                reference: None,
            }
        }),
    )
}

fn profile_curve_endpoint_ids<'a>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    lane: &FeatureInputLane,
    owned: &[&SketchInputEntity],
    markers: &[&'a SketchInputEntity],
    indexed_only: bool,
) -> Result<BTreeSet<&'a str>, CodecError> {
    const OPERATION: &str = "scan SLDPRT profile curve endpoints";
    let geometry = crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
        ctx,
        markers,
        crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
            ctx,
            &lane.native_payload,
        )?,
    )?;
    let mut ids = BTreeSet::new();
    for curve in ctx.admit_iter(owned, OPERATION)? {
        let (endpoints, _endpoint_storage) = ctx.with_scoped_storage(OPERATION, || {
            roster_curve_endpoint_markers(ctx, &lane.native_payload, curve, markers, &geometry)
        })?;
        for endpoint in ctx.admit_iter(endpoints, OPERATION)? {
            if indexed_only && endpoint.object_index().is_none() {
                continue;
            }
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut ids,
                    endpoint.id(),
                    "index SLDPRT profile curve endpoints",
                )
            })?;
        }
    }
    Ok(ids)
}

/// The owned, indexed point markers among the curve endpoints: the points an
/// axis must keep on one side.
fn profile_bounding_points(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    owned: &[&SketchInputEntity],
    curve_endpoints: &BTreeSet<&str>,
) -> Result<Vec<[f64; 2]>, CodecError> {
    const OPERATION: &str = "collect SLDPRT profile bounding points";
    let mut points = Vec::new();
    for marker in ctx.admit_iter(owned, OPERATION)? {
        if !matches!(
            marker.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        ) || marker.object_index().is_none()
            || !ctx.contains_btree_set(curve_endpoints, marker.id(), OPERATION)?
        {
            continue;
        }
        if let Some(coordinates) = marker.coordinates_m {
            storage.with_storage(|| ctx.push_vec(&mut points, coordinates.get(), OPERATION))?;
        }
    }
    Ok(points)
}

/// The coordinates of the markers that are curve endpoints, each with whether
/// the marker carries an object index.
fn curve_endpoint_points(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    roster: &LaneMarkerIndex<'_, '_>,
    curve_endpoints: &BTreeSet<&str>,
) -> Result<Vec<([f64; 2], bool)>, CodecError> {
    const OPERATION: &str = "collect SLDPRT profile curve endpoint points";
    let mut candidate_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut candidates = Vec::new();
    for id in ctx.admit_iter(curve_endpoints, OPERATION)? {
        let group = ctx
            .get_hash_map(&roster.by_id, *id, OPERATION)?
            .map_or(&[][..], Vec::as_slice);
        for &(index, marker) in ctx.admit_iter(group, OPERATION)? {
            if let Some(coordinates) = marker.coordinates_m {
                candidate_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut candidates,
                        (index, coordinates.get(), marker.object_index().is_some()),
                        OPERATION,
                    )
                })?;
            }
        }
    }
    ctx.sort_unstable_by(&mut candidates, |value| &value.0, Ord::cmp, OPERATION)?;
    storage.with_storage(|| {
        ctx.collect_vec(
            candidates
                .into_iter()
                .map(|(_, coordinates, indexed)| (coordinates, indexed)),
            OPERATION,
        )
    })
}

fn profile_roster_origin_axis_endpoints(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    roster: &LaneMarkerIndex<'_, '_>,
) -> Result<Option<[[f64; 2]; 2]>, CodecError> {
    const OPERATION: &str = "find SLDPRT origin axis";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let markers = &roster.all;
    let owned = ctx
        .get_hash_map(
            &roster.by_feature,
            profile_native,
            "collect SLDPRT owned profile markers",
        )?
        .map_or(&[][..], Vec::as_slice);
    let curve_endpoints =
        profile_curve_endpoint_ids(ctx, &mut storage, lane, owned, markers, true)?;
    let unreferenced = |marker: &&&SketchInputEntity| {
        Ok(matches!(
            marker.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        ) && marker.coordinates_m.is_some()
            && !ctx.contains_btree_set(&curve_endpoints, marker.id(), OPERATION)?)
    };
    let mut remaining = owned.iter();
    let Some(origin) = ctx.find_by(&mut remaining, unreferenced, OPERATION)? else {
        return Ok(None);
    };
    if ctx.any_by(&mut remaining, |marker| unreferenced(&marker), OPERATION)? {
        return Ok(None);
    }
    let Some(coordinates) = origin.coordinates_m else {
        return Ok(None);
    };
    let [origin_u, origin_v] = coordinates.get();
    if origin_u.abs() > EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
        || origin_v.abs() > EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
    {
        return Ok(None);
    }
    let bounding = profile_bounding_points(ctx, &mut storage, owned, &curve_endpoints)?;
    let endpoint_points = curve_endpoint_points(ctx, &mut storage, roster, &curve_endpoints)?;
    let mut candidates = Vec::new();
    for &(end, indexed) in ctx.admit_iter(&endpoint_points, OPERATION)? {
        let endpoints = [[origin_u, origin_v], end];
        if indexed && bounded_profile_axis_coordinates(ctx, &bounding, endpoints)? {
            storage.with_storage(|| ctx.push_vec(&mut candidates, endpoints, OPERATION))?;
        }
    }
    ctx.stable_sort_by_key(
        &mut candidates,
        |value| (value[1][0], value[1][1]),
        |left, right| left.0.total_cmp(&right.0).then(left.1.total_cmp(&right.1)),
        "sort SLDPRT origin axis candidates",
    )?;
    let collinear = |line: &[[f64; 2]; 2], [u, v]: [f64; 2]| {
        let [line_u, line_v] = [line[1][0] - origin_u, line[1][1] - origin_v];
        let relative_u = u - origin_u;
        let relative_v = v - origin_v;
        (relative_u * line_v - relative_v * line_u).abs()
            <= EPS_AXES_PROFILE_ROSTER_ORIGIN_AXIS_ENDPOINTS_E9
                * relative_u.hypot(relative_v)
                * line_u.hypot(line_v)
    };
    let mut lines = Vec::<[[f64; 2]; 2]>::new();
    for candidate in ctx.admit_iter(candidates, OPERATION)? {
        if ctx.any_by(&lines, |line| Ok(collinear(line, candidate[1])), OPERATION)? {
            continue;
        }
        storage.with_storage(|| {
            ctx.push_vec(
                &mut lines,
                candidate,
                "collect SLDPRT distinct origin axis lines",
            )
        })?;
    }
    let incidence = |line: &[[f64; 2]; 2]| -> Result<usize, CodecError> {
        let mut count = 0usize;
        for &(point, indexed) in ctx.admit_iter(&endpoint_points, OPERATION)? {
            if indexed && collinear(line, point) {
                count += 1;
            }
        }
        Ok(count)
    };
    let mut best: Option<(usize, [[f64; 2]; 2], bool)> = None;
    for line in ctx.admit_iter(&lines, OPERATION)? {
        let count = incidence(line)?;
        best = match best {
            Some((maximum, axis, unique)) if count < maximum => Some((maximum, axis, unique)),
            Some((maximum, axis, _)) if count == maximum => Some((maximum, axis, false)),
            _ => Some((count, *line, true)),
        };
    }
    Ok(best.and_then(|(_, axis, unique)| unique.then_some(axis)))
}

fn profile_roster_principal_axis_endpoints(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    roster: &LaneMarkerIndex<'_, '_>,
) -> Result<Option<[[f64; 2]; 2]>, CodecError> {
    const OPERATION: &str = "collect SLDPRT principal axis candidates";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let markers = &roster.all;
    let owned = ctx
        .get_hash_map(
            &roster.by_feature,
            profile_native,
            "collect SLDPRT owned profile markers",
        )?
        .map_or(&[][..], Vec::as_slice);
    let curve_endpoints =
        profile_curve_endpoint_ids(ctx, &mut storage, lane, owned, markers, true)?;
    let bounding = profile_bounding_points(ctx, &mut storage, owned, &curve_endpoints)?;
    let endpoint_points = curve_endpoint_points(ctx, &mut storage, roster, &curve_endpoints)?;
    let mut best: Option<(usize, [[f64; 2]; 2], bool)> = None;
    for axis in [[[0.0, 0.0], [1.0, 0.0]], [[0.0, 0.0], [0.0, 1.0]]] {
        if !bounded_profile_axis_coordinates(ctx, &bounding, axis)? {
            continue;
        }
        let [axis_u, axis_v] = axis[1];
        let mut count = 0usize;
        for &([u, v], _) in ctx.admit_iter(&endpoint_points, OPERATION)? {
            if (u * axis_v - v * axis_u).abs()
                <= EPS_AXES_PROFILE_ROSTER_PRINCIPAL_AXIS_ENDPOINTS_E9
            {
                count += 1;
            }
        }
        best = match best {
            Some((maximum, selected, unique)) if count < maximum => {
                Some((maximum, selected, unique))
            }
            Some((maximum, selected, _)) if count == maximum => Some((maximum, selected, false)),
            _ => Some((count, axis, true)),
        };
    }
    Ok(best.and_then(|(maximum, axis, unique)| (unique && maximum >= 2).then_some(axis)))
}

fn profile_roster_implicit_axis_endpoints<'a>(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    profile_native: &str,
    roster: &LaneMarkerIndex<'a, '_>,
) -> Result<Option<[&'a SketchInputEntity; 2]>, CodecError> {
    const OPERATION: &str = "find SLDPRT implicit profile axis";
    let markers = &roster.all;
    let geometry = crate::resolved_features::endpoints::geometry_index::MarkerGeometryIndex::new(
        ctx,
        markers,
        crate::resolved_features::endpoints::geometry_index::MarkerPrefixIndex::new(
            ctx,
            &lane.native_payload,
        )?,
    )?;
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let owned = ctx
        .get_hash_map(
            &roster.by_feature,
            profile_native,
            "collect SLDPRT owned profile markers",
        )?
        .map_or(&[][..], Vec::as_slice);
    let is_curve = |marker: &&&SketchInputEntity| {
        let Ok(offset) = usize::try_from(marker.offset()) else {
            return Ok(false);
        };
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
        Ok(current_code_two || detailed_indexed_curve)
    };
    let mut remaining = owned.iter();
    let first_curve = ctx.find_by(&mut remaining, is_curve, OPERATION)?.copied();
    let second_curve = match first_curve {
        Some(_) => ctx.find_by(&mut remaining, is_curve, OPERATION)?.copied(),
        None => None,
    };
    let curve_endpoints =
        profile_curve_endpoint_ids(ctx, &mut storage, lane, owned, markers, false)?;
    let bounding = profile_bounding_points(ctx, &mut storage, owned, &curve_endpoints)?;
    let bounded = |endpoints: [&SketchInputEntity; 2]| {
        let [Some(start), Some(end)] = endpoints.map(|endpoint| {
            endpoint
                .coordinates_m
                .map(cadmpeg_ir::units::FiniteVector::get)
        }) else {
            return Ok(false);
        };
        bounded_profile_axis_coordinates(ctx, &bounding, [start, end])
    };
    let mut unreferenced_points = Vec::new();
    for marker in ctx.admit_iter(owned, OPERATION)? {
        if matches!(
            marker.kind(),
            SketchInputKind::Point | SketchInputKind::ConstrainedPoint
        ) && marker.coordinates_m.is_some()
            && !ctx.contains_btree_set(&curve_endpoints, marker.id(), OPERATION)?
        {
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut unreferenced_points,
                    *marker,
                    "collect SLDPRT unreferenced profile points",
                )
            })?;
        }
    }
    if let [start, end] = unreferenced_points.as_slice() {
        let endpoints = [*start, *end];
        if bounded(endpoints)? {
            return Ok(Some(endpoints));
        }
    }
    let flagged = |marker: &&&SketchInputEntity| {
        Ok(index_from_u64(marker.offset()).is_some_and(|offset| {
            lane.native_payload.get(offset + 76..offset + 80) == Some(&1u32.to_le_bytes())
        }))
    };
    let mut remaining = unreferenced_points.iter();
    let first_flagged = ctx.find_by(&mut remaining, flagged, OPERATION)?;
    let second_flagged = match first_flagged {
        Some(_) => ctx.find_by(&mut remaining, flagged, OPERATION)?,
        None => None,
    };
    let third_flagged = match second_flagged {
        Some(_) => ctx.find_by(&mut remaining, flagged, OPERATION)?,
        None => None,
    };
    match (first_flagged, second_flagged, third_flagged) {
        (Some(start), Some(end), None) => {
            let endpoints = [*start, *end];
            if bounded(endpoints)? {
                return Ok(Some(endpoints));
            }
        }
        (Some(end), None, None) => {
            let mut by_offset = storage.with_storage(|| {
                ctx.collect_vec(
                    owned.iter().copied(),
                    "collect SLDPRT owned profile markers",
                )
            })?;
            ctx.sort_unstable_by_key(
                &mut by_offset,
                |value| value.offset(),
                Ord::cmp,
                "sldprt profile axis owned markers sort",
            )?;
            let preceding = ctx.find_map(
                by_offset.windows(2),
                |pair| {
                    Ok(ctx
                        .equal(pair[1].id(), end.id(), OPERATION)?
                        .then_some(pair[0]))
                },
                OPERATION,
            )?;
            if let Some(start) = preceding.filter(|marker| {
                marker.coordinates_m.is_some()
                    && matches!(
                        marker.kind(),
                        SketchInputKind::Point | SketchInputKind::ConstrainedPoint
                    )
            }) {
                let endpoints = [start, *end];
                if bounded(endpoints)? {
                    return Ok(Some(endpoints));
                }
            }
        }
        _ => {}
    }
    let mut boundary_relations = Vec::new();
    for candidate in ctx.admit_iter(owned, OPERATION)? {
        if !index_from_u64(candidate.offset()).is_some_and(|offset| {
            extended_wide_horizontal_relation_endpoint_indices(&lane.native_payload, offset)
                .is_some()
        }) {
            continue;
        }
        let endpoints = roster_curve_endpoint_markers(
            ctx,
            &lane.native_payload,
            candidate,
            markers,
            &geometry,
        )?;
        let [start, end] = endpoints.as_slice() else {
            continue;
        };
        let endpoints = [*start, *end];
        if !bounded(endpoints)? {
            continue;
        }
        storage.with_storage(|| {
            ctx.push_vec(
                &mut boundary_relations,
                endpoints,
                "collect SLDPRT profile boundary relations",
            )
        })?;
    }
    ctx.sort_unstable_by_key(
        &mut boundary_relations,
        |value| [value[0].offset(), value[1].offset()],
        Ord::cmp,
        "sldprt profile axis boundary relations sort",
    )?;
    ctx.dedup_by_key(
        &mut boundary_relations,
        |endpoints| Ok([endpoints[0].id(), endpoints[1].id()]),
        "deduplicate SLDPRT profile boundary relations",
    )?;
    match boundary_relations.as_slice() {
        [endpoints] => return Ok(Some(*endpoints)),
        [] => {}
        _ => return Ok(None),
    }
    let (Some(candidate), None) = (first_curve, second_curve) else {
        return Ok(None);
    };
    let (endpoints, _endpoint_storage) = ctx.with_scoped_storage(OPERATION, || {
        roster_curve_endpoint_markers(ctx, &lane.native_payload, candidate, markers, &geometry)
    })?;
    let [start, end] = endpoints.as_slice() else {
        return Ok(None);
    };
    let endpoints = [*start, *end];
    Ok(bounded(endpoints)?.then_some(endpoints))
}

#[cfg(test)]
fn bounded_profile_axis_endpoints(
    profile_native: &str,
    markers: &[&SketchInputEntity],
    curve_endpoints: &BTreeSet<&str>,
    endpoints: [&SketchInputEntity; 2],
) -> bool {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut storage = ctx.reserve_scoped(0, "test bounding points").unwrap();
    let roster = LaneMarkerIndex::new(&ctx, markers.iter().copied()).unwrap();
    let owned = ctx
        .get_hash_map(
            &roster.by_feature,
            profile_native,
            "test owned profile markers",
        )
        .unwrap()
        .map_or(&[][..], Vec::as_slice);
    let bounding = profile_bounding_points(&ctx, &mut storage, owned, curve_endpoints).unwrap();
    let [Some(start), Some(end)] = endpoints.map(|endpoint| {
        endpoint
            .coordinates_m
            .map(cadmpeg_ir::units::FiniteVector::get)
    }) else {
        return false;
    };
    bounded_profile_axis_coordinates(&ctx, &bounding, [start, end]).unwrap()
}

/// Whether every bounding point lies on one side of the axis through
/// `endpoints`.
fn bounded_profile_axis_coordinates(
    ctx: &DecodeContext<'_>,
    bounding: &[[f64; 2]],
    endpoints: [[f64; 2]; 2],
) -> Result<bool, CodecError> {
    const TOLERANCE_M: f64 = 1e-9;

    let [[start_u, start_v], [end_u, end_v]] = endpoints;
    let delta_u = end_u - start_u;
    let delta_v = end_v - start_v;
    let length = delta_u.hypot(delta_v);
    if !length.is_finite() || length <= TOLERANCE_M {
        return Ok(false);
    }
    let tangent_u = delta_u / length;
    let tangent_v = delta_v / length;
    let mut minimum_side = f64::INFINITY;
    let mut maximum_side = f64::NEG_INFINITY;
    let mut observed = false;
    for [u, v] in ctx.admit_iter(bounding, "bound SLDPRT profile axis")? {
        let relative_u = u - start_u;
        let relative_v = v - start_v;
        let side = relative_u * -tangent_v + relative_v * tangent_u;
        observed = true;
        minimum_side = minimum_side.min(side);
        maximum_side = maximum_side.max(side);
    }
    Ok(observed && (minimum_side >= -TOLERANCE_M || maximum_side <= TOLERANCE_M))
}

#[cfg(test)]
mod axes_tests;
