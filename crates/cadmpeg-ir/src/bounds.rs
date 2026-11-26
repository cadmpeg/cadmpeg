// SPDX-License-Identifier: Apache-2.0
//! The two depth bounds of a CADIR document (§2.5).
//!
//! A document nests at most [`MAX_DOCUMENT_DEPTH`] levels of JSON, the root
//! object included: a scalar is no level, and an array or object is one more
//! than its deepest member. An expression's tree is at most
//! [`MAX_EXPRESSION_DEPTH`] levels deep: a leaf is one level, and a node is
//! one more than its deepest operand. Neither bound is a setting (§2.8).
//!
//! Reading refuses text that passes the document bound before a parser
//! recurses into it ([`admit_text`]), and refuses an expression past its
//! bound where it parses one. The decode wrapper gives up each record a
//! decoder states past a bound: it finds them with [`Measure`] while it
//! writes the document's canonical JSON for the content digest.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::{self, Write};

/// The deepest JSON a CADIR document holds, the root object included.
pub const MAX_DOCUMENT_DEPTH: usize = 128;

/// The deepest expression tree a CADIR document holds, a leaf being one level.
pub const MAX_EXPRESSION_DEPTH: usize = 64;

/// The least nesting the decode wrapper's digest walk admits: twice
/// [`MAX_DOCUMENT_DEPTH`], for the nesting a value adds beside its level of
/// JSON (an option, a newtype, a scalar), so that the walk reaches the first
/// container past the bound.
pub(crate) const DIGEST_WALK_DEPTH: u64 = 256;

/// Reads `text` as one JSON value of `T`, as [`deserializer`] admits it.
///
/// # Errors
///
/// Refuses text past the bound, and returns the error of `T` or of trailing
/// text.
pub(crate) fn read_json<'a, T: serde::Deserialize<'a>>(
    text: &'a str,
) -> Result<T, serde_json::Error> {
    let mut deserializer = deserializer(text)?;
    let value = T::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

/// A parser of `text`. Text that nests deeper than [`MAX_DOCUMENT_DEPTH`] is
/// refused before a parser recurses into it, and the parser keeps no
/// recursion limit of its own, so a document at the bound reads. The caller
/// checks for trailing text with [`serde_json::Deserializer::end`].
///
/// # Errors
///
/// Refuses text past the bound.
pub(crate) fn deserializer(
    text: &str,
) -> Result<serde_json::Deserializer<serde_json::de::StrRead<'_>>, serde_json::Error> {
    admit_text(text).map_err(serde::de::Error::custom)?;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.disable_recursion_limit();
    Ok(deserializer)
}

/// Refuses JSON text that nests deeper than [`MAX_DOCUMENT_DEPTH`]. Text that
/// is not JSON passes, for the parser to refuse.
fn admit_text(text: &str) -> Result<(), String> {
    if text_depth(text) > MAX_DOCUMENT_DEPTH {
        return Err(format!(
            "the document nests deeper than the {MAX_DOCUMENT_DEPTH} levels of a CADIR document"
        ));
    }
    Ok(())
}

/// The depth of JSON text (§2.5), read without parsing it.
pub(crate) fn text_depth(text: &str) -> usize {
    let mut scan = Scan::default();
    let mut deepest = 0;
    for &byte in text.as_bytes() {
        scan.byte(byte);
        deepest = deepest.max(scan.depth);
    }
    deepest
}

/// The nesting of JSON text read one byte at a time.
#[derive(Debug, Default)]
struct Scan {
    depth: usize,
    string: bool,
    escaped: bool,
}

impl Scan {
    fn byte(&mut self, byte: u8) {
        if self.string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.string = false;
            }
            return;
        }
        match byte {
            b'"' => self.string = true,
            b'{' | b'[' => self.depth += 1,
            b'}' | b']' => {
                if let Some(depth) = self.depth.checked_sub(1) {
                    self.depth = depth;
                }
            }
            _ => {}
        }
    }
}

thread_local! {
    static EXPRESSION_PAST: Cell<bool> = const { Cell::new(false) };
}

/// Notes that an expression deeper than [`MAX_EXPRESSION_DEPTH`] was
/// written. An expression writes as text, so its depth is not in the JSON;
/// [`Measure`] charges the note to the record being written.
pub(crate) fn note_expression_past() {
    EXPRESSION_PAST.with(|past| past.set(true));
}

pub(crate) fn take_expression_past() -> bool {
    EXPRESSION_PAST.with(|past| past.replace(false))
}

thread_local! {
    static EXPRESSION_REFUSED: Cell<bool> = const { Cell::new(false) };
}

/// Notes that the text of an expression deeper than
/// [`MAX_EXPRESSION_DEPTH`] was refused. The CADIR reader gives up the record
/// that holds it as undecoded (§3.3).
pub(crate) fn note_expression_refused() {
    EXPRESSION_REFUSED.with(|refused| refused.set(true));
}

pub(crate) fn take_expression_refused() -> bool {
    EXPRESSION_REFUSED.with(|refused| refused.replace(false))
}

/// The depth bound of §2.5 that a record or a document passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// [`MAX_DOCUMENT_DEPTH`].
    Document,
    /// [`MAX_EXPRESSION_DEPTH`].
    Expression,
}

impl Bound {
    /// The bound as a report names it.
    pub(crate) fn describe(self) -> String {
        match self {
            Self::Document => {
                format!("nests deeper than the {MAX_DOCUMENT_DEPTH} levels of a CADIR document")
            }
            Self::Expression => format!(
                "holds an expression deeper than the {MAX_EXPRESSION_DEPTH} levels of an expression"
            ),
        }
    }
}

/// Where a record sits in a document: the home or native namespace, the
/// arena, and the record's position in the arena as written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Place {
    /// `home.arena[index]`, `home` being `geometry`, `model`, `design` or
    /// `process`.
    Home {
        home: String,
        arena: String,
        index: usize,
    },
    /// `native.codec.arena[index]`.
    Native {
        codec: String,
        arena: String,
        index: usize,
    },
}

/// Where a value outside every record sits: `field` is the root member, or
/// `home.field` for a member of a home or of `native`, and `entry` is the
/// position in that member's array.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Outside {
    pub(crate) field: String,
    pub(crate) entry: Option<usize>,
}

/// What [`Measure`] found past a bound.
#[derive(Debug, Default)]
pub(crate) struct Measured {
    /// Each record past a bound, with the first bound it passes.
    pub(crate) records: BTreeMap<Place, Bound>,
    /// Each value outside every record past a bound, with the first bound it
    /// passes.
    pub(crate) outside: BTreeMap<Outside, Bound>,
}

const HOMES: [&str; 4] = ["geometry", "model", "design", "process"];

/// One container of the first levels of a document, down to a record's
/// arena: an object with the key of its member being written, or an array
/// with the position of its element being written.
#[derive(Debug)]
struct Level {
    array: bool,
    key: Vec<u8>,
    index: usize,
    expect_key: bool,
}

/// The containers [`Measure`] follows: the root, a home or `native`, a
/// home's arena or a codec namespace, and a native arena.
const FOLLOWED: usize = 4;

/// The objects whose keys [`Measure`] reads: the root, a home or `native`,
/// and a codec namespace.
const KEYED: usize = 3;

/// A writer that passes a document's JSON through and finds the records
/// past a bound in it.
pub(crate) struct Measure<W> {
    inner: W,
    scan: Scan,
    levels: Vec<Level>,
    /// The string being written is a key of the innermost level.
    in_key: bool,
    found: Measured,
}

impl<W: Write> Measure<W> {
    /// Measures the JSON written through to `inner`.
    pub(crate) fn new(inner: W) -> Self {
        take_expression_past();
        Self {
            inner,
            scan: Scan::default(),
            levels: Vec::with_capacity(FOLLOWED),
            in_key: false,
            found: Measured::default(),
        }
    }

    /// What the measure found.
    pub(crate) fn finish(mut self) -> Measured {
        if take_expression_past() {
            self.found
                .outside
                .entry(Outside {
                    field: String::new(),
                    entry: None,
                })
                .or_insert(Bound::Expression);
        }
        self.found
    }

    /// The level of the arena array whose elements are records, for the
    /// record being written: 3 in a home, 4 in a native namespace.
    fn record_level(&self) -> Option<usize> {
        let root = self.levels.first()?;
        if HOMES.iter().any(|home| home.as_bytes() == root.key) {
            Some(3)
        } else if root.key == b"native" {
            Some(4)
        } else {
            None
        }
    }

    /// The record being written, if JSON is inside one.
    fn place(&self) -> Option<Place> {
        let level = self.record_level()?;
        let array = self.levels.get(level - 1).filter(|array| array.array)?;
        let text = |at: usize| String::from_utf8_lossy(&self.levels[at].key).into_owned();
        Some(if level == 3 {
            let (home, arena) = (text(0), text(1));
            crate::give_up::Arena::home(&home, &arena)?;
            Place::Home {
                home,
                arena,
                index: array.index,
            }
        } else {
            Place::Native {
                codec: text(1),
                arena: text(2),
                index: array.index,
            }
        })
    }

    fn mark(&mut self, bound: Bound) {
        match self.place() {
            Some(place) => {
                self.found.records.entry(place).or_insert(bound);
            }
            None => {
                let outside = self.outside();
                self.found.outside.entry(outside).or_insert(bound);
            }
        }
    }

    /// Where the value being written sits, outside every record.
    fn outside(&self) -> Outside {
        let key = |at: usize| {
            self.levels
                .get(at)
                .filter(|level| !level.array)
                .map(|level| String::from_utf8_lossy(&level.key).into_owned())
        };
        let root = key(0).unwrap_or_default();
        let member = (HOMES.contains(&root.as_str()) || root == "native")
            .then(|| key(1))
            .flatten();
        match member {
            Some(member) => Outside {
                field: format!("{root}.{member}"),
                entry: self
                    .levels
                    .get(2)
                    .filter(|level| level.array)
                    .map(|level| level.index),
            },
            None => Outside {
                field: root,
                entry: None,
            },
        }
    }

    /// Charges a noted expression to the value outside every record that
    /// ends here, when a member of the root or of a home ends at `depth`.
    fn member_ends(&mut self, depth: usize) {
        if (1..=2).contains(&depth)
            && self.levels.get(depth - 1).is_some_and(|level| !level.array)
            && take_expression_past()
        {
            self.mark(Bound::Expression);
        }
    }

    /// Charges a noted expression to the record that ends here, when an
    /// element of the array at `depth` ends and the array is a record arena.
    fn element_ends(&mut self, depth: usize) {
        if self.record_level() == Some(depth)
            && self.levels.len() == depth
            && take_expression_past()
        {
            self.mark(Bound::Expression);
        }
    }

    fn byte(&mut self, byte: u8) {
        let in_string = self.scan.string;
        self.scan.byte(byte);
        if in_string {
            if self.in_key {
                if self.scan.string {
                    if let Some(level) = self.levels.last_mut() {
                        level.key.push(byte);
                    }
                } else {
                    self.in_key = false;
                }
            }
            return;
        }
        let followed = self.levels.len() == self.scan.depth;
        match byte {
            b'"' => {
                let keyed = followed && self.levels.len() <= KEYED;
                if let Some(level) = self
                    .levels
                    .last_mut()
                    .filter(|level| keyed && level.expect_key)
                {
                    level.expect_key = false;
                    level.key.clear();
                    self.in_key = true;
                }
            }
            b'{' | b'[' => {
                if self.scan.depth > MAX_DOCUMENT_DEPTH {
                    self.mark(Bound::Document);
                }
                if self.levels.len() + 1 == self.scan.depth && self.levels.len() < FOLLOWED {
                    self.levels.push(Level {
                        array: byte == b'[',
                        key: Vec::new(),
                        index: 0,
                        expect_key: byte == b'{',
                    });
                }
            }
            b'}' | b']' => {
                let closed = self.scan.depth + 1;
                if self.levels.len() == closed {
                    if byte == b']' {
                        self.element_ends(closed);
                    } else {
                        self.member_ends(closed);
                    }
                    self.levels.pop();
                }
            }
            b',' if followed => {
                self.element_ends(self.scan.depth);
                self.member_ends(self.scan.depth);
                if let Some(level) = self.levels.last_mut() {
                    if level.array {
                        level.index += 1;
                    } else {
                        level.expect_key = true;
                    }
                }
            }
            _ => {}
        }
    }
}

impl<W: Write> Write for Measure<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        for &byte in &buf[..written] {
            self.byte(byte);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// A value outside every record that [`unstate_outside`] stated in its
/// unstated form: its field, and what the document states in its place.
#[derive(Debug)]
pub(crate) struct Unstated {
    pub(crate) field: String,
    pub(crate) what: String,
}

/// States each value of `outside`, which belongs to no record and passes a
/// bound, as the value it takes when unstated (§2.4). An entry of the name
/// table states its native name alone, or is removed where it has none
/// (§5.7); any other field takes its value in the empty document.
///
/// # Errors
///
/// [`CodecError::Malformed`] where the document does not read back from its
/// JSON.
pub(crate) fn unstate_outside(
    ir: &mut crate::document::CadIr,
    outside: &BTreeMap<Outside, Bound>,
) -> Result<Vec<Unstated>, cadmpeg_core::CodecError> {
    let mut unstated = Vec::new();
    let mut fields = Vec::new();
    for (place, bound) in outside.iter().rev() {
        let what = match (place.field.as_str(), place.entry) {
            ("design.names", Some(entry)) => {
                let mut entries = ir.design.names.entries().clone();
                let Some((entity, mut stated)) = entries
                    .iter()
                    .nth(entry)
                    .map(|(entity, stated)| (entity.clone(), stated.clone()))
                else {
                    continue;
                };
                let what = if stated.native.is_some() {
                    stated.name = None;
                    entries.insert(entity.clone(), stated);
                    format!(
                        "the name of {} with its native name alone, since its name {}",
                        entity.id(),
                        bound.describe()
                    )
                } else {
                    entries.remove(&entity);
                    format!(
                        "no name of {}, since its name {}",
                        entity.id(),
                        bound.describe()
                    )
                };
                ir.design.names = entries.into();
                what
            }
            (field, _) => {
                fields.push(field);
                format!("the unstated value of a field that {}", bound.describe())
            }
        };
        unstated.push(Unstated {
            field: place.field.clone(),
            what,
        });
    }
    unstate(ir, &fields)?;
    Ok(unstated)
}

/// Replaces each of `fields` of `ir`, a root member or `home.field`, by its
/// value in the empty document, or removes it where the empty document omits
/// it, in one pass through the document's JSON. The arenas the wire omits
/// keep their records.
fn unstate(
    ir: &mut crate::document::CadIr,
    fields: &[&str],
) -> Result<(), cadmpeg_core::CodecError> {
    if fields.is_empty() {
        return Ok(());
    }
    let malformed = |error: String| cadmpeg_core::CodecError::Malformed(error);
    let mut document = crate::hash::finite_json::to_json_value(&*ir)
        .map_err(|error| malformed(error.to_string()))?;
    let empty = crate::hash::finite_json::to_json_value(&crate::document::CadIr::empty())
        .map_err(|error| malformed(error.to_string()))?;
    for field in fields {
        let pointer = format!("/{}", field.replace('.', "/"));
        let unstated = empty.pointer(&pointer).cloned();
        let Some((parent, key)) = pointer.rsplit_once('/') else {
            continue;
        };
        let Some(parent) = document
            .pointer_mut(parent)
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        match unstated {
            Some(value) => {
                parent.insert(key.to_owned(), value);
            }
            None => {
                parent.remove(key);
            }
        }
    }
    let decoded = cadmpeg_core::serde_erase::from_json_value(&document)
        .map_err(|error| malformed(error.to_string()))?;
    let original = std::mem::replace(ir, decoded);
    ir.model.adopt_omitted_arenas(&original.model);
    Ok(())
}

#[cfg(test)]
mod tests;
