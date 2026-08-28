// SPDX-License-Identifier: Apache-2.0
//! Dialect identification: what a document is, as read from its own bytes.
//!
//! A dialect is a named region of one format's document space, bounded only by
//! discriminants the format itself declares. It exists whether or not cadmpeg
//! reads it. [`DialectMatch`] is the record of one run: inspect or decode
//! handled the file, and [`Admission`] states how.
//!
//! This vocabulary is about the source document. cadmpeg's own version
//! universe — `IR_VERSION`, `NativeNamespace::version`, report
//! `schema_version`, sidecar versions — is data about cadmpeg, never merged
//! with this one and never sharing a type with it.
//!
//! # `DialectId` is opaque here
//!
//! [`DialectId`] is a namespaced pinned string at this layer and nothing more:
//! no shared enum of every format's dialects, no version parsing, no ordering.
//! An IR-level enum of dialects would make the shared layer depend on every
//! codec, and orderings are codec-local where they are real at all. Outside its
//! owning codec the id is comparable and printable; only the owner parses it.
//!
//! The construction path is [`DialectId::pinned`], which takes a `&'static
//! str`. A codec backs its dialects with its own enum and returns pinned ids
//! from it — the `*LossCode` template: enum inside, pinned string at the
//! boundary, one construction path, closed vocabulary. Deserialization is the
//! only other way an id comes into being, and it reconstructs an id that some
//! producer pinned.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A registry dialect id, for example `"rhino:archive-80"`.
///
/// Canonical form is `<format>:<name>`, lowercase and hyphenated, stable
/// forever. The form is a convention of the identity registry, not a parse this
/// type performs: outside the owning codec the id is an opaque label.
///
/// Serializes and deserializes as the plain string. Read it with
/// [`DialectId::as_str`] or print it; there is no other access to the raw
/// string.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DialectId(Cow<'static, str>);

impl DialectId {
    /// Pins a dialect id from a static string.
    ///
    /// The only construction path for a producer. A codec's dialect enum maps
    /// each of its variants through here, which keeps the vocabulary closed and
    /// the ids greppable.
    #[must_use]
    pub const fn pinned(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DialectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for DialectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DialectId({:?})", self.as_str())
    }
}

impl Serialize for DialectId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DialectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|id| Self(Cow::Owned(id)))
    }
}

/// How one format layer admitted, or refused, one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Admission {
    /// Parsed with the strategy declared for the identified dialect.
    Admitted,
    /// Parsed with a strategy not declared for the identified dialect, or
    /// no dialect was identified at all. Identity and admission are
    /// orthogonal: [`DialectMatch::dialect`] may still be `Some` — a legacy
    /// document can carry a registry row of its own while its bytes are
    /// read with a newer grammar.
    ///
    /// A format's residual `unknown` row is never [`Admission::Admitted`]:
    /// admission verifies a declared identity, and the residual row is the
    /// absence of one. When the substituted strategy is the residual row's
    /// own declared fallback, `nearest` names the row itself.
    ///
    /// The codec must charge its dialect-unverified loss.
    AdmittedUnverified {
        /// Dialect whose declared strategy was substituted for the parse.
        nearest: DialectId,
    },
    /// Structurally identified; semantic decode refused.
    Refused,
}

/// One format layer's identification of one document.
///
/// A document carries several format layers: `.sldprt` contains Parasolid;
/// `.f3d`, `.ipt`, and SAT contain ACIS; NX contains Parasolid and JT. One
/// match describes one layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DialectMatch {
    /// Format layer this classifies: `"rhino"`, `"acis"`, `"parasolid"`.
    pub format: String,
    /// Registry dialect id, for example `"rhino:archive-80"`.
    ///
    /// `None`: the discriminants matched no declared dialect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialect: Option<DialectId>,
    /// Version fields the source declared, verbatim, under keys pinned per
    /// codec in the registry.
    ///
    /// Declarations are evidence, never a control input: the dialect is what
    /// the bytes obey, not what they declare.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub declared: BTreeMap<String, String>,
    /// How this layer was admitted.
    pub admission: Admission,
}

/// Returns the primary-layer match in `dialects`, the one whose `format` equals
/// the reporting layer's own `format`.
///
/// The primary layer needs no marker field and no ordering convention: exactly
/// one entry's `format` equals the report's own, and that entry is the primary.
/// Consumers never index by position.
#[must_use]
pub fn primary_layer<'a>(dialects: &'a [DialectMatch], format: &str) -> Option<&'a DialectMatch> {
    let mut found = None;
    for entry in dialects {
        if entry.format == format {
            if found.is_some() {
                return None;
            }
            found = Some(entry);
        }
    }
    found
}

/// Whether `dialects` satisfies the primary-layer invariant for `format`.
///
/// Vacuously true while `dialects` is empty, which is the staged state before a
/// codec populates it. Once populated, exactly one entry must name `format`.
fn holds_primary_layer_invariant(dialects: &[DialectMatch], format: &str) -> bool {
    dialects.is_empty() || primary_layer(dialects, format).is_some()
}

/// Debug-asserts the primary-layer invariant at a construction path.
///
/// A no-op in release builds: a producer that violates the invariant has a bug
/// in its own classification, and the checker is the release-side oracle.
#[track_caller]
pub fn debug_assert_primary_layer(dialects: &[DialectMatch], format: &str) {
    debug_assert!(
        holds_primary_layer_invariant(dialects, format),
        "dialects for format {format:?} must contain exactly one entry naming it"
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn layer(format: &str) -> DialectMatch {
        DialectMatch {
            format: format.to_owned(),
            dialect: Some(DialectId::pinned("rhino:archive-80")),
            declared: BTreeMap::new(),
            admission: Admission::Admitted,
        }
    }

    #[test]
    fn a_pinned_id_prints_and_serializes_as_the_plain_string() {
        let id = DialectId::pinned("rhino:archive-80");

        assert_eq!(id.to_string(), "rhino:archive-80");
        assert_eq!(id.as_str(), "rhino:archive-80");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"rhino:archive-80\"");
        assert_eq!(
            serde_json::from_str::<DialectId>("\"rhino:archive-80\"").unwrap(),
            id
        );
    }

    #[test]
    fn an_empty_dialect_list_serializes_to_nothing_extra() {
        let admitted = DialectMatch {
            format: "rhino".into(),
            dialect: None,
            declared: BTreeMap::new(),
            admission: Admission::Admitted,
        };

        assert_eq!(
            serde_json::to_string(&admitted).unwrap(),
            "{\"format\":\"rhino\",\"admission\":\"admitted\"}"
        );
    }

    #[test]
    fn an_unverified_admission_names_the_nearest_dialect() {
        let unverified = Admission::AdmittedUnverified {
            nearest: DialectId::pinned("acis:save-format-217"),
        };

        assert_eq!(
            serde_json::to_string(&unverified).unwrap(),
            "{\"admitted_unverified\":{\"nearest\":\"acis:save-format-217\"}}"
        );
    }

    #[test]
    fn exactly_one_entry_may_name_the_reporting_format() {
        let primary = [layer("rhino"), layer("acis")];
        assert_eq!(
            primary_layer(&primary, "rhino").map(|entry| entry.format.as_str()),
            Some("rhino")
        );
        assert!(holds_primary_layer_invariant(&primary, "rhino"));

        assert!(holds_primary_layer_invariant(&[], "rhino"));
        assert!(!holds_primary_layer_invariant(&[layer("acis")], "rhino"));
        assert!(!holds_primary_layer_invariant(
            &[layer("rhino"), layer("rhino")],
            "rhino"
        ));
    }
}
