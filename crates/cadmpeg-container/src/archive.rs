// SPDX-License-Identifier: Apache-2.0
//! Single-pass ZIP metadata snapshots with budgeted entry opening.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use cadmpeg_codec_core::decode::{ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_codec_core::{CodecError, ContainerEntry};
use zip::CompressionMethod;

/// Compression methods supported by [`ArchiveSnapshot::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCompression {
    /// The entry payload is stored directly in the archive.
    Stored,
    /// The entry payload is a raw-DEFLATE member.
    Deflate,
}

impl EntryCompression {
    fn from_zip(method: CompressionMethod, name: &str) -> Result<Self, CodecError> {
        match method {
            CompressionMethod::Stored => Ok(Self::Stored),
            CompressionMethod::Deflated => Ok(Self::Deflate),
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
}

impl EntryRecord {
    /// Returns the exclusive compressed-payload boundary.
    pub fn data_end(&self) -> Result<u64, CodecError> {
        self.data_start
            .checked_add(self.compressed_size)
            .ok_or_else(|| {
                CodecError::Malformed(format!("ZIP data range overflows for {}", self.name))
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
            .map_err(|error| CodecError::Malformed(format!("not a readable ZIP: {error}")))?;
        let mut names = BTreeSet::new();
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let file = archive.by_index(index).map_err(|error| {
                CodecError::Malformed(format!("bad ZIP entry {index}: {error}"))
            })?;
            let name = file.name().to_owned();
            if !names.insert(name.clone()) {
                return Err(CodecError::Malformed(format!(
                    "duplicate ZIP entry name {name}"
                )));
            }
            if file.encrypted() {
                return Err(CodecError::Malformed(format!("encrypted ZIP entry {name}")));
            }
            let compression = EntryCompression::from_zip(file.compression(), &name)?;
            let data_start = file
                .data_start()
                .ok_or_else(|| CodecError::Malformed(format!("missing data offset for {name}")))?;
            let record = EntryRecord {
                name,
                compression,
                crc32: file.crc32(),
                compressed_size: file.compressed_size(),
                uncompressed_size: file.size(),
                header_start: file.header_start(),
                data_start,
                central_start: file.central_header_start(),
            };
            for offset in [
                record.header_start,
                record.data_start,
                record.data_end()?,
                record.central_start,
            ] {
                if offset > root.window().len() as u64 {
                    return Err(CodecError::Malformed(format!(
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

    /// Opens one entry as a borrowed stored slice or budgeted expanded view.
    pub fn open(
        &self,
        ctx: &DecodeContext<'a>,
        entry: &EntryRecord,
    ) -> Result<View<'a>, CodecError> {
        let end = entry.data_end()?;
        match entry.compression {
            EntryCompression::Stored => {
                let view = ctx.register_slice(
                    self.root,
                    ByteRange {
                        start: entry.data_start,
                        end,
                    },
                )?;
                if view.window().len() as u64 != entry.uncompressed_size {
                    return Err(CodecError::Malformed(format!(
                        "stored size mismatch for {}",
                        entry.name
                    )));
                }
                if crc32fast::hash(view.window()) != entry.crc32 {
                    return Err(CodecError::Malformed(format!(
                        "CRC mismatch for {}",
                        entry.name
                    )));
                }
                Ok(view)
            }
            EntryCompression::Deflate => {
                let start = usize::try_from(entry.data_start).map_err(|_| {
                    CodecError::Malformed("ZIP data offset does not fit memory".into())
                })?;
                let end = usize::try_from(end).map_err(|_| {
                    CodecError::Malformed("ZIP data offset does not fit memory".into())
                })?;
                let source = self.root.child(start, end).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "ZIP data range escapes archive for {}",
                        entry.name
                    ))
                })?;
                let mut decoder = flate2::read::DeflateDecoder::new(source.window());
                let mut writer =
                    ctx.begin_expand(source, ExpandSpec::Exact(entry.uncompressed_size))?;
                let mut chunk = [0_u8; 16 * 1024];
                loop {
                    let read = decoder.read(&mut chunk).map_err(|error| {
                        CodecError::Malformed(format!("cannot inflate {}: {error}", entry.name))
                    })?;
                    if read == 0 {
                        break;
                    }
                    writer.write(&chunk[..read])?;
                }
                let view = writer.finalize()?;
                if crc32fast::hash(view.window()) != entry.crc32 {
                    return Err(CodecError::Malformed(format!(
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
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write as _};

    use cadmpeg_codec_core::decode::{DecodeArena, DecodePolicy};
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
        archive.finish().expect("archive finishes").into_inner()
    }

    #[test]
    fn snapshot_opens_stored_and_deflated_entries_after_parser_drop() {
        let bytes = archive_bytes();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("archive fits root policy");
        let snapshot = ArchiveSnapshot::new(root).expect("archive snapshots");
        assert_eq!(snapshot.entries().len(), 2);
        let stored = snapshot.entry("stored.bin").expect("stored record");
        let deflated = snapshot.entry("deflated.bin").expect("deflated record");
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
    }
}
