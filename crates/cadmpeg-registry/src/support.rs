// SPDX-License-Identifier: Apache-2.0
//! The dialect registries, joined and rendered.
//!
//! `docs/dialects.toml` says which dialects exist; `docs/dialect-support.toml`
//! says what cadmpeg does with each. Both are embedded with `include_str!` and
//! parsed on first use, so this module has no table of its own to fall out of
//! date: a row added to a TOML file appears in `cadmpeg dialects` on the next
//! build, and a row deleted from one disappears. The third source is the
//! compiled `Encoder::targets()` catalogs, which are the only thing that can
//! answer "what can *this* build write", because a codec the build left out
//! has no catalog to report.
//!
//! Embedding rather than reading from disk: a shipped binary has no repository
//! beside it, and a registry the user could edit under the tool would make
//! `cadmpeg dialects` disagree with what the encoders do. [`dialects`] and
//! [`support`] therefore answer at build time and perform no I/O.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use cadmpeg_core::dialect::{DialectId, DialectLayers};
use cadmpeg_ir::codec::{find_target, TargetDescriptor};
use serde::Deserialize;

use crate::{build_encoder, Format, InputCatalog};

/// The identity registry, embedded.
const IDENTITY_TOML: &str = include_str!("../../../docs/dialects.toml");
/// The capability registry, embedded.
const SUPPORT_TOML: &str = include_str!("../../../docs/dialect-support.toml");

/// A capability-registry cell whose word is outside its vocabulary.
#[derive(Debug, thiserror::Error)]
#[error("{word:?} is not a {column} disposition; expected one of: {expected}")]
pub struct UnknownDisposition {
    /// The registry column the word came from, `read` or `write`.
    column: &'static str,
    /// The word the registry carried.
    word: String,
    /// The column's vocabulary.
    expected: &'static str,
}

/// What cadmpeg does when it reads a dialect.
///
/// The `read` column of `docs/dialect-support.toml`, verbatim. The column is
/// three refusal-and-recovery states plus one ladder score, and they are not
/// points on one scale: `detected` is "recognized, with no fixture witnessing
/// a decode", which is a statement about evidence, while `refused` is a
/// statement about the codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum ReadDisposition {
    /// A `docs/format-support.md` ladder score, `L0` through `L9`.
    Level(u8),
    /// Classified, with no fixture witnessing a decode. The floor for an
    /// unwitnessed dialect.
    Detected,
    /// The codec refuses the file, by `Admission::Refused` or by a
    /// `CodecError` raised before any report exists.
    Refused,
    /// Parsed with a strategy some other row declares, which is
    /// `Admission::AdmittedUnverified`.
    UnclassifiedRecovered,
}

impl ReadDisposition {
    /// The vocabulary, for a refusal message.
    const VOCABULARY: &'static str = "L0..L9, detected, refused, unclassified-recovered";
}

impl fmt::Display for ReadDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Level(level) => write!(f, "L{level}"),
            Self::Detected => f.write_str("detected"),
            Self::Refused => f.write_str("refused"),
            Self::UnclassifiedRecovered => f.write_str("unclassified-recovered"),
        }
    }
}

impl TryFrom<String> for ReadDisposition {
    type Error = UnknownDisposition;

    fn try_from(word: String) -> Result<Self, Self::Error> {
        match word.as_str() {
            "detected" => return Ok(Self::Detected),
            "refused" => return Ok(Self::Refused),
            "unclassified-recovered" => return Ok(Self::UnclassifiedRecovered),
            _ => {}
        }
        if let Some(level) = word
            .strip_prefix('L')
            .and_then(|rest| rest.parse::<u8>().ok())
            .filter(|level| *level <= 9)
        {
            return Ok(Self::Level(level));
        }
        Err(UnknownDisposition {
            column: "read",
            word,
            expected: Self::VOCABULARY,
        })
    }
}

/// What cadmpeg does when it writes a dialect.
///
/// The `write` column of `docs/dialect-support.toml`, verbatim. Synthesis and
/// preservation are different capabilities and the column never conflates
/// them: `verified` and `emitted` grade a `TargetDescriptor` this build can
/// synthesize, `preserved` records that a same-dialect re-export replays a
/// retained baseline instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum WriteDisposition {
    /// Synthesized, with checked-in golden artifacts pinning the bytes.
    Verified,
    /// Synthesized, with no golden pinning the bytes.
    Emitted,
    /// Not synthesized. A same-dialect re-export replays the retained source.
    Preserved,
    /// Not written at all.
    None,
}

impl WriteDisposition {
    /// The vocabulary, for a refusal message.
    const VOCABULARY: &'static str = "verified, emitted, preserved, none";
}

impl fmt::Display for WriteDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Verified => "verified",
            Self::Emitted => "emitted",
            Self::Preserved => "preserved",
            Self::None => "none",
        })
    }
}

impl TryFrom<String> for WriteDisposition {
    type Error = UnknownDisposition;

    fn try_from(word: String) -> Result<Self, Self::Error> {
        match word.as_str() {
            "verified" => Ok(Self::Verified),
            "emitted" => Ok(Self::Emitted),
            "preserved" => Ok(Self::Preserved),
            "none" => Ok(Self::None),
            _ => Err(UnknownDisposition {
                column: "write",
                word,
                expected: Self::VOCABULARY,
            }),
        }
    }
}

/// What cadmpeg declares it does with one dialect, read and write.
///
/// Declared, not observed: this is the static fact a file-open dialog needs
/// before it opens anything. What a particular run did is
/// `cadmpeg_core::dialect::Admission`, and no preflight can report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disposition {
    /// The `read` cell.
    pub read: ReadDisposition,
    /// The `write` cell.
    pub write: WriteDisposition,
}

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
struct Registries {
    /// Format ids the identity registry declares, in alphabetical order.
    formats: Vec<String>,
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
                            .and_then(|(format, _)| catalog_of(format))
                            .and_then(|targets| find_target(targets, row.id.as_str()))
                            .is_some(),
                        id: row.id,
                        title: row.title,
                        unknown_kind: row.unknown_kind,
                    })
                })
                .collect::<Result<_, RegistryLoadError>>()?,
        })
    }

    /// The joined rows of one format, in registry order.
    fn rows_of<'a>(&'a self, format: &str) -> impl Iterator<Item = &'a DialectEntry> {
        let prefix = format!("{format}:");
        self.rows_all()
            .filter(move |entry| entry.id.as_str().starts_with(&prefix))
    }

    fn rows_all(&self) -> impl Iterator<Item = &DialectEntry> {
        self.entries.iter()
    }
}

/// The parsed registries, parsed once.
fn registries() -> &'static Registries {
    static REGISTRIES: OnceLock<Registries> = OnceLock::new();
    REGISTRIES.get_or_init(|| {
        Registries::load().expect(
            "embedded dialect registry invariant failed: registries must parse, support rows must be unique, and the identity/support join must be total",
        )
    })
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

/// The synthesis catalog of `format`'s encoder in this build, or `None` when
/// this build carries no encoder for it.
fn catalog_of(format: &str) -> Option<&'static [TargetDescriptor]> {
    Format::from_name(format).map(|format| build_encoder(format).targets())
}

/// What `cadmpeg inspect` knows about the dialect it matched.
///
/// Three sources in one value: the classifier's own primary-layer match, the
/// read disposition the capability registry records for that id, and the write
/// targets this build's encoder for that format can synthesize. How they are
/// rendered belongs to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectProvenance {
    /// The matched id.
    pub id: DialectId,
    /// The declared read disposition for that id, when the registry has one.
    pub read: Option<ReadDisposition>,
    /// The target ids this build can synthesize for the format, in catalog
    /// order. Empty when the build has no encoder for it, or the encoder has
    /// no catalog.
    pub write_targets: Vec<&'static str>,
}

/// The provenance of the primary dialect the codec matched.
///
/// Returns `None` when the codec reported no dialects at all, which is the
/// honest answer for a codec that does not classify.
#[must_use]
pub fn dialect_provenance(dialects: Option<&DialectLayers>) -> Option<DialectProvenance> {
    let entry = dialects?.primary();
    Some(DialectProvenance {
        id: entry.dialect().clone(),
        read: support(entry.dialect()).map(|disposition| disposition.read),
        write_targets: catalog_of(entry.format())
            .unwrap_or(&[])
            .iter()
            .map(|target| target.id)
            .collect(),
    })
}

/// One row of the format table: what this build does with one readable format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatRow {
    /// The format id.
    pub id: &'static str,
    /// Whether this build writes it.
    pub write: bool,
    /// The extensions the detector accepts for it.
    pub extensions: &'static [&'static str],
}

/// The readable formats in this build, with their write capability, in catalog
/// order.
#[must_use]
pub fn format_rows(inputs: &InputCatalog) -> Vec<FormatRow> {
    inputs
        .descriptors()
        .map(|descriptor| {
            let id = descriptor.format_id();
            FormatRow {
                // Every input descriptor is readable. CADIR carries no codec
                // because the neutral document is parsed, not decoded.
                id,
                write: Format::from_name(id).is_some(),
                extensions: descriptor.extensions,
            }
        })
        .collect()
}

/// A `format` argument that names no section of the identity registry.
#[derive(Debug, thiserror::Error)]
#[error("no format {name} in the dialect registry; known: {known}")]
pub struct UnknownFormat {
    /// The word the caller passed.
    name: String,
    /// The format ids the registry declares.
    known: String,
}

/// Every declared dialect of one format, with this build's write catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatDialects {
    /// The format id.
    pub format: String,
    /// `None` when this build has no encoder for the format; otherwise the
    /// encoder's catalog, which is empty for an encoder with no dialects.
    pub catalog: Option<&'static [TargetDescriptor]>,
    /// The catalog's default target id, when it declares one.
    pub default_target: Option<&'static str>,
    /// The declared dialects, in registry order.
    pub rows: Vec<DialectEntry>,
}

/// The identity registry crossed with the capability registry.
///
/// `format` selects one section; `None` returns every one. The word is
/// resolved through [`Format::from_name`] first, so an output-format spelling
/// and a registry section name reach the same rows.
pub fn dialect_table(format: Option<&str>) -> Result<Vec<FormatDialects>, UnknownFormat> {
    let registries = registries();
    let formats = match format {
        None => registries.formats.clone(),
        Some(name) => {
            let name =
                Format::from_name(name).map_or_else(|| name.to_owned(), |f| f.name().to_owned());
            if !registries.formats.contains(&name) {
                return Err(UnknownFormat {
                    name,
                    known: registries.formats.join(", "),
                });
            }
            vec![name]
        }
    };

    Ok(formats
        .into_iter()
        .map(|name| {
            let catalog = catalog_of(&name);
            FormatDialects {
                catalog,
                default_target: catalog
                    .and_then(|targets| targets.iter().find(|target| target.default))
                    .map(|target| target.id),
                rows: registries.rows_of(&name).cloned().collect(),
                format: name,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden reports are outputs of the compiled decoders. Join every
    /// observed admission family to the registry read cell so the static table
    /// cannot contradict decoder behavior.
    #[test]
    fn compiled_read_admissions_match_registry_policy() {
        let expected = registries()
            .rows_all()
            .map(|row| (row.id.as_str().to_owned(), row.disposition.read))
            .collect::<BTreeMap<_, _>>();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut snapshots = Vec::new();
        collect_json_files(&root.join("crates"), &mut snapshots);
        let mut observed = BTreeMap::<String, BTreeMap<&'static str, usize>>::new();
        for path in snapshots {
            if !path.to_string_lossy().contains("/tests/golden/") {
                continue;
            }
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
            collect_admissions(&value, &mut observed);
        }

        assert!(!observed.is_empty(), "no golden decoder admissions found");
        for (dialect, families) in observed {
            let read = expected
                .get(&dialect)
                .unwrap_or_else(|| panic!("{dialect}: decoder emitted no registry row"));
            for family in families.keys() {
                let compatible = match *family {
                    "admitted" => {
                        matches!(read, ReadDisposition::Level(_) | ReadDisposition::Detected)
                    }
                    "admitted_unverified" => {
                        matches!(read, ReadDisposition::UnclassifiedRecovered)
                    }
                    "refused" => matches!(read, ReadDisposition::Refused),
                    other => panic!("{dialect}: unknown admission family {other}"),
                };
                assert!(
                    compatible,
                    "{dialect}: decoder admission {family} contradicts registry read {read}"
                );
            }
        }
    }

    fn collect_json_files(directory: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("read directory entry").path();
            if path.is_dir() {
                collect_json_files(&path, files);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                files.push(path);
            }
        }
    }

    fn collect_admissions(
        value: &serde_json::Value,
        observed: &mut BTreeMap<String, BTreeMap<&'static str, usize>>,
    ) {
        match value {
            serde_json::Value::Object(object) => {
                if let (Some(dialect), Some(admission)) = (
                    object.get("dialect").and_then(serde_json::Value::as_str),
                    object.get("admission"),
                ) {
                    let family = if admission.as_str() == Some("admitted") {
                        "admitted"
                    } else if admission.get("admitted_unverified").is_some() {
                        "admitted_unverified"
                    } else if admission.as_str() == Some("refused") {
                        "refused"
                    } else {
                        panic!("{dialect}: unrecognized admission value {admission}");
                    };
                    *observed
                        .entry(dialect.to_owned())
                        .or_default()
                        .entry(family)
                        .or_default() += 1;
                }
                for child in object.values() {
                    collect_admissions(child, observed);
                }
            }
            serde_json::Value::Array(array) => {
                for child in array {
                    collect_admissions(child, observed);
                }
            }
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_) => {}
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

        for format in Format::ALL {
            let targets = build_encoder(*format).targets();
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

    /// The disposition words round-trip through their own vocabulary, and a
    /// word outside it is a refusal rather than a silent default.
    #[test]
    fn the_disposition_vocabulary_is_closed() {
        for word in ["L0", "L9", "detected", "refused", "unclassified-recovered"] {
            let read = ReadDisposition::try_from(word.to_owned()).expect("a declared word parses");
            assert_eq!(read.to_string(), word);
        }
        for word in ["verified", "emitted", "preserved", "none"] {
            let write =
                WriteDisposition::try_from(word.to_owned()).expect("a declared word parses");
            assert_eq!(write.to_string(), word);
        }
        assert!(ReadDisposition::try_from("L10".to_owned()).is_err());
        assert!(ReadDisposition::try_from("L".to_owned()).is_err());
        assert!(ReadDisposition::try_from("verified".to_owned()).is_err());
        assert!(WriteDisposition::try_from("L4".to_owned()).is_err());
    }

    /// The provenance joins the match, the registry, and the catalog. Its
    /// rendering is the CLI's; the three sources are this crate's.
    #[cfg(feature = "rhino")]
    #[test]
    fn the_provenance_joins_the_match_the_registry_and_the_catalog() {
        use cadmpeg_core::dialect::{Admission, DialectMatch};

        let dialects = DialectLayers::of(DialectMatch::new(
            DialectId::pinned("rhino:archive-50"),
            Admission::Admitted,
        ));
        let provenance = dialect_provenance(Some(&dialects)).expect("a primary layer exists");
        assert_eq!(provenance.id.as_str(), "rhino:archive-50");
        assert!(provenance.read.is_some());
        assert!(provenance.write_targets.contains(&"rhino:archive-50"));
        assert!(provenance.write_targets.contains(&"rhino:archive-80"));
    }

    /// A codec that classified nothing has no provenance to report.
    #[test]
    fn no_dialects_is_no_provenance() {
        assert!(dialect_provenance(None).is_none());
    }

    /// The dialect table refuses a word the identity registry does not carry
    /// and serves the same rows `dialects` does for one it carries.
    #[test]
    fn the_dialect_table_selects_one_format_or_every_one() {
        assert!(dialect_table(Some("nonesuch")).is_err());
        let all = dialect_table(None).expect("every declared format");
        assert!(all.len() > 1);
        let one = dialect_table(Some(&all[0].format)).expect("a declared format");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].rows.len(), all[0].rows.len());
    }

    #[test]
    fn format_aliases_reach_the_same_dialect_rows() {
        let canonical = dialects("rhino")
            .into_iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>();
        let alias = dialects("3dm")
            .into_iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(alias, canonical);
        assert_eq!(
            dialect_table(Some("3dm")).expect("Rhino alias"),
            dialect_table(Some("rhino")).expect("Rhino format")
        );
    }
}
