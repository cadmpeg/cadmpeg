// SPDX-License-Identifier: Apache-2.0
//! ZIP entry listing with the physical offsets the other byte tools take.

use std::fmt::Write as _;

use anyhow::{Context, Result};
use cadmpeg_container::{ArchiveSnapshot, EntryRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};

/// Lists the ZIP entries in `bytes`.
///
/// # Errors
///
/// Returns an error when the bytes exceed the resource-limit profile or the
/// central directory does not parse.
pub fn list(bytes: &[u8], limits: ResourceLimits) -> Result<Vec<EntryRecord>> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits,
        ..DecodePolicy::default()
    };
    let (_ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .context("the file does not fit the resource-limit profile")?;
    let snapshot = ArchiveSnapshot::new(root).context("reading the ZIP central directory")?;
    Ok(snapshot.entries().to_vec())
}

/// Extracts one entry's decompressed, CRC-verified bytes.
///
/// The archive `ctx` and arena stay alive in this scope until the view's
/// bytes are copied out; `list` drops them, which is why extraction cannot
/// reuse it.
///
/// # Errors
///
/// Returns an error when the bytes are not a ZIP archive within the limit
/// profile, when no entry has exactly `name` (the message suggests close
/// names), or when the entry fails its size or CRC check.
pub fn extract(bytes: &[u8], limits: ResourceLimits, name: &str) -> Result<Vec<u8>> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits,
        ..DecodePolicy::default()
    };
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .context("the file does not fit the resource-limit profile")?;
    let snapshot = ArchiveSnapshot::new(root).context("reading the ZIP central directory")?;
    let entry = snapshot
        .entry(name)
        .ok_or_else(|| anyhow::anyhow!("{}", missing_member_message(&snapshot, name)))?;
    let view = snapshot
        .open(&ctx, entry)
        .with_context(|| format!("opening entry {}", shell_quote(name)))?;
    Ok(view.window().to_vec())
}

/// Builds the error text for a member name with no exact match.
///
/// Names that contain the request case-insensitively, or whose final path
/// component equals it, are suggested first; with no near-miss the first
/// entries are listed instead. Every name is shell-quoted the way the
/// listing prints it.
fn missing_member_message(snapshot: &ArchiveSnapshot<'_>, name: &str) -> String {
    const SHOWN: usize = 10;
    let lower = name.to_lowercase();
    let mut label = "close names";
    let mut names: Vec<String> = snapshot
        .entries()
        .iter()
        .filter(|entry| {
            entry.name.to_lowercase().contains(&lower)
                || entry.name.rsplit('/').next() == Some(name)
        })
        .take(SHOWN)
        .map(|entry| shell_quote(&entry.name))
        .collect();
    if names.is_empty() {
        label = "entries include";
        names = snapshot
            .entries()
            .iter()
            .take(SHOWN)
            .map(|entry| shell_quote(&entry.name))
            .collect();
    }
    format!(
        "no entry is named exactly {}; {label}: {}; run `cadmpeg inspect container FILE` \
         for the full list",
        shell_quote(name),
        names.join(", ")
    )
}

/// Quotes an entry name so it survives a POSIX shell verbatim.
///
/// Fusion `.f3d` entry names hold `[` and `]`, which a shell expands as a glob
/// character class. Single quotes suppress every expansion, and an embedded
/// single quote is closed, escaped, and reopened.
pub fn shell_quote(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('\'');
    for c in name.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Formats an entry listing as the versioned JSON envelope.
///
/// Names are raw strings here — shell quoting belongs to the table
/// rendering, not to JSON.
pub fn render_json(entries: &[EntryRecord]) -> String {
    let entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "name": entry.name,
                "compression": entry.compression.label(),
                "crc32": entry.crc32,
                "compressed_size": entry.compressed_size,
                "uncompressed_size": entry.uncompressed_size,
                "header_start": entry.header_start,
                "data_start": entry.data_start,
                "central_start": entry.central_start,
            })
        })
        .collect();
    let envelope = serde_json::json!({
        "schema_version": crate::commands::CLI_SCHEMA_VERSION,
        "command": "inspect container",
        "entries": entries,
    });
    let mut rendered = serde_json::to_string_pretty(&envelope).expect("the envelope serializes");
    rendered.push('\n');
    rendered
}

/// Formats an entry listing as an aligned table.
pub fn render(entries: &[EntryRecord]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:>10}  {:>10}  {:>12}  {:>12}  {:>8}  {:>10}  name",
        "header", "data", "packed", "unpacked", "method", "crc32"
    );
    for entry in entries {
        let _ = writeln!(
            out,
            "0x{:08x}  0x{:08x}  {:>12}  {:>12}  {:>8}  0x{:08x}  {}",
            entry.header_start,
            entry.data_start,
            entry.compressed_size,
            entry.uncompressed_size,
            entry.compression.label(),
            entry.crc32,
            shell_quote(&entry.name)
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_plain_names() {
        assert_eq!(shell_quote("Document.xml"), "'Document.xml'");
    }

    #[test]
    fn quotes_bracketed_and_spaced_names() {
        assert_eq!(
            shell_quote("FusionAssetName[Active]/Design.dat"),
            "'FusionAssetName[Active]/Design.dat'"
        );
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("$HOME`x`"), "'$HOME`x`'");
    }

    #[test]
    fn closes_reopens_around_an_embedded_single_quote() {
        // 'it'\''s' concatenates to the four characters it's in a POSIX shell.
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
