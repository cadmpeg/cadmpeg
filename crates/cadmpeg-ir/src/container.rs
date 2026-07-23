// SPDX-License-Identifier: Apache-2.0
//! Walking a ZIP container's entries under injected caps and classification.
//!
//! Two container codecs enumerate a `zip::ZipArchive` the same way — iterate
//! entries, classify each by name, enforce a per-entry and a total inflated-size
//! cap, then consume the payload — but consume it differently.
//! `cadmpeg-codec-f3d` inflates each entry into the decode platform, registering
//! a stored entry as a borrowed slice and a compressed one as a charged
//! expansion. `cadmpeg-codec-freecad` inflates into a plain `Vec<u8>` under its
//! own caps. The iteration, classification, and cap enforcement are shared; the
//! payload handling is not.
//!
//! This module holds the shared iteration and offers the payload in three modes:
//!
//! - [`walk_admitted`] admits each entry into the decode platform, yielding a
//!   [`View`]: a stored entry becomes a borrowed slice via
//!   [`DecodeContext::register_slice`], a compressed one a charged expansion via
//!   [`DecodeContext::begin_expand`]. This is f3d's path.
//! - [`walk_bounded`] inflates each entry into a `Vec<u8>` capped at
//!   `max_entry_bytes`. This is freecad's path.
//! - [`walk_metadata`] visits each entry's [`ZipEntry`] without inflating it.
//!
//! All three enforce the same [`WalkConfig`]: an injected `classify`, a name
//! validation hook, and a per-entry and total inflated-size cap. Neither codec's
//! caps are baked in.
//!
//! # Byte-floor
//!
//! The per-entry cap is enforced with [`bounded_len`](crate::wire::cursor::bounded_len)
//! against the *declared* uncompressed size: a size that exceeds the cap is
//! refused before any allocation, so a declared-size lie cannot drive a large
//! up-front reservation. [`walk_bounded`] additionally reads at most
//! `max_entry_bytes + 1` and refuses an entry whose *actual* inflated length
//! exceeds the cap, catching an entry that declares a small size but inflates
//! past it. [`walk_admitted`] gets the same guarantee from the decode platform's
//! incremental charging and exact-size expansion.
//!
//! # What stays codec-owned
//!
//! Only the two caps and the two hooks are shared. A codec keeps its own
//! entry-count limit, compression-method allowlist, duplicate-name detection,
//! expansion-ratio guard, per-entry header parsing (f3d's ASM facts,
//! freecad's physical ledger), and its exact-declared-size assertion —
//! [`walk_bounded`] enforces the cap but not equality with the declared size,
//! so freecad layers that check in its callback. The [`ZipEntry`] carries the
//! raw ZIP offsets those codec passes need.
#![deny(clippy::disallowed_methods)]

use std::io::{Cursor, Read, Seek};

use zip::read::ZipFile;
use zip::{CompressionMethod, ZipArchive};

use crate::codec::CodecError;
use crate::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use crate::wire::cursor::bounded_len;

/// The chunk size for streaming a compressed entry into the decode platform.
const EXPAND_CHUNK: usize = 16 * 1024;

/// Per-codec configuration for a ZIP walk.
///
/// The caps are the codec's own limits; nothing here is a default. `classify`
/// labels an entry by name for [`ZipEntry::role`], and `validate_name` rejects
/// an unsafe or unexpected entry path before its payload is touched.
#[derive(Clone, Copy)]
pub struct WalkConfig {
    /// Labels an entry by name, e.g. the codec's role families.
    pub classify: fn(&str) -> &'static str,
    /// Rejects an entry whose name is unsafe or unexpected. A codec that does
    /// not validate names passes a hook that always returns `Ok`.
    pub validate_name: fn(&str) -> Result<(), CodecError>,
    /// Maximum inflated bytes for one entry.
    pub max_entry_bytes: u64,
    /// Maximum inflated bytes summed over all entries.
    pub max_total_bytes: u64,
}

/// One ZIP entry's metadata, yielded before its payload is consumed.
///
/// The offsets and CRC mirror the local- and central-directory header fields a
/// codec needs to place the entry physically (freecad's ledger) or to admit it
/// into the decode platform (f3d's slice registration).
#[derive(Debug, Clone)]
pub struct ZipEntry {
    /// Entry name as stored in the archive.
    pub name: String,
    /// Role label from [`WalkConfig::classify`].
    pub role: &'static str,
    /// Compression method declared for the entry.
    pub compression: CompressionMethod,
    /// Whether the entry is encrypted.
    pub encrypted: bool,
    /// Compressed (stored) byte length.
    pub compressed_size: u64,
    /// Declared uncompressed byte length.
    pub uncompressed_size: u64,
    /// CRC-32 of the uncompressed data.
    pub crc32: u32,
    /// Absolute offset of the local file header.
    pub header_start: u64,
    /// Absolute offset of the entry's compressed data, when known.
    pub data_start: Option<u64>,
    /// Absolute offset of the entry's central-directory header.
    pub central_start: u64,
}

/// Reads one entry's metadata and charges it against the caps.
///
/// Validates the name, floors the declared uncompressed size against
/// `max_entry_bytes`, and accumulates `total`, refusing an entry or running
/// total that exceeds its cap.
fn entry_meta<R: Read + Seek>(
    file: &ZipFile<'_, R>,
    config: &WalkConfig,
    total: &mut u64,
) -> Result<ZipEntry, CodecError> {
    let name = file.name().to_string();
    (config.validate_name)(&name)?;
    let uncompressed_size = file.size();
    let cap = usize::try_from(config.max_entry_bytes).unwrap_or(usize::MAX);
    if bounded_len(uncompressed_size, 1, cap).is_none() {
        return Err(CodecError::Malformed(format!(
            "ZIP entry {name} declares {uncompressed_size} inflated bytes, over the \
             {}-byte per-entry cap",
            config.max_entry_bytes
        )));
    }
    *total = total
        .checked_add(uncompressed_size)
        .ok_or_else(|| CodecError::Malformed("ZIP total inflated size overflows u64".into()))?;
    if *total > config.max_total_bytes {
        return Err(CodecError::Malformed(format!(
            "ZIP entries declare {total} inflated bytes, over the {}-byte total cap",
            config.max_total_bytes
        )));
    }
    let role = (config.classify)(&name);
    Ok(ZipEntry {
        role,
        compression: file.compression(),
        encrypted: file.encrypted(),
        compressed_size: file.compressed_size(),
        uncompressed_size,
        crc32: file.crc32(),
        header_start: file.header_start(),
        data_start: file.data_start(),
        central_start: file.central_header_start(),
        name,
    })
}

/// Visits each entry's metadata without inflating its payload.
///
/// Enforces [`WalkConfig`] in archive order and calls `on_entry` with the
/// entry's [`ZipEntry`]. Nothing is decompressed.
pub fn walk_metadata<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    config: &WalkConfig,
    mut on_entry: impl FnMut(&ZipEntry) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let mut total = 0u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| CodecError::Malformed(format!("bad ZIP entry {index}: {error}")))?;
        let entry = entry_meta(&file, config, &mut total)?;
        drop(file);
        on_entry(&entry)?;
    }
    Ok(())
}

/// Inflates each entry into a `Vec<u8>` capped at [`WalkConfig::max_entry_bytes`].
///
/// Reads at most `max_entry_bytes + 1` bytes and refuses an entry whose inflated
/// length exceeds the cap, so an entry that declares a small size but inflates
/// past it is caught here. The cap is enforced, not equality with the declared
/// size; a codec that requires an exact match asserts it in `on_entry`.
pub fn walk_bounded<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    config: &WalkConfig,
    mut on_entry: impl FnMut(&ZipEntry, Vec<u8>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| CodecError::Malformed(format!("bad ZIP entry {index}: {error}")))?;
        let entry = entry_meta(&file, config, &mut total)?;
        let bytes = read_capped(&mut file, config.max_entry_bytes, &entry.name)?;
        drop(file);
        on_entry(&entry, bytes)?;
    }
    Ok(())
}

/// Reads at most `max + 1` bytes, refusing a payload that exceeds the cap.
fn read_capped(reader: &mut impl Read, max: u64, name: &str) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    Read::take(reader, max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| CodecError::Malformed(format!("cannot inflate {name}: {error}")))?;
    if bytes.len() as u64 > max {
        return Err(CodecError::Malformed(format!(
            "ZIP entry {name} inflates past the {max}-byte per-entry cap"
        )));
    }
    Ok(bytes)
}

/// Admits each entry into the decode platform, yielding a bounded [`View`].
///
/// A stored entry is registered as a borrowed slice of `root`; a compressed one
/// is inflated through a charged, exact-size expansion. The archive must read
/// the same bytes `root` spans so a stored entry's byte range resolves within
/// the parent space.
pub fn walk_admitted<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    archive: &mut ZipArchive<Cursor<&'a [u8]>>,
    config: &WalkConfig,
    mut on_entry: impl FnMut(&ZipEntry, View<'a>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| CodecError::Malformed(format!("bad ZIP entry {index}: {error}")))?;
        let entry = entry_meta(&file, config, &mut total)?;
        let view = admit(ctx, root, &mut file, &entry)?;
        drop(file);
        on_entry(&entry, view)?;
    }
    Ok(())
}

/// Registers a stored entry as a slice or inflates a compressed one, charging
/// the decode platform.
fn admit<'a>(
    ctx: &DecodeContext<'a>,
    parent: View<'a>,
    file: &mut ZipFile<'_, Cursor<&'a [u8]>>,
    entry: &ZipEntry,
) -> Result<View<'a>, CodecError> {
    let data_start = entry.data_start.ok_or_else(|| {
        CodecError::Malformed(format!("entry {} has no local data offset", entry.name))
    })?;
    let data_end = data_start
        .checked_add(entry.compressed_size)
        .ok_or_else(|| {
            CodecError::Malformed(format!("entry {} data range overflows", entry.name))
        })?;

    if entry.compression == CompressionMethod::Stored {
        return ctx.register_slice(
            parent,
            ByteRange {
                start: data_start,
                end: data_end,
            },
        );
    }

    let source = child_range(parent, data_start, data_end).ok_or_else(|| {
        CodecError::Malformed(format!(
            "entry {} data range escapes its parent space",
            entry.name
        ))
    })?;
    let mut writer = ctx.begin_expand(source, ExpandSpec::Exact(entry.uncompressed_size))?;
    let mut chunk = [0u8; EXPAND_CHUNK];
    loop {
        let read = file.read(&mut chunk).map_err(|error| {
            CodecError::Malformed(format!("cannot inflate {}: {error}", entry.name))
        })?;
        if read == 0 {
            break;
        }
        writer.write(chunk.get(..read).unwrap_or_default())?;
    }
    writer.finalize()
}

/// Builds a child view over an absolute `[start, end)` root range.
fn child_range(root: View<'_>, start: u64, end: u64) -> Option<View<'_>> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    root.child(start, end)
}

#[cfg(test)]
mod tests {
    use super::{walk_admitted, walk_bounded, walk_metadata, WalkConfig, ZipEntry};
    use crate::codec::CodecError;
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::io::{Cursor, Write as _};

    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn accept_all(_name: &str) -> Result<(), CodecError> {
        Ok(())
    }

    /// Classifies by extension into a couple of stable labels.
    fn classify(name: &str) -> &'static str {
        if name.ends_with(".brep") {
            "brep"
        } else if name.ends_with(".xml") {
            "document"
        } else {
            "other"
        }
    }

    fn config(max_entry_bytes: u64, max_total_bytes: u64) -> WalkConfig {
        WalkConfig {
            classify,
            validate_name: accept_all,
            max_entry_bytes,
            max_total_bytes,
        }
    }

    /// Builds an in-memory ZIP from `(name, method, data)` entries.
    fn build_zip(entries: &[(&str, CompressionMethod, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, method, data) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(*method),
                )
                .expect("start entry");
            writer.write_all(data).expect("write entry");
        }
        writer.finish().expect("finish ZIP").into_inner()
    }

    fn open(bytes: &[u8]) -> zip::ZipArchive<Cursor<&[u8]>> {
        zip::ZipArchive::new(Cursor::new(bytes)).expect("read ZIP")
    }

    #[test]
    fn walk_metadata_yields_classified_entries_without_inflating() {
        let bytes = build_zip(&[
            ("Document.xml", CompressionMethod::Stored, b"<Document/>"),
            ("Body.brep", CompressionMethod::Deflated, b"brep body bytes"),
        ]);
        let mut archive = open(&bytes);
        let cfg = config(1024, 4096);

        let mut seen: Vec<(String, &'static str)> = Vec::new();
        walk_metadata(&mut archive, &cfg, |entry: &ZipEntry| {
            seen.push((entry.name.clone(), entry.role));
            Ok(())
        })
        .expect("walk");
        assert_eq!(
            seen,
            vec![
                ("Document.xml".to_string(), "document"),
                ("Body.brep".to_string(), "brep"),
            ]
        );
    }

    #[test]
    fn walk_bounded_inflates_each_entry() {
        let bytes = build_zip(&[
            ("stored.bin", CompressionMethod::Stored, b"stored payload"),
            (
                "packed.bin",
                CompressionMethod::Deflated,
                b"deflated payload here",
            ),
        ]);
        let mut archive = open(&bytes);
        let cfg = config(1024, 4096);

        let mut payloads: Vec<(String, Vec<u8>)> = Vec::new();
        walk_bounded(&mut archive, &cfg, |entry, data| {
            payloads.push((entry.name.clone(), data));
            Ok(())
        })
        .expect("walk");
        assert_eq!(
            payloads[0],
            ("stored.bin".to_string(), b"stored payload".to_vec())
        );
        assert_eq!(
            payloads[1],
            ("packed.bin".to_string(), b"deflated payload here".to_vec())
        );
    }

    #[test]
    fn walk_admitted_registers_stored_and_inflates_compressed() {
        let bytes = build_zip(&[
            ("stored.bin", CompressionMethod::Stored, b"stored payload"),
            (
                "packed.bin",
                CompressionMethod::Deflated,
                b"deflated payload here",
            ),
        ]);
        let arena = DecodeArena::default();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("root");
        let mut archive = open(&bytes);
        let cfg = config(1024, 4096);

        let mut windows: Vec<Vec<u8>> = Vec::new();
        walk_admitted(&ctx, root, &mut archive, &cfg, |_entry, view| {
            windows.push(view.window().to_vec());
            Ok(())
        })
        .expect("walk");
        assert_eq!(windows[0], b"stored payload");
        assert_eq!(windows[1], b"deflated payload here");
    }

    #[test]
    fn entry_over_the_per_entry_cap_is_refused() {
        let bytes = build_zip(&[("big.bin", CompressionMethod::Stored, &[b'A'; 300])]);
        let mut archive = open(&bytes);
        let cfg = config(100, 4096);

        let error = walk_metadata(&mut archive, &cfg, |_| Ok(())).expect_err("over per-entry cap");
        assert!(
            matches!(error, CodecError::Malformed(message) if message.contains("per-entry cap"))
        );
    }

    #[test]
    fn total_over_the_cap_is_refused_on_the_entry_that_crosses_it() {
        let bytes = build_zip(&[
            ("a.bin", CompressionMethod::Stored, &[b'A'; 80]),
            ("b.bin", CompressionMethod::Stored, &[b'B'; 80]),
        ]);
        let mut archive = open(&bytes);
        // Each entry fits the per-entry cap; together they exceed the total.
        let cfg = config(100, 128);

        let mut seen = 0;
        let error = walk_metadata(&mut archive, &cfg, |_| {
            seen += 1;
            Ok(())
        })
        .expect_err("over total cap");
        assert!(matches!(error, CodecError::Malformed(message) if message.contains("total cap")));
        // The first entry was admitted; the second crossed the total cap.
        assert_eq!(seen, 1);
    }

    #[test]
    fn declared_size_lie_is_floored_before_allocation() {
        // An honest small entry whose central-directory uncompressed size is
        // then overwritten with a huge value: the byte-floor refuses it on the
        // declared size, before any allocation the lie would otherwise drive.
        let mut bytes = build_zip(&[("bomb.bin", CompressionMethod::Stored, b"tiny")]);
        overwrite_central_uncompressed_size(&mut bytes, 2_000_000_000);
        let mut archive = open(&bytes);
        let cfg = config(1_000_000, 8_000_000);

        let error = walk_metadata(&mut archive, &cfg, |_| Ok(())).expect_err("declared-size lie");
        assert!(
            matches!(error, CodecError::Malformed(message) if message.contains("per-entry cap"))
        );
    }

    /// Overwrites the uncompressed-size field of the single central-directory
    /// header (`PK\x01\x02`, field at offset +24) with `size`.
    fn overwrite_central_uncompressed_size(bytes: &mut [u8], size: u32) {
        let signature = b"PK\x01\x02";
        let at = bytes
            .windows(4)
            .position(|window| window == signature)
            .expect("central header present");
        let field = at + 24;
        bytes[field..field + 4].copy_from_slice(&size.to_le_bytes());
    }
}
