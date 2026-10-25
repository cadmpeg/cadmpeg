// SPDX-License-Identifier: Apache-2.0
//! Bounded `FCStd` archive scanning and physical byte accounting.

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::brep::ShapePayloadRecord;
use crate::gui;
use crate::native::element_map::ElementMapRecord;
use crate::native::{
    DocumentFacts, EntryRecord, PropertyFamily, PropertyRecord, StringTableRecord,
};
use cadmpeg_ir::source_fidelity::freecad::{
    ArchiveSpan, ArchiveSpanRole, ByteCoverageRecord, LogicalClassification, LogicalSpan,
};

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
    /// Actual source archive retained transiently for entry-origin framing.
    pub(crate) archive: std::rc::Rc<ArchiveSnapshot<'a>>,
    /// Recoverable container metadata diagnostics.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Container summary entries.
    pub(crate) entries: Vec<ContainerEntry>,
    /// Persistence metadata.
    pub(crate) document: DocumentFacts,
    /// Declared persistence schema version, owned by the source declaration.
    pub(crate) schema_version: String,
    pub(crate) source_whitespace: Vec<View<'a>>,
    pub(crate) entry_records: Vec<crate::native::EntryRecord>,
    pub(crate) entry_owners: parts::EntryOwners<'a>,
    pub(crate) entry_parents: BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>,
    pub(crate) document_properties: Option<crate::persistence::document_properties::RootProperties>,
    pub(crate) object_declarations:
        Option<crate::persistence::object_parts::ObjectDeclarations<'a>>,
    /// Exact physical archive partition.
    pub(crate) ledger: Vec<ArchiveSpan>,
    /// Inflated entry views, each retaining its [`SpaceId`](cadmpeg_core::decode::SpaceId).
    pub(crate) data: BTreeMap<String, View<'a>>,
}

pub(crate) enum ScanOutcome<'a> {
    Readable {
        scan: Scan<'a>,
        parts: cadmpeg_ir::source_fidelity::parts::PartOutput,
    },
    GivenUp {
        summary: ContainerSummary,
        parts: cadmpeg_ir::source_fidelity::parts::PartOutput,
        pointers: BTreeMap<String, BTreeMap<String, u64>>,
        source: Option<crate::archive_recovery::Source<'a>>,
    },
}

pub(crate) fn located_scope(
    depth: cadmpeg_core::decode::DecodeDepth,
    fidelity: &cadmpeg_ir::SourceFidelity,
) -> cadmpeg_ir::ContentScope {
    let document_found = fidelity.parts().values().any(|part| {
        matches!(
            part.level().as_str(),
            "freecad.document_xml" | "freecad.document"
        )
    });
    let gui_found = fidelity.parts().values().any(|part| {
        matches!(
            part.level().as_str(),
            "freecad.gui_document_xml" | "freecad.gui_document"
        )
    });
    cadmpeg_ir::ContentScope {
        model: (document_found || gui_found)
            && depth != cadmpeg_core::decode::DecodeDepth::Container,
        design: document_found && depth == cadmpeg_core::decode::DecodeDepth::Full,
    }
}

#[cfg(test)]
pub(crate) fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let (mut scan, parts) = scan_for_decode(ctx, root)?;
    scan.losses.extend(parts.into_parts(ctx)?.into_parts().3);
    Ok(scan)
}

#[cfg(test)]
pub(crate) fn scan_for_decode<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<(Scan<'a>, cadmpeg_ir::source_fidelity::parts::PartOutput), CodecError> {
    match attempt(ctx, root)? {
        ScanOutcome::Readable {
            mut scan,
            mut parts,
        } => {
            let updates = scan
                .object_declarations
                .as_mut()
                .map(|decl| std::mem::take(&mut decl.entry_contents))
                .unwrap_or_default();
            parts::merge_entry_contents(ctx, &mut scan.entry_records, updates)?;
            parts::stage_entries(
                ctx,
                scan.entry_records.clone(),
                &[],
                &gui::Graph::default(),
                &mut parts,
            )?;
            if !ctx.container_only() {
                if let Some(properties) = &scan.document_properties {
                    let document = scan.data.get("Document.xml").copied().ok_or_else(|| {
                        CodecError::malformed("attribute projection has no source document")
                    })?;
                    crate::test_support::source_fidelity::project_document_attributes(
                        ctx, document, properties, &mut parts,
                    );
                }
            }
            Ok((scan, parts))
        }
        ScanOutcome::GivenUp { .. } => Err(CodecError::malformed("FCStd archive was given up")),
    }
}

pub(crate) fn attempt<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ScanOutcome<'a>, CodecError> {
    use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
    let archive = parts::archive_source(ctx, root)?;
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
    let outcome = ctx.decode_part(&mut parts, root, parts::archive_spec()?, |stage| {
        let mut opened = parts::open_optional(ctx, &archive, stage)?;
        parts::incomplete_local_header(ctx, &archive, stage)?;
        let parts::DocumentRead {
            record,
            document,
            view,
            mut pointers,
        } = parts::document(
            ctx,
            &archive,
            stage,
            &opened.data,
            &opened.parents,
            &opened.records,
            &mut opened.owners,
        )?;
        if let Some(record) = record {
            opened.records.push(record);
        }
        if let Some(view) = view {
            opened.data.insert("Document.xml".to_owned(), view);
        }
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
        let Some(document) = document else {
            let declared_entries = parts::declared_entries(&opened.records);
            let (gui, source_whitespace, entry_interpretations) =
                if let Some(source) = opened.data.get("GuiDocument.xml") {
                    crate::gui::source::parse(ctx, *source)?.decode_with_whitespace(
                        ctx,
                        &crate::gui::GuiSources {
                            entries: &opened.data,
                            declared_entries: Some(&declared_entries),
                            declared_objects: Some(&std::collections::BTreeSet::new()),
                            admitted_objects: Some(&std::collections::BTreeSet::new()),
                            entry_parents: Some(&opened.parents),
                            entry_owners: Some(&opened.owners),
                            entry_interpretations: None,
                            gui_entry_interpretations: None,
                            objects: &[],
                            object_sources: None,
                            properties: &[],
                            payloads: &[],
                            topology_occurrences: &[],
                            requires_alpha_conversion: false,
                        },
                        stage,
                    )?
                } else {
                    (crate::gui::Graph::default(), Vec::new(), Default::default())
                };
            parts::opaque_members(ctx, &opened.data, &opened.parents, stage)?;
            pointers.extend(parts::keep_readable_entries(&archive, &opened)?);
            return Ok((
                None,
                pointers,
                Some(crate::archive_recovery::Source {
                    archive,
                    entries: opened.data,
                    entry_parents: opened.parents,
                    entry_records: opened.records,
                    entry_owners: opened.owners,
                    entry_interpretations,
                    gui,
                    source_whitespace,
                }),
            ));
        };
        Ok((
            Some(scan_impl(archive, document, opened)?),
            BTreeMap::new(),
            None,
        ))
    })?;
    match outcome {
        Some((Some(scan), _, _)) => Ok(ScanOutcome::Readable { scan, parts }),
        Some((None, pointers, source)) => Ok(ScanOutcome::GivenUp {
            summary,
            parts,
            pointers,
            source,
        }),
        None => Ok(ScanOutcome::GivenUp {
            summary,
            parts,
            pointers: BTreeMap::new(),
            source: None,
        }),
    }
}

fn scan_impl<'a>(
    archive: std::rc::Rc<ArchiveSnapshot<'a>>,
    document: parts::Document<'a>,
    mut opened: parts::OptionalMembers<'a>,
) -> Result<Scan<'a>, CodecError> {
    let parts::Document {
        view: document_view,
        facts: document,
        schema: schema_version,
        source_whitespace,
        root_properties: document_properties,
        object_declarations,
        losses,
    } = document;
    let mut data = BTreeMap::new();
    for file in archive.entries() {
        let name = file.name.clone();
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
    let physical_ledger = archive.physical_ledger()?;
    let mut ledger = Vec::new();
    for (index, span) in physical_ledger.into_iter().enumerate() {
        ledger.push(ArchiveSpan {
            id: crate::native::native_id("archive-span", index.to_string()),
            span: cadmpeg_ir::source_fidelity::freecad::ByteSpan::try_new(span.start, span.end)
                .map_err(CodecError::Malformed)?,
            role: archive_span_role(&span.role),
        });
    }
    Ok(Scan {
        losses,
        entries: archive.container_entries(classify),
        archive,
        document,
        schema_version,
        source_whitespace,
        entry_parents: opened.parents,
        entry_records: opened.records,
        entry_owners: opened.owners,
        document_properties,
        object_declarations,
        ledger,
        data,
    })
}

pub(crate) fn source_attributes(
    scan: &Scan<'_>,
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    let mut attributes = BTreeMap::new();
    for (key, value) in [
        ("document_root", scan.document.root_name.clone()),
        ("object_count", format!("{}", scan.document.object_count)),
        (
            "document_kind",
            scan.document.document_kind().as_str().to_owned(),
        ),
        ("application_domains", scan.document.domains.join(",")),
        ("archive_entry_count", format!("{}", scan.entries.len())),
        ("physical_ledger_spans", format!("{}", scan.ledger.len())),
    ]
    .into_iter()
    .chain(
        scan.ledger
            .last()
            .map(|last| {
                Ok::<_, CodecError>(("physical_archive_bytes", format!("{}", last.span.end())))
            })
            .transpose()?,
    )
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
        format!("physical ledger spans={} coverage=exact", scan.ledger.len()),
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

/// Read container metadata through the recovering source scanner.
#[cfg(test)]
pub(crate) fn parse_document(
    xml: &roxmltree::Document<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(DocumentFacts, String), CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(
        xml.input_text().as_bytes(),
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let source = crate::xml_framing::XmlSource::new(&ctx, view)?;
    let root = source
        .elements()
        .iter()
        .position(|element| element.parent().is_none())
        .ok_or_else(|| CodecError::malformed("Document.xml has no root"))?;
    document_facts(&ctx, &source, root, losses)
}

pub(super) fn document_facts(
    ctx: &DecodeContext<'_>,
    source: &crate::xml_framing::XmlSource<'_>,
    root: usize,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(DocumentFacts, String), CodecError> {
    let attributes = source.peek_attributes(ctx, root)?;
    if source.name(root)? != "Document" {
        return Err(CodecError::WrongFormat(
            "Document.xml root is not Document".into(),
        ));
    }
    if attributes.contains_key("schemaVersion") {
        return Err(CodecError::malformed(
            "Document uses unsupported schemaVersion metadata",
        ));
    }
    let schema = source
        .peek_attribute(ctx, root, "SchemaVersion")?
        .ok_or_else(|| CodecError::WrongFormat("Document.xml has no SchemaVersion".into()))?;
    let vocabulary = crate::persistence::Vocabulary::from_declaration(&schema)?;
    let mut metadata = |canonical,
                        alias,
                        code: crate::loss::FreecadLossCode|
     -> Result<Option<String>, CodecError> {
        if let Some(alias_value) = attributes.get(alias) {
            let message = if attributes
                .get(canonical)
                .is_some_and(|value| value != alias_value)
            {
                format!("Document has conflicting {canonical} and {alias} declarations; retaining canonical {canonical}")
            } else {
                format!("Document uses {alias} metadata; read as {canonical}")
            };
            losses.push(code.note(message));
        }
        source.attribute(
            ctx,
            root,
            if attributes.contains_key(canonical) {
                canonical
            } else {
                alias
            },
        )
    };
    let file_version = crate::native::FileVersion::from(
        metadata(
            "FileVersion",
            "fileVersion",
            crate::loss::FreecadLossCode::FileVersionNoncanonical,
        )?
        .unwrap_or_else(|| "0".into()),
    );
    let program_version = metadata(
        "ProgramVersion",
        "programVersion",
        crate::loss::FreecadLossCode::ProgramVersionNoncanonical,
    )?;
    if file_version.value().is_none() {
        losses.push(crate::loss::FreecadLossCode::FileVersionUnverified.note(format!("Document FileVersion {:?} is not an unsigned integer; version-dependent inline element maps remain source-only", file_version.as_str())));
    }
    let (objects, _, record) = vocabulary.tags();
    let mut object_count = 0;
    for section in source
        .children(root)
        .filter(|index| source.name(*index).is_ok_and(|name| name == objects))
    {
        object_count += source
            .children(section)
            .filter(|index| source.name(*index).is_ok_and(|name| name == record))
            .count();
    }
    Ok((
        DocumentFacts {
            id: crate::native::native_id("document", "0"),
            file_version,
            program_version,
            root_name: source.read_name(root)?.to_owned(),
            object_count,
            domains: Vec::new(),
        },
        schema,
    ))
}

pub(crate) fn logical_ledger(
    entries: &[EntryRecord],
    properties: &[PropertyRecord],
    property_sources: &BTreeMap<String, crate::fidelity::ValueSourceRanges>,
    sources: &crate::entry_sources::EntrySources<'_>,
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
    for entry in entries.iter().filter(|entry| sources.contains(entry)) {
        let entry_len = sources.byte_len(entry)?;
        if typed_entries.contains(entry.id()) || typed_entries.contains(entry.name()) {
            push_logical_span(
                &mut output,
                entry,
                0,
                entry_len,
                LogicalClassification::Typed {
                    owner: entry.id().to_owned(),
                },
            )?;
        } else if entry.name() == "Document.xml" || entry.name() == "GuiDocument.xml" {
            let mut ranges = Vec::new();
            if entry.name() == "Document.xml" {
                for property in properties {
                    let range = property_sources
                        .get(&property.id)
                        .ok_or_else(|| {
                            CodecError::malformed("property has no original source range")
                        })?
                        .record_range();
                    ranges.push((
                        cadmpeg_core::decode::u64_from_index(range.start),
                        cadmpeg_core::decode::u64_from_index(range.end),
                        if property.body.is_none() || property.family == PropertyFamily::Unknown {
                            "named_opaque"
                        } else {
                            "typed"
                        },
                        property.id.clone(),
                    ));
                }
            } else {
                for property in &gui.properties {
                    let source = gui.property_sources.get(&property.id).ok_or_else(|| {
                        CodecError::malformed("GUI property has no original source range")
                    })?;
                    let range = source.record_range();
                    ranges.push((
                        cadmpeg_core::decode::u64_from_index(range.start),
                        cadmpeg_core::decode::u64_from_index(range.end),
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
                        let source = gui.state_sources.get(&state.id).ok_or_else(|| {
                            CodecError::malformed("GUI state has no original source range")
                        })?;
                        let range = source.record_range();
                        ranges.push((
                            cadmpeg_core::decode::u64_from_index(range.start),
                            cadmpeg_core::decode::u64_from_index(range.end),
                            "typed",
                            document.id.clone(),
                        ));
                    }
                }
            }
            ranges.sort_by_key(|left| left.0);
            let mut cursor = 0_u64;
            for (start, end, classification, owner) in ranges {
                if start < cursor || end < start || end > entry_len {
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
                entry_len,
                LogicalClassification::Structural,
            )?;
        } else {
            push_logical_span(
                &mut output,
                entry,
                0,
                entry_len,
                LogicalClassification::NamedOpaque {
                    owner: entry.id().to_owned(),
                },
            )?;
        }
    }
    Ok(output)
}

pub(crate) fn byte_coverage(
    physical: &[ArchiveSpan],
    entries: &[EntryRecord],
    sources: &crate::entry_sources::EntrySources<'_>,
    logical: &[LogicalSpan],
    physical_byte_len: u64,
) -> Result<ByteCoverageRecord, CodecError> {
    let logical_byte_len = entries
        .iter()
        .filter(|entry| sources.contains(entry))
        .try_fold(0_u64, |total, entry| {
            total
                .checked_add(sources.byte_len(entry)?)
                .ok_or_else(|| CodecError::malformed("logical entry byte length sum overflows u64"))
        })?;
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
    let mut logical_exact = logical
        .iter()
        .all(|span| entries.iter().any(|entry| entry.name() == span.entry));
    if logical_exact {
        for entry in entries.iter().filter(|entry| sources.contains(entry)) {
            let entry_len = sources.byte_len(entry)?;
            let mut spans = Vec::new();
            for span in logical.iter().filter(|span| span.entry == entry.name()) {
                spans.push(span);
            }
            spans.sort_by_key(|left| left.span.start());
            let exact = if entry_len == 0 {
                spans.is_empty()
            } else {
                spans.first().is_some_and(|span| span.span.start() == 0)
                    && spans
                        .windows(2)
                        .all(|pair| pair[0].span.end() == pair[1].span.start())
                    && spans
                        .last()
                        .is_some_and(|span| span.span.end() == entry_len)
            };
            if !exact {
                logical_exact = false;
                break;
            }
        }
    }
    let mut opaque_entries = Vec::new();
    opaque_entries.extend(named_opaque_entries);
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
        span: cadmpeg_ir::source_fidelity::freecad::ByteSpan::try_new(start, end)
            .map_err(CodecError::Malformed)?,
        classification,
    });
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;

fn archive_span_role(role: &cadmpeg_container::ZipSpanRole) -> ArchiveSpanRole {
    use cadmpeg_container::ZipSpanRole;
    match role {
        ZipSpanRole::LocalSignature(entry) => ArchiveSpanRole::LocalSignature(entry.clone()),
        ZipSpanRole::LocalFields(entry) => ArchiveSpanRole::LocalFields(entry.clone()),
        ZipSpanRole::LocalName(entry) => ArchiveSpanRole::LocalName(entry.clone()),
        ZipSpanRole::LocalExtra(entry) => ArchiveSpanRole::LocalExtra(entry.clone()),
        ZipSpanRole::CompressedPayload(entry) => ArchiveSpanRole::CompressedPayload(entry.clone()),
        ZipSpanRole::DataDescriptor(entry) => ArchiveSpanRole::DataDescriptor(entry.clone()),
        ZipSpanRole::CentralSignature(entry) => ArchiveSpanRole::CentralSignature(entry.clone()),
        ZipSpanRole::CentralFields(entry) => ArchiveSpanRole::CentralFields(entry.clone()),
        ZipSpanRole::CentralName(entry) => ArchiveSpanRole::CentralName(entry.clone()),
        ZipSpanRole::CentralExtra(entry) => ArchiveSpanRole::CentralExtra(entry.clone()),
        ZipSpanRole::CentralComment(entry) => ArchiveSpanRole::CentralComment(entry.clone()),
        ZipSpanRole::Padding { entry: Some(entry) } => {
            ArchiveSpanRole::EntryArchivePadding(entry.clone())
        }
        ZipSpanRole::Padding { entry: None } => ArchiveSpanRole::ArchivePadding,
        ZipSpanRole::Zip64EndRecord => ArchiveSpanRole::Zip64EndRecord,
        ZipSpanRole::Zip64EndLocator => ArchiveSpanRole::Zip64EndLocator,
        ZipSpanRole::EndRecord => ArchiveSpanRole::EndRecord,
    }
}
