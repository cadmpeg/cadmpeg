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
//! Producers use [`DialectId::pinned`] for checked static ids. Wire readers use
//! [`DialectId::parse`] for checked owned ids. A codec backs its dialects with
//! its own enum and returns pinned ids from it — the `*LossCode` template: enum
//! inside, pinned string at the boundary, one construction path, closed
//! vocabulary. Deserialization is the only other way an id comes into being,
//! and it reconstructs an id that some producer pinned.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A registry dialect id, for example `"rhino:archive-80"`.
///
/// Canonical form is `<format>:<name>`, lowercase and hyphenated, stable
/// forever. Outside the owning codec the validated id is an opaque label.
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
        assert!(valid_dialect_id(id), "invalid pinned dialect id");
        Self(Cow::Borrowed(id))
    }

    /// Parses and validates an owned dialect id.
    pub fn parse(id: impl Into<String>) -> Result<Self, DialectIdError> {
        let id = id.into();
        if valid_dialect_id(&id) {
            Ok(Self(Cow::Owned(id)))
        } else {
            Err(DialectIdError(id))
        }
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn namespace(&self) -> &str {
        self.as_str()
            .split_once(':')
            .expect("DialectId validation guarantees a namespace")
            .0
    }
}

const fn valid_dialect_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let mut index = 0;
    let mut colon = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b':' {
            if colon.is_some() || index == 0 || index + 1 == bytes.len() {
                return false;
            }
            colon = Some(index);
        } else if (!byte.is_ascii_lowercase()
            && !byte.is_ascii_digit()
            && byte != b'-'
            && byte != b'.')
            || (byte == b'-'
                && (index == 0
                    || index + 1 == bytes.len()
                    || bytes[index - 1] == b':'
                    || bytes[index + 1] == b':'))
        {
            return false;
        }
        index += 1;
    }
    colon.is_some()
}

/// A string is not a canonical dialect id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectIdError(String);

impl fmt::Display for DialectIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid dialect id {:?}: expected <format>:<name> in lowercase canonical form",
            self.0
        )
    }
}

impl Error for DialectIdError {}

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
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
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
    /// own declared fallback, `using` names the row itself.
    ///
    /// The codec must charge its dialect-unverified loss.
    AdmittedUnverified {
        /// Dialect whose declared strategy was substituted for the parse.
        using: DialectId,
    },
    /// Structurally identified; semantic decode refused.
    Refused,
}

/// One format layer's identification of one document.
///
/// A document carries several format layers: `.sldprt` contains Parasolid;
/// `.f3d`, `.ipt`, and SAT contain ACIS; NX contains Parasolid and JT. One
/// match describes one layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DialectMatch {
    /// Format layer this classifies: `"rhino"`, `"acis"`, `"parasolid"`.
    format: String,
    /// Registry dialect id, for example `"rhino:archive-80"`.
    dialect: DialectId,
    /// Version fields the source declared, verbatim, under keys pinned per
    /// codec in the registry.
    ///
    /// Declarations are evidence, never a control input: the dialect is what
    /// the bytes obey, not what they declare.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    declared: BTreeMap<String, String>,
    /// Instance of this format layer inside the containing document.
    ///
    /// `None` when the layer occurs once or has no report-local identity. This
    /// is not source-declared evidence and therefore does not belong in
    /// [`Self::declared`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance: Option<String>,
    /// How this layer was admitted.
    admission: Admission,
}

#[derive(Deserialize)]
struct DialectMatchWire {
    format: String,
    dialect: DialectId,
    #[serde(default)]
    declared: BTreeMap<String, String>,
    #[serde(default)]
    instance: Option<String>,
    admission: Admission,
}

impl<'de> Deserialize<'de> for DialectMatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = DialectMatchWire::deserialize(deserializer)?;
        if wire.format != wire.dialect.namespace() {
            return Err(serde::de::Error::custom(format_args!(
                "dialect {:?} is not in format namespace {:?}",
                wire.dialect.as_str(),
                wire.format
            )));
        }
        Ok(Self {
            format: wire.format,
            dialect: wire.dialect,
            declared: wire.declared,
            instance: wire.instance,
            admission: wire.admission,
        })
    }
}

/// A report's primary format layer and any nested or carried format layers.
///
/// Extra layers are unique by `(format, instance)`. An extra layer for the
/// primary format has an instance that identifies it inside the containing
/// document. The wire names the primary explicitly, so the collection carries
/// its complete identity without an enclosing report's format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DialectLayers {
    primary: DialectMatch,
    extra: Vec<DialectMatch>,
}

#[derive(Deserialize)]
struct DialectLayersWire {
    primary: DialectMatch,
    extra: Vec<DialectMatch>,
}

impl<'de> Deserialize<'de> for DialectLayers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = DialectLayersWire::deserialize(deserializer)?;
        Self::new(wire.primary, wire.extra).map_err(serde::de::Error::custom)
    }
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

    /// Constructs dialect layers with extras unique by `(format, instance)`.
    pub fn new(
        primary: DialectMatch,
        extra: Vec<DialectMatch>,
    ) -> Result<Self, DialectLayersError> {
        let mut keys = BTreeSet::new();
        for layer in &extra {
            if layer.format == primary.format && layer.instance.is_none() {
                return Err(DialectLayersError {
                    format: layer.format.clone(),
                    instance: None,
                    reason: DialectLayersErrorReason::UnidentifiedPrimaryFormatExtra,
                });
            }
            if !keys.insert((&layer.format, &layer.instance)) {
                return Err(DialectLayersError {
                    format: layer.format.clone(),
                    instance: layer.instance.clone(),
                    reason: DialectLayersErrorReason::DuplicateExtra,
                });
            }
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

/// A dialect collection violated the extra-layer identity invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectLayersError {
    format: String,
    instance: Option<String>,
    reason: DialectLayersErrorReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialectLayersErrorReason {
    UnidentifiedPrimaryFormatExtra,
    DuplicateExtra,
}

impl fmt::Display for DialectLayersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            DialectLayersErrorReason::UnidentifiedPrimaryFormatExtra => write!(
                f,
                "extra dialect layer for primary format {:?} requires an instance",
                self.format
            ),
            DialectLayersErrorReason::DuplicateExtra => write!(
                f,
                "duplicate extra dialect layer for format {:?} and instance {:?}",
                self.format, self.instance
            ),
        }
    }
}

impl Error for DialectLayersError {}

impl DialectMatch {
    /// Constructs one identified dialect layer without declarations or an instance.
    #[must_use]
    pub fn new(dialect: DialectId, admission: Admission) -> Self {
        let format = dialect.namespace().to_owned();
        Self {
            format,
            dialect,
            declared: BTreeMap::new(),
            instance: None,
            admission,
        }
    }

    /// Construct one identified dialect layer.
    #[must_use]
    pub fn layer(
        dialect: DialectId,
        declared: BTreeMap<String, String>,
        admission: Admission,
    ) -> Self {
        Self::new(dialect, admission).with_declared(declared)
    }

    /// Attaches source-declared version fields before the match enters a report.
    #[must_use]
    pub fn with_declared(mut self, declared: BTreeMap<String, String>) -> Self {
        self.declared = declared;
        self
    }

    /// Attaches a report-local layer instance before the match enters a report.
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// Returns the classified format layer.
    #[must_use]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Returns the registry dialect identity.
    #[must_use]
    pub fn dialect(&self) -> &DialectId {
        &self.dialect
    }

    /// Returns the source-declared version fields.
    #[must_use]
    pub fn declared(&self) -> &BTreeMap<String, String> {
        &self.declared
    }

    /// Returns the report-local layer instance.
    #[must_use]
    pub fn instance(&self) -> Option<&str> {
        self.instance.as_deref()
    }

    /// Returns how this layer was admitted.
    #[must_use]
    pub fn admission(&self) -> Admission {
        self.admission.clone()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn layer(format: &str) -> DialectMatch {
        DialectMatch {
            format: format.to_owned(),
            dialect: DialectId::parse(format!("{format}:unknown")).unwrap(),
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
    fn dialect_id_deserialization_rejects_noncanonical_strings() {
        for id in [
            "rhino",
            ":archive-80",
            "rhino:",
            "Rhino:archive-80",
            "rhino:archive_80",
            "rhino:-archive-80",
        ] {
            serde_json::from_value::<DialectId>(serde_json::json!(id))
                .expect_err("a malformed dialect id must be rejected");
        }
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
    fn dialect_match_deserialization_rejects_a_foreign_namespace() {
        let malformed = serde_json::json!({
            "format": "rhino",
            "dialect": "step:ap242-e3",
            "admission": "admitted",
        });

        let error = serde_json::from_value::<DialectMatch>(malformed)
            .expect_err("the dialect namespace must equal the classified format");
        assert!(
            error
                .to_string()
                .contains("dialect \"step:ap242-e3\" is not in format namespace \"rhino\""),
            "{error}"
        );
    }

    #[test]
    fn an_unverified_admission_names_the_dialect_in_use() {
        let unverified = Admission::AdmittedUnverified {
            using: DialectId::pinned("acis:save-format-217"),
        };

        assert_eq!(
            serde_json::to_string(&unverified).unwrap(),
            "{\"admitted_unverified\":{\"using\":\"acis:save-format-217\"}}"
        );
    }

    #[test]
    fn dialect_layers_accept_a_same_format_extra_with_an_instance() {
        let member = layer("rhino").with_instance("components/member.3dm");
        let layers = DialectLayers::new(layer("rhino"), vec![member.clone()]).unwrap();

        let serialized = serde_json::to_value(&layers).unwrap();
        assert_eq!(
            serde_json::from_value::<DialectLayers>(serialized).unwrap(),
            layers
        );
        assert_eq!(layers.into_parts().1, [member]);
    }

    #[test]
    fn dialect_layers_reject_a_same_format_extra_without_an_instance() {
        let error = DialectLayers::new(layer("rhino"), vec![layer("acis"), layer("rhino")])
            .expect_err("an unidentified nested rhino layer must be rejected");

        assert_eq!(
            error.to_string(),
            "extra dialect layer for primary format \"rhino\" requires an instance"
        );
    }

    #[test]
    fn dialect_layers_deserialization_rejects_an_unidentified_same_format_extra() {
        let serialized = serde_json::json!({
            "primary": layer("rhino"),
            "extra": [layer("acis"), layer("rhino")],
        });

        let error = serde_json::from_value::<DialectLayers>(serialized)
            .expect_err("wire input must use the checked constructor");
        assert!(
            error
                .to_string()
                .contains("extra dialect layer for primary format \"rhino\" requires an instance"),
            "{error}"
        );
    }

    #[test]
    fn dialect_layers_reject_duplicate_format_instance_pairs() {
        let first = layer("acis").with_instance("body");
        let duplicate = layer("acis").with_instance("body");
        let error = DialectLayers::new(layer("rhino"), vec![first, duplicate])
            .expect_err("duplicate extra-layer keys must be rejected");

        assert_eq!(
            error.to_string(),
            "duplicate extra dialect layer for format \"acis\" and instance Some(\"body\")"
        );
    }

    #[test]
    fn dialect_layers_keep_cross_format_extras_with_distinct_instances() {
        let anonymous = layer("acis");
        let named = layer("acis").with_instance("body");
        let layers =
            DialectLayers::new(layer("rhino"), vec![anonymous.clone(), named.clone()]).unwrap();

        assert_eq!(layers.into_parts().1, [anonymous, named]);
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
