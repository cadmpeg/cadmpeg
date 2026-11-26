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

/// The text [`point32`] writes for a coordinate past the binary32 range.
/// No USDA token holds a NUL character (strings write control characters
/// as `\x` escapes), so a line that contains it holds a point the layer
/// does not state, and the prim whose geometry needs it is left out.
const PAST_RANGE_POINT: &str = "\0";

/// The text [`float`] and [`floats`] write for a value past the binary32
/// range: the attribute line that holds it is left out, and the attribute
/// takes its fallback value. The `PhysX` placeholders use `U+0001`.
const PAST_RANGE_VALUE: &str = "\u{2}";

/// One open block.
#[derive(Debug)]
struct Block {
    /// The prim name, or `None` for a block that is not a named prim.
    name: Option<String>,
    /// Where the prim's head starts in the text.
    start: usize,
    /// Whether a line of the prim, outside its named child prims, holds a
    /// point past the binary32 range.
    past_range: bool,
    /// The points written in the block that binary32 rounds.
    rounded: usize,
}

/// What [`Emitter::finish`] returns.
#[derive(Debug, Default)]
pub(super) struct Finished {
    pub(super) text: String,
    /// The paths of the prims left out because their geometry holds a
    /// point past the binary32 range.
    pub(super) prims: Vec<String>,
    /// The attributes left out because their value is past the binary32
    /// range, each as `prim.attribute`, or as the attribute name outside
    /// every prim.
    pub(super) values: Vec<String>,
    /// The points written that binary32 rounds, in the prims written.
    pub(super) rounded: usize,
}

/// A text buffer with a nesting depth and the names of the open prims.
///
/// A named prim one of whose lines holds a point past the binary32 range is
/// left out when it closes, with its child prims, and its path is
/// recorded: its geometry needs the point. A line outside every named prim
/// that holds one, and an attribute line whose value is past the range, is
/// not written with the metadata block it opens, and the attribute takes
/// its fallback value (CADIR §3.4).
#[derive(Debug, Default)]
pub(super) struct Emitter {
    text: String,
    depth: usize,
    /// Each open block, innermost last.
    blocks: Vec<Block>,
    /// The prim name that the next opened block takes, where its head
    /// starts, and whether a line of its head holds a point past the
    /// binary32 range.
    pending: Option<(String, usize, bool)>,
    /// Whether the lines up to the next `)` are the metadata of a line
    /// left out.
    skipping: bool,
    left_out: Finished,
}

impl Emitter {
    /// The text, and what was left out because the layer does not state it.
    pub(crate) fn finish(self) -> Finished {
        Finished {
            text: self.text,
            ..self.left_out
        }
    }

    /// Writes one indented line.
    pub(crate) fn line(&mut self, content: &str) {
        if self.skipping {
            self.skipping = content != ")";
            return;
        }
        if content.contains(PAST_RANGE_POINT) {
            if let Some((_, _, head)) = &mut self.pending {
                *head = true;
            } else if let Some(prim) = self
                .blocks
                .iter_mut()
                .rev()
                .find(|block| block.name.is_some())
            {
                prim.past_range = true;
            } else {
                self.leave_out(content);
                return;
            }
        } else if content.contains(PAST_RANGE_VALUE) {
            self.leave_out(content);
            return;
        }
        for _ in 0..self.depth * INDENT {
            self.text.push(' ');
        }
        self.text.push_str(content);
        self.text.push('\n');
    }

    /// Records the attribute of the line `content`, which is not written,
    /// and leaves out the metadata block the line opens.
    fn leave_out(&mut self, content: &str) {
        let head = content.split(" = ").next().unwrap_or(content).trim();
        let attribute = head.rsplit(' ').next().unwrap_or(head);
        let mut owner = self.path();
        if let Some((name, _, _)) = &self.pending {
            owner.push('/');
            owner.push_str(name);
        }
        self.left_out.values.push(if owner.is_empty() {
            attribute.to_owned()
        } else {
            format!("{owner}.{attribute}")
        });
        self.skipping = content.ends_with(" (");
    }

    /// Records a point written in the innermost block that binary32
    /// rounds.
    pub(crate) fn rounded_point(&mut self) {
        match self.blocks.last_mut() {
            Some(block) => block.rounded += 1,
            None => self.left_out.rounded += 1,
        }
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
        let (name, start, past_range) = match self.pending.take() {
            Some((name, start, head)) => (Some(name), start, head),
            None => (None, self.text.len(), false),
        };
        self.line("{");
        self.depth += 1;
        self.blocks.push(Block {
            name,
            start,
            past_range,
            rounded: 0,
        });
    }

    pub(crate) fn close(&mut self) {
        self.outdent();
        self.line("}");
        let path = self.path();
        let Some(block) = self.blocks.pop() else {
            return;
        };
        if block.past_range {
            self.text.truncate(block.start);
            let inside = format!("{path}/");
            let property = format!("{path}.");
            self.left_out
                .prims
                .retain(|left_out| !left_out.starts_with(&inside));
            self.left_out.values.retain(|left_out| {
                !left_out.starts_with(&inside) && !left_out.starts_with(&property)
            });
            self.left_out.prims.push(path);
        } else {
            match self.blocks.last_mut() {
                Some(parent) => parent.rounded += block.rounded,
                None => self.left_out.rounded += block.rounded,
            }
        }
    }

    /// Names the prim whose block opens next; its head starts here.
    pub(crate) fn name_next(&mut self, name: &str) {
        self.pending = Some((name.to_owned(), self.text.len(), false));
    }

    /// The path of the innermost open prim.
    pub(crate) fn path(&self) -> String {
        let mut path = String::new();
        for name in self.blocks.iter().filter_map(|block| block.name.as_ref()) {
            path.push('/');
            path.push_str(name);
        }
        path
    }

    /// Writes `head = [` then `items`, wrapped, then `]` and `tail`. An
    /// item past the binary32 range leaves the attribute out.
    pub(crate) fn array(&mut self, head: &str, items: &[String], tail: &str) {
        if items.iter().any(|item| item.contains(PAST_RANGE_VALUE)) {
            self.line(&format!("{head} = {PAST_RANGE_VALUE}{tail}"));
            return;
        }
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

/// The layer text `text` without the attribute lines whose value is the
/// asset path of one of `paths`, with the metadata block each opens: each
/// attribute takes its fallback value.
pub(crate) fn without_assets(text: &str, paths: &[String]) -> String {
    let values: Vec<String> = paths.iter().map(|path| asset(path)).collect();
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    for line in text.split_inclusive('\n') {
        if skipping {
            skipping = line.trim() != ")";
            continue;
        }
        let value = line
            .split_once(" = ")
            .map_or("", |(_, value)| value.trim_end());
        let (value, opens) = value
            .strip_suffix(" (")
            .map_or((value, false), |value| (value, true));
        if values.iter().any(|asset| asset == value) {
            skipping = opens;
            continue;
        }
        out.push_str(line);
    }
    out
}

/// What [`cut_links`] left out.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Cut {
    /// The text without them.
    pub(super) text: String,
    /// The prims left out because they need a prim left out: a joint that
    /// names it, and a prim whose composition arc reaches it.
    pub(super) needing: Vec<String>,
    /// Each link target left out, as `prim.property <target>`.
    pub(super) links: Vec<String>,
}

/// A prim of the text [`cut_links`] reads.
struct Prim {
    path: String,
    /// Whether the prim is a joint: it needs each prim it names.
    joint: bool,
}

/// Whether the trimmed line `line` states a composition arc: the prim
/// composes the prims it targets, so it needs each of them.
fn composes(line: &str) -> bool {
    let head = line.split(" = ").next().unwrap_or_default();
    let mut words = head.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    let last = words.last().unwrap_or(first);
    !matches!(first, "delete" | "reorder")
        && matches!(last, "references" | "inherits" | "specializes" | "payload")
}

/// Whether `path` is one of `prims` or below one.
fn below(path: &str, prims: &[String]) -> bool {
    prims.iter().any(|prim| {
        path.strip_prefix(prim.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// The type and name of the prim whose head is the trimmed line `line`.
fn prim_head(line: &str) -> Option<(&str, &str)> {
    let (keyword, rest) = line.split_once(' ')?;
    if !matches!(keyword, "def" | "over" | "class") {
        return None;
    }
    let (kind, rest) = rest.split_once('"')?;
    let (name, _) = rest.split_once('"')?;
    Some((kind.trim(), name))
}

/// The prim paths `line` targets in this layer: each `<path>` outside a
/// string that no asset path precedes.
fn targets(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut previous = ' ';
    for (at, character) in line.char_indices() {
        if quoted {
            match (escaped, character) {
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => escaped = false,
            }
        } else if character == '"' {
            quoted = true;
        } else if character == '<' && previous != '@' {
            if let Some(length) = line[at + 1..].find('>') {
                let target = &line[at + 1..at + 1 + length];
                if target.starts_with('/') {
                    out.push(target);
                }
            }
        }
        previous = character;
    }
    out
}

/// The prim part of the path `target`.
fn prim_of(target: &str) -> &str {
    target.split('.').next().unwrap_or(target)
}

/// `text` without each link whose target is a prim of `left_out`, a prim
/// below one, or a property of either: a link list keeps its other targets,
/// and a line whose targets are all left out is not written. A joint that
/// names such a prim, and a prim whose composition arc reaches one, needs
/// it, so it is left out too, as are the links that name it.
pub(super) fn cut_links(text: &str, left_out: &[String]) -> Cut {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut prims: Vec<Prim> = Vec::new();
    let mut owners: Vec<Option<usize>> = Vec::with_capacity(lines.len());
    let mut stack: Vec<Option<usize>> = Vec::new();
    let mut pending: Option<usize> = None;
    for line in &lines {
        let trimmed = line.trim();
        let enclosing = stack.iter().rev().find_map(|block| *block);
        if let Some((kind, name)) = prim_head(trimmed) {
            let parent = enclosing.map_or("", |index| prims[index].path.as_str());
            prims.push(Prim {
                path: format!("{parent}/{name}"),
                joint: kind.ends_with("Joint"),
            });
            pending = Some(prims.len() - 1);
            owners.push(pending);
            continue;
        }
        match trimmed {
            "{" => stack.push(pending.take()),
            "}" => {
                owners.push(enclosing);
                stack.pop();
                continue;
            }
            _ => {}
        }
        owners.push(pending.or_else(|| stack.iter().rev().find_map(|block| *block)));
    }
    let mut gone = left_out.to_vec();
    let mut needing = Vec::new();
    loop {
        let mut grew = false;
        for (line, owner) in lines.iter().zip(&owners) {
            let Some(prim) = owner.map(|index| &prims[index]) else {
                continue;
            };
            if (prim.joint || composes(line.trim()))
                && !below(&prim.path, &gone)
                && targets(line)
                    .iter()
                    .any(|target| below(prim_of(target), &gone))
            {
                gone.push(prim.path.clone());
                needing.push(prim.path.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut out = Cut {
        text: String::with_capacity(text.len()),
        needing,
        links: Vec::new(),
    };
    let mut skipping = false;
    for (line, owner) in lines.iter().zip(&owners) {
        let owner = owner.map_or("", |index| prims[index].path.as_str());
        if below(owner, &gone) {
            continue;
        }
        if skipping {
            skipping = line.trim() != ")";
            continue;
        }
        let dangling: Vec<&str> = targets(line)
            .into_iter()
            .filter(|target| below(prim_of(target), &gone))
            .collect();
        if dangling.is_empty() {
            out.text.push_str(line);
            continue;
        }
        let (head, value) = line.split_once(" = ").unwrap_or((line, ""));
        let value = value.trim_end();
        let (value, tail) = value
            .strip_suffix(" (")
            .map_or((value, ""), |value| (value, " ("));
        let property = head.trim().rsplit(' ').next().unwrap_or_default();
        out.links.extend(
            dangling
                .iter()
                .map(|target| format!("{owner}.{property} <{target}>")),
        );
        let kept: Vec<&str> = value
            .strip_prefix('[')
            .and_then(|list| list.strip_suffix(']'))
            .map(|list| {
                list.split(", ")
                    .filter(|item| {
                        !targets(item)
                            .iter()
                            .any(|target| below(prim_of(target), &gone))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if kept.is_empty() {
            skipping = !tail.is_empty();
        } else {
            for part in [head, " = [", kept.join(", ").as_str(), "]", tail, "\n"] {
                out.text.push_str(part);
            }
        }
    }
    out
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
pub(crate) fn asset(path: &str) -> String {
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

/// The USDA token of a non-finite value: `inf`, `-inf` or `nan`. A
/// floating-point attribute holds it exactly.
fn non_finite(value: f64) -> &'static str {
    if value.is_nan() {
        "nan"
    } else if value.is_sign_negative() {
        "-inf"
    } else {
        "inf"
    }
}

/// A binary64 value in the shortest text that reads back to it.
pub(super) fn real(value: f64) -> String {
    if !value.is_finite() {
        return non_finite(value).to_owned();
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
    if !value.is_finite() {
        return non_finite(f64::from(value)).to_owned();
    }
    let magnitude = f64::from(value.abs());
    if magnitude == 0.0 || (SMALLEST_PLAIN..LARGEST_PLAIN).contains(&magnitude) {
        format!("{value}")
    } else {
        format!("{value:e}")
    }
}

/// `value` rounded to binary32, or `past_range` for a finite value past
/// the binary32 range.
fn binary32(value: f64, past_range: &str) -> String {
    let rounded = crate::text::value::round_f32(value);
    if value.is_finite() && !rounded.is_finite() {
        past_range.to_owned()
    } else {
        real32(rounded)
    }
}

/// A `float` attribute value: `value` rounded to binary32. A value past
/// the binary32 range leaves its attribute out ([`Emitter`]).
pub(super) fn float(value: f64) -> String {
    binary32(value, PAST_RANGE_VALUE)
}

/// A `float` tuple value, each component rounded to binary32. A component
/// past the binary32 range leaves its attribute out ([`Emitter`]).
pub(super) fn floats(values: &[f64]) -> String {
    format!(
        "({})",
        values
            .iter()
            .map(|value| binary32(*value, PAST_RANGE_VALUE))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A point of a prim's geometry, each coordinate rounded to binary32. A
/// coordinate past the binary32 range leaves the prim out ([`Emitter`]).
pub(super) fn point32(values: &[f64]) -> String {
    format!(
        "({})",
        values
            .iter()
            .map(|value| binary32(*value, PAST_RANGE_POINT))
            .collect::<Vec<_>>()
            .join(", ")
    )
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
