// SPDX-License-Identifier: Apache-2.0
//! Encoder target catalogs and typed target-selection refusals.

use std::collections::BTreeMap;
use std::fmt;

use crate::dialect::DialectId;
use serde::ser::{SerializeSeq, Serializer};
use serde::Serialize;

/// One dialect that a caller can request from an encoder.
///
/// Synthesis is a static capability. The catalog states ids, aliases, and
/// the cross-format default; input-conditioned preservation remains a write
/// planner decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDescriptor {
    /// Registry dialect id, e.g. `step:ap242-e3`.
    pub id: DialectId,
    /// Short spellings accepted for `id`, e.g. `["6"]` for `rhino:archive-60`.
    pub aliases: &'static [&'static str],
}

impl TargetDescriptor {
    /// Every token accepted for this target: full id, format-local id, and aliases.
    pub fn accepted_tokens(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str())
            .chain(std::iter::once(self.id.local()))
            .chain(self.aliases.iter().copied())
    }
}

/// An encoder's synthesis targets and its optional cross-format default.
///
/// The default is a position in `targets`, validated when the catalog is
/// constructed. A catalog may have no default when the encoder cannot
/// synthesize a document from a source of another format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetCatalog {
    targets: &'static [TargetDescriptor],
    default: Option<usize>,
}

impl TargetCatalog {
    /// The empty catalog used by a dialect-free encoder.
    pub const EMPTY: Self = Self::new(&[], None);

    /// Builds a catalog whose optional default indexes one of its rows.
    #[must_use]
    pub const fn new(targets: &'static [TargetDescriptor], default: Option<usize>) -> Self {
        if let Some(index) = default {
            assert!(
                index < targets.len(),
                "target catalog default is out of bounds"
            );
        }
        Self { targets, default }
    }

    /// Returns all synthesis targets in catalog order.
    #[must_use]
    pub const fn targets(self) -> &'static [TargetDescriptor] {
        self.targets
    }

    /// Returns whether this catalog has no synthesis targets.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.targets.is_empty()
    }

    /// Returns the number of synthesis targets.
    #[must_use]
    pub const fn len(self) -> usize {
        self.targets.len()
    }

    /// Iterates over synthesis targets in catalog order.
    pub fn iter(self) -> std::slice::Iter<'static, TargetDescriptor> {
        self.targets.iter()
    }

    /// Returns the row named by a full id, format-local id, or alias, with its
    /// position in the catalog.
    #[must_use]
    pub fn find(self, token: &str) -> Option<(usize, &'static TargetDescriptor)> {
        self.targets
            .iter()
            .enumerate()
            .find(|(_, target)| target.accepted_tokens().any(|accepted| accepted == token))
    }

    /// Returns the cross-format default and its position, when declared.
    #[must_use]
    pub fn default(self) -> Option<(usize, &'static TargetDescriptor)> {
        self.default.map(|index| (index, &self.targets[index]))
    }
}

impl IntoIterator for TargetCatalog {
    type Item = &'static TargetDescriptor;
    type IntoIter = std::slice::Iter<'static, TargetDescriptor>;

    fn into_iter(self) -> Self::IntoIter {
        self.targets.iter()
    }
}

impl Serialize for TargetCatalog {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct TargetDescriptorWire<'a> {
            id: &'a DialectId,
            aliases: &'static [&'static str],
            default: bool,
        }

        let mut sequence = serializer.serialize_seq(Some(self.targets.len()))?;
        for (index, target) in self.targets.iter().enumerate() {
            sequence.serialize_element(&TargetDescriptorWire {
                id: &target.id,
                aliases: target.aliases,
                default: self.default == Some(index),
            })?;
        }
        sequence.end()
    }
}

/// Panics when a static encoder target catalog violates its uniqueness rules.
///
/// Every accepted spelling belongs to one row. A row may
/// repeat its own format-local id as an alias, but no spelling may select two
/// different rows.
pub fn assert_valid_target_catalog(catalog: TargetCatalog) {
    let targets = catalog.targets();
    let mut tokens = BTreeMap::<&str, usize>::new();
    for (index, target) in targets.iter().enumerate() {
        for token in target.accepted_tokens() {
            if let Some(previous) = tokens.insert(token, index) {
                assert_eq!(
                    previous, index,
                    "target catalog invariant failed: token {token:?} selects both {:?} and {:?}",
                    targets[previous].id, target.id
                );
            }
        }
    }
}

/// A caller-supplied dialect token requested from an encoder.
///
/// Unlike [`DialectId`], this token need not name a registered dialect:
/// refusals preserve an unknown explicit request verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TargetToken(String);

impl TargetToken {
    /// Retains one requested target token verbatim.
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// Returns the requested token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One target-refusal reason, independent of the catalog presented with it.
///
/// Each request state is distinct. In particular, absence of an explicit
/// token does not conflate an unclassified same-format source with a foreign
/// source and a missing cross-format default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TargetRefusalKind {
    /// An explicit token names no entry in the encoder catalog.
    UnknownExplicit {
        /// Token supplied by the caller, retained verbatim.
        requested: TargetToken,
    },
    /// A catalog target was selected explicitly but this input cannot reach it.
    ExplicitUnavailable {
        /// Canonical catalog target selected by the token.
        target: DialectId,
        /// Token supplied by the caller, retained verbatim.
        requested: TargetToken,
        /// Input-conditioned reason the writer cannot deliver the target.
        reason: String,
    },
    /// Same-format inheritance selected a source dialect the writer cannot preserve.
    InheritedUnavailable {
        /// Recorded source dialect selected by inheritance.
        source: DialectId,
        /// Input-conditioned reason the writer cannot preserve it.
        reason: String,
    },
    /// Same-format inheritance found source metadata without a dialect.
    UnrecordedSource,
    /// Inheritance had no same-format source and the catalog declares no default.
    NoDefault {
        /// Why no same-format source identity was available to inherit.
        source: DefaultSource,
    },
    /// A cross-format default was selected but this input cannot reach it.
    DefaultUnavailable {
        /// Canonical catalog default.
        target: DialectId,
        /// Why the catalog default, rather than source inheritance, was selected.
        source: DefaultSource,
        /// Input-conditioned reason the writer cannot deliver the target.
        reason: String,
    },
}

/// Why an encoder could not resolve or deliver a write target.
///
/// The refusal carries the encoder catalog once, beside the request-state
/// reason, so every reason is rendered and reported against the same catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct TargetRefusal {
    /// Refusing encoder format, stated once for every request state.
    format: String,
    #[serde(flatten)]
    kind: TargetRefusalKind,
    available: TargetCatalog,
}

/// Why inheritance must select an encoder catalog default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "format")]
pub enum DefaultSource {
    /// The document records no source metadata.
    NoSource,
    /// The document's source belongs to another format.
    ForeignFormat(String),
}

impl TargetRefusal {
    /// Associates one request-state reason with the refusing encoder format
    /// and catalog.
    #[must_use]
    pub fn new(
        format: impl Into<String>,
        kind: TargetRefusalKind,
        available: TargetCatalog,
    ) -> Self {
        Self {
            format: format.into(),
            kind,
            available,
        }
    }

    /// Builds the refusal for an explicit token outside an encoder catalog.
    #[must_use]
    pub fn unknown_explicit(
        format: impl Into<String>,
        requested: impl Into<String>,
        available: TargetCatalog,
    ) -> Self {
        Self::new(
            format,
            TargetRefusalKind::UnknownExplicit {
                requested: TargetToken::new(requested),
            },
            available,
        )
    }

    /// Returns the request-state reason.
    #[must_use]
    pub const fn kind(&self) -> &TargetRefusalKind {
        &self.kind
    }

    /// Returns the refusing encoder format.
    #[must_use]
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Returns the encoder's structured synthesis catalog.
    #[must_use]
    pub const fn available(&self) -> &'static [TargetDescriptor] {
        self.available.targets()
    }

    /// Returns the dialect spelling the refusal is about, when one exists.
    ///
    /// Explicit requests retain the caller's spelling. Inherited refusals
    /// return the recorded source dialect. Missing-source and missing-default
    /// states have no requested dialect.
    #[must_use]
    pub fn requested(&self) -> Option<&str> {
        match &self.kind {
            TargetRefusalKind::UnknownExplicit { requested, .. }
            | TargetRefusalKind::ExplicitUnavailable { requested, .. } => Some(requested.as_str()),
            TargetRefusalKind::InheritedUnavailable { source, .. } => Some(source.as_str()),
            TargetRefusalKind::UnrecordedSource
            | TargetRefusalKind::NoDefault { .. }
            | TargetRefusalKind::DefaultUnavailable { .. } => None,
        }
    }

    /// Returns the input-conditioned delivery reason, when this is a resolved
    /// target rather than a target-selection failure.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match &self.kind {
            TargetRefusalKind::ExplicitUnavailable { reason, .. }
            | TargetRefusalKind::InheritedUnavailable { reason, .. }
            | TargetRefusalKind::DefaultUnavailable { reason, .. } => Some(reason),
            TargetRefusalKind::UnknownExplicit { .. }
            | TargetRefusalKind::UnrecordedSource
            | TargetRefusalKind::NoDefault { .. } => None,
        }
    }

    fn write_available(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("; available targets: ")?;
        let mut targets = self.available().iter();
        let Some(first) = targets.next() else {
            return f.write_str("none");
        };
        f.write_str(first.id.as_str())?;
        for target in targets {
            write!(f, ", {}", target.id)?;
        }
        Ok(())
    }
}

impl fmt::Display for TargetRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let format = &self.format;
        match &self.kind {
            TargetRefusalKind::UnknownExplicit { requested } => write!(
                f,
                "{format} cannot write {requested}: not a target this encoder can synthesize"
            )?,
            TargetRefusalKind::ExplicitUnavailable {
                target,
                requested,
                reason,
                ..
            } => write!(
                f,
                "{format} cannot write explicit target {requested} ({target}): {reason}"
            )?,
            TargetRefusalKind::InheritedUnavailable { source, reason } => write!(
                f,
                "{format} cannot preserve source dialect {source}: {reason}"
            )?,
            TargetRefusalKind::UnrecordedSource => write!(
                f,
                "{format} cannot inherit a write target: the {format} source records no dialect; name an explicit target"
            )?,
            TargetRefusalKind::NoDefault {
                source: DefaultSource::ForeignFormat(source_format),
            } => write!(
                f,
                "{format} cannot inherit a write target from source format {source_format}: this encoder declares no cross-format default"
            )?,
            TargetRefusalKind::NoDefault {
                source: DefaultSource::NoSource,
            } => write!(
                f,
                "{format} cannot select an inherited write target: the document records no source format and this encoder declares no default"
            )?,
            TargetRefusalKind::DefaultUnavailable {
                target,
                source,
                reason,
            } => {
                write!(f, "{format} cannot write default target {target}")?;
                if let DefaultSource::ForeignFormat(source_format) = source {
                    write!(f, " selected for source format {source_format}")?;
                }
                write!(f, ": {reason}")?;
            }
        }
        self.write_available(f)
    }
}

impl std::error::Error for TargetRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_ALIASES: &[&str] = &[];

    const TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
        id: DialectId::pinned("fcstd:schema-4"),
        aliases: &["4"],
    }];

    const fn target(id: &'static str, aliases: &'static [&'static str]) -> TargetDescriptor {
        TargetDescriptor {
            id: DialectId::pinned(id),
            aliases,
        }
    }

    #[test]
    fn target_refusal_serializes_request_state_and_the_complete_catalog() {
        let refusal = TargetRefusal::new(
            "fcstd",
            TargetRefusalKind::ExplicitUnavailable {
                target: DialectId::pinned("fcstd:schema-4"),
                requested: TargetToken::new("4"),
                reason: "the source image cannot be patched".into(),
            },
            TargetCatalog::new(TARGETS, None),
        );

        assert_eq!(
            serde_json::to_value(refusal).expect("target refusal serializes"),
            serde_json::json!({
                "format": "fcstd",
                "kind": "explicit_unavailable",
                "target": "fcstd:schema-4",
                "requested": "4",
                "reason": "the source image cannot be patched",
                "available": [{
                    "id": "fcstd:schema-4",
                    "aliases": ["4"],
                    "default": false
                }]
            })
        );
    }

    #[test]
    fn target_lookup_accepts_each_owned_spelling_and_rejects_a_miss() {
        const TARGETS: &[TargetDescriptor] = &[target("test:first", &["one", "primary"])];
        let catalog = TargetCatalog::new(TARGETS, None);

        assert_eq!(
            TARGETS[0].accepted_tokens().collect::<Vec<_>>(),
            vec!["test:first", "first", "one", "primary"]
        );
        for token in ["test:first", "first", "one", "primary"] {
            assert_eq!(
                catalog.find(token).map(|(_, entry)| entry.id.as_str()),
                Some("test:first"),
                "lookup failed for {token:?}"
            );
        }
        assert_eq!(catalog.find("missing"), None);
    }

    #[test]
    #[should_panic(expected = "target catalog default is out of bounds")]
    fn a_target_catalog_rejects_an_invalid_default_index() {
        let _ = TargetCatalog::new(TARGETS, Some(TARGETS.len()));
    }

    #[test]
    #[should_panic(expected = "token \"test:same\" selects both")]
    fn a_target_catalog_rejects_duplicate_ids() {
        const DUPLICATES: &[TargetDescriptor] = &[
            target("test:same", NO_ALIASES),
            target("test:same", NO_ALIASES),
        ];
        assert_valid_target_catalog(TargetCatalog::new(DUPLICATES, None));
    }

    #[test]
    #[should_panic(expected = "token \"same\" selects both")]
    fn a_target_catalog_rejects_duplicate_aliases() {
        const DUPLICATES: &[TargetDescriptor] = &[
            target("test:first", &["same"]),
            target("test:second", &["same"]),
        ];
        assert_valid_target_catalog(TargetCatalog::new(DUPLICATES, None));
    }

    #[test]
    #[should_panic(expected = "token \"test:second\" selects both")]
    fn a_target_catalog_rejects_an_alias_that_is_an_id() {
        const DUPLICATES: &[TargetDescriptor] = &[
            target("test:first", &["test:second"]),
            target("test:second", NO_ALIASES),
        ];
        assert_valid_target_catalog(TargetCatalog::new(DUPLICATES, None));
    }

    #[test]
    #[should_panic(expected = "token \"second\" selects both")]
    fn a_target_catalog_rejects_an_alias_that_is_another_rows_local_id() {
        const DUPLICATES: &[TargetDescriptor] = &[
            target("test:first", &["second"]),
            target("test:second", NO_ALIASES),
        ];
        assert_valid_target_catalog(TargetCatalog::new(DUPLICATES, None));
    }

    #[test]
    fn missing_default_names_a_foreign_source_without_inventing_a_dialect() {
        let refusal = TargetRefusal::new(
            "fcstd",
            TargetRefusalKind::NoDefault {
                source: DefaultSource::ForeignFormat("step".into()),
            },
            TargetCatalog::new(TARGETS, None),
        );

        assert_eq!(refusal.requested(), None);
        assert_eq!(refusal.available(), TARGETS);
        assert_eq!(
            refusal.to_string(),
            "fcstd cannot inherit a write target from source format step: this encoder declares no cross-format default; available targets: fcstd:schema-4"
        );
    }

    #[test]
    fn unrecorded_same_format_source_is_not_a_missing_default() {
        let refusal = TargetRefusal::new(
            "fcstd",
            TargetRefusalKind::UnrecordedSource,
            TargetCatalog::new(TARGETS, None),
        );

        assert_eq!(
            refusal.to_string(),
            "fcstd cannot inherit a write target: the fcstd source records no dialect; name an explicit target; available targets: fcstd:schema-4"
        );
    }
}
