// SPDX-License-Identifier: Apache-2.0
//! Encodes Part 21 DATA instances.
//!
//! The emitter allocates instance names, formats scalar values, counts entity
//! types, and interns repeated points and directions.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::rc::Rc;

use crate::options::StepSchema;

pub(crate) mod target;

/// A STEP instance name such as `#42`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Ref(pub u64);

impl std::fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// The text [`Emitter::real`] writes for a non-finite number. No Part 21
/// token holds a NUL character (strings write control characters as `\X\`
/// escapes), so a line that contains it holds a value Part 21 cannot state.
const NON_FINITE: &str = "\0";

/// A DATA instance [`Emitter::drop_non_finite`] left out of the section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DroppedInstance {
    /// The instance name the instance would have had.
    pub(crate) name: Ref,
    /// Its entity-count key.
    pub(crate) tally: &'static str,
    /// The document record it was written for ([`Emitter::write_for`]).
    pub(crate) record: Option<Rc<str>>,
}

/// A point in the emission that [`Emitter::take_non_finite_since`] reads
/// back from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NonFiniteMark {
    /// The length of [`Emitter::non_finite`] at the mark.
    non_finite: usize,
    /// The number of instances emitted before the mark.
    line: usize,
}

/// Accumulates DATA instances in allocation order and counts their entity types.
pub(crate) struct Emitter {
    schema: StepSchema,
    /// Each emitted instance, with its entity-count key; a dropped one is
    /// `None`.
    lines: Vec<Option<(String, &'static str)>>,
    /// The document record each instance in `lines` was written for.
    records: Vec<Option<Rc<str>>>,
    /// The record [`Self::write_for`] names for the instances emitted next.
    record: Option<Rc<str>>,
    counts: BTreeMap<&'static str, usize>,
    /// Leaf instances keyed by encoded type and parameters.
    interned: HashMap<String, Ref>,
    /// The index in `lines` of each instance that holds a non-finite number,
    /// in emission order, with whether a [`Self::take_non_finite_since`]
    /// caller answered for it.
    non_finite: Vec<(usize, bool)>,
    /// The index ranges in `lines` of the instances emitted for a carrier
    /// that a [`Self::take_non_finite_since`] caller discarded.
    discarded: Vec<std::ops::Range<usize>>,
}

impl Emitter {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::for_schema(StepSchema::Ap214)
    }

    pub(crate) fn for_schema(schema: StepSchema) -> Self {
        Emitter {
            schema,
            lines: Vec::new(),
            records: Vec::new(),
            record: None,
            counts: BTreeMap::new(),
            interned: HashMap::new(),
            non_finite: Vec::new(),
            discarded: Vec::new(),
        }
    }

    pub(crate) fn operator_2d_entity(&self) -> &'static str {
        match self.schema {
            StepSchema::Ap203Edition1 => "CARTESIAN_TRANSFORMATION_OPERATOR",
            _ => "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        }
    }

    /// Format `v` as a Part 21 real literal.
    ///
    /// A Part 21 real states a finite number only. For a non-finite `v` this
    /// returns a marker, and the instance whose parameters hold it is left
    /// out of the section with every instance that references it
    /// ([`Self::drop_non_finite`]).
    pub(crate) fn real(v: f64) -> String {
        part21_real(v).unwrap_or_else(|| NON_FINITE.to_owned())
    }

    /// Names `record` as the document record the instances emitted next are
    /// written for, until the next call.
    pub(crate) fn write_for(&mut self, record: Option<&str>) {
        self.record = record.map(Rc::from);
    }

    /// A mark for [`Self::take_non_finite_since`].
    pub(crate) fn non_finite_mark(&self) -> NonFiniteMark {
        NonFiniteMark {
            non_finite: self.non_finite.len(),
            line: self.lines.len(),
        }
    }

    /// Whether an instance emitted since `mark` holds a non-finite number
    /// that no nested caller answered for. Every such instance is answered
    /// for from now on, so an enclosing caller does not answer for it again.
    ///
    /// A caller that gets `true` discards what it emitted since `mark`:
    /// [`Self::drop_non_finite`] leaves out each of those instances that no
    /// instance in the section references.
    pub(crate) fn take_non_finite_since(&mut self, mark: NonFiniteMark) -> bool {
        let mut fresh = false;
        for (_, answered) in self.non_finite.iter_mut().skip(mark.non_finite) {
            fresh |= !*answered;
            *answered = true;
        }
        if fresh {
            self.discarded.push(mark.line..self.lines.len());
        }
        fresh
    }

    fn push(&mut self, tally: &'static str, line: String) -> Ref {
        let index = self.lines.len();
        if line.contains(NON_FINITE) {
            self.non_finite.push((index, false));
        }
        self.lines.push(Some((line, tally)));
        self.records.push(self.record.clone());
        *self.counts.entry(tally).or_insert(0) += 1;
        Ref(cadmpeg_core::decode::u64_from_index(index) + 1)
    }

    /// Append `#id = TYPE(params);` and return the allocated reference.
    ///
    /// `type_` is also the entity-count key. Complex instances use their leading
    /// keyword as the key.
    pub(crate) fn emit(&mut self, type_: &'static str, params: &str) -> Ref {
        let id = cadmpeg_core::decode::u64_from_index(self.lines.len()) + 1;
        self.push(type_, format!("#{id} = {type_}({params});"))
    }

    /// Append a preformatted entity or complex-instance body.
    ///
    /// `tally` supplies its entity-count key.
    pub(crate) fn emit_raw(&mut self, tally: &'static str, body: &str) -> Ref {
        let id = cadmpeg_core::decode::u64_from_index(self.lines.len()) + 1;
        self.push(tally, format!("#{id} = {body};"))
    }

    /// Emit a value-like leaf or reuse an identical encoded instance. A leaf
    /// that holds a non-finite number is emitted each time and never reused.
    pub(crate) fn emit_interned(&mut self, type_: &'static str, params: &str) -> Ref {
        if params.contains(NON_FINITE) {
            return self.emit(type_, params);
        }
        let key = format!("{type_}|{params}");
        if let Some(r) = self.interned.get(&key) {
            return *r;
        }
        let r = self.emit(type_, params);
        self.interned.insert(key, r);
        r
    }

    /// The number of instances emitted so far.
    pub(crate) fn instance_count(&self) -> usize {
        self.lines.len()
    }

    /// Whether the section holds the instance `reference`.
    pub(crate) fn holds(&self, reference: Ref) -> bool {
        usize::try_from(reference.0)
            .ok()
            .and_then(|name| name.checked_sub(1))
            .and_then(|index| self.lines.get(index))
            .is_some_and(Option::is_some)
    }

    /// Whether the section holds an instance emitted after the first
    /// `count`.
    pub(crate) fn holds_after(&self, count: usize) -> bool {
        self.lines.iter().skip(count).any(Option::is_some)
    }

    pub(crate) fn counts(&self) -> BTreeMap<String, usize> {
        self.counts
            .iter()
            .map(|(type_, count)| ((*type_).to_string(), *count))
            .collect()
    }

    /// Leaves out of the section every instance that holds a non-finite
    /// number and every instance that needs one left out, as a record that
    /// needs a dropped record is dropped (CADIR §3.3); a set that lists one
    /// keeps its other members ([`cascade`]). Then it leaves out each
    /// instance a discarded carrier emitted that no instance left in the
    /// section references. The others keep their instance names; Part 21
    /// does not require them to be consecutive. Returns the instances left
    /// out whose non-finite number no [`Self::take_non_finite_since`] caller
    /// answered for, with those that need them: a caller that answered for
    /// one discarded what it wrote, so no kept instance references it.
    pub(crate) fn drop_non_finite(&mut self) -> Vec<DroppedInstance> {
        if self.non_finite.is_empty() {
            return Vec::new();
        }
        let all = self.non_finite.iter().map(|(index, _)| *index);
        let unanswered = self
            .non_finite
            .iter()
            .filter(|(_, answered)| !*answered)
            .map(|(index, _)| *index);
        let reported = cascade(&self.lines, unanswered).dropped;
        let Cascade { dropped, kept } = cascade(&self.lines, all);
        self.non_finite.clear();
        for (index, line) in kept {
            if let Some((held, _)) = self.lines.get_mut(index).and_then(Option::as_mut) {
                *held = line;
            }
        }
        let mut out = Vec::new();
        for index in dropped {
            let Some(tally) = self.remove(index) else {
                continue;
            };
            if reported.contains(&index) {
                out.push(DroppedInstance {
                    name: Ref(cadmpeg_core::decode::u64_from_index(index) + 1),
                    tally,
                    record: self.records.get(index).cloned().flatten(),
                });
            }
        }
        self.prune_discarded();
        out
    }

    /// Leaves out each instance in a discarded range that no instance left
    /// in the section references. An instance references only instances
    /// emitted before it, so one pass from the last instance back sees every
    /// reference to an instance before it reaches that instance.
    fn prune_discarded(&mut self) {
        let mut discarded = vec![false; self.lines.len()];
        for range in std::mem::take(&mut self.discarded) {
            for flag in &mut discarded[range] {
                *flag = true;
            }
        }
        let mut referenced = std::collections::BTreeSet::new();
        for index in (0..self.lines.len()).rev() {
            let Some((line, _)) = &self.lines[index] else {
                continue;
            };
            if discarded[index] && !referenced.contains(&index) {
                self.remove(index);
            } else {
                referenced.extend(instance_references(line));
            }
        }
    }

    /// Leaves the instance at `index` out of the section and its entity
    /// count, and returns its entity-count key.
    fn remove(&mut self, index: usize) -> Option<&'static str> {
        let (_, tally) = self.lines.get_mut(index)?.take()?;
        if let Some(count) = self.counts.get_mut(tally) {
            *count -= 1;
            if *count == 0 {
                self.counts.remove(tally);
            }
        }
        Some(tally)
    }

    /// Consume the emitter and return one encoded DATA instance per element,
    /// without the instances [`Self::drop_non_finite`] leaves out.
    pub(crate) fn into_lines(mut self) -> Vec<String> {
        self.drop_non_finite();
        self.lines
            .into_iter()
            .flatten()
            .map(|(line, _)| line)
            .collect()
    }
}

/// What leaving the instances at some seeds out of the section does to the
/// instances emitted after them.
struct Cascade {
    /// The seeds and every instance that needs one, directly or through
    /// other such instances.
    dropped: BTreeSet<usize>,
    /// The instances that list a dropped instance as a member of a set and
    /// keep another member, with the text that leaves it out.
    kept: BTreeMap<usize, String>,
}

/// The instances that need `seeds`. An instance whose type [`lists_members`]
/// stays in the section without them while each list keeps a member; any
/// other instance that references one needs it.
///
/// An instance references only instances emitted before it, so one pass in
/// emission order finds them all.
fn cascade(
    lines: &[Option<(String, &'static str)>],
    seeds: impl Iterator<Item = usize>,
) -> Cascade {
    let mut dropped: BTreeSet<usize> = seeds.collect();
    let mut kept = BTreeMap::new();
    let Some(&first) = dropped.first() else {
        return Cascade { dropped, kept };
    };
    for (index, line) in lines.iter().enumerate().skip(first + 1) {
        let Some((line, tally)) = line else {
            continue;
        };
        if dropped.contains(&index)
            || !instance_references(line)
                .iter()
                .any(|referenced| dropped.contains(referenced))
        {
            continue;
        }
        match lists_members(tally)
            .then(|| without_members(line, &dropped))
            .flatten()
        {
            Some(line) => {
                kept.insert(index, line);
            }
            None => {
                dropped.insert(index);
            }
        }
    }
    Cascade { dropped, kept }
}

/// Whether instances of the entity type `tally` list the members of a set
/// that holds without any one of them: the items of a representation, the
/// elements of a geometric set, and the items a layer or an invisibility
/// assigns.
fn lists_members(tally: &str) -> bool {
    (tally.ends_with("_REPRESENTATION") && !tally.ends_with("DEFINITION_REPRESENTATION"))
        || matches!(
            tally,
            "DRAUGHTING_MODEL"
                | "GEOMETRIC_SET"
                | "GEOMETRIC_CURVE_SET"
                | "PRESENTATION_LAYER_ASSIGNMENT"
                | "INVISIBILITY"
        )
}

/// The DATA line `line` with each reference to a `dropped` instance taken
/// out of the aggregate that lists it, or `None` when a parameter outside an
/// aggregate references one or an aggregate loses its last member.
fn without_members(line: &str, dropped: &BTreeSet<usize>) -> Option<String> {
    let (name, parameters) = line.split_once('=')?;
    let mut out = String::with_capacity(line.len());
    out.push_str(name);
    out.push('=');
    // The position in `out` of each open parenthesis, with whether a member
    // left its aggregate.
    let mut open: Vec<(usize, bool)> = Vec::new();
    let mut quoted = false;
    let mut characters = parameters.char_indices().peekable();
    while let Some((at, character)) = characters.next() {
        match character {
            '\'' => {
                quoted = !quoted;
                out.push(character);
            }
            _ if quoted => out.push(character),
            '(' => {
                open.push((out.len(), false));
                out.push(character);
            }
            ')' => {
                let (start, removed) = open.pop()?;
                if removed && out.len() == start + 1 {
                    return None;
                }
                out.push(character);
            }
            '#' => {
                let digits = parameters[at + 1..]
                    .bytes()
                    .take_while(u8::is_ascii_digit)
                    .count();
                let index = parameters[at + 1..at + 1 + digits]
                    .parse::<usize>()
                    .ok()
                    .and_then(|name| name.checked_sub(1));
                if !index.is_some_and(|index| dropped.contains(&index)) {
                    out.push(character);
                    continue;
                }
                // The entity's own parameter list is the first parenthesis;
                // an aggregate parameter is nested in it.
                if open.len() < 2 {
                    return None;
                }
                let (_, removed) = open.last_mut()?;
                *removed = true;
                for _ in 0..digits {
                    characters.next();
                }
                if characters.peek().is_some_and(|(_, next)| *next == ',') {
                    characters.next();
                } else if out.ends_with(',') {
                    out.pop();
                }
            }
            _ => out.push(character),
        }
    }
    Some(out)
}

/// The line index of each instance name `#n` that the parameters of the
/// DATA line `line` reference, outside its string literals.
fn instance_references(line: &str) -> Vec<usize> {
    let parameters = line.split_once('=').map_or("", |(_, rest)| rest);
    let mut quoted = false;
    let mut out = Vec::new();
    let bytes = parameters.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\'' => quoted = !quoted,
            b'#' if !quoted => {
                let digits = bytes[at + 1..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                if let Some(index) = parameters[at + 1..at + 1 + digits]
                    .parse::<usize>()
                    .ok()
                    .and_then(|name| name.checked_sub(1))
                {
                    out.push(index);
                }
                at += digits;
            }
            _ => {}
        }
        at += 1;
    }
    out
}

/// Format a finite `f64` as a Part 21 real literal, or `None` for a
/// non-finite one.
///
/// The result always contains a decimal point, including scientific notation.
fn part21_real(v: f64) -> Option<String> {
    if !v.is_finite() {
        return None;
    }
    // Shortest round-tripping decimal, then normalize to Part 21 lexical rules.
    let mut s = format!("{v}");
    if let Some(e_pos) = s.find(['e', 'E']) {
        // Scientific: ensure the mantissa has a decimal point and use uppercase E.
        let (mantissa, exp) = s.split_at(e_pos);
        let exp = &exp[1..];
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.")
        };
        let exp = if let Some(rest) = exp.strip_prefix('-') {
            format!("-{rest}")
        } else {
            exp.strip_prefix('+').unwrap_or(exp).to_string()
        };
        return Some(format!("{mantissa}E{exp}"));
    }
    if !s.contains('.') {
        s.push('.');
    }
    Some(s)
}

/// Encode a Rust string as a Part 21 single-quoted string literal.
///
/// Apostrophes are doubled. Non-ASCII and control characters use
/// `\X2\..\X0\` or `\X4\..\X0\` hexadecimal notation, keeping the encoded file 7-bit.
pub(crate) fn string(s: &str) -> String {
    format!("'{}'", crate::strings::encode(s))
}

/// Join instance references into a Part 21 aggregate such as `(#1,#2,#3)`.
pub(crate) fn refs(items: &[Ref]) -> String {
    let mut out = String::from("(");
    for (i, r) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('#');
        out.push_str(&r.0.to_string());
    }
    out.push(')');
    out
}

#[cfg(test)]
pub(crate) mod tests;
