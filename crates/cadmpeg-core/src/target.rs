// SPDX-License-Identifier: Apache-2.0
//! Encoder target catalogs and typed target-selection refusals.

use std::collections::BTreeMap;
use std::fmt;

use crate::dialect::DialectId;

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
    /// True on at most one entry: the cross-format conversion default.
    ///
    /// A catalog may have no default when the encoder cannot synthesize a
    /// document from a source of another format.
    pub default: bool,
}

/// Panics when a static encoder target catalog violates its uniqueness rules.
///
/// Every spelling accepted by [`find_target`] belongs to one row. A row may
/// repeat its own format-local id as an alias, but no spelling may select two
/// different rows.
pub fn assert_valid_target_catalog(targets: &[TargetDescriptor]) {
    let defaults = targets.iter().filter(|target| target.default).count();
    assert!(
        defaults <= 1,
        "target catalog invariant failed: at most one entry may be the default"
    );

    let mut tokens = BTreeMap::<&str, usize>::new();
    for (index, target) in targets.iter().enumerate() {
        let local = target
            .id
            .as_str()
            .split_once(':')
            .map_or(target.id.as_str(), |(_, local)| local);
        for token in std::iter::once(target.id.as_str())
            .chain(std::iter::once(local))
            .chain(target.aliases.iter().copied())
        {
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

/// The catalog entry `token` names, by full id, format-local id, or alias.
///
/// A format-local id is the part after the first colon. The caller has already
/// selected an encoder, so `archive-60` is unambiguous within the Rhino
/// catalog and lets `--to rhino:archive-60` pass its right half unchanged.
#[must_use]
pub fn find_target<'a>(
    targets: &'a [TargetDescriptor],
    token: &str,
) -> Option<&'a TargetDescriptor> {
    targets.iter().find(|target| {
        target.id.as_str() == token
            || target
                .id
                .as_str()
                .split_once(':')
                .is_some_and(|(_, local)| local == token)
            || target.aliases.contains(&token)
    })
}

/// The catalog's cross-format default, or `None` when none is declared.
#[must_use]
pub fn default_target(targets: &'static [TargetDescriptor]) -> Option<&'static TargetDescriptor> {
    targets.iter().find(|target| target.default)
}

/// A caller-supplied dialect token requested from an encoder.
///
/// Unlike [`DialectId`], this token need not name a registered dialect:
/// refusals preserve an unknown explicit request verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TargetRefusalKind {
    /// An explicit token names no entry in the encoder catalog.
    UnknownExplicit {
        /// Encoder format.
        format: String,
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
    UnrecordedSource {
        /// Encoder and source format.
        format: String,
    },
    /// Inheritance had no same-format source and the catalog declares no default.
    NoDefault {
        /// Encoder format.
        format: String,
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TargetRefusal {
    kind: TargetRefusalKind,
    available: &'static [TargetDescriptor],
}

/// Why inheritance must select an encoder catalog default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultSource {
    /// The document records no source metadata.
    NoSource,
    /// The document's source belongs to another format.
    ForeignFormat(String),
}

impl TargetRefusal {
    /// Associates one request-state reason with the refusing encoder catalog.
    #[must_use]
    pub const fn new(kind: TargetRefusalKind, available: &'static [TargetDescriptor]) -> Self {
        Self { kind, available }
    }

    /// Builds the refusal for an explicit token outside an encoder catalog.
    #[must_use]
    pub fn unknown_explicit(
        format: impl Into<String>,
        requested: impl Into<String>,
        available: &'static [TargetDescriptor],
    ) -> Self {
        Self::new(
            TargetRefusalKind::UnknownExplicit {
                format: format.into(),
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
        match &self.kind {
            TargetRefusalKind::UnknownExplicit { format, .. }
            | TargetRefusalKind::UnrecordedSource { format }
            | TargetRefusalKind::NoDefault { format, .. } => format,
            TargetRefusalKind::ExplicitUnavailable { target, .. }
            | TargetRefusalKind::DefaultUnavailable { target, .. } => target.namespace(),
            TargetRefusalKind::InheritedUnavailable { source, .. } => source.namespace(),
        }
    }

    /// Returns the encoder's structured synthesis catalog.
    #[must_use]
    pub const fn available(&self) -> &'static [TargetDescriptor] {
        self.available
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
            TargetRefusalKind::UnrecordedSource { .. }
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
            | TargetRefusalKind::UnrecordedSource { .. }
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
        match &self.kind {
            TargetRefusalKind::UnknownExplicit {
                format, requested, ..
            } => write!(
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
                "{} cannot write explicit target {requested} ({target}): {reason}",
                target.namespace()
            )?,
            TargetRefusalKind::InheritedUnavailable { source, reason, .. } => write!(
                f,
                "{} cannot preserve source dialect {source}: {reason}",
                source.namespace()
            )?,
            TargetRefusalKind::UnrecordedSource { format, .. } => write!(
                f,
                "{format} cannot inherit a write target: the {format} source records no dialect; name an explicit target"
            )?,
            TargetRefusalKind::NoDefault {
                format,
                source: DefaultSource::ForeignFormat(source_format),
                ..
            } => write!(
                f,
                "{format} cannot inherit a write target from source format {source_format}: this encoder declares no cross-format default"
            )?,
            TargetRefusalKind::NoDefault {
                format,
                source: DefaultSource::NoSource,
                ..
            } => write!(
                f,
                "{format} cannot select an inherited write target: the document records no source format and this encoder declares no default"
            )?,
            TargetRefusalKind::DefaultUnavailable {
                target,
                source,
                reason,
                ..
            } => {
                write!(f, "{} cannot write default target {target}", target.namespace())?;
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
        default: false,
    }];

    fn target(
        id: &'static str,
        aliases: &'static [&'static str],
        default: bool,
    ) -> TargetDescriptor {
        TargetDescriptor {
            id: DialectId::pinned(id),
            aliases,
            default,
        }
    }

    #[test]
    #[should_panic(expected = "at most one entry may be the default")]
    fn a_target_catalog_rejects_multiple_defaults() {
        let targets = [
            target("test:first", NO_ALIASES, true),
            target("test:second", NO_ALIASES, true),
        ];
        assert_valid_target_catalog(&targets);
    }

    #[test]
    #[should_panic(expected = "token \"test:same\" selects both")]
    fn a_target_catalog_rejects_duplicate_ids() {
        let targets = [
            target("test:same", NO_ALIASES, false),
            target("test:same", NO_ALIASES, false),
        ];
        assert_valid_target_catalog(&targets);
    }

    #[test]
    #[should_panic(expected = "token \"same\" selects both")]
    fn a_target_catalog_rejects_duplicate_aliases() {
        let targets = [
            target("test:first", &["same"], false),
            target("test:second", &["same"], false),
        ];
        assert_valid_target_catalog(&targets);
    }

    #[test]
    #[should_panic(expected = "token \"test:second\" selects both")]
    fn a_target_catalog_rejects_an_alias_that_is_an_id() {
        let targets = [
            target("test:first", &["test:second"], false),
            target("test:second", NO_ALIASES, false),
        ];
        assert_valid_target_catalog(&targets);
    }

    #[test]
    #[should_panic(expected = "token \"second\" selects both")]
    fn a_target_catalog_rejects_an_alias_that_is_another_rows_local_id() {
        let targets = [
            target("test:first", &["second"], false),
            target("test:second", NO_ALIASES, false),
        ];
        assert_valid_target_catalog(&targets);
    }

    #[test]
    fn missing_default_names_a_foreign_source_without_inventing_a_dialect() {
        let refusal = TargetRefusal::new(
            TargetRefusalKind::NoDefault {
                format: "fcstd".into(),
                source: DefaultSource::ForeignFormat("step".into()),
            },
            TARGETS,
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
            TargetRefusalKind::UnrecordedSource {
                format: "fcstd".into(),
            },
            TARGETS,
        );

        assert_eq!(
            refusal.to_string(),
            "fcstd cannot inherit a write target: the fcstd source records no dialect; name an explicit target; available targets: fcstd:schema-4"
        );
    }
}
