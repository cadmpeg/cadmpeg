// SPDX-License-Identifier: Apache-2.0
//! A writer of the MFC `CArchive` object graph of `Contents/Config-N` and
//! `Contents/Config-N-ResolvedFeatures` (`docs/formats/sldprt.md` §2.2).
//!
//! Classes and objects share one map counter. A class's first object writes
//! its declaration, which takes the next index for the class and the one after
//! it for the object; a later object writes the class tag and takes one index.
//! The `ResolvedFeatures` stream continues the map its `Config-N` stream ended
//! with ([`Archive::continuing`]).

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;

/// The map index `moPart_c` takes: `Config-N` starts its map after the
/// entries the stream does not write.
pub(super) const FIRST_INDEX: u32 = 2;

/// The class tag `0x8000 | index` reaches index `0x7ffe`; a larger index
/// writes `0x7fff` and a u32.
const LONG_TAG: u16 = 0x7fff;

/// The marker that opens a Unicode `CString`.
const STRING_MARKER: [u8; 3] = [0xff, 0xfe, 0xff];

/// A `CString` length byte of `ff` is followed by a u16 length.
const LONG_STRING: u8 = 0xff;

/// The class map one stream ended with.
#[derive(Debug, Clone)]
pub(super) struct ClassMap {
    classes: BTreeMap<&'static str, u32>,
    next: u32,
}

impl ClassMap {
    /// The map count after the stream's last object.
    pub(super) fn count(&self) -> u32 {
        self.next
    }
}

/// One stream being written.
pub(super) struct Archive {
    bytes: Vec<u8>,
    map: ClassMap,
    /// The first value that did not fit its field.
    overflow: Option<&'static str>,
    /// What the default tree's object identifiers add to their `ids` values.
    id_base: u32,
}

impl Archive {
    /// A `Config-N` stream, its map starting at [`FIRST_INDEX`], whose tree
    /// identifiers add `id_base`.
    pub(super) fn new(id_base: u32) -> Self {
        Self {
            bytes: Vec::new(),
            map: ClassMap {
                classes: BTreeMap::new(),
                next: FIRST_INDEX,
            },
            overflow: None,
            id_base,
        }
    }

    /// A stream that continues `map`, whose tree identifiers add `id_base`.
    pub(super) fn continuing(map: ClassMap, id_base: u32) -> Self {
        Self {
            bytes: Vec::new(),
            map,
            overflow: None,
            id_base,
        }
    }

    /// The object identifier of the default-tree node `id` (an `ids` value).
    pub(super) fn tree_id(&mut self, id: u32) -> u32 {
        match self.id_base.checked_add(id) {
            Some(id) => id,
            None => {
                self.overflow.get_or_insert("tree object identifier");
                0
            }
        }
    }

    /// The written bytes and the map they end with, or the refusal of a
    /// value that did not fit its field.
    pub(super) fn finish(self) -> Result<(Vec<u8>, ClassMap), CodecError> {
        match self.overflow {
            Some(field) => Err(CodecError::malformed(format_args!(
                "SLDPRT native skeleton: the {field} does not fit its field"
            ))),
            None => Ok((self.bytes, self.map)),
        }
    }

    /// `value` as a u16 count; a value that does not fit is recorded as the
    /// stream's refusal.
    pub(super) fn count16(&mut self, value: usize, field: &'static str) -> u16 {
        match u16::try_from(value) {
            Ok(count) => count,
            Err(_) => {
                self.overflow.get_or_insert(field);
                0
            }
        }
    }

    /// `value` as a u32 count; a value that does not fit is recorded as the
    /// stream's refusal.
    pub(super) fn count32(&mut self, value: usize, field: &'static str) -> u32 {
        match u32::try_from(value) {
            Ok(count) => count,
            Err(_) => {
                self.overflow.get_or_insert(field);
                0
            }
        }
    }

    /// The bytes written so far.
    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Opens an object of `class` (schema 1) and returns its map index.
    pub(super) fn object(&mut self, class: &'static str) -> u32 {
        self.object_with_schema(class, 1)
    }

    /// Opens an object of `class`, declaring the class with `schema` on its
    /// first use, and returns the object's map index.
    pub(super) fn object_with_schema(&mut self, class: &'static str, schema: u16) -> u32 {
        if let Some(&index) = self.map.classes.get(class) {
            self.tag(index);
        } else {
            let index = self.map.next;
            self.map.classes.insert(class, index);
            self.map.next += 1;
            self.u16(u16::MAX);
            self.u16(schema);
            let name = self.count16(class.len(), "class name");
            self.u16(name);
            self.bytes.extend_from_slice(class.as_bytes());
        }
        let object = self.map.next;
        self.map.next += 1;
        object
    }

    /// The class tag of the class at `index`.
    fn tag(&mut self, index: u32) {
        match u16::try_from(index) {
            Ok(short) if short < LONG_TAG => self.u16(0x8000 | short),
            _ => {
                self.u16(LONG_TAG);
                self.u32(0x8000_0000 | index);
            }
        }
    }

    /// A reference to the object already written at `index`.
    pub(super) fn reference(&mut self, index: u32) {
        match u16::try_from(index) {
            Ok(short) if short < LONG_TAG => self.u16(short),
            _ => {
                self.u16(LONG_TAG);
                self.u32(index);
            }
        }
    }

    /// The null object pointer.
    pub(super) fn null(&mut self) {
        self.u16(0);
    }

    pub(super) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(super) fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn f64s(&mut self, values: &[f64]) {
        for &value in values {
            self.f64(value);
        }
    }

    pub(super) fn u32s(&mut self, values: &[u32]) {
        for &value in values {
            self.u32(value);
        }
    }

    /// `count` zero bytes.
    pub(super) fn zeros(&mut self, count: usize) {
        self.bytes.resize(self.bytes.len() + count, 0);
    }

    pub(super) fn bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// A Unicode `CString`. `text` holds at most `u16::MAX` UTF-16 code
    /// units; the callers' texts are names this writer fits.
    pub(super) fn cstring(&mut self, text: &str) {
        let units = text.encode_utf16().collect::<Vec<_>>();
        self.bytes.extend_from_slice(&STRING_MARKER);
        match u8::try_from(units.len()) {
            Ok(short) if short < LONG_STRING => self.u8(short),
            _ => {
                self.u8(LONG_STRING);
                let length = self.count16(units.len(), "CString length");
                self.u16(length);
            }
        }
        for unit in units.iter().take(usize::from(u16::MAX)) {
            self.u16(*unit);
        }
    }
}
