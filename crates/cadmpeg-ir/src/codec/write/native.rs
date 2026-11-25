// SPDX-License-Identifier: Apache-2.0
//! Per-arena native dispositions declared by every write path (§3.4, Rule W3).
//!
//! A backend states what its payload does with each native arena of the
//! document, with record overrides inside mixed arenas. The sealed wrapper
//! charges `export:native.records-omitted` with the ids of every record the
//! declaration drops, so a namespace the backend never reads is reported too.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::document::CadIr;
use crate::report::loss::LossNote;

use super::loss::native_records_omitted;

/// The arena that holds given-up parts' stubs, which incur no omission loss.
const UNKNOWNS_ARENA: &str = "unknowns";

/// What one write path does with one native record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDisposition {
    /// The payload re-emits the record.
    Written,
    /// The payload states the record's content regenerated from neutral
    /// records.
    Regenerated,
    /// The payload does not state the record.
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Declared {
    disposition: NativeDisposition,
    reason: Option<Arc<str>>,
}

impl Declared {
    const fn new(disposition: NativeDisposition) -> Self {
        Self {
            disposition,
            reason: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NamespaceDeclaration {
    disposition: Option<Declared>,
    arenas: BTreeMap<String, Declared>,
    records: HashMap<String, Declared>,
}

/// What one write path does with each native arena of the document.
///
/// A record takes its own override, else its arena's declaration, else its
/// namespace's, else the declaration of every namespace, which is
/// [`NativeDisposition::Dropped`] unless the payload is built with
/// [`Self::written`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDispositions {
    every: NativeDisposition,
    namespaces: BTreeMap<String, NamespaceDeclaration>,
}

impl NativeDispositions {
    /// A payload that states no native record but those declared.
    #[must_use]
    pub fn dropped() -> Self {
        Self {
            every: NativeDisposition::Dropped,
            namespaces: BTreeMap::new(),
        }
    }

    /// A payload that writes every native record of every namespace.
    #[must_use]
    pub fn written() -> Self {
        Self {
            every: NativeDisposition::Written,
            namespaces: BTreeMap::new(),
        }
    }

    fn entry(&mut self, namespace: &str) -> &mut NamespaceDeclaration {
        self.namespaces.entry(namespace.to_owned()).or_default()
    }

    /// Declares every arena of `namespace`.
    pub fn namespace(&mut self, namespace: &str, disposition: NativeDisposition) -> &mut Self {
        self.entry(namespace).disposition = Some(Declared::new(disposition));
        self
    }

    /// Declares one arena of `namespace`.
    pub fn arena(
        &mut self,
        namespace: &str,
        arena: &str,
        disposition: NativeDisposition,
    ) -> &mut Self {
        self.entry(namespace)
            .arenas
            .insert(arena.to_owned(), Declared::new(disposition));
        self
    }

    /// Drops one arena of `namespace`; the loss gives `reason` after the
    /// arena's name.
    pub fn drop_arena(&mut self, namespace: &str, arena: &str, reason: &str) -> &mut Self {
        self.entry(namespace).arenas.insert(
            arena.to_owned(),
            Declared {
                disposition: NativeDisposition::Dropped,
                reason: Some(Arc::from(reason)),
            },
        );
        self
    }

    /// Drops the records `ids` of `namespace`; the loss gives `reason` after
    /// the name of the arena that holds them.
    pub fn drop_records<I, S>(&mut self, namespace: &str, reason: &str, ids: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let reason: Arc<str> = Arc::from(reason);
        let declaration = self.entry(namespace);
        for id in ids {
            declaration.records.insert(
                id.into(),
                Declared {
                    disposition: NativeDisposition::Dropped,
                    reason: Some(Arc::clone(&reason)),
                },
            );
        }
        self
    }

    /// The declaration that holds for record `id` of `arena` in `namespace`.
    fn declared(
        &self,
        namespace: &str,
        arena: &str,
        id: &str,
    ) -> (NativeDisposition, Option<&str>) {
        let Some(declaration) = self.namespaces.get(namespace) else {
            return (self.every, None);
        };
        declaration
            .records
            .get(id)
            .or_else(|| declaration.arenas.get(arena))
            .or(declaration.disposition.as_ref())
            .map_or((self.every, None), |declared| {
                (declared.disposition, declared.reason.as_deref())
            })
    }

    /// What the payload does with record `id` of `arena` in `namespace`.
    #[must_use]
    pub fn disposition(&self, namespace: &str, arena: &str, id: &str) -> NativeDisposition {
        self.declared(namespace, arena, id).0
    }

    /// One `export:native.records-omitted` loss per arena of `ir` and drop
    /// reason, naming every dropped id, sorted. The `unknowns` arena is never
    /// charged.
    pub(super) fn omission_losses(&self, ir: &CadIr, format: &str) -> Vec<LossNote> {
        let mut losses = Vec::new();
        for (namespace, arenas) in &ir.native.0 {
            let namespace = namespace.as_str();
            for (arena, records) in arenas.arenas() {
                let arena = arena.as_str();
                if arena == UNKNOWNS_ARENA {
                    continue;
                }
                let mut dropped: BTreeMap<Option<&str>, Vec<&str>> = BTreeMap::new();
                for record in records {
                    let (disposition, reason) = self.declared(namespace, arena, record.id());
                    if disposition == NativeDisposition::Dropped {
                        dropped.entry(reason).or_default().push(record.id());
                    }
                }
                for (reason, ids) in dropped {
                    let reason = match reason {
                        Some(reason) => format!("of `{namespace}` arena `{arena}` {reason}"),
                        None => format!("of `{namespace}` arena `{arena}`"),
                    };
                    losses.extend(native_records_omitted(format, &reason, ids));
                }
            }
        }
        losses
    }
}

#[cfg(test)]
mod tests;
