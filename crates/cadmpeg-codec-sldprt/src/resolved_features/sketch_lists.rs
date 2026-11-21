//! The point and curve record lists of `sgSketch` objects
//! (`docs/formats/sldprt.md`, "Sketch objects").
//!
//! An `sgSketch` object writes a 14-byte header (a little-endian u32 point
//! count *N*, u16 `0`, a u16, u16 `0` and a little-endian u32), *N* point
//! records, a little-endian u32 and its curve lists. In a curve record whose
//! selector has bit 0 clear, the endpoint and centre fields are zero-based
//! indices into the point records of its own sketch, and the sense word
//! selects a line, a counterclockwise arc or a clockwise arc.

use super::SKETCH_ANGLE_TOLERANCE;
use crate::brep::graph::LINEAR_RESOLUTION_MM;
use crate::layout::{
    sketch_arc_record as arc, sketch_arc_record_legacy as legacy_arc, sketch_line_record as line,
    sketch_record_head as head,
};
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
const CURRENT_FIELDS: usize = arc::START;
/// Where a record's fields start in files before `swVersion` 11000.
const LEGACY_FIELDS: usize = legacy_arc::START;
/// Where a record's u16 selector stands.
const SELECTOR: usize = head::SELECTOR;
/// The selector bit of the curve records of open item EV-27.
const SELECTOR_OPEN_BIT: u16 = 1;
/// The selectors of the curve records that [`ListedReading::Roster`] reads.
const ROSTER_SELECTORS: [u16; 2] = [4, 12];
/// Where a current-layout line record stores its trailer u32.
const LINE_TRAILER: usize = line::TRAILER;
/// Where a current-layout line record stores its identity.
const LINE_IDENTITY: usize = line::RECORD_ID;
/// Where a curve record's f64 `-1` stands after its fields start.
const MINUS_ONE: usize = arc::MINUS_ONE - arc::START;
/// Where a curve record's sense word stands after its fields start.
const SENSE: usize = arc::SENSE - arc::START;
/// Where an arc record's centre index stands after its fields start.
const CENTER: usize = arc::CENTER - arc::START;
/// Where an arc record's four i32 `-2` start after its fields start.
const MINUS_TWO: usize = arc::MINUS_TWO - arc::START;
/// The sense word of a counterclockwise arc.
const COUNTERCLOCKWISE: i32 = 1;
/// The sense word of a clockwise arc.
const CLOCKWISE: i32 = -1;
/// The low half of a line's sense word.
const LINE_TYPE_MASK: i32 = 0xffff;
/// A point record's coordinate masks: bit 4 set writes two coordinates.
const PLANAR_POINT_MASKS: [u16; 6] = [0x12, 0x13, 0x16, 0x1a, 0x1e, 0x1f];
/// A point record's coordinate masks that write three coordinates.
const SPATIAL_POINT_MASKS: [u16; 2] = [0x0e, 0x0f];

/// The curve records a listed reading takes geometry from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ListedReading {
    /// Records of a sketch's first curve list in the current layout with
    /// selector `4` or `12`, without a line's identifier trailer: the records
    /// the sketch passes read before their own rules.
    Roster,
    /// Every curve record whose selector has bit 0 clear.
    Complete,
}

/// A curve record's sense word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CurveForm {
    /// Low half `0`: the segment from the first endpoint to the second.
    Line { start: usize, end: usize },
    /// `1` or `-1`: the arc about `center` from `start` to `end`, swept
    /// counterclockwise for `1`; a full circle through `start` when the two
    /// endpoint indices are equal.
    Arc {
        start: usize,
        end: usize,
        center: usize,
        counterclockwise: bool,
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
    /// A curve record of `sketch`; `later` when it follows the first curve
    /// list.
    Curve {
        sketch: &'a SketchRecordList,
        later: bool,
    },
}

/// Where a record stands in its sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordPlace {
    Point,
    FirstCurveList,
    LaterCurveList,
}

/// Every `sgSketch` record list of one stream.
#[derive(Debug, Clone, Default)]
pub(super) struct SketchRecordLists {
    lists: Vec<SketchRecordList>,
    /// Record offset to list index and place.
    records: BTreeMap<usize, (usize, RecordPlace)>,
}

impl SketchRecordLists {
    /// Find the `sgSketch` record lists of `payload`. A curve record after
    /// the first curve list belongs to the nearest sketch whose point records
    /// precede it.
    pub(super) fn new(payload: &[u8]) -> Self {
        let heads = record_heads(payload);
        let mut lists: Vec<SketchRecordList> = Vec::new();
        let mut later_curves = Vec::new();
        let mut index = 0;
        while index < heads.len() {
            let Some(list) = sketch_at(payload, &heads, index) else {
                if curve_record(payload, heads[index]) {
                    later_curves.push(heads[index]);
                }
                index += 1;
                continue;
            };
            index += list.points.len() + list.curves.len();
            lists.push(list);
        }
        let mut records = BTreeMap::new();
        for (list_index, list) in lists.iter().enumerate() {
            for &point in &list.points {
                records.insert(point, (list_index, RecordPlace::Point));
            }
            for &curve in &list.curves {
                records.insert(curve, (list_index, RecordPlace::FirstCurveList));
            }
        }
        let mut owner = None;
        let mut next = 0;
        for curve in later_curves {
            while lists
                .get(next)
                .and_then(|list| list.points.last())
                .is_some_and(|&last| last < curve)
            {
                owner = Some(next);
                next += 1;
            }
            if let Some(owner) = owner {
                records.insert(curve, (owner, RecordPlace::LaterCurveList));
            }
        }
        Self { lists, records }
    }

    /// The listed record that starts at `offset`.
    pub(super) fn record(&self, offset: usize) -> Option<ListedRecord<'_>> {
        let &(list, place) = self.records.get(&offset)?;
        let sketch = self.lists.get(list)?;
        Some(match place {
            RecordPlace::Point => ListedRecord::Point,
            RecordPlace::FirstCurveList => ListedRecord::Curve {
                sketch,
                later: false,
            },
            RecordPlace::LaterCurveList => ListedRecord::Curve {
                sketch,
                later: true,
            },
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
    matches!(at(head::SCHEMA, 2), Some([0x1f | 0x07, 0x00]))
        && at(head::ATTRIBUTE_MINUS_ONE, 4) == Some(&F32_MINUS_ONE[..])
        && at(head::MINUS_ONE, 4) == Some(&F32_MINUS_ONE[..])
}

/// Whether the record at `offset` writes an `sgExtEnt_c` object at +35.
fn external_record(payload: &[u8], offset: usize) -> bool {
    offset
        .checked_add(head::OBJECT_POINTER)
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
        let zeros = offset.checked_add(head::ZERO_PAD)?;
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
        let minus_one = fields.checked_add(MINUS_ONE)?;
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

/// The form of the curve record at `offset`, in either head layout, when its
/// selector has bit 0 clear: a line when the low half of its sense word is
/// `0`, and an arc when the word is `1` or `-1` and the centre index is
/// followed by i32 `-2`. [`ListedReading::Roster`] also requires the current
/// layout, a roster selector, and for a line no identifier trailer.
pub(super) fn curve_form(
    payload: &[u8],
    offset: usize,
    reading: ListedReading,
) -> Option<CurveForm> {
    let fields = curve_fields(payload, offset)?;
    let selector = View::u16_le_at(payload, offset.checked_add(SELECTOR)?)?;
    if selector & SELECTOR_OPEN_BIT != 0 {
        return None;
    }
    let roster = reading == ListedReading::Roster;
    if roster
        && (fields != offset.checked_add(CURRENT_FIELDS)? || !ROSTER_SELECTORS.contains(&selector))
    {
        return None;
    }
    let start = usize::from(View::u16_le_at(payload, fields)?);
    let end = usize::from(View::u16_le_at(payload, fields.checked_add(2)?)?);
    let sense = View::i32_le_at(payload, fields.checked_add(SENSE)?)?;
    if sense == COUNTERCLOCKWISE || sense == CLOCKWISE {
        let center_at = fields.checked_add(CENTER)?;
        let tail = fields.checked_add(MINUS_TWO)?;
        if payload.get(tail..tail.checked_add(4)?)? != I32_MINUS_TWO {
            return None;
        }
        let center = usize::from(View::u16_le_at(payload, center_at)?);
        return Some(CurveForm::Arc {
            start,
            end,
            center,
            counterclockwise: sense == COUNTERCLOCKWISE,
        });
    }
    (sense & LINE_TYPE_MASK == 0
        && start != end
        && !(roster && identifier_trailer_line(payload, offset)?))
    .then_some(CurveForm::Line { start, end })
}

/// Whether the current-layout line record at `offset` stores a trailer u32
/// `1`, or a zero trailer and an identity other than `0` and null.
fn identifier_trailer_line(payload: &[u8], offset: usize) -> Option<bool> {
    let trailer = View::u32_le_at(payload, offset.checked_add(LINE_TRAILER)?)?;
    let identity = View::u32_le_at(payload, offset.checked_add(LINE_IDENTITY)?)?;
    Some(trailer == 1 || (trailer == 0 && identity != 0 && identity != u32::MAX))
}

/// What the `sgSketch` curve record of a marker gives in place of the
/// straight chord a marker reading gave it.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ChordRecord {
    /// The record's arc, which runs between the chord's two endpoints.
    Arc(SketchGeometry),
    /// The record states an arc that the chord's endpoints do not carry.
    Conflict(ChordConflict),
}

/// Why the curve record of a marker leaves the line a marker reading gave it
/// in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChordConflict {
    /// A point record the record indexes writes no planar coordinates.
    PointUnread,
    /// The two endpoint indices are equal.
    FullCircle,
    /// The record's endpoint point records stand at other positions.
    OtherEndpoints,
    /// The record's endpoints and centre define no arc.
    NoArc,
    /// The record's arc does not pass through both endpoints of the chord.
    MissesChord,
    /// The record has sense `1` or `-1` and selector bit 0 set (open item
    /// EV-27).
    OpenSelector,
    /// The record has sense `1` or `-1` and no four i32 `-2` after its
    /// centre index: an ellipse.
    Ellipse,
    /// The record's sense word has a curve type other than `0` and is not
    /// `1` or `-1`.
    OtherSense(i32),
    /// The record is an arc, and the line comes from a reading that does not
    /// compare the two.
    Unchecked,
}

impl std::fmt::Display for ChordConflict {
    /// What the record states, as the predicate of a sentence about it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PointUnread => f.write_str("states an arc whose point records are not all read"),
            Self::FullCircle => {
                f.write_str("states an arc whose equal endpoint indices make a full circle")
            }
            Self::OtherEndpoints => f.write_str("states an arc between point records at other positions"),
            Self::NoArc => f.write_str("states an arc whose endpoints and centre define no arc"),
            Self::MissesChord => f.write_str(
                "states an arc between the line's endpoint point records, but the arc misses the line's endpoints",
            ),
            Self::OpenSelector => f.write_str(
                "states an arc sense with selector bit 0 set, whose indices are open item EV-27",
            ),
            Self::Ellipse => f.write_str(
                "states an ellipse: sense `1` or `-1` without the four i32 `-2` after its centre index",
            ),
            Self::OtherSense(sense) => write!(
                f,
                "has sense word {sense}, which reads as neither a line nor an arc"
            ),
            Self::Unchecked => f.write_str(
                "states an arc, and the line comes from a reading that does not compare the record with it",
            ),
        }
    }
}

/// The reading of the listed curve record at `offset` when a marker reading
/// gave it the straight chord `chord`: for an arc record, the record's arc
/// when its two endpoint point records project onto the chord's endpoints in
/// either order and the arc itself ends at both chord endpoints, each within
/// the linear resolution, and the conflict otherwise; for a record whose form
/// [`curve_form`] does not read, the conflict its sense and selector give.
/// `None` when the offset starts no listed curve record or the record states
/// a line.
pub(super) fn chord_record(
    payload: &[u8],
    lists: &SketchRecordLists,
    offset: usize,
    chord: [Point2; 2],
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<ChordRecord> {
    let ListedRecord::Curve { sketch, .. } = lists.record(offset)? else {
        return None;
    };
    let (start, end, center, counterclockwise) =
        match curve_form(payload, offset, ListedReading::Complete) {
            Some(CurveForm::Arc {
                start,
                end,
                center,
                counterclockwise,
            }) => (start, end, center, counterclockwise),
            Some(CurveForm::Line { .. }) => return None,
            None => return unread_form_conflict(payload, offset).map(ChordRecord::Conflict),
        };
    if start == end {
        return Some(ChordRecord::Conflict(ChordConflict::FullCircle));
    }
    let point = |index: usize| point_coordinates(payload, sketch.point(index)?);
    let (Some(start_m), Some(end_m), Some(center_m)) = (point(start), point(end), point(center))
    else {
        return Some(ChordRecord::Conflict(ChordConflict::PointUnread));
    };
    let (Some(start_ir), Some(end_ir)) = (project(start_m), project(end_m)) else {
        return Some(ChordRecord::Conflict(ChordConflict::PointUnread));
    };
    let joins_chord = |first: Point2, second: Point2| {
        let near = |left: Point2, right: Point2| {
            (left.u - right.u).hypot(left.v - right.v) <= LINEAR_RESOLUTION_MM
        };
        (near(first, chord[0]) && near(second, chord[1]))
            || (near(first, chord[1]) && near(second, chord[0]))
    };
    if !joins_chord(start_ir, end_ir) {
        return Some(ChordRecord::Conflict(ChordConflict::OtherEndpoints));
    }
    let sweep = if counterclockwise {
        ArcSweep::Counterclockwise
    } else {
        ArcSweep::Clockwise
    };
    let Some(arc) = arc_geometry(center_m, start_m, end_m, sweep, project) else {
        return Some(ChordRecord::Conflict(ChordConflict::NoArc));
    };
    let SketchGeometryDefinition::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = *arc.definition()
    else {
        return Some(ChordRecord::Conflict(ChordConflict::NoArc));
    };
    let (center, radius) = (center.get(), radius.get());
    let at = |angle: f64| {
        Point2::new(
            center.u + radius * angle.cos(),
            center.v + radius * angle.sin(),
        )
    };
    Some(if joins_chord(at(start_angle.get()), at(end_angle.get())) {
        ChordRecord::Arc(arc)
    } else {
        ChordRecord::Conflict(ChordConflict::MissesChord)
    })
}

/// The conflict of the listed curve record at `offset` with a line that a
/// reading other than [`chord_record`] gave its marker: [`ChordConflict::Unchecked`]
/// for an arc record and the form conflict for a record whose form
/// [`curve_form`] does not read. `None` when the offset starts no listed
/// curve record or the record states a line.
pub(super) fn unchecked_record_conflict(
    payload: &[u8],
    lists: &SketchRecordLists,
    offset: usize,
) -> Option<ChordConflict> {
    let ListedRecord::Curve { .. } = lists.record(offset)? else {
        return None;
    };
    match curve_form(payload, offset, ListedReading::Complete) {
        Some(CurveForm::Arc { .. }) => Some(ChordConflict::Unchecked),
        Some(CurveForm::Line { .. }) => None,
        None => unread_form_conflict(payload, offset),
    }
}

/// The conflict of a curve record that [`curve_form`] does not read: `None`
/// when the low half of its sense word is `0`, and otherwise by its sense
/// word and selector.
fn unread_form_conflict(payload: &[u8], offset: usize) -> Option<ChordConflict> {
    let fields = curve_fields(payload, offset)?;
    let sense = View::i32_le_at(payload, fields.checked_add(SENSE)?)?;
    if sense & LINE_TYPE_MASK == 0 {
        return None;
    }
    if sense != COUNTERCLOCKWISE && sense != CLOCKWISE {
        return Some(ChordConflict::OtherSense(sense));
    }
    let selector = View::u16_le_at(payload, offset.checked_add(SELECTOR)?)?;
    Some(if selector & SELECTOR_OPEN_BIT != 0 {
        ChordConflict::OpenSelector
    } else {
        ChordConflict::Ellipse
    })
}

/// Whether the record at `offset` is a current-layout line with an
/// identifier trailer, which [`ListedReading::Roster`] leaves to the marker
/// readings of `sldprt.md` §2: whether its indices are point-record
/// positions is open item EV-29.
pub(super) fn identifier_trailer_line_record(payload: &[u8], offset: usize) -> bool {
    curve_fields(payload, offset)
        .is_some_and(|fields| Some(fields) == offset.checked_add(CURRENT_FIELDS))
        && matches!(
            curve_form(payload, offset, ListedReading::Complete),
            Some(CurveForm::Line { .. })
        )
        && identifier_trailer_line(payload, offset) == Some(true)
}

/// The geometry of the listed record at `offset`, from the record and the
/// point records of its own sketch: a point record's coordinates, a line
/// between two point records, a full circle about a point record through
/// another, or the arc about a point record from one to another in the
/// record's sense.
/// `project` maps a coordinate pair in metres to sketch coordinates. `None`
/// when the offset starts no listed record, a referenced point record writes
/// no coordinates, or `reading` does not admit the record.
pub(super) fn listed_record_geometry(
    payload: &[u8],
    lists: &SketchRecordLists,
    offset: usize,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
    reading: ListedReading,
) -> Option<SketchGeometry> {
    let point =
        |sketch: &SketchRecordList, index: usize| point_coordinates(payload, sketch.point(index)?);
    match lists.record(offset)? {
        ListedRecord::Point => {
            let position = project(point_coordinates(payload, offset)?)?;
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).ok()
        }
        ListedRecord::Curve { later: true, .. } if reading == ListedReading::Roster => None,
        ListedRecord::Curve { sketch, .. } => match curve_form(payload, offset, reading)? {
            CurveForm::Line { start, end } => {
                let start = project(point(sketch, start)?)?;
                let end = project(point(sketch, end)?)?;
                (start != end)
                    .then(|| {
                        SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end })
                    })?
                    .ok()
            }
            CurveForm::Arc {
                start,
                end,
                center,
                counterclockwise,
            } => {
                let center_m = point(sketch, center)?;
                let start_m = point(sketch, start)?;
                let end_m = point(sketch, end)?;
                let sweep = if start == end {
                    ArcSweep::Full
                } else if counterclockwise {
                    ArcSweep::Counterclockwise
                } else {
                    ArcSweep::Clockwise
                };
                arc_geometry(center_m, start_m, end_m, sweep, project)
            }
        },
    }
}

/// How an arc record runs from its start point to its end point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArcSweep {
    /// Equal endpoint indices: the full circle through the start point.
    Full,
    Counterclockwise,
    Clockwise,
}

/// A circle about `center` through `start`, or the arc about `center` from
/// `start` to `end` in `sweep`, all in metres, mapped by `project`. The arc's
/// end angle is its start angle plus its counterclockwise sweep. The arc's
/// midpoint fixes its side after the mapping, which can mirror.
fn arc_geometry(
    center: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
    sweep: ArcSweep,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<SketchGeometry> {
    let radius_m = (start[0] - center[0]).hypot(start[1] - center[1]);
    if radius_m <= EPS_ARC_RADIUS_M {
        return None;
    }
    let center_ir = project(center)?;
    let start_ir = project(start)?;
    let radius = Length::new((start_ir.u - center_ir.u).hypot(start_ir.v - center_ir.v))?;
    let (from, to) = match sweep {
        ArcSweep::Full => {
            return SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: center_ir,
                radius,
            })
            .ok();
        }
        ArcSweep::Counterclockwise => (start, end),
        ArcSweep::Clockwise => (end, start),
    };
    let end_radius_m = (end[0] - center[0]).hypot(end[1] - center[1]);
    if (end_radius_m - radius_m).abs() > EPS_ARC_RADIUS_M {
        return None;
    }
    let angle_m = |point: [f64; 2]| (point[1] - center[1]).atan2(point[0] - center[0]);
    let from_angle_m = angle_m(from);
    let sweep_m = (angle_m(to) - from_angle_m).rem_euclid(TAU);
    if sweep_m <= SKETCH_ANGLE_TOLERANCE {
        return None;
    }
    let middle_angle_m = from_angle_m + sweep_m / 2.0;
    let middle = project([
        center[0] + radius_m * middle_angle_m.cos(),
        center[1] + radius_m * middle_angle_m.sin(),
    ])?;
    let (from_ir, to_ir) = (project(from)?, project(to)?);
    let angle = |point: Point2| (point.v - center_ir.v).atan2(point.u - center_ir.u);
    let (from_angle, to_angle, middle_angle) = (angle(from_ir), angle(to_ir), angle(middle));
    let counterclockwise =
        (middle_angle - from_angle).rem_euclid(TAU) < (to_angle - from_angle).rem_euclid(TAU);
    let (start_angle, end_angle) = if counterclockwise {
        (from_angle, to_angle)
    } else {
        (to_angle, from_angle)
    };
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
