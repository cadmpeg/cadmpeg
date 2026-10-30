// SPDX-License-Identifier: Apache-2.0
//! USDA text emission: indentation, names, strings, and numbers.

use std::collections::BTreeSet;

/// Spaces per nesting level.
const INDENT: usize = 4;
/// Magnitudes at or above this, and below [`SMALLEST_PLAIN`], use exponent
/// notation.
const LARGEST_PLAIN: f64 = 1e15;
const SMALLEST_PLAIN: f64 = 1e-5;
/// Values written on one line of an array before a line break.
const VALUES_PER_LINE: usize = 8;

/// A text buffer with a nesting depth and the names of the open prims.
#[derive(Debug, Default)]
pub(super) struct Emitter {
    text: String,
    depth: usize,
    /// The name of each open block: a prim name, or `None` for a block that
    /// is not a named prim.
    names: Vec<Option<String>>,
    /// The prim name that the next opened block takes.
    pending: Option<String>,
}

impl Emitter {
    pub(crate) fn finish(self) -> String {
        self.text
    }

    /// Writes one indented line.
    pub(crate) fn line(&mut self, content: &str) {
        for _ in 0..self.depth * INDENT {
            self.text.push(' ');
        }
        self.text.push_str(content);
        self.text.push('\n');
    }

    pub(crate) fn blank(&mut self) {
        self.text.push('\n');
    }

    fn outdent(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    pub(crate) fn open(&mut self) {
        self.line("{");
        self.depth += 1;
        let name = self.pending.take();
        self.names.push(name);
    }

    pub(crate) fn close(&mut self) {
        self.outdent();
        self.line("}");
        self.names.pop();
    }

    /// Names the prim whose block opens next.
    pub(crate) fn name_next(&mut self, name: &str) {
        self.pending = Some(name.to_owned());
    }

    /// The path of the innermost open prim.
    pub(crate) fn path(&self) -> String {
        let mut path = String::new();
        for name in self.names.iter().flatten() {
            path.push('/');
            path.push_str(name);
        }
        path
    }

    /// Writes `head = [` then `items`, wrapped, then `]` and `tail`.
    pub(crate) fn array(&mut self, head: &str, items: &[String], tail: &str) {
        if items.len() <= VALUES_PER_LINE {
            self.line(&format!("{head} = [{}]{tail}", items.join(", ")));
            return;
        }
        self.line(&format!("{head} = ["));
        self.depth += 1;
        let chunks: Vec<&[String]> = items.chunks(VALUES_PER_LINE).collect();
        for (index, chunk) in chunks.iter().enumerate() {
            let separator = if index + 1 == chunks.len() { "" } else { "," };
            self.line(&format!("{}{separator}", chunk.join(", ")));
        }
        self.outdent();
        self.line(&format!("]{tail}"));
    }
}

/// A quoted string with the escapes of spec §2.7.
pub(super) fn quoted(text: &str) -> String {
    let mut out = String::from('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 || control == '\u{7f}' => {
                out.push_str("\\x");
                out.push(hex_digit(u32::from(control) >> 4));
                out.push(hex_digit(u32::from(control) & 0xf));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// The lowercase hexadecimal digit of a value below 16.
fn hex_digit(value: u32) -> char {
    char::from_digit(value, 16).unwrap_or('0')
}

/// An asset path in the delimiter its text admits (spec §2.8).
pub(super) fn asset(path: &str) -> String {
    if path.contains('@') {
        format!("@@@{}@@@", path.replace("@@@", "\\@@@"))
    } else {
        format!("@{path}@")
    }
}

/// A prim name: the identifier of spec §2.4 nearest to `name`, in ASCII.
pub(super) fn identifier(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|character: char| character.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Unique sibling names.
#[derive(Debug, Default)]
pub(super) struct Names {
    taken: BTreeSet<String>,
    /// Names given by [`Self::unique`], in order.
    given: Vec<String>,
}

impl Names {
    pub(crate) fn reserve(&mut self, name: &str) {
        self.taken.insert(name.to_owned());
    }

    /// `base` made an identifier and made unique among the names taken.
    pub(crate) fn unique(&mut self, base: &str) -> String {
        let base = identifier(base);
        let name = if self.taken.insert(base.clone()) {
            base
        } else {
            let mut suffix = 1_usize;
            loop {
                let candidate = format!("{base}_{suffix}");
                if self.taken.insert(candidate.clone()) {
                    break candidate;
                }
                suffix += 1;
            }
        };
        self.given.push(name.clone());
        name
    }

    /// The first name [`Self::unique`] gave.
    pub(crate) fn first(&self) -> Option<&str> {
        self.given.first().map(String::as_str)
    }
}

/// A binary64 value in the shortest text that reads back to it.
pub(super) fn real(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    let magnitude = value.abs();
    if magnitude == 0.0 || (SMALLEST_PLAIN..LARGEST_PLAIN).contains(&magnitude) {
        format!("{value}")
    } else {
        format!("{value:e}")
    }
}

/// A binary32 value in the shortest text that reads back to it.
pub(super) fn real32(value: f32) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    let magnitude = f64::from(value.abs());
    if magnitude == 0.0 || (SMALLEST_PLAIN..LARGEST_PLAIN).contains(&magnitude) {
        format!("{value}")
    } else {
        format!("{value:e}")
    }
}

/// A tuple of binary64 values.
pub(super) fn tuple(values: &[f64]) -> String {
    format!(
        "({})",
        values
            .iter()
            .map(|value| real(*value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A tuple of binary32 values.
pub(super) fn tuple32(values: &[f32]) -> String {
    format!(
        "({})",
        values
            .iter()
            .map(|value| real32(*value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests;
