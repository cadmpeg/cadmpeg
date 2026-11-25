// SPDX-License-Identifier: Apache-2.0
//! What one process record holds, found by writing its JSON.
//!
//! A process record nests `Choice`s, process dimensions, selections, and
//! constructions through some two hundred fields. Writing the record's JSON
//! through the canonical walk reaches every one of them: a struct is
//! recognised by the name serde gives it, so a field added to a process type
//! is walked without a change here. The [`Reader`] also measures the record's
//! JSON nesting, which the reader bounds.

use std::cell::RefCell;

use serde::Serialize;
use serde_json::{Map, Value};

use super::records::{OwnerKey, ProcessRecords};
use crate::design::dimension::ParamRef;
use crate::design::selection::{Anchor, PickContext};
use crate::hash::finite_json::{admission::CanonicalAdmission, write_serde_json, Structs};
use crate::process::state::ProcessStateOf;

/// Where in a record a value sits: its field names, map keys, and indices,
/// as a JSON pointer.
pub(super) type At = String;

/// A `Choice` (part 7).
pub(super) struct ChoiceSite {
    pub(super) at: At,
    pub(super) has_value: bool,
    pub(super) parameters: Vec<String>,
}

/// The values one record holds that the process rules check.
#[derive(Default)]
pub(super) struct Found {
    pub(super) choices: Vec<ChoiceSite>,
    /// Every `ParamRef`: a process dimension's binding and an expression's
    /// reference.
    pub(super) references: Vec<(At, ParamRef)>,
    /// The `workpiece` of every `WorkpieceSelection` and
    /// `ProcessConstruction`.
    pub(super) workpieces: Vec<(At, Option<u64>)>,
    /// The anchor of every selection.
    pub(super) anchors: Vec<(At, Anchor)>,
    /// The context of every pick.
    pub(super) contexts: Vec<(At, PickContext)>,
}

impl Found {
    /// Whether the record holds a selection or a construction that picks.
    pub(super) fn holds_selection(&self) -> bool {
        !self.anchors.is_empty() || !self.workpieces.is_empty()
    }
}

/// One process record and what it holds.
pub(super) struct Walked<'a> {
    /// The record's arena.
    pub(super) arena: &'static str,
    pub(super) id: &'a str,
    /// The record its selections and paths resolve from: the record itself,
    /// a toolpath's or a state's operation or setup, or a parameter's owner.
    pub(super) owner: OwnerKey<'a>,
    pub(super) found: Found,
}

/// Walks every process record.
pub(super) fn walk_records<'a>(records: &ProcessRecords<'a>) -> Vec<Walked<'a>> {
    let process = &records.ir.process;
    let mut walked = Vec::new();
    let mut push = |arena, id, owner, found| {
        walked.push(Walked {
            arena,
            id,
            owner,
            found,
        });
    };
    for setup in &process.setups {
        let id = setup.id.as_str();
        push("setups", id, OwnerKey::Setup(id), walk(setup));
    }
    for operation in &process.operations {
        let id = operation.id.as_str();
        push("operations", id, OwnerKey::Operation(id), walk(operation));
    }
    for feature in &process.features {
        let id = feature.id.as_str();
        push("features", id, OwnerKey::Feature(id), walk(feature));
    }
    for group in &process.groups {
        let id = group.id.as_str();
        push("groups", id, OwnerKey::Group(id), walk(group));
    }
    for tool in &process.tools {
        let id = tool.id.as_str();
        push("tools", id, OwnerKey::Tool(id), walk(tool));
    }
    for preset in &process.presets {
        let id = preset.id.as_str();
        push("presets", id, OwnerKey::Preset(id), walk(preset));
    }
    for machine in &process.machines {
        let id = machine.id.as_str();
        push("machines", id, OwnerKey::Machine(id), walk(machine));
    }
    for program in &process.programs {
        let id = program.id.as_str();
        push("programs", id, OwnerKey::Program(id), walk(program));
    }
    for parameter in &process.parameters {
        let owner = OwnerKey::of(&parameter.owner);
        push("parameters", parameter.id.as_str(), owner, walk(parameter));
    }
    for toolpath in &process.toolpaths {
        let owner = OwnerKey::Operation(toolpath.operation.as_str());
        push("toolpaths", toolpath.id.as_str(), owner, walk(toolpath));
    }
    for state in &process.states {
        let owner = match &state.of {
            ProcessStateOf::Stock(setup) => OwnerKey::Setup(setup.as_str()),
            ProcessStateOf::After(operation) | ProcessStateOf::Removal(operation) => {
                OwnerKey::Operation(operation.as_str())
            }
        };
        push("states", state.id.as_str(), owner, walk(state));
    }
    walked
}

/// Walks `record` and returns what it holds.
pub(super) fn walk<T: Serialize + ?Sized>(record: &T) -> Found {
    let reader = RefCell::new(Reader::default());
    let structs: &dyn Structs = &reader;
    // The reader has no failing path; a type whose own `Serialize` fails is
    // reported by the document's serialization.
    match write_serde_json(
        CanonicalAdmission::unlimited(),
        Some(structs),
        &mut Sink(&reader),
        record,
    ) {
        Ok(()) | Err(_) => {}
    }
    reader.into_inner().found
}

/// What a struct of a recognised name captures from its JSON members.
enum Capture {
    Choice,
    ParamRef,
    PickContext,
    /// The `workpiece` of a `WorkpieceSelection` or `ProcessConstruction`.
    Workpiece,
    /// The `at` of a `Selection`.
    Selection,
}

impl Capture {
    fn of(name: &str) -> Option<Self> {
        Some(match name {
            "Choice" => Self::Choice,
            "ParamRef" => Self::ParamRef,
            "PickContext" => Self::PickContext,
            "WorkpieceSelection" | "ProcessConstruction" => Self::Workpiece,
            "Selection" => Self::Selection,
            _ => return None,
        })
    }

    fn finish(self, at: At, mut members: Map<String, Value>, found: &mut Found) {
        match self {
            Self::Choice => found.choices.push(ChoiceSite {
                at,
                has_value: members.contains_key("value"),
                parameters: match members.remove("parameters") {
                    Some(Value::Array(items)) => items
                        .into_iter()
                        .filter_map(|item| match item {
                            Value::String(text) => Some(text),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                },
            }),
            Self::ParamRef => {
                if let Ok(reference) = serde_json::from_value(Value::Object(members)) {
                    found.references.push((at, reference));
                }
            }
            Self::PickContext => {
                if let Ok(context) = serde_json::from_value(Value::Object(members)) {
                    found.contexts.push((at, context));
                }
            }
            Self::Selection => {
                if let Some(anchor) = members
                    .remove("at")
                    .and_then(|value| serde_json::from_value(value).ok())
                {
                    found.anchors.push((at, anchor));
                }
            }
            Self::Workpiece => found
                .workpieces
                .push((at, members.get("workpiece").and_then(Value::as_u64))),
        }
    }
}

/// A capturing struct whose JSON text is being written.
struct Open {
    capture: Capture,
    at: At,
    /// Where the struct's `{` sits in [`Reader::text`].
    start: usize,
}

/// An open JSON container and the member being written in it.
enum Container {
    Array { index: usize },
    Object { key: Vec<u8>, expects_key: bool },
}

/// Reads one record's JSON text as it is written, with the structs that
/// write it.
#[derive(Default)]
struct Reader {
    containers: Vec<Container>,
    in_string: bool,
    escaped: bool,
    /// The current string is an object key.
    in_key: bool,
    /// The open structs, innermost last.
    structs: Vec<Option<Open>>,
    /// The text since the outermost open capture began.
    text: Vec<u8>,
    captures: usize,
    found: Found,
}

impl Reader {
    fn read(&mut self, byte: u8) {
        if self.captures > 0 {
            self.text.push(byte);
        }
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
                self.in_key = false;
                return;
            }
            if self.in_key {
                if let Some(Container::Object { key, .. }) = self.containers.last_mut() {
                    key.push(byte);
                }
            }
            return;
        }
        match byte {
            b'"' => {
                self.in_string = true;
                if let Some(Container::Object { key, expects_key }) = self.containers.last_mut() {
                    if *expects_key {
                        *expects_key = false;
                        key.clear();
                        self.in_key = true;
                    }
                }
            }
            b'{' => self.open(Container::Object {
                key: Vec::new(),
                expects_key: true,
            }),
            b'[' => self.open(Container::Array { index: 0 }),
            b'}' | b']' => {
                self.containers.pop();
            }
            b',' => match self.containers.last_mut() {
                Some(Container::Array { index }) => *index += 1,
                Some(Container::Object { expects_key, .. }) => *expects_key = true,
                None => {}
            },
            _ => {}
        }
    }

    fn open(&mut self, container: Container) {
        self.containers.push(container);
    }

    /// The JSON pointer of the value about to be written.
    fn at(&self) -> At {
        let mut at = String::new();
        for container in &self.containers {
            at.push('/');
            match container {
                Container::Array { index } => at.push_str(&index.to_string()),
                Container::Object { key, .. } => at.push_str(&key_text(key)),
            }
        }
        at
    }

    fn begin(&mut self, name: &'static str) {
        let open = Capture::of(name).map(|capture| Open {
            capture,
            at: self.at(),
            start: self.text.len(),
        });
        if open.is_some() {
            self.captures += 1;
        }
        self.structs.push(open);
    }

    fn end(&mut self) {
        let Some(Some(open)) = self.structs.pop() else {
            return;
        };
        let members = std::str::from_utf8(&self.text[open.start..])
            .ok()
            .and_then(|text| crate::native::replay::value(text).ok());
        self.captures -= 1;
        if self.captures == 0 {
            self.text.clear();
        }
        if let Some(Value::Object(members)) = members {
            open.capture.finish(open.at, members, &mut self.found);
        }
    }
}

/// The text of an object key, from the bytes between its quotes.
fn key_text(raw: &[u8]) -> String {
    if !raw.contains(&b'\\') {
        return String::from_utf8_lossy(raw).into_owned();
    }
    let mut quoted = String::with_capacity(raw.len() + 2);
    quoted.push('"');
    quoted.push_str(&String::from_utf8_lossy(raw));
    quoted.push('"');
    serde_json::from_str(&quoted).unwrap_or_default()
}

impl Structs for RefCell<Reader> {
    fn begin(&self, name: &'static str) {
        self.borrow_mut().begin(name);
    }

    fn end(&self) {
        self.borrow_mut().end();
    }
}

/// Feeds the written JSON text to a [`Reader`].
struct Sink<'a>(&'a RefCell<Reader>);

impl std::io::Write for Sink<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut reader = self.0.borrow_mut();
        for &byte in bytes {
            reader.read(byte);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
