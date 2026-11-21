// SPDX-License-Identifier: Apache-2.0
//! The named-object header and the feature-object block every feature and
//! folder object writes (`docs/formats/sldprt.md` §2.3).

use super::archive::Archive;
use super::{CREATION_DATE, FILETIME, VERSION};

/// The schema byte of a named head: `0x40`, or `0xc0` for reference planes,
/// the origin and lights.
#[derive(Debug, Clone, Copy)]
pub(super) enum Schema {
    Plain,
    Geometric,
}

impl Schema {
    fn byte(self) -> u8 {
        match self {
            Self::Plain => 0x40,
            Self::Geometric => 0xc0,
        }
    }
}

/// A named object's head: its `moNodeName_c` object (`None` for an unnamed
/// object), u32 `0`, u16 family `0`, Boolean byte `0`, the schema byte, the
/// object identifier, u32 `0` and an empty `CString`.
pub(super) fn named(archive: &mut Archive, name: Option<&str>, schema: Schema, id: u32) {
    named_with(archive, name, schema, id, 0);
}

/// [`named`] with the u32 after the identifier.
pub(super) fn named_with(
    archive: &mut Archive,
    name: Option<&str>,
    schema: Schema,
    id: u32,
    after_id: u32,
) {
    match name {
        Some(name) => {
            archive.object(super::NODE_NAME_CLASS);
            archive.cstring(name);
        }
        None => archive.null(),
    }
    archive.u32(0);
    archive.u16(0);
    archive.u8(0);
    archive.u8(schema.byte());
    archive.u32(id);
    archive.u32(after_id);
    archive.cstring("");
}

/// The fields of a block's tail that differ between objects.
#[derive(Debug, Clone, Copy)]
pub(super) struct Tail {
    /// The u32 at block +20.
    pub(super) block_word: u32,
    /// The byte at tail +5, `1` or `3`.
    pub(super) kind: u8,
    /// The u32 at tail +30.
    pub(super) word30: u32,
    /// The u32 at tail +56.
    pub(super) word56: u32,
    /// The u32 at tail +60.
    pub(super) word60: u32,
    /// Whether the block states the version and date it was created in at
    /// +29 and +33.
    pub(super) created: bool,
    /// The u32 at tail +64, written in a part of `swVersion` 11000 or above.
    pub(super) word64: u32,
}

/// The tail of most folders and features.
pub(super) const TAIL: Tail = Tail {
    block_word: 101,
    kind: 1,
    word30: 0x0004_0000,
    word56: 0,
    word60: 1,
    created: false,
    word64: 1,
};

/// The tail of the folders the version adds to a part's tree: selection sets,
/// notes and markups.
pub(super) const ADDED_FOLDER_TAIL: Tail = Tail {
    block_word: 101,
    kind: 3,
    word30: 0,
    word56: 320_106,
    word60: 1,
    created: false,
    word64: 1,
};

/// The opening of a feature-object block: u32 `0`, u16 `0` and the count of
/// the inputs the caller writes next.
pub(super) fn block_open(archive: &mut Archive, inputs: u16) {
    archive.u32(0);
    archive.u16(0);
    archive.u16(inputs);
}

/// The rest of a feature-object block after its inputs: twelve zero bytes,
/// the fixed fields from +20, the +55 `CString` and the tail.
pub(super) fn block_close(archive: &mut Archive, tail: Tail) {
    archive.zeros(12);
    archive.u32(tail.block_word);
    archive.u32(0);
    archive.u8(0);
    if tail.created {
        archive.u32(VERSION);
        archive.u32(CREATION_DATE);
    } else {
        archive.u32(0);
        archive.u32(0);
    }
    archive.u32(VERSION);
    archive.f64(0.0);
    archive.u32(CREATION_DATE);
    archive.u16(0);
    archive.cstring("");
    archive.bytes(&[0x00, 0xff, 0xff, 0x00, 0x00]);
    archive.u8(tail.kind);
    archive.bytes(&[0xff; 8]);
    archive.f32(-1.0);
    archive.u32s(&[0, 0, 0, tail.word30, 0, 0]);
    archive.u32(u32::MAX);
    archive.u32(FILETIME.0);
    archive.u32(FILETIME.1);
    archive.u16(2);
    archive.u32(tail.word56);
    archive.u32(tail.word60);
    archive.u32(tail.word64);
}

/// A block with no inputs.
pub(super) fn block(archive: &mut Archive, tail: Tail) {
    block_open(archive, 0);
    block_close(archive, tail);
}

/// A feature body head with *h* `0` or `1`, the *n* references `inputs`
/// (when *h* is `1`), *f* `0`, the *m* identifiers `ids`, *t* `0`, *g* `0`,
/// *e* `0` and *x* `0` (§2.3).
pub(super) fn body_head(archive: &mut Archive, inputs: Option<&[u32]>, ids: &[u32]) {
    let head = inputs.is_some();
    archive.zeros(6);
    archive.u16(u16::from(head));
    archive.u16(0);
    let inputs = inputs.unwrap_or_default();
    let count = archive.count16(inputs.len(), "body head input count");
    archive.u16(count);
    for &input in inputs {
        archive.reference(input);
    }
    archive.u32(0);
    if head {
        archive.u16(0);
    }
    let count = archive.count32(ids.len(), "body head identifier count");
    archive.u32(count);
    archive.u32s(ids);
    archive.u32s(&[0, 0, 0]);
    archive.u16(0);
    archive.u16(1);
    archive.zeros(18);
    archive.u32(1);
}
