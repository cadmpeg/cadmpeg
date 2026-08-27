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

use cadmpeg_core::dialect::{primary_layer, DialectId, DialectMatch};
use cadmpeg_ir::codec::{find_target, TargetDescriptor};
use serde::Deserialize;

use crate::{build_encoder, Format, InputCatalog, LossPolicy};

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

/// One dialect, as the two registries jointly describe it.
#[derive(Debug, Clone)]
pub struct DialectEntry {
    /// The registry id, `<format>:<name>`.
    pub id: DialectId,
    /// The human-facing name.
    pub title: String,
    /// The capability row for this id.
    ///
    /// `Option` because the join is a registry invariant rather than a type
    /// invariant: `the_embedded_registries_parse_and_join` and
    /// `scripts/check-dialect-support.py` prove it total, and this field is
    /// what a break in it would look like.
    pub disposition: Option<Disposition>,
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

impl Registries {
    /// Parses both embedded registries and joins them.
    ///
    /// A parse failure means the binary shipped a malformed registry, which
    /// `scripts/check-dialects.py` and `scripts/check-dialect-support.py`
    /// forbid; `tests::the_embedded_registries_parse_and_join` is the in-tree
    /// guard.
    fn load() -> Result<Self, toml::de::Error> {
        let identity: Identity = toml::from_str(IDENTITY_TOML)?;
        let support: Support = toml::from_str(SUPPORT_TOML)?;
        let dispositions = support
            .support
            .into_iter()
            .map(|row| {
                (
                    row.dialect,
                    Disposition {
                        read: row.read,
                        write: row.write,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        Ok(Self {
            formats: identity.format.keys().cloned().collect(),
            entries: identity
                .dialect
                .into_iter()
                .map(|row| DialectEntry {
                    disposition: dispositions.get(row.id.as_str()).copied(),
                    id: row.id,
                    title: row.title,
                })
                .collect(),
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
    REGISTRIES.get_or_init(|| Registries::load().expect("the embedded dialect registries parse"))
}

/// Every dialect the identity registry declares for `format`, in registry
/// order, joined with its capability row.
///
/// Empty when `format` names no registry section. Answers from the embedded
/// tables and reads no file.
#[must_use]
pub fn dialects(format: &str) -> Vec<&'static DialectEntry> {
    registries().rows_of(format).collect()
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
        .and_then(|entry| entry.disposition)
}

/// The synthesis catalog of `format`'s encoder in this build, or `None` when
/// this build carries no encoder for it.
fn catalog_of(format: &str) -> Option<&'static [TargetDescriptor]> {
    Some(build_encoder(Format::from_name(format)?, LossPolicy::Report).targets())
}

/// The `dialect:` line `cadmpeg inspect` prints under `format:`.
///
/// Three sources in one sentence: the classifier's own primary-layer match,
/// the read disposition the capability registry records for that id, and the
/// write targets this build's encoder for that format can synthesize. Returns
/// `None` when the codec reported no dialects at all, which is the honest
/// output for a codec that does not classify.
#[must_use]
pub fn dialect_provenance(dialects: &[DialectMatch], format: &str) -> Option<String> {
    let entry = primary_layer(dialects, format)?;
    let id = entry
        .dialect
        .as_ref()
        .map_or_else(|| "<unmatched>".to_owned(), |id| id.as_str().to_owned());

    let mut clauses = Vec::new();
    if let Some(read) = entry
        .dialect
        .as_ref()
        .and_then(support)
        .map(|disposition| disposition.read)
    {
        clauses.push(format!("read {read}"));
    }
    if let Some(catalog) = catalog_of(format) {
        let targets = catalog
            .iter()
            .map(|target| suffix(target.id))
            .collect::<Vec<_>>();
        if !targets.is_empty() {
            clauses.push(format!("write targets {}", targets.join(", ")));
        }
    }
    Some(if clauses.is_empty() {
        format!("dialect: {id}")
    } else {
        format!("dialect: {id} — {}", clauses.join(", "))
    })
}

/// The part of a dialect id after its format prefix.
fn suffix(id: &str) -> &str {
    id.split_once(':').map_or(id, |(_, rest)| rest)
}

/// Prints the formats this build reads and writes.
///
/// Two columns, because reading and writing differ per format: Inventor,
/// CATIA, Creo, NX, and SAT are read-only, and one column would have to
/// misstate one half of each of them.
pub fn print_formats(inputs: &InputCatalog) {
    println!("FORMAT     READ   WRITE  EXTENSIONS");
    for descriptor in inputs.descriptors() {
        let id = descriptor.format_id();
        // Every input descriptor is readable. CADIR carries no codec because
        // the neutral document is parsed, not decoded.
        println!(
            "{id:<10} {:<6} {:<6} {}",
            "yes",
            yes_no(Format::from_name(id).is_some()),
            descriptor.extensions.join(", ")
        );
    }
    println!();
    println!("`cadmpeg dialects [FORMAT]` lists the dialects of each format.");
}

const fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
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

/// Prints the identity registry crossed with the capability registry.
pub fn print_dialects(format: Option<&str>) -> Result<(), UnknownFormat> {
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

    for (index, name) in formats.iter().enumerate() {
        if index > 0 {
            println!();
        }
        let catalog = catalog_of(name);
        match catalog {
            Some(targets) if !targets.is_empty() => {
                let default = targets
                    .iter()
                    .find(|target| target.default)
                    .map_or("none", |target| target.id);
                println!("{name}  (write targets in this build; default {default})");
            }
            Some(_) => println!("{name}  (this build writes it, with no dialect catalog)"),
            None => println!("{name}  (no encoder in this build)"),
        }
        println!("  DIALECT                            READ                     WRITE                    TITLE");
        for row in registries.rows_of(name) {
            let target = catalog
                .and_then(|targets| find_target(targets, row.id.as_str()))
                .is_some();
            println!(
                "  {:<34} {:<24} {:<24} {}",
                row.id.as_str(),
                row.disposition
                    .map_or_else(|| "-".to_owned(), |value| value.read.to_string()),
                match (row.disposition.map(|value| value.write), target) {
                    (Some(write), true) => format!("{write} (target)"),
                    (Some(write), false) => write.to_string(),
                    (None, _) => "-".to_owned(),
                },
                row.title
            );
        }
    }
    Ok(())
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
        for row in &registries.entries {
            assert!(
                row.disposition.is_some(),
                "{}: identity row with no capability row",
                row.id
            );
        }
    }

    /// Every compiled write target is a declared identity row.
    ///
    /// The `(target)` column would otherwise be able to mark nothing, or to
    /// mark a row the registry does not carry.
    #[test]
    fn every_write_target_is_a_registry_row() {
        let ids = registries()
            .rows_all()
            .map(|row| row.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for format in Format::ALL {
            let encoder = build_encoder(*format, LossPolicy::Report);
            for target in encoder.targets() {
                assert!(ids.contains(target.id), "{}: not a registry row", target.id);
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

    /// The provenance line names the id, the read disposition, and the
    /// catalog. It is what `cadmpeg inspect` prints, so a change to any of the
    /// three sources shows up here.
    #[cfg(feature = "rhino")]
    #[test]
    fn the_provenance_line_joins_the_match_the_registry_and_the_catalog() {
        use cadmpeg_core::dialect::Admission;

        let dialects = vec![DialectMatch {
            format: "rhino".to_owned(),
            dialect: Some(DialectId::pinned("rhino:archive-50")),
            declared: BTreeMap::new(),
            admission: Admission::Admitted,
        }];
        let line = dialect_provenance(&dialects, "rhino").expect("a primary layer exists");
        assert!(line.starts_with("dialect: rhino:archive-50 — "), "{line}");
        assert!(line.contains("read "), "{line}");
        assert!(line.contains("write targets archive-50"), "{line}");
        assert!(line.contains("archive-80"), "{line}");
    }

    /// A codec that classified nothing prints no dialect line.
    #[test]
    fn no_dialects_is_no_line() {
        assert!(dialect_provenance(&[], "rhino").is_none());
    }
}
