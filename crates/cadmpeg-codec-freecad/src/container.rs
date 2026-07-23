// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, SeekFrom};
use std::path::{Component, Path};

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use cadmpeg_ir::container::{walk_bounded, WalkConfig};
use flate2::{Decompress, FlushDecompress};
use zip::CompressionMethod;

use crate::native::{ArchiveSpan, DocumentFacts};

const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 16_384;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_EXPANSION_RATIO: u64 = 1_000;
const DETECTION_XML_BYTES: usize = 8 * 1024;

/// Inspect the first local entry deeply enough to confirm `FCStd` document markers.
pub(crate) fn has_document_markers(prefix: &[u8]) -> bool {
    if prefix.len() < 30 || &prefix[..4] != b"PK\x03\x04" {
        return false;
    }
    let method = u16::from_le_bytes([prefix[8], prefix[9]]);
    let name_len = u16::from_le_bytes([prefix[26], prefix[27]]) as usize;
    let extra_len = u16::from_le_bytes([prefix[28], prefix[29]]) as usize;
    let name_end = 30_usize.saturating_add(name_len);
    let data_start = name_end.saturating_add(extra_len);
    if name_end > prefix.len()
        || data_start > prefix.len()
        || &prefix[30..name_end] != b"Document.xml"
    {
        return false;
    }
    let compressed = &prefix[data_start..];
    let document = match method {
        0 => compressed.to_vec(),
        8 => {
            let mut output = Vec::with_capacity(DETECTION_XML_BYTES);
            if Decompress::new(false)
                .decompress_vec(compressed, &mut output, FlushDecompress::None)
                .is_err()
            {
                return false;
            }
            output
        }
        _ => return false,
    };
    memchr::memmem::find(&document, b"<Document").is_some()
        && memchr::memmem::find(&document, b"SchemaVersion").is_some()
        && memchr::memmem::find(&document, b"FileVersion").is_some()
}

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
    let mut raw_entries = Vec::new();

    // The shared walker owns entry iteration, name validation, and the
    // per-entry and total inflated-size caps (`MAX_ENTRY_BYTES`,
    // `MAX_TOTAL_BYTES`); it inflates each entry into a capped `Vec`. Every
    // check the walker does not perform — duplicate names, encryption, the
    // compression-method allowlist, the expansion-ratio guard, the physical
    // offset bounds, and the exact-declared-size assertion — stays here in the
    // callback, so decode and inspect see the same acceptance boundary.
    let source_len = source.len() as u64;
    let config = WalkConfig {
        classify,
        validate_name,
        max_entry_bytes: MAX_ENTRY_BYTES,
        max_total_bytes: MAX_TOTAL_BYTES,
    };
    walk_bounded(&mut archive, &config, |entry, bytes| {
        let name = entry.name.as_str();
        if !names.insert(entry.name.clone()) {
            return Err(CodecError::Malformed(format!(
                "duplicate ZIP entry name {name}"
            )));
        }
        if entry.encrypted {
            return Err(CodecError::Malformed(format!("encrypted ZIP entry {name}")));
        }
        if !matches!(
            entry.compression,
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(CodecError::NotImplemented(format!(
                "FCStd ZIP compression {:?} for {name}",
                entry.compression
            )));
        }
        if entry.compressed_size > 0
            && entry.uncompressed_size / entry.compressed_size > MAX_EXPANSION_RATIO
        {
            return Err(CodecError::Malformed(format!(
                "expansion-ratio limit exceeded for {name}"
            )));
        }

        let header_start = entry.header_start;
        let data_start = entry
            .data_start
            .ok_or_else(|| CodecError::Malformed(format!("missing data offset for {name}")))?;
        let data_end = data_start
            .checked_add(entry.compressed_size)
            .ok_or_else(|| CodecError::Malformed("compressed range overflow".into()))?;
        let central_start = entry.central_start;
        for point in [header_start, data_start, data_end, central_start] {
            if point > source_len {
                return Err(CodecError::Malformed(format!(
                    "ZIP offset outside archive for {name}"
                )));
            }
        }
        raw_entries.push(RawEntry {
            name: entry.name.clone(),
            header_start,
            data_start,
            data_end,
            central_start,
            crc32: entry.crc32,
            compressed_size: entry.compressed_size,
            uncompressed_size: entry.uncompressed_size,
        });

        let declared_size = entry.uncompressed_size;
        if u64::try_from(bytes.len()).ok() != Some(declared_size) {
            return Err(CodecError::Malformed(format!(
                "entry {name} expanded to {} bytes but declares {declared_size}",
                bytes.len()
            )));
        }
        let mut attributes = BTreeMap::new();
        attributes.insert("crc32".into(), format!("{:08x}", entry.crc32));
        attributes.insert("header_offset".into(), header_start.to_string());
        attributes.insert("data_offset".into(), data_start.to_string());
        attributes.insert("central_header_offset".into(), central_start.to_string());
        entries.push(ContainerEntry {
            role: entry.role.into(),
            compression: compression_label(entry.compression).into(),
            compressed_size: entry.compressed_size,
            uncompressed_size: entry.uncompressed_size,
            name: entry.name.clone(),
            attributes,
        });
        data.insert(entry.name.clone(), bytes);
        Ok(())
    })?;

    let document_bytes = data
        .get("Document.xml")
        .ok_or_else(|| CodecError::WrongFormat("ZIP has no root Document.xml".into()))?;
    let document = parse_document(document_bytes)?;
    let ledger = physical_ledger(&source, &raw_entries)?;
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
        format!("document kind={}", scan.document.document_kind),
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
    if root.tag_name().name() != "Document" {
        return Err(CodecError::WrongFormat(format!(
            "Document.xml root is {}, expected Document",
            root.tag_name().name()
        )));
    }
    let schema_version = attr(root, &["SchemaVersion", "schemaVersion"])
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no SchemaVersion".into()))?;
    let file_version = attr(root, &["FileVersion", "fileVersion"])
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no FileVersion".into()))?;
    let declarations = root
        .children()
        .find(|node| node.has_tag_name("Objects"))
        .into_iter()
        .flat_map(|objects| {
            objects
                .children()
                .filter(|node| node.has_tag_name("Object"))
        })
        .collect::<Vec<_>>();
    let object_count = declarations.len();
    let domains = declarations
        .iter()
        .filter_map(|node| node.attribute("type"))
        .filter_map(|type_name| {
            type_name
                .split_once("::")
                .map(|(domain, _)| domain.to_owned())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let document_kind = if domains.iter().any(|domain| domain == "Assembly") {
        "assembly"
    } else if domains.iter().any(|domain| domain == "TechDraw") {
        "drawing"
    } else if domains.iter().any(|domain| domain == "PartDesign") {
        "part-design"
    } else if domains.iter().any(|domain| domain == "Part") {
        "part"
    } else if object_count == 0 {
        "empty"
    } else {
        "application-document"
    }
    .to_owned();
    Ok(DocumentFacts {
        id: crate::native::native_id("document", "0"),
        schema_version,
        file_version,
        program_version: attr(root, &["ProgramVersion", "programVersion"]),
        root_name: root.tag_name().name().into(),
        object_count,
        document_kind,
        domains,
    })
}

#[derive(Debug)]
struct RawEntry {
    name: String,
    header_start: u64,
    data_start: u64,
    data_end: u64,
    central_start: u64,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
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
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], offset: u64) -> Result<u32, CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed("ZIP offset does not fit memory".into()))?;
    let raw = bytes
        .get(start..start + 4)
        .ok_or_else(|| CodecError::Malformed("truncated ZIP integer".into()))?;
    Ok(u32::from_le_bytes(raw.try_into().expect("four-byte slice")))
}

fn u64_at(bytes: &[u8], offset: u64) -> Result<u64, CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed("ZIP offset does not fit memory".into()))?;
    let raw = bytes
        .get(start..start + 8)
        .ok_or_else(|| CodecError::Malformed("truncated ZIP integer".into()))?;
    Ok(u64::from_le_bytes(
        raw.try_into().expect("eight-byte slice"),
    ))
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

fn physical_ledger(bytes: &[u8], entries: &[RawEntry]) -> Result<Vec<ArchiveSpan>, CodecError> {
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
            return Err(CodecError::Malformed(format!(
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
            return Err(CodecError::Malformed(format!(
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
            entry.data_end,
            "compressed-payload",
            Some(&entry.name),
        );

        let next = local_order
            .get(index + 1)
            .map_or(central_begin, |next| next.header_start);
        if entry.data_end > next {
            return Err(CodecError::Malformed(format!(
                "compressed payload overlaps following ZIP record for {}",
                entry.name
            )));
        }
        if entry.data_end < next {
            let flags = u16_at(bytes, entry.header_start + 6)?;
            if flags & 0x0008 != 0 {
                let descriptor_end = parse_data_descriptor(bytes, entry, next)?;
                push_region(
                    &mut regions,
                    entry.data_end,
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
                    entry.data_end,
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
            return Err(CodecError::Malformed(format!(
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
            return Err(CodecError::Malformed(format!(
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
    entry: &RawEntry,
    record_end: u64,
) -> Result<u64, CodecError> {
    let start = entry.data_end;
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
    Err(CodecError::Malformed(format!(
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
                let body = u64::from_le_bytes(raw.try_into().expect("eight-byte slice"));
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
            return Err(CodecError::Malformed(format!("truncated {role}")));
        }
        push_region(regions, offset, end, role, None);
        offset = end;
    }
    Ok(())
}

fn partition(len: u64, regions: &[Region]) -> Result<Vec<ArchiveSpan>, CodecError> {
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
        .enumerate()
        .filter_map(|(index, pair)| {
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
                ArchiveSpan {
                    id: crate::native::native_id("archive-span", index.to_string()),
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
