// SPDX-License-Identifier: Apache-2.0
//! Frame SAB (ACIS binary) token streams.
//!
//! The active slice of an ASM `.smbh` or `.smb` stream uses one-byte type tags.
//! Payloads have fixed widths or length prefixes. A `0x11` tag terminates a
//! record at subtype depth zero, while `0x0f` and `0x10` delimit subtype scopes.
//! Record names join a chain of `0x0e` sub-identifiers ending in one `0x0d`
//! identifier.
//!
//! [`frame`] returns the indexed [`Record`] table consumed by
//! [`crate::brep`]. Framing every token preserves byte synchronization and
//! record extents without requiring semantic decoding of each payload.

use cadmpeg_ir::wire::le::{f64_at as read_f64, int_at as read_i, vec3_at as read_vec3};
use std::sync::Arc;

/// A decoded SAB token. Only the payload this codec consumes is retained with a
/// typed value; all tokens are still framed so record boundaries stay exact.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// `0x02` unsigned 8-bit.
    Char(u8),
    /// `0x03` signed 16-bit.
    Short(i16),
    /// `0x04` signed integer of the stream's ref width.
    Long(i64),
    /// `0x05` IEEE float32.
    Float(f32),
    /// `0x06` IEEE float64.
    Double(f64),
    /// `0x07`/`0x08`/`0x09`/`0x12` UTF-8 string.
    Str(String),
    /// `0x0a` logical true (also `reversed` in sense fields).
    True,
    /// `0x0b` logical false (also `forward` in sense fields).
    False,
    /// `0x0c` entity reference (`RecordTable` index; `-1` is null).
    Ref(i64),
    /// `0x0f` subtype-scope open.
    SubtypeOpen,
    /// `0x10` subtype-scope close.
    SubtypeClose,
    /// `0x15` enumeration / secondary integer.
    Enum(i64),
    /// `0x13` 3D position `(x, y, z)`.
    Position([f64; 3]),
    /// `0x14` 3D vector `(x, y, z)`.
    Vector3([f64; 3]),
    /// `0x16` 2D `(u, v)` vector.
    Vector2([f64; 2]),
    /// `0x17` `AutoCAD` ASM int64 attribute value.
    Int64(i64),
}

/// One framed record: its `RecordTable` index, assembled name, payload tokens
/// (the tokens after the name chain), and byte extent within the stream.
#[derive(Debug, Clone)]
pub struct Record {
    /// `RecordTable` index. `asmheader` is index 0.
    pub index: usize,
    /// Full `-`-joined record name, e.g. `cone-surface`, `body`.
    pub name: String,
    /// Leading name component used for dispatch, e.g. `cone`, `body`.
    pub head: String,
    /// Payload tokens following the name chain (subtype delimiters included).
    pub tokens: Arc<[Token]>,
    /// Byte offset of the record's first name-chain tag in the stream.
    pub offset: usize,
    /// Byte length of the record including its terminator.
    pub len: usize,
}

impl Record {
    /// The `chunk[i]` value: the `i`-th payload token, as topology field tables
    /// index them.
    pub fn chunk(&self, i: usize) -> Option<&Token> {
        self.tokens.get(i)
    }

    /// The `chunk[i]` as a non-null entity reference index. Returns `None` for a
    /// null reference (`-1`) or a non-reference token.
    pub fn ref_at(&self, i: usize) -> Option<i64> {
        match self.tokens.get(i) {
            Some(Token::Ref(v)) if *v >= 0 => Some(*v),
            _ => None,
        }
    }
}

/// Return the bytes inside payload subtype `token_index` when its immediately
/// following identifier is `expected`.
pub(crate) fn payload_subtype_span<'a>(
    bytes: &'a [u8],
    record: &Record,
    token_index: usize,
    ref_width: usize,
    expected: &str,
) -> Option<&'a [u8]> {
    let range = payload_subtype_range(bytes, record, token_index, ref_width, expected)?;
    bytes.get(range)
}

/// Return the absolute byte range inside payload subtype `token_index` when
/// its immediately following identifier is `expected`.
pub(crate) fn payload_subtype_range(
    bytes: &[u8],
    record: &Record,
    token_index: usize,
    ref_width: usize,
    expected: &str,
) -> Option<std::ops::Range<usize>> {
    let limit = record.offset.checked_add(record.len)?;
    let mut pos = record.offset;
    let mut name_done = false;
    let mut payload_index = 0usize;
    while pos < limit {
        let Ok((lexed, next)) = lex(bytes, pos, ref_width) else {
            return None;
        };
        pos = next;
        match lexed {
            Lexed::SubIdent(_) if !name_done => {}
            Lexed::Ident(_) if !name_done => name_done = true,
            Lexed::Value(token) => {
                name_done = true;
                if payload_index == token_index {
                    if !matches!(token, Token::SubtypeOpen) {
                        return None;
                    }
                    let Ok((Lexed::Ident(name) | Lexed::SubIdent(name), start)) =
                        lex(bytes, pos, ref_width)
                    else {
                        return None;
                    };
                    if name != expected {
                        return None;
                    }
                    pos = start;
                    let mut depth = 1usize;
                    while pos < limit {
                        let token_start = pos;
                        let Ok((nested, next)) = lex(bytes, pos, ref_width) else {
                            return None;
                        };
                        pos = next;
                        match nested {
                            Lexed::Value(Token::SubtypeOpen) => depth += 1,
                            Lexed::Value(Token::SubtypeClose) => {
                                depth -= 1;
                                if depth == 0 {
                                    return Some(start..token_start);
                                }
                            }
                            _ => {}
                        }
                    }
                    return None;
                }
                payload_index += 1;
            }
            Lexed::Terminator => return None,
            Lexed::Ident(_) | Lexed::SubIdent(_) => {}
        }
    }
    None
}

/// Return the absolute byte offset of one payload token by its framed index.
pub(crate) fn payload_token_offset(
    bytes: &[u8],
    record: &Record,
    ref_width: usize,
    token_index: usize,
) -> Option<usize> {
    let limit = record.offset.checked_add(record.len)?;
    let mut position = record.offset;
    let mut name_done = false;
    let mut payload_index = 0usize;
    while position < limit {
        let token_offset = position;
        let (lexed, next) = lex(bytes, position, ref_width).ok()?;
        position = next;
        match lexed {
            Lexed::SubIdent(_) if !name_done => {}
            Lexed::Ident(_) if !name_done => name_done = true,
            Lexed::Value(_) => {
                name_done = true;
                if payload_index == token_index {
                    return Some(token_offset);
                }
                payload_index += 1;
            }
            Lexed::Terminator => return None,
            Lexed::Ident(_) | Lexed::SubIdent(_) => {}
        }
    }
    None
}

/// A framing error: an unrecognized tag or a truncated token payload leaves the
/// stream un-synchronizable, so the caller falls back to metadata-only decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    /// Byte offset where framing could not continue.
    pub offset: usize,
    /// What went wrong.
    pub reason: String,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SAB framing failed at byte {}: {}",
            self.offset, self.reason
        )
    }
}

/// Read one token starting at `pos`. Returns the token (or a control marker) and
/// the offset just past it. Control tags (`0x0d`/`0x0e` name tokens, `0x11`
/// terminator) are returned via [`Lexed`] so the framer can act on them.
enum Lexed {
    /// A payload token.
    Value(Token),
    /// `0x0d` identifier (name terminator).
    Ident(String),
    /// `0x0e` sub-identifier (name component).
    SubIdent(String),
    /// `0x11` record terminator.
    Terminator,
}

fn read_string(bytes: &[u8], start: usize, len: usize) -> Option<String> {
    let slice = bytes.get(start..start + len)?;
    Some(String::from_utf8_lossy(slice).into_owned())
}

fn lex(bytes: &[u8], pos: usize, ref_width: usize) -> Result<(Lexed, usize), FrameError> {
    let err = |reason: &str| FrameError {
        offset: pos,
        reason: reason.to_string(),
    };
    let tag = *bytes.get(pos).ok_or_else(|| err("end of stream"))?;
    let p = pos + 1;
    let truncated = || FrameError {
        offset: pos,
        reason: format!("truncated payload for tag {tag:#04x}"),
    };
    let out = match tag {
        0x02 => (
            Lexed::Value(Token::Char(*bytes.get(p).ok_or_else(truncated)?)),
            p + 1,
        ),
        0x03 => {
            let s = bytes.get(p..p + 2).ok_or_else(truncated)?;
            (
                Lexed::Value(Token::Short(i16::from_le_bytes(
                    s.try_into()
                        .expect("invariant: bytes.get(p..p+2) is a 2-byte slice"),
                ))),
                p + 2,
            )
        }
        0x04 => {
            let v = read_i(bytes, p, ref_width).ok_or_else(truncated)?;
            (Lexed::Value(Token::Long(v)), p + ref_width)
        }
        0x05 => {
            let s = bytes.get(p..p + 4).ok_or_else(truncated)?;
            (
                Lexed::Value(Token::Float(f32::from_le_bytes(
                    s.try_into()
                        .expect("invariant: bytes.get(p..p+4) is a 4-byte slice"),
                ))),
                p + 4,
            )
        }
        0x06 => (
            Lexed::Value(Token::Double(read_f64(bytes, p).ok_or_else(truncated)?)),
            p + 8,
        ),
        0x07 => {
            let len = *bytes.get(p).ok_or_else(truncated)? as usize;
            (
                Lexed::Value(Token::Str(
                    read_string(bytes, p + 1, len).ok_or_else(truncated)?,
                )),
                p + 1 + len,
            )
        }
        0x08 => {
            let s = bytes.get(p..p + 2).ok_or_else(truncated)?;
            let len = u16::from_le_bytes(
                s.try_into()
                    .expect("invariant: bytes.get(p..p+2) is a 2-byte slice"),
            ) as usize;
            (
                Lexed::Value(Token::Str(
                    read_string(bytes, p + 2, len).ok_or_else(truncated)?,
                )),
                p + 2 + len,
            )
        }
        0x09 | 0x12 => {
            let s = bytes.get(p..p + 4).ok_or_else(truncated)?;
            let len = u32::from_le_bytes(
                s.try_into()
                    .expect("invariant: bytes.get(p..p+4) is a 4-byte slice"),
            ) as usize;
            (
                Lexed::Value(Token::Str(
                    read_string(bytes, p + 4, len).ok_or_else(truncated)?,
                )),
                p + 4 + len,
            )
        }
        0x0a => (Lexed::Value(Token::True), p),
        0x0b => (Lexed::Value(Token::False), p),
        0x0c => {
            let v = read_i(bytes, p, ref_width).ok_or_else(truncated)?;
            (Lexed::Value(Token::Ref(v)), p + ref_width)
        }
        0x0d => {
            let len = *bytes.get(p).ok_or_else(truncated)? as usize;
            (
                Lexed::Ident(read_string(bytes, p + 1, len).ok_or_else(truncated)?),
                p + 1 + len,
            )
        }
        0x0e => {
            let len = *bytes.get(p).ok_or_else(truncated)? as usize;
            (
                Lexed::SubIdent(read_string(bytes, p + 1, len).ok_or_else(truncated)?),
                p + 1 + len,
            )
        }
        0x0f => (Lexed::Value(Token::SubtypeOpen), p),
        0x10 => (Lexed::Value(Token::SubtypeClose), p),
        0x11 => (Lexed::Terminator, p),
        0x13 => (
            Lexed::Value(Token::Position(read_vec3(bytes, p).ok_or_else(truncated)?)),
            p + 24,
        ),
        0x14 => (
            Lexed::Value(Token::Vector3(read_vec3(bytes, p).ok_or_else(truncated)?)),
            p + 24,
        ),
        0x15 => {
            let v = read_i(bytes, p, ref_width).ok_or_else(truncated)?;
            (Lexed::Value(Token::Enum(v)), p + ref_width)
        }
        0x16 => {
            let u = read_f64(bytes, p).ok_or_else(truncated)?;
            let v = read_f64(bytes, p + 8).ok_or_else(truncated)?;
            (Lexed::Value(Token::Vector2([u, v])), p + 16)
        }
        0x17 => {
            let v = read_i(bytes, p, 8).ok_or_else(truncated)?;
            (Lexed::Value(Token::Int64(v)), p + 8)
        }
        other => {
            return Err(FrameError {
                offset: pos,
                reason: format!("unrecognized tag {other:#04x}"),
            })
        }
    };
    Ok(out)
}

/// Byte offsets of payload tokens with `tag` inside one framed record.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn payload_token_offsets(
    bytes: &[u8],
    record: &Record,
    ref_width: usize,
    tag: u8,
) -> Result<Vec<usize>, FrameError> {
    let end = record.offset + record.len;
    let mut position = record.offset;
    let mut offsets = Vec::new();
    while position < end {
        let token_offset = position;
        let (token, next) = lex(bytes, position, ref_width)?;
        if bytes[token_offset] == tag && matches!(&token, Lexed::Value(_)) {
            offsets.push(token_offset);
        }
        position = next;
        if matches!(&token, Lexed::Terminator) {
            break;
        }
    }
    Ok(offsets)
}

/// Frame `bytes[start..limit]` into an indexed record table.
///
/// `ref_width` is the stream's reference width (8 for `BinaryFile8`). Framing
/// stops at `limit`, the end of the byte slice, or the `delta_state` history
/// boundary.
pub fn frame(
    bytes: &[u8],
    start: usize,
    limit: usize,
    ref_width: usize,
) -> Result<Vec<Record>, FrameError> {
    frame_impl(bytes, start, limit, ref_width, false)
}

/// Frame a history-section slice whose final record ends at the enclosing
/// stream boundary without an explicit `0x11` terminator.
pub(crate) fn frame_history(
    bytes: &[u8],
    start: usize,
    limit: usize,
    ref_width: usize,
) -> Result<Vec<Record>, FrameError> {
    frame_impl(bytes, start, limit, ref_width, true)
}

fn frame_impl(
    bytes: &[u8],
    start: usize,
    limit: usize,
    ref_width: usize,
    eof_terminates_final_record: bool,
) -> Result<Vec<Record>, FrameError> {
    let limit = limit.min(bytes.len());
    let mut records = Vec::new();
    let mut pos = start;
    let mut index = 0usize;

    while pos < limit {
        let rec_start = pos;
        let mut name_parts: Vec<String> = Vec::new();
        let mut tokens: Vec<Token> = Vec::new();
        let mut depth = 0i32;
        let mut name_done = false;
        let mut is_delta = false;
        let mut embedded_history_entity = None;
        let mut payload_start = true;

        loop {
            if eof_terminates_final_record && pos == limit && depth == 0 && !name_parts.is_empty() {
                break;
            }
            let (lexed, next) = lex(bytes, pos, ref_width)?;
            pos = next;
            match lexed {
                Lexed::Terminator if depth == 0 => break,
                Lexed::Terminator => {
                    // A terminator inside a subtype scope is not a record end;
                    // preserve nothing but keep scanning (does not occur in
                    // well-formed streams, guarded defensively).
                }
                Lexed::SubIdent(s) if !name_done => name_parts.push(s),
                Lexed::Ident(s) if !name_done => {
                    name_parts.push(s);
                    name_done = true;
                    // The history partition opens with the delta_state record;
                    // stop at its name before consuming a payload the active
                    // slice does not include.
                    if name_parts.first().is_some_and(|n| n == "delta_state") {
                        is_delta = true;
                        break;
                    }
                }
                Lexed::Ident(identifier) => {
                    // Identifier tokens after the name belong to the payload
                    // (e.g. subtype names inside a spline). An archived ASM
                    // history record may wrap an edge record in the exact
                    // End-of-ASM-History-Section marker chain; its following
                    // identifier is the wrapped record's dispatch name.
                    if payload_start
                        && name_parts.join("-") == "End-of-ASM-History-Section"
                        && identifier == "edge"
                    {
                        embedded_history_entity = Some(identifier);
                    }
                    payload_start = false;
                }
                Lexed::SubIdent(_) => payload_start = false,
                Lexed::Value(Token::SubtypeOpen) => {
                    payload_start = false;
                    depth += 1;
                    name_done = true;
                    tokens.push(Token::SubtypeOpen);
                }
                Lexed::Value(Token::SubtypeClose) => {
                    payload_start = false;
                    depth -= 1;
                    tokens.push(Token::SubtypeClose);
                }
                Lexed::Value(v) => {
                    payload_start = false;
                    name_done = true;
                    tokens.push(v);
                }
            }
        }

        if is_delta {
            break;
        }
        let mut name = name_parts.join("-");
        if let Some(embedded) = embedded_history_entity {
            name = embedded;
        }
        let head = name.split('-').next().unwrap_or_default().to_owned();

        records.push(Record {
            index,
            name,
            head,
            tokens: tokens.into(),
            offset: rec_start,
            len: pos - rec_start,
        });
        index += 1;
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::{frame, frame_history, payload_subtype_span, payload_token_offset};

    #[test]
    fn history_framer_accepts_only_the_final_record_at_eof() {
        let bytes = [0x0d, 4, b'e', b'd', b'g', b'e'];
        assert!(frame(&bytes, 0, bytes.len(), 8).is_err());
        let records = frame_history(&bytes, 0, bytes.len(), 8).expect("history EOF record");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "edge");
        assert_eq!(records[0].len, bytes.len());
    }

    #[test]
    fn history_marker_dispatches_its_embedded_edge_record() {
        let mut bytes = Vec::new();
        for part in ["End", "of", "ASM", "History"] {
            bytes.extend_from_slice(&[0x0e, u8::try_from(part.len()).unwrap()]);
            bytes.extend_from_slice(part.as_bytes());
        }
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"Section");
        bytes.extend_from_slice(&[0x0d, 4]);
        bytes.extend_from_slice(b"edge");
        for reference in [4i64, -1, 5, 6, 7, 8] {
            bytes.push(0x0c);
            bytes.extend_from_slice(&reference.to_le_bytes());
        }
        bytes.push(0x11);

        let records = frame_history(&bytes, 0, bytes.len(), 8).expect("wrapped edge");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "edge");
        assert_eq!(records[0].head, "edge");
        assert_eq!(records[0].ref_at(0), Some(4));
        assert_eq!(records[0].ref_at(5), Some(8));

        let embedded_offset = bytes
            .windows(6)
            .position(|window| window == [0x0d, 4, b'e', b'd', b'g', b'e'])
            .unwrap();
        let mut non_wrapper = bytes;
        non_wrapper.splice(embedded_offset..embedded_offset, [0x02, 0]);
        let records = frame_history(&non_wrapper, 0, non_wrapper.len(), 8)
            .expect("marker with a later payload identifier");
        assert_eq!(records[0].name, "End-of-ASM-History-Section");
    }

    fn generated_pcurve_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 6];
        bytes.extend_from_slice(b"pcurve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1), (0x04, 0)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.extend_from_slice(&[0x0b, 0x0f, 0x0d, 11]);
        bytes.extend_from_slice(b"exp_par_cur");
        bytes.extend_from_slice(&[0x02, 0x7f, 0x10, 0x11]);
        bytes
    }

    fn generated_ref_pcurve_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 6];
        bytes.extend_from_slice(b"pcurve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1), (0x04, 2), (0x0c, 20)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        for value in [-2.0f64, 4.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_cone_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 4];
        bytes.extend_from_slice(b"cone");
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"surface");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        for (tag, values) in [
            (0x13, [1.0f64, 2.0, 3.0]),
            (0x14, [0.0, 0.0, 1.0]),
            (0x14, [4.0, 0.0, 0.0]),
        ] {
            bytes.push(tag);
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&0.5f64.to_le_bytes());
        bytes.extend_from_slice(&[0x0b, 0x0b]);
        for value in [-0.25f64, 0.75, 4.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0x0b; 5]);
        bytes.push(0x11);
        bytes
    }

    fn generated_sphere_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 6];
        bytes.extend_from_slice(b"sphere");
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"surface");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&(-2.5f64).to_le_bytes());
        for values in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            bytes.push(0x14);
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&[0x0b; 5]);
        bytes.push(0x11);
        bytes
    }

    fn generated_torus_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 5];
        bytes.extend_from_slice(b"torus");
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"surface");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        for (tag, values) in [(0x13, [1.0f64, 2.0, 3.0]), (0x14, [0.0, 0.0, 1.0])] {
            bytes.push(tag);
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in [4.0f64, -5.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [1.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0x0b; 5]);
        bytes.push(0x11);
        bytes
    }

    fn generated_plane_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 5];
        bytes.extend_from_slice(b"plane");
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"surface");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        for (tag, values) in [
            (0x13, [1.0f64, 2.0, 3.0]),
            (0x14, [0.0, 0.0, 1.0]),
            (0x14, [1.0, 0.0, 0.0]),
        ] {
            bytes.push(tag);
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&[0x0b, 0x11]);
        bytes
    }

    fn generated_ellipse_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 7];
        bytes.extend_from_slice(b"ellipse");
        bytes.extend_from_slice(&[0x0d, 5]);
        bytes.extend_from_slice(b"curve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        for (tag, values) in [
            (0x13, [1.0f64, 2.0, 3.0]),
            (0x14, [0.0, 0.0, 1.0]),
            (0x14, [4.0, 0.0, 0.0]),
        ] {
            bytes.push(tag);
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&(-0.5f64).to_le_bytes());
        bytes.push(0x11);
        bytes
    }

    fn generated_straight_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 8];
        bytes.extend_from_slice(b"straight");
        bytes.extend_from_slice(&[0x0d, 5]);
        bytes.extend_from_slice(b"curve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        for (tag, values) in [(0x13, [1.0f64, 2.0, 3.0]), (0x14, [4.0, 5.0, 6.0])] {
            bytes.push(tag);
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_degenerate_curve_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 16];
        bytes.extend_from_slice(b"degenerate_curve");
        bytes.extend_from_slice(&[0x0d, 5]);
        bytes.extend_from_slice(b"curve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0x0b, 0x0b, 0x11]);
        bytes
    }

    fn generated_point_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 5];
        bytes.extend_from_slice(b"point");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_edge_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 4];
        bytes.extend_from_slice(b"edge");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1), (0x0c, 10)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&(-2.0f64).to_le_bytes());
        bytes.push(0x0c);
        bytes.extend_from_slice(&11i64.to_le_bytes()[..ref_width]);
        bytes.push(0x06);
        bytes.extend_from_slice(&3.0f64.to_le_bytes());
        for value in [-1i64, 12] {
            bytes.push(0x0c);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.extend_from_slice(&[0x0b, 0x07, 7]);
        bytes.extend_from_slice(b"unknown");
        bytes.push(0x11);
        bytes
    }

    fn generated_tedge_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = generated_edge_record(ref_width);
        bytes.splice(1..6, [5, b't', b'e', b'd', b'g', b'e']);
        bytes.pop();
        bytes.push(0x06);
        bytes.extend_from_slice(&0.0035f64.to_le_bytes());
        for value in [22800i64, 0] {
            bytes.push(0x04);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_tcoedge_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 7];
        bytes.extend_from_slice(b"tcoedge");
        for (tag, value) in [
            (0x0c, -1i64),
            (0x04, -1),
            (0x0c, -1),
            (0x0c, 1),
            (0x0c, 2),
            (0x0c, 3),
            (0x0c, 4),
        ] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.push(0x0b);
        for (tag, value) in [(0x0c, 5i64), (0x04, 0), (0x0c, 6)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        for value in [-2.0f64, 3.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [-1i64, 0, 0] {
            bytes.push(if value == -1 { 0x0c } else { 0x04 });
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_face_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 4];
        bytes.extend_from_slice(b"face");
        for (tag, value) in [
            (0x0c, -1i64),
            (0x04, -1),
            (0x0c, -1),
            (0x0c, -1),
            (0x0c, 1),
            (0x0c, 2),
            (0x04, 0),
            (0x0c, 3),
        ] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.extend_from_slice(&[0x0b, 0x0a, 0x0b, 0x11]);
        bytes
    }

    fn generated_tvertex_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 7];
        bytes.extend_from_slice(b"tvertex");
        for (tag, value) in [
            (0x0c, -1i64),
            (0x04, -1),
            (0x0c, -1),
            (0x0c, 10),
            (0x04, 1),
            (0x0c, 11),
        ] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        // Three f64 tolerance slots — two unevaluated `-1` sentinels and the
        // evaluated tolerance last — followed by an integer 0.
        for value in [-1.0f64, -1.0, 0.001] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x04);
        bytes.extend_from_slice(&0i64.to_le_bytes()[..ref_width]);
        bytes.push(0x11);
        bytes
    }

    fn generated_body_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 4];
        bytes.extend_from_slice(b"body");
        for (tag, value) in [
            (0x0c, -1i64),
            (0x04, 42),
            (0x0c, -1),
            (0x0c, 1),
            (0x0c, -1),
            (0x0c, -1),
        ] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_transform_record() -> Vec<u8> {
        let mut bytes = vec![0x0d, 9];
        bytes.extend_from_slice(b"transform");
        for vector in [
            [1.0f64, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [2.0, 3.0, 4.0],
        ] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        bytes.extend_from_slice(&[0x0b, 0x0a, 0x0b, 0x11]);
        bytes
    }

    fn generated_wire_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0d, 4];
        bytes.extend_from_slice(b"wire");
        for (tag, value) in [
            (0x0c, -1i64),
            (0x04, -1),
            (0x0c, -1),
            (0x0c, -1),
            (0x0c, 1),
            (0x0c, 2),
            (0x0c, -1),
        ] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width]);
        }
        bytes.extend_from_slice(&[0x0b, 0x11]);
        bytes
    }

    fn generated_rgb_attribute_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 9];
        bytes.extend_from_slice(b"rgb_color");
        bytes.extend_from_slice(&[0x0e, 2]);
        bytes.extend_from_slice(b"st");
        bytes.extend_from_slice(&[0x0d, 6]);
        bytes.extend_from_slice(b"attrib");
        bytes.push(0x0c);
        bytes.extend_from_slice(&(-1i64).to_le_bytes()[..ref_width]);
        for value in [0.1f64, 0.2, 0.3] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_timestamp_attribute_record(ref_width: usize) -> Vec<u8> {
        let mut bytes = vec![0x0e, 13];
        bytes.extend_from_slice(b"ATTRIB_CUSTOM");
        bytes.extend_from_slice(&[0x0d, 6]);
        bytes.extend_from_slice(b"attrib");
        bytes.push(0x0c);
        bytes.extend_from_slice(&(-1i64).to_le_bytes()[..ref_width]);
        bytes.extend_from_slice(&[0x07, 20]);
        bytes.extend_from_slice(b"Timestamp_attrib_def");
        bytes.push(0x04);
        bytes.extend_from_slice(&1i64.to_le_bytes()[..ref_width]);
        bytes.push(0x06);
        bytes.extend_from_slice(&1_579_392_000_000_007.0f64.to_le_bytes());
        bytes.push(0x11);
        bytes
    }

    #[test]
    fn generated_payload_subtype_lookup_uses_declared_integer_width() {
        for ref_width in [4, 8] {
            let bytes = generated_pcurve_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated record");
            let record = records.first().expect("generated pcurve");
            assert!(payload_subtype_span(&bytes, record, 5, ref_width, "exp_par_cur").is_some());
            assert!(payload_subtype_span(&bytes, record, 4, ref_width, "exp_par_cur").is_none());
            assert!(payload_subtype_span(&bytes, record, 5, ref_width, "bad_par_cur").is_none());
            assert_eq!(
                bytes[payload_token_offset(&bytes, record, ref_width, 4)
                    .expect("required invariant")],
                0x0b
            );
            assert_eq!(
                bytes[payload_token_offset(&bytes, record, ref_width, 5)
                    .expect("required invariant")],
                0x0f
            );
            assert!(payload_token_offset(&bytes, record, ref_width, 8).is_none());
        }
    }

    #[test]
    fn generated_ref_pcurve_range_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_ref_pcurve_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated ref pcurve");
            let record = &records[0];
            for (index, expected) in [(5usize, -2.0f64), (6, 4.0)] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("range field offset");
                assert_eq!(bytes[offset], 0x06);
                assert_eq!(
                    f64::from_le_bytes(
                        bytes[offset + 1..offset + 9]
                            .try_into()
                            .expect("required invariant")
                    ),
                    expected
                );
            }
        }
    }

    #[test]
    fn generated_cone_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_cone_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated cone");
            let record = &records[0];
            for (index, tag) in [
                (3usize, 0x13),
                (4, 0x14),
                (5, 0x14),
                (6, 0x06),
                (9, 0x06),
                (10, 0x06),
                (11, 0x06),
            ] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("cone field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_sphere_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_sphere_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated sphere");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x06), (5, 0x14), (6, 0x14)] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("sphere field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_torus_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_torus_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated torus");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x14), (5, 0x06), (6, 0x06), (7, 0x14)] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("torus field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_plane_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_plane_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated plane");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x14), (5, 0x14)] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("plane field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_ellipse_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_ellipse_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated ellipse");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x14), (5, 0x14), (6, 0x06)] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("ellipse field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_straight_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_straight_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated straight");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x14)] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("straight field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_degenerate_curve_has_fixed_point_field_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_degenerate_curve_record(ref_width);
            let records =
                frame(&bytes, 0, bytes.len(), ref_width).expect("generated degenerate curve");
            let record = &records[0];
            let offset = payload_token_offset(&bytes, record, ref_width, 3)
                .expect("degenerate point offset");
            assert_eq!(bytes[offset], 0x13);
        }
    }

    #[test]
    fn generated_point_has_fixed_position_field_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_point_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated point");
            let record = &records[0];
            let offset =
                payload_token_offset(&bytes, record, ref_width, 3).expect("point position offset");
            assert_eq!(bytes[offset], 0x13);
        }
    }

    #[test]
    fn generated_topology_ranges_have_fixed_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let edge = generated_edge_record(ref_width);
            let records = frame(&edge, 0, edge.len(), ref_width).expect("generated edge");
            for index in [4usize, 6] {
                let offset = payload_token_offset(&edge, &records[0], ref_width, index)
                    .expect("edge range offset");
                assert_eq!(edge[offset], 0x06);
            }

            let edge = generated_tedge_record(ref_width);
            let records = frame(&edge, 0, edge.len(), ref_width).expect("generated tolerant edge");
            assert!(
                matches!(records[0].chunk(11), Some(super::Token::Double(value)) if *value == 0.0035)
            );
            assert!(matches!(
                records[0].chunk(12),
                Some(super::Token::Long(22800))
            ));
            assert!(matches!(records[0].chunk(13), Some(super::Token::Long(0))));

            let coedge = generated_tcoedge_record(ref_width);
            let records =
                frame(&coedge, 0, coedge.len(), ref_width).expect("generated tolerant coedge");
            for index in [11usize, 12] {
                let offset = payload_token_offset(&coedge, &records[0], ref_width, index)
                    .expect("tolerant coedge parameter offset");
                assert_eq!(coedge[offset], 0x06);
            }
            assert!(matches!(records[0].chunk(13), Some(super::Token::Ref(-1))));
            assert!(matches!(records[0].chunk(14), Some(super::Token::Long(0))));
            assert!(matches!(records[0].chunk(15), Some(super::Token::Long(0))));
        }
    }

    #[test]
    fn generated_topology_senses_have_fixed_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let face = generated_face_record(ref_width);
            let records = frame(&face, 0, face.len(), ref_width).expect("generated face");
            for index in [8usize, 9, 10] {
                let offset = payload_token_offset(&face, &records[0], ref_width, index)
                    .expect("face sense field");
                assert!(matches!(face[offset], 0x0a | 0x0b));
            }

            let coedge = generated_tcoedge_record(ref_width);
            let records =
                frame(&coedge, 0, coedge.len(), ref_width).expect("generated tolerant coedge");
            let offset = payload_token_offset(&coedge, &records[0], ref_width, 7)
                .expect("coedge sense field");
            assert_eq!(coedge[offset], 0x0b);

            let edge = generated_edge_record(ref_width);
            let records = frame(&edge, 0, edge.len(), ref_width).expect("generated edge");
            let sense =
                payload_token_offset(&edge, &records[0], ref_width, 9).expect("edge sense field");
            let continuity = payload_token_offset(&edge, &records[0], ref_width, 10)
                .expect("edge continuity field");
            assert_eq!(edge[sense], 0x0b);
            assert_eq!(edge[continuity], 0x07);
        }
    }

    #[test]
    fn generated_tolerant_vertex_has_fixed_metadata_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_tvertex_record(ref_width);
            let records =
                frame(&bytes, 0, bytes.len(), ref_width).expect("generated tolerant vertex");
            let record = &records[0];
            for (index, tag) in [
                (3usize, 0x0c),
                (4, 0x04),
                (5, 0x0c),
                (6, 0x06),
                (7, 0x06),
                (8, 0x06),
                (9, 0x04),
            ] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("tolerant vertex metadata field");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_ownership_keys_have_fixed_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let body = generated_body_record(ref_width);
            let records = frame(&body, 0, body.len(), ref_width).expect("generated body");
            let key =
                payload_token_offset(&body, &records[0], ref_width, 1).expect("body key field");
            assert_eq!(body[key], 0x04);

            let edge = generated_edge_record(ref_width);
            let records = frame(&edge, 0, edge.len(), ref_width).expect("generated edge");
            let owner =
                payload_token_offset(&edge, &records[0], ref_width, 7).expect("edge owner field");
            assert_eq!(edge[owner], 0x0c);
        }
    }

    #[test]
    fn generated_transform_has_fixed_matrix_and_hint_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_transform_record();
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated transform");
            let record = &records[0];
            for (index, tag) in [(0usize, 0x14), (1, 0x14), (2, 0x14), (3, 0x14), (4, 0x06)] {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("transform numeric field");
                assert_eq!(bytes[offset], tag);
            }
            for index in 5..=7 {
                let offset = payload_token_offset(&bytes, record, ref_width, index)
                    .expect("transform hint field");
                assert!(matches!(bytes[offset], 0x0a | 0x0b));
            }
        }
    }

    #[test]
    fn generated_wire_has_fixed_side_field_at_both_widths() {
        for ref_width in [4, 8] {
            let bytes = generated_wire_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated wire");
            let offset =
                payload_token_offset(&bytes, &records[0], ref_width, 7).expect("wire side field");
            assert_eq!(bytes[offset], 0x0b);
        }
    }

    #[test]
    fn generated_attribute_values_have_semantic_fields_at_both_widths() {
        for ref_width in [4, 8] {
            let color = generated_rgb_attribute_record(ref_width);
            let records =
                frame(&color, 0, color.len(), ref_width).expect("generated RGB attribute");
            for index in 1..=3 {
                let offset = payload_token_offset(&color, &records[0], ref_width, index)
                    .expect("RGB channel field");
                assert_eq!(color[offset], 0x06);
            }

            let timestamp = generated_timestamp_attribute_record(ref_width);
            let records = frame(&timestamp, 0, timestamp.len(), ref_width)
                .expect("generated timestamp attribute");
            for (index, tag) in [(1usize, 0x07), (2, 0x04), (3, 0x06)] {
                let offset = payload_token_offset(&timestamp, &records[0], ref_width, index)
                    .expect("timestamp semantic field");
                assert_eq!(timestamp[offset], tag);
            }
        }
    }
}
