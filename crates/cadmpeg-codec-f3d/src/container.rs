// SPDX-License-Identifier: Apache-2.0
//! Scan and classify the ZIP container inside a `.f3d` file.
//!
//! [`scan`] retains the source archive, enumerates each entry, reads ASM headers
//! from `.smb` and `.smbh` B-rep streams, and locates their `delta_state`
//! history boundaries. [`select_active_brep`] chooses the `.smbh` stream when
//! present and otherwise uses the first `.smb` construction snapshot.
//! [`crate::decode`] passes the selected stream to the SAB and B-rep layers.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, SeekFrom};

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use sha2::{Digest, Sha256};
use zip::CompressionMethod;

use crate::asm_header;

/// Codec-defined role labels for [`ContainerEntry::role`].
pub mod role {
    /// The authoritative final-model ASM BREP stream.
    pub const BREP_SMBH: &str = "brep-smbh";
    /// An earlier construction-snapshot ASM BREP stream.
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
    /// Whether this is the `.smbh` authoritative stream.
    pub is_smbh: bool,
    /// Uncompressed byte length.
    pub uncompressed_len: u64,
    /// Parsed ASM header, if the magic was present.
    pub header: Option<asm_header::AsmHeader>,
    /// Offset of the first `delta_state` marker (active-slice boundary).
    pub delta_state_offset: Option<usize>,
    /// SHA-256 (lowercase hex) of the decompressed stream.
    pub sha256: String,
}

/// The full result of reading a `.f3d` container: the entry list plus decoded
/// BREP facts. Shared by `inspect` and `decode`.
pub struct ContainerScan {
    /// Complete source archive retained for byte-exact native replay.
    pub source_image: Vec<u8>,
    /// Enumerated entries with classification.
    pub entries: Vec<ContainerEntry>,
    /// Decoded BREP stream facts, in archive order.
    pub breps: Vec<BrepFacts>,
    /// The asset-folder prefix observed from BREP entry paths, if any.
    pub asset_folder: Option<String>,
}

/// Read and classify every entry, decoding ASM headers for BREP streams.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<ContainerScan, CodecError> {
    reader.seek(SeekFrom::Start(0))?;
    let mut source_image = Vec::new();
    reader.read_to_end(&mut source_image)?;
    reader.seek(SeekFrom::Start(0))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(&source_image))
        .map_err(|e| CodecError::Malformed(format!("not a readable ZIP: {e}")))?;

    let mut entries = Vec::with_capacity(archive.len());
    let mut breps = Vec::new();
    let mut asset_folder = None;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CodecError::Malformed(format!("bad ZIP entry {i}: {e}")))?;
        let name = file.name().to_string();
        let role = classify(&name);
        let mut attributes = BTreeMap::new();

        let is_brep = role == role::BREP_SMBH || role == role::BREP_SMB;
        if is_brep {
            if asset_folder.is_none() {
                if let Some((folder, _)) = name.split_once("/Breps.BlobParts") {
                    asset_folder = Some(folder.to_string());
                }
            }
            // Decompress and read the header fields.
            let mut buf = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut buf)
                .map_err(|e| CodecError::Malformed(format!("cannot read {name}: {e}")))?;

            let header = asm_header::parse(&buf);
            let delta = asm_header::first_delta_state_offset(&buf);
            let sha = hex_sha256(&buf);

            attributes.insert("asm_magic".to_string(), asm_magic_label(&buf));
            if let Some(h) = &header {
                attributes.insert("asm_width".to_string(), h.width.to_string());
                if let Some(v) = h.release {
                    attributes.insert("asm_release".to_string(), v.to_string());
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
            match delta {
                Some(off) => {
                    attributes.insert("delta_state_first_offset".to_string(), off.to_string());
                    attributes.insert("active_slice_len".to_string(), off.to_string());
                }
                None => {
                    attributes.insert("delta_state_first_offset".to_string(), "none".to_string());
                }
            }
            attributes.insert("sha256".to_string(), sha.clone());

            breps.push(BrepFacts {
                name: name.clone(),
                is_smbh: role == role::BREP_SMBH,
                uncompressed_len: file.size(),
                header,
                delta_state_offset: delta,
                sha256: sha,
            });
        }

        entries.push(ContainerEntry {
            name,
            role: role.to_string(),
            compression: compression_label(file.compression()),
            compressed_size: file.compressed_size(),
            uncompressed_size: file.size(),
            attributes,
        });
    }

    Ok(ContainerScan {
        source_image,
        entries,
        breps,
        asset_folder,
    })
}

/// Build a [`ContainerSummary`] with the active B-rep selection.
pub fn summarize(scan: &ContainerScan) -> ContainerSummary {
    let mut notes = Vec::new();
    if let Some(folder) = &scan.asset_folder {
        notes.push(format!("asset folder (from entry paths): {folder}"));
    }
    match select_active_brep(scan) {
        Some(b) => notes.push(format!(
            "active BREP candidate: {} ({} bytes uncompressed, {})",
            b.name,
            b.uncompressed_len,
            if b.is_smbh {
                "authoritative .smbh"
            } else {
                "no .smbh present; .smb is a construction snapshot"
            }
        )),
        None => notes.push("no ASM BREP stream found".to_string()),
    }
    notes.push(
        "container-level inspection only; run `decode` to build the B-rep graph and analytic \
         geometry from the active BREP's SAB record stream"
            .to_string(),
    );

    ContainerSummary {
        format: "f3d".to_string(),
        container_kind: "zip".to_string(),
        entries: scan.entries.clone(),
        notes,
    }
}

/// Decompress a named ZIP entry.
pub fn decompress_entry(reader: &mut dyn ReadSeek, name: &str) -> Result<Vec<u8>, CodecError> {
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(CodecError::Io)?;
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| CodecError::Malformed(format!("not a readable ZIP: {e}")))?;
    let mut file = archive
        .by_name(name)
        .map_err(|e| CodecError::Malformed(format!("entry {name} not found: {e}")))?;
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf)
        .map_err(|e| CodecError::Malformed(format!("cannot read {name}: {e}")))?;
    Ok(buf)
}

/// Select the first `.smbh` B-rep, falling back to the first `.smb`.
pub fn select_active_brep(scan: &ContainerScan) -> Option<&BrepFacts> {
    scan.breps
        .iter()
        .find(|b| b.is_smbh)
        .or_else(|| scan.breps.first())
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn asm_magic_label(bytes: &[u8]) -> String {
    if asm_header::has_asm_magic(bytes) {
        // Both magics are the 15-byte prefix plus the width digit; byte 15 is
        // release-word data.
        String::from_utf8_lossy(&bytes[..15]).to_string()
    } else {
        "absent".to_string()
    }
}
