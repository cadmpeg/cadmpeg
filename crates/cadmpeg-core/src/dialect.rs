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
use std::error::Error;
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
    /// the identified dialect. Identity and admission are orthogonal: a
    /// legacy document can carry a registry row of its own while its bytes
    /// are read with a newer grammar.
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
    pub dialect: DialectId,
    /// Version fields the source declared, verbatim, under keys pinned per
    /// codec in the registry.
    ///
    /// Declarations are evidence, never a control input: the dialect is what
    /// the bytes obey, not what they declare.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub declared: BTreeMap<String, String>,
    /// Instance of this format layer inside the containing document.
    ///
    /// `None` when the layer occurs once or has no report-local identity. This
    /// is not source-declared evidence and therefore does not belong in
    /// [`Self::declared`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// How this layer was admitted.
    pub admission: Admission,
}

/// A report's primary format layer and any nested or carried format layers.
///
/// Construction rejects an extra layer whose format equals the primary
/// layer's format. The wire names the primary explicitly, so the collection
/// carries its complete identity without an enclosing report's format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DialectLayers {
    primary: DialectMatch,
    extra: Vec<DialectMatch>,
}

impl DialectLayers {
    /// Constructs dialect layers with one primary and no extra layers.
    #[must_use]
    pub fn of(primary: DialectMatch) -> Self {
        Self {
            primary,
            extra: Vec::new(),
        }
    }

    /// Constructs dialect layers with one unique primary format.
    pub fn new(
        primary: DialectMatch,
        extra: Vec<DialectMatch>,
    ) -> Result<Self, DialectLayersError> {
        if extra.iter().any(|layer| layer.format == primary.format) {
            return Err(DialectLayersError {
                format: primary.format,
                reason: DialectLayersErrorReason::RepeatedPrimary,
            });
        }
        Ok(Self { primary, extra })
    }

    /// Returns the report's primary format layer.
    #[must_use]
    pub fn primary(&self) -> &DialectMatch {
        &self.primary
    }

    /// Iterates over the primary layer followed by every extra layer.
    pub fn iter(&self) -> impl Iterator<Item = &DialectMatch> {
        std::iter::once(&self.primary).chain(&self.extra)
    }

    /// Consumes the collection into its primary and extra layers.
    #[must_use]
    pub fn into_parts(self) -> (DialectMatch, Vec<DialectMatch>) {
        (self.primary, self.extra)
    }
}

/// A dialect collection included a second layer for its primary format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectLayersError {
    format: String,
    reason: DialectLayersErrorReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialectLayersErrorReason {
    RepeatedPrimary,
}

impl fmt::Display for DialectLayersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            DialectLayersErrorReason::RepeatedPrimary => write!(
                f,
                "extra dialect layer repeats primary format {:?}",
                self.format
            ),
        }
    }
}

impl Error for DialectLayersError {}

impl DialectMatch {
    /// Construct one identified dialect layer.
    #[must_use]
    pub fn layer(
        format: impl Into<String>,
        dialect: DialectId,
        declared: BTreeMap<String, String>,
        admission: Admission,
    ) -> Self {
        Self {
            format: format.into(),
            dialect,
            declared,
            instance: None,
            admission,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn layer(format: &str) -> DialectMatch {
        DialectMatch {
            format: format.to_owned(),
            dialect: DialectId::pinned("rhino:archive-80"),
            declared: BTreeMap::new(),
            instance: None,
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
    fn an_admitted_match_serializes_its_identity() {
        let admitted = DialectMatch {
            format: "rhino".into(),
            dialect: DialectId::pinned("rhino:archive-80"),
            declared: BTreeMap::new(),
            instance: None,
            admission: Admission::Admitted,
        };

        assert_eq!(
            serde_json::to_string(&admitted).unwrap(),
            "{\"format\":\"rhino\",\"dialect\":\"rhino:archive-80\",\"admission\":\"admitted\"}"
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
    fn dialect_layers_reject_a_same_format_extra() {
        let error = DialectLayers::new(layer("rhino"), vec![layer("acis"), layer("rhino")])
            .expect_err("a second rhino layer must be rejected");

        assert_eq!(
            error.to_string(),
            "extra dialect layer repeats primary format \"rhino\""
        );
    }

    #[test]
    fn dialect_layers_serialize_with_an_explicit_primary() {
        let layers = DialectLayers::new(layer("rhino"), vec![layer("acis")]).unwrap();
        let serialized = serde_json::to_value(&layers).unwrap();

        assert_eq!(
            serialized,
            serde_json::json!({
                "primary": layer("rhino"),
                "extra": [layer("acis")],
            })
        );
        assert_eq!(
            serde_json::from_value::<DialectLayers>(serialized).unwrap(),
            layers
        );
        assert_eq!(
            layers
                .iter()
                .map(|layer| layer.format.as_str())
                .collect::<Vec<_>>(),
            ["rhino", "acis"]
        );
    }
}
