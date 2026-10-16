// SPDX-License-Identifier: Apache-2.0
//! Document locations named by validation finding entities and related ids.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;

use crate::document::CadIr;
use crate::native::view::NativeView;
use crate::report::check::Finding;
use crate::schema::{EntityKind, EntitySchema};

/// Where a document holds one identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityLocation<'ir> {
    /// A neutral model arena of this kind.
    Model(EntityKind),
    /// A source-native namespace, keyed by its codec id.
    Native(&'ir str),
}

/// Document locations of the identities a set of findings names.
pub(crate) struct FindingLocator<'ir, 'r> {
    locations: HashMap<&'r str, Vec<EntityLocation<'ir>>>,
}

impl<'ir, 'r> FindingLocator<'ir, 'r> {
    /// Indexes the identities named by `findings` in one pass over `ir`.
    pub(crate) fn new(ir: &'ir CadIr, findings: impl IntoIterator<Item = &'r Finding>) -> Self {
        let wanted: HashSet<&'r str> = findings
            .into_iter()
            .flat_map(|finding| {
                finding
                    .entity
                    .as_deref()
                    .into_iter()
                    .chain(finding.related.iter().map(String::as_str))
            })
            .collect();
        let mut locations: HashMap<&'r str, Vec<EntityLocation<'ir>>> = HashMap::new();
        let mut record = |identity: &str, location| {
            if let Some(&identity) = wanted.get(identity) {
                let entries = locations.entry(identity).or_default();
                if !entries.contains(&location) {
                    entries.push(location);
                }
            }
        };

        macro_rules! walk_model {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(
                    for entity in &ir.model.$field {
                        record(entity.identity(), EntityLocation::Model(arena_kind(&ir.model.$field)));
                    }
                )*
            };
        }
        crate::document::arena_registry!(walk_model);

        match NativeView::new(ir, None).visit(|format, _, arena| {
            for entity in arena.records() {
                record(entity.id(), EntityLocation::Native(format));
            }
            Ok::<(), Infallible>(())
        }) {
            Ok(()) => {}
            Err(never) => match never {},
        }
        Self { locations }
    }

    /// Every location of the entity of a finding supplied to `new` and of its
    /// related ids, or `None` when the finding names no entity or its entity
    /// resolves nowhere in the document.
    pub(crate) fn scope(&self, finding: &'r Finding) -> Option<Vec<EntityLocation<'ir>>> {
        let locations = finding
            .entity
            .as_deref()
            .and_then(|id| self.locations.get(id))?;
        let mut locations = locations.clone();
        for identity in &finding.related {
            if let Some(entries) = self.locations.get(identity.as_str()) {
                for &location in entries {
                    if !locations.contains(&location) {
                        locations.push(location);
                    }
                }
            }
        }
        Some(locations)
    }
}

fn arena_kind<T: EntitySchema>(_: &[T]) -> EntityKind {
    T::KIND
}

#[cfg(test)]
mod tests;
