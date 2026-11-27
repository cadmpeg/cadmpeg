// SPDX-License-Identifier: Apache-2.0
//! The DOCTYPE of an XML text as roxmltree 0.21.1 reads it, and bounds on
//! what its entity references expand to.
//!
//! roxmltree reads the internal subset only: it stores each `<!ENTITY>`
//! declaration, skips `<!ELEMENT>`, `<!ATTLIST>` and `<!NOTATION>`, and
//! fetches no external subset. The session gives it a resolver that
//! expands every external entity to empty text.

use std::collections::HashSet;
use std::mem::size_of;
use std::ops::Range;

use super::{u64_from_index, xml_counts_checked, XmlCounts, XmlExpansion, XmlScope};

/// What a parse leaves unread from a DOCTYPE.
#[derive(Clone, Copy, Default)]
pub(super) struct Unread {
    /// The DOCTYPE names an external subset.
    pub(super) external_subset: bool,
    /// The internal subset declares attribute defaults.
    pub(super) attribute_defaults: bool,
    /// References that expand an external entity to empty text.
    pub(super) external_references: u64,
}

/// What the references of a text expand to, summed over the references.
pub(super) struct Expanded {
    /// The expanded text, as the scan counts it.
    pub(super) counts: XmlCounts,
    pub(super) expansion: XmlExpansion,
    /// Nesting levels past the document's: the elements an expansion
    /// opens and roxmltree's recursion per entity level.
    pub(super) depth: u64,
    pub(super) unread: Unread,
}

/// Why a DOCTYPE stops the parse before it is charged.
pub(super) enum Stop<E> {
    /// A check or admission refused.
    Refused(E),
    /// A bound passes `u64`.
    Unbounded,
    /// The DOCTYPE does not parse, or an entity refers to itself or holds
    /// content that does not nest.
    Malformed(String),
}

/// One `<!ENTITY>` declaration.
struct Declared<'text> {
    name: &'text [u8],
    /// The value's range in the text; none for an external entity.
    value: Option<Range<usize>>,
}

/// The references to one entity from the document.
#[derive(Clone, Copy, Default)]
struct Roots {
    text: u64,
    value: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    New,
    Open,
    Done,
}

/// The expansion of one entity: its own text until the fold completes it,
/// then its text with every nested expansion.
#[derive(Clone, Copy)]
struct Plan {
    counts: XmlCounts,
    /// Its references in the shared reference list.
    references: (usize, usize),
    bytes: u64,
    nested: u64,
    levels: u64,
    depth: u64,
    external: u64,
    visit: Visit,
}

/// The DOCTYPE's range in the text: the first markup after a byte order
/// mark, an XML declaration, comments, processing instructions and spaces,
/// where roxmltree reads it.
fn locate(bytes: &[u8]) -> Option<usize> {
    let mut at = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    if bytes.get(at..)?.starts_with(b"<?xml ") {
        at = declaration_end(bytes, at)?;
    }
    loop {
        at = spaces(bytes, at);
        let tail = bytes.get(at..)?;
        if tail.starts_with(b"<!--") {
            at = find(bytes, at + 4, b"-->")? + 3;
        } else if tail.starts_with(b"<?") {
            at = find(bytes, at + 2, b"?>")? + 2;
        } else if tail.starts_with(b"<!DOCTYPE") {
            return Some(at);
        } else {
            return None;
        }
    }
}

/// The end of the XML declaration at `at`: its first `?>` outside quotes.
fn declaration_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut quote = None::<u8>;
    for (index, &byte) in bytes.iter().enumerate().skip(at) {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if matches!(byte, b'"' | b'\'') => quote = Some(byte),
            None if bytes
                .get(index..)
                .is_some_and(|tail| tail.starts_with(b"?>")) =>
            {
                return Some(index + 2)
            }
            None => {}
        }
    }
    None
}

fn spaces(bytes: &[u8], mut at: usize) -> usize {
    while bytes
        .get(at)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        at += 1;
    }
    at
}

/// The start of the first `pattern` at or after `from`.
fn find(bytes: &[u8], from: usize, pattern: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(pattern.len())
        .position(|window| window == pattern)
        .map(|offset| from + offset)
}

/// The end of a name at `at`: the first space, quote, `%`, `[` or `>`.
fn name_end(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|byte| {
        !matches!(
            byte,
            b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b'%' | b'[' | b'>'
        )
    }) {
        at += 1;
    }
    at
}

/// The end of a quoted literal at `at`.
fn literal_end(bytes: &[u8], at: usize) -> Result<usize, &'static str> {
    const MISSING: &str = "a DOCTYPE literal does not close";
    let quote = *bytes
        .get(at)
        .filter(|byte| matches!(byte, b'"' | b'\''))
        .ok_or("a DOCTYPE literal has no quote")?;
    let close = bytes
        .iter()
        .skip(at + 1)
        .position(|&byte| byte == quote)
        .ok_or(MISSING)?;
    Ok(at + 1 + close + 1)
}

/// The end of a `SYSTEM` or `PUBLIC` identifier at `at`.
fn external_id_end(bytes: &[u8], at: usize) -> Result<usize, &'static str> {
    let tail = bytes.get(at..).unwrap_or_default();
    let public = tail.starts_with(b"PUBLIC");
    if !public && !tail.starts_with(b"SYSTEM") {
        return Err("a DOCTYPE external identifier is neither SYSTEM nor PUBLIC");
    }
    let mut end = literal_end(bytes, spaces(bytes, at + 6))?;
    if public {
        end = literal_end(bytes, spaces(bytes, end))?;
    }
    Ok(end)
}

/// The DOCTYPE at `start` as roxmltree 0.21.1 reads it: its end and what it
/// leaves unread. `declare` receives each entity declaration in order.
fn walk<'text>(
    bytes: &'text [u8],
    start: usize,
    mut declare: impl FnMut(Declared<'text>),
) -> Result<(usize, Unread), &'static str> {
    const OPEN: &str = "the DOCTYPE does not close";
    let mut unread = Unread::default();
    let mut at = spaces(bytes, start + b"<!DOCTYPE".len());
    at = spaces(bytes, name_end(bytes, at));
    if bytes
        .get(at..)
        .is_some_and(|tail| tail.starts_with(b"SYSTEM") || tail.starts_with(b"PUBLIC"))
    {
        unread.external_subset = true;
        at = spaces(bytes, external_id_end(bytes, at)?);
    }
    match bytes.get(at) {
        Some(b'>') => return Ok((at + 1, unread)),
        Some(b'[') => at += 1,
        _ => return Err("the DOCTYPE has no internal subset or end"),
    }
    loop {
        at = spaces(bytes, at);
        let tail = bytes
            .get(at..)
            .filter(|tail| !tail.is_empty())
            .ok_or(OPEN)?;
        if tail.starts_with(b"<!ENTITY") {
            at = spaces(bytes, at + b"<!ENTITY".len());
            if bytes.get(at) == Some(&b'%') {
                at = spaces(bytes, at + 1);
            }
            let name_start = at;
            at = name_end(bytes, at);
            let name = bytes.get(name_start..at).unwrap_or_default();
            if name.is_empty() {
                return Err("an entity declaration has no name");
            }
            at = spaces(bytes, at);
            let value = if matches!(bytes.get(at), Some(b'"' | b'\'')) {
                let end = literal_end(bytes, at)?;
                let value = at + 1..end - 1;
                at = end;
                Some(value)
            } else {
                at = spaces(bytes, external_id_end(bytes, at)?);
                if bytes
                    .get(at..)
                    .is_some_and(|tail| tail.starts_with(b"NDATA"))
                {
                    at = name_end(bytes, spaces(bytes, at + b"NDATA".len()));
                }
                None
            };
            declare(Declared { name, value });
            at = spaces(bytes, at);
            if bytes.get(at) != Some(&b'>') {
                return Err("an entity declaration does not close");
            }
            at += 1;
        } else if tail.starts_with(b"<!--") {
            at = find(bytes, at + 4, b"-->").ok_or(OPEN)? + 3;
        } else if tail.starts_with(b"<?") {
            at = find(bytes, at + 2, b"?>").ok_or(OPEN)? + 2;
        } else if tail.starts_with(b"]") {
            at = spaces(bytes, at + 1);
            if bytes.get(at) != Some(&b'>') {
                return Err("the DOCTYPE's internal subset does not close");
            }
            return Ok((at + 1, unread));
        } else if [&b"<!ELEMENT"[..], b"<!ATTLIST", b"<!NOTATION"]
            .iter()
            .any(|keyword| tail.starts_with(keyword))
        {
            let close = find(bytes, at, b">").ok_or(OPEN)?;
            if tail.starts_with(b"<!ATTLIST")
                && bytes
                    .get(at..close)
                    .is_some_and(|text| text.iter().any(|byte| matches!(byte, b'"' | b'\'')))
            {
                unread.attribute_defaults = true;
            }
            at = close + 1;
        } else {
            return Err("the DOCTYPE's internal subset holds an unknown declaration");
        }
    }
}

/// The entity name of a general reference at the start of `tail`, or none
/// for a character reference, a predefined entity or no reference.
pub(super) fn reference(tail: &[u8]) -> Option<&[u8]> {
    let rest = tail.strip_prefix(b"&")?;
    let length = rest.iter().position(|byte| {
        matches!(
            byte,
            b';' | b'&' | b'<' | b'>' | b'"' | b'\'' | b' ' | b'\t' | b'\n' | b'\r'
        )
    })?;
    let name = rest.get(..length)?;
    let predefined = matches!(name, b"lt" | b"gt" | b"amp" | b"apos" | b"quot");
    (rest.get(length) == Some(&b';') && !name.is_empty() && !name.starts_with(b"#") && !predefined)
        .then_some(name)
}

/// A text's DOCTYPE, its entities and the references to them.
pub(super) struct Doctype<'text> {
    /// The DOCTYPE's range in the text.
    pub(super) range: Range<usize>,
    unread: Unread,
    declared: Vec<Declared<'text>>,
    /// Declarations sorted by name, the first of each name.
    names: Vec<usize>,
    roots: Vec<Roots>,
    /// Bytes this analysis holds.
    analysis: u64,
}

impl<'text> Doctype<'text> {
    /// Reads the DOCTYPE of `text`, if it has one, after `admit` accepts the
    /// bytes the analysis holds.
    pub(super) fn read<E>(
        text: &'text str,
        admit: impl FnOnce(u64) -> Result<(), E>,
    ) -> Result<Option<Self>, Stop<E>> {
        let bytes = text.as_bytes();
        let Some(start) = locate(bytes) else {
            return Ok(None);
        };
        let mut entities = 0_usize;
        let (end, unread) = walk(bytes, start, |_| entities += 1)
            .map_err(|reason| Stop::Malformed(reason.to_owned()))?;
        let references =
            memchr::memchr_iter(b'&', bytes.get(start..end).unwrap_or_default()).count();
        let per_entity = size_of::<Declared<'_>>()
            + size_of::<usize>()
            + size_of::<Roots>()
            + size_of::<Plan>()
            + size_of::<(usize, usize)>();
        let analysis = u64_from_index(entities)
            .checked_mul(u64_from_index(per_entity))
            .and_then(|bytes| {
                bytes.checked_add(
                    u64_from_index(references).checked_mul(u64_from_index(size_of::<usize>()))?,
                )
            })
            .ok_or(Stop::Unbounded)?;
        admit(analysis).map_err(Stop::Refused)?;
        let mut declared = Vec::with_capacity(entities);
        walk(bytes, start, |entity| declared.push(entity))
            .map_err(|reason| Stop::Malformed(reason.to_owned()))?;
        let mut names = (0..declared.len()).collect::<Vec<_>>();
        names.sort_unstable_by(|&left, &right| {
            declared[left]
                .name
                .cmp(declared[right].name)
                .then(left.cmp(&right))
        });
        names.dedup_by(|later, earlier| declared[*later].name == declared[*earlier].name);
        Ok(Some(Self {
            range: start..end,
            unread,
            roots: vec![Roots::default(); declared.len()],
            declared,
            names,
            analysis,
        }))
    }

    /// Bytes this analysis holds.
    pub(super) fn analysis(&self) -> u64 {
        self.analysis
    }

    fn find(&self, name: &[u8]) -> Option<usize> {
        self.names
            .binary_search_by(|&index| self.declared[index].name.cmp(name))
            .ok()
            .and_then(|at| self.names.get(at).copied())
    }

    /// Counts a reference of the document, in text or in a quoted value.
    pub(super) fn refer(&mut self, name: &[u8], in_text: bool) {
        if let Some(index) = self.find(name) {
            let roots = &mut self.roots[index];
            if in_text {
                roots.text += 1;
            } else {
                roots.value += 1;
            }
        }
    }

    /// What the document's references expand to. `check` and
    /// `admit_prefix` run as the document scan runs them, over each entity
    /// value, and the values' namespace prefixes join `prefixes`.
    pub(super) fn expand<E>(
        &self,
        text: &'text str,
        prefixes: &mut HashSet<&'text [u8]>,
        mut check: impl FnMut() -> Result<(), E>,
        mut admit_prefix: impl FnMut(u64) -> Result<(), E>,
    ) -> Result<Expanded, Stop<E>> {
        let bytes = text.as_bytes();
        let capacity =
            memchr::memchr_iter(b'&', bytes.get(self.range.clone()).unwrap_or_default()).count();
        let mut references = Vec::with_capacity(capacity);
        let mut plans = Vec::with_capacity(self.declared.len());
        for entity in &self.declared {
            let first = references.len();
            let (counts, bytes_held) = match &entity.value {
                Some(value) => {
                    for at in value.clone() {
                        if let Some(index) = bytes
                            .get(at..value.end)
                            .and_then(reference)
                            .and_then(|name| self.find(name))
                        {
                            references.push(index);
                        }
                    }
                    let scope = XmlScope {
                        range: value.clone(),
                        skip: 0..0,
                        open: 1,
                    };
                    let counts = xml_counts_checked(
                        text,
                        &scope,
                        prefixes,
                        &mut check,
                        &mut admit_prefix,
                        |_, _| {},
                    )
                    .map_err(Stop::Refused)?;
                    (counts, u64_from_index(value.len()))
                }
                None => (
                    XmlCounts {
                        balanced: true,
                        ..XmlCounts::default()
                    },
                    0,
                ),
            };
            plans.push(Plan {
                counts,
                references: (first, references.len()),
                bytes: bytes_held,
                nested: 0,
                levels: 1,
                depth: counts.depth.max(2) - 2,
                external: u64::from(entity.value.is_none()),
                visit: Visit::New,
            });
        }
        let mut stack = Vec::with_capacity(self.declared.len());
        for (root, roots) in self.roots.iter().enumerate() {
            if roots.text == 0 && roots.value == 0 {
                continue;
            }
            self.fold(root, &references, &mut plans, &mut stack)?;
        }
        let names = self
            .declared
            .iter()
            .try_fold(u64_from_index(self.declared.len()), |sum, entity| {
                sum.checked_add(u64_from_index(entity.name.len()))
            })
            .ok_or(Stop::Unbounded)?;
        let mut expanded = Expanded {
            counts: XmlCounts::default(),
            expansion: XmlExpansion {
                declared: u64_from_index(self.declared.len()),
                analysis: self.analysis,
                ..XmlExpansion::default()
            },
            depth: 0,
            unread: self.unread,
        };
        let mut levels = 0;
        for ((roots, plan), entity) in self.roots.iter().zip(&plans).zip(&self.declared) {
            let count = roots.text.checked_add(roots.value).ok_or(Stop::Unbounded)?;
            if count == 0 {
                continue;
            }
            if roots.text != 0 && !plan.counts.balanced {
                return Err(Stop::Malformed(format!(
                    "entity {:?} expands to content that does not nest",
                    String::from_utf8_lossy(entity.name)
                )));
            }
            let mut counts = expanded
                .counts
                .plus(&plan.counts, count)
                .ok_or(Stop::Unbounded)?;
            let expansions = plan.nested.checked_add(1).ok_or(Stop::Unbounded)?;
            let scaled = |value: u64| value.checked_mul(count);
            counts.copied = counts
                .copied
                .checked_add(scaled(plan.bytes).ok_or(Stop::Unbounded)?)
                .ok_or(Stop::Unbounded)?;
            let references = scaled(expansions).ok_or(Stop::Unbounded)?;
            counts.copies = counts
                .copies
                .checked_add(references)
                .ok_or(Stop::Unbounded)?;
            expanded.counts = counts;
            expanded.expansion.references = expanded
                .expansion
                .references
                .checked_add(references)
                .ok_or(Stop::Unbounded)?;
            expanded.unread.external_references = expanded
                .unread
                .external_references
                .checked_add(scaled(plan.external).ok_or(Stop::Unbounded)?)
                .ok_or(Stop::Unbounded)?;
            expanded.depth = expanded.depth.max(plan.depth);
            levels = plan.levels.max(levels);
        }
        expanded.depth = expanded.depth.checked_add(levels).ok_or(Stop::Unbounded)?;
        expanded.expansion.lookups = expanded
            .expansion
            .references
            .checked_mul(names)
            .ok_or(Stop::Unbounded)?;
        expanded.expansion.attributes = expanded.counts.attributes != 0;
        Ok(expanded)
    }

    /// Completes the plan of `root` and of each entity it reaches, after
    /// their references, in post-order.
    fn fold<E>(
        &self,
        root: usize,
        references: &[usize],
        plans: &mut [Plan],
        stack: &mut Vec<(usize, usize)>,
    ) -> Result<(), Stop<E>> {
        if plans[root].visit != Visit::New {
            return Ok(());
        }
        plans[root].visit = Visit::Open;
        stack.push((root, plans[root].references.0));
        while let Some(&mut (entity, ref mut next)) = stack.last_mut() {
            let (_, last) = plans[entity].references;
            if *next < last {
                let target = references[*next];
                *next += 1;
                match plans[target].visit {
                    Visit::New => {
                        plans[target].visit = Visit::Open;
                        stack.push((target, plans[target].references.0));
                    }
                    Visit::Open => {
                        return Err(Stop::Malformed(format!(
                            "entity {:?} refers to itself",
                            String::from_utf8_lossy(self.declared[target].name)
                        )))
                    }
                    Visit::Done => {}
                }
                continue;
            }
            stack.pop();
            let mut plan = plans[entity];
            let (first, last) = plan.references;
            let mut deepest = 0;
            let mut levels = 0;
            for &target in references.get(first..last).unwrap_or_default() {
                let nested = plans[target];
                plan.counts = plan.counts.plus(&nested.counts, 1).ok_or(Stop::Unbounded)?;
                plan.bytes = plan
                    .bytes
                    .checked_add(nested.bytes)
                    .ok_or(Stop::Unbounded)?;
                plan.nested = nested
                    .nested
                    .checked_add(1)
                    .and_then(|count| plan.nested.checked_add(count))
                    .ok_or(Stop::Unbounded)?;
                plan.external = plan
                    .external
                    .checked_add(nested.external)
                    .ok_or(Stop::Unbounded)?;
                deepest = nested.depth.max(deepest);
                levels = nested.levels.max(levels);
            }
            plan.depth = plan.depth.checked_add(deepest).ok_or(Stop::Unbounded)?;
            plan.levels = levels + 1;
            plan.visit = Visit::Done;
            plans[entity] = plan;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
