// SPDX-License-Identifier: Apache-2.0
//! Standard model-space datum planes stored in `ActDatums`.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::scalar;
use crate::surface::cylinder_frame_readers::positional_cylinder_frames_agree;
use crate::surface::{PositionalCylinderFrame, SurfaceKind, SurfaceParameterRecord, SurfaceRow};

const EPS_ACTIVE_CYLINDER_RELATIVE: f64 = 1.0e-9;
const EPS_ACTIVE_CYLINDER_MIN: f64 = 1.0e-12;

const EPS_DATUM_COORDINATE_AGREEMENT: f64 = 1.0e-9;

use crate::axis::Axis;
use cadmpeg_ir::scalar::FiniteReal;

/// An axis-aligned model-space datum plane with equation `x_axis = offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DatumPlane {
    /// Axis normal to the plane.
    pub(crate) axis: Axis,
    /// Constant coordinate along the normal axis.
    offset: FiniteReal,
}

impl DatumPlane {
    pub(crate) fn new(axis: Axis, offset: f64) -> Option<Self> {
        Some(Self {
            axis,
            offset: FiniteReal::new(offset)?,
        })
    }
    pub(crate) fn offset(self) -> f64 {
        self.offset.get()
    }
    /// The positive unit basis vector normal to the plane.
    pub(crate) fn normal(self) -> [f64; 3] {
        match self.axis {
            Axis::X => [1.0, 0.0, 0.0],
            Axis::Y => [0.0, 1.0, 0.0],
            Axis::Z => [0.0, 0.0, 1.0],
        }
    }
}

/// Source identity and outline for one decoded `ActDatums` plane.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DatumPlaneRecord {
    /// The row's `geom_id`, joined by nested `ref_planes.plane_id` fields.
    pub(crate) id: u32,
    /// Modeling feature identifier from the owning `srf_array.feat_id`.
    pub(crate) feature_id: u32,
    /// Plane defined by the shared outline coordinate.
    plane: DatumPlane,
    /// Second outline corner's coordinate along the plane normal.
    opposite_offset: FiniteReal,
    /// Corner coordinates on the remaining axes, in XYZ order.
    in_plane_corners: [[Option<FiniteReal>; 2]; 2],
    /// Byte offset of the row's `geom_id` field in the original stream.
    pub(crate) offset_in_payload: usize,
}

impl DatumPlaneRecord {
    pub(crate) fn new(
        id: u32,
        feature_id: u32,
        plane: DatumPlane,
        opposite_offset: f64,
        corners: [[Option<f64>; 2]; 2],
        offset_in_payload: usize,
    ) -> Option<Self> {
        let opposite_offset = FiniteReal::new(opposite_offset)?;
        let scale = plane
            .offset()
            .abs()
            .max(opposite_offset.get().abs())
            .max(1.0);
        if (plane.offset() - opposite_offset.get()).abs() > EPS_DATUM_COORDINATE_AGREEMENT * scale {
            return None;
        }
        let mut in_plane_corners = [[None; 2]; 2];
        for (i, row) in corners.into_iter().enumerate() {
            for (j, value) in row.into_iter().enumerate() {
                in_plane_corners[i][j] = match value {
                    Some(value) => Some(FiniteReal::new(value)?),
                    None => None,
                };
            }
        }
        Some(Self {
            id,
            feature_id,
            plane,
            opposite_offset,
            in_plane_corners,
            offset_in_payload,
        })
    }
    pub(crate) fn plane(&self) -> DatumPlane {
        self.plane
    }
    /// The two outline corners in model-space XYZ.
    pub(crate) fn corners(&self) -> [[Option<f64>; 3]; 2] {
        let [u, v] = self.plane.axis.complement().map(Axis::index);
        let offsets = [self.plane.offset(), self.opposite_offset.get()];
        std::array::from_fn(|index| {
            let mut corner = [None; 3];
            corner[self.plane.axis.index()] = Some(offsets[index]);
            corner[u] = self.in_plane_corners[index][0].map(FiniteReal::get);
            corner[v] = self.in_plane_corners[index][1].map(FiniteReal::get);
            corner
        })
    }
}

/// A complete model-space cylinder stored in an `ActDatums` `srf_array` row.
///
/// Active datum geometry uses a bounded type-24 envelope in the `ActDatums`
/// namespace. The native topology can reference these rows as face surfaces,
/// so their source namespace and orientation must survive the container scan
/// instead of being inferred later from visible rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DatumCylinder {
    /// The row's `geom_id` in the `ActDatums` surface namespace.
    pub(crate) id: u32,
    /// Modeling feature identifier from the owning `srf_array.feat_id`.
    pub(crate) feature_id: u32,
    /// Native row orientation: `true` when the row stores `0xf6`.
    pub(crate) reversed: bool,
    /// Complete model-space cylinder carrier decoded from the row body.
    pub(crate) frame: PositionalCylinderFrame,
    /// Byte offset of the row's `geom_id` field in the original stream.
    pub(crate) offset_in_payload: usize,
}

/// Decode datum rows whose outline corners share one coordinate.
///
/// This promotion applies only to model-space `ActDatums` outlines.
pub(crate) fn planes(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<DatumPlaneRecord>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo datum row scratch")?;
    let rows = scratch.with_storage(|| crate::surface::counted_row_bounds(ctx, payload))?;
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let mut planes = Vec::new();
    for (index, (row, frame_end)) in ctx.admit_iter(&rows, "creo datum plane rows")?.enumerate().filter(|(_, (row, _))| {
        row.id != 0
            && row.kind == SurfaceKind::Plane
            && row.boundary_type == crate::surface::BoundaryType::Code01
            && row.next_surface == 0
    }) {
        let row_end = rows
            .get(index + 1)
            .map_or(*frame_end, |(next, _)| (*frame_end).min(next.offset));
        if let Some(plane) = positional_plane(ctx, payload, row, row_end, &cache)? {
            ctx.reserve_vec(&mut planes, 1, "creo datum plane records")?;
            planes.push(plane);
        }
    }
    Ok(planes)
}

/// Decode complete cylinder carriers from active-datum surface rows.
///
/// A row is promoted only when its identifier and parameter body are unique in
/// the namespace and one complete, valid positional or active-envelope frame
/// is proved. This keeps unrelated scalar-shaped bytes and ambiguous duplicate
/// rows out of the native surface join.
pub(crate) fn cylinders(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<DatumCylinder>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo datum cylinder scratch")?;
    let rows = scratch.with_storage(|| crate::surface::rows(ctx, payload))?;
    let parameters = scratch.with_storage(|| crate::surface::SurfaceParameters::new(
        ctx,
        crate::surface::parameter_records_for_rows(ctx, payload, &rows)?,
        "creo datum cylinder parameter index",
    ))?;
    let mut cylinders = Vec::new();
    for row in ctx
        .admit_iter(&rows, "creo datum cylinder rows")?
        .filter(|row| row.id != 0 && row.kind == SurfaceKind::Cylinder)
    {
        let Some(parameter) = crate::surface::unique_surface_parameter(&parameters, row.id) else {
            continue;
        };
        if parameter.offset != row.offset {
            continue;
        }
        let frame = match parameter.positional_cylinder_frame() {
            Some(frame) => Some(frame),
            None => active_cylinder_frame(ctx, row, parameter)?,
        };
        if let Some(frame) = frame {
            ctx.reserve_vec(&mut cylinders, 1, "creo datum cylinders")?;
            cylinders.push(DatumCylinder {
                id: row.id,
                feature_id: row.feature_id,
                reversed: row.reversed,
                frame,
                offset_in_payload: row.offset,
            });
        }
    }
    Ok(cylinders)
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
    ctx: &DecodeContext<'_>, row: &SurfaceRow, parameter: &SurfaceParameterRecord,
) -> Result<Option<PositionalCylinderFrame>, CodecError> {
    let Some(values) = active_cylinder_corners(row, parameter) else { return Ok(None); };
    let corners = [[values[1], values[2], values[3]], [values[4], values[5], values[6]]];
    let spans = std::array::from_fn::<_, 3, _>(|index| (corners[1][index] - corners[0][index]).abs());
    let scale = values.into_iter().chain(spans).map(f64::abs).fold(1.0, f64::max);
    let mut selected = match active_cylinder_candidate(row, values[0], corners, spans, scale) {
        Ok(candidate) => candidate,
        Err(()) => return Ok(None),
    };
    let mut frames = parameter.scalar_frames[..parameter.scalar_frames.len() - 1].iter();
    while let Some(frame) = ctx.next_charged(&mut frames, "creo active datum cylinder frames")? {
        let mut slots = frame.slots.iter();
        while let Some(slot) = ctx.next_charged(&mut slots, "creo active datum cylinder lengths")? {
            let Some(length) = slot.value else { continue; };
            let candidate = match active_cylinder_candidate(row, length, corners, spans, scale) {
                Ok(Some(candidate)) => candidate,
                Ok(None) => continue,
                Err(()) => return Ok(None),
            };
            if let Some(existing) = selected {
                if !positional_cylinder_frames_agree(existing, candidate) { return Ok(None); }
            } else { selected = Some(candidate); }
        }
    }
    Ok(selected)
}

fn active_cylinder_corners(
    row: &SurfaceRow, parameter: &SurfaceParameterRecord,
) -> Option<[f64; 7]> {
    (row.kind == crate::surface::SurfaceKind::Cylinder
        && matches!(
            row.boundary_type,
            crate::surface::BoundaryType::Code00 | crate::surface::BoundaryType::Code01
        ))
    .then_some(())?;
    let terminal = parameter.terminal_scalar_frame()?;
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
    Some(terminal_values)
}
fn active_cylinder_candidate(
    row: &SurfaceRow, signed_length: f64, corners: [[f64; 3]; 2],
    spans: [f64; 3], scale: f64,
) -> Result<Option<PositionalCylinderFrame>, ()> {
    let close = |first: f64, second: f64|
        (first - second).abs() <= EPS_ACTIVE_CYLINDER_RELATIVE * scale;
        if !signed_length.is_finite() || signed_length == 0.0 {
            return Ok(None);
        }
        let length = signed_length.abs();
        let mut axis_indices = (0..3).filter(|index| close(spans[*index], length));
        let Some(axis_index) = axis_indices.next() else {
            return Ok(None);
        };
        if axis_indices.next().is_some() {
            return Ok(None);
        }
        let [first_radial, second_radial] = match axis_index {
            0 => [1, 2],
            1 => [0, 2],
            2 => [0, 1],
            _ => return Ok(None),
        };
        let (diameter_index, radius_index) =
            if close(spans[first_radial], 2.0 * spans[second_radial]) {
                (first_radial, second_radial)
            } else if close(spans[second_radial], 2.0 * spans[first_radial]) {
                (second_radial, first_radial)
            } else {
                return Ok(None);
            };
        let radius = spans[diameter_index] * 0.5;
        if radius <= EPS_ACTIVE_CYLINDER_MIN * scale
            || spans[radius_index] <= EPS_ACTIVE_CYLINDER_MIN * scale
        {
            return Ok(None);
        }
        let mut origin = [0.0; 3];
        origin[diameter_index] =
            f64::midpoint(corners[0][diameter_index], corners[1][diameter_index]);
        origin[axis_index] = corners[1][axis_index];
        origin[radius_index] = corners[0][radius_index];
        let mut axis = [0.0; 3];
        axis[axis_index] = (corners[0][axis_index] - corners[1][axis_index]).signum();
        let orientation = if signed_length.is_sign_negative() {
            -1.0
        } else {
            1.0
        } * if row.reversed { -1.0 } else { 1.0 };
        let mut ref_direction = [0.0; 3];
        ref_direction[diameter_index] =
            orientation * (corners[1][diameter_index] - corners[0][diameter_index]).signum();
        let candidate =
            PositionalCylinderFrame::new(origin, axis, ref_direction, radius, Some(length)).ok_or(())?;
    Ok(Some(candidate))
}

fn positional_plane(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    row: &SurfaceRow,
    row_end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<DatumPlaneRecord>, CodecError> {
    let id_start = row.offset;
    if payload.get(id_start).is_none_or(|byte| *byte > 0xbf) { return Ok(None); }
    let (_, after_id) = crate::psb::compact_int(payload, id_start);
    if payload.get(after_id) != Some(&0x22) { return Ok(None); }
    let (_, after_feature) = crate::psb::compact_int(payload, after_id + 1);
    let body_start = crate::psb::compact_int(payload, after_feature + 2).1;
    let mut slots_scope = ctx.reserve_scoped(0, "creo datum plane scratch")?;
    let Some(values) = slots_scope.with_storage(|| datum_slots(ctx, payload, body_start, 10, row_end, cache))? else {
        return Ok(None);
    };
    let outline = &values[4..];
    let equal = [
        slot_equal(&outline[0], &outline[3]),
        slot_equal(&outline[1], &outline[4]),
        slot_equal(&outline[2], &outline[5]),
    ];
    let mut held = [Axis::X, Axis::Y, Axis::Z]
        .into_iter()
        .zip(equal)
        .filter_map(|(axis, equal)| (equal == Some(true)).then_some(axis));
    let Some(axis) = held.next().filter(|_| held.next().is_none()) else {
        return Ok(None);
    };
    let Some(plane_offset) = outline[axis.index()].value else { return Ok(None); };
    let Some(plane) = DatumPlane::new(axis, plane_offset) else { return Ok(None); };
    let Some(opposite) = outline[axis.index() + 3].value else { return Ok(None); };
    let [u, v] = axis.complement().map(Axis::index);
    Ok(DatumPlaneRecord::new(row.id, row.feature_id, plane, opposite,
        [[outline[u].value, outline[v].value], [outline[u + 3].value, outline[v + 3].value]], id_start))

}

/// Decode a named datum from its matching outline coordinates.
pub(crate) fn named_plane(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<DatumPlaneRecord>, CodecError> {
    let marker = b"outline\0\xf9\x02\x03";
    let Some(outline) = ctx.find_map(payload.get(0..).unwrap_or_default().windows(marker.len()).enumerate(), |(offset, bytes)| Ok((bytes == marker).then_some(offset)), "find Creo datum outline")? else {
        return Ok(None);
    };
    let id_marker = b"\xe0\x01geom_id\0";
    let Some(id_at) = ctx.find_map(payload[..outline].windows(id_marker.len()).enumerate().rev(), |(offset, bytes)| Ok((bytes == id_marker).then_some(offset)), "creo datum ID lookup")? else { return Ok(None); };
    let feature_marker = b"feat_id\0";
    let Some(feature_at) = ctx.find_map(payload[..outline].windows(feature_marker.len()).enumerate().rev(), |(offset, bytes)| Ok((bytes == feature_marker).then_some(offset)), "creo datum feature lookup")? else { return Ok(None); };
    let Some(feature_field_start) = feature_at.checked_sub(2) else { return Ok(None); };
    if payload.get(feature_field_start) != Some(&crate::psb::token::NAMED_RECORD) { return Ok(None); }
    let outline_field_start = outline.checked_sub(2)
        .filter(|start| payload.get(*start) == Some(&crate::psb::token::NAMED_RECORD))
        .unwrap_or(outline);
    let Ok((id, id_end)) = crate::psb::reference_id(payload, id_at + id_marker.len()) else { return Ok(None); };
    if id_end > feature_field_start { return Ok(None); }
    let Ok((feature_id, feature_end)) = crate::psb::reference_id(payload, feature_at + feature_marker.len()) else { return Ok(None); };
    if feature_end > outline_field_start { return Ok(None); }
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let mut slots_scope = ctx.reserve_scoped(0, "creo datum plane scratch")?;
    let Some(slots) = slots_scope.with_storage(|| named_outline_slots(ctx, payload, outline + marker.len(), &cache))? else {
        return Ok(None);
    };
    let standalone_zero = |slot: &DatumSlot<'_>| matches!(slot.token, [0x18 | 0x0f]);
    let mut zero_axes = [Axis::X, Axis::Y, Axis::Z].into_iter().filter(|axis| {
        standalone_zero(&slots[axis.index()]) && standalone_zero(&slots[axis.index() + 3])
    });
    let zero_first = zero_axes.next();
    let zero_second = zero_axes.next();
    let mut held = [Axis::X, Axis::Y, Axis::Z]
        .into_iter()
        .filter(|axis| slot_equal(&slots[axis.index()], &slots[axis.index() + 3]) == Some(true));
    let axis = match (zero_first, zero_second) {
        (Some(axis), None) => axis,
        (None, None) => {
            let Some(axis) = held.next().filter(|_| held.next().is_none()) else {
                return Ok(None);
            };
            axis
        }
        _ => return Ok(None),
    };
    let Some(plane_offset) = slots[axis.index()].value else { return Ok(None); };
    let Some(plane) = DatumPlane::new(axis, plane_offset) else { return Ok(None); };
    let Some(opposite) = slots[axis.index() + 3].value else { return Ok(None); };
    let [u, v] = axis.complement().map(Axis::index);
    Ok(DatumPlaneRecord::new(id, feature_id, plane, opposite,
        [[slots[u].value, slots[v].value], [slots[u + 3].value, slots[v + 3].value]], outline))

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

fn named_outline_slots<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<Vec<DatumSlot<'a>>>, CodecError> {
    let mut slots = Vec::new();
    ctx.reserve_vec(&mut slots, 6, "creo named datum outline slots")?;
    let mut cursor = crate::psb::Cursor::at(data, offset);
    while slots.len() < 6 {
        let start = cursor.pos();
        let filled = slots.len();
        let Some(value) =
            cursor.take_with(|data, pos| decode_outline_slot(data, pos, cache, filled))
        else {
            return Ok(None);
        };
        slots.push(DatumSlot {
            value,
            token: &data[start..cursor.pos()],
        });
    }
    Ok(Some(slots))
}

#[derive(Debug)]
struct DatumSlot<'a> {
    value: Option<f64>,
    token: &'a [u8],
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

fn datum_slots<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    offset: usize,
    count: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<Vec<DatumSlot<'a>>>, CodecError> {
    let mut slots = Vec::new();
    ctx.reserve_vec(&mut slots, count, "creo positional datum slots")?;
    let mut cursor = offset;
    while slots.len() < count {
        let start = cursor;
        let Some((value, next)) = decode_datum_slot(data, cursor, cache) else {
            return Ok(None);
        };
        if next > end {
            return Ok(None);
        }
        let Some(token) = data.get(start..next) else {
            return Ok(None);
        };
        slots.push(DatumSlot { value, token });
        cursor = next;
    }
    Ok(Some(slots))
}

fn slot_equal(first: &DatumSlot<'_>, second: &DatumSlot<'_>) -> Option<bool> {
    match (first.value, second.value) {
        (Some(first), Some(second)) => {
            let scale = first.abs().max(second.abs()).max(1.0);
            Some((first - second).abs() <= EPS_DATUM_COORDINATE_AGREEMENT * scale)
        }
        (None, None) => Some(first.token == second.token),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
