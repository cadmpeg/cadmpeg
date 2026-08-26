// SPDX-License-Identifier: Apache-2.0
//! Container member listing (ZIP or CFB) and exact member extraction.

use std::fmt::Write as _;

use anyhow::{Context, Result};
use cadmpeg_container::compound::{CompoundAllocation, CompoundEntry, CompoundSnapshot};
use cadmpeg_container::{ArchiveSnapshot, EntryRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};

const CFB_MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// One CFB directory row in a container listing.
pub struct CfbRow {
    /// `"storage"` or `"stream"`.
    pub kind: &'static str,
    /// Hierarchy path with source spelling preserved.
    pub path: String,
    /// Logical stream size; `None` for storages.
    pub size: Option<u64>,
    /// `"fat"` or `"mini-fat"`; `None` for storages.
    pub allocation: Option<&'static str>,
    /// CFB directory-entry index.
    pub directory_id: u32,
}

/// Container members listed from a ZIP archive or a CFB file.
pub enum Listing {
    /// ZIP central-directory entries.
    Zip(Vec<EntryRecord>),
    /// CFB directory rows (storages and streams).
    Cfb(Vec<CfbRow>),
}

/// Lists ZIP entries or CFB directory members in `bytes`.
///
/// # Errors
///
/// Returns an error when the bytes exceed the resource-limit profile or the
/// ZIP central directory or CFB directory does not parse.
pub fn list(bytes: &[u8], limits: ResourceLimits) -> Result<Listing> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits,
        ..DecodePolicy::default()
    };
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .context("the file does not fit the resource-limit profile")?;
    if bytes.starts_with(&CFB_MAGIC) {
        let snapshot = CompoundSnapshot::new(&ctx, root).context("reading the CFB directory")?;
        let rows = snapshot
            .entries()
            .iter()
            .map(|entry| match entry {
                CompoundEntry::Storage(storage) => CfbRow {
                    kind: "storage",
                    path: storage.path().to_string(),
                    size: None,
                    allocation: None,
                    directory_id: entry.directory_id(),
                },
                CompoundEntry::Stream(stream) => CfbRow {
                    kind: "stream",
                    path: stream.path().to_string(),
                    size: Some(stream.logical_size()),
                    allocation: Some(match stream.allocation() {
                        CompoundAllocation::Regular => "fat",
                        CompoundAllocation::Mini => "mini-fat",
                    }),
                    directory_id: entry.directory_id(),
                },
            })
            .collect();
        Ok(Listing::Cfb(rows))
    } else {
        let snapshot = ArchiveSnapshot::new(root).context("reading the ZIP central directory")?;
        Ok(Listing::Zip(snapshot.entries().to_vec()))
    }
}

/// Extracts one ZIP entry or CFB stream.
///
/// The archive `ctx` and arena stay alive in this scope until the view's
/// bytes are copied out; `list` drops them, which is why extraction cannot
/// reuse it.
///
/// # Errors
///
/// Returns an error when the bytes are not a supported container within the
/// limit profile, when no stream or entry has exactly `name`, or when opening
/// the member fails structural, size, or integrity checks.
pub fn extract(bytes: &[u8], limits: ResourceLimits, name: &str) -> Result<Vec<u8>> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits,
        ..DecodePolicy::default()
    };
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .context("the file does not fit the resource-limit profile")?;
    if bytes.starts_with(&CFB_MAGIC) {
        let snapshot = CompoundSnapshot::new(&ctx, root).context("reading the CFB directory")?;
        let entry = snapshot.stream(name).ok_or_else(|| {
            anyhow::anyhow!("{}", missing_compound_member_message(&snapshot, name))
        })?;
        let view = snapshot
            .open(&ctx, entry)
            .with_context(|| format!("opening stream {}", shell_quote(name)))?;
        return Ok(view.window().to_vec());
    }
    let snapshot = ArchiveSnapshot::new(root).context("reading the ZIP central directory")?;
    let entry = snapshot
        .entry(name)
        .ok_or_else(|| anyhow::anyhow!("{}", missing_member_message(&snapshot, name)))?;
    let view = snapshot
        .open(&ctx, entry)
        .with_context(|| format!("opening entry {}", shell_quote(name)))?;
    Ok(view.window().to_vec())
}

fn missing_compound_member_message(snapshot: &CompoundSnapshot<'_>, name: &str) -> String {
    const SHOWN: usize = 10;
    let lower = name.to_lowercase();
    let mut label = "close stream names";
    let mut names = snapshot
        .entries()
        .iter()
        .filter(|entry| matches!(entry, CompoundEntry::Stream(_)))
        .filter(|entry| {
            entry.path().to_lowercase().contains(&lower)
                || entry.path().rsplit('/').next() == Some(name)
        })
        .take(SHOWN)
        .map(|entry| shell_quote(entry.path()))
        .collect::<Vec<_>>();
    if names.is_empty() {
        label = "streams include";
        names = snapshot
            .entries()
            .iter()
            .filter(|entry| matches!(entry, CompoundEntry::Stream(_)))
            .take(SHOWN)
            .map(|entry| shell_quote(entry.path()))
            .collect();
    }
    format!(
        "no stream is named exactly {}; {label}: {}; run `cadmpeg inspect FILE` for the full list",
        shell_quote(name),
        names.join(", ")
    )
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
pub fn render_json(listing: &Listing) -> String {
    let (container_kind, entries): (&str, Vec<serde_json::Value>) = match listing {
        Listing::Zip(entries) => (
            "zip",
            entries
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
                .collect(),
        ),
        Listing::Cfb(rows) => (
            "cfb",
            rows.iter()
                .map(|row| {
                    serde_json::json!({
                        "kind": row.kind,
                        "path": row.path,
                        "size": row.size,
                        "allocation": row.allocation,
                        "directory_id": row.directory_id,
                    })
                })
                .collect(),
        ),
    };
    let envelope = serde_json::json!({
        "schema_version": crate::commands::CLI_SCHEMA_VERSION,
        "command": "inspect container",
        "status": "ok",
        "refusal": null,
        "container_kind": container_kind,
        "entries": entries,
    });
    let mut rendered = serde_json::to_string_pretty(&envelope).expect("the envelope serializes");
    rendered.push('\n');
    rendered
}

/// Formats an entry listing as an aligned table.
pub fn render(listing: &Listing) -> String {
    match listing {
        Listing::Zip(entries) => {
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
        Listing::Cfb(rows) => {
            let mut out = String::new();
            let _ = writeln!(
                out,
                "{:>4}  {:>8}  {:>12}  {:>8}  path",
                "id", "kind", "size", "alloc"
            );
            for row in rows {
                let size = row.size.map(|n| n.to_string()).unwrap_or_default();
                let alloc = row.allocation.unwrap_or("");
                let _ = writeln!(
                    out,
                    "{:>4}  {:>8}  {:>12}  {:>8}  {}",
                    row.directory_id,
                    row.kind,
                    size,
                    alloc,
                    shell_quote(&row.path)
                );
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFB_SECTOR: usize = 512;
    const CFB_FREE: u32 = 0xffff_ffff;
    const CFB_END: u32 = 0xffff_fffe;
    const CFB_FAT: u32 = 0xffff_fffd;

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

    #[test]
    fn extracts_a_compound_stream_by_exact_path() {
        let file = compound_fixture();
        assert_eq!(
            extract(&file, ResourceLimits::desktop(), "Payload")
                .expect("synthetic CFB stream extracts"),
            vec![0x5a; 4096]
        );
    }

    #[test]
    fn lists_compound_storages_and_streams() {
        let file = compound_fixture();
        let Listing::Cfb(rows) =
            list(&file, ResourceLimits::desktop()).expect("synthetic CFB lists")
        else {
            panic!("expected a CFB listing");
        };
        let payload = rows
            .iter()
            .find(|row| row.path == "Payload")
            .expect("Payload row");
        assert_eq!(payload.kind, "stream");
        assert_eq!(payload.size, Some(4096));
    }

    fn compound_fixture() -> Vec<u8> {
        let mut file = vec![0_u8; CFB_SECTOR * 11];
        file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        put_u16(&mut file, 24, 0x003e);
        put_u16(&mut file, 26, 3);
        put_u16(&mut file, 28, 0xfffe);
        put_u16(&mut file, 30, 9);
        put_u16(&mut file, 32, 6);
        put_u32(&mut file, 44, 1);
        put_u32(&mut file, 48, 0);
        put_u32(&mut file, 56, 4096);
        put_u32(&mut file, 60, CFB_END);
        put_u32(&mut file, 68, CFB_END);
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, CFB_FREE);
        }
        put_u32(&mut file, 76, 9);
        let directory = sector_mut(&mut file, 0);
        for entry in directory.chunks_exact_mut(128) {
            entry[68..80].fill(0xff);
        }
        directory_entry(directory, 0, "Root Entry", 5, 1, CFB_END, 0);
        directory_entry(directory, 1, "Payload", 2, CFB_FREE, 1, 4096);
        for sector in 1..=8 {
            sector_mut(&mut file, sector).fill(0x5a);
        }
        let fat = sector_mut(&mut file, 9);
        fat.fill(0xff);
        put_u32(fat, 0, CFB_END);
        for sector in 1..8 {
            put_u32(fat, sector * 4, (sector + 1) as u32);
        }
        put_u32(fat, 8 * 4, CFB_END);
        put_u32(fat, 9 * 4, CFB_FAT);
        file
    }

    fn directory_entry(
        directory: &mut [u8],
        index: usize,
        name: &str,
        object_type: u8,
        child: u32,
        start: u32,
        size: u64,
    ) {
        let entry = &mut directory[index * 128..(index + 1) * 128];
        let units = name.encode_utf16().collect::<Vec<_>>();
        for (offset, unit) in units.iter().enumerate() {
            put_u16(entry, offset * 2, *unit);
        }
        put_u16(entry, 64, ((units.len() + 1) * 2) as u16);
        entry[66] = object_type;
        entry[67] = 1;
        put_u32(entry, 68, CFB_FREE);
        put_u32(entry, 72, CFB_FREE);
        put_u32(entry, 76, child);
        put_u32(entry, 116, start);
        entry[120..128].copy_from_slice(&size.to_le_bytes());
    }

    fn sector_mut(file: &mut [u8], sector: usize) -> &mut [u8] {
        let start = (sector + 1) * CFB_SECTOR;
        &mut file[start..start + CFB_SECTOR]
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
