// SPDX-License-Identifier: Apache-2.0
//! Single-pass ZIP metadata snapshots with budgeted entry opening.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use cadmpeg_core::decode::{ByteRange, DecodeContext, ExpandSpec, ScopedReservation, View};
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
    fn from_zip(ctx: &DecodeContext<'_>, method: CompressionMethod, name: &str) -> Result<Self, CodecError> {
        match method {
            CompressionMethod::Stored => Ok(Self::Stored),
            CompressionMethod::Deflated => Ok(Self::Deflate),
            CompressionMethod::Zstd => Ok(Self::Zstd),
            other => Err(CodecError::NotImplemented(ctx.format_retained(
                format_args!("ZIP compression {other:?} for {name}"), "ZIP compression error"
            )?)),
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

/// Dependency index and the storage reservation that outlives its metadata.
struct ZipIndex<'bytes, 'ctx> {
    archive: zip::ZipArchive<Cursor<&'bytes [u8]>>,
    _workspace: ScopedReservation<'ctx>,
}

struct ZipParserAdmission<'bytes, 'ctx> {
    bytes: &'bytes [u8],
    workspace: ScopedReservation<'ctx>,
}

fn zip_parser_admission<'bytes, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'bytes [u8],
) -> Result<ZipParserAdmission<'bytes, 'ctx>, CodecError> {
    let bound = preflight_central_directory(ctx, bytes)?;
    let workspace = ctx.reserve_scoped(bound.workspace, "ZIP indexing workspace")?;
    ctx.charge_work(bound.work, "ZIP dependency indexing")?;
    Ok(ZipParserAdmission { bytes, workspace })
}

impl<'bytes, 'ctx> ZipIndex<'bytes, 'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>, bytes: &'bytes [u8]) -> Result<Self, CodecError> {
        let admission = zip_parser_admission(ctx, bytes)?;
        let archive = zip::ZipArchive::new(Cursor::new(admission.bytes))
            .map_err(|error| CodecError::malformed(format_args!("not a readable ZIP: {error}")))?;
        Ok(Self {
            archive,
            _workspace: admission.workspace,
        })
    }
}

impl<'a> ArchiveSnapshot<'a> {
    /// Parses the central directory once and retains replayable physical facts.
    pub fn new(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let mut index = ZipIndex::new(ctx, root.window())?;
        let archive = &mut index.archive;
        let archive_central_start = archive.central_directory_start();
        let central_entry_count =
            reject_duplicate_central_names(ctx, root.window(), archive_central_start)?;
        if central_entry_count != archive.len() {
            return Err(CodecError::Malformed(
                "ZIP central directory contains duplicate entry names".into(),
            ));
        }
        let mut name_storage = ctx.reserve_scoped(0, "ZIP duplicate names")?;
        let mut names = BTreeSet::new();
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(archive.len()),
            "ZIP entry records",
        )?;
        let mut entries = ctx.vector_storage(archive.len(), "ZIP entry records")?;
        for index in 0..archive.len() {
            ctx.charge_work(1, "visit ZIP entry records")?;
            let file = archive.by_index_raw(index).map_err(|error| {
                CodecError::malformed(format_args!("bad ZIP entry {index}: {error}"))
            })?;
            let name = ctx.copy_retained_text(file.name(), "ZIP entry record name")?;
            let duplicate_key = ctx.copy_scoped_text(&name, &mut name_storage, "ZIP duplicate name")?;
            if !ctx.insert_scoped_btree_set(&mut name_storage, &mut names, duplicate_key,
                "ZIP decoded name comparison", "ZIP decoded name set")? {
                return Err(CodecError::malformed(format_args!(
                    "duplicate ZIP entry name {name}"
                )));
            }
            if file.encrypted() {
                return Err(CodecError::malformed(format_args!(
                    "encrypted ZIP entry {name}"
                )));
            }
            let compression = ZipCompression::from_zip(ctx, file.compression(), &name)?;
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
                if offset > cadmpeg_core::decode::u64_from_index(root.window().len()) {
                    return Err(CodecError::malformed(format_args!(
                        "ZIP offset outside archive for {}",
                        record.name
                    )));
                }
            }
            ctx.reserve_capacity(&mut entries, 1, "ZIP entry record slot")?;
            entries.push(record);
        }
        drop(index);
        let mut by_name = BTreeMap::new();
        for (index, entry) in ctx.admit_iter(&entries, "visit ZIP name index")?.enumerate() {
            let key = ctx.copy_retained_text(&entry.name, "ZIP indexed entry name")?;
            ctx.insert_btree_map(&mut by_name, key, index, "ZIP name index")?;
        }
        Ok(Self {
            root,
            central_start: archive_central_start,
            entries,
            by_name,
        })
    }

    /// Tests an indexed name without opening payloads or applying compression
    /// and encryption admission. Temporary index storage remains scoped.
    pub fn contains_name(
        ctx: &DecodeContext<'_>,
        root: View<'_>,
        name: &str,
    ) -> Result<bool, CodecError> {
        let index = ZipIndex::new(ctx, root.window())?;
        for ordinal in 0..index.archive.len() {
            ctx.charge_work(1, "visit ZIP name probe")?;
            let candidate = index.archive.name_for_index(ordinal)
                .ok_or_else(|| CodecError::Malformed("ZIP indexed name is absent".into()))?;
            if ctx.equal(candidate, name, "ZIP name probe comparison")? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Returns central-directory records in archive order.
    pub fn entries(&self) -> &[EntryRecord] {
        &self.entries
    }

    /// Finds an entry record by its exact archive name.
    pub fn entry(&self, ctx: &DecodeContext<'_>, name: &str) -> Result<Option<&EntryRecord>, CodecError> {
        Ok(ctx.get_btree_map(&self.by_name, name, "ZIP entry lookup")?.map(|index| &self.entries[*index]))
    }

    /// Opens an exact entry name as a borrowed stored slice or budgeted expanded view.
    pub fn open(&self, ctx: &DecodeContext<'a>, name: &str) -> Result<View<'a>, CodecError> {
        let entry = self
            .entry(ctx, name)?
            .ok_or_else(|| CodecError::malformed(format_args!("ZIP entry {name} is absent")))?;
        let end = entry.data_end()?;
        let archive_start = cadmpeg_core::decode::u64_from_index(self.root.start());
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
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(source.window().len()),
                    "ZIP compressed input",
                )?;
                let mut decoder = flate2::read::DeflateDecoder::new(source.window());
                let view = Self::open_expanded(ctx, entry, |chunk| {
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(chunk.len()),
                        "ZIP expansion step",
                    )?;
                    let read = decoder.read(chunk).map_err(|error| {
                        CodecError::malformed(format_args!(
                            "cannot inflate {}: {error}",
                            entry.name
                        ))
                    })?;
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(read),
                        "ZIP expansion copy",
                    )?;
                    Ok(read)
                })?;
                if decoder.total_in() != cadmpeg_core::decode::u64_from_index(source.window().len())
                {
                    return Err(CodecError::Malformed(
                        "raw-DEFLATE member does not exhaust its declared ZIP payload".into(),
                    ));
                }
                Ok(view)
            }
            ZipCompression::Zstd => {
                let source = self.compressed_source(entry, range)?;
                let mut decoder = ctx.open_zstd(source)?;
                Self::open_expanded(ctx, entry, |chunk| decoder.read_chunk(chunk))
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
        if cadmpeg_core::decode::u64_from_index(view.window().len()) != entry.uncompressed_size {
            return Err(CodecError::malformed(format_args!(
                "stored size mismatch for {}",
                entry.name
            )));
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(view.window().len()),
            "ZIP payload CRC",
        )?;
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
        mut read_chunk: impl FnMut(&mut [u8]) -> Result<usize, CodecError>,
    ) -> Result<View<'a>, CodecError> {
        let mut writer = ctx.begin_expand(ExpandSpec::Exact(entry.uncompressed_size))?;
        let mut chunk = [0_u8; 16 * 1024];
        loop {
            ctx.charge_work(1, "visit ZIP expansion chunk")?;
            let read = read_chunk(&mut chunk)?;
            if read == 0 {
                break;
            }
            writer.write(&chunk[..read])?;
        }
        let view = writer.finalize()?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(view.window().len()),
            "ZIP payload CRC",
        )?;
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
        ctx: &DecodeContext<'_>,
        classify: impl Fn(&str) -> cadmpeg_core::container::ContainerRole,
    ) -> Result<Vec<ContainerEntry>, CodecError> {
        let mut output = ctx.collection_vec(self.entries.len(), "ZIP container summaries")?;
        for entry in ctx.admit_iter(&self.entries, "ZIP summary entry visits")? {
            let mut attributes = BTreeMap::new();
            ctx.insert_btree_map(
                &mut attributes,
                ctx.copy_retained_text("crc32", "ZIP summary attribute key")?,
                ctx.format_retained(
                    format_args!("{:08x}", entry.crc32),
                    "ZIP summary attribute value",
                )?,
                "ZIP summary attributes",
            )?;
            for (key, value) in [
                ("header_offset", entry.header_start),
                ("data_offset", entry.data_start),
                ("central_header_offset", entry.central_start),
            ] {
                ctx.insert_btree_map(
                &mut attributes,
                    ctx.copy_retained_text(key, "ZIP summary attribute key")?,
                    ctx.format_retained(format_args!("{value}"), "ZIP summary attribute value")?,
                    "ZIP summary attributes",
                )?;
            }
            let storage = declared_storage(
                ctx,
                entry.compression,
                entry.compressed_size,
                entry.uncompressed_size,
                &mut attributes,
            )?;
            ctx.reserve_capacity(&mut output, 1, "ZIP summary slot")?;
            output.push(ContainerEntry {
                name: ctx.copy_retained_text(&entry.name, "ZIP summary entry name")?,
                role: classify(&entry.name),
                storage,
                attributes,
            });
        }
        Ok(output)
    }

    /// Partitions every physical archive byte by ZIP structural role.
    pub fn physical_ledger(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<PhysicalSpan>, CodecError> {
        physical_ledger(ctx, self.root.window(), &self.entries, self.central_start)
    }
}

/// Peak dependency storage and work admitted before ZIP indexing.
#[derive(Debug)]
struct ZipIndexAdmission {
    workspace: u64,
    work: u64,
}

fn preflight_central_directory(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<ZipIndexAdmission, CodecError> {
    let mut first_error = None;
    let mut max_count = None::<u64>;
    let input_bytes = cadmpeg_core::decode::u64_from_index(bytes.len());
    let mut declared_capacity = 0;
    let mut declared_comment = 0;
    let mut end_candidates = 0_u64;
    let mut zip64_candidates = 0_u64;
    for (end, signature) in ctx.admit_iter(bytes, "ZIP end record search")?.windows(
        std::num::NonZeroUsize::new(4).ok_or_else(|| CodecError::Malformed("zero ZIP signature width".into()))?
    ).enumerate().rev() {
        if signature == b"PK\x06\x06" {
            zip64_candidates = zip64_candidates.checked_add(1).ok_or_else(||
                ctx.refuse_codec_limit("ZIP dependency indexing", u64::MAX, u64::MAX))?;
        }
        if signature != b"PK\x05\x06" {
            continue;
        }
        end_candidates = end_candidates.checked_add(1).ok_or_else(||
            ctx.refuse_codec_limit("ZIP dependency indexing", u64::MAX, u64::MAX))?;
        // ZIP32 allocates from the declaration before parsing its records,
        // but uses zero capacity when the count exceeds the directory offset.
        if let Some(count) = View::u16_le_at(bytes, end + 10).map(u64::from) {
            if count <= input_bytes {
                declared_capacity = declared_capacity.max(count);
            }
        }
        if let Some(comment) = View::u16_le_at(bytes, end + 20).map(u64::from) {
            declared_comment = declared_comment.max(comment);
        }
        ctx.charge_work(1, "ZIP end record candidate")?;
        match central_directory_inventory(ctx, bytes, end) {
            Ok(count) => {
                max_count = Some(max_count.map_or(count, |current| current.max(count)));
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
    // ZIP64 validates count * 46 against its physical end before allocating.
    if zip64_candidates != 0 {
        declared_capacity = declared_capacity.max(input_bytes / 46);
    }
    ctx.charge_collection_items(count.max(declared_capacity), "ZIP central directory entries")?;
    // The input term covers decoded text, extra fields, index tables and
    // ZIP64 extensible data. A metadata record is below 1 KiB. Failed ZIP32
    // candidates can allocate their full declared capacity and comment.
    let workspace = input_bytes.checked_mul(64)
        .and_then(|bytes| declared_capacity.checked_mul(1024)?.checked_add(bytes))
        .and_then(|bytes| bytes.checked_add(declared_comment))
        .ok_or_else(|| ctx.refuse_codec_limit("ZIP indexing workspace", u64::MAX, u64::MAX))?;
    // The work factor covers three fixed-metadata moves per 46-byte record,
    // text decoding, hashing and searches. Each retry can scan a directory
    // and each ZIP64 candidate; the final failed search and comment fill count.
    let work = input_bytes.checked_mul(128)
        .and_then(|bytes| bytes.checked_add(declared_comment))
        .and_then(|bytes| bytes.checked_mul(end_candidates.checked_add(1)?))
        .and_then(|bytes| bytes.checked_mul(zip64_candidates.checked_add(1)?))
        .ok_or_else(|| ctx.refuse_codec_limit("ZIP dependency indexing", u64::MAX, u64::MAX))?;
    Ok(ZipIndexAdmission { workspace, work })
}

fn central_directory_inventory(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    end: usize,
) -> Result<u64, CodecError> {
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
            let record_start = ctx.admit_iter(&bytes[..locator_start], "ZIP64 end record search")?
                .windows(std::num::NonZeroUsize::new(4)
                    .ok_or_else(|| CodecError::Malformed("zero ZIP signature width".into()))?)
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
    let directory_end = cadmpeg_core::decode::u64_from_index(directory_end);
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
        let search = bytes.get(search_start..search_end)
            .ok_or_else(|| CodecError::Malformed("ZIP directory search range is invalid".into()))?;
        let relative = ctx.admit_iter(search, "ZIP central header search")?
            .windows(std::num::NonZeroUsize::new(4)
                .ok_or_else(|| CodecError::Malformed("zero ZIP signature width".into()))?)
            .position(|window| window == b"PK\x01\x02")
            .ok_or_else(|| CodecError::Malformed("ZIP central header is absent".into()))?;
        let start = search_start.checked_add(relative)
            .ok_or_else(|| CodecError::Malformed("ZIP central header offset overflow".into()))?;
        cadmpeg_core::decode::u64_from_index(start)
    };
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
    }
    Ok(count)
}

fn reject_duplicate_central_names(ctx: &DecodeContext<'_>, bytes: &[u8], central_start: u64) -> Result<usize, CodecError> {
    let mut offset = central_start;
    let mut storage = ctx.reserve_scoped(0, "ZIP duplicate central names")?;
    let mut names = BTreeSet::new();
    let mut entry_count = 0;
    while signature_at(bytes, offset) == Some(*b"PK\x01\x02") {
        ctx.charge_work(1, "visit ZIP central name")?;
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
        if !ctx.insert_scoped_btree_set(&mut storage, &mut names, name,
            "ZIP central name comparison", "ZIP duplicate name set")? {
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
    fn copy_with_context(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let name = |name: &str| ctx.copy_retained_text(name, "ZIP partition role name");
        Ok(match self {
            Self::LocalSignature(value) => Self::LocalSignature(name(value)?),
            Self::LocalFields(value) => Self::LocalFields(name(value)?),
            Self::LocalName(value) => Self::LocalName(name(value)?),
            Self::LocalExtra(value) => Self::LocalExtra(name(value)?),
            Self::CompressedPayload(value) => Self::CompressedPayload(name(value)?),
            Self::DataDescriptor(value) => Self::DataDescriptor(name(value)?),
            Self::Padding { entry } => Self::Padding {
                entry: entry.as_deref().map(name).transpose()?,
            },
            Self::CentralSignature(value) => Self::CentralSignature(name(value)?),
            Self::CentralFields(value) => Self::CentralFields(name(value)?),
            Self::CentralName(value) => Self::CentralName(name(value)?),
            Self::CentralExtra(value) => Self::CentralExtra(name(value)?),
            Self::CentralComment(value) => Self::CentralComment(name(value)?),
            Self::Zip64EndRecord => Self::Zip64EndRecord,
            Self::Zip64EndLocator => Self::Zip64EndLocator,
            Self::EndRecord => Self::EndRecord,
        })
    }

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

fn push_region(
    ctx: &DecodeContext<'_>,
    regions: &mut Vec<PhysicalSpan>,
    start: u64,
    end: u64,
    role: ZipSpanRole,
) -> Result<(), CodecError> {
    if start < end {
        ctx.reserve_vec(regions, 1, "ZIP ledger regions")?;
        regions.push(PhysicalSpan { start, end, role });
    }
    Ok(())
}

fn physical_ledger(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    entries: &[EntryRecord],
    central_begin: u64,
) -> Result<Vec<PhysicalSpan>, CodecError> {
    let len = cadmpeg_core::decode::u64_from_index(bytes.len());
    let mut region_storage = ctx.reserve_scoped(0, "ZIP ledger source regions")?;
    let regions = region_storage.with_storage(|| {
        let mut regions = Vec::new();
        let mut local_order = ctx.collection_vec(entries.len(), "ZIP ledger local order")?;
        for entry in ctx.admit_iter(entries, "visit ZIP ledger local entries")? {
            ctx.reserve_capacity(&mut local_order, 1, "ZIP ledger local slot")?;
            local_order.push(entry);
        }
        ctx.stable_sort_by(
            &mut local_order,
            |value| &value.header_start,
            Ord::cmp,
            "ZIP ledger local order",
        )?;
        if central_begin > len {
            return Err(CodecError::Malformed(
                "ZIP central directory begins after the archive".into(),
            ));
        }

        for (index, entry) in ctx.admit_iter(&local_order, "visit ZIP ledger local order")?.enumerate() {
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
                ctx,
                &mut regions,
                entry.header_start,
                entry.header_start + 4,
                ZipSpanRole::LocalSignature(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            push_region(
                ctx,
                &mut regions,
                entry.header_start + 4,
                fixed_end,
                ZipSpanRole::LocalFields(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            push_region(
                ctx,
                &mut regions,
                fixed_end,
                name_end,
                ZipSpanRole::LocalName(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            push_region(
                ctx,
                &mut regions,
                name_end,
                extra_end,
                ZipSpanRole::LocalExtra(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            push_region(
                ctx,
                &mut regions,
                entry.data_start,
                entry.data_end()?,
                ZipSpanRole::CompressedPayload(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;

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
                        ctx,
                        &mut regions,
                        entry.data_end()?,
                        descriptor_end,
                        ZipSpanRole::DataDescriptor(
                            ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                        ),
                    )?;
                    push_region(
                        ctx,
                        &mut regions,
                        descriptor_end,
                        next,
                        ZipSpanRole::Padding {
                            entry: Some(
                                ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                            ),
                        },
                    )?;
                } else {
                    push_region(
                        ctx,
                        &mut regions,
                        entry.data_end()?,
                        next,
                        ZipSpanRole::Padding {
                            entry: Some(
                                ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                            ),
                        },
                    )?;
                }
            }
        }

        let mut central_order = ctx.collection_vec(entries.len(), "ZIP ledger central order")?;
        for entry in ctx.admit_iter(entries, "visit ZIP ledger central entries")? {
            ctx.reserve_capacity(&mut central_order, 1, "ZIP ledger central slot")?;
            central_order.push(entry);
        }
        ctx.stable_sort_by(
            &mut central_order,
            |value| &value.central_start,
            Ord::cmp,
            "ZIP ledger central order",
        )?;
        let mut central_end = central_begin;
        for entry in ctx.admit_iter(&central_order, "visit ZIP ledger central order")? {
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
                ctx,
                &mut regions,
                entry.central_start,
                entry.central_start + 4,
                ZipSpanRole::CentralSignature(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            push_region(
                ctx,
                &mut regions,
                entry.central_start + 4,
                fixed_end,
                ZipSpanRole::CentralFields(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            push_region(
                ctx,
                &mut regions,
                fixed_end,
                name_end,
                ZipSpanRole::CentralName(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            push_region(
                ctx,
                &mut regions,
                name_end,
                extra_end,
                ZipSpanRole::CentralExtra(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            push_region(
                ctx,
                &mut regions,
                extra_end,
                record_end,
                ZipSpanRole::CentralComment(
                    ctx.copy_retained_text(&entry.name, "ZIP ledger entry name")?,
                ),
            )?;
            central_end = central_end.max(record_end);
        }

        classify_end_records(ctx, bytes, central_end, len, &mut regions)?;
        Ok::<_, CodecError>(regions)
    })?;
    partition(ctx, len, &regions)
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
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut offset: u64,
    len: u64,
    regions: &mut Vec<PhysicalSpan>,
) -> Result<(), CodecError> {
    while offset < len {
        ctx.charge_work(1, "visit ZIP end records")?;
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
        push_region(ctx, regions, offset, end, role)?;
        offset = end;
    }
    Ok(())
}

fn partition(
    ctx: &DecodeContext<'_>,
    len: u64,
    regions: &[PhysicalSpan],
) -> Result<Vec<PhysicalSpan>, CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "ZIP ledger partition indices")?;
    let mut boundaries = BTreeSet::from([0_u64, len]);
    for region in ctx.admit_iter(regions, "visit ZIP ledger regions")? {
        if region.end > len || region.start > region.end {
            return Err(CodecError::Malformed(
                "invalid physical ledger region".into(),
            ));
        }
        index_storage.with_storage(|| {
            ctx.insert_btree_set(&mut boundaries, region.start, "ZIP ledger boundaries")
        })?;
        index_storage.with_storage(|| {
            ctx.insert_btree_set(&mut boundaries, region.end, "ZIP ledger boundaries")
        })?;
    }
    let mut points = index_storage
        .with_storage(|| ctx.collection_vec(boundaries.len(), "ZIP ledger boundary points"))?;
    for &point in ctx.admit_iter(&boundaries, "visit ZIP ledger boundaries")? {
        ctx.reserve_capacity(&mut points, 1, "ZIP ledger point slot")?;
        points.push(point);
    }
    let mut ordered_regions = index_storage
        .with_storage(|| ctx.collection_vec(regions.len(), "ZIP ledger ordered regions"))?;
    for region in ctx.admit_iter(regions, "visit ZIP ledger ordered regions")? {
        ctx.reserve_capacity(&mut ordered_regions, 1, "ZIP ledger ordered slot")?;
        ordered_regions.push(region);
    }
    ctx.stable_sort_by_key(
        &mut ordered_regions,
            |value| (value.start,value.end),
            Ord::cmp,
        "ZIP ledger ordered regions",
    )?;
    let mut region_index = 0_usize;
    let mut spans = Vec::new();
    for pair in ctx.admit_iter(&points, "visit ZIP ledger intervals")?.windows(std::num::NonZeroUsize::new(2).ok_or_else(|| CodecError::Malformed("ZIP interval width is zero".into()))?) {
        let (start, end) = (pair[0], pair[1]);
        while ordered_regions
            .get(region_index)
            .is_some_and(|region| region.end <= start)
        {
            ctx.charge_work(1, "visit ZIP ledger interval owners")?;
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
        ctx.reserve_vec(&mut spans, 1, "ZIP ledger spans")?;
        spans.push(PhysicalSpan {
            start,
            end,
            role: owner.role.copy_with_context(ctx)?,
        });
    }
    Ok(spans)
}

/// Storage for one member, recording a malformed declaration as an attribute.
///
/// A stored member declaring fewer compressed than uncompressed bytes has no
/// usable stored span; the payload stands alone and the declaration is reported.
fn declared_storage(
    ctx: &DecodeContext<'_>,
    compression: ZipCompression,
    compressed_size: u64,
    uncompressed_size: u64,
    attributes: &mut BTreeMap<String, String>,
) -> Result<cadmpeg_core::container::EntryStorage, CodecError> {
    match compression.storage(compressed_size, uncompressed_size) {
        Ok(storage) => Ok(storage),
        Err(message) => {
            ctx.insert_btree_map(
                attributes,
                ctx.copy_retained_text("storage_declaration", "ZIP storage declaration key")?,
                ctx.format_retained(
                    format_args!("{message}: {compressed_size}/{uncompressed_size}"),
                    "ZIP storage declaration value",
                )?,
                "ZIP storage declaration attribute",
            )?;
            Ok(cadmpeg_core::container::EntryStorage::payload_only(
                cadmpeg_core::container::VerbatimLabel::Stored,
                uncompressed_size,
            ))
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

    fn summary_refuses(dimension: ResourceDimension, limit: u64, operation: &str) {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "part.p21",
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .expect("start ZIP member");
        writer.write_all(b"part").expect("write ZIP member");
        let bytes = writer.finish().expect("finish ZIP").into_inner();

        let setup_arena = DecodeArena::new();
        let setup_policy = DecodePolicy::service();
        let (setup_ctx, root) = DecodeContext::from_root_bytes(&bytes, &setup_arena, &setup_policy)
            .expect("root fits setup policy");
        let snapshot = ArchiveSnapshot::new(&setup_ctx, root).expect("ZIP snapshot");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = limit
                    + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                        cadmpeg_core::container::ContainerEntry,
                    >());
            }
            _ => panic!("test only selects collection or retained limits"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("root fits selected policy");
        assert!(matches!(
            snapshot.container_entries(&ctx, |_| cadmpeg_core::container::ContainerRole::Stream),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == dimension && refusal.operation == operation
        ));
    }

    #[test]
    fn central_name_scan_refuses_before_reading_records() {
        let mut bytes = [0u8; 49];
        bytes[..4].copy_from_slice(b"PK\x01\x02");
        bytes[28..30].copy_from_slice(&3u16.to_le_bytes());
        bytes[46..].copy_from_slice(b"abc");
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
        assert_eq!(super::reject_duplicate_central_names(&ctx, &bytes, 0).expect("one central name"), 1);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) = super::reject_duplicate_central_names(&ctx, &bytes, 0).expect_err("visit work") else { panic!("refusal") };
        assert_eq!(first.operation, "visit ZIP central name");
        let CodecError::ResourceLimit(repeated) = ctx.charge_work(1, "later").expect_err("fused refusal") else { panic!("refusal") };
        assert_eq!(first, repeated);
    }

    #[test]
    fn name_probe_reads_metadata_without_opening_local_headers() {
        let mut bytes = archive_bytes();
        bytes[..4].fill(0);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
        assert!(ArchiveSnapshot::contains_name(&ctx, root, "stored.bin").expect("metadata name"));
        assert!(!ArchiveSnapshot::contains_name(&ctx, root, "absent").expect("absent metadata name"));
    }

    #[test]
    fn unsupported_zip_compression_refuses_before_error_formatting() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let CodecError::ResourceLimit(first) = super::ZipCompression::from_zip(&ctx, CompressionMethod::BZIP2, "entry").expect_err("error format work") else { panic!("refusal") };
        assert_eq!(first.operation, "ZIP compression error");
        let CodecError::ResourceLimit(repeated) = ctx.charge_work(1, "later").expect_err("fused refusal") else { panic!("refusal") };
        assert_eq!(first, repeated);
    }

    #[test]
    fn zip_summary_entries_refuse_collection_limit() {
        summary_refuses(
            ResourceDimension::CollectionItems,
            0,
            "ZIP container summaries",
        );
    }

    #[test]
    fn zip_summary_attributes_refuse_collection_limit() {
        summary_refuses(
            ResourceDimension::CollectionItems,
            1,
            "ZIP summary attributes",
        );
    }

    #[test]
    fn zip_summary_text_refuses_retained_limit() {
        summary_refuses(
            ResourceDimension::RetainedBytes,
            3,
            "ZIP summary attribute key",
        );
    }

    #[test]
    fn zip_end_search_admits_work_before_scanning_non_candidates() {
        let bytes = [0_u8; 4096];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = super::preflight_central_directory(&ctx, &bytes)
            .expect_err("search must be admitted even without a candidate");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "ZIP end record search")
        );
    }

    #[test]
    fn zip64_end_search_admits_its_full_span() {
        let mut bytes = [0_u8; 130];
        bytes[88..92].copy_from_slice(b"PK\x06\x07");
        bytes[108..112].copy_from_slice(b"PK\x05\x06");
        cadmpeg_test_support::bytes::put_u16(&mut bytes, 118, u16::MAX);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = super::central_directory_inventory(&ctx, &bytes, 108)
            .expect_err("ZIP64 search is admitted before seeking a record");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "ZIP64 end record search")
        );
    }

    #[test]
    fn zip_payload_hash_and_expansion_admit_caller_work() {
        for method in [
            CompressionMethod::Stored,
            CompressionMethod::Deflated,
            CompressionMethod::Zstd,
        ] {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            writer
                .start_file(
                    "part",
                    SimpleFileOptions::default().compression_method(method),
                )
                .expect("entry");
            writer.write_all(b"part").expect("payload");
            let bytes = writer.finish().expect("archive").into_inner();
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshot");
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let other_arena = DecodeArena::new();
            let (other, _) =
                DecodeContext::from_root_bytes(&[], &other_arena, &policy).expect("fresh context");
            let error = snapshot
                .open(&other, "part")
                .expect_err("hash or input work is admitted");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
            );
        }
    }

    #[test]
    fn expanded_zip_crc_refuses_before_hashing_the_output() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "part",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("entry");
        writer.write_all(b"part").expect("payload");
        let bytes = writer.finish().expect("archive").into_inner();
        let arena = DecodeArena::new();
        let (service, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let snapshot = ArchiveSnapshot::new(&service, root).expect("snapshot");
        let entry = &snapshot.entries[0];
        let mut policy = DecodePolicy::service();
        // Two read calls and one four-byte output copy consume this allowance.
        policy.limits.max_work_units = 2 * 16 * 1024 + 4;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fresh root");
        let mut reader = Cursor::new(b"part");
        let error = ArchiveSnapshot::open_expanded(&ctx, entry, |chunk| {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(chunk.len()),
                "ZIP expansion step",
            )?;
            let read = std::io::Read::read(&mut reader, chunk).map_err(CodecError::Io)?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(read),
                "ZIP expansion copy",
            )?;
            Ok(read)
        })
        .expect_err("expanded CRC needs its own work");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "ZIP payload CRC"));
    }

    #[test]
    fn zip_storage_declaration_admits_its_retained_text() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fresh root");
        let error = super::declared_storage(
            &ctx,
            ZipCompression::Stored,
            1,
            2,
            &mut std::collections::BTreeMap::new(),
        )
        .expect_err("declaration text needs bytes before allocation");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "ZIP storage declaration key"));
    }

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
                .physical_ledger(&ctx)
                .expect("empty ZIP has a complete physical ledger"),
            vec![PhysicalSpan {
                start: 0,
                end: cadmpeg_core::decode::u64_from_index(bytes.len()),
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
    fn physical_ledger_local_order_refuses_at_caller_limit() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("archive root");
        let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items =
            cadmpeg_core::decode::u64_from_index(snapshot.entries().len()) - 1;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
        assert!(matches!(snapshot.physical_ledger(&limited),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "ZIP ledger local order"));
    }

    fn assert_ledger_sort_work_refusal(operation: &str) {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("archive root");
        let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 0;
        let mut target_charges = 0;
        for _ in 0..256 {
            let (limited, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
            let error = snapshot
                .physical_ledger(&limited)
                .expect_err("ledger must refuse");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("expected work refusal: {error:?}");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            let threshold = limit
                .used
                .checked_add(limit.additional)
                .expect("finite test budget");
            if limit.operation == operation {
                target_charges += 1;
            }
            // The sort admits its key scan, then its comparison work.
            if limit.operation == operation && target_charges == 2 {
                policy.limits.max_work_units = threshold - 1;
                let (limited, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
                assert!(matches!(snapshot.physical_ledger(&limited),
                    Err(CodecError::ResourceLimit(ref refusal))
                        if refusal.dimension == ResourceDimension::WorkUnits
                            && refusal.operation == operation));
                return;
            }
            policy.limits.max_work_units = threshold;
        }
        panic!("{operation} comparison work was not reached");
    }

    #[test]
    fn physical_ledger_local_sort_refuses_work() {
        assert_ledger_sort_work_refusal("ZIP ledger local order");
    }

    #[test]
    fn physical_ledger_central_sort_refuses_work() {
        assert_ledger_sort_work_refusal("ZIP ledger central order");
    }

    #[test]
    fn physical_ledger_region_sort_refuses_work() {
        assert_ledger_sort_work_refusal("ZIP ledger ordered regions");
    }

    #[test]
    fn partition_sort_preserves_equal_region_order() {
        let regions = [
            PhysicalSpan {
                start: 4,
                end: 8,
                role: ZipSpanRole::EndRecord,
            },
            PhysicalSpan {
                start: 0,
                end: 4,
                role: ZipSpanRole::Zip64EndRecord,
            },
            PhysicalSpan {
                start: 0,
                end: 4,
                role: ZipSpanRole::Zip64EndLocator,
            },
        ];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");
        assert_eq!(
            super::partition(&ctx, 8, &regions).expect("complete partition"),
            vec![regions[1].clone(), regions[0].clone()]
        );
    }

    #[test]
    fn physical_ledger_entry_name_refuses_at_retained_limit() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("archive root");
        let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
            snapshot.entries()[0].name.len()
                + snapshot.entries().len() * std::mem::size_of::<&EntryRecord>(),
        ) - 1;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
        assert!(matches!(snapshot.physical_ledger(&limited),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "ZIP ledger entry name"));
    }

    #[test]
    fn container_summaries_refuse_at_caller_limit() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("archive root");
        let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items =
            cadmpeg_core::decode::u64_from_index(snapshot.entries().len()) - 1;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
        assert!(
            matches!(snapshot.container_entries(&limited, |_| cadmpeg_core::container::ContainerRole::Auxiliary),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "ZIP container summaries")
        );
    }

    #[test]
    fn container_summary_name_refuses_at_retained_limit() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("archive root");
        let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(
            snapshot.entries()[0].name.len()
                + snapshot.entries().len()
                    * std::mem::size_of::<cadmpeg_core::container::ContainerEntry>(),
        ) - 1;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
        assert!(matches!(
            snapshot.container_entries(&limited, |_| cadmpeg_core::container::ContainerRole::Auxiliary),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "ZIP summary attribute value"
        ));
        let entry = &snapshot.entries()[0];
        let attribute_bytes = "crc32".len()
            + 8
            + "header_offset".len()
            + entry.header_start.to_string().len()
            + "data_offset".len()
            + entry.data_start.to_string().len()
            + "central_header_offset".len()
            + entry.central_start.to_string().len();
        let node_bytes = 22 * std::mem::size_of::<String>()
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<String>();
        // Four attribute texts and nine admitted tree nodes precede the first entry name.
        policy.limits.max_retained_bytes += cadmpeg_core::decode::u64_from_index(attribute_bytes + 9 * node_bytes);
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
        assert!(
            matches!(snapshot.container_entries(&limited, |_| cadmpeg_core::container::ContainerRole::Auxiliary),
            Err(CodecError::ResourceLimit(limit))
                if limit.operation == "ZIP summary entry name")
        );
    }

    fn assert_ledger_collection_refusal(operation: &str) {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("archive root");
        let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        for _ in 0..256 {
            let (limited, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
            let error = snapshot
                .physical_ledger(&limited)
                .expect_err("ledger must refuse");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("expected {operation} refusal: {error:?}");
            };
            assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
            let threshold = limit
                .used
                .checked_add(limit.additional)
                .expect("finite test budget");
            if limit.operation == operation {
                policy.limits.max_collection_items = threshold - 1;
                let (limited, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
                assert!(matches!(snapshot.physical_ledger(&limited),
                    Err(CodecError::ResourceLimit(ref refusal)) if refusal.operation == operation));
                return;
            }
            policy.limits.max_collection_items = threshold;
        }
        panic!("{operation} was not reached");
    }

    #[test]
    fn physical_ledger_regions_refuse_at_caller_limit() {
        assert_ledger_collection_refusal("ZIP ledger regions");
    }

    #[test]
    fn physical_ledger_central_order_refuses_at_caller_limit() {
        assert_ledger_collection_refusal("ZIP ledger central order");
    }

    #[test]
    fn physical_ledger_boundaries_refuse_at_caller_limit() {
        assert_ledger_collection_refusal("ZIP ledger boundaries");
    }

    #[test]
    fn physical_ledger_boundary_points_refuse_at_caller_limit() {
        assert_ledger_collection_refusal("ZIP ledger boundary points");
    }

    #[test]
    fn physical_ledger_ordered_regions_refuse_at_caller_limit() {
        assert_ledger_collection_refusal("ZIP ledger ordered regions");
    }

    #[test]
    fn physical_ledger_spans_refuse_at_caller_limit() {
        assert_ledger_collection_refusal("ZIP ledger spans");
    }

    #[test]
    fn physical_ledger_partition_role_refuses_at_retained_limit() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                .expect("archive root");
        let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        for _ in 0..256 {
            let (limited, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
            let error = snapshot
                .physical_ledger(&limited)
                .expect_err("ledger must refuse");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("expected retained refusal: {error:?}");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            let threshold = limit
                .used
                .checked_add(limit.additional)
                .expect("finite test budget");
            if limit.operation == "ZIP partition role name" {
                policy.limits.max_retained_bytes = threshold - 1;
                let (limited, _) =
                    DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited root");
                assert!(matches!(snapshot.physical_ledger(&limited),
                    Err(CodecError::ResourceLimit(ref refusal))
                        if refusal.operation == "ZIP partition role name"));
                return;
            }
            policy.limits.max_retained_bytes = threshold;
        }
        panic!("ZIP partition role name was not reached");
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
                    && limit.operation == "ZIP end record search"
        ));
        // The end candidate and two headers precede the third header's admission.
        policy.limits.max_work_units += cadmpeg_core::decode::u64_from_index(bytes.len());
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
    fn zip_dependency_workspace_refuses_before_indexing() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("archive fits root policy");
        assert!(matches!(
            ArchiveSnapshot::new(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "ZIP indexing workspace"
        ));
    }

    #[test]
    fn zip_indexing_workspace_is_shared_scoped_and_released() {
        let bytes = archive_bytes();
        for probe in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            // Input storage plus three declared metadata records; no ZIP comment.
            let indexing_bytes = 64 * cadmpeg_core::decode::u64_from_index(bytes.len()) + 3 * 1024;
            // Six insertion-path nodes and the three decoded duplicate names.
            let duplicate_bytes = 6 * (11 * std::mem::size_of::<String>()
                + 16 * std::mem::size_of::<usize>() + 2 * std::mem::align_of::<String>()) + 30;
            policy.limits.max_materialized_bytes = indexing_bytes
                + if probe { 0 } else { cadmpeg_core::decode::u64_from_index(duplicate_bytes) };
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            if probe {
                assert!(ArchiveSnapshot::contains_name(&ctx, root, "stored.bin").expect("probe"));
            } else {
                assert_eq!(
                    ArchiveSnapshot::new(&ctx, root)
                        .expect("snapshot")
                        .entries()
                        .len(),
                    3
                );
            }
            ctx.reserve_scoped(policy.limits.max_materialized_bytes, "index released")
                .expect("dependency workspace is released");

            policy.limits.max_materialized_bytes = indexing_bytes - 1;
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            let refused = if probe {
                ArchiveSnapshot::contains_name(&ctx, root, "stored.bin").map(|_| ())
            } else {
                ArchiveSnapshot::new(&ctx, root).map(|_| ())
            };
            assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "ZIP indexing workspace"));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        assert!(ArchiveSnapshot::contains_name(&ctx, root, "stored.bin").expect("names are scoped"));
    }

    fn archive_with_false_end_record(count: u16, comment: u16) -> Vec<u8> {
        let mut bytes = archive_bytes();
        let end = bytes.windows(4).rposition(|window| window == b"PK\x05\x06")
            .expect("end record");
        bytes[end + 20..end + 22].copy_from_slice(&22_u16.to_le_bytes());
        let fake = bytes.len();
        bytes.resize(fake + 22, 0);
        bytes[fake..fake + 4].copy_from_slice(b"PK\x05\x06");
        bytes[fake + 8..fake + 10].copy_from_slice(&count.to_le_bytes());
        bytes[fake + 10..fake + 12].copy_from_slice(&count.to_le_bytes());
        bytes[fake + 20..fake + 22].copy_from_slice(&comment.to_le_bytes());
        bytes
    }

    #[test]
    fn zip_failed_end_candidate_admits_declared_capacity() {
        let bytes = archive_with_false_end_record(64, 0);
        assert_eq!(zip::ZipArchive::new(Cursor::new(bytes.as_slice())).expect("earlier directory").len(), 3);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 63;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        assert!(matches!(ArchiveSnapshot::new(&ctx, root),
            Err(CodecError::ResourceLimit(limit)) if limit.operation == "ZIP central directory entries"));
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let admission = super::preflight_central_directory(&ctx, &bytes).expect("admission");
        // Input-derived metadata, 64 declared records and the outer 22-byte comment.
        assert_eq!(admission.workspace, 64 * bytes.len() as u64 + 64 * 1024 + 22);
        // Two candidates plus the final search, with comment initialization.
        assert_eq!(admission.work, (128 * bytes.len() as u64 + 22) * 3);
    }

    #[test]
    fn zip_truncated_candidate_comment_is_admitted_before_indexing() {
        let bytes = archive_with_false_end_record(3, u16::MAX);
        assert_eq!(zip::ZipArchive::new(Cursor::new(bytes.as_slice())).expect("earlier directory").len(), 3);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Input-derived metadata, three declared records and a truncated comment.
        let workspace = 64 * bytes.len() as u64 + 3 * 1024 + u64::from(u16::MAX);
        policy.limits.max_materialized_bytes = workspace - 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = ArchiveSnapshot::new(&ctx, root).expect_err("dependency allocation is admitted");
        assert!(matches!(&error, CodecError::ResourceLimit(limit) if limit.operation == "ZIP indexing workspace"));
        let CodecError::ResourceLimit(original) = error else { panic!("resource refusal"); };
        assert!(matches!(ctx.charge_work(1, "after refusal"),
            Err(CodecError::ResourceLimit(repeated)) if repeated == original));
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
                    && limit.operation == "ZIP end record search"
        ));
        policy.limits.max_work_units += cadmpeg_core::decode::u64_from_index(bytes.len()) + 1;
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
            let name_len =
                usize::from(u16::from_le_bytes([bytes[record + 28], bytes[record + 29]]));
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
        let entries = snapshot
            .container_entries(&ctx, |_| ContainerRole::Stream)
            .expect("container summary fits policy");
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
    fn archive_name_lookup_refuses_before_key_comparison() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("setup");
        let snapshot = ArchiveSnapshot::new(&setup, root).expect("snapshot");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let CodecError::ResourceLimit(first) = snapshot.entry(&ctx, "stored.bin").expect_err("lookup work") else { panic!("resource refusal") };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        let CodecError::ResourceLimit(repeated) = snapshot.entry(&ctx, "absent").expect_err("fused lookup") else { panic!("resource refusal") };
        assert_eq!(first, repeated);
    }

    #[test]
    fn snapshot_opens_supported_entries_after_parser_drop() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        assert_eq!(snapshot.entries().len(), 3);
        let stored = snapshot.entry(&ctx, "stored.bin").expect("lookup admission").expect("stored record");
        let deflated = snapshot.entry(&ctx, "deflated.bin").expect("lookup admission").expect("deflated record");
        let zstd = snapshot.entry(&ctx, "zstd.bin").expect("lookup admission").expect("Zstandard record");
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
                .entry(&ctx, "deflated.bin").expect("lookup admission")
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
        let compressed_size = u32::try_from(
            entry.compressed_size + cadmpeg_core::decode::u64_from_index(suffix.len()),
        )
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
        let mut detached = second.entry(&ctx, "stored.bin").expect("lookup admission").expect("entry exists").clone();
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
            let entry = snapshot.entry(&ctx, name).expect("lookup admission").expect("entry exists");
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
