// SPDX-License-Identifier: Apache-2.0
//! Document locations named by validation findings.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;

use crate::document::CadIr;
use crate::native::view::NativeView;
use crate::report::check::Finding;
use crate::schema::{EntityKind, EntitySchema};

/// Where a document holds one identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the convert gate in the next unit")
)]
pub(crate) enum EntityLocation<'ir> {
    /// A neutral model arena of this kind.
    Model(EntityKind),
    /// A source-native namespace, keyed by its codec id.
    Native(&'ir str),
}

/// What one finding names in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the convert gate in the next unit")
)]
pub(crate) enum FindingScope<'ir> {
    /// The finding names no entity, or its entity resolves nowhere in the document.
    Unlocated,
    /// Every location of the entity and each document identity in the message.
    Located(Vec<EntityLocation<'ir>>),
}

/// Document locations of the identities a set of findings names.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the convert gate in the next unit")
)]
pub(crate) struct FindingLocator<'ir, 'r> {
    locations: HashMap<&'r str, Vec<EntityLocation<'ir>>>,
}

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the convert gate in the next unit")
)]
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
                    .chain(message_candidates(&finding.message))
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

    /// Returns the scope of a finding supplied to `new`.
    pub(crate) fn scope(&self, finding: &'r Finding) -> FindingScope<'ir> {
        let Some(locations) = finding
            .entity
            .as_deref()
            .and_then(|id| self.locations.get(id))
        else {
            return FindingScope::Unlocated;
        };
        let mut locations = locations.clone();
        for identity in message_candidates(&finding.message) {
            if let Some(entries) = self.locations.get(identity) {
                for &location in entries {
                    if !locations.contains(&location) {
                        locations.push(location);
                    }
                }
            }
        }
        FindingScope::Located(locations)
    }
}

fn arena_kind<T: EntitySchema>(_: &[T]) -> EntityKind {
    T::KIND
}

fn message_candidates(message: &str) -> impl Iterator<Item = &str> {
    const TRIM: &[char] = &[
        '`', '\'', '"', '(', ')', '[', ']', '{', '}', '<', '>', ',', ';',
    ];
    message.split_whitespace().flat_map(|token| {
        let token = token.trim_matches(TRIM);
        let stripped = token
            .strip_suffix(['.', ':'])
            .map(|id| id.trim_matches(TRIM));
        std::iter::once(token)
            .chain(stripped)
            .filter(|id| id.contains(':') && id.contains('#'))
    })
}

#[cfg(test)]
mod tests;
