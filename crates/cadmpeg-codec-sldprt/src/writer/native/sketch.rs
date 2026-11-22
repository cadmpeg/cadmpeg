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

/// The handle class of a curve.
const fn handle_class(curve: &Curve) -> &'static str {
    match curve {
        Curve::Line { .. } => "sgLineHandle",
        Curve::Arc { .. } | Curve::Circle { .. } => "sgArcHandle",
    }
}

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
    /// The nested Parasolid stream of the profile body.
    pub(in crate::writer) body: Vec<u8>,
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
    /// The point positions its ends stand at, twice for a circle.
    fn ends(self) -> [usize; 2] {
        match self {
            Self::Line { start, end } | Self::Arc { start, end, .. } => [start, end],
            Self::Circle { on, .. } => [on, on],
        }
    }
}

/// Writes `sketch` as a top-level `moProfileFeature_c`.
pub(super) fn profile_feature(archive: &mut Archive, sketch: &Sketch) -> Result<(), CodecError> {
    archive.object("moProfileFeature_c");
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
    // Three empty dimension groups, eight empty relation groups and a u16 0.
    archive.zeros(2 * (3 + 8 + 1));
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
    Ok(())
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
/// the record identifier (its position plus one).
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
    for &(curve_index, curve) in &meeting {
        curve_handle(archive, curve_index, curve);
    }
    for _ in meeting.len()..3 {
        archive.null();
    }
    archive.u32(0xffff_fffe);
    archive.zeros(36);
    archive.u32(0);
    archive.u16(0);
    let id = archive.count32(index + 1, "point identifier");
    archive.u32(id);
}

/// The handle of the curve at `index`: its identifier, i32 `-1` and u32 `0`.
fn curve_handle(archive: &mut Archive, index: usize, curve: &Curve) {
    archive.object(handle_class(curve));
    let slot = archive.count16(index + 1, "curve identifier");
    archive.u16(slot);
    archive.u32(u32::MAX);
    archive.u32(0);
}

/// The curve lists: u32 `-1` and the lines, then, when arcs follow, the
/// 12-byte list header (`0`, the arc count, `1`) and the arcs and circles;
/// a sketch with arcs only opens with u32 `0x10000`. The list trailer is 36
/// zero bytes after a line and 22 after an arc.
fn curve_lists(archive: &mut Archive, sketch: &Sketch) {
    let lines = sketch
        .curves
        .iter()
        .take_while(|curve| matches!(curve, Curve::Line { .. }))
        .count();
    let arcs = sketch.curves.len() - lines;
    archive.u32(if lines == 0 && arcs != 0 {
        0x1_0000
    } else {
        u32::MAX
    });
    for (index, curve) in sketch.curves.iter().enumerate() {
        if index == lines {
            archive.u32(0);
            let count = archive.count32(arcs, "arc count");
            archive.u32(count);
            archive.u32(1);
        }
        curve_record(archive, index, *curve);
    }
    archive.zeros(if arcs == 0 { 36 } else { 22 });
}

/// A curve record: the head, the two endpoint positions, u32 `1`, f64 `-1`,
/// the curve type (`0` a line; `1` and u16 `0`, the sense `+1`, for an arc),
/// for an arc its centre's position and four i32 `-2`, then u16 `0`, u32 `0`
/// and the record identifier (its position plus one).
fn curve_record(archive: &mut Archive, index: usize, curve: Curve) {
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
    let id = archive.count32(index + 1, "curve identifier");
    archive.u32(id);
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
/// (three zero bytes, u32 `4`, i32 `-1`, u32 `0`, u32 `0`, u32 `1`, no hops).
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
    archive.u32(0);
    archive.u32(1);
    archive.u16(0);
    archive.u16(0);
    archive.zeros(4);
}
