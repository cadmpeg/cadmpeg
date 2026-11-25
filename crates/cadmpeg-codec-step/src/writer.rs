// SPDX-License-Identifier: Apache-2.0
//! Encodes Part 21 DATA instances.
//!
//! The emitter allocates instance names, formats scalar values, counts entity
//! types, and interns repeated points and directions.

use std::collections::BTreeMap;
use std::collections::HashMap;

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
}

/// Accumulates DATA instances in allocation order and counts their entity types.
pub(crate) struct Emitter {
    schema: StepSchema,
    /// Each emitted instance, with its entity-count key; a dropped one is
    /// `None`.
    lines: Vec<Option<(String, &'static str)>>,
    counts: BTreeMap<&'static str, usize>,
    /// Leaf instances keyed by encoded type and parameters.
    interned: HashMap<String, Ref>,
    /// The index in `lines` of each instance that holds a non-finite number,
    /// in emission order, with whether a [`Self::take_non_finite_since`]
    /// caller answered for it.
    non_finite: Vec<(usize, bool)>,
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
            counts: BTreeMap::new(),
            interned: HashMap::new(),
            non_finite: Vec::new(),
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

    /// A mark for [`Self::take_non_finite_since`].
    pub(crate) fn non_finite_mark(&self) -> usize {
        self.non_finite.len()
    }

    /// Whether an instance emitted since `mark` holds a non-finite number
    /// that no nested caller answered for. Every such instance is answered
    /// for from now on, so an enclosing caller does not answer for it again.
    pub(crate) fn take_non_finite_since(&mut self, mark: usize) -> bool {
        let mut fresh = false;
        for (_, answered) in self.non_finite.iter_mut().skip(mark) {
            fresh |= !*answered;
            *answered = true;
        }
        fresh
    }

    fn push(&mut self, tally: &'static str, line: String) -> Ref {
        let index = self.lines.len();
        if line.contains(NON_FINITE) {
            self.non_finite.push((index, false));
        }
        self.lines.push(Some((line, tally)));
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

    pub(crate) fn counts(&self) -> BTreeMap<String, usize> {
        self.counts
            .iter()
            .map(|(type_, count)| ((*type_).to_string(), *count))
            .collect()
    }

    /// Leaves out of the section every instance that holds a non-finite
    /// number and every instance that references one left out, as a record
    /// that needs a dropped record is dropped (CADIR §3.3). The others keep
    /// their instance names; Part 21 does not require them to be
    /// consecutive. Returns the instances left out whose non-finite number
    /// no [`Self::take_non_finite_since`] caller answered for, with those
    /// that need them: a caller that answered for one discarded what it
    /// wrote, so no kept instance references it.
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
        let dropped = dependents(&self.lines, all);
        let reported = dependents(&self.lines, unanswered);
        self.non_finite.clear();
        let mut out = Vec::new();
        for index in dropped {
            let Some((_, tally)) = self.lines[index].take() else {
                continue;
            };
            if let Some(count) = self.counts.get_mut(tally) {
                *count -= 1;
                if *count == 0 {
                    self.counts.remove(tally);
                }
            }
            if reported.contains(&index) {
                out.push(DroppedInstance {
                    name: Ref(cadmpeg_core::decode::u64_from_index(index) + 1),
                    tally,
                });
            }
        }
        out
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

/// The indices of `seeds` and of every line that references one of them,
/// directly or through other such lines.
///
/// An instance references only instances emitted before it, so one pass in
/// emission order finds them all.
fn dependents(
    lines: &[Option<(String, &'static str)>],
    seeds: impl Iterator<Item = usize>,
) -> std::collections::BTreeSet<usize> {
    let mut out: std::collections::BTreeSet<usize> = seeds.collect();
    let Some(&first) = out.first() else {
        return out;
    };
    for (index, line) in lines.iter().enumerate().skip(first + 1) {
        let Some((line, _)) = line else {
            continue;
        };
        if instance_references(line)
            .iter()
            .any(|referenced| out.contains(referenced))
        {
            out.insert(index);
        }
    }
    out
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
