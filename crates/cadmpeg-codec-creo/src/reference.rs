// SPDX-License-Identifier: Apache-2.0
//! Model-space reference entities from `MdlRefInfo`.

use crate::scalar::{self, ScalarCache};

/// Stored reference-line family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceLineKind {
    /// Planar `entity(line)` record.
    Line,
    /// Spatial `line3d` record with a stored original length.
    Line3d,
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
            let mut cursor = start;
            let mut values = Vec::with_capacity(count);
            while values.len() < count {
                let (value, next) = coordinate(row, cursor, cache)?;
                values.push(value);
                cursor = next;
            }
            (cursor == row.len() && values.iter().all(|value| value.is_finite()))
                .then_some((start, values))
        })
        .min_by_key(|(start, _)| *start)
        .map(|(_, values)| values)
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
            } else if payload.get(end) == Some(&0xe3) {
                end
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

fn line3d_fields(body: &[u8], cache: &ScalarCache) -> Option<([f64; 3], [f64; 3])> {
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
            .then_some((start, first, second))
    });
    let (_, first, second) = candidates.min_by_key(|(start, _, _)| *start)?;
    Some((first, second))
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
            headers.push((close, body_start));
        }
        for (index, (close, body_start)) in headers.iter().copied().enumerate() {
            let body_end = headers
                .get(index + 1)
                .map_or(block_end, |(next_close, _)| *next_close)
                .min(body_start.saturating_add(384));
            let Some((start, end)) = line3d_fields(&payload[body_start..body_end], &cache) else {
                continue;
            };
            result.push(ReferenceLine {
                kind: ReferenceLineKind::Line3d,
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

fn arc_z_fields(body: &[u8], cache: &ScalarCache) -> Option<ReferenceCircle> {
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
            center,
            center_stored: true,
            radius,
            axis,
            start: first,
            end: second,
            offset: start,
        })
    });
    if let Some(circle) = explicit.min_by_key(|circle| circle.offset) {
        return Some(circle);
    }
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
                center,
                center_stored: false,
                radius,
                axis: [0.0, 0.0, 1.0],
                start: first,
                end: second,
                offset: start,
            })
    });
    diametric.min_by_key(|circle| circle.offset)
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
            headers.push((close, body_start + 1));
        }
        for (index, (close, body_start)) in headers.iter().copied().enumerate() {
            let body_end = headers
                .get(index + 1)
                .map_or(block_end, |(next_close, _)| *next_close)
                .min(body_start.saturating_add(256));
            let Some(mut circle) = arc_z_fields(&payload[body_start..body_end], &cache) else {
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
        assert_eq!(line.kind, ReferenceLineKind::Line3d);
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
    fn decodes_arc_z_diameter_rows() {
        let body = b"\x01\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f";
        let circle = arc_z_fields(body, &ScalarCache::from_section(body)).expect("diameter row");
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
        let circle = arc_z_fields(body, &ScalarCache::from_section(body)).expect("quarter arc");
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
        let circle = arc_z_fields(body, &cache).expect("general arc");
        assert_eq!(circle.center[0], -30.0);
        assert_eq!(circle.start[0], -30.0);
        assert_eq!(circle.end[0], -30.0);
        assert!((circle.axis[0].abs() - 1.0).abs() < 1e-12);
    }
}
