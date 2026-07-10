// SPDX-License-Identifier: Apache-2.0
//! SAB (ACIS binary) token stream framing.
//!
//! The active model slice of an ASM `.smbh`/`.smb` stream is a tag-typed token
//! stream: every value is introduced by a one-byte tag whose payload width is
//! either fixed or carried by a length prefix (see the tag table below). Records
//! are delimited by the `0x11` terminator at subtype-nesting depth 0; subtype
//! scopes are brace-balanced by `0x0f`/`0x10`. A record's name is the `-`-joined
//! chain of `0x0e` sub-identifiers terminated by one `0x0d` identifier.
//!
//! This module turns those bytes into a [`Vec<Record>`], the `RecordTable` that
//! topology and geometry decoding index into. Because every token's width is
//! known, the framer stays byte-synchronized even across records whose interior
//! payload this codec does not interpret (splines, attributes) — it can still
//! find their boundaries and preserve them.

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
    pub tokens: Vec<Token>,
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

fn read_i(bytes: &[u8], at: usize, width: usize) -> Option<i64> {
    let slice = bytes.get(at..at + width)?;
    let v = match width {
        8 => i64::from_le_bytes(
            slice
                .try_into()
                .expect("invariant: bytes.get(at..at+width) with width=8 is an 8-byte slice"),
        ),
        4 => i32::from_le_bytes(
            slice
                .try_into()
                .expect("invariant: bytes.get(at..at+width) with width=4 is a 4-byte slice"),
        ) as i64,
        _ => return None,
    };
    Some(v)
}

fn read_f64(bytes: &[u8], at: usize) -> Option<f64> {
    let slice = bytes.get(at..at + 8)?;
    Some(f64::from_le_bytes(
        slice
            .try_into()
            .expect("invariant: bytes.get(at..at+8) is an 8-byte slice"),
    ))
}

fn read_vec3(bytes: &[u8], at: usize) -> Option<[f64; 3]> {
    Some([
        read_f64(bytes, at)?,
        read_f64(bytes, at + 8)?,
        read_f64(bytes, at + 16)?,
    ])
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

/// Frame the active slice `bytes[start..limit]` into the `RecordTable`.
///
/// `ref_width` is the stream's reference width (8 for `BinaryFile8`). Framing
/// stops at `limit`, at end of stream, or when it reaches the `delta_state`
/// history boundary record — the active model slice is everything before it.
pub fn frame(
    bytes: &[u8],
    start: usize,
    limit: usize,
    ref_width: usize,
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

        loop {
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
                Lexed::SubIdent(_) | Lexed::Ident(_) => {
                    // Identifier tokens after the name belong to the payload
                    // (e.g. subtype names inside a spline). Ignore for framing;
                    // they carry no value this codec reads positionally.
                }
                Lexed::Value(Token::SubtypeOpen) => {
                    depth += 1;
                    name_done = true;
                    tokens.push(Token::SubtypeOpen);
                }
                Lexed::Value(Token::SubtypeClose) => {
                    depth -= 1;
                    tokens.push(Token::SubtypeClose);
                }
                Lexed::Value(v) => {
                    name_done = true;
                    tokens.push(v);
                }
            }
        }

        if is_delta {
            break;
        }
        let name = name_parts.join("-");
        let head = name_parts.first().cloned().unwrap_or_default();

        records.push(Record {
            index,
            name,
            head,
            tokens,
            offset: rec_start,
            len: pos - rec_start,
        });
        index += 1;
    }

    Ok(records)
}
