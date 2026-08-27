// SPDX-License-Identifier: Apache-2.0
//! Rendering of the registry tables for the terminal.
//!
//! `cadmpeg-registry` joins the identity registry, the capability registry,
//! and this build's encoder catalogs, and returns rows. The column widths, the
//! word "yes", and the trailing pointer to another command are this crate's,
//! because they are what a terminal reader sees and nothing a library caller
//! should have to parse back out of a string.

use cadmpeg_registry::{
    dialect_provenance, dialect_table, format_rows, DialectProvenance, InputCatalog, UnknownFormat,
};

/// Prints the formats this build reads and writes.
///
/// Two columns, because reading and writing differ per format: Inventor,
/// CATIA, Creo, NX, and SAT are read-only, and one column would have to
/// misstate one half of each of them.
pub fn print_formats(inputs: &InputCatalog) {
    println!("FORMAT     READ   WRITE  EXTENSIONS");
    for row in format_rows(inputs) {
        println!(
            "{:<10} {:<6} {:<6} {}",
            row.id,
            yes_no(row.read),
            yes_no(row.write),
            row.extensions.join(", ")
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
pub fn print_dialects(format: Option<&str>) -> Result<(), UnknownFormat> {
    for (index, section) in dialect_table(format)?.iter().enumerate() {
        if index > 0 {
            println!();
        }
        let name = &section.format;
        match section.catalog {
            Some(targets) if !targets.is_empty() => {
                let default = section.default_target.unwrap_or("none");
                println!("{name}  (write targets in this build; default {default})");
            }
            Some(_) => println!("{name}  (this build writes it, with no dialect catalog)"),
            None => println!("{name}  (no encoder in this build)"),
        }
        println!("  DIALECT                            READ                     WRITE                    TITLE");
        for row in &section.rows {
            println!(
                "  {:<34} {:<24} {:<24} {}",
                row.id.as_str(),
                row.read
                    .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                match (row.write, row.target) {
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

/// The `dialect:` line `cadmpeg inspect` prints under `format:`.
///
/// Three sources in one sentence: the matched id, the declared read
/// disposition, and the write targets this build can synthesize. `None` when
/// the codec reported no dialects at all, which is the honest output for a
/// codec that does not classify.
pub fn dialect_line(
    dialects: &[cadmpeg_core::dialect::DialectMatch],
    format: &str,
) -> Option<String> {
    let DialectProvenance {
        id,
        read,
        write_targets,
    } = dialect_provenance(dialects, format)?;
    let id = id.map_or_else(|| "<unmatched>".to_owned(), |id| id.as_str().to_owned());

    let mut clauses = Vec::new();
    if let Some(read) = read {
        clauses.push(format!("read {read}"));
    }
    if !write_targets.is_empty() {
        let targets = write_targets
            .iter()
            .map(|id| suffix(id))
            .collect::<Vec<_>>()
            .join(", ");
        clauses.push(format!("write targets {targets}"));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The provenance line names the id, the read disposition, and the
    /// catalog. It is what `cadmpeg inspect` prints, so a change to any of the
    /// three sources shows up here.
    #[cfg(feature = "rhino")]
    #[test]
    fn the_provenance_line_joins_the_match_the_registry_and_the_catalog() {
        use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};

        let dialects = vec![DialectMatch {
            format: "rhino".to_owned(),
            dialect: Some(DialectId::pinned("rhino:archive-50")),
            declared: std::collections::BTreeMap::new(),
            admission: Admission::Admitted,
        }];
        let line = dialect_line(&dialects, "rhino").expect("a primary layer exists");
        assert!(line.starts_with("dialect: rhino:archive-50 — "), "{line}");
        assert!(line.contains("read "), "{line}");
        assert!(line.contains("write targets archive-50"), "{line}");
        assert!(line.contains("archive-80"), "{line}");
    }

    /// A codec that classified nothing prints no dialect line.
    #[test]
    fn no_dialects_is_no_line() {
        assert!(dialect_line(&[], "rhino").is_none());
    }
}
