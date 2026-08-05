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
