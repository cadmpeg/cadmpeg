// SPDX-License-Identifier: Apache-2.0
//! D16 (§5.11): in a `FeatureStateOverride.definition`, of a configuration or
//! an arrangement, every `Dimension` at a place the base feature's definition
//! also has equals the base's; a dimension difference is a parameter
//! override.
//!
//! A place is the path of field names, variant names, map keys, and list
//! indices from the definition to the dimension; a struct's `kind` tag, once
//! written, also names the place of the struct's later fields. The dimensions
//! of a definition are found in the JSON text the canonical walk writes for
//! it, with the structs that write it: `Dimension` is the adjacently tagged
//! enum serde names `Dimension`, tagged `literal`, `parameter`, or `negated`.
//! Names and picks hold no dimension, and a dimension's value is not entered.
//! Two literal reals are equal within the document's linear and angular
//! tolerances.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::Value;

use crate::design::configuration::Overrides;
use crate::design::feature::FeatureDefinition;
use crate::hash::finite_json::{admission::CanonicalAdmission, write_serde_json, Structs};
use crate::report::check::Finding;

use super::records::DesignRecords;

pub(super) fn check(records: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let design = &records.ir.design;
    let tolerances = &records.ir.tolerances;
    let tolerance = tolerances.linear.get().max(tolerances.angular.get());
    let holders = design
        .configurations
        .iter()
        .map(|configuration| (configuration.id.as_str(), &configuration.overrides))
        .chain(
            design
                .arrangements
                .iter()
                .map(|arrangement| (arrangement.id.as_str(), &arrangement.overrides)),
        );
    for (holder, overrides) in holders {
        check_overrides(records, holder, overrides, tolerance, findings);
    }
}

fn check_overrides(
    records: &DesignRecords<'_>,
    holder: &str,
    overrides: &Overrides,
    tolerance: f64,
    findings: &mut Vec<Finding>,
) {
    for (feature, state) in &overrides.features {
        let (Some(definition), Some(base)) =
            (&state.definition, records.features.get(feature.as_str()))
        else {
            continue;
        };
        let (Some(own), Some(base)) = (dimensions(definition), dimensions(&base.definition)) else {
            continue;
        };
        for (place, dimension) in &own {
            let Some(stated) = base.get(place) else {
                continue;
            };
            if !dimension.equals(stated, tolerance) {
                super::error(findings, holder, crate::rule!("design.definition-override-states-another-dimension-base", 
                    "definition override of {} states another dimension at {} than the base definition; a dimension difference is a parameter override",
                    feature.as_str(),
                    place.join(".")
                ));
            }
        }
    }
}

/// A dimension's arm and its value or parameter.
struct Found {
    arm: String,
    value: Value,
}

impl Found {
    fn equals(&self, other: &Self, tolerance: f64) -> bool {
        self.arm == other.arm && close(&self.value, &other.value, tolerance)
    }
}

/// The relative difference below which two literal reals are equal when it
/// exceeds the document's tolerances.
const LITERAL_RELATIVE_TOLERANCE: f64 = 1e-9;

fn close(left: &Value, right: &Value, tolerance: f64) -> bool {
    let reals = |left: f64, right: f64| {
        (left - right).abs()
            <= tolerance.max(LITERAL_RELATIVE_TOLERANCE * left.abs().max(right.abs()))
    };
    match (left, right) {
        (Value::Number(left), Value::Number(right)) if left.is_f64() && right.is_f64() => left
            .as_f64()
            .zip(right.as_f64())
            .is_some_and(|(left, right)| reals(left, right)),
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| close(left, right, tolerance))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|((left_key, left), (right_key, right))| {
                        left_key == right_key && close(left, right, tolerance)
                    })
        }
        _ => left == right,
    }
}

type Places = BTreeMap<Vec<String>, Found>;

/// Every dimension of a definition, by place. `None` where a value of it does
/// not serialize.
fn dimensions(definition: &FeatureDefinition) -> Option<Places> {
    let reader = RefCell::new(Reader::default());
    let structs: &dyn Structs = &reader;
    write_serde_json(
        CanonicalAdmission::unlimited(),
        Some(structs),
        &mut Sink(&reader),
        definition,
    )
    .ok()?;
    Some(reader.into_inner().found)
}

/// What an open struct is to the walk.
enum Role {
    /// Its fields are places.
    Entered,
    /// A name or a pick, or a struct inside an unentered one.
    Unentered,
    /// A dimension at `place`, whose JSON text begins at `from`.
    Dimension { place: Vec<String>, from: usize },
}

/// An open JSON container and the member being written in it.
enum Container {
    Array {
        index: usize,
    },
    Object {
        /// The object is a map's: its `kind` key is a key like any other.
        map: bool,
        /// The tags the object's `kind` members wrote so far.
        tags: Vec<String>,
        key: Option<String>,
        expects_key: bool,
        /// Where the value of a `kind` member being written begins.
        tag_from: Option<usize>,
    },
}

/// Reads a definition's JSON text as it is written, with the structs and
/// maps that write it.
#[derive(Default)]
struct Reader {
    text: Vec<u8>,
    containers: Vec<Container>,
    in_string: bool,
    escaped: bool,
    /// Where the object key being written begins: its opening quote.
    key_from: Option<usize>,
    /// The next object to open is a map's.
    map_next: bool,
    /// The open structs, innermost last.
    structs: Vec<Role>,
    /// The open structs that are not entered: names, picks, and dimensions.
    unentered: usize,
    found: Places,
}

impl Reader {
    fn read(&mut self, byte: u8) {
        self.text.push(byte);
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
                if let Some(from) = self.key_from.take() {
                    let text = string(&self.text[from..]);
                    if let Some(Container::Object { key, .. }) = self.containers.last_mut() {
                        *key = text;
                    }
                }
            }
            return;
        }
        let end = self.text.len() - 1;
        match byte {
            b'"' => {
                self.in_string = true;
                if let Some(Container::Object { expects_key, .. }) = self.containers.last_mut() {
                    if *expects_key {
                        *expects_key = false;
                        self.key_from = Some(end);
                    }
                }
            }
            b'{' => self.containers.push(Container::Object {
                map: std::mem::take(&mut self.map_next),
                tags: Vec::new(),
                key: None,
                expects_key: true,
                tag_from: None,
            }),
            b'[' => self.containers.push(Container::Array { index: 0 }),
            b'}' | b']' => {
                self.containers.pop();
            }
            b':' => {
                if let Some(Container::Object {
                    map: false,
                    key: Some(key),
                    tag_from,
                    ..
                }) = self.containers.last_mut()
                {
                    if key.as_str() == "kind" {
                        *tag_from = Some(self.text.len());
                    }
                }
            }
            b',' => match self.containers.last_mut() {
                Some(Container::Array { index }) => *index += 1,
                Some(Container::Object {
                    tags,
                    expects_key,
                    tag_from,
                    ..
                }) => {
                    if let Some(from) = tag_from.take() {
                        tags.extend(tag(&self.text[from..end]));
                    }
                    *expects_key = true;
                }
                None => {}
            },
            _ => {}
        }
    }

    /// The place of the value about to be written.
    fn place(&self) -> Vec<String> {
        let mut place = Vec::new();
        for container in &self.containers {
            match container {
                Container::Array { index } => place.push(index.to_string()),
                Container::Object { tags, key, .. } => {
                    place.extend(tags.iter().cloned());
                    place.extend(key.iter().cloned());
                }
            }
        }
        place
    }

    fn begin(&mut self, name: &'static str) {
        let role = if self.unentered > 0 || matches!(name, "TopoName" | "Picked") {
            Role::Unentered
        } else if name == "Dimension" {
            Role::Dimension {
                place: self.place(),
                from: self.text.len(),
            }
        } else {
            Role::Entered
        };
        if !matches!(role, Role::Entered) {
            self.unentered += 1;
        }
        self.structs.push(role);
    }

    fn end(&mut self) {
        match self.structs.pop() {
            Some(Role::Dimension { place, from }) => {
                self.unentered -= 1;
                if let Some(found) = dimension(&self.text[from..]) {
                    self.found.insert(place, found);
                }
            }
            Some(Role::Unentered) => self.unentered -= 1,
            Some(Role::Entered) | None => {}
        }
    }
}

fn json(text: &[u8]) -> Option<Value> {
    std::str::from_utf8(text)
        .ok()
        .and_then(|text| crate::native::replay::value(text).ok())
}

/// The text of a JSON string.
fn string(text: &[u8]) -> Option<String> {
    match json(text)? {
        Value::String(text) => Some(text),
        _ => None,
    }
}

/// The tag a `kind` member's value names: a string, a whole number, or a
/// boolean.
fn tag(text: &[u8]) -> Option<String> {
    match json(text)? {
        Value::String(text) => Some(text),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) if !number.is_f64() => Some(number.to_string()),
        _ => None,
    }
}

/// The arm and value of a dimension's JSON text.
fn dimension(text: &[u8]) -> Option<Found> {
    let Value::Object(mut members) = json(text)? else {
        return None;
    };
    let Some(Value::String(arm)) = members.remove("kind") else {
        return None;
    };
    if !matches!(arm.as_str(), "literal" | "parameter" | "negated") {
        return None;
    }
    let value = members.remove("value")?;
    Some(Found { arm, value })
}

impl Structs for RefCell<Reader> {
    fn begin(&self, name: &'static str) {
        self.borrow_mut().begin(name);
    }

    fn end(&self) {
        self.borrow_mut().end();
    }

    fn map(&self) {
        self.borrow_mut().map_next = true;
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
