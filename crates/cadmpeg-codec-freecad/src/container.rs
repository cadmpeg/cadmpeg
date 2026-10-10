// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::tree::AdmittedXml;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::brep::ShapePayloadRecord;
use crate::gui;
use crate::native::element_map::{ElementMapRecord, ScopedData};
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
    Ok(
        ctx.position_by(
            document.windows(b"<Document".len()),
            |window| Ok(window == b"<Document"),
            "scan FreeCAD probe XML",
        )?.is_some()
            && ctx.position_by(
                document.windows(b"SchemaVersion".len()),
                |window| Ok(window == b"SchemaVersion"),
                "scan FreeCAD probe XML",
            )?.is_some(),
    )
}

/// Fully scanned container used by inspection and decode.
pub(crate) struct Scan<'a, 'c> {
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
    /// The parsed `Document.xml`, held so decode reads the one tree the scan
    /// built; decode takes it and drops it once persistence is read.
    pub(crate) document_xml: Option<DocumentXml<'a, 'c>>,
}

/// `Document.xml` text and its admitted tree.
pub(crate) struct DocumentXml<'a, 'c> {
    pub(crate) text: &'a str,
    pub(crate) xml: AdmittedXml<'a, 'c>,
}

/// Scan an archive through the session resource budget.
pub(crate) fn scan<'a, 'c>(
    ctx: &'c DecodeContext<'a>,
    root: View<'a>,
) -> Result<Scan<'a, 'c>, CodecError> {
    let archive = ArchiveSnapshot::new(ctx, root)?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(archive.entries().len()),
        "fcstd ZIP entries",
    )?;
    // The literal bounds each name comparison.
    if !ctx.any_by(
        archive.entries(),
        |file| Ok(file.name == "Document.xml"),
        "FCStd document entry search",
    )? {
        return Err(CodecError::WrongFormat(
            "ZIP has no root Document.xml".into(),
        ));
    }
    let document_view = archive.open(ctx, "Document.xml")?;
    let document_bytes = document_view.window();
    if let Some(object_count) = xml_object_count(ctx, document_bytes)? {
        ctx.charge_entities(object_count, "admit FCStd document objects")?;
    }
    let (document, schema_version, document_xml) = parse_document(ctx, document_bytes)?;
    let mut data = BTreeMap::new();
    let mut files = archive.entries().iter();
    while files.len() != 0 {
        let Some(file) = ctx.next_charged(&mut files, "FCStd archive entries")? else {
            break;
        };
        if !crate::native::is_safe_entry_name_charged(ctx, &file.name)? {
            return Err(crate::resource::malformed_charged(
                ctx,
                format_args!("unsafe ZIP entry path {:?}", file.name),
                "FCStd unsafe entry name diagnostic",
            ));
        }
        // The literal bounds the comparison.
        let view = if file.name == "Document.xml" {
            document_view
        } else {
            archive.open(ctx, &file.name)?
        };
        let name = ctx.copy_retained_text(&file.name, "FCStd archive entry name")?;
        ctx.insert_btree_map(&mut data, name, view, "FCStd archive entry map")?;
    }
    let physical_ledger = archive.physical_ledger(ctx)?;
    let mut ledger = ctx.collection_vec(physical_ledger.len(), "FCStd archive ledger records")?;
    let mut spans = physical_ledger.into_iter().enumerate();
    while spans.len() != 0 {
        let Some((index, span)) = ctx.next_charged(&mut spans, "FCStd archive ledger records")?
        else {
            break;
        };
        let id = ctx.format_retained(
            format_args!("fcstd:native:archive-span#{index}"),
            "FCStd archive span identity",
        )?;
        ledger.push(ArchiveSpan {
            id,
            span: crate::native::ByteSpan::try_new(span.start, span.end)
                .map_err(CodecError::Malformed)?,
            role: crate::native::ArchiveSpanRole::from(span.role),
        });
    }
    Ok(Scan {
        entries: archive.container_entries(ctx, classify)?,
        document,
        schema_version,
        ledger,
        data,
        document_xml: Some(document_xml),
    })
}

pub(crate) fn entry_records(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_, '_>,
    properties: &[PropertyRecord],
) -> Result<Vec<EntryRecord>, CodecError> {
    // One pass over the properties indexes each side entry's referencing
    // properties in property order; a property naming one entry twice is
    // listed once, and it is the last owner pushed when it repeats.
    let mut index_storage = ctx.reserve_scoped(0, "FCStd entry reference index")?;
    let mut referencing = HashMap::<&str, Vec<&str>>::new();
    let mut input_properties = properties.iter();
    while input_properties.len() != 0 {
        let Some(property) =
            ctx.next_charged(&mut input_properties, "FCStd entry reference index")?
        else {
            break;
        };
        let mut side_entries = property.side_entries().iter();
        while side_entries.len() != 0 {
            let Some(entry) = ctx.next_charged(&mut side_entries, "FCStd entry reference index")?
            else {
                break;
            };
            index_storage.with_storage(|| {
                let owners = ctx
                    .entry_hash_map(
                        &mut referencing,
                        entry.as_str(),
                        "FCStd entry reference index",
                    )?
                    .or_default();
                let repeated = match owners.last() {
                    Some(last) => {
                        ctx.equal(*last, property.id.as_str(), "FCStd entry reference index")?
                    }
                    None => false,
                };
                if !repeated {
                    ctx.push_vec(owners, property.id.as_str(), "FCStd entry reference index")?;
                }
                Ok::<(), CodecError>(())
            })?;
        }
    }
    let mut records = ctx.collection_vec(scan.entries.len(), "FCStd entry records")?;
    let mut entries = scan.entries.iter();
    while entries.len() != 0 {
        let Some(entry) = ctx.next_charged(&mut entries, "FCStd entry records")? else {
            break;
        };
        let Some(bytes) = ctx
            .get_btree_map(&scan.data, entry.name.as_str(), "FCStd archive entry map")?
            .map(|view| view.window())
        else {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("entry {} disappeared after scan", entry.name),
                "FCStd missing entry error",
            )?));
        };
        let owners = ctx
            .get_hash_map(
                &referencing,
                entry.name.as_str(),
                "FCStd entry reference index",
            )?
            .map_or(&[][..], Vec::as_slice);
        let mut referenced_by =
            ctx.collection_vec(owners.len(), "FCStd entry referencing properties")?;
        let mut owners_iter = owners.iter();
        while owners_iter.len() != 0 {
            let Some(&owner) =
                ctx.next_charged(&mut owners_iter, "FCStd entry referencing properties")?
            else {
                break;
            };
            referenced_by.push(ctx.copy_retained_text(owner, "FCStd entry referencing identity")?);
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
    const OPERATION: &str = "FCStd source attribute records";
    let mut attributes = BTreeMap::new();
    let mut insert = |key: NonBlankString, value: String| {
        ctx.insert_btree_map(&mut attributes, key, value, OPERATION)
            .map(drop)
    };
    insert(
        cadmpeg_core::nonblank_literal!("document_root"),
        ctx.copy_retained_text(&scan.document.root_name, "FCStd source root")?,
    )?;
    insert(
        cadmpeg_core::nonblank_literal!("object_count"),
        ctx.format_retained(
            format_args!("{}", scan.document.object_count),
            "FCStd source object count",
        )?,
    )?;
    insert(
        cadmpeg_core::nonblank_literal!("document_kind"),
        ctx.copy_retained_text(scan.document.document_kind_with_admission(ctx)?.as_str(), "FCStd source kind")?,
    )?;
    insert(
        cadmpeg_core::nonblank_literal!("application_domains"),
        ctx.join_retained(&scan.document.domains, ",", "FCStd source domain list")?,
    )?;
    insert(
        cadmpeg_core::nonblank_literal!("archive_entry_count"),
        ctx.format_retained(
            format_args!("{}", scan.entries.len()),
            "FCStd source entry count",
        )?,
    )?;
    insert(
        cadmpeg_core::nonblank_literal!("physical_ledger_spans"),
        ctx.format_retained(
            format_args!("{}", scan.ledger.len()),
            "FCStd source ledger spans",
        )?,
    )?;
    if let Some(last) = scan.ledger.last() {
        insert(
            cadmpeg_core::nonblank_literal!("physical_archive_bytes"),
            ctx.format_retained(
                format_args!("{}", last.span.end()),
                "FCStd source archive bytes",
            )?,
        )?;
    }
    if let Some(value) = &scan.document.program_version {
        insert(
            cadmpeg_core::nonblank_literal!("program_version"),
            ctx.copy_retained_text(value, "FCStd source program version")?,
        )?;
    }
    Ok(attributes)
}

/// Summarize one scan, moving its entry summaries into the result.
pub(crate) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: Scan<'_, '_>,
) -> Result<ContainerSummary, CodecError> {
    let matched =
        crate::dialect::FcstdDialect::classify(ctx, &scan.document, &scan.schema_version)?;
    let losses = crate::dialect::FcstdDialect::dialect_loss(&matched)
        .into_iter()
        .collect();
    let notes = summary_notes(ctx, &scan)?;
    Ok(ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched),
        cadmpeg_ir::ContainerKind::Zip,
        scan.entries,
        losses,
        notes,
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
            format_args!("document kind={}", scan.document.document_kind_with_admission(ctx)?.as_str()),
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

/// The canonical attribute's text; the lower-case alias is refused.
pub(crate) fn canonical_attribute<'a>(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'a, '_>,
    canonical: &str,
    alias: &str,
) -> Result<Option<&'a str>, CodecError> {
    const OPERATION: &str = "FCStd document attribute search";
    match (
        ctx.xml_attribute(root, canonical, OPERATION)?,
        ctx.xml_attribute(root, alias, OPERATION)?,
    ) {
        (Some(_), Some(_)) => Err(CodecError::malformed(format_args!(
            "Document element has both {canonical} and {alias} attributes"
        ))),
        (Some(value), None) => Ok(Some(value)),
        (None, Some(_)) => Err(CodecError::malformed(format_args!(
            "Document element uses unsupported {alias}; expected {canonical}"
        ))),
        (None, None) => Ok(None),
    }
}

fn unique_section<'a, 'input>(
    ctx: &DecodeContext<'_>,
    root: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Result<Option<roxmltree::Node<'a, 'input>>, CodecError> {
    let mut selected = None;
    let mut children = root.children();
    while let Some(node) = ctx.next_charged(&mut children, "FCStd document section search")? {
        if ctx.xml_has_tag_name(node, tag, "FCStd document section search")?
            && selected.replace(node).is_some()
        {
            return Err(CodecError::malformed(format_args!(
                "Document.xml has duplicate {tag} sections"
            )));
        }
    }
    Ok(selected)
}

// This allocation-free lexical pass counts direct object declarations
// before roxmltree constructs any nodes. Syntax errors remain
// owned by the complete XML parser.
pub(crate) fn xml_object_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<u64>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut offset = 0;
    let mut depth = 0_usize;
    let mut objects = 0_u64;
    let mut envelope = None;
    while offset < bytes.len() {
        let Some(&byte) = ctx.next_charged(
            &mut bytes[offset..].iter(),
            "FCStd Document.xml lexical dispatch",
        )?
        else {
            break;
        };
        if byte != b'<' {
            let next = ctx
                .position_by(
                    &bytes[offset + 1..],
                    |byte| Ok(*byte == b'<'),
                    "FCStd Document.xml lexical text",
                )?
                .map_or(bytes.len(), |delta| offset + 1 + delta);
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
            let tail = &rest[prefix.len()..];
            let Some(end) = ctx.position_by(
                tail.windows(suffix.len()),
                |window| Ok(window == suffix),
                "FCStd Document.xml lexical markup suffix",
            )?
            else {
                return Ok(None);
            };
            offset += prefix.len() + end + suffix.len();
            continue;
        }
        if rest.starts_with(b"<!") {
            let Some((end, _)) = scan_tag_end(ctx, bytes, offset + 2)? else {
                return Ok(None);
            };
            offset = end + 1;
            continue;
        }
        let closing = rest.get(1) == Some(&b'/');
        let name_start = offset + if closing { 2 } else { 1 };
        let mut local_name_start = name_start;
        let name_end = ctx
            .position_by(
                bytes[name_start..].iter().enumerate(),
                |(index, byte)| {
                    if *byte == b':' {
                        local_name_start = name_start + index + 1;
                    }
                    Ok(byte.is_ascii_whitespace() || matches!(*byte, b'/' | b'>'))
                },
                "FCStd Document.xml lexical tag name",
            )?
            .map_or(bytes.len(), |delta| name_start + delta);
        if name_end == name_start {
            return Ok(None);
        }
        let Some((end, self_closing)) = scan_tag_end(ctx, bytes, name_end)? else {
            return Ok(None);
        };
        let name = &bytes[local_name_start..name_end];
        if closing {
            let Some(next_depth) = depth.checked_sub(1) else {
                return Ok(None);
            };
            depth = next_depth;
            if depth == 1 {
                envelope = None;
            }
        } else {
            if depth == 1 && (name == b"Objects" || name == b"Features") {
                envelope = Some(if name == b"Objects" {
                    b"Object".as_slice()
                } else {
                    b"Feature".as_slice()
                });
            } else if depth == 2 && envelope == Some(name) {
                let Some(count) = objects.checked_add(1) else {
                    return Ok(None);
                };
                objects = count;
            }
            if !self_closing {
                let Some(next_depth) = depth.checked_add(1) else {
                    return Ok(None);
                };
                depth = next_depth;
            } else if depth == 1 {
                envelope = None;
            }
        }
        offset = end + 1;
    }
    Ok((depth == 0).then_some(objects))
}

fn scan_tag_end(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<(usize, bool)>, CodecError> {
    let mut quote = None;
    let mut last_nonspace = None;
    let end = ctx.position_by(
        &bytes[start..],
        |&byte| {
            if quote.is_none() && byte == b'>' {
                return Ok(true);
            }
            if !byte.is_ascii_whitespace() {
                last_nonspace = Some(byte);
            }
            match (quote, byte) {
                (None, b'\'' | b'"') => quote = Some(byte),
                (Some(open), close) if open == close => quote = None,
                _ => {}
            }
            Ok(false)
        },
        "FCStd Document.xml lexical tag tail",
    )?;
    Ok(end.map(|offset| (start + offset, last_nonspace == Some(b'/'))))
}

/// Reads the document facts from `Document.xml` and returns its admitted tree
/// for decode to reuse.
pub(crate) fn parse_document<'a, 'c>(
    ctx: &'c DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<(DocumentFacts, String, DocumentXml<'a, 'c>), CodecError> {
    let text = ctx
        .validate_utf8(bytes, "validate FreeCAD XML UTF-8")?
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
    let xml = admitted_xml.document();
    let root = xml.root_element();
    // The literal bounds the comparison.
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
    ctx.parse_text::<u32>(schema_version, "FCStd schema version parse")?
        .map_err(|_| CodecError::Malformed("Document.xml SchemaVersion is invalid".into()))?;
    let schema_version = ctx.copy_retained_text(schema_version, "FCStd document attribute")?;
    let file_version = ctx.copy_retained_text(
        canonical_attribute(ctx, root, "FileVersion", "fileVersion")?.unwrap_or("0"),
        "FCStd document attribute",
    )?;
    // The shared validator parses the copied spelling once more.
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(file_version.len()),
        "FCStd file version parse",
    )?;
    let file_version =
        crate::native::FileVersion::try_from(file_version).map_err(CodecError::Malformed)?;
    let schema = crate::dialect::FcstdDialect::from_schema_version(&schema_version);
    let (declaration_tag, data_tag, record_tag) = schema.persistence_tags();
    // discarded-value: the data section's uniqueness is the check; ? states its refusal and the node has no reader
    let _ = unique_section(ctx, root, data_tag)?;
    let declarations = unique_section(ctx, root, declaration_tag)?;
    let mut object_count = 0_usize;
    let mut domain_set = BTreeSet::new();
    if let Some(section) = declarations {
        let mut children = section.children();
        while let Some(node) = ctx.next_charged(&mut children, "FCStd document declarations")? {
            if !ctx.xml_has_tag_name(node, record_tag, "FCStd document declarations")? {
                continue;
            }
            object_count = object_count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("FCStd document declarations", u64::MAX, u64::MAX)
            })?;
            let Some(type_name) = ctx.xml_attribute(node, "type", "FCStd document declarations")?
            else {
                continue;
            };
            let Some((domain, _)) =
                ctx.split_once(type_name, "::", "FCStd document domain name")?
            else {
                continue;
            };
            if !ctx.contains_btree_set(&domain_set, domain, "FCStd document domains")? {
                ctx.insert_btree_set(
                    &mut domain_set,
                    ctx.copy_retained_text(domain, "FCStd document domain name")?,
                    "FCStd document domains",
                )?;
            }
        }
    }
    let mut domains = ctx.collection_vec(domain_set.len(), "FCStd document domain list")?;
    domains.extend(ctx.admit_iter(domain_set, "FCStd document domain list")?);
    let program_version = canonical_attribute(ctx, root, "ProgramVersion", "programVersion")?
        .map(|value| ctx.copy_retained_text(value, "FCStd document attribute"))
        .transpose()?;
    let document = DocumentFacts {
        id: crate::native::native_id("document", "0"),
        file_version,
        program_version,
        // The root element name was checked equal to this literal.
        root_name: "Document".into(),
        object_count,
        domains,
    };
    Ok((
        document,
        schema_version,
        DocumentXml {
            text,
            xml: admitted_xml,
        },
    ))
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
    let mut typed_storage = ctx.reserve_scoped(0, "FCStd typed entry identities")?;
    let mut typed_entries = HashSet::new();
    let mut payloads = shape_payloads.iter();
    while payloads.len() != 0 {
        let Some(payload) = ctx.next_charged(&mut payloads, "FCStd typed entry identities")? else {
            break;
        };
        typed_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut typed_entries,
                payload.entry.as_str(),
                "FCStd typed entry identities",
            )
        })?;
    }
    let mut tables = string_tables.iter();
    while tables.len() != 0 {
        let Some(table) = ctx.next_charged(&mut tables, "FCStd typed entry identities")? else {
            break;
        };
        if let Some(name) = table.source_entry.as_deref() {
            typed_storage.with_storage(|| {
                ctx.insert_hash_set(&mut typed_entries, name, "FCStd typed entry identities")
            })?;
        }
    }
    let mut maps = element_maps.iter();
    while maps.len() != 0 {
        let Some(map) = ctx.next_charged(&mut maps, "FCStd typed entry identities")? else {
            break;
        };
        if let Some(name) = map.source_entry.as_deref() {
            typed_storage.with_storage(|| {
                ctx.insert_hash_set(&mut typed_entries, name, "FCStd typed entry identities")
            })?;
        }
    }
    let mut output = Vec::new();
    let mut entries_iter = entries.iter();
    while entries_iter.len() != 0 {
        let Some(entry) = ctx.next_charged(&mut entries_iter, "FCStd logical ledger entries")?
        else {
            break;
        };
        if ctx.contains_hash_set(&typed_entries, entry.id(), "FCStd typed entry identities")?
            || ctx.contains_hash_set(
                &typed_entries,
                entry.name(),
                "FCStd typed entry identities",
            )?
        {
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
            continue;
        }
        // The literals bound both comparisons.
        let is_document = entry.name() == "Document.xml";
        if !is_document && entry.name() != "GuiDocument.xml" {
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
            continue;
        }
        // Ranges borrow their owners; only the spans the ledger keeps copy them.
        let mut ranges_storage = ctx.reserve_scoped(0, "FCStd logical ranges")?;
        let mut ranges: Vec<(u64, u64, bool, &str)> = Vec::new();
        if is_document {
            let mut properties_iter = properties.iter();
            while properties_iter.len() != 0 {
                let Some(property) =
                    ctx.next_charged(&mut properties_iter, "FCStd logical property ranges")?
                else {
                    break;
                };
                ctx.push_scoped_vec(
                    &mut ranges_storage,
                    &mut ranges,
                    (
                        property.xml.start(),
                        property.xml.end(),
                        property.family != PropertyFamily::Unknown,
                        property.id.as_str(),
                    ),
                    "FCStd logical ranges",
                )?;
            }
        } else {
            let mut properties_iter = gui.properties.iter();
            while properties_iter.len() != 0 {
                let Some(property) =
                    ctx.next_charged(&mut properties_iter, "FCStd logical GUI ranges")?
                else {
                    break;
                };
                ctx.push_scoped_vec(
                    &mut ranges_storage,
                    &mut ranges,
                    (
                        property.xml.start(),
                        property.xml.end(),
                        gui::has_registered_property_grammar(&property.name, &property.type_name),
                        property.id.as_str(),
                    ),
                    "FCStd logical ranges",
                )?;
            }
            let mut documents = gui.documents.iter();
            while documents.len() != 0 {
                let Some(document) =
                    ctx.next_charged(&mut documents, "FCStd logical GUI ranges")?
                else {
                    break;
                };
                let mut states = document.states.iter();
                while states.len() != 0 {
                    let Some(state) = ctx.next_charged(&mut states, "FCStd logical GUI ranges")?
                    else {
                        break;
                    };
                    ctx.push_scoped_vec(
                        &mut ranges_storage,
                        &mut ranges,
                        (state.xml.start(), state.xml.end(), true, state.id.as_str()),
                        "FCStd logical ranges",
                    )?;
                }
            }
        }
        ctx.stable_sort_by(
            &mut ranges,
            |value| &value.0,
            Ord::cmp,
            "FCStd logical GUI range sort",
        )?;
        let mut cursor = 0_u64;
        let mut ranges_iter = ranges.iter();
        while ranges_iter.len() != 0 {
            let Some(&(start, end, typed, owner)) =
                ctx.next_charged(&mut ranges_iter, "FCStd logical ranges")?
            else {
                break;
            };
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
            let owner = ctx.copy_retained_text(owner, "FCStd logical range owner")?;
            let classification = if typed {
                LogicalClassification::Typed { owner }
            } else {
                LogicalClassification::NamedOpaque { owner }
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
    let overflow = || CodecError::malformed("FCStd byte coverage total overflows");
    // Logical spans grouped by entry once; each entry then takes its own.
    let mut groups_storage = ctx.reserve_scoped(0, "FCStd entry logical spans")?;
    let mut spans_by_entry = BTreeMap::<&str, ScopedData<'_, Vec<&LogicalSpan>>>::new();
    let mut opaque_names = BTreeSet::new();
    // Byte totals of the classifications present, by the wire order of their names.
    let mut totals: [Option<u64>; 3] = [None; 3];
    let mut logical_spans = logical.iter();
    while logical_spans.len() != 0 {
        let Some(span) = ctx.next_charged(&mut logical_spans, "FCStd entry logical spans")? else {
            break;
        };
        let bytes = span.span.end() - span.span.start();
        let total = &mut totals[match span.classification {
            LogicalClassification::NamedOpaque { .. } => 0,
            LogicalClassification::Structural => 1,
            LogicalClassification::Typed { .. } => 2,
        }];
        *total = Some(total.unwrap_or(0).checked_add(bytes).ok_or_else(overflow)?);
        groups_storage.with_storage(|| {
            if matches!(
                span.classification,
                LogicalClassification::NamedOpaque { .. }
            ) {
                ctx.insert_btree_set(
                    &mut opaque_names,
                    span.entry.as_str(),
                    "FCStd opaque coverage entries",
                )?;
            }
            let spans = match ctx.entry_btree_map(
                &mut spans_by_entry,
                span.entry.as_str(),
                "FCStd entry logical spans",
            )? {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(ScopedData {
                    data: Vec::new(),
                    _storage: ctx.reserve_scoped(0, "FCStd entry logical spans")?,
                }),
            };
            ctx.push_scoped_vec(
                &mut spans._storage,
                &mut spans.data,
                span,
                "FCStd entry logical spans",
            )
        })?;
    }
    // At most three literal names: the map is constant-sized.
    let classification_bytes = ["named_opaque", "structural", "typed"]
        .into_iter()
        .zip(totals)
        .filter_map(|(name, total)| Some((name.to_owned(), total?)))
        .collect::<BTreeMap<_, _>>();
    let physical_exact = {
        let (data, storage) = ctx.temporary_vec(physical.len(), "FCStd ordered physical spans")?;
        let mut ordered_physical = ScopedData {
            data,
            _storage: storage,
        };
        ordered_physical
            .data
            .extend(ctx.admit_iter(physical, "FCStd ordered physical spans")?);
        ctx.stable_sort_by_key(
            &mut ordered_physical.data,
            |value| value.span.start(),
            Ord::cmp,
            "FCStd physical span sort",
        )?;
        chain_is_exact(
            ctx,
            ordered_physical.data.iter().map(|span| &span.span),
            physical_byte_len,
            "FCStd physical span chain",
        )?
    };
    let mut logical_exact = true;
    let mut logical_byte_len = 0_u64;
    let mut entries_iter = entries.iter();
    while entries_iter.len() != 0 {
        let Some(entry) = ctx.next_charged(&mut entries_iter, "FCStd entry logical spans")? else {
            break;
        };
        logical_byte_len = logical_byte_len
            .checked_add(entry.byte_len())
            .ok_or_else(overflow)?;
        if !logical_exact {
            continue;
        }
        let spans = ctx.remove_btree_map(
            &mut spans_by_entry,
            entry.name(),
            "FCStd entry logical spans",
        )?;
        let mut spans = match spans {
            Some(spans) => spans,
            None => ScopedData {
                data: Vec::new(),
                _storage: ctx.reserve_scoped(0, "FCStd entry logical spans")?,
            },
        };
        ctx.stable_sort_by_key(
            &mut spans.data,
            |value| value.span.start(),
            Ord::cmp,
            "FCStd entry logical span sort",
        )?;
        logical_exact = if entry.byte_len() == 0 {
            spans.data.is_empty()
        } else {
            chain_is_exact(
                ctx,
                spans.data.iter().map(|span| &span.span),
                entry.byte_len(),
                "FCStd entry logical span chain",
            )?
        };
    }
    // A span naming no entry leaves its group behind.
    logical_exact &= spans_by_entry.is_empty();
    let mut opaque_entries =
        ctx.collection_vec(opaque_names.len(), "FCStd opaque coverage list")?;
    let mut names = opaque_names.iter();
    while names.len() != 0 {
        let Some(name) = ctx.next_charged(&mut names, "FCStd opaque coverage list")? else {
            break;
        };
        opaque_entries.push(ctx.copy_retained_text(name, "FCStd opaque entry name")?);
    }
    Ok(ByteCoverageRecord {
        id: crate::native::native_id("byte-coverage", "0"),
        physical_byte_len,
        physical_span_count: physical.len(),
        logical_entry_count: entries.len(),
        logical_byte_len,
        logical_span_count: logical.len(),
        classification_bytes,
        named_opaque_entries: opaque_entries,
        exact: physical_exact && logical_exact,
    })
}

/// Whether spans ordered by start tile `0..end` with no gap or overlap,
/// stepping until the first break.
pub(crate) fn chain_is_exact<'a>(
    ctx: &DecodeContext<'_>,
    mut spans: impl Iterator<Item = &'a crate::native::ByteSpan>,
    end: u64,
    operation: &'static str,
) -> Result<bool, CodecError> {
    let mut cursor = 0_u64;
    let mut empty = true;
    loop {
        if spans.size_hint().1 == Some(0) {
            ctx.charge_work(0, operation)?;
            break;
        }
        let Some(span) = ctx.next_charged(&mut spans, operation)? else {
            break;
        };
        if span.start() != cursor {
            return Ok(false);
        }
        cursor = span.end();
        empty = false;
    }
    Ok(!empty && cursor == end)
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
    let id = ctx.format_retained(
        format_args!("fcstd:native:logical-span#{}", output.len()),
        "FCStd logical span identity",
    )?;
    output.push(LogicalSpan {
        id,
        entry: ctx.copy_retained_text(entry.name(), "FCStd logical span entry")?,
        span: crate::native::ByteSpan::try_new(start, end).map_err(CodecError::Malformed)?,
        classification,
    });
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
