// SPDX-License-Identifier: Apache-2.0
//! A blind extrusion as native objects: `moExtrusion_c` or `moCut_c`
//! (`docs/formats/sldprt.md` §2.3 "Extrusion feature objects" and "Dimension
//! display objects"). Byte runs `sldprt.md` gives as lengths only are zero.

use super::archive::Archive;
use super::block::{block_close, block_open, named, named_operation, Oid, Schema, Tail, TAIL};
use super::config::FIRST_BODY_ID;

/// A blind extrusion the tree writes.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::writer) struct Extrusion {
    /// The feature's name.
    pub(in crate::writer) name: String,
    /// The feature's object identifier: the history summary's source id.
    pub(in crate::writer) feature_id: u32,
    /// The position, among the part's user features, of the sketch it
    /// extrudes.
    pub(in crate::writer) profile: usize,
    /// Whether the extrusion removes material (`moCut_c`).
    pub(in crate::writer) cut: bool,
    /// The depth in metres.
    pub(in crate::writer) depth: f64,
    /// Whether it travels against the sketch plane's normal.
    pub(in crate::writer) reversed: bool,
}

/// The named-head family word of an extrusion that joins its result.
const JOIN_FAMILY: u16 = 0x0140;

/// The named-head family word of an extrusion that cuts its result.
const CUT_FAMILY: u16 = 0x01ca;

/// The block word of an extrusion.
const EXTRUSION_BLOCK_WORD: u32 = 104;

/// The tail of an extrusion's block.
const EXTRUSION_TAIL: Tail = Tail {
    block_word: EXTRUSION_BLOCK_WORD,
    kind: 3,
    word56: 320_108,
    created: true,
    word64: 0,
    ..TAIL
};

/// The `X` word of the extrusion's flag-0 mark record.
const MARK_X: u32 = 5;

/// The 76-byte block's first eight bytes.
const BLOCK76_HEAD: [u8; 8] = [0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00];

/// The zero run a favourite handle writes before an extrusion's 76-byte
/// block (`swVersion` 14000).
const FAVOURITE_RUN_BEFORE_BLOCK: usize = 50;

/// The parameter name of an extrusion's depth.
const DEPTH_NAME: &str = "D1";

/// Writes `extrusion` as a top-level object. `profile` is the map index of
/// the `moProfileFeature_c` it extrudes, `atom` that of the part's
/// `moAtom_c`; the depth parameter takes object identifier `parameter_id`.
pub(super) fn extrusion(
    archive: &mut Archive,
    extrusion: &Extrusion,
    profile: u32,
    atom: u32,
    parameter_id: u32,
) {
    archive.object(if extrusion.cut {
        "moCut_c"
    } else {
        "moExtrusion_c"
    });
    named_operation(
        archive,
        &extrusion.name,
        extrusion.feature_id,
        if extrusion.cut {
            CUT_FAMILY
        } else {
            JOIN_FAMILY
        },
    );
    block_open(archive, 1);
    archive.reference(profile);
    block_close(archive, EXTRUSION_TAIL);
    body_head(archive, atom);
    units(archive);
    mark(archive);
    end_spec(archive, extrusion.reversed);
    depth_display(archive, extrusion.depth, parameter_id);
    archive.bytes(&BLOCK76_HEAD);
    archive.zeros(56);
    archive.u32(1);
    archive.zeros(8);
    archive.object("moFromEndSpec_c");
    archive.zeros(20);
    archive.f64(0.0);
    archive.i32(-1);
    archive.null();
    archive.null();
}

/// The body head: *h* `1`, the atom as its one input, *f* `0`, the body
/// identifier, *t* `3`, *g* `0`, *e* `1`, *x* `1`.
fn body_head(archive: &mut Archive, atom: u32) {
    archive.zeros(6);
    archive.u16(1);
    archive.u16(0);
    archive.u16(1);
    archive.reference(atom);
    archive.u32(0);
    archive.u16(0);
    archive.u32(1);
    archive.u32(FIRST_BODY_ID);
    archive.u32(3);
    archive.u32(0);
    archive.u32(1);
    archive.u16(1);
}

/// One per-body unit: u16 `1`, a `moPerBodyChooserData_c` with no face
/// references, and its `moBBoxCenterData_c` (u32 `1`, four f64, u32 `0`,
/// u32 `0`, flag `1`, u32 `0`, u16 `0`, identifier `0`, continuation `1`,
/// u32 `1`, the identifier again and twenty bytes of u32 `0` and `1`).
fn units(archive: &mut Archive) {
    archive.u16(1);
    archive.object("moPerBodyChooserData_c");
    archive.u16(0);
    archive.bytes(&[0x00, 0x00, 0x01, 0x00]);
    archive.object("moBBoxCenterData_c");
    archive.u32(1);
    archive.f64s(&[0.0; 4]);
    archive.u32s(&[0, 0, 1, 0]);
    archive.u16(0);
    archive.u16(0);
    archive.u16(1);
    archive.u32(1);
    archive.u16(0);
    archive.u32s(&[0, 0, 1, 0, 1]);
}

/// The flag-0 `moPMarkRecord_c` with no lists, object or GUID.
fn mark(archive: &mut Archive) {
    archive.object("moPMarkRecord_c");
    archive.u32(MARK_X);
    archive.u32(0);
    archive.u32(0);
    archive.zeros(8);
    archive.u32s(&[0, 0, 0, 0]);
    archive.u32(0);
    archive.u32(0);
    archive.u32(0);
    archive.u32(0);
    archive.zeros(14);
}

/// `moEndSpec_c` of a blind first direction: u16 `0`, direction count `1`,
/// u32 `0`, the direction flag, u16 `0`, end type `0` and an inactive second
/// direction.
fn end_spec(archive: &mut Archive, reversed: bool) {
    archive.object("moEndSpec_c");
    archive.u16(0);
    archive.u32(1);
    archive.u32(0);
    archive.u32(u32::from(reversed));
    archive.u16(0);
    archive.u32(0);
    archive.u32(0);
}

/// The depth's display dimension: a `moDisplayDistanceDim_c` (151 bytes, one
/// listed identifier, 261 bytes, an empty `CString`, 132 bytes), its
/// `moFeatureDimHandle_c` (the 103-byte prefix) holding a
/// `ParallelPlaneDistanceDim_c` and its `moLengthParameter_c`, and the
/// `moFavoriteHandle_c` that ends before the 76-byte block.
fn depth_display(archive: &mut Archive, depth: f64, parameter_id: u32) {
    archive.object("moDisplayDistanceDim_c");
    archive.zeros(151);
    archive.u32(1);
    archive.u32(parameter_id);
    archive.zeros(261);
    archive.cstring("");
    archive.zeros(132);
    archive.object("moFeatureDimHandle_c");
    archive.zeros(42);
    archive.bytes(&[0xff; 16]);
    archive.zeros(20);
    archive.zeros(14);
    archive.bytes(&[0x00, 0xff, 0xff, 0xff, 0xff]);
    archive.zeros(6);
    archive.object("ParallelPlaneDistanceDim_c");
    archive.u32(1);
    archive.object("moLengthParameter_c");
    named(
        archive,
        Some(DEPTH_NAME),
        Schema::Plain,
        Oid::Plain(u32::MAX),
    );
    archive.u16(0);
    archive.f64(depth);
    archive.zeros(3);
    archive.u32(parameter_id);
    archive.zeros(16);
    // The placement block: its 84-byte prefix with no items, four empty
    // `CString` values, 262 bytes, a `CString`, 40 bytes and a `CString`.
    archive.zeros(84);
    for _ in 0..4 {
        archive.cstring("");
    }
    archive.zeros(262);
    archive.cstring("");
    archive.zeros(40);
    archive.cstring("");
    archive.object("moFavoriteHandle_c");
    archive.zeros(206);
    for _ in 0..4 {
        archive.cstring("");
    }
    archive.zeros(92);
    archive.cstring("");
    archive.cstring("");
    archive.zeros(12);
    archive.cstring("");
    archive.zeros(40);
    archive.cstring("");
    archive.zeros(FAVOURITE_RUN_BEFORE_BLOCK);
}
