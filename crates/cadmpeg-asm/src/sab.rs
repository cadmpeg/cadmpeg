// SPDX-License-Identifier: Apache-2.0
//! Frame SAB (ACIS binary) token streams.
//!
//! The active slice of an ASM `.smbh` or `.smb` stream uses one-byte type tags.
//! Payloads have fixed widths or length prefixes. A `0x11` tag terminates a
//! record at subtype depth zero, while `0x0f` and `0x10` delimit subtype scopes.
//! Record names join a chain of `0x0e` sub-identifiers ending in one `0x0d`
//! identifier.
//!
//! [`frame`] returns an indexed [`Record`] table. Framing every token preserves
//! byte synchronization and record extents without requiring semantic decoding
//! of each payload.

use crate::kernel_header::RefWidth;
use crate::stream_error::{StreamError, StreamFailure, StreamFormat};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use std::sync::Arc;

pub(crate) fn int_le_at(bytes: &[u8], offset: usize, width: RefWidth) -> Option<i64> {
    match width {
        RefWidth::Four => Some(i64::from(View::i32_le_at(bytes, offset)?)),
        RefWidth::Eight => View::i64_le_at(bytes, offset),
    }
}

pub(crate) fn vec3_le_at(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
    Some([
        View::f64_le_at(bytes, offset)?,
        View::f64_le_at(bytes, offset.checked_add(8)?)?,
        View::f64_le_at(bytes, offset.checked_add(16)?)?,
    ])
}

/// Whether an exact short identifier token begins at `at`.
///
/// Header partition scanners use this after the framed solved records. Keeping
/// the token law with SAB framing prevents ACIS and ASM headers from defining
/// subtly different boundary recognizers.
#[cfg(test)]
pub(crate) fn exact_identifier_at(bytes: &[u8], at: usize, expected: &str) -> bool {
    let Some((&0x0d, rest)) = bytes.get(at..).and_then(|tail| tail.split_first()) else {
        return false;
    };
    let Some((&length, payload)) = rest.split_first() else {
        return false;
    };
    usize::from(length) == expected.len()
        && payload.get(..usize::from(length)) == Some(expected.as_bytes())
}

/// Scan record boundaries and exact names without constructing a record table.
/// The history-partition classifiers use this before the admitted SAB parse.
pub(crate) fn scan_history_boundary(
    bytes: &[u8],
    start: usize,
    ref_width: RefWidth,
    preamble: Option<&[&str]>,
) -> Option<usize> {
    let mut pos = start;
    while pos < bytes.len() {
        let rec_start = pos;
        let mut name_index = 0usize;
        let mut preamble_matches = preamble.is_some();
        let mut delta_matches = true;
        let mut preamble_candidate = false;
        let mut name_done = false;
        let mut depth = 0usize;
        loop {
            let (lexed, next) = lex(bytes, pos, ref_width).ok()?;
            pos = next;
            let terminal_name = matches!(lexed, Lexed::Ident(_));
            match lexed {
                Lexed::SubIdent(part) | Lexed::Ident(part) if !name_done => {
                    preamble_matches &= preamble
                        .and_then(|parts| parts.get(name_index))
                        .is_some_and(|expected| *expected == part);
                    delta_matches &= name_index == 0 && part == "delta_state";
                    name_index += 1;
                    if terminal_name {
                        name_done = true;
                        preamble_candidate = (preamble_matches
                            && preamble.is_some_and(|parts| parts.len() == name_index))
                            || (preamble.is_some()
                                && name_index == 1
                                && part == "Begin-of-ASM-History-Data");
                        if delta_matches && name_index == 1 {
                            return Some(rec_start);
                        }
                    }
                }
                Lexed::Value(Token::SubtypeOpen) => depth = depth.checked_add(1)?,
                Lexed::Value(Token::SubtypeClose) => depth = depth.checked_sub(1)?,
                Lexed::Terminator if depth == 0 => {
                    if preamble_candidate {
                        return Some(rec_start);
                    }
                    break;
                }
                Lexed::Terminator => return None,
                _ => {}
            }
        }
    }
    None
}

/// A decoded SAB token. The codec assigns typed values to the payload it
/// consumes; framing preserves every token so record boundaries stay exact.
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
    /// `0x07`/`0x08`/`0x09`/`0x12` UTF-8 string. The `0x07` length prefix is
    /// one byte, `0x08` two bytes, and `0x09`/`0x12` the stream's ref width.
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
    /// `0x0d` identifier inside a record payload: the terminal component of a
    /// subtype definition's name chain, e.g. `nubs` in a B-spline cache or
    /// `exactcur` in a legacy intcurve construction.
    Ident(String),
    /// `0x0e` sub-identifier inside a record payload: a non-terminal component
    /// of a subtype definition's name chain, e.g. `full` preceding `nubs`.
    SubIdent(String),
}

impl Token {
    /// Whether this token is a payload identifier ([`Token::Ident`] or
    /// [`Token::SubIdent`]).
    ///
    /// Positional field semantics in topology tables and serialized counts use
    /// value tokens. Payload identifiers name constructions; [`Record::chunk`]
    /// and [`Record::chunk_len`] skip them.
    pub fn is_payload_ident(&self) -> bool {
        matches!(self, Token::Ident(_) | Token::SubIdent(_))
    }
}

/// One framed record: its `RecordTable` index, assembled name, payload tokens
/// (the tokens after the name chain), and byte extent within the stream.
#[derive(Debug, Clone)]
pub struct Record {
    /// `RecordTable` index. `asmheader` is index 0.
    pub index: usize,
    /// Full `-`-joined record name, e.g. `cone-surface`, `body`.
    pub name: String,

    /// Payload tokens following the name chain (subtype delimiters included).
    pub tokens: Arc<[Token]>,
    /// Byte offset of the record's first name-chain tag in the stream.
    pub offset: usize,
    /// Byte length of the record including its terminator.
    pub len: usize,
}

impl Record {
    /// Leading name component used for dispatch, such as `cone` or `body`.
    #[must_use]
    pub fn head(&self) -> &str {
        self.name
            .split_once('-')
            .map_or(self.name.as_str(), |(head, _)| head)
    }

    /// Returns the `i`th payload value token. Payload identifiers leave field
    /// positions unchanged, so `chunk[i]` has the same meaning in records with
    /// and without named subtypes.
    pub fn chunk(&self, i: usize) -> Option<&Token> {
        self.chunks().nth(i)
    }

    /// The `chunk[i]` as a non-null entity reference index. Returns `None` for a
    /// null reference (`-1`) or a non-reference token.
    pub fn ref_at(&self, i: usize) -> Option<i64> {
        match self.chunk(i) {
            Some(Token::Ref(v)) if *v >= 0 => Some(*v),
            _ => None,
        }
    }

    /// The payload value tokens in order, payload identifiers skipped: the
    /// stream that `chunk[i]` indexes.
    pub fn chunks(&self) -> impl DoubleEndedIterator<Item = &Token> {
        self.tokens.iter().filter(|t| !t.is_payload_ident())
    }

    /// Number of payload value tokens: the length of the stream that
    /// `chunk[i]` indexes.
    pub fn chunk_len(&self) -> usize {
        self.chunks().count()
    }
}

/// Return the absolute byte range inside payload subtype `token_index` when
/// its immediately following identifier is `expected`.
pub fn payload_subtype_range(
    bytes: &[u8],
    record: &Record,
    token_index: usize,
    ref_width: RefWidth,
    expected: &str,
) -> Option<std::ops::Range<usize>> {
    let limit = record.offset.checked_add(record.len)?;
    let bytes = bytes.get(..limit)?;
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
                            Lexed::Terminator => return None,
                            _ => {}
                        }
                    }
                    return None;
                }
                payload_index += 1;
            }
            Lexed::Str(_) => {
                name_done = true;
                payload_index += 1;
            }
            Lexed::Terminator => return None,
            Lexed::Ident(_) | Lexed::SubIdent(_) => {}
        }
    }
    None
}

/// Return one payload token and its absolute byte offset by its framed index.
pub fn payload_token(
    bytes: &[u8],
    record: &Record,
    ref_width: RefWidth,
    token_index: usize,
) -> Option<(usize, Token)> {
    let limit = record.offset.checked_add(record.len)?;
    let bytes = bytes.get(..limit)?;
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
            Lexed::Value(token) => {
                name_done = true;
                if payload_index == token_index {
                    return Some((token_offset, token));
                }
                payload_index += 1;
            }
            Lexed::Str(value) => {
                name_done = true;
                if payload_index == token_index {
                    return Some((token_offset, Token::Str(value.to_owned())));
                }
                payload_index += 1;
            }
            Lexed::Terminator => return None,
            Lexed::Ident(_) | Lexed::SubIdent(_) => {}
        }
    }
    None
}

/// Read one token starting at `pos`. Returns the token (or a control marker) and
/// the offset just past it. Control tags (`0x0d`/`0x0e` name tokens, `0x11`
/// terminator) are returned via [`Lexed`] so the framer can act on them.
pub(crate) enum Lexed<'a> {
    /// A payload token.
    Value(Token),
    /// `0x0d` identifier (name terminator).
    Ident(&'a str),
    /// `0x0e` sub-identifier (name component).
    SubIdent(&'a str),
    /// A borrowed `0x07`/`0x08`/`0x09`/`0x12` payload string.
    Str(&'a str),
    /// `0x11` record terminator.
    Terminator,
}

pub(crate) fn lex(
    bytes: &[u8],
    pos: usize,
    ref_width: RefWidth,
) -> Result<(Lexed<'_>, usize), StreamError> {
    let err = |reason: &str| StreamError {
        format: StreamFormat::Binary,
        offset: pos,
        reason: reason.to_string(),
    };
    let tag = *bytes.get(pos).ok_or_else(|| err("end of stream"))?;
    let p = pos + 1;
    let truncated = || StreamError {
        format: StreamFormat::Binary,
        offset: pos,
        reason: format!("truncated payload for tag {tag:#04x}"),
    };
    let string = |start: usize, len: usize| {
        let end = start.checked_add(len).ok_or_else(truncated)?;
        let slice = bytes.get(start..end).ok_or_else(truncated)?;
        std::str::from_utf8(slice).map_err(|error| StreamError {
            format: StreamFormat::Binary,
            offset: start + error.valid_up_to(),
            reason: format!("payload for tag {tag:#04x} is not valid UTF-8"),
        })
    };
    let out = match tag {
        0x02 => (
            Lexed::Value(Token::Char(*bytes.get(p).ok_or_else(truncated)?)),
            p + 1,
        ),
        0x03 => (
            Lexed::Value(Token::Short(
                View::i16_le_at(bytes, p).ok_or_else(truncated)?,
            )),
            p + 2,
        ),
        0x04 => {
            let v = int_le_at(bytes, p, ref_width).ok_or_else(truncated)?;
            (Lexed::Value(Token::Long(v)), p + ref_width.bytes())
        }
        0x05 => (
            Lexed::Value(Token::Float(
                View::f32_le_at(bytes, p).ok_or_else(truncated)?,
            )),
            p + 4,
        ),
        0x06 => (
            Lexed::Value(Token::Double(
                View::f64_le_at(bytes, p).ok_or_else(truncated)?,
            )),
            p + 8,
        ),
        0x07 => {
            let len = *bytes.get(p).ok_or_else(truncated)? as usize;
            (Lexed::Str(string(p + 1, len)?), p + 1 + len)
        }
        0x08 => {
            let len = usize::from(View::u16_le_at(bytes, p).ok_or_else(truncated)?);
            (Lexed::Str(string(p + 2, len)?), p + 2 + len)
        }
        0x09 | 0x12 => {
            let len = int_le_at(bytes, p, ref_width).ok_or_else(truncated)?;
            let len = usize::try_from(len).map_err(|_| err("negative string length"))?;
            (
                Lexed::Str(string(p + ref_width.bytes(), len)?),
                p + ref_width.bytes() + len,
            )
        }
        0x0a => (Lexed::Value(Token::True), p),
        0x0b => (Lexed::Value(Token::False), p),
        0x0c => {
            let v = int_le_at(bytes, p, ref_width).ok_or_else(truncated)?;
            (Lexed::Value(Token::Ref(v)), p + ref_width.bytes())
        }
        0x0d => {
            let len = *bytes.get(p).ok_or_else(truncated)? as usize;
            (Lexed::Ident(string(p + 1, len)?), p + 1 + len)
        }
        0x0e => {
            let len = *bytes.get(p).ok_or_else(truncated)? as usize;
            (Lexed::SubIdent(string(p + 1, len)?), p + 1 + len)
        }
        0x0f => (Lexed::Value(Token::SubtypeOpen), p),
        0x10 => (Lexed::Value(Token::SubtypeClose), p),
        0x11 => (Lexed::Terminator, p),
        0x13 => (
            Lexed::Value(Token::Position(vec3_le_at(bytes, p).ok_or_else(truncated)?)),
            p + 24,
        ),
        0x14 => (
            Lexed::Value(Token::Vector3(vec3_le_at(bytes, p).ok_or_else(truncated)?)),
            p + 24,
        ),
        0x15 => {
            let v = int_le_at(bytes, p, ref_width).ok_or_else(truncated)?;
            (Lexed::Value(Token::Enum(v)), p + ref_width.bytes())
        }
        0x16 => {
            let u = View::f64_le_at(bytes, p).ok_or_else(truncated)?;
            let v = View::f64_le_at(bytes, p + 8).ok_or_else(truncated)?;
            (Lexed::Value(Token::Vector2([u, v])), p + 16)
        }
        0x17 => {
            let v = int_le_at(bytes, p, RefWidth::Eight).ok_or_else(truncated)?;
            (Lexed::Value(Token::Int64(v)), p + 8)
        }
        other => {
            return Err(StreamError {
                format: StreamFormat::Binary,
                offset: pos,
                reason: format!("unrecognized tag {other:#04x}"),
            })
        }
    };
    Ok(out)
}

/// Frame `bytes[start..limit]` into an indexed record table.
///
/// `ref_width` is the stream's reference width (8 for `BinaryFile8`). Framing
/// stops at `limit` or at the `delta_state` history boundary. A `limit` past
/// the end of `bytes` is a truncated stream, and it is refused with both the
/// declared end and the available length.
pub fn frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    limit: usize,
    ref_width: RefWidth,
) -> Result<Vec<Record>, StreamFailure> {
    frame_impl(Some(ctx), bytes, start, limit, ref_width, false)
}

/// Frame a history-section slice whose final record ends at the enclosing
/// stream boundary without an explicit `0x11` terminator.
pub fn frame_history(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    limit: usize,
    ref_width: RefWidth,
) -> Result<Vec<Record>, StreamFailure> {
    frame_impl(Some(ctx), bytes, start, limit, ref_width, true)
}

/// Frame source records for an encoder edit, outside a decode session.
pub(crate) fn frame_for_edit(
    bytes: &[u8],
    start: usize,
    limit: usize,
    ref_width: RefWidth,
) -> Result<Vec<Record>, StreamFailure> {
    frame_impl(None, bytes, start, limit, ref_width, false)
}

fn charge_items(
    ctx: Option<&DecodeContext<'_>>,
    count: u64,
    operation: &'static str,
) -> Result<(), StreamFailure> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count, operation)?;
    }
    Ok(())
}

fn charge_retained(
    ctx: Option<&DecodeContext<'_>>,
    bytes: u64,
    operation: &'static str,
) -> Result<(), StreamFailure> {
    if let Some(ctx) = ctx {
        ctx.charge_retained(bytes, operation)?;
    }
    Ok(())
}

fn grow_scratch(
    scratch: &mut Option<ScopedReservation<'_>>,
    bytes: u64,
) -> Result<(), StreamFailure> {
    if let Some(scratch) = scratch {
        scratch.grow(bytes)?;
    }
    Ok(())
}

fn refuse_size(ctx: Option<&DecodeContext<'_>>, operation: &'static str) -> StreamFailure {
    match ctx {
        Some(ctx) => StreamFailure::Resource(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX)),
        None => StreamFailure::Parse(StreamError {
            format: StreamFormat::Binary,
            offset: 0,
            reason: format!("{operation} exceeds the address space"),
        }),
    }
}

/// Admit a valid string payload before `lex` copies it. Invalid or truncated
/// payloads stay with the lexer's byte-specific parse error.
fn admit_lex_string(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    pos: usize,
    ref_width: RefWidth,
    name_done: bool,
    scratch: &mut Option<ScopedReservation<'_>>,
) -> Result<(), StreamFailure> {
    let Some(tag) = bytes.get(pos).copied() else {
        return Ok(());
    };
    let Some(prefix) = pos.checked_add(1) else {
        return Ok(());
    };
    let (start, len) = match tag {
        0x07 | 0x0d | 0x0e => (
            prefix.checked_add(1),
            bytes.get(prefix).copied().map(usize::from),
        ),
        0x08 => (
            prefix.checked_add(2),
            View::u16_le_at(bytes, prefix).map(usize::from),
        ),
        0x09 | 0x12 => (
            prefix.checked_add(ref_width.bytes()),
            int_le_at(bytes, prefix, ref_width).and_then(|value| usize::try_from(value).ok()),
        ),
        _ => return Ok(()),
    };
    let Some(payload) = start
        .zip(len)
        .and_then(|(start, len)| start.checked_add(len).and_then(|end| bytes.get(start..end)))
    else {
        return Ok(());
    };
    if std::str::from_utf8(payload).is_err() {
        return Ok(());
    }
    if (tag == 0x0d || tag == 0x0e) && !name_done {
        grow_scratch(scratch, payload.len() as u64)?;
    } else {
        charge_retained(ctx, payload.len() as u64, "retain SAB token string")?;
    }
    Ok(())
}

fn frame_impl(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    start: usize,
    limit: usize,
    ref_width: RefWidth,
    eof_terminates_final_record: bool,
) -> Result<Vec<Record>, StreamFailure> {
    let Some(bytes) = bytes.get(..limit) else {
        return Err(StreamError {
            format: StreamFormat::Binary,
            offset: start,
            reason: format!(
                "record stream declares its end at byte {limit}, but the stream holds {} bytes",
                bytes.len()
            ),
        }
        .into());
    };
    if start > limit {
        return Err(StreamError {
            format: StreamFormat::Binary,
            offset: start,
            reason: format!("record stream starts at byte {start} after its end at byte {limit}"),
        }
        .into());
    }
    let mut records = Vec::new();
    let mut pos = start;
    let mut index = 0usize;

    while pos < limit {
        let rec_start = pos;
        let mut scratch = match ctx {
            Some(ctx) => Some(ctx.reserve_scoped(0, "frame SAB record")?),
            None => None,
        };
        let mut name_parts: Vec<String> = Vec::new();
        let mut tokens: Vec<Token> = Vec::new();
        let mut depth_guards = Vec::new();
        let mut name_done = false;
        let mut is_delta = false;
        let mut embedded_history_edge = false;
        let mut payload_start = true;

        loop {
            if eof_terminates_final_record
                && pos == limit
                && depth_guards.is_empty()
                && !name_parts.is_empty()
            {
                break;
            }
            let token_offset = pos;
            admit_lex_string(ctx, bytes, pos, ref_width, name_done, &mut scratch)?;
            if let Some(ctx) = ctx {
                ctx.charge_work(1, "lex SAB token")?;
            }
            let (lexed, next) = lex(bytes, pos, ref_width)?;
            pos = next;
            match lexed {
                Lexed::Terminator if depth_guards.is_empty() => break,
                Lexed::Terminator => {
                    return Err(StreamError {
                        format: StreamFormat::Binary,
                        offset: token_offset,
                        reason: "record terminates inside a subtype scope".to_string(),
                    }
                    .into());
                }
                Lexed::SubIdent(s) if !name_done => {
                    charge_items(ctx, 1, "frame SAB name part")?;
                    grow_scratch(&mut scratch, std::mem::size_of::<String>() as u64)?;
                    name_parts.push(s.to_owned());
                }
                Lexed::Ident(s) if !name_done => {
                    charge_items(ctx, 1, "frame SAB name part")?;
                    grow_scratch(&mut scratch, std::mem::size_of::<String>() as u64)?;
                    name_parts.push(s.to_owned());
                    name_done = true;
                    // The history partition opens with the delta_state record.
                    // Stop at its name; the active slice ends before its payload.
                    if name_parts.first().is_some_and(|n| n == "delta_state") {
                        is_delta = true;
                        break;
                    }
                }
                Lexed::Ident(identifier) => {
                    // Identifier tokens after the name belong to the payload
                    // (e.g. subtype names inside a spline) and are retained as
                    // payload tokens. An archived ASM history record may wrap
                    // an edge record in the exact End-of-ASM-History-Section
                    // marker chain; its following identifier is the wrapped
                    // record's dispatch name.
                    if payload_start
                        && name_parts
                            .iter()
                            .map(String::as_str)
                            .eq(["End", "of", "ASM", "History", "Section"])
                        && identifier == "edge"
                    {
                        embedded_history_edge = true;
                    }
                    payload_start = false;
                    charge_items(ctx, 1, "frame SAB token")?;
                    grow_scratch(&mut scratch, std::mem::size_of::<Token>() as u64)?;
                    tokens.push(Token::Ident(identifier.to_owned()));
                }
                Lexed::SubIdent(identifier) => {
                    payload_start = false;
                    charge_items(ctx, 1, "frame SAB token")?;
                    grow_scratch(&mut scratch, std::mem::size_of::<Token>() as u64)?;
                    tokens.push(Token::SubIdent(identifier.to_owned()));
                }
                Lexed::Str(value) => {
                    payload_start = false;
                    name_done = true;
                    charge_items(ctx, 1, "frame SAB token")?;
                    grow_scratch(&mut scratch, std::mem::size_of::<Token>() as u64)?;
                    tokens.push(Token::Str(value.to_owned()));
                }
                Lexed::Value(Token::SubtypeOpen) => {
                    payload_start = false;
                    let guard = match ctx {
                        Some(ctx) => Some(ctx.enter_nested("frame SAB subtype")?),
                        None => None,
                    };
                    charge_items(ctx, 1, "frame SAB subtype guards")?;
                    grow_scratch(
                        &mut scratch,
                        std::mem::size_of::<Option<cadmpeg_core::decode::DepthGuard<'_>>>() as u64,
                    )?;
                    depth_guards.push(guard);
                    name_done = true;
                    charge_items(ctx, 1, "frame SAB token")?;
                    grow_scratch(&mut scratch, std::mem::size_of::<Token>() as u64)?;
                    tokens.push(Token::SubtypeOpen);
                }
                Lexed::Value(Token::SubtypeClose) => {
                    payload_start = false;
                    if depth_guards.pop().is_none() {
                        return Err(StreamError {
                            format: StreamFormat::Binary,
                            offset: token_offset,
                            reason: "record closes an unopened subtype scope".to_string(),
                        }
                        .into());
                    }
                    charge_items(ctx, 1, "frame SAB token")?;
                    grow_scratch(&mut scratch, std::mem::size_of::<Token>() as u64)?;
                    tokens.push(Token::SubtypeClose);
                }
                Lexed::Value(v) => {
                    payload_start = false;
                    name_done = true;
                    charge_items(ctx, 1, "frame SAB token")?;
                    grow_scratch(&mut scratch, std::mem::size_of::<Token>() as u64)?;
                    tokens.push(v);
                }
            }
        }

        if is_delta {
            break;
        }
        let separators = if name_parts.is_empty() {
            0
        } else {
            name_parts.len() - 1
        };
        let name_bytes = name_parts
            .iter()
            .try_fold(0usize, |used, part| used.checked_add(part.len()))
            .and_then(|length| length.checked_add(separators))
            .ok_or_else(|| refuse_size(ctx, "SAB record name"))?;
        charge_retained(
            ctx,
            if embedded_history_edge {
                4
            } else {
                name_bytes as u64
            },
            "retain SAB record name",
        )?;
        let name = if embedded_history_edge {
            "edge".to_owned()
        } else {
            name_parts.join("-")
        };

        let token_bytes = tokens
            .len()
            .checked_mul(std::mem::size_of::<Token>())
            .ok_or_else(|| refuse_size(ctx, "SAB token bytes"))?;
        charge_retained(ctx, token_bytes as u64, "retain SAB tokens")?;
        charge_items(ctx, 1, "frame SAB record")?;
        if let Some(ctx) = ctx {
            ctx.charge_entities(1, "admit SAB native record")?;
        }
        records.push(Record {
            index,
            name,

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
    use super::{
        exact_identifier_at, frame as frame_stream, frame_history as frame_history_stream,
        payload_token, Record,
    };
    use crate::kernel_header::RefWidth;
    use crate::stream_error::{StreamError, StreamFailure};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn sab_framing_refuses_token_name_record_and_scope_resources() {
        type LimitCase = (ResourceDimension, fn(&mut DecodePolicy));
        let bytes = b"\x0d\x01x\x0f\x07\x01s\x10\x11";
        let cases: [LimitCase; 6] = [
            (ResourceDimension::RetainedBytes, |policy| {
                policy.limits.max_retained_bytes = 0;
            }),
            (ResourceDimension::Entities, |policy| {
                policy.limits.max_entities = 0;
            }),
            (ResourceDimension::CollectionItems, |policy| {
                policy.limits.max_collection_items = 0;
            }),
            (ResourceDimension::MaterializedBytes, |policy| {
                policy.limits.max_materialized_bytes = 0;
            }),
            (ResourceDimension::WorkUnits, |policy| {
                policy.limits.max_work_units = 0;
            }),
            (ResourceDimension::RecursionDepth, |policy| {
                policy.limits.max_recursion_depth = 0;
            }),
        ];
        for (expected, set_limit) in cases {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            set_limit(&mut policy);
            let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
                .expect("source fits input limit");
            let error = frame_stream(&ctx, bytes, 0, bytes.len(), RefWidth::Eight)
                .expect_err("resource limit must refuse");
            let StreamFailure::Resource(CodecError::ResourceLimit(limit)) = error else {
                panic!("expected resource refusal, got {error:?}");
            };
            assert_eq!(limit.dimension, expected);
        }
        assert_eq!(
            frame(bytes, 0, bytes.len(), RefWidth::Eight).unwrap().len(),
            1
        );
    }

    fn framed(
        bytes: &[u8],
        start: usize,
        limit: usize,
        width: RefWidth,
        history: bool,
    ) -> Result<Vec<Record>, StreamError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("test stream fits the service input limit");
        let outcome = if history {
            frame_history_stream(&ctx, bytes, start, limit, width)
        } else {
            frame_stream(&ctx, bytes, start, limit, width)
        };
        match outcome {
            Ok(records) => Ok(records),
            Err(
                StreamFailure::Parse(error)
                | StreamFailure::Malformed(error)
                | StreamFailure::NotImplemented(error),
            ) => Err(error),
            Err(StreamFailure::Resource(error)) => {
                panic!("test stream exhausted a resource: {error}")
            }
        }
    }

    fn frame(
        bytes: &[u8],
        start: usize,
        limit: usize,
        width: RefWidth,
    ) -> Result<Vec<Record>, StreamError> {
        framed(bytes, start, limit, width, false)
    }

    fn frame_history(
        bytes: &[u8],
        start: usize,
        limit: usize,
        width: RefWidth,
    ) -> Result<Vec<Record>, StreamError> {
        framed(bytes, start, limit, width, true)
    }

    #[test]
    fn subtype_lookup_rejects_a_record_terminator_before_its_close() {
        for width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = b"\x0d\x01x\x0f\x0d\x01y\x0a\x10\x11".to_vec();
            let records = frame(&bytes, 0, bytes.len(), width).unwrap();
            assert_eq!(
                super::payload_subtype_range(&bytes, &records[0], 0, width, "y"),
                Some(7..8)
            );
            bytes[7] = 0x11;
            assert!(frame(&bytes, 0, bytes.len(), width).is_err());
            assert!(super::payload_subtype_range(&bytes, &records[0], 0, width, "y").is_none());
        }
    }

    #[test]
    fn framing_cannot_complete_a_token_from_bytes_after_its_limit() {
        for width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = vec![0x0d, 1, b'x', 0x06];
            bytes.extend_from_slice(&1.5f64.to_le_bytes());
            bytes.push(0x11);
            let records = frame(&bytes, 0, bytes.len(), width).expect("complete record");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].len, bytes.len());
            for limit in 1..bytes.len() {
                assert!(frame(&bytes, 0, limit, width).is_err(), "limit {limit}");
            }
            for limit in [1, 2, 4, 5, 6, 7, 8, 9, 10, 11] {
                assert!(
                    frame_history(&bytes, 0, limit, width).is_err(),
                    "history limit {limit}"
                );
            }
        }
    }

    #[test]
    fn payload_lookup_cannot_complete_a_token_outside_its_record() {
        for width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = vec![0x0d, 1, b'x', 0x06];
            bytes.extend_from_slice(&1.5f64.to_le_bytes());
            bytes.push(0x11);
            let mut record = frame(&bytes, 0, bytes.len(), width).unwrap().remove(0);
            record.len = 11;
            assert!(payload_token(&bytes, &record, width, 0).is_none());
            assert!(
                crate::test_support::sab::payload_token_offsets(&bytes, &record, width, 0x06)
                    .is_err()
            );
        }
    }

    #[test]
    fn record_readers_reject_inverted_and_overflowing_extents() {
        let bytes = b"\x0d\x01x\x11";
        assert!(frame(bytes, bytes.len() + 1, bytes.len(), RefWidth::Eight).is_err());
        let mut record = frame(bytes, 0, bytes.len(), RefWidth::Eight)
            .unwrap()
            .remove(0);
        record.offset = usize::MAX;
        record.len = 1;
        assert!(payload_token(bytes, &record, RefWidth::Eight, 0).is_none());
        assert!(crate::test_support::sab::payload_token_offsets(
            bytes,
            &record,
            RefWidth::Eight,
            0x06
        )
        .is_err());
        assert!(super::payload_subtype_range(bytes, &record, 0, RefWidth::Eight, "x").is_none());
    }

    #[test]
    fn exact_identifier_requires_the_tag_length_and_complete_payload() {
        let bytes = b"prefix\x0d\x0bdelta_state suffix";
        let at = "prefix".len();
        assert!(exact_identifier_at(bytes, at, "delta_state"));
        assert!(!exact_identifier_at(bytes, at, "delta"));
        assert!(!exact_identifier_at(&bytes[..at + 7], at, "delta_state"));
        assert!(!exact_identifier_at(
            b"prefix\x07\x0bdelta_state",
            at,
            "delta_state"
        ));
    }

    #[test]
    fn history_framer_accepts_only_the_final_record_at_eof() {
        let bytes = [0x0d, 4, b'e', b'd', b'g', b'e'];
        assert!(frame(&bytes, 0, bytes.len(), RefWidth::Eight).is_err());
        let records =
            frame_history(&bytes, 0, bytes.len(), RefWidth::Eight).expect("history EOF record");
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

        let records = frame_history(&bytes, 0, bytes.len(), RefWidth::Eight).expect("wrapped edge");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "edge");
        assert_eq!(records[0].head(), "edge");
        assert_eq!(records[0].ref_at(0), Some(4));
        assert_eq!(records[0].ref_at(5), Some(8));

        let embedded_offset = bytes
            .windows(6)
            .position(|window| window == [0x0d, 4, b'e', b'd', b'g', b'e'])
            .unwrap();
        let mut non_wrapper = bytes;
        non_wrapper.splice(embedded_offset..embedded_offset, [0x02, 0]);
        let records = frame_history(&non_wrapper, 0, non_wrapper.len(), RefWidth::Eight)
            .expect("marker with a later payload identifier");
        assert_eq!(records[0].name, "End-of-ASM-History-Section");
    }

    #[test]
    fn payload_identifiers_are_retained_and_skipped_by_chunk_indexing() {
        // spline record shape: one value, a named subtype scope wrapping one
        // double, then one trailing value.
        let mut bytes = vec![0x0d, 6];
        bytes.extend_from_slice(b"spline");
        bytes.push(0x0a); // chunk 0
        bytes.push(0x0f); // chunk 1: subtype open
        bytes.extend_from_slice(&[0x0e, 4]);
        bytes.extend_from_slice(b"full"); // payload sub-identifier
        bytes.extend_from_slice(&[0x0d, 4]);
        bytes.extend_from_slice(b"nubs"); // payload identifier
        bytes.push(0x06); // chunk 2
        bytes.extend_from_slice(&1.5f64.to_le_bytes());
        bytes.push(0x10); // chunk 3: subtype close
        bytes.push(0x0b); // chunk 4
        bytes.push(0x11);

        let records = frame(&bytes, 0, bytes.len(), RefWidth::Eight).expect("named subtype");
        assert_eq!(records.len(), 1);
        let record = &records[0];
        // The token stream is faithful: the subtype's name chain is present.
        assert_eq!(
            &*record.tokens,
            [
                super::Token::True,
                super::Token::SubtypeOpen,
                super::Token::SubIdent("full".to_string()),
                super::Token::Ident("nubs".to_string()),
                super::Token::Double(1.5),
                super::Token::SubtypeClose,
                super::Token::False,
            ]
        );
        // Chunk indexing is defined over value tokens and skips the names.
        assert_eq!(record.chunk(0), Some(&super::Token::True));
        assert_eq!(record.chunk(1), Some(&super::Token::SubtypeOpen));
        assert_eq!(record.chunk(2), Some(&super::Token::Double(1.5)));
        assert_eq!(record.chunk(3), Some(&super::Token::SubtypeClose));
        assert_eq!(record.chunk(4), Some(&super::Token::False));
        assert_eq!(record.chunk_len(), 5);
    }

    #[test]
    fn subtype_delimiters_fail_at_the_unbalanced_token() {
        for (tail, bad_token_index) in [([0x10, 0x11], 0), ([0x0f, 0x11], 1)] {
            let mut bytes = vec![0x0d, 4, b'e', b'd', b'g', b'e'];
            let tail_offset = bytes.len();
            bytes.extend_from_slice(&tail);

            let error = frame(&bytes, 0, bytes.len(), RefWidth::Eight)
                .expect_err("unbalanced subtype scope");
            assert_eq!(error.offset, tail_offset + bad_token_index);
        }
    }

    #[test]
    fn wide_string_length_prefix_uses_the_stream_ref_width() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let text = "subtransform program text";
            let mut bytes = vec![0x0d, 4];
            bytes.extend_from_slice(b"tspl");
            bytes.push(0x09);
            bytes.extend_from_slice(&(text.len() as u64).to_le_bytes()[..ref_width.bytes()]);
            bytes.extend_from_slice(text.as_bytes());
            bytes.push(0x04);
            bytes.extend_from_slice(&7i64.to_le_bytes()[..ref_width.bytes()]);
            bytes.push(0x11);

            let records =
                frame(&bytes, 0, bytes.len(), ref_width).expect("0x09 string at ref width");
            assert_eq!(records.len(), 1);
            assert_eq!(
                records[0].chunk(0),
                Some(&super::Token::Str(text.to_string()))
            );
            assert_eq!(records[0].chunk(1), Some(&super::Token::Long(7)));
        }
    }

    #[test]
    fn binary_strings_reject_invalid_utf8_at_the_source_byte() {
        let mut cases = vec![
            vec![0x0d, 1, 0xff, 0x11],
            vec![0x0e, 1, 0xff, 0x0d, 4, b'e', b'd', b'g', b'e', 0x11],
        ];
        for tag in [0x07, 0x08, 0x09, 0x12] {
            let mut bytes = vec![0x0d, 4, b'e', b'd', b'g', b'e', tag];
            match tag {
                0x07 => bytes.push(1),
                0x08 => bytes.extend_from_slice(&1_u16.to_le_bytes()),
                0x09 | 0x12 => bytes.extend_from_slice(&1_i64.to_le_bytes()),
                _ => unreachable!("string tag is exhaustive"),
            }
            bytes.extend_from_slice(&[0xff, 0x11]);
            cases.push(bytes);
        }

        for bytes in cases {
            let invalid_offset = bytes
                .iter()
                .position(|byte| *byte == 0xff)
                .expect("invalid byte offset");
            let error = frame(&bytes, 0, bytes.len(), RefWidth::Eight)
                .expect_err("invalid UTF-8 must fail");
            assert_eq!(error.offset, invalid_offset);
        }
    }

    #[test]
    fn binary_string_lengths_count_utf8_bytes() {
        for tag in [0x07, 0x08, 0x09, 0x12] {
            let mut bytes = vec![0x0d, 4, b'e', b'd', b'g', b'e', tag];
            match tag {
                0x07 => bytes.push(2),
                0x08 => bytes.extend_from_slice(&2_u16.to_le_bytes()),
                0x09 | 0x12 => bytes.extend_from_slice(&2_i64.to_le_bytes()),
                _ => unreachable!("string tag is exhaustive"),
            }
            bytes.extend_from_slice("é".as_bytes());
            bytes.push(0x11);

            let records =
                frame(&bytes, 0, bytes.len(), RefWidth::Eight).expect("multibyte UTF-8 string");
            assert_eq!(records[0].chunk(0), Some(&super::Token::Str("é".into())));
        }
    }

    fn generated_pcurve_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0d, 6];
        bytes.extend_from_slice(b"pcurve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1), (0x04, 0)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.extend_from_slice(&[0x0b, 0x0f, 0x0d, 11]);
        bytes.extend_from_slice(b"exp_par_cur");
        bytes.extend_from_slice(&[0x02, 0x7f, 0x10, 0x11]);
        bytes
    }

    fn generated_ref_pcurve_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0d, 6];
        bytes.extend_from_slice(b"pcurve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1), (0x04, 2), (0x0c, 20)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        for value in [-2.0f64, 4.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_cone_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 4];
        bytes.extend_from_slice(b"cone");
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"surface");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
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

    fn generated_sphere_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 6];
        bytes.extend_from_slice(b"sphere");
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"surface");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
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

    fn generated_torus_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 5];
        bytes.extend_from_slice(b"torus");
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"surface");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
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

    fn generated_plane_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 5];
        bytes.extend_from_slice(b"plane");
        bytes.extend_from_slice(&[0x0d, 7]);
        bytes.extend_from_slice(b"surface");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
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

    fn generated_ellipse_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 7];
        bytes.extend_from_slice(b"ellipse");
        bytes.extend_from_slice(&[0x0d, 5]);
        bytes.extend_from_slice(b"curve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
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

    fn generated_straight_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 8];
        bytes.extend_from_slice(b"straight");
        bytes.extend_from_slice(&[0x0d, 5]);
        bytes.extend_from_slice(b"curve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
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

    fn generated_degenerate_curve_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 16];
        bytes.extend_from_slice(b"degenerate_curve");
        bytes.extend_from_slice(&[0x0d, 5]);
        bytes.extend_from_slice(b"curve");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0x0b, 0x0b, 0x11]);
        bytes
    }

    fn generated_point_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0d, 5];
        bytes.extend_from_slice(b"point");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_edge_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0d, 4];
        bytes.extend_from_slice(b"edge");
        for (tag, value) in [(0x0c, -1i64), (0x04, -1), (0x0c, -1), (0x0c, 10)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&(-2.0f64).to_le_bytes());
        bytes.push(0x0c);
        bytes.extend_from_slice(&11i64.to_le_bytes()[..ref_width.bytes()]);
        bytes.push(0x06);
        bytes.extend_from_slice(&3.0f64.to_le_bytes());
        for value in [-1i64, 12] {
            bytes.push(0x0c);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.extend_from_slice(&[0x0b, 0x07, 7]);
        bytes.extend_from_slice(b"unknown");
        bytes.push(0x11);
        bytes
    }

    fn generated_tedge_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = generated_edge_record(ref_width);
        bytes.splice(1..6, [5, b't', b'e', b'd', b'g', b'e']);
        bytes.pop();
        bytes.push(0x06);
        bytes.extend_from_slice(&0.0035f64.to_le_bytes());
        for value in [22800i64, 0] {
            bytes.push(0x04);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_tcoedge_record(ref_width: RefWidth) -> Vec<u8> {
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
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.push(0x0b);
        for (tag, value) in [(0x0c, 5i64), (0x04, 0), (0x0c, 6)] {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        for value in [-2.0f64, 3.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [-1i64, 0, 0] {
            bytes.push(if value == -1 { 0x0c } else { 0x04 });
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_face_record(ref_width: RefWidth) -> Vec<u8> {
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
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.extend_from_slice(&[0x0b, 0x0a, 0x0b, 0x11]);
        bytes
    }

    fn generated_tvertex_record(ref_width: RefWidth) -> Vec<u8> {
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
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        // The fixture writes two `-1` sentinels, the evaluated tolerance, and
        // integer 0.
        for value in [-1.0f64, -1.0, 0.001] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x04);
        bytes.extend_from_slice(&0i64.to_le_bytes()[..ref_width.bytes()]);
        bytes.push(0x11);
        bytes
    }

    fn generated_body_record(ref_width: RefWidth) -> Vec<u8> {
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
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
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

    fn generated_wire_record(ref_width: RefWidth) -> Vec<u8> {
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
            bytes.extend_from_slice(&value.to_le_bytes()[..ref_width.bytes()]);
        }
        bytes.extend_from_slice(&[0x0b, 0x11]);
        bytes
    }

    fn generated_rgb_attribute_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 9];
        bytes.extend_from_slice(b"rgb_color");
        bytes.extend_from_slice(&[0x0e, 2]);
        bytes.extend_from_slice(b"st");
        bytes.extend_from_slice(&[0x0d, 6]);
        bytes.extend_from_slice(b"attrib");
        bytes.push(0x0c);
        bytes.extend_from_slice(&(-1i64).to_le_bytes()[..ref_width.bytes()]);
        for value in [0.1f64, 0.2, 0.3] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        bytes
    }

    fn generated_timestamp_attribute_record(ref_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x0e, 13];
        bytes.extend_from_slice(b"ATTRIB_CUSTOM");
        bytes.extend_from_slice(&[0x0d, 6]);
        bytes.extend_from_slice(b"attrib");
        bytes.push(0x0c);
        bytes.extend_from_slice(&(-1i64).to_le_bytes()[..ref_width.bytes()]);
        bytes.extend_from_slice(&[0x07, 20]);
        bytes.extend_from_slice(b"Timestamp_attrib_def");
        bytes.push(0x04);
        bytes.extend_from_slice(&1i64.to_le_bytes()[..ref_width.bytes()]);
        bytes.push(0x06);
        bytes.extend_from_slice(&1_579_392_000_000_007.0f64.to_le_bytes());
        bytes.push(0x11);
        bytes
    }

    #[test]
    fn generated_payload_offsets_use_declared_integer_width() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_pcurve_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated record");
            let record = records.first().expect("generated pcurve");
            assert_eq!(
                bytes[payload_token(&bytes, record, ref_width, 4)
                    .expect("required invariant")
                    .0],
                0x0b
            );
            assert_eq!(
                bytes[payload_token(&bytes, record, ref_width, 5)
                    .expect("required invariant")
                    .0],
                0x0f
            );
            assert!(payload_token(&bytes, record, ref_width, 8).is_none());
        }
    }

    #[test]
    fn generated_ref_pcurve_range_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_ref_pcurve_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated ref pcurve");
            let record = &records[0];
            for (index, expected) in [(5usize, -2.0f64), (6, 4.0)] {
                let (offset, _) =
                    payload_token(&bytes, record, ref_width, index).expect("range field offset");
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
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
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
                let (offset, _) =
                    payload_token(&bytes, record, ref_width, index).expect("cone field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_sphere_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_sphere_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated sphere");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x06), (5, 0x14), (6, 0x14)] {
                let (offset, _) =
                    payload_token(&bytes, record, ref_width, index).expect("sphere field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_torus_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_torus_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated torus");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x14), (5, 0x06), (6, 0x06), (7, 0x14)] {
                let (offset, _) =
                    payload_token(&bytes, record, ref_width, index).expect("torus field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_plane_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_plane_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated plane");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x14), (5, 0x14)] {
                let (offset, _) =
                    payload_token(&bytes, record, ref_width, index).expect("plane field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_ellipse_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_ellipse_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated ellipse");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x14), (5, 0x14), (6, 0x06)] {
                let (offset, _) =
                    payload_token(&bytes, record, ref_width, index).expect("ellipse field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_straight_geometry_has_fixed_payload_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_straight_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated straight");
            let record = &records[0];
            for (index, tag) in [(3usize, 0x13), (4, 0x14)] {
                let (offset, _) =
                    payload_token(&bytes, record, ref_width, index).expect("straight field offset");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_degenerate_curve_has_fixed_point_field_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_degenerate_curve_record(ref_width);
            let records =
                frame(&bytes, 0, bytes.len(), ref_width).expect("generated degenerate curve");
            let record = &records[0];
            let (offset, _) =
                payload_token(&bytes, record, ref_width, 3).expect("degenerate point offset");
            assert_eq!(bytes[offset], 0x13);
        }
    }

    #[test]
    fn generated_point_has_fixed_position_field_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_point_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated point");
            let record = &records[0];
            let (offset, _) =
                payload_token(&bytes, record, ref_width, 3).expect("point position offset");
            assert_eq!(bytes[offset], 0x13);
        }
    }

    #[test]
    fn generated_topology_ranges_have_fixed_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let edge = generated_edge_record(ref_width);
            let records = frame(&edge, 0, edge.len(), ref_width).expect("generated edge");
            for index in [4usize, 6] {
                let (offset, _) =
                    payload_token(&edge, &records[0], ref_width, index).expect("edge range offset");
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
                let (offset, _) = payload_token(&coedge, &records[0], ref_width, index)
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
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let face = generated_face_record(ref_width);
            let records = frame(&face, 0, face.len(), ref_width).expect("generated face");
            for index in [8usize, 9, 10] {
                let (offset, _) =
                    payload_token(&face, &records[0], ref_width, index).expect("face sense field");
                assert!(matches!(face[offset], 0x0a | 0x0b));
            }

            let coedge = generated_tcoedge_record(ref_width);
            let records =
                frame(&coedge, 0, coedge.len(), ref_width).expect("generated tolerant coedge");
            let (offset, _) =
                payload_token(&coedge, &records[0], ref_width, 7).expect("coedge sense field");
            assert_eq!(coedge[offset], 0x0b);

            let edge = generated_edge_record(ref_width);
            let records = frame(&edge, 0, edge.len(), ref_width).expect("generated edge");
            let (sense, _) =
                payload_token(&edge, &records[0], ref_width, 9).expect("edge sense field");
            let (continuity, _) =
                payload_token(&edge, &records[0], ref_width, 10).expect("edge continuity field");
            assert_eq!(edge[sense], 0x0b);
            assert_eq!(edge[continuity], 0x07);
        }
    }

    #[test]
    fn generated_tolerant_vertex_has_fixed_metadata_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
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
                let (offset, _) = payload_token(&bytes, record, ref_width, index)
                    .expect("tolerant vertex metadata field");
                assert_eq!(bytes[offset], tag);
            }
        }
    }

    #[test]
    fn generated_ownership_keys_have_fixed_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let body = generated_body_record(ref_width);
            let records = frame(&body, 0, body.len(), ref_width).expect("generated body");
            let (key, _) = payload_token(&body, &records[0], ref_width, 1).expect("body key field");
            assert_eq!(body[key], 0x04);

            let edge = generated_edge_record(ref_width);
            let records = frame(&edge, 0, edge.len(), ref_width).expect("generated edge");
            let (owner, _) =
                payload_token(&edge, &records[0], ref_width, 7).expect("edge owner field");
            assert_eq!(edge[owner], 0x0c);
        }
    }

    #[test]
    fn generated_transform_has_fixed_matrix_and_hint_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_transform_record();
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated transform");
            let record = &records[0];
            for (index, tag) in [(0usize, 0x14), (1, 0x14), (2, 0x14), (3, 0x14), (4, 0x06)] {
                let (offset, _) = payload_token(&bytes, record, ref_width, index)
                    .expect("transform numeric field");
                assert_eq!(bytes[offset], tag);
            }
            for index in 5..=7 {
                let (offset, _) =
                    payload_token(&bytes, record, ref_width, index).expect("transform hint field");
                assert!(matches!(bytes[offset], 0x0a | 0x0b));
            }
        }
    }

    #[test]
    fn generated_wire_has_fixed_side_field_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = generated_wire_record(ref_width);
            let records = frame(&bytes, 0, bytes.len(), ref_width).expect("generated wire");
            let (offset, _) =
                payload_token(&bytes, &records[0], ref_width, 7).expect("wire side field");
            assert_eq!(bytes[offset], 0x0b);
        }
    }

    #[test]
    fn generated_attribute_values_have_semantic_fields_at_both_widths() {
        for ref_width in [RefWidth::Four, RefWidth::Eight] {
            let color = generated_rgb_attribute_record(ref_width);
            let records =
                frame(&color, 0, color.len(), ref_width).expect("generated RGB attribute");
            for index in 1..=3 {
                let (offset, _) = payload_token(&color, &records[0], ref_width, index)
                    .expect("RGB channel field");
                assert_eq!(color[offset], 0x06);
            }

            let timestamp = generated_timestamp_attribute_record(ref_width);
            let records = frame(&timestamp, 0, timestamp.len(), ref_width)
                .expect("generated timestamp attribute");
            for (index, tag) in [(1usize, 0x07), (2, 0x04), (3, 0x06)] {
                let (offset, _) = payload_token(&timestamp, &records[0], ref_width, index)
                    .expect("timestamp semantic field");
                assert_eq!(timestamp[offset], tag);
            }
        }
    }
}
