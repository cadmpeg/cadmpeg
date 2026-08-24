// SPDX-License-Identifier: Apache-2.0
//! Model-space reference entities from `MdlRefInfo`.

use cadmpeg_core::bytes::find_in;

use crate::scalar::{self, ScalarCache};
use crate::vecmath::{cross, dot, normalize_with_length};

const EPS_FRAME_ORTHONORMAL: f64 = 1.0e-9;
const EPS_ENDPOINT_AGREEMENT: f64 = 1.0e-9;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;
const EPS_PLANE_RESIDUAL: f64 = 1.0e-9;
const EPS_EQUAL_RADII_RELATIVE: f64 = 1.0e-12;
const EPS_ORIENTATION_NONZERO: f64 = 1.0e-12;
const EPS_LINE_NONZERO: f64 = 1.0e-12;
const EPS_CIRCLE_NORMAL_NONZERO: f64 = 1.0e-12;
const EPS_DIAMETER_PLANAR: f64 = 1.0e-10;

/// Stored reference-line family.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceLineKind {
    /// Planar `entity(line)` record.
    Line,
    /// Spatial `line3d` record with a stored original length.
    Line3d {
        /// Canonical entity identifier repeated across the row boundary.
        entity_id: u32,
        /// Positive stored `orig_len`, equal to the endpoint distance.
        original_length: f64,
    },
}

/// One finite model-space line entity.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceLine {
    /// Native entity family.
    pub kind: ReferenceLineKind,
    /// First endpoint in model coordinates.
    pub start: [f64; 3],
    /// Second endpoint in model coordinates.
    pub end: [f64; 3],
    /// Byte offset of the positional row in its section.
    pub offset: usize,
}

/// One circular reference entity reconstructed from a positional row.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceCircle {
    /// Canonical entity identifier repeated across the row boundary.
    pub entity_id: u32,
    /// Circle center in model coordinates.
    pub center: [f64; 3],
    /// Whether the center is stored explicitly rather than derived as a midpoint.
    pub center_stored: bool,
    /// Positive circle radius.
    pub radius: f64,
    /// Unit circle-plane normal.
    pub axis: [f64; 3],
    /// First stored endpoint.
    pub start: [f64; 3],
    /// Second stored endpoint.
    pub end: [f64; 3],
    /// Byte offset of the positional row in its section.
    pub offset: usize,
}

/// One named model-reference conic record.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceConic {
    /// Entity identifier in the conic list.
    pub entity_id: u32,
    /// Stored conic type discriminator.
    pub type_id: u32,
    /// Stored orientation selector.
    pub flip: u32,
    /// First stored endpoint in model coordinates.
    pub start: [f64; 3],
    /// Second stored endpoint in model coordinates.
    pub end: [f64; 3],
    /// First stored conic parameter, when its scalar form is defined.
    pub parameter_start: Option<f64>,
    /// Second stored conic parameter, when its scalar form is defined.
    pub parameter_end: Option<f64>,
    /// First stored conic coefficient.
    pub coefficient_1: f64,
    /// Second stored conic coefficient.
    pub coefficient_2: f64,
    /// Twelve decoded local-system slots, when the body is complete.
    pub local_system: Option<[f64; 12]>,
    /// Exact bytes from the `id` value through the local-system body.
    pub body: Vec<u8>,
    /// Byte offset of the named conic list record.
    pub offset: usize,
}

/// Complete model-space ellipse derived from a conic record.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceEllipse {
    /// Canonical identifier of the source conic entity.
    pub source_entity_id: u32,
    /// Ellipse center.
    pub center: [f64; 3],
    /// Unit normal of the ellipse plane.
    pub axis: [f64; 3],
    /// Unit direction of the semi-major axis.
    pub major_direction: [f64; 3],
    /// Positive semi-major radius.
    pub major_radius: f64,
    /// Positive semi-minor radius.
    pub minor_radius: f64,
    /// Source conic byte offset.
    pub offset: usize,
}

/// Derive every ellipse whose conic frame, radii, and endpoints independently
/// satisfy one model-space equation.
pub fn ellipse_carriers(conics: &[ReferenceConic]) -> Vec<ReferenceEllipse> {
    let mut result = Vec::new();
    for conic in conics {
        if conic.type_id != 30 {
            continue;
        }
        let Some(frame) = conic.local_system else {
            continue;
        };
        let center: [f64; 3] = frame[9..12].try_into().expect("three frame origin slots");
        let first_frame: [f64; 3] = frame[..3].try_into().expect("three frame axis slots");
        let second_frame: [f64; 3] = frame[3..6].try_into().expect("three frame axis slots");
        let Some((first_frame, first_length)) = normalize_with_length(first_frame) else {
            continue;
        };
        let Some((second_frame, second_length)) = normalize_with_length(second_frame) else {
            continue;
        };
        let scale = center
            .iter()
            .chain(conic.start.iter())
            .chain(conic.end.iter())
            .map(|value| value.abs())
            .fold(1.0_f64, f64::max);
        if (first_length - 1.0).abs() > EPS_FRAME_ORTHONORMAL
            || (second_length - 1.0).abs() > EPS_FRAME_ORTHONORMAL
            || dot(first_frame, second_frame).abs() > EPS_FRAME_ORTHONORMAL
        {
            continue;
        }
        let Some((axis, _)) = normalize_with_length(cross(first_frame, second_frame)) else {
            continue;
        };
        let radii = [conic.coefficient_1.abs(), conic.coefficient_2.abs()];
        if radii
            .iter()
            .any(|radius| !radius.is_finite() || *radius <= 0.0)
        {
            continue;
        }
        let major_radius = radii[0].max(radii[1]);
        let minor_radius = radii[0].min(radii[1]);
        let endpoints = [conic.start, conic.end];
        let endpoint_deltas =
            endpoints.map(|endpoint| std::array::from_fn(|index| endpoint[index] - center[index]));
        let antipodal_major_direction = (|| {
            let (first_direction, first_radius) = normalize_with_length(endpoint_deltas[0])?;
            let (_, second_radius) = normalize_with_length(endpoint_deltas[1])?;
            ((0..3).all(|index| {
                (endpoint_deltas[0][index] + endpoint_deltas[1][index]).abs()
                    <= EPS_ENDPOINT_AGREEMENT * scale
            }) && dot(first_direction, axis).abs() <= EPS_ENDPOINT_AGREEMENT
                && (first_radius - second_radius).abs() <= EPS_RADIUS_AGREEMENT * scale)
                .then_some(())?;
            let radius_scale = major_radius.max(1.0);
            if (first_radius - major_radius).abs() <= EPS_RADIUS_AGREEMENT * radius_scale {
                Some(first_direction)
            } else if (first_radius - minor_radius).abs() <= EPS_RADIUS_AGREEMENT * radius_scale {
                normalize_with_length(cross(first_direction, axis)).map(|(direction, _)| direction)
            } else {
                None
            }
        })();
        if let Some(major_direction) = antipodal_major_direction {
            result.push(ReferenceEllipse {
                source_entity_id: conic.entity_id,
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
                offset: conic.offset,
            });
            continue;
        }
        let mapping_is_valid = |first_radius: f64, second_radius: f64| {
            endpoints.iter().all(|endpoint| {
                let delta = std::array::from_fn(|index| endpoint[index] - center[index]);
                let first = dot(delta, first_frame);
                let second = dot(delta, second_frame);
                let plane = dot(delta, axis);
                plane.abs() <= EPS_PLANE_RESIDUAL * scale
                    && ((first / first_radius).powi(2) + (second / second_radius).powi(2) - 1.0)
                        .abs()
                        <= EPS_PLANE_RESIDUAL
            })
        };
        let direct = mapping_is_valid(radii[0], radii[1]);
        let swapped = mapping_is_valid(radii[1], radii[0]);
        let equal_radii =
            (radii[0] - radii[1]).abs() <= EPS_EQUAL_RADII_RELATIVE * radii[0].max(radii[1]);
        let (first_radius, second_radius) = if direct && (!swapped || equal_radii) {
            (radii[0], radii[1])
        } else if swapped && !direct {
            (radii[1], radii[0])
        } else {
            continue;
        };
        let mut major_direction = if first_radius >= second_radius {
            first_frame
        } else {
            second_frame
        };
        let orientation = endpoints
            .iter()
            .map(|endpoint| {
                dot(
                    std::array::from_fn(|index| endpoint[index] - center[index]),
                    major_direction,
                )
            })
            .find(|projection| projection.abs() > EPS_ORIENTATION_NONZERO * scale);
        if orientation.is_some_and(f64::is_sign_negative) {
            major_direction = major_direction.map(|value| -value);
        }
        result.push(ReferenceEllipse {
            source_entity_id: conic.entity_id,
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
            offset: conic.offset,
        });
    }
    result.sort_by_key(|ellipse| ellipse.offset);
    result
}

fn coordinate(data: &[u8], offset: usize, cache: &ScalarCache) -> Option<(f64, usize)> {
    if data.get(offset) == Some(&0x18)
        && scalar::decode_model_reference_coordinate(data, offset + 1, cache).is_some()
    {
        return Some((0.0, offset + 1));
    }
    scalar::decode_model_reference_coordinate(data, offset, cache)
}

fn arc_z_coordinate(data: &[u8], offset: usize, cache: &ScalarCache) -> Option<(f64, usize)> {
    // Arc rows use the first-coordinate lane for every stored coordinate.
    // Its overlapping prefixes have different mappings in the model-reference
    // lane, so that lane is only a fallback for tokens with no arc form.
    if data.get(offset) == Some(&0x18)
        && (scalar::decode_tabulated_cylinder_first_coordinate(data, offset + 1, cache).is_some()
            || scalar::decode_model_reference_coordinate(data, offset + 1, cache).is_some())
    {
        return Some((0.0, offset + 1));
    }
    scalar::decode_tabulated_cylinder_first_coordinate(data, offset, cache)
        .or_else(|| scalar::decode_model_reference_coordinate(data, offset, cache))
}

fn scalar_suffix(row: &[u8], count: usize, cache: &ScalarCache) -> Option<Vec<f64>> {
    let mut candidate = None;
    for start in 0..row.len() {
        let Some(values) = (|| {
            let mut cursor = crate::psb::Cursor::at(row, start);
            let mut values = Vec::with_capacity(count);
            while values.len() < count {
                values.push(cursor.take_with(|data, pos| coordinate(data, pos, cache))?);
            }
            (cursor.pos() == row.len() && values.iter().all(|value| value.is_finite()))
                .then_some(values)
        })() else {
            continue;
        };
        if candidate.is_some() {
            return None;
        }
        candidate = Some(values);
    }
    candidate
}

const CONIC_FIELD_HEADERS: [&[u8]; 10] = [
    b"\xe0\x01id\0",
    b"\xe0\x01type\0",
    b"\xe0\x01flip\0",
    b"\xe0\x02end1\0",
    b"\xe0\x02end2\0",
    b"\xe0\x02t0\0",
    b"\xe0\x02t1\0",
    b"\xe0\x02c1\0",
    b"\xe0\x02c2\0",
    b"\xe0\x02local_sys\0",
];

fn next_conic_field(data: &[u8], start: usize, end: usize) -> Option<(usize, usize)> {
    CONIC_FIELD_HEADERS
        .iter()
        .enumerate()
        .filter_map(|(field, header)| {
            find_in(data, header, start, end).map(|offset| (offset, field))
        })
        .min_by_key(|(offset, _)| *offset)
}

fn expected_conic_field(data: &[u8], start: usize, end: usize, expected: usize) -> Option<usize> {
    let (offset, field) = next_conic_field(data, start, end)?;
    (field == expected).then_some(offset)
}

fn conic_point_at(
    data: &[u8],
    label_offset: usize,
    end: usize,
    cache: &ScalarCache,
) -> Option<([f64; 3], usize)> {
    let array_open = label_offset + CONIC_FIELD_HEADERS[3].len();
    (array_open < end && data.get(array_open) == Some(&crate::psb::token::ARRAY_OPEN))
        .then_some(())?;
    let (count, after_count) = crate::psb::compact_int(data, array_open + 1);
    (count == 3 && after_count > array_open + 1).then_some(())?;
    let mut cursor = crate::psb::Cursor::at(data, after_count);
    let mut values = [0.0; 3];
    for value in &mut values {
        let decoded = cursor.take_with(|data, pos| coordinate(data, pos, cache))?;
        (cursor.pos() <= end).then_some(())?;
        *value = decoded;
    }
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some((values, cursor.pos()))
}

/// One decoded run of conic local-frame slots: the axis marker `18 e5` emits
/// `[0, 1, 0]`, a bare `18` zero marker emits `[0]`, and a frame coordinate
/// emits its single value.
struct ConicFrameRun {
    slots: [f64; 3],
    len: usize,
}

impl ConicFrameRun {
    fn triple(slots: [f64; 3]) -> Self {
        Self { slots, len: 3 }
    }

    fn single(value: f64) -> Self {
        Self {
            slots: [value, 0.0, 0.0],
            len: 1,
        }
    }

    fn as_slice(&self) -> &[f64] {
        &self.slots[..self.len]
    }
}

/// Decode one conic local-frame slot run at `offset`.
///
/// - `18 e5`: axis marker, emits `[0, 1, 0]` and consumes two bytes.
/// - `18` at end of body: zero marker, emits `[0]` and consumes one byte.
/// - `18` followed by a frame coordinate: zero marker, emits `[0]` and
///   consumes only the `18` (the coordinate is left for the next run).
/// - otherwise: a frame coordinate, emitting its value.
///
/// Returns `None` only when no arm applies, aborting the frame walk exactly as
/// the original trailing `frame_coordinate(cursor)?` did.
fn conic_frame_run(
    data: &[u8],
    offset: usize,
    cache: &ScalarCache,
) -> Option<(ConicFrameRun, usize)> {
    let frame_coordinate = |off| {
        scalar::decode_model_reference_coordinate(data, off, cache)
            .or_else(|| scalar::decode_tabulated_cylinder_frame_coordinate(data, off, cache))
    };
    if data.get(offset..offset + 2) == Some(&[0x18, 0xe5]) {
        return Some((ConicFrameRun::triple([0.0, 1.0, 0.0]), offset + 2));
    }
    if data.get(offset) == Some(&0x18) && offset + 1 == data.len() {
        return Some((ConicFrameRun::single(0.0), offset + 1));
    }
    if data.get(offset) == Some(&0x18) && frame_coordinate(offset + 1).is_some() {
        return Some((ConicFrameRun::single(0.0), offset + 1));
    }
    let (value, next) = frame_coordinate(offset)?;
    Some((ConicFrameRun::single(value), next))
}

fn conic_local_system(body: &[u8], cache: &ScalarCache) -> Option<[f64; 12]> {
    if let Some(slots) = scalar::decode_explicit_local_system_slots(body, cache) {
        return Some(slots);
    }
    let mut values = Vec::with_capacity(12);
    let mut cursor = crate::psb::Cursor::new(body);
    while cursor.pos() < body.len() && values.len() < 12 {
        let run = cursor.take_with(|data, pos| conic_frame_run(data, pos, cache))?;
        values.extend_from_slice(run.as_slice());
    }
    (cursor.pos() == body.len()
        && values.len() == 12
        && values.iter().all(|value| value.is_finite()))
    .then(|| values.try_into().expect("twelve bounded conic frame slots"))
}

fn named_conic_local_system(
    data: &[u8],
    start: usize,
    end: usize,
    cache: &ScalarCache,
) -> Option<(usize, Option<[f64; 12]>)> {
    const TERMINATOR: &[u8] = &[0xf2, crate::psb::token::ENTITY_REF];
    const MAX_FRAME_BYTES: usize = 12 * 9;
    let mut marker_count = 0;
    let mut only_marker = 0;
    let mut complete_frame = None;
    let mut competing_frame = false;
    for candidate in start..end {
        if data.get(candidate..candidate + TERMINATOR.len()) != Some(TERMINATOR) {
            continue;
        }
        marker_count += 1;
        only_marker = candidate;
        if candidate - start <= MAX_FRAME_BYTES {
            if let Some(frame) = conic_local_system(&data[start..candidate], cache) {
                competing_frame |= complete_frame.is_some();
                complete_frame = Some((candidate, frame));
            }
        }
    }
    if !competing_frame {
        if let Some(frame) = complete_frame {
            return Some((frame.0, Some(frame.1)));
        }
    }
    match marker_count {
        0 => Some((end, conic_local_system(&data[start..end], cache))),
        1 => Some((
            only_marker,
            conic_local_system(&data[start..only_marker], cache),
        )),
        _ => None,
    }
}

/// Decode the named entity that establishes each `ent_list(conic)` schema.
///
/// The coefficients and parameter fields remain stored conic semantics; this
/// function does not classify the record as an ellipse, parabola, or
/// hyperbola.
pub fn named_conics(payload: &[u8]) -> Vec<ReferenceConic> {
    const LIST: &[u8] = b"ent_list(conic)\0";
    const NEXT_LIST: &[u8] = b"\xe0\x00ent_list(";
    let cache = ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(offset) = find_in(payload, LIST, search, payload.len()) {
        let fields_start = offset + LIST.len();
        let block_end =
            find_in(payload, NEXT_LIST, fields_start, payload.len()).unwrap_or(payload.len());
        let Some(id_label) = expected_conic_field(payload, fields_start, block_end, 0) else {
            search = block_end.max(fields_start);
            continue;
        };
        let (entity_id, after_id) =
            crate::psb::compact_int(payload, id_label + CONIC_FIELD_HEADERS[0].len());
        if after_id == id_label + CONIC_FIELD_HEADERS[0].len() {
            search = block_end.max(fields_start);
            continue;
        }
        let Some(type_label) = expected_conic_field(payload, after_id, block_end, 1) else {
            search = block_end.max(fields_start);
            continue;
        };
        let (type_id, after_type) =
            crate::psb::compact_int(payload, type_label + CONIC_FIELD_HEADERS[1].len());
        if after_type == type_label + CONIC_FIELD_HEADERS[1].len() {
            search = block_end.max(fields_start);
            continue;
        }
        let Some(flip_label) = expected_conic_field(payload, after_type, block_end, 2) else {
            search = block_end.max(fields_start);
            continue;
        };
        let (flip, after_flip) =
            crate::psb::compact_int(payload, flip_label + CONIC_FIELD_HEADERS[2].len());
        if after_flip == flip_label + CONIC_FIELD_HEADERS[2].len() {
            search = block_end.max(fields_start);
            continue;
        }
        let Some(end1_label) = expected_conic_field(payload, after_flip, block_end, 3) else {
            search = block_end.max(fields_start);
            continue;
        };
        let Some((start, after_end1)) = conic_point_at(payload, end1_label, block_end, &cache)
        else {
            search = block_end.max(fields_start);
            continue;
        };
        let Some(end2_label) = expected_conic_field(payload, after_end1, block_end, 4) else {
            search = block_end.max(fields_start);
            continue;
        };
        let Some((end, mut cursor)) = conic_point_at(payload, end2_label, block_end, &cache) else {
            search = block_end.max(fields_start);
            continue;
        };
        let mut parameter_start = None;
        let mut parameter_end = None;
        if let Some((label, 5)) = next_conic_field(payload, cursor, block_end) {
            let Some((value, next)) =
                coordinate(payload, label + CONIC_FIELD_HEADERS[5].len(), &cache)
            else {
                search = block_end.max(fields_start);
                continue;
            };
            if !value.is_finite() || next > block_end {
                search = block_end.max(fields_start);
                continue;
            }
            parameter_start = Some(value);
            cursor = next;
        }
        if let Some((label, 6)) = next_conic_field(payload, cursor, block_end) {
            let value_offset = label + CONIC_FIELD_HEADERS[6].len();
            if payload.get(value_offset) == Some(&0x11) {
                let Some(value) = parameter_start else {
                    search = block_end.max(fields_start);
                    continue;
                };
                parameter_end = Some(value + std::f64::consts::PI);
                cursor = value_offset + 1;
            } else if let Some((value, next)) = coordinate(payload, value_offset, &cache) {
                if !value.is_finite() || next > block_end {
                    search = block_end.max(fields_start);
                    continue;
                }
                parameter_end = Some(value);
                cursor = next;
            } else {
                search = block_end.max(fields_start);
                continue;
            }
        }
        let Some(c1_label) = expected_conic_field(payload, cursor, block_end, 7) else {
            search = block_end.max(fields_start);
            continue;
        };
        let Some((coefficient_1, after_c1)) =
            coordinate(payload, c1_label + CONIC_FIELD_HEADERS[7].len(), &cache)
        else {
            search = block_end.max(fields_start);
            continue;
        };
        let Some(c2_label) = expected_conic_field(payload, after_c1, block_end, 8) else {
            search = block_end.max(fields_start);
            continue;
        };
        let Some((coefficient_2, after_c2)) =
            coordinate(payload, c2_label + CONIC_FIELD_HEADERS[8].len(), &cache)
        else {
            search = block_end.max(fields_start);
            continue;
        };
        let Some(local_label) = expected_conic_field(payload, after_c2, block_end, 9) else {
            search = block_end.max(fields_start);
            continue;
        };
        let local_opener = local_label + CONIC_FIELD_HEADERS[9].len();
        if payload.get(local_opener..local_opener + 3) != Some(&[0xf9, 0x04, 0x03]) {
            search = block_end.max(fields_start);
            continue;
        }
        let local_start = local_opener + 3;
        let Some((local_end, local_system)) =
            named_conic_local_system(payload, local_start, block_end, &cache)
        else {
            search = block_end.max(fields_start);
            continue;
        };
        if !coefficient_1.is_finite()
            || !coefficient_2.is_finite()
            || next_conic_field(payload, local_end, block_end).is_some()
        {
            search = block_end.max(fields_start);
            continue;
        }
        result.push(ReferenceConic {
            entity_id,
            type_id,
            flip,
            start,
            end,
            parameter_start,
            parameter_end,
            coefficient_1,
            coefficient_2,
            local_system,
            body: payload[id_label + CONIC_FIELD_HEADERS[0].len()..local_end].to_vec(),
            offset,
        });
        search = block_end.max(fields_start);
    }
    result.sort_by_key(|conic| conic.offset);
    result
}

fn conic_parameter(
    body: &[u8],
    offset: usize,
    opposite_of: Option<f64>,
    cache: &ScalarCache,
) -> Option<(Option<f64>, usize)> {
    if body.get(offset) == Some(&0x11) {
        return Some((
            opposite_of.map(|value| value + std::f64::consts::PI),
            offset + 1,
        ));
    }
    coordinate(body, offset, cache).map(|(value, next)| (Some(value), next))
}

fn positional_conic_local_system(
    body: &[u8],
    local_start: usize,
    cache: &ScalarCache,
) -> Option<(usize, [f64; 12])> {
    const MAX_FRAME_BYTES: usize = 12 * 9;
    let first_end = local_start.checked_add(1)?;
    let last_end = local_start.saturating_add(MAX_FRAME_BYTES).min(body.len());
    let candidates = (first_end..=last_end)
        .filter_map(|end| {
            let tail = body.get(end..)?;
            (tail.is_empty() || tail.first() == Some(&0xe2)).then_some(())?;
            conic_local_system(&body[local_start..end], cache).map(|frame| (end, frame))
        })
        .collect::<Vec<_>>();
    let [(local_end, local_system)] = candidates.as_slice() else {
        return None;
    };
    Some((*local_end, *local_system))
}

fn positional_conic_body(
    body: &[u8],
    entity_id: u32,
    type_id: u32,
    offset: usize,
    cache: &ScalarCache,
) -> Option<ReferenceConic> {
    const GENERAL_INFO: &[u8] = &[0x02, 0x48, 0x10, 0x00, 0xeb, 0x10, 0, 0, 0, 0];
    (body.get(..GENERAL_INFO.len()) == Some(GENERAL_INFO)).then_some(())?;
    let (flip, mut cursor) = crate::psb::compact_int(body, GENERAL_INFO.len());
    (cursor > GENERAL_INFO.len()).then_some(())?;
    let mut endpoints = [[0.0; 3]; 2];
    for point in &mut endpoints {
        for value in point {
            let (decoded, next) = coordinate(body, cursor, cache)?;
            *value = decoded;
            cursor = next;
        }
    }
    let (parameter_start, next) = conic_parameter(body, cursor, None, cache)?;
    cursor = next;
    let (parameter_end, next) = conic_parameter(body, cursor, parameter_start, cache)?;
    cursor = next;
    let (coefficient_1, next) = coordinate(body, cursor, cache)?;
    cursor = next;
    let (coefficient_2, local_start) = coordinate(body, cursor, cache)?;
    let (local_end, local_system) = positional_conic_local_system(body, local_start, cache)?;
    endpoints
        .iter()
        .flatten()
        .chain(parameter_start.iter())
        .chain(parameter_end.iter())
        .chain([&coefficient_1, &coefficient_2])
        .all(|value| value.is_finite())
        .then_some(())?;
    Some(ReferenceConic {
        entity_id,
        type_id,
        flip,
        start: endpoints[0],
        end: endpoints[1],
        parameter_start,
        parameter_end,
        coefficient_1,
        coefficient_2,
        local_system: Some(local_system),
        body: body[..local_end].to_vec(),
        offset,
    })
}

/// Decode complete positional rows following an `ent_list(conic)` schema.
pub fn positional_conics(payload: &[u8]) -> Vec<ReferenceConic> {
    const LIST: &[u8] = b"ent_list(conic)\0";
    const NEXT_LIST: &[u8] = b"\xe0\x00ent_list(";
    let cache = ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(prototype) = find_in(payload, LIST, search, payload.len()) {
        let rows_start = prototype + LIST.len();
        let block_end =
            find_in(payload, NEXT_LIST, rows_start, payload.len()).unwrap_or(payload.len());
        let mut headers = Vec::new();
        for close in rows_start..block_end {
            if payload.get(close) != Some(&0xe3) {
                continue;
            }
            let Ok((entity_id, after_id)) = crate::psb::reference_id(payload, close + 1) else {
                continue;
            };
            if !matching_row_id(payload, close, entity_id) {
                continue;
            }
            let (type_id, after_type) = crate::psb::compact_int(payload, after_id);
            if after_type == after_id || payload.get(after_type) != Some(&0xe2) {
                continue;
            }
            headers.push((close, entity_id, type_id, after_type + 1));
        }
        for (index, &(close, entity_id, type_id, body_start)) in headers.iter().enumerate() {
            let body_end = headers
                .get(index + 1)
                .map_or(block_end, |(next_close, _, _, _)| *next_close);
            if let Some(conic) = positional_conic_body(
                &payload[body_start..body_end],
                entity_id,
                type_id,
                close + 1,
                &cache,
            ) {
                result.push(conic);
            }
        }
        search = block_end.max(rows_start);
    }
    result.sort_by_key(|conic| conic.offset);
    result.dedup_by_key(|conic| conic.offset);
    result
}

/// Decode every complete positional `entity(line)` row.
pub fn lines(payload: &[u8]) -> Vec<ReferenceLine> {
    const PROTOTYPE: &[u8] = b"ent_list(line)\0";
    const LIST: &[u8] = b"\xe0\x00ent_list(";
    const INSTANCE: &[u8] = b"\xe0\x00entity(line)\0";
    const ENTITY: &[u8] = b"\xe0\x00entity(";
    const ROW_START: &[u8] = b"\xf6\xe2";

    let cache = ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(prototype) = payload[search..]
        .windows(PROTOTYPE.len())
        .position(|window| window == PROTOTYPE)
        .map(|relative| search + relative)
    {
        let instance_search = prototype + PROTOTYPE.len();
        let prototype_end = payload[instance_search..]
            .windows(LIST.len())
            .position(|window| window == LIST)
            .map_or(payload.len(), |relative| instance_search + relative);
        let Some(instance) = payload[instance_search..prototype_end]
            .windows(INSTANCE.len())
            .position(|window| window == INSTANCE)
            .map(|relative| instance_search + relative)
        else {
            search = prototype_end.max(instance_search);
            continue;
        };
        let rows_start = instance + INSTANCE.len();
        let block_end = payload[rows_start..]
            .windows(ENTITY.len())
            .position(|window| window == ENTITY)
            .map_or(payload.len(), |relative| rows_start + relative);
        let mut starts = Vec::new();
        let mut cursor = rows_start;
        while let Some(start) = payload[cursor..block_end]
            .windows(ROW_START.len())
            .position(|window| window == ROW_START)
            .map(|relative| cursor + relative)
        {
            if starts.is_empty() || payload.get(start.wrapping_sub(1)) == Some(&0xe3) {
                starts.push(start);
            }
            cursor = start + ROW_START.len();
        }
        for (index, start) in starts.iter().copied().enumerate() {
            let end = starts.get(index + 1).map_or(block_end, |next| next - 1);
            let end = if payload.get(end.wrapping_sub(1)) == Some(&0xe3) {
                end - 1
            } else {
                end
            };
            if start >= end {
                continue;
            }
            let Some(values) = scalar_suffix(&payload[start..end], 6, &cache) else {
                continue;
            };
            result.push(ReferenceLine {
                kind: ReferenceLineKind::Line,
                start: values[..3].try_into().expect("three bounded coordinates"),
                end: values[3..].try_into().expect("three bounded coordinates"),
                offset: start,
            });
        }
        search = block_end.max(instance_search);
    }
    result.sort_by_key(|line| line.offset);
    result.dedup_by_key(|line| line.offset);
    result
}

fn line3d_fields(body: &[u8], cache: &ScalarCache) -> Option<([f64; 3], [f64; 3], f64)> {
    let candidates = (0..body.len()).filter_map(|start| {
        let mut cursor = start;
        let mut values = Vec::with_capacity(7);
        while values.len() < 7 {
            let (value, next) = coordinate(body, cursor, cache)?;
            values.push(value);
            cursor = next;
        }
        let first: [f64; 3] = values[..3].try_into().ok()?;
        let second: [f64; 3] = values[3..6].try_into().ok()?;
        let delta = std::array::from_fn::<_, 3, _>(|axis| second[axis] - first[axis]);
        let distance = delta.iter().fold(0.0_f64, |norm, value| norm.hypot(*value));
        let stored_length = values[6].abs();
        let scale = distance.max(stored_length).max(1.0);
        (distance.is_finite()
            && distance > EPS_LINE_NONZERO
            && stored_length > 0.0
            && (distance - stored_length).abs() <= EPS_ENDPOINT_AGREEMENT * scale)
            .then_some((start, first, second, stored_length))
    });
    let mut candidates = candidates;
    let (_, first, second, stored_length) = candidates.next()?;
    candidates.next().is_none().then_some(())?;
    Some((first, second, stored_length))
}

fn matching_row_id(payload: &[u8], close: usize, id: u32) -> bool {
    let start = close.saturating_sub(8);
    (start..close).any(|candidate| {
        let Ok((previous, after)) = crate::psb::reference_id(payload, candidate) else {
            return false;
        };
        if previous != id {
            return false;
        }
        after == close
            || (payload.get(after) == Some(&crate::psb::token::ENTITY_REF)
                && crate::psb::reference_id(payload, after + 1)
                    .is_ok_and(|(_, reference_end)| reference_end == close))
    })
}

/// Decode complete positional `line3d` rows whose endpoint distance equals
/// their stored original length.
pub fn line3d_lines(payload: &[u8]) -> Vec<ReferenceLine> {
    const PROTOTYPE: &[u8] = b"ent_list(line3d)\0";
    const LIST: &[u8] = b"\xe0\x00ent_list(";

    let cache = ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(prototype) = payload[search..]
        .windows(PROTOTYPE.len())
        .position(|window| window == PROTOTYPE)
        .map(|relative| search + relative)
    {
        let rows_start = prototype + PROTOTYPE.len();
        let block_end = payload[rows_start..]
            .windows(LIST.len())
            .position(|window| window == LIST)
            .map_or(payload.len(), |relative| rows_start + relative);
        let mut headers = Vec::new();
        for close in rows_start..block_end {
            if payload.get(close) != Some(&0xe3) {
                continue;
            }
            let Ok((id, after_id)) = crate::psb::reference_id(payload, close + 1) else {
                continue;
            };
            if !matching_row_id(payload, close, id) {
                continue;
            }
            let (_, body_start) = crate::psb::compact_int(payload, after_id);
            if body_start == after_id || payload.get(body_start) != Some(&0xe2) {
                continue;
            }
            let body_start = body_start + 1;
            headers.push((close, body_start, id));
        }
        for (index, (close, body_start, entity_id)) in headers.iter().copied().enumerate() {
            let body_end = headers
                .get(index + 1)
                .map_or(block_end, |(next_close, _, _)| *next_close);
            let Some((start, end, original_length)) =
                line3d_fields(&payload[body_start..body_end], &cache)
            else {
                continue;
            };
            result.push(ReferenceLine {
                kind: ReferenceLineKind::Line3d {
                    entity_id,
                    original_length,
                },
                start,
                end,
                offset: close + 1,
            });
        }
        search = block_end.max(rows_start);
    }
    result.sort_by_key(|line| line.offset);
    result.dedup_by_key(|line| line.offset);
    result
}

fn arc_z_fields(body: &[u8], cache: &ScalarCache, entity_id: u32) -> Option<ReferenceCircle> {
    let scalar_run = |start: usize, count: usize| {
        let mut cursor = start;
        let mut values = Vec::with_capacity(count);
        while values.len() < count {
            let (value, next) = arc_z_coordinate(body, cursor, cache)?;
            values.push(value);
            cursor = next;
        }
        Some(values)
    };
    let explicit_axis = |center: [f64; 3], radius: f64, first: [f64; 3], second: [f64; 3]| {
        let first_delta = std::array::from_fn::<_, 3, _>(|axis| first[axis] - center[axis]);
        let second_delta = std::array::from_fn::<_, 3, _>(|axis| second[axis] - center[axis]);
        let first_distance = first_delta
            .iter()
            .fold(0.0_f64, |norm, value| norm.hypot(*value));
        let second_distance = second_delta
            .iter()
            .fold(0.0_f64, |norm, value| norm.hypot(*value));
        let scale = radius.max(first_distance).max(second_distance).max(1.0);
        let normal = [
            first_delta[1] * second_delta[2] - first_delta[2] * second_delta[1],
            first_delta[2] * second_delta[0] - first_delta[0] * second_delta[2],
            first_delta[0] * second_delta[1] - first_delta[1] * second_delta[0],
        ];
        let normal_length = normal
            .iter()
            .fold(0.0_f64, |norm, value| norm.hypot(*value));
        (radius.is_finite()
            && radius > 0.0
            && center
                .iter()
                .chain(first.iter())
                .chain(second.iter())
                .all(|value| value.is_finite())
            && first_distance.is_finite()
            && second_distance.is_finite()
            && (first_distance - radius).abs() <= EPS_RADIUS_AGREEMENT * scale
            && (second_distance - radius).abs() <= EPS_RADIUS_AGREEMENT * scale
            && normal_length.is_finite()
            && normal_length > EPS_CIRCLE_NORMAL_NONZERO * scale * scale)
            .then(|| normal.map(|value| value / normal_length))
    };
    let explicit = (0..body.len()).filter_map(|start| {
        let values = scalar_run(start, 10)?;
        let center: [f64; 3] = values[..3].try_into().ok()?;
        let radius = values[3].abs();
        let first: [f64; 3] = values[4..7].try_into().ok()?;
        let second: [f64; 3] = values[7..10].try_into().ok()?;
        let axis = explicit_axis(center, radius, first, second)?;
        Some(ReferenceCircle {
            entity_id,
            center,
            center_stored: true,
            radius,
            axis,
            start: first,
            end: second,
            offset: start,
        })
    });
    let diametric = (0..body.len()).filter_map(|start| {
        let values = scalar_run(start, 7)?;
        let radius = values[0].abs();
        let first: [f64; 3] = values[1..4].try_into().ok()?;
        let second: [f64; 3] = values[4..7].try_into().ok()?;
        let center = std::array::from_fn(|axis| (first[axis] + second[axis]) * 0.5);
        let delta = std::array::from_fn::<_, 3, _>(|axis| second[axis] - first[axis]);
        let diameter = delta.iter().fold(0.0_f64, |norm, value| norm.hypot(*value));
        let scale = radius.max(diameter).max(1.0);
        (diameter.is_finite()
            && radius > 0.0
            && values.iter().all(|value| value.is_finite())
            && delta[2].abs() <= EPS_DIAMETER_PLANAR * scale
            && (diameter - 2.0 * radius).abs() <= EPS_RADIUS_AGREEMENT * scale)
            .then_some(ReferenceCircle {
                entity_id,
                center,
                center_stored: false,
                radius,
                axis: [0.0, 0.0, 1.0],
                start: first,
                end: second,
                offset: start,
            })
    });
    let mut candidates = explicit.chain(diametric);
    let circle = candidates.next()?;
    candidates.next().is_none().then_some(circle)
}

/// Decode complete positional `arc_z` rows whose stored center, radius, and
/// endpoints satisfy the model-Z circle equation. Diameter-compressed rows
/// derive the center from their endpoint midpoint.
pub fn arc_z_circles(payload: &[u8]) -> Vec<ReferenceCircle> {
    const PROTOTYPE: &[u8] = b"ent_list(arc_z)\0";
    const LIST: &[u8] = b"\xe0\x00ent_list(";

    let cache = ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(prototype) = payload[search..]
        .windows(PROTOTYPE.len())
        .position(|window| window == PROTOTYPE)
        .map(|relative| search + relative)
    {
        let rows_start = prototype + PROTOTYPE.len();
        let block_end = payload[rows_start..]
            .windows(LIST.len())
            .position(|window| window == LIST)
            .map_or(payload.len(), |relative| rows_start + relative);
        let mut headers = Vec::new();
        for close in rows_start..block_end {
            if payload.get(close) != Some(&0xe3) {
                continue;
            }
            let Ok((id, after_id)) = crate::psb::reference_id(payload, close + 1) else {
                continue;
            };
            if !matching_row_id(payload, close, id) {
                continue;
            }
            let (_, body_start) = crate::psb::compact_int(payload, after_id);
            if body_start == after_id || payload.get(body_start) != Some(&0xe2) {
                continue;
            }
            headers.push((close, body_start + 1, id));
        }
        for (index, (close, body_start, entity_id)) in headers.iter().copied().enumerate() {
            let body_end = headers
                .get(index + 1)
                .map_or(block_end, |(next_close, _, _)| *next_close);
            let Some(mut circle) = arc_z_fields(&payload[body_start..body_end], &cache, entity_id)
            else {
                continue;
            };
            circle.offset = close + 1;
            result.push(circle);
        }
        search = block_end.max(rows_start);
    }
    result.sort_by_key(|circle| circle.offset);
    result.dedup_by_key(|circle| circle.offset);
    result
}

#[cfg(test)]
mod tests;
