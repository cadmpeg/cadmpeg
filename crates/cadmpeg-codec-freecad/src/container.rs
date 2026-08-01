// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use cadmpeg_codec_core::decode::{DecodeContext, View};
use cadmpeg_codec_core::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_container::ArchiveSnapshot;

use crate::native::{ArchiveSpan, DocumentFacts};

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
        8 => match cadmpeg_container::compression::inflate_bounded_probe(
            compressed,
            DETECTION_XML_BYTES,
        ) {
            Some(output) => output,
            None => return false,
        },
        _ => return false,
    };
    contains(&document, b"<Document")
        && contains(&document, b"SchemaVersion")
        && contains(&document, b"FileVersion")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Fully scanned container used by inspection and decode.
pub struct Scan<'a> {
    /// Container summary entries.
    pub entries: Vec<ContainerEntry>,
    /// Persistence metadata.
    pub document: DocumentFacts,
    /// Exact physical archive partition.
    pub ledger: Vec<ArchiveSpan>,
    /// Inflated entry data.
    pub data: BTreeMap<String, &'a [u8]>,
}

/// Scan an archive through the session resource budget.
pub fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let archive = ArchiveSnapshot::new(root)?;
    ctx.charge_collection_items(archive.entries().len() as u64, "fcstd ZIP entries")?;
    let mut data = BTreeMap::new();
    for file in archive.entries() {
        let name = file.name.clone();
        validate_name(&name)?;
        let view = archive.open(ctx, file)?;
        data.insert(name, view.window());
    }

    let document_bytes = data
        .get("Document.xml")
        .ok_or_else(|| CodecError::WrongFormat("ZIP has no root Document.xml".into()))?;
    let document = parse_document(document_bytes)?;
    let ledger = archive
        .physical_ledger()?
        .into_iter()
        .enumerate()
        .map(|(index, span)| ArchiveSpan {
            id: crate::native::native_id("archive-span", index.to_string()),
            start: span.start,
            end: span.end,
            role: span.role,
            entry: span.entry,
        })
        .collect();
    Ok(Scan {
        entries: archive.container_entries(classify),
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
