// SPDX-License-Identifier: Apache-2.0
//! Parse the modern SPLMSSTR header and legacy Compound File Binary wrapper.
//!
//! A modern NX part begins with the eight-byte `SPLMSSTR` signature. Legacy NX
//! parts use a Compound File Binary envelope whose directory contains the
//! `UG_PART/UG_PART` stream. Modern container integers are little-endian.
//! Directory entries name `/Root/...` paths and may carry an in-bounds file
//! offset and size. [`crate::parasolid`] uses the canonical modern
//! `/Root/UG_PART/UG_PART` span or the opened legacy `UG_PART/UG_PART` stream
//! to locate Parasolid data.

use cadmpeg_core::container::ContainerRole;

pub(crate) mod extref_handles;
pub(crate) mod extref_slot;
use extref_handles::ExtrefHandles;
pub(crate) mod entry_ref;
pub(crate) mod membership;
use entry_ref::EntryRef;
use membership::ObjectIdMembers;

pub(crate) mod source_image;
use source_image::SourceImage;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use cadmpeg_container::compound::{CompoundEntry, CompoundPrefixProbe, CompoundSnapshot};
use cadmpeg_core::bytes::find;
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::directory_entry as dir_entry;
use crate::layout::directory_file_payload as file_payload;
use crate::layout::extrefstream_handle_set_record as handle_set;
use crate::layout::legacy_ugii_payload_prefix;
use crate::layout::splmsstr_header as splmsstr;
use crate::layout::ug_part_segment_index_row as index_row;

/// The eight-byte signature used to identify an SPLMSSTR container.
pub(crate) const MAGIC: &[u8; 8] = &splmsstr::MAGIC_VALUE;

const LEGACY_UGII_PREFIX: &[u8; legacy_ugii_payload_prefix::VERSION] = b"\x0d\x01UGII  ";

/// A directory entry from the `HEADER` or `FOOTER` region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirEntry {
    /// The `/Root/...` path name.
    pub(crate) name: String,
    /// Which region the entry was read from.
    pub(crate) region: Region,
    /// Directory or file payload.
    pub(crate) body: DirEntryBody,
}

/// Payload of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirEntryBody {
    /// A directory without a file payload.
    Directory,
    /// A file payload at an offset and length.
    File {
        /// Payload byte offset.
        offset: u64,
        /// Payload byte length.
        len: u64,
    },
}

/// Directory region containing an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    /// The `HEADER` region near the start of the file.
    Header,
    /// The `FOOTER` region near EOF.
    Footer,
}

/// Stable semantic classification of one named SPLMSSTR entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryContent {
    Directory,
    PartPayload,
    ActiveBodyIndex,
    FastLoadStructure,
    FastLoadJt,
    DisplayJt,
    ExternalReferences,
    SaveToggleInfo,
    PreviewImage,
    MaterialTexture,
    Arrangements,
    PartAttributes,
    AssetCatalog,
    NamedOpaqueStream,
}

impl EntryContent {
    /// Stable inspection label for this content family.
    pub(crate) fn role(self) -> ContainerRole {
        match self {
            Self::Directory => ContainerRole::Directory,
            Self::PartPayload => ContainerRole::PartPayload,
            Self::ActiveBodyIndex => ContainerRole::ActiveBodyIndex,
            Self::FastLoadStructure => ContainerRole::FastLoadStructure,
            Self::FastLoadJt => ContainerRole::FastLoadJt,
            Self::DisplayJt => ContainerRole::DisplayJt,
            Self::ExternalReferences => ContainerRole::ExternalReferences,
            Self::SaveToggleInfo => ContainerRole::SaveToggleInfo,
            Self::PreviewImage => ContainerRole::PreviewImage,
            Self::MaterialTexture => ContainerRole::MaterialTexture,
            Self::Arrangements => ContainerRole::Arrangements,
            Self::PartAttributes => ContainerRole::PartAttributes,
            Self::AssetCatalog => ContainerRole::AssetCatalog,
            Self::NamedOpaqueStream => ContainerRole::NamedOpaqueStream,
        }
    }

    /// Whether the codec retains the complete payload as named opaque content.
    pub(crate) fn retains_opaque_payload(self) -> bool {
        matches!(
            self,
            Self::FastLoadStructure
                | Self::FastLoadJt
                | Self::SaveToggleInfo
                | Self::NamedOpaqueStream
        )
    }
    pub(crate) fn label(self) -> &'static str {
        self.role().as_str()
    }
}

impl DirEntry {
    pub(crate) fn file_span(&self) -> Option<(u64, u64)> {
        match self.body {
            DirEntryBody::Directory => None,
            DirEntryBody::File { offset, len } => Some((offset, len)),
        }
    }

    /// Classify this entry by its canonical storage path.
    pub(crate) fn content(&self) -> EntryContent {
        match self.body {
            DirEntryBody::Directory => EntryContent::Directory,
            DirEntryBody::File { .. } => match self.name.as_str() {
                "/Root/UG_PART/UG_PART" => EntryContent::PartPayload,
                "/Root/FastLoad/RMFastLoad" => EntryContent::ActiveBodyIndex,
                "/Root/FastLoad/Structure" => EntryContent::FastLoadStructure,
                "/Root/FastLoad/JT" => EntryContent::FastLoadJt,
                "/Root/UG_PART/DisplayJT" => EntryContent::DisplayJt,
                "/Root/UG_PART/LastSavedToggleInfoStream" => EntryContent::SaveToggleInfo,
                "/Root/images/preview" => EntryContent::PreviewImage,
                "/Root/part/arrangements" => EntryContent::Arrangements,
                "/Root/part/attrs" => EntryContent::PartAttributes,
                "/Root/qafmetadata" => EntryContent::AssetCatalog,
                name if name.ends_with("/ExternalReferences") => EntryContent::ExternalReferences,
                name if name.starts_with("/Root/materialsTif/") => EntryContent::MaterialTexture,
                _ => EntryContent::NamedOpaqueStream,
            },
        }
    }
}

/// One 12-byte row in the canonical `UG_PART` segment index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SegmentIndexRow {
    /// First little-endian row word.
    pub(crate) type_code: u32,
    /// Second little-endian row word.
    pub(crate) subtype_code: u32,
    /// Third little-endian row word.
    pub(crate) value: u32,
}

/// Self-bounded segment index at the start of the canonical `UG_PART` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentIndex<'a> {
    /// Complete 12-byte rows before the declared table end.
    rows: &'a [u8],
    /// Zero to eleven trailing bytes after the last complete row.
    padding: &'a [u8],
}

impl<'a> SegmentIndex<'a> {
    pub(crate) fn row(&self, ordinal: usize) -> Option<SegmentIndexRow> {
        let start = ordinal.checked_mul(index_row::LEN)?;
        let end = start.checked_add(index_row::LEN)?;
        let bytes = self.rows.get(start..end)?;
        Self::parse_row(bytes)
    }

    pub(crate) fn rows(&self) -> impl Iterator<Item = SegmentIndexRow> + 'a {
        self.rows
            .chunks_exact(index_row::LEN)
            .filter_map(Self::parse_row)
    }

    fn parse_row(bytes: &[u8]) -> Option<SegmentIndexRow> {
        Some(SegmentIndexRow {
            type_code: View::u32_le_at(bytes, index_row::TYPE_CODE)?,
            subtype_code: View::u32_le_at(bytes, index_row::SUBTYPE_CODE)?,
            value: View::u32_le_at(bytes, index_row::VALUE)?,
        })
    }
}

/// One segment-index word whose target frames a compressed stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SegmentStreamWrapper {
    /// Zero-based segment-index row ordinal.
    pub(crate) row_ordinal: usize,
    /// Zero-based word ordinal within the row.
    pub(crate) word_ordinal: usize,
    /// Absolute file offset of the wrapper.
    pub(crate) wrapper_offset: usize,
    /// Bytes from the wrapper start through its extension.
    pub(crate) wrapper_byte_len: usize,
    /// Absolute file offset of the candidate zlib member.
    pub(crate) zlib_offset: usize,
}

/// Counted object-id table in `/Root/FastLoad/RMFastLoad`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RmFastLoadObjectIdTable {
    /// Payload-relative offset of the `UGS::Solid::Topol` registry marker.
    pub(crate) registry_offset: usize,
    /// Payload-relative offset of the four-byte count word.
    pub(crate) count_offset: usize,
    /// Ordered fixed-width object-id members.
    pub(crate) object_ids: ObjectIdMembers<u32>,
}

impl RmFastLoadObjectIdTable {
    pub(crate) fn member_offset(&self, ordinal: usize) -> usize {
        self.count_offset + 4 + ordinal * 4
    }
}

impl Region {
    /// Return the directory-region label used in summaries.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Region::Header => "HEADER",
            Region::Footer => "FOOTER",
        }
    }
}

impl<'a> Container<'a> {
    /// Number of directory entries in a region.
    pub(crate) fn entry_count(&self, region: Region) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.region == region)
            .count()
    }

    pub(crate) fn has_external_references(&self) -> bool {
        for entry in &self.entries {
            if entry.name.contains("ExternalReferences") {
                return true;
            }
        }
        false
    }

    /// Return an absolute source span only when it is wholly owned by one
    /// catalogued directory entry.
    pub(crate) fn bounded_entry_bytes(&self, offset: u64, byte_len: u64) -> Option<&[u8]> {
        let bytes = (|| {
            let offset = usize::try_from(offset).ok()?;
            let byte_len = usize::try_from(byte_len).ok()?;
            let end = offset.checked_add(byte_len)?;
            let owner_end = self
                .entries
                .iter()
                .filter_map(|entry| {
                    let (start, entry_byte_len) = entry.file_span()?;
                    let start = usize::try_from(start).ok()?;
                    let entry_byte_len = usize::try_from(entry_byte_len).ok()?;
                    let entry_end = start.checked_add(entry_byte_len)?;
                    (start <= offset && end <= entry_end).then_some(entry_end)
                })
                .min()?;
            (end <= owner_end)
                .then(|| self.data.get(offset..end))
                .flatten()
        })();
        bytes
    }

    /// Return bytes from an absolute offset through the end of its bounded
    /// directory-entry span.
    pub(crate) fn bounded_entry_tail(&self, offset: u64) -> Option<&[u8]> {
        let bytes = (|| {
            let offset = usize::try_from(offset).ok()?;
            let end = self
                .entries
                .iter()
                .filter_map(|entry| {
                    let (start, byte_len) = entry.file_span()?;
                    let start = usize::try_from(start).ok()?;
                    let byte_len = usize::try_from(byte_len).ok()?;
                    let end = start.checked_add(byte_len)?;
                    (start <= offset && offset < end).then_some(end)
                })
                .min()?;
            self.data.get(offset..end)
        })();
        bytes
    }

    /// Decode the self-bounded segment index in `/Root/UG_PART/UG_PART`.
    pub(crate) fn segment_index(&self) -> Option<(&DirEntry, SegmentIndex<'_>)> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.name == "/Root/UG_PART/UG_PART" && entry.file_span().is_some())?;
        let (offset, size) = entry.file_span()?;
        let (offset, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
        let payload = self.data.get(offset..offset.checked_add(size)?)?;
        let row_one = payload.get(index_row::LEN..index_row::LEN * 2)?;
        let type_code = View::u32_le_at(row_one, index_row::TYPE_CODE)?;
        let subtype_code = View::u32_le_at(row_one, index_row::SUBTYPE_CODE)?;
        let byte_len = usize::try_from(View::u32_le_at(row_one, index_row::VALUE)?).ok()?;
        if type_code != 1
            || subtype_code != 1
            || !(index_row::LEN * 2..=payload.len()).contains(&byte_len)
        {
            return None;
        }
        let complete_len = byte_len / index_row::LEN * index_row::LEN;
        let rows = &payload[..complete_len];
        rows.chunks_exact(index_row::LEN)
            .try_for_each(|row| SegmentIndex::parse_row(row).map(|_| ()))?;
        Some((
            entry,
            SegmentIndex {
                rows,
                padding: &payload[complete_len..byte_len],
            },
        ))
    }

    /// Resolve every in-bounds compressed-stream wrapper addressed by the
    /// canonical segment index, preserving row and word order.
    pub(crate) fn segment_stream_wrappers(
        &self,
    ) -> impl Iterator<Item = SegmentStreamWrapper> + '_ {
        let source = (|| {
            let (entry, index) = self.segment_index()?;
            let (entry_offset, entry_size) = entry.file_span()?;
            let entry_start = usize::try_from(entry_offset).ok()?;
            let entry_size = usize::try_from(entry_size).ok()?;
            let entry_end = entry_start.checked_add(entry_size)?;
            let payload = self.data.get(entry_start..entry_end)?;
            Some((index, payload, entry_start))
        })();
        source
            .into_iter()
            .flat_map(|(index, payload, entry_start)| {
                index
                    .rows()
                    .enumerate()
                    .flat_map(move |(row_ordinal, row)| {
                        [row.type_code, row.subtype_code, row.value]
                            .into_iter()
                            .enumerate()
                            .filter_map(move |(word_ordinal, relative)| {
                                let relative = usize::try_from(relative).ok()?;
                                let wrapper = payload.get(relative..)?;
                                let wrapper_word = View::u32_le_at(wrapper, 0)?;
                                let extension = usize::try_from(wrapper_word & 0x3fff_ffff).ok()?;
                                let wrapper_byte_len = match wrapper_word & 0xc000_0000 {
                                    0x8000_0000 => 8usize.checked_add(extension),
                                    0xc000_0000 => 33usize.checked_add(extension),
                                    _ => None,
                                }?;
                                let zlib_relative = relative.checked_add(wrapper_byte_len)?;
                                (zlib_relative < payload.len()).then_some(SegmentStreamWrapper {
                                    row_ordinal,
                                    word_ordinal,
                                    wrapper_offset: entry_start.checked_add(relative)?,
                                    wrapper_byte_len,
                                    zlib_offset: entry_start.checked_add(zlib_relative)?,
                                })
                            })
                    })
            })
    }

    /// Locate independently size-framed NX object-model sections.
    pub(crate) fn om_sections(
        &self,
    ) -> Result<Vec<(EntryRef<'_>, crate::om::Section<'_>)>, CodecError> {
        let framed_cache = if let Some(cache) = self.om_section_cache.get() {
            cache
        } else {
            let cache = match &self.data {
                SourceImage::Borrowed(view) => {
                    let bytes = view.window();
                    let bytes: &'a [u8] = bytes;
                    let (sections, _) = parse_framed_section_cache(bytes, &self.entries, false)?;
                    FramedSectionCache::Borrowed { sections }
                }
                SourceImage::Owned(bytes) => {
                    let (sections, layouts) =
                        parse_framed_section_cache(bytes, &self.entries, true)?;
                    drop(sections);
                    FramedSectionCache::Owned { layouts }
                }
            };
            self.om_section_cache.get_or_init(|| cache)
        };
        let mut result = Vec::new();
        match framed_cache {
            FramedSectionCache::Borrowed { sections } => {
                for (entry_index, section) in sections {
                    if let Some(entry) = EntryRef::new(&self.entries, *entry_index) {
                        result.push((entry, section.clone()));
                    }
                }
            }
            FramedSectionCache::Owned { layouts } => {
                for (entry_index, layout) in layouts {
                    if let Some(entry) = EntryRef::new(&self.entries, *entry_index) {
                        result.push((entry, layout.materialize()));
                    }
                }
            }
        }
        Ok(result)
    }

    /// Locate indexed NX object-model sections in catalogued file entries.
    pub(crate) fn indexed_om_sections(
        &self,
    ) -> Result<Vec<(EntryRef<'_>, crate::om::IndexedSection<'_>)>, CodecError> {
        let cache = if let Some(cache) = self.indexed_section_layouts.get() {
            cache
        } else {
            let cache = match &self.data {
                SourceImage::Borrowed(view) => {
                    let bytes = view.window();
                    let bytes: &'a [u8] = bytes;
                    let (sections, _) = parse_indexed_section_cache(bytes, &self.entries, false)?;
                    let mut blocks = BTreeMap::new();
                    for (section_ordinal, (entry_index, section)) in sections.iter().enumerate() {
                        let Some((control, _, records)) = section.as_offset_only() else {
                            continue;
                        };
                        let entry_offset = self
                            .entries
                            .get(*entry_index)
                            .and_then(crate::container::DirEntry::file_span)
                            .map_or(0, |(offset, _)| offset);
                        blocks.insert(
                            offset_block_key(section_ordinal, 0),
                            (
                                control.bytes,
                                entry_offset + cadmpeg_core::decode::u64_from_index(control.offset),
                            ),
                        );
                        for (ordinal, block) in (1..).zip(records) {
                            blocks.insert(
                                offset_block_key(section_ordinal, ordinal),
                                (
                                    block.bytes,
                                    entry_offset
                                        + cadmpeg_core::decode::u64_from_index(block.offset),
                                ),
                            );
                        }
                    }
                    IndexedSectionCache::Borrowed { sections, blocks }
                }
                SourceImage::Owned(bytes) => {
                    let (_, layouts) = parse_indexed_section_cache(bytes, &self.entries, true)?;
                    IndexedSectionCache::Owned { layouts }
                }
            };
            self.indexed_section_layouts.get_or_init(|| cache)
        };
        let mut result = Vec::new();
        match cache {
            IndexedSectionCache::Borrowed { sections, .. } => {
                for (entry_index, section) in sections {
                    if let Some(entry) = EntryRef::new(&self.entries, *entry_index) {
                        result.push((entry, section.clone()));
                    }
                }
            }
            IndexedSectionCache::Owned { layouts } => {
                for (entry_index, layout) in layouts {
                    if let Some(entry) = EntryRef::new(&self.entries, *entry_index) {
                        result.push((entry, layout.materialize()));
                    }
                }
            }
        }
        Ok(result)
    }

    /// Return the cached bytes and source offsets of every borrowed offset-store block.
    ///
    /// Owned inputs retain independent source-backed caches; these slices refer
    /// only to an input borrowed for the container lifetime.
    pub(crate) fn cached_offset_data_block_bytes(
        &self,
    ) -> Option<&BTreeMap<String, (&'a [u8], u64)>> {
        self.indexed_section_layouts
            .get()
            .and_then(|cache| match cache {
                IndexedSectionCache::Borrowed { blocks, .. } => Some(blocks),
                IndexedSectionCache::Owned { .. } => None,
            })
    }

    /// Extract child-part paths from catalogued external-reference payloads.
    pub(crate) fn external_reference_paths(&self) -> Vec<String> {
        self.external_reference_strings()
            .into_iter()
            .map(|(_, _, path)| path)
            .collect()
    }

    /// Extract child-part strings with their owning entry and payload offset.
    pub(crate) fn external_reference_strings(&self) -> Vec<(&DirEntry, usize, String)> {
        let mut out = Vec::new();
        for entry in &self.entries {
            if !entry.name.contains("ExternalReferences") {
                continue;
            }
            let Some((offset, size)) = entry.file_span() else {
                continue;
            };
            let (Ok(offset), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
                continue;
            };
            let Some(end) = offset.checked_add(size) else {
                continue;
            };
            let Some(payload) = self.data.get(offset..end) else {
                continue;
            };
            let Some((_, strings)) = parse_extref_string_table(payload) else {
                continue;
            };
            out.extend(
                strings
                    .into_iter()
                    .map(|(relative, value)| (entry, relative, value)),
            );
        }
        out
    }

    /// Decode indexed EXTREFSTREAM record prefixes and sorted handle lanes.
    pub(crate) fn external_reference_records(&self) -> Vec<(&DirEntry, ExtrefRecord)> {
        let mut out = Vec::new();
        for entry in &self.entries {
            if !entry.name.contains("ExternalReferences") {
                continue;
            }
            let Some((offset, size)) = entry.file_span() else {
                continue;
            };
            let (Ok(offset), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
                continue;
            };
            let Some(end) = offset.checked_add(size) else {
                continue;
            };
            let Some(payload) = self.data.get(offset..end) else {
                continue;
            };
            let records = parse_extref_records(payload);
            out.extend(records.into_iter().map(|record| (entry, record)));
        }
        out
    }

    /// Retain every record boundary from each valid EXTREFSTREAM index.
    pub(crate) fn external_reference_indexed_records(
        &self,
    ) -> Vec<(&DirEntry, ExtrefIndexedRecord)> {
        let mut out = Vec::new();
        for entry in &self.entries {
            if !entry.name.contains("ExternalReferences") {
                continue;
            }
            let Some((offset, size)) = entry.file_span() else {
                continue;
            };
            let (Ok(offset), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
                continue;
            };
            let Some(end) = offset.checked_add(size) else {
                continue;
            };
            let Some(payload) = self.data.get(offset..end) else {
                continue;
            };
            let Some(records) = parse_extref_record_index(payload) else {
                continue;
            };
            out.extend(records.into_iter().map(|record| (entry, record)));
        }
        out
    }

    /// Borrow the admitted object-id table from `/Root/FastLoad/RMFastLoad`.
    pub(crate) fn rmfastload_object_id_table(
        &self,
    ) -> Option<(&DirEntry, &RmFastLoadObjectIdTable)> {
        let (entry_index, table) = self.fastload_table.as_ref()?;
        Some((self.entries.get(*entry_index)?, table))
    }

    fn parse_rmfastload_object_id_table(
        &self,
    ) -> Result<Option<(usize, RmFastLoadObjectIdTable)>, CodecError> {
        const REGISTRY_MARKER: &[u8] = b"UGS::Solid::Topol";
        let Some((entry_index, bytes, registry_offset, search_start)) = (|| {
            let entry_index = self.entries.iter().position(|entry| {
                entry.name == "/Root/FastLoad/RMFastLoad" && entry.file_span().is_some()
            })?;
            let (offset, size) = self.entries[entry_index].file_span()?;
            let (offset, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
            let bytes = self.data.get(offset..offset.checked_add(size)?)?;
            let registry_offset = find(bytes, REGISTRY_MARKER)?;
            let search_start = registry_offset.checked_add(REGISTRY_MARKER.len())?;
            Some((entry_index, bytes, registry_offset, search_start))
        })() else {
            return Ok(None);
        };
        // Search candidates in byte order and use the first span whose suffix
        // parses as a modern product record.
        let mut candidate = None;
        let Some(search_end) = bytes.len().checked_sub(3) else {
            return Ok(None);
        };
        for count_offset in search_start..search_end {
            let Some((count, ids_start, id_bytes)) = (|| {
                let count = usize::try_from(View::u32_le_at(bytes, count_offset)?).ok()?;
                let id_bytes = count.checked_mul(4)?;
                let ids_start = count_offset.checked_add(4)?;
                let ids_end = ids_start.checked_add(id_bytes)?;
                crate::om::product::ProductRecord::read(
                    bytes.get(ids_end..)?,
                    crate::om::product::ProductRecordForm::Modern,
                )
                .is_some()
                .then_some((count, ids_start, id_bytes))
            })() else {
                continue;
            };
            candidate = Some((count_offset, count, ids_start, id_bytes));
            break;
        }
        let Some((count_offset, count, ids_start, _id_bytes)) = candidate else {
            return Ok(None);
        };
        let mut object_ids = Vec::new();
        for ordinal in 0..count {
            let offset = ids_start + ordinal * 4;
            let object_id = View::u32_le_at(bytes, offset).ok_or_else(|| {
                CodecError::Malformed("FastLoad object ID span is inconsistent".into())
            })?;
            object_ids.push(object_id);
        }
        let object_ids = ObjectIdMembers::new(object_ids)
            .map_err(|message| CodecError::Malformed(message.into()))?;
        Ok(Some((
            entry_index,
            RmFastLoadObjectIdTable {
                registry_offset,
                count_offset,
                object_ids,
            },
        )))
    }
}

/// Decoded prefix of one indexed EXTREFSTREAM record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtrefRecord {
    pub(crate) record_id: u32,
    pub(crate) offset: usize,
    pub(crate) declared_count: u16,
    pub(crate) id_slots: [u32; 4],
    pub(crate) handles: ExtrefHandles,
    pub(crate) tail_byte_len: usize,
}

/// One externally bounded record from a validated EXTREFSTREAM index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtrefIndexedRecord {
    pub(crate) record_id: u32,
    pub(crate) offset: usize,
    pub(crate) byte_len: usize,
}

fn locate_extref_string_table(payload: &[u8]) -> Option<(usize, usize, usize)> {
    let last_marker = payload.len().checked_sub(4)?;
    for marker in (0..last_marker).rev() {
        if payload[marker] != 1 {
            continue;
        }
        let Some(count) = View::u32_le_at(payload, marker + 1) else {
            continue;
        };
        let Some(start) = marker.checked_add(5) else {
            continue;
        };
        // Each entry is a 2-byte length prefix plus at least one non-empty string byte.
        let Some(remaining) = payload.len().checked_sub(start) else {
            continue;
        };
        let Some(count) = bounded_len(u64::from(count), 3, remaining) else {
            continue;
        };
        let valid = (|| -> Option<usize> {
            let mut pos = start;
            for _ in 0..count {
                let length = View::u16_le_at(payload, pos).map(usize::from)?;
                let string_offset = pos.checked_add(2)?;
                let end = string_offset.checked_add(length)?;
                let raw = payload.get(string_offset..end)?;
                let value = std::str::from_utf8(raw).ok()?;
                if value.is_empty() || value.chars().any(char::is_control) {
                    return None;
                }
                pos = end;
            }
            Some(pos)
        })();
        if valid != Some(payload.len()) {
            continue;
        }
        return Some((marker, count, start));
    }
    None
}

type ExtrefStringTable = (usize, Vec<(usize, String)>);

fn parse_extref_string_table(payload: &[u8]) -> Option<ExtrefStringTable> {
    let (marker, count, start) = locate_extref_string_table(payload)?;
    let mut out = Vec::new();
    let mut pos = start;
    for _ in 0..count {
        let length = View::u16_le_at(payload, pos).map(usize::from)?;
        let string_offset = pos.checked_add(2)?;
        let end = string_offset.checked_add(length)?;
        let raw = payload.get(string_offset..end)?;
        let value = std::str::from_utf8(raw).ok()?;
        out.push((string_offset, value.to_owned()));
        pos = end;
    }
    Some((marker, out))
}

fn parse_extref_records(payload: &[u8]) -> Vec<ExtrefRecord> {
    let Some(index) = parse_extref_record_index(payload) else {
        return Vec::new();
    };
    let parse_record = |record_id, offset, end| -> Option<ExtrefRecord> {
        let bytes = payload.get(offset..end)?;
        if bytes.get(..handle_set::N) != Some(&[1, 0, 0, 0])
            || bytes.get(handle_set::MARKER_A) != Some(&1)
        {
            return None;
        }
        let declared_count = View::u16_be_at(bytes, handle_set::N)?;
        let mut id_slots = [0; 4];
        for (slot, value) in id_slots.iter_mut().enumerate() {
            *value = View::u32_le_at(bytes, handle_set::ID_SLOTS + slot * 4)?;
        }
        if bytes.get(handle_set::MARKER_B) != Some(&1) {
            return None;
        }
        let count = bytes
            .get(handle_set::COUNT)
            .map(|value| usize::from(*value))?;
        if count < 2 {
            return None;
        }
        let handle_token_count = count - 1;
        for handle_index in 0..handle_token_count {
            let token = handle_set::LEN + handle_index * 5;
            if bytes.get(token) != Some(&0xe0) || View::u32_be_at(bytes, token + 1).is_none() {
                return None;
            }
        }
        let mut handles = Vec::new();
        for handle_index in 0..handle_token_count {
            let token = handle_set::LEN + handle_index * 5;
            handles.push(View::u32_be_at(bytes, token + 1)?);
        }
        let handles = ExtrefHandles::new(handles).ok()?;
        let prefix_byte_len = handles.prefix_byte_len();
        let count = u8::try_from(count).ok()?;
        if bytes.get(prefix_byte_len - 1) != Some(&count) {
            return None;
        }
        Some(ExtrefRecord {
            record_id,
            offset,
            declared_count,
            id_slots,
            handles,
            tail_byte_len: bytes.len() - prefix_byte_len,
        })
    };
    index
        .into_iter()
        .filter_map(|record| {
            let end = record.offset.checked_add(record.byte_len)?;
            parse_record(record.record_id, record.offset, end)
        })
        .collect()
}

fn parse_extref_record_index(payload: &[u8]) -> Option<Vec<ExtrefIndexedRecord>> {
    if !payload.starts_with(b"EXTREFSTREAM") || payload.get(24) != Some(&0) {
        return None;
    }
    let (string_table, _, _) = locate_extref_string_table(payload)?;
    let mut directory = Vec::new();
    let mut record_ids = std::collections::BTreeSet::new();
    let mut at = 25usize;
    loop {
        let record_id = View::u32_le_at(payload, at)?;
        at += 4;
        if record_id == 0 {
            break;
        }
        if !record_ids.insert(record_id) {
            return None;
        }
        let offset = usize::try_from(View::u32_le_at(payload, at)?).ok()?;
        at += 4;
        if offset >= string_table {
            return None;
        }
        directory.push((record_id, offset));
    }
    if directory.is_empty()
        || !directory.windows(2).all(|pair| pair[0].1 < pair[1].1)
        || at > directory[0].1
    {
        return None;
    }
    let mut records = Vec::new();
    for (index, (record_id, offset)) in directory.iter().copied().enumerate() {
        let end = directory
            .get(index + 1)
            .map_or(string_table, |(_, offset)| *offset);
        let byte_len = end.checked_sub(offset)?;
        records.push(ExtrefIndexedRecord {
            record_id,
            offset,
            byte_len,
        });
    }
    Some(records)
}

/// Decode the two exact empty indexed-record forms.
pub(crate) fn parse_extref_empty_record(bytes: &[u8]) -> Option<bool> {
    match bytes {
        [1, 0, 0, 0, 0, 1] => Some(false),
        [1, 0, 0, 0, 0, 1, 1] => Some(true),
        _ => None,
    }
}

/// Decode exact adjacent persistent-handle and tagged-reference pairs.
pub(crate) fn parse_extref_reference_pairs(
    bytes: &[u8],
) -> Vec<(usize, u32, crate::om::reference_value::Tagged28)> {
    let mut pairs = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let pair = (|| {
            let bytes = bytes.get(at..)?;
            if bytes.first() != Some(&0xe0) || *bytes.get(5)? & 0xf0 != 0xc0 {
                return None;
            }
            Some((
                View::u32_be_at(bytes, 1)?,
                crate::om::reference_value::Tagged28::from_word(View::u32_be_at(bytes, 5)?),
            ))
        })();
        if let Some((handle, tagged_reference)) = pair {
            pairs.push((at, handle, tagged_reference));
            at += 9;
        } else {
            at += 1;
        }
    }
    pairs
}

/// Layout-specific facts of a parsed NX container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerLayout {
    /// Modern SPLMSSTR framing and its footer metadata.
    Modern {
        /// Version byte at file offset 8.
        version: u8,
        /// File-specific 24-bit little-endian value at offset 9.
        file_tag: u32,
        /// `FOOTER` region offset.
        footer_offset: u64,
        /// Exact four-byte value following the counted FOOTER directory.
        footer_fingerprint: [u8; 4],
    },
    /// Legacy Compound File Binary envelope.
    LegacyCfb {
        /// Version byte in the UGII payload.
        version: u8,
    },
}

impl ContainerLayout {
    /// Version byte selected by this container's grammar.
    pub(crate) fn version(self) -> u8 {
        match self {
            Self::Modern { version, .. } | Self::LegacyCfb { version, .. } => version,
        }
    }
}

#[cfg(test)]
pub(crate) fn test_modern_layout(version: u8) -> ContainerLayout {
    ContainerLayout::Modern {
        version,
        file_tag: 0,
        footer_offset: 0,
        footer_fingerprint: [0; 4],
    }
}

/// A parsed NX container and its directory entries.
#[derive(Debug, Clone)]
pub(crate) struct Container<'a> {
    /// The source image, or the materialized logical stream image for legacy CFB.
    pub(crate) data: SourceImage<'a>,
    /// Physical source-image length before legacy CFB stream materialization.
    pub(crate) physical_size: u64,
    /// Facts owned by the container grammar that parsed the source.
    pub(crate) layout: ContainerLayout,
    /// Modern entries from both regions or legacy CFB paths, in serialized
    /// order.
    pub(crate) entries: Vec<DirEntry>,
    /// Admitted active-object table, shared by selection and native extraction.
    pub(crate) fastload_table: Option<(usize, RmFastLoadObjectIdTable)>,
    /// Cached source ranges for indexed object-model sections.
    pub(crate) indexed_section_layouts: OnceLock<IndexedSectionCache<'a>>,
    /// Cached size-framed object-model sections when the container borrows its input.
    pub(crate) om_section_cache: OnceLock<FramedSectionCache<'a>>,
}

/// Parsed indexed sections borrow the input or retain an immutable source
/// owner with checked ranges when the container owns its input.
#[derive(Debug, Clone)]
pub(crate) enum IndexedSectionCache<'a> {
    Borrowed {
        sections: Vec<(usize, crate::om::IndexedSection<'a>)>,
        blocks: BTreeMap<String, (&'a [u8], u64)>,
    },
    Owned {
        layouts: Vec<(usize, crate::om::cache::IndexedSectionLayout)>,
    },
}

/// Parsed size-framed sections retained when their source bytes are borrowed
/// from the container input, or checked ranges with an immutable source owner
/// when the container owns its input bytes.
#[derive(Debug, Clone)]
pub(crate) enum FramedSectionCache<'a> {
    Borrowed {
        sections: Vec<(usize, crate::om::Section<'a>)>,
    },
    Owned {
        layouts: Vec<(usize, crate::om::cache::SectionLayout)>,
    },
}

type FramedSections<'a> = Vec<(usize, crate::om::Section<'a>)>;
type FramedSectionLayouts = Vec<(usize, crate::om::cache::SectionLayout)>;

fn offset_block_key(section: usize, block: usize) -> String {
    format!("nx:om-data-blocks-{section}:block#{block}")
}

fn parse_framed_section_cache<'bytes>(
    bytes: &'bytes [u8],
    entries: &[DirEntry],
    retain_layouts: bool,
) -> Result<(FramedSections<'bytes>, FramedSectionLayouts), CodecError> {
    let mut sections = Vec::new();
    let mut layouts = Vec::new();
    for (entry_index, entry) in entries.iter().enumerate() {
        let Some((offset, size)) = entry.file_span() else {
            continue;
        };
        let (Ok(offset), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
            continue;
        };
        let Some(end) = offset.checked_add(size) else {
            continue;
        };
        let Some(payload) = bytes.get(offset..end) else {
            continue;
        };
        let parsed = crate::om::sections(payload)?;
        if parsed.is_empty() {
            continue;
        }
        let source = if retain_layouts {
            Some(std::sync::Arc::<[u8]>::from(payload))
        } else {
            None
        };
        for section in parsed {
            if let Some(source) = &source {
                let Some(layout) = crate::om::cache::SectionLayout::from_section(&section, source)
                else {
                    continue;
                };
                layouts.push((entry_index, layout));
            }
            sections.push((entry_index, section));
        }
    }
    Ok((sections, layouts))
}

type IndexedSections<'a> = Vec<(usize, crate::om::IndexedSection<'a>)>;
type IndexedSectionLayouts = Vec<(usize, crate::om::cache::IndexedSectionLayout)>;

fn parse_indexed_section_cache<'bytes>(
    bytes: &'bytes [u8],
    entries: &[DirEntry],
    retain_layouts: bool,
) -> Result<(IndexedSections<'bytes>, IndexedSectionLayouts), CodecError> {
    let mut sections = Vec::new();
    let mut layouts = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (entry_index, entry) in entries.iter().enumerate() {
        let Some((offset, size)) = entry.file_span() else {
            continue;
        };
        let (Ok(offset), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
            continue;
        };
        let Some(end) = offset.checked_add(size) else {
            continue;
        };
        let Some(payload) = bytes.get(offset..end) else {
            continue;
        };
        let parsed = crate::om::indexed_sections(payload)?;
        if parsed.is_empty() {
            continue;
        }
        let source = if retain_layouts {
            Some(std::sync::Arc::<[u8]>::from(payload))
        } else {
            None
        };
        for section in parsed {
            if !seen.insert((offset, section.object_id_table_offset)) {
                continue;
            }
            if let Some(source) = &source {
                let Some(layout) =
                    crate::om::cache::IndexedSectionLayout::from_section(&section, source)
                else {
                    continue;
                };
                layouts.push((entry_index, layout));
            }
            sections.push((entry_index, section));
        }
    }
    Ok((sections, layouts))
}

/// Return whether `prefix` starts with [`MAGIC`].
pub(crate) fn looks_like_nx(prefix: &[u8]) -> bool {
    prefix.starts_with(MAGIC)
}

/// Return whether a CFB prefix contains the NX directory evidence required for
/// legacy detection.
///
/// The CFB signature alone is not sufficient: Inventor and other CAD formats
/// use the same envelope. Requiring the canonical `UG_PART/UG_PART` path keeps
/// detection tied to the NX payload namespace.
pub(crate) fn looks_like_legacy_nx(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
) -> Result<bool, CodecError> {
    let (probe, _storage) =
        CompoundPrefixProbe::inspect_with_context(ctx, View::over_retained(prefix))?;
    let CompoundPrefixProbe::DirectoryEvidence(paths) = probe else {
        return Ok(false);
    };
    for path in &paths {
        if path.eq_ignore_ascii_case("UG_PART/UG_PART") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn u24_le(d: &[u8], at: usize) -> u32 {
    if at + 3 > d.len() {
        return 0;
    }
    u32::from(d[at]) | (u32::from(d[at + 1]) << 8) | (u32::from(d[at + 2]) << 16)
}

fn u48_le(d: &[u8], at: usize) -> u64 {
    let mut v = 0u64;
    for i in 0..6 {
        if at + i < d.len() {
            v |= u64::from(d[at + i]) << (8 * i);
        }
    }
    v
}

/// Parse an SPLMSSTR file image.
pub(crate) fn scan_bytes<'a>(
    data: impl Into<SourceImage<'a>>,
) -> Result<Container<'a>, CodecError> {
    let data = data.into();
    if !data.starts_with(MAGIC) {
        return Err(CodecError::WrongFormat(
            "missing SPLMSSTR magic".to_string(),
        ));
    }
    let version = data
        .get(splmsstr::VERSION_TAG)
        .copied()
        .ok_or_else(|| CodecError::Malformed("SPLMSSTR header has no version byte".to_string()))?;
    if version != 0x06 {
        return Err(CodecError::malformed(format_args!(
            "SPLMSSTR version byte must be 0x06, found 0x{version:02x}"
        )));
    }
    let file_tag = u24_le(&data, splmsstr::FILE_TAG);
    let footer_offset = u48_le(&data, splmsstr::FOOTER_OFFSET);
    let fo = usize::try_from(footer_offset)
        .map_err(|_| CodecError::Malformed("FOOTER offset exceeds address space".to_string()))?;
    if fo > data.len() {
        return Err(CodecError::Malformed(
            "FOOTER offset exceeds the file image".to_string(),
        ));
    }
    let footer_directory_end = data
        .len()
        .checked_sub(4)
        .ok_or_else(|| CodecError::Malformed("truncated FOOTER fingerprint".to_string()))?;

    let (mut entries, header_end) = directory_region(
        &data,
        splmsstr::HEADER_MARKER,
        *b"HEADER",
        Region::Header,
        fo,
    )?;
    let (footer_entries, footer_end) =
        directory_region(&data, fo, *b"FOOTER", Region::Footer, footer_directory_end)?;
    entries.extend(footer_entries);
    if header_end > fo {
        return Err(CodecError::Malformed(
            "HEADER directory overlaps the FOOTER region".to_string(),
        ));
    }
    if entries
        .iter()
        .filter_map(crate::container::DirEntry::file_span)
        .any(|(offset, size)| {
            offset
                .checked_add(size)
                .is_none_or(|end| end > footer_offset)
        })
    {
        return Err(CodecError::Malformed(
            "directory file span extends into the FOOTER region".to_string(),
        ));
    }
    let fingerprint_end = footer_end
        .checked_add(4)
        .ok_or_else(|| CodecError::Malformed("FOOTER fingerprint offset overflow".to_string()))?;
    // The fingerprint is the last four bytes, which is what makes it an array
    // here rather than a slice that a later read has to measure again.
    let Some(footer_fingerprint) = data
        .last_chunk::<4>()
        .copied()
        .filter(|_| fingerprint_end == data.len())
    else {
        return Err(CodecError::Malformed(
            "counted FOOTER directory is not followed by exactly four bytes".to_string(),
        ));
    };
    let physical_size = cadmpeg_core::decode::u64_from_index(data.len());

    let mut container = Container {
        data,
        physical_size,
        layout: ContainerLayout::Modern {
            version,
            file_tag,
            footer_offset,
            footer_fingerprint,
        },
        entries,
        fastload_table: None,
        indexed_section_layouts: OnceLock::new(),
        om_section_cache: OnceLock::new(),
    };
    container.fastload_table = container.parse_rmfastload_object_id_table()?;
    Ok(container)
}

/// Open the legacy NX `UG_PART/UG_PART` stream from a validated CFB source.
pub(crate) fn scan_legacy<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<(Container<'a>, View<'a>), CodecError> {
    let snapshot = CompoundSnapshot::new(ctx, root)?;
    let part = snapshot
        .stream("UG_PART/UG_PART")
        .ok_or_else(|| CodecError::WrongFormat("missing legacy UG_PART/UG_PART stream".into()))?;
    let part_view = snapshot.open(ctx, part)?;
    let payload_prefix = part_view
        .window()
        .get(..legacy_ugii_payload_prefix::LEN)
        .ok_or_else(|| {
            CodecError::WrongFormat("legacy UG_PART/UG_PART stream has no UGII prefix".into())
        })?;
    if payload_prefix[..legacy_ugii_payload_prefix::VERSION] != LEGACY_UGII_PREFIX[..] {
        return Err(CodecError::WrongFormat(
            "legacy UG_PART/UG_PART stream is not a UGII payload".into(),
        ));
    }
    let part_id = part.id();
    let mut stream_views = Vec::new();
    let mut stream_spans = BTreeMap::new();
    let mut logical_offset = 0_u64;
    for entry in snapshot.entries() {
        let CompoundEntry::Stream(stream) = entry else {
            continue;
        };
        let view = if stream.id() == part_id {
            part_view
        } else {
            snapshot.open(ctx, stream)?
        };
        let byte_len = u64::try_from(view.window().len())
            .map_err(|_| CodecError::Malformed("legacy CFB stream exceeds u64".into()))?;
        let span = (logical_offset, byte_len);
        logical_offset = logical_offset
            .checked_add(byte_len)
            .ok_or_else(|| CodecError::Malformed("legacy CFB logical image overflows".into()))?;
        stream_spans.insert(stream.id(), span);
        stream_views.push(view);
    }
    let logical_data = ctx.concat_views(&stream_views)?;
    let mut entries = Vec::new();
    for entry in snapshot.entries() {
        let body = match entry {
            CompoundEntry::Stream(stream) => stream_spans
                .get(&stream.id())
                .map_or(DirEntryBody::Directory, |&(offset, len)| {
                    DirEntryBody::File { offset, len }
                }),
            CompoundEntry::Storage(_) => DirEntryBody::Directory,
        };
        entries.push(DirEntry {
            name: format!("/Root/{}", entry.path()),
            region: Region::Header,
            body,
        });
    }
    let version = payload_prefix[legacy_ugii_payload_prefix::VERSION];
    let mut container = Container {
        data: SourceImage::Borrowed(logical_data),
        physical_size: cadmpeg_core::decode::u64_from_index(root.window().len()),
        layout: ContainerLayout::LegacyCfb { version },
        entries,
        fastload_table: None,
        indexed_section_layouts: OnceLock::new(),
        om_section_cache: OnceLock::new(),
    };
    container.fastload_table = container.parse_rmfastload_object_id_table()?;
    Ok((container, part_view))
}

fn directory_region(
    data: &[u8],
    marker_offset: usize,
    marker: [u8; 6],
    region: Region,
    region_end: usize,
) -> Result<(Vec<DirEntry>, usize), CodecError> {
    let count_offset = marker_offset
        .checked_add(marker.len())
        .ok_or_else(|| CodecError::Malformed("directory marker offset overflow".to_string()))?;
    let entries_offset = count_offset
        .checked_add(4)
        .ok_or_else(|| CodecError::Malformed("directory count offset overflow".to_string()))?;
    if data.get(marker_offset..count_offset) != Some(marker.as_slice()) {
        return Err(CodecError::malformed(format_args!(
            "missing {} directory marker",
            String::from_utf8_lossy(&marker)
        )));
    }
    let count = View::u32_le_at(data, count_offset)
        .ok_or_else(|| CodecError::Malformed("truncated directory entry count".to_string()))?;
    let capacity = usize::try_from(count)
        .map_err(|_| CodecError::Malformed("directory entry count exceeds address space".into()))?;
    let available = region_end.checked_sub(entries_offset).ok_or_else(|| {
        CodecError::Malformed("directory marker extends beyond its region".to_string())
    })?;
    if capacity > available / 26 {
        return Err(CodecError::Malformed(
            "directory entry count exceeds its bounded region".to_string(),
        ));
    }
    let mut entries = Vec::new();
    let mut at = entries_offset;
    for ordinal in 0..count {
        let Some((entry, next)) = try_entry(data, at, region, region_end, ordinal)? else {
            return Err(CodecError::malformed(format_args!(
                "directory entry {ordinal} is truncated or malformed"
            )));
        };
        entries.push(entry);
        at = next;
    }
    Ok((entries, at))
}

/// Try to read one directory entry at `o`: `name_len:u32 LE`, then that many bytes
/// of printable ASCII beginning `/Root`, then a 16-byte payload. Returns the entry
/// and the offset just past its payload.
fn try_entry(
    data: &[u8],
    o: usize,
    region: Region,
    region_end: usize,
    ordinal: u32,
) -> Result<Option<(DirEntry, usize)>, CodecError> {
    let Some(name_len) = View::u32_le_at(data, o).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if !(6..=128).contains(&name_len) {
        return Ok(None);
    }
    let Some(name_start) = o.checked_add(dir_entry::LEN) else {
        return Ok(None);
    };
    let Some(name_end) = name_start.checked_add(name_len) else {
        return Ok(None);
    };
    let Some(raw) = data.get(name_start..name_end) else {
        return Ok(None);
    };
    if !raw.starts_with(b"/Root") || !raw.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        return Ok(None);
    }
    let payload = name_end;
    let Some(next) = payload.checked_add(file_payload::LEN) else {
        return Ok(None);
    };
    if next > region_end {
        return Err(CodecError::malformed(format_args!(
            "directory entry {ordinal} extends beyond its bounded region"
        )));
    }

    let Ok(value) = std::str::from_utf8(raw) else {
        return Ok(None);
    };
    let name = value.to_owned();
    // Interpret the 16-byte payload as a file span when it lands within the file.
    let body = match (
        View::u64_le_at(data, payload),
        View::u64_le_at(data, payload + file_payload::SIZE),
    ) {
        (Some(off), Some(size)) => {
            let end = off.checked_add(size);
            match end {
                Some(e)
                    if size > 0
                        && e <= cadmpeg_core::decode::u64_from_index(data.len())
                        && off >= 8 =>
                {
                    DirEntryBody::File {
                        offset: off,
                        len: size,
                    }
                }
                _ => DirEntryBody::Directory,
            }
        }
        _ => DirEntryBody::Directory,
    };
    Ok(Some((DirEntry { name, region, body }, next)))
}

#[cfg(test)]
mod tests;
