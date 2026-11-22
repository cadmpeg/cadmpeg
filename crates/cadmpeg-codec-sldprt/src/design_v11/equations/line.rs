// SPDX-License-Identifier: Apache-2.0
//! One equation-manager line (`docs/formats/sldprt.md`): a comment that opens
//! with an apostrophe, or a double-quoted name, `=`, and an expression, which
//! an apostrophe outside double quotes ends before a trailing comment. Two
//! double quotes inside a quoted name are one.

/// A line that assigns a name.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Line {
    /// The assigned name, unquoted.
    pub(super) name: String,
    /// The expression text, without the trailing comment, trimmed.
    pub(super) expression: String,
    /// The expression's quoted names and the text around them, in order.
    pub(super) segments: Vec<Segment>,
    pub(super) comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Segment {
    Text(String),
    /// A quoted name, unquoted.
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Parsed {
    Comment,
    Line(Line),
    Malformed,
}

pub(super) fn parse(text: &str) -> Parsed {
    let text = text.trim_start();
    if text.starts_with('\'') {
        return Parsed::Comment;
    }
    let Some((name, rest)) = quoted_name(text) else {
        return Parsed::Malformed;
    };
    let Some(rest) = rest.trim_start().strip_prefix('=') else {
        return Parsed::Malformed;
    };
    let (expression, comment) = match comment_start(rest) {
        Some(at) => (&rest[..at], Some(rest[at + 1..].trim())),
        None => (rest, None),
    };
    let expression = expression.trim();
    let Some(segments) = segments(expression) else {
        return Parsed::Malformed;
    };
    if expression.is_empty() {
        return Parsed::Malformed;
    }
    Parsed::Line(Line {
        name,
        expression: expression.to_owned(),
        segments,
        comment: comment
            .filter(|comment| !comment.is_empty())
            .map(str::to_owned),
    })
}

/// The quoted name that opens `text`, unquoted, and the text after it.
fn quoted_name(text: &str) -> Option<(String, &str)> {
    let body = text.strip_prefix('"')?;
    let mut name = String::new();
    let mut chars = body.char_indices();
    while let Some((at, character)) = chars.next() {
        if character != '"' {
            name.push(character);
            continue;
        }
        if body[at + 1..].starts_with('"') {
            name.push('"');
            chars.next();
            continue;
        }
        return Some((name, &body[at + 1..]));
    }
    None
}

/// The byte offset of the first apostrophe outside double quotes.
fn comment_start(text: &str) -> Option<usize> {
    let mut quoted = false;
    for (at, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '\'' if !quoted => return Some(at),
            _ => {}
        }
    }
    None
}

/// The quoted names of an expression and the text between them, or `None`
/// where a quote is not closed.
fn segments(expression: &str) -> Option<Vec<Segment>> {
    let mut segments = Vec::new();
    let mut rest = expression;
    while let Some(at) = rest.find('"') {
        if at > 0 {
            segments.push(Segment::Text(rest[..at].to_owned()));
        }
        let (name, after) = quoted_name(&rest[at..])?;
        segments.push(Segment::Name(name));
        rest = after;
    }
    if !rest.is_empty() {
        segments.push(Segment::Text(rest.to_owned()));
    }
    Some(segments)
}

/// A name as a line quotes it.
pub(super) fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
