// SPDX-License-Identifier: Apache-2.0
//! Explicit accounting for typed native content in every namespace.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::report::loss::LossNote;
use crate::CadIr;

use super::loss::ExportLossCode;

/// How an output represents a typed native record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeDisposition {
    /// The writer re-emits the document's typed native content.
    Written,
    /// The writer regenerates the native content from its neutral counterpart.
    Regenerated,
    /// The output does not carry this record's content.
    Dropped,
}

/// An arena declaration, including any record-specific exceptions.
///
/// Exceptions let an unresolved native record be dropped without misreporting
/// the valid records re-emitted from the same arena.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NativeArenaDisposition {
    /// Disposition of records without an explicit exception.
    pub disposition: NativeDisposition,
    /// Exceptions keyed by original document record identity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub records: BTreeMap<String, NativeDisposition>,
}

impl NativeArenaDisposition {
    fn new(disposition: NativeDisposition) -> Self {
        Self {
            disposition,
            records: BTreeMap::new(),
        }
    }

    /// The disposition of a specific record.
    pub fn record(&self, id: &str) -> NativeDisposition {
        self.records.get(id).copied().unwrap_or(self.disposition)
    }
}

/// All arena declarations for one format's native namespace.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NativeNamespaceDispositions {
    /// Arena names from the owning codec's native registry.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub arenas: BTreeMap<String, NativeArenaDisposition>,
}

/// Native-content accounting across every namespace in the input document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NativeDispositions {
    /// Format namespaces and their explicit arena declarations.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub namespaces: BTreeMap<String, NativeNamespaceDispositions>,
}

impl NativeDispositions {
    /// Declare every registered arena of one output format.
    pub fn for_namespace<I, S>(namespace: &str, arenas: I, disposition: NativeDisposition) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut value = Self::default();
        for arena in arenas {
            value.set(namespace, arena.as_ref(), disposition);
        }
        value
    }

    /// CADIR serializes every typed native record in the document.
    pub fn written(ir: &CadIr) -> Self {
        let mut value = Self::default();
        for (namespace, records) in &ir.native.0 {
            for arena in records.arenas().keys() {
                value.set(namespace, arena, NativeDisposition::Written);
            }
        }
        value
    }

    /// Declare how a whole arena is represented.
    pub fn set(&mut self, namespace: &str, arena: &str, disposition: NativeDisposition) {
        self.namespaces
            .entry(namespace.to_owned())
            .or_default()
            .arenas
            .insert(arena.to_owned(), NativeArenaDisposition::new(disposition));
    }

    /// Declare a record-specific outcome without discarding valid siblings.
    pub fn set_record(
        &mut self,
        namespace: &str,
        arena: &str,
        id: &str,
        disposition: NativeDisposition,
    ) {
        self.namespaces
            .entry(namespace.to_owned())
            .or_default()
            .arenas
            .entry(arena.to_owned())
            .or_insert_with(|| NativeArenaDisposition::new(NativeDisposition::Dropped))
            .records
            .insert(id.to_owned(), disposition);
    }

    /// The default outcome for an arena; an undeclared arena is dropped.
    pub fn disposition(&self, namespace: &str, arena: &str) -> NativeDisposition {
        self.namespaces
            .get(namespace)
            .and_then(|value| value.arenas.get(arena))
            .map_or(NativeDisposition::Dropped, |value| value.disposition)
    }

    /// The outcome for one original document record.
    pub fn record_disposition(&self, namespace: &str, arena: &str, id: &str) -> NativeDisposition {
        self.namespaces
            .get(namespace)
            .and_then(|value| value.arenas.get(arena))
            .map_or(NativeDisposition::Dropped, |value| value.record(id))
    }

    /// Materialize undeclared typed input arenas as dropped, including foreign ones.
    /// Reserved `unknowns` stubs carry no typed content to charge here.
    pub(super) fn complete(&mut self, ir: &CadIr) {
        for (namespace, records) in &ir.native.0 {
            let declared = self.namespaces.entry(namespace.clone()).or_default();
            for arena in records
                .arenas()
                .keys()
                .filter(|arena| arena.as_str() != "unknowns")
            {
                declared
                    .arenas
                    .entry(arena.clone())
                    .or_insert_with(|| NativeArenaDisposition::new(NativeDisposition::Dropped));
            }
        }
    }

    /// Counts of omitted records, grouped by their original namespace and arena.
    pub fn dropped_counts(&self, ir: &CadIr) -> BTreeMap<String, BTreeMap<String, usize>> {
        self.dropped_records(ir)
            .into_iter()
            .map(|(namespace, arenas)| {
                (
                    namespace,
                    arenas
                        .into_iter()
                        .map(|(arena, ids)| (arena, ids.len()))
                        .collect(),
                )
            })
            .collect()
    }

    fn dropped_records<'a>(
        &self,
        ir: &'a CadIr,
    ) -> BTreeMap<String, BTreeMap<String, Vec<&'a str>>> {
        let mut dropped = BTreeMap::new();
        for (namespace, data) in &ir.native.0 {
            for (arena, records) in data.arenas() {
                if arena == "unknowns" {
                    continue;
                }
                let mut ids: Vec<_> = records
                    .iter()
                    .filter(|record| {
                        self.record_disposition(namespace, arena, record.id())
                            == NativeDisposition::Dropped
                    })
                    .map(crate::native::NativeRecord::id)
                    .collect();
                if !ids.is_empty() {
                    ids.sort_unstable();
                    dropped
                        .entry(namespace.clone())
                        .or_insert_with(BTreeMap::new)
                        .insert(arena.clone(), ids);
                }
            }
        }
        dropped
    }

    pub(super) fn omission_losses(&self, ir: &CadIr, format: &str) -> Vec<LossNote> {
        self.dropped_records(ir)
            .into_iter()
            .flat_map(|(namespace, arenas)| {
                arenas.into_iter().map(move |(arena, ids)| {
                    ExportLossCode::NativeRecordsOmitted.note(format!(
                        "the {format} write omits {} native record(s) from `{namespace}.{arena}`: {}",
                        ids.len(),
                        ids.join(", "),
                    ))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
