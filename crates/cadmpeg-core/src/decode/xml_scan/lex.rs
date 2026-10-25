// SPDX-License-Identifier: Apache-2.0
//! Quote-aware XML framing tokens without semantic byte reads.

use super::{XmlEncoding, XmlFramingIssue};

#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    Start {
        name: (usize, usize),
        suffix: usize,
        empty: bool,
    },
    End {
        name: (usize, usize),
        clean: bool,
    },
    Ignored,
    Declaration,
    Bad(XmlFramingIssue),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub(super) struct Lexer<'a> {
    bytes: &'a [u8],
    encoding: XmlEncoding,
    position: usize,
}

impl<'a> Lexer<'a> {
    pub(super) fn new(bytes: &'a [u8], encoding: XmlEncoding) -> Self {
        Self {
            bytes,
            encoding,
            position: 0,
        }
    }

    // ASCII framing operates on code units; name boundaries below use actual
    // XML Name characters while retaining the original encoded source bytes.
    fn unit(&self, position: usize) -> Option<u8> {
        match self.encoding {
            XmlEncoding::Utf8 => self.bytes.get(position).copied(),
            XmlEncoding::Utf16Le | XmlEncoding::Utf16Be => {
                let pair = self.bytes.get(position..position.checked_add(2)?)?;
                let (low, high) = if self.encoding == XmlEncoding::Utf16Le {
                    (pair[0], pair[1])
                } else {
                    (pair[1], pair[0])
                };
                Some(if high == 0 && low < 128 { low } else { 128 })
            }
        }
    }

    fn starts(&self, position: usize, pattern: &[u8]) -> bool {
        pattern.iter().enumerate().all(|(index, byte)| {
            index
                .checked_mul(self.encoding.width())
                .and_then(|delta| position.checked_add(delta))
                .and_then(|at| self.unit(at))
                == Some(*byte)
        })
    }

    fn character(&self, position: usize) -> Option<(char, usize)> {
        match self.encoding {
            XmlEncoding::Utf8 => {
                let width = match *self.bytes.get(position)? {
                    0..=0x7f => 1,
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => return None,
                };
                let raw = self.bytes.get(position..position.checked_add(width)?)?;
                Some((std::str::from_utf8(raw).ok()?.chars().next()?, width))
            }
            XmlEncoding::Utf16Le | XmlEncoding::Utf16Be => {
                let word = |at: usize| {
                    // Offset probes navigate this bounded scanner input without
                    // attributing a semantic read to any record producer.
                    if self.encoding == XmlEncoding::Utf16Le {
                        crate::decode::View::u16_le_at(self.bytes, at)
                    } else {
                        crate::decode::View::u16_be_at(self.bytes, at)
                    }
                };
                let first = word(position)?;
                let (code, width) = if (0xd800..=0xdbff).contains(&first) {
                    let second = word(position.checked_add(2)?)?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return None;
                    }
                    (
                        0x10000 + (u32::from(first) - 0xd800) * 0x400 + u32::from(second) - 0xdc00,
                        4,
                    )
                } else {
                    (u32::from(first), 2)
                };
                Some((char::from_u32(code)?, width))
            }
        }
    }

    fn after(&self, position: usize, pattern: &[u8]) -> usize {
        // Called only after starts() proved every complete code unit exists.
        position + pattern.len() * self.encoding.width()
    }

    fn until(&self, mut position: usize, marker: &[u8]) -> Option<usize> {
        while self.unit(position).is_some() {
            if self.starts(position, marker) {
                return Some(self.after(position, marker));
            }
            position += self.encoding.width();
        }
        None
    }

    fn declaration_end(&self, mut position: usize) -> Option<usize> {
        let mut quote = None;
        let mut brackets = 0usize;
        while let Some(unit) = self.unit(position) {
            if let Some(delimiter) = quote {
                if unit == delimiter {
                    quote = None;
                }
            } else if matches!(unit, b'\'' | b'"') {
                quote = Some(unit);
            } else if self.starts(position, b"<!--") {
                position = self.until(self.after(position, b"<!--"), b"-->")?;
                continue;
            } else if self.starts(position, b"<?") {
                position = self.until(self.after(position, b"<?"), b"?>")?;
                continue;
            } else {
                match unit {
                    b'[' => brackets += 1,
                    b']' if brackets != 0 => brackets -= 1,
                    b'>' if brackets == 0 => return Some(position + self.encoding.width()),
                    _ => {}
                }
            }
            position += self.encoding.width();
        }
        None
    }

    fn tag(&self, start: usize, closing: bool) -> Token {
        let width = self.encoding.width();
        let name_start = start + if closing { 2 * width } else { width };
        let mut position = name_start;
        while let Some((character, width)) = self.character(position) {
            if !(if position == name_start {
                name_start_character(character)
            } else {
                name_character(character)
            }) {
                break;
            }
            position += width;
        }
        let name_end = position;
        let mut quote = None;
        let mut clean = true;
        while let Some(unit) = self.unit(position) {
            if let Some(delimiter) = quote {
                if unit == delimiter {
                    quote = None;
                }
            } else {
                match unit {
                    b'\'' | b'"' => {
                        quote = Some(unit);
                        clean = false;
                    }
                    b'<' => {
                        return Token {
                            start,
                            end: position,
                            kind: Kind::Bad(XmlFramingIssue::MalformedTag),
                        }
                    }
                    b'>' => {
                        let end = position + width;
                        if name_start == name_end {
                            return Token {
                                start,
                                end,
                                kind: Kind::Bad(XmlFramingIssue::MalformedTag),
                            };
                        }
                        let kind = if closing {
                            Kind::End {
                                name: (name_start, name_end),
                                clean,
                            }
                        } else {
                            let empty =
                                position >= width && self.unit(position - width) == Some(b'/');
                            Kind::Start {
                                name: (name_start, name_end),
                                suffix: if empty { position - width } else { position },
                                empty,
                            }
                        };
                        return Token { start, end, kind };
                    }
                    _ => {
                        if !is_space(unit) {
                            clean = false;
                        }
                    }
                }
            }
            position += width;
        }
        Token {
            start,
            end: self.bytes.len(),
            kind: Kind::Bad(XmlFramingIssue::UnterminatedTag),
        }
    }

    pub(super) fn next(&mut self) -> Option<Token> {
        while let Some(unit) = self.unit(self.position) {
            if unit == b'<' {
                break;
            }
            self.position += self.encoding.width();
        }
        let start = self.position;
        if start == self.bytes.len() {
            return None;
        }
        if self.unit(start).is_none() {
            self.position = self.bytes.len();
            return Some(Token {
                start,
                end: self.position,
                kind: Kind::Bad(XmlFramingIssue::IncompleteCodeUnit),
            });
        }
        let token = if self.starts(start, b"<!--") {
            self.delimited(start, b"<!--", b"-->")
        } else if self.starts(start, b"<![CDATA[") {
            self.delimited(start, b"<![CDATA[", b"]]>")
        } else if self.starts(start, b"<?") {
            self.delimited(start, b"<?", b"?>")
        } else if self.starts(start, b"<!") {
            let end = self.declaration_end(self.after(start, b"<!"));
            Token {
                start,
                end: end.unwrap_or(self.bytes.len()),
                kind: if end.is_some() {
                    Kind::Declaration
                } else {
                    Kind::Bad(XmlFramingIssue::UnterminatedDeclaration)
                },
            }
        } else {
            self.tag(start, self.starts(start, b"</"))
        };
        self.position = token.end;
        Some(token)
    }

    fn delimited(&self, start: usize, opening: &[u8], closing: &[u8]) -> Token {
        let end = self.until(self.after(start, opening), closing);
        Token {
            start,
            end: end.unwrap_or(self.bytes.len()),
            kind: if end.is_some() {
                Kind::Ignored
            } else {
                Kind::Bad(XmlFramingIssue::UnterminatedMarkup)
            },
        }
    }
}

fn is_space(unit: u8) -> bool {
    matches!(unit, b' ' | b'\t' | b'\r' | b'\n')
}

fn name_start_character(character: char) -> bool {
    matches!(character, ':' | '_' | 'A'..='Z' | 'a'..='z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}' | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' | '\u{200c}'..='\u{200d}' | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
}

fn name_character(character: char) -> bool {
    name_start_character(character)
        || matches!(character, '-' | '.' | '0'..='9' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
}
