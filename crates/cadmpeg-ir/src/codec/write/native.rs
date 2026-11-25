// SPDX-License-Identifier: Apache-2.0
//! Per-arena native dispositions declared by every write path (§3.4, Rule W3).
//!
//! A backend states what its payload does with each native arena of the
//! document, with record overrides inside mixed arenas. The sealed wrapper
//! charges `export:native.records-omitted` with the ids of every record the
//! declaration drops, so a namespace the backend never reads is reported too.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::document::CadIr;
use crate::report::loss::LossNote;

use super::loss::ExportLossCode;

/// The arena that holds given-up parts' stubs, which incur no omission loss.
pub(super) const UNKNOWNS_ARENA: &str = "unknowns";

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
/// [`Self::written`]. A drop declared without a reason takes the reason of
/// that last declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDispositions {
    every: Declared,
    namespaces: BTreeMap<String, NamespaceDeclaration>,
}

impl NativeDispositions {
    /// A payload that states no native record but those declared; the loss
    /// gives `reason` after the name of each arena left undeclared.
    #[must_use]
    pub fn dropped(reason: &str) -> Self {
        Self {
            every: Declared {
                disposition: NativeDisposition::Dropped,
                reason: Some(Arc::from(reason)),
            },
            namespaces: BTreeMap::new(),
        }
    }

    /// A payload that writes every native record of every namespace.
    #[must_use]
    pub fn written() -> Self {
        Self {
            every: Declared::new(NativeDisposition::Written),
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
        let declared = self
            .namespaces
            .get(namespace)
            .and_then(|declaration| {
                declaration
                    .records
                    .get(id)
                    .or_else(|| declaration.arenas.get(arena))
                    .or(declaration.disposition.as_ref())
            })
            .unwrap_or(&self.every);
        let reason = declared.reason.as_deref().or(self.every.reason.as_deref());
        (declared.disposition, reason)
    }

    /// What the payload does with record `id` of `arena` in `namespace`.
    #[must_use]
    pub fn disposition(&self, namespace: &str, arena: &str, id: &str) -> NativeDisposition {
        self.declared(namespace, arena, id).0
    }

    /// One `export:native.records-omitted` loss per nonempty dropped arena
    /// of `ir`, which gives every dropped id, sorted, after the count; where
    /// the drops of the arena have several reasons, each reason follows with
    /// the ids it covers. `format` is the format's name as its write's
    /// losses spell it. The `unknowns` arena is never charged.
    pub(super) fn omission_losses(&self, ir: &CadIr, format: &str) -> Vec<LossNote> {
        let mut losses = Vec::new();
        for (namespace, arenas) in &ir.native.0 {
            let namespace = namespace.as_str();
            for (arena, records) in arenas.arenas() {
                let arena = arena.as_str();
                if arena == UNKNOWNS_ARENA {
                    continue;
                }
                let mut dropped: BTreeMap<Option<&str>, BTreeSet<&str>> = BTreeMap::new();
                for record in records {
                    let (disposition, reason) = self.declared(namespace, arena, record.id());
                    if disposition == NativeDisposition::Dropped {
                        dropped.entry(reason).or_default().insert(record.id());
                    }
                }
                let ids = dropped.values().flatten().copied().collect::<BTreeSet<_>>();
                if ids.is_empty() {
                    continue;
                }
                let place = format!(
                    "the {format} write omits {} native record(s) of `{namespace}` arena `{arena}`",
                    ids.len()
                );
                let list =
                    |ids: &BTreeSet<&str>| ids.iter().copied().collect::<Vec<_>>().join(", ");
                let message = match dropped.iter().collect::<Vec<_>>().as_slice() {
                    [(Some(reason), _)] => format!("{place} {reason}: {}", list(&ids)),
                    [(None, _)] => format!("{place}: {}", list(&ids)),
                    groups => {
                        let reasons = groups
                            .iter()
                            .map(|(reason, ids)| {
                                format!(
                                    "{}: {}",
                                    reason.unwrap_or("for no stated reason"),
                                    list(ids)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("; ");
                        format!("{place}: {}; {reasons}", list(&ids))
                    }
                };
                losses.push(ExportLossCode::NativeRecordsOmitted.note(message));
            }
        }
        losses
    }
}

#[cfg(test)]
mod tests;
