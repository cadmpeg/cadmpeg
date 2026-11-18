// SPDX-License-Identifier: Apache-2.0
//! A document's JSON Schema, as `schemars` writes it, read as the object
//! positions and definitions a document reaches.
//!
//! A location is a JSON pointer into the schema. A definition is reached at
//! `/$defs/<name>`, and every struct position at the location of its own
//! object schema, so one Rust type is one position whatever route reaches it.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use regex::Regex;
use serde_json::{Map, Value};

use crate::unknown_keys::Step;

/// The keywords that constrain a value without stating a member of it.
const CONSTRAINT_KEYWORDS: [&str; 5] = ["not", "if", "then", "else", "allOf"];

/// Called with each target a walk reaches, the instance path that reaches
/// it, and the members of that value the target does not own.
pub(super) type Visit<'v> = dyn FnMut(&str, &[Step], &[String]) + 'v;

/// One step from a schema location to a child location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Edge {
    /// An object member.
    Property(String),
    /// Every element of an array.
    Item,
    /// One positional element of an array.
    Prefix(usize),
    /// A map value, under the pattern or `additionalProperties` schema.
    Entry(Option<String>),
    /// One branch of `anyOf` or `oneOf`.
    Branch(&'static str, usize),
    /// The definition a `$ref` names.
    Ref(String),
}

/// How an object schema treats the keys it does not list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    /// The object states its key set: every other key is undeclared.
    Struct,
    /// The keys are data, under `patternProperties` or an
    /// `additionalProperties` schema.
    Map,
    /// `additionalProperties: true`: the object admits every key.
    Open,
    /// The value is not an object.
    Other,
}

/// The JSON Schema of a document.
pub(super) struct Schema<'a> {
    root: &'a Value,
    patterns: RefCell<HashMap<String, Option<Regex>>>,
    inhabited: BTreeMap<String, bool>,
}

impl<'a> Schema<'a> {
    pub(super) fn new(root: &'a Value) -> Self {
        let mut schema = Self {
            root,
            patterns: RefCell::new(HashMap::new()),
            inhabited: BTreeMap::new(),
        };
        schema.inhabited = schema.definitions().map(|name| (name, false)).collect();
        // A recursive definition is inhabited when a finite value reaches it,
        // so inhabitation is the least fixed point from "none".
        loop {
            let found = schema
                .inhabited
                .iter()
                .filter(|(name, inhabited)| {
                    !**inhabited && schema.node_inhabited(schema.node(&definition(name)))
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            if found.is_empty() {
                break;
            }
            for name in found {
                schema.inhabited.insert(name, true);
            }
        }
        schema
    }

    fn definitions(&self) -> impl Iterator<Item = String> + 'a {
        self.root
            .get("$defs")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(Map::keys)
            .cloned()
    }

    /// The schema node at `location`.
    ///
    /// # Panics
    ///
    /// Panics when `location` names no node: every location this module
    /// hands out names one.
    pub(super) fn node(&self, location: &str) -> &'a Value {
        self.root
            .pointer(location)
            .unwrap_or_else(|| panic!("schema location {location} names no node"))
    }

    /// Whether `location` names a node.
    pub(super) fn has_node(&self, location: &str) -> bool {
        self.root.pointer(location).is_some()
    }

    /// The definitions reachable from the root through `$ref`.
    pub(super) fn reachable_definitions(&self) -> BTreeSet<String> {
        let mut reached = BTreeSet::new();
        let mut pending = Vec::new();
        let mut top = self.root.clone();
        if let Some(fields) = top.as_object_mut() {
            fields.remove("$defs");
        }
        references(&top, &mut pending);
        while let Some(name) = pending.pop() {
            if reached.insert(name.clone()) {
                references(self.node(&definition(&name)), &mut pending);
            }
        }
        reached
    }

    /// Whether some value satisfies the node.
    pub(super) fn inhabited(&self, location: &str) -> bool {
        self.node_inhabited(self.node(location))
    }

    fn node_inhabited(&self, node: &Value) -> bool {
        let Some(fields) = node.as_object() else {
            return node != &Value::Bool(false);
        };
        if fields.get("not").is_some_and(admits_everything) {
            return false;
        }
        if let Some(name) = reference(node) {
            if !self.inhabited.get(name).copied().unwrap_or(false) {
                return false;
            }
        }
        if fields.contains_key("const") || fields.contains_key("enum") {
            return true;
        }
        for keyword in ["anyOf", "oneOf"] {
            if let Some(branches) = fields.get(keyword).and_then(Value::as_array) {
                return branches.iter().any(|branch| self.node_inhabited(branch));
            }
        }
        let types = types(node);
        if types.contains(&"null") {
            return true;
        }
        if types.contains(&"object") {
            return stated_properties(node).all(|(_, property)| self.node_inhabited(property));
        }
        if types.contains(&"array") {
            let prefix = fields
                .get("prefixItems")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice);
            let least = min_items(node);
            return prefix
                .iter()
                .take(least)
                .all(|item| self.node_inhabited(item))
                && (least <= prefix.len()
                    || fields
                        .get("items")
                        .is_none_or(|items| self.node_inhabited(items)));
        }
        true
    }

    /// Whether `location` is a coverage target: a definition or a struct
    /// position.
    pub(super) fn is_target(&self, location: &str) -> bool {
        is_definition(location) || self.is_position(location)
    }

    /// Whether `location` is a struct position.
    pub(super) fn is_position(&self, location: &str) -> bool {
        !under_constraint(location) && shape(self.node(location)) == Shape::Struct
    }

    /// Whether the node admits `null`.
    pub(super) fn nullable(&self, node: &Value) -> bool {
        if types(node).contains(&"null") {
            return true;
        }
        if ["anyOf", "oneOf"].iter().any(|keyword| {
            node.get(keyword)
                .and_then(Value::as_array)
                .is_some_and(|branches| branches.iter().any(|branch| self.nullable(branch)))
        }) {
            return true;
        }
        reference(node).is_some_and(|name| self.nullable(self.node(&definition(name))))
    }

    /// Every struct position, and every reachable definition, split by
    /// whether a value can reach it.
    pub(super) fn targets(&self) -> (BTreeSet<String>, BTreeSet<String>) {
        let mut inhabited = BTreeSet::new();
        let mut uninhabited = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut pending = vec![String::new()];
        while let Some(location) = pending.pop() {
            if !seen.insert(location.clone()) {
                continue;
            }
            if self.is_target(&location) {
                if self.inhabited(&location) {
                    inhabited.insert(location.clone());
                } else {
                    uninhabited.insert(location.clone());
                }
            }
            for (_, child) in self.children(&location) {
                pending.push(child);
            }
        }
        (inhabited, uninhabited)
    }

    /// Every location a value can reach whose array states each member
    /// once (`uniqueItems`): a set, a pair-map, or a member list.
    pub(super) fn distinct_arrays(&self) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut pending = vec![String::new()];
        while let Some(location) = pending.pop() {
            if !seen.insert(location.clone()) {
                continue;
            }
            let node = self.node(&location);
            if !under_constraint(&location)
                && types(node).contains(&"array")
                && node.get("uniqueItems") == Some(&Value::Bool(true))
                && self.inhabited(&location)
            {
                found.insert(location.clone());
            }
            for (_, child) in self.children(&location) {
                pending.push(child);
            }
        }
        found
    }

    /// The structural children of `location`, without the constraint
    /// keywords.
    pub(super) fn children(&self, location: &str) -> Vec<(Edge, String)> {
        let node = self.node(location);
        let mut found = Vec::new();
        if let Some(name) = reference(node) {
            found.push((Edge::Ref(name.to_owned()), definition(name)));
        }
        for keyword in ["anyOf", "oneOf"] {
            let branches = node.get(keyword).and_then(Value::as_array);
            for index in 0..branches.map_or(0, Vec::len) {
                found.push((
                    Edge::Branch(keyword, index),
                    format!("{location}/{keyword}/{index}"),
                ));
            }
        }
        for name in node
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(Map::keys)
        {
            found.push((
                Edge::Property(name.clone()),
                format!("{location}/properties/{}", escape(name)),
            ));
        }
        for pattern in node
            .get("patternProperties")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(Map::keys)
        {
            found.push((
                Edge::Entry(Some(pattern.clone())),
                format!("{location}/patternProperties/{}", escape(pattern)),
            ));
        }
        if node
            .get("additionalProperties")
            .is_some_and(Value::is_object)
        {
            found.push((
                Edge::Entry(None),
                format!("{location}/additionalProperties"),
            ));
        }
        if node.get("items").is_some_and(Value::is_object) {
            found.push((Edge::Item, format!("{location}/items")));
        }
        let prefix = node.get("prefixItems").and_then(Value::as_array);
        for index in 0..prefix.map_or(0, Vec::len) {
            found.push((
                Edge::Prefix(index),
                format!("{location}/prefixItems/{index}"),
            ));
        }
        found
    }

    /// Every route from `location` to the next targets below it, each with
    /// the target it reaches.
    pub(super) fn routes(&self, location: &str) -> Vec<(Vec<Edge>, String)> {
        let mut found = Vec::new();
        let mut route = Vec::new();
        self.collect_routes(location, &mut route, &mut found, true);
        found
    }

    fn collect_routes(
        &self,
        location: &str,
        route: &mut Vec<Edge>,
        found: &mut Vec<(Vec<Edge>, String)>,
        start: bool,
    ) {
        if !start && self.is_target(location) {
            found.push((route.clone(), location.to_owned()));
            return;
        }
        for (edge, child) in self.children(location) {
            route.push(edge);
            self.collect_routes(&child, route, found, false);
            route.pop();
        }
    }

    /// Calls `visit` with every target `value` reaches from `location`, the
    /// instance path that reaches it, and the members of that value the
    /// target does not own: those an enclosing node states beside the `$ref`
    /// that names the target, such as the tag of an internally tagged variant.
    pub(super) fn walk(
        &self,
        location: &str,
        value: &Value,
        path: &mut Vec<Step>,
        foreign: &[String],
        visit: &mut Visit<'_>,
    ) {
        self.walk_where(
            location,
            value,
            path,
            foreign,
            &|location| self.is_target(location),
            visit,
        );
    }

    /// The walk of [`Schema::walk`], visiting the locations `keep` admits.
    pub(super) fn walk_where(
        &self,
        location: &str,
        value: &Value,
        path: &mut Vec<Step>,
        foreign: &[String],
        keep: &dyn Fn(&str) -> bool,
        visit: &mut Visit<'_>,
    ) {
        let node = self.node(location);
        if keep(location) {
            visit(location, path, foreign);
        }
        if let Some(name) = reference(node) {
            let mut beside = foreign.to_vec();
            beside.extend(
                node.get("properties")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flat_map(Map::keys)
                    .cloned(),
            );
            self.walk_where(&definition(name), value, path, &beside, keep, visit);
        }
        for keyword in ["anyOf", "oneOf"] {
            if node.get(keyword).is_some() {
                if let Some(index) = self.branch_of(node, keyword, value) {
                    let branch = format!("{location}/{keyword}/{index}");
                    self.walk_where(&branch, value, path, foreign, keep, visit);
                }
            }
        }
        match value {
            Value::Object(fields) => {
                for (key, field) in fields {
                    let Some(child) = self.member_location(location, node, key) else {
                        continue;
                    };
                    path.push(Step::Key(key.clone()));
                    self.walk_where(&child, field, path, &[], keep, visit);
                    path.pop();
                }
            }
            Value::Array(items) => {
                let prefix = node
                    .get("prefixItems")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                for (index, item) in items.iter().enumerate() {
                    let child = if index < prefix {
                        format!("{location}/prefixItems/{index}")
                    } else if node.get("items").is_some_and(Value::is_object) {
                        format!("{location}/items")
                    } else {
                        continue;
                    };
                    path.push(Step::Index(index));
                    self.walk_where(&child, item, path, &[], keep, visit);
                    path.pop();
                }
            }
            _ => {}
        }
    }

    /// The location of the member `key` of an object value of `node`.
    pub(super) fn member_location(
        &self,
        location: &str,
        node: &Value,
        key: &str,
    ) -> Option<String> {
        if node
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| properties.contains_key(key))
        {
            return Some(format!("{location}/properties/{}", escape(key)));
        }
        if let Some(patterns) = node.get("patternProperties").and_then(Value::as_object) {
            return patterns
                .keys()
                .find(|pattern| self.pattern_matches(pattern, key))
                .map(|pattern| format!("{location}/patternProperties/{}", escape(pattern)));
        }
        node.get("additionalProperties")
            .filter(|schema| schema.is_object())
            .map(|_| format!("{location}/additionalProperties"))
    }

    /// The branch of `node`'s `keyword` list that `value` satisfies.
    pub(super) fn branch_of(&self, node: &Value, keyword: &str, value: &Value) -> Option<usize> {
        let branches = node.get(keyword)?.as_array()?;
        let candidates = branches
            .iter()
            .enumerate()
            .filter(|(_, branch)| self.compatible(branch, value))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [only] => Some(*only),
            _ => candidates
                .into_iter()
                .find(|index| self.matches(&branches[*index], value)),
        }
    }

    /// A cheap test that `value` can satisfy `node`: its JSON type and the
    /// constant tags of its members.
    fn compatible(&self, node: &Value, value: &Value) -> bool {
        let node = match reference(node) {
            Some(name) if node.get("type").is_none() => self.node(&definition(name)),
            _ => node,
        };
        if let Some(constant) = node.get("const") {
            return constant == value;
        }
        if !type_admits(node, value) {
            return false;
        }
        let (Some(properties), Some(fields)) = (
            node.get("properties").and_then(Value::as_object),
            value.as_object(),
        ) else {
            return true;
        };
        properties.iter().all(
            |(key, property)| match (property.get("const"), fields.get(key)) {
                (Some(constant), Some(field)) => constant == field,
                (Some(_), None) => false,
                _ => true,
            },
        )
    }

    /// Whether `value` satisfies `node`, for the keywords `schemars` writes.
    pub(super) fn matches(&self, node: &Value, value: &Value) -> bool {
        let Some(fields) = node.as_object() else {
            return node != &Value::Bool(false);
        };
        if let Some(name) = reference(node) {
            if !self.matches(self.node(&definition(name)), value) {
                return false;
            }
        }
        if !type_admits(node, value) {
            return false;
        }
        if fields
            .get("const")
            .is_some_and(|constant| constant != value)
        {
            return false;
        }
        if fields
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| !values.contains(value))
        {
            return false;
        }
        if fields
            .get("not")
            .is_some_and(|refused| self.matches(refused, value))
        {
            return false;
        }
        if fields
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|branches| !branches.iter().any(|branch| self.matches(branch, value)))
        {
            return false;
        }
        if fields
            .get("oneOf")
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches
                    .iter()
                    .filter(|branch| self.matches(branch, value))
                    .count()
                    != 1
            })
        {
            return false;
        }
        if fields
            .get("allOf")
            .and_then(Value::as_array)
            .is_some_and(|parts| !parts.iter().all(|part| self.matches(part, value)))
        {
            return false;
        }
        if let Some(condition) = fields.get("if") {
            let branch = if self.matches(condition, value) {
                fields.get("then")
            } else {
                fields.get("else")
            };
            if branch.is_some_and(|branch| !self.matches(branch, value)) {
                return false;
            }
        }
        match value {
            Value::Object(members) => self.object_matches(node, members),
            Value::Array(items) => self.array_matches(node, items),
            Value::String(text) => self.string_matches(node, text),
            Value::Number(number) => number_matches(node, number),
            _ => true,
        }
    }

    fn object_matches(&self, node: &Value, members: &Map<String, Value>) -> bool {
        if required(node).iter().any(|key| !members.contains_key(*key)) {
            return false;
        }
        let properties = node.get("properties").and_then(Value::as_object);
        members.iter().all(|(key, member)| {
            if let Some(property) = properties.and_then(|properties| properties.get(key)) {
                return self.matches(property, member);
            }
            if let Some(patterns) = node.get("patternProperties").and_then(Value::as_object) {
                let matching = patterns
                    .iter()
                    .filter(|(pattern, _)| self.pattern_matches(pattern, key))
                    .collect::<Vec<_>>();
                if !matching.is_empty() {
                    return matching
                        .iter()
                        .all(|(_, schema)| self.matches(schema, member));
                }
            }
            match node.get("additionalProperties") {
                Some(Value::Bool(false)) => false,
                Some(schema @ Value::Object(_)) => self.matches(schema, member),
                _ => true,
            }
        })
    }

    fn array_matches(&self, node: &Value, items: &[Value]) -> bool {
        if items.len() < min_items(node) {
            return false;
        }
        if node
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|most| u64::try_from(items.len()).is_ok_and(|length| length > most))
        {
            return false;
        }
        let prefix = node
            .get("prefixItems")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        items.iter().enumerate().all(|(index, item)| {
            prefix.get(index).map_or_else(
                || {
                    node.get("items")
                        .is_none_or(|schema| self.matches(schema, item))
                },
                |schema| self.matches(schema, item),
            )
        })
    }

    fn string_matches(&self, node: &Value, text: &str) -> bool {
        if node
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|least| {
                u64::try_from(text.chars().count()).is_ok_and(|length| length < least)
            })
        {
            return false;
        }
        node.get("pattern")
            .and_then(Value::as_str)
            .is_none_or(|pattern| self.pattern_matches(pattern, text))
    }

    /// Whether `text` matches the ECMA-262 `pattern`.
    ///
    /// The `regex` crate has no look-around; `(?![\s\S])`, the one form the
    /// schema writes, asserts the end of the text and is read as `\z`. A
    /// pattern the crate still cannot compile matches every text.
    pub(super) fn pattern_matches(&self, pattern: &str, text: &str) -> bool {
        let mut cache = self.patterns.borrow_mut();
        let compiled = cache
            .entry(pattern.to_owned())
            .or_insert_with(|| Regex::new(&pattern.replace(r"(?![\s\S])", r"\z")).ok());
        compiled.as_ref().is_none_or(|regex| regex.is_match(text))
    }
}

/// How the object schema at `node` treats keys it does not list.
pub(super) fn shape(node: &Value) -> Shape {
    let is_object = types(node).contains(&"object")
        || (node.get("type").is_none() && node.get("properties").is_some());
    if !is_object {
        return Shape::Other;
    }
    if node.get("patternProperties").is_some() {
        return Shape::Map;
    }
    match node.get("additionalProperties") {
        Some(Value::Bool(true)) => Shape::Open,
        Some(Value::Object(_)) => Shape::Map,
        _ => Shape::Struct,
    }
}

/// Every member of the object schema `node` with whether a value may
/// leave it out: whether it is absent from `required`.
pub(super) fn properties(node: &Value) -> Vec<(&str, &Value, bool)> {
    let required = required(node);
    node.get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, property)| (name.as_str(), property, !required.contains(name.as_str())))
        .collect()
}

/// The members a value of `node` always states.
pub(super) fn stated_properties(node: &Value) -> impl Iterator<Item = (&str, &Value)> {
    properties(node)
        .into_iter()
        .filter(|(_, _, option)| !option)
        .map(|(name, property, _)| (name, property))
}

/// Whether `node` is the schema of an enum whose variants hold no data: a
/// string that is one of the names it or each of its branches states.
pub(super) fn is_name_enum(node: &Value) -> bool {
    let named = |node: &Value| {
        types(node) == ["string"]
            && (node.get("const").is_some_and(Value::is_string)
                || node
                    .get("enum")
                    .and_then(Value::as_array)
                    .is_some_and(|names| !names.is_empty() && names.iter().all(Value::is_string)))
    };
    named(node)
        || ["oneOf", "anyOf"].iter().any(|keyword| {
            node.get(keyword)
                .and_then(Value::as_array)
                .is_some_and(|branches| !branches.is_empty() && branches.iter().all(named))
        })
}

/// The names a data-free enum's schema `node` states, in schema order.
pub(super) fn enum_names(node: &Value) -> Vec<Value> {
    let own = |node: &Value| {
        node.get("const")
            .cloned()
            .into_iter()
            .chain(
                node.get("enum")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .cloned(),
            )
            .collect::<Vec<_>>()
    };
    let mut names = own(node);
    for keyword in ["oneOf", "anyOf"] {
        for branch in node
            .get(keyword)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            names.extend(own(branch));
        }
    }
    names
}

/// The location of definition `name`.
pub(super) fn definition(name: &str) -> String {
    format!("/$defs/{}", escape(name))
}

/// Whether `location` is the location of a definition.
pub(super) fn is_definition(location: &str) -> bool {
    location
        .strip_prefix("/$defs/")
        .is_some_and(|rest| !rest.contains('/'))
}

/// Whether `location` lies under a keyword that constrains rather than
/// states a member.
fn under_constraint(location: &str) -> bool {
    location
        .split('/')
        .any(|segment| CONSTRAINT_KEYWORDS.contains(&segment))
}

/// The definition a node's `$ref` names.
pub(super) fn reference(node: &Value) -> Option<&str> {
    node.get("$ref")?.as_str()?.strip_prefix("#/$defs/")
}

fn references(node: &Value, found: &mut Vec<String>) {
    match node {
        Value::Object(fields) => {
            for (key, field) in fields {
                match (key.as_str(), field) {
                    ("$ref", Value::String(target)) => {
                        if let Some(name) = target.strip_prefix("#/$defs/") {
                            found.push(name.to_owned());
                        }
                    }
                    _ => references(field, found),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                references(item, found);
            }
        }
        _ => {}
    }
}

fn admits_everything(node: &Value) -> bool {
    node == &Value::Bool(true) || node.as_object().is_some_and(Map::is_empty)
}

/// The JSON types `node` names.
pub(super) fn types(node: &Value) -> Vec<&str> {
    match node.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn type_admits(node: &Value, value: &Value) -> bool {
    let kinds = types(node);
    if kinds.is_empty() {
        return true;
    }
    let kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    kinds.contains(&kind) || (kind == "integer" && kinds.contains(&"number"))
}

fn number_matches(node: &Value, number: &serde_json::Number) -> bool {
    let Some(value) = number.as_f64() else {
        return true;
    };
    let bound = |key: &str| node.get(key).and_then(Value::as_f64);
    bound("minimum").is_none_or(|least| value >= least)
        && bound("maximum").is_none_or(|most| value <= most)
        && bound("exclusiveMinimum").is_none_or(|least| value > least)
        && bound("exclusiveMaximum").is_none_or(|most| value < most)
}

pub(super) fn required(node: &Value) -> BTreeSet<&str> {
    node.get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

pub(super) fn min_items(node: &Value) -> usize {
    node.get("minItems")
        .and_then(Value::as_u64)
        .and_then(|least| usize::try_from(least).ok())
        .unwrap_or(0)
}

/// `key` escaped as one JSON pointer segment.
pub(super) fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// The order a parser finishes the values at `left` and `right` in: a value
/// finishes after every value inside it, and siblings finish in document
/// order.
pub(super) fn finish_order(left: &[Step], right: &[Step]) -> Ordering {
    for (left_step, right_step) in left.iter().zip(right) {
        let order = match (left_step, right_step) {
            (Step::Key(left), Step::Key(right)) => left.cmp(right),
            (Step::Index(left), Step::Index(right)) => left.cmp(right),
            _ => Ordering::Equal,
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    right.len().cmp(&left.len())
}
