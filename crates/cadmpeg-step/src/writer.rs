// SPDX-License-Identifier: Apache-2.0
//! Encodes Part 21 DATA instances.
//!
//! The emitter allocates instance names, formats scalar values, counts entity
//! types, and interns repeated points and directions.

use std::collections::BTreeMap;
use std::collections::HashMap;

/// A STEP instance name such as `#42`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ref(pub u64);

impl std::fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Accumulates DATA instances in allocation order and counts their entity types.
pub struct Emitter {
    next: u64,
    lines: Vec<String>,
    counts: BTreeMap<&'static str, usize>,
    /// Leaf instances keyed by encoded type and parameters.
    interned: HashMap<String, Ref>,
}

impl Emitter {
    pub fn new() -> Self {
        Emitter {
            next: 1,
            lines: Vec::new(),
            counts: BTreeMap::new(),
            interned: HashMap::new(),
        }
    }

    /// Append `#id = TYPE(params);` and return the allocated reference.
    ///
    /// `type_` is also the entity-count key. Complex instances use their leading
    /// keyword as the key.
    pub fn emit(&mut self, type_: &'static str, params: &str) -> Ref {
        let id = self.next;
        self.next += 1;
        self.lines.push(format!("#{id} = {type_}({params});"));
        *self.counts.entry(type_).or_insert(0) += 1;
        Ref(id)
    }

    /// Append a preformatted entity or complex-instance body.
    ///
    /// `tally` supplies its entity-count key.
    pub fn emit_raw(&mut self, tally: &'static str, body: &str) -> Ref {
        let id = self.next;
        self.next += 1;
        self.lines.push(format!("#{id} = {body};"));
        *self.counts.entry(tally).or_insert(0) += 1;
        Ref(id)
    }

    /// Emit a value-like leaf or reuse an identical encoded instance.
    pub fn emit_interned(&mut self, type_: &'static str, params: &str) -> Ref {
        let key = format!("{type_}|{params}");
        if let Some(r) = self.interned.get(&key) {
            return *r;
        }
        let r = self.emit(type_, params);
        self.interned.insert(key, r);
        r
    }

    pub fn counts(&self) -> BTreeMap<String, usize> {
        self.counts
            .iter()
            .map(|(type_, count)| ((*type_).to_string(), *count))
            .collect()
    }

    pub fn total(&self) -> usize {
        self.lines.len()
    }

    /// Consume the emitter and return one encoded DATA instance per element.
    pub fn into_lines(self) -> Vec<String> {
        self.lines
    }
}

/// Format an `f64` as a Part 21 real literal.
///
/// The result always contains a decimal point, including scientific notation.
/// Non-finite inputs become `0.`.
pub fn real(v: f64) -> String {
    if !v.is_finite() {
        return "0.".to_string();
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
        return format!("{mantissa}E{exp}");
    }
    if !s.contains('.') {
        s.push('.');
    }
    s
}

/// Encode a Rust string as a Part 21 single-quoted string literal.
///
/// Apostrophes are doubled. Non-ASCII and control characters use
/// `\X2\..\X0\` or `\X4\..\X0\` hexadecimal notation, keeping the encoded file 7-bit.
pub fn string(s: &str) -> String {
    format!("'{}'", crate::strings::encode(s))
}

/// Join instance references into a Part 21 aggregate such as `(#1,#2,#3)`.
pub fn refs(items: &[Ref]) -> String {
    use std::fmt::Write as _;

    let mut out = String::from("(");
    for (i, r) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write!(out, "{r}").expect("writing to a String cannot fail");
    }
    out.push(')');
    out
}
