// SPDX-License-Identifier: Apache-2.0
//! CADIR document index for graph, join, and inferred schema.
//!
//! Parses the document as JSON and inventories every array arena under
//! `model` and `native.<codec>`. Identity lookup is exact string
//! match against each record's top-level `id`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::item::{ambiguous_message, miss_id_message, unknown_arena_message, ArenaTarget};
use super::{read_input, sniff_kind, ArtifactKind};

/// One addressable JSON-array arena and its records.
#[derive(Debug, Clone)]
pub(crate) struct Arena {
    pub target: ArenaTarget,
    pub records: Vec<Value>,
}

/// A record location valid only in the document whose index constructed it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) struct RecordRef {
    arena: usize,
    rec: usize,
}

/// Indexed CADIR document: arenas, addressable names, and id lookup.
#[derive(Debug, Clone)]
pub(crate) struct CadirDocument {
    arenas: Vec<Arena>,
    /// Exact ID to each record location that carries it.
    by_id: BTreeMap<String, Vec<RecordRef>>,
}

/// Which records of an arena a query selects.
///
/// The two forms are exclusive: a list of requested IDs, or the first N records
/// in arena order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordSelection {
    /// Records matching these IDs, exactly or as a unique suffix.
    Ids(RequestedIds),
    /// The first N records in arena order.
    Head(usize),
}

/// A record-ID request naming at least one ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedIds(Vec<String>);

impl RequestedIds {
    /// Returns the request, or `None` when the list names no ID.
    pub fn new(ids: Vec<String>) -> Option<Self> {
        (!ids.is_empty()).then_some(Self(ids))
    }

    /// Returns the requested IDs in the order they were given.
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

impl clap::Args for RecordSelection {
    fn augment_args(command: clap::Command) -> clap::Command {
        command
            .arg(
                clap::Arg::new("ids")
                    .value_name("ID")
                    .help(
                        "Record IDs (exact or unique suffix); omit for the first record, \
                         conflicts with --head",
                    )
                    .action(clap::ArgAction::Append)
                    .value_parser(clap::value_parser!(String)),
            )
            .arg(
                clap::Arg::new("head")
                    .long("head")
                    .value_name("N")
                    .help("Take the first N records in arena order; conflicts with explicit IDs")
                    .conflicts_with("ids")
                    .value_parser(clap::value_parser!(usize)),
            )
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        Self::augment_args(command)
    }
}

impl clap::FromArgMatches for RecordSelection {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        let ids: Vec<String> = matches
            .get_many::<String>("ids")
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        match (RequestedIds::new(ids), matches.get_one::<usize>("head")) {
            (None, Some(head)) => Ok(Self::Head(*head)),
            (None, None) => Ok(Self::Head(1)),
            (Some(ids), None) => Ok(Self::Ids(ids)),
            (Some(_), Some(_)) => Err(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                "--head cannot be used with explicit record IDs\n",
            )),
        }
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl CadirDocument {
    /// Reads `path`, rejects reports and sidecars, and indexes every array arena.
    pub(crate) fn load(path: &Path, view: &str) -> Result<Self> {
        let bytes = read_input(path)?;
        reject_non_cadir(&bytes, path, view)?;
        Self::from_bytes(&bytes, path)
    }

    pub(crate) fn from_bytes(bytes: &[u8], path: &Path) -> Result<Self> {
        let root: Value = serde_json::from_slice(bytes)
            .with_context(|| format!("parsing the CADIR document {}", path.display()))?;
        Ok(Self::from_value(&root))
    }

    pub(crate) fn from_value(root: &Value) -> Self {
        let mut arenas = Vec::new();
        if let Some(model) = root.get("model").and_then(Value::as_object) {
            for (name, value) in model {
                if let Some(arr) = value.as_array() {
                    arenas.push(Arena {
                        target: ArenaTarget::Model {
                            arena: name.clone(),
                        },
                        records: arr.clone(),
                    });
                }
            }
        }
        if let Some(native) = root.get("native").and_then(Value::as_object) {
            for (codec, namespace) in native {
                let Some(native_arenas) = namespace.as_object() else {
                    continue;
                };
                for (name, value) in native_arenas {
                    if let Some(arr) = value.as_array() {
                        arenas.push(Arena {
                            target: ArenaTarget::Native {
                                codec: codec.clone(),
                                arena: name.clone(),
                            },
                            records: arr.clone(),
                        });
                    }
                }
            }
        }

        let mut by_id: BTreeMap<String, Vec<RecordRef>> = BTreeMap::new();
        for (ai, arena) in arenas.iter().enumerate() {
            for (ri, rec) in arena.records.iter().enumerate() {
                if let Some(id) = record_id(rec) {
                    by_id
                        .entry(id.to_owned())
                        .or_default()
                        .push(RecordRef { arena: ai, rec: ri });
                }
            }
        }

        Self { arenas, by_id }
    }

    pub(crate) fn arenas(&self) -> &[Arena] {
        &self.arenas
    }

    pub(crate) fn id_locations(&self, id: &str) -> Option<&[RecordRef]> {
        self.by_id.get(id).map(Vec::as_slice)
    }

    pub(crate) fn addressable(&self) -> Vec<(String, u64)> {
        self.arenas
            .iter()
            .map(|arena| (arena.target.dotted(), arena.records.len() as u64))
            .collect()
    }

    pub(crate) fn require_arena(&self, target: &ArenaTarget) -> Result<&Arena> {
        self.arenas
            .iter()
            .find(|arena| &arena.target == target)
            .ok_or_else(|| anyhow::anyhow!(unknown_arena_message(target, &self.addressable())))
    }

    pub(crate) fn all_ids(&self) -> std::collections::BTreeSet<String> {
        self.by_id.keys().cloned().collect()
    }

    /// Returns each record with its indexed location.
    pub(crate) fn records(&self) -> impl Iterator<Item = (RecordRef, &Value)> {
        self.arenas.iter().enumerate().flat_map(|(ai, arena)| {
            arena
                .records
                .iter()
                .enumerate()
                .map(move |(ri, record)| (RecordRef { arena: ai, rec: ri }, record))
        })
    }

    /// Returns a record using a location constructed by this document only.
    pub(crate) fn record(&self, location: RecordRef) -> &Value {
        &self.arenas[location.arena].records[location.rec]
    }

    /// Formats a location constructed by this document only.
    pub(crate) fn locator(&self, location: RecordRef) -> String {
        let arena = &self.arenas[location.arena];
        match record_id(self.record(location)) {
            Some(id) => format!("{}#{id}", arena.target.dotted()),
            None => format!("{}#{}", arena.target.dotted(), location.rec),
        }
    }

    /// Selects indexed records by first-N, exact ID, or unique ID suffix.
    pub(crate) fn select_records(
        &self,
        target: &ArenaTarget,
        selection: &RecordSelection,
    ) -> Result<(Vec<RecordRef>, Vec<String>)> {
        let (ai, arena) = self
            .arenas
            .iter()
            .enumerate()
            .find(|(_, arena)| &arena.target == target)
            .ok_or_else(|| anyhow::anyhow!(unknown_arena_message(target, &self.addressable())))?;
        let ids = match selection {
            RecordSelection::Head(head) => {
                let end = (*head).min(arena.records.len());
                return Ok((
                    (0..end).map(|rec| RecordRef { arena: ai, rec }).collect(),
                    Vec::new(),
                ));
            }
            RecordSelection::Ids(ids) => ids.as_slice(),
        };

        let indexed: Vec<(Option<&str>, RecordRef)> = arena
            .records
            .iter()
            .enumerate()
            .map(|(rec, value)| (record_id(value), RecordRef { arena: ai, rec }))
            .collect();
        let all_ids: Vec<String> = indexed
            .iter()
            .filter_map(|(id, _)| id.map(str::to_owned))
            .collect();

        let mut records = Vec::new();
        let mut errors = Vec::new();
        for request in ids {
            match resolve_one(request, indexed.iter().copied()) {
                Ok(record) => records.push(record),
                Err(ResolveError::Ambiguous(matches)) => {
                    errors.push(ambiguous_message(request, &arena.target.dotted(), &matches));
                }
                Err(ResolveError::Missing) => {
                    errors.push(miss_id_message(
                        &arena.target.dotted(),
                        request,
                        arena.records.len() as u64,
                        &all_ids,
                    ));
                }
            }
        }
        Ok((records, errors))
    }
}

/// Top-level JSON-string `id`, if present.
pub(crate) fn record_id(record: &Value) -> Option<&str> {
    record.get("id").and_then(Value::as_str)
}

/// Failure to select one record by ID.
pub(crate) enum ResolveError {
    Missing,
    Ambiguous(Vec<String>),
}

/// Resolves the first exact ID or one unique suffix match.
pub(crate) fn resolve_one<'a, T: Copy>(
    request: &str,
    indexed: impl Iterator<Item = (Option<&'a str>, T)> + Clone,
) -> Result<T, ResolveError> {
    for (id, value) in indexed.clone() {
        if id == Some(request) {
            return Ok(value);
        }
    }
    let mut suffix = Vec::new();
    for (id, value) in indexed {
        if let Some(id) = id {
            if id.ends_with(request) {
                suffix.push((id, value));
            }
        }
    }
    match suffix.as_slice() {
        [] => Err(ResolveError::Missing),
        [(_, value)] => Ok(*value),
        _ => Err(ResolveError::Ambiguous(
            suffix.into_iter().map(|(id, _)| id.to_owned()).collect(),
        )),
    }
}

/// Admits a CADIR document of this build's `IR_VERSION` and refuses the other
/// two artifact kinds with the dump-then-query recipe for `view`.
///
/// Decides on the top-level keys alone, so a caller that goes on to index the
/// document parses the body once.
pub(crate) fn reject_non_cadir(bytes: &[u8], path: &Path, view: &str) -> Result<()> {
    match sniff_kind(bytes, path)? {
        ArtifactKind::Cadir => Ok(()),
        ArtifactKind::Report => bail!(
            "{} is a command report; reports have no arenas. Use \
             `cadmpeg query findings` / `cadmpeg query losses` on the report, or \
             `cadmpeg dump SOURCE -o doc.json && cadmpeg query {view} doc.json …`",
            path.display()
        ),
        ArtifactKind::Sidecar => bail!(
            "{} is a decode sidecar (`<stem>.fidelity.json`); sidecars have no \
             arenas. Run `cadmpeg dump SOURCE -o doc.json && cadmpeg query {view} \
             doc.json …`",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn indexes_model_and_native_arenas_and_skips_non_arrays() {
        let doc = CadirDocument::from_value(&json!({
            "ir_version": "4",
            "model": {
                "faces": [{"id": "f1"}, {"id": "f2"}],
                "empty": [],
                "null_arena": null
            },
            "native": {
                "rhino": {"unknowns": [{"id": "n1"}]}
            }
        }));
        let names: Vec<String> = doc.arenas.iter().map(|a| a.target.dotted()).collect();
        assert!(names.contains(&"model.faces".to_owned()));
        assert!(names.contains(&"model.empty".to_owned()));
        assert!(names.contains(&"native.rhino.unknowns".to_owned()));
        assert!(!names.iter().any(|n| n.contains("null")));
        let location = doc.by_id["f1"][0];
        assert_eq!(doc.arenas[location.arena].target.dotted(), "model.faces");
        assert_eq!(location.rec, 0);
        assert_eq!(doc.locator(location), "model.faces#f1");
    }

    #[test]
    fn id_collision_keeps_every_hit() {
        let doc = CadirDocument::from_value(&json!({
            "model": {
                "a": [{"id": "dup"}],
                "b": [{"id": "dup"}]
            }
        }));
        assert_eq!(doc.by_id["dup"].len(), 2);
    }

    /// Selects by ID, falling back to the first record when no ID is given.
    fn ids_selection(ids: &[&str]) -> RecordSelection {
        RequestedIds::new(ids.iter().map(|id| (*id).to_owned()).collect())
            .map_or(RecordSelection::Head(1), RecordSelection::Ids)
    }

    #[test]
    fn select_head_and_suffix_and_ambiguous() {
        let doc = CadirDocument::from_value(&json!({
            "model": {"faces": [
                {"id": "other:face#1"},
                {"id": "other:face#2"},
                {"id": "other:face#802"},
                {"id": "other:coedge#802"}
            ]}
        }));
        let target = ArenaTarget::parse("faces").unwrap();
        let (idx, err) = doc
            .select_records(&target, &RecordSelection::Head(2))
            .unwrap();
        assert_eq!(
            idx.iter().map(|location| location.rec).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert!(err.is_empty());

        let (idx, err) = doc
            .select_records(&target, &ids_selection(&["face#2"]))
            .unwrap();
        assert_eq!(
            idx.iter().map(|location| location.rec).collect::<Vec<_>>(),
            vec![1]
        );
        assert!(err.is_empty());

        let (_, err) = doc
            .select_records(&target, &ids_selection(&["#802"]))
            .unwrap();
        assert_eq!(err.len(), 1);
        assert!(err[0].contains("ambiguous"));
    }
}
