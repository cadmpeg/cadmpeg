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
    Name(String),
    UserName(String),
    Instance(u64),
    Integer(i64),
    Real(f64),
    Enumeration(String),
    String(Vec<u8>),
    Binary(Vec<u8>),
    LParen,
    RParen,
    Comma,
    Semicolon,
    Equals,
    Omitted,
    Derived,
}

/// Lexical failure with a stable byte position.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} at byte {offset}")]
pub struct LexError {
    pub offset: usize,
    pub message: String,
}

/// Tokenize one complete clear-text exchange structure.
pub fn lex(input: &[u8]) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer { input, at: 0 };
    let mut tokens = Vec::new();
    while lexer.skip_trivia()? {
        tokens.push(lexer.token()?);
    }
    Ok(tokens)
}

struct Lexer<'a> {
    input: &'a [u8],
    at: usize,
}

impl Lexer<'_> {
    fn skip_trivia(&mut self) -> Result<bool, LexError> {
        loop {
            while self
                .input
                .get(self.at)
                .is_some_and(|b| b.is_ascii_whitespace())
            {
                self.at += 1;
            }
            if self.input.get(self.at..self.at + 2) != Some(b"/*") {
                return Ok(self.at < self.input.len());
            }
            let start = self.at;
            self.at += 2;
            let Some(end) = self.input[self.at..].windows(2).position(|w| w == b"*/") else {
                return Err(self.error(start, "unterminated comment"));
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
            _ => return Err(self.error(start, "unexpected byte")),
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
            return Err(self.error(start, "user-defined name has no identifier"));
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
            return Err(self.error(start, "instance name has no digits"));
        }
        let value = std::str::from_utf8(&self.input[digits..self.at])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| self.error(start, "instance name is out of range"))?;
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
            let normalized = raw.replace(['D', 'd'], "E");
            normalized
                .parse()
                .map(TokenKind::Real)
                .map_err(|_| self.error(start, "invalid real"))
        } else {
            raw.parse()
                .map(TokenKind::Integer)
                .map_err(|_| self.error(start, "invalid integer"))
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
            return Err(self.error(start, "unterminated enumeration"));
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
                    bytes.push(b'\'');
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
                None => return Err(self.error(start, "unterminated string")),
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
            return Err(self.error(start, "invalid binary literal"));
        }
        let bytes = self.input[content..self.at].to_vec();
        self.at += 1;
        Ok(TokenKind::Binary(bytes))
    }

    fn error(&self, offset: usize, message: &str) -> LexError {
        LexError {
            offset,
            message: message.into(),
        }
    }
}
