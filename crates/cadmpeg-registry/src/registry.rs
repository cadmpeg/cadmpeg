// SPDX-License-Identifier: Apache-2.0
//! Embedded dialect-registry parsing and the total identity/support join.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::TargetCatalog;
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
}

/// One `[[dialect]]` row of the identity registry.
///
/// Only the fields this view renders are read. The checkers own the rest. The
/// id deserializes straight into [`DialectId`], which is the one construction
/// path open to a producer that did not pin the string itself.
#[derive(Debug, Clone, Deserialize)]
struct IdentityRow {
    id: DialectId,
    title: String,
}

#[derive(Debug, Clone, Deserialize)]
struct FormatIdentity {
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Identity {
    #[serde(default)]
    format: BTreeMap<String, FormatIdentity>,
    #[serde(default)]
    dialect: Vec<IdentityRow>,
}

/// The identity-only half needed to resolve format words.
struct IdentityRegistry {
    identity: Identity,
    format_names: BTreeMap<String, String>,
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
    #[error("format word {name:?} belongs to both {first:?} and {second:?}")]
    DuplicateFormatName {
        name: String,
        first: String,
        second: String,
    },
}

impl IdentityRegistry {
    fn load() -> Result<Self, RegistryLoadError> {
        Self::load_from(IDENTITY_TOML)
    }

    fn load_from(identity_toml: &str) -> Result<Self, RegistryLoadError> {
        let identity: Identity =
            toml::from_str(identity_toml).map_err(RegistryLoadError::Identity)?;
        let mut format_names = BTreeMap::new();
        for (id, row) in &identity.format {
            for name in std::iter::once(id).chain(&row.aliases) {
                if let Some(first) = format_names.insert(name.clone(), id.clone()) {
                    return Err(RegistryLoadError::DuplicateFormatName {
                        name: name.clone(),
                        first,
                        second: id.clone(),
                    });
                }
            }
        }
        Ok(Self {
            identity,
            format_names,
        })
    }
}

impl Registries {
    /// Parses both embedded registries and joins them.
    ///
    /// A parse failure means the binary shipped a malformed registry, which
    /// `scripts/check-dialects.py` and `scripts/check-dialect-support.py`
    /// forbid. Runtime loading validates only the parse and total join needed
    /// by this view; the Python checkers own fields that the view does not read.
    fn load() -> Result<Self, RegistryLoadError> {
        Self::join(identity_registry().identity.clone(), SUPPORT_TOML)
    }

    #[cfg(test)]
    fn load_from(identity_toml: &str, support_toml: &str) -> Result<Self, RegistryLoadError> {
        let identity = IdentityRegistry::load_from(identity_toml)?.identity;
        Self::join(identity, support_toml)
    }

    fn join(identity: Identity, support_toml: &str) -> Result<Self, RegistryLoadError> {
        let support: Support = toml::from_str(support_toml).map_err(RegistryLoadError::Support)?;
        let identity_ids = identity
            .dialect
            .iter()
            .map(|row| row.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
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
                        id: row.id,
                        title: row.title,
                    })
                })
                .collect::<Result<_, RegistryLoadError>>()?,
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

fn identity_registry() -> &'static IdentityRegistry {
    static IDENTITIES: OnceLock<IdentityRegistry> = OnceLock::new();
    IDENTITIES.get_or_init(|| {
        IdentityRegistry::load().expect(
            "embedded dialect identity registry invariant failed: the registry must parse and every format word must have one owner",
        )
    })
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

pub(crate) fn catalog_of(format: &str) -> Option<TargetCatalog> {
    Format::from_name(format).map(|format| build_encoder(format).targets())
}

pub(crate) fn is_format_name(name: &str) -> bool {
    identity_registry().format_names.contains_key(name)
}

pub(crate) fn canonical_format_name(name: &str) -> Option<&'static str> {
    identity_registry()
        .format_names
        .get(name)
        .map(String::as_str)
}

/// Canonical name and identity aliases for one format.
pub(crate) fn format_words(format: &str) -> impl Iterator<Item = &'static str> {
    identity_registry()
        .identity
        .format
        .get_key_value(format)
        .into_iter()
        .flat_map(|(id, row)| {
            std::iter::once(id.as_str()).chain(row.aliases.iter().map(String::as_str))
        })
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

    fn expect_err<T, E>(result: Result<T, E>, message: &str) -> E {
        match result {
            Ok(_) => panic!("{message}"),
            Err(error) => error,
        }
    }

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
        let error = expect_err(
            Registries::load_from(
                "[format.step]\n[[dialect]]\nid = \"step:ap203\"\ntitle = \"AP203\"\n",
                "[[support]]\ndialect = \"step:ap214\"\nread = \"L1\"\nwrite = \"none\"\n",
            ),
            "the unmatched support row is rejected",
        );
        assert!(
            matches!(error, RegistryLoadError::SupportWithoutIdentity(id) if id == "step:ap214")
        );
    }

    #[test]
    fn the_registry_join_rejects_an_identity_row_without_support() {
        let error = expect_err(
            Registries::load_from(
                "[format.step]\n[[dialect]]\nid = \"step:ap203\"\ntitle = \"AP203\"\n",
                "",
            ),
            "the unmatched identity row is rejected",
        );
        assert!(
            matches!(error, RegistryLoadError::IdentityWithoutSupport(id) if id.as_str() == "step:ap203")
        );
    }

    #[test]
    fn the_registry_join_rejects_duplicate_support_rows() {
        let error = expect_err(
            Registries::load_from(
            "[format.step]\n[[dialect]]\nid = \"step:ap203\"\ntitle = \"AP203\"\n",
            "[[support]]\ndialect = \"step:ap203\"\nread = \"L1\"\nwrite = \"none\"\n\n[[support]]\ndialect = \"step:ap203\"\nread = \"L2\"\nwrite = \"none\"\n",
            ),
            "the duplicate support row is rejected",
        );
        assert!(matches!(error, RegistryLoadError::DuplicateSupport(id) if id == "step:ap203"));
    }

    #[test]
    fn the_identity_registry_rejects_a_format_word_with_two_owners() {
        let error = expect_err(
            IdentityRegistry::load_from(
                "[format.step]\naliases = [\"cad\"]\n[format.iges]\naliases = [\"cad\"]\n",
            ),
            "a format word must have one owner",
        );
        assert!(
            matches!(error, RegistryLoadError::DuplicateFormatName { name, .. } if name == "cad")
        );
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
                for token in target.accepted_tokens() {
                    assert!(
                        !is_format_name(token),
                        "{}: accepted target token {token:?} is also an output-format word",
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
