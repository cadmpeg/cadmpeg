//! The point and curve record lists of `sgSketch` objects
//! (`docs/formats/sldprt.md`, "Sketch objects").
//!
//! An `sgSketch` object writes a 14-byte header (a little-endian u32 point
//! count *N*, u16 `0`, a u16, u16 `0` and a little-endian u32), *N* point
//! records, a little-endian u32 and its curve records. In a current-layout
//! curve record with selector `4` or `12` and no object-identifier trailer,
//! the endpoint and centre fields are zero-based indices into the point
//! records of its own sketch. Other curve records keep the readings of
//! `super::endpoints`.

use super::endpoints::minor_arc_angles;
use super::SKETCH_ANGLE_TOLERANCE;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// The largest difference, in metres, between an arc's two endpoint radii.
const EPS_ARC_RADIUS_M: f64 = 1.0e-9;
/// The largest distance, in metres, of a three-coordinate point record from
/// the sketch plane that still reads as a planar point.
const EPS_PLANE_OFFSET_M: f64 = 1.0e-9;

/// The f32 `-1` at +13 and +31 of every record head.
const F32_MINUS_ONE: [u8; 4] = [0x00, 0x00, 0x80, 0xbf];
/// The f64 `-1` that follows a curve record's endpoint fields.
const F64_MINUS_ONE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0xf0, 0xbf];
/// The i32 `-2` that follows an arc's centre index.
const I32_MINUS_TWO: [u8; 4] = [0xfe, 0xff, 0xff, 0xff];
/// The length of the `sgSketch` header before the first point record.
const SKETCH_HEADER_LEN: usize = 14;
/// The largest point count read as an `sgSketch` header.
const MAX_SKETCH_POINTS: usize = 4096;
/// Where a record's fields start after a head with its eight zero bytes.
const CURRENT_FIELDS: usize = 64;
/// Where a record's fields start in files before `swVersion` 11000.
const LEGACY_FIELDS: usize = 56;
/// Where a record's u16 selector stands.
const SELECTOR: usize = 37;
/// The selectors whose curve records index the point records of their own
/// sketch. Selectors `68` and `132` mark the link-bearing forms, and an odd
/// selector is open item EV-27.
const ROSTER_SELECTORS: [u16; 2] = [4, 12];
/// Where a current-layout line record stores its endpoint trailer u32: `1`
/// selects direct point-object identifiers.
const LINE_TRAILER: usize = 84;
/// Where a current-layout line record stores its identity: with a zero
/// trailer, a value other than `0` and null selects one-based ordinals.
const LINE_IDENTITY: usize = 88;
/// A point record's coordinate masks: bit 4 set writes two coordinates.
const PLANAR_POINT_MASKS: [u16; 6] = [0x12, 0x13, 0x16, 0x1a, 0x1e, 0x1f];
/// A point record's coordinate masks that write three coordinates.
const SPATIAL_POINT_MASKS: [u16; 2] = [0x0e, 0x0f];

/// A curve record's type field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CurveForm {
    /// Type `0`: the segment from the first endpoint to the second.
    Line { start: usize, end: usize },
    /// Type `1` in its arc layout: the minor arc about `center` between
    /// `start` and `end`; a full circle through `start` when the two endpoint
    /// indices are equal.
    Arc {
        start: usize,
        end: usize,
        center: usize,
    },
}

/// One sketch's point records and curve records, by stream offset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SketchRecordList {
    points: Vec<usize>,
    curves: Vec<usize>,
}

impl SketchRecordList {
    /// The stream offset of point record `index`.
    pub(super) fn point(&self, index: usize) -> Option<usize> {
        self.points.get(index).copied()
    }
}

/// The record a stream offset starts, located in its sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ListedRecord<'a> {
    /// A point record.
    Point,
    /// A curve record of `sketch`.
    Curve { sketch: &'a SketchRecordList },
}

/// Every `sgSketch` record list of one stream.
#[derive(Debug, Clone, Default)]
pub(super) struct SketchRecordLists {
    lists: Vec<SketchRecordList>,
    /// Record offset to list index and point flag.
    records: BTreeMap<usize, (usize, bool)>,
}

impl SketchRecordLists {
    /// Find the `sgSketch` record lists of `payload`.
    pub(super) fn new(payload: &[u8]) -> Self {
        let heads = record_heads(payload);
        let mut lists = Vec::new();
        let mut index = 0;
        while index < heads.len() {
            let Some(list) = sketch_at(payload, &heads, index) else {
                index += 1;
                continue;
            };
            index += list.points.len() + list.curves.len();
            lists.push(list);
        }
        let mut records = BTreeMap::new();
        for (list_index, list) in lists.iter().enumerate() {
            for &point in &list.points {
                records.insert(point, (list_index, true));
            }
            for &curve in &list.curves {
                records.insert(curve, (list_index, false));
            }
        }
        Self { lists, records }
    }

    /// The listed record that starts at `offset`.
    pub(super) fn record(&self, offset: usize) -> Option<ListedRecord<'_>> {
        let &(list, point) = self.records.get(&offset)?;
        let sketch = self.lists.get(list)?;
        Some(if point {
            ListedRecord::Point
        } else {
            ListedRecord::Curve { sketch }
        })
    }
}

/// The sketch whose first point record is `heads[first]`: the 14-byte header
/// before it states *N*, the *N* heads from there are point records (or
/// records projected from outside the sketch), and the next head is a curve
/// record. The curve list is the run of curve records from there.
fn sketch_at(payload: &[u8], heads: &[usize], first: usize) -> Option<SketchRecordList> {
    let head = *heads.get(first)?;
    let header = head.checked_sub(SKETCH_HEADER_LEN)?;
    let count = usize::try_from(View::u32_le_at(payload, header)?).ok()?;
    if count == 0
        || count > MAX_SKETCH_POINTS
        || View::u16_le_at(payload, header.checked_add(4)?)? != 0
        || View::u16_le_at(payload, header.checked_add(8)?)? != 0
    {
        return None;
    }
    let end = first.checked_add(count)?;
    let points = heads.get(first..end)?;
    if !points
        .iter()
        .all(|&point| point_record(payload, point) || external_record(payload, point))
    {
        return None;
    }
    let curves = heads
        .get(end..)?
        .iter()
        .copied()
        .take_while(|&curve| curve_record(payload, curve))
        .collect::<Vec<_>>();
    if curves.is_empty() {
        return None;
    }
    Some(SketchRecordList {
        points: points.to_vec(),
        curves,
    })
}

/// The offsets of every record head: `ff ff`, the schema version `1f 00` or
/// `07 00`, and f32 `-1` at +13 and at +31.
fn record_heads(payload: &[u8]) -> Vec<usize> {
    payload
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair == &[0xff, 0xff])
        .map(|(offset, _)| offset)
        .filter(|&offset| record_head(payload, offset))
        .collect()
}

fn record_head(payload: &[u8], offset: usize) -> bool {
    let at = |start: usize, len: usize| {
        let start = offset.checked_add(start)?;
        payload.get(start..start.checked_add(len)?)
    };
    matches!(at(2, 2), Some([0x1f | 0x07, 0x00]))
        && at(13, 4) == Some(&F32_MINUS_ONE[..])
        && at(31, 4) == Some(&F32_MINUS_ONE[..])
}

/// Whether the record at `offset` writes an `sgExtEnt_c` object at +35.
fn external_record(payload: &[u8], offset: usize) -> bool {
    offset
        .checked_add(35)
        .and_then(|at| View::u16_le_at(payload, at))
        .is_some_and(|pointer| pointer != 0)
}

/// Where the fields of the record at `offset` start: +64 after the eight zero
/// bytes at +56, or +56 when they are absent.
fn record_fields(payload: &[u8], offset: usize, current: bool) -> Option<usize> {
    if external_record(payload, offset) {
        return None;
    }
    if current {
        let zeros = offset.checked_add(LEGACY_FIELDS)?;
        (payload.get(zeros..zeros.checked_add(8)?)? == [0; 8])
            .then(|| offset.checked_add(CURRENT_FIELDS))
            .flatten()
    } else {
        offset.checked_add(LEGACY_FIELDS)
    }
}

/// The fields of a point record: its coordinate mask stands first.
fn point_fields(payload: &[u8], offset: usize) -> Option<(usize, u16)> {
    [true, false].into_iter().find_map(|current| {
        let fields = record_fields(payload, offset, current)?;
        let mask = View::u16_le_at(payload, fields)?;
        (PLANAR_POINT_MASKS.contains(&mask) || SPATIAL_POINT_MASKS.contains(&mask))
            .then_some((fields, mask))
    })
}

/// The fields of a curve record: two u16 endpoint indices, a u32, f64 `-1`
/// and the u16 curve type.
fn curve_fields(payload: &[u8], offset: usize) -> Option<usize> {
    [true, false].into_iter().find_map(|current| {
        let fields = record_fields(payload, offset, current)?;
        let minus_one = fields.checked_add(8)?;
        (payload.get(minus_one..minus_one.checked_add(8)?)? == F64_MINUS_ONE).then_some(fields)
    })
}

fn point_record(payload: &[u8], offset: usize) -> bool {
    point_fields(payload, offset).is_some() && curve_fields(payload, offset).is_none()
}

fn curve_record(payload: &[u8], offset: usize) -> bool {
    curve_fields(payload, offset).is_some()
}

/// The planar coordinates of the point record at `offset`, in metres.
pub(super) fn point_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
    if curve_fields(payload, offset).is_some() {
        return None;
    }
    let (fields, mask) = point_fields(payload, offset)?;
    let u = View::f64_le_at(payload, fields.checked_add(2)?)?;
    let v = View::f64_le_at(payload, fields.checked_add(10)?)?;
    if SPATIAL_POINT_MASKS.contains(&mask)
        && View::f64_le_at(payload, fields.checked_add(18)?)?.abs() > EPS_PLANE_OFFSET_M
    {
        return None;
    }
    (u.is_finite() && v.is_finite()).then_some([u, v])
}

/// The form of the curve record at `offset`, when its type is a line or an
/// arc in its arc layout and its fields are positions in the point records of
/// its own sketch: the current layout with its eight zero bytes, a roster
/// selector, and for a line no object-identifier trailer. The layout before
/// `swVersion` 11000 keeps the readings of `super::endpoints`.
pub(super) fn curve_form(payload: &[u8], offset: usize) -> Option<CurveForm> {
    let fields = curve_fields(payload, offset)?;
    if fields != offset.checked_add(CURRENT_FIELDS)?
        || !ROSTER_SELECTORS.contains(&View::u16_le_at(payload, offset.checked_add(SELECTOR)?)?)
    {
        return None;
    }
    let start = usize::from(View::u16_le_at(payload, fields)?);
    let end = usize::from(View::u16_le_at(payload, fields.checked_add(2)?)?);
    match View::u16_le_at(payload, fields.checked_add(16)?)? {
        0 => (start != end && !object_identifier_line(payload, offset)?)
            .then_some(CurveForm::Line { start, end }),
        1 => {
            let center_at = fields.checked_add(20)?;
            let tail = center_at.checked_add(2)?;
            if View::u16_le_at(payload, fields.checked_add(18)?)? != 0
                || payload.get(tail..tail.checked_add(4)?)? != I32_MINUS_TWO
            {
                return None;
            }
            let center = usize::from(View::u16_le_at(payload, center_at)?);
            Some(CurveForm::Arc { start, end, center })
        }
        u16::MAX => {
            let center_at = fields.checked_add(20)?;
            let tail = center_at.checked_add(2)?;
            if View::u16_le_at(payload, fields.checked_add(18)?)? != u16::MAX
                || payload.get(tail..tail.checked_add(4)?)? != I32_MINUS_TWO
            {
                return None;
            }
            let center = usize::from(View::u16_le_at(payload, center_at)?);
            Some(CurveForm::Arc { start, end, center })
        }
        _ => None,
    }
}

/// Whether the current-layout line record at `offset` stores point-object
/// identifiers or one-based ordinals: a trailer u32 `1`, or a zero trailer and
/// an identity other than `0` and null.
fn object_identifier_line(payload: &[u8], offset: usize) -> Option<bool> {
    let trailer = View::u32_le_at(payload, offset.checked_add(LINE_TRAILER)?)?;
    let identity = View::u32_le_at(payload, offset.checked_add(LINE_IDENTITY)?)?;
    Some(trailer == 1 || (trailer == 0 && identity != 0 && identity != u32::MAX))
}

/// The geometry of the listed record at `offset`, from the record and the
/// point records of its own sketch: a point record's coordinates, a line
/// between two point records, a full circle about a point record through
/// another, or the minor arc about a point record between two others.
/// `project` maps a coordinate pair in metres to sketch coordinates. `None`
/// when the offset starts no listed record, a referenced point record writes
/// no coordinates, or the record's form is not one this module reads.
pub(super) fn listed_record_geometry(
    payload: &[u8],
    lists: &SketchRecordLists,
    offset: usize,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<SketchGeometry> {
    let point =
        |sketch: &SketchRecordList, index: usize| point_coordinates(payload, sketch.point(index)?);
    match lists.record(offset)? {
        ListedRecord::Point => {
            let position = project(point_coordinates(payload, offset)?)?;
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).ok()
        }
        ListedRecord::Curve { sketch } => match curve_form(payload, offset)? {
            CurveForm::Line { start, end } => {
                let start = project(point(sketch, start)?)?;
                let end = project(point(sketch, end)?)?;
                (start != end)
                    .then(|| {
                        SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
                    })?
                    .ok()
            }
            CurveForm::Arc { start, end, center } => {
                let center_m = point(sketch, center)?;
                let start_m = point(sketch, start)?;
                let end_m = point(sketch, end)?;
                arc_geometry(center_m, start_m, end_m, start == end, project)
            }
        },
    }
}

/// A circle about `center` through `start`, or the minor arc about `center`
/// between `start` and `end`, all in metres, mapped by `project`. The arc's
/// end angle is its start angle plus its sweep. The arc's midpoint fixes its
/// sense after the mapping, which can mirror.
fn arc_geometry(
    center: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
    full: bool,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<SketchGeometry> {
    let radius_m = (start[0] - center[0]).hypot(start[1] - center[1]);
    if radius_m <= EPS_ARC_RADIUS_M {
        return None;
    }
    let center_ir = project(center)?;
    let start_ir = project(start)?;
    let radius = Length::new((start_ir.u - center_ir.u).hypot(start_ir.v - center_ir.v))?;
    if full {
        return SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: center_ir,
            radius,
        })
        .ok();
    }
    let end_radius_m = (end[0] - center[0]).hypot(end[1] - center[1]);
    if (end_radius_m - radius_m).abs() > EPS_ARC_RADIUS_M {
        return None;
    }
    let (start_angle_m, end_angle_m, swapped) = minor_arc_angles(
        (start[1] - center[1]).atan2(start[0] - center[0]),
        (end[1] - center[1]).atan2(end[0] - center[0]),
    );
    let (start_ir, end) = if swapped {
        (project(end)?, start)
    } else {
        (start_ir, end)
    };
    let sweep_m = (end_angle_m - start_angle_m).rem_euclid(TAU);
    if sweep_m <= SKETCH_ANGLE_TOLERANCE {
        return None;
    }
    let middle_angle = start_angle_m + sweep_m / 2.0;
    let middle = project([
        center[0] + radius_m * middle_angle.cos(),
        center[1] + radius_m * middle_angle.sin(),
    ])?;
    let end_ir = project(end)?;
    let angle = |point: Point2| (point.v - center_ir.v).atan2(point.u - center_ir.u);
    let (start_angle, end_angle, middle_angle) = (angle(start_ir), angle(end_ir), angle(middle));
    let counterclockwise =
        (middle_angle - start_angle).rem_euclid(TAU) < (end_angle - start_angle).rem_euclid(TAU);
    let (start_angle, end_angle) = if counterclockwise {
        (start_angle, end_angle)
    } else {
        (end_angle, start_angle)
    };
    // The end angle exceeds the start angle by the counterclockwise sweep.
    let end_angle = start_angle + (end_angle - start_angle).rem_euclid(TAU);
    SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: center_ir,
        radius,
        start_angle: Angle::new(start_angle)?,
        end_angle: Angle::new(end_angle)?,
    })
    .ok()
}

#[cfg(test)]
mod tests;
