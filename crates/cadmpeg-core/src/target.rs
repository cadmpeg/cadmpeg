// SPDX-License-Identifier: Apache-2.0
//! Encoder target catalogs and typed target-selection refusals.

use std::fmt;

use crate::dialect::DialectId;

/// One dialect that a caller can request from an encoder.
///
/// Synthesis is a static capability. The catalog states names, aliases, and
/// the cross-format default; input-conditioned preservation remains a write
/// planner decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDescriptor {
    /// Registry dialect id, e.g. `step:ap242-e3`.
    pub id: DialectId,
    /// Human-readable name, e.g. `STEP AP242 edition 3`.
    pub label: &'static str,
    /// Short spellings accepted for `id`, e.g. `["6"]` for `rhino:archive-60`.
    pub aliases: &'static [&'static str],
    /// True on at most one entry: the cross-format conversion default.
    ///
    /// A catalog may have no default when the encoder cannot synthesize a
    /// document from a source of another format.
    pub default: bool,
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

/// Why an encoder could not resolve or deliver a write target.
///
/// Each request state is distinct. In particular, absence of an explicit
/// token does not conflate an unclassified same-format source with a foreign
/// source and a missing cross-format default.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TargetRefusal {
    /// An explicit token names no entry in the encoder catalog.
    UnknownExplicit {
        /// Encoder format.
        format: String,
        /// Token supplied by the caller, retained verbatim.
        requested: TargetToken,
        /// Encoder synthesis catalog in declared order.
        available: &'static [TargetDescriptor],
    },
    /// A catalog target was selected explicitly but this input cannot reach it.
    ExplicitUnavailable {
        /// Canonical catalog target selected by the token.
        target: DialectId,
        /// Token supplied by the caller, retained verbatim.
        requested: TargetToken,
        /// Input-conditioned reason the writer cannot deliver the target.
        reason: String,
        /// Encoder synthesis catalog in declared order.
        available: &'static [TargetDescriptor],
    },
    /// Same-format inheritance selected a source dialect the writer cannot preserve.
    InheritedUnavailable {
        /// Recorded source dialect selected by inheritance.
        source: DialectId,
        /// Input-conditioned reason the writer cannot preserve it.
        reason: String,
        /// Encoder synthesis catalog in declared order.
        available: &'static [TargetDescriptor],
    },
    /// Same-format inheritance found source metadata without a dialect.
    UnrecordedSource {
        /// Encoder and source format.
        format: String,
        /// Encoder synthesis catalog in declared order.
        available: &'static [TargetDescriptor],
    },
    /// Inheritance had no same-format source and the catalog declares no default.
    NoDefault {
        /// Encoder format.
        format: String,
        /// Why no same-format source identity was available to inherit.
        source: DefaultSource,
        /// Encoder synthesis catalog in declared order.
        available: &'static [TargetDescriptor],
    },
    /// A cross-format default was selected but this input cannot reach it.
    DefaultUnavailable {
        /// Canonical catalog default.
        target: DialectId,
        /// Why the catalog default, rather than source inheritance, was selected.
        source: DefaultSource,
        /// Input-conditioned reason the writer cannot deliver the target.
        reason: String,
        /// Encoder synthesis catalog in declared order.
        available: &'static [TargetDescriptor],
    },
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
    /// Returns the refusing encoder format.
    #[must_use]
    pub fn format(&self) -> &str {
        match self {
            Self::UnknownExplicit { format, .. }
            | Self::UnrecordedSource { format, .. }
            | Self::NoDefault { format, .. } => format,
            Self::ExplicitUnavailable { target, .. } | Self::DefaultUnavailable { target, .. } => {
                target.namespace()
            }
            Self::InheritedUnavailable { source, .. } => source.namespace(),
        }
    }

    /// Returns the encoder's structured synthesis catalog.
    #[must_use]
    pub const fn available(&self) -> &'static [TargetDescriptor] {
        match self {
            Self::UnknownExplicit { available, .. }
            | Self::ExplicitUnavailable { available, .. }
            | Self::InheritedUnavailable { available, .. }
            | Self::UnrecordedSource { available, .. }
            | Self::NoDefault { available, .. }
            | Self::DefaultUnavailable { available, .. } => available,
        }
    }

    /// Returns the dialect spelling the refusal is about, when one exists.
    ///
    /// Explicit requests retain the caller's spelling. Inherited refusals
    /// return the recorded source dialect. Missing-source and missing-default
    /// states have no requested dialect.
    #[must_use]
    pub fn requested(&self) -> Option<&str> {
        match self {
            Self::UnknownExplicit { requested, .. }
            | Self::ExplicitUnavailable { requested, .. } => Some(requested.as_str()),
            Self::InheritedUnavailable { source, .. } => Some(source.as_str()),
            Self::UnrecordedSource { .. }
            | Self::NoDefault { .. }
            | Self::DefaultUnavailable { .. } => None,
        }
    }

    /// Returns the input-conditioned delivery reason, when this is a resolved
    /// target rather than a target-selection failure.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::ExplicitUnavailable { reason, .. }
            | Self::InheritedUnavailable { reason, .. }
            | Self::DefaultUnavailable { reason, .. } => Some(reason),
            Self::UnknownExplicit { .. }
            | Self::UnrecordedSource { .. }
            | Self::NoDefault { .. } => None,
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
        match self {
            Self::UnknownExplicit {
                format, requested, ..
            } => write!(
                f,
                "{format} cannot write {requested}: not a target this encoder can synthesize"
            )?,
            Self::ExplicitUnavailable {
                target,
                requested,
                reason,
                ..
            } => write!(
                f,
                "{} cannot write explicit target {requested} ({target}): {reason}",
                target.namespace()
            )?,
            Self::InheritedUnavailable { source, reason, .. } => write!(
                f,
                "{} cannot preserve source dialect {source}: {reason}",
                source.namespace()
            )?,
            Self::UnrecordedSource { format, .. } => write!(
                f,
                "{format} cannot inherit a write target: the {format} source records no dialect; name an explicit target"
            )?,
            Self::NoDefault {
                format,
                source: DefaultSource::ForeignFormat(source_format),
                ..
            } => write!(
                f,
                "{format} cannot inherit a write target from source format {source_format}: this encoder declares no cross-format default"
            )?,
            Self::NoDefault {
                format,
                source: DefaultSource::NoSource,
                ..
            } => write!(
                f,
                "{format} cannot select an inherited write target: the document records no source format and this encoder declares no default"
            )?,
            Self::DefaultUnavailable {
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

    const TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
        id: DialectId::pinned("fcstd:schema-4"),
        label: "FreeCAD schema 4",
        aliases: &["4"],
        default: false,
    }];

    #[test]
    fn missing_default_names_a_foreign_source_without_inventing_a_dialect() {
        let refusal = TargetRefusal::NoDefault {
            format: "fcstd".into(),
            source: DefaultSource::ForeignFormat("step".into()),
            available: TARGETS,
        };

        assert_eq!(refusal.requested(), None);
        assert_eq!(refusal.available(), TARGETS);
        assert_eq!(
            refusal.to_string(),
            "fcstd cannot inherit a write target from source format step: this encoder declares no cross-format default; available targets: fcstd:schema-4"
        );
    }

    #[test]
    fn unrecorded_same_format_source_is_not_a_missing_default() {
        let refusal = TargetRefusal::UnrecordedSource {
            format: "fcstd".into(),
            available: TARGETS,
        };

        assert_eq!(
            refusal.to_string(),
            "fcstd cannot inherit a write target: the fcstd source records no dialect; name an explicit target; available targets: fcstd:schema-4"
        );
    }
}
