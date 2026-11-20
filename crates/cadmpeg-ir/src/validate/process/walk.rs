// SPDX-License-Identifier: Apache-2.0
//! What one process record holds, found by serializing it.
//!
//! A process record nests `Choice`s, process dimensions, selections, and
//! constructions through some two hundred fields. Serializing the record
//! into [`Walk`] reaches every one of them: a type is recognised by the name
//! serde gives it, so a field added to a process type is walked without a
//! change here. The walk also measures the record's JSON nesting, which the
//! reader bounds.

use std::fmt;

use serde::ser::{self, Serialize};
use serde_json::{Map, Value};

use super::records::{OwnerKey, ProcessRecords};
use crate::design::dimension::ParamRef;
use crate::design::selection::{Anchor, PickContext};
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
    /// The deepest JSON nesting inside the record, counting the record's own
    /// object as one level.
    pub(super) depth: usize,
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
    let mut walk = Walk::default();
    // The walk's serializer has no failing path; a type whose own
    // `Serialize` fails is reported by the document's serialization.
    match record.serialize(&mut walk) {
        Ok(()) | Err(_) => {}
    }
    walk.found
}

#[derive(Debug)]
pub(super) struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

enum Segment {
    Name(&'static str),
    Index(usize),
    Key(String),
}

#[derive(Default)]
struct Walk {
    path: Vec<Segment>,
    depth: usize,
    found: Found,
}

impl Walk {
    fn nested<T: Serialize + ?Sized>(&mut self, segment: Segment, value: &T) -> Result<(), Error> {
        self.path.push(segment);
        let result = value.serialize(&mut *self);
        self.path.pop();
        result
    }

    fn at(&self) -> At {
        let mut at = String::new();
        for segment in &self.path {
            at.push('/');
            match segment {
                Segment::Name(name) => at.push_str(name),
                Segment::Index(index) => at.push_str(&index.to_string()),
                Segment::Key(key) => at.push_str(key),
            }
        }
        at
    }

    /// Opens `levels` levels of JSON nesting.
    fn open(&mut self, levels: usize) {
        self.depth += levels;
        self.found.depth = self.found.depth.max(self.depth);
    }

    fn close(&mut self, levels: usize) {
        self.depth -= levels;
    }
}

/// What a struct of a recognised type captures while it is walked.
enum Capture {
    Choice(ChoiceSite),
    /// Every field, read back as the named type at the end.
    Whole {
        name: &'static str,
        at: At,
        fields: Map<String, Value>,
    },
    /// One field, whose value is absent where the field is skipped.
    Field {
        name: &'static str,
        at: At,
        field: &'static str,
        value: Option<Value>,
    },
}

impl Capture {
    fn of(name: &'static str, at: At) -> Option<Self> {
        Some(match name {
            "Choice" => Self::Choice(ChoiceSite {
                at,
                has_value: false,
                parameters: Vec::new(),
            }),
            "ParamRef" | "PickContext" => Self::Whole {
                name,
                at,
                fields: Map::new(),
            },
            "WorkpieceSelection" | "ProcessConstruction" => Self::Field {
                name,
                at,
                field: "workpiece",
                value: None,
            },
            "Selection" => Self::Field {
                name,
                at,
                field: "at",
                value: None,
            },
            _ => return None,
        })
    }

    fn field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) {
        match self {
            Self::Choice(choice) => match key {
                "value" => choice.has_value = true,
                "parameters" => choice.parameters = strings(value),
                _ => {}
            },
            Self::Whole { fields, .. } => {
                if let Ok(value) = serde_json::to_value(value) {
                    fields.insert(key.to_owned(), value);
                }
            }
            Self::Field {
                field, value: slot, ..
            } => {
                if key == *field {
                    *slot = serde_json::to_value(value).ok();
                }
            }
        }
    }

    fn finish(self, found: &mut Found) {
        match self {
            Self::Choice(choice) => found.choices.push(choice),
            Self::Whole { name, at, fields } => {
                let value = Value::Object(fields);
                match name {
                    "ParamRef" => {
                        if let Ok(reference) = serde_json::from_value(value) {
                            found.references.push((at, reference));
                        }
                    }
                    _ => {
                        if let Ok(context) = serde_json::from_value(value) {
                            found.contexts.push((at, context));
                        }
                    }
                }
            }
            Self::Field {
                name: "Selection",
                at,
                value,
                ..
            } => {
                if let Some(anchor) = value.and_then(|value| serde_json::from_value(value).ok()) {
                    found.anchors.push((at, anchor));
                }
            }
            Self::Field { at, value, .. } => {
                found
                    .workpieces
                    .push((at, value.and_then(|value| value.as_u64())));
            }
        }
    }
}

fn strings<T: Serialize + ?Sized>(value: &T) -> Vec<String> {
    match serde_json::to_value(value) {
        Ok(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

struct Seq<'a> {
    walk: &'a mut Walk,
    index: usize,
    levels: usize,
    popped: bool,
}

struct MapWalk<'a> {
    walk: &'a mut Walk,
    key: String,
}

struct Struct<'a> {
    walk: &'a mut Walk,
    capture: Option<Capture>,
    levels: usize,
    popped: bool,
}

impl Seq<'_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.index += 1;
        self.walk.nested(Segment::Index(self.index - 1), value)
    }

    fn finish(self) {
        self.walk.close(self.levels);
        if self.popped {
            self.walk.path.pop();
        }
    }
}

impl Struct<'_> {
    fn field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), Error> {
        if let Some(capture) = &mut self.capture {
            capture.field(key, value);
        }
        self.walk.nested(Segment::Name(key), value)
    }

    fn finish(self) {
        if let Some(capture) = self.capture {
            capture.finish(&mut self.walk.found);
        }
        self.walk.close(self.levels);
        if self.popped {
            self.walk.path.pop();
        }
    }
}

macro_rules! leaves {
    ($($method:ident($t:ty)),*) => {$(
        fn $method(self, _: $t) -> Result<(), Error> {
            Ok(())
        }
    )*};
}

impl<'a> ser::Serializer for &'a mut Walk {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Seq<'a>;
    type SerializeTuple = Seq<'a>;
    type SerializeTupleStruct = Seq<'a>;
    type SerializeTupleVariant = Seq<'a>;
    type SerializeMap = MapWalk<'a>;
    type SerializeStruct = Struct<'a>;
    type SerializeStructVariant = Struct<'a>;

    leaves!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_unit_struct(&'static str)
    );

    fn serialize_bytes(self, _: &[u8]) -> Result<(), Error> {
        self.open(1);
        self.close(1);
        Ok(())
    }

    fn serialize_none(self) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.open(1);
        let result = self.nested(Segment::Name(variant), value);
        self.close(1);
        result
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Seq<'a>, Error> {
        self.open(1);
        Ok(Seq {
            walk: self,
            index: 0,
            levels: 1,
            popped: false,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<Seq<'a>, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Seq<'a>, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Seq<'a>, Error> {
        self.open(2);
        self.path.push(Segment::Name(variant));
        Ok(Seq {
            walk: self,
            index: 0,
            levels: 2,
            popped: true,
        })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<MapWalk<'a>, Error> {
        self.open(1);
        Ok(MapWalk {
            walk: self,
            key: String::new(),
        })
    }

    fn serialize_struct(self, name: &'static str, _: usize) -> Result<Struct<'a>, Error> {
        self.open(1);
        let capture = Capture::of(name, self.at());
        Ok(Struct {
            walk: self,
            capture,
            levels: 1,
            popped: false,
        })
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Struct<'a>, Error> {
        self.open(2);
        self.path.push(Segment::Name(variant));
        Ok(Struct {
            walk: self,
            capture: None,
            levels: 2,
            popped: true,
        })
    }
}

impl ser::SerializeSeq for Seq<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.element(value)
    }
    fn end(self) -> Result<(), Error> {
        self.finish();
        Ok(())
    }
}

impl ser::SerializeTuple for Seq<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.element(value)
    }
    fn end(self) -> Result<(), Error> {
        self.finish();
        Ok(())
    }
}

impl ser::SerializeTupleStruct for Seq<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.element(value)
    }
    fn end(self) -> Result<(), Error> {
        self.finish();
        Ok(())
    }
}

impl ser::SerializeTupleVariant for Seq<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.element(value)
    }
    fn end(self) -> Result<(), Error> {
        self.finish();
        Ok(())
    }
}

impl ser::SerializeMap for MapWalk<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        self.key = match serde_json::to_value(key) {
            Ok(Value::String(text)) => text,
            Ok(other) => other.to_string(),
            Err(_) => String::new(),
        };
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.walk
            .nested(Segment::Key(std::mem::take(&mut self.key)), value)
    }
    fn end(self) -> Result<(), Error> {
        self.walk.close(1);
        Ok(())
    }
}

impl ser::SerializeStruct for Struct<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.field(key, value)
    }
    fn end(self) -> Result<(), Error> {
        self.finish();
        Ok(())
    }
}

impl ser::SerializeStructVariant for Struct<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.field(key, value)
    }
    fn end(self) -> Result<(), Error> {
        self.finish();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
