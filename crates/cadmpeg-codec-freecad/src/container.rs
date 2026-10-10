// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::tree::AdmittedXml;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::ContainerSummary;

use crate::brep::ShapePayloadRecord;
use crate::gui;
use crate::native::element_map::ElementMapRecord;
use crate::native::{
    ArchiveSpan, ByteCoverageRecord, DocumentFacts, EntryRecord, LogicalClassification,
    LogicalSpan, PropertyFamily, PropertyRecord, StringTableRecord,
};

const DETECTION_XML_BYTES: usize = 8 * 1024;

/// Inspect the first local entry deeply enough to confirm `FCStd` document markers.
pub(crate) fn has_document_markers(
    ctx: &DecodeContext<'_>,
    prefix: &[u8],
) -> Result<bool, CodecError> {
    if prefix.len() < 30 || &prefix[..4] != b"PK\x03\x04" {
        return Ok(false);
    }
    let Some(method) = View::u16_le_at(prefix, 8) else {
        return Ok(false);
    };
    let Some(name_len) = View::u16_le_at(prefix, 26).map(usize::from) else {
        return Ok(false);
    };
    let Some(extra_len) = View::u16_le_at(prefix, 28).map(usize::from) else {
        return Ok(false);
    };
    let Some(name_end) = 30_usize.checked_add(name_len) else {
        return Ok(false);
    };
    let Some(data_start) = name_end.checked_add(extra_len) else {
        return Ok(false);
    };
    if name_end > prefix.len()
        || data_start > prefix.len()
        || &prefix[30..name_end] != b"Document.xml"
    {
        return Ok(false);
    }
    let compressed = &prefix[data_start..];
    let inflated;
    let document = match method {
        0 => compressed,
        8 => {
            inflated =
                ctx.inflate_probe(View::over_retained(compressed), DETECTION_XML_BYTES, false)?;
            let Some((ref output, _)) = inflated else {
                return Ok(false);
            };
            output.as_slice()
        }
        _ => return Ok(false),
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(document.len()) * 2,
        "scan FreeCAD probe XML",
    )?;
    Ok(contains(document, b"<Document") && contains(document, b"SchemaVersion"))
}

/// Fully scanned container used by inspection and decode.
pub(crate) struct Scan<'a, 'ctx> {
    /// Recoverable container metadata diagnostics.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Container summary entries.
    pub(crate) entries: Vec<ContainerEntry>,
    /// Persistence metadata.
    pub(crate) document: DocumentFacts,
    /// Declared persistence schema version, owned by the source declaration.
    pub(crate) schema_version: String,
    /// One admitted persistence tree shared by all document readers.
    pub(crate) document_xml: AdmittedXml<'a, 'ctx>,
    /// Exact physical archive partition.
    pub(crate) ledger: Vec<ArchiveSpan>,
    /// Bounded physical payloads that could not be opened.
    pub(crate) unreadable_entries: Vec<UnreadableEntry>,
    /// Inflated entry views, each retaining its [`SpaceId`](cadmpeg_core::decode::SpaceId).
    pub(crate) data: BTreeMap<String, View<'a>>,
}

/// A source-only ZIP payload. Its bytes are stored bytes, never expanded data.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnreadableEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) data_start: u64,
    pub(crate) data_end: u64,
    #[serde(deserialize_with = "crate::native::deserialize_lowercase_native_bytes")]
    pub(crate) stored_data: NativeBytes,
    pub(crate) error: String,
}

/// Scan an archive through the session resource budget.
pub(crate) fn scan<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'a>,
    root: View<'a>,
) -> Result<Scan<'a, 'ctx>, CodecError> {
    let archive = ArchiveSnapshot::new(ctx, root)?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(archive.entries().len()),
        "fcstd ZIP entries",
    )?;
    if !archive
        .entries()
        .iter()
        .any(|file| file.name == "Document.xml")
    {
        return Err(CodecError::WrongFormat(
            "ZIP has no root Document.xml".into(),
        ));
    }
    let document_view = archive.open(ctx, "Document.xml")?;
    let document_bytes = document_view.window();
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(document_bytes.len()),
        "FCStd Document.xml lexical admission",
    )?;
    if let Some((node_count, object_count)) = xml_envelope_counts(document_bytes) {
        ctx.charge_entities(object_count, "admit FCStd document objects")?;
        ctx.charge_collection_items(node_count, "FCStd Document.xml node tree")?;
    }
    let mut losses = Vec::new();
    let document_xml = admit_document(ctx, document_bytes)?;
    let (document, schema_version) = parse_document(ctx, document_xml.document(), &mut losses)?;
    let mut data = BTreeMap::new();
    let mut unreadable_entries = Vec::new();
    for file in archive.entries() {
        let name = ctx.copy_retained_text(&file.name, "FCStd archive entry name")?;
        if !crate::native::is_safe_entry_name(&name) {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("unsafe ZIP entry path {name:?}"),
                "FCStd unsafe entry name diagnostic",
            ));
        }
        let view = if file.name == "Document.xml" {
            document_view
        } else {
            match archive.open(ctx, &file.name) {
                Ok(view) => view,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    let range = file.stored_range()?;
                    let end = range.end;
                    let start_index = usize::try_from(range.start)
                        .map_err(|_| CodecError::malformed("ZIP data offset exceeds memory"))?;
                    let end_index = usize::try_from(end)
                        .map_err(|_| CodecError::malformed("ZIP data offset exceeds memory"))?;
                    let bytes = root
                        .window()
                        .get(start_index..end_index)
                        .ok_or_else(|| CodecError::malformed("ZIP payload escapes archive"))?;
                    ctx.push_vec(
                        &mut losses,
                        crate::loss::FreecadLossCode::ArchiveEntryUnreadable.note(
                            ctx.format_retained(
                                format_args!(
                                    "{} cannot be opened: {error}; stored payload retained",
                                    file.name
                                ),
                                "FCStd unreadable entry loss",
                            )?,
                        ),
                        "FCStd unreadable entry losses",
                    )?;
                    ctx.push_vec(
                        &mut unreadable_entries,
                        UnreadableEntry {
                            id: ctx.format_retained(
                                format_args!("fcstd:native:unreadable_entry#{name}"),
                                "FCStd unreadable entry identity",
                            )?,
                            name,
                            data_start: range.start,
                            data_end: end,
                            stored_data: ctx
                                .copy_retained(bytes, "FCStd unreadable stored payload")?
                                .into(),
                            error: ctx.format_retained(
                                format_args!("{error}"),
                                "FCStd unreadable entry diagnostic",
                            )?,
                        },
                        "FCStd unreadable entries",
                    )?;
                    continue;
                }
            }
        };
        ctx.insert_btree_map(&mut data, name, view, "FCStd archive entry map")?;
    }
    let physical_ledger = archive.physical_ledger(ctx)?;
    let mut ledger = ctx.collection_vec(physical_ledger.len(), "FCStd archive ledger records")?;
    for (index, span) in physical_ledger.into_iter().enumerate() {
        ledger.push(ArchiveSpan {
            id: crate::native::native_id_charged(ctx, "archive-span", &index.to_string())?,
            span: crate::native::ByteSpan::try_new(span.start, span.end)
                .map_err(CodecError::Malformed)?,
            role: crate::native::ArchiveSpanRole::from(&span.role),
        });
    }
    Ok(Scan {
        losses,
        entries: archive.container_entries(ctx, classify)?,
        document,
        schema_version,
        document_xml,
        unreadable_entries,
        ledger,
        data,
    })
}

pub(crate) fn entry_records(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_, '_>,
    properties: &[PropertyRecord],
) -> Result<Vec<EntryRecord>, CodecError> {
    let mut records = ctx.collection_vec(scan.entries.len(), "FCStd entry records")?;
    let mut name_storage = ctx.reserve_scoped(0, "FCStd unreadable entry lookup")?;
    let mut unreadable_names = HashSet::new();
    for entry in &scan.unreadable_entries {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entry.name.len()),
            "FCStd unreadable entry name hashing",
        )?;
        name_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut unreadable_names,
                entry.name.as_str(),
                "FCStd unreadable entry lookup",
            )
        })?;
    }
    let (references, _reference_storage) =
        ctx.with_scoped_storage("FCStd entry reference index", || {
            let mut references = HashMap::<&str, Vec<usize>>::new();
            for (index, property) in properties.iter().enumerate() {
                ctx.charge_work(1, "FCStd entry reference index construction")?;
                for name in property.side_entries() {
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(name.len()).max(1),
                        "FCStd entry reference index construction",
                    )?;
                    if !references.contains_key(name.as_str()) {
                        ctx.reserve_map(&mut references, 1, "FCStd entry reference index")?;
                    }
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(name.len()).max(1),
                        "FCStd entry reference index construction",
                    )?;
                    let owners = references.entry(name.as_str()).or_default();
                    // Repeated side-entry markers in one property yield one owner.
                    if owners.last() != Some(&index) {
                        ctx.reserve_vec(owners, 1, "FCStd entry reference index")?;
                        owners.push(index);
                    }
                }
            }
            Ok::<_, CodecError>(references)
        })?;
    for entry in &scan.entries {
        let Some(bytes) = scan.data.get(&entry.name).map(|view| view.window()) else {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(entry.name.len()),
                "FCStd unreadable entry lookup hashing",
            )?;
            if unreadable_names.contains(entry.name.as_str()) {
                // Stored source-only bytes have no expanded-byte coverage.
                continue;
            }
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("entry {} disappeared after scan", entry.name),
                "FCStd missing entry error",
            )?));
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entry.name.len()).max(1),
            "FCStd entry reference index lookup",
        )?;
        let owners = references
            .get(entry.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut referenced_by =
            ctx.collection_vec(owners.len(), "FCStd entry referencing properties")?;
        for &index in owners {
            ctx.charge_work(1, "FCStd entry reference index lookup")?;
            referenced_by.push(
                ctx.copy_retained_text(&properties[index].id, "FCStd entry referencing identity")?,
            );
        }
        records.push(EntryRecord::new(
            ctx,
            crate::native::native_id_charged(ctx, "entry", &entry.name)?,
            ctx.copy_retained_text(&entry.name, "FCStd entry record name")?,
            entry.role,
            referenced_by,
            ctx.copy_retained(bytes, "retain FCStd entry")?,
        )?);
    }
    Ok(records)
}

pub(crate) fn source_attributes(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_, '_>,
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan.document.root_name.len()),
        "FCStd source root",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan.document.document_kind().as_str().len()),
        "FCStd source kind",
    )?;
    for (index, domain) in scan.document.domains.iter().enumerate() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(domain.len()),
            "FCStd source domain list",
        )?;
        if index != 0 {
            ctx.charge_work(1, "FCStd source domain list")?;
        }
    }
    if let Some(version) = &scan.document.program_version {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(version.len()),
            "FCStd source program version",
        )?;
    }
    let mut attributes = BTreeMap::new();
    for (key, value) in [
        (
            "document_root",
            ctx.copy_retained_text(&scan.document.root_name, "FCStd source root")?,
        ),
        (
            "object_count",
            ctx.format_retained(
                format_args!("{}", scan.document.object_count),
                "FCStd source object count",
            )?,
        ),
        (
            "document_kind",
            ctx.copy_retained_text(scan.document.document_kind().as_str(), "FCStd source kind")?,
        ),
        (
            "application_domains",
            ctx.join_retained(&scan.document.domains, ",", "FCStd source domain list")?,
        ),
        (
            "archive_entry_count",
            ctx.format_retained(
                format_args!("{}", scan.entries.len()),
                "FCStd source entry count",
            )?,
        ),
        (
            "physical_ledger_spans",
            ctx.format_retained(
                format_args!("{}", scan.ledger.len()),
                "FCStd source ledger spans",
            )?,
        ),
    ]
    .into_iter()
    .chain(
        scan.ledger
            .last()
            .map(|last| {
                Ok::<_, CodecError>((
                    "physical_archive_bytes",
                    ctx.format_retained(
                        format_args!("{}", last.span.end()),
                        "FCStd source archive bytes",
                    )?,
                ))
            })
            .transpose()?,
    )
    .chain(
        scan.document
            .program_version
            .as_ref()
            .map(|value| {
                Ok::<_, CodecError>((
                    "program_version",
                    ctx.copy_retained_text(value, "FCStd source program version")?,
                ))
            })
            .transpose()?,
    ) {
        ctx.admit_retained_btree_record::<NonBlankString, String>(
            0,
            "FCStd source attribute records",
        )?;
        let key = NonBlankString::new(ctx.copy_retained_text(key, "FCStd source attribute key")?)
            .ok_or_else(|| CodecError::malformed("source attribute key is empty"))?;
        attributes.insert(key, value);
    }
    Ok(attributes)
}

/// Summarize one scan.
pub(crate) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_, '_>,
) -> Result<ContainerSummary, CodecError> {
    let matched = crate::dialect::FcstdDialect::classify(&scan.document, &scan.schema_version);
    let mut losses = Vec::new();
    for loss in scan
        .losses
        .iter()
        .chain(crate::dialect::FcstdDialect::dialect_loss(&matched).iter())
    {
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(loss.message.len()),
            "FCStd summary loss message",
        )?;
        ctx.push_vec(&mut losses, loss.clone(), "FCStd summary losses")?;
    }
    let mut entries = ctx.collection_vec(scan.entries.len(), "FCStd summary entries")?;
    for entry in &scan.entries {
        let mut attributes = BTreeMap::new();
        for (key, value) in &entry.attributes {
            ctx.insert_btree_map(
                &mut attributes,
                ctx.copy_retained_text(key, "FCStd summary attribute key")?,
                ctx.copy_retained_text(value, "FCStd summary attribute value")?,
                "FCStd summary entry attributes",
            )?;
        }
        entries.push(ContainerEntry {
            name: ctx.copy_retained_text(&entry.name, "FCStd summary entry name")?,
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
pub(crate) fn summary_notes(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_, '_>,
) -> Result<Vec<String>, CodecError> {
    let mut notes = ctx.collection_vec(
        6 + usize::from(scan.document.program_version.is_some()),
        "FCStd summary notes",
    )?;
    notes.extend([
        ctx.format_retained(
            format_args!("SchemaVersion={}", scan.schema_version),
            "FCStd schema note",
        )?,
        ctx.format_retained(
            format_args!("FileVersion={}", scan.document.file_version.as_str()),
            "FCStd file version note",
        )?,
        ctx.format_retained(
            format_args!("document root={}", scan.document.root_name),
            "FCStd document root note",
        )?,
        ctx.format_retained(
            format_args!("document kind={}", scan.document.document_kind().as_str()),
            "FCStd document kind note",
        )?,
        ctx.format_retained(
            format_args!("object count={}", scan.document.object_count),
            "FCStd object count note",
        )?,
        ctx.format_retained(
            format_args!("physical ledger spans={} coverage=exact", scan.ledger.len()),
            "FCStd physical ledger note",
        )?,
    ]);
    if let Some(version) = &scan.document.program_version {
        notes.push(ctx.format_retained(
            format_args!("ProgramVersion={version}"),
            "FCStd program version note",
        )?);
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
        (Some(value), None) => Ok(Some(
            ctx.copy_retained_text(value, "FCStd document attribute")?,
        )),
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
    let mut sections = root.children().filter(|node| node.has_tag_name(tag));
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

/// Admit the persistence tree once and keep its storage charged while it is borrowed.
pub(crate) fn admit_document<'input, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'input [u8],
) -> Result<AdmittedXml<'input, 'ctx>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "validate FreeCAD XML UTF-8",
    )?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let admitted_xml = match ctx.parse_xml(text, "FreeCAD document XML tree") {
        Ok(xml) => xml,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            let CodecError::Malformed(error) = error else {
                return Err(error);
            };
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("invalid Document.xml: {error}"),
                "FCStd document parse error",
            )?));
        }
    };
    Ok(admitted_xml)
}

/// Read container metadata from the admitted persistence tree.
pub(crate) fn parse_document(
    ctx: &DecodeContext<'_>,
    xml: &roxmltree::Document<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(DocumentFacts, String), CodecError> {
    let root = xml.root_element();
    if root.tag_name().name() != "Document" {
        return Err(CodecError::WrongFormat(ctx.format_retained(
            format_args!(
                "Document.xml root is {}, expected Document",
                root.tag_name().name()
            ),
            "FCStd document root error",
        )?));
    }
    let schema_version = canonical_attribute(ctx, root, "SchemaVersion", "schemaVersion")?
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no SchemaVersion".into()))?;
    let file_version = metadata_attribute(
        ctx,
        root,
        "FileVersion",
        "fileVersion",
        crate::loss::FreecadLossCode::FileVersionNoncanonical,
        losses,
    )?
    .unwrap_or_else(|| "0".into());
    let vocabulary = crate::persistence::Vocabulary::from_declaration(&schema_version)?;
    let file_version = crate::native::FileVersion::from(file_version);
    if file_version.value().is_none() {
        ctx.push_vec(losses, crate::loss::FreecadLossCode::FileVersionUnverified.note(
            ctx.format_retained(format_args!("Document FileVersion {:?} is not an unsigned integer; version-dependent inline element maps remain source-only", file_version.as_str()), "FCStd file version diagnostic")?
        ), "FCStd container losses")?;
    }
    let (declaration_tag, data_tag, record_tag) = vocabulary.tags();
    // discarded-value: the data section's uniqueness is the check; ? states its refusal and the node has no reader
    let _ = unique_section(root, data_tag)?;
    let declarations = unique_section(root, declaration_tag)?;
    let mut object_count = 0;
    let mut domain_set = BTreeSet::new();
    for node in declarations
        .into_iter()
        .flat_map(|section| section.children())
        .filter(|node| node.has_tag_name(record_tag))
    {
        object_count += 1;
        if let Some((domain, _)) = node
            .attribute("type")
            .and_then(|name| name.split_once("::"))
        {
            if !domain_set.contains(domain) {
                ctx.insert_btree_set(
                    &mut domain_set,
                    ctx.copy_retained_text(domain, "FCStd document domain name")?,
                    "FCStd document domains",
                )?;
            }
        }
    }
    let mut domains = ctx.collection_vec(domain_set.len(), "FCStd document domain list")?;
    domains.extend(domain_set);
    let program_version = metadata_attribute(
        ctx,
        root,
        "ProgramVersion",
        "programVersion",
        crate::loss::FreecadLossCode::ProgramVersionNoncanonical,
        losses,
    )?;
    let document = DocumentFacts {
        id: crate::native::native_id("document", "0"),
        file_version,
        program_version,
        root_name: root.tag_name().name().into(),
        object_count,
        domains,
    };
    Ok((document, schema_version))
}

/// Canonical metadata controls interpretation when a noncanonical alias conflicts.
fn metadata_attribute(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'_, '_>,
    canonical: &str,
    alias: &str,
    code: crate::loss::FreecadLossCode,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<String>, CodecError> {
    let value = root.attribute(canonical);
    if let Some(alias_value) = root.attribute(alias) {
        let message = if value.is_some_and(|value| value != alias_value) {
            ctx.format_retained(format_args!("Document has conflicting {canonical} and {alias} declarations; retaining canonical {canonical}"), "FCStd metadata diagnostic")?
        } else {
            ctx.format_retained(
                format_args!("Document uses {alias} metadata; read as {canonical}"),
                "FCStd metadata diagnostic",
            )?
        };
        ctx.push_vec(losses, code.note(message), "FCStd container losses")?;
    }
    value
        .or_else(|| root.attribute(alias))
        .map(|value| ctx.copy_retained_text(value, "FCStd version metadata"))
        .transpose()
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
        ctx.insert_hash_set(
            &mut typed_entries,
            payload.entry.as_str(),
            "FCStd typed entry identities",
        )?;
    }
    for name in string_tables
        .iter()
        .filter_map(|table| table.source_entry.as_deref())
    {
        ctx.insert_hash_set(&mut typed_entries, name, "FCStd typed entry identities")?;
    }
    for name in element_maps
        .iter()
        .filter_map(|map| map.source_entry.as_deref())
    {
        ctx.insert_hash_set(&mut typed_entries, name, "FCStd typed entry identities")?;
    }
    let mut output = Vec::new();
    for entry in entries {
        if typed_entries.contains(entry.id()) || typed_entries.contains(entry.name()) {
            push_logical_span(
                ctx,
                &mut output,
                entry,
                0,
                entry.byte_len(),
                LogicalClassification::Typed {
                    owner: ctx.copy_retained_text(entry.id(), "FCStd logical span owner")?,
                },
            )?;
        } else if entry.name() == "Document.xml" || entry.name() == "GuiDocument.xml" {
            let mut ranges = Vec::new();
            if entry.name() == "Document.xml" {
                for property in properties {
                    ctx.reserve_vec(&mut ranges, 1, "FCStd logical property ranges")?;
                    ranges.push((
                        property.xml.start(),
                        property.xml.end(),
                        if property.family == PropertyFamily::Unknown {
                            "named_opaque"
                        } else {
                            "typed"
                        },
                        ctx.copy_retained_text(&property.id, "FCStd logical range owner")?,
                    ));
                }
            } else {
                for property in &gui.properties {
                    ctx.reserve_vec(&mut ranges, 1, "FCStd logical GUI ranges")?;
                    ranges.push((
                        property.xml.start(),
                        property.xml.end(),
                        if gui::has_registered_property_grammar(&property.name, &property.type_name)
                        {
                            "typed"
                        } else {
                            "named_opaque"
                        },
                        ctx.copy_retained_text(&property.id, "FCStd logical range owner")?,
                    ));
                }
                for document in &gui.documents {
                    for state in &document.states {
                        ctx.reserve_vec(&mut ranges, 1, "FCStd logical GUI ranges")?;
                        ranges.push((
                            state.xml.start(),
                            state.xml.end(),
                            "typed",
                            ctx.copy_retained_text(&state.id, "FCStd logical range owner")?,
                        ));
                    }
                }
            }
            ctx.stable_sort_by(
                &mut ranges,
                |left, right| left.0.cmp(&right.0),
                |_| 0,
                "FCStd logical GUI range sort",
            )?;
            let mut cursor = 0_u64;
            for (start, end, classification, owner) in ranges {
                if start < cursor || end < start || end > entry.byte_len() {
                    return Err(CodecError::Malformed(ctx.format_retained(
                        format_args!("overlapping or invalid {} record spans", entry.name()),
                        "FCStd logical span error",
                    )?));
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
                    owner: ctx.copy_retained_text(entry.id(), "FCStd logical span owner")?,
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
            ctx.insert_btree_map(
                &mut classification_bytes,
                ctx.copy_retained_text(classification, "FCStd coverage classification name")?,
                0,
                "FCStd coverage classifications",
            )?;
        }
        if let Some(bytes) = classification_bytes.get_mut(classification) {
            *bytes += span.span.end() - span.span.start();
        }
        if matches!(
            span.classification,
            LogicalClassification::NamedOpaque { .. }
        ) && !named_opaque_entries.contains(&span.entry)
        {
            ctx.insert_btree_set(
                &mut named_opaque_entries,
                ctx.copy_retained_text(&span.entry, "FCStd opaque entry name")?,
                "FCStd opaque coverage entries",
            )?;
        }
    }
    let mut ordered_physical =
        ctx.collection_vec(physical.len(), "FCStd ordered physical spans")?;
    ordered_physical.extend(physical.iter());
    ctx.stable_sort_by(
        &mut ordered_physical,
        |left, right| left.span.start().cmp(&right.span.start()),
        |_| 0,
        "FCStd physical span sort",
    )?;
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
        .all(|span| entries.iter().any(|entry| entry.name() == span.entry));
    if logical_exact {
        for entry in entries {
            let mut spans = Vec::new();
            for span in logical.iter().filter(|span| span.entry == entry.name()) {
                ctx.reserve_vec(&mut spans, 1, "FCStd entry logical spans")?;
                spans.push(span);
            }
            ctx.stable_sort_by(
                &mut spans,
                |left, right| left.span.start().cmp(&right.span.start()),
                |_| 0,
                "FCStd entry logical span sort",
            )?;
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
    let mut opaque_entries =
        ctx.collection_vec(named_opaque_entries.len(), "FCStd opaque coverage list")?;
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
    ctx.reserve_vec(output, 1, "FCStd logical ledger spans")?;
    output.push(LogicalSpan {
        id: crate::native::native_id("logical-span", output.len().to_string()),
        entry: ctx.copy_retained_text(entry.name(), "FCStd logical span entry")?,
        span: crate::native::ByteSpan::try_new(start, end).map_err(CodecError::Malformed)?,
        classification,
    });
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
