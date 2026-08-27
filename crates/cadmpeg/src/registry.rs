// SPDX-License-Identifier: Apache-2.0
//! The dialect registries, rendered.
//!
//! `docs/dialects.toml` says which dialects exist; `docs/dialect-support.toml`
//! says what cadmpeg does with each. Both are embedded with `include_str!` and
//! parsed at run time, so this module has no table of its own to fall out of
//! date: a row added to a TOML file appears in `cadmpeg dialects` on the next
//! build, and a row deleted from one disappears. The third source is the
//! compiled `Encoder::targets()` catalogs, which are the only thing that can
//! answer "what can *this* build write", because a codec the build left out
//! has no catalog to report.
//!
//! Embedding rather than reading from disk: a shipped binary has no repository
//! beside it, and a registry the user could edit under the tool would make
//! `cadmpeg dialects` disagree with what the encoders do.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use cadmpeg_core::dialect::{primary_layer, DialectMatch};
use cadmpeg_ir::codec::{find_target, TargetDescriptor};
use serde::Deserialize;

use crate::application::{build_encoder, InputCatalog, LossPolicy};
use crate::Format;

/// The identity registry, embedded.
const IDENTITY_TOML: &str = include_str!("../../../docs/dialects.toml");
/// The capability registry, embedded.
const SUPPORT_TOML: &str = include_str!("../../../docs/dialect-support.toml");

/// One `[[dialect]]` row of the identity registry.
///
/// Only the fields this view renders are read. The checkers own the rest.
#[derive(Debug, Deserialize)]
struct IdentityRow {
    id: String,
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
    read: String,
    write: String,
}

#[derive(Debug, Deserialize)]
struct Support {
    #[serde(default)]
    support: Vec<SupportRow>,
}

/// The two registries, joined by dialect id.
struct Registries {
    /// Format ids the identity registry declares, in file order.
    formats: Vec<String>,
    /// Identity rows in file order.
    rows: Vec<IdentityRow>,
    /// Capability row per dialect id.
    support: BTreeMap<String, SupportRow>,
}

impl Registries {
    /// Parses both embedded registries.
    ///
    /// A parse failure means the binary shipped a malformed registry, which
    /// `scripts/check-dialects.py` and `scripts/check-dialect-support.py`
    /// forbid; `tests::the_embedded_registries_parse` is the in-tree guard.
    fn load() -> Result<Self> {
        let identity: Identity = toml::from_str(IDENTITY_TOML)?;
        let support: Support = toml::from_str(SUPPORT_TOML)?;
        Ok(Self {
            formats: identity.format.keys().cloned().collect(),
            rows: identity.dialect,
            support: support
                .support
                .into_iter()
                .map(|row| (row.dialect.clone(), row))
                .collect(),
        })
    }

    /// The identity rows of one format, in registry order.
    fn rows_of<'a>(&'a self, format: &str) -> impl Iterator<Item = &'a IdentityRow> {
        let prefix = format!("{format}:");
        self.rows
            .iter()
            .filter(move |row| row.id.starts_with(&prefix))
    }
}

/// The synthesis catalog of `format`'s encoder in this build, or `None` when
/// this build carries no encoder for it.
fn catalog_of(format: &str) -> Option<&'static [TargetDescriptor]> {
    Some(build_encoder(Format::from_name(format)?, LossPolicy::Report).targets())
}

/// The read disposition the capability registry records for a dialect.
fn read_of(support: &BTreeMap<String, SupportRow>, id: &str) -> Option<String> {
    support.get(id).map(|row| row.read.clone())
}

/// The `dialect:` line `cadmpeg inspect` prints under `format:`.
///
/// Three sources in one sentence: the classifier's own primary-layer match,
/// the read disposition the capability registry records for that id, and the
/// write targets this build's encoder for that format can synthesize. Returns
/// `None` when the codec reported no dialects at all, which is the honest
/// output for a codec that does not classify.
pub fn dialect_provenance(dialects: &[DialectMatch], format: &str) -> Option<String> {
    let entry = primary_layer(dialects, format)?;
    let registries = Registries::load().ok();
    let id = entry
        .dialect
        .as_ref()
        .map_or_else(|| "<unmatched>".to_owned(), |id| id.as_str().to_owned());

    let mut clauses = Vec::new();
    if let Some(read) = registries
        .as_ref()
        .and_then(|registries| read_of(&registries.support, &id))
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

/// Prints the identity registry crossed with the capability registry.
pub fn print_dialects(format: Option<&str>) -> Result<()> {
    let registries = Registries::load()?;
    let formats = match format {
        None => registries.formats.clone(),
        Some(name) => {
            let name =
                Format::from_name(name).map_or_else(|| name.to_owned(), |f| f.name().to_owned());
            if !registries.formats.contains(&name) {
                bail!(
                    "no format {name} in the dialect registry; known: {}",
                    registries.formats.join(", ")
                );
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
            let support = registries.support.get(&row.id);
            let target = catalog
                .and_then(|targets| find_target(targets, &row.id))
                .is_some();
            println!(
                "  {:<34} {:<24} {:<24} {}",
                row.id,
                support.map_or("-", |row| row.read.as_str()),
                match (support.map(|row| row.write.as_str()), target) {
                    (Some(write), true) => format!("{write} (target)"),
                    (Some(write), false) => write.to_owned(),
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
        assert!(!registries.rows.is_empty());
        for row in &registries.rows {
            assert!(
                registries.support.contains_key(&row.id),
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
        let registries = Registries::load().expect("the embedded registries parse");
        let ids = registries
            .rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for format in Format::ALL {
            let encoder = build_encoder(*format, LossPolicy::Report);
            for target in encoder.targets() {
                assert!(ids.contains(target.id), "{}: not a registry row", target.id);
            }
        }
    }

    /// The provenance line names the id, the read disposition, and the
    /// catalog. It is what `cadmpeg inspect` prints, so a change to any of the
    /// three sources shows up here.
    #[cfg(feature = "rhino")]
    #[test]
    fn the_provenance_line_joins_the_match_the_registry_and_the_catalog() {
        use cadmpeg_core::dialect::{Admission, DialectId};

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
