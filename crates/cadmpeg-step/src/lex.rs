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
pub enum TokenKind {
    /// Standard keyword or entity name.
    Name(String),
    /// User-defined `!`-prefixed keyword.
    UserName(String),
    /// Numeric `#`-prefixed entity-instance name.
    Instance(u64),
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
    let mut lexer = Lexer { input, at: 0 };
    let mut tokens = Vec::new();
    while lexer.skip_trivia()? {
        tokens.push(lexer.token()?);
        if matches!(
            tokens.last().map(|token| &token.kind),
            Some(TokenKind::Semicolon)
        ) && matches!(
            tokens.get(tokens.len().saturating_sub(2)).map(|token| &token.kind),
            Some(TokenKind::Name(name)) if name == "SIGNATURE"
        ) {
            lexer.skip_signature_payload()?;
        }
    }
    Ok(tokens)
}

struct Lexer<'a> {
    input: &'a [u8],
    at: usize,
}

impl Lexer<'_> {
    fn skip_signature_payload(&mut self) -> Result<(), LexError> {
        let start = self.at;
        let tail = &self.input[start..];
        let exchange_end = tail
            .windows(b"END-ISO-10303-21".len())
            .position(|window| window == b"END-ISO-10303-21")
            .ok_or_else(|| Self::error(start, "unterminated signature section"))?;
        let section_end = tail[..exchange_end]
            .windows(b"ENDSEC".len())
            .rposition(|window| window == b"ENDSEC")
            .ok_or_else(|| Self::error(start, "unterminated signature section"))?;
        self.at = start + section_end;
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
            b'#' => self.instance()?,
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
            b if b.is_ascii_alphabetic() => self.name(),
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
        if !self.input.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
            return Err(Self::error(start, "user-defined name has no identifier"));
        }
        let TokenKind::Name(name) = self.name() else {
            unreachable!()
        };
        Ok(TokenKind::UserName(name))
    }

    fn instance(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let digits = self.at;
        while self.input.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        if digits == self.at {
            return Err(Self::error(start, "instance name has no digits"));
        }
        let value = std::str::from_utf8(&self.input[digits..self.at])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| Self::error(start, "instance name is out of range"))?;
        Ok(TokenKind::Instance(value))
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
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
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
