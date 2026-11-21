// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, Part};
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
            let Some(ref output) = inflated else {
                return Ok(false);
            };
            output.as_slice()
        }
        _ => return Ok(false),
    };
    Ok(contains(document, b"<Document") && contains(document, b"SchemaVersion"))
}

/// The persistence document admitted from `Document.xml`.
pub(crate) struct Persistence<'a> {
    /// Persistence metadata.
    pub(crate) document: DocumentFacts,
    /// Declared persistence schema version, owned by the source declaration.
    pub(crate) schema_version: String,
    /// One admitted persistence tree shared by all document readers.
    pub(crate) xml: roxmltree::Document<'a>,
}

/// An archive entry that could not be opened. It is given up once the
/// records that need it are known.
pub(crate) struct UnreadableEntry<'a> {
    /// The archive entry name.
    pub(crate) name: String,
    part: Part<'a>,
    error: CodecError,
}

impl UnreadableEntry<'_> {
    /// Gives the entry up, naming the records dropped because they need it.
    pub(crate) fn give_up(
        self,
        ctx: &DecodeContext<'_>,
        drops: Option<&crate::persistence::UnreadableDrops>,
    ) -> Result<GivenUpPart, CodecError> {
        let mut part = self.part;
        if let Some(drops) = drops {
            let records = drops
                .records
                .iter()
                .map(|id| Identity::new(id.clone()).map_err(CodecError::malformed))
                .collect::<Result<Vec<_>, _>>()?;
            part = part.dropping(records);
        }
        ctx.give_up_part(part, self.error)
    }
}

/// Fully scanned container used by inspection and decode.
pub(crate) struct Scan<'a> {
    /// Recoverable container metadata diagnostics.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Container summary entries.
    pub(crate) entries: Vec<ContainerEntry>,
    /// The persistence document, or `None` when `Document.xml` was given up.
    pub(crate) persistence: Option<Persistence<'a>>,
    /// Exact physical archive partition.
    pub(crate) ledger: Vec<ArchiveSpan>,
    /// Archive entries that could not be opened, to be given up as parts.
    pub(crate) unreadable: Vec<UnreadableEntry<'a>>,
    /// Names of the given-up archive entries.
    pub(crate) unreadable_names: BTreeSet<String>,
    /// Inflated entry views, each retaining its [`SpaceId`](cadmpeg_core::decode::SpaceId).
    pub(crate) data: BTreeMap<String, View<'a>>,
}

/// Scan an archive through the session resource budget.
pub(crate) fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let archive = ArchiveSnapshot::new(ctx, root)?;
    if !archive
        .entries()
        .iter()
        .any(|file| file.name == "Document.xml")
    {
        return Err(CodecError::WrongFormat(
            "ZIP has no root Document.xml".into(),
        ));
    }
    let mut losses = Vec::new();
    let mut persistence = None;
    // Opening the entry is the part; a Part refusal while admitting its tree
    // gives the part up too. A declaration that does not admit refuses the
    // file.
    let opened = ctx.run_part(|| {
        let view = archive.open(ctx, "Document.xml")?;
        Ok(admit_persistence(ctx, view))
    })?;
    let mut document = Some(match opened {
        Ok(admitted) => {
            let (view, admitted, metadata) = admitted?;
            persistence = Some(admitted);
            losses.extend(metadata);
            Ok(view)
        }
        Err(error) => Err(error),
    });
    let physical_ledger = archive.physical_ledger()?;
    let mut ledger = Vec::new();
    for (index, span) in physical_ledger.into_iter().enumerate() {
        ledger.push(ArchiveSpan {
            id: crate::native::native_id("archive-span", index.to_string()),
            span: crate::native::ByteSpan::try_new(span.start, span.end)
                .map_err(CodecError::Malformed)?,
            role: crate::native::ArchiveSpanRole::from(&span.role),
        });
    }
    let mut data = BTreeMap::new();
    let mut unreadable = Vec::new();
    let mut unreadable_names = BTreeSet::new();
    for file in archive.entries() {
        let name = file.name.clone();
        if !crate::native::is_safe_entry_name(&name) {
            return Err(CodecError::malformed(format_args!(
                "unsafe ZIP entry path {name:?}"
            )));
        }
        let opened = if name == "Document.xml" {
            document
                .take()
                .ok_or_else(|| CodecError::malformed("ZIP has two Document.xml entries"))?
        } else {
            ctx.run_part(|| archive.open(ctx, &name))?
        };
        let error = match opened {
            Ok(view) => {
                data.insert(name, view);
                continue;
            }
            Err(error) => error,
        };
        let range = file.stored_range()?;
        let stored = usize::try_from(range.start)
            .ok()
            .zip(usize::try_from(range.end).ok())
            .and_then(|(start, end)| root.child(start, end))
            .ok_or_else(|| CodecError::malformed("ZIP payload escapes archive"))?;
        // The ledger span of the stored payload names the entry; an empty
        // payload has no span and belongs to the document, or with no
        // document, to the span of its name.
        let span_of = |payload: bool| {
            ledger
                .iter()
                .find(|span| match &span.role {
                    crate::native::ArchiveSpanRole::CompressedPayload(entry) => {
                        payload && *entry == name
                    }
                    crate::native::ArchiveSpanRole::LocalName(entry) => !payload && *entry == name,
                    _ => false,
                })
                .map(|span| span.id.clone())
        };
        let containing = span_of(true)
            .or_else(|| {
                persistence
                    .is_some()
                    .then(|| crate::native::native_id("document", "0"))
            })
            .or_else(|| span_of(false));
        let mut part = Part::new(
            crate::dialect::FORMAT,
            "archive-entry",
            stored,
            held_classes(classify(&name)).iter().copied(),
        )
        .named(name.as_str());
        if let Some(containing) = containing {
            part = part.contained_in(Identity::new(containing).map_err(CodecError::malformed)?);
        }
        unreadable_names.insert(name.clone());
        unreadable.push(UnreadableEntry { name, part, error });
    }
    Ok(Scan {
        losses,
        entries: archive.container_entries(classify),
        persistence,
        unreadable,
        unreadable_names,
        ledger,
        data,
    })
}

/// Admits the opened `Document.xml`, returning its view, the persistence
/// document, and the metadata losses its declaration charges.
fn admit_persistence<'a>(
    ctx: &DecodeContext<'_>,
    view: View<'a>,
) -> Result<
    (
        View<'a>,
        Persistence<'a>,
        Vec<cadmpeg_ir::report::loss::LossNote>,
    ),
    CodecError,
> {
    let bytes = view.window();
    if let Some((_, object_count)) = xml_envelope_counts(bytes) {
        ctx.charge_entities(object_count, "admit FCStd document objects")?;
    }
    let xml = admit_document(ctx, bytes)?;
    let mut losses = Vec::new();
    let (document, schema_version) = parse_document(&xml, &mut losses)?;
    Ok((
        view,
        Persistence {
            document,
            schema_version,
            xml,
        },
        losses,
    ))
}

/// The content classes an entry of `role` can hold.
fn held_classes(role: ContainerRole) -> &'static [ArenaClass] {
    match role {
        ContainerRole::Directory => &[],
        ContainerRole::Brep | ContainerRole::GuiDocument | ContainerRole::Thumbnail => {
            &[ArenaClass::Model]
        }
        _ => &ArenaClass::ALL,
    }
}

pub(crate) fn entry_records(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    properties: &[PropertyRecord],
) -> Result<Vec<EntryRecord>, CodecError> {
    let mut records = Vec::new();
    let references = {
        let mut references = HashMap::<&str, Vec<usize>>::new();
        for (index, property) in properties.iter().enumerate() {
            for name in property.side_entries() {
                let owners = references.entry(name.as_str()).or_default();
                // Repeated side-entry markers in one property yield one owner.
                if owners.last() != Some(&index) {
                    owners.push(index);
                }
            }
        }
        Ok::<_, CodecError>(references)
    }?;
    for entry in &scan.entries {
        let Some(bytes) = scan.data.get(&entry.name).map(|view| view.window()) else {
            if scan.unreadable_names.contains(&entry.name) {
                // Stored source-only bytes have no expanded-byte coverage.
                continue;
            }
            return Err(CodecError::malformed(format_args!(
                "entry {} disappeared after scan",
                entry.name
            )));
        };
        let owners = references
            .get(entry.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut referenced_by = Vec::new();
        for &index in owners {
            referenced_by.push(properties[index].id.clone());
        }
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "retain FCStd entry",
        )?;
        records.push(EntryRecord::new(
            crate::native::native_id("entry", &entry.name),
            entry.name.clone(),
            entry.role,
            referenced_by,
            bytes.to_vec(),
        )?);
    }
    Ok(records)
}

pub(crate) fn source_attributes(
    document: &DocumentFacts,
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    let mut attributes = BTreeMap::new();
    for (key, value) in [
        ("document_root", document.root_name.clone()),
        (
            "document_kind",
            document.document_kind().as_str().to_owned(),
        ),
        ("application_domains", document.domains.join(",")),
    ]
    .into_iter()
    .chain(
        document
            .program_version
            .as_ref()
            .map(|value| Ok::<_, CodecError>(("program_version", value.to_owned())))
            .transpose()?,
    ) {
        let key = NonBlankString::new(key.to_owned())
            .ok_or_else(|| CodecError::malformed("source attribute key is empty"))?;
        attributes.insert(key, value);
    }
    Ok(attributes)
}

/// Summarize one scan.
pub(crate) fn summarize(
    ctx: &DecodeContext<'_>,
    mut scan: Scan<'_>,
) -> Result<ContainerSummary, CodecError> {
    let given_up = std::mem::take(&mut scan.unreadable)
        .into_iter()
        .map(|entry| entry.give_up(ctx, None))
        .collect::<Result<Vec<_>, _>>()?;
    let scan = &scan;
    let matched = scan.persistence.as_ref().map(|persistence| {
        crate::dialect::FcstdDialect::classify(&persistence.document, &persistence.schema_version)
    });
    let mut losses = Vec::new();
    for loss in scan
        .losses
        .iter()
        .cloned()
        .chain(given_up.iter().map(GivenUpPart::loss))
        .chain(
            matched
                .as_ref()
                .and_then(crate::dialect::FcstdDialect::dialect_loss),
        )
    {
        losses.push(loss);
    }
    let mut entries = Vec::new();
    for entry in &scan.entries {
        let mut attributes = BTreeMap::new();
        for (key, value) in &entry.attributes {
            attributes.insert(key.to_owned(), value.to_owned());
        }
        entries.push(ContainerEntry {
            name: entry.name.clone(),
            role: entry.role,
            storage: entry.storage.clone(),
            attributes,
        });
    }
    let kind = cadmpeg_ir::ContainerKind::Zip;
    let notes = summary_notes(scan);
    Ok(match matched {
        Some(matched) => ContainerSummary::classified(
            cadmpeg_core::dialect::DialectLayers::of(matched),
            kind,
            entries,
            losses,
            notes,
        ),
        None => {
            ContainerSummary::unclassified(crate::dialect::FORMAT, kind, entries, losses, notes)
        }
    })
}

/// Notes shared by inspect and decode without reclassifying host identity.
pub(crate) fn summary_notes(scan: &Scan<'_>) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(Persistence {
        document,
        schema_version,
        ..
    }) = &scan.persistence
    {
        notes.extend([
            format!("SchemaVersion={schema_version}"),
            format!("FileVersion={}", document.file_version.as_str()),
            format!("document root={}", document.root_name),
            format!("document kind={}", document.document_kind().as_str()),
            format!("object count={}", document.object_count),
        ]);
    }
    notes.push(format!(
        "physical ledger spans={} coverage=exact",
        scan.ledger.len()
    ));
    if let Some(version) = scan
        .persistence
        .as_ref()
        .and_then(|persistence| persistence.document.program_version.as_ref())
    {
        notes.push(format!("ProgramVersion={version}"));
    }
    notes
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

/// Parse the persistence tree once, charging its nesting depth.
pub(crate) fn admit_document<'input>(
    ctx: &DecodeContext<'_>,
    bytes: &'input [u8],
) -> Result<roxmltree::Document<'input>, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("Document.xml is not UTF-8".into()))?;
    let admitted_xml = match ctx.parse_xml(text, "FreeCAD document XML tree") {
        Ok(xml) => xml,
        Err(error) => {
            let CodecError::Malformed(error) = error else {
                return Err(error);
            };
            return Err(CodecError::malformed(format_args!(
                "invalid Document.xml: {error}"
            )));
        }
    };
    Ok(admitted_xml)
}

/// Read container metadata from the admitted persistence tree.
pub(crate) fn parse_document(
    xml: &roxmltree::Document<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(DocumentFacts, String), CodecError> {
    let root = xml.root_element();
    if root.tag_name().name() != "Document" {
        return Err(CodecError::WrongFormat(format!(
            "Document.xml root is {}, expected Document",
            root.tag_name().name()
        )));
    }
    let schema_version = canonical_attribute(root, "SchemaVersion", "schemaVersion")?
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no SchemaVersion".into()))?;
    let file_version = metadata_attribute(
        root,
        "FileVersion",
        "fileVersion",
        crate::loss::FreecadLossCode::FileVersionNoncanonical,
        losses,
    )
    .unwrap_or_else(|| "0".into());
    let vocabulary = crate::persistence::Vocabulary::from_declaration(&schema_version)?;
    let file_version = crate::native::FileVersion::from(file_version);
    if file_version.value().is_none() {
        losses.push(crate::loss::FreecadLossCode::FileVersionUnverified.note(
            format!("Document FileVersion {:?} is not an unsigned integer; version-dependent inline element maps remain source-only", file_version.as_str())
        ));
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
                domain_set.insert(domain.to_owned());
            }
        }
    }
    let mut domains = Vec::new();
    domains.extend(domain_set);
    let program_version = metadata_attribute(
        root,
        "ProgramVersion",
        "programVersion",
        crate::loss::FreecadLossCode::ProgramVersionNoncanonical,
        losses,
    );
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
    root: roxmltree::Node<'_, '_>,
    canonical: &str,
    alias: &str,
    code: crate::loss::FreecadLossCode,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<String> {
    let value = root.attribute(canonical);
    if let Some(alias_value) = root.attribute(alias) {
        let message = if value.is_some_and(|value| value != alias_value) {
            format!("Document has conflicting {canonical} and {alias} declarations; retaining canonical {canonical}")
        } else {
            format!("Document uses {alias} metadata; read as {canonical}")
        };
        losses.push(code.note(message));
    }
    value.or_else(|| root.attribute(alias)).map(str::to_owned)
}

pub(crate) fn logical_ledger(
    entries: &[EntryRecord],
    properties: &[PropertyRecord],
    gui: &gui::Graph,
    shape_payloads: &[ShapePayloadRecord],
    string_tables: &[StringTableRecord],
    element_maps: &[ElementMapRecord],
) -> Result<Vec<LogicalSpan>, CodecError> {
    let mut typed_entries = HashSet::new();
    for payload in shape_payloads {
        typed_entries.insert(payload.entry.as_str());
    }
    for name in string_tables
        .iter()
        .filter_map(|table| table.source_entry.as_deref())
    {
        typed_entries.insert(name);
    }
    for name in element_maps
        .iter()
        .filter_map(|map| map.source_entry.as_deref())
    {
        typed_entries.insert(name);
    }
    let mut output = Vec::new();
    for entry in entries {
        if typed_entries.contains(entry.id()) || typed_entries.contains(entry.name()) {
            push_logical_span(
                &mut output,
                entry,
                0,
                entry.byte_len(),
                LogicalClassification::Typed {
                    owner: entry.id().to_owned(),
                },
            )?;
        } else if entry.name() == "Document.xml" || entry.name() == "GuiDocument.xml" {
            let mut ranges = Vec::new();
            if entry.name() == "Document.xml" {
                for property in properties {
                    ranges.push((
                        property.xml.start(),
                        property.xml.end(),
                        if property.family == PropertyFamily::Unknown {
                            "named_opaque"
                        } else {
                            "typed"
                        },
                        property.id.clone(),
                    ));
                }
            } else {
                for property in &gui.properties {
                    ranges.push((
                        property.xml.start(),
                        property.xml.end(),
                        if gui::has_registered_property_grammar(&property.name, &property.type_name)
                        {
                            "typed"
                        } else {
                            "named_opaque"
                        },
                        property.id.clone(),
                    ));
                }
                for document in &gui.documents {
                    for state in &document.states {
                        ranges.push((
                            state.xml.start(),
                            state.xml.end(),
                            "typed",
                            state.id.clone(),
                        ));
                    }
                }
            }
            ranges.sort_by_key(|left| left.0);
            let mut cursor = 0_u64;
            for (start, end, classification, owner) in ranges {
                if start < cursor || end < start || end > entry.byte_len() {
                    return Err(CodecError::malformed(format_args!(
                        "overlapping or invalid {} record spans",
                        entry.name()
                    )));
                }
                push_logical_span(
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
                push_logical_span(&mut output, entry, start, end, classification)?;
                cursor = end;
            }
            push_logical_span(
                &mut output,
                entry,
                cursor,
                entry.byte_len(),
                LogicalClassification::Structural,
            )?;
        } else {
            push_logical_span(
                &mut output,
                entry,
                0,
                entry.byte_len(),
                LogicalClassification::NamedOpaque {
                    owner: entry.id().to_owned(),
                },
            )?;
        }
    }
    Ok(output)
}

/// The logical partition of entries no persistence document reads: each
/// entry is one named opaque span.
pub(crate) fn opaque_ledger(entries: &[EntryRecord]) -> Result<Vec<LogicalSpan>, CodecError> {
    let mut output = Vec::new();
    for entry in entries {
        push_logical_span(
            &mut output,
            entry,
            0,
            entry.byte_len(),
            LogicalClassification::NamedOpaque {
                owner: entry.id().to_owned(),
            },
        )?;
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
        let classification = span.classification.as_str();
        if !classification_bytes.contains_key(classification) {
            classification_bytes.insert(classification.to_owned(), 0);
        }
        if let Some(bytes) = classification_bytes.get_mut(classification) {
            *bytes += span.span.end() - span.span.start();
        }
        if matches!(
            span.classification,
            LogicalClassification::NamedOpaque { .. }
        ) && !named_opaque_entries.contains(&span.entry)
        {
            named_opaque_entries.insert(span.entry.clone());
        }
    }
    let mut ordered_physical = Vec::new();
    ordered_physical.extend(physical.iter());
    ordered_physical.sort_by_key(|left| left.span.start());
    let physical_exact = ordered_physical
        .first()
        .is_some_and(|span| span.span.start() == 0)
        && ordered_physical
            .windows(2)
            .all(|pair| pair[0].span.end() == pair[1].span.start())
        && ordered_physical
            .last()
            .is_some_and(|span| span.span.end() == physical_byte_len);
    let entry_names = entries
        .iter()
        .map(EntryRecord::name)
        .collect::<HashSet<_>>();
    let mut logical_exact = logical
        .iter()
        .all(|span| entry_names.contains(span.entry.as_str()));
    if logical_exact {
        let mut spans_by_entry = HashMap::<&str, Vec<&LogicalSpan>>::new();
        for span in logical {
            spans_by_entry
                .entry(span.entry.as_str())
                .or_default()
                .push(span);
        }
        for entry in entries {
            let mut spans = spans_by_entry
                .get(entry.name())
                .cloned()
                .unwrap_or_default();
            spans.sort_by_key(|left| left.span.start());
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
    let mut opaque_entries = Vec::new();
    opaque_entries.extend(named_opaque_entries);
    ByteCoverageRecord {
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
    }
}

fn push_logical_span(
    output: &mut Vec<LogicalSpan>,
    entry: &EntryRecord,
    start: u64,
    end: u64,
    classification: LogicalClassification,
) -> Result<(), CodecError> {
    if start == end {
        return Ok(());
    }
    output.push(LogicalSpan {
        id: crate::native::native_id("logical-span", output.len().to_string()),
        entry: entry.name().to_owned(),
        span: crate::native::ByteSpan::try_new(start, end).map_err(CodecError::Malformed)?,
        classification,
    });
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
