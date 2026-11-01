// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning.

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::native::{DocumentFacts, EntryRecord, PropertyRecord};

pub(crate) mod parts;

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

/// Fully scanned container used by inspection and decode.
pub(crate) struct Scan<'a> {
    /// Recoverable container metadata diagnostics.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Container summary entries.
    pub(crate) entries: Vec<ContainerEntry>,
    /// Persistence metadata.
    pub(crate) document: DocumentFacts,
    /// Declared persistence schema version, owned by the source declaration.
    pub(crate) schema_version: String,
    /// One admitted persistence tree shared by all document readers.
    pub(crate) document_xml: roxmltree::Document<'a>,
    pub(crate) document_properties: Option<crate::persistence::document_properties::RootProperties>,
    pub(crate) object_declarations: Option<crate::persistence::object_parts::ObjectDeclarations>,
    /// Independently framed members given up by the shared part helper.
    pub(crate) failed_members: BTreeSet<String>,
    /// Inflated entry views, each retaining its [`SpaceId`](cadmpeg_core::decode::SpaceId).
    pub(crate) data: BTreeMap<String, View<'a>>,
}

pub(crate) enum ScanOutcome<'a> {
    Readable {
        scan: Box<Scan<'a>>,
        parts: cadmpeg_ir::source_fidelity::parts::PartOutput,
    },
    GivenUp {
        summary: Box<ContainerSummary>,
        parts: cadmpeg_ir::source_fidelity::parts::PartOutput,
        /// The `Document.xml` member opened, so the document class was located.
        document_found: bool,
        /// The archive part itself gave up and retained the whole source.
        archive_given_up: bool,
    },
}

pub(crate) fn inspect(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<ContainerSummary, CodecError> {
    match attempt(ctx, root)? {
        ScanOutcome::Readable { scan, parts } => {
            let mut summary = summarize(&scan);
            summary.losses.extend(parts.into_parts().3);
            Ok(summary)
        }
        ScanOutcome::GivenUp {
            mut summary, parts, ..
        } => {
            summary.losses.extend(parts.into_parts().3);
            Ok(*summary)
        }
    }
}

#[cfg(test)]
pub(crate) fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let (mut scan, parts) = scan_for_decode(ctx, root)?;
    scan.losses.extend(parts.into_parts().3);
    Ok(scan)
}

#[cfg(test)]
pub(crate) fn scan_for_decode<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<(Scan<'a>, cadmpeg_ir::source_fidelity::parts::PartOutput), CodecError> {
    match attempt(ctx, root)? {
        ScanOutcome::Readable { scan, parts } => Ok((*scan, parts)),
        ScanOutcome::GivenUp { .. } => Err(CodecError::malformed("FCStd archive was given up")),
    }
}

pub(crate) fn attempt<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ScanOutcome<'a>, CodecError> {
    use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
    let archive = ArchiveSnapshot::new(ctx, root)?;
    if archive.entry("Document.xml").is_none() {
        return Err(CodecError::WrongFormat(
            "ZIP has no root Document.xml".into(),
        ));
    }
    let mut summary = ContainerSummary::unclassified(
        cadmpeg_ir::CodecFormat::Fcstd,
        cadmpeg_ir::ContainerKind::Zip,
        archive.container_entries(classify),
        Vec::new(),
        Vec::new(),
    );
    let mut parts = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let mut document_found = false;
    let outcome = ctx.decode_part(&mut parts, root, parts::part("zip.archive"), |stage| {
        let document = parts::document(ctx, &archive, stage, &mut document_found)?;
        if let Some(document) = &document {
            let matched = crate::dialect::FcstdDialect::classify(&document.facts, &document.schema);
            summary = ContainerSummary::classified(
                cadmpeg_core::dialect::DialectLayers::of(matched.clone()),
                cadmpeg_ir::ContainerKind::Zip,
                std::mem::take(&mut summary.entries),
                crate::dialect::FcstdDialect::dialect_loss(&matched)
                    .into_iter()
                    .collect(),
                Vec::new(),
            );
        }
        let opened = parts::open_optional(ctx, &archive, stage)?;
        let Some(document) = document else {
            parts::keep_readable_entries(ctx, &archive, &opened, stage)?;
            return Ok(None);
        };
        scan_impl(&archive, document, opened).map(Some)
    })?;
    match outcome {
        Some(Some(scan)) => Ok(ScanOutcome::Readable {
            scan: Box::new(scan),
            parts,
        }),
        outcome => Ok(ScanOutcome::GivenUp {
            summary: Box::new(summary),
            parts,
            document_found,
            archive_given_up: outcome.is_none(),
        }),
    }
}

fn scan_impl<'a>(
    archive: &ArchiveSnapshot<'a>,
    document: parts::Document<'a>,
    mut opened: parts::OptionalMembers<'a>,
) -> Result<Scan<'a>, CodecError> {
    let parts::Document {
        view: document_view,
        xml: document_xml,
        facts: document,
        schema: schema_version,
        root_properties: document_properties,
        object_declarations,
        losses,
    } = document;
    let mut data = BTreeMap::new();
    for file in archive.entries() {
        let name = file.name.clone();
        if !crate::native::is_safe_entry_name(&name) {
            return Err(CodecError::malformed(format_args!(
                "unsafe ZIP entry path {name:?}"
            )));
        }
        let view = if file.name == "Document.xml" {
            document_view
        } else if let Some(view) = opened.data.remove(&file.name) {
            view
        } else if opened.failed.contains(&file.name) {
            continue;
        } else {
            return Err(CodecError::malformed(
                "ZIP member has no admitted bounded payload",
            ));
        };
        data.insert(name, view);
    }
    Ok(Scan {
        losses,
        entries: archive.container_entries(classify),
        document,
        schema_version,
        document_xml,
        document_properties,
        object_declarations,
        failed_members: opened.failed,
        data,
    })
}

pub(crate) fn entry_records(
    scan: &Scan<'_>,
    properties: &[PropertyRecord],
) -> Result<Vec<EntryRecord>, CodecError> {
    let mut records = Vec::new();
    let mut unreadable_names = HashSet::new();
    unreadable_names.extend(scan.failed_members.iter().map(String::as_str));
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
        if !scan.data.contains_key(&entry.name) {
            if unreadable_names.contains(entry.name.as_str()) {
                // Stored source-only bytes have no expanded-byte coverage.
                continue;
            }
            return Err(CodecError::malformed(format_args!(
                "entry {} disappeared after scan",
                entry.name
            )));
        }
        let owners = references
            .get(entry.name.as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut referenced_by = Vec::new();
        for &index in owners {
            referenced_by.push(properties[index].id.clone());
        }
        records.push(EntryRecord::new(
            crate::native::native_id("entry", &entry.name),
            entry.name.clone(),
            entry.role,
            referenced_by,
        )?);
    }
    Ok(records)
}

pub(crate) fn source_attributes(
    scan: &Scan<'_>,
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    let mut attributes = BTreeMap::new();
    for (key, value) in [
        ("document_root", scan.document.root_name.clone()),
        (
            "document_kind",
            scan.document.document_kind().as_str().to_owned(),
        ),
        ("application_domains", scan.document.domains.join(",")),
    ]
    .into_iter()
    .chain(
        scan.document
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
pub(crate) fn summarize(scan: &Scan<'_>) -> ContainerSummary {
    let matched = crate::dialect::FcstdDialect::classify(&scan.document, &scan.schema_version);
    let mut losses = Vec::new();
    for loss in scan
        .losses
        .iter()
        .chain(crate::dialect::FcstdDialect::dialect_loss(&matched).iter())
    {
        losses.push(loss.clone());
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
    ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched),
        cadmpeg_ir::ContainerKind::Zip,
        entries,
        losses,
        summary_notes(scan),
    )
}

/// Notes shared by inspect and decode without reclassifying host identity.
pub(crate) fn summary_notes(scan: &Scan<'_>) -> Vec<String> {
    let mut notes = Vec::new();
    notes.extend([
        format!("SchemaVersion={}", scan.schema_version),
        format!("FileVersion={}", scan.document.file_version.as_str()),
        format!("document root={}", scan.document.root_name),
        format!("document kind={}", scan.document.document_kind().as_str()),
        format!("object count={}", scan.document.object_count),
    ]);
    if let Some(version) = &scan.document.program_version {
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
#[cfg(test)]
pub(crate) fn parse_document(
    xml: &roxmltree::Document<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(DocumentFacts, String), CodecError> {
    parse_document_impl(xml, losses, None)
}

fn parse_document_impl(
    xml: &roxmltree::Document<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source: Option<&crate::persistence::source::Source<'_>>,
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
        source,
    )?
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
        source,
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
    root: roxmltree::Node<'_, '_>,
    canonical: &str,
    alias: &str,
    code: crate::loss::FreecadLossCode,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source: Option<&crate::persistence::source::Source<'_>>,
) -> Result<Option<String>, CodecError> {
    let value = root.attribute(canonical);
    if let Some(alias_value) = root.attribute(alias) {
        let message = if value.is_some_and(|value| value != alias_value) {
            format!("Document has conflicting {canonical} and {alias} declarations; retaining canonical {canonical}")
        } else {
            format!("Document uses {alias} metadata; read as {canonical}")
        };
        losses.push(code.note(message));
    }
    let Some(attribute) = root.attributes().find(|attribute| {
        attribute.namespace().is_none()
            && attribute.name() == if value.is_some() { canonical } else { alias }
    }) else {
        return Ok(None);
    };
    match source
        .filter(|_| canonical != "FileVersion" || attribute.value().parse::<usize>().is_ok())
    {
        Some(source) => source.attribute(&attribute).map(Some),
        None => Ok(Some(attribute.value().to_owned())),
    }
}

#[cfg(test)]
pub(crate) mod tests;
