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
//! its own enum and returns registry-generated pinned constants from it — the
//! `*LossCode` template: enum inside, pinned string at the boundary, one
//! construction path, closed vocabulary. Deserialization is the only other way
//! an id comes into being, and it reconstructs an id that some producer pinned.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A registry dialect id, for example `"rhino:archive-80"`.
///
/// Canonical form is `<format>:<name>`. The format contains lowercase ASCII
/// letters and digits. The name also admits dots and hyphens, but a hyphen
/// cannot be first or last. Outside the owning codec the validated id is an
/// opaque label.
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

    /// Returns the format namespace before the id's validated separator.
    #[must_use]
    pub fn namespace(&self) -> &str {
        self.parts().0
    }

    pub(crate) fn local(&self) -> &str {
        self.parts().1
    }

    fn parts(&self) -> (&str, &str) {
        self.as_str()
            .split_once(':')
            .expect("DialectId validation guarantees a namespace")
    }
}

const fn valid_dialect_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let mut index = 0;
    let mut colon = None;
    while index < bytes.len() {
        if bytes[index] == b':' {
            if colon.is_some() || index == 0 || index + 1 == bytes.len() {
                return false;
            }
            colon = Some(index);
        }
        index += 1;
    }
    let Some(colon) = colon else {
        return false;
    };
    index = 0;
    while index < colon {
        let byte = bytes[index];
        if !byte.is_ascii_lowercase() && !byte.is_ascii_digit() {
            return false;
        }
        index += 1;
    }
    index = colon + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && byte != b'-' && byte != b'.' {
            return false;
        }
        index += 1;
    }
    bytes[colon + 1] != b'-' && bytes[bytes.len() - 1] != b'-'
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
///
/// Identity and admission are orthogonal: a legacy document can carry a
/// registry row of its own while its bytes are read with a newer grammar, and
/// a damaged frame can retain its identity while applying that row's grammar
/// unverified. The wire form lives on [`DialectMatch`], which owns the format
/// namespace the grammar name is local to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Parsed with the strategy declared for the identified dialect.
    Admitted,
    /// Parsed with another declared grammar of the same format namespace.
    ///
    /// The codec must charge its dialect-unverified loss.
    Unverified {
        /// Format-local name of the grammar the parser applied.
        using: Grammar,
    },
    /// Parsed without any declared grammar: the residual path.
    ///
    /// The codec must charge its dialect-unverified loss.
    Residual,
    /// Structurally identified; semantic decode refused.
    Refused,
}

/// Format-local name of a dialect grammar, for example `sch-sw-33103`.
///
/// The namespace is the owning [`DialectMatch`]'s, so a grammar cannot name a
/// foreign format by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grammar(String);

impl Grammar {
    /// Names the format-local half of a registry dialect id.
    #[must_use]
    pub fn of(dialect: &DialectId) -> Self {
        Self(dialect.local().to_owned())
    }

    /// Returns the format-local grammar name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Current wire form of [`Admission`], with the grammar as a full registry id.
#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum AdmissionWire {
    Admitted,
    Unverified { using: DialectId },
    Residual,
    Refused,
}

/// Read wire, including the single legacy admission spelling.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum AdmissionReadWire {
    Admitted,
    Unverified {
        using: DialectId,
    },
    Residual,
    Refused,
    /// Legacy spelling: `using` present is `Unverified`, absent is `Residual`.
    AdmittedUnverified {
        #[serde(default)]
        using: Option<DialectId>,
    },
}

/// One format layer's identification of one document.
///
/// A document carries several format layers: `.sldprt` contains Parasolid;
/// `.f3d`, `.ipt`, and SAT contain ACIS; NX contains Parasolid and JT. One
/// match describes one layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectMatch {
    /// Registry dialect id, for example `"rhino:archive-80"`.
    dialect: DialectId,
    /// Version fields the source declared, verbatim, under keys pinned per
    /// codec in the registry.
    ///
    /// Declarations are evidence, never a control input: the dialect is what
    /// the bytes obey, not what they declare.
    declared: BTreeMap<String, String>,
    /// Instance of this format layer inside the containing document.
    ///
    /// `None` when the layer occurs once or has no report-local identity. This
    /// is not source-declared evidence and therefore does not belong in
    /// [`Self::declared`].
    instance: Option<String>,
    /// How this layer was admitted.
    admission: Admission,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DialectMatchWire<A> {
    format: String,
    dialect: DialectId,
    #[serde(default)]
    declared: BTreeMap<String, String>,
    #[serde(default)]
    instance: Option<String>,
    admission: A,
}

impl<'de> Deserialize<'de> for DialectMatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = DialectMatchWire::<AdmissionReadWire>::deserialize(deserializer)?;
        if wire.format != wire.dialect.namespace() {
            return Err(serde::de::Error::custom(format_args!(
                "dialect {:?} is not in format namespace {:?}",
                wire.dialect.as_str(),
                wire.format
            )));
        }
        let grammar = |using: DialectId| {
            if using.namespace() == wire.dialect.namespace() {
                Ok(Grammar::of(&using))
            } else {
                Err(serde::de::Error::custom(format_args!(
                    "unverified dialect {:?} cannot use grammar from foreign namespace {:?}",
                    wire.dialect.as_str(),
                    using.as_str()
                )))
            }
        };
        let admission = match wire.admission {
            AdmissionReadWire::Admitted => Admission::Admitted,
            AdmissionReadWire::Unverified { using }
            | AdmissionReadWire::AdmittedUnverified { using: Some(using) } => {
                Admission::Unverified {
                    using: grammar(using)?,
                }
            }
            AdmissionReadWire::Residual | AdmissionReadWire::AdmittedUnverified { using: None } => {
                Admission::Residual
            }
            AdmissionReadWire::Refused => Admission::Refused,
        };
        Ok(Self {
            dialect: wire.dialect,
            declared: wire.declared,
            instance: wire.instance,
            admission,
        })
    }
}

impl Serialize for DialectMatch {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("DialectMatch", 5)?;
        state.serialize_field("format", self.format())?;
        state.serialize_field("dialect", &self.dialect)?;
        if !self.declared.is_empty() {
            state.serialize_field("declared", &self.declared)?;
        }
        if let Some(instance) = &self.instance {
            state.serialize_field("instance", instance)?;
        }
        let admission = match &self.admission {
            Admission::Admitted => AdmissionWire::Admitted,
            Admission::Unverified { using } => AdmissionWire::Unverified {
                using: self.grammar_id(using),
            },
            Admission::Residual => AdmissionWire::Residual,
            Admission::Refused => AdmissionWire::Refused,
        };
        state.serialize_field("admission", &admission)?;
        state.end()
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for DialectMatch {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DialectMatch".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::DialectMatch").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        DialectMatchWire::<AdmissionWire>::json_schema(generator)
    }
}

/// Whether a format layer is the sole instance or needs its carrier identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerInstance {
    /// The host contains one layer of this format.
    Sole,
    /// The host contains several layers and the carrier disambiguates this one.
    Tagged,
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
        Ok(wire
            .extra
            .into_iter()
            .fold(Self::of(wire.primary), DialectLayers::with))
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

    /// Inserts a layer keyed by `(format, instance)`.
    ///
    /// The first layer for a key remains authoritative. A duplicate is
    /// returned unchanged so its producer can report the collision.
    pub fn insert(&mut self, layer: DialectMatch) -> Result<(), DialectMatch> {
        if Self::same_key(&self.primary, &layer) {
            return Err(layer);
        }
        if self
            .extra
            .iter()
            .any(|existing| Self::same_key(existing, &layer))
        {
            return Err(layer);
        }
        self.extra.push(layer);
        Ok(())
    }

    /// Adds a layer, replacing a layer with the same key.
    ///
    /// This last-wins builder matches the deserialization policy for tolerant
    /// wire reads. Use [`Self::insert`] when the first layer is authoritative.
    #[must_use]
    pub fn with(mut self, layer: DialectMatch) -> Self {
        if Self::same_key(&self.primary, &layer) {
            self.primary = layer;
        } else if let Some(existing) = self
            .extra
            .iter_mut()
            .find(|existing| Self::same_key(existing, &layer))
        {
            *existing = layer;
        } else {
            self.extra.push(layer);
        }
        self
    }

    fn same_key(existing: &DialectMatch, layer: &DialectMatch) -> bool {
        existing.format() == layer.format() && existing.instance == layer.instance
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

mod format_identity_sealed {
    pub trait Sealed {}
}

/// A classified payload whose format namespace is intrinsic to the payload.
///
/// This trait is sealed. The supported payloads are [`DialectId`],
/// [`DialectMatch`], and [`DialectLayers`].
pub trait FormatIdentityPayload: format_identity_sealed::Sealed {
    /// Returns the payload's authoritative format namespace.
    fn format(&self) -> &str;
}

impl format_identity_sealed::Sealed for DialectId {}

impl FormatIdentityPayload for DialectId {
    fn format(&self) -> &str {
        self.namespace()
    }
}

impl format_identity_sealed::Sealed for DialectMatch {}

impl FormatIdentityPayload for DialectMatch {
    fn format(&self) -> &str {
        self.format()
    }
}

impl format_identity_sealed::Sealed for DialectLayers {}

impl FormatIdentityPayload for DialectLayers {
    fn format(&self) -> &str {
        self.primary().format()
    }
}

/// A format identity that is either classified by a typed payload or retains
/// only its known format.
///
/// Classified state stores no second format string. The payload is the one
/// author of its namespace, so in-memory identity cannot drift. Wire readers
/// use [`Self::from_wire`] to validate a denormalized top-level `format` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatIdentity<T> {
    /// The payload carries the complete classified identity.
    Classified(T),
    /// The format is known but no classification payload exists.
    Unclassified(String),
}

impl<T: FormatIdentityPayload> FormatIdentity<T> {
    /// Constructs an identity whose format comes from its classified payload.
    #[must_use]
    pub fn classified(payload: T) -> Self {
        Self::Classified(payload)
    }

    /// Constructs an identity for a known format without classification.
    #[must_use]
    pub fn unclassified(format: impl Into<String>) -> Self {
        Self::Unclassified(format.into())
    }

    /// Validates and constructs the identity projected by a wire envelope.
    pub fn from_wire(
        format: impl Into<String>,
        payload: Option<T>,
    ) -> Result<Self, FormatIdentityError> {
        let format = format.into();
        match payload {
            Some(payload) if payload.format() == format => Ok(Self::Classified(payload)),
            Some(payload) => Err(FormatIdentityError {
                envelope: format,
                classified: payload.format().to_owned(),
            }),
            None => Ok(Self::Unclassified(format)),
        }
    }

    /// Returns the authoritative format id.
    #[must_use]
    pub fn format(&self) -> &str {
        match self {
            Self::Classified(payload) => payload.format(),
            Self::Unclassified(format) => format,
        }
    }

    /// Returns the classified payload, when present.
    #[must_use]
    pub fn classified_payload(&self) -> Option<&T> {
        match self {
            Self::Classified(payload) => Some(payload),
            Self::Unclassified(_) => None,
        }
    }

    /// Consumes the identity into the denormalized wire fields.
    #[must_use]
    pub fn into_wire_parts(self) -> (String, Option<T>) {
        match self {
            Self::Classified(payload) => {
                let format = payload.format().to_owned();
                (format, Some(payload))
            }
            Self::Unclassified(format) => (format, None),
        }
    }
}

/// A wire envelope's format disagrees with its classified payload.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("format {envelope:?} does not match classified payload format {classified:?}")]
pub struct FormatIdentityError {
    envelope: String,
    classified: String,
}

impl DialectMatch {
    fn with_admission(dialect: DialectId, admission: Admission) -> Self {
        Self {
            dialect,
            declared: BTreeMap::new(),
            instance: None,
            admission,
        }
    }

    /// Constructs a layer parsed with its identified dialect's grammar.
    #[must_use]
    pub fn admitted(dialect: DialectId) -> Self {
        Self::with_admission(dialect, Admission::Admitted)
    }

    /// Constructs a layer parsed unverified with another declared grammar.
    ///
    /// `using` supplies its format-local name; the grammar's namespace is the
    /// layer's own.
    #[must_use]
    pub fn unverified(dialect: DialectId, using: Grammar) -> Self {
        Self::with_admission(dialect, Admission::Unverified { using })
    }

    /// Constructs a layer parsed on the residual path without a declared grammar.
    #[must_use]
    pub fn residual(dialect: DialectId) -> Self {
        Self::with_admission(dialect, Admission::Residual)
    }

    /// Constructs a structurally identified layer whose decode was refused.
    #[must_use]
    pub fn refused(dialect: DialectId) -> Self {
        Self::with_admission(dialect, Admission::Refused)
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
        self.dialect.namespace()
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
    pub fn admission(&self) -> &Admission {
        &self.admission
    }

    /// Returns the full registry id of the grammar applied, when this layer
    /// was parsed unverified with a declared grammar.
    #[must_use]
    pub fn using(&self) -> Option<DialectId> {
        match &self.admission {
            Admission::Unverified { using } => Some(self.grammar_id(using)),
            Admission::Admitted | Admission::Residual | Admission::Refused => None,
        }
    }

    fn grammar_id(&self, grammar: &Grammar) -> DialectId {
        DialectId(Cow::Owned(format!(
            "{}:{}",
            self.format(),
            grammar.as_str()
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[derive(serde::Deserialize)]
    struct DialectIdConformance {
        valid: Vec<String>,
        invalid: Vec<String>,
    }

    fn layer(format: &str) -> DialectMatch {
        DialectMatch::admitted(DialectId::parse(format!("{format}:known")).unwrap())
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
    fn dialect_id_matches_the_shared_conformance_corpus() {
        let cases: DialectIdConformance =
            toml::from_str(include_str!("../../../docs/dialect-id-conformance.toml")).unwrap();
        for id in cases.valid {
            DialectId::parse(id.clone()).unwrap_or_else(|_| panic!("valid dialect id {id:?}"));
        }
        for id in cases.invalid {
            assert!(
                DialectId::parse(id.clone()).is_err(),
                "invalid dialect id {id:?}"
            );
        }
    }

    #[test]
    fn an_admitted_match_serializes_its_identity() {
        let admitted = DialectMatch {
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
    fn residual_constructor_records_the_absence_of_a_declared_grammar() {
        let residual = DialectMatch::residual(DialectId::pinned("rhino:unknown"));

        assert_eq!(residual.admission(), &Admission::Residual);
        assert_eq!(residual.using(), None);
        assert_eq!(
            serde_json::to_string(&residual).unwrap(),
            "{\"format\":\"rhino\",\"dialect\":\"rhino:unknown\",\"admission\":\"residual\"}"
        );
    }

    #[test]
    fn an_unverified_admission_names_the_grammar_in_use_by_full_id() {
        let unverified = DialectMatch::unverified(
            DialectId::pinned("acis:save-format-217"),
            Grammar::of(&DialectId::pinned("acis:save-format-218")),
        );

        assert_eq!(
            unverified.admission(),
            &Admission::Unverified {
                using: Grammar::of(&DialectId::pinned("acis:save-format-218")),
            }
        );
        assert_eq!(
            unverified.using(),
            Some(DialectId::pinned("acis:save-format-218"))
        );
        let serialized = serde_json::to_string(&unverified).unwrap();
        assert_eq!(
            serialized,
            "{\"format\":\"acis\",\"dialect\":\"acis:save-format-217\",\"admission\":{\"unverified\":{\"using\":\"acis:save-format-218\"}}}"
        );
        assert_eq!(
            serde_json::from_str::<DialectMatch>(&serialized).unwrap(),
            unverified
        );
    }

    #[test]
    fn a_self_named_unverified_grammar_remains_opaque() {
        let self_named = serde_json::json!({
            "format": "rhino",
            "dialect": "rhino:unknown",
            "admission": {
                "unverified": { "using": "rhino:unknown" }
            },
        });
        let matched = serde_json::from_value::<DialectMatch>(self_named)
            .expect("core does not infer grammar semantics from an opaque dialect name");
        assert_eq!(
            matched.admission(),
            &Admission::Unverified {
                using: Grammar::of(&DialectId::pinned("rhino:unknown")),
            }
        );
    }

    #[test]
    fn legacy_admitted_unverified_migrates_on_read() {
        let with_grammar = serde_json::json!({
            "format": "rhino",
            "dialect": "rhino:unknown",
            "admission": { "admitted_unverified": { "using": "rhino:archive-80" } },
        });
        let without_grammar = serde_json::json!({
            "format": "rhino",
            "dialect": "rhino:unknown",
            "admission": { "admitted_unverified": {} },
        });

        assert_eq!(
            serde_json::from_value::<DialectMatch>(with_grammar).unwrap(),
            DialectMatch::unverified(
                DialectId::pinned("rhino:unknown"),
                Grammar::of(&DialectId::pinned("rhino:archive-80"))
            )
        );
        assert_eq!(
            serde_json::from_value::<DialectMatch>(without_grammar).unwrap(),
            DialectMatch::residual(DialectId::pinned("rhino:unknown"))
        );
    }

    #[cfg(feature = "schema")]
    #[test]
    fn current_dialect_schema_excludes_the_legacy_admission_spelling() {
        let schema = serde_json::to_string(&schemars::schema_for!(DialectMatch)).unwrap();
        assert!(schema.contains("unverified"), "{schema}");
        assert!(schema.contains("residual"), "{schema}");
        assert!(!schema.contains("admitted_unverified"), "{schema}");
    }

    #[test]
    fn dialect_match_deserialization_rejects_a_foreign_grammar_namespace() {
        let malformed = serde_json::json!({
            "format": "rhino",
            "dialect": "rhino:unknown",
            "admission": {
                "unverified": { "using": "step:ap242-e3" }
            },
        });
        let error = serde_json::from_value::<DialectMatch>(malformed)
            .expect_err("a grammar substitute belongs to the classified format layer");
        assert!(
            error.to_string().contains(
                "unverified dialect \"rhino:unknown\" cannot use grammar from foreign namespace \"step:ap242-e3\""
            ),
            "{error}"
        );
    }

    #[test]
    fn identity_does_not_encode_whether_an_unverified_path_used_a_grammar() {
        let dialect = DialectId::pinned("rhino:archive-80");
        let without_grammar = DialectMatch::residual(dialect.clone());
        assert_eq!(without_grammar.admission(), &Admission::Residual);

        let self_named = DialectMatch::unverified(dialect.clone(), Grammar::of(&dialect));
        assert_eq!(
            self_named.admission(),
            &Admission::Unverified {
                using: Grammar::of(&DialectId::pinned("rhino:archive-80")),
            }
        );
        assert_eq!(self_named.using(), Some(dialect));
    }

    #[test]
    fn dialect_layers_accept_a_same_format_extra_with_an_instance() {
        let member = layer("rhino").with_instance("components/member.3dm");
        let layers = DialectLayers::of(layer("rhino")).with(member.clone());

        let serialized = serde_json::to_value(&layers).unwrap();
        assert_eq!(
            serde_json::from_value::<DialectLayers>(serialized).unwrap(),
            layers
        );
        assert_eq!(layers.into_parts().1, [member]);
    }

    #[test]
    fn dialect_layers_insert_keeps_the_first_extra_layer_for_a_key() {
        let first = layer("acis").with_instance("body");
        let replacement =
            DialectMatch::residual(DialectId::pinned("acis:other")).with_instance("body");
        let mut layers = DialectLayers::of(layer("rhino")).with(first.clone());

        assert_eq!(layers.insert(replacement.clone()), Err(replacement));
        assert_eq!(layers.into_parts().1, [first]);
    }

    #[test]
    fn dialect_layers_insert_keeps_the_primary_on_its_own_key() {
        let replacement = DialectMatch::residual(DialectId::pinned("rhino:other"));
        let mut layers = DialectLayers::of(layer("rhino")).with(layer("acis"));

        assert_eq!(layers.insert(replacement.clone()), Err(replacement));
        assert_eq!(layers.primary(), &layer("rhino"));
        assert_eq!(layers.into_parts().1, [layer("acis")]);
    }

    #[test]
    fn dialect_layers_keep_same_format_extras_with_distinct_instances() {
        let anonymous = layer("acis");
        let named = layer("acis").with_instance("body");
        let mut layers = DialectLayers::of(layer("rhino"));

        assert_eq!(layers.insert(anonymous.clone()), Ok(()));
        assert_eq!(layers.insert(named.clone()), Ok(()));
        assert_eq!(layers.into_parts().1, [anonymous, named]);
    }

    #[test]
    fn dialect_layers_deserialization_keeps_the_last_layer_per_key() {
        let serialized = serde_json::json!({
            "primary": layer("rhino"),
            "extra": [layer("acis"), DialectMatch::residual(DialectId::pinned("acis:other"))],
        });

        let layers = serde_json::from_value::<DialectLayers>(serialized).unwrap();
        assert_eq!(
            layers.into_parts().1,
            [DialectMatch::residual(DialectId::pinned("acis:other"))]
        );
    }

    #[test]
    fn dialect_layers_serialize_with_an_explicit_primary() {
        let layers = DialectLayers::of(layer("rhino")).with(layer("acis"));
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
            layers.iter().map(DialectMatch::format).collect::<Vec<_>>(),
            ["rhino", "acis"]
        );
    }
}
