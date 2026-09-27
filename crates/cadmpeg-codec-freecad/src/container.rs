// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::ContainerSummary;

use crate::brep::ShapePayloadRecord;
use crate::gui;
use crate::native::element_map::ElementMapRecord;
use crate::native::{
    ArchiveSpan, ByteCoverageRecord, DocumentFacts, EntryRecord, LogicalClassification,
    LogicalSpan, PropertyFamily, PropertyRecord, StringTableRecord,
};
use crate::resource::{collection_vec, insert_hash_set, reserve_vec_items, retained_string, retained_suffix};

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
    let Some(name_end) = 30_usize.checked_add(name_len) else {
        return false;
    };
    let Some(data_start) = name_end.checked_add(extra_len) else {
        return false;
    };
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
pub(crate) struct Scan<'a> {
    /// Container summary entries.
    pub(crate) entries: Vec<ContainerEntry>,
    /// Persistence metadata.
    pub(crate) document: DocumentFacts,
    /// Declared persistence schema version, owned by the source declaration.
    pub(crate) schema_version: String,
    /// Exact physical archive partition.
    pub(crate) ledger: Vec<ArchiveSpan>,
    /// Inflated entry views, each retaining its [`SpaceId`](cadmpeg_core::decode::SpaceId).
    pub(crate) data: BTreeMap<String, View<'a>>,
}

/// Scan an archive through the session resource budget.
pub(crate) fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let archive = ArchiveSnapshot::new(ctx, root)?;
    ctx.charge_collection_items(archive.entries().len() as u64, "fcstd ZIP entries")?;
    if !archive.entries().iter().any(|file| file.name == "Document.xml") {
        return Err(CodecError::WrongFormat("ZIP has no root Document.xml".into()));
    }
    let document_view = archive.open(ctx, "Document.xml")?;
    let document_bytes = document_view.window();
    ctx.charge_work(
        document_bytes.len() as u64,
        "FCStd Document.xml lexical admission",
    )?;
    if let Some((node_count, object_count)) = xml_envelope_counts(document_bytes) {
        ctx.charge_entities(object_count, "admit FCStd document objects")?;
        ctx.charge_collection_items(node_count, "FCStd Document.xml node tree")?;
    }
    let (document, schema_version) = parse_document(ctx, document_bytes)?;
    let mut data = BTreeMap::new();
    for file in archive.entries() {
        let name = retained_string(ctx, &file.name, "FCStd archive entry name")?;
        crate::native::check_entry_name(&name).map_err(CodecError::Malformed)?;
        let view = if file.name == "Document.xml" {
            document_view
        } else {
            archive.open(ctx, &file.name)?
        };
        ctx.charge_collection_items(1, "FCStd archive entry map")?;
        data.insert(name, view);
    }
    let physical_ledger = archive.physical_ledger()?;
    let mut ledger = collection_vec(ctx, physical_ledger.len(), "FCStd archive ledger records")?;
    for (index, span) in physical_ledger.into_iter().enumerate() {
        ledger.push(ArchiveSpan {
                id: crate::native::native_id("archive-span", index.to_string()),
                span: crate::native::ByteSpan::try_new(span.start, span.end)
                    .map_err(CodecError::Malformed)?,
                role: crate::native::ArchiveSpanRole::from(&span.role),
            });
    }
    Ok(Scan {
        entries: archive.container_entries(classify),
        document,
        schema_version,
        ledger,
        data,
    })
}

pub(crate) fn entry_records(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    properties: &[PropertyRecord],
) -> Result<Vec<EntryRecord>, CodecError> {
    let mut records = collection_vec(ctx, scan.entries.len(), "FCStd entry records")?;
    for entry in &scan.entries {
        let bytes = scan.data.get(&entry.name).map(|view| view.window()).ok_or_else(|| {
            CodecError::malformed(format_args!("entry {} disappeared after scan", entry.name))
        })?;
        let mut referenced_by = Vec::new();
        for property in properties.iter().filter(|property| property.side_entries().contains(&entry.name)) {
            reserve_vec_items(ctx, &mut referenced_by, 1, "FCStd entry referencing properties")?;
            referenced_by.push(retained_string(ctx, &property.id, "FCStd entry referencing identity")?);
        }
        records.push(EntryRecord {
            id: crate::native::native_id_charged(ctx, "entry", &entry.name)?,
            name: retained_string(ctx, &entry.name, "FCStd entry record name")?,
            role: entry.role,
            referenced_by,
            data: ctx.copy_retained(bytes, "retain FCStd entry")?,
        });
    }
    Ok(records)
}

pub(crate) fn source_attributes(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    let mut attributes = BTreeMap::new();
    attributes.insert(cadmpeg_core::nonblank_literal!("document_root"), scan.document.root_name.clone());
    attributes.insert(cadmpeg_core::nonblank_literal!("object_count"), scan.document.object_count.to_string());
    attributes.insert(cadmpeg_core::nonblank_literal!("document_kind"), scan.document.document_kind().as_str().to_owned());
    attributes.insert(
        cadmpeg_core::nonblank_literal!("application_domains"),
        crate::resource::retained_join(ctx, &scan.document.domains, ",", "FCStd source domain list")?,
    );
    attributes.insert(cadmpeg_core::nonblank_literal!("archive_entry_count"), scan.entries.len().to_string());
    attributes.insert(cadmpeg_core::nonblank_literal!("physical_ledger_spans"), scan.ledger.len().to_string());
    if let Some(last) = scan.ledger.last() {
        attributes.insert(cadmpeg_core::nonblank_literal!("physical_archive_bytes"), last.span.end().to_string());
    }
    if let Some(value) = &scan.document.program_version {
        attributes.insert(
            cadmpeg_core::nonblank_literal!("program_version"),
            retained_string(ctx, value, "FCStd source program version")?,
        );
    }
    Ok(attributes)
}

/// Summarize one scan.
pub(crate) fn summarize(ctx: &DecodeContext<'_>, scan: &Scan) -> Result<ContainerSummary, CodecError> {
    let matched = crate::dialect::FcstdDialect::classify(&scan.document, &scan.schema_version);
    let losses = crate::dialect::FcstdDialect::dialect_loss(&matched)
        .into_iter()
        .collect();
    let mut entries = collection_vec(ctx, scan.entries.len(), "FCStd summary entries")?;
    for entry in &scan.entries {
        let mut attributes = BTreeMap::new();
        for (key, value) in &entry.attributes {
            ctx.charge_collection_items(1, "FCStd summary entry attributes")?;
            attributes.insert(
                retained_string(ctx, key, "FCStd summary attribute key")?,
                retained_string(ctx, value, "FCStd summary attribute value")?,
            );
        }
        entries.push(ContainerEntry {
            name: retained_string(ctx, &entry.name, "FCStd summary entry name")?,
            role: entry.role,
            storage: entry.storage.clone(),
            attributes,
        });
    }
    Ok(ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched),
        cadmpeg_ir::ContainerKind::Zip,
        entries,
        losses,
        summary_notes(ctx, scan)?,
    ))
}

/// Notes shared by inspect and decode without reclassifying host identity.
pub(crate) fn summary_notes(ctx: &DecodeContext<'_>, scan: &Scan) -> Result<Vec<String>, CodecError> {
    let mut notes = vec![
        retained_suffix(ctx, "SchemaVersion=", &scan.schema_version, "FCStd schema note")?,
        retained_suffix(ctx, "FileVersion=", scan.document.file_version.as_str(), "FCStd file version note")?,
        retained_suffix(ctx, "document root=", &scan.document.root_name, "FCStd document root note")?,
        retained_suffix(ctx, "document kind=", scan.document.document_kind().as_str(), "FCStd document kind note")?,
        format!("object count={}", scan.document.object_count),
        format!("physical ledger spans={} coverage=exact", scan.ledger.len()),
    ];
    if let Some(version) = &scan.document.program_version {
        notes.push(retained_suffix(ctx, "ProgramVersion=", version, "FCStd program version note")?);
    }
    Ok(notes)
}

fn classify(name: &str) -> ContainerRole {
    match name {
        "Document.xml" => ContainerRole::Document,
        "GuiDocument.xml" => ContainerRole::GuiDocument,
        "thumbnails/Thumbnail.png" | "Thumbnail.png" => ContainerRole::Thumbnail,
        _ if name.ends_with('/') => ContainerRole::Directory,
        _ if Path::new(name).extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("brp") || extension.eq_ignore_ascii_case("brep")
        }) =>
        {
            ContainerRole::Brep
        }
        _ => ContainerRole::Auxiliary,
    }
}

pub(crate) fn canonical_attribute(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    canonical: &str,
    alias: &str,
) -> Result<Option<String>, CodecError> {
    match (root.attribute(canonical), root.attribute(alias)) {
        (Some(_), Some(_)) => Err(CodecError::malformed(format_args!(
            "Document element has both {canonical} and {alias} attributes"
        ))),
        (Some(value), None) => Ok(Some(retained_string(ctx, value, "FCStd document attribute")?)),
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
    let mut sections = root
        .children()
        .filter(|node| node.has_tag_name(tag));
    let first = sections.next();
    if sections.next().is_some() {
        Err(CodecError::malformed(format_args!(
            "Document.xml has duplicate {tag} sections"
        )))
    } else {
        Ok(first)
    }
}

// This allocation-free lexical pass admits the XML tree and direct object
// declarations before roxmltree constructs any nodes. Syntax errors remain
// owned by the complete XML parser.
pub(crate) fn xml_envelope_counts(bytes: &[u8]) -> Option<(u64, u64)> {
    let mut offset = 0;
    let mut depth = 0_usize;
    // Include the document node. Count lexical text and markup nodes as an upper
    // bound because roxmltree can merge adjacent text into one stored node.
    let mut nodes = 1_u64;
    let mut objects = 0_u64;
    let mut envelope = None;
    while offset < bytes.len() {
        if bytes[offset] != b'<' {
            let next = bytes[offset..]
                .iter()
                .position(|byte| *byte == b'<')
                .map_or(bytes.len(), |delta| offset + delta);
            nodes = nodes.checked_add(1)?;
            offset = next;
            continue;
        }
        let rest = &bytes[offset..];
        if let Some((prefix, suffix)) = [
            (b"<!--".as_slice(), b"-->".as_slice()),
            (b"<![CDATA[".as_slice(), b"]]>".as_slice()),
            (b"<?".as_slice(), b"?>".as_slice()),
        ]
        .into_iter()
        .find(|(prefix, _)| rest.starts_with(prefix))
        {
            nodes = nodes.checked_add(1)?;
            let tail = &rest[prefix.len()..];
            let end = tail
                .windows(suffix.len())
                .position(|window| window == suffix)?;
            offset += prefix.len() + end + suffix.len();
            continue;
        }
        if rest.starts_with(b"<!") {
            nodes = nodes.checked_add(1)?;
            offset = scan_tag_end(bytes, offset + 2)? + 1;
            continue;
        }
        let closing = rest.get(1) == Some(&b'/');
        let name_start = offset + if closing { 2 } else { 1 };
        let mut name_end = name_start;
        while let Some(byte) = bytes.get(name_end) {
            if byte.is_ascii_whitespace() || *byte == b'/' || *byte == b'>' {
                break;
            }
            name_end += 1;
        }
        if name_end == name_start {
            return None;
        }
        let end = scan_tag_end(bytes, name_end)?;
        let name = bytes[name_start..name_end]
            .rsplit(|byte| *byte == b':')
            .next()
            .unwrap_or(&bytes[name_start..name_end]);
        if closing {
            depth = depth.checked_sub(1)?;
            if depth == 1 {
                envelope = None;
            }
        } else {
            nodes = nodes.checked_add(1)?;
            if depth == 1 && (name == b"Objects" || name == b"Features") {
                envelope = Some(if name == b"Objects" {
                    b"Object".as_slice()
                } else {
                    b"Feature".as_slice()
                });
            } else if depth == 2 && envelope == Some(name) {
                objects = objects.checked_add(1)?;
            }
            let self_closing = bytes[name_end..end]
                .iter()
                .rev()
                .find(|byte| !byte.is_ascii_whitespace())
                == Some(&b'/');
            if !self_closing {
                depth = depth.checked_add(1)?;
            } else if depth == 1 {
                envelope = None;
            }
        }
        offset = end + 1;
    }
    (depth == 0).then_some((nodes, objects))
}

fn scan_tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, &byte) in bytes.iter().enumerate().skip(start) {
        match (quote, byte) {
            (None, b'\'' | b'"') => quote = Some(byte),
            (Some(open), close) if open == close => quote = None,
            (None, b'>') => return Some(offset),
            _ => {}
        }
    }
    None
}

pub(crate) fn parse_document(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<(DocumentFacts, String), CodecError> {
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
    let schema_version = canonical_attribute(ctx, root, "SchemaVersion", "schemaVersion")?
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no SchemaVersion".into()))?;
    let file_version =
        canonical_attribute(ctx, root, "FileVersion", "fileVersion")?.unwrap_or_else(|| "0".into());
    schema_version
        .parse::<u32>()
        .map_err(|_| CodecError::Malformed("Document.xml SchemaVersion is invalid".into()))?;
    let file_version =
        crate::native::FileVersion::try_from(file_version).map_err(CodecError::Malformed)?;
    let schema = crate::dialect::FcstdDialect::from_schema_version(&schema_version);
    let (declaration_tag, data_tag, record_tag) = schema.persistence_tags();
    // discarded-value: the data section's uniqueness is the check; ? states its refusal and the node has no reader
    let _ = unique_section(root, data_tag)?;
    let declarations = unique_section(root, declaration_tag)?;
    let mut object_count = 0;
    let mut domain_set = BTreeSet::new();
    for node in declarations.into_iter().flat_map(|section| section.children())
        .filter(|node| node.has_tag_name(record_tag))
    {
        object_count += 1;
        if let Some((domain, _)) = node.attribute("type").and_then(|name| name.split_once("::")) {
            if !domain_set.contains(domain) {
                ctx.charge_collection_items(1, "FCStd document domains")?;
                domain_set.insert(retained_string(ctx, domain, "FCStd document domain name")?);
            }
        }
    }
    let mut domains = collection_vec(ctx, domain_set.len(), "FCStd document domain list")?;
    domains.extend(domain_set);
    let document = DocumentFacts {
        id: crate::native::native_id("document", "0"),
        file_version,
        program_version: canonical_attribute(ctx, root, "ProgramVersion", "programVersion")?,
        root_name: root.tag_name().name().into(),
        object_count,
        domains,
    };
    Ok((document, schema_version))
}

pub(crate) fn logical_ledger(
    ctx: &DecodeContext<'_>,
    entries: &[EntryRecord],
    properties: &[PropertyRecord],
    gui: &gui::Graph,
    shape_payloads: &[ShapePayloadRecord],
    string_tables: &[StringTableRecord],
    element_maps: &[ElementMapRecord],
) -> Result<Vec<LogicalSpan>, CodecError> {
    let mut typed_entries = HashSet::new();
    for payload in shape_payloads {
        insert_hash_set(ctx, &mut typed_entries, payload.entry.as_str(), "FCStd typed entry identities")?;
    }
    for name in string_tables.iter().filter_map(|table| table.source_entry.as_deref()) {
        insert_hash_set(ctx, &mut typed_entries, name, "FCStd typed entry identities")?;
    }
    for name in element_maps.iter().filter_map(|map| map.source_entry.as_deref()) {
        insert_hash_set(ctx, &mut typed_entries, name, "FCStd typed entry identities")?;
    }
    let mut output = Vec::new();
    for entry in entries {
        if typed_entries.contains(entry.id.as_str()) || typed_entries.contains(entry.name.as_str())
        {
            push_logical_span(
                ctx,
                &mut output,
                entry,
                0,
                entry.byte_len(),
                LogicalClassification::Typed {
                    owner: retained_string(ctx, &entry.id, "FCStd logical span owner")?,
                },
            )?;
        } else if entry.name == "Document.xml" || entry.name == "GuiDocument.xml" {
            let mut ranges = Vec::new();
            if entry.name == "Document.xml" {
                for property in properties {
                    reserve_vec_items(ctx, &mut ranges, 1, "FCStd logical property ranges")?;
                    ranges.push((
                        property.xml.start(),
                        property.xml.end(),
                        if property.family == PropertyFamily::Unknown { "named_opaque" } else { "typed" },
                        retained_string(ctx, &property.id, "FCStd logical range owner")?,
                    ));
                }
            } else {
                for property in &gui.properties {
                    reserve_vec_items(ctx, &mut ranges, 1, "FCStd logical GUI ranges")?;
                    ranges.push((
                        property.xml.start(),
                        property.xml.end(),
                        if gui::has_registered_property_grammar(&property.name, &property.type_name) { "typed" } else { "named_opaque" },
                        retained_string(ctx, &property.id, "FCStd logical range owner")?,
                    ));
                }
                for document in &gui.documents {
                    for state in &document.states {
                        reserve_vec_items(ctx, &mut ranges, 1, "FCStd logical GUI ranges")?;
                        ranges.push((
                            state.xml.start(),
                            state.xml.end(),
                            "typed",
                            retained_string(ctx, &state.id, "FCStd logical range owner")?,
                        ));
                    }
                }
            }
            ranges.sort_by_key(|range| range.0);
            let mut cursor = 0_u64;
            for (start, end, classification, owner) in ranges {
                if start < cursor || end < start || end > entry.byte_len() {
                    return Err(CodecError::malformed(format_args!(
                        "overlapping or invalid {} record spans",
                        entry.name
                    )));
                }
                push_logical_span(
                    ctx,
                    &mut output,
                    entry,
                    cursor,
                    start,
                    LogicalClassification::Structural,
                )?;
                let classification = match classification {
                    "typed" => LogicalClassification::Typed { owner },
                    _ => LogicalClassification::NamedOpaque { owner },
                };
                push_logical_span(ctx, &mut output, entry, start, end, classification)?;
                cursor = end;
            }
            push_logical_span(
                ctx,
                &mut output,
                entry,
                cursor,
                entry.byte_len(),
                LogicalClassification::Structural,
            )?;
        } else {
            push_logical_span(
                ctx,
                &mut output,
                entry,
                0,
                entry.byte_len(),
                LogicalClassification::NamedOpaque {
                    owner: retained_string(ctx, &entry.id, "FCStd logical span owner")?,
                },
            )?;
        }
    }
    Ok(output)
}

pub(crate) fn byte_coverage(
    ctx: &DecodeContext<'_>,
    physical: &[ArchiveSpan],
    entries: &[EntryRecord],
    logical: &[LogicalSpan],
    physical_byte_len: u64,
) -> Result<ByteCoverageRecord, CodecError> {
    let mut classification_bytes = BTreeMap::new();
    let mut named_opaque_entries = BTreeSet::new();
    for span in logical {
        let classification = span.classification.as_str();
        if !classification_bytes.contains_key(classification) {
            ctx.charge_collection_items(1, "FCStd coverage classifications")?;
            classification_bytes.insert(retained_string(ctx, classification, "FCStd coverage classification name")?, 0);
        }
        if let Some(bytes) = classification_bytes.get_mut(classification) {
            *bytes += span.span.end() - span.span.start();
        }
        if matches!(
            span.classification,
            LogicalClassification::NamedOpaque { .. }
        ) {
            if !named_opaque_entries.contains(&span.entry) {
                ctx.charge_collection_items(1, "FCStd opaque coverage entries")?;
                named_opaque_entries.insert(retained_string(ctx, &span.entry, "FCStd opaque entry name")?);
            }
        }
    }
    let mut ordered_physical = collection_vec(ctx, physical.len(), "FCStd ordered physical spans")?;
    ordered_physical.extend(physical.iter());
    ordered_physical.sort_by_key(|span| span.span.start());
    let physical_exact = ordered_physical
        .first()
        .is_some_and(|span| span.span.start() == 0)
        && ordered_physical
            .windows(2)
            .all(|pair| pair[0].span.end() == pair[1].span.start())
        && ordered_physical
            .last()
            .is_some_and(|span| span.span.end() == physical_byte_len);
    let mut logical_exact = logical
        .iter()
        .all(|span| entries.iter().any(|entry| entry.name == span.entry));
    if logical_exact {
        for entry in entries {
            let mut spans = Vec::new();
            for span in logical.iter().filter(|span| span.entry == entry.name) {
                reserve_vec_items(ctx, &mut spans, 1, "FCStd entry logical spans")?;
                spans.push(span);
            }
            spans.sort_by_key(|span| span.span.start());
            let exact = if entry.byte_len() == 0 {
                spans.is_empty()
            } else {
                spans.first().is_some_and(|span| span.span.start() == 0)
                    && spans
                        .windows(2)
                        .all(|pair| pair[0].span.end() == pair[1].span.start())
                    && spans
                    .last()
                    .is_some_and(|span| span.span.end() == entry.byte_len())
            };
            if !exact {
                logical_exact = false;
                break;
            }
        }
    }
    let mut opaque_entries = collection_vec(ctx, named_opaque_entries.len(), "FCStd opaque coverage list")?;
    opaque_entries.extend(named_opaque_entries);
    Ok(ByteCoverageRecord {
        id: crate::native::native_id("byte-coverage", "0"),
        physical_byte_len,
        physical_span_count: physical.len(),
        logical_entry_count: entries.len(),
        logical_byte_len: entries
            .iter()
            .map(super::native::EntryRecord::byte_len)
            .sum(),
        logical_span_count: logical.len(),
        classification_bytes,
        named_opaque_entries: opaque_entries,
        exact: physical_exact && logical_exact,
    })
}

fn push_logical_span(
    ctx: &DecodeContext<'_>,
    output: &mut Vec<LogicalSpan>,
    entry: &EntryRecord,
    start: u64,
    end: u64,
    classification: LogicalClassification,
) -> Result<(), CodecError> {
    if start == end {
        return Ok(());
    }
    reserve_vec_items(ctx, output, 1, "FCStd logical ledger spans")?;
    output.push(LogicalSpan {
        id: crate::native::native_id("logical-span", output.len().to_string()),
        entry: retained_string(ctx, &entry.name, "FCStd logical span entry")?,
        span: crate::native::ByteSpan::try_new(start, end).map_err(CodecError::Malformed)?,
        classification,
    });
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
