// SPDX-License-Identifier: Apache-2.0
//! Scan and classify the ZIP container inside a `.f3d` file.
//!
//! [`scan`] retains the source archive, enumerates each entry, reads ASM headers
//! from `.smb` and `.smbh` B-rep streams, and locates their `delta_state`
//! history boundaries. [`select_active_brep`] chooses the `.smbh` history
//! stream when present and otherwise uses the first `.smb` stream. Design body
//! maps independently select the B-rep blobs that compose the document model.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, SeekFrom};

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use cadmpeg_ir::hash::sha256_hex;

use crate::bytes::{is_guid_hyphenated, lp_utf16_bounded};
use zip::CompressionMethod;

use crate::asm_header;

/// Maximum compressed `.f3d` archive size accepted by the in-memory codec.
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
/// Maximum inflated size accepted for one top-level or nested ZIP entry.
pub(crate) const MAX_INFLATED_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn read_entry_bounded(
    reader: &mut impl Read,
    declared_size: u64,
    name: &str,
) -> Result<Vec<u8>, CodecError> {
    if declared_size > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::Malformed(format!(
            "ZIP entry {name} declares {declared_size} inflated bytes; limit is {MAX_INFLATED_ENTRY_BYTES}"
        )));
    }
    let capacity = usize::try_from(declared_size.min(8 * 1024 * 1024)).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    reader
        .take(MAX_INFLATED_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CodecError::Malformed(format!("cannot read {name}: {error}")))?;
    if bytes.len() as u64 > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::Malformed(format!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    Ok(bytes)
}

/// Codec-defined role labels for [`ContainerEntry::role`].
pub mod role {
    /// An ASM BREP stream with a history partition.
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
    /// A T-spline Form control-cage source.
    pub const TSPLINE: &str = "tspline";
    /// Secondary tessellated mesh data (`.paramesh`), not the exact source.
    pub const PARAMESH: &str = "paramesh";
    /// An empty/placeholder design-configuration entry.
    pub const DESIGN_CONFIG: &str = "design-config";
    /// The top-level document-properties slot: empty, or a JSON `docstruct`
    /// document-type declaration.
    pub const PROPERTIES: &str = "properties";
    /// The top-level external-reference table (`RedirectionsStream.dat`).
    pub const REDIRECTIONS: &str = "redirections";
    /// The top-level component-reference slot (`ComponentReferenceData.json`).
    pub const COMPONENT_REFERENCES: &str = "component-reference-data";
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

/// The `.f3z` marker substrings: an archive-level JSON member name or a
/// `.f3d` document member name in a ZIP local file header.
pub const F3Z_DETECT_MARKERS: &[&[u8]] = &[b"DesignDescription.json", b".f3d"];

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
    } else if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("tsm"))
        && name.contains("TSplines.BlobParts/")
    {
        role::TSPLINE
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
    } else if base == "RedirectionsStream.dat" {
        role::REDIRECTIONS
    } else if base == "ComponentReferenceData.json" {
        role::COMPONENT_REFERENCES
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
    /// Whether this is a `.smbh` history stream.
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
    /// Decompressed entry payloads, keyed by archive path.
    inflated_entries: BTreeMap<String, Vec<u8>>,
}

impl ContainerScan {
    /// Returns a decompressed entry retained during the single archive scan.
    pub fn entry_bytes(&self, name: &str) -> Result<&[u8], CodecError> {
        self.inflated_entries
            .get(name)
            .map(Vec::as_slice)
            .ok_or_else(|| CodecError::Malformed(format!("entry {name} not found")))
    }
}

/// Read and classify every entry, decoding ASM headers for BREP streams.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<ContainerScan, CodecError> {
    reader.seek(SeekFrom::Start(0))?;
    let mut source_image = Vec::new();
    Read::take(&mut *reader, MAX_ARCHIVE_BYTES + 1).read_to_end(&mut source_image)?;
    if source_image.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(CodecError::Malformed(format!(
            "F3D archive exceeds the {MAX_ARCHIVE_BYTES}-byte input limit"
        )));
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(&source_image))
        .map_err(|e| CodecError::Malformed(format!("not a readable ZIP: {e}")))?;

    let mut entries = Vec::with_capacity(archive.len());
    let mut breps = Vec::new();
    let mut asset_folder = None;
    let mut inflated_entries = BTreeMap::new();
    let mut total_declared_inflated = 0_u64;
    let mut total_actual_inflated = 0_u64;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CodecError::Malformed(format!("bad ZIP entry {i}: {e}")))?;
        let name = file.name().to_string();
        let role = classify(&name);
        let mut attributes = BTreeMap::new();

        let is_brep = role == role::BREP_SMBH || role == role::BREP_SMB;
        total_declared_inflated = total_declared_inflated
            .checked_add(file.size())
            .ok_or_else(|| CodecError::Malformed("F3D total inflated size overflows u64".into()))?;
        if total_declared_inflated > MAX_ARCHIVE_BYTES {
            return Err(CodecError::Malformed(format!(
                "F3D entries declare {total_declared_inflated} inflated bytes; total limit is {MAX_ARCHIVE_BYTES}"
            )));
        }
        let declared_size = file.size();
        let buf = read_entry_bounded(&mut file, declared_size, &name)?;
        total_actual_inflated = total_actual_inflated
            .checked_add(buf.len() as u64)
            .ok_or_else(|| CodecError::Malformed("F3D total inflated size overflows u64".into()))?;
        if total_actual_inflated > MAX_ARCHIVE_BYTES {
            return Err(CodecError::Malformed(format!(
                "F3D entries exceed the {MAX_ARCHIVE_BYTES}-byte total inflated limit"
            )));
        }
        if is_brep {
            if asset_folder.is_none() {
                if let Some((folder, _)) = name.split_once("/Breps.BlobParts") {
                    asset_folder = Some(folder.to_string());
                }
            }
            // Decompress and read the header fields.
            let header = asm_header::parse(&buf);
            let delta = asm_header::first_delta_state_offset(&buf);
            let sha = sha256_hex(&buf);

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
            name: name.clone(),
            role: role.to_string(),
            compression: compression_label(file.compression()),
            compressed_size: file.compressed_size(),
            uncompressed_size: file.size(),
            attributes,
        });
        inflated_entries.insert(name, buf);
    }

    let asset_folder = manifest_asset_folder(&inflated_entries).or(asset_folder);
    Ok(ContainerScan {
        source_image,
        entries,
        breps,
        asset_folder,
        inflated_entries,
    })
}

fn manifest_asset_folder(entries: &BTreeMap<String, Vec<u8>>) -> Option<String> {
    let top = entries.get("Manifest.dat")?;
    let folders = entries
        .keys()
        .filter_map(|name| name.strip_suffix("/Manifest.dat"))
        .filter(|name| !name.contains('/'))
        .collect::<Vec<_>>();
    if folders.is_empty() {
        return None;
    }
    let (folder_run, manifest_uuid) = counted_folder_run(top, &folders)?;
    let matches = folder_run
        .into_iter()
        .filter(|folder| {
            entries
                .get(&format!("{folder}/Manifest.dat"))
                .and_then(|manifest| first_asset_manifest_uuid(manifest))
                .is_some_and(|uuid| uuid == manifest_uuid)
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0].clone())
}

fn counted_folder_run(bytes: &[u8], folders: &[&str]) -> Option<(Vec<String>, String)> {
    let mut resolved = None;
    for offset in 0..bytes.len().saturating_sub(4) {
        let count = u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize;
        if count == 0 || count > folders.len() {
            continue;
        }
        let mut cursor = offset + 4;
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            let Some((name, after)) = lp_utf16_bounded(bytes, cursor, 0..=usize::MAX) else {
                names.clear();
                break;
            };
            if !folders.contains(&name.as_str()) {
                names.clear();
                break;
            }
            names.push(name);
            cursor = after;
        }
        if names.len() != count {
            continue;
        }
        let Some(uuid) = lp_utf16_ending_at(bytes, offset) else {
            continue;
        };
        if !is_guid_hyphenated(&uuid) || resolved.is_some() {
            return None;
        }
        resolved = Some((names, uuid));
    }
    resolved
}

fn first_asset_manifest_uuid(bytes: &[u8]) -> Option<String> {
    let (_, cursor) = lp_utf16_bounded(bytes, 0, 0..=usize::MAX)?;
    let (uuid, _) = lp_utf16_bounded(bytes, cursor, 0..=usize::MAX)?;
    is_guid_hyphenated(&uuid).then_some(uuid)
}

fn lp_utf16_ending_at(bytes: &[u8], end: usize) -> Option<String> {
    (0..end)
        .rev()
        .find_map(|start| {
            lp_utf16_bounded(bytes, start, 0..=usize::MAX).filter(|(_, after)| *after == end)
        })
        .map(|(value, _)| value)
}

/// Build a [`ContainerSummary`] with the active history-stream selection.
pub fn summarize(scan: &ContainerScan) -> ContainerSummary {
    let mut notes = Vec::new();
    if let Some(folder) = &scan.asset_folder {
        notes.push(format!("active asset folder: {folder}"));
    }
    match select_active_brep(scan) {
        Some(b) => notes.push(format!(
            "active BREP history candidate: {} ({} bytes uncompressed, {})",
            b.name,
            b.uncompressed_len,
            if b.is_smbh {
                ".smbh history stream"
            } else {
                "no .smbh present; .smb is a construction snapshot"
            }
        )),
        None => notes.push("no ASM BREP stream found".to_string()),
    }
    notes.push(
        "container-level inspection only; run `decode` to build B-rep graphs and analytic \
         geometry from the Design-referenced SAB record streams"
            .to_string(),
    );

    ContainerSummary {
        format: "f3d".to_string(),
        container_kind: "zip".to_string(),
        entries: scan.entries.clone(),
        notes,
    }
}

/// Select the first `.smbh` B-rep in the active asset folder, falling back to
/// the first `.smb` there. Without an asset folder, use archive-wide order.
pub fn select_active_brep(scan: &ContainerScan) -> Option<&BrepFacts> {
    if let Some(folder) = &scan.asset_folder {
        let prefix = format!("{folder}/");
        let mut scoped = scan
            .breps
            .iter()
            .filter(|brep| brep.name.starts_with(&prefix));
        return scoped
            .clone()
            .find(|brep| brep.is_smbh)
            .or_else(|| scoped.next());
    }
    scan.breps
        .iter()
        .find(|brep| brep.is_smbh)
        .or_else(|| scan.breps.first())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lp_utf16(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        for unit in value.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn brep(name: &str, is_smbh: bool) -> BrepFacts {
        BrepFacts {
            name: name.into(),
            is_smbh,
            uncompressed_len: 0,
            header: None,
            delta_state_offset: None,
            sha256: String::new(),
        }
    }

    #[test]
    fn manifest_uuid_selects_design_folder_and_scopes_brep() {
        const DESIGN_UUID: &str = "11111111-2222-3333-4444-555555555555";
        const OTHER_UUID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let mut top = Vec::new();
        lp_utf16(&mut top, DESIGN_UUID);
        top.extend_from_slice(&2_u32.to_le_bytes());
        lp_utf16(&mut top, "DesignAsset");
        lp_utf16(&mut top, "OtherAsset");
        let mut design = Vec::new();
        lp_utf16(&mut design, "DesignAsset");
        lp_utf16(&mut design, DESIGN_UUID);
        let mut other = Vec::new();
        lp_utf16(&mut other, "OtherAsset");
        lp_utf16(&mut other, OTHER_UUID);
        let inflated_entries = BTreeMap::from([
            ("Manifest.dat".into(), top),
            ("DesignAsset/Manifest.dat".into(), design),
            ("OtherAsset/Manifest.dat".into(), other),
        ]);

        assert_eq!(
            manifest_asset_folder(&inflated_entries).as_deref(),
            Some("DesignAsset")
        );

        let mut scan = ContainerScan {
            source_image: Vec::new(),
            entries: Vec::new(),
            breps: vec![
                brep("OtherAsset/Breps.BlobParts/other.smbh", true),
                brep("DesignAsset/Breps.BlobParts/design.smb", false),
            ],
            asset_folder: Some("DesignAsset".into()),
            inflated_entries,
        };
        assert_eq!(
            select_active_brep(&scan).map(|brep| brep.name.as_str()),
            Some("DesignAsset/Breps.BlobParts/design.smb")
        );
        scan.asset_folder = Some("NoGeometryAsset".into());
        assert!(select_active_brep(&scan).is_none());
    }
}
