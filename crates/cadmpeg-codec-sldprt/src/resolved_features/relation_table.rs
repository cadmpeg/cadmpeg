// SPDX-License-Identifier: Apache-2.0
//! Per-sketch relation tables.
//!
//! A sketch block in a feature-input lane is a chain of counted ARRAYS of
//! marker records: points, lines, arcs, then other curves. The array's `u32`
//! count sits 14 bytes before its first record, and ten bytes precede every
//! record: a `u16` external flag, the `u32` contour ordinal and a `u32` id.
//! External-reference records (flag 1) hold slots like local ones.
//!
//! After the block comes its RELATION TABLE. Each row is:
//!
//! | bytes | field |
//! |---|---|
//! | `u32` | native relation code (`SketchRelationKind::from_native_code`) |
//! | `02 00` | constant |
//! | `u16` | row flag, 0 or 1 |
//! | `00 00 fe ff 00 00` | constant |
//! | 12 bytes each | operand handle cells |
//!
//! A handle cell is a class tag (or an inline class declaration), the `u16`
//! slot in the operand kind's array, `ff ff ff ff` and four zero bytes. A
//! local point record lists the curves that end at it in the same cells, which
//! is how the line and arc tags are learned without reading a relation code.
//!
//! The code is the native relation taxonomy of
//! `SketchRelationKind::from_native_code`. A flag-0 row's solved geometry
//! satisfies its relation; what flag 1 marks is open (`sldprt-open-items.md`
//! DI-45), so a flag-1 row is retained and never typed.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::nonblank_literal;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchCoordinateAxis,
    SketchEntity, SketchEntityId, SketchGeometryDefinition, SketchId, SketchLocus,
    SketchNativeOperand, SketchSameCoordinate,
};

use super::markers::sketch_marker_prefix_at;
use crate::records::{FeatureInputLane, SketchInputEntity};

const ROW_HEADER_TAIL: [u8; 10] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfe, 0xff, 0x00, 0x00];
const ARC_TAIL: [u8; 16] = [
    0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff, 0xfe, 0xff, 0xff, 0xff,
];
const MAX_ARRAY: usize = 100_000;
const MAX_OPERANDS: usize = 16;

/// The kind of a sketch block array, and of a handle cell's operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ArrayKind {
    Point,
    Line,
    Arc,
}

/// A handle cell's class tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HandleTag {
    /// `0x8000 | class index`.
    Class(u16),
    /// `ff 7f` and a `u32` class index.
    Wide(u32),
    /// An inline declaration of the named class.
    Declared(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowOperand {
    pub(crate) tag: HandleTag,
    pub(crate) slot: u16,
}

/// One relation-table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelationRow {
    pub(crate) offset: usize,
    pub(crate) code: u32,
    pub(crate) flag: u16,
    pub(crate) operands: Vec<RowOperand>,
}

/// One sketch block's arrays (record offsets by slot) and its table rows.
#[derive(Debug, Clone, Default)]
pub(crate) struct SketchTable {
    pub(crate) points: Vec<usize>,
    pub(crate) lines: Vec<usize>,
    pub(crate) arcs: Vec<usize>,
    pub(crate) rows: Vec<RelationRow>,
}

impl SketchTable {
    pub(crate) fn record(&self, kind: ArrayKind, slot: u16) -> Option<usize> {
        let array = match kind {
            ArrayKind::Point => &self.points,
            ArrayKind::Line => &self.lines,
            ArrayKind::Arc => &self.arcs,
        };
        array.get(usize::from(slot)).copied()
    }
}

/// The identity of the sketch constraint a relation row projects to. The row
/// is read from its lane's payload and is not a native arena record, so the
/// constraint names it by this identity and carries no `native_ref`.
pub(crate) fn row_id(lane_key: &str, offset: usize) -> String {
    format!("sldprt:model:sketch-constraint#relation-row:{lane_key}:{offset}")
}

fn marker_offsets(payload: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut offset = 0;
    while let Some(found) = payload
        .get(offset..)
        .and_then(|rest| rest.windows(2).position(|pair| pair == [0xff, 0xff]))
    {
        let at = offset + found;
        if marker_at(payload, at) {
            out.push(at);
        }
        offset = at + 1;
    }
    out
}

/// A marker record: one of the three prefixes, `ff ff ff ff ?? ?? ff ff` at
/// `+5`, and `f32` −1 at `+13`. The two middle bytes are `ff ff` in most
/// records and `04 00` in construction lines kept in a line array.
fn marker_at(payload: &[u8], offset: usize) -> bool {
    sketch_marker_prefix_at(payload, offset)
        && payload.get(offset + 5..offset + 9) == Some(&[0xff; 4])
        && payload.get(offset + 11..offset + 13) == Some(&[0xff; 2])
        && payload.get(offset + 13..offset + 17) == Some(&[0x00, 0x00, 0x80, 0xbf])
}

fn curve_body(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 48..offset + 56) == Some(&1.0f64.to_le_bytes())
        && payload.get(offset + 72..offset + 80) == Some(&(-1.0f64).to_le_bytes())
}

const POINT_TAGS: [[u8; 2]; 4] = [[0x1e, 0], [0x1a, 0], [0x16, 0], [0x5e, 0]];

fn record_kind(payload: &[u8], offset: usize) -> Option<ArrayKind> {
    if curve_body(payload, offset) {
        if payload.get(offset + 86..offset + 102) == Some(&ARC_TAIL) {
            return Some(ArrayKind::Arc);
        }
        return (View::u16_le_at(payload, offset + 80) == Some(0)).then_some(ArrayKind::Line);
    }
    let tag64 = payload.get(offset + 64..offset + 66)?;
    let tag56 = payload.get(offset + 56..offset + 58)?;
    (POINT_TAGS.iter().any(|tag| tag == tag64) || tag56 == [0x1e, 0] || tag56 == [0x1a, 0])
        .then_some(ArrayKind::Point)
}

fn external(payload: &[u8], offset: usize) -> bool {
    offset >= 10 && View::u16_le_at(payload, offset - 10) == Some(1)
}

/// The kind an array's non-external records agree on, or `None`.
fn array_kind(payload: &[u8], records: &[usize]) -> Option<ArrayKind> {
    let mut kind = None;
    for &record in records {
        if external(payload, record) {
            continue;
        }
        let this = record_kind(payload, record)?;
        match kind {
            None => kind = Some(this),
            Some(seen) if seen != this => return None,
            Some(_) => {}
        }
    }
    kind
}

/// Handle cells from `offset`: at most [`MAX_OPERANDS`], ending at the first
/// bytes that are not a cell. Returns the cells and the offset after them.
pub(crate) fn handle_cells(payload: &[u8], mut offset: usize) -> (Vec<RowOperand>, usize) {
    let mut cells = Vec::new();
    while cells.len() < MAX_OPERANDS {
        let Some(tag) = View::u16_le_at(payload, offset) else {
            break;
        };
        let (tag, slot_at) = match tag {
            0xffff => {
                let Some(length) = View::u16_le_at(payload, offset + 4).map(usize::from) else {
                    break;
                };
                let Some(name) = payload.get(offset + 6..offset + 6 + length) else {
                    break;
                };
                if name.is_empty()
                    || !name
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    break;
                }
                (
                    HandleTag::Declared(String::from_utf8_lossy(name).into_owned()),
                    offset + 6 + length,
                )
            }
            0x7fff => {
                let Some(index) = View::u32_le_at(payload, offset + 2) else {
                    break;
                };
                (HandleTag::Wide(index), offset + 6)
            }
            tag if tag & 0x8000 != 0 => (HandleTag::Class(tag), offset + 2),
            _ => break,
        };
        if payload.get(slot_at + 2..slot_at + 10)
            != Some(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00])
        {
            break;
        }
        let Some(slot) = View::u16_le_at(payload, slot_at) else {
            break;
        };
        cells.push(RowOperand { tag, slot });
        offset = slot_at + 10;
    }
    (cells, offset)
}

fn rows_between(payload: &[u8], start: usize, end: usize) -> Vec<RelationRow> {
    let mut rows = Vec::new();
    let end = end.min(payload.len());
    let mut offset = start;
    while offset + 14 <= end {
        let header = payload.get(offset + 4..offset + 14);
        let candidate = header.is_some_and(|header| {
            header[..2] == ROW_HEADER_TAIL[..2]
                && matches!(header[2], 0 | 1)
                && header[3..] == ROW_HEADER_TAIL[3..]
        });
        if candidate {
            let code = View::u32_le_at(payload, offset).unwrap_or(0);
            let flag = View::u16_le_at(payload, offset + 6).unwrap_or(0);
            if (1..=255).contains(&code) {
                let (operands, after) = handle_cells(payload, offset + 14);
                rows.push(RelationRow {
                    offset,
                    code,
                    flag,
                    operands,
                });
                offset = after.max(offset + 14);
                continue;
            }
        }
        offset += 1;
    }
    rows
}

/// Every sketch block of a lane, with the relation rows that follow it.
///
/// Arrays chain while each record's count field names a run of records that
/// stays inside the marker list; a point array starts a new sketch, and every
/// later array of the chain (of any kind) belongs to it. A sketch's rows are
/// read from after its last record to the next marker record of the lane: a
/// block this reading does not segment must not lend its rows to the sketch
/// before it.
pub(crate) fn lane_tables(payload: &[u8]) -> Vec<SketchTable> {
    let markers = marker_offsets(payload);
    let mut sketches: Vec<SketchTable> = Vec::new();
    // The marker index of each sketch's last record.
    let mut last_records: Vec<usize> = Vec::new();
    let mut index = 0;
    while index < markers.len() {
        let mut chained = false;
        let mut cursor = index;
        let mut in_sketch = false;
        while cursor < markers.len() {
            let Some(count) = markers[cursor]
                .checked_sub(14)
                .and_then(|at| View::u32_le_at(payload, at))
            else {
                break;
            };
            let Ok(count) = usize::try_from(count) else {
                break;
            };
            if count == 0 || count > MAX_ARRAY || cursor + count > markers.len() {
                break;
            }
            let records = &markers[cursor..cursor + count];
            match array_kind(payload, records) {
                Some(ArrayKind::Point) => {
                    sketches.push(SketchTable {
                        points: records.to_vec(),
                        ..SketchTable::default()
                    });
                    last_records.push(cursor + count - 1);
                    in_sketch = true;
                }
                kind => {
                    if let (true, Some(sketch), Some(last)) =
                        (in_sketch, sketches.last_mut(), last_records.last_mut())
                    {
                        let array = match kind {
                            Some(ArrayKind::Line) => Some(&mut sketch.lines),
                            Some(ArrayKind::Arc) => Some(&mut sketch.arcs),
                            _ => None,
                        };
                        if let Some(array) = array {
                            if array.is_empty() {
                                *array = records.to_vec();
                            }
                        }
                        *last = cursor + count - 1;
                    }
                }
            }
            chained = true;
            cursor += count;
        }
        index = if chained { cursor } else { index + 1 };
    }
    for (sketch, &last) in sketches.iter_mut().zip(&last_records) {
        let start = markers[last];
        let end = markers.get(last + 1).copied().unwrap_or(payload.len());
        sketch.rows = rows_between(payload, start, end);
    }
    sketches
}

fn point_cells(payload: &[u8], record: usize) -> Vec<RowOperand> {
    for tag_at in [64usize, 56] {
        if payload.get(record + tag_at..record + tag_at + 2) == Some(&[0x1e, 0]) {
            return handle_cells(payload, record + tag_at + 22).0;
        }
    }
    Vec::new()
}

fn curve_ends(payload: &[u8], record: usize) -> Option<[u16; 2]> {
    curve_body(payload, record).then_some([
        View::u16_le_at(payload, record + 64)?,
        View::u16_le_at(payload, record + 66)?,
    ])
}

const POINT_HANDLE: &str = "sgPointHandle";
const LINE_HANDLE: &str = "sgLineHandle";
const ARC_HANDLE: &str = "sgArcHandle";

/// The operand kind of each class tag in a part's lanes.
///
/// 1. A point record's cell `(tag, k)`, where curve `k` of a kind ends at that
///    point, votes `tag` for that kind (lines and arcs).
/// 2. Ascending class tags follow the first declarations of the handle classes,
///    because a class tag is `0x8000` or-ed with the class's archive index;
///    unknown tags between voted ones take the declared classes in order when the
///    counts match. Only when every handle class is declared in one lane.
/// 3. Otherwise, when exactly one other class tag occurs in the rows, it is the
///    point tag.
pub(crate) fn tag_kinds<'a>(
    lanes: impl IntoIterator<Item = (&'a [u8], &'a [SketchTable])> + Clone,
) -> HashMap<HandleTag, ArrayKind> {
    let mut votes: HashMap<HandleTag, BTreeMap<u8, usize>> = HashMap::new();
    for (payload, sketches) in lanes.clone() {
        for sketch in sketches {
            let mut ends: HashMap<(ArrayKind, u16), [u16; 2]> = HashMap::new();
            for (kind, array) in [
                (ArrayKind::Line, &sketch.lines),
                (ArrayKind::Arc, &sketch.arcs),
            ] {
                for (slot, &record) in array.iter().enumerate() {
                    if let (Ok(slot), Some(pair)) =
                        (u16::try_from(slot), curve_ends(payload, record))
                    {
                        ends.insert((kind, slot), pair);
                    }
                }
            }
            for (position, &record) in sketch.points.iter().enumerate() {
                let Ok(position) = u16::try_from(position) else {
                    continue;
                };
                for cell in point_cells(payload, record) {
                    for (kind, key) in [(ArrayKind::Line, 1u8), (ArrayKind::Arc, 2u8)] {
                        if ends
                            .get(&(kind, cell.slot))
                            .is_some_and(|pair| pair.contains(&position))
                        {
                            *votes
                                .entry(cell.tag.clone())
                                .or_default()
                                .entry(key)
                                .or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    let mut kinds = HashMap::new();
    for (tag, counts) in votes {
        let total: usize = counts.values().sum();
        if let Some((&key, &best)) = counts.iter().max_by_key(|(_, &count)| count) {
            if best >= 2 && best * 10 >= total * 9 {
                kinds.insert(
                    tag,
                    if key == 1 {
                        ArrayKind::Line
                    } else {
                        ArrayKind::Arc
                    },
                );
            }
        }
    }
    let mut unknown = std::collections::BTreeSet::new();
    for (_, sketches) in lanes.clone() {
        for row in sketches.iter().flat_map(|sketch| &sketch.rows) {
            for operand in &row.operands {
                if let HandleTag::Class(tag) = operand.tag {
                    if !kinds.contains_key(&operand.tag) {
                        unknown.insert(tag);
                    }
                }
            }
        }
    }
    if unknown.is_empty() {
        return kinds;
    }
    let mut declared: Vec<(usize, usize, String)> = Vec::new();
    for (lane, (payload, _)) in lanes.into_iter().enumerate() {
        for (at, name) in handle_declarations(payload) {
            if !declared.iter().any(|(_, _, seen)| *seen == name) {
                declared.push((lane, at, name));
            }
        }
    }
    let lanes_declaring = declared
        .iter()
        .map(|(lane, _, _)| *lane)
        .collect::<HashSet<_>>();
    if declared.is_empty() || lanes_declaring.len() != 1 {
        // With no single-lane declaration order to contradict it, the one
        // unknown tag left is the point's. Where an order exists, only the
        // order assigns: elimination can contradict it.
        if unknown.len() == 1 {
            if let Some(&tag) = unknown.iter().next() {
                kinds.insert(HandleTag::Class(tag), ArrayKind::Point);
            }
        }
        return kinds;
    }
    declared.sort_by_key(|(_, at, _)| *at);
    let order = declared
        .into_iter()
        .map(|(_, _, name)| name)
        .collect::<Vec<_>>();
    let class_of = |kind: ArrayKind| match kind {
        ArrayKind::Point => POINT_HANDLE,
        ArrayKind::Line => LINE_HANDLE,
        ArrayKind::Arc => ARC_HANDLE,
    };
    let mut anchors = kinds
        .iter()
        .filter_map(|(tag, kind)| match tag {
            HandleTag::Class(tag) => order
                .iter()
                .position(|name| name == class_of(*kind))
                .map(|position| (*tag, position)),
            _ => None,
        })
        .collect::<Vec<_>>();
    anchors.sort_unstable();
    if anchors.windows(2).any(|pair| pair[0].1 >= pair[1].1) {
        return kinds;
    }
    let mut bounds = vec![(None, None)];
    bounds.extend(
        anchors
            .iter()
            .map(|&(tag, position)| (Some(tag), Some(position))),
    );
    bounds.push((None, None));
    for gap in bounds.windows(2) {
        let (low_tag, low_position) = gap[0];
        let (high_tag, high_position) = gap[1];
        let tags = unknown
            .iter()
            .copied()
            .filter(|&tag| {
                low_tag.is_none_or(|low| tag > low) && high_tag.is_none_or(|high| tag < high)
            })
            .collect::<Vec<_>>();
        let first = low_position.map_or(0, |position| position + 1);
        let last = high_position.unwrap_or(order.len());
        let classes = order.get(first..last).unwrap_or_default();
        if !tags.is_empty() && tags.len() == classes.len() {
            for (tag, class) in tags.into_iter().zip(classes) {
                let kind = match class.as_str() {
                    POINT_HANDLE => Some(ArrayKind::Point),
                    LINE_HANDLE => Some(ArrayKind::Line),
                    ARC_HANDLE => Some(ArrayKind::Arc),
                    _ => None,
                };
                if let Some(kind) = kind {
                    kinds.insert(HandleTag::Class(tag), kind);
                }
            }
        }
    }
    kinds
}

/// The kind of an operand: its tag's, or its inline declaration's class.
pub(crate) fn operand_kind(
    operand: &RowOperand,
    kinds: &HashMap<HandleTag, ArrayKind>,
) -> Option<ArrayKind> {
    if let HandleTag::Declared(name) = &operand.tag {
        return match name.as_str() {
            POINT_HANDLE => Some(ArrayKind::Point),
            LINE_HANDLE => Some(ArrayKind::Line),
            ARC_HANDLE => Some(ArrayKind::Arc),
            _ => None,
        };
    }
    kinds.get(&operand.tag).copied()
}

fn handle_declarations(payload: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut offset = 0;
    while let Some(found) = payload.get(offset..).and_then(|rest| {
        rest.windows(4)
            .position(|window| window == [0xff, 0xff, 0x01, 0x00])
    }) {
        let at = offset + found;
        if let Some(length) = View::u16_le_at(payload, at + 4).map(usize::from) {
            if let Some(name) = payload.get(at + 6..at + 6 + length) {
                if name.starts_with(b"sg")
                    && name.ends_with(b"Handle")
                    && name.iter().all(u8::is_ascii_alphanumeric)
                {
                    out.push((at, String::from_utf8_lossy(name).into_owned()));
                }
            }
        }
        offset = at + 1;
    }
    out
}

/// Native coordinates (metres) of a local point record with tag `1e 00`.
fn point_coordinates(payload: &[u8], record: usize) -> Option<[f64; 2]> {
    if curve_body(payload, record) {
        return None;
    }
    for tag_at in [64usize, 56] {
        if payload.get(record + tag_at..record + tag_at + 2) == Some(&[0x1e, 0]) {
            let u = View::f64_le_at(payload, record + tag_at + 2)?;
            let v = View::f64_le_at(payload, record + tag_at + 10)?;
            return (u.is_finite() && v.is_finite()).then_some([u, v]);
        }
    }
    None
}

/// The IR loci of a curve: start, end and centre positions in sketch units.
fn curve_loci(entity: &SketchEntity) -> Vec<(SketchLocus, [f64; 2])> {
    let id = entity.id().clone();
    match entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => vec![
            (SketchLocus::Start(id.clone()), [start.u, start.v]),
            (SketchLocus::End(id), [end.u, end.v]),
        ],
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let at = |angle: f64| {
                [
                    center.u + radius.get() * angle.cos(),
                    center.v + radius.get() * angle.sin(),
                ]
            };
            vec![
                (SketchLocus::Start(id.clone()), at(start_angle.get())),
                (SketchLocus::End(id.clone()), at(end_angle.get())),
                (SketchLocus::Center(id), [center.u, center.v]),
            ]
        }
        SketchGeometryDefinition::Circle { center, .. } => {
            vec![(SketchLocus::Center(id), [center.u, center.v])]
        }
        _ => Vec::new(),
    }
}

/// The curves of one sketch block by the point slots they name: end points at
/// `+64`, `+66` and an arc's centre at `+84`. Built once per block, so a
/// point operand is resolved without scanning every curve.
struct CurveIndex {
    by_slot: HashMap<u16, Vec<usize>>,
}

impl CurveIndex {
    fn new(payload: &[u8], sketch: &SketchTable) -> Self {
        let mut by_slot = HashMap::<u16, Vec<usize>>::new();
        for &curve in sketch.lines.iter().chain(&sketch.arcs) {
            let Some(ends) = curve_ends(payload, curve) else {
                continue;
            };
            let centre = (payload.get(curve + 86..curve + 102) == Some(&ARC_TAIL))
                .then(|| View::u16_le_at(payload, curve + 84))
                .flatten();
            let mut slots = vec![ends[0], ends[1]];
            slots.extend(centre);
            slots.sort_unstable();
            slots.dedup();
            for slot in slots {
                by_slot.entry(slot).or_default().push(curve);
            }
        }
        Self { by_slot }
    }
}

/// The locus of point slot `slot` on a curve that names it, taken only when the
/// IR curve's point at that locus lies at the point record's own coordinates
/// (native metres, IR millimetres). A curve end is preferred to the point
/// entity: it binds the curve, where a point entity binds nothing else.
fn point_locus_on_curves(
    payload: &[u8],
    sketch: &SketchTable,
    curves: &CurveIndex,
    slot: u16,
    markers_by_offset: &HashMap<u64, &SketchInputEntity>,
    entity_by_ref: &HashMap<&str, &SketchEntity>,
) -> Option<SketchLocus> {
    let record = sketch.record(ArrayKind::Point, slot)?;
    let [u, v] = point_coordinates(payload, record)?;
    let target = [u * NATIVE_TO_IR, v * NATIVE_TO_IR];
    let tolerance = RELATIVE_TOLERANCE * target[0].abs().max(target[1].abs()).max(1.0);
    for &curve in curves.by_slot.get(&slot)? {
        let Some(entity) = u64::try_from(curve)
            .ok()
            .and_then(|offset| markers_by_offset.get(&offset))
            .and_then(|marker| entity_by_ref.get(marker.id()))
        else {
            continue;
        };
        for (locus, at) in curve_loci(entity) {
            if (at[0] - target[0]).abs() <= tolerance && (at[1] - target[1]).abs() <= tolerance {
                return Some(locus);
            }
        }
    }
    None
}

const NATIVE_TO_IR: f64 = 1000.0;

/// A position or length agrees with another within this fraction of the
/// coordinates it is measured among (at least 1 mm).
const RELATIVE_TOLERANCE: f64 = 1e-6;

/// Two unit directions are parallel or perpendicular within this sine or cosine.
const ANGLE: f64 = 1e-7;

/// Relation codes typed into the neutral IR from flag-0 rows. Concentric (`10`)
/// is not among them: its operand roles are not settled.
const TYPED_CODES: [u32; 14] = [4, 5, 6, 7, 8, 9, 11, 12, 13, 14, 25, 26, 27, 28];

struct Resolved<'a> {
    kind: Option<ArrayKind>,
    tag: HandleTag,
    slot: u16,
    record: Option<usize>,
    marker: Option<&'a SketchInputEntity>,
    entity: Option<SketchEntityId>,
    locus: Option<SketchLocus>,
}

impl Resolved<'_> {
    /// Whether the operand names an entity or locus of `in_sketch`'s sketch.
    fn complete(&self, in_sketch: impl Fn(&SketchEntityId) -> bool) -> bool {
        match self.kind {
            None => false,
            Some(ArrayKind::Point) => self
                .locus
                .as_ref()
                .is_some_and(|locus| in_sketch(locus_entity(locus))),
            Some(_) => self.entity.as_ref().is_some_and(in_sketch),
        }
    }
}

fn locus_entity(locus: &SketchLocus) -> &SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

fn geometry_kind(entity: &SketchEntity) -> Option<ArrayKind> {
    match entity.geometry.definition() {
        SketchGeometryDefinition::Point { .. } => Some(ArrayKind::Point),
        SketchGeometryDefinition::Line { .. } => Some(ArrayKind::Line),
        SketchGeometryDefinition::Arc { .. } | SketchGeometryDefinition::Circle { .. } => {
            Some(ArrayKind::Arc)
        }
        _ => None,
    }
}

/// The neutral definition of a flag-0 row whose operands all resolved, or
/// `None` when its code or operand kinds have no typed form here.
///
/// Not typed although the code is: tangency between two lines, which has no
/// neutral meaning, and symmetry of two lines, which `SolidWorks` holds on the
/// lines' carriers where `SketchLocus::Entity` would mirror their end points.
fn typed_definition(
    code: u32,
    operands: &[Resolved<'_>],
) -> Option<SketchConstraintDefinitionInput> {
    use ArrayKind::{Arc, Line, Point};
    use SketchConstraintDefinitionInput as Def;
    if !TYPED_CODES.contains(&code) {
        return None;
    }
    let kinds = operands
        .iter()
        .map(|operand| operand.kind)
        .collect::<Option<Vec<_>>>()?;
    let entity = |index: usize| {
        operands
            .get(index)
            .and_then(|operand| operand.entity.clone())
    };
    let locus = |index: usize| {
        operands
            .get(index)
            .and_then(|operand| operand.locus.clone())
    };
    let pick = |kind: ArrayKind| kinds.iter().position(|&seen| seen == kind);
    let others = |kind: ArrayKind| {
        kinds
            .iter()
            .enumerate()
            .filter(|(_, &seen)| seen != kind)
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };
    let same_coordinate = |axis| {
        SketchSameCoordinate::try_new(locus(0)?, locus(1)?, axis)
            .ok()
            .map(|relation| Def::SameCoordinate { relation })
    };
    match (code, kinds.as_slice()) {
        (4, [Line]) => Some(Def::Horizontal { entity: entity(0)? }),
        (5, [Line]) => Some(Def::Vertical { entity: entity(0)? }),
        (4 | 25, [Point, Point]) => same_coordinate(SketchCoordinateAxis::V),
        (5 | 26, [Point, Point]) => same_coordinate(SketchCoordinateAxis::U),
        (6, [Line | Arc, Arc] | [Arc, Line]) => Some(Def::Tangent {
            first: entity(0)?,
            second: entity(1)?,
        }),
        (7, [Line, Line]) => Some(Def::Parallel {
            first: entity(0)?,
            second: entity(1)?,
        }),
        (8, [Line, Line]) => Some(Def::Perpendicular {
            first: entity(0)?,
            second: entity(1)?,
        }),
        (27, [Line, Line]) => Some(Def::Collinear {
            first: entity(0)?,
            second: entity(1)?,
        }),
        (14, [Line, Line] | [Arc, Arc]) => Some(Def::Equal {
            first: entity(0)?,
            second: entity(1)?,
        }),
        (28, [Arc, Arc]) => Some(Def::Coradial {
            first: entity(0)?,
            second: entity(1)?,
        }),
        (9, [Point, Point]) => Some(Def::CoincidentLoci {
            loci: vec![locus(0)?, locus(1)?],
        }),
        (9, [Point, Line | Arc] | [Line | Arc, Point]) => {
            let point = pick(Point)?;
            let curve = others(Point).first().copied()?;
            Some(Def::PointOnObject {
                point: locus(point)?,
                entity: entity(curve)?,
            })
        }
        (12, [Point, Line] | [Line, Point]) => Some(Def::Midpoint {
            point: locus(pick(Point)?)?,
            entity: entity(pick(Line)?)?,
        }),
        (13, _) if kinds.len() == 3 && kinds.iter().filter(|&&kind| kind == Point).count() == 1 => {
            let curves = others(Point);
            Some(Def::AtIntersection {
                point: locus(pick(Point)?)?,
                first: entity(*curves.first()?)?,
                second: entity(*curves.get(1)?)?,
            })
        }
        (11, [Point, Point, Line]) => Some(Def::Symmetric {
            first: locus(0)?,
            second: locus(1)?,
            axis: entity(2)?,
        }),
        (11, [Arc, Arc, Line]) => Some(Def::Symmetric {
            first: SketchLocus::Entity(entity(0)?),
            second: SketchLocus::Entity(entity(1)?),
            axis: entity(2)?,
        }),
        _ => None,
    }
}

/// The IR geometry a typed definition is checked against.
struct Geometry<'a> {
    entities: HashMap<&'a SketchEntityId, &'a SketchEntity>,
}

enum Shape {
    Point([f64; 2]),
    Line([f64; 2], [f64; 2]),
    Circle([f64; 2], f64),
}

impl Geometry<'_> {
    fn shape(&self, entity: &SketchEntityId) -> Option<Shape> {
        Some(match self.entities.get(entity)?.geometry.definition() {
            SketchGeometryDefinition::Point { position } => Shape::Point([position.u, position.v]),
            SketchGeometryDefinition::Line { start, end } => {
                Shape::Line([start.u, start.v], [end.u, end.v])
            }
            SketchGeometryDefinition::Arc { center, radius, .. }
            | SketchGeometryDefinition::Circle { center, radius } => {
                Shape::Circle([center.u, center.v], radius.get())
            }
            _ => return None,
        })
    }

    fn position(&self, locus: &SketchLocus) -> Option<[f64; 2]> {
        match locus {
            SketchLocus::Entity(entity) => match self.shape(entity)? {
                Shape::Point(at) => Some(at),
                _ => None,
            },
            SketchLocus::Start(entity) | SketchLocus::End(entity) | SketchLocus::Center(entity) => {
                curve_loci(self.entities.get(entity)?)
                    .into_iter()
                    .find(|(candidate, _)| candidate == locus)
                    .map(|(_, at)| at)
            }
        }
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn direction(start: [f64; 2], end: [f64; 2]) -> Option<[f64; 2]> {
    let length = distance(start, end);
    (length > 0.0).then(|| [(end[0] - start[0]) / length, (end[1] - start[1]) / length])
}

fn line_distance(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> Option<f64> {
    let [du, dv] = direction(start, end)?;
    Some((du * (point[1] - start[1]) - dv * (point[0] - start[0])).abs())
}

/// Whether the IR geometry satisfies a typed definition: a row whose operands
/// resolved into the wrong records, or whose stored solution does not hold it,
/// stays native rather than becoming a constraint the IR contradicts.
fn holds(definition: &SketchConstraintDefinitionInput, geometry: &Geometry<'_>) -> Option<bool> {
    use SketchConstraintDefinitionInput as Def;
    let mut extent = 1.0f64;
    let mut note = |at: [f64; 2]| {
        extent = extent.max(at[0].abs()).max(at[1].abs());
        at
    };
    let mut point = |locus: &SketchLocus| geometry.position(locus).map(&mut note);
    let line = |entity: &SketchEntityId| match geometry.shape(entity)? {
        Shape::Line(start, end) => Some((start, end)),
        _ => None,
    };
    let circle = |entity: &SketchEntityId| match geometry.shape(entity)? {
        Shape::Circle(center, radius) => Some((center, radius)),
        _ => None,
    };
    let span = |pairs: &[[f64; 2]]| {
        pairs
            .iter()
            .fold(1.0f64, |most, at| most.max(at[0].abs()).max(at[1].abs()))
    };
    let tolerance = |scale: f64| RELATIVE_TOLERANCE * scale;
    Some(match definition {
        Def::Horizontal { entity } | Def::Vertical { entity } => {
            let (start, end) = line(entity)?;
            let [du, dv] = direction(start, end)?;
            (if matches!(definition, Def::Horizontal { .. }) {
                dv
            } else {
                du
            })
            .abs()
                <= ANGLE
        }
        Def::Parallel { first, second }
        | Def::Perpendicular { first, second }
        | Def::Collinear { first, second } => {
            let (a0, a1) = line(first)?;
            let (b0, b1) = line(second)?;
            let ([au, av], [bu, bv]) = (direction(a0, a1)?, direction(b0, b1)?);
            let cross = au * bv - av * bu;
            match definition {
                Def::Parallel { .. } => cross.abs() <= ANGLE,
                Def::Perpendicular { .. } => (au * bu + av * bv).abs() <= ANGLE,
                _ => {
                    cross.abs() <= ANGLE
                        && line_distance(b0, a0, a1)? <= tolerance(span(&[a0, a1, b0, b1]))
                }
            }
        }
        Def::Equal { first, second } => match (line(first), line(second)) {
            (Some((a0, a1)), Some((b0, b1))) => {
                (distance(a0, a1) - distance(b0, b1)).abs() <= tolerance(span(&[a0, a1, b0, b1]))
            }
            _ => {
                let ((ac, ar), (bc, br)) = (circle(first)?, circle(second)?);
                (ar - br).abs() <= tolerance(span(&[ac, bc]).max(ar))
            }
        },
        Def::Coradial { first, second } => {
            let ((ac, ar), (bc, br)) = (circle(first)?, circle(second)?);
            let scale = tolerance(span(&[ac, bc]).max(ar));
            distance(ac, bc) <= scale && (ar - br).abs() <= scale
        }
        Def::Tangent { first, second } => {
            let (arc, other) = if circle(first).is_some() {
                (first, second)
            } else {
                (second, first)
            };
            let (center, radius) = circle(arc)?;
            match (line(other), circle(other)) {
                (Some((start, end)), _) => {
                    (line_distance(center, start, end)? - radius).abs()
                        <= tolerance(span(&[center, start, end]).max(radius))
                }
                (None, Some((other_center, other_radius))) => {
                    let gap = distance(center, other_center);
                    let scale = tolerance(span(&[center, other_center]).max(radius));
                    (gap - radius - other_radius).abs() <= scale
                        || (gap - (radius - other_radius).abs()).abs() <= scale
                }
                _ => return None,
            }
        }
        Def::CoincidentLoci { loci } => {
            let at = loci.iter().map(&mut point).collect::<Option<Vec<_>>>()?;
            let scale = tolerance(span(&at));
            at.iter().all(|other| distance(at[0], *other) <= scale)
        }
        Def::SameCoordinate { relation } => {
            let a = point(relation.first())?;
            let b = point(relation.second())?;
            let axis = usize::from(matches!(relation.axis(), SketchCoordinateAxis::V));
            (a[axis] - b[axis]).abs() <= tolerance(span(&[a, b]))
        }
        Def::PointOnObject { point: on, entity } => {
            let at = point(on)?;
            match geometry.shape(entity)? {
                Shape::Line(start, end) => {
                    line_distance(at, start, end)? <= tolerance(span(&[at, start, end]))
                }
                Shape::Circle(center, radius) => {
                    (distance(at, center) - radius).abs()
                        <= tolerance(span(&[at, center]).max(radius))
                }
                Shape::Point(other) => distance(at, other) <= tolerance(span(&[at, other])),
            }
        }
        Def::Midpoint {
            point: middle,
            entity,
        } => {
            let at = point(middle)?;
            let (start, end) = line(entity)?;
            let half = [start[0].midpoint(end[0]), start[1].midpoint(end[1])];
            distance(at, half) <= tolerance(span(&[at, start, end]))
        }
        Def::AtIntersection {
            point: at,
            first,
            second,
        } => {
            let at = point(at)?;
            [first, second]
                .into_iter()
                .all(|curve| match geometry.shape(curve) {
                    Some(Shape::Line(start, end)) => line_distance(at, start, end)
                        .is_some_and(|gap| gap <= tolerance(span(&[at, start, end]))),
                    Some(Shape::Circle(center, radius)) => {
                        (distance(at, center) - radius).abs()
                            <= tolerance(span(&[at, center]).max(radius))
                    }
                    _ => false,
                })
        }
        Def::Symmetric {
            first,
            second,
            axis,
        } => {
            let (origin, end) = line(axis)?;
            let [du, dv] = direction(origin, end)?;
            let mirror = |at: [f64; 2]| {
                let (x, y) = (at[0] - origin[0], at[1] - origin[1]);
                let along = x * du + y * dv;
                [
                    origin[0] + 2.0 * along * du - x,
                    origin[1] + 2.0 * along * dv - y,
                ]
            };
            match (first, second) {
                (SketchLocus::Entity(a), SketchLocus::Entity(b)) if circle(a).is_some() => {
                    let ((ac, ar), (bc, br)) = (circle(a)?, circle(b)?);
                    let scale = tolerance(span(&[ac, bc, origin, end]).max(ar));
                    distance(mirror(ac), bc) <= scale && (ar - br).abs() <= scale
                }
                _ => {
                    let (a, b) = (point(first)?, point(second)?);
                    distance(mirror(a), b) <= tolerance(span(&[a, b, origin, end]))
                }
            }
        }
        _ => return None,
    })
}

/// How much of a row a projection resolved: typed, native with every operand
/// resolved, or native with an operand unresolved. A later projection replaces
/// an earlier one of the same row only when it resolves more.
fn resolution(definition: &SketchConstraintDefinitionInput) -> u8 {
    match definition {
        SketchConstraintDefinitionInput::Native { operands, .. } => u8::from(
            !operands
                .iter()
                .any(|operand| operand.native_kind.as_str().ends_with(":unresolved")),
        ),
        _ => 2,
    }
}

/// Project every relation-table row of every lane into `constraints`.
///
/// A flag-0 row of a confirmed code whose operands resolve to the sketch's
/// entities, and whose definition the IR geometry satisfies, is typed. Every
/// other row whose sketch is known is retained as a `Native` constraint carrying
/// its code, flag, and each operand's kind, slot, class tag and marker. A row
/// whose operands do not identify one sketch is left to
/// `sketch.relation-unprojected`.
pub(super) fn project_relation_tables(
    constraints: &mut Vec<SketchConstraint>,
    sketches_by_feature: &HashMap<&str, &SketchId>,
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) {
    let tables = lanes
        .iter()
        .map(|lane| lane_tables(&lane.native_payload))
        .collect::<Vec<_>>();
    if tables
        .iter()
        .all(|sketches| sketches.iter().all(|sketch| sketch.rows.is_empty()))
    {
        return;
    }
    let kinds = tag_kinds(
        lanes
            .iter()
            .zip(&tables)
            .map(|(lane, sketches)| (&lane.native_payload[..], sketches.as_slice())),
    );
    let geometry = Geometry {
        entities: sketch_entities
            .iter()
            .map(|entity| (entity.id(), entity))
            .collect(),
    };
    let mut entity_by_ref = HashMap::<&str, &SketchEntity>::new();
    for entity in sketch_entities {
        if let Some(native_ref) = entity.native_ref.as_deref() {
            entity_by_ref.entry(native_ref).or_insert(entity);
        }
    }
    let mut existing = HashMap::<String, usize>::new();
    for (index, constraint) in constraints.iter().enumerate() {
        existing
            .entry(constraint.id.as_str().to_owned())
            .or_insert(index);
    }
    for (lane, sketches) in lanes.iter().zip(&tables) {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let markers_by_offset = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.offset(), marker))
            .collect::<HashMap<_, _>>();
        // A configuration lane repeats the sketches of the same features, so the
        // sketch of a feature is read from this lane's own entities first.
        let mut lane_sketches = HashMap::<&str, BTreeMap<&SketchId, usize>>::new();
        for marker in &lane.sketch_entities {
            if let (Some(feature), Some(entity)) = (
                marker.feature_ref.as_deref(),
                entity_by_ref.get(marker.id()),
            ) {
                *lane_sketches
                    .entry(feature)
                    .or_default()
                    .entry(&entity.sketch)
                    .or_default() += 1;
            }
        }
        for sketch in sketches {
            if sketch.rows.is_empty() {
                continue;
            }
            let block_feature = block_feature(sketch, &markers_by_offset);
            let curves = CurveIndex::new(&lane.native_payload, sketch);
            for row in &sketch.rows {
                let row_identity = row_id(lane_key, row.offset);
                let earlier = existing.get(&row_identity).copied();
                if earlier
                    .is_some_and(|index| resolution(constraints[index].definition.kind()) == 2)
                {
                    continue;
                }
                let mut resolved = Vec::new();
                let mut owners = HashSet::new();
                for operand in &row.operands {
                    let kind = operand_kind(operand, &kinds);
                    let record = kind.and_then(|kind| sketch.record(kind, operand.slot));
                    let marker = record
                        .and_then(|record| u64::try_from(record).ok())
                        .and_then(|record| markers_by_offset.get(&record).copied());
                    if let Some(feature) = marker.and_then(|marker| marker.feature_ref.as_deref()) {
                        owners.insert(feature);
                    }
                    let entity = marker
                        .and_then(|marker| entity_by_ref.get(marker.id()))
                        .filter(|entity| kind.is_some() && geometry_kind(entity) == kind)
                        .map(|entity| entity.id().clone());
                    let locus = match kind {
                        Some(ArrayKind::Point) => point_locus_on_curves(
                            &lane.native_payload,
                            sketch,
                            &curves,
                            operand.slot,
                            &markers_by_offset,
                            &entity_by_ref,
                        )
                        .or_else(|| entity.clone().map(SketchLocus::Entity)),
                        Some(_) => entity.clone().map(SketchLocus::Entity),
                        None => None,
                    };
                    resolved.push(Resolved {
                        kind,
                        tag: operand.tag.clone(),
                        slot: operand.slot,
                        record,
                        marker,
                        entity,
                        locus,
                    });
                }
                let feature = match owners.into_iter().collect::<Vec<_>>()[..] {
                    [feature] => feature,
                    [] => match block_feature {
                        Some(feature) => feature,
                        None => continue,
                    },
                    _ => continue,
                };
                let lane_sketch = lane_sketches.get(feature).and_then(|counts| {
                    counts
                        .iter()
                        .max_by_key(|&(_, count)| count)
                        .map(|(&sketch, _)| sketch)
                });
                let Some(sketch_id) =
                    lane_sketch.or_else(|| sketches_by_feature.get(feature).copied())
                else {
                    continue;
                };
                let in_sketch = |entity: &SketchEntityId| {
                    geometry
                        .entities
                        .get(entity)
                        .is_some_and(|owner| &owner.sketch == sketch_id)
                };
                let complete = resolved.iter().all(|operand| operand.complete(in_sketch));
                let typed = (row.flag == 0 && complete)
                    .then(|| typed_definition(row.code, &resolved))
                    .flatten()
                    .filter(|definition| holds(definition, &geometry) == Some(true));
                let definition = match typed {
                    Some(definition) => definition,
                    None => native_definition(row, &resolved, in_sketch),
                };
                let rank = resolution(&definition);
                let Ok(definition) =
                    cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition)
                else {
                    continue;
                };
                let Ok(id) = SketchConstraintId::mint(row_identity.clone()) else {
                    continue;
                };
                let projected = SketchConstraint {
                    id,
                    sketch: sketch_id.clone(),
                    definition,
                    name: None,
                    driving: None,
                    active: None,
                    virtual_space: None,
                    visible: None,
                    orientation: None,
                    label_distance: None,
                    label_position: None,
                    metadata: None,
                    native_ref: None,
                };
                match earlier {
                    Some(index) => {
                        if rank > resolution(constraints[index].definition.kind()) {
                            constraints[index] = projected;
                        }
                    }
                    None => {
                        existing.insert(row_identity, constraints.len());
                        constraints.push(projected);
                    }
                }
            }
        }
    }
}

/// The feature most of a block's records belong to, by their markers.
fn block_feature<'a>(
    sketch: &SketchTable,
    markers_by_offset: &HashMap<u64, &'a SketchInputEntity>,
) -> Option<&'a str> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for &record in sketch
        .points
        .iter()
        .chain(&sketch.lines)
        .chain(&sketch.arcs)
    {
        let feature = u64::try_from(record)
            .ok()
            .and_then(|offset| markers_by_offset.get(&offset))
            .and_then(|marker| marker.feature_ref.as_deref());
        if let Some(feature) = feature {
            *counts.entry(feature).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(feature, _)| feature)
}

fn handle_tag_text(tag: &HandleTag) -> String {
    match tag {
        HandleTag::Class(tag) => format!("{tag:#06x}"),
        HandleTag::Wide(index) => format!("wide:{index}"),
        HandleTag::Declared(name) => name.clone(),
    }
}

/// A row retained with its code, flag, and every operand: its kind (or
/// `unknown` when the class tag has none), its slot, its class tag, and the
/// marker it resolved to. An operand that names no entity of the sketch is
/// marked `:unresolved`.
fn native_definition(
    row: &RelationRow,
    operands: &[Resolved<'_>],
    in_sketch: impl Fn(&SketchEntityId) -> bool + Copy,
) -> SketchConstraintDefinitionInput {
    let mut entities = operands
        .iter()
        .filter_map(|operand| operand.entity.clone())
        .filter(|entity| in_sketch(entity))
        .collect::<Vec<_>>();
    entities.sort();
    entities.dedup();
    let mut properties = BTreeMap::new();
    let native_operands = operands
        .iter()
        .enumerate()
        .map(|(position, operand)| {
            properties.insert(
                format!("operand.{position}.class-tag"),
                handle_tag_text(&operand.tag),
            );
            let kind = match operand.kind {
                Some(ArrayKind::Point) => "point",
                Some(ArrayKind::Line) => "line",
                Some(ArrayKind::Arc) => "arc",
                None => "unknown",
            };
            let resolved = operand.record.is_some() && operand.complete(in_sketch);
            SketchNativeOperand {
                native_kind: if resolved {
                    nonblank_literal!("sldprt:relation-operand:{kind}")
                } else {
                    nonblank_literal!("sldprt:relation-operand:{kind}:unresolved")
                },
                field: None,
                object_index: Some(u32::from(operand.slot)),
                native_ref: operand.marker.map(|marker| marker.id().to_owned()),
            }
        })
        .collect::<Vec<_>>();
    SketchConstraintDefinitionInput::Native {
        native_kind: nonblank_literal!("sldprt:sketch-relation:{}", row.code),
        native_state: None,
        native_flags: Some(u64::from(row.flag)),
        native_properties: properties,
        entities,
        parameter: None,
        operands: native_operands,
    }
}

#[cfg(test)]
mod tests;
