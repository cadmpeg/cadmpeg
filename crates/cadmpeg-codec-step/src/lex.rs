// SPDX-License-Identifier: Apache-2.0
//! Byte-oriented ISO 10303-21 lexical analysis.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

/// The entity or value occurrence class.
#[derive(Debug, Clone, Copy)]
enum OccurrencePrefix {
    Entity,
    Value,
}

/// A lexical token with its exact source-byte extent.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    /// Parsed token category.
    pub(crate) kind: TokenKind,
    /// Half-open byte range in the exchange structure.
    pub(crate) span: Range<usize>,
}

/// Part 21 token categories.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenKind {
    /// Standard keyword or entity name.
    Name(String),
    /// User-defined `!`-prefixed keyword.
    UserName(String),
    /// Numeric `#`-prefixed entity-instance name.
    Instance(u64),
    /// Numeric `@`-prefixed value-instance name.
    ValueInstance(u64),
    /// `#`-prefixed EXPRESS entity constant name.
    ConstantEntity(String),
    /// `@`-prefixed EXPRESS value constant name.
    ConstantValue(String),
    /// Signed decimal integer.
    Integer(i64),
    /// Decimal real, including an optional exponent.
    Real(FiniteReal),
    /// Dot-delimited enumeration or logical literal.
    Enumeration(String),
    /// Bytes between apostrophe delimiters, before escape decoding.
    String(Vec<u8>),
    /// Decoded quoted hexadecimal binary literal.
    Binary(BinaryValue),
    /// Edition-3 resource token.
    Resource(String),
    /// Opening parenthesis.
    LParen,
    /// Closing parenthesis.
    RParen,
    /// Parameter separator.
    Comma,
    /// Statement terminator.
    Semicolon,
    /// Assignment operator.
    Equals,
    /// Omitted-value marker `$`.
    Omitted,
    /// Derived-value marker `*`.
    Derived,
    /// Anchor-tag name, preserving source case.
    TagName(String),
    /// Opening anchor-tag delimiter.
    LBrace,
    /// Closing anchor-tag delimiter.
    RBrace,
    /// Anchor-tag name/value separator.
    Colon,
}

/// Binary literal payload packed most-significant nibble first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BinaryValue {
    /// Unused low-order bits in the final byte, including nibble padding.
    unused_bits: u8,
    data: Box<[u8]>,
}

impl BinaryValue {
    pub(crate) fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let data = ctx.copy_slice(&self.data, operation)?;
        Ok(Self {
            unused_bits: self.unused_bits,
            data: ctx.into_boxed_slice(data, operation)?,
        })
    }

    pub(crate) fn bit_len(&self) -> usize {
        self.data.len() * 8 - usize::from(self.unused_bits)
    }

    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Lexical failure with a stable byte position.
#[derive(Debug)]
pub(crate) struct LexError {
    /// Byte offset at which tokenization failed.
    offset: usize,
    /// Violated lexical invariant.
    pub(crate) message: String,
    resource: Option<CodecError>,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.resource {
            Some(error) => write!(formatter, "{error} at byte {}", self.offset),
            None => write!(formatter, "{} at byte {}", self.message, self.offset),
        }
    }
}

impl std::error::Error for LexError {}

impl From<CodecError> for LexError {
    fn from(error: CodecError) -> Self {
        Self {
            offset: 0,
            message: String::new(),
            resource: Some(error),
        }
    }
}

impl LexError {
    pub(crate) fn into_codec_error(self, ctx: &DecodeContext<'_>) -> CodecError {
        match self.resource {
            Some(error) => error,
            None => ctx
                .format_retained(
                    format_args!("{} at byte {}", self.message, self.offset),
                    "STEP lexical error",
                )
                .map_or_else(std::convert::identity, CodecError::Malformed),
        }
    }

    pub(crate) fn into_resource_error(self) -> Option<CodecError> {
        self.resource
    }
}

/// Tokenize one complete clear-text exchange structure.
#[cfg(test)]
pub(crate) fn lex_with_context(
    input: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(input, ctx);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    Ok(tokens)
}

#[derive(Clone, Copy)]
enum LiteralStorage {
    Retained,
    Transient,
}

pub(crate) struct Lexer<'a, 'ctx, 'arena> {
    input: &'a [u8],
    budget: &'ctx DecodeContext<'arena>,
    literal_storage: LiteralStorage,
    transient_storage: Option<ScopedReservation<'ctx>>,
    at: usize,
    allow_print_controls: bool,
    previous_was_signature: bool,
    tag_name_expected: bool,
}

const MAX_STORED_STRING_OCTETS: usize = 32_769;

pub(crate) fn print_control_end(
    ctx: &DecodeContext<'_>,
    input: &[u8],
    at: usize,
) -> Result<Option<usize>, CodecError> {
    if let Some(end) = match_exact_ignoring_controls(ctx, input, at, b"\\N\\")? {
        return Ok(Some(end));
    }
    match_exact_ignoring_controls(ctx, input, at, b"\\F\\")
}

impl<'a, 'ctx, 'arena> Lexer<'a, 'ctx, 'arena> {
    pub(crate) fn new(input: &'a [u8], ctx: &'ctx DecodeContext<'arena>) -> Self {
        Self {
            input,
            budget: ctx,
            literal_storage: LiteralStorage::Retained,
            transient_storage: None,
            at: 0,
            allow_print_controls: true,
            previous_was_signature: false,
            tag_name_expected: false,
        }
    }

    pub(crate) fn set_transient_literals(&mut self) {
        self.literal_storage = LiteralStorage::Transient;
    }

    pub(crate) fn set_allow_print_controls(&mut self, allow: bool) {
        self.allow_print_controls = allow;
    }

    pub(crate) fn input(&self) -> &[u8] {
        self.input
    }

    pub(crate) fn next_token(&mut self) -> Result<Option<Token>, LexError> {
        self.transient_storage = None;
        if matches!(self.literal_storage, LiteralStorage::Transient) {
            let mut storage = self
                .budget
                .reserve_scoped(0, "STEP transient token storage")?;
            let token = storage.with_storage(|| self.next_token_inner());
            self.transient_storage = Some(storage);
            token
        } else {
            self.next_token_inner()
        }
    }

    fn next_token_inner(&mut self) -> Result<Option<Token>, LexError> {
        if !self.skip_trivia()? {
            return Ok(None);
        }
        let token = self.token()?;
        let skip_signature =
            self.previous_was_signature && matches!(&token.kind, TokenKind::Semicolon);
        self.previous_was_signature =
            matches!(&token.kind, TokenKind::Name(name) if name == "SIGNATURE");
        if skip_signature {
            self.skip_signature_payload()?;
        }
        Ok(Some(token))
    }

    fn skip_signature_payload(&mut self) -> Result<(), LexError> {
        let start = self.at;
        let name_len = b"ENDSEC".len();
        let mut at = start;
        let mut boundary_allowed = true;
        while at + name_len <= self.input.len() {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            if self.input.get(at..at + 2) == Some(b"/*") {
                let comment_start = at;
                at += 2;
                let Some(end) = self.budget.position_by(
                    self.input[at..].windows(2),
                    |w| Ok(w == b"*/"),
                    "STEP lexer comment traversal",
                )?
                else {
                    return Err(self.error(comment_start, "unterminated comment")?);
                };
                at += end + 2;
                boundary_allowed = true;
                continue;
            }
            if self.input[at].is_ascii_control() {
                while self.input.get(at).is_some_and(u8::is_ascii_control) {
                    self.budget.charge_work(1, "STEP lexer cursor traversal")?;
                    at += 1;
                }
                boundary_allowed = true;
                continue;
            }
            if let Some(end) = self.print_control_end(at)? {
                at = end;
                boundary_allowed = true;
                continue;
            }
            if self.input.get(at) == Some(&b' ') {
                at += 1;
                boundary_allowed = true;
                continue;
            }
            let candidate = at;
            let Some(mut after_name) = self.match_ignoring_controls(candidate, b"ENDSEC")? else {
                at += 1;
                boundary_allowed = false;
                continue;
            };
            while self
                .input
                .get(after_name)
                .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
            {
                self.budget.charge_work(1, "STEP lexer cursor traversal")?;
                after_name += 1;
            }
            if boundary_allowed && self.input.get(after_name) == Some(&b';') {
                self.validate_signature_payload(start, candidate)?;
                self.at = candidate;
                return Ok(());
            }
            at += 1;
            boundary_allowed = false;
        }
        Err(self.error(start, "unterminated signature section")?)
    }

    fn validate_signature_payload(&self, start: usize, end: usize) -> Result<(), LexError> {
        let mut quantum_len = 0;
        let mut padding = 0;
        let mut finished = false;
        let mut saw_content = false;
        let mut trailing_separator = false;
        let mut relative = 0;
        while relative < end - start {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            let byte = self.input[start + relative];
            if byte.is_ascii_control() {
                relative += 1;
                continue;
            }
            if byte == b' ' {
                trailing_separator |= saw_content;
                relative += 1;
                continue;
            }
            if let Some(separator_end) = self.print_control_end(start + relative)? {
                if separator_end > end {
                    return Err(self.error(start + relative, "invalid SIGNATURE base64 character")?);
                }
                trailing_separator |= saw_content;
                relative = separator_end - start;
                continue;
            }
            if self.input.get(start + relative..start + relative + 2) == Some(b"/*") {
                let comment_start = start + relative;
                let comment_body = comment_start + 2;
                let Some(comment_end) = self.budget.position_by(
                    self.input[comment_body..end].windows(2),
                    |window| Ok(window == b"*/"),
                    "STEP lexer comment traversal",
                )?
                else {
                    return Err(self.error(comment_start, "unterminated comment")?);
                };
                trailing_separator |= saw_content;
                relative = comment_body + comment_end + 2 - start;
                continue;
            }
            if trailing_separator {
                return Err(self.error(start + relative, "invalid SIGNATURE base64 character")?);
            }
            saw_content = true;
            let is_alphabet = byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/');
            if finished || (padding != 0 && is_alphabet) {
                return Err(self.error(start + relative, "invalid SIGNATURE base64 padding")?);
            }
            if is_alphabet {
                quantum_len += 1;
            } else if byte == b'=' {
                if quantum_len < 2 || padding == 2 {
                    return Err(self.error(start + relative, "invalid SIGNATURE base64 padding")?);
                }
                padding += 1;
                quantum_len += 1;
            } else {
                return Err(self.error(start + relative, "invalid SIGNATURE base64 character")?);
            }
            if quantum_len == 4 {
                finished = padding != 0;
                quantum_len = 0;
                padding = 0;
            }
            relative += 1;
        }
        if !saw_content {
            return Err(self.error(start, "SIGNATURE section has empty base64 content")?);
        }
        if quantum_len != 0 {
            return Err(self.error(end, "SIGNATURE base64 content has incomplete quantum")?);
        }
        Ok(())
    }

    fn skip_trivia(&mut self) -> Result<bool, LexError> {
        self.budget.charge_work(0, "STEP lexer cursor traversal")?;
        while self.at < self.input.len() {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            while self.input.get(self.at).is_some_and(u8::is_ascii_control) {
                self.budget.charge_work(1, "STEP lexer cursor traversal")?;
                self.at += 1;
            }
            if let Some(end) = self.print_control_end(self.at)? {
                if !self.allow_print_controls {
                    return Err(self.error(
                        self.at,
                        "print control directive is not allowed in this section",
                    )?);
                }
                self.at = end;
                continue;
            }
            if self.input.get(self.at) == Some(&b' ') {
                self.at += 1;
                continue;
            }
            if self.input.get(self.at..self.at + 2) != Some(b"/*") {
                return Ok(self.at < self.input.len());
            }
            let start = self.at;
            self.at += 2;
            let Some(end) = self.budget.position_by(
                self.input[self.at..].windows(2),
                |w| Ok(w == b"*/"),
                "STEP lexer comment traversal",
            )?
            else {
                return Err(self.error(start, "unterminated comment")?);
            };
            self.at += end + 2;
        }
        Ok(false)
    }

    fn token(&mut self) -> Result<Token, LexError> {
        self.budget.charge_work(1, "step_lex_token")?;
        let start = self.at;
        if self.tag_name_expected {
            self.tag_name_expected = false;
            let kind = self.tag_name()?;
            return Ok(Token {
                kind,
                span: start..self.at,
            });
        }
        let byte = self.input[self.at];
        let kind = match byte {
            b'(' => self.one(TokenKind::LParen),
            b')' => self.one(TokenKind::RParen),
            b',' => self.one(TokenKind::Comma),
            b';' => self.one(TokenKind::Semicolon),
            b'=' => self.one(TokenKind::Equals),
            b'{' => self.one(TokenKind::LBrace),
            b'}' => self.one(TokenKind::RBrace),
            b':' => self.one(TokenKind::Colon),
            b'$' => self.one(TokenKind::Omitted),
            b'*' => self.one(TokenKind::Derived),
            b'#' => self.occurrence(OccurrencePrefix::Entity)?,
            b'@' => self.occurrence(OccurrencePrefix::Value)?,
            b'\'' => self.string()?,
            b'"' => self.binary()?,
            b'<' => self.resource()?,
            b'.' if self
                .next_non_ignored(self.at + 1)?
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_') =>
            {
                self.enumeration()?
            }
            b'!' => self.user_name()?,
            b'+' | b'-' | b'0'..=b'9' | b'.' => self.number()?,
            b if b.is_ascii_alphabetic() || b == b'_' => TokenKind::Name(self.name()?),
            _ => return Err(self.error(start, "unexpected byte")?),
        };
        Ok(Token {
            kind,
            span: start..self.at,
        })
    }

    fn one(&mut self, kind: TokenKind) -> TokenKind {
        self.at += 1;
        if matches!(kind, TokenKind::LBrace) {
            self.tag_name_expected = true;
        }
        kind
    }

    fn name(&mut self) -> Result<String, LexError> {
        let start = self.at;
        self.at += 1;
        while self.input.get(self.at).is_some_and(|b| {
            b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-' || b.is_ascii_control()
        }) {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            self.at += 1;
        }
        let (mut name, _) = self.normalized(start, self.at, LiteralStorage::Retained)?;
        self.budget
            .make_ascii_uppercase(&mut name, "STEP lexer name uppercase")?;
        Ok(name)
    }

    fn tag_name(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        if !self
            .input
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return Err(self.error(start, "tag name has no identifier")?);
        }
        self.at += 1;
        while self.input.get(self.at).is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || *byte == b'_' || byte.is_ascii_control()
        }) {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            self.at += 1;
        }
        let (name, _) = self.normalized(start, self.at, LiteralStorage::Retained)?;
        Ok(TokenKind::TagName(name))
    }

    fn user_name(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        self.skip_ignored()?;
        if !self
            .input
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return Err(self.error(start, "user-defined name has no identifier")?);
        }
        Ok(TokenKind::UserName(self.name()?))
    }

    fn occurrence(&mut self, prefix: OccurrencePrefix) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        self.skip_ignored()?;
        match self.input.get(self.at).copied() {
            Some(byte) if byte.is_ascii_digit() => {
                let digits = self.at;
                while let Some(byte) = self.input.get(self.at).copied() {
                    self.budget.charge_work(1, "STEP lexer cursor traversal")?;
                    if byte.is_ascii_digit() || byte.is_ascii_control() {
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                let (_temporary, raw) = self
                    .normalized(digits, self.at, LiteralStorage::Transient)
                    .map(|(raw, reservation)| (reservation, raw))?;
                let value = self
                    .budget
                    .parse_text::<u64>(raw.as_str(), "STEP occurrence number parse")?
                    .ok()
                    .map_or_else(
                        || Err(self.error(start, "instance name is out of range")?),
                        Ok,
                    )?;
                if value == 0 {
                    return Err(self.error(start, "instance name must not be zero")?);
                }
                match prefix {
                    OccurrencePrefix::Entity => Ok(TokenKind::Instance(value)),
                    OccurrencePrefix::Value => Ok(TokenKind::ValueInstance(value)),
                }
            }
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => {
                let name_start = self.at;
                self.at += 1;
                while let Some(byte) = self.input.get(self.at).copied() {
                    self.budget.charge_work(1, "STEP lexer cursor traversal")?;
                    if byte.is_ascii_alphanumeric() || byte == b'_' || byte.is_ascii_control() {
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                let (mut name, _) =
                    self.normalized(name_start, self.at, LiteralStorage::Retained)?;
                self.budget
                    .make_ascii_uppercase(&mut name, "STEP lexer name uppercase")?;
                match prefix {
                    OccurrencePrefix::Entity => Ok(TokenKind::ConstantEntity(name)),
                    OccurrencePrefix::Value => Ok(TokenKind::ConstantValue(name)),
                }
            }
            _ => Err(self.error(start, "occurrence name has no identifier")?),
        }
    }

    fn number(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        if matches!(self.input[self.at], b'+' | b'-') {
            self.at += 1;
            self.skip_ignored()?;
        }
        let mut dot = false;
        let mut exponent = false;
        while let Some(&b) = self.input.get(self.at) {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            match b {
                byte if byte.is_ascii_control() => self.at += 1,
                b'0'..=b'9' => self.at += 1,
                b'.' if !dot => {
                    dot = true;
                    self.at += 1;
                }
                b'E' | b'e' | b'D' | b'd' if !exponent => {
                    exponent = true;
                    self.at += 1;
                    self.skip_ignored()?;
                    if self
                        .input
                        .get(self.at)
                        .is_some_and(|b| matches!(b, b'+' | b'-'))
                    {
                        self.at += 1;
                        self.skip_ignored()?;
                    }
                }
                _ => break,
            }
        }
        let (_temporary, mut raw) = self
            .normalized_chars(start, self.at, LiteralStorage::Transient, |byte| {
                char::from(match byte {
                    b'D' | b'd' => b'E',
                    byte => byte.to_ascii_uppercase(),
                })
            })
            .map(|(raw, reservation)| (reservation, raw))?;
        if exponent && raw.ends_with('.') {
            raw.pop();
        }
        if dot || exponent {
            let parsed = self
                .budget
                .parse_text::<f64>(raw.as_str(), "STEP real number parse")?
                .or_else(|_| Err(self.error(start, "invalid real")?))?;
            FiniteReal::new(parsed).map(TokenKind::Real).map_or_else(
                || Err(self.error(start, "real exceeds finite binary64 range")?),
                Ok,
            )
        } else {
            self.budget
                .parse_text::<i64>(raw.as_str(), "STEP integer number parse")?
                .map(TokenKind::Integer)
                .or_else(|_| Err(self.error(start, "invalid integer")?))
        }
    }

    fn enumeration(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        self.skip_ignored()?;
        let name_start = self.at;
        while self
            .input
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || b.is_ascii_control())
        {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            self.at += 1;
        }
        self.skip_ignored()?;
        if self.input.get(self.at) != Some(&b'.') {
            return Err(self.error(start, "unterminated enumeration")?);
        }
        let (mut name, _) = self.normalized(name_start, self.at, LiteralStorage::Retained)?;
        self.budget
            .make_ascii_uppercase(&mut name, "STEP lexer name uppercase")?;
        self.at += 1;
        Ok(TokenKind::Enumeration(name))
    }

    fn string(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let mut bytes = Vec::new();
        while self.at < self.input.len() {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            if self.at - start + 1 > MAX_STORED_STRING_OCTETS {
                return Err(self.error(start, "string exceeds maximum stored length")?);
            }
            match self.input[self.at] {
                b'\'' => {
                    if let Some(end) = self.match_exact_ignoring_controls(self.at, b"''")? {
                        self.extend_string_bytes(&mut bytes, b"''", start)?;
                        self.at = end;
                    } else {
                        self.at += 1;
                        return Ok(TokenKind::String(bytes));
                    }
                }
                byte if byte.is_ascii_control() => {
                    self.at += 1;
                }
                b'\\' => {
                    if let Some(end) = self.print_control_end(self.at)? {
                        if !self.allow_print_controls {
                            return Err(self.error(
                                self.at,
                                "print control directive is not allowed in this section",
                            )?);
                        }
                        let directive = if self
                            .match_exact_ignoring_controls(self.at, b"\\N\\")?
                            .is_some()
                        {
                            b"\\N\\"
                        } else {
                            b"\\F\\"
                        };
                        self.extend_string_bytes(&mut bytes, directive, start)?;
                        self.at = end;
                    } else {
                        self.extend_string_bytes(&mut bytes, b"\\", start)?;
                        self.at += 1;
                    }
                }
                byte => {
                    self.extend_string_bytes(&mut bytes, &[byte], start)?;
                    self.at += 1;
                }
            }
        }
        if self.at - start + 1 > MAX_STORED_STRING_OCTETS {
            return Err(self.error(start, "string exceeds maximum stored length")?);
        }
        Err(self.error(start, "unterminated string")?)
    }

    fn binary(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let content = self.at;
        let mut digit_count = 0usize;
        let mut indicator = None;
        let mut last_digit = None;
        while let Some(byte) = self.input.get(self.at).copied() {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            if let Some(digit) = HexDigit::new(byte) {
                indicator.get_or_insert(digit.nibble());
                last_digit = Some(digit.nibble());
                digit_count += 1;
                self.at += 1;
            } else if byte.is_ascii_control() {
                self.at += 1;
            } else if byte == b'\\' {
                let Some(after_print_control) = self.print_control_end(self.at)? else {
                    break;
                };
                if !self.allow_print_controls {
                    return Err(self.error(
                        self.at,
                        "print control directive is not allowed in this section",
                    )?);
                }
                self.at = after_print_control;
            } else {
                break;
            }
        }
        if self.input.get(self.at) != Some(&b'"') {
            return Err(self.error(start, "invalid binary literal")?);
        }
        let Some(unused_bits) = indicator else {
            return Err(self.error(start, "binary literal has no unused-bit indicator")?);
        };
        if unused_bits > 3 {
            return Err(self.error(start, "binary unused-bit indicator exceeds three")?);
        }
        let payload_digits = digit_count - 1;
        if payload_digits == 0 && unused_bits != 0 {
            return Err(self.error(start, "empty binary payload has unused bits")?);
        }
        if unused_bits != 0 && last_digit.is_some_and(|digit| digit & ((1 << unused_bits) - 1) != 0)
        {
            return Err(self.error(start, "unused binary bits are not zero")?);
        }
        let operation = match self.literal_storage {
            LiteralStorage::Retained => "step_binary_lexeme_retained",
            LiteralStorage::Transient => "step_binary_packed_temp",
        };
        let packed_len = payload_digits.div_ceil(2);
        let mut data = self.budget.vector_storage(packed_len, operation)?;
        self.budget
            .charge_collection_items(u64_from_index(packed_len), "step_binary_packed_bytes")?;
        let mut cursor = content;
        let mut skip_indicator = true;
        let mut high = None;
        while cursor < self.at {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            let byte = self.input[cursor];
            if let Some(digit) = HexDigit::new(byte) {
                if skip_indicator {
                    skip_indicator = false;
                } else if let Some(high) = high.take() {
                    data.push((high << 4) | digit.nibble());
                } else {
                    high = Some(digit.nibble());
                }
                cursor += 1;
            } else if byte.is_ascii_control() {
                cursor += 1;
            } else if byte == b'\\' {
                let Some(end) = self.print_control_end(cursor)? else {
                    return Err(self.error(cursor, "invalid binary literal")?);
                };
                cursor = end;
            } else {
                return Err(self.error(cursor, "invalid binary literal")?);
            }
        }
        if let Some(high) = high {
            data.push(high << 4);
        }
        let unused_bits = unused_bits + if payload_digits % 2 == 1 { 4 } else { 0 };
        self.at += 1;
        Ok(TokenKind::Binary(BinaryValue {
            unused_bits,
            data: self
                .budget
                .into_boxed_slice(data, "STEP binary boxed storage")?,
        }))
    }

    fn resource(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let content = self.at;
        let mut value_len = 0usize;
        while let Some(byte) = self.input.get(self.at).copied() {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            if byte == b'>' {
                break;
            }
            if byte.is_ascii_control() {
                let controls_start = self.at;
                self.skip_ignored()?;
                if self.print_control_end(self.at)?.is_some() {
                    return Err(self.error(
                        controls_start,
                        "print control directive is not allowed in a resource",
                    )?);
                }
                continue;
            }
            if self.print_control_end(self.at)?.is_some() {
                return Err(self.error(
                    self.at,
                    "print control directive is not allowed in a resource",
                )?);
            }
            value_len += 1;
            self.at += 1;
        }
        if self.input.get(self.at) != Some(&b'>') {
            return Err(self.error(start, "unterminated resource token")?);
        }
        let mut value = self
            .budget
            .alloc_filled(value_len, 0_u8, "step_uri_lexeme_bytes")?;
        let mut written = 0usize;
        for &byte in self
            .budget
            .admit_iter(
                &(self.input[content..self.at])[..],
                "STEP resource traversal",
            )
            .map_err(cadmpeg_core::CodecError::from)?
        {
            if !byte.is_ascii_control() {
                value[written] = byte;
                written += 1;
            }
        }
        let value = String::from_utf8(value)
            .or_else(|_| Err(self.error(content, "resource token is not UTF-8")?))?;
        self.at += 1;
        Ok(TokenKind::Resource(value))
    }

    fn skip_ignored(&mut self) -> Result<(), LexError> {
        while self.input.get(self.at).is_some_and(u8::is_ascii_control) {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            self.at += 1;
        }
        Ok(())
    }

    fn next_non_ignored(&self, mut at: usize) -> Result<Option<u8>, LexError> {
        while self.input.get(at).is_some_and(u8::is_ascii_control) {
            self.budget.charge_work(1, "STEP lexer cursor traversal")?;
            at += 1;
        }
        Ok(self.input.get(at).copied())
    }

    fn normalized(
        &self,
        start: usize,
        end: usize,
        storage: LiteralStorage,
    ) -> Result<(String, Option<ScopedReservation<'_>>), LexError> {
        self.normalized_chars(start, end, storage, char::from)
    }

    fn normalized_chars(
        &self,
        start: usize,
        end: usize,
        storage: LiteralStorage,
        convert: impl Fn(u8) -> char,
    ) -> Result<(String, Option<ScopedReservation<'_>>), LexError> {
        let source = &self.input[start..end];
        let length = self.budget.fold(
            source,
            0_usize,
            |length, byte| {
                let bytes = if byte.is_ascii_control() {
                    0
                } else {
                    convert(*byte).len_utf8()
                };
                length.checked_add(bytes).ok_or_else(|| {
                    self.budget.refuse_codec_limit(
                        "STEP normalized text length",
                        u64::MAX,
                        u64::MAX,
                    )
                })
            },
            "STEP normalized measurement",
        )?;
        let collect = |operation| -> Result<String, CodecError> {
            let mut text = self.budget.retained_string(length, operation)?;
            let mut bytes = source.iter();
            for _ in 0..bytes.len() {
                let byte = self
                    .budget
                    .next_charged(&mut bytes, "STEP normalized traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP normalized source ended early"))?;
                if !byte.is_ascii_control() {
                    self.budget
                        .push_retained_char(&mut text, convert(*byte), operation)?;
                }
            }
            Ok(text)
        };
        match storage {
            LiteralStorage::Retained => Ok((collect("step_lex_normalized_retained")?, None)),
            LiteralStorage::Transient => {
                let (text, storage) = self
                    .budget
                    .with_scoped_storage("step_lex_normalized_temp", || {
                        collect("step_lex_normalized_temp")
                    })?;
                Ok((text, Some(storage)))
            }
        }
    }

    fn extend_string_bytes(
        &self,
        output: &mut Vec<u8>,
        bytes: &[u8],
        start: usize,
    ) -> Result<(), LexError> {
        self.budget
            .extend_retained_bytes(output, bytes, "step_string_lexeme_items")
            .map_err(|error| Self::resource_error(start, error))
    }

    fn print_control_end(&self, at: usize) -> Result<Option<usize>, CodecError> {
        print_control_end(self.budget, self.input, at)
    }

    fn match_exact_ignoring_controls(
        &self,
        at: usize,
        expected: &[u8],
    ) -> Result<Option<usize>, CodecError> {
        match_exact_ignoring_controls(self.budget, self.input, at, expected)
    }

    fn match_ignoring_controls(
        &self,
        mut at: usize,
        expected: &[u8],
    ) -> Result<Option<usize>, CodecError> {
        for &byte in expected {
            while self.input.get(at).is_some_and(u8::is_ascii_control) {
                self.budget.charge_work(1, "STEP lexer cursor traversal")?;
                at += 1;
            }
            if !self
                .input
                .get(at)
                .is_some_and(|value| value.eq_ignore_ascii_case(&byte))
            {
                return Ok(None);
            }
            at += 1;
        }
        Ok(Some(at))
    }

    fn error(&self, offset: usize, message: &str) -> Result<LexError, CodecError> {
        Ok(LexError {
            offset,
            message: self
                .budget
                .copy_retained_text(message, "STEP lexer error message")?,
            resource: None,
        })
    }

    fn resource_error(offset: usize, error: CodecError) -> LexError {
        LexError {
            offset,
            message: String::new(),
            resource: Some(error),
        }
    }
}

/// An ASCII hexadecimal digit, carrying the four bits it names.
///
/// The type makes the nibble map total: a value of this type exists only
/// because the byte it came from names a hexadecimal digit, so the map from a
/// scanned binary literal to its nibbles has no unreachable arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HexDigit(u8);

impl HexDigit {
    /// The digit an ASCII byte names, or `None` when the byte names none.
    const fn new(byte: u8) -> Option<Self> {
        Some(Self(match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        }))
    }

    /// The four bits the digit names, 0 through 15.
    const fn nibble(self) -> u8 {
        self.0
    }
}

fn match_exact_ignoring_controls(
    ctx: &DecodeContext<'_>,
    input: &[u8],
    mut at: usize,
    expected: &[u8],
) -> Result<Option<usize>, CodecError> {
    for &byte in expected {
        while input.get(at).is_some_and(u8::is_ascii_control) {
            ctx.charge_work(1, "STEP lexer control lookahead")?;
            at += 1;
        }
        if input.get(at) != Some(&byte) {
            return Ok(None);
        }
        at += 1;
    }
    Ok(Some(at))
}

#[cfg(test)]
mod tests;
