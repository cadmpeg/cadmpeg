// SPDX-License-Identifier: Apache-2.0
//! The deterministic record walker of the inflated model stream.
//!
//! Every record has its own length rule (docs/formats/edrawings.md, section
//! 4). The walker never resynchronises: a byte that no rule explains, a
//! record that does not fit its template, an unbalanced segment close, or
//! bytes after the end record stop the walk with a [`WalkError`] that names
//! the byte offset of the record.

use cadmpeg_core::decode::{u64_from_index, View};

use crate::dialect::Grammar;

/// Deepest segment nesting the walker accepts.
pub(super) const MAX_SEGMENT_DEPTH: usize = 256;
/// Most vertices one shell record may declare.
pub(super) const MAX_SHELL_VERTICES: usize = 1 << 24;
/// Most mask words one attribute record may carry.
const MAX_MASK_WORDS: usize = 4;

/// Size of one vertex in a shell record: three `f32`.
const VERTEX_BYTES: usize = 12;
/// Length of the trailer record from its `[` through the final end byte.
const TRAILER_LEN: usize = 24;
/// Bytes of a line weight record `=` or an edge weight record after the
/// opcode: one `f32`.
const WEIGHT_PAYLOAD: usize = 4;
/// Bytes of a hole marker record `@` after its opcode.
const MARKER_PAYLOAD: usize = 1;
/// Flag byte of a line pattern or edge pattern record.
const PATTERN_FLAG: u8 = 0xff;
/// Opcode of the edge weight record.
const EDGE_WEIGHT: u8 = 0x0e;
/// Opcode of the edge pattern record.
const EDGE_PATTERN: u8 = 0x0d;
/// Opcode of the wide user option record.
const WIDE_USER_OPTION: u8 = 0x16;
/// Opcode of the polyline set item of a geometry side object.
const POLYLINE_SET: u8 = 0x10;
/// Bytes of a polyline set item between the opcode and the total count.
const POLYLINE_HEAD: usize = 2;
/// Bytes of one polyline point in the `x` and `z` arrays.
const POLYLINE_POINT_BYTES: usize = 8;
/// Bytes of one polyline entry in the `y` array and the length table.
const POLYLINE_ENTRY_BYTES: usize = 4;
/// Bytes of a colour record after the mask bytes: three channels.
const COLOUR_CHANNELS: usize = 3;
/// Top bit of a colour mask byte: another mask byte follows.
const COLOUR_MASK_MORE: u8 = 0x80;
/// Most mask bytes one colour record may carry.
const MAX_COLOUR_MASK_BYTES: usize = 4;
/// Bytes of a tag-0 record after its opcode.
const TAG0_PAYLOAD: usize = 4;
/// Fixed bytes between the zero run of a text object and its length byte.
const TEXT_ZERO_RUN: usize = 12;
/// Marker byte of a text object before its length.
const TEXT_MARK: u8 = 5;
/// Length byte value that announces a 16-bit length.
const TEXT_LONG_MARK: u8 = 0xfe;
/// Bytes between the image tag and the name: a zero run and a `u32`.
const IMAGE_PREFIX: usize = 16;
/// Bytes between the image tag and the name in `SolidWorks` streams: a zero run
/// and one byte.
const IMAGE_PREFIX_SOLIDWORKS: usize = 13;
/// Bytes between the name of an image object and its length.
const IMAGE_GAP: usize = 9;
/// Final bytes of an image object.
const IMAGE_END: [u8; 2] = [0x00, 0xff];
/// Bytes of the `f32` items of a geometry side object after the opcode, by
/// opcode: point `X`, line `l`, arc `C`, and circle `c`.
const GEOMETRY_ITEMS: [(u8, usize); 4] = [(b'X', 3), (b'l', 6), (b'C', 9), (b'c', 9)];
/// Opcodes of the geometry items that end with a zero byte.
const GEOMETRY_ZERO_ENDED: [u8; 2] = *b"Cc";
/// Bytes of an `f32`.
const F32_BYTES: usize = 4;
/// Opcode of the range block after the normals of a shell.
const SHELL_RANGE: u8 = 3;
/// The two bytes after the opcode of the range block of a shell.
const SHELL_RANGE_HEAD: [u8; 2] = [1, 2];
/// Marker byte of the range block before its length.
const SHELL_RANGE_MARK: u8 = 8;
/// Opcode of the face tag after the range block of a shell.
const SHELL_FACE_TAG: u8 = 0x63;
/// Opcode of the narrow user option inside the face tag.
const NARROW_USER_OPTION: u8 = b'U';
/// Final bytes of the stream: the trailer closes with these.
const STREAM_END: [u8; 2] = *b"]z";

/// Opaque `V` records, as the bytes that follow the opcode.
const V_TEMPLATES: [&[u8]; 3] = [
    &[0xff, 0x00, 0xff, 0xff, 0x00, 0x00, 0x07, 0x00],
    &[0xdf, 0x56, 0x0e, 0x20, 0x00, 0x00],
    &[0x80, 0x00, 0x00, 0x01, 0x00, 0x00],
];
/// Opaque `V` records of `SolidWorks` streams that name a text slot or a 2D
/// segment, as the bytes that follow the opcode.
const V_TEMPLATES_SOLIDWORKS: [&[u8]; 2] = [
    &[0xff, 0x00, 0xff, 0xff, 0x00, 0x00, 0x03, 0x00],
    &[0xdf, 0x56, 0x0e, 0x20, 0x00, 0x00],
];
/// The `V` record that follows a texture definition record `t`, after the
/// opcode.
const V_TEXTURE_SOLIDWORKS: [u8; 2] = [0x40, 0x00];
/// Bytes that follow the two strings of a texture definition record `t`.
const TEXTURE_RECORD_SUFFIX: usize = 6;
/// Flag bit of the second material flag byte that appends [`MATERIAL_EXTRA`]
/// bytes.
const MATERIAL_EXTRA_FLAG: u8 = 0x08;
/// Bytes a material record appends when [`MATERIAL_EXTRA_FLAG`] is set.
const MATERIAL_EXTRA: usize = 7;
/// Opcode of a line style entry.
const LINE_STYLE_ENTRY: u8 = 0x4a;
/// The mask of the `R` record with a run of default bytes.
const R_RUN_MASK: [u32; 1] = [0x0010_0000];
/// Bytes of the `R` run record before its pairs.
const R_RUN_HEAD: usize = 16;
/// One pair of the paired form of the `R` run record.
const R_RUN_PAIR: [u8; 2] = [0xff, 0x01];
/// One byte of the single byte form of the `R` run record.
const R_RUN_BYTE: u8 = 0xff;
/// Bytes of the header block past its declared length in `SolidWorks` streams:
/// the declared length leaves out one byte before the closing bracket.
const HEADER_SPAN_SOLIDWORKS: usize = 2;

/// Prefix of a texture definition record, after the opcode.
const TEXTURE_PREFIX: [u8; 3] = [0x40, 0x00, 0x74];
/// Bytes that follow the two strings of a texture definition.
const TEXTURE_SUFFIX: usize = 6;

/// Extra bytes of an `H` record by mask word list.
const H_EXTRA: [(&[u32], usize); 5] = [
    (&[0x8000, 0x0002], 0),
    (&[0x0002], 0),
    (&[0x0001], 0),
    (&[0x8004, 0x0200], 1),
    (&[0x0004], 1),
];
/// Extra bytes of an `R` record by mask word list.
const R_EXTRA: [(&[u32], usize); 7] = [
    (&[0x8001_0000, 0x8000_0000, 0x0000_0002], 1),
    (&[0x8000_0000, 0x0008_0000], 1),
    (&[0x8010_0000, 0x0000_0100], 9),
    (&[0x0000_1000], 1),
    (&[0x0010_1000], 21),
    (&[0x8000_0000, 0x8000_0000, 0x0000_0002], 0),
    (&[0x8101_0000, 0x0000_0010], 13),
];

/// A walk failure: the byte offset of the record and what was wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WalkError {
    /// Offset of the first byte of the failing record.
    pub(super) offset: usize,
    /// What the walker could not place.
    pub(super) reason: &'static str,
}

impl std::fmt::Display for WalkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.reason, self.offset)
    }
}

/// What the object slot `q` that follows refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlotKind {
    /// A shell placeholder.
    Shell,
    /// A text object slot.
    Text,
    /// A texture definition.
    Texture,
    /// Anything else.
    Other,
}

/// An inline shell record. The slices borrow the stream.
#[derive(Debug, Clone, Copy)]
pub(super) struct Shell<'a> {
    /// Object id.
    pub(super) id: u32,
    /// `count` vertices of three `f32` each, in metres.
    pub(super) points: &'a [u8],
    /// The face list section: index width, then strips.
    pub(super) facelist: &'a [u8],
}

/// A shell instance record.
#[derive(Debug, Clone, Copy)]
pub(super) struct Instance {
    /// Object id of the source shell.
    pub(super) source: u64,
    /// Object id of this instance.
    pub(super) own: u64,
    /// Nine matrix rows then the translation, metres.
    pub(super) matrix: [f32; 12],
}

/// One decoded record.
#[derive(Debug, Clone, Copy)]
pub(super) enum Record<'a> {
    /// The header block.
    Header,
    /// The trailer block.
    Trailer,
    /// The camera.
    Camera,
    /// The line style table.
    LineStyles,
    /// A separator byte.
    Separator,
    /// A zero byte between records.
    Pad,
    /// A user option string: Latin-1 bytes.
    UserOption(&'a [u8]),
    /// A wide user option string: UTF-16LE bytes.
    UserOptionWide(&'a [u8]),
    /// A segment opens.
    Open(&'a [u8]),
    /// A segment or side scope closes.
    Close,
    /// An include reference.
    Include(&'a [u8]),
    /// A style reference, a condition, a texture matrix, and the other
    /// records the model does not use.
    Ignored,
    /// A material: the text of its description.
    Material(&'a [u8]),
    /// A modelling matrix.
    Matrix([f32; 12]),
    /// A colour.
    Colour([u8; 3]),
    /// An object slot.
    Slot(SlotKind),
    /// An inline shell.
    Shell(Shell<'a>),
    /// A shell placeholder.
    ShellPlaceholder,
    /// A shell instance.
    Instance(Instance),
    /// An empty side object; opens a scope.
    SideEmpty,
    /// A geometry side object (points, lines, arcs, polylines); opens a
    /// scope.
    SideGeometry,
    /// A text side object (UTF-16LE bytes); opens a scope.
    SideText(u32, &'a [u8]),
    /// An image side object; opens a scope.
    SideImage,
    /// The end of the stream.
    End,
}

/// The walker over one inflated stream.
pub(super) struct Walker<'a> {
    view: View<'a>,
    grammar: Grammar,
    mark: usize,
    depth: usize,
    previous: SlotKind,
    finished: bool,
}

impl<'a> Walker<'a> {
    /// A walker at the start of `view`.
    pub(super) fn new(view: View<'a>, grammar: Grammar) -> Self {
        Self {
            view,
            grammar,
            mark: 0,
            depth: 0,
            previous: SlotKind::Other,
            finished: false,
        }
    }

    /// Offset of the record the last [`Self::next`] call returned.
    pub(super) fn offset(&self) -> usize {
        self.mark
    }

    fn fail<T>(&self, reason: &'static str) -> Result<T, WalkError> {
        Err(WalkError {
            offset: self.mark,
            reason,
        })
    }

    fn u8(&mut self) -> Result<u8, WalkError> {
        match self.view.u8() {
            Some(value) => Ok(value),
            None => self.fail("record is truncated"),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.view.unread().first().copied()
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], WalkError> {
        match self.view.take(count) {
            Some(bytes) => Ok(bytes),
            None => self.fail("record is truncated"),
        }
    }

    fn u16(&mut self) -> Result<u16, WalkError> {
        match self.view.u16_le() {
            Some(value) => Ok(value),
            None => self.fail("record is truncated"),
        }
    }

    fn u32(&mut self) -> Result<u32, WalkError> {
        match self.view.u32_le() {
            Some(value) => Ok(value),
            None => self.fail("record is truncated"),
        }
    }

    fn u64(&mut self) -> Result<u64, WalkError> {
        match self.view.u64_le() {
            Some(value) => Ok(value),
            None => self.fail("record is truncated"),
        }
    }

    fn len32(&mut self) -> Result<usize, WalkError> {
        let value = self.u32()?;
        match usize::try_from(value) {
            Ok(length) => Ok(length),
            Err(_) => self.fail("length exceeds the address space"),
        }
    }

    fn f32s<const N: usize>(&mut self) -> Result<[f32; N], WalkError> {
        let mut values = [0.0_f32; N];
        for value in &mut values {
            *value = match self.view.f32_le() {
                Some(read) => read,
                None => return self.fail("record is truncated"),
            };
        }
        Ok(values)
    }

    fn skip(&mut self, count: usize) -> Result<(), WalkError> {
        self.take(count).map(|_| ())
    }

    fn pstr(&mut self) -> Result<&'a [u8], WalkError> {
        let length = usize::from(self.u8()?);
        self.take(length)
    }

    fn expect(&mut self, byte: u8, reason: &'static str) -> Result<(), WalkError> {
        if self.u8()? == byte {
            Ok(())
        } else {
            self.fail(reason)
        }
    }

    /// Mask words of an attribute record. `wide` selects `u32` words.
    fn mask_words(&mut self, wide: bool) -> Result<Vec<u32>, WalkError> {
        let top = if wide { 0x8000_0000_u32 } else { 0x8000 };
        let mut words = Vec::new();
        loop {
            let word = if wide {
                self.u32()?
            } else {
                u32::from(self.u16()?)
            };
            words.push(word);
            if word & top == 0 {
                return Ok(words);
            }
            if words.len() >= MAX_MASK_WORDS {
                return self.fail("attribute mask has too many words");
            }
        }
    }

    fn attribute(&mut self, wide: bool) -> Result<(), WalkError> {
        let words = self.mask_words(wide)?;
        let width = if wide { 4 } else { 2 };
        self.skip(width * words.len())?;
        if wide && words == R_RUN_MASK {
            self.skip(R_RUN_HEAD)?;
            if self.view.unread().starts_with(&R_RUN_PAIR) {
                while self.view.unread().starts_with(&R_RUN_PAIR) {
                    self.skip(R_RUN_PAIR.len())?;
                }
            } else {
                while self.peek() == Some(R_RUN_BYTE) {
                    self.skip(1)?;
                }
            }
            return Ok(());
        }
        let table: &[(&[u32], usize)] = if wide { &R_EXTRA } else { &H_EXTRA };
        match table.iter().find(|(mask, _)| *mask == words.as_slice()) {
            Some((_, extra)) => self.skip(*extra),
            None => self.fail("attribute mask is not in the known table"),
        }
    }

    fn opaque_v(&mut self) -> Result<SlotKind, WalkError> {
        let rest = self.view.unread();
        if self.grammar.solidworks_layout() {
            for template in V_TEMPLATES_SOLIDWORKS {
                if rest.starts_with(template) {
                    self.skip(template.len())?;
                    return Ok(SlotKind::Text);
                }
            }
            if rest.starts_with(&V_TEXTURE_SOLIDWORKS) {
                self.skip(V_TEXTURE_SOLIDWORKS.len())?;
                return Ok(SlotKind::Texture);
            }
            return self.fail("unknown V payload");
        }
        for template in V_TEMPLATES {
            if rest.starts_with(template) {
                self.skip(template.len())?;
                return Ok(SlotKind::Text);
            }
        }
        if rest.starts_with(&TEXTURE_PREFIX) {
            self.skip(TEXTURE_PREFIX.len())?;
            self.pstr()?;
            self.pstr()?;
            self.skip(TEXTURE_SUFFIX)?;
            return Ok(SlotKind::Texture);
        }
        self.fail("unknown V payload")
    }

    fn shell(&mut self) -> Result<Record<'a>, WalkError> {
        let id = self.u32()?;
        self.expect(0, "shell: byte after the id is not zero")?;
        let count = self.len32()?;
        if count == 0 || count > MAX_SHELL_VERTICES {
            return self.fail("shell: vertex count is zero or exceeds the cap");
        }
        if self
            .view
            .counted(u64_from_index(count), VERTEX_BYTES)
            .is_none()
        {
            return self.fail("shell: vertices exceed the stream");
        }
        let Some(size) = count.checked_mul(VERTEX_BYTES) else {
            return self.fail("shell: vertex bytes overflow");
        };
        let points = self.take(size)?;
        self.expect(1, "shell: no face list marker")?;
        let length = self.len32()?;
        let facelist = self.take(length)?;
        if self.take(3)? != [0x01, 0x07, 0x0c] {
            return self.fail("shell: no normals marker");
        }
        let normals = self.len32()?;
        self.skip(normals)?;
        if self.peek() == Some(SHELL_RANGE) {
            self.shell_range(count)?;
        }
        if self.peek() == Some(SHELL_FACE_TAG) {
            self.shell_face_tag()?;
        }
        self.expect(0, "shell: no terminator")?;
        Ok(Record::Shell(Shell {
            id,
            points,
            facelist,
        }))
    }

    /// The range block after the normals: two `(u, v)` pairs with `u` zero and
    /// one 16-bit value per vertex.
    fn shell_range(&mut self, count: usize) -> Result<(), WalkError> {
        self.skip(1)?;
        if self.take(SHELL_RANGE_HEAD.len())? != SHELL_RANGE_HEAD {
            return self.fail("shell: bad range block head");
        }
        let [u0, _, u1, _] = self.f32s::<4>()?;
        if u0 != 0.0 || u1 != 0.0 {
            return self.fail("shell: range block u is not zero");
        }
        self.expect(SHELL_RANGE_MARK, "shell: no range block marker")?;
        let length = self.len32()?;
        if count.checked_mul(2) != Some(length) {
            return self.fail("shell: range block length is not twice the vertex count");
        }
        self.skip(length)
    }

    /// The face tag after the range block: a user option and a zero byte.
    fn shell_face_tag(&mut self) -> Result<(), WalkError> {
        self.skip(1)?;
        match self.u8()? {
            NARROW_USER_OPTION => {
                let length = usize::from(self.u16()?);
                self.skip(length)?;
            }
            WIDE_USER_OPTION => {
                let length = usize::from(self.u16()?);
                self.skip(2 * length)?;
            }
            _ => return self.fail("shell: face tag without user option"),
        }
        self.expect(0, "shell: no zero byte after the face tag")
    }

    /// The items of a geometry side object, up to the implicit close.
    fn geometry_items(&mut self) -> Result<(), WalkError> {
        loop {
            let Some(opcode) = self.peek() else {
                return self.fail("record is truncated");
            };
            if opcode == POLYLINE_SET {
                self.polyline_set()?;
            } else if let Some((_, floats)) =
                GEOMETRY_ITEMS.iter().find(|(code, _)| *code == opcode)
            {
                self.skip(1 + F32_BYTES * floats)?;
                if GEOMETRY_ZERO_ENDED.contains(&opcode) {
                    self.expect(0, "geometry object: no zero byte after the arc")?;
                }
            } else {
                return Ok(());
            }
        }
    }

    /// A polyline set: a head, the total point count, point counts that sum
    /// to it, the `x` and `z` arrays, and one `y` per polyline.
    fn polyline_set(&mut self) -> Result<(), WalkError> {
        self.skip(1 + POLYLINE_HEAD)?;
        let total = u64::from(self.u32()?);
        let mut sum = 0_u64;
        let mut polylines = 0_usize;
        while sum < total {
            sum += u64::from(self.u32()?);
            polylines += 1;
        }
        if sum != total {
            return self.fail("polyline set: lengths overrun the total");
        }
        let Some(points) = usize::try_from(total)
            .ok()
            .and_then(|count| count.checked_mul(POLYLINE_POINT_BYTES))
        else {
            return self.fail("polyline set: point bytes overflow");
        };
        let Some(ys) = polylines.checked_mul(POLYLINE_ENTRY_BYTES) else {
            return self.fail("polyline set: entry bytes overflow");
        };
        self.skip(points)?;
        self.skip(ys)
    }

    fn side(&mut self) -> Result<Record<'a>, WalkError> {
        let id = self.u32()?;
        match self.peek() {
            Some(0x78) => {
                self.skip(1)?;
                self.skip(TEXT_ZERO_RUN)?;
                self.expect(TEXT_MARK, "text object: no marker")?;
                let mut length = usize::from(self.u8()?);
                if length == usize::from(TEXT_LONG_MARK) {
                    length = usize::from(self.u16()?);
                }
                let text = self.take(length)?;
                self.expect(0, "text object: no terminator")?;
                Ok(Record::SideText(id, text))
            }
            Some(0x69) => {
                self.skip(1)?;
                self.skip(if self.grammar.short_image_header() {
                    IMAGE_PREFIX_SOLIDWORKS
                } else {
                    IMAGE_PREFIX
                })?;
                self.pstr()?;
                self.skip(IMAGE_GAP)?;
                let length = self.len32()?;
                self.skip(length)?;
                if self.take(IMAGE_END.len())? != IMAGE_END {
                    return self.fail("image object: no terminator");
                }
                Ok(Record::SideImage)
            }
            Some(opcode)
                if opcode == POLYLINE_SET || GEOMETRY_ITEMS.iter().any(|(c, _)| *c == opcode) =>
            {
                self.geometry_items()?;
                Ok(Record::SideGeometry)
            }
            Some(_) => Ok(Record::SideEmpty),
            None => self.fail("record is truncated"),
        }
    }

    fn header(&mut self) -> Result<Record<'a>, WalkError> {
        let length = self.len32()?;
        let slack = if self.grammar.solidworks_layout() {
            HEADER_SPAN_SOLIDWORKS
        } else {
            1
        };
        let Some(span) = length.checked_add(slack) else {
            return self.fail("header block: length overflows");
        };
        self.skip(span)?;
        self.expect(b']', "header block: no closing bracket")?;
        Ok(Record::Header)
    }

    fn trailer(&mut self) -> Result<Record<'a>, WalkError> {
        if self.view.remaining() != TRAILER_LEN - 1 || !self.view.unread().ends_with(&STREAM_END) {
            return self.fail("bracket record is not the 24 byte trailer");
        }
        self.skip(TRAILER_LEN - 2)?;
        Ok(Record::Trailer)
    }

    fn open(&mut self) -> Result<Record<'a>, WalkError> {
        let name = self.pstr()?;
        self.depth += 1;
        if self.depth > MAX_SEGMENT_DEPTH {
            return self.fail("segment nesting exceeds the cap");
        }
        Ok(Record::Open(name))
    }

    fn scope(&mut self, record: Record<'a>) -> Result<Record<'a>, WalkError> {
        self.depth += 1;
        if self.depth > MAX_SEGMENT_DEPTH {
            return self.fail("segment nesting exceeds the cap");
        }
        Ok(record)
    }

    fn line_styles(&mut self) -> Result<Record<'a>, WalkError> {
        self.expect(1, "line style table: unknown version")?;
        self.line_style_run()
    }

    /// Line style entries while the next byte opens one.
    fn line_style_run(&mut self) -> Result<Record<'a>, WalkError> {
        while self.peek() == Some(LINE_STYLE_ENTRY) {
            self.skip(1)?;
            self.pstr()?;
            let length = usize::from(self.u16()?);
            self.skip(length)?;
        }
        Ok(Record::LineStyles)
    }

    /// A colour: mask bytes, each with the top bit when another follows, then
    /// three channels.
    fn colour(&mut self) -> Result<Record<'a>, WalkError> {
        let mut masks = 1;
        while self.u8()? & COLOUR_MASK_MORE != 0 {
            masks += 1;
            if masks > MAX_COLOUR_MASK_BYTES {
                return self.fail("colour mask has too many bytes");
            }
        }
        match self.take(COLOUR_CHANNELS)? {
            [r, g, b] => Ok(Record::Colour([*r, *g, *b])),
            _ => self.fail("colour record is truncated"),
        }
    }

    fn material(&mut self) -> Result<Record<'a>, WalkError> {
        let flags = self.take(2)?;
        let text = self.pstr()?;
        let extra = flags
            .last()
            .is_some_and(|flag| flag & MATERIAL_EXTRA_FLAG != 0);
        if extra {
            self.skip(MATERIAL_EXTRA)?;
        }
        Ok(Record::Material(text))
    }

    fn style(&mut self) -> Result<(), WalkError> {
        if self.peek() == Some(0) {
            self.skip(1)?;
            self.pstr()?;
        }
        self.pstr().map(|_| ())
    }

    /// The next record. After [`Record::End`] the stream is fully consumed.
    pub(super) fn next(&mut self) -> Result<Record<'a>, WalkError> {
        self.mark = self.view.read_len();
        if self.finished {
            return self.fail("walk continued past the end record");
        }
        let opcode = self.u8()?;
        let record = match opcode {
            b'[' if self.mark == 0 => self.header()?,
            b'[' => self.trailer()?,
            b'z' => {
                if self.depth != 0 {
                    return self.fail("end record inside an open segment");
                }
                if !self.view.is_empty() {
                    return self.fail("bytes follow the end record");
                }
                self.finished = true;
                Record::End
            }
            b'}' => {
                self.skip(1 + 44)?;
                self.pstr()?;
                Record::Camera
            }
            b'U' => {
                let length = usize::from(self.u16()?);
                Record::UserOption(self.take(length)?)
            }
            WIDE_USER_OPTION => {
                let length = usize::from(self.u16()?);
                Record::UserOptionWide(self.take(2 * length)?)
            }
            b'h' => self.line_styles()?,
            LINE_STYLE_ENTRY if self.grammar.solidworks_layout() => {
                self.pstr()?;
                let length = usize::from(self.u16()?);
                self.skip(length)?;
                self.line_style_run()?
            }
            b't' if self.grammar.solidworks_layout() => {
                self.pstr()?;
                self.pstr()?;
                self.skip(TEXTURE_RECORD_SUFFIX)?;
                Record::Ignored
            }
            b'(' => self.open()?,
            b')' => {
                if self.depth == 0 {
                    return self.fail("unbalanced segment close");
                }
                self.depth -= 1;
                Record::Close
            }
            b'<' => Record::Include(self.pstr()?),
            b'{' => {
                self.style()?;
                Record::Ignored
            }
            b'?' => {
                self.pstr()?;
                Record::Ignored
            }
            b'%' => Record::Matrix(self.f32s::<12>()?),
            b'$' => {
                self.f32s::<12>()?;
                Record::Ignored
            }
            b'~' => self.colour()?,
            b'"' => self.material()?,
            b'q' => Record::Slot(self.previous),
            b'0' => {
                self.skip(TAG0_PAYLOAD)?;
                Record::Ignored
            }
            b'=' | EDGE_WEIGHT => {
                self.skip(WEIGHT_PAYLOAD)?;
                Record::Ignored
            }
            b'-' | EDGE_PATTERN => {
                self.expect(PATTERN_FLAG, "line pattern: flag is not ff")?;
                self.pstr()?;
                Record::Ignored
            }
            b'@' => {
                self.skip(MARKER_PAYLOAD)?;
                Record::Ignored
            }
            b'H' => {
                self.attribute(false)?;
                Record::Ignored
            }
            b'R' => {
                self.attribute(true)?;
                Record::Ignored
            }
            b'V' => {
                self.previous = self.opaque_v()?;
                return Ok(Record::Ignored);
            }
            b'S' if self.peek() == Some(0x0e) => {
                self.skip(1)?;
                self.shell()?
            }
            b'S' => {
                if self.take(4)? != [0x90, 0x02, 0x00, 0x00] {
                    return self.fail("unknown shell variant");
                }
                Record::ShellPlaceholder
            }
            b'&' => {
                let source = self.u64()?;
                let own = self.u64()?;
                self.skip(1)?;
                let matrix = self.f32s::<12>()?;
                Record::Instance(Instance {
                    source,
                    own,
                    matrix,
                })
            }
            b's' => {
                let side = self.side()?;
                self.scope(side)?
            }
            0 => Record::Pad,
            1 => Record::Separator,
            _ => return self.fail("unknown opcode"),
        };
        if !matches!(record, Record::Close | Record::Slot(_) | Record::Pad) {
            self.previous = match record {
                Record::ShellPlaceholder => SlotKind::Shell,
                _ => SlotKind::Other,
            };
        }
        Ok(record)
    }
}

#[cfg(test)]
mod tests;
