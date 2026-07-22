// SPDX-License-Identifier: Apache-2.0
//! Standard model-space datum planes stored in `ActDatums`.

use crate::scalar;

/// An axis-aligned model-space datum plane.
///
/// The plane comes from an `ActDatums` `act_datum_geoms -> srf_array` row. Its
/// normal is a basis vector and its equation is `x_k = offset` for that axis.
#[derive(Debug, Clone, PartialEq)]
pub struct DatumPlane {
    /// The row's `geom_id`, the datum's identifier in the `ActDatums`
    /// `srf_array` namespace. `ref_planes` nested `plane_id` fields join
    /// this identifier ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#81-scalar-and-datum-tokens)).
    pub id: u32,
    /// Modeling feature identifier from the owning `srf_array.feat_id`.
    pub feature_id: u32,
    /// The plane's unit normal, one of the three standard basis vectors.
    pub normal: [f64; 3],
    /// The plane's model-space offset along the axis identified by
    /// `normal`: the constant coordinate shared by both `outline` corners.
    pub offset: f64,
    /// The row's two `outline` corner points, in model-space XYZ.
    pub corners: [[Option<f64>; 3]; 2],
    /// Byte offset of the row's `outline` field in the original stream.
    pub offset_in_payload: usize,
}

/// Decode datum rows whose outline corners share one coordinate.
///
/// This promotion applies only to model-space `ActDatums` outlines.
pub fn planes(payload: &[u8]) -> Vec<DatumPlane> {
    let mut result = Vec::new();
    for offset in 0..payload.len().saturating_sub(6) {
        let id = payload[offset];
        if id == 0 || id > 0x40 || payload.get(offset + 1) != Some(&0x22) {
            continue;
        }
        if !matches!(payload.get(offset + 3), Some(0x01 | 0xf6)) {
            continue;
        }
        if !matches!(payload.get(offset + 4), Some(0 | 1 | 6 | 0xf6)) {
            continue;
        }
        let Some(values) = datum_slots(payload, offset + 6, 10) else {
            continue;
        };
        let outline = &values[4..];
        let equal = [
            slot_equal(&outline[0], &outline[3]),
            slot_equal(&outline[1], &outline[4]),
            slot_equal(&outline[2], &outline[5]),
        ];
        let held = equal
            .iter()
            .enumerate()
            .filter_map(|(axis, equal)| (*equal == Some(true)).then_some(axis))
            .collect::<Vec<_>>();
        let Some(axis) = held.first() else {
            continue;
        };
        let Some(plane_offset) = outline[*axis].value else {
            continue;
        };
        let mut normal = [0.0; 3];
        normal[*axis] = 1.0;
        result.push(DatumPlane {
            id: id as u32,
            feature_id: payload[offset + 2] as u32,
            normal,
            offset: plane_offset,
            corners: [
                [outline[0].value, outline[1].value, outline[2].value],
                [outline[3].value, outline[4].value, outline[5].value],
            ],
            offset_in_payload: offset,
        });
    }
    result
}

/// Decode a named datum from its matching outline coordinates.
pub fn named_plane(payload: &[u8]) -> Option<DatumPlane> {
    let marker = b"outline\0\xf9\x02\x03";
    let outline = find(payload, marker, 0)?;
    let id_marker = b"\xe0\x01geom_id\0";
    let id_at = payload[..outline]
        .windows(id_marker.len())
        .rposition(|window| window == id_marker)?;
    let id_start = id_at + id_marker.len();
    let id = *payload.get(id_start)? as u32;
    let feature_marker = b"feat_id\0";
    let feature_at = payload[..outline]
        .windows(feature_marker.len())
        .rposition(|window| window == feature_marker)?;
    let feature_id = *payload.get(feature_at + feature_marker.len())? as u32;
    let cache = scalar::ScalarCache::from_section(payload);
    let slots = named_outline_slots(payload, outline + marker.len(), &cache)?;
    let standalone_zero = |slot: &DatumSlot| matches!(slot.token.as_slice(), [0x18 | 0x0f]);
    let zero_axis =
        (0..3).find(|axis| standalone_zero(&slots[*axis]) && standalone_zero(&slots[*axis + 3]));
    let held = (0..3)
        .filter(|axis| slot_equal(&slots[*axis], &slots[*axis + 3]) == Some(true))
        .collect::<Vec<_>>();
    let axis = match (zero_axis, held.as_slice()) {
        (Some(axis), _) => axis,
        (None, [axis]) => *axis,
        _ => return None,
    };
    let offset = slots[axis].value?;
    let mut normal = [0.0; 3];
    normal[axis] = 1.0;
    Some(DatumPlane {
        id,
        feature_id,
        normal,
        offset,
        corners: [
            [slots[0].value, slots[1].value, slots[2].value],
            [slots[3].value, slots[4].value, slots[5].value],
        ],
        offset_in_payload: outline,
    })
}

fn find(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

/// Decode one named-outline slot token at `offset`, given the number of slots
/// already filled. Returns the slot value and the offset past the token;
/// `None` aborts the walk.
///
/// - `18`: an in-lane scalar, or a one-byte zero marker when the following
///   byte opens a slot or exactly five slots are already filled.
/// - `0f`/`e6`: a one-byte zero marker.
/// - `41`: a seven-byte tail forming the IEEE double `3f XX..`.
/// - `46`/`2d`: a world-coordinate scalar.
/// - `40..=bf`/`d3`/`d7`/`df`: a seven-byte in-lane scalar whose value is kept
///   only when the scalar decode consumes exactly seven bytes, otherwise a
///   valueless seven-byte token.
fn decode_outline_slot(
    data: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
    filled: usize,
) -> Option<(Option<f64>, usize)> {
    let head = *data.get(offset)?;
    match head {
        0x18 => {
            let next_is_slot = matches!(
                data.get(offset + 1),
                Some(0x0f | 0x18 | 0x2d | 0x40..=0xbf | 0xd3 | 0xd7 | 0xdf)
            );
            let (value, next) = scalar::decode_in_lane(data, offset, cache)
                .or_else(|| next_is_slot.then_some((0.0, offset + 1)))
                .or_else(|| (filled == 5).then_some((0.0, offset + 1)))?;
            Some((Some(value), next))
        }
        0x0f | 0xe6 => Some((Some(0.0), offset + 1)),
        0x41 => {
            let tail = data.get(offset + 1..offset + 8)?;
            let mut raw = [0; 8];
            raw[0] = 0x3f;
            raw[1..].copy_from_slice(tail);
            Some((Some(f64::from_be_bytes(raw)), offset + 8))
        }
        0x46 | 0x2d => {
            let (value, next) = scalar::decode(data, offset)?;
            Some((Some(value), next))
        }
        0x40..=0xbf | 0xd3 | 0xd7 | 0xdf => {
            let next = offset + 7;
            data.get(offset..next)?;
            let value = scalar::decode(data, offset)
                .filter(|(_, decoded_end)| *decoded_end == next)
                .map(|(value, _)| value);
            Some((value, next))
        }
        _ => None,
    }
}

fn named_outline_slots(
    data: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<DatumSlot>> {
    let mut slots = Vec::with_capacity(6);
    let mut cursor = crate::psb::Cursor::at(data, offset);
    while slots.len() < 6 {
        let start = cursor.pos();
        let filled = slots.len();
        let value = cursor.take_with(|data, pos| decode_outline_slot(data, pos, cache, filled))?;
        slots.push(DatumSlot {
            value,
            token: data[start..cursor.pos()].to_vec(),
        });
    }
    Some(slots)
}

#[derive(Debug)]
struct DatumSlot {
    value: Option<f64>,
    token: Vec<u8>,
}

/// Decode one datum-slot token at `offset`, returning its value (`None` for
/// the seven-byte valueless sentinels) and the offset past the token; a `None`
/// return aborts the walk.
///
/// - `18`/`0f`/`e6`: a one-byte zero marker.
/// - `41`: a seven-byte tail forming the IEEE double `3f XX..`.
/// - `73`/`9f`/`a5`/`bb`: a seven-byte valueless sentinel.
/// - otherwise: a generic scalar in the datum lane.
fn decode_datum_slot(data: &[u8], offset: usize) -> Option<(Option<f64>, usize)> {
    let head = *data.get(offset)?;
    match head {
        0x18 | 0x0f | 0xe6 => Some((Some(0.0), offset + 1)),
        0x41 => {
            let tail = data.get(offset + 1..offset + 8)?;
            let mut raw = [0; 8];
            raw[0] = 0x3f;
            raw[1..].copy_from_slice(tail);
            Some((Some(f64::from_be_bytes(raw)), offset + 8))
        }
        0x73 | 0x9f | 0xa5 | 0xbb => {
            let next = offset + 7;
            data.get(offset..next)?;
            Some((None, next))
        }
        _ => {
            let (value, next) = scalar::decode(data, offset)?;
            Some((Some(value), next))
        }
    }
}

fn datum_slots(data: &[u8], offset: usize, count: usize) -> Option<Vec<DatumSlot>> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = crate::psb::Cursor::at(data, offset);
    while slots.len() < count {
        let start = cursor.pos();
        let value = cursor.take_with(decode_datum_slot)?;
        slots.push(DatumSlot {
            value,
            token: data.get(start..cursor.pos())?.to_vec(),
        });
    }
    Some(slots)
}

fn slot_equal(first: &DatumSlot, second: &DatumSlot) -> Option<bool> {
    match (first.value, second.value) {
        (Some(first), Some(second)) => {
            let scale = first.abs().max(second.abs()).max(1.0);
            Some((first - second).abs() <= 1e-9 * scale)
        }
        (None, None) => Some(first.token == second.token),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    fn ieee8(value: f64) -> Vec<u8> {
        let mut raw = value.to_be_bytes();
        raw[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
        raw.to_vec()
    }
    #[test]
    fn decodes_constant_outline_coordinate_as_a_model_plane() {
        let mut data = vec![4, 0x22, 1, 1, 0, 0];
        data.extend([0x0f; 4]);
        data.extend(ieee8(2.0));
        data.push(0x0f);
        data.extend(ieee8(3.0));
        data.extend(ieee8(-2.0));
        data.push(0x0f);
        data.extend(ieee8(-3.0));
        assert_eq!(
            planes(&data),
            vec![DatumPlane {
                id: 4,
                feature_id: 1,
                normal: [0.0, 1.0, 0.0],
                offset: 0.0,
                corners: [
                    [Some(2.0), Some(0.0), Some(3.0)],
                    [Some(-2.0), Some(0.0), Some(-3.0)]
                ],
                offset_in_payload: 0
            }]
        );
    }

    #[test]
    fn decodes_named_standard_plane_from_zero_slots() {
        let data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03\x18\x46\x08\0\0\0\0\0\0\x18\x18\x46\x08\0\0\0\0\0\0\x18";
        let plane = named_plane(data).unwrap();
        assert_eq!(plane.id, 2);
        assert_eq!(plane.feature_id, 1);
        assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
        assert_eq!(plane.offset, 0.0);
        assert_eq!(
            plane.corners,
            [
                [Some(0.0), Some(3.0), Some(0.0)],
                [Some(0.0), Some(3.0), Some(0.0)]
            ]
        );
    }

    #[test]
    fn named_outline_41_form_occupies_eight_bytes() {
        let data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03\x18\x41\xba\x13\x99\xa9\xb3\xd8\x74\x41\x94\xad\x7e\x6a\xb0\x34\x5e\x18\x93\x29\x5a\xfc\xd5\x60\x69\x8c\x40\x79\xe9\x12\xa5\x83";
        let plane = named_plane(data).expect("named plane");
        assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
        assert_eq!(plane.corners[0][0], Some(0.0));
        assert_eq!(plane.corners[1][0], Some(0.0));
    }

    #[test]
    fn positional_outline_preserves_opaque_seven_byte_slots() {
        let a5 = [0xa5, 1, 2, 3, 4, 5, 6];
        let nine_f = [0x9f, 7, 8, 9, 10, 11, 12];
        let mut data = vec![4, 0x22, 3, 1, 1, 0];
        data.extend(a5);
        data.extend(ieee8(2.0));
        data.extend(nine_f);
        data.extend(ieee8(-2.0));
        data.extend(ieee8(3.0));
        data.push(0x18);
        data.extend(a5);
        data.extend(ieee8(-3.0));
        data.push(0x18);
        data.extend(nine_f);

        assert_eq!(planes(&data)[0].normal, [0.0, 1.0, 0.0]);
        assert_eq!(planes(&data)[0].offset, 0.0);
    }

    #[test]
    fn named_outline_resolves_a_cache_indexed_nonzero_offset() {
        let cached = ieee8(2.5);
        let mut data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01".to_vec();
        data.extend(&cached);
        data.extend(b"outline\0\xf9\x02\x03");
        data.extend([0x18, 0x00]);
        data.extend(ieee8(-3.0));
        data.extend(ieee8(-4.0));
        data.extend([0x18, 0x00]);
        data.extend(ieee8(3.0));
        data.extend(ieee8(4.0));

        let plane = named_plane(&data).expect("cache-indexed named plane");
        assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
        assert_eq!(plane.offset, 2.5);
        assert_eq!(plane.corners[0], [Some(2.5), Some(-3.0), Some(-4.0)]);
        assert_eq!(plane.corners[1], [Some(2.5), Some(3.0), Some(4.0)]);
    }
}
