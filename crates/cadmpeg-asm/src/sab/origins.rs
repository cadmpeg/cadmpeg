// SPDX-License-Identifier: Apache-2.0
//! Optional SAB lexical origins from the real binary lexer.
//!
//! This is framing metadata, not semantic read attribution. Its views preserve
//! the actual caller session, address space and source window. No record scope,
//! capture or byte credit is created. The existing owned AST framer is not
//! called: new storage is temporary and reserved through the caller context.

use super::{lex_with_origin, Lexed, Token};
use crate::kernel_header::RefWidth;
use crate::stream_error::{StreamError, StreamFailure, StreamFormat};
use cadmpeg_core::decode::{DecodeContext, View};

/// The grammatical role at the time the real lexer consumed this token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRole {
    /// A component of the source record's name chain.
    RecordName,
    /// A value in the existing `Record.tokens` sequence.
    Value,
    /// A construction identifier in the existing payload token sequence.
    PayloadIdentifier,
    /// A subtype open or close in that payload token sequence.
    SubtypeDelimiter,
    /// The top-level record terminator, outside the AST payload sequence.
    RecordTerminator,
}

/// Exact source windows for one consumed token; fields cannot be forged.
#[derive(Debug, Clone, Copy)]
pub struct TokenOrigin<'a> {
    role: TokenRole,
    extent: View<'a>,
    payload: Option<View<'a>>,
}

impl<'a> TokenOrigin<'a> {
    /// The token's actual grammatical role.
    pub const fn role(&self) -> TokenRole {
        self.role
    }

    /// Type tag, length prefix and value bytes exactly as consumed.
    /// Access itself attributes no semantic read.
    pub fn extent(&self, ctx: &DecodeContext<'_>) -> Result<View<'a>, StreamFailure> {
        ctx.require_source(self.extent)
            .map_err(StreamFailure::from_operation)?;
        Ok(self.extent)
    }

    /// Value bytes without a type tag or string-length prefix. A Boolean's
    /// value is its tag, so its payload is that one byte. Delimiters have none;
    /// an empty string has a present empty payload. This grants no read credit.
    pub fn payload(&self, ctx: &DecodeContext<'_>) -> Result<Option<View<'a>>, StreamFailure> {
        ctx.require_source(self.extent)
            .map_err(StreamFailure::from_operation)?;
        Ok(self.payload)
    }
}

/// One source-record table slot, with no neutral identity or semantic ownership.
#[derive(Debug)]
pub struct RecordOrigins<'a> {
    index: usize,
    extent: View<'a>,
    tokens: Vec<TokenOrigin<'a>>,
}

impl<'a> RecordOrigins<'a> {
    /// Genuine table slot in the framed input, starting at zero like `Record`.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Exact framed source extent; this is not a semantic read claim.
    pub fn extent(&self, ctx: &DecodeContext<'_>) -> Result<View<'a>, StreamFailure> {
        ctx.require_source(self.extent)
            .map_err(StreamFailure::from_operation)?;
        Ok(self.extent)
    }

    /// All lexical tokens, including record names and delimiters.
    pub fn tokens(&self) -> &[TokenOrigin<'a>] {
        &self.tokens
    }

    /// Origins in precisely the existing AST payload-token order. This skips
    /// record names and the record terminator, not payload identifiers/scopes.
    pub fn payload_tokens(&self) -> impl Iterator<Item = &TokenOrigin<'a>> {
        self.tokens.iter().filter(|token| {
            !matches!(
                token.role,
                TokenRole::RecordName | TokenRole::RecordTerminator
            )
        })
    }
}

/// Frame a bounded live source view using BinaryFile4/8 or ACISBinary token laws.
/// Stops before `delta_state`, like `sab::frame`. It builds only optional lexical
/// metadata, not an owned AST, admitted entities or semantic record scopes.
pub fn frame<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    ref_width: RefWidth,
) -> Result<Vec<RecordOrigins<'a>>, StreamFailure> {
    frame_impl(ctx, source, ref_width, false)
}

/// The same history convention as `sab::frame_history`: the final record may
/// end at the enclosing source boundary without a record terminator.
pub fn frame_history<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    ref_width: RefWidth,
) -> Result<Vec<RecordOrigins<'a>>, StreamFailure> {
    frame_impl(ctx, source, ref_width, true)
}

fn failure(ctx: &DecodeContext<'_>, offset: usize, reason: &str) -> StreamFailure {
    let mut detail = String::new();
    if let Err(error) = ctx.reserve_text_working(&mut detail, reason.len(), "SAB origin diagnostic")
    {
        return StreamFailure::from_operation(error);
    }
    detail.push_str(reason);
    StreamError {
        format: StreamFormat::Binary,
        offset,
        reason: detail,
    }
    .into()
}

fn frame_impl<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    ref_width: RefWidth,
    history: bool,
) -> Result<Vec<RecordOrigins<'a>>, StreamFailure> {
    ctx.require_source(source)
        .map_err(StreamFailure::from_operation)?;
    let mut cursor = source;
    cursor
        .seek(source.start())
        .ok_or_else(|| failure(ctx, source.start(), "source start is outside its window"))?;
    let mut records = Vec::new();
    while !cursor.is_empty() {
        let start = cursor.position();
        let mut tokens = Vec::new();
        let mut guards = Vec::new();
        let mut name_done = false;
        let mut first_name = true;
        let mut delta_name = false;
        let mut has_name = false;
        loop {
            if history && cursor.is_empty() && guards.is_empty() && has_name {
                break;
            }
            let offset = cursor.position();
            let (lexed, next, payload) =
                lex_with_origin(cursor.unread(), 0, ref_width).map_err(|mut error| {
                    let mut at = cursor;
                    match at.skip(error.offset) {
                        Some(()) => {
                            error.offset = at.position();
                            StreamFailure::from(error)
                        }
                        None => {
                            failure(ctx, offset, "lexer error points outside its source window")
                        }
                    }
                })?;
            let role = match &lexed {
                Lexed::Terminator if guards.is_empty() => TokenRole::RecordTerminator,
                Lexed::Terminator => {
                    return Err(failure(
                        ctx,
                        offset,
                        "record terminates inside a subtype scope",
                    ))
                }
                Lexed::SubIdent(name) | Lexed::Ident(name) if !name_done => {
                    if first_name {
                        delta_name = *name == "delta_state";
                        first_name = false;
                    }
                    has_name = true;
                    if matches!(lexed, Lexed::Ident(_)) {
                        name_done = true;
                        if delta_name {
                            return Ok(records);
                        }
                    }
                    TokenRole::RecordName
                }
                Lexed::Ident(_) | Lexed::SubIdent(_) => TokenRole::PayloadIdentifier,
                Lexed::Value(Token::SubtypeOpen) => {
                    ctx.reserve_working(&mut guards, 1, "SAB origin subtype stack")
                        .map_err(StreamFailure::from_operation)?;
                    guards.push(
                        ctx.enter_nested("frame SAB origin subtype")
                            .map_err(StreamFailure::from_operation)?,
                    );
                    TokenRole::SubtypeDelimiter
                }
                Lexed::Value(Token::SubtypeClose) => {
                    if guards.pop().is_none() {
                        return Err(failure(
                            ctx,
                            offset,
                            "record closes an unopened subtype scope",
                        ));
                    }
                    TokenRole::SubtypeDelimiter
                }
                Lexed::Value(_) | Lexed::Str(_) => TokenRole::Value,
            };
            if !matches!(role, TokenRole::RecordName) {
                name_done = true;
            }
            let mut end_cursor = cursor;
            end_cursor
                .skip(next)
                .ok_or_else(|| failure(ctx, offset, "lexed token exceeds its source window"))?;
            let extent = cursor
                .child(offset, end_cursor.position())
                .ok_or_else(|| failure(ctx, offset, "lexed token is outside its source window"))?;
            let payload = match payload {
                Some(range) => {
                    let mut value = cursor;
                    value.skip(range.start).ok_or_else(|| {
                        failure(ctx, offset, "lexed payload starts outside its token")
                    })?;
                    let begin = value.position();
                    let length = range
                        .end
                        .checked_sub(range.start)
                        .ok_or_else(|| failure(ctx, offset, "lexed payload bounds are inverted"))?;
                    value.skip(length).ok_or_else(|| {
                        failure(ctx, offset, "lexed payload ends outside its token")
                    })?;
                    Some(cursor.child(begin, value.position()).ok_or_else(|| {
                        failure(ctx, offset, "lexed payload is outside its source")
                    })?)
                }
                None => None,
            };
            ctx.reserve_working(&mut tokens, 1, "SAB token origins")
                .map_err(StreamFailure::from_operation)?;
            tokens.push(TokenOrigin {
                role,
                extent,
                payload,
            });
            cursor = end_cursor;
            if role == TokenRole::RecordTerminator {
                break;
            }
        }
        let extent = source
            .child(start, cursor.position())
            .ok_or_else(|| failure(ctx, start, "framed record is outside its source window"))?;
        ctx.reserve_working(&mut records, 1, "SAB record origins")
            .map_err(StreamFailure::from_operation)?;
        records.push(RecordOrigins {
            index: records.len(),
            extent,
            tokens,
        });
    }
    Ok(records)
}

#[cfg(test)]
mod tests;
