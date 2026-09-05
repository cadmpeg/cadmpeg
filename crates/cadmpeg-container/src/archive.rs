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

    const fn summary(self) -> cadmpeg_core::container::EntryCompression {
        use cadmpeg_core::container::EntryCompression;
        match self {
            Self::Stored => EntryCompression::Stored,
            Self::Deflate => EntryCompression::Deflate,
            Self::Zstd => EntryCompression::Zstd,
        }
    }

    /// Returns the stable container-summary label.
    pub const fn label(self) -> &'static str {
        self.summary().as_str()
    }
}

impl From<ZipCompression> for cadmpeg_core::container::EntryCompression {
    fn from(value: ZipCompression) -> Self {
        value.summary()
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
    entries: Vec<EntryRecord>,
    by_name: BTreeMap<String, usize>,
}

impl<'a> ArchiveSnapshot<'a> {
    /// Parses the central directory once and retains replayable physical facts.
    pub fn new(root: View<'a>) -> Result<Self, CodecError> {
        let mut archive = zip::ZipArchive::new(Cursor::new(root.window()))
            .map_err(|error| CodecError::malformed(format_args!("not a readable ZIP: {error}")))?;
        let central_entry_count =
            reject_duplicate_central_names(root.window(), archive.central_directory_start())?;
        if central_entry_count != archive.len() {
            return Err(CodecError::Malformed(
                "ZIP central directory contains duplicate entry names".into(),
            ));
        }
        let mut names = BTreeSet::new();
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let file = archive.by_index(index).map_err(|error| {
                CodecError::malformed(format_args!("bad ZIP entry {index}: {error}"))
            })?;
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
        let by_name = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.name.clone(), index))
            .collect();
        Ok(Self {
            root,
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
        match entry.compression {
            ZipCompression::Stored => {
                let view = ctx.register_slice_as(
                    self.root,
                    ByteRange {
                        start: absolute_start,
                        end: absolute_end,
                    },
                    entry.name.clone(),
                )?;
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
            ZipCompression::Deflate | ZipCompression::Zstd => {
                let start = usize::try_from(absolute_start).map_err(|_| {
                    CodecError::Malformed("ZIP data offset does not fit memory".into())
                })?;
                let end = usize::try_from(absolute_end).map_err(|_| {
                    CodecError::Malformed("ZIP data offset does not fit memory".into())
                })?;
                let source = self.root.child(start, end).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "ZIP data range escapes archive for {}",
                        entry.name
                    ))
                })?;
                let mut decoder: Box<dyn Read> = match entry.compression {
                    ZipCompression::Deflate => {
                        Box::new(flate2::read::DeflateDecoder::new(source.window()))
                    }
                    ZipCompression::Zstd => Box::new(
                        zstd::stream::read::Decoder::with_buffer(source.window()).map_err(
                            |error| {
                                CodecError::malformed(format_args!(
                                    "cannot open Zstandard frame for {}: {error}",
                                    entry.name
                                ))
                            },
                        )?,
                    ),
                    ZipCompression::Stored => unreachable!("stored entries use borrowed views"),
                };
                let mut writer = ctx.begin_expand_as(
                    source,
                    ExpandSpec::Exact(entry.uncompressed_size),
                    entry.name.clone(),
                )?;
                let mut chunk = [0_u8; 16 * 1024];
                loop {
                    let read = decoder.read(&mut chunk).map_err(|error| {
                        CodecError::malformed(format_args!(
                            "cannot inflate {}: {error}",
                            entry.name
                        ))
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
        }
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
                ContainerEntry {
                    name: entry.name.clone(),
                    role: classify(&entry.name),
                    compression: entry.compression.into(),
                    compressed_size: entry.compressed_size,
                    uncompressed_size: entry.uncompressed_size,
                    attributes,
                }
            })
            .collect()
    }

    /// Partitions every physical archive byte by ZIP structural role.
    pub fn physical_ledger(&self) -> Result<Vec<PhysicalSpan>, CodecError> {
        physical_ledger(self.root.window(), &self.entries)
    }
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
        if !names.insert(name.to_vec()) {
            return Err(CodecError::Malformed(
                "duplicate ZIP central entry name".into(),
            ));
        }
        offset = record_end;
    }
    Ok(entry_count)
}

/// The closed structural role of one physical container range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanRole {
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
    /// CFB file header.
    CfbHeader,
    /// CFB version-4 range-lock sector.
    CfbRangeLockSector,
    /// CFB file-allocation-table sector.
    CfbFat,
    /// CFB double-indirect file-allocation-table sector.
    CfbDifat,
    /// CFB directory sector.
    CfbDirectory,
    /// CFB mini-file-allocation-table sector.
    CfbMiniFat,
    /// CFB regular-sector payload for the named stream.
    CfbRegularStreamPayload(String),
    /// CFB allocation padding, optionally owned by a stream.
    CfbPadding {
        /// Owning entry, when the padding belongs to one.
        entry: Option<String>,
    },
    /// CFB mini-sector payload for the named stream.
    CfbMiniStreamPayload(String),
    /// Unallocated bytes inside the CFB root mini stream.
    CfbMiniStreamPadding,
    /// Unallocated CFB sector.
    CfbUnallocatedSector,
}

impl SpanRole {
    /// Returns the stable physical-ledger label.
    pub const fn label(&self) -> &'static str {
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
            Self::CfbHeader => "header",
            Self::CfbRangeLockSector => "range lock sector",
            Self::CfbFat => "FAT",
            Self::CfbDifat => "DIFAT",
            Self::CfbDirectory => "directory",
            Self::CfbMiniFat => "mini FAT",
            Self::CfbRegularStreamPayload(_) => "regular stream payload",
            Self::CfbPadding { .. } => "padding",
            Self::CfbMiniStreamPayload(_) => "mini stream payload",
            Self::CfbMiniStreamPadding => "mini-stream padding",
            Self::CfbUnallocatedSector => "unallocated sector",
        }
    }

    /// Returns the owning entry for an entry-owned range.
    pub fn entry(&self) -> Option<&str> {
        match self {
            Self::LocalSignature(entry)
            | Self::LocalFields(entry)
            | Self::LocalName(entry)
            | Self::LocalExtra(entry)
            | Self::CompressedPayload(entry)
            | Self::DataDescriptor(entry)
            | Self::CentralSignature(entry)
            | Self::CentralFields(entry)
            | Self::CentralName(entry)
            | Self::CentralExtra(entry)
            | Self::CentralComment(entry)
            | Self::CfbRegularStreamPayload(entry)
            | Self::CfbMiniStreamPayload(entry) => Some(entry),
            Self::Padding { entry } | Self::CfbPadding { entry } => entry.as_deref(),
            _ => None,
        }
    }
}

/// One exact physical range in an archive or compound file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalSpan {
    /// Inclusive byte offset.
    pub start: u64,
    /// Exclusive byte offset.
    pub end: u64,
    /// Structural role, including an owning entry where applicable.
    pub role: SpanRole,
}

#[derive(Debug)]
struct Region {
    start: u64,
    end: u64,
    role: SpanRole,
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

fn push_region(regions: &mut Vec<Region>, start: u64, end: u64, role: SpanRole) {
    if start < end {
        regions.push(Region { start, end, role });
    }
}

fn physical_ledger(bytes: &[u8], entries: &[EntryRecord]) -> Result<Vec<PhysicalSpan>, CodecError> {
    let len = bytes.len() as u64;
    let mut regions = Vec::new();
    let mut local_order = entries.iter().collect::<Vec<_>>();
    local_order.sort_by_key(|entry| entry.header_start);
    let central_begin = entries
        .iter()
        .map(|entry| entry.central_start)
        .min()
        .unwrap_or(len);

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
            SpanRole::LocalSignature(entry.name.clone()),
        );
        push_region(
            &mut regions,
            entry.header_start + 4,
            fixed_end,
            SpanRole::LocalFields(entry.name.clone()),
        );
        push_region(
            &mut regions,
            fixed_end,
            name_end,
            SpanRole::LocalName(entry.name.clone()),
        );
        push_region(
            &mut regions,
            name_end,
            extra_end,
            SpanRole::LocalExtra(entry.name.clone()),
        );
        push_region(
            &mut regions,
            entry.data_start,
            entry.data_end()?,
            SpanRole::CompressedPayload(entry.name.clone()),
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
                    SpanRole::DataDescriptor(entry.name.clone()),
                );
                push_region(
                    &mut regions,
                    descriptor_end,
                    next,
                    SpanRole::Padding {
                        entry: Some(entry.name.clone()),
                    },
                );
            } else {
                push_region(
                    &mut regions,
                    entry.data_end()?,
                    next,
                    SpanRole::Padding {
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
            SpanRole::CentralSignature(entry.name.clone()),
        );
        push_region(
            &mut regions,
            entry.central_start + 4,
            fixed_end,
            SpanRole::CentralFields(entry.name.clone()),
        );
        push_region(
            &mut regions,
            fixed_end,
            name_end,
            SpanRole::CentralName(entry.name.clone()),
        );
        push_region(
            &mut regions,
            name_end,
            extra_end,
            SpanRole::CentralExtra(entry.name.clone()),
        );
        push_region(
            &mut regions,
            extra_end,
            record_end,
            SpanRole::CentralComment(entry.name.clone()),
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
    let values_start = start + if has_signature { 4 } else { 0 };
    let local_zip64 = u32_at(bytes, entry.header_start + 18)? == u32::MAX
        || u32_at(bytes, entry.header_start + 22)? == u32::MAX;
    let widths = if local_zip64 { [8_u64, 4] } else { [4_u64, 8] };
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
    Err(CodecError::malformed(format_args!(
        "invalid data descriptor for {}",
        entry.name
    )))
}

fn classify_end_records(
    bytes: &[u8],
    mut offset: u64,
    len: u64,
    regions: &mut Vec<Region>,
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
                    SpanRole::Zip64EndRecord,
                    12_u64
                        .checked_add(body)
                        .ok_or_else(|| CodecError::Malformed("ZIP64 end size overflow".into()))?,
                )
            }
            Some(signature) if signature == *b"PK\x06\x07" => (SpanRole::Zip64EndLocator, 20),
            Some(signature) if signature == *b"PK\x05\x06" => {
                let comment = u64::from(u16_at(bytes, offset + 20)?);
                (SpanRole::EndRecord, 22_u64 + comment)
            }
            _ => (SpanRole::Padding { entry: None }, len - offset),
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

fn partition(len: u64, regions: &[Region]) -> Result<Vec<PhysicalSpan>, CodecError> {
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
        if start == end {
            continue;
        }
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
            end: end.min(len),
            role: owner.role.clone(),
        });
    }
    for pair in spans.windows(2) {
        if pair[0].end != pair[1].start {
            return Err(CodecError::Malformed(
                "physical ZIP ledger has a gap or overlap".into(),
            ));
        }
    }
    Ok(spans)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write as _};

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use zip::write::SimpleFileOptions;

    use super::*;

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
    fn snapshot_opens_supported_entries_after_parser_drop() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(root).expect("archive snapshots");
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
    fn snapshot_opens_names_using_its_own_metadata() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let first = ArchiveSnapshot::new(root).expect("first archive snapshot");
        let second = ArchiveSnapshot::new(root).expect("second archive snapshot");
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
        let snapshot = ArchiveSnapshot::new(nested).expect("nested archive snapshots");

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
    fn labeled_deflate_member_resolves_for_inspect() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(
                "GuiDocument.xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("GuiDocument entry starts");
        archive
            .write_all(b"<GuiDocument SchemaVersion=\"1\"/>")
            .expect("GuiDocument entry writes");
        let zip_bytes = archive.finish().expect("archive finishes").into_inner();

        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&zip_bytes, &arena, &DecodePolicy::default())
                .expect("archive fits root policy");
        let archive = ArchiveSnapshot::new(root).expect("archive snapshots");
        let entry = archive.entry("GuiDocument.xml").expect("member present");
        let view = archive
            .open(&ctx, &entry.name)
            .expect("GuiDocument member opens");
        let address = ctx.resolve_location(view.location_at(5));
        assert!(
            address.path().ends_with("GuiDocument.xml@5"),
            "path={}",
            address.path()
        );
        let commands = address.inspect_commands("part.FCStd");
        assert_eq!(
            commands[0],
            "cadmpeg inspect extract part.FCStd GuiDocument.xml -o part.FCStd.member"
        );
        assert_eq!(
            commands[1],
            "cadmpeg inspect hex part.FCStd.member --offset 5 --len 64"
        );
    }
}
