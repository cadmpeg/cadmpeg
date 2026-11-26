// SPDX-License-Identifier: Apache-2.0
//! Frame ACIS binary streams that intern identifier text (`asm.md` §2.4).
//!
//! Every `0x0D`/`0x0E` token carries its text, `%`, and a little-endian `u32`
//! name id. Non-empty text defines the id; empty text repeats the text of an
//! id defined earlier. Records carry the extended entity prefix of §7.6, which
//! this framer reduces to the shared prefix so that record fields keep the
//! positions of §§5–6.

use std::sync::Arc;

use cadmpeg_core::decode::{DecodeContext, View};

use crate::kernel_header::RefWidth;
use crate::sab::{lex, Lexed, Record, Token};
use crate::stream_error::{StreamError, StreamFailure, StreamFormat};

/// Identifier that ends the record stream. It is stored without a name id.
pub const END_OF_DATA: &str = "End-of-ACIS-data";

/// Byte length of the `%` marker and the `u32` name id.
const NAME_ID_LEN: usize = 5;

/// A framed interned stream.
#[derive(Debug, Default)]
pub struct InternedStream {
    /// Records in `RecordTable` order. Index 0 is the first record.
    pub records: Vec<Record>,
    /// The tokens between the header doubles and the first record.
    pub header_extension: Vec<Token>,
    /// Records whose extended prefix was reduced to the shared prefix.
    pub reduced_records: usize,
    /// Offset of the first byte that the framed records leave unread: a
    /// `delta_state` history partition, or bytes after `End-of-ACIS-data`.
    pub unread: Option<usize>,
}

/// One identifier token: its text, resolved through the name table.
struct Name {
    text: String,
    sub: bool,
}

/// Whether the stream that starts at `start` interns identifier text: the
/// first identifier token after the header extension defines name id 1.
#[must_use]
pub fn applies(bytes: &[u8], start: usize) -> bool {
    let Some(first) = header_extension_end(bytes, start) else {
        return false;
    };
    let Some(&tag) = bytes.get(first) else {
        return false;
    };
    if !matches!(tag, 0x0d | 0x0e) {
        return false;
    }
    let Some(&len) = bytes.get(first + 1) else {
        return false;
    };
    let Some(payload) = bytes.get(first + 2..first + 2 + usize::from(len)) else {
        return false;
    };
    let Some(split) = payload.len().checked_sub(NAME_ID_LEN) else {
        return false;
    };
    split > 0 && payload[split] == b'%' && View::u32_le_at(payload, split + 1) == Some(1)
}

/// Offset of the first record: past an optional `TRUE` and string, or past a
/// `FALSE`.
fn header_extension_end(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start)? {
        0x0a => {
            let (lexed, next) = lex(bytes, start + 1, RefWidth::Four).ok()?;
            matches!(lexed, Lexed::Str(_)).then_some(next)
        }
        0x0b => Some(start + 1),
        _ => Some(start),
    }
}

/// Frame `bytes[start..limit]`, where `start` follows the header doubles.
pub fn frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    limit: usize,
    declared_entities: Option<u64>,
) -> Result<InternedStream, StreamFailure> {
    let error = |offset: usize, reason: String| -> StreamFailure {
        StreamError {
            format: StreamFormat::Binary,
            offset,
            reason,
        }
        .into()
    };
    let Some(bytes) = bytes.get(..limit) else {
        return Err(error(
            start,
            format!(
                "record stream declares its end at byte {limit}, but the stream holds {} bytes",
                bytes.len()
            ),
        ));
    };
    let mut stream = InternedStream::default();
    let first = header_extension_end(bytes, start)
        .ok_or_else(|| error(start, "header extension does not lex".to_owned()))?;
    let mut pos = start;
    while pos < first {
        let (lexed, next) = lex(bytes, pos, RefWidth::Four)?;
        if let Lexed::Value(token) = lexed {
            stream.header_extension.push(token);
        } else if let Lexed::Str(text) = lexed {
            stream.header_extension.push(Token::Str(text.into_owned()));
        }
        pos = next;
    }
    let mut admitted = 0;
    if let Some(count) = declared_entities {
        ctx.admit_entities(count, &mut admitted, "admit ACIS header entities")
            .map_err(StreamFailure::from_operation)?;
    }
    let mut names: Vec<String> = Vec::new();
    while pos < limit {
        let record_start = pos;
        let mut name_parts = Vec::new();
        let mut tokens = Vec::new();
        let mut depth = Vec::new();
        let mut name_done = false;
        loop {
            let token_offset = pos;
            if let Some(name) = identifier(bytes, &mut pos, &mut names, &error)? {
                if !name_done && name_parts.is_empty() && name.text == END_OF_DATA {
                    stream.unread = (pos < limit).then_some(pos);
                    return Ok(stream);
                }
                if name_done {
                    tokens.push(if name.sub {
                        Token::SubIdent(name.text)
                    } else {
                        Token::Ident(name.text)
                    });
                } else {
                    name_done = !name.sub;
                    name_parts.push(name.text);
                }
                continue;
            }
            let (lexed, next) = lex(bytes, pos, RefWidth::Four)?;
            pos = next;
            name_done = true;
            match lexed {
                Lexed::Terminator if depth.is_empty() => break,
                Lexed::Terminator => {
                    return Err(error(
                        token_offset,
                        "record terminates inside a subtype scope".to_owned(),
                    ))
                }
                Lexed::Value(Token::SubtypeOpen) => {
                    depth.push(
                        ctx.enter_nested("frame interned SAB subtype")
                            .map_err(StreamFailure::from_operation)?,
                    );
                    tokens.push(Token::SubtypeOpen);
                }
                Lexed::Value(Token::SubtypeClose) => {
                    if depth.pop().is_none() {
                        return Err(error(
                            token_offset,
                            "record closes an unopened subtype scope".to_owned(),
                        ));
                    }
                    tokens.push(Token::SubtypeClose);
                }
                Lexed::Value(token) => tokens.push(token),
                Lexed::Str(text) => tokens.push(Token::Str(text.into_owned())),
                Lexed::Ident(_) | Lexed::SubIdent(_) => {}
            }
        }
        let name = name_parts.join("-");
        if name_parts.first().is_some_and(|part| part == "delta_state") {
            stream.unread = Some(record_start);
            return Ok(stream);
        }
        if reduce_prefix(&name, &mut tokens) {
            stream.reduced_records += 1;
        }
        let index = stream.records.len();
        let population = cadmpeg_core::decode::u64_from_index(index + 1);
        if population > admitted {
            ctx.admit_entities(population, &mut admitted, "admit interned SAB record")
                .map_err(StreamFailure::from_operation)?;
        }
        stream.records.push(Record {
            index,
            name,
            tokens: Arc::from(tokens),
            offset: record_start,
            len: pos - record_start,
        });
    }
    Err(error(
        pos,
        format!("record stream ends without `{END_OF_DATA}`"),
    ))
}

/// Reads one `0x0D`/`0x0E` token at `pos`, resolving its name id.
fn identifier(
    bytes: &[u8],
    pos: &mut usize,
    names: &mut Vec<String>,
    error: &impl Fn(usize, String) -> StreamFailure,
) -> Result<Option<Name>, StreamFailure> {
    let at = *pos;
    let sub = match bytes.get(at) {
        Some(0x0d) => false,
        Some(0x0e) => true,
        _ => return Ok(None),
    };
    let len = usize::from(
        *bytes
            .get(at + 1)
            .ok_or_else(|| error(at, "truncated identifier".to_owned()))?,
    );
    let payload = bytes
        .get(at + 2..at + 2 + len)
        .ok_or_else(|| error(at, "truncated identifier".to_owned()))?;
    *pos = at + 2 + len;
    if payload == END_OF_DATA.as_bytes() {
        return Ok(Some(Name {
            text: END_OF_DATA.to_owned(),
            sub,
        }));
    }
    let split = payload
        .len()
        .checked_sub(NAME_ID_LEN)
        .filter(|split| payload[*split] == b'%')
        .ok_or_else(|| error(at, "identifier has no name id".to_owned()))?;
    let id = View::u32_le_at(payload, split + 1)
        .and_then(|id| usize::try_from(id).ok())
        .ok_or_else(|| error(at, "name id does not fit the address space".to_owned()))?;
    let text = std::str::from_utf8(&payload[..split])
        .map_err(|_| error(at, "identifier text is not valid UTF-8".to_owned()))?;
    if text.is_empty() {
        let text = id
            .checked_sub(1)
            .and_then(|slot| names.get(slot))
            .ok_or_else(|| error(at, format!("name id {id} is not defined")))?;
        return Ok(Some(Name {
            text: text.clone(),
            sub,
        }));
    }
    if id != names.len() + 1 {
        return Err(error(
            at,
            format!("name id {id} does not follow id {}", names.len()),
        ));
    }
    names.push(text.to_owned());
    Ok(Some(Name {
        text: text.to_owned(),
        sub,
    }))
}

/// Removes the extension integer of the extended entity prefix (`asm.md`
/// §7.6), and the body's integer before its first lump. Returns whether the
/// record carried the extended prefix.
fn reduce_prefix(name: &str, tokens: &mut Vec<Token>) -> bool {
    let extended = matches!(
        (tokens.first(), tokens.get(1), tokens.get(2), tokens.get(3)),
        (
            Some(Token::Ref(_)),
            Some(Token::Long(_)),
            Some(Token::Long(_)),
            Some(Token::Ref(_))
        )
    );
    if !extended {
        return false;
    }
    tokens.remove(2);
    if name == "body" && matches!(tokens.get(3), Some(Token::Long(_))) {
        tokens.remove(3);
    }
    true
}

#[cfg(test)]
mod tests;
