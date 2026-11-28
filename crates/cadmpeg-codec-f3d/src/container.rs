// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Scan and classify Fusion `.f3d` and `.f3z` ZIP containers.
//!
//! [`scan`] retains the source archive, enumerates each entry, reads ASM headers
//! from `.smb` and `.smbh` B-rep streams, and locates their `delta_state`
//! history boundaries. Model geometry is selected from Design body-to-blob
//! bindings by [`crate::decode`]. [`select_history_brep`] independently locates
//! the stream whose header declares a history partition. Compatibility helpers
//! expose unique or legacy carrier sets for metadata reporting; model decode
//! uses the typed Design body-map catalog.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use std::collections::BTreeMap;
use std::io::Read;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{
    ArenaClass, DecodeArena, DecodeCheckpoint, DecodeContext, LimitScope, ResourceLimit, View,
};
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::unknown::part::{Cause, DecodeParts, GivenUpPart, Part};
use cadmpeg_ir::ContainerSummary;
use cadmpeg_ir::StreamName;
use std::sync::Arc;

use cadmpeg_asm::kernel_header::{BinaryHeader, KernelHeader};
use cadmpeg_asm::{acis_header, asm_header};

use crate::dialect::F3dDialect;
use crate::manifest;

/// Write-path local cap for nested Protein rewriting (`patch_protein_appearances`).
/// Decode opens nested archives through `ArchiveSnapshot` / `begin_expand`, so
/// session `ResourceLimits` bind there instead of these constants.
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
/// Write-path per-entry inflate cap for `read_entry_bounded`.
pub(crate) const MAX_INFLATED_ENTRY_BYTES: u64 = 128 * 1024 * 1024;

/// The f3d marker substrings used for confident detection from a byte prefix
/// (ZIP local file headers store entry names in cleartext near the start).
pub(crate) const DETECT_MARKERS: &[&[u8]] = &[
    b"Breps.BlobParts",
    b"FusionAssetName",
    b"FusionDocType",
    b".smbh",
];
/// The kind of the part an archive entry gives up as.
pub(crate) const ENTRY_PART: &str = "archive-entry";

/// Marker names that distinguish a multi-document F3Z archive from a generic ZIP.
pub(crate) const F3Z_DETECT_MARKERS: &[&[u8]] =
    &[b"Manifest.json", b"DesignDescription.json", b".f3d"];

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
        bytes.extend_from_slice(&chunk[..read]);
    }
    if u64_from_index(bytes.len()) > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::malformed(format_args!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    Ok(bytes)
}

/// Classify an entry by its name using the spec's naming families ([§1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#1-container-layer), [§6](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#6-geometry-carriers)).
pub(crate) fn classify(name: &str) -> ContainerRole {
    if name.ends_with('/') {
        return ContainerRole::Directory;
    }
    let base = name.rsplit('/').next().unwrap_or(name);
    if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("smbh"))
    {
        ContainerRole::BrepSmbh
    } else if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("smb"))
    {
        ContainerRole::BrepSmb
    } else if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sat") || ext.eq_ignore_ascii_case("smt"))
    {
        ContainerRole::BrepText
    } else if name.ends_with(".protein") {
        ContainerRole::ProteinAssets
    } else if name.ends_with(".paramesh") {
        ContainerRole::Paramesh
    } else if name.ends_with(".dsgcfg") || name.ends_with(".dsgcfgrule") {
        ContainerRole::DesignConfig
    } else if base == "Manifest.dat" {
        ContainerRole::Manifest
    } else if base == "MetaStream.dat" {
        ContainerRole::Metastream
    } else if base == "BulkStream.dat" {
        ContainerRole::Bulkstream
    } else if base == "Properties.dat" {
        ContainerRole::Properties
    } else if name.contains("Previews/") {
        ContainerRole::Preview
    } else if name.contains("Images.BlobParts") {
        ContainerRole::Image
    } else if name.contains("OGS.BlobFolder/") {
        ContainerRole::OgsCache
    } else {
        ContainerRole::Other
    }
}

/// The content classes an entry of `role` can hold.
fn held_classes(role: ContainerRole) -> &'static [ArenaClass] {
    match role {
        ContainerRole::Directory => &[],
        ContainerRole::DesignConfig => &[ArenaClass::Design],
        ContainerRole::Metastream | ContainerRole::Bulkstream => {
            &[ArenaClass::Model, ArenaClass::Design]
        }
        ContainerRole::Other => &ArenaClass::ALL,
        _ => &[ArenaClass::Model],
    }
}

/// Owned kernel framing for one BREP stream.
#[derive(Debug, Clone)]
pub(crate) enum KernelFraming {
    /// Autodesk Shape Manager binary framing.
    Asm {
        /// Parsed ASM header.
        header: BinaryHeader,
        /// Exact byte boundary between solved records and construction history.
        solved_record_limit: Option<usize>,
    },
    /// Spatial ACIS binary framing.
    Acis(BinaryHeader),
    /// Text kernel framing without a binary reference width.
    Text {
        /// Encoding-independent header metadata.
        header: KernelHeader,
        /// Family selected by the text terminator.
        terminator: cadmpeg_asm::sat::Terminator,
    },
}

impl KernelFraming {
    /// Borrow this owned framing as the shared dialect-classification input.
    pub(crate) fn as_header_ref(&self) -> cadmpeg_asm::dialect::KernelHeaderRef<'_> {
        match self {
            Self::Asm { header, .. } => cadmpeg_asm::dialect::KernelHeaderRef::Asm(header),
            Self::Acis(header) => cadmpeg_asm::dialect::KernelHeaderRef::Acis(header),
            Self::Text { header, terminator } => match terminator {
                cadmpeg_asm::sat::Terminator::Asm => {
                    cadmpeg_asm::dialect::KernelHeaderRef::TextAsm(header)
                }
                cadmpeg_asm::sat::Terminator::Acis => {
                    cadmpeg_asm::dialect::KernelHeaderRef::TextAcis(header)
                }
            },
        }
    }

    /// The ASM solved-record boundary, when present.
    pub(crate) fn solved_record_limit(&self) -> Option<usize> {
        match self {
            Self::Asm {
                solved_record_limit,
                ..
            } => *solved_record_limit,
            Self::Acis(_) | Self::Text { .. } => None,
        }
    }

    /// Metadata retained for ASM binary and text model reports.
    pub(crate) fn model_metadata(&self) -> Option<&KernelHeader> {
        match self {
            Self::Asm { header, .. } => Some(&header.metadata),
            Self::Text { header, .. } => Some(header),
            Self::Acis(_) => None,
        }
    }

    /// Return the header only when ASM framing owns it.
    pub(crate) fn asm_header(&self) -> Option<&BinaryHeader> {
        match self {
            Self::Asm { header, .. } => Some(header),
            Self::Acis(_) | Self::Text { .. } => None,
        }
    }
}

/// One decoded BREP stream's header facts, kept for the summary and decode
/// metadata.
#[derive(Debug, Clone)]
pub(crate) struct BrepFacts {
    /// Entry name.
    pub(crate) name: String,
    /// Uncompressed byte length.
    pub(crate) uncompressed_len: u64,
    /// Parsed ASM or ACIS framing, when either header matched.
    pub(crate) kernel: Option<KernelFraming>,
    /// SHA-256 (lowercase hex) of the decompressed stream.
    pub(crate) sha256: Sha256Digest,
}

/// The manifest-level kind of a scanned Fusion archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum F3dContainerKind {
    /// One F3D document whose manifests select one Design asset folder.
    Document {
        /// Exact archive folder of the Design asset.
        design_asset_folder: String,
        /// Primary GUID of the Design asset's per-asset manifest.
        design_asset_guid: String,
        /// Dialect classified from the document manifest.
        matched: DialectMatch,
    },
    /// An outer F3Z archive whose `.f3d` members each carry their own
    /// manifests and Design asset.
    MultiDocument {
        /// Dialect classified from the outer archive manifests.
        matched: DialectMatch,
    },
}

impl F3dContainerKind {
    /// Primary dialect selected by this container kind's discriminants.
    pub(crate) fn dialect(&self) -> &DialectMatch {
        match self {
            Self::Document { matched, .. } | Self::MultiDocument { matched } => matched,
        }
    }
}

/// The full result of reading a Fusion ZIP: the entry list plus decoded BREP
/// facts. Shared by `inspect` and `decode`.
///
/// The `'a` lifetime is the session's root address space: stored entries are
/// views borrowing the root without copying, and compressed entries are arena-backed views the
/// platform expander produced, so both live for the decode's duration.
pub(crate) struct ContainerScan<'a> {
    /// Complete source archive retained for byte-exact native replay.
    pub(crate) source_image: &'a [u8],
    /// Enumerated entries with classification.
    pub(crate) entries: Vec<ContainerEntry>,
    /// Directory lengths retained for archive-session member listing.
    pub(crate) archive_member_sizes: Vec<u64>,
    /// Decoded BREP stream facts, in archive order.
    pub(crate) breps: Vec<BrepFacts>,
    /// Each text B-rep's one admitted parse, shared by model and report paths.
    pub(crate) text_breps: std::collections::HashMap<String, TextBrepFraming>,
    /// Whether this ZIP is one F3D document or an outer F3Z archive.
    pub(crate) kind: F3dContainerKind,
    /// Optional manifest payloads rejected after their byte spans were admitted.
    pub(crate) manifest_diagnostics: Vec<manifest::MetadataDiagnostic>,
    pub(crate) manifest_reference_extension: Option<View<'a>>,
    pub(crate) manifest_lineage_urn: Option<String>,
    /// Optional archive entries the scan could not open.
    pub(crate) given_up: Vec<GivenUpPart>,
    /// The stream of the outer document that holds this archive, for a
    /// scanned F3Z member.
    stream: Option<StreamName>,
    /// The stored byte range of each entry in `source_image`, by archive path.
    stored_ranges: BTreeMap<String, cadmpeg_core::decode::ByteRange>,
    /// Design `BulkStream` record runs after the last indexed record, by
    /// entry name, with the stream's stored length. Their entry views end
    /// where the run starts; complete stored bytes remain in `source_image`.
    pub(crate) unindexed_record_runs: Vec<(
        String,
        crate::design::decode::meta::UnindexedRecordRun,
        usize,
    )>,
    /// Entry payload views, keyed by archive path.
    inflated_entries: BTreeMap<String, View<'a>>,
    archive: Option<Arc<ArchiveSnapshot<'a>>>,
    working_bytes: Option<u64>,
    deferred_members: BTreeMap<String, DecodeCheckpoint>,
    /// Entry indices per native scope key, in entry order.
    scope_entry_indices: std::collections::HashMap<String, Vec<usize>>,
    /// Parsed `MetaStream` entries, memoized for the scan's duration.
    metastream_cache: std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Arc<crate::metastream::MetaStream>>,
    >,
    /// Indexed `BulkStream` record offsets, memoized per entry for the scan's duration.
    record_offsets_cache: std::sync::Mutex<
        std::collections::HashMap<
            String,
            std::sync::Arc<crate::design::decode::sketch::IndexedRecordOffsets>,
        >,
    >,
    /// Resolved primary frames per `BulkStream`, memoized for the scan's duration.
    primary_frames_cache: std::sync::Mutex<
        std::collections::HashMap<
            String,
            Option<std::sync::Arc<[crate::design::decode::meta::CachedDesignPrimaryFrame]>>,
        >,
    >,
}

/// The framing outcome of one text B-rep member.
#[derive(Clone)]
pub(crate) enum TextBrepFraming {
    Parsed(Box<cadmpeg_asm::sat::TextStream>),
    Unframed(cadmpeg_asm::stream_error::StreamError),
    Malformed(cadmpeg_asm::stream_error::StreamError),
    UnsupportedLength(cadmpeg_asm::stream_error::StreamError),
}

impl<'a> ContainerScan<'a> {
    pub(crate) fn fork(&self) -> Self {
        Self {
            source_image: self.source_image,
            entries: self.entries.clone(),
            archive_member_sizes: self.archive_member_sizes.clone(),
            breps: self.breps.clone(),
            text_breps: self.text_breps.clone(),
            kind: self.kind.clone(),
            manifest_diagnostics: self.manifest_diagnostics.clone(),
            manifest_reference_extension: self.manifest_reference_extension,
            manifest_lineage_urn: self.manifest_lineage_urn.clone(),
            given_up: self.given_up.clone(),
            stream: self.stream.clone(),
            stored_ranges: self.stored_ranges.clone(),
            unindexed_record_runs: self.unindexed_record_runs.clone(),
            inflated_entries: self.inflated_entries.clone(),
            archive: self.archive.clone(),
            working_bytes: self.working_bytes,
            deferred_members: self.deferred_members.clone(),
            scope_entry_indices: self.scope_entry_indices.clone(),
            metastream_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            record_offsets_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            primary_frames_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Returns an entry payload retained during the single archive scan.
    pub(crate) fn entry_bytes(&self, name: &str) -> Result<&'a [u8], CodecError> {
        self.entry_view(name)
            .map(View::window)
            .ok_or_else(|| CodecError::malformed(format_args!("entry {name} not found")))
    }

    /// Returns an entry's payload view.
    pub(crate) fn entry_view(&self, name: &str) -> Option<View<'a>> {
        self.inflated_entries.get(name).copied()
    }

    #[cfg(test)]
    pub(crate) fn retains_entry_payload(&self, name: &str) -> bool {
        self.inflated_entries.contains_key(name)
    }

    #[cfg(test)]
    pub(crate) fn archive_owner_count(&self) -> usize {
        self.archive.as_ref().map_or(0, Arc::strong_count)
    }

    pub(crate) fn declared_working_bytes(&self) -> Option<u64> {
        self.working_bytes
    }

    pub(crate) fn has_entry(&self, name: &str) -> bool {
        self.inflated_entries.contains_key(name) || self.deferred_members.contains_key(name)
    }

    /// Gives up the archive entry `name`, whose payload opened and then
    /// failed to decode with `error` for `cause`, as an `archive-entry` part
    /// holding content of the classes its role holds. The source image keeps the
    /// entry's stored bytes, so the part keeps their digest. `None` when the
    /// scan did not open the entry: the scan gave that entry up itself.
    ///
    /// # Errors
    ///
    /// `error` itself when it is fatal to the session, or a refusal of the
    /// part helper.
    pub(crate) fn give_up_entry(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
        cause: Cause,
        error: CodecError,
    ) -> Result<Option<GivenUpPart>, CodecError> {
        let classes = self
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .map_or(&ArenaClass::ALL[..], |entry| held_classes(entry.role));
        self.give_up_entry_holding(ctx, name, classes, cause, error)
    }

    /// [`Self::give_up_entry`] for a part holding content of the classes in
    /// `holds`.
    ///
    /// # Errors
    ///
    /// As [`Self::give_up_entry`].
    pub(crate) fn give_up_entry_holding(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
        holds: &[ArenaClass],
        cause: Cause,
        error: CodecError,
    ) -> Result<Option<GivenUpPart>, CodecError> {
        if !self.has_entry(name) {
            return Ok(None);
        }
        let stored = self
            .stored_ranges
            .get(name)
            .and_then(|range| {
                let start = usize::try_from(range.start).ok()?;
                let end = usize::try_from(range.end).ok()?;
                View::over_retained(self.source_image).child(start, end)
            })
            .ok_or_else(|| {
                CodecError::malformed(format_args!("F3D entry {name} has no stored span"))
            })?;
        let mut part = Part::new("f3d", ENTRY_PART, stored, holds.iter().copied())
            .named(name)
            .digest_only();
        if let Some(stream) = &self.stream {
            part = part.in_stream(stream.clone());
        }
        ctx.give_up_part(part, cause, error).map(Some)
    }

    /// The stream name of the archive entry `name`: the entry path, inside
    /// the outer document's stream for a scanned F3Z member.
    ///
    /// # Errors
    ///
    /// [`CodecError::Malformed`] when the path forms no stream name.
    pub(crate) fn entry_stream(&self, name: &str) -> Result<StreamName, CodecError> {
        let path = match &self.stream {
            Some(outer) => format!("{}/{name}", outer.as_str()),
            None => name.to_owned(),
        };
        StreamName::try_from(path).map_err(CodecError::malformed)
    }

    /// Gives up bytes `span` of the opened archive entry `entry` for the
    /// cause and error of `why` as a part of `kind` in the entry's stream, holding content of
    /// the classes in `holds`. The inflated entry bytes are retained in no other record, so
    /// the part keeps them.
    ///
    /// # Errors
    ///
    /// [`CodecError::Malformed`] when the scan holds no such entry or
    /// `span` leaves it, `error` itself when it is fatal to the session, or
    /// a refusal of the part helper.
    pub(crate) fn give_up_entry_span(
        &self,
        ctx: &DecodeContext<'_>,
        entry: &str,
        kind: &'static str,
        span: std::ops::Range<usize>,
        holds: impl IntoIterator<Item = ArenaClass>,
        (cause, error): (Cause, CodecError),
    ) -> Result<GivenUpPart, CodecError> {
        let bytes = self.entry_bytes(entry)?.get(span.clone()).ok_or_else(|| {
            CodecError::malformed(format_args!("F3D {kind} part leaves the entry {entry}"))
        })?;
        let part = Part::at("f3d", kind, u64_from_index(span.start), bytes, holds)
            .in_stream(self.entry_stream(entry)?);
        ctx.give_up_part(part, cause, error)
    }

    pub(crate) fn member_view<'b>(
        &'b self,
        arena: &'b DecodeArena,
        name: &str,
    ) -> Result<View<'b>, CodecError> {
        if let Some(checkpoint) = self.deferred_members.get(name) {
            let local = checkpoint.resume(arena);
            local.take_trace();
            let archive: &ArchiveSnapshot<'b> = self
                .archive
                .as_ref()
                .ok_or_else(|| CodecError::malformed("F3Z member directory is absent"))?;
            archive.open(&local, name)
        } else {
            self.entry_view(name)
                .ok_or_else(|| CodecError::malformed(format_args!("entry {name} not found")))
        }
    }

    /// The first entry, in entry order, whose native scope key is `scope` and
    /// which is a Design stream of `expected_role`. Equivalent to a linear
    /// `find` over `entries` with both predicates.
    pub(crate) fn design_stream_entry_for_scope(
        &self,
        expected_role: ContainerRole,
        scope: &str,
    ) -> Option<&ContainerEntry> {
        self.scope_entry_indices
            .get(scope)?
            .iter()
            .filter_map(|index| self.entries.get(*index))
            .find(|entry| self.is_design_stream(entry, expected_role))
    }

    /// The parse of one `MetaStream` entry, computed at most once per scan.
    pub(crate) fn parsed_metastream(
        &self,
        name: &str,
    ) -> Result<std::sync::Arc<crate::metastream::MetaStream>, CodecError> {
        if let Some(cached) = self
            .metastream_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
        {
            return Ok(std::sync::Arc::clone(cached));
        }
        let parsed = crate::metastream::parse(self.entry_bytes(name)?, name)?;
        let mut cache = self
            .metastream_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let key = name.to_owned();
        let parsed = std::sync::Arc::new(parsed);
        cache.insert(key, std::sync::Arc::clone(&parsed));
        Ok(parsed)
    }

    /// The indexed-record offsets of one `BulkStream` entry, computed at most once per scan.
    pub(crate) fn design_record_offsets(
        &self,
        name: &str,
    ) -> Result<std::sync::Arc<crate::design::decode::sketch::IndexedRecordOffsets>, CodecError>
    {
        if let Some(cached) = self
            .record_offsets_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
        {
            return Ok(std::sync::Arc::clone(cached));
        }
        let built = std::sync::Arc::new(
            crate::design::decode::sketch::IndexedRecordOffsets::build(self.entry_bytes(name)?),
        );
        let mut cache = self
            .record_offsets_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.insert(name.to_owned(), std::sync::Arc::clone(&built));
        Ok(built)
    }

    /// Cached primary frames for one Design `BulkStream` entry, computed at most once per scan.
    pub(crate) fn design_primary_frames(
        &self,
        bulk_entry_name: &str,
    ) -> Result<
        Option<std::sync::Arc<[crate::design::decode::meta::CachedDesignPrimaryFrame]>>,
        CodecError,
    > {
        if let Some(cached) = self
            .primary_frames_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(bulk_entry_name)
        {
            return Ok(cached.as_ref().map(std::sync::Arc::clone));
        }
        let Some(meta) =
            crate::design::decode::meta::metadata_for_bulk_stream(self, bulk_entry_name)?
        else {
            self.primary_frames_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(bulk_entry_name.to_owned(), None);
            return Ok(None);
        };
        let bytes = self.entry_bytes(bulk_entry_name)?;
        let frames: std::sync::Arc<[crate::design::decode::meta::CachedDesignPrimaryFrame]> =
            crate::design::decode::meta::collect_cached_design_primary_frames(bytes, &meta)?.into();
        let mut cache = self
            .primary_frames_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.insert(
            bulk_entry_name.to_owned(),
            Some(std::sync::Arc::clone(&frames)),
        );
        Ok(Some(frames))
    }

    /// Exact archive folder of the manifest-selected Design asset. An outer
    /// F3Z archive has no folder of its own; each member has one.
    pub(crate) fn design_asset_folder(&self) -> Option<&str> {
        match &self.kind {
            F3dContainerKind::Document {
                design_asset_folder,
                ..
            } => Some(design_asset_folder),
            F3dContainerKind::MultiDocument { .. } => None,
        }
    }

    /// Primary GUID of the manifest-selected Design asset. An outer F3Z
    /// archive has no Design asset of its own.
    pub(crate) fn design_asset_guid(&self) -> Option<&str> {
        match &self.kind {
            F3dContainerKind::Document {
                design_asset_guid, ..
            } => Some(design_asset_guid),
            F3dContainerKind::MultiDocument { .. } => None,
        }
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
        expected_role: ContainerRole,
    ) -> bool {
        entry.role == expected_role && self.belongs_to_design_asset(&entry.name)
    }

    /// Whether `entry` is a stream of `expected_role` in a Design segment of
    /// the manifest-selected Design asset.
    pub(crate) fn is_design_stream(
        &self,
        entry: &ContainerEntry,
        expected_role: ContainerRole,
    ) -> bool {
        if !self.is_design_asset_entry(entry, expected_role) {
            return false;
        }
        self.asset_segment(&entry.name)
            .is_some_and(is_design_segment)
    }

    /// Whether `entry` is a `BulkStream.dat` in an ACT segment of the
    /// manifest-selected Design asset.
    pub(crate) fn is_act_stream(&self, entry: &ContainerEntry) -> bool {
        self.is_design_asset_entry(entry, ContainerRole::Bulkstream)
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
/// Readable entries are registered as stored slices or decompressed spaces.
/// An unreadable optional asset is given up as one part.
pub(crate) fn scan<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerScan<'a>, CodecError> {
    scan_in(ctx, root, None, false)
}

/// [`scan`] of the F3Z member stored at `path`, whose given-up parts are
/// placed in the member's stream.
pub(crate) fn scan_member<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    path: &str,
) -> Result<ContainerScan<'a>, CodecError> {
    let stream = StreamName::try_from(path.to_owned())
        .map_err(|_| CodecError::malformed("F3Z member path is empty"))?;
    scan_in(ctx, root, Some(&stream), false)
}

/// Read only the container members needed by the configuration decoder.
pub(crate) fn scan_configurations<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerScan<'a>, CodecError> {
    scan_in(ctx, root, None, true)
}

fn is_design_segment(segment: &str) -> bool {
    segment == "Design1" || is_numbered_segment(segment, "FusionDesignSegmentType")
}

fn scan_in<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    stream: Option<&StreamName>,
    configurations_only: bool,
) -> Result<ContainerScan<'a>, CodecError> {
    #[cfg(test)]
    crate::test_support::scan_counts::record(root);
    let source_image = root.window();
    let archive = Arc::new(ArchiveSnapshot::new(ctx, root)?);
    let defer_members = archive.entry("Manifest.dat").is_none()
        && archive.entry("Manifest.json").is_some()
        && archive.entry("DesignDescription.json").is_some();
    let mut deferred_members = BTreeMap::new();

    let mut entries = Vec::new();
    let mut breps = Vec::new();
    let mut inflated_entries = BTreeMap::new();
    let mut given_up = Vec::new();
    let mut stored_ranges = BTreeMap::new();

    for file in archive.entries() {
        let role = classify(&file.name);
        if configurations_only
            && !matches!(role, ContainerRole::Manifest | ContainerRole::DesignConfig)
            && !(matches!(role, ContainerRole::Bulkstream | ContainerRole::Metastream)
                && file.name.rsplit('/').nth(1).is_some_and(is_design_segment))
        {
            continue;
        }
        let name = file.name.clone();
        if let Ok(range) = file.stored_range() {
            stored_ranges.insert(name.clone(), range);
        }
        let compression = file.compression;
        let compressed_size = file.compressed_size;
        let uncompressed_size = file.uncompressed_size;
        let mut attributes = BTreeMap::new();
        if let cadmpeg_container::ZipCompression::Unsupported(method) = compression {
            attributes.insert("compression".to_owned(), format!("{method}"));
        }
        if file.encrypted {
            attributes.insert("encrypted".to_owned(), "true".to_owned());
        }

        let is_brep = role == ContainerRole::BrepSmbh || role == ContainerRole::BrepSmb;
        let deferred = defer_members
            && (is_f3d_name(&name)
                || std::path::Path::new(&name)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("f2d")));
        let checkpoint = ctx.checkpoint();
        let temporary = DecodeArena::new();
        let opened = ctx.run_part(|| {
            if deferred {
                ctx.with_arena(&temporary, |local| {
                    archive.open(local, &file.name).map(|_| None)
                })
            } else {
                archive.open(ctx, &file.name).map(Some)
            }
        })?;
        let view = match opened {
            Ok(view) => {
                if deferred {
                    deferred_members.insert(name.clone(), checkpoint);
                }
                view
            }
            Err(error)
                if matches!(
                    role,
                    ContainerRole::Preview
                        | ContainerRole::Image
                        | ContainerRole::OgsCache
                        | ContainerRole::Other
                        | ContainerRole::BrepSmb
                        | ContainerRole::BrepSmbh
                ) || matches!(
                    error,
                    CodecError::ResourceLimit(ResourceLimit {
                        scope: LimitScope::Part,
                        ..
                    })
                ) =>
            {
                let range = file.stored_range()?;
                let stored = usize::try_from(range.start)
                    .ok()
                    .zip(usize::try_from(range.end).ok())
                    .and_then(|(start, end)| View::over_retained(source_image).child(start, end))
                    .ok_or_else(|| CodecError::malformed("F3D ZIP payload escapes the archive"))?;
                attributes.insert("payload_error".to_owned(), error.to_string());
                // The source image record keeps every stored byte of the archive.
                let mut part = Part::new(
                    "f3d",
                    ENTRY_PART,
                    stored,
                    held_classes(role).iter().copied(),
                )
                .named(name.as_str())
                .digest_only();
                if let Some(stream) = stream {
                    part = part.in_stream(stream.clone());
                }
                let cause = Cause::Unreadable.of(&error);
                let part = ctx.give_up_part(part, cause, error)?;
                // Two directory entries can name one stored span.
                if given_up
                    .iter()
                    .all(|kept: &GivenUpPart| kept.id() != part.id())
                {
                    given_up.push(part);
                }
                None
            }
            Err(error) => return Err(error),
        };
        let buf = view.map_or(&[][..], |view| view.window());
        if is_brep && view.is_some() {
            let kernel = if asm_header::has_asm_magic(buf) {
                asm_header::parse(buf).map(|header| KernelFraming::Asm {
                    solved_record_limit: asm_header::solved_record_limit_with_header(buf, &header),
                    header,
                })
            } else {
                acis_header::parse(buf).map(KernelFraming::Acis)
            };
            let solved_record_limit = kernel.as_ref().and_then(KernelFraming::solved_record_limit);
            let sha = cadmpeg_ir::hash::digest::Sha256Digest::digest(buf);

            attributes.insert("asm_magic".to_owned(), asm_magic_label(buf));
            if let Some(h) = kernel.as_ref().and_then(KernelFraming::asm_header) {
                attributes.insert("asm_width".to_owned(), h.width.to_string());
                if let Some(v) = h.metadata.save_format_version {
                    attributes.insert("acis_save_format_version".to_owned(), v.to_string());
                }
                if let Some(v) = asm_header::record_count(buf) {
                    attributes.insert("asm_record_count".to_owned(), v.to_string());
                }
                if let Some(v) = h.metadata.entity_count {
                    attributes.insert("asm_entity_count".to_owned(), v.to_string());
                }
                if let Some(v) = h.metadata.flags {
                    attributes.insert("asm_flags".to_owned(), v.to_string());
                }
                if let Some(pf) = &h.metadata.product_family {
                    attributes.insert("product_family".to_owned(), pf.to_owned());
                }
                if let Some(pv) = &h.metadata.product_version {
                    attributes.insert("product_version".to_owned(), pv.to_owned());
                }
                if let Some(sd) = &h.metadata.save_date {
                    attributes.insert("save_date".to_owned(), sd.to_owned());
                }
                if let Some(s) = h.metadata.scale {
                    attributes.insert("scale".to_owned(), format!("{s}"));
                }
                if let Some(r) = h.metadata.linear {
                    attributes.insert("resabs".to_owned(), format!("{r}"));
                }
                if let Some(r) = h.metadata.angular {
                    attributes.insert("resnor".to_owned(), format!("{r}"));
                }
            }
            match solved_record_limit {
                Some(offset) => {
                    attributes.insert("history_partition_offset".to_owned(), offset.to_string());
                    attributes.insert("solved_record_len".to_owned(), offset.to_string());
                }
                None => {
                    attributes.insert("history_partition_offset".to_owned(), "none".to_string());
                }
            }
            attributes.insert("sha256".to_owned(), sha.as_str().to_owned());

            breps.push(BrepFacts {
                name: name.clone(),
                uncompressed_len: uncompressed_size,
                kernel,
                sha256: sha,
            });
        }

        let declared_storage = if file.encrypted {
            Ok(EntryStorage::payload_only(
                VerbatimLabel::None,
                compressed_size,
            ))
        } else {
            compression.storage(compressed_size, uncompressed_size)
        };
        let storage = match declared_storage {
            Ok(storage) => storage,
            Err(message) => {
                attributes.insert(
                    "storage_declaration".to_owned(),
                    format!("{message}: {compressed_size}/{uncompressed_size}"),
                );
                EntryStorage::payload_only(VerbatimLabel::Stored, uncompressed_size)
            }
        };
        entries.push(ContainerEntry {
            name: name.clone(),
            role,
            storage,
            attributes,
        });
        if let Some(view) = view {
            inflated_entries.insert(name, view);
        }
    }

    // The parse strategy and the dialect row are chosen together, from the same
    // discriminants, before anything semantic is read. Classifying here is what
    // keeps the report from re-deriving an identity the parse already settled.
    let mut root_document_members = root_f3d_members(&inflated_entries);
    root_document_members.extend(
        deferred_members
            .keys()
            .map(String::as_str)
            .filter(|name| !name.contains('/') && is_f3d_name(name)),
    );
    root_document_members.sort_unstable();
    let mut manifest_diagnostics = Vec::new();
    let mut manifest_reference_extension = None;
    let mut manifest_lineage_urn = None;
    let kind = if let Some(top_level_view) = inflated_entries.get("Manifest.dat") {
        let top_level_manifest = manifest::parse_top_level(top_level_view.window())?;
        let mut extension = *top_level_view;
        extension
            .req_take(top_level_manifest.reference_extension.start)
            .map_err(CodecError::from)?;
        manifest_reference_extension = Some(
            extension
                .req_take_child(top_level_manifest.reference_extension.len())
                .map_err(CodecError::from)?,
        );
        let matched = F3dDialect::classify_document(top_level_manifest.declared_version());
        let design_asset = manifest::resolve_design_folder(
            &top_level_manifest,
            inflated_entries.keys().map(String::as_str),
            |name| inflated_entries.get(name).map(|view| view.window()),
        )?;
        manifest_lineage_urn = top_level_manifest.lineage_urn;
        manifest_diagnostics = top_level_manifest.diagnostics;
        F3dContainerKind::Document {
            design_asset_folder: design_asset.folder,
            design_asset_guid: design_asset.asset_guid,
            matched,
        }
    } else if inflated_entries.contains_key("Manifest.json")
        && inflated_entries.contains_key("DesignDescription.json")
        && !root_document_members.is_empty()
    {
        F3dContainerKind::MultiDocument {
            matched: F3dDialect::classify_f3z(&root_document_members),
        }
    } else {
        return Err(CodecError::Malformed(
            "Fusion ZIP has neither a top-level Manifest.dat nor the F3Z manifest set".into(),
        ));
    };

    let mut scope_entry_indices = std::collections::HashMap::<String, Vec<usize>>::new();
    for (index, entry) in entries.iter().enumerate() {
        let scope = crate::ids::native_scope(&entry.name);
        scope_entry_indices.entry(scope).or_default().push(index);
    }

    let archive_member_sizes = match &kind {
        F3dContainerKind::MultiDocument { .. } => archive
            .entries()
            .iter()
            .map(|entry| entry.uncompressed_size)
            .collect(),
        F3dContainerKind::Document { .. } => Vec::new(),
    };
    let working_bytes = archive
        .entries()
        .iter()
        .try_fold(u64_from_index(source_image.len()), |total, entry| {
            total.checked_add(entry.uncompressed_size)
        });
    let archive = matches!(kind, F3dContainerKind::MultiDocument { .. }).then_some(archive);
    let mut scan = ContainerScan {
        source_image,
        entries,
        archive_member_sizes,
        breps,
        text_breps: std::collections::HashMap::new(),
        kind,
        manifest_diagnostics,
        manifest_reference_extension,
        manifest_lineage_urn,
        given_up,
        stream: stream.cloned(),
        stored_ranges,
        unindexed_record_runs: Vec::new(),
        inflated_entries,
        archive,
        working_bytes,
        deferred_members,
        scope_entry_indices,
        metastream_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        record_offsets_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        primary_frames_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
    };
    bound_design_bulk_streams(&mut scan);
    for index in 0..scan.entries.len() {
        let entry = &scan.entries[index];
        if entry.role != ContainerRole::BrepText || !scan.belongs_to_design_asset(&entry.name) {
            continue;
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        let framing = match cadmpeg_asm::sat::parse(ctx, bytes) {
            Ok(stream) => TextBrepFraming::Parsed(Box::new(stream)),
            Err(cadmpeg_asm::stream_error::StreamFailure::Parse(error)) => {
                TextBrepFraming::Unframed(error)
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Malformed(error)) => {
                TextBrepFraming::Malformed(error)
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::NotImplemented(error)) => {
                TextBrepFraming::UnsupportedLength(error)
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
                return Err(error.into())
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
                return Err(error.into_codec_error())
            }
        };

        let name = entry.name.clone();
        scan.text_breps.insert(name, framing);
    }
    Ok(scan)
}

/// End each Design `BulkStream` view at the record run after its last indexed
/// record, so that decoders read only the records its `MetaStream` indexes.
fn bound_design_bulk_streams(scan: &mut ContainerScan<'_>) {
    let bulk_streams = scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
        .map(|entry| entry.name.clone())
        .collect::<Vec<_>>();
    for name in bulk_streams {
        let Some(meta_name) = name
            .strip_suffix("BulkStream.dat")
            .map(|prefix| format!("{prefix}MetaStream.dat"))
        else {
            continue;
        };
        let (Some(view), Ok(meta)) = (scan.entry_view(&name), scan.parsed_metastream(&meta_name))
        else {
            continue;
        };
        let Some(run) = crate::design::decode::meta::unindexed_record_run(view.window(), &meta)
        else {
            continue;
        };
        let Some(bounded) = view.child(view.start(), view.start() + run.start) else {
            continue;
        };
        let stored = view.window().len();
        scan.inflated_entries.insert(name.clone(), bounded);
        scan.unindexed_record_runs.push((name, run, stored));
    }
}

/// Build a [`ContainerSummary`] without assigning model authority from a ZIP
/// extension. Design body bindings perform the model selection during decode.
pub(crate) fn summarize(
    scan: &ContainerScan<'_>,
    dialects: cadmpeg_core::dialect::DialectLayers,
) -> ContainerSummary {
    ContainerSummary::classified(
        dialects,
        cadmpeg_ir::ContainerKind::Zip,
        scan.entries.clone(),
        Vec::new(),
        summary_notes(scan, SummaryScope::ContainerOnly),
    )
}

/// Whether the caller transferred beyond container metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SummaryScope {
    /// Inspection or a container-only decode.
    ContainerOnly,
    /// A decode that attempted the document model.
    FullDecode,
}

/// Container notes shared by inspection and decode report construction.
pub(crate) fn summary_notes(scan: &ContainerScan<'_>, scope: SummaryScope) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(folder) = scan.design_asset_folder() {
        notes.push(format!("Design asset folder (from manifests): {folder}"));
    } else {
        notes.push("outer F3Z archive; each F3D member selects its own Design asset".to_string());
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
    let history_brep_count = history_breps(scan).count();
    match history_brep_count {
        0 => {
            if design_brep_count != 0 {
                notes.push("no BREP header declares a history partition".to_string());
            }
        }
        1 => {
            if let Some(history) = history_breps(scan).next() {
                notes.push(format!(
                    "history-bearing BREP: {} ({} bytes uncompressed)",
                    history.name, history.uncompressed_len
                ));
            }
        }
        count => notes.push(format!(
            "{count} history-bearing BREPs; each history graph is decoded independently"
        )),
    }
    if scope == SummaryScope::ContainerOnly {
        notes.push("container-level inspection only; run `decode` to resolve Design body bindings and build \
             each referenced BREP graph".to_string());
    }

    notes
}

/// Root-level `*.f3d` member names, sorted by archive path.
///
/// The third clause of the F3Z discriminant: a member whose name carries no `/`
/// and whose extension is `f3d`, case-insensitively. Each name is returned as
/// the archive spells it, and the order is the entry map's, which is sorted
/// rather than the archive's own sequence.
fn root_f3d_members<'a>(entries: &'a BTreeMap<String, View<'_>>) -> Vec<&'a str> {
    let mut members = Vec::new();
    for name in entries.keys().map(String::as_str) {
        if !name.contains('/') && is_f3d_name(name) {
            members.push(name);
        }
    }
    members
}

/// Whether an archive path names an F3D document by extension.
pub(crate) fn is_f3d_name(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("f3d"))
}

/// Iterate over every BREP whose parsed header sets the history-partition bit.
/// The extension is not used as a semantic substitute for the header flag.
pub(crate) fn history_breps<'s>(
    scan: &'s ContainerScan<'_>,
) -> impl Iterator<Item = &'s BrepFacts> + 's {
    design_breps(scan).filter(|brep| {
        brep.kernel
            .as_ref()
            .and_then(KernelFraming::asm_header)
            .is_some_and(|header| header.metadata.has_history_partition())
    })
}

/// Return the history-bearing BREP only when the header relation is unique.
pub(crate) fn select_history_brep<'s>(scan: &'s ContainerScan<'_>) -> Option<&'s BrepFacts> {
    let mut candidates = history_breps(scan);
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

/// Return one unambiguous BREP for compatibility metadata and reporting.
pub(crate) fn select_fallback_brep<'s>(scan: &'s ContainerScan<'_>) -> Option<&'s BrepFacts> {
    if let Some(history) = select_history_brep(scan) {
        return Some(history);
    }
    let mut candidates = design_breps(scan);
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

/// Iterate over binary ASM BREP entries inside the manifest-selected Design
/// asset.
pub(crate) fn design_breps<'s>(
    scan: &'s ContainerScan<'_>,
) -> impl Iterator<Item = &'s BrepFacts> + 's {
    scan.breps
        .iter()
        .filter(|brep| scan.belongs_to_design_asset(&brep.name))
}

/// Mesh geometry container entries inside the manifest-selected Design asset.
pub(crate) fn paramesh_entries<'s>(
    scan: &'s ContainerScan<'_>,
) -> impl Iterator<Item = &'s ContainerEntry> + 's {
    scan.entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, ContainerRole::Paramesh))
}

/// Binary and text BREP entries inside the manifest-selected Design asset,
/// including entries whose payload did not open.
pub(crate) fn brep_entries<'s>(
    scan: &'s ContainerScan<'_>,
) -> impl Iterator<Item = &'s ContainerEntry> + 's {
    scan.entries.iter().filter(|entry| {
        matches!(
            entry.role,
            ContainerRole::BrepSmb | ContainerRole::BrepSmbh | ContainerRole::BrepText
        ) && scan.belongs_to_design_asset(&entry.name)
    })
}

/// Names of the text-encoded ASM BREP entries, in archive order.
///
/// [`ContainerScan::breps`] holds binary BREP candidate facts. Text streams are
/// admitted separately into [`ContainerScan::text_breps`]. Carrier presence is
/// independent of geometry transfer: a text-only document has a carrier even
/// when its transfer fails.
pub(crate) fn text_brep_names<'s>(
    scan: &'s ContainerScan<'_>,
) -> impl Iterator<Item = &'s str> + 's {
    scan.entries
        .iter()
        .filter(|entry| {
            entry.role == ContainerRole::BrepText && scan.belongs_to_design_asset(&entry.name)
        })
        .map(|entry| entry.name.as_str())
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

#[cfg(test)]
mod tests {
    use super::is_f3d_name;

    #[test]
    fn f3d_name_requires_a_nonempty_stem_and_case_insensitive_extension() {
        assert!(is_f3d_name("part.f3d"));
        assert!(is_f3d_name("folder/part.F3D"));
        assert!(!is_f3d_name(".f3d"));
        assert!(!is_f3d_name("part.f3d.tmp"));
    }
}
