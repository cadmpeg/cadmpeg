// SPDX-License-Identifier: Apache-2.0
//! Byte-oriented ISO 10303-21 lexical analysis.

use std::ops::Range;

/// A lexical token with its exact source-byte extent.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Parsed token category.
    pub kind: TokenKind,
    /// Half-open byte range in the exchange structure.
    pub span: Range<usize>,
}

/// Part 21 token categories.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TokenKind {
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
    Real(f64),
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
}

/// Binary literal payload packed most-significant nibble first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryValue {
    /// Number of significant payload bits.
    pub bit_len: usize,
    /// Packed bytes; unused low-order bits in the final byte are zero.
    pub data: Vec<u8>,
}

/// Lexical failure with a stable byte position.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} at byte {offset}")]
pub struct LexError {
    /// Byte offset at which tokenization failed.
    pub offset: usize,
    /// Violated lexical invariant.
    pub message: String,
}

/// Tokenize one complete clear-text exchange structure.
pub fn lex(input: &[u8]) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    Ok(tokens)
}

pub(crate) struct Lexer<'a> {
    input: &'a [u8],
    at: usize,
    previous_was_signature: bool,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            at: 0,
            previous_was_signature: false,
        }
    }

    pub(crate) fn next_token(&mut self) -> Result<Option<Token>, LexError> {
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
        while at + name_len <= self.input.len() {
            if self.input.get(at..at + 2) == Some(b"/*") {
                let comment_start = at;
                at += 2;
                let Some(end) = self.input[at..].windows(2).position(|w| w == b"*/") else {
                    return Err(Self::error(comment_start, "unterminated comment"));
                };
                at += end + 2;
                continue;
            }
            let candidate = at;
            let matches_name =
                self.input[candidate..candidate + name_len].eq_ignore_ascii_case(b"ENDSEC");
            let preceded_by_separator = candidate == start
                || self.input[..candidate]
                    .last()
                    .is_some_and(u8::is_ascii_whitespace)
                || self.input[..candidate].ends_with(b"*/");
            let mut after_name = candidate + name_len;
            while self
                .input
                .get(after_name)
                .is_some_and(u8::is_ascii_whitespace)
            {
                after_name += 1;
            }
            if matches_name && preceded_by_separator && self.input.get(after_name) == Some(&b';') {
                self.validate_signature_payload(start, candidate)?;
                self.at = candidate;
                return Ok(());
            }
            at += 1;
        }
        Err(Self::error(start, "unterminated signature section"))
    }

    fn validate_signature_payload(&self, start: usize, end: usize) -> Result<(), LexError> {
        let mut quantum_len = 0;
        let mut padding = 0;
        let mut finished = false;
        let mut saw_content = false;
        for (relative, &byte) in self.input[start..end].iter().enumerate() {
            if byte.is_ascii_whitespace() {
                continue;
            }
            saw_content = true;
            let is_alphabet = byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/');
            if finished || (padding != 0 && is_alphabet) {
                return Err(Self::error(
                    start + relative,
                    "invalid SIGNATURE base64 padding",
                ));
            }
            if is_alphabet {
                quantum_len += 1;
            } else if byte == b'=' {
                if quantum_len < 2 || padding == 2 {
                    return Err(Self::error(
                        start + relative,
                        "invalid SIGNATURE base64 padding",
                    ));
                }
                padding += 1;
                quantum_len += 1;
            } else {
                return Err(Self::error(
                    start + relative,
                    "invalid SIGNATURE base64 character",
                ));
            }
            if quantum_len == 4 {
                finished = padding != 0;
                quantum_len = 0;
                padding = 0;
            }
        }
        if !saw_content {
            return Err(Self::error(
                start,
                "SIGNATURE section has empty base64 content",
            ));
        }
        if quantum_len != 0 {
            return Err(Self::error(
                end,
                "SIGNATURE base64 content has incomplete quantum",
            ));
        }
        Ok(())
    }

    fn skip_trivia(&mut self) -> Result<bool, LexError> {
        loop {
            while self.input.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
            if self.input.get(self.at..self.at + 2) != Some(b"/*") {
                return Ok(self.at < self.input.len());
            }
            let start = self.at;
            self.at += 2;
            let Some(end) = self.input[self.at..].windows(2).position(|w| w == b"*/") else {
                return Err(Self::error(start, "unterminated comment"));
            };
            self.at += end + 2;
        }
    }

    fn token(&mut self) -> Result<Token, LexError> {
        let start = self.at;
        let byte = self.input[self.at];
        let kind = match byte {
            b'(' => self.one(TokenKind::LParen),
            b')' => self.one(TokenKind::RParen),
            b',' => self.one(TokenKind::Comma),
            b';' => self.one(TokenKind::Semicolon),
            b'=' => self.one(TokenKind::Equals),
            b'$' => self.one(TokenKind::Omitted),
            b'*' => self.one(TokenKind::Derived),
            b'#' => self.occurrence(b'#')?,
            b'@' => self.occurrence(b'@')?,
            b'\'' => self.string()?,
            b'"' => self.binary()?,
            b'<' => self.resource()?,
            b'.' if self
                .input
                .get(self.at + 1)
                .is_some_and(u8::is_ascii_alphabetic) =>
            {
                self.enumeration()?
            }
            b'!' => self.user_name()?,
            b'+' | b'-' | b'0'..=b'9' | b'.' => self.number()?,
            b if b.is_ascii_alphabetic() || b == b'_' => self.name(),
            _ => return Err(Self::error(start, "unexpected byte")),
        };
        Ok(Token {
            kind,
            span: start..self.at,
        })
    }

    fn one(&mut self, kind: TokenKind) -> TokenKind {
        self.at += 1;
        kind
    }

    fn name(&mut self) -> TokenKind {
        let start = self.at;
        self.at += 1;
        while self
            .input
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        {
            self.at += 1;
        }
        TokenKind::Name(String::from_utf8_lossy(&self.input[start..self.at]).to_ascii_uppercase())
    }

    fn user_name(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        if !self
            .input
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return Err(Self::error(start, "user-defined name has no identifier"));
        }
        let TokenKind::Name(name) = self.name() else {
            unreachable!()
        };
        Ok(TokenKind::UserName(name))
    }

    fn occurrence(&mut self, prefix: u8) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        match self.input.get(self.at).copied() {
            Some(byte) if byte.is_ascii_digit() => {
                let digits = self.at;
                while self.input.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                let value = std::str::from_utf8(&self.input[digits..self.at])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| Self::error(start, "instance name is out of range"))?;
                if value == 0 {
                    return Err(Self::error(start, "instance name must not be zero"));
                }
                match prefix {
                    b'#' => Ok(TokenKind::Instance(value)),
                    b'@' => Ok(TokenKind::ValueInstance(value)),
                    _ => unreachable!("occurrence prefixes are fixed by the lexer"),
                }
            }
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => {
                let name_start = self.at;
                self.at += 1;
                while self
                    .input
                    .get(self.at)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    self.at += 1;
                }
                let name =
                    String::from_utf8_lossy(&self.input[name_start..self.at]).to_ascii_uppercase();
                match prefix {
                    b'#' => Ok(TokenKind::ConstantEntity(name)),
                    b'@' => Ok(TokenKind::ConstantValue(name)),
                    _ => unreachable!("occurrence prefixes are fixed by the lexer"),
                }
            }
            _ => Err(Self::error(start, "occurrence name has no identifier")),
        }
    }

    fn number(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        if matches!(self.input[self.at], b'+' | b'-') {
            self.at += 1;
        }
        let mut dot = false;
        let mut exponent = false;
        while let Some(&b) = self.input.get(self.at) {
            match b {
                b'0'..=b'9' => self.at += 1,
                b'.' if !dot && !exponent => {
                    dot = true;
                    self.at += 1;
                }
                b'E' | b'e' | b'D' | b'd' if !exponent => {
                    exponent = true;
                    self.at += 1;
                    if self
                        .input
                        .get(self.at)
                        .is_some_and(|b| matches!(b, b'+' | b'-'))
                    {
                        self.at += 1;
                    }
                }
                _ => break,
            }
        }
        let raw = std::str::from_utf8(&self.input[start..self.at]).unwrap_or_default();
        if dot || exponent {
            let parsed = if raw
                .as_bytes()
                .iter()
                .any(|byte| matches!(byte, b'D' | b'd'))
            {
                raw.replace(['D', 'd'], "E").parse()
            } else {
                raw.parse()
            };
            parsed
                .map(TokenKind::Real)
                .map_err(|_| Self::error(start, "invalid real"))
        } else {
            raw.parse()
                .map(TokenKind::Integer)
                .map_err(|_| Self::error(start, "invalid integer"))
        }
    }

    fn enumeration(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let name_start = self.at;
        while self
            .input
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(*b, b'_' | b'-'))
        {
            self.at += 1;
        }
        if self.input.get(self.at) != Some(&b'.') {
            return Err(Self::error(start, "unterminated enumeration"));
        }
        let name = String::from_utf8_lossy(&self.input[name_start..self.at]).to_ascii_uppercase();
        self.at += 1;
        Ok(TokenKind::Enumeration(name))
    }

    fn string(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let mut bytes = Vec::new();
        loop {
            match self.input.get(self.at).copied() {
                Some(b'\'') if self.input.get(self.at + 1) == Some(&b'\'') => {
                    bytes.extend_from_slice(b"''");
                    self.at += 2;
                }
                Some(b'\'') => {
                    self.at += 1;
                    return Ok(TokenKind::String(bytes));
                }
                Some(byte) => {
                    bytes.push(byte);
                    self.at += 1;
                }
                None => return Err(Self::error(start, "unterminated string")),
            }
        }
    }

    fn binary(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let content = self.at;
        while self.input.get(self.at).is_some_and(u8::is_ascii_hexdigit) {
            self.at += 1;
        }
        if self.input.get(self.at) != Some(&b'"') {
            return Err(Self::error(start, "invalid binary literal"));
        }
        let raw = &self.input[content..self.at];
        let Some((&indicator, digits)) = raw.split_first() else {
            return Err(Self::error(
                start,
                "binary literal has no unused-bit indicator",
            ));
        };
        let unused_bits = match indicator {
            b'0'..=b'3' => indicator - b'0',
            _ => {
                return Err(Self::error(
                    start,
                    "binary unused-bit indicator exceeds three",
                ))
            }
        };
        if digits.is_empty() && unused_bits != 0 {
            return Err(Self::error(start, "empty binary payload has unused bits"));
        }
        let nibbles = digits
            .iter()
            .map(|byte| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => unreachable!("binary digits were validated as ASCII hexadecimal"),
            })
            .collect::<Vec<_>>();
        if unused_bits != 0
            && nibbles
                .last()
                .is_some_and(|nibble| nibble & ((1 << unused_bits) - 1) != 0)
        {
            return Err(Self::error(start, "unused binary bits are not zero"));
        }
        let mut data = Vec::with_capacity(nibbles.len().div_ceil(2));
        for chunk in nibbles.chunks(2) {
            data.push((chunk[0] << 4) | chunk.get(1).copied().unwrap_or(0));
        }
        let bit_len = digits.len() * 4 - usize::from(unused_bits);
        self.at += 1;
        Ok(TokenKind::Binary(BinaryValue { bit_len, data }))
    }

    fn resource(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let content = self.at;
        while self.input.get(self.at).is_some_and(|byte| *byte != b'>') {
            self.at += 1;
        }
        if self.input.get(self.at) != Some(&b'>') {
            return Err(Self::error(start, "unterminated resource token"));
        }
        let value = String::from_utf8(self.input[content..self.at].to_vec())
            .map_err(|_| Self::error(start, "resource token is not UTF-8"))?;
        self.at += 1;
        Ok(TokenKind::Resource(value))
    }

    fn error(offset: usize, message: &str) -> LexError {
        LexError {
            offset,
            message: message.into(),
        }
    }
}
