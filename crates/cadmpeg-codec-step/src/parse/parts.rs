// SPDX-License-Identifier: Apache-2.0
//! Shared failure homes around the actual bounded DATA-record parser.

use super::{
    BTreeSet, DecodeContext, Exchange, Lexer, ParseDiagnostic, ParseError, Parser, RawRecord,
    TokenKind, View,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};

pub(super) enum RecordAttempt {
    Parsed(u64, RawRecord),
    GivenUp { end: usize },
}

/// Parse source-addressed bare STEP, retaining shared homes separately from the
/// transient RawRecord graph. Only independently terminated simple entities
/// adopt the callback here; complex and cut-short statements retain their
/// existing parser path until their respective framing callbacks are adopted.
pub(crate) fn parse(
    input: View<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(Exchange, Vec<ParseDiagnostic>, ParserPartOutput), CodecError> {
    let mut parts = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Step);
    let mut parser = Parser {
        current: None,
        lexer: Lexer::new(input.window(), ctx),
        last_end: 0,
        diagnostics: Vec::new(),
        omitted_entity_names: None,
        defer_lexical_errors: false,
        budget: ctx,
        source: Some(input),
        parts: Some(&mut parts),
        shared_omitted: BTreeSet::new(),
    };
    parser.current = parser.lex_next().map_err(ParseError::into_codec_error)?;
    let (exchange, diagnostics) = parser.exchange().map_err(ParseError::into_codec_error)?;
    Ok((exchange, diagnostics, parts))
}

impl<'input, 'ctx, 'arena> Parser<'input, 'ctx, 'arena> {
    pub(super) fn record_attempt(&mut self) -> Result<RecordAttempt, ParseError> {
        let Some((extent, spec, end)) = self.simple_part() else {
            return self
                .record()
                .map(|(id, record)| RecordAttempt::Parsed(id, record));
        };
        let Some(output) = self.parts.take() else {
            return self
                .record()
                .map(|(id, record)| RecordAttempt::Parsed(id, record));
        };
        let ctx = self.budget;
        let Some(input) = self.lexer.limit_input(end) else {
            self.parts = Some(output);
            return Self::err_at(end, "framed STEP statement exceeds its source input");
        };
        // This callback is the existing record parser itself. Its successful
        // RawRecord is returned to the unchanged exchange/semantic readers.
        let result = ctx.decode_parser_part(output, extent, spec, || {
            self.record().map_err(ParseError::into_codec_error)
        });
        self.lexer.restore_input(input);
        self.parts = Some(output);
        match result.map_err(ParseError::Resource)? {
            Some((id, record)) => {
                self.lexer.seek(end);
                self.current = self.lex_next()?;
                Ok(RecordAttempt::Parsed(id, record))
            }
            None => Ok(RecordAttempt::GivenUp { end }),
        }
    }

    fn simple_part(&self) -> Option<(View<'input>, PartSpec, usize)> {
        if self.parts.is_none() {
            return None;
        }
        let source = self.source?;
        let token = self.current.as_ref()?;
        match &token.kind {
            TokenKind::Instance(_) => {}
            TokenKind::ValueInstance(_) if self.lexer.is_draft() => {}
            _ => return None,
        }
        let start = token.span.start;
        let input = self.lexer.input();
        let draft = self.lexer.is_draft();
        let end = super::recovery::statement_end(input, start, draft).ok()?;
        if input.get(end.checked_sub(1)?) != Some(&b';') {
            return None;
        }
        let equals = trivia(input, token.span.end, end, draft)?;
        if input.get(equals) != Some(&b'=') {
            return None;
        }
        let body = trivia(input, equals.checked_add(1)?, end, draft)?;
        if input.get(body) == Some(&b'(') {
            return None;
        }
        let absolute = |offset: usize| source.start().checked_add(offset);
        let extent = source.child(absolute(start)?, absolute(end)?)?;
        let range = |from, to| {
            Some(
                cadmpeg_core::decode::u64_from_index(absolute(from)?)
                    ..cadmpeg_core::decode::u64_from_index(absolute(to)?),
            )
        };
        let framing = vec![
            range(start, token.span.end)?,
            range(equals, equals.checked_add(1)?)?,
            range(end.checked_sub(1)?, end)?,
        ];
        let level = PartLevel::parse("step.simple_entity_instance")?;
        Some((extent, PartSpec::new(level, framing), end))
    }
}

fn trivia(input: &[u8], mut at: usize, end: usize, draft: bool) -> Option<usize> {
    while at < end {
        if input
            .get(at)
            .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
        {
            at = at.checked_add(1)?;
            continue;
        }
        let pair = input.get(at..at.checked_add(2)?)?;
        let closing: &[u8] = if pair == b"/*" {
            b"*/"
        } else if draft && pair == b"!*" {
            b"*!"
        } else {
            return Some(at);
        };
        let content = at.checked_add(2)?;
        let length = input
            .get(content..end)?
            .windows(2)
            .position(|pair| pair == closing)?;
        at = content.checked_add(length)?.checked_add(2)?;
    }
    Some(at)
}

#[cfg(test)]
mod tests;
