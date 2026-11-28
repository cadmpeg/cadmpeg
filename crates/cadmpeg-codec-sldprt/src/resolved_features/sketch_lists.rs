//! The point and curve record lists of `sgSketch` objects
//! (`docs/formats/sldprt.md`, "Sketch objects").
//!
//! An `sgSketch` object writes a 14-byte header (a little-endian u32 point
//! count *N*, u16 `0`, a u16, u16 `0` and a little-endian u32), *N* point
//! records, a little-endian u32 and its curve lists. In a curve record whose
//! selector has bit 0 clear, other than the current-prefix 92-byte
//! selector-4 line with locus `05 00 01 00` that §2 reads, the endpoint and
//! centre fields are zero-based indices into the point records of its own
//! sketch, and the sense word selects a line, a counterclockwise arc or a
//! clockwise arc. A curve record of state `0` stands outside the sketch's
//! profile. A marker whose record is one of these records takes its geometry
//! from the record (`docs/formats/sldprt.md` §2, "A marker whose record is a
//! point record or a curve record").

use super::{SKETCH_ANGLE_TOLERANCE, SKETCH_MARKER};
use crate::brep::graph::LINEAR_RESOLUTION_MM;
use crate::layout::{
    sketch_arc_record as arc, sketch_arc_record_legacy as legacy_arc, sketch_line_record as line,
    sketch_record_head as head,
};
use crate::loss::SldprtLossCode;
use crate::records::{FeatureInputLane, SketchInputEntity};
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use std::collections::{BTreeMap, HashSet};
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
// resource-cap: MAX_SKETCH_POINTS = 2
const MAX_SKETCH_POINTS: usize = 4096;
/// Where a record's fields start after a head with its eight zero bytes.
const CURRENT_FIELDS: usize = arc::START;
/// Where a record's fields start in files before `swVersion` 11000.
const LEGACY_FIELDS: usize = legacy_arc::START;
/// Where a record's u16 selector stands.
const SELECTOR: usize = head::SELECTOR;
/// The selector bit of the curve records of open item EV-27.
const SELECTOR_OPEN_BIT: u16 = 1;
/// The selector bit of a construction curve.
const SELECTOR_CONSTRUCTION_BIT: u16 = 8;
/// Where a record's u16 state stands.
const STATE: usize = head::STATE;
/// The state of a curve record outside the sketch's profile.
const STATE_OUTSIDE_PROFILE: u16 = 0;
/// The selectors of the curve records whose kind [`framed_curve`] and
/// [`SketchRecordLists::listed_curve`] decide.
const ROSTER_SELECTORS: [u16; 2] = [4, 12];
/// The locus at +23 of a geometry curve record.
const OBJECT_INDEX_LOCUS: [u8; 4] = [0x05, 0x00, 0x01, 0x00];
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
pub(crate) struct SketchRecordLists {
    lists: Vec<SketchRecordList>,
    /// Record offset to list index and place.
    records: BTreeMap<usize, (usize, RecordPlace)>,
}

impl SketchRecordLists {
    /// Find the `sgSketch` record lists of `payload`. A curve record after
    /// the first curve list belongs to the nearest sketch whose point records
    /// precede it.
    pub(crate) fn new(payload: &[u8]) -> Self {
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

    /// The offsets of the curve records that start in `range`.
    pub(super) fn curves_in(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = usize> + '_ {
        self.records
            .range(range)
            .filter(|(_, (_, place))| *place != RecordPlace::Point)
            .map(|(&offset, _)| offset)
    }

    /// Whether a curve record starts at `offset`.
    pub(crate) fn is_curve(&self, payload: &[u8], offset: usize) -> bool {
        matches!(
            self.records.get(&offset),
            Some((_, RecordPlace::FirstCurveList | RecordPlace::LaterCurveList))
        ) && curve_record(payload, offset)
    }

    /// The form of the curve record of a sketch's first curve list that
    /// starts at `offset`, when it is in the current layout with selector `4`
    /// or `12` and [`curve_form`] reads it as a line or an arc.
    pub(super) fn listed_curve(&self, payload: &[u8], offset: usize) -> Option<CurveForm> {
        let &(_, RecordPlace::FirstCurveList) = self.records.get(&offset)? else {
            return None;
        };
        let selector = View::u16_le_at(payload, offset.checked_add(SELECTOR)?)?;
        if curve_fields(payload, offset)? != offset.checked_add(CURRENT_FIELDS)?
            || !ROSTER_SELECTORS.contains(&selector)
        {
            return None;
        }
        curve_form(payload, offset)
    }

    /// The kind of the curve record of a list that starts at `offset`,
    /// decided from the record's own shape ([`framed_curve`]).
    pub(super) fn listed_curve_kind(&self, payload: &[u8], offset: usize) -> Option<FramedCurve> {
        match self.record(offset)? {
            ListedRecord::Curve { .. } => framed_curve(payload, offset).map(|(_, kind)| kind),
            ListedRecord::Point => None,
        }
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

/// The ids of the markers of `lane` whose record is a curve record of an
/// `sgSketch` object. Such a marker is a curve of its sketch, whatever its
/// u32 at +17, and states no relation.
pub(crate) fn curve_record_markers(lane: &FeatureInputLane) -> HashSet<&str> {
    let lists = SketchRecordLists::new(&lane.native_payload);
    lane.sketch_entities
        .iter()
        .filter(|marker| {
            usize::try_from(marker.offset())
                .is_ok_and(|offset| lists.is_curve(&lane.native_payload, offset))
        })
        .map(SketchInputEntity::id)
        .collect()
}

/// The sketch whose first point record is `heads[first]`: the 14-byte header
/// before it states *N*, the *N* heads from there are point records (or
/// records projected from outside the sketch), and the next head is a curve
/// record or a record projected from outside the sketch. The curve list is
/// the run of curve records and projected records from there, up to its last
/// curve record.
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
    let run = heads
        .get(end..)?
        .iter()
        .copied()
        .take_while(|&curve| curve_record(payload, curve) || external_record(payload, curve))
        .collect::<Vec<_>>();
    // Projected records stand among the curve records; the list ends at its
    // last curve record.
    let last = run
        .iter()
        .rposition(|&curve| curve_record(payload, curve))?;
    let curves = run.get(..=last)?.to_vec();
    Some(SketchRecordList {
        points: points.to_vec(),
        curves,
    })
}

/// The offsets of every record head: `ff ff`, the schema version `1f 00` or
/// `07 00`, and f32 `-1` at +13 and at +31.
fn record_heads(payload: &[u8]) -> Vec<usize> {
    let Some(end) = payload.len().checked_sub(1) else {
        return Vec::new();
    };
    memchr::memchr_iter(0xff, &payload[..end])
        .filter(|&offset| payload[offset + 1] == 0xff && record_head(payload, offset))
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

/// The kind of a curve record from its own shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FramedCurve {
    /// Type `0`.
    Line,
    /// Sense `1` or `-1` in its arc layout with equal endpoint indices.
    Circle,
    /// Sense `1` or `-1` in its arc layout with distinct endpoint indices.
    Arc,
}

/// The kind of the curve record at `offset` in the current layout with a
/// roster selector: a low sense half of `0` is a line; sense `1` or `-1`
/// is an arc when four i32 `-2` follow the centre index.
fn framed_curve(payload: &[u8], offset: usize) -> Option<(usize, FramedCurve)> {
    let fields = curve_fields(payload, offset)?;
    if fields != offset.checked_add(CURRENT_FIELDS)?
        || !ROSTER_SELECTORS.contains(&View::u16_le_at(payload, offset.checked_add(SELECTOR)?)?)
    {
        return None;
    }
    match View::i32_le_at(payload, fields.checked_add(SENSE)?)? {
        sense if sense & LINE_TYPE_MASK == 0 => Some((fields, FramedCurve::Line)),
        COUNTERCLOCKWISE | CLOCKWISE => {
            let tail = fields.checked_add(MINUS_TWO)?;
            if !payload
                .get(tail..tail.checked_add(4 * I32_MINUS_TWO.len())?)?
                .chunks_exact(4)
                .all(|word| word == I32_MINUS_TWO)
            {
                return None;
            }
            let start = View::u16_le_at(payload, fields)?;
            let end = View::u16_le_at(payload, fields.checked_add(2)?)?;
            Some((
                fields,
                if start == end {
                    FramedCurve::Circle
                } else {
                    FramedCurve::Arc
                },
            ))
        }
        _ => None,
    }
}

/// The form of the curve record at `offset`, in either head layout, when its
/// selector has bit 0 clear: a line when the low half of its sense word is
/// `0`, its endpoint indices differ and it is not the object-index line that
/// §2 reads ([`object_index_line`]), and an arc when the word is `1` or `-1`
/// and the centre index is followed by i32 `-2`.
pub(super) fn curve_form(payload: &[u8], offset: usize) -> Option<CurveForm> {
    let fields = curve_fields(payload, offset)?;
    let selector = View::u16_le_at(payload, offset.checked_add(SELECTOR)?)?;
    if selector & SELECTOR_OPEN_BIT != 0 {
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
        && !(object_index_line(payload, offset, fields, selector)
            && super::endpoints::wide_indexed_curve_endpoint_indices(payload, offset).is_some()))
    .then_some(CurveForm::Line { start, end })
}

/// Whether the record at `offset` has locus `05 00 01 00` at +23.
fn object_index_locus(payload: &[u8], offset: usize) -> bool {
    offset.checked_add(head::LOCUS).is_some_and(|at| {
        at.checked_add(OBJECT_INDEX_LOCUS.len())
            .and_then(|end| payload.get(at..end))
            == Some(&OBJECT_INDEX_LOCUS)
    })
}

/// Whether the record at `offset` is a current-prefix 92-byte line with
/// selector `4` and locus `05 00 01 00`, whose endpoint values plus one are
/// object indices of point markers when §2 reads it.
fn object_index_line(payload: &[u8], offset: usize, fields: usize, selector: u16) -> bool {
    let prefix_end = offset.checked_add(SKETCH_MARKER.len());
    let end = offset.checked_add(line::RECORD_ID + size_of::<u32>());
    selector == 4
        && Some(fields) == offset.checked_add(CURRENT_FIELDS)
        && prefix_end.and_then(|end| payload.get(offset..end)) == Some(SKETCH_MARKER)
        && object_index_locus(payload, offset)
        && end.is_some_and(|end| end == payload.len() || record_head(payload, end))
}

/// The loss of a curve record on locus `05 00 01 00` that [`curve_form`]
/// reads: its indices are taken as zero-based point-record positions.
pub(super) fn point_record_index_basis_loss(payload: &[u8], offset: usize) -> Option<LossNote> {
    curve_form(payload, offset)?;
    object_index_locus(payload, offset).then(|| {
        SldprtLossCode::SketchPointRecordIndexBasisAssumed.note(format!(
            "curve record at {offset}: locus 05 00 01 00 uses assumed zero-based point-record positions in its sgSketch for endpoint and centre indices",
        ))
    })
}

/// Whether the record at `offset` is a current-layout line that [`curve_form`]
/// reads and that has an identifier trailer: u32 `1` at +84, or u32 `0` at
/// +84 and an identity other than `0` and null at +88 (§2.3).
pub(super) fn identifier_trailer_line(payload: &[u8], offset: usize) -> bool {
    let word = |at: usize| View::u32_le_at(payload, offset.checked_add(at)?);
    curve_fields(payload, offset) == offset.checked_add(CURRENT_FIELDS)
        && matches!(curve_form(payload, offset), Some(CurveForm::Line { .. }))
        && match (word(line::TRAILER), word(line::RECORD_ID)) {
            (Some(1), Some(_)) => true,
            (Some(0), Some(identity)) => identity != 0 && identity != u32::MAX,
            _ => false,
        }
}

/// Whether bit 3 of the selector of the record at `offset` is set: the
/// record is construction geometry.
pub(super) fn construction_record(payload: &[u8], offset: usize) -> bool {
    offset
        .checked_add(SELECTOR)
        .and_then(|at| View::u16_le_at(payload, at))
        .is_some_and(|selector| selector & SELECTOR_CONSTRUCTION_BIT != 0)
}

/// The entity a listed record gives the marker that stands on it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ListedEntity {
    pub(super) geometry: SketchGeometry,
    /// The stream offsets of the point records a line or an arc runs
    /// between, the first at the start of [`Self::geometry`].
    pub(super) endpoints: Option<[usize; 2]>,
    /// Bit 3 of the curve selector, when the record is a curve.
    pub(super) construction: Option<bool>,
    /// The record is a curve record of state `0`: a curve outside the
    /// sketch's profile.
    pub(super) outside_profile: bool,
    /// The record is a curve record on locus `05 00 01 00`
    /// ([`point_record_index_basis_loss`]).
    pub(super) index_basis_loss: Option<LossNote>,
}

/// Why the curve record of a marker gives it no entity, so that the marker
/// keeps what another reading gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChordConflict {
    /// A point record the record indexes writes no planar coordinates, or an
    /// index lies past the point records.
    PointUnread,
    /// The record's endpoints and centre define no arc.
    NoArc,
    /// The record's arc, placed in the sketch, ends off its endpoint point
    /// records.
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
            Self::NoArc => f.write_str("states an arc whose endpoints and centre define no arc"),
            Self::MissesChord => f.write_str(
                "states an arc that, placed in the sketch, ends off its endpoint point records",
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

/// The entity the listed record at `offset` gives its marker, read by
/// point-record position: a point record's point, whatever its u32 at +17,
/// or the line, arc or full circle of a curve record whose selector has
/// bit 0 clear, between the point records at its indices, with selector bit 3
/// marking construction and state `0` a curve outside the profile. `owns`
/// admits the point records a curve record may index: those of the marker's
/// own feature object. `project` maps a coordinate pair in metres to sketch
/// coordinates.
///
/// `None` when the offset starts no listed record, a point record writes no
/// planar coordinates, a curve record indexes a point record `owns` does not
/// admit, a line's point records write no planar coordinates, or the record
/// states a line [`curve_form`] does not read. The conflict when the record
/// states another form that gives no entity: an arc whose point records are
/// not all read, whose endpoints and centre define no arc, or whose arc ends
/// off its endpoint point records by more than the linear resolution after
/// the projection; and a form [`curve_form`] does not read.
pub(super) fn listed_entity(
    payload: &[u8],
    lists: &SketchRecordLists,
    offset: usize,
    owns: &impl Fn(usize) -> bool,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<Result<ListedEntity, ChordConflict>> {
    read_listed_entity(payload, lists, offset, owns, project, ChordTest::Required)
}

/// The entity [`listed_entity`] reads at `offset` from the point records of
/// any feature object, where an arc record that it refuses with
/// [`ChordConflict::MissesChord`] reads all the same, without endpoints: the
/// arc about its centre point record from its start point record, swept in
/// its sense to the direction of its end point record.
pub(super) fn off_chord_arc(
    payload: &[u8],
    lists: &SketchRecordLists,
    offset: usize,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
) -> Option<ListedEntity> {
    read_listed_entity(
        payload,
        lists,
        offset,
        &|_| true,
        project,
        ChordTest::Waived,
    )?
    .ok()
}

/// Whether an arc record must end on its endpoint point records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChordTest {
    /// An arc that ends off them is [`ChordConflict::MissesChord`].
    Required,
    /// An arc that ends off them reads without endpoints.
    Waived,
}

fn read_listed_entity(
    payload: &[u8],
    lists: &SketchRecordLists,
    offset: usize,
    owns: &impl Fn(usize) -> bool,
    project: &impl Fn([f64; 2]) -> Option<Point2>,
    chord: ChordTest,
) -> Option<Result<ListedEntity, ChordConflict>> {
    let ListedRecord::Curve { sketch, .. } = lists.record(offset)? else {
        let position = project(point_coordinates(payload, offset)?)?;
        return Some(Ok(ListedEntity {
            geometry: SketchGeometry::try_from(SketchGeometryDefinition::Point { position })
                .ok()?,
            endpoints: None,
            construction: None,
            outside_profile: false,
            index_basis_loss: None,
        }));
    };
    let selector = View::u16_le_at(payload, offset.checked_add(SELECTOR)?)?;
    let construction = Some(selector & SELECTOR_CONSTRUCTION_BIT != 0);
    let outside_profile =
        View::u16_le_at(payload, offset.checked_add(STATE)?)? == STATE_OUTSIDE_PROFILE;
    let entity = |geometry, endpoints| {
        Some(Ok(ListedEntity {
            geometry,
            endpoints,
            construction,
            outside_profile,
            index_basis_loss: point_record_index_basis_loss(payload, offset),
        }))
    };
    let (start, end, center, counterclockwise) = match curve_form(payload, offset) {
        None => return unread_form_conflict(payload, offset).map(Err),
        Some(CurveForm::Line { start, end }) => {
            let start = sketch.point(start).filter(|&at| owns(at))?;
            let end = sketch.point(end).filter(|&at| owns(at))?;
            let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: project(point_coordinates(payload, start)?)?,
                end: project(point_coordinates(payload, end)?)?,
            })
            .ok()?;
            return entity(geometry, Some([start, end]));
        }
        Some(CurveForm::Arc {
            start,
            end,
            center,
            counterclockwise,
        }) => (start, end, center, counterclockwise),
    };
    let (Some(start_at), Some(end_at), Some(center_at)) =
        (sketch.point(start), sketch.point(end), sketch.point(center))
    else {
        return Some(Err(ChordConflict::PointUnread));
    };
    if ![start_at, end_at, center_at].into_iter().all(owns) {
        return None;
    }
    let (Some(start_m), Some(end_m), Some(center_m)) = (
        point_coordinates(payload, start_at),
        point_coordinates(payload, end_at),
        point_coordinates(payload, center_at),
    ) else {
        return Some(Err(ChordConflict::PointUnread));
    };
    if start == end {
        return match arc_geometry(center_m, start_m, end_m, ArcSweep::Full, project) {
            Some(circle) => entity(circle, None),
            None => Some(Err(ChordConflict::NoArc)),
        };
    }
    let (Some(start_ir), Some(end_ir)) = (project(start_m), project(end_m)) else {
        return Some(Err(ChordConflict::PointUnread));
    };
    let sweep = if counterclockwise {
        ArcSweep::Counterclockwise
    } else {
        ArcSweep::Clockwise
    };
    let Some(arc) = arc_geometry(center_m, start_m, end_m, sweep, project) else {
        return Some(Err(ChordConflict::NoArc));
    };
    let SketchGeometryDefinition::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = *arc.definition()
    else {
        return Some(Err(ChordConflict::NoArc));
    };
    let (center, radius) = (center.get(), radius.get());
    let at = |angle: f64| {
        Point2::new(
            center.u + radius * angle.cos(),
            center.v + radius * angle.sin(),
        )
    };
    let near = |left: Point2, right: Point2| {
        (left.u - right.u).hypot(left.v - right.v) <= LINEAR_RESOLUTION_MM
    };
    let (first, last) = (at(start_angle.get()), at(end_angle.get()));
    let endpoints = if near(first, start_ir) && near(last, end_ir) {
        [start_at, end_at]
    } else if near(first, end_ir) && near(last, start_ir) {
        [end_at, start_at]
    } else if chord == ChordTest::Waived {
        return entity(arc, None);
    } else {
        return Some(Err(ChordConflict::MissesChord));
    };
    entity(arc, Some(endpoints))
}

/// The conflict of the listed curve record at `offset` with a line that a
/// reading other than [`listed_entity`] gave its marker: [`ChordConflict::Unchecked`]
/// for an arc record and the form conflict for a record whose form
/// [`curve_form`] does not read. `None` when the offset starts no listed
/// curve record or the record states a line.
pub(crate) fn unchecked_record_conflict(
    payload: &[u8],
    lists: &SketchRecordLists,
    offset: usize,
) -> Option<ChordConflict> {
    let ListedRecord::Curve { .. } = lists.record(offset)? else {
        return None;
    };
    match curve_form(payload, offset) {
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
