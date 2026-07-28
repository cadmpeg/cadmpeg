// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Scan and classify the ZIP container inside a `.f3d` file.
//!
//! [`scan`] retains the source archive, enumerates each entry, reads ASM headers
//! from `.smb` and `.smbh` B-rep streams, and locates their `delta_state`
//! history boundaries. Model geometry is selected from Design body-to-blob
//! bindings by [`crate::decode`]. [`select_history_brep`] independently locates
//! the stream whose header declares a history partition. When Design bindings
//! are absent, [`select_fallback_brep`] supplies an explicit compatibility
//! fallback without asserting that one extension is the document model.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_ir::hash::sha256_hex;
use zip::CompressionMethod;

use crate::asm_header;

/// Maximum `.f3d` archive accepted by the container scanner.
const INPUT_CAP: u64 = 1024 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_BYTES: u64 = INPUT_CAP;
pub(crate) const MAX_INFLATED_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

const EXPAND_CHUNK: usize = 16 * 1024;

/// Codec-defined role labels for [`ContainerEntry::role`].
pub mod role {
    /// An ASM BREP entry with the `.smbh` extension. Its header normally
    /// declares a history partition.
    pub const BREP_SMBH: &str = "brep-smbh";
    /// An ASM BREP entry with the `.smb` extension. Its header normally omits
    /// the history partition.
    pub const BREP_SMB: &str = "brep-smb";
    /// A nested `.protein` material/appearance ZIP.
    pub const PROTEIN: &str = "protein-assets";
    /// A design/ACT/browser `BulkStream.dat`.
    pub const BULKSTREAM: &str = "bulkstream";
    /// A per-segment `MetaStream.dat` object table.
    pub const METASTREAM: &str = "metastream";
    /// A top-level or per-asset `Manifest.dat`.
    pub const MANIFEST: &str = "manifest";
    /// A thumbnail or preview asset.
    pub const PREVIEW: &str = "preview";
    /// An optional appearance/decal image blob.
    pub const IMAGE: &str = "image";
    /// Secondary tessellated mesh data (`.paramesh`), not the exact source.
    pub const PARAMESH: &str = "paramesh";
    /// An empty/placeholder design-configuration entry.
    pub const DESIGN_CONFIG: &str = "design-config";
    /// The empty top-level document-properties slot.
    pub const PROPERTIES: &str = "properties";
    /// A directory entry.
    pub const DIRECTORY: &str = "directory";
    /// Anything not matched by a known family.
    pub const OTHER: &str = "other";
}

/// The f3d marker substrings used for confident detection from a byte prefix
/// (ZIP local file headers store entry names in cleartext near the start).
pub const DETECT_MARKERS: &[&[u8]] = &[
    b"Breps.BlobParts",
    b"FusionAssetName",
    b"FusionDocType",
    b".smbh",
];
/// Marker names that distinguish a multi-document F3Z archive from a generic ZIP.
pub const F3Z_DETECT_MARKERS: &[&[u8]] = &[b"Manifest.json", b"DesignDescription.json", b".f3d"];

pub(crate) fn read_entry_bounded(
    entry: &mut impl Read,
    declared_size: u64,
    name: &str,
) -> Result<Vec<u8>, CodecError> {
    if declared_size > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::Malformed(format!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    let mut bytes = Vec::new();
    Read::take(entry, MAX_INFLATED_ENTRY_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::Malformed(format!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    Ok(bytes)
}

/// Classify an entry by its name using the spec's naming families ([§1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#1-container-layer), [§7](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#7-geometry-carriers)).
pub fn classify(name: &str) -> &'static str {
    if name.ends_with('/') {
        return role::DIRECTORY;
    }
    let base = name.rsplit('/').next().unwrap_or(name);
    if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("smbh"))
    {
        role::BREP_SMBH
    } else if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("smb"))
    {
        role::BREP_SMB
    } else if name.ends_with(".protein") {
        role::PROTEIN
    } else if name.ends_with(".paramesh") {
        role::PARAMESH
    } else if name.ends_with(".dsgcfg") || name.ends_with(".dsgcfgrule") {
        role::DESIGN_CONFIG
    } else if base == "Manifest.dat" {
        role::MANIFEST
    } else if base == "MetaStream.dat" {
        role::METASTREAM
    } else if base == "BulkStream.dat" {
        role::BULKSTREAM
    } else if base == "Properties.dat" {
        role::PROPERTIES
    } else if name.contains("Previews/") {
        role::PREVIEW
    } else if name.contains("Images.BlobParts") {
        role::IMAGE
    } else {
        role::OTHER
    }
}

fn compression_label(method: CompressionMethod) -> String {
    match method {
        CompressionMethod::Stored => "stored".to_string(),
        CompressionMethod::Deflated => "deflate".to_string(),
        CompressionMethod::Zstd => "zstd".to_string(),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// One decoded BREP stream's header facts, kept for the summary and decode
/// metadata.
#[derive(Debug, Clone)]
pub struct BrepFacts {
    /// Entry name.
    pub name: String,
    /// Whether the archive entry has the `.smbh` extension.
    pub is_smbh: bool,
    /// Uncompressed byte length.
    pub uncompressed_len: u64,
    /// Parsed ASM header, if the magic was present.
    pub header: Option<asm_header::AsmHeader>,
    /// Exact byte boundary between solved records and construction history.
    pub solved_record_limit: Option<usize>,
    /// SHA-256 (lowercase hex) of the decompressed stream.
    pub sha256: String,
}

/// The full result of reading a `.f3d` container: the entry list plus decoded
/// BREP facts. Shared by `inspect` and `decode`.
///
/// The `'a` lifetime is the session's root address space: stored entries are
/// views borrowing the root without copying, and compressed entries are arena-backed views the
/// platform expander produced, so both live for the decode's duration.
pub struct ContainerScan<'a> {
    /// Complete source archive retained for byte-exact native replay.
    pub source_image: &'a [u8],
    /// Enumerated entries with classification.
    pub entries: Vec<ContainerEntry>,
    /// Decoded BREP stream facts, in archive order.
    pub breps: Vec<BrepFacts>,
    /// The asset-folder prefix observed from BREP entry paths, if any.
    pub asset_folder: Option<String>,
    /// Entry payload views, keyed by archive path.
    inflated_entries: BTreeMap<String, View<'a>>,
}

impl<'a> ContainerScan<'a> {
    /// Returns an entry payload retained during the single archive scan.
    pub fn entry_bytes(&self, name: &str) -> Result<&'a [u8], CodecError> {
        self.entry_view(name)
            .map(View::window)
            .ok_or_else(|| CodecError::Malformed(format!("entry {name} not found")))
    }

    /// Returns an entry's payload view.
    pub(crate) fn entry_view(&self, name: &str) -> Option<View<'a>> {
        self.inflated_entries.get(name).copied()
    }
}

/// Admit one archive entry and enforce its declared uncompressed size.
pub(crate) fn admit_entry<'a>(
    ctx: &DecodeContext<'a>,
    parent: View<'a>,
    file: &mut zip::read::ZipFile<'_, Cursor<&'a [u8]>>,
    name: &str,
) -> Result<View<'a>, CodecError> {
    let compression = file.compression();
    let compressed_size = file.compressed_size();
    let uncompressed_size = file.size();
    let data_start = file
        .data_start()
        .ok_or_else(|| CodecError::Malformed(format!("entry {name} has no local data offset")))?;
    let data_end = data_start
        .checked_add(compressed_size)
        .ok_or_else(|| CodecError::Malformed(format!("entry {name} data range overflows")))?;

    if compression == CompressionMethod::Stored {
        let view = ctx.register_slice(
            parent,
            ByteRange {
                start: data_start,
                end: data_end,
            },
        )?;
        return Ok(view);
    }

    let source = child_range(parent, data_start, data_end).ok_or_else(|| {
        CodecError::Malformed(format!("entry {name} data range escapes its parent space"))
    })?;
    let mut writer = ctx.begin_expand(source, ExpandSpec::Exact(uncompressed_size))?;
    let mut chunk = [0u8; EXPAND_CHUNK];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|e| CodecError::Malformed(format!("cannot inflate {name}: {e}")))?;
        if read == 0 {
            break;
        }
        writer.write(&chunk[..read])?;
    }
    writer.finalize()
}

/// Build a child view over an absolute `[start, end)` root range, refusing a
/// range that escapes the root window or overflows the address space.
fn child_range(root: View<'_>, start: u64, end: u64) -> Option<View<'_>> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    root.child(start, end)
}

/// Read and classify every entry, decoding ASM headers for BREP streams.
///
/// Every entry is registered as a slice when stored or a decompressed space
/// when compressed.
pub fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<ContainerScan<'a>, CodecError> {
    let source_image = root.window();
    if source_image.len() as u64 > INPUT_CAP {
        return Err(CodecError::Malformed(format!(
            "input exceeds f3d size cap of {INPUT_CAP} bytes"
        )));
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(source_image))
        .map_err(|e| CodecError::Malformed(format!("not a readable ZIP: {e}")))?;

    let mut entries = Vec::new();
    let mut breps = Vec::new();
    let mut asset_folder = None;
    let mut inflated_entries = BTreeMap::new();
    let mut total_declared_inflated = 0_u64;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CodecError::Malformed(format!("bad ZIP entry {i}: {e}")))?;
        let name = file.name().to_string();
        let role = classify(&name);
        let method = file.compression();
        let compression = compression_label(method);
        let compressed_size = file.compressed_size();
        let uncompressed_size = file.size();
        if uncompressed_size > MAX_INFLATED_ENTRY_BYTES {
            return Err(CodecError::Malformed(format!(
                "ZIP entry {name} declares {uncompressed_size} inflated bytes, exceeding the \
                 {MAX_INFLATED_ENTRY_BYTES}-byte entry limit"
            )));
        }
        total_declared_inflated = total_declared_inflated
            .checked_add(uncompressed_size)
            .ok_or_else(|| CodecError::Malformed("F3D total inflated size overflows u64".into()))?;
        if total_declared_inflated > MAX_ARCHIVE_BYTES {
            return Err(CodecError::Malformed(format!(
                "F3D entries declare {total_declared_inflated} inflated bytes, exceeding the \
                 {MAX_ARCHIVE_BYTES}-byte archive limit"
            )));
        }
        let mut attributes = BTreeMap::new();

        let is_brep = role == role::BREP_SMBH || role == role::BREP_SMB;
        let view = admit_entry(ctx, root, &mut file, &name)?;
        drop(file);
        let buf = view.window();
        if is_brep {
            if asset_folder.is_none() {
                if let Some((folder, _)) = name.split_once("/Breps.BlobParts") {
                    asset_folder = Some(folder.to_string());
                }
            }
            let header = asm_header::parse(buf);
            let solved_record_limit = asm_header::solved_record_limit(buf);
            let sha = sha256_hex(buf);

            attributes.insert("asm_magic".to_string(), asm_magic_label(buf));
            if let Some(h) = &header {
                attributes.insert("asm_width".to_string(), h.width.to_string());
                if let Some(v) = h.save_format_version {
                    attributes.insert("acis_save_format_version".to_string(), v.to_string());
                }
                if let Some(v) = h.record_count {
                    attributes.insert("asm_record_count".to_string(), v.to_string());
                }
                if let Some(v) = h.entity_count {
                    attributes.insert("asm_entity_count".to_string(), v.to_string());
                }
                if let Some(v) = h.flags {
                    attributes.insert("asm_flags".to_string(), v.to_string());
                }
                if let Some(pf) = &h.product_family {
                    attributes.insert("product_family".to_string(), pf.clone());
                }
                if let Some(pv) = &h.product_version {
                    attributes.insert("product_version".to_string(), pv.clone());
                }
                if let Some(sd) = &h.save_date {
                    attributes.insert("save_date".to_string(), sd.clone());
                }
                if let Some(s) = h.scale {
                    attributes.insert("scale".to_string(), format!("{s}"));
                }
                if let Some(r) = h.linear {
                    attributes.insert("resabs".to_string(), format!("{r}"));
                }
                if let Some(r) = h.angular {
                    attributes.insert("resnor".to_string(), format!("{r}"));
                }
            }
            match solved_record_limit {
                Some(offset) => {
                    attributes.insert("history_partition_offset".to_string(), offset.to_string());
                    attributes.insert("solved_record_len".to_string(), offset.to_string());
                }
                None => {
                    attributes.insert("history_partition_offset".to_string(), "none".to_string());
                }
            }
            attributes.insert("sha256".to_string(), sha.clone());

            breps.push(BrepFacts {
                name: name.clone(),
                is_smbh: role == role::BREP_SMBH,
                uncompressed_len: uncompressed_size,
                header,
                solved_record_limit,
                sha256: sha,
            });
        }

        entries.push(ContainerEntry {
            name: name.clone(),
            role: role.to_string(),
            compression,
            compressed_size,
            uncompressed_size,
            attributes,
        });
        inflated_entries.insert(name, view);
    }

    Ok(ContainerScan {
        source_image,
        entries,
        breps,
        asset_folder,
        inflated_entries,
    })
}

/// Build a [`ContainerSummary`] without assigning model authority from a ZIP
/// extension. Design body bindings perform the model selection during decode.
pub fn summarize(scan: &ContainerScan<'_>) -> ContainerSummary {
    let mut notes = Vec::new();
    if let Some(folder) = &scan.asset_folder {
        notes.push(format!("asset folder (from entry paths): {folder}"));
    }
    notes.push(format!(
        "{} ASM BREP stream(s); Design body-to-blob bindings select model geometry",
        scan.breps.len()
    ));
    let history_count = history_breps(scan).count();
    match history_count {
        0 if !scan.breps.is_empty() => {
            notes.push("no BREP header declares a history partition".to_string());
        }
        1 => {
            let history = select_history_brep(scan)
                .expect("invariant: exactly one history-bearing BREP was counted");
            notes.push(format!(
                "history-bearing BREP: {} ({} bytes uncompressed)",
                history.name, history.uncompressed_len
            ));
        }
        count if count > 1 => notes.push(format!(
            "{count} history-bearing BREPs; each history graph is decoded independently"
        )),
        _ => {}
    }
    notes.push(
        "container-level inspection only; run `decode` to resolve Design body bindings and build \
         each referenced BREP graph"
            .to_string(),
    );

    ContainerSummary {
        format: "f3d".to_string(),
        container_kind: "zip".to_string(),
        entries: scan.entries.clone(),
        notes,
    }
}

/// Iterate over every BREP whose parsed header sets the history-partition bit.
/// The extension is not used as a semantic substitute for the header flag.
pub fn history_breps<'s>(scan: &'s ContainerScan<'_>) -> impl Iterator<Item = &'s BrepFacts> + 's {
    scan.breps.iter().filter(|brep| {
        brep.header
            .as_ref()
            .is_some_and(asm_header::AsmHeader::has_history_partition)
    })
}

/// Return the history-bearing BREP only when the header relation is unique.
pub fn select_history_brep<'s>(scan: &'s ContainerScan<'_>) -> Option<&'s BrepFacts> {
    let mut candidates = history_breps(scan);
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

/// Compatibility fallback used only when Design body-to-blob bindings are
/// absent. It returns a BREP only when the available evidence identifies one
/// unambiguously: exactly one history-bearing stream, or exactly one BREP in
/// total. Ambiguous archives do not acquire an archive-order guess.
pub fn select_fallback_brep<'s>(scan: &'s ContainerScan<'_>) -> Option<&'s BrepFacts> {
    if let Some(history) = select_history_brep(scan) {
        return Some(history);
    }
    match scan.breps.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

fn asm_magic_label(bytes: &[u8]) -> String {
    if asm_header::has_asm_magic(bytes) {
        // Both magics are the 15-byte prefix plus the width digit; byte 15 is
        // save-format-version data.
        String::from_utf8_lossy(&bytes[..15]).to_string()
    } else {
        "absent".to_string()
    }
}
