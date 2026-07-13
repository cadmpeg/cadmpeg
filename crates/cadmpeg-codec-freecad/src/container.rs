// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, SeekFrom};
use std::path::{Component, Path};

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use zip::CompressionMethod;

use crate::native::{ArchiveSpan, DocumentFacts};

const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 16_384;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_EXPANSION_RATIO: u64 = 1_000;

/// Fully scanned container used by inspection and decode.
pub struct Scan {
    /// Container summary entries.
    pub entries: Vec<ContainerEntry>,
    /// Persistence metadata.
    pub document: DocumentFacts,
    /// Exact physical archive partition.
    pub ledger: Vec<ArchiveSpan>,
    /// Inflated entry data.
    pub data: BTreeMap<String, Vec<u8>>,
}

/// Scan an archive with deterministic resource limits.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<Scan, CodecError> {
    reader.seek(SeekFrom::Start(0))?;
    let mut source = Vec::new();
    reader
        .take((MAX_ARCHIVE_BYTES + 1) as u64)
        .read_to_end(&mut source)?;
    if source.len() > MAX_ARCHIVE_BYTES {
        return Err(CodecError::Malformed("archive size limit exceeded".into()));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(&source))
        .map_err(|error| CodecError::Malformed(format!("not a readable ZIP: {error}")))?;
    if archive.len() > MAX_ENTRIES {
        return Err(CodecError::Malformed(
            "ZIP entry-count limit exceeded".into(),
        ));
    }

    let mut names = BTreeSet::new();
    let mut entries = Vec::with_capacity(archive.len());
    let mut data = BTreeMap::new();
    let mut total = 0_u64;
    let mut boundaries = BTreeSet::from([0_u64, source.len() as u64]);
    let mut regions = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| CodecError::Malformed(format!("bad ZIP entry {index}: {error}")))?;
        let name = file.name().to_owned();
        validate_name(&name)?;
        if !names.insert(name.clone()) {
            return Err(CodecError::Malformed(format!(
                "duplicate ZIP entry name {name}"
            )));
        }
        if file.encrypted() {
            return Err(CodecError::Malformed(format!("encrypted ZIP entry {name}")));
        }
        if !matches!(
            file.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(CodecError::NotImplemented(format!(
                "FCStd ZIP compression {:?} for {name}",
                file.compression()
            )));
        }
        if file.size() > MAX_ENTRY_BYTES {
            return Err(CodecError::Malformed(format!(
                "entry size limit exceeded for {name}"
            )));
        }
        total = total
            .checked_add(file.size())
            .ok_or_else(|| CodecError::Malformed("expanded size overflow".into()))?;
        if total > MAX_TOTAL_BYTES {
            return Err(CodecError::Malformed(
                "total expanded-size limit exceeded".into(),
            ));
        }
        if file.compressed_size() > 0 && file.size() / file.compressed_size() > MAX_EXPANSION_RATIO
        {
            return Err(CodecError::Malformed(format!(
                "expansion-ratio limit exceeded for {name}"
            )));
        }

        let header_start = file.header_start();
        let data_start = file
            .data_start()
            .ok_or_else(|| CodecError::Malformed(format!("missing data offset for {name}")))?;
        let data_end = data_start
            .checked_add(file.compressed_size())
            .ok_or_else(|| CodecError::Malformed("compressed range overflow".into()))?;
        let central_start = file.central_header_start();
        for point in [header_start, data_start, data_end, central_start] {
            if point > source.len() as u64 {
                return Err(CodecError::Malformed(format!(
                    "ZIP offset outside archive for {name}"
                )));
            }
            boundaries.insert(point);
        }
        regions.push((header_start, data_start, "local-header", Some(name.clone())));
        regions.push((
            data_start,
            data_end,
            "compressed-payload",
            Some(name.clone()),
        ));

        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|error| CodecError::Malformed(format!("cannot read {name}: {error}")))?;
        let mut attributes = BTreeMap::new();
        attributes.insert("crc32".into(), format!("{:08x}", file.crc32()));
        attributes.insert("header_offset".into(), header_start.to_string());
        attributes.insert("data_offset".into(), data_start.to_string());
        attributes.insert("central_header_offset".into(), central_start.to_string());
        entries.push(ContainerEntry {
            role: classify(&name).into(),
            compression: compression_label(file.compression()).into(),
            compressed_size: file.compressed_size(),
            uncompressed_size: file.size(),
            name: name.clone(),
            attributes,
        });
        data.insert(name, bytes);
    }

    let document_bytes = data
        .get("Document.xml")
        .ok_or_else(|| CodecError::WrongFormat("ZIP has no root Document.xml".into()))?;
    let document = parse_document(document_bytes)?;
    let ledger = partition(source.len() as u64, boundaries, &regions);
    Ok(Scan {
        entries,
        document,
        ledger,
        data,
    })
}

/// Summarize one scan.
pub fn summarize(scan: &Scan) -> ContainerSummary {
    let mut notes = vec![
        format!("SchemaVersion={}", scan.document.schema_version),
        format!("FileVersion={}", scan.document.file_version),
        format!("document root={}", scan.document.root_name),
        format!("object count={}", scan.document.object_count),
        format!("physical ledger spans={} coverage=exact", scan.ledger.len()),
    ];
    if let Some(version) = &scan.document.program_version {
        notes.push(format!("ProgramVersion={version}"));
    }
    ContainerSummary {
        format: "fcstd".into(),
        container_kind: "zip".into(),
        entries: scan.entries.clone(),
        notes,
    }
}

fn validate_name(name: &str) -> Result<(), CodecError> {
    let path = Path::new(name);
    if name.is_empty()
        || path.is_absolute()
        || name.contains('\\')
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CodecError::Malformed(format!(
            "unsafe ZIP entry path {name:?}"
        )));
    }
    Ok(())
}

fn classify(name: &str) -> &'static str {
    match name {
        "Document.xml" => "document",
        "GuiDocument.xml" => "gui-document",
        "thumbnails/Thumbnail.png" | "Thumbnail.png" => "thumbnail",
        _ if name.ends_with('/') => "directory",
        _ if Path::new(name).extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("brp") || extension.eq_ignore_ascii_case("brep")
        }) =>
        {
            "brep"
        }
        _ => "auxiliary",
    }
}

fn compression_label(method: CompressionMethod) -> &'static str {
    match method {
        CompressionMethod::Stored => "stored",
        CompressionMethod::Deflated => "deflate",
        _ => "unsupported",
    }
}

fn attr(root: roxmltree::Node<'_, '_>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| root.attribute(*name).map(str::to_owned))
}

fn parse_document(bytes: &[u8]) -> Result<DocumentFacts, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::Malformed(format!("invalid Document.xml: {error}")))?;
    let root = xml.root_element();
    let schema_version = attr(root, &["SchemaVersion", "schemaVersion"])
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no SchemaVersion".into()))?;
    let file_version = attr(root, &["FileVersion", "fileVersion"])
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no FileVersion".into()))?;
    let object_count = root
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "Object")
        .count();
    Ok(DocumentFacts {
        id: "fcstd:document#0".into(),
        schema_version,
        file_version,
        program_version: attr(root, &["ProgramVersion", "programVersion"]),
        root_name: root.tag_name().name().into(),
        object_count,
    })
}

fn partition(
    len: u64,
    boundaries: BTreeSet<u64>,
    regions: &[(u64, u64, &'static str, Option<String>)],
) -> Vec<ArchiveSpan> {
    let points = boundaries.into_iter().collect::<Vec<_>>();
    points
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| {
            let (start, end) = (pair[0], pair[1]);
            (start < end).then(|| {
                let owner = regions.iter().find(|(a, b, _, _)| *a <= start && end <= *b);
                let (role, entry) = owner.map_or(("zip-structure", None), |(_, _, role, entry)| {
                    (*role, entry.clone())
                });
                ArchiveSpan {
                    id: format!("fcstd:archive-span#{index}"),
                    start,
                    end: end.min(len),
                    role: role.into(),
                    entry,
                }
            })
        })
        .collect()
}
