// SPDX-License-Identifier: Apache-2.0
//! The partial forms of a record that names a lost record (§3.3).
//!
//! A record is rewritten through its JSON, typed by the CADIR schema: a pick
//! of a lost record is `Native` (§5.8), an expression that names one is
//! `Source` with no tokens (§5.13), a procedural definition on one is
//! `Unknown`, a curve or surface carrier that names one is `Unknown`,
//! a name that names one leaves its entry's native name alone (§5.7), and a
//! collection whose order carries no meaning loses the member that names one.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::design::expr::{ExpressionDialect, Neutral};
use crate::design::selection::{Members, PickTarget, Picked, Unordered};
use crate::geometry::procedural_curves::ProceduralCurveDefinition;
use crate::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};

/// The CADIR schema, read to type each value of a record.
pub(crate) struct Schema(Value);

impl Schema {
    /// The committed CADIR schema, or `None` where it does not parse.
    pub(crate) fn read() -> Option<Self> {
        serde_json::from_str(crate::CADIR_JSON_SCHEMA)
            .ok()
            .map(Self)
    }

    /// The schema of a record of the arena `field` of `home`.
    fn record(&self, home: &str, field: &str) -> Option<&Value> {
        let (home, _) = self.resolve(self.0.get("properties")?.get(home)?);
        home.get("properties")?.get(field)?.get("items")
    }

    /// `node` with its `$ref` links followed, and the name of the last
    /// definition reached. A chain of more links than there are definitions
    /// is a cycle, and stops at the definition it reached.
    fn resolve<'s>(&'s self, mut node: &'s Value) -> (&'s Value, Option<&'s str>) {
        let Some(defs) = self.0.get("$defs").and_then(Value::as_object) else {
            return (node, None);
        };
        let mut name = None;
        for _ in 0..=defs.len() {
            let Some(target) = node
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|link| link.strip_prefix("#/$defs/"))
            else {
                break;
            };
            let Some(definition) = defs.get(target) else {
                break;
            };
            name = Some(target);
            node = definition;
        }
        (node, name)
    }
}

/// The schema definition families the rewrite gives a partial form or a
/// collection rule, and the one definition it sorts again.
pub(super) const PICKED: &str = "Picked";
pub(super) const EXPRESSION: &str = "Expression";
pub(super) const PROCEDURAL_CURVE: &str = "ProceduralCurveDefinition";
pub(super) const PROCEDURAL_SURFACE: &str = "ProceduralSurfaceDefinition";
pub(super) const CURVE_GEOMETRY: &str = "CurveGeometry";
pub(super) const SURFACE_GEOMETRY: &str = "SurfaceGeometry";
pub(super) const NAME_ENTRY: &str = "NameEntry";
pub(super) const UNORDERED_MEMBERS: &str = "UnorderedMembers";
pub(super) const ORDERED_MEMBERS: &str = "OrderedMembers";
pub(super) const NAME_TABLE: &str = "NameTable";
pub(super) const UNORDERED_PICKS: &str = "UnorderedMembers_Picked";

/// The family of a schema definition: its name without the type argument
/// after `_` and without the digits that tell apart instances of one
/// generic type.
pub(super) fn family(name: &str) -> &str {
    let base = name.split('_').next().unwrap_or(name);
    base.trim_end_matches(|c: char| c.is_ascii_digit())
}

/// What a rewrite left of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// The value names no lost id and is unchanged.
    Clean,
    /// The value names no lost id and changed.
    Kept,
    /// The value names a lost id and has no form without it.
    Lost,
}

/// Whether `value` holds a string of `gone`, as a value or as a key.
pub(crate) fn names(value: &Value, gone: &BTreeSet<String>) -> bool {
    match value {
        Value::String(text) => gone.contains(text.as_str()),
        Value::Array(items) => items.iter().any(|item| names(item, gone)),
        Value::Object(map) => map
            .iter()
            .any(|(key, item)| gone.contains(key.as_str()) || names(item, gone)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Whether `expression`, the JSON of an expression, reads back: a neutral
/// tree reads where its text parses within the bound of an expression.
fn reads(expression: &Value) -> bool {
    if expression.get("kind").and_then(Value::as_str) != Some("neutral") {
        return true;
    }
    expression.get("value").is_some_and(|wire| {
        cadmpeg_core::serde_erase::from_json_value::<Neutral<Value>>(wire).is_ok()
    })
}

/// The rewrite of the records that name an id of `gone`.
pub(crate) struct Restate<'a> {
    schema: &'a Schema,
    gone: &'a BTreeSet<String>,
    /// Whether an expression whose tree passes its bound (§2.5) is lost.
    past_the_bound: bool,
    /// The dialect of an expression restated from a neutral tree.
    dialect: Option<Value>,
    /// The target of a pick restated `Native`.
    native: Value,
}

impl<'a> Restate<'a> {
    /// The rewrite of the records of the codec that minted `holder`, a record
    /// of the `process` home where `process`.
    pub(crate) fn new(
        schema: &'a Schema,
        gone: &'a BTreeSet<String>,
        holder: &str,
        process: bool,
    ) -> Option<Self> {
        let codec = holder.split(':').next().unwrap_or_default();
        Some(Self {
            schema,
            gone,
            past_the_bound: false,
            dialect: ExpressionDialect::of_codec(codec, process).and_then(|dialect| {
                cadmpeg_core::serde_erase::serialize_to_json_value(&|adapter| {
                    serde::Serialize::serialize(&dialect, adapter)
                })
                .ok()
            }),
            native: cadmpeg_core::serde_erase::serialize_to_json_value(&|adapter| {
                serde::Serialize::serialize(&PickTarget::Native { within: None }, adapter)
            })
            .ok()?,
        })
    }

    /// The rewrite that also states `Source` each expression whose tree
    /// passes the bound of an expression (§2.5): its text does not read.
    pub(crate) fn past_the_bound(mut self) -> Self {
        self.past_the_bound = true;
        self
    }

    /// Rewrites `record`, a record of the arena `field` of `home`, into the
    /// form that names no lost id, and returns whether it has one.
    pub(crate) fn record(&self, home: &str, field: &str, record: &mut Value) -> bool {
        let Some(schema) = self.schema.record(home, field) else {
            return false;
        };
        self.walk(schema, record) == Fate::Kept && !names(record, self.gone)
    }

    fn walk(&self, schema: &Value, value: &mut Value) -> Fate {
        let (node, name) = self.schema.resolve(schema);
        match name.map(family) {
            Some(PICKED) => return self.pick(value),
            Some(EXPRESSION) => return self.expression(value),
            Some(carrier @ (PROCEDURAL_CURVE | PROCEDURAL_SURFACE)) => {
                return self.carrier(carrier, value);
            }
            Some(NAME_ENTRY) => return self.name_entry(value),
            Some(carrier @ (CURVE_GEOMETRY | SURFACE_GEOMETRY)) => {
                return self.carrier_geometry(carrier, value);
            }
            _ => {}
        }
        if let Some(branches) = ["oneOf", "anyOf", "allOf"]
            .iter()
            .find_map(|key| node.get(*key).and_then(Value::as_array))
        {
            return match self.branch(branches, value) {
                Some(branch) => self.walk(branch, value),
                None => self.opaque(value),
            };
        }
        match value {
            Value::String(text) if self.gone.contains(text.as_str()) => Fate::Lost,
            Value::Array(items) => self.array(node, name, items),
            Value::Object(map) => self.object(node, map),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Fate::Clean,
        }
    }

    /// A value whose type the schema does not tell: it is lost where it
    /// names a lost id.
    fn opaque(&self, value: &Value) -> Fate {
        if names(value, self.gone) {
            Fate::Lost
        } else {
            Fate::Clean
        }
    }

    /// The alternative of `branches` that `value` is: the one whose `kind`
    /// is the value's, else the one alternative of the value's JSON type.
    fn branch<'s>(&'s self, branches: &'s [Value], value: &Value) -> Option<&'s Value> {
        if let Some(kind) = value.get("kind").and_then(Value::as_str) {
            let tagged = branches.iter().find(|branch| {
                let kind_schema = self.schema.resolve(branch).0.pointer("/properties/kind");
                kind_schema.and_then(|schema| schema.get("const")) == Some(&Value::from(kind))
            });
            if tagged.is_some() {
                return tagged;
            }
        }
        let json_type = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        let mut candidates =
            branches
                .iter()
                .filter(|branch| match self.schema.resolve(branch).0.get("type") {
                    None => true,
                    Some(Value::String(stated)) => {
                        stated == json_type || (stated == "integer" && json_type == "number")
                    }
                    Some(Value::Array(stated)) => stated.iter().any(|stated| {
                        stated == json_type || (stated == "integer" && json_type == "number")
                    }),
                    Some(_) => false,
                });
        let first = candidates.next()?;
        candidates.next().is_none().then_some(first)
    }

    /// An object whose keys are the ids of a map loses the entry whose key
    /// is a lost id: the map is keyed by its members' own ids.
    fn object(&self, node: &Value, map: &mut Map<String, Value>) -> Fate {
        let properties = node.get("properties").and_then(Value::as_object);
        let patterned = node
            .get("patternProperties")
            .and_then(Value::as_object)
            .filter(|patterns| patterns.len() == 1)
            .and_then(|patterns| patterns.values().next());
        let values = node
            .get("additionalProperties")
            .filter(|schema| schema.is_object())
            .or(patterned);
        let mut fate = Fate::Clean;
        if values.is_some() {
            let before = map.len();
            map.retain(|key, _| {
                !self.gone.contains(key.as_str())
                    || properties.is_some_and(|properties| properties.contains_key(key))
            });
            if map.len() != before {
                fate = Fate::Kept;
            }
        }
        for (key, value) in map.iter_mut() {
            let child = match properties
                .and_then(|properties| properties.get(key))
                .or(values)
            {
                Some(schema) => self.walk(schema, value),
                None => self.opaque(value),
            };
            match child {
                Fate::Lost => return Fate::Lost,
                Fate::Kept => fate = Fate::Kept,
                Fate::Clean => {}
            }
        }
        fate
    }

    /// An array loses a member that names a lost id where its order carries
    /// no meaning: an `Unordered` member list, or a set. A pair of a map
    /// stated as pairs is removed where its key is lost, and a name table
    /// also releases a key whose entry is lost.
    fn array(&self, node: &Value, name: Option<&str>, items: &mut Vec<Value>) -> Fate {
        let family = name.map(family);
        let positions = node.get("prefixItems").and_then(Value::as_array);
        let members = node.get("items").filter(|schema| schema.is_object());
        let fates: Vec<Fate> = items
            .iter_mut()
            .enumerate()
            .map(|(at, item)| {
                match positions
                    .and_then(|positions| positions.get(at))
                    .or(members)
                {
                    Some(schema) => self.walk(schema, item),
                    None => self.opaque(item),
                }
            })
            .collect();
        if fates.iter().all(|fate| *fate == Fate::Clean) {
            return Fate::Clean;
        }
        if fates.contains(&Fate::Lost) {
            let unordered = family == Some(UNORDERED_MEMBERS)
                || (node.get("uniqueItems") == Some(&Value::Bool(true))
                    && family != Some(ORDERED_MEMBERS));
            let pairs = members
                .is_some_and(|schema| self.schema.resolve(schema).0.get("prefixItems").is_some());
            let removable = |item: &Value| {
                unordered
                    && (!pairs
                        || family == Some(NAME_TABLE)
                        || item.get(0).is_some_and(|key| names(key, self.gone)))
            };
            if items
                .iter()
                .zip(&fates)
                .any(|(item, fate)| *fate == Fate::Lost && !removable(item))
            {
                return Fate::Lost;
            }
            let mut fates = fates.iter();
            items.retain(|_| fates.next() != Some(&Fate::Lost));
        }
        if name == Some(UNORDERED_PICKS) && !Self::sort_picks(items) {
            return Fate::Lost;
        }
        Fate::Kept
    }

    /// Restates `items`, the picks of an `Unordered` member list, in the
    /// order of their identity keys, and returns whether no two share one.
    fn sort_picks(items: &mut Vec<Value>) -> bool {
        let Ok(picks) = cadmpeg_core::serde_erase::from_json_value::<Vec<Picked>>(&Value::Array(
            std::mem::take(items),
        )) else {
            return false;
        };
        let Ok(members) = Members::<Picked, Unordered>::new(picks) else {
            return false;
        };
        match cadmpeg_core::serde_erase::serialize_to_json_value(&|adapter| {
            serde::Serialize::serialize(&members, adapter)
        }) {
            Ok(Value::Array(sorted)) => {
                *items = sorted;
                true
            }
            _ => false,
        }
    }

    /// A pick whose target or `resolved` names a lost id: `resolved`, a
    /// cache, is removed, and the target is `Native` where the pick keeps a
    /// native name or a witness (§5.8).
    fn pick(&self, value: &mut Value) -> Fate {
        if !names(value, self.gone) {
            return Fate::Clean;
        }
        let Some(pick) = value.as_object_mut() else {
            return Fate::Lost;
        };
        let lost: Vec<String> = pick
            .iter()
            .filter(|(_, item)| names(item, self.gone))
            .map(|(key, _)| key.clone())
            .collect();
        if lost.iter().any(|key| key != "target" && key != "resolved") {
            return Fate::Lost;
        }
        if lost.iter().any(|key| key == "target") {
            let stated = |key: &str| pick.get(key).is_some_and(|item| !item.is_null());
            if !stated("native") && !stated("witness") {
                return Fate::Lost;
            }
            pick.insert("target".to_owned(), self.native.clone());
        }
        if lost.iter().any(|key| key == "resolved") {
            pick.remove("resolved");
        }
        Fate::Kept
    }

    /// An expression that names a lost id, or that does not read where
    /// `past_the_bound`, is `Source` with no tokens, an expression of which
    /// nothing is read (§5.13). A `Source` keeps its dialect.
    fn expression(&self, value: &mut Value) -> Fate {
        let lost = names(value, self.gone) || (self.past_the_bound && !reads(value));
        if !lost {
            return Fate::Clean;
        }
        match value.get("kind").and_then(Value::as_str) {
            Some("source") => match value.pointer_mut("/value/tokens") {
                Some(tokens) => {
                    *tokens = Value::Array(Vec::new());
                    Fate::Kept
                }
                None => Fate::Lost,
            },
            Some("neutral") => match &self.dialect {
                Some(dialect) => {
                    *value = serde_json::json!({
                        "kind": "source",
                        "value": {"dialect": dialect, "tokens": []}
                    });
                    Fate::Kept
                }
                None => Fate::Lost,
            },
            _ => Fate::Lost,
        }
    }

    /// A procedural carrier that names a lost id is `Unknown`, with its
    /// solved cache where the cache names none and `Unknown` holds it.
    fn carrier(&self, carrier: &str, value: &mut Value) -> Fate {
        if !names(value, self.gone) {
            return Fate::Clean;
        }
        let bare = serde_json::json!({"kind": "unknown"});
        let cached = value
            .get("cache")
            .filter(|cache| !names(cache, self.gone))
            .map(|cache| serde_json::json!({"kind": "unknown", "cache": cache}))
            .filter(|unknown| {
                if carrier == PROCEDURAL_CURVE {
                    cadmpeg_core::serde_erase::from_json_value::<ProceduralCurveDefinition>(unknown)
                        .is_ok()
                } else {
                    cadmpeg_core::serde_erase::from_json_value::<ProceduralSurfaceDefinition>(
                        unknown,
                    )
                    .is_ok()
                }
            });
        *value = cached.unwrap_or(bare);
        Fate::Kept
    }

    /// A curve or surface carrier that names a lost id, its construction or
    /// its retained record, is `Unknown` (§3.3).
    fn carrier_geometry(&self, carrier: &str, value: &mut Value) -> Fate {
        if !names(value, self.gone) {
            return Fate::Clean;
        }
        let unknown = if carrier == CURVE_GEOMETRY {
            cadmpeg_core::serde_erase::serialize_to_json_value(&|adapter| {
                serde::Serialize::serialize(
                    &CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                    adapter,
                )
            })
        } else {
            cadmpeg_core::serde_erase::serialize_to_json_value(&|adapter| {
                serde::Serialize::serialize(
                    &SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
                    adapter,
                )
            })
        };
        let Ok(unknown) = unknown else {
            return Fate::Lost;
        };
        *value = unknown;
        Fate::Kept
    }

    /// A name-table entry whose name names a lost id keeps its native name
    /// alone (§5.7).
    fn name_entry(&self, value: &mut Value) -> Fate {
        if !names(value, self.gone) {
            return Fate::Clean;
        }
        let Some(entry) = value.as_object_mut() else {
            return Fate::Lost;
        };
        let native = entry
            .get("native")
            .is_some_and(|native| !native.is_null() && !names(native, self.gone));
        let name = entry.get("name").is_some_and(|name| names(name, self.gone));
        if native && name {
            entry.remove("name");
            Fate::Kept
        } else {
            Fate::Lost
        }
    }
}
