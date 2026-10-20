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
mod tests;
