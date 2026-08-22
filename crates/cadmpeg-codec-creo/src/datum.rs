// SPDX-License-Identifier: Apache-2.0
//! Standard model-space datum planes stored in `ActDatums`.

use cadmpeg_core::bytes::find_from as find;

use crate::scalar;
use crate::surface::{PositionalCylinderFrame, SurfaceKind, SurfaceParameterRecord, SurfaceRow};

const EPS_ACTIVE_CYLINDER_RELATIVE: f64 = 1e-9;
const EPS_ACTIVE_CYLINDER_MIN: f64 = 1e-12;

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
    /// Byte offset of the row's `geom_id` field in the original stream.
    pub offset_in_payload: usize,
}

/// A complete model-space cylinder stored in an `ActDatums` `srf_array` row.
///
/// Active datum geometry uses a bounded type-24 envelope in the `ActDatums`
/// namespace. The native topology can reference these rows as face surfaces,
/// so their source namespace and orientation must survive the container scan
/// instead of being inferred later from visible rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatumCylinder {
    /// The row's `geom_id` in the `ActDatums` surface namespace.
    pub id: u32,
    /// Modeling feature identifier from the owning `srf_array.feat_id`.
    pub feature_id: u32,
    /// Native row orientation: `true` when the row stores `0xf6`.
    pub reversed: bool,
    /// Complete model-space cylinder carrier decoded from the row body.
    pub frame: PositionalCylinderFrame,
    /// Byte offset of the row's `geom_id` field in the original stream.
    pub offset_in_payload: usize,
}

/// Decode datum rows whose outline corners share one coordinate.
///
/// This promotion applies only to model-space `ActDatums` outlines.
pub fn planes(payload: &[u8]) -> Vec<DatumPlane> {
    let rows = crate::surface::counted_row_bounds(payload);
    let cache = scalar::ScalarCache::from_section(payload);
    rows.iter()
        .enumerate()
        .filter(|(_, (row, _))| {
            row.id != 0
                && row.kind == SurfaceKind::Plane
                && row.boundary_type == 0x01
                && row.next_surface == 0
        })
        .filter_map(|(index, (row, frame_end))| {
            let row_end = rows
                .get(index + 1)
                .map_or(*frame_end, |(next, _)| (*frame_end).min(next.offset));
            positional_plane(payload, row, row_end, &cache)
        })
        .collect()
}

/// Decode complete cylinder carriers from active-datum surface rows.
///
/// A row is promoted only when its identifier and parameter body are unique in
/// the namespace and one complete, valid positional or active-envelope frame
/// is proved. This keeps unrelated scalar-shaped bytes and ambiguous duplicate
/// rows out of the native surface join.
pub fn cylinders(payload: &[u8]) -> Vec<DatumCylinder> {
    let rows = crate::surface::rows(payload);
    let parameters = crate::surface::parameter_records(payload);
    rows.iter()
        .filter(|row| row.id != 0 && row.kind == SurfaceKind::Cylinder)
        .filter_map(|row| {
            let parameter = crate::surface::unique_surface_parameter(&parameters, row.id)?;
            (parameter.offset == row.offset).then_some(())?;
            Some(DatumCylinder {
                id: row.id,
                feature_id: row.feature_id,
                reversed: row.reversed,
                frame: parameter
                    .positional_cylinder_frame
                    .or_else(|| active_cylinder_frame(row, parameter))?,
                offset_in_payload: row.offset,
            })
        })
        .collect()
}

/// Decode the bounded active-datum cylinder envelope used by type-24 rows.
///
/// The terminal seven-slot frame stores one signed axial span followed by two
/// opposite envelope corners. A preceding scalar frame may carry the signed
/// span in split forms. The three corner-coordinate spans are a diameter, the
/// axial length, and one radius; their 2:1:1 relationship is the admission
/// invariant. The second corner is the oriented axial end and the first
/// corner supplies the held radial coordinate.
fn active_cylinder_frame(
    row: &SurfaceRow,
    parameter: &SurfaceParameterRecord,
) -> Option<PositionalCylinderFrame> {
    (row.type_byte == 0x24 && matches!(row.boundary_type, 0x00 | 0x01)).then_some(())?;
    let terminal = parameter.terminal_scalar_frame.as_ref()?;
    let [length_slot, corner0, corner1, corner2, corner3, corner4, corner5] =
        terminal.slots.as_slice()
    else {
        return None;
    };
    let terminal_values = [
        length_slot.value?,
        corner0.value?,
        corner1.value?,
        corner2.value?,
        corner3.value?,
        corner4.value?,
        corner5.value?,
    ];
    terminal_values
        .into_iter()
        .all(f64::is_finite)
        .then_some(())?;
    let preceding_values = parameter
        .scalar_frames
        .iter()
        .take(parameter.scalar_frames.len().saturating_sub(1))
        .flat_map(|frame| frame.slots.iter().filter_map(|slot| slot.value));
    let lengths = std::iter::once(length_slot.value?).chain(preceding_values);
    let corners = [
        [terminal_values[1], terminal_values[2], terminal_values[3]],
        [terminal_values[4], terminal_values[5], terminal_values[6]],
    ];
    let spans =
        std::array::from_fn::<_, 3, _>(|index| (corners[1][index] - corners[0][index]).abs());
    let scale = terminal_values
        .into_iter()
        .chain(spans)
        .map(f64::abs)
        .fold(1.0, f64::max);
    let close =
        |first: f64, second: f64| (first - second).abs() <= EPS_ACTIVE_CYLINDER_RELATIVE * scale;
    let mut candidates = Vec::new();
    for signed_length in lengths {
        if !signed_length.is_finite() || signed_length == 0.0 {
            continue;
        }
        let length = signed_length.abs();
        let axis_indices = (0..3)
            .filter(|index| close(spans[*index], length))
            .collect::<Vec<_>>();
        let [axis_index] = axis_indices.as_slice() else {
            continue;
        };
        let radial_indices = (0..3)
            .filter(|index| *index != *axis_index)
            .collect::<Vec<_>>();
        let [first_radial, second_radial] = radial_indices.as_slice() else {
            continue;
        };
        let (diameter_index, radius_index) =
            if close(spans[*first_radial], 2.0 * spans[*second_radial]) {
                (*first_radial, *second_radial)
            } else if close(spans[*second_radial], 2.0 * spans[*first_radial]) {
                (*second_radial, *first_radial)
            } else {
                continue;
            };
        let radius = spans[diameter_index] * 0.5;
        if radius <= EPS_ACTIVE_CYLINDER_MIN * scale
            || spans[radius_index] <= EPS_ACTIVE_CYLINDER_MIN * scale
        {
            continue;
        }
        let mut origin = [0.0; 3];
        origin[diameter_index] =
            f64::midpoint(corners[0][diameter_index], corners[1][diameter_index]);
        origin[*axis_index] = corners[1][*axis_index];
        origin[radius_index] = corners[0][radius_index];
        let mut axis = [0.0; 3];
        axis[*axis_index] = (corners[0][*axis_index] - corners[1][*axis_index]).signum();
        let orientation = if signed_length.is_sign_negative() {
            -1.0
        } else {
            1.0
        } * if row.reversed { -1.0 } else { 1.0 };
        let mut ref_direction = [0.0; 3];
        ref_direction[diameter_index] =
            orientation * (corners[1][diameter_index] - corners[0][diameter_index]).signum();
        let candidate = PositionalCylinderFrame {
            origin,
            axis,
            ref_direction,
            radius,
            length: Some(length),
        };
        if !candidates
            .iter()
            .any(|existing| active_cylinder_frames_agree(*existing, candidate))
        {
            candidates.push(candidate);
        }
    }
    let first = candidates.first().copied()?;
    (candidates.len() == 1).then_some(first)
}

fn active_cylinder_frames_agree(
    first: PositionalCylinderFrame,
    second: PositionalCylinderFrame,
) -> bool {
    let scale = first
        .origin
        .into_iter()
        .chain(second.origin)
        .chain([first.radius, second.radius])
        .chain(first.length)
        .chain(second.length)
        .map(f64::abs)
        .fold(1.0, f64::max);
    let close =
        |left: f64, right: f64| (left - right).abs() <= EPS_ACTIVE_CYLINDER_RELATIVE * scale;
    first
        .origin
        .into_iter()
        .zip(second.origin)
        .all(|(left, right)| close(left, right))
        && first
            .axis
            .into_iter()
            .zip(second.axis)
            .all(|(left, right)| close(left, right))
        && first
            .ref_direction
            .into_iter()
            .zip(second.ref_direction)
            .all(|(left, right)| close(left, right))
        && close(first.radius, second.radius)
        && match (first.length, second.length) {
            (Some(left), Some(right)) => close(left, right),
            (None, None) => true,
            _ => false,
        }
}

fn positional_plane(
    payload: &[u8],
    row: &SurfaceRow,
    row_end: usize,
    cache: &scalar::ScalarCache,
) -> Option<DatumPlane> {
    let id_start = row.offset;
    if payload.get(id_start).copied()? > 0xbf {
        return None;
    }
    let (_, after_id) = crate::psb::compact_int(payload, id_start);
    if payload.get(after_id) != Some(&0x22) {
        return None;
    }
    let (_, after_feature) = crate::psb::compact_int(payload, after_id + 1);
    let body_start = crate::psb::compact_int(payload, after_feature + 2).1;
    let values = datum_slots(payload, body_start, 10, row_end, cache)?;
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
    let [axis] = held.as_slice() else {
        return None;
    };
    let plane_offset = outline[*axis].value?;
    let mut normal = [0.0; 3];
    normal[*axis] = 1.0;
    Some(DatumPlane {
        id: row.id,
        feature_id: row.feature_id,
        normal,
        offset: plane_offset,
        corners: [
            [outline[0].value, outline[1].value, outline[2].value],
            [outline[3].value, outline[4].value, outline[5].value],
        ],
        offset_in_payload: id_start,
    })
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
    let feature_marker = b"feat_id\0";
    let feature_at = payload[..outline]
        .windows(feature_marker.len())
        .rposition(|window| window == feature_marker)?;
    let feature_field_start = feature_at.checked_sub(2)?;
    (payload.get(feature_field_start) == Some(&crate::psb::token::NAMED_RECORD)).then_some(())?;
    let outline_field_start = outline
        .checked_sub(2)
        .filter(|start| payload.get(*start) == Some(&crate::psb::token::NAMED_RECORD))
        .unwrap_or(outline);
    let (id, id_end) = crate::psb::reference_id(payload, id_start).ok()?;
    (id_end <= feature_field_start).then_some(())?;
    let feature_start = feature_at + feature_marker.len();
    let (feature_id, feature_end) = crate::psb::reference_id(payload, feature_start).ok()?;
    (feature_end <= outline_field_start).then_some(())?;
    let cache = scalar::ScalarCache::from_section(payload);
    let slots = named_outline_slots(payload, outline + marker.len(), &cache)?;
    let standalone_zero = |slot: &DatumSlot| matches!(slot.token.as_slice(), [0x18 | 0x0f]);
    let zero_axes = (0..3)
        .filter(|axis| standalone_zero(&slots[*axis]) && standalone_zero(&slots[*axis + 3]))
        .collect::<Vec<_>>();
    let held = (0..3)
        .filter(|axis| slot_equal(&slots[*axis], &slots[*axis + 3]) == Some(true))
        .collect::<Vec<_>>();
    let axis = match (zero_axes.as_slice(), held.as_slice()) {
        ([axis], _) => *axis,
        ([], [axis]) => *axis,
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

/// Decode one named-outline slot token at `offset`, given the number of slots
/// already filled. Returns the slot value and the offset past the token;
/// `None` aborts the walk.
///
/// - `18`: an in-lane scalar, or a one-byte zero marker when the following
///   byte opens a slot or exactly five slots are already filled.
/// - `0f`/`e6`: a one-byte zero marker.
/// - `41`: a seven-byte tail forming the IEEE double `3f XX..`.
/// - `46`/`2d`: a world-coordinate scalar.
/// - Datum-outline DICT prefixes use the model-coordinate lane in
///   `scalar::decode_datum_outline_coordinate`.
/// - `45`/`5c` retain a seven-byte token with an unresolved value.
/// - Other `40..=bf`/`d3`/`d7`/`df` prefixes retain a seven-byte token whose
///   value is kept only when the generic scalar decode consumes exactly seven
///   bytes; otherwise the token remains valueless.
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
        _ => {
            if let Some((value, next)) =
                scalar::decode_datum_outline_coordinate(data, offset, cache)
            {
                return Some((Some(value), next));
            }
            let head = *data.get(offset)?;
            if matches!(head, 0x45 | 0x5c) {
                let next = offset + 7;
                data.get(offset..next)?;
                return Some((None, next));
            }
            if !matches!(head, 0x40..=0xbf | 0xd3 | 0xd7 | 0xdf) {
                return None;
            }
            let next = offset + 7;
            data.get(offset..next)?;
            let value = scalar::decode(data, offset)
                .filter(|(_, decoded_end)| *decoded_end == next)
                .map(|(value, _)| value);
            Some((value, next))
        }
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
/// - `45`/`5c`: a seven-byte token whose numeric value is unresolved.
/// - other tokens: a coordinate in the bounded datum-outline lane.
fn decode_datum_slot(
    data: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<(Option<f64>, usize)> {
    let head = *data.get(offset)?;
    match head {
        0x18 | 0x0f | 0xe6 => Some((Some(0.0), offset + 1)),
        0x45 | 0x5c => {
            let next = offset + 7;
            data.get(offset..next)?;
            Some((None, next))
        }
        _ => scalar::decode_datum_outline_coordinate(data, offset, cache)
            .map(|(value, next)| (Some(value), next)),
    }
}

fn datum_slots(
    data: &[u8],
    offset: usize,
    count: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<DatumSlot>> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = offset;
    while slots.len() < count {
        let start = cursor;
        let (value, next) = decode_datum_slot(data, cursor, cache)?;
        if next > end {
            return None;
        }
        slots.push(DatumSlot {
            value,
            token: data.get(start..next)?.to_vec(),
        });
        cursor = next;
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
mod tests;
