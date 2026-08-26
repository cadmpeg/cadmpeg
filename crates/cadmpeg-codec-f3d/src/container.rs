// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Scan and classify Fusion `.f3d` and `.f3z` ZIP containers.
//!
//! [`scan`] retains the source archive, enumerates each entry, reads ASM headers
//! from `.smb` and `.smbh` B-rep streams, and locates their `delta_state`
//! history boundaries. Model geometry is selected from Design body-to-blob
//! bindings by [`crate::decode`]. [`select_history_brep`] independently locates
//! the stream whose header declares a history partition. When Design bindings
//! are absent, [`legacy_design_model_breps`] and [`select_fallback_brep`]
//! supply explicit compatibility fallbacks without asserting that one
//! extension is the document model.

use std::collections::BTreeMap;
use std::io::Read;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::hash::sha256_hex;

use cadmpeg_asm::asm_header;
use cadmpeg_asm::kernel_header::KernelHeader;

use crate::manifest;

/// Write-path local cap for nested Protein rewriting (`patch_protein_appearances`).
/// Decode opens nested archives through `ArchiveSnapshot` / `begin_expand`, so
/// session `ResourceLimits` bind there instead of these constants.
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
/// Write-path per-entry inflate cap for `read_entry_bounded`.
pub(crate) const MAX_INFLATED_ENTRY_BYTES: u64 = 128 * 1024 * 1024;

/// Codec-defined role labels for [`ContainerEntry::role`].
pub mod role {
    /// An ASM BREP entry with the `.smbh` extension. Its header normally
    /// declares a history partition.
    pub const BREP_SMBH: &str = "brep-smbh";
    /// An ASM BREP entry with the `.smb` extension. Its header normally omits
    /// the history partition.
    pub const BREP_SMB: &str = "brep-smb";
    /// An ASM BREP entry in the text encoding, with the `.sat` or `.smt`
    /// extension. It carries the same entity model as `.smb` and `.smbh` in a
    /// line-oriented ASCII form that ends with `End-of-ASM-data`. This role
    /// exists so that a document whose only geometry carrier is text is
    /// reported as a carrier that is present and not read, and not as a
    /// document with no carrier.
    pub const BREP_TEXT: &str = "brep-text";
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
    /// The `OGS.BlobFolder` display scene graph and its buffer arenas. The
    /// `world` member's drawable nodes carry the design entity ID they draw,
    /// `stream_mesh_NNN` and `Fusion_mesh_NNN` are the vertex and index
    /// buffer arenas that graph addresses by byte offset, its geometry is a
    /// tessellation of the B-rep streams, and its appearance bindings repeat
    /// the ACT and protein assets, so no carrier depends on it. See DR-28 for
    /// the one value class whose design source is unknown.
    pub const OGS_CACHE: &str = "ogs-cache";
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
        return Err(CodecError::malformed(format_args!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    let mut bytes = Vec::new();
    let mut limited = Read::take(entry, MAX_INFLATED_ENTRY_BYTES + 1);
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let read = limited.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        bytes.try_reserve(read).map_err(|_| {
            cadmpeg_core::decode::refuse_local_limit(
                "F3D entry allocation",
                MAX_INFLATED_ENTRY_BYTES,
                bytes.len().saturating_add(read) as u64,
                None,
            )
        })?;
        bytes.extend_from_slice(&chunk[..read]);
    }
    if bytes.len() as u64 > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::malformed(format_args!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    Ok(bytes)
}

/// Classify an entry by its name using the spec's naming families ([§1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#1-container-layer), [§6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#6-geometry-carriers)).
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
    } else if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sat") || ext.eq_ignore_ascii_case("smt"))
    {
        role::BREP_TEXT
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
    } else if name.contains("OGS.BlobFolder/") {
        role::OGS_CACHE
    } else {
        role::OTHER
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
    pub header: Option<KernelHeader>,
    /// Exact byte boundary between solved records and construction history.
    pub solved_record_limit: Option<usize>,
    /// SHA-256 (lowercase hex) of the decompressed stream.
    pub sha256: String,
}

/// The manifest-level kind of a scanned Fusion archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum F3dContainerKind {
    /// One F3D document whose manifests select one Design asset folder.
    Document {
        /// Exact archive folder of the Design asset.
        design_asset_folder: String,
    },
    /// An outer F3Z archive whose `.f3d` members each carry their own
    /// manifests and Design asset.
    MultiDocument,
}

/// The full result of reading a Fusion ZIP: the entry list plus decoded BREP
/// facts. Shared by `inspect` and `decode`.
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
    /// Whether this ZIP is one F3D document or an outer F3Z archive.
    pub kind: F3dContainerKind,
    /// Entry payload views, keyed by archive path.
    inflated_entries: BTreeMap<String, View<'a>>,
}

impl<'a> ContainerScan<'a> {
    /// Returns an entry payload retained during the single archive scan.
    pub fn entry_bytes(&self, name: &str) -> Result<&'a [u8], CodecError> {
        self.entry_view(name)
            .map(View::window)
            .ok_or_else(|| CodecError::malformed(format_args!("entry {name} not found")))
    }

    /// Returns an entry's payload view.
    pub(crate) fn entry_view(&self, name: &str) -> Option<View<'a>> {
        self.inflated_entries.get(name).copied()
    }

    /// Exact archive folder of the manifest-selected Design asset. An outer
    /// F3Z archive has no folder of its own; each member has one.
    pub fn design_asset_folder(&self) -> Option<&str> {
        match &self.kind {
            F3dContainerKind::Document {
                design_asset_folder,
            } => Some(design_asset_folder),
            F3dContainerKind::MultiDocument => None,
        }
    }

    /// Whether this is an outer multi-document F3Z archive.
    pub fn is_multi_document(&self) -> bool {
        matches!(self.kind, F3dContainerKind::MultiDocument)
    }

    /// Whether `name` is inside the manifest-selected Design asset folder.
    pub(crate) fn belongs_to_design_asset(&self, name: &str) -> bool {
        self.design_asset_folder().is_some_and(|folder| {
            name.strip_prefix(folder)
                .is_some_and(|suffix| suffix.starts_with('/'))
        })
    }

    /// Whether `entry` has `expected_role` inside the manifest-selected
    /// Design asset.
    pub(crate) fn is_design_asset_entry(
        &self,
        entry: &ContainerEntry,
        expected_role: &str,
    ) -> bool {
        entry.role == expected_role && self.belongs_to_design_asset(&entry.name)
    }

    /// Whether `entry` is a stream of `expected_role` in a Design segment of
    /// the manifest-selected Design asset.
    pub(crate) fn is_design_stream(&self, entry: &ContainerEntry, expected_role: &str) -> bool {
        if !self.is_design_asset_entry(entry, expected_role) {
            return false;
        }
        self.asset_segment(&entry.name).is_some_and(|segment| {
            segment == "Design1" || is_numbered_segment(segment, "FusionDesignSegmentType")
        })
    }

    /// Whether `entry` is a `BulkStream.dat` in an ACT segment of the
    /// manifest-selected Design asset.
    pub(crate) fn is_act_stream(&self, entry: &ContainerEntry) -> bool {
        self.is_design_asset_entry(entry, role::BULKSTREAM)
            && self
                .asset_segment(&entry.name)
                .is_some_and(|segment| is_numbered_segment(segment, "FusionACTSegmentType"))
    }

    fn asset_segment<'n>(&self, name: &'n str) -> Option<&'n str> {
        let relative = self
            .design_asset_folder()
            .and_then(|folder| name.strip_prefix(folder))?
            .strip_prefix('/')?;
        relative.split_once('/').map(|(segment, _)| segment)
    }
}

fn is_numbered_segment(segment: &str, prefix: &str) -> bool {
    segment.strip_prefix(prefix).is_some_and(|ordinal| {
        !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Read and classify every entry, decoding ASM headers for BREP streams.
///
/// Every entry is registered as a slice when stored or a decompressed space
/// when compressed.
pub fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<ContainerScan<'a>, CodecError> {
    let source_image = root.window();
    let archive = ArchiveSnapshot::new(root)?;

    let mut entries = Vec::new();
    let mut breps = Vec::new();
    let mut inflated_entries = BTreeMap::new();

    for file in archive.entries() {
        let name = file.name.clone();
        let role = classify(&name);
        let compression = file.compression.label().to_string();
        let compressed_size = file.compressed_size;
        let uncompressed_size = file.uncompressed_size;
        let mut attributes = BTreeMap::new();

        let is_brep = role == role::BREP_SMBH || role == role::BREP_SMB;
        let view = archive.open(ctx, file)?;
        let buf = view.window();
        if is_brep {
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

    let kind = if let Some(top_level_manifest) = inflated_entries.get("Manifest.dat") {
        let top_level_manifest = manifest::parse_top_level(top_level_manifest.window())?;
        let design_asset_folder = manifest::resolve_design_folder(
            &top_level_manifest,
            inflated_entries.keys().map(String::as_str),
            |name| inflated_entries.get(name).map(|view| view.window()),
        )?;
        F3dContainerKind::Document {
            design_asset_folder,
        }
    } else if inflated_entries.contains_key("Manifest.json")
        && inflated_entries.contains_key("DesignDescription.json")
        && inflated_entries
            .keys()
            .any(|name| !name.contains('/') && name.to_ascii_lowercase().ends_with(".f3d"))
    {
        F3dContainerKind::MultiDocument
    } else {
        return Err(CodecError::Malformed(
            "Fusion ZIP has neither a top-level Manifest.dat nor the F3Z manifest set".into(),
        ));
    };

    Ok(ContainerScan {
        source_image,
        entries,
        breps,
        kind,
        inflated_entries,
    })
}

/// Build a [`ContainerSummary`] without assigning model authority from a ZIP
/// extension. Design body bindings perform the model selection during decode.
pub fn summarize(scan: &ContainerScan<'_>) -> ContainerSummary {
    let mut notes = Vec::new();
    if let Some(folder) = scan.design_asset_folder() {
        notes.push(format!("Design asset folder (from manifests): {folder}"));
    } else {
        notes.push("outer F3Z archive; each F3D member selects its own Design asset".into());
    }
    let design_brep_count = design_breps(scan).count();
    notes.push(format!(
        "{design_brep_count} ASM BREP stream(s); Design body-to-blob bindings select model geometry"
    ));
    if design_brep_count != scan.breps.len() {
        notes.push(format!(
            "{} ASM BREP stream(s) belong to non-Design assets",
            scan.breps.len() - design_brep_count
        ));
    }
    let history_count = history_breps(scan).count();
    match history_count {
        0 if design_brep_count != 0 => {
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
    design_breps(scan).filter(|brep| {
        brep.header
            .as_ref()
            .is_some_and(KernelHeader::has_history_partition)
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
    let mut candidates = design_breps(scan);
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

/// Iterate over binary ASM BREP entries inside the manifest-selected Design
/// asset.
pub fn design_breps<'s>(scan: &'s ContainerScan<'_>) -> impl Iterator<Item = &'s BrepFacts> + 's {
    scan.breps
        .iter()
        .filter(|brep| scan.belongs_to_design_asset(&brep.name))
}

/// Names of the text-encoded ASM BREP entries, in archive order.
///
/// These entries stay out of [`ContainerScan::breps`] because that set holds the
/// streams whose binary ASM header decoded, and the text encoding has no such
/// header. A caller that reports on geometry must still count them: a document
/// whose only carrier is text has a carrier that is present and not read, which
/// is a different finding from a document that declares no carrier.
pub fn text_brep_names<'s>(scan: &'s ContainerScan<'_>) -> Vec<&'s str> {
    scan.entries
        .iter()
        .filter(|entry| entry.role == role::BREP_TEXT && scan.belongs_to_design_asset(&entry.name))
        .map(|entry| entry.name.as_str())
        .collect()
}

/// Return the complete BREP set for the legacy `Design1` segment layout.
///
/// That layout predates body-to-blob bindings: its model is distributed across
/// the archive's BREP entries, in archive order. Both design streams must be
/// present so an unrelated path component named `Design1` cannot select this
/// fallback.
pub fn legacy_design_model_breps<'s>(scan: &'s ContainerScan<'_>) -> Option<Vec<&'s BrepFacts>> {
    let has = |leaf: &str| {
        scan.design_asset_folder().is_some_and(|folder| {
            scan.entries
                .iter()
                .any(|entry| entry.name == format!("{folder}/Design1/{leaf}"))
        })
    };
    let breps = design_breps(scan).collect::<Vec<_>>();
    (has("BulkStream.dat") && has("MetaStream.dat") && !breps.is_empty()).then_some(breps)
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
