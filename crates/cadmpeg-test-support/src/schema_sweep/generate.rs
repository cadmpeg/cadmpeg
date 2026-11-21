// SPDX-License-Identifier: Apache-2.0
//! Values built from the schema, read back through the compiled readers.
//!
//! The schema states types, tags, and bounds; a reader also checks what the
//! schema cannot state, such as a unit length or an ordered pair. Each value
//! is therefore built under a set of choices, one per slot: a number, a string,
//! a branch, an array length, or a known value against a built one. The
//! search starts from the default of every slot and changes slots inside the
//! value the parse error is located in: one at a time, all of them together,
//! two at a time, and one together with a slot the first change created. A
//! change is kept when the read then fails in a value the parser finishes
//! later, or in the same value with a message no kept change has met.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Number, Value};

use super::schema::{
    definition, escape, finish_order, min_items, properties, reference, stated_properties, types,
    Edge, Schema,
};
use super::Read;
use crate::unknown_keys::Step;

/// The slots inside the failing value that pair moves combine.
const PAIRED_SLOTS: usize = 12;

/// The single moves whose new slots a lookahead move tries.
const LOOKAHEAD_MOVES: usize = 24;

/// The shortest word of a reader's message that ranks a slot.
const MESSAGE_WORD: usize = 4;

/// The elements a built array holds beyond its `minItems`, at most.
const LONGEST_EXTRA: usize = 5;

/// Nesting past which a built value takes the shortest alternative of every
/// slot, so a recursive type ends.
const DEPTH_LIMIT: usize = 48;

/// The number candidates, in the order they are tried.
const NUMBERS: [f64; 9] = [0.0, 1.0, 2.0, 3.0, 0.5, 0.25, 10.0, 100.0, -1.0];

/// The string candidates beside the identity, in the order they are tried.
const STRINGS: [&str; 8] = ["zz", "A1", "B2", "AAAA", "0", "a", "image/png", "zz.zz"];

/// A document and the instance path of one value in it.
#[derive(Clone)]
pub(super) struct Sample {
    pub(super) document: Value,
    pub(super) path: Vec<Step>,
}

impl Sample {
    pub(super) fn value(&self) -> Option<&Value> {
        value_at(&self.document, &self.path)
    }
}

/// The accepted values known for each location.
pub(super) type Known = BTreeMap<String, Vec<Value>>;

/// One choice point of a built value.
#[derive(Clone)]
struct Slot {
    key: String,
    path: Vec<Step>,
    alternatives: usize,
}

type Choices = BTreeMap<String, usize>;

struct Builder<'s, 'a> {
    schema: &'s Schema<'a>,
    known: &'s Known,
    choices: &'s Choices,
    slots: Vec<Slot>,
    identities: usize,
    uses: BTreeMap<String, usize>,
}

impl Builder<'_, '_> {
    fn choose(&mut self, path: &[Step], location: &str, alternatives: usize) -> usize {
        let key = format!("{} {location}", pointer(path));
        let chosen = self.choices.get(&key).copied().unwrap_or(0);
        if alternatives > 1 {
            self.slots.push(Slot {
                key,
                path: path.to_vec(),
                alternatives,
            });
        }
        alternatives
            .checked_sub(1)
            .map_or(0, |last| chosen.min(last))
    }

    /// A value of the node at `location`: a known value, or one built from
    /// the schema.
    ///
    /// Each use of a location in one build starts from the next candidate, so
    /// two members of one type differ by default: two directions, two ordered
    /// parameters, or two distinct identities. A scalar offers its known and
    /// its built candidates in one slot.
    fn build(&mut self, location: &str, path: &mut Vec<Step>, depth: usize) -> Value {
        let known = self.known.get(location).map_or(&[][..], Vec::as_slice);
        let first = self.next_use(location);
        if let Some(built) = self.scalar_candidates(location) {
            let mut candidates = known.to_vec();
            candidates.extend(built.into_iter().filter(|value| !known.contains(value)));
            return self.pick(path, location, first, candidates);
        }
        if !known.is_empty() {
            let choice = self.choose(path, &format!("{location} known"), known.len() + 1);
            if choice < known.len() {
                return known[(first + choice) % known.len()].clone();
            }
        }
        self.build_fresh(location, path, depth)
    }

    fn next_use(&mut self, location: &str) -> usize {
        let uses = self.uses.entry(location.to_owned()).or_default();
        *uses += 1;
        *uses - 1
    }

    /// The candidate `first` places after the slot's choice.
    fn pick(
        &mut self,
        path: &[Step],
        location: &str,
        first: usize,
        candidates: Vec<Value>,
    ) -> Value {
        if candidates.is_empty() {
            return Value::Null;
        }
        let choice = self.choose(path, location, candidates.len());
        let index = (first + choice) % candidates.len();
        candidates.into_iter().nth(index).unwrap_or(Value::Null)
    }

    /// The built candidates of a scalar node, or `None` for another node.
    fn scalar_candidates(&mut self, location: &str) -> Option<Vec<Value>> {
        let node = self.schema.node(location);
        if ["const", "$ref", "oneOf", "anyOf"]
            .iter()
            .any(|keyword| node.get(keyword).is_some())
        {
            return None;
        }
        if let Some(values) = node.get("enum").and_then(Value::as_array) {
            return Some(values.clone());
        }
        let mut kinds = types(node);
        if kinds.len() > 1 {
            kinds.retain(|kind| *kind != "null");
        }
        let candidates = match kinds.as_slice() {
            ["string"] => {
                self.identities += 1;
                let identity = format!("zz:sweep:zz#{}", self.identities);
                std::iter::once(identity.as_str())
                    .chain(STRINGS)
                    .map(Value::from)
                    .collect::<Vec<_>>()
            }
            ["integer"] => {
                let least = node
                    .get("minimum")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .max(0);
                (least..least + 4).chain([-1]).map(Value::from).collect()
            }
            ["number"] => NUMBERS
                .iter()
                .filter_map(|number| Number::from_f64(*number).map(Value::Number))
                .collect(),
            ["boolean"] => vec![Value::Bool(false), Value::Bool(true)],
            _ => return None,
        };
        Some(
            candidates
                .into_iter()
                .filter(|value| self.schema.matches(node, value))
                .collect(),
        )
    }

    /// A value of the node at `location`, built from its schema.
    fn build_fresh(&mut self, location: &str, path: &mut Vec<Step>, depth: usize) -> Value {
        let node = self.schema.node(location);
        if let Some(constant) = node.get("const") {
            return constant.clone();
        }
        if let Some(candidates) = self.scalar_candidates(location) {
            let first = self.next_use(location);
            return self.pick(path, location, first, candidates);
        }
        if let Some(name) = reference(node) {
            let value = self.build(&definition(name), path, depth + 1);
            return self.with_own_members(location, value, path, depth);
        }
        for keyword in ["oneOf", "anyOf"] {
            if let Some(branches) = node.get(keyword).and_then(Value::as_array) {
                let mut order = (0..branches.len())
                    .filter(|index| {
                        self.schema
                            .inhabited(&format!("{location}/{keyword}/{index}"))
                    })
                    .collect::<Vec<_>>();
                order.sort_by_key(|index| self.schema.nullable(&branches[*index]));
                if depth > DEPTH_LIMIT {
                    order.sort_by_key(|index| !is_leaf(&branches[*index]));
                }
                let index = self.choose(path, location, order.len());
                let Some(branch) = order.get(index) else {
                    return Value::Null;
                };
                return self.build(&format!("{location}/{keyword}/{branch}"), path, depth + 1);
            }
        }
        let mut kinds = types(node);
        if kinds.len() > 1 {
            kinds.retain(|kind| *kind != "null");
        }
        let kind = kinds
            .get(self.choose(path, &format!("{location} type"), kinds.len()))
            .copied();
        match kind {
            Some("array") => self.build_array(location, path, depth),
            Some("object") => self.build_object(location, path, depth, None),
            _ if node.get("properties").is_some() => self.build_object(location, path, depth, None),
            _ => Value::Null,
        }
    }

    /// `value`, a value of the definition the node at `location` names, with
    /// the members the node states beside its `$ref`, such as the tag of an
    /// internally tagged variant.
    fn with_own_members(
        &mut self,
        location: &str,
        value: Value,
        path: &mut Vec<Step>,
        depth: usize,
    ) -> Value {
        let node = self.schema.node(location);
        let Value::Object(mut fields) = value else {
            return value;
        };
        for (name, _) in stated_properties(node) {
            if fields.contains_key(name) {
                continue;
            }
            path.push(Step::Key(name.to_owned()));
            let member = self.build(
                &format!("{location}/properties/{}", escape(name)),
                path,
                depth + 1,
            );
            path.pop();
            fields.insert(name.to_owned(), member);
        }
        Value::Object(fields)
    }

    /// The members an object of `location` always states, without `skip`.
    fn build_object(
        &mut self,
        location: &str,
        path: &mut Vec<Step>,
        depth: usize,
        skip: Option<&str>,
    ) -> Value {
        let node = self.schema.node(location);
        let mut object = Map::new();
        for (name, _, option) in properties(node) {
            if Some(name) == skip {
                continue;
            }
            let member = format!("{location}/properties/{}", escape(name));
            path.push(Step::Key(name.to_owned()));
            let stated = !option
                || (depth <= DEPTH_LIMIT
                    && self.choose(path, &format!("{member} present"), 2) == 1);
            if stated {
                let value = self.build(&member, path, depth + 1);
                object.insert(name.to_owned(), value);
            }
            path.pop();
        }
        Value::Object(object)
    }

    fn build_array(&mut self, location: &str, path: &mut Vec<Step>, depth: usize) -> Value {
        let node = self.schema.node(location);
        let least = min_items(node);
        let most = node
            .get("maxItems")
            .and_then(Value::as_u64)
            .and_then(|most| usize::try_from(most).ok())
            .unwrap_or(least + LONGEST_EXTRA)
            .min(least + LONGEST_EXTRA);
        let mut lengths = [1, 0, 2, 3, 4, 5]
            .into_iter()
            .map(|extra| least + extra)
            .filter(|length| *length <= most)
            .collect::<Vec<_>>();
        if depth > DEPTH_LIMIT {
            lengths.sort_unstable();
        }
        let length = lengths
            .get(self.choose(path, &format!("{location} length"), lengths.len()))
            .copied()
            .unwrap_or(least);
        let mut items = Vec::new();
        for index in 0..length {
            path.push(Step::Index(index));
            let item = self.build(&item_location(location, node, index), path, depth + 1);
            path.pop();
            items.push(item);
        }
        Value::Array(items)
    }

    /// `existing`, the value at `location`, with the target at the end of
    /// `route` built in place.
    ///
    /// The route keeps every member of `existing` it does not pass through,
    /// and the first element or entry of a collection it passes through.
    fn place(
        &mut self,
        existing: Option<&Value>,
        location: &str,
        route: &[Edge],
        path: &mut Vec<Step>,
        depth: usize,
    ) -> Value {
        let Some((edge, rest)) = route.split_first() else {
            return self.build_fresh(location, path, depth);
        };
        let node = self.schema.node(location);
        match edge {
            Edge::Ref(name) => {
                let value = self.place(existing, &definition(name), rest, path, depth + 1);
                self.with_own_members(location, value, path, depth)
            }
            Edge::Branch(keyword, index) => {
                let child = format!("{location}/{keyword}/{index}");
                let kept = existing
                    .filter(|value| self.schema.branch_of(node, keyword, value) == Some(*index));
                self.place(kept, &child, rest, path, depth + 1)
            }
            Edge::Property(name) => {
                let mut object = match self.kept(existing, location, path) {
                    Some(Value::Object(fields)) => fields.clone(),
                    _ => match self.build_object(location, path, depth, Some(name)) {
                        Value::Object(fields) => fields,
                        _ => Map::new(),
                    },
                };
                path.push(Step::Key(name.clone()));
                let value = self.place(
                    object.get(name),
                    &format!("{location}/properties/{}", escape(name)),
                    rest,
                    path,
                    depth + 1,
                );
                path.pop();
                object.insert(name.clone(), value);
                Value::Object(object)
            }
            Edge::Item | Edge::Prefix(_) => {
                let index = match edge {
                    Edge::Prefix(index) => *index,
                    _ => 0,
                };
                let mut items = match self.kept(existing, location, path) {
                    Some(Value::Array(items)) if items.len() > index => items.clone(),
                    _ => match self.build_array(location, path, depth) {
                        Value::Array(items) => items,
                        _ => Vec::new(),
                    },
                };
                while items.len() <= index {
                    items.push(Value::Null);
                }
                path.push(Step::Index(index));
                let child = item_location(location, node, index);
                let item = self.place(items.get(index), &child, rest, path, depth + 1);
                path.pop();
                items[index] = item;
                Value::Array(items)
            }
            Edge::Entry(pattern) => {
                let child = match pattern {
                    Some(pattern) => format!("{location}/patternProperties/{}", escape(pattern)),
                    None => format!("{location}/additionalProperties"),
                };
                let mut entries = match self.kept(existing, location, path) {
                    Some(Value::Object(fields)) => fields.clone(),
                    _ => Map::new(),
                };
                let key = match entries.keys().next() {
                    Some(key) => key.clone(),
                    None => {
                        self.identities += 1;
                        let identity = format!("zz:sweep:zz#{}", self.identities);
                        let candidates = std::iter::once(identity.as_str())
                            .chain(STRINGS)
                            .filter(|text| {
                                pattern.as_ref().is_none_or(|pattern| {
                                    self.schema.pattern_matches(pattern, text)
                                })
                            })
                            .map(Value::from)
                            .collect();
                        let first = self.next_use(&child);
                        self.pick(path, &format!("{child} key"), first, candidates)
                            .as_str()
                            .unwrap_or("zz")
                            .to_owned()
                    }
                };
                path.push(Step::Key(key.clone()));
                let value = self.place(entries.get(&key), &child, rest, path, depth + 1);
                path.pop();
                entries.insert(key, value);
                Value::Object(entries)
            }
        }
    }
}

impl<'v> Builder<'_, '_> {
    /// `existing`, or `None` when the choice is to build the value the route
    /// passes through afresh, without the members it already holds.
    fn kept(
        &mut self,
        existing: Option<&'v Value>,
        location: &str,
        path: &[Step],
    ) -> Option<&'v Value> {
        let existing = existing?;
        (self.choose(path, &format!("{location} kept"), 2) == 0).then_some(existing)
    }
}

/// The location of element `index` of an array of `node`.
fn item_location(location: &str, node: &Value, index: usize) -> String {
    let prefix = node
        .get("prefixItems")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if index < prefix {
        format!("{location}/prefixItems/{index}")
    } else {
        format!("{location}/items")
    }
}

/// Whether a branch holds no member that can recurse.
fn is_leaf(node: &Value) -> bool {
    node.get("const").is_some() || node.get("properties").is_none() && node.get("$ref").is_none()
}

/// One built document and where its read failed.
struct Attempt {
    document: Value,
    slots: Vec<Slot>,
    failure: Option<Failure>,
}

/// The value a read failed in, and its message without digits.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Failure {
    at: Vec<Step>,
    message: String,
}

/// The route one search builds along: the value of `from` at `base`, and the
/// edges from it to the target.
struct Goal<'g> {
    base: &'g Sample,
    existing: &'g Value,
    from: &'g str,
    route: &'g [Edge],
}

/// The search for a document that reaches one target.
pub(super) struct Search<'s, 'a> {
    pub(super) schema: &'s Schema<'a>,
    pub(super) known: &'s Known,
    pub(super) read: Read<'s>,
    /// The reads the search may spend.
    pub(super) budget: usize,
    /// The documents read.
    pub(super) reads: usize,
}

/// The search state: the choices kept so far and where their read failed.
struct Position {
    choices: Choices,
    slots: Vec<Slot>,
    failure: Failure,
}

/// What a run of trial moves found.
enum Outcome {
    Accepted(Value),
    Progress(Position),
    Stalled,
    Exhausted,
}

/// The state one search carries across its moves.
struct Progress {
    budget: usize,
    /// Every failure a kept move has met at the value it failed in.
    seen: BTreeSet<Failure>,
}

impl Search<'_, '_> {
    /// A document the readers accept that reaches the end of `route` from the
    /// value of `from` at `base`, or `None` when the budget runs out.
    pub(super) fn reach(&mut self, base: &Sample, from: &str, route: &[Edge]) -> Option<Value> {
        let goal = Goal {
            base,
            existing: base.value()?,
            from,
            route,
        };
        self.search(&goal)
    }

    /// A move is kept when its read fails in a value the parser finishes
    /// later, or in the same value with a message no kept move has met: a
    /// reader that checks its members after reading them reports each check
    /// at the value that owns them.
    fn search(&mut self, goal: &Goal<'_>) -> Option<Value> {
        let choices = Choices::new();
        let first = self.attempt(goal, &choices)?;
        let Some(failure) = first.failure else {
            return Some(first.document);
        };
        let mut progress = Progress {
            budget: self.budget,
            seen: BTreeSet::from([failure.clone()]),
        };
        let mut position = Position {
            choices,
            slots: first.slots,
            failure,
        };
        loop {
            order_by_relevance(&mut position.slots, &position.failure);
            let inside = position
                .slots
                .iter()
                .filter(|slot| slot.path.starts_with(&position.failure.at))
                .cloned()
                .collect::<Vec<_>>();
            let choices = &position.choices;
            let singles = position
                .slots
                .iter()
                .flat_map(|slot| {
                    alternatives(slot, choices)
                        .map(move |alternative| with(choices, [(slot, alternative)]))
                })
                .collect::<Vec<_>>();
            let mut stalled = Vec::new();
            let outcome = self.run(goal, singles, &position, &mut progress, Some(&mut stalled));
            let outcome = match outcome {
                Outcome::Stalled => {
                    let most = inside
                        .iter()
                        .map(|slot| slot.alternatives)
                        .max()
                        .unwrap_or(0);
                    let together = (1..most).map(|alternative| {
                        with(
                            choices,
                            inside
                                .iter()
                                .map(|slot| (slot, alternative.min(slot.alternatives - 1))),
                        )
                    });
                    let present = position
                        .slots
                        .iter()
                        .map(|slot| slot.key.clone())
                        .collect::<BTreeSet<_>>();
                    let created = stalled.iter().flat_map(|(moved, slots)| {
                        slots
                            .iter()
                            .filter(|slot| {
                                !present.contains(&slot.key)
                                    && slot.path.starts_with(&position.failure.at)
                            })
                            .flat_map(move |slot| {
                                alternatives(slot, moved)
                                    .map(move |alternative| with(moved, [(slot, alternative)]))
                            })
                    });
                    let paired = &inside[..inside.len().min(PAIRED_SLOTS)];
                    let pairs = paired.iter().enumerate().flat_map(|(index, first)| {
                        paired[index + 1..].iter().flat_map(move |second| {
                            alternatives(first, choices).flat_map(move |left| {
                                alternatives(second, choices).map(move |right| {
                                    with(choices, [(first, left), (second, right)])
                                })
                            })
                        })
                    });
                    let trials = together.chain(created).chain(pairs).collect::<Vec<_>>();
                    self.run(goal, trials, &position, &mut progress, None)
                }
                outcome => outcome,
            };
            match outcome {
                Outcome::Accepted(document) => return Some(document),
                Outcome::Progress(next) => position = next,
                Outcome::Stalled | Outcome::Exhausted => return None,
            }
        }
    }

    /// Reads each of `trials` until one is accepted or kept. A trial that is
    /// neither is recorded in `stalled` with its slots.
    fn run(
        &mut self,
        goal: &Goal<'_>,
        trials: Vec<Choices>,
        position: &Position,
        progress: &mut Progress,
        mut stalled: Option<&mut Vec<(Choices, Vec<Slot>)>>,
    ) -> Outcome {
        for trial in trials {
            if progress.budget == 0 {
                return Outcome::Exhausted;
            }
            progress.budget -= 1;
            let Some(attempt) = self.attempt(goal, &trial) else {
                return Outcome::Exhausted;
            };
            let Some(failure) = attempt.failure else {
                return Outcome::Accepted(attempt.document);
            };
            let later = finish_order(&failure.at, &position.failure.at).is_gt();
            let unmet = failure.at == position.failure.at && !progress.seen.contains(&failure);
            if later || unmet {
                progress.seen.insert(failure.clone());
                return Outcome::Progress(Position {
                    choices: trial,
                    slots: attempt.slots,
                    failure,
                });
            }
            if let Some(stalled) = stalled.as_deref_mut() {
                if stalled.len() < LOOKAHEAD_MOVES {
                    stalled.push((trial, attempt.slots));
                }
            }
        }
        Outcome::Stalled
    }

    /// The document built under `choices`, its slots, and where its read
    /// failed.
    fn attempt(&mut self, goal: &Goal<'_>, choices: &Choices) -> Option<Attempt> {
        let mut builder = Builder {
            schema: self.schema,
            known: self.known,
            choices,
            slots: Vec::new(),
            identities: 0,
            uses: BTreeMap::new(),
        };
        let mut path = goal.base.path.clone();
        let value = builder.place(Some(goal.existing), goal.from, goal.route, &mut path, 0);
        let mut document = goal.base.document.clone();
        *value_at_mut(&mut document, &goal.base.path)? = value;
        let (text, spans) = write_with_spans(&document);
        self.reads += 1;
        let failure = (self.read)(&text).err().map(|error| Failure {
            at: failing_path(&spans, error.column()),
            message: error
                .to_string()
                .chars()
                .filter(|character| !character.is_ascii_digit())
                .collect(),
        });
        Some(Attempt {
            document,
            slots: builder.slots,
            failure,
        })
    }
}

/// Every alternative of `slot` but the one `choices` holds.
fn alternatives(slot: &Slot, choices: &Choices) -> impl Iterator<Item = usize> {
    let current = choices.get(&slot.key).copied().unwrap_or(0);
    (0..slot.alternatives).filter(move |alternative| *alternative != current)
}

/// `choices` with each slot set to its alternative.
fn with<'s>(choices: &Choices, moves: impl IntoIterator<Item = (&'s Slot, usize)>) -> Choices {
    let mut moved = choices.clone();
    for (slot, alternative) in moves {
        moved.insert(slot.key.clone(), alternative);
    }
    moved
}

/// Puts the slots inside the failing value first, then the slots of the
/// values that enclose it, nearest first. Among the slots inside, those whose
/// key holds a word of the message come first, since a reader's message names
/// the member or type it refuses, then the deepest.
fn order_by_relevance(slots: &mut [Slot], failure: &Failure) {
    let words = failure
        .message
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|word| word.len() >= MESSAGE_WORD)
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    slots.sort_by_cached_key(|slot| {
        let shared = slot
            .path
            .iter()
            .zip(&failure.at)
            .take_while(|(left, right)| left == right)
            .count();
        let inside = shared == failure.at.len();
        let key = slot.key.to_ascii_lowercase();
        let named = words
            .iter()
            .filter(|word| key.contains(word.as_str()))
            .count();
        (
            !inside,
            usize::MAX - named,
            if inside {
                usize::MAX - slot.path.len()
            } else {
                failure.at.len() - shared
            },
        )
    });
}

/// The text span of every value of a compact JSON text.
struct Span {
    start: usize,
    end: usize,
    path: Vec<Step>,
}

/// `value` as compact JSON text, with the span of every value in it.
fn write_with_spans(value: &Value) -> (String, Vec<Span>) {
    let mut text = String::new();
    let mut spans = Vec::new();
    write_value(value, &mut Vec::new(), &mut text, &mut spans);
    (text, spans)
}

fn write_value(value: &Value, path: &mut Vec<Step>, text: &mut String, spans: &mut Vec<Span>) {
    let start = text.len();
    match value {
        Value::Object(fields) => {
            text.push('{');
            for (index, (key, field)) in fields.iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                text.push_str(&Value::String(key.clone()).to_string());
                text.push(':');
                path.push(Step::Key(key.clone()));
                write_value(field, path, text, spans);
                path.pop();
            }
            text.push('}');
        }
        Value::Array(items) => {
            text.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                path.push(Step::Index(index));
                write_value(item, path, text, spans);
                path.pop();
            }
            text.push(']');
        }
        _ => text.push_str(&value.to_string()),
    }
    spans.push(Span {
        start,
        end: text.len(),
        path: path.clone(),
    });
}

/// The deepest value whose span holds the byte the parser stopped at.
///
/// `column` counts from 1 on the one line of a compact text, and names the
/// last byte read.
fn failing_path(spans: &[Span], column: usize) -> Vec<Step> {
    let Some(at) = column.checked_sub(1) else {
        return Vec::new();
    };
    spans
        .iter()
        .filter(|span| span.start <= at && at < span.end)
        .max_by_key(|span| span.path.len())
        .map(|span| span.path.clone())
        .unwrap_or_default()
}

/// `path` as a JSON pointer.
pub(super) fn pointer(path: &[Step]) -> String {
    let mut text = String::new();
    for step in path {
        text.push('/');
        match step {
            Step::Key(key) => text.push_str(&escape(key)),
            Step::Index(index) => text.push_str(&index.to_string()),
        }
    }
    text
}

pub(super) fn value_at<'v>(value: &'v Value, path: &[Step]) -> Option<&'v Value> {
    let mut node = value;
    for step in path {
        node = match (step, node) {
            (Step::Key(key), Value::Object(fields)) => fields.get(key)?,
            (Step::Index(index), Value::Array(items)) => items.get(*index)?,
            _ => return None,
        };
    }
    Some(node)
}

pub(super) fn value_at_mut<'v>(value: &'v mut Value, path: &[Step]) -> Option<&'v mut Value> {
    let mut node = value;
    for step in path {
        node = match (step, node) {
            (Step::Key(key), Value::Object(fields)) => fields.get_mut(key)?,
            (Step::Index(index), Value::Array(items)) => items.get_mut(*index)?,
            _ => return None,
        };
    }
    Some(node)
}
