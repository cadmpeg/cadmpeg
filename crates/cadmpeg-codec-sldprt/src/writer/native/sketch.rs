// SPDX-License-Identifier: Apache-2.0
//! A user sketch as native objects: `moProfileFeature_c` and its `sgSketch`
//! (`docs/formats/sldprt.md` §2.3 "Sketch objects"), lying on a default
//! reference plane.
//!
//! The sketch writes its point records, then its lines and its arcs and
//! circles as curve records, its dimensions and relations, no display or
//! region objects, the settings block, a `moSketchChain_c` per profile chain,
//! and the tail with the profile body and the plane. A sketch of one circle
//! about the origin also holds the origin point's projection, as native ones
//! do. Byte runs `sldprt.md` gives as lengths only are zero.

use std::io::Write as _;

use cadmpeg_core::CodecError;

use super::archive::Archive;
use super::block::{block, named, Oid, Schema, Tail, TAIL};
use super::tree::{unit_component, Plane};
use super::TIME_STAMP;
use crate::records::SketchRelationKind;

/// The block tail of a user feature: created in the part's version, kind `3`,
/// the block word of its class, +56 `320108` and +64 `0`.
const fn feature_tail(block_word: u32) -> Tail {
    Tail {
        block_word,
        kind: 3,
        word56: 320_108,
        created: true,
        word64: 0,
        ..TAIL
    }
}

/// The block word of a `moProfileFeature_c`.
const PROFILE_BLOCK_WORD: u32 = 103;

/// The attribute record a current sketch record opens with: `ff ff`, schema
/// `1f 00`, slot `3`, eight `ff` bytes and f32 `-1` (17 bytes).
const RECORD_ATTRIBUTE: [u8; 17] = [
    0xff, 0xff, 0x1f, 0x00, 0x03, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80,
    0xbf,
];

/// A geometry locus at record +23: `05 00 01 00`.
const GEOMETRY_LOCUS: [u16; 2] = [5, 1];

/// The locus at record +23 of the centre, the circle and the projected
/// origin in a native sketch of one circle about the origin: `04 00 02 00`.
const CENTRE_LOCUS: [u16; 2] = [4, 2];

/// The u32 at +11 of the `moUnitComponent_c` of a native sketch's
/// projection of the origin point (EV-16).
const PROJECTION_COMPONENT_WORD: u32 = 10_001;

/// The name a sketch's projection of the origin point ends with.
const ORIGIN_POINT_NAME: &str = "Point1@Origin";

/// The selector at record +37 of a record whose indices are positions.
const SELECTOR: u16 = 4;

/// The coordinate mask of a two-coordinate point record.
const TWO_COORDINATES: u16 = 0x1e;

/// The byte runs of native sketches and extrusions written as zero bytes,
/// whose fields `sldprt.md` does not lay out.
pub(in crate::writer) const FEATURE_ZERO_RUNS: &str = "each native sketch's 76-byte and \
     42-byte settings runs; each extrusion's display dimension (151, 261 and 132 bytes), its \
     dimension handle (42, 20 and 14 bytes), its depth parameter's placement block (84, 262 and \
     40 bytes) and its favourite handle (206, 92, 12, 40 and 50 bytes)";

/// The object identifier `next_id` holds, which it then advances past; an
/// identifier past the field's range is the stream's refusal.
pub(super) fn take_id(archive: &mut Archive, next_id: &mut u32) -> u32 {
    let id = *next_id;
    *next_id = match id.checked_add(1) {
        Some(next) => next,
        None => archive.count32(usize::MAX, "object identifier"),
    };
    id
}

/// The slot of a placeholder entity handle.
const PLACEHOLDER_SLOT: u16 = 0x777f;

/// The 34 bytes an `sgCircleDim` writes after its parameter: f64 `1` at +10.
const CIRCLE_DIMENSION_TAIL: [u8; 34] = {
    let mut tail = [0; 34];
    let one = 1.0_f64.to_le_bytes();
    let mut index = 0;
    while index < one.len() {
        tail[10 + index] = one[index];
        index += 1;
    }
    tail
};

/// The 34 bytes a distance dimension writes after its parameter: u16 `1`
/// three times and zero bytes.
const DISTANCE_DIMENSION_TAIL: [u8; 34] = {
    let mut tail = [0; 34];
    tail[0] = 1;
    tail[2] = 1;
    tail[4] = 1;
    tail
};

/// The class of a dimension the tree writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::writer) enum DimensionClass {
    /// `sgCircleDim`: a radius or diameter.
    Circle,
    /// `sgPntPntDist`.
    PointPoint,
    /// `sgPntLineDist`.
    PointLine,
    /// `sgLLDist`.
    LineLine,
    /// `sgPntPntHorDist`.
    Horizontal,
}

impl DimensionClass {
    fn name(self) -> &'static str {
        match self {
            Self::Circle => "sgCircleDim",
            Self::PointPoint => "sgPntPntDist",
            Self::PointLine => "sgPntLineDist",
            Self::LineLine => "sgLLDist",
            Self::Horizontal => "sgPntPntHorDist",
        }
    }

    /// The 34 bytes the dimension writes after its parameter.
    fn tail(self) -> [u8; 34] {
        match self {
            Self::Circle => CIRCLE_DIMENSION_TAIL,
            Self::PointPoint | Self::PointLine | Self::LineLine | Self::Horizontal => {
                DISTANCE_DIMENSION_TAIL
            }
        }
    }
}

/// The wrapper magic of a compressed body (`zlb_wrapper_header`).
const BODY_MAGIC: [u8; 16] = crate::layout::zlb_wrapper_header::MAGIC_VALUE;

/// The marker before a sketch's profile body: u32 `3`.
const PROFILE_MARKER: u32 = 3;

/// The u32 a sketch tail opens its group list with when it has no groups.
const NO_CONTOUR_GROUPS: u32 = 100_000;

/// The zero bytes between a sketch's empty region list and its chain list.
const CHAIN_LIST_ZEROS: usize = 30;

/// The six u32 every native `moSketchChain_c` of `swVersion` 14000 holds
/// after its chain index.
const CHAIN_WORDS: [u32; 6] = [6, u32::MAX, 0, 0, 5, 5];

/// A sketch the tree writes.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::writer) struct Sketch {
    /// The feature's name.
    pub(in crate::writer) name: String,
    /// The feature's object identifier: the history summary's source id.
    pub(in crate::writer) feature_id: u32,
    /// The default plane it lies on.
    pub(in crate::writer) plane: Plane,
    /// The point records, in metres in the plane's frame.
    pub(in crate::writer) points: Vec<Point>,
    /// The curve records, lines first.
    pub(in crate::writer) curves: Vec<Curve>,
    /// The relations, in the relation table's order.
    pub(in crate::writer) relations: Vec<Relation>,
    /// The dimensions.
    pub(in crate::writer) dimensions: Vec<Dimension>,
    /// The profile chains: each the positions in `curves` of one closed
    /// chain's curves.
    pub(in crate::writer) chains: Vec<Vec<usize>>,
    /// The nested Parasolid stream of the profile body.
    pub(in crate::writer) body: Vec<u8>,
}

/// An operand handle: a point, line or arc by its position among the
/// records of its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::writer) enum Operand {
    Point(usize),
    Line(usize),
    Arc(usize),
}

/// A geometric relation (`SketchRelationKind` code) and its operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::writer) struct Relation {
    pub(in crate::writer) code: u32,
    pub(in crate::writer) operands: Vec<Operand>,
}

/// A driving dimension: its class, its parameter's name and value (metres),
/// the relation code of its measure and its operands.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::writer) struct Dimension {
    pub(in crate::writer) class: DimensionClass,
    pub(in crate::writer) name: String,
    pub(in crate::writer) value: f64,
    pub(in crate::writer) code: u32,
    pub(in crate::writer) operands: Vec<Operand>,
}

/// A point record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::writer) struct Point {
    /// Its coordinates in metres in the plane's frame.
    pub(in crate::writer) at: [f64; 2],
    /// The kind at record +17: `0` a point, `1` a line or circle locus, `2`
    /// an arc locus.
    pub(in crate::writer) kind: u32,
}

/// A curve record; indices are positions in the point records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::writer) enum Curve {
    Line {
        start: usize,
        end: usize,
    },
    /// A counterclockwise arc from `start` to `end` about `centre`.
    Arc {
        start: usize,
        end: usize,
        centre: usize,
    },
    /// A full circle through `on` about `centre`.
    Circle {
        on: usize,
        centre: usize,
    },
}

impl Curve {
    /// The handle of the curve at `index` among `curves`.
    fn operand(curves: &[Self], index: usize) -> Operand {
        let rank = |line: bool| {
            curves[..index]
                .iter()
                .filter(|curve| matches!(curve, Self::Line { .. }) == line)
                .count()
        };
        match curves[index] {
            Self::Line { .. } => Operand::Line(rank(true)),
            Self::Arc { .. } | Self::Circle { .. } => Operand::Arc(rank(false)),
        }
    }

    /// The point positions its ends stand at, twice for a circle.
    fn ends(self) -> [usize; 2] {
        match self {
            Self::Line { start, end } | Self::Arc { start, end, .. } => [start, end],
            Self::Circle { on, .. } => [on, on],
        }
    }
}

/// Writes `sketch` as a top-level `moProfileFeature_c`.
/// Dimension parameters take object identifiers from `next_id` on, and the
/// plane reference's trailer holds `reference`. Returns the map index of the
/// `moProfileFeature_c`.
pub(super) fn profile_feature(
    archive: &mut Archive,
    sketch: &Sketch,
    next_id: &mut u32,
    reference: u32,
) -> Result<u32, CodecError> {
    let index = archive.object("moProfileFeature_c");
    named(
        archive,
        Some(&sketch.name),
        Schema::Plain,
        Oid::Plain(sketch.feature_id),
    );
    block(archive, feature_tail(PROFILE_BLOCK_WORD));
    archive.zeros(10);
    archive.object("sgSketch");
    let about_origin = one_circle_about_origin(sketch);
    let points = archive.count32(
        sketch.points.len() + usize::from(about_origin),
        "sketch point count",
    );
    archive.u32(points);
    archive.u16(0);
    archive.u16(0);
    archive.u16(0);
    let curve_head = if about_origin {
        one_circle_records(archive, sketch, reference);
        (1, CENTRE_LOCUS)
    } else {
        archive.u32(1);
        for (index, point) in sketch.points.iter().enumerate() {
            point_record(archive, sketch, index, point);
        }
        (0, GEOMETRY_LOCUS)
    };
    curve_lists(archive, sketch, curve_head);
    dimension_groups(archive, sketch, next_id);
    relation_groups(archive, sketch, about_origin);
    archive.object("suObList");
    archive.u16(0);
    // The in-context block with no group, which native sketches holding the
    // origin point's projection write too.
    archive.zeros(12);
    settings(archive);
    archive.object("suObList");
    archive.u16(0);
    archive.zeros(CHAIN_LIST_ZEROS);
    chain_list(archive, &sketch.chains);
    archive.u32(PROFILE_MARKER);
    profile_body(archive, &sketch.body)?;
    archive.u32(0);
    archive.u16(0);
    archive.u32(0);
    archive.u32(NO_CONTOUR_GROUPS);
    archive.u16(0);
    archive.null();
    plane(archive, sketch.plane, reference);
    Ok(index)
}

/// The 64-byte head of a sketch record of `kind` at `locus`. The object
/// pointer at +35 is null, or, when `projection` holds the sketch's
/// plane-reference identifier, the `sgExtEnt_c` of the origin point's
/// projection.
fn record_head(
    archive: &mut Archive,
    kind: u32,
    locus: [u16; 2],
    state: u16,
    projection: Option<u32>,
) {
    archive.bytes(&RECORD_ATTRIBUTE);
    archive.u32(kind);
    archive.u16(0);
    archive.u16(locus[0]);
    archive.u16(locus[1]);
    archive.u16(1);
    archive.u16(state);
    archive.f32(-1.0);
    match projection {
        Some(reference) => origin_projection(archive, reference),
        None => archive.null(),
    }
    archive.u16(SELECTOR);
    archive.zeros(9);
    archive.f64(1.0);
    archive.zeros(8);
}

/// A point record: the head, the mask and coordinates, the count of curve
/// ends at the point, the handles of the curves that meet there (at most
/// three, then null pointers), i32 `-2`, 36 zero bytes, u32 `0`, u16 `0` and
/// the identifier of the next point record (its position plus two; the
/// sketch's u32 before the first record is that record's identifier, `1`).
/// The last point record writes the line count in place of its u32 `0`, and
/// the first line's chain identifier, or `0` when the sketch holds no line,
/// in place of the identifier.
fn point_record(archive: &mut Archive, sketch: &Sketch, index: usize, point: &Point) {
    record_head(archive, point.kind, GEOMETRY_LOCUS, 0, None);
    archive.u16(TWO_COORDINATES);
    archive.f64(point.at[0]);
    archive.f64(point.at[1]);
    let ends = sketch
        .curves
        .iter()
        .flat_map(|curve| curve.ends())
        .filter(|&end| end == index)
        .count();
    archive.u16(0);
    let ends = archive.count16(ends, "point incidence count");
    archive.u16(ends);
    let meeting = sketch
        .curves
        .iter()
        .enumerate()
        .filter(|(_, curve)| curve.ends().contains(&index))
        .take(3)
        .collect::<Vec<_>>();
    for &(curve_index, _) in &meeting {
        handle(archive, Curve::operand(&sketch.curves, curve_index));
    }
    for _ in meeting.len()..3 {
        archive.null();
    }
    archive.u32(0xffff_fffe);
    archive.zeros(36);
    let last = index + 1 == sketch.points.len();
    let lines = lines(sketch);
    let next_array = archive.count32(if last { lines } else { 0 }, "line count");
    archive.u32(next_array);
    archive.u16(0);
    let id = match (last, lines) {
        (false, _) => index + 2,
        (true, 0) => 0,
        (true, _) => chain_identifier(0),
    };
    let id = archive.count32(id, "point identifier");
    archive.u32(id);
}

/// Whether `sketch` is one circle about the origin: its records are the
/// centre, at the origin, and the circle's point. The 5 native sketches of
/// one circle hold the centre there and the origin point's projection, and
/// the writer writes them in that form ([`one_circle_records`]).
fn one_circle_about_origin(sketch: &Sketch) -> bool {
    matches!(
        sketch.curves.as_slice(),
        [Curve::Circle { on: 1, centre: 0 }]
    ) && sketch.points.len() == 2
        && sketch.points[0]
            .at
            .iter()
            .all(|coordinate| coordinate.abs() <= super::from_ir::POINT_MERGE_TOLERANCE_M)
}

/// A point record of a sketch of one circle about the origin.
struct CircleRecord {
    kind: u32,
    locus: [u16; 2],
    /// The plane-reference identifier, for the origin point's projection.
    projection: Option<u32>,
    at: [f64; 2],
    /// The u16 after the coordinates.
    word: u16,
    /// The count of curve uses at the point.
    uses: u16,
    /// The handle of the curve whose ends meet there.
    curve: Option<Operand>,
    /// The count of the record array that follows, or the line count after
    /// the last record.
    next_array: u32,
    /// The identifier of the next record, `0` after the last.
    next_id: u32,
}

/// The identifier before the records and the three point records of a
/// sketch of one circle about the origin, as the 5 native ones hold them
/// (sldprt.md §2.3): identifier `2`; the centre (kind `1`, locus `04 00 02
/// 00`, one curve use, no handle, next identifier `1`); the circle's point
/// (kind `0`, locus `05 00 01 00`, u16 `2`, two curve uses and the circle's
/// handle, u32 `1` and next identifier `3`); the origin point's projection
/// (kind `1`, locus `04 00 02 00`, the `sgExtEnt_c`, u16 `6`, no curve, the
/// line count `0` and identifier `0`).
fn one_circle_records(archive: &mut Archive, sketch: &Sketch, reference: u32) {
    archive.u32(2);
    let records = [
        CircleRecord {
            kind: 1,
            locus: CENTRE_LOCUS,
            projection: None,
            at: sketch.points[0].at,
            word: 0,
            uses: 1,
            curve: None,
            next_array: 0,
            next_id: 1,
        },
        CircleRecord {
            kind: 0,
            locus: GEOMETRY_LOCUS,
            projection: None,
            at: sketch.points[1].at,
            word: 2,
            uses: 2,
            curve: Some(Operand::Arc(0)),
            next_array: 1,
            next_id: 3,
        },
        CircleRecord {
            kind: 1,
            locus: CENTRE_LOCUS,
            projection: Some(reference),
            at: [0.0, 0.0],
            word: 6,
            uses: 0,
            curve: None,
            next_array: 0,
            next_id: 0,
        },
    ];
    for record in records {
        record_head(archive, record.kind, record.locus, 0, record.projection);
        archive.u16(TWO_COORDINATES);
        archive.f64(record.at[0]);
        archive.f64(record.at[1]);
        archive.u16(record.word);
        archive.u16(record.uses);
        match record.curve {
            Some(curve) => handle(archive, curve),
            None => archive.null(),
        }
        archive.null();
        archive.null();
        archive.u32(0xffff_fffe);
        archive.zeros(36);
        archive.u32(record.next_array);
        archive.u16(0);
        archive.u32(record.next_id);
    }
}

/// The origin point's projection into a sketch: an `sgExtEnt_c` naming a
/// `moSketchExtRef_w` naming a `moCompSketchEntHandle_c`, whose component
/// is a `moUnitComponent_c` with [`PROJECTION_COMPONENT_WORD`] at +11, whose
/// feature is the origin sketch with its time stamp and whose handle is the
/// origin sketch's point; then u32 `1`, u16 `0`, a null component, the
/// 54-byte block (u32 `0`, the sketch's plane-reference identifier,
/// twelve zero bytes, three i32 `-12345`, u32 `0`, the version and fourteen
/// zero bytes), a `moPointBackedUpData_c` at the origin, six zero bytes and
/// the name, as the native one-circle sketch whose block holds version
/// `14000` holds it (sldprt.md §2.3).
fn origin_projection(archive: &mut Archive, reference: u32) {
    archive.object("sgExtEnt_c");
    archive.object("moSketchExtRef_w");
    archive.object("moCompSketchEntHandle_c");
    archive.object("moUnitComponent_c");
    archive.u16(2);
    archive.u16(0);
    archive.zeros(11);
    archive.u32(PROJECTION_COMPONENT_WORD);
    archive.zeros(24);
    archive.bytes(&[0xff; 16]);
    archive.zeros(20);
    let origin = archive.tree_id(super::ids::ORIGIN);
    archive.u32(origin);
    archive.u32(TIME_STAMP);
    handle(archive, Operand::Point(0));
    archive.u32(1);
    archive.u16(0);
    archive.null();
    archive.u32(0);
    archive.u32(reference);
    archive.zeros(12);
    for _ in 0..3 {
        archive.i32(-12_345);
    }
    archive.u32(0);
    archive.u32(super::VERSION);
    archive.zeros(14);
    archive.object("moPointBackedUpData_c");
    archive.f64s(&[0.0; 3]);
    archive.zeros(6);
    archive.cstring(ORIGIN_POINT_NAME);
}

/// The identifier a chain names the curve at `position` among the sketch's
/// curve records by: its position plus one.
const fn chain_identifier(position: usize) -> usize {
    position + 1
}

/// An operand handle: its position among the records of its kind, i32 `-1`
/// and u32 `0`.
fn handle(archive: &mut Archive, operand: Operand) {
    let (class, position) = match operand {
        Operand::Point(position) => ("sgPointHandle", position),
        Operand::Line(position) => ("sgLineHandle", position),
        Operand::Arc(position) => ("sgArcHandle", position),
    };
    archive.object(class);
    let slot = archive.count16(position, "handle slot");
    archive.u16(slot);
    archive.u32(u32::MAX);
    archive.u32(0);
}

/// The dimension groups: every dimension in the first, then two empty groups.
fn dimension_groups(archive: &mut Archive, sketch: &Sketch, next_id: &mut u32) {
    let count = archive.count16(sketch.dimensions.len(), "dimension count");
    archive.u16(count);
    for dimension in &sketch.dimensions {
        archive.object(dimension.class.name());
        archive.u32(0);
        archive.object("moLengthParameter_c");
        named(
            archive,
            Some(&dimension.name),
            Schema::Plain,
            Oid::Plain(u32::MAX),
        );
        archive.u16(0);
        archive.f64(dimension.value);
        archive.u8(0);
        archive.null();
        let id = take_id(archive, next_id);
        archive.u32(id);
        archive.zeros(16);
        archive.u32(dimension.code);
        let placeholder = dimension.operands.len() == 1;
        let operands = archive.count16(
            dimension.operands.len() + usize::from(placeholder),
            "dimension operand count",
        );
        archive.u16(operands);
        archive.u32(1);
        archive.u16(0);
        for &operand in &dimension.operands {
            handle(archive, operand);
        }
        if placeholder {
            archive.object("sgEntHandle");
            archive.u16(PLACEHOLDER_SLOT);
            archive.u32(0xffff_fffe);
            archive.u32(u32::MAX);
            archive.u16(0);
        }
        archive.bytes(&dimension.class.tail());
    }
    archive.u16(0);
    archive.u16(0);
}

/// The eight relation groups and a u16 `0`: relations on one operand in the
/// first group, tangent relations in the third and the others in the second,
/// where native sketches hold them (EV-16); a sketch of one circle about the
/// origin opens the third with the centre's coincidence with the origin
/// point's projection. A relation writes eight zero
/// bytes, i32 `-1`, sixteen zero bytes, its code, u32 `2`, u16 `0`, u32
/// `0xfffe` and its operand handles; one with two operands then writes six
/// zero bytes, u16 `0`, eight `ff` bytes and 24 zero bytes.
fn relation_groups(archive: &mut Archive, sketch: &Sketch, about_origin: bool) {
    let projection = about_origin.then(|| Relation {
        code: SketchRelationKind::Coincident.native_code(),
        operands: vec![Operand::Point(2), Operand::Point(0)],
    });
    let group_of = |relation: &Relation| {
        if relation.operands.len() == 1 {
            0
        } else if relation.code == SketchRelationKind::Tangent.native_code() {
            2
        } else {
            1
        }
    };
    for written in 0..RELATION_GROUPS_WRITTEN {
        let group = projection
            .iter()
            .filter(|_| written == 2)
            .chain(
                sketch
                    .relations
                    .iter()
                    .filter(|relation| group_of(relation) == written),
            )
            .collect::<Vec<_>>();
        let count = archive.count16(group.len(), "relation count");
        archive.u16(count);
        for relation in group {
            archive.zeros(8);
            archive.i32(-1);
            archive.zeros(16);
            archive.u32(relation.code);
            archive.u32(2);
            archive.u16(0);
            archive.u32(0xfffe);
            for &operand in &relation.operands {
                handle(archive, operand);
            }
            if relation.operands.len() == 2 {
                archive.zeros(6);
                archive.u16(0);
                archive.bytes(&[0xff; 8]);
                archive.zeros(24);
            }
        }
    }
    archive.zeros(2 * (RELATION_GROUPS - RELATION_GROUPS_WRITTEN + 1));
}

/// The relation groups of an `sgSketch`.
const RELATION_GROUPS: usize = 8;

/// The relation groups that hold the relations the tree writes.
const RELATION_GROUPS_WRITTEN: usize = 3;

/// The curve lists: the lead word and the lines, then, when arcs follow, the
/// 12-byte list header and the arcs and circles. The ten bytes before each
/// curve record end with its chain identifier and its record identifier, and
/// the record identifiers of each kind count from `1`: the lead word is the
/// first line's record identifier, `1`; a sketch with arcs only opens with
/// the arc count in the high half of its lead word; the list header is u32
/// `0`, the first arc's chain identifier and its record identifier `1`. An
/// array's count stands 14 bytes before its first record: the arc count is
/// the high half of the last line's last word. The list trailer is 36 zero
/// bytes after a line and 22 after an arc.
fn curve_lists(archive: &mut Archive, sketch: &Sketch, head: (u32, [u16; 2])) {
    let lines = lines(sketch);
    let arcs = sketch.curves.len() - lines;
    let arc_count = archive.count16(arcs, "arc count");
    let arc_count_high = u32::from(arc_count) << 16;
    archive.u32(match (lines, arcs) {
        (0, 0) => u32::MAX,
        (0, _) => arc_count_high,
        _ => 1,
    });
    for (index, curve) in sketch.curves.iter().enumerate() {
        if index == lines {
            archive.u32(0);
            let chain = archive.count32(chain_identifier(index), "chain identifier");
            archive.u32(chain);
            archive.u32(1);
        }
        let (kind_index, kind_count) = if index < lines {
            (index, lines)
        } else {
            (index - lines, arcs)
        };
        let next = if kind_index + 1 < kind_count {
            let chain = archive.count32(chain_identifier(index + 1), "chain identifier");
            let record = archive.count32(kind_index + 2, "curve identifier");
            (chain, record)
        } else if index < lines {
            (0, arc_count_high)
        } else {
            (0, 0)
        };
        curve_record(archive, *curve, next, head);
    }
    archive.zeros(if arcs == 0 { 36 } else { 22 });
}

/// A curve record: the head, the two endpoint positions, u32 `1`, f64 `-1`,
/// the curve type (`0` a line; `1` and u16 `0`, the sense `+1`, for an arc),
/// for an arc its centre's position and four i32 `-2`, then u16 `0` and
/// `next`: the chain and record identifiers of the next record of its kind,
/// or `0` and the next array's count in the high half of a u32 after the
/// last.
fn curve_record(archive: &mut Archive, curve: Curve, next: (u32, u32), head: (u32, [u16; 2])) {
    record_head(archive, head.0, head.1, 1, None);
    let [start, end] = curve.ends();
    let start = archive.count16(start, "curve endpoint");
    let end = archive.count16(end, "curve endpoint");
    archive.u16(start);
    archive.u16(end);
    archive.u32(1);
    archive.f64(-1.0);
    match curve {
        Curve::Line { .. } => archive.u16(0),
        Curve::Arc { centre, .. } | Curve::Circle { centre, .. } => {
            archive.u16(1);
            archive.u16(0);
            let centre = archive.count16(centre, "arc centre");
            archive.u16(centre);
            archive.u32s(&[0xffff_fffe; 4]);
        }
    }
    archive.u16(0);
    archive.u32(next.0);
    archive.u32(next.1);
}

/// The count of the sketch's lines, which stand before its arcs.
fn lines(sketch: &Sketch) -> usize {
    sketch
        .curves
        .iter()
        .take_while(|curve| matches!(curve, Curve::Line { .. }))
        .count()
}

/// The chain list: the chain count twice, then per chain a
/// `moSketchChain_c`: a u16 count and the chain identifiers of its curves,
/// u16 `1` and the chain's position, [`CHAIN_WORDS`], an empty `CString`, u16
/// `0` (no handles), i32 `-1` and twenty zero bytes.
fn chain_list(archive: &mut Archive, chains: &[Vec<usize>]) {
    let count = archive.count32(chains.len(), "chain count");
    archive.u32(count);
    archive.u32(count);
    for (position, chain) in chains.iter().enumerate() {
        archive.object("moSketchChain_c");
        let curves = archive.count16(chain.len(), "chain curve count");
        archive.u16(curves);
        for &curve in chain {
            let id = archive.count32(chain_identifier(curve), "chain identifier");
            archive.u32(id);
        }
        archive.u16(1);
        let position = archive.count32(position, "chain position");
        archive.u32(position);
        archive.u32s(&CHAIN_WORDS);
        archive.cstring("");
        archive.u16(0);
        archive.i32(-1);
        archive.zeros(20);
    }
}

/// The settings block: 76 bytes, an empty `CString`, 42 bytes, u32 `4`,
/// u32 `1`, no extension-line indicators and 24 zero bytes (EV-11, EV-16).
fn settings(archive: &mut Archive) {
    archive.zeros(76);
    archive.cstring("");
    archive.zeros(42);
    archive.u32(4);
    archive.u32(1);
    archive.u32(0);
    archive.zeros(24);
}

/// The compressed profile body: u8 `0`, u32 *A*, u8 `0`, u32 *B* = *A* − 5,
/// then *B* bytes: the wrapper magic, the inflated and compressed sizes, the
/// zlib member and eight zero bytes.
fn profile_body(archive: &mut Archive, stream: &[u8]) -> Result<(), CodecError> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(stream)?;
    let member = encoder.finish()?;
    let inflated = archive.count32(stream.len(), "profile body length");
    let compressed = archive.count32(member.len(), "profile body length");
    let wrapped_length = BODY_MAGIC.len() + 8 + member.len() + 8;
    let wrapped = archive.count32(wrapped_length, "profile body length");
    let outer = archive.count32(wrapped_length + 5, "profile body length");
    archive.u8(0);
    archive.u32(outer);
    archive.u8(0);
    archive.u32(wrapped);
    archive.bytes(&BODY_MAGIC);
    archive.u32(inflated);
    archive.u32(compressed);
    archive.bytes(&member);
    archive.zeros(8);
    Ok(())
}

/// The sketch's plane: a `moCompRefPlane_c` naming the default plane by its
/// feature number and its u16 word, with its frame (flag `0` and three zero
/// f64 for Front; flag `1`, the rotation and a zero translation for Top and
/// Right), f64 `1` and the trailer of a user sketch (three zero bytes, u32
/// `4`, i32 `-1`, u32 `0`, the identifier `reference`, u32 `1`, no hops).
fn plane(archive: &mut Archive, plane: Plane, reference: u32) {
    archive.object("moCompRefPlane_c");
    unit_component(archive, 0);
    let feature = archive.tree_id(plane.tree_id());
    archive.u32(feature);
    archive.u32(TIME_STAMP);
    archive.u16(0);
    archive.u16(plane.reference_word());
    archive.u16(0);
    match plane.reference_rotation() {
        Some(rotation) => {
            archive.u8(1);
            archive.f64s(&rotation);
        }
        None => archive.u8(0),
    }
    archive.f64s(&[0.0, 0.0, 0.0, 1.0]);
    archive.zeros(3);
    archive.u32(4);
    archive.i32(-1);
    archive.u32(0);
    archive.u32(reference);
    archive.u32(1);
    archive.u16(0);
    archive.u16(0);
    archive.zeros(4);
}
