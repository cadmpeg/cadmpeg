// SPDX-License-Identifier: Apache-2.0
//! Reading CADIR (§3.3, Reading CADIR).
//!
//! A read is a decode whose parts are the root's members and each record of
//! an arena or of a native namespace. Each part is read on its own: a
//! spelling or a value with one reading is stated in its canonical form and
//! reported, a member the reader does not know is reported and left out, and
//! every other part that breaks its type is given up with an `unknowns`
//! stub. The read refuses only what §3.3 lists.

mod loss;
mod repair;
mod text;

use std::collections::BTreeMap;
use std::fmt;

use cadmpeg_core::decode::{
    ArenaClass, ContentScope, DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceLimit,
    ResourceLimits,
};
use cadmpeg_core::read_session::{self, Repair, Repaired};
use cadmpeg_core::text::{CodecId, NonBlankString};
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde_json::Value;

use self::loss::ReadLossCode;
use self::text::{Found, Member, Scanner, Span};
use super::{one_zero, CadIr, Design, Model, Process, Topology};
use crate::bounds::MAX_DOCUMENT_DEPTH;
use crate::design::evaluation::Cycles;
use crate::native::{Native, NativeRecord};
use crate::report::loss::LossNote;
use crate::report::strictness::Strictness;
use crate::source_fidelity::SourceFidelity;
use crate::units::{CanonicalUnitsWire, Tolerances};
use crate::unknown::part::{record_unreadable_kind, DecodeParts, GivenUpPart, Part};

/// The refusals one record's repairs may take before the record is given up.
const MAX_REPAIRS: usize = 64;

/// The objects of a record tried as the home of one omitted collection.
const MAX_COLLECTION_SITES: usize = 16;

/// The root's members.
const ROOT_MEMBERS: &[&str] = &[
    "ir_version",
    "fragment",
    "repro",
    "scope",
    "source",
    "units",
    "tolerances",
    "geometry",
    "model",
    "design",
    "process",
    "native",
];

/// The caller's ceilings and strictness for one read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadOptions {
    /// The ceilings of the read (§2.5).
    pub limits: ResourceLimits,
    /// The report entries on which the caller refuses the read.
    pub strictness: Strictness,
}

/// A document read from CADIR text, with its report.
#[derive(Debug, Clone, PartialEq)]
pub struct CadirRead {
    /// The document.
    pub document: CadIr,
    /// What the read repaired, left out, and gave up.
    pub report: Vec<LossNote>,
}

/// Why a read states no document.
#[derive(Debug, thiserror::Error)]
pub enum ReadFailure {
    /// The input holds no JSON object.
    #[error("the input holds no JSON object: {0}")]
    Malformed(String),
    /// `ir_version` is not `"11"` (§3.6).
    #[error("{0}")]
    Version(String),
    /// `units` is not the canonical record (§2).
    #[error("units is not the canonical units record: {0}")]
    Units(String),
    /// A session ceiling of the read's limits ran out.
    #[error("{}", CodecError::ResourceLimit(*.0))]
    ResourceLimit(ResourceLimit),
    /// The input cannot be read.
    #[error("the input cannot be read: {0}")]
    Io(#[from] std::io::Error),
    /// The completed report holds an entry the caller's strictness refuses.
    #[error("the read report entry {entry} is refused by the caller's strictness")]
    StrictRejected {
        /// The completed report.
        report: Vec<LossNote>,
        /// The index of the first entry the strictness refuses.
        entry: usize,
    },
}

impl CadIr {
    /// Reads a document from its CADIR text (§3.3, Reading CADIR).
    ///
    /// # Errors
    ///
    /// Returns a [`ReadFailure`] for another `ir_version`, another `units`
    /// record, input that holds no JSON object, a session ceiling of
    /// `options.limits`, or a report entry `options.strictness` refuses.
    pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<CadirRead, ReadFailure> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy {
            limits: options.limits,
        };
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .map_err(|error| session_failure(&error))?;
        let mut reader = Reader {
            bytes,
            ctx: &ctx,
            limits: options.limits,
            report: Vec::new(),
            parts: Vec::new(),
            stop: None,
            refusal: None,
        };
        let document = reader.document()?;
        let report = reader.report;
        match options.strictness.first_refused(&report, false) {
            Some(entry) => Err(ReadFailure::StrictRejected { report, entry }),
            None => Ok(CadirRead { document, report }),
        }
    }
}

fn session_failure(error: &CodecError) -> ReadFailure {
    match error {
        CodecError::ResourceLimit(limit) => ReadFailure::ResourceLimit(ResourceLimit {
            scope: LimitScope::Session,
            ..*limit
        }),
        other => ReadFailure::Malformed(other.to_string()),
    }
}

/// One read in progress.
struct Reader<'r, 'a> {
    bytes: &'r [u8],
    ctx: &'r DecodeContext<'a>,
    limits: ResourceLimits,
    report: Vec<LossNote>,
    parts: Vec<GivenUpPart>,
    stop: Option<text::Stop>,
    refusal: Option<ResourceLimit>,
}

/// The members of one object, by the name the reader reads each under.
struct Members {
    named: BTreeMap<&'static str, Span>,
    /// Whether the object closes before the text stops being JSON.
    closed: bool,
    /// Whether the object holds a member the reader does not know.
    ignored: bool,
    /// The names given up because the object states them twice with
    /// different values.
    repeated: Vec<String>,
}

impl Members {
    fn take(&mut self, name: &str) -> Option<Span> {
        self.named.remove(name)
    }

    /// The members of an object the reader gave up or never opened.
    fn unread() -> Self {
        Self {
            named: BTreeMap::new(),
            closed: false,
            ignored: false,
            repeated: Vec::new(),
        }
    }
}

impl Reader<'_, '_> {
    fn note(&mut self, code: ReadLossCode, message: String) {
        self.report.push(code.note(message));
    }

    fn halted(&self) -> bool {
        self.refusal.is_some()
    }

    fn refuse(&mut self, limit: ResourceLimit) {
        self.refusal.get_or_insert(ResourceLimit {
            scope: LimitScope::Session,
            ..limit
        });
    }

    fn stopped(&mut self, stop: Option<text::Stop>) {
        if self.stop.is_none() {
            self.stop = stop;
        }
    }

    /// Gives up the text `span` frames as one part, and returns the loss the
    /// part charges.
    fn give_up_part(
        &mut self,
        span: Span,
        path: &str,
        classes: &[ArenaClass],
        error: impl fmt::Display,
    ) -> Option<LossNote> {
        let end = span.end.min(self.bytes.len());
        let start = span.start.min(end);
        let part = Part::at(
            "cadir",
            "record",
            cadmpeg_core::decode::u64_from_index(start),
            &self.bytes[start..end],
            classes.iter().copied(),
        )
        .named(path)
        .digest_only();
        match self.ctx.give_up_part(part, CodecError::malformed(error)) {
            Ok(given) => {
                let loss = given.loss();
                self.parts.push(given);
                Some(loss)
            }
            Err(CodecError::ResourceLimit(limit)) => {
                self.refuse(limit);
                None
            }
            Err(other) => Some(LossNote::new(
                record_unreadable_kind(),
                format!("{path}: {other}"),
            )),
        }
    }

    /// Gives up `span` and charges `record.unreadable`.
    fn give_up(
        &mut self,
        span: Span,
        path: &str,
        classes: &[ArenaClass],
        error: impl fmt::Display,
    ) {
        if let Some(loss) = self.give_up_part(span, path, classes, error) {
            self.report.push(loss);
        }
    }

    /// Runs `read` in a repair session under the ceilings of the read.
    fn attempt<T>(
        &self,
        read: impl FnOnce() -> Result<T, serde_json::Error>,
    ) -> (
        Result<T, serde_json::Error>,
        Vec<Repaired>,
        Option<ResourceLimit>,
    ) {
        let ((result, repairs), ceiling) =
            crate::design::read::with_limits(&self.limits, || read_session::run(read));
        (result, repairs, ceiling)
    }

    fn repaired(&mut self, path: &str, repairs: Vec<Repaired>) {
        for repaired in repairs {
            let code = match repaired.repair {
                Repair::Spelling => ReadLossCode::SpellingNormalized,
                Repair::Value => ReadLossCode::ValueNormalized,
            };
            self.note(code, format!("{path}: {}", repaired.what));
        }
    }

    /// Reads one part as a value of `T`, or gives it up.
    fn value<T: DeserializeOwned>(
        &mut self,
        span: Span,
        path: &str,
        enclosing: usize,
        classes: &[ArenaClass],
    ) -> Option<T> {
        if self.halted() {
            return None;
        }
        if !span.closed {
            self.give_up(span, path, classes, "the text stops being JSON inside it");
            return None;
        }
        let depth = enclosing + span.depth;
        if depth > MAX_DOCUMENT_DEPTH {
            self.give_up(
                span,
                path,
                classes,
                format_args!("it nests to depth {depth}, past the bound of {MAX_DOCUMENT_DEPTH}"),
            );
            return None;
        }
        if let Ok(text) = std::str::from_utf8(&self.bytes[span.start..span.end]) {
            let (read, repairs, ceiling) = self.attempt(|| one_zero::from_str::<T>(text));
            if let Some(limit) = ceiling {
                self.give_up(span, path, classes, CodecError::ResourceLimit(limit));
                return None;
            }
            if let Ok(value) = read {
                self.repaired(path, repairs);
                return Some(value);
            }
        }
        self.repair(span, path, classes)
    }

    /// Reads one part through its value, repairing each refusal that has one
    /// reading, or gives it up.
    fn repair<T: DeserializeOwned>(
        &mut self,
        span: Span,
        path: &str,
        classes: &[ArenaClass],
    ) -> Option<T> {
        let mut found = Vec::new();
        let mut value = match text::value(self.bytes, span, &mut found) {
            Ok(value) => value,
            Err(error) => {
                self.give_up(span, path, classes, error);
                return None;
            }
        };
        let mut notes = Vec::new();
        let mut repeated = false;
        let mut copies = Vec::new();
        for found in found {
            notes.push(match found {
                Found::Replaced(at) => ReadLossCode::ValueNormalized.note(format!(
                    "{path}: the string at offset {at} holds an invalid UTF-8 sequence, replaced by U+FFFD"
                )),
                Found::Restated(key) => ReadLossCode::SpellingNormalized.note(format!(
                    "{path}: key `{key}` is stated more than once with one value"
                )),
                Found::Conflicting { key, copies: extents } => {
                    repeated = true;
                    let positions: Vec<usize> = extents.iter().map(|(start, _)| *start).collect();
                    copies.extend(extents.into_iter().map(|extent| (key.clone(), extent)));
                    ReadLossCode::RecordRepeated.note(format!(
                        "{path}: key `{key}` is stated at offsets {positions:?} with different values; no copy is kept"
                    ))
                }
            });
        }
        let mut ignored = false;
        for _ in 0..MAX_REPAIRS {
            let (read, repairs, ceiling) = self.attempt(|| one_zero::from_value::<T>(&value));
            let error = match read {
                Ok(read) => {
                    self.report.append(&mut notes);
                    self.repaired(path, repairs);
                    for (key, (start, end)) in &copies {
                        let copy = Span {
                            start: *start,
                            end: *end,
                            closed: true,
                            depth: 0,
                        };
                        // discarded-value: `record.repeated` charges the key.
                        let _ = self.give_up_part(
                            copy,
                            &format!("{path}.{key}"),
                            classes,
                            "a restated key",
                        );
                    }
                    return Some(read);
                }
                Err(error) => error,
            };
            if let Some(limit) = ceiling {
                self.report.append(&mut notes);
                self.give_up(span, path, classes, CodecError::ResourceLimit(limit));
                return None;
            }
            let message = error.to_string();
            let repaired = match repair::refusal(&message) {
                Some(repair::Refusal::UnknownField { name, expected }) => {
                    match repair::member(&mut value, &name, &expected) {
                        Some(repair::MemberRepair::Renamed { from, to }) => {
                            notes.push(
                                ReadLossCode::SpellingNormalized
                                    .note(format!("{path}: member `{from}` is read as `{to}`")),
                            );
                            true
                        }
                        Some(repair::MemberRepair::Restated {
                            from,
                            to,
                            one_value: true,
                        }) => {
                            notes.push(ReadLossCode::SpellingNormalized.note(format!(
                                "{path}: member `{from}` restates `{to}` with one value"
                            )));
                            true
                        }
                        Some(repair::MemberRepair::Restated {
                            from,
                            to,
                            one_value: false,
                        }) => {
                            repeated = true;
                            notes.push(ReadLossCode::RecordRepeated.note(format!(
                                "{path}: members `{from}` and `{to}` state one member with different values; no copy is kept"
                            )));
                            true
                        }
                        Some(repair::MemberRepair::Ignored(name)) => {
                            ignored = true;
                            notes.push(ReadLossCode::MemberIgnored.note(format!(
                                "{path}: member `{name}` is no member of its record and is not stated"
                            )));
                            true
                        }
                        None => false,
                    }
                }
                Some(repair::Refusal::UnknownVariant { name, expected }) => {
                    match repair::case_variant(&name, &expected) {
                        Some(to) if repair::variant(&mut value, &name, to) => {
                            notes.push(
                                ReadLossCode::SpellingNormalized
                                    .note(format!("{path}: variant `{name}` is read as `{to}`")),
                            );
                            true
                        }
                        _ => false,
                    }
                }
                Some(repair::Refusal::MissingField(name))
                    if !ignored && !repeated && self.collection::<T>(&mut value, &name) =>
                {
                    notes.push(ReadLossCode::SpellingNormalized.note(format!(
                        "{path}: the omitted collection `{name}` is read as empty"
                    )));
                    true
                }
                _ => false,
            };
            if !repaired {
                self.report.append(&mut notes);
                let loss = self.give_up_part(span, path, classes, &message);
                if let Some(loss) = loss {
                    if repeated || message.contains("stated twice with different values") {
                        self.note(
                            ReadLossCode::RecordRepeated,
                            format!("{path}: {message}; the record is given up"),
                        );
                    } else {
                        self.report.push(loss);
                    }
                }
                return None;
            }
        }
        self.report.append(&mut notes);
        self.give_up(
            span,
            path,
            classes,
            "its refusals take more repairs than the reader makes",
        );
        None
    }

    /// States the omitted collection `name` as empty in the object of
    /// `value` that the record's type then reads past it.
    fn collection<T: DeserializeOwned>(&self, value: &mut Value, name: &str) -> bool {
        let missing = format!("missing field `{name}`");
        for site in repair::collection_sites(value, name, MAX_COLLECTION_SITES) {
            for filler in [
                Value::Array(Vec::new()),
                Value::Object(serde_json::Map::new()),
            ] {
                repair::insert(value, &site, name, filler);
                let (read, _, _) = self.attempt(|| one_zero::from_value::<T>(value));
                let message = read.err().map(|error| error.to_string());
                let progressed = message.as_deref().is_none_or(|message| {
                    !(message.contains(&missing)
                        || message.contains("invalid type")
                        || message.contains("invalid length")
                        || message.contains(&format!("unknown field `{name}`")))
                });
                if progressed {
                    return true;
                }
            }
            repair::remove(value, &site, name);
        }
        false
    }
}

impl<'r, 'a> Reader<'r, 'a> {
    fn document(&mut self) -> Result<CadIr, ReadFailure> {
        let mut scanner = Scanner::new(self.bytes, 0);
        if !scanner.next_is(b'{') {
            return Err(ReadFailure::Malformed(format!(
                "the text at offset {} opens no object",
                scanner.at()
            )));
        }
        let (members, stop) = scanner.members();
        let root_closed = stop.is_none();
        self.stopped(stop);
        if root_closed {
            scanner.whitespace();
            let tail = scanner.at();
            if tail < self.bytes.len() {
                self.stopped(Some(text::Stop {
                    at: tail,
                    expected: "the end of the input after the root object",
                }));
            }
        }
        let mut root = self.members("the root", members, ROOT_MEMBERS, root_closed);
        let version = root
            .take("ir_version")
            .and_then(|span| self.plain::<Value>(span));
        super::check_ir_version::<serde_json::Error>(version.as_ref())
            .map_err(|error| ReadFailure::Version(error.to_string()))?;
        let units = root
            .take("units")
            .ok_or_else(|| ReadFailure::Units("units is missing".to_owned()))?;
        let units_text = std::str::from_utf8(&self.bytes[units.start..units.end])
            .map_err(|error| ReadFailure::Units(error.to_string()))?;
        if !units.closed {
            return Err(ReadFailure::Units(
                "the text stops being JSON inside it".to_owned(),
            ));
        }
        one_zero::from_str::<CanonicalUnitsWire>(units_text)
            .map_err(|error| ReadFailure::Units(error.to_string()))?;

        let fragment = self.required(&mut root, "fragment", false);
        let scope = self.required(&mut root, "scope", ContentScope::full());
        let tolerances = self.required(&mut root, "tolerances", Tolerances::default());
        let repro = self.optional::<Box<crate::repro::BundleMetadata>>(&mut root, "repro");
        let source = self.optional::<super::SourceMeta>(&mut root, "source");
        let geometry = {
            let mut home = self.home(&mut root, "geometry", Topology::READ_FIELDS);
            Topology::read_from(&mut home)
        };
        let model = {
            let mut home = self.home(&mut root, "model", Model::READ_FIELDS);
            Model::read_from(&mut home)
        };
        let design = {
            let mut home = self.home(&mut root, "design", Design::READ_FIELDS);
            Design::read_from(&mut home)
        };
        let process = {
            let mut home = self.home(&mut root, "process", Process::READ_FIELDS);
            Process::read_from(&mut home)
        };
        let native = self.native(&mut root);
        if let Some(limit) = self.refusal {
            return Err(ReadFailure::ResourceLimit(limit));
        }
        if let Some(stop) = self.stop.clone().filter(|stop| stop.at < self.bytes.len()) {
            let span = Span {
                start: stop.at,
                end: self.bytes.len(),
                closed: false,
                depth: 0,
            };
            self.give_up(span, "the text after the last record read", &[], &stop);
        }
        let mut document = CadIr {
            fragment,
            repro,
            scope,
            source,
            tolerances,
            geometry,
            model,
            design,
            process,
            native,
        };
        let (_, ceiling) = crate::design::read::with_limits(&self.limits, || {
            super::admit_state_chains(&document.design, &document.process, Cycles::End)
        });
        if let Some(limit) = ceiling {
            return Err(ReadFailure::ResourceLimit(ResourceLimit {
                scope: LimitScope::Session,
                ..limit
            }));
        }
        let mut fidelity =
            SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(self.bytes));
        for part in std::mem::take(&mut self.parts) {
            if let Err(error) = part.commit(self.ctx, &mut document, &mut fidelity) {
                match error {
                    CodecError::ResourceLimit(limit) => {
                        return Err(ReadFailure::ResourceLimit(ResourceLimit {
                            scope: LimitScope::Session,
                            ..limit
                        }));
                    }
                    other => self.report.push(LossNote::new(
                        record_unreadable_kind(),
                        format!("a given-up part keeps no stub: {other}"),
                    )),
                }
            }
        }
        Ok(document)
    }

    /// Reads a part with no repairs, for a member whose refusal refuses the
    /// read.
    fn plain<T: DeserializeOwned>(&self, span: Span) -> Option<T> {
        if !span.closed {
            return None;
        }
        let text = std::str::from_utf8(&self.bytes[span.start..span.end]).ok()?;
        one_zero::from_str(text).ok()
    }

    /// Reads a required root member, or states `default` where it is
    /// missing or given up.
    fn required<T: DeserializeOwned>(&mut self, root: &mut Members, name: &str, default: T) -> T {
        if let Some(span) = root.take(name) {
            if let Some(value) = self.value(span, name, 1, &[]) {
                return value;
            }
        } else if !root.repeated.iter().any(|repeated| repeated == name) {
            self.report.push(LossNote::new(
                record_unreadable_kind(),
                format!("the root: the required member `{name}` is missing; its default is stated"),
            ));
        }
        default
    }

    /// Reads an optional root member.
    fn optional<T: DeserializeOwned>(&mut self, root: &mut Members, name: &str) -> Option<T> {
        let span = root.take(name)?;
        if span.closed && &self.bytes[span.start..span.end] == b"null" {
            self.note(
                ReadLossCode::SpellingNormalized,
                format!("the root: `{name}` states null for an omitted value"),
            );
            return None;
        }
        self.value(span, name, 1, &[])
    }

    /// Resolves the members of an object to the names of its type: a key that
    /// differs from exactly one name only in letter case is read as that
    /// name; a name stated more than once, in any letter case, is read once
    /// where every copy holds one value and given up otherwise; any other key
    /// is not stated. `closed` states whether the object closes.
    fn members(
        &mut self,
        path: &str,
        members: Vec<Member>,
        known: &[&'static str],
        closed: bool,
    ) -> Members {
        let mut ignored = false;
        let mut read = Vec::with_capacity(members.len());
        for mut member in members {
            if !member.key_invalid && !known.contains(&member.key.as_str()) {
                let mut folded = known
                    .iter()
                    .filter(|name| name.eq_ignore_ascii_case(&member.key));
                if let (Some(name), None) = (folded.next(), folded.next()) {
                    self.note(
                        ReadLossCode::SpellingNormalized,
                        format!("{path}: member `{}` is read as `{name}`", member.key),
                    );
                    (*name).clone_into(&mut member.key);
                } else {
                    ignored = true;
                    self.note(
                        ReadLossCode::MemberIgnored,
                        format!(
                            "{path}: member `{}` is no member of {path} and is not stated{}",
                            member.key,
                            if path == "model" {
                                super::legacy::omitted_arena_hint(&member.key)
                            } else {
                                String::new()
                            }
                        ),
                    );
                    continue;
                }
            }
            read.push(member);
        }
        let (distinct, repeated) = self.distinct(path, read);
        let named = distinct
            .into_iter()
            .filter_map(|(key, span)| {
                known
                    .iter()
                    .find(|name| **name == key)
                    .map(|name| (*name, span))
            })
            .collect();
        Members {
            named,
            closed,
            ignored,
            repeated,
        }
    }

    /// Charges the omitted member `name` of `holder`: read as empty where the
    /// object closes and holds no member the reader does not know, and given
    /// up otherwise (§3.3). A name given up as repeated, or one past the
    /// point where the text stops being JSON, charges nothing more.
    fn omitted(&mut self, holder: &Members, path: &str, name: &str) {
        if holder.repeated.iter().any(|repeated| repeated == name) || !holder.closed {
            return;
        }
        if holder.ignored {
            self.report.push(LossNote::new(
                record_unreadable_kind(),
                format!(
                    "{path}: the omitted collection `{name}` has no one reading: {path} holds a \
                     member the reader does not know"
                ),
            ));
        } else {
            self.note(
                ReadLossCode::SpellingNormalized,
                format!("{path}: the omitted collection `{name}` is read as empty"),
            );
        }
    }

    /// The members of an object, each key once: a restated key is read once
    /// where every copy holds one value, and given up otherwise. A key that
    /// holds an invalid UTF-8 sequence is given up.
    fn distinct(&mut self, path: &str, members: Vec<Member>) -> (Vec<(String, Span)>, Vec<String>) {
        let mut copies: BTreeMap<String, Vec<(usize, Span)>> = BTreeMap::new();
        let mut order = Vec::new();
        for member in members {
            if member.key_invalid {
                self.give_up(
                    member.value,
                    &format!("{path}.{}", member.key),
                    &[],
                    format_args!(
                        "the key at offset {} holds an invalid UTF-8 sequence",
                        member.at
                    ),
                );
                continue;
            }
            let entry = copies.entry(member.key.clone()).or_default();
            if entry.is_empty() {
                order.push(member.key);
            }
            entry.push((member.at, member.value));
        }
        let mut distinct = Vec::with_capacity(order.len());
        let mut repeated = Vec::new();
        for key in order {
            let copies = copies.remove(&key).unwrap_or_default();
            let Some(&(_, first)) = copies.first() else {
                continue;
            };
            if copies.len() > 1 {
                let holds_one = copies.iter().all(|(_, span)| self.same_value(first, *span));
                let positions: Vec<usize> = copies.iter().map(|(at, _)| *at).collect();
                if !holds_one {
                    for (_, span) in &copies {
                        // discarded-value: `record.repeated` charges the key.
                        let _ = self.give_up_part(
                            *span,
                            &format!("{path}.{key}"),
                            &[],
                            "a restated key",
                        );
                    }
                    self.note(
                        ReadLossCode::RecordRepeated,
                        format!(
                            "{path}: key `{key}` is stated at offsets {positions:?} with different values; no copy is kept"
                        ),
                    );
                    repeated.push(key);
                    continue;
                }
                self.note(
                    ReadLossCode::SpellingNormalized,
                    format!(
                        "{path}: key `{key}` is stated at offsets {positions:?} with one value"
                    ),
                );
            }
            distinct.push((key, first));
        }
        (distinct, repeated)
    }

    /// Whether two parts hold one value.
    fn same_value(&self, left: Span, right: Span) -> bool {
        if !(left.closed && right.closed) {
            return false;
        }
        if self.bytes[left.start..left.end] == self.bytes[right.start..right.end] {
            return true;
        }
        let value = |span| text::value(self.bytes, span, &mut Vec::new()).ok();
        left.depth <= MAX_DOCUMENT_DEPTH
            && right.depth <= MAX_DOCUMENT_DEPTH
            && value(left).is_some_and(|left| value(right).is_some_and(|right| left == right))
    }

    /// Opens a home of the root.
    fn home<'h>(
        &'h mut self,
        root: &mut Members,
        name: &'static str,
        known: &[&'static str],
    ) -> Home<'h, 'r, 'a> {
        let class = match name {
            "design" => ArenaClass::Design,
            "process" => ArenaClass::Process,
            _ => ArenaClass::Model,
        };
        let members = match root.take(name) {
            None => {
                self.omitted(root, "the root", name);
                Members::unread()
            }
            Some(span) => match self.object(span, name, &[class]) {
                Some((members, closed)) => self.members(name, members, known, closed),
                None => Members::unread(),
            },
        };
        Home {
            reader: self,
            path: name,
            class,
            members,
            spans: BTreeMap::new(),
        }
    }

    /// Frames the members of the object `span` holds, with whether it
    /// closes, or gives up a part that holds no object.
    fn object(
        &mut self,
        span: Span,
        path: &str,
        classes: &[ArenaClass],
    ) -> Option<(Vec<Member>, bool)> {
        let mut scanner = Scanner::new(self.bytes, span.start);
        if !scanner.next_is(b'{') {
            self.give_up(span, path, classes, "it holds no object");
            return None;
        }
        let (members, stop) = scanner.members();
        let closed = stop.is_none();
        self.stopped(stop);
        Some((members, closed))
    }

    /// Reads the records of one arena, sorted by id, with each id once, and
    /// the span of each.
    fn arena<T: DeserializeOwned>(
        &mut self,
        path: &str,
        span: Span,
        enclosing: usize,
        class: ArenaClass,
        id: fn(&T) -> &str,
    ) -> Vec<(T, Span)> {
        let mut scanner = Scanner::new(self.bytes, span.start);
        if !scanner.next_is(b'[') {
            self.give_up(span, path, &[class], "it holds no array");
            return Vec::new();
        }
        let (elements, stop) = scanner.elements();
        self.stopped(stop);
        let mut records = Vec::with_capacity(elements.len());
        for (index, element) in elements.into_iter().enumerate() {
            if self.halted() {
                break;
            }
            let at = format!("{path}[{index}]");
            if let Some(record) = self.value::<T>(element, &at, enclosing + 1, &[class]) {
                if let Err(error) = self.ctx.charge_entities(1, "CADIR record") {
                    if let CodecError::ResourceLimit(limit) = error {
                        self.refuse(limit);
                    }
                    break;
                }
                records.push((record, element));
            }
        }
        if !records
            .windows(2)
            .all(|pair| id(&pair[0].0) <= id(&pair[1].0))
        {
            records.sort_by(|left, right| id(&left.0).cmp(id(&right.0)));
            self.note(
                ReadLossCode::SpellingNormalized,
                format!("{path}: the arena states its records out of id order"),
            );
        }
        self.distinct_ids(path, records, class, id)
    }

    /// Keeps one record of each id where every copy holds one value, and
    /// gives up every copy of an id stated with different values.
    fn distinct_ids<T>(
        &mut self,
        path: &str,
        records: Vec<(T, Span)>,
        class: ArenaClass,
        id: fn(&T) -> &str,
    ) -> Vec<(T, Span)> {
        let mut kept: Vec<(T, Span)> = Vec::with_capacity(records.len());
        let mut run: Vec<(T, Span)> = Vec::new();
        let mut records = records.into_iter().peekable();
        while let Some(record) = records.next() {
            let continues = records
                .peek()
                .is_some_and(|next| id(&next.0) == id(&record.0));
            run.push(record);
            if continues {
                continue;
            }
            let copies = std::mem::take(&mut run);
            if copies.len() == 1 {
                kept.extend(copies);
                continue;
            }
            let first = copies[0].1;
            let record_id = id(&copies[0].0).to_owned();
            let positions: Vec<usize> = copies.iter().map(|(_, span)| span.start).collect();
            if copies.iter().all(|(_, span)| self.same_value(first, *span)) {
                self.note(
                    ReadLossCode::SpellingNormalized,
                    format!(
                        "{path}: id {record_id} is stated at offsets {positions:?} with one value"
                    ),
                );
                kept.extend(copies.into_iter().take(1));
                continue;
            }
            for (_, span) in &copies {
                // discarded-value: `record.repeated` charges the id.
                let _ = self.give_up_part(
                    *span,
                    &format!("{path} {record_id}"),
                    &[class],
                    "a restated id",
                );
            }
            self.note(
                ReadLossCode::RecordRepeated,
                format!(
                    "{path}: id {record_id} is stated at offsets {positions:?} with different values; no copy is kept; dropped=[{record_id}]"
                ),
            );
        }
        kept
    }

    fn native(&mut self, root: &mut Members) -> Native {
        let mut native = Native::default();
        let Some(span) = root.take("native") else {
            self.omitted(root, "the root", "native");
            return native;
        };
        let Some((members, _)) = self.object(span, "native", &[ArenaClass::Model]) else {
            return native;
        };
        for (key, span) in self.distinct("native", members).0 {
            let path = format!("native.{key}");
            let Some(codec) = CodecId::new(key.clone()) else {
                self.give_up(
                    span,
                    &path,
                    &[ArenaClass::Model],
                    format_args!("{key:?} is no codec id"),
                );
                continue;
            };
            let Some((arenas, _)) = self.object(span, &path, &[ArenaClass::Model]) else {
                continue;
            };
            for (name, span) in self.distinct(&path, arenas).0 {
                let arena_path = format!("{path}.{name}");
                let Some(name) = NonBlankString::new(name) else {
                    self.give_up(
                        span,
                        &arena_path,
                        &[ArenaClass::Model],
                        "a blank arena name",
                    );
                    continue;
                };
                let records = self
                    .arena(&arena_path, span, 3, ArenaClass::Model, NativeRecord::id)
                    .into_iter()
                    .map(|(record, _)| record)
                    .collect();
                native
                    .namespace_mut(codec.clone())
                    .arenas_mut()
                    .insert(name, records);
            }
        }
        native
    }
}

/// One home of the root, read arena by arena.
pub(super) struct Home<'h, 'r, 'a> {
    reader: &'h mut Reader<'r, 'a>,
    path: &'static str,
    class: ArenaClass,
    members: Members,
    spans: BTreeMap<String, Span>,
}

impl Home<'_, '_, '_> {
    /// Reads the arena `name`.
    pub(super) fn arena<T: DeserializeOwned>(&mut self, name: &str, id: fn(&T) -> &str) -> Vec<T> {
        self.arena_spans(name, id)
            .into_iter()
            .map(|(record, _)| record)
            .collect()
    }

    /// Reads the arena `name`, keeping each record's span for a later give-up.
    pub(super) fn arena_keeping_spans<T: DeserializeOwned>(
        &mut self,
        name: &str,
        id: fn(&T) -> &str,
    ) -> Vec<T> {
        let records = self.arena_spans(name, id);
        records
            .into_iter()
            .map(|(record, span)| {
                self.spans.insert(format!("{name}/{}", id(&record)), span);
                record
            })
            .collect()
    }

    fn arena_spans<T: DeserializeOwned>(
        &mut self,
        name: &str,
        id: fn(&T) -> &str,
    ) -> Vec<(T, Span)> {
        let Some(span) = self.members.take(name) else {
            self.reader.omitted(&self.members, self.path, name);
            return Vec::new();
        };
        let path = format!("{}.{name}", self.path);
        self.reader.arena(&path, span, 2, self.class, id)
    }

    /// Reads the member `name` as one part.
    pub(super) fn value<T: DeserializeOwned + Default>(&mut self, name: &str) -> T {
        let path = format!("{}.{name}", self.path);
        match self.members.take(name) {
            Some(span) => self
                .reader
                .value(span, &path, 2, &[self.class])
                .unwrap_or_default(),
            None => {
                self.reader.omitted(&self.members, self.path, name);
                T::default()
            }
        }
    }

    /// Gives up the record `id` of arena `name` that its type admitted and
    /// the home then cannot hold.
    pub(super) fn give_up_record(&mut self, name: &str, id: &str, error: &str) {
        let path = format!("{}.{name} {id}", self.path);
        let span = self.spans.get(&format!("{name}/{id}")).copied();
        match span {
            Some(span) => self.reader.give_up(span, &path, &[self.class], error),
            None => self.reader.report.push(LossNote::new(
                record_unreadable_kind(),
                format!("{path}: {error}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests;
