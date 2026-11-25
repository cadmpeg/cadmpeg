// SPDX-License-Identifier: Apache-2.0
//! The JSON text of a document, read for its framing.
//!
//! [`Scanner`] finds where each member of an object and each element of an
//! array starts and ends without reading its content, so a reader can read
//! each part on its own and keep every part that closes before the point
//! where the text stops being JSON. [`value`] reads one part's text into a
//! `serde_json::Value` for the reader's repairs: it keeps every copy of a
//! restated key, and replaces an invalid UTF-8 sequence in a string by
//! U+FFFD, reporting both.

use serde_json::{Map, Number, Value};

/// The extent of one value in the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Span {
    /// The offset of the value's first byte.
    pub(super) start: usize,
    /// The offset after the value's last byte, or the point where the text
    /// stops being JSON inside the value.
    pub(super) end: usize,
    /// Whether the value closes before the text stops being JSON.
    pub(super) closed: bool,
    /// The value's JSON depth (§2.5), as far as it was read.
    pub(super) depth: usize,
}

/// One member of an object: its key and its value's extent.
#[derive(Debug, Clone)]
pub(super) struct Member {
    /// The key, with each invalid UTF-8 sequence replaced by U+FFFD.
    pub(super) key: String,
    /// Whether the key holds an invalid UTF-8 sequence.
    pub(super) key_invalid: bool,
    /// The offset of the key's opening quote.
    pub(super) at: usize,
    /// The member's value.
    pub(super) value: Span,
}

/// Where the text stops being JSON, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Stop {
    /// The offset of the first byte that is not JSON, or the input's length.
    pub(super) at: usize,
    /// What the reader expected there.
    pub(super) expected: &'static str,
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the text stops being JSON at offset {}: expected {}",
            self.at, self.expected
        )
    }
}

/// A cursor over the input that frames values.
pub(super) struct Scanner<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Scanner<'a> {
    pub(super) const fn new(bytes: &'a [u8], at: usize) -> Self {
        Self { bytes, at }
    }

    pub(super) const fn at(&self) -> usize {
        self.at
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn stop(&self, expected: &'static str) -> Stop {
        Stop {
            at: self.at,
            expected,
        }
    }

    pub(super) fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8, expected: &'static str) -> Result<(), Stop> {
        self.whitespace();
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.stop(expected))
        }
    }

    /// Whether the next byte, after whitespace, is `byte`.
    pub(super) fn next_is(&mut self, byte: u8) -> bool {
        self.whitespace();
        self.peek() == Some(byte)
    }

    /// Reads a string and returns its text, and whether it held an invalid
    /// UTF-8 sequence or an unpaired surrogate escape, each replaced by
    /// U+FFFD.
    pub(super) fn string(&mut self) -> Result<(String, bool), Stop> {
        self.eat(b'"', "a string")?;
        let mut raw = Vec::new();
        let mut invalid = false;
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.stop("the end of a string"));
            };
            match byte {
                b'"' => {
                    self.at += 1;
                    break;
                }
                b'\\' => {
                    self.at += 1;
                    let escaped = match self.peek() {
                        Some(b'"') => b'"',
                        Some(b'\\') => b'\\',
                        Some(b'/') => b'/',
                        Some(b'b') => 0x08,
                        Some(b'f') => 0x0c,
                        Some(b'n') => b'\n',
                        Some(b'r') => b'\r',
                        Some(b't') => b'\t',
                        Some(b'u') => {
                            self.at += 1;
                            let unit = self.hex4()?;
                            let scalar = self.surrogate_pair(unit)?;
                            let character = scalar.unwrap_or_else(|| {
                                invalid = true;
                                char::REPLACEMENT_CHARACTER
                            });
                            let mut buffer = [0_u8; 4];
                            raw.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                            continue;
                        }
                        _ => return Err(self.stop("an escape")),
                    };
                    self.at += 1;
                    raw.push(escaped);
                }
                0x00..=0x1f => return Err(self.stop("a string character")),
                _ => {
                    self.at += 1;
                    raw.push(byte);
                }
            }
        }
        match String::from_utf8(raw) {
            Ok(text) => Ok((text, invalid)),
            Err(error) => Ok((String::from_utf8_lossy(error.as_bytes()).into_owned(), true)),
        }
    }

    fn hex4(&mut self) -> Result<u32, Stop> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.stop("four hex digits"))?;
        let mut unit = 0_u32;
        for digit in digits {
            let value = char::from(*digit)
                .to_digit(16)
                .ok_or_else(|| self.stop("four hex digits"))?;
            unit = unit * 16 + value;
        }
        self.at += 4;
        Ok(unit)
    }

    /// The scalar a `\u` escape states, reading the low half that follows a
    /// high surrogate; `None` for an unpaired surrogate.
    fn surrogate_pair(&mut self, unit: u32) -> Result<Option<char>, Stop> {
        if !(0xd800..0xdc00).contains(&unit) {
            return Ok(char::from_u32(unit));
        }
        if self.bytes.get(self.at..self.at + 2) != Some(b"\\u") {
            return Ok(None);
        }
        let resume = self.at;
        self.at += 2;
        let low = self.hex4()?;
        if !(0xdc00..0xe000).contains(&low) {
            self.at = resume;
            return Ok(None);
        }
        Ok(char::from_u32(
            0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00),
        ))
    }

    fn literal(&mut self, text: &'static [u8]) -> Result<(), Stop> {
        if self.bytes.get(self.at..self.at + text.len()) == Some(text) {
            self.at += text.len();
            Ok(())
        } else {
            Err(self.stop("a JSON value"))
        }
    }

    /// Reads a number and returns its text.
    fn number(&mut self) -> Result<&'a str, Stop> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.stop("a digit")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.stop("a digit"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.stop("a digit"));
            }
            self.digits();
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or_default())
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
    }

    fn scalar(&mut self) -> Result<(), Stop> {
        match self.peek() {
            Some(b'"') => self.string().map(drop),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number().map(drop),
            _ => Err(self.stop("a JSON value")),
        }
    }

    /// Frames the next value. The walk keeps its own stack, so input of any
    /// nesting is framed without recursion.
    pub(super) fn value(&mut self) -> Span {
        self.whitespace();
        let start = self.at;
        let mut open: Vec<u8> = Vec::new();
        let mut depth = 0;
        let closed = self.walk(&mut open, &mut depth).is_ok();
        Span {
            start,
            end: self.at,
            closed,
            depth,
        }
    }

    fn walk(&mut self, open: &mut Vec<u8>, depth: &mut usize) -> Result<(), Stop> {
        loop {
            self.whitespace();
            match self.peek() {
                Some(byte @ (b'{' | b'[')) => {
                    self.at += 1;
                    open.push(if byte == b'{' { b'}' } else { b']' });
                    *depth = (*depth).max(open.len());
                    self.whitespace();
                    if self.peek() == Some(open[open.len() - 1]) {
                        self.at += 1;
                        open.pop();
                    } else {
                        if byte == b'{' {
                            self.string()?;
                            self.eat(b':', "`:`")?;
                        }
                        continue;
                    }
                }
                _ => self.scalar()?,
            }
            loop {
                let Some(&close) = open.last() else {
                    return Ok(());
                };
                self.whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.at += 1;
                        if close == b'}' {
                            self.string()?;
                            self.eat(b':', "`:`")?;
                        }
                        break;
                    }
                    Some(byte) if byte == close => {
                        self.at += 1;
                        open.pop();
                    }
                    _ => return Err(self.stop("`,` or the end of a container")),
                }
            }
        }
    }

    /// Frames the members of the object at the cursor. Returns the members
    /// whose values were framed, and where the text stops being JSON when it
    /// does inside the object; a member whose value does not close is
    /// returned with an open span.
    pub(super) fn members(&mut self) -> (Vec<Member>, Option<Stop>) {
        let mut members = Vec::new();
        let stop = self.members_into(&mut members).err();
        (members, stop)
    }

    fn members_into(&mut self, members: &mut Vec<Member>) -> Result<(), Stop> {
        self.eat(b'{', "an object")?;
        if self.next_is(b'}') {
            self.at += 1;
            return Ok(());
        }
        loop {
            self.whitespace();
            let at = self.at;
            let (key, key_invalid) = self.string()?;
            self.eat(b':', "`:`")?;
            let value = self.value();
            let closed = value.closed;
            members.push(Member {
                key,
                key_invalid,
                at,
                value,
            });
            if !closed {
                return Err(self.stop("a JSON value"));
            }
            self.whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err(self.stop("`,` or `}`")),
            }
        }
    }

    /// Frames the elements of the array at the cursor, as
    /// [`Self::members`] frames an object's members.
    pub(super) fn elements(&mut self) -> (Vec<Span>, Option<Stop>) {
        let mut elements = Vec::new();
        let stop = self.elements_into(&mut elements).err();
        (elements, stop)
    }

    fn elements_into(&mut self, elements: &mut Vec<Span>) -> Result<(), Stop> {
        self.eat(b'[', "an array")?;
        if self.next_is(b']') {
            self.at += 1;
            return Ok(());
        }
        loop {
            let element = self.value();
            let closed = element.closed;
            elements.push(element);
            if !closed {
                return Err(self.stop("a JSON value"));
            }
            self.whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err(self.stop("`,` or `]`")),
            }
        }
    }
}

/// What reading a part's text into a value found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Found {
    /// A string held an invalid UTF-8 sequence, replaced by U+FFFD.
    Replaced(usize),
    /// A key was stated twice and every copy holds one value.
    Restated(String),
    /// A key was stated twice with different values; no copy is kept. Each
    /// copy is the extent from its key's opening quote to its value's end.
    Conflicting {
        key: String,
        copies: Vec<(usize, usize)>,
    },
}

/// Why a part's text has no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Unreadable {
    /// The text is not JSON.
    Text(Stop),
    /// A key holds an invalid UTF-8 sequence.
    Key(usize),
    /// A string a record reads as an id holds an invalid UTF-8 sequence: the
    /// value of an `id` member, or text in the identity grammar.
    Id(usize),
    /// A number is past the range of a 64-bit float.
    Number(usize),
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(stop) => stop.fmt(f),
            Self::Key(at) => write!(f, "the key at offset {at} holds an invalid UTF-8 sequence"),
            Self::Id(at) => write!(f, "the id at offset {at} holds an invalid UTF-8 sequence"),
            Self::Number(at) => {
                write!(
                    f,
                    "the number at offset {at} is past the range of a 64-bit float"
                )
            }
        }
    }
}

/// Reads the value that `span` frames. The span's depth is within §2.5's
/// bound, which bounds the recursion.
pub(super) fn value(bytes: &[u8], span: Span, found: &mut Vec<Found>) -> Result<Value, Unreadable> {
    let mut scanner = Scanner::new(bytes, span.start);
    scanner.read(found, None)
}

impl Scanner<'_> {
    /// Reads the value at the cursor; `member` is the key that holds it.
    fn read(&mut self, found: &mut Vec<Found>, member: Option<&str>) -> Result<Value, Unreadable> {
        self.whitespace();
        let at = self.at;
        match self.peek() {
            Some(b'{') => self.read_object(found),
            Some(b'[') => {
                self.at += 1;
                let mut values = Vec::new();
                if self.next_is(b']') {
                    self.at += 1;
                    return Ok(Value::Array(values));
                }
                loop {
                    values.push(self.read(found, None)?);
                    self.whitespace();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Value::Array(values));
                        }
                        _ => return Err(Unreadable::Text(self.stop("`,` or `]`"))),
                    }
                }
            }
            Some(b'"') => {
                let (text, invalid) = self.string().map_err(Unreadable::Text)?;
                if invalid {
                    if member == Some("id") || crate::ids::is_valid_identity(&text) {
                        return Err(Unreadable::Id(at));
                    }
                    found.push(Found::Replaced(at));
                }
                Ok(Value::String(text))
            }
            Some(b'-' | b'0'..=b'9') => {
                let text = self.number().map_err(Unreadable::Text)?;
                number(text).ok_or(Unreadable::Number(at))
            }
            _ => {
                self.scalar().map_err(Unreadable::Text)?;
                Ok(match self.bytes[at] {
                    b't' => Value::Bool(true),
                    b'f' => Value::Bool(false),
                    _ => Value::Null,
                })
            }
        }
    }

    fn read_object(&mut self, found: &mut Vec<Found>) -> Result<Value, Unreadable> {
        self.at += 1;
        let mut copies: Vec<(String, (usize, usize), Value)> = Vec::new();
        if self.next_is(b'}') {
            self.at += 1;
        } else {
            loop {
                self.whitespace();
                let at = self.at;
                let (key, invalid) = self.string().map_err(Unreadable::Text)?;
                if invalid {
                    return Err(Unreadable::Key(at));
                }
                self.eat(b':', "`:`").map_err(Unreadable::Text)?;
                let value = self.read(found, Some(&key))?;
                copies.push((key, (at, self.at), value));
                self.whitespace();
                match self.peek() {
                    Some(b',') => self.at += 1,
                    Some(b'}') => {
                        self.at += 1;
                        break;
                    }
                    _ => return Err(Unreadable::Text(self.stop("`,` or `}`"))),
                }
            }
        }
        let mut object = Map::new();
        let mut extents: std::collections::BTreeMap<String, Vec<(usize, usize)>> =
            std::collections::BTreeMap::new();
        let mut conflicting = std::collections::BTreeSet::new();
        for (key, extent, value) in copies {
            extents.entry(key.clone()).or_default().push(extent);
            match object.get(&key) {
                Some(held) if *held == value => found.push(Found::Restated(key)),
                Some(_) => {
                    conflicting.insert(key);
                }
                None => {
                    object.insert(key, value);
                }
            }
        }
        for key in conflicting {
            object.remove(&key);
            found.push(Found::Conflicting {
                copies: extents.remove(&key).unwrap_or_default(),
                key,
            });
        }
        Ok(Value::Object(object))
    }
}

/// The value of a JSON number, read as `serde_json` reads it: an integer
/// where the text states one that fits 64 bits, and a float otherwise.
fn number(text: &str) -> Option<Value> {
    if !text.contains(['.', 'e', 'E']) {
        if let Ok(value) = text.parse::<u64>() {
            return Some(Value::Number(value.into()));
        }
        if let Ok(value) = text.parse::<i64>() {
            return Some(Value::Number(value.into()));
        }
    }
    let value = text.parse::<f64>().ok()?;
    Number::from_f64(value).map(Value::Number)
}

#[cfg(test)]
mod tests;
