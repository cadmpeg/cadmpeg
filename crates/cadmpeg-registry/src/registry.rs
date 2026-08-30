// SPDX-License-Identifier: Apache-2.0
//! Embedded dialect-registry parsing and the total identity/support join.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use cadmpeg_core::dialect::DialectId;
use cadmpeg_ir::codec::{find_target, TargetDescriptor};
use serde::Deserialize;

use crate::disposition::{Disposition, ReadDisposition, WriteDisposition};
use crate::{build_encoder, Format};

const IDENTITY_TOML: &str = include_str!("../../../docs/dialects.toml");
const SUPPORT_TOML: &str = include_str!("../../../docs/dialect-support.toml");

/// Why an identity registry row uses the reserved `unknown` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownDialectKind {
    /// Detection cannot obtain enough format evidence to produce a report.
    DetectUnreachable,
    /// Classification read the evidence and no declared dialect row matched it.
    RecoveredResidual,
}

/// One dialect, as the two registries jointly describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectEntry {
    /// The registry id, `<format>:<name>`.
    pub id: DialectId,
    /// The human-facing name.
    pub title: String,
    /// The capability row for this id.
    pub disposition: Disposition,
    /// Whether this build's encoder carries the dialect as a target.
    pub target: bool,
    /// The meaning of an `unknown` row; `None` for every named dialect row.
    pub unknown_kind: Option<UnknownDialectKind>,
}

/// One `[[dialect]]` row of the identity registry.
///
/// Only the fields this view renders are read. The checkers own the rest. The
/// id deserializes straight into [`DialectId`], which is the one construction
/// path open to a producer that did not pin the string itself.
#[derive(Debug, Deserialize)]
struct IdentityRow {
    id: DialectId,
    title: String,
    #[serde(default)]
    unknown_kind: Option<UnknownDialectKind>,
}

#[derive(Debug, Deserialize)]
struct Identity {
    #[serde(default)]
    format: BTreeMap<String, toml::Value>,
    #[serde(default)]
    dialect: Vec<IdentityRow>,
}

/// One `[[support]]` row of the capability registry.
#[derive(Debug, Deserialize)]
struct SupportRow {
    dialect: String,
    read: ReadDisposition,
    write: WriteDisposition,
}

#[derive(Debug, Deserialize)]
struct Support {
    #[serde(default)]
    support: Vec<SupportRow>,
}

/// The two registries, joined by dialect id.
pub(crate) struct Registries {
    /// Format ids the identity registry declares, in alphabetical order.
    pub(crate) formats: Vec<String>,
    /// Joined rows in identity-registry order.
    entries: Vec<DialectEntry>,
    /// Encoder catalogs constructed once per writable format.
    catalogs: BTreeMap<&'static str, &'static [TargetDescriptor]>,
}

#[derive(Debug, thiserror::Error)]
enum RegistryLoadError {
    #[error("cannot parse the dialect identity registry: {0}")]
    Identity(#[source] toml::de::Error),
    #[error("cannot parse the dialect support registry: {0}")]
    Support(#[source] toml::de::Error),
    #[error("duplicate support row for dialect {0}")]
    DuplicateSupport(String),
    #[error("support row for dialect {0} has no identity row")]
    SupportWithoutIdentity(String),
    #[error("identity row for dialect {0} has no support row")]
    IdentityWithoutSupport(DialectId),
}

impl Registries {
    /// Parses both embedded registries and joins them.
    ///
    /// A parse failure means the binary shipped a malformed registry, which
    /// `scripts/check-dialects.py` and `scripts/check-dialect-support.py`
    /// forbid; `tests::the_embedded_registries_parse_and_join` is the in-tree
    /// guard.
    fn load() -> Result<Self, RegistryLoadError> {
        Self::load_from(IDENTITY_TOML, SUPPORT_TOML)
    }

    fn load_from(identity_toml: &str, support_toml: &str) -> Result<Self, RegistryLoadError> {
        let identity: Identity =
            toml::from_str(identity_toml).map_err(RegistryLoadError::Identity)?;
        let support: Support = toml::from_str(support_toml).map_err(RegistryLoadError::Support)?;
        let identity_ids = identity
            .dialect
            .iter()
            .map(|row| row.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let catalogs = Format::all()
            .map(|format| (format.name(), build_encoder(format).targets()))
            .collect::<BTreeMap<_, _>>();
        let mut dispositions = BTreeMap::new();
        for row in support.support {
            let dialect = row.dialect;
            let disposition = Disposition {
                read: row.read,
                write: row.write,
            };
            if dispositions.insert(dialect.clone(), disposition).is_some() {
                return Err(RegistryLoadError::DuplicateSupport(dialect));
            }
            if !identity_ids.contains(dialect.as_str()) {
                return Err(RegistryLoadError::SupportWithoutIdentity(dialect));
            }
        }
        Ok(Self {
            formats: identity.format.keys().cloned().collect(),
            entries: identity
                .dialect
                .into_iter()
                .map(|row| {
                    let disposition = dispositions
                        .remove(row.id.as_str())
                        .ok_or_else(|| RegistryLoadError::IdentityWithoutSupport(row.id.clone()))?;
                    Ok(DialectEntry {
                        disposition,
                        target: row
                            .id
                            .as_str()
                            .split_once(':')
                            .and_then(|(format, _)| catalogs.get(format).copied())
                            .and_then(|targets| find_target(targets, row.id.as_str()))
                            .is_some(),
                        id: row.id,
                        title: row.title,
                        unknown_kind: row.unknown_kind,
                    })
                })
                .collect::<Result<_, RegistryLoadError>>()?,
            catalogs,
        })
    }

    /// The joined rows of one format, in registry order.
    pub(crate) fn rows_of<'a>(&'a self, format: &str) -> impl Iterator<Item = &'a DialectEntry> {
        let prefix = format!("{format}:");
        self.rows_all()
            .filter(move |entry| entry.id.as_str().starts_with(&prefix))
    }

    pub(crate) fn rows_all(&self) -> impl Iterator<Item = &DialectEntry> {
        self.entries.iter()
    }
}

/// The parsed registries, parsed once.
pub(crate) fn registries() -> &'static Registries {
    static REGISTRIES: OnceLock<Registries> = OnceLock::new();
    REGISTRIES.get_or_init(|| {
        Registries::load().expect(
            "embedded dialect registry invariant failed: registries must parse, support rows must be unique, and the identity/support join must be total",
        )
    })
}

pub(crate) fn catalog_of(format: &str) -> Option<&'static [TargetDescriptor]> {
    registries().catalogs.get(format).copied()
}

/// Every dialect the identity registry declares for `format`, in registry
/// order, joined with its capability row.
///
/// Empty when `format` names no registry section. Answers from the embedded
/// tables and reads no file.
#[must_use]
pub fn dialects(format: &str) -> Vec<&'static DialectEntry> {
    match Format::from_name(format) {
        Some(format) => registries().rows_of(format.name()).collect(),
        None => registries().rows_of(format).collect(),
    }
}

/// The declared disposition for one dialect id, or `None` when the registry
/// carries no row for it.
///
/// Answers from the embedded tables and reads no file.
#[must_use]
pub fn support(dialect: &DialectId) -> Option<Disposition> {
    registries()
        .rows_all()
        .find(|entry| entry.id == *dialect)
        .map(|entry| entry.disposition)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded registries parse, and the join is total in the direction
    /// this view depends on: every identity row has a capability row.
    ///
    /// The checkers prove the same thing against the working tree. This proves
    /// it against the bytes the binary actually carries, which is the pair a
    /// user sees.
    #[test]
    fn the_embedded_registries_parse_and_join() {
        let registries = Registries::load().expect("the embedded registries parse");
        assert!(!registries.formats.is_empty());
        assert!(!registries.entries.is_empty());
    }

    #[test]
    fn unknown_rows_report_whether_detection_or_recovery_owns_the_residual() {
        let registries = Registries::load().expect("the embedded registries parse");
        let kinds = registries
            .rows_all()
            .filter_map(|row| row.unknown_kind.map(|kind| (row.id.as_str(), kind)))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            kinds.get("nx:unknown"),
            Some(&UnknownDialectKind::DetectUnreachable)
        );
        assert_eq!(
            kinds.get("rhino:unknown"),
            Some(&UnknownDialectKind::RecoveredResidual)
        );
        assert_eq!(
            kinds.len(),
            13,
            "every unknown registry row states its kind"
        );
    }

    #[test]
    fn the_registry_join_rejects_a_support_row_without_an_identity() {
        let error = Registries::load_from(
            "[format.step]\n[[dialect]]\nid = \"step:ap203\"\ntitle = \"AP203\"\n",
            "[[support]]\ndialect = \"step:ap214\"\nread = \"L1\"\nwrite = \"none\"\n",
        )
        .err()
        .expect("the unmatched support row is rejected");
        assert!(
            matches!(error, RegistryLoadError::SupportWithoutIdentity(id) if id == "step:ap214")
        );
    }

    #[test]
    fn the_registry_join_rejects_an_identity_row_without_support() {
        let error = Registries::load_from(
            "[format.step]\n[[dialect]]\nid = \"step:ap203\"\ntitle = \"AP203\"\n",
            "",
        )
        .err()
        .expect("the unmatched identity row is rejected");
        assert!(
            matches!(error, RegistryLoadError::IdentityWithoutSupport(id) if id.as_str() == "step:ap203")
        );
    }

    #[test]
    fn the_registry_join_rejects_duplicate_support_rows() {
        let error = Registries::load_from(
            "[format.step]\n[[dialect]]\nid = \"step:ap203\"\ntitle = \"AP203\"\n",
            "[[support]]\ndialect = \"step:ap203\"\nread = \"L1\"\nwrite = \"none\"\n\n[[support]]\ndialect = \"step:ap203\"\nread = \"L2\"\nwrite = \"none\"\n",
        )
        .err()
        .expect("the duplicate support row is rejected");
        assert!(matches!(error, RegistryLoadError::DuplicateSupport(id) if id == "step:ap203"));
    }

    /// Compiled catalogs and write-policy rows describe the same synthesis
    /// surface, separate preservation from synthesis, and keep aliases
    /// disjoint from output-format words.
    #[test]
    fn compiled_write_catalogs_match_registry_policy() {
        let registries = registries();
        let dispositions = registries
            .rows_all()
            .map(|row| (row.id.as_str(), row.disposition))
            .collect::<BTreeMap<_, _>>();

        for format in Format::all() {
            let targets = build_encoder(format).targets();
            cadmpeg_ir::codec::assert_valid_target_catalog(targets);
            let prefix = format!("{}:", format.name());
            for target in targets {
                assert!(
                    target.id.starts_with(&prefix),
                    "{}: compiled target belongs to the {} catalog but does not use its registry prefix {prefix:?}",
                    target.id,
                    format.name()
                );
                let disposition = dispositions
                    .get(target.id)
                    .unwrap_or_else(|| panic!("{}: not a registry row", target.id));
                assert!(
                    matches!(
                        disposition.write,
                        WriteDisposition::Verified | WriteDisposition::Emitted
                    ),
                    "{}: a compiled target must be verified or emitted, not {}",
                    target.id,
                    disposition.write
                );
                for alias in target.aliases {
                    assert!(
                        !registries
                            .formats
                            .iter()
                            .any(|word| word.as_str() == *alias)
                            && Format::from_name(alias).is_none(),
                        "{}: alias {alias:?} is also an output-format word",
                        target.id
                    );
                }
            }

            if targets.is_empty() {
                continue;
            }
            for (id, disposition) in &dispositions {
                if id.starts_with(&prefix)
                    && matches!(
                        disposition.write,
                        WriteDisposition::Verified | WriteDisposition::Emitted
                    )
                {
                    assert!(
                        targets.iter().any(|target| target.id == *id),
                        "{id}: synthesis write is absent from the {} catalog",
                        format.name()
                    );
                }
            }
        }
    }

    /// `dialects` and `support` answer from the same joined table the renderer
    /// prints, and neither reads a file.
    #[cfg(feature = "rhino")]
    #[test]
    fn the_lookups_serve_the_embedded_tables() {
        let rows = dialects("rhino");
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| row.id.as_str().starts_with("rhino:")));

        let archive_50 = DialectId::pinned("rhino:archive-50");
        let disposition = support(&archive_50).expect("a declared dialect has a disposition");
        assert!(matches!(disposition.read, ReadDisposition::Level(_)));
        assert_eq!(disposition.write, WriteDisposition::Emitted);

        assert_eq!(
            support(&DialectId::pinned("rhino:unknown")),
            Some(Disposition {
                read: ReadDisposition::UnclassifiedRecovered,
                write: WriteDisposition::None,
            })
        );
        assert!(support(&DialectId::pinned("rhino:nonesuch")).is_none());
        assert!(dialects("nonesuch").is_empty());
    }
}
