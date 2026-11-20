// SPDX-License-Identifier: Apache-2.0
//! D16 (§5.11): in a `FeatureStateOverride.definition`, of a configuration or
//! an arrangement, every `Dimension` at a place the base feature's definition
//! also has equals the base's; a dimension difference is a parameter
//! override.
//!
//! A place is the path of field names, variant names, map keys, and list
//! indices from the definition to the dimension. The dimensions of a
//! definition are found by serializing it: `Dimension` is the adjacently
//! tagged enum serde names `Dimension`, tagged `literal`, `parameter`, or
//! `negated`. Names and picks hold no dimension and are not entered, so the
//! walk nests only as deep as the definition's constructions. Two literal
//! reals are equal within the document's linear and angular tolerances.

use std::collections::BTreeMap;
use std::fmt;

use serde::ser::{self, Impossible, Serialize};
use serde_json::Value;

use crate::design::configuration::Overrides;
use crate::design::feature::FeatureDefinition;
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
                super::error(findings, holder, format_args!(
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
    let mut path = Vec::new();
    let mut found = BTreeMap::new();
    definition
        .serialize(Collect {
            path: &mut path,
            found: &mut found,
        })
        .ok()?;
    Some(found)
}

#[derive(Debug)]
struct Unsupported;

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a value has no place")
    }
}

impl std::error::Error for Unsupported {}

impl ser::Error for Unsupported {
    fn custom<T: fmt::Display>(_: T) -> Self {
        Self
    }
}

/// Collects the dimensions below the place `path` names.
struct Collect<'c> {
    path: &'c mut Vec<String>,
    found: &'c mut Places,
}

impl Collect<'_> {
    fn reborrow(&mut self) -> Collect<'_> {
        Collect {
            path: &mut *self.path,
            found: &mut *self.found,
        }
    }

    fn nested<T: Serialize + ?Sized>(
        &mut self,
        segment: String,
        value: &T,
    ) -> Result<(), Unsupported> {
        self.path.push(segment);
        let result = value.serialize(self.reborrow());
        self.path.pop();
        result
    }
}

macro_rules! leaves {
    ($($method:ident($type:ty)),* $(,)?) => {$(
        fn $method(self, _: $type) -> Result<(), Unsupported> {
            Ok(())
        }
    )*};
}

impl<'c> ser::Serializer for Collect<'c> {
    type Ok = ();
    type Error = Unsupported;
    type SerializeSeq = List<'c>;
    type SerializeTuple = List<'c>;
    type SerializeTupleStruct = List<'c>;
    type SerializeTupleVariant = List<'c>;
    type SerializeMap = Map<'c>;
    type SerializeStruct = Fields<'c>;
    type SerializeStructVariant = Fields<'c>;

    leaves! {
        serialize_bool(bool), serialize_i8(i8), serialize_i16(i16), serialize_i32(i32),
        serialize_i64(i64), serialize_u8(u8), serialize_u16(u16), serialize_u32(u32),
        serialize_u64(u64), serialize_f32(f32), serialize_f64(f64), serialize_char(char),
        serialize_str(&str), serialize_bytes(&[u8]), serialize_unit_struct(&'static str),
    }

    fn serialize_none(self) -> Result<(), Unsupported> {
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Unsupported> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Unsupported> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), Unsupported> {
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Unsupported> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        mut self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Unsupported> {
        self.nested(variant.to_owned(), value)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<List<'c>, Unsupported> {
        Ok(List::new(self, false))
    }

    fn serialize_tuple(self, _: usize) -> Result<List<'c>, Unsupported> {
        Ok(List::new(self, false))
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<List<'c>, Unsupported> {
        Ok(List::new(self, false))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<List<'c>, Unsupported> {
        self.path.push(variant.to_owned());
        Ok(List::new(self, true))
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Map<'c>, Unsupported> {
        Ok(Map {
            into: self,
            key: None,
        })
    }

    fn serialize_struct(self, name: &'static str, _: usize) -> Result<Fields<'c>, Unsupported> {
        Ok(match name {
            "Dimension" => Fields::Dimension {
                into: self,
                arm: None,
                value: None,
            },
            "TopoName" | "Picked" => Fields::Skipped,
            _ => Fields::Plain {
                into: self,
                pushed: 0,
            },
        })
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Fields<'c>, Unsupported> {
        self.path.push(variant.to_owned());
        Ok(Fields::Plain {
            into: self,
            pushed: 1,
        })
    }
}

/// The elements of a list, each at its index.
struct List<'c> {
    into: Collect<'c>,
    next: usize,
    variant: bool,
}

impl<'c> List<'c> {
    const fn new(into: Collect<'c>, variant: bool) -> Self {
        Self {
            into,
            next: 0,
            variant,
        }
    }

    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Unsupported> {
        let at = self.next;
        self.next += 1;
        self.into.nested(at.to_string(), value)
    }

    fn finish(self) {
        if self.variant {
            self.into.path.pop();
        }
    }
}

macro_rules! list_impl {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for List<'_> {
            type Ok = ();
            type Error = Unsupported;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Unsupported> {
                self.element(value)
            }
            fn end(self) -> Result<(), Unsupported> {
                self.finish();
                Ok(())
            }
        }
    };
}

list_impl!(SerializeSeq, serialize_element);
list_impl!(SerializeTuple, serialize_element);
list_impl!(SerializeTupleStruct, serialize_field);
list_impl!(SerializeTupleVariant, serialize_field);

/// The entries of a map, each at its key.
struct Map<'c> {
    into: Collect<'c>,
    key: Option<String>,
}

impl ser::SerializeMap for Map<'_> {
    type Ok = ();
    type Error = Unsupported;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Unsupported> {
        let text = key
            .serialize(Text)
            .or_else(|_| serde_json::to_string(key).map_err(|_| Unsupported))?;
        self.key = Some(text);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Unsupported> {
        let key = self.key.take().ok_or(Unsupported)?;
        self.into.nested(key, value)
    }

    fn end(self) -> Result<(), Unsupported> {
        Ok(())
    }
}

/// The fields of a struct: each at its name, after the tag a tagged enum
/// writes first; the arm and value of a dimension; or nothing below a name
/// or a pick.
enum Fields<'c> {
    Plain {
        into: Collect<'c>,
        pushed: usize,
    },
    Dimension {
        into: Collect<'c>,
        arm: Option<String>,
        value: Option<Value>,
    },
    Skipped,
}

impl Fields<'_> {
    fn field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Unsupported> {
        match self {
            Fields::Plain { into, pushed } => {
                if key == "kind" {
                    if let Ok(tag) = value.serialize(Text) {
                        into.path.push(tag);
                        *pushed += 1;
                        return Ok(());
                    }
                }
                into.nested(key.to_owned(), value)
            }
            Fields::Dimension {
                arm, value: held, ..
            } => {
                match key {
                    "kind" => *arm = value.serialize(Text).ok(),
                    "value" => *held = serde_json::to_value(value).ok(),
                    _ => {}
                }
                Ok(())
            }
            Fields::Skipped => Ok(()),
        }
    }

    fn finish(self) {
        match self {
            Fields::Plain { into, pushed } => {
                for _ in 0..pushed {
                    into.path.pop();
                }
            }
            Fields::Dimension {
                into,
                arm: Some(arm),
                value: Some(value),
            } if matches!(arm.as_str(), "literal" | "parameter" | "negated") => {
                into.found.insert(into.path.clone(), Found { arm, value });
            }
            Fields::Dimension { .. } | Fields::Skipped => {}
        }
    }
}

macro_rules! fields_impl {
    ($trait:ident) => {
        impl ser::$trait for Fields<'_> {
            type Ok = ();
            type Error = Unsupported;

            fn serialize_field<T: Serialize + ?Sized>(
                &mut self,
                key: &'static str,
                value: &T,
            ) -> Result<(), Unsupported> {
                self.field(key, value)
            }

            fn end(self) -> Result<(), Unsupported> {
                self.finish();
                Ok(())
            }
        }
    };
}

fields_impl!(SerializeStruct);
fields_impl!(SerializeStructVariant);

/// A map key or an enum tag as text.
struct Text;

impl ser::Serializer for Text {
    type Ok = String;
    type Error = Unsupported;
    type SerializeSeq = Impossible<String, Unsupported>;
    type SerializeTuple = Impossible<String, Unsupported>;
    type SerializeTupleStruct = Impossible<String, Unsupported>;
    type SerializeTupleVariant = Impossible<String, Unsupported>;
    type SerializeMap = Impossible<String, Unsupported>;
    type SerializeStruct = Impossible<String, Unsupported>;
    type SerializeStructVariant = Impossible<String, Unsupported>;

    fn serialize_bool(self, value: bool) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_i8(self, value: i8) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_i16(self, value: i16) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_i32(self, value: i32) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_i64(self, value: i64) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_u8(self, value: u8) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_u16(self, value: u16) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_u32(self, value: u32) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_u64(self, value: u64) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_f32(self, _: f32) -> Result<String, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_f64(self, _: f64) -> Result<String, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_char(self, value: char) -> Result<String, Unsupported> {
        Ok(value.to_string())
    }
    fn serialize_str(self, value: &str) -> Result<String, Unsupported> {
        Ok(value.to_owned())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_none(self) -> Result<String, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<String, Unsupported> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<String, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<String, Unsupported> {
        Ok(variant.to_owned())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<String, Unsupported> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Unsupported> {
        Err(Unsupported)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Unsupported> {
        Err(Unsupported)
    }
}

#[cfg(test)]
mod tests;
