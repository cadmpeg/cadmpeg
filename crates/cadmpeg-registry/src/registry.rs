// SPDX-License-Identifier: Apache-2.0
//! Embedded dialect-registry parsing and the total identity/support join.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::{find_target, TargetDescriptor};
use serde::Deserialize;

use crate::disposition::{Disposition, ReadDisposition, WriteDisposition};
use crate::{build_encoder, Format};

const IDENTITY_TOML: &str = include_str!("../../../docs/dialects.toml");
const SUPPORT_TOML: &str = include_str!("../../../docs/dialect-support.toml");

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
}

#[derive(Debug, Deserialize)]
struct FormatIdentity {
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Identity {
    #[serde(default)]
    format: BTreeMap<String, FormatIdentity>,
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
    /// Canonical format ids and aliases mapped to their canonical registry id.
    format_names: BTreeMap<String, String>,
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
    /// forbid. Runtime loading validates only the parse and total join needed
    /// by this view; the Python checkers own fields that the view does not read.
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
        let format_names = identity
            .format
            .iter()
            .flat_map(|(id, row)| {
                std::iter::once((id.clone(), id.clone()))
                    .chain(row.aliases.iter().cloned().map(|alias| (alias, id.clone())))
            })
            .collect();
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
            format_names,
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
                    })
                })
                .collect::<Result<_, RegistryLoadError>>()?,
            catalogs,
        })
    }

    /// The joined rows of one format, in registry order.
    pub(crate) fn rows_of<'a, 'format>(
        &'a self,
        format: &'format str,
    ) -> impl Iterator<Item = &'a DialectEntry> + use<'a, 'format> {
        self.rows_all()
            .filter(move |entry| entry.id.namespace() == format)
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

pub(crate) fn is_format_name(name: &str) -> bool {
    registries().format_names.contains_key(name)
}

pub(crate) fn canonical_format_name(name: &str) -> Option<&'static str> {
    registries().format_names.get(name).map(String::as_str)
}

/// Every dialect the identity registry declares for `format`, in registry
/// order, joined with its capability row.
///
/// Empty when `format` names no registry section. Answers from the embedded
/// tables and reads no file.
#[must_use]
pub fn dialects(format: &str) -> Vec<&'static DialectEntry> {
    let canonical = Format::from_name(format)
        .map(Format::name)
        .or_else(|| canonical_format_name(format))
        .unwrap_or(format);
    registries().rows_of(canonical).collect()
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
            cadmpeg_core::target::assert_valid_target_catalog(targets);
            let prefix = format!("{}:", format.name());
            for target in targets {
                assert!(
                    target.id.as_str().starts_with(&prefix),
                    "{}: compiled target belongs to the {} catalog but does not use its registry prefix {prefix:?}",
                    target.id,
                    format.name()
                );
                let disposition = dispositions
                    .get(target.id.as_str())
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
                        !is_format_name(alias),
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
                        targets.iter().any(|target| target.id.as_str() == *id),
                        "{id}: synthesis write is absent from the {} catalog",
                        format.name()
                    );
                }
            }
        }
    }

    /// `dialects` and `support` answer from the same joined table the renderer
    /// prints, and neither reads a file.
    #[test]
    fn the_lookups_serve_the_embedded_tables() {
        let registries = registries();
        for entry in registries.rows_all() {
            assert_eq!(support(&entry.id), Some(entry.disposition), "{}", entry.id);
        }
        for format in &registries.formats {
            let rows = dialects(format);
            let expected = registries.rows_of(format).collect::<Vec<_>>();
            assert_eq!(rows, expected, "{format}");
            assert!(
                rows.iter().all(|row| row.id.namespace() == format),
                "{format}"
            );
        }

        let absent = DialectId::parse("test:nonesuch").expect("the absent id is grammatical");
        assert!(support(&absent).is_none());
        assert!(dialects("nonesuch").is_empty());
    }
}
