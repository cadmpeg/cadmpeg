// SPDX-License-Identifier: Apache-2.0
//! The key of an annotation: a top-level entity id, or a delta record's id
//! qualified by the evaluation or process state that holds the delta (§3.2).

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::design::ids::{EvaluationId, ProcessStateId};
use crate::ids::{Identity, NativeId};

/// The separator between a delta record's qualifier and its id.
const QUALIFIER_SEPARATOR: char = ' ';

/// The key of one annotation, in its wire form.
///
/// A key that holds a space is `<qualifier id> <record id>`: the record of
/// that id in the delta of the evaluation or process state of the qualifier
/// id. Any other key is a top-level entity id. Ids hold no whitespace (§2.2),
/// so every string is one key of one form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnnotationKey(String);

/// What an annotation key names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationTarget<'key> {
    /// A top-level entity, by its id.
    Entity(&'key str),
    /// A record of the delta of an evaluation or a process state.
    Delta {
        /// The id of the evaluation or process state.
        qualifier: &'key str,
        /// The id of the record in that delta.
        record: &'key str,
    },
}

/// The id of a record that holds a delta: an evaluation or a process state.
pub trait Qualifier: sealed::Sealed {
    /// The id, in its wire form.
    fn as_qualifier(&self) -> &str;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for crate::design::ids::EvaluationId {}
    impl Sealed for crate::design::ids::ProcessStateId {}
}

impl Qualifier for EvaluationId {
    fn as_qualifier(&self) -> &str {
        self.as_str()
    }
}

impl Qualifier for ProcessStateId {
    fn as_qualifier(&self) -> &str {
        self.as_str()
    }
}

impl AnnotationKey {
    /// The key of `record` in the delta of `qualifier`, an evaluation or a
    /// process state.
    #[must_use]
    pub fn in_delta(qualifier: &impl Qualifier, record: &impl AsRef<Identity>) -> Self {
        Self::qualified(qualifier.as_qualifier(), record.as_ref().as_str())
    }

    fn qualified(qualifier: &str, record: &str) -> Self {
        Self(format!("{qualifier}{QUALIFIER_SEPARATOR}{record}"))
    }

    /// What the key names.
    #[must_use]
    pub fn target(&self) -> AnnotationTarget<'_> {
        match self.0.split_once(QUALIFIER_SEPARATOR) {
            Some((qualifier, record)) => AnnotationTarget::Delta { qualifier, record },
            None => AnnotationTarget::Entity(&self.0),
        }
    }

    /// The id of the top-level record the annotation belongs to: the entity,
    /// or the evaluation or process state whose delta holds the record.
    #[must_use]
    pub fn owner(&self) -> &str {
        match self.target() {
            AnnotationTarget::Entity(id) => id,
            AnnotationTarget::Delta { qualifier, .. } => qualifier,
        }
    }

    /// The key with each id it holds replaced by `map` of that id.
    pub(super) fn map(&self, mut map: impl FnMut(&str) -> String) -> Self {
        match self.target() {
            AnnotationTarget::Entity(id) => Self(map(id)),
            AnnotationTarget::Delta { qualifier, record } => {
                Self::qualified(&map(qualifier), &map(record))
            }
        }
    }

    /// Each id the key holds.
    pub(super) fn ids(&self) -> impl Iterator<Item = &str> {
        let (first, second) = match self.target() {
            AnnotationTarget::Entity(id) => (id, None),
            AnnotationTarget::Delta { qualifier, record } => (qualifier, Some(record)),
        };
        std::iter::once(first).chain(second)
    }

    /// The key in its wire form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<Identity> for AnnotationKey {
    /// The key of the top-level entity `id`.
    fn from(id: Identity) -> Self {
        Self(id.into_string())
    }
}

impl From<&Identity> for AnnotationKey {
    /// The key of the top-level entity `id`.
    fn from(id: &Identity) -> Self {
        Self(id.as_str().to_owned())
    }
}

impl From<&NativeId> for AnnotationKey {
    /// The key of the native record `id`.
    fn from(id: &NativeId) -> Self {
        Self(id.as_str().to_owned())
    }
}

impl From<&AnnotationKey> for AnnotationKey {
    fn from(key: &AnnotationKey) -> Self {
        key.clone()
    }
}

/// Builder tests name entities by bare text that no codec mints.
#[cfg(test)]
impl From<&str> for AnnotationKey {
    fn from(key: &str) -> Self {
        Self(key.to_owned())
    }
}

#[cfg(test)]
impl From<String> for AnnotationKey {
    fn from(key: String) -> Self {
        Self(key)
    }
}

#[cfg(test)]
impl From<&String> for AnnotationKey {
    fn from(key: &String) -> Self {
        Self(key.clone())
    }
}

impl std::borrow::Borrow<str> for AnnotationKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<String> for AnnotationKey {
    fn borrow(&self) -> &String {
        &self.0
    }
}

impl PartialEq<str> for AnnotationKey {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl Display for AnnotationKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
