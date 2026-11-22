// SPDX-License-Identifier: Apache-2.0
//! A user sketch as native objects: `moProfileFeature_c` and its `sgSketch`
//! (`docs/formats/sldprt.md` §2.3 "Sketch objects"), lying on a default
//! reference plane.
//!
//! The sketch writes its point records, then its lines and its arcs and
//! circles as curve records, no dimension, relation, display or region
//! objects, the settings block, and the tail with the profile body and the
//! plane. Byte runs `sldprt.md` gives as lengths only are zero.

use std::io::Write as _;

use cadmpeg_core::CodecError;

use super::archive::Archive;
use super::block::{block, named, Oid, Schema, Tail, TAIL};
use super::tree::{unit_component, Plane};
use super::TIME_STAMP;

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

/// The bytes a dimension writes after its parameter: ten zero bytes, f64 `1`
/// and sixteen zero bytes.
const DIMENSION_TAIL_ZEROS: (usize, usize) = (10, 16);

/// The wrapper magic of a compressed body (`zlb_wrapper_header`).
const BODY_MAGIC: [u8; 16] = crate::layout::zlb_wrapper_header::MAGIC_VALUE;

/// The marker before a sketch's profile body: u32 `3`.
const PROFILE_MARKER: u32 = 3;

/// The u32 a sketch tail opens its group list with when it has no groups.
const NO_CONTOUR_GROUPS: u32 = 100_000;

/// The u16 a sketch's `moCompRefPlane_c` writes after the plane's feature
/// number (as the origin sketch's does).
const PLANE_WORD: u16 = 3;

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

/// A driving dimension: its class, its parameter's name and value (metres or
/// radians), the relation code of its measure and its operands.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::writer) struct Dimension {
    pub(in crate::writer) class: &'static str,
    pub(in crate::writer) name: String,
    pub(in crate::writer) value: f64,
    pub(in crate::writer) angle: bool,
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
/// Dimension parameters take object identifiers from `next_id` on. Returns
/// the map index of the `moProfileFeature_c`.
pub(super) fn profile_feature(
    archive: &mut Archive,
    sketch: &Sketch,
    next_id: &mut u32,
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
    let points = archive.count32(sketch.points.len(), "sketch point count");
    archive.u32(points);
    archive.u16(0);
    archive.u16(0);
    archive.u16(0);
    archive.u32(1);
    for (index, point) in sketch.points.iter().enumerate() {
        point_record(archive, sketch, index, point);
    }
    curve_lists(archive, sketch);
    dimension_groups(archive, sketch, next_id);
    relation_groups(archive, sketch);
    archive.object("suObList");
    archive.u16(0);
    // The in-context block of a sketch with no external references.
    archive.zeros(12);
    settings(archive);
    archive.object("suObList");
    archive.u16(0);
    archive.zeros(38);
    archive.u32(PROFILE_MARKER);
    profile_body(archive, &sketch.body)?;
    archive.u32(0);
    archive.u16(0);
    archive.u32(0);
    archive.u32(NO_CONTOUR_GROUPS);
    archive.u16(0);
    archive.null();
    plane(archive, sketch.plane);
    Ok(index)
}

/// The 64-byte head of a sketch record of `kind`.
fn record_head(archive: &mut Archive, kind: u32, state: u16) {
    archive.bytes(&RECORD_ATTRIBUTE);
    archive.u32(kind);
    archive.u16(0);
    archive.u16(GEOMETRY_LOCUS[0]);
    archive.u16(GEOMETRY_LOCUS[1]);
    archive.u16(1);
    archive.u16(state);
    archive.f32(-1.0);
    archive.null();
    archive.u16(SELECTOR);
    archive.zeros(9);
    archive.f64(1.0);
    archive.zeros(8);
}

/// A point record: the head, the mask and coordinates, the count of curve
/// ends at the point, the handles of the curves that meet there (at most
/// three, then null pointers), i32 `-2`, 36 zero bytes, u32 `0`, u16 `0` and
/// the identifier of the next record (its position plus two; the sketch's
/// u32 before the first record is that record's identifier, `1`).
fn point_record(archive: &mut Archive, sketch: &Sketch, index: usize, point: &Point) {
    record_head(archive, point.kind, 0);
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
    // The last point record's u32 here is the count of the line array that
    // follows, which stands 14 bytes before the first line record.
    let next_array = if index + 1 == sketch.points.len() {
        lines(sketch)
    } else {
        0
    };
    let next_array = archive.count32(next_array, "line count");
    archive.u32(next_array);
    archive.u16(0);
    let id = archive.count32(index + 2, "point identifier");
    archive.u32(id);
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
        archive.object(dimension.class);
        archive.u32(0);
        archive.object(if dimension.angle {
            "moAngleParameter_c"
        } else {
            "moLengthParameter_c"
        });
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
        archive.zeros(DIMENSION_TAIL_ZEROS.0);
        archive.f64(1.0);
        archive.zeros(DIMENSION_TAIL_ZEROS.1);
    }
    archive.u16(0);
    archive.u16(0);
}

/// The eight relation groups and a u16 `0`: relations on one operand in the
/// first group, the others in the second. A relation writes eight zero bytes,
/// i32 `-1`, sixteen zero bytes, its code, u32 `2`, u16 `0`, u32 `0xfffe` and
/// its operand handles; one with two operands then writes six zero bytes,
/// u16 `0`, eight `ff` bytes and 24 zero bytes.
fn relation_groups(archive: &mut Archive, sketch: &Sketch) {
    for unary in [true, false] {
        let group = sketch
            .relations
            .iter()
            .filter(|relation| (relation.operands.len() == 1) == unary)
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
    archive.zeros(2 * (6 + 1));
}

/// The curve lists: u32 `-1` and the lines, then, when arcs follow, the
/// 12-byte list header (`0`, `1`, `1`) and the arcs and circles; a sketch
/// with arcs only opens with the arc count in the high half of its lead
/// word. The list trailer is 36 zero bytes after a line and 22 after an arc.
fn curve_lists(archive: &mut Archive, sketch: &Sketch) {
    let lines = lines(sketch);
    let arcs = sketch.curves.len() - lines;
    let arc_count = archive.count16(arcs, "arc count");
    // An array's count stands 14 bytes before its first record: the arc
    // count is the high half of the u32 that ends two bytes before the list
    // header, the lead word without lines and the last line's last word
    // with them.
    let arc_count_high = u32::from(arc_count) << 16;
    archive.u32(if lines == 0 && arcs != 0 {
        arc_count_high
    } else {
        u32::MAX
    });
    for (index, curve) in sketch.curves.iter().enumerate() {
        if index == lines {
            archive.u32(0);
            archive.u32(1);
            archive.u32(1);
        }
        let last_line = index + 1 == lines && arcs != 0;
        curve_record(archive, index, *curve, last_line.then_some(arc_count_high));
    }
    archive.zeros(if arcs == 0 { 36 } else { 22 });
}

/// A curve record: the head, the two endpoint positions, u32 `1`, f64 `-1`,
/// the curve type (`0` a line; `1` and u16 `0`, the sense `+1`, for an arc),
/// for an arc its centre's position and four i32 `-2`, then u16 `0`, u32 `0`
/// and the identifier of the next record (its position plus two), or
/// `last_word` in its place.
fn curve_record(archive: &mut Archive, index: usize, curve: Curve, last_word: Option<u32>) {
    record_head(archive, 0, 1);
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
    archive.u32(0);
    let id = archive.count32(index + 2, "curve identifier");
    archive.u32(last_word.unwrap_or(id));
}

/// The count of the sketch's lines, which stand before its arcs.
fn lines(sketch: &Sketch) -> usize {
    sketch
        .curves
        .iter()
        .take_while(|curve| matches!(curve, Curve::Line { .. }))
        .count()
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
/// feature number, with the identity frame and the trailer of a user sketch
/// (three zero bytes, u32 `4`, i32 `-1`, u32 `0`, the first body identifier,
/// u32 `1`, no hops).
fn plane(archive: &mut Archive, plane: Plane) {
    archive.object("moCompRefPlane_c");
    unit_component(archive, 0);
    let feature = archive.tree_id(plane.tree_id());
    archive.u32(feature);
    archive.u32(TIME_STAMP);
    archive.u16(0);
    archive.u16(PLANE_WORD);
    archive.u16(0);
    archive.u8(0);
    archive.f64s(&[0.0, 0.0, 0.0, 1.0]);
    archive.zeros(3);
    archive.u32(4);
    archive.i32(-1);
    archive.u32(0);
    archive.u32(super::config::FIRST_BODY_ID);
    archive.u32(1);
    archive.u16(0);
    archive.u16(0);
    archive.zeros(4);
}
