// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Component, Path};

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::debug_assert_primary_layer;
use cadmpeg_core::{CodecError, ContainerEntry, ContainerSummary};

use crate::brep::ShapePayloadRecord;
use crate::dialect::FcstdDialect;
use crate::gui;
use crate::native::{
    ArchiveSpan, ByteCoverageRecord, DocumentFacts, ElementMapRecord, EntryRecord, LogicalSpan,
    PropertyFamily, PropertyRecord, StringTableRecord,
};

const DETECTION_XML_BYTES: usize = 8 * 1024;

/// Inspect the first local entry deeply enough to confirm `FCStd` document markers.
pub(crate) fn has_document_markers(prefix: &[u8]) -> bool {
    if prefix.len() < 30 || &prefix[..4] != b"PK\x03\x04" {
        return false;
    }
    let Some(method) = View::u16_le_at(prefix, 8) else {
        return false;
    };
    let Some(name_len) = View::u16_le_at(prefix, 26).map(usize::from) else {
        return false;
    };
    let Some(extra_len) = View::u16_le_at(prefix, 28).map(usize::from) else {
        return false;
    };
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
    contains(&document, b"<Document") && contains(&document, b"SchemaVersion")
}

/// Fully scanned container used by inspection and decode.
pub struct Scan<'a> {
    /// Container summary entries.
    pub entries: Vec<ContainerEntry>,
    /// Persistence metadata.
    pub document: DocumentFacts,
    /// Typed persistence schema selected at the container boundary.
    pub(crate) schema: FcstdDialect,
    /// Exact physical archive partition.
    pub ledger: Vec<ArchiveSpan>,
    /// Inflated entry views, each retaining its [`SpaceId`](cadmpeg_core::decode::SpaceId).
    pub data: BTreeMap<String, View<'a>>,
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
        data.insert(name, view);
    }

    let document_bytes = data
        .get("Document.xml")
        .map(|view| view.window())
        .ok_or_else(|| CodecError::WrongFormat("ZIP has no root Document.xml".into()))?;
    let (document, schema) = parse_document(document_bytes)?;
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
        schema,
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
    let summary = ContainerSummary {
        dialects: vec![FcstdDialect::classify(&scan.document, scan.schema)],
        format: crate::dialect::FORMAT.into(),
        container_kind: "zip".into(),
        entries: scan.entries.clone(),
        notes,
    };
    debug_assert_primary_layer(&summary.dialects, &summary.format);
    summary
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
        return Err(CodecError::malformed(format_args!(
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

pub(crate) fn canonical_attribute(
    root: roxmltree::Node<'_, '_>,
    canonical: &str,
    alias: &str,
) -> Result<Option<String>, CodecError> {
    match (root.attribute(canonical), root.attribute(alias)) {
        (Some(_), Some(_)) => Err(CodecError::malformed(format_args!(
            "Document element has both {canonical} and {alias} attributes"
        ))),
        (Some(value), None) => Ok(Some(value.to_owned())),
        (None, Some(_)) => Err(CodecError::malformed(format_args!(
            "Document element uses unsupported {alias}; expected {canonical}"
        ))),
        (None, None) => Ok(None),
    }
}

fn unique_section<'a, 'input>(
    root: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Result<Option<roxmltree::Node<'a, 'input>>, CodecError> {
    let sections = root
        .children()
        .filter(|node| node.has_tag_name(tag))
        .collect::<Vec<_>>();
    match sections.as_slice() {
        [section] => Ok(Some(*section)),
        [] => Ok(None),
        _ => Err(CodecError::malformed(format_args!(
            "Document.xml has duplicate {tag} sections"
        ))),
    }
}

fn parse_document(bytes: &[u8]) -> Result<(DocumentFacts, FcstdDialect), CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::malformed(format_args!("invalid Document.xml: {error}")))?;
    let root = xml.root_element();
    if root.tag_name().name() != "Document" {
        return Err(CodecError::WrongFormat(format!(
            "Document.xml root is {}, expected Document",
            root.tag_name().name()
        )));
    }
    let schema_version = canonical_attribute(root, "SchemaVersion", "schemaVersion")?
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no SchemaVersion".into()))?;
    let file_version =
        canonical_attribute(root, "FileVersion", "fileVersion")?.unwrap_or_else(|| "0".into());
    schema_version
        .parse::<u32>()
        .map_err(|_| CodecError::Malformed("Document.xml SchemaVersion is invalid".into()))?;
    let schema = FcstdDialect::from_schema_version(&schema_version);
    file_version
        .parse::<u32>()
        .map_err(|_| CodecError::Malformed("Document.xml FileVersion is invalid".into()))?;
    let (declaration_tag, data_tag, record_tag) = crate::persistence::persistence_tags(schema);
    let _ = unique_section(root, data_tag)?;
    let declarations = unique_section(root, declaration_tag)?
        .into_iter()
        .flat_map(|section| section.children())
        .filter(|node| node.has_tag_name(record_tag))
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
    let document = DocumentFacts {
        id: crate::native::native_id("document", "0"),
        schema_version,
        file_version,
        program_version: canonical_attribute(root, "ProgramVersion", "programVersion")?,
        root_name: root.tag_name().name().into(),
        object_count,
        document_kind,
        domains,
    };
    Ok((document, schema))
}

pub(crate) fn logical_ledger(
    entries: &[EntryRecord],
    properties: &[PropertyRecord],
    gui: &gui::Graph,
    shape_payloads: &[ShapePayloadRecord],
    string_tables: &[StringTableRecord],
    element_maps: &[ElementMapRecord],
) -> Result<Vec<LogicalSpan>, CodecError> {
    let typed_entries = shape_payloads
        .iter()
        .map(|payload| payload.entry.as_str())
        .chain(
            string_tables
                .iter()
                .filter_map(|table| table.source_entry.as_deref()),
        )
        .chain(
            element_maps
                .iter()
                .filter_map(|map| map.source_entry.as_deref()),
        )
        .collect::<HashSet<_>>();
    let mut output = Vec::new();
    for entry in entries {
        if typed_entries.contains(entry.id.as_str()) || typed_entries.contains(entry.name.as_str())
        {
            push_logical_span(
                &mut output,
                entry,
                0,
                entry.byte_len,
                "typed",
                Some(entry.id.clone()),
            );
        } else if entry.name == "Document.xml" || entry.name == "GuiDocument.xml" {
            let mut ranges = if entry.name == "Document.xml" {
                properties
                    .iter()
                    .map(|property| {
                        (
                            property.byte_start,
                            property.byte_end,
                            if property.family == PropertyFamily::Unknown {
                                "named_opaque"
                            } else {
                                "typed"
                            },
                            property.id.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                gui.properties
                    .iter()
                    .map(|property| {
                        (
                            property.byte_start,
                            property.byte_end,
                            if gui::has_registered_property_grammar(
                                &property.name,
                                &property.type_name,
                            ) {
                                "typed"
                            } else {
                                "named_opaque"
                            },
                            property.id.clone(),
                        )
                    })
                    .chain(gui.documents.iter().flat_map(|document| {
                        document.states.iter().map(|state| {
                            (state.byte_start, state.byte_end, "typed", state.id.clone())
                        })
                    }))
                    .collect::<Vec<_>>()
            };
            ranges.sort_by_key(|range| range.0);
            let mut cursor = 0_u64;
            for (start, end, classification, owner) in ranges {
                if start < cursor || end < start || end > entry.byte_len {
                    return Err(CodecError::malformed(format_args!(
                        "overlapping or invalid {} record spans",
                        entry.name
                    )));
                }
                push_logical_span(&mut output, entry, cursor, start, "structural", None);
                push_logical_span(&mut output, entry, start, end, classification, Some(owner));
                cursor = end;
            }
            push_logical_span(
                &mut output,
                entry,
                cursor,
                entry.byte_len,
                "structural",
                None,
            );
        } else {
            push_logical_span(
                &mut output,
                entry,
                0,
                entry.byte_len,
                "named_opaque",
                Some(entry.id.clone()),
            );
        }
    }
    Ok(output)
}

pub(crate) fn byte_coverage(
    physical: &[ArchiveSpan],
    entries: &[EntryRecord],
    logical: &[LogicalSpan],
    physical_byte_len: u64,
) -> ByteCoverageRecord {
    let mut classification_bytes = BTreeMap::new();
    let mut named_opaque_entries = BTreeSet::new();
    for span in logical {
        *classification_bytes
            .entry(span.classification.clone())
            .or_insert(0) += span.end.saturating_sub(span.start);
        if span.classification == "named_opaque" {
            named_opaque_entries.insert(span.entry.clone());
        }
    }
    let mut ordered_physical = physical.iter().collect::<Vec<_>>();
    ordered_physical.sort_by_key(|span| span.start);
    let physical_exact = ordered_physical.first().is_some_and(|span| span.start == 0)
        && ordered_physical.iter().all(|span| span.start < span.end)
        && ordered_physical
            .windows(2)
            .all(|pair| pair[0].end == pair[1].start)
        && ordered_physical
            .last()
            .is_some_and(|span| span.end == physical_byte_len);
    let logical_exact = logical.iter().all(|span| {
        entries.iter().any(|entry| entry.name == span.entry)
            && span.start < span.end
            && matches!(
                span.classification.as_str(),
                "structural" | "typed" | "named_opaque"
            )
    }) && entries.iter().all(|entry| {
        let mut spans = logical
            .iter()
            .filter(|span| span.entry == entry.name)
            .collect::<Vec<_>>();
        spans.sort_by_key(|span| span.start);
        if entry.byte_len == 0 {
            spans.is_empty()
        } else {
            spans.first().is_some_and(|span| span.start == 0)
                && spans.windows(2).all(|pair| pair[0].end == pair[1].start)
                && spans.last().is_some_and(|span| span.end == entry.byte_len)
        }
    });
    ByteCoverageRecord {
        id: crate::native::native_id("byte-coverage", "0"),
        physical_byte_len,
        physical_span_count: physical.len(),
        logical_entry_count: entries.len(),
        logical_byte_len: entries.iter().map(|entry| entry.byte_len).sum(),
        logical_span_count: logical.len(),
        classification_bytes,
        named_opaque_entries: named_opaque_entries.into_iter().collect(),
        exact: physical_exact && logical_exact,
    }
}

fn push_logical_span(
    output: &mut Vec<LogicalSpan>,
    entry: &EntryRecord,
    start: u64,
    end: u64,
    classification: &str,
    owner: Option<String>,
) {
    if start < end {
        output.push(LogicalSpan {
            id: crate::native::native_id("logical-span", output.len().to_string()),
            entry: entry.name.clone(),
            start,
            end,
            classification: classification.into(),
            owner,
        });
    }
}

#[cfg(test)]
pub(crate) mod tests;
