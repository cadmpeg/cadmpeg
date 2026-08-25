// SPDX-License-Identifier: Apache-2.0
//! Single-pass ZIP metadata snapshots with budgeted entry opening.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use cadmpeg_core::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use zip::CompressionMethod;

static NEXT_ARCHIVE_SNAPSHOT_ID: AtomicU64 = AtomicU64::new(1);

/// Compression methods supported by [`ArchiveSnapshot::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCompression {
    /// The entry payload is stored directly in the archive.
    Stored,
    /// The entry payload is a raw-DEFLATE member.
    Deflate,
    /// The entry payload is a Zstandard frame.
    Zstd,
}

impl EntryCompression {
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
}

/// ZIP central-directory facts retained after the parser is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRecord {
    /// Entry name as stored in the central directory.
    pub name: String,
    /// Compression method admitted by the snapshot.
    pub compression: EntryCompression,
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
    snapshot_id: u64,
}

impl EntryRecord {
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
    snapshot_id: u64,
    entries: Vec<EntryRecord>,
    by_name: BTreeMap<String, usize>,
}

impl<'a> ArchiveSnapshot<'a> {
    /// Parses the central directory once and retains replayable physical facts.
    pub fn new(root: View<'a>) -> Result<Self, CodecError> {
        let mut archive = zip::ZipArchive::new(Cursor::new(root.window()))
            .map_err(|error| CodecError::malformed(format_args!("not a readable ZIP: {error}")))?;
        let snapshot_id = NEXT_ARCHIVE_SNAPSHOT_ID.fetch_add(1, AtomicOrdering::Relaxed);
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
            let compression = EntryCompression::from_zip(file.compression(), &name)?;
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
                snapshot_id,
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
            snapshot_id,
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

    /// Opens one entry as a borrowed stored slice or budgeted expanded view.
    pub fn open(
        &self,
        ctx: &DecodeContext<'a>,
        entry: &EntryRecord,
    ) -> Result<View<'a>, CodecError> {
        if entry.snapshot_id != self.snapshot_id {
            return Err(CodecError::Malformed(
                "ZIP entry handle does not belong to this snapshot".into(),
            ));
        }
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
            EntryCompression::Stored => {
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
            EntryCompression::Deflate | EntryCompression::Zstd => {
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
                    EntryCompression::Deflate => {
                        Box::new(flate2::read::DeflateDecoder::new(source.window()))
                    }
                    EntryCompression::Zstd => Box::new(
                        zstd::stream::read::Decoder::with_buffer(source.window()).map_err(
                            |error| {
                                CodecError::malformed(format_args!(
                                    "cannot open Zstandard frame for {}: {error}",
                                    entry.name
                                ))
                            },
                        )?,
                    ),
                    EntryCompression::Stored => unreachable!("stored entries use borrowed views"),
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
        classify: impl Fn(&str) -> &'static str,
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
                    role: classify(&entry.name).into(),
                    compression: entry.compression.label().into(),
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

/// One exact physical range in a ZIP archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalSpan {
    /// Inclusive byte offset.
    pub start: u64,
    /// Exclusive byte offset.
    pub end: u64,
    /// ZIP structural role.
    pub role: String,
    /// Owning entry name, when applicable.
    pub entry: Option<String>,
}

#[derive(Debug)]
struct Region {
    start: u64,
    end: u64,
    role: &'static str,
    entry: Option<String>,
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
    regions: &mut Vec<Region>,
    start: u64,
    end: u64,
    role: &'static str,
    entry: Option<&str>,
) {
    if start < end {
        regions.push(Region {
            start,
            end,
            role,
            entry: entry.map(str::to_owned),
        });
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
            "local-signature",
            Some(&entry.name),
        );
        push_region(
            &mut regions,
            entry.header_start + 4,
            fixed_end,
            "local-fields",
            Some(&entry.name),
        );
        push_region(
            &mut regions,
            fixed_end,
            name_end,
            "local-name",
            Some(&entry.name),
        );
        push_region(
            &mut regions,
            name_end,
            extra_end,
            "local-extra",
            Some(&entry.name),
        );
        push_region(
            &mut regions,
            entry.data_start,
            entry.data_end()?,
            "compressed-payload",
            Some(&entry.name),
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
                    "data-descriptor",
                    Some(&entry.name),
                );
                push_region(
                    &mut regions,
                    descriptor_end,
                    next,
                    "archive-padding",
                    Some(&entry.name),
                );
            } else {
                push_region(
                    &mut regions,
                    entry.data_end()?,
                    next,
                    "archive-padding",
                    Some(&entry.name),
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
            "central-signature",
            Some(&entry.name),
        );
        push_region(
            &mut regions,
            entry.central_start + 4,
            fixed_end,
            "central-fields",
            Some(&entry.name),
        );
        push_region(
            &mut regions,
            fixed_end,
            name_end,
            "central-name",
            Some(&entry.name),
        );
        push_region(
            &mut regions,
            name_end,
            extra_end,
            "central-extra",
            Some(&entry.name),
        );
        push_region(
            &mut regions,
            extra_end,
            record_end,
            "central-comment",
            Some(&entry.name),
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
                    "zip64-end-record",
                    12_u64
                        .checked_add(body)
                        .ok_or_else(|| CodecError::Malformed("ZIP64 end size overflow".into()))?,
                )
            }
            Some(signature) if signature == *b"PK\x06\x07" => ("zip64-end-locator", 20),
            Some(signature) if signature == *b"PK\x05\x06" => {
                let comment = u64::from(u16_at(bytes, offset + 20)?);
                ("end-record", 22_u64 + comment)
            }
            _ => ("archive-padding", len - offset),
        };
        let end = offset
            .checked_add(size)
            .ok_or_else(|| CodecError::Malformed("ZIP end-record range overflow".into()))?;
        if end > len {
            return Err(CodecError::malformed(format_args!("truncated {role}")));
        }
        push_region(regions, offset, end, role, None);
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
    let spans = points
        .windows(2)
        .filter_map(|pair| {
            let (start, end) = (pair[0], pair[1]);
            (start < end).then(|| {
                while ordered_regions
                    .get(region_index)
                    .is_some_and(|region| region.end <= start)
                {
                    region_index += 1;
                }
                let owner = ordered_regions
                    .get(region_index)
                    .copied()
                    .filter(|region| region.start <= start && end <= region.end);
                let (role, entry) = owner.map_or(("unclassified", None), |region| {
                    (region.role, region.entry.clone())
                });
                PhysicalSpan {
                    start,
                    end: end.min(len),
                    role: role.into(),
                    entry,
                }
            })
        })
        .collect::<Vec<_>>();
    if spans.iter().any(|span| span.role == "unclassified") {
        return Err(CodecError::Malformed(
            "physical ZIP ledger contains an unclassified byte range".into(),
        ));
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
            snapshot.open(&ctx, stored).expect("stored opens").window(),
            b"stored"
        );
        assert_eq!(
            snapshot
                .open(&ctx, deflated)
                .expect("deflated opens")
                .window(),
            b"deflated payload"
        );
        assert_eq!(
            snapshot
                .open(&ctx, zstd)
                .expect("Zstandard entry opens")
                .window(),
            b"Zstandard payload"
        );
    }

    #[test]
    fn snapshot_rejects_entry_handles_from_another_snapshot() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let first = ArchiveSnapshot::new(root).expect("first archive snapshot");
        let second = ArchiveSnapshot::new(root).expect("second archive snapshot");
        let foreign = second.entry("stored.bin").expect("foreign entry exists");
        assert!(first.open(&ctx, foreign).is_err());

        let owned = first
            .entry("stored.bin")
            .expect("owned entry exists")
            .clone();
        assert_eq!(
            first
                .open(&ctx, &owned)
                .expect("owned clone opens")
                .window(),
            b"stored"
        );
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
                    .open(&ctx, entry)
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
        let view = archive.open(&ctx, entry).expect("GuiDocument member opens");
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
