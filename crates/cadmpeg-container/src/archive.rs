// SPDX-License-Identifier: Apache-2.0
//! Single-pass ZIP metadata snapshots with budgeted entry opening.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use cadmpeg_core::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_core::{CodecError, ContainerEntry};
mod entry;
mod index;
mod local;
mod probe;

/// Compression methods supported by [`ArchiveSnapshot::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipCompression {
    /// The entry payload is stored directly in the archive.
    Stored,
    /// The entry payload is a raw-DEFLATE member.
    Deflate,
    /// The entry payload is a Zstandard frame.
    Zstd,
    /// A declared method whose payload cannot be decoded by this service.
    Unsupported(u16),
}

impl ZipCompression {
    /// Returns the stable container-summary label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Deflate => "deflate",
            Self::Zstd => "zstd",
            Self::Unsupported(_) => "unsupported",
        }
    }

    /// Storage of a member with these declared stored and expanded sizes.
    ///
    /// A stored member whose declared compressed size is smaller than its
    /// uncompressed size is a malformed declaration, reported here
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
            Self::Unsupported(_) => {
                return Ok(EntryStorage::payload_only(
                    VerbatimLabel::None,
                    compressed_size,
                ))
            }
        };
        Ok(EntryStorage::Compressed {
            method,
            stored: Some(compressed_size),
            expanded: Some(uncompressed_size),
        })
    }
}

/// ZIP declaration facts retained after the parser is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRecord {
    /// Entry name from the central directory or a recovered local header.
    pub name: String,
    /// Compression method admitted by the snapshot.
    pub compression: ZipCompression,
    /// Whether the declared payload is encrypted.
    pub encrypted: bool,
    /// CRC-32 of the uncompressed payload.
    pub crc32: u32,
    /// Compressed payload size.
    pub compressed_size: u64,
    /// Uncompressed payload size.
    pub uncompressed_size: u64,
    /// Physical start of the local header.
    pub header_start: u64,
    /// Physical start of the payload, when the local member frame is readable.
    pub data_start: Option<u64>,
    /// Physical start of the central record, or local header during recovery.
    pub central_start: u64,
    declaration: Declaration,
    utf8_name: bool,
    local_header_error: Option<&'static str>,
    unreadable_range: Option<ByteRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Declaration {
    Central(ByteRange),
    Local(ByteRange),
}

impl EntryRecord {
    /// Whether this entry is identified by a recovered local declaration.
    pub fn is_recovered(&self) -> bool {
        matches!(self.declaration, Declaration::Local(_))
    }

    /// Exact central record or recovered local-header declaration.
    pub fn declaration_range(&self) -> ByteRange {
        match self.declaration {
            Declaration::Central(range) | Declaration::Local(range) => range,
        }
    }

    /// Whether the declaration uses the Unicode filename flag.
    pub fn uses_utf8_name_encoding(&self) -> bool {
        self.utf8_name
    }

    /// Exact stored bytes retained when opening a member fails. An unreadable
    /// local frame is retained whole, without claiming a payload boundary.
    pub fn stored_range(&self) -> Result<ByteRange, CodecError> {
        match self.data_start {
            Some(start) => Ok(ByteRange {
                start,
                end: self.data_end()?,
            }),
            None => self.unreadable_range.ok_or_else(|| {
                CodecError::malformed("ZIP unreadable member has no bounded source span")
            }),
        }
    }

    /// Returns the exclusive compressed-payload boundary.
    pub fn data_end(&self) -> Result<u64, CodecError> {
        self.data_start
            .ok_or_else(|| CodecError::malformed("ZIP member has no readable payload frame"))?
            .checked_add(self.compressed_size)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("ZIP data range overflows for {}", self.name))
            })
    }
}

/// A ZIP declaration snapshot over one decode-session root view.
#[derive(Debug)]
pub struct ArchiveSnapshot<'a> {
    root: View<'a>,
    central_start: u64,
    entries: Vec<EntryRecord>,
    by_name: BTreeMap<String, usize>,
    recovery: Option<Box<local::Recovery>>,
}

/// Dependency index over the admitted central directory.
struct ZipIndex<'bytes> {
    archive: zip::ZipArchive<index::NamespaceReader<'bytes>>,
}

enum Admission<T> {
    Indexed(T),
    Unreadable(CodecError),
}

impl<'bytes> ZipIndex<'bytes> {
    fn admit(ctx: &DecodeContext<'_>, bytes: &'bytes [u8]) -> Result<Admission<Self>, CodecError> {
        let admitted = match directory_admission(ctx, bytes)? {
            Admission::Indexed(admitted) => admitted,
            Admission::Unreadable(error) => return Ok(Admission::Unreadable(error)),
        };
        let archive =
            match zip::ZipArchive::new(index::NamespaceReader::new(bytes, admitted.end_record)) {
                Ok(archive) => archive,
                Err(
                    error @ (zip::result::ZipError::InvalidArchive(_)
                    | zip::result::ZipError::Io(_)),
                ) => {
                    return Ok(Admission::Unreadable(CodecError::malformed(format_args!(
                        "not a readable ZIP: {error}"
                    ))));
                }
                Err(error) => {
                    return Err(CodecError::malformed(format_args!(
                        "not a readable ZIP: {error}"
                    )))
                }
            };
        Ok(Admission::Indexed(Self { archive }))
    }
}

impl<'a> ArchiveSnapshot<'a> {
    /// Parses the directory, or recovers bounded local frames when it is unreadable.
    pub fn new(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let index = match ZipIndex::admit(ctx, root.window())? {
            Admission::Indexed(index) => index,
            Admission::Unreadable(error) => return local::recover(root, error),
        };
        let archive = &index.archive;
        let archive_central_start = archive.central_directory_start();
        let central_entry_count =
            reject_duplicate_central_names(root.window(), archive_central_start)?;
        if central_entry_count != archive.len() {
            return Err(CodecError::Malformed(
                "ZIP central directory contains duplicate entry names".into(),
            ));
        }
        let mut names = BTreeSet::new();
        let mut entries = Vec::new();
        let mut central = archive_central_start;
        for ordinal in 0..archive.len() {
            let name = archive
                .name_for_index(ordinal)
                .ok_or_else(|| CodecError::malformed("ZIP indexed name is absent"))?
                .map_err(|error| {
                    CodecError::malformed(format_args!("invalid ZIP entry name: {error}"))
                })?;
            if !names.insert(name.clone().into_owned()) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate ZIP entry name {name}"
                )));
            }
            let admission = entry::read(
                root.window(),
                central,
                name.into_owned(),
                archive.offset(),
                archive_central_start,
            )?;
            let (record, next) = match admission {
                Admission::Indexed(record) => record,
                Admission::Unreadable(error) => {
                    drop(index);
                    return local::recover(root, error);
                }
            };
            entries.push(record);
            central = next;
        }
        entry::frame_unreadable_members(&mut entries, archive_central_start);
        drop(index);
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
            recovery: None,
        })
    }

    /// Tests an indexed name without opening payloads or applying compression
    /// and encryption admission. Temporary index storage remains scoped.
    pub fn contains_matching_name(
        ctx: &DecodeContext<'_>,
        root: View<'_>,
        mut matches: impl FnMut(&str) -> bool,
    ) -> Result<bool, CodecError> {
        let index = match ZipIndex::admit(ctx, root.window())? {
            Admission::Indexed(index) => index,
            Admission::Unreadable(error) => return Err(error),
        };
        for candidate in index.archive.file_names() {
            let candidate = candidate.map_err(|error| {
                CodecError::malformed(format_args!("invalid ZIP entry name: {error}"))
            })?;
            if matches(&candidate) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether the bounded ZIP footer window contains an end-record signature.
    /// This is a probe hint; directory admission must still prove the archive.
    pub fn has_footer(bytes: &[u8]) -> bool {
        let tail = match bytes.len().checked_sub(65_557) {
            Some(start) => &bytes[start..],
            None => bytes,
        };
        tail.windows(4).any(|signature| signature == b"PK\x05\x06")
    }

    /// Reads only directory and footer evidence for seekable source detection.
    /// The returned image has rebased directory offsets and excludes member
    /// payloads and opaque archive comments;
    /// it must only be used for detection, never inspection or decoding.
    pub fn detection_image(
        ctx: &DecodeContext<'_>,
        source: &mut dyn cadmpeg_core::ReadSeek,
    ) -> Result<Option<Vec<u8>>, CodecError> {
        probe::read(ctx, source)
    }

    /// Returns declarations in directory order, or physical order during recovery.
    pub fn entries(&self) -> &[EntryRecord] {
        &self.entries
    }

    /// Finds an entry record by its exact archive name.
    pub fn entry(&self, name: &str) -> Option<&EntryRecord> {
        self.by_name.get(name).map(|index| &self.entries[*index])
    }

    /// Directory failure that required local recovery. A codec must report this
    /// through its loss vocabulary even if all recovered members open.
    pub fn recovery_reason(&self) -> Option<&str> {
        self.recovery
            .as_ref()
            .map(|recovery| recovery.reason.as_str())
    }

    /// Unframed remainder after the bounded local walk. Offsets are relative to
    /// this archive view. These bytes remain source-only, never expanded data.
    pub fn recovery_tail(&self) -> Option<ByteRange> {
        self.recovery.as_ref().and_then(|recovery| recovery.tail)
    }

    /// Opens an exact entry name as a borrowed stored slice or budgeted expanded view.
    pub fn open(&self, ctx: &DecodeContext<'a>, name: &str) -> Result<View<'a>, CodecError> {
        let entry = self
            .entry(name)
            .ok_or_else(|| CodecError::malformed(format_args!("ZIP entry {name} is absent")))?;
        if let Some(error) = entry.local_header_error {
            return Err(CodecError::malformed(format_args!(
                "{}: {error}",
                entry.name
            )));
        }
        if entry.encrypted {
            return Err(CodecError::NotImplemented(format!(
                "encrypted ZIP entry {}",
                entry.name
            )));
        }
        let end = entry.data_end()?;
        let archive_start = cadmpeg_core::decode::u64_from_index(self.root.start());
        let absolute_start = archive_start
            .checked_add(
                entry
                    .data_start
                    .ok_or_else(|| CodecError::malformed("ZIP member payload frame is absent"))?,
            )
            .ok_or_else(|| {
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
            ZipCompression::Unsupported(method) => Err(CodecError::NotImplemented(format!(
                "ZIP compression {method:?} for {}",
                entry.name
            ))),
            ZipCompression::Stored => self.open_stored(ctx, entry, range),
            ZipCompression::Deflate => {
                let source = self.compressed_source(entry, range)?;
                let mut decoder = flate2::read::DeflateDecoder::new(source.window());
                let view = Self::open_expanded(ctx, entry, |chunk| {
                    let read = decoder.read(chunk).map_err(|error| {
                        CodecError::malformed(format_args!(
                            "cannot inflate {}: {error}",
                            entry.name
                        ))
                    })?;
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
            let read = read_chunk(&mut chunk)?;
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
        let mut output = Vec::new();
        for entry in &self.entries {
            let mut attributes = BTreeMap::new();
            attributes.insert("crc32".to_owned(), format!("{:08x}", entry.crc32));
            for (key, value) in [
                ("header_offset", entry.header_start),
                (
                    if entry.is_recovered() {
                        "recovered_local_header_offset"
                    } else {
                        "central_header_offset"
                    },
                    entry.central_start,
                ),
            ] {
                attributes.insert(key.to_owned(), format!("{value}"));
            }
            if let Some(reason) = self.recovery_reason() {
                attributes.insert("archive_recovery".to_owned(), reason.to_owned());
            }
            if let Some(offset) = entry.data_start {
                attributes.insert("data_offset".to_owned(), format!("{offset}"));
            }
            if let Some(error) = entry.local_header_error {
                attributes.insert("local_header_error".to_owned(), error.to_owned());
            }
            if let ZipCompression::Unsupported(method) = entry.compression {
                attributes.insert("compression".to_owned(), format!("{method:?}"));
            }
            if entry.encrypted {
                attributes.insert("encrypted".to_owned(), "true".to_owned());
            }
            let storage = if entry.encrypted {
                cadmpeg_core::container::EntryStorage::payload_only(
                    cadmpeg_core::container::VerbatimLabel::None,
                    entry.compressed_size,
                )
            } else {
                declared_storage(
                    entry.compression,
                    entry.compressed_size,
                    entry.uncompressed_size,
                    &mut attributes,
                )
            };
            output.push(ContainerEntry {
                name: entry.name.clone(),
                role: classify(&entry.name),
                storage,
                attributes,
            });
        }
        output
    }

    /// Partitions every physical archive byte by ZIP structural role.
    pub fn physical_ledger(&self) -> Result<Vec<PhysicalSpan>, CodecError> {
        physical_ledger(self.root.window(), &self.entries, self.central_start)
    }
}

#[derive(Debug)]
struct DirectoryPreflight {
    end_record: usize,
}

fn preflight_central_directory(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<DirectoryPreflight, CodecError> {
    match directory_admission(ctx, bytes)? {
        Admission::Indexed(admitted) => Ok(admitted),
        Admission::Unreadable(error) => Err(error),
    }
}

fn directory_admission(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Admission<DirectoryPreflight>, CodecError> {
    let mut first_error = None;
    let mut admitted = None;
    let candidates = bytes
        .windows(4)
        .enumerate()
        .rev()
        .filter(|(end, signature)| {
            *signature == b"PK\x05\x06"
                && View::u16_le_at(bytes, end + 20).is_some_and(|comment| {
                    end.checked_add(22 + usize::from(comment)) == Some(bytes.len())
                })
        })
        .map(|(end, _)| end)
        .collect::<Vec<_>>();
    let mut search = DirectorySearch {
        zip64_records: probe::Zip64Records::new(
            candidates
                .iter()
                .filter_map(|&end| zip64_locator_at(bytes, end))
                .map(cadmpeg_core::decode::u64_from_index)
                .collect(),
        ),
        central_headers: None,
    };
    for end in candidates {
        match central_directory_inventory(ctx, bytes, end, &mut search) {
            Ok(()) => {
                if admitted.is_some() {
                    return Err(CodecError::malformed(
                        "ZIP has ambiguous terminal central-directory declarations",
                    ));
                }
                admitted = Some(DirectoryPreflight { end_record: end });
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    Ok(match admitted {
        Some(admitted) => Admission::Indexed(admitted),
        None => Admission::Unreadable(
            first_error.unwrap_or_else(|| CodecError::Malformed("ZIP end record is absent".into())),
        ),
    })
}

/// Lookups shared by the candidate end records of one archive, so each
/// candidate searches the input in logarithmic or amortized constant time.
struct DirectorySearch {
    zip64_records: probe::Zip64Records,
    /// Every central-header signature offset, in order, once a candidate's
    /// directory is not at its canonical start.
    central_headers: Option<Vec<usize>>,
}

impl DirectorySearch {
    /// The first central-header signature that starts at or after `start`
    /// and ends by `end`.
    fn central_header(&mut self, bytes: &[u8], start: usize, end: usize) -> Option<usize> {
        if end > bytes.len() {
            return None;
        }
        let headers = self.central_headers.get_or_insert_with(|| {
            bytes
                .windows(4)
                .enumerate()
                .filter(|(_, window)| *window == b"PK\x01\x02")
                .map(|(offset, _)| offset)
                .collect()
        });
        headers
            .get(headers.partition_point(|&offset| offset < start))
            .copied()
            .filter(|&offset| offset + 4 <= end)
    }
}

fn zip64_locator_at(bytes: &[u8], end: usize) -> Option<usize> {
    end.checked_sub(20)
        .filter(|&start| bytes.get(start..start + 4) == Some(b"PK\x06\x07".as_slice()))
}

fn central_directory_inventory(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    end: usize,
    search: &mut DirectorySearch,
) -> Result<(), CodecError> {
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
    let locator_start = zip64_locator_at(bytes, end);
    let (count, directory_size, directory_start_hint, directory_end) = if let Some(locator_start) =
        locator_start
    {
        let hint = View::u64_le_at(bytes, locator_start + 8)
            .ok_or_else(|| CodecError::malformed("ZIP64 end locator is truncated"))?;
        let record_start = search
            .zip64_records
            .find(
                ctx,
                &mut std::io::Cursor::new(bytes),
                cadmpeg_core::decode::u64_from_index(locator_start),
                hint,
            )?
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or_else(|| CodecError::Malformed("ZIP64 end record is absent".into()))?;
        let count = View::u64_le_at(bytes, record_start + 32)
            .ok_or_else(|| CodecError::Malformed("ZIP64 entry count is truncated".into()))?;
        let size = View::u64_le_at(bytes, record_start + 40)
            .ok_or_else(|| CodecError::Malformed("ZIP64 directory size is truncated".into()))?;
        let start = View::u64_le_at(bytes, record_start + 48)
            .ok_or_else(|| CodecError::Malformed("ZIP64 directory offset is truncated".into()))?;
        (count, size, start, record_start)
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
        search_end
            .checked_sub(search_start)
            .ok_or_else(|| CodecError::Malformed("ZIP directory search range is invalid".into()))?;
        let start = search
            .central_header(bytes, search_start, search_end)
            .ok_or_else(|| CodecError::Malformed("ZIP central header is absent".into()))?;
        cadmpeg_core::decode::u64_from_index(start)
    };
    let entries_start = offset;
    for _ in 0..count {
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
    let declared_end = entries_start
        .checked_add(directory_size)
        .ok_or_else(|| CodecError::malformed("ZIP declared directory end overflows"))?;
    if offset < declared_end && signature_at(bytes, offset) == Some(*b"PK\x05\x05") {
        let size = u64::from(u16_at(
            bytes,
            offset
                + cadmpeg_core::decode::u64_from_index(
                    crate::layout::digital_signature::DATA_LENGTH,
                ),
        )?);
        offset = offset
            .checked_add(cadmpeg_core::decode::u64_from_index(
                crate::layout::digital_signature::LEN,
            ))
            .and_then(|end| end.checked_add(size))
            .ok_or_else(|| CodecError::malformed("ZIP directory signature end overflows"))?;
    }
    if offset != declared_end {
        return Err(CodecError::malformed(
            "ZIP directory count and declared size disagree",
        ));
    }
    Ok(())
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
    /// Uninterpreted ZIP bytes, including padding and unreadable local frames.
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
    let len = cadmpeg_core::decode::u64_from_index(bytes.len());
    let mut regions = Vec::new();
    let mut local_order: Vec<&EntryRecord> = entries.iter().collect();
    local_order.sort_by_key(|left| left.header_start);
    if central_begin > len {
        return Err(CodecError::Malformed(
            "ZIP central directory begins after the archive".into(),
        ));
    }
    for (index, entry) in local_order.iter().enumerate() {
        let Some(data_start) = entry.data_start else {
            let range = entry.stored_range()?;
            push_region(
                &mut regions,
                range.start,
                range.end,
                ZipSpanRole::Padding {
                    entry: Some(entry.name.clone()),
                },
            );
            continue;
        };
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
        if extra_end != data_start {
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
            data_start,
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

    let mut central_order: Vec<&EntryRecord> = entries
        .iter()
        .filter(|entry| !entry.is_recovered())
        .collect();
    central_order.sort_by_key(|left| left.central_start);
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
    let points: Vec<u64> = boundaries.into_iter().collect();
    let mut ordered_regions: Vec<&PhysicalSpan> = regions.iter().collect();
    ordered_regions.sort_by_key(|left| (left.start, left.end));
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
                "storage_declaration".to_owned(),
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

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    use super::{ArchiveSnapshot, EntryRecord, PhysicalSpan, ZipCompression, ZipSpanRole};

    #[test]
    fn indexed_names_preserve_cp437_and_utf8_identity() {
        for (stored, expected, legacy) in [("x", "é", true), ("水", "水", false)] {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            writer.start_file(stored, options).expect("entry");
            writer.write_all(b"payload").expect("payload");
            let mut bytes = writer.finish().expect("archive").into_inner();
            if legacy {
                let archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("index");
                let central = usize::try_from(archive.central_directory_start()).expect("offset");
                drop(archive);
                bytes[crate::layout::local_header::LEN] = 0x82;
                bytes[central + crate::layout::central_header::LEN] = 0x82;
            }
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            assert!(
                ArchiveSnapshot::contains_matching_name(&ctx, root, |name| name == expected)
                    .expect("name probe")
            );
            let archive = ArchiveSnapshot::new(&ctx, root).expect("snapshot");
            assert_eq!(archive.entries().len(), 1);
            assert_eq!(
                archive.open(&ctx, expected).expect("member").window(),
                b"payload"
            );
        }
    }

    #[test]
    fn payload_admission_is_local_to_opened_members() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, data) in [
            ("required", b"geometry".as_slice()),
            ("optional", b"preview".as_slice()),
        ] {
            writer.start_file(name, options).expect("member");
            writer.write_all(data).expect("payload");
        }
        let original = writer.finish().expect("archive").into_inner();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) = DecodeContext::from_root_bytes(&original, &arena, &policy).expect("root");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("index");
        let entry = snapshot.entry("optional").expect("optional");
        let local = usize::try_from(entry.header_start).expect("offset");
        let central = usize::try_from(entry.central_start).expect("offset");
        let payload = usize::try_from(entry.data_start.expect("readable frame")).expect("offset");
        for kind in [
            "encrypted",
            "unsupported",
            "crc",
            "local-signature",
            "local-length",
        ] {
            let mut bytes = original.clone();
            match kind {
                "encrypted" => {
                    cadmpeg_test_support::bytes::put_u16(&mut bytes, local + 6, 1);
                    cadmpeg_test_support::bytes::put_u16(&mut bytes, central + 8, 1);
                }
                "unsupported" => {
                    cadmpeg_test_support::bytes::put_u16(&mut bytes, local + 8, 12);
                    cadmpeg_test_support::bytes::put_u16(&mut bytes, central + 10, 12);
                }
                "local-signature" => bytes[local] ^= 1,
                "local-length" => {
                    cadmpeg_test_support::bytes::put_u16(&mut bytes, local + 26, u16::MAX);
                }
                _ => bytes[payload] ^= 1,
            }
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            let snapshot = ArchiveSnapshot::new(&ctx, root)
                .expect("bounded index survives optional payload defect");
            assert_eq!(
                snapshot
                    .open(&ctx, "required")
                    .expect("independent member")
                    .window(),
                b"geometry"
            );
            assert!(snapshot.open(&ctx, "optional").is_err(), "{kind}");
            assert!(!snapshot
                .physical_ledger()
                .expect("physical framing")
                .is_empty());
        }
    }

    #[test]
    fn zstd_member_with_a_window_past_the_allowance_is_skipped_without_fusing() {
        // A streamed frame as a level-22 encoder writes it: a 128 MiB window
        // and no content size. Laid out by `zip_fixture`: `zip` is built
        // without its `zstd` feature.
        let streamed = zip_fixture::zstd_streamed_frame(27, b"preview");
        let bytes = zip_fixture::archive(&[
            RawEntry::compressed("streamed.bin", zip_fixture::ZSTD, &streamed, b"preview"),
            RawEntry::compressed(
                "geometry.bin",
                zip_fixture::ZSTD,
                &zip_fixture::zstd_stored_frame(b"geometry"),
                b"geometry",
            ),
        ]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("index");
        let Err(CodecError::NotImplemented(message)) = snapshot.open(&ctx, "streamed.bin") else {
            panic!("a window past the per-expansion allowance is unsupported");
        };
        assert!(
            message.starts_with(&format!("Zstandard window of {} bytes", 1_u64 << 27))
                && message.ends_with("per-expansion allowance"),
            "{message}"
        );
        assert_eq!(ctx.resource_refusal(), None);
        assert_eq!(
            snapshot
                .open(&ctx, "geometry.bin")
                .expect("an independent member opens")
                .window(),
            b"geometry"
        );
        assert!(ctx.finish_session().is_ok());
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
                .physical_ledger()
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
            encrypted: false,
            crc32: u32::from_le_bytes(signature),
            compressed_size: 0,
            uncompressed_size: 0,
            header_start: 0,
            data_start: Some(30),
            central_start: 42,
            declaration: super::Declaration::Central(cadmpeg_core::decode::ByteRange {
                start: 42,
                end: 42,
            }),
            utf8_name: false,
            local_header_error: None,
            unreadable_range: None,
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

    /// DEFLATE `plain` the way a ZIP writer would: raw deflate, no zlib wrapper.
    fn deflate(plain: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(plain).expect("deflate accepts input");
        encoder.finish().expect("deflate finishes")
    }

    /// The mixed fixture: one stored, one deflated and one Zstandard entry.
    ///
    /// Laid out by [`zip_fixture::archive`] rather than by `zip::ZipWriter`, because `zip` is
    /// built without its `zstd` feature and its writer cannot emit method 93.  The layout is
    /// still judged by two parsers the builder does not share code with -- `zip::ZipArchive`,
    /// which indexes the names, and this crate's own central-directory and local-header
    /// readers -- and `the_raw_builder_and_zip_writer_produce_the_same_records` compares the
    /// builder against `ZipWriter` on the two methods both can write.
    fn archive_bytes() -> Vec<u8> {
        let zstd_plain = zip_fixture::zstd_golden_plaintext();
        zip_fixture::archive(&[
            RawEntry::stored("stored.bin", b"stored"),
            RawEntry::compressed(
                "deflated.bin",
                zip_fixture::DEFLATE,
                &deflate(b"deflated payload"),
                b"deflated payload",
            ),
            RawEntry::compressed(
                "zstd.bin",
                zip_fixture::ZSTD,
                zip_fixture::ZSTD_GOLDEN_FRAME,
                &zstd_plain,
            ),
        ])
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
        assert_eq!(
            super::partition(8, &regions).expect("complete partition"),
            vec![regions[1].clone(), regions[0].clone()]
        );
    }

    #[test]
    fn central_directory_indexes_every_entry() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (service_ctx, service_root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("build the decode context");
        assert_eq!(
            ArchiveSnapshot::new(&service_ctx, service_root)
                .expect("construct the archive snapshot")
                .entries()
                .len(),
            3
        );
    }

    #[test]
    fn zip_name_probe_retains_no_bytes() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        assert!(
            ArchiveSnapshot::contains_matching_name(&ctx, root, |name| name == "stored.bin")
                .expect("names are scoped")
        );
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
            .expect("build the decode context");
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
            .expect("build the decode context");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("directory remains readable")
                .entries()
                .len(),
            1
        );
    }

    #[test]
    fn false_end_record_cannot_replace_the_selected_directory() {
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
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("build the decode context");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("valid end record selects the directory")
                .entries()
                .len(),
            3
        );
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
            .expect("build the decode context");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("directory remains readable")
                .entries()
                .len(),
            1
        );
        let end = end + 9;
        let size = View::u32_le_at(&bytes, end + 12).expect("directory size");
        cadmpeg_test_support::bytes::put_u32(&mut bytes, end + 12, size + 9);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("build the decode context");
        assert_eq!(
            ArchiveSnapshot::new(&ctx, root)
                .expect("declared directory size includes its digital signature")
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
            zip_fixture::zstd_golden_plaintext()
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
                    .expect("build the decode context");
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
            .expect("build the decode context");
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
        detached.data_start = Some(u64::MAX);
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

        let zstd_plain = zip_fixture::zstd_golden_plaintext();
        for (name, expected) in [
            ("stored.bin", b"stored".as_slice()),
            ("deflated.bin", b"deflated payload".as_slice()),
            ("zstd.bin", zstd_plain.as_slice()),
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

    /// The raw fixture builder is not silently laying out a different archive from `ZipWriter`.
    ///
    /// `archive_bytes` cannot use `zip::ZipWriter` because `zip` without its `zstd` feature
    /// cannot write method 93, which replaces a well-tested writer with sixty lines in this repository.  On
    /// the two methods both can write, the records `ArchiveSnapshot` derives must agree, so a
    /// layout mistake in the builder shows up here rather than as a decoder failure.
    #[test]
    fn the_raw_builder_and_zip_writer_produce_the_same_records() {
        let mut written = zip::ZipWriter::new(Cursor::new(Vec::new()));
        written
            .start_file(
                "stored.bin",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .expect("stored entry starts");
        written.write_all(b"stored").expect("stored entry writes");
        written
            .start_file(
                "deflated.bin",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("deflated entry starts");
        written
            .write_all(b"deflated payload")
            .expect("deflated entry writes");
        let written = written.finish().expect("archive finishes").into_inner();

        let built = zip_fixture::archive(&[
            RawEntry::stored("stored.bin", b"stored"),
            RawEntry::compressed(
                "deflated.bin",
                zip_fixture::DEFLATE,
                &deflate(b"deflated payload"),
                b"deflated payload",
            ),
        ]);

        // Every LOCAL header must agree with its central record, and the end-of-central-directory
        // record must agree with what was actually written. `zip::ZipArchive` validates neither:
        // measured, it refuses a central-directory OFFSET that is off by one and accepts both a
        // wrong `central_size` and a wrong total entry count, and it never reads a local header
        // at all through this path. So the builder could emit an archive whose local headers say
        // "stored, crc 0, sizes transposed" while the central directory says "zstd, crc X" -- an
        // archive no streaming reader would agree with -- and the comparison below, which is
        // built entirely from central-directory fields, would stay green.
        //
        // A byte-for-byte comparison against `ZipWriter` is NOT the fix and was measured: both
        // producers emit 242 bytes for this pair but share only a 4-byte prefix, because
        // `ZipWriter` writes a different version-needed and a real modification time. Agreement
        // between the two headers is the property; identity with another writer is not.
        assert_local_headers_agree(&built);
        assert_local_headers_agree(&written);

        let arena = DecodeArena::new();
        let mut readings = Vec::new();
        for bytes in [&written, &built] {
            let (ctx, root) =
                DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
                    .expect("archive fits root policy");
            let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
            readings.push(
                snapshot
                    .entries()
                    .iter()
                    .map(|entry| {
                        let view = snapshot.open(&ctx, &entry.name).expect("entry opens");
                        (
                            entry.name.clone(),
                            entry.compression,
                            entry.crc32,
                            entry.uncompressed_size,
                            view.window().to_vec(),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }
        assert_eq!(readings[0], readings[1]);
    }

    /// The expansion loop reassembles a payload larger than one 16 KiB chunk, for BOTH methods.
    ///
    /// Every other fixture in this file is smaller than one read.  Measured, that leaves the
    /// classic `Read` misuse -- treating a short read as end of stream -- a no-op on the whole
    /// suite: the golden Zstandard entry reads `16384, 16384, 3584, 0`, so its only short read
    /// IS the last, and the deflate fixtures are 16 and 32 bytes.  A large enough deflate
    /// member makes `flate2` return short reads mid-stream, and under that misuse such a member
    /// decodes truncated while every other test stays green.  Deflate is by far the commonest
    /// method in real containers, so this is the arm that most needs the fixture.
    ///
    /// The payload is generated rather than committed, and is deliberately not uniform: a run of
    /// one byte compresses to almost nothing and would not produce multiple reads on the way
    /// out.
    #[test]
    fn a_payload_larger_than_one_read_is_reassembled_for_every_method() {
        let plain = zip_fixture::zstd_golden_large_plaintext();
        assert!(
            plain.len() > 16 * 1024 * 4,
            "the payload must span many reads"
        );
        let entries = [
            RawEntry::stored("stored.bin", &plain),
            RawEntry::compressed(
                "deflated.bin",
                zip_fixture::DEFLATE,
                &deflate(&plain),
                &plain,
            ),
            RawEntry::compressed(
                "zstd.bin",
                zip_fixture::ZSTD,
                zip_fixture::ZSTD_GOLDEN_LARGE_FRAME,
                &plain,
            ),
        ];
        let bytes = zip_fixture::archive(&entries);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        for entry in &entries {
            let record = snapshot.entry(&entry.name).expect("record");
            let view = snapshot.open(&ctx, &record.name).expect("entry opens");
            // The whole payload, not merely a successful call: a truncating decoder returns Ok
            // for as long as the guards downstream let it.
            assert_eq!(
                view.window(),
                plain.as_slice(),
                "{} did not round-trip in full",
                entry.name
            );
        }
    }

    /// `open_expanded` must not treat a SHORT read as end of stream, nor an error as one.
    ///
    /// This drives the expansion loop directly instead of through a decoder, and that is the
    /// point. The obvious fixture -- a
    /// payload larger than one 16 KiB chunk -- does NOT hold this property: measured, both
    /// `flate2` and `ruzstd` return exact 16 384-byte reads until the tail at 20 KB and at
    /// 200 KB (`16384 x 12, 3392, 0`), so a plant that returns after the first short read is a
    /// no-op on every fixture in this file. It is not a no-op on real input -- a large enough
    /// deflate member makes `flate2` return MID-STREAM short reads, and the member then decodes
    /// truncated -- but sizing a fixture until a dependency
    /// happens to fragment is a test of that dependency's buffering, not of this function,
    /// and it would go quiet the next time `flate2` changed.
    ///
    /// A chunk reader is permitted to return fewer bytes than the buffer holds at any time
    /// and `0` only at end of stream, so the reader below is a conforming one that exercises
    /// exactly the contract.
    #[test]
    fn open_expanded_is_not_defeated_by_a_short_read_or_an_error() {
        let plain = zip_fixture::zstd_golden_plaintext();
        // Only the record matters here: it declares `plain`'s size and CRC-32.
        let bytes = zip_fixture::archive(&[RawEntry::compressed(
            "probe",
            zip_fixture::DEFLATE,
            &deflate(&plain),
            &plain,
        )]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).expect("root");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let entry = snapshot.entry("probe").expect("record");

        // Short reads must be reassembled in full.
        for step in [1_usize, 7, 4096] {
            let mut rest = plain.as_slice();
            let view = ArchiveSnapshot::open_expanded(&ctx, entry, |chunk| {
                let n = step.min(chunk.len()).min(rest.len());
                chunk[..n].copy_from_slice(&rest[..n]);
                rest = &rest[n..];
                Ok(n)
            })
            .expect("expansion succeeds");
            assert_eq!(
                view.window(),
                plain.as_slice(),
                "a {step}-byte read cadence lost bytes"
            );
        }

        // A mid-stream error must propagate, not read as end of stream.
        let mut served = 0_usize;
        let result = ArchiveSnapshot::open_expanded(&ctx, entry, |chunk| {
            if served >= 4096 {
                return Err(CodecError::Malformed(
                    "planted mid-stream read error".into(),
                ));
            }
            let n = 4096.min(chunk.len());
            chunk[..n].copy_from_slice(&plain[served..served + n]);
            served += n;
            Ok(n)
        });
        assert!(
            matches!(&result, Err(CodecError::Malformed(message)) if message == "planted mid-stream read error"),
            "a mid-stream read error was swallowed: {result:?}"
        );
    }

    /// A wrong CRC-32 in the central directory is refused, on every method.
    ///
    /// The phase's argument for why a decoder disagreement cannot become wrong output rests on
    /// two guards: `ExpandSpec::Exact` and this CRC.  `Exact` is held by
    /// `a_zstandard_entry_whose_declared_size_disagrees_is_refused`; this one was held by
    /// nothing, and disabling it left all 34 tests green.  It is genuinely redundant against
    /// everything the suite can currently produce -- with both guards disabled the full
    /// 103 275-mutation sweep still reports zero wrong outputs -- which is a reason to keep the
    /// guard and write the case, not to delete either.
    #[test]
    fn a_wrong_crc32_is_refused_on_every_method() {
        let plain = zip_fixture::zstd_golden_plaintext();
        let plain = plain.as_slice();
        for (label, mut entry) in [
            ("stored", RawEntry::stored("payload.bin", plain)),
            (
                "deflate",
                RawEntry::compressed("payload.bin", zip_fixture::DEFLATE, &deflate(plain), plain),
            ),
            (
                "zstd",
                RawEntry::compressed(
                    "payload.bin",
                    zip_fixture::ZSTD,
                    zip_fixture::ZSTD_GOLDEN_FRAME,
                    plain,
                ),
            ),
        ] {
            // Control first: the untouched entry opens and returns the payload.
            let bytes = zip_fixture::archive(std::slice::from_ref(&entry));
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("archive fits root policy");
            let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
            let record = snapshot.entry("payload.bin").expect("record");
            assert_eq!(
                snapshot
                    .open(&ctx, &record.name)
                    .expect("control opens")
                    .window(),
                plain,
                "{label}: the control did not open"
            );

            entry.crc32 ^= 0x0000_0001;
            let bytes = zip_fixture::archive(std::slice::from_ref(&entry));
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("archive fits root policy");
            let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
            let record = snapshot.entry("payload.bin").expect("record");
            assert!(
                snapshot.open(&ctx, &record.name).is_err(),
                "{label}: a wrong CRC-32 was accepted"
            );
        }
    }

    /// The Zstandard window cap is the decode policy's per-expansion allowance, checked on
    /// BOTH sides, and its refusal leaves the session open.
    ///
    /// `cadmpeg_core`'s `open_zstd` admits a frame whose buffered output -- its window, or its
    /// declared content size when that is smaller -- fits the per-expansion allowance. For an
    /// archive this small the allowance is its 16 MiB base, so windowLog 24 decodes and one
    /// mantissa step past it is refused as `NotImplemented`, with no session refusal.
    ///
    /// The frames are built here rather than committed, and are deliberately minimal: a header
    /// declaring a window and one raw block. That makes this a boundary test of the ceiling and
    /// NOT an interop test -- the frames come from this file, not from a producer.
    #[test]
    fn the_zstandard_window_cap_is_the_policys_and_bites_on_both_sides() {
        /// A frame declaring `window_log` and `mantissa`, carrying one raw block of `payload`.
        fn frame_declaring(window_log: u8, mantissa: u8, payload: &[u8]) -> Vec<u8> {
            let mut out = vec![0x28, 0xb5, 0x2f, 0xfd];
            out.push(0x00); // no Frame_Content_Size, not single-segment, no checksum
            out.push((window_log - 10) << 3 | mantissa);
            let size = u32::try_from(payload.len()).expect("fixture block fits");
            let header = size << 3 | 1; // last block, Raw_Block
            out.extend_from_slice(&header.to_le_bytes()[..3]);
            out.extend_from_slice(payload);
            out
        }

        let payload = b"abcd";
        let desktop = DecodePolicy::default();
        let service = DecodePolicy::service();
        for (policy, window_log, mantissa, accepted) in [
            (&desktop, 23_u8, 0_u8, true),
            (&desktop, 24, 0, true),
            (&desktop, 24, 1, false),
            (&desktop, 27, 0, false),
            (&service, 24, 0, true),
            (&service, 24, 1, false),
            (&service, 30, 0, false),
        ] {
            let frame = frame_declaring(window_log, mantissa, payload);
            let bytes = zip_fixture::archive(&[RawEntry::compressed(
                "payload.bin",
                zip_fixture::ZSTD,
                &frame,
                payload,
            )]);
            let arena = DecodeArena::new();
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, policy)
                .expect("archive fits root policy");
            let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
            let got = snapshot
                .open(&ctx, "payload.bin")
                .map(|view| view.window().to_vec());
            if accepted {
                assert_eq!(
                    got.as_deref().ok(),
                    Some(payload.as_slice()),
                    "windowLog {window_log} mantissa {mantissa}: got {got:?}"
                );
            } else {
                assert!(
                    matches!(&got, Err(CodecError::NotImplemented(message))
                        if message.ends_with("per-expansion allowance")),
                    "windowLog {window_log} mantissa {mantissa}: got {got:?}"
                );
                assert_eq!(ctx.resource_refusal(), None);
            }
        }
    }

    /// A frame carrying NO content checksum decodes, and its guards are the remaining ones.
    ///
    /// The decoder's checksum comparison is skipped whenever the frame declares no checksum --
    /// legal, and the case the Zstandard CLI accepts too.  It is also the case this container
    /// will actually meet: libzstd defaults `ZSTD_c_checksumFlag` to 0 and `zip`'s own zstd
    /// writer never sets it, so every method-93 entry that writer produced is checksum-free.
    /// The committed golden is the unusual one. Without this fixture, making an absent checksum
    /// REFUSE left all tests green.
    #[test]
    fn a_frame_without_a_content_checksum_decodes_and_is_still_guarded() {
        let plain = zip_fixture::zstd_golden_plaintext();
        assert_eq!(
            open_one_zstd_entry(zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME, &plain)
                .expect("a checksum-free frame must decode"),
            plain
        );

        // With the checksum gone, the CRC-32 and the decoder's framing of the whole entry are
        // what is left. Both are asserted here rather than assumed, because the argument that a
        // decoder disagreement cannot become wrong output rests on exactly them.
        let mut frame = zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME.to_vec();
        frame.extend_from_slice(b"tail");
        assert!(
            open_one_zstd_entry(&frame, &plain).is_err(),
            "trailing bytes were accepted on a checksum-free frame"
        );
        let bytes = zip_fixture::archive(&[{
            let mut e = RawEntry::compressed(
                "payload.bin",
                zip_fixture::ZSTD,
                zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME,
                &plain,
            );
            e.crc32 ^= 1;
            e
        }]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let record = snapshot.entry("payload.bin").expect("record");
        assert!(
            snapshot.open(&ctx, &record.name).is_err(),
            "a wrong CRC-32 was accepted on a checksum-free frame"
        );
    }

    /// A declared-encrypted entry is refused, whether it says so with the flag or with AE-x.
    ///
    /// `zip` rewrites an AE-x entry's method 99 to the method its 0x9901 extra field names, so
    /// a reader that asked `zip` for the method would see an AE-x entry with the encrypted bit
    /// CLEAR as ordinary Stored and decode it. The snapshot reads the central record itself
    /// (`entry::read`) and never asks `zip` for a method, so the rewrite cannot reach it:
    /// method 99 stays 99, which no arm of `open` decodes. Both shapes are refused: the archive
    /// is admitted and the member is refused at `open` as `NotImplemented`.
    #[test]
    fn a_declared_encrypted_entry_is_refused() {
        let plain = b"ciphertext";
        for (label, entry, encrypted, compression) in [
            (
                "general-purpose bit 0",
                RawEntry::stored("payload.bin", plain).with_flags(1),
                true,
                ZipCompression::Stored,
            ),
            (
                "AE-x with the encrypted bit clear",
                RawEntry::stored("payload.bin", plain)
                    .aes_without_the_encrypted_bit(zip_fixture::STORED),
                false,
                ZipCompression::Unsupported(99),
            ),
            (
                "AE-x with the encrypted bit set",
                RawEntry::stored("payload.bin", plain)
                    .aes_without_the_encrypted_bit(zip_fixture::STORED)
                    .with_flags(1),
                true,
                ZipCompression::Unsupported(99),
            ),
        ] {
            // Beside an ordinary member, which must stay readable.
            let bytes = zip_fixture::archive(&[entry, RawEntry::stored("plain.bin", plain)]);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("archive fits root policy");
            let snapshot = ArchiveSnapshot::new(&ctx, root).expect("the archive is admitted");
            assert!(
                snapshot.recovery_reason().is_none(),
                "{label}: the directory was not read"
            );
            let record = snapshot.entry("payload.bin").expect("record");
            assert_eq!(record.encrypted, encrypted, "{label}: encrypted flag");
            assert_eq!(record.compression, compression, "{label}: method");
            let opened = snapshot.open(&ctx, "payload.bin");
            assert!(
                matches!(opened, Err(CodecError::NotImplemented(_))),
                "{label}: an encrypted entry was opened: {opened:?}"
            );
            assert_eq!(
                snapshot
                    .open(&ctx, "plain.bin")
                    .expect("the ordinary member opens")
                    .window(),
                plain
            );
        }

        // The control: the same entry without either marking is opened, so the refusals above
        // are about the marking and not about the fixture.
        let bytes = zip_fixture::archive(&[RawEntry::stored("payload.bin", plain)]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        assert_eq!(
            snapshot
                .open(&ctx, "payload.bin")
                .expect("the control opens")
                .window(),
            plain
        );
    }

    /// A Zstandard frame written by the reference implementation decodes through `ruzstd`.
    ///
    /// Zstandard members are rare in real containers, so ordinary inputs seldom reach this
    /// arm; this fixture is what holds it in the unit suite.
    /// The frame comes from the Zstandard CLI and the expected bytes are generated, so the two
    /// sides of the comparison do not share a producer -- unlike a round trip through this
    /// crate's own decoder, which cannot see the two implementations disagreeing.
    #[test]
    fn a_reference_written_zstandard_entry_decodes() {
        let plain = zip_fixture::zstd_golden_plaintext();
        let bytes = zip_fixture::archive(&[RawEntry::compressed(
            "payload.bin",
            zip_fixture::ZSTD,
            zip_fixture::ZSTD_GOLDEN_FRAME,
            &plain,
        )]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let entry = snapshot.entry("payload.bin").expect("Zstandard record");
        assert_eq!(entry.compression, ZipCompression::Zstd);
        assert_eq!(entry.compression.label(), "zstd");
        assert_eq!(entry.compressed_size, 405);
        assert_eq!(entry.uncompressed_size, 19_968);
        let view = snapshot
            .open(&ctx, &entry.name)
            .expect("Zstandard entry opens");
        assert_eq!(view.window(), plain.as_slice());
    }

    /// Every byte of the fixture's every accepted position, measured by the exhaustive sweep
    /// (all 255 deltas at all 405 bytes) that `ZSTD_FULL_SWEEP=1` re-runs.  Read as a
    /// CEILING, not an equality: see the assertion below.
    ///
    /// Measured 2026-10-12 (103 275 mutations, 100 s, no wrong output).  Bytes 5 and 6 are not
    /// in the set: they are the frame's `Frame_Content_Size`, which the decoder compares with
    /// what the frame regenerated.
    const ZSTD_GOLDEN_ACCEPTING_BYTES: &[usize] = &[4, 18, 19, 285, 290, 294, 301, 302, 334];

    /// No corruption of the golden frame yields WRONG output, and every corruption that is
    /// accepted at all yields the correct bytes.
    ///
    /// A decoder with no negative case is a gate whose passing form is no output: the positive
    /// test above passes just as well against a decoder that returns the expected bytes for
    /// anything.  Three things about the shape of this test, each of which was got wrong first:
    ///
    /// **What is asserted is the property, not the reading.**  An earlier version pinned
    /// `accepted == [5, 6, 285, 334]`, which goes RED when `ruzstd` gets *stricter* -- i.e. on
    /// an improvement -- and a gate that is red for a fix is a gate that gets deleted.  The
    /// assertions are therefore: nothing decodes to the wrong bytes; the accepting positions are
    /// a SUBSET of the measured set above; and the checksum trailer is refused outright, which
    /// is the row this crate's own checksum comparison holds and the one a plant confirms.
    /// There is no `assert_eq!(refused, 401)`: every index lands in exactly one bucket, so the
    /// count is implied by the one above it.
    ///
    /// **The sampling is stated.**  `frame[i] ^= 1` is one bit of eight and 405 of the 103 275
    /// possible single-byte mutations -- 0.39 %, and widening it changes the answer.  The
    /// default here sweeps all 255 deltas over the frame header and the trailer, where the
    /// structural behaviour lives, and all eight single-bit flips over every byte, so no byte
    /// is unswept: 9 168 mutations, ~10 s (eight accepting positions).  The exhaustive 103 275
    /// takes 100 s in a debug build (nine), which is a test someone eventually marks
    /// `#[ignore]`, so it is opt-in -- and the numbers recorded above come from it.
    ///
    /// **The interesting bytes are 4, 5, 6 and 401-404**, all inside the exhaustive region:
    ///
    /// | byte(s) | what it is | this decoder | Zstandard CLI v1.5.7 |
    /// |---|---|---|---|
    /// | 4 | frame descriptor | some deltas accepted, output correct | varies |
    /// | 5, 6 | `Frame_Content_Size` | refused | refused |
    /// | 401-404 | content checksum | refused | refused |
    ///
    /// `ruzstd` compares neither `Frame_Content_Size` nor the content checksum with what it
    /// produced.  Both comparisons are made by `cadmpeg_core`'s `open_zstd` decoder, which reads
    /// the frame header itself and so can tell an absent size field from one that says 0 --
    /// which `ruzstd`'s accessor cannot.  `ExpandSpec::Exact`, fed the central directory's
    /// size, refuses independently of both.
    /// Bytes 401-404 are refused only because that decoder compares the checksum itself.  Delete
    /// the comparison and the assertion that fires is the SUBSET one -- `these bytes now accept
    /// a corruption and did not before: [401, 402, 403, 404]` -- not `checksum_bytes_accepted`,
    /// which cannot speak while the ceiling excludes those bytes.
    #[test]
    fn no_corruption_of_the_zstandard_frame_yields_wrong_output() {
        let plain = zip_fixture::zstd_golden_plaintext();
        let len = zip_fixture::ZSTD_GOLDEN_FRAME.len();
        // `is_some()` on the OsString would make `ZSTD_FULL_SWEEP=0` and `=` both run the 76 s
        // sweep, which is how a run gets labelled "default" and silently isn't.
        let full = matches!(
            std::env::var("ZSTD_FULL_SWEEP").as_deref(),
            Ok("1" | "true" | "yes")
        );
        let exhaustive = |i: usize| full || i < 12 || i + 12 >= len;
        let deltas = |i: usize| -> Vec<u8> {
            if exhaustive(i) {
                (1..=255_u8).collect()
            } else {
                (0..8).map(|b| 1_u8 << b).collect()
            }
        };

        let mut accepting = std::collections::BTreeSet::new();
        let mut checksum_bytes_accepted = Vec::new();
        let mut swept_bytes = std::collections::BTreeSet::new();
        let mut swept = 0_usize;
        for index in 0..len {
            for delta in deltas(index) {
                swept += 1;
                swept_bytes.insert(index);
                let mut frame = zip_fixture::ZSTD_GOLDEN_FRAME.to_vec();
                frame[index] ^= delta;
                match open_one_zstd_entry(&frame, &plain) {
                    Ok(bytes) if bytes == plain => {
                        accepting.insert(index);
                        if (401..=404).contains(&index) {
                            checksum_bytes_accepted.push((index, delta));
                        }
                    }
                    Ok(bytes) => panic!(
                        "byte {index} ^ {delta:#04x} produced {} bytes of WRONG output",
                        bytes.len()
                    ),
                    // `Truncated` is the bounded reader's own refusal: a corrupted block size
                    // can end the frame early and leave fewer than four bytes where the next
                    // frame's magic would be.
                    Err(CodecError::Malformed(_) | CodecError::Truncated { .. }) => {}
                    // Flipping the frame descriptor can turn the next byte into a window
                    // descriptor, and a window above the per-expansion allowance is refused as
                    // unsupported rather than as malformed input.
                    Err(CodecError::NotImplemented(message))
                        if message.starts_with("Zstandard window") => {}
                    Err(other) => panic!("byte {index} ^ {delta:#04x} gave {other:?}"),
                }
            }
        }
        // The floor is the byte SET, not the total.  A total-only floor is satisfied by a sweep
        // that skips a hundred contiguous bytes -- measured: giving bytes 100..200 no mutations
        // at all drops the count 9 168 -> 8 368, over any threshold loose enough not to be
        // tuned, and the docstring's "no byte is unswept" was held by nothing.
        assert_eq!(
            swept_bytes.len(),
            len,
            "the sweep reached {} of {len} bytes",
            swept_bytes.len()
        );
        assert!(swept >= 9_000, "the sweep collapsed to {swept} mutations");

        // The measured set is a CEILING: a stricter decoder accepts fewer and stays green.
        let ceiling: std::collections::BTreeSet<usize> =
            ZSTD_GOLDEN_ACCEPTING_BYTES.iter().copied().collect();
        let surprises: Vec<_> = accepting.difference(&ceiling).copied().collect();
        assert!(
            surprises.is_empty(),
            "these bytes now accept a corruption and did not before: {surprises:?}"
        );

        // The row this crate's own checksum comparison holds.  Its region is always exhaustive.
        assert!(
            checksum_bytes_accepted.is_empty(),
            "the content-checksum trailer accepted {checksum_bytes_accepted:?}"
        );

        // Truncation, which no single-byte mutation reaches.
        for keep in [0_usize, 3, 4, 8, 200, 404] {
            assert!(
                open_one_zstd_entry(&zip_fixture::ZSTD_GOLDEN_FRAME[..keep], &plain).is_err(),
                "a frame truncated to {keep} bytes was accepted"
            );
        }

        // Trailing bytes after a complete frame.  `ruzstd` stops at the end of the first frame
        // and ignores what follows, so this is held by the decoder's own framing of the whole
        // entry: bytes that are not a frame are refused before anything is decoded.  A second
        // complete frame IS a frame, is decoded (every frame of an entry is decoded, as libzstd does), and
        // is refused here by `ExpandSpec::Exact`, because the entry declares one plaintext.
        for tail in [
            b"".as_slice(),
            b"\x00",
            b"junk",
            zip_fixture::ZSTD_GOLDEN_FRAME,
        ] {
            let mut frame = zip_fixture::ZSTD_GOLDEN_FRAME.to_vec();
            frame.extend_from_slice(tail);
            let result = open_one_zstd_entry(&frame, &plain);
            if tail.is_empty() {
                assert_eq!(
                    result.expect("the untailed frame opens"),
                    plain,
                    "control: the frame with an EMPTY tail must still decode"
                );
            } else {
                assert!(
                    result.is_err(),
                    "a frame with {} trailing bytes was accepted",
                    tail.len()
                );
            }
        }

        // The control, last: the unmutated frame through the same code is still accepted.
        // Without it a decoder that refused everything would pass every assertion above.
        assert_eq!(
            open_one_zstd_entry(zip_fixture::ZSTD_GOLDEN_FRAME, &plain)
                .expect("the unmutated frame opens"),
            plain
        );
    }

    /// Re-parses a ZIP the way a streaming reader would and requires it to agree with itself.
    ///
    /// Independent of `zip` on purpose: the point is to hold the fields `zip::ZipArchive` does
    /// not read on the snapshot's path (every local header) or does not validate (the EOCD's
    /// central-directory size and total entry count).
    fn assert_local_headers_agree(bytes: &[u8]) {
        let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let len16 = |i: usize| usize::from(u16_at(i));
        let len32 = |i: usize| usize::try_from(u32_at(i)).expect("offset fits memory");
        let eocd = (0..bytes.len().saturating_sub(21))
            .rev()
            .find(|&i| &bytes[i..i + 4] == b"PK\x05\x06")
            .expect("end-of-central-directory record");
        let count = len16(eocd + 10);
        assert_eq!(len16(eocd + 8), count, "disk vs total entry count");
        let central_size = len32(eocd + 12);
        let central_start = len32(eocd + 16);
        assert_eq!(
            central_start + central_size,
            eocd,
            "the central directory does not end where the EOCD begins"
        );

        let mut at = central_start;
        for index in 0..count {
            assert_eq!(&bytes[at..at + 4], b"PK\x01\x02", "central header {index}");
            let (flags, method) = (u16_at(at + 8), u16_at(at + 10));
            let (crc, csize, usize_) = (u32_at(at + 16), u32_at(at + 20), u32_at(at + 24));
            let (nlen, elen, clen) = (len16(at + 28), len16(at + 30), len16(at + 32));
            let name = &bytes[at + 46..at + 46 + nlen];
            let local = len32(at + 42);

            assert_eq!(
                &bytes[local..local + 4],
                b"PK\x03\x04",
                "local header {index}"
            );
            assert_eq!(u16_at(local + 6), flags, "{index}: flags disagree");
            assert_eq!(u16_at(local + 8), method, "{index}: method disagrees");
            assert_eq!(u32_at(local + 14), crc, "{index}: CRC-32 disagrees");
            assert_eq!(
                u32_at(local + 18),
                csize,
                "{index}: compressed size disagrees"
            );
            assert_eq!(
                u32_at(local + 22),
                usize_,
                "{index}: uncompressed size disagrees"
            );
            let lnlen = len16(local + 26);
            assert_eq!(
                &bytes[local + 30..local + 30 + lnlen],
                name,
                "{index}: name disagrees"
            );
            at += 46 + nlen + elen + clen;
        }
        assert_eq!(
            at, eocd,
            "the central directory has {count} entries but does not end at EOCD"
        );
    }

    /// Wraps `frame` as the sole Zstandard entry of a ZIP declaring `plain`, and opens it.
    fn open_one_zstd_entry(frame: &[u8], plain: &[u8]) -> Result<Vec<u8>, CodecError> {
        open_one_zstd_entry_with(frame, plain, &DecodePolicy::default())
    }

    fn open_one_zstd_entry_with(
        frame: &[u8],
        plain: &[u8],
        policy: &DecodePolicy,
    ) -> Result<Vec<u8>, CodecError> {
        let bytes = zip_fixture::archive(&[RawEntry::compressed(
            "payload.bin",
            zip_fixture::ZSTD,
            frame,
            plain,
        )]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, policy)
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        let entry = snapshot.entry("payload.bin").expect("Zstandard record");
        snapshot
            .open(&ctx, &entry.name)
            .map(|view| view.window().to_vec())
    }

    /// An entry of several Zstandard frames decodes to their concatenation, and skippable
    /// frames contribute nothing.
    ///
    /// This is libzstd's behaviour (its streaming decoder continues into the next
    /// frame), kept with `ruzstd`, whose `FrameDecoder` stops at the end of the first frame.
    /// The Zstandard CLI v1.5.5 decodes the same layouts.
    #[test]
    fn a_multi_frame_zstandard_entry_decodes() {
        let plain = zip_fixture::zstd_golden_plaintext();
        let skippable = [&[0x50, 0x2a, 0x4d, 0x18, 5, 0, 0, 0][..], b"notes"].concat();
        let frames = [
            skippable.as_slice(),
            zip_fixture::ZSTD_GOLDEN_FRAME,
            skippable.as_slice(),
            zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME,
            &zip_fixture::zstd_stored_frame(b"tail"),
            skippable.as_slice(),
        ]
        .concat();
        let expected = [plain.as_slice(), plain.as_slice(), b"tail"].concat();
        assert_eq!(
            open_one_zstd_entry(&frames, &expected).expect("every frame decodes"),
            expected
        );

        // The second frame's own checksum still counts: one frame's verdict does not cover
        // another's.
        let mut corrupt = [
            zip_fixture::ZSTD_GOLDEN_NOCHECK_FRAME,
            zip_fixture::ZSTD_GOLDEN_FRAME,
        ]
        .concat();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        let doubled = [plain.as_slice(), plain.as_slice()].concat();
        assert!(matches!(
            open_one_zstd_entry(&corrupt, &doubled),
            Err(CodecError::Malformed(message)) if message.contains("checksum")
        ));
        corrupt[last] ^= 1;
        assert_eq!(
            open_one_zstd_entry(&corrupt, &doubled).expect("control"),
            doubled
        );
    }

    /// A payload whose declared uncompressed size disagrees with the frame is refused.
    ///
    /// `ExpandSpec::Exact` is the guard that does not depend on the decoder at all: whatever a
    /// Zstandard decoder makes of the entry, the bytes it yields must be exactly as many as the
    /// central directory declares.  The two directions are separate failures and both are
    /// asserted.
    #[test]
    fn a_zstandard_entry_whose_declared_size_disagrees_is_refused() {
        let plain = zip_fixture::zstd_golden_plaintext();
        for declared in [plain.len() - 1, plain.len() + 1] {
            let mut entry = RawEntry::compressed(
                "payload.bin",
                zip_fixture::ZSTD,
                zip_fixture::ZSTD_GOLDEN_FRAME,
                &plain,
            );
            entry.uncompressed_size = u32::try_from(declared).expect("fits u32");
            let bytes = zip_fixture::archive(&[entry]);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("archive fits root policy");
            let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
            let entry = snapshot.entry("payload.bin").expect("Zstandard record");
            assert!(
                snapshot.open(&ctx, &entry.name).is_err(),
                "a declared size of {declared} was accepted against a 19 968-byte frame"
            );
        }
    }

    /// A compression method this crate does not implement is still named, not mistaken for one.
    ///
    /// The snapshot reads the method number from the central record itself and carries it as
    /// `ZipCompression::Unsupported(n)`: the archive is admitted, the member is summarized
    /// with its number, and `open` refuses it as `NotImplemented`, while the other members
    /// stay readable.
    ///
    /// Method 20 is NOT in this list any more. It is the Zstandard method ID that APPNOTE
    /// 6.3.7 deprecated in favour of 93, and this crate decodes both; the second half holds
    /// that with the reference-written frame.
    #[test]
    fn an_unimplemented_compression_method_is_refused_by_number() {
        for method in [1_u16, 12, 14, 94, 95, 98] {
            // Distinct compressed and uncompressed sizes: with `b"whatever"` for both, this
            // fixture was the one archive test blind to a builder that transposes them.
            let bytes = zip_fixture::archive(&[
                RawEntry::compressed(
                    "payload.bin",
                    method,
                    b"compressed",
                    b"the uncompressed form, which is longer",
                ),
                RawEntry::stored("plain.bin", b"plain"),
            ]);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("archive fits root policy");
            let snapshot = ArchiveSnapshot::new(&ctx, root).expect("the archive is admitted");
            assert!(snapshot.recovery_reason().is_none());
            assert_eq!(
                snapshot.entry("payload.bin").expect("record").compression,
                ZipCompression::Unsupported(method)
            );
            let opened = snapshot.open(&ctx, "payload.bin");
            assert!(
                matches!(&opened, Err(CodecError::NotImplemented(message))
                    if message.contains(&format!("ZIP compression {method} "))),
                "ZIP compression method {method} was not refused as unimplemented: {opened:?}"
            );
            assert_eq!(
                snapshot.open(&ctx, "plain.bin").expect("sibling").window(),
                b"plain"
            );
        }

        let plain = zip_fixture::zstd_golden_plaintext();
        let bytes = zip_fixture::archive(&[RawEntry::compressed(
            "payload.bin",
            20,
            zip_fixture::ZSTD_GOLDEN_FRAME,
            &plain,
        )]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(&ctx, root).expect("archive snapshots");
        assert_eq!(
            snapshot.entry("payload.bin").expect("record").compression,
            ZipCompression::Zstd
        );
        assert_eq!(
            snapshot.open(&ctx, "payload.bin").expect("opens").window(),
            plain.as_slice()
        );
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
