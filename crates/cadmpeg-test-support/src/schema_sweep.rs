// SPDX-License-Identifier: Apache-2.0
//! One spelling for each value of every type the document schema reaches
//! (`docs/cadir-v11.md` §2.5).
//!
//! [`crate::unknown_keys`] and [`crate::spelling`] probe the objects that
//! committed documents state. This sweep probes every object position of the
//! document's JSON Schema, whatever source shape its reader has: a derive, a
//! hand-written `Deserialize`, or a reader in another crate.
//!
//! A target is a reachable definition or a struct position: an object schema
//! that states its key set. Each target is first reached in a supplied
//! document, in a graft as [`crate::unknown_keys`] builds one. A target no
//! document reaches is built from the schema in place, a few schema edges
//! below a target already reached (see `generate`), until no target is left
//! that a route reaches. A value no schema admits, such as a variant that
//! holds an empty enum, is uninhabited and has no target.
//!
//! Every struct position is then read after each edit of its sample: a key no
//! member declares, `null` for each member, each stated member left out, and
//! the first key stated twice. An edit states a second spelling when the
//! readers accept it and the document they write back differs at that object.
//! Every data-free enum is read with its name stated as `{"<name>": null}`,
//! and every array that states each member once with two members swapped
//! (see `values`).

mod generate;
mod schema;
mod values;

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::CadIr;
use serde_json::{Map, Value};

use crate::spelling::{first_difference, write_repeating};
use crate::unknown_keys::{grafted_document, probe_key, probe_values, Step};
use generate::{pointer, value_at, value_at_mut, Known, Sample, Search};
use schema::{is_definition, Edge, Schema};

/// What one sweep of a document schema found.
#[derive(Debug, Default)]
pub struct SchemaSweepReport {
    /// The definitions reachable from the schema root.
    pub definitions: usize,
    /// The reachable definitions some document reached.
    pub definitions_reached: usize,
    /// The struct positions some document reached; each is probed.
    pub positions: usize,
    /// The targets first reached by a supplied document.
    pub from_documents: usize,
    /// The targets first reached by a document built from the schema.
    pub built: usize,
    /// The locations no value can reach.
    pub uninhabited: Vec<String>,
    /// The inhabited targets no document reached.
    pub unreached: Vec<String>,
    /// Each second spelling a position admits, by schema location.
    pub accepting: BTreeSet<String>,
    /// The data-free enum definitions read with their name stated as
    /// `{"<name>": null}`.
    pub names: usize,
    /// The set and pair-map positions that refuse two members swapped.
    pub sets: usize,
    /// The member-list positions that keep two members swapped as stated.
    pub ordered: usize,
    /// The value positions no probe could read, with the reason.
    pub unprobed: Vec<String>,
    /// The documents read.
    pub reads: usize,
}

/// Sweeps every target of the `CadIr` schema `schema`, starting from the
/// serialized `documents`.
///
/// # Errors
///
/// Returns the serde error when the empty document does not serialize.
pub fn sweep(schema: &Value, documents: &[Value]) -> Result<SchemaSweepReport, serde_json::Error> {
    let empty = serde_json::to_value(CadIr::empty())?;
    let read = |text: &str| serde_json::to_value(serde_json::from_str::<CadIr>(text)?);
    Ok(sweep_with(schema, documents, &empty, &read))
}

/// Reads a document's text and writes the document back.
pub(crate) type Read<'r> = &'r dyn Fn(&str) -> Result<Value, serde_json::Error>;

/// The sweep of [`sweep`], reading through `read`.
pub(crate) fn sweep_with(
    schema: &Value,
    documents: &[Value],
    empty: &Value,
    read: Read<'_>,
) -> SchemaSweepReport {
    let schema = Schema::new(schema);
    let (targets, uninhabited) = schema.targets();
    let mut report = SchemaSweepReport {
        definitions: schema.reachable_definitions().len(),
        uninhabited: uninhabited.into_iter().collect(),
        ..SchemaSweepReport::default()
    };
    let mut reached = Reached::default();
    let mut ordered = documents.iter().collect::<Vec<_>>();
    ordered.sort_by_cached_key(|document| document.to_string().len());
    for document in ordered {
        reached.seed(&schema, &targets, document, empty, read, &mut report.reads);
    }
    report.from_documents = reached.samples.len();
    reached.expand(&schema, &targets, read, &mut report.reads);
    report.built = reached.samples.len() - report.from_documents;
    report.unreached = targets
        .iter()
        .filter(|target| !reached.samples.contains_key(*target))
        .cloned()
        .collect();
    report.definitions_reached = reached
        .samples
        .keys()
        .filter(|location| is_definition(location))
        .count();
    for (location, sample) in &reached.samples {
        if schema.is_position(location) {
            report.positions += 1;
            probe(&schema, location, sample, read, &mut report);
        }
    }
    values::probe_names(&schema, &reached.samples, read, &mut report);
    values::probe_orders(&schema, &reached.samples, &reached.known, read, &mut report);
    report
}

/// The distinct accepted values kept for each location.
const KNOWN_VALUES: usize = 3;

/// The JSON values a known value may hold.
const KNOWN_NODES: usize = 256;

/// Whether `value` holds at most `budget` JSON values.
fn within(value: &Value, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    match value {
        Value::Object(fields) => fields.values().all(|field| within(field, budget)),
        Value::Array(items) => items.iter().all(|item| within(item, budget)),
        _ => true,
    }
}

/// One target a document reaches: its location, the instance path, and the
/// members of the value at that path the target does not own.
type Hit = (String, Vec<Step>, Vec<String>);

/// The accepted sample of each target reached so far.
#[derive(Default)]
struct Reached {
    samples: BTreeMap<String, Sample>,
    known: Known,
}

impl Reached {
    /// Records every target `document` reaches, each in the smallest graft
    /// that reads back.
    fn seed(
        &mut self,
        schema: &Schema<'_>,
        targets: &BTreeSet<String>,
        document: &Value,
        empty: &Value,
        read: Read<'_>,
        reads: &mut usize,
    ) {
        let mut grafts = BTreeMap::<String, bool>::new();
        for (location, path, foreign) in hits(schema, targets, document) {
            self.learn(&location, document, &path, &foreign);
            if self.samples.contains_key(&location) {
                continue;
            }
            let graft = grafted_document(document, empty, &path).filter(|(graft, graft_path)| {
                *grafts.entry(anchor(graft_path, &path)).or_insert_with(|| {
                    *reads += 1;
                    read(&graft.to_string()).is_ok()
                })
            });
            let sample = match graft {
                Some((graft, graft_path)) => Sample {
                    document: graft,
                    path: graft_path,
                },
                None => Sample {
                    document: document.clone(),
                    path,
                },
            };
            self.samples.insert(location, sample);
        }
    }

    /// Keeps the value at `path` of an accepted `document` as a known value
    /// of `location`, without the members the target does not own.
    fn learn(&mut self, location: &str, document: &Value, path: &[Step], foreign: &[String]) {
        let known = self.known.entry(location.to_owned()).or_default();
        if known.len() >= KNOWN_VALUES {
            return;
        }
        let Some(value) = value_at(document, path) else {
            return;
        };
        let mut budget = KNOWN_NODES;
        if !within(value, &mut budget) {
            return;
        }
        let mut value = value.clone();
        if let Value::Object(fields) = &mut value {
            for key in foreign {
                fields.remove(key);
            }
        }
        if !known.contains(&value) {
            known.push(value);
        }
    }

    /// Builds each unreached target below a reached one, along routes of one
    /// schema edge first and of up to [`ROUTE_EDGES`] edges when the shorter
    /// routes reach nothing new; a longer route lets the build replace the
    /// members beside the target that its reader checks it against.
    fn expand(
        &mut self,
        schema: &Schema<'_>,
        targets: &BTreeSet<String>,
        read: Read<'_>,
        reads: &mut usize,
    ) {
        let mut parents = Parents::new();
        for from in targets {
            for (route, to) in schema.routes(from) {
                parents.entry(to).or_default().push((from.clone(), route));
            }
        }
        // A failed route is tried once more, after more targets are reached.
        let mut tried = BTreeMap::<String, (usize, usize)>::new();
        let mut edges = 1;
        while edges <= ROUTE_EDGES {
            let mut progress = false;
            for target in targets {
                if self.samples.contains_key(target) {
                    continue;
                }
                for (from, route) in routes_to(&parents, target, edges) {
                    let Some(base) = self.samples.get(&from).cloned() else {
                        continue;
                    };
                    let key = format!("{target} {from} {route:?}");
                    let reached = self.samples.len();
                    let (tries, at) = tried.get(&key).copied().unwrap_or((0, 0));
                    if tries == ROUTE_TRIES || (tries > 0 && at == reached) {
                        continue;
                    }
                    tried.insert(key, (tries + 1, reached));
                    let mut search = Search {
                        schema,
                        known: &self.known,
                        read,
                        budget: if edges == 1 {
                            SEARCH_BUDGET
                        } else {
                            SEARCH_BUDGET / 4
                        },
                        reads: 0,
                    };
                    let found = search.reach(&base, &from, &route);
                    *reads += search.reads;
                    if let Some(document) = found {
                        self.record(schema, targets, &document);
                        progress = true;
                        break;
                    }
                }
            }
            edges = if progress { 1 } else { edges + 1 };
        }
    }

    /// Records every target an accepted built document reaches.
    fn record(&mut self, schema: &Schema<'_>, targets: &BTreeSet<String>, document: &Value) {
        for (location, path, foreign) in hits(schema, targets, document) {
            self.learn(&location, document, &path, &foreign);
            if let Entry::Vacant(vacant) = self.samples.entry(location) {
                vacant.insert(Sample {
                    document: document.clone(),
                    path,
                });
            }
        }
    }
}

/// Every target of `targets` that `document` reaches.
fn hits(schema: &Schema<'_>, targets: &BTreeSet<String>, document: &Value) -> Vec<Hit> {
    let mut found = Vec::new();
    schema.walk(
        "",
        document,
        &mut Vec::new(),
        &[],
        &mut |location, path, foreign| {
            if targets.contains(location) {
                found.push((location.to_owned(), path.to_vec(), foreign.to_vec()));
            }
        },
    );
    found
}

/// The edges into each target: the target one edge above, and the route.
type Parents = BTreeMap<String, Vec<(String, Vec<Edge>)>>;

/// The schema edges a route to an unreached target may span.
const ROUTE_EDGES: usize = 3;

/// The searches one route may run.
const ROUTE_TRIES: usize = 2;

/// The reads one search along a one-edge route may spend; a longer route
/// spends a quarter of it.
const SEARCH_BUDGET: usize = 1500;

/// The routes of one length tried for one target.
const ROUTES_PER_TARGET: usize = 64;

/// Every route of `edges` target-to-target edges that ends at `target`, with
/// the target it starts from.
fn routes_to(parents: &Parents, target: &str, edges: usize) -> Vec<(String, Vec<Edge>)> {
    let direct = parents.get(target).map_or(&[][..], Vec::as_slice);
    if edges <= 1 {
        return direct.to_vec();
    }
    let mut found = Vec::new();
    for (parent, last) in direct {
        for (from, mut route) in routes_to(parents, parent, edges - 1) {
            if found.len() == ROUTES_PER_TARGET {
                return found;
            }
            route.extend(last.iter().cloned());
            found.push((from, route));
        }
    }
    found
}

/// The key one graft's acceptance is cached under: the grafted member the
/// path passes through.
fn anchor(graft_path: &[Step], path: &[Step]) -> String {
    let depth = graft_path
        .iter()
        .position(|step| matches!(step, Step::Index(_)))
        .map_or(1, |depth| depth + 1);
    pointer(&path[..depth.min(path.len())])
}

/// Reads the struct position at `location` after each edit of its sample: a
/// key no member declares, `null` for each member, each member left out, and
/// the first key stated twice.
///
/// An edit states a second spelling when the readers accept it and the
/// document they write back differs at this object: the edit then spells a
/// value that has another spelling. An `Option` left out writes back left
/// out, and a collection left out writes back as an empty one. A sample that
/// does not write back as itself is reported and not edited.
fn probe(
    schema: &Schema<'_>,
    location: &str,
    sample: &Sample,
    read: Read<'_>,
    report: &mut SchemaSweepReport,
) {
    let Some(Value::Object(fields)) = sample.value() else {
        return;
    };
    let at = format!("#{location}");
    let rewrite = |document: &Value, reads: &mut usize| {
        *reads += 1;
        let written = read(&document.to_string()).ok()?;
        match (
            value_at(document, &sample.path),
            value_at(&written, &sample.path),
        ) {
            (Some(stated), Some(written)) => first_difference(stated, written),
            _ => Some(String::new()),
        }
    };
    if let Some(at_member) = rewrite(&sample.document, &mut report.reads) {
        let member = at_member.trim_start_matches('/');
        let direct = !member.is_empty() && !member.contains('/');
        report
            .accepting
            .insert(if !direct || fields.contains_key(member) {
                format!("{at}: the sample does not write back as itself at `{at_member}`")
            } else {
                format!("{at}: `{member}` left out")
            });
        return;
    }
    let mut second = |edit: &dyn Fn(&mut Map<String, Value>)| {
        let mut document = sample.document.clone();
        if let Some(Value::Object(object)) = value_at_mut(&mut document, &sample.path) {
            edit(object);
        }
        rewrite(&document, &mut report.reads).is_some()
    };
    let key = probe_key(fields);
    if probe_values().into_iter().any(|value| {
        second(&|object| {
            object.insert(key.clone(), value.clone());
        })
    }) {
        report
            .accepting
            .insert(format!("{at}: the undeclared key `{key}` is accepted"));
    }
    let declared = schema::properties(schema.node(location))
        .into_iter()
        .map(|(name, _, _)| name.to_owned());
    let names = fields
        .keys()
        .cloned()
        .chain(declared)
        .collect::<BTreeSet<_>>();
    for name in &names {
        if fields.get(name) != Some(&Value::Null)
            && second(&|object| {
                object.insert(name.clone(), Value::Null);
            })
        {
            report
                .accepting
                .insert(format!("{at}: `{name}` written null"));
        }
        if fields.contains_key(name)
            && second(&|object| {
                object.remove(name);
            })
        {
            report.accepting.insert(format!("{at}: `{name}` left out"));
        }
    }
    if let Some(key) = fields.keys().next() {
        let mut text = String::new();
        write_repeating(&sample.document, &sample.path, &mut text);
        report.reads += 1;
        if read(&text).is_ok() {
            report
                .accepting
                .insert(format!("{at}: `{key}` stated twice"));
        }
    }
}

#[cfg(test)]
mod tests;
