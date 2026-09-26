// SPDX-License-Identifier: Apache-2.0
//! Single-pass ZIP metadata snapshots with budgeted entry opening.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use cadmpeg_core::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use zip::{CompressionMethod, HasZipMetadata};

/// Compression methods supported by [`ArchiveSnapshot::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipCompression {
    /// The entry payload is stored directly in the archive.
    Stored,
    /// The entry payload is a raw-DEFLATE member.
    Deflate,
    /// The entry payload is a Zstandard frame.
    Zstd,
}

impl ZipCompression {
    fn from_zip(method: CompressionMethod, name: &str) -> Result<Self, CodecError> {
        match method {
            CompressionMethod::Stored => Ok(Self::Stored),
            CompressionMethod::Deflated => Ok(Self::Deflate),
            CompressionMethod::Zstd => Ok(Self::Zstd),
            other => Err(CodecError::NotImplemented(format!(
                "ZIP compression {other:?} for {name}"
            ))),
        }
    }

    /// Returns the stable container-summary label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Deflate => "deflate",
            Self::Zstd => "zstd",
        }
    }

    /// Storage of a member with these declared stored and expanded sizes.
    ///
    /// A stored member whose declared compressed size is smaller than its
    /// uncompressed size is a malformed central-directory record, reported here
    /// rather than normalized into a legal span.
    pub fn storage(
        self,
        compressed_size: u64,
        uncompressed_size: u64,
    ) -> Result<cadmpeg_core::container::EntryStorage, &'static str> {
        use cadmpeg_core::container::{CompressionMethod, EntryStorage, VerbatimLabel};
        let method = match self {
            Self::Stored => {
                return EntryStorage::framed(
                    VerbatimLabel::Stored,
                    uncompressed_size,
                    compressed_size,
                )
            }
            Self::Deflate => CompressionMethod::Deflate,
            Self::Zstd => CompressionMethod::Zstd,
        };
        Ok(EntryStorage::Compressed {
            method,
            stored: Some(compressed_size),
            expanded: Some(uncompressed_size),
        })
    }
}

/// ZIP central-directory facts retained after the parser is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRecord {
    /// Entry name as stored in the central directory.
    pub name: String,
    /// Compression method admitted by the snapshot.
    pub compression: ZipCompression,
    /// CRC-32 of the uncompressed payload.
    pub crc32: u32,
    /// Compressed payload size.
    pub compressed_size: u64,
    /// Uncompressed payload size.
    pub uncompressed_size: u64,
    /// Physical start of the local header.
    pub header_start: u64,
    /// Physical start of the compressed payload.
    pub data_start: u64,
    /// Physical start of the central-directory record.
    pub central_start: u64,
    utf8_name: bool,
}

impl EntryRecord {
    /// Returns whether ZIP metadata uses Unicode filename support for this entry.
    pub fn uses_utf8_name_encoding(&self) -> bool {
        self.utf8_name
    }

    /// Returns the exclusive compressed-payload boundary.
    pub fn data_end(&self) -> Result<u64, CodecError> {
        self.data_start
            .checked_add(self.compressed_size)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("ZIP data range overflows for {}", self.name))
            })
    }
}

/// A ZIP central-directory snapshot over one decode-session root view.
#[derive(Debug)]
pub struct ArchiveSnapshot<'a> {
    root: View<'a>,
    central_start: u64,
    entries: Vec<EntryRecord>,
    by_name: BTreeMap<String, usize>,
}

impl<'a> ArchiveSnapshot<'a> {
    /// Parses the central directory once and retains replayable physical facts.
    pub fn new(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        preflight_central_directory(ctx, root.window())?;
        let mut archive = zip::ZipArchive::new(Cursor::new(root.window()))
            .map_err(|error| CodecError::malformed(format_args!("not a readable ZIP: {error}")))?;
        let archive_central_start = archive.central_directory_start();
        ctx.charge_collection_items(archive.len() as u64, "ZIP duplicate name set")?;
        let central_entry_count =
            reject_duplicate_central_names(root.window(), archive_central_start)?;
        if central_entry_count != archive.len() {
            return Err(CodecError::Malformed(
                "ZIP central directory contains duplicate entry names".into(),
            ));
        }
        let mut names = BTreeSet::new();
        ctx.charge_collection_items(archive.len() as u64, "ZIP entry records")?;
        ctx.charge_collection_items(archive.len() as u64, "ZIP decoded name set")?;
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let file = archive.by_index(index).map_err(|error| {
                CodecError::malformed(format_args!("bad ZIP entry {index}: {error}"))
            })?;
            let name_len = file.name().len() as u64;
            ctx.charge_retained(name_len, "ZIP entry record name")?;
            ctx.charge_retained(name_len, "ZIP duplicate name")?;
            let name = file.name().to_owned();
            if !names.insert(name.clone()) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate ZIP entry name {name}"
                )));
            }
            if file.encrypted() {
                return Err(CodecError::malformed(format_args!(
                    "encrypted ZIP entry {name}"
                )));
            }
            let compression = ZipCompression::from_zip(file.compression(), &name)?;
            let data_start = file.data_start().ok_or_else(|| {
                CodecError::malformed(format_args!("missing data offset for {name}"))
            })?;
            let record = EntryRecord {
                name,
                compression,
                crc32: file.crc32(),
                compressed_size: file.compressed_size(),
                uncompressed_size: file.size(),
                header_start: file.header_start(),
                data_start,
                central_start: file.central_header_start(),
                utf8_name: file.get_metadata().is_utf8,
            };
            for offset in [
                record.header_start,
                record.data_start,
                record.data_end()?,
                record.central_start,
            ] {
                if offset > root.window().len() as u64 {
                    return Err(CodecError::malformed(format_args!(
                        "ZIP offset outside archive for {}",
                        record.name
                    )));
                }
            }
            entries.push(record);
        }
        drop(archive);
        ctx.charge_collection_items(entries.len() as u64, "ZIP name index")?;
        for entry in &entries {
            ctx.charge_retained(entry.name.len() as u64, "ZIP indexed entry name")?;
        }
        let by_name = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.name.clone(), index))
            .collect();
        Ok(Self {
            root,
            central_start: archive_central_start,
            entries,
            by_name,
        })
    }

    /// Returns central-directory records in archive order.
    pub fn entries(&self) -> &[EntryRecord] {
        &self.entries
    }

    /// Finds an entry record by its exact archive name.
    pub fn entry(&self, name: &str) -> Option<&EntryRecord> {
        self.by_name.get(name).map(|index| &self.entries[*index])
    }

    /// Opens an exact entry name as a borrowed stored slice or budgeted expanded view.
    pub fn open(&self, ctx: &DecodeContext<'a>, name: &str) -> Result<View<'a>, CodecError> {
        let entry = self
            .entry(name)
            .ok_or_else(|| CodecError::malformed(format_args!("ZIP entry {name} is absent")))?;
        let end = entry.data_end()?;
        let archive_start = u64::try_from(self.root.start())
            .map_err(|_| CodecError::Malformed("ZIP root offset does not fit u64".into()))?;
        let absolute_start = archive_start.checked_add(entry.data_start).ok_or_else(|| {
            CodecError::malformed(format_args!("ZIP data range overflows for {}", entry.name))
        })?;
        let absolute_end = archive_start.checked_add(end).ok_or_else(|| {
            CodecError::malformed(format_args!("ZIP data range overflows for {}", entry.name))
        })?;
        let range = ByteRange {
            start: absolute_start,
            end: absolute_end,
        };
        match entry.compression {
            ZipCompression::Stored => self.open_stored(ctx, entry, range),
            ZipCompression::Deflate => {
                let source = self.compressed_source(entry, range)?;
                let mut decoder = flate2::read::DeflateDecoder::new(source.window());
                let view = Self::open_expanded(ctx, entry, &mut decoder)?;
                if decoder.total_in() != source.window().len() as u64 {
                    return Err(CodecError::Malformed(
                        "raw-DEFLATE member does not exhaust its declared ZIP payload".into(),
                    ));
                }
                Ok(view)
            }
            ZipCompression::Zstd => {
                let source = self.compressed_source(entry, range)?;
                let decoder =
                    zstd::stream::read::Decoder::with_buffer(source.window()).map_err(|error| {
                        CodecError::malformed(format_args!(
                            "cannot open Zstandard frame for {}: {error}",
                            entry.name
                        ))
                    })?;
                Self::open_expanded(ctx, entry, decoder)
            }
        }
    }

    fn compressed_source(
        &self,
        entry: &EntryRecord,
        range: ByteRange,
    ) -> Result<View<'a>, CodecError> {
        let start = usize::try_from(range.start)
            .map_err(|_| CodecError::Malformed("ZIP data offset does not fit memory".into()))?;
        let end = usize::try_from(range.end)
            .map_err(|_| CodecError::Malformed("ZIP data offset does not fit memory".into()))?;
        self.root.child(start, end).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "ZIP data range escapes archive for {}",
                entry.name
            ))
        })
    }

    fn open_stored(
        &self,
        ctx: &DecodeContext<'a>,
        entry: &EntryRecord,
        range: ByteRange,
    ) -> Result<View<'a>, CodecError> {
        let view = ctx.register_slice(self.root, range)?;
        if view.window().len() as u64 != entry.uncompressed_size {
            return Err(CodecError::malformed(format_args!(
                "stored size mismatch for {}",
                entry.name
            )));
        }
        if crc32fast::hash(view.window()) != entry.crc32 {
            return Err(CodecError::malformed(format_args!(
                "CRC mismatch for {}",
                entry.name
            )));
        }
        Ok(view)
    }

    fn open_expanded(
        ctx: &DecodeContext<'a>,
        entry: &EntryRecord,
        mut decoder: impl Read,
    ) -> Result<View<'a>, CodecError> {
        let mut writer = ctx.begin_expand(ExpandSpec::Exact(entry.uncompressed_size))?;
        let mut chunk = [0_u8; 16 * 1024];
        loop {
            let read = decoder.read(&mut chunk).map_err(|error| {
                CodecError::malformed(format_args!("cannot inflate {}: {error}", entry.name))
            })?;
            if read == 0 {
                break;
            }
            writer.write(&chunk[..read])?;
        }
        let view = writer.finalize()?;
        if crc32fast::hash(view.window()) != entry.crc32 {
            return Err(CodecError::malformed(format_args!(
                "CRC mismatch for {}",
                entry.name
            )));
        }
        Ok(view)
    }

    /// Builds generic entry summaries using a codec-owned role classifier.
    pub fn container_entries(
        &self,
        classify: impl Fn(&str) -> cadmpeg_core::container::ContainerRole,
    ) -> Vec<ContainerEntry> {
        self.entries
            .iter()
            .map(|entry| {
                let mut attributes = BTreeMap::new();
                attributes.insert("crc32".into(), format!("{:08x}", entry.crc32));
                attributes.insert("header_offset".into(), entry.header_start.to_string());
                attributes.insert("data_offset".into(), entry.data_start.to_string());
                attributes.insert(
                    "central_header_offset".into(),
                    entry.central_start.to_string(),
                );
                let storage = declared_storage(
                    entry.compression,
                    entry.compressed_size,
                    entry.uncompressed_size,
                    &mut attributes,
                );
                ContainerEntry {
                    name: entry.name.clone(),
                    role: classify(&entry.name),
                    storage,
                    attributes,
                }
            })
            .collect()
    }

    /// Partitions every physical archive byte by ZIP structural role.
    pub fn physical_ledger(&self) -> Result<Vec<PhysicalSpan>, CodecError> {
        physical_ledger(self.root.window(), &self.entries, self.central_start)
    }
}

fn preflight_central_directory(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(), CodecError> {
    let mut first_error = None;
    let mut max_count = None::<u64>;
    let mut max_name_bytes = 0_u64;
    for (end, signature) in bytes.windows(4).enumerate().rev() {
        if signature != b"PK\x05\x06" {
            continue;
        }
        ctx.charge_work(1, "ZIP end record candidate")?;
        match central_directory_inventory(ctx, bytes, end) {
            Ok((count, name_bytes)) => {
                max_count = Some(max_count.map_or(count, |current| current.max(count)));
                max_name_bytes = max_name_bytes.max(name_bytes);
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    let count = max_count.ok_or_else(|| {
        first_error.unwrap_or_else(|| CodecError::Malformed("ZIP end record is absent".into()))
    })?;
    ctx.charge_collection_items(count, "ZIP central directory entries")?;
    ctx.charge_retained(max_name_bytes, "ZIP library indexed names")?;
    Ok(())
}

fn central_directory_inventory(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    end: usize,
) -> Result<(u64, u64), CodecError> {
    let comment_len = View::u16_le_at(bytes, end + 20)
        .ok_or_else(|| CodecError::Malformed("ZIP end record is truncated".into()))?;
    if end
        .checked_add(22 + usize::from(comment_len))
        .is_none_or(|record_end| record_end > bytes.len())
    {
        return Err(CodecError::Malformed("ZIP end comment is truncated".into()));
    }
    let count = View::u16_le_at(bytes, end + 10)
        .ok_or_else(|| CodecError::Malformed("ZIP end record is truncated".into()))?;
    let (count, directory_size, directory_start_hint, directory_end) = if count == u16::MAX {
        if let Some(locator_start) = end
            .checked_sub(20)
            .filter(|&start| bytes.get(start..start + 4) == Some(b"PK\x06\x07".as_slice()))
        {
            let record_start = bytes[..locator_start]
                .windows(4)
                .rposition(|signature| signature == b"PK\x06\x06")
                .ok_or_else(|| CodecError::Malformed("ZIP64 end record is absent".into()))?;
            let record_size = View::u64_le_at(bytes, record_start + 4)
                .ok_or_else(|| CodecError::Malformed("ZIP64 end record is truncated".into()))?;
            let record_len = usize::try_from(record_size)
                .ok()
                .and_then(|size| record_start.checked_add(12)?.checked_add(size));
            if record_len != Some(locator_start) {
                return Err(CodecError::Malformed(
                    "ZIP64 end record size is invalid".into(),
                ));
            }
            let count = View::u64_le_at(bytes, record_start + 32)
                .ok_or_else(|| CodecError::Malformed("ZIP64 entry count is truncated".into()))?;
            let size = View::u64_le_at(bytes, record_start + 40)
                .ok_or_else(|| CodecError::Malformed("ZIP64 directory size is truncated".into()))?;
            let start = View::u64_le_at(bytes, record_start + 48).ok_or_else(|| {
                CodecError::Malformed("ZIP64 directory offset is truncated".into())
            })?;
            (count, size, start, record_start)
        } else {
            let size = View::u32_le_at(bytes, end + 12)
                .ok_or_else(|| CodecError::Malformed("ZIP directory size is truncated".into()))?;
            let start = View::u32_le_at(bytes, end + 16)
                .ok_or_else(|| CodecError::Malformed("ZIP directory offset is truncated".into()))?;
            (u64::from(count), u64::from(size), u64::from(start), end)
        }
    } else {
        let size = View::u32_le_at(bytes, end + 12)
            .ok_or_else(|| CodecError::Malformed("ZIP directory size is truncated".into()))?;
        let start = View::u32_le_at(bytes, end + 16)
            .ok_or_else(|| CodecError::Malformed("ZIP directory offset is truncated".into()))?;
        (u64::from(count), u64::from(size), u64::from(start), end)
    };
    let directory_end = u64::try_from(directory_end)
        .map_err(|_| CodecError::Malformed("ZIP directory end exceeds u64".into()))?;
    let canonical_start = directory_end
        .checked_sub(directory_size)
        .ok_or_else(|| CodecError::Malformed("ZIP directory size exceeds archive".into()))?;
    let mut offset = if count == 0 || signature_at(bytes, canonical_start) == Some(*b"PK\x01\x02") {
        canonical_start
    } else {
        let search_start = usize::try_from(directory_start_hint).map_err(|_| {
            CodecError::Malformed("ZIP directory offset does not fit memory".into())
        })?;
        let search_end = usize::try_from(directory_end)
            .map_err(|_| CodecError::Malformed("ZIP directory end does not fit memory".into()))?;
        let search_len = search_end
            .checked_sub(search_start)
            .ok_or_else(|| CodecError::Malformed("ZIP directory search range is invalid".into()))?;
        let search_work = u64::try_from(search_len)
            .map_err(|_| CodecError::Malformed("ZIP directory search exceeds u64".into()))?;
        ctx.charge_work(search_work, "ZIP central header search")?;
        let start = bytes
            .get(search_start..search_end)
            .and_then(|range| range.windows(4).position(|window| window == b"PK\x01\x02"))
            .and_then(|relative| search_start.checked_add(relative))
            .ok_or_else(|| CodecError::Malformed("ZIP central header is absent".into()))?;
        u64::try_from(start)
            .map_err(|_| CodecError::Malformed("ZIP directory offset exceeds u64".into()))?
    };
    let mut indexed_name_bytes = 0_u64;
    for _ in 0..count {
        ctx.charge_work(1, "ZIP central header preflight")?;
        if signature_at(bytes, offset) != Some(*b"PK\x01\x02") {
            return Err(CodecError::Malformed("ZIP central header is absent".into()));
        }
        let name_len = u64::from(u16_at(bytes, offset + 28)?);
        let extra_len = u64::from(u16_at(bytes, offset + 30)?);
        let comment_len = u64::from(u16_at(bytes, offset + 32)?);
        let name_start = offset
            .checked_add(46)
            .ok_or_else(|| CodecError::Malformed("ZIP central-header offset overflow".into()))?;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or_else(|| CodecError::Malformed("ZIP central-name offset overflow".into()))?;
        offset = name_end
            .checked_add(extra_len)
            .and_then(|value| value.checked_add(comment_len))
            .ok_or_else(|| CodecError::Malformed("ZIP central-record offset overflow".into()))?;
        if offset > directory_end {
            return Err(CodecError::Malformed(
                "ZIP central record exceeds directory".into(),
            ));
        }
        let name = usize::try_from(name_start)
            .ok()
            .zip(usize::try_from(name_end).ok())
            .and_then(|(start, end)| bytes.get(start..end))
            .ok_or_else(|| CodecError::Malformed("truncated ZIP central name".into()))?;
        let decoded_upper_bound = if name.is_ascii() {
            name_len
        } else {
            name_len
                .checked_mul(3)
                .ok_or_else(|| CodecError::Malformed("ZIP indexed name length overflow".into()))?
        };
        indexed_name_bytes = indexed_name_bytes
            .checked_add(decoded_upper_bound)
            .ok_or_else(|| CodecError::Malformed("ZIP indexed names length overflow".into()))?;
    }
    Ok((count, indexed_name_bytes))
}

fn reject_duplicate_central_names(bytes: &[u8], central_start: u64) -> Result<usize, CodecError> {
    let mut offset = central_start;
    let mut names = BTreeSet::new();
    let mut entry_count = 0;
    while signature_at(bytes, offset) == Some(*b"PK\x01\x02") {
        entry_count += 1;
        let fixed_end = offset
            .checked_add(46)
            .ok_or_else(|| CodecError::Malformed("ZIP central-header offset overflow".into()))?;
        let name_len = u64::from(u16_at(bytes, offset + 28)?);
        let extra_len = u64::from(u16_at(bytes, offset + 30)?);
        let comment_len = u64::from(u16_at(bytes, offset + 32)?);
        let name_end = fixed_end
            .checked_add(name_len)
            .ok_or_else(|| CodecError::Malformed("ZIP central-name offset overflow".into()))?;
        let record_end = name_end
            .checked_add(extra_len)
            .and_then(|end| end.checked_add(comment_len))
            .ok_or_else(|| CodecError::Malformed("ZIP central-record offset overflow".into()))?;
        let name_start = usize::try_from(fixed_end).map_err(|_| {
            CodecError::Malformed("ZIP central-name offset does not fit memory".into())
        })?;
        let name_end = usize::try_from(name_end).map_err(|_| {
            CodecError::Malformed("ZIP central-name end does not fit memory".into())
        })?;
        let name = bytes
            .get(name_start..name_end)
            .ok_or_else(|| CodecError::Malformed("truncated ZIP central name".into()))?;
        if !names.insert(name) {
            return Err(CodecError::Malformed(
                "duplicate ZIP central entry name".into(),
            ));
        }
        offset = record_end;
    }
    Ok(entry_count)
}

/// The structural role of a ZIP physical range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipSpanRole {
    /// ZIP local-header signature for the named entry.
    LocalSignature(String),
    /// ZIP local-header fields for the named entry.
    LocalFields(String),
    /// ZIP local-header name for the named entry.
    LocalName(String),
    /// ZIP local-header extra data for the named entry.
    LocalExtra(String),
    /// ZIP compressed payload for the named entry.
    CompressedPayload(String),
    /// ZIP data descriptor for the named entry.
    DataDescriptor(String),
    /// ZIP padding, optionally owned by an entry.
    Padding {
        /// Owning entry, when the padding belongs to one.
        entry: Option<String>,
    },
    /// ZIP central-header signature for the named entry.
    CentralSignature(String),
    /// ZIP central-header fields for the named entry.
    CentralFields(String),
    /// ZIP central-header name for the named entry.
    CentralName(String),
    /// ZIP central-header extra data for the named entry.
    CentralExtra(String),
    /// ZIP central-header comment for the named entry.
    CentralComment(String),
    /// ZIP64 end-of-central-directory record.
    Zip64EndRecord,
    /// ZIP64 end-of-central-directory locator.
    Zip64EndLocator,
    /// ZIP end-of-central-directory record.
    EndRecord,
}

impl ZipSpanRole {
    const fn label(&self) -> &'static str {
        match self {
            Self::LocalSignature(_) => "local-signature",
            Self::LocalFields(_) => "local-fields",
            Self::LocalName(_) => "local-name",
            Self::LocalExtra(_) => "local-extra",
            Self::CompressedPayload(_) => "compressed-payload",
            Self::DataDescriptor(_) => "data-descriptor",
            Self::Padding { .. } => "archive-padding",
            Self::CentralSignature(_) => "central-signature",
            Self::CentralFields(_) => "central-fields",
            Self::CentralName(_) => "central-name",
            Self::CentralExtra(_) => "central-extra",
            Self::CentralComment(_) => "central-comment",
            Self::Zip64EndRecord => "zip64-end-record",
            Self::Zip64EndLocator => "zip64-end-locator",
            Self::EndRecord => "end-record",
        }
    }
}

/// One exact physical range in a ZIP archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalSpan {
    /// Inclusive byte offset.
    pub start: u64,
    /// Exclusive byte offset.
    pub end: u64,
    /// Structural role, including an owning entry where applicable.
    pub role: ZipSpanRole,
}

fn u16_at(bytes: &[u8], offset: u64) -> Result<u16, CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed("ZIP offset does not fit memory".into()))?;
    let raw = bytes
        .get(start..start + 2)
        .ok_or_else(|| CodecError::Malformed("truncated ZIP integer".into()))?;
    View::u16_le_at(raw, 0).ok_or_else(|| CodecError::Malformed("truncated ZIP integer".into()))
}

fn u32_at(bytes: &[u8], offset: u64) -> Result<u32, CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed("ZIP offset does not fit memory".into()))?;
    let raw = bytes
        .get(start..start + 4)
        .ok_or_else(|| CodecError::Malformed("truncated ZIP integer".into()))?;
    View::u32_le_at(raw, 0).ok_or_else(|| CodecError::Malformed("truncated ZIP integer".into()))
}

fn u64_at(bytes: &[u8], offset: u64) -> Result<u64, CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed("ZIP offset does not fit memory".into()))?;
    let raw = bytes
        .get(start..start + 8)
        .ok_or_else(|| CodecError::Malformed("truncated ZIP integer".into()))?;
    View::u64_le_at(raw, 0).ok_or_else(|| CodecError::Malformed("truncated ZIP integer".into()))
}

fn signature_at(bytes: &[u8], offset: u64) -> Option<[u8; 4]> {
    let start = usize::try_from(offset).ok()?;
    bytes
        .get(start..start + 4)
        .map(|raw| [raw[0], raw[1], raw[2], raw[3]])
}

fn push_region(regions: &mut Vec<PhysicalSpan>, start: u64, end: u64, role: ZipSpanRole) {
    if start < end {
        regions.push(PhysicalSpan { start, end, role });
    }
}

fn physical_ledger(
    bytes: &[u8],
    entries: &[EntryRecord],
    central_begin: u64,
) -> Result<Vec<PhysicalSpan>, CodecError> {
    let len = bytes.len() as u64;
    let mut regions = Vec::new();
    let mut local_order = entries.iter().collect::<Vec<_>>();
    local_order.sort_by_key(|entry| entry.header_start);
    if central_begin > len {
        return Err(CodecError::Malformed(
            "ZIP central directory begins after the archive".into(),
        ));
    }

    for (index, entry) in local_order.iter().enumerate() {
        if signature_at(bytes, entry.header_start) != Some(*b"PK\x03\x04") {
            return Err(CodecError::malformed(format_args!(
                "invalid local header signature for {}",
                entry.name
            )));
        }
        let fixed_end = entry.header_start + 30;
        let name_len = u64::from(u16_at(bytes, entry.header_start + 26)?);
        let extra_len = u64::from(u16_at(bytes, entry.header_start + 28)?);
        let name_end = fixed_end + name_len;
        let extra_end = name_end + extra_len;
        if extra_end != entry.data_start {
            return Err(CodecError::malformed(format_args!(
                "local header lengths disagree for {}",
                entry.name
            )));
        }
        push_region(
            &mut regions,
            entry.header_start,
            entry.header_start + 4,
            ZipSpanRole::LocalSignature(entry.name.clone()),
        );
        push_region(
            &mut regions,
            entry.header_start + 4,
            fixed_end,
            ZipSpanRole::LocalFields(entry.name.clone()),
        );
        push_region(
            &mut regions,
            fixed_end,
            name_end,
            ZipSpanRole::LocalName(entry.name.clone()),
        );
        push_region(
            &mut regions,
            name_end,
            extra_end,
            ZipSpanRole::LocalExtra(entry.name.clone()),
        );
        push_region(
            &mut regions,
            entry.data_start,
            entry.data_end()?,
            ZipSpanRole::CompressedPayload(entry.name.clone()),
        );

        let next = local_order
            .get(index + 1)
            .map_or(central_begin, |next| next.header_start);
        if entry.data_end()? > next {
            return Err(CodecError::malformed(format_args!(
                "compressed payload overlaps following ZIP record for {}",
                entry.name
            )));
        }
        if entry.data_end()? < next {
            let flags = u16_at(bytes, entry.header_start + 6)?;
            if flags & 0x0008 != 0 {
                let descriptor_end = parse_data_descriptor(bytes, entry, next)?;
                push_region(
                    &mut regions,
                    entry.data_end()?,
                    descriptor_end,
                    ZipSpanRole::DataDescriptor(entry.name.clone()),
                );
                push_region(
                    &mut regions,
                    descriptor_end,
                    next,
                    ZipSpanRole::Padding {
                        entry: Some(entry.name.clone()),
                    },
                );
            } else {
                push_region(
                    &mut regions,
                    entry.data_end()?,
                    next,
                    ZipSpanRole::Padding {
                        entry: Some(entry.name.clone()),
                    },
                );
            }
        }
    }

    let mut central_order = entries.iter().collect::<Vec<_>>();
    central_order.sort_by_key(|entry| entry.central_start);
    let mut central_end = central_begin;
    for entry in central_order {
        if signature_at(bytes, entry.central_start) != Some(*b"PK\x01\x02") {
            return Err(CodecError::malformed(format_args!(
                "invalid central header signature for {}",
                entry.name
            )));
        }
        let fixed_end = entry.central_start + 46;
        let name_len = u64::from(u16_at(bytes, entry.central_start + 28)?);
        let extra_len = u64::from(u16_at(bytes, entry.central_start + 30)?);
        let comment_len = u64::from(u16_at(bytes, entry.central_start + 32)?);
        let name_end = fixed_end + name_len;
        let extra_end = name_end + extra_len;
        let record_end = extra_end + comment_len;
        if record_end > len {
            return Err(CodecError::malformed(format_args!(
                "truncated central header for {}",
                entry.name
            )));
        }
        push_region(
            &mut regions,
            entry.central_start,
            entry.central_start + 4,
            ZipSpanRole::CentralSignature(entry.name.clone()),
        );
        push_region(
            &mut regions,
            entry.central_start + 4,
            fixed_end,
            ZipSpanRole::CentralFields(entry.name.clone()),
        );
        push_region(
            &mut regions,
            fixed_end,
            name_end,
            ZipSpanRole::CentralName(entry.name.clone()),
        );
        push_region(
            &mut regions,
            name_end,
            extra_end,
            ZipSpanRole::CentralExtra(entry.name.clone()),
        );
        push_region(
            &mut regions,
            extra_end,
            record_end,
            ZipSpanRole::CentralComment(entry.name.clone()),
        );
        central_end = central_end.max(record_end);
    }

    classify_end_records(bytes, central_end, len, &mut regions)?;
    partition(len, &regions)
}

fn parse_data_descriptor(
    bytes: &[u8],
    entry: &EntryRecord,
    record_end: u64,
) -> Result<u64, CodecError> {
    let start = entry.data_end()?;
    let has_signature = signature_at(bytes, start) == Some(*b"PK\x07\x08");
    let local_zip64 = u32_at(bytes, entry.header_start + 18)? == u32::MAX
        || u32_at(bytes, entry.header_start + 22)? == u32::MAX;
    let widths = if local_zip64 { [8_u64, 4] } else { [4_u64, 8] };
    for signed in [true, false] {
        if signed && !has_signature {
            continue;
        }
        let values_start = start + if signed { 4 } else { 0 };
        for width in widths {
            let end = values_start + 4 + 2 * width;
            if end > record_end {
                continue;
            }
            let crc = u32_at(bytes, values_start)?;
            let (compressed, uncompressed) = if width == 4 {
                (
                    u64::from(u32_at(bytes, values_start + 4)?),
                    u64::from(u32_at(bytes, values_start + 8)?),
                )
            } else {
                (
                    u64_at(bytes, values_start + 4)?,
                    u64_at(bytes, values_start + 12)?,
                )
            };
            if crc == entry.crc32
                && compressed == entry.compressed_size
                && uncompressed == entry.uncompressed_size
            {
                return Ok(end);
            }
        }
    }
    Err(CodecError::malformed(format_args!(
        "invalid data descriptor for {}",
        entry.name
    )))
}

fn classify_end_records(
    bytes: &[u8],
    mut offset: u64,
    len: u64,
    regions: &mut Vec<PhysicalSpan>,
) -> Result<(), CodecError> {
    while offset < len {
        let (role, size) = match signature_at(bytes, offset) {
            Some(signature) if signature == *b"PK\x06\x06" => {
                let start = usize::try_from(offset + 4)
                    .map_err(|_| CodecError::Malformed("ZIP64 offset overflow".into()))?;
                let raw = bytes
                    .get(start..start + 8)
                    .ok_or_else(|| CodecError::Malformed("truncated ZIP64 end record".into()))?;
                let body = View::u64_le_at(raw, 0)
                    .ok_or_else(|| CodecError::Malformed("truncated ZIP64 end record".into()))?;
                (
                    ZipSpanRole::Zip64EndRecord,
                    12_u64
                        .checked_add(body)
                        .ok_or_else(|| CodecError::Malformed("ZIP64 end size overflow".into()))?,
                )
            }
            Some(signature) if signature == *b"PK\x06\x07" => (ZipSpanRole::Zip64EndLocator, 20),
            Some(signature) if signature == *b"PK\x05\x06" => {
                let comment = u64::from(u16_at(bytes, offset + 20)?);
                (ZipSpanRole::EndRecord, 22_u64 + comment)
            }
            _ => (ZipSpanRole::Padding { entry: None }, len - offset),
        };
        let end = offset
            .checked_add(size)
            .ok_or_else(|| CodecError::Malformed("ZIP end-record range overflow".into()))?;
        if end > len {
            return Err(CodecError::malformed(format_args!(
                "truncated {}",
                role.label()
            )));
        }
        push_region(regions, offset, end, role);
        offset = end;
    }
    Ok(())
}

fn partition(len: u64, regions: &[PhysicalSpan]) -> Result<Vec<PhysicalSpan>, CodecError> {
    let mut boundaries = BTreeSet::from([0_u64, len]);
    for region in regions {
        if region.end > len || region.start > region.end {
            return Err(CodecError::Malformed(
                "invalid physical ledger region".into(),
            ));
        }
        boundaries.insert(region.start);
        boundaries.insert(region.end);
    }
    let points = boundaries.into_iter().collect::<Vec<_>>();
    let mut ordered_regions = regions.iter().collect::<Vec<_>>();
    ordered_regions.sort_by_key(|region| (region.start, region.end));
    let mut region_index = 0_usize;
    let mut spans = Vec::new();
    for pair in points.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        while ordered_regions
            .get(region_index)
            .is_some_and(|region| region.end <= start)
        {
            region_index += 1;
        }
        let owner = ordered_regions
            .get(region_index)
            .copied()
            .filter(|region| region.start <= start && end <= region.end)
            .ok_or_else(|| {
                CodecError::Malformed(
                    "physical ZIP ledger contains an unclassified byte range".into(),
                )
            })?;
        spans.push(PhysicalSpan {
            start,
            end,
            role: owner.role.clone(),
        });
    }
    Ok(spans)
}

/// Storage for one member, recording a malformed declaration as an attribute.
///
/// A stored member declaring fewer compressed than uncompressed bytes has no
/// usable stored span; the payload stands alone and the declaration is reported.
fn declared_storage(
    compression: ZipCompression,
    compressed_size: u64,
    uncompressed_size: u64,
    attributes: &mut BTreeMap<String, String>,
) -> cadmpeg_core::container::EntryStorage {
    match compression.storage(compressed_size, uncompressed_size) {
        Ok(storage) => storage,
        Err(message) => {
            attributes.insert(
                "storage_declaration".into(),
                format!("{message}: {compressed_size}/{uncompressed_size}"),
            );
            cadmpeg_core::container::EntryStorage::payload_only(
                cadmpeg_core::container::VerbatimLabel::Stored,
                uncompressed_size,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write as _};

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
    use cadmpeg_core::CodecError;
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    use super::{ArchiveSnapshot, EntryRecord, PhysicalSpan, ZipCompression, ZipSpanRole};

    #[test]
    fn empty_zip_ledger_covers_its_end_record() {
        let bytes = zip::ZipWriter::new(Cursor::new(Vec::new()))
            .finish()
            .expect("empty ZIP finishes")
            .into_inner();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("empty ZIP fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("empty ZIP is valid");
        assert!(snapshot.entries().is_empty());
        assert_eq!(
            snapshot
                .physical_ledger()
                .expect("empty ZIP has a complete physical ledger"),
            vec![PhysicalSpan {
                start: 0,
                end: bytes.len() as u64,
                role: ZipSpanRole::EndRecord,
            }]
        );
    }

    #[test]
    fn descriptor_crc_equal_to_optional_signature_keeps_unsigned_layout() {
        let signature = *b"PK\x07\x08";
        let mut bytes = vec![0_u8; 42];
        bytes[30..34].copy_from_slice(&signature);
        let entry = EntryRecord {
            name: "empty".to_owned(),
            compression: ZipCompression::Stored,
            crc32: u32::from_le_bytes(signature),
            compressed_size: 0,
            uncompressed_size: 0,
            header_start: 0,
            data_start: 30,
            central_start: 42,
            utf8_name: false,
        };
        assert_eq!(
            super::parse_data_descriptor(&bytes, &entry, 42)
                .expect("unsigned descriptor matches its central record"),
            42
        );

        bytes.extend_from_slice(&[0; 4]);
        bytes[34..38].copy_from_slice(&signature);
        assert_eq!(
            super::parse_data_descriptor(&bytes, &entry, 46)
                .expect("signed descriptor matches its central record"),
            46
        );
    }

    fn archive_bytes() -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(
                "stored.bin",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .expect("stored entry starts");
        archive.write_all(b"stored").expect("stored entry writes");
        archive
            .start_file(
                "deflated.bin",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("deflated entry starts");
        archive
            .write_all(b"deflated payload")
            .expect("deflated entry writes");
        archive
            .start_file(
                "zstd.bin",
                SimpleFileOptions::default().compression_method(CompressionMethod::Zstd),
            )
            .expect("Zstandard entry starts");
        archive
            .write_all(b"Zstandard payload")
            .expect("Zstandard entry writes");
        archive.finish().expect("archive finishes").into_inner()
    }

    #[test]
    fn central_directory_count_refuses_before_zip_indexing() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("archive fits root policy");
        assert!(matches!(
            ArchiveSnapshot::new(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "ZIP central directory entries"
        ));

        let (service_ctx, service_root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("archive fits service profile");
        assert_eq!(
            ArchiveSnapshot::new(&service_ctx, service_root)
                .expect("directory fits service profile")
                .entries()
                .len(),
            3
        );
    }

    #[test]
    fn central_header_preflight_refuses_at_lowered_work_limit() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 3;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("archive fits input limit");
        assert!(matches!(
            ArchiveSnapshot::new(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "ZIP central header preflight"
        ));
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("archive fits service profile");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("directory fits service work limit")
                .entries()
                .len(),
            3
        );
    }

    #[test]
    fn central_name_bytes_refuse_before_zip_indexing() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("archive fits root policy");
        assert!(matches!(
            ArchiveSnapshot::new(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "ZIP library indexed names"
        ));
    }

    #[test]
    fn zip_end_signature_inside_comment_does_not_replace_directory_count() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("entry", SimpleFileOptions::default())
            .expect("entry starts");
        writer.write_all(b"data").expect("entry writes");
        writer
            .set_raw_comment(b"comment PK\x05\x06 suffix".to_vec().into_boxed_slice())
            .expect("comment is valid");
        let bytes = writer.finish().expect("ZIP finishes").into_inner();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("archive fits service profile");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("directory remains readable")
                .entries()
                .len(),
            1
        );
    }

    #[test]
    fn malformed_end_record_inside_comment_does_not_mask_the_archive() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("entry", SimpleFileOptions::default())
            .expect("entry starts");
        writer.write_all(b"data").expect("entry writes");
        let mut comment = vec![0_u8; 40];
        comment[..4].copy_from_slice(b"PK\x05\x06");
        comment[8..10].copy_from_slice(&999_u16.to_le_bytes());
        comment[10..12].copy_from_slice(&999_u16.to_le_bytes());
        writer
            .set_raw_comment(comment.into_boxed_slice())
            .expect("comment is valid");
        let bytes = writer.finish().expect("ZIP finishes").into_inner();
        assert_eq!(
            zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
                .expect("ZIP library finds the preceding end record")
                .len(),
            1
        );
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("archive fits service profile");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("directory remains readable")
                .entries()
                .len(),
            1
        );
    }

    #[test]
    fn false_end_record_cannot_undercharge_the_selected_directory() {
        let mut bytes = archive_bytes();
        let end = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("ZIP end record exists");
        let directory_size = View::u32_le_at(&bytes, end + 12).expect("directory size exists");
        let directory_offset = View::u32_le_at(&bytes, end + 16).expect("directory offset exists");
        bytes[end + 20..end + 22].copy_from_slice(&40_u16.to_le_bytes());
        bytes.resize(bytes.len() + 40, 0);
        let fake = end + 22;
        bytes[fake..fake + 4].copy_from_slice(b"PK\x05\x06");
        bytes[fake + 4..fake + 6].copy_from_slice(&1_u16.to_le_bytes());
        bytes[fake + 8..fake + 10].copy_from_slice(&1_u16.to_le_bytes());
        bytes[fake + 10..fake + 12].copy_from_slice(&1_u16.to_le_bytes());
        bytes[fake + 12..fake + 16].copy_from_slice(&(directory_size + 22).to_le_bytes());
        bytes[fake + 16..fake + 20].copy_from_slice(&directory_offset.to_le_bytes());
        assert_eq!(
            zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
                .expect("ZIP library selects the valid end record")
                .len(),
            3
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("archive fits input limit");
        assert!(matches!(
            ArchiveSnapshot::new(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "ZIP central directory entries"
        ));
    }

    #[test]
    fn central_directory_signature_after_entries_keeps_archive_readable() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("entry", SimpleFileOptions::default())
            .expect("entry starts");
        writer.write_all(b"data").expect("entry writes");
        let mut bytes = writer.finish().expect("ZIP finishes").into_inner();
        let end = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("ZIP end record exists");
        bytes.splice(end..end, *b"PK\x05\x05\x03\x00abc");
        assert_eq!(
            zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
                .expect("ZIP library accepts the signature")
                .len(),
            1
        );
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("archive fits service profile");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("directory remains readable")
                .entries()
                .len(),
            1
        );
    }

    #[test]
    fn signed_directory_search_refuses_before_unbounded_scan() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("entry", SimpleFileOptions::default())
            .expect("entry starts");
        writer.write_all(b"data").expect("entry writes");
        let mut bytes = writer.finish().expect("ZIP finishes").into_inner();
        let end = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("ZIP end record exists");
        let directory_offset =
            usize::try_from(View::u32_le_at(&bytes, end + 16).expect("directory offset exists"))
                .expect("directory offset fits memory");
        bytes.splice(end..end, *b"PK\x05\x05\x03\x00abc");
        let search_len = (end + 8)
            .checked_sub(directory_offset)
            .expect("directory begins before end record");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(search_len).expect("search fits work limit");
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("archive fits input limit");
        assert!(matches!(
            ArchiveSnapshot::new(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "ZIP central header search"
        ));
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("archive fits service profile");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("signed directory fits service work limit")
                .entries()
                .len(),
            1
        );
    }

    /// Rewrites the central-directory `uncompressed_size` of `name` to `size`.
    fn patch_central_uncompressed_size(bytes: &mut [u8], name: &str, size: u32) {
        let mut at = 0;
        while let Some(found) = bytes[at..]
            .windows(4)
            .position(|window| window == [0x50, 0x4b, 0x01, 0x02])
        {
            let record = at + found;
            let name_len = u16::from_le_bytes([bytes[record + 28], bytes[record + 29]]) as usize;
            let start = record + 46;
            if &bytes[start..start + name_len] == name.as_bytes() {
                bytes[record + 24..record + 28].copy_from_slice(&size.to_le_bytes());
                return;
            }
            at = record + 4;
        }
        panic!("central directory record not found");
    }

    #[test]
    fn a_stored_member_declaring_a_span_under_its_payload_is_reported() {
        use cadmpeg_core::container::{ContainerRole, VerbatimLabel, VerbatimSize};

        let mut bytes = archive_bytes();
        // "stored" is six bytes; the declaration now claims it expands to nine.
        patch_central_uncompressed_size(&mut bytes, "stored.bin", 9);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let entries = snapshot.container_entries(|_| ContainerRole::Stream);
        let stored = entries
            .iter()
            .find(|entry| entry.name == "stored.bin")
            .expect("stored entry summarized");

        assert_eq!(
            stored.storage,
            cadmpeg_core::container::EntryStorage::Verbatim {
                label: VerbatimLabel::Stored,
                size: VerbatimSize::PayloadOnly(9),
            }
        );
        assert_eq!(stored.storage.stored_size(), None);
        assert_eq!(stored.storage.expanded_size(), Some(9));
        assert_eq!(
            stored.attributes["storage_declaration"],
            "verbatim container entry stores fewer bytes than it expands to: 6/9"
        );

        let deflated = entries
            .iter()
            .find(|entry| entry.name == "deflated.bin")
            .expect("deflated entry summarized");
        assert!(!deflated.attributes.contains_key("storage_declaration"));
    }

    #[test]
    fn snapshot_opens_supported_entries_after_parser_drop() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        assert_eq!(snapshot.entries().len(), 3);
        let stored = snapshot.entry("stored.bin").expect("stored record");
        let deflated = snapshot.entry("deflated.bin").expect("deflated record");
        let zstd = snapshot.entry("zstd.bin").expect("Zstandard record");
        assert_eq!(
            snapshot
                .open(&ctx, &stored.name)
                .expect("stored opens")
                .window(),
            b"stored"
        );
        assert_eq!(
            snapshot
                .open(&ctx, &deflated.name)
                .expect("deflated opens")
                .window(),
            b"deflated payload"
        );
        assert_eq!(
            snapshot
                .open(&ctx, &zstd.name)
                .expect("Zstandard entry opens")
                .window(),
            b"Zstandard payload"
        );
    }

    #[test]
    fn zip_deflate_open_rejects_trailing_declared_payload_bytes() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "deflated.bin",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("deflate entry starts");
        writer.write_all(b"one member").expect("entry writes");
        let mut bytes = writer.finish().expect("ZIP finishes").into_inner();
        let entry = {
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                    .expect("archive fits service profile");
            ArchiveSnapshot::new(&ctx, root)
                .expect("original directory is valid")
                .entry("deflated.bin")
                .expect("deflate entry exists")
                .clone()
        };
        let suffix = b"suffix";
        let payload_end = usize::try_from(entry.data_end().expect("payload end"))
            .expect("payload end fits memory");
        bytes.splice(payload_end..payload_end, suffix.iter().copied());
        let header = usize::try_from(entry.header_start).expect("header fits memory");
        let central = usize::try_from(entry.central_start).expect("central header fits memory")
            + suffix.len();
        let compressed_size = u32::try_from(entry.compressed_size + suffix.len() as u64)
            .expect("fixture compressed size fits u32");
        bytes[header + 18..header + 22].copy_from_slice(&compressed_size.to_le_bytes());
        bytes[central + 20..central + 24].copy_from_slice(&compressed_size.to_le_bytes());
        let end = bytes
            .windows(4)
            .rposition(|signature| signature == b"PK\x05\x06")
            .expect("ZIP end record exists");
        let central_start = u32::try_from(central).expect("fixture directory start fits u32");
        bytes[end + 16..end + 20].copy_from_slice(&central_start.to_le_bytes());

        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("patched archive fits service profile");
        let archive = ArchiveSnapshot::new(&ctx, root).expect("patched directory is valid");
        assert!(matches!(
            archive.open(&ctx, "deflated.bin"),
            Err(CodecError::Malformed(message))
                if message == "raw-DEFLATE member does not exhaust its declared ZIP payload"
        ));
    }

    #[test]
    fn snapshot_opens_names_using_its_own_metadata() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let first = ArchiveSnapshot::new(&ctx, root).expect("first archive snapshot");
        let second = ArchiveSnapshot::new(&ctx, root).expect("second archive snapshot");
        let mut detached = second.entry("stored.bin").expect("entry exists").clone();
        detached.data_start = u64::MAX;
        detached.crc32 = 0;
        assert_eq!(
            first
                .open(&ctx, &detached.name)
                .expect("name opens own metadata")
                .window(),
            b"stored"
        );
        assert!(first.open(&ctx, "missing.bin").is_err());
    }

    #[test]
    fn snapshot_opens_entries_from_a_nonzero_root_view() {
        let archive = archive_bytes();
        let mut bytes = b"prefix".to_vec();
        let start = bytes.len();
        bytes.extend_from_slice(&archive);
        let end = bytes.len();
        bytes.extend_from_slice(b"suffix");
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("outer bytes fit root policy");
        let nested = root.child(start, end).expect("archive child range");
        let snapshot = ArchiveSnapshot::new(&ctx, nested).expect("nested archive snapshots");

        for (name, expected) in [
            ("stored.bin", b"stored".as_slice()),
            ("deflated.bin", b"deflated payload".as_slice()),
            ("zstd.bin", b"Zstandard payload".as_slice()),
        ] {
            let entry = snapshot.entry(name).expect("entry exists");
            assert_eq!(
                snapshot
                    .open(&ctx, &entry.name)
                    .expect("nested entry opens")
                    .window(),
                expected
            );
        }
    }

    #[test]
    fn nested_archive_members_keep_distinct_address_spaces() {
        let mut inner = zip::ZipWriter::new(Cursor::new(Vec::new()));
        inner
            .start_file("Data/payload bytes.bin", SimpleFileOptions::default())
            .expect("nested archive fixture");
        inner
            .write_all(b"payload bytes")
            .expect("nested archive fixture");
        let inner_bytes = inner.finish().expect("nested archive fixture").into_inner();
        let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        outer
            .start_file("Assets/inner archive.zip", SimpleFileOptions::default())
            .expect("nested archive fixture");
        outer
            .write_all(&inner_bytes)
            .expect("nested archive fixture");
        let bytes = outer.finish().expect("nested archive fixture").into_inner();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("nested archive fixture");
        let archive = ArchiveSnapshot::new(&ctx, root).expect("nested archive fixture");
        let inner_view = archive
            .open(&ctx, "Assets/inner archive.zip")
            .expect("nested archive fixture");
        let nested = ArchiveSnapshot::new(&ctx, inner_view).expect("nested archive fixture");
        let payload = nested
            .open(&ctx, "Data/payload bytes.bin")
            .expect("nested archive fixture");
        assert_eq!(payload.window(), b"payload bytes");
        assert_ne!(root.location().space, inner_view.location().space);
        assert_ne!(inner_view.location().space, payload.location().space);
        assert_eq!(payload.location_at(7).offset, 7);
    }
}
