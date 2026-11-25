// SPDX-License-Identifier: Apache-2.0
//! The partial forms of a record that names a lost record (§3.3).
//!
//! A record is rewritten through its JSON, typed by the CADIR schema: a pick
//! of a lost record is `Native` (§5.8), an expression that names one is
//! `Source` with no tokens (§5.13), a procedural carrier on one is `Unknown`,
//! a name that names one leaves its entry's native name alone (§5.7), and a
//! collection whose order carries no meaning loses the member that names one.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::design::expr::ExpressionDialect;
use crate::design::selection::{Members, PickTarget, Picked, Unordered};
use crate::geometry::procedural_curves::ProceduralCurveDefinition;
use crate::geometry::procedural_surfaces::ProceduralSurfaceDefinition;

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

/// The family of a schema definition: its name without the type argument
/// after `_` and without the digits that tell apart instances of one
/// generic type.
fn family(name: &str) -> &str {
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

/// Whether `value` holds a string of `gone`.
pub(crate) fn names(value: &Value, gone: &BTreeSet<String>) -> bool {
    match value {
        Value::String(text) => gone.contains(text.as_str()),
        Value::Array(items) => items.iter().any(|item| names(item, gone)),
        Value::Object(map) => map.values().any(|item| names(item, gone)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// The rewrite of the records that name an id of `gone`.
pub(crate) struct Restate<'a> {
    schema: &'a Schema,
    gone: &'a BTreeSet<String>,
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
            dialect: ExpressionDialect::of_codec(codec, process)
                .and_then(|dialect| serde_json::to_value(dialect).ok()),
            native: serde_json::to_value(PickTarget::Native { within: None }).ok()?,
        })
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
            Some("Picked") => return self.pick(value),
            Some("Expression") => return self.expression(value),
            Some(carrier @ ("ProceduralCurveDefinition" | "ProceduralSurfaceDefinition")) => {
                return self.carrier(carrier, value);
            }
            Some("NameEntry") => return self.name_entry(value),
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
            let unordered = family == Some("UnorderedMembers")
                || (node.get("uniqueItems") == Some(&Value::Bool(true))
                    && family != Some("OrderedMembers"));
            let pairs = members
                .is_some_and(|schema| self.schema.resolve(schema).0.get("prefixItems").is_some());
            let removable = |item: &Value| {
                unordered
                    && (!pairs
                        || family == Some("NameTable")
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
        if name == Some("UnorderedMembers_Picked") && !Self::sort_picks(items) {
            return Fate::Lost;
        }
        Fate::Kept
    }

    /// Restates `items`, the picks of an `Unordered` member list, in the
    /// order of their identity keys, and returns whether no two share one.
    fn sort_picks(items: &mut Vec<Value>) -> bool {
        let Ok(picks) = serde_json::from_value::<Vec<Picked>>(Value::Array(std::mem::take(items)))
        else {
            return false;
        };
        let Ok(members) = Members::<Picked, Unordered>::new(picks) else {
            return false;
        };
        match serde_json::to_value(members) {
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

    /// An expression that names a lost id is `Source` with no tokens, an
    /// expression of which nothing is read (§5.13). A `Source` keeps its
    /// dialect.
    fn expression(&self, value: &mut Value) -> Fate {
        if !names(value, self.gone) {
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
                let unknown = unknown.clone();
                if carrier == "ProceduralCurveDefinition" {
                    serde_json::from_value::<ProceduralCurveDefinition>(unknown).is_ok()
                } else {
                    serde_json::from_value::<ProceduralSurfaceDefinition>(unknown).is_ok()
                }
            });
        *value = cached.unwrap_or(bare);
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
