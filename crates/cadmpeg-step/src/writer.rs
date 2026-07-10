// SPDX-License-Identifier: Apache-2.0
//! Low-level Part 21 instance emitter: id allocation, real/string encoding, and
//! deduplication of leaf geometry (points and directions).

use std::collections::BTreeMap;
use std::collections::HashMap;

/// A STEP instance name (`#42`). Newtype so the graph builder cannot confuse an
/// instance id with an ordinary integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ref(pub u64);

impl std::fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Accumulates DATA-section instances, handing out sequential ids and tallying
/// how many of each entity type were written (for the export report).
pub struct Emitter {
    next: u64,
    lines: Vec<String>,
    counts: BTreeMap<String, usize>,
    /// Interned leaf primitives, keyed by their fully-encoded parameter text, so
    /// repeated points/directions collapse to a single instance.
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

    /// Write one instance `#id = TYPE(params);` and return its reference. `type_`
    /// is the entity keyword used both in the output and as the report tally key;
    /// for a complex (AND-combined) instance pass the leading keyword to tally.
    pub fn emit(&mut self, type_: &str, params: &str) -> Ref {
        let id = self.next;
        self.next += 1;
        self.lines.push(format!("#{id} = {type_}({params});"));
        *self.counts.entry(type_.to_string()).or_insert(0) += 1;
        Ref(id)
    }

    /// Emit a raw pre-formatted instance body (already including `TYPE(...)` or a
    /// complex `( ... )` grouping). `tally` names it in the report.
    pub fn emit_raw(&mut self, tally: &str, body: &str) -> Ref {
        let id = self.next;
        self.next += 1;
        self.lines.push(format!("#{id} = {body};"));
        *self.counts.entry(tally.to_string()).or_insert(0) += 1;
        Ref(id)
    }

    /// Emit `type_`, reusing an existing instance when an identical one (same
    /// encoded params) was already interned. Use only for value-like leaves
    /// (`CARTESIAN_POINT`, `DIRECTION`) where sharing is always sound.
    pub fn emit_interned(&mut self, type_: &str, params: &str) -> Ref {
        let key = format!("{type_}|{params}");
        if let Some(r) = self.interned.get(&key) {
            return *r;
        }
        let r = self.emit(type_, params);
        self.interned.insert(key, r);
        r
    }

    pub fn counts(&self) -> &BTreeMap<String, usize> {
        &self.counts
    }

    pub fn total(&self) -> usize {
        self.lines.len()
    }

    /// The DATA-section body, one instance per line.
    pub fn into_lines(self) -> Vec<String> {
        self.lines
    }
}

/// Format an `f64` as a Part 21 real literal. A STEP real must always carry a
/// decimal point (`0.`, `1.`, `2.5`, `1.E-06`); Rust's default `f64` formatting
/// drops it for integral values, so we re-add it. Non-finite values are clamped
/// to `0.` — callers should have reported the loss before reaching here.
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

/// Encode a Rust string as a Part 21 single-quoted string literal. Apostrophes
/// double; non-ASCII and control characters use the `\X2\..\X0\` extended
/// notation so the file stays 7-bit clean per ISO 10303-21.
pub fn string(s: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("''");
        } else if ch.is_ascii() && !ch.is_ascii_control() {
            out.push(ch);
        } else {
            // Extended encoding: 4 hex digits per UTF-16 code unit, wrapped in
            // a single \X2\...\X0\ run.
            out.push_str("\\X2\\");
            let mut buf = [0u16; 2];
            for cu in ch.encode_utf16(&mut buf) {
                let _ = write!(out, "{cu:04X}");
            }
            out.push_str("\\X0\\");
        }
    }
    out.push('\'');
    out
}

/// Join instance references as a Part 21 aggregate: `(#1,#2,#3)`.
pub fn refs(items: &[Ref]) -> String {
    let mut out = String::from("(");
    for (i, r) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&r.to_string());
    }
    out.push(')');
    out
}
