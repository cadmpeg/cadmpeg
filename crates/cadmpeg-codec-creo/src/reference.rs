// SPDX-License-Identifier: Apache-2.0
//! Model-space reference entities from `MdlRefInfo`.

use crate::scalar::{self, ScalarCache};

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

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0].mul_add(right[0], left[1].mul_add(right[1], left[2] * right[2]))
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1].mul_add(right[2], -(left[2] * right[1])),
        left[2].mul_add(right[0], -(left[0] * right[2])),
        left[0].mul_add(right[1], -(left[1] * right[0])),
    ]
}

fn normalize(vector: [f64; 3]) -> Option<([f64; 3], f64)> {
    let magnitude = vector
        .iter()
        .fold(0.0_f64, |norm, value| norm.hypot(*value));
    (magnitude.is_finite() && magnitude > 1e-12)
        .then(|| (vector.map(|value| value / magnitude), magnitude))
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
        let Some((first_frame, first_length)) = normalize(first_frame) else {
            continue;
        };
        let Some((second_frame, second_length)) = normalize(second_frame) else {
            continue;
        };
        let scale = center
            .iter()
            .chain(conic.start.iter())
            .chain(conic.end.iter())
            .map(|value| value.abs())
            .fold(1.0_f64, f64::max);
        if (first_length - 1.0).abs() > 1e-9
            || (second_length - 1.0).abs() > 1e-9
            || dot(first_frame, second_frame).abs() > 1e-9
        {
            continue;
        }
        let Some((axis, _)) = normalize(cross(first_frame, second_frame)) else {
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
            let (first_direction, first_radius) = normalize(endpoint_deltas[0])?;
            let (_, second_radius) = normalize(endpoint_deltas[1])?;
            ((0..3).all(|index| {
                (endpoint_deltas[0][index] + endpoint_deltas[1][index]).abs() <= 1e-9 * scale
            }) && dot(first_direction, axis).abs() <= 1e-9
                && (first_radius - second_radius).abs() <= 1e-9 * scale)
                .then_some(())?;
            let radius_scale = major_radius.max(1.0);
            if (first_radius - major_radius).abs() <= 1e-9 * radius_scale {
                Some(first_direction)
            } else if (first_radius - minor_radius).abs() <= 1e-9 * radius_scale {
                normalize(cross(first_direction, axis)).map(|(direction, _)| direction)
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
                plane.abs() <= 1e-9 * scale
                    && ((first / first_radius).powi(2) + (second / second_radius).powi(2) - 1.0)
                        .abs()
                        <= 1e-9
            })
        };
        let direct = mapping_is_valid(radii[0], radii[1]);
        let swapped = mapping_is_valid(radii[1], radii[0]);
        let equal_radii = (radii[0] - radii[1]).abs() <= 1e-12 * radii[0].max(radii[1]);
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
            .find(|projection| projection.abs() > 1e-12 * scale);
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
    (0..row.len())
        .filter_map(|start| {
            let mut cursor = crate::psb::Cursor::at(row, start);
            let mut values = Vec::with_capacity(count);
            while values.len() < count {
                values.push(cursor.take_with(|data, pos| coordinate(data, pos, cache))?);
            }
            (cursor.pos() == row.len() && values.iter().all(|value| value.is_finite()))
                .then_some((start, values))
        })
        .min_by_key(|(start, _)| *start)
        .map(|(_, values)| values)
}

fn find_in(data: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    (start <= end && end <= data.len()).then_some(())?;
    data[start..end]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| start + relative)
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

fn conic_local_system(body: &[u8], cache: &ScalarCache) -> Option<[f64; 12]> {
    if let Some(slots) = scalar::decode_explicit_local_system_slots(body, cache) {
        return Some(slots);
    }
    let mut values = Vec::with_capacity(12);
    let mut cursor = 0;
    let frame_coordinate = |offset| {
        scalar::decode_model_reference_coordinate(body, offset, cache)
            .or_else(|| scalar::decode_tabulated_cylinder_frame_coordinate(body, offset, cache))
    };
    while cursor < body.len() && values.len() < 12 {
        if body.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
            values.extend([0.0, 1.0, 0.0]);
            cursor += 2;
            continue;
        }
        if body.get(cursor) == Some(&0x18) && cursor + 1 == body.len() {
            values.push(0.0);
            cursor += 1;
            continue;
        }
        if body.get(cursor) == Some(&0x18) && frame_coordinate(cursor + 1).is_some() {
            values.push(0.0);
            cursor += 1;
            continue;
        }
        let (value, next) = frame_coordinate(cursor)?;
        values.push(value);
        cursor = next;
    }
    (cursor == body.len() && values.len() == 12 && values.iter().all(|value| value.is_finite()))
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
    let (local_end, local_system) = (local_start + 1..=body.len()).find_map(|end| {
        conic_local_system(&body[local_start..end], cache).map(|frame| (end, frame))
    })?;
    let tail = body.get(local_end..)?;
    (tail.is_empty() || tail.first() == Some(&0xe2)).then_some(())?;
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
            && distance > 1e-12
            && stored_length > 0.0
            && (distance - stored_length).abs() <= 1e-9 * scale)
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
                .map_or(block_end, |(next_close, _, _)| *next_close)
                .min(body_start.saturating_add(384));
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
            && (first_distance - radius).abs() <= 1e-9 * scale
            && (second_distance - radius).abs() <= 1e-9 * scale
            && normal_length.is_finite()
            && normal_length > 1e-12 * scale * scale)
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
            && delta[2].abs() <= 1e-10 * scale
            && (diameter - 2.0 * radius).abs() <= 1e-9 * scale)
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
                .map_or(block_end, |(next_close, _, _)| *next_close)
                .min(body_start.saturating_add(256));
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
mod tests {
    use super::*;

    #[test]
    fn decodes_complete_positional_line_rows() {
        let payload = b"ent_list(line)\0\xe0\x02end1\0\xf8\x03\x18\xdf\x1d\x84\xe8\xb0\xed\x7b\x46\x19\x87\x25\xdc\x17\x53\xfa\
            \xe0\x00entity(line)\0\xf1\xe3\xf7\x11\xf6\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x02\
            \x18\xdf\x1d\x84\xe8\xb0\xed\x7b\x2d\x19\x87\x25\xdc\x17\x53\xfa\
            \x18\x2d\x43\x23\xb0\x9d\x16\x1d\xaf\x2d\x19\x87\x25\xdc\x17\x53\xfa\xe3\
            \xe0\x00entity(text)\0";
        let decoded = lines(payload);
        let [line] = decoded.as_slice() else {
            panic!("one line");
        };
        assert_eq!(line.start[0], 0.0);
        assert_eq!(line.end[0], 0.0);
        assert_ne!(line.start, line.end);
    }

    #[test]
    fn decodes_named_conic_fields_without_classifying_the_conic() {
        let local_body = b"\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6";
        assert!(conic_local_system(local_body, &ScalarCache::from_section(local_body)).is_some());
        let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\
            \xe0\x00gen_info\0\xe2\xf7\x13\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\
            \xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02t0\0\x0f\xe0\x02t1\0\x11\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xf2\xf7\x0e\xe3";

        let decoded = named_conics(payload);
        let [conic] = decoded.as_slice() else {
            panic!("one conic");
        };
        assert_eq!(conic.entity_id, 42);
        assert_eq!(conic.type_id, 30);
        assert_eq!(conic.flip, 1);
        assert_eq!(conic.start, [1.0, 0.0, 0.0]);
        assert_eq!(conic.end, [-1.0, 0.0, 0.0]);
        assert_eq!(conic.parameter_start, Some(0.0));
        assert_eq!(conic.parameter_end, Some(std::f64::consts::PI));
        assert_eq!([conic.coefficient_1, conic.coefficient_2], [-1.0, 1.0]);
        assert_eq!(
            conic.local_system,
            Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn named_conic_withholds_duplicate_optional_parameter_fields() {
        let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02t0\0\x0f\xe0\x02t0\0\x0f\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xf2\xf7\x0e\xe3";

        assert!(named_conics(payload).is_empty());
    }

    #[test]
    fn named_conic_opposite_parameter_requires_a_start_parameter() {
        let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02t1\0\x11\xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xf2\xf7\x0e\xe3";

        assert!(named_conics(payload).is_empty());
    }

    #[test]
    fn named_conic_ignores_field_header_bytes_inside_an_ieee_coordinate() {
        let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\x32\xe0\x02c1\0\0\0\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xf2\xf7\x0e\xe3";

        assert_eq!(named_conics(payload).len(), 1);
    }

    #[test]
    fn named_conic_local_system_ignores_a_terminator_inside_an_ieee_coordinate() {
        let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\
            \x32\xf2\xf7\0\0\0\0\0\x0f\x0f\x0f\x0f\x0f\x0f\x0f\x0f\x0f\x0f\x0f\
            \xf2\xf7\x0e\xe3";

        let decoded = named_conics(payload);
        let [conic] = decoded.as_slice() else {
            panic!("one conic")
        };
        assert!(conic.local_system.is_some());
        assert!(conic.body.ends_with(&[0x0f; 11]));
    }

    #[test]
    fn named_conic_withholds_an_ambiguous_local_system_boundary() {
        let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\xaa\xf2\xf7\xbb\xf2\xf7\x0e\xe3";

        assert!(named_conics(payload).is_empty());
    }

    #[test]
    fn conic_frame_accepts_positive_seven_byte_origin_and_terminal_zero() {
        let body = [
            0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x4a, 0, 0, 0, 0, 0, 0, 0x0f,
            0x18,
        ];

        assert_eq!(
            conic_local_system(&body, &ScalarCache::from_section(&body)),
            Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0])
        );
    }

    #[test]
    fn decodes_positional_conic_with_an_opposite_endpoint_parameter() {
        let payload = b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\
            \x2b\x1e\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x01\
            \xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\x0f\x11\x43\xf0\x00\xe4\
            \xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x43\xf0\x00\x0f\x0f\
            \xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0";

        let decoded = positional_conics(payload);
        let [conic] = decoded.as_slice() else {
            panic!("one positional conic");
        };
        assert_eq!(conic.entity_id, 43);
        assert_eq!(conic.type_id, 30);
        assert_eq!(conic.start, [1.0, 0.0, 0.0]);
        assert_eq!(conic.end, [-1.0, 0.0, 0.0]);
        assert_eq!(conic.parameter_start, Some(0.0));
        assert_eq!(conic.parameter_end, Some(std::f64::consts::PI));
        assert_eq!([conic.coefficient_1, conic.coefficient_2], [-1.0, 1.0]);
        assert_eq!(conic.local_system.expect("complete local system")[9], -1.0);
    }

    #[test]
    fn positional_conic_withholds_non_finite_parameters() {
        let payload = b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\
            \x2b\x1e\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x01\
            \xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\
            \xed\x7f\xf8\x00\x00\x00\x00\x00\x00\x11\x43\xf0\x00\xe4\
            \xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x43\xf0\x00\x0f\x0f\
            \xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0";

        assert!(positional_conics(payload).is_empty());
    }

    #[test]
    fn opposite_endpoint_parameter_requires_a_decoded_start_parameter() {
        let body = [0x11];
        assert_eq!(
            conic_parameter(&body, 0, None, &ScalarCache::from_section(&body)),
            Some((None, 1))
        );
    }

    #[test]
    fn derives_ellipse_from_orthonormal_frame_and_non_antipodal_endpoints() {
        let conic = ReferenceConic {
            entity_id: 7,
            type_id: 30,
            flip: 1,
            start: [-3.0, 2.0, 4.0],
            end: [2.0, 4.0, 4.0],
            parameter_start: None,
            parameter_end: None,
            coefficient_1: -5.0,
            coefficient_2: 2.0,
            local_system: Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0]),
            body: Vec::new(),
            offset: 10,
        };

        assert_eq!(
            ellipse_carriers(std::slice::from_ref(&conic)),
            [ReferenceEllipse {
                source_entity_id: 7,
                center: [2.0, 2.0, 4.0],
                axis: [0.0, 0.0, 1.0],
                major_direction: [-1.0, 0.0, 0.0],
                major_radius: 5.0,
                minor_radius: 2.0,
                offset: 10,
            }]
        );

        let mut invalid = conic.clone();
        invalid
            .local_system
            .as_mut()
            .expect("complete local system")[3] = 1.0;
        assert!(ellipse_carriers(&[invalid]).is_empty());

        let diagonal = (100.0_f64 / 29.0).sqrt();
        let ambiguous = ReferenceConic {
            local_system: Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
            start: [diagonal, diagonal, 0.0],
            end: [-diagonal, -diagonal, 0.0],
            ..conic
        };
        assert!(ellipse_carriers(&[ambiguous]).is_empty());
    }

    #[test]
    fn withholds_incomplete_coordinate_suffix() {
        let payload =
            b"ent_list(line)\0\xe0\x00entity(line)\0\xf6\xe2\x02\x18\xe3\xe0\x00entity(text)\0";
        assert!(lines(payload).is_empty());
    }

    #[test]
    fn decodes_signed_coordinate_dictionary_line_rows() {
        let coordinates = b"\x18\x41\x93\x8a\x07\xa0\xe6\xf8\x55\x8c\x3e\x32\xfb\x7f\x13\x0b\
            \x18\x93\x27\x14\x0f\x41\xcd\xf1\x8c\x3e\x32\xfb\x7f\x13\x0b";
        assert!(scalar_suffix(coordinates, 6, &ScalarCache::from_section(coordinates)).is_some());
        let payload = b"ent_list(line)\0\xe0\x00entity(line)\0\xf1\xe3\xf7\x11\
            \xf6\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x02\
            \x18\x41\x93\x8a\x07\xa0\xe6\xf8\x55\x8c\x3e\x32\xfb\x7f\x13\x0b\
            \x18\x93\x27\x14\x0f\x41\xcd\xf1\x8c\x3e\x32\xfb\x7f\x13\x0b\
            \xe0\x00entity(text)\0";
        assert_eq!(lines(payload).len(), 1);
    }

    #[test]
    fn decodes_line3d_with_matching_original_length() {
        let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
            \x0f\x0f\x0f\xe4\x0f\x0f\xe4";
        let decoded = line3d_lines(payload);
        let [line] = decoded.as_slice() else {
            panic!("one line3d");
        };
        assert_eq!(
            line.kind,
            ReferenceLineKind::Line3d {
                entity_id: 35,
                original_length: 1.0
            }
        );
        assert_eq!(line.start, [0.0; 3]);
        assert_eq!(line.end, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn decodes_line3d_with_positive_full_width_coordinates() {
        let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
            \x0f\x0f\x32\xb3\xa2\x70\xe5\xa0\x3f\xfa\
            \xe4\x0f\x32\xb3\xa2\x70\xe5\xa0\x3f\xfa\xe4";
        let decoded = line3d_lines(payload);
        let [line] = decoded.as_slice() else {
            panic!("one line3d");
        };
        assert_eq!(line.start[2], line.end[2]);
        assert_eq!(line.end[0] - line.start[0], 1.0);
    }

    #[test]
    fn withholds_line3d_with_inconsistent_original_length() {
        let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\
            \x0f\x0f\x0f\xe4\x0f\x0f\x0e";
        assert!(line3d_lines(payload).is_empty());
    }

    #[test]
    fn withholds_line3d_when_endpoint_norm_overflows() {
        let mut body = Vec::new();
        for value in [-f64::MAX, 0.0, 0.0, f64::MAX, 0.0, 0.0, f64::MAX] {
            body.push(0xed);
            body.extend_from_slice(&value.to_be_bytes());
        }
        assert!(line3d_fields(&body, &ScalarCache::from_section(&body)).is_none());
    }

    #[test]
    fn line3d_withholds_competing_scalar_runs() {
        let body = b"\x0f\x0f\x0f\xe4\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x0f\x0f\xe4";

        assert!(line3d_fields(body, &ScalarCache::from_section(body)).is_none());
    }

    #[test]
    fn decodes_arc_z_diameter_rows() {
        let body = b"\x01\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f";
        let circle = arc_z_fields(body, &ScalarCache::from_section(body), 7).expect("diameter row");
        assert_eq!(circle.entity_id, 7);
        assert_eq!(circle.center, [0.0; 3]);
        assert_eq!(circle.radius, 1.0);
        assert_eq!(circle.start, [1.0, 0.0, 0.0]);
        assert_eq!(circle.end, [-1.0, 0.0, 0.0]);
    }

    #[test]
    fn decodes_arc_z_explicit_center_rows() {
        let body = b"\x01\x2f\x0c\x00\x2f\x24\x00\x48\x10\x00\
            \x2f\x00\x00\x2f\x16\x00\x2f\x24\x00\x48\x10\x00\
            \x2f\x0c\x00\x2f\x20\x00\x48\x10\x00";
        let circle = arc_z_fields(body, &ScalarCache::from_section(body), 8).expect("quarter arc");
        assert_eq!(circle.center, [3.5, 10.0, -4.0]);
        assert_eq!(circle.radius, 2.0);
        assert_eq!(circle.start, [5.5, 10.0, -4.0]);
        assert_eq!(circle.end, [3.5, 8.0, -4.0]);
    }

    #[test]
    fn decodes_arc_z_positive_full_width_coordinate_rows() {
        let body = b"\x48\x3e\x00\x93\x3b\x57\xbb\x8a\x68\xf5\
            \x8c\x6e\x94\xe1\x50\xe8\xf6\x9a\x54\x2f\x35\xcd\x11\x56\
            \x48\x3e\x00\x2d\x19\x9e\xd7\x77\x97\xfd\xfc\
            \x9b\xa7\x3d\x24\xb6\x7b\x09\x48\x3e\x00\
            \x9f\x6b\xf0\x6f\x95\x50\xb9\xa0\xff\x43\xd5\xa5\xa5\x6c";
        let cache = ScalarCache::from_section(body);
        let circle = arc_z_fields(body, &cache, 9).expect("general arc");
        assert_eq!(circle.center[0], -30.0);
        assert_eq!(circle.start[0], -30.0);
        assert_eq!(circle.end[0], -30.0);
        assert!((circle.axis[0].abs() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn arc_z_withholds_competing_diameter_runs() {
        let body = b"\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f";

        assert!(arc_z_fields(body, &ScalarCache::from_section(body), 7).is_none());
    }
}
